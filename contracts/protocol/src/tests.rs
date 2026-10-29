//! Role: Verify current protocol framing, validation and independent contract fixtures.
fn accept<T: serde::de::DeserializeOwned>(case: &ContractCase) -> T {
    serde_json::from_value(case.params.clone()).unwrap()
}

use std::collections::HashSet;
use std::io::{BufReader, Cursor};

use serde::Deserialize;
use serde_json::json;

use super::*;

#[test]
fn outgoing_frames_can_exceed_the_incoming_control_message_limit() {
    let message = json!({
        "jsonrpc": "2.0", "id": 1,
        "result": { "data": "A".repeat(MAX_MESSAGE_BYTES + 1) }
    });
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &message).unwrap();
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let body = &bytes[header_end + 4..];
    assert!(body.len() > MAX_MESSAGE_BYTES);
    assert_eq!(
        &bytes[..header_end],
        format!("Content-Length: {}", body.len()).as_bytes()
    );
    assert_eq!(serde_json::from_slice::<Value>(body).unwrap(), message);
    assert!(matches!(
        read_frame(&mut Cursor::new(bytes)),
        Err(FrameError::MessageTooLarge)
    ));
}

#[test]
fn rpc_id_domain_accepts_safe_values_and_rejects_invalid_values() {
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
        serde_json::to_value(Response::error(
            Value::Null,
            crate::error_code::PARSE_ERROR,
            "parse error"
        ))
        .unwrap(),
        json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": { "code": crate::error_code::PARSE_ERROR, "message": "parse error" }
        })
    );
}

#[test]
fn framing_handles_back_to_back_messages_and_length_errors() {
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
    let mut missing = Cursor::new(b"Other: x\r\n\r\n".as_slice());
    assert!(matches!(
        read_frame(&mut missing),
        Err(FrameError::MissingContentLength)
    ));
    let mut duplicate = Cursor::new(b"Content-Length: 0\r\ncontent-length: 0\r\n\r\n".as_slice());
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
        "../../../tests/fixtures/protocol/contract.json"
    ))
    .unwrap()
}

fn accept_contract_params(case: &ContractCase, spec: &MethodSpec) {
    let params_type = spec
        .client_to_server_params_type
        .or(spec.server_to_client_params_type)
        .unwrap();
    match params_type {
        WireType::InitializeParams => {
            let params = accept::<InitializeParams>(case);
            assert!(!params.capabilities.embedded_html);
            assert!(params.workspaces.is_empty());
        }
        WireType::AttachDocumentParams => drop(accept::<AttachDocumentParams>(case)),
        WireType::CreatePreviewParams => {
            let params = accept::<CreatePreviewParams>(case);
            assert_eq!(params.target, PreviewTarget::ExternalBrowser);
        }
        WireType::RerenderPreviewParams => drop(accept::<RerenderPreviewParams>(case)),
        WireType::PreviewChangedParams => {
            let params = PreviewChangedParams {
                frame: serde_json::from_value(case.params["frame"].clone()).unwrap(),
                daemon_instance_id: case.params["daemonInstanceId"].as_str().unwrap().into(),
                preview_session_id: case.params["previewSessionId"].as_str().unwrap().into(),
                render_revision: JsSafeU64::new(case.params["renderRevision"].as_u64().unwrap())
                    .unwrap(),
            };
            assert_eq!(serde_json::to_value(params).unwrap(), case.params);
        }
        WireType::DisposePreviewParams => drop(accept::<DisposePreviewParams>(case)),
        WireType::SetSelectionParams => drop(accept::<SetSelectionParams>(case)),
        WireType::SetViewportParams => drop(accept::<SetViewportParams>(case)),
        WireType::PreviewEventParams => drop(accept::<PreviewEventParams>(case)),
        WireType::ExecuteCommandParams => {
            let params = accept::<ExecuteCommandParams>(case);
            assert!(params.document_session_id.is_none());
            assert!(params.expected_document_version.is_none());
            assert!(params.workspace_uri.is_none());
            assert!(params.destination_uri.is_none());
            assert!(params.note_category_path.is_none());
            assert!(params.note_template.is_none());
        }
        WireType::GetNoteOptionsParams => drop(accept::<GetNoteOptionsParams>(case)),
        WireType::ReconfigureWorkspaceParams => drop(accept::<ReconfigureWorkspaceParams>(case)),
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
        })
        .unwrap(),
        Some(WireType::CreatePreviewResult) => serde_json::to_value(CreatePreviewResult {
            frame: serde_json::from_value(case.result["frame"].clone()).unwrap(),
            preview_session_id: string("previewSessionId"),
            url: case
                .result
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
        .unwrap(),
        Some(WireType::RenderFrame) => case.result.clone(),
        Some(WireType::GetNoteOptionsResult) => serde_json::to_value(GetNoteOptionsResult {
            categories: case.result["categories"]
                .as_array()
                .unwrap()
                .iter()
                .map(note_category_option_from_value)
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
            data: None,
        })
        .unwrap(),
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

fn note_category_option_from_value(value: &Value) -> NoteCategoryOption {
    NoteCategoryOption {
        name: value["name"].as_str().unwrap().to_owned(),
        children: value["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(note_category_option_from_value)
            .collect(),
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
        serde_json::to_value(WireType::RenderFrame).unwrap(),
        "renderFrame"
    );
}

#[test]
fn navigation_and_editor_positions_have_typed_camel_case_wire_forms() {
    let position = SourcePosition {
        line: 2.into(),
        character: 4.into(),
        encoding: PositionEncoding::Utf8,
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
            event: ServerPreviewEvent::RenderNavigation(RenderNavigationEvent::Viewport {
                preview_session_id: "preview".into(),
                render_revision: 4.into(),
                node_id: NodeId("block-a".into()),
            },),
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
            "protocolVersion":PROTOCOL_VERSION,
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
fn attachments_enforce_json_integer_boundaries() {
    let params = |document_version| {
        json!({
            "daemonInstanceId": "daemon",
            "uri": "file:///document.md",
            "expectedDocumentVersion": document_version
        })
    };
    for version in [-MAX_SAFE_INTEGER, MAX_SAFE_INTEGER] {
        let decoded: AttachDocumentParams =
            serde_json::from_value(params(version)).expect("safe boundary is accepted");
        assert_eq!(decoded.expected_document_version.get(), version);
    }
    for version in [-MAX_SAFE_INTEGER - 1, MAX_SAFE_INTEGER + 1] {
        assert!(serde_json::from_value::<AttachDocumentParams>(params(version)).is_err());
    }
}
