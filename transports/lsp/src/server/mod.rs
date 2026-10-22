//! Role: Assemble standard LSP routing and external contract adapters.
mod commands;
mod diagnostics;
mod language;
mod lsp;
mod protocol;
mod routing;
mod rpc;
mod standard_commands;
mod state;
mod wire;
mod workspace_ui;
pub use state::Server;
pub(crate) use wire::response_value;
#[cfg(test)]
pub(crate) use wire::session_error;
