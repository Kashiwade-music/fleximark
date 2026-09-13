mod error;
mod index;
mod workspace;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fleximark_engine::{
    DocumentSession as EngineSession, PreviewSessionId, RenderAsset, RenderConfig,
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

pub use error::SessionError;
use error::engine_error;
use index::SessionIndex;
use workspace::WorkspaceAuthority;

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
    index: SessionIndex,
    events: Vec<RequestFullTextParams>,
    workspaces: WorkspaceAuthority,
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
            index: SessionIndex::default(),
            events: Vec::new(),
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

    pub fn configure_plugins(
        &mut self,
        host: Option<PluginHost>,
        render_config: Option<RenderConfig>,
    ) -> Result<(), SessionError> {
        if !self.index.is_empty() {
            return Err(SessionError::Engine(
                "plugins must be configured before opening documents".into(),
            ));
        }
        self.workspaces.configure_default(host, render_config);
        Ok(())
    }

    pub fn configure_workspaces(
        &mut self,
        workspaces: Vec<(String, PluginHost, RenderConfig)>,
    ) -> Result<(), SessionError> {
        if !self.index.is_empty() {
            return Err(SessionError::Engine(
                "workspaces must be configured before opening documents".into(),
            ));
        }
        self.workspaces.replace_roots(workspaces);
        Ok(())
    }

