//! Role: Verify standard LSP initialization and authoritative document language features.
mod support;
use fleximark_protocol::method;
use serde_json::json;
use support::*;

#[test]
fn lifecycle_advertises_complete_editor_commands_and_rejects_removed_handshakes() {
    let mut server = native_server();
    let result = initialize(&mut server, json!([]));
    assert_eq!(result["capabilities"]["positionEncoding"], "utf-8");
    assert_eq!(result["capabilities"]["textDocumentSync"]["change"], 1);
    let commands = result["capabilities"]["executeCommandProvider"]["commands"]
        .as_array()
        .unwrap();
    for command in [
        "fleximark.preview",
        "fleximark.previewAt",
        "fleximark.workspace",
        "fleximark.createNote",
        "fleximark.exportHtml",
        "fleximark.initializeWorkspace",
        "fleximark.editTheme",
        "fleximark.collectAdmonitions",
        "fleximark.migrate",
    ] {
        assert!(commands.contains(&json!(command)), "missing {command}");
    }
    for method in [
        "fleximark/initialize",
        "fleximark/openDocument",
        "fleximark/changeDocument",
        "fleximark/closeDocument",
    ] {
        let response = server.request(10, method, json!({}));
        assert_eq!(
            response[0]["error"]["code"], -32601,
            "removed route {method}"
        );
    }
    assert!(server.notify("future/notification", json!({})).is_empty());
    let malformed = server.request(11, "textDocument/completion", json!({}));
    assert_eq!(malformed[0]["error"]["code"], -32602);
    let shutdown = server.request(12, "shutdown", json!(null));
    assert!(shutdown[0]["result"].is_null());
    assert!(server.notify("exit", json!(null)).is_empty());
}

#[test]
fn code_actions_capture_document_version_and_selection() {
    let mut server = native_server();
    initialize(&mut server, json!([]));
    open(&mut server, "file:///actions.md", "# First\n", 7);
    let range = json!({"start":{"line":0,"character":2},"end":{"line":0,"character":6}});
    let actions=server.request(2,"textDocument/codeAction",json!({"textDocument":{"uri":"file:///actions.md"},"range":range,"context":{"diagnostics":[]}}));
    let actions = actions[0]["result"].as_array().unwrap();
    assert_eq!(actions.len(), 9);
    for action in actions {
        assert_eq!(
            action["command"]["arguments"][0]["documentUri"],
            "file:///actions.md"
        );
        assert_eq!(action["command"]["arguments"][0]["documentVersion"], 7);
        assert_eq!(action["command"]["arguments"][0]["range"], range);
    }
    let stale=server.request(3,"workspace/executeCommand",json!({"command":"fleximark.preview","arguments":[{"documentUri":"file:///actions.md","documentVersion":6}]}));
    assert_eq!(
        stale[0]["error"]["code"],
        fleximark_protocol::CONTENT_MODIFIED
    );
}
#[test]
fn completion_uses_authoritative_open_document_and_position() {
    let mut server = native_server();
    let initialized = server.request(
        1,
        "initialize",
        json!({"capabilities":{
            "general":{"positionEncodings":["utf-8"]},
            "textDocument":{"completion":{"completionItem":{"snippetSupport":true}}}
        }}),
    );
    assert_eq!(
        initialized[0]["result"]["capabilities"]["completionProvider"]["triggerCharacters"],
        json!([":", "`", ">"])
    );
    assert_eq!(
        initialized[0]["result"]["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"],
        json!(["keyword", "string", "operator", "type", "property"])
    );
    server.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":"file:///completion.md","version":7,"text":"::"}}),
    );
    let completion = server.request(
        3,
        "textDocument/completion",
        json!({"textDocument":{"uri":"file:///completion.md"},"position":{"line":0,"character":2}}),
    );
    assert_eq!(completion[0]["result"]["items"][0]["label"], "admonition");
    assert_eq!(completion[0]["result"]["items"][0]["insertTextFormat"], 2);
    assert_eq!(
        completion[0]["result"]["items"][0]["textEdit"]["range"]["start"],
        json!({"line":0,"character":0})
    );
    assert_eq!(
        completion[0]["result"]["items"][0]["textEdit"]["range"]["end"],
        json!({"line":0,"character":2})
    );
    let attach = server.request(
        4,
        method::ATTACH_DOCUMENT,
        json!({"daemonInstanceId":server.sessions().daemon_instance_id(),
                "uri":"file:///completion.md","expectedDocumentVersion":7}),
    );
    assert!(attach[0]["result"]["documentSessionId"].is_string());
    let invalid = server.request(
        5,
        "textDocument/completion",
        json!({"textDocument":{"uri":"file:///completion.md"},"position":{"line":1,"character":0}}),
    );
    assert_eq!(invalid[0]["error"]["code"], -32602);
}

