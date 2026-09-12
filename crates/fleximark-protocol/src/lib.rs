use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub use fleximark_model::{NavigationEntry, NodeId, SourceRange};

pub const PROTOCOL_VERSION: u32 = 1;
pub const CONTENT_MODIFIED: i64 = -32801;
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

pub mod method {
    pub const INITIALIZE: &str = "fleximark/initialize";
    pub const ATTACH_DOCUMENT: &str = "fleximark/attachDocument";
    pub const CHECKPOINT_DOCUMENT: &str = "fleximark/checkpointDocument";
    pub const REQUEST_FULL_TEXT: &str = "fleximark/requestFullText";
    pub const RENDER: &str = "fleximark/render";
    pub const CREATE_PREVIEW: &str = "fleximark/createPreview";
    pub const DISPOSE_PREVIEW: &str = "fleximark/disposePreview";
    pub const SET_SELECTION: &str = "fleximark/setSelection";
    pub const SET_VIEWPORT: &str = "fleximark/setViewport";
    pub const PREVIEW_EVENT: &str = "fleximark/previewEvent";
    pub const RELOAD_PREVIEW: &str = "fleximark/reloadPreview";
    pub const EXECUTE_COMMAND: &str = "fleximark/executeCommand";
    pub const GET_NOTE_OPTIONS: &str = "fleximark/getNoteOptions";
    pub const RECONFIGURE_WORKSPACE: &str = "fleximark/reconfigureWorkspace";
    pub const OPEN_DOCUMENT: &str = "fleximark/openDocument";
    pub const CHANGE_DOCUMENT: &str = "fleximark/changeDocument";
    pub const CLOSE_DOCUMENT: &str = "fleximark/closeDocument";
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncomingMessage {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

impl Response {
    pub fn success(id: Value, result: impl Serialize) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(serde_json::to_value(result).expect("serializable protocol result")),
            error: None,
        }
    }

    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(ResponseError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ResponseError {
    pub code: i64,
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
#[serde(deny_unknown_fields)]
pub struct WorkspaceGrant {
    pub uri: String,
    pub trusted: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReconfigureWorkspaceParams {
    pub daemon_instance_id: String,
    pub workspace_uri: String,
    pub trusted: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
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
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: u32,
    pub daemon_instance_id: String,
    pub capabilities: ServerCapabilities,
    pub workspace_statuses: Vec<WorkspaceStatus>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatus {
    pub uri: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    pub html_render: bool,
    pub document_checkpoint: bool,
    pub selection_events: bool,
    pub viewport_events: bool,
    pub workspace_commands: Vec<&'static str>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachDocumentParams {
    pub daemon_instance_id: String,
    pub uri: String,
    pub expected_document_version: i64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachDocumentResult {
    pub document_session_id: String,
    pub document_version: i64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckpointDocumentParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub document_version: i64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointDocumentResult {
    pub document_version: i64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcOpenDocumentParams {
    pub daemon_instance_id: String,
    pub uri: String,
    pub document_version: i64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcChangeDocumentParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub base_document_version: i64,
    pub base_content_hash: String,
    pub document_version: i64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RpcCloseDocumentParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFullTextParams {
    pub daemon_instance_id: String,
    pub uri: String,
    pub document_session_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub document_version: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreatePreviewParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub expected_document_version: i64,
    pub target: PreviewTarget,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PreviewTarget {
    EmbeddedHtml,
    ExternalBrowser,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePreviewResult<T> {
    pub preview_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub initial_publication: T,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisposePreviewParams {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetSelectionParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub expected_document_version: i64,
    pub selections: Vec<TextSelection>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextPosition {
    pub line: u64,
    pub character: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextSelection {
    pub anchor: TextPosition,
    pub active: TextPosition,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PreviewNavigationEvent {
    SelectNode {
        preview_session_id: String,
        render_revision: u64,
        node_id: NodeId,
    },
    RevealNode {
        preview_session_id: String,
        render_revision: u64,
        node_id: NodeId,
    },
}

#[derive(Clone, Debug, Serialize)]
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
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RenderNavigationEvent {
    Selection {
        preview_session_id: String,
        render_revision: u64,
        node_ids: Vec<NodeId>,
        active_position: Option<TextPosition>,
    },
    Viewport {
        preview_session_id: String,
        render_revision: u64,
        node_id: NodeId,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetViewportParams {
    pub daemon_instance_id: String,
    pub document_session_id: String,
    pub expected_document_version: i64,
    pub ranges: Vec<TextRange>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewEventParams {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
    pub render_revision: u64,
    pub event: PreviewNavigationEvent,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReloadPreviewParams {
    pub daemon_instance_id: String,
    pub preview_session_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteCommandParams {
    pub daemon_instance_id: String,
    pub command: String,
    #[serde(default)]
    pub document_session_id: Option<String>,
    #[serde(default)]
    pub expected_document_version: Option<i64>,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetNoteOptionsParams {
    pub daemon_instance_id: String,
    pub workspace_uri: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct GetNoteOptionsResult {
    pub categories: Vec<String>,
    pub templates: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<CommandMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_uri: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandMessage {
    pub level: &'static str,
    pub text: String,
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

    fn accept_contract_params(case: &ContractCase) {
        macro_rules! accept {
            ($type:ty) => {
                serde_json::from_value::<$type>(case.params.clone()).unwrap()
            };
        }

        match case.method.as_str() {
            method::INITIALIZE => {
                let params = accept!(InitializeParams);
                assert!(!params.capabilities.embedded_html);
                assert!(params.workspaces.is_empty());
            }
            method::ATTACH_DOCUMENT => drop(accept!(AttachDocumentParams)),
            method::CHECKPOINT_DOCUMENT => drop(accept!(CheckpointDocumentParams)),
            method::REQUEST_FULL_TEXT => {
                let params = RequestFullTextParams {
                    daemon_instance_id: case.params["daemonInstanceId"].as_str().unwrap().into(),
                    uri: case.params["uri"].as_str().unwrap().into(),
                    document_session_id: case.params["documentSessionId"].as_str().unwrap().into(),
                    reason: case.params["reason"].as_str().unwrap().into(),
                };
                assert_eq!(serde_json::to_value(params).unwrap(), case.params);
            }
            method::RENDER => drop(accept!(RenderParams)),
            method::CREATE_PREVIEW => {
                let params = accept!(CreatePreviewParams);
                assert_eq!(params.target, PreviewTarget::ExternalBrowser);
            }
            method::DISPOSE_PREVIEW => drop(accept!(DisposePreviewParams)),
            method::SET_SELECTION => drop(accept!(SetSelectionParams)),
            method::SET_VIEWPORT => drop(accept!(SetViewportParams)),
            method::PREVIEW_EVENT => drop(accept!(PreviewEventParams)),
            method::RELOAD_PREVIEW => drop(accept!(ReloadPreviewParams)),
            method::EXECUTE_COMMAND => {
                let params = accept!(ExecuteCommandParams);
                assert!(params.document_session_id.is_none());
                assert!(params.expected_document_version.is_none());
                assert!(params.workspace_uri.is_none());
                assert!(params.destination_uri.is_none());
                assert!(params.note_category.is_none());
                assert!(params.note_template.is_none());
            }
            method::GET_NOTE_OPTIONS => drop(accept!(GetNoteOptionsParams)),
            method::RECONFIGURE_WORKSPACE => drop(accept!(ReconfigureWorkspaceParams)),
            method::OPEN_DOCUMENT => drop(accept!(RpcOpenDocumentParams)),
            method::CHANGE_DOCUMENT => drop(accept!(RpcChangeDocumentParams)),
            method::CLOSE_DOCUMENT => drop(accept!(RpcCloseDocumentParams)),
            method => panic!("fixture contains unknown method {method}"),
        }
    }

    fn serialize_contract_result(case: &ContractCase) -> Value {
        let string = |field: &str| case.result[field].as_str().unwrap().to_owned();
        let integer = |field: &str| case.result[field].as_i64().unwrap();
        match case.method.as_str() {
            method::INITIALIZE => serde_json::to_value(InitializeResult {
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
            method::ATTACH_DOCUMENT | method::OPEN_DOCUMENT => {
                serde_json::to_value(AttachDocumentResult {
                    document_session_id: string("documentSessionId"),
                    document_version: integer("documentVersion"),
                    content_hash: string("contentHash"),
                })
                .unwrap()
            }
            method::CHECKPOINT_DOCUMENT | method::CHANGE_DOCUMENT => {
                serde_json::to_value(CheckpointDocumentResult {
                    document_version: integer("documentVersion"),
                    content_hash: string("contentHash"),
                })
                .unwrap()
            }
            method::CREATE_PREVIEW => serde_json::to_value(CreatePreviewResult {
                preview_session_id: string("previewSessionId"),
                url: case
                    .result
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                initial_publication: case.result["initialPublication"].clone(),
            })
            .unwrap(),
            method::GET_NOTE_OPTIONS => serde_json::to_value(GetNoteOptionsResult {
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
            method::EXECUTE_COMMAND => serde_json::to_value(CommandResult {
                message: None,
                open_uri: None,
            })
            .unwrap(),
            method::RENDER
            | method::REQUEST_FULL_TEXT
            | method::DISPOSE_PREVIEW
            | method::SET_SELECTION
            | method::SET_VIEWPORT
            | method::PREVIEW_EVENT
            | method::RELOAD_PREVIEW
            | method::RECONFIGURE_WORKSPACE
            | method::CLOSE_DOCUMENT => case.result.clone(),
            method => panic!("fixture contains unknown method {method}"),
        }
    }

    #[test]
    fn every_custom_method_matches_the_shared_wire_fixture() {
        let fixture = contract_fixture();
        assert_eq!(fixture.schema_version, PROTOCOL_VERSION);
        let expected = HashSet::from([
            method::INITIALIZE,
            method::ATTACH_DOCUMENT,
            method::CHECKPOINT_DOCUMENT,
            method::REQUEST_FULL_TEXT,
            method::RENDER,
            method::CREATE_PREVIEW,
            method::DISPOSE_PREVIEW,
            method::SET_SELECTION,
            method::SET_VIEWPORT,
            method::PREVIEW_EVENT,
            method::RELOAD_PREVIEW,
            method::EXECUTE_COMMAND,
            method::GET_NOTE_OPTIONS,
            method::RECONFIGURE_WORKSPACE,
            method::OPEN_DOCUMENT,
            method::CHANGE_DOCUMENT,
            method::CLOSE_DOCUMENT,
        ]);
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
            accept_contract_params(case);
            let result = serialize_contract_result(case);
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
    fn navigation_and_editor_positions_have_typed_camel_case_wire_forms() {
        let position = fleximark_model::SourcePosition {
            line: 2,
            character: 4,
            encoding: fleximark_model::PositionEncoding::Utf8,
        };
        let entry = NavigationEntry {
            node_id: NodeId("block-a".into()),
            source_range: SourceRange {
                byte_start: 7,
                byte_end: 9,
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
                render_revision: 4,
                node_id: NodeId("block-a".into())
            })
            .unwrap(),
            json!({"type":"selectNode","previewSessionId":"preview","renderRevision":4,"nodeId":"block-a"})
        );
        assert_eq!(
            serde_json::to_value(RenderNavigationEvent::Viewport {
                preview_session_id: "preview".into(),
                render_revision: 4,
                node_id: NodeId("block-a".into())
            })
            .unwrap(),
            json!({"type":"viewport","previewSessionId":"preview","renderRevision":4,"nodeId":"block-a"})
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
}
