use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use fleximark_lsp::content_hash;
use fleximark_plugin_host::CancellationToken;
use fleximark_protocol::{IncomingMessage, RpcId};
use serde_json::json;

use fleximark_engine::RenderFrame;
use fleximark_lsp::SessionError;
use fleximark_protocol::{CONTENT_MODIFIED, method};
use serde_json::Value;

use crate::cancellation::CancellationCoordinator;
use crate::preview_http::{PreviewPage, StoredEvent, preview_shell, serve_preview_request};
use crate::server::{Server, session_error};
use crate::telemetry::OperationalTrace;

fn message(id: Option<i64>, method: &str, params: Value) -> IncomingMessage {
    IncomingMessage {
        jsonrpc: "2.0".into(),
        id: id.map(|id| RpcId::try_from(id).unwrap()),
        method: method.into(),
        params,
    }
}

fn result_string(messages: &[Value], field: &str) -> String {
    messages[0]["result"][field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} result must be a string"))
        .to_owned()
}

fn initialize_daemon(server: &mut Server, params: Value) -> String {
    result_string(
        &server.request(1, method::INITIALIZE, params),
        "daemonInstanceId",
    )
}

fn initialize_params(capabilities: Value, workspaces: Option<Value>) -> Value {
    let mut params = json!({
        "protocolVersion": 4,
        "client": {"name": "test", "version": "1"},
        "capabilities": capabilities,
    });
    if let Some(workspaces) = workspaces {
        params["workspaces"] = workspaces;
    }
    params
}

fn open_rpc_document(
    server: &mut Server,
    id: i64,
    daemon_instance_id: &str,
    uri: &str,
    text: &str,
) -> Vec<Value> {
    server.request(
        id,
        method::OPEN_DOCUMENT,
        json!({
            "daemonInstanceId": daemon_instance_id,
            "uri": uri,
            "documentVersion": 1,
            "text": text,
        }),
    )
}

trait TestServer {
    fn request(&mut self, id: i64, method: &str, params: Value) -> Vec<Value>;
    fn notify(&mut self, method: &str, params: Value) -> Vec<Value>;
}

impl TestServer for Server {
    fn request(&mut self, id: i64, method: &str, params: Value) -> Vec<Value> {
        self.handle(message(Some(id), method, params))
    }

    fn notify(&mut self, method: &str, params: Value) -> Vec<Value> {
        self.handle(message(None, method, params))
    }
}

struct TestDirectory(PathBuf);

impl Deref for TestDirectory {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn test_directory(label: &str) -> TestDirectory {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("fleximark-daemon-{label}-{nonce}"));
    std::fs::create_dir(&path).expect("create daemon test directory");
    TestDirectory(path)
}

fn preview_pages(token: &str) -> Arc<Mutex<HashMap<String, PreviewPage>>> {
    let frame: RenderFrame = serde_json::from_value(json!({
        "previewSessionId":"preview-1",
        "documentVersion":1,
        "renderRevision":7,
        "rendererFingerprint":"0000000000000000000000000000000000000000000000000000000000000000",
        "style":null,
        "assets":[],
        "blocks":[{"id":"node-1","html":"<p data-fleximark-node-id=\"node-1\">hello</p>","nodeIds":["node-1"]}],
        "navigation":[{
            "nodeId":"node-1",
            "sourceRange":{
                "byteStart":4,
                "byteEnd":9,
                "start":{"line":2,"character":1,"encoding":"utf8"},
                "end":{"line":2,"character":6,"encoding":"utf8"}
            },
            "depth":1
        }],
        "annotations":{}
    }))
    .unwrap();
    Arc::new(Mutex::new(HashMap::from([(
        token.to_owned(),
        PreviewPage {
            shell: preview_shell(token),
            daemon_instance_id: "daemon-1".into(),
            preview_session_id: "preview-1".into(),
            frame,
            change_event: StoredEvent {
                encoded: serde_json::to_string(&json!({
                    "daemonInstanceId":"daemon-1",
                    "previewSessionId":"preview-1",
                    "renderRevision":7
                }))
                .unwrap(),
                sequence: 1,
            },
            navigation_event: None,
            next_sequence: 2,
            document_version: 1.into(),
            last_browser_event: None,
        },
    )])))
}

fn preview_http_request(
    pages: &Arc<Mutex<HashMap<String, PreviewPage>>>,
    sender: Option<Sender<Value>>,
    request: impl FnOnce(u16) -> Vec<u8>,
) -> Vec<u8> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind preview fixture");
    let port = listener
        .local_addr()
        .expect("preview fixture address")
        .port();
    let pages = Arc::clone(pages);
    let worker = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept preview fixture request");
        serve_preview_request(stream, port, &pages, sender.as_ref());
    });
    let mut client = TcpStream::connect(("127.0.0.1", port)).expect("connect preview fixture");
    client
        .write_all(&request(port))
        .expect("write preview fixture request");
    client
        .shutdown(Shutdown::Write)
        .expect("finish preview fixture request");
    let mut response = Vec::new();
    client
        .read_to_end(&mut response)
        .expect("read preview fixture response");
    worker.join().expect("preview fixture worker");
    response
}

fn preview_navigation_request(
    token: &str,
    port: u16,
    content_length: usize,
    body: &str,
) -> Vec<u8> {
    format!(
        "POST /preview/{token}/navigation HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nContent-Type: application/json\r\nContent-Length: {content_length}\r\n\r\n{body}"
    )
    .into_bytes()
}

