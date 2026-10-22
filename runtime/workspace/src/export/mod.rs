//! Role: Native workspace mod operations and validation.
mod commit;
mod filesystem;
mod journal;
mod model;
mod ownership;
mod recovery;

use std::fs;
use std::io::{Read, Seek, Write};
use std::path::Path;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions as CapOpenOptions;
use cap_std::{ambient_authority, fs::Dir};
use fleximark_application::operations::ExportReceipt;
use fleximark_engine::DocumentEngine;
use fleximark_plugin_host::CancellationToken;
use fleximark_render_html::{HtmlTarget, RenderContext};

use crate::ServiceError;
use crate::assets::{ExportAsset, compose_portable_html, resolve_export_assets};
use crate::uri::{file_uri_path, path_to_file_uri, path_to_file_uri_unchecked, workspace_path};
use crate::workspace::{reject_link, validate_config};
use commit::commit_installed_export;
pub(crate) use filesystem::sha256;
use filesystem::{
    index_for_destination, object_identity, object_identity_from_metadata, payload_digest,
    random_id, record_tree, reserved_export_path, sync_directory, verify_directory_tree,
    write_json, write_synced,
};
use journal::{append_journal, create_journal};
use model::{
    ExportFile, ExportJournal, ExportJournalState, ManagedExport, OwnershipMarker,
    OwnershipPayload, RegistryRecord,
};
use ownership::inspect_export;
use recovery::recover_export;

pub fn default_export_destination(document_uri: &str) -> Result<String, ServiceError> {
    let source = workspace_path(document_uri)?;
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::InvalidExportDestination)?;
    let destination = source
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?
        .join(format!("{stem}.fleximark-export"));
    path_to_file_uri_unchecked(&destination)
}

pub(crate) fn export_render_context(workspace_uri: &str) -> Result<RenderContext, ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    validate_config(&workspace.join(".fleximark/config.toml"))?;
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
) -> Result<ExportReceipt, ServiceError> {
    preflight_export(source_uri, workspace_uri, destination_uri)?;
    let mut context = export_render_context(workspace_uri)?;
    context.resolved_resources = session.render_config().context.resolved_resources.clone();
    let prepared = session.prepare_safe_export(&context)?;
    let mut assets = Vec::new();
    let resolved = prepared.compose_portable(
        &fleximark_preview_assets::read("browser-host.js")?,
        |safe_html, style, render_assets, runtime| {
            let resolved =
                resolve_export_assets(source_uri, workspace_uri, safe_html, render_assets)?;
            assets = resolved.assets;
            compose_portable_html(&resolved.html, style, runtime)
        },
    )?;
    let output = session
        .apply_unsafe_export_html(resolved, cancellation)?
        .value;
    export_html_with_safety(
        source_uri,
        workspace_uri,
        destination_uri,
        &output.html,
        &assets,
        output.unsafe_output_used,
    )
}

pub fn export_html_with_safety(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    html: &str,
    assets: &[ExportAsset],
    unsafe_output_used: bool,
) -> Result<ExportReceipt, ServiceError> {
    let marked_html = if unsafe_output_used {
        format!(
            "<!-- fleximark-output-safety: unsafe-plugin-html --><aside role=\"alert\" data-fleximark-output-safety=\"unsafe-plugin-html\">This export contains HTML produced by an explicitly granted unsafe plugin.</aside>{html}"
        )
    } else {
        html.to_owned()
    };
    export_html_transaction(
        source_uri,
        workspace_uri,
        destination_uri,
        &marked_html,
        assets,
        unsafe_output_used,
        |_| Ok(()),
    )
}

fn verify_export_assets(assets: &[ExportAsset]) -> Result<(), ServiceError> {
    for asset in assets {
        let (Some(source), Some(source_identity)) = (&asset.source, &asset.source_identity) else {
            continue;
        };
        if object_identity(source).ok().as_ref() != Some(source_identity) {
            return Err(ServiceError::ExportContentConflict);
        }
        let parent = Dir::open_ambient_dir(
            source
                .parent()
                .ok_or(ServiceError::InvalidExportDestination)?,
            ambient_authority(),
        )?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = parent.open_with(
            source
                .file_name()
                .ok_or(ServiceError::InvalidExportDestination)?,
            &options,
        )?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes != asset.bytes || sha256(&bytes) != asset.content_hash {
            return Err(ServiceError::ExportContentConflict);
        }
    }
    Ok(())
}

