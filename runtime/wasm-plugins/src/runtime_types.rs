//! Role: Execute and validate sandboxed WASM plugins.
use std::collections::BTreeMap;
use std::time::Duration;

use fleximark_plugin_sdk::{HookRequest, HookResponse};

#[derive(Clone, Debug)]
pub struct ExecutionLimits {
    pub timeout: Duration,
    pub max_linear_memory_bytes: u64,
    pub max_output_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_fuel: u64,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(250),
            max_linear_memory_bytes: 64 * 1024 * 1024,
            max_output_bytes: 4 * 1024 * 1024,
            max_nodes: 100_000,
            max_depth: 128,
            max_fuel: 10_000_000,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct HostPolicy {
    pub workspace_trusted: bool,
    pub workspace_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub unsafe_html_output: bool,
    pub max_linear_memory_bytes: u64,
    pub max_output_bytes: usize,
    pub max_fuel: u64,
    pub timeout: Duration,
}

pub use fleximark_plugin_api::CancellationToken;

#[derive(Debug)]
pub struct RuntimeOutput {
    pub response: HookResponse,
    pub peak_memory_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Trap(String),
    Malformed(String),
    Timeout(String),
    MemoryLimit(String),
    OutputLimit(String),
    Cancelled,
    Policy(String),
}

pub(crate) trait PluginRuntime: Send + Sync + 'static {
    fn invoke(
        &self,
        request: HookRequest,
        sandbox: SandboxPolicy,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput, RuntimeError>;
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Trap(value) => write!(f, "plugin trapped: {0}", value),
            Self::Malformed(value) => write!(f, "plugin output could not be decoded: {0}", value),
            Self::Timeout(value) => write!(f, "plugin exceeded its execution budget: {0}", value),
            Self::MemoryLimit(value) => write!(f, "plugin exceeded its memory budget: {0}", value),
            Self::OutputLimit(value) => write!(f, "plugin exceeded its output budget: {0}", value),
            Self::Cancelled => f.write_str("plugin invocation was cancelled"),
            Self::Policy(value) => {
                write!(f, "plugin capability cannot be provided safely: {0}", value)
            }
        }
    }
}
impl std::error::Error for RuntimeError {}
