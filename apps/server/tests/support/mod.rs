//! Role: Build native composition fixtures and exchange standard LSP messages.
#![allow(dead_code)]
use fleximark_lsp_transport::{BrowserAssets, Server};
use fleximark_protocol::{IncomingMessage, RpcId};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn native_server() -> Server {
    Server::new(
        Arc::new(fleximark_workspace::NativeWorkspace),
        browser_assets(),
    )
}
pub fn browser_assets() -> BrowserAssets {
    BrowserAssets {
        preview_client: "const source = new EventSource('events');".into(),
        preview_css: ".markdown-body { color: black; }".into(),
        katex_css: ".katex { font: serif; }".into(),
        workspace_client: "const workspace = true;".into(),
        workspace_css: "body { color: black; }".into(),
        workspace_html: "<!doctype html><main id=\"workspace\"></main>".into(),
    }
}
pub fn message(id: Option<i64>, method: &str, params: Value) -> IncomingMessage {
    IncomingMessage {
        jsonrpc: "2.0".into(),
        id: id.map(|id| RpcId::try_from(id).unwrap()),
        method: method.into(),
        params,
    }
}
pub trait LspClient {
    fn request(&mut self, id: i64, method: &str, params: Value) -> Vec<Value>;
    fn notify(&mut self, method: &str, params: Value) -> Vec<Value>;
}
impl LspClient for Server {
    fn request(&mut self, id: i64, method: &str, params: Value) -> Vec<Value> {
        self.handle(message(Some(id), method, params))
    }
    fn notify(&mut self, method: &str, params: Value) -> Vec<Value> {
        self.handle(message(None, method, params))
    }
}
pub fn initialize(server: &mut Server, roots: Value) -> Value {
    let response = server.request(1, "initialize", json!({
        "processId":null,"capabilities":{"general":{"positionEncodings":["utf-8"]},"window":{"showDocument":{"support":true}}},
        "initializationOptions":{"fleximark":{"workspaces":roots,"capabilities":{"selectionEvents":true,"viewportEvents":true}}}
    }));
    assert!(response[0].get("result").is_some(), "{response:?}");
    assert!(server.notify("initialized", json!({})).is_empty());
    response[0]["result"].clone()
}
pub fn open(server: &mut Server, uri: &str, text: &str, version: i64) -> Vec<Value> {
    server.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"markdown","version":version,"text":text}}),
    )
}
pub fn session(server: &Server, uri: &str) -> String {
    server
        .sessions()
        .session_id_for_uri(uri)
        .expect("open document session")
        .to_owned()
}
pub fn daemon(server: &Server) -> String {
    server.sessions().daemon_instance_id().to_owned()
}
pub fn preview(server: &mut Server, uri: &str, version: i64, target: &str) -> Value {
    let response = server.request(3, fleximark_protocol::method::CREATE_PREVIEW, json!({"daemonInstanceId":daemon(server),"documentSessionId":session(server,uri),"expectedDocumentVersion":version,"target":target}));
    assert!(response[0].get("result").is_some(), "{response:?}");
    response[0]["result"].clone()
}
pub fn read_frame(server: &mut Server, preview: &str) -> Value {
    let response = server.request(
        4,
        fleximark_protocol::method::READ_PREVIEW,
        json!({"daemonInstanceId":daemon(server),"previewSessionId":preview}),
    );
    assert!(response[0].get("result").is_some(), "{response:?}");
    response[0]["result"]["frame"].clone()
}
pub fn http(url: &str, suffix: &str, method: &str, body: Option<&str>) -> String {
    let (address, path) = url
        .strip_prefix("http://")
        .expect("HTTP URL")
        .split_once('/')
        .unwrap();
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    let request = match body {
        Some(body) => format!(
            "{method} /{path}{suffix} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
        None => format!("{method} /{path}{suffix} HTTP/1.1\r\nHost: {address}\r\n\r\n"),
    };
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
pub struct TestDirectory(PathBuf);
impl Deref for TestDirectory {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn test_directory(label: &str) -> TestDirectory {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "fleximark-composition-{label}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    TestDirectory(path)
}
pub fn uri(path: &Path) -> String {
    fleximark_workspace::path_to_file_uri(path).unwrap()
}

pub fn installed_resources() -> TestDirectory {
    use sha2::{Digest, Sha256};
    let root = test_directory("runtime-resources");
    let resources = [
        (
            "resources/preview/browser-host.js",
            "const source=new EventSource('events');",
        ),
        ("resources/preview/fleximark.css", ".markdown-body{}"),
        ("resources/preview/katex.css", ".katex{}"),
        ("resources/workspace/workspace.js", "const workspace=true;"),
        ("resources/workspace/workspace.css", "body{}"),
    ];
    let mut files = serde_json::Map::new();
    for (name, content) in resources {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        files.insert(
            name.into(),
            json!(format!("{:x}", Sha256::digest(content.as_bytes()))),
        );
    }
    let metadata = include_str!("../../../../release.toml");
    let version = metadata
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .unwrap()
        .trim_matches('"');
    let target = match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        _ => panic!("unsupported runtime test platform"),
    };
    std::fs::write(root.join("manifest.json"),json!({"schemaVersion":1,"version":version,"target":target,"protocolVersion":fleximark_protocol::PROTOCOL_VERSION,"files":files}).to_string()).unwrap();
    root
}
