//! Role: Exercise every descriptor through custom, standard and browser boundaries.
mod support;
use fleximark_protocol::{COMMAND_DESCRIPTORS, CommandKind};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use support::*;
fn operation(kind: CommandKind, destination: &str, plan: &str) -> Value {
    match kind {
        CommandKind::InitializeWorkspace => json!({"command":"initializeWorkspace"}),
        CommandKind::EditTheme => json!({"command":"editTheme"}),
        CommandKind::CreateNote => json!({"command":"createNote","name":"new-note"}),
        CommandKind::GetNoteOptions => json!({"command":"getNoteOptions"}),
        CommandKind::CollectAdmonitions => json!({"command":"collectAdmonitions"}),
        CommandKind::ExportHtml => json!({"command":"exportHtml","destinationUri":destination}),
        CommandKind::AcknowledgeExport => {
            json!({"command":"acknowledgeExport","destinationUri":destination})
        }
        CommandKind::PlanMigration => {
            json!({"command":"planMigration","settingsUri":null,"recover":false})
        }
        CommandKind::ApplyMigration => json!({"command":"applyMigration","planId":plan}),
        CommandKind::OpenPreview => {
            json!({"command":"openPreview","target":"externalBrowser","sourceOffset":null})
        }
        CommandKind::OpenWorkspace => json!({"command":"openWorkspace"}),
    }
}
#[test]
fn every_command_round_trips_through_custom_standard_and_browser_dispatch() {
    for channel in ["custom", "standard", "browser"] {
        let root = test_directory(channel);
        let workspace = uri(&root);
        let document = uri(&root.join("document.md"));
        std::fs::write(root.join("document.md"), "# Saved").unwrap();
        let mut instance = native_server();
        initialize(&mut instance, json!([{"uri":workspace,"trusted":true}]));
        open(
            &mut instance,
            &document,
            "# Unsaved\n\n:::info\nCollect me\n:::\n",
            1,
        );
        let server = Arc::new(Mutex::new(instance));
        let weak = Arc::downgrade(&server);
        server
            .lock()
            .unwrap()
            .set_workspace_handler(Arc::new(move |request| {
                weak.upgrade()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .workspace_request(request)
            }));
        let browser_url = if channel == "browser" {
            let mut server = server.lock().unwrap();
            let request = command_request(
                &server,
                Some(&workspace),
                None,
                operation(CommandKind::OpenWorkspace, "", ""),
            );
            let response = server.request(10, fleximark_protocol::method::EXECUTE_COMMAND, request);
            Some(response[0]["result"]["url"].as_str().unwrap().to_owned())
        } else {
            None
        };
        let destination = uri(&root.join("published"));
        let mut plan = String::new();
        for descriptor in COMMAND_DESCRIPTORS {
            let operation = operation(descriptor.kind, &destination, &plan);
            let result = if let Some(url) = &browser_url {
                let body=json!({"context":{"workspaceUri":workspace,"documentUri":document,"documentVersion":1},"operation":operation}).to_string();
                let response = http(url, "/command", "POST", Some(&body));
                if !descriptor.browser_ui {
                    assert!(
                        response.starts_with("HTTP/1.1 400"),
                        "{}: {response}",
                        descriptor.name
                    );
                    continue;
                }
                assert!(
                    response.starts_with("HTTP/1.1 200"),
                    "{}: {response}",
                    descriptor.name
                );
                serde_json::from_str::<Value>(response.split_once("\r\n\r\n").unwrap().1).unwrap()
            } else {
                let mut server = server.lock().unwrap();
                let request =
                    command_request(&server, Some(&workspace), Some((&document, 1)), operation);
                let response = if channel == "standard" {
                    server.request(
                        11,
                        "workspace/executeCommand",
                        json!({"command":descriptor.lsp_command,"arguments":[request]}),
                    )
                } else {
                    server.request(11, fleximark_protocol::method::EXECUTE_COMMAND, request)
                };
                assert!(
                    response[0].get("result").is_some(),
                    "{channel}/{}: {response:?}",
                    descriptor.name
                );
                response[0]["result"].clone()
            };
            if descriptor.kind == CommandKind::CreateNote {
                assert!(
                    fleximark_application::WorkspaceAuthority::workspace_path(
                        result["uri"].as_str().unwrap()
                    )
                    .unwrap()
                    .exists()
                );
            }
            assert!(
                result["kind"].is_string(),
                "{channel}/{}: {result}",
                descriptor.name
            );
            if descriptor.kind == CommandKind::PlanMigration {
                plan = result["plan"]["planId"].as_str().unwrap().to_owned();
            }
        }
        assert!(root.join("published/index.html").exists());
    }
}
#[test]
fn typed_boundaries_reject_wrong_identity_context_stale_versions_and_old_arguments() {
    let root = test_directory("command-rejections");
    let workspace = uri(&root);
    initialize_workspace(&workspace).unwrap();
    let document = uri(&root.join("document.md"));
    std::fs::write(root.join("document.md"), "# Disk").unwrap();
    let mut server = native_server();
    initialize(&mut server, json!([{"uri":workspace,"trusted":true}]));
    open(&mut server, &document, "# Unsaved", 3);
    let valid = command_request(
        &server,
        Some(&workspace),
        Some((&document, 3)),
        operation(CommandKind::GetNoteOptions, "", ""),
    );
    for standard in [false, true] {
        for (field, value, code) in [
            (
                "daemonInstanceId",
                json!("other"),
                fleximark_protocol::error_code::INVALID_PARAMS,
            ),
            (
                "context",
                json!({"workspaceUri":workspace,"document":{"documentSessionId":session(&server,&document),"expectedDocumentVersion":2}}),
                fleximark_protocol::error_code::CONTENT_MODIFIED,
            ),
        ] {
            let mut request = valid.clone();
            request[field] = value;
            let response = if standard {
                server.request(
                    2,
                    "workspace/executeCommand",
                    json!({"command":"fleximark.getNoteOptions","arguments":[request]}),
                )
            } else {
                server.request(2, fleximark_protocol::method::EXECUTE_COMMAND, request)
            };
            assert_eq!(response[0]["error"]["code"], code, "{response:?}");
        }
    }
    let old=server.request(3,"workspace/executeCommand",json!({"command":"fleximark.openPreview","arguments":[{"documentUri":document,"documentVersion":3}]}));
    assert_eq!(
        old[0]["error"]["code"],
        fleximark_protocol::error_code::INVALID_PARAMS
    );
}

