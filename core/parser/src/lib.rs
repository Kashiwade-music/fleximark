//! Role: Parse Markdown into the document model.

use comrak::{
    Arena, Options,
    nodes::{AstNode, ListType, NodeValue, Sourcepos},
    parse_document,
};
use fleximark_model::{
    Block, BlockKind, Document, DocumentMetadata, DocumentUri, Inline, InlineKind, Node, NodeId,
    PositionEncoding, SourceIndex, SourceRange, ValidationError,
    syntax::{BuiltinSyntaxKind, lookup_alert, lookup_directive, lookup_fence, youtube_video_id},
};

#[derive(Debug)]
pub enum ParseError {
    InvalidSourcePosition(String),
    InvalidDocument(ValidationError),
    UnsupportedNode,
}
impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSourcePosition(value) => write!(
                f,
                "the parser returned an invalid source position: {0}",
                value
            ),
            Self::InvalidDocument(value) => std::fmt::Display::fmt(value, f),
            Self::UnsupportedNode => f.write_str("unsupported Markdown node"),
        }
    }
}
impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidDocument(value) => Some(value),
            _ => None,
        }
    }
}
impl From<ValidationError> for ParseError {
    fn from(value: ValidationError) -> Self {
        Self::InvalidDocument(value)
    }
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
    let positions = ParserSourceIndex::new(source);
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
    positions: &ParserSourceIndex,
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
                    NodeValue::Link(link) if youtube_video_id(&link.url).is_some() => (
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
        NodeValue::Alert(alert) => {
            let descriptor = lookup_alert(alert.alert_type.default_title())
                .ok_or(ParseError::UnsupportedNode)?;
            (
                BlockKind::Admonition {
                    kind: descriptor.name.to_owned(),
                    title: alert
                        .title
                        .unwrap_or_else(|| descriptor.default_title.to_owned()),
                },
                block_nodes(node, positions, next_id)?,
            )
        }
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
            let kind = match language
                .as_deref()
                .and_then(lookup_fence)
                .map(|descriptor| descriptor.kind)
            {
                Some(BuiltinSyntaxKind::Mermaid) => BlockKind::Mermaid {
                    source: code.literal,
                },
                Some(BuiltinSyntaxKind::Abc) => BlockKind::AbcNotation {
                    source: code.literal,
                },
                Some(BuiltinSyntaxKind::Math) => BlockKind::Math {
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
            let kind = match lookup_directive(name).map(|descriptor| descriptor.kind) {
                Some(BuiltinSyntaxKind::Tabs) => BlockKind::Tabs,
                Some(BuiltinSyntaxKind::Tab) => BlockKind::Tab {
                    label: label.unwrap_or("Tab").to_owned(),
                },
                Some(BuiltinSyntaxKind::Details) => BlockKind::Details {
                    summary: label.unwrap_or("Details").to_owned(),
                },
                Some(BuiltinSyntaxKind::Admonition) => BlockKind::Admonition {
                    kind: name.to_owned(),
                    title: label.unwrap_or(name).to_owned(),
                },
                _ => BlockKind::Directive {
                    name: name.to_owned(),
                },
            };
            (kind, block_nodes(node, positions, next_id)?)
        }
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
        | NodeValue::Math(_)
        | NodeValue::Escaped
        | NodeValue::WikiLink(_)
        | NodeValue::Underline
        | NodeValue::Subscript
        | NodeValue::SpoileredText
        | NodeValue::EscapedTag(_) => return Ok(None),
        NodeValue::FrontMatter(_) | NodeValue::Document => return Ok(None),
        _ => return Err(ParseError::UnsupportedNode),
    };
    let range = positions.range_enclosing(sourcepos, &children)?;
    let id = NodeId::pending(*next_id);
    *next_id += 1;
    Ok(Some(Block {
        id,
        range,
        kind,
        children,
    }))
}

fn block_nodes<'a>(
    node: &'a AstNode<'a>,
    positions: &ParserSourceIndex,
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
    positions: &ParserSourceIndex,
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
    positions: &ParserSourceIndex,
) -> Result<Vec<Node>, ParseError> {
    node.children()
        .filter_map(|child| convert_inline(child, positions).transpose())
        .map(|result| result.map(Node::Inline))
        .collect()
}

fn convert_inline<'a>(
    node: &'a AstNode<'a>,
    positions: &ParserSourceIndex,
) -> Result<Option<Inline>, ParseError> {
    let data = node.data();
    let range = positions.range(data.sourcepos)?;
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
        | NodeValue::Escaped => InlineKind::Text {
            // Unsupported formatting degrades to its literal text, never to emphasis.
            value: inline_plain_text(&nested()?),
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
        _ => return Err(ParseError::UnsupportedNode),
    };
    Ok(Some(Inline { range, kind }))
}

