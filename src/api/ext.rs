//! Hand-written ergonomic conveniences layered on top of the generated api.
//!
//! These add the Python-style ergonomics that pure protocol generation can't
//! express: the process-owning entry point, page methods that delegate to the
//! main frame, and `evaluate` returning decoded JSON.

use base64::Engine;
use serde_json::{json, Value};

use crate::api::{Browser, BrowserType, Frame, Page, Playwright, Response};
use crate::driver::Driver;
use crate::error::{Error, Result};
use crate::serialized_value;

impl Playwright {
    /// Launches the Playwright driver, performs the handshake, and returns the
    /// root [`Playwright`] object. The driver process is kept alive for as long
    /// as any object derived from this one is alive, and killed afterwards.
    pub async fn initialize() -> Result<Playwright> {
        let conn = Driver::locate()?.spawn_connection()?;
        let guid = conn.initialize().await?;
        Ok(Playwright::from_channel(conn, guid))
    }

    /// The Chromium browser type.
    pub async fn chromium(&self) -> Result<BrowserType> {
        self.engine("chromium").await
    }

    /// The Firefox browser type.
    pub async fn firefox(&self) -> Result<BrowserType> {
        self.engine("firefox").await
    }

    /// The WebKit browser type.
    pub async fn webkit(&self) -> Result<BrowserType> {
        self.engine("webkit").await
    }

    async fn engine(&self, name: &str) -> Result<BrowserType> {
        let init = self.conn.initializer(&self.guid).await?;
        let guid = init
            .get(name)
            .and_then(|v| v.get("guid"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Protocol(format!("playwright initializer missing `{name}`")))?
            .to_string();
        Ok(BrowserType::from_channel(self.conn.clone(), guid))
    }

    /// Shuts the driver process down.
    pub async fn stop(&self) {
        self.conn.close().await;
    }
}

impl Browser {
    /// Convenience: creates a fresh context and opens a single page in it.
    pub async fn new_page(&self) -> Result<Page> {
        let context = self.new_context(Default::default()).await?;
        context.new_page().await
    }
}

impl Page {
    /// The page's main frame.
    pub async fn main_frame(&self) -> Result<Frame> {
        Ok(Frame::from_channel(self.conn.clone(), self.main_frame_guid().await?))
    }

