//! Role: Execute and validate sandboxed WASM plugins.
use std::collections::HashSet;

use fleximark_model::{Block, Document, Inline, InlineKind, Node, NodeId};
use fleximark_plugin_sdk::{CandidateBlock, CandidateDocument, CandidateIdentity, CandidateNode};

use crate::runtime_types::ExecutionLimits;

pub(super) fn resolve_document_candidate(
    current: &Document,
    candidate: CandidateDocument,
    plugin_id: &str,
    limits: &ExecutionLimits,
) -> Result<Document, String> {
    if candidate.schema_version != current.schema_version
        || candidate.document_version != current.document_version
        || candidate.uri != current.uri
    {
        return Err("candidate changed immutable document identity or version".to_owned());
    }
    let mut known_ids = HashSet::new();
    collect_ids(&current.blocks, &mut known_ids);
    let mut used_ids = HashSet::new();
    let mut creation_keys = HashSet::new();
    let mut node_count = 0;
    let mut blocks = Vec::with_capacity(candidate.blocks.len());
    for block in candidate.blocks {
        blocks.push(resolve_block(
            block,
            &known_ids,
            &mut used_ids,
            &mut creation_keys,
            plugin_id,
            "document",
            limits,
            1,
            &mut node_count,
        )?);
    }
    let document = Document {
        schema_version: candidate.schema_version,
        document_version: candidate.document_version,
        uri: candidate.uri,
        metadata: candidate.metadata,
        blocks,
    };
    Ok(document)
}

pub(super) fn resolve_block_invocation(
    current: &Document,
    candidate: CandidateBlock,
    plugin_id: &str,
    invocation_scope: &str,
    limits: &ExecutionLimits,
) -> Result<Block, String> {
    let mut known_ids = HashSet::new();
    let mut node_count = 0;
    collect_ids(&current.blocks, &mut known_ids);
    resolve_block(
        candidate,
        &known_ids,
        &mut HashSet::new(),
        &mut HashSet::new(),
        plugin_id,
        invocation_scope,
        limits,
        1,
        &mut node_count,
    )
}

#[allow(clippy::too_many_arguments)]
fn resolve_block(
    candidate: CandidateBlock,
    known_ids: &HashSet<NodeId>,
    used_ids: &mut HashSet<NodeId>,
    creation_keys: &mut HashSet<String>,
    plugin_id: &str,
    invocation_scope: &str,
    limits: &ExecutionLimits,
    depth: usize,
    node_count: &mut usize,
) -> Result<Block, String> {
    *node_count += 1;
    if *node_count > limits.max_nodes || depth > limits.max_depth {
        return Err("candidate exceeds node count or depth limit".to_owned());
    }
    let id = match candidate.identity {
        CandidateIdentity::Existing { id } => {
            if !known_ids.contains(&id) {
                return Err(format!("candidate references unknown NodeId {}", id.0));
            }
            if !used_ids.insert(id.clone()) {
                return Err(format!("candidate duplicates NodeId {}", id.0));
            }
            id
        }
        CandidateIdentity::Created { key } => {
            if key.0.is_empty() || key.0.len() > 128 || !creation_keys.insert(key.0.clone()) {
                return Err(format!(
                    "candidate has invalid or reused CreationKey {}",
                    key.0
                ));
            }
            let mut salt = 0;
            loop {
                let material = format!("{plugin_id}\0{invocation_scope}\0{}\0{salt}", key.0);
                let id = NodeId(format!(
                    "block-{}",
                    &blake3::hash(material.as_bytes()).to_hex()[..20]
                ));
                if !known_ids.contains(&id) && used_ids.insert(id.clone()) {
                    break id;
                }
                salt += 1;
            }
        }
    };
    let mut children = Vec::with_capacity(candidate.children.len());
    for child in candidate.children {
        children.push(match child {
            CandidateNode::Block(block) => Node::Block(resolve_block(
                block,
                known_ids,
                used_ids,
                creation_keys,
                plugin_id,
                invocation_scope,
                limits,
                depth + 1,
                node_count,
            )?),
            CandidateNode::Inline(inline) => {
                validate_inline_budget(&inline, depth + 1, node_count, limits)?;
                Node::Inline(inline)
            }
        });
    }
    Ok(Block {
        id,
        provenance: candidate.provenance,
        kind: candidate.kind,
        attributes: candidate.attributes,
        children,
    })
}

fn collect_ids(blocks: &[Block], output: &mut HashSet<NodeId>) {
    for block in blocks {
        output.insert(block.id.clone());
        for child in &block.children {
            if let Node::Block(block) = child {
                collect_ids(std::slice::from_ref(block), output);
            }
        }
    }
}

fn validate_inline_budget(
    inline: &Inline,
    depth: usize,
    node_count: &mut usize,
    limits: &ExecutionLimits,
) -> Result<(), String> {
    let mut pending = vec![(inline, depth)];
    while let Some((inline, depth)) = pending.pop() {
        *node_count += 1;
        if *node_count > limits.max_nodes || depth > limits.max_depth {
            return Err("candidate exceeds node count or depth limit".to_owned());
        }
        let children = match &inline.kind {
            InlineKind::Emphasis { children }
            | InlineKind::Strong { children }
            | InlineKind::Strikethrough { children }
            | InlineKind::Link { children, .. }
            | InlineKind::Image { children, .. } => children,
            _ => continue,
        };
        pending.extend(children.iter().map(|child| (child, depth + 1)));
    }
    Ok(())
}

pub(super) fn validate_document_budget(
    document: &Document,
    limits: &ExecutionLimits,
) -> Result<(), String> {
    let mut count = 0;
    let mut pending: Vec<_> = document.blocks.iter().map(|block| (block, 1)).collect();
    while let Some((block, depth)) = pending.pop() {
        count += 1;
        if count > limits.max_nodes || depth > limits.max_depth {
            return Err("candidate exceeds node count or depth limit".to_owned());
        }
        for child in &block.children {
            match child {
                Node::Block(block) => pending.push((block, depth + 1)),
                Node::Inline(inline) => {
                    validate_inline_budget(inline, depth + 1, &mut count, limits)?
                }
            }
        }
    }
    Ok(())
}