fn export_html_transaction(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    html: &str,
    assets: &[ExportAsset],
    unsafe_output_used: bool,
    mut checkpoint: impl FnMut(&'static str) -> Result<(), ServiceError>,
) -> Result<ExportReceipt, ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    validate_config(&workspace.join(".fleximark/config.toml"))?;
    let source = workspace_path(source_uri)?;
    if source == workspace || !source.starts_with(&workspace) {
        return Err(ServiceError::InvalidExportDestination);
    }
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ServiceError::InvalidExportDestination)?;
    let parent_dir = Dir::open_ambient_dir(&parent, ambient_authority())?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ServiceError::InvalidExportDestination)?,
    );
    if destination == workspace
        || !destination.starts_with(&workspace)
        || destination.starts_with(workspace.join(".fleximark"))
    {
        return Err(ServiceError::ReservedExportPath);
    }
    if fs::symlink_metadata(&destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ServiceError::InvalidExportDestination);
    }

    let destination_identity = destination.to_string_lossy().into_owned();
    let workspace_identity = workspace.to_string_lossy().into_owned();
    let source_identity = source.to_string_lossy().into_owned();
    let registry_key = sha256(destination_identity.as_bytes());
    let registry = workspace
        .join(".fleximark/export-targets")
        .join(format!("{registry_key}.json"));
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::InvalidExportDestination)?;
    let journal_path = parent.join(format!(".{name}.fleximark-export-journal.json"));
    recover_export(&journal_path, &destination, &registry, false)?;
    reject_link(
        registry
            .parent()
            .ok_or(ServiceError::InvalidExportDestination)?,
    )?;

    let previous = inspect_export(&destination, &registry)?;
    if previous.as_ref().is_some_and(|managed| {
        managed.registry.source_identity != source_identity
            || managed.registry.workspace_identity != workspace_identity
            || managed.registry.destination_identity != destination_identity
    }) {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    let generation = previous
        .as_ref()
        .map_or(1, |managed| managed.registry.generation + 1);
    let destination_id = previous
        .as_ref()
        .map(|managed| managed.registry.destination_id.clone())
        .map(Ok)
        .unwrap_or_else(random_id)?;
    let transaction = random_id()?;
    let staging = parent.join(format!(".{name}.fleximark-export-staging-{transaction}"));
    let backup = parent.join(format!(".{name}.fleximark-export-backup-{transaction}"));
    if staging.exists() || backup.exists() {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let previous_destination_identity = fs::symlink_metadata(&destination)
        .ok()
        .map(|_| object_identity(&destination))
        .transpose()?;
    let previous_destination_objects = if destination.exists() {
        record_tree(&destination)?
    } else {
        Vec::new()
    };
    let previous_registry_identity = fs::symlink_metadata(&registry)
        .ok()
        .map(|_| object_identity(&registry))
        .transpose()?;
    let destination_name = destination
        .file_name()
        .ok_or(ServiceError::InvalidExportDestination)?
        .to_os_string();
    let staging_name = staging
        .file_name()
        .ok_or(ServiceError::InvalidExportDestination)?
        .to_os_string();
    let backup_name = backup
        .file_name()
        .ok_or(ServiceError::InvalidExportDestination)?
        .to_os_string();
    parent_dir.create_dir(&staging_name)?;
    let stage_result = (|| {
        if let Some(managed) = previous.as_ref() {
            copy_user_files(&destination, &staging, managed)?;
        }
        write_synced(&staging.join("index.html"), html.as_bytes(), true)?;
        for asset in assets {
            let target = staging.join(&asset.path);
            fs::create_dir_all(target.parent().ok_or(ServiceError::ReservedExportPath)?)?;
            if sha256(&asset.bytes) != asset.content_hash {
                return Err(ServiceError::ExportContentConflict);
            }
            write_synced(&target, &asset.bytes, true)?;
        }
        sync_directory(&staging)
    })();
    if let Err(error) = stage_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let destination_object_identity = object_identity(&staging)?;
    let mut files = vec![ExportFile {
        path: "index.html".into(),
        kind: "file".into(),
        content_hash: sha256(html.as_bytes()),
        object_identity: object_identity(&staging.join("index.html"))?,
    }];
    if !assets.is_empty() {
        files.push(ExportFile {
            path: "assets".into(),
            kind: "directory".into(),
            content_hash: String::new(),
            object_identity: object_identity(&staging.join("assets"))?,
        });
    }
    files.extend(
        assets
            .iter()
            .map(|asset| {
                Ok(ExportFile {
                    path: asset.path.clone(),
                    kind: "file".into(),
                    content_hash: asset.content_hash.clone(),
                    object_identity: object_identity(&staging.join(&asset.path))?,
                })
            })
            .collect::<Result<Vec<_>, ServiceError>>()?,
    );
    let payload = OwnershipPayload {
        format_version: 1,
        destination_id: destination_id.clone(),
        generation,
        source_identity: source_identity.clone(),
        workspace_identity: workspace_identity.clone(),
        destination_identity: destination_identity.clone(),
        destination_object_identity: destination_object_identity.clone(),
        unsafe_output_used,
        files,
    };
    let marker = OwnershipMarker {
        digest: payload_digest(&payload)?,
        payload,
    };
    if let Err(error) = write_json(&staging.join(".fleximark-export.json"), &marker, true)
        .and_then(|_| sync_directory(&staging))
    {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let next_registry = RegistryRecord {
        destination_id,
        generation,
        digest: marker.digest.clone(),
        source_identity,
        workspace_identity,
        destination_identity,
        destination_object_identity,
        unsafe_output_used,
    };
    let staging_identity = object_identity(&staging)?;
    let staging_marker_identity = object_identity(&staging.join(".fleximark-export.json"))?;
    let generated_identities = record_tree(&staging)?;
    let journal_identity = create_journal(&journal_path)?;
    let mut journal = ExportJournal {
        transaction_id: transaction,
        state: ExportJournalState::Prepared,
        destination: destination.clone(),
        staging,
        backup,
        registry,
        journal_identity,
        registry_staging: None,
        registry_staging_identity: None,
        previous_registry: previous.map(|managed| managed.registry),
        next_registry,
        staging_identity,
        staging_marker_identity,
        generated_identities,
        previous_destination_objects,
        previous_destination_identity,
        previous_registry_identity,
        backup_identity: None,
        installed_identity: None,
        installed_marker_identity: None,
        next_registry_identity: None,
    };
    let mut journal_digest = append_journal(&journal_path, &journal, None)?;
    sync_directory(&parent)?;
    verify_export_assets(assets)?;
    if journal.previous_destination_identity.is_some() {
        verify_directory_tree(&destination, &journal.previous_destination_objects)?;
    }
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    journal.state = ExportJournalState::OldMoveIntent;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    checkpoint("old-move-intent")?;
    if destination.exists() {
        parent_dir.rename(&destination_name, &parent_dir, &backup_name)?;
        let backup_identity = object_identity(&journal.backup)?;
        if Some(&backup_identity) != journal.previous_destination_identity.as_ref() {
            return Err(ServiceError::ExportRecoveryConflict);
        }
        journal.backup_identity = Some(backup_identity);
        sync_directory(&parent)?;
    }
    checkpoint("old-moved")?;
    journal.state = ExportJournalState::OldMoved;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    journal.state = ExportJournalState::InstallIntent;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    checkpoint("install-intent")?;
    parent_dir.rename(&staging_name, &parent_dir, &destination_name)?;
    journal.installed_identity = Some(object_identity(&destination)?);
    journal.installed_marker_identity = Some(object_identity(
        &destination.join(".fleximark-export.json"),
    )?);
    if journal.installed_identity.as_ref() != Some(&journal.staging_identity)
        || journal.installed_marker_identity.as_ref() != Some(&journal.staging_marker_identity)
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    checkpoint("installed")?;
    journal.state = ExportJournalState::NewInstalled;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    commit_installed_export(&mut journal, &journal_path, journal_digest, &mut checkpoint)?;
    Ok(ExportReceipt {
        output_uri: path_to_file_uri(&index_for_destination(&destination))?,
        generation,
        unsafe_output: unsafe_output_used,
    })
}

pub fn preflight_export(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
) -> Result<(), ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    validate_config(&workspace.join(".fleximark/config.toml"))?;
    let source = workspace_path(source_uri)?;
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ServiceError::InvalidExportDestination)?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ServiceError::InvalidExportDestination)?,
    );
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    let registry_key = sha256(destination.to_string_lossy().as_bytes());
    let registry = workspace
        .join(".fleximark/export-targets")
        .join(format!("{registry_key}.json"));
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::InvalidExportDestination)?;
    let journal = parent.join(format!(".{name}.fleximark-export-journal.json"));
    recover_export(&journal, &destination, &registry, false)?;
    let existing = inspect_export(&destination, &registry)?;
    if existing.as_ref().is_some_and(|managed| {
        managed.registry.source_identity != source.to_string_lossy()
            || managed.registry.workspace_identity != workspace.to_string_lossy()
            || managed.registry.destination_identity != destination.to_string_lossy()
    }) {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    Ok(())
}

