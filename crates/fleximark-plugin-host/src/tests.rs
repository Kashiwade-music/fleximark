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
use fleximark_model::{
    Document, DocumentUri, NodeId, PositionEncoding, SourcePosition, SourceRange,
};
use fleximark_parser::parse;
use fleximark_plugin_sdk::{
    CandidateDocument, CandidateIdentity, EditOrigin, Hook, HookRequest, HookResponse,
    PluginCapabilities, PluginManifest, PreprocessedSource,
};

use super::PluginHost;
use crate::edit_map::test_utf8_source_range as utf8_source_range;
use crate::error::{HostError, PluginDiagnostic, PluginFailureKind, UnsafeExportOutput};
use crate::package::{VerifiedPluginPackage, sha256};
use crate::runtime::{
    CancellationToken, ExecutionLimits, HostPolicy, PluginRuntime, RuntimeError, RuntimeOutput,
    SandboxPolicy, WasmtimeRuntime,
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

fn observing_runtime(observed: Arc<Mutex<Option<SandboxPolicy>>>) -> Arc<dyn PluginRuntime> {
    runtime(move |request, sandbox, _| {
        *observed.lock().unwrap() = Some(sandbox);
        let HookRequest::TransformDocument { document } = request else {
            unreachable!()
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
            unreachable!()
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
        let fixture_target = fixture_manifest.parent().unwrap().join("target");
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
            } => {
                let wrapper = Document {
                    schema_version: 1,
                    document_version,
                    uri: DocumentUri("file:///hook-matrix.md".into()),
                    metadata: Default::default(),
                    blocks: vec![block],
                };
                HookResponse::Block {
                    candidate: CandidateDocument::from_document(&wrapper).blocks.remove(0),
                }
            }
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
                let wrapper = Document {
                    schema_version: 1,
                    document_version,
                    uri: DocumentUri("file:///hook-commit.md".into()),
                    metadata: Default::default(),
                    blocks: vec![block],
                };
                let mut candidate = CandidateDocument::from_document(&wrapper).blocks.remove(0);
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

#[test]
fn every_hook_preserves_optional_and_required_malformed_failure_boundaries() {
    let cases = [
        (
            Hook::PreprocessSource,
            "preprocess_source returned a response for another hook",
        ),
        (
            Hook::TransformBlock,
            "transform_block returned a response for another hook",
        ),
        (
            Hook::TransformDocument,
            "transform_document returned a response for another hook",
        ),
        (
            Hook::ExtendRenderModel,
            "extend_render_model returned a response for another hook",
        ),
        (
            Hook::UnsafeExportHtml,
            "unsafe_export_html returned a response for another hook",
        ),
    ];

    for (hook, message) in cases {
        let mut optional = trusted_host(ExecutionLimits::default());
        register_hook_runtime(
            &mut optional,
            &hook,
            "a-malformed",
            false,
            malformed_hook_runtime(),
        );
        register_hook_runtime(
            &mut optional,
            &hook,
            "b-malformed",
            false,
            malformed_hook_runtime(),
        );
        let downstream_ran = Arc::new(AtomicBool::new(false));
        register_hook_runtime(
            &mut optional,
            &hook,
            "c-downstream",
            false,
            passing_hook_runtime(Arc::clone(&downstream_ran)),
        );
        assert_eq!(
            invoke_hook(&optional, &hook).unwrap(),
            vec![
                PluginDiagnostic {
                    plugin_id: "a-malformed".to_owned(),
                    hook: hook.clone(),
                    kind: PluginFailureKind::MalformedOutput,
                    message: message.to_owned(),
                },
                PluginDiagnostic {
                    plugin_id: "b-malformed".to_owned(),
                    hook: hook.clone(),
                    kind: PluginFailureKind::MalformedOutput,
                    message: message.to_owned(),
                },
            ],
            "optional diagnostics changed for {hook:?}"
        );
        assert!(downstream_ran.load(Ordering::Acquire));

        let mut required = trusted_host(ExecutionLimits::default());
        register_hook_runtime(
            &mut required,
            &hook,
            "a-required",
            true,
            malformed_hook_runtime(),
        );
        let downstream_ran = Arc::new(AtomicBool::new(false));
        register_hook_runtime(
            &mut required,
            &hook,
            "b-downstream",
            false,
            passing_hook_runtime(Arc::clone(&downstream_ran)),
        );
        assert!(matches!(
            invoke_hook(&required, &hook),
            Err(HostError::RequiredPluginFailed {
                plugin_id,
                kind: PluginFailureKind::MalformedOutput,
                message: actual,
            }) if plugin_id == "a-required" && actual == message
        ));
        assert!(!downstream_ran.load(Ordering::Acquire));
    }
}

#[test]
fn optional_failure_preserves_the_previous_plugin_commit_for_every_hook() {
    for hook in [
        Hook::PreprocessSource,
        Hook::TransformDocument,
        Hook::TransformBlock,
        Hook::ExtendRenderModel,
        Hook::UnsafeExportHtml,
    ] {
        let mut host = trusted_host(ExecutionLimits::default());
        register_hook_runtime(&mut host, &hook, "prefix", false, committing_hook_runtime());
        register_hook_runtime(&mut host, &hook, "failure", false, malformed_hook_runtime());
        assert_eq!(assert_committed_hook(&host, &hook).len(), 1, "{hook:?}");
    }
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
        successful_response(HookResponse::Document { candidate })
    });
    let observed = Arc::new(Mutex::new(None));
    let observed_by_runtime = Arc::clone(&observed);
    let next = runtime(move |request, _, _| {
        let HookRequest::TransformDocument { document } = request else {
            unreachable!()
        };
        *observed_by_runtime.lock().unwrap() = Some(document.blocks[0].id.clone());
        document_response(&document)
    });
    register_runtime(&mut host, "a-invalid", false, invalid);
    register_runtime(&mut host, "b-next", false, next);
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
    register_runtime(
        &mut host,
        "a-required",
        true,
        runtime(|_, _, _| Err(RuntimeError::Trap("boom".into()))),
    );
    let ran = Arc::new(AtomicBool::new(false));
    let ran_by_runtime = Arc::clone(&ran);
    register_runtime(
        &mut host,
        "b-later",
        false,
        runtime(move |request, _, _| {
            ran_by_runtime.store(true, Ordering::Release);
            let HookRequest::TransformDocument { document } = request else {
                unreachable!()
            };
            document_response(&document)
        }),
    );
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
        successful_response(HookResponse::Document { candidate })
    });
    register_runtime(&mut host, "creator", false, creator);
    let result = host
        .transform_document("hello\n", &document(), &CancellationToken::default())
        .unwrap();
    assert_eq!(
        result.diagnostics[0].kind,
        PluginFailureKind::InvalidCandidate
    );
}

