use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, OnceLock};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signer, SigningKey};
use fleximark_model::{
    AnchorAffinity, DocumentUri, GeneratedAnchor, JsSafeU64, NodeId, PositionEncoding,
    SourcePosition, SourceProvenance, TransformId,
};
use fleximark_plugin_host::{
    CancellationToken, ExecutionLimits, HostPolicy, PluginFailureKind, PluginHost,
    VerifiedPluginPackage,
};
use fleximark_plugin_sdk::{EditOrigin, Hook, PluginCapabilities, PreprocessedSource};
use fleximark_render_html::{HtmlTarget, RawHtmlPolicy, RenderContext, RenderError};

use crate::assets::MAX_RENDER_ASSET_BYTES;
use crate::identity::{content_hash, content_hash_bytes};
use crate::provenance::remap_provenance_for_test;
use crate::{
    DocumentSession, EngineError, PatchOperation, PreviewSessionId, RenderConfig, RenderPatch,
    RenderPublication, RenderSnapshot, RenderStyle, ResolvedRenderAsset,
};

fn utf8_range(source: &str, byte_start: usize, byte_end: usize) -> fleximark_model::SourceRange {
    let position = |offset: usize| {
        let before = &source[..offset];
        let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
        SourcePosition {
            line: JsSafeU64::new(before.bytes().filter(|byte| *byte == b'\n').count() as u64)
                .unwrap(),
            character: JsSafeU64::new((offset - line_start) as u64).unwrap(),
            encoding: PositionEncoding::Utf8,
        }
    };
    fleximark_model::SourceRange {
        byte_start: JsSafeU64::new(byte_start as u64).unwrap(),
        byte_end: JsSafeU64::new(byte_end as u64).unwrap(),
        start: position(byte_start),
        end: position(byte_end),
    }
}

fn open(source: &str) -> DocumentSession {
    DocumentSession::open(
        DocumentUri("file:///test.md".into()),
        1,
        source.to_owned(),
        PositionEncoding::Utf16,
    )
    .unwrap()
}

fn full(publication: RenderPublication, message: &str) -> RenderSnapshot {
    let RenderPublication::Full(snapshot) = publication else {
        panic!("{message}")
    };
    snapshot
}

fn patch(publication: RenderPublication, message: &str) -> RenderPatch {
    let RenderPublication::Patch(patch) = publication else {
        panic!("{message}")
    };
    patch
}

fn render_default(session: &mut DocumentSession, id: &str) -> RenderPublication {
    session
        .render(PreviewSessionId(id.into()), &RenderContext::default())
        .unwrap()
}

fn fixture_component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let fixture_manifest = manifest_dir
            .join("../../fixtures/plugin-component/Cargo.toml")
            .canonicalize()
            .unwrap();
        let fixture_target = fixture_manifest.parent().unwrap().join("target");
        let status = Command::new(env!("CARGO"))
            .args([
                "build",
                "--quiet",
                "--manifest-path",
                fixture_manifest.to_str().unwrap(),
                "--target-dir",
                fixture_target.to_str().unwrap(),
                "--target",
                "wasm32-wasip2",
                "--locked",
            ])
            .status()
            .unwrap();
        assert!(status.success(), "fixture component build failed");
        std::fs::read(fixture_target.join("wasm32-wasip2/debug/fleximark_fixture_plugin.wasm"))
            .unwrap()
    })
}

fn fixture_host(required: bool) -> Arc<PluginHost> {
    let wasm = fixture_component();
    let manifest = format!(
        "schema_version = 1\n\n[plugin]\nid = \"engine-fixture\"\napi_version = 1\nrequired = {required}\n\n[artifact]\nwasm_sha256 = \"{}\"\n\n[capabilities]\n",
        content_hash_bytes(wasm)
    );
    let signing_key = SigningKey::from_bytes(&[11; 32]);
    let signature = signing_key.sign(manifest.as_bytes()).to_bytes();
    let public_key = signing_key
        .verifying_key()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut host = PluginHost::configured(
        ExecutionLimits::default(),
        HostPolicy {
            workspace_trusted: true,
            workspace_root: ".".to_owned(),
        },
        content_hash("engine-config"),
        1,
    )
    .unwrap();
    host.register_verified(VerifiedPluginPackage {
        configured_id: "engine-fixture",
        config_order: 0,
        manifest_bytes: manifest.as_bytes(),
        expected_manifest_sha256: &content_hash(&manifest),
        wasm_bytes: wasm,
        signature_bytes: &signature,
        signer_public_key: &public_key,
        grants: PluginCapabilities::default(),
        environment: BTreeMap::new(),
    })
    .unwrap();
    Arc::new(host)
}

