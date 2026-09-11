use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use fleximark_engine::RenderPublication;
use fleximark_model::NavigationEntry;
use fleximark_protocol::{
    PreviewNavigationEvent, RenderNavigationEvent, SourceNavigationEvent, method,
};
use serde_json::{Value, json};

use crate::telemetry::log_operational_event;

pub(crate) struct PreviewServer {
    port: u16,
    pub(crate) pages: Arc<Mutex<HashMap<String, PreviewPage>>>,
}

#[derive(Clone)]
pub(crate) struct PreviewPage {
    pub(crate) shell: String,
    pub(crate) daemon_instance_id: String,
    pub(crate) preview_session_id: String,
    pub(crate) publications: Vec<StoredPublication>,
    pub(crate) publication_bytes: usize,
    pub(crate) next_sequence: u64,
    pub(crate) current_revision: u64,
    pub(crate) last_browser_event: Option<Instant>,
}

#[derive(Clone)]
pub(crate) struct StoredPublication {
    pub(crate) value: Value,
    pub(crate) encoded: String,
    pub(crate) sequence: u64,
}

pub(crate) const MAX_PREVIEW_PUBLICATIONS: usize = 32;
pub(crate) const MAX_PREVIEW_HISTORY_BYTES: usize = 8 * 1024 * 1024;

impl PreviewServer {
    pub(crate) fn start(sender: Option<Sender<Value>>) -> io::Result<Self> {
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

    pub(crate) fn publish(
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

    pub(crate) fn remove(&self, token: &str) {
        self.pages.lock().expect("preview map lock").remove(token);
    }

    pub(crate) fn update(&self, token: &str, publication: &RenderPublication) -> bool {
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

    pub(crate) fn navigate(&self, token: &str, event: &RenderNavigationEvent) -> bool {
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

    pub(crate) fn needs_full(&self, token: &str) -> bool {
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

pub(crate) fn preview_shell(token: &str) -> String {
    const DEFAULT_PREVIEW_CSS: &str = include_str!("../../../web/preview-client/fleximark.css");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><style>{DEFAULT_PREVIEW_CSS}</style></head><body><main id=\"preview\" class=\"markdown-body\"></main><script data-fleximark-live src=\"/preview/{token}/client.js\"></script></body></html>"
    )
}

pub(crate) const PREVIEW_CLIENT: &str = include_str!("../../../web/preview-client/browser-host.js");

struct RequestDeadline(Arc<AtomicBool>);

impl Drop for RequestDeadline {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

struct PreviewRequest {
    method: Option<String>,
    path: Option<String>,
    origin: Option<String>,
    host: Option<String>,
    last_event_id: Option<u64>,
    content_length: Option<usize>,
    content_type: Option<String>,
}

fn parse_preview_request(reader: &mut impl BufRead) -> Option<PreviewRequest> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.len() > 2048 {
        return None;
    }
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().map(str::to_owned);
    let request_path = request_parts.next();
    let version = request_parts.next();
    let path = request_path
        .filter(|_| {
            matches!(method.as_deref(), Some("GET" | "POST")) && version == Some("HTTP/1.1")
        })
        .filter(|_| request_parts.next().is_none())
        .and_then(|path| path.strip_prefix("/preview/"))
        .map(str::to_owned);
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
            return None;
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
    complete_headers.then_some(PreviewRequest {
        method,
        path,
        origin,
        host,
        last_event_id,
        content_length,
        content_type,
    })
}

fn authorized_preview_path(request: &PreviewRequest, port: u16) -> Option<&str> {
    let allowed_origin = request.origin.as_deref().is_none_or(|origin| {
        origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}")
    });
    let allowed_host = request.host.as_deref().is_some_and(|host| {
        host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
    });
    let post_origin_allowed =
        request.method.as_deref() != Some("POST") || request.origin.is_some() && allowed_origin;
    request
        .path
        .as_deref()
        .filter(|_| allowed_origin && post_origin_allowed && allowed_host)
}

fn write_preview_response(stream: &mut TcpStream, response: &[u8]) {
    let _ = stream.write_all(response);
}

pub(crate) fn serve_preview_request(
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
    let Some(request) = parse_preview_request(&mut reader) else {
        return;
    };
    let method = request.method.as_deref();
    let last_event_id = request.last_event_id;
    let content_length = request.content_length;
    let content_type = request.content_type.as_deref();
    let Some(path) = authorized_preview_path(&request, port) else {
        write_preview_response(
            &mut stream,
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    };
    let mut parts = path.split('/');
    let token = parts.next().unwrap_or("");
    let endpoint = parts.next();
    if parts.next().is_some() {
        write_preview_response(
            &mut stream,
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    }
    if method == Some("POST") && endpoint == Some("navigation") {
        let length = content_length.unwrap_or(usize::MAX);
        if length > 4096 || content_type != Some("application/json") {
            write_preview_response(
                &mut stream,
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
                write_preview_response(
                    &mut stream,
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
            write_preview_response(
                &mut stream,
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        };
        if event_preview_id != &page.preview_session_id || page.current_revision != event_revision {
            write_preview_response(
                &mut stream,
                b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        let now = Instant::now();
        if page
            .last_browser_event
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(20))
        {
            write_preview_response(
                &mut stream,
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
            write_preview_response(
                &mut stream,
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
            write_preview_response(
                &mut stream,
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        };
        if sender.send(notification).is_err() {
            write_preview_response(
                &mut stream,
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        }
        page.last_browser_event = Some(now);
        write_preview_response(
            &mut stream,
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
            write_preview_response(&mut stream, response.as_bytes());
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
            write_preview_response(&mut stream, response.as_bytes());
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
    write_preview_response(&mut stream, response.as_bytes());
}

pub(crate) fn random_token() -> io::Result<String> {
    let mut bytes = [0_u8; 24];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| io::Error::other(format!("OS random source failed: {error}")))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
