//! The wire transport used to talk to the Playwright driver.
//!
//! Playwright frames each JSON message with a 4-byte little-endian length
//! prefix followed by the UTF-8 JSON payload. This module reads and writes
//! those frames over the driver process's stdio pipes.

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, Result};

/// The maximum allowed frame size (256 MiB), a sanity guard against a corrupt
/// length prefix.
const MAX_FRAME: u32 = 256 * 1024 * 1024;

/// Writes a single framed message to the driver.
pub async fn write_message<W>(writer: &mut W, message: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(message)?;
    let len = payload.len();
    if len as u64 > MAX_FRAME as u64 {
        return Err(Error::Protocol(format!("outbound frame too large: {len}")));
    }
    let mut buf = Vec::with_capacity(4 + len);
    buf.extend_from_slice(&(len as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads a single framed message from the driver.
///
/// Returns `Ok(None)` on a clean end-of-stream before any bytes of a new frame.
pub async fn read_message<R>(reader: &mut R) -> Result<Option<Value>>
where
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(Error::Io(e)),
    }
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME {
        return Err(Error::Protocol(format!("inbound frame too large: {len}")));
    }
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).await?;
    let value = serde_json::from_slice(&payload)?;
    Ok(Some(value))
}