#[test]
fn reconciles_moved_unique_nodes_but_not_ambiguous_duplicates() {
    let mut session = open("# A\n\n# B\n");
    let old = session
        .document
        .blocks
        .iter()
        .map(|block| block.id.clone())
        .collect::<Vec<_>>();
    session
        .change_full_text(2, "# Intro\n\n# A\n\n# B\n".into())
        .unwrap();
    assert_eq!(session.document.blocks[1].id, old[0]);
    assert_eq!(session.document.blocks[2].id, old[1]);

    let mut duplicates = open("same\n");
    let old_id = duplicates.document.blocks[0].id.clone();
    duplicates
        .change_full_text(2, "# lead\n\nsame\n\nsame\n".into())
        .unwrap();
    assert!(
        duplicates.document.blocks[1..]
            .iter()
            .all(|block| block.id != old_id)
    );
    assert_ne!(
        duplicates.document.blocks[1].id,
        duplicates.document.blocks[2].id
    );
}

#[test]
fn rejects_stale_changes_until_full_text_resynchronization() {
    let mut session = open("old\n");
    assert!(matches!(
        session.change_full_text(1, "stale\n".into()),
        Err(EngineError::StaleVersion { .. })
    ));
    assert!(session.is_out_of_sync());
    assert!(matches!(
        session.render(PreviewSessionId("p".into()), &RenderContext::default()),
        Err(EngineError::ContentModified)
    ));
    session.resynchronize(1, "fresh\n".into()).unwrap();
    assert!(!session.is_out_of_sync());
    assert_eq!(session.document().document_version, 1);
    assert_eq!(session.source(), "fresh\n");
}

#[test]
fn configured_open_change_and_resync_preserve_pipeline_state_and_diagnostic_order() {
    let optional_host = fixture_host(false);
    let config = RenderConfig::for_plugins(RenderContext::default(), None, &optional_host);
    let mut configured = DocumentSession::open_configured(
        DocumentUri("file:///configured-pipeline.md".into()),
        1,
        "# one\n".to_owned(),
        PositionEncoding::Utf16,
        config,
        Arc::clone(&optional_host),
        &CancellationToken::default(),
    )
    .unwrap();
    let mut plain = DocumentSession::open(
        DocumentUri("file:///configured-pipeline.md".into()),
        1,
        "# one\n".to_owned(),
        PositionEncoding::Utf16,
    )
    .unwrap();
    assert_eq!(
        configured.document().document_version,
        plain.document().document_version
    );
    assert_eq!(configured.document().uri, plain.document().uri);
    assert_eq!(
        configured.document().blocks[0].kind,
        plain.document().blocks[0].kind
    );
    let SourceProvenance::Original {
        ranges: plain_ranges,
        ..
    } = &plain.document().blocks[0].provenance
    else {
        panic!("plain parsing must retain original provenance")
    };
    let SourceProvenance::Derived {
        ranges, transform, ..
    } = &configured.document().blocks[0].provenance
    else {
        panic!("plugin preprocessing must remap provenance before identity reconciliation")
    };
    assert_eq!(ranges, plain_ranges);
    assert_eq!(transform, &TransformId("plugin-preprocess-v1".to_owned()));
    assert_ne!(
        configured.document().blocks[0].id,
        plain.document().blocks[0].id
    );
    assert!(configured.plugin_diagnostics().is_empty());

    let cancelled = CancellationToken::default();
    cancelled.cancel();
    configured
        .change_full_text_with_cancellation(2, "# two\n".to_owned(), &cancelled)
        .unwrap();
    plain.change_full_text(2, "# two\n".to_owned()).unwrap();
    assert_eq!(configured.source(), "# two\n");
    assert_eq!(configured.document().document_version, 2);
    assert_eq!(configured.content_hash(), content_hash("# two\n"));
    assert_eq!(configured.source(), plain.source());
    assert_eq!(configured.content_hash(), plain.content_hash());
    assert_eq!(
        configured.document().blocks[0].kind,
        plain.document().blocks[0].kind
    );
    assert_eq!(
        configured.document().blocks[0].provenance,
        plain.document().blocks[0].provenance
    );
    assert!(!configured.is_out_of_sync());
    assert_eq!(
        configured
            .plugin_diagnostics()
            .iter()
            .map(|diagnostic| (&diagnostic.hook, &diagnostic.kind))
            .collect::<Vec<_>>(),
        [
            (&Hook::PreprocessSource, &PluginFailureKind::Cancelled),
            (&Hook::TransformBlock, &PluginFailureKind::Cancelled),
            (&Hook::TransformDocument, &PluginFailureKind::Cancelled),
        ]
    );

    assert!(matches!(
        configured.change_full_text(2, "stale\n".to_owned()),
        Err(EngineError::StaleVersion { .. })
    ));
    assert!(matches!(
        plain.change_full_text(2, "stale\n".to_owned()),
        Err(EngineError::StaleVersion { .. })
    ));
    assert!(configured.is_out_of_sync());
    assert!(plain.is_out_of_sync());
    configured
        .resynchronize_with_cancellation(2, "# resynced\n".to_owned(), &cancelled)
        .unwrap();
    plain.resynchronize(2, "# resynced\n".to_owned()).unwrap();
    assert_eq!(configured.source(), "# resynced\n");
    assert_eq!(configured.document().document_version, 2);
    assert!(!configured.is_out_of_sync());
    assert_eq!(configured.source(), plain.source());
    assert_eq!(configured.content_hash(), plain.content_hash());
    assert_eq!(
        configured.document().blocks[0].provenance,
        plain.document().blocks[0].provenance
    );
    assert_eq!(
        configured
            .plugin_diagnostics()
            .iter()
            .map(|diagnostic| &diagnostic.hook)
            .collect::<Vec<_>>(),
        [
            &Hook::PreprocessSource,
            &Hook::TransformBlock,
            &Hook::TransformDocument
        ]
    );
    configured
        .checkpoint(2, &content_hash("# resynced\n"))
        .unwrap();
}

