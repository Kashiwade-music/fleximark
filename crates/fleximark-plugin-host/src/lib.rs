use std::collections::{BTreeMap, HashSet};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use ed25519_dalek::{Signature, VerifyingKey};
use fleximark_model::SourceProvenance;
use fleximark_model::{
    Block, Document, Node, NodeId, PositionEncoding, SourcePosition, SourceRange,
};
use fleximark_plugin_sdk::{
    CandidateBlock, CandidateDocument, CandidateIdentity, CandidateNode, EditOrigin, Hook,
    HookRequest, HookResponse, PluginCapabilities, PluginManifest, PreprocessedSource,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, ResourceLimiter, Store};
use wasmtime_wasi::{DirPerms, FilePerms, IoView, WasiCtx, WasiCtxBuilder, WasiView};

mod component_api {
    wasmtime::component::bindgen!({
        path: "../fleximark-plugin-sdk/wit",
        world: "fleximark-plugin",
    });
}

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

trait PluginRuntime: Send + Sync + 'static {
    fn invoke(
        &self,
        request: HookRequest,
        sandbox: SandboxPolicy,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput, RuntimeError>;
}

/// Production runtime for the versioned FlexiMark Component Model world.
/// WASI Preview 2 starts empty, then receives only granted roots and environment access.
pub struct WasmtimeRuntime {
    engine: Engine,
    component: Component,
    invoke_lock: std::sync::Mutex<()>,
}

struct WasmState {
    wasi: WasiCtx,
    table: ResourceTable,
    max_memory_bytes: usize,
    peak_memory_bytes: usize,
    memory_limit_hit: bool,
}

impl IoView for WasmState {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiView for WasmState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

impl ResourceLimiter for WasmState {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = desired <= self.max_memory_bytes;
        self.peak_memory_bytes = self.peak_memory_bytes.max(desired);
        self.memory_limit_hit |= !allowed;
        Ok(allowed)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 10_000)
    }
}

impl WasmtimeRuntime {
    pub fn new(wasm: &[u8]) -> Result<Self, RuntimeError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        let engine =
            Engine::new(&config).map_err(|error| RuntimeError::Malformed(error.to_string()))?;
        let component = Component::new(&engine, wasm)
            .map_err(|error| RuntimeError::Malformed(error.to_string()))?;
        Ok(Self {
            engine,
            component,
            invoke_lock: std::sync::Mutex::new(()),
        })
    }
}

impl PluginRuntime for WasmtimeRuntime {
    fn invoke(
        &self,
        request: HookRequest,
        sandbox: SandboxPolicy,
        cancellation: CancellationToken,
    ) -> Result<RuntimeOutput, RuntimeError> {
        let _invoke_guard = self
            .invoke_lock
            .lock()
            .map_err(|_| RuntimeError::Trap("plugin invocation lock was poisoned".to_owned()))?;
        if cancellation.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        let mut wasi = WasiCtxBuilder::new();
        wasi.allow_blocking_current_thread(true);
        for (name, value) in &sandbox.environment {
            wasi.env(name, value);
        }
        for (index, root) in sandbox.read_roots.iter().enumerate() {
            wasi.preopened_dir(
                root,
                format!("workspace-read-{index}"),
                DirPerms::READ,
                FilePerms::READ,
            )
            .map_err(|error| RuntimeError::Policy(error.to_string()))?;
        }
        for (index, root) in sandbox.write_roots.iter().enumerate() {
            wasi.preopened_dir(
                root,
                format!("workspace-write-{index}"),
                DirPerms::MUTATE,
                FilePerms::WRITE,
            )
            .map_err(|error| RuntimeError::Policy(error.to_string()))?;
        }

        let mut store = Store::new(
            &self.engine,
            WasmState {
                wasi: wasi.build(),
                table: ResourceTable::new(),
                max_memory_bytes: usize::try_from(sandbox.max_linear_memory_bytes)
                    .unwrap_or(usize::MAX),
                peak_memory_bytes: 0,
                memory_limit_hit: false,
            },
        );
        store.limiter(|state| state);
        store
            .set_fuel(sandbox.max_fuel)
            .map_err(|error| RuntimeError::Malformed(error.to_string()))?;
        store.set_epoch_deadline(1);

        let mut linker = Linker::new(&self.engine);
        wasmtime_wasi::add_to_linker_sync(&mut linker)
            .map_err(|error| RuntimeError::Malformed(error.to_string()))?;
        let hook = serde_json::to_value(request.hook())
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| RuntimeError::Malformed("hook name is not a string".to_owned()))?;
        let request_json = serde_json::to_string(&request)
            .map_err(|error| RuntimeError::Malformed(error.to_string()))?;

        let done = Arc::new(AtomicBool::new(false));
        let done_by_watchdog = Arc::clone(&done);
        let cancelled_by_watchdog = cancellation.clone();
        let deadline = Instant::now() + sandbox.timeout;
        let watchdog_engine = self.engine.clone();
        let watchdog = thread::spawn(move || {
            while !done_by_watchdog.load(Ordering::Acquire) {
                if cancelled_by_watchdog.is_cancelled() || Instant::now() >= deadline {
                    watchdog_engine.increment_epoch();
                    return;
                }
                thread::sleep(Duration::from_millis(1));
            }
        });
        let started = Instant::now();
        let execution = (|| {
            let plugin =
                component_api::FleximarkPlugin::instantiate(&mut store, &self.component, &linker)
                    .map_err(|error| classify_wasm_error(error, false, false))?;
            let invocation = component_api::exports::fleximark::plugin::hooks::Invocation {
                api_version: fleximark_plugin_sdk::PLUGIN_API_VERSION,
                hook,
                request_json,
            };
            let response = plugin
                .fleximark_plugin_hooks()
                .call_invoke(&mut store, &invocation)
                .map_err(|error| classify_wasm_error(error, false, false))?
                .map_err(RuntimeError::Trap)?;
            if response.api_version != fleximark_plugin_sdk::PLUGIN_API_VERSION {
                return Err(RuntimeError::Malformed(format!(
                    "guest returned plugin API version {}",
                    response.api_version
                )));
            }
            if response.response_json.len() > sandbox.max_output_bytes {
                return Err(RuntimeError::OutputLimit(
                    "guest response exceeds the output limit".to_owned(),
                ));
            }
            let response = serde_json::from_str(&response.response_json)
                .map_err(|error| RuntimeError::Malformed(error.to_string()))?;
            Ok(RuntimeOutput {
                response,
                peak_memory_bytes: store.data().peak_memory_bytes as u64,
            })
        })();
        done.store(true, Ordering::Release);
        let _ = watchdog.join();
        let timed_out = started.elapsed() >= sandbox.timeout;
        if store.data().memory_limit_hit {
            return Err(RuntimeError::MemoryLimit(
                "linear memory limit exceeded".to_owned(),
            ));
        }
        if cancellation.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        if timed_out {
            return Err(RuntimeError::Timeout(
                "execution deadline exceeded".to_owned(),
            ));
        }
        execution
    }
}

