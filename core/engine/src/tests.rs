//! Role: Behavioral verification of document processing.
use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use fleximark_model::{DocumentUri, PositionEncoding};
use fleximark_render_html::{HtmlTarget, RenderContext};

use crate::identity::content_hash_bytes;
use crate::{
    DocumentEngine, EngineError, RenderConfig, RenderSnapshot, RenderStyle, ResolvedRenderAsset,
};

fn open(source: &str) -> DocumentEngine {
    DocumentEngine::open(
        DocumentUri("file:///test.md".into()),
        1,
        source.to_owned(),
        PositionEncoding::Utf16,
        RenderConfig::default(),
    )
    .unwrap()
}

fn render_default(session: &mut DocumentEngine, _id: &str) -> RenderSnapshot {
    session.render(&RenderContext::default()).unwrap()
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
    session.change(2, "# Intro\n\n# A\n\n# B\n".into()).unwrap();
    assert_eq!(session.document.blocks[1].id, old[0]);
    assert_eq!(session.document.blocks[2].id, old[1]);

    let mut duplicates = open("same\n");
    let old_id = duplicates.document.blocks[0].id.clone();
    duplicates
        .change(2, "# lead\n\nsame\n\nsame\n".into())
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
        session.change(1, "stale\n".into(),),
        Err(EngineError::StaleVersion { .. })
    ));
    assert!(session.is_out_of_sync());
    assert!(matches!(
        session.render(&RenderContext::default()),
        Err(EngineError::ContentModified)
    ));
    session.resync(1, "fresh\n".into()).unwrap();
    assert!(!session.is_out_of_sync());
    assert_eq!(session.document().document_version, 1);
    assert_eq!(session.source(), "fresh\n");
}

#[test]
fn frame_carries_updated_block_html() {
    let mut session = open(":::info[Note]\nbody\n:::\n");
    render_default(&mut session, "attributes");
    let original_id = session.document.blocks[0].id.clone();
    session
        .change(2, ":::tip[Note]\nbody\n:::\n".to_owned())
        .unwrap();
    assert_eq!(session.document.blocks[0].id, original_id);
    let frame = render_default(&mut session, "attributes");
    assert!(
        frame.blocks[0]
            .html
            .contains("data-admonition-kind=\"tip\"")
    );
    assert_eq!(frame.blocks[0].id, original_id);
}

#[test]
fn snapshots_are_independent_document_results_with_navigation() {
    let mut engine = open("# A\n");
    let first = engine.render(&RenderContext::default()).unwrap();
    assert_eq!(first.document_version, 1);
    assert_eq!(first.navigation[0].node_id, first.blocks[0].id);
    assert!(first.blocks[0].node_ids.contains(&first.blocks[0].id));
    assert_eq!(first.renderer_fingerprint.len(), 64);
    assert!(
        first
            .html()
            .starts_with("<main data-fleximark-node-id=\"document-root\">")
    );
    engine.change(2, "# Updated\n".into()).unwrap();
    let second = engine.render(&RenderContext::default()).unwrap();
    assert_eq!(first.document_version, 1);
    assert_eq!(second.document_version, 2);
    assert_eq!(first.blocks[0].id, second.blocks[0].id);
    assert_ne!(first.blocks[0].html, second.blocks[0].html);
}

#[test]
fn resolved_assets_are_published_and_fingerprinted_without_source_paths() {
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
    let context = session.render_config.context.clone();
    let first = session.render(&context).unwrap();
    assert_eq!(first.assets.len(), 1);
    assert_eq!(first.assets[0].data, BASE64.encode(b"png-one"));
    assert!(first.html().contains(&first.assets[0].reference));
    let wire = serde_json::to_string(&first.assets).unwrap();
    assert!(!wire.contains("private/diagram.png"));

    session
        .change(2, "![changed](private/diagram.png)\n".to_owned())
        .unwrap();
    let context = session.render_config.context.clone();
    let updated = session.render(&context).unwrap();
    assert_eq!(updated.assets, first.assets);

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
    let second = session.render(&context).unwrap();
    assert_ne!(first.renderer_fingerprint, second.renderer_fingerprint);
    assert_ne!(first.assets[0].content_hash, second.assets[0].content_hash);

    assert!(
        ResolvedRenderAsset::from_validated_bytes(
            "".to_owned(),
            "application/octet-stream".to_owned(),
            b"asset",
        )
        .is_err()
    );
}

