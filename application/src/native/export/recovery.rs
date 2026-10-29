//! Role: Native workspace recovery operations and validation.
use std::path::Path;

use super::commit::{commit_installed_export, install_staged_registry};
use super::filesystem::{
    discard_staging, object_identity, remove_recorded_directory, remove_recorded_file,
    restore_previous_destination, sync_directory, verify_directory_tree,
};
use super::journal::{append_journal, read_journal};
use super::model::ExportJournalState;
use super::ownership::{inspect_export, verify_installed};
use crate::native::ApplicationError;

pub(super) fn recover_export(
    journal_path: &Path,
    expected_destination: &Path,
    expected_registry: &Path,
    acknowledge: bool,
) -> Result<(), ApplicationError> {
    if !journal_path.exists() {
        return Ok(());
    }
    let (mut journal, journal_digest) = read_journal(journal_path)?;
    let parent = journal_path
        .parent()
        .ok_or(ApplicationError::ExportRecoveryConflict)?;
    let name = journal
        .destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ApplicationError::ExportRecoveryConflict)?;
    let valid = journal.destination == expected_destination
        && journal.registry == expected_registry
        && journal.destination.parent() == Some(parent)
        && journal.staging.parent() == Some(parent)
        && journal.backup.parent() == Some(parent)
        && journal
            .staging
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with(&format!(".{name}.fleximark-export-staging-")))
        && journal
            .backup
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with(&format!(".{name}.fleximark-export-backup-")))
        && journal.registry_staging.as_ref().is_none_or(|staging| {
            staging.parent() == journal.registry.parent()
                && staging
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with(".registry-") && value.ends_with(".tmp"))
        });
    if !valid {
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    if object_identity(journal_path).ok() != Some(journal.journal_identity.clone()) {
        return Err(ApplicationError::ExportRecoveryConflict);
    }
    match journal.state {
        ExportJournalState::Prepared => {
            if object_identity(&journal.staging).ok() != Some(journal.staging_identity.clone())
                || object_identity(&journal.staging.join(".fleximark-export.json")).ok()
                    != Some(journal.staging_marker_identity.clone())
                || journal.backup.exists()
                || object_identity(&journal.destination).ok()
                    != journal.previous_destination_identity
            {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
            verify_directory_tree(&journal.staging, &journal.generated_identities)?;
            remove_recorded_directory(
                &journal.staging,
                &journal.staging_identity,
                &journal.generated_identities,
            )?;
            remove_recorded_file(journal_path, &journal.journal_identity)?;
        }
        ExportJournalState::OldMoveIntent => {
            let destination_identity = object_identity(&journal.destination).ok();
            let backup_identity = object_identity(&journal.backup).ok();
            if destination_identity == journal.previous_destination_identity
                && backup_identity.is_none()
            {
                discard_staging(&journal, journal_path)?;
            } else if destination_identity.is_none()
                && backup_identity == journal.previous_destination_identity
                && journal.previous_destination_identity.is_some()
            {
                restore_previous_destination(&journal)?;
                discard_staging(&journal, journal_path)?;
            } else if destination_identity.is_none()
                && backup_identity.is_none()
                && journal.previous_destination_identity.is_none()
            {
                discard_staging(&journal, journal_path)?;
            } else {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
        }
        ExportJournalState::OldMoved => {
            if journal.destination.exists()
                || object_identity(&journal.staging).ok() != Some(journal.staging_identity.clone())
                || object_identity(&journal.backup).ok() != journal.backup_identity
            {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
            if journal.previous_destination_identity.is_some() {
                restore_previous_destination(&journal)?;
            }
            discard_staging(&journal, journal_path)?;
        }
        ExportJournalState::InstallIntent => {
            let staging_identity = object_identity(&journal.staging).ok();
            let destination_identity = object_identity(&journal.destination).ok();
            if staging_identity == Some(journal.staging_identity.clone())
                && destination_identity.is_none()
            {
                if journal.previous_destination_identity.is_some() {
                    restore_previous_destination(&journal)?;
                }
                discard_staging(&journal, journal_path)?;
            } else if staging_identity.is_none()
                && destination_identity == Some(journal.staging_identity.clone())
            {
                journal.installed_identity = Some(journal.staging_identity.clone());
                journal.installed_marker_identity = Some(journal.staging_marker_identity.clone());
                verify_directory_tree(&journal.destination, &journal.generated_identities)?;
                journal.state = ExportJournalState::NewInstalled;
                let digest = append_journal(journal_path, &journal, Some(journal_digest))?;
                commit_installed_export(&mut journal, journal_path, digest, &mut |_| Ok(()))?;
                return Err(ApplicationError::ExportAwaitingAcknowledgement);
            } else {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
        }
        ExportJournalState::NewInstalled => {
            if object_identity(&journal.destination).ok() != journal.installed_identity
                || object_identity(&journal.destination.join(".fleximark-export.json")).ok()
                    != journal.installed_marker_identity
                || journal.staging.exists()
                || (journal.backup.exists()
                    && object_identity(&journal.backup).ok() != journal.backup_identity)
            {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
            verify_directory_tree(&journal.destination, &journal.generated_identities)?;
            commit_installed_export(&mut journal, journal_path, journal_digest, &mut |_| Ok(()))?;
            return Err(ApplicationError::ExportAwaitingAcknowledgement);
        }
        ExportJournalState::RegistryIntent => {
            verify_installed(&journal.destination, &journal.next_registry)?;
            verify_directory_tree(&journal.destination, &journal.generated_identities)?;
            let registry_staging = journal
                .registry_staging
                .as_ref()
                .ok_or(ApplicationError::ExportRecoveryConflict)?;
            let staged_identity = journal
                .registry_staging_identity
                .as_ref()
                .ok_or(ApplicationError::ExportRecoveryConflict)?;
            let current_registry = object_identity(&journal.registry).ok();
            if object_identity(registry_staging).ok().as_ref() == Some(staged_identity)
                && current_registry == journal.previous_registry_identity
            {
                install_staged_registry(&mut journal)?;
                sync_directory(
                    journal
                        .registry
                        .parent()
                        .ok_or(ApplicationError::ExportRecoveryConflict)?,
                )?;
            } else if !registry_staging.exists()
                && current_registry.as_ref() == Some(staged_identity)
            {
                journal.next_registry_identity = current_registry;
            } else {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
            journal.state = ExportJournalState::Committed;
            append_journal(journal_path, &journal, Some(journal_digest))?;
            return Err(ApplicationError::ExportAwaitingAcknowledgement);
        }
        ExportJournalState::Committed => {
            if object_identity(&journal.destination).ok() != journal.installed_identity
                || object_identity(&journal.destination.join(".fleximark-export.json")).ok()
                    != journal.installed_marker_identity
                || object_identity(&journal.registry).ok() != journal.next_registry_identity
                || (journal.backup.exists()
                    && object_identity(&journal.backup).ok() != journal.backup_identity)
            {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
            verify_directory_tree(&journal.destination, &journal.generated_identities)?;
            let managed = inspect_export(&journal.destination, &journal.registry)?
                .ok_or(ApplicationError::ExportRecoveryConflict)?;
            if managed.registry != journal.next_registry {
                return Err(ApplicationError::ExportRecoveryConflict);
            }
            if !acknowledge {
                return Err(ApplicationError::ExportAwaitingAcknowledgement);
            }
            if journal.backup.exists() {
                remove_recorded_directory(
                    &journal.backup,
                    journal
                        .backup_identity
                        .as_ref()
                        .ok_or(ApplicationError::ExportRecoveryConflict)?,
                    &journal.previous_destination_objects,
                )?;
            }
            remove_recorded_file(journal_path, &journal.journal_identity)?;
        }
    }
    sync_directory(parent)
}
