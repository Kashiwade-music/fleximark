//! Role: Own native document, workspace, and preview services.
mod document;
mod outcomes;
mod registry;
pub use registry::SessionRegistry;
mod error;
mod index;
pub mod native;
pub mod operations;
mod positions;
mod preview_state;
#[cfg(test)]
mod preview_tests;
mod requests;
mod snapshots;
pub use document::DocumentSession;
#[cfg(test)]
pub use positions::content_hash;
use positions::position_offset;
pub use requests::*;
pub use snapshots::{PreviewSessionId, RenderFrame};
mod workspace;
mod workspace_config;
pub use workspace_config::WorkspaceConfigurationStatus;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fleximark_engine::{DocumentEngine as EngineSession, RenderConfig, ResolvedAssets};
use fleximark_model::{DocumentUri, NavigationEntry, NodeId, PositionEncoding};
use fleximark_plugin_api::{CancellationToken, PluginExecutor};

pub use error::SessionError;
use error::engine_error;
pub use fleximark_language::{
    CompletionItem, SEMANTIC_TOKEN_TYPES, completion_items, semantic_tokens,
};
use index::SessionIndex;
use workspace::WorkspaceAuthority;

static NEXT_DAEMON: AtomicU64 = AtomicU64::new(1);

impl SessionRegistry {
    pub fn new(position_encoding: PositionEncoding) -> Self {
        let serial = NEXT_DAEMON.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seed = format!("{}:{nanos}:{serial}", std::process::id());
        let digest = blake3::hash(seed.as_bytes()).to_hex().to_string();
        Self {
            config_generation: 0,
            daemon_instance_id: format!("daemon-{}", &digest[..24]),
            position_encoding,
            index: SessionIndex::default(),
            workspaces: WorkspaceAuthority::default(),
        }
    }

    pub fn daemon_instance_id(&self) -> &str {
        &self.daemon_instance_id
    }

    pub fn set_position_encoding(&mut self, encoding: PositionEncoding) {
        if self.index.is_empty() {
            self.position_encoding = encoding;
        }
    }

    pub fn configure_workspaces(
        &mut self,
        workspaces: Vec<(String, Arc<dyn PluginExecutor>, RenderConfig)>,
    ) -> Result<(), SessionError> {
        if !self.index.is_empty() {
            return Err(SessionError::Engine(
                "workspaces must be configured before opening documents".into(),
            ));
        }
        self.workspaces.replace_roots(workspaces)?;
        Ok(())
    }

