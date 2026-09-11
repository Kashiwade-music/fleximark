use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use fleximark_model::Document;
use fleximark_plugin_sdk::{
    Hook, HookRequest, HookResponse, PluginCapabilities, PluginManifest, PreprocessedSource,
};
use sha2::{Digest, Sha256};

use crate::candidate::{resolve_block_invocation, validate_candidate};
use crate::edit_map::{compose_edit_map, identity_edit_map, validate_edit_map};
use crate::error::{
    Failure, HostError, PluginDiagnostic, PluginFailureKind, PluginRun, UnsafeExportOutput,
};
use crate::package::{
    RegisteredPlugin, VerifiedPluginPackage, is_sha256, sha256, verify_signature,
};
use crate::runtime::{
    CancellationToken, ExecutionLimits, HostPolicy, PluginRuntime, RuntimeError, SandboxPolicy,
    WasmtimeRuntime,
};

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
            let invocation = self.invoke_trusted(plugin, || Ok(request), cancellation);
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
                Err(failure) => {
                    record_or_abort(plugin, Hook::TransformDocument, failure, &mut diagnostics)?
                }
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
            let result = match self.invoke_trusted(
                plugin,
                || {
                    Ok(HookRequest::PreprocessSource {
                        document_version,
                        text: current.text.clone(),
                        edit_map: current.segments.clone(),
                    })
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
            };
            match result {
                Ok(candidate) => {
                    current = candidate;
                    accepted = true;
                }
                Err(failure) => {
                    record_or_abort(plugin, Hook::PreprocessSource, failure, &mut diagnostics)?
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
                let result = self.invoke_trusted(
                    plugin,
                    || {
                        Ok(HookRequest::TransformBlock {
                            document_version: current.document_version,
                            block: block.clone(),
                        })
                    },
                    cancellation,
                );
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
                Err(failure) => {
                    record_or_abort(plugin, Hook::TransformBlock, failure, &mut diagnostics)?
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
            let result = match self.invoke_trusted(
                plugin,
                || {
                    Ok(HookRequest::ExtendRenderModel {
                        document: document.clone(),
                        target: target.to_owned(),
                    })
                },
                cancellation,
            ) {
                Ok(HookResponse::RenderAnnotations { annotations }) => Ok(annotations),
                Ok(_) => Err(Failure::new(
                    PluginFailureKind::MalformedOutput,
                    "extend_render_model returned a response for another hook",
                )),
                Err(failure) => Err(failure),
            };
            match result {
                Ok(candidate) => annotations.extend(candidate),
                Err(failure) => {
                    record_or_abort(plugin, Hook::ExtendRenderModel, failure, &mut diagnostics)?
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
            let invocation = self.invoke_trusted(
                plugin,
                || {
                    if !plugin.grants.unsafe_html_output {
                        return Err(Failure::new(
                            PluginFailureKind::PolicyViolation,
                            "unsafe_html_output was not granted by the workspace",
                        ));
                    }
                    Ok(HookRequest::UnsafeExportHtml {
                        document_version,
                        html: html.clone(),
                    })
                },
                cancellation,
            );
            let failure = match invocation {
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
            };
            let Some(failure) = failure else {
                continue;
            };
            record_or_abort(plugin, Hook::UnsafeExportHtml, failure, &mut diagnostics)?;
        }
        Ok(PluginRun {
            value: UnsafeExportOutput {
                html,
                unsafe_output_used,
            },
            diagnostics,
        })
    }

    fn invoke_trusted(
        &self,
        plugin: &RegisteredPlugin,
        request: impl FnOnce() -> Result<HookRequest, Failure>,
        external_cancellation: &CancellationToken,
    ) -> Result<HookResponse, Failure> {
        if !self.policy.workspace_trusted {
            return Err(Failure::new(
                PluginFailureKind::PolicyViolation,
                "workspace trust is required before plugin execution",
            ));
        }
        self.invoke(plugin, request()?, external_cancellation)
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

fn record_or_abort(
    plugin: &RegisteredPlugin,
    hook: Hook,
    failure: Failure,
    diagnostics: &mut Vec<PluginDiagnostic>,
) -> Result<(), HostError> {
    if plugin.manifest.plugin.required {
        return Err(required_failure(plugin, failure));
    }
    diagnostics.push(plugin_diagnostic(plugin, hook, failure));
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
