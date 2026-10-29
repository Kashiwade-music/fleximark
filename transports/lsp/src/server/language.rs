//! Role: Map byte-based language results to negotiated standard LSP payloads.
use super::lsp_types::Range;
use fleximark_application::ApplicationError;
use fleximark_language::{CompletionItem, CompletionKind, InsertTextFormat, SemanticKind};
use fleximark_model::{PositionEncoding, SourceIndex, SourceRange};

pub(super) use fleximark_protocol::SEMANTIC_TOKEN_TYPES;
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Completion {
    label: String,
    kind: u8,
    detail: String,
    filter_text: String,
    insert_text_format: u8,
    text_edit: TextEdit,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TextEdit {
    range: Range,
    new_text: String,
}
fn source_range(
    index: &SourceIndex<'_>,
    range: &SourceRange,
    encoding: PositionEncoding,
) -> Result<Range, ApplicationError> {
    let start = usize::try_from(range.byte_start).map_err(|_| ApplicationError::InvalidRange)?;
    let end = usize::try_from(range.byte_end).map_err(|_| ApplicationError::InvalidRange)?;
    if start > end {
        return Err(ApplicationError::InvalidRange);
    }
    Ok(Range {
        start: index
            .byte_to_position(start, encoding)
            .ok_or(ApplicationError::InvalidRange)?
            .into(),
        end: index
            .byte_to_position(end, encoding)
            .ok_or(ApplicationError::InvalidRange)?
            .into(),
    })
}
pub(super) fn completion(
    item: CompletionItem,
    index: &SourceIndex<'_>,
    encoding: PositionEncoding,
) -> Result<Completion, ApplicationError> {
    Ok(Completion {
        label: item.label,
        kind: match item.kind {
            CompletionKind::Snippet => 15,
        },
        detail: item.detail,
        filter_text: item.filter_text,
        insert_text_format: match item.insert_text_format {
            InsertTextFormat::PlainText => 1,
            InsertTextFormat::Snippet => 2,
        },
        text_edit: TextEdit {
            range: source_range(index, &item.text_edit.range, encoding)?,
            new_text: item.text_edit.new_text,
        },
    })
}
fn semantic_kind(kind: SemanticKind) -> u64 {
    match kind {
        SemanticKind::Keyword => 0,
        SemanticKind::String => 1,
        SemanticKind::Operator => 2,
        SemanticKind::Type => 3,
        SemanticKind::Property => 4,
    }
}
pub(super) fn semantic_data(
    source: &str,
    encoding: PositionEncoding,
) -> Result<Vec<u64>, ApplicationError> {
    let index = SourceIndex::new(source);
    let mut data = Vec::new();
    let (mut line, mut start) = (0, 0);
    for token in fleximark_language::semantic_tokens(source) {
        let range = source_range(&index, &token.range, encoding)?;
        if range.start.line != range.end.line {
            return Err(ApplicationError::InvalidRange);
        }
        let delta_line = range
            .start
            .line
            .checked_sub(line)
            .ok_or(ApplicationError::InvalidRange)?;
        let delta_start = if delta_line == 0 {
            range
                .start
                .character
                .checked_sub(start)
                .ok_or(ApplicationError::InvalidRange)?
        } else {
            range.start.character
        };
        data.extend([
            delta_line,
            delta_start,
            range.end.character - range.start.character,
            semantic_kind(token.kind),
            0,
        ]);
        line = range.start.line;
        start = range.start.character;
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    //! Role: Verify explicit wire kinds and byte-range validation at the LSP boundary.
    use super::*;
    #[test]
    fn semantic_kinds_map_explicitly_to_advertised_legend_entries() {
        for (kind, number, name) in [
            (SemanticKind::Keyword, 0, "keyword"),
            (SemanticKind::String, 1, "string"),
            (SemanticKind::Operator, 2, "operator"),
            (SemanticKind::Type, 3, "type"),
            (SemanticKind::Property, 4, "property"),
        ] {
            assert_eq!(semantic_kind(kind), number);
            assert_eq!(SEMANTIC_TOKEN_TYPES[number as usize], name);
        }
    }
    #[test]
    fn completion_rejects_ranges_inside_a_unicode_scalar() {
        let index = SourceIndex::new("\u{1f600}");
        let item = CompletionItem {
            label: "test".into(),
            kind: CompletionKind::Snippet,
            detail: String::new(),
            filter_text: String::new(),
            insert_text_format: InsertTextFormat::PlainText,
            text_edit: fleximark_language::CompletionTextEdit {
                range: SourceRange {
                    byte_start: 1,
                    byte_end: 4,
                },
                new_text: String::new(),
            },
        };
        assert!(matches!(
            completion(item, &index, PositionEncoding::Utf16),
            Err(ApplicationError::InvalidRange)
        ));
    }
}
