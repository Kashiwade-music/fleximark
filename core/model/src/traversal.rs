//! Role: Traverse document nodes and collect unique block navigation metadata.
use crate::nodes::{Block, Inline, InlineKind, NavigationEntry, Node, ValidationError};
use std::collections::HashSet;

impl InlineKind {
    pub fn children(&self) -> &[Inline] {
        match self {
            Self::Emphasis { children }
            | Self::Strong { children }
            | Self::Strikethrough { children }
            | Self::Link { children, .. }
            | Self::Image { children, .. } => children,
            _ => &[],
        }
    }
}

pub trait Visitor {
    fn visit_node(&mut self, _node: &Node) {}
    fn visit_block(&mut self, _block: &Block, _depth: u32) {}
    fn visit_inline(&mut self, _inline: &Inline) {}
}

enum Entry<'a> {
    Node(&'a Node, u32),
    Block(&'a Block, u32),
    Inline(&'a Inline),
}

pub fn walk_blocks(blocks: &[Block], visitor: &mut impl Visitor) {
    let mut pending = blocks
        .iter()
        .rev()
        .map(|block| Entry::Block(block, 0))
        .collect::<Vec<_>>();
    while let Some(entry) = pending.pop() {
        match entry {
            Entry::Node(node, depth) => {
                visitor.visit_node(node);
                pending.push(match node {
                    Node::Block(block) => Entry::Block(block, depth),
                    Node::Inline(inline) => Entry::Inline(inline),
                });
            }
            Entry::Block(block, depth) => {
                visitor.visit_block(block, depth);
                pending.extend(
                    block
                        .children
                        .iter()
                        .rev()
                        .map(|node| Entry::Node(node, depth + 1)),
                );
            }
            Entry::Inline(inline) => {
                visitor.visit_inline(inline);
                pending.extend(inline.kind.children().iter().rev().map(Entry::Inline));
            }
        }
    }
}

pub trait VisitorMut {
    fn visit_block(&mut self, _block: &mut Block, _depth: u32) {}
}

pub fn walk_blocks_mut(blocks: &mut [Block], visitor: &mut impl VisitorMut) {
    let mut pending = blocks
        .iter_mut()
        .rev()
        .map(|block| (block, 0))
        .collect::<Vec<_>>();
    while let Some((block, depth)) = pending.pop() {
        visitor.visit_block(block, depth);
        pending.extend(
            block
                .children
                .iter_mut()
                .rev()
                .filter_map(|node| match node {
                    Node::Block(child) => Some((child, depth + 1)),
                    Node::Inline(_) => None,
                }),
        );
    }
}

pub fn walk_inlines(inlines: &[Inline], visitor: &mut impl Visitor) {
    let mut pending = inlines.iter().rev().collect::<Vec<_>>();
    while let Some(inline) = pending.pop() {
        visitor.visit_inline(inline);
        pending.extend(inline.kind.children().iter().rev());
    }
}

pub fn collect_navigation(blocks: &[Block]) -> Result<Vec<NavigationEntry>, ValidationError> {
    struct Navigation {
        entries: Vec<NavigationEntry>,
        ids: HashSet<crate::nodes::NodeId>,
        duplicate: Option<String>,
    }
    impl Visitor for Navigation {
        fn visit_block(&mut self, block: &Block, depth: u32) {
            if !self.ids.insert(block.id.clone()) && self.duplicate.is_none() {
                self.duplicate = Some(block.id.0.clone());
            }
            self.entries.push(NavigationEntry {
                node_id: block.id.clone(),
                source_range: block.range.clone(),
                depth,
            });
        }
    }
    let mut visitor = Navigation {
        entries: Vec::new(),
        ids: HashSet::new(),
        duplicate: None,
    };
    walk_blocks(blocks, &mut visitor);
    match visitor.duplicate {
        Some(id) => Err(ValidationError::DuplicateNodeId(id)),
        None => Ok(visitor.entries),
    }
}

#[cfg(test)]
mod tests {
    //! Role: Verify traversal ordering and unique subtree navigation metadata.
    use super::*;
    use crate::nodes::{BlockKind, NodeId, SourceRange};
    #[test]
    fn traverses_nested_blocks_and_inlines_in_document_order() -> Result<(), ValidationError> {
        let range = SourceRange {
            byte_start: 0,
            byte_end: 1,
        };
        let leaf = Inline {
            range: range.clone(),
            kind: InlineKind::Text {
                value: "leaf".to_owned(),
            },
        };
        let nested = Inline {
            range: range.clone(),
            kind: InlineKind::Strong {
                children: vec![leaf],
            },
        };
        let child = Block {
            id: NodeId("child".to_owned()),
            range: range.clone(),
            kind: BlockKind::Paragraph,
            children: vec![Node::Inline(nested)],
        };
        let root = Block {
            id: NodeId("root".to_owned()),
            range,
            kind: BlockKind::Quote,
            children: vec![Node::Block(child)],
        };
        #[derive(Default)]
        struct Trace {
            blocks: Vec<(String, u32)>,
            inlines: Vec<&'static str>,
        }
        impl Visitor for Trace {
            fn visit_block(&mut self, block: &Block, depth: u32) {
                self.blocks.push((block.id.0.clone(), depth));
            }
            fn visit_inline(&mut self, inline: &Inline) {
                self.inlines
                    .push(if matches!(inline.kind, InlineKind::Strong { .. }) {
                        "strong"
                    } else {
                        "text"
                    });
            }
        }
        let mut trace = Trace::default();
        walk_blocks(std::slice::from_ref(&root), &mut trace);
        assert_eq!(
            trace.blocks,
            [("root".to_owned(), 0), ("child".to_owned(), 1)]
        );
        assert_eq!(trace.inlines, ["strong", "text"]);
        let navigation = collect_navigation(std::slice::from_ref(&root))?;
        assert_eq!(
            navigation
                .iter()
                .map(|entry| entry.depth)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        struct Rename;
        impl VisitorMut for Rename {
            fn visit_block(&mut self, block: &mut Block, depth: u32) {
                block.id.0 = format!("{}-{depth}", block.id.0);
            }
        }
        let mut renamed = root.clone();
        walk_blocks_mut(std::slice::from_mut(&mut renamed), &mut Rename);
        let renamed_navigation = collect_navigation(&[renamed])?;
        assert_eq!(
            renamed_navigation
                .iter()
                .map(|entry| entry.node_id.0.as_str())
                .collect::<Vec<_>>(),
            ["root-0", "child-1"]
        );
        let mut duplicate = root.clone();
        duplicate.id = NodeId("child".to_owned());
        assert_eq!(
            collect_navigation(&[duplicate]),
            Err(ValidationError::DuplicateNodeId("child".to_owned()))
        );
        Ok(())
    }
}
