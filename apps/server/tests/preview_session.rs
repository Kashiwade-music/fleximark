//! Role: Verify unsaved browser previews and bidirectional standard editor navigation.
use fleximark_protocol::error_code::CONTENT_MODIFIED;
mod support;
use fleximark_application::CancellationToken;
use fleximark_lsp_transport::Server;
use fleximark_protocol::method;
use serde_json::json;
use std::sync::mpsc;
use std::time::Duration;
use support::*;

#[test]
fn standalone_preview_keeps_source_links_and_unsaved_updates_without_workspace_configuration() {
    let root = test_directory("standalone-source-links");
    std::fs::create_dir(root.join(".fleximark")).unwrap();
    std::fs::write(
        root.join(".fleximark/config.toml"),
        "invalid workspace config",
    )
    .unwrap();
    let path = root.join("standalone.md");
    std::fs::write(&path, "disk contents").unwrap();
    let document = uri(&path);
    let mut server = Server::new(browser_assets()).unwrap();
    let initialized = server.request(1, "initialize", json!({"capabilities": {}}));
    assert_eq!(
        initialized[0]["result"]["capabilities"]["experimental"]["fleximark"]["workspaceStatuses"],
        json!([])
    );
    open(
        &mut server,
        &document,
        "[Rust](./src/main.rs)\n\n[Local Rust](file:///tmp/main.rs)\n\n# Following content\n",
        1,
    );
    let request = command_request(
        &server,
        None,
        Some((&document, 1)),
        json!({"command":"openPreview","target":"externalBrowser","sourceOffset":null}),
    );
    let preview = server.request(
        2,
        "workspace/executeCommand",
        json!({"command":"fleximark.openPreview","arguments":[request]}),
    );
    let preview_id = preview[0]["result"]["preview"]["previewSessionId"]
        .as_str()
        .unwrap();
    let url = preview[0]["result"]["preview"]["url"].as_str().unwrap();
    let initial = read_frame(&mut server, preview_id);
    let html = initial["blocks"].to_string();
    assert!(html.contains("src/main.rs"));
    assert!(html.contains("Local Rust"));
    assert!(html.contains("Following content"));
    assert!(!html.contains("file:///tmp/main.rs"));
    let changed = server.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": document, "version": 2},
            "contentChanges": [{"text": "[Rust](../src/main.rs#L2)\n\n# Unsaved update\n"}]
        }),
    );
    assert!(
        changed
            .iter()
            .any(|event| event["method"] == method::PREVIEW_CHANGED)
    );
    let updated = read_frame(&mut server, preview_id);
    assert_eq!(updated["documentVersion"], 2);
    assert!(
        updated["renderRevision"].as_u64().unwrap() > initial["renderRevision"].as_u64().unwrap()
    );
    assert!(http(url, "/frame", "GET", None).contains("Unsaved update"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "disk contents");
}

#[test]
fn browser_navigation_has_one_delivery_owner_for_each_client_capability() {
    for source_events in [None, Some(false), Some(true)] {
        for event_type in ["selectSource", "revealSource"] {
            let mut server = native_server();
            let mut capabilities = json!({});
            if let Some(enabled) = source_events {
                capabilities["sourceNavigationEvents"] = json!(enabled);
            }
            let initialized = server.request(
                1,
                "initialize",
                json!({"capabilities":{},"initializationOptions":{"fleximark":{"capabilities":capabilities}}}),
            );
            assert!(initialized[0].get("result").is_some());
            let document = "untitled:navigation-delivery";
            open(&mut server, document, "# Source\n", 1);
            let preview = preview(&mut server, document, 1, "externalBrowser");
            let frame = &preview["frame"];
            let event = json!({
                "jsonrpc":"2.0",
                "method":method::PREVIEW_EVENT,
                "params":{
                    "daemonInstanceId":daemon(&server),
                    "previewSessionId":preview["previewSessionId"],
                    "renderRevision":frame["renderRevision"],
                    "event":{"type":event_type,"sourceRange":frame["navigation"][0]["sourceRange"]}
                }
            });
            let routed = server.browser_notification(event.clone());
            assert_eq!(
                routed.len(),
                1,
                "{source_events:?} {event_type}: {routed:?}"
            );
            if source_events == Some(true) {
                assert_eq!(routed, vec![event.clone()]);
            } else {
                assert_eq!(routed[0]["method"], "window/showDocument");
                assert_eq!(routed[0]["params"]["uri"], document);
                assert_eq!(routed[0]["params"]["external"], false);
                assert_eq!(routed[0]["params"]["selection"]["start"]["line"], 0);
                server.client_response(
                    &json!({"jsonrpc":"2.0","id":routed[0]["id"],"result":{"success":true}}),
                );
            }
            let mut stale = event.clone();
            stale["params"]["renderRevision"] = json!(0);
            assert!(server.browser_notification(stale).is_empty());
            server.notify(
                "textDocument/didClose",
                json!({"textDocument":{"uri":document}}),
            );
            assert!(server.browser_notification(event).is_empty());
        }
    }
}

