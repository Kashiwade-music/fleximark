//! Role: Report plugin execution failures and diagnostics.
use fleximark_plugin_sdk::Hook;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDiagnostic {
    pub plugin_id: String,
    pub hook: Hook,
    pub kind: PluginFailureKind,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginFailureKind {
    Timeout,
    MemoryLimit,
    OutputLimit,
    Cancelled,
    Trap,
    MalformedOutput,
    InvalidCandidate,
    PolicyViolation,
}

#[derive(Debug)]
pub struct PluginRun<T> {
    pub value: T,
    pub diagnostics: Vec<PluginDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsafeExportOutput {
    pub html: String,
    pub unsafe_output_used: bool,
}

#[derive(Debug)]
pub enum HostError {
    DuplicatePlugin(String),
    InvalidManifest(String),
    RuntimeLoad(String),
    Integrity(String),
    RequiredPluginFailed {
        plugin_id: String,
        kind: PluginFailureKind,
        message: String,
    },
}

#[derive(Debug)]
pub(super) struct Failure {
    pub(super) kind: PluginFailureKind,
    pub(super) message: String,
}

impl Failure {
    pub(super) fn new(kind: PluginFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicatePlugin(value) => write!(f, "plugin is already registered: {0}", value),
            Self::InvalidManifest(value) => write!(f, "plugin manifest is invalid: {0}", value),
            Self::RuntimeLoad(value) => write!(f, "plugin module could not be loaded: {0}", value),
            Self::Integrity(value) => {
                write!(f, "plugin package integrity check failed: {0}", value)
            }
            Self::RequiredPluginFailed {
                plugin_id,
                kind,
                message,
            } => write!(
                f,
                "plugin {plugin_id} is required and failed ({kind:?}): {message}"
            ),
        }
    }
}
impl std::error::Error for HostError {}
