//! Role: Supply a filesystem-free workspace port for transport unit tests.
use fleximark_application::operations::*;
use fleximark_engine::{DocumentEngine, RenderConfig, ResolvedAssets};
use fleximark_model::Document;
use fleximark_plugin_api::{CancellationToken, PluginExecutor};
use std::sync::Arc;

pub(crate) struct NoWorkspace;

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
fn unavailable<T>() -> Result<T, OperationError> {
    Err(OperationError::NotInitialized)
}
impl WorkspaceServices for NoWorkspace {
    fn load_plugin_host(
        &self,
        _: &str,
        _: bool,
        _: u64,
    ) -> Result<(Arc<dyn PluginExecutor>, RenderConfig), OperationError> {
        unavailable()
    }
    fn resolve_render_assets(
        &self,
        _: &str,
        _: &str,
        _: &Document,
    ) -> Result<ResolvedAssets, OperationError> {
        unavailable()
    }
    fn initialize_workspace(&self, _: &str) -> Result<InitializedWorkspace, OperationError> {
        unavailable()
    }
    fn edit_theme(&self, _: &str) -> Result<ThemeResource, OperationError> {
        unavailable()
    }
    fn get_note_options(&self, _: &str) -> Result<NoteOptions, OperationError> {
        unavailable()
    }
    fn create_note_with_options(
        &self,
        _: &str,
        _: Option<&[String]>,
        _: Option<&str>,
        _: Option<&str>,
    ) -> Result<CreatedNote, OperationError> {
        unavailable()
    }
    fn collect_admonitions(
        &self,
        _: &Document,
        _: &str,
        _: &str,
    ) -> Result<CollectedAdmonitions, OperationError> {
        unavailable()
    }
    fn default_export_destination(&self, _: &str) -> Result<String, OperationError> {
        unavailable()
    }
    fn export_document(
        &self,
        _: &DocumentEngine,
        _: &str,
        _: &str,
        _: &str,
        _: &CancellationToken,
    ) -> Result<ExportReceipt, OperationError> {
        unavailable()
    }
    fn acknowledge_export(&self, _: &str, _: &str, _: &str) -> Result<(), OperationError> {
        unavailable()
    }
    fn same_workspace(&self, left: &str, right: &str) -> bool {
        left == right
    }
    fn document_is_in_workspace(&self, document: &str, workspace: &str) -> bool {
        document
            .strip_prefix(workspace)
            .is_some_and(|tail| tail.starts_with('/'))
    }
    fn migrate_workspace(&self, _: &str) -> Result<MigrationOutput, OperationError> {
        unavailable()
    }
}
