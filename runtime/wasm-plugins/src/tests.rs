//! Role: Execute and validate sandboxed WASM plugins.
use crate::wasmtime::WasmtimeRuntime;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use ed25519_dalek::{Signer, SigningKey};
use fleximark_model::{Document, DocumentUri, NodeId, SourceRange};
use fleximark_parser::parse;
use fleximark_plugin_sdk::{
    CandidateDocument, CandidateIdentity, EditOrigin, Hook, HookRequest, HookResponse,
    PluginCapabilities, PluginManifest, PreprocessedSource,
};

use super::PluginHost;
use crate::edit_map::test_utf8_source_range as utf8_source_range;
use crate::error::{HostError, PluginDiagnostic, PluginFailureKind, UnsafeExportOutput};
use crate::package::{VerifiedPluginPackage, sha256};
use crate::runtime_types::{
    CancellationToken, ExecutionLimits, HostPolicy, PluginRuntime, RuntimeError, RuntimeOutput,
    SandboxPolicy,
};

struct TestRuntime {
    handler: Box<
        dyn Fn(HookRequest, SandboxPolicy, CancellationToken) -> Result<RuntimeOutput, RuntimeError>
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
            api_version: fleximark_plugin_sdk::PLUGIN_API_VERSION,
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

fn signing_public_key(signing_key: &SigningKey) -> String {
    signing_key
        .verifying_key()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

fn successful_response(response: HookResponse) -> Result<RuntimeOutput, RuntimeError> {
    Ok(RuntimeOutput {
        response,
        peak_memory_bytes: 0,
    })
}

fn document_response(document: &Document) -> Result<RuntimeOutput, RuntimeError> {
    successful_response(HookResponse::Document {
        candidate: CandidateDocument::from_document(document),
    })
}

fn candidate_block(
    document_version: u64,
    uri: &str,
    block: fleximark_model::Block,
) -> fleximark_plugin_sdk::CandidateBlock {
    CandidateDocument::from_document(&Document {
        schema_version: fleximark_model::DOCUMENT_SCHEMA_VERSION,
        document_version,
        uri: DocumentUri(uri.into()),
        metadata: Default::default(),
        blocks: vec![block],
    })
    .blocks
    .remove(0)
}

fn observing_runtime(observed: Arc<Mutex<Option<SandboxPolicy>>>) -> Arc<dyn PluginRuntime> {
    runtime(move |request, sandbox, _| {
        *observed.lock().unwrap() = Some(sandbox);
        let HookRequest::TransformDocument { document } = request else {
            panic!("unexpected test path")
        };
        document_response(&document)
    })
}

fn observed_sandbox(
    requested: PluginCapabilities,
    grants: PluginCapabilities,
    environment: BTreeMap<String, String>,
) -> SandboxPolicy {
    let observed = Arc::new(Mutex::new(None));
    let mut plugin = manifest("observed", false);
    plugin.capabilities = requested;
    let mut host = trusted_host(ExecutionLimits::default());
    host.register_hashed(
        plugin,
        grants,
        environment,
        sha256(b"manifest"),
        sha256(b"test"),
        observing_runtime(Arc::clone(&observed)),
    )
    .unwrap();
    host.transform_document("hello\n", &document(), &CancellationToken::default())
        .unwrap();
    observed.lock().unwrap().clone().unwrap()
}

fn pass_runtime() -> Arc<dyn PluginRuntime> {
    runtime(|request, _, _| {
        let HookRequest::TransformDocument { document } = request else {
            panic!("unexpected test path")
        };
        document_response(&document)
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

fn cancel_after(delay: Duration) -> CancellationToken {
    let cancellation = CancellationToken::default();
    let trigger = cancellation.clone();
    thread::spawn(move || {
        thread::sleep(delay);
        trigger.cancel();
    });
    cancellation
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
            .join("../../tests/fixtures/plugin-component/Cargo.toml")
            .canonicalize()
            .unwrap();
        let fixture_target = manifest_dir.join("../../target/fixture-wasm");
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "--quiet",
                "--manifest-path",
                fixture_manifest.to_str().unwrap(),
                "--target-dir",
                fixture_target.to_str().unwrap(),
                "--target",
                "wasm32-wasip2",
                "--locked",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "fixture component build failed");
        std::fs::read(fixture_target.join("wasm32-wasip2/debug/fleximark_fixture_plugin.wasm"))
            .unwrap()
    })
}

fn component_runtime() -> WasmtimeRuntime {
    WasmtimeRuntime::new(fixture_component()).unwrap()
}

fn register_hook_runtime(
    host: &mut PluginHost,
    hook: &Hook,
    id: &str,
    required: bool,
    runtime: Arc<dyn PluginRuntime>,
) {
    let mut plugin = manifest(id, required);
    let mut grants = PluginCapabilities::default();
    if hook == &Hook::UnsafeExportHtml {
        plugin.capabilities.unsafe_html_output = true;
        grants.unsafe_html_output = true;
    }
    host.register(plugin, grants, runtime).unwrap();
}

fn register_runtime(
    host: &mut PluginHost,
    id: &str,
    required: bool,
    runtime: Arc<dyn PluginRuntime>,
) {
    host.register(
        manifest(id, required),
        PluginCapabilities::default(),
        runtime,
    )
    .unwrap();
}

fn malformed_hook_runtime() -> Arc<dyn PluginRuntime> {
    runtime(|request, _, _| {
        let response = match request {
            HookRequest::ExtendRenderModel { .. } => HookResponse::UnsafeExportHtml {
                html: "wrong-hook".to_owned(),
            },
            _ => HookResponse::RenderAnnotations {
                annotations: BTreeMap::new(),
            },
        };
        successful_response(response)
    })
}

fn passing_hook_runtime(ran: Arc<AtomicBool>) -> Arc<dyn PluginRuntime> {
    runtime(move |request, _, _| {
        ran.store(true, Ordering::Release);
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
            HookRequest::TransformBlock {
                document_version,
                block,
            } => HookResponse::Block {
                candidate: candidate_block(document_version, "file:///hook-matrix.md", block),
            },
            HookRequest::ExtendRenderModel { .. } => HookResponse::RenderAnnotations {
                annotations: BTreeMap::from([("mode".into(), "test".into())]),
            },
            HookRequest::UnsafeExportHtml { html, .. } => HookResponse::UnsafeExportHtml { html },
        };
        successful_response(response)
    })
}

fn committing_hook_runtime() -> Arc<dyn PluginRuntime> {
    runtime(|request, _, _| {
        let response = match request {
            HookRequest::PreprocessSource { text, .. } => {
                let end = text.len() as u64;
                HookResponse::PreprocessedSource {
                    candidate: PreprocessedSource {
                        text: format!("{text}!"),
                        segments: vec![
                            fleximark_plugin_sdk::EditMapSegment {
                                output_start: 0,
                                output_end: end,
                                origin: EditOrigin::Original {
                                    ranges: vec![utf8_source_range(&text, 0, end).unwrap()],
                                    primary_range_index: 0,
                                },
                            },
                            fleximark_plugin_sdk::EditMapSegment {
                                output_start: end,
                                output_end: end + 1,
                                origin: EditOrigin::Generated { anchor: None },
                            },
                        ],
                    },
                }
            }
            HookRequest::TransformDocument { document } => {
                let mut candidate = CandidateDocument::from_document(&document);
                candidate.blocks[0]
                    .attributes
                    .insert("data-prefix".into(), "document".into());
                HookResponse::Document { candidate }
            }
            HookRequest::TransformBlock {
                document_version,
                block,
            } => {
                let mut candidate =
                    candidate_block(document_version, "file:///hook-commit.md", block);
                candidate
                    .attributes
                    .insert("data-prefix".into(), "block".into());
                HookResponse::Block { candidate }
            }
            HookRequest::ExtendRenderModel { .. } => HookResponse::RenderAnnotations {
                annotations: BTreeMap::from([("prefix".into(), "annotation".into())]),
            },
            HookRequest::UnsafeExportHtml { html, .. } => HookResponse::UnsafeExportHtml {
                html: format!("{html}<prefix>"),
            },
        };
        successful_response(response)
    })
}

fn assert_committed_hook(host: &PluginHost, hook: &Hook) -> Vec<PluginDiagnostic> {
    let original = document();
    match hook {
        Hook::PreprocessSource => {
            let run = host
                .preprocess_source(1, "hello\n", &CancellationToken::default())
                .unwrap();
            assert_eq!(run.value.text, "hello\n!");
            run.diagnostics
        }
        Hook::TransformDocument => {
            let run = host
                .transform_document("hello\n", &original, &CancellationToken::default())
                .unwrap();
            assert_eq!(run.value.blocks[0].attributes["data-prefix"], "document");
            run.diagnostics
        }
        Hook::TransformBlock => {
            let run = host
                .transform_blocks("hello\n", &original, &CancellationToken::default())
                .unwrap();
            assert_eq!(run.value.blocks[0].attributes["data-prefix"], "block");
            run.diagnostics
        }
        Hook::ExtendRenderModel => {
            let run = host
                .extend_render_model(&original, "preview", &CancellationToken::default())
                .unwrap();
            assert_eq!(run.value["prefix"], "annotation");
            run.diagnostics
        }
        Hook::UnsafeExportHtml => {
            let run = host
                .unsafe_export_html(1, "safe".into(), &CancellationToken::default())
                .unwrap();
            assert_eq!(run.value.html, "safe<prefix>");
            assert!(run.value.unsafe_output_used);
            run.diagnostics
        }
    }
}

fn invoke_hook(host: &PluginHost, hook: &Hook) -> Result<Vec<PluginDiagnostic>, HostError> {
    let original = document();
    match hook {
        Hook::PreprocessSource => {
            let run = host.preprocess_source(1, "hello\n", &CancellationToken::default())?;
            assert_eq!(run.value.text, "hello\n");
            Ok(run.diagnostics)
        }
        Hook::TransformDocument => {
            let run =
                host.transform_document("hello\n", &original, &CancellationToken::default())?;
            assert_eq!(run.value, original);
            Ok(run.diagnostics)
        }
        Hook::TransformBlock => {
            let run = host.transform_blocks("hello\n", &original, &CancellationToken::default())?;
            assert_eq!(run.value, original);
            Ok(run.diagnostics)
        }
        Hook::ExtendRenderModel => {
            let run =
                host.extend_render_model(&original, "preview", &CancellationToken::default())?;
            assert_eq!(run.value["mode"], "test");
            Ok(run.diagnostics)
        }
        Hook::UnsafeExportHtml => {
            let run =
                host.unsafe_export_html(1, "safe".to_owned(), &CancellationToken::default())?;
            assert_eq!(
                run.value,
                UnsafeExportOutput {
                    html: "safe".to_owned(),
                    unsafe_output_used: false,
                }
            );
            Ok(run.diagnostics)
        }
    }
}

#[path = "tests/hooks.rs"]
mod hooks;
#[path = "tests/packages.rs"]
mod packages;
#[path = "tests/sandbox.rs"]
mod sandbox;
