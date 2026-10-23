//! Role: Translate transport positions through the shared source index.
use crate::requests::Position;
use fleximark_model::{PositionEncoding, SourceIndex, SourcePosition};
#[cfg(test)]
pub fn content_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}
pub(crate) fn position_offset(
    text: &str,
    position: Position,
    encoding: PositionEncoding,
) -> Option<usize> {
    SourceIndex::new(text).position_to_byte(&SourcePosition {
        line: position.line.into(),
        character: position.character.into(),
        encoding,
    })
}
