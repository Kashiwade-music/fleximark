//! Role: Compose the FlexiMark language-server executable.
fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "lsp".into());
    if matches!(mode.as_str(), "--version" | "-V") {
        println!("fleximarkd {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if mode != "lsp" {
        eprintln!("usage: fleximarkd [lsp|--version]");
        std::process::exit(2);
    }
    if let Err(error) = run() {
        eprintln!("fleximarkd: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let resources = fleximark_preview_assets::PreviewAssets::installed()?;
    let assets = fleximark_lsp_transport::BrowserAssets {
        preview_client: resources.read("browser-host.js")?,
        preview_css: resources.read("fleximark.css")?,
        katex_css: resources.read("katex.css")?,
        workspace_client: resources.read_path("resources/workspace/workspace.js")?,
        workspace_css: resources.read_path("resources/workspace/workspace.css")?,
        workspace_html: include_str!("../../../web/workspace-ui/index.html").to_owned(),
    };
    fleximark_lsp_transport::run(assets, std::sync::Arc::new(resources))
}