fn preview_server_request(address: &str, request: impl AsRef<[u8]>) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(request.as_ref()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn session_errors_preserve_their_wire_codes_and_messages() {
    let cases = [
        (SessionError::NotOpen, -32602, "document is not open"),
        (
            SessionError::UnknownSession,
            -32602,
            "document session is unknown or no longer active",
        ),
        (
            SessionError::WrongDaemon,
            -32602,
            "daemon instance does not match this connection",
        ),
        (
            SessionError::ContentModified,
            CONTENT_MODIFIED,
            "document is out of sync",
        ),
        (
            SessionError::StaleVersion,
            CONTENT_MODIFIED,
            "document version is stale",
        ),
        (
            SessionError::VersionMismatch,
            CONTENT_MODIFIED,
            "document version does not match",
        ),
        (
            SessionError::HashMismatch,
            CONTENT_MODIFIED,
            "content hash does not match",
        ),
        (
            SessionError::InvalidRange,
            -32602,
            "incremental edit range is invalid",
        ),
        (
            SessionError::EmptyChange,
            -32602,
            "a change notification must contain edits",
        ),
        (
            SessionError::Engine("fixture failure".into()),
            -32602,
            "engine rejected the document: fixture failure",
        ),
    ];

    for (error, code, message) in cases {
        let engine_rejection = matches!(error, SessionError::Engine(_));
        let mut expected = json!({
            "jsonrpc":"2.0",
            "id":"request-7",
            "error":{"code":code,"message":message}
        });
        if engine_rejection {
            expected["error"]["data"] = json!({"kind":"engine"});
        }
        assert_eq!(session_error(json!("request-7"), error), expected);
    }
}

#[test]
fn lsp_and_rpc_only_methods_preserve_the_mode_routing_matrix() {
    assert!(
        Server::new(false)
            .notify("future/notification", json!({}))
            .is_empty(),
        "unknown notifications must remain silent"
    );

    let lsp_requests = [
        "initialize",
        "shutdown",
        "textDocument/completion",
        "textDocument/semanticTokens/full",
        "textDocument/hover",
        "textDocument/documentSymbol",
        "textDocument/diagnostic",
    ];
    for method_name in lsp_requests {
        let lsp_result = Server::new(true).request(91, method_name, json!({}));
        assert_eq!(
            lsp_result.len(),
            1,
            "recognized LSP request did not produce exactly one response: {method_name}"
        );
        assert_ne!(
            lsp_result[0].pointer("/error/code"),
            Some(&json!(-32601)),
            "LSP route was not recognized: {method_name}: {lsp_result:?}"
        );
        let rpc_result = Server::new(false).request(92, method_name, json!({}));
        assert_eq!(
            rpc_result,
            [json!({
                "jsonrpc":"2.0","id":92,
                "error":{"code":-32601,"message":"method not found"}
            })],
            "standalone RPC accepted LSP-only route {method_name}"
        );
    }

    for method_name in [
        "initialized",
        "exit",
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/didClose",
    ] {
        let mut lsp = Server::new(true);
        let notification = lsp.notify(method_name, json!({}));
        assert!(
            notification.is_empty(),
            "recognized LSP notification unexpectedly responded: {method_name}"
        );
        if method_name == "exit" {
            assert!(lsp.exit, "the recognized exit notification must set exit");
        }
        let mut request_shaped_lsp = Server::new(true);
        let request_shaped = request_shaped_lsp.request(93, method_name, json!({}));
        if method_name == "initialized" || method_name == "exit" {
            assert!(
                request_shaped.is_empty(),
                "protocol notification {method_name} must remain silent even with an id"
            );
            if method_name == "exit" {
                assert!(request_shaped_lsp.exit);
            }
        } else {
            assert_eq!(
                request_shaped,
                [json!({
                    "jsonrpc":"2.0","id":93,
                    "error":{"code":-32600,"message":"LSP notification must not have an id"}
                })],
                "LSP document notification route was not recognized: {method_name}"
            );
        }
        let rpc_result = Server::new(false).request(94, method_name, json!({}));
        assert_eq!(
            rpc_result,
            [json!({
                "jsonrpc":"2.0","id":94,
                "error":{"code":-32601,"message":"method not found"}
            })],
            "standalone RPC accepted LSP-only notification route {method_name}"
        );
    }

    let lsp_open = Server::new(true).request(95, method::OPEN_DOCUMENT, json!({}));
    assert_eq!(lsp_open.len(), 1);
    assert_ne!(
        lsp_open[0]["error"]["code"], -32601,
        "LSP connection must accept the acknowledged document-open request"
    );

    for method_name in [method::CHANGE_DOCUMENT, method::CLOSE_DOCUMENT] {
        let rpc_result = Server::new(false).request(95, method_name, json!({}));
        assert_eq!(
            rpc_result.len(),
            1,
            "recognized standalone request did not produce exactly one response: {method_name}"
        );
        assert_ne!(
            rpc_result[0]["error"]["code"], -32601,
            "standalone RPC route was not recognized: {method_name}"
        );
        let lsp_result = Server::new(true).request(96, method_name, json!({}));
        assert_eq!(
            lsp_result,
            [json!({
                "jsonrpc":"2.0","id":96,
                "error":{"code":-32601,"message":"method not found"}
            })],
            "LSP accepted standalone-only route {method_name}"
        );
    }
}

#[test]
fn recognized_request_deserialization_preserves_error_wire_and_notification_silence() {
    let malformed = json!({"daemonInstanceId":[]});
    let response = Server::new(false).request(97, method::EXECUTE_COMMAND, malformed.clone());
    assert_eq!(
        response,
        [json!({
            "jsonrpc":"2.0",
            "id":97,
            "error":{
                "code":-32602,
                "message":"invalid params: invalid type: sequence, expected a string"
            }
        })],
        "recognized malformed requests must retain the exact shared deserialization wire error"
    );

    assert!(
        Server::new(false)
            .notify(method::EXECUTE_COMMAND, malformed)
            .is_empty(),
        "the notification pair must stay silent even when its params are malformed"
    );
}

#[test]
fn rpc_error_precedes_full_text_recovery_notification() {
    let mut server = Server::new(false);
    let daemon = initialize_daemon(&mut server, initialize_params(json!({}), None));
    let opened = open_rpc_document(&mut server, 2, &daemon, "file:///order.md", "old\n");
    let session = result_string(&opened, "documentSessionId");

    let outgoing = server.request(
        3,
        method::CHANGE_DOCUMENT,
        json!({"daemonInstanceId":daemon,"documentSessionId":session,
                "baseDocumentVersion":1,"baseContentHash":"wrong",
                "documentVersion":2,"text":"new\n"}),
    );
    assert_eq!(outgoing.len(), 2);
    assert_eq!(
        outgoing[0],
        json!({
            "jsonrpc":"2.0",
            "id":3,
            "error":{"code":CONTENT_MODIFIED,"message":"document is out of sync"}
        })
    );
    assert_eq!(
        outgoing[1],
        json!({
            "jsonrpc":"2.0",
            "method":method::REQUEST_FULL_TEXT,
            "params":{
                "daemonInstanceId":daemon,
                "documentSessionId":session,
                "reason":"standalone RPC change base mismatch",
                "uri":"file:///order.md"
            }
        })
    );
}

#[test]
fn unknown_workspace_command_preserves_exact_wire_error() {
    let mut server = Server::build(false, None);
    let daemon_instance_id = server.registry.daemon_instance_id().to_owned();

    assert_eq!(
        server.execute_command(
            Some(json!(19)),
            json!({
                "daemonInstanceId":daemon_instance_id,
                "command":"futureCommand"
            }),
        ),
        Some(json!({
            "jsonrpc":"2.0",
            "id":19,
            "error":{
                "code":-32020,
                "message":"unknown FlexiMark command: futureCommand"
            }
        }))
    );
}

#[test]
fn every_workspace_command_preserves_its_success_dispatch() {
    let workspace = test_directory("workspace-command-dispatch");
    let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
    let document_path = workspace.join("document.md");
    let document_text = ":::info\nImportant\n:::\n";
    std::fs::write(&document_path, document_text).unwrap();
    let document_uri = fleximark_service::path_to_file_uri(&document_path).unwrap();
    let mut server = Server::new(false);
    let initialized = server.request(
        1,
        method::INITIALIZE,
        initialize_params(
            json!({}),
            Some(json!([{"uri":workspace_uri.clone(),"trusted":true}])),
        ),
    );
    let daemon = initialized[0]["result"]["daemonInstanceId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        initialized[0]["result"]["capabilities"]["workspaceCommands"],
        json!([
            "initializeWorkspace",
            "editTheme",
            "collectAdmonitions",
            "createNote",
            "exportHtml"
        ])
    );

    let initialize = server.request(
        2,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"initializeWorkspace",
                "workspaceUri":workspace_uri}),
    );
    assert_eq!(initialize[0]["id"], 2);
    assert_eq!(
        initialize[0]["result"]["message"],
        json!({"level":"info","text":"Initialized .fleximark/config.toml and .fleximark/theme.css"})
    );

    let edit_theme = server.request(
        3,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"editTheme",
                "workspaceUri":workspace_uri}),
    );
    assert!(edit_theme[0]["result"]["message"].is_null());
    assert_eq!(
        edit_theme[0]["result"]["openUri"],
        fleximark_service::path_to_file_uri(&workspace.join(".fleximark/theme.css")).unwrap()
    );

    let create_note = server.request(
        4,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"createNote",
                "workspaceUri":workspace_uri}),
    );
    assert_eq!(
        create_note[0]["result"]["message"],
        json!({"level":"info","text":"Created a new note"})
    );
    assert!(create_note[0]["result"]["openUri"].is_string());

    let opened = open_rpc_document(&mut server, 5, &daemon, &document_uri, document_text);
    let session = result_string(&opened, "documentSessionId");

    let collected = server.request(
        6,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"collectAdmonitions",
                "documentSessionId":session,"expectedDocumentVersion":1,
                "workspaceUri":workspace_uri}),
    );
    assert_eq!(
        collected[0]["result"]["message"],
        json!({"level":"info","text":"Collected 1 admonition(s)"})
    );

    let exported = server.request(
        7,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"exportHtml",
                "documentSessionId":session,"expectedDocumentVersion":1,
                "workspaceUri":workspace_uri}),
    );
    assert_eq!(
        exported[0]["result"]["message"],
        json!({"level":"info","text":"Exported generation 1"})
    );

    let acknowledged = server.request(
        8,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"acknowledgeExport",
                "documentSessionId":session,"expectedDocumentVersion":1,
                "workspaceUri":workspace_uri}),
    );
    assert_eq!(
        acknowledged[0],
        json!({
            "jsonrpc":"2.0",
            "id":8,
            "result":{
                "message":{
                    "level":"info",
                    "text":"Export opened and validated; recovery backup released"
                }
            }
        })
    );
}

