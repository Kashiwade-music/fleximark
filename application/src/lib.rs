//! Role: Own native document, workspace, and preview services.
mod authority;
mod cancellation;
mod cancellation_checks;
mod document;
pub use cancellation::CancellationToken;
mod outcomes;
mod registry;
pub use registry::DocumentStore;
mod error;
mod index;
pub mod logging;
pub(crate) mod native;
mod operations;
mod workspace_worker;
pub use workspace_worker::{OperationCompletion, OperationProgress, PreparedWorkspaceOperation};
#[cfg(test)]
mod continuation_tests;
#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod operations_tests;
pub use operations::{CommandOutcome, MigrationAppliedOutcome, WorkspaceOps};
pub use outcomes::{
    CollectedAdmonitions, CreatedNote, ExportReceipt, InitializedWorkspace, NoteCategory,
    NoteOptions, ThemeResource,
};
mod preview_state;
#[cfg(test)]
mod preview_tests;
mod requests;
mod snapshots;
pub use document::DocumentSession;
#[cfg(test)]
fn content_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}
pub use requests::*;
pub use snapshots::{PreviewHandle, PreviewSessionId, RenderFrame};
mod workspace;
mod workspace_config;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use fleximark_engine::{DocumentEngine as EngineSession, RenderConfig, ResolvedAssets};
use fleximark_model::{DocumentUri, NavigationEntry, NodeId, PositionEncoding};

use error::engine_error;
pub use error::{ApplicationError, ErrorKind};
pub use fleximark_language::{CompletionItem, completion_items, semantic_tokens};
use index::SessionIndex;
pub use workspace::{WorkspaceAuthority, WorkspaceMembership, WorkspaceState, WorkspaceStatus};

static NEXT_DOCUMENT: AtomicU64 = AtomicU64::new(1);

impl DocumentStore {
    pub fn new(position_encoding: PositionEncoding) -> Self {
        Self {
            config_generation: 0,
            position_encoding,
            index: SessionIndex::default(),
            workspaces: WorkspaceAuthority::default(),
        }
    }

    pub fn set_position_encoding(&mut self, encoding: PositionEncoding) {
        if self.index.is_empty() {
            self.position_encoding = encoding;
        }
    }

    pub(crate) fn configure_workspaces(
        &mut self,
        workspaces: Vec<(String, RenderConfig)>,
    ) -> Result<(), ApplicationError> {
        if !self.index.is_empty() {
            return Err(ApplicationError::InvalidState(
                "workspaces must be configured before opening documents".into(),
            ));
        }
        self.workspaces.replace_roots(workspaces)?;
        Ok(())
    }

    pub fn workspace_statuses(&self) -> Vec<WorkspaceStatus> {
        self.workspaces.statuses()
    }
    pub fn workspace_for_document(&self, document: &str) -> Option<WorkspaceMembership> {
        self.workspaces.document_workspace(document)
    }
    pub fn refresh_document_assets(&mut self, uri: &str) -> Result<(), ApplicationError> {
        let id = self
            .session_id_for_uri(uri)
            .ok_or(ApplicationError::UnknownSession)?
            .to_owned();
        let version = self.current_version(&id)?;
        let document = self.document(&id, version)?;
        let Some(workspace) = document.workspace_uri().map(str::to_owned) else {
            return Ok(());
        };
        if !self.workspaces.is_trusted(&workspace) {
            return Ok(());
        }
        let session = self
            .index
            .by_uri_mut(uri)
            .ok_or(ApplicationError::NotOpen)?;
        let config = self.workspaces.cached_config(&workspace)?;
        native::refresh_document_assets(
            &mut session.engine,
            &workspace,
            &config,
            &mut session.asset_memo,
        )
    }
    pub fn open_documents(&self) -> Vec<(String, fleximark_model::Document)> {
        self.index
            .iter()
            .map(|(uri, session)| (uri.to_owned(), session.document().clone()))
            .collect()
    }

    pub fn workspace_documents(
        &self,
        workspace_uri: &str,
    ) -> Vec<(String, fleximark_model::Document)> {
        self.index
            .iter()
            .filter(|(_, session)| session.workspace_uri.as_deref() == Some(workspace_uri))
            .map(|(uri, session)| (uri.to_owned(), session.document().clone()))
            .collect()
    }

