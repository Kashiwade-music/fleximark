use std::collections::HashMap;
use std::sync::Arc;

use fleximark_model::{Document, DocumentUri, PositionEncoding};
use fleximark_parser::{ParseError, parse};
use fleximark_plugin_host::{CancellationToken, PluginDiagnostic, PluginHost};

use crate::assets::RenderConfig;
use crate::error::EngineError;
use crate::identity::reconcile_node_ids;
use crate::provenance::remap_document_provenance;
use crate::session::{DocumentSession, new_session_id};

struct PipelineCandidate {
    document: Document,
    diagnostics: Vec<PluginDiagnostic>,
}

impl DocumentSession {
    pub(super) fn open_with_plugins(
        uri: DocumentUri,
        version: u64,
        source: String,
        position_encoding: PositionEncoding,
        host: Arc<PluginHost>,
        cancellation: &CancellationToken,
    ) -> Result<(Self, Vec<PluginDiagnostic>), EngineError> {
        let (candidate, id) = prepare_plugin_candidate(
            uri,
            None,
            version,
            &source,
            &host,
            cancellation,
            |document| new_session_id(&document.uri),
        )?;
        let session = Self {
            id,
            source,
            position_encoding,
            document: candidate.document,
            out_of_sync: false,
            render_config: RenderConfig::default(),
            plugins: Some(Arc::clone(&host)),
            plugin_diagnostics: candidate.diagnostics.clone(),
            previews: HashMap::new(),
        };
        Ok((session, candidate.diagnostics))
    }

    pub(super) fn install_source(
        &mut self,
        version: u64,
        source: String,
    ) -> Result<(), EngineError> {
        let mut document = parse(self.document.uri.clone(), version, &source)?;
        reconcile_node_ids(Some(&self.document), &mut document);
        document.validate(&source).map_err(ParseError::from)?;
        self.install_source_candidate(source, document);
        Ok(())
    }

    pub(super) fn install_source_with_plugins(
        &mut self,
        version: u64,
        source: String,
        host: &PluginHost,
        cancellation: &CancellationToken,
    ) -> Result<Vec<PluginDiagnostic>, EngineError> {
        let (candidate, ()) = prepare_plugin_candidate(
            self.document.uri.clone(),
            Some(&self.document),
            version,
            &source,
            host,
            cancellation,
            |_| (),
        )?;
        self.install_source_candidate(source, candidate.document);
        Ok(candidate.diagnostics)
    }

    fn install_source_candidate(&mut self, source: String, document: Document) {
        self.source = source;
        self.document = document;
    }
}

fn prepare_plugin_candidate<T>(
    uri: DocumentUri,
    previous: Option<&Document>,
    version: u64,
    source: &str,
    host: &PluginHost,
    cancellation: &CancellationToken,
    after_base_validation: impl FnOnce(&Document) -> T,
) -> Result<(PipelineCandidate, T), EngineError> {
    let preprocessed = host
        .preprocess_source(version, source, cancellation)
        .map_err(|error| EngineError::Plugin(error.to_string()))?;
    let mut document = parse(uri, version, &preprocessed.value.text)?;
    if !preprocessed.value.segments.is_empty() {
        remap_document_provenance(&mut document, &preprocessed.value, source)?;
    }
    reconcile_node_ids(previous, &mut document);
    document.validate(source).map_err(ParseError::from)?;
    let validated_base = after_base_validation(&document);
    let block_run = host
        .transform_blocks(source, &document, cancellation)
        .map_err(|error| EngineError::Plugin(error.to_string()))?;
    let document_run = host
        .transform_document(source, &block_run.value, cancellation)
        .map_err(|error| EngineError::Plugin(error.to_string()))?;
    document_run
        .value
        .validate(source)
        .map_err(|error| EngineError::Plugin(error.to_string()))?;
    let mut diagnostics = preprocessed.diagnostics;
    diagnostics.extend(block_run.diagnostics);
    diagnostics.extend(document_run.diagnostics);
    Ok((
        PipelineCandidate {
            document: document_run.value,
            diagnostics,
        },
        validated_base,
    ))
}
