//! Role: Typed workspace operation outcomes and filesystem operation port.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitializedWorkspace {
    pub config_uri: String,
    pub theme_uri: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatedNote {
    pub uri: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectedAdmonitions {
    pub uri: String,
    pub count: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeResource {
    pub uri: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportReceipt {
    pub output_uri: String,
    pub generation: u64,
    pub unsafe_output: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteCategory {
    pub name: String,
    pub children: Vec<NoteCategory>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteOptions {
    pub categories: Vec<NoteCategory>,
    pub templates: Vec<String>,
}

#[derive(Debug)]
pub enum OperationError {
    NotInitialized,
    InvalidWorkspaceUri,
    Failed(String),
}
impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInitialized => f.write_str("workspace is not initialized"),
            Self::InvalidWorkspaceUri => f.write_str("workspace URI is invalid"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for OperationError {}
pub struct MigrationOutput {
    pub stdout: String,
}

pub trait WorkspaceServices: Send + Sync {
    fn load_plugin_host(
        &self,
        uri: &str,
        trusted: bool,
        generation: u64,
    ) -> Result<
        (
            std::sync::Arc<dyn fleximark_plugin_api::PluginExecutor>,
            fleximark_engine::RenderConfig,
        ),
        OperationError,
    >;
    fn resolve_render_assets(
        &self,
        document_uri: &str,
        workspace_uri: &str,
        document: &fleximark_model::Document,
    ) -> Result<fleximark_engine::ResolvedAssets, OperationError>;
    fn initialize_workspace(&self, uri: &str) -> Result<InitializedWorkspace, OperationError>;
    fn edit_theme(&self, uri: &str) -> Result<ThemeResource, OperationError>;
    fn get_note_options(&self, uri: &str) -> Result<NoteOptions, OperationError>;
    fn create_note_with_options(
        &self,
        uri: &str,
        category: Option<&[String]>,
        template: Option<&str>,
        file_name: Option<&str>,
    ) -> Result<CreatedNote, OperationError>;
    fn collect_admonitions(
        &self,
        document: &fleximark_model::Document,
        source: &str,
        workspace: &str,
    ) -> Result<CollectedAdmonitions, OperationError>;
    fn default_export_destination(&self, source_uri: &str) -> Result<String, OperationError>;
    fn export_document(
        &self,
        engine: &fleximark_engine::DocumentEngine,
        source_uri: &str,
        workspace_uri: &str,
        destination: &str,
        cancellation: &fleximark_plugin_api::CancellationToken,
    ) -> Result<ExportReceipt, OperationError>;
    fn acknowledge_export(
        &self,
        source_uri: &str,
        workspace_uri: &str,
        destination: &str,
    ) -> Result<(), OperationError>;
    fn same_workspace(&self, left_uri: &str, right_uri: &str) -> bool;
    fn document_is_in_workspace(&self, document_uri: &str, workspace_uri: &str) -> bool;
    fn migrate_workspace(&self, workspace_uri: &str) -> Result<MigrationOutput, OperationError>;
}
