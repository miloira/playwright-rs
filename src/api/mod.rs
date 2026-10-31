//! The ergonomic Playwright API.
//!
//! Every protocol interface has a typed, snake_case wrapper here (generated in
//! [`generated`] from the protocol spec), with channel-typed return values,
//! unwrapped single-field results and typed event streams. Hand-written
//! convenience methods (in `ext`) add the Python-style niceties: the
//! [`Playwright`] entry point, page methods that delegate to the main frame,
//! `evaluate` with JSON values, and so on.

mod assertions;
mod events;
mod ext;
mod extras;
mod generated;
mod input;
mod locator;
mod route;

pub use assertions::{
    expect, expect_page, expect_response, ApiResponseAssertions, LocatorAssertions, PageAssertions,
};
pub use events::{ConsoleMessage, Download, FileChooser};
pub use extras::{Clock, WebSocketFrame};
pub use generated::*;
pub use input::{Keyboard, Mouse, Touchscreen};
pub use locator::{BoundingBox, FrameLocator, Locator};
