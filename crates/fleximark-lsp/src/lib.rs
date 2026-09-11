use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fleximark_engine::{
    DocumentSession as EngineSession, EngineError, PreviewSessionId, RenderAsset, RenderConfig,
    RenderPublication, RenderStyle, ResolvedRenderAsset,
};
use fleximark_model::{DocumentUri, NavigationEntry, NodeId, PositionEncoding};
use fleximark_plugin_host::{CancellationToken, PluginHost, UnsafeExportOutput};
use fleximark_protocol::{
    AttachDocumentParams, AttachDocumentResult, CheckpointDocumentParams, CheckpointDocumentResult,
    RequestFullTextParams, RpcChangeDocumentParams, RpcCloseDocumentParams, RpcOpenDocumentParams,
    TextPosition,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

static NEXT_DAEMON: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenParams {
    pub text_document: TextDocumentItem,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TextDocumentItem {
    pub uri: String,
    pub version: i64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeParams {
    pub text_document: VersionedTextDocumentIdentifier,
    pub content_changes: Vec<ContentChange>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VersionedTextDocumentIdentifier {
    pub uri: String,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ContentChange {
    pub range: Option<Range>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidCloseParams {
    pub text_document: TextDocumentIdentifier,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

pub struct DocumentSession {
    pub id: String,
    pub uri: String,
    pub content_hash: String,
    workspace_uri: Option<String>,
    engine: EngineSession,
}

impl DocumentSession {
    pub fn document(&self) -> &fleximark_model::Document {
        self.engine.document()
    }

    pub fn source(&self) -> &str {
        self.engine.source()
    }

    pub fn node_at_source_offset(&self, byte_offset: u64) -> Option<NavigationEntry> {
        self.engine.node_at_source_offset(byte_offset)
    }

    pub fn source_range_for_node(&self, node_id: &NodeId) -> Option<NavigationEntry> {
        self.engine.source_range_for_node(node_id)
    }

    pub fn position_encoding(&self) -> PositionEncoding {
        self.engine.position_encoding()
    }

    pub fn workspace_uri(&self) -> Option<&str> {
        self.workspace_uri.as_deref()
    }

    pub fn asset_diagnostics(&self) -> &[fleximark_engine::AssetDiagnostic] {
        self.engine.asset_diagnostics()
    }

    pub fn plugin_diagnostics(&self) -> &[fleximark_plugin_host::PluginDiagnostic] {
        self.engine.plugin_diagnostics()
    }
}

pub struct SessionRegistry {
    daemon_instance_id: String,
    position_encoding: PositionEncoding,
    documents: HashMap<String, DocumentSession>,
    session_uris: HashMap<String, String>,
    events: Vec<RequestFullTextParams>,
    plugin_host: Option<Arc<PluginHost>>,
    render_config: Option<RenderConfig>,
    workspace_configs: Vec<(String, Arc<PluginHost>, RenderConfig)>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("document is not open")]
    NotOpen,
    #[error("document session is unknown or no longer active")]
    UnknownSession,
    #[error("daemon instance does not match this connection")]
    WrongDaemon,
    #[error("document is out of sync")]
    ContentModified,
    #[error("document version is stale")]
    StaleVersion,
    #[error("document version does not match")]
    VersionMismatch,
    #[error("content hash does not match")]
    HashMismatch,
    #[error("incremental edit range is invalid")]
    InvalidRange,
    #[error("a change notification must contain edits")]
    EmptyChange,
    #[error("engine rejected the document: {0}")]
    Engine(String),
}

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
            daemon_instance_id: format!("daemon-{}", &digest[..24]),
            position_encoding,
            documents: HashMap::new(),
            session_uris: HashMap::new(),
            events: Vec::new(),
            plugin_host: None,
            render_config: None,
            workspace_configs: Vec::new(),
        }
    }

    pub fn daemon_instance_id(&self) -> &str {
        &self.daemon_instance_id
    }

    pub fn set_position_encoding(&mut self, encoding: PositionEncoding) {
        if self.documents.is_empty() {
            self.position_encoding = encoding;
        }
    }

    pub fn configure_plugins(
        &mut self,
        host: Option<PluginHost>,
        render_config: Option<RenderConfig>,
    ) -> Result<(), SessionError> {
        if !self.documents.is_empty() {
            return Err(SessionError::Engine(
                "plugins must be configured before opening documents".into(),
            ));
        }
        self.plugin_host = host.map(Arc::new);
        self.render_config = render_config;
        Ok(())
    }

    pub fn configure_workspaces(
        &mut self,
        workspaces: Vec<(String, PluginHost, RenderConfig)>,
    ) -> Result<(), SessionError> {
        if !self.documents.is_empty() {
            return Err(SessionError::Engine(
                "workspaces must be configured before opening documents".into(),
            ));
        }
        self.workspace_configs = workspaces
            .into_iter()
            .map(|(uri, host, config)| {
                (uri.trim_end_matches('/').to_owned(), Arc::new(host), config)
            })
            .collect();
        self.workspace_configs
            .sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
        Ok(())
    }

    pub fn reconfigure_document_assets(
        &mut self,
        uri: &str,
        assets: Vec<ResolvedRenderAsset>,
    ) -> Result<(), SessionError> {
        let workspace_uri = self
            .documents
            .get(uri)
            .ok_or(SessionError::NotOpen)?
            .workspace_uri
            .clone();
        let config = workspace_uri
            .as_deref()
            .and_then(|workspace_uri| self.workspace_config_exact(workspace_uri))
            .map(|(_, config)| config.clone())
            .or_else(|| self.render_config.clone())
            .ok_or_else(|| SessionError::Engine("render configuration is missing".into()))?
            .with_resolved_assets(assets)
            .map_err(engine_error)?;
        self.documents
            .get_mut(uri)
            .ok_or(SessionError::NotOpen)?
            .engine
            .reconfigure_render(config)
            .map_err(engine_error)
    }

    pub fn open_documents(&self) -> Vec<(String, fleximark_model::Document)> {
        self.documents
            .iter()
            .map(|(uri, session)| (uri.clone(), session.document().clone()))
            .collect()
    }

    pub fn workspace_documents(
        &self,
        workspace_uri: &str,
    ) -> Vec<(String, fleximark_model::Document)> {
        self.documents
            .iter()
            .filter(|(_, session)| session.workspace_uri.as_deref() == Some(workspace_uri))
            .map(|(uri, session)| (uri.clone(), session.document().clone()))
            .collect()
    }

    pub fn reconfigure_workspace(
        &mut self,
        workspace_uri: &str,
        host: PluginHost,
        render_config: RenderConfig,
        mut assets: HashMap<String, Vec<ResolvedRenderAsset>>,
    ) -> Result<(), SessionError> {
        let workspace_uri = workspace_uri.trim_end_matches('/');
        let host = Arc::new(host);
        let mut replacements = Vec::new();
        for (uri, session) in &self.documents {
            if session.workspace_uri.as_deref() == Some(workspace_uri) {
                let config = render_config
                    .clone()
                    .with_resolved_assets(assets.remove(uri).unwrap_or_default())
                    .map_err(engine_error)?;
                let engine = EngineSession::open_configured(
                    DocumentUri(uri.clone()),
                    session.engine.document().document_version,
                    session.engine.source().to_owned(),
                    session.engine.position_encoding(),
                    config,
                    Arc::clone(&host),
                    &CancellationToken::default(),
                )
                .map_err(engine_error)?;
                replacements.push((uri.clone(), engine));
            }
        }
        if let Some(entry) = self
            .workspace_configs
            .iter_mut()
            .find(|(uri, _, _)| uri == workspace_uri)
        {
            entry.1 = host;
            entry.2 = render_config;
        } else {
            self.workspace_configs
                .push((workspace_uri.to_owned(), host, render_config));
            self.workspace_configs
                .sort_by_key(|entry| std::cmp::Reverse(entry.0.len()));
        }
        for (uri, engine) in replacements {
            self.documents
                .get_mut(&uri)
                .expect("staged session remains open")
                .engine
                .adopt_reconfiguration(engine)
                .map_err(engine_error)?;
        }
        Ok(())
    }

    pub fn open(&mut self, params: DidOpenParams) -> Result<(), SessionError> {
        let item = params.text_document;
        let version = u64::try_from(item.version).map_err(|_| SessionError::StaleVersion)?;
        let workspace = self
            .workspace_config(&item.uri)
            .map(|(workspace_uri, host, config)| {
                (workspace_uri.to_owned(), Arc::clone(host), config.clone())
            });
        let workspace_uri = workspace.as_ref().map(|(uri, _, _)| uri.clone());
        let engine = match workspace {
            Some((_, host, config)) => EngineSession::open_configured(
                DocumentUri(item.uri.clone()),
                version,
                item.text.clone(),
                self.position_encoding,
                config,
                host,
                &CancellationToken::default(),
            )
            .map_err(engine_error),
            None if self.plugin_host.is_none() => EngineSession::open(
                DocumentUri(item.uri.clone()),
                version,
                item.text.clone(),
                self.position_encoding,
            )
            .map_err(engine_error),
            None => EngineSession::open_configured(
                DocumentUri(item.uri.clone()),
                version,
                item.text.clone(),
                self.position_encoding,
                self.render_config.clone().ok_or_else(|| {
                    SessionError::Engine("plugin host is missing render configuration".into())
                })?,
                Arc::clone(self.plugin_host.as_ref().expect("checked above")),
                &CancellationToken::default(),
            )
            .map_err(engine_error),
        }?;
        if let Some(previous) = self.documents.remove(&item.uri) {
            self.session_uris.remove(&previous.id);
        }
        let id = engine.id().0.clone();
        let content_hash = content_hash(&item.text);
        self.session_uris.insert(id.clone(), item.uri.clone());
        self.documents.insert(
            item.uri.clone(),
            DocumentSession {
                id,
                uri: item.uri,
                content_hash,
                workspace_uri,
                engine,
            },
        );
        Ok(())
    }

    fn workspace_config(
        &self,
        document_uri: &str,
    ) -> Option<(&str, &Arc<PluginHost>, &RenderConfig)> {
        self.workspace_configs
            .iter()
            .find(|(workspace_uri, _, _)| {
                document_uri == workspace_uri
                    || document_uri
                        .strip_prefix(workspace_uri)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
            .map(|(uri, host, config)| (uri.as_str(), host, config))
    }

    fn workspace_config_exact(
        &self,
        workspace_uri: &str,
    ) -> Option<(&Arc<PluginHost>, &RenderConfig)> {
        self.workspace_configs
            .iter()
            .find(|(uri, _, _)| uri == workspace_uri)
            .map(|(_, host, config)| (host, config))
    }

    pub fn change(&mut self, params: DidChangeParams) -> Result<(), SessionError> {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let current = self.documents.get(&uri).ok_or(SessionError::NotOpen)?;
        let current_version = current.engine.document().document_version;
        let version_u64 = u64::try_from(version).map_err(|_| SessionError::StaleVersion)?;
        let is_full_replacement =
            params.content_changes.len() == 1 && params.content_changes[0].range.is_none();
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

        let mut text = current.engine.source().to_owned();
        if is_full_replacement {
            text = params.content_changes[0].text.clone();
        } else {
            for change in params.content_changes {
                let Some(range) = change.range else {
                    self.mark_out_of_sync(&uri, "mixed full and incremental changes");
                    return Err(SessionError::InvalidRange);
                };
                let Some(start) = position_offset(&text, range.start, self.position_encoding)
                else {
                    self.mark_out_of_sync(&uri, "invalid incremental edit range");
                    return Err(SessionError::InvalidRange);
                };
                let Some(end) = position_offset(&text, range.end, self.position_encoding) else {
                    self.mark_out_of_sync(&uri, "invalid incremental edit range");
                    return Err(SessionError::InvalidRange);
                };
                if start > end {
                    self.mark_out_of_sync(&uri, "reversed incremental edit range");
                    return Err(SessionError::InvalidRange);
                }
                text.replace_range(start..end, &change.text);
            }
        }

        let session = self.documents.get_mut(&uri).expect("checked above");
        let result = if session.engine.is_out_of_sync() {
            session.engine.resynchronize(version_u64, text.clone())
        } else {
            session.engine.change_full_text(version_u64, text.clone())
        };
        result.map_err(engine_error)?;
        session.content_hash = content_hash(&text);
        Ok(())
    }

    pub fn close(&mut self, params: DidCloseParams) {
        if let Some(session) = self.documents.remove(&params.text_document.uri) {
            self.session_uris.remove(&session.id);
        }
    }

    pub fn open_rpc(
        &mut self,
        params: RpcOpenDocumentParams,
    ) -> Result<AttachDocumentResult, SessionError> {
        self.verify_daemon(&params.daemon_instance_id)?;
        let hash = content_hash(&params.text);
        self.open(DidOpenParams {
            text_document: TextDocumentItem {
                uri: params.uri.clone(),
                version: params.document_version,
                text: params.text,
            },
        })?;
        self.attach(&AttachDocumentParams {
            daemon_instance_id: params.daemon_instance_id,
            uri: params.uri,
            expected_document_version: params.document_version,
            content_hash: hash,
        })
    }

    pub fn change_rpc(
        &mut self,
        params: RpcChangeDocumentParams,
    ) -> Result<CheckpointDocumentResult, SessionError> {
        self.verify_daemon(&params.daemon_instance_id)?;
        let uri = self
            .session_uris
            .get(&params.document_session_id)
            .cloned()
            .ok_or(SessionError::UnknownSession)?;
        let session = self
            .documents
            .get(&uri)
            .expect("session index is consistent");
        let current_version = i64::try_from(session.engine.document().document_version)
            .map_err(|_| SessionError::VersionMismatch)?;
        if session.engine.is_out_of_sync()
            || current_version != params.base_document_version
            || session.content_hash != params.base_content_hash
        {
            self.mark_out_of_sync(&uri, "standalone RPC change base mismatch");
            return Err(SessionError::ContentModified);
        }
        self.change(DidChangeParams {
            text_document: VersionedTextDocumentIdentifier {
                uri,
                version: params.document_version,
            },
            content_changes: vec![ContentChange {
                range: None,
                text: params.text,
            }],
        })?;
        let session = self
            .documents
            .get(
                self.session_uris
                    .get(&params.document_session_id)
                    .expect("session remains open"),
            )
            .expect("session index is consistent");
        Ok(CheckpointDocumentResult {
            document_version: params.document_version,
            content_hash: session.content_hash.clone(),
        })
    }

    pub fn close_rpc(&mut self, params: RpcCloseDocumentParams) -> Result<(), SessionError> {
        self.verify_daemon(&params.daemon_instance_id)?;
        let uri = self
            .session_uris
            .get(&params.document_session_id)
            .cloned()
            .ok_or(SessionError::UnknownSession)?;
        self.close(DidCloseParams {
            text_document: TextDocumentIdentifier { uri },
        });
        Ok(())
    }

    pub fn attach(
        &self,
        params: &AttachDocumentParams,
    ) -> Result<AttachDocumentResult, SessionError> {
        let session = self
            .documents
            .get(&params.uri)
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
        if params.content_hash != session.content_hash {
            return Err(SessionError::HashMismatch);
        }
        Ok(AttachDocumentResult {
            document_session_id: session.id.clone(),
            document_version: version,
            content_hash: session.content_hash.clone(),
        })
    }

    pub fn checkpoint(
        &mut self,
        params: &CheckpointDocumentParams,
    ) -> Result<CheckpointDocumentResult, SessionError> {
        self.verify_daemon(&params.daemon_instance_id)?;
        let uri = self
            .session_uris
            .get(&params.document_session_id)
            .cloned()
            .ok_or(SessionError::UnknownSession)?;
        let session = self
            .documents
            .get(&uri)
            .expect("session index is consistent");
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        let version = i64::try_from(session.engine.document().document_version)
            .map_err(|_| SessionError::VersionMismatch)?;
        if params.document_version != version {
            self.mark_out_of_sync(&uri, "checkpoint version mismatch");
            return Err(SessionError::VersionMismatch);
        }
        if params.content_hash != session.content_hash {
            self.mark_out_of_sync(&uri, "checkpoint content hash mismatch");
            return Err(SessionError::HashMismatch);
        }
        let session = self.documents.get_mut(&uri).expect("still present");
        session
            .engine
            .checkpoint(
                u64::try_from(params.document_version)
                    .map_err(|_| SessionError::VersionMismatch)?,
                &session.engine.content_hash(),
            )
            .map_err(engine_error)?;
        Ok(CheckpointDocumentResult {
            document_version: version,
            content_hash: session.content_hash.clone(),
        })
    }

    pub fn document(
        &self,
        daemon: &str,
        session_id: &str,
        version: i64,
    ) -> Result<&DocumentSession, SessionError> {
        self.verify_daemon(daemon)?;
        let uri = self
            .session_uris
            .get(session_id)
            .ok_or(SessionError::UnknownSession)?;
        let session = self
            .documents
            .get(uri)
            .expect("session index is consistent");
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
        position: &TextPosition,
    ) -> Result<Option<NavigationEntry>, SessionError> {
        let session = self.document(daemon, session_id, version)?;
        let line = u32::try_from(position.line).map_err(|_| SessionError::InvalidRange)?;
        let character =
            u32::try_from(position.character).map_err(|_| SessionError::InvalidRange)?;
        let offset = position_offset(
            session.source(),
            Position { line, character },
            session.position_encoding(),
        )
        .ok_or(SessionError::InvalidRange)?;
        Ok(session.node_at_source_offset(offset as u64))
    }

    pub fn navigation_for_node(
        &self,
        daemon: &str,
        session_id: &str,
        version: i64,
        node_id: &NodeId,
    ) -> Result<Option<NavigationEntry>, SessionError> {
        Ok(self
            .document(daemon, session_id, version)?
            .source_range_for_node(node_id))
    }

    pub fn render(
        &mut self,
        daemon: &str,
        session_id: &str,
        version: i64,
        preview_id: &str,
    ) -> Result<RenderPublication, SessionError> {
        self.verify_daemon(daemon)?;
        let uri = self
            .session_uris
            .get(session_id)
            .cloned()
            .ok_or(SessionError::UnknownSession)?;
        let session = self
            .documents
            .get_mut(&uri)
            .expect("session index is consistent");
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        if i64::try_from(session.engine.document().document_version).ok() != Some(version) {
            return Err(SessionError::VersionMismatch);
        }
        session
            .engine
            .render_configured(
                PreviewSessionId(preview_id.to_owned()),
                &CancellationToken::default(),
            )
            .map(|result| result.publication)
            .map_err(engine_error)
    }

    pub fn render_full(
        &mut self,
        daemon: &str,
        session_id: &str,
        version: i64,
        preview_id: &str,
    ) -> Result<fleximark_engine::RenderSnapshot, SessionError> {
        self.verify_daemon(daemon)?;
        let uri = self
            .session_uris
            .get(session_id)
            .cloned()
            .ok_or(SessionError::UnknownSession)?;
        let session = self
            .documents
            .get_mut(&uri)
            .expect("session index is consistent");
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        if i64::try_from(session.engine.document().document_version).ok() != Some(version) {
            return Err(SessionError::VersionMismatch);
        }
        let publication = session
            .engine
            .render_full_configured(
                PreviewSessionId(preview_id.to_owned()),
                &CancellationToken::default(),
            )
            .map_err(engine_error)?
            .publication;
        match publication {
            RenderPublication::Full(snapshot) => Ok(snapshot),
            RenderPublication::Patch(_) => unreachable!("full render returned a patch"),
        }
    }

    pub fn dispose_preview(
        &mut self,
        daemon: &str,
        session_id: &str,
        preview_id: &str,
    ) -> Result<(), SessionError> {
        self.verify_daemon(daemon)?;
        let uri = self
            .session_uris
            .get(session_id)
            .cloned()
            .ok_or(SessionError::UnknownSession)?;
        self.documents
            .get_mut(&uri)
            .expect("session index is consistent")
            .engine
            .dispose_preview(&PreviewSessionId(preview_id.to_owned()));
        Ok(())
    }

    pub fn current_version(&self, daemon: &str, session_id: &str) -> Result<i64, SessionError> {
        self.verify_daemon(daemon)?;
        let uri = self
            .session_uris
            .get(session_id)
            .ok_or(SessionError::UnknownSession)?;
        let session = self
            .documents
            .get(uri)
            .expect("session index is consistent");
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        i64::try_from(session.engine.document().document_version)
            .map_err(|_| SessionError::VersionMismatch)
    }

    pub fn export_html(
        &self,
        daemon: &str,
        session_id: &str,
        context: &fleximark_render_html::RenderContext,
        common_runtime: &str,
        composer: impl FnOnce(
            &str,
            Option<&RenderStyle>,
            &[RenderAsset],
            &str,
        ) -> Result<String, SessionError>,
    ) -> Result<UnsafeExportOutput, SessionError> {
        self.verify_daemon(daemon)?;
        let uri = self
            .session_uris
            .get(session_id)
            .ok_or(SessionError::UnknownSession)?;
        let session = self
            .documents
            .get(uri)
            .expect("session index is consistent");
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        let mut context = context.clone();
        context.resolved_resources = session
            .engine
            .render_config()
            .context
            .resolved_resources
            .clone();
        let prepared = session
            .engine
            .prepare_safe_export(&context)
            .map_err(engine_error)?;
        let resolved = prepared.compose_portable(common_runtime, composer)?;
        session
            .engine
            .apply_unsafe_export_html(resolved, &CancellationToken::default())
            .map(|run| run.value)
            .map_err(engine_error)
    }

    pub fn session_id_for_uri(&self, uri: &str) -> Option<&str> {
        self.documents.get(uri).map(|session| session.id.as_str())
    }

    pub fn line_prefix(&self, uri: &str, position: Position) -> Result<&str, SessionError> {
        let session = self.documents.get(uri).ok_or(SessionError::NotOpen)?;
        let source = session.engine.source();
        let offset = position_offset(source, position, self.position_encoding)
            .ok_or(SessionError::InvalidRange)?;
        let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
        Ok(&source[line_start..offset])
    }

    pub fn take_full_text_requests(&mut self) -> Vec<RequestFullTextParams> {
        std::mem::take(&mut self.events)
    }

    fn verify_daemon(&self, daemon: &str) -> Result<(), SessionError> {
        if daemon == self.daemon_instance_id {
            Ok(())
        } else {
            Err(SessionError::WrongDaemon)
        }
    }

    fn mark_out_of_sync(&mut self, uri: &str, reason: &str) {
        if let Some(session) = self.documents.get_mut(uri) {
            let version = session.engine.document().document_version;
            let _ = session.engine.checkpoint(version, "invalid");
            self.events.push(RequestFullTextParams {
                daemon_instance_id: self.daemon_instance_id.clone(),
                uri: session.uri.clone(),
                document_session_id: session.id.clone(),
                reason: reason.to_owned(),
            });
        }
    }
}

pub fn content_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn engine_error(error: EngineError) -> SessionError {
    match error {
        EngineError::ContentModified => SessionError::ContentModified,
        EngineError::StaleVersion { .. } => SessionError::StaleVersion,
        EngineError::CheckpointMismatch => SessionError::HashMismatch,
        other => SessionError::Engine(other.to_string()),
    }
}

fn position_offset(text: &str, position: Position, encoding: PositionEncoding) -> Option<usize> {
    let mut line_start = 0;
    for _ in 0..position.line {
        let newline = text[line_start..].find('\n')?;
        line_start += newline + 1;
    }
    let mut line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |index| line_start + index);
    if line_end > line_start && text.as_bytes()[line_end - 1] == b'\r' {
        line_end -= 1;
    }
    let line = &text[line_start..line_end];
    let target = usize::try_from(position.character).ok()?;
    match encoding {
        PositionEncoding::Utf8 => {
            if target <= line.len() && line.is_char_boundary(target) {
                Some(line_start + target)
            } else {
                None
            }
        }
        PositionEncoding::Utf16 | PositionEncoding::Utf32 => {
            let mut units = 0;
            for (offset, character) in line.char_indices() {
                if units == target {
                    return Some(line_start + offset);
                }
                units += if encoding == PositionEncoding::Utf16 {
                    character.len_utf16()
                } else {
                    1
                };
                if units > target {
                    return None;
                }
            }
            if units == target {
                Some(line_end)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use fleximark_protocol::{AttachDocumentParams, CheckpointDocumentParams, TextPosition};

    use super::*;

    fn open(registry: &mut SessionRegistry, text: &str, version: i64) {
        registry
            .open(DidOpenParams {
                text_document: TextDocumentItem {
                    uri: "file:///doc.md".into(),
                    version,
                    text: text.into(),
                },
            })
            .unwrap();
    }

    #[test]
    fn did_open_attach_and_checkpoint_share_one_session() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf16);
        open(&mut registry, "hello", 4);
        let hash = content_hash("hello");
        let attached = registry
            .attach(&AttachDocumentParams {
                daemon_instance_id: registry.daemon_instance_id().into(),
                uri: "file:///doc.md".into(),
                expected_document_version: 4,
                content_hash: hash.clone(),
            })
            .unwrap();
        let checked = registry
            .checkpoint(&CheckpointDocumentParams {
                daemon_instance_id: registry.daemon_instance_id().into(),
                document_session_id: attached.document_session_id,
                document_version: 4,
                content_hash: hash.clone(),
            })
            .unwrap();
        assert_eq!(checked.content_hash, hash);
    }

    #[test]
    fn utf16_edit_is_applied_at_character_boundary() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf16);
        open(&mut registry, "a😀b\n", 1);
        registry
            .change(DidChangeParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///doc.md".into(),
                    version: 8,
                },
                content_changes: vec![ContentChange {
                    range: Some(Range {
                        start: Position {
                            line: 0,
                            character: 1,
                        },
                        end: Position {
                            line: 0,
                            character: 3,
                        },
                    }),
                    text: "x".into(),
                }],
            })
            .unwrap();
        assert_eq!(
            registry.documents["file:///doc.md"].engine.source(),
            "axb\n"
        );
    }

    #[test]
    fn utf16_position_and_node_id_round_trip_through_authoritative_session() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf16);
        open(&mut registry, "# 😀 heading\n", 1);
        let daemon = registry.daemon_instance_id().to_owned();
        let session = registry
            .session_id_for_uri("file:///doc.md")
            .unwrap()
            .to_owned();
        let entry = registry
            .navigation_at_position(
                &daemon,
                &session,
                1,
                &TextPosition {
                    line: 0,
                    character: 4,
                },
            )
            .unwrap()
            .expect("heading has a navigation node");
        assert!(entry.source_range.byte_start <= 6);
        assert!(entry.source_range.byte_end >= 6);
        assert_eq!(entry.source_range.start.encoding, PositionEncoding::Utf8);
        assert_eq!(
            registry
                .navigation_for_node(&daemon, &session, 1, &entry.node_id)
                .unwrap(),
            Some(entry)
        );
    }

    #[test]
    fn stale_change_marks_session_out_of_sync_until_full_text() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf16);
        open(&mut registry, "old", 3);
        let stale = DidChangeParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: "file:///doc.md".into(),
                version: 3,
            },
            content_changes: vec![ContentChange {
                range: None,
                text: "ignored".into(),
            }],
        };
        assert_eq!(registry.change(stale), Err(SessionError::StaleVersion));
        assert!(registry.documents["file:///doc.md"].engine.is_out_of_sync());
        let requests = registry.take_full_text_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].daemon_instance_id,
            registry.daemon_instance_id()
        );
        assert_eq!(
            requests[0].document_session_id,
            registry.documents["file:///doc.md"].id
        );

        registry
            .change(DidChangeParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///doc.md".into(),
                    version: 9,
                },
                content_changes: vec![ContentChange {
                    range: None,
                    text: "fresh".into(),
                }],
            })
            .unwrap();
        assert!(!registry.documents["file:///doc.md"].engine.is_out_of_sync());
    }

    #[test]
    fn bad_checkpoint_requests_full_text_and_blocks_render_lookup() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
        open(&mut registry, "text", 1);
        let attached = registry
            .attach(&AttachDocumentParams {
                daemon_instance_id: registry.daemon_instance_id().into(),
                uri: "file:///doc.md".into(),
                expected_document_version: 1,
                content_hash: content_hash("text"),
            })
            .unwrap();
        let error = registry
            .checkpoint(&CheckpointDocumentParams {
                daemon_instance_id: registry.daemon_instance_id().into(),
                document_session_id: attached.document_session_id.clone(),
                document_version: 1,
                content_hash: content_hash("different"),
            })
            .unwrap_err();
        assert_eq!(error, SessionError::HashMismatch);
        assert_eq!(
            registry
                .document(
                    registry.daemon_instance_id(),
                    &attached.document_session_id,
                    1
                )
                .err()
                .unwrap(),
            SessionError::ContentModified
        );
        assert_eq!(registry.take_full_text_requests().len(), 1);
        registry
            .change(DidChangeParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///doc.md".into(),
                    version: 1,
                },
                content_changes: vec![ContentChange {
                    range: None,
                    text: "text".into(),
                }],
            })
            .unwrap();
        assert!(
            registry
                .document(
                    registry.daemon_instance_id(),
                    &attached.document_session_id,
                    1
                )
                .is_ok()
        );
    }
}
