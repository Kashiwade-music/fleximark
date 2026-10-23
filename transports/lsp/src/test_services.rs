//! Role: Supply minimal browser assets for transport unit tests.
pub(crate) fn assets() -> fleximark_preview_http::BrowserAssets {
    fleximark_preview_http::BrowserAssets {
        preview_client: String::new(),
        preview_css: String::new(),
        katex_css: String::new(),
        workspace_client: String::new(),
        workspace_css: String::new(),
        workspace_html: "<!doctype html><main id=\"workspace\"></main>".into(),
    }
}