#[test]
fn explicit_preview_selection_survives_initial_browser_load_and_returns_to_source() {
    let (sender, events) = mpsc::channel();
    let mut server = Server::with_assets(Some(sender), browser_assets()).unwrap();
    let result=server.request(1,"initialize",json!({"capabilities":{"general":{"positionEncodings":["utf-8"]}},"initializationOptions":{"fleximark":{"capabilities":{"selectionEvents":false,"viewportEvents":false}}}}));
    assert!(result[0].get("result").is_some());
    let document = "file:///unsaved-selection.md";
    open(&mut server, document, "# Unsaved\n", 7);
    let actions=server.request(2,"textDocument/codeAction",json!({"textDocument":{"uri":document},"range":{"start":{"line":0,"character":2},"end":{"line":0,"character":9}}}));
    let mut action = actions[0]["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["title"] == "Preview at selection")
        .unwrap()["command"]
        .clone();
    action.as_object_mut().unwrap().remove("title");
    let command = server.request(3, "workspace/executeCommand", action);
    let result = &command[0]["result"]["preview"];
    let url = result["url"].as_str().unwrap();
    let preview_id = result["previewSessionId"].as_str().unwrap();
    assert!(
        command
            .iter()
            .any(|message| message["method"] == "window/showDocument"
                && message["params"]["external"] == true
                && message["params"]["uri"] == url)
    );
    let shell = http(url, "", "GET", None);
    assert!(shell.starts_with("HTTP/1.1 200 OK"));
    assert!(shell.contains("Content-Security-Policy: default-src 'none'"));
    assert!(http(url, "/client.js", "GET", None).contains("EventSource"));
    let initial_response = http(url, "/frame", "GET", None);
    let initial_payload: http_payload::Payload = http_payload::parse(&initial_response);
    assert_eq!(initial_payload["navigation"]["event"]["type"], "viewport");
    assert!(initial_payload["navigation"]["event"]["nodeId"].is_string());
    let jump_sequence = initial_payload["navigation"]["sequence"].clone();
    let frame = read_frame(&mut server, preview_id);
    assert!(frame["blocks"].to_string().contains("Unsaved"));
    let node = frame["navigation"][0]["nodeId"].as_str().unwrap();
    let revision = frame["renderRevision"].as_u64().unwrap();
    let navigation=json!({"type":"revealNode","previewSessionId":preview_id,"renderRevision":revision,"nodeId":node}).to_string();
    let response = http(url, "/navigation", "POST", Some(&navigation));
    assert!(
        response.starts_with("HTTP/1.1 204 No Content"),
        "{response}"
    );
    let event = events.recv_timeout(Duration::from_secs(1)).unwrap();
    let routed = server.browser_notification(event);
    let show = routed
        .iter()
        .find(|message| message["method"] == "window/showDocument")
        .expect("browser source navigation uses standard showDocument");
    assert_eq!(show["params"]["uri"], document);
    assert_eq!(show["params"]["external"], false);
    assert_eq!(show["params"]["selection"]["start"]["line"], 0);
    assert!(
        show["params"]["selection"]["start"]
            .get("encoding")
            .is_none()
    );
    server.client_response(&json!({"jsonrpc":"2.0","id":show["id"],"result":{"success":true}}));
    let repeat = http(url, "/navigation", "POST", Some(&navigation));
    assert!(repeat.starts_with("HTTP/1.1 429 Too Many Requests"));
    let updated=server.notify("textDocument/didChange",json!({"textDocument":{"uri":document,"version":8},"contentChanges":[{"text":"# Updated buffer\n"}]}));
    assert!(
        updated
            .iter()
            .any(|value| value["method"] == method::PREVIEW_CHANGED)
    );
    assert!(http(url, "/frame", "GET", None).contains("Updated buffer"));
    let frame = read_frame(&mut server, preview_id);
    assert_eq!(frame["documentVersion"], 8);
    assert!(frame["renderRevision"].as_u64().unwrap() > revision);
    let updated_response = http(url, "/frame", "GET", None);
    let updated_payload = http_payload::parse(&updated_response);
    assert!(
        updated_payload["navigation"]["sequence"].as_u64().unwrap()
            > jump_sequence.as_u64().unwrap()
    );
    assert_eq!(
        updated_payload["navigation"]["event"]["renderRevision"],
        frame["renderRevision"]
    );
    server.notify(
        "textDocument/didClose",
        json!({"textDocument":{"uri":document}}),
    );
    assert!(http(url, "", "GET", None).starts_with("HTTP/1.1 403 Forbidden"));
}

mod http_payload {
    //! Role: Decode the HTTP JSON body returned by native preview endpoints.
    pub type Payload = serde_json::Value;
    pub fn parse(response: &str) -> Payload {
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }
}

#[test]
fn stale_navigation_is_rejected_when_changed_document_cannot_publish() {
    let mut server = native_server();
    initialize(&mut server, json!([]));
    let document = "file:///stale-preview.md";
    open(&mut server, document, "# Old\n", 1);
    let preview = preview(&mut server, document, 1, "externalBrowser");
    let preview_id = preview["previewSessionId"].as_str().unwrap();
    let url = preview["url"].as_str().unwrap();
    let frame = read_frame(&mut server, preview_id);
    let node = frame["navigation"][0]["nodeId"].as_str().unwrap();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    let _=server.handle_cancellable(message(None,"textDocument/didChange",json!({"textDocument":{"uri":document,"version":2},"contentChanges":[{"text":"# New\n"}]})),Some(cancelled),None);
    let stale=server.request(7,method::PREVIEW_EVENT,json!({"daemonInstanceId":daemon(&server),"previewSessionId":preview_id,"renderRevision":1,"event":{"type":"selectNode","previewSessionId":preview_id,"renderRevision":1,"nodeId":node}}));
    assert_eq!(stale[0]["error"]["code"], CONTENT_MODIFIED, "{stale:?}");
    let navigation =
        json!({"type":"revealNode","previewSessionId":preview_id,"renderRevision":1,"nodeId":node})
            .to_string();
    assert!(
        http(url, "/navigation", "POST", Some(&navigation)).starts_with("HTTP/1.1 409 Conflict")
    );
}

#[test]
fn stale_notifications_preserve_source_and_next_full_sync_recovers() {
    let mut server = native_server();
    initialize(&mut server, json!([]));
    let document = "file:///resync.md";
    open(&mut server, document, "old", 2);
    let outgoing=server.notify("textDocument/didChange",json!({"textDocument":{"uri":document,"version":2},"contentChanges":[{"text":"invalid stale replacement"}]}));
    assert!(
        outgoing
            .iter()
            .all(|message| message["method"] != "fleximark/requestFullText")
    );
    assert_eq!(server.sessions().source_for_uri(document).unwrap(), "old");
}
