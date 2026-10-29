//! Role: Authorize typed workspace commands and invoke their native operations.
use crate::outcomes::*;
use crate::workspace::PendingMigration;
use crate::{ApplicationError, CancellationToken, DocumentStore};
use fleximark_preview_assets::PreviewAssets;
use fleximark_protocol::{
    CommandContext, MigrationPlanResult, MigrationStatus, PreviewTarget, WorkspaceCommand,
};
use std::sync::Arc;

pub struct MigrationAppliedOutcome {
    pub backup_uri: Option<String>,
    pub recovered: bool,
}

pub enum CommandOutcome {
    Initialized(InitializedWorkspace),
    Theme(ThemeResource),
    Note(CreatedNote),
    NoteOptions(NoteOptions),
    Exported(ExportReceipt),
    ExportAcknowledged,
    Admonitions(CollectedAdmonitions),
    MigrationPlanned(MigrationPlanResult),
    MigrationApplied(MigrationAppliedOutcome),
    OpenPreview {
        document_session_id: String,
        document_version: u64,
        target: PreviewTarget,
        source_offset: Option<u64>,
    },
    OpenWorkspace {
        workspace_uri: String,
    },
}
pub struct WorkspaceOps<'a> {
    store: &'a mut DocumentStore,
    resources: Option<Arc<PreviewAssets>>,
}
impl<'a> WorkspaceOps<'a> {
    pub fn new(store: &'a mut DocumentStore, resources: Option<Arc<PreviewAssets>>) -> Self {
        Self { store, resources }
    }
    pub fn execute(
        &mut self,
        command: WorkspaceCommand,
        context: CommandContext,
        cancellation: &CancellationToken,
    ) -> Result<CommandOutcome, ApplicationError> {
        crate::cancellation_checks::check(cancellation, "command_start")?;
        let descriptor = command.descriptor();
        let workspace = context
            .workspace_uri
            .as_deref()
            .map(|uri| uri.trim_end_matches('/'));
        if descriptor.requires_workspace && workspace.is_none() {
            return Err(ApplicationError::InvalidState(
                "command requires workspaceUri".into(),
            ));
        }
        if descriptor.requires_trust
            && !workspace.is_some_and(|uri| self.store.workspaces.is_trusted(uri))
        {
            return Err(ApplicationError::WriteDenied);
        }
        if let Some(uri) = workspace {
            if !self
                .store
                .workspace_statuses()
                .iter()
                .any(|status| status.uri == uri.trim_end_matches('/'))
            {
                return Err(ApplicationError::InvalidState(
                    "workspace is not registered".into(),
                ));
            }
        }
        let document = if descriptor.requires_document {
            let request = context.document.as_ref().ok_or_else(|| {
                ApplicationError::InvalidState("operation requires an active document".into())
            })?;
            let version = u64::try_from(request.expected_document_version.get())
                .map_err(|_| ApplicationError::VersionMismatch)?;
            let session = self.store.document(&request.document_session_id, version)?;
            if workspace.is_some() && session.workspace_uri() != workspace {
                return Err(ApplicationError::InvalidState(
                    "active document belongs to a different workspace authority".into(),
                ));
            }
            Some((request.document_session_id.clone(), version))
        } else {
            None
        };
        if let WorkspaceCommand::OpenPreview {
            target,
            source_offset,
        } = command
        {
            let (document_session_id, document_version) =
                document.ok_or(ApplicationError::NotOpen)?;
            if let Some(offset) = source_offset {
                let source = self
                    .store
                    .document(&document_session_id, document_version)?
                    .source();
                if !source.is_char_boundary(
                    usize::try_from(offset.get()).map_err(|_| ApplicationError::InvalidRange)?,
                ) {
                    return Err(ApplicationError::InvalidRange);
                }
            }
            return Ok(CommandOutcome::OpenPreview {
                document_session_id,
                document_version,
                target,
                source_offset: source_offset.map(|value| value.get()),
            });
        }
        let uri = workspace.ok_or_else(|| {
            ApplicationError::InvalidState("command requires workspaceUri".into())
        })?;
        if !matches!(
            command,
            WorkspaceCommand::InitializeWorkspace {}
                | WorkspaceCommand::PlanMigration { .. }
                | WorkspaceCommand::ApplyMigration { .. }
                | WorkspaceCommand::OpenWorkspace {}
        ) {
            self.store.workspaces.cached_config(uri)?;
        }
        match command {
            WorkspaceCommand::InitializeWorkspace {} => {
                let outcome = crate::native::initialize_workspace(uri)?;
                self.store
                    .reconfigure_native_workspace(uri, true, cancellation)?;
                Ok(CommandOutcome::Initialized(outcome))
            }
            WorkspaceCommand::EditTheme {} => {
                self.store.workspaces.cached_config(uri)?;
                Ok(CommandOutcome::Theme(crate::native::edit_theme(uri)?))
            }
            WorkspaceCommand::CreateNote {
                category_path,
                template,
                name,
            } => {
                let config = self.store.workspaces.cached_config(uri)?;
                Ok(CommandOutcome::Note(
                    crate::native::create_note_with_options(
                        uri,
                        &config,
                        category_path.as_deref(),
                        template.as_deref(),
                        name.as_deref(),
                    )?,
                ))
            }
            WorkspaceCommand::GetNoteOptions {} => {
                let config = self.store.workspaces.cached_config(uri)?;
                Ok(CommandOutcome::NoteOptions(
                    crate::native::get_note_options(&config),
                ))
            }
            WorkspaceCommand::CollectAdmonitions {} => {
                let (id, version) = document.ok_or(ApplicationError::NotOpen)?;
                let document = self.store.document(&id, version)?;
                Ok(CommandOutcome::Admonitions(
                    crate::native::collect_admonitions(
                        document.document(),
                        document.source(),
                        uri,
                        self.store.workspaces.cached_config(uri)?.as_ref(),
                    )?,
                ))
            }
            WorkspaceCommand::ExportHtml { destination_uri }
            | WorkspaceCommand::AcknowledgeExport { destination_uri } => {
                self.store.workspaces.cached_config(uri)?;
                let (id, version) = document.ok_or(ApplicationError::NotOpen)?;
                let document = self.store.document(&id, version)?;
                let destination = destination_uri
                    .map(Ok)
                    .unwrap_or_else(|| crate::native::default_export_destination(&document.uri))?;
                if descriptor.kind == fleximark_protocol::CommandKind::ExportHtml {
                    Ok(CommandOutcome::Exported(crate::native::export_document(
                        document.engine(),
                        &document.uri,
                        uri,
                        &destination,
                        cancellation,
                        self.resources.as_deref(),
                    )?))
                } else {
                    crate::native::acknowledge_export(&document.uri, uri, &destination)?;
                    Ok(CommandOutcome::ExportAcknowledged)
                }
            }
            WorkspaceCommand::OpenWorkspace {} => Ok(CommandOutcome::OpenWorkspace {
                workspace_uri: uri.into(),
            }),
            WorkspaceCommand::PlanMigration {
                settings_uri,
                recover,
            } => self.plan_migration(uri, settings_uri.as_deref(), recover),
            WorkspaceCommand::ApplyMigration { plan_id } => {
                self.apply_migration(uri, &plan_id, cancellation)
            }
            WorkspaceCommand::OpenPreview { .. } => Err(ApplicationError::InvalidState(
                "preview requires a document".into(),
            )),
        }
    }
    fn plan_migration(
        &mut self,
        uri: &str,
        settings: Option<&str>,
        recover: bool,
    ) -> Result<CommandOutcome, ApplicationError> {
        let root = crate::native::workspace_path(uri)?;
        let next = self
            .store
            .workspaces
            .next_plan
            .checked_add(1)
            .ok_or_else(|| {
                ApplicationError::InternalState("migration plan counter exhausted".into())
            })?;
        let plan_id = format!("migration-{next}");
        let (pending, status, outputs, warnings, config) = if recover {
            (
                PendingMigration::Recovery(root),
                MigrationStatus::RecoveryRequired,
                vec![],
                vec![],
                String::new(),
            )
        } else {
            let settings = settings.map(crate::native::workspace_path).transpose()?;
            let plan = fleximark_migration::plan(&root, settings.as_deref())?;
            let status = match plan.status {
                fleximark_migration::Status::Ready => MigrationStatus::Ready,
                fleximark_migration::Status::AlreadyCurrent => MigrationStatus::AlreadyCurrent,
            };
            let outputs = plan.outputs.clone();
            let warnings = plan.warnings.clone();
            let config = plan.config.clone();
            (
                PendingMigration::Plan(plan),
                status,
                outputs,
                warnings,
                config,
            )
        };
        self.store.workspaces.next_plan = next;
        self.store
            .workspaces
            .migrations
            .insert(plan_id.clone(), (uri.into(), pending));
        Ok(CommandOutcome::MigrationPlanned(MigrationPlanResult {
            plan_id,
            workspace_uri: uri.into(),
            status,
            outputs,
            warnings,
            config,
            recovery: recover,
        }))
    }
    fn apply_migration(
        &mut self,
        uri: &str,
        id: &str,
        cancellation: &CancellationToken,
    ) -> Result<CommandOutcome, ApplicationError> {
        let (root, plan) = self.store.workspaces.migrations.get(id).ok_or_else(|| {
            ApplicationError::InvalidState("migration plan is unknown or expired".into())
        })?;
        if root != uri {
            return Err(ApplicationError::InvalidState(
                "migration plan belongs to a different workspace".into(),
            ));
        }
        crate::cancellation_checks::check(cancellation, "migration_apply")?;
        let result = match plan {
            PendingMigration::Plan(plan) => {
                CommandOutcome::MigrationApplied(MigrationAppliedOutcome {
                    backup_uri: plan
                        .apply()?
                        .map(|path| crate::native::path_to_file_uri(&path))
                        .transpose()?,
                    recovered: false,
                })
            }
            PendingMigration::Recovery(root) => {
                fleximark_migration::recover(root)?;
                CommandOutcome::MigrationApplied(MigrationAppliedOutcome {
                    backup_uri: None,
                    recovered: true,
                })
            }
        };
        self.store.workspaces.migrations.remove(id);
        self.store
            .reconfigure_native_workspace(uri, true, &CancellationToken::default())?;
        Ok(result)
    }
}
