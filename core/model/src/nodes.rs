//! Role: Define document nodes, source positions, and byte-range validation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DocumentUri(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn pending(index: usize) -> Self {
        Self(format!("pending-{index}"))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentMetadata {
    pub front_matter: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub document_version: u64,
    pub uri: DocumentUri,
    pub metadata: DocumentMetadata,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub id: NodeId,
    pub range: SourceRange,
    pub kind: BlockKind,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Block(Block),
    Inline(Inline),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inline {
    pub range: SourceRange,
    pub kind: InlineKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    Directive {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourcePosition {
    pub line: u64,
    pub character: u64,
    pub encoding: PositionEncoding,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceRange {
    pub byte_start: u64,
    pub byte_end: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationEntry {
    pub node_id: NodeId,
    pub source_range: SourceRange,
    pub depth: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    DuplicateNodeId(String),
    InvalidRange,
    InvalidUtf8Boundary,
    InvalidChildShape,
}
impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNodeId(value) => {
                write!(f, "a block NodeId occurs more than once: {0}", value)
            }
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
