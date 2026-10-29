//! Role: Verify independent wire examples against the current contract.
use fleximark_protocol::{
    CommandMessage, CommandResult, CreatePreviewResult, InitializeParams, InitializeResult,
    ServerCapabilities, WorkspaceCommandRequest, WorkspaceStatus,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    schema_version: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    definition: String,
    value: Value,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../fixtures/optional-present.json")).unwrap()
}

fn case<'a>(fixture: &'a Fixture, definition: &str) -> &'a Value {
    &fixture
        .cases
        .iter()
        .find(|item| item.definition == definition)
        .unwrap()
        .value
}

#[test]
fn optional_present_fixture_matches_directional_public_dtos() {
    let fixture = fixture();
    assert_eq!(fixture.schema_version, fleximark_protocol::PROTOCOL_VERSION);
    assert_eq!(fixture.cases.len(), 5);

    let initialize: InitializeParams =
        serde_json::from_value(case(&fixture, "initializeParams").clone()).unwrap();
    assert!(initialize.capabilities.selection_events);
    assert!(initialize.capabilities.viewport_events);
    assert_eq!(initialize.workspaces.len(), 1);
    assert!(initialize.workspaces[0].trusted);

    let execute: WorkspaceCommandRequest =
        serde_json::from_value(case(&fixture, "workspaceCommandRequest").clone()).unwrap();
    let document = execute.context.document.unwrap();
    assert_eq!(document.document_session_id, "document");
    assert_eq!(document.expected_document_version.get(), 3);
    assert_eq!(
        execute.context.workspace_uri.as_deref(),
        Some("file:///workspace")
    );
    match execute.operation {
        fleximark_protocol::WorkspaceCommand::CreateNote {
            category_path,
            template,
            name,
        } => {
            assert_eq!(category_path, Some(vec!["Work".into()]));
            assert_eq!(template.as_deref(), Some("daily"));
            assert_eq!(name.as_deref(), Some("session"));
        }
        _ => panic!("expected create note"),
    }

    let initialize_result = InitializeResult {
        protocol_version: fleximark_protocol::PROTOCOL_VERSION,
        daemon_instance_id: "daemon".into(),
        workspace_statuses: vec![WorkspaceStatus {
            uri: "file:///workspace".into(),
            enabled: false,
            error: Some("invalid configuration".into()),
        }],
        capabilities: ServerCapabilities {
            html_render: true,
            selection_events: true,
            viewport_events: true,
            workspace_commands: vec!["editTheme"],
        },
    };
    assert_eq!(
        serde_json::to_value(initialize_result).unwrap(),
        *case(&fixture, "initializeResult")
    );

    let create_preview = case(&fixture, "createPreviewResult");
    let frame: fleximark_protocol::RenderFrame =
        serde_json::from_value(create_preview["frame"].clone()).unwrap();
    assert_eq!(
        create_preview["previewSessionId"],
        json!(frame.preview_session_id.0)
    );
    let create_preview_result = CreatePreviewResult {
        preview_session_id: frame.preview_session_id.0.clone(),
        frame,
        url: Some("http://127.0.0.1:3000/preview".into()),
    };
    assert_eq!(
        serde_json::to_value(create_preview_result).unwrap(),
        *create_preview
    );

    let command_result = CommandResult::NoteCreated {
        message: Some(CommandMessage {
            level: fleximark_protocol::CommandMessageLevel::Warning,
            text: "review note".into(),
        }),
        uri: "file:///workspace/note.md".into(),
    };
    assert_eq!(
        serde_json::to_value(command_result).unwrap(),
        *case(&fixture, "commandResult")
    );

    assert_eq!(
        case(&fixture, "initializeParams")["protocolVersion"],
        json!(fleximark_protocol::PROTOCOL_VERSION)
    );
}

#[test]
fn note_category_path_rejects_present_invalid_values() {
    let base =
        json!({"daemonInstanceId":"daemon","context":{},"operation":{"command":"createNote"}});
    let too_deep = Value::Array((0..33).map(|_| Value::String("x".into())).collect());
    for invalid in [Value::Null, json!([]), too_deep, json!(["Work", ""])] {
        let mut value = base.clone();
        value["operation"]["categoryPath"] = invalid;
        assert!(serde_json::from_value::<WorkspaceCommandRequest>(value).is_err());
    }
    let missing = serde_json::from_value::<WorkspaceCommandRequest>(base).unwrap();
    assert!(matches!(
        missing.operation,
        fleximark_protocol::WorkspaceCommand::CreateNote {
            category_path: None,
            name: None,
            ..
        }
    ));
}

#[test]
fn note_file_name_rejects_present_invalid_values() {
    let base =
        json!({"daemonInstanceId":"daemon","context":{},"operation":{"command":"createNote"}});
    for invalid in [Value::Null, json!(""), json!("x".repeat(256))] {
        let mut value = base.clone();
        value["operation"]["name"] = invalid;
        assert!(serde_json::from_value::<WorkspaceCommandRequest>(value).is_err());
    }
}