#[test]
fn migration_code_action_opens_the_authenticated_confirmation_ui_with_workspace_context() {
    let root = test_directory("migration-code-action-ui");
    let workspace = uri(&root);
    initialize_workspace(&workspace).unwrap();
    let document = uri(&root.join("document.md"));
    std::fs::write(root.join("document.md"), "# Disk").unwrap();
    let mut instance = native_server();
    initialize(&mut instance, json!([{"uri":workspace,"trusted":true}]));
    open(&mut instance, &document, "# Unsaved", 1);
    let actions = instance.request(
        20,
        "textDocument/codeAction",
        json!({"textDocument":{"uri":document}}),
    );
    let action = actions[0]["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| {
            action["command"]["command"] == CommandKind::PlanMigration.descriptor().lsp_command
        })
        .unwrap();
    let mut arguments = action["command"].clone();
    arguments.as_object_mut().unwrap().remove("title");
    let response = instance.request(21, "workspace/executeCommand", arguments);
    assert_eq!(response[0]["result"]["kind"], "migrationPlanned");
    assert_eq!(response[0]["result"]["plan"]["workspaceUri"], workspace);
    let show = response
        .iter()
        .find(|message| message["method"] == "window/showDocument")
        .expect("migration action opens its confirmation UI");
    assert_eq!(show["params"]["external"], true);
    assert_eq!(show["params"]["takeFocus"], true);
    let url = show["params"]["uri"].as_str().unwrap().to_owned();
    assert!(url.starts_with("http://127.0.0.1:"));
    let token = url.split("/workspace/").nth(1).unwrap();
    assert!(!token.is_empty());
    let server = Arc::new(Mutex::new(instance));
    let weak = Arc::downgrade(&server);
    server
        .lock()
        .unwrap()
        .set_workspace_handler(Arc::new(move |request| {
            weak.upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .workspace_request(request)
        }));
    assert!(http(&url, "", "GET", None).contains("id=\"workspace\""));
    let response = http(&url, "/state", "GET", None);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let state: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(state["command"], "planMigration");
    assert_eq!(state["context"]["workspaceUri"], workspace);
    assert!(state["context"].get("documentUri").is_none());
    assert!(state["context"].get("documentVersion").is_none());
}
