//! `Locator` — a client-side, lazy handle to element(s) matching a selector.
//!
//! Locators have no protocol object of their own. A locator is a `(frame,
//! selector)` pair; every action re-resolves the selector against the frame at
//! call time, routing to the frame's selector-based protocol commands.

use base64::Engine;
use serde_json::{json, Value};

use crate::api::{ElementHandle, Frame, Page};
use crate::error::{Error, Result};
use crate::serialized_value;

/// An element's position and size in CSS pixels, relative to the viewport.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A way to find element(s) on a page, resolved lazily on each action.
#[derive(Clone)]
pub struct Locator {
    frame: Frame,
    selector: String,
}

impl Locator {
    pub(crate) fn new(frame: Frame, selector: String) -> Locator {
        Locator { frame, selector }
    }

    /// The selector string this locator resolves.
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// The frame this locator is bound to.
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// Returns a locator scoped to descendants matching `selector`.
    pub fn locator(&self, selector: &str) -> Locator {
        Locator::new(self.frame.clone(), format!("{} >> {}", self.selector, selector))
    }

    /// The i-th match (0-based). Negative indexing is not supported; use
    /// [`Locator::last`].
    pub fn nth(&self, index: usize) -> Locator {
        Locator::new(self.frame.clone(), format!("{} >> nth={}", self.selector, index))
    }

    /// The first match.
    pub fn first(&self) -> Locator {
        Locator::new(self.frame.clone(), format!("{} >> nth=0", self.selector))
    }

    /// The last match.
    pub fn last(&self) -> Locator {
        Locator::new(self.frame.clone(), format!("{} >> nth=-1", self.selector))
    }

    async fn call(&self, method: &str, mut params: Value) -> Result<Value> {
        if let Value::Object(map) = &mut params {
            map.insert("selector".to_string(), json!(self.selector));
        }
        self.frame
            .connection()
            .send(self.frame.guid(), method, params)
            .await
    }

    /// Clicks the element.
    pub async fn click(&self) -> Result<()> {
        self.call("click", json!({})).await.map(drop)
    }

    /// Double-clicks the element.
    pub async fn dblclick(&self) -> Result<()> {
        self.call("dblclick", json!({})).await.map(drop)
    }

    /// Hovers over the element.
    pub async fn hover(&self) -> Result<()> {
        self.call("hover", json!({})).await.map(drop)
    }

    /// Fills an input/textarea with `value`.
    pub async fn fill(&self, value: &str) -> Result<()> {
        self.call("fill", json!({ "value": value })).await.map(drop)
    }

    /// Types `text` key by key (prefer [`Locator::fill`] for most inputs).
    pub async fn type_text(&self, text: &str) -> Result<()> {
        self.call("type", json!({ "text": text })).await.map(drop)
    }

    /// Presses `key` (e.g. `"Enter"`, `"Control+A"`).
    pub async fn press(&self, key: &str) -> Result<()> {
        self.call("press", json!({ "key": key })).await.map(drop)
    }

    /// Focuses the element.
    pub async fn focus(&self) -> Result<()> {
        self.call("focus", json!({})).await.map(drop)
    }

    /// Taps the element (touch).
    pub async fn tap(&self) -> Result<()> {
        self.call("tap", json!({})).await.map(drop)
    }

    /// Checks a checkbox/radio.
    pub async fn check(&self) -> Result<()> {
        self.call("check", json!({})).await.map(drop)
    }

    /// Unchecks a checkbox.
    pub async fn uncheck(&self) -> Result<()> {
        self.call("uncheck", json!({})).await.map(drop)
    }

    /// Scrolls the element into view if needed.
    pub async fn scroll_into_view_if_needed(&self) -> Result<()> {
        self.call("scrollIntoViewIfNeeded", json!({})).await.map(drop)
    }

    /// Selects `<option>`s by value.
    pub async fn select_option(&self, values: &[&str]) -> Result<Vec<String>> {
        let options: Vec<Value> = values.iter().map(|v| json!({ "value": v })).collect();
        let r = self.call("selectOption", json!({ "options": options })).await?;
        Ok(r.get("values")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default())
    }

    /// Waits for the element to become visible.
    pub async fn wait_for(&self) -> Result<()> {
        self.call("waitForSelector", json!({ "state": "visible" })).await.map(drop)
    }