#[test]
fn candidate_node_and_depth_limits_reject_without_committing() {
    let original = document();
    let cases = [
        (
            "node-limit",
            ExecutionLimits {
                max_nodes: 0,
                ..ExecutionLimits::default()
            },
        ),
        (
            "depth-limit",
            ExecutionLimits {
                max_depth: 0,
                ..ExecutionLimits::default()
            },
        ),
    ];
    for (plugin_id, limits) in cases {
        let mut host = trusted_host(limits);
        register_runtime(&mut host, plugin_id, false, pass_runtime());
        let run = host
            .transform_document("hello\n", &original, &CancellationToken::default())
            .unwrap();
        assert_eq!(run.value, original);
        assert_eq!(
            run.diagnostics,
            vec![PluginDiagnostic {
                plugin_id: plugin_id.to_owned(),
                hook: Hook::TransformDocument,
                kind: PluginFailureKind::InvalidCandidate,
                message: "candidate exceeds node count or depth limit".to_owned(),
            }]
        );
    }
}

#[test]
fn transform_block_creation_keys_are_scoped_to_each_invocation() {
    let source = "first\n\nsecond\n";
    let mut input = parse(DocumentUri("file:///blocks.md".into()), 1, source).unwrap();
    input.blocks[0].id = NodeId("first".into());
    input.blocks[1].id = NodeId("second".into());
    let mut host = trusted_host(ExecutionLimits::default());
    register_runtime(
        &mut host,
        "block-creator",
        true,
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
            successful_response(HookResponse::Block { candidate })
        }),
    );

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
fn optional_transform_block_failure_rolls_back_every_block_before_the_next_plugin() {
    let source = "first\n\nsecond\n";
    let original = parse(DocumentUri("file:///block-rollback.md".into()), 1, source).unwrap();
    let invocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let invocations_by_runtime = Arc::clone(&invocations);
    let partial_then_malformed = runtime(move |request, _, _| {
        let HookRequest::TransformBlock {
            document_version,
            block,
        } = request
        else {
            unreachable!()
        };
        if invocations_by_runtime.fetch_add(1, Ordering::AcqRel) == 0 {
            let wrapper = Document {
                schema_version: 1,
                document_version,
                uri: DocumentUri("file:///block-rollback.md".into()),
                metadata: Default::default(),
                blocks: vec![block],
            };
            let mut candidate = CandidateDocument::from_document(&wrapper).blocks.remove(0);
            candidate
                .attributes
                .insert("data-partial".to_owned(), "must-rollback".to_owned());
            successful_response(HookResponse::Block { candidate })
        } else {
            successful_response(HookResponse::RenderAnnotations {
                annotations: BTreeMap::new(),
            })
        }
    });
    let observed = Arc::new(Mutex::new(Vec::new()));
    let observed_by_runtime = Arc::clone(&observed);
    let downstream = runtime(move |request, _, _| {
        let HookRequest::TransformBlock {
            document_version,
            block,
        } = request
        else {
            unreachable!()
        };
        observed_by_runtime
            .lock()
            .unwrap()
            .push(block.attributes.clone());
        let wrapper = Document {
            schema_version: 1,
            document_version,
            uri: DocumentUri("file:///block-rollback.md".into()),
            metadata: Default::default(),
            blocks: vec![block],
        };
        successful_response(HookResponse::Block {
            candidate: CandidateDocument::from_document(&wrapper).blocks.remove(0),
        })
    });
    let mut host = trusted_host(ExecutionLimits::default());
    register_runtime(&mut host, "partial-block", false, partial_then_malformed);
    register_runtime(&mut host, "downstream-block", false, downstream);

    let run = host
        .transform_blocks(source, &original, &CancellationToken::default())
        .unwrap();
    assert_eq!(run.value, original);
    assert_eq!(
        run.diagnostics,
        vec![PluginDiagnostic {
            plugin_id: "partial-block".to_owned(),
            hook: Hook::TransformBlock,
            kind: PluginFailureKind::MalformedOutput,
            message: "transform_block returned a response for another hook".to_owned(),
        }]
    );
    assert_eq!(
        observed.lock().unwrap().as_slice(),
        &[BTreeMap::new(), BTreeMap::new()]
    );
}

