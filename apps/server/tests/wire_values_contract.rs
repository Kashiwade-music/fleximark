//! Role: Compare model and wire acceptance of hashes and asset references.
use fleximark_model::{AssetRef, Sha256Hex};
use fleximark_protocol::{RenderAsset, RenderStyle};
use serde_json::json;
fn asset(reference: &str, hash: &str) -> serde_json::Value {
    json!({"reference":reference,"mediaType":"image/png","contentHash":hash,"byteLength":0,"data":""})
}
#[test]
fn model_and_wire_hashes_accept_the_same_canonical_strings() {
    let valid = "0123456789abcdef".repeat(4);
    for value in [
        valid.clone(),
        "a".repeat(64),
        "A".repeat(64),
        "\u{e9}".repeat(32),
        "a".repeat(63),
        "a".repeat(65),
        format!("{}\0", "a".repeat(63)),
        String::new(),
        "g".repeat(64),
    ] {
        let expected = Sha256Hex::try_from(value.as_str()).is_ok();
        let style = serde_json::from_value::<RenderStyle>(json!({"css":"","fingerprint":value}));
        assert_eq!(style.is_ok(), expected, "style fingerprint: {value:?}");
        let render_asset = serde_json::from_value::<RenderAsset>(asset(
            &format!("fleximark-asset:{valid}"),
            &value,
        ));
        assert_eq!(
            render_asset.is_ok(),
            expected,
            "asset content hash: {value:?}"
        );
    }
}
#[test]
fn model_and_wire_asset_references_accept_the_same_canonical_strings() {
    let valid = "0123456789abcdef".repeat(4);
    let mut references = vec![
        String::new(),
        valid.clone(),
        format!("FLEXIMARK-ASSET:{valid}"),
    ];
    for hash in [
        valid.clone(),
        "A".repeat(64),
        "\u{e9}".repeat(32),
        "a".repeat(63),
        "a".repeat(65),
        format!("{}\0", "a".repeat(63)),
        String::new(),
    ] {
        references.push(format!("fleximark-asset:{hash}"));
    }
    for value in references {
        let expected = AssetRef::try_from(value.as_str()).is_ok();
        let actual = serde_json::from_value::<RenderAsset>(asset(&value, &valid)).is_ok();
        assert_eq!(actual, expected, "asset reference: {value:?}");
    }
}
