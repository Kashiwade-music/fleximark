//! Role: Verify session synchronization and workspace reconfiguration.
use crate::test_plugins::TestPlugins;

use crate::requests::{DocumentAttachmentRequest, DocumentCheckpointRequest, Position};
use fleximark_engine::EngineError;

use super::*;

const DOCUMENT_URI: &str = "file:///doc.md";

fn open(registry: &mut SessionRegistry, text: &str, version: i64) {
    open_uri(registry, DOCUMENT_URI, text, version);
}

fn open_uri(registry: &mut SessionRegistry, uri: &str, text: &str, version: i64) {
    registry
        .open_with_cancellation(
            DidOpenParams {
                text_document: TextDocumentItem {
                    uri: uri.into(),
                    version,
                    text: text.into(),
                },
            },
            &CancellationToken::default(),
        )
        .unwrap();
}

fn change(uri: &str, version: i64, range: Option<Range>, text: &str) -> DidChangeParams {
    DidChangeParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: uri.into(),
            version,
        },
        content_changes: vec![ContentChange {
            range,
            text: text.into(),
        }],
    }
}

fn session_id<'a>(registry: &'a SessionRegistry, uri: &str) -> &'a str {
    registry.session_id_for_uri(uri).unwrap()
}

fn attach_params(
    registry: &SessionRegistry,
    uri: &str,
    version: u32,
    text: &str,
) -> DocumentAttachmentRequest {
    DocumentAttachmentRequest {
        daemon_instance_id: registry.daemon_instance_id().into(),
        uri: uri.into(),
        expected_document_version: version.into(),
        content_hash: content_hash(text),
    }
}

fn checkpoint_params(
    registry: &SessionRegistry,
    session_id: &str,
    version: u32,
    hash: String,
) -> DocumentCheckpointRequest {
    DocumentCheckpointRequest {
        daemon_instance_id: registry.daemon_instance_id().into(),
        document_session_id: session_id.into(),
        document_version: version.into(),
        content_hash: hash,
    }
}

fn document<'a>(registry: &'a SessionRegistry, uri: &str, version: i64) -> &'a DocumentSession {
    registry
        .document(
            registry.daemon_instance_id(),
            session_id(registry, uri),
            version,
        )
        .unwrap()
}

fn configured_host(label: &str, _trusted: bool) -> (Arc<dyn PluginExecutor>, RenderConfig) {
    let host: Arc<dyn PluginExecutor> = Arc::new(TestPlugins {
        hash: content_hash(label),
    });
    let config = RenderConfig::for_plugins(Default::default(), None, host.as_ref());
    (host, config)
}
fn render_fingerprint(
    registry: &mut SessionRegistry,
    uri: &str,
    version: i64,
    preview_id: &str,
) -> String {
    let daemon = registry.daemon_instance_id().to_owned();
    let session_id = session_id(registry, uri).to_owned();
    registry
        .render_with_cancellation(
            &daemon,
            &session_id,
            version,
            preview_id,
            &CancellationToken::default(),
        )
        .unwrap()
        .renderer_fingerprint
}

fn assets_exceeding_total_limit() -> ResolvedAssets {
    (0_u8..9)
        .map(|index| {
            fleximark_engine::ResolvedRenderAsset::from_validated_bytes(
                format!("file:///asset-{index}.bin"),
                "application/octet-stream".into(),
                &vec![index; 1024 * 1024],
            )
            .unwrap()
        })
        .collect::<Vec<_>>()
        .into()
}

#[test]
fn utf16_position_resolves_through_the_authoritative_session() {
    let mut registry = SessionRegistry::new(PositionEncoding::Utf16);
    open(&mut registry, "# 😀 heading\n", 1);
    let daemon = registry.daemon_instance_id().to_owned();
    let session = session_id(&registry, DOCUMENT_URI).to_owned();
    let entry = registry
        .navigation_at_position(
            &daemon,
            &session,
            1,
            &Position {
                line: 0,
                character: 4,
            },
        )
        .unwrap()
        .expect("heading has a navigation node");
    assert!(entry.source_range.byte_start <= 6);
    assert!(entry.source_range.byte_end >= 6);
    assert_eq!(entry.source_range.start.encoding, PositionEncoding::Utf8);
}