#[test]
fn legacy_workspace_commands_inspect_then_migrate_without_a_second_transport() {
    let workspace = test_directory("legacy-workspace-command");
    let control = workspace.join(".fleximark");
    std::fs::create_dir(&control).unwrap();
    std::fs::write(control.join("fleximark.json"), "{}").unwrap();
    std::fs::write(control.join("parserPlugin.js"), "module.exports = {};").unwrap();
    let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
    let mut server = Server::new(false);
    let initialized = server.request(
        1,
        method::INITIALIZE,
        initialize_params(
            json!({}),
            Some(json!([{"uri":workspace_uri.clone(),"trusted":true}])),
        ),
    );
    let daemon = result_string(&initialized, "daemonInstanceId");

    let inspected = server.request(
        2,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"inspectLegacyWorkspace",
            "workspaceUri":workspace_uri}),
    );
    assert_eq!(inspected[0]["result"]["data"], "legacy-plugin");

    let rejected = server.request(
        3,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"migrateWorkspace",
            "workspaceUri":workspace_uri,"arguments":[]}),
    );
    assert_eq!(
        rejected[0]["error"]["message"],
        "migrateWorkspace requires exactly one settings object"
    );

    let migrated = server.request(
        4,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"migrateWorkspace",
            "workspaceUri":workspace_uri,
            "arguments":["{\"noteFileNamePrefix\":\"note-\"}"]}),
    );
    assert!(migrated[0]["result"].is_object(), "{migrated:?}");
    assert!(control.join("config.toml").is_file());
    assert!(control.join("theme.css").is_file());
    assert!(control.join("fleximark.json").is_file());

    let reinspected = server.request(
        5,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"inspectLegacyWorkspace",
            "workspaceUri":workspace_uri}),
    );
    assert!(reinspected[0]["result"].get("data").is_none());
}

#[test]
fn cancellation_intake_stops_requests_and_obsolete_document_work() {
    let coordinator = CancellationCoordinator::default();
    let render = message(
        Some(7),
        method::CREATE_PREVIEW,
        json!({"documentSessionId":"session-1"}),
    );
    let render_permit = coordinator.prepare(&render).unwrap();
    coordinator.bind("session-1", "file:///document.md");

    let change = message(
        None,
        "textDocument/didChange",
        json!({"textDocument":{"uri":"file:///document.md","version":2}}),
    );
    let change_permit = coordinator.prepare(&change).unwrap();
    let next_change = message(
        None,
        "textDocument/didChange",
        json!({"textDocument":{"uri":"file:///document.md","version":3}}),
    );
    let next_change_permit = coordinator.prepare(&next_change).unwrap();
    assert!(!coordinator.is_current(&render_permit));
    assert!(coordinator.should_execute(&change_permit));
    assert!(!coordinator.is_current(&change_permit));
    assert!(coordinator.is_current(&next_change_permit));

    let request = message(
        Some(8),
        "textDocument/hover",
        json!({"textDocument":{"uri":"file:///other.md"}}),
    );
    let request_permit = coordinator.prepare(&request).unwrap();
    let cancel = message(None, "$/cancelRequest", json!({"id":8}));
    assert!(coordinator.prepare(&cancel).is_none());
    assert!(!coordinator.is_current(&request_permit));
}

#[test]
fn operational_trace_is_correlated_and_redacts_document_data() {
    let uri = "file:///C:/private/work/secret.md";
    let trace = OperationalTrace::new(Some(uri), Some("session-7"), Some(12));
    let first = trace.event(
        "transform-complete",
        Duration::from_millis(2),
        None,
        false,
        false,
    );
    let second = trace.event(
        "render-complete",
        Duration::from_millis(3),
        Some(9),
        false,
        false,
    );
    assert_eq!(first["correlationId"], second["correlationId"]);
    assert_eq!(second["renderRevision"], 9);
    assert_eq!(first["uriHash"].as_str().unwrap().len(), 64);
    assert!(first["stageDurationMs"].as_f64().unwrap() >= 2.0);
    let encoded = serde_json::to_string(&(first, second)).unwrap();
    assert!(!encoded.contains(uri));
    assert!(!encoded.contains("private"));
    assert!(!encoded.contains("secret.md"));
}

