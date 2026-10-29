//! Role: Reconcile stable document node identities across source edits.
use std::collections::{HashMap, HashSet};

use fleximark_model::{
    Block, BlockKind, Document, Inline, InlineKind, Node, NodeId, SourceRange, Visitor, VisitorMut,
    syntax::BuiltinSyntaxKind, walk_blocks, walk_blocks_mut,
};
use sha2::{Digest, Sha256};

#[derive(Clone)]
struct IdentityRecord {
    semantic: String,
    structure: std::mem::Discriminant<BlockKind>,
    parent: String,
    range: String,
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
        combined_key(&record.semantic, &record.range)
    });
    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        combined_key(&record.semantic, &record.parent)
    });
    assign_unique(&old, &new, &mut assigned, &mut used_old, |record| {
        combined_key(&record.range, &record.parent)
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
            let mut material = Vec::new();
            for field in [
                current.uri.0.as_bytes(),
                &current.document_version.to_le_bytes(),
                &(index as u64).to_le_bytes(),
                new[index].semantic.as_bytes(),
                new[index].range.as_bytes(),
                &salt.to_le_bytes(),
            ] {
                append_field(&mut material, field);
            }
            let id = NodeId(format!("block-{}", &blake3::hash(&material).to_hex()[..20]));
            if occupied.insert(id.clone()) {
                *slot = Some(id);
                break;
            }
            salt += 1;
        }
    }
    let mut ids = assigned.into_iter().flatten();
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
    struct Identity<'a> {
        ancestors: Vec<String>,
        root: &'a str,
        output: &'a mut Vec<IdentityRecord>,
    }
    impl Visitor for Identity<'_> {
        fn visit_block(&mut self, block: &Block, depth: u32) {
            self.ancestors.truncate(depth as usize);
            let semantic = semantic_key(block);
            self.output.push(IdentityRecord {
                semantic: semantic.clone(),
                structure: std::mem::discriminant(&block.kind),
                parent: self
                    .ancestors
                    .last()
                    .map_or(self.root, String::as_str)
                    .to_owned(),
                range: range_key(&block.range),
                id: block.id.clone(),
            });
            self.ancestors.push(semantic);
        }
    }
    walk_blocks(
        blocks,
        &mut Identity {
            ancestors: Vec::new(),
            root: parent,
            output,
        },
    );
}

fn append_field(output: &mut Vec<u8>, field: &[u8]) {
    output.extend_from_slice(&(field.len() as u64).to_le_bytes());
    output.extend_from_slice(field);
}

fn combined_key(first: &str, second: &str) -> String {
    let mut material = Vec::new();
    field(&mut material, first);
    field(&mut material, second);
    blake3::hash(&material).to_hex().to_string()
}

fn field(output: &mut Vec<u8>, value: &str) {
    append_field(output, value.as_bytes());
}
fn number(output: &mut Vec<u8>, value: u64) {
    append_field(output, &value.to_le_bytes());
}
fn boolean(output: &mut Vec<u8>, value: bool) {
    append_field(output, &[u8::from(value)]);
}
fn optional(output: &mut Vec<u8>, value: Option<&str>) {
    boolean(output, value.is_some());
    if let Some(value) = value {
        field(output, value);
    }
}

fn semantic_key(block: &Block) -> String {
    let mut material = Vec::new();
    walk_blocks(std::slice::from_ref(block), &mut Semantic(&mut material));
    blake3::hash(&material).to_hex().to_string()
}

struct Semantic<'a>(&'a mut Vec<u8>);
impl Visitor for Semantic<'_> {
    fn visit_node(&mut self, node: &Node) {
        field(
            self.0,
            match node {
                Node::Block(_) => "block",
                Node::Inline(_) => "inline",
            },
        );
    }
    fn visit_block(&mut self, block: &Block, _depth: u32) {
        encode_block(block, self.0);
    }
    fn visit_inline(&mut self, inline: &Inline) {
        encode_inline(inline, self.0);
    }
}

