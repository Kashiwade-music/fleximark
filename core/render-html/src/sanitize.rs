//! Role: Sanitize author HTML while protecting renderer-owned fragments and identities.

use crate::escaping::escape_attribute;
use crate::sequences::inline_run;
use crate::types::{BlockMetadata, RenderError, RenderedBlock};
use fleximark_model::{Block, BlockKind, Inline, Node, NodeId};
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Selects the inert placeholder tag required by the surrounding HTML grammar.
pub(crate) enum FragmentMode {
    Block,
    Inline,
}

struct ProtectedFragment {
    marker: String,
    html: String,
}

pub(crate) struct ProtectedFragments {
    mode: FragmentMode,
    candidate: String,
    fragments: Vec<ProtectedFragment>,
}

impl ProtectedFragments {
    pub(crate) fn new(mode: FragmentMode) -> Self {
        Self {
            mode,
            candidate: String::new(),
            fragments: Vec::new(),
        }
    }

    pub(crate) fn raw(&mut self, html: &str) {
        self.candidate.push_str(&neutralize_internal_markers(html));
    }

    pub(crate) fn boundary(&mut self, index: usize) {
        self.candidate.push_str(&block_marker(index));
    }

    pub(crate) fn protect(&mut self, html: String) {
        let index = self.fragments.len();
        let marker = match self.mode {
            FragmentMode::Block => safe_block_marker(index),
            FragmentMode::Inline => format!(
                "<{INTERNAL_FRAGMENT_TAG} data-index=\"{index}\"></{INTERNAL_FRAGMENT_TAG}>"
            ),
        };
        self.candidate.push_str(&marker);
        self.fragments.push(ProtectedFragment { marker, html });
    }

    pub(crate) fn sanitize(&self) -> String {
        clean_raw_html(&self.candidate)
    }

    pub(crate) fn restore(&self, mut sanitized: String) -> String {
        for fragment in &self.fragments {
            sanitized = sanitized.replace(&fragment.marker, &fragment.html);
        }
        sanitized
    }

    pub(crate) fn finish(self) -> String {
        self.restore(self.sanitize())
    }
}

struct ProtectedBlock<'a> {
    block: &'a Block,
    node_ids: Vec<NodeId>,
    navigation: Vec<fleximark_model::NavigationEntry>,
    html: Option<String>,
}

pub(crate) fn render_sanitized_block_sequence(
    blocks: &[Block],
    metadata: Vec<BlockMetadata>,
    render_block: impl Fn(&Block) -> Result<String, RenderError>,
) -> Result<Vec<RenderedBlock>, RenderError> {
    let mut rendered = Vec::with_capacity(blocks.len());
    let mut fragments = ProtectedFragments::new(FragmentMode::Block);
    for (index, (block, metadata)) in blocks.iter().zip(metadata).enumerate() {
        let BlockMetadata {
            node_ids,
            navigation,
        } = metadata;
        let html = if matches!(block.kind, BlockKind::RawHtml { .. }) {
            None
        } else {
            Some(render_block(block)?)
        };
        rendered.push(ProtectedBlock {
            block,
            node_ids,
            navigation,
            html,
        });
        fragments.boundary(index);
        match &block.kind {
            BlockKind::RawHtml { html } => fragments.raw(html),
            _ => fragments.protect(rendered[index].html.clone().unwrap_or_default()),
        }
    }

    let sanitized = fragments.sanitize();
    let starts = top_level_block_marker_offsets(&sanitized);
    if starts.is_empty() {
        // Every sequence begins with a protected marker. An empty set means an
        // internal invariant was violated rather than author-controlled input.
        return Err(RenderError::InvalidRawHtmlBoundary);
    }

    let mut output = Vec::with_capacity(starts.len());
    for (group_index, start) in starts.iter().copied().enumerate() {
        let end = starts
            .get(group_index + 1)
            .copied()
            .unwrap_or(sanitized.len());
        let mut group = sanitized[start..end].to_owned();
        let indices = (0..blocks.len())
            .filter(|index| group.contains(&block_marker(*index)))
            .collect::<Vec<_>>();
        let Some(&root_index) = indices.first() else {
            return Err(RenderError::InvalidRawHtmlBoundary);
        };

        if indices.len() == 1 {
            if let Some(html) = &rendered[root_index].html {
                output.push(RenderedBlock {
                    id: rendered[root_index].block.id.clone(),
                    node_ids: rendered[root_index].node_ids.clone(),
                    navigation: rendered[root_index].navigation.clone(),
                    html: html.clone(),
                });
                continue;
            }
        }

        let mut node_ids = Vec::new();
        let mut navigation = Vec::new();
        for &index in &indices {
            node_ids.extend(rendered[index].node_ids.iter().cloned());
            navigation.extend(rendered[index].navigation.iter().cloned());
            let identity = if index == root_index {
                String::new()
            } else if rendered[index].html.is_none() {
                format!(
                    "<span data-fleximark-node-id=\"{}\" data-fleximark-kind=\"raw-html-boundary\" hidden></span>",
                    escape_attribute(&rendered[index].block.id.0)
                )
            } else {
                String::new()
            };
            group = group.replace(&block_marker(index), &identity);
        }
        group = fragments.restore(group);
        output.push(RenderedBlock {
            id: rendered[root_index].block.id.clone(),
            node_ids,
            navigation,
            html: format!(
                "<div data-fleximark-node-id=\"{}\" data-fleximark-kind=\"raw-html-container\">{group}</div>\n",
                escape_attribute(&rendered[root_index].block.id.0)
            ),
        });
    }
    Ok(output)
}

