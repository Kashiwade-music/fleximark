//! Role: Verify document ranges, child shapes, and source positions.
use crate::nodes::*;
use crate::validation::validate_block;
use std::collections::HashSet;

fn range(start: u64, end: u64) -> SourceRange {
    SourceRange {
        byte_start: start,
        byte_end: end,
    }
}

#[test]
fn rejects_invalid_source_ranges() {
    let index = SourceIndex::new("é");
    assert_eq!(
        index.validate_range(&range(1, 2)),
        Err(ValidationError::InvalidUtf8Boundary)
    );
    assert_eq!(
        index.validate_range(&range(2, 1)),
        Err(ValidationError::InvalidRange)
    );
    assert_eq!(
        index.validate_range(&range(0, 3)),
        Err(ValidationError::InvalidRange)
    );
    assert_eq!(index.validate_range(&range(0, 2)), Ok(()));
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
        range: range(0, 1),
        kind: BlockKind::Paragraph,
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
            range: leaf.range.clone(),
            kind,
            children: vec![Node::Block(leaf.clone())],
        };
        let document = Document {
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
        range: leaf.range.clone(),
        kind: BlockKind::ListItem { checked: None },
        children: vec![Node::Block(leaf)],
    };
    assert_eq!(
        validate_block(&parent, &SourceIndex::new("x"), &mut ids),
        Ok(())
    );
}
