//! Role: Define native workspace operation outcomes and errors.

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
