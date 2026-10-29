//! Role: Assemble standard LSP routing and external contract adapters.
mod commands;
mod diagnostics;
mod document_sync;
mod fleximark_methods;
mod language;
mod lsp;
mod lsp_types;
mod positions;
mod publication;
mod routing;
mod standard_commands;
mod state;
mod wire;
mod workspace_folders;
mod workspace_ui;
pub use state::Server;
pub(crate) use wire::response_value;
#[cfg(test)]
pub(crate) use wire::session_error;

pub(crate) use commands::{CommandCompletion, CommandProgress, PreparedCommand};
pub(crate) use wire::session_error as operation_error;