#[test]
fn lsp_and_fleximark_requests_use_the_same_document() {
    let (browser_sender, browser_events) = mpsc::channel();
    let mut server = Server::build(true, Some(browser_sender));
    let workspace = test_directory("command");
    let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
    fleximark_service::initialize_workspace(&workspace_uri).unwrap();
    let document_path = workspace.join("doc.md");
    std::fs::write(&document_path, "# Hello\n").unwrap();
    let document_uri = fleximark_service::path_to_file_uri(&document_path).unwrap();
    server.request(
        1,
        "initialize",
        json!({"capabilities": {"general":{"positionEncodings":["utf-8"]}}}),
    );
    server.request(
        2,
        method::INITIALIZE,
        json!({
            "protocolVersion": 4, "client":{"name":"test","version":"1"},
            "capabilities":{"selectionEvents":true,"viewportEvents":true},
            "workspaces":[{"uri":workspace_uri.clone(),"trusted":true}]
        }),
    );
    server.notify(
        "textDocument/didOpen",
        json!({
            "textDocument":{"uri":document_uri.clone(),"version":1,"text":"# Hello\n"}
        }),
    );
    let attached = server.request(
        3,
        method::ATTACH_DOCUMENT,
        json!({
            "daemonInstanceId":server.registry.daemon_instance_id(),
            "uri":document_uri.clone(), "expectedDocumentVersion":1,
            "contentHash":content_hash("# Hello\n")
        }),
    );
    assert!(attached[0].get("result").is_some(), "{attached:?}");
    let session_id = result_string(&attached, "documentSessionId");
    let checkpoint = server.request(
        4,
        method::CHECKPOINT_DOCUMENT,
        json!({
            "daemonInstanceId":server.registry.daemon_instance_id(),
            "documentSessionId":session_id.clone(),
            "documentVersion":1,
            "contentHash":content_hash("# Hello\n")
        }),
    );
    assert_eq!(checkpoint[0]["result"]["documentVersion"], 1);
    let preview = server.request(
        6,
        method::CREATE_PREVIEW,
        json!({
            "daemonInstanceId":server.registry.daemon_instance_id(),
            "documentSessionId":session_id.clone(),
            "expectedDocumentVersion":1,
            "target":"externalBrowser"
        }),
    );
    assert_eq!(preview[0]["result"].as_object().unwrap().len(), 2);
    assert!(
        preview[0]["result"]["url"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:")
    );
    let url = preview[0]["result"]["url"].as_str().unwrap();
    let address_and_path = url.strip_prefix("http://").unwrap();
    let (address, path) = address_and_path.split_once('/').unwrap();
    let response = preview_server_request(
        address,
        format!("GET /{path} HTTP/1.1\r\nHost: {address}\r\n\r\n"),
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("Content-Security-Policy: default-src 'none'"));
    let event_response = preview_server_request(
        address,
        format!("GET /{path}/events HTTP/1.1\r\nHost: {address}\r\n\r\n"),
    );
    assert!(event_response.contains("Content-Type: text/event-stream"));
    assert!(event_response.contains("renderRevision"));
    assert!(!event_response.contains("Hello"));
    let client_response = preview_server_request(
        address,
        format!("GET /{path}/client.js HTTP/1.1\r\nHost: {address}\r\n\r\n"),
    );
    assert!(client_response.contains("Content-Type: application/javascript"));
    assert!(client_response.contains("EventSource"));

    let rejected = preview_server_request(
        address,
        format!("GET /{path} HTTP/1.1\r\nHost: {address}\r\nOrigin: https://evil.example\r\n\r\n"),
    );
    assert!(rejected.starts_with("HTTP/1.1 403 Forbidden"));

    let mut oversized = TcpStream::connect(address).unwrap();
    let request = format!(
        "GET /preview/{} HTTP/1.1\r\nHost: {address}\r\n\r\n",
        "a".repeat(3000)
    );
    oversized.write_all(request.as_bytes()).unwrap();
    let mut oversized_response = Vec::new();
    match std::io::Read::read_to_end(&mut oversized, &mut oversized_response) {
        Ok(_) => assert!(oversized_response.is_empty()),
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset),
    }

    let preview_id = preview[0]["result"]["previewSessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    let rendered = server.request(
        7,
        method::READ_PREVIEW,
        json!({
            "daemonInstanceId":server.registry.daemon_instance_id(),
            "previewSessionId":preview_id.clone()
        }),
    );
    assert!(
        rendered[0]["result"]["frame"]["blocks"][0]["html"]
            .as_str()
            .unwrap()
            .contains("Hello")
    );
    let frame_response = preview_server_request(
        address,
        format!("GET /{path}/frame HTTP/1.1\r\nHost: {address}\r\n\r\n"),
    );
    assert!(frame_response.contains("Hello"));
    let selection = server.notify(
        method::SET_SELECTION,
        json!({
            "daemonInstanceId":server.registry.daemon_instance_id(),
            "documentSessionId":session_id.clone(),
            "expectedDocumentVersion":1,
            "selections":[{"anchor":{"line":0,"character":0},"active":{"line":0,"character":1}}]
        }),
    );
    assert_eq!(selection[0]["method"], method::PREVIEW_EVENT);
    assert_eq!(
        selection[0]["params"]["daemonInstanceId"],
        server.registry.daemon_instance_id()
    );
    assert_eq!(selection[0]["params"]["event"]["type"], "selection");
    let selected_node_id = selection[0]["params"]["event"]["nodeIds"][0]
        .as_str()
        .expect("selection resolves through the authoritative navigation map")
        .to_owned();

    let viewport = server.notify(
        method::SET_VIEWPORT,
        json!({
            "daemonInstanceId":server.registry.daemon_instance_id(),
            "documentSessionId":session_id.clone(),
            "expectedDocumentVersion":1,
            "ranges":[{"start":{"line":0,"character":0},"end":{"line":1,"character":0}}]
        }),
    );
    assert_eq!(viewport[0]["params"]["event"]["type"], "viewport");

    let browser_event = server.notify(
        method::PREVIEW_EVENT,
        json!({
            "daemonInstanceId":server.registry.daemon_instance_id(),
            "previewSessionId":preview_id.clone(),
            "renderRevision":1,
            "event":{"type":"selectNode","previewSessionId":preview_id,
                "renderRevision":1,"nodeId":selected_node_id}
        }),
    );
    assert_eq!(browser_event[0]["params"]["event"]["type"], "selectSource");
    assert_eq!(
        browser_event[0]["params"]["event"]["sourceRange"]["start"]["line"],
        0
    );

    let post_body = serde_json::to_string(&json!({
        "type":"revealNode", "previewSessionId":preview_id,
        "renderRevision":1, "nodeId":selected_node_id
    }))
    .unwrap();
    let navigation_request = format!(
        "POST /{path}/navigation HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{post_body}",
        post_body.len()
    );
    let post_response = preview_server_request(address, &navigation_request);
    assert!(post_response.starts_with("HTTP/1.1 204 No Content"));
    let external_event = browser_events.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(external_event["method"], method::PREVIEW_EVENT);
    assert_eq!(external_event["params"]["event"]["type"], "revealSource");
    assert_eq!(external_event["params"]["previewSessionId"], preview_id);

    let repeated_response = preview_server_request(address, navigation_request);
    assert!(
        repeated_response.starts_with("HTTP/1.1 429 Too Many Requests"),
        "{repeated_response}"
    );

    let unchanged = server.request(
        10,
        method::READ_PREVIEW,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),
            "previewSessionId":preview_id.clone(),"afterRevision":1}),
    );
    assert!(unchanged[0]["result"]["frame"].is_null());
    let rerendered = server.request(
        12,
        method::RERENDER_PREVIEW,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),
            "previewSessionId":preview_id.clone()}),
    );
    assert!(rerendered.iter().any(|message| message["result"].is_null()));
    assert!(rerendered.iter().any(|message| {
        message["method"] == method::PREVIEW_CHANGED && message["params"]["renderRevision"] == 2
    }));

    let exported = server.request(
        9,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),"command":"exportHtml",
                "documentSessionId":session_id.clone(),"expectedDocumentVersion":1,
                "workspaceUri":workspace_uri.clone()}),
    );
    assert!(
        exported[0]["result"]["openUri"]
            .as_str()
            .unwrap()
            .ends_with("index.html")
    );
    let collected = server.request(
        8,
        method::EXECUTE_COMMAND,
        json!({
            "daemonInstanceId":server.registry.daemon_instance_id(),"command":"collectAdmonitions",
            "documentSessionId":session_id,"expectedDocumentVersion":1,
            "workspaceUri":workspace_uri
        }),
    );
    assert!(
        collected[0]["result"]["message"]["text"]
            .as_str()
            .unwrap()
            .contains("0 admonition")
    );
    let update = server.notify(
        "textDocument/didChange",
        json!({
            "textDocument":{"uri":document_uri.clone(),"version":2},
            "contentChanges":[{"text":"# Updated\n"}]
        }),
    );
    let preview_update = update
        .iter()
        .find(|item| item["method"] == method::PREVIEW_CHANGED)
        .expect("document changes publish a preview event");
    assert_eq!(preview_update["params"]["renderRevision"], 3);
    let updated = server.request(
        11,
        method::READ_PREVIEW,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),
            "previewSessionId":preview_id.clone(),"afterRevision":2}),
    );
    assert!(
        updated[0]["result"]["frame"]["blocks"][0]["html"]
            .as_str()
            .unwrap()
            .contains("Updated")
    );

    for version in 3..103 {
        server.notify(
            "textDocument/didChange",
            json!({
                "textDocument":{"uri":document_uri.clone(),"version":version},
                "contentChanges":[{"text":format!("# Updated {version}\n")}]
            }),
        );
    }
    let token = server.preview_states[&preview_id]
        .token
        .as_ref()
        .expect("external preview has a token");
    let pages = server.previews.pages.lock().unwrap();
    let page = &pages[token];
    assert_eq!(page.frame.render_revision, 103);
    assert_eq!(page.change_event.sequence, page.next_sequence - 1);
    drop(pages);

    server.notify(
        "textDocument/didClose",
        json!({"textDocument":{"uri":document_uri}}),
    );
    let expired_response = preview_server_request(
        address,
        format!("GET /{path} HTTP/1.1\r\nHost: {address}\r\n\r\n"),
    );
    assert!(expired_response.starts_with("HTTP/1.1 403 Forbidden"));
}