    pub(crate) fn reconfigure_workspace(
        &mut self,
        workspace_uri: &str,
        render_config: RenderConfig,
        mut assets: HashMap<String, ResolvedAssets>,
        cancellation: &CancellationToken,
    ) -> Result<(), ApplicationError> {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::Cancelled);
        }
        let workspace_uri = workspace_uri.trim_end_matches('/');
        let mut replacements = Vec::new();
        for (uri, session) in self.index.iter() {
            if session.workspace_uri.as_deref() == Some(workspace_uri) {
                let config = render_config
                    .clone()
                    .with_resolved_assets(assets.remove(uri).unwrap_or_default())
                    .map_err(engine_error)?;
                let mut engine = session.engine.clone();
                engine.set_render_config(config).map_err(engine_error)?;
                replacements.push((uri.to_owned(), engine));
            }
        }
        #[cfg(test)]
        cancellation_checks::checkpoint("configuration_prepared");
        cancellation_checks::check(cancellation, "reconfigure_commit")?;
        for (uri, _) in &replacements {
            self.index
                .by_uri(uri)
                .ok_or(ApplicationError::NotOpen)?
                .authority
                .check_advance()?;
        }
        self.workspaces.replace(workspace_uri, render_config);
        for (uri, engine) in replacements {
            let session = self
                .index
                .by_uri_mut(&uri)
                .ok_or(ApplicationError::NotOpen)?;
            session
                .authority
                .commit(engine.document().document_version)?;
            session.asset_memo.clear();
            session.engine = engine;
        }
        Ok(())
    }

    pub fn open(
        &mut self,
        item: DocumentUpdate,
        cancellation: &CancellationToken,
    ) -> Result<(), ApplicationError> {
        cancellation_checks::check(cancellation, "open_start")?;
        if self
            .workspace_for_document(&item.uri)
            .is_some_and(|membership| {
                self.workspace_statuses().iter().any(|status| {
                    status.uri == membership.uri
                        && matches!(status.state, WorkspaceState::Invalid(_))
                })
            })
        {
            return Err(ApplicationError::InvalidState(
                "workspace configuration is disabled".into(),
            ));
        }
        let version = item.version;
        let workspace = self
            .workspaces
            .matching(&item.uri)
            .map(|(workspace_uri, config)| (workspace_uri.to_owned(), config.clone()));
        let workspace_uri = self
            .workspace_for_document(&item.uri)
            .map(|membership| membership.uri);
        let config = workspace.map(|(_, config)| config).unwrap_or_default();
        let engine = EngineSession::open(
            DocumentUri(item.uri.clone()),
            version,
            item.source.clone(),
            self.position_encoding,
            config,
        )
        .map_err(engine_error)?;
        cancellation_checks::check(cancellation, "open_commit")?;
        let id = format!("document-{}", NEXT_DOCUMENT.fetch_add(1, Ordering::Relaxed));
        if let Some(previous) = self.index.by_uri(&item.uri) {
            previous.authority.close();
        }
        self.index.insert(DocumentSession {
            asset_memo: Default::default(),
            authority: std::sync::Arc::new(authority::DocumentAuthority::new(version)),
            id,
            uri: item.uri,
            workspace_uri,
            engine,
            previews: HashMap::new(),
        });
        Ok(())
    }

    pub fn change(
        &mut self,
        update: DocumentUpdate,
        cancellation: &CancellationToken,
    ) -> Result<(), ApplicationError> {
        if cancellation.is_cancelled() {
            self.mark_out_of_sync(&update.uri);
            return Err(ApplicationError::Cancelled);
        }
        let DocumentUpdate {
            uri,
            version: version_u64,
            source: text,
        } = update;
        let current = self.index.by_uri(&uri).ok_or(ApplicationError::NotOpen)?;
        let current_version = current.engine.document().document_version;
        if !current.engine.is_out_of_sync()
            && version_u64 == current_version
            && text == current.engine.source()
        {
            return Ok(());
        }
        let recovery = current.engine.is_out_of_sync() && version_u64 == current_version;
        if version_u64 <= current_version && !recovery {
            self.mark_out_of_sync(&uri);
            return Err(ApplicationError::StaleVersion);
        }
        let session = self
            .index
            .by_uri_mut(&uri)
            .ok_or(ApplicationError::NotOpen)?;
        let mut candidate = session.engine.clone();
        let result = if candidate.is_out_of_sync() {
            candidate.resync(version_u64, text)
        } else {
            candidate.change(version_u64, text)
        };
        #[cfg(test)]
        cancellation_checks::checkpoint("change_candidate");
        if cancellation.is_cancelled() {
            session.engine.mark_out_of_sync();
            session.authority.invalidate();
            return Err(ApplicationError::Cancelled);
        }
        if let Err(error) = result {
            session.engine.mark_out_of_sync();
            session.authority.invalidate();
            return Err(engine_error(error));
        }
        if let Err(error) = cancellation_checks::check(cancellation, "change_commit") {
            session.engine.mark_out_of_sync();
            session.authority.invalidate();
            return Err(error);
        }
        session.authority.commit(version_u64)?;
        session.engine = candidate;
        Ok(())
    }

    pub fn close(&mut self, uri: &str) {
        if let Some(session) = self.index.by_uri(uri) {
            session.authority.close();
        }
        self.index.remove_by_uri(uri);
    }
    pub fn attach(
        &self,
        uri: &str,
        expected_version: u64,
    ) -> Result<DocumentAttachment, ApplicationError> {
        let session = self.index.by_uri(uri).ok_or(ApplicationError::NotOpen)?;
        if session.engine.is_out_of_sync() {
            return Err(ApplicationError::ContentModified);
        }
        let version = session.document().document_version;
        if expected_version != version {
            return Err(ApplicationError::VersionMismatch);
        }
        Ok(DocumentAttachment {
            document_session_id: session.id.clone(),
            document_version: version,
        })
    }

    pub fn document(
        &self,
        session_id: &str,
        version: u64,
    ) -> Result<&DocumentSession, ApplicationError> {
        let session = self
            .index
            .by_session(session_id)
            .ok_or(ApplicationError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(ApplicationError::ContentModified);
        }
        if session.engine.document().document_version != version {
            return Err(ApplicationError::VersionMismatch);
        }
        Ok(session)
    }

    pub fn navigation_at_source_offset(
        &self,
        session_id: &str,
        version: u64,
        offset: u64,
    ) -> Result<Option<NavigationEntry>, ApplicationError> {
        let session = self.document(session_id, version)?;
        let offset_native = usize::try_from(offset).map_err(|_| ApplicationError::InvalidRange)?;
        if !session.source().is_char_boundary(offset_native) {
            return Err(ApplicationError::InvalidRange);
        }
        Ok(session.node_at_source_offset(offset))
    }
    pub fn navigate_preview(
        &self,
        session_id: &str,
        preview_id: &str,
        revision: u64,
        node: &NodeId,
    ) -> Result<Option<NavigationEntry>, ApplicationError> {
        self.index
            .by_session(session_id)
            .ok_or(ApplicationError::UnknownSession)?
            .navigate(preview_id, revision, node)
    }
    pub fn render_with_cancellation(
        &mut self,
        session_id: &str,
        version: u64,
        preview_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<PreviewHandle, ApplicationError> {
        let session = self
            .index
            .by_session_mut(session_id)
            .ok_or(ApplicationError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(ApplicationError::ContentModified);
        }
        if session.document().document_version != version {
            return Err(ApplicationError::VersionMismatch);
        }
        session.publish(preview_id, cancellation)
    }
    pub fn read_preview(
        &self,
        session_id: &str,
        preview_id: &str,
        after_revision: Option<u64>,
    ) -> Result<Option<PreviewHandle>, ApplicationError> {
        let session = self
            .index
            .by_session(session_id)
            .ok_or(ApplicationError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(ApplicationError::ContentModified);
        }
        Ok(session
            .previews
            .get(preview_id)
            .filter(|frame| after_revision.is_none_or(|after| frame.render_revision > after))
            .cloned())
    }
    pub fn dispose_preview(
        &mut self,
        session_id: &str,
        preview_id: &str,
    ) -> Result<(), ApplicationError> {
        if let Some(previous) = self
            .index
            .by_session_mut(session_id)
            .ok_or(ApplicationError::UnknownSession)?
            .previews
            .remove(preview_id)
        {
            previous.invalidate_publication();
        }
        Ok(())
    }

    pub fn current_version(&self, session_id: &str) -> Result<u64, ApplicationError> {
        let session = self
            .index
            .by_session(session_id)
            .ok_or(ApplicationError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(ApplicationError::ContentModified);
        }
        Ok(session.engine.document().document_version)
    }

    pub fn session_id_for_uri(&self, uri: &str) -> Option<&str> {
        self.index.session_id_for_uri(uri)
    }

    pub fn source_for_uri(&self, uri: &str) -> Result<&str, ApplicationError> {
        self.index
            .by_uri(uri)
            .map(DocumentSession::source)
            .ok_or(ApplicationError::NotOpen)
    }

    pub fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }

    pub fn line_prefix(&self, uri: &str, offset: usize) -> Result<&str, ApplicationError> {
        let source = self.source_for_uri(uri)?;
        if !source.is_char_boundary(offset) {
            return Err(ApplicationError::InvalidRange);
        }
        let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
        Ok(&source[line_start..offset])
    }
    pub fn mark_out_of_sync(&mut self, uri: &str) {
        if let Some(session) = self.index.by_uri_mut(uri) {
            session.engine.mark_out_of_sync();
            session.authority.invalidate();
        }
    }
    pub fn add_workspace(&mut self, uri: String) -> Result<WorkspaceStatus, ApplicationError> {
        let existing = self.workspace_statuses();
        let status = self.workspaces.add_restricted(uri)?;
        if existing.iter().any(|old| old.uri == status.uri) {
            return Ok(status);
        }
        let attach = self
            .index
            .iter()
            .filter_map(|(uri, session)| {
                self.workspaces
                    .membership(uri)
                    .filter(|membership| {
                        membership.uri == status.uri
                            && session.workspace_uri.as_deref() != Some(status.uri.as_str())
                    })
                    .map(|_| uri.to_owned())
            })
            .collect::<Vec<_>>();
        for uri in &attach {
            if let Err(error) = self
                .index
                .by_uri(uri)
                .ok_or(ApplicationError::NotOpen)?
                .authority
                .check_advance()
            {
                self.workspaces.remove_root(&status.uri);
                return Err(error);
            }
        }
        for uri in attach {
            let session = self
                .index
                .by_uri_mut(&uri)
                .ok_or(ApplicationError::NotOpen)?;
            session.engine.set_render_config(Default::default())?;
            session.asset_memo.clear();
            session.workspace_uri = Some(status.uri.clone());
            session
                .authority
                .commit(session.document().document_version)?;
        }
        Ok(status)
    }
    pub fn remove_workspace(&mut self, uri: &str) -> Result<(), ApplicationError> {
        let uri = uri.trim_end_matches('/');
        let detach = self
            .index
            .iter()
            .filter(|(_, session)| session.workspace_uri.as_deref() == Some(uri))
            .map(|(uri, _)| uri.to_owned())
            .collect::<Vec<_>>();
        for uri in &detach {
            self.index
                .by_uri(uri)
                .ok_or(ApplicationError::NotOpen)?
                .authority
                .check_advance()?;
        }
        self.workspaces.remove_root(uri);
        for uri in detach {
            let session = self
                .index
                .by_uri_mut(&uri)
                .ok_or(ApplicationError::NotOpen)?;
            session.engine.set_render_config(Default::default())?;
            session.asset_memo.clear();
            session.workspace_uri = None;
            session
                .authority
                .commit(session.document().document_version)?;
        }
        Ok(())
    }
    pub fn authority(&self) -> &WorkspaceAuthority {
        &self.workspaces
    }
    pub fn authority_mut(&mut self) -> &mut WorkspaceAuthority {
        &mut self.workspaces
    }
}

#[cfg(test)]
mod tests;
