//! Role: Commit installed exports and replace staged ownership registries.
use super::filesystem::{object_identity, sync_directory, verify_directory_tree};
use super::journal::{append_journal, replace_staged_json, stage_json};
use super::model::{ExportJournal, ExportJournalState};
use super::ownership::verify_installed;
use crate::native::{ServiceError, workspace::reject_link};
use std::{fs, path::Path};
pub(super) fn commit_installed_export(
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

pub(super) fn install_staged_registry(journal: &mut ExportJournal) -> Result<(), ServiceError> {
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
