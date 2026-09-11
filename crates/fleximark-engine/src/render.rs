use std::collections::BTreeMap;

use fleximark_model::{Document, NavigationEntry, NodeId};
use fleximark_plugin_host::{
    CancellationToken, PluginDiagnostic, PluginHost, PluginRun, UnsafeExportOutput,
};
use fleximark_render_html::{
    HtmlRenderer, HtmlTarget, RawHtmlPolicy, RenderContext, RenderedBlock,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::assets::{RenderAsset, RenderConfig, RenderStyle};
use crate::diff::{ROOT_NODE_ID, RenderPatch, diff_blocks};
use crate::error::EngineError;
use crate::session::{DocumentSession, PreviewCache, PreviewSessionId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderSnapshot {
    pub preview_session_id: PreviewSessionId,
    pub document_version: u64,
    pub result_render_revision: u64,
    pub renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub assets: Vec<RenderAsset>,
    pub node_ids: Vec<NodeId>,
    pub navigation: Vec<NavigationEntry>,
    pub html: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RenderPublication {
    Full(RenderSnapshot),
    Patch(RenderPatch),
}

#[derive(Debug)]
pub struct PluginRenderPublication {
    pub publication: RenderPublication,
    pub annotations: BTreeMap<String, String>,
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

struct RenderPreparation {
    revision: u64,
    fingerprint: String,
    blocks: Vec<RenderedBlock>,
}

impl DocumentSession {
    pub fn render(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
    ) -> Result<RenderPublication, EngineError> {
        self.render_internal(preview_session_id, context, &BTreeMap::new())
    }

    fn render_internal(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        annotations: &BTreeMap<String, String>,
    ) -> Result<RenderPublication, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let RenderPreparation {
            revision,
            fingerprint,
            blocks,
        } = self.prepare_render(&preview_session_id, context, annotations)?;
        let previous = self.previews.get(&preview_session_id).cloned();
        let publication = match previous {
            None => RenderPublication::Full(build_render_snapshot(
                &preview_session_id,
                &self.document,
                revision,
                &fingerprint,
                &self.render_config,
                &blocks,
                annotations,
            )),
            Some(cache) if cache.fingerprint != fingerprint => {
                RenderPublication::Full(build_render_snapshot(
                    &preview_session_id,
                    &self.document,
                    revision,
                    &fingerprint,
                    &self.render_config,
                    &blocks,
                    annotations,
                ))
            }
            Some(cache) => RenderPublication::Patch(RenderPatch {
                preview_session_id: preview_session_id.clone(),
                document_version: self.document.document_version,
                base_render_revision: cache.revision,
                result_render_revision: revision,
                base_renderer_fingerprint: fingerprint.clone(),
                result_renderer_fingerprint: fingerprint.clone(),
                style: self.render_config.style.clone(),
                navigation: blocks
                    .iter()
                    .flat_map(|block| block.navigation.iter().cloned())
                    .collect(),
                operations: diff_blocks(&cache.blocks, &blocks),
            }),
        };
        self.commit_render(preview_session_id, revision, fingerprint, blocks);
        Ok(publication)
    }

    #[cfg(test)]
    pub(super) fn render_internal_for_test(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        annotations: &BTreeMap<String, String>,
    ) -> Result<RenderPublication, EngineError> {
        self.render_internal(preview_session_id, context, annotations)
    }

    pub fn render_full(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
    ) -> Result<RenderSnapshot, EngineError> {
        self.render_full_internal(preview_session_id, context, &BTreeMap::new())
    }

    pub fn dispose_preview(&mut self, preview_session_id: &PreviewSessionId) -> bool {
        self.previews.remove(preview_session_id).is_some()
    }

    #[cfg(test)]
    pub(super) fn preview_count(&self) -> usize {
        self.previews.len()
    }

    fn render_full_internal(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        annotations: &BTreeMap<String, String>,
    ) -> Result<RenderSnapshot, EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        let RenderPreparation {
            revision,
            fingerprint,
            blocks,
        } = self.prepare_render(&preview_session_id, context, annotations)?;
        let snapshot = build_render_snapshot(
            &preview_session_id,
            &self.document,
            revision,
            &fingerprint,
            &self.render_config,
            &blocks,
            annotations,
        );
        self.commit_render(preview_session_id, revision, fingerprint, blocks);
        Ok(snapshot)
    }

    fn prepare_render(
        &self,
        preview_session_id: &PreviewSessionId,
        context: &RenderContext,
        annotations: &BTreeMap<String, String>,
    ) -> Result<RenderPreparation, EngineError> {
        let fingerprint = renderer_fingerprint(context, &self.render_config, annotations);
        let blocks = HtmlRenderer.render_blocks(&self.document, context)?;
        let revision = self
            .previews
            .get(preview_session_id)
            .map_or(1, |cache| cache.revision + 1);
        Ok(RenderPreparation {
            revision,
            fingerprint,
            blocks,
        })
    }

    fn commit_render(
        &mut self,
        preview_session_id: PreviewSessionId,
        revision: u64,
        fingerprint: String,
        blocks: Vec<RenderedBlock>,
    ) {
        self.previews.insert(
            preview_session_id,
            PreviewCache {
                revision,
                fingerprint,
                blocks,
            },
        );
    }

    fn render_with_plugins(
        &mut self,
        preview_session_id: PreviewSessionId,
        context: &RenderContext,
        host: &PluginHost,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderPublication, EngineError> {
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
        let publication = self.render_internal(preview_session_id, context, &extension.value)?;
        Ok(PluginRenderPublication {
            publication,
            annotations: extension.value,
            diagnostics: extension.diagnostics,
        })
    }

    pub fn render_configured(
        &mut self,
        preview_session_id: PreviewSessionId,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderPublication, EngineError> {
        if cancellation.is_cancelled() {
            return Err(EngineError::Plugin("operation cancelled".to_owned()));
        }
        let context = self.render_config.context.clone();
        let Some(host) = self.plugins.clone() else {
            return Ok(PluginRenderPublication {
                publication: self.render_internal(
                    preview_session_id,
                    &context,
                    &BTreeMap::new(),
                )?,
                annotations: BTreeMap::new(),
                diagnostics: Vec::new(),
            });
        };
        self.render_with_plugins(preview_session_id, &context, &host, cancellation)
    }

    pub fn render_full_configured(
        &mut self,
        preview_session_id: PreviewSessionId,
        cancellation: &CancellationToken,
    ) -> Result<PluginRenderPublication, EngineError> {
        if cancellation.is_cancelled() {
            return Err(EngineError::Plugin("operation cancelled".to_owned()));
        }
        let context = self.render_config.context.clone();
        let extension = match self.plugins.clone() {
            Some(host) => host
                .extend_render_model(
                    &self.document,
                    match context.target {
                        HtmlTarget::Preview => "preview",
                        HtmlTarget::Portable => "portable",
                    },
                    cancellation,
                )
                .map_err(|error| EngineError::Plugin(error.to_string()))?,
            None => PluginRun {
                value: BTreeMap::new(),
                diagnostics: Vec::new(),
            },
        };
        if cancellation.is_cancelled() {
            return Err(EngineError::Plugin("operation cancelled".to_owned()));
        }
        let snapshot = self.render_full_internal(preview_session_id, &context, &extension.value)?;
        Ok(PluginRenderPublication {
            publication: RenderPublication::Full(snapshot),
            annotations: extension.value,
            diagnostics: extension.diagnostics,
        })
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

fn build_render_snapshot(
    preview: &PreviewSessionId,
    document: &Document,
    revision: u64,
    fingerprint: &str,
    config: &RenderConfig,
    blocks: &[RenderedBlock],
    annotations: &BTreeMap<String, String>,
) -> RenderSnapshot {
    let annotation_html = if annotations.is_empty() {
        String::new()
    } else {
        let json = serde_json::to_string(annotations)
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
    RenderSnapshot {
        preview_session_id: preview.clone(),
        document_version: document.document_version,
        result_render_revision: revision,
        renderer_fingerprint: fingerprint.to_owned(),
        style: config.style.clone(),
        assets: config
            .assets
            .iter()
            .map(|asset| asset.published.clone())
            .collect(),
        node_ids: std::iter::once(NodeId(ROOT_NODE_ID.to_owned()))
            .chain(
                blocks
                    .iter()
                    .flat_map(|block| block.node_ids.iter().cloned()),
            )
            .collect(),
        navigation: blocks
            .iter()
            .flat_map(|block| block.navigation.iter().cloned())
            .collect(),
        html: format!(
            "<main data-fleximark-node-id=\"{ROOT_NODE_ID}\">{annotation_html}{}</main>",
            blocks
                .iter()
                .map(|block| block.html.as_str())
                .collect::<String>()
        ),
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
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}