fn block_marker(index: usize) -> String {
    format!("<{INTERNAL_BLOCK_MARKER_TAG} data-index=\"{index}\"></{INTERNAL_BLOCK_MARKER_TAG}>")
}

fn safe_block_marker(index: usize) -> String {
    format!("<{INTERNAL_SAFE_BLOCK_TAG} data-index=\"{index}\"></{INTERNAL_SAFE_BLOCK_TAG}>")
}

fn top_level_block_marker_offsets(html: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut depth = 0usize;
    let mut cursor = 0usize;
    while let Some(relative) = html[cursor..].find('<') {
        let start = cursor + relative;
        let Some(end) = find_html_tag_end(html, start + 1) else {
            break;
        };
        let token = &html[start + 1..end];
        if let Some(closing) = token.strip_prefix('/') {
            let name = closing.split_ascii_whitespace().next().unwrap_or("");
            if !name.is_empty() {
                depth = depth.saturating_sub(1);
            }
        } else {
            let name = token
                .split_ascii_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('/');
            if depth == 0 && name.eq_ignore_ascii_case(INTERNAL_BLOCK_MARKER_TAG) {
                offsets.push(start);
            }
            let is_void = matches!(name, "br" | "col" | "hr" | "wbr") || token.ends_with('/');
            if !name.is_empty() && !is_void {
                depth += 1;
            }
        }
        cursor = end + 1;
    }
    offsets
}

fn find_html_tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (relative, character) in html[start..].char_indices() {
        match (quote, character) {
            (Some(active), current) if current == active => quote = None,
            (None, current @ ('\'' | '"')) => quote = Some(current),
            (None, '>') => return Some(start + relative),
            _ => {}
        }
    }
    None
}

pub(crate) fn render_sanitized_nodes(
    nodes: &[Node],
    render_block: impl Fn(&Block) -> Result<String, RenderError>,
    render_inlines: impl Fn(&[&Inline]) -> Result<String, RenderError>,
) -> Result<String, RenderError> {
    let mut fragments = ProtectedFragments::new(FragmentMode::Block);
    let mut index = 0;
    while index < nodes.len() {
        match &nodes[index] {
            Node::Block(block) => {
                if let BlockKind::RawHtml { html } = &block.kind {
                    let marker_index = fragments.fragments.len();
                    let marker = block_marker(marker_index);
                    fragments.boundary(marker_index);
                    fragments.fragments.push(ProtectedFragment {
                        marker,
                        html: format!("<span data-fleximark-node-id=\"{}\" data-fleximark-kind=\"raw-html-boundary\" hidden></span>", escape_attribute(&block.id.0)),
                    });
                    fragments.raw(html);
                } else {
                    fragments.protect(render_block(block)?);
                }
                index += 1;
            }
            Node::Inline(_) => {
                let (end, inlines) = inline_run(nodes, index);
                fragments.protect(render_inlines(&inlines)?);
                index = end;
            }
        }
    }
    Ok(fragments.finish())
}

