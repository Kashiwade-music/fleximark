use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fleximark_engine::RenderPublication;
use fleximark_lsp::{
    DidChangeParams, DidCloseParams, DidOpenParams, SessionError, SessionRegistry, TextDocumentItem,
};
use fleximark_model::{
    Block, BlockKind, Document, Inline, InlineKind, NavigationEntry, Node, NodeId, PositionEncoding,
};
use fleximark_plugin_host::CancellationToken;
use fleximark_protocol::{
    AttachDocumentParams, CONTENT_MODIFIED, CheckpointDocumentParams, CreatePreviewParams,
    CreatePreviewResult, DisposePreviewParams, ExecuteCommandParams, GetNoteOptionsParams,
    IncomingMessage, InitializeParams, InitializeResult, Notification, PROTOCOL_VERSION,
    PreviewEventParams, PreviewNavigationEvent, PreviewTarget, ReconfigureWorkspaceParams,
    ReloadPreviewParams, RenderNavigationEvent, RenderParams, Response, RpcChangeDocumentParams,
    RpcCloseDocumentParams, RpcOpenDocumentParams, ServerCapabilities, SetSelectionParams,
    SetViewportParams, SourceNavigationEvent, WorkspaceStatus, method, read_frame, write_frame,
};
use fleximark_service::{
    acknowledge_export, collect_admonitions, create_note_with_options, default_export_destination,
    edit_theme, export_html_with_safety, get_note_options, initialize_workspace,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct OperationalTrace {
    correlation_id: String,
    uri_hash: Option<String>,
    session_id: Option<String>,
    document_version: Option<i64>,
    started: Instant,
}

impl OperationalTrace {
    fn new(uri: Option<&str>, session_id: Option<&str>, document_version: Option<i64>) -> Self {
        Self {
            correlation_id: random_token().unwrap_or_else(|_| "rng-unavailable".into()),
            uri_hash: uri.map(|uri| format!("{:x}", Sha256::digest(uri.as_bytes()))),
            session_id: session_id.map(str::to_owned),
            document_version,
            started: Instant::now(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn event(
        &self,
        stage: &str,
        stage_duration: Duration,
        render_revision: Option<u64>,
        patch_bytes: usize,
        fallback_reason: Option<&str>,
        plugin_failure: bool,
        recovery: bool,
    ) -> Value {
        json!({
            "event":"fleximark.operation",
            "stage":stage,
            "correlationId":self.correlation_id,
            "uriHash":self.uri_hash,
            "documentSessionId":self.session_id,
            "documentVersion":self.document_version,
            "renderRevision":render_revision,
            "elapsedMs":self.started.elapsed().as_secs_f64() * 1_000.0,
            "stageDurationMs":stage_duration.as_secs_f64() * 1_000.0,
            "patchBytes":patch_bytes,
            "fallbackReason":fallback_reason,
            "pluginFailure":plugin_failure,
            "restart":false,
            "recovery":recovery
        })
    }

    fn log(&self, stage: &str) {
        eprintln!(
            "{}",
            self.event(stage, Duration::ZERO, None, 0, None, false, false)
        );
    }
}

fn log_operational_event(
    event: &str,
    uri: Option<&str>,
    session_id: Option<&str>,
    document_version: Option<i64>,
) {
    OperationalTrace::new(uri, session_id, document_version).log(event);
}

fn main() {
    let mut args = env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "lsp".into());
    if mode == "serve" {
        let Some(document) = args.next() else {
            eprintln!("usage: fleximarkd serve <document>");
            std::process::exit(2);
        };
        if let Err(error) = serve_document(&document) {
            let _ = error;
            log_operational_event("serve-failed", None, None, None);
            std::process::exit(1);
        }
        return;
    }
    if mode != "lsp" && mode != "rpc" {
        eprintln!("usage: fleximarkd <lsp|rpc|serve>");
        std::process::exit(2);
    }
    if let Err(error) = run(&mode) {
        let _ = error;
        log_operational_event("daemon-failed", None, None, None);
        std::process::exit(1);
    }
}

fn serve_document(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let uri = fleximark_service::path_to_file_uri(std::path::Path::new(path))?;
    let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
    registry.open(DidOpenParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            version: 1,
            text: source,
        },
    })?;
    let session_id = registry
        .session_id_for_uri(&uri)
        .expect("opened document")
        .to_owned();
    let daemon_id = registry.daemon_instance_id().to_owned();
    let publication = registry.render(&daemon_id, &session_id, 1, "serve-preview")?;
    let RenderPublication::Full(snapshot) = publication else {
        unreachable!("first render is full")
    };
    let previews = PreviewServer::start(None)?;
    println!(
        "{}",
        previews.publish(
            &random_token()?,
            &daemon_id,
            "serve-preview",
            &RenderPublication::Full(snapshot)
        )
    );
    loop {
        thread::park();
    }
}

fn run(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (outgoing, incoming) = mpsc::channel::<Value>();
    thread::spawn(move || {
        let mut writer = BufWriter::new(io::stdout());
        for message in incoming {
            if let Err(error) = write_frame(&mut writer, &message) {
                let _ = error;
                log_operational_event("stdout-write-failed", None, None, None);
                break;
            }
        }
    });
    let coordinator = Arc::new(CancellationCoordinator::default());
    let input_coordinator = Arc::clone(&coordinator);
    let (input_sender, input_receiver) = mpsc::channel::<InputEvent>();
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            match read_frame(&mut reader) {
                Ok(Some(body)) => {
                    let message = match serde_json::from_slice::<IncomingMessage>(&body) {
                        Ok(message) if message.jsonrpc == "2.0" => message,
                        _ => {
                            if input_sender.send(InputEvent::Invalid).is_err() {
                                break;
                            }
                            continue;
                        }
                    };
                    let permit = input_coordinator.prepare(&message);
                    if input_sender
                        .send(InputEvent::Message { message, permit })
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = input_sender.send(InputEvent::Closed);
                    break;
                }
                Err(error) => {
                    let _ = input_sender.send(InputEvent::Failed(error.to_string()));
                    break;
                }
            }
        }
    });
    let mut server = Server::with_sender(mode == "lsp", outgoing.clone());

    loop {
        let (message, permit) = match input_receiver.recv()? {
            InputEvent::Message { message, permit } => (message, permit),
            InputEvent::Invalid => {
                outgoing.send(response_value(Response::error(
                    Value::Null,
                    -32700,
                    "invalid JSON-RPC message",
                )))?;
                continue;
            }
            InputEvent::Closed => break,
            InputEvent::Failed(error) => return Err(error.into()),
        };
        let cancelled_before_start = permit
            .as_ref()
            .is_some_and(|permit| !coordinator.should_execute(permit));
        let mut messages = if cancelled_before_start {
            Vec::new()
        } else {
            server.handle_cancellable(
                message.clone(),
                permit.as_ref().map(|p| p.token.clone()),
                permit.as_ref().map(|p| p.publication_token.clone()),
            )
        };
        server.bind_document_alias(&message, &coordinator);
        let cancelled = permit
            .as_ref()
            .is_some_and(|permit| !coordinator.is_current(permit));
        if cancelled {
            messages.clear();
            if let Some(id) = message.id.clone() {
                messages.push(response_value(Response::error(
                    id,
                    -32800,
                    "request cancelled",
                )));
            }
        }
        if let Some(permit) = permit {
            coordinator.finish(&permit);
        }
        for outgoing_message in messages {
            outgoing.send(outgoing_message)?;
        }
        if server.exit {
            break;
        }
    }
    Ok(())
}

enum InputEvent {
    Message {
        message: IncomingMessage,
        permit: Option<WorkPermit>,
    },
    Invalid,
    Closed,
    Failed(String),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum DocumentKey {
    Uri(String),
    Session(String),
}

struct WorkPermit {
    work_id: u64,
    request_id: Option<String>,
    document: Option<DocumentKey>,
    generation: u64,
    is_mutation: bool,
    token: CancellationToken,
    publication_token: CancellationToken,
}

#[derive(Default)]
struct CancellationCoordinator {
    state: Mutex<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    next_work_id: u64,
    generations: HashMap<DocumentKey, u64>,
    session_uris: HashMap<String, String>,
    work: HashMap<u64, (Option<DocumentKey>, CancellationToken, CancellationToken)>,
    requests: HashMap<String, (u64, CancellationToken, CancellationToken)>,
}

impl CancellationCoordinator {
    fn prepare(&self, message: &IncomingMessage) -> Option<WorkPermit> {
        let mut state = self.state.lock().expect("cancellation state poisoned");
        if message.method == "$/cancelRequest" {
            if let Some(id) = message.params.get("id").and_then(rpc_id_key)
                && let Some((_, token, publication_token)) = state.requests.get(&id)
            {
                token.cancel();
                publication_token.cancel();
            }
            return None;
        }

        let document = document_key(message);
        let is_mutation = is_document_mutation(&message.method);
        if is_mutation && let Some(document) = document.as_ref() {
            let canonical = canonical_document(&state, document);
            let generation = state.generations.entry(canonical.clone()).or_default();
            *generation += 1;
            for (active_document, _, publication_token) in state.work.values() {
                if active_document
                    .as_ref()
                    .is_some_and(|active| canonical_document(&state, active) == canonical)
                {
                    publication_token.cancel();
                }
            }
        }

        state.next_work_id += 1;
        let work_id = state.next_work_id;
        let generation = document
            .as_ref()
            .map(|document| {
                let canonical = canonical_document(&state, document);
                state.generations.get(&canonical).copied().unwrap_or(0)
            })
            .unwrap_or(0);
        let token = CancellationToken::default();
        let publication_token = if is_mutation {
            CancellationToken::default()
        } else {
            token.clone()
        };
        let request_id = message.id.as_ref().and_then(rpc_id_key);
        state.work.insert(
            work_id,
            (document.clone(), token.clone(), publication_token.clone()),
        );
        if let Some(request_id) = &request_id {
            state.requests.insert(
                request_id.clone(),
                (work_id, token.clone(), publication_token.clone()),
            );
        }
        Some(WorkPermit {
            work_id,
            request_id,
            document,
            generation,
            is_mutation,
            token,
            publication_token,
        })
    }

    fn should_execute(&self, permit: &WorkPermit) -> bool {
        !permit.token.is_cancelled() && (permit.is_mutation || self.is_current(permit))
    }

    fn is_current(&self, permit: &WorkPermit) -> bool {
        if permit.publication_token.is_cancelled() {
            return false;
        }
        let state = self.state.lock().expect("cancellation state poisoned");
        permit.document.as_ref().is_none_or(|document| {
            let canonical = canonical_document(&state, document);
            state.generations.get(&canonical).copied().unwrap_or(0) == permit.generation
        })
    }

    fn finish(&self, permit: &WorkPermit) {
        let mut state = self.state.lock().expect("cancellation state poisoned");
        state.work.remove(&permit.work_id);
        if let Some(request_id) = &permit.request_id
            && state
                .requests
                .get(request_id)
                .is_some_and(|(work_id, _, _)| *work_id == permit.work_id)
        {
            state.requests.remove(request_id);
        }
    }

    fn bind(&self, session_id: &str, uri: &str) {
        let mut state = self.state.lock().expect("cancellation state poisoned");
        state
            .session_uris
            .insert(session_id.to_owned(), uri.to_owned());
        let session_key = DocumentKey::Session(session_id.to_owned());
        let uri_key = DocumentKey::Uri(uri.to_owned());
        let generation = state
            .generations
            .get(&session_key)
            .copied()
            .unwrap_or(0)
            .max(state.generations.get(&uri_key).copied().unwrap_or(0));
        state.generations.insert(uri_key, generation);
        state.generations.remove(&session_key);
    }
}

fn canonical_document(state: &CancellationState, document: &DocumentKey) -> DocumentKey {
    match document {
        DocumentKey::Session(session_id) => state
            .session_uris
            .get(session_id)
            .cloned()
            .map(DocumentKey::Uri)
            .unwrap_or_else(|| document.clone()),
        DocumentKey::Uri(_) => document.clone(),
    }
}

fn rpc_id_key(value: &Value) -> Option<String> {
    match value {
        Value::String(_) | Value::Number(_) => serde_json::to_string(value).ok(),
        _ => None,
    }
}

fn document_key(message: &IncomingMessage) -> Option<DocumentKey> {
    message
        .params
        .pointer("/textDocument/uri")
        .or_else(|| message.params.get("uri"))
        .and_then(Value::as_str)
        .map(|uri| DocumentKey::Uri(uri.to_owned()))
        .or_else(|| {
            message
                .params
                .get("documentSessionId")
                .and_then(Value::as_str)
                .map(|session| DocumentKey::Session(session.to_owned()))
        })
}

fn is_document_mutation(method_name: &str) -> bool {
    matches!(
        method_name,
        "textDocument/didOpen"
            | "textDocument/didChange"
            | "textDocument/didClose"
            | method::OPEN_DOCUMENT
            | method::CHANGE_DOCUMENT
            | method::CLOSE_DOCUMENT
    )
}

struct Server {
    lsp_mode: bool,
    lsp_initialized: bool,
    fleximark_initialized: bool,
    selection_events: bool,
    viewport_events: bool,
    workspaces: HashMap<String, bool>,
    disabled_workspaces: HashSet<String>,
    config_generation: u64,
    shutdown: bool,
    exit: bool,
    registry: SessionRegistry,
    previews: PreviewServer,
    preview_states: HashMap<String, PreviewState>,
    outgoing_events: Vec<Value>,
    work_cancellation: CancellationToken,
    publication_cancellation: CancellationToken,
}

impl Server {
    #[cfg(test)]
    fn new(lsp_mode: bool) -> Self {
        Self::build(lsp_mode, None)
    }

