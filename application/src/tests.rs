//! Role: Verify session synchronization and workspace reconfiguration.

use fleximark_engine::EngineError;

use super::*;

pub(super) fn workspace_uri() -> &'static str {
    static URI: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    URI.get_or_init(|| {
        let root = crate::native::test_support::test_workspace("authority");
        std::fs::create_dir(root.join("nested")).unwrap();
        crate::native::path_to_file_uri(&root).unwrap()
    })
}
pub(super) fn workspace_document_uri(suffix: &str) -> &'static str {
    Box::leak(format!("{}/{}", workspace_uri(), suffix).into_boxed_str())
}

const DOCUMENT_URI: &str = "file:///doc.md";

fn open(registry: &mut DocumentStore, text: &str, version: u64) {
    open_uri(registry, DOCUMENT_URI, text, version);
}

fn open_uri(registry: &mut DocumentStore, uri: &str, text: &str, version: u64) {
    registry
        .open(
            DocumentUpdate {
                uri: uri.into(),
                version,
                source: text.into(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
}

fn change(uri: &str, version: u64, text: &str) -> DocumentUpdate {
    DocumentUpdate {
        uri: uri.into(),
        version,
        source: text.into(),
    }
}

fn session_id<'a>(registry: &'a DocumentStore, uri: &str) -> &'a str {
    registry.session_id_for_uri(uri).unwrap()
}

fn document<'a>(registry: &'a DocumentStore, uri: &str, version: u64) -> &'a DocumentSession {
    registry
        .document(session_id(registry, uri), version)
        .unwrap()
}

fn configured_render(label: &str, _trusted: bool) -> RenderConfig {
    let mut config = RenderConfig::default();
    config.config_hash = content_hash(label);
    config.config_generation = 1;
    config
}
fn render_fingerprint(
    registry: &mut DocumentStore,
    uri: &str,
    version: u64,
    preview_id: &str,
) -> String {
    let session_id = session_id(registry, uri).to_owned();
    registry
        .render_with_cancellation(
            &session_id,
            version,
            preview_id,
            &CancellationToken::default(),
        )
        .unwrap()
        .renderer_fingerprint
        .clone()
}

fn assets_with_duplicate_sources() -> ResolvedAssets {
    let asset = fleximark_engine::ResolvedRenderAsset::from_validated_bytes(
        "file:///asset.bin".into(),
        "application/octet-stream".into(),
        b"asset content",
    )
    .unwrap();
    vec![asset.clone(), asset].into()
}

#[test]
fn byte_offset_resolves_through_the_authoritative_session() {
    let mut registry = DocumentStore::new(PositionEncoding::Utf16);
    open(&mut registry, "# 😀 heading\n", 1);
    let session = session_id(&registry, DOCUMENT_URI).to_owned();
    let entry = registry
        .navigation_at_source_offset(&session, 1, 6)
        .unwrap()
        .unwrap();
    assert!(entry.source_range.byte_start <= 6 && entry.source_range.byte_end >= 6);
    assert!(matches!(
        registry.navigation_at_source_offset(&session, 1, 3),
        Err(ApplicationError::InvalidRange)
    ));
}