static RAW_HTML_SANITIZER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut builder = ammonia::Builder::default();
    builder
        .add_tags(&[
            INTERNAL_FRAGMENT_TAG,
            INTERNAL_BLOCK_MARKER_TAG,
            INTERNAL_SAFE_BLOCK_TAG,
        ])
        .add_tag_attributes(INTERNAL_FRAGMENT_TAG, &["data-index"])
        .add_tag_attributes(INTERNAL_BLOCK_MARKER_TAG, &["data-index"])
        .add_tag_attributes(INTERNAL_SAFE_BLOCK_TAG, &["data-index"])
        .add_tag_attributes("details", &["open"])
        .add_generic_attributes(&["class", "id", "style", "role", "dir", "tabindex"])
        .add_generic_attribute_prefixes(&["aria-", "data-"])
        .url_schemes(
            ["http", "https", "mailto"]
                .into_iter()
                .collect::<HashSet<_>>(),
        )
        .url_relative(ammonia::UrlRelative::Custom(Box::new(
            sanitize_relative_url,
        )))
        .add_clean_content_tags(&[
            "embed", "form", "iframe", "noscript", "object", "script", "template",
        ])
        .attribute_filter(|tag, attribute, value| {
            if attribute.starts_with("on")
                || attribute == "srcdoc"
                || (attribute.starts_with("data-fleximark-") && !tag.starts_with("fleximark-"))
            {
                None
            } else {
                Some(Cow::Borrowed(value))
            }
        })
        // `target` is not allowed, so reverse-tabnabbing is impossible.
        .link_rel(None);
    builder
});

const INTERNAL_FRAGMENT_TAG: &str = "fleximark-safe-fragment";
const INTERNAL_BLOCK_MARKER_TAG: &str = "fleximark-block-marker";
const INTERNAL_SAFE_BLOCK_TAG: &str = "fleximark-safe-block";

fn sanitize_relative_url(url: &str) -> Option<Cow<'_, str>> {
    let normalized = url.trim();
    if normalized.starts_with("//")
        || normalized.starts_with("\\\\")
        || normalized.contains('\\')
        || normalized.chars().any(char::is_control)
    {
        None
    } else {
        Some(Cow::Borrowed(url))
    }
}

pub(crate) fn clean_raw_html(source: &str) -> String {
    RAW_HTML_SANITIZER.clean(source).to_string()
}

pub(crate) fn neutralize_internal_markers(source: &str) -> Cow<'_, str> {
    let mut output = String::new();
    let mut copied_until = 0;
    for (index, character) in source.char_indices() {
        if character != '<' {
            continue;
        }
        let mut tail = &source[index + 1..];
        if let Some(rest) = tail.strip_prefix('/') {
            tail = rest;
        }
        let is_internal = [
            INTERNAL_FRAGMENT_TAG,
            INTERNAL_BLOCK_MARKER_TAG,
            INTERNAL_SAFE_BLOCK_TAG,
        ]
        .into_iter()
        .any(|internal| {
            tail.get(..internal.len()).is_some_and(|name| {
                let boundary = tail[internal.len()..].chars().next();
                name.eq_ignore_ascii_case(internal)
                    && boundary.is_none_or(|value| {
                        value.is_ascii_whitespace() || matches!(value, '/' | '>')
                    })
            })
        });
        if is_internal {
            output.push_str(&source[copied_until..index]);
            output.push_str("&lt;");
            copied_until = index + 1;
        }
    }
    if copied_until == 0 {
        Cow::Borrowed(source)
    } else {
        output.push_str(&source[copied_until..]);
        Cow::Owned(output)
    }
}
