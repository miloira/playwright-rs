//! The connection to the Playwright driver.
//!
//! A [`Connection`] owns the driver process, frames messages over its stdio,
//! correlates command responses by id, and maintains the registry of remote
//! objects that the driver creates via `__create__` events.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::error::{Error, Result};
use crate::transport;

/// A remote object owned by the driver, as announced by a `__create__` event.
#[derive(Debug, Clone)]
pub struct RemoteObject {
    /// The object's globally unique id.
    pub guid: String,
    /// The object's Playwright type, e.g. `BrowserType`, `Page`.
    pub typ: String,
    /// The guid of the parent object, if any.
    pub parent: Option<String>,
    /// The initializer payload the driver sent for this object.
    pub initializer: Value,
}

/// A single event delivered to a subscriber: the method name and its params.
pub type Event = (String, Value);

struct Shared {
    callbacks: Mutex<HashMap<i64, oneshot::Sender<Result<Value>>>>,
    objects: Mutex<HashMap<String, RemoteObject>>,
    listeners: Mutex<HashMap<String, Vec<mpsc::UnboundedSender<Event>>>>,
    next_id: AtomicI64,
    /// The driver child process, if this connection owns one. Kept here so the
    /// process is killed (via `kill_on_drop`) once the last `Connection` clone
    /// is dropped.
    child: std::sync::Mutex<Option<tokio::process::Child>>,
}

impl Shared {
    async fn fail_all(&self, make_err: impl Fn() -> Error) {
        let mut cbs = self.callbacks.lock().await;
        for (_, cb) in cbs.drain() {
            let _ = cb.send(Err(make_err()));
        }
    }

    async fn dispatch(&self, msg: Value) {
        if let Some(id) = msg.get("id").and_then(Value::as_i64) {
            let cb = self.callbacks.lock().await.remove(&id);
            if let Some(cb) = cb {
                if let Some(err) = msg.get("error") {
                    let inner = err.get("error").cloned().unwrap_or(json!({}));
                    let _ = cb.send(Err(Error::Driver {
                        name: inner
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        message: inner
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        stack: inner
                            .get("stack")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    }));
                } else {
                    let result = msg.get("result").cloned().unwrap_or_else(|| json!({}));
                    let _ = cb.send(Ok(result));
                }
            }
            return;
        }

        let guid = msg
            .get("guid")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        match method.as_str() {
            "__create__" => {
                let typ = params
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let child_guid = params
                    .get("guid")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let initializer = params.get("initializer").cloned().unwrap_or_else(|| json!({}));
                self.objects.lock().await.insert(
                    child_guid.clone(),
                    RemoteObject {
                        guid: child_guid,
                        typ,
                        parent: Some(guid),
                        initializer,
                    },
                );
            }
            "__dispose__" => {
                self.objects.lock().await.remove(&guid);
                self.listeners.lock().await.remove(&guid);
            }
            _ => {
                let mut listeners = self.listeners.lock().await;
                if let Some(senders) = listeners.get_mut(&guid) {
                    senders.retain(|s| s.send((method.clone(), params.clone())).is_ok());
                }
            }
        }
    }
}

/// A connection to a running Playwright driver.
#[derive(Clone)]
pub struct Connection {
    shared: Arc<Shared>,
    writer: Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>,
}

impl Connection {
    /// Builds a connection over the given reader/writer halves and spawns the
    /// background read loop. The returned connection registers the empty-guid
    /// root object, ready for [`Connection::initialize`].
    pub fn new<R, W>(reader: R, writer: W) -> Connection
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Connection::with_child(reader, writer, None)
    }

    /// Like [`Connection::new`], but takes ownership of the driver child
    /// process so it is killed when the last connection clone is dropped.
    pub fn with_child<R, W>(reader: R, writer: W, child: Option<tokio::process::Child>) -> Connection
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        // Pre-register the root object (guid "") before any message can arrive.
        let mut objects = HashMap::new();
        objects.insert(
            String::new(),
            RemoteObject {
                guid: String::new(),
                typ: "Root".to_string(),
                parent: None,
                initializer: json!({}),
            },
        );

        let shared = Arc::new(Shared {
            callbacks: Mutex::new(HashMap::new()),
            objects: Mutex::new(objects),
            listeners: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(0),
            child: std::sync::Mutex::new(child),
        });

        // The read loop holds only a weak reference, so dropping the last
        // `Connection` lets `Shared` (and the child process) drop, which closes
        // the driver's stdout and ends this task.
        let weak = Arc::downgrade(&shared);
        tokio::spawn(async move {
            let mut reader = reader;
            loop {
                match transport::read_message(&mut reader).await {
                    Ok(Some(msg)) => match weak.upgrade() {
                        Some(shared) => shared.dispatch(msg).await,
                        None => break,
                    },
                    Ok(None) => break,
                    Err(e) => {
                        tracing::error!("driver read error: {e}");
                        break;
                    }
                }
            }
            if let Some(shared) = weak.upgrade() {
                shared.fail_all(|| Error::ConnectionClosed).await;
            }
        });

        Connection {
            shared,
            writer: Arc::new(Mutex::new(Box::new(writer))),
        }
    }

    /// Explicitly kills the driver process, if this connection owns one.
    pub async fn close(&self) {
        let child = self.shared.child.lock().unwrap().take();
        if let Some(mut child) = child {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }

    /// Sends a command to the driver and awaits its response.
    pub async fn send(&self, guid: &str, method: &str, params: Value) -> Result<Value> {
        let id = self.shared.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        self.shared.callbacks.lock().await.insert(id, tx);

        let wall_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let msg = json!({
            "id": id,
            "guid": guid,
            "method": method,
            "params": params,
            "metadata": { "wallTime": wall_time }
        });

        {
            let mut w = self.writer.lock().await;
            if let Err(e) = transport::write_message(&mut *w, &msg).await {
                self.shared.callbacks.lock().await.remove(&id);
                return Err(e);
            }
        }

        match rx.await {
            Ok(res) => res,
            Err(_) => Err(Error::ConnectionClosed),
        }
    }

    /// Performs the initial handshake and returns the guid of the root
    /// `Playwright` object.
    pub async fn initialize(&self) -> Result<String> {
        let result = self
            .send("", "initialize", json!({ "sdkLanguage": "javascript" }))
            .await?;
        result
            .get("playwright")
            .and_then(|p| p.get("guid"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| Error::Protocol("initialize did not return a playwright guid".into()))
    }

    /// Looks up a remote object by guid.
    pub async fn object(&self, guid: &str) -> Option<RemoteObject> {
        self.shared.objects.lock().await.get(guid).cloned()
    }

    /// Returns the initializer of a remote object, or an error if it is absent.
    pub async fn initializer(&self, guid: &str) -> Result<Value> {
        self.shared
            .objects
            .lock()
            .await
            .get(guid)
            .map(|o| o.initializer.clone())
            .ok_or_else(|| Error::ObjectNotFound(guid.to_string()))
    }

    /// Finds the first child object of `parent` with the given type.
    pub async fn child_of_type(&self, parent: &str, typ: &str) -> Option<RemoteObject> {
        self.shared
            .objects
            .lock()
            .await
            .values()
            .find(|o| o.parent.as_deref() == Some(parent) && o.typ == typ)
            .cloned()
    }

    /// Subscribes to events emitted on `guid`. Dropping the receiver
    /// unsubscribes on the next event.
    pub async fn subscribe(&self, guid: &str) -> mpsc::UnboundedReceiver<Event> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.shared
            .listeners
            .lock()
            .await
            .entry(guid.to_string())
            .or_default()
            .push(tx);
        rx
    }
}
