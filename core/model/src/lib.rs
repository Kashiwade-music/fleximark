//! Role: Editor-independent document model and source provenance.
use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

pub const DOCUMENT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentUri(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn pending(index: usize) -> Self {
        Self(format!("pending-{index}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransformId(pub String);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front_matter: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub document_version: u64,
    pub uri: DocumentUri,
    pub metadata: DocumentMetadata,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub id: NodeId,
    pub provenance: SourceProvenance,
    pub kind: BlockKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Node {
    Block(Block),
    Inline(Inline),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inline {
    pub provenance: SourceProvenance,
    pub kind: InlineKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BlockKind {
    Paragraph,
    Heading {
        level: u8,
    },
    Quote,
    List {
        ordered: bool,
        start: u64,
        tight: bool,
    },
    ListItem {
        checked: Option<bool>,
    },
    Table,
    TableRow {
        header: bool,
    },
    TableCell {
        header: bool,
    },
    CodeBlock {
        language: Option<String>,
        title: Option<String>,
        line_numbers: bool,
        code: String,
    },
    Mermaid {
        source: String,
    },
    AbcNotation {
        source: String,
    },
    Math {
        source: String,
    },
    ThematicBreak,
    Admonition {
        kind: String,
        title: String,
    },
    Tabs,
    Tab {
        label: String,
    },
    Details {
        summary: String,
    },
    Media {
        source: String,
    },
    RawHtml {
        html: String,
    },
    Plugin {
        namespace: String,
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InlineKind {
    Text {
        value: String,
    },
    Code {
        value: String,
    },
    Emphasis {
        children: Vec<Inline>,
    },
    Strong {
        children: Vec<Inline>,
    },
    Strikethrough {
        children: Vec<Inline>,
    },
    Link {
        destination: String,
        title: String,
        children: Vec<Inline>,
    },
    Image {
        source: String,
        title: String,
        children: Vec<Inline>,
    },
    Math {
        source: String,
    },
    SoftBreak,
    HardBreak,
    RawHtml {
        html: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PositionEncoding {
    Utf8,
    Utf16,
    Utf32,
}

impl PositionEncoding {
    pub fn encoded_len(self, text: &str) -> u64 {
        match self {
            Self::Utf8 => text.len() as u64,
            Self::Utf16 => text.encode_utf16().count() as u64,
            Self::Utf32 => text.chars().count() as u64,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePosition {
    pub line: u64,
    pub character: u64,
    pub encoding: PositionEncoding,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRange {
    pub byte_start: u64,
    pub byte_end: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationEntry {
    pub node_id: NodeId,
    pub source_range: SourceRange,
    pub depth: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnchorAffinity {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GeneratedAnchor {
    pub range: SourceRange,
    pub affinity: AnchorAffinity,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SourceProvenance {
    Original {
        ranges: Vec<SourceRange>,
        primary_range_index: u32,
    },
    Derived {
        ranges: Vec<SourceRange>,
        primary_range_index: u32,
        transform: TransformId,
    },
    Generated {
        anchor: Option<GeneratedAnchor>,
        transform: TransformId,
    },
}

impl SourceProvenance {
    pub fn original(range: SourceRange) -> Self {
        Self::Original {
            ranges: vec![range],
            primary_range_index: 0,
        }
    }

    pub fn primary_range(&self) -> Option<&SourceRange> {
        match self {
            Self::Original {
                ranges,
                primary_range_index,
            }
            | Self::Derived {
                ranges,
                primary_range_index,
                ..
            } => ranges.get(*primary_range_index as usize),
            Self::Generated { anchor, .. } => anchor.as_ref().map(|anchor| &anchor.range),
        }
    }

    pub fn navigation_range(&self) -> Option<SourceRange> {
        match self {
            Self::Original { .. } | Self::Derived { .. } => self.primary_range().cloned(),
            Self::Generated { anchor, .. } => anchor.as_ref().map(|anchor| {
                let byte_offset = match anchor.affinity {
                    AnchorAffinity::Before => anchor.range.byte_start,
                    AnchorAffinity::After => anchor.range.byte_end,
                };
                SourceRange {
                    byte_start: byte_offset,
                    byte_end: byte_offset,
                }
            }),
        }
    }

    pub fn validate(&self, source: &str) -> Result<(), ValidationError> {
        self.validate_with_index(&SourceIndex::new(source))
    }

    fn validate_with_index(&self, source: &SourceIndex<'_>) -> Result<(), ValidationError> {
        let (ranges, primary) = match self {
            Self::Original {
                ranges,
                primary_range_index,
            }
            | Self::Derived {
                ranges,
                primary_range_index,
                ..
            } => (ranges.as_slice(), Some(*primary_range_index)),
            Self::Generated { anchor, .. } => {
                if let Some(anchor) = anchor {
                    validate_range(&anchor.range, source)?;
                }
                return Ok(());
            }
        };
        if ranges.is_empty() {
            return Err(ValidationError::EmptyProvenance);
        }
        if primary.unwrap() as usize >= ranges.len() {
            return Err(ValidationError::InvalidPrimaryRange);
        }
        for range in ranges {
            validate_range(range, source)?;
        }
        for pair in ranges.windows(2) {
            if pair[1].byte_start < pair[0].byte_start {
                return Err(ValidationError::UnorderedProvenanceRanges);
            }
            if pair[1].byte_start < pair[0].byte_end {
                return Err(ValidationError::OverlappingProvenanceRanges);
            }
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    Schema { actual: u32, expected: u32 },
    DuplicateNodeId(String),
    EmptyProvenance,
    InvalidPrimaryRange,
    UnorderedProvenanceRanges,
    OverlappingProvenanceRanges,
    InvalidRange,
    InvalidUtf8Boundary,
    InvalidChildShape,
}
impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema { actual, expected } => write!(
                f,
                "document schema {actual} is not supported; expected {expected}"
            ),
            Self::DuplicateNodeId(value) => {
                write!(f, "a block NodeId occurs more than once: {0}", value)
            }
            Self::EmptyProvenance => write!(f, "original or derived provenance has no ranges"),
            Self::InvalidPrimaryRange => write!(f, "provenance primary range is out of bounds"),
            Self::UnorderedProvenanceRanges => write!(
                f,
                "provenance ranges are not ordered by original UTF-8 byte offset"
            ),
            Self::OverlappingProvenanceRanges => write!(
                f,
                "provenance ranges overlap in the original source snapshot"
            ),
            Self::InvalidRange => {
                write!(f, "source range is reversed or outside the source snapshot")
            }
            Self::InvalidUtf8Boundary => {
                write!(f, "source range does not fall on UTF-8 boundaries")
            }
            Self::InvalidChildShape => write!(
                f,
                "document block contains children incompatible with its kind"
            ),
        }
    }
}
impl std::error::Error for ValidationError {}

impl Document {
    pub fn navigation(&self) -> Vec<NavigationEntry> {
        let mut entries = Vec::new();
        let mut stack = self
            .blocks
            .iter()
            .rev()
            .map(|block| (block, 0_u32))
            .collect::<Vec<_>>();
        while let Some((block, depth)) = stack.pop() {
            if let Some(source_range) = block.provenance.navigation_range() {
                entries.push(NavigationEntry {
                    node_id: block.id.clone(),
                    source_range,
                    depth,
                });
            }
            stack.extend(block.children.iter().rev().filter_map(|node| match node {
                Node::Block(block) => Some((block, depth + 1)),
                Node::Inline(_) => None,
            }));
        }
        entries
    }

    pub fn validate(&self, source: &str) -> Result<(), ValidationError> {
        if self.schema_version != DOCUMENT_SCHEMA_VERSION {
            return Err(ValidationError::Schema {
                actual: self.schema_version,
                expected: DOCUMENT_SCHEMA_VERSION,
            });
        }
        let source = SourceIndex::new(source);
        let mut ids = HashSet::new();
        for block in &self.blocks {
            validate_block(block, &source, &mut ids)?;
        }
        Ok(())
    }
}

fn validate_block(
    block: &Block,
    source: &SourceIndex<'_>,
    ids: &mut HashSet<NodeId>,
) -> Result<(), ValidationError> {
    if !ids.insert(block.id.clone()) {
        return Err(ValidationError::DuplicateNodeId(block.id.0.clone()));
    }
    block.provenance.validate_with_index(source)?;
    let valid_children = block.children.iter().all(|child| match &block.kind {
        BlockKind::Paragraph | BlockKind::Heading { .. } | BlockKind::TableCell { .. } => {
            matches!(child, Node::Inline(_))
        }
        BlockKind::Table => matches!(
            child,
            Node::Block(Block {
                kind: BlockKind::TableRow { .. },
                ..
            })
        ),
        BlockKind::TableRow { .. } => matches!(
            child,
            Node::Block(Block {
                kind: BlockKind::TableCell { .. },
                ..
            })
        ),
        BlockKind::Tabs => matches!(
            child,
            Node::Block(Block {
                kind: BlockKind::Tab { .. },
                ..
            })
        ),
        BlockKind::List { .. } => matches!(
            child,
            Node::Block(Block {
                kind: BlockKind::ListItem { .. },
                ..
            })
        ),
        BlockKind::CodeBlock { .. }
        | BlockKind::Mermaid { .. }
        | BlockKind::AbcNotation { .. }
        | BlockKind::Math { .. }
        | BlockKind::ThematicBreak
        | BlockKind::Media { .. } => false,
        _ => true,
    });
    if !valid_children {
        return Err(ValidationError::InvalidChildShape);
    }

    for child in &block.children {
        match child {
            Node::Block(block) => validate_block(block, source, ids)?,
            Node::Inline(inline) => validate_inline(inline, source)?,
        }
    }
    Ok(())
}

fn validate_inline(inline: &Inline, source: &SourceIndex<'_>) -> Result<(), ValidationError> {
    inline.provenance.validate_with_index(source)?;
    let children = match &inline.kind {
        InlineKind::Emphasis { children }
        | InlineKind::Strong { children }
        | InlineKind::Strikethrough { children }
        | InlineKind::Link { children, .. }
        | InlineKind::Image { children, .. } => children,
        _ => return Ok(()),
    };
    for child in children {
        validate_inline(child, source)?;
    }
    Ok(())
}

fn validate_range(range: &SourceRange, source: &SourceIndex<'_>) -> Result<(), ValidationError> {
    let start = usize::try_from(range.byte_start).map_err(|_| ValidationError::InvalidRange)?;
    let end = usize::try_from(range.byte_end).map_err(|_| ValidationError::InvalidRange)?;
    if start > end || end > source.source.len() {
        return Err(ValidationError::InvalidRange);
    }
    if !source.source.is_char_boundary(start) || !source.source.is_char_boundary(end) {
        return Err(ValidationError::InvalidUtf8Boundary);
    }
    Ok(())
}

pub struct SourceIndex<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceIndex<'a> {
    pub fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(source.match_indices('\n').map(|(index, _)| index + 1));
        Self {
            source,
            line_starts,
        }
    }

    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn line_start(&self, line: usize) -> Option<usize> {
        self.line_starts.get(line).copied()
    }

    pub fn validate_range(&self, range: &SourceRange) -> Result<(), ValidationError> {
        validate_range(range, self)
    }

    pub fn range(&self, range: std::ops::Range<usize>) -> Result<SourceRange, ValidationError> {
        let range = SourceRange {
            byte_start: range.start as u64,
            byte_end: range.end as u64,
        };
        self.validate_range(&range)?;
        Ok(range)
    }

    pub fn byte_to_position(
        &self,
        offset: usize,
        encoding: PositionEncoding,
    ) -> Option<SourcePosition> {
        if !self.source.is_char_boundary(offset) {
            return None;
        }
        let line = self
            .line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let line_end = self.line_content_end(line);
        let text = &self.source[self.line_starts[line]..offset.min(line_end)];
        let character = encoding.encoded_len(text);
        Some(SourcePosition {
            line: line as u64,
            character,
            encoding,
        })
    }

    fn line_content_end(&self, line: usize) -> usize {
        let end = self
            .line_starts
            .get(line + 1)
            .map_or(self.source.len(), |next| next - 1);
        if self.line_starts.get(line + 1).is_some()
            && end > self.line_starts[line]
            && self.source.as_bytes()[end - 1] == b'\r'
        {
            end - 1
        } else {
            end
        }
    }

    pub fn position_to_byte(&self, position: &SourcePosition) -> Option<usize> {
        let line = usize::try_from(position.line).ok()?;
        let character = usize::try_from(position.character).ok()?;
        let line_start = *self.line_starts.get(line)?;
        let line_end = self.line_content_end(line);
        let line_text = &self.source[line_start..line_end];
        let mut units = 0;
        for (offset, value) in line_text.char_indices() {
            if units == character {
                return Some(line_start + offset);
            }
            units += match position.encoding {
                PositionEncoding::Utf8 => value.len_utf8(),
                PositionEncoding::Utf16 => value.len_utf16(),
                PositionEncoding::Utf32 => 1,
            };
            if units > character {
                return None;
            }
        }
        (units == character).then_some(line_end)
    }
}

#[cfg(test)]
mod tests {
    //! Role: Verify document model validation and source provenance.
    use super::*;

    fn range(start: u64, end: u64) -> SourceRange {
        SourceRange {
            byte_start: start,
            byte_end: end,
        }
    }

    #[test]
    fn rejects_invalid_provenance_and_accepts_adjacent_ranges() {
        let empty = SourceProvenance::Original {
            ranges: vec![],
            primary_range_index: 0,
        };
        assert_eq!(empty.validate("é"), Err(ValidationError::EmptyProvenance));
        let split = SourceProvenance::original(range(1, 2));
        assert_eq!(
            split.validate("é"),
            Err(ValidationError::InvalidUtf8Boundary)
        );
        let unordered = SourceProvenance::Derived {
            ranges: vec![range(3, 4), range(0, 1)],
            primary_range_index: 0,
            transform: TransformId("merge".into()),
        };
        assert_eq!(
            unordered.validate("abcd"),
            Err(ValidationError::UnorderedProvenanceRanges)
        );
        let overlapping = SourceProvenance::Derived {
            ranges: vec![range(0, 3), range(2, 4)],
            primary_range_index: 0,
            transform: TransformId("merge".into()),
        };
        assert_eq!(
            overlapping.validate("abcd"),
            Err(ValidationError::OverlappingProvenanceRanges)
        );
        let adjacent = SourceProvenance::Derived {
            ranges: vec![range(0, 2), range(2, 4)],
            primary_range_index: 1,
            transform: TransformId("merge".into()),
        };
        assert_eq!(adjacent.validate("abcd"), Ok(()));
    }

    #[test]
    fn provenance_round_trips_without_collapsing_ranges() {
        let value = SourceProvenance::Derived {
            ranges: vec![range(0, 1), range(3, 4)],
            primary_range_index: 1,
            transform: TransformId("merge".into()),
        };
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(
            serde_json::from_str::<SourceProvenance>(&json).unwrap(),
            value
        );
    }

    #[test]
    fn validates_utf16_positions_without_accepting_surrogate_midpoints() {
        let source = "a🦀b";
        let index = SourceIndex::new(source);
        let position = SourcePosition {
            line: 0,
            character: 3,
            encoding: PositionEncoding::Utf16,
        };
        assert_eq!(index.position_to_byte(&position), Some(5));
        assert_eq!(
            index.byte_to_position(5, PositionEncoding::Utf16),
            Some(position)
        );
        assert_eq!(
            index.position_to_byte(&SourcePosition {
                line: 0,
                character: 2,
                encoding: PositionEncoding::Utf16
            }),
            None
        );
    }

    #[test]
    fn index_round_trips_every_unicode_boundary_and_encoding() {
        let source = "a🦀é\r\n日本語\n";
        let index = SourceIndex::new(source);
        for offset in 0..=source.len() {
            for encoding in [
                PositionEncoding::Utf8,
                PositionEncoding::Utf16,
                PositionEncoding::Utf32,
            ] {
                let position = index.byte_to_position(offset, encoding);
                if source.is_char_boundary(offset) {
                    let expected = if source.as_bytes().get(offset) == Some(&b'\n')
                        && offset > 0
                        && source.as_bytes()[offset - 1] == b'\r'
                    {
                        offset - 1
                    } else {
                        offset
                    };
                    assert_eq!(index.position_to_byte(&position.unwrap()), Some(expected));
                } else {
                    assert_eq!(position, None);
                }
            }
        }
        assert_eq!(index.range(2..5), Err(ValidationError::InvalidUtf8Boundary));
        assert_eq!(
            index.range(std::ops::Range { start: 5, end: 2 }),
            Err(ValidationError::InvalidRange)
        );
    }

    #[test]
    fn external_positions_exclude_crlf_and_preserve_final_empty_line() {
        let source = "🦀\r\né\n";
        let index = SourceIndex::new(source);
        let position = |line, character| SourcePosition {
            line,
            character,
            encoding: PositionEncoding::Utf16,
        };
        assert_eq!(index.position_to_byte(&position(0, 1)), None);
        assert_eq!(index.position_to_byte(&position(0, 2)), Some(4));
        assert_eq!(index.position_to_byte(&position(0, 3)), None);
        assert_eq!(
            index.byte_to_position(4, PositionEncoding::Utf16),
            Some(position(0, 2))
        );
        assert_eq!(
            index.byte_to_position(5, PositionEncoding::Utf16),
            Some(position(0, 2))
        );
        assert_eq!(
            index.byte_to_position(6, PositionEncoding::Utf16),
            Some(position(1, 0))
        );
        assert_eq!(
            index.byte_to_position(source.len(), PositionEncoding::Utf16),
            Some(position(2, 0))
        );
        assert_eq!(index.position_to_byte(&position(2, 0)), Some(source.len()));
        assert_eq!(index.position_to_byte(&position(2, 1)), None);
    }

    #[test]
    fn rejects_incompatible_children_without_rejecting_container_forms() {
        let leaf = Block {
            id: NodeId("child".into()),
            provenance: SourceProvenance::original(range(0, 1)),
            kind: BlockKind::Paragraph,
            attributes: BTreeMap::new(),
            children: vec![],
        };
        for kind in [
            BlockKind::Paragraph,
            BlockKind::Heading { level: 1 },
            BlockKind::Table,
            BlockKind::TableRow { header: false },
            BlockKind::Tabs,
        ] {
            let parent = Block {
                id: NodeId("parent".into()),
                provenance: leaf.provenance.clone(),
                kind,
                attributes: BTreeMap::new(),
                children: vec![Node::Block(leaf.clone())],
            };
            let document = Document {
                schema_version: DOCUMENT_SCHEMA_VERSION,
                document_version: 1,
                uri: DocumentUri("test".into()),
                metadata: DocumentMetadata::default(),
                blocks: vec![parent],
            };
            assert_eq!(
                document.validate("x"),
                Err(ValidationError::InvalidChildShape)
            );
        }
        let mut ids = HashSet::new();
        let parent = Block {
            id: NodeId("parent".into()),
            provenance: leaf.provenance.clone(),
            kind: BlockKind::ListItem { checked: None },
            attributes: BTreeMap::new(),
            children: vec![Node::Block(leaf)],
        };
        assert_eq!(
            validate_block(&parent, &SourceIndex::new("x"), &mut ids),
            Ok(())
        );
    }

    #[test]
    fn generated_navigation_resolves_to_the_declared_anchor_edge() {
        let anchor = range(1, 3);
        for (affinity, expected) in [(AnchorAffinity::Before, 1), (AnchorAffinity::After, 3)] {
            let provenance = SourceProvenance::Generated {
                anchor: Some(GeneratedAnchor {
                    range: anchor.clone(),
                    affinity,
                }),
                transform: TransformId("generated".into()),
            };
            let navigation = provenance.navigation_range().unwrap();
            assert_eq!(
                (navigation.byte_start, navigation.byte_end),
                (expected, expected)
            );
        }
    }
}
