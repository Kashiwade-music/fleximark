use fleximark_model::PositionEncoding;
use serde::Serialize;

use crate::Position;

pub const SEMANTIC_TOKEN_TYPES: [&str; 5] = ["keyword", "string", "operator", "type", "property"];

const KEYWORD: u32 = 0;
const STRING: u32 = 1;
const OPERATOR: u32 = 2;
const TYPE: u32 = 3;
const PROPERTY: u32 = 4;

#[derive(Clone, Copy)]
struct Snippet {
    label: &'static str,
    filter_text: &'static str,
    detail: &'static str,
    snippet: &'static str,
    plain_text: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    pub kind: u32,
    pub detail: String,
    pub filter_text: String,
    pub insert_text_format: u32,
    pub text_edit: CompletionTextEdit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionTextEdit {
    pub range: CompletionRange,
    pub new_text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CompletionRange {
    pub start: Position,
    pub end: Position,
}

const DIRECTIVE_SNIPPETS: &[Snippet] = &[
    Snippet {
        label: "admonition",
        filter_text: ":::admonition",
        detail: "Admonition block",
        snippet: ":::${1|info,tip,important,warning,danger|}\n\n${2:Content}\n\n:::",
        plain_text: ":::info\n\nContent\n\n:::",
    },
    Snippet {
        label: "admonition with title",
        filter_text: ":::admonition",
        detail: "Admonition block with title",
        snippet: ":::${1|info,tip,important,warning,danger|}[${2:Title}]\n\n${3:Content}\n\n:::",
        plain_text: ":::info[Title]\n\nContent\n\n:::",
    },
    Snippet {
        label: "info",
        filter_text: ":::info",
        detail: "Info block",
        snippet: ":::info\n\n${1:Content}\n\n:::",
        plain_text: ":::info\n\nContent\n\n:::",
    },
    Snippet {
        label: "info with title",
        filter_text: ":::info",
        detail: "Info block with title",
        snippet: ":::info[${1:Title}]\n\n${2:Content}\n\n:::",
        plain_text: ":::info[Title]\n\nContent\n\n:::",
    },
    Snippet {
        label: "tip",
        filter_text: ":::tip",
        detail: "Tip block",
        snippet: ":::tip\n\n${1:Content}\n\n:::",
        plain_text: ":::tip\n\nContent\n\n:::",
    },
    Snippet {
        label: "tip with title",
        filter_text: ":::tip",
        detail: "Tip block with title",
        snippet: ":::tip[${1:Title}]\n\n${2:Content}\n\n:::",
        plain_text: ":::tip[Title]\n\nContent\n\n:::",
    },
    Snippet {
        label: "important",
        filter_text: ":::important",
        detail: "Important block",
        snippet: ":::important\n\n${1:Content}\n\n:::",
        plain_text: ":::important\n\nContent\n\n:::",
    },
    Snippet {
        label: "important with title",
        filter_text: ":::important",
        detail: "Important block with title",
        snippet: ":::important[${1:Title}]\n\n${2:Content}\n\n:::",
        plain_text: ":::important[Title]\n\nContent\n\n:::",
    },
    Snippet {
        label: "warning",
        filter_text: ":::warning",
        detail: "Warning block",
        snippet: ":::warning\n\n${1:Content}\n\n:::",
        plain_text: ":::warning\n\nContent\n\n:::",
    },
    Snippet {
        label: "warning with title",
        filter_text: ":::warning",
        detail: "Warning block with title",
        snippet: ":::warning[${1:Title}]\n\n${2:Content}\n\n:::",
        plain_text: ":::warning[Title]\n\nContent\n\n:::",
    },
    Snippet {
        label: "danger",
        filter_text: ":::danger",
        detail: "Danger block",
        snippet: ":::danger\n\n${1:Content}\n\n:::",
        plain_text: ":::danger\n\nContent\n\n:::",
    },
    Snippet {
        label: "danger with title",
        filter_text: ":::danger",
        detail: "Danger block with title",
        snippet: ":::danger[${1:Title}]\n\n${2:Content}\n\n:::",
        plain_text: ":::danger[Title]\n\nContent\n\n:::",
    },
    Snippet {
        label: "tabs",
        filter_text: ":::tabs",
        detail: "Tabs block",
        snippet: "::::tabs\n\n  :::tab[${1:Tab 1}]\n\n  ${2:Content}\n\n  :::\n\n::::",
        plain_text: "::::tabs\n\n  :::tab[Tab 1]\n\n  Content\n\n  :::\n\n::::",
    },
    Snippet {
        label: "details",
        filter_text: ":::details",
        detail: "Details block",
        snippet: ":::details\n\n${1:Content}\n\n:::",
        plain_text: ":::details\n\nContent\n\n:::",
    },
    Snippet {
        label: "details with title",
        filter_text: ":::details",
        detail: "Details block with title",
        snippet: ":::details[${1:Title}]\n\n${2:Content}\n\n:::",
        plain_text: ":::details[Title]\n\nContent\n\n:::",
    },
];

const ALERT_SNIPPETS: &[Snippet] = &[
    Snippet {
        label: "alert note",
        filter_text: "> [!NOTE]",
        detail: "GitHub NOTE alert",
        snippet: "> [!NOTE]\n> ${1:Content}",
        plain_text: "> [!NOTE]\n> Content",
    },
    Snippet {
        label: "alert tip",
        filter_text: "> [!TIP]",
        detail: "GitHub TIP alert",
        snippet: "> [!TIP]\n> ${1:Content}",
        plain_text: "> [!TIP]\n> Content",
    },
    Snippet {
        label: "alert important",
        filter_text: "> [!IMPORTANT]",
        detail: "GitHub IMPORTANT alert",
        snippet: "> [!IMPORTANT]\n> ${1:Content}",
        plain_text: "> [!IMPORTANT]\n> Content",
    },
    Snippet {
        label: "alert warning",
        filter_text: "> [!WARNING]",
        detail: "GitHub WARNING alert",
        snippet: "> [!WARNING]\n> ${1:Content}",
        plain_text: "> [!WARNING]\n> Content",
    },
    Snippet {
        label: "alert caution",
        filter_text: "> [!CAUTION]",
        detail: "GitHub CAUTION alert",
        snippet: "> [!CAUTION]\n> ${1:Content}",
        plain_text: "> [!CAUTION]\n> Content",
    },
];

const FENCE_SNIPPETS: &[Snippet] = &[
    Snippet {
        label: "abc",
        filter_text: "```abc",
        detail: "ABC notation template",
        snippet: "```abc\nX:1\nT:${1:Title}\nM:${2|4/4,3/4,6/8,12/8,2/2|}\nL:${3|1/8,1/4,1/16|}\nK:${4:C}\nQ:${5:120}\nV:1\n\n${6}\n```",
        plain_text: "```abc\nX:1\nT:Title\nM:4/4\nL:1/8\nK:C\nQ:120\nV:1\n\n\n```",
    },
    Snippet {
        label: "abc piano",
        filter_text: "```abc piano",
        detail: "Piano ABC notation template",
        snippet: "```abc\nX:1\nT:${1:Title}\nM:${2|4/4,3/4,6/8,12/8,2/2|}\nL:${3|1/4,1/8,1/16|}\nK:${4:C}\nQ:${5:120}\n%%staves {(rh) (lh)}\nV:rh clef=treble name=\"Piano\" snm=\"Pno\"\nV:lh clef=bass octave=-2\n% Start of the melody\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n% next line\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n```",
        plain_text: "```abc\nX:1\nT:Title\nM:4/4\nL:1/4\nK:C\nQ:120\n%%staves {(rh) (lh)}\nV:rh clef=treble name=\"Piano\" snm=\"Pno\"\nV:lh clef=bass octave=-2\n% Start of the melody\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n% next line\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n```",
    },
    Snippet {
        label: "abc piano-vocal",
        filter_text: "```abc piano-vocal",
        detail: "Piano and vocal ABC notation template",
        snippet: "```abc\nX:1\nT:${1:Title}\nM:${2|4/4,3/4,6/8,12/8,2/2|}\nL:${3|1/4,1/8,1/16|}\nK:${4:C}\nQ:${5:120}\n%%staves (ml) {(rh) (lh)}\nV:ml clef=treble name=\"Melody\" snm=\"M\"\n%%MIDI program 80\nV:rh clef=treble name=\"Chord\" snm=\"C\"\n%%MIDI program 0\nV:lh clef=bass octave=-2\n% Start of the melody\n[V:ml]\nC D E F | G z G z | | |\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n% next line\n[V:ml]\nC D E F | G z G z | | |\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n```",
        plain_text: "```abc\nX:1\nT:Title\nM:4/4\nL:1/4\nK:C\nQ:120\n%%staves (ml) {(rh) (lh)}\nV:ml clef=treble name=\"Melody\" snm=\"M\"\n%%MIDI program 80\nV:rh clef=treble name=\"Chord\" snm=\"C\"\n%%MIDI program 0\nV:lh clef=bass octave=-2\n% Start of the melody\n[V:ml]\nC D E F | G z G z | | |\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n% next line\n[V:ml]\nC D E F | G z G z | | |\n[V:rh]\nC E G C | E z E z | | |\n[V:lh]\nC G c e | f z g z | | |\n```",
    },
    Snippet {
        label: "abc satb",
        filter_text: "```abc satb",
        detail: "SATB ABC notation template",
        snippet: "```abc\nX:1\nT:${1:Title}\nM:${2|4/4,3/4,6/8,12/8,2/2|}\nL:${3|1/4,1/8,1/16|}\nK:${4:C}\nQ:${5:120}\n%%staves { (S A) (T B) }\nV:S name=\"Soprano\" snm=\"S\" clef=treble\nV:A name=\"Alto\" snm=\"A\" clef=treble\nV:T name=\"Tenor\" snm=\"T\" clef=bass octave=-1\nV:B name=\"Bass\" snm=\"B\" clef=bass octave=-1\n% Start of the melody\n[V:S]\nE F G A | B c d e | | |\n[V:A]\nC D E F | G A B c | | |\n[V:T]\nG A B c | d e f g | | |\n[V:B]\nC D E F | G A B c | | |\n% next line\n[V:S]\nE F G A | B c d e | | |\n[V:A]\nC D E F | G A B c | | |\n[V:T]\nG A B c | d e f g | | |\n[V:B]\nC D E F | G A B c | | |\n```",
        plain_text: "```abc\nX:1\nT:Title\nM:4/4\nL:1/4\nK:C\nQ:120\n%%staves { (S A) (T B) }\nV:S name=\"Soprano\" snm=\"S\" clef=treble\nV:A name=\"Alto\" snm=\"A\" clef=treble\nV:T name=\"Tenor\" snm=\"T\" clef=bass octave=-1\nV:B name=\"Bass\" snm=\"B\" clef=bass octave=-1\n% Start of the melody\n[V:S]\nE F G A | B c d e | | |\n[V:A]\nC D E F | G A B c | | |\n[V:T]\nG A B c | d e f g | | |\n[V:B]\nC D E F | G A B c | | |\n% next line\n[V:S]\nE F G A | B c d e | | |\n[V:A]\nC D E F | G A B c | | |\n[V:T]\nG A B c | d e f g | | |\n[V:B]\nC D E F | G A B c | | |\n```",
    },
    Snippet {
        label: "mermaid",
        filter_text: "```mermaid",
        detail: "Mermaid diagram",
        snippet: "```mermaid\n${1:graph TD}\n```",
        plain_text: "```mermaid\ngraph TD\n```",
    },
    Snippet {
        label: "math",
        filter_text: "```math",
        detail: "Display math block",
        snippet: "```math\n${1:formula}\n```",
        plain_text: "```math\nformula\n```",
    },
];

pub fn completion_items(
    line_prefix: &str,
    position: Position,
    encoding: PositionEncoding,
    snippet_support: bool,
) -> Vec<CompletionItem> {
    let trimmed = line_prefix.trim_start_matches([' ', '\t']);
    let snippets = if trimmed.is_empty() {
        DIRECTIVE_SNIPPETS
            .iter()
            .chain(ALERT_SNIPPETS)
            .chain(FENCE_SNIPPETS)
            .collect::<Vec<_>>()
    } else if trimmed.starts_with(':') {
        DIRECTIVE_SNIPPETS.iter().collect()
    } else if trimmed.starts_with('>') {
        ALERT_SNIPPETS.iter().collect()
    } else if trimmed.starts_with('`') {
        FENCE_SNIPPETS.iter().collect()
    } else {
        return Vec::new();
    };
    let start = Position {
        line: position.line,
        character: encoded_len(&line_prefix[..line_prefix.len() - trimmed.len()], encoding),
    };
    snippets
        .into_iter()
        .map(|snippet| CompletionItem {
            label: snippet.label.to_owned(),
            kind: 15,
            detail: snippet.detail.to_owned(),
            filter_text: if snippet.label == "tabs" && trimmed.starts_with("::::") {
                "::::tabs"
            } else {
                snippet.filter_text
            }
            .to_owned(),
            insert_text_format: if snippet_support { 2 } else { 1 },
            text_edit: CompletionTextEdit {
                range: CompletionRange {
                    start,
                    end: position,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AbsoluteSemanticToken {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
}

pub fn semantic_token_data(source: &str, encoding: PositionEncoding) -> Vec<u32> {
    let mut tokens = Vec::new();
    for (line_number, line) in source.lines().enumerate() {
        collect_line_tokens(line_number as u32, line, encoding, &mut tokens);
    }
    let mut data = Vec::with_capacity(tokens.len() * 5);
    let mut previous_line = 0;
    let mut previous_start = 0;
    for token in tokens {
        let delta_line = token.line - previous_line;
        let delta_start = if delta_line == 0 {
            token.start - previous_start
        } else {
            token.start
        };
        data.extend([delta_line, delta_start, token.length, token.token_type, 0]);
        previous_line = token.line;
        previous_start = token.start;
    }
    data
}

fn collect_line_tokens(
    line_number: u32,
    line: &str,
    encoding: PositionEncoding,
    tokens: &mut Vec<AbsoluteSemanticToken>,
) {
    let indent_bytes = line.len() - line.trim_start_matches([' ', '\t']).len();
    let trimmed = &line[indent_bytes..];
    if let Some(marker) = directive_marker(trimmed) {
        push_token(
            tokens,
            line_number,
            line,
            indent_bytes,
            marker,
            OPERATOR,
            encoding,
        );
        let mut cursor = indent_bytes + marker;
        let name_len = line[cursor..]
            .chars()
            .take_while(|character| character.is_alphanumeric() || matches!(character, '-' | '_'))
            .map(char::len_utf8)
            .sum::<usize>();
        if name_len > 0 {
            push_token(
                tokens,
                line_number,
                line,
                cursor,
                name_len,
                KEYWORD,
                encoding,
            );
            cursor += name_len;
        }
        if let Some(open) = line[cursor..].find('[') {
            let start = cursor + open;
            if let Some(close) = line[start..].find(']') {
                push_token(
                    tokens,
                    line_number,
                    line,
                    start,
                    close + 1,
                    STRING,
                    encoding,
                );
            }
        }
        return;
    }
    if let Some(marker) = fence_marker(trimmed) {
        push_token(
            tokens,
            line_number,
            line,
            indent_bytes,
            marker,
            OPERATOR,
            encoding,
        );
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
            push_token(
                tokens,
                line_number,
                line,
                info_start,
                language_len,
                TYPE,
                encoding,
            );
            let attributes = info[language_len..].trim_start();
            if !attributes.is_empty() {
                let attributes_start = line.len() - attributes.len();
                push_token(
                    tokens,
                    line_number,
                    line,
                    attributes_start,
                    attributes.len(),
                    PROPERTY,
                    encoding,
                );
            }
        }
        return;
    }
    let alert = trimmed.strip_prefix('>').map(str::trim_start);
    if let Some(alert) = alert {
        for name in ["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"] {
            let marker = format!("[!{name}]");
            if alert.starts_with(&marker) {
                let start = line.len() - alert.len();
                push_token(
                    tokens,
                    line_number,
                    line,
                    start,
                    marker.len(),
                    KEYWORD,
                    encoding,
                );
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
    tokens: &mut Vec<AbsoluteSemanticToken>,
    line_number: u32,
    line: &str,
    byte_start: usize,
    byte_length: usize,
    token_type: u32,
    encoding: PositionEncoding,
) {
    if byte_length == 0 {
        return;
    }
    tokens.push(AbsoluteSemanticToken {
        line: line_number,
        start: encoded_len(&line[..byte_start], encoding),
        length: encoded_len(&line[byte_start..byte_start + byte_length], encoding),
        token_type,
    });
}

fn encoded_len(value: &str, encoding: PositionEncoding) -> u32 {
    match encoding {
        PositionEncoding::Utf8 => value.len() as u32,
        PositionEncoding::Utf16 => value.encode_utf16().count() as u32,
        PositionEncoding::Utf32 => value.chars().count() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_catalog_replaces_the_typed_prefix() {
        let items = completion_items(
            "  :::det",
            Position {
                line: 4,
                character: 8,
            },
            PositionEncoding::Utf16,
            true,
        );
        assert_eq!(items.len(), DIRECTIVE_SNIPPETS.len());
        assert!(items.iter().any(|item| item.label == "details with title"));
        assert_eq!(items[0].text_edit.range.start.character, 2);
        assert_eq!(items[0].text_edit.range.end.character, 8);
        assert_eq!(items[0].insert_text_format, 2);
    }

    #[test]
    fn clients_without_snippet_support_receive_plain_text() {
        let items = completion_items(
            "```abc",
            Position {
                line: 0,
                character: 6,
            },
            PositionEncoding::Utf16,
            false,
        );
        assert_eq!(items[0].insert_text_format, 1);
        assert!(!items[0].text_edit.new_text.contains("${"));
    }

    #[test]
    fn github_alert_completions_are_lsp_snippets() {
        let items = completion_items(
            "> [!",
            Position {
                line: 2,
                character: 4,
            },
            PositionEncoding::Utf16,
            true,
        );
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].label, "alert note");
        assert_eq!(items[0].filter_text, "> [!NOTE]");
        assert_eq!(items[0].text_edit.range.start.character, 0);
        assert_eq!(items[0].text_edit.range.end.character, 4);
        assert_eq!(items[0].text_edit.new_text, "> [!NOTE]\n> ${1:Content}");
    }

    #[test]
    fn semantic_tokens_cover_directives_fences_and_alerts() {
        let data = semantic_token_data(
            ":::warning[注意]\n```mermaid title=Flow\n```\n> [!NOTE] text\n",
            PositionEncoding::Utf16,
        );
        assert_eq!(data.len(), 8 * 5);
        assert_eq!(&data[0..5], &[0, 0, 3, OPERATOR, 0]);
        assert_eq!(&data[5..10], &[0, 3, 7, KEYWORD, 0]);
        assert_eq!(&data[10..15], &[0, 7, 4, STRING, 0]);
        assert_eq!(&data[15..20], &[1, 0, 3, OPERATOR, 0]);
    }
}