    /// `node.textContent`, if the element exists.
    pub async fn text_content(&self) -> Result<Option<String>> {
        let r = self.call("textContent", json!({})).await?;
        Ok(r.get("value").and_then(Value::as_str).map(str::to_string))
    }

    /// The element's rendered inner text.
    pub async fn inner_text(&self) -> Result<String> {
        Ok(str_value(&self.call("innerText", json!({})).await?))
    }

    /// The element's inner HTML.
    pub async fn inner_html(&self) -> Result<String> {
        Ok(str_value(&self.call("innerHTML", json!({})).await?))
    }

    /// The value of an `<input>`/`<textarea>`/`<select>`.
    pub async fn input_value(&self) -> Result<String> {
        Ok(str_value(&self.call("inputValue", json!({})).await?))
    }

    /// The value of `attribute`, if present.
    pub async fn get_attribute(&self, attribute: &str) -> Result<Option<String>> {
        let r = self.call("getAttribute", json!({ "name": attribute })).await?;
        Ok(r.get("value").and_then(Value::as_str).map(str::to_string))
    }

    /// Whether the element is visible.
    pub async fn is_visible(&self) -> Result<bool> {
        Ok(bool_value(&self.call("isVisible", json!({})).await?))
    }

    /// Whether the element is hidden.
    pub async fn is_hidden(&self) -> Result<bool> {
        Ok(bool_value(&self.call("isHidden", json!({})).await?))
    }

    /// Whether the element is enabled.
    pub async fn is_enabled(&self) -> Result<bool> {
        Ok(bool_value(&self.call("isEnabled", json!({})).await?))
    }

    /// Whether the element is disabled.
    pub async fn is_disabled(&self) -> Result<bool> {
        Ok(bool_value(&self.call("isDisabled", json!({})).await?))
    }

    /// Whether a checkbox/radio is checked.
    pub async fn is_checked(&self) -> Result<bool> {
        Ok(bool_value(&self.call("isChecked", json!({})).await?))
    }

    /// Whether the element is editable.
    pub async fn is_editable(&self) -> Result<bool> {
        Ok(bool_value(&self.call("isEditable", json!({})).await?))
    }

    /// The number of elements matching the selector.
    pub async fn count(&self) -> Result<i64> {
        let r = self.call("queryCount", json!({})).await?;
        Ok(r.get("value").and_then(Value::as_i64).unwrap_or(0))
    }

    /// Clears an input/textarea (equivalent to `fill("")`).
    pub async fn clear(&self) -> Result<()> {
        self.fill("").await
    }

    /// Sets the files for a file input by local path.
    pub async fn set_input_files(&self, paths: &[&str]) -> Result<()> {
        self.call("setInputFiles", json!({ "localPaths": paths })).await.map(drop)
    }

    /// Drags this element onto `target`.
    pub async fn drag_to(&self, target: &Locator) -> Result<()> {
        self.frame
            .connection()
            .send(
                self.frame.guid(),
                "dragAndDrop",
                json!({ "source": self.selector, "target": target.selector }),
            )
            .await
            .map(drop)
    }

    /// Dispatches a DOM event (e.g. `"click"`, `"mousedown"`) to the element.
    pub async fn dispatch_event(&self, event_type: &str) -> Result<()> {
        self.call(
            "dispatchEvent",
            json!({ "type": event_type, "eventInit": serialized_value::to_arg(&Value::Null) }),
        )
        .await
        .map(drop)
    }

    /// Captures the element's accessibility (ARIA) snapshot as YAML-ish text.
    pub async fn aria_snapshot(&self) -> Result<String> {
        let r = self.call("ariaSnapshot", json!({})).await?;
        Ok(r.get("snapshot").and_then(Value::as_str).unwrap_or_default().to_string())
    }

    /// The element's bounding box, or `None` if it is not rendered.
    pub async fn bounding_box(&self) -> Result<Option<BoundingBox>> {
        let v = self
            .evaluate(
                "el => { const r = el.getBoundingClientRect(); \
                 return { x: r.x, y: r.y, width: r.width, height: r.height }; }",
            )
            .await?;
        if v.is_null() {
            Ok(None)
        } else {
            Ok(Some(serde_json::from_value(v)?))
        }
    }

    /// The `textContent` of every matching element.
    pub async fn all_text_contents(&self) -> Result<Vec<String>> {
        let v = self.eval_all("ee => ee.map(e => e.textContent || '')").await?;
        Ok(serde_json::from_value(v).unwrap_or_default())
    }

