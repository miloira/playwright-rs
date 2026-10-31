//! Additional Python-parity conveniences: tracing, clock, websocket frames,
//! dialog handling, and request/response accessors.

use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver};

use crate::api::{BrowserContext, Dialog, Request, Response, Tracing, WebSocket};
use crate::connection::Connection;
use crate::error::Result;

// ---------------------------------------------------------------------------
// Tracing
// ---------------------------------------------------------------------------

impl BrowserContext {
    /// The context's [`Tracing`] controller.
    pub async fn tracing(&self) -> Result<Tracing> {
        let init = self.connection().initializer(&self.guid).await?;
        let guid = init
            .get("tracing")
            .and_then(|v| v.get("guid"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(Tracing::from_channel(self.connection(), guid))
    }
}

impl Tracing {
    /// Starts tracing. `screenshots` captures a screencast; `snapshots` captures
    /// DOM/ARIA snapshots for the trace viewer.
    pub async fn start(&self, name: Option<&str>, screenshots: bool, snapshots: bool) -> Result<()> {
        self.connection()
            .send(
                &self.guid,
                "tracingStart",
                json!({
                    "name": name,
                    "screencast": screenshots,
                    "snapshotDom": snapshots,
                    "snapshotAria": snapshots,
                }),
            )
            .await?;
        self.connection()
            .send(&self.guid, "tracingStartChunk", json!({}))
            .await?;
        Ok(())
    }

    /// Starts a new trace chunk (after a previous [`Tracing::stop`]).
    pub async fn start_chunk(&self, title: Option<&str>) -> Result<()> {
        self.connection()
            .send(&self.guid, "tracingStartChunk", json!({ "title": title }))
            .await
            .map(drop)
    }

    /// Stops tracing. If `path` is given, the trace zip is saved there.
    pub async fn stop(&self, path: Option<&str>) -> Result<()> {
        let mode = if path.is_some() { "archive" } else { "discard" };
        let result = self
            .connection()
            .send(&self.guid, "tracingStopChunk", json!({ "mode": mode }))
            .await?;
        if let Some(path) = path {
            if let Some(artifact_guid) = result
                .get("artifact")
                .and_then(|a| a.get("guid"))
                .and_then(Value::as_str)
            {
                self.connection()
                    .send(artifact_guid, "saveAs", json!({ "path": path }))
                    .await?;
            }
        }
        self.connection().send(&self.guid, "tracingStop", json!({})).await?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

impl BrowserContext {
    /// The context's [`Clock`] controller for faking time.
    pub fn clock(&self) -> Clock {
        Clock { conn: self.connection(), guid: self.guid.clone() }
    }
}

/// Controls the browser clock (time faking) for a context.
#[derive(Clone)]
pub struct Clock {
    conn: Connection,
    guid: String,
}

impl Clock {
    /// Installs the fake clock at the current time.
    pub async fn install(&self) -> Result<()> {
        self.conn.send(&self.guid, "clockInstall", json!({})).await.map(drop)
    }

    /// Installs the fake clock initialized to `time_ms` (ms since epoch).
    pub async fn install_at(&self, time_ms: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "clockInstall", json!({ "timeNumber": time_ms }))
            .await
            .map(drop)
    }

    /// Sets a fixed `Date.now()` without pausing timers.
    pub async fn set_fixed_time(&self, time_ms: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "clockSetFixedTime", json!({ "timeNumber": time_ms }))
            .await
            .map(drop)
    }

    /// Sets the system time (fires timers as appropriate).
    pub async fn set_system_time(&self, time_ms: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "clockSetSystemTime", json!({ "timeNumber": time_ms }))
            .await
            .map(drop)
    }

    /// Fast-forwards time by `ticks_ms`, firing timers without delay.
    pub async fn fast_forward(&self, ticks_ms: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "clockFastForward", json!({ "ticksNumber": ticks_ms }))
            .await
            .map(drop)
    }

    /// Advances time by `ticks_ms`, firing timers along the way.
    pub async fn run_for(&self, ticks_ms: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "clockRunFor", json!({ "ticksNumber": ticks_ms }))
            .await
            .map(drop)
    }

    /// Pauses time at `time_ms`.
    pub async fn pause_at(&self, time_ms: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "clockPauseAt", json!({ "timeNumber": time_ms }))
            .await
            .map(drop)
    }

    /// Resumes the clock.
    pub async fn resume(&self) -> Result<()> {
        self.conn.send(&self.guid, "clockResume", json!({})).await.map(drop)
    }
}