    async fn main_frame_guid(&self) -> Result<String> {
        self.conn
            .initializer(&self.guid)
            .await?
            .get("mainFrame")
            .and_then(|v| v.get("guid"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| Error::Protocol("page initializer missing mainFrame".into()))
    }

    /// Navigates to `url`, waiting for the `load` event.
    pub async fn goto(&self, url: &str) -> Result<Option<Response>> {
        let frame = self.main_frame_guid().await?;
        let v = self
            .conn
            .send(&frame, "goto", json!({ "url": url, "waitUntil": "load" }))
            .await?;
        Ok(v.get("response")
            .and_then(|r| r.get("guid"))
            .and_then(Value::as_str)
            .map(|g| Response::from_channel(self.conn.clone(), g.to_string())))
    }

    /// The page's full HTML content.
    pub async fn content(&self) -> Result<String> {
        let frame = self.main_frame_guid().await?;
        let v = self.conn.send(&frame, "content", json!({})).await?;
        Ok(string_field(&v, "value"))
    }

    /// The page title.
    pub async fn title(&self) -> Result<String> {
        let frame = self.main_frame_guid().await?;
        let v = self.conn.send(&frame, "title", json!({})).await?;
        Ok(string_field(&v, "value"))
    }

    /// The current URL (read via `window.location.href`).
    pub async fn url(&self) -> Result<String> {
        Ok(self
            .evaluate("() => window.location.href")
            .await?
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// Sets the page HTML content.
    pub async fn set_content(&self, html: &str) -> Result<()> {
        let frame = self.main_frame_guid().await?;
        self.conn
            .send(&frame, "setContent", json!({ "html": html, "waitUntil": "load" }))
            .await?;
        Ok(())
    }

    /// Clicks the first element matching `selector`.
    pub async fn click(&self, selector: &str) -> Result<()> {
        let frame = self.main_frame_guid().await?;
        self.conn
            .send(&frame, "click", json!({ "selector": selector }))
            .await?;
        Ok(())
    }

    /// Fills an input matching `selector` with `value`.
    pub async fn fill(&self, selector: &str, value: &str) -> Result<()> {
        let frame = self.main_frame_guid().await?;
        self.conn
            .send(&frame, "fill", json!({ "selector": selector, "value": value }))
            .await?;
        Ok(())
    }

    /// The rendered text of the element matching `selector`.
    pub async fn inner_text(&self, selector: &str) -> Result<String> {
        let frame = self.main_frame_guid().await?;
        let v = self
            .conn
            .send(&frame, "innerText", json!({ "selector": selector }))
            .await?;
        Ok(string_field(&v, "value"))
    }

    /// `node.textContent` of the element matching `selector`.
    pub async fn text_content(&self, selector: &str) -> Result<Option<String>> {
        let frame = self.main_frame_guid().await?;
        let v = self
            .conn
            .send(&frame, "textContent", json!({ "selector": selector }))
            .await?;
        Ok(v.get("value").and_then(Value::as_str).map(str::to_string))
    }

    /// The value of `attribute` on the element matching `selector`.
    pub async fn get_attribute(&self, selector: &str, attribute: &str) -> Result<Option<String>> {
        let frame = self.main_frame_guid().await?;
        let v = self
            .conn
            .send(&frame, "getAttribute", json!({ "selector": selector, "name": attribute }))
            .await?;
        Ok(v.get("value").and_then(Value::as_str).map(str::to_string))
    }

    /// Waits until an element matching `selector` is visible.
    pub async fn wait_for_selector(&self, selector: &str) -> Result<()> {
        let frame = self.main_frame_guid().await?;
        self.conn
            .send(&frame, "waitForSelector", json!({ "selector": selector, "state": "visible" }))
            .await?;
        Ok(())
    }

    /// Evaluates a JS expression or function in the page, returning JSON.
    pub async fn evaluate(&self, expression: &str) -> Result<Value> {
        self.evaluate_value(expression, &Value::Null).await
    }

    /// Like [`Page::evaluate`] but passes `arg` as the function argument.
    pub async fn evaluate_with(&self, expression: &str, arg: Value) -> Result<Value> {
        self.evaluate_value(expression, &arg).await
    }

    async fn evaluate_value(&self, expression: &str, arg: &Value) -> Result<Value> {
        let frame = self.main_frame_guid().await?;
        let is_function = {
            let t = expression.trim_start();
            t.starts_with("function") || t.starts_with("async") || t.contains("=>")
        };
        let v = self
            .conn
            .send(
                &frame,
                "evaluateExpression",
                json!({
                    "expression": expression,
                    "isFunction": is_function,
                    "arg": serialized_value::to_arg(arg),
                }),
            )
            .await?;
        Ok(v.get("value").map(serialized_value::deserialize).unwrap_or(Value::Null))
    }

    /// Captures a PNG screenshot and returns the raw image bytes.
    ///
    /// (The generated [`Page::screenshot`] takes full protocol options and
    /// returns base64; this is the convenient byte-returning shortcut.)
    pub async fn screenshot_png(&self) -> Result<Vec<u8>> {
        let v = self.conn.send(&self.guid, "screenshot", json!({ "type": "png" })).await?;
        decode_binary(&v, "binary")
    }
}

impl Response {
    /// The response URL.
    pub async fn url(&self) -> Result<String> {
        Ok(string_field(&self.conn.initializer(&self.guid).await?, "url"))
    }

    /// The HTTP status code.
    pub async fn status(&self) -> Result<u16> {
        Ok(self
            .conn
            .initializer(&self.guid)
            .await?
            .get("status")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u16)
    }

    /// Whether the status indicates success (2xx/3xx, or 0 for `file:`/`data:`).
    pub async fn ok(&self) -> Result<bool> {
        let s = self.status().await?;
        Ok(s == 0 || (200..400).contains(&s))
    }

    /// The raw response body bytes.
    pub async fn body_bytes(&self) -> Result<Vec<u8>> {
        let v = self.conn.send(&self.guid, "body", json!({})).await?;
        decode_binary(&v, "binary")
    }

    /// The response body decoded as UTF-8 text.
    pub async fn text(&self) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.body_bytes().await?).into_owned())
    }
}

fn string_field(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn decode_binary(v: &Value, key: &str) -> Result<Vec<u8>> {
    let b64 = v
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Protocol(format!("missing binary field {key}")))?;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| Error::Protocol(format!("invalid base64 data: {e}")))
}
