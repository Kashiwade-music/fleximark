use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use fleximark_plugin_sdk::{HookRequest, HookResponse};
use thiserror::Error;

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

#[derive(Debug)]
pub struct RuntimeOutput {
    pub response: HookResponse,
    pub peak_memory_bytes: u64,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("plugin trapped: {0}")]
    Trap(String),
    #[error("plugin output could not be decoded: {0}")]
    Malformed(String),
    #[error("plugin exceeded its execution budget: {0}")]
    Timeout(String),
    #[error("plugin exceeded its memory budget: {0}")]
    MemoryLimit(String),
    #[error("plugin exceeded its output budget: {0}")]
    OutputLimit(String),
    #[error("plugin invocation was cancelled")]
    Cancelled,
    #[error("plugin capability cannot be provided safely: {0}")]
    Policy(String),
}

pub(super) trait PluginRuntime: Send + Sync + 'static {
    fn invoke(
        &self,
        request: HookRequest,
        sandbox: SandboxPolicy,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput, RuntimeError>;
}

mod wasmtime;

pub use wasmtime::WasmtimeRuntime;