#[test]
fn resolved_assets_share_one_publication_across_source_aliases() {
    let first = ResolvedRenderAsset::from_validated_bytes(
        "images/first.png".to_owned(),
        "image/png".to_owned(),
        b"shared-image",
    )
    .unwrap();
    let second = ResolvedRenderAsset::from_validated_bytes(
        "images/second.png".to_owned(),
        "image/png".to_owned(),
        b"shared-image",
    )
    .unwrap();

    let config = RenderConfig::default()
        .with_resolved_assets(vec![first, second])
        .unwrap();

    assert_eq!(config.assets().len(), 1);
    assert_eq!(config.context.resolved_resources.len(), 2);
    assert_eq!(
        config.context.resolved_resources["images/first.png"],
        config.context.resolved_resources["images/second.png"]
    );
}

#[test]
fn resolved_assets_accept_large_content_and_still_validate_integrity() {
    let bytes = vec![0_u8; 9 * 1024 * 1024];
    let asset = ResolvedRenderAsset::from_validated_bytes(
        "large.png".to_owned(),
        "image/png".to_owned(),
        &bytes,
    )
    .unwrap();
    let config = RenderConfig::default()
        .with_resolved_assets(vec![asset.clone()])
        .unwrap();
    let published = config.assets().next().unwrap();
    assert_eq!(published.byte_length, bytes.len() as u64);
    assert_eq!(BASE64.decode(&published.data).unwrap(), bytes);
    assert_eq!(published.content_hash, content_hash_bytes(&bytes));

    let mut corrupted = asset;
    corrupted.published.content_hash = "0".repeat(64);
    assert!(
        RenderConfig::default()
            .with_resolved_assets(vec![corrupted])
            .is_err()
    );
}

#[test]
fn render_configuration_is_transactional_and_preserves_document_identity() -> Result<(), EngineError>
{
    let mut session = open("# A\n");
    let first = render_default(&mut session, "configured");
    let document = session.document().clone();
    let source = session.source().to_owned();
    let config = RenderConfig {
        style: Some(RenderStyle::from_validated_css(
            "main { color: rebeccapurple; }".to_owned(),
        )),
        ..RenderConfig::default()
    };
    session.set_render_config(config.clone())?;
    let second = render_default(&mut session, "configured");
    assert_ne!(first.renderer_fingerprint, second.renderer_fingerprint);
    assert_eq!(second.style, config.style);
    assert_eq!(session.document(), &document);
    assert_eq!(session.source(), source);
    let invalid = RenderConfig {
        config_hash: "invalid".to_owned(),
        ..config.clone()
    };
    assert!(matches!(
        session.set_render_config(invalid),
        Err(EngineError::Configuration(_))
    ));
    assert_eq!(session.render_config(), &config);
    assert_eq!(render_default(&mut session, "configured"), second);
    assert_eq!(session.document(), &document);
    assert_eq!(session.source(), source);
    Ok(())
}

#[test]
fn portable_render_preserves_style_and_sanitizes_html() -> Result<(), EngineError> {
    let mut session = open("hello\n\n<script>alert(1)</script>\n");
    session.render_config.style = Some(RenderStyle::from_validated_css(
        "body { color: green; }".to_owned(),
    ));
    let context = RenderContext {
        target: HtmlTarget::Portable,
        allow_remote_resources: false,
        allow_data_resources: false,
        resolved_resources: BTreeMap::new(),
    };
    let snapshot = session.render(&context)?;
    assert!(snapshot.html().contains("hello"));
    assert!(!snapshot.html().contains("<script>"));
    assert_eq!(
        snapshot.style.as_ref().map(RenderStyle::css),
        Some("body { color: green; }")
    );
    assert!(snapshot.assets.is_empty());
    Ok(())
}

#[test]
fn render_style_rejects_a_fingerprint_that_does_not_match_its_css() {
    let style = RenderStyle::from_validated_css("body { color: red; }".to_owned());
    let mut forged = serde_json::to_value(style).unwrap();
    forged["fingerprint"] = "0".repeat(64).into();
    assert!(serde_json::from_value::<RenderStyle>(forged).is_err());
}

#[test]
fn navigation_selects_the_smallest_deepest_unicode_block() {
    let session = open("- Héllo\n");
    let selected = session.node_at_source_offset(3).unwrap();
    assert!(session.node_at_source_offset(4).is_none());
    assert!(selected.depth >= 2);
    assert!(selected.source_range.byte_start <= selected.source_range.byte_end);
    assert!(selected.source_range.byte_start <= 3 && selected.source_range.byte_end >= 5);
}
