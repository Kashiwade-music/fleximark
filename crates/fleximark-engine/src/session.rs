use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fleximark_model::{Document, DocumentUri, NavigationEntry, PositionEncoding};
use fleximark_plugin_host::{CancellationToken, PluginDiagnostic, PluginHost};
use serde::{Deserialize, Serialize};

use crate::assets::{AssetDiagnostic, RenderConfig};
use crate::error::EngineError;
use crate::identity::content_hash;
use crate::pipeline::{CandidateHooks, SourceCandidateInput, prepare_source_candidate};
use crate::render::RenderFrame;

static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentSessionId(pub String);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct PreviewSessionId(pub String);

#[derive(Clone)]
pub(super) struct PreviewCache {
    pub(super) frame: RenderFrame,
}

pub struct DocumentSession {
    pub(super) id: DocumentSessionId,
    pub(super) source: String,
    pub(super) position_encoding: PositionEncoding,
    pub(super) document: Document,
    pub(super) out_of_sync: bool,
    pub(super) render_config: RenderConfig,
    pub(super) plugins: Option<Arc<PluginHost>>,
    pub(super) plugin_diagnostics: Vec<PluginDiagnostic>,
    pub(super) previews: HashMap<PreviewSessionId, PreviewCache>,
}

impl DocumentSession {
    pub fn open(
        uri: DocumentUri,
        version: u64,
        source: String,
        position_encoding: PositionEncoding,
    ) -> Result<Self, EngineError> {
        let candidate = prepare_source_candidate(SourceCandidateInput {
            uri,
            previous: None,
            version,
            source,
            hooks: CandidateHooks::Empty,
        })?;
        Ok(Self::from_candidate(
            candidate,
            position_encoding,
            RenderConfig::default(),
            None,
        ))
    }

    pub fn open_configured(
        uri: DocumentUri,
        version: u64,
        source: String,
        position_encoding: PositionEncoding,
        render_config: RenderConfig,
        plugins: Arc<PluginHost>,
        cancellation: &CancellationToken,
    ) -> Result<Self, EngineError> {
        render_config.validate_for(&plugins)?;
        let candidate = prepare_source_candidate(SourceCandidateInput {
            uri,
            previous: None,
            version,
            source,
            hooks: CandidateHooks::Plugins {
                host: &plugins,
                cancellation,
            },
        })?;
        Ok(Self::from_candidate(
            candidate,
            position_encoding,
            render_config,
            Some(plugins),
        ))
    }

    pub fn id(&self) -> &DocumentSessionId {
        &self.id
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }
    pub fn is_out_of_sync(&self) -> bool {
        self.out_of_sync
    }
    pub fn content_hash(&self) -> String {
        content_hash(&self.source)
    }
    pub fn render_config(&self) -> &RenderConfig {
        &self.render_config
    }
    pub fn plugin_diagnostics(&self) -> &[PluginDiagnostic] {
        &self.plugin_diagnostics
    }
    pub fn asset_diagnostics(&self) -> &[AssetDiagnostic] {
        self.render_config.asset_diagnostics()
    }

    pub fn node_at_source_offset(&self, byte_offset: u64) -> Option<NavigationEntry> {
        let offset = usize::try_from(byte_offset).ok()?;
        if !self.source.is_char_boundary(offset) {
            return None;
        }
        self.document
            .navigation()
            .into_iter()
            .filter(|entry| {
                (entry.source_range.byte_start.get() <= byte_offset
                    && byte_offset < entry.source_range.byte_end.get())
                    || (entry.source_range.byte_start.get() == byte_offset
                        && entry.source_range.byte_end.get() == byte_offset)
            })
            .min_by(|left, right| {
                let left_span =
                    left.source_range.byte_end.get() - left.source_range.byte_start.get();
                let right_span =
                    right.source_range.byte_end.get() - right.source_range.byte_start.get();
                left_span
                    .cmp(&right_span)
                    .then_with(|| right.depth.cmp(&left.depth))
                    .then_with(|| left.node_id.0.cmp(&right.node_id.0))
            })
    }

