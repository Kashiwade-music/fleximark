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
    for descriptor in fleximark_protocol::COMMAND_DESCRIPTORS {
        assert!(
            commands.contains(&json!(descriptor.lsp_command)),
            "missing {}",
            descriptor.lsp_command
        );
    }
    for method in [
        "fleximark/initialize",
        "fleximark/openDocument",
        "fleximark/changeDocument",
        "fleximark/closeDocument",
    ] {
        let response = server.request(10, method, json!({}));
        assert_eq!(
            response[0]["error"]["code"],
            fleximark_protocol::error_code::METHOD_NOT_FOUND,
            "removed route {method}"
        );
    }
    assert!(server.notify("future/notification", json!({})).is_empty());
    let malformed = server.request(11, "textDocument/completion", json!({}));
    assert_eq!(
        malformed[0]["error"]["code"],
        fleximark_protocol::error_code::INVALID_PARAMS
    );
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
            action["command"]["arguments"][0]["context"]["document"]["documentSessionId"],
            session(&server, "file:///actions.md")
        );
        assert_eq!(
            action["command"]["arguments"][0]["context"]["document"]["expectedDocumentVersion"],
            7
        );
    }
    let selected = actions
        .iter()
        .find(|action| action["title"] == "Preview at selection")
        .unwrap();
    assert_eq!(
        selected["command"]["arguments"][0]["operation"]["sourceOffset"],
        2
    );
    let mut args = selected["command"].clone();
    args["arguments"][0]["context"]["document"]["expectedDocumentVersion"] = json!(6);
    args.as_object_mut().unwrap().remove("title");
    let stale = server.request(3, "workspace/executeCommand", args);
    assert_eq!(
        stale[0]["error"]["code"],
        fleximark_protocol::error_code::CONTENT_MODIFIED
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
        json!({"daemonInstanceId":server.daemon_instance_id(),
                "uri":"file:///completion.md","expectedDocumentVersion":7}),
    );
    assert!(attach[0]["result"]["documentSessionId"].is_string());
    let invalid = server.request(
        5,
        "textDocument/completion",
        json!({"textDocument":{"uri":"file:///completion.md"},"position":{"line":1,"character":0}}),
    );
    assert_eq!(
        invalid[0]["error"]["code"],
        fleximark_protocol::error_code::INVALID_PARAMS
    );
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
        assert_eq!(
            invalid[0]["error"]["code"],
            fleximark_protocol::error_code::INVALID_PARAMS,
            "{method}"
        );
    }
}

#[test]
fn previous_protocol_is_rejected_before_workspace_or_document_setup() {
    let mut server = native_server();
    let response = server.request(1, "initialize", json!({"capabilities":{},"initializationOptions":{"fleximark":{"protocolVersion":fleximark_protocol::PROTOCOL_VERSION - 1}}}));
    assert_eq!(
        response[0]["error"]["code"],
        fleximark_protocol::error_code::UNSUPPORTED_PROTOCOL
    );
    assert!(server.sessions().open_documents().is_empty());
    let current = initialize(&mut server, json!([]));
    assert_eq!(
        current["capabilities"]["experimental"]["fleximark"]["protocolVersion"],
        fleximark_protocol::PROTOCOL_VERSION
    );
}

