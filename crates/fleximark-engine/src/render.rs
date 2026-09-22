use std::collections::BTreeMap;

use fleximark_model::{NavigationEntry, NodeId};
use fleximark_plugin_host::{
    CancellationToken, PluginDiagnostic, PluginHost, PluginRun, UnsafeExportOutput,
};
use fleximark_render_html::{
    HtmlRenderer, HtmlTarget, RawHtmlPolicy, RenderContext, RenderedBlock,
};
use fleximark_wire::JsSafeU64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::assets::{RenderAsset, RenderConfig, RenderStyle};
use crate::error::EngineError;
use crate::session::{DocumentSession, PreviewCache, PreviewSessionId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
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

/// The complete, independently applicable state of a preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RenderFrame {
    pub preview_session_id: PreviewSessionId,
    pub document_version: JsSafeU64,
    pub render_revision: JsSafeU64,
    pub renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub assets: Vec<RenderAsset>,
    pub blocks: Vec<RenderBlock>,
    pub navigation: Vec<NavigationEntry>,
    pub annotations: BTreeMap<String, String>,
}

impl RenderFrame {
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

    pub fn navigation_for(
        &self,
        preview_session_id: &PreviewSessionId,
        render_revision: JsSafeU64,
        current_document_version: u64,
        node_id: &NodeId,
    ) -> Result<Option<NavigationEntry>, EngineError> {
        if &self.preview_session_id != preview_session_id
            || self.render_revision != render_revision
            || self.document_version.get() != current_document_version
        {
            return Err(EngineError::ContentModified);
        }
        Ok(self
            .navigation
            .iter()
            .find(|entry| &entry.node_id == node_id)
            .cloned())
    }
}

#[derive(Debug)]
pub struct PluginRenderFrame {
    pub frame: RenderFrame,
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

impl DocumentSession {
    /// Generates and atomically adopts the next preview frame.
    pub fn render(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
    ) -> Result<RenderFrame, EngineError> {
        self.generate_frame(preview_session_id, context, BTreeMap::new())
    }

    fn generate_frame(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        annotations: BTreeMap<String, String>,
    ) -> Result<RenderFrame, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let revision = self
            .previews
            .get(&preview_session_id)
            .map_or(1, |cache| cache.frame.render_revision.get() + 1);
        let fingerprint = renderer_fingerprint(context, &self.render_config, &annotations);
        let rendered = HtmlRenderer.render_blocks(&self.document, context)?;
        let navigation = rendered
            .iter()
            .flat_map(|block| block.navigation.iter().cloned())
            .collect();
        let frame = RenderFrame {
            preview_session_id: preview_session_id.clone(),
            document_version: JsSafeU64::new(self.document.document_version)
                .expect("document version is JavaScript-safe"),
            render_revision: JsSafeU64::new(revision).expect("render revision is JavaScript-safe"),
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
        self.previews.insert(
            preview_session_id,
            PreviewCache {
                frame: frame.clone(),
            },
        );
        Ok(frame)
    }

    #[cfg(test)]
    pub(super) fn render_internal_for_test(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        annotations: &BTreeMap<String, String>,
    ) -> Result<RenderFrame, EngineError> {
        self.generate_frame(preview_session_id, context, annotations.clone())
    }

    /// Reads the adopted frame without rendering or advancing its revision.
    pub fn read_preview_frame(
        &self,
        preview_session_id: &PreviewSessionId,
        after_revision: Option<JsSafeU64>,
    ) -> Option<RenderFrame> {
        let frame = &self.previews.get(preview_session_id)?.frame;
        (after_revision != Some(frame.render_revision)).then(|| frame.clone())
    }

    pub fn navigate_preview(
        &self,
        preview_session_id: &PreviewSessionId,
        render_revision: JsSafeU64,
        node_id: &NodeId,
    ) -> Result<Option<NavigationEntry>, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let frame = self
            .previews
            .get(preview_session_id)
            .ok_or(EngineError::ContentModified)?;
        frame.frame.navigation_for(
            preview_session_id,
            render_revision,
            self.document.document_version,
            node_id,
        )
    }

    pub fn dispose_preview(&mut self, preview_session_id: &PreviewSessionId) -> bool {
        self.previews.remove(preview_session_id).is_some()
    }

    #[cfg(test)]
    pub(super) fn preview_count(&self) -> usize {
        self.previews.len()
    }

    fn render_with_plugins(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        host: &PluginHost,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderFrame, EngineError> {
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
        Ok(PluginRenderFrame {
            frame: self.generate_frame(preview_session_id, context, extension.value)?,
            diagnostics: extension.diagnostics,
        })
    }

    pub fn render_configured(
        &mut self,
        preview_session_id: PreviewSessionId,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderFrame, EngineError> {
        if cancellation.is_cancelled() {
            return Err(EngineError::Plugin("operation cancelled".to_owned()));
        }
        let context = self.render_config.context.clone();
        match self.plugins.clone() {
            Some(host) => {
                self.render_with_plugins(preview_session_id, &context, &host, cancellation)
            }
            None => Ok(PluginRenderFrame {
                frame: self.generate_frame(preview_session_id, &context, BTreeMap::new())?,
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
    let raw = match context.raw_html {
        RawHtmlPolicy::Escape => "escape",
        RawHtmlPolicy::Reject => "reject",
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
        "{}\0{}\0{target}\0{raw}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{annotations}\0{assets}\0{asset_diagnostics}",
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
