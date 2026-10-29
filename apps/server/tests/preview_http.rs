//! Role: Verify authenticated preview HTTP framing, security and navigation.
use fleximark_application::{PreviewSessionId, RenderFrame};
use fleximark_preview_http::{BrowserAssets, PreviewPage, StoredEvent, serve_preview_request};
use fleximark_protocol::method;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

fn preview_assets() -> BrowserAssets {
    BrowserAssets {
        preview_client: "const source=new EventSource('events');".into(),
        preview_css: include_str!("../../../web/preview/fleximark.css").into(),
        katex_css: include_str!("../../../web/preview/katex.css").into(),
        workspace_client: "const workspace=true;".into(),
        workspace_css: "body{}".into(),
        workspace_html: "<main id=\"workspace\"></main>".into(),
    }
}

fn preview_pages(token: &str) -> Arc<Mutex<HashMap<String, PreviewPage>>> {
    let snapshot = fleximark_engine::RenderSnapshot {
        document_version: 1,
        renderer_fingerprint: "0".repeat(64),
        style: None,
        assets: vec![],
        blocks: vec![fleximark_engine::RenderBlock {
            id: fleximark_model::NodeId("node-1".into()),
            html: "<p data-fleximark-node-id=\"node-1\">hello</p>".into(),
            node_ids: vec![fleximark_model::NodeId("node-1".into())],
        }],
        navigation: vec![fleximark_model::NavigationEntry {
            node_id: fleximark_model::NodeId("node-1".into()),
            source_range: fleximark_model::SourceRange {
                byte_start: 4,
                byte_end: 9,
            },
            depth: 1,
        }],
    };
    let mut frame = RenderFrame::from_snapshot(PreviewSessionId("preview-1".into()), 7, snapshot);
    frame.source = Arc::from("x\n\nxhello");
    let frame_event =
        serde_json::to_string(&fleximark_preview_http::protocol::frame(&frame).unwrap()).unwrap();
    Arc::new(Mutex::new(HashMap::from([(
        token.to_owned(),
        PreviewPage {
            shell: preview_assets().preview_shell(token),
            daemon_instance_id: "daemon-1".into(),
            preview_session_id: "preview-1".into(),
            frame,
            change_event: StoredEvent {
                encoded: frame_event,
                sequence: 1,
            },
            navigation_event: None,
            durable_navigation: None,
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
        serve_preview_request(stream, port, &pages, sender.as_ref(), &preview_assets());
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

#[test]
fn preview_http_shell_preserves_exact_security_headers_and_rejections() {
    let token = "fixture-token";
    let pages = preview_pages(token);
    let response = preview_http_request(&pages, None, |port| {
        format!("GET /preview/{token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").into_bytes()
    });
    let body = preview_assets().preview_shell(token);
    assert!(body.contains(".katex{font:"));
    assert!(body.contains("data:font/woff2;base64,"));
    assert!(body.contains("[data-admonition-kind=\"note\"]"));
    assert!(body.contains("[data-admonition-kind=\"important\"]"));
    assert!(body.contains("[data-admonition-kind=\"caution\"]"));
    let expected = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; base-uri 'none'; form-action 'none'; font-src data:; img-src 'self' https://i.ytimg.com data: blob:; media-src 'self' blob:; frame-src https://www.youtube-nocookie.com; object-src 'none'; style-src 'unsafe-inline'; script-src 'self'; connect-src 'self'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
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
fn native_frame_integer_overflow_returns_a_structured_http_error() {
    let token = "overflow-frame";
    let pages = preview_pages(token);
    pages
        .lock()
        .unwrap()
        .get_mut(token)
        .unwrap()
        .frame
        .render_revision = u64::MAX;
    let response = preview_http_request(&pages, None, |port| {
        format!("GET /preview/{token}/frame HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n")
            .into_bytes()
    });
    let response = String::from_utf8(response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 500 Internal Server Error"),
        "{response}"
    );
    let body: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("exceeds the JSON transport range")
    );
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
    assert!(
        !from_start.contains("navigation-2"),
        "navigation from an older frame must not follow a new frame"
    );
    assert!(from_start.contains("id: 3\ndata: change-3"));

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
fn sse_reconnect_does_not_repeat_unchanged_frame_or_retained_navigation() {
    let token = "reconnect";
    let pages = preview_pages(token);
    pages
        .lock()
        .unwrap()
        .get_mut(token)
        .unwrap()
        .durable_navigation = Some(StoredEvent {
        encoded: "retained-jump".into(),
        sequence: 2,
    });
    let request = |last: Option<u64>| {
        String::from_utf8(preview_http_request(&pages, None, |port| {
            let header = last
                .map(|last| format!("Last-Event-ID: {last}\r\n"))
                .unwrap_or_default();
            format!(
                "GET /preview/{token}/events HTTP/1.1\r\nHost: localhost:{port}\r\n{header}\r\n"
            )
            .into_bytes()
        }))
        .unwrap()
    };
    let initial = request(None);
    assert!(initial.contains("rendererFingerprint"));
    assert!(initial.contains("id: 2\ndata: retained-jump"));
    let interrupted = request(Some(1));
    assert!(!interrupted.contains("rendererFingerprint"));
    assert!(interrupted.contains("id: 2\ndata: retained-jump"));
    let reconnect = request(Some(2));
    assert!(!reconnect.contains("data:"));
    assert!(!reconnect.contains("retained-jump"));
}
