//! Role: Implement application workspace ports with native I/O.
use crate::{NativeWorkspace, ServiceError};
use fleximark_application::operations::*;
use fleximark_engine::{DocumentEngine, RenderConfig, ResolvedAssets};
use fleximark_model::Document;
use fleximark_plugin_api::{CancellationToken, PluginExecutor};
use std::sync::Arc;

impl From<ServiceError> for OperationError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::NotInitialized => Self::NotInitialized,
            ServiceError::InvalidWorkspaceUri => Self::InvalidWorkspaceUri,
            other => Self::Failed(other.to_string()),
        }
    }
}
impl WorkspaceServices for NativeWorkspace {
    fn load_plugin_host(
        &self,
        uri: &str,
        trusted: bool,
        generation: u64,
    ) -> Result<(Arc<dyn PluginExecutor>, RenderConfig), OperationError> {
        let (host, config) = crate::load_plugin_host(uri, trusted, generation)?;
        Ok((Arc::new(host), config))
    }
    fn resolve_render_assets(
        &self,
        document_uri: &str,
        workspace_uri: &str,
        document: &Document,
    ) -> Result<ResolvedAssets, OperationError> {
        Ok(crate::resolve_render_assets(
            document_uri,
            workspace_uri,
            document,
        )?)
    }
    fn initialize_workspace(&self, uri: &str) -> Result<InitializedWorkspace, OperationError> {
        Ok(crate::initialize_workspace(uri)?)
    }
    fn edit_theme(&self, uri: &str) -> Result<ThemeResource, OperationError> {
        Ok(crate::edit_theme(uri)?)
    }
    fn get_note_options(&self, uri: &str) -> Result<NoteOptions, OperationError> {
        Ok(crate::get_note_options(uri)?)
    }
    fn create_note_with_options(
        &self,
        uri: &str,
        category: Option<&[String]>,
        template: Option<&str>,
        name: Option<&str>,
    ) -> Result<CreatedNote, OperationError> {
        Ok(crate::create_note_with_options(
            uri, category, template, name,
        )?)
    }
    fn collect_admonitions(
        &self,
        document: &Document,
        source: &str,
        workspace: &str,
    ) -> Result<CollectedAdmonitions, OperationError> {
        Ok(crate::collect_admonitions(document, source, workspace)?)
    }
    fn default_export_destination(&self, source_uri: &str) -> Result<String, OperationError> {
        Ok(crate::default_export_destination(source_uri)?)
    }
    fn export_document(
        &self,
        engine: &DocumentEngine,
        source_uri: &str,
        workspace_uri: &str,
        destination: &str,
        cancellation: &CancellationToken,
    ) -> Result<ExportReceipt, OperationError> {
        Ok(crate::export_document(
            engine,
            source_uri,
            workspace_uri,
            destination,
            cancellation,
        )?)
    }
    fn acknowledge_export(
        &self,
        source_uri: &str,
        workspace_uri: &str,
        destination: &str,
    ) -> Result<(), OperationError> {
        Ok(crate::acknowledge_export(
            source_uri,
            workspace_uri,
            destination,
        )?)
    }
    fn same_workspace(&self, left_uri: &str, right_uri: &str) -> bool {
        match (
            crate::workspace_path(left_uri),
            crate::workspace_path(right_uri),
        ) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        }
    }
    fn document_is_in_workspace(&self, document_uri: &str, workspace_uri: &str) -> bool {
        crate::document_is_in_workspace(document_uri, workspace_uri).unwrap_or(false)
    }
    fn migrate_workspace(&self, workspace_uri: &str) -> Result<MigrationOutput, OperationError> {
        let workspace = crate::workspace_path(workspace_uri)?;
        let binary = std::env::current_exe()
            .map_err(|e| OperationError::Failed(e.to_string()))?
            .with_file_name(if std::cfg!(windows) {
                "fleximark.exe"
            } else {
                "fleximark"
            });
        let output = std::process::Command::new(binary)
            .arg("migrate")
            .arg(workspace)
            .output()
            .map_err(|e| OperationError::Failed(e.to_string()))?;
        if !output.status.success() {
            return Err(OperationError::Failed(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        Ok(MigrationOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        })
    }
}
