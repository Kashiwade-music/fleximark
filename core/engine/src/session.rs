//! Role: Manage transactional document state and plugin processing.
use std::sync::Arc;

use fleximark_model::{Document, DocumentUri, NavigationEntry, PositionEncoding};
use fleximark_plugin_api::{CancellationToken, PluginDiagnostic, PluginExecutor};

use crate::assets::{AssetDiagnostic, RenderConfig};
use crate::error::EngineError;
use crate::identity::content_hash;
use crate::identity::reconcile_node_ids;
use crate::provenance::remap_document_provenance;
use fleximark_parser::{ParseError, parse};
use fleximark_plugin_api::{PluginRun, PreprocessedSource};

#[derive(Clone)]
pub struct DocumentEngine {
    pub(super) source: String,
    pub(super) position_encoding: PositionEncoding,
    pub(super) document: Document,
    pub(super) out_of_sync: bool,
    pub(super) render_config: RenderConfig,
    pub(super) plugins: Option<Arc<dyn PluginExecutor>>,
    pub(super) plugin_diagnostics: Vec<PluginDiagnostic>,
}

impl DocumentEngine {
    pub fn clear_configuration(&mut self) -> Result<(), EngineError> {
        self.plugins = None;
        self.render_config = RenderConfig::default();
        self.plugin_diagnostics.clear();
        let result = self.install_current_source(
            self.document.document_version,
            self.source.clone(),
            &CancellationToken::default(),
        );
        if result.is_err() {
            self.mark_out_of_sync();
        }
        result
    }

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
        plugins: Arc<dyn PluginExecutor>,
        cancellation: &CancellationToken,
    ) -> Result<Self, EngineError> {
        render_config.validate_for(plugins.as_ref())?;
        let candidate = prepare_source_candidate(SourceCandidateInput {
            uri,
            previous: None,
            version,
            source,
            hooks: CandidateHooks::Plugins {
                host: plugins.as_ref(),
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
                (entry.source_range.byte_start <= byte_offset
                    && byte_offset < entry.source_range.byte_end)
                    || (entry.source_range.byte_start == byte_offset
                        && entry.source_range.byte_end == byte_offset)
            })
            .min_by(|left, right| {
                let left_span = left.source_range.byte_end - left.source_range.byte_start;
                let right_span = right.source_range.byte_end - right.source_range.byte_start;
                left_span
                    .cmp(&right_span)
                    .then_with(|| right.depth.cmp(&left.depth))
                    .then_with(|| left.node_id.0.cmp(&right.node_id.0))
            })
    }

    pub fn reconfigure(
        &mut self,
        render_config: RenderConfig,
        plugins: Arc<dyn PluginExecutor>,
        cancellation: &CancellationToken,
    ) -> Result<(), EngineError> {
        render_config.validate_for(plugins.as_ref())?;
        let diagnostics = self.install_source(
            self.document.document_version,
            self.source.clone(),
            CandidateHooks::Plugins {
                host: plugins.as_ref(),
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
        render_config.validate_for(plugins.as_ref())?;
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

#[derive(Clone, Copy)]
pub(super) enum CandidateHooks<'a> {
    Empty,
    Plugins {
        host: &'a dyn PluginExecutor,
        cancellation: &'a CancellationToken,
    },
}

pub(super) struct SourceCandidateInput<'a> {
    pub(super) uri: DocumentUri,
    pub(super) previous: Option<&'a Document>,
    pub(super) version: u64,
    pub(super) source: String,
    pub(super) hooks: CandidateHooks<'a>,
}

pub(super) struct SourceCandidate {
    source: String,
    document: Document,
    diagnostics: Vec<PluginDiagnostic>,
}

impl CandidateHooks<'_> {
    fn preprocess(
        self,
        version: u64,
        source: &str,
    ) -> Result<PluginRun<PreprocessedSource>, EngineError> {
        match self {
            Self::Empty => Ok(PluginRun {
                value: PreprocessedSource {
                    text: source.to_owned(),
                    segments: Vec::new(),
                },
                diagnostics: Vec::new(),
            }),
            Self::Plugins { host, cancellation } => host
                .preprocess_source(version, source, cancellation)
                .map_err(|error| EngineError::Plugin(error.to_string())),
        }
    }

    fn transform(
        self,
        source: &str,
        document: Document,
    ) -> Result<PluginRun<Document>, EngineError> {
        let Self::Plugins { host, cancellation } = self else {
            return Ok(PluginRun {
                value: document,
                diagnostics: Vec::new(),
            });
        };
        let block_run = host
            .transform_blocks(source, &document, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        let mut document_run = host
            .transform_document(source, &block_run.value, cancellation)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        document_run
            .value
            .validate(source)
            .map_err(|error| EngineError::Plugin(error.to_string()))?;
        document_run.diagnostics.splice(0..0, block_run.diagnostics);
        Ok(document_run)
    }
}

pub(super) fn prepare_source_candidate(
    input: SourceCandidateInput<'_>,
) -> Result<SourceCandidate, EngineError> {
    let preprocessed = input.hooks.preprocess(input.version, &input.source)?;
    let mut document = parse(input.uri, input.version, &preprocessed.value.text)?;
    if !preprocessed.value.segments.is_empty() {
        remap_document_provenance(&mut document, &preprocessed.value, &input.source)?;
    }
    reconcile_node_ids(input.previous, &mut document);
    document.validate(&input.source).map_err(ParseError::from)?;
    let transformed = input.hooks.transform(&input.source, document)?;
    let mut diagnostics = preprocessed.diagnostics;
    diagnostics.extend(transformed.diagnostics);
    Ok(SourceCandidate {
        source: input.source,
        document: transformed.value,
        diagnostics,
    })
}

impl DocumentEngine {
    pub(super) fn from_candidate(
        candidate: SourceCandidate,
        position_encoding: PositionEncoding,
        render_config: RenderConfig,
        plugins: Option<Arc<dyn PluginExecutor>>,
    ) -> Self {
        Self {
            source: candidate.source,
            position_encoding,
            document: candidate.document,
            out_of_sync: false,
            render_config,
            plugins,
            plugin_diagnostics: candidate.diagnostics,
        }
    }

    pub(super) fn install_source(
        &mut self,
        version: u64,
        source: String,
        hooks: CandidateHooks<'_>,
    ) -> Result<Vec<PluginDiagnostic>, EngineError> {
        let candidate = prepare_source_candidate(SourceCandidateInput {
            uri: self.document.uri.clone(),
            previous: Some(&self.document),
            version,
            source,
            hooks,
        })?;
        self.source = candidate.source;
        self.document = candidate.document;
        Ok(candidate.diagnostics)
    }
}