#[test]
fn stale_notification_emits_full_text_request() {
    let mut server = Server::new(true);
    server.notify(
        "textDocument/didOpen",
        json!({
            "textDocument":{"uri":"file:///doc.md","version":2,"text":"x"}
        }),
    );
    let outgoing = server.notify(
        "textDocument/didChange",
        json!({
            "textDocument":{"uri":"file:///doc.md","version":2},
            "contentChanges":[{"text":"y"}]
        }),
    );
    assert_eq!(outgoing[0]["method"], method::REQUEST_FULL_TEXT);
    assert_eq!(
        outgoing[0]["params"]["daemonInstanceId"],
        server.registry.daemon_instance_id()
    );
    assert_eq!(
        outgoing[0]["params"]["documentSessionId"],
        server
            .registry
            .session_id_for_uri("file:///doc.md")
            .unwrap()
    );
}

#[test]
fn untrusted_workspace_cannot_run_write_commands() {
    let workspace = test_directory("untrusted");
    let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
    let mut server = Server::new(true);
    server.request(
        1,
        method::INITIALIZE,
        initialize_params(
            json!({"selectionEvents":false,"viewportEvents":false}),
            Some(json!([{"uri":workspace_uri.clone(),"trusted":false}])),
        ),
    );
    let denied = server.request(
        2,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),
                "command":"initializeWorkspace","workspaceUri":workspace_uri}),
    );
    assert_eq!(denied[0]["error"]["code"], -32021);
    assert!(!workspace.join(".fleximark").exists());
}

#[test]
fn workspace_command_authority_policy_is_checked_before_command_parameters() {
    let workspace = test_directory("workspace-command-authority");
    let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
    fleximark_service::initialize_workspace(&workspace_uri).unwrap();
    let mut server = Server::new(false);
    server.request(
        1,
        method::INITIALIZE,
        initialize_params(
            json!({}),
            Some(json!([{"uri":workspace_uri,"trusted":false}])),
        ),
    );

    for command in [
        "initializeWorkspace",
        "migrateWorkspace",
        "createNote",
        "collectAdmonitions",
        "exportHtml",
        "acknowledgeExport",
    ] {
        for workspace in [Some(workspace_uri.as_str()), None] {
            let mut params = json!({"daemonInstanceId":server.registry.daemon_instance_id(),
                "command":command});
            if let Some(workspace) = workspace {
                params["workspaceUri"] = Value::String(workspace.to_owned());
            }
            let response = server.request(2, method::EXECUTE_COMMAND, params);
            assert_eq!(
                response[0]["error"],
                json!({"code":-32021,
                    "message":"workspace write denied: the active workspace is not trusted"}),
                "authority did not reject {command} before command-specific parameters"
            );
        }
    }

    let edit = server.request(
        3,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),
            "command":"editTheme","workspaceUri":workspace_uri}),
    );
    assert!(edit[0]["result"]["openUri"].is_string());
    let missing_edit = server.request(
        4,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),
            "command":"editTheme"}),
    );
    assert_eq!(
        missing_edit[0]["error"],
        json!({"code":-32020,"message":"command requires workspaceUri"})
    );
}

