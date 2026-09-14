use std::collections::{BTreeMap, HashMap, HashSet};

use fleximark_model::{NavigationEntry, NodeId};
use fleximark_render_html::RenderedBlock;
use fleximark_wire::JsSafeU64;
use serde::{Deserialize, Serialize};

use crate::assets::RenderStyle;
use crate::session::PreviewSessionId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RenderPatch {
    pub preview_session_id: PreviewSessionId,
    pub document_version: JsSafeU64,
    pub base_render_revision: JsSafeU64,
    pub result_render_revision: JsSafeU64,
    pub base_renderer_fingerprint: String,
    pub result_renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub navigation: Vec<NavigationEntry>,
    pub operations: Vec<PatchOperation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct PatchPrecondition {
    pub node_exists: bool,
    pub current_parent_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PatchOperation {
    Insert {
        node_id: NodeId,
        parent_id: String,
        before_id: Option<NodeId>,
        after_id: Option<NodeId>,
        at_end: bool,
        content: String,
        content_node_ids: Vec<NodeId>,
    },
    Remove {
        node_id: NodeId,
        parent_id: String,
        precondition: PatchPrecondition,
    },
    Replace {
        node_id: NodeId,
        parent_id: String,
        content: String,
        content_node_ids: Vec<NodeId>,
        precondition: PatchPrecondition,
    },
    Move {
        node_id: NodeId,
        parent_id: String,
        before_id: Option<NodeId>,
        after_id: Option<NodeId>,
        at_end: bool,
        precondition: PatchPrecondition,
    },
    SetAttributes {
        node_id: NodeId,
        parent_id: String,
        attributes: BTreeMap<String, Option<String>>,
        precondition: PatchPrecondition,
    },
}

pub(super) const ROOT_NODE_ID: &str = "document-root";

pub(super) fn diff_blocks(
    previous: &[RenderedBlock],
    current: &[RenderedBlock],
) -> Vec<PatchOperation> {
    let current_ids = current
        .iter()
        .map(|block| block.id.clone())
        .collect::<HashSet<_>>();
    let previous_html = previous
        .iter()
        .map(|block| (&block.id, block.html.as_str()))
        .collect::<HashMap<_, _>>();
    let mut working = previous
        .iter()
        .map(|block| block.id.clone())
        .collect::<Vec<_>>();
    let mut operations = Vec::new();
    let precondition = || PatchPrecondition {
        node_exists: true,
        current_parent_id: ROOT_NODE_ID.to_owned(),
    };

    for block in previous {
        if !current_ids.contains(&block.id) {
            operations.push(PatchOperation::Remove {
                node_id: block.id.clone(),
                parent_id: ROOT_NODE_ID.to_owned(),
                precondition: precondition(),
            });
            working.retain(|id| id != &block.id);
        }
    }
    for (index, block) in current.iter().enumerate() {
        let position = if working.get(index) == Some(&block.id) {
            Some(index)
        } else {
            working.iter().position(|id| id == &block.id)
        };
        if let Some(position) = position {
            if position != index {
                working.remove(position);
                let before_id = working.get(index).cloned();
                operations.push(PatchOperation::Move {
                    node_id: block.id.clone(),
                    parent_id: ROOT_NODE_ID.to_owned(),
                    before_id,
                    after_id: None,
                    at_end: index == working.len(),
                    precondition: precondition(),
                });
                working.insert(index, block.id.clone());
            }
            if let Some(previous) = previous_html.get(&block.id) {
                if *previous != block.html {
                    if let Some(attributes) = presentation_attribute_delta(previous, &block.html) {
                        operations.push(PatchOperation::SetAttributes {
                            node_id: block.id.clone(),
                            parent_id: ROOT_NODE_ID.to_owned(),
                            attributes,
                            precondition: precondition(),
                        });
                    } else {
                        operations.push(PatchOperation::Replace {
                            node_id: block.id.clone(),
                            parent_id: ROOT_NODE_ID.to_owned(),
                            content: block.html.clone(),
                            content_node_ids: block.node_ids.clone(),
                            precondition: precondition(),
                        });
                    }
                }
            }
        } else {
            let before_id = working.get(index).cloned();
            operations.push(PatchOperation::Insert {
                node_id: block.id.clone(),
                parent_id: ROOT_NODE_ID.to_owned(),
                before_id,
                after_id: None,
                at_end: index == working.len(),
                content: block.html.clone(),
                content_node_ids: block.node_ids.clone(),
            });
            working.insert(index, block.id.clone());
        }
    }
    operations
}

fn presentation_attribute_delta(
    previous: &str,
    current: &str,
) -> Option<BTreeMap<String, Option<String>>> {
    fn root(value: &str) -> Option<(&str, BTreeMap<&str, &str>, &str)> {
        let end = value.find('>')?;
        let opening = value.get(1..end)?;
        let mut parts = opening.split_ascii_whitespace();
        let tag = parts.next()?;
        let mut attributes = BTreeMap::new();
        for part in parts {
            let (name, quoted) = part.split_once('=')?;
            attributes.insert(name, quoted.strip_prefix('"')?.strip_suffix('"')?);
        }
        Some((tag, attributes, &value[end + 1..]))
    }

    let (previous_tag, mut previous_attributes, previous_content) = root(previous)?;
    let (current_tag, mut current_attributes, current_content) = root(current)?;
    if previous_tag != current_tag || previous_content != current_content {
        return None;
    }
    let identity = "data-fleximark-node-id";
    if previous_attributes.remove(identity) != current_attributes.remove(identity) {
        return None;
    }
    let allowed = [
        "role",
        "aria-checked",
        "open",
        "class",
        "data-line-numbers",
        "data-admonition-kind",
    ];
    let mut delta = BTreeMap::new();
    for name in previous_attributes
        .keys()
        .chain(current_attributes.keys())
        .copied()
        .collect::<HashSet<_>>()
    {
        if previous_attributes.get(name) != current_attributes.get(name) {
            if !allowed.contains(&name) {
                return None;
            }
            delta.insert(
                name.to_owned(),
                current_attributes
                    .get(name)
                    .map(|value| (*value).to_owned()),
            );
        }
    }
    (!delta.is_empty()).then_some(delta)
}