    /// The rendered `innerText` of every matching element.
    pub async fn all_inner_texts(&self) -> Result<Vec<String>> {
        let v = self.eval_all("ee => ee.map(e => e.innerText)").await?;
        Ok(serde_json::from_value(v).unwrap_or_default())
    }

    /// One locator per current match (snapshotted by count).
    pub async fn all(&self) -> Result<Vec<Locator>> {
        let n = self.count().await?.max(0);
        Ok((0..n).map(|i| self.nth(i as usize)).collect())
    }

    /// Resolves the first matching element to an [`ElementHandle`], if any.
    pub async fn element_handle(&self) -> Result<Option<ElementHandle>> {
        let r = self.call("querySelector", json!({ "strict": true })).await?;
        Ok(r.get("element")
            .and_then(|e| e.get("guid"))
            .and_then(Value::as_str)
            .map(|g| ElementHandle::from_channel(self.frame.connection(), g.to_string())))
    }

    /// Captures a PNG screenshot of the element.
    pub async fn screenshot_png(&self) -> Result<Vec<u8>> {
        let handle = self
            .element_handle()
            .await?
            .ok_or_else(|| Error::Protocol(format!("no element matched {}", self.selector)))?;
        let v = handle
            .connection()
            .send(handle.guid(), "screenshot", json!({ "type": "png" }))
            .await?;
        let b64 = v
            .get("binary")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Protocol("screenshot returned no binary".into()))?;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| Error::Protocol(format!("invalid base64: {e}")))
    }

    /// Evaluates a JS function with the matched element as its first argument.
    pub async fn evaluate(&self, expression: &str) -> Result<Value> {
        let r = self
            .call(
                "evalOnSelector",
                json!({
                    "expression": expression,
                    "isFunction": true,
                    "arg": serialized_value::to_arg(&Value::Null),
                }),
            )
            .await?;
        Ok(r.get("value").map(serialized_value::deserialize).unwrap_or(Value::Null))
    }

    /// Evaluates a JS function over all matching elements (as an array).
    async fn eval_all(&self, expression: &str) -> Result<Value> {
        let r = self
            .call(
                "evalOnSelectorAll",
                json!({
                    "expression": expression,
                    "isFunction": true,
                    "arg": serialized_value::to_arg(&Value::Null),
                }),
            )
            .await?;
        Ok(r.get("value").map(serialized_value::deserialize).unwrap_or(Value::Null))
    }
}

// --- Selector builders for the `get_by_*` family (Playwright engines). ---
fn sel_text(text: &str) -> String {
    format!("text={text:?}")
}
fn sel_role(role: &str, name: Option<&str>) -> String {
    match name {
        Some(n) => format!("role={role}[name={n:?}]"),
        None => format!("role={role}"),
    }
}
fn sel_test_id(id: &str) -> String {
    format!("[data-testid={id:?}]")
}
fn sel_placeholder(text: &str) -> String {
    format!("[placeholder={text:?}]")
}
fn sel_alt(text: &str) -> String {
    format!("[alt={text:?}]")
}
fn sel_title(text: &str) -> String {
    format!("[title={text:?}]")
}
fn sel_label(text: &str) -> String {
    format!("internal:label={text:?}i")
}

/// A handle to an iframe, used to locate elements inside it.
#[derive(Clone)]
pub struct FrameLocator {
    frame: Frame,
    prefix: String,
}

impl FrameLocator {
    fn child(&self, selector: &str) -> Locator {
        Locator::new(
            self.frame.clone(),
            format!("{} >> internal:control=enter-frame >> {}", self.prefix, selector),
        )
    }

    /// Locates elements inside the frame.
    pub fn locator(&self, selector: &str) -> Locator {
        self.child(selector)
    }

    /// The first matching frame.
    pub fn first(&self) -> FrameLocator {
        FrameLocator { frame: self.frame.clone(), prefix: format!("{} >> nth=0", self.prefix) }
    }

    /// The i-th matching frame.
    pub fn nth(&self, index: usize) -> FrameLocator {
        FrameLocator { frame: self.frame.clone(), prefix: format!("{} >> nth={index}", self.prefix) }
    }

