//! Role: Behavioral verification of document processing.
use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use fleximark_model::{
    AnchorAffinity, DocumentUri, GeneratedAnchor, PositionEncoding, SourceProvenance, TransformId,
};
use fleximark_plugin_api::CancellationToken;
use fleximark_plugin_api::{EditOrigin, PreprocessedSource};
use fleximark_render_html::{HtmlTarget, RenderContext};

use crate::identity::content_hash_bytes;
use crate::provenance::remap_provenance_for_test;
use crate::{
    DocumentEngine, EngineError, RenderConfig, RenderSnapshot, RenderStyle, ResolvedRenderAsset,
};

fn utf8_range(source: &str, byte_start: usize, byte_end: usize) -> fleximark_model::SourceRange {
    fleximark_model::SourceIndex::new(source)
        .range(byte_start..byte_end)
        .unwrap()
}

fn open(source: &str) -> DocumentEngine {
    DocumentEngine::open(
        DocumentUri("file:///test.md".into()),
        1,
        source.to_owned(),
        PositionEncoding::Utf16,
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
    session
        .change_full_text_with_cancellation(
            2,
            "# Intro\n\n# A\n\n# B\n".into(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(session.document.blocks[1].id, old[0]);
    assert_eq!(session.document.blocks[2].id, old[1]);

    let mut duplicates = open("same\n");
    let old_id = duplicates.document.blocks[0].id.clone();
    duplicates
        .change_full_text_with_cancellation(
            2,
            "# lead\n\nsame\n\nsame\n".into(),
            &CancellationToken::default(),
        )
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
        session.change_full_text_with_cancellation(
            1,
            "stale\n".into(),
            &CancellationToken::default(),
        ),
        Err(EngineError::StaleVersion { .. })
    ));
    assert!(session.is_out_of_sync());
    assert!(matches!(
        session.render(&RenderContext::default()),
        Err(EngineError::ContentModified)
    ));
    session
        .resynchronize_with_cancellation(1, "fresh\n".into(), &CancellationToken::default())
        .unwrap();
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
        .change_full_text_with_cancellation(
            2,
            ":::tip[Note]\nbody\n:::\n".to_owned(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(session.document.blocks[0].id, original_id);
    let frame = render_default(&mut session, "attributes");
    assert!(
        frame.blocks[0]
            .html
            .contains("data-admonition-kind=\"tip\"")
    );
    let wire = serde_json::to_value(&frame.blocks[0]).unwrap();
    assert_eq!(wire["id"], original_id.0);
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
    engine
        .change_full_text_with_cancellation(2, "# Updated\n".into(), &CancellationToken::default())
        .unwrap();
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
    let wire = serde_json::to_string(&first).unwrap();
    assert!(!wire.contains("private/diagram.png"));

    session
        .change_full_text_with_cancellation(
            2,
            "![changed](private/diagram.png)\n".to_owned(),
            &CancellationToken::default(),
        )
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
fn render_metadata_and_annotations_are_part_of_the_publication() {
    let mut session = open("# A\n");
    let first = render_default(&mut session, "configured");

    session.render_config.style = Some(RenderStyle::from_validated_css(
        "main { color: rebeccapurple; }".to_owned(),
    ));
    let second = render_default(&mut session, "configured");
    assert_ne!(first.renderer_fingerprint, second.renderer_fingerprint);
    assert_eq!(second.style, session.render_config.style);
    let wire = serde_json::to_value(&second).unwrap();
    assert_eq!(wire["style"]["css"], "main { color: rebeccapurple; }");
    assert!(wire["style"]["fingerprint"].is_string());

    let annotations = BTreeMap::from([("plugin".to_owned(), "</script><img src=x>".to_owned())]);
    let annotated = session
        .render_internal_for_test(&RenderContext::default(), &annotations)
        .unwrap();
    assert!(
        annotated
            .html()
            .contains("data-fleximark-render-annotations")
    );
    assert!(annotated.html().contains("\\u003c/script\\u003e"));
    assert!(!annotated.html().contains("</script><img"));
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
            fleximark_plugin_api::EditMapSegment {
                output_start: 0,
                output_end: 3,
                origin: EditOrigin::Generated {
                    anchor: Some(utf8_range(original, 4, 4)),
                },
            },
            fleximark_plugin_api::EditMapSegment {
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
        segments: vec![fleximark_plugin_api::EditMapSegment {
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
                    source.as_bytes()[range.byte_start as usize..range.byte_end as usize]
                        .iter()
                        .copied()
                })
                .collect::<Vec<_>>();
            assert_eq!(bytes, source.as_bytes()[start..end]);
        }
    }
}

#[test]
fn navigation_selects_the_smallest_deepest_unicode_block() {
    let session = open("- Héllo\n");
    let selected = session.node_at_source_offset(3).unwrap();
    assert!(session.node_at_source_offset(4).is_none());
    assert!(selected.depth >= 2);
    assert!(selected.source_range.byte_start <= selected.source_range.byte_end);
    assert!(selected.source_range.byte_start <= 3 && selected.source_range.byte_end >= 5);
    assert_eq!(
        serde_json::to_value(&selected).unwrap()["sourceRange"]["byteStart"],
        selected.source_range.byte_start
    );
}
