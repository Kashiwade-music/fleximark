use std::collections::{BTreeMap, HashSet};

pub use fleximark_wire::JsSafeU64;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DOCUMENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentUri(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum PositionEncoding {
    Utf8,
    Utf16,
    Utf32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct SourcePosition {
    pub line: JsSafeU64,
    pub character: JsSafeU64,
    pub encoding: PositionEncoding,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct SourceRange {
    pub byte_start: JsSafeU64,
    pub byte_end: JsSafeU64,
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
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
                let (byte_offset, position) = match anchor.affinity {
                    AnchorAffinity::Before => (anchor.range.byte_start, anchor.range.start.clone()),
                    AnchorAffinity::After => (anchor.range.byte_end, anchor.range.end.clone()),
                };
                SourceRange {
                    byte_start: byte_offset,
                    byte_end: byte_offset,
                    start: position.clone(),
                    end: position,
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("document schema {actual} is not supported; expected {expected}")]
    Schema { actual: u32, expected: u32 },
    #[error("a block NodeId occurs more than once: {0}")]
    DuplicateNodeId(String),
    #[error("original or derived provenance has no ranges")]
    EmptyProvenance,
    #[error("provenance primary range is out of bounds")]
    InvalidPrimaryRange,
    #[error("provenance ranges are not ordered by original UTF-8 byte offset")]
    UnorderedProvenanceRanges,
    #[error("provenance ranges overlap in the original source snapshot")]
    OverlappingProvenanceRanges,
    #[error("source range is reversed or outside the source snapshot")]
    InvalidRange,
    #[error("source range does not fall on UTF-8 boundaries")]
    InvalidUtf8Boundary,
    #[error("tagged source position does not match its UTF-8 byte offset")]
    InvalidPosition,
}

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
    if source.position_to_byte(&range.start) != Some(start)
        || source.position_to_byte(&range.end) != Some(end)
    {
        return Err(ValidationError::InvalidPosition);
    }
    Ok(())
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

    fn position_to_byte(&self, position: &SourcePosition) -> Option<usize> {
        let line = usize::try_from(position.line).ok()?;
        let character = usize::try_from(position.character).ok()?;
        let line_start = *self.line_starts.get(line)?;
        let line_end = self
            .line_starts
            .get(line + 1)
            .map_or(self.source.len(), |next_start| next_start - 1);
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
    use super::*;

    fn range(start: u64, end: u64) -> SourceRange {
        let start = JsSafeU64::new(start).unwrap();
        let end = JsSafeU64::new(end).unwrap();
        SourceRange {
            byte_start: start,
            byte_end: end,
            start: SourcePosition {
                line: 0.into(),
                character: start,
                encoding: PositionEncoding::Utf8,
            },
            end: SourcePosition {
                line: 0.into(),
                character: end,
                encoding: PositionEncoding::Utf8,
            },
        }
    }

    #[test]
    fn rejects_empty_and_non_utf8_provenance() {
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
    fn provenance_ranges_must_be_ordered_and_non_overlapping() {
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
    fn validates_utf16_positions_without_accepting_surrogate_midpoints() {
        let source = "a🦀b";
        let valid = SourceRange {
            byte_start: 1.into(),
            byte_end: 5.into(),
            start: SourcePosition {
                line: 0.into(),
                character: 1.into(),
                encoding: PositionEncoding::Utf16,
            },
            end: SourcePosition {
                line: 0.into(),
                character: 3.into(),
                encoding: PositionEncoding::Utf16,
            },
        };
        assert_eq!(SourceProvenance::original(valid).validate(source), Ok(()));

        let invalid = SourceRange {
            byte_start: 1.into(),
            byte_end: 5.into(),
            start: SourcePosition {
                line: 0.into(),
                character: 2.into(),
                encoding: PositionEncoding::Utf16,
            },
            end: SourcePosition {
                line: 0.into(),
                character: 3.into(),
                encoding: PositionEncoding::Utf16,
            },
        };
        assert_eq!(
            SourceProvenance::original(invalid).validate(source),
            Err(ValidationError::InvalidPosition)
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
                (navigation.byte_start.get(), navigation.byte_end.get()),
                (expected, expected)
            );
        }
    }
}