fn inline_plain_text(inlines: &[Inline]) -> String {
    struct Text(String);
    impl fleximark_model::Visitor for Text {
        fn visit_inline(&mut self, inline: &Inline) {
            match &inline.kind {
                InlineKind::Text { value } | InlineKind::Code { value } => self.0.push_str(value),
                InlineKind::Math { source } => self.0.push_str(source),
                InlineKind::RawHtml { html } => self.0.push_str(html),
                InlineKind::SoftBreak | InlineKind::HardBreak => self.0.push('\n'),
                _ => {}
            }
        }
    }
    let mut text = Text(String::new());
    fleximark_model::walk_inlines(inlines, &mut text);
    text.0
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

struct ParserSourceIndex<'a> {
    source: &'a str,
    index: SourceIndex<'a>,
}

impl<'a> ParserSourceIndex<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            index: SourceIndex::new(source),
        }
    }

    fn range(&self, sourcepos: Sourcepos) -> Result<SourceRange, ParseError> {
        self.range_enclosing(sourcepos, &[])
    }

    fn range_enclosing(
        &self,
        sourcepos: Sourcepos,
        children: &[Node],
    ) -> Result<SourceRange, ParseError> {
        let start = self.sourcepos_start(sourcepos);
        let end_line = sourcepos.end.line.checked_sub(1);
        // Comrak can truncate the final item and paragraph end inside a block directive.
        // Only a direct child ending on the same reported line can repair that end. Some valid
        // list-item ranges include following blank lines and therefore extend past their list's
        // own reported end; those must not widen or invalidate the parent list.
        let reported_end = self.sourcepos_end_offset(sourcepos);
        let child_end = children
            .iter()
            .map(|child| match child {
                Node::Block(block) => &block.range,
                Node::Inline(inline) => &inline.range,
            })
            .filter(|range| {
                start.is_some_and(|start| range.byte_start >= start as u64)
                    && end_line.is_some_and(|line| {
                        self.index
                            .byte_to_position(range.byte_end as usize, PositionEncoding::Utf8)
                            .is_some_and(|position| position.line == line as u64)
                    })
            })
            .max_by_key(|range| range.byte_end)
            .map(|range| range.byte_end as usize);
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
        self.index
            .range(start..end)
            .map_err(|_| self.invalid(sourcepos))
    }

    fn sourcepos_start(&self, sourcepos: Sourcepos) -> Option<usize> {
        let line = sourcepos
            .start
            .line
            .checked_sub(1)
            .and_then(|line| self.index.line_start(line))?;
        let column = sourcepos.start.column.checked_sub(1)?;
        let offset = line.checked_add(column)?;
        (offset <= self.source.len() && self.source.is_char_boundary(offset)).then_some(offset)
    }

    fn sourcepos_end_offset(&self, sourcepos: Sourcepos) -> Option<usize> {
        let line = sourcepos
            .end
            .line
            .checked_sub(1)
            .and_then(|line| self.index.line_start(line))?;
        // Comrak columns are one-based and its end is inclusive, so the numeric end column is
        // already the zero-based exclusive byte offset within that line.
        let offset = line.checked_add(sourcepos.end.column)?;
        (offset <= self.source.len()).then_some(offset)
    }

    fn invalid(&self, sourcepos: Sourcepos) -> ParseError {
        ParseError::InvalidSourcePosition(sourcepos.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! Role: Verify Markdown parsing and UTF source positions.
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
        let range = &document.blocks[0].range;
        assert_eq!((range.byte_start, range.byte_end), (0, 6));
        assert_eq!(
            SourceIndex::new(source)
                .byte_to_position(range.byte_end as usize, PositionEncoding::Utf8)
                .unwrap()
                .character,
            6
        );
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
        let tabs = include_str!("../../../tests/fixtures/markdown/tabs.md");
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

        let details = include_str!("../../../tests/fixtures/markdown/collapsible.md");
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

        let youtube = include_str!("../../../tests/fixtures/markdown/youtube.md");
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
        let range = &item.range;
        assert_eq!(
            &source[range.byte_start as usize..range.byte_end as usize],
            "- そんなに震えない"
        );
        let paragraph = match &item.children[0] {
            Node::Block(block) => block,
            Node::Inline(_) => panic!("list paragraph must be a block"),
        };
        let range = &paragraph.range;
        assert_eq!(
            &source[range.byte_start as usize..range.byte_end as usize],
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
        let range = &item.range;
        assert_eq!(
            &source[range.byte_start as usize..range.byte_end as usize],
            "- last"
        );
    }

    #[test]
    fn accepts_list_items_that_include_blank_lines_past_the_parent_list() {
        let source = "- 近距離\n  - ベース幅: 3.0\n- 遠距離\n  - エラー: 10.0\n\n\n:::info\n- そんなに震えない\n:::\n";
        let document = parse(DocumentUri("file:///nested-list.md".into()), 1, source).unwrap();
        let range = &document.blocks[0].range;
        assert_eq!(
            &source[range.byte_start as usize..range.byte_end as usize],
            "- 近距離\n  - ベース幅: 3.0\n- 遠距離\n  - エラー: 10.0"
        );
    }
    #[test]
    fn every_builtin_descriptor_is_recognized() -> Result<(), ParseError> {
        use fleximark_model::syntax::{BUILTIN_SYNTAX, SyntaxRecognition};
        for descriptor in BUILTIN_SYNTAX {
            for recognizer in descriptor.recognizers {
                let source = match recognizer {
                    SyntaxRecognition::Fence(name) => format!("```{name}\ncontent\n```\n"),
                    SyntaxRecognition::Directive(_)
                        if descriptor.kind == BuiltinSyntaxKind::Tabs =>
                    {
                        "::::tabs\n:::tab[First]\ncontent\n:::\n::::\n".to_owned()
                    }
                    SyntaxRecognition::Directive(name) => format!(":::{name}\ncontent\n:::\n"),
                    SyntaxRecognition::Alert(name) => format!("> [!{name}]\n> content\n"),
                    SyntaxRecognition::Youtube => "https://youtu.be/dQw4w9WgXcQ\n".to_owned(),
                };
                let document = parse(DocumentUri("file:///catalog.md".to_owned()), 1, &source)?;
                let block = document.blocks.first().ok_or(ParseError::UnsupportedNode)?;
                let matches = match descriptor.kind {
                    BuiltinSyntaxKind::Admonition | BuiltinSyntaxKind::Alert => {
                        matches!(block.kind, BlockKind::Admonition { .. })
                    }
                    BuiltinSyntaxKind::Mermaid => matches!(block.kind, BlockKind::Mermaid { .. }),
                    BuiltinSyntaxKind::Abc => matches!(block.kind, BlockKind::AbcNotation { .. }),
                    BuiltinSyntaxKind::Math => matches!(block.kind, BlockKind::Math { .. }),
                    BuiltinSyntaxKind::Youtube => matches!(block.kind, BlockKind::Media { .. }),
                    BuiltinSyntaxKind::Tabs => matches!(block.kind, BlockKind::Tabs),
                    BuiltinSyntaxKind::Tab => matches!(block.kind, BlockKind::Tab { .. }),
                    BuiltinSyntaxKind::Details => matches!(block.kind, BlockKind::Details { .. }),
                };
                assert!(matches, "{}: {:?}", descriptor.name, block.kind);
            }
        }
        Ok(())
    }

    #[test]
    fn math_fences_are_display_math_and_alert_titles_are_preserved() -> Result<(), ParseError> {
        let source = "```math\nE=mc^2\n```\n\n> [!NOTE] Custom title\n> Body\n\n:::unknown[Custom]\nBody\n:::\n";
        let document = parse(DocumentUri("file:///special.md".to_owned()), 1, source)?;
        assert!(
            matches!(&document.blocks[0].kind, BlockKind::Math { source } if source == "E=mc^2\n")
        );
        assert!(
            matches!(&document.blocks[1].kind, BlockKind::Admonition { kind, title } if kind == "note" && title == "Custom title")
        );
        assert!(
            matches!(&document.blocks[2].kind, BlockKind::Directive { name } if name == "unknown")
        );
        assert!(!document.blocks[2].children.is_empty());
        Ok(())
    }

    #[test]
    fn unsupported_inline_formatting_degrades_to_text_without_emphasis() -> Result<(), ParseError> {
        for (source, expected) in [
            ("==marked==", "marked"),
            ("^super^", "super"),
            ("\\*escaped\\*", "*escaped*"),
        ] {
            let arena = Arena::new();
            let mut options = Options::default();
            options.extension.highlight = true;
            options.extension.superscript = true;
            let root = parse_document(&arena, source, &options);
            let paragraph = root.first_child().ok_or(ParseError::UnsupportedNode)?;
            let block = convert_block(paragraph, &ParserSourceIndex::new(source), &mut 0)?
                .ok_or(ParseError::UnsupportedNode)?;
            let inlines = block
                .children
                .iter()
                .filter_map(|node| {
                    if let Node::Inline(inline) = node {
                        Some(inline.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(inline_plain_text(&inlines), expected);
            assert!(
                inlines
                    .iter()
                    .all(|inline| matches!(inline.kind, InlineKind::Text { .. }))
            );
        }
        Ok(())
    }
}
