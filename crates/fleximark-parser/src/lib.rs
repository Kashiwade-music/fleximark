use std::collections::BTreeMap;

use comrak::{
    Arena, Options,
    nodes::{AstNode, ListType, NodeValue, Sourcepos},
    parse_document,
};
use fleximark_model::{
    Block, BlockKind, DOCUMENT_SCHEMA_VERSION, Document, DocumentMetadata, DocumentUri, Inline,
    InlineKind, JsSafeU64, Node, NodeId, PositionEncoding, SourcePosition, SourceProvenance,
    SourceRange, ValidationError,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("the parser returned an invalid source position: {0}")]
    InvalidSourcePosition(String),
    #[error(transparent)]
    InvalidDocument(#[from] ValidationError),
}

pub fn parse(
    uri: DocumentUri,
    document_version: u64,
    source: &str,
) -> Result<Document, ParseError> {
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.alerts = true;
    options.extension.block_directive = true;
    options.extension.math_dollars = true;
    options.extension.math_code = true;
    options.extension.front_matter_delimiter = Some("---".to_owned());

    let root = parse_document(&arena, source, &options);
    let positions = SourceIndex::new(source);
    let mut next_id = 0;
    let mut metadata = DocumentMetadata::default();
    let mut blocks = Vec::new();
    for child in root.children() {
        if let NodeValue::FrontMatter(value) = &child.data().value {
            metadata.front_matter = Some(value.clone());
            continue;
        }
        if let Some(block) = convert_block(child, &positions, &mut next_id)? {
            blocks.push(block);
        }
    }
    let document = Document {
        schema_version: DOCUMENT_SCHEMA_VERSION,
        document_version,
        uri,
        metadata,
        blocks,
    };
    document.validate(source)?;
    Ok(document)
}

