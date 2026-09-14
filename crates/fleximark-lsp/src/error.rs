use fleximark_engine::EngineError;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SessionError {
    #[error("document is not open")]
    NotOpen,
    #[error("document session is unknown or no longer active")]
    UnknownSession,
    #[error("daemon instance does not match this connection")]
    WrongDaemon,
    #[error("document is out of sync")]
    ContentModified,
    #[error("document version is stale")]
    StaleVersion,
    #[error("document version does not match")]
    VersionMismatch,
    #[error("content hash does not match")]
    HashMismatch,
    #[error("incremental edit range is invalid")]
    InvalidRange,
    #[error("a change notification must contain edits")]
    EmptyChange,
    #[error("engine rejected the document: {0}")]
    Engine(String),
}

pub(super) fn engine_error(error: EngineError) -> SessionError {
    match error {
        EngineError::ContentModified => SessionError::ContentModified,
        EngineError::StaleVersion { .. } => SessionError::StaleVersion,
        EngineError::CheckpointMismatch => SessionError::HashMismatch,
        other => SessionError::Engine(other.to_string()),
    }
}
