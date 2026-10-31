//! The complete, generated Playwright protocol channel layer.
//!
//! Every interface, command, event and type in the Playwright protocol is
//! represented here. The contents are generated from the protocol spec by
//! `cargo run -p codegen`; see [`generated`].
//!
//! These are low-level channel wrappers: each holds a
//! [`Connection`](crate::connection::Connection) and a guid, and each command
//! maps directly to a protocol message. The ergonomic [`crate::api`] layer is
//! built on top of these.

mod generated;

pub use generated::*;
