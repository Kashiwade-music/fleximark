//! Role: Verify export containment, asset identities and retained user files.
use super::{
    filesystem::{object_identity, object_identity_from_metadata, reserved_export_path},
    model::ManagedExport,
};
use crate::ApplicationError;
use crate::native::{assets::ExportAsset, integrity::sha256};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions as CapOpenOptions},
};
use std::{
    fs,
    io::{Read, Seek, Write},
    path::Path,
};
pub(super) fn verify_export_assets(assets: &[ExportAsset]) -> Result<(), ApplicationError> {
    for asset in assets {
        let (Some(source), Some(source_identity)) = (&asset.source, &asset.source_identity) else {
            continue;
        };
        if object_identity(source).ok().as_ref() != Some(source_identity) {
            return Err(ApplicationError::ExportContentConflict);
        }
        let parent = Dir::open_ambient_dir(
            source
                .parent()
                .ok_or(ApplicationError::InvalidExportDestination)?,
            ambient_authority(),
        )?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = parent.open_with(
            source
                .file_name()
                .ok_or(ApplicationError::InvalidExportDestination)?,
            &options,
        )?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes != asset.bytes || sha256(&bytes) != asset.content_hash {
            return Err(ApplicationError::ExportContentConflict);
        }
    }
    Ok(())
}

pub(super) fn revalidate_export_paths(
    workspace: &Path,
    source: &Path,
    destination: &Path,
    expected_parent: &Path,
) -> Result<(), ApplicationError> {
    let parent = destination
        .parent()
        .ok_or(ApplicationError::InvalidExportDestination)?;
    if parent.canonicalize()? != expected_parent
        || !expected_parent.starts_with(workspace)
        || destination == workspace
        || destination.starts_with(workspace.join(".fleximark"))
    {
        return Err(ApplicationError::InvalidExportDestination);
    }
    let mut current = workspace.to_path_buf();
    for component in expected_parent
        .strip_prefix(workspace)
        .map_err(|_| ApplicationError::InvalidExportDestination)?
        .components()
    {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ApplicationError::InvalidExportDestination);
        }
    }
    let source_metadata = fs::symlink_metadata(source)?;
    if !source_metadata.is_file() || source_metadata.file_type().is_symlink() {
        return Err(ApplicationError::InvalidExportDestination);
    }
    if fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ApplicationError::InvalidExportDestination);
    }
    Ok(())
}

pub(super) fn copy_user_files(
    source: &Path,
    destination: &Path,
    previous: &ManagedExport,
) -> Result<(), ApplicationError> {
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
                .map_err(|_| ApplicationError::ReservedExportPath)?;
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if relative == ".fleximark-export.json" {
                continue;
            }
            if reserved_export_path(&relative) {
                return Err(ApplicationError::ReservedExportPath);
            }
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(ApplicationError::ExportContentConflict);
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
                    return Err(ApplicationError::ExportContentConflict);
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
                    return Err(ApplicationError::ExportContentConflict);
                }
            } else {
                return Err(ApplicationError::ExportContentConflict);
            }
        }
    }
    Ok(())
}
