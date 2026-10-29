//! Role: Open CLI documents and resolve assets with the shared native policy.

use fleximark_engine::DocumentEngine;
#[cfg(test)]
use fleximark_model::{DocumentUri, PositionEncoding};

use super::{ApplicationError, resolve_render_assets};
#[cfg(test)]
use super::{workspace_for_document, workspace_path};

#[cfg(test)]
pub fn open_document(
    uri: String,
    source: String,
    trusted_workspace: bool,
) -> Result<DocumentEngine, ApplicationError> {
    let workspace = if trusted_workspace {
        match workspace_for_document(&uri) {
            Ok(workspace) => Some(workspace),
            Err(ApplicationError::NotInitialized) => None,
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
            Default::default(),
        )?);
    };
    let config = super::configured_render_config(&workspace, true, 1)?;
    let mut engine =
        DocumentEngine::open(DocumentUri(uri), 1, source, PositionEncoding::Utf8, config)?;
    let config = super::workspace::validate_config(
        &workspace_path(&workspace)?.join(".fleximark/config.toml"),
    )?;
    refresh_document_assets(&mut engine, &workspace, &config)?;
    Ok(engine)
}

pub fn refresh_document_assets(
    engine: &mut DocumentEngine,
    workspace: &str,
    config: &fleximark_config::FlexiMarkConfig,
) -> Result<(), ApplicationError> {
    let assets = resolve_render_assets(
        &engine.document().uri.0,
        workspace,
        engine.document(),
        config,
    )?;
    let config = engine
        .render_config()
        .clone()
        .with_resolved_assets(assets)?;
    Ok(engine.set_render_config(config)?)
}