    pub fn get_by_text(&self, text: &str) -> Locator {
        self.child(&sel_text(text))
    }
    pub fn get_by_role(&self, role: &str, name: Option<&str>) -> Locator {
        self.child(&sel_role(role, name))
    }
    pub fn get_by_test_id(&self, id: &str) -> Locator {
        self.child(&sel_test_id(id))
    }
}

impl Frame {
    /// Creates a [`Locator`] rooted at this frame.
    pub fn locator(&self, selector: &str) -> Locator {
        Locator::new(self.clone(), selector.to_string())
    }

    /// Locates an iframe to drill into with [`FrameLocator::locator`].
    pub fn frame_locator(&self, selector: &str) -> FrameLocator {
        FrameLocator { frame: self.clone(), prefix: selector.to_string() }
    }

    pub fn get_by_text(&self, text: &str) -> Locator {
        self.locator(&sel_text(text))
    }
    pub fn get_by_role(&self, role: &str, name: Option<&str>) -> Locator {
        self.locator(&sel_role(role, name))
    }
    pub fn get_by_label(&self, text: &str) -> Locator {
        self.locator(&sel_label(text))
    }
    pub fn get_by_placeholder(&self, text: &str) -> Locator {
        self.locator(&sel_placeholder(text))
    }
    pub fn get_by_alt_text(&self, text: &str) -> Locator {
        self.locator(&sel_alt(text))
    }
    pub fn get_by_title(&self, text: &str) -> Locator {
        self.locator(&sel_title(text))
    }
    pub fn get_by_test_id(&self, id: &str) -> Locator {
        self.locator(&sel_test_id(id))
    }
}

impl Locator {
    /// A locator for a descendant iframe.
    pub fn frame_locator(&self, selector: &str) -> FrameLocator {
        FrameLocator {
            frame: self.frame.clone(),
            prefix: format!("{} >> {}", self.selector, selector),
        }
    }

    pub fn get_by_text(&self, text: &str) -> Locator {
        self.locator(&sel_text(text))
    }
    pub fn get_by_role(&self, role: &str, name: Option<&str>) -> Locator {
        self.locator(&sel_role(role, name))
    }
    pub fn get_by_label(&self, text: &str) -> Locator {
        self.locator(&sel_label(text))
    }
    pub fn get_by_placeholder(&self, text: &str) -> Locator {
        self.locator(&sel_placeholder(text))
    }
    pub fn get_by_alt_text(&self, text: &str) -> Locator {
        self.locator(&sel_alt(text))
    }
    pub fn get_by_title(&self, text: &str) -> Locator {
        self.locator(&sel_title(text))
    }
    pub fn get_by_test_id(&self, id: &str) -> Locator {
        self.locator(&sel_test_id(id))
    }
}

impl Page {
    /// Creates a [`Locator`] rooted at the page's main frame.
    pub async fn locator(&self, selector: &str) -> Result<Locator> {
        Ok(self.main_frame().await?.locator(selector))
    }

    /// Locates an iframe to drill into with [`FrameLocator::locator`].
    pub async fn frame_locator(&self, selector: &str) -> Result<FrameLocator> {
        Ok(self.main_frame().await?.frame_locator(selector))
    }

    pub async fn get_by_text(&self, text: &str) -> Result<Locator> {
        Ok(self.main_frame().await?.get_by_text(text))
    }
    pub async fn get_by_role(&self, role: &str, name: Option<&str>) -> Result<Locator> {
        Ok(self.main_frame().await?.get_by_role(role, name))
    }
    pub async fn get_by_label(&self, text: &str) -> Result<Locator> {
        Ok(self.main_frame().await?.get_by_label(text))
    }
    pub async fn get_by_placeholder(&self, text: &str) -> Result<Locator> {
        Ok(self.main_frame().await?.get_by_placeholder(text))
    }
    pub async fn get_by_alt_text(&self, text: &str) -> Result<Locator> {
        Ok(self.main_frame().await?.get_by_alt_text(text))
    }
    pub async fn get_by_title(&self, text: &str) -> Result<Locator> {
        Ok(self.main_frame().await?.get_by_title(text))
    }
    pub async fn get_by_test_id(&self, id: &str) -> Result<Locator> {
        Ok(self.main_frame().await?.get_by_test_id(id))
    }
}

fn str_value(v: &Value) -> String {
    v.get("value").and_then(Value::as_str).unwrap_or_default().to_string()
}

fn bool_value(v: &Value) -> bool {
    v.get("value").and_then(Value::as_bool).unwrap_or(false)
}