#[test]
fn capabilities_are_deny_by_default_even_when_requested() {
    let all = PluginCapabilities {
        read_workspace: true,
        write_workspace: true,
        environment: true,
        unsafe_html_output: true,
    };
    let sandbox = observed_sandbox(all.clone(), PluginCapabilities::default(), BTreeMap::new());
    assert!(sandbox.read_roots.is_empty() && sandbox.write_roots.is_empty());
    assert!(sandbox.environment.is_empty());
    assert!(!sandbox.unsafe_html_output);

    let requested = PluginCapabilities {
        read_workspace: true,
        write_workspace: true,
        ..PluginCapabilities::default()
    };
    let sandbox = observed_sandbox(
        requested,
        PluginCapabilities {
            read_workspace: true,
            ..PluginCapabilities::default()
        },
        BTreeMap::new(),
    );
    assert_eq!(sandbox.read_roots, vec!["C:/workspace"]);
    assert!(sandbox.write_roots.is_empty());

    let requested = PluginCapabilities {
        write_workspace: true,
        environment: true,
        ..PluginCapabilities::default()
    };
    let environment = BTreeMap::from([("FLEXIMARK_MODE".to_owned(), "test".to_owned())]);
    let sandbox = observed_sandbox(requested.clone(), requested, environment.clone());
    assert!(sandbox.read_roots.is_empty());
    assert_eq!(sandbox.write_roots, vec!["C:/workspace"]);
    assert_eq!(sandbox.environment, environment);

    let sandbox = observed_sandbox(
        PluginCapabilities::default(),
        all.clone(),
        BTreeMap::from([("SECRET".to_owned(), "not-exposed".to_owned())]),
    );
    assert!(sandbox.read_roots.is_empty() && sandbox.write_roots.is_empty());
    assert!(sandbox.environment.is_empty());
    assert!(!sandbox.unsafe_html_output);

    let ran = Arc::new(AtomicBool::new(false));
    let ran_by_runtime = Arc::clone(&ran);
    let untrusted_runtime = runtime(move |request, _, _| {
        ran_by_runtime.store(true, Ordering::Release);
        let HookRequest::TransformDocument { document } = request else {
            unreachable!()
        };
        document_response(&document)
    });
    let mut requested = manifest("untrusted-fully-granted", false);
    requested.capabilities = all.clone();
    let mut host = PluginHost::new(
        ExecutionLimits::default(),
        HostPolicy {
            workspace_trusted: false,
            workspace_root: "C:/workspace".to_owned(),
        },
    );
    host.register(requested, all, untrusted_runtime).unwrap();
    let result = host
        .transform_document("hello\n", &document(), &CancellationToken::default())
        .unwrap();
    assert_eq!(result.value, document());
    assert_eq!(
        result.diagnostics,
        vec![PluginDiagnostic {
            plugin_id: "untrusted-fully-granted".to_owned(),
            hook: Hook::TransformDocument,
            kind: PluginFailureKind::PolicyViolation,
            message: "workspace trust is required before plugin execution".to_owned(),
        }]
    );
    assert!(!ran.load(Ordering::Acquire));
}

