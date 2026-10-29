//! Role: Define the sole current wire contract independently of processing internals.
use serde::Serialize;
use std::io::{self, BufRead, Write};
mod command_values;
pub use command_values::{CreatePreviewResult, PreviewTarget};
mod commands;
pub use commands::*;
mod lsp_values;
pub use lsp_values::*;
mod registry;
mod rendering;
mod value_validation;
pub use fleximark_wire::{JsSafeI64, JsSafeU64, MAX_SAFE_INTEGER};
pub use registry::*;
pub use rendering::*;
mod wire_values;
pub use wire_values::*;
pub mod error_code;
/// Limits incoming control messages; outgoing render frames may contain large assets.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    Serialization(serde_json::Error),
    MalformedHeader,
    MissingContentLength,
    DuplicateContentLength,
    MessageTooLarge,
}

pub fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, FrameError> {
    let mut content_length = None;
    let mut saw_header = false;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return if saw_header {
                Err(FrameError::MalformedHeader)
            } else {
                Ok(None)
            };
        }
        saw_header = true;
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(FrameError::MalformedHeader)?;
        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(FrameError::DuplicateContentLength);
            }
            let length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| FrameError::MalformedHeader)?;
            if length > MAX_MESSAGE_BYTES {
                return Err(FrameError::MessageTooLarge);
            }
            content_length = Some(length);
        }
    }
    let length = content_length.ok_or(FrameError::MissingContentLength)?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), FrameError> {
    let body = serde_json::to_vec(value).map_err(FrameError::Serialization)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Serialization(error) => write!(f, "protocol serialization failed: {error}"),
            Self::MalformedHeader => f.write_str("malformed protocol header"),
            Self::MissingContentLength => f.write_str("missing Content-Length header"),
            Self::DuplicateContentLength => f.write_str("duplicate Content-Length header"),
            Self::MessageTooLarge => write!(
                f,
                "message is larger than the {MAX_MESSAGE_BYTES} byte limit"
            ),
        }
    }
}
impl std::error::Error for FrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}
impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "contract-schema")]
mod schema;
