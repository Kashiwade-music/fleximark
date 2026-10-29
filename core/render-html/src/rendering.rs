//! Role: Coordinate recursive blocks through sanitized fragment callbacks.

use crate::block;
use crate::inline::render_inline_sequence;
use crate::sanitize::render_sanitized_nodes;
use crate::sequences::inline_run;
use crate::types::{RenderContext, RenderError};
use fleximark_model::{Block, BlockKind, Node};

pub(crate) fn render_block(block: &Block, context: &RenderContext) -> Result<String, RenderError> {
    block::render_block(block, context, |children| render_nodes(children, context))
}

pub(crate) fn render_nodes(nodes: &[Node], context: &RenderContext) -> Result<String, RenderError> {
    if nodes.iter().any(|node| {
        matches!(
            node,
            Node::Block(Block {
                kind: BlockKind::RawHtml { .. },
                ..
            })
        )
    }) {
        return render_sanitized_nodes(
            nodes,
            |block| render_block(block, context),
            |inlines| render_inline_sequence(inlines, context),
        );
    }
    let mut html = String::new();
    let mut index = 0;
    while index < nodes.len() {
        match &nodes[index] {
            Node::Block(block) => {
                html.push_str(&render_block(block, context)?);
                index += 1;
            }
            Node::Inline(_) => {
                let (end, inlines) = inline_run(nodes, index);
                html.push_str(&render_inline_sequence(&inlines, context)?);
                index = end;
            }
        }
    }
    Ok(html)
}
