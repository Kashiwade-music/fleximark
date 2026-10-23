//! Role: Report session consistency and engine failures.
use fleximark_engine::EngineError;

#[derive(Debug, PartialEq, Eq)]
pub enum SessionError {
    RevisionExhausted,
    NotOpen,
    UnknownSession,
    WrongDaemon,
    ContentModified,
    StaleVersion,
    VersionMismatch,
    InvalidRange,
    EmptyChange,
    Engine(String),
}
impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevisionExhausted => write!(f, "preview revision counter exhausted"),
            Self::NotOpen => write!(f, "document is not open"),
            Self::UnknownSession => write!(f, "document session is unknown or no longer active"),
            Self::WrongDaemon => write!(f, "daemon instance does not match this connection"),
            Self::ContentModified => write!(f, "document is out of sync"),
            Self::StaleVersion => write!(f, "document version is stale"),
            Self::VersionMismatch => write!(f, "document version does not match"),
            Self::InvalidRange => write!(f, "document position or full-text change is invalid"),
            Self::EmptyChange => write!(f, "a change notification must contain edits"),
            Self::Engine(value) => write!(f, "engine rejected the document: {0}", value),
        }
    }
}
impl std::error::Error for SessionError {}

pub(super) fn engine_error(error: EngineError) -> SessionError {
    match error {
        EngineError::ContentModified => SessionError::ContentModified,
        EngineError::StaleVersion { .. } => SessionError::StaleVersion,
        other => SessionError::Engine(other.to_string()),
    }
}
