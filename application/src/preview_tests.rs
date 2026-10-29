//! Role: Verify preview framing, revisions, cache lifetime, and cancellation.
use crate::*;

fn opened() -> (DocumentStore, String) {
    let mut registry = DocumentStore::new(PositionEncoding::Utf8);
    registry
        .open(
            DocumentUpdate {
                uri: "file:///preview.md".into(),
                version: 1,
                source: "# Heading\n".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let session = registry
        .session_id_for_uri("file:///preview.md")
        .unwrap()
        .to_owned();
    (registry, session)
}

#[test]
fn publication_revisions_advance_only_on_successful_rendering() {
    let (mut registry, session) = opened();
    let first = registry
        .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(first.render_revision, 1);
    assert_eq!(first.preview_session_id.0, "one");
    assert_eq!(first.document_version, 1);
    assert!(first.html().contains("Heading"));
    assert_eq!(
        registry.read_preview(&session, "one", None).unwrap(),
        Some(first.clone())
    );
    assert!(
        registry
            .read_preview(&session, "one", Some(1))
            .unwrap()
            .is_none()
    );
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(
        registry
            .render_with_cancellation(&session, 1, "one", &cancelled)
            .is_err()
    );
    assert_eq!(
        registry.read_preview(&session, "one", None).unwrap(),
        Some(first)
    );
    let second = registry
        .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(second.render_revision, 2);
    let independent = registry
        .render_with_cancellation(&session, 1, "two", &CancellationToken::default())
        .unwrap();
    assert_eq!(independent.render_revision, 1);
    assert_eq!(independent.blocks, second.blocks);
}

#[test]
fn navigation_checks_revision_and_document_version() {
    let (mut registry, session) = opened();
    let first = registry
        .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
        .unwrap();
    let node = &first.navigation[0].node_id;
    assert!(
        registry
            .navigate_preview(&session, "one", 1, node)
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        registry.navigate_preview(&session, "one", 2, node),
        Err(ApplicationError::ContentModified)
    ));
    registry
        .change(
            DocumentUpdate {
                uri: "file:///preview.md".into(),
                version: 2,
                source: "# Edited\n".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(matches!(
        registry.navigate_preview(&session, "one", 1, node),
        Err(ApplicationError::ContentModified)
    ));
    let updated = registry
        .render_with_cancellation(&session, 2, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(updated.render_revision, 2);
    assert!(updated.html().contains("Edited"));
}

#[test]
fn disposal_and_document_close_evict_preview_caches() {
    let (mut registry, session) = opened();
    for id in ["one", "two"] {
        registry
            .render_with_cancellation(&session, 1, id, &CancellationToken::default())
            .unwrap();
    }
    registry.dispose_preview(&session, "one").unwrap();
    assert!(
        registry
            .read_preview(&session, "one", None)
            .unwrap()
            .is_none()
    );
    assert!(
        registry
            .read_preview(&session, "two", None)
            .unwrap()
            .is_some()
    );
    registry.close("file:///preview.md");
    assert!(matches!(
        registry.read_preview(&session, "two", None),
        Err(ApplicationError::UnknownSession)
    ));
}

#[test]
fn exhausted_revision_keeps_the_last_published_frame() {
    let (mut registry, session) = opened();
    registry
        .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
        .unwrap();
    let handle = registry
        .index
        .by_session_mut(&session)
        .unwrap()
        .previews
        .get_mut("one")
        .unwrap();
    std::sync::Arc::make_mut(&mut handle.frame).render_revision = u64::MAX;
    let previous = registry.read_preview(&session, "one", None).unwrap();
    assert!(matches!(
        registry.render_with_cancellation(&session, 1, "one", &CancellationToken::default()),
        Err(ApplicationError::RevisionExhausted)
    ));
    assert_eq!(
        registry.read_preview(&session, "one", None).unwrap(),
        previous
    );
}

#[test]
fn reconfiguration_preserves_preview_identity_and_revision_sequence() {
    let mut registry = DocumentStore::new(PositionEncoding::Utf8);
    let mut config = RenderConfig::default();
    config.config_hash = content_hash("initial");
    config.config_generation = 1;
    registry
        .configure_workspaces(vec![(crate::tests::workspace_uri().into(), config)])
        .unwrap();
    let uri = crate::tests::workspace_document_uri("reconfigure.md");
    registry
        .open(
            DocumentUpdate {
                uri: uri.into(),
                version: 1,
                source: "# Heading\n".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let session = registry.session_id_for_uri(uri).unwrap().to_owned();
    let first = registry
        .render_with_cancellation(&session, 1, "preview", &CancellationToken::default())
        .unwrap();
    let mut config = RenderConfig::default();
    config.config_hash = content_hash("replacement");
    config.config_generation = 1;
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
        .render_with_cancellation(&session, 1, "preview", &CancellationToken::default())
        .unwrap();
    assert_eq!(second.preview_session_id, first.preview_session_id);
    assert_eq!(second.render_revision, first.render_revision + 1);
    assert_ne!(second.renderer_fingerprint, first.renderer_fingerprint);
    assert_eq!(second.blocks, first.blocks);
}

#[test]
fn cancelled_change_retains_source_document_frame_and_revision() {
    let (mut registry, session) = opened();
    let frame = registry
        .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
        .unwrap();
    let before = registry.document(&session, 1).unwrap().document().clone();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    let result = registry.change(
        DocumentUpdate {
            uri: "file:///preview.md".into(),
            version: 2,
            source: "# After".into(),
        },
        &cancelled,
    );
    assert!(matches!(result, Err(ApplicationError::Cancelled)));
    let retained = registry.index.by_uri("file:///preview.md").unwrap();
    assert_eq!(retained.source(), "# Heading\n");
    assert_eq!(retained.document(), &before);
    assert_eq!(retained.previews.get("one"), Some(&frame));
    assert!(matches!(
        registry.document(&session, 1).err(),
        Some(ApplicationError::ContentModified)
    ));
    registry
        .change(
            DocumentUpdate {
                uri: "file:///preview.md".into(),
                version: 2,
                source: "# Heading\n".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let next = registry
        .render_with_cancellation(&session, 2, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(next.blocks, frame.blocks);
    assert_eq!(next.render_revision, frame.render_revision + 1);
}

#[test]
fn cancelled_open_never_installs_a_document_session() {
    for stage in ["open_start", "open_commit"] {
        let mut registry = DocumentStore::new(PositionEncoding::Utf8);
        let token = CancellationToken::default();
        if stage == "open_start" {
            token.cancel();
        } else {
            crate::cancellation_checks::cancel_at(stage, &token);
        }
        let result = registry.open(
            DocumentUpdate {
                uri: "file:///cancel-open.md".into(),
                version: 1,
                source: "# Source".into(),
            },
            &token,
        );
        assert!(matches!(result, Err(ApplicationError::Cancelled)));
        assert!(registry.index.is_empty());
    }
}

#[test]
fn cancellation_before_change_commit_preserves_authoritative_state_and_preview() {
    for (recovery, stage) in [
        (false, "change_candidate"),
        (true, "change_candidate"),
        (false, "change_commit"),
        (true, "change_commit"),
    ] {
        let (mut registry, session) = opened();
        let frame = registry
            .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
            .unwrap();
        let before = registry
            .index
            .by_uri("file:///preview.md")
            .unwrap()
            .document()
            .clone();
        if recovery {
            registry.mark_out_of_sync("file:///preview.md");
        }
        let token = CancellationToken::default();
        crate::cancellation_checks::cancel_at(stage, &token);
        let result = registry.change(
            DocumentUpdate {
                uri: "file:///preview.md".into(),
                version: 2,
                source: "# Replacement".into(),
            },
            &token,
        );
        assert!(matches!(result, Err(ApplicationError::Cancelled)));
        let retained = registry.index.by_uri("file:///preview.md").unwrap();
        assert_eq!(retained.source(), "# Heading\n");
        assert_eq!(retained.document(), &before);
        assert_eq!(retained.previews.get("one"), Some(&frame));
        assert!(retained.engine.is_out_of_sync());
        assert!(matches!(
            registry.document(&session, 1).err(),
            Some(ApplicationError::ContentModified)
        ));
    }
}

#[test]
fn cancelled_reconfiguration_discards_configuration_candidates() {
    for stage in ["configuration_prepared", "reconfigure_commit"] {
        let mut registry = DocumentStore::new(PositionEncoding::Utf8);
        let root = crate::tests::workspace_uri();
        registry
            .configure_workspaces(vec![(root.into(), RenderConfig::default())])
            .unwrap();
        let uri = crate::tests::workspace_document_uri("cancel-candidate.md");
        registry
            .open(
                DocumentUpdate {
                    uri: uri.into(),
                    version: 1,
                    source: "# Heading\n".into(),
                },
                &CancellationToken::default(),
            )
            .unwrap();
        let session = registry.session_id_for_uri(uri).unwrap().to_owned();
        let frame = registry
            .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
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

#[test]
fn cancellation_after_render_keeps_the_published_frame_and_revision() {
    let (mut registry, session) = opened();
    let previous = registry
        .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
        .unwrap();
    let token = CancellationToken::default();
    crate::cancellation_checks::cancel_at("render_commit", &token);
    assert!(matches!(
        registry.render_with_cancellation(&session, 1, "one", &token),
        Err(ApplicationError::Cancelled)
    ));
    assert_eq!(
        registry.read_preview(&session, "one", None).unwrap(),
        Some(previous.clone())
    );
    let next = registry
        .render_with_cancellation(&session, 1, "one", &CancellationToken::default())
        .unwrap();
    assert_eq!(next.render_revision, previous.render_revision + 1);
    assert_eq!(next.blocks, previous.blocks);
}

#[test]
fn retained_handles_are_shared_and_reject_source_recovery_close_and_disposal() {
    let (mut store, id) = opened();
    let first = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    let read = store.read_preview(&id, "one", None).unwrap().unwrap();
    assert!(std::sync::Arc::ptr_eq(first.frame_arc(), read.frame_arc()));
    assert!(first.is_current());
    store.mark_out_of_sync("file:///preview.md");
    assert!(!first.is_current());
    store
        .change(
            DocumentUpdate {
                uri: "file:///preview.md".into(),
                version: 1,
                source: "# New same version".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(!first.is_current());
    let current = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    assert!(current.is_current());
    assert!(!first.is_current());
    store.dispose_preview(&id, "one").unwrap();
    assert!(!current.is_current());
    let current = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    store.close("file:///preview.md");
    assert!(!current.is_current());
}
#[test]
fn cancelled_render_never_revalidates_the_previous_handle() {
    let (mut store, id) = opened();
    let first = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    let epoch = store
        .index
        .by_session(&id)
        .unwrap()
        .authority
        .state()
        .unwrap()
        .source_epoch;
    let token = CancellationToken::default();
    token.cancel();
    assert!(
        store
            .render_with_cancellation(&id, 1, "one", &token)
            .is_err()
    );
    assert!(!first.is_current());
    assert_eq!(
        store
            .index
            .by_session(&id)
            .unwrap()
            .authority
            .state()
            .unwrap()
            .source_epoch,
        epoch
    );
    let next = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    assert!(next.is_current());
    assert!(!first.is_current());
}
#[test]
fn exhausted_source_epoch_does_not_commit_source_or_frame() {
    let (mut store, id) = opened();
    store
        .index
        .by_session(&id)
        .unwrap()
        .authority
        .exhaust_epoch();
    let first = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    assert!(matches!(
        store.change(
            DocumentUpdate {
                uri: "file:///preview.md".into(),
                version: 2,
                source: "# Rejected".into()
            },
            &CancellationToken::default()
        ),
        Err(ApplicationError::RevisionExhausted)
    ));
    assert_eq!(
        store.source_for_uri("file:///preview.md").unwrap(),
        "# Heading\n"
    );
    assert!(first.is_current());
    assert_eq!(store.read_preview(&id, "one", None).unwrap(), Some(first));
}
