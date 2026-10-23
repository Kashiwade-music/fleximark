//! Role: Execute and validate sandboxed WASM plugins.
mod candidate;
mod edit_map;
mod error;
mod executor;
mod package;
mod pipeline;
mod runtime_types;
#[path = "runtime/wasmtime.rs"]
mod wasmtime;

pub use error::{HostError, PluginDiagnostic, PluginFailureKind, PluginRun, UnsafeExportOutput};
pub use package::VerifiedPluginPackage;
pub use pipeline::PluginHost;
pub use runtime_types::{
    CancellationToken, ExecutionLimits, HostPolicy, RuntimeError, RuntimeOutput, SandboxPolicy,
};
pub use wasmtime::WasmtimeRuntime;