pub fn acknowledge_export(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
) -> Result<(), ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    let source = workspace_path(source_uri)?;
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ServiceError::InvalidExportDestination)?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ServiceError::InvalidExportDestination)?,
    );
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    let registry_key = sha256(destination.to_string_lossy().as_bytes());
    let registry = workspace
        .join(".fleximark/export-targets")
        .join(format!("{registry_key}.json"));
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::InvalidExportDestination)?;
    let journal = parent.join(format!(".{name}.fleximark-export-journal.json"));
    if !journal.exists() {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    recover_export(&journal, &destination, &registry, true)
}

fn revalidate_export_paths(
    workspace: &Path,
    source: &Path,
    destination: &Path,
    expected_parent: &Path,
) -> Result<(), ServiceError> {
    let parent = destination
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?;
    if parent.canonicalize()? != expected_parent
        || !expected_parent.starts_with(workspace)
        || destination == workspace
        || destination.starts_with(workspace.join(".fleximark"))
    {
        return Err(ServiceError::InvalidExportDestination);
    }
    let mut current = workspace.to_path_buf();
    for component in expected_parent
        .strip_prefix(workspace)
        .map_err(|_| ServiceError::InvalidExportDestination)?
        .components()
    {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ServiceError::InvalidExportDestination);
        }
    }
    let source_metadata = fs::symlink_metadata(source)?;
    if !source_metadata.is_file() || source_metadata.file_type().is_symlink() {
        return Err(ServiceError::InvalidExportDestination);
    }
    if fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ServiceError::InvalidExportDestination);
    }
    Ok(())
}

