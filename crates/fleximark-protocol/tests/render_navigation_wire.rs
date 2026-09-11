use fleximark_protocol::{NodeId, RenderNavigationEvent, TextPosition};
use serde_json::json;

#[test]
fn selection_wire_always_includes_null_or_object_active_position() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../test/fixtures/protocol-v1-render-navigation.json"
    ))
    .unwrap();
    let without_active = RenderNavigationEvent::Selection {
        preview_session_id: "preview".into(),
        render_revision: 1,
        node_ids: vec![NodeId("document-root".into())],
        active_position: None,
    };
    let with_active = RenderNavigationEvent::Selection {
        preview_session_id: "preview".into(),
        render_revision: 2,
        node_ids: vec![NodeId("paragraph".into())],
        active_position: Some(TextPosition {
            line: 3,
            character: 5,
        }),
    };

    let expected_without_active = json!({
        "type": "selection",
        "previewSessionId": "preview",
        "renderRevision": 1,
        "nodeIds": ["document-root"],
        "activePosition": null,
    });
    let expected_with_active = json!({
        "type": "selection",
        "previewSessionId": "preview",
        "renderRevision": 2,
        "nodeIds": ["paragraph"],
        "activePosition": { "line": 3, "character": 5 },
    });
    assert_eq!(fixture["schemaVersion"], 1);
    assert_eq!(fixture["events"].as_array().unwrap().len(), 3);
    assert_eq!(
        serde_json::to_value(&without_active).unwrap(),
        fixture["events"][1]
    );
    assert_eq!(
        serde_json::to_value(&with_active).unwrap(),
        fixture["events"][2]
    );
    assert_eq!(fixture["events"][1], expected_without_active);
    assert_eq!(fixture["events"][2], expected_with_active);
    assert_eq!(
        serde_json::to_string(&without_active).unwrap(),
        r#"{"type":"selection","previewSessionId":"preview","renderRevision":1,"nodeIds":["document-root"],"activePosition":null}"#
    );
    assert_eq!(
        serde_json::to_string(&with_active).unwrap(),
        r#"{"type":"selection","previewSessionId":"preview","renderRevision":2,"nodeIds":["paragraph"],"activePosition":{"line":3,"character":5}}"#
    );
}
