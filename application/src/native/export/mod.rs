//! Role: Native workspace mod operations and validation.
mod commit;
mod filesystem;
mod journal;
mod model;
mod ownership;
mod recovery;
mod transaction;
mod validation;
use transaction::export_html_transaction;
use validation::revalidate_export_paths;

use crate::CancellationToken;
use crate::outcomes::ExportReceipt;
use fleximark_engine::DocumentEngine;
use fleximark_render_html::{HtmlTarget, RenderContext};

use crate::native::ApplicationError;
use crate::native::assets::{ExportAsset, compose_portable_html, resolve_export_assets};
use crate::native::integrity::sha256;
use crate::native::uri::{file_uri_path, path_to_file_uri_unchecked, workspace_path};
use ownership::inspect_export;
use recovery::recover_export;

pub fn default_export_destination(document_uri: &str) -> Result<String, ApplicationError> {
    let source = workspace_path(document_uri)?;
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or(ApplicationError::InvalidExportDestination)?;
    let destination = source
        .parent()
        .ok_or(ApplicationError::InvalidExportDestination)?
        .join(format!("{stem}.fleximark-export"));
    path_to_file_uri_unchecked(&destination)
}

pub(crate) fn export_render_context(
    workspace_uri: &str,
) -> Result<RenderContext, ApplicationError> {
    workspace_path(workspace_uri)?;
    Ok(RenderContext {
        target: HtmlTarget::Portable,
        allow_remote_resources: false,
        allow_data_resources: false,
        resolved_resources: Default::default(),
    })
}

pub fn export_document(
    session: &DocumentEngine,
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    cancellation: &CancellationToken,
    resources: Option<&fleximark_preview_assets::PreviewAssets>,
) -> Result<ExportReceipt, ApplicationError> {
    crate::cancellation_checks::check(cancellation, "export_start")
        .map_err(|_| ApplicationError::Cancelled)?;
    preflight_export(source_uri, workspace_uri, destination_uri)?;
    let mut context = export_render_context(workspace_uri)?;
    context.resolved_resources = session.render_config().context.resolved_resources.clone();
    crate::cancellation_checks::check(cancellation, "export_render_start")
        .map_err(|_| ApplicationError::Cancelled)?;
    let rendered = session.render(&context)?;
    crate::cancellation_checks::check(cancellation, "export_rendered")
        .map_err(|_| ApplicationError::Cancelled)?;
    let mut assets = Vec::new();
    let resolved = resolve_export_assets(
        source_uri,
        workspace_uri,
        &rendered.html(),
        &rendered.assets,
    )?;
    crate::cancellation_checks::check(cancellation, "export_assets")
        .map_err(|_| ApplicationError::Cancelled)?;
    assets.extend(resolved.assets);
    let resources = resources.ok_or_else(|| {
        ApplicationError::InternalState("runtime resources are not configured".into())
    })?;
    let html = compose_portable_html(
        &resolved.html,
        rendered.style.as_ref(),
        &resources.read("browser-host.js")?,
        resources,
    )?;
    crate::cancellation_checks::check(cancellation, "export_commit")
        .map_err(|_| ApplicationError::Cancelled)?;
    export_html(source_uri, workspace_uri, destination_uri, &html, &assets)
}

pub fn export_html(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    html: &str,
    assets: &[ExportAsset],
) -> Result<ExportReceipt, ApplicationError> {
    export_html_transaction(
        source_uri,
        workspace_uri,
        destination_uri,
        html,
        assets,
        |_| Ok(()),
    )
}

pub fn preflight_export(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
) -> Result<(), ApplicationError> {
    let workspace = workspace_path(workspace_uri)?;
    let source = workspace_path(source_uri)?;
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ApplicationError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ApplicationError::InvalidExportDestination)?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ApplicationError::InvalidExportDestination)?,
    );
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    let registry_key = sha256(destination.to_string_lossy().as_bytes());
    let registry = workspace
        .join(".fleximark/export-targets")
        .join(format!("{registry_key}.json"));
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ApplicationError::InvalidExportDestination)?;
    let journal = parent.join(format!(".{name}.fleximark-export-journal.json"));
    recover_export(&journal, &destination, &registry, false)?;
    let existing = inspect_export(&destination, &registry)?;
    if existing.as_ref().is_some_and(|managed| {
        managed.registry.source_identity != source.to_string_lossy()
            || managed.registry.workspace_identity != workspace.to_string_lossy()
            || managed.registry.destination_identity != destination.to_string_lossy()
    }) {
        return Err(ApplicationError::ExportOwnershipConflict);
    }
    Ok(())
}

pub fn acknowledge_export(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
) -> Result<(), ApplicationError> {
    let workspace = workspace_path(workspace_uri)?;
    let source = workspace_path(source_uri)?;
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ApplicationError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ApplicationError::InvalidExportDestination)?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ApplicationError::InvalidExportDestination)?,
    );
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    let registry_key = sha256(destination.to_string_lossy().as_bytes());
    let registry = workspace
        .join(".fleximark/export-targets")
        .join(format!("{registry_key}.json"));
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ApplicationError::InvalidExportDestination)?;
    let journal = parent.join(format!(".{name}.fleximark-export-journal.json"));
    if !journal.exists() {
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    recover_export(&journal, &destination, &registry, true)
}

#[cfg(test)]
mod tests;