#[test]
fn lsp_features_share_the_authoritative_ir_and_always_respond() {
    let mut server = native_server();
    server.request(
        1,
        "initialize",
        json!({"capabilities":{"general":{"positionEncodings":["utf-8"]}}}),
    );
    let unopened_tokens = server.request(
        3,
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":"file:///not-open.md"}}),
    );
    assert!(unopened_tokens[0]["result"].is_null());
    let opened = server.notify(
            "textDocument/didOpen",
            json!({"textDocument":{"uri":"file:///features.md","version":7,"text":"# Héllo\n\n:::warning[Care]\n<svg>x</svg>\n:::\n"}}),
        );
    let published = opened
        .iter()
        .find(|item| item["method"] == "textDocument/publishDiagnostics")
        .unwrap();
    assert_eq!(published["params"]["version"], 7);
    assert_eq!(published["params"]["diagnostics"], json!([]));

    let hover = server.request(
        3,
        "textDocument/hover",
        json!({"textDocument":{"uri":"file:///features.md"},"position":{"line":0,"character":3}}),
    );
    assert!(
        hover[0]["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Heading")
    );
    let symbols = server.request(
        4,
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":"file:///features.md"}}),
    );
    assert_eq!(symbols[0]["result"][0]["name"], "Héllo");
    assert!(
        symbols[0]["result"][0]["range"]["start"]
            .get("encoding")
            .is_none()
    );
    let semantic_tokens = server.request(
        5,
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":"file:///features.md"}}),
    );
    assert_eq!(
        semantic_tokens[0]["result"]["data"],
        json!([2, 0, 3, 2, 0, 0, 3, 7, 0, 0, 0, 7, 6, 1, 0, 2, 0, 3, 2, 0])
    );
    let diagnostics = server.request(
        6,
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":"file:///features.md"}}),
    );
    assert_eq!(diagnostics[0]["result"]["items"], json!([]));

    for method in [
        "textDocument/hover",
        "textDocument/documentSymbol",
        "textDocument/semanticTokens/full",
        "textDocument/diagnostic",
    ] {
        let invalid = server.request(7, method, json!({}));
        assert_eq!(invalid.len(), 1, "{method} dropped its request");
        assert_eq!(invalid[0]["error"]["code"], -32602, "{method}");
    }
}

#[test]
fn previous_protocol_is_rejected_before_workspace_or_document_setup() {
    let mut server = native_server();
    let response = server.request(1, "initialize", json!({"capabilities":{},"initializationOptions":{"fleximark":{"protocolVersion":fleximark_protocol::PROTOCOL_VERSION - 1}}}));
    assert_eq!(response[0]["error"]["code"], -32001);
    assert!(server.sessions().open_documents().is_empty());
    let current = initialize(&mut server, json!([]));
    assert_eq!(
        current["capabilities"]["experimental"]["fleximark"]["protocolVersion"],
        fleximark_protocol::PROTOCOL_VERSION
    );
}