    fn with_sender(lsp_mode: bool, sender: Sender<Value>) -> Self {
        Self::build(lsp_mode, Some(sender))
    }

    fn build(lsp_mode: bool, sender: Option<Sender<Value>>) -> Self {
        Self {
            lsp_mode,
            lsp_initialized: false,
            fleximark_initialized: false,
            selection_events: false,
            viewport_events: false,
            workspaces: HashMap::new(),
            disabled_workspaces: HashSet::new(),
            config_generation: 0,
            shutdown: false,
            exit: false,
            registry: SessionRegistry::new(PositionEncoding::Utf16),
            previews: PreviewServer::start(sender).expect("loopback preview server must start"),
            preview_states: HashMap::new(),
            outgoing_events: Vec::new(),
            work_cancellation: CancellationToken::default(),
            publication_cancellation: CancellationToken::default(),
        }
    }

    fn handle_cancellable(
        &mut self,
        message: IncomingMessage,
        cancellation: Option<CancellationToken>,
        publication_cancellation: Option<CancellationToken>,
    ) -> Vec<Value> {
        self.work_cancellation = cancellation.unwrap_or_default();
        self.publication_cancellation = publication_cancellation.unwrap_or_default();
        let outgoing = self.handle(message);
        self.work_cancellation = CancellationToken::default();
        self.publication_cancellation = CancellationToken::default();
        outgoing
    }

    fn bind_document_alias(
        &self,
        message: &IncomingMessage,
        coordinator: &CancellationCoordinator,
    ) {
        let uri = message
            .params
            .pointer("/textDocument/uri")
            .or_else(|| message.params.get("uri"))
            .and_then(Value::as_str);
        if let Some(uri) = uri
            && let Some(session_id) = self.registry.session_id_for_uri(uri)
        {
            coordinator.bind(session_id, uri);
        }
    }

    fn handle(&mut self, message: IncomingMessage) -> Vec<Value> {
        let id = message.id.clone();
        let response = match message.method.as_str() {
            "initialize" if self.lsp_mode => self.lsp_initialize(id, &message.params),
            "initialized" if self.lsp_mode => None,
            "shutdown" if self.lsp_mode => {
                self.shutdown = true;
                id.map(|id| response_value(Response::success(id, Value::Null)))
            }
            "exit" if self.lsp_mode => {
                self.exit = true;
                None
            }
            "$/cancelRequest" => None,
            "textDocument/didOpen" if self.lsp_mode => self.open_lsp_document(id, message.params),
            "textDocument/didChange" if self.lsp_mode => self.change_document(id, message.params),
            "textDocument/didClose" if self.lsp_mode => self.close_document(id, message.params),
            "textDocument/completion" if self.lsp_mode => self.completion(id, &message.params),
            "textDocument/hover" if self.lsp_mode => self.hover(id, &message.params),
            "textDocument/documentSymbol" if self.lsp_mode => {
                self.document_symbols(id, &message.params)
            }
            "textDocument/diagnostic" if self.lsp_mode => self.diagnostics(id, &message.params),
            "textDocument/codeAction" if self.lsp_mode => self.code_actions(id, &message.params),
            method::INITIALIZE => self.fleximark_initialize(id, message.params),
            method::ATTACH_DOCUMENT => self.request(
                id,
                message.params,
                |registry, params: AttachDocumentParams| registry.attach(&params),
            ),
            method::CHECKPOINT_DOCUMENT => self.request(
                id,
                message.params,
                |registry, params: CheckpointDocumentParams| registry.checkpoint(&params),
            ),
            method::RENDER => self.render(id, message.params),
            method::CREATE_PREVIEW => self.create_preview(id, message.params),
            method::DISPOSE_PREVIEW => self.dispose_preview(id, message.params),
            method::SET_SELECTION => self.set_selection(id, message.params),
            method::SET_VIEWPORT => self.set_viewport(id, message.params),
            method::PREVIEW_EVENT => self.preview_event(id, message.params),
            method::RELOAD_PREVIEW => self.reload_preview(id, message.params),
            method::EXECUTE_COMMAND => self.execute_command(id, message.params),
            method::GET_NOTE_OPTIONS => self.get_note_options(id, message.params),
            method::RECONFIGURE_WORKSPACE => self.reconfigure_workspace(id, message.params),
            method::OPEN_DOCUMENT if !self.lsp_mode => self.open_rpc_document(id, message.params),
            method::CHANGE_DOCUMENT if !self.lsp_mode => {
                self.change_rpc_document(id, message.params)
            }
            method::CLOSE_DOCUMENT if !self.lsp_mode => self.request(
                id,
                message.params,
                |registry, params: RpcCloseDocumentParams| registry.close_rpc(params),
            ),
            _ => id.map(|id| response_value(Response::error(id, -32601, "method not found"))),
        };

        let mut outgoing = response.into_iter().collect::<Vec<_>>();
        outgoing.extend(
            self.registry
                .take_full_text_requests()
                .into_iter()
                .map(|params| {
                    serde_json::to_value(Notification {
                        jsonrpc: "2.0",
                        method: method::REQUEST_FULL_TEXT,
                        params,
                    })
                    .unwrap()
                }),
        );
        outgoing.append(&mut self.outgoing_events);
        outgoing
    }

    fn lsp_initialize(&mut self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let encoding = params
            .pointer("/capabilities/general/positionEncodings")
            .and_then(Value::as_array)
            .and_then(|encodings| {
                encodings
                    .iter()
                    .filter_map(Value::as_str)
                    .find_map(|value| match value {
                        "utf-8" => Some(PositionEncoding::Utf8),
                        "utf-16" => Some(PositionEncoding::Utf16),
                        "utf-32" => Some(PositionEncoding::Utf32),
                        _ => None,
                    })
            })
            .unwrap_or(PositionEncoding::Utf16);
        self.registry.set_position_encoding(encoding);
        self.lsp_initialized = true;
        let position_encoding = match encoding {
            PositionEncoding::Utf8 => "utf-8",
            PositionEncoding::Utf16 => "utf-16",
            PositionEncoding::Utf32 => "utf-32",
        };
        Some(response_value(Response::success(
            id,
            json!({
                "capabilities": {
                    "positionEncoding": position_encoding,
                    "textDocumentSync": { "openClose": true, "change": 2 },
                    "completionProvider": {"triggerCharacters":[":","`"]},
                    "hoverProvider": true,
                    "documentSymbolProvider": true,
                    "diagnosticProvider": {"interFileDependencies":false,"workspaceDiagnostics":false},
                    "codeActionProvider": true
                },
                "serverInfo": { "name": "fleximarkd", "version": env!("CARGO_PKG_VERSION") }
            }),
        )))
    }

