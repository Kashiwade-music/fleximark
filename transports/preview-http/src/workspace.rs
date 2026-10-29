//! Role: Serve authenticated workspace forms through typed operation requests.
use crate::assets::BrowserAssets;
use crate::http::*;
use crate::state::{Handler, PreviewServer, WorkspaceRequest, WorkspaceResponse, WorkspaceService};
use fleximark_application::logging::log_operational_failure;
use fleximark_application::{ApplicationError, ErrorKind};
use fleximark_protocol::{WorkspaceStateRequest, WorkspaceUiCommandRequest, WorkspaceUiError};
use serde_json::Value;
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
        if let Ok(mut stored) = self.workspace.handler.lock() {
            *stored = Some(handler);
        }
    }

    pub fn publish_workspace(&self, token: &str, request: WorkspaceStateRequest) -> String {
        if let Ok(mut sessions) = self.workspace.sessions.lock() {
            sessions.insert(token.to_owned(), request);
        }
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
        (Some("GET"), Some("state")) => match parse_state_query(session, query) {
            Ok(request) => dispatch(stream, service, WorkspaceRequest::State(request)),
            Err(error) => respond_error(stream, &error),
        },
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
            let Ok(command) = serde_json::from_slice::<WorkspaceUiCommandRequest>(&body) else {
                respond_error(
                    stream,
                    &ApplicationError::InvalidState("invalid workspace command request".into()),
                );
                return;
            };
            dispatch(stream, service, WorkspaceRequest::Command(command));
        }
        _ => write_empty_response(stream, BAD_REQUEST_RESPONSE),
    }
}

fn dispatch(stream: &mut TcpStream, service: &WorkspaceService, request: WorkspaceRequest) {
    let Some(handler) = service
        .handler
        .lock()
        .ok()
        .and_then(|handler| handler.clone())
    else {
        write_empty_response(stream, UNAVAILABLE_RESPONSE);
        return;
    };
    let result = handler(request).and_then(|response| {
        match response {
            WorkspaceResponse::State(state) => serde_json::to_string(&state),
            WorkspaceResponse::Command(result) => serde_json::to_string(&result),
        }
        .map_err(ApplicationError::from)
    });
    match result {
        Ok(body) => respond(stream, "application/json", &body),
        Err(error) => respond_error(stream, &error),
    }
}

fn error_status(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::NotFound => "404 Not Found",
        ErrorKind::Denied => "403 Forbidden",
        ErrorKind::Conflict => "409 Conflict",
        ErrorKind::Invalid => "400 Bad Request",
        ErrorKind::Cancelled => "408 Request Timeout",
        ErrorKind::Internal => "500 Internal Server Error",
    }
}

fn respond_error(stream: &mut TcpStream, error: &ApplicationError) {
    log_operational_failure("workspace-http-failed", error, None, None, None);
    let message = match error.kind() {
        ErrorKind::Internal => "workspace operation failed".to_owned(),
        _ => error.to_string(),
    };
    let Ok(body) = serde_json::to_string(&WorkspaceUiError { error: message }) else {
        write_empty_response(stream, UNAVAILABLE_RESPONSE);
        return;
    };
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        error_status(error.kind()),
        body.len()
    );
    write_preview_response(stream, response.as_bytes());
}

fn invalid_query() -> ApplicationError {
    ApplicationError::InvalidState("invalid workspace state query".into())
}

fn decode_query(value: &str) -> Result<String, ApplicationError> {
    let mut bytes = Vec::new();
    let mut input = value.bytes();
    while let Some(byte) = input.next() {
        bytes.push(match byte {
            b'%' => {
                let a = (input.next().ok_or_else(invalid_query)? as char)
                    .to_digit(16)
                    .ok_or_else(invalid_query)?;
                let b = (input.next().ok_or_else(invalid_query)? as char)
                    .to_digit(16)
                    .ok_or_else(invalid_query)?;
                (a * 16 + b) as u8
            }
            b'+' => b' ',
            byte => byte,
        });
    }
    String::from_utf8(bytes).map_err(|_| invalid_query())
}

