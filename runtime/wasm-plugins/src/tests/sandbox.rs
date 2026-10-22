//! Role: Verify sandbox plugin execution invariants.
use super::*;
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
                    panic!("unexpected test path")
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
                panic!("unexpected test path")
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
    let cancellation = cancel_after(Duration::from_millis(10));
    let result = host
        .transform_document("hello\n", &document(), &cancellation)
        .unwrap();
    assert_eq!(result.value, document());
    assert_eq!(result.diagnostics[0].kind, PluginFailureKind::Cancelled);
}

#[test]
fn wasmtime_runtime_enforces_memory_and_cancellation_limits() {
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
    let cancellation = cancel_after(Duration::from_millis(10));
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
    let started = Instant::now();
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
    assert!(started.elapsed() < Duration::from_secs(1));
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
                panic!("unexpected test path")
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
                panic!("unexpected test path")
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
                panic!("unexpected test path")
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