#[test]
fn one_daemon_keeps_distinct_multi_root_trust_and_render_configuration() {
    let root = test_directory("multi-root");
    let trusted = root.join("trusted");
    let untrusted = trusted.join("nested-untrusted");
    std::fs::create_dir_all(&trusted).unwrap();
    std::fs::create_dir(&untrusted).unwrap();
    let trusted_uri = fleximark_service::path_to_file_uri(&trusted).unwrap();
    let untrusted_uri = fleximark_service::path_to_file_uri(&untrusted).unwrap();
    fleximark_service::initialize_workspace(&trusted_uri).unwrap();
    fleximark_service::initialize_workspace(&untrusted_uri).unwrap();
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
    let trusted_document = trusted.join("doc.md");
    let untrusted_document = untrusted.join("doc.md");
    std::fs::write(&trusted_document, "# Trusted\n").unwrap();
    std::fs::write(&untrusted_document, "# Untrusted\n").unwrap();
    let trusted_document_uri = fleximark_service::path_to_file_uri(&trusted_document).unwrap();
    let untrusted_document_uri = fleximark_service::path_to_file_uri(&untrusted_document).unwrap();
    let mut server = Server::new(false);
    let daemon = initialize_daemon(
        &mut server,
        initialize_params(
            json!({}),
            Some(json!([
                {"uri":trusted_uri,"trusted":true},
                {"uri":untrusted_uri,"trusted":false}
            ])),
        ),
    );
    let trusted_open = open_rpc_document(
        &mut server,
        2,
        &daemon,
        &trusted_document_uri,
        "# Trusted\n",
    );
    let untrusted_open = open_rpc_document(
        &mut server,
        3,
        &daemon,
        &untrusted_document_uri,
        "# Untrusted\n",
    );
    let trusted_session = result_string(&trusted_open, "documentSessionId");
    let untrusted_session = result_string(&untrusted_open, "documentSessionId");
    let trusted_preview = server.request(
        4,
        method::CREATE_PREVIEW,
        json!({"daemonInstanceId":daemon,"documentSessionId":trusted_session,
                "expectedDocumentVersion":1,"target":"embeddedHtml"}),
    );
    let untrusted_preview = server.request(
        5,
        method::CREATE_PREVIEW,
        json!({"daemonInstanceId":daemon,"documentSessionId":untrusted_session,
                "expectedDocumentVersion":1,"target":"embeddedHtml"}),
    );
    let trusted_preview_id = trusted_preview[0]["result"]["previewSessionId"]
        .as_str()
        .unwrap();
    let untrusted_preview_id = untrusted_preview[0]["result"]["previewSessionId"]
        .as_str()
        .unwrap();
    let trusted_frame = server.request(
        40,
        method::READ_PREVIEW,
        json!({"daemonInstanceId":daemon,"previewSessionId":trusted_preview_id}),
    );
    let untrusted_frame = server.request(
        41,
        method::READ_PREVIEW,
        json!({"daemonInstanceId":daemon,"previewSessionId":untrusted_preview_id}),
    );
    assert_eq!(
        trusted_frame[0]["result"]["frame"]["style"]["css"],
        ":root { color: red; }"
    );
    assert!(untrusted_frame[0]["result"]["frame"]["style"].is_null());
    assert!(trusted_preview[0]["result"].get("url").is_none());
    assert!(untrusted_preview[0]["result"].get("url").is_none());
    assert!(server.previews.pages.lock().unwrap().is_empty());
    let escalated = server.request(
        6,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":daemon,"command":"exportHtml",
                "documentSessionId":untrusted_session,"expectedDocumentVersion":1,
                "workspaceUri":trusted_uri}),
    );
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
        json!({"daemonInstanceId":daemon,"workspaceUri":trusted_uri,"trusted":true}),
    );
    assert!(reconfigured[0]["result"].is_null(), "{reconfigured:?}");
    let publication = reconfigured
        .iter()
        .find(|message| message["method"] == method::PREVIEW_CHANGED)
        .expect("active trusted preview receives a reconfiguration notification");
    assert_eq!(publication["params"]["renderRevision"], 2);
    let reconfigured_frame = server.request(
        42,
        method::READ_PREVIEW,
        json!({"daemonInstanceId":daemon,"previewSessionId":trusted_preview_id}),
    );
    assert_eq!(
        reconfigured_frame[0]["result"]["frame"]["style"]["css"],
        ":root { color: green; }"
    );
}

#[test]
fn invalid_workspace_is_disabled_without_disabling_other_roots() {
    let root = test_directory("root-status");
    let valid = root.join("valid");
    let invalid = root.join("invalid");
    std::fs::create_dir_all(&valid).unwrap();
    std::fs::create_dir(&invalid).unwrap();
    let valid_uri = fleximark_service::path_to_file_uri(&valid).unwrap();
    let invalid_uri = fleximark_service::path_to_file_uri(&invalid).unwrap();
    fleximark_service::initialize_workspace(&valid_uri).unwrap();
    fleximark_service::initialize_workspace(&invalid_uri).unwrap();
    std::fs::write(
        invalid.join(".fleximark/config.toml"),
        "schema_version=2\nunknown=true\n",
    )
    .unwrap();
    let mut server = Server::new(false);
    let initialized = server.request(
        1,
        method::INITIALIZE,
        initialize_params(
            json!({}),
            Some(json!([
                {"uri":valid_uri,"trusted":true},{"uri":invalid_uri,"trusted":true}
            ])),
        ),
    );
    let statuses = initialized[0]["result"]["workspaceStatuses"]
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
    let daemon = initialized[0]["result"]["daemonInstanceId"]
        .as_str()
        .unwrap();
    let valid_document = valid.join("doc.md");
    let invalid_document = invalid.join("doc.md");
    std::fs::write(&valid_document, "# Valid\n").unwrap();
    std::fs::write(&invalid_document, "# Invalid\n").unwrap();
    let valid_document_uri = fleximark_service::path_to_file_uri(&valid_document).unwrap();
    let opened = open_rpc_document(&mut server, 2, daemon, &valid_document_uri, "# Valid\n");
    assert!(opened[0]["result"]["documentSessionId"].is_string());
    let invalid_document_uri = fleximark_service::path_to_file_uri(&invalid_document).unwrap();
    let denied = open_rpc_document(&mut server, 3, daemon, &invalid_document_uri, "# Invalid\n");
    assert_eq!(denied[0]["error"]["code"], -32022);
}

#[test]
fn note_options_rpc_reads_only_the_granted_canonical_config() {
    let workspace = test_directory("note-options");
    let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
    fleximark_service::initialize_workspace(&workspace_uri).unwrap();
    std::fs::write(
            workspace.join(".fleximark/config.toml"),
            "schema_version = 2\n[notes.categories]\nWork = { Project = {} }\n[notes.templates]\ndaily = [\"# Daily\"]\n",
        )
        .unwrap();
    let mut server = Server::new(false);
    server.request(
        1,
        method::INITIALIZE,
        initialize_params(
            json!({}),
            Some(json!([{"uri":workspace_uri.clone(),"trusted":true}])),
        ),
    );
    let response = server.request(
        2,
        method::GET_NOTE_OPTIONS,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),
                "workspaceUri":workspace_uri}),
    );
    assert_eq!(
        response[0]["result"]["categories"],
        json!([{"name":"Work","children":[{"name":"Project","children":[]}]}])
    );
    assert_eq!(response[0]["result"]["templates"], json!(["daily"]));
}

#[test]
fn export_preflight_rejects_unmanaged_destination_before_render_hooks() {
    let workspace = test_directory("export-preflight");
    let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
    fleximark_service::initialize_workspace(&workspace_uri).unwrap();
    let document_path = workspace.join("doc.md");
    std::fs::write(&document_path, "# Safe\n").unwrap();
    let document_uri = fleximark_service::path_to_file_uri(&document_path).unwrap();
    let destination = workspace.join("public");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("owned-by-user.txt"), "keep").unwrap();
    let destination_uri = fleximark_service::path_to_file_uri(&destination).unwrap();
    let mut server = Server::new(false);
    server.request(
        1,
        method::INITIALIZE,
        initialize_params(
            json!({}),
            Some(json!([{"uri":workspace_uri.clone(),"trusted":true}])),
        ),
    );
    let daemon = server.registry.daemon_instance_id().to_owned();
    let opened = open_rpc_document(&mut server, 2, &daemon, &document_uri, "# Safe\n");
    let session = opened[0]["result"]["documentSessionId"].as_str().unwrap();
    let response = server.request(
        3,
        method::EXECUTE_COMMAND,
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),"command":"exportHtml",
                "documentSessionId":session,"expectedDocumentVersion":1,
                "workspaceUri":workspace_uri,"destinationUri":destination_uri}),
    );
    assert!(
        response[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unmanaged non-empty")
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("owned-by-user.txt")).unwrap(),
        "keep"
    );
    assert!(!destination.join(".fleximark-export.json").exists());
}