    pub fn reconfigure(
        &mut self,
        render_config: RenderConfig,
        plugins: Arc<PluginHost>,
        cancellation: &CancellationToken,
    ) -> Result<(), EngineError> {
        render_config.validate_for(&plugins)?;
        let diagnostics = self.install_source(
            self.document.document_version,
            self.source.clone(),
            CandidateHooks::Plugins {
                host: &plugins,
                cancellation,
            },
        )?;
        self.render_config = render_config;
        self.plugins = Some(plugins);
        self.plugin_diagnostics = diagnostics;
        Ok(())
    }

    pub fn adopt_reconfiguration(&mut self, configured: Self) -> Result<(), EngineError> {
        if self.document.uri != configured.document.uri
            || self.document.document_version != configured.document.document_version
            || self.source != configured.source
            || self.position_encoding != configured.position_encoding
        {
            return Err(EngineError::Plugin(
                "reconfigured session does not match the authoritative source".to_owned(),
            ));
        }
        self.document = configured.document;
        self.render_config = configured.render_config;
        self.plugins = configured.plugins;
        self.plugin_diagnostics = configured.plugin_diagnostics;
        self.out_of_sync = configured.out_of_sync;
        Ok(())
    }

    /// Replaces publication-only configuration without re-running IR plugin hooks.
    pub fn reconfigure_render(&mut self, render_config: RenderConfig) -> Result<(), EngineError> {
        let Some(plugins) = self.plugins.as_ref() else {
            return Err(EngineError::Plugin(
                "render-only reconfiguration requires the session plugin set".to_owned(),
            ));
        };
        render_config.validate_for(plugins)?;
        self.render_config = render_config;
        Ok(())
    }

    pub fn change_full_text_with_cancellation(
        &mut self,
        version: u64,
        source: String,
        cancellation: &CancellationToken,
    ) -> Result<(), EngineError> {
        if version <= self.document.document_version {
            self.mark_out_of_sync();
            return Err(EngineError::StaleVersion {
                current: self.document.document_version,
                received: version,
            });
        }
        self.require_in_sync()?;
        self.install_current_source(version, source, cancellation)
    }

    pub fn resynchronize_with_cancellation(
        &mut self,
        version: u64,
        source: String,
        cancellation: &CancellationToken,
    ) -> Result<(), EngineError> {
        if version < self.document.document_version
            || (!self.out_of_sync && version == self.document.document_version)
        {
            return Err(EngineError::StaleVersion {
                current: self.document.document_version,
                received: version,
            });
        }
        self.install_current_source(version, source, cancellation)?;
        self.mark_in_sync();
        Ok(())
    }

    fn install_current_source(
        &mut self,
        version: u64,
        source: String,
        cancellation: &CancellationToken,
    ) -> Result<(), EngineError> {
        let plugins = self.plugins.clone();
        let hooks =
            plugins
                .as_deref()
                .map_or(CandidateHooks::Empty, |host| CandidateHooks::Plugins {
                    host,
                    cancellation,
                });
        self.plugin_diagnostics = self.install_source(version, source, hooks)?;
        Ok(())
    }

    fn require_in_sync(&self) -> Result<(), EngineError> {
        if self.out_of_sync {
            return Err(EngineError::ContentModified);
        }
        Ok(())
    }

    pub fn mark_out_of_sync(&mut self) {
        self.out_of_sync = true;
    }

    fn mark_in_sync(&mut self) {
        self.out_of_sync = false;
    }
}

pub(super) fn new_session_id(uri: &DocumentUri) -> DocumentSessionId {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let value = format!("{}\0{time}\0{sequence}", uri.0);
    DocumentSessionId(format!(
        "document-{}",
        &blake3::hash(value.as_bytes()).to_hex()[..24]
    ))
}
