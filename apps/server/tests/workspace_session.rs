//! Role: Verify native multi-root trust, rendering configuration and asset authority.
mod support;
use fleximark_protocol::method;
use serde_json::json;
use support::*;

#[test]
fn nested_roots_keep_independent_trust_and_render_configuration() {
    let root = test_directory("multi-root");
    let trusted = root.join("trusted");
    let untrusted = trusted.join("nested-untrusted");
    std::fs::create_dir_all(&untrusted).unwrap();
    let trusted_uri = uri(&trusted);
    let untrusted_uri = uri(&untrusted);
    fleximark_application::native::initialize_workspace(&trusted_uri).unwrap();
    fleximark_application::native::initialize_workspace(&untrusted_uri).unwrap();
    std::fs::write(
        trusted.join(".fleximark/theme.css"),
        ":root { color: red; }",
    )
    .unwrap();
    std::fs::write(
        untrusted.join(".fleximark/theme.css"),
        ":root { color: blue; }",
    )
    .unwrap();
    let trusted_document = uri(&trusted.join("doc.md"));
    let untrusted_document = uri(&untrusted.join("doc.md"));
    std::fs::write(trusted.join("doc.md"), "# Trusted\n").unwrap();
    std::fs::write(untrusted.join("doc.md"), "# Untrusted\n").unwrap();
    let mut server = native_server();
    initialize(
        &mut server,
        json!([{"uri":trusted_uri,"trusted":true},{"uri":untrusted_uri,"trusted":false}]),
    );
    open(&mut server, &trusted_document, "# Trusted\n", 1);
    open(&mut server, &untrusted_document, "# Untrusted\n", 1);
    let trusted_preview = preview(&mut server, &trusted_document, 1, "embeddedHtml");
    let untrusted_preview = preview(&mut server, &untrusted_document, 1, "embeddedHtml");
    let trusted_frame = read_frame(
        &mut server,
        trusted_preview["previewSessionId"].as_str().unwrap(),
    );
    let untrusted_frame = read_frame(
        &mut server,
        untrusted_preview["previewSessionId"].as_str().unwrap(),
    );
    assert_eq!(trusted_frame["style"]["css"], ":root { color: red; }");
    assert!(untrusted_frame["style"].is_null());
    assert!(trusted_preview.get("url").is_none());
    assert!(untrusted_preview.get("url").is_none());
    let escalated=server.request(6,method::EXECUTE_COMMAND,json!({"daemonInstanceId":daemon(&server),"command":"exportHtml","documentSessionId":session(&server,&untrusted_document),"expectedDocumentVersion":1,"workspaceUri":trusted_uri}));
    assert!(
        escalated[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("different workspace authority")
    );
    std::fs::write(
        trusted.join(".fleximark/theme.css"),
        ":root { color: green; }",
    )
    .unwrap();
    let reconfigured = server.request(
        7,
        method::RECONFIGURE_WORKSPACE,
        json!({"daemonInstanceId":daemon(&server),"workspaceUri":trusted_uri,"trusted":true}),
    );
    assert!(reconfigured[0]["result"].is_null(), "{reconfigured:?}");
    assert!(
        reconfigured
            .iter()
            .any(|value| value["method"] == method::PREVIEW_CHANGED
                && value["params"]["renderRevision"] == 2)
    );
    let frame = read_frame(
        &mut server,
        trusted_preview["previewSessionId"].as_str().unwrap(),
    );
    assert_eq!(frame["style"]["css"], ":root { color: green; }");
}

#[test]
fn invalid_configuration_disables_only_its_root() {
    let root = test_directory("root-status");
    let valid = root.join("valid");
    let invalid = root.join("invalid");
    std::fs::create_dir_all(&valid).unwrap();
    std::fs::create_dir(&invalid).unwrap();
    let valid_uri = uri(&valid);
    let invalid_uri = uri(&invalid);
    fleximark_application::native::initialize_workspace(&valid_uri).unwrap();
    fleximark_application::native::initialize_workspace(&invalid_uri).unwrap();
    std::fs::write(
        invalid.join(".fleximark/config.toml"),
        "schema_version=2\nunknown=true\n",
    )
    .unwrap();
    let mut server = native_server();
    let result = initialize(
        &mut server,
        json!([{"uri":valid_uri,"trusted":true},{"uri":invalid_uri,"trusted":true}]),
    );
    let statuses = result["capabilities"]["experimental"]["fleximark"]["workspaceStatuses"]
        .as_array()
        .unwrap();
    assert_eq!(
        statuses
            .iter()
            .filter(|item| item["enabled"] == true)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|item| item["enabled"] == false)
            .count(),
        1
    );
    let valid_doc = uri(&valid.join("doc.md"));
    std::fs::write(valid.join("doc.md"), "# Valid\n").unwrap();
    open(&mut server, &valid_doc, "# Valid\n", 1);
    assert!(server.sessions().session_id_for_uri(&valid_doc).is_some());
    let invalid_doc = uri(&invalid.join("doc.md"));
    std::fs::write(invalid.join("doc.md"), "# Invalid\n").unwrap();
    open(&mut server, &invalid_doc, "# Invalid\n", 1);
    assert!(server.sessions().session_id_for_uri(&invalid_doc).is_none());
}

