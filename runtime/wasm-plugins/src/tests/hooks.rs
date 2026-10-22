//! Role: Verify hooks plugin execution invariants.
use super::*;
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
            panic!("unexpected test path")
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
            panic!("unexpected test path")
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
                panic!("unexpected test path")
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
            panic!("unexpected test path")
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
                panic!("unexpected test path")
            };
            let mut candidate = candidate_block(1, "file:///blocks.md", block);
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
            panic!("unexpected test path")
        };
        if invocations_by_runtime.fetch_add(1, Ordering::AcqRel) == 0 {
            let mut candidate =
                candidate_block(document_version, "file:///block-rollback.md", block);
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
            panic!("unexpected test path")
        };
        observed_by_runtime
            .lock()
            .unwrap()
            .push(block.attributes.clone());
        successful_response(HookResponse::Block {
            candidate: candidate_block(document_version, "file:///block-rollback.md", block),
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
