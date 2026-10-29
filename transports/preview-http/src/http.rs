//! Role: Parse bounded HTTP requests and enforce loopback origin authority.
use std::io::{BufRead, Write};
use std::net::TcpStream;

pub(crate) fn write_conversion_error(
    stream: &mut TcpStream,
    error: &fleximark_application::ApplicationError,
) {
    fleximark_application::logging::log_operational_failure(
        "preview-http-conversion-failed",
        error,
        None,
        None,
        None,
    );
    let message = if error.kind() == fleximark_application::ErrorKind::Internal {
        "preview conversion failed".to_owned()
    } else {
        error.to_string()
    };
    let body = serde_json::json!({"message":message}).to_string();
    let response = format!(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    write_preview_response(stream, response.as_bytes());
}
pub(crate) struct PreviewRequest {
    pub(crate) method: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) origin: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) last_event_id: Option<u64>,
    pub(crate) content_length: Option<usize>,
    pub(crate) content_type: Option<String>,
}

pub(crate) fn parse_preview_request(reader: &mut impl BufRead) -> Option<PreviewRequest> {
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
        .and_then(|path| {
            path.strip_prefix("/preview/")
                .map(str::to_owned)
                .or_else(|| {
                    path.strip_prefix("/workspace/")
                        .map(|path| format!("workspace/{path}"))
                })
        });
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

pub(crate) fn authorized_preview_path(request: &PreviewRequest, port: u16) -> Option<&str> {
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

pub(crate) fn write_preview_response(stream: &mut TcpStream, response: &[u8]) {
    let _ = stream.write_all(response);
}

pub(crate) const FORBIDDEN_RESPONSE: &[u8] =
    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
pub(crate) const BAD_REQUEST_RESPONSE: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
pub(crate) const CONFLICT_RESPONSE: &[u8] =
    b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
pub(crate) const TOO_MANY_REQUESTS_RESPONSE: &[u8] =
    b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
pub(crate) const UNAVAILABLE_RESPONSE: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
pub(crate) const NO_CONTENT_RESPONSE: &[u8] = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";

pub(crate) fn write_empty_response(stream: &mut TcpStream, response: &'static [u8]) {
    write_preview_response(stream, response);
}
