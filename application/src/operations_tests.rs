//! Role: Verify shared command authorization, cached configuration and owned plans.
use crate::*;
use fleximark_protocol::{CommandContext, DocumentCommandContext, PreviewTarget, WorkspaceCommand};
use std::sync::Arc;
fn context(uri: &str) -> CommandContext {
    CommandContext {
        workspace_uri: Some(uri.into()),
        document: None,
    }
}
fn root(label: &str) -> (fleximark_test_support::TempWorkspace, String) {
    let path = native::test_support::test_workspace(label);
    let uri = WorkspaceAuthority::path_to_file_uri(&path).unwrap();
    (path, uri)
}

#[test]
fn restricted_commands_fail_before_workspace_resources_are_read() {
    let (path, uri) = root("restricted-commands");
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .configure_native_workspaces(vec![(uri.clone(), false)])
        .unwrap();
    let mut ops = WorkspaceOps::new(&mut store, None);
    for command in [
        WorkspaceCommand::EditTheme {},
        WorkspaceCommand::GetNoteOptions {},
        WorkspaceCommand::InitializeWorkspace {},
        WorkspaceCommand::PlanMigration {
            settings_uri: None,
            recover: false,
        },
        WorkspaceCommand::ApplyMigration {
            plan_id: "unknown".into(),
        },
    ] {
        assert!(matches!(
            ops.execute(command, context(&uri), &CancellationToken::default()),
            Err(ApplicationError::WriteDenied)
        ));
    }
    assert!(!path.join(".fleximark").exists());
}

#[test]
fn cached_config_is_reused_until_explicit_reload_and_invalidity_keeps_trust() {
    let (path, uri) = root("cached-commands");
    native::initialize_workspace(&uri).unwrap();
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .configure_native_workspaces(vec![(uri.clone(), true)])
        .unwrap();
    let first = store.authority().cached_config(&uri).unwrap();
    let second = store.authority().cached_config(&uri).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    std::fs::write(path.join(".fleximark/config.toml"), "invalid = [").unwrap();
    let result = WorkspaceOps::new(&mut store, None)
        .execute(
            WorkspaceCommand::CreateNote {
                category_path: None,
                template: None,
                name: Some("cached".into()),
            },
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(matches!(result, CommandOutcome::Note(_)));
    assert!(path.join("cached.md").is_file());
    assert!(matches!(
        store.reconfigure_native_workspace(&uri, true, &CancellationToken::default()),
        Err(ApplicationError::WorkspaceConfiguration(_))
    ));
    let status = store.workspace_statuses().remove(0);
    assert!(status.trusted);
    assert!(matches!(status.state, WorkspaceState::Invalid(_)));
    assert!(store.authority().cached_config(&uri).is_err());
    assert!(
        WorkspaceOps::new(&mut store, None)
            .execute(
                WorkspaceCommand::GetNoteOptions {},
                context(&uri),
                &CancellationToken::default()
            )
            .is_err()
    );
    let error = WorkspaceOps::new(&mut store, None)
        .execute(
            WorkspaceCommand::InitializeWorkspace {},
            context(&uri),
            &CancellationToken::default(),
        )
        .err()
        .unwrap();
    assert!(matches!(error, ApplicationError::Config(_)));
}

#[test]
fn migration_apply_uses_its_owned_plan_and_revalidates_changed_inputs() {
    let (path, uri) = root("owned-migration");
    std::fs::create_dir(path.join(".fleximark")).unwrap();
    std::fs::write(path.join(".fleximark/fleximark.json"), "{}").unwrap();
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .configure_native_workspaces(vec![(uri.clone(), true)])
        .unwrap();
    let planned = WorkspaceOps::new(&mut store, None)
        .execute(
            WorkspaceCommand::PlanMigration {
                settings_uri: None,
                recover: false,
            },
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    let id = match planned {
        CommandOutcome::MigrationPlanned(plan) => Some(plan.plan_id),
        _ => None,
    }
    .unwrap();
    std::fs::write(
        path.join(".fleximark/fleximark.json"),
        "{\"notePath\":\"changed\"}",
    )
    .unwrap();
    assert!(
        WorkspaceOps::new(&mut store, None)
            .execute(
                WorkspaceCommand::ApplyMigration { plan_id: id },
                context(&uri),
                &CancellationToken::default()
            )
            .is_err()
    );
    assert!(!path.join(".fleximark/config.toml").exists());
    let planned = WorkspaceOps::new(&mut store, None)
        .execute(
            WorkspaceCommand::PlanMigration {
                settings_uri: None,
                recover: false,
            },
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    let id = match planned {
        CommandOutcome::MigrationPlanned(plan) => Some(plan.plan_id),
        _ => None,
    }
    .unwrap();
    let applied = WorkspaceOps::new(&mut store, None)
        .execute(
            WorkspaceCommand::ApplyMigration {
                plan_id: id.clone(),
            },
            context(&uri),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(matches!(
        applied,
        CommandOutcome::MigrationApplied(MigrationAppliedOutcome {
            backup_uri: Some(_),
            recovered: false
        })
    ));
    assert!(path.join(".fleximark/config.toml").is_file());
    assert!(
        WorkspaceOps::new(&mut store, None)
            .execute(
                WorkspaceCommand::ApplyMigration { plan_id: id },
                context(&uri),
                &CancellationToken::default()
            )
            .is_err()
    );
}

#[test]
fn restricted_preview_validates_document_version_without_workspace_theme_reads() {
    let (path, uri) = root("restricted-preview");
    let mut store = DocumentStore::new(PositionEncoding::Utf8);
    store
        .configure_native_workspaces(vec![(uri.clone(), false)])
        .unwrap();
    let document_uri = format!("{uri}/note.md");
    store
        .open(
            DocumentUpdate {
                uri: document_uri.clone(),
                version: 1,
                source: "# Safe".into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let id = store.session_id_for_uri(&document_uri).unwrap().to_owned();
    for (version, valid) in [(2, false), (1, true)] {
        let result = WorkspaceOps::new(&mut store, None).execute(
            WorkspaceCommand::OpenPreview {
                target: PreviewTarget::EmbeddedHtml,
                source_offset: Some(0.into()),
            },
            CommandContext {
                workspace_uri: Some(uri.clone()),
                document: Some(DocumentCommandContext {
                    document_session_id: id.clone(),
                    expected_document_version: version.into(),
                }),
            },
            &CancellationToken::default(),
        );
        if valid {
            assert!(matches!(
                result,
                Ok(CommandOutcome::OpenPreview {
                    document_version: 1,
                    ..
                })
            ));
        } else {
            assert!(matches!(result, Err(ApplicationError::VersionMismatch)));
        }
    }
    assert!(
        store
            .render_with_cancellation(&id, 1, "preview", &CancellationToken::default())
            .unwrap()
            .html()
            .contains("Safe")
    );
    assert!(!path.join(".fleximark").exists());
}