#[test]
fn completion_uses_authoritative_open_document_and_position() {
    let mut server = Server::new(true);
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
    server.request(2, method::INITIALIZE, initialize_params(json!({}), None));
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
        json!({"daemonInstanceId":server.registry.daemon_instance_id(),
                "uri":"file:///completion.md","expectedDocumentVersion":7,
                "contentHash":content_hash("::")}),
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
    let mut server = Server::new(true);
    server.request(
        1,
        "initialize",
        json!({"capabilities":{"general":{"positionEncodings":["utf-8"]}}}),
    );
    server.request(2, method::INITIALIZE, initialize_params(json!({}), None));
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
fn asset_errors_publish_precise_diagnostics_without_blocking_preview_rendering() {
    let workspace = test_directory("asset-lsp-diagnostics");
    let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
    fleximark_service::initialize_workspace(&workspace_uri).unwrap();
    std::fs::write(workspace.join("valid.png"), b"\x89PNG\r\n\x1a\nvalid").unwrap();
    let document_path = workspace.join("doc.md");
    let source = "![missing](missing.png)\n\n![valid](valid.png)\n";
    std::fs::write(&document_path, source).unwrap();
    let document_uri = fleximark_service::path_to_file_uri(&document_path).unwrap();
    let mut server = Server::new(true);
    server.request(
        1,
        "initialize",
        json!({"capabilities":{"general":{"positionEncodings":["utf-8"]}}}),
    );
    server.request(
        2,
        method::INITIALIZE,
        initialize_params(
            json!({}),
            Some(json!([{"uri":workspace_uri,"trusted":true}])),
        ),
    );
    let opened = server.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":document_uri,"version":1,"text":source}}),
    );

    let published = opened
        .iter()
        .find(|item| item["method"] == "textDocument/publishDiagnostics")
        .expect("asset diagnostics are published after opening the document");
    assert_eq!(
        published["params"]["diagnostics"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        published["params"]["diagnostics"][0]["code"],
        "asset-missing"
    );
    assert_eq!(
        published["params"]["diagnostics"][0]["range"]["start"],
        json!({"line":0,"character":0})
    );
    assert_eq!(
        published["params"]["diagnostics"][0]["range"]["end"],
        json!({"line":0,"character":23})
    );
    let daemon = server.registry.daemon_instance_id().to_owned();
    let session_id = server
        .registry
        .session_id_for_uri(&document_uri)
        .unwrap()
        .to_owned();
    let frame = server
        .registry
        .render_with_cancellation(
            &daemon,
            &session_id,
            1,
            "asset-diagnostic-preview",
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(frame.assets.len(), 1);
    assert!(frame.html().contains("Asset unavailable"));
}

#[test]
fn preview_http_shell_preserves_exact_security_headers_and_rejections() {
    let token = "fixture-token";
    let pages = preview_pages(token);
    let response = preview_http_request(&pages, None, |port| {
        format!("GET /preview/{token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").into_bytes()
    });
    let body = preview_shell(token);
    assert!(body.contains(".katex{font:"));
    assert!(body.contains("data:font/woff2;base64,"));
    assert!(body.contains("[data-admonition-kind=\"note\"]"));
    assert!(body.contains("[data-admonition-kind=\"important\"]"));
    assert!(body.contains("[data-admonition-kind=\"caution\"]"));
    let expected = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; base-uri 'none'; form-action 'none'; font-src data:; img-src 'self' data: blob:; media-src 'self' blob:; frame-src https://www.youtube-nocookie.com; object-src 'none'; style-src 'unsafe-inline'; script-src 'self'; connect-src 'self'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    assert_eq!(response, expected.as_bytes());

    for request in [
        format!("GET /preview/{token} HTTP/1.1\r\nHost: evil.example\r\n\r\n"),
        format!(
            "GET /preview/{token} HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nOrigin: https://evil.example\r\n\r\n"
        ),
        format!(
            "POST /preview/{token}/navigation HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nContent-Type: application/json\r\nContent-Length: 0\r\n\r\n"
        ),
    ] {
        let response = preview_http_request(&pages, None, |port| {
            request.replace("{port}", &port.to_string()).into_bytes()
        });
        assert_eq!(
            response,
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    }
}

#[test]
fn preview_http_enforces_header_and_body_parser_boundaries() {
    let token = "fixture-token";
    let forbidden = b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let bad_request = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    for duplicate in [
        "Host: localhost:{port}\r\nHost: localhost:{port}\r\n",
        "Host: localhost:{port}\r\nOrigin: http://localhost:{port}\r\nOrigin: http://localhost:{port}\r\n",
    ] {
        let response = preview_http_request(&preview_pages(token), None, |port| {
            format!(
                "GET /preview/{token} HTTP/1.1\r\n{}\r\n",
                duplicate.replace("{port}", &port.to_string())
            )
            .into_bytes()
        });
        assert_eq!(response, forbidden);
    }

    for duplicate in [
        "Content-Type: application/json\r\nContent-Type: application/json\r\nContent-Length: 0\r\n",
        "Content-Type: application/json\r\nContent-Length: 0\r\nContent-Length: 0\r\n",
    ] {
        let response = preview_http_request(&preview_pages(token), None, |port| {
            format!(
                "POST /preview/{token}/navigation HTTP/1.1\r\nHost: localhost:{port}\r\nOrigin: http://localhost:{port}\r\n{duplicate}\r\n"
            )
            .into_bytes()
        });
        assert_eq!(response, bad_request);
    }

    let event = serde_json::to_string(&json!({
        "type":"revealNode",
        "previewSessionId":"preview-1",
        "renderRevision":7,
        "nodeId":"node-1"
    }))
    .unwrap();
    let body = format!("{event}{}", " ".repeat(4096 - event.len()));
    let (sender, events) = mpsc::channel();
    let response = preview_http_request(&preview_pages(token), Some(sender), |port| {
        preview_navigation_request(token, port, 4096, &body)
    });
    assert_eq!(
        response,
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    );
    assert!(events.recv().is_ok());

    let invalid_bodies: [(usize, &str, &[u8]); 3] = [
        (4097, "", bad_request),
        (1, "{", bad_request),
        (2, "{", b""),
    ];
    for (content_length, body, expected) in invalid_bodies {
        let response = preview_http_request(&preview_pages(token), None, |port| {
            preview_navigation_request(token, port, content_length, body)
        });
        assert_eq!(response, expected, "content length {content_length}");
    }
    let request_with_header_lengths = |port: u16, lengths: &[usize]| {
        let mut request = format!("GET /preview/{token} HTTP/1.1\r\nHost: localhost:{port}\r\n");
        for (index, length) in lengths.iter().copied().enumerate() {
            let prefix = format!("X-{index}: ");
            request.push_str(&prefix);
            request.push_str(&"a".repeat(length - prefix.len() - 2));
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        request.into_bytes()
    };

    for (individual, total_delta, accepted) in [
        (8192, None, true),
        (8193, None, false),
        (8192, Some(0), true),
        (8192, Some(1), false),
    ] {
        let actual = preview_http_request(&preview_pages(token), None, |port| {
            let base = format!("GET /preview/{token} HTTP/1.1\r\nHost: localhost:{port}\r\n").len();
            let lengths = total_delta.map_or_else(
                || vec![individual],
                |delta| vec![individual, 16 * 1024 - base - individual + delta],
            );
            request_with_header_lengths(port, &lengths)
        });
        if accepted {
            assert!(actual.starts_with(b"HTTP/1.1 200 OK\r\n"));
        } else {
            assert!(
                actual.is_empty(),
                "individual={individual}, total_delta={total_delta:?}"
            );
        }
    }
}

#[test]
fn preview_sse_preserves_wire_format_order_and_acknowledged_sequences() {
    let token = "fixture-token";
    let pages = preview_pages(token);
    let expected_event = pages.lock().unwrap()[token].change_event.encoded.clone();
    let response = preview_http_request(&pages, None, |port| {
        format!(
                "GET /preview/{token}/events HTTP/1.1\r\nHost: localhost:{port}\r\nLast-Event-ID: 0\r\n\r\n"
            )
            .into_bytes()
    });
    let body = format!("retry: 250\nid: 1\ndata: {expected_event}\n\n");
    let expected = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    assert_eq!(response, expected.as_bytes());
    {
        let mut pages = pages.lock().unwrap();
        let page = pages.get_mut(token).unwrap();
        page.navigation_event = Some(StoredEvent {
            encoded: "navigation-2".to_owned(),
            sequence: 2,
        });
        page.change_event = StoredEvent {
            encoded: "change-3".to_owned(),
            sequence: 3,
        };
        page.next_sequence = 4;
    }

    let response = |last_event_id: u64| {
        String::from_utf8(preview_http_request(&pages, None, |port| {
            format!(
                "GET /preview/{token}/events HTTP/1.1\r\nHost: localhost:{port}\r\nLast-Event-ID: {last_event_id}\r\n\r\n"
            )
            .into_bytes()
        }))
        .unwrap()
    };

    let from_start = response(0);
    let navigation = from_start.find("id: 2\ndata: navigation-2").unwrap();
    let change = from_start.find("id: 3\ndata: change-3").unwrap();
    assert!(navigation < change);

    let after_navigation = response(2);
    assert!(!after_navigation.contains("id: 2\n"));
    assert!(after_navigation.contains("id: 3\ndata: change-3"));

    let after_change = response(3);
    assert!(!after_change.contains("id: 2\n"));
    assert!(!after_change.contains("id: 3\n"));
}

#[test]
fn preview_navigation_preserves_revision_gate_and_notification_wire() {
    let token = "fixture-token";
    let pages = preview_pages(token);
    let (sender, events) = mpsc::channel();
    let request = |port: u16, revision: u64| {
        let body = serde_json::to_string(&json!({
            "type":"revealNode",
            "previewSessionId":"preview-1",
            "renderRevision":revision,
            "nodeId":"node-1"
        }))
        .unwrap();
        preview_navigation_request(token, port, body.len(), &body)
    };

    let stale = preview_http_request(&pages, Some(sender.clone()), |port| request(port, 6));
    assert_eq!(
        stale,
        b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    assert!(events.try_recv().is_err());

    let accepted = preview_http_request(&pages, Some(sender), |port| request(port, 7));
    assert_eq!(
            accepted,
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
        );
    assert_eq!(
        events.recv().unwrap(),
        json!({
            "jsonrpc":"2.0",
            "method":method::PREVIEW_EVENT,
            "params":{
                "daemonInstanceId":"daemon-1",
                "previewSessionId":"preview-1",
                "renderRevision":7,
                "event":{
                    "type":"revealSource",
                    "sourceRange":{
                        "byteStart":4,
                        "byteEnd":9,
                        "start":{"line":2,"character":1,"encoding":"utf8"},
                        "end":{"line":2,"character":6,"encoding":"utf8"}
                    }
                }
            }
        })
    );
}

#[test]
fn standalone_rpc_uses_explicit_base_version_and_hash() {
    let mut server = Server::new(false);
    let daemon = initialize_daemon(&mut server, initialize_params(json!({}), None));
    let opened = open_rpc_document(&mut server, 2, &daemon, "file:///rpc.md", "old\n");
    let session = result_string(&opened, "documentSessionId");
    let changed = server.request(
        3,
        method::CHANGE_DOCUMENT,
        json!({
            "daemonInstanceId":daemon,"documentSessionId":session,
            "baseDocumentVersion":1,"baseContentHash":content_hash("old\n"),
            "documentVersion":4,"text":"new\n"
        }),
    );
    assert_eq!(changed[0]["result"]["documentVersion"], 4);
}

#[test]
fn stale_frame_navigation_is_rejected_after_a_failed_rerender() {
    let mut server = Server::new(false);
    let daemon = initialize_daemon(&mut server, initialize_params(json!({}), None));
    let opened = open_rpc_document(
        &mut server,
        2,
        &daemon,
        "file:///stale-navigation.md",
        "# Old\n",
    );
    let session = result_string(&opened, "documentSessionId");
    let preview = server.request(
        3,
        method::CREATE_PREVIEW,
        json!({
            "daemonInstanceId":daemon,
            "documentSessionId":session,
            "expectedDocumentVersion":1,
            "target":"externalBrowser"
        }),
    );
    let preview_id = result_string(&preview, "previewSessionId");
    let url = preview[0]["result"]["url"].as_str().unwrap().to_owned();
    let frame = server.request(
        4,
        method::READ_PREVIEW,
        json!({"daemonInstanceId":daemon,"previewSessionId":preview_id}),
    );
    let node_id = frame[0]["result"]["frame"]["navigation"][0]["nodeId"]
        .as_str()
        .unwrap()
        .to_owned();
    let changed = server.request(
        5,
        method::CHANGE_DOCUMENT,
        json!({
            "daemonInstanceId":daemon,"documentSessionId":session,
            "baseDocumentVersion":1,"baseContentHash":content_hash("# Old\n"),
            "documentVersion":2,"text":"# New\n"
        }),
    );
    assert_eq!(changed[0]["result"]["documentVersion"], 2);

    let cancelled = CancellationToken::default();
    cancelled.cancel();
    let failed = server.handle_cancellable(
        message(
            Some(6),
            method::RERENDER_PREVIEW,
            json!({"daemonInstanceId":daemon,"previewSessionId":preview_id}),
        ),
        Some(cancelled),
        None,
    );
    assert!(failed[0].get("error").is_some(), "{failed:?}");

    let stale_rpc = server.request(
        7,
        method::PREVIEW_EVENT,
        json!({
            "daemonInstanceId":daemon,
            "previewSessionId":preview_id,
            "renderRevision":1,
            "event":{"type":"selectNode","previewSessionId":preview_id,
                "renderRevision":1,"nodeId":node_id}
        }),
    );
    assert_eq!(stale_rpc[0]["error"]["code"], CONTENT_MODIFIED);

    let address_and_path = url.strip_prefix("http://").unwrap();
    let (address, path) = address_and_path.split_once('/').unwrap();
    let body = serde_json::to_string(&json!({
        "type":"selectNode","previewSessionId":preview_id,
        "renderRevision":1,"nodeId":node_id
    }))
    .unwrap();
    let stale_http = preview_server_request(
        address,
        format!(
            "POST /{path}/navigation HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(
        stale_http.starts_with("HTTP/1.1 409 Conflict"),
        "{stale_http}"
    );
}
