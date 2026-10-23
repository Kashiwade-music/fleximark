//! Role: Accept versioned browser resources supplied by the executable composition root.
#[derive(Clone)]
pub struct BrowserAssets {
    pub preview_client: String,
    pub preview_css: String,
    pub katex_css: String,
    pub workspace_client: String,
    pub workspace_css: String,
    pub workspace_html: String,
}

impl BrowserAssets {
    pub fn preview_shell(&self, token: &str) -> String {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><style>{}\n{}</style></head><body><main id=\"preview\" class=\"markdown-body\"></main><script data-fleximark-live src=\"/preview/{token}/client.js\"></script></body></html>",
            self.katex_css, self.preview_css
        )
    }
}