// ---------------------------------------------------------------------------
// WebSocket frames
// ---------------------------------------------------------------------------

/// A single WebSocket frame.
#[derive(Debug, Clone)]
pub struct WebSocketFrame {
    /// The frame opcode (1 = text, 2 = binary).
    pub opcode: i64,
    /// The frame payload (text, or base64 for binary frames).
    pub payload: String,
}

fn ws_frame(params: &Value) -> Option<WebSocketFrame> {
    Some(WebSocketFrame {
        opcode: params.get("opcode").and_then(Value::as_i64).unwrap_or(0),
        payload: params.get("data").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

async fn subscribe_map<T, F>(conn: Connection, guid: String, method: &'static str, map: F) -> UnboundedReceiver<T>
where
    T: Send + 'static,
    F: Fn(&Value) -> Option<T> + Send + 'static,
{
    let mut raw = conn.subscribe(&guid).await;
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some((m, params)) = raw.recv().await {
            if m == method {
                if let Some(item) = map(&params) {
                    if tx.send(item).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

impl WebSocket {
    /// The WebSocket URL.
    pub async fn url(&self) -> Result<String> {
        let init = self.connection().initializer(&self.guid).await?;
        Ok(init.get("url").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// A stream of frames sent by the page.
    pub async fn on_frame_sent(&self) -> UnboundedReceiver<WebSocketFrame> {
        subscribe_map(self.connection(), self.guid.clone(), "frameSent", ws_frame).await
    }

    /// A stream of frames received from the server.
    pub async fn on_frame_received(&self) -> UnboundedReceiver<WebSocketFrame> {
        subscribe_map(self.connection(), self.guid.clone(), "frameReceived", ws_frame).await
    }

    /// Emits once when the socket closes.
    pub async fn on_close(&self) -> UnboundedReceiver<()> {
        subscribe_map(self.connection(), self.guid.clone(), "close", |_| Some(())).await
    }
}

// ---------------------------------------------------------------------------
// Dialog
// ---------------------------------------------------------------------------

impl Dialog {
    /// The dialog type: `"alert"`, `"confirm"`, `"prompt"` or `"beforeunload"`.
    pub async fn dialog_type(&self) -> Result<String> {
        Ok(self
            .connection()
            .initializer(&self.guid)
            .await?
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// The dialog message.
    pub async fn message(&self) -> Result<String> {
        Ok(self
            .connection()
            .initializer(&self.guid)
            .await?
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// Accepts the dialog, optionally supplying prompt text.
    ///
    /// (The generated [`Dialog::accept`] takes the full protocol options; this
    /// is the convenient shortcut.)
    pub async fn accept_text(&self, prompt_text: Option<&str>) -> Result<()> {
        self.connection()
            .send(&self.guid, "accept", json!({ "promptText": prompt_text }))
            .await
            .map(drop)
    }
}

// ---------------------------------------------------------------------------
// Request / Response accessors
// ---------------------------------------------------------------------------

fn headers_map(init: &Value) -> Vec<(String, String)> {
    init.get("headers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    Some((
                        h.get("name")?.as_str()?.to_string(),
                        h.get("value")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Request {
    /// The request URL.
    pub async fn url(&self) -> Result<String> {
        Ok(self
            .connection()
            .initializer(&self.guid)
            .await?
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// The HTTP method.
    pub async fn method(&self) -> Result<String> {
        Ok(self
            .connection()
            .initializer(&self.guid)
            .await?
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// The resource type (`"document"`, `"xhr"`, `"fetch"`, …).
    pub async fn resource_type(&self) -> Result<String> {
        Ok(self
            .connection()
            .initializer(&self.guid)
            .await?
            .get("resourceType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// Whether this is a navigation request.
    pub async fn is_navigation_request(&self) -> Result<bool> {
        Ok(self
            .connection()
            .initializer(&self.guid)
            .await?
            .get("isNavigationRequest")
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }

    /// The request headers as `(name, value)` pairs.
    pub async fn headers(&self) -> Result<Vec<(String, String)>> {
        Ok(headers_map(&self.connection().initializer(&self.guid).await?))
    }
}

impl Response {
    /// The response headers as `(name, value)` pairs.
    pub async fn headers(&self) -> Result<Vec<(String, String)>> {
        Ok(headers_map(&self.connection().initializer(&self.guid).await?))
    }

    /// The response body parsed as JSON.
    pub async fn json(&self) -> Result<Value> {
        let text = self.text().await?;
        Ok(serde_json::from_str(&text)?)
    }
}
