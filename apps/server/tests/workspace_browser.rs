//! Role: Verify browser forms preserve explicit native workspace authority.
mod support;
use fleximark_lsp_transport::Server;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use support::*;

fn payload(response: &str) -> Value {
    serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn browser_forms_select_context_and_preserve_native_workspace_authority() {
    let root = test_directory("browser-forms");
    let workspace = uri(&root);
    fleximark_application::native::initialize_workspace(&workspace).unwrap();
    std::fs::write(root.join(".fleximark/config.toml"),"schema_version = 2\n[notes]\nroot = \"notes\"\n[notes.categories]\nWork = { Project = {} }\n[notes.templates]\ndaily = [\"# Daily\"]\n").unwrap();
    let document = uri(&root.join("document.md"));
    std::fs::write(root.join("document.md"), "# Saved\n").unwrap();
    let mut instance = Server::with_assets(None, browser_assets());
    initialize(&mut instance, json!([{"uri":workspace,"trusted":true}]));
    open(
        &mut instance,
        &document,
        ":::info\nUnsaved admonition\n:::\n",
        9,
    );
    let server = Arc::new(Mutex::new(instance));
    let weak = Arc::downgrade(&server);
    server
        .lock()
        .unwrap()
        .set_workspace_handler(Arc::new(move |request| {
            weak.upgrade()
                .ok_or_else(|| "server closed".to_owned())?
                .lock()
                .map_err(|_| "server poisoned".to_owned())?
                .workspace_request(request)
        }));
    let command = server.lock().unwrap().request(
        2,
        "workspace/executeCommand",
        json!({"command":"fleximark.workspace","arguments":[]}),
    );
    let url = command[0]["result"]["url"].as_str().unwrap();
    assert!(
        command
            .iter()
            .any(|message| message["method"] == "window/showDocument"
                && message["params"]["external"] == true)
    );
    assert!(http(url, "", "GET", None).contains("id=\"workspace\""));
    assert!(http(url, "/client.js", "GET", None).contains("const workspace"));
    let state = payload(&http(url, "/state", "GET", None));
    assert_eq!(state["documents"][0]["uri"], document);
    assert_eq!(state["documents"][0]["version"], 9);
    assert!(
        state.get("noteOptions").is_none(),
        "missing workspace context must remain a browser selection"
    );
    let selected = payload(&http(
        url,
        &format!("/state?workspaceUri={workspace}"),
        "GET",
        None,
    ));
    assert_eq!(
        selected["noteOptions"]["categories"],
        json!([{"name":"Work","children":[{"name":"Project","children":[]}]}])
    );
    assert_eq!(selected["noteOptions"]["templates"], json!(["daily"]));
    let create=json!({"command":"createNote","workspaceUri":workspace,"noteCategoryPath":["Work","Project"],"noteTemplate":"daily","noteFileName":"meeting"}).to_string();
    let response = http(url, "/command", "POST", Some(&create));
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let created = payload(&response);
    assert!(
        created["openUri"]
            .as_str()
            .unwrap()
            .ends_with("/notes/Work/Project/meeting.md")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notes/Work/Project/meeting.md")).unwrap(),
        "# Daily\n"
    );
    let theme = json!({"command":"editTheme","workspaceUri":workspace}).to_string();
    assert!(
        payload(&http(url, "/command", "POST", Some(&theme)))["openUri"]
            .as_str()
            .unwrap()
            .ends_with("/.fleximark/theme.css")
    );
    let collect=json!({"command":"collectAdmonitions","workspaceUri":workspace,"documentUri":document,"documentVersion":9}).to_string();
    let collected = http(url, "/command", "POST", Some(&collect));
    assert!(collected.starts_with("HTTP/1.1 200 OK"), "{collected}");
    assert!(
        payload(&collected)["message"]
            .as_str()
            .unwrap()
            .contains("1 admonition")
    );
    let missing_context = http(
        url,
        "/command",
        "POST",
        Some(&json!({"command":"exportHtml","workspaceUri":workspace}).to_string()),
    );
    assert!(missing_context.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(
        payload(&missing_context)["message"]
            .as_str()
            .unwrap()
            .contains("select an open document")
    );
    let stale=http(url,"/command","POST",Some(&json!({"command":"collectAdmonitions","workspaceUri":workspace,"documentUri":document,"documentVersion":8}).to_string()));
    assert!(stale.starts_with("HTTP/1.1 400 Bad Request"));
    let destination = root.join("public");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("user.txt"), "keep").unwrap();
    let export=http(url,"/command","POST",Some(&json!({"command":"exportHtml","workspaceUri":workspace,"documentUri":document,"documentVersion":9,"destinationUri":uri(&destination)}).to_string()));
    assert!(export.starts_with("HTTP/1.1 400 Bad Request"));
    assert!(
        payload(&export)["message"]
            .as_str()
            .unwrap()
            .contains("unmanaged non-empty")
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("user.txt")).unwrap(),
        "keep"
    );
}
