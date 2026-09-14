use std::io::{self, BufRead, Write};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use thiserror::Error;

pub use fleximark_model::{NavigationEntry, NodeId, SourcePosition, SourceRange};
pub use fleximark_wire::{JsSafeI64, JsSafeU64, MAX_SAFE_INTEGER};

pub const PROTOCOL_VERSION: u32 = 1;
pub const CONTENT_MODIFIED: i64 = -32801;
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MethodDirection {
    ClientToServer,
    ServerToClient,
    Bidirectional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    Request,
    Notification,
}

macro_rules! declare_wire_types {
    ($(($variant:ident, $wire_name:literal, $schema_name:literal, $ts_name:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub enum WireType { $($variant),+ }

        impl WireType {
            pub const ALL: &[Self] = &[$(Self::$variant),+];

            pub const fn wire_name(self) -> &'static str {
                match self { $(Self::$variant => $wire_name),+ }
            }

            pub const fn schema_name(self) -> &'static str {
                match self { $(Self::$variant => $schema_name),+ }
            }

            pub const fn typescript_name(self) -> &'static str {
                match self { $(Self::$variant => $ts_name),+ }
            }
        }
    };
}

declare_wire_types! {
    (Unit, "Unit", "unit", "null"),
    (InitializeParams, "InitializeParams", "initializeParams", "InitializeParams"),
    (InitializeResult, "InitializeResult", "initializeResult", "InitializeResult"),
    (AttachDocumentParams, "AttachDocumentParams", "attachDocumentParams", "AttachDocumentParams"),
    (AttachDocumentResult, "AttachDocumentResult", "attachDocumentResult", "AttachDocumentResult"),
    (CheckpointDocumentParams, "CheckpointDocumentParams", "checkpointDocumentParams", "CheckpointDocumentParams"),
    (CheckpointDocumentResult, "CheckpointDocumentResult", "checkpointDocumentResult", "CheckpointDocumentResult"),
    (RequestFullTextParams, "RequestFullTextParams", "requestFullTextParams", "RequestFullTextParams"),
    (RenderParams, "RenderParams", "renderParams", "RenderParams"),
    (RenderPublication, "RenderPublication", "renderPublication", "RenderPublication"),
    (CreatePreviewParams, "CreatePreviewParams", "createPreviewParams", "CreatePreviewParams"),
    (CreatePreviewResult, "CreatePreviewResult", "createPreviewResult", "CreatePreviewResult"),
    (DisposePreviewParams, "DisposePreviewParams", "disposePreviewParams", "DisposePreviewParams"),
    (SetSelectionParams, "SetSelectionParams", "setSelectionParams", "SetSelectionParams"),
    (SetViewportParams, "SetViewportParams", "setViewportParams", "SetViewportParams"),
    (PreviewEventParams, "PreviewEventParams", "previewEventParams", "PreviewEventParams"),
    (ServerPreviewEventParams, "ServerPreviewEventParams", "serverPreviewEventParams", "ServerPreviewEventParams"),
    (ReloadPreviewParams, "ReloadPreviewParams", "reloadPreviewParams", "ReloadPreviewParams"),
    (ExecuteCommandParams, "ExecuteCommandParams", "executeCommandParams", "ExecuteCommandParams"),
    (CommandResult, "CommandResult", "commandResult", "CommandResult"),
    (GetNoteOptionsParams, "GetNoteOptionsParams", "getNoteOptionsParams", "GetNoteOptionsParams"),
    (GetNoteOptionsResult, "GetNoteOptionsResult", "getNoteOptionsResult", "GetNoteOptionsResult"),
    (ReconfigureWorkspaceParams, "ReconfigureWorkspaceParams", "reconfigureWorkspaceParams", "ReconfigureWorkspaceParams"),
    (RpcOpenDocumentParams, "RpcOpenDocumentParams", "openDocumentParams", "RpcOpenDocumentParams"),
    (RpcChangeDocumentParams, "RpcChangeDocumentParams", "changeDocumentParams", "RpcChangeDocumentParams"),
    (RpcCloseDocumentParams, "RpcCloseDocumentParams", "closeDocumentParams", "RpcCloseDocumentParams")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MethodSpec {
    pub name: &'static str,
    pub direction: MethodDirection,
    pub kind: MethodKind,
    pub client_to_server_params_type: Option<WireType>,
    pub server_to_client_params_type: Option<WireType>,
    pub result_type: Option<WireType>,
}

macro_rules! declare_methods {
    ($(($constant:ident, $name:literal, $direction:ident, $kind:ident,
        $client_params:expr, $server_params:expr, $result:expr)),+ $(,)?) => {
        pub mod method {
            $(pub const $constant: &str = $name;)+
        }

        pub const METHOD_SPECS: &[MethodSpec] = &[
            $(MethodSpec {
                name: $name,
                direction: MethodDirection::$direction,
                kind: MethodKind::$kind,
                client_to_server_params_type: $client_params,
                server_to_client_params_type: $server_params,
                result_type: $result,
            },)+
        ];
    };
}

// Engine-owned payloads use semantic WireType variants to avoid a protocol -> engine cycle.
declare_methods! {
    (INITIALIZE, "fleximark/initialize", ClientToServer, Request, Some(WireType::InitializeParams), None, Some(WireType::InitializeResult)),
    (ATTACH_DOCUMENT, "fleximark/attachDocument", ClientToServer, Request, Some(WireType::AttachDocumentParams), None, Some(WireType::AttachDocumentResult)),
    (CHECKPOINT_DOCUMENT, "fleximark/checkpointDocument", ClientToServer, Request, Some(WireType::CheckpointDocumentParams), None, Some(WireType::CheckpointDocumentResult)),
    (REQUEST_FULL_TEXT, "fleximark/requestFullText", ServerToClient, Notification, None, Some(WireType::RequestFullTextParams), None),
    (RENDER, "fleximark/render", ClientToServer, Request, Some(WireType::RenderParams), None, Some(WireType::RenderPublication)),
    (CREATE_PREVIEW, "fleximark/createPreview", ClientToServer, Request, Some(WireType::CreatePreviewParams), None, Some(WireType::CreatePreviewResult)),
    (DISPOSE_PREVIEW, "fleximark/disposePreview", ClientToServer, Request, Some(WireType::DisposePreviewParams), None, Some(WireType::Unit)),
    (SET_SELECTION, "fleximark/setSelection", ClientToServer, Notification, Some(WireType::SetSelectionParams), None, None),
    (SET_VIEWPORT, "fleximark/setViewport", ClientToServer, Notification, Some(WireType::SetViewportParams), None, None),
    (PREVIEW_EVENT, "fleximark/previewEvent", Bidirectional, Notification, Some(WireType::PreviewEventParams), Some(WireType::ServerPreviewEventParams), None),
    (RELOAD_PREVIEW, "fleximark/reloadPreview", ClientToServer, Request, Some(WireType::ReloadPreviewParams), None, Some(WireType::Unit)),
    (EXECUTE_COMMAND, "fleximark/executeCommand", ClientToServer, Request, Some(WireType::ExecuteCommandParams), None, Some(WireType::CommandResult)),
    (GET_NOTE_OPTIONS, "fleximark/getNoteOptions", ClientToServer, Request, Some(WireType::GetNoteOptionsParams), None, Some(WireType::GetNoteOptionsResult)),
    (RECONFIGURE_WORKSPACE, "fleximark/reconfigureWorkspace", ClientToServer, Request, Some(WireType::ReconfigureWorkspaceParams), None, Some(WireType::Unit)),
    (OPEN_DOCUMENT, "fleximark/openDocument", ClientToServer, Request, Some(WireType::RpcOpenDocumentParams), None, Some(WireType::AttachDocumentResult)),
    (CHANGE_DOCUMENT, "fleximark/changeDocument", ClientToServer, Request, Some(WireType::RpcChangeDocumentParams), None, Some(WireType::CheckpointDocumentResult)),
    (CLOSE_DOCUMENT, "fleximark/closeDocument", ClientToServer, Request, Some(WireType::RpcCloseDocumentParams), None, Some(WireType::Unit))
}

pub fn method_spec(name: &str) -> Option<&'static MethodSpec> {
    METHOD_SPECS.iter().find(|spec| spec.name == name)
}

