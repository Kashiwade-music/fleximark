mod filesystem;
mod journal;
mod model;
mod recovery;

use std::fs;
use std::io::{Read, Seek, Write};
use std::path::Path;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions as CapOpenOptions;
use cap_std::{ambient_authority, fs::Dir};
use fleximark_engine::DocumentSession;
use fleximark_plugin_host::CancellationToken;
use fleximark_plugin_sdk::RawHtmlRenderPolicy;
use fleximark_protocol::{CommandMessage, CommandResult};
use fleximark_render_html::{HtmlTarget, RawHtmlPolicy, RenderContext};

use crate::ServiceError;
use crate::assets::{ExportAsset, compose_portable_html, resolve_export_assets};
use crate::uri::{file_uri_path, path_to_file_uri, path_to_file_uri_unchecked, workspace_path};
use crate::workspace::{reject_link, validate_config};
pub(crate) use filesystem::sha256;
use filesystem::{
    index_for_destination, object_identity, object_identity_from_metadata, open_relative_parent,
    payload_digest, random_id, read_json_regular, read_regular_at, record_tree,
    reserved_export_path, safe_relative_path, sync_directory, verify_directory_tree, write_json,
    write_synced,
};
use journal::{append_journal, create_journal, replace_staged_json, stage_json};
pub(crate) use model::ObjectIdentity;
use model::{
    ExportFile, ExportJournal, ExportJournalState, ManagedExport, OwnershipMarker,
    OwnershipPayload, RegistryRecord,
};
use recovery::{recover_export, verify_installed};

const EXPORT_CLIENT: &str = include_str!("../../../../web/preview-client/browser-host.js");

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
    let config = validate_config(&workspace.join(".fleximark/config.toml"))?;
    Ok(RenderContext {
        target: HtmlTarget::Portable,
        raw_html: match config.security.raw_html_export {
            RawHtmlRenderPolicy::Sanitize => RawHtmlPolicy::Sanitize,
            RawHtmlRenderPolicy::Escape => RawHtmlPolicy::Escape,
            RawHtmlRenderPolicy::Reject => RawHtmlPolicy::Reject,
        },
        allow_remote_resources: false,
        allow_data_resources: false,
        resolved_resources: Default::default(),
    })
}

