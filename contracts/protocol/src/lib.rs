//! Role: Define the sole current wire contract independently of processing internals.
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
mod registry;
mod rendering;
pub use fleximark_wire::{JsSafeI64, JsSafeU64, MAX_SAFE_INTEGER};
pub use registry::*;
pub use rendering::*;
pub const PROTOCOL_VERSION: u32 = 7;
pub const CONTENT_MODIFIED: i64 = -32801;
/// Limits incoming control messages; outgoing render frames may contain large assets.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
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
pub struct CreatePreviewResult {
    pub frame: RenderFrame,
    pub preview_session_id: String,
    #[cfg_attr(feature = "contract-schema", schemars(transform = schema::non_null))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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
    #[serde(default, deserialize_with = "deserialize_note_category_path")]
    #[cfg_attr(feature = "contract-schema", schemars(transform = schema::non_null, extend("minItems" = 1, "maxItems" = 32, "items" = {"type":"string","minLength":1})))]
    pub note_category_path: Option<Vec<String>>,
    #[serde(default)]
    pub note_template: Option<String>,
    #[serde(default, deserialize_with = "deserialize_note_file_name")]
    #[cfg_attr(feature = "contract-schema", schemars(transform = schema::non_null, extend("minLength" = 1, "maxLength" = 255)))]
    pub note_file_name: Option<String>,
    #[serde(default)]
    pub arguments: Vec<String>,
}

fn deserialize_note_file_name<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let name = String::deserialize(deserializer)?;
    if name.is_empty() || name.chars().count() > 255 {
        return Err(serde::de::Error::custom(
            "noteFileName must contain 1 to 255 characters",
        ));
    }
    Ok(Some(name))
}

fn deserialize_note_category_path<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let path = Vec::<String>::deserialize(deserializer)?;
    if path.is_empty() || path.len() > 32 || path.iter().any(String::is_empty) {
        return Err(serde::de::Error::custom(
            "noteCategoryPath must contain 1 to 32 non-empty segments",
        ));
    }
    Ok(Some(path))
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetNoteOptionsParams {
    pub daemon_instance_id: String,
    pub workspace_uri: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct NoteCategoryOption {
    #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
    pub name: String,
    pub children: Vec<NoteCategoryOption>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
pub struct GetNoteOptionsResult {
    #[cfg_attr(feature = "contract-schema", schemars(extend("uniqueItems" = true)))]
    pub categories: Vec<NoteCategoryOption>,
    #[cfg_attr(feature = "contract-schema", schemars(extend("uniqueItems" = true)))]
    pub templates: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    #[cfg_attr(feature = "contract-schema", schemars(transform = schema::non_null))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<CommandMessage>,
    #[cfg_attr(feature = "contract-schema", schemars(transform = schema::non_null))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_uri: Option<String>,
    #[cfg_attr(feature = "contract-schema", schemars(transform = schema::non_null))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
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

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    MalformedHeader,
    MissingContentLength,
    DuplicateContentLength,
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
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::MalformedHeader => f.write_str("malformed protocol header"),
            Self::MissingContentLength => f.write_str("missing Content-Length header"),
            Self::DuplicateContentLength => f.write_str("duplicate Content-Length header"),
            Self::MessageTooLarge => write!(
                f,
                "message is larger than the {MAX_MESSAGE_BYTES} byte limit"
            ),
        }
    }
}
impl std::error::Error for FrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "contract-schema")]
mod schema {
    //! Role: Express optional non-null wire fields in derived contract schemas.
    pub(super) fn non_null(schema: &mut schemars::Schema) {
        if let Some(types) = schema
            .get_mut("type")
            .and_then(serde_json::Value::as_array_mut)
        {
            types.retain(|kind| kind.as_str() != Some("null"));
            if types.len() == 1 {
                let kind = types[0].clone();
                schema.insert("type".into(), kind);
            }
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(variants) = schema
                .get_mut(key)
                .and_then(serde_json::Value::as_array_mut)
            {
                variants.retain(|variant| {
                    variant.get("type").and_then(serde_json::Value::as_str) != Some("null")
                });
                if variants.len() == 1 {
                    *schema = variants[0].clone().try_into().expect("schema variant");
                }
                break;
            }
        }
    }
}