#[test]
fn utf16_position_resolves_through_the_authoritative_session() {
    let mut server = native_server();
    server.request(
        1,
        "initialize",
        json!({"capabilities":{"general":{"positionEncodings":["utf-16"]}}}),
    );
    let uri = "file:///utf16-position.md";
    open(&mut server, uri, "# \u{1f600} heading\r\n", 1);
    let hover = server.request(
        2,
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":4}}),
    );
    assert!(
        hover[0]["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("UTF-8 source bytes")
    );
    let actions = server.request(
        3,
        "textDocument/codeAction",
        json!({"textDocument":{"uri":uri},"range":{"start":{"line":0,"character":4}}}),
    );
    let selected = actions[0]["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["title"] == "Preview at selection")
        .unwrap();
    assert_eq!(
        selected["command"]["arguments"][0]["operation"]["sourceOffset"],
        6
    );
    let invalid = server.request(
        4,
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":3}}),
    );
    assert!(invalid[0]["result"].is_null());
}
#[test]
fn full_sync_accepts_unicode_in_all_negotiated_encodings_and_rejects_incremental_input() {
    for encoding in ["utf-8", "utf-16", "utf-32"] {
        let mut server = native_server();
        let initialized = server.request(
            1,
            "initialize",
            json!({"capabilities":{"general":{"positionEncodings":[encoding]}}}),
        );
        assert_eq!(
            initialized[0]["result"]["capabilities"]["positionEncoding"],
            encoding
        );
        let uri = "file:///unicode-full-sync.md";
        open(&mut server, uri, "a\u{1f600}b\r\n", 1);
        for changes in [
            json!([{ "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}},"text":"x"}]),
            json!([{ "text":"first"},{"text":"second"}]),
            json!([]),
            json!([{ "range":null,"text":"x"}]),
        ] {
            server.notify(
                "textDocument/didChange",
                json!({"textDocument":{"uri":uri,"version":2},"contentChanges":changes}),
            );
            assert_eq!(
                server.sessions().source_for_uri(uri).unwrap(),
                "a\u{1f600}b\r\n"
            );
            assert!(
                server
                    .sessions()
                    .current_version(&session(&server, uri))
                    .is_err()
            );
        }
        server.notify("textDocument/didChange",json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"axb\u{1f600}\r\n"}]}));
        assert_eq!(
            server.sessions().source_for_uri(uri).unwrap(),
            "axb\u{1f600}\r\n"
        );
        assert_eq!(
            server
                .sessions()
                .current_version(&session(&server, uri))
                .unwrap(),
            2
        );
        server.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":uri,"version":-1},"contentChanges":[{"text":"negative"}]}),
        );
        assert_eq!(
            server.sessions().source_for_uri(uri).unwrap(),
            "axb\u{1f600}\r\n"
        );
        assert!(
            server
                .sessions()
                .current_version(&session(&server, uri))
                .is_err()
        );
    }
}

#[test]
fn byte_language_results_map_to_every_negotiated_encoding_with_unicode_and_crlf() {
    for (encoding, character, title_length) in
        [("utf-8", 18, 6), ("utf-16", 16, 4), ("utf-32", 15, 3)]
    {
        for snippets in [false, true] {
            let mut server = native_server();
            let initialized=server.request(1,"initialize",json!({"capabilities":{"general":{"positionEncodings":[encoding]},"textDocument":{"completion":{"completionItem":{"snippetSupport":snippets}}}}}));
            assert_eq!(
                initialized[0]["result"]["capabilities"]["positionEncoding"],
                encoding
            );
            let document = "file:///byte-language.md";
            open(
                &mut server,
                document,
                "\u{1f600}\r\n  :::warning[\u{1f600}]\r\n",
                1,
            );
            let completion=server.request(2,"textDocument/completion",json!({"textDocument":{"uri":document},"position":{"line":1,"character":character}}));
            let items = completion[0]["result"]["items"].as_array().unwrap();
            assert!(!items.is_empty());
            for item in items {
                assert_eq!(item["kind"], 15);
                assert_eq!(item["insertTextFormat"], if snippets { 2 } else { 1 });
                assert_eq!(
                    item["textEdit"]["range"],
                    json!({"start":{"line":1,"character":2},"end":{"line":1,"character":character}})
                );
                assert!(item["detail"].is_string());
                assert!(item["filterText"].is_string());
                if !snippets {
                    assert!(!item["textEdit"]["newText"].as_str().unwrap().contains("${"));
                }
            }
            let tokens = server.request(
                3,
                "textDocument/semanticTokens/full",
                json!({"textDocument":{"uri":document}}),
            );
            assert_eq!(
                tokens[0]["result"]["data"],
                json!([1, 2, 3, 2, 0, 0, 3, 7, 0, 0, 0, 7, title_length, 1, 0])
            );
        }
    }
}
#[test]
fn semantic_tokens_resume_after_code_fences_without_highlighting_their_contents() {
    let mut server = native_server();
    initialize(&mut server, json!([]));
    let document = "file:///semantic-code-fence.md";
    open(
        &mut server,
        document,
        "```rust\r\n:::hidden\r\n```\r\n:::after\r\n",
        1,
    );
    let tokens = server.request(
        2,
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":document}}),
    );
    assert_eq!(
        tokens[0]["result"]["data"],
        json!([
            0, 0, 3, 2, 0, 0, 3, 4, 3, 0, 2, 0, 3, 2, 0, 1, 0, 3, 2, 0, 0, 3, 5, 0, 0
        ])
    );
}