#[test]
fn full_sync_recovers_after_a_stale_update_and_reopen_replaces_identity() {
    let mut registry = DocumentStore::new(PositionEncoding::Utf16);
    open(&mut registry, "old", 3);
    let old = session_id(&registry, DOCUMENT_URI).to_owned();
    assert!(matches!(
        registry.change(
            change(DOCUMENT_URI, 3, "stale"),
            &CancellationToken::default()
        ),
        Err(ApplicationError::StaleVersion)
    ));
    registry
        .change(
            change(DOCUMENT_URI, 4, "latest 😀"),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(document(&registry, DOCUMENT_URI, 4).source(), "latest 😀");
    open(&mut registry, "reconnected", 1);
    assert_ne!(session_id(&registry, DOCUMENT_URI), old);
    assert_eq!(registry.open_documents().len(), 1);
}

#[test]
fn session_index_replaces_colliding_session_ids_atomically() {
    let mut registry = DocumentStore::new(PositionEncoding::Utf8);
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
fn full_document_updates_accept_unicode_in_all_snapshot_encodings() {
    for encoding in [
        PositionEncoding::Utf8,
        PositionEncoding::Utf16,
        PositionEncoding::Utf32,
    ] {
        let mut registry = DocumentStore::new(encoding);
        open(&mut registry, "a😀b\n", 1);
        registry
            .change(
                change(DOCUMENT_URI, 2, "axb😀\n"),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(document(&registry, DOCUMENT_URI, 2).source(), "axb😀\n");
    }
}

#[test]
fn workspace_membership_uses_longest_file_uri_segment_and_excludes_root_itself() {
    let mut registry = DocumentStore::new(PositionEncoding::Utf8);
    let parent_config = configured_render("parent", true);
    let nested_config = configured_render("nested", false);
    let file_root_config = configured_render("file-root", true);
    let empty_root_config = configured_render("empty-root", true);
    registry
        .configure_workspaces(vec![
            ("".into(), empty_root_config),
            ("file:///".into(), file_root_config),
            (workspace_uri().into(), parent_config),
            (workspace_document_uri("nested/").into(), nested_config),
        ])
        .unwrap();

    let cases = [
        (workspace_document_uri("nested"), Some(workspace_uri())),
        (
            workspace_document_uri("nested/deep.md"),
            Some(workspace_document_uri("nested")),
        ),
        ("file:///outside.md", None),
        ("/outside.md", None),
        (
            workspace_document_uri("NESTED/case.md"),
            if cfg!(windows) {
                Some(workspace_document_uri("nested"))
            } else {
                Some(workspace_uri())
            },
        ),
        (
            workspace_document_uri("%6Eested/encoded.md"),
            Some(workspace_document_uri("nested")),
        ),
        ("file:///workspace-other/sibling.md", None),
        ("relative.md", None),
    ];
    for (uri, expected_workspace) in cases {
        open_uri(&mut registry, uri, "text", 1);
        let session = document(&registry, uri, 1);
        assert_eq!(session.workspace_uri(), expected_workspace);
    }

    let parent_fingerprint = render_fingerprint(
        &mut registry,
        workspace_document_uri("NESTED/case.md"),
        1,
        "parent-case",
    );
    let encoded_fingerprint = render_fingerprint(
        &mut registry,
        workspace_document_uri("%6Eested/encoded.md"),
        1,
        "parent-encoded",
    );
    let nested_fingerprint = render_fingerprint(
        &mut registry,
        workspace_document_uri("nested/deep.md"),
        1,
        "nested",
    );
    assert_eq!(encoded_fingerprint, nested_fingerprint);
    if !cfg!(windows) {
        assert_ne!(nested_fingerprint, parent_fingerprint);
    }
}

#[test]
fn workspace_reconfiguration_is_transactional() {
    let mut workspace = DocumentStore::new(PositionEncoding::Utf8);
    let old_config = configured_render("old", true);
    workspace
        .configure_workspaces(vec![(workspace_uri().into(), old_config)])
        .unwrap();
    let original_uris = [
        workspace_document_uri("alpha.md"),
        workspace_document_uri("beta.md"),
    ];
    open_uri(&mut workspace, original_uris[0], "alpha", 1);
    open_uri(&mut workspace, original_uris[1], "beta", 2);
    let baseline = original_uris.map(|uri| {
        let session_id = session_id(&workspace, uri).to_owned();
        let version = if uri.ends_with("alpha.md") { 1 } else { 2 };
        let session = workspace.document(&session_id, version).unwrap();
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
        .filter(|(_, session)| session.workspace_uri() == Some(workspace_uri()))
        .map(|(uri, _)| uri.to_owned())
        .collect::<Vec<_>>();
    assert_eq!(stage_order.len(), 2);
    let failing_uri = stage_order.last().unwrap().clone();
    let mut invalid_assets = HashMap::new();
    invalid_assets.insert(failing_uri, assets_with_duplicate_sources());
    let mismatched_config = configured_render("mismatched", true);
    let error = workspace
        .reconfigure_workspace(
            workspace_uri(),
            mismatched_config,
            invalid_assets,
            &CancellationToken::default(),
        )
        .unwrap_err();
    assert!(
        matches!(error, ApplicationError::Engine(EngineError::Asset(ref detail)) if detail == "resolved asset sources must be unique")
    );

    for (uri, session_id, source, version, fingerprint) in &baseline {
        assert_eq!(workspace.session_id_for_uri(uri), Some(session_id.as_str()));
        let session = workspace.document(session_id, *version).unwrap();
        assert_eq!(session.source(), source);
        assert_eq!(session.document().document_version, *version);
        assert_eq!(
            render_fingerprint(
                &mut workspace,
                uri,
                *version,
                &format!("after-failure-{uri}"),
            ),
            *fingerprint
        );
    }

    let new_uri = workspace_document_uri("after-failure.md");
    open_uri(&mut workspace, new_uri, "new", 3);
    let new_session_id = session_id(&workspace, new_uri).to_owned();
    assert_eq!(
        workspace
            .document(&new_session_id, 3)
            .unwrap()
            .workspace_uri(),
        Some(workspace_uri())
    );
    assert_eq!(
        render_fingerprint(&mut workspace, new_uri, 3, "new-after-failure"),
        baseline[0].4
    );

    let replacement_config = configured_render("replacement", true);
    workspace
        .reconfigure_workspace(
            workspace_uri(),
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
        let session = workspace.document(session_id, version).unwrap();
        assert_eq!(session.source(), source);
        assert_eq!(session.document().document_version, version);
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
    use std::error::Error;
    for (error, kind) in [
        (EngineError::ContentModified, ErrorKind::Conflict),
        (
            EngineError::StaleVersion {
                current: 3,
                received: 2,
            },
            ErrorKind::Conflict,
        ),
        (
            EngineError::Configuration("config detail".into()),
            ErrorKind::Invalid,
        ),
        (
            EngineError::Asset("asset detail".into()),
            ErrorKind::Invalid,
        ),
    ] {
        let message = error.to_string();
        let application = engine_error(error);
        assert!(matches!(application, ApplicationError::Engine(_)));
        assert_eq!(application.kind(), kind);
        assert_eq!(application.to_string(), message);
        assert_eq!(application.source().unwrap().to_string(), message);
    }
}

#[test]
fn canonical_nested_authority_beats_long_outer_alias_and_duplicate_roots_reject() {
    let root = crate::native::test_support::test_workspace("precedence");
    let outer = root.join("outer");
    let nested = outer.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let base_uri = native::path_to_file_uri(&root).unwrap();
    let outer_uri = native::path_to_file_uri(&outer).unwrap();
    let nested_uri = format!("{outer_uri}/%6Eested");
    let document_uri = format!("{outer_uri}/nested/new.md");
    let alias_name =
        "very-long-trusted-outer-workspace-alias-that-must-never-outrank-a-nested-root";
    let alias = root.join(alias_name);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&outer, &alias);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_dir(&outer, &alias);
    let outer_alias = if linked.is_ok() {
        format!("{base_uri}/{alias_name}")
    } else {
        format!("{base_uri}/%6Futer")
    };
    let mut registry = DocumentStore::new(PositionEncoding::Utf8);
    let outer_config = configured_render("privileged-outer", true);
    let nested_config = configured_render("restricted-nested", false);
    let nested_hash = nested_config.config_hash.clone();
    registry
        .configure_workspaces(vec![
            (outer_alias.clone(), outer_config),
            (nested_uri.clone(), nested_config),
        ])
        .unwrap();
    registry
        .authority_mut()
        .set_state(&nested_uri, WorkspaceState::Restricted);
    assert_eq!(
        registry.workspace_for_document(&document_uri),
        Some(WorkspaceMembership {
            uri: nested_uri.clone(),
            trusted: false
        })
    );
    open_uri(&mut registry, &document_uri, "# Source", 1);
    assert_eq!(
        document(&registry, &document_uri, 1).workspace_uri(),
        Some(nested_uri.as_str())
    );
    assert_eq!(
        document(&registry, &document_uri, 1)
            .engine()
            .render_config()
            .config_hash,
        nested_hash
    );
    assert!(!registry.workspaces.is_trusted(&nested_uri));
    let mut duplicate = DocumentStore::new(PositionEncoding::Utf8);
    assert!(
        duplicate
            .configure_native_workspaces(vec![
                (outer_uri.clone(), true),
                (format!("{base_uri}/%6Futer"), false)
            ])
            .is_err()
    );
    assert!(duplicate.workspace_statuses().is_empty());
    if linked.is_ok() {
        std::fs::remove_dir(alias).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn revocation_ignores_invalid_config_and_cancellation() {
    for cancel in [false, true] {
        let root = crate::native::test_support::test_workspace("revoke");
        let uri = native::path_to_file_uri(&root).unwrap();
        native::initialize_workspace(&uri).unwrap();
        let document_uri = format!("{uri}/new.md");
        let config = configured_render("privileged", true);
        let mut registry = DocumentStore::new(PositionEncoding::Utf8);
        registry
            .configure_workspaces(vec![(uri.clone(), config)])
            .unwrap();
        open_uri(&mut registry, &document_uri, "# Plain", 1);
        std::fs::write(root.join(".fleximark/config.toml"), "invalid = [").unwrap();
        let token = CancellationToken::default();
        if cancel {
            token.cancel();
        }
        let result = registry.reconfigure_native_workspace(&uri, false, &token);
        if !cancel {
            result.unwrap();
        }
        assert!(!registry.workspaces.is_trusted(&uri));
        assert!(
            document(&registry, &document_uri, 1)
                .document()
                .metadata
                .front_matter
                .is_none()
        );
        assert_eq!(
            document(&registry, &document_uri, 1)
                .document()
                .blocks
                .len(),
            1,
            "generated privileged IR must be discarded even on cancellation"
        );
        registry
            .change(
                change(&document_uri, 2, "# Changed"),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(
            document(&registry, &document_uri, 2)
                .document()
                .blocks
                .len(),
            1
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
