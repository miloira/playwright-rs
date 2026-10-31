//! Low-level input devices: [`Keyboard`], [`Mouse`] and [`Touchscreen`].
//!
//! These mirror the Python `page.keyboard` / `page.mouse` / `page.touchscreen`
//! helpers. Each dispatches the corresponding page-level protocol command.

use serde_json::json;

use crate::api::Page;
use crate::connection::Connection;
use crate::error::Result;

impl Page {
    /// The page's keyboard.
    pub fn keyboard(&self) -> Keyboard {
        Keyboard { conn: self.connection(), guid: self.guid.clone() }
    }

    /// The page's mouse.
    pub fn mouse(&self) -> Mouse {
        Mouse { conn: self.connection(), guid: self.guid.clone() }
    }

    /// The page's touchscreen.
    pub fn touchscreen(&self) -> Touchscreen {
        Touchscreen { conn: self.connection(), guid: self.guid.clone() }
    }
}

/// Raw keyboard input for a page.
#[derive(Clone)]
pub struct Keyboard {
    conn: Connection,
    guid: String,
}

impl Keyboard {
    /// Holds down `key` (e.g. `"Shift"`, `"a"`, `"ArrowLeft"`).
    pub async fn down(&self, key: &str) -> Result<()> {
        self.conn.send(&self.guid, "keyboardDown", json!({ "key": key })).await.map(drop)
    }

    /// Releases `key`.
    pub async fn up(&self, key: &str) -> Result<()> {
        self.conn.send(&self.guid, "keyboardUp", json!({ "key": key })).await.map(drop)
    }

    /// Inserts `text` verbatim without individual key events.
    pub async fn insert_text(&self, text: &str) -> Result<()> {
        self.conn
            .send(&self.guid, "keyboardInsertText", json!({ "text": text }))
            .await
            .map(drop)
    }

    /// Types `text`, emitting a keydown/keypress/keyup for each character.
    pub async fn type_text(&self, text: &str) -> Result<()> {
        self.conn.send(&self.guid, "keyboardType", json!({ "text": text })).await.map(drop)
    }

    /// Presses `key` (a full down+up), e.g. `"Enter"` or `"Control+A"`.
    pub async fn press(&self, key: &str) -> Result<()> {
        self.conn.send(&self.guid, "keyboardPress", json!({ "key": key })).await.map(drop)
    }
}

/// Raw mouse input for a page. Coordinates are in CSS pixels from the top-left.
#[derive(Clone)]
pub struct Mouse {
    conn: Connection,
    guid: String,
}

impl Mouse {
    /// Moves the mouse to `(x, y)`.
    pub async fn move_to(&self, x: f64, y: f64) -> Result<()> {
        self.conn.send(&self.guid, "mouseMove", json!({ "x": x, "y": y })).await.map(drop)
    }

    /// Presses the left mouse button.
    pub async fn down(&self) -> Result<()> {
        self.conn.send(&self.guid, "mouseDown", json!({})).await.map(drop)
    }

    /// Releases the left mouse button.
    pub async fn up(&self) -> Result<()> {
        self.conn.send(&self.guid, "mouseUp", json!({})).await.map(drop)
    }

    /// Moves to `(x, y)` and clicks.
    pub async fn click(&self, x: f64, y: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "mouseClick", json!({ "x": x, "y": y, "clickCount": 1 }))
            .await
            .map(drop)
    }

    /// Moves to `(x, y)` and double-clicks.
    pub async fn dblclick(&self, x: f64, y: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "mouseClick", json!({ "x": x, "y": y, "clickCount": 2 }))
            .await
            .map(drop)
    }

    /// Scrolls the mouse wheel by `(delta_x, delta_y)`.
    pub async fn wheel(&self, delta_x: f64, delta_y: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "mouseWheel", json!({ "deltaX": delta_x, "deltaY": delta_y }))
            .await
            .map(drop)
    }
}

/// Touchscreen input for a page (requires a touch-enabled context).
#[derive(Clone)]
pub struct Touchscreen {
    conn: Connection,
    guid: String,
}

impl Touchscreen {
    /// Taps at `(x, y)`.
    pub async fn tap(&self, x: f64, y: f64) -> Result<()> {
        self.conn
            .send(&self.guid, "touchscreenTap", json!({ "x": x, "y": y }))
            .await
            .map(drop)
    }
}
