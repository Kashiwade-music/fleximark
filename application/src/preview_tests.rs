//! Role: Verify preview framing, revisions, cache lifetime, and cancellation.
use crate::test_plugins::TestPlugins;
use crate::*;

fn opened() -> (SessionRegistry, String, String) {
    let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
    registry
        .open_with_cancellation(
            DidOpenParams {
                text_document: TextDocumentItem {
                    uri: "file:///preview.md".into(),
                    version: 1,
                    text: "# Heading\n".into(),
                },
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let session = registry
        .session_id_for_uri("file:///preview.md")
        .unwrap()
        .to_owned();
    let daemon = registry.daemon_instance_id().to_owned();
    (registry, daemon, session)
}

#[test]
fn publication_revisions_advance_only_on_successful_rendering() {
    let (mut registry, daemon, session) = opened();
    let first = registry
        .render_with_cancellation(&daemon, &session, 1, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(first.render_revision, 1);
    assert_eq!(first.preview_session_id.0, "one");
    assert_eq!(first.document_version, 1);
    assert!(first.html().contains("Heading"));
    assert_eq!(
        registry
            .read_preview(&daemon, &session, "one", None)
            .unwrap(),
        Some(first.clone())
    );
    assert!(
        registry
            .read_preview(&daemon, &session, "one", Some(1))
            .unwrap()
            .is_none()
    );
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(
        registry
            .render_with_cancellation(&daemon, &session, 1, "one", &cancelled)
            .is_err()
    );
    assert_eq!(
        registry
            .read_preview(&daemon, &session, "one", None)
            .unwrap(),
        Some(first)
    );
    let second = registry
        .render_with_cancellation(&daemon, &session, 1, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(second.render_revision, 2);
    let independent = registry
        .render_with_cancellation(&daemon, &session, 1, "two", &CancellationToken::default())
        .unwrap();
    assert_eq!(independent.render_revision, 1);
    assert_eq!(independent.blocks, second.blocks);
}

#[test]
fn navigation_checks_revision_and_document_version() {
    let (mut registry, daemon, session) = opened();
    let first = registry
        .render_with_cancellation(&daemon, &session, 1, "one", &CancellationToken::default())
        .unwrap();
    let node = &first.navigation[0].node_id;
    assert!(
        registry
            .navigate_preview(&daemon, &session, "one", 1, node)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        registry.navigate_preview(&daemon, &session, "one", 2, node),
        Err(SessionError::ContentModified)
    );
    registry
        .change_with_cancellation(
            DidChangeParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///preview.md".into(),
                    version: 2,
                },
                content_changes: vec![ContentChange {
                    range: None,
                    text: "# Edited\n".into(),
                }],
            },
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(
        registry.navigate_preview(&daemon, &session, "one", 1, node),
        Err(SessionError::ContentModified)
    );
    let updated = registry
        .render_with_cancellation(&daemon, &session, 2, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(updated.render_revision, 2);
    assert!(updated.html().contains("Edited"));
}

#[test]
fn disposal_and_document_close_evict_preview_caches() {
    let (mut registry, daemon, session) = opened();
    for id in ["one", "two"] {
        registry
            .render_with_cancellation(&daemon, &session, 1, id, &CancellationToken::default())
            .unwrap();
    }
    registry.dispose_preview(&daemon, &session, "one").unwrap();
    assert!(
        registry
            .read_preview(&daemon, &session, "one", None)
            .unwrap()
            .is_none()
    );
    assert!(
        registry
            .read_preview(&daemon, &session, "two", None)
            .unwrap()
            .is_some()
    );
    registry.close(DidCloseParams {
        text_document: TextDocumentIdentifier {
            uri: "file:///preview.md".into(),
        },
    });
    assert_eq!(
        registry.read_preview(&daemon, &session, "two", None),
        Err(SessionError::UnknownSession)
    );
}

#[test]
fn exhausted_revision_keeps_the_last_published_frame() {
    let (mut registry, daemon, session) = opened();
    registry
        .render_with_cancellation(&daemon, &session, 1, "one", &CancellationToken::default())
        .unwrap();
    registry
        .index
        .by_session_mut(&session)
        .unwrap()
        .previews
        .get_mut("one")
        .unwrap()
        .render_revision = u64::MAX;
    let previous = registry
        .read_preview(&daemon, &session, "one", None)
        .unwrap();
    assert_eq!(
        registry.render_with_cancellation(
            &daemon,
            &session,
            1,
            "one",
            &CancellationToken::default()
        ),
        Err(SessionError::RevisionExhausted)
    );
    assert_eq!(
        registry
            .read_preview(&daemon, &session, "one", None)
            .unwrap(),
        previous
    );
}

#[test]
fn reconfiguration_preserves_preview_identity_and_revision_sequence() {
    let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
    let initial: Arc<dyn PluginExecutor> = Arc::new(TestPlugins {
        calls: None,
        marker: None,
        hash: content_hash("initial"),
    });
    let config = RenderConfig::for_plugins(Default::default(), None, initial.as_ref());
    registry
        .configure_workspaces(vec![(
            crate::tests::workspace_uri().into(),
            initial,
            config,
        )])
        .unwrap();
    let uri = crate::tests::workspace_document_uri("reconfigure.md");
    registry
        .open_with_cancellation(
            DidOpenParams {
                text_document: TextDocumentItem {
                    uri: uri.into(),
                    version: 1,
                    text: "# Heading\n".into(),
                },
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let daemon = registry.daemon_instance_id().to_owned();
    let session = registry.session_id_for_uri(uri).unwrap().to_owned();
    let first = registry
        .render_with_cancellation(
            &daemon,
            &session,
            1,
            "preview",
            &CancellationToken::default(),
        )
        .unwrap();
    let replacement: Arc<dyn PluginExecutor> = Arc::new(TestPlugins {
        calls: None,
        marker: None,
        hash: content_hash("replacement"),
    });
    let config = RenderConfig::for_plugins(Default::default(), None, replacement.as_ref());
    registry
        .reconfigure_workspace(
            crate::tests::workspace_uri(),
            replacement,
            config,
            HashMap::new(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(registry.session_id_for_uri(uri), Some(session.as_str()));
    let second = registry
        .render_with_cancellation(
            &daemon,
            &session,
            1,
            "preview",
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(second.preview_session_id, first.preview_session_id);
    assert_eq!(second.render_revision, first.render_revision + 1);
    assert_ne!(second.renderer_fingerprint, first.renderer_fingerprint);
    assert_eq!(second.blocks, first.blocks);
}