fn convert_block<'a>(
    node: &'a AstNode<'a>,
    positions: &SourceIndex,
    next_id: &mut usize,
) -> Result<Option<Block>, ParseError> {
    let data = node.data();
    let sourcepos = data.sourcepos;
    let value = data.value.clone();
    drop(data);

    let (kind, children) = match value {
        NodeValue::Paragraph => {
            let inline_children = node.children().collect::<Vec<_>>();
            if inline_children.len() == 1 {
                match &inline_children[0].data().value {
                    NodeValue::Math(math) if math.display_math => (
                        BlockKind::Math {
                            source: math.literal.clone(),
                        },
                        Vec::new(),
                    ),
                    NodeValue::Link(link) if is_youtube_url(&link.url) => (
                        BlockKind::Media {
                            source: link.url.clone(),
                        },
                        Vec::new(),
                    ),
                    _ => (BlockKind::Paragraph, inline_nodes(node, positions)?),
                }
            } else {
                (BlockKind::Paragraph, inline_nodes(node, positions)?)
            }
        }
        NodeValue::Heading(heading) => (
            BlockKind::Heading {
                level: heading.level,
            },
            inline_nodes(node, positions)?,
        ),
        NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
            (BlockKind::Quote, block_nodes(node, positions, next_id)?)
        }
        NodeValue::Alert(alert) => (
            BlockKind::Admonition {
                kind: format!("{:?}", alert.alert_type).to_lowercase(),
                title: format!("{:?}", alert.alert_type),
            },
            block_nodes(node, positions, next_id)?,
        ),
        NodeValue::List(list) => (
            BlockKind::List {
                ordered: list.list_type == ListType::Ordered,
                start: list.start as u64,
                tight: list.tight,
            },
            block_nodes(node, positions, next_id)?,
        ),
        NodeValue::Item(_) => {
            let checked = node.children().find_map(|child| match child.data().value {
                NodeValue::TaskItem(task) => Some(task.symbol.is_some()),
                _ => None,
            });
            (
                BlockKind::ListItem { checked },
                block_nodes(node, positions, next_id)?,
            )
        }
        NodeValue::TaskItem(task) => (
            BlockKind::ListItem {
                checked: Some(task.symbol.is_some()),
            },
            mixed_nodes(node, positions, next_id)?,
        ),
        NodeValue::Table(_) => (BlockKind::Table, block_nodes(node, positions, next_id)?),
        NodeValue::TableRow(header) => (
            BlockKind::TableRow { header },
            block_nodes(node, positions, next_id)?,
        ),
        NodeValue::TableCell => {
            let header = node
                .parent()
                .is_some_and(|parent| matches!(parent.data().value, NodeValue::TableRow(true)));
            (
                BlockKind::TableCell { header },
                inline_nodes(node, positions)?,
            )
        }
        NodeValue::CodeBlock(code) => {
            let (language, title, line_numbers) = parse_code_info(&code.info);
            let kind = match language.as_deref() {
                Some("mermaid") => BlockKind::Mermaid {
                    source: code.literal,
                },
                Some("abc") => BlockKind::AbcNotation {
                    source: code.literal,
                },
                _ => BlockKind::CodeBlock {
                    language,
                    title,
                    line_numbers,
                    code: code.literal,
                },
            };
            (kind, Vec::new())
        }
        NodeValue::HtmlBlock(html) => (BlockKind::RawHtml { html: html.literal }, Vec::new()),
        NodeValue::ThematicBreak => (BlockKind::ThematicBreak, Vec::new()),
        NodeValue::BlockDirective(directive) => {
            let info = directive.info.trim();
            let name = info.split_once('[').map_or(info, |(name, _)| name);
            let label = info
                .strip_prefix(name)
                .and_then(|value| value.strip_prefix('['))
                .and_then(|value| value.strip_suffix(']'))
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let kind = match name {
                "tabs" => BlockKind::Tabs,
                "tab" => BlockKind::Tab {
                    label: label.unwrap_or("Tab").to_owned(),
                },
                "details" => BlockKind::Details {
                    summary: label.unwrap_or("Details").to_owned(),
                },
                "info" | "tip" | "important" | "warning" | "danger" => BlockKind::Admonition {
                    kind: name.to_owned(),
                    title: label.map(str::to_owned).unwrap_or_else(|| name.to_owned()),
                },
                _ => BlockKind::Plugin {
                    namespace: "directive".to_owned(),
                    name: name.to_owned(),
                },
            };
            (kind, block_nodes(node, positions, next_id)?)
        }
        NodeValue::FootnoteDefinition(definition) => (
            BlockKind::Plugin {
                namespace: "commonmark".to_owned(),
                name: format!("footnote-definition:{}", definition.name),
            },
            block_nodes(node, positions, next_id)?,
        ),
        other @ (NodeValue::DescriptionList
        | NodeValue::DescriptionItem(_)
        | NodeValue::DescriptionTerm
        | NodeValue::DescriptionDetails
        | NodeValue::Subtext) => (
            BlockKind::Plugin {
                namespace: "commonmark".to_owned(),
                name: format!("{other:?}")
                    .split(['(', '{'])
                    .next()
                    .unwrap_or("block")
                    .to_owned(),
            },
            mixed_nodes(node, positions, next_id)?,
        ),
        NodeValue::Text(_)
        | NodeValue::SoftBreak
        | NodeValue::LineBreak
        | NodeValue::Code(_)
        | NodeValue::HtmlInline(_)
        | NodeValue::Raw(_)
        | NodeValue::Emph
        | NodeValue::Strong
        | NodeValue::Strikethrough
        | NodeValue::Highlight
        | NodeValue::Insert
        | NodeValue::Superscript
        | NodeValue::Link(_)
        | NodeValue::Image(_)
        | NodeValue::FootnoteReference(_)
        | NodeValue::Math(_)
        | NodeValue::Escaped
        | NodeValue::WikiLink(_)
        | NodeValue::Underline
        | NodeValue::Subscript
        | NodeValue::SpoileredText
        | NodeValue::EscapedTag(_) => return Ok(None),
        NodeValue::FrontMatter(_) | NodeValue::Document => return Ok(None),
    };
    let provenance = positions.provenance_enclosing(sourcepos, &children)?;
    let id = NodeId::pending(*next_id);
    *next_id += 1;
    Ok(Some(Block {
        id,
        provenance,
        kind,
        attributes: BTreeMap::new(),
        children,
    }))
}

