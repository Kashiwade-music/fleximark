//! Role: Classify application failures while preserving their native causes.
use fleximark_engine::EngineError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    NotFound,
    Denied,
    Conflict,
    Invalid,
    Cancelled,
    Internal,
}

#[derive(Debug)]
pub enum ApplicationError {
    Cancelled,
    RevisionExhausted,
    NotOpen,
    UnknownSession,
    WrongDaemon,
    ContentModified,
    StaleVersion,
    VersionMismatch,
    InvalidRange,
    EmptyChange,
    InvalidState(String),
    InternalState(String),
    InternalSource(Box<dyn std::error::Error + Send + Sync>),
    WriteDenied,
    IntegerRange(u64),
    InvalidSourceRange,
    InvalidWorkspaceUri,
    LinkedControlDirectory,
    NotInitialized,
    InvalidConfig,
    Config(fleximark_config::ConfigError),
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
impl ApplicationError {
    pub fn internal(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::InternalSource(Box::new(error))
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Cancelled => ErrorKind::Cancelled,
            Self::WriteDenied => ErrorKind::Denied,
            Self::NotOpen | Self::UnknownSession | Self::NotInitialized => ErrorKind::NotFound,
            Self::ContentModified
            | Self::StaleVersion
            | Self::VersionMismatch
            | Self::NoteAlreadyExists
            | Self::UnmanagedExport
            | Self::ExportOwnershipConflict
            | Self::ExportContentConflict
            | Self::ExportRecoveryConflict
            | Self::ExportAwaitingAcknowledgement => ErrorKind::Conflict,
            Self::RevisionExhausted
            | Self::InternalState(_)
            | Self::InternalSource(_)
            | Self::Json(_) => ErrorKind::Internal,
            Self::Io(error) => match error.kind() {
                std::io::ErrorKind::NotFound => ErrorKind::NotFound,
                std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
                std::io::ErrorKind::AlreadyExists => ErrorKind::Conflict,
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                    ErrorKind::Invalid
                }
                _ => ErrorKind::Internal,
            },
            Self::Engine(error) => match error {
                EngineError::ContentModified | EngineError::StaleVersion { .. } => {
                    ErrorKind::Conflict
                }
                EngineError::Configuration(_) | EngineError::Asset(_) | EngineError::Parse(_) => {
                    ErrorKind::Invalid
                }
                EngineError::Render(_) => ErrorKind::Internal,
            },
            _ => ErrorKind::Invalid,
        }
    }
}
impl std::fmt::Display for ApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "operation cancelled"),
            Self::RevisionExhausted => write!(f, "preview revision counter exhausted"),
            Self::NotOpen => write!(f, "document is not open"),
            Self::UnknownSession => write!(f, "document session is unknown or no longer active"),
            Self::WrongDaemon => write!(f, "daemon instance does not match this connection"),
            Self::ContentModified => write!(f, "document is out of sync"),
            Self::StaleVersion => write!(f, "document version is stale"),
            Self::VersionMismatch => write!(f, "document version does not match"),
            Self::InvalidRange => write!(f, "document position or full-text change is invalid"),
            Self::EmptyChange => write!(f, "a change notification must contain edits"),
            Self::InternalState(value) | Self::InvalidState(value) => f.write_str(value),
            Self::WriteDenied => {
                f.write_str("workspace write denied: the active workspace is not trusted")
            }
            Self::IntegerRange(value) => {
                write!(f, "integer {value} exceeds the JSON transport range")
            }
            Self::InvalidSourceRange => {
                f.write_str("source range is invalid for the original document")
            }
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
            Self::InternalSource(value) => std::fmt::Display::fmt(value, f),
            Self::Config(value) => std::fmt::Display::fmt(value, f),
            Self::Engine(value) => std::fmt::Display::fmt(value, f),
            Self::Json(value) => std::fmt::Display::fmt(value, f),
            Self::Io(value) => std::fmt::Display::fmt(value, f),
        }
    }
}
impl std::error::Error for ApplicationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InternalSource(error) => Some(error.as_ref()),
            Self::Config(error) => Some(error),
            Self::Engine(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
impl From<fleximark_engine::EngineError> for ApplicationError {
    fn from(value: fleximark_engine::EngineError) -> Self {
        Self::Engine(value)
    }
}
impl From<fleximark_config::ConfigError> for ApplicationError {
    fn from(value: fleximark_config::ConfigError) -> Self {
        Self::Config(value)
    }
}
impl From<serde_json::Error> for ApplicationError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
impl From<std::io::Error> for ApplicationError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub(super) fn engine_error(error: EngineError) -> ApplicationError {
    ApplicationError::Engine(error)
}

#[cfg(test)]
mod tests {
    //! Role: Check error classification and retained native failure sources.
    use super::*;
    use std::error::Error;
    #[test]
    fn failure_categories_preserve_their_semantic_meaning() {
        for (error, kind) in [
            (ApplicationError::NotInitialized, ErrorKind::NotFound),
            (ApplicationError::UnknownSession, ErrorKind::NotFound),
            (ApplicationError::WriteDenied, ErrorKind::Denied),
            (
                ApplicationError::ExportOwnershipConflict,
                ErrorKind::Conflict,
            ),
            (ApplicationError::VersionMismatch, ErrorKind::Conflict),
            (ApplicationError::WrongDaemon, ErrorKind::Invalid),
            (ApplicationError::InvalidConfig, ErrorKind::Invalid),
            (ApplicationError::IntegerRange(u64::MAX), ErrorKind::Invalid),
            (ApplicationError::Cancelled, ErrorKind::Cancelled),
            (ApplicationError::RevisionExhausted, ErrorKind::Internal),
        ] {
            assert_eq!(error.kind(), kind);
        }
    }

    #[test]
    fn json_io_and_internal_sources_remain_downcastable() {
        let json = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let application = ApplicationError::from(json);
        assert!(application.source().unwrap().is::<serde_json::Error>());
        let application =
            ApplicationError::from(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(application.kind(), ErrorKind::NotFound);
        assert_eq!(
            application
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        let application = ApplicationError::internal(std::io::Error::other("native cause"));
        assert!(application.source().unwrap().is::<std::io::Error>());
    }

    #[test]
    fn workspace_operations_preserve_the_native_io_source() {
        use crate::operations::{OperationRequest, WorkspaceOperation};
        let root = crate::native::test_support::test_workspace("operation-source");
        let uri = crate::native::path_to_file_uri(&root).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        let registry = crate::SessionRegistry::new(fleximark_model::PositionEncoding::Utf8);
        let result = registry.execute_operation(
            WorkspaceOperation::EditTheme,
            OperationRequest {
                daemon: registry.daemon_instance_id(),
                workspace: Some(&uri),
                document_session: None,
                document_version: None,
                destination: None,
                category: None,
                template: None,
                name: None,
            },
            &crate::CancellationToken::default(),
        );
        let error = result.err().unwrap();
        assert!(matches!(error, ApplicationError::Io(_)));
        assert!(error.source().unwrap().is::<std::io::Error>());
    }
    #[test]
    fn io_errors_keep_their_semantic_category_and_native_cause() {
        for (native, kind) in [
            (std::io::ErrorKind::NotFound, ErrorKind::NotFound),
            (std::io::ErrorKind::PermissionDenied, ErrorKind::Denied),
            (std::io::ErrorKind::AlreadyExists, ErrorKind::Conflict),
            (std::io::ErrorKind::InvalidInput, ErrorKind::Invalid),
            (std::io::ErrorKind::InvalidData, ErrorKind::Invalid),
            (std::io::ErrorKind::Other, ErrorKind::Internal),
        ] {
            let application = ApplicationError::from(std::io::Error::from(native));
            assert_eq!(application.kind(), kind);
            assert_eq!(
                application
                    .source()
                    .unwrap()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                native
            );
        }
    }

    #[test]
    fn config_syntax_and_validation_causes_survive_native_operations() {
        use crate::operations::{OperationRequest, WorkspaceOperation};
        let root = crate::native::test_support::test_workspace("config-source");
        let uri = crate::native::path_to_file_uri(&root).unwrap();
        crate::native::initialize_workspace(&uri).unwrap();
        let mut registry = crate::SessionRegistry::new(fleximark_model::PositionEncoding::Utf8);
        registry
            .configure_workspaces(vec![(
                uri.clone(),
                fleximark_engine::RenderConfig::default(),
            )])
            .unwrap();
        for (text, syntax) in [
            ("schema_version=1\nunknown=true", true),
            ("schema_version=1\n[notes]\nroot='../escape'", false),
        ] {
            std::fs::write(root.join(".fleximark/config.toml"), text).unwrap();
            let result = registry.execute_operation(
                WorkspaceOperation::CreateNote,
                OperationRequest {
                    daemon: registry.daemon_instance_id(),
                    workspace: Some(&uri),
                    document_session: None,
                    document_version: None,
                    destination: None,
                    category: None,
                    template: None,
                    name: None,
                },
                &crate::CancellationToken::default(),
            );
            let error = result.err().unwrap();
            assert_eq!(error.kind(), ErrorKind::Invalid);
            let source = error
                .source()
                .unwrap()
                .downcast_ref::<fleximark_config::ConfigError>()
                .unwrap();
            if syntax {
                assert!(matches!(source, fleximark_config::ConfigError::Syntax(_)));
            } else {
                assert!(
                    matches!(source, fleximark_config::ConfigError::NoteRoot(root) if root == "../escape")
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn engine_parse_and_model_validation_causes_remain_direct_sources() {
        let application = ApplicationError::from(EngineError::Parse(
            fleximark_model::ValidationError::InvalidRange.into(),
        ));
        let engine = application
            .source()
            .unwrap()
            .downcast_ref::<EngineError>()
            .unwrap();
        let parser = match engine {
            EngineError::Parse(parser) => Some(parser),
            _ => None,
        }
        .unwrap();
        // Infer ParseError from the engine variant instead of adding a parser dependency.
        let parser_from_source = engine.source().unwrap().downcast_ref().unwrap();
        assert!(std::ptr::eq(parser, parser_from_source));
        let validation = parser_from_source
            .source()
            .unwrap()
            .downcast_ref::<fleximark_model::ValidationError>()
            .unwrap();
        assert!(matches!(
            validation,
            fleximark_model::ValidationError::InvalidRange
        ));
    }
}
