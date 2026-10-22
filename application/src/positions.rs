//! Role: Hash document text and translate native positions to UTF-8 byte offsets.
use crate::requests::Position;
use fleximark_model::PositionEncoding;
use sha2::{Digest, Sha256};
pub fn content_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub(crate) fn position_offset(
    text: &str,
    position: Position,
    encoding: PositionEncoding,
) -> Option<usize> {
    let mut line_start = 0;
    for _ in 0..position.line {
        let newline = text[line_start..].find('\n')?;
        line_start += newline + 1;
    }
    let mut line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |index| line_start + index);
    if line_end > line_start && text.as_bytes()[line_end - 1] == b'\r' {
        line_end -= 1;
    }
    let line = &text[line_start..line_end];
    let target = usize::try_from(position.character).ok()?;
    match encoding {
        PositionEncoding::Utf8 => {
            if target <= line.len() && line.is_char_boundary(target) {
                Some(line_start + target)
            } else {
                None
            }
        }
        PositionEncoding::Utf16 | PositionEncoding::Utf32 => {
            let mut units = 0;
            for (offset, character) in line.char_indices() {
                if units == target {
                    return Some(line_start + offset);
                }
                units += if encoding == PositionEncoding::Utf16 {
                    character.len_utf16()
                } else {
                    1
                };
                if units > target {
                    return None;
                }
            }
            if units == target {
                Some(line_end)
            } else {
                None
            }
        }
    }
}
