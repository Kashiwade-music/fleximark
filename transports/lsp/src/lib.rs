//! Role: Route standard LSP and current FlexiMark RPC over stdio.
mod cancellation;
mod server;
mod telemetry;
mod transport;
use fleximark_preview_http as preview_http;
pub use fleximark_preview_http::BrowserAssets;
pub use server::Server;
pub use transport::run;

#[cfg(test)]
mod test_services;
#[cfg(test)]
mod tests;
