//! Rust bindings for [Microsoft Playwright](https://playwright.dev).
//!
//! These bindings drive the same Node.js "driver" that the official Python,
//! Java and .NET bindings use. The driver is launched as a child process and
//! controlled over a length-prefixed JSON protocol; this crate exposes a typed,
//! async API over tokio.
//!
//! # Prerequisites
//!
//! The Playwright driver and browser binaries must be available. The simplest
//! way during development:
//!
//! ```text
//! mkdir driver && cd driver
//! npm init -y
//! npm install playwright-core
//! node node_modules/playwright-core/cli.js install chromium
//! ```
//!
//! The driver is located automatically (see [`driver::Driver::locate`]); set
//! `PLAYWRIGHT_CLI_JS` to override.
//!
//! # Example
//!
//! ```no_run
//! use playwright::Playwright;
//!
//! #[tokio::main]
//! async fn main() -> playwright::Result<()> {
//!     let pw = Playwright::initialize().await?;
//!     let chromium = pw.chromium().await?;
//!     let browser = chromium.launch(Default::default()).await?;
//!     let page = browser.new_page().await?;
//!     page.goto("https://example.com").await?;
//!     println!("title: {}", page.title().await?);
//!     browser.close(Default::default()).await?;
//!     pw.stop().await;
//!     Ok(())
//! }
//! ```

pub mod api;
pub mod connection;
pub mod driver;
pub mod error;
pub mod protocol;
pub mod serialized_value;
pub mod transport;

pub use api::{
    expect, expect_page, expect_response, ApiResponseAssertions, BoundingBox, Browser,
    BrowserContext, BrowserType, Clock, ConsoleMessage, Dialog, Download, FileChooser, Frame,
    FrameLocator, Keyboard, Locator, LocatorAssertions, Mouse, Page, PageAssertions, Playwright,
    Request, Response, Route, Touchscreen, Tracing, WebSocket, WebSocketFrame,
};
pub use error::{Error, Result};
