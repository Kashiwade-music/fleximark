//! Role: Validate export ownership and installed filesystem identities.
use super::filesystem::{
    object_identity_from_metadata, open_relative_parent, payload_digest, read_json_regular,
    read_regular_at, safe_relative_path,
};
use super::model::{ManagedExport, OwnershipMarker, RegistryRecord};
use crate::native::ApplicationError;
use crate::native::integrity::sha256;
use cap_std::{ambient_authority, fs::Dir};
use std::path::Path;
pub(super) fn inspect_export(
    destination: &Path,
    registry_path: &Path,
) -> Result<Option<ManagedExport>, ApplicationError> {
    if !destination.exists() {
        return if registry_path.exists() {
            Err(ApplicationError::ExportOwnershipConflict)
        } else {
            Ok(None)
        };
    }
    let directory = Dir::open_ambient_dir(destination, ambient_authority())?;
    let destination_identity = object_identity_from_metadata(&directory.metadata(".")?)?;
    if destination_identity.kind != "directory" {
        return Err(ApplicationError::InvalidExportDestination);
    }
    if directory
        .symlink_metadata(".fleximark-export.json")
        .is_err()
    {
        return if directory.entries()?.next().is_none() {
            Ok(None)
        } else {
            Err(ApplicationError::UnmanagedExport)
        };
    }
    let marker: OwnershipMarker = serde_json::from_slice(&read_regular_at(
        &directory,
        Path::new(".fleximark-export.json"),
    )?)?;
    if marker.payload.format_version != 1 || payload_digest(&marker.payload)? != marker.digest {
        return Err(ApplicationError::ExportOwnershipConflict);
    }
    if destination_identity != marker.payload.destination_object_identity {
        return Err(ApplicationError::ExportOwnershipConflict);
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
        return Err(ApplicationError::ExportOwnershipConflict);
    }
    let mut owned_paths = std::collections::HashSet::new();
    for file in &marker.payload.files {
        if !matches!(file.kind.as_str(), "file" | "directory")
            || !safe_relative_path(&file.path)
            || !owned_paths.insert(file.path.as_str())
        {
            return Err(ApplicationError::ExportOwnershipConflict);
        }
        let relative = Path::new(&file.path);
        let (parent, name) = open_relative_parent(&directory, relative)
            .map_err(|_| ApplicationError::ExportContentConflict)?;
        let metadata = parent
            .symlink_metadata(&name)
            .map_err(|_| ApplicationError::ExportContentConflict)?;
        if metadata.file_type().is_symlink()
            || (file.kind == "file" && !metadata.is_file())
            || (file.kind == "directory" && !metadata.is_dir())
            || object_identity_from_metadata(&metadata)? != file.object_identity
        {
            return Err(ApplicationError::ExportContentConflict);
        }
        if file.kind == "file"
            && sha256(&read_regular_at(&directory, relative)?) != file.content_hash
        {
            return Err(ApplicationError::ExportContentConflict);
        }
    }
    Ok(Some(ManagedExport { marker, registry }))
}

pub(super) fn verify_installed(
    destination: &Path,
    expected: &RegistryRecord,
) -> Result<(), ApplicationError> {
    let directory = Dir::open_ambient_dir(destination, ambient_authority())?;
    let marker: OwnershipMarker = serde_json::from_slice(&read_regular_at(
        &directory,
        Path::new(".fleximark-export.json"),
    )?)?;
    if payload_digest(&marker.payload)? != marker.digest
        || marker.digest != expected.digest
        || marker.payload.destination_id != expected.destination_id
        || marker.payload.generation != expected.generation
        || marker.payload.source_identity != expected.source_identity
        || marker.payload.workspace_identity != expected.workspace_identity
        || marker.payload.destination_identity != expected.destination_identity
        || marker.payload.destination_object_identity != expected.destination_object_identity
        || object_identity_from_metadata(&directory.metadata(".")?)?
            != expected.destination_object_identity
    {
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    let mut owned_paths = std::collections::HashSet::new();
    for file in &marker.payload.files {
        let relative = Path::new(&file.path);
        let (parent, name) = open_relative_parent(&directory, relative)?;
        let metadata = parent.symlink_metadata(&name).ok();
        if !safe_relative_path(&file.path)
            || !owned_paths.insert(file.path.as_str())
            || metadata
                .as_ref()
                .is_none_or(|metadata| metadata.file_type().is_symlink())
            || (file.kind == "file"
                && (metadata.as_ref().is_none_or(|metadata| !metadata.is_file())
                    || sha256(&read_regular_at(&directory, relative)?) != file.content_hash))
            || (file.kind == "directory"
                && metadata.as_ref().is_none_or(|metadata| !metadata.is_dir()))
            || object_identity_from_metadata(
                &metadata.ok_or(ApplicationError::ExportRecoveryConflict)?,
            )? != file.object_identity
            || !matches!(file.kind.as_str(), "file" | "directory")
        {
            return Err(ApplicationError::ExportRecoveryConflict);
        }
    }
    Ok(())
}
