use std::collections::{HashMap, HashSet};

use fleximark_model::{
    Block, BlockKind, Document, Inline, InlineKind, Node, NodeId, SourceProvenance,
};
use sha2::{Digest, Sha256};

#[derive(Clone)]
struct IdentityRecord {
    semantic: String,
    structure: std::mem::Discriminant<BlockKind>,
    parent: String,
    provenance: String,
    id: NodeId,
}

pub(super) fn reconcile_node_ids(previous: Option<&Document>, current: &mut Document) {
    let mut old = Vec::new();
    if let Some(previous) = previous {
        collect_identity(&previous.blocks, "root", &mut old);
    }
    let mut new = Vec::new();
    collect_identity(&current.blocks, "root", &mut new);
    let mut assigned = vec![None; new.len()];
    let mut used_old = HashSet::new();

    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        format!("{}\0{}", record.semantic, record.provenance)
    });
    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        format!("{}\0{}", record.semantic, record.parent)
    });
    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        format!("{}\0{}", record.provenance, record.parent)
    });
    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        record.semantic.clone()
    });
    for (new_index, candidate) in new.iter().enumerate() {
        if assigned[new_index].is_some() {
            continue;
        }
        let available_old = old
            .iter()
            .enumerate()
            .filter(|(old_index, previous)| {
                !used_old.contains(old_index)
                    && previous.structure == candidate.structure
                    && previous.parent == candidate.parent
            })
            .count();
        let available_new = new
            .iter()
            .enumerate()
            .filter(|(index, next)| {
                assigned[*index].is_none()
                    && next.structure == candidate.structure
                    && next.parent == candidate.parent
            })
            .count();
        if available_old != available_new {
            continue;
        }
        if let Some((old_index, previous)) = old.iter().enumerate().find(|(old_index, previous)| {
            !used_old.contains(old_index)
                && previous.structure == candidate.structure
                && previous.parent == candidate.parent
        }) {
            assigned[new_index] = Some(previous.id.clone());
            used_old.insert(old_index);
        }
    }

    let mut occupied = old
        .iter()
        .map(|record| record.id.clone())
        .collect::<HashSet<_>>();
    for (index, slot) in assigned.iter_mut().enumerate() {
        if slot.is_some() {
            continue;
        }
        let mut salt = 0_u64;
        loop {
            let material = format!(
                "{}\0{}\0{index}\0{}\0{}\0{salt}",
                current.uri.0, current.document_version, new[index].semantic, new[index].provenance
            );
            let id = NodeId(format!(
                "block-{}",
                &blake3::hash(material.as_bytes()).to_hex()[..20]
            ));
            if occupied.insert(id.clone()) {
                *slot = Some(id);
                break;
            }
            salt += 1;
        }
    }
    let mut ids = assigned.into_iter().map(Option::unwrap);
    apply_ids(&mut current.blocks, &mut ids);
}

fn assign_unique(
    old: &[IdentityRecord],
    new: &[IdentityRecord],
    assigned: &mut [Option<NodeId>],
    used_old: &mut HashSet<usize>,
    key: impl Fn(&IdentityRecord) -> String,
) {
    let mut old_groups: HashMap<String, Vec<usize>> = HashMap::new();
    let mut new_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, record) in old
        .iter()
        .enumerate()
        .filter(|(index, _)| !used_old.contains(index))
    {
        old_groups.entry(key(record)).or_default().push(index);
    }
    for (index, record) in new
        .iter()
        .enumerate()
        .filter(|(index, _)| assigned[*index].is_none())
    {
        new_groups.entry(key(record)).or_default().push(index);
    }
    for (key, old_indexes) in old_groups {
        let Some(new_indexes) = new_groups.get(&key) else {
            continue;
        };
        if old_indexes.len() == 1 && new_indexes.len() == 1 {
            let old_index = old_indexes[0];
            assigned[new_indexes[0]] = Some(old[old_index].id.clone());
            used_old.insert(old_index);
        }
    }
}

fn collect_identity(blocks: &[Block], parent: &str, output: &mut Vec<IdentityRecord>) {
    for block in blocks {
        let semantic = semantic_key(block);
        output.push(IdentityRecord {
            semantic: semantic.clone(),
            structure: std::mem::discriminant(&block.kind),
            parent: parent.to_owned(),
            provenance: provenance_key(&block.provenance),
            id: block.id.clone(),
        });
        for child in &block.children {
            if let Node::Block(child) = child {
                collect_identity(std::slice::from_ref(child), &semantic, output);
            }
        }
    }
}

fn semantic_key(block: &Block) -> String {
    let mut material = format!("{:?}\0{:?}", block.kind, block.attributes);
    for child in &block.children {
        match child {
            Node::Block(block) => material.push_str(&semantic_key(block)),
            Node::Inline(inline) => append_inline_semantics(inline, &mut material),
        }
        material.push('\0');
    }
    blake3::hash(material.as_bytes()).to_hex().to_string()
}

fn append_inline_semantics(inline: &Inline, output: &mut String) {
    match &inline.kind {
        InlineKind::Text { value } => {
            output.push_str("text:");
            output.push_str(value);
        }
        InlineKind::Code { value } => {
            output.push_str("code:");
            output.push_str(value);
        }
        InlineKind::Math { source } => {
            output.push_str("math:");
            output.push_str(source);
        }
        InlineKind::SoftBreak => output.push_str("soft-break"),
        InlineKind::HardBreak => output.push_str("hard-break"),
        InlineKind::RawHtml { html } => {
            output.push_str("raw:");
            output.push_str(html);
        }
        InlineKind::Emphasis { children } => append_nested("em", children, output),
        InlineKind::Strong { children } => append_nested("strong", children, output),
        InlineKind::Strikethrough { children } => append_nested("strike", children, output),
        InlineKind::Link {
            destination,
            title,
            children,
        } => {
            output.push_str("link:");
            output.push_str(destination);
            output.push('\0');
            output.push_str(title);
            append_nested("", children, output);
        }
        InlineKind::Image {
            source,
            title,
            children,
        } => {
            output.push_str("image:");
            output.push_str(source);
            output.push('\0');
            output.push_str(title);
            append_nested("", children, output);
        }
    }
}

fn append_nested(label: &str, children: &[Inline], output: &mut String) {
    output.push_str(label);
    output.push('[');
    for child in children {
        append_inline_semantics(child, output);
        output.push('\0');
    }
    output.push(']');
}

fn provenance_key(provenance: &SourceProvenance) -> String {
    serde_json::to_string(provenance).expect("source provenance is serializable")
}

fn apply_ids(blocks: &mut [Block], ids: &mut impl Iterator<Item = NodeId>) {
    for block in blocks {
        block.id = ids.next().expect("identity record count matches blocks");
        for child in &mut block.children {
            if let Node::Block(child) = child {
                apply_ids(std::slice::from_mut(child), ids);
            }
        }
    }
}

pub(super) fn content_hash(source: &str) -> String {
    content_hash_bytes(source.as_bytes())
}

pub(super) fn content_hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
