//! Role: Native workspace error operations and validation.

#[derive(Debug)]
pub enum ServiceError {
    Cancelled,
    InvalidWorkspaceUri,
    LinkedControlDirectory,
    NotInitialized,
    InvalidConfig,
    InvalidNoteName,
    NoteAlreadyExists,
    InvalidControlPath,
    UnmanagedExport,
    ExportOwnershipConflict,
    ExportContentConflict,
    ReservedExportPath,
    InvalidExportDestination,
    LinkedPath(String),
    ExportRecoveryConflict,
    ExportAwaitingAcknowledgement,
    Engine(fleximark_engine::EngineError),
    Json(serde_json::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "operation cancelled"),
            Self::InvalidWorkspaceUri => f.write_str("workspace URI must be a local file URI"),
            Self::LinkedControlDirectory => {
                f.write_str("refusing a workspace whose .fleximark path is a symbolic link")
            }
            Self::NotInitialized => {
                f.write_str("workspace is not initialized; run initializeWorkspace first")
            }
            Self::InvalidConfig => {
                f.write_str(".fleximark/config.toml is not a valid current configuration")
            }
            Self::InvalidNoteName => f.write_str("note file name is invalid"),
            Self::NoteAlreadyExists => f.write_str("a note with that name already exists"),
            Self::InvalidControlPath => {
                f.write_str("refusing a symbolic link or non-file at a FlexiMark-owned path")
            }
            Self::UnmanagedExport => {
                f.write_str("refusing an unmanaged non-empty export destination")
            }
            Self::ExportOwnershipConflict => {
                f.write_str("export ownership marker and registry do not match")
            }
            Self::ExportContentConflict => {
                f.write_str("a managed export file was changed outside FlexiMark")
            }
            Self::ReservedExportPath => f.write_str("reserved export path collision"),
            Self::InvalidExportDestination => {
                f.write_str("export destination must have an existing local parent directory")
            }
            Self::LinkedPath(value) => write!(f, "workspace path is invalid: {0}", value),
            Self::ExportRecoveryConflict => {
                f.write_str("export recovery journal is invalid or conflicts with filesystem state")
            }
            Self::ExportAwaitingAcknowledgement => {
                f.write_str("the previous export must be acknowledged after opening and validation")
            }
            Self::Engine(value) => std::fmt::Display::fmt(value, f),
            Self::Json(value) => std::fmt::Display::fmt(value, f),
            Self::Io(value) => std::fmt::Display::fmt(value, f),
        }
    }
}
impl std::error::Error for ServiceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Engine(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
impl From<fleximark_engine::EngineError> for ServiceError {
    fn from(value: fleximark_engine::EngineError) -> Self {
        Self::Engine(value)
    }
}
impl From<serde_json::Error> for ServiceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
impl From<std::io::Error> for ServiceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
