mod cancellation;
mod preview_http;
mod server;
mod telemetry;
mod transport;

use std::env;
use std::fs;
use std::thread;

use fleximark_lsp::{DidOpenParams, SessionRegistry, TextDocumentItem};
use fleximark_model::PositionEncoding;
use fleximark_plugin_host::CancellationToken;

use preview_http::{PreviewServer, random_token};
use telemetry::log_operational_event;
use transport::run;

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
    registry.open_with_cancellation(
        DidOpenParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                version: 1,
                text: source,
            },
        },
        &CancellationToken::default(),
    )?;
    let session_id = registry
        .session_id_for_uri(&uri)
        .expect("opened document")
        .to_owned();
    let daemon_id = registry.daemon_instance_id().to_owned();
    let frame = registry.render_with_cancellation(
        &daemon_id,
        &session_id,
        1,
        "serve-preview",
        &CancellationToken::default(),
    )?;
    let previews = PreviewServer::start(None)?;
    println!(
        "{}",
        previews.publish(&random_token()?, &daemon_id, "serve-preview", &frame)
    );
    loop {
        thread::park();
    }
}

#[cfg(test)]
mod tests;
