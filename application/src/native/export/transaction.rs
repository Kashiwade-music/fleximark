//! Role: Plan, stage and install export transactions without losing recovery state.
use super::{
    commit::commit_installed_export,
    filesystem::{
        index_for_destination, object_identity, payload_digest, random_id, record_tree,
        sync_directory, verify_directory_tree, write_json, write_synced,
    },
    journal::{append_journal, create_journal},
    model::{
        ExportFile, ExportJournal, ExportJournalState, ManagedExport, OwnershipMarker,
        OwnershipPayload, RegistryRecord,
    },
    ownership::inspect_export,
    recovery::recover_export,
    validation::{copy_user_files, revalidate_export_paths, verify_export_assets},
};
use crate::native::{
    assets::ExportAsset,
    integrity::sha256,
    uri::{file_uri_path, path_to_file_uri, workspace_path},
    workspace::reject_link,
};
use crate::{ApplicationError, ExportReceipt};
use cap_std::{ambient_authority, fs::Dir};
use std::fs;
struct DestinationScope {
    workspace: std::path::PathBuf,
    source: std::path::PathBuf,
    destination: std::path::PathBuf,
    parent: std::path::PathBuf,
    parent_dir: Dir,
}
struct ExportPlan {
    workspace: std::path::PathBuf,
    source: std::path::PathBuf,
    destination: std::path::PathBuf,
    parent: std::path::PathBuf,
    parent_dir: Dir,
    source_identity: String,
    workspace_identity: String,
    destination_identity: String,
    registry: std::path::PathBuf,
    journal_path: std::path::PathBuf,
    previous: Option<ManagedExport>,
    generation: u64,
    destination_id: String,
    transaction: String,
    staging: std::path::PathBuf,
    backup: std::path::PathBuf,
    previous_destination_identity: Option<crate::native::integrity::ObjectIdentity>,
    previous_destination_objects: Vec<super::model::RecordedObject>,
    previous_registry_identity: Option<crate::native::integrity::ObjectIdentity>,
    destination_name: std::ffi::OsString,
    staging_name: std::ffi::OsString,
    backup_name: std::ffi::OsString,
}
struct StagedExport {
    next_registry: RegistryRecord,
    staging_identity: crate::native::integrity::ObjectIdentity,
    staging_marker_identity: crate::native::integrity::ObjectIdentity,
    generated_identities: Vec<super::model::RecordedObject>,
}
fn validate_destination(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
) -> Result<DestinationScope, ApplicationError> {
    let workspace = workspace_path(workspace_uri)?;
    let source = workspace_path(source_uri)?;
    if source == workspace || !source.starts_with(&workspace) {
        return Err(ApplicationError::InvalidExportDestination);
    }
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ApplicationError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ApplicationError::InvalidExportDestination)?;
    let parent_dir = Dir::open_ambient_dir(&parent, ambient_authority())?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ApplicationError::InvalidExportDestination)?,
    );
    if destination == workspace
        || !destination.starts_with(&workspace)
        || destination.starts_with(workspace.join(".fleximark"))
    {
        return Err(ApplicationError::ReservedExportPath);
    }
    if fs::symlink_metadata(&destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ApplicationError::InvalidExportDestination);
    }

    Ok(DestinationScope {
        workspace,
        source,
        destination,
        parent,
        parent_dir,
    })
}
fn plan_export(scope: DestinationScope) -> Result<ExportPlan, ApplicationError> {
    let DestinationScope {
        workspace,
        source,
        destination,
        parent,
        parent_dir,
    } = scope;
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
        .ok_or(ApplicationError::InvalidExportDestination)?
        .to_owned();
    let journal_path = parent.join(format!(".{name}.fleximark-export-journal.json"));
    recover_export(&journal_path, &destination, &registry, false)?;
    reject_link(
        registry
            .parent()
            .ok_or(ApplicationError::InvalidExportDestination)?,
    )?;

    let previous = inspect_export(&destination, &registry)?;
    if previous.as_ref().is_some_and(|managed| {
        managed.registry.source_identity != source_identity
            || managed.registry.workspace_identity != workspace_identity
            || managed.registry.destination_identity != destination_identity
    }) {
        return Err(ApplicationError::ExportOwnershipConflict);
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
        return Err(ApplicationError::ExportRecoveryConflict);
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
        .ok_or(ApplicationError::InvalidExportDestination)?
        .to_os_string();
    let staging_name = staging
        .file_name()
        .ok_or(ApplicationError::InvalidExportDestination)?
        .to_os_string();
    let backup_name = backup
        .file_name()
        .ok_or(ApplicationError::InvalidExportDestination)?
        .to_os_string();
    Ok(ExportPlan {
        workspace,
        source,
        destination,
        parent,
        parent_dir,
        source_identity,
        workspace_identity,
        destination_identity,
        registry,
        journal_path,
        previous,
        generation,
        destination_id,
        transaction,
        staging,
        backup,
        previous_destination_identity,
        previous_destination_objects,
        previous_registry_identity,
        destination_name,
        staging_name,
        backup_name,
    })
}
fn stage_export(
    plan: &ExportPlan,
    html: &str,
    assets: &[ExportAsset],
) -> Result<StagedExport, ApplicationError> {
    let previous = plan.previous.as_ref();
    let parent_dir = &plan.parent_dir;
    let staging_name = &plan.staging_name;
    let destination = &plan.destination;
    let staging = &plan.staging;
    let destination_id = plan.destination_id.clone();
    let generation = plan.generation;
    let source_identity = plan.source_identity.clone();
    let workspace_identity = plan.workspace_identity.clone();
    let destination_identity = plan.destination_identity.clone();
    parent_dir.create_dir(staging_name)?;
    let stage_result = (|| {
        if let Some(managed) = previous {
            copy_user_files(destination, staging, managed)?;
        }
        write_synced(&staging.join("index.html"), html.as_bytes(), true)?;
        for asset in assets {
            let target = staging.join(&asset.path);
            fs::create_dir_all(
                target
                    .parent()
                    .ok_or(ApplicationError::ReservedExportPath)?,
            )?;
            if sha256(&asset.bytes) != asset.content_hash {
                return Err(ApplicationError::ExportContentConflict);
            }
            write_synced(&target, &asset.bytes, true)?;
        }
        sync_directory(staging)
    })();
    if let Err(error) = stage_result {
        let _ = fs::remove_dir_all(staging);
        return Err(error);
    }
    let destination_object_identity = object_identity(staging)?;
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
            .collect::<Result<Vec<_>, ApplicationError>>()?,
    );
    let payload = OwnershipPayload {
        format_version: 1,
        destination_id: destination_id.clone(),
        generation,
        source_identity: source_identity.clone(),
        workspace_identity: workspace_identity.clone(),
        destination_identity: destination_identity.clone(),
        destination_object_identity: destination_object_identity.clone(),
        files,
    };
    let marker = OwnershipMarker {
        digest: payload_digest(&payload)?,
        payload,
    };
    if let Err(error) = write_json(&staging.join(".fleximark-export.json"), &marker, true)
        .and_then(|_| sync_directory(staging))
    {
        let _ = fs::remove_dir_all(staging);
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
    };
    let staging_identity = object_identity(staging)?;
    let staging_marker_identity = object_identity(&staging.join(".fleximark-export.json"))?;
    let generated_identities = record_tree(staging)?;
    Ok(StagedExport {
        next_registry,
        staging_identity,
        staging_marker_identity,
        generated_identities,
    })
}
fn install_export(
    plan: ExportPlan,
    staged: StagedExport,
    assets: &[ExportAsset],
    mut checkpoint: impl FnMut(&'static str) -> Result<(), ApplicationError>,
) -> Result<ExportReceipt, ApplicationError> {
    let ExportPlan {
        workspace,
        source,
        destination,
        parent,
        parent_dir,
        registry,
        journal_path,
        previous,
        generation,
        transaction,
        staging,
        backup,
        previous_destination_identity,
        previous_destination_objects,
        previous_registry_identity,
        destination_name,
        staging_name,
        backup_name,
        ..
    } = plan;
    let StagedExport {
        next_registry,
        staging_identity,
        staging_marker_identity,
        generated_identities,
    } = staged;
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
            return Err(ApplicationError::ExportRecoveryConflict);
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
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    checkpoint("installed")?;
    journal.state = ExportJournalState::NewInstalled;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    commit_installed_export(&mut journal, &journal_path, journal_digest, &mut checkpoint)?;
    export_receipt(&destination, generation)
}
fn export_receipt(
    destination: &std::path::Path,
    generation: u64,
) -> Result<ExportReceipt, ApplicationError> {
    Ok(ExportReceipt {
        output_uri: path_to_file_uri(&index_for_destination(destination))?,
        generation,
    })
}
pub(super) fn export_html_transaction(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    html: &str,
    assets: &[ExportAsset],
    checkpoint: impl FnMut(&'static str) -> Result<(), ApplicationError>,
) -> Result<ExportReceipt, ApplicationError> {
    let destination = validate_destination(source_uri, workspace_uri, destination_uri)?;
    let plan = plan_export(destination)?;
    let staged = stage_export(&plan, html, assets)?;
    install_export(plan, staged, assets, checkpoint)
}
