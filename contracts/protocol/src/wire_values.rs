//! Role: Define protocol envelopes and document session DTOs independently of registries.
use crate::command_values::PreviewTarget;
use crate::rendering::*;
#[cfg(feature = "contract-schema")]
use crate::schema;
use fleximark_wire::{JsSafeI64, JsSafeU64};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
pub const PROTOCOL_VERSION: u32 = 1;
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
        match id {
            RpcId::Integer(value) => Value::Number(value.get().into()),
            RpcId::String(value) => Value::String(value),
        }
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
    #[cfg_attr(feature = "contract-schema", schemars(extend("const" = PROTOCOL_VERSION)))]
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
    pub source_navigation_events: bool,
    #[serde(default)]
    pub selection_events: bool,
    #[serde(default)]
    pub viewport_events: bool,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    #[cfg_attr(feature = "contract-schema", schemars(extend("const" = PROTOCOL_VERSION)))]
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
    #[cfg_attr(feature = "contract-schema", schemars(transform = schema::non_null))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    pub html_render: bool,
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
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct AttachDocumentResult {
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

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct PreviewChangedParams {
    pub frame: RenderFrame,
    pub daemon_instance_id: String,
    pub preview_session_id: String,
    #[cfg_attr(feature = "contract-schema", schemars(extend("minimum" = 1)))]
    pub render_revision: JsSafeU64,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RerenderPreviewParams {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
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
        #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
        preview_session_id: String,
        #[cfg_attr(feature = "contract-schema", schemars(extend("minimum" = 1)))]
        render_revision: JsSafeU64,
        #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
        node_id: NodeId,
    },
    RevealNode {
        #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
        preview_session_id: String,
        #[cfg_attr(feature = "contract-schema", schemars(extend("minimum" = 1)))]
        render_revision: JsSafeU64,
        #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
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
        #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
        preview_session_id: String,
        #[cfg_attr(feature = "contract-schema", schemars(extend("minimum" = 1)))]
        render_revision: JsSafeU64,
        #[cfg_attr(feature = "contract-schema", schemars(extend("uniqueItems" = true, "items" = {"type":"string","minLength":1})))]
        node_ids: Vec<NodeId>,
        #[cfg_attr(
            feature = "contract-schema",
            schemars(skip_serializing_if = "Option::is_none")
        )]
        active_position: Option<TextPosition>,
    },
    Viewport {
        #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
        preview_session_id: String,
        #[cfg_attr(feature = "contract-schema", schemars(extend("minimum" = 1)))]
        render_revision: JsSafeU64,
        #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
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
    #[cfg_attr(feature = "contract-schema", schemars(extend("minimum" = 1)))]
    pub render_revision: JsSafeU64,
    pub event: PreviewNavigationEvent,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum ServerPreviewEvent {
    RenderNavigation(RenderNavigationEvent),
    SourceNavigation(SourceNavigationEvent),
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct ServerPreviewEventParams {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
    #[cfg_attr(feature = "contract-schema", schemars(extend("minimum" = 1)))]
    pub render_revision: JsSafeU64,
    pub event: ServerPreviewEvent,
}