#[test]
fn required_plugin_cancellation_rolls_back_authoritative_state_and_render_cache() {
    let required_host = fixture_host(true);
    let config = RenderConfig::for_plugins(RenderContext::default(), None, &required_host);
    let mut session = DocumentSession::open_configured(
        DocumentUri("file:///required-rollback.md".into()),
        1,
        "before\n".to_owned(),
        PositionEncoding::Utf8,
        config,
        required_host,
        &CancellationToken::default(),
    )
    .unwrap();
    let preview = PreviewSessionId("cancelled-preview".into());
    let RenderPublication::Full(initial) = session
        .render_configured(preview.clone(), &CancellationToken::default())
        .unwrap()
        .publication
    else {
        panic!("the first configured render must be full")
    };
    assert_eq!(initial.result_render_revision, 1);
    assert_eq!(session.preview_count(), 1);
    let original_document = session.document().clone();
    let original_hash = session.content_hash();
    let original_diagnostics = session.plugin_diagnostics().to_vec();
    let cancelled = CancellationToken::default();
    cancelled.cancel();

    let error = session
        .change_full_text_with_cancellation(2, "after\n".to_owned(), &cancelled)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "plugin pipeline failed: plugin engine-fixture is required and failed (Cancelled): document version was cancelled"
    );
    assert_eq!(session.source(), "before\n");
    assert_eq!(session.content_hash(), original_hash);
    assert_eq!(session.document(), &original_document);
    assert_eq!(session.plugin_diagnostics(), original_diagnostics);
    assert!(!session.is_out_of_sync());

    let RenderPublication::Patch(unchanged) = session
        .render_configured(preview.clone(), &CancellationToken::default())
        .unwrap()
        .publication
    else {
        panic!("the unchanged document must retain its preview cache")
    };
    assert_eq!(unchanged.base_render_revision, 1);
    assert_eq!(unchanged.result_render_revision, 2);
    assert!(unchanged.operations.is_empty());
    assert_eq!(session.preview_count(), 1);

    assert_eq!(
        session
            .render_configured(preview.clone(), &cancelled)
            .unwrap_err()
            .to_string(),
        "plugin pipeline failed: operation cancelled"
    );
    assert_eq!(session.preview_count(), 1);
    let RenderPublication::Patch(after_cancelled_render) = session
        .render_configured(preview, &CancellationToken::default())
        .unwrap()
        .publication
    else {
        panic!("a cancelled render must leave the existing preview cache unchanged")
    };
    assert_eq!(after_cancelled_render.base_render_revision, 2);
    assert_eq!(after_cancelled_render.result_render_revision, 3);
    assert!(after_cancelled_render.operations.is_empty());
}