#[test]
fn verified_registration_binds_config_manifest_signature_and_wasm() {
    assert_verified_package_rejections_are_retryable();
}

fn assert_verified_package_rejections_are_retryable() {
    let cases = [
        (
            VerifiedPackageCorruption::ManifestHash,
            "manifest hash mismatch",
        ),
        (
            VerifiedPackageCorruption::ManifestAfterSigning,
            "plugin signature verification failed",
        ),
        (
            VerifiedPackageCorruption::Signature,
            "plugin signature verification failed",
        ),
        (
            VerifiedPackageCorruption::PublicKey,
            "plugin signature verification failed",
        ),
        (
            VerifiedPackageCorruption::ConfiguredId,
            "configured and manifested plugin ids differ",
        ),
        (VerifiedPackageCorruption::Wasm, "WASM hash mismatch"),
    ];
    let wasm = fixture_component();
    let manifest = format!(
        "schema_version = 1\n\n[plugin]\nid = \"verified\"\napi_version = 1\nrequired = false\n\n[artifact]\nwasm_sha256 = \"{}\"\n\n[capabilities]\n",
        sha256(wasm)
    );
    let modified_manifest = manifest.replace("required = false", "required = true");
    let manifest_hash = sha256(manifest.as_bytes());
    let modified_manifest_hash = sha256(modified_manifest.as_bytes());
    let wrong_manifest_hash = sha256(b"different manifest bytes");
    let signing_key = SigningKey::from_bytes(&[7; 32]);
    let signature = signing_key.sign(manifest.as_bytes()).to_bytes();
    let mut modified_signature = signature;
    modified_signature[0] ^= 1;
    let public_key = signing_key
        .verifying_key()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let wrong_public_key = SigningKey::from_bytes(&[8; 32])
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
    let empty_hash = host.plugin_set_hash();

    for (corruption, expected_message) in cases {
        let (configured_id, manifest_bytes, expected_hash, wasm_bytes, signature_bytes, signer_key) =
            match corruption {
                VerifiedPackageCorruption::ManifestHash => (
                    "verified",
                    manifest.as_bytes(),
                    wrong_manifest_hash.as_str(),
                    wasm,
                    signature.as_slice(),
                    public_key.as_str(),
                ),
                VerifiedPackageCorruption::ManifestAfterSigning => (
                    "verified",
                    modified_manifest.as_bytes(),
                    modified_manifest_hash.as_str(),
                    wasm,
                    signature.as_slice(),
                    public_key.as_str(),
                ),
                VerifiedPackageCorruption::Signature => (
                    "verified",
                    manifest.as_bytes(),
                    manifest_hash.as_str(),
                    wasm,
                    modified_signature.as_slice(),
                    public_key.as_str(),
                ),
                VerifiedPackageCorruption::PublicKey => (
                    "verified",
                    manifest.as_bytes(),
                    manifest_hash.as_str(),
                    wasm,
                    signature.as_slice(),
                    wrong_public_key.as_str(),
                ),
                VerifiedPackageCorruption::ConfiguredId => (
                    "another-id",
                    manifest.as_bytes(),
                    manifest_hash.as_str(),
                    wasm,
                    signature.as_slice(),
                    public_key.as_str(),
                ),
                VerifiedPackageCorruption::Wasm => (
                    "verified",
                    manifest.as_bytes(),
                    manifest_hash.as_str(),
                    b"tampered".as_slice(),
                    signature.as_slice(),
                    public_key.as_str(),
                ),
            };
        let error = host
            .register_verified(VerifiedPluginPackage {
                configured_id,
                config_order: 0,
                manifest_bytes,
                expected_manifest_sha256: expected_hash,
                wasm_bytes,
                signature_bytes,
                signer_public_key: signer_key,
                grants: PluginCapabilities::default(),
                environment: BTreeMap::new(),
            })
            .unwrap_err();
        match error {
            HostError::Integrity(message) => assert_eq!(message, expected_message),
            other => panic!("{corruption:?} returned the wrong error variant: {other}"),
        }
        assert_eq!(host.plugin_set_hash(), empty_hash);
    }

    // Every rejected package left config_order 0 available for the same valid package.
    host.register_verified(VerifiedPluginPackage {
        configured_id: "verified",
        config_order: 0,
        manifest_bytes: manifest.as_bytes(),
        expected_manifest_sha256: &manifest_hash,
        wasm_bytes: wasm,
        signature_bytes: &signature,
        signer_public_key: &public_key,
        grants: PluginCapabilities::default(),
        environment: BTreeMap::new(),
    })
    .unwrap();
    assert_ne!(host.plugin_set_hash(), empty_hash);
}

