//! Role: Verify deterministic fixture hook output on the native test host.
use crate::hooks::invoke;
use fleximark_model::{
    Block, BlockKind, Document, DocumentMetadata, DocumentUri, Node, NodeId, SourceProvenance,
    TransformId,
};
use fleximark_plugin_sdk::{CandidateIdentity, CandidateNode, HookRequest, HookResponse};

fn document() -> Document {
    Document {
        schema_version: 1,
        document_version: 7,
        uri: DocumentUri("file:///fixture.md".into()),
        metadata: DocumentMetadata::default(),
        blocks: vec![],
    }
}

fn block(id: &str, children: Vec<Node>) -> Block {
    Block {
        id: NodeId(id.into()),
        provenance: SourceProvenance::Generated {
            anchor: None,
            transform: TransformId("fixture".into()),
        },
        kind: BlockKind::Paragraph,
        attributes: Default::default(),
        children,
    }
}

#[test]
fn preprocessing_preserves_source_and_edit_map() {
    let response = invoke(HookRequest::PreprocessSource {
        document_version: 7,
        text: "# unchanged α\n".into(),
        edit_map: vec![],
    })
    .unwrap();
    let HookResponse::PreprocessedSource { candidate } = response else {
        panic!("fixture returned another hook response")
    };
    assert_eq!(candidate.text, "# unchanged α\n");
    assert!(candidate.segments.is_empty());
}

#[test]
fn document_and_nested_block_transforms_preserve_candidate_identity() {
    let original = document();
    let response = invoke(HookRequest::TransformDocument {
        document: original.clone(),
    })
    .unwrap();
    let HookResponse::Document { candidate } = response else {
        panic!("fixture returned another hook response")
    };
    assert_eq!(candidate.document_version, original.document_version);
    assert_eq!(candidate.uri, original.uri);
    let response = invoke(HookRequest::TransformBlock {
        document_version: 7,
        block: block("parent", vec![Node::Block(block("child", vec![]))]),
    })
    .unwrap();
    let HookResponse::Block { candidate } = response else {
        panic!("fixture returned another hook response")
    };
    assert_eq!(
        candidate.identity,
        CandidateIdentity::Existing {
            id: NodeId("parent".into())
        }
    );
    let CandidateNode::Block(child) = &candidate.children[0] else {
        panic!("nested block was not preserved")
    };
    assert_eq!(
        child.identity,
        CandidateIdentity::Existing {
            id: NodeId("child".into())
        }
    );
}

#[test]
fn render_extension_and_normal_export_preserve_existing_behavior() {
    let response = invoke(HookRequest::ExtendRenderModel {
        document: document(),
        target: "preview".into(),
    })
    .unwrap();
    let HookResponse::RenderAnnotations { annotations } = response else {
        panic!("fixture returned another hook response")
    };
    assert_eq!(annotations["data-fixture-plugin"], "active");
    let response = invoke(HookRequest::UnsafeExportHtml {
        document_version: 7,
        html: "<p>unchanged</p>".into(),
    })
    .unwrap();
    assert_eq!(
        response,
        HookResponse::UnsafeExportHtml {
            html: "<p>unchanged</p>".into()
        }
    );
}
