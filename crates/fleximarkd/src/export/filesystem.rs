use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt as _, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions as CapOpenOptions;
use cap_std::{ambient_authority, fs::Dir};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::model::{ExportJournal, ObjectIdentity, OwnershipPayload, RecordedObject};
use crate::ServiceError;

pub(super) fn read_json_regular<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<T, ServiceError> {
    let bytes = read_regular_from_parent(path).map_err(|error| match error {
        ServiceError::Json(error) => ServiceError::Json(error),
        _ => ServiceError::ExportOwnershipConflict,
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn write_json<T: Serialize>(
    path: &Path,
    value: &T,
    create_new: bool,
) -> Result<(), ServiceError> {
    write_synced(path, &serde_json::to_vec_pretty(value)?, create_new)
}

pub(super) fn write_synced(
    path: &Path,
    bytes: &[u8],
    create_new: bool,
) -> Result<(), ServiceError> {
    let (parent, name) = open_parent(path)?;
    let mut options = CapOpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    options.follow(FollowSymlinks::No);
    let mut file = parent.open_with(&name, &options)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn payload_digest(payload: &OwnershipPayload) -> Result<String, ServiceError> {
    Ok(sha256(&serde_json::to_vec(payload)?))
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn open_parent(path: &Path) -> Result<(Dir, std::ffi::OsString), ServiceError> {
    let parent = path.parent().ok_or(ServiceError::ExportRecoveryConflict)?;
    let name = path
        .file_name()
        .ok_or(ServiceError::ExportRecoveryConflict)?
        .to_owned();
    Ok((Dir::open_ambient_dir(parent, ambient_authority())?, name))
}

pub(super) fn read_regular_from_parent(path: &Path) -> Result<Vec<u8>, ServiceError> {
    let (parent, name) = open_parent(path)?;
    read_regular_at(&parent, Path::new(&name))
}

pub(super) fn open_relative_parent(
    root: &Dir,
    path: &Path,
) -> Result<(Dir, std::ffi::OsString), ServiceError> {
    use std::path::Component;
    let mut components = path.components().peekable();
    let mut directory = root.try_clone()?;
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(ServiceError::ExportRecoveryConflict);
        };
        if components.peek().is_none() {
            return Ok((directory, name.to_owned()));
        }
        directory = directory.open_dir_nofollow(name)?;
    }
    Err(ServiceError::ExportRecoveryConflict)
}

pub(super) fn read_regular_at(root: &Dir, path: &Path) -> Result<Vec<u8>, ServiceError> {
    let (parent, name) = open_relative_parent(root, path)?;
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = parent.open_with(&name, &options)?;
    let before = object_identity_from_metadata(&file.metadata()?)?;
    if before.kind != "file" {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if object_identity_from_metadata(&file.metadata()?)? != before {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    Ok(bytes)
}

pub(super) fn object_identity_from_metadata(
    metadata: &cap_std::fs::Metadata,
) -> Result<ObjectIdentity, ServiceError> {
    let kind = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else {
        return Err(ServiceError::ExportRecoveryConflict);
    };
    Ok(ObjectIdentity {
        platform_id: format!("{}:{}", metadata.dev(), metadata.ino()),
        kind: kind.into(),
    })
}

pub(super) fn object_identity(path: &Path) -> Result<ObjectIdentity, ServiceError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let kind = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else {
        return Err(ServiceError::ExportRecoveryConflict);
    };
    #[cfg(windows)]
    let platform_id = {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let handle = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
        if unsafe { GetFileInformationByHandle(handle.as_raw_handle().cast(), &mut information) }
            == 0
        {
            return Err(ServiceError::Io(std::io::Error::last_os_error()));
        }
        format!(
            "{}:{}",
            information.dwVolumeSerialNumber,
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow)
        )
    };
    #[cfg(unix)]
    let platform_id = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", metadata.dev(), metadata.ino())
    };
    #[cfg(not(any(unix, windows)))]
    return Err(ServiceError::ExportRecoveryConflict);
    Ok(ObjectIdentity {
        platform_id,
        kind: kind.into(),
    })
}

pub(super) fn record_tree(root: &Path) -> Result<Vec<RecordedObject>, ServiceError> {
    let mut objects = Vec::new();
    let mut pending = vec![(
        Dir::open_ambient_dir(root, ambient_authority())?,
        String::new(),
    )];
    while let Some((directory, prefix)) = pending.pop() {
        for entry in directory.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(ServiceError::ExportRecoveryConflict)?;
            if name.contains('/') || name.contains('\\') || matches!(name, "." | "..") {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            let relative_path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            let (identity, content_hash) = if file_type.is_dir() {
                let child = entry.open_dir()?;
                let identity = object_identity_from_metadata(&child.metadata(".")?)?;
                pending.push((child, relative_path.clone()));
                (identity, None)
            } else if file_type.is_file() {
                let mut options = CapOpenOptions::new();
                options.read(true).follow(FollowSymlinks::No);
                let mut file = entry.open_with(&options)?;
                let before = object_identity_from_metadata(&file.metadata()?)?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                if object_identity_from_metadata(&file.metadata()?)? != before {
                    return Err(ServiceError::ExportRecoveryConflict);
                }
                (before, Some(sha256(&bytes)))
            } else {
                return Err(ServiceError::ExportRecoveryConflict);
            };
            objects.push(RecordedObject {
                relative_path,
                content_hash,
                identity,
            });
        }
    }
    objects.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(objects)
}

pub(super) fn remove_recorded_directory(
    path: &Path,
    expected: &ObjectIdentity,
    objects: &[RecordedObject],
) -> Result<(), ServiceError> {
    let directory = Dir::open_ambient_dir(path, ambient_authority())?;
    if object_identity_from_metadata(&directory.metadata(".")?)? != *expected
        || expected.kind != "directory"
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let actual = record_tree(path)?;
    if actual != objects {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let mut leaf_first = objects.to_vec();
    leaf_first.sort_by_key(|object| std::cmp::Reverse(object.relative_path.matches('/').count()));
    for object in leaf_first {
        let (parent, name) = open_relative_parent(&directory, Path::new(&object.relative_path))?;
        if object_identity_from_metadata(&parent.symlink_metadata(&name)?)? != object.identity {
            return Err(ServiceError::ExportRecoveryConflict);
        }
        if object.identity.kind == "directory" {
            parent.remove_dir(&name)?;
        } else {
            parent.remove_file(&name)?;
        }
    }
    drop(directory);
    let (parent, name) = open_parent(path)?;
    if object_identity_from_metadata(&parent.symlink_metadata(&name)?)? != *expected {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    parent.remove_dir(&name)?;
    Ok(())
}

pub(super) fn remove_recorded_file(
    path: &Path,
    expected: &ObjectIdentity,
) -> Result<(), ServiceError> {
    let (parent, name) = open_parent(path)?;
    if object_identity_from_metadata(&parent.symlink_metadata(&name)?)? != *expected
        || expected.kind != "file"
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    parent.remove_file(&name)?;
    Ok(())
}

pub(super) fn verify_directory_tree(
    root: &Path,
    expected: &[RecordedObject],
) -> Result<(), ServiceError> {
    if record_tree(root)? == expected {
        Ok(())
    } else {
        Err(ServiceError::ExportRecoveryConflict)
    }
}

pub(super) fn restore_previous_destination(journal: &ExportJournal) -> Result<(), ServiceError> {
    let expected = journal
        .backup_identity
        .as_ref()
        .or(journal.previous_destination_identity.as_ref())
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    if journal.destination.exists()
        || object_identity(&journal.backup).ok().as_ref() != Some(expected)
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    verify_directory_tree(&journal.backup, &journal.previous_destination_objects)?;
    let backup_parent = journal
        .backup
        .parent()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    let destination_parent = journal
        .destination
        .parent()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    if backup_parent != destination_parent {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let parent = Dir::open_ambient_dir(backup_parent, ambient_authority())?;
    parent.rename(
        journal
            .backup
            .file_name()
            .ok_or(ServiceError::ExportRecoveryConflict)?,
        &parent,
        journal
            .destination
            .file_name()
            .ok_or(ServiceError::ExportRecoveryConflict)?,
    )?;
    if object_identity_from_metadata(
        &parent.symlink_metadata(
            journal
                .destination
                .file_name()
                .ok_or(ServiceError::ExportRecoveryConflict)?,
        )?,
    )? != *expected
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    Ok(())
}

pub(super) fn discard_staging(
    journal: &ExportJournal,
    journal_path: &Path,
) -> Result<(), ServiceError> {
    verify_directory_tree(&journal.staging, &journal.generated_identities)?;
    remove_recorded_directory(
        &journal.staging,
        &journal.staging_identity,
        &journal.generated_identities,
    )?;
    remove_recorded_file(journal_path, &journal.journal_identity)
}

pub(super) fn random_id() -> Result<String, ServiceError> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        ServiceError::Io(std::io::Error::other(format!(
            "OS random source failed: {error}"
        )))
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn sync_directory(path: &Path) -> Result<(), ServiceError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{
            CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
        };
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FlushFileBuffers, OPEN_EXISTING,
        };
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(ServiceError::Io(std::io::Error::last_os_error()));
        }
        let flushed = unsafe { FlushFileBuffers(handle) };
        let flush_error = if flushed == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        unsafe { CloseHandle(handle) };
        if let Some(error) = flush_error {
            return Err(ServiceError::Io(error));
        }
    }
    #[cfg(not(windows))]
    OpenOptions::new().read(true).open(path)?.sync_all()?;
    Ok(())
}

pub(super) fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !reserved_export_path(path)
        && Path::new(path)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

pub(super) fn reserved_export_path(path: &str) -> bool {
    path.split('/').any(|part| {
        part == ".fleximark-export.json"
            || part.contains(".fleximark-export-staging-")
            || part.contains(".fleximark-export-backup-")
            || part.ends_with(".fleximark-export-journal.json")
    })
}

pub(super) fn index_for_destination(destination: &Path) -> PathBuf {
    destination.join("index.html")
}