fn is_youtube_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://youtu.be/")
        || lower.starts_with("https://www.youtube.com/watch?")
        || lower.starts_with("https://youtube.com/watch?")
}

fn block_nodes<'a>(
    node: &'a AstNode<'a>,
    positions: &SourceIndex,
    next_id: &mut usize,
) -> Result<Vec<Node>, ParseError> {
    let mut children = Vec::new();
    for child in node.children() {
        if let Some(block) = convert_block(child, positions, next_id)? {
            children.push(Node::Block(block));
        }
    }
    Ok(children)
}

fn mixed_nodes<'a>(
    node: &'a AstNode<'a>,
    positions: &SourceIndex,
    next_id: &mut usize,
) -> Result<Vec<Node>, ParseError> {
    let mut children = Vec::new();
    for child in node.children() {
        if let Some(block) = convert_block(child, positions, next_id)? {
            children.push(Node::Block(block));
        } else if let Some(inline) = convert_inline(child, positions)? {
            children.push(Node::Inline(inline));
        }
    }
    Ok(children)
}

fn inline_nodes<'a>(
    node: &'a AstNode<'a>,
    positions: &SourceIndex,
) -> Result<Vec<Node>, ParseError> {
    node.children()
        .filter_map(|child| convert_inline(child, positions).transpose())
        .map(|result| result.map(Node::Inline))
        .collect()
}

fn convert_inline<'a>(
    node: &'a AstNode<'a>,
    positions: &SourceIndex,
) -> Result<Option<Inline>, ParseError> {
    let data = node.data();
    let provenance = positions.provenance(data.sourcepos)?;
    let value = data.value.clone();
    drop(data);

    let nested = || -> Result<Vec<Inline>, ParseError> {
        node.children()
            .filter_map(|child| convert_inline(child, positions).transpose())
            .collect()
    };
    let kind = match value {
        NodeValue::Text(value) => InlineKind::Text {
            value: value.into_owned(),
        },
        NodeValue::Code(code) => InlineKind::Code {
            value: code.literal,
        },
        NodeValue::Emph => InlineKind::Emphasis {
            children: nested()?,
        },
        NodeValue::Strong => InlineKind::Strong {
            children: nested()?,
        },
        NodeValue::Strikethrough => InlineKind::Strikethrough {
            children: nested()?,
        },
        NodeValue::Highlight
        | NodeValue::Insert
        | NodeValue::Superscript
        | NodeValue::Underline
        | NodeValue::Subscript
        | NodeValue::SpoileredText
        | NodeValue::Escaped => InlineKind::Emphasis {
            children: nested()?,
        },
        NodeValue::Link(link) => InlineKind::Link {
            destination: link.url,
            title: link.title,
            children: nested()?,
        },
        NodeValue::Image(link) => InlineKind::Image {
            source: link.url,
            title: link.title,
            children: nested()?,
        },
        NodeValue::WikiLink(link) => InlineKind::Link {
            destination: link.url,
            title: String::new(),
            children: nested()?,
        },
        NodeValue::FootnoteReference(reference) => InlineKind::Link {
            destination: format!("#footnote-{}", reference.name),
            title: String::new(),
            children: vec![Inline {
                provenance: provenance.clone(),
                kind: InlineKind::Text {
                    value: format!("[{}]", reference.name),
                },
            }],
        },
        NodeValue::Math(math) => InlineKind::Math {
            source: math.literal,
        },
        NodeValue::SoftBreak => InlineKind::SoftBreak,
        NodeValue::LineBreak => InlineKind::HardBreak,
        NodeValue::HtmlInline(html) => InlineKind::RawHtml { html },
        NodeValue::Raw(raw) => InlineKind::RawHtml { html: raw },
        NodeValue::EscapedTag(tag) => InlineKind::Text {
            value: tag.to_owned(),
        },
        other => InlineKind::Text {
            value: format!("{other:?}"),
        },
    };
    Ok(Some(Inline { provenance, kind }))
}

