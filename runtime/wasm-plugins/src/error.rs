//! Role: Report plugin execution failures and diagnostics.
pub use fleximark_plugin_api::{
    HostError, PluginDiagnostic, PluginFailureKind, PluginRun, UnsafeExportOutput,
};

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