fn encode_block(block: &Block, output: &mut Vec<u8>) {
    match &block.kind {
        BlockKind::Paragraph => field(output, "paragraph"),
        BlockKind::Heading { level } => {
            field(output, "heading");
            number(output, u64::from(*level));
        }
        BlockKind::Quote => field(output, "quote"),
        BlockKind::List {
            ordered,
            start,
            tight,
        } => {
            field(output, "list");
            boolean(output, *ordered);
            number(output, *start);
            boolean(output, *tight);
        }
        BlockKind::ListItem { checked } => {
            field(output, "list-item");
            boolean(output, checked.is_some());
            if let Some(value) = checked {
                boolean(output, *value);
            }
        }
        BlockKind::Table => field(output, "table"),
        BlockKind::TableRow { header } => {
            field(output, "table-row");
            boolean(output, *header);
        }
        BlockKind::TableCell { header } => {
            field(output, "table-cell");
            boolean(output, *header);
        }
        BlockKind::CodeBlock {
            language,
            title,
            line_numbers,
            code,
        } => {
            field(output, "code-block");
            optional(output, language.as_deref());
            optional(output, title.as_deref());
            boolean(output, *line_numbers);
            field(output, code);
        }
        BlockKind::Mermaid { source } => {
            field(output, BuiltinSyntaxKind::Mermaid.render_kind());
            field(output, source);
        }
        BlockKind::AbcNotation { source } => {
            field(output, BuiltinSyntaxKind::Abc.render_kind());
            field(output, source);
        }
        BlockKind::Math { source } => {
            field(output, "math");
            field(output, source);
        }
        BlockKind::ThematicBreak => field(output, "thematic-break"),
        BlockKind::Admonition { kind, title } => {
            field(output, "admonition");
            field(output, kind);
            field(output, title);
        }
        BlockKind::Tabs => field(output, "tabs"),
        BlockKind::Tab { label } => {
            field(output, "tab");
            field(output, label);
        }
        BlockKind::Details { summary } => {
            field(output, "details");
            field(output, summary);
        }
        BlockKind::Media { source } => {
            field(output, "media");
            field(output, source);
        }
        BlockKind::RawHtml { html } => {
            field(output, "raw-html");
            field(output, html);
        }
        BlockKind::Directive { name } => {
            field(output, "directive");
            field(output, name);
        }
    }
    number(output, block.children.len() as u64);
}

fn encode_inline(inline: &Inline, output: &mut Vec<u8>) {
    match &inline.kind {
        InlineKind::Text { value } => {
            field(output, "text");
            field(output, value);
        }
        InlineKind::Code { value } => {
            field(output, "code");
            field(output, value);
        }
        InlineKind::Math { source } => {
            field(output, "math");
            field(output, source);
        }
        InlineKind::SoftBreak => field(output, "soft-break"),
        InlineKind::HardBreak => field(output, "hard-break"),
        InlineKind::RawHtml { html } => {
            field(output, "raw-html");
            field(output, html);
        }
        InlineKind::Emphasis { .. } => field(output, "emphasis"),
        InlineKind::Strong { .. } => field(output, "strong"),
        InlineKind::Strikethrough { .. } => field(output, "strikethrough"),
        InlineKind::Link {
            destination, title, ..
        } => {
            field(output, "link");
            field(output, destination);
            field(output, title);
            field(output, "children");
        }
        InlineKind::Image { source, title, .. } => {
            field(output, "image");
            field(output, source);
            field(output, title);
            field(output, "children");
        }
    }
    if matches!(
        inline.kind,
        InlineKind::Emphasis { .. }
            | InlineKind::Strong { .. }
            | InlineKind::Strikethrough { .. }
            | InlineKind::Link { .. }
            | InlineKind::Image { .. }
    ) {
        number(output, inline.kind.children().len() as u64);
    }
}

fn range_key(range: &SourceRange) -> String {
    let mut material = Vec::new();
    number(&mut material, range.byte_start);
    number(&mut material, range.byte_end);
    blake3::hash(&material).to_hex().to_string()
}

fn apply_ids(blocks: &mut [Block], ids: &mut impl Iterator<Item = NodeId>) {
    struct Ids<'a, I>(&'a mut I);
    impl<I: Iterator<Item = NodeId>> VisitorMut for Ids<'_, I> {
        fn visit_block(&mut self, block: &mut Block, _depth: u32) {
            if let Some(id) = self.0.next() {
                block.id = id;
            }
        }
    }
    walk_blocks_mut(blocks, &mut Ids(ids));
}

pub(super) fn content_hash(source: &str) -> String {
    content_hash_bytes(source.as_bytes())
}

pub(super) fn content_hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
