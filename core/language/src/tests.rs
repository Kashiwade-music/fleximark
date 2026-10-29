//! Role: Verify byte edits, descriptor completeness and fenced syntax classification.
use super::*;
fn token_texts(source: &str) -> Vec<(&str, SemanticKind)> {
    semantic_tokens(source)
        .iter()
        .map(|token| {
            (
                &source[token.range.byte_start as usize..token.range.byte_end as usize],
                token.kind,
            )
        })
        .collect()
}
#[test]
fn completion_catalog_replaces_the_typed_prefix() {
    let items = completion_items("  :::det", 108, true);
    assert_eq!(
        items.len(),
        BUILTIN_SYNTAX
            .iter()
            .filter(|descriptor| descriptor
                .recognizers
                .iter()
                .any(|recognizer| matches!(recognizer, SyntaxRecognition::Directive(_))))
            .map(|descriptor| descriptor.completions.len())
            .sum::<usize>()
    );
    assert!(items.iter().any(|item| item.label == "details with title"));
    assert_eq!(
        items[0].text_edit.range,
        SourceRange {
            byte_start: 102,
            byte_end: 108
        }
    );
    assert_eq!(items[0].insert_text_format, InsertTextFormat::Snippet);
}
#[test]
fn clients_without_snippet_support_receive_plain_text() {
    let items = completion_items("```abc", 6, false);
    assert_eq!(items[0].insert_text_format, InsertTextFormat::PlainText);
    assert!(!items[0].text_edit.new_text.contains("${"));
}
#[test]
fn github_alert_completions_preserve_snippets_and_byte_edits() {
    let items = completion_items("> [!", 104, true);
    assert_eq!(items.len(), 5);
    assert_eq!(items[0].label, "alert note");
    assert_eq!(items[0].filter_text, "> [!NOTE]");
    assert_eq!(
        items[0].text_edit.range,
        SourceRange {
            byte_start: 100,
            byte_end: 104
        }
    );
    assert_eq!(items[0].text_edit.new_text, "> [!NOTE]\n> ${1:Content}");
}
#[test]
fn semantic_tokens_cover_directives_fences_and_alerts() {
    let source = ":::warning[\u{6ce8}\u{610f}]\n```mermaid title=Flow\n```\n> [!NOTE] text\n";
    let data = semantic_tokens(source);
    assert_eq!(data.len(), 8);
    assert_eq!(
        data[0],
        SemanticToken {
            range: SourceRange {
                byte_start: 0,
                byte_end: 3
            },
            kind: OPERATOR
        }
    );
    assert_eq!(
        data[1],
        SemanticToken {
            range: SourceRange {
                byte_start: 3,
                byte_end: 10
            },
            kind: KEYWORD
        }
    );
    assert_eq!(
        data[2],
        SemanticToken {
            range: SourceRange {
                byte_start: 10,
                byte_end: 18
            },
            kind: STRING
        }
    );
    assert_eq!(
        data[3],
        SemanticToken {
            range: SourceRange {
                byte_start: 19,
                byte_end: 22
            },
            kind: OPERATOR
        }
    );
}
#[test]
fn every_descriptor_completion_preserves_snippet_and_plain_text() {
    for descriptor in BUILTIN_SYNTAX {
        for expected in descriptor.completions {
            for support in [false, true] {
                let items =
                    completion_items(expected.filter_text, expected.filter_text.len(), support);
                let item = items
                    .iter()
                    .find(|item| item.label == expected.label)
                    .unwrap_or_else(|| panic!("missing {}", expected.label));
                assert_eq!(item.detail, expected.detail);
                assert_eq!(
                    item.text_edit.new_text,
                    if support {
                        expected.snippet
                    } else {
                        expected.plain_text
                    }
                );
            }
        }
    }
}
#[test]
fn completion_offsets_include_unicode_and_crlf_on_previous_lines() {
    let previous = "\u{1f600}\r\n";
    let prefix = "\t :::det";
    let items = completion_items(prefix, previous.len() + prefix.len(), true);
    assert_eq!(
        items[0].text_edit.range,
        SourceRange {
            byte_start: 8,
            byte_end: 14
        }
    );
    assert!(completion_items(prefix, 1, true).is_empty());
}
#[test]
fn code_fences_suppress_directives_alerts_and_short_or_mismatched_closers() {
    let source = ":::outside\n````rust\n:::hidden\n```\n~~~\n> [!NOTE]\n`````  \n:::after\n";
    assert_eq!(
        token_texts(source),
        vec![
            (":::", OPERATOR),
            ("outside", KEYWORD),
            ("````", OPERATOR),
            ("rust", TYPE),
            ("`````", OPERATOR),
            (":::", OPERATOR),
            ("after", KEYWORD)
        ]
    );
}
#[test]
fn tilde_fences_honor_lengths_and_zero_to_three_space_indentation() {
    let source = "   ~~~~ text\r\n:::hidden\r\n   ~~~\r\n    ~~~~\r\n  ~~~~~\t\r\n:::after\r\n";
    assert_eq!(
        token_texts(source),
        vec![
            ("~~~~", OPERATOR),
            ("text", TYPE),
            ("~~~~~", OPERATOR),
            (":::", OPERATOR),
            ("after", KEYWORD)
        ]
    );
    for source in [
        "    ```rust\n:::visible\n",
        "\t~~~rust\n:::visible\n",
        "```bad`info\n:::visible\n",
    ] {
        assert_eq!(
            token_texts(source),
            vec![(":::", OPERATOR), ("visible", KEYWORD)]
        );
    }
}
#[test]
fn unclosed_fence_keeps_directives_as_code_until_end_of_document() {
    assert_eq!(
        token_texts("~~~rust\n:::hidden\n```\n> [!NOTE]"),
        vec![("~~~", OPERATOR), ("rust", TYPE)]
    );
}
