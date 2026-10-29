//! Role: Validate document structure and expose navigation through shared walkers.
use crate::nodes::{
    Block, BlockKind, Document, Inline, NavigationEntry, Node, NodeId, SourceIndex, ValidationError,
};
use crate::traversal::{Visitor, walk_blocks};
use std::collections::HashSet;

impl Document {
    pub fn navigation(&self) -> Vec<NavigationEntry> {
        struct Navigation(Vec<NavigationEntry>);
        impl Visitor for Navigation {
            fn visit_block(&mut self, block: &Block, depth: u32) {
                self.0.push(NavigationEntry {
                    node_id: block.id.clone(),
                    source_range: block.range.clone(),
                    depth,
                });
            }
        }
        let mut visitor = Navigation(Vec::new());
        walk_blocks(&self.blocks, &mut visitor);
        visitor.0
    }

    pub fn validate(&self, source: &str) -> Result<(), ValidationError> {
        struct Validation<'a> {
            source: SourceIndex<'a>,
            ids: HashSet<NodeId>,
            result: Result<(), ValidationError>,
        }
        impl Visitor for Validation<'_> {
            fn visit_block(&mut self, block: &Block, _depth: u32) {
                if self.result.is_ok() {
                    self.result = validate_block(block, &self.source, &mut self.ids);
                }
            }
            fn visit_inline(&mut self, inline: &Inline) {
                if self.result.is_ok() {
                    self.result = self.source.validate_range(&inline.range);
                }
            }
        }
        let mut visitor = Validation {
            source: SourceIndex::new(source),
            ids: HashSet::new(),
            result: Ok(()),
        };
        walk_blocks(&self.blocks, &mut visitor);
        visitor.result
    }
}

pub(super) fn validate_block(
    block: &Block,
    source: &SourceIndex<'_>,
    ids: &mut HashSet<NodeId>,
) -> Result<(), ValidationError> {
    if !ids.insert(block.id.clone()) {
        return Err(ValidationError::DuplicateNodeId(block.id.0.clone()));
    }
    source.validate_range(&block.range)?;
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

    Ok(())
}
