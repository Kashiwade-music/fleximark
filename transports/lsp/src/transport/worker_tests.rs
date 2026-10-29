//! Role: Prove slow workspace jobs leave the actor responsive and release waiters.
use super::*;
use fleximark_application::{CancellationToken, DocumentStore, WorkspaceAuthority, WorkspaceOps};
use fleximark_protocol::{CommandContext, WorkspaceCommand, method};
use fleximark_test_support::TempWorkspace;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
fn message(id: i64, method: &str, params: Value) -> IncomingMessage {
    IncomingMessage {
        jsonrpc: "2.0".into(),
        id: Some(id.try_into().unwrap()),
        method: method.into(),
        params,
    }
}
fn next_output(output: &CoalescingOutput) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    let (queue, ready) = &*output.state;
    let mut queue = queue.lock().unwrap();
    loop {
        if let Some(message) = queue.pop_front() {
            return message;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "actor did not produce output before the deadline"
        );
        queue = ready.wait_timeout(queue, remaining).unwrap().0;
    }
}
fn fixture() -> (TempWorkspace, Server, String, String) {
    let root = TempWorkspace::new("worker").unwrap();
    let workspace = root.join("workspace");
    let resources = root.join("resources");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::create_dir(&resources).unwrap();
    let specification: Value = serde_json::from_str(include_str!(
        "../../../../contracts/distribution/distribution.json"
    ))
    .unwrap();
    let os = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        value => value,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        value => value,
    };
    let target = specification["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["os"] == os && target["arch"] == arch)
        .unwrap();
    let mut files = serde_json::Map::new();
    for file in specification["files"].as_array().unwrap() {
        let name = file["path"]
            .as_str()
            .unwrap()
            .replace("{exe}", target["executableSuffix"].as_str().unwrap());
        let path = resources.join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "test").unwrap();
        files.insert(
            name,
            json!("9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"),
        );
    }
    std::fs::write(resources.join("manifest.json"),json!({"schemaVersion":1,"version":fleximark_distribution::release_metadata().unwrap().version,"protocolVersion":fleximark_protocol::PROTOCOL_VERSION,"target":target["triple"],"files":files}).to_string()).unwrap();
    let workspace_uri = WorkspaceAuthority::path_to_file_uri(&workspace).unwrap();
    let mut store = DocumentStore::new(fleximark_model::PositionEncoding::Utf8);
    store
        .configure_native_workspaces(vec![(workspace_uri.clone(), true)])
        .unwrap();
    WorkspaceOps::new(&mut store, None)
        .execute(
            WorkspaceCommand::InitializeWorkspace {},
            CommandContext {
                workspace_uri: Some(workspace_uri.clone()),
                document: None,
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let document = WorkspaceAuthority::path_to_file_uri(&workspace.join("document.md")).unwrap();
    std::fs::write(workspace.join("document.md"), "# Disk").unwrap();
    let mut server = Server::new(crate::test_services::assets()).unwrap();
    server.set_preview_assets(Arc::new(
        fleximark_preview_assets::PreviewAssets::open(&resources).unwrap(),
    ));
    let initialized=server.handle(message(1,"initialize",json!({"capabilities":{"general":{"positionEncodings":["utf-8"]}},"initializationOptions":{"fleximark":{"workspaces":[{"uri":workspace_uri,"trusted":true}]}}})));
    assert!(initialized[0].get("result").is_some());
    server.handle(IncomingMessage {
        jsonrpc: "2.0".into(),
        id: None,
        method: "textDocument/didOpen".into(),
        params: json!({"textDocument":{"uri":document,"version":1,"text":"# Original\n"}}),
    });
    (root, server, workspace_uri, document)
}
fn export(server: &Server, workspace: &str, document: &str) -> Value {
    json!({"daemonInstanceId":server.daemon_instance_id(),"context":{"workspaceUri":workspace,"document":{"documentSessionId":server.sessions().session_id_for_uri(document).unwrap(),"expectedDocumentVersion":1}},"operation":{"command":"exportHtml","destinationUri":null}})
}
fn send(
    sender: &mpsc::Sender<InputEvent>,
    coordinator: &CancellationCoordinator,
    message: IncomingMessage,
) {
    let permit = coordinator.prepare(&message);
    sender
        .send(InputEvent::Message { message, permit })
        .unwrap();
}
fn actor(
    server: Server,
    hook: Arc<dyn Fn() + Send + Sync>,
    after: bool,
) -> (
    mpsc::Sender<InputEvent>,
    Arc<CancellationCoordinator>,
    CoalescingOutput,
    thread::JoinHandle<()>,
) {
    let (sender, receiver) = mpsc::channel();
    let coordinator = Arc::new(CancellationCoordinator::default());
    let output = CoalescingOutput::default();
    install_workspace_handler(&server, sender.clone());
    let mut operations = ActorOperations::new(sender.clone(), coordinator.clone());
    if after {
        operations.after_execute = Some(hook);
    } else {
        operations.before_execute = Some(hook);
    }
    let thread_coordinator = coordinator.clone();
    let thread_output = output.clone();
    let worker = thread::spawn(move || {
        run_actor(
            server,
            &thread_coordinator,
            &thread_output,
            receiver,
            operations,
        )
        .unwrap()
    });
    (sender, coordinator, output, worker)
}
#[test]
fn ten_second_export_allows_completion_and_source_changes_before_success() {
    let (_directory, server, workspace, document) = fixture();
    let export = export(&server, &workspace, &document);
    let (started, entered) = mpsc::channel();
    let (sender, coordinator, output, worker) = actor(
        server,
        Arc::new(move || {
            started.send(()).unwrap();
            thread::sleep(Duration::from_secs(10));
        }),
        true,
    );
    send(
        &sender,
        &coordinator,
        message(10, method::EXECUTE_COMMAND, export),
    );
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let started = Instant::now();
    send(
        &sender,
        &coordinator,
        message(
            11,
            "textDocument/completion",
            json!({"textDocument":{"uri":document},"position":{"line":0,"character":0}}),
        ),
    );
    let completion = next_output(&output);
    assert_eq!(completion["id"], 11);
    assert!(completion.get("result").is_some());
    assert!(started.elapsed() < Duration::from_secs(1));
    sender.send(InputEvent::Message{message:IncomingMessage{jsonrpc:"2.0".into(),id:None,method:"textDocument/didChange".into(),params:json!({"textDocument":{"uri":document,"version":2},"contentChanges":[{"text":"# New source\n"}]})},permit:None}).unwrap();
    loop {
        let result = next_output(&output);
        if result["id"] == 10 {
            assert_eq!(result["result"]["kind"], "htmlExported", "{result}");
            break;
        }
    }
    sender.send(InputEvent::Closed).unwrap();
    worker.join().unwrap();
}
fn workspace_url(server: &mut Server, workspace: &str) -> String {
    let result=server.handle(message(3,method::EXECUTE_COMMAND,json!({"daemonInstanceId":server.daemon_instance_id(),"context":{"workspaceUri":workspace},"operation":{"command":"openWorkspace"}})));
    result[0]["result"]["url"].as_str().unwrap().to_owned()
}
fn http_command(url: &str, body: String) -> String {
    let (address, path) = url
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream,"POST /{path}/command HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
#[test]
fn http_workspace_operation_can_finish_after_more_than_one_second() {
    let (_directory, mut server, workspace, document) = fixture();
    let url = workspace_url(&mut server, &workspace);
    let body=json!({"context":{"workspaceUri":workspace,"documentUri":document,"documentVersion":1},"operation":{"command":"exportHtml","destinationUri":null}}).to_string();
    let (started, entered) = mpsc::channel();
    let (sender, _coordinator, _output, worker) = actor(
        server,
        Arc::new(move || {
            started.send(()).unwrap();
            thread::sleep(Duration::from_millis(1200));
        }),
        false,
    );
    let started = Instant::now();
    let response = thread::spawn(move || http_command(&url, body));
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let response = response.join().unwrap();
    assert!(started.elapsed() > Duration::from_secs(1));
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("htmlExported"));
    sender.send(InputEvent::Closed).unwrap();
    worker.join().unwrap();
}
#[test]
fn shutdown_drops_pending_http_reply_before_worker_is_released() {
    let (_directory, mut server, workspace, document) = fixture();
    let url = workspace_url(&mut server, &workspace);
    let body=json!({"context":{"workspaceUri":workspace,"documentUri":document,"documentVersion":1},"operation":{"command":"exportHtml","destinationUri":null}}).to_string();
    let (started, entered) = mpsc::channel();
    let gate = Arc::new(std::sync::Barrier::new(2));
    let hook_gate = gate.clone();
    let (sender, coordinator, output, worker) = actor(
        server,
        Arc::new(move || {
            started.send(()).unwrap();
            hook_gate.wait();
        }),
        false,
    );
    let response = thread::spawn(move || http_command(&url, body));
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    send(&sender, &coordinator, message(99, "shutdown", Value::Null));
    let result = next_output(&output);
    assert_eq!(result["id"], 99);
    assert!(result.get("result").is_some());
    let response = response.join().unwrap();
    assert!(response.starts_with("HTTP/1.1 500"), "{response}");
    gate.wait();
    sender.send(InputEvent::Closed).unwrap();
    worker.join().unwrap();
}

#[test]
fn workspace_worker_lane_serializes_writes_without_blocking_the_actor() {
    let (_directory, server, workspace, document) = fixture();
    let first = json!({"daemonInstanceId":server.daemon_instance_id(),"context":{"workspaceUri":workspace},"operation":{"command":"createNote","name":"first"}});
    let second = json!({"daemonInstanceId":server.daemon_instance_id(),"context":{"workspaceUri":workspace},"operation":{"command":"createNote","name":"second"}});
    let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook_active = active.clone();
    let hook_peak = peak.clone();
    let (sender, coordinator, output, worker) = actor(
        server,
        Arc::new(move || {
            let current = hook_active.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            hook_peak.fetch_max(current, std::sync::atomic::Ordering::SeqCst);
            thread::sleep(Duration::from_millis(150));
            hook_active.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }),
        false,
    );
    send(
        &sender,
        &coordinator,
        message(30, method::EXECUTE_COMMAND, first),
    );
    send(
        &sender,
        &coordinator,
        message(31, method::EXECUTE_COMMAND, second),
    );
    let started = Instant::now();
    send(
        &sender,
        &coordinator,
        message(
            32,
            "textDocument/completion",
            json!({"textDocument":{"uri":document},"position":{"line":0,"character":0}}),
        ),
    );
    let completion = next_output(&output);
    assert_eq!(completion["id"], 32);
    assert!(started.elapsed() < Duration::from_secs(1));
    let mut completed = 0;
    while completed < 2 {
        let result = next_output(&output);
        if result["id"] == 30 || result["id"] == 31 {
            assert_eq!(result["result"]["kind"], "noteCreated", "{result}");
            completed += 1;
        }
    }
    assert_eq!(peak.load(std::sync::atomic::Ordering::SeqCst), 1);
    sender.send(InputEvent::Closed).unwrap();
    worker.join().unwrap();
}

#[test]
fn configuration_continuation_keeps_lsp_and_http_pending_until_latest_assets_are_ready() {
    for http in [false, true] {
        let (directory, mut server, workspace, document) = fixture();
        let root = directory.join("workspace");
        std::fs::create_dir(root.join("images")).unwrap();
        std::fs::write(root.join("images/first.png"), b"\x89PNG\r\n\x1a\nfirst").unwrap();
        std::fs::write(root.join("images/second.png"), b"\x89PNG\r\n\x1a\nsecond").unwrap();
        server.handle(IncomingMessage{jsonrpc:"2.0".into(),id:None,method:"textDocument/didChange".into(),params:json!({"textDocument":{"uri":document,"version":2},"contentChanges":[{"text":"# First\n\n![first](images/first.png)\n"}]})});
        let preview=server.handle(message(4,method::CREATE_PREVIEW,json!({"daemonInstanceId":server.daemon_instance_id(),"documentSessionId":server.sessions().session_id_for_uri(&document).unwrap(),"expectedDocumentVersion":2,"target":"externalBrowser"})));
        let preview_id = preview[0]["result"]["previewSessionId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            preview[0]["result"]["frame"]["assets"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        std::fs::write(
            root.join(".fleximark/config.toml"),
            "schema_version=1\n[assets]\nroots=['images']\n",
        )
        .unwrap();
        let request = json!({"daemonInstanceId":server.daemon_instance_id(),"context":{"workspaceUri":workspace},"operation":{"command":"initializeWorkspace"}});
        let url = if http {
            Some(workspace_url(&mut server, &workspace))
        } else {
            None
        };
        let (phase_started, phase_entered) = mpsc::channel();
        let (release, continue_phase) = mpsc::channel();
        let continue_phase = Arc::new(Mutex::new(continue_phase));
        let phases = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hook_phases = phases.clone();
        let (sender, coordinator, output, worker) = actor(
            server,
            Arc::new(move || {
                phase_started
                    .send(hook_phases.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
                    .unwrap();
                continue_phase.lock().unwrap().recv().unwrap();
            }),
            true,
        );
        let response = if let Some(url) = url {
            let body=json!({"context":{"workspaceUri":workspace},"operation":{"command":"initializeWorkspace"}}).to_string();
            Some(thread::spawn(move || http_command(&url, body)))
        } else {
            send(
                &sender,
                &coordinator,
                message(40, method::EXECUTE_COMMAND, request),
            );
            None
        };
        assert_eq!(
            phase_entered.recv_timeout(Duration::from_secs(3)).unwrap(),
            0
        );
        // The native command has committed. Replaying it would now fail config validation.
        std::fs::write(
            root.join(".fleximark/config.toml"),
            "schema_version=1\ninvalid=true\n",
        )
        .unwrap();
        sender.send(InputEvent::Message{message:IncomingMessage{jsonrpc:"2.0".into(),id:None,method:"textDocument/didChange".into(),params:json!({"textDocument":{"uri":document,"version":3},"contentChanges":[{"text":"# Second\n\n![second](images/second.png)\n"}]})},permit:None}).unwrap();
        send(
            &sender,
            &coordinator,
            message(
                55,
                "textDocument/completion",
                json!({"textDocument":{"uri":document},"position":{"line":0,"character":0}}),
            ),
        );
        loop {
            let event = next_output(&output);
            assert_ne!(event["id"], 40);
            if event["id"] == 55 {
                break;
            }
        }
        release.send(()).unwrap();
        assert_eq!(
            phase_entered.recv_timeout(Duration::from_secs(3)).unwrap(),
            1
        );
        assert!(!output.drain().iter().any(|event| event["id"] == 40));
        if let Some(response) = &response {
            assert!(
                !response.is_finished(),
                "HTTP reply must remain owned by the actor across continuation"
            );
        }
        release.send(()).unwrap();
        let mut replied = http;
        let mut published = false;
        while !replied || !published {
            let event = next_output(&output);
            if event["id"] == 40 {
                assert_eq!(event["result"]["kind"], "workspaceInitialized", "{event}");
                replied = true;
            }
            if event["method"] == method::PREVIEW_CHANGED
                && event["params"]["previewSessionId"] == preview_id
            {
                let frame = &event["params"]["frame"];
                assert_eq!(frame["documentVersion"], 3);
                assert_eq!(frame["assets"].as_array().unwrap().len(), 1);
                assert!(frame["blocks"].to_string().contains("Second"));
                published = true;
            }
        }
        if let Some(response) = response {
            let response = response.join().unwrap();
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            assert!(response.contains("workspaceInitialized"));
        }
        assert_eq!(phases.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            std::fs::read_to_string(root.join(".fleximark/config.toml")).unwrap(),
            "schema_version=1\ninvalid=true\n"
        );
        sender.send(InputEvent::Closed).unwrap();
        worker.join().unwrap();
    }
}