#[test]
fn stale_change_marks_session_out_of_sync_until_full_text() {
    let mut registry = SessionRegistry::new(PositionEncoding::Utf16);
    open(&mut registry, "old", 3);
    let stale = change(DOCUMENT_URI, 3, None, "ignored");
    assert_eq!(
        registry.change_with_cancellation(stale, &CancellationToken::default()),
        Err(SessionError::StaleVersion)
    );
    assert!(
        registry
            .index
            .by_uri(DOCUMENT_URI)
            .unwrap()
            .engine
            .is_out_of_sync()
    );
    let requests = registry.take_full_text_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].daemon_instance_id,
        registry.daemon_instance_id()
    );
    assert_eq!(
        requests[0].document_session_id,
        session_id(&registry, DOCUMENT_URI)
    );

    registry
        .change_with_cancellation(
            change(DOCUMENT_URI, 9, None, "fresh"),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(
        !registry
            .index
            .by_uri(DOCUMENT_URI)
            .unwrap()
            .engine
            .is_out_of_sync()
    );
}

#[test]
fn bad_checkpoint_requests_full_text_and_blocks_render_lookup() {
    let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
    open(&mut registry, "text", 1);
    let attached = registry
        .attach(&attach_params(&registry, DOCUMENT_URI, 1, "text"))
        .unwrap();
    let error = registry
        .checkpoint(&checkpoint_params(
            &registry,
            &attached.document_session_id,
            1,
            content_hash("different"),
        ))
        .unwrap_err();
    assert_eq!(error, SessionError::HashMismatch);
    assert_eq!(
        registry
            .document(
                registry.daemon_instance_id(),
                &attached.document_session_id,
                1
            )
            .err()
            .unwrap(),
        SessionError::ContentModified
    );
    assert_eq!(registry.take_full_text_requests().len(), 1);
    registry
        .change_with_cancellation(
            change(DOCUMENT_URI, 1, None, "text"),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(
        registry
            .document(
                registry.daemon_instance_id(),
                &attached.document_session_id,
                1
            )
            .is_ok()
    );
}

#[test]
fn duplicate_open_change_checkpoint_close_and_reopen_keep_one_session_index() {
    let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
    let daemon = registry.daemon_instance_id().to_owned();
    open(&mut registry, "one", 1);
    let first = session_id(&registry, DOCUMENT_URI).to_owned();

    open(&mut registry, "two", 2);
    let second = session_id(&registry, DOCUMENT_URI).to_owned();
    assert_ne!(first, second);
    assert_eq!(
        registry.current_version(&daemon, &first),
        Err(SessionError::UnknownSession)
    );
    let attached = registry
        .attach(&attach_params(&registry, DOCUMENT_URI, 2, "two"))
        .unwrap();
    assert_eq!(attached.document_session_id, second);

    registry
        .change_with_cancellation(
            change(DOCUMENT_URI, 3, None, "three"),
            &CancellationToken::default(),
        )
        .unwrap();
    registry
        .checkpoint(&checkpoint_params(
            &registry,
            &second,
            3,
            content_hash("three"),
        ))
        .unwrap();
    assert_eq!(registry.current_version(&daemon, &second), Ok(3));

    registry.close(DidCloseParams {
        text_document: TextDocumentIdentifier {
            uri: DOCUMENT_URI.into(),
        },
    });
    assert_eq!(registry.session_id_for_uri(DOCUMENT_URI), None);
    assert_eq!(
        registry.current_version(&daemon, &second),
        Err(SessionError::UnknownSession)
    );
    assert_eq!(
        registry
            .attach(&attach_params(&registry, DOCUMENT_URI, 3, "three"))
            .unwrap_err(),
        SessionError::NotOpen
    );

    open(&mut registry, "reopened", 4);
    let third = session_id(&registry, DOCUMENT_URI);
    assert_ne!(third, first);
    assert_ne!(third, second);
}

#[test]
fn session_index_replaces_colliding_session_ids_atomically() {
    let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
    open_uri(&mut registry, "file:///first.md", "first", 1);
    open_uri(&mut registry, "file:///second.md", "second", 1);
    let first_id = session_id(&registry, "file:///first.md").to_owned();
    let second_id = session_id(&registry, "file:///second.md").to_owned();

    let mut replacement = registry.index.remove_by_uri("file:///second.md").unwrap();
    replacement.id = first_id.clone();
    registry.index.insert(replacement);

    assert!(registry.index.by_uri("file:///first.md").is_none());
    assert_eq!(
        registry
            .index
            .by_session(&first_id)
            .map(|session| session.uri.as_str()),
        Some("file:///second.md")
    );
    assert!(registry.index.by_session(&second_id).is_none());
    assert_eq!(registry.index.iter().count(), 1);
}

#[test]
fn incremental_edits_use_utf8_utf16_and_utf32_units() {
    let cases = [
        (PositionEncoding::Utf8, 1, 5),
        (PositionEncoding::Utf16, 1, 3),
        (PositionEncoding::Utf32, 1, 2),
    ];
    for (encoding, start, end) in cases {
        let mut registry = SessionRegistry::new(encoding);
        open(&mut registry, "a😀b\n", 1);
        registry
            .change_with_cancellation(
                change(
                    DOCUMENT_URI,
                    2,
                    Some(Range {
                        start: Position {
                            line: 0,
                            character: start,
                        },
                        end: Position {
                            line: 0,
                            character: end,
                        },
                    }),
                    "x",
                ),
                &CancellationToken::default(),
            )
            .unwrap();
        let session = document(&registry, DOCUMENT_URI, 2);
        assert_eq!(session.source(), "axb\n", "encoding: {encoding:?}");
        assert_eq!(session.position_encoding(), encoding);
    }
}

#[test]
fn workspace_matching_preserves_raw_longest_segment_prefix_semantics() {
    let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
    let (parent_host, parent_config) = configured_host("parent", true);
    let (nested_host, nested_config) = configured_host("nested", false);
    let (file_root_host, file_root_config) = configured_host("file-root", true);
    let (empty_root_host, empty_root_config) = configured_host("empty-root", true);
    registry
        .configure_workspaces(vec![
            ("".into(), empty_root_host, empty_root_config),
            ("file:///".into(), file_root_host, file_root_config),
            ("file:///workspace".into(), parent_host, parent_config),
            (
                "file:///workspace/nested/".into(),
                nested_host,
                nested_config,
            ),
        ])
        .unwrap();

    let cases = [
        ("file:///workspace/nested", Some("file:///workspace/nested")),
        (
            "file:///workspace/nested/deep.md",
            Some("file:///workspace/nested"),
        ),
        ("file:///outside.md", Some("file:")),
        ("/outside.md", Some("")),
        (
            "file:///workspace/NESTED/case.md",
            Some("file:///workspace"),
        ),
        (
            "file:///workspace/%6Eested/encoded.md",
            Some("file:///workspace"),
        ),
        ("file:///workspace-other/sibling.md", Some("file:")),
        ("relative.md", None),
    ];
    for (uri, expected_workspace) in cases {
        open_uri(&mut registry, uri, "text", 1);
        let session = document(&registry, uri, 1);
        assert_eq!(session.workspace_uri(), expected_workspace);
    }

    let parent_fingerprint = render_fingerprint(
        &mut registry,
        "file:///workspace/NESTED/case.md",
        1,
        "parent-case",
    );
    let encoded_fingerprint = render_fingerprint(
        &mut registry,
        "file:///workspace/%6Eested/encoded.md",
        1,
        "parent-encoded",
    );
    let nested_fingerprint = render_fingerprint(
        &mut registry,
        "file:///workspace/nested/deep.md",
        1,
        "nested",
    );
    assert_eq!(encoded_fingerprint, parent_fingerprint);
    assert_ne!(nested_fingerprint, parent_fingerprint);
    // Security boundary: authority matching compares raw URI text. Case and percent-encoded
    // aliases therefore miss the untrusted nested root and inherit its trusted parent.
    // Normalization requires a separately reviewed behavior and policy change.
}

#[test]
fn workspace_reconfiguration_is_transactional() {
    let mut workspace = SessionRegistry::new(PositionEncoding::Utf8);
    let (old_host, old_config) = configured_host("old", true);
    workspace
        .configure_workspaces(vec![("file:///workspace".into(), old_host, old_config)])
        .unwrap();
    let original_uris = ["file:///workspace/alpha.md", "file:///workspace/beta.md"];
    open_uri(&mut workspace, original_uris[0], "alpha", 1);
    open_uri(&mut workspace, original_uris[1], "beta", 2);
    let daemon = workspace.daemon_instance_id().to_owned();
    let baseline = original_uris.map(|uri| {
        let session_id = session_id(&workspace, uri).to_owned();
        let version = if uri.ends_with("alpha.md") { 1 } else { 2 };
        let session = workspace.document(&daemon, &session_id, version).unwrap();
        (
            uri,
            session_id,
            session.source().to_owned(),
            session.document().document_version,
            render_fingerprint(&mut workspace, uri, version, &format!("before-{uri}")),
        )
    });
    let stage_order = workspace
        .index
        .iter()
        .filter(|(_, session)| session.workspace_uri() == Some("file:///workspace"))
        .map(|(uri, _)| uri.to_owned())
        .collect::<Vec<_>>();
    assert_eq!(stage_order.len(), 2);
    let failing_uri = stage_order.last().unwrap().clone();
    let mut oversized_assets = HashMap::new();
    oversized_assets.insert(failing_uri, assets_exceeding_total_limit());
    let (mismatched_host, mismatched_config) = configured_host("mismatched", true);
    let error = workspace
        .reconfigure_workspace(
            "file:///workspace",
            mismatched_host,
            mismatched_config,
            oversized_assets,
            &CancellationToken::default(),
        )
        .unwrap_err();
    assert_eq!(
        error,
        SessionError::Engine("invalid resolved render asset: resolved assets exceed 8 MiB".into())
    );
    for (uri, session_id, source, version, fingerprint) in &baseline {
        assert_eq!(workspace.session_id_for_uri(uri), Some(session_id.as_str()));
        let session = workspace
            .document(&daemon, session_id, *version as i64)
            .unwrap();
        assert_eq!(session.source(), source);
        assert_eq!(session.document().document_version, *version);
        assert_eq!(
            render_fingerprint(
                &mut workspace,
                uri,
                *version as i64,
                &format!("after-failure-{uri}"),
            ),
            *fingerprint
        );
    }

    let new_uri = "file:///workspace/after-failure.md";
    open_uri(&mut workspace, new_uri, "new", 3);
    let new_session_id = session_id(&workspace, new_uri).to_owned();
    assert_eq!(
        workspace
            .document(&daemon, &new_session_id, 3)
            .unwrap()
            .workspace_uri(),
        Some("file:///workspace")
    );
    assert_eq!(
        render_fingerprint(&mut workspace, new_uri, 3, "new-after-failure"),
        baseline[0].4
    );

    let (replacement_host, replacement_config) = configured_host("replacement", true);
    workspace
        .reconfigure_workspace(
            "file:///workspace",
            replacement_host,
            replacement_config,
            HashMap::new(),
            &CancellationToken::default(),
        )
        .unwrap();
    let all_documents = [
        (original_uris[0], baseline[0].1.as_str(), "alpha", 1),
        (original_uris[1], baseline[1].1.as_str(), "beta", 2),
        (new_uri, new_session_id.as_str(), "new", 3),
    ];
    let mut replacement_fingerprint = None;
    for (uri, session_id, source, version) in all_documents {
        assert_eq!(workspace.session_id_for_uri(uri), Some(session_id));
        let session = workspace.document(&daemon, session_id, version).unwrap();
        assert_eq!(session.source(), source);
        assert_eq!(session.document().document_version, version as u64);
        let fingerprint = render_fingerprint(
            &mut workspace,
            uri,
            version,
            &format!("after-success-{uri}"),
        );
        assert_ne!(fingerprint, baseline[0].4);
        if let Some(expected) = &replacement_fingerprint {
            assert_eq!(&fingerprint, expected);
        } else {
            replacement_fingerprint = Some(fingerprint);
        }
    }
}

#[test]
fn engine_error_categories_preserve_session_error_identity_and_messages() {
    let cases = [
        (EngineError::ContentModified, SessionError::ContentModified),
        (
            EngineError::StaleVersion {
                current: 3,
                received: 2,
            },
            SessionError::StaleVersion,
        ),
        (
            EngineError::Plugin("plugin detail".into()),
            SessionError::Engine("plugin pipeline failed: plugin detail".into()),
        ),
        (
            EngineError::UnsafeExportPolicy,
            SessionError::Engine(
                "unsafe plugin HTML is available only after a safe portable render".into(),
            ),
        ),
        (
            EngineError::Asset("asset detail".into()),
            SessionError::Engine("invalid resolved render asset: asset detail".into()),
        ),
    ];
    for (engine, expected) in cases {
        assert_eq!(engine_error(engine), expected);
    }
}