fn parse_state_query(
    mut request: WorkspaceStateRequest,
    query: &str,
) -> Result<WorkspaceStateRequest, ApplicationError> {
    let mut context = serde_json::to_value(&request.context)?;
    let mut seen = std::collections::HashSet::new();
    if query.is_empty() {
        return Ok(request);
    }
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(invalid_query)?;
        let key = decode_query(key)?;
        let value = decode_query(value)?;
        if !seen.insert(key.clone()) || value.trim().is_empty() {
            return Err(invalid_query());
        }
        match key.as_str() {
            "workspaceUri" | "documentUri" => context[&key] = Value::String(value),
            "documentVersion" => {
                context[&key] = serde_json::from_str(&value).map_err(|_| invalid_query())?;
            }
            "command" => {
                request.command = Some(
                    serde_json::from_value(Value::String(value)).map_err(|_| invalid_query())?,
                );
            }
            _ => return Err(invalid_query()),
        }
    }
    request.context = serde_json::from_value(context).map_err(|_| invalid_query())?;
    Ok(request)
}

fn respond(stream: &mut TcpStream, content_type: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    write_preview_response(stream, response.as_bytes());
}

#[cfg(test)]
mod tests {
    //! Role: Verify strict browser workspace inputs and failure classification.
    use super::*;
    use serde_json::json;

    fn state() -> WorkspaceStateRequest {
        serde_json::from_value(json!({"context":{},"command":null})).unwrap()
    }

    #[test]
    fn state_query_decodes_context_and_typed_command() {
        let request = parse_state_query(state(),
            "workspaceUri=file%3A%2F%2F%2Froot&documentUri=untitled%3Adoc&documentVersion=-2&command=createNote"
        ).unwrap();
        assert_eq!(
            request.context.workspace_uri.as_deref(),
            Some("file:///root")
        );
        assert_eq!(
            request.context.document_uri.as_deref(),
            Some("untitled:doc")
        );
        assert_eq!(request.context.document_version.unwrap().get(), -2);
        assert_eq!(
            request.command,
            Some(fleximark_protocol::CommandKind::CreateNote)
        );
    }

    #[test]
    fn state_query_rejects_unknown_duplicate_and_invalid_fields() {
        for query in [
            "unknown=value",
            "command=",
            "command=unknown",
            "command=null",
            "command=createNote&command=editTheme",
            "workspaceUri=",
            "workspaceUri=%20",
            "workspaceUri=%FF",
            "workspaceUri=%Z0",
            "workspaceUri=%",
            "workspaceUri",
            "documentUri=",
            "documentVersion=null",
            "documentVersion=1.5",
            "documentVersion=9007199254740992",
            "documentVersion=",
        ] {
            assert!(parse_state_query(state(), query).is_err(), "{query}");
        }
    }

    #[test]
    fn command_input_rejects_legacy_and_malformed_envelopes() {
        for input in [
            json!({"command":"createNote","context":{}}),
            json!({"context":{},"operation":{"kind":"unknown"}}),
            json!({"context":{"documentUri":null},"operation":{"command":"createNote"}}),
            json!({"context":{},"operation":null}),
        ] {
            assert!(serde_json::from_value::<WorkspaceUiCommandRequest>(input).is_err());
        }
    }

    #[test]
    fn internal_failure_response_hides_native_source() {
        use std::io::Read;
        use std::net::TcpListener;
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        respond_error(
            &mut server,
            &ApplicationError::internal(std::io::Error::other("private source token=secret")),
        );
        drop(server);
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 500 Internal Server Error"));
        let (_, body) = response.split_once("\r\n\r\n").unwrap();
        let error: WorkspaceUiError = serde_json::from_str(body).unwrap();
        assert_eq!(error.error, "workspace operation failed");
        assert!(!response.contains("private"));
        assert!(!response.contains("secret"));
    }

    #[test]
    fn application_categories_have_distinct_http_statuses() {
        for (kind, status) in [
            (ErrorKind::NotFound, "404 Not Found"),
            (ErrorKind::Denied, "403 Forbidden"),
            (ErrorKind::Conflict, "409 Conflict"),
            (ErrorKind::Invalid, "400 Bad Request"),
            (ErrorKind::Cancelled, "408 Request Timeout"),
            (ErrorKind::Internal, "500 Internal Server Error"),
        ] {
            assert_eq!(error_status(kind), status);
        }
    }
}
