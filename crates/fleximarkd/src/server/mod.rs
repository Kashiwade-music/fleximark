mod commands;
mod diagnostics;
mod lsp;
mod routing;
mod rpc;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::time::Instant;

use fleximark_engine::RenderPublication;
use fleximark_lsp::{
    DidChangeParams, DidCloseParams, DidOpenParams, SessionError, SessionRegistry,
};
use fleximark_model::{
    Block, BlockKind, Document, Inline, InlineKind, Node, NodeId, PositionEncoding,
};
use fleximark_plugin_host::CancellationToken;
use fleximark_protocol::{
    AttachDocumentParams, CONTENT_MODIFIED, CheckpointDocumentParams, CreatePreviewParams,
    CreatePreviewResult, DisposePreviewParams, ExecuteCommandParams, GetNoteOptionsParams,
    IncomingMessage, InitializeParams, InitializeResult, Notification, PROTOCOL_VERSION,
    PreviewEventParams, PreviewNavigationEvent, PreviewTarget, ReconfigureWorkspaceParams,
    ReloadPreviewParams, RenderNavigationEvent, RenderParams, Response, RpcChangeDocumentParams,
    RpcCloseDocumentParams, RpcOpenDocumentParams, ServerCapabilities, SetSelectionParams,
    SetViewportParams, SourceNavigationEvent, WorkspaceStatus, method,
};
use fleximark_service::{
    acknowledge_export, collect_admonitions, create_note_with_options, default_export_destination,
    edit_theme, export_html_with_safety, get_note_options, initialize_workspace,
};
use serde_json::{Value, json};

use crate::cancellation::CancellationCoordinator;
use crate::preview_http::{PREVIEW_CLIENT, PreviewServer, random_token};
use crate::telemetry::{OperationalTrace, log_operational_event};

use diagnostics::{collect_heading_symbols, find_block};

pub(crate) struct Server {
    lsp_mode: bool,
    lsp_initialized: bool,
    fleximark_initialized: bool,
    selection_events: bool,
    viewport_events: bool,
    workspaces: HashMap<String, bool>,
    disabled_workspaces: HashSet<String>,
    config_generation: u64,
    shutdown: bool,
    pub(crate) exit: bool,
    pub(crate) registry: SessionRegistry,
    pub(crate) previews: PreviewServer,
    pub(crate) preview_states: HashMap<String, PreviewState>,
    outgoing_events: Vec<Value>,
    work_cancellation: CancellationToken,
    publication_cancellation: CancellationToken,
}

#[derive(Clone)]
pub(crate) struct PreviewState {
    pub(crate) token: Option<String>,
    document_session_id: String,
    // Browser delivery state only; the engine owns the authoritative render revision.
    delivered_revision: u64,
}

fn invalid_params(id: Value, error: impl std::fmt::Display) -> Value {
    response_value(Response::error(
        id,
        -32602,
        format!("invalid params: {error}"),
    ))
}

fn deserialize_params<P: serde::de::DeserializeOwned>(
    id: &Value,
    params: Value,
) -> Result<P, Value> {
    serde_json::from_value(params).map_err(|error| invalid_params(id.clone(), error))
}

pub(crate) fn session_error(id: Value, error: SessionError) -> Value {
    let code = if matches!(
        error,
        SessionError::ContentModified
            | SessionError::StaleVersion
            | SessionError::VersionMismatch
            | SessionError::HashMismatch
    ) {
        CONTENT_MODIFIED
    } else {
        -32602
    };
    response_value(Response::error(id, code, error.to_string()))
}

pub(crate) fn response_value(response: Response) -> Value {
    serde_json::to_value(response).expect("response is serializable")
}
