//! Role: Connect official Zed Markdown buffers to the shared native runtime.

use crate::release;
use zed_extension_api::{self as zed, settings::LspSettings};

pub(crate) struct Fleximark;

impl zed::Extension for Fleximark {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings = LspSettings::for_worktree(id.as_ref(), worktree)?;
        let configured = settings
            .binary
            .as_ref()
            .and_then(|binary| binary.path.clone());
        let command = match configured.or_else(|| worktree.which("fleximarkd")) {
            Some(path) => path,
            None => release::install(id)?,
        };
        release::check_binary(&command)?;
        Ok(zed::Command {
            command,
            args: vec!["lsp".into()],
            env: worktree.shell_env(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<Option<zed::serde_json::Value>> {
        Ok(LspSettings::for_worktree(id.as_ref(), worktree)?.initialization_options)
    }
}
