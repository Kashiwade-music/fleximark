//! Role: Render document nodes as sanitized HTML.
use fleximark_model::{BlockKind, Document, ValidationError, collect_navigation};

mod block;
mod escaping;
mod inline;
mod rendering;
mod resource_policy;
mod sanitize;
mod sequences;
mod types;

use rendering::render_block;
use sanitize::render_sanitized_block_sequence;
use types::BlockMetadata;
pub use types::{HtmlTarget, RenderContext, RenderError, RenderedBlock};

pub struct HtmlRenderer;

impl HtmlRenderer {
    pub fn render(
        &self,
        document: &Document,
        context: &RenderContext,
    ) -> Result<String, RenderError> {
        Ok(self
            .render_blocks(document, context)?
            .into_iter()
            .map(|block| block.html)
            .collect())
    }

    pub fn render_blocks(
        &self,
        document: &Document,
        context: &RenderContext,
    ) -> Result<Vec<RenderedBlock>, RenderError> {
        let metadata = render_metadata(&document.blocks)?;
        if document
            .blocks
            .iter()
            .any(|block| matches!(block.kind, BlockKind::RawHtml { .. }))
        {
            return render_sanitized_block_sequence(&document.blocks, metadata, |block| {
                render_block(block, context)
            });
        }
        let mut rendered = Vec::with_capacity(document.blocks.len());
        for (block, metadata) in document.blocks.iter().zip(metadata) {
            rendered.push(RenderedBlock {
                id: block.id.clone(),
                node_ids: metadata.node_ids,
                navigation: metadata.navigation,
                html: render_block(block, context)?,
            });
        }
        Ok(rendered)
    }
}

fn render_metadata(blocks: &[fleximark_model::Block]) -> Result<Vec<BlockMetadata>, RenderError> {
    let navigation = collect_navigation(blocks).map_err(|error| match error {
        ValidationError::DuplicateNodeId(id) => RenderError::DuplicateNodeId(id),
        _ => RenderError::InvalidRawHtmlBoundary,
    })?;
    let mut metadata: Vec<BlockMetadata> = Vec::with_capacity(blocks.len());
    for entry in navigation {
        if entry.depth == 0 {
            metadata.push(BlockMetadata {
                node_ids: Vec::new(),
                navigation: Vec::new(),
            });
        }
        let Some(current) = metadata.last_mut() else {
            return Err(RenderError::InvalidRawHtmlBoundary);
        };
        current.node_ids.push(entry.node_id.clone());
        current.navigation.push(entry);
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests;
