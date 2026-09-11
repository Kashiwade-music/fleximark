use fleximark_protocol::{
    CommandMessage, CommandResult, CreatePreviewResult, ExecuteCommandParams, InitializeParams,
    InitializeResult, ServerCapabilities, WorkspaceStatus,
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
    serde_json::from_str(include_str!(
        "../../../test/fixtures/protocol-v1-optional-present.json"
    ))
    .unwrap()
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
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.cases.len(), 5);

    let initialize: InitializeParams =
        serde_json::from_value(case(&fixture, "initializeParams").clone()).unwrap();
    assert!(initialize.capabilities.embedded_html);
    assert!(!initialize.capabilities.structured_preview);
    assert!(initialize.capabilities.selection_events);
    assert!(initialize.capabilities.viewport_events);
    assert!(initialize.capabilities.open_external);
    assert_eq!(initialize.workspaces.len(), 1);
    assert!(initialize.workspaces[0].trusted);

    let execute: ExecuteCommandParams =
        serde_json::from_value(case(&fixture, "executeCommandParams").clone()).unwrap();
    assert_eq!(execute.document_session_id.as_deref(), Some("document"));
    assert_eq!(execute.expected_document_version, Some(3));
    assert_eq!(execute.workspace_uri.as_deref(), Some("file:///workspace"));
    assert_eq!(
        execute.destination_uri.as_deref(),
        Some("file:///workspace/note.md")
    );
    assert_eq!(execute.note_category.as_deref(), Some("work"));
    assert_eq!(execute.note_template.as_deref(), Some("daily"));

    let initialize_result = InitializeResult {
        protocol_version: 1,
        daemon_instance_id: "daemon".into(),
        workspace_statuses: vec![WorkspaceStatus {
            uri: "file:///workspace".into(),
            enabled: false,
            error: Some("invalid configuration".into()),
        }],
        capabilities: ServerCapabilities {
            html_render: true,
            document_checkpoint: true,
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
    let create_preview_result = CreatePreviewResult {
        preview_session_id: "preview".into(),
        url: Some("http://127.0.0.1:3000/preview".into()),
        initial_publication: create_preview["initialPublication"].clone(),
    };
    assert_eq!(
        serde_json::to_value(create_preview_result).unwrap(),
        *create_preview
    );

    let command_result = CommandResult {
        message: Some(CommandMessage {
            level: "warning",
            text: "review note".into(),
        }),
        open_uri: Some("file:///workspace/note.md".into()),
    };
    assert_eq!(
        serde_json::to_value(command_result).unwrap(),
        *case(&fixture, "commandResult")
    );

    assert_eq!(
        case(&fixture, "initializeParams")["protocolVersion"],
        json!(1)
    );
}
