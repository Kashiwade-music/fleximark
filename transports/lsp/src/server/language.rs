//! Role: Encode language features as standard LSP values.
use super::state::*;

use fleximark_language::{CompletionItem, InsertTextFormat, SemanticKind};

pub(super) fn completion(item: CompletionItem) -> Value {
    let format = match item.insert_text_format {
        InsertTextFormat::PlainText => 1,
        InsertTextFormat::Snippet => 2,
    };
    json!({"label":item.label,"kind":15,"detail":item.detail,"filterText":item.filter_text,"insertTextFormat":format,
        "textEdit":{"range":item.text_edit.range,"newText":item.text_edit.new_text}})
}

pub(super) fn semantic_data(source: &str, encoding: PositionEncoding) -> Vec<u64> {
    let mut data = Vec::new();
    let (mut line, mut start) = (0, 0);
    for token in fleximark_language::semantic_tokens(source, encoding) {
        let kind = match token.token_type {
            SemanticKind::Keyword => 0,
            SemanticKind::String => 1,
            SemanticKind::Operator => 2,
            SemanticKind::Type => 3,
            SemanticKind::Property => 4,
        };
        let delta_line = token.line - line;
        let delta_start = if delta_line == 0 {
            token.start - start
        } else {
            token.start
        };
        data.extend([delta_line, delta_start, token.length, kind, 0]);
        line = token.line;
        start = token.start;
    }
    data
}
