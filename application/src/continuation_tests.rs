//! Role: Verify post-commit asset jobs converge without repeating native writes.
use crate::*;
use fleximark_protocol::{CommandContext, WorkspaceCommand};
use std::fs;
fn context(uri: &str) -> CommandContext {
    CommandContext {
        workspace_uri: Some(uri.into()),
        document: None,
    }
}
fn pending() -> (
    fleximark_test_support::TempWorkspace,
    String,
    DocumentStore,
    String,
    PreparedWorkspaceOperation,
) {
    let root = native::test_support::test_workspace("asset-continuation");
    let uri = WorkspaceAuthority::path_to_file_uri(&root).unwrap();
    native::initialize_workspace(&uri).unwrap();
    fs::create_dir(root.join("images")).unwrap();
    fs::write(root.join("image.png"), b"\x89PNG\r\n\x1a\nfirst").unwrap();
    fs::write(root.join("second.png"), b"\x89PNG\r\n\x1a\nsecond").unwrap();
    fs::write(root.join("note.md"), "# Old").unwrap();
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .configure_native_workspaces(vec![(uri.clone(), true)])
        .unwrap();
    let document_uri = format!("{uri}/note.md");
    store
        .open(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 1,
                source: "# Old".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let id = store.session_id_for_uri(&document_uri).unwrap().to_owned();
    store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    let prepared = WorkspaceOps::new(&mut store, None)
        .prepare(
            WorkspaceCommand::InitializeWorkspace {},
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    fs::write(
        root.join(".fleximark/config.toml"),
        "schema_version=1\n[assets]\nroots=['images']\n",
    )
    .unwrap();
    let completion = prepared.execute();
    store
        .change(
            DocumentUpdate {
                uri: document_uri,
                version: 2,
                source: "![first](image.png)".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let progress = WorkspaceOps::new(&mut store, None)
        .complete(completion)
        .unwrap();
    let continuation = match progress {
        OperationProgress::Continue(next) => Some(next),
        _ => None,
    }
    .unwrap();
    (root, uri, store, id, continuation)
}
fn completed(store: &mut DocumentStore, completion: OperationCompletion) -> CommandOutcome {
    match WorkspaceOps::new(store, None).complete(completion).unwrap() {
        OperationProgress::Complete(outcome) => Some(outcome),
        _ => None,
    }
    .unwrap()
}
#[test]
fn static_source_receives_assets_without_a_second_edit_or_native_replay() {
    let (root, _, mut store, id, continuation) = pending();
    let generation = store.config_generation;
    assert_eq!(
        store
            .document(&id, 2)
            .unwrap()
            .engine
            .render_config()
            .assets()
            .len(),
        0
    );
    // The committed initialize must not run again while resolving assets.
    fs::write(root.join(".fleximark/config.toml"), "broken = [").unwrap();
    assert!(matches!(
        completed(&mut store, continuation.execute()),
        CommandOutcome::Initialized(_)
    ));
    assert_eq!(store.config_generation, generation);
    assert_eq!(
        store
            .document(&id, 2)
            .unwrap()
            .engine
            .render_config()
            .assets()
            .len(),
        1
    );
    assert!(
        store
            .render_with_cancellation(&id, 2, "one", &CancellationToken::default())
            .unwrap()
            .html()
            .contains("first")
    );
}
#[test]
fn changed_source_requeues_latest_assets_without_applying_old_result() {
    let (_root, uri, mut store, id, first) = pending();
    let completion = first.execute();
    store
        .change(
            DocumentUpdate {
                uri: format!("{uri}/note.md"),
                version: 3,
                source: "![second](second.png)".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let progress = WorkspaceOps::new(&mut store, None)
        .complete(completion)
        .unwrap();
    let next = match progress {
        OperationProgress::Continue(next) => Some(next),
        _ => None,
    }
    .unwrap();
    assert_eq!(
        store
            .document(&id, 3)
            .unwrap()
            .engine
            .render_config()
            .assets()
            .len(),
        0
    );
    assert!(matches!(
        completed(&mut store, next.execute()),
        CommandOutcome::Initialized(_)
    ));
    let session = store.document(&id, 3).unwrap();
    assert_eq!(session.source(), "![second](second.png)");
    assert_eq!(session.engine.render_config().assets().len(), 1);
    assert_eq!(
        session
            .engine
            .render_config()
            .context
            .resolved_resources
            .len(),
        1
    );
    assert!(
        session
            .engine
            .render_config()
            .context
            .resolved_resources
            .contains_key("second.png")
    );
}
#[test]
fn removed_or_revoked_scopes_discard_asset_results_and_keep_committed_success() {
    for remove in [false, true] {
        let (_root, uri, mut store, id, next) = pending();
        let completion = next.execute();
        if remove {
            store.remove_workspace(&uri).unwrap();
        } else {
            store
                .reconfigure_native_workspace(&uri, false, &CancellationToken::default())
                .unwrap();
        }
        assert!(matches!(
            completed(&mut store, completion),
            CommandOutcome::Initialized(_)
        ));
        let session = store.document(&id, 2).unwrap();
        assert_eq!(session.engine.render_config().assets().len(), 0);
        assert!(session.engine.render_config().style.is_none());
        if remove {
            assert!(session.workspace_uri().is_none());
        }
        assert!(
            store
                .workspace_statuses()
                .iter()
                .all(|status| !status.trusted)
        );
    }
}
#[test]
fn explicit_cancellation_does_not_turn_a_committed_operation_into_an_error() {
    let root = native::test_support::test_workspace("cancel-continuation");
    let uri = WorkspaceAuthority::path_to_file_uri(&root).unwrap();
    native::initialize_workspace(&uri).unwrap();
    fs::create_dir(root.join("images")).unwrap();
    fs::write(root.join("note.md"), "# Old").unwrap();
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .configure_native_workspaces(vec![(uri.clone(), true)])
        .unwrap();
    let document_uri = format!("{uri}/note.md");
    store
        .open(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 1,
                source: "# Old".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let id = store.session_id_for_uri(&document_uri).unwrap().to_owned();
    let frame = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    let token = CancellationToken::default();
    let prepared = WorkspaceOps::new(&mut store, None)
        .prepare(
            WorkspaceCommand::InitializeWorkspace {},
            context(&uri),
            &token,
        )
        .unwrap();
    fs::write(
        root.join(".fleximark/config.toml"),
        "schema_version=1\n[assets]\nroots=['images']",
    )
    .unwrap();
    let completion = prepared.execute();
    store
        .change(
            DocumentUpdate {
                uri: document_uri,
                version: 2,
                source: "# New".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let next = match WorkspaceOps::new(&mut store, None)
        .complete(completion)
        .unwrap()
    {
        OperationProgress::Continue(next) => Some(next),
        _ => None,
    }
    .unwrap();
    let config = store
        .document(&id, 2)
        .unwrap()
        .engine
        .render_config()
        .clone();
    token.cancel();
    assert!(matches!(
        completed(&mut store, next.execute()),
        CommandOutcome::Initialized(_)
    ));
    assert_eq!(
        store.document(&id, 2).unwrap().engine.render_config(),
        &config
    );
    assert_eq!(store.read_preview(&id, "one", None).unwrap(), Some(frame));
}
#[test]
fn authority_overflow_preserves_engine_and_frame_after_a_native_commit() {
    let root = native::test_support::test_workspace("committed-overflow");
    let uri = WorkspaceAuthority::path_to_file_uri(&root).unwrap();
    native::initialize_workspace(&uri).unwrap();
    fs::write(root.join("note.md"), "# Note").unwrap();
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .configure_native_workspaces(vec![(uri.clone(), true)])
        .unwrap();
    let document_uri = format!("{uri}/note.md");
    store
        .open(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 1,
                source: "# Note".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let id = store.session_id_for_uri(&document_uri).unwrap().to_owned();
    store
        .index
        .by_session(&id)
        .unwrap()
        .authority
        .exhaust_epoch();
    let frame = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    let config = store
        .document(&id, 1)
        .unwrap()
        .engine
        .render_config()
        .clone();
    let prepared = WorkspaceOps::new(&mut store, None)
        .prepare(
            WorkspaceCommand::InitializeWorkspace {},
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(matches!(
        completed(&mut store, prepared.execute()),
        CommandOutcome::Initialized(_)
    ));
    assert_eq!(
        store.document(&id, 1).unwrap().engine.render_config(),
        &config
    );
    assert!(frame.is_current());
    assert_eq!(store.read_preview(&id, "one", None).unwrap(), Some(frame));
    assert!(matches!(
        store.workspace_statuses()[0].state,
        WorkspaceState::Invalid(_)
    ));
}
