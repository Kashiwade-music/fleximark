use fleximark_model::{Block, Node};
use fleximark_plugin_sdk::component::exports::fleximark::plugin::hooks::{
    Guest, Invocation, Response,
};
use fleximark_plugin_sdk::{
    CandidateBlock, CandidateDocument, CandidateIdentity, CandidateNode, HookRequest, HookResponse,
    PreprocessedSource,
};

struct FixturePlugin;

impl Guest for FixturePlugin {
    fn invoke(request: Invocation) -> Result<Response, String> {
        fleximark_plugin_sdk::component::invoke(request, |request| {
            Ok(match request {
                HookRequest::PreprocessSource { text, edit_map, .. } => {
                    HookResponse::PreprocessedSource {
                        candidate: PreprocessedSource {
                            text,
                            segments: edit_map,
                        },
                    }
                }
                HookRequest::TransformDocument { document } => HookResponse::Document {
                    candidate: CandidateDocument::from_document(&document),
                },
                HookRequest::TransformBlock { block, .. } => HookResponse::Block {
                    candidate: candidate_block(block),
                },
                HookRequest::ExtendRenderModel { .. } => HookResponse::RenderAnnotations {
                    annotations: [("data-fixture-plugin".to_owned(), "active".to_owned())]
                        .into_iter()
                        .collect(),
                },
                HookRequest::UnsafeExportHtml { html, .. } => {
                    if html == "__spin__" {
                        loop {
                            core::hint::spin_loop();
                        }
                    }
                    if html == "__memory__" {
                        let allocation = vec![1_u8; 128 * 1024 * 1024];
                        core::hint::black_box(&allocation);
                        core::mem::forget(allocation);
                    }
                    if html == "__filesystem__" {
                        let entries = std::fs::read_dir("/")
                            .map(|entries| entries.count())
                            .unwrap_or_default();
                        return Ok(HookResponse::RenderAnnotations {
                            annotations: [("root-entry-count".to_owned(), entries.to_string())]
                                .into_iter()
                                .collect(),
                        });
                    }
                    HookResponse::UnsafeExportHtml { html }
                }
            })
        })
    }
}

fn candidate_block(block: Block) -> CandidateBlock {
    CandidateBlock {
        identity: CandidateIdentity::Existing { id: block.id },
        provenance: block.provenance,
        kind: block.kind,
        attributes: block.attributes,
        children: block
            .children
            .into_iter()
            .map(|node| match node {
                Node::Block(block) => CandidateNode::Block(candidate_block(block)),
                Node::Inline(inline) => CandidateNode::Inline(inline),
            })
            .collect(),
    }
}

fleximark_plugin_sdk::component::export!(FixturePlugin with_types_in fleximark_plugin_sdk::component);
