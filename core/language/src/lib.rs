//! Role: Provide Markdown completions and syntax classifications.
use fleximark_model::{
    SourceRange,
    syntax::{BUILTIN_SYNTAX, SyntaxRecognition},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticKind {
    Keyword,
    String,
    Operator,
    Type,
    Property,
}
const KEYWORD: SemanticKind = SemanticKind::Keyword;
const STRING: SemanticKind = SemanticKind::String;
const OPERATOR: SemanticKind = SemanticKind::Operator;
const TYPE: SemanticKind = SemanticKind::Type;
const PROPERTY: SemanticKind = SemanticKind::Property;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: String,
    pub filter_text: String,
    pub insert_text_format: InsertTextFormat,
    pub text_edit: CompletionTextEdit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionTextEdit {
    pub range: SourceRange,
    pub new_text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionKind {
    Snippet,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertTextFormat {
    PlainText,
    Snippet,
}

pub fn completion_items(
    line_prefix: &str,
    byte_offset: usize,
    snippet_support: bool,
) -> Vec<CompletionItem> {
    let trimmed = line_prefix.trim_start_matches([' ', '\t']);
    let snippets = BUILTIN_SYNTAX
        .iter()
        .filter(|descriptor| {
            trimmed.is_empty()
                || descriptor
                    .recognizers
                    .iter()
                    .any(|recognizer| match recognizer {
                        SyntaxRecognition::Directive(_) => trimmed.starts_with(':'),
                        SyntaxRecognition::Alert(_) => trimmed.starts_with('>'),
                        SyntaxRecognition::Fence(_) => trimmed.starts_with('`'),
                        SyntaxRecognition::Youtube => trimmed.starts_with("https:"),
                    })
        })
        .flat_map(|descriptor| descriptor.completions.iter())
        .collect::<Vec<_>>();
    let Some(line_start) = byte_offset.checked_sub(line_prefix.len()) else {
        return Vec::new();
    };
    let start = line_start + line_prefix.len() - trimmed.len();
    snippets
        .into_iter()
        .map(|snippet| CompletionItem {
            label: snippet.label.to_owned(),
            kind: CompletionKind::Snippet,
            detail: snippet.detail.to_owned(),
            filter_text: if snippet.label == "tabs" && trimmed.starts_with("::::") {
                "::::tabs"
            } else {
                snippet.filter_text
            }
            .to_owned(),
            insert_text_format: if snippet_support {
                InsertTextFormat::Snippet
            } else {
                InsertTextFormat::PlainText
            },
            text_edit: CompletionTextEdit {
                range: SourceRange {
                    byte_start: start as u64,
                    byte_end: byte_offset as u64,
                },
                new_text: if snippet_support {
                    snippet.snippet
                } else {
                    snippet.plain_text
                }
                .to_owned(),
            },
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticToken {
    pub range: SourceRange,
    pub kind: SemanticKind,
}

pub fn semantic_tokens(source: &str) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut line_offset = 0;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        // Markdown fences allow up to three leading spaces. A tab reaches column four.
        let permitted_indent = indent <= 3 && !line[..indent].contains('\t');
        let trimmed = &line[indent..];
        if let Some((marker, length)) = fence {
            if permitted_indent && trimmed.as_bytes().first() == Some(&marker) {
                if let Some(close_length) = fence_marker(trimmed) {
                    if close_length >= length && trimmed[close_length..].trim().is_empty() {
                        push_token(
                            &mut tokens,
                            line_offset,
                            line,
                            indent,
                            close_length,
                            OPERATOR,
                        );
                        fence = None;
                    }
                }
            }
        } else if permitted_indent {
            if let Some(length) = fence_marker(trimmed) {
                let marker = trimmed.as_bytes()[0];
                if marker != b'`' || !trimmed[length..].contains('`') {
                    collect_line_tokens(line_offset, line, &mut tokens);
                    fence = Some((marker, length));
                }
            } else {
                collect_line_tokens(line_offset, line, &mut tokens);
            }
        }
        line_offset += raw_line.len();
    }
    tokens
}

fn collect_line_tokens(line_offset: usize, line: &str, tokens: &mut Vec<SemanticToken>) {
    let indent_bytes = line.len() - line.trim_start_matches([' ', '\t']).len();
    let trimmed = &line[indent_bytes..];
    if let Some(marker) = directive_marker(trimmed) {
        push_token(tokens, line_offset, line, indent_bytes, marker, OPERATOR);
        let mut cursor = indent_bytes + marker;
        let name_len = line[cursor..]
            .chars()
            .take_while(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
            .map(char::len_utf8)
            .sum::<usize>();
        if name_len > 0 {
            push_token(tokens, line_offset, line, cursor, name_len, KEYWORD);
            cursor += name_len;
        }
        if let Some(open) = line[cursor..].find('[') {
            let start = cursor + open;
            if let Some(close) = line[start..].find(']') {
                push_token(tokens, line_offset, line, start, close + 1, STRING);
            }
        }
        return;
    }
    if let Some(marker) = fence_marker(trimmed) {
        push_token(tokens, line_offset, line, indent_bytes, marker, OPERATOR);
        let after_marker = &line[indent_bytes + marker..];
        let whitespace = after_marker.len() - after_marker.trim_start().len();
        let info_start = indent_bytes + marker + whitespace;
        let info = &line[info_start..];
        let language_len = info
            .chars()
            .take_while(|character| !character.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>();
        if language_len > 0 {
            push_token(tokens, line_offset, line, info_start, language_len, TYPE);
            let attributes = info[language_len..].trim_start();
            if !attributes.is_empty() {
                let attributes_start = line.len() - attributes.len();
                push_token(
                    tokens,
                    line_offset,
                    line,
                    attributes_start,
                    attributes.len(),
                    PROPERTY,
                );
            }
        }
        return;
    }
    let alert = trimmed.strip_prefix('>').map(str::trim_start);
    if let Some(alert) = alert {
        for name in BUILTIN_SYNTAX
            .iter()
            .flat_map(|descriptor| descriptor.recognizers)
            .filter_map(|recognizer| {
                if let SyntaxRecognition::Alert(name) = recognizer {
                    Some(*name)
                } else {
                    None
                }
            })
        {
            let marker = format!("[!{name}]");
            if alert.starts_with(&marker) {
                let start = line.len() - alert.len();
                push_token(tokens, line_offset, line, start, marker.len(), KEYWORD);
                break;
            }
        }
    }
}

fn directive_marker(value: &str) -> Option<usize> {
    let length = value
        .chars()
        .take_while(|character| *character == ':')
        .count();
    (length >= 3).then_some(length)
}

fn fence_marker(value: &str) -> Option<usize> {
    let marker = value.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let length = value
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (length >= 3).then_some(length)
}

fn push_token(
    tokens: &mut Vec<SemanticToken>,
    line_offset: usize,
    _line: &str,
    byte_start: usize,
    byte_length: usize,
    token_type: SemanticKind,
) {
    if byte_length == 0 {
        return;
    }
    tokens.push(SemanticToken {
        range: SourceRange {
            byte_start: (line_offset + byte_start) as u64,
            byte_end: (line_offset + byte_start + byte_length) as u64,
        },
        kind: token_type,
    });
}

#[cfg(test)]
mod tests;