fn copy_user_files(
    source: &Path,
    destination: &Path,
    previous: &ManagedExport,
) -> Result<(), ServiceError> {
    let owned = previous
        .marker
        .payload
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut pending = vec![(
        Dir::open_ambient_dir(source, ambient_authority())?,
        Dir::open_ambient_dir(destination, ambient_authority())?,
        String::new(),
    )];
    while let Some((from, to, prefix)) = pending.pop() {
        for entry in from.entries()? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ServiceError::ReservedExportPath)?;
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if relative == ".fleximark-export.json" {
                continue;
            }
            if reserved_export_path(&relative) {
                return Err(ServiceError::ReservedExportPath);
            }
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(ServiceError::ExportContentConflict);
            }
            if file_type.is_dir() {
                to.create_dir(&name)?;
                pending.push((entry.open_dir()?, to.open_dir_nofollow(&name)?, relative));
            } else if file_type.is_file() {
                if owned.contains(relative.as_str()) {
                    continue;
                }
                let mut read_options = CapOpenOptions::new();
                read_options.read(true).follow(FollowSymlinks::No);
                let mut input = entry.open_with(&read_options)?;
                let before = object_identity_from_metadata(&input.metadata()?)?;
                let mut bytes = Vec::new();
                input.read_to_end(&mut bytes)?;
                input.rewind()?;
                let mut repeated = Vec::new();
                input.read_to_end(&mut repeated)?;
                if bytes != repeated || object_identity_from_metadata(&input.metadata()?)? != before
                {
                    return Err(ServiceError::ExportContentConflict);
                }
                let mut write_options = CapOpenOptions::new();
                write_options
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .follow(FollowSymlinks::No);
                let mut output = to.open_with(&name, &write_options)?;
                output.write_all(&bytes)?;
                output.sync_all()?;
                output.rewind()?;
                let mut copied = Vec::new();
                output.read_to_end(&mut copied)?;
                if sha256(&copied) != sha256(&bytes) {
                    return Err(ServiceError::ExportContentConflict);
                }
            } else {
                return Err(ServiceError::ExportContentConflict);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
