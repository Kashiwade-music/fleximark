//! Role: Runtime-independent plugin execution ports and hook values.
use fleximark_model::{Document, SourceRange};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hook {
    PreprocessSource,
    TransformDocument,
    TransformBlock,
    ExtendRenderModel,
    UnsafeExportHtml,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreprocessedSource {
    pub text: String,
    pub segments: Vec<EditMapSegment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditMapSegment {
    pub output_start: u64,
    pub output_end: u64,
    pub origin: EditOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EditOrigin {
    Original {
        ranges: Vec<SourceRange>,
        primary_range_index: u32,
    },
    Derived {
        ranges: Vec<SourceRange>,
        primary_range_index: u32,
    },
    Generated {
        anchor: Option<SourceRange>,
    },
}

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

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
pub trait PluginExecutor: Send + Sync {
    fn config_hash(&self) -> &str;
    fn generation(&self) -> u64;
    fn plugin_set_hash(&self) -> String;
    fn preprocess_source(
        &self,
        version: u64,
        source: &str,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<PreprocessedSource>, HostError>;
    fn transform_blocks(
        &self,
        source: &str,
        document: &Document,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError>;
    fn transform_document(
        &self,
        source: &str,
        document: &Document,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError>;
    fn extend_render_model(
        &self,
        document: &Document,
        target: &str,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<BTreeMap<String, String>>, HostError>;
    fn unsafe_export_html(
        &self,
        version: u64,
        html: String,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<UnsafeExportOutput>, HostError>;
}