pub fn export_document(
    session: &DocumentSession,
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    cancellation: &CancellationToken,
) -> Result<CommandResult, ServiceError> {
    preflight_export(source_uri, workspace_uri, destination_uri)?;
    let mut context = export_render_context(workspace_uri)?;
    context.resolved_resources = session.render_config().context.resolved_resources.clone();
    let prepared = session.prepare_safe_export(&context)?;
    let mut assets = Vec::new();
    let resolved =
        prepared.compose_portable(EXPORT_CLIENT, |safe_html, style, render_assets, runtime| {
            let resolved =
                resolve_export_assets(source_uri, workspace_uri, safe_html, render_assets)?;
            assets = resolved.assets;
            compose_portable_html(&resolved.html, style, runtime)
        })?;
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
) -> Result<CommandResult, ServiceError> {
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
) -> Result<CommandResult, ServiceError> {
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
    Ok(CommandResult {
        message: Some(CommandMessage {
            level: fleximark_protocol::CommandMessageLevel::Info,
            text: format!("Exported generation {generation}"),
        }),
        open_uri: Some(path_to_file_uri(&index_for_destination(&destination))?),
        data: None,
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

fn inspect_export(
    destination: &Path,
    registry_path: &Path,
) -> Result<Option<ManagedExport>, ServiceError> {
    if !destination.exists() {
        return if registry_path.exists() {
            Err(ServiceError::ExportOwnershipConflict)
        } else {
            Ok(None)
        };
    }
    let directory = Dir::open_ambient_dir(destination, ambient_authority())?;
    let destination_identity = object_identity_from_metadata(&directory.metadata(".")?)?;
    if destination_identity.kind != "directory" {
        return Err(ServiceError::InvalidExportDestination);
    }
    if directory
        .symlink_metadata(".fleximark-export.json")
        .is_err()
    {
        return if directory.entries()?.next().is_none() {
            Ok(None)
        } else {
            Err(ServiceError::UnmanagedExport)
        };
    }
    let marker: OwnershipMarker = serde_json::from_slice(&read_regular_at(
        &directory,
        Path::new(".fleximark-export.json"),
    )?)?;
    if marker.payload.format_version != 1 || payload_digest(&marker.payload)? != marker.digest {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    if destination_identity != marker.payload.destination_object_identity {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    let registry: RegistryRecord = read_json_regular(registry_path)?;
    if registry.destination_id != marker.payload.destination_id
        || registry.generation != marker.payload.generation
        || registry.digest != marker.digest
        || registry.source_identity != marker.payload.source_identity
        || registry.workspace_identity != marker.payload.workspace_identity
        || registry.destination_identity != marker.payload.destination_identity
        || registry.destination_object_identity != marker.payload.destination_object_identity
    {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    let mut owned_paths = std::collections::HashSet::new();
    for file in &marker.payload.files {
        if !matches!(file.kind.as_str(), "file" | "directory")
            || !safe_relative_path(&file.path)
            || !owned_paths.insert(file.path.as_str())
        {
            return Err(ServiceError::ExportOwnershipConflict);
        }
        let relative = Path::new(&file.path);
        let (parent, name) = open_relative_parent(&directory, relative)
            .map_err(|_| ServiceError::ExportContentConflict)?;
        let metadata = parent
            .symlink_metadata(&name)
            .map_err(|_| ServiceError::ExportContentConflict)?;
        if metadata.file_type().is_symlink()
            || (file.kind == "file" && !metadata.is_file())
            || (file.kind == "directory" && !metadata.is_dir())
            || object_identity_from_metadata(&metadata)? != file.object_identity
        {
            return Err(ServiceError::ExportContentConflict);
        }
        if file.kind == "file"
            && sha256(&read_regular_at(&directory, relative)?) != file.content_hash
        {
            return Err(ServiceError::ExportContentConflict);
        }
    }
    Ok(Some(ManagedExport { marker, registry }))
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

fn commit_installed_export(
    journal: &mut ExportJournal,
    journal_path: &Path,
    previous_digest: String,
    checkpoint: &mut dyn FnMut(&'static str) -> Result<(), ServiceError>,
) -> Result<(), ServiceError> {
    verify_installed(&journal.destination, &journal.next_registry)?;
    verify_directory_tree(&journal.destination, &journal.generated_identities)?;
    if object_identity(&journal.destination).ok().as_ref() != journal.installed_identity.as_ref()
        || object_identity(&journal.destination.join(".fleximark-export.json"))
            .ok()
            .as_ref()
            != journal.installed_marker_identity.as_ref()
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let registry_parent = journal
        .registry
        .parent()
        .ok_or(ServiceError::ExportRecoveryConflict)?
        .to_owned();
    reject_link(&registry_parent)?;
    fs::create_dir_all(&registry_parent)?;
    if fs::symlink_metadata(&journal.registry).is_ok()
        && object_identity(&journal.registry).ok().as_ref()
            != journal.previous_registry_identity.as_ref()
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let (registry_staging, registry_staging_identity) =
        stage_json(&registry_parent, &journal.next_registry)?;
    journal.registry_staging = Some(registry_staging);
    journal.registry_staging_identity = Some(registry_staging_identity);
    journal.state = ExportJournalState::RegistryIntent;
    let intent_digest = append_journal(journal_path, journal, Some(previous_digest))?;
    sync_directory(&registry_parent)?;
    checkpoint("registry-intent")?;
    install_staged_registry(journal)?;
    sync_directory(&registry_parent)?;
    checkpoint("registry-replaced")?;
    journal.state = ExportJournalState::Committed;
    append_journal(journal_path, journal, Some(intent_digest))?;
    sync_directory(
        journal_path
            .parent()
            .ok_or(ServiceError::ExportRecoveryConflict)?,
    )?;
    checkpoint("committed")?;
    Ok(())
}

fn install_staged_registry(journal: &mut ExportJournal) -> Result<(), ServiceError> {
    let staging = journal
        .registry_staging
        .as_ref()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    let expected = journal
        .registry_staging_identity
        .as_ref()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    if object_identity(staging).ok().as_ref() != Some(expected) {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    replace_staged_json(staging, &journal.registry)?;
    let installed = object_identity(&journal.registry)?;
    if &installed != expected {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    journal.next_registry_identity = Some(installed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::journal::read_journal;
    use super::model::RecordedObject;
    use super::*;
    use crate::assets::resolve_export_assets;
    use crate::test_support::test_workspace;
    use crate::{initialize_workspace, path_to_file_uri};
    use fleximark_engine::ResolvedRenderAsset;

    #[test]
    fn export_resolves_and_manifests_only_contained_assets() {
        let root = test_workspace("export-assets-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        let asset = root.join("image.png");
        fs::write(&source, "![image](image.png)\n").unwrap();
        let png = b"\x89PNG\r\n\x1a\nasset";
        fs::write(&asset, png).unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let render_asset =
            ResolvedRenderAsset::from_validated_bytes("image.png".into(), "image/png".into(), png)
                .unwrap()
                .published()
                .clone();
        let resolved = resolve_export_assets(
            &source_uri,
            &workspace_uri,
            &format!("<img src=\"{}\">", render_asset.reference),
            &[render_asset],
        )
        .unwrap();
        assert_eq!(resolved.assets.len(), 1);
        assert!(resolved.html.contains("src=\"assets/"));
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        preflight_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            &resolved.html,
            &resolved.assets,
            false,
        )
        .unwrap();
        let marker: OwnershipMarker =
            read_json_regular(&destination.join(".fleximark-export.json")).unwrap();
        assert!(
            marker
                .payload
                .files
                .iter()
                .any(|file| file.path == "assets")
        );
        let exported_asset = marker
            .payload
            .files
            .iter()
            .find(|file| file.path.starts_with("assets/") && file.kind == "file")
            .unwrap();
        assert_eq!(
            fs::read(destination.join(&exported_asset.path)).unwrap(),
            png
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn export_requires_ownership_and_preserves_user_files() {
        let root = test_workspace("export-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# hello\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("user.txt"), "keep").unwrap();
        let destination_uri = path_to_file_uri(&destination).unwrap();
        assert!(matches!(
            export_html_with_safety(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                "<p>first</p>",
                &[],
                false,
            ),
            Err(ServiceError::UnmanagedExport)
        ));
        fs::remove_dir(&destination).unwrap_err();
        fs::remove_file(destination.join("user.txt")).unwrap();
        fs::remove_dir(&destination).unwrap();

        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>first</p>",
            &[],
            false,
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        fs::write(destination.join("user.txt"), "keep").unwrap();
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>second</p>",
            &[],
            false,
        )
        .unwrap();
        let journal = root.join(".public.fleximark-export-journal.json");
        assert!(
            journal.exists(),
            "committed journal is retained for verification"
        );
        assert!(fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".public.fleximark-export-backup-")
        }));
        assert!(matches!(
            preflight_export(&source_uri, &workspace_uri, &destination_uri),
            Err(ServiceError::ExportAwaitingAcknowledgement)
        ));
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        assert!(!journal.exists());
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".public.fleximark-export-backup-")
        }));
        assert_eq!(
            fs::read_to_string(destination.join("user.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(destination.join("index.html")).unwrap(),
            "<p>second</p>"
        );
        let marker: OwnershipMarker =
            read_json_regular(&destination.join(".fleximark-export.json")).unwrap();
        assert_eq!(marker.payload.generation, 2);

        fs::write(destination.join("index.html"), "tampered").unwrap();
        assert!(matches!(
            export_html_with_safety(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                "<p>third</p>",
                &[],
                false,
            ),
            Err(ServiceError::ExportContentConflict)
        ));
        assert_eq!(
            fs::read_to_string(destination.join("index.html")).unwrap(),
            "tampered"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_ownership_ids_are_random_and_cannot_be_copied_to_another_destination() {
        let root = test_workspace("export-random-ownership-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# export\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let first = root.join("first");
        let second = root.join("second");
        let first_uri = path_to_file_uri_unchecked(&first).unwrap();
        let second_uri = path_to_file_uri_unchecked(&second).unwrap();

        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &first_uri,
            "<p>first</p>",
            &[],
            false,
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &first_uri).unwrap();
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &second_uri,
            "<p>second</p>",
            &[],
            false,
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &second_uri).unwrap();
        let first_marker: OwnershipMarker =
            read_json_regular(&first.join(".fleximark-export.json")).unwrap();
        let second_marker: OwnershipMarker =
            read_json_regular(&second.join(".fleximark-export.json")).unwrap();
        assert_ne!(
            first_marker.payload.destination_id,
            second_marker.payload.destination_id
        );
        assert_eq!(first_marker.payload.destination_id.len(), 32);

        fs::write(
            second.join(".fleximark-export.json"),
            serde_json::to_vec(&first_marker).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            preflight_export(&source_uri, &workspace_uri, &second_uri),
            Err(ServiceError::ExportOwnershipConflict) | Err(ServiceError::ExportContentConflict)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_export_output_is_marked_in_html_marker_and_registry() {
        let root = test_workspace("unsafe-export-marker");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("note.md");
        fs::write(&source, "# Note\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "PLUGIN OUTPUT WITHOUT AN HTML HEAD",
            &[],
            true,
        )
        .unwrap();
        let html = fs::read_to_string(destination.join("index.html")).unwrap();
        assert!(html.starts_with("<!-- fleximark-output-safety: unsafe-plugin-html -->"));
        assert!(html.contains("role=\"alert\""));
        let marker: OwnershipMarker =
            read_json_regular(&destination.join(".fleximark-export.json")).unwrap();
        assert!(marker.payload.unsafe_output_used);
        let registry_path = fs::read_dir(root.join(".fleximark/export-targets"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let registry: RegistryRecord = read_json_regular(&registry_path).unwrap();
        assert!(registry.unsafe_output_used);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn acknowledged_export_rejects_a_swapped_destination_directory() {
        let root = test_workspace("export-directory-swap-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# export\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>original</p>",
            &[],
            false,
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();

        let original = root.join("original-public");
        fs::rename(&destination, &original).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::copy(original.join("index.html"), destination.join("index.html")).unwrap();
        fs::copy(
            original.join(".fleximark-export.json"),
            destination.join(".fleximark-export.json"),
        )
        .unwrap();
        assert!(matches!(
            preflight_export(&source_uri, &workspace_uri, &destination_uri),
            Err(ServiceError::ExportOwnershipConflict)
        ));
        assert_eq!(
            fs::read_to_string(original.join("index.html")).unwrap(),
            "<p>original</p>"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn old_moved_journal_recovers_the_previous_destination() {
        let root = test_workspace("recovery-test");
        let destination = root.join("public");
        let staging = root.join(".public.fleximark-export-staging-test");
        let backup = root.join(".public.fleximark-export-backup-test");
        let journal_path = root.join(".public.fleximark-export-journal.json");
        fs::create_dir(&staging).unwrap();
        fs::create_dir(&backup).unwrap();
        fs::write(backup.join("old.txt"), "old").unwrap();
        let record = RegistryRecord {
            destination_id: "id".into(),
            generation: 2,
            digest: "digest".into(),
            source_identity: "source".into(),
            workspace_identity: "workspace".into(),
            destination_identity: destination.to_string_lossy().into_owned(),
            destination_object_identity: object_identity(&staging).unwrap(),
            unsafe_output_used: false,
        };
        let staging_identity = object_identity(&staging).unwrap();
        let backup_identity = object_identity(&backup).unwrap();
        let journal_identity = create_journal(&journal_path).unwrap();
        let mut journal = ExportJournal {
            transaction_id: "test".into(),
            state: ExportJournalState::Prepared,
            destination: destination.clone(),
            staging: staging.clone(),
            backup: backup.clone(),
            registry: root.join("registry.json"),
            journal_identity,
            registry_staging: None,
            registry_staging_identity: None,
            previous_registry: None,
            next_registry: record,
            staging_identity,
            staging_marker_identity: ObjectIdentity {
                platform_id: "unused".into(),
                kind: "file".into(),
            },
            generated_identities: Vec::new(),
            previous_destination_objects: vec![RecordedObject {
                relative_path: "old.txt".into(),
                identity: object_identity(&backup.join("old.txt")).unwrap(),
                content_hash: Some(sha256(b"old")),
            }],
            previous_destination_identity: Some(backup_identity.clone()),
            previous_registry_identity: None,
            backup_identity: None,
            installed_identity: None,
            installed_marker_identity: None,
            next_registry_identity: None,
        };
        let digest = append_journal(&journal_path, &journal, None).unwrap();
        journal.state = ExportJournalState::OldMoveIntent;
        let digest = append_journal(&journal_path, &journal, Some(digest)).unwrap();
        journal.state = ExportJournalState::OldMoved;
        journal.backup_identity = Some(backup_identity);
        append_journal(&journal_path, &journal, Some(digest)).unwrap();
        recover_export(
            &journal_path,
            &destination,
            &root.join("registry.json"),
            false,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("old.txt")).unwrap(),
            "old"
        );
        assert!(!staging.exists() && !journal_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_records_are_append_only_and_digest_chained() {
        let root = test_workspace("journal-digest-test");
        let path = root.join("journal.json");
        let registry = RegistryRecord {
            destination_id: "id".into(),
            generation: 1,
            digest: "digest".into(),
            source_identity: "source".into(),
            workspace_identity: "workspace".into(),
            destination_identity: "destination".into(),
            destination_object_identity: ObjectIdentity {
                platform_id: "destination-object".into(),
                kind: "directory".into(),
            },
            unsafe_output_used: false,
        };
        let journal_identity = create_journal(&path).unwrap();
        let mut journal = ExportJournal {
            transaction_id: "transaction".into(),
            state: ExportJournalState::Prepared,
            destination: root.join("output"),
            staging: root.join("staging"),
            backup: root.join("backup"),
            registry: root.join("registry"),
            journal_identity,
            registry_staging: None,
            registry_staging_identity: None,
            previous_registry: None,
            next_registry: registry,
            staging_identity: ObjectIdentity {
                platform_id: "staging".into(),
                kind: "directory".into(),
            },
            staging_marker_identity: ObjectIdentity {
                platform_id: "marker".into(),
                kind: "file".into(),
            },
            generated_identities: Vec::new(),
            previous_destination_objects: Vec::new(),
            previous_destination_identity: None,
            previous_registry_identity: None,
            backup_identity: None,
            installed_identity: None,
            installed_marker_identity: None,
            next_registry_identity: None,
        };
        let first = append_journal(&path, &journal, None).unwrap();
        journal.state = ExportJournalState::OldMoveIntent;
        append_journal(&path, &journal, Some(first)).unwrap();
        assert_eq!(
            read_journal(&path).unwrap().0.state,
            ExportJournalState::OldMoveIntent
        );
        let tampered = fs::read_to_string(&path)
            .unwrap()
            .replacen("transaction", "forged", 1);
        fs::write(&path, tampered).unwrap();
        assert!(matches!(
            read_journal(&path),
            Err(ServiceError::ExportRecoveryConflict)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_refuses_swapped_staging_identity_without_deleting_it() {
        let root = test_workspace("journal-swap-test");
        let staging = root.join(".output.fleximark-export-staging-test");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join(".fleximark-export.json"), "original").unwrap();
        let journal_path = root.join(".output.fleximark-export-journal.json");
        let journal_identity = create_journal(&journal_path).unwrap();
        let journal = ExportJournal {
            transaction_id: "transaction".into(),
            state: ExportJournalState::Prepared,
            destination: root.join("output"),
            staging: staging.clone(),
            backup: root.join(".output.fleximark-export-backup-test"),
            registry: root.join("registry.json"),
            journal_identity,
            registry_staging: None,
            registry_staging_identity: None,
            previous_registry: None,
            next_registry: RegistryRecord {
                destination_id: "id".into(),
                generation: 1,
                digest: "digest".into(),
                source_identity: "source".into(),
                workspace_identity: "workspace".into(),
                destination_identity: "destination".into(),
                destination_object_identity: ObjectIdentity {
                    platform_id: "destination-object".into(),
                    kind: "directory".into(),
                },
                unsafe_output_used: false,
            },
            staging_identity: object_identity(&staging).unwrap(),
            staging_marker_identity: object_identity(&staging.join(".fleximark-export.json"))
                .unwrap(),
            generated_identities: Vec::new(),
            previous_destination_objects: Vec::new(),
            previous_destination_identity: None,
            previous_registry_identity: None,
            backup_identity: None,
            installed_identity: None,
            installed_marker_identity: None,
            next_registry_identity: None,
        };
        append_journal(&journal_path, &journal, None).unwrap();
        fs::remove_dir_all(&staging).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join(".fleximark-export.json"), "replacement").unwrap();
        assert!(matches!(
            recover_export(
                &journal_path,
                &root.join("output"),
                &root.join("registry.json"),
                false
            ),
            Err(ServiceError::ExportRecoveryConflict)
        ));
        assert_eq!(
            fs::read_to_string(staging.join(".fleximark-export.json")).unwrap(),
            "replacement"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_refuses_replaced_journal_identity_without_touching_staging() {
        let root = test_workspace("journal-identity-test");
        let staging = root.join(".output.fleximark-export-staging-test");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join(".fleximark-export.json"), "original").unwrap();
        let journal_path = root.join(".output.fleximark-export-journal.json");
        let journal_identity = create_journal(&journal_path).unwrap();
        let journal = ExportJournal {
            transaction_id: "transaction".into(),
            state: ExportJournalState::Prepared,
            destination: root.join("output"),
            staging: staging.clone(),
            backup: root.join(".output.fleximark-export-backup-test"),
            registry: root.join("registry.json"),
            journal_identity,
            registry_staging: None,
            registry_staging_identity: None,
            previous_registry: None,
            next_registry: RegistryRecord {
                destination_id: "id".into(),
                generation: 1,
                digest: "digest".into(),
                source_identity: "source".into(),
                workspace_identity: "workspace".into(),
                destination_identity: "destination".into(),
                destination_object_identity: ObjectIdentity {
                    platform_id: "destination-object".into(),
                    kind: "directory".into(),
                },
                unsafe_output_used: false,
            },
            staging_identity: object_identity(&staging).unwrap(),
            staging_marker_identity: object_identity(&staging.join(".fleximark-export.json"))
                .unwrap(),
            generated_identities: Vec::new(),
            previous_destination_objects: Vec::new(),
            previous_destination_identity: None,
            previous_registry_identity: None,
            backup_identity: None,
            installed_identity: None,
            installed_marker_identity: None,
            next_registry_identity: None,
        };
        append_journal(&journal_path, &journal, None).unwrap();
        let records = fs::read(&journal_path).unwrap();
        let replacement = root.join("replacement-journal.json");
        fs::write(&replacement, records).unwrap();
        fs::remove_file(&journal_path).unwrap();
        fs::rename(replacement, &journal_path).unwrap();
        assert!(matches!(
            recover_export(
                &journal_path,
                &root.join("output"),
                &root.join("registry.json"),
                false
            ),
            Err(ServiceError::ExportRecoveryConflict)
        ));
        assert_eq!(
            fs::read_to_string(staging.join(".fleximark-export.json")).unwrap(),
            "original"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn representative_export_intents_recover_idempotently_after_an_injected_crash() {
        for fault in ["old-move-intent", "installed", "committed"] {
            let root = test_workspace(&format!("export-crash-{fault}"));
            let workspace_uri = path_to_file_uri(&root).unwrap();
            initialize_workspace(&workspace_uri).unwrap();
            let source = root.join("doc.md");
            fs::write(&source, "# export\n").unwrap();
            let source_uri = path_to_file_uri(&source).unwrap();
            let destination = root.join("public");
            let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
            export_html_with_safety(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                "<p>old</p>",
                &[],
                false,
            )
            .unwrap();
            acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();

            let result = export_html_transaction(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                "<p>new</p>",
                &[],
                false,
                |checkpoint| {
                    if checkpoint == fault {
                        Err(ServiceError::Io(std::io::Error::other("injected crash")))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err(), "fault {fault} was not reached");

            let preflight = preflight_export(&source_uri, &workspace_uri, &destination_uri);
            if matches!(fault, "old-move-intent" | "old-moved" | "install-intent") {
                preflight.unwrap();
                assert_eq!(
                    fs::read_to_string(destination.join("index.html")).unwrap(),
                    "<p>old</p>"
                );
            } else {
                assert!(matches!(
                    preflight,
                    Err(ServiceError::ExportAwaitingAcknowledgement)
                ));
                assert_eq!(
                    fs::read_to_string(destination.join("index.html")).unwrap(),
                    "<p>new</p>"
                );
                acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
            }
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn acknowledgement_refuses_unrecorded_backup_entries_without_deleting_them() {
        let root = test_workspace("export-backup-injection");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# export\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>old</p>",
            &[],
            false,
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>new</p>",
            &[],
            false,
        )
        .unwrap();
        let backup = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".public.fleximark-export-backup-"))
            })
            .unwrap();
        fs::write(backup.join("unrecorded.txt"), "do not delete").unwrap();
        assert!(matches!(
            acknowledge_export(&source_uri, &workspace_uri, &destination_uri),
            Err(ServiceError::ExportRecoveryConflict)
        ));
        assert_eq!(
            fs::read_to_string(backup.join("unrecorded.txt")).unwrap(),
            "do not delete"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