#[test]
fn publishes_patch_only_with_equal_fingerprints_and_full_on_policy_change() {
    let mut session = open("# A\n");
    session.render_config.style = Some(RenderStyle::from_validated_css(
        "main { color: canvastext; }".to_owned(),
    ));
    let first = render_default(&mut session, "preview-1");
    let snapshot = full(first, "first render must be full");
    let original_id = session.document.blocks[0].id.clone();
    assert!(
        snapshot
            .html
            .starts_with("<main data-fleximark-node-id=\"document-root\">")
    );
    assert_eq!(snapshot.node_ids[0], NodeId("document-root".into()));
    assert_eq!(snapshot.node_ids[1], session.document.blocks[0].id);
    assert_eq!(snapshot.style, session.render_config.style);
    assert!(!snapshot.html.contains("color: canvastext"));
    assert_eq!(
        snapshot.navigation[0].node_id,
        session.document.blocks[0].id
    );
    session.change_full_text(2, "# A changed\n".into()).unwrap();
    let second = render_default(&mut session, "preview-1");
    let patch = patch(second, "expected patch");
    assert_eq!(session.document.blocks[0].id, original_id);
    assert!(
        matches!(patch.operations.as_slice(), [PatchOperation::Replace { node_id, .. }] if node_id == &original_id)
    );
    assert_eq!(
        patch.base_renderer_fingerprint,
        patch.result_renderer_fingerprint
    );
    assert_eq!(patch.style, session.render_config.style);
    assert_eq!(
        (
            patch.base_render_revision.get(),
            patch.result_render_revision.get()
        ),
        (1, 2)
    );
    assert!(!patch.operations.is_empty());
    let wire = serde_json::to_value(&patch).unwrap();
    assert_eq!(wire["baseRenderRevision"], 1);
    assert!(wire["navigation"][0]["sourceRange"]["byteStart"].is_number());
    assert!(wire["operations"][0].get("nodeId").is_some());
    assert!(
        wire["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| operation.get("contentNodeIds").is_some())
    );

    let strict = RenderContext {
        raw_html: RawHtmlPolicy::Reject,
        ..RenderContext::default()
    };
    let third = session
        .render(PreviewSessionId("preview-1".into()), &strict)
        .unwrap();
    let snapshot = full(third, "fingerprint change must be full");
    assert_eq!(snapshot.result_render_revision, 3);
}

#[test]
fn emits_allowlisted_attribute_delta_and_serializes_it_in_camel_case() {
    let mut session = open(":::info[Note]\nbody\n:::\n");
    render_default(&mut session, "attributes");
    let original_id = session.document.blocks[0].id.clone();
    session
        .change_full_text(2, ":::tip[Note]\nbody\n:::\n".to_owned())
        .unwrap();
    assert_eq!(session.document.blocks[0].id, original_id);
    let patch = patch(
        render_default(&mut session, "attributes"),
        "presentation-only change should patch",
    );
    assert!(matches!(
        patch.operations.as_slice(),
        [PatchOperation::SetAttributes { attributes, .. }]
            if attributes.get("data-admonition-kind") == Some(&Some("tip".to_owned()))
    ));
    let wire = serde_json::to_value(&patch.operations[0]).unwrap();
    assert_eq!(wire["type"], "setAttributes");
    assert_eq!(wire["attributes"]["data-admonition-kind"], "tip");
}

#[test]
fn resolved_assets_are_bounded_published_and_fingerprinted_without_source_paths() {
    let mut session = open("![diagram](private/diagram.png)\n");
    let first_asset = ResolvedRenderAsset::from_validated_bytes(
        "private/diagram.png".to_owned(),
        "image/png".to_owned(),
        b"png-one",
    )
    .unwrap();
    session.render_config = RenderConfig::default()
        .with_resolved_assets(vec![first_asset])
        .unwrap();
    let preview = PreviewSessionId("asset-preview".into());
    let context = session.render_config.context.clone();
    let first = full(
        session.render(preview.clone(), &context).unwrap(),
        "first asset publication must be full",
    );
    assert_eq!(first.assets.len(), 1);
    assert_eq!(first.assets[0].data, BASE64.encode(b"png-one"));
    assert!(first.html.contains(&first.assets[0].reference));
    let wire = serde_json::to_string(&first).unwrap();
    assert!(!wire.contains("private/diagram.png"));

    session
        .change_full_text(2, "![changed](private/diagram.png)\n".to_owned())
        .unwrap();
    let context = session.render_config.context.clone();
    let patch = patch(
        session.render(preview.clone(), &context).unwrap(),
        "unchanged asset set may patch",
    );
    assert!(
        serde_json::to_value(&patch)
            .unwrap()
            .get("assets")
            .is_none()
    );

    session.render_config = RenderConfig::default()
        .with_resolved_assets(vec![
            ResolvedRenderAsset::from_validated_bytes(
                "private/diagram.png".to_owned(),
                "image/png".to_owned(),
                b"png-two",
            )
            .unwrap(),
        ])
        .unwrap();
    let context = session.render_config.context.clone();
    let second = full(
        session.render(preview, &context).unwrap(),
        "asset content change must force full publication",
    );
    assert_ne!(first.renderer_fingerprint, second.renderer_fingerprint);
    assert_ne!(first.assets[0].content_hash, second.assets[0].content_hash);

    assert!(
        ResolvedRenderAsset::from_validated_bytes(
            "too-large.bin".to_owned(),
            "application/octet-stream".to_owned(),
            &vec![0; MAX_RENDER_ASSET_BYTES + 1],
        )
        .is_err()
    );
}

#[test]
fn render_only_reconfiguration_preserves_ir_and_plugin_diagnostics() {
    let host = Arc::new(
        PluginHost::configured(
            fleximark_plugin_host::ExecutionLimits::default(),
            fleximark_plugin_host::HostPolicy {
                workspace_trusted: true,
                workspace_root: ".".to_owned(),
            },
            content_hash("config"),
            3,
        )
        .unwrap(),
    );
    let initial = RenderConfig::for_plugins(RenderContext::default(), None, &host);
    let mut session = DocumentSession::open_configured(
        DocumentUri("file:///render-only.md".into()),
        1,
        "before\n".to_owned(),
        PositionEncoding::Utf8,
        initial,
        Arc::clone(&host),
        &CancellationToken::default(),
    )
    .unwrap();
    let document = session.document.clone();
    let diagnostics = session.plugin_diagnostics.clone();
    let updated = RenderConfig::for_plugins(RenderContext::default(), None, &host)
        .with_resolved_assets(vec![
            ResolvedRenderAsset::from_validated_bytes(
                "unused.png".to_owned(),
                "image/png".to_owned(),
                b"image",
            )
            .unwrap(),
        ])
        .unwrap();

    session.reconfigure_render(updated).unwrap();

    assert_eq!(session.document, document);
    assert_eq!(session.source, "before\n");
    assert_eq!(session.plugin_diagnostics, diagnostics);
    assert_eq!(session.render_config.assets().len(), 1);
}

#[test]
fn staged_reconfiguration_preserves_preview_revision_and_forces_full() {
    let mut session = open("hello\n");
    render_default(&mut session, "reconfigure");
    let mut configured = open("hello\n");
    configured.render_config.style = Some(RenderStyle::from_validated_css(
        "p { color: green; }".to_owned(),
    ));
    session.adopt_reconfiguration(configured).unwrap();
    let snapshot = full(
        render_default(&mut session, "reconfigure"),
        "configuration fingerprint change must be full",
    );
    assert_eq!(snapshot.result_render_revision, 2);
}

#[test]
fn checkpoint_is_bound_to_version_and_utf8_content() {
    let mut session = open("🦀\n");
    let hash = session.content_hash();
    assert_eq!(
        hash,
        "5d40fbf44301a6d80c06a5a5fb6aa8cdbb0c987d3aa07740e2bb941df3d7b862"
    );
    session.checkpoint(1, &hash).unwrap();
    assert!(matches!(
        session.checkpoint(1, "wrong"),
        Err(EngineError::CheckpointMismatch)
    ));
}

#[test]
fn render_metadata_and_annotations_are_part_of_the_publication() {
    let mut session = open("# A\n");
    let first = render_default(&mut session, "configured");
    let first = full(first, "first publication must be full");

    session.render_config.style = Some(RenderStyle::from_validated_css(
        "main { color: rebeccapurple; }".to_owned(),
    ));
    let second = render_default(&mut session, "configured");
    let second = full(second, "metadata changes must force a full publication");
    assert_ne!(first.renderer_fingerprint, second.renderer_fingerprint);
    assert_eq!(second.style, session.render_config.style);
    let wire = serde_json::to_value(&second).unwrap();
    assert_eq!(wire["style"]["css"], "main { color: rebeccapurple; }");
    assert!(wire["style"]["fingerprint"].is_string());

    let annotations = BTreeMap::from([("plugin".to_owned(), "</script><img src=x>".to_owned())]);
    let annotated = session
        .render_internal_for_test(
            PreviewSessionId("annotations".into()),
            &RenderContext::default(),
            &annotations,
        )
        .unwrap();
    let annotated = full(annotated, "first annotated publication must be full");
    assert!(annotated.html.contains("data-fleximark-render-annotations"));
    assert!(annotated.html.contains("\\u003c/script\\u003e"));
    assert!(!annotated.html.contains("</script><img"));
}

#[test]
fn unsafe_plugin_html_is_reachable_only_from_explicit_portable_export() {
    let mut session = open("hello\n");
    session.render_config.style = Some(RenderStyle::from_validated_css(
        "body { color: green; }".to_owned(),
    ));
    assert!(matches!(
        session.prepare_safe_export(&RenderContext::default()),
        Err(EngineError::UnsafeExportPolicy)
    ));
    let portable = RenderContext {
        target: HtmlTarget::Portable,
        raw_html: RawHtmlPolicy::Reject,
        allow_remote_resources: false,
        allow_data_resources: false,
        resolved_resources: BTreeMap::new(),
    };
    let prepared = session.prepare_safe_export(&portable).unwrap();
    assert!(prepared.safe_html().contains("hello"));
    let denied = session
        .prepare_safe_export(&portable)
        .unwrap()
        .compose_portable("common-runtime", |_, _, _, _| {
            Err::<String, _>("asset containment failed")
        });
    assert!(matches!(denied, Err("asset containment failed")));
    let resolved = prepared
        .compose_portable("common-runtime", |html, style, assets, runtime| {
            assert_eq!(style.unwrap().css(), "body { color: green; }");
            assert!(assets.is_empty());
            assert_eq!(runtime, "common-runtime");
            Ok::<_, EngineError>(format!(
                "<style>{}</style>{}<script>{runtime}</script>",
                style.unwrap().css(),
                html.replace("hello", "resolved")
            ))
        })
        .unwrap();
    let exported = session
        .apply_unsafe_export_html(resolved, &CancellationToken::default())
        .unwrap();
    assert!(exported.value.html.contains("resolved"));
    assert!(!exported.value.unsafe_output_used);
    assert!(exported.diagnostics.is_empty());

    let raw = open("<b>unsafe</b>\n");
    let escaped = RenderContext {
        target: HtmlTarget::Portable,
        raw_html: RawHtmlPolicy::Escape,
        ..RenderContext::default()
    };
    assert!(
        raw.prepare_safe_export(&escaped)
            .unwrap()
            .safe_html()
            .contains("&lt;b&gt;unsafe&lt;/b&gt;")
    );
    assert!(matches!(
        raw.prepare_safe_export(&portable),
        Err(EngineError::Render(RenderError::RawHtmlRejected))
    ));
}

#[test]
fn render_style_rejects_a_fingerprint_that_does_not_match_its_css() {
    let style = RenderStyle::from_validated_css("body { color: red; }".to_owned());
    let mut forged = serde_json::to_value(style).unwrap();
    forged["fingerprint"] = "0".repeat(64).into();
    assert!(serde_json::from_value::<RenderStyle>(forged).is_err());
}

#[test]
fn preprocess_mapping_clips_unicode_ranges_across_insertions_and_deletions() {
    let original = "αβ--tail";
    let output = "★αβtail";
    let map = PreprocessedSource {
        text: output.to_owned(),
        segments: vec![
            fleximark_plugin_sdk::EditMapSegment {
                output_start: 0,
                output_end: 3,
                origin: EditOrigin::Generated {
                    anchor: Some(utf8_range(original, 4, 4)),
                },
            },
            fleximark_plugin_sdk::EditMapSegment {
                output_start: 3,
                output_end: 11,
                origin: EditOrigin::Original {
                    ranges: vec![utf8_range(original, 0, 4), utf8_range(original, 6, 10)],
                    primary_range_index: 1,
                },
            },
        ],
    };
    let mapped = remap_provenance_for_test(
        &SourceProvenance::original(utf8_range(output, 5, 10)),
        &map,
        original,
    )
    .unwrap();
    let SourceProvenance::Derived {
        ranges,
        primary_range_index,
        ..
    } = mapped
    else {
        panic!("source-backed output must remain navigable")
    };
    assert_eq!(
        ranges,
        vec![utf8_range(original, 2, 4), utf8_range(original, 6, 9)]
    );
    assert_eq!(primary_range_index, 1);

    let generated = remap_provenance_for_test(
        &SourceProvenance::original(utf8_range(output, 0, 3)),
        &map,
        original,
    )
    .unwrap();
    assert_eq!(
        generated,
        SourceProvenance::Generated {
            anchor: Some(GeneratedAnchor {
                range: utf8_range(original, 4, 4),
                affinity: AnchorAffinity::After,
            }),
            transform: TransformId("plugin-preprocess-v1".to_owned()),
        }
    );
}

#[test]
fn identity_edit_mapping_preserves_every_unicode_half_open_slice() {
    let source = "a🦀éz";
    let boundaries = (0..=source.len())
        .filter(|offset| source.is_char_boundary(*offset))
        .collect::<Vec<_>>();
    let map = PreprocessedSource {
        text: source.to_owned(),
        segments: vec![fleximark_plugin_sdk::EditMapSegment {
            output_start: 0,
            output_end: source.len() as u64,
            origin: EditOrigin::Original {
                ranges: vec![utf8_range(source, 0, source.len())],
                primary_range_index: 0,
            },
        }],
    };
    for &start in &boundaries {
        for &end in boundaries.iter().filter(|end| **end >= start) {
            let mapped = remap_provenance_for_test(
                &SourceProvenance::original(utf8_range(source, start, end)),
                &map,
                source,
            )
            .unwrap();
            let SourceProvenance::Derived { ranges, .. } = mapped else {
                panic!("identity mapping must be derived from original bytes")
            };
            let bytes = ranges
                .iter()
                .flat_map(|range| {
                    source.as_bytes()
                        [range.byte_start.get() as usize..range.byte_end.get() as usize]
                        .iter()
                        .copied()
                })
                .collect::<Vec<_>>();
            assert_eq!(bytes, source.as_bytes()[start..end]);
        }
    }
}

#[test]
fn navigation_selects_the_smallest_deepest_unicode_block_and_round_trips() {
    let session = open("- Héllo\n");
    let selected = session.node_at_source_offset(3).unwrap();
    assert!(session.node_at_source_offset(4).is_none());
    assert!(selected.depth >= 2);
    assert_eq!(selected.source_range.start.encoding, PositionEncoding::Utf8);
    assert!(selected.source_range.byte_start <= 3 && selected.source_range.byte_end >= 5);
    assert_eq!(
        session.source_range_for_node(&selected.node_id),
        Some(selected.clone())
    );
    assert_eq!(
        serde_json::to_value(&selected).unwrap()["sourceRange"]["byteStart"],
        selected.source_range.byte_start.get()
    );
}

#[test]
fn disposing_previews_evicts_their_authoritative_caches() {
    let mut session = open("# preview\n");
    for preview in ["first", "second"] {
        session
            .render_full(PreviewSessionId(preview.into()), &RenderContext::default())
            .unwrap();
    }
    assert_eq!(session.preview_count(), 2);
    assert!(session.dispose_preview(&PreviewSessionId("first".into())));
    assert_eq!(session.preview_count(), 1);
    assert!(session.dispose_preview(&PreviewSessionId("second".into())));
    assert_eq!(session.preview_count(), 0);
}
