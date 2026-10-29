//! Role: Persist and validate append-only export recovery journals.
use crate::native::integrity::sha256;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions as CapOpenOptions;
use cap_std::{ambient_authority, fs::Dir};
use serde::Serialize;

use super::filesystem::{
    object_identity, object_identity_from_metadata, open_parent, random_id, sync_directory,
    write_json,
};
use super::model::{ExportJournal, ExportJournalRecord, ExportJournalState, ObjectIdentity};
use crate::native::ApplicationError;

pub(super) fn create_journal(path: &Path) -> Result<ObjectIdentity, ApplicationError> {
    let (parent, name) = open_parent(path)?;
    let mut options = CapOpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let file = parent.open_with(&name, &options)?;
    file.sync_all()?;
    object_identity_from_metadata(&file.metadata()?)
}

pub(super) fn append_journal(
    path: &Path,
    journal: &ExportJournal,
    previous_record_digest: Option<String>,
) -> Result<String, ApplicationError> {
    let (parent, name) = open_parent(path)?;
    let mut options = CapOpenOptions::new();
    options.read(true).append(true).follow(FollowSymlinks::No);
    let mut file = parent.open_with(&name, &options)?;
    if object_identity_from_metadata(&file.metadata()?)? != journal.journal_identity {
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    let digest = sha256(&serde_json::to_vec(&(
        journal,
        previous_record_digest.as_deref(),
    ))?);
    let record = ExportJournalRecord {
        journal: journal.clone(),
        previous_record_digest,
        record_digest: digest.clone(),
    };
    serde_json::to_writer(&mut file, &record)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(digest)
}

pub(super) fn read_journal(path: &Path) -> Result<(ExportJournal, String), ApplicationError> {
    let (parent, name) = open_parent(path)?;
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = parent.open_with(&name, &options)?;
    let identity = object_identity_from_metadata(&file.metadata()?)?;
    if identity.kind != "file" {
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    if object_identity_from_metadata(&file.metadata()?)? != identity {
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    let mut previous: Option<ExportJournalRecord> = None;
    for line in text.lines() {
        let record: ExportJournalRecord =
            serde_json::from_str(line).map_err(|_| ApplicationError::ExportRecoveryConflict)?;
        let expected = sha256(&serde_json::to_vec(&(
            &record.journal,
            record.previous_record_digest.as_deref(),
        ))?);
        if expected != record.record_digest
            || record.previous_record_digest
                != previous.as_ref().map(|record| record.record_digest.clone())
        {
            return Err(ApplicationError::ExportRecoveryConflict);
        }
        if let Some(prior) = &previous {
            let transition = matches!(
                (&prior.journal.state, &record.journal.state),
                (
                    ExportJournalState::Prepared,
                    ExportJournalState::OldMoveIntent
                ) | (
                    ExportJournalState::OldMoveIntent,
                    ExportJournalState::OldMoved
                ) | (
                    ExportJournalState::OldMoved,
                    ExportJournalState::InstallIntent
                ) | (
                    ExportJournalState::InstallIntent,
                    ExportJournalState::NewInstalled
                ) | (
                    ExportJournalState::NewInstalled,
                    ExportJournalState::RegistryIntent
                ) | (
                    ExportJournalState::RegistryIntent,
                    ExportJournalState::Committed
                )
            );
            let registry_staging_is_stable = if matches!(
                (&prior.journal.state, &record.journal.state),
                (
                    ExportJournalState::NewInstalled,
                    ExportJournalState::RegistryIntent
                )
            ) {
                prior.journal.registry_staging.is_none()
                    && prior.journal.registry_staging_identity.is_none()
                    && record.journal.registry_staging.is_some()
                    && record.journal.registry_staging_identity.is_some()
            } else {
                record.journal.registry_staging == prior.journal.registry_staging
                    && record.journal.registry_staging_identity
                        == prior.journal.registry_staging_identity
            };
            let stable = record.journal.transaction_id == prior.journal.transaction_id
                && record.journal.destination == prior.journal.destination
                && record.journal.staging == prior.journal.staging
                && record.journal.backup == prior.journal.backup
                && record.journal.registry == prior.journal.registry
                && record.journal.journal_identity == prior.journal.journal_identity
                && record.journal.previous_registry == prior.journal.previous_registry
                && record.journal.next_registry == prior.journal.next_registry
                && record.journal.staging_identity == prior.journal.staging_identity
                && record.journal.staging_marker_identity == prior.journal.staging_marker_identity
                && record.journal.generated_identities == prior.journal.generated_identities
                && record.journal.previous_destination_objects
                    == prior.journal.previous_destination_objects
                && record.journal.previous_destination_identity
                    == prior.journal.previous_destination_identity
                && record.journal.previous_registry_identity
                    == prior.journal.previous_registry_identity
                && registry_staging_is_stable;
            if !transition || !stable {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
        } else if record.journal.state != ExportJournalState::Prepared {
            return Err(ApplicationError::ExportRecoveryConflict);
        }
        previous = Some(record);
    }
    previous
        .map(|record| (record.journal, record.record_digest))
        .ok_or(ApplicationError::ExportRecoveryConflict)
}

pub(super) fn stage_json<T: Serialize>(
    parent: &Path,
    value: &T,
) -> Result<(PathBuf, ObjectIdentity), ApplicationError> {
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".registry-{}.tmp", random_id()?));
    write_json(&temporary, value, true)?;
    let identity = object_identity(&temporary)?;
    sync_directory(parent)?;
    Ok((temporary, identity))
}

pub(super) fn replace_staged_json(temporary: &Path, path: &Path) -> Result<(), ApplicationError> {
    let parent = path
        .parent()
        .ok_or(ApplicationError::ExportRecoveryConflict)?;
    if temporary.parent() != Some(parent) {
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    let directory = Dir::open_ambient_dir(parent, ambient_authority())?;
    directory.rename(
        temporary
            .file_name()
            .ok_or(ApplicationError::ExportRecoveryConflict)?,
        &directory,
        path.file_name()
            .ok_or(ApplicationError::ExportRecoveryConflict)?,
    )?;
    sync_directory(parent)
}
