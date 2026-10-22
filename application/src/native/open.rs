//! Role: Open CLI documents and resolve assets with the shared native policy.
use std::sync::Arc;

use fleximark_engine::DocumentEngine;
use fleximark_model::{DocumentUri, PositionEncoding};
use fleximark_plugin_api::CancellationToken;

use super::{ServiceError, resolve_render_assets, workspace_for_document};

pub fn open_document(
    uri: String,
    source: String,
    trusted_workspace: bool,
) -> Result<DocumentEngine, ServiceError> {
    let workspace = if trusted_workspace {
        match workspace_for_document(&uri) {
            Ok(workspace) => Some(workspace),
            Err(ServiceError::NotInitialized) => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let Some(workspace) = workspace else {
        return Ok(DocumentEngine::open(
            DocumentUri(uri),
            1,
            source,
            PositionEncoding::Utf8,
        )?);
    };
    let (host, config) = super::load_plugin_host(&workspace, true, 1)?;
    let mut engine = DocumentEngine::open_configured(
        DocumentUri(uri),
        1,
        source,
        PositionEncoding::Utf8,
        config,
        Arc::new(host),
        &CancellationToken::default(),
    )?;
    refresh_document_assets(&mut engine, &workspace)?;
    Ok(engine)
}

pub fn refresh_document_assets(
    engine: &mut DocumentEngine,
    workspace: &str,
) -> Result<(), ServiceError> {
    let assets = resolve_render_assets(&engine.document().uri.0, workspace, engine.document())?;
    let config = engine
        .render_config()
        .clone()
        .with_resolved_assets(assets)?;
    Ok(engine.reconfigure_render(config)?)
}
