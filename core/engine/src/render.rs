//! Role: Render immutable document snapshots for preview and portable output.
use crate::assets::{RenderAsset, RenderConfig, RenderStyle};
use crate::error::EngineError;
use crate::session::DocumentEngine;
use fleximark_model::{NavigationEntry, NodeId};
use fleximark_render_html::{HtmlRenderer, HtmlTarget, RenderContext, RenderedBlock};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderBlock {
    pub id: NodeId,
    pub html: String,
    pub node_ids: Vec<NodeId>,
}

impl From<RenderedBlock> for RenderBlock {
    fn from(block: RenderedBlock) -> Self {
        Self {
            id: block.id,
            html: block.html,
            node_ids: block.node_ids,
        }
    }
}

/// An editor-independent document rendering result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderSnapshot {
    pub document_version: u64,
    pub renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub assets: Vec<RenderAsset>,
    pub blocks: Vec<RenderBlock>,
    pub navigation: Vec<NavigationEntry>,
}

impl RenderSnapshot {
    pub fn html(&self) -> String {
        format!(
            "<main data-fleximark-node-id=\"document-root\">{}</main>",
            self.blocks
                .iter()
                .map(|block| block.html.as_str())
                .collect::<String>()
        )
    }
}

impl DocumentEngine {
    pub fn render(&self, context: &RenderContext) -> Result<RenderSnapshot, EngineError> {
        self.generate_frame(context)
    }

    fn generate_frame(&self, context: &RenderContext) -> Result<RenderSnapshot, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let fingerprint = renderer_fingerprint(context, &self.render_config)?;
        let rendered = HtmlRenderer.render_blocks(&self.document, context)?;
        let navigation = rendered
            .iter()
            .flat_map(|block| block.navigation.iter().cloned())
            .collect();
        let frame = RenderSnapshot {
            document_version: (self.document.document_version),
            renderer_fingerprint: fingerprint,
            style: self.render_config.style.clone(),
            assets: self
                .render_config
                .assets
                .iter()
                .map(|asset| asset.published.clone())
                .collect(),
            blocks: rendered.into_iter().map(RenderBlock::from).collect(),
            navigation,
        };
        Ok(frame)
    }
}

fn renderer_fingerprint(
    context: &RenderContext,
    config: &RenderConfig,
) -> Result<String, EngineError> {
    let target = match context.target {
        HtmlTarget::Preview => "preview",
        HtmlTarget::Portable => "portable",
    };
    let style_fingerprint = config
        .style
        .as_ref()
        .map_or("none", RenderStyle::fingerprint);
    let assets = serde_json::to_string(
        &config
            .assets
            .iter()
            .map(|asset| &asset.published)
            .collect::<Vec<_>>(),
    )
    .map_err(|error| {
        EngineError::Configuration(format!("cannot fingerprint render assets: {error}"))
    })?;
    let asset_diagnostics = serde_json::to_string(
        &config
            .asset_diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    &diagnostic.source,
                    diagnostic.kind.code(),
                    &diagnostic.message,
                    diagnostic
                        .source_range
                        .as_ref()
                        .map(|range| (range.byte_start, range.byte_end)),
                )
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|error| {
        EngineError::Configuration(format!("cannot fingerprint asset diagnostics: {error}"))
    })?;
    let value = format!(
        "{}\0{}\0{target}\0{}\0{}\0{}\0{}\0{}\0{assets}\0{asset_diagnostics}",
        config.renderer_version,
        config.sanitizer_version,
        context.allow_remote_resources,
        context.allow_data_resources,
        style_fingerprint,
        config.config_hash,
        config.config_generation,
    );
    Ok(format!("{:x}", Sha256::digest(value.as_bytes())))
}
