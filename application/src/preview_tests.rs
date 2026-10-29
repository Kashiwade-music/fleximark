//! Role: Verify preview framing, revisions, cache lifetime, and cancellation.
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
    let mut config = RenderConfig::default();
    config.config_hash = content_hash("initial");
    config.config_generation = 1;
    config.plugin_generation = 1;
    registry
        .configure_workspaces(vec![(crate::tests::workspace_uri().into(), config)])
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
    let mut config = RenderConfig::default();
    config.config_hash = content_hash("replacement");
    config.config_generation = 1;
    config.plugin_generation = 1;
    registry
        .reconfigure_workspace(
            crate::tests::workspace_uri(),
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

#[test]
fn cancelled_change_retains_source_document_frame_and_revision() {
    let (mut registry, daemon, session) = opened();
    let frame = registry
        .render_with_cancellation(&daemon, &session, 1, "one", &CancellationToken::default())
        .unwrap();
    let before = registry
        .document(&daemon, &session, 1)
        .unwrap()
        .document()
        .clone();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    let result = registry.change_with_cancellation(
        DidChangeParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: "file:///preview.md".into(),
                version: 2,
            },
            content_changes: vec![ContentChange {
                range: None,
                text: "# After".into(),
            }],
        },
        &cancelled,
    );
    assert_eq!(result, Err(SessionError::Cancelled));
    let retained = registry.index.by_uri("file:///preview.md").unwrap();
    assert_eq!(retained.source(), "# Heading\n");
    assert_eq!(retained.document(), &before);
    assert_eq!(retained.previews.get("one"), Some(&frame));
    assert_eq!(
        registry.document(&daemon, &session, 1).err(),
        Some(SessionError::ContentModified)
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
                    text: "# Heading\n".into(),
                }],
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let next = registry
        .render_with_cancellation(&daemon, &session, 2, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(next.blocks, frame.blocks);
    assert_eq!(next.render_revision, frame.render_revision + 1);
}

#[test]
fn cancelled_open_never_installs_a_document_session() {
    for stage in ["open_start", "open_commit"] {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
        let token = CancellationToken::default();
        if stage == "open_start" {
            token.cancel();
        } else {
            crate::cancellation_checks::cancel_at(stage, &token);
        }
        let result = registry.open_with_cancellation(
            DidOpenParams {
                text_document: TextDocumentItem {
                    uri: "file:///cancel-open.md".into(),
                    version: 1,
                    text: "# Source".into(),
                },
            },
            &token,
        );
        assert_eq!(result, Err(SessionError::Cancelled));
        assert!(registry.index.is_empty());
    }
}

#[test]
fn cancellation_before_change_commit_preserves_authoritative_state_and_preview() {
    for recovery in [false, true] {
        let (mut registry, daemon, session) = opened();
        let frame = registry
            .render_with_cancellation(&daemon, &session, 1, "one", &CancellationToken::default())
            .unwrap();
        let before = registry
            .index
            .by_uri("file:///preview.md")
            .unwrap()
            .document()
            .clone();
        if recovery {
            registry.mark_out_of_sync("file:///preview.md", "test recovery");
        }
        let token = CancellationToken::default();
        crate::cancellation_checks::cancel_at("change_commit", &token);
        let result = registry.change_with_cancellation(
            DidChangeParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: "file:///preview.md".into(),
                    version: 2,
                },
                content_changes: vec![ContentChange {
                    range: None,
                    text: "# Replacement".into(),
                }],
            },
            &token,
        );
        assert_eq!(result, Err(SessionError::Cancelled));
        let retained = registry.index.by_uri("file:///preview.md").unwrap();
        assert_eq!(retained.source(), "# Heading\n");
        assert_eq!(retained.document(), &before);
        assert_eq!(retained.previews.get("one"), Some(&frame));
        assert!(retained.engine.is_out_of_sync());
        assert_eq!(
            registry.document(&daemon, &session, 1).err(),
            Some(SessionError::ContentModified)
        );
    }
}

#[test]
fn cancellation_after_parse_and_before_reconfiguration_commit_discards_candidates() {
    for stage in ["after_parse", "reconfigure_commit"] {
        let mut registry = SessionRegistry::new(PositionEncoding::Utf8);
        let root = crate::tests::workspace_uri();
        registry
            .configure_workspaces(vec![(root.into(), RenderConfig::default())])
            .unwrap();
        let uri = crate::tests::workspace_document_uri("cancel-candidate.md");
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
        let frame = registry
            .render_with_cancellation(&daemon, &session, 1, "one", &CancellationToken::default())
            .unwrap();
        let before = registry.index.by_uri(uri).unwrap().document().clone();
        let config = registry
            .index
            .by_uri(uri)
            .unwrap()
            .engine
            .render_config()
            .clone();
        let token = CancellationToken::default();
        crate::cancellation_checks::cancel_at(stage, &token);
        let mut replacement = config.clone();
        replacement.config_hash = content_hash("changed");
        assert!(
            registry
                .reconfigure_workspace(root, replacement, HashMap::new(), &token)
                .is_err()
        );
        assert!(token.is_cancelled());
        let retained = registry.index.by_uri(uri).unwrap();
        assert_eq!(retained.document(), &before);
        assert_eq!(retained.engine.render_config(), &config);
        assert_eq!(retained.previews.get("one"), Some(&frame));
        assert_eq!(retained.source(), "# Heading\n");
    }
}
