use fleximark_plugin_sdk::Hook;
use serde::{Deserialize, Serialize};
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum HostError {
    #[error("plugin is already registered: {0}")]
    DuplicatePlugin(String),
    #[error("plugin manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("plugin module could not be loaded: {0}")]
    RuntimeLoad(String),
    #[error("plugin package integrity check failed: {0}")]
    Integrity(String),
    #[error("plugin {plugin_id} is required and failed ({kind:?}): {message}")]
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
