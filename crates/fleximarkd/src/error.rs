use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("workspace URI must be a local file URI")]
    InvalidWorkspaceUri,
    #[error("refusing a workspace whose .fleximark path is a symbolic link")]
    LinkedControlDirectory,
    #[error("workspace is not initialized; run initializeWorkspace first")]
    NotInitialized,
    #[error(".fleximark/config.toml is not a valid version 1 configuration")]
    InvalidConfig,
    #[error("note file name is invalid")]
    InvalidNoteName,
    #[error("a note with that name already exists")]
    NoteAlreadyExists,
    #[error("refusing a symbolic link or non-file at a FlexiMark-owned path")]
    InvalidControlPath,
    #[error("refusing an unmanaged non-empty export destination")]
    UnmanagedExport,
    #[error("export ownership marker and registry do not match")]
    ExportOwnershipConflict,
    #[error("a managed export file was changed outside FlexiMark")]
    ExportContentConflict,
    #[error("reserved export path collision")]
    ReservedExportPath,
    #[error("export destination must have an existing local parent directory")]
    InvalidExportDestination,
    #[error("plugin configuration is invalid: {0}")]
    PluginConfig(String),
    #[error("export recovery journal is invalid or conflicts with filesystem state")]
    ExportRecoveryConflict,
    #[error("the previous export must be acknowledged after opening and validation")]
    ExportAwaitingAcknowledgement,
    #[error(transparent)]
    Engine(#[from] fleximark_engine::EngineError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