    fn fleximark_initialize(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        if self.lsp_mode && !self.lsp_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "LSP initialize must complete first",
            )));
        }
        let params = match serde_json::from_value::<InitializeParams>(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        if params.protocol_version != PROTOCOL_VERSION {
            return Some(response_value(Response::error(
                id,
                -32001,
                format!(
                    "unsupported protocol version {}; expected {PROTOCOL_VERSION}",
                    params.protocol_version
                ),
            )));
        }
        let generation = self.config_generation + 1;
        let mut configured = Vec::new();
        let mut grants = HashMap::new();
        let mut workspace_statuses = Vec::new();
        let mut disabled_workspaces = HashSet::new();
        for workspace in params.workspaces {
            if grants
                .insert(workspace.uri.clone(), workspace.trusted)
                .is_some()
            {
                return Some(response_value(Response::error(
                    id,
                    -32602,
                    "workspace URI is duplicated",
                )));
            }
            match fleximark_service::load_plugin_host(&workspace.uri, workspace.trusted, generation)
            {
                Ok((host, render_config)) => {
                    configured.push((workspace.uri.clone(), host, render_config));
                    workspace_statuses.push(WorkspaceStatus {
                        uri: workspace.uri,
                        enabled: true,
                        error: None,
                    });
                }
                Err(error) => {
                    grants.insert(workspace.uri.clone(), false);
                    disabled_workspaces.insert(workspace.uri.clone());
                    workspace_statuses.push(WorkspaceStatus {
                        uri: workspace.uri,
                        enabled: false,
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        if let Err(error) = self.registry.configure_workspaces(configured) {
            return Some(session_error(id, error));
        }
        self.fleximark_initialized = true;
        self.config_generation = generation;
        self.selection_events = params.capabilities.selection_events;
        self.viewport_events = params.capabilities.viewport_events;
        self.workspaces = grants;
        self.disabled_workspaces = disabled_workspaces;
        Some(response_value(Response::success(
            id,
            InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                daemon_instance_id: self.registry.daemon_instance_id().to_owned(),
                workspace_statuses,
                capabilities: ServerCapabilities {
                    html_render: true,
                    document_checkpoint: true,
                    selection_events: self.selection_events,
                    viewport_events: self.viewport_events,
                    workspace_commands: vec![
                        "initializeWorkspace",
                        "editTheme",
                        "collectAdmonitions",
                        "createNote",
                        "exportHtml",
                    ],
                },
            },
        )))
    }

    fn completion(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let uri = params.pointer("/textDocument/uri").and_then(Value::as_str);
        let line = params.pointer("/position/line").and_then(Value::as_u64);
        let character = params
            .pointer("/position/character")
            .and_then(Value::as_u64);
        let (Some(uri), Some(line), Some(character)) = (uri, line, character) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "completion requires textDocument.uri and position",
            )));
        };
        let Ok(prefix) = self.registry.line_prefix(
            uri,
            fleximark_lsp::Position {
                line: line as u32,
                character: character as u32,
            },
        ) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "completion position is not in an open document",
            )));
        };
        let directive = prefix.trim_start().starts_with(':');
        let items = if directive {
            vec![
                json!({"label":"info admonition","insertText":":::info\n${1:content}\n:::","insertTextFormat":2}),
                json!({"label":"tabs","insertText":":::tabs\n${1:content}\n:::","insertTextFormat":2}),
                json!({"label":"details","insertText":":::details\n${1:content}\n:::","insertTextFormat":2}),
            ]
        } else {
            vec![
                json!({"label":"mermaid","insertText":"```mermaid\n${1:graph TD}\n```","insertTextFormat":2}),
                json!({"label":"abc","insertText":"```abc\n${1:X:1}\n```","insertTextFormat":2}),
                json!({"label":"math","insertText":"```math\n${1:formula}\n```","insertTextFormat":2}),
            ]
        };
        Some(response_value(Response::success(
            id,
            json!({"isIncomplete":false,"items":items}),
        )))
    }

    fn hover(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some((uri, line, character)) = params
            .pointer("/textDocument/uri")
            .and_then(Value::as_str)
            .zip(params.pointer("/position/line").and_then(Value::as_u64))
            .zip(
                params
                    .pointer("/position/character")
                    .and_then(Value::as_u64),
            )
            .map(|((uri, line), character)| (uri, line, character))
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "invalid hover parameters",
            )));
        };
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not open",
            )));
        };
        let Ok(version) = self
            .registry
            .current_version(self.registry.daemon_instance_id(), session_id)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not current",
            )));
        };
        let Ok(Some(entry)) = self.registry.navigation_at_position(
            self.registry.daemon_instance_id(),
            session_id,
            version,
            &fleximark_protocol::TextPosition { line, character },
        ) else {
            return Some(response_value(Response::success(id, Value::Null)));
        };
        let Ok(document) =
            self.registry
                .document(self.registry.daemon_instance_id(), session_id, version)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is unavailable",
            )));
        };
        let Some(kind) = find_block(document.document(), &entry.node_id)
            .map(|block| format!("{:?}", block.kind))
        else {
            return Some(response_value(Response::success(id, Value::Null)));
        };
        Some(response_value(Response::success(
            id,
            json!({"contents":{"kind":"markdown","value":format!("**FlexiMark block**: `{kind}`\n\nUTF-8 source bytes {}..{}", entry.source_range.byte_start, entry.source_range.byte_end)}}),
        )))
    }

    fn document_symbols(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "invalid symbol parameters",
            )));
        };
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not open",
            )));
        };
        let Ok(version) = self
            .registry
            .current_version(self.registry.daemon_instance_id(), session_id)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not current",
            )));
        };
        let Ok(document) =
            self.registry
                .document(self.registry.daemon_instance_id(), session_id, version)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is unavailable",
            )));
        };
        let mut symbols = Vec::new();
        collect_heading_symbols(&document.document().blocks, &mut symbols);
        Some(response_value(Response::success(id, symbols)))
    }

    fn diagnostics(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "invalid diagnostic parameters",
            )));
        };
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not open",
            )));
        };
        let Ok(version) = self
            .registry
            .current_version(self.registry.daemon_instance_id(), session_id)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is not current",
            )));
        };
        let Ok(document) =
            self.registry
                .document(self.registry.daemon_instance_id(), session_id, version)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "document is unavailable",
            )));
        };
        let diagnostics = collect_session_diagnostics(document);
        Some(response_value(Response::success(
            id,
            json!({"kind":"full","items":diagnostics}),
        )))
    }

    fn code_actions(&self, id: Option<Value>, params: &Value) -> Option<Value> {
        let id = id?;
        let Some(uri) = params.pointer("/textDocument/uri").and_then(Value::as_str) else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "invalid code action parameters",
            )));
        };
        let Some(context_diagnostics) = params
            .pointer("/context/diagnostics")
            .and_then(Value::as_array)
        else {
            return Some(response_value(Response::error(
                id,
                -32602,
                "invalid code action parameters",
            )));
        };
        let diagnostic = context_diagnostics
            .iter()
            .find(|item| item.get("code") == Some(&json!("raw-html")));
        let actions = if let Some(diagnostic) = diagnostic {
            let mut changes = serde_json::Map::new();
            changes.insert(
                        uri.to_owned(),
                        json!([{
                            "range": diagnostic.get("range").cloned().unwrap_or(Value::Null),
                            "newText": diagnostic.pointer("/data/escapedText").and_then(Value::as_str).unwrap_or("")
                        }]),
                    );
            vec![json!({
                "title":"Escape raw HTML for safe preview",
                "kind":"quickfix",
                "diagnostics":[diagnostic],
                "edit":{"changes":changes}
            })]
        } else {
            Vec::new()
        };
        Some(response_value(Response::success(id, actions)))
    }

    fn publish_lsp_diagnostics(&mut self, uri: &str) {
        if !self.lsp_mode || self.publication_cancellation.is_cancelled() {
            return;
        }
        let Some(session_id) = self.registry.session_id_for_uri(uri) else {
            return;
        };
        let daemon = self.registry.daemon_instance_id();
        let Ok(version) = self.registry.current_version(daemon, session_id) else {
            return;
        };
        let Ok(document) = self.registry.document(daemon, session_id, version) else {
            return;
        };
        self.outgoing_events.push(json!({
            "jsonrpc":"2.0",
            "method":"textDocument/publishDiagnostics",
            "params":{"uri":uri,"version":version,"diagnostics":collect_session_diagnostics(document)}
        }));
    }

    fn request<P, R>(
        &mut self,
        id: Option<Value>,
        params: Value,
        action: impl FnOnce(&mut SessionRegistry, P) -> Result<R, SessionError>,
    ) -> Option<Value>
    where
        P: serde::de::DeserializeOwned,
        R: serde::Serialize,
    {
        let id = id?;
        if !self.fleximark_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "FlexiMark connection is not initialized",
            )));
        }
        let params = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        Some(match action(&mut self.registry, params) {
            Ok(result) => response_value(Response::success(id, result)),
            Err(error) => session_error(id, error),
        })
    }

    fn render(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        if !self.fleximark_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "FlexiMark connection is not initialized",
            )));
        }
        let params = match serde_json::from_value::<RenderParams>(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        let preview_id = format!("render-{}", params.document_session_id);
        let publication = match self.registry.render_with_cancellation(
            &params.daemon_instance_id,
            &params.document_session_id,
            params.document_version,
            &preview_id,
            &self.work_cancellation,
        ) {
            Ok(publication) => publication,
            Err(error) => return Some(session_error(id, error)),
        };
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        Some(response_value(Response::success(id, publication)))
    }

    fn create_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        if !self.fleximark_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "FlexiMark connection is not initialized",
            )));
        }
        let params = match serde_json::from_value::<CreatePreviewParams>(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        let preview_nonce = match random_token() {
            Ok(preview_nonce) => preview_nonce,
            Err(_) => {
                return Some(response_value(Response::error(
                    id,
                    -32603,
                    "operating system random source is unavailable",
                )));
            }
        };
        let preview_id = format!("preview-{preview_nonce}");
        let publication = match self.registry.render_with_cancellation(
            &params.daemon_instance_id,
            &params.document_session_id,
            params.expected_document_version,
            &preview_id,
            &self.work_cancellation,
        ) {
            Ok(publication) => publication,
            Err(error) => return Some(session_error(id, error)),
        };
        let RenderPublication::Full(snapshot) = &publication else {
            return Some(response_value(Response::error(
                id,
                -32603,
                "new preview did not produce a full snapshot",
            )));
        };
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        let (token, url) = if params.target == PreviewTarget::ExternalBrowser {
            let token = match random_token() {
                Ok(token) => token,
                Err(_) => {
                    return Some(response_value(Response::error(
                        id,
                        -32603,
                        "operating system random source is unavailable",
                    )));
                }
            };
            let url = self.previews.publish(
                &token,
                &params.daemon_instance_id,
                &preview_id,
                &publication,
            );
            (Some(token), Some(url))
        } else {
            (None, None)
        };
        self.preview_states.insert(
            preview_id.clone(),
            PreviewState {
                token,
                document_session_id: params.document_session_id,
                delivered_revision: snapshot.result_render_revision,
            },
        );
        Some(response_value(Response::success(
            id,
            CreatePreviewResult {
                preview_session_id: preview_id,
                url,
                initial_publication: publication,
            },
        )))
    }

    fn dispose_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<DisposePreviewParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return id.map(|id| session_error(id, SessionError::WrongDaemon));
        }
        if let Some(state) = self.preview_states.remove(&params.preview_session_id) {
            if let Err(error) = self.registry.dispose_preview(
                &params.daemon_instance_id,
                &state.document_session_id,
                &params.preview_session_id,
            ) {
                return id.map(|id| session_error(id, error));
            }
            if let Some(token) = &state.token {
                self.previews.remove(token);
            }
        }
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    fn close_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    -32600,
                    "LSP notification must not have an id",
                ))
            });
        }
        let params = match serde_json::from_value::<DidCloseParams>(params) {
            Ok(params) => params,
            Err(error) => {
                let _ = error;
                log_operational_event("did-close-invalid", None, None, None);
                return None;
            }
        };
        let uri = params.text_document.uri.clone();
        if let Some(session_id) = self.registry.session_id_for_uri(&uri) {
            let expired = self
                .preview_states
                .iter()
                .filter(|(_, state)| state.document_session_id == session_id)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for preview_id in expired {
                if let Some(state) = self.preview_states.remove(&preview_id) {
                    let daemon_instance_id = self.registry.daemon_instance_id().to_owned();
                    let _ = self.registry.dispose_preview(
                        &daemon_instance_id,
                        &state.document_session_id,
                        &preview_id,
                    );
                    if let Some(token) = &state.token {
                        self.previews.remove(token);
                    }
                }
            }
        }
        self.registry.close(params);
        if self.lsp_mode {
            self.outgoing_events.push(json!({
                "jsonrpc":"2.0","method":"textDocument/publishDiagnostics",
                "params":{"uri":uri,"diagnostics":[]}
            }));
        }
        None
    }

    fn open_lsp_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    -32600,
                    "LSP notification must not have an id",
                ))
            });
        }
        let params = match serde_json::from_value::<DidOpenParams>(params) {
            Ok(params) => params,
            Err(error) => {
                let _ = error;
                log_operational_event("did-open-invalid", None, None, None);
                return None;
            }
        };
        let uri = params.text_document.uri.clone();
        if self
            .workspace_for_document(&uri)
            .is_some_and(|(workspace_uri, _)| self.disabled_workspaces.contains(&workspace_uri))
        {
            log_operational_event("document-open-denied", Some(&uri), None, None);
            return None;
        }
        if let Err(error) = self
            .registry
            .open_with_cancellation(params, &self.work_cancellation)
            .and_then(|_| self.refresh_assets(&uri))
        {
            let _ = error;
            log_operational_event("document-open-failed", Some(&uri), None, None);
        } else {
            self.publish_lsp_diagnostics(&uri);
        }
        None
    }

    fn open_rpc_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match serde_json::from_value::<RpcOpenDocumentParams>(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        let uri = params.uri.clone();
        if self
            .workspace_for_document(&uri)
            .is_some_and(|(workspace_uri, _)| self.disabled_workspaces.contains(&workspace_uri))
        {
            return Some(response_value(Response::error(
                id,
                -32022,
                "workspace configuration is disabled",
            )));
        }
        Some(
            match self
                .registry
                .open_rpc_with_cancellation(params, &self.work_cancellation)
                .and_then(|result| {
                    self.refresh_assets(&uri)?;
                    Ok(result)
                }) {
                Ok(result) => response_value(Response::success(id, result)),
                Err(error) => session_error(id, error),
            },
        )
    }

    fn change_rpc_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        if !self.fleximark_initialized {
            return Some(response_value(Response::error(
                id,
                -32002,
                "FlexiMark connection is not initialized",
            )));
        }
        let params = match serde_json::from_value::<RpcChangeDocumentParams>(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        match self
            .registry
            .change_rpc_with_cancellation(params, &self.work_cancellation)
        {
            Ok(result) if !self.publication_cancellation.is_cancelled() => {
                Some(response_value(Response::success(id, result)))
            }
            Ok(_) => None,
            Err(error) => Some(session_error(id, error)),
        }
    }

    fn refresh_assets(&mut self, uri: &str) -> Result<(), SessionError> {
        let session_id = self
            .registry
            .session_id_for_uri(uri)
            .ok_or(SessionError::UnknownSession)?
            .to_owned();
        let daemon = self.registry.daemon_instance_id().to_owned();
        let version = self.registry.current_version(&daemon, &session_id)?;
        let session = self.registry.document(&daemon, &session_id, version)?;
        let Some(workspace_uri) = session.workspace_uri().map(str::to_owned) else {
            return Ok(());
        };
        if !self
            .workspaces
            .get(&workspace_uri)
            .copied()
            .unwrap_or(false)
        {
            return Ok(());
        }
        let document = session.document().clone();
        let assets = fleximark_service::resolve_render_assets(uri, &workspace_uri, &document)
            .map_err(|error| SessionError::Engine(error.to_string()))?;
        self.registry.reconfigure_document_assets(uri, assets)
    }

    fn change_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    -32600,
                    "LSP notification must not have an id",
                ))
            });
        }
        let params = match serde_json::from_value::<DidChangeParams>(params) {
            Ok(params) => params,
            Err(error) => {
                let _ = error;
                log_operational_event("did-change-invalid", None, None, None);
                return None;
            }
        };
        let uri = params.text_document.uri.clone();
        let document_version = params.text_document.version;
        let trace = OperationalTrace::new(Some(&uri), None, Some(document_version));
        trace.log("change-received");
        let transform_started = Instant::now();
        if let Err(error) = self
            .registry
            .change_with_cancellation(params, &self.work_cancellation)
        {
            let _ = error;
            eprintln!(
                "{}",
                trace.event(
                    "document-sync-failed",
                    transform_started.elapsed(),
                    None,
                    0,
                    None,
                    true,
                    false,
                )
            );
            return None;
        }
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        eprintln!(
            "{}",
            trace.event(
                "transform-complete",
                transform_started.elapsed(),
                None,
                0,
                None,
                false,
                false,
            )
        );
        let asset_started = Instant::now();
        if let Err(error) = self.refresh_assets(&uri) {
            let _ = error;
            eprintln!(
                "{}",
                trace.event(
                    "asset-refresh-failed",
                    asset_started.elapsed(),
                    None,
                    0,
                    None,
                    false,
                    false,
                )
            );
            return None;
        }
        self.publish_lsp_diagnostics(&uri);
        let session_id = self.registry.session_id_for_uri(&uri).map(str::to_owned)?;
        let daemon_id = self.registry.daemon_instance_id().to_owned();
        let version = self
            .registry
            .current_version(&daemon_id, &session_id)
            .expect("changed document remains current");
        let preview_ids = self
            .preview_states
            .iter()
            .filter(|(_, state)| state.document_session_id == session_id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for preview_id in preview_ids {
            let force_full = self
                .preview_states
                .get(&preview_id)
                .and_then(|state| state.token.as_deref())
                .is_some_and(|token| self.previews.needs_full(token));
            let publication = if force_full {
                self.registry
                    .render_full_with_cancellation(
                        &daemon_id,
                        &session_id,
                        version,
                        &preview_id,
                        &self.work_cancellation,
                    )
                    .map(RenderPublication::Full)
            } else {
                self.registry.render_with_cancellation(
                    &daemon_id,
                    &session_id,
                    version,
                    &preview_id,
                    &self.work_cancellation,
                )
            };
            let publication = match publication {
                Ok(publication) => publication,
                Err(error) => {
                    let _ = error;
                    log_operational_event(
                        "preview-update-failed",
                        Some(&uri),
                        Some(&session_id),
                        Some(version),
                    );
                    continue;
                }
            };
            let revision = match &publication {
                RenderPublication::Full(snapshot) => snapshot.result_render_revision,
                RenderPublication::Patch(patch) => patch.result_render_revision,
            };
            if self.publication_cancellation.is_cancelled() {
                return None;
            }
            if let Some(token) = self
                .preview_states
                .get(&preview_id)
                .and_then(|state| state.token.as_deref())
            {
                if !self.previews.update(token, &publication) {
                    let full = match self.registry.render_full_with_cancellation(
                        &daemon_id,
                        &session_id,
                        version,
                        &preview_id,
                        &self.work_cancellation,
                    ) {
                        Ok(snapshot) => RenderPublication::Full(snapshot),
                        Err(error) => {
                            let _ = error;
                            log_operational_event(
                                "preview-collapse-failed",
                                Some(&uri),
                                Some(&session_id),
                                Some(version),
                            );
                            continue;
                        }
                    };
                    self.previews.update(token, &full);
                    let RenderPublication::Full(snapshot) = full else {
                        unreachable!()
                    };
                    if let Some(state) = self.preview_states.get_mut(&preview_id) {
                        state.delivered_revision = snapshot.result_render_revision;
                    }
                    self.outgoing_events.push(json!({
                        "jsonrpc":"2.0", "method":method::PREVIEW_EVENT,
                        "params":{"daemonInstanceId":daemon_id,"previewSessionId":preview_id,
                            "renderRevision":snapshot.result_render_revision,
                            "event":RenderPublication::Full(snapshot)}
                    }));
                    continue;
                }
            }
            if let Some(state) = self.preview_states.get_mut(&preview_id) {
                state.delivered_revision = revision;
            }
            self.outgoing_events.push(json!({
                "jsonrpc":"2.0", "method":method::PREVIEW_EVENT,
                "params":{"daemonInstanceId":daemon_id,"previewSessionId":preview_id,
                    "renderRevision":revision,"event":publication}
            }));
        }
        None
    }

    fn set_selection(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<SetSelectionParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        let mut node_ids = Vec::new();
        for selection in &params.selections {
            match self.registry.navigation_at_position(
                &params.daemon_instance_id,
                &params.document_session_id,
                params.expected_document_version,
                &selection.active,
            ) {
                Ok(Some(entry)) if !node_ids.contains(&entry.node_id) => {
                    node_ids.push(entry.node_id)
                }
                Ok(_) => {}
                Err(error) => return id.map(|id| session_error(id, error)),
            }
        }
        if !self.selection_events {
            return id.map(|id| response_value(Response::success(id, Value::Null)));
        }
        for (preview_id, state) in &self.preview_states {
            if state.document_session_id == params.document_session_id {
                let event = RenderNavigationEvent::Selection {
                    preview_session_id: preview_id.clone(),
                    render_revision: state.delivered_revision,
                    node_ids: node_ids.clone(),
                };
                if let Some(token) = &state.token {
                    self.previews.navigate(token, &event);
                }
                self.outgoing_events.push(json!({
                    "jsonrpc":"2.0", "method":method::PREVIEW_EVENT,
                    "params":{"daemonInstanceId":params.daemon_instance_id,
                        "previewSessionId":preview_id,"renderRevision":state.delivered_revision,
                        "event":event}
                }));
            }
        }
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    fn set_viewport(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<SetViewportParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        let node_id = match params.ranges.first() {
            Some(range) => match self.registry.navigation_at_position(
                &params.daemon_instance_id,
                &params.document_session_id,
                params.expected_document_version,
                &range.start,
            ) {
                Ok(Some(entry)) => Some(entry.node_id),
                Ok(None) => None,
                Err(error) => return id.map(|id| session_error(id, error)),
            },
            None => None,
        };
        if !self.viewport_events {
            return id.map(|id| response_value(Response::success(id, Value::Null)));
        }
        let Some(node_id) = node_id else {
            return id.map(|id| response_value(Response::success(id, Value::Null)));
        };
        for (preview_id, state) in &self.preview_states {
            if state.document_session_id == params.document_session_id {
                let event = RenderNavigationEvent::Viewport {
                    preview_session_id: preview_id.clone(),
                    render_revision: state.delivered_revision,
                    node_id: node_id.clone(),
                };
                if let Some(token) = &state.token {
                    self.previews.navigate(token, &event);
                }
                self.outgoing_events.push(json!({
                    "jsonrpc":"2.0", "method":method::PREVIEW_EVENT,
                    "params":{"daemonInstanceId":params.daemon_instance_id,
                        "previewSessionId":preview_id,"renderRevision":state.delivered_revision,
                        "event":event}
                }));
            }
        }
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    fn preview_event(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<PreviewEventParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return id.map(|id| session_error(id, SessionError::WrongDaemon));
        }
        let Some(state) = self.preview_states.get(&params.preview_session_id) else {
            return id
                .map(|id| response_value(Response::error(id, -32602, "unknown preview session")));
        };
        let navigation = params.event;
        let (event_preview_id, event_revision, node_id) = match &navigation {
            PreviewNavigationEvent::SelectNode {
                preview_session_id,
                render_revision,
                node_id,
            }
            | PreviewNavigationEvent::RevealNode {
                preview_session_id,
                render_revision,
                node_id,
            } => (preview_session_id, *render_revision, node_id),
        };
        if params.render_revision != state.delivered_revision
            || event_preview_id != &params.preview_session_id
            || event_revision != state.delivered_revision
        {
            return id.map(|id| {
                response_value(Response::error(
                    id,
                    CONTENT_MODIFIED,
                    "render revision does not match the displayed preview",
                ))
            });
        }
        let version = match self
            .registry
            .current_version(&params.daemon_instance_id, &state.document_session_id)
        {
            Ok(version) => version,
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let entry = match self.registry.navigation_for_node(
            &params.daemon_instance_id,
            &state.document_session_id,
            version,
            node_id,
        ) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                return id.map(|id| {
                    response_value(Response::error(id, -32602, "unknown rendered node"))
                });
            }
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let event = match navigation {
            PreviewNavigationEvent::SelectNode { .. } => SourceNavigationEvent::SelectSource {
                source_range: entry.source_range,
            },
            PreviewNavigationEvent::RevealNode { .. } => SourceNavigationEvent::RevealSource {
                source_range: entry.source_range,
            },
        };
        self.outgoing_events.push(json!({
            "jsonrpc":"2.0", "method":method::PREVIEW_EVENT,
            "params":{"daemonInstanceId":params.daemon_instance_id,
                "previewSessionId":params.preview_session_id,
                "renderRevision":state.delivered_revision,"event":event}
        }));
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    fn reload_preview(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let params = match serde_json::from_value::<ReloadPreviewParams>(params) {
            Ok(params) => params,
            Err(error) => return id.map(|id| invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return id.map(|id| session_error(id, SessionError::WrongDaemon));
        }
        let Some(state) = self.preview_states.get(&params.preview_session_id).cloned() else {
            return id
                .map(|id| response_value(Response::error(id, -32602, "unknown preview session")));
        };
        let document_version = match self
            .registry
            .current_version(&params.daemon_instance_id, &state.document_session_id)
        {
            Ok(version) => version,
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        let snapshot = match self.registry.render_full_with_cancellation(
            &params.daemon_instance_id,
            &state.document_session_id,
            document_version,
            &params.preview_session_id,
            &self.work_cancellation,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => return id.map(|id| session_error(id, error)),
        };
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        let revision = snapshot.result_render_revision;
        if let Some(token) = &state.token {
            self.previews
                .update(token, &RenderPublication::Full(snapshot.clone()));
        }
        self.preview_states
            .get_mut(&params.preview_session_id)
            .unwrap()
            .delivered_revision = revision;
        self.outgoing_events.push(json!({
            "jsonrpc":"2.0", "method":method::PREVIEW_EVENT,
            "params":{"daemonInstanceId":params.daemon_instance_id,
                "previewSessionId":params.preview_session_id,
                "renderRevision":revision,"event":RenderPublication::Full(snapshot)}
        }));
        id.map(|id| response_value(Response::success(id, Value::Null)))
    }

    fn execute_command(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match serde_json::from_value::<ExecuteCommandParams>(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return Some(session_error(id, SessionError::WrongDaemon));
        }
        let writes_workspace = matches!(
            params.command.as_str(),
            "initializeWorkspace"
                | "createNote"
                | "collectAdmonitions"
                | "exportHtml"
                | "acknowledgeExport"
        );
        if writes_workspace
            && !params
                .workspace_uri
                .as_deref()
                .is_some_and(|uri| self.workspace_grants_write(uri))
        {
            return Some(response_value(Response::error(
                id,
                -32021,
                "workspace write denied: the active workspace is not trusted",
            )));
        }
        let workspace = || {
            params
                .workspace_uri
                .as_deref()
                .ok_or_else(|| "command requires workspaceUri".to_owned())
        };
        let result = match params.command.as_str() {
            "initializeWorkspace" => workspace()
                .and_then(|uri| initialize_workspace(uri).map_err(|error| error.to_string())),
            "editTheme" => {
                workspace().and_then(|uri| edit_theme(uri).map_err(|error| error.to_string()))
            }
            "createNote" => workspace().and_then(|uri| {
                create_note_with_options(
                    uri,
                    params.note_category.as_deref(),
                    params.note_template.as_deref(),
                )
                .map_err(|error| error.to_string())
            }),
            "exportHtml" => match (
                &params.document_session_id,
                params.expected_document_version,
                params.workspace_uri.as_deref(),
            ) {
                (Some(session), Some(version), Some(workspace_uri)) => self
                    .registry
                    .document(&params.daemon_instance_id, session, version)
                    .map_err(|error| error.to_string())
                    .and_then(|document| {
                        if document.workspace_uri() != Some(workspace_uri) {
                            return Err(
                                "active document belongs to a different workspace authority".into(),
                            );
                        }
                        Ok(document.uri.clone())
                    })
                    .and_then(|source_uri| {
                        let destination_uri = params
                            .destination_uri
                            .clone()
                            .map(Ok)
                            .unwrap_or_else(|| default_export_destination(&source_uri))
                            .map_err(|error| error.to_string())?;
                        fleximark_service::preflight_export(
                            &source_uri,
                            workspace_uri,
                            &destination_uri,
                        )
                        .map_err(|error| error.to_string())?;
                        let export_context =
                            fleximark_service::export_render_context(workspace_uri)
                                .map_err(|error| error.to_string())?;
                        let mut assets = None;
                        let output = self
                            .registry
                            .export_html(
                                &params.daemon_instance_id,
                                session,
                                &export_context,
                                PREVIEW_CLIENT,
                                |safe_html, style, render_assets, runtime| {
                                    let resolved = fleximark_service::resolve_export_assets(
                                        &source_uri,
                                        workspace_uri,
                                        safe_html,
                                        render_assets,
                                    )
                                    .map_err(|error| SessionError::Engine(error.to_string()))?;
                                    assets = Some(resolved.assets);
                                    fleximark_service::compose_portable_html(
                                        &resolved.html,
                                        style,
                                        runtime,
                                    )
                                    .map_err(|error| SessionError::Engine(error.to_string()))
                                },
                            )
                            .map_err(|error| error.to_string())?;
                        export_html_with_safety(
                            &source_uri,
                            workspace_uri,
                            &destination_uri,
                            &output.html,
                            &assets.unwrap_or_default(),
                            output.unsafe_output_used,
                        )
                        .map_err(|error| error.to_string())
                    }),
                _ => Err("exportHtml requires an active document and workspaceUri".into()),
            },
            "acknowledgeExport" => match (
                &params.document_session_id,
                params.expected_document_version,
                params.workspace_uri.as_deref(),
            ) {
                (Some(session), Some(version), Some(workspace_uri)) => self
                    .registry
                    .document(&params.daemon_instance_id, session, version)
                    .map_err(|error| error.to_string())
                    .and_then(|document| {
                        if document.workspace_uri() != Some(workspace_uri) {
                            return Err(
                                "active document belongs to a different workspace authority".into(),
                            );
                        }
                        let source_uri = document.uri.clone();
                        let destination_uri = params
                            .destination_uri
                            .clone()
                            .map(Ok)
                            .unwrap_or_else(|| default_export_destination(&source_uri))
                            .map_err(|error| error.to_string())?;
                        acknowledge_export(&source_uri, workspace_uri, &destination_uri)
                            .map_err(|error| error.to_string())?;
                        Ok(fleximark_protocol::CommandResult {
                            message: Some(fleximark_protocol::CommandMessage {
                                level: "info",
                                text: "Export opened and validated; recovery backup released"
                                    .into(),
                            }),
                            open_uri: None,
                        })
                    }),
                _ => Err("acknowledgeExport requires an active document and workspaceUri".into()),
            },
            "collectAdmonitions" => match (
                &params.document_session_id,
                params.expected_document_version,
                params.workspace_uri.as_deref(),
            ) {
                (Some(session), Some(version), Some(workspace_uri)) => self
                    .registry
                    .document(&params.daemon_instance_id, session, version)
                    .map_err(|error| error.to_string())
                    .and_then(|document| {
                        if document.workspace_uri() != Some(workspace_uri) {
                            return Err(
                                "active document belongs to a different workspace authority".into(),
                            );
                        }
                        collect_admonitions(document.document(), document.source(), workspace_uri)
                            .map_err(|error| error.to_string())
                    }),
                _ => Err("collectAdmonitions requires an active document and workspaceUri".into()),
            },
            _ => Err(format!("unknown FlexiMark command: {}", params.command)),
        };
        Some(match result {
            Ok(result) => response_value(Response::success(id, result)),
            Err(message) => response_value(Response::error(id, -32020, message)),
        })
    }

    fn get_note_options(&self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match serde_json::from_value::<GetNoteOptionsParams>(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return Some(session_error(id, SessionError::WrongDaemon));
        }
        if !self.workspace_grants_write(&params.workspace_uri) {
            return Some(response_value(Response::error(
                id,
                -32021,
                "note options denied: the workspace is not trusted",
            )));
        }
        Some(match get_note_options(&params.workspace_uri) {
            Ok(options) => response_value(Response::success(id, options)),
            Err(error) => response_value(Response::error(id, -32020, error.to_string())),
        })
    }

    fn reconfigure_workspace(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match serde_json::from_value::<ReconfigureWorkspaceParams>(params) {
            Ok(params) => params,
            Err(error) => return Some(invalid_params(id, error)),
        };
        if params.daemon_instance_id != self.registry.daemon_instance_id() {
            return Some(session_error(id, SessionError::WrongDaemon));
        }
        if !self.workspaces.contains_key(&params.workspace_uri) {
            return Some(response_value(Response::error(
                id,
                -32602,
                "workspace is not registered",
            )));
        }
        let generation = self.config_generation + 1;
        let (host, render_config) = match fleximark_service::load_plugin_host(
            &params.workspace_uri,
            params.trusted,
            generation,
        ) {
            Ok(configured) => configured,
            Err(error) => {
                return Some(response_value(Response::error(
                    id,
                    -32022,
                    error.to_string(),
                )));
            }
        };
        let mut assets = HashMap::new();
        for (uri, document) in self.registry.workspace_documents(&params.workspace_uri) {
            let resolved = if params.trusted {
                match fleximark_service::resolve_render_assets(
                    &uri,
                    &params.workspace_uri,
                    &document,
                ) {
                    Ok(assets) => assets,
                    Err(error) => {
                        return Some(response_value(Response::error(
                            id,
                            -32022,
                            error.to_string(),
                        )));
                    }
                }
            } else {
                Vec::new()
            };
            assets.insert(uri, resolved);
        }
        if let Err(error) =
            self.registry
                .reconfigure_workspace(&params.workspace_uri, host, render_config, assets)
        {
            return Some(session_error(id, error));
        }
        self.config_generation = generation;
        self.workspaces
            .insert(params.workspace_uri.clone(), params.trusted);
        self.disabled_workspaces.remove(&params.workspace_uri);

        let daemon = self.registry.daemon_instance_id().to_owned();
        let sessions = self
            .registry
            .workspace_documents(&params.workspace_uri)
            .into_iter()
            .filter_map(|(uri, _)| self.registry.session_id_for_uri(&uri).map(str::to_owned))
            .collect::<Vec<_>>();
        for (preview_id, state) in self.preview_states.clone() {
            if !sessions.contains(&state.document_session_id) {
                continue;
            }
            let version = match self
                .registry
                .current_version(&daemon, &state.document_session_id)
            {
                Ok(version) => version,
                Err(error) => return Some(session_error(id, error)),
            };
            let snapshot = match self.registry.render_full_with_cancellation(
                &daemon,
                &state.document_session_id,
                version,
                &preview_id,
                &self.work_cancellation,
            ) {
                Ok(snapshot) => snapshot,
                Err(error) => return Some(session_error(id, error)),
            };
            if self.publication_cancellation.is_cancelled() {
                return None;
            }
            if let Some(token) = &state.token {
                self.previews
                    .update(token, &RenderPublication::Full(snapshot.clone()));
            }
            if let Some(current) = self.preview_states.get_mut(&preview_id) {
                current.delivered_revision = snapshot.result_render_revision;
            }
            self.outgoing_events.push(json!({
                "jsonrpc":"2.0","method":method::PREVIEW_EVENT,
                "params":{"daemonInstanceId":daemon,"previewSessionId":preview_id,
                    "renderRevision":snapshot.result_render_revision,
                    "event":RenderPublication::Full(snapshot)}
            }));
        }
        Some(response_value(Response::success(id, Value::Null)))
    }

    fn workspace_grants_write(&self, requested_uri: &str) -> bool {
        self.workspaces.iter().any(|(granted_uri, trusted)| {
            *trusted
                && match (
                    fleximark_service::workspace_path(granted_uri),
                    fleximark_service::workspace_path(requested_uri),
                ) {
                    (Ok(granted), Ok(requested)) => granted == requested,
                    _ => false,
                }
        })
    }

    fn workspace_for_document(&self, document_uri: &str) -> Option<(String, bool)> {
        self.workspaces
            .iter()
            .filter(|(workspace_uri, _)| {
                fleximark_service::document_is_in_workspace(document_uri, workspace_uri)
                    .unwrap_or(false)
            })
            .max_by_key(|(workspace_uri, _)| workspace_uri.len())
            .map(|(uri, trusted)| (uri.clone(), *trusted))
    }
}

fn find_block<'a>(document: &'a Document, node_id: &NodeId) -> Option<&'a Block> {
    fn visit<'a>(blocks: &'a [Block], node_id: &NodeId) -> Option<&'a Block> {
        for block in blocks {
            if &block.id == node_id {
                return Some(block);
            }
            for child in &block.children {
                if let Node::Block(child) = child
                    && let Some(found) = visit(std::slice::from_ref(child), node_id)
                {
                    return Some(found);
                }
            }
        }
        None
    }
    visit(&document.blocks, node_id)
}