#[derive(Clone, Copy, Debug)]
enum VerifiedPackageCorruption {
    ManifestHash,
    ManifestAfterSigning,
    Signature,
    PublicKey,
    ConfiguredId,
    Wasm,
}

#[test]
fn verified_registration_rejects_noncanonical_config_order_without_mutating_the_host() {
    let wasm = fixture_component();
    let manifest = format!(
        "schema_version = 1\n\n[plugin]\nid = \"ordered\"\napi_version = 1\nrequired = false\n\n[artifact]\nwasm_sha256 = \"{}\"\n\n[capabilities]\n",
        sha256(wasm)
    );
    let signing_key = SigningKey::from_bytes(&[19; 32]);
    let signature = signing_key.sign(manifest.as_bytes()).to_bytes();
    let manifest_hash = sha256(manifest.as_bytes());
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
        sha256(b"ordered-config"),
        1,
    )
    .unwrap();
    let empty_hash = host.plugin_set_hash();
    let package = |config_order| VerifiedPluginPackage {
        configured_id: "ordered",
        config_order,
        manifest_bytes: manifest.as_bytes(),
        expected_manifest_sha256: &manifest_hash,
        wasm_bytes: wasm,
        signature_bytes: &signature,
        signer_public_key: &public_key,
        grants: PluginCapabilities::default(),
        environment: BTreeMap::new(),
    };

    assert!(matches!(
        host.register_verified(package(1)),
        Err(HostError::Integrity(message))
            if message == "plugins must be registered once in canonical config order"
    ));
    assert_eq!(host.plugin_set_hash(), empty_hash);

    host.register_verified(package(0)).unwrap();
    let registered_hash = host.plugin_set_hash();
    assert_ne!(registered_hash, empty_hash);
    assert!(matches!(
        host.register_verified(package(0)),
        Err(HostError::Integrity(message))
            if message == "plugins must be registered once in canonical config order"
    ));
    assert_eq!(host.plugin_set_hash(), registered_hash);
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
        register_runtime(&mut host, id, false, runtime);
        let result = host
            .transform_document("hello\n", &original, &CancellationToken::default())
            .unwrap();
        assert_eq!(result.value, original);
        assert_eq!(result.diagnostics[0].kind, expected);
    }
    let mut host = trusted_host(ExecutionLimits::default());
    register_runtime(&mut host, "cancel", false, pass_runtime());
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
    register_runtime(&mut untrusted, "safe", false, pass_runtime());
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
            successful_response(HookResponse::UnsafeExportHtml {
                html: format!("{html}<script>unsafe()</script>"),
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
    register_runtime(
        &mut host,
        "slow",
        false,
        runtime(|_, _, cancellation| {
            while !cancellation.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Err(RuntimeError::Trap("interrupted".into()))
        }),
    );
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
    let mut all_hooks = manifest("all-hooks", false);
    all_hooks.capabilities.unsafe_html_output = true;
    let ran = Arc::new(AtomicBool::new(false));
    host.register(
        all_hooks,
        PluginCapabilities {
            unsafe_html_output: true,
            ..PluginCapabilities::default()
        },
        passing_hook_runtime(Arc::clone(&ran)),
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
        host.transform_document("hello\n", &original, &CancellationToken::default())
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
    assert_eq!(
        host.unsafe_export_html(1, "safe".into(), &CancellationToken::default())
            .unwrap()
            .value,
        UnsafeExportOutput {
            html: "safe".to_owned(),
            unsafe_output_used: false,
        }
    );
    assert!(ran.load(Ordering::Acquire));
}

#[test]
fn invalid_preprocess_map_discards_the_complete_optional_candidate() {
    let mut host = trusted_host(ExecutionLimits::default());
    register_runtime(
        &mut host,
        "bad-map",
        false,
        runtime(|_, _, _| {
            successful_response(HookResponse::PreprocessedSource {
                candidate: PreprocessedSource {
                    text: "changed".to_owned(),
                    segments: Vec::new(),
                },
            })
        }),
    );
    let run = host
        .preprocess_source(1, "original", &CancellationToken::default())
        .unwrap();
    assert_eq!(run.value.text, "original");
    assert_eq!(run.diagnostics[0].kind, PluginFailureKind::InvalidCandidate);
}

#[test]
fn preprocessors_compose_unicode_and_generated_ranges_to_the_original_snapshot() {
    let mut host = trusted_host(ExecutionLimits::default());
    register_runtime(
        &mut host,
        "reorder-one",
        true,
        runtime(|request, _, _| {
            let HookRequest::PreprocessSource { text, .. } = request else {
                unreachable!()
            };
            assert_eq!(text, "AéB");
            successful_response(HookResponse::PreprocessedSource {
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
            })
        }),
    );
    register_runtime(
        &mut host,
        "reorder-two",
        true,
        runtime(|request, _, _| {
            let HookRequest::PreprocessSource { text, .. } = request else {
                unreachable!()
            };
            assert_eq!(text, "éA!");
            successful_response(HookResponse::PreprocessedSource {
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
            })
        }),
    );

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
    register_runtime(
        &mut host,
        "regenerate",
        true,
        runtime(|request, _, _| {
            let HookRequest::PreprocessSource { text, .. } = request else {
                unreachable!()
            };
            successful_response(HookResponse::PreprocessedSource {
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
            })
        }),
    );

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
        byte_start: start.try_into().unwrap(),
        byte_end: end.try_into().unwrap(),
        start: SourcePosition {
            line: 0.into(),
            character: start.try_into().unwrap(),
            encoding: PositionEncoding::Utf8,
        },
        end: SourcePosition {
            line: 0.into(),
            character: end.try_into().unwrap(),
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
        register_runtime(
            &mut host,
            id,
            false,
            runtime(move |_, _, _| {
                successful_response(HookResponse::PreprocessedSource {
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
                })
            }),
        );
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
