//! Role: Split mixed nodes into ordered blocks and contiguous inline runs.

use fleximark_model::{Inline, Node};

pub(crate) fn inline_run(nodes: &[Node], start: usize) -> (usize, Vec<&Inline>) {
    let end = nodes[start..]
        .iter()
        .position(|node| matches!(node, Node::Block(_)))
        .map_or(nodes.len(), |offset| start + offset);
    let inlines = nodes[start..end]
        .iter()
        .filter_map(|node| match node {
            Node::Inline(inline) => Some(inline),
            Node::Block(_) => None,
        })
        .collect();
    (end, inlines)
}
