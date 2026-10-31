use std::fmt;

/// The result type used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced by the Playwright bindings.
#[derive(Debug)]
pub enum Error {
    /// An I/O error, typically from the driver process or transport.
    Io(std::io::Error),
    /// A (de)serialization error for a protocol message.
    Serde(serde_json::Error),
    /// The driver reported an error in response to a command.
    ///
    /// This mirrors Playwright's server-side errors (e.g. a failed navigation
    /// or a selector that never resolved).
    Driver {
        /// Short error name, e.g. `TimeoutError`.
        name: String,
        /// Human readable message.
        message: String,
        /// Server-side stack trace, when provided.
        stack: String,
    },
    /// The connection to the driver was closed while a command was in flight.
    ConnectionClosed,
    /// A referenced remote object was not found in the local registry.
    ObjectNotFound(String),
    /// A protocol message did not have the expected shape.
    Protocol(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Serde(e) => write!(f, "serialization error: {e}"),
            Error::Driver { name, message, .. } => {
                if name.is_empty() {
                    write!(f, "driver error: {message}")
                } else {
                    write!(f, "{name}: {message}")
                }
            }
            Error::ConnectionClosed => write!(f, "connection to the driver was closed"),
            Error::ObjectNotFound(guid) => write!(f, "remote object not found: {guid}"),
            Error::Protocol(msg) => write!(f, "protocol error: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Serde(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Serde(e)
    }
}
