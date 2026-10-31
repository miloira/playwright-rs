//! High-level, typed event helpers.
//!
//! The generated layer exposes raw event streams via each wrapper's `events()`
//! method (yielding the `*ChannelEvent` enum). These helpers narrow that to a
//! single event kind and hand back wrapped objects, matching the `page.on(...)`
//! / `context.on(...)` ergonomics of other Playwright bindings.
//!
//! Note: in the Playwright protocol, `console`, `dialog`, `request` and
//! `response` are emitted on the **browser context**, not the page.

use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver};

use crate::api::{Artifact, BrowserContext, Dialog, ElementHandle, Frame, Page, Request, Response, Worker};
use crate::connection::Connection;
use crate::error::Result;

/// A console message logged by the page.
#[derive(Debug, Clone)]
pub struct ConsoleMessage {
    /// The message type, e.g. `"log"`, `"error"`, `"warning"`.
    pub r#type: String,
    /// The message text.
    pub text: String,
}

/// A file download started by the page.
#[derive(Clone)]
pub struct Download {
    /// The download URL.
    pub url: String,
    /// The filename suggested by the server/page.
    pub suggested_filename: String,
    /// The artifact handle, used to save or read the downloaded file.
    pub artifact: Artifact,
}

/// A file chooser opened by the page (e.g. an `<input type=file>` click).
#[derive(Clone)]
pub struct FileChooser {
    /// The input element that triggered the chooser.
    pub element: ElementHandle,
    /// Whether multiple files may be selected.
    pub is_multiple: bool,
}

impl Download {
    /// Saves the downloaded file to `path`, waiting for the download to finish.
    pub async fn save_as(&self, path: &str) -> Result<()> {
        self.artifact
            .connection()
            .send(self.artifact.guid(), "saveAs", json!({ "path": path }))
            .await
            .map(drop)
    }