    pub fn workspace_statuses(&self) -> Vec<(String, bool, bool)> {
        self.workspaces.status()
    }
    pub fn set_workspace_status(&mut self, uri: &str, trusted: bool, enabled: bool) {
        self.workspaces.set_status(uri, trusted, enabled);
    }
    pub fn workspace_grants_write(&self, uri: &str) -> bool {
        self.workspaces.grants_write(uri)
    }
    pub fn workspace_for_document(&self, document: &str) -> Option<(String, bool)> {
        self.workspaces.document_workspace(document)
    }
    pub fn refresh_document_assets(&mut self, uri: &str) -> Result<(), SessionError> {
        let id = self
            .session_id_for_uri(uri)
            .ok_or(SessionError::UnknownSession)?
            .to_owned();
        let version = self.current_version(self.daemon_instance_id(), &id)?;
        let document = self.document(self.daemon_instance_id(), &id, version)?;
        let Some(workspace) = document.workspace_uri().map(str::to_owned) else {
            return Ok(());
        };
        if !self.workspace_grants_write(&workspace) {
            return Ok(());
        }
        let session = self.index.by_uri_mut(uri).ok_or(SessionError::NotOpen)?;
        native::refresh_document_assets(&mut session.engine, &workspace)
            .map_err(|error| SessionError::Engine(error.to_string()))
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

    pub fn reconfigure_workspace(
        &mut self,
        workspace_uri: &str,
        host: Arc<dyn PluginExecutor>,
        render_config: RenderConfig,
        mut assets: HashMap<String, ResolvedAssets>,
        cancellation: &CancellationToken,
    ) -> Result<(), SessionError> {
        let workspace_uri = workspace_uri.trim_end_matches('/');
        let host = host;
        let mut replacements = Vec::new();
        for (uri, session) in self.index.iter() {
            if session.workspace_uri.as_deref() == Some(workspace_uri) {
                let config = render_config
                    .clone()
                    .with_resolved_assets(assets.remove(uri).unwrap_or_default())
                    .map_err(engine_error)?;
                let engine = EngineSession::open_configured(
                    DocumentUri(uri.to_owned()),
                    session.engine.document().document_version,
                    session.engine.source().to_owned(),
                    session.engine.position_encoding(),
                    config,
                    Arc::clone(&host),
                    cancellation,
                )
                .map_err(engine_error)?;
                replacements.push((uri.to_owned(), engine));
            }
        }
        self.workspaces.replace(workspace_uri, host, render_config);
        for (uri, engine) in replacements {
            self.index
                .by_uri_mut(&uri)
                .ok_or(SessionError::NotOpen)?
                .engine
                .adopt_reconfiguration(engine)
                .map_err(engine_error)?;
        }
        Ok(())
    }

    pub fn open_with_cancellation(
        &mut self,
        params: DidOpenParams,
        cancellation: &CancellationToken,
    ) -> Result<(), SessionError> {
        let item = params.text_document;
        if self
            .workspace_for_document(&item.uri)
            .is_some_and(|(uri, _)| {
                self.workspace_statuses()
                    .iter()
                    .any(|(root, _, enabled)| root == &uri && !enabled)
            })
        {
            return Err(SessionError::Engine(
                "workspace configuration is disabled".into(),
            ));
        }
        let version = u64::try_from(item.version).map_err(|_| SessionError::StaleVersion)?;
        let workspace = self
            .workspaces
            .matching(&item.uri)
            .map(|(workspace_uri, host, config)| {
                (workspace_uri.to_owned(), Arc::clone(host), config.clone())
            });
        let workspace_uri = self.workspace_for_document(&item.uri).map(|(uri, _)| uri);
        let engine = match workspace {
            Some((_, host, config)) => EngineSession::open_configured(
                DocumentUri(item.uri.clone()),
                version,
                item.text.clone(),
                self.position_encoding,
                config,
                host,
                cancellation,
            )
            .map_err(engine_error),
            None => EngineSession::open(
                DocumentUri(item.uri.clone()),
                version,
                item.text.clone(),
                self.position_encoding,
            )
            .map_err(engine_error),
        }?;
        let id = format!("document-{}", NEXT_DAEMON.fetch_add(1, Ordering::Relaxed));
        self.index.insert(DocumentSession {
            id,
            uri: item.uri,
            workspace_uri,
            engine,
            previews: HashMap::new(),
        });
        Ok(())
    }

    pub fn change_with_cancellation(
        &mut self,
        params: DidChangeParams,
        cancellation: &CancellationToken,
    ) -> Result<(), SessionError> {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let current = self.index.by_uri(&uri).ok_or(SessionError::NotOpen)?;
        let current_version = current.engine.document().document_version;
        let version_u64 = u64::try_from(version).map_err(|_| SessionError::StaleVersion)?;
        let is_full_replacement =
            params.content_changes.len() == 1 && params.content_changes[0].range.is_none();
        // Recovery replies can race with an ordinary change at the same version.
        // An identical full-text replay already represents the authoritative state.
        if !current.engine.is_out_of_sync()
            && is_full_replacement
            && version_u64 == current_version
            && params.content_changes[0].text == current.engine.source()
        {
            return Ok(());
        }
        let same_version_recovery = current.engine.is_out_of_sync()
            && is_full_replacement
            && version_u64 == current_version;
        if version_u64 <= current_version && !same_version_recovery {
            self.mark_out_of_sync(&uri, "stale document version");
            return Err(SessionError::StaleVersion);
        }
        if params.content_changes.is_empty() {
            self.mark_out_of_sync(&uri, "empty change notification");
            return Err(SessionError::EmptyChange);
        }

        if current.engine.is_out_of_sync() && !is_full_replacement {
            return Err(SessionError::ContentModified);
        }

        if !is_full_replacement {
            return Err(SessionError::InvalidRange);
        }
        let text = params.content_changes[0].text.clone();

        let session = self.index.by_uri_mut(&uri).ok_or(SessionError::NotOpen)?;
        let result = if session.engine.is_out_of_sync() {
            session
                .engine
                .resynchronize_with_cancellation(version_u64, text.clone(), cancellation)
        } else {
            session.engine.change_full_text_with_cancellation(
                version_u64,
                text.clone(),
                cancellation,
            )
        };
        result.map_err(engine_error)?;
        Ok(())
    }

    pub fn close(&mut self, params: DidCloseParams) {
        self.index.remove_by_uri(&params.text_document.uri);
    }

    pub fn attach(
        &self,
        params: &DocumentAttachmentRequest,
    ) -> Result<DocumentAttachment, SessionError> {
        let session = self
            .index
            .by_uri(&params.uri)
            .ok_or(SessionError::NotOpen)?;
        self.verify_daemon(&params.daemon_instance_id)?;
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        let version = i64::try_from(session.engine.document().document_version)
            .map_err(|_| SessionError::VersionMismatch)?;
        if params.expected_document_version != version {
            return Err(SessionError::VersionMismatch);
        }
        Ok(DocumentAttachment {
            document_session_id: session.id.clone(),
            document_version: version,
        })
    }

    pub fn document(
        &self,
        daemon: &str,
        session_id: &str,
        version: i64,
    ) -> Result<&DocumentSession, SessionError> {
        self.verify_daemon(daemon)?;
        let session = self
            .index
            .by_session(session_id)
            .ok_or(SessionError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        if i64::try_from(session.engine.document().document_version).ok() != Some(version) {
            return Err(SessionError::VersionMismatch);
        }
        Ok(session)
    }

    pub fn navigation_at_position(
        &self,
        daemon: &str,
        session_id: &str,
        version: i64,
        position: &Position,
    ) -> Result<Option<NavigationEntry>, SessionError> {
        let session = self.document(daemon, session_id, version)?;
        let offset = position_offset(session.source(), *position, session.position_encoding())
            .ok_or(SessionError::InvalidRange)?;
        Ok(session.node_at_source_offset(offset as u64))
    }

    pub fn navigate_preview(
        &self,
        daemon: &str,
        session_id: &str,
        preview_id: &str,
        revision: u64,
        node: &NodeId,
    ) -> Result<Option<NavigationEntry>, SessionError> {
        self.verify_daemon(daemon)?;
        self.index
            .by_session(session_id)
            .ok_or(SessionError::UnknownSession)?
            .navigate(preview_id, revision, node)
    }
    pub fn render_with_cancellation(
        &mut self,
        daemon: &str,
        session_id: &str,
        version: i64,
        preview_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<RenderFrame, SessionError> {
        self.verify_daemon(daemon)?;
        let session = self
            .index
            .by_session_mut(session_id)
            .ok_or(SessionError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        if i64::try_from(session.document().document_version).ok() != Some(version) {
            return Err(SessionError::VersionMismatch);
        }
        session.publish(preview_id, cancellation)
    }
    pub fn read_preview(
        &self,
        daemon: &str,
        session_id: &str,
        preview_id: &str,
        after_revision: Option<u64>,
    ) -> Result<Option<RenderFrame>, SessionError> {
        self.verify_daemon(daemon)?;
        let session = self
            .index
            .by_session(session_id)
            .ok_or(SessionError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        Ok(session
            .previews
            .get(preview_id)
            .filter(|frame| after_revision.is_none_or(|after| frame.render_revision > after))
            .cloned())
    }
    pub fn dispose_preview(
        &mut self,
        daemon: &str,
        session_id: &str,
        preview_id: &str,
    ) -> Result<(), SessionError> {
        self.verify_daemon(daemon)?;
        self.index
            .by_session_mut(session_id)
            .ok_or(SessionError::UnknownSession)?
            .previews
            .remove(preview_id);
        Ok(())
    }

    pub fn current_version(&self, daemon: &str, session_id: &str) -> Result<i64, SessionError> {
        self.verify_daemon(daemon)?;
        let session = self
            .index
            .by_session(session_id)
            .ok_or(SessionError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        i64::try_from(session.engine.document().document_version)
            .map_err(|_| SessionError::VersionMismatch)
    }

    pub fn session_id_for_uri(&self, uri: &str) -> Option<&str> {
        self.index.session_id_for_uri(uri)
    }

    pub fn source_for_uri(&self, uri: &str) -> Result<&str, SessionError> {
        self.index
            .by_uri(uri)
            .map(DocumentSession::source)
            .ok_or(SessionError::NotOpen)
    }

    pub fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }

    pub fn line_prefix(&self, uri: &str, position: Position) -> Result<&str, SessionError> {
        let session = self.index.by_uri(uri).ok_or(SessionError::NotOpen)?;
        let source = session.engine.source();
        let offset = position_offset(source, position, self.position_encoding)
            .ok_or(SessionError::InvalidRange)?;
        let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
        Ok(&source[line_start..offset])
    }

    pub(crate) fn verify_daemon(&self, daemon: &str) -> Result<(), SessionError> {
        if daemon == self.daemon_instance_id {
            Ok(())
        } else {
            Err(SessionError::WrongDaemon)
        }
    }

    fn mark_out_of_sync(&mut self, uri: &str, _reason: &str) {
        if let Some(session) = self.index.by_uri_mut(uri) {
            session.engine.mark_out_of_sync();
        }
    }
}

#[cfg(test)]
mod test_plugins;
#[cfg(test)]
mod tests;