/// A JSON-RPC request identifier that can be represented exactly by JavaScript.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum RpcId {
    Integer(JsSafeI64),
    String(String),
}

impl From<RpcId> for Value {
    fn from(id: RpcId) -> Self {
        serde_json::to_value(id).expect("JSON-RPC ids are serializable")
    }
}

impl TryFrom<Value> for RpcId {
    type Error = serde_json::Error;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value)
    }
}

impl TryFrom<i64> for RpcId {
    type Error = fleximark_wire::IntegerOutOfRange;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        JsSafeI64::try_from(value).map(Self::Integer)
    }
}

impl From<String> for RpcId {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for RpcId {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

fn deserialize_present_rpc_id<'de, D>(deserializer: D) -> Result<Option<RpcId>, D::Error>
where
    D: Deserializer<'de>,
{
    RpcId::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncomingMessage {
    pub jsonrpc: String,
    #[serde(default, deserialize_with = "deserialize_present_rpc_id")]
    pub id: Option<RpcId>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Option<RpcId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

impl Response {
    pub fn success(id: Value, result: impl Serialize) -> Self {
        Self {
            jsonrpc: "2.0",
            id: Some(RpcId::try_from(id).expect("response id must be a valid JSON-RPC id")),
            result: Some(serde_json::to_value(result).expect("serializable protocol result")),
            error: None,
        }
    }

    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: if id.is_null() {
                None
            } else {
                Some(RpcId::try_from(id).expect("response id must be a valid JSON-RPC id"))
            },
            result: None,
            error: Some(ResponseError {
                code: JsSafeI64::new(code).expect("JSON-RPC error code must be JavaScript-safe"),
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ResponseError {
    pub code: JsSafeI64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct Notification<P> {
    pub jsonrpc: &'static str,
    pub method: &'static str,
    pub params: P,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializeParams {
    pub protocol_version: u32,
    pub client: ClientInfo,
    #[serde(default)]
    pub capabilities: ClientCapabilities,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceGrant>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WorkspaceGrant {
    pub uri: String,
    pub trusted: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconfigureWorkspaceParams {
    pub daemon_instance_id: String,
    pub workspace_uri: String,
    pub trusted: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientCapabilities {
    #[serde(default)]
    pub embedded_html: bool,
    #[serde(default)]
    pub structured_preview: bool,
    #[serde(default)]
    pub selection_events: bool,
    #[serde(default)]
    pub viewport_events: bool,
    #[serde(default)]
    pub open_external: bool,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: u32,
    pub daemon_instance_id: String,
    pub capabilities: ServerCapabilities,
    pub workspace_statuses: Vec<WorkspaceStatus>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatus {
    pub uri: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    pub html_render: bool,
    pub document_checkpoint: bool,
    pub selection_events: bool,
    pub viewport_events: bool,
    pub workspace_commands: Vec<&'static str>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachDocumentParams {
    pub daemon_instance_id: String,
    pub uri: String,
    pub expected_document_version: JsSafeI64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AttachDocumentResult {
    pub document_session_id: String,
    pub document_version: JsSafeI64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointDocumentParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub document_version: JsSafeI64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct CheckpointDocumentResult {
    pub document_version: JsSafeI64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcOpenDocumentParams {
    pub daemon_instance_id: String,
    pub uri: String,
    pub document_version: JsSafeI64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcChangeDocumentParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub base_document_version: JsSafeI64,
    pub base_content_hash: String,
    pub document_version: JsSafeI64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcCloseDocumentParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RequestFullTextParams {
    pub daemon_instance_id: String,
    pub uri: String,
    pub document_session_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub document_version: JsSafeI64,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePreviewParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub expected_document_version: JsSafeI64,
    pub target: PreviewTarget,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum PreviewTarget {
    EmbeddedHtml,
    ExternalBrowser,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct CreatePreviewResult<T> {
    pub preview_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub initial_publication: T,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisposePreviewParams {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetSelectionParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub expected_document_version: JsSafeI64,
    pub selections: Vec<TextSelection>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextPosition {
    pub line: JsSafeU64,
    pub character: JsSafeU64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TextSelection {
    pub anchor: TextPosition,
    pub active: TextPosition,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PreviewNavigationEvent {
    SelectNode {
        preview_session_id: String,
        render_revision: JsSafeU64,
        node_id: NodeId,
    },
    RevealNode {
        preview_session_id: String,
        render_revision: JsSafeU64,
        node_id: NodeId,
    },
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SourceNavigationEvent {
    SelectSource { source_range: SourceRange },
    RevealSource { source_range: SourceRange },
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RenderNavigationEvent {
    Selection {
        preview_session_id: String,
        render_revision: JsSafeU64,
        node_ids: Vec<NodeId>,
        active_position: Option<TextPosition>,
    },
    Viewport {
        preview_session_id: String,
        render_revision: JsSafeU64,
        node_id: NodeId,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetViewportParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub expected_document_version: JsSafeI64,
    pub ranges: Vec<TextRange>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewEventParams {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
    pub render_revision: JsSafeU64,
    pub event: PreviewNavigationEvent,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ServerPreviewEvent<P> {
    Publication(P),
    RenderNavigation(RenderNavigationEvent),
    SourceNavigation(SourceNavigationEvent),
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServerPreviewEventParams<P> {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
    pub render_revision: JsSafeU64,
    pub event: ServerPreviewEvent<P>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReloadPreviewParams {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteCommandParams {
    pub daemon_instance_id: String,
    pub command: String,
    #[serde(default)]
    pub document_session_id: Option<String>,
    #[serde(default)]
    pub expected_document_version: Option<JsSafeI64>,
    #[serde(default)]
    pub workspace_uri: Option<String>,
    #[serde(default)]
    pub destination_uri: Option<String>,
    #[serde(default)]
    pub note_category: Option<String>,
    #[serde(default)]
    pub note_template: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetNoteOptionsParams {
    pub daemon_instance_id: String,
    pub workspace_uri: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
pub struct GetNoteOptionsResult {
    pub categories: Vec<String>,
    pub templates: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<CommandMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_uri: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
pub struct CommandMessage {
    pub level: CommandMessageLevel,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CommandMessageLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("malformed protocol header")]
    MalformedHeader,
    #[error("missing Content-Length header")]
    MissingContentLength,
    #[error("duplicate Content-Length header")]
    DuplicateContentLength,
    #[error("message is larger than the {MAX_MESSAGE_BYTES} byte limit")]
    MessageTooLarge,
}

pub fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, FrameError> {
    let mut content_length = None;
    let mut saw_header = false;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return if saw_header {
                Err(FrameError::MalformedHeader)
            } else {
                Ok(None)
            };
        }
        saw_header = true;
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(FrameError::MalformedHeader)?;
        if name.eq_ignore_ascii_case("Content-Length") {
            if content_length.is_some() {
                return Err(FrameError::DuplicateContentLength);
            }
            let length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| FrameError::MalformedHeader)?;
            if length > MAX_MESSAGE_BYTES {
                return Err(FrameError::MessageTooLarge);
            }
            content_length = Some(length);
        }
    }
    let length = content_length.ok_or(FrameError::MissingContentLength)?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), FrameError> {
    let body = serde_json::to_vec(value).expect("serializable protocol message");
    if body.len() > MAX_MESSAGE_BYTES {
        return Err(FrameError::MessageTooLarge);
    }
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::io::{BufReader, Cursor};

    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[test]
    fn rpc_ids_accept_exact_javascript_boundaries_and_strings() {
        for id in [-MAX_SAFE_INTEGER, MAX_SAFE_INTEGER] {
            let message: IncomingMessage = serde_json::from_value(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "unknown/request"
            }))
            .unwrap();
            assert_eq!(message.id, Some(RpcId::try_from(id).unwrap()));
        }
        let message: IncomingMessage = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "id": "request-id",
            "method": "unknown/request"
        }))
        .unwrap();
        assert_eq!(message.id, Some(RpcId::from("request-id")));
    }

    #[test]
    fn rpc_ids_reject_unsafe_integers_and_explicit_null() {
        for id in [-MAX_SAFE_INTEGER - 1, MAX_SAFE_INTEGER + 1] {
            assert!(
                serde_json::from_value::<IncomingMessage>(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "unknown/request"
                }))
                .is_err()
            );
        }
        assert!(
            serde_json::from_value::<IncomingMessage>(json!({
                "jsonrpc": "2.0",
                "id": null,
                "method": "unknown/request"
            }))
            .is_err()
        );
        let notification: IncomingMessage = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "method": "unknown/notification"
        }))
        .unwrap();
        assert_eq!(notification.id, None);
    }

    #[test]
    fn null_response_id_is_reserved_for_unattributable_errors() {
        assert_eq!(
            serde_json::to_value(Response::error(Value::Null, -32700, "parse error")).unwrap(),
            json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": "parse error" }
            })
        );
    }

    #[test]
    fn framed_messages_round_trip_back_to_back() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &json!({"jsonrpc":"2.0","id":1})).unwrap();
        write_frame(&mut bytes, &json!({"jsonrpc":"2.0","id":2})).unwrap();
        let mut reader = BufReader::new(Cursor::new(bytes));
        assert_eq!(
            serde_json::from_slice::<Value>(&read_frame(&mut reader).unwrap().unwrap()).unwrap()["id"],
            1
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&read_frame(&mut reader).unwrap().unwrap()).unwrap()["id"],
            2
        );
        assert!(read_frame(&mut reader).unwrap().is_none());
    }

    #[test]
    fn rejects_missing_and_duplicate_lengths() {
        let mut missing = Cursor::new(b"Other: x\r\n\r\n".as_slice());
        assert!(matches!(
            read_frame(&mut missing),
            Err(FrameError::MissingContentLength)
        ));
        let mut duplicate =
            Cursor::new(b"Content-Length: 0\r\ncontent-length: 0\r\n\r\n".as_slice());
        assert!(matches!(
            read_frame(&mut duplicate),
            Err(FrameError::DuplicateContentLength)
        ));
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ContractFixture {
        schema_version: u32,
        methods: Vec<ContractCase>,
    }

    #[derive(Deserialize)]
    struct ContractCase {
        method: String,
        direction: String,
        kind: String,
        params: Value,
        result: Value,
    }

    fn contract_fixture() -> ContractFixture {
        serde_json::from_str(include_str!(
            "../../../test/fixtures/protocol-v1-contract.json"
        ))
        .unwrap()
    }

    fn accept_contract_params(case: &ContractCase, spec: &MethodSpec) {
        macro_rules! accept {
            ($type:ty) => {
                serde_json::from_value::<$type>(case.params.clone()).unwrap()
            };
        }

        let params_type = spec
            .client_to_server_params_type
            .or(spec.server_to_client_params_type)
            .unwrap();
        match params_type {
            WireType::InitializeParams => {
                let params = accept!(InitializeParams);
                assert!(!params.capabilities.embedded_html);
                assert!(params.workspaces.is_empty());
            }
            WireType::AttachDocumentParams => drop(accept!(AttachDocumentParams)),
            WireType::CheckpointDocumentParams => drop(accept!(CheckpointDocumentParams)),
            WireType::RequestFullTextParams => {
                let params = RequestFullTextParams {
                    daemon_instance_id: case.params["daemonInstanceId"].as_str().unwrap().into(),
                    uri: case.params["uri"].as_str().unwrap().into(),
                    document_session_id: case.params["documentSessionId"].as_str().unwrap().into(),
                    reason: case.params["reason"].as_str().unwrap().into(),
                };
                assert_eq!(serde_json::to_value(params).unwrap(), case.params);
            }
            WireType::RenderParams => drop(accept!(RenderParams)),
            WireType::CreatePreviewParams => {
                let params = accept!(CreatePreviewParams);
                assert_eq!(params.target, PreviewTarget::ExternalBrowser);
            }
            WireType::DisposePreviewParams => drop(accept!(DisposePreviewParams)),
            WireType::SetSelectionParams => drop(accept!(SetSelectionParams)),
            WireType::SetViewportParams => drop(accept!(SetViewportParams)),
            WireType::PreviewEventParams => drop(accept!(PreviewEventParams)),
            WireType::ReloadPreviewParams => drop(accept!(ReloadPreviewParams)),
            WireType::ExecuteCommandParams => {
                let params = accept!(ExecuteCommandParams);
                assert!(params.document_session_id.is_none());
                assert!(params.expected_document_version.is_none());
                assert!(params.workspace_uri.is_none());
                assert!(params.destination_uri.is_none());
                assert!(params.note_category.is_none());
                assert!(params.note_template.is_none());
            }
            WireType::GetNoteOptionsParams => drop(accept!(GetNoteOptionsParams)),
            WireType::ReconfigureWorkspaceParams => drop(accept!(ReconfigureWorkspaceParams)),
            WireType::RpcOpenDocumentParams => drop(accept!(RpcOpenDocumentParams)),
            WireType::RpcChangeDocumentParams => drop(accept!(RpcChangeDocumentParams)),
            WireType::RpcCloseDocumentParams => drop(accept!(RpcCloseDocumentParams)),
            wire_type => panic!(
                "registry uses non-params type {wire_type:?} for {}",
                case.method
            ),
        }
    }

    fn serialize_contract_result(case: &ContractCase, spec: &MethodSpec) -> Value {
        let string = |field: &str| case.result[field].as_str().unwrap().to_owned();
        let integer = |field: &str| JsSafeI64::new(case.result[field].as_i64().unwrap()).unwrap();
        match spec.result_type {
            Some(WireType::InitializeResult) => serde_json::to_value(InitializeResult {
                protocol_version: case.result["protocolVersion"].as_u64().unwrap() as u32,
                daemon_instance_id: string("daemonInstanceId"),
                capabilities: ServerCapabilities {
                    html_render: true,
                    document_checkpoint: true,
                    selection_events: true,
                    viewport_events: false,
                    workspace_commands: vec!["editTheme"],
                },
                workspace_statuses: vec![WorkspaceStatus {
                    uri: "file:///workspace".into(),
                    enabled: true,
                    error: None,
                }],
            })
            .unwrap(),
            Some(WireType::AttachDocumentResult) => serde_json::to_value(AttachDocumentResult {
                document_session_id: string("documentSessionId"),
                document_version: integer("documentVersion"),
                content_hash: string("contentHash"),
            })
            .unwrap(),
            Some(WireType::CheckpointDocumentResult) => {
                serde_json::to_value(CheckpointDocumentResult {
                    document_version: integer("documentVersion"),
                    content_hash: string("contentHash"),
                })
                .unwrap()
            }
            Some(WireType::CreatePreviewResult) => serde_json::to_value(CreatePreviewResult {
                preview_session_id: string("previewSessionId"),
                url: case
                    .result
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                initial_publication: case.result["initialPublication"].clone(),
            })
            .unwrap(),
            Some(WireType::GetNoteOptionsResult) => serde_json::to_value(GetNoteOptionsResult {
                categories: case.result["categories"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap().to_owned())
                    .collect(),
                templates: case.result["templates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap().to_owned())
                    .collect(),
            })
            .unwrap(),
            Some(WireType::CommandResult) => serde_json::to_value(CommandResult {
                message: None,
                open_uri: None,
            })
            .unwrap(),
            Some(WireType::RenderPublication) => {
                assert!(
                    matches!(case.result["type"].as_str(), Some("full" | "patch")),
                    "{} result is not a render publication",
                    case.method
                );
                case.result.clone()
            }
            None => case.result.clone(),
            Some(WireType::Unit) => {
                assert!(
                    case.result.is_null(),
                    "unit result changed for {}",
                    case.method
                );
                case.result.clone()
            }
            result_type => panic!(
                "registry uses non-result type {result_type:?} for {}",
                case.method
            ),
        }
    }

    #[test]
    fn every_custom_method_matches_the_shared_wire_fixture() {
        let fixture = contract_fixture();
        assert_eq!(fixture.schema_version, PROTOCOL_VERSION);
        let expected = METHOD_SPECS
            .iter()
            .map(|spec| spec.name)
            .collect::<HashSet<_>>();
        let actual = fixture
            .methods
            .iter()
            .map(|case| case.method.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            fixture.methods.len(),
            expected.len(),
            "duplicate fixture method"
        );
        assert_eq!(actual, expected);

        for case in &fixture.methods {
            let spec = method_spec(&case.method).unwrap();
            assert_eq!(
                serde_json::to_value(spec.direction).unwrap(),
                case.direction,
                "registry direction changed for {}",
                case.method
            );
            assert_eq!(
                case.kind,
                match spec.kind {
                    MethodKind::Request => "request",
                    MethodKind::Notification => "notification",
                },
                "registry kind changed for {}",
                case.method
            );
            accept_contract_params(case, spec);
            let result = serialize_contract_result(case, spec);
            assert_eq!(
                result, case.result,
                "wire result changed for {}",
                case.method
            );
            if case.kind == "request" {
                assert_eq!(
                    serde_json::to_value(Response::success(json!(7), result)).unwrap(),
                    json!({"jsonrpc":"2.0","id":7,"result":case.result}),
                    "response envelope changed for {}",
                    case.method
                );
            } else {
                assert_eq!(case.kind, "notification");
                assert!(case.result.is_null());
            }
        }
    }

    #[test]
    fn method_registry_is_unique_and_directionally_complete() {
        let names = METHOD_SPECS
            .iter()
            .map(|spec| spec.name)
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), METHOD_SPECS.len(), "duplicate registry method");

        for spec in METHOD_SPECS {
            let expected_payloads = match spec.direction {
                MethodDirection::ClientToServer => (true, false),
                MethodDirection::ServerToClient => (false, true),
                MethodDirection::Bidirectional => (true, true),
            };
            assert_eq!(
                (
                    spec.client_to_server_params_type.is_some(),
                    spec.server_to_client_params_type.is_some()
                ),
                expected_payloads,
                "registry direction disagrees with payloads for {}",
                spec.name
            );
            assert_eq!(
                spec.result_type.is_some(),
                spec.kind == MethodKind::Request,
                "registry kind disagrees with result for {}",
                spec.name
            );
            assert_eq!(method_spec(spec.name), Some(spec));
        }

        assert!(method_spec("fleximark/notRegistered").is_none());
        let preview_event = method_spec(method::PREVIEW_EVENT).unwrap();
        assert_eq!(preview_event.direction, MethodDirection::Bidirectional);
        assert_eq!(
            preview_event.client_to_server_params_type,
            Some(WireType::PreviewEventParams)
        );
        assert_eq!(
            preview_event.server_to_client_params_type,
            Some(WireType::ServerPreviewEventParams)
        );
        assert_eq!(
            serde_json::to_value(WireType::RenderPublication).unwrap(),
            "renderPublication"
        );
    }

    #[test]
    fn navigation_and_editor_positions_have_typed_camel_case_wire_forms() {
        let position = fleximark_model::SourcePosition {
            line: 2.into(),
            character: 4.into(),
            encoding: fleximark_model::PositionEncoding::Utf8,
        };
        let entry = NavigationEntry {
            node_id: NodeId("block-a".into()),
            source_range: SourceRange {
                byte_start: 7.into(),
                byte_end: 9.into(),
                start: position.clone(),
                end: position,
            },
            depth: 1,
        };
        let wire = serde_json::to_value(entry).unwrap();
        assert_eq!(wire["nodeId"], "block-a");
        assert_eq!(wire["sourceRange"]["byteStart"], 7);
        assert_eq!(wire["sourceRange"]["start"]["encoding"], "utf8");

        let selection: SetSelectionParams = serde_json::from_value(json!({
            "daemonInstanceId":"daemon",
            "documentSessionId":"document",
            "expectedDocumentVersion":1,
            "selections":[{"anchor":{"line":0,"character":1},"active":{"line":0,"character":2}}]
        }))
        .unwrap();
        assert_eq!(selection.selections[0].active.character, 2);
        assert_eq!(
            serde_json::to_value(PreviewNavigationEvent::SelectNode {
                preview_session_id: "preview".to_owned(),
                render_revision: 4.into(),
                node_id: NodeId("block-a".into())
            })
            .unwrap(),
            json!({"type":"selectNode","previewSessionId":"preview","renderRevision":4,"nodeId":"block-a"})
        );
        assert_eq!(
            serde_json::to_value(RenderNavigationEvent::Viewport {
                preview_session_id: "preview".into(),
                render_revision: 4.into(),
                node_id: NodeId("block-a".into())
            })
            .unwrap(),
            json!({"type":"viewport","previewSessionId":"preview","renderRevision":4,"nodeId":"block-a"})
        );
        assert_eq!(
            serde_json::to_value(ServerPreviewEventParams {
                daemon_instance_id: "daemon".into(),
                preview_session_id: "preview".into(),
                render_revision: 4.into(),
                event: ServerPreviewEvent::<Value>::RenderNavigation(
                    RenderNavigationEvent::Viewport {
                        preview_session_id: "preview".into(),
                        render_revision: 4.into(),
                        node_id: NodeId("block-a".into()),
                    },
                ),
            })
            .unwrap(),
            json!({"daemonInstanceId":"daemon","previewSessionId":"preview","renderRevision":4,
                "event":{"type":"viewport","previewSessionId":"preview","renderRevision":4,"nodeId":"block-a"}})
        );
    }

    #[test]
    fn inbound_dtos_reject_fields_not_declared_by_the_wire_schema() {
        assert!(
            serde_json::from_value::<InitializeParams>(json!({
                "protocolVersion":1,
                "client":{"name":"test","version":"1"},
                "unexpected":true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<SetSelectionParams>(json!({
                "daemonInstanceId":"d","documentSessionId":"s","expectedDocumentVersion":1,
                "selections":[{"anchor":{"line":0,"character":0,"unexpected":true},
                    "active":{"line":0,"character":0}}]
            }))
            .is_err()
        );
    }

    #[test]
    fn open_document_enforces_javascript_safe_integer_boundaries() {
        let params = |document_version| {
            json!({
                "daemonInstanceId": "daemon",
                "uri": "file:///document.md",
                "documentVersion": document_version,
                "text": "text"
            })
        };
        for version in [-MAX_SAFE_INTEGER, MAX_SAFE_INTEGER] {
            let decoded: RpcOpenDocumentParams =
                serde_json::from_value(params(version)).expect("safe boundary is accepted");
            assert_eq!(decoded.document_version.get(), version);
        }
        for version in [-MAX_SAFE_INTEGER - 1, MAX_SAFE_INTEGER + 1] {
            assert!(serde_json::from_value::<RpcOpenDocumentParams>(params(version)).is_err());
        }
    }
}
