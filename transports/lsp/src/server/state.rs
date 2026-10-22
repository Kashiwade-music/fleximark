//! Role: Hold transport session state and injected application I/O ports.
pub(super) use std::collections::{HashMap, HashSet};
pub(super) use std::sync::mpsc::Sender;
pub(super) use std::time::Instant;

pub(super) use fleximark_application::{
    DidChangeParams, DidCloseParams, DidOpenParams, SessionError, SessionRegistry,
};
pub(super) use fleximark_model::{
    Block, BlockKind, Document, Inline, InlineKind, Node, NodeId, PositionEncoding,
};
pub(super) use fleximark_plugin_api::CancellationToken;
pub(super) use fleximark_protocol::{
    AttachDocumentParams, CONTENT_MODIFIED, CreatePreviewParams, CreatePreviewResult,
    DisposePreviewParams, ExecuteCommandParams, GetNoteOptionsParams, IncomingMessage,
    InitializeParams, InitializeResult, JsSafeU64, PROTOCOL_VERSION, PreviewChangedParams,
    PreviewEventParams, PreviewNavigationEvent, PreviewTarget, ReconfigureWorkspaceParams,
    RenderNavigationEvent, RerenderPreviewParams, Response, ServerCapabilities, ServerPreviewEvent,
    ServerPreviewEventParams, SetSelectionParams, SetViewportParams, SourceNavigationEvent,
    WorkspaceStatus, method,
};
pub(super) use serde_json::{Value, json};
pub(super) use std::sync::Arc;

pub(super) use crate::cancellation::CancellationCoordinator;
pub(super) use crate::preview_http::{PreviewServer, random_token};
pub(super) use crate::telemetry::{OperationalTrace, log_operational_event};

pub struct Server {
    pub(super) next_client_request: u64,
    pub(super) pending_client_requests: HashSet<String>,
    pub(super) lsp_initialized: bool,
    pub(super) snippet_support: bool,
    pub(super) selection_events: bool,
    pub(super) viewport_events: bool,
    pub(super) shutdown: bool,
    pub(crate) exit: bool,
    pub(crate) registry: SessionRegistry,
    pub(crate) previews: PreviewServer,
    pub(crate) preview_states: HashMap<String, PreviewState>,
    pub(super) outgoing_events: Vec<Value>,
    pub(super) work_cancellation: CancellationToken,
    pub(super) publication_cancellation: CancellationToken,
}

#[derive(Clone)]
pub(crate) struct PreviewState {
    pub(crate) token: Option<String>,
    pub(super) document_session_id: String,
    pub(super) explicit_jump: Option<fleximark_application::Position>,
    // Browser delivery state only; the engine owns the authoritative render revision.
    pub(super) delivered_revision: u64,
}