    pub fn reconfigure_document_assets(
        &mut self,
        uri: &str,
        assets: Vec<ResolvedRenderAsset>,
    ) -> Result<(), SessionError> {
        let workspace_uri = self
            .index
            .by_uri(uri)
            .ok_or(SessionError::NotOpen)?
            .workspace_uri
            .clone();
        let config = workspace_uri
            .as_deref()
            .and_then(|workspace_uri| self.workspaces.exact(workspace_uri))
            .map(|(_, config)| config.clone())
            .or_else(|| self.workspaces.default_render().cloned())
            .ok_or_else(|| SessionError::Engine("render configuration is missing".into()))?
            .with_resolved_assets(assets)
            .map_err(engine_error)?;
        self.index
            .by_uri_mut(uri)
            .ok_or(SessionError::NotOpen)?
            .engine
            .reconfigure_render(config)
            .map_err(engine_error)
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
        host: PluginHost,
        render_config: RenderConfig,
        mut assets: HashMap<String, Vec<ResolvedRenderAsset>>,
    ) -> Result<(), SessionError> {
        let workspace_uri = workspace_uri.trim_end_matches('/');
        let host = Arc::new(host);
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
                    &CancellationToken::default(),
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

    pub fn open(&mut self, params: DidOpenParams) -> Result<(), SessionError> {
        self.open_with_cancellation(params, &CancellationToken::default())
    }

    pub fn open_with_cancellation(
        &mut self,
        params: DidOpenParams,
        cancellation: &CancellationToken,
    ) -> Result<(), SessionError> {
        let item = params.text_document;
        let version = u64::try_from(item.version).map_err(|_| SessionError::StaleVersion)?;
        let workspace = self
            .workspaces
            .matching(&item.uri)
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
                cancellation,
            )
            .map_err(engine_error),
            None if self.workspaces.default_host().is_none() => EngineSession::open(
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
                self.workspaces.default_render().cloned().ok_or_else(|| {
                    SessionError::Engine("plugin host is missing render configuration".into())
                })?,
                Arc::clone(self.workspaces.default_host().expect("checked above")),
                cancellation,
            )
            .map_err(engine_error),
        }?;
        let id = engine.id().0.clone();
        let content_hash = content_hash(&item.text);
        self.index.insert(DocumentSession {
            id,
            uri: item.uri,
            content_hash,
            workspace_uri,
            engine,
        });
        Ok(())
    }

    pub fn change(&mut self, params: DidChangeParams) -> Result<(), SessionError> {
        self.change_with_cancellation(params, &CancellationToken::default())
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
        session.content_hash = content_hash(&text);
        Ok(())
    }

    pub fn close(&mut self, params: DidCloseParams) {
        self.index.remove_by_uri(&params.text_document.uri);
    }

    pub fn open_rpc(
        &mut self,
        params: RpcOpenDocumentParams,
    ) -> Result<AttachDocumentResult, SessionError> {
        self.open_rpc_with_cancellation(params, &CancellationToken::default())
    }

    pub fn open_rpc_with_cancellation(
        &mut self,
        params: RpcOpenDocumentParams,
        cancellation: &CancellationToken,
    ) -> Result<AttachDocumentResult, SessionError> {
        self.verify_daemon(&params.daemon_instance_id)?;
        let hash = content_hash(&params.text);
        self.open_with_cancellation(
            DidOpenParams {
                text_document: TextDocumentItem {
                    uri: params.uri.clone(),
                    version: params.document_version.get(),
                    text: params.text,
                },
            },
            cancellation,
        )?;
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
        self.change_rpc_with_cancellation(params, &CancellationToken::default())
    }

    pub fn change_rpc_with_cancellation(
        &mut self,
        params: RpcChangeDocumentParams,
        cancellation: &CancellationToken,
    ) -> Result<CheckpointDocumentResult, SessionError> {
        self.verify_daemon(&params.daemon_instance_id)?;
        let session = self
            .index
            .by_session(&params.document_session_id)
            .ok_or(SessionError::UnknownSession)?;
        let uri = session.uri.clone();
        let current_version = i64::try_from(session.engine.document().document_version)
            .map_err(|_| SessionError::VersionMismatch)?;
        if session.engine.is_out_of_sync()
            || current_version != params.base_document_version.get()
            || session.content_hash != params.base_content_hash
        {
            self.mark_out_of_sync(&uri, "standalone RPC change base mismatch");
            return Err(SessionError::ContentModified);
        }
        self.change_with_cancellation(
            DidChangeParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri,
                    version: params.document_version.get(),
                },
                content_changes: vec![ContentChange {
                    range: None,
                    text: params.text,
                }],
            },
            cancellation,
        )?;
        let session = self
            .index
            .by_session(&params.document_session_id)
            .ok_or(SessionError::UnknownSession)?;
        Ok(CheckpointDocumentResult {
            document_version: params.document_version,
            content_hash: session.content_hash.clone(),
        })
    }

    pub fn close_rpc(&mut self, params: RpcCloseDocumentParams) -> Result<(), SessionError> {
        self.verify_daemon(&params.daemon_instance_id)?;
        let uri = self
            .index
            .uri_for_session(&params.document_session_id)
            .ok_or(SessionError::UnknownSession)?
            .to_owned();
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
        if params.content_hash != session.content_hash {
            return Err(SessionError::HashMismatch);
        }
        Ok(AttachDocumentResult {
            document_session_id: session.id.clone(),
            document_version: version
                .try_into()
                .map_err(|_| SessionError::VersionMismatch)?,
            content_hash: session.content_hash.clone(),
        })
    }

    pub fn checkpoint(
        &mut self,
        params: &CheckpointDocumentParams,
    ) -> Result<CheckpointDocumentResult, SessionError> {
        self.verify_daemon(&params.daemon_instance_id)?;
        let session = self
            .index
            .by_session(&params.document_session_id)
            .ok_or(SessionError::UnknownSession)?;
        let uri = session.uri.clone();
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
        let session = self
            .index
            .by_session_mut(&params.document_session_id)
            .ok_or(SessionError::UnknownSession)?;
        session
            .engine
            .checkpoint(
                u64::try_from(params.document_version)
                    .map_err(|_| SessionError::VersionMismatch)?,
                &session.engine.content_hash(),
            )
            .map_err(engine_error)?;
        Ok(CheckpointDocumentResult {
            document_version: version
                .try_into()
                .map_err(|_| SessionError::VersionMismatch)?,
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
        self.render_with_cancellation(
            daemon,
            session_id,
            version,
            preview_id,
            &CancellationToken::default(),
        )
    }

    pub fn render_with_cancellation(
        &mut self,
        daemon: &str,
        session_id: &str,
        version: i64,
        preview_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<RenderPublication, SessionError> {
        self.verify_daemon(daemon)?;
        let session = self
            .index
            .by_session_mut(session_id)
            .ok_or(SessionError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        if i64::try_from(session.engine.document().document_version).ok() != Some(version) {
            return Err(SessionError::VersionMismatch);
        }
        session
            .engine
            .render_configured(PreviewSessionId(preview_id.to_owned()), cancellation)
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
        self.render_full_with_cancellation(
            daemon,
            session_id,
            version,
            preview_id,
            &CancellationToken::default(),
        )
    }

    pub fn render_full_with_cancellation(
        &mut self,
        daemon: &str,
        session_id: &str,
        version: i64,
        preview_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<fleximark_engine::RenderSnapshot, SessionError> {
        self.verify_daemon(daemon)?;
        let session = self
            .index
            .by_session_mut(session_id)
            .ok_or(SessionError::UnknownSession)?;
        if session.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        if i64::try_from(session.engine.document().document_version).ok() != Some(version) {
            return Err(SessionError::VersionMismatch);
        }
        let publication = session
            .engine
            .render_full_configured(PreviewSessionId(preview_id.to_owned()), cancellation)
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
        self.index
            .by_session_mut(session_id)
            .ok_or(SessionError::UnknownSession)?
            .engine
            .dispose_preview(&PreviewSessionId(preview_id.to_owned()));
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
        let session = self
            .index
            .by_session(session_id)
            .ok_or(SessionError::UnknownSession)?;
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
        self.index.session_id_for_uri(uri)
    }

    pub fn line_prefix(&self, uri: &str, position: Position) -> Result<&str, SessionError> {
        let session = self.index.by_uri(uri).ok_or(SessionError::NotOpen)?;
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
        if let Some(session) = self.index.by_uri_mut(uri) {
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
    use fleximark_engine::EngineError;
    use fleximark_plugin_host::{ExecutionLimits, HostPolicy};
    use fleximark_protocol::{AttachDocumentParams, CheckpointDocumentParams, TextPosition};

    use super::*;

    fn open(registry: &mut SessionRegistry, text: &str, version: i64) {
        open_uri(registry, "file:///doc.md", text, version);
    }

    fn open_uri(registry: &mut SessionRegistry, uri: &str, text: &str, version: i64) {
        registry
            .open(DidOpenParams {
                text_document: TextDocumentItem {
                    uri: uri.into(),
                    version,
                    text: text.into(),
                },
            })
            .unwrap();
    }

    fn configured_host(label: &str, trusted: bool) -> (PluginHost, RenderConfig) {
        let host = PluginHost::configured(
            ExecutionLimits::default(),
            HostPolicy {
                workspace_trusted: trusted,
                workspace_root: format!("file:///{label}"),
            },
            content_hash(label),
            1,
        )
        .unwrap();
        let config = RenderConfig::for_plugins(Default::default(), None, &host);
        (host, config)
    }

    fn render_fingerprint(
        registry: &mut SessionRegistry,
        uri: &str,
        version: i64,
        preview_id: &str,
    ) -> String {
        let daemon = registry.daemon_instance_id().to_owned();
        let session_id = registry.session_id_for_uri(uri).unwrap().to_owned();
        registry
            .render_full(&daemon, &session_id, version, preview_id)
            .unwrap()
            .renderer_fingerprint
    }

    fn assets_exceeding_total_limit() -> Vec<ResolvedRenderAsset> {
        (0_u8..9)
            .map(|index| {
                ResolvedRenderAsset::from_validated_bytes(
                    format!("file:///asset-{index}.bin"),
                    "application/octet-stream".into(),
                    &vec![index; 1024 * 1024],
                )
                .unwrap()
            })
            .collect()
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
                    line: 0.into(),
                    character: 4.into(),
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
        assert!(
            registry
                .index
                .by_uri("file:///doc.md")
                .unwrap()
                .engine
                .is_out_of_sync()
        );
        let requests = registry.take_full_text_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].daemon_instance_id,
            registry.daemon_instance_id()
        );
        assert_eq!(
            requests[0].document_session_id,
            registry.session_id_for_uri("file:///doc.md").unwrap()
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
        assert!(
            !registry
                .index
                .by_uri("file:///doc.md")
                .unwrap()
                .engine
                .is_out_of_sync()
        );
    }

    #[test]
    fn bad_checkpoint_requests_full_text_and_blocks_render_lookup() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
        open(&mut registry, "text", 1);
        let attached = registry
            .attach(&AttachDocumentParams {
                daemon_instance_id: registry.daemon_instance_id().into(),
                uri: "file:///doc.md".into(),
                expected_document_version: 1.into(),
                content_hash: content_hash("text"),
            })
            .unwrap();
        let error = registry
            .checkpoint(&CheckpointDocumentParams {
                daemon_instance_id: registry.daemon_instance_id().into(),
                document_session_id: attached.document_session_id.clone(),
                document_version: 1.into(),
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

    #[test]
    fn duplicate_open_change_checkpoint_close_and_reopen_keep_one_session_index() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
        let daemon = registry.daemon_instance_id().to_owned();
        open(&mut registry, "one", 1);
        let first = registry
            .session_id_for_uri("file:///doc.md")
            .unwrap()
            .to_owned();

        open(&mut registry, "two", 2);
        let second = registry
            .session_id_for_uri("file:///doc.md")
            .unwrap()
            .to_owned();
        assert_ne!(first, second);
        assert_eq!(
            registry.current_version(&daemon, &first),
            Err(SessionError::UnknownSession)
        );
        let attached = registry
            .attach(&AttachDocumentParams {
                daemon_instance_id: daemon.clone(),
                uri: "file:///doc.md".into(),
                expected_document_version: 2.into(),
                content_hash: content_hash("two"),
            })
            .unwrap();
        assert_eq!(attached.document_session_id, second);

        registry
            .change(DidChangeParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///doc.md".into(),
                    version: 3,
                },
                content_changes: vec![ContentChange {
                    range: None,
                    text: "three".into(),
                }],
            })
            .unwrap();
        registry
            .checkpoint(&CheckpointDocumentParams {
                daemon_instance_id: daemon.clone(),
                document_session_id: second.clone(),
                document_version: 3.into(),
                content_hash: content_hash("three"),
            })
            .unwrap();
        assert_eq!(registry.current_version(&daemon, &second), Ok(3));

        registry.close(DidCloseParams {
            text_document: TextDocumentIdentifier {
                uri: "file:///doc.md".into(),
            },
        });
        assert_eq!(registry.session_id_for_uri("file:///doc.md"), None);
        assert_eq!(
            registry.current_version(&daemon, &second),
            Err(SessionError::UnknownSession)
        );
        assert_eq!(
            registry
                .attach(&AttachDocumentParams {
                    daemon_instance_id: daemon.clone(),
                    uri: "file:///doc.md".into(),
                    expected_document_version: 3.into(),
                    content_hash: content_hash("three"),
                })
                .unwrap_err(),
            SessionError::NotOpen
        );

        open(&mut registry, "reopened", 4);
        let third = registry.session_id_for_uri("file:///doc.md").unwrap();
        assert_ne!(third, first);
        assert_ne!(third, second);
    }

    #[test]
    fn session_index_replaces_colliding_session_ids_atomically() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
        open_uri(&mut registry, "file:///first.md", "first", 1);
        open_uri(&mut registry, "file:///second.md", "second", 1);
        let first_id = registry
            .session_id_for_uri("file:///first.md")
            .unwrap()
            .to_owned();
        let second_id = registry
            .session_id_for_uri("file:///second.md")
            .unwrap()
            .to_owned();

        let mut replacement = registry.index.remove_by_uri("file:///second.md").unwrap();
        replacement.id = first_id.clone();
        registry.index.insert(replacement);

        assert!(registry.index.by_uri("file:///first.md").is_none());
        assert_eq!(
            registry.index.uri_for_session(&first_id),
            Some("file:///second.md")
        );
        assert_eq!(registry.index.uri_for_session(&second_id), None);
        assert_eq!(registry.index.iter().count(), 1);
    }

    #[test]
    fn incremental_edits_use_utf8_utf16_and_utf32_units() {
        let cases = [
            (PositionEncoding::Utf8, 1, 5),
            (PositionEncoding::Utf16, 1, 3),
            (PositionEncoding::Utf32, 1, 2),
        ];
        for (encoding, start, end) in cases {
            let mut registry = SessionRegistry::new(encoding);
            open(&mut registry, "a😀b\n", 1);
            registry
                .change(DidChangeParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: "file:///doc.md".into(),
                        version: 2,
                    },
                    content_changes: vec![ContentChange {
                        range: Some(Range {
                            start: Position {
                                line: 0,
                                character: start,
                            },
                            end: Position {
                                line: 0,
                                character: end,
                            },
                        }),
                        text: "x".into(),
                    }],
                })
                .unwrap();
            let session = registry
                .document(
                    registry.daemon_instance_id(),
                    registry.session_id_for_uri("file:///doc.md").unwrap(),
                    2,
                )
                .unwrap();
            assert_eq!(session.source(), "axb\n", "encoding: {encoding:?}");
            assert_eq!(session.position_encoding(), encoding);
        }
    }

    #[test]
    fn workspace_matching_preserves_raw_longest_segment_prefix_semantics() {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
        let (parent_host, parent_config) = configured_host("parent", true);
        let (nested_host, nested_config) = configured_host("nested", false);
        let (file_root_host, file_root_config) = configured_host("file-root", true);
        let (empty_root_host, empty_root_config) = configured_host("empty-root", true);
        registry
            .configure_workspaces(vec![
                ("".into(), empty_root_host, empty_root_config),
                ("file:///".into(), file_root_host, file_root_config),
                ("file:///workspace".into(), parent_host, parent_config),
                (
                    "file:///workspace/nested/".into(),
                    nested_host,
                    nested_config,
                ),
            ])
            .unwrap();

        let cases = [
            ("file:///workspace/nested", Some("file:///workspace/nested")),
            (
                "file:///workspace/nested/deep.md",
                Some("file:///workspace/nested"),
            ),
            ("file:///outside.md", Some("file:")),
            ("/outside.md", Some("")),
            (
                "file:///workspace/NESTED/case.md",
                Some("file:///workspace"),
            ),
            (
                "file:///workspace/%6Eested/encoded.md",
                Some("file:///workspace"),
            ),
            ("file:///workspace-other/sibling.md", Some("file:")),
            ("relative.md", None),
        ];
        for (uri, expected_workspace) in cases {
            open_uri(&mut registry, uri, "text", 1);
            let session_id = registry.session_id_for_uri(uri).unwrap();
            let session = registry
                .document(registry.daemon_instance_id(), session_id, 1)
                .unwrap();
            assert_eq!(session.workspace_uri(), expected_workspace);
        }

        let parent_fingerprint = render_fingerprint(
            &mut registry,
            "file:///workspace/NESTED/case.md",
            1,
            "parent-case",
        );
        let encoded_fingerprint = render_fingerprint(
            &mut registry,
            "file:///workspace/%6Eested/encoded.md",
            1,
            "parent-encoded",
        );
        let nested_fingerprint = render_fingerprint(
            &mut registry,
            "file:///workspace/nested/deep.md",
            1,
            "nested",
        );
        assert_eq!(encoded_fingerprint, parent_fingerprint);
        assert_ne!(nested_fingerprint, parent_fingerprint);
        // Compatibility boundary/security debt: authority matching compares raw URI text. Case and
        // percent-encoded aliases therefore miss the untrusted nested root and inherit its trusted
        // parent. Normalization requires a separately reviewed behavior and policy change.
    }

    #[test]
    fn compatibility_configuration_and_workspace_reconfiguration_are_transactional() {
        let uri = "file:///outside/document.md";
        let mut no_defaults = SessionRegistry::new(PositionEncoding::Utf8);
        no_defaults.configure_plugins(None, None).unwrap();
        open_uri(&mut no_defaults, uri, "global", 1);
        let plain_fingerprint = render_fingerprint(&mut no_defaults, uri, 1, "plain");

        let mut render_without_host = SessionRegistry::new(PositionEncoding::Utf8);
        let (_, ignored_render) = configured_host("ignored-render", true);
        render_without_host
            .configure_plugins(None, Some(ignored_render))
            .unwrap();
        open_uri(&mut render_without_host, uri, "global", 1);
        assert_eq!(
            render_fingerprint(&mut render_without_host, uri, 1, "render-only"),
            plain_fingerprint
        );

        let mut host_without_render = SessionRegistry::new(PositionEncoding::Utf8);
        let (orphan_host, _) = configured_host("orphan-host", true);
        host_without_render
            .configure_plugins(Some(orphan_host), None)
            .unwrap();
        assert_eq!(
            host_without_render.open(DidOpenParams {
                text_document: TextDocumentItem {
                    uri: uri.into(),
                    version: 1,
                    text: "global".into(),
                },
            }),
            Err(SessionError::Engine(
                "plugin host is missing render configuration".into()
            ))
        );
        assert!(host_without_render.open_documents().is_empty());
        assert_eq!(host_without_render.session_id_for_uri(uri), None);
        let (retry_host, retry_render) = configured_host("retry", true);
        host_without_render
            .configure_plugins(Some(retry_host), Some(retry_render))
            .unwrap();
        open_uri(&mut host_without_render, uri, "global", 1);

        let mut compatibility = SessionRegistry::new(PositionEncoding::Utf8);
        let (compatibility_host, compatibility_config) = configured_host("compatibility", true);
        compatibility
            .configure_plugins(Some(compatibility_host), Some(compatibility_config))
            .unwrap();
        open_uri(&mut compatibility, uri, "global", 1);
        let compatibility_fingerprint =
            render_fingerprint(&mut compatibility, uri, 1, "configured");
        assert_ne!(compatibility_fingerprint, plain_fingerprint);
        let compatibility_session = compatibility.session_id_for_uri(uri).unwrap().to_owned();
        let compatibility_daemon = compatibility.daemon_instance_id().to_owned();
        let (late_host, late_config) = configured_host("late", true);
        assert_eq!(
            compatibility.configure_plugins(Some(late_host), Some(late_config)),
            Err(SessionError::Engine(
                "plugins must be configured before opening documents".into()
            ))
        );
        assert_eq!(
            compatibility.current_version(&compatibility_daemon, &compatibility_session),
            Ok(1)
        );
        assert_eq!(
            render_fingerprint(&mut compatibility, uri, 1, "after-late-rejection"),
            compatibility_fingerprint
        );

        let mut workspace = SessionRegistry::new(PositionEncoding::Utf8);
        let (old_host, old_config) = configured_host("old", true);
        workspace
            .configure_workspaces(vec![("file:///workspace".into(), old_host, old_config)])
            .unwrap();
        let original_uris = ["file:///workspace/alpha.md", "file:///workspace/beta.md"];
        open_uri(&mut workspace, original_uris[0], "alpha", 1);
        open_uri(&mut workspace, original_uris[1], "beta", 2);
        let daemon = workspace.daemon_instance_id().to_owned();
        let baseline = original_uris.map(|uri| {
            let session_id = workspace.session_id_for_uri(uri).unwrap().to_owned();
            let session = workspace
                .document(
                    &daemon,
                    &session_id,
                    if uri.ends_with("alpha.md") { 1 } else { 2 },
                )
                .unwrap();
            (
                uri,
                session_id,
                session.source().to_owned(),
                session.document().document_version,
                render_fingerprint(
                    &mut workspace,
                    uri,
                    if uri.ends_with("alpha.md") { 1 } else { 2 },
                    &format!("before-{uri}"),
                ),
            )
        });
        let stage_order = workspace
            .index
            .iter()
            .filter(|(_, session)| session.workspace_uri() == Some("file:///workspace"))
            .map(|(uri, _)| uri.to_owned())
            .collect::<Vec<_>>();
        assert_eq!(stage_order.len(), 2);
        let failing_uri = stage_order.last().unwrap().clone();
        let mut oversized_assets = HashMap::new();
        oversized_assets.insert(failing_uri, assets_exceeding_total_limit());
        let (mismatched_host, mismatched_config) = configured_host("mismatched", true);
        let error = workspace
            .reconfigure_workspace(
                "file:///workspace",
                mismatched_host,
                mismatched_config,
                oversized_assets,
            )
            .unwrap_err();
        assert_eq!(
            error,
            SessionError::Engine(
                "invalid resolved render asset: resolved assets exceed 8 MiB".into()
            )
        );
        for (uri, session_id, source, version, fingerprint) in &baseline {
            assert_eq!(workspace.session_id_for_uri(uri), Some(session_id.as_str()));
            let session = workspace
                .document(&daemon, session_id, *version as i64)
                .unwrap();
            assert_eq!(session.source(), source);
            assert_eq!(session.document().document_version, *version);
            assert_eq!(
                render_fingerprint(
                    &mut workspace,
                    uri,
                    *version as i64,
                    &format!("after-failure-{uri}"),
                ),
                *fingerprint
            );
        }

        let new_uri = "file:///workspace/after-failure.md";
        open_uri(&mut workspace, new_uri, "new", 3);
        let new_session_id = workspace.session_id_for_uri(new_uri).unwrap().to_owned();
        assert_eq!(
            workspace
                .document(&daemon, &new_session_id, 3)
                .unwrap()
                .workspace_uri(),
            Some("file:///workspace")
        );
        assert_eq!(
            render_fingerprint(&mut workspace, new_uri, 3, "new-after-failure"),
            baseline[0].4
        );

        let (replacement_host, replacement_config) = configured_host("replacement", true);
        workspace
            .reconfigure_workspace(
                "file:///workspace",
                replacement_host,
                replacement_config,
                HashMap::new(),
            )
            .unwrap();
        let all_documents = [
            (original_uris[0], baseline[0].1.as_str(), "alpha", 1),
            (original_uris[1], baseline[1].1.as_str(), "beta", 2),
            (new_uri, new_session_id.as_str(), "new", 3),
        ];
        let mut replacement_fingerprint = None;
        for (uri, session_id, source, version) in all_documents {
            assert_eq!(workspace.session_id_for_uri(uri), Some(session_id));
            let session = workspace.document(&daemon, session_id, version).unwrap();
            assert_eq!(session.source(), source);
            assert_eq!(session.document().document_version, version as u64);
            let fingerprint = render_fingerprint(
                &mut workspace,
                uri,
                version,
                &format!("after-success-{uri}"),
            );
            assert_ne!(fingerprint, baseline[0].4);
            if let Some(expected) = &replacement_fingerprint {
                assert_eq!(&fingerprint, expected);
            } else {
                replacement_fingerprint = Some(fingerprint);
            }
        }
    }

    #[test]
    fn engine_error_categories_preserve_session_error_identity_and_messages() {
        let cases = [
            (EngineError::ContentModified, SessionError::ContentModified),
            (
                EngineError::StaleVersion {
                    current: 3,
                    received: 2,
                },
                SessionError::StaleVersion,
            ),
            (EngineError::CheckpointMismatch, SessionError::HashMismatch),
            (
                EngineError::Plugin("plugin detail".into()),
                SessionError::Engine("plugin pipeline failed: plugin detail".into()),
            ),
            (
                EngineError::UnsafeExportPolicy,
                SessionError::Engine(
                    "unsafe plugin HTML is available only after a safe portable render".into(),
                ),
            ),
            (
                EngineError::Asset("asset detail".into()),
                SessionError::Engine("invalid resolved render asset: asset detail".into()),
            ),
        ];
        for (engine, expected) in cases {
            assert_eq!(engine_error(engine), expected);
        }
    }
}
