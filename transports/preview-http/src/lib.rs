//! Role: Serve authenticated loopback browser previews and navigation events.
mod assets;
pub mod protocol;
pub use assets::BrowserAssets;
mod workspace;
use workspace::serve_workspace_request;
mod state;
use state::WorkspaceService;
pub use state::{PreviewPage, PreviewServer, StoredEvent};
mod http;
use http::*;
use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fleximark_application::RenderFrame;
use fleximark_protocol::{
    JsSafeU64, PreviewChangedParams, PreviewNavigationEvent, RenderNavigationEvent,
    ServerPreviewEvent, ServerPreviewEventParams, SourceNavigationEvent,
};
use serde_json::Value;

use fleximark_protocol::Notification;
fn preview_event_notification(params: ServerPreviewEventParams) -> Value {
    serde_json::to_value(Notification {
        jsonrpc: "2.0",
        method: fleximark_protocol::method::PREVIEW_EVENT,
        params,
    })
    .expect("serializable event")
}
fn log_operational_event(event: &str, _: Option<&str>, _: Option<&str>, _: Option<i64>) {
    eprintln!("{event}");
}

impl PreviewServer {
    pub fn with_assets(sender: Option<Sender<Value>>, assets: BrowserAssets) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let pages = Arc::new(Mutex::new(HashMap::new()));
        let workspace = WorkspaceService::new(assets.clone());
        let (connections, receiver) = mpsc::sync_channel::<TcpStream>(16);
        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..4 {
            let shared = Arc::clone(&pages);
            let receiver = Arc::clone(&receiver);
            let sender = sender.clone();
            let workspace = workspace.clone();
            let assets = assets.clone();
            thread::spawn(move || {
                loop {
                    let stream = receiver.lock().expect("preview receiver lock").recv();
                    let Ok(stream) = stream else { return };
                    serve_request(stream, port, &shared, sender.as_ref(), &workspace, &assets);
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
        Ok(Self {
            port,
            pages,
            workspace,
            assets,
        })
    }

    pub fn publish(
        &self,
        token: &str,
        daemon_instance_id: &str,
        preview_session_id: &str,
        frame: &RenderFrame,
    ) -> Result<String, protocol::ConversionError> {
        let checked = protocol::frame(frame)?;
        let shell = self.assets.preview_shell(token);
        let changed = PreviewChangedParams {
            daemon_instance_id: daemon_instance_id.to_owned(),
            preview_session_id: preview_session_id.to_owned(),
            render_revision: checked.render_revision,
        };
        self.pages.lock().expect("preview map lock").insert(
            token.to_owned(),
            PreviewPage {
                shell,
                daemon_instance_id: daemon_instance_id.to_owned(),
                preview_session_id: preview_session_id.to_owned(),
                frame: frame.clone(),
                change_event: StoredEvent {
                    encoded: serde_json::to_string(&changed)
                        .expect("preview change is serializable"),
                    sequence: 1,
                },
                navigation_event: None,
                durable_navigation: None,
                next_sequence: 2,
                document_version: checked.document_version,
                last_browser_event: None,
            },
        );
        Ok(format!("http://127.0.0.1:{}/preview/{token}", self.port))
    }

    pub fn remove(&self, token: &str) {
        self.pages.lock().expect("preview map lock").remove(token);
    }

    pub fn update(
        &self,
        token: &str,
        frame: &RenderFrame,
    ) -> Result<bool, protocol::ConversionError> {
        let checked = protocol::frame(frame)?;
        if let Some(page) = self.pages.lock().expect("preview map lock").get_mut(token) {
            page.frame = frame.clone();
            page.document_version = checked.document_version;
            page.change_event = StoredEvent {
                encoded: serde_json::to_string(&PreviewChangedParams {
                    daemon_instance_id: page.daemon_instance_id.clone(),
                    preview_session_id: page.preview_session_id.clone(),
                    render_revision: checked.render_revision,
                })
                .expect("preview change is serializable"),
                sequence: page.next_sequence,
            };
            page.next_sequence += 1;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn mark_document_version(&self, token: &str, version: JsSafeU64) -> bool {
        let mut pages = self.pages.lock().expect("preview map lock");
        let Some(page) = pages.get_mut(token) else {
            return false;
        };
        page.document_version = version;
        true
    }

    pub fn retain_jump(&self, token: &str, event: &RenderNavigationEvent) -> bool {
        let mut pages = self.pages.lock().expect("preview map lock");
        let Some(page) = pages.get_mut(token) else {
            return false;
        };
        let sequence = page
            .durable_navigation
            .as_ref()
            .map(|event| event.sequence)
            .unwrap_or_else(|| {
                let sequence = page.next_sequence;
                page.next_sequence += 1;
                sequence
            });
        page.durable_navigation = Some(StoredEvent {
            encoded: serde_json::to_string(event).expect("navigation event is serializable"),
            sequence,
        });
        true
    }

    pub fn navigate(&self, token: &str, event: &RenderNavigationEvent) -> bool {
        let mut pages = self.pages.lock().expect("preview map lock");
        let Some(page) = pages.get_mut(token) else {
            return false;
        };
        page.navigation_event = Some(StoredEvent {
            encoded: serde_json::to_string(event).expect("navigation event is serializable"),
            sequence: page.next_sequence,
        });
        page.next_sequence += 1;
        true
    }
}

struct RequestDeadline(Arc<AtomicBool>);

impl Drop for RequestDeadline {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub fn serve_preview_request(
    stream: TcpStream,
    port: u16,
    pages: &Arc<Mutex<HashMap<String, PreviewPage>>>,
    sender: Option<&Sender<Value>>,
    assets: &BrowserAssets,
) {
    serve_request(
        stream,
        port,
        pages,
        sender,
        &WorkspaceService::new(assets.clone()),
        assets,
    );
}

fn serve_request(
    mut stream: TcpStream,
    port: u16,
    pages: &Arc<Mutex<HashMap<String, PreviewPage>>>,
    sender: Option<&Sender<Value>>,
    workspace: &WorkspaceService,
    assets: &BrowserAssets,
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
    let Some(request) = parse_preview_request(&mut reader) else {
        return;
    };
    let method = request.method.as_deref();
    let last_event_id = request.last_event_id;
    let content_length = request.content_length;
    let content_type = request.content_type.as_deref();
    let Some(path) = authorized_preview_path(&request, port) else {
        write_empty_response(&mut stream, FORBIDDEN_RESPONSE);
        return;
    };
    if let Some(path) = path.strip_prefix("workspace/") {
        let mut body = Vec::new();
        if request.method.as_deref() == Some("POST") {
            let length = request.content_length.unwrap_or(usize::MAX);
            if length > 4096 {
                drop(reader);
                write_empty_response(&mut stream, BAD_REQUEST_RESPONSE);
                return;
            }
            body.resize(length, 0);
            if reader.read_exact(&mut body).is_err() {
                return;
            }
        }
        drop(reader);
        serve_workspace_request(&mut stream, &mut body.as_slice(), &request, path, workspace);
        return;
    }
    let mut parts = path.split('/');
    let token = parts.next().unwrap_or("");
    let endpoint = parts.next();
    if parts.next().is_some() {
        write_empty_response(&mut stream, FORBIDDEN_RESPONSE);
        return;
    }
    if method == Some("POST") && endpoint == Some("navigation") {
        let length = content_length.unwrap_or(usize::MAX);
        if length > 4096 || content_type != Some("application/json") {
            write_empty_response(&mut stream, BAD_REQUEST_RESPONSE);
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
                write_empty_response(&mut stream, BAD_REQUEST_RESPONSE);
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
            write_empty_response(&mut stream, FORBIDDEN_RESPONSE);
            return;
        };
        if page.frame.preview_session_id.0 != *event_preview_id
            || page.frame.render_revision != event_revision.get()
            || page.frame.document_version != page.document_version.get()
        {
            write_empty_response(&mut stream, CONFLICT_RESPONSE);
            return;
        }
        let Some(entry) = page
            .frame
            .navigation
            .iter()
            .find(|entry| entry.node_id.0 == node_id.0)
        else {
            write_empty_response(&mut stream, BAD_REQUEST_RESPONSE);
            return;
        };
        if event_preview_id != &page.preview_session_id {
            write_empty_response(&mut stream, CONFLICT_RESPONSE);
            return;
        }
        let now = Instant::now();
        if page
            .last_browser_event
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(20))
        {
            write_empty_response(&mut stream, TOO_MANY_REQUESTS_RESPONSE);
            return;
        }
        let source_range = match protocol::source_range(&entry.source_range) {
            Ok(range) => range,
            Err(error) => {
                write_conversion_error(&mut stream, &error);
                return;
            }
        };
        let event = match navigation {
            PreviewNavigationEvent::SelectNode { .. } => {
                SourceNavigationEvent::SelectSource { source_range }
            }
            PreviewNavigationEvent::RevealNode { .. } => {
                SourceNavigationEvent::RevealSource { source_range }
            }
        };
        let notification = preview_event_notification(ServerPreviewEventParams {
            daemon_instance_id: page.daemon_instance_id.clone(),
            preview_session_id: page.preview_session_id.clone(),
            render_revision: event_revision,
            event: ServerPreviewEvent::SourceNavigation(event),
        });
        let Some(sender) = sender else {
            write_empty_response(&mut stream, UNAVAILABLE_RESPONSE);
            return;
        };
        if sender.send(notification).is_err() {
            write_empty_response(&mut stream, UNAVAILABLE_RESPONSE);
            return;
        }
        page.last_browser_event = Some(now);
        write_empty_response(&mut stream, NO_CONTENT_RESPONSE);
        return;
    }
    drop(reader);
    if endpoint == Some("events") {
        let events = pages.lock().ok().and_then(|pages| {
            pages.get(token).map(|page| {
                let mut events = std::iter::once(&page.change_event)
                    .chain(page.navigation_event.iter())
                    .filter(|event| last_event_id.is_none_or(|last| event.sequence > last))
                    .map(|event| (event.sequence, event.encoded.clone()))
                    .collect::<Vec<_>>();
                events.sort_unstable_by_key(|(sequence, _)| *sequence);
                events
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
            write_preview_response(&mut stream, response.as_bytes());
        }
        return;
    }
    if endpoint == Some("client.js") {
        if pages.lock().is_ok_and(|pages| pages.contains_key(token)) {
            let body = &assets.preview_client;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/javascript\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            write_preview_response(&mut stream, response.as_bytes());
        }
        return;
    }
    if endpoint == Some("frame") {
        let page = pages
            .lock()
            .ok()
            .and_then(|pages| pages.get(token).cloned());
        let body = if let Some(page) = page {
            let frame = match protocol::frame(&page.frame) {
                Ok(frame) => frame,
                Err(error) => {
                    write_conversion_error(&mut stream, &error);
                    return;
                }
            };
            let navigation = page.durable_navigation.as_ref().map(|navigation|serde_json::json!({"sequence":navigation.sequence,"event":serde_json::from_str::<Value>(&navigation.encoded).ok()}));
            Some(serde_json::json!({"frame":frame,"navigation":navigation}).to_string())
        } else {
            None
        };
        let (status, body) = body.map_or(("403 Forbidden", String::new()), |body| ("200 OK", body));
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        write_preview_response(&mut stream, response.as_bytes());
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
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; base-uri 'none'; form-action 'none'; font-src data:; img-src 'self' https://i.ytimg.com data: blob:; media-src 'self' blob:; frame-src https://www.youtube-nocookie.com; object-src 'none'; style-src 'unsafe-inline'; script-src 'self'; connect-src 'self'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    write_preview_response(&mut stream, response.as_bytes());
}

pub fn random_token() -> io::Result<String> {
    let mut bytes = [0_u8; 24];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| io::Error::other(format!("OS random source failed: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