#[test]
fn workspace_writes_require_explicit_trust_before_command_arguments() {
    let root = test_directory("untrusted");
    let workspace_uri = uri(&root);
    let mut server = native_server();
    initialize(&mut server, json!([{"uri":workspace_uri,"trusted":false}]));
    for command in [
        "initializeWorkspace",
        "createNote",
        "collectAdmonitions",
        "exportHtml",
        "acknowledgeExport",
    ] {
        let result=server.request(2,method::EXECUTE_COMMAND,json!({"daemonInstanceId":daemon(&server),"command":command,"workspaceUri":workspace_uri}));
        assert_eq!(result[0]["error"]["code"], -32021, "{command}: {result:?}");
    }
    assert!(!root.join(".fleximark").exists());
    let migration = server.request(
        3,
        "workspace/executeCommand",
        json!({"command":"fleximark.migrate","arguments":[{"workspaceUri":workspace_uri}]}),
    );
    assert!(
        migration[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("trusted")
    );
}

#[test]
fn asset_errors_have_precise_ranges_without_blocking_safe_images() {
    let root = test_directory("assets");
    let workspace_uri = uri(&root);
    fleximark_application::native::initialize_workspace(&workspace_uri).unwrap();
    std::fs::write(root.join("valid.png"), b"\x89PNG\r\n\x1a\nvalid").unwrap();
    let document_uri = uri(&root.join("doc.md"));
    let text = "![missing](missing.png)\n\n![valid](valid.png)\n";
    std::fs::write(root.join("doc.md"), text).unwrap();
    let mut server = native_server();
    initialize(&mut server, json!([{"uri":workspace_uri,"trusted":true}]));
    let opened = open(&mut server, &document_uri, text, 1);
    let diagnostics = opened
        .iter()
        .find(|item| item["method"] == "textDocument/publishDiagnostics")
        .unwrap();
    assert_eq!(
        diagnostics["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        diagnostics["params"]["diagnostics"][0]["code"],
        "asset-missing"
    );
    assert_eq!(
        diagnostics["params"]["diagnostics"][0]["range"],
        json!({"start":{"line":0,"character":0},"end":{"line":0,"character":23}})
    );
    let preview = preview(&mut server, &document_uri, 1, "embeddedHtml");
    let frame = read_frame(&mut server, preview["previewSessionId"].as_str().unwrap());
    assert_eq!(frame["assets"].as_array().unwrap().len(), 1);
    assert!(frame["blocks"].to_string().contains("Asset unavailable"));
}
