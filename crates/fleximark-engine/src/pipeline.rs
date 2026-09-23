use std::collections::HashMap;
use std::sync::Arc;

use fleximark_model::{Document, DocumentUri, PositionEncoding};
use fleximark_parser::{ParseError, parse};
use fleximark_plugin_host::{CancellationToken, PluginDiagnostic, PluginHost, PluginRun};
use fleximark_plugin_sdk::PreprocessedSource;

use crate::assets::RenderConfig;
use crate::error::EngineError;
use crate::identity::reconcile_node_ids;
use crate::provenance::remap_document_provenance;
use crate::session::{DocumentSession, new_session_id};

#[derive(Clone, Copy)]
pub(super) enum CandidateHooks<'a> {
    Empty,
    Plugins {
        host: &'a PluginHost,
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

impl DocumentSession {
    pub(super) fn from_candidate(
        candidate: SourceCandidate,
        position_encoding: PositionEncoding,
        render_config: RenderConfig,
        plugins: Option<Arc<PluginHost>>,
    ) -> Self {
        Self {
            id: new_session_id(&candidate.document.uri),
            source: candidate.source,
            position_encoding,
            document: candidate.document,
            out_of_sync: false,
            render_config,
            plugins,
            plugin_diagnostics: candidate.diagnostics,
            previews: HashMap::new(),
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