fn classify_wasm_error(error: wasmtime::Error, cancelled: bool, timed_out: bool) -> RuntimeError {
    let message = format!("{error:#}");
    if cancelled {
        RuntimeError::Cancelled
    } else if timed_out
        || matches!(
            error.downcast_ref::<wasmtime::Trap>(),
            Some(wasmtime::Trap::Interrupt | wasmtime::Trap::OutOfFuel)
        )
    {
        RuntimeError::Timeout(message)
    } else {
        RuntimeError::Trap(message)
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn verify_signature(public_key: &str, signature: &[u8], manifest: &[u8]) -> Result<(), HostError> {
    let key_bytes = decode_hex::<32>(public_key)
        .ok_or_else(|| HostError::Integrity("invalid Ed25519 public key".to_owned()))?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| HostError::Integrity("invalid Ed25519 public key".to_owned()))?;
    let signature = Signature::from_slice(signature)
        .map_err(|_| HostError::Integrity("invalid Ed25519 signature encoding".to_owned()))?;
    key.verify_strict(manifest, &signature)
        .map_err(|_| HostError::Integrity("plugin signature verification failed".to_owned()))
}

fn decode_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2 {
        return None;
    }
    let mut output = [0; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(pair).ok()?;
        output[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(output)
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

pub struct VerifiedPluginPackage<'a> {
    pub configured_id: &'a str,
    pub config_order: usize,
    pub manifest_bytes: &'a [u8],
    pub expected_manifest_sha256: &'a str,
    pub wasm_bytes: &'a [u8],
    pub signature_bytes: &'a [u8],
    pub signer_public_key: &'a str,
    pub grants: PluginCapabilities,
    pub environment: BTreeMap<String, String>,
}

struct RegisteredPlugin {
    manifest: PluginManifest,
    grants: PluginCapabilities,
    manifest_hash: String,
    wasm_hash: String,
    environment: BTreeMap<String, String>,
    runtime: Arc<dyn PluginRuntime>,
}

pub struct PluginHost {
    limits: ExecutionLimits,
    policy: HostPolicy,
    plugins: Vec<RegisteredPlugin>,
    config_hash: String,
    generation: u64,
}

impl PluginHost {
    #[cfg(test)]
    fn new(limits: ExecutionLimits, policy: HostPolicy) -> Self {
        Self {
            limits,
            policy,
            plugins: Vec::new(),
            config_hash: sha256(b""),
            generation: 0,
        }
    }

    pub fn configured(
        limits: ExecutionLimits,
        policy: HostPolicy,
        config_hash: String,
        generation: u64,
    ) -> Result<Self, HostError> {
        if !is_sha256(&config_hash) || generation == 0 {
            return Err(HostError::Integrity(
                "config hash or generation is invalid".to_owned(),
            ));
        }
        Ok(Self {
            limits,
            policy,
            plugins: Vec::new(),
            config_hash,
            generation,
        })
    }

    pub fn register_verified(
        &mut self,
        package: VerifiedPluginPackage<'_>,
    ) -> Result<(), HostError> {
        if package.config_order != self.plugins.len() {
            return Err(HostError::Integrity(
                "plugins must be registered once in canonical config order".to_owned(),
            ));
        }
        let manifest_hash = sha256(package.manifest_bytes);
        if manifest_hash != package.expected_manifest_sha256 {
            return Err(HostError::Integrity("manifest hash mismatch".to_owned()));
        }
        verify_signature(
            package.signer_public_key,
            package.signature_bytes,
            package.manifest_bytes,
        )?;
        let manifest_text = std::str::from_utf8(package.manifest_bytes)
            .map_err(|_| HostError::Integrity("manifest is not UTF-8".to_owned()))?;
        let manifest = PluginManifest::from_toml(manifest_text)
            .map_err(|error| HostError::InvalidManifest(error.to_string()))?;
        if manifest.plugin.id != package.configured_id {
            return Err(HostError::Integrity(
                "configured and manifested plugin ids differ".to_owned(),
            ));
        }
        let wasm_hash = sha256(package.wasm_bytes);
        if wasm_hash != manifest.artifact.wasm_sha256 {
            return Err(HostError::Integrity("WASM hash mismatch".to_owned()));
        }
        let runtime = WasmtimeRuntime::new(package.wasm_bytes)
            .map_err(|error| HostError::RuntimeLoad(error.to_string()))?;
        self.register_hashed(
            manifest,
            package.grants,
            package.environment,
            manifest_hash,
            wasm_hash,
            Arc::new(runtime),
        )
    }

    #[cfg(test)]
    fn register(
        &mut self,
        manifest: PluginManifest,
        grants: PluginCapabilities,
        runtime: Arc<dyn PluginRuntime>,
    ) -> Result<(), HostError> {
        let manifest_hash = sha256(&serde_json::to_vec(&manifest).unwrap_or_default());
        self.register_hashed(
            manifest,
            grants,
            BTreeMap::new(),
            manifest_hash,
            sha256(b"test"),
            runtime,
        )
    }

    fn register_hashed(
        &mut self,
        manifest: PluginManifest,
        grants: PluginCapabilities,
        environment: BTreeMap<String, String>,
        manifest_hash: String,
        wasm_hash: String,
        runtime: Arc<dyn PluginRuntime>,
    ) -> Result<(), HostError> {
        manifest
            .validate()
            .map_err(|error| HostError::InvalidManifest(error.to_string()))?;
        if self
            .plugins
            .iter()
            .any(|plugin| plugin.manifest.plugin.id == manifest.plugin.id)
        {
            return Err(HostError::DuplicatePlugin(manifest.plugin.id));
        }
        self.plugins.push(RegisteredPlugin {
            manifest,
            grants,
            manifest_hash,
            wasm_hash,
            environment,
            runtime,
        });
        Ok(())
    }

    pub fn config_hash(&self) -> &str {
        &self.config_hash
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn plugin_set_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.config_hash.as_bytes());
        hasher.update(self.generation.to_le_bytes());
        for plugin in &self.plugins {
            hasher.update(plugin.manifest.plugin.id.as_bytes());
            hasher.update(plugin.manifest_hash.as_bytes());
            hasher.update(plugin.wasm_hash.as_bytes());
            hasher.update(serde_json::to_vec(&plugin.manifest.capabilities).unwrap_or_default());
            hasher.update(serde_json::to_vec(&plugin.grants).unwrap_or_default());
            hasher.update([u8::from(plugin.manifest.plugin.required)]);
        }
        format!("{:x}", hasher.finalize())
    }

    pub fn transform_document(
        &self,
        source: &str,
        document: &Document,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError> {
        let mut current = document.clone();
        let mut diagnostics = Vec::new();
        for plugin in &self.plugins {
            let request = HookRequest::TransformDocument {
                document: current.clone(),
            };
            let invocation = if self.policy.workspace_trusted {
                self.invoke(plugin, request, cancellation)
            } else {
                Err(Failure::new(
                    PluginFailureKind::PolicyViolation,
                    "workspace trust is required before plugin execution",
                ))
            };
            let candidate = match invocation {
                Ok(HookResponse::Document { candidate }) => validate_candidate(
                    &current,
                    candidate,
                    source,
                    &plugin.manifest.plugin.id,
                    &self.limits,
                )
                .map_err(|message| Failure::new(PluginFailureKind::InvalidCandidate, message)),
                Ok(_) => Err(Failure::new(
                    PluginFailureKind::MalformedOutput,
                    "transform_document returned a response for another hook",
                )),
                Err(failure) => Err(failure),
            };
            match candidate {
                Ok(value) => current = value,
                Err(failure) if plugin.manifest.plugin.required => {
                    return Err(HostError::RequiredPluginFailed {
                        plugin_id: plugin.manifest.plugin.id.clone(),
                        kind: failure.kind,
                        message: failure.message,
                    });
                }
                Err(failure) => diagnostics.push(PluginDiagnostic {
                    plugin_id: plugin.manifest.plugin.id.clone(),
                    hook: Hook::TransformDocument,
                    kind: failure.kind,
                    message: failure.message,
                }),
            }
        }
        Ok(PluginRun {
            value: current,
            diagnostics,
        })
    }

    pub fn preprocess_source(
        &self,
        document_version: u64,
        source: &str,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<PreprocessedSource>, HostError> {
        let mut current = PreprocessedSource {
            text: source.to_owned(),
            segments: identity_edit_map(source),
        };
        let mut accepted = false;
        let mut diagnostics = Vec::new();
        for plugin in &self.plugins {
            let result = if self.policy.workspace_trusted {
                match self.invoke(
                    plugin,
                    HookRequest::PreprocessSource {
                        document_version,
                        text: current.text.clone(),
                        edit_map: current.segments.clone(),
                    },
                    cancellation,
                ) {
                    Ok(HookResponse::PreprocessedSource { candidate }) => {
                        validate_edit_map(&candidate, &current.text)
                            .and_then(|()| compose_edit_map(&candidate, &current, source))
                            .map_err(|message| {
                                Failure::new(PluginFailureKind::InvalidCandidate, message)
                            })
                    }
                    Ok(_) => Err(Failure::new(
                        PluginFailureKind::MalformedOutput,
                        "preprocess_source returned a response for another hook",
                    )),
                    Err(failure) => Err(failure),
                }
            } else {
                Err(Failure::new(
                    PluginFailureKind::PolicyViolation,
                    "workspace trust is required before plugin execution",
                ))
            };
            match result {
                Ok(candidate) => {
                    current = candidate;
                    accepted = true;
                }
                Err(failure) if plugin.manifest.plugin.required => {
                    return Err(required_failure(plugin, failure));
                }
                Err(failure) => {
                    diagnostics.push(plugin_diagnostic(plugin, Hook::PreprocessSource, failure))
                }
            }
        }
        if !accepted {
            current.segments.clear();
        }
        Ok(PluginRun {
            value: current,
            diagnostics,
        })
    }

    pub fn transform_blocks(
        &self,
        source: &str,
        document: &Document,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<Document>, HostError> {
        let mut current = document.clone();
        let mut diagnostics = Vec::new();
        for plugin in &self.plugins {
            let mut transformed = current.clone();
            let mut invocation_failure = None;
            for (index, block) in current.blocks.iter().enumerate() {
                let result = if self.policy.workspace_trusted {
                    self.invoke(
                        plugin,
                        HookRequest::TransformBlock {
                            document_version: current.document_version,
                            block: block.clone(),
                        },
                        cancellation,
                    )
                } else {
                    Err(Failure::new(
                        PluginFailureKind::PolicyViolation,
                        "workspace trust is required before plugin execution",
                    ))
                };
                match result {
                    Ok(HookResponse::Block { candidate: block }) => {
                        match resolve_block_invocation(
                            &current,
                            block,
                            &plugin.manifest.plugin.id,
                            &format!("{}:{}", index, current.blocks[index].id.0),
                            &self.limits,
                        ) {
                            Ok(block) => transformed.blocks[index] = block,
                            Err(message) => {
                                invocation_failure = Some(Failure::new(
                                    PluginFailureKind::InvalidCandidate,
                                    message,
                                ));
                                break;
                            }
                        }
                    }
                    Ok(_) => {
                        invocation_failure = Some(Failure::new(
                            PluginFailureKind::MalformedOutput,
                            "transform_block returned a response for another hook",
                        ));
                        break;
                    }
                    Err(failure) => {
                        invocation_failure = Some(failure);
                        break;
                    }
                }
            }
            let result = match invocation_failure {
                Some(failure) => Err(failure),
                None => transformed
                    .validate(source)
                    .map(|()| transformed)
                    .map_err(|error| {
                        Failure::new(PluginFailureKind::InvalidCandidate, error.to_string())
                    }),
            };
            match result {
                Ok(document) => current = document,
                Err(failure) if plugin.manifest.plugin.required => {
                    return Err(required_failure(plugin, failure));
                }
                Err(failure) => {
                    diagnostics.push(plugin_diagnostic(plugin, Hook::TransformBlock, failure))
                }
            }
        }
        Ok(PluginRun {
            value: current,
            diagnostics,
        })
    }

    pub fn extend_render_model(
        &self,
        document: &Document,
        target: &str,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<BTreeMap<String, String>>, HostError> {
        let mut annotations = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for plugin in &self.plugins {
            let result = if self.policy.workspace_trusted {
                match self.invoke(
                    plugin,
                    HookRequest::ExtendRenderModel {
                        document: document.clone(),
                        target: target.to_owned(),
                    },
                    cancellation,
                ) {
                    Ok(HookResponse::RenderAnnotations { annotations }) => Ok(annotations),
                    Ok(_) => Err(Failure::new(
                        PluginFailureKind::MalformedOutput,
                        "extend_render_model returned a response for another hook",
                    )),
                    Err(failure) => Err(failure),
                }
            } else {
                Err(Failure::new(
                    PluginFailureKind::PolicyViolation,
                    "workspace trust is required before plugin execution",
                ))
            };
            match result {
                Ok(candidate) => annotations.extend(candidate),
                Err(failure) if plugin.manifest.plugin.required => {
                    return Err(required_failure(plugin, failure));
                }
                Err(failure) => {
                    diagnostics.push(plugin_diagnostic(plugin, Hook::ExtendRenderModel, failure))
                }
            }
        }
        Ok(PluginRun {
            value: annotations,
            diagnostics,
        })
    }

    pub fn unsafe_export_html(
        &self,
        document_version: u64,
        safe_html: String,
        cancellation: &CancellationToken,
    ) -> Result<PluginRun<UnsafeExportOutput>, HostError> {
        let mut html = safe_html;
        let mut unsafe_output_used = false;
        let mut diagnostics = Vec::new();
        let ordered = self
            .plugins
            .iter()
            .filter(|plugin| plugin.manifest.capabilities.unsafe_html_output)
            .collect::<Vec<_>>();
        for plugin in ordered {
            let failure = if !self.policy.workspace_trusted {
                Some(Failure::new(
                    PluginFailureKind::PolicyViolation,
                    "workspace trust is required before plugin execution",
                ))
            } else if !plugin.grants.unsafe_html_output {
                Some(Failure::new(
                    PluginFailureKind::PolicyViolation,
                    "unsafe_html_output was not granted by the workspace",
                ))
            } else {
                let request = HookRequest::UnsafeExportHtml {
                    document_version,
                    html: html.clone(),
                };
                match self.invoke(plugin, request, cancellation) {
                    Ok(HookResponse::UnsafeExportHtml { html: candidate }) => {
                        unsafe_output_used |= candidate != html;
                        html = candidate;
                        None
                    }
                    Ok(_) => Some(Failure::new(
                        PluginFailureKind::MalformedOutput,
                        "unsafe_export_html returned a response for another hook",
                    )),
                    Err(failure) => Some(failure),
                }
            };
            let Some(failure) = failure else {
                continue;
            };
            if plugin.manifest.plugin.required {
                return Err(HostError::RequiredPluginFailed {
                    plugin_id: plugin.manifest.plugin.id.clone(),
                    kind: failure.kind,
                    message: failure.message,
                });
            }
            diagnostics.push(PluginDiagnostic {
                plugin_id: plugin.manifest.plugin.id.clone(),
                hook: Hook::UnsafeExportHtml,
                kind: failure.kind,
                message: failure.message,
            });
        }
        Ok(PluginRun {
            value: UnsafeExportOutput {
                html,
                unsafe_output_used,
            },
            diagnostics,
        })
    }

    fn invoke(
        &self,
        plugin: &RegisteredPlugin,
        request: HookRequest,
        external_cancellation: &CancellationToken,
    ) -> Result<HookResponse, Failure> {
        if external_cancellation.is_cancelled() {
            return Err(Failure::new(
                PluginFailureKind::Cancelled,
                "document version was cancelled",
            ));
        }
        let sandbox = sandbox_policy(plugin, &self.policy, &self.limits);
        let started = Instant::now();
        let result = plugin
            .runtime
            .invoke(request, sandbox, external_cancellation.clone());
        if external_cancellation.is_cancelled() {
            return Err(Failure::new(
                PluginFailureKind::Cancelled,
                "document version was cancelled",
            ));
        }
        if started.elapsed() >= self.limits.timeout {
            return Err(Failure::new(
                PluginFailureKind::Timeout,
                "execution deadline exceeded",
            ));
        }
        let output = result.map_err(|error| match error {
            RuntimeError::Trap(message) => Failure::new(PluginFailureKind::Trap, message),
            RuntimeError::Malformed(message) => {
                Failure::new(PluginFailureKind::MalformedOutput, message)
            }
            RuntimeError::Timeout(message) => Failure::new(PluginFailureKind::Timeout, message),
            RuntimeError::MemoryLimit(message) => {
                Failure::new(PluginFailureKind::MemoryLimit, message)
            }
            RuntimeError::OutputLimit(message) => {
                Failure::new(PluginFailureKind::OutputLimit, message)
            }
            RuntimeError::Cancelled => Failure::new(
                PluginFailureKind::Cancelled,
                "plugin invocation was cancelled",
            ),
            RuntimeError::Policy(message) => {
                Failure::new(PluginFailureKind::PolicyViolation, message)
            }
        })?;
        if output.peak_memory_bytes > self.limits.max_linear_memory_bytes {
            return Err(Failure::new(
                PluginFailureKind::MemoryLimit,
                "linear memory limit exceeded",
            ));
        }
        let output_size = serde_json::to_vec(&output.response)
            .map_err(|error| Failure::new(PluginFailureKind::MalformedOutput, error.to_string()))?
            .len();
        if output_size > self.limits.max_output_bytes {
            return Err(Failure::new(
                PluginFailureKind::OutputLimit,
                "serialized output limit exceeded",
            ));
        }
        Ok(output.response)
    }
}

fn sandbox_policy(
    plugin: &RegisteredPlugin,
    policy: &HostPolicy,
    limits: &ExecutionLimits,
) -> SandboxPolicy {
    let requested = &plugin.manifest.capabilities;
    let granted = &plugin.grants;
    SandboxPolicy {
        read_roots: if requested.read_workspace && granted.read_workspace {
            vec![policy.workspace_root.clone()]
        } else {
            Vec::new()
        },
        write_roots: if requested.write_workspace && granted.write_workspace {
            vec![policy.workspace_root.clone()]
        } else {
            Vec::new()
        },
        environment: if requested.environment && granted.environment {
            plugin.environment.clone()
        } else {
            BTreeMap::new()
        },
        unsafe_html_output: requested.unsafe_html_output && granted.unsafe_html_output,
        max_linear_memory_bytes: limits.max_linear_memory_bytes,
        max_output_bytes: limits.max_output_bytes,
        max_fuel: limits.max_fuel,
        timeout: limits.timeout,
    }
}

fn plugin_diagnostic(plugin: &RegisteredPlugin, hook: Hook, failure: Failure) -> PluginDiagnostic {
    PluginDiagnostic {
        plugin_id: plugin.manifest.plugin.id.clone(),
        hook,
        kind: failure.kind,
        message: failure.message,
    }
}

fn required_failure(plugin: &RegisteredPlugin, failure: Failure) -> HostError {
    HostError::RequiredPluginFailed {
        plugin_id: plugin.manifest.plugin.id.clone(),
        kind: failure.kind,
        message: failure.message,
    }
}

fn identity_edit_map(source: &str) -> Vec<fleximark_plugin_sdk::EditMapSegment> {
    if source.is_empty() {
        return Vec::new();
    }
    let (line, character) = source.rfind('\n').map_or((0, source.len()), |index| {
        (
            source[..=index]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count(),
            source.len() - index - 1,
        )
    });
    vec![fleximark_plugin_sdk::EditMapSegment {
        output_start: 0,
        output_end: source.len() as u64,
        origin: EditOrigin::Original {
            ranges: vec![SourceRange {
                byte_start: 0,
                byte_end: source.len() as u64,
                start: SourcePosition {
                    line: 0,
                    character: 0,
                    encoding: PositionEncoding::Utf8,
                },
                end: SourcePosition {
                    line: line as u64,
                    character: character as u64,
                    encoding: PositionEncoding::Utf8,
                },
            }],
            primary_range_index: 0,
        },
    }]
}

fn validate_edit_map(candidate: &PreprocessedSource, source: &str) -> Result<(), String> {
    if candidate.text != source && candidate.segments.is_empty() {
        return Err("changed source requires a complete edit map".to_owned());
    }
    let mut expected_start = 0_u64;
    for segment in &candidate.segments {
        if segment.output_start != expected_start || segment.output_end <= segment.output_start {
            return Err("edit map segments must be contiguous and non-empty".to_owned());
        }
        let start = usize::try_from(segment.output_start)
            .map_err(|_| "edit map offset does not fit this platform".to_owned())?;
        let end = usize::try_from(segment.output_end)
            .map_err(|_| "edit map offset does not fit this platform".to_owned())?;
        if !candidate.text.is_char_boundary(start) || !candidate.text.is_char_boundary(end) {
            return Err("edit map offsets must be UTF-8 boundaries".to_owned());
        }
        validate_edit_origin(&segment.origin, source, &candidate.text[start..end])?;
        expected_start = segment.output_end;
    }
    if !candidate.segments.is_empty()
        && expected_start != u64::try_from(candidate.text.len()).unwrap_or(u64::MAX)
    {
        return Err("edit map must cover the complete output".to_owned());
    }
    Ok(())
}

fn compose_edit_map(
    candidate: &PreprocessedSource,
    previous: &PreprocessedSource,
    original: &str,
) -> Result<PreprocessedSource, String> {
    if candidate.segments.is_empty() && candidate.text == previous.text {
        return Ok(previous.clone());
    }
    let mut segments = Vec::new();
    for segment in &candidate.segments {
        match &segment.origin {
            EditOrigin::Original { ranges, .. } => {
                let mut output = segment.output_start;
                for range in ranges {
                    for (length, origin) in map_input_range(range, previous, original)? {
                        let output_end = output + length;
                        segments.push(fleximark_plugin_sdk::EditMapSegment {
                            output_start: output,
                            output_end,
                            origin,
                        });
                        output = output_end;
                    }
                }
                if output != segment.output_end {
                    return Err("composed Original edit-map length changed".to_owned());
                }
            }
            EditOrigin::Derived {
                ranges,
                primary_range_index,
            } => {
                let mut mapped = Vec::new();
                let mut primary_start = None;
                for (index, range) in ranges.iter().enumerate() {
                    let before = mapped.len();
                    for (_, origin) in map_input_range(range, previous, original)? {
                        match origin {
                            EditOrigin::Original { ranges, .. }
                            | EditOrigin::Derived { ranges, .. } => mapped.extend(ranges),
                            EditOrigin::Generated {
                                anchor: Some(anchor),
                            } => mapped.push(anchor),
                            EditOrigin::Generated { anchor: None } => {}
                        }
                    }
                    if index == *primary_range_index as usize {
                        primary_start = (before < mapped.len()).then_some(mapped[before].clone());
                    }
                }
                mapped.sort_by_key(|range| (range.byte_start, range.byte_end));
                mapped.dedup_by_key(|range| (range.byte_start, range.byte_end));
                if mapped
                    .windows(2)
                    .any(|pair| pair[0].byte_end > pair[1].byte_start)
                {
                    return Err("composed Derived ranges overlap".to_owned());
                }
                let origin = if mapped.is_empty() {
                    EditOrigin::Generated { anchor: None }
                } else {
                    let primary = primary_start
                        .and_then(|primary| mapped.iter().position(|range| range == &primary))
                        .unwrap_or(0) as u32;
                    EditOrigin::Derived {
                        ranges: mapped,
                        primary_range_index: primary,
                    }
                };
                segments.push(fleximark_plugin_sdk::EditMapSegment {
                    output_start: segment.output_start,
                    output_end: segment.output_end,
                    origin,
                });
            }
            EditOrigin::Generated { anchor } => {
                let anchor = anchor
                    .as_ref()
                    .map(|range| map_input_range(range, previous, original))
                    .transpose()?
                    .and_then(|origins| {
                        origins.into_iter().find_map(|(_, origin)| match origin {
                            EditOrigin::Original { ranges, .. }
                            | EditOrigin::Derived { ranges, .. } => ranges.into_iter().next(),
                            EditOrigin::Generated { anchor } => anchor,
                        })
                    });
                segments.push(fleximark_plugin_sdk::EditMapSegment {
                    output_start: segment.output_start,
                    output_end: segment.output_end,
                    origin: EditOrigin::Generated { anchor },
                });
            }
        }
    }
    let composed = PreprocessedSource {
        text: candidate.text.clone(),
        segments,
    };
    validate_edit_map(&composed, original)?;
    Ok(composed)
}

fn map_input_range(
    range: &SourceRange,
    previous: &PreprocessedSource,
    original: &str,
) -> Result<Vec<(u64, EditOrigin)>, String> {
    let mut mapped = Vec::new();
    for segment in &previous.segments {
        let start = range.byte_start.max(segment.output_start);
        let end = range.byte_end.min(segment.output_end);
        if start >= end {
            continue;
        }
        let origin = match &segment.origin {
            EditOrigin::Original {
                ranges,
                primary_range_index: _,
            } => {
                let mut skip = start - segment.output_start;
                let mut take = end - start;
                let mut clipped = Vec::new();
                for source_range in ranges {
                    let length = source_range.byte_end - source_range.byte_start;
                    if skip >= length {
                        skip -= length;
                        continue;
                    }
                    let clipped_start = source_range.byte_start + skip;
                    let clipped_end = clipped_start + take.min(length - skip);
                    clipped.push(utf8_source_range(original, clipped_start, clipped_end)?);
                    take -= clipped_end - clipped_start;
                    skip = 0;
                    if take == 0 {
                        break;
                    }
                }
                if take != 0 {
                    return Err("Original edit-map range could not be clipped".to_owned());
                }
                EditOrigin::Original {
                    ranges: clipped,
                    primary_range_index: 0,
                }
            }
            EditOrigin::Derived {
                ranges,
                primary_range_index,
            } => EditOrigin::Derived {
                ranges: ranges.clone(),
                primary_range_index: *primary_range_index,
            },
            EditOrigin::Generated { anchor } => EditOrigin::Generated {
                anchor: anchor.clone(),
            },
        };
        mapped.push((end - start, origin));
    }
    if mapped.iter().map(|(length, _)| *length).sum::<u64>() != range.byte_end - range.byte_start {
        return Err("edit-map range is not covered by the previous output".to_owned());
    }
    Ok(mapped)
}

fn utf8_source_range(source: &str, start: u64, end: u64) -> Result<SourceRange, String> {
    let position = |offset: u64| -> Result<SourcePosition, String> {
        let offset = usize::try_from(offset).map_err(|_| "source offset is too large")?;
        if !source.is_char_boundary(offset) {
            return Err("source offset is not a UTF-8 boundary".to_owned());
        }
        let before = &source[..offset];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        Ok(SourcePosition {
            line: before.bytes().filter(|byte| *byte == b'\n').count() as u64,
            character: (offset - line_start) as u64,
            encoding: PositionEncoding::Utf8,
        })
    };
    Ok(SourceRange {
        byte_start: start,
        byte_end: end,
        start: position(start)?,
        end: position(end)?,
    })
}

fn validate_edit_origin(origin: &EditOrigin, source: &str, output: &str) -> Result<(), String> {
    match origin {
        EditOrigin::Original {
            ranges,
            primary_range_index,
        } => {
            SourceProvenance::Original {
                ranges: ranges.clone(),
                primary_range_index: *primary_range_index,
            }
            .validate(source)
            .map_err(|error| error.to_string())?;
            let mut original = Vec::new();
            for range in ranges {
                let start = usize::try_from(range.byte_start)
                    .map_err(|_| "origin offset does not fit this platform".to_owned())?;
                let end = usize::try_from(range.byte_end)
                    .map_err(|_| "origin offset does not fit this platform".to_owned())?;
                original.extend_from_slice(&source.as_bytes()[start..end]);
            }
            if original != output.as_bytes() {
                return Err(
                    "an Original edit-map segment must exactly reproduce its source ranges"
                        .to_owned(),
                );
            }
        }
        EditOrigin::Derived {
            ranges,
            primary_range_index,
        } => {
            SourceProvenance::Derived {
                ranges: ranges.clone(),
                primary_range_index: *primary_range_index,
                transform: fleximark_model::TransformId("plugin-preprocess-v1".to_owned()),
            }
            .validate(source)
            .map_err(|error| error.to_string())?;
        }
        EditOrigin::Generated { anchor } => {
            if let Some(range) = anchor {
                SourceProvenance::original(range.clone())
                    .validate(source)
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Failure {
    kind: PluginFailureKind,
    message: String,
}

impl Failure {
    fn new(kind: PluginFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

fn validate_candidate(
    current: &Document,
    candidate: CandidateDocument,
    source: &str,
    plugin_id: &str,
    limits: &ExecutionLimits,
) -> Result<Document, String> {
    if candidate.schema_version != current.schema_version
        || candidate.document_version != current.document_version
        || candidate.uri != current.uri
    {
        return Err("candidate changed immutable document identity or version".to_owned());
    }
    let mut known_ids = HashSet::new();
    collect_ids(&current.blocks, &mut known_ids);
    let mut used_ids = HashSet::new();
    let mut creation_keys = HashSet::new();
    let mut node_count = 0;
    let mut blocks = Vec::with_capacity(candidate.blocks.len());
    for block in candidate.blocks {
        blocks.push(resolve_block(
            block,
            &known_ids,
            &mut used_ids,
            &mut creation_keys,
            plugin_id,
            "document",
            limits,
            1,
            &mut node_count,
        )?);
    }
    let document = Document {
        schema_version: candidate.schema_version,
        document_version: candidate.document_version,
        uri: candidate.uri,
        metadata: candidate.metadata,
        blocks,
    };
    document
        .validate(source)
        .map_err(|error| error.to_string())?;
    Ok(document)
}

fn resolve_block_invocation(
    current: &Document,
    candidate: CandidateBlock,
    plugin_id: &str,
    invocation_scope: &str,
    limits: &ExecutionLimits,
) -> Result<Block, String> {
    let mut known_ids = HashSet::new();
    let mut node_count = 0;
    collect_ids(&current.blocks, &mut known_ids);
    resolve_block(
        candidate,
        &known_ids,
        &mut HashSet::new(),
        &mut HashSet::new(),
        plugin_id,
        invocation_scope,
        limits,
        1,
        &mut node_count,
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_block(
    candidate: CandidateBlock,
    known_ids: &HashSet<NodeId>,
    used_ids: &mut HashSet<NodeId>,
    creation_keys: &mut HashSet<String>,
    plugin_id: &str,
    invocation_scope: &str,
    limits: &ExecutionLimits,
    depth: usize,
    node_count: &mut usize,
) -> Result<Block, String> {
    *node_count += 1;
    if *node_count > limits.max_nodes || depth > limits.max_depth {
        return Err("candidate exceeds node count or depth limit".to_owned());
    }
    let id = match candidate.identity {
        CandidateIdentity::Existing { id } => {
            if !known_ids.contains(&id) {
                return Err(format!("candidate references unknown NodeId {}", id.0));
            }
            if !used_ids.insert(id.clone()) {
                return Err(format!("candidate duplicates NodeId {}", id.0));
            }
            id
        }
        CandidateIdentity::Created { key } => {
            if key.0.is_empty() || key.0.len() > 128 || !creation_keys.insert(key.0.clone()) {
                return Err(format!(
                    "candidate has invalid or reused CreationKey {}",
                    key.0
                ));
            }
            let mut salt = 0;
            loop {
                let material = format!("{plugin_id}\0{invocation_scope}\0{}\0{salt}", key.0);
                let id = NodeId(format!(
                    "block-{}",
                    &blake3::hash(material.as_bytes()).to_hex()[..20]
                ));
                if !known_ids.contains(&id) && used_ids.insert(id.clone()) {
                    break id;
                }
                salt += 1;
            }
        }
    };
    let mut children = Vec::with_capacity(candidate.children.len());
    for child in candidate.children {
        children.push(match child {
            CandidateNode::Block(block) => Node::Block(resolve_block(
                block,
                known_ids,
                used_ids,
                creation_keys,
                plugin_id,
                invocation_scope,
                limits,
                depth + 1,
                node_count,
            )?),
            CandidateNode::Inline(inline) => Node::Inline(inline),
        });
    }
    Ok(Block {
        id,
        provenance: candidate.provenance,
        kind: candidate.kind,
        attributes: candidate.attributes,
        children,
    })
}

fn collect_ids(blocks: &[Block], output: &mut HashSet<NodeId>) {
    for block in blocks {
        output.insert(block.id.clone());
        for child in &block.children {
            if let Node::Block(block) = child {
                collect_ids(std::slice::from_ref(block), output);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::{Mutex, OnceLock};

    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use fleximark_model::DocumentUri;
    use fleximark_parser::parse;

    struct TestRuntime {
        handler: Box<
            dyn Fn(
                    HookRequest,
                    SandboxPolicy,
                    CancellationToken,
                ) -> Result<RuntimeOutput, RuntimeError>
                + Send
                + Sync,
        >,
    }

    impl PluginRuntime for TestRuntime {
        fn invoke(
            &self,
            request: HookRequest,
            sandbox: SandboxPolicy,
            cancellation: CancellationToken,
        ) -> Result<RuntimeOutput, RuntimeError> {
            (self.handler)(request, sandbox, cancellation)
        }
    }

    fn manifest(id: &str, required: bool) -> PluginManifest {
        PluginManifest {
            schema_version: 1,
            plugin: fleximark_plugin_sdk::PluginMetadata {
                id: id.to_owned(),
                api_version: 1,
                required,
            },
            artifact: fleximark_plugin_sdk::PluginArtifact {
                wasm_sha256: sha256(b"test"),
            },
            capabilities: PluginCapabilities::default(),
        }
    }

    fn document() -> Document {
        let mut document = parse(DocumentUri("file:///plugin.md".into()), 1, "hello\n").unwrap();
        document.blocks[0].id = NodeId("block-original".into());
        document
    }

    fn runtime(
        handler: impl Fn(
            HookRequest,
            SandboxPolicy,
            CancellationToken,
        ) -> Result<RuntimeOutput, RuntimeError>
        + Send
        + Sync
        + 'static,
    ) -> Arc<dyn PluginRuntime> {
        Arc::new(TestRuntime {
            handler: Box::new(handler),
        })
    }

    fn pass_runtime() -> Arc<dyn PluginRuntime> {
        runtime(|request, _, _| {
            let HookRequest::TransformDocument { document } = request else {
                unreachable!()
            };
            Ok(RuntimeOutput {
                response: HookResponse::Document {
                    candidate: CandidateDocument::from_document(&document),
                },
                peak_memory_bytes: 0,
            })
        })
    }

    fn trusted_host(limits: ExecutionLimits) -> PluginHost {
        PluginHost::new(
            limits,
            HostPolicy {
                workspace_trusted: true,
                workspace_root: "C:/workspace".into(),
            },
        )
    }

    fn wasm_sandbox() -> SandboxPolicy {
        SandboxPolicy {
            read_roots: Vec::new(),
            write_roots: Vec::new(),
            environment: BTreeMap::new(),
            unsafe_html_output: false,
            max_linear_memory_bytes: 16 * 1024 * 1024,
            max_output_bytes: 1024,
            max_fuel: 1_000_000,
            timeout: Duration::from_millis(50),
        }
    }

    fn wasm_request() -> HookRequest {
        HookRequest::PreprocessSource {
            document_version: 1,
            text: String::new(),
            edit_map: Vec::new(),
        }
    }

    fn special_request(html: &str) -> HookRequest {
        HookRequest::UnsafeExportHtml {
            document_version: 1,
            html: html.to_owned(),
        }
    }

    fn fixture_component() -> &'static [u8] {
        static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
        COMPONENT.get_or_init(|| {
            let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let fixture_manifest = manifest_dir
                .join("../../fixtures/plugin-component/Cargo.toml")
                .canonicalize()
                .unwrap();
            let status = Command::new(env!("CARGO"))
                .args([
                    "build",
                    "--quiet",
                    "--manifest-path",
                    fixture_manifest.to_str().unwrap(),
                    "--target",
                    "wasm32-wasip2",
                ])
                .status()
                .unwrap();
            assert!(status.success(), "fixture component build failed");
            std::fs::read(
                fixture_manifest
                    .parent()
                    .unwrap()
                    .join("target/wasm32-wasip2/debug/fleximark_fixture_plugin.wasm"),
            )
            .unwrap()
        })
    }

    fn component_runtime() -> WasmtimeRuntime {
        WasmtimeRuntime::new(fixture_component()).unwrap()
    }

    #[test]
    fn optional_invalid_candidate_is_discarded_before_next_plugin() {
        let mut host = trusted_host(ExecutionLimits::default());
        let invalid = runtime(|request, _, _| {
            let HookRequest::TransformDocument { document } = request else {
                unreachable!()
            };
            let mut candidate = CandidateDocument::from_document(&document);
            candidate.blocks[0].identity = CandidateIdentity::Existing {
                id: NodeId("unknown".into()),
            };
            Ok(RuntimeOutput {
                response: HookResponse::Document { candidate },
                peak_memory_bytes: 0,
            })
        });
        let observed = Arc::new(Mutex::new(None));
        let observed_by_runtime = Arc::clone(&observed);
        let next = runtime(move |request, _, _| {
            let HookRequest::TransformDocument { document } = request else {
                unreachable!()
            };
            *observed_by_runtime.lock().unwrap() = Some(document.blocks[0].id.clone());
            Ok(RuntimeOutput {
                response: HookResponse::Document {
                    candidate: CandidateDocument::from_document(&document),
                },
                peak_memory_bytes: 0,
            })
        });
        host.register(
            manifest("a-invalid", false),
            PluginCapabilities::default(),
            invalid,
        )
        .unwrap();
        host.register(
            manifest("b-next", false),
            PluginCapabilities::default(),
            next,
        )
        .unwrap();
        let original = document();
        let result = host
            .transform_document("hello\n", &original, &CancellationToken::default())
            .unwrap();
        assert_eq!(result.value, original);
        assert_eq!(
            result.diagnostics[0].kind,
            PluginFailureKind::InvalidCandidate
        );
        assert_eq!(
            *observed.lock().unwrap(),
            Some(NodeId("block-original".into()))
        );
    }

    #[test]
    fn required_failure_stops_downstream_and_keeps_input_uncommitted() {
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(
            manifest("a-required", true),
            PluginCapabilities::default(),
            runtime(|_, _, _| Err(RuntimeError::Trap("boom".into()))),
        )
        .unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_by_runtime = Arc::clone(&ran);
        host.register(
            manifest("b-later", false),
            PluginCapabilities::default(),
            runtime(move |request, _, _| {
                ran_by_runtime.store(true, Ordering::Release);
                let HookRequest::TransformDocument { document } = request else {
                    unreachable!()
                };
                Ok(RuntimeOutput {
                    response: HookResponse::Document {
                        candidate: CandidateDocument::from_document(&document),
                    },
                    peak_memory_bytes: 0,
                })
            }),
        )
        .unwrap();
        assert!(matches!(
            host.transform_document("hello\n", &document(), &CancellationToken::default()),
            Err(HostError::RequiredPluginFailed {
                kind: PluginFailureKind::Trap,
                ..
            })
        ));
        assert!(!ran.load(Ordering::Acquire));
    }

    #[test]
    fn duplicate_creation_keys_reject_the_whole_candidate() {
        let mut host = trusted_host(ExecutionLimits::default());
        let creator = runtime(|request, _, _| {
            let HookRequest::TransformDocument { document } = request else {
                unreachable!()
            };
            let mut candidate = CandidateDocument::from_document(&document);
            let template = candidate.blocks[0].clone();
            for block in &mut candidate.blocks {
                block.identity = CandidateIdentity::Created {
                    key: fleximark_plugin_sdk::CreationKey("same".into()),
                };
            }
            let mut duplicate = template;
            duplicate.identity = CandidateIdentity::Created {
                key: fleximark_plugin_sdk::CreationKey("same".into()),
            };
            candidate.blocks.push(duplicate);
            Ok(RuntimeOutput {
                response: HookResponse::Document { candidate },
                peak_memory_bytes: 0,
            })
        });
        host.register(
            manifest("creator", false),
            PluginCapabilities::default(),
            creator,
        )
        .unwrap();
        let result = host
            .transform_document("hello\n", &document(), &CancellationToken::default())
            .unwrap();
        assert_eq!(
            result.diagnostics[0].kind,
            PluginFailureKind::InvalidCandidate
        );
    }

    #[test]
    fn transform_block_creation_keys_are_scoped_to_each_invocation() {
        let source = "first\n\nsecond\n";
        let mut input = parse(DocumentUri("file:///blocks.md".into()), 1, source).unwrap();
        input.blocks[0].id = NodeId("first".into());
        input.blocks[1].id = NodeId("second".into());
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(
            manifest("block-creator", true),
            PluginCapabilities::default(),
            runtime(|request, _, _| {
                let HookRequest::TransformBlock { block, .. } = request else {
                    unreachable!()
                };
                let document = Document {
                    schema_version: 1,
                    document_version: 1,
                    uri: DocumentUri("file:///blocks.md".into()),
                    metadata: Default::default(),
                    blocks: vec![block],
                };
                let mut candidate = CandidateDocument::from_document(&document).blocks.remove(0);
                candidate.identity = CandidateIdentity::Created {
                    key: fleximark_plugin_sdk::CreationKey("same-local-key".into()),
                };
                Ok(RuntimeOutput {
                    response: HookResponse::Block { candidate },
                    peak_memory_bytes: 0,
                })
            }),
        )
        .unwrap();

        let result = host
            .transform_blocks(source, &input, &CancellationToken::default())
            .unwrap();
        assert!(result.diagnostics.is_empty());
        assert_ne!(result.value.blocks[0].id, result.value.blocks[1].id);
        assert!(
            result
                .value
                .blocks
                .iter()
                .all(|block| block.id.0.starts_with("block-"))
        );
    }

    #[test]
    fn capabilities_are_deny_by_default_even_when_requested() {
        let observed = Arc::new(Mutex::new(None));
        let observed_by_runtime = Arc::clone(&observed);
        let denied_runtime = runtime(move |request, sandbox, _| {
            *observed_by_runtime.lock().unwrap() = Some(sandbox);
            let HookRequest::TransformDocument { document } = request else {
                unreachable!()
            };
            Ok(RuntimeOutput {
                response: HookResponse::Document {
                    candidate: CandidateDocument::from_document(&document),
                },
                peak_memory_bytes: 0,
            })
        });
        let mut requested = manifest("requests-all", false);
        requested.capabilities = PluginCapabilities {
            read_workspace: true,
            write_workspace: true,
            environment: true,
            unsafe_html_output: true,
        };
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(requested, PluginCapabilities::default(), denied_runtime)
            .unwrap();
        host.transform_document("hello\n", &document(), &CancellationToken::default())
            .unwrap();
        let sandbox = observed.lock().unwrap().clone().unwrap();
        assert!(sandbox.read_roots.is_empty() && sandbox.write_roots.is_empty());
        assert!(sandbox.environment.is_empty());
        assert!(!sandbox.unsafe_html_output);

        let observed = Arc::new(Mutex::new(None));
        let observed_by_runtime = Arc::clone(&observed);
        let granted_runtime = runtime(move |request, sandbox, _| {
            *observed_by_runtime.lock().unwrap() = Some(sandbox);
            let HookRequest::TransformDocument { document } = request else {
                unreachable!()
            };
            Ok(RuntimeOutput {
                response: HookResponse::Document {
                    candidate: CandidateDocument::from_document(&document),
                },
                peak_memory_bytes: 0,
            })
        });
        let mut requested = manifest("read-only", false);
        requested.capabilities.read_workspace = true;
        requested.capabilities.write_workspace = true;
        let grants = PluginCapabilities {
            read_workspace: true,
            ..PluginCapabilities::default()
        };
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(requested, grants, granted_runtime).unwrap();
        host.transform_document("hello\n", &document(), &CancellationToken::default())
            .unwrap();
        let sandbox = observed.lock().unwrap().clone().unwrap();
        assert_eq!(sandbox.read_roots, vec!["C:/workspace"]);
        assert!(sandbox.write_roots.is_empty());

        let observed = Arc::new(Mutex::new(None));
        let observed_by_runtime = Arc::clone(&observed);
        let runtime = runtime(move |request, sandbox, _| {
            *observed_by_runtime.lock().unwrap() = Some(sandbox);
            let HookRequest::TransformDocument { document } = request else {
                unreachable!()
            };
            Ok(RuntimeOutput {
                response: HookResponse::Document {
                    candidate: CandidateDocument::from_document(&document),
                },
                peak_memory_bytes: 0,
            })
        });
        let mut requested = manifest("write-and-env", false);
        requested.capabilities.write_workspace = true;
        requested.capabilities.environment = true;
        let grants = requested.capabilities.clone();
        let environment = BTreeMap::from([("FLEXIMARK_MODE".to_owned(), "test".to_owned())]);
        let mut host = trusted_host(ExecutionLimits::default());
        host.register_hashed(
            requested,
            grants,
            environment.clone(),
            sha256(b"manifest"),
            sha256(b"test"),
            runtime,
        )
        .unwrap();
        host.transform_document("hello\n", &document(), &CancellationToken::default())
            .unwrap();
        let sandbox = observed.lock().unwrap().clone().unwrap();
        assert!(sandbox.read_roots.is_empty());
        assert_eq!(sandbox.write_roots, vec!["C:/workspace"]);
        assert_eq!(sandbox.environment, environment);
    }

    #[test]
    fn verified_registration_binds_config_manifest_signature_and_wasm() {
        let wasm = fixture_component();
        let manifest = format!(
            "schema_version = 1\n\n[plugin]\nid = \"verified\"\napi_version = 1\nrequired = false\n\n[artifact]\nwasm_sha256 = \"{}\"\n\n[capabilities]\n",
            sha256(wasm)
        );
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let signature = signing_key.sign(manifest.as_bytes()).to_bytes();
        let public_key = signing_key
            .verifying_key()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut host = PluginHost::configured(
            ExecutionLimits::default(),
            HostPolicy {
                workspace_trusted: true,
                workspace_root: "C:/workspace".to_owned(),
            },
            sha256(b"config"),
            1,
        )
        .unwrap();
        host.register_verified(VerifiedPluginPackage {
            configured_id: "verified",
            config_order: 0,
            manifest_bytes: manifest.as_bytes(),
            expected_manifest_sha256: &sha256(manifest.as_bytes()),
            wasm_bytes: wasm,
            signature_bytes: &signature,
            signer_public_key: &public_key,
            grants: PluginCapabilities::default(),
            environment: BTreeMap::new(),
        })
        .unwrap();
        assert_ne!(host.plugin_set_hash(), sha256(b""));

        let mut tampered = PluginHost::configured(
            ExecutionLimits::default(),
            HostPolicy::default(),
            sha256(b"config"),
            1,
        )
        .unwrap();
        let error = tampered
            .register_verified(VerifiedPluginPackage {
                configured_id: "verified",
                config_order: 0,
                manifest_bytes: manifest.as_bytes(),
                expected_manifest_sha256: &sha256(manifest.as_bytes()),
                wasm_bytes: b"tampered",
                signature_bytes: &signature,
                signer_public_key: &public_key,
                grants: PluginCapabilities::default(),
                environment: BTreeMap::new(),
            })
            .unwrap_err();
        assert!(matches!(error, HostError::Integrity(message) if message == "WASM hash mismatch"));
    }

    #[test]
    fn timeout_memory_output_and_cancel_limits_do_not_commit() {
        let original = document();
        let limits = ExecutionLimits {
            timeout: Duration::from_millis(10),
            max_linear_memory_bytes: 8,
            max_output_bytes: 64,
            ..ExecutionLimits::default()
        };
        let cases: Vec<(&str, Arc<dyn PluginRuntime>, PluginFailureKind)> = vec![
            (
                "memory",
                runtime(|request, _, _| {
                    let HookRequest::TransformDocument { document } = request else {
                        unreachable!()
                    };
                    Ok(RuntimeOutput {
                        response: HookResponse::Document {
                            candidate: CandidateDocument::from_document(&document),
                        },
                        peak_memory_bytes: 9,
                    })
                }),
                PluginFailureKind::MemoryLimit,
            ),
            ("output", pass_runtime(), PluginFailureKind::OutputLimit),
            (
                "timeout",
                runtime(|_, _, _| {
                    thread::sleep(Duration::from_millis(20));
                    Err(RuntimeError::Trap("cancelled".into()))
                }),
                PluginFailureKind::Timeout,
            ),
        ];
        for (id, runtime, expected) in cases {
            let mut host = trusted_host(limits.clone());
            host.register(manifest(id, false), PluginCapabilities::default(), runtime)
                .unwrap();
            let result = host
                .transform_document("hello\n", &original, &CancellationToken::default())
                .unwrap();
            assert_eq!(result.value, original);
            assert_eq!(result.diagnostics[0].kind, expected);
        }
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(
            manifest("cancel", false),
            PluginCapabilities::default(),
            pass_runtime(),
        )
        .unwrap();
        let cancelled = CancellationToken::default();
        cancelled.cancel();
        let result = host
            .transform_document("hello\n", &original, &cancelled)
            .unwrap();
        assert_eq!(result.diagnostics[0].kind, PluginFailureKind::Cancelled);
    }

    #[test]
    fn untrusted_workspace_and_unsafe_export_follow_required_boundary() {
        let mut untrusted = PluginHost::new(ExecutionLimits::default(), HostPolicy::default());
        untrusted
            .register(
                manifest("safe", false),
                PluginCapabilities::default(),
                pass_runtime(),
            )
            .unwrap();
        let original = document();
        let result = untrusted
            .transform_document("hello\n", &original, &CancellationToken::default())
            .unwrap();
        assert_eq!(result.value, original);
        assert_eq!(
            result.diagnostics[0].kind,
            PluginFailureKind::PolicyViolation
        );

        let mut trusted = trusted_host(ExecutionLimits::default());
        let mut unsafe_manifest = manifest("unsafe", false);
        unsafe_manifest.capabilities.unsafe_html_output = true;
        trusted
            .register(
                unsafe_manifest,
                PluginCapabilities::default(),
                pass_runtime(),
            )
            .unwrap();
        let result = trusted
            .unsafe_export_html(1, "safe".into(), &CancellationToken::default())
            .unwrap();
        assert_eq!(result.value.html, "safe");
        assert!(!result.value.unsafe_output_used);
        assert_eq!(
            result.diagnostics[0].kind,
            PluginFailureKind::PolicyViolation
        );
    }

    #[test]
    fn unsafe_export_is_marked_only_when_a_granted_hook_changes_output() {
        let mut host = trusted_host(ExecutionLimits::default());
        let mut unsafe_manifest = manifest("unsafe-change", false);
        unsafe_manifest.capabilities.unsafe_html_output = true;
        host.register(
            unsafe_manifest,
            PluginCapabilities {
                unsafe_html_output: true,
                ..PluginCapabilities::default()
            },
            runtime(|request, _, _| {
                let HookRequest::UnsafeExportHtml { html, .. } = request else {
                    unreachable!()
                };
                Ok(RuntimeOutput {
                    response: HookResponse::UnsafeExportHtml {
                        html: format!("{html}<script>unsafe()</script>"),
                    },
                    peak_memory_bytes: 0,
                })
            }),
        )
        .unwrap();
        let result = host
            .unsafe_export_html(1, "safe".into(), &CancellationToken::default())
            .unwrap();
        assert_eq!(result.value.html, "safe<script>unsafe()</script>");
        assert!(result.value.unsafe_output_used);
    }

    #[test]
    fn active_document_version_cancellation_interrupts_publication() {
        let mut host = trusted_host(ExecutionLimits {
            timeout: Duration::from_secs(1),
            ..ExecutionLimits::default()
        });
        host.register(
            manifest("slow", false),
            PluginCapabilities::default(),
            runtime(|_, _, cancellation| {
                while !cancellation.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(RuntimeError::Trap("interrupted".into()))
            }),
        )
        .unwrap();
        let cancellation = CancellationToken::default();
        let trigger = cancellation.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            trigger.cancel();
        });
        let result = host
            .transform_document("hello\n", &document(), &cancellation)
            .unwrap();
        assert_eq!(result.value, document());
        assert_eq!(result.diagnostics[0].kind, PluginFailureKind::Cancelled);
    }

    #[test]
    fn wasmtime_runtime_enforces_fuel_and_wall_clock_timeout() {
        let runtime = component_runtime();
        let started = Instant::now();
        let result = runtime.invoke(
            special_request("__spin__"),
            SandboxPolicy {
                max_fuel: u64::MAX,
                ..wasm_sandbox()
            },
            CancellationToken::default(),
        );
        assert!(matches!(result, Err(RuntimeError::Timeout(_))));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn wasmtime_runtime_enforces_store_memory_limit() {
        let runtime = component_runtime();
        let result = runtime.invoke(
            special_request("__memory__"),
            wasm_sandbox(),
            CancellationToken::default(),
        );
        assert!(
            matches!(result, Err(RuntimeError::MemoryLimit(_))),
            "{result:?}"
        );
    }

    #[test]
    fn wasmtime_runtime_interrupts_on_document_cancellation() {
        let runtime = component_runtime();
        let cancellation = CancellationToken::default();
        let trigger = cancellation.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            trigger.cancel();
        });
        let result = runtime.invoke(
            special_request("__spin__"),
            SandboxPolicy {
                max_fuel: u64::MAX,
                timeout: Duration::from_secs(1),
                ..wasm_sandbox()
            },
            cancellation,
        );
        assert_eq!(result.unwrap_err(), RuntimeError::Cancelled);
    }

    #[test]
    fn component_runtime_timeout_does_not_interrupt_the_next_invocation() {
        let runtime = Arc::new(component_runtime());
        let spinning_runtime = Arc::clone(&runtime);
        let spinning = thread::spawn(move || {
            spinning_runtime.invoke(
                special_request("__spin__"),
                SandboxPolicy {
                    max_fuel: u64::MAX,
                    timeout: Duration::from_millis(30),
                    ..wasm_sandbox()
                },
                CancellationToken::default(),
            )
        });
        thread::sleep(Duration::from_millis(5));
        let fast = runtime.invoke(wasm_request(), wasm_sandbox(), CancellationToken::default());
        assert!(matches!(
            spinning.join().unwrap(),
            Err(RuntimeError::Timeout(_))
        ));
        assert!(fast.is_ok(), "later invocation was interrupted: {fast:?}");
    }

    #[test]
    fn wasmtime_runtime_has_no_default_filesystem_preopens() {
        let runtime = component_runtime();
        let result = runtime
            .invoke(
                special_request("__filesystem__"),
                wasm_sandbox(),
                CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(
            result.response,
            HookResponse::RenderAnnotations {
                annotations: [("root-entry-count".to_owned(), "0".to_owned())]
                    .into_iter()
                    .collect()
            }
        );
    }

    #[test]
    fn all_typed_hooks_run_through_the_same_transaction_boundary() {
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(
            manifest("all-hooks", false),
            PluginCapabilities::default(),
            runtime(|request, _, _| {
                let response = match request {
                    HookRequest::PreprocessSource { text, edit_map, .. } => {
                        HookResponse::PreprocessedSource {
                            candidate: PreprocessedSource {
                                text,
                                segments: edit_map,
                            },
                        }
                    }
                    HookRequest::TransformDocument { document } => HookResponse::Document {
                        candidate: CandidateDocument::from_document(&document),
                    },
                    HookRequest::TransformBlock { block, .. } => {
                        let wrapper = Document {
                            schema_version: 1,
                            document_version: 1,
                            uri: DocumentUri("file:///hook.md".into()),
                            metadata: Default::default(),
                            blocks: vec![block],
                        };
                        HookResponse::Block {
                            candidate: CandidateDocument::from_document(&wrapper).blocks.remove(0),
                        }
                    }
                    HookRequest::ExtendRenderModel { .. } => HookResponse::RenderAnnotations {
                        annotations: BTreeMap::from([("mode".to_owned(), "test".to_owned())]),
                    },
                    HookRequest::UnsafeExportHtml { html, .. } => {
                        HookResponse::UnsafeExportHtml { html }
                    }
                };
                Ok(RuntimeOutput {
                    response,
                    peak_memory_bytes: 0,
                })
            }),
        )
        .unwrap();
        let original = document();
        assert_eq!(
            host.preprocess_source(1, "hello\n", &CancellationToken::default())
                .unwrap()
                .value
                .text,
            "hello\n"
        );
        assert_eq!(
            host.transform_blocks("hello\n", &original, &CancellationToken::default())
                .unwrap()
                .value,
            original
        );
        assert_eq!(
            host.extend_render_model(&original, "preview", &CancellationToken::default())
                .unwrap()
                .value["mode"],
            "test"
        );
    }

    #[test]
    fn invalid_preprocess_map_discards_the_complete_optional_candidate() {
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(
            manifest("bad-map", false),
            PluginCapabilities::default(),
            runtime(|_, _, _| {
                Ok(RuntimeOutput {
                    response: HookResponse::PreprocessedSource {
                        candidate: PreprocessedSource {
                            text: "changed".to_owned(),
                            segments: Vec::new(),
                        },
                    },
                    peak_memory_bytes: 0,
                })
            }),
        )
        .unwrap();
        let run = host
            .preprocess_source(1, "original", &CancellationToken::default())
            .unwrap();
        assert_eq!(run.value.text, "original");
        assert_eq!(run.diagnostics[0].kind, PluginFailureKind::InvalidCandidate);
    }

    #[test]
    fn preprocessors_compose_unicode_and_generated_ranges_to_the_original_snapshot() {
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(
            manifest("reorder-one", true),
            PluginCapabilities::default(),
            runtime(|request, _, _| {
                let HookRequest::PreprocessSource { text, .. } = request else {
                    unreachable!()
                };
                assert_eq!(text, "AéB");
                Ok(RuntimeOutput {
                    response: HookResponse::PreprocessedSource {
                        candidate: PreprocessedSource {
                            text: "éA!".to_owned(),
                            segments: vec![
                                fleximark_plugin_sdk::EditMapSegment {
                                    output_start: 0,
                                    output_end: 2,
                                    origin: EditOrigin::Original {
                                        ranges: vec![utf8_source_range(&text, 1, 3).unwrap()],
                                        primary_range_index: 0,
                                    },
                                },
                                fleximark_plugin_sdk::EditMapSegment {
                                    output_start: 2,
                                    output_end: 3,
                                    origin: EditOrigin::Original {
                                        ranges: vec![utf8_source_range(&text, 0, 1).unwrap()],
                                        primary_range_index: 0,
                                    },
                                },
                                fleximark_plugin_sdk::EditMapSegment {
                                    output_start: 3,
                                    output_end: 4,
                                    origin: EditOrigin::Generated { anchor: None },
                                },
                            ],
                        },
                    },
                    peak_memory_bytes: 0,
                })
            }),
        )
        .unwrap();
        host.register(
            manifest("reorder-two", true),
            PluginCapabilities::default(),
            runtime(|request, _, _| {
                let HookRequest::PreprocessSource { text, .. } = request else {
                    unreachable!()
                };
                assert_eq!(text, "éA!");
                Ok(RuntimeOutput {
                    response: HookResponse::PreprocessedSource {
                        candidate: PreprocessedSource {
                            text: "Aé?".to_owned(),
                            segments: vec![
                                fleximark_plugin_sdk::EditMapSegment {
                                    output_start: 0,
                                    output_end: 3,
                                    origin: EditOrigin::Derived {
                                        ranges: vec![
                                            utf8_source_range(&text, 0, 2).unwrap(),
                                            utf8_source_range(&text, 2, 3).unwrap(),
                                        ],
                                        primary_range_index: 1,
                                    },
                                },
                                fleximark_plugin_sdk::EditMapSegment {
                                    output_start: 3,
                                    output_end: 4,
                                    origin: EditOrigin::Generated {
                                        anchor: Some(utf8_source_range(&text, 2, 3).unwrap()),
                                    },
                                },
                            ],
                        },
                    },
                    peak_memory_bytes: 0,
                })
            }),
        )
        .unwrap();

        let run = host
            .preprocess_source(1, "AéB", &CancellationToken::default())
            .unwrap();
        assert_eq!(run.value.text, "Aé?");
        assert!(run.diagnostics.is_empty());
        assert!(matches!(
            &run.value.segments[0].origin,
            EditOrigin::Derived { ranges, primary_range_index }
                if ranges.len() == 2
                    && ranges[0].byte_start == 0 && ranges[0].byte_end == 1
                    && ranges[1].byte_start == 1 && ranges[1].byte_end == 3
                    && *primary_range_index == 0
        ));
        assert!(matches!(
            &run.value.segments[1].origin,
            EditOrigin::Generated { anchor: Some(anchor) } if anchor.byte_start == 0 && anchor.byte_end == 1
        ));
    }

    #[test]
    fn unchanged_bytes_do_not_discard_explicit_generated_provenance() {
        let mut host = trusted_host(ExecutionLimits::default());
        host.register(
            manifest("regenerate", true),
            PluginCapabilities::default(),
            runtime(|request, _, _| {
                let HookRequest::PreprocessSource { text, .. } = request else {
                    unreachable!()
                };
                Ok(RuntimeOutput {
                    response: HookResponse::PreprocessedSource {
                        candidate: PreprocessedSource {
                            text: text.clone(),
                            segments: vec![fleximark_plugin_sdk::EditMapSegment {
                                output_start: 0,
                                output_end: text.len() as u64,
                                origin: EditOrigin::Generated {
                                    anchor: Some(utf8_source_range(&text, 0, 1).unwrap()),
                                },
                            }],
                        },
                    },
                    peak_memory_bytes: 0,
                })
            }),
        )
        .unwrap();

        let value = host
            .preprocess_source(1, "same", &CancellationToken::default())
            .unwrap()
            .value;
        assert_eq!(value.text, "same");
        assert!(matches!(
            value.segments[0].origin,
            EditOrigin::Generated { .. }
        ));
    }

    #[test]
    fn original_edit_map_requires_ordered_exact_utf8_source_bytes() {
        let source = "a🦀b";
        let range = |start: u64, end: u64| SourceRange {
            byte_start: start,
            byte_end: end,
            start: SourcePosition {
                line: 0,
                character: start,
                encoding: PositionEncoding::Utf8,
            },
            end: SourcePosition {
                line: 0,
                character: end,
                encoding: PositionEncoding::Utf8,
            },
        };
        let cases = [
            ("unordered", "ba", vec![range(5, 6), range(0, 1)]),
            ("rewritten", "zz", vec![range(0, 1), range(5, 6)]),
        ];
        for (id, output, ranges) in cases {
            let output = output.to_owned();
            let mut host = trusted_host(ExecutionLimits::default());
            host.register(
                manifest(id, false),
                PluginCapabilities::default(),
                runtime(move |_, _, _| {
                    Ok(RuntimeOutput {
                        response: HookResponse::PreprocessedSource {
                            candidate: PreprocessedSource {
                                text: output.clone(),
                                segments: vec![fleximark_plugin_sdk::EditMapSegment {
                                    output_start: 0,
                                    output_end: 2,
                                    origin: EditOrigin::Original {
                                        ranges: ranges.clone(),
                                        primary_range_index: 0,
                                    },
                                }],
                            },
                        },
                        peak_memory_bytes: 0,
                    })
                }),
            )
            .unwrap();
            let result = host
                .preprocess_source(1, source, &CancellationToken::default())
                .unwrap();
            assert_eq!(result.value.text, source);
            assert_eq!(
                result.diagnostics[0].kind,
                PluginFailureKind::InvalidCandidate
            );
        }
    }
}
