//! Role: Transfer commands and asset continuations without blocking the actor.
use crate::authority::DocumentAuthority;
use crate::{ApplicationError, CancellationToken, CommandOutcome, DocumentStore, WorkspaceOps};
use fleximark_protocol::{CommandContext, CommandKind, WorkspaceCommand};
use std::sync::Arc;
struct DocumentCapture {
    id: String,
    version: u64,
    epoch: u64,
    authority: Arc<DocumentAuthority>,
}
impl DocumentCapture {
    fn is_current(&self) -> bool {
        self.authority.state().is_ok_and(|state| {
            state.open
                && state.in_sync
                && state.version == self.version
                && state.source_epoch == self.epoch
        })
    }
}
pub enum OperationProgress {
    Complete(CommandOutcome),
    Continue(PreparedWorkspaceOperation),
}
enum PreparedPhase {
    Command(WorkspaceCommand),
    AssetRefresh {
        outcome: CommandOutcome,
        kind: CommandKind,
    },
}
pub struct PreparedWorkspaceOperation(Box<PreparedState>);
struct PreparedState {
    store: DocumentStore,
    phase: PreparedPhase,
    context: CommandContext,
    cancellation: CancellationToken,
    request_token: CancellationToken,
    lease: Option<CancellationToken>,
    requested: Option<DocumentCapture>,
    captures: Vec<DocumentCapture>,
    resources: Option<Arc<fleximark_preview_assets::PreviewAssets>>,
}
pub struct OperationCompletion {
    store: DocumentStore,
    workspace: Option<String>,
    kind: CommandKind,
    cancellation: CancellationToken,
    request_token: CancellationToken,
    lease: Option<CancellationToken>,
    requested: Option<DocumentCapture>,
    captures: Vec<DocumentCapture>,
    result: Result<CommandOutcome, ApplicationError>,
    refresh: bool,
    failed_assets: Vec<String>,
}
impl PreparedWorkspaceOperation {
    pub fn workspace_uri(&self) -> Option<&str> {
        self.0.context.workspace_uri.as_deref()
    }
    pub fn execute(self) -> OperationCompletion {
        let PreparedState {
            mut store,
            phase,
            context,
            cancellation,
            request_token,
            lease,
            requested,
            captures,
            resources,
        } = *self.0;
        let mut failed_assets = Vec::new();
        let (kind, result, refresh) = match phase {
            PreparedPhase::Command(command) => {
                let kind = command.kind();
                let result = if cancellation.is_cancelled() {
                    Err(ApplicationError::Cancelled)
                } else if requested
                    .as_ref()
                    .is_some_and(|capture| !capture.is_current())
                {
                    Err(ApplicationError::ContentModified)
                } else {
                    let valid = if command.descriptor().requires_trust {
                        context
                            .workspace_uri
                            .as_deref()
                            .map(|uri| store.workspaces.validate_execution_root(uri))
                            .transpose()
                            .map(|_| ())
                    } else {
                        Ok(())
                    };
                    valid.and_then(|()| {
                        WorkspaceOps::new(&mut store, resources).execute_immediate(
                            command,
                            context.clone(),
                            &cancellation,
                        )
                    })
                };
                if result.is_ok()
                    && matches!(
                        kind,
                        CommandKind::InitializeWorkspace | CommandKind::ApplyMigration
                    )
                {
                    if let Some(lease) = &lease {
                        lease.cancel();
                    }
                }
                (kind, result, false)
            }
            PreparedPhase::AssetRefresh { outcome, kind } => {
                if !cancellation.is_cancelled() {
                    let uris = store
                        .index
                        .iter()
                        .map(|(uri, _)| uri.to_owned())
                        .collect::<Vec<_>>();
                    for uri in uris {
                        if cancellation.is_cancelled() {
                            break;
                        }
                        let result = (|| {
                            let workspace = context.workspace_uri.as_deref().ok_or_else(|| {
                                ApplicationError::InvalidState(
                                    "asset refresh requires a workspace".into(),
                                )
                            })?;
                            store.workspaces.validate_execution_root(workspace)?;
                            let config = store.workspaces.cached_config(workspace)?;
                            let session = store
                                .index
                                .by_uri_mut(&uri)
                                .ok_or(ApplicationError::NotOpen)?;
                            crate::native::refresh_document_assets(
                                &mut session.engine,
                                workspace,
                                &config,
                                &mut session.asset_memo,
                            )
                        })();
                        if let Err(error) = result {
                            crate::logging::log_operational_failure(
                                "workspace-assets-failed",
                                &error,
                                Some(&uri),
                                None,
                                None,
                            );
                            failed_assets.push(uri);
                        }
                    }
                }
                (kind, Ok(outcome), true)
            }
        };
        OperationCompletion {
            store,
            workspace: context.workspace_uri,
            kind,
            cancellation,
            request_token,
            lease,
            requested,
            captures,
            result,
            refresh,
            failed_assets,
        }
    }
}
impl WorkspaceOps<'_> {
    pub fn prepare(
        &mut self,
        command: WorkspaceCommand,
        mut context: CommandContext,
        cancellation: &CancellationToken,
    ) -> Result<PreparedWorkspaceOperation, ApplicationError> {
        crate::cancellation_checks::check(cancellation, "operation_prepare")?;
        context.workspace_uri = context
            .workspace_uri
            .map(|uri| uri.trim_end_matches('/').to_owned());
        let requested = self.authorize(&command, &context)?;
        let lease = context
            .workspace_uri
            .as_deref()
            .map(|uri| {
                self.store.workspaces.operation_lease(uri).ok_or_else(|| {
                    ApplicationError::InvalidState("workspace is not registered".into())
                })
            })
            .transpose()?;
        let request_token = cancellation.clone();
        let cancellation = lease.as_ref().map_or_else(
            || cancellation.clone(),
            |lease| CancellationToken::linked(cancellation, lease),
        );
        let mut store = DocumentStore::new(self.store.position_encoding());
        store.config_generation = self.store.config_generation;
        store.workspaces = self
            .store
            .workspaces
            .worker_snapshot(context.workspace_uri.as_deref());
        let mut captures = Vec::new();
        let mut requested_capture = None;
        for (_, session) in self.store.index.iter() {
            let is_requested = requested.as_ref().is_some_and(|(id, _)| *id == session.id);
            if is_requested
                || (context.workspace_uri.is_some()
                    && session.workspace_uri == context.workspace_uri)
            {
                let state = session.authority.state()?;
                if is_requested {
                    requested_capture = Some(DocumentCapture {
                        id: session.id.clone(),
                        version: state.version,
                        epoch: state.source_epoch,
                        authority: Arc::clone(&session.authority),
                    });
                }
                captures.push(DocumentCapture {
                    id: session.id.clone(),
                    version: state.version,
                    epoch: state.source_epoch,
                    authority: Arc::clone(&session.authority),
                });
                store.index.insert(crate::DocumentSession {
                    id: session.id.clone(),
                    uri: session.uri.clone(),
                    workspace_uri: session.workspace_uri.clone(),
                    engine: session.engine.clone(),
                    asset_memo: Default::default(),
                    previews: session.previews.clone(),
                    authority: Arc::new(DocumentAuthority::new(
                        session.document().document_version,
                    )),
                });
            }
        }
        if matches!(command, WorkspaceCommand::PlanMigration { .. }) {
            let next = self
                .store
                .workspaces
                .next_plan
                .checked_add(1)
                .ok_or_else(|| {
                    ApplicationError::InternalState("migration plan counter exhausted".into())
                })?;
            self.store.workspaces.next_plan = next;
            store.workspaces.next_plan = next - 1;
        }
        if let WorkspaceCommand::ApplyMigration { plan_id } = &command {
            let (root, _) = self
                .store
                .workspaces
                .migrations
                .get(plan_id)
                .ok_or_else(|| {
                    ApplicationError::InvalidState("migration plan is unknown or expired".into())
                })?;
            if Some(root.as_str()) != context.workspace_uri.as_deref() {
                return Err(ApplicationError::InvalidState(
                    "migration plan belongs to a different workspace".into(),
                ));
            }
            let plan = self
                .store
                .workspaces
                .migrations
                .remove(plan_id)
                .ok_or_else(|| {
                    ApplicationError::InvalidState("migration plan is unknown or expired".into())
                })?;
            store.workspaces.migrations.insert(plan_id.clone(), plan);
        }
        Ok(PreparedWorkspaceOperation(Box::new(PreparedState {
            store,
            phase: PreparedPhase::Command(command),
            context,
            request_token,
            cancellation,
            lease,
            requested: requested_capture,
            captures,
            resources: self.resources.clone(),
        })))
    }
    pub fn complete(
        &mut self,
        completion: OperationCompletion,
    ) -> Result<OperationProgress, ApplicationError> {
        let OperationCompletion {
            store,
            workspace,
            kind,
            cancellation,
            request_token,
            lease,
            requested,
            captures,
            result,
            refresh,
            failed_assets,
        } = completion;
        let outcome = result?;
        let committed = matches!(
            outcome,
            CommandOutcome::Initialized(_)
                | CommandOutcome::Note(_)
                | CommandOutcome::Exported(_)
                | CommandOutcome::ExportAcknowledged
                | CommandOutcome::Admonitions(_)
                | CommandOutcome::MigrationApplied(_)
        );
        let config_write = !refresh
            && matches!(
                kind,
                CommandKind::InitializeWorkspace | CommandKind::ApplyMigration
            );
        let current = match (workspace.as_deref(), lease.as_ref()) {
            (Some(uri), Some(lease)) => {
                if config_write {
                    self.store.workspaces.operation_matches(uri, lease)
                } else {
                    self.store.workspaces.operation_current(uri, lease)
                }
            }
            (None, None) => true,
            _ => false,
        };
        if !current {
            if committed {
                return Ok(OperationProgress::Complete(outcome));
            }
            return Err(ApplicationError::Cancelled);
        }
        if !committed && cancellation.is_cancelled() {
            return Err(ApplicationError::Cancelled);
        }
        if !committed
            && requested
                .as_ref()
                .is_some_and(|capture| !capture.is_current())
        {
            return Err(ApplicationError::ContentModified);
        }
        if refresh {
            let Some(workspace) = workspace else {
                return Ok(OperationProgress::Complete(outcome));
            };
            if request_token.is_cancelled() {
                return Ok(OperationProgress::Complete(outcome));
            }
            let mut retry = Vec::new();
            for capture in captures {
                let Some(snapshot) = store.index.by_session(&capture.id) else {
                    continue;
                };
                if !capture.is_current() {
                    retry.push(snapshot.uri.clone());
                    continue;
                }
                if failed_assets.contains(&snapshot.uri) {
                    continue;
                }
                let Some(session) = self.store.index.by_session_mut(&capture.id) else {
                    continue;
                };
                let mut candidate = session.engine.clone();
                if let Err(error) =
                    candidate.set_render_config(snapshot.engine.render_config().clone())
                {
                    crate::logging::log_operational_failure(
                        "workspace-assets-invalid",
                        &ApplicationError::from(error),
                        Some(&session.uri),
                        Some(&session.id),
                        None,
                    );
                    continue;
                }
                session.engine = candidate;
                session.asset_memo = snapshot.asset_memo.clone();
                for preview in session.previews.values() {
                    preview.invalidate_publication();
                }
            }
            return Ok(self.prepare_asset_followup(workspace, kind, outcome, request_token, retry));
        }
        let mut refresh_uris = Vec::new();
        if config_write {
            if let Some(uri) = workspace.as_deref() {
                let documents = self
                    .store
                    .index
                    .iter()
                    .filter(|(_, session)| session.workspace_uri.as_deref() == Some(uri))
                    .map(|(uri, _)| uri.to_owned())
                    .collect::<Vec<_>>();
                for document_uri in &documents {
                    if let Err(error) = self
                        .store
                        .index
                        .by_uri(document_uri)
                        .ok_or(ApplicationError::NotOpen)?
                        .authority
                        .check_advance()
                    {
                        crate::logging::log_operational_failure(
                            "workspace-authority-exhausted",
                            &error,
                            Some(document_uri),
                            None,
                            None,
                        );
                        self.store.workspaces.invalidate(uri);
                        self.store
                            .workspaces
                            .set_state(uri, crate::WorkspaceState::Invalid(error.to_string()));
                        self.store.workspaces.record_failure(uri, error);
                        return Ok(OperationProgress::Complete(outcome));
                    }
                }
                let Some(base) = store.workspaces.root_render(uri) else {
                    return Ok(OperationProgress::Complete(outcome));
                };
                let compatible_assets = self
                    .store
                    .workspaces
                    .cached_config(uri)
                    .ok()
                    .zip(store.workspaces.cached_config(uri).ok())
                    .is_some_and(|(old, new)| old.assets == new.assets);
                self.store
                    .workspaces
                    .merge_worker_root(uri, &store.workspaces);
                self.store.config_generation =
                    self.store.config_generation.max(store.config_generation);
                for document_uri in documents {
                    let Some(session) = self.store.index.by_uri_mut(&document_uri) else {
                        continue;
                    };
                    let unchanged = captures
                        .iter()
                        .any(|capture| capture.id == session.id && capture.is_current());
                    let config = if unchanged {
                        store
                            .index
                            .by_uri(&document_uri)
                            .map(|snapshot| snapshot.engine.render_config().clone())
                            .unwrap_or_else(|| base.clone())
                    } else if compatible_assets {
                        let mut config = session.engine.render_config().clone();
                        config.style = base.style.clone();
                        config.config_hash = base.config_hash.clone();
                        config.config_generation = base.config_generation;
                        config.context.target = base.context.target;
                        config.context.allow_remote_resources = base.context.allow_remote_resources;
                        config.context.allow_data_resources = base.context.allow_data_resources;
                        config.renderer_version = base.renderer_version.clone();
                        config.sanitizer_version = base.sanitizer_version.clone();
                        config
                    } else {
                        refresh_uris.push(document_uri.clone());
                        base.clone()
                    };
                    let mut candidate = session.engine.clone();
                    if let Err(error) = candidate.set_render_config(config) {
                        crate::logging::log_operational_failure(
                            "workspace-render-policy-failed",
                            &ApplicationError::from(error),
                            Some(&document_uri),
                            None,
                            None,
                        );
                        continue;
                    }
                    if let Err(error) = session
                        .authority
                        .commit(candidate.document().document_version)
                    {
                        crate::logging::log_operational_failure(
                            "workspace-authority-failed",
                            &error,
                            Some(&document_uri),
                            None,
                            None,
                        );
                        continue;
                    }
                    session.asset_memo.clear();
                    session.engine = candidate;
                }
            }
        } else if matches!(kind, CommandKind::PlanMigration) {
            self.store
                .workspaces
                .migrations
                .extend(store.workspaces.migrations);
        }
        self.store.workspaces.next_plan = self
            .store
            .workspaces
            .next_plan
            .max(store.workspaces.next_plan);
        if let Some(workspace) = workspace {
            return Ok(self.prepare_asset_followup(
                workspace,
                kind,
                outcome,
                request_token,
                refresh_uris,
            ));
        }
        Ok(OperationProgress::Complete(outcome))
    }
    fn prepare_asset_followup(
        &mut self,
        workspace: String,
        kind: CommandKind,
        outcome: CommandOutcome,
        request_token: CancellationToken,
        uris: Vec<String>,
    ) -> OperationProgress {
        if uris.is_empty()
            || request_token.is_cancelled()
            || !self.store.workspaces.is_trusted(&workspace)
        {
            return OperationProgress::Complete(outcome);
        }
        let Some(lease) = self.store.workspaces.operation_lease(&workspace) else {
            return OperationProgress::Complete(outcome);
        };
        if lease.is_cancelled() {
            return OperationProgress::Complete(outcome);
        }
        let mut snapshot = DocumentStore::new(self.store.position_encoding());
        snapshot.config_generation = self.store.config_generation;
        snapshot.workspaces = self.store.workspaces.worker_snapshot(Some(&workspace));
        let mut captures = Vec::new();
        for uri in uris {
            let Some(session) = self.store.index.by_uri(&uri) else {
                continue;
            };
            if session.workspace_uri.as_deref() != Some(workspace.as_str())
                || session.engine.is_out_of_sync()
            {
                continue;
            }
            let state = match session.authority.state() {
                Ok(state) => state,
                Err(error) => {
                    crate::logging::log_operational_failure(
                        "workspace-assets-capture-failed",
                        &error,
                        Some(&uri),
                        None,
                        None,
                    );
                    continue;
                }
            };
            if !state.open || !state.in_sync {
                continue;
            }
            captures.push(DocumentCapture {
                id: session.id.clone(),
                version: state.version,
                epoch: state.source_epoch,
                authority: Arc::clone(&session.authority),
            });
            snapshot.index.insert(crate::DocumentSession {
                id: session.id.clone(),
                uri: session.uri.clone(),
                workspace_uri: session.workspace_uri.clone(),
                engine: session.engine.clone(),
                asset_memo: Default::default(),
                previews: session.previews.clone(),
                authority: Arc::new(DocumentAuthority::new(state.version)),
            });
        }
        if captures.is_empty() {
            return OperationProgress::Complete(outcome);
        }
        let cancellation = CancellationToken::linked(&request_token, &lease);
        OperationProgress::Continue(PreparedWorkspaceOperation(Box::new(PreparedState {
            store: snapshot,
            phase: PreparedPhase::AssetRefresh { outcome, kind },
            context: CommandContext {
                workspace_uri: Some(workspace),
                document: None,
            },
            cancellation,
            request_token,
            lease: Some(lease),
            requested: None,
            captures,
            resources: self.resources.clone(),
        })))
    }
    pub fn execute(
        &mut self,
        command: WorkspaceCommand,
        context: CommandContext,
        cancellation: &CancellationToken,
    ) -> Result<CommandOutcome, ApplicationError> {
        let mut prepared = self.prepare(command, context, cancellation)?;
        loop {
            match self.complete(prepared.execute())? {
                OperationProgress::Complete(outcome) => return Ok(outcome),
                OperationProgress::Continue(next) => prepared = next,
            }
        }
    }
}
