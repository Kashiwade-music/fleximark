//! Role: Verify command coverage, typed outcomes and strict request decoding.
use super::*;
use serde_json::{Value, json};
use std::collections::HashSet;

fn commands() -> Vec<WorkspaceCommand> {
    vec![
        WorkspaceCommand::InitializeWorkspace {},
        WorkspaceCommand::EditTheme {},
        WorkspaceCommand::CreateNote {
            category_path: Some(vec!["Work".into()]),
            template: Some("daily".into()),
            name: Some("note".into()),
        },
        WorkspaceCommand::GetNoteOptions {},
        WorkspaceCommand::CollectAdmonitions {},
        WorkspaceCommand::ExportHtml {
            destination_uri: Some("file:///output".into()),
        },
        WorkspaceCommand::AcknowledgeExport {
            destination_uri: None,
        },
        WorkspaceCommand::PlanMigration {
            settings_uri: None,
            recover: false,
        },
        WorkspaceCommand::ApplyMigration {
            plan_id: "plan".into(),
        },
        WorkspaceCommand::OpenPreview {
            target: PreviewTarget::ExternalBrowser,
            source_offset: Some(JsSafeU64::new(3).unwrap()),
        },
        WorkspaceCommand::OpenWorkspace {},
    ]
}
#[test]
fn every_command_roundtrips_and_has_unique_complete_metadata() {
    let mut names = HashSet::new();
    let mut lsp_names = HashSet::new();
    let commands = commands();
    assert_eq!(commands.len(), COMMAND_DESCRIPTORS.len());
    for command in commands {
        let descriptor = command.descriptor();
        assert!(names.insert(descriptor.name));
        assert!(lsp_names.insert(descriptor.lsp_command));
        assert!(!descriptor.title.is_empty());
        assert_eq!(descriptor.kind, command.kind());
        let value = serde_json::to_value(&command).unwrap();
        assert_eq!(value["command"], descriptor.name);
        let decoded: WorkspaceCommand = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        let mut invalid = value;
        invalid["unknownField"] = json!(true);
        assert!(serde_json::from_value::<WorkspaceCommand>(invalid).is_err());
        if descriptor.requires_trust {
            assert!(descriptor.requires_workspace);
        }
        if matches!(command, WorkspaceCommand::OpenPreview { .. }) {
            assert!(descriptor.requires_document);
            assert!(!descriptor.requires_trust);
            assert!(!descriptor.requires_workspace);
        }
    }
}
#[test]
fn all_outcomes_roundtrip_with_one_message_shape() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/optional-present.json")).unwrap();
    let preview = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["definition"] == "createPreviewResult")
        .unwrap()["value"]
        .clone();
    let cases = vec![
        json!({"kind":"workspaceInitialized","configUri":"config","themeUri":"theme"}),
        json!({"kind":"themeEdited","uri":"theme"}),
        json!({"kind":"noteCreated","uri":"note"}),
        json!({"kind":"noteOptions","options":{"categories":[],"templates":[]}}),
        json!({"kind":"admonitionsCollected","uri":"note","count":3}),
        json!({"kind":"htmlExported","outputUri":"out","generation":1}),
        json!({"kind":"exportAcknowledged"}),
        json!({"kind":"migrationPlanned","plan":{"planId":"p","workspaceUri":"w","status":"ready","outputs":[],"warnings":[],"config":"config","recovery":false}}),
        json!({"kind":"migrationApplied","backupUri":null,"recovered":false}),
        json!({"kind":"previewOpened","preview":preview}),
        json!({"kind":"workspaceOpened","url":"http://127.0.0.1/"}),
    ];
    assert_eq!(cases.len(), COMMAND_DESCRIPTORS.len());
    for mut case in cases {
        case["message"] = json!({"level":"info","text":"completed"});
        let result: CommandResult = serde_json::from_value(case.clone()).unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(), case);
    }
}
#[test]
fn envelopes_reject_legacy_shapes_empty_identifiers_and_incomplete_documents() {
    let base = json!({"daemonInstanceId":"daemon","context":{},"operation":commands()[0]});
    assert!(serde_json::from_value::<WorkspaceCommandRequest>(base.clone()).is_ok());
    for context in [
        json!({"workspaceUri":""}),
        json!({"workspaceUri":null}),
        json!({"document":null}),
        json!({"document":{"documentSessionId":"s"}}),
        json!({"document":{"expectedDocumentVersion":1}}),
        json!({"document":{"documentSessionId":"","expectedDocumentVersion":1}}),
        json!({"extra":true}),
    ] {
        let mut value = base.clone();
        value["context"] = context;
        assert!(serde_json::from_value::<WorkspaceCommandRequest>(value).is_err());
    }
    for value in [
        json!({"daemonInstanceId":"daemon","command":"initializeWorkspace"}),
        json!({"daemonInstanceId":"daemon","context":{},"operation":{"command":"migrate"}}),
        json!({"daemonInstanceId":"","context":{},"operation":commands()[0]}),
    ] {
        assert!(serde_json::from_value::<WorkspaceCommandRequest>(value).is_err());
    }
    for value in [
        json!({"documentVersion":null}),
        json!({"documentVersion":9007199254740992_i64}),
        json!({"unknown":true}),
    ] {
        assert!(serde_json::from_value::<WorkspaceUiContext>(value).is_err());
    }
}

#[test]
fn workspace_error_is_a_strict_message_object() {
    let value = json!({"error":"workspace unavailable"});
    let error: WorkspaceUiError = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(error).unwrap(), value);
    assert!(
        serde_json::from_value::<WorkspaceUiError>(json!({"error":"failed","extra":true})).is_err()
    );
    assert!(serde_json::from_value::<WorkspaceUiError>(json!({"error":null})).is_err());
    assert!(serde_json::from_value::<PreviewTarget>(json!("embeddedHtml")).is_ok());
}
