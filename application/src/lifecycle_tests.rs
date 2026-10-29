//! Role: Verify worker lifetime isolation, root changes and safe asset memoization.
use crate::*;
use fleximark_protocol::{CommandContext, DocumentCommandContext, WorkspaceCommand};
use sha2::Digest;
use std::{fs, sync::Arc};
fn context(uri: &str) -> CommandContext {
    CommandContext {
        workspace_uri: Some(uri.into()),
        document: None,
    }
}
fn opened(
    label: &str,
    source: &str,
) -> (
    fleximark_test_support::TempWorkspace,
    String,
    DocumentStore,
    String,
) {
    let root = native::test_support::test_workspace(label);
    let uri = WorkspaceAuthority::path_to_file_uri(&root).unwrap();
    native::initialize_workspace(&uri).unwrap();
    fs::write(root.join("note.md"), source).unwrap();
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
                source: source.into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let id = store.session_id_for_uri(&document_uri).unwrap().to_owned();
    (root, uri, store, id)
}
fn note(name: &str) -> WorkspaceCommand {
    WorkspaceCommand::CreateNote {
        category_path: None,
        template: None,
        name: Some(name.into()),
    }
}

#[test]
fn queued_operations_are_cancelled_by_revocation_and_removal() {
    for remove in [false, true] {
        let (root, uri, mut store, _) = opened("queued-lease", "# Note");
        let prepared = WorkspaceOps::new(&mut store, None)
            .prepare(note("queued"), context(&uri), &CancellationToken::default())
            .unwrap();
        if remove {
            store.remove_workspace(&uri).unwrap();
        } else {
            store
                .reconfigure_native_workspace(&uri, false, &CancellationToken::default())
                .unwrap();
        }
        let completion = prepared.execute();
        assert!(matches!(
            WorkspaceOps::new(&mut store, None).complete(completion),
            Err(ApplicationError::Cancelled)
        ));
        assert!(!root.join("queued.md").exists());
        assert!(
            store
                .workspace_statuses()
                .iter()
                .all(|status| !status.trusted)
        );
    }
}
#[test]
fn finished_worker_cannot_restore_revoked_configuration() {
    let (_root, uri, mut store, _) = opened("finished-lease", "# Note");
    let prepared = WorkspaceOps::new(&mut store, None)
        .prepare(
            WorkspaceCommand::InitializeWorkspace {},
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    let completed = prepared.execute();
    store
        .reconfigure_native_workspace(&uri, false, &CancellationToken::default())
        .unwrap();
    assert!(matches!(
        WorkspaceOps::new(&mut store, None).complete(completed),
        Ok(OperationProgress::Complete(CommandOutcome::Initialized(_)))
    ));
    let status = store.workspace_statuses().remove(0);
    assert!(!status.trusted);
    assert!(matches!(status.state, WorkspaceState::Restricted));
}
#[test]
fn prepared_document_requests_reject_same_version_recovery_before_io() {
    let (_root, uri, mut store, id) = opened("prepared-source", "# Old");
    let command = WorkspaceCommand::CollectAdmonitions {};
    let mut ctx = context(&uri);
    ctx.document = Some(DocumentCommandContext {
        document_session_id: id,
        expected_document_version: 1.into(),
    });
    let prepared = WorkspaceOps::new(&mut store, None)
        .prepare(command, ctx, &CancellationToken::default())
        .unwrap();
    store.mark_out_of_sync(&format!("{uri}/note.md"));
    store
        .change(
            DocumentUpdate {
                uri: format!("{uri}/note.md"),
                version: 1,
                source: "# Different".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(matches!(
        WorkspaceOps::new(&mut store, None).complete(prepared.execute()),
        Err(ApplicationError::ContentModified)
    ));
}
fn resources() -> (
    fleximark_test_support::TempWorkspace,
    Arc<fleximark_preview_assets::PreviewAssets>,
) {
    let root = native::test_support::test_workspace("worker-resources");
    let spec: serde_json::Value = serde_json::from_str(include_str!(
        "../../contracts/distribution/distribution.json"
    ))
    .unwrap();
    let os = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        value => value,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        value => value,
    };
    let target = spec["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|target| target["os"] == os && target["arch"] == arch)
        .unwrap();
    let mut files = serde_json::Map::new();
    for file in spec["files"].as_array().unwrap() {
        let path = file["path"]
            .as_str()
            .unwrap()
            .replace("{exe}", target["executableSuffix"].as_str().unwrap());
        let destination = root.join(&path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(destination, "resource").unwrap();
        files.insert(
            path,
            serde_json::json!(format!("{:x}", sha2::Sha256::digest(b"resource"))),
        );
    }
    let metadata = include_str!("../../release.toml");
    let version = metadata
        .lines()
        .find_map(|line| line.strip_prefix("version = "))
        .unwrap()
        .trim_matches('"');
    fs::write(root.join("manifest.json"),serde_json::json!({"schemaVersion":1,"version":version,"target":target["triple"],"protocolVersion":fleximark_protocol::PROTOCOL_VERSION,"files":files}).to_string()).unwrap();
    let resources = Arc::new(fleximark_preview_assets::PreviewAssets::open(&root).unwrap());
    (root, resources)
}
#[test]
fn committed_export_is_successful_after_actor_source_changes() {
    let (root, uri, mut store, id) = opened("committed-export", "# Old snapshot");
    let (_runtime, resources) = resources();
    let mut ctx = context(&uri);
    ctx.document = Some(DocumentCommandContext {
        document_session_id: id.clone(),
        expected_document_version: 1.into(),
    });
    let prepared = WorkspaceOps::new(&mut store, Some(resources))
        .prepare(
            WorkspaceCommand::ExportHtml {
                destination_uri: Some(format!("{uri}/public")),
            },
            ctx,
            &CancellationToken::default(),
        )
        .unwrap();
    let completion = prepared.execute();
    store
        .change(
            DocumentUpdate {
                uri: format!("{uri}/note.md"),
                version: 2,
                source: "# Current source".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(matches!(
        WorkspaceOps::new(&mut store, None).complete(completion),
        Ok(OperationProgress::Complete(CommandOutcome::Exported(_)))
    ));
    assert!(
        fs::read_to_string(root.join("public/index.html"))
            .unwrap()
            .contains("Old snapshot")
    );
    assert_eq!(store.document(&id, 2).unwrap().source(), "# Current source");
}
#[test]
fn add_duplicate_trust_and_remove_keep_document_identity_and_frames() {
    let root = native::test_support::test_workspace("folder-lifecycle");
    let uri = WorkspaceAuthority::path_to_file_uri(&root).unwrap();
    native::initialize_workspace(&uri).unwrap();
    let document_uri = format!("{uri}/note.md");
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .open(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 1,
                source: "# Standalone".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let id = store.session_id_for_uri(&document_uri).unwrap().to_owned();
    let original = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    let added = store.add_workspace(uri.clone()).unwrap();
    assert!(!added.trusted);
    assert!(!original.is_current());
    assert_eq!(
        store.document(&id, 1).unwrap().workspace_uri(),
        Some(uri.as_str())
    );
    assert!(
        store
            .document(&id, 1)
            .unwrap()
            .engine
            .render_config()
            .style
            .is_none()
    );
    store
        .reconfigure_native_workspace(&uri, true, &CancellationToken::default())
        .unwrap();
    let trusted = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    assert!(trusted.style.is_some());
    assert!(store.add_workspace(uri.clone()).unwrap().trusted);
    assert!(trusted.is_current());
    store.remove_workspace(&uri).unwrap();
    assert!(!trusted.is_current());
    let detached = store.document(&id, 1).unwrap();
    assert!(detached.workspace_uri().is_none());
    assert!(detached.engine.render_config().style.is_none());
    assert_eq!(detached.source(), "# Standalone");
    assert_eq!(store.read_preview(&id, "one", None).unwrap(), Some(trusted));
}
#[test]
fn memo_reuses_bytes_and_hashes_but_tracks_references_metadata_and_scope() {
    let (root, uri, mut store, id) = opened("asset-memo", "![image](image.png)");
    let document_uri = format!("{uri}/note.md");
    let asset = root.join("image.png");
    fs::write(&asset, b"\x89PNG\r\n\x1a\npayload-a").unwrap();
    store.refresh_document_assets(&document_uri).unwrap();
    assert_eq!(store.index.by_session(&id).unwrap().asset_memo.reads(), 1);
    let hash = store
        .document(&id, 1)
        .unwrap()
        .engine
        .render_config()
        .assets()
        .next()
        .unwrap()
        .content_hash
        .clone();
    store
        .change(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 2,
                source: "# Text only\n\n![image](image.png)".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    store.refresh_document_assets(&document_uri).unwrap();
    assert_eq!(store.index.by_session(&id).unwrap().asset_memo.reads(), 1);
    fs::write(&asset, b"\x89PNG\r\n\x1a\npayload-b").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&asset)
        .unwrap()
        .set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1234567890),
        )
        .unwrap();
    store.refresh_document_assets(&document_uri).unwrap();
    assert_eq!(store.index.by_session(&id).unwrap().asset_memo.reads(), 2);
    assert_ne!(
        store
            .document(&id, 2)
            .unwrap()
            .engine
            .render_config()
            .assets()
            .next()
            .unwrap()
            .content_hash,
        hash
    );
    store
        .change(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 3,
                source: "# No assets".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    store.refresh_document_assets(&document_uri).unwrap();
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
    store
        .change(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 4,
                source: "![image](image.png)".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    store.refresh_document_assets(&document_uri).unwrap();
    assert_eq!(store.index.by_session(&id).unwrap().asset_memo.reads(), 3);
    store
        .reconfigure_native_workspace(&uri, true, &CancellationToken::default())
        .unwrap();
    store.refresh_document_assets(&document_uri).unwrap();
    assert_eq!(store.index.by_session(&id).unwrap().asset_memo.reads(), 4);
    store
        .reconfigure_native_workspace(&uri, false, &CancellationToken::default())
        .unwrap();
    store.refresh_document_assets(&document_uri).unwrap();
    assert_eq!(store.index.by_session(&id).unwrap().asset_memo.reads(), 4);
    assert_eq!(
        store
            .document(&id, 4)
            .unwrap()
            .engine
            .render_config()
            .assets()
            .len(),
        0
    );
}
#[test]
fn memo_hit_still_refuses_a_replaced_symbolic_link() {
    let (root, uri, mut store, id) = opened("memo-symlink", "![image](image.png)");
    let outside = native::test_support::test_workspace("memo-outside");
    let image = root.join("image.png");
    fs::write(&image, b"\x89PNG\r\n\x1a\ninside").unwrap();
    fs::write(outside.join("image.png"), b"\x89PNG\r\n\x1a\noutside").unwrap();
    let document_uri = format!("{uri}/note.md");
    store.refresh_document_assets(&document_uri).unwrap();
    fs::remove_file(&image).unwrap();
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(outside.join("image.png"), &image);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(outside.join("image.png"), &image);
    if linked.is_ok() {
        store.refresh_document_assets(&document_uri).unwrap();
        let session = store.document(&id, 1).unwrap();
        assert_eq!(session.engine.render_config().assets().len(), 0);
        assert_eq!(session.asset_memo.reads(), 1);
        assert!(session.asset_diagnostics().iter().any(|diagnostic|diagnostic.kind==fleximark_engine::AssetDiagnosticKind::Symlink));
    }
}

#[test]
fn completed_configuration_cannot_restore_a_removed_and_readded_root() {
    let (_root, uri, mut store, id) = opened("readded-worker", "# Note");
    let prepared = WorkspaceOps::new(&mut store, None)
        .prepare(
            WorkspaceCommand::InitializeWorkspace {},
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    let completion = prepared.execute();
    store.remove_workspace(&uri).unwrap();
    store.add_workspace(uri.clone()).unwrap();
    assert!(matches!(
        WorkspaceOps::new(&mut store, None).complete(completion),
        Ok(OperationProgress::Complete(CommandOutcome::Initialized(_)))
    ));
    let status = store.workspace_statuses().remove(0);
    assert!(!status.trusted);
    assert!(matches!(status.state, WorkspaceState::Restricted));
    assert!(
        store
            .document(&id, 1)
            .unwrap()
            .engine
            .render_config()
            .style
            .is_none()
    );
}
#[test]
fn queued_write_is_cancelled_by_configuration_generation_change() {
    let (root, uri, mut store, _) = opened("config-generation", "# Note");
    let prepared = WorkspaceOps::new(&mut store, None)
        .prepare(
            note("old-config"),
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    store
        .reconfigure_native_workspace(&uri, true, &CancellationToken::default())
        .unwrap();
    assert!(matches!(
        WorkspaceOps::new(&mut store, None).complete(prepared.execute()),
        Err(ApplicationError::Cancelled)
    ));
    assert!(!root.join("old-config.md").exists());
    assert!(store.workspace_statuses()[0].trusted);
}
#[test]
fn migration_plan_is_claimed_once_before_worker_execution() {
    let (_root, uri, mut store, _) = opened("claimed-plan", "# Note");
    let outcome = WorkspaceOps::new(&mut store, None)
        .execute(
            WorkspaceCommand::PlanMigration {
                settings_uri: None,
                recover: false,
            },
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    let plan_id = match outcome {
        CommandOutcome::MigrationPlanned(plan) => Some(plan.plan_id),
        _ => None,
    }
    .unwrap();
    let claimed = WorkspaceOps::new(&mut store, None)
        .prepare(
            WorkspaceCommand::ApplyMigration {
                plan_id: plan_id.clone(),
            },
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(
        WorkspaceOps::new(&mut store, None)
            .prepare(
                WorkspaceCommand::ApplyMigration { plan_id },
                context(&uri),
                &CancellationToken::default()
            )
            .is_err()
    );
    assert!(matches!(
        WorkspaceOps::new(&mut store, None).complete(claimed.execute()),
        Ok(OperationProgress::Complete(
            CommandOutcome::MigrationApplied(_)
        ))
    ));
}
#[test]
fn folder_removal_epoch_overflow_keeps_authority_and_preview() {
    let (_root, uri, mut store, id) = opened("folder-overflow", "# Note");
    store
        .index
        .by_session(&id)
        .unwrap()
        .authority
        .exhaust_epoch();
    let handle = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    assert!(matches!(
        store.remove_workspace(&uri),
        Err(ApplicationError::RevisionExhausted)
    ));
    assert_eq!(store.workspace_statuses().len(), 1);
    assert_eq!(
        store.document(&id, 1).unwrap().workspace_uri(),
        Some(uri.as_str())
    );
    assert!(handle.is_current());
}

#[test]
fn cancelled_revocation_invalidates_old_handles_but_keeps_restricted_preview_available() {
    let (_root, uri, mut store, id) = opened("cancelled-revocation-handle", "# Note");
    let before = store
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
    assert!(matches!(
        store.reconfigure_native_workspace(&uri, false, &token),
        Err(ApplicationError::Cancelled)
    ));
    assert!(!before.is_current());
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
    let restricted = store
        .render_with_cancellation(&id, 1, "one", &CancellationToken::default())
        .unwrap();
    assert!(restricted.is_current());
    assert!(restricted.style.is_none());
    assert_eq!(restricted.assets.len(), 0);
}

#[test]
fn configuration_completion_keeps_current_source_assets_when_asset_policy_matches() {
    let (root, uri, mut store, id) = opened("worker-current-assets", "# Old");
    fs::write(root.join("image.png"), b"\x89PNG\r\n\x1a\ncurrent").unwrap();
    let prepared = WorkspaceOps::new(&mut store, None)
        .prepare(
            WorkspaceCommand::InitializeWorkspace {},
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    let completion = prepared.execute();
    let document_uri = format!("{uri}/note.md");
    store
        .change(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 2,
                source: "![new image](image.png)".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    store.refresh_document_assets(&document_uri).unwrap();
    let reference = store
        .document(&id, 2)
        .unwrap()
        .engine
        .render_config()
        .assets()
        .next()
        .unwrap()
        .reference
        .clone();
    WorkspaceOps::new(&mut store, None)
        .complete(completion)
        .unwrap();
    let session = store.document(&id, 2).unwrap();
    assert_eq!(
        session
            .engine
            .render_config()
            .assets()
            .next()
            .unwrap()
            .reference,
        reference
    );
    assert_eq!(session.source(), "![new image](image.png)");
}