fn parse_code_info(info: &str) -> (Option<String>, Option<String>, bool) {
    let mut parts = info.split_whitespace();
    let language = parts
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let mut title = None;
    let mut line_numbers = false;
    for part in parts {
        if part == "line-numbers" || part.starts_with("showLineNumbers") {
            line_numbers = true;
        } else if let Some(value) = part.strip_prefix("title=") {
            title = Some(value.trim_matches(['\'', '"']).to_owned());
        }
    }
    (language, title, line_numbers)
}

struct SourceIndex<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceIndex<'a> {
    fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(source.match_indices('\n').map(|(index, _)| index + 1));
        Self {
            source,
            line_starts,
        }
    }

    fn provenance(&self, sourcepos: Sourcepos) -> Result<SourceProvenance, ParseError> {
        self.provenance_enclosing(sourcepos, &[])
    }

    fn provenance_enclosing(
        &self,
        sourcepos: Sourcepos,
        children: &[Node],
    ) -> Result<SourceProvenance, ParseError> {
        let start = self.sourcepos_start(sourcepos);
        let end_line = sourcepos.end.line.checked_sub(1);
        // Comrak can truncate the final item and paragraph end inside a block directive.
        // Only a direct child ending on the same reported line can repair that end. Some valid
        // list-item ranges include following blank lines and therefore extend past their list's
        // own reported end; those must not widen or invalidate the parent list.
        let reported_end = self.sourcepos_end_offset(sourcepos);
        let child_end = children
            .iter()
            .filter_map(|child| match child {
                Node::Block(block) => block.provenance.primary_range(),
                Node::Inline(inline) => inline.provenance.primary_range(),
            })
            .filter(|range| {
                start.is_some_and(|start| range.byte_start.get() >= start as u64)
                    && end_line.is_some_and(|line| range.end.line.get() == line as u64)
            })
            .max_by_key(|range| range.byte_end)
            .map(|range| range.byte_end.get() as usize);
        let end = match (reported_end, child_end) {
            (Some(reported), Some(child)) if self.source.is_char_boundary(reported) => {
                Some(reported.max(child))
            }
            (Some(reported), Some(child))
                if start.is_some_and(|start| start <= reported)
                    && reported <= child
                    && self.source.is_char_boundary(child) =>
            {
                Some(child)
            }
            (Some(reported), None) if self.source.is_char_boundary(reported) => Some(reported),
            _ => None,
        };
        let (start, end) = match (start, end) {
            (Some(start), Some(end)) if start <= end => (start, end),
            _ => return Err(self.invalid(sourcepos)),
        };
        Ok(SourceProvenance::original(SourceRange {
            byte_start: JsSafeU64::new(start as u64).expect("source length is JavaScript-safe"),
            byte_end: JsSafeU64::new(end as u64).expect("source length is JavaScript-safe"),
            start: self.position(start),
            end: self.position(end),
        }))
    }

    fn sourcepos_start(&self, sourcepos: Sourcepos) -> Option<usize> {
        let line = sourcepos
            .start
            .line
            .checked_sub(1)
            .and_then(|line| self.line_starts.get(line))?;
        let column = sourcepos.start.column.checked_sub(1)?;
        let offset = line.checked_add(column)?;
        (offset <= self.source.len() && self.source.is_char_boundary(offset)).then_some(offset)
    }

    fn sourcepos_end_offset(&self, sourcepos: Sourcepos) -> Option<usize> {
        let line = sourcepos
            .end
            .line
            .checked_sub(1)
            .and_then(|line| self.line_starts.get(line))?;
        // Comrak columns are one-based and its end is inclusive, so the numeric end column is
        // already the zero-based exclusive byte offset within that line.
        let offset = line.checked_add(sourcepos.end.column)?;
        (offset <= self.source.len()).then_some(offset)
    }

    fn position(&self, byte_offset: usize) -> SourcePosition {
        let line = self
            .line_starts
            .partition_point(|start| *start <= byte_offset)
            .saturating_sub(1);
        SourcePosition {
            line: JsSafeU64::new(line as u64).expect("source line count is JavaScript-safe"),
            character: JsSafeU64::new((byte_offset - self.line_starts[line]) as u64)
                .expect("source line length is JavaScript-safe"),
            encoding: PositionEncoding::Utf8,
        }
    }

    fn invalid(&self, sourcepos: Sourcepos) -> ParseError {
        ParseError::InvalidSourcePosition(sourcepos.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_gfm_special_blocks_and_front_matter() {
        let source = "---\ntitle: Demo\n---\n# Héllo\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```mermaid\ngraph TD; A-->B\n```\n";
        let document = parse(DocumentUri("file:///demo.md".into()), 7, source).unwrap();
        assert_eq!(document.document_version, 7);
        assert!(
            document
                .metadata
                .front_matter
                .as_deref()
                .unwrap()
                .contains("title: Demo")
        );
        assert!(matches!(
            document.blocks[0].kind,
            BlockKind::Heading { level: 1 }
        ));
        assert!(
            document
                .blocks
                .iter()
                .any(|block| matches!(block.kind, BlockKind::Table))
        );
        assert!(
            document
                .blocks
                .iter()
                .any(|block| matches!(block.kind, BlockKind::Mermaid { .. }))
        );
        document.validate(source).unwrap();
    }

    #[test]
    fn parses_all_github_alerts_and_the_important_directive() {
        let source = "> [!NOTE]\n> note\n\n> [!TIP]\n> tip\n\n> [!IMPORTANT]\n> important\n\n> [!WARNING]\n> warning\n\n> [!CAUTION]\n> caution\n\n:::important[Custom]\nimportant directive\n:::\n";
        let document = parse(DocumentUri("file:///alerts.md".into()), 1, source).unwrap();
        let expected = [
            ("note", "Note"),
            ("tip", "Tip"),
            ("important", "Important"),
            ("warning", "Warning"),
            ("caution", "Caution"),
            ("important", "Custom"),
        ];

        assert_eq!(document.blocks.len(), expected.len());
        for (block, (expected_kind, expected_title)) in document.blocks.iter().zip(expected) {
            let BlockKind::Admonition { kind, title } = &block.kind else {
                panic!("expected an admonition, got {:?}", block.kind);
            };
            assert_eq!(kind, expected_kind);
            assert_eq!(title, expected_title);
        }
        document.validate(source).unwrap();
    }

    #[test]
    fn uses_utf8_byte_offsets_and_half_open_ranges() {
        let source = "# 🦀\n";
        let document = parse(DocumentUri("file:///unicode.md".into()), 1, source).unwrap();
        let range = document.blocks[0].provenance.primary_range().unwrap();
        assert_eq!((range.byte_start.get(), range.byte_end.get()), (0, 6));
        assert_eq!(range.end.character, 6);
    }

    #[test]
    fn code_info_is_new_ir_data_not_legacy_html() {
        let source = "```rust title='sample' line-numbers\nfn main() {}\n```\n";
        let document = parse(DocumentUri("file:///code.md".into()), 1, source).unwrap();
        assert!(matches!(
            &document.blocks[0].kind,
            BlockKind::CodeBlock { language: Some(language), title: Some(title), line_numbers: true, .. }
                if language == "rust" && title == "sample"
        ));
    }

    #[test]
    fn directives_and_standalone_youtube_are_typed_blocks() {
        let tabs = include_str!("../../../markdown_for_debug/tabs.md");
        let document = parse(DocumentUri("file:///tabs.md".into()), 1, tabs).unwrap();
        assert!(
            document
                .blocks
                .iter()
                .any(|block| block.kind == BlockKind::Tabs)
        );
        let tabs = document
            .blocks
            .iter()
            .find(|block| block.kind == BlockKind::Tabs)
            .unwrap();
        let labels = tabs
            .children
            .iter()
            .filter_map(|node| match node {
                Node::Block(Block {
                    kind: BlockKind::Tab { label },
                    ..
                }) => Some(label.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(labels, ["Tab 1", "Tab 2"]);

        let details = include_str!("../../../markdown_for_debug/collapsible.md");
        let document = parse(DocumentUri("file:///details.md".into()), 1, details).unwrap();
        let summaries = document
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                BlockKind::Details { summary } => Some(summary.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(summaries, ["Details", "Custom Title"]);

        let youtube = include_str!("../../../markdown_for_debug/youtube.md");
        let document = parse(DocumentUri("file:///youtube.md".into()), 1, youtube).unwrap();
        assert!(document.blocks.iter().any(|block| matches!(
            &block.kind,
            BlockKind::Media { source } if source.starts_with("https://youtu.be/")
        )));
    }

    #[test]
    fn repairs_truncated_container_positions_from_unicode_children() {
        let source = ":::info\n**目標**\n- 遠くの線は細く\n- 線の入り抜きを入れる\n- そんなに震えない\n:::\n";
        let document = parse(DocumentUri("file:///directive.md".into()), 1, source).unwrap();
        let directive = &document.blocks[0];
        let list = match &directive.children[1] {
            Node::Block(block) => block,
            Node::Inline(_) => panic!("directive list must be a block"),
        };
        let item = match &list.children[2] {
            Node::Block(block) => block,
            Node::Inline(_) => panic!("list item must be a block"),
        };
        let range = item.provenance.primary_range().unwrap();
        assert_eq!(
            &source[range.byte_start.get() as usize..range.byte_end.get() as usize],
            "- そんなに震えない"
        );
        let paragraph = match &item.children[0] {
            Node::Block(block) => block,
            Node::Inline(_) => panic!("list paragraph must be a block"),
        };
        let range = paragraph.provenance.primary_range().unwrap();
        assert_eq!(
            &source[range.byte_start.get() as usize..range.byte_end.get() as usize],
            "そんなに震えない"
        );

        let source = ":::info\n- first\n- last\n:::\n";
        let document = parse(DocumentUri("file:///ascii-directive.md".into()), 1, source).unwrap();
        let list = match &document.blocks[0].children[0] {
            Node::Block(block) => block,
            Node::Inline(_) => panic!("directive list must be a block"),
        };
        let item = match &list.children[1] {
            Node::Block(block) => block,
            Node::Inline(_) => panic!("list item must be a block"),
        };
        let range = item.provenance.primary_range().unwrap();
        assert_eq!(
            &source[range.byte_start.get() as usize..range.byte_end.get() as usize],
            "- last"
        );
    }

    #[test]
    fn accepts_list_items_that_include_blank_lines_past_the_parent_list() {
        let source = "- 近距離\n  - ベース幅: 3.0\n- 遠距離\n  - エラー: 10.0\n\n\n:::info\n- そんなに震えない\n:::\n";
        let document = parse(DocumentUri("file:///nested-list.md".into()), 1, source).unwrap();
        let range = document.blocks[0].provenance.primary_range().unwrap();
        assert_eq!(
            &source[range.byte_start.get() as usize..range.byte_end.get() as usize],
            "- 近距離\n  - ベース幅: 3.0\n- 遠距離\n  - エラー: 10.0"
        );
    }
}
