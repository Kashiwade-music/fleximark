//! Role: Verify wire errors, cancellation intake and redacted operational traces.
use crate::cancellation::CancellationCoordinator;
use crate::server::session_error;
use crate::telemetry::OperationalTrace;
use fleximark_application::SessionError;
use fleximark_protocol::{CONTENT_MODIFIED, IncomingMessage, RpcId, method};
use serde_json::{Value, json};
use std::time::Duration;

fn message(id: Option<i64>, method: &str, params: Value) -> IncomingMessage {
    IncomingMessage {
        jsonrpc: "2.0".into(),
        id: id.map(|id| RpcId::try_from(id).unwrap()),
        method: method.into(),
        params,
    }
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
            SessionError::InvalidRange,
            -32602,
            "document position or full-text change is invalid",
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