fn inline_text(inline: &Inline, output: &mut String) {
    match &inline.kind {
        InlineKind::Text { value } | InlineKind::Code { value } => output.push_str(value),
        InlineKind::Emphasis { children }
        | InlineKind::Strong { children }
        | InlineKind::Strikethrough { children } => {
            for child in children {
                inline_text(child, output);
            }
        }
        InlineKind::Link { children, .. } | InlineKind::Image { children, .. } => {
            for child in children {
                inline_text(child, output);
            }
        }
        InlineKind::Math { source } => output.push_str(source),
        InlineKind::SoftBreak | InlineKind::HardBreak => output.push(' '),
        InlineKind::RawHtml { .. } => {}
    }
}

fn collect_heading_symbols(blocks: &[Block], output: &mut Vec<Value>) {
    for block in blocks {
        if let BlockKind::Heading { .. } = block.kind
            && let Some(range) = block.provenance.navigation_range()
        {
            let mut name = String::new();
            for child in &block.children {
                if let Node::Inline(inline) = child {
                    inline_text(inline, &mut name);
                }
            }
            output.push(json!({
                "name": if name.is_empty() { "Heading" } else { &name },
                "kind": 13,
                "range": {"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}},
                "selectionRange": {"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}}
            }));
        }
        let children = block
            .children
            .iter()
            .filter_map(|child| match child {
                Node::Block(block) => Some(block.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        collect_heading_symbols(&children, output);
    }
}

fn collect_raw_html_diagnostics(blocks: &[Block], output: &mut Vec<Value>) {
    for block in blocks {
        if let BlockKind::RawHtml { html } = &block.kind
            && let Some(range) = block.provenance.navigation_range()
        {
            output.push(json!({
                "range":{"start":{"line":range.start.line,"character":range.start.character},"end":{"line":range.end.line,"character":range.end.character}},
                "severity":2,
                "code":"raw-html",
                "source":"fleximark",
                "message":"Raw HTML is governed by the preview security policy",
                "data":{"escapedText":html.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")}
            }));
        }
        let children = block
            .children
            .iter()
            .filter_map(|child| match child {
                Node::Block(block) => Some(block.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        collect_raw_html_diagnostics(&children, output);
    }
}

fn collect_session_diagnostics(document: &fleximark_lsp::DocumentSession) -> Vec<Value> {
    let mut output = Vec::new();
    collect_raw_html_diagnostics(&document.document().blocks, &mut output);
    for diagnostic in document.asset_diagnostics() {
        let range = diagnostic.source_range.as_ref().map_or_else(
            || json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}}),
            |range| {
                json!({
                    "start":{"line":range.start.line,"character":range.start.character},
                    "end":{"line":range.end.line,"character":range.end.character}
                })
            },
        );
        output.push(json!({
            "range":range,
            "severity":1,
            "code":"asset",
            "source":"fleximark",
            "message":diagnostic.message
        }));
    }
    for diagnostic in document.plugin_diagnostics() {
        output.push(json!({
            "range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
            "severity":1,
            "code":"plugin",
            "source":format!("fleximark:{}", diagnostic.plugin_id),
            "message":diagnostic.message
        }));
    }
    output
}

#[derive(Clone)]
struct PreviewState {
    token: Option<String>,
    document_session_id: String,
    // Browser delivery state only; the engine owns the authoritative render revision.
    delivered_revision: u64,
}

struct PreviewServer {
    port: u16,
    pages: Arc<Mutex<HashMap<String, PreviewPage>>>,
}

#[derive(Clone)]
struct PreviewPage {
    shell: String,
    daemon_instance_id: String,
    preview_session_id: String,
    publications: Vec<StoredPublication>,
    publication_bytes: usize,
    next_sequence: u64,
    current_revision: u64,
    last_browser_event: Option<Instant>,
}

#[derive(Clone)]
struct StoredPublication {
    value: Value,
    encoded: String,
    sequence: u64,
}

const MAX_PREVIEW_PUBLICATIONS: usize = 32;
const MAX_PREVIEW_HISTORY_BYTES: usize = 8 * 1024 * 1024;

impl PreviewServer {
    fn start(sender: Option<Sender<Value>>) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let pages = Arc::new(Mutex::new(HashMap::new()));
        let (connections, receiver) = mpsc::sync_channel::<TcpStream>(16);
        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..4 {
            let shared = Arc::clone(&pages);
            let receiver = Arc::clone(&receiver);
            let sender = sender.clone();
            thread::spawn(move || {
                loop {
                    let stream = receiver.lock().expect("preview receiver lock").recv();
                    let Ok(stream) = stream else { return };
                    serve_preview_request(stream, port, &shared, sender.as_ref());
                }
            });
        }
        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        if let Err(error) = connections.try_send(stream) {
                            stream = match error {
                                mpsc::TrySendError::Full(stream)
                                | mpsc::TrySendError::Disconnected(stream) => stream,
                            };
                            let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                    Err(error) => {
                        let _ = error;
                        log_operational_event("preview-listener-failed", None, None, None);
                    }
                }
            }
        });
        Ok(Self { port, pages })
    }

    fn publish(
        &self,
        token: &str,
        daemon_instance_id: &str,
        preview_session_id: &str,
        publication: &RenderPublication,
    ) -> String {
        let shell = preview_shell(token);
        let value = serde_json::to_value(publication).expect("publication is serializable");
        let encoded = serde_json::to_string(std::slice::from_ref(&value))
            .expect("preview publication is serializable");
        let publication_bytes = encoded.len();
        let current_revision = match publication {
            RenderPublication::Full(snapshot) => snapshot.result_render_revision,
            RenderPublication::Patch(patch) => patch.result_render_revision,
        };
        self.pages.lock().expect("preview map lock").insert(
            token.to_owned(),
            PreviewPage {
                shell,
                daemon_instance_id: daemon_instance_id.to_owned(),
                preview_session_id: preview_session_id.to_owned(),
                publications: vec![StoredPublication {
                    value,
                    encoded,
                    sequence: 1,
                }],
                publication_bytes,
                next_sequence: 2,
                current_revision,
                last_browser_event: None,
            },
        );
        format!("http://127.0.0.1:{}/preview/{token}", self.port)
    }

    fn remove(&self, token: &str) {
        self.pages.lock().expect("preview map lock").remove(token);
    }

    fn update(&self, token: &str, publication: &RenderPublication) -> bool {
        if let Some(page) = self.pages.lock().expect("preview map lock").get_mut(token) {
            let value = serde_json::to_value(publication).expect("publication is serializable");
            let encoded = serde_json::to_string(std::slice::from_ref(&value))
                .expect("preview publication is serializable");
            if matches!(publication, RenderPublication::Full(_)) {
                page.publications.clear();
                page.publication_bytes = 0;
            } else if page.publications.len() >= MAX_PREVIEW_PUBLICATIONS
                || page.publication_bytes.saturating_add(encoded.len()) > MAX_PREVIEW_HISTORY_BYTES
            {
                return false;
            }
            page.publication_bytes = page.publication_bytes.saturating_add(encoded.len());
            page.current_revision = match publication {
                RenderPublication::Full(snapshot) => snapshot.result_render_revision,
                RenderPublication::Patch(patch) => patch.result_render_revision,
            };
            page.publications.push(StoredPublication {
                value,
                encoded,
                sequence: page.next_sequence,
            });
            page.next_sequence += 1;
            debug_assert!(page.publications.len() <= MAX_PREVIEW_PUBLICATIONS);
            return true;
        }
        false
    }

    fn navigate(&self, token: &str, event: &RenderNavigationEvent) -> bool {
        let value = serde_json::to_value(event).expect("navigation event is serializable");
        let encoded = serde_json::to_string(std::slice::from_ref(&value))
            .expect("navigation event is serializable");
        let mut pages = self.pages.lock().expect("preview map lock");
        let Some(page) = pages.get_mut(token) else {
            return false;
        };
        if page.publications.len() >= MAX_PREVIEW_PUBLICATIONS
            || page.publication_bytes.saturating_add(encoded.len()) > MAX_PREVIEW_HISTORY_BYTES
        {
            return false;
        }
        page.publication_bytes += encoded.len();
        page.publications.push(StoredPublication {
            value,
            encoded,
            sequence: page.next_sequence,
        });
        page.next_sequence += 1;
        true
    }

    fn needs_full(&self, token: &str) -> bool {
        self.pages
            .lock()
            .expect("preview map lock")
            .get(token)
            .is_some_and(|page| {
                page.publications.len() >= MAX_PREVIEW_PUBLICATIONS
                    || page.publication_bytes >= MAX_PREVIEW_HISTORY_BYTES
            })
    }
}

fn preview_shell(token: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><style>[data-fleximark-selected=true]{{outline:2px solid Highlight;outline-offset:2px}}.fleximark-token-keyword{{color:#8959a8}}.fleximark-token-string{{color:#718c00}}.fleximark-token-number{{color:#f5871f}}.fleximark-token-comment{{color:#8e908c}}</style></head><body><main id=\"preview\"></main><script data-fleximark-live src=\"/preview/{token}/client.js\"></script></body></html>"
    )
}

const PREVIEW_CLIENT: &str = include_str!("../../../web/preview-client/browser-host.js");

struct RequestDeadline(Arc<AtomicBool>);

impl Drop for RequestDeadline {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn serve_preview_request(
    mut stream: TcpStream,
    port: u16,
    pages: &Arc<Mutex<HashMap<String, PreviewPage>>>,
    sender: Option<&Sender<Value>>,
) {
    let completed = Arc::new(AtomicBool::new(false));
    let _deadline = RequestDeadline(Arc::clone(&completed));
    if let Ok(deadline_stream) = stream.try_clone() {
        thread::spawn(move || {
            for _ in 0..200 {
                if completed.load(Ordering::Acquire) {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            let _ = deadline_stream.shutdown(Shutdown::Both);
        });
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut reader = BufReader::new((&mut stream).take(20 * 1024));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.len() > 2048 {
        return;
    }
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next();
    let request_path = request_parts.next();
    let version = request_parts.next();
    let path = request_path
        .filter(|_| matches!(method, Some("GET" | "POST")) && version == Some("HTTP/1.1"))
        .filter(|_| request_parts.next().is_none())
        .and_then(|path| path.strip_prefix("/preview/"));
    let mut origin = None;
    let mut host = None;
    let mut last_event_id = None;
    let mut content_length = None;
    let mut content_type = None;
    let mut header_bytes = request_line.len();
    let complete_headers = loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line).ok();
        if count.is_none() || count == Some(0) {
            break false;
        }
        if line == "\r\n" || line == "\n" {
            break true;
        }
        header_bytes += line.len();
        if line.len() > 8192 || header_bytes > 16 * 1024 {
            return;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("origin") {
                origin = Some(if origin.is_some() {
                    "duplicate-origin".to_owned()
                } else {
                    value.trim().to_owned()
                });
            }
            if name.eq_ignore_ascii_case("host") {
                host = Some(if host.is_some() {
                    "duplicate-host".to_owned()
                } else {
                    value.trim().to_owned()
                });
            }
            if name.eq_ignore_ascii_case("last-event-id") {
                last_event_id = value.trim().parse::<u64>().ok();
            }
            if name.eq_ignore_ascii_case("content-length") {
                content_length = if content_length.is_some() {
                    Some(usize::MAX)
                } else {
                    value.trim().parse::<usize>().ok()
                };
            }
            if name.eq_ignore_ascii_case("content-type") {
                content_type = Some(if content_type.is_some() {
                    "duplicate-content-type".to_owned()
                } else {
                    value.trim().to_ascii_lowercase()
                });
            }
        }
    };
    if !complete_headers {
        return;
    }
    let allowed_origin = origin.as_deref().is_none_or(|origin| {
        origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}")
    });
    let allowed_host = host.as_deref().is_some_and(|host| {
        host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
    });
    let post_origin_allowed = method != Some("POST") || origin.is_some() && allowed_origin;
    let Some(path) = path.filter(|_| allowed_origin && post_origin_allowed && allowed_host) else {
        let _ = stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    };
    let mut parts = path.split('/');
    let token = parts.next().unwrap_or("");
    let endpoint = parts.next();
    if parts.next().is_some() {
        let _ = stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    if method == Some("POST") && endpoint == Some("navigation") {
        let length = content_length.unwrap_or(usize::MAX);
        if length > 4096 || content_type.as_deref() != Some("application/json") {
            let _ = stream.write_all(
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        drop(reader);
        let navigation = match serde_json::from_slice::<PreviewNavigationEvent>(&body) {
            Ok(navigation) => navigation,
            Err(_) => {
                let _ = stream.write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                return;
            }
        };
        let (event_preview_id, event_revision, node_id) = match &navigation {
            PreviewNavigationEvent::SelectNode {
                preview_session_id,
                render_revision,
                node_id,
            }
            | PreviewNavigationEvent::RevealNode {
                preview_session_id,
                render_revision,
                node_id,
            } => (preview_session_id, *render_revision, node_id),
        };
        let mut locked_pages = pages.lock().expect("preview map lock");
        let Some(page) = locked_pages.get_mut(token) else {
            let _ = stream.write_all(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        };
        if event_preview_id != &page.preview_session_id || page.current_revision != event_revision {
            let _ = stream.write_all(
                b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        let now = Instant::now();
        if page
            .last_browser_event
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(20))
        {
            let _ = stream.write_all(
                b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        let entry = page
            .publications
            .iter()
            .rev()
            .find_map(|publication| publication.value.get("navigation"))
            .cloned()
            .and_then(|navigation| serde_json::from_value::<Vec<NavigationEntry>>(navigation).ok())
            .and_then(|navigation| {
                navigation
                    .into_iter()
                    .find(|entry| &entry.node_id == node_id)
            });
        let Some(entry) = entry else {
            let _ = stream.write_all(
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        };
        let event = match navigation {
            PreviewNavigationEvent::SelectNode { .. } => SourceNavigationEvent::SelectSource {
                source_range: entry.source_range,
            },
            PreviewNavigationEvent::RevealNode { .. } => SourceNavigationEvent::RevealSource {
                source_range: entry.source_range,
            },
        };
        let notification = json!({
            "jsonrpc":"2.0", "method":method::PREVIEW_EVENT,
            "params":{"daemonInstanceId":page.daemon_instance_id,
                "previewSessionId":page.preview_session_id,
                "renderRevision":event_revision,"event":event}
        });
        let Some(sender) = sender else {
            let _ = stream.write_all(
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        };
        if sender.send(notification).is_err() {
            let _ = stream.write_all(
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        page.last_browser_event = Some(now);
        let _ = stream.write_all(
            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        );
        return;
    }
    drop(reader);
    if endpoint == Some("events") {
        let events = pages.lock().ok().and_then(|pages| {
            pages.get(token).map(|page| {
                page.publications
                    .iter()
                    .filter(|publication| {
                        last_event_id.is_none_or(|last| publication.sequence > last)
                    })
                    .map(|publication| (publication.sequence, publication.encoded.clone()))
                    .collect::<Vec<_>>()
            })
        });
        if let Some(events) = events {
            let mut body = "retry: 250\n".to_owned();
            for (revision, publication) in events {
                body.push_str(&format!("id: {revision}\ndata: {publication}\n\n"));
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
        return;
    }
    if endpoint == Some("client.js") {
        if pages.lock().is_ok_and(|pages| pages.contains_key(token)) {
            let body = PREVIEW_CLIENT;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
        return;
    }
    let body = if endpoint.is_none() {
        pages
            .lock()
            .ok()
            .and_then(|pages| pages.get(token).map(|page| page.shell.clone()))
    } else {
        None
    };
    let (status, body) = body.map_or(("403 Forbidden", String::new()), |body| ("200 OK", body));
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; img-src 'self' data: blob:; media-src 'self' blob:; frame-src https://www.youtube-nocookie.com; object-src 'none'; style-src 'unsafe-inline'; script-src 'self'; connect-src 'self'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn random_token() -> io::Result<String> {
    let mut bytes = [0_u8; 24];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| io::Error::other(format!("OS random source failed: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn invalid_params(id: Value, error: impl std::fmt::Display) -> Value {
    response_value(Response::error(
        id,
        -32602,
        format!("invalid params: {error}"),
    ))
}

fn session_error(id: Value, error: SessionError) -> Value {
    let code = if matches!(
        error,
        SessionError::ContentModified
            | SessionError::StaleVersion
            | SessionError::VersionMismatch
            | SessionError::HashMismatch
    ) {
        CONTENT_MODIFIED
    } else {
        -32602
    };
    response_value(Response::error(id, code, error.to_string()))
}

fn response_value(response: Response) -> Value {
    serde_json::to_value(response).expect("response is serializable")
}

#[cfg(test)]
mod tests {
    use fleximark_lsp::content_hash;
    use fleximark_protocol::IncomingMessage;
    use serde_json::json;

    use super::*;

    fn message(id: Option<i64>, method: &str, params: Value) -> IncomingMessage {
        IncomingMessage {
            jsonrpc: "2.0".into(),
            id: id.map(Value::from),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn cancellation_intake_stops_requests_and_obsolete_document_work() {
        let coordinator = CancellationCoordinator::default();
        let render = message(
            Some(7),
            method::RENDER,
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
            0,
            None,
            false,
            false,
        );
        let second = trace.event(
            "render-complete",
            Duration::from_millis(3),
            Some(9),
            321,
            Some("history-lag"),
            false,
            false,
        );
        assert_eq!(first["correlationId"], second["correlationId"]);
        assert_eq!(second["renderRevision"], 9);
        assert_eq!(second["patchBytes"], 321);
        assert_eq!(second["fallbackReason"], "history-lag");
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
        let workspace = std::env::temp_dir().join(format!(
            "fleximark-command-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&workspace).unwrap();
        let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
        fleximark_service::initialize_workspace(&workspace_uri).unwrap();
        let document_path = workspace.join("doc.md");
        std::fs::write(&document_path, "# Hello\n").unwrap();
        let document_uri = fleximark_service::path_to_file_uri(&document_path).unwrap();
        server.handle(message(
            Some(1),
            "initialize",
            json!({"capabilities": {"general":{"positionEncodings":["utf-8"]}}}),
        ));
        server.handle(message(
            Some(2),
            method::INITIALIZE,
            json!({
                "protocolVersion": 1, "client":{"name":"test","version":"1"},
                "capabilities":{"selectionEvents":true,"viewportEvents":true},
                "workspaces":[{"uri":workspace_uri.clone(),"trusted":true}]
            }),
        ));
        server.handle(message(
            None,
            "textDocument/didOpen",
            json!({
                "textDocument":{"uri":document_uri.clone(),"version":1,"text":"# Hello\n"}
            }),
        ));
        let attached = server.handle(message(
            Some(3),
            method::ATTACH_DOCUMENT,
            json!({
                "daemonInstanceId":server.registry.daemon_instance_id(),
                "uri":document_uri.clone(), "expectedDocumentVersion":1,
                "contentHash":content_hash("# Hello\n")
            }),
        ));
        assert!(attached[0].get("result").is_some(), "{attached:?}");
        let session_id = attached[0]["result"]["documentSessionId"]
            .as_str()
            .unwrap()
            .to_owned();
        let checkpoint = server.handle(message(
            Some(4),
            method::CHECKPOINT_DOCUMENT,
            json!({
                "daemonInstanceId":server.registry.daemon_instance_id(),
                "documentSessionId":session_id.clone(),
                "documentVersion":1,
                "contentHash":content_hash("# Hello\n")
            }),
        ));
        assert_eq!(checkpoint[0]["result"]["documentVersion"], 1);
        let rendered = server.handle(message(
            Some(5),
            method::RENDER,
            json!({
                "daemonInstanceId":server.registry.daemon_instance_id(),
                "documentSessionId":session_id.clone(),
                "documentVersion":1
            }),
        ));
        assert!(
            rendered[0]["result"]["html"]
                .as_str()
                .unwrap()
                .contains("Hello")
        );

        let preview = server.handle(message(
            Some(6),
            method::CREATE_PREVIEW,
            json!({
                "daemonInstanceId":server.registry.daemon_instance_id(),
                "documentSessionId":session_id.clone(),
                "expectedDocumentVersion":1,
                "target":"externalBrowser"
            }),
        ));
        assert_eq!(preview[0]["result"]["initialPublication"]["type"], "full");
        assert!(
            preview[0]["result"]["url"]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
        let url = preview[0]["result"]["url"].as_str().unwrap();
        let address_and_path = url.strip_prefix("http://").unwrap();
        let (address, path) = address_and_path.split_once('/').unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        write!(stream, "GET /{path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
        let mut response = String::new();
        std::io::Read::read_to_string(&mut stream, &mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("Content-Security-Policy: default-src 'none'"));
        let mut events = TcpStream::connect(address).unwrap();
        write!(
            events,
            "GET /{path}/events HTTP/1.1\r\nHost: {address}\r\n\r\n"
        )
        .unwrap();
        let mut event_response = String::new();
        std::io::Read::read_to_string(&mut events, &mut event_response).unwrap();
        assert!(event_response.contains("Content-Type: text/event-stream"));
        assert!(event_response.contains("Hello"));
        assert!(event_response.contains("resultRenderRevision"));
        let mut client = TcpStream::connect(address).unwrap();
        write!(
            client,
            "GET /{path}/client.js HTTP/1.1\r\nHost: {address}\r\n\r\n"
        )
        .unwrap();
        let mut client_response = String::new();
        std::io::Read::read_to_string(&mut client, &mut client_response).unwrap();
        assert!(client_response.contains("Content-Type: application/javascript"));
        assert!(client_response.contains("EventSource"));

        let mut hostile = TcpStream::connect(address).unwrap();
        write!(
            hostile,
            "GET /{path} HTTP/1.1\r\nHost: {address}\r\nOrigin: https://evil.example\r\n\r\n"
        )
        .unwrap();
        let mut rejected = String::new();
        std::io::Read::read_to_string(&mut hostile, &mut rejected).unwrap();
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
        let selection = server.handle(message(
            None,
            method::SET_SELECTION,
            json!({
                "daemonInstanceId":server.registry.daemon_instance_id(),
                "documentSessionId":session_id.clone(),
                "expectedDocumentVersion":1,
                "selections":[{"anchor":{"line":0,"character":0},"active":{"line":0,"character":1}}]
            }),
        ));
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

        let viewport = server.handle(message(
            None,
            method::SET_VIEWPORT,
            json!({
                "daemonInstanceId":server.registry.daemon_instance_id(),
                "documentSessionId":session_id.clone(),
                "expectedDocumentVersion":1,
                "ranges":[{"start":{"line":0,"character":0},"end":{"line":1,"character":0}}]
            }),
        ));
        assert_eq!(viewport[0]["params"]["event"]["type"], "viewport");

        let browser_event = server.handle(message(
            None,
            method::PREVIEW_EVENT,
            json!({
                "daemonInstanceId":server.registry.daemon_instance_id(),
                "previewSessionId":preview_id.clone(),
                "renderRevision":1,
                "event":{"type":"selectNode","previewSessionId":preview_id,
                    "renderRevision":1,"nodeId":selected_node_id}
            }),
        ));
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
        let mut browser_post = TcpStream::connect(address).unwrap();
        write!(
            browser_post,
            "POST /{path}/navigation HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{post_body}",
            post_body.len()
        )
        .unwrap();
        let mut post_response = String::new();
        std::io::Read::read_to_string(&mut browser_post, &mut post_response).unwrap();
        assert!(post_response.starts_with("HTTP/1.1 204 No Content"));
        let external_event = browser_events.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(external_event["method"], method::PREVIEW_EVENT);
        assert_eq!(external_event["params"]["event"]["type"], "revealSource");
        assert_eq!(external_event["params"]["previewSessionId"], preview_id);

        let mut repeated_post = TcpStream::connect(address).unwrap();
        write!(
            repeated_post,
            "POST /{path}/navigation HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{post_body}",
            post_body.len()
        )
        .unwrap();
        let mut repeated_response = String::new();
        std::io::Read::read_to_string(&mut repeated_post, &mut repeated_response).unwrap();
        assert!(
            repeated_response.starts_with("HTTP/1.1 429 Too Many Requests"),
            "{repeated_response}"
        );

        let reload = server.handle(message(
            None,
            method::RELOAD_PREVIEW,
            json!({"daemonInstanceId":server.registry.daemon_instance_id(),"previewSessionId":preview_id.clone()}),
        ));
        assert_eq!(reload[0]["params"]["renderRevision"], 2);

        let exported = server.handle(message(
            Some(9),
            method::EXECUTE_COMMAND,
            json!({"daemonInstanceId":server.registry.daemon_instance_id(),"command":"exportHtml",
                "documentSessionId":session_id.clone(),"expectedDocumentVersion":1,
                "workspaceUri":workspace_uri.clone()}),
        ));
        assert!(
            exported[0]["result"]["openUri"]
                .as_str()
                .unwrap()
                .ends_with("index.html")
        );
        let collected = server.handle(message(
            Some(8),
            method::EXECUTE_COMMAND,
            json!({
                "daemonInstanceId":server.registry.daemon_instance_id(),"command":"collectAdmonitions",
                "documentSessionId":session_id,"expectedDocumentVersion":1,
                "workspaceUri":workspace_uri
            }),
        ));
        assert!(
            collected[0]["result"]["message"]["text"]
                .as_str()
                .unwrap()
                .contains("0 admonition")
        );
        let update = server.handle(message(
            None,
            "textDocument/didChange",
            json!({
                "textDocument":{"uri":document_uri.clone(),"version":2},
                "contentChanges":[{"text":"# Updated\n"}]
            }),
        ));
        let preview_update = update
            .iter()
            .find(|item| item["method"] == method::PREVIEW_EVENT)
            .expect("document changes publish a preview event");
        assert_eq!(preview_update["params"]["event"]["type"], "patch");

        for version in 3..103 {
            server.handle(message(
                None,
                "textDocument/didChange",
                json!({
                    "textDocument":{"uri":document_uri.clone(),"version":version},
                    "contentChanges":[{"text":format!("# Updated {version}\n")}]
                }),
            ));
        }
        let token = server.preview_states[&preview_id]
            .token
            .as_ref()
            .expect("external preview has a token");
        let pages = server.previews.pages.lock().unwrap();
        let page = &pages[token];
        assert!(page.publications.len() <= MAX_PREVIEW_PUBLICATIONS);
        assert!(
            page.publication_bytes <= MAX_PREVIEW_HISTORY_BYTES || page.publications.len() == 1,
            "one oversized full snapshot may exceed the history budget"
        );
        drop(pages);

        server.handle(message(
            None,
            "textDocument/didClose",
            json!({"textDocument":{"uri":document_uri}}),
        ));
        let mut expired = TcpStream::connect(address).unwrap();
        write!(expired, "GET /{path} HTTP/1.1\r\nHost: {address}\r\n\r\n").unwrap();
        let mut expired_response = String::new();
        std::io::Read::read_to_string(&mut expired, &mut expired_response).unwrap();
        assert!(expired_response.starts_with("HTTP/1.1 403 Forbidden"));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn stale_notification_emits_full_text_request() {
        let mut server = Server::new(true);
        server.handle(message(
            None,
            "textDocument/didOpen",
            json!({
                "textDocument":{"uri":"file:///doc.md","version":2,"text":"x"}
            }),
        ));
        let outgoing = server.handle(message(
            None,
            "textDocument/didChange",
            json!({
                "textDocument":{"uri":"file:///doc.md","version":2},
                "contentChanges":[{"text":"y"}]
            }),
        ));
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
        let workspace = std::env::temp_dir().join(format!(
            "fleximark-untrusted-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&workspace).unwrap();
        let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
        let mut server = Server::new(true);
        server.handle(message(
            Some(1),
            method::INITIALIZE,
            json!({
                "protocolVersion":1,"client":{"name":"test","version":"1"},
                "capabilities":{"selectionEvents":false,"viewportEvents":false},
                "workspaces":[{"uri":workspace_uri.clone(),"trusted":false}]
            }),
        ));
        let denied = server.handle(message(
            Some(2),
            method::EXECUTE_COMMAND,
            json!({"daemonInstanceId":server.registry.daemon_instance_id(),
                "command":"initializeWorkspace","workspaceUri":workspace_uri}),
        ));
        assert_eq!(denied[0]["error"]["code"], -32021);
        assert!(!workspace.join(".fleximark").exists());
        std::fs::remove_dir(workspace).unwrap();
    }

    #[test]
    fn one_daemon_keeps_distinct_multi_root_trust_and_render_configuration() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fleximark-multi-root-{nonce}"));
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
        let untrusted_document_uri =
            fleximark_service::path_to_file_uri(&untrusted_document).unwrap();
        let mut server = Server::new(false);
        let initialized = server.handle(message(
            Some(1),
            method::INITIALIZE,
            json!({"protocolVersion":1,"client":{"name":"test","version":"1"},
            "capabilities":{},"workspaces":[
                {"uri":trusted_uri,"trusted":true},
                {"uri":untrusted_uri,"trusted":false}
            ]}),
        ));
        let daemon = initialized[0]["result"]["daemonInstanceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let trusted_open = server.handle(message(
            Some(2),
            method::OPEN_DOCUMENT,
            json!({"daemonInstanceId":daemon,"uri":trusted_document_uri,
                "documentVersion":1,"text":"# Trusted\n"}),
        ));
        let untrusted_open = server.handle(message(
            Some(3),
            method::OPEN_DOCUMENT,
            json!({"daemonInstanceId":daemon,"uri":untrusted_document_uri,
                "documentVersion":1,"text":"# Untrusted\n"}),
        ));
        let trusted_session = trusted_open[0]["result"]["documentSessionId"]
            .as_str()
            .unwrap()
            .to_owned();
        let untrusted_session = untrusted_open[0]["result"]["documentSessionId"]
            .as_str()
            .unwrap()
            .to_owned();
        let trusted_preview = server.handle(message(
            Some(4),
            method::CREATE_PREVIEW,
            json!({"daemonInstanceId":daemon,"documentSessionId":trusted_session,
                "expectedDocumentVersion":1,"target":"embeddedHtml"}),
        ));
        let untrusted_preview = server.handle(message(
            Some(5),
            method::CREATE_PREVIEW,
            json!({"daemonInstanceId":daemon,"documentSessionId":untrusted_session,
                "expectedDocumentVersion":1,"target":"embeddedHtml"}),
        ));
        assert_eq!(
            trusted_preview[0]["result"]["initialPublication"]["style"]["css"],
            ":root { color: red; }"
        );
        assert!(untrusted_preview[0]["result"]["initialPublication"]["style"].is_null());
        assert!(trusted_preview[0]["result"].get("url").is_none());
        assert!(untrusted_preview[0]["result"].get("url").is_none());
        assert!(server.previews.pages.lock().unwrap().is_empty());
        let escalated = server.handle(message(
            Some(6),
            method::EXECUTE_COMMAND,
            json!({"daemonInstanceId":daemon,"command":"exportHtml",
                "documentSessionId":untrusted_session,"expectedDocumentVersion":1,
                "workspaceUri":trusted_uri}),
        ));
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
        let reconfigured = server.handle(message(
            Some(7),
            method::RECONFIGURE_WORKSPACE,
            json!({"daemonInstanceId":daemon,"workspaceUri":trusted_uri,"trusted":true}),
        ));
        assert!(reconfigured[0]["result"].is_null(), "{reconfigured:?}");
        let publication = reconfigured
            .iter()
            .find(|message| message["method"] == method::PREVIEW_EVENT)
            .expect("active trusted preview receives a full reconfiguration publication");
        assert_eq!(publication["params"]["event"]["type"], "full");
        assert_eq!(publication["params"]["event"]["documentVersion"], 1);
        assert_eq!(
            publication["params"]["event"]["style"]["css"],
            ":root { color: green; }"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_workspace_is_disabled_without_disabling_other_roots() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("fleximark-root-status-{nonce}"));
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
            "schema_version=1\nunknown=true\n",
        )
        .unwrap();
        let mut server = Server::new(false);
        let initialized = server.handle(message(
            Some(1),
            method::INITIALIZE,
            json!({"protocolVersion":1,"client":{"name":"test","version":"1"},
            "capabilities":{},"workspaces":[
                {"uri":valid_uri,"trusted":true},{"uri":invalid_uri,"trusted":true}
            ]}),
        ));
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
        let opened = server.handle(message(
            Some(2),
            method::OPEN_DOCUMENT,
            json!({"daemonInstanceId":daemon,
                "uri":fleximark_service::path_to_file_uri(&valid_document).unwrap(),
                "documentVersion":1,"text":"# Valid\n"}),
        ));
        assert!(opened[0]["result"]["documentSessionId"].is_string());
        let denied = server.handle(message(
            Some(3),
            method::OPEN_DOCUMENT,
            json!({"daemonInstanceId":daemon,
                "uri":fleximark_service::path_to_file_uri(&invalid_document).unwrap(),
                "documentVersion":1,"text":"# Invalid\n"}),
        ));
        assert_eq!(denied[0]["error"]["code"], -32022);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn note_options_rpc_reads_only_the_granted_canonical_config() {
        let workspace = std::env::temp_dir().join(format!(
            "fleximark-note-options-rpc-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&workspace).unwrap();
        let workspace_uri = fleximark_service::path_to_file_uri(&workspace).unwrap();
        fleximark_service::initialize_workspace(&workspace_uri).unwrap();
        std::fs::write(
            workspace.join(".fleximark/config.toml"),
            "schema_version = 1\n[notes.categories]\nwork = \"work\"\n[notes.templates]\ndaily = [\"# Daily\"]\n",
        )
        .unwrap();
        let mut server = Server::new(false);
        server.handle(message(
            Some(1),
            method::INITIALIZE,
            json!({
                "protocolVersion":1,"client":{"name":"test","version":"1"},
                "capabilities":{},
                "workspaces":[{"uri":workspace_uri.clone(),"trusted":true}]
            }),
        ));
        let response = server.handle(message(
            Some(2),
            method::GET_NOTE_OPTIONS,
            json!({"daemonInstanceId":server.registry.daemon_instance_id(),
                "workspaceUri":workspace_uri}),
        ));
        assert_eq!(response[0]["result"]["categories"], json!(["work"]));
        assert_eq!(response[0]["result"]["templates"], json!(["daily"]));
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn export_preflight_rejects_unmanaged_destination_before_render_hooks() {
        let workspace = std::env::temp_dir().join(format!(
            "fleximark-export-preflight-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&workspace).unwrap();
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
        server.handle(message(
            Some(1),
            method::INITIALIZE,
            json!({"protocolVersion":1,"client":{"name":"test","version":"1"},
                "capabilities":{},"workspaces":[{"uri":workspace_uri.clone(),"trusted":true}]}),
        ));
        let opened = server.handle(message(
            Some(2),
            method::OPEN_DOCUMENT,
            json!({"daemonInstanceId":server.registry.daemon_instance_id(),
                "uri":document_uri,"documentVersion":1,"text":"# Safe\n"}),
        ));
        let session = opened[0]["result"]["documentSessionId"].as_str().unwrap();
        let response = server.handle(message(
            Some(3),
            method::EXECUTE_COMMAND,
            json!({"daemonInstanceId":server.registry.daemon_instance_id(),"command":"exportHtml",
                "documentSessionId":session,"expectedDocumentVersion":1,
                "workspaceUri":workspace_uri,"destinationUri":destination_uri}),
        ));
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
        std::fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn completion_uses_authoritative_open_document_and_position() {
        let mut server = Server::new(true);
        server.handle(message(
            Some(1),
            "initialize",
            json!({"capabilities":{"general":{"positionEncodings":["utf-8"]}}}),
        ));
        server.handle(message(
            Some(2),
            method::INITIALIZE,
            json!({"protocolVersion":1,"client":{"name":"test","version":"1"},"capabilities":{}}),
        ));
        server.handle(message(
            None,
            "textDocument/didOpen",
            json!({"textDocument":{"uri":"file:///completion.md","version":7,"text":"::"}}),
        ));
        let completion = server.handle(message(
            Some(3),
            "textDocument/completion",
            json!({"textDocument":{"uri":"file:///completion.md"},"position":{"line":0,"character":2}}),
        ));
        assert_eq!(
            completion[0]["result"]["items"][0]["label"],
            "info admonition"
        );
        let attach = server.handle(message(
            Some(4),
            method::ATTACH_DOCUMENT,
            json!({"daemonInstanceId":server.registry.daemon_instance_id(),
                "uri":"file:///completion.md","expectedDocumentVersion":7,
                "contentHash":content_hash("::")}),
        ));
        assert!(attach[0]["result"]["documentSessionId"].is_string());
        let invalid = server.handle(message(
            Some(5),
            "textDocument/completion",
            json!({"textDocument":{"uri":"file:///completion.md"},"position":{"line":1,"character":0}}),
        ));
        assert_eq!(invalid[0]["error"]["code"], -32602);
    }

    #[test]
    fn lsp_features_share_the_authoritative_ir_and_always_respond() {
        let mut server = Server::new(true);
        server.handle(message(
            Some(1),
            "initialize",
            json!({"capabilities":{"general":{"positionEncodings":["utf-8"]}}}),
        ));
        server.handle(message(
            Some(2),
            method::INITIALIZE,
            json!({"protocolVersion":1,"client":{"name":"test","version":"1"},"capabilities":{}}),
        ));
        let opened = server.handle(message(
            None,
            "textDocument/didOpen",
            json!({"textDocument":{"uri":"file:///features.md","version":7,"text":"# Héllo\n\n<div>x</div>\n"}}),
        ));
        let published = opened
            .iter()
            .find(|item| item["method"] == "textDocument/publishDiagnostics")
            .unwrap();
        assert_eq!(published["params"]["version"], 7);
        assert_eq!(published["params"]["diagnostics"][0]["code"], "raw-html");

        let hover = server.handle(message(
            Some(3),
            "textDocument/hover",
            json!({"textDocument":{"uri":"file:///features.md"},"position":{"line":0,"character":3}}),
        ));
        assert!(
            hover[0]["result"]["contents"]["value"]
                .as_str()
                .unwrap()
                .contains("Heading")
        );
        let symbols = server.handle(message(
            Some(4),
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":"file:///features.md"}}),
        ));
        assert_eq!(symbols[0]["result"][0]["name"], "Héllo");
        assert!(
            symbols[0]["result"][0]["range"]["start"]
                .get("encoding")
                .is_none()
        );
        let diagnostics = server.handle(message(
            Some(5),
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":"file:///features.md"}}),
        ));
        let diagnostic = diagnostics[0]["result"]["items"][0].clone();
        let actions = server.handle(message(
            Some(6),
            "textDocument/codeAction",
            json!({"textDocument":{"uri":"file:///features.md"},"range":diagnostic["range"],"context":{"diagnostics":[diagnostic]}}),
        ));
        assert_eq!(actions[0]["result"][0]["kind"], "quickfix");
        assert_eq!(
            actions[0]["result"][0]["edit"]["changes"]["file:///features.md"][0]["newText"],
            "&lt;div&gt;x&lt;/div&gt;\n"
        );

        for method in [
            "textDocument/hover",
            "textDocument/documentSymbol",
            "textDocument/diagnostic",
            "textDocument/codeAction",
        ] {
            let invalid = server.handle(message(Some(7), method, json!({})));
            assert_eq!(invalid.len(), 1, "{method} dropped its request");
            assert_eq!(invalid[0]["error"]["code"], -32602, "{method}");
        }
    }

    #[test]
    fn preview_tokens_come_from_the_os_random_source() {
        let first = random_token().unwrap();
        let second = random_token().unwrap();
        assert_eq!(first.len(), 48);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn embedded_preview_client_matches_the_generated_bundle() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/preview-client/browser-host.js");
        assert_eq!(PREVIEW_CLIENT, std::fs::read_to_string(path).unwrap());
        assert!(preview_shell("token").contains("data-fleximark-live"));
    }

    #[test]
    fn standalone_rpc_uses_explicit_base_version_and_hash() {
        let mut server = Server::new(false);
        let initialized = server.handle(message(
            Some(1),
            method::INITIALIZE,
            json!({"protocolVersion":1,"client":{"name":"test","version":"1"},"capabilities":{}}),
        ));
        let daemon = initialized[0]["result"]["daemonInstanceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let opened = server.handle(message(
            Some(2),
            method::OPEN_DOCUMENT,
            json!({"daemonInstanceId":daemon.clone(),"uri":"file:///rpc.md","documentVersion":1,"text":"old\n"}),
        ));
        let session = opened[0]["result"]["documentSessionId"]
            .as_str()
            .unwrap()
            .to_owned();
        let changed = server.handle(message(
            Some(3),
            method::CHANGE_DOCUMENT,
            json!({
                "daemonInstanceId":daemon,"documentSessionId":session,
                "baseDocumentVersion":1,"baseContentHash":content_hash("old\n"),
                "documentVersion":4,"text":"new\n"
            }),
        ));
        assert_eq!(changed[0]["result"]["documentVersion"], 4);
    }
}