    /// The local path of the downloaded file once finished.
    pub async fn path(&self) -> Result<String> {
        let v = self
            .artifact
            .connection()
            .send(self.artifact.guid(), "pathAfterFinished", json!({}))
            .await?;
        Ok(v.get("value").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// The download failure message, if the download failed.
    pub async fn failure(&self) -> Result<Option<String>> {
        let v = self
            .artifact
            .connection()
            .send(self.artifact.guid(), "failure", json!({}))
            .await?;
        Ok(v.get("error").and_then(Value::as_str).map(str::to_string))
    }

    /// Deletes the downloaded file.
    pub async fn delete(&self) -> Result<()> {
        self.artifact.connection().send(self.artifact.guid(), "delete", json!({})).await.map(drop)
    }

    /// Cancels the download.
    pub async fn cancel(&self) -> Result<()> {
        self.artifact.connection().send(self.artifact.guid(), "cancel", json!({})).await.map(drop)
    }
}

impl FileChooser {
    /// Whether multiple files may be selected.
    pub fn is_multiple(&self) -> bool {
        self.is_multiple
    }

    /// The input element that triggered the chooser.
    pub fn element(&self) -> &ElementHandle {
        &self.element
    }

    /// Sets the chosen files by local path.
    pub async fn set_files(&self, paths: &[&str]) -> Result<()> {
        self.element
            .connection()
            .send(self.element.guid(), "setInputFiles", json!({ "localPaths": paths }))
            .await
            .map(drop)
    }
}

/// Subscribes to `guid` and forwards events whose method is `method`, mapping
/// each event's params with `map`. Subscription happens before returning, so no
/// event emitted after the call is missed.
async fn on_event<T, F>(
    conn: Connection,
    guid: String,
    method: &'static str,
    map: F,
) -> UnboundedReceiver<T>
where
    T: Send + 'static,
    F: Fn(&Connection, Value) -> Option<T> + Send + 'static,
{
    let mut raw = conn.subscribe(&guid).await;
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some((m, params)) = raw.recv().await {
            if m == method {
                if let Some(item) = map(&conn, params) {
                    if tx.send(item).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

fn channel_wrapper<T>(
    conn: &Connection,
    params: &Value,
    field: &str,
    build: impl Fn(Connection, String) -> T,
) -> Option<T> {
    params
        .get(field)
        .and_then(|v| v.get("guid"))
        .and_then(Value::as_str)
        .map(|g| build(conn.clone(), g.to_string()))
}

impl BrowserContext {
    /// Enables server-side emission of a gated event (console/dialog/request/
    /// response). High-volume events are off by default to save bandwidth.
    async fn enable_event(&self, event: &str) {
        let _ = self
            .connection()
            .send(&self.guid, "updateSubscription", json!({ "event": event, "enabled": true }))
            .await;
    }

    /// A stream of console messages from any page in this context.
    pub async fn on_console(&self) -> UnboundedReceiver<ConsoleMessage> {
        let rx = on_event(self.connection(), self.guid.clone(), "console", |_, p| {
            Some(ConsoleMessage {
                r#type: p.get("type").and_then(Value::as_str).unwrap_or_default().to_string(),
                text: p.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
            })
        })
        .await;
        self.enable_event("console").await;
        rx
    }

    /// A stream of dialogs (alert/confirm/prompt) opened in this context.
    pub async fn on_dialog(&self) -> UnboundedReceiver<Dialog> {
        let rx = on_event(self.connection(), self.guid.clone(), "dialog", |c, p| {
            channel_wrapper(c, &p, "dialog", Dialog::from_channel)
        })
        .await;
        self.enable_event("dialog").await;
        rx
    }

    /// A stream of new pages opened in this context (e.g. popups).
    pub async fn on_page(&self) -> UnboundedReceiver<Page> {
        // `page` is emitted unconditionally; no subscription needed.
        on_event(self.connection(), self.guid.clone(), "page", |c, p| {
            channel_wrapper(c, &p, "page", Page::from_channel)
        })
        .await
    }

    /// A stream of network requests issued in this context.
    pub async fn on_request(&self) -> UnboundedReceiver<Request> {
        let rx = on_event(self.connection(), self.guid.clone(), "request", |c, p| {
            channel_wrapper(c, &p, "request", Request::from_channel)
        })
        .await;
        self.enable_event("request").await;
        rx
    }

    /// A stream of network responses received in this context.
    pub async fn on_response(&self) -> UnboundedReceiver<Response> {
        let rx = on_event(self.connection(), self.guid.clone(), "response", |c, p| {
            channel_wrapper(c, &p, "response", Response::from_channel)
        })
        .await;
        self.enable_event("response").await;
        rx
    }

    /// A stream of requests that finished successfully.
    pub async fn on_request_finished(&self) -> UnboundedReceiver<Request> {
        let rx = on_event(self.connection(), self.guid.clone(), "requestFinished", |c, p| {
            channel_wrapper(c, &p, "request", Request::from_channel)
        })
        .await;
        self.enable_event("requestFinished").await;
        rx
    }

    /// A stream of requests that failed.
    pub async fn on_request_failed(&self) -> UnboundedReceiver<Request> {
        let rx = on_event(self.connection(), self.guid.clone(), "requestFailed", |c, p| {
            channel_wrapper(c, &p, "request", Request::from_channel)
        })
        .await;
        self.enable_event("requestFailed").await;
        rx
    }

    /// A stream of dialogs that were closed.
    pub async fn on_dialog_closed(&self) -> UnboundedReceiver<Dialog> {
        let rx = on_event(self.connection(), self.guid.clone(), "dialogClosed", |c, p| {
            channel_wrapper(c, &p, "dialog", Dialog::from_channel)
        })
        .await;
        self.enable_event("dialogClosed").await;
        rx
    }
}

impl Page {
    /// Emits once when the page is closed.
    pub async fn on_close(&self) -> UnboundedReceiver<()> {
        on_event(self.connection(), self.guid.clone(), "close", |_, _| Some(())).await
    }

    /// Emits when the page crashes.
    pub async fn on_crash(&self) -> UnboundedReceiver<()> {
        on_event(self.connection(), self.guid.clone(), "crash", |_, _| Some(())).await
    }

    /// A stream of dedicated web workers created by the page.
    pub async fn on_worker(&self) -> UnboundedReceiver<Worker> {
        on_event(self.connection(), self.guid.clone(), "worker", |c, p| {
            channel_wrapper(c, &p, "worker", Worker::from_channel)
        })
        .await
    }

    /// A stream of downloads started by the page.
    pub async fn on_download(&self) -> UnboundedReceiver<Download> {
        on_event(self.connection(), self.guid.clone(), "download", |c, p| {
            let guid = p.get("artifact").and_then(|a| a.get("guid")).and_then(Value::as_str)?;
            Some(Download {
                url: p.get("url").and_then(Value::as_str).unwrap_or_default().to_string(),
                suggested_filename: p
                    .get("suggestedFilename")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                artifact: Artifact::from_channel(c.clone(), guid.to_string()),
            })
        })
        .await
    }

    /// A stream of file choosers opened by the page.
    pub async fn on_file_chooser(&self) -> UnboundedReceiver<FileChooser> {
        let rx = on_event(self.connection(), self.guid.clone(), "fileChooser", |c, p| {
            let guid = p.get("element").and_then(|e| e.get("guid")).and_then(Value::as_str)?;
            Some(FileChooser {
                element: ElementHandle::from_channel(c.clone(), guid.to_string()),
                is_multiple: p.get("isMultiple").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .await;
        // `fileChooser` is gated per-page.
        let _ = self
            .connection()
            .send(&self.guid, "updateSubscription", json!({ "event": "fileChooser", "enabled": true }))
            .await;
        rx
    }

    /// A stream of frames attached to the page.
    pub async fn on_frame_attached(&self) -> UnboundedReceiver<Frame> {
        on_event(self.connection(), self.guid.clone(), "frameAttached", |c, p| {
            channel_wrapper(c, &p, "frame", Frame::from_channel)
        })
        .await
    }

    /// A stream of frames detached from the page.
    pub async fn on_frame_detached(&self) -> UnboundedReceiver<Frame> {
        on_event(self.connection(), self.guid.clone(), "frameDetached", |c, p| {
            channel_wrapper(c, &p, "frame", Frame::from_channel)
        })
        .await
    }
}
