//! Role: Produce document render snapshots and policy-checked export stages.
use std::collections::BTreeMap;

use fleximark_model::{NavigationEntry, NodeId};
use fleximark_plugin_api::{
    CancellationToken, PluginDiagnostic, PluginExecutor, PluginRun, UnsafeExportOutput,
};
use fleximark_render_html::{HtmlRenderer, HtmlTarget, RenderContext, RenderedBlock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::assets::{RenderAsset, RenderConfig, RenderStyle};
use crate::error::EngineError;
use crate::session::DocumentEngine;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderSnapshot {
    pub document_version: u64,
    pub renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub assets: Vec<RenderAsset>,
    pub blocks: Vec<RenderBlock>,
    pub navigation: Vec<NavigationEntry>,
    pub annotations: BTreeMap<String, String>,
}

impl RenderSnapshot {
    pub fn html(&self) -> String {
        let annotations = if self.annotations.is_empty() {
            String::new()
        } else {
            let json = serde_json::to_string(&self.annotations)
                .expect("render annotations are serializable")
                .replace('&', "\\u0026")
                .replace('<', "\\u003c")
                .replace('>', "\\u003e")
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029");
            format!(
                "<script type=\"application/json\" data-fleximark-render-annotations>{json}</script>"
            )
        };
        format!(
            "<main data-fleximark-node-id=\"document-root\">{annotations}{}</main>",
            self.blocks
                .iter()
                .map(|block| block.html.as_str())
                .collect::<String>()
        )
    }
}

#[derive(Debug)]
pub struct PluginRenderSnapshot {
    pub snapshot: RenderSnapshot,
    pub diagnostics: Vec<PluginDiagnostic>,
}

pub struct PreparedExport {
    safe_html: String,
    style: Option<RenderStyle>,
    assets: Vec<RenderAsset>,
}

pub struct ResolvedExport {
    html: String,
}

impl PreparedExport {
    pub fn safe_html(&self) -> &str {
        &self.safe_html
    }

    pub fn style(&self) -> Option<&RenderStyle> {
        self.style.as_ref()
    }

    pub fn assets(&self) -> &[RenderAsset] {
        &self.assets
    }

    pub fn compose_portable<E>(
        self,
        common_runtime: &str,
        composer: impl FnOnce(&str, Option<&RenderStyle>, &[RenderAsset], &str) -> Result<String, E>,
    ) -> Result<ResolvedExport, E> {
        composer(
            &self.safe_html,
            self.style.as_ref(),
            &self.assets,
            common_runtime,
        )
        .map(|html| ResolvedExport { html })
    }
}

impl DocumentEngine {
    /// Renders the current document without session or revision framing.
    pub fn render(&mut self, context: &RenderContext) -> Result<RenderSnapshot, EngineError> {
        self.generate_frame(context, BTreeMap::new())
    }

    fn generate_frame(
        &mut self,
        context: &RenderContext,
        annotations: BTreeMap<String, String>,
    ) -> Result<RenderSnapshot, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let fingerprint = renderer_fingerprint(context, &self.render_config, &annotations);
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
            annotations,
        };
        Ok(frame)
    }

    #[cfg(test)]
    pub(super) fn render_internal_for_test(
        &mut self,
        context: &RenderContext,
        annotations: &BTreeMap<String, String>,
    ) -> Result<RenderSnapshot, EngineError> {
        self.generate_frame(context, annotations.clone())
    }

    fn render_with_plugins(
        &mut self,
        context: &RenderContext,
        host: &dyn PluginExecutor,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderSnapshot, EngineError> {
        let target = match context.target {
            HtmlTarget::Preview => "preview",
            HtmlTarget::Portable => "portable",
        };
        let extension = host
            .extend_render_model(&self.document, target, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        if cancellation.is_cancelled() {
            return Err(EngineError::Plugin("operation cancelled".to_owned()));
        }
        Ok(PluginRenderSnapshot {
            snapshot: self.generate_frame(context, extension.value)?,
            diagnostics: extension.diagnostics,
        })
    }

    pub fn render_configured(
        &mut self,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderSnapshot, EngineError> {
        if cancellation.is_cancelled() {
            return Err(EngineError::Plugin("operation cancelled".to_owned()));
        }
        let context = self.render_config.context.clone();
        match self.plugins.clone() {
            Some(host) => self.render_with_plugins(&context, host.as_ref(), cancellation),
            None => Ok(PluginRenderSnapshot {
                snapshot: self.generate_frame(&context, BTreeMap::new())?,
                diagnostics: Vec::new(),
            }),
        }
    }

    pub fn prepare_safe_export(
        &self,
        context: &RenderContext,
    ) -> Result<PreparedExport, EngineError> {
        if context.target != HtmlTarget::Portable {
            return Err(EngineError::UnsafeExportPolicy);
        }
        Ok(PreparedExport {
            safe_html: HtmlRenderer.render(&self.document, context)?,
            style: self.render_config.style.clone(),
            assets: self.render_config.assets().cloned().collect(),
        })
    }

    pub fn apply_unsafe_export_html(
        &self,
        export: ResolvedExport,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<UnsafeExportOutput>, EngineError> {
        match &self.plugins {
            Some(host) => host
                .unsafe_export_html(self.document.document_version, export.html, cancellation)
                .map_err(|error| EngineError::Plugin(error.to_string())),
            None => Ok(PluginRun {
                value: UnsafeExportOutput {
                    html: export.html,
                    unsafe_output_used: false,
                },
                diagnostics: Vec::new(),
            }),
        }
    }
}

fn renderer_fingerprint(
    context: &RenderContext,
    config: &RenderConfig,
    annotations: &BTreeMap<String, String>,
) -> String {
    let target = match context.target {
        HtmlTarget::Preview => "preview",
        HtmlTarget::Portable => "portable",
    };
    let style_fingerprint = config
        .style
        .as_ref()
        .map_or("none", RenderStyle::fingerprint);
    let annotations = serde_json::to_string(annotations).expect("render annotations serialize");
    let assets = serde_json::to_string(
        &config
            .assets
            .iter()
            .map(|asset| &asset.published)
            .collect::<Vec<_>>(),
    )
    .expect("resolved assets serialize");
    let asset_diagnostics =
        serde_json::to_string(&config.asset_diagnostics).expect("asset diagnostics serialize");
    let value = format!(
        "{}\0{}\0{target}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{annotations}\0{assets}\0{asset_diagnostics}",
        config.renderer_version,
        config.sanitizer_version,
        context.allow_remote_resources,
        context.allow_data_resources,
        style_fingerprint,
        config.config_hash,
        config.config_generation,
        config.plugin_set_hash,
        config.plugin_generation,
    );
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
