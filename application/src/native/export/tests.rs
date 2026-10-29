//! Role: Verify workspace operations and security invariants.
use super::filesystem::object_identity;
use super::filesystem::read_json_regular;
use super::journal::read_journal;
use super::journal::{append_journal, create_journal};
use super::model::RecordedObject;
use super::model::{ExportJournal, ExportJournalState, OwnershipMarker, RegistryRecord};
use super::*;
use crate::native::assets::resolve_export_assets;
use crate::native::integrity::ObjectIdentity;
use crate::native::test_support::test_workspace;
use crate::native::{initialize_workspace, path_to_file_uri};
use fleximark_engine::ResolvedRenderAsset;
use std::fs;

#[test]
fn export_resolves_and_manifests_only_contained_assets() {
    let root = test_workspace("export-assets-test");
    let workspace_uri = path_to_file_uri(&root).unwrap();
    initialize_workspace(&workspace_uri).unwrap();
    let source = root.join("doc.md");
    let asset = root.join("image.png");
    fs::write(&source, "![image](image.png)\n").unwrap();
    let png = b"\x89PNG\r\n\x1a\nasset";
    fs::write(&asset, png).unwrap();
    let source_uri = path_to_file_uri(&source).unwrap();
    let render_asset =
        ResolvedRenderAsset::from_validated_bytes("image.png".into(), "image/png".into(), png)
            .unwrap()
            .published()
            .clone();
    let resolved = resolve_export_assets(
        &source_uri,
        &workspace_uri,
        &format!("<img src=\"{}\">", render_asset.reference),
        &[render_asset],
    )
    .unwrap();
    assert_eq!(resolved.assets.len(), 1);
    assert!(resolved.html.contains("src=\"assets/"));
    let destination = root.join("public");
    let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
    preflight_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        &resolved.html,
        &resolved.assets,
    )
    .unwrap();
    let marker: OwnershipMarker =
        read_json_regular(&destination.join(".fleximark-export.json")).unwrap();
    assert!(
        marker
            .payload
            .files
            .iter()
            .any(|file| file.path == "assets")
    );
    let exported_asset = marker
        .payload
        .files
        .iter()
        .find(|file| file.path.starts_with("assets/") && file.kind == "file")
        .unwrap();
    assert_eq!(
        fs::read(destination.join(&exported_asset.path)).unwrap(),
        png
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn export_requires_ownership_and_preserves_user_files() {
    let root = test_workspace("export-test");
    let workspace_uri = path_to_file_uri(&root).unwrap();
    initialize_workspace(&workspace_uri).unwrap();
    let source = root.join("doc.md");
    fs::write(&source, "# hello\n").unwrap();
    let source_uri = path_to_file_uri(&source).unwrap();
    let destination = root.join("public");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("user.txt"), "keep").unwrap();
    let destination_uri = path_to_file_uri(&destination).unwrap();
    assert!(matches!(
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>first</p>",
            &[],
        ),
        Err(ApplicationError::UnmanagedExport)
    ));
    fs::remove_dir(&destination).unwrap_err();
    fs::remove_file(destination.join("user.txt")).unwrap();
    fs::remove_dir(&destination).unwrap();

    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>first</p>",
        &[],
    )
    .unwrap();
    acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
    fs::write(destination.join("user.txt"), "keep").unwrap();
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>second</p>",
        &[],
    )
    .unwrap();
    let journal = root.join(".public.fleximark-export-journal.json");
    assert!(
        journal.exists(),
        "committed journal is retained for verification"
    );
    assert!(fs::read_dir(&root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".public.fleximark-export-backup-")
    }));
    assert!(matches!(
        preflight_export(&source_uri, &workspace_uri, &destination_uri),
        Err(ApplicationError::ExportAwaitingAcknowledgement)
    ));
    acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
    assert!(!journal.exists());
    assert!(!fs::read_dir(&root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".public.fleximark-export-backup-")
    }));
    assert_eq!(
        fs::read_to_string(destination.join("user.txt")).unwrap(),
        "keep"
    );
    assert_eq!(
        fs::read_to_string(destination.join("index.html")).unwrap(),
        "<p>second</p>"
    );
    let marker: OwnershipMarker =
        read_json_regular(&destination.join(".fleximark-export.json")).unwrap();
    assert_eq!(marker.payload.generation, 2);

    fs::write(destination.join("index.html"), "tampered").unwrap();
    assert!(matches!(
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>third</p>",
            &[],
        ),
        Err(ApplicationError::ExportContentConflict)
    ));
    assert_eq!(
        fs::read_to_string(destination.join("index.html")).unwrap(),
        "tampered"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn export_ownership_ids_are_random_and_cannot_be_copied_to_another_destination() {
    let root = test_workspace("export-random-ownership-test");
    let workspace_uri = path_to_file_uri(&root).unwrap();
    initialize_workspace(&workspace_uri).unwrap();
    let source = root.join("doc.md");
    fs::write(&source, "# export\n").unwrap();
    let source_uri = path_to_file_uri(&source).unwrap();
    let first = root.join("first");
    let second = root.join("second");
    let first_uri = path_to_file_uri_unchecked(&first).unwrap();
    let second_uri = path_to_file_uri_unchecked(&second).unwrap();

    export_html(&source_uri, &workspace_uri, &first_uri, "<p>first</p>", &[]).unwrap();
    acknowledge_export(&source_uri, &workspace_uri, &first_uri).unwrap();
    export_html(
        &source_uri,
        &workspace_uri,
        &second_uri,
        "<p>second</p>",
        &[],
    )
    .unwrap();
    acknowledge_export(&source_uri, &workspace_uri, &second_uri).unwrap();
    let first_marker: OwnershipMarker =
        read_json_regular(&first.join(".fleximark-export.json")).unwrap();
    let second_marker: OwnershipMarker =
        read_json_regular(&second.join(".fleximark-export.json")).unwrap();
    assert_ne!(
        first_marker.payload.destination_id,
        second_marker.payload.destination_id
    );
    assert_eq!(first_marker.payload.destination_id.len(), 32);

    fs::write(
        second.join(".fleximark-export.json"),
        serde_json::to_vec(&first_marker).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        preflight_export(&source_uri, &workspace_uri, &second_uri),
        Err(ApplicationError::ExportOwnershipConflict)
            | Err(ApplicationError::ExportContentConflict)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn acknowledged_export_rejects_a_swapped_destination_directory() {
    let root = test_workspace("export-directory-swap-test");
    let workspace_uri = path_to_file_uri(&root).unwrap();
    initialize_workspace(&workspace_uri).unwrap();
    let source = root.join("doc.md");
    fs::write(&source, "# export\n").unwrap();
    let source_uri = path_to_file_uri(&source).unwrap();
    let destination = root.join("public");
    let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>original</p>",
        &[],
    )
    .unwrap();
    acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();

    let original = root.join("original-public");
    fs::rename(&destination, &original).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::copy(original.join("index.html"), destination.join("index.html")).unwrap();
    fs::copy(
        original.join(".fleximark-export.json"),
        destination.join(".fleximark-export.json"),
    )
    .unwrap();
    assert!(matches!(
        preflight_export(&source_uri, &workspace_uri, &destination_uri),
        Err(ApplicationError::ExportOwnershipConflict)
    ));
    assert_eq!(
        fs::read_to_string(original.join("index.html")).unwrap(),
        "<p>original</p>"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn old_moved_journal_recovers_the_previous_destination() {
    let root = test_workspace("recovery-test");
    let destination = root.join("public");
    let staging = root.join(".public.fleximark-export-staging-test");
    let backup = root.join(".public.fleximark-export-backup-test");
    let journal_path = root.join(".public.fleximark-export-journal.json");
    fs::create_dir(&staging).unwrap();
    fs::create_dir(&backup).unwrap();
    fs::write(backup.join("old.txt"), "old").unwrap();
    let record = RegistryRecord {
        destination_id: "id".into(),
        generation: 2,
        digest: "digest".into(),
        source_identity: "source".into(),
        workspace_identity: "workspace".into(),
        destination_identity: destination.to_string_lossy().into_owned(),
        destination_object_identity: object_identity(&staging).unwrap(),
    };
    let staging_identity = object_identity(&staging).unwrap();
    let backup_identity = object_identity(&backup).unwrap();
    let journal_identity = create_journal(&journal_path).unwrap();
    let mut journal = ExportJournal {
        transaction_id: "test".into(),
        state: ExportJournalState::Prepared,
        destination: destination.clone(),
        staging: staging.clone(),
        backup: backup.clone(),
        registry: root.join("registry.json"),
        journal_identity,
        registry_staging: None,
        registry_staging_identity: None,
        previous_registry: None,
        next_registry: record,
        staging_identity,
        staging_marker_identity: ObjectIdentity {
            platform_id: "unused".into(),
            kind: "file".into(),
        },
        generated_identities: Vec::new(),
        previous_destination_objects: vec![RecordedObject {
            relative_path: "old.txt".into(),
            identity: object_identity(&backup.join("old.txt")).unwrap(),
            content_hash: Some(sha256(b"old")),
        }],
        previous_destination_identity: Some(backup_identity.clone()),
        previous_registry_identity: None,
        backup_identity: None,
        installed_identity: None,
        installed_marker_identity: None,
        next_registry_identity: None,
    };
    let digest = append_journal(&journal_path, &journal, None).unwrap();
    journal.state = ExportJournalState::OldMoveIntent;
    let digest = append_journal(&journal_path, &journal, Some(digest)).unwrap();
    journal.state = ExportJournalState::OldMoved;
    journal.backup_identity = Some(backup_identity);
    append_journal(&journal_path, &journal, Some(digest)).unwrap();
    recover_export(
        &journal_path,
        &destination,
        &root.join("registry.json"),
        false,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(destination.join("old.txt")).unwrap(),
        "old"
    );
    assert!(!staging.exists() && !journal_path.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn journal_records_are_append_only_and_digest_chained() {
    let root = test_workspace("journal-digest-test");
    let path = root.join("journal.json");
    let registry = RegistryRecord {
        destination_id: "id".into(),
        generation: 1,
        digest: "digest".into(),
        source_identity: "source".into(),
        workspace_identity: "workspace".into(),
        destination_identity: "destination".into(),
        destination_object_identity: ObjectIdentity {
            platform_id: "destination-object".into(),
            kind: "directory".into(),
        },
    };
    let journal_identity = create_journal(&path).unwrap();
    let mut journal = ExportJournal {
        transaction_id: "transaction".into(),
        state: ExportJournalState::Prepared,
        destination: root.join("output"),
        staging: root.join("staging"),
        backup: root.join("backup"),
        registry: root.join("registry"),
        journal_identity,
        registry_staging: None,
        registry_staging_identity: None,
        previous_registry: None,
        next_registry: registry,
        staging_identity: ObjectIdentity {
            platform_id: "staging".into(),
            kind: "directory".into(),
        },
        staging_marker_identity: ObjectIdentity {
            platform_id: "marker".into(),
            kind: "file".into(),
        },
        generated_identities: Vec::new(),
        previous_destination_objects: Vec::new(),
        previous_destination_identity: None,
        previous_registry_identity: None,
        backup_identity: None,
        installed_identity: None,
        installed_marker_identity: None,
        next_registry_identity: None,
    };
    let first = append_journal(&path, &journal, None).unwrap();
    journal.state = ExportJournalState::OldMoveIntent;
    append_journal(&path, &journal, Some(first)).unwrap();
    assert_eq!(
        read_journal(&path).unwrap().0.state,
        ExportJournalState::OldMoveIntent
    );
    let tampered = fs::read_to_string(&path)
        .unwrap()
        .replacen("transaction", "forged", 1);
    fs::write(&path, tampered).unwrap();
    assert!(matches!(
        read_journal(&path),
        Err(ApplicationError::ExportRecoveryConflict)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recovery_refuses_swapped_staging_identity_without_deleting_it() {
    let root = test_workspace("journal-swap-test");
    let staging = root.join(".output.fleximark-export-staging-test");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join(".fleximark-export.json"), "original").unwrap();
    let journal_path = root.join(".output.fleximark-export-journal.json");
    let journal_identity = create_journal(&journal_path).unwrap();
    let journal = ExportJournal {
        transaction_id: "transaction".into(),
        state: ExportJournalState::Prepared,
        destination: root.join("output"),
        staging: staging.clone(),
        backup: root.join(".output.fleximark-export-backup-test"),
        registry: root.join("registry.json"),
        journal_identity,
        registry_staging: None,
        registry_staging_identity: None,
        previous_registry: None,
        next_registry: RegistryRecord {
            destination_id: "id".into(),
            generation: 1,
            digest: "digest".into(),
            source_identity: "source".into(),
            workspace_identity: "workspace".into(),
            destination_identity: "destination".into(),
            destination_object_identity: ObjectIdentity {
                platform_id: "destination-object".into(),
                kind: "directory".into(),
            },
        },
        staging_identity: object_identity(&staging).unwrap(),
        staging_marker_identity: object_identity(&staging.join(".fleximark-export.json")).unwrap(),
        generated_identities: Vec::new(),
        previous_destination_objects: Vec::new(),
        previous_destination_identity: None,
        previous_registry_identity: None,
        backup_identity: None,
        installed_identity: None,
        installed_marker_identity: None,
        next_registry_identity: None,
    };
    append_journal(&journal_path, &journal, None).unwrap();
    fs::remove_dir_all(&staging).unwrap();
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join(".fleximark-export.json"), "replacement").unwrap();
    assert!(matches!(
        recover_export(
            &journal_path,
            &root.join("output"),
            &root.join("registry.json"),
            false
        ),
        Err(ApplicationError::ExportRecoveryConflict)
    ));
    assert_eq!(
        fs::read_to_string(staging.join(".fleximark-export.json")).unwrap(),
        "replacement"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recovery_refuses_replaced_journal_identity_without_touching_staging() {
    let root = test_workspace("journal-identity-test");
    let staging = root.join(".output.fleximark-export-staging-test");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join(".fleximark-export.json"), "original").unwrap();
    let journal_path = root.join(".output.fleximark-export-journal.json");
    let journal_identity = create_journal(&journal_path).unwrap();
    let journal = ExportJournal {
        transaction_id: "transaction".into(),
        state: ExportJournalState::Prepared,
        destination: root.join("output"),
        staging: staging.clone(),
        backup: root.join(".output.fleximark-export-backup-test"),
        registry: root.join("registry.json"),
        journal_identity,
        registry_staging: None,
        registry_staging_identity: None,
        previous_registry: None,
        next_registry: RegistryRecord {
            destination_id: "id".into(),
            generation: 1,
            digest: "digest".into(),
            source_identity: "source".into(),
            workspace_identity: "workspace".into(),
            destination_identity: "destination".into(),
            destination_object_identity: ObjectIdentity {
                platform_id: "destination-object".into(),
                kind: "directory".into(),
            },
        },
        staging_identity: object_identity(&staging).unwrap(),
        staging_marker_identity: object_identity(&staging.join(".fleximark-export.json")).unwrap(),
        generated_identities: Vec::new(),
        previous_destination_objects: Vec::new(),
        previous_destination_identity: None,
        previous_registry_identity: None,
        backup_identity: None,
        installed_identity: None,
        installed_marker_identity: None,
        next_registry_identity: None,
    };
    append_journal(&journal_path, &journal, None).unwrap();
    let records = fs::read(&journal_path).unwrap();
    let replacement = root.join("replacement-journal.json");
    fs::write(&replacement, records).unwrap();
    fs::remove_file(&journal_path).unwrap();
    fs::rename(replacement, &journal_path).unwrap();
    assert!(matches!(
        recover_export(
            &journal_path,
            &root.join("output"),
            &root.join("registry.json"),
            false
        ),
        Err(ApplicationError::ExportRecoveryConflict)
    ));
    assert_eq!(
        fs::read_to_string(staging.join(".fleximark-export.json")).unwrap(),
        "original"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn representative_export_intents_recover_idempotently_after_an_injected_crash() {
    for fault in ["old-move-intent", "installed", "committed"] {
        let root = test_workspace(&format!("export-crash-{fault}"));
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# export\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>old</p>",
            &[],
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();

        let result = export_html_transaction(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>new</p>",
            &[],
            |checkpoint| {
                if checkpoint == fault {
                    Err(ApplicationError::Io(std::io::Error::other(
                        "injected crash",
                    )))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err(), "fault {fault} was not reached");

        let preflight = preflight_export(&source_uri, &workspace_uri, &destination_uri);
        if matches!(fault, "old-move-intent" | "old-moved" | "install-intent") {
            preflight.unwrap();
            assert_eq!(
                fs::read_to_string(destination.join("index.html")).unwrap(),
                "<p>old</p>"
            );
        } else {
            assert!(matches!(
                preflight,
                Err(ApplicationError::ExportAwaitingAcknowledgement)
            ));
            assert_eq!(
                fs::read_to_string(destination.join("index.html")).unwrap(),
                "<p>new</p>"
            );
            acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        }
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn acknowledgement_refuses_unrecorded_backup_entries_without_deleting_them() {
    let root = test_workspace("export-backup-injection");
    let workspace_uri = path_to_file_uri(&root).unwrap();
    initialize_workspace(&workspace_uri).unwrap();
    let source = root.join("doc.md");
    fs::write(&source, "# export\n").unwrap();
    let source_uri = path_to_file_uri(&source).unwrap();
    let destination = root.join("public");
    let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>old</p>",
        &[],
    )
    .unwrap();
    acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>new</p>",
        &[],
    )
    .unwrap();
    let backup = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(".public.fleximark-export-backup-"))
        })
        .unwrap();
    fs::write(backup.join("unrecorded.txt"), "do not delete").unwrap();
    assert!(matches!(
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri),
        Err(ApplicationError::ExportRecoveryConflict)
    ));
    assert_eq!(
        fs::read_to_string(backup.join("unrecorded.txt")).unwrap(),
        "do not delete"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn old_export_registry_and_marker_fields_are_rejected() {
    let root = test_workspace("obsolete-export-fields");
    let workspace_uri = path_to_file_uri(&root).unwrap();
    initialize_workspace(&workspace_uri).unwrap();
    let source = root.join("note.md");
    fs::write(&source, "# Note").unwrap();
    let source_uri = path_to_file_uri(&source).unwrap();
    let destination = root.join("public");
    let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>Note</p>",
        &[],
    )
    .unwrap();
    let marker_path = destination.join(".fleximark-export.json");
    let mut marker: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker_path).unwrap()).unwrap();
    marker["payload"]["unsafeOutputUsed"] = serde_json::json!(false);
    assert!(serde_json::from_value::<OwnershipMarker>(marker).is_err());
    let registry_path = fs::read_dir(root.join(".fleximark/export-targets"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut registry: serde_json::Value =
        serde_json::from_slice(&fs::read(registry_path).unwrap()).unwrap();
    registry["unsafeOutputUsed"] = serde_json::json!(false);
    assert!(serde_json::from_value::<RegistryRecord>(registry).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_export_preserves_existing_destination() {
    let root = test_workspace("cancelled-export");
    let workspace_uri = path_to_file_uri(&root).unwrap();
    initialize_workspace(&workspace_uri).unwrap();
    let source = root.join("note.md");
    fs::write(&source, "# Note").unwrap();
    let source_uri = path_to_file_uri(&source).unwrap();
    let destination = root.join("public");
    let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>Original</p>",
        &[],
    )
    .unwrap();
    acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
    let engine =
        crate::native::open_document(source_uri.clone(), "# Changed".into(), true).unwrap();
    for stage in [
        "export_start",
        "export_render_start",
        "export_rendered",
        "export_assets",
    ] {
        let token = CancellationToken::default();
        crate::cancellation_checks::cancel_at(stage, &token);
        let result = export_document(
            &engine,
            &source_uri,
            &workspace_uri,
            &destination_uri,
            &token,
            None,
        );
        assert!(
            matches!(result, Err(ApplicationError::Cancelled)),
            "stage {stage}: {result:?}"
        );
        assert_eq!(
            fs::read_to_string(destination.join("index.html")).unwrap(),
            "<p>Original</p>"
        );
        assert!(token.is_cancelled());
    }
    fs::remove_dir_all(root).unwrap();
}
