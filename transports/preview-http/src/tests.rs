//! Role: Exercise shared frame authority and persistent browser delivery.
use super::*;
use fleximark_application::{CancellationToken, DocumentStore, DocumentUpdate};
use fleximark_model::PositionEncoding;
use std::io::BufRead;

const URI: &str = "untitled:stream-test";
fn assets() -> BrowserAssets {
    BrowserAssets {
        preview_client: String::new(),
        preview_css: String::new(),
        katex_css: String::new(),
        workspace_client: String::new(),
        workspace_css: String::new(),
        workspace_html: String::new(),
    }
}
fn fixture() -> (DocumentStore, PreviewHandle) {
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .open(
            DocumentUpdate {
                uri: URI.into(),
                version: 1,
                source: "# First".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let frame = render(&mut store, 1);
    (store, frame)
}
fn render(store: &mut DocumentStore, version: u64) -> PreviewHandle {
    let attachment = store.attach(URI, version).unwrap();
    store
        .render_with_cancellation(
            &attachment.document_session_id,
            version,
            "preview-1",
            &CancellationToken::default(),
        )
        .unwrap()
}
fn update(store: &mut DocumentStore, version: u64) -> PreviewHandle {
    store
        .change(
            DocumentUpdate {
                uri: URI.into(),
                version,
                source: format!("# Version {version}"),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    render(store, version)
}
fn server(frame: &PreviewHandle) -> PreviewServer {
    let server = PreviewServer::with_assets(None, assets()).unwrap();
    server
        .publish("token", "daemon", "preview-1", frame)
        .unwrap();
    server
}
fn connect(server: &PreviewServer, path: &str, last: Option<u64>) -> BufReader<TcpStream> {
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let last = last.map_or(String::new(), |last| format!("Last-Event-ID: {last}\r\n"));
    write!(
        stream,
        "GET /preview/token/{path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{last}\r\n",
        server.port
    )
    .unwrap();
    BufReader::new(stream)
}
fn headers(reader: &mut BufReader<TcpStream>) -> String {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        headers.push_str(&line);
        if line == "\r\n" {
            return headers;
        }
    }
}
fn event(reader: &mut BufReader<TcpStream>) -> (u64, Value) {
    let mut id = None;
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        if let Some(value) = line.strip_prefix("id: ") {
            id = Some(value.trim().parse().unwrap());
        }
        if let Some(value) = line.strip_prefix("data: ") {
            return (id.unwrap(), serde_json::from_str(value.trim()).unwrap());
        }
    }
}

#[test]
fn shared_handles_stream_multiple_updates_and_reconnect_latest() {
    let (mut store, first) = fixture();
    let server = server(&first);
    assert!(Arc::ptr_eq(
        first.frame_arc(),
        server.pages.lock().unwrap()["token"].frame.frame_arc()
    ));
    let mut stream = connect(&server, "events", None);
    let header = headers(&mut stream);
    assert!(header.starts_with("HTTP/1.1 200"));
    assert!(!header.contains("Content-Length"));
    assert!(header.contains("Connection: keep-alive"));
    assert_eq!(event(&mut stream).1["documentVersion"], 1);
    for version in 2..=3 {
        let frame = update(&mut store, version);
        server.update("token", &frame).unwrap();
        assert!(Arc::ptr_eq(
            frame.frame_arc(),
            server.pages.lock().unwrap()["token"].frame.frame_arc()
        ));
        assert_eq!(event(&mut stream).1["documentVersion"], version);
    }
    let mut reconnected = connect(&server, "events", Some(u64::MAX));
    headers(&mut reconnected);
    assert_eq!(event(&mut reconnected).1["documentVersion"], 3);
}

#[test]
fn subscriptions_have_separate_capacity_and_shutdown_closes_all() {
    let (_store, frame) = fixture();
    let server = server(&frame);
    let mut streams = Vec::new();
    for _ in 0..16 {
        let mut stream = connect(&server, "events", None);
        assert!(headers(&mut stream).starts_with("HTTP/1.1 200"));
        event(&mut stream);
        streams.push(stream);
    }
    let mut excess = connect(&server, "events", None);
    assert!(headers(&mut excess).starts_with("HTTP/1.1 503"));
    let mut normal = connect(&server, "frame", None);
    assert!(headers(&mut normal).starts_with("HTTP/1.1 200"));
    drop(server);
    for mut stream in streams {
        let mut remaining = String::new();
        stream.read_to_string(&mut remaining).unwrap();
        assert!(remaining.trim().is_empty());
    }
}

#[test]
fn navigation_rejects_invalidated_and_same_version_recovered_handles() {
    let (mut store, frame) = fixture();
    let server = server(&frame);
    let node = frame.navigation.first().unwrap().node_id.0.clone();
    let request = serde_json::json!({"type":"selectNode","previewSessionId":"preview-1",
        "renderRevision":frame.render_revision,"nodeId":node})
    .to_string();
    // A rejected stale change invalidates authority, even after recovery at the same version.
    assert!(
        store
            .change(
                DocumentUpdate {
                    uri: URI.into(),
                    version: 0,
                    source: "old".into()
                },
                &CancellationToken::default()
            )
            .is_err()
    );
    assert!(!frame.is_current());
    store
        .change(
            DocumentUpdate {
                uri: URI.into(),
                version: 1,
                source: "# Recovered".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(!frame.is_current());
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(stream, "POST /preview/token/navigation HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nOrigin: http://127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{request}", server.port,server.port,request.len()).unwrap();
    assert!(headers(&mut BufReader::new(stream)).starts_with("HTTP/1.1 409"));
}

#[test]
fn frame_precedes_navigation_and_updates_clear_stale_navigation() {
    let (mut store, frame) = fixture();
    let server = server(&frame);
    let navigation = RenderNavigationEvent::Selection {
        preview_session_id: "preview-1".into(),
        render_revision: fleximark_protocol::JsSafeU64::new(frame.render_revision).unwrap(),
        node_ids: Vec::new(),
        active_position: None,
    };
    server.navigate("token", &navigation);
    let mut stream = connect(&server, "events", None);
    headers(&mut stream);
    let (frame_sequence, frame_event) = event(&mut stream);
    let (nav_sequence, nav_event) = event(&mut stream);
    assert!(frame_sequence < nav_sequence);
    assert_eq!(frame_event["documentVersion"], 1);
    assert_eq!(nav_event["type"], "selection");
    let newer = update(&mut store, 2);
    server.update("token", &newer).unwrap();
    let (next_sequence, next) = event(&mut stream);
    assert!(next_sequence > nav_sequence);
    assert_eq!(next["documentVersion"], 2);
    assert!(
        server.pages.lock().unwrap()["token"]
            .navigation_event
            .is_none()
    );
}

#[test]
fn partial_request_has_absolute_socket_deadline() {
    let (_store, frame) = fixture();
    let server = server(&frame);
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    stream
        .write_all(b"GET /preview/token/events HTTP/1.1\r\n")
        .unwrap();
    // Frequent bytes cannot extend the two-second request budget.
    let mut drip = stream.try_clone().unwrap();
    let writer = thread::spawn(move || {
        for _ in 0..12 {
            if drip.write_all(b"X").is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
    let started = Instant::now();
    let mut response = [0; 1];
    let closed = stream.read(&mut response);
    assert!(matches!(closed, Ok(0)) || closed.is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
    writer.join().unwrap();
}
