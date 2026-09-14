use fleximark_protocol::{NodeId, RenderNavigationEvent, TextPosition};

#[test]
fn selection_wire_always_includes_null_or_object_active_position() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../test/fixtures/protocol-v1-render-navigation.json"
    ))
    .unwrap();
    let without_active = RenderNavigationEvent::Selection {
        preview_session_id: "preview".into(),
        render_revision: 1.into(),
        node_ids: vec![NodeId("document-root".into())],
        active_position: None,
    };
    let with_active = RenderNavigationEvent::Selection {
        preview_session_id: "preview".into(),
        render_revision: 2.into(),
        node_ids: vec![NodeId("paragraph".into())],
        active_position: Some(TextPosition {
            line: 3.into(),
            character: 5.into(),
        }),
    };

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
}
