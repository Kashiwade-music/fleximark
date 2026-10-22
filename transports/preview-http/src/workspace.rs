//! Role: Serve token-scoped shared workspace forms and dispatch typed user operations.
use crate::assets::BrowserAssets;
use crate::http::*;
use crate::state::{Handler, PreviewServer, WorkspaceService};
use serde_json::Value;
use serde_json::json;
use std::io::Read;
use std::net::TcpStream;

impl WorkspaceService {
    pub(super) fn new(assets: BrowserAssets) -> Self {
        Self {
            assets,
            sessions: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            handler: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

impl PreviewServer {
    pub fn set_workspace_handler(&self, handler: Handler) {
        *self
            .workspace
            .handler
            .lock()
            .expect("workspace handler lock") = Some(handler);
    }

    pub fn publish_workspace(&self, token: &str, command: &str, context: &Value) -> String {
        self.workspace
            .sessions
            .lock()
            .expect("workspace sessions lock")
            .insert(
                token.to_owned(),
                json!({"command":command,"context":context}),
            );
        format!("http://127.0.0.1:{}/workspace/{token}", self.port)
    }
}

pub(super) fn serve_workspace_request(
    stream: &mut TcpStream,
    reader: &mut impl Read,
    request: &PreviewRequest,
    path: &str,
    service: &WorkspaceService,
) {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let mut parts = route.split('/');
    let token = parts.next().unwrap_or("");
    let endpoint = parts.next();
    if parts.next().is_some() {
        write_empty_response(stream, FORBIDDEN_RESPONSE);
        return;
    }
    let Some(session) = service
        .sessions
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(token).cloned())
    else {
        write_empty_response(stream, FORBIDDEN_RESPONSE);
        return;
    };
    match (request.method.as_deref(), endpoint) {
        (Some("GET"), None) => respond(
            stream,
            "text/html; charset=utf-8",
            &service
                .assets
                .workspace_html
                .replace("__WORKSPACE_BASE__", &format!("/workspace/{token}")),
        ),
        (Some("GET"), Some("client.js")) => respond(
            stream,
            "application/javascript",
            &service.assets.workspace_client,
        ),
        (Some("GET"), Some("workspace.css")) => {
            respond(stream, "text/css", &service.assets.workspace_css)
        }
        (Some("GET"), Some("state")) => {
            dispatch(
                stream,
                service,
                json!({"kind":"state","context":session["context"],"command":session["command"],"query":query}),
            );
        }
        (Some("POST"), Some("command")) => {
            let length = request.content_length.unwrap_or(usize::MAX);
            if length > 4096 || request.content_type.as_deref() != Some("application/json") {
                write_empty_response(stream, BAD_REQUEST_RESPONSE);
                return;
            }
            let mut body = vec![0; length];
            if reader.read_exact(&mut body).is_err() {
                return;
            }
            let Ok(command) = serde_json::from_slice::<Value>(&body) else {
                write_empty_response(stream, BAD_REQUEST_RESPONSE);
                return;
            };
            dispatch(stream, service, json!({"kind":"command","params":command}));
        }
        _ => write_empty_response(stream, BAD_REQUEST_RESPONSE),
    }
}

fn dispatch(stream: &mut TcpStream, service: &WorkspaceService, request: Value) {
    let Some(handler) = service
        .handler
        .lock()
        .ok()
        .and_then(|handler| handler.clone())
    else {
        write_empty_response(stream, UNAVAILABLE_RESPONSE);
        return;
    };
    match handler(request) {
        Ok(value) => respond(stream, "application/json", &value.to_string()),
        Err(message) => {
            let body = json!({"message":message}).to_string();
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            write_preview_response(stream, response.as_bytes());
        }
    }
}

fn respond(stream: &mut TcpStream, content_type: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    write_preview_response(stream, response.as_bytes());
}
