//! Role: Render document nodes as sanitized HTML.
use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

use fleximark_model::{
    Block, BlockKind, Document, Inline, InlineKind, NavigationEntry, Node, NodeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlTarget {
    Preview,
    Portable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderContext {
    pub target: HtmlTarget,
    pub allow_remote_resources: bool,
    pub allow_data_resources: bool,
    /// Canonical source spelling to an opaque, already-validated asset reference.
    pub resolved_resources: BTreeMap<String, String>,
}

impl Default for RenderContext {
    fn default() -> Self {
        Self {
            target: HtmlTarget::Preview,
            allow_remote_resources: false,
            allow_data_resources: false,
            resolved_resources: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedBlock {
    pub id: NodeId,
    pub node_ids: Vec<NodeId>,
    pub navigation: Vec<NavigationEntry>,
    pub html: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RenderError {
    DuplicateNodeId(String),
    InvalidRawHtmlBoundary,
    ResourceRejected(String),
}
impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateNodeId(value) => {
                write!(f, "duplicate NodeId at render boundary: {0}", value)
            }
            Self::InvalidRawHtmlBoundary => {
                write!(f, "raw HTML sequence lost its internal boundary marker")
            }
            Self::ResourceRejected(value) => write!(
                f,
                "resource URL is rejected by the target policy: {0}",
                value
            ),
        }
    }
}
impl std::error::Error for RenderError {}

pub struct HtmlRenderer;

impl HtmlRenderer {
    pub fn render(
        &self,
        document: &Document,
        context: &RenderContext,
    ) -> Result<String, RenderError> {
        Ok(self
            .render_blocks(document, context)?
            .into_iter()
            .map(|block| block.html)
            .collect())
    }

    pub fn render_blocks(
        &self,
        document: &Document,
        context: &RenderContext,
    ) -> Result<Vec<RenderedBlock>, RenderError> {
        if document
            .blocks
            .iter()
            .any(|block| matches!(block.kind, BlockKind::RawHtml { .. }))
        {
            return render_sanitized_block_sequence(&document.blocks, context);
        }
        let mut ids = HashSet::new();
        let mut rendered = Vec::with_capacity(document.blocks.len());
        for block in &document.blocks {
            let mut node_ids = Vec::new();
            let mut navigation = Vec::new();
            collect_render_metadata(block, 0, &mut ids, &mut node_ids, &mut navigation)?;
            rendered.push(RenderedBlock {
                id: block.id.clone(),
                node_ids,
                navigation,
                html: render_block(block, context)?,
            });
        }
        Ok(rendered)
    }
}

fn render_sanitized_block_sequence(
    blocks: &[Block],
    context: &RenderContext,
) -> Result<Vec<RenderedBlock>, RenderError> {
    let mut seen = HashSet::new();
    let mut rendered = Vec::with_capacity(blocks.len());
    let mut candidate = String::new();
    for (index, block) in blocks.iter().enumerate() {
        let mut node_ids = Vec::new();
        let mut navigation = Vec::new();
        collect_render_metadata(block, 0, &mut seen, &mut node_ids, &mut navigation)?;
        let html = if matches!(block.kind, BlockKind::RawHtml { .. }) {
            None
        } else {
            Some(render_block(block, context)?)
        };
        rendered.push((block, node_ids, navigation, html));
        candidate.push_str(&block_marker(index));
        match &block.kind {
            BlockKind::RawHtml { html } => candidate.push_str(&neutralize_internal_markers(html)),
            _ => candidate.push_str(&safe_block_marker(index)),
        }
    }

    let sanitized = clean_raw_html(&candidate);
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

        if indices.len() == 1 && rendered[root_index].3.is_some() {
            output.push(RenderedBlock {
                id: rendered[root_index].0.id.clone(),
                node_ids: rendered[root_index].1.clone(),
                navigation: rendered[root_index].2.clone(),
                html: rendered[root_index].3.clone().expect("checked above"),
            });
            continue;
        }

        let mut node_ids = Vec::new();
        let mut navigation = Vec::new();
        for &index in &indices {
            node_ids.extend(rendered[index].1.iter().cloned());
            navigation.extend(rendered[index].2.iter().cloned());
            group = group.replace(
                &safe_block_marker(index),
                rendered[index].3.as_deref().unwrap_or(""),
            );
            let identity = if index == root_index {
                String::new()
            } else if rendered[index].3.is_none() {
                format!(
                    "<span data-fleximark-node-id=\"{}\" data-fleximark-kind=\"raw-html-boundary\" hidden></span>",
                    escape_attribute(&rendered[index].0.id.0)
                )
            } else {
                String::new()
            };
            group = group.replace(&block_marker(index), &identity);
        }
        output.push(RenderedBlock {
            id: rendered[root_index].0.id.clone(),
            node_ids,
            navigation,
            html: format!(
                "<div data-fleximark-node-id=\"{}\" data-fleximark-kind=\"raw-html-container\">{group}</div>\n",
                escape_attribute(&rendered[root_index].0.id.0)
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

fn collect_render_metadata(
    block: &Block,
    depth: u32,
    seen: &mut HashSet<NodeId>,
    node_ids: &mut Vec<NodeId>,
    navigation: &mut Vec<NavigationEntry>,
) -> Result<(), RenderError> {
    if !seen.insert(block.id.clone()) {
        return Err(RenderError::DuplicateNodeId(block.id.0.clone()));
    }
    node_ids.push(block.id.clone());
    if let Some(source_range) = block.provenance.navigation_range() {
        navigation.push(NavigationEntry {
            node_id: block.id.clone(),
            source_range,
            depth,
        });
    }
    for child in &block.children {
        if let Node::Block(block) = child {
            collect_render_metadata(block, depth + 1, seen, node_ids, navigation)?;
        }
    }
    Ok(())
}

fn render_block(block: &Block, context: &RenderContext) -> Result<String, RenderError> {
    let id = escape_attribute(&block.id.0);
    let children = || render_nodes(&block.children, context);
    Ok(match &block.kind {
        BlockKind::Paragraph => format!("<p data-fleximark-node-id=\"{id}\">{}</p>\n", children()?),
        BlockKind::Heading { level } => {
            let level = (*level).clamp(1, 6);
            format!(
                "<h{level} data-fleximark-node-id=\"{id}\">{}</h{level}>\n",
                children()?
            )
        }
        BlockKind::Quote => format!(
            "<blockquote data-fleximark-node-id=\"{id}\">{}</blockquote>\n",
            children()?
        ),
        BlockKind::List {
            ordered,
            start,
            tight,
        } => {
            let tight = if *tight {
                " data-fleximark-tight=\"true\""
            } else {
                ""
            };
            if *ordered {
                format!(
                    "<ol data-fleximark-node-id=\"{id}\" start=\"{start}\"{tight}>{}</ol>\n",
                    children()?
                )
            } else {
                format!(
                    "<ul data-fleximark-node-id=\"{id}\"{tight}>{}</ul>\n",
                    children()?
                )
            }
        }
        BlockKind::ListItem { checked } => {
            let task = checked
                .map(|checked| {
                    format!(
                        " role=\"checkbox\" aria-checked=\"{}\"",
                        if checked { "true" } else { "false" }
                    )
                })
                .unwrap_or_default();
            format!(
                "<li data-fleximark-node-id=\"{id}\"{task}>{}</li>\n",
                children()?
            )
        }
        BlockKind::Table => format!(
            "<table data-fleximark-node-id=\"{id}\"><tbody>{}</tbody></table>\n",
            children()?
        ),
        BlockKind::TableRow { .. } => {
            format!("<tr data-fleximark-node-id=\"{id}\">{}</tr>\n", children()?)
        }
        BlockKind::TableCell { header } => {
            let tag = if *header { "th" } else { "td" };
            format!(
                "<{tag} data-fleximark-node-id=\"{id}\">{}</{tag}>\n",
                children()?
            )
        }
        BlockKind::CodeBlock {
            language,
            title,
            line_numbers,
            code,
        } => {
            let language = language
                .as_deref()
                .map(escape_attribute)
                .unwrap_or_default();
            let caption = title
                .as_deref()
                .map(|value| {
                    format!(
                        "<figcaption class=\"fleximark-code-title\">{}</figcaption>",
                        escape_text(value)
                    )
                })
                .unwrap_or_default();
            let code = if *line_numbers {
                code.split_inclusive('\n')
                    .enumerate()
                    .map(|(index, line)| {
                        let content = line.strip_suffix('\n').unwrap_or(line);
                        format!(
                            "<span class=\"fleximark-code-line\" data-line=\"{}\">{}</span>{}",
                            index + 1,
                            escape_text(content),
                            if line.ends_with('\n') { "\n" } else { "" }
                        )
                    })
                    .collect::<String>()
            } else {
                escape_text(code)
            };
            format!(
                "<figure data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"code\">{caption}<pre><code class=\"language-{language}\">{code}</code></pre></figure>\n"
            )
        }
        BlockKind::Mermaid { source } => special_block(&id, "mermaid", source),
        BlockKind::AbcNotation { source } => special_block(&id, "abc", source),
        BlockKind::Math { source } => format!(
            "<div data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"math\">{}</div>\n",
            escape_text(source)
        ),
        BlockKind::ThematicBreak => format!("<hr data-fleximark-node-id=\"{id}\">\n"),
        BlockKind::Admonition { kind, title } => format!(
            "<aside data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"admonition\" data-admonition-kind=\"{}\"><header class=\"fleximark-admonition-title\">{}</header>{}</aside>\n",
            escape_attribute(kind),
            escape_text(title),
            children()?
        ),
        BlockKind::Tabs => format!(
            "<section data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"tabs\">{}</section>\n",
            children()?
        ),
        BlockKind::Tab { label } => format!(
            "<section data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"tab\" data-tab-label=\"{}\">{}</section>\n",
            escape_attribute(label),
            children()?
        ),
        BlockKind::Details { summary } => format!(
            "<details data-fleximark-node-id=\"{id}\"><summary>{}</summary>{}</details>\n",
            escape_text(summary),
            children()?
        ),
        BlockKind::Media { source } => {
            if is_youtube_url(source) {
                format!(
                    "<div data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"youtube\" data-source=\"{}\"></div>\n",
                    escape_attribute(source)
                )
            } else {
                match resolve_resource(source, context, ResourceUse::Image) {
                    Ok(source) => format!(
                        "<img data-fleximark-node-id=\"{id}\" src=\"{}\" alt=\"\">\n",
                        escape_attribute(&source)
                    ),
                    Err(RenderError::ResourceRejected(_))
                        if context.target == HtmlTarget::Preview =>
                    {
                        format!(
                            "<span data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"asset-placeholder\" role=\"img\">Asset unavailable</span>\n"
                        )
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        BlockKind::RawHtml { html } => format!(
            "<div data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"raw-html\">{}</div>\n",
            clean_raw_html(&neutralize_internal_markers(html))
        ),
        BlockKind::Plugin { namespace, name } => format!(
            "<div data-fleximark-node-id=\"{id}\" data-plugin-namespace=\"{}\" data-plugin-name=\"{}\">{}</div>\n",
            escape_attribute(namespace),
            escape_attribute(name),
            children()?
        ),
    })
}

fn special_block(id: &str, kind: &str, source: &str) -> String {
    let payload = serde_json::to_string(source)
        .expect("serializing an owned Rust string as JSON cannot fail")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    format!(
        "<div data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"{kind}\"><script type=\"application/json\">{}</script></div>\n",
        payload
    )
}

fn render_nodes(nodes: &[Node], context: &RenderContext) -> Result<String, RenderError> {
    if nodes.iter().any(|node| {
        matches!(
            node,
            Node::Block(Block {
                kind: BlockKind::RawHtml { .. },
                ..
            })
        )
    }) {
        return render_sanitized_nodes(nodes, context);
    }
    let mut html = String::new();
    let mut index = 0;
    while index < nodes.len() {
        match &nodes[index] {
            Node::Block(block) => {
                html.push_str(&render_block(block, context)?);
                index += 1;
            }
            Node::Inline(_) => {
                let end = nodes[index..]
                    .iter()
                    .position(|node| matches!(node, Node::Block(_)))
                    .map_or(nodes.len(), |offset| index + offset);
                let inlines = nodes[index..end]
                    .iter()
                    .filter_map(|node| match node {
                        Node::Inline(inline) => Some(inline),
                        Node::Block(_) => None,
                    })
                    .collect::<Vec<_>>();
                html.push_str(&render_inline_sequence(&inlines, context)?);
                index = end;
            }
        }
    }
    Ok(html)
}

fn render_sanitized_nodes(nodes: &[Node], context: &RenderContext) -> Result<String, RenderError> {
    enum ProtectedNode {
        Safe(String),
        RawIdentity(NodeId),
    }

    let mut candidate = String::new();
    let mut protected = Vec::new();
    let mut index = 0;
    while index < nodes.len() {
        match &nodes[index] {
            Node::Block(block) => {
                let protected_index = protected.len();
                match &block.kind {
                    BlockKind::RawHtml { html } => {
                        candidate.push_str(&block_marker(protected_index));
                        candidate.push_str(&neutralize_internal_markers(html));
                        protected.push(ProtectedNode::RawIdentity(block.id.clone()));
                    }
                    _ => {
                        candidate.push_str(&safe_block_marker(protected_index));
                        protected.push(ProtectedNode::Safe(render_block(block, context)?));
                    }
                }
                index += 1;
            }
            Node::Inline(_) => {
                let end = nodes[index..]
                    .iter()
                    .position(|node| matches!(node, Node::Block(_)))
                    .map_or(nodes.len(), |offset| index + offset);
                let inlines = nodes[index..end]
                    .iter()
                    .filter_map(|node| match node {
                        Node::Inline(inline) => Some(inline),
                        Node::Block(_) => None,
                    })
                    .collect::<Vec<_>>();
                let protected_index = protected.len();
                candidate.push_str(&safe_block_marker(protected_index));
                protected.push(ProtectedNode::Safe(render_inline_sequence(
                    &inlines, context,
                )?));
                index = end;
            }
        }
    }

    let mut sanitized = clean_raw_html(&candidate);
    for (index, node) in protected.into_iter().enumerate() {
        match node {
            ProtectedNode::Safe(html) => {
                sanitized = sanitized.replace(&safe_block_marker(index), &html);
            }
            ProtectedNode::RawIdentity(id) => {
                sanitized = sanitized.replace(
                    &block_marker(index),
                    &format!(
                        "<span data-fleximark-node-id=\"{}\" data-fleximark-kind=\"raw-html-boundary\" hidden></span>",
                        escape_attribute(&id.0)
                    ),
                );
            }
        }
    }
    Ok(sanitized)
}

fn render_inline_sequence(
    inlines: &[&Inline],
    context: &RenderContext,
) -> Result<String, RenderError> {
    if !inlines
        .iter()
        .any(|inline| matches!(inline.kind, InlineKind::RawHtml { .. }))
    {
        return inlines
            .iter()
            .map(|inline| render_inline(inline, context))
            .collect();
    }

    // CommonMark represents an inline element as separate opening and closing
    // raw nodes. Sanitize the complete sequence so harmless constructs such as
    // `<kbd>Ctrl</kbd>` retain their structure. Renderer-owned fragments are
    // replaced by inert text markers during sanitization and restored after it,
    // preventing the raw allowlist from granting authors access to internal
    // `data-fleximark-*` attributes or resource-loading elements.
    let mut candidate = String::new();
    let mut safe_fragments = Vec::new();
    for inline in inlines {
        if let InlineKind::RawHtml { html } = &inline.kind {
            candidate.push_str(&neutralize_internal_markers(html));
        } else {
            let marker = format!(
                "<{INTERNAL_FRAGMENT_TAG} data-index=\"{}\"></{INTERNAL_FRAGMENT_TAG}>",
                safe_fragments.len()
            );
            candidate.push_str(&marker);
            safe_fragments.push((marker, render_inline(inline, context)?));
        }
    }
    let mut sanitized = clean_raw_html(&candidate);
    for (marker, fragment) in safe_fragments {
        sanitized = sanitized.replace(&marker, &fragment);
    }
    Ok(sanitized)
}

fn render_inline(inline: &Inline, context: &RenderContext) -> Result<String, RenderError> {
    let nested = |children: &[Inline]| -> Result<String, RenderError> {
        render_inline_sequence(&children.iter().collect::<Vec<_>>(), context)
    };
    Ok(match &inline.kind {
        InlineKind::Text { value } => escape_text(value),
        InlineKind::Code { value } => format!("<code>{}</code>", escape_text(value)),
        InlineKind::Emphasis { children } => format!("<em>{}</em>", nested(children)?),
        InlineKind::Strong { children } => format!("<strong>{}</strong>", nested(children)?),
        InlineKind::Strikethrough { children } => format!("<del>{}</del>", nested(children)?),
        InlineKind::Link {
            destination,
            title,
            children,
        } => {
            let destination = resolve_resource(destination, context, ResourceUse::Link)?;
            format!(
                "<a href=\"{}\" title=\"{}\">{}</a>",
                escape_attribute(&destination),
                escape_attribute(title),
                nested(children)?
            )
        }
        InlineKind::Image {
            source,
            title,
            children,
        } => {
            let alt = children.iter().map(inline_text).collect::<String>();
            match resolve_resource(source, context, ResourceUse::Image) {
                Ok(source) => format!(
                    "<img src=\"{}\" title=\"{}\" alt=\"{}\">",
                    escape_attribute(&source),
                    escape_attribute(title),
                    escape_attribute(&alt)
                ),
                Err(RenderError::ResourceRejected(_)) if context.target == HtmlTarget::Preview => {
                    format!(
                        "<span data-fleximark-kind=\"asset-placeholder\" role=\"img\" aria-label=\"{}\">Asset unavailable</span>",
                        escape_attribute(&alt)
                    )
                }
                Err(error) => return Err(error),
            }
        }
        InlineKind::Math { source } => format!(
            "<span data-fleximark-kind=\"math\">{}</span>",
            escape_text(source)
        ),
        InlineKind::SoftBreak => "\n".to_owned(),
        InlineKind::HardBreak => "<br>\n".to_owned(),
        InlineKind::RawHtml { html } => clean_raw_html(&neutralize_internal_markers(html)),
    })
}

fn inline_text(inline: &Inline) -> String {
    match &inline.kind {
        InlineKind::Text { value } | InlineKind::Code { value } => value.clone(),
        InlineKind::Emphasis { children }
        | InlineKind::Strong { children }
        | InlineKind::Strikethrough { children }
        | InlineKind::Link { children, .. }
        | InlineKind::Image { children, .. } => children.iter().map(inline_text).collect(),
        InlineKind::Math { source } => source.clone(),
        InlineKind::SoftBreak | InlineKind::HardBreak => " ".to_owned(),
        InlineKind::RawHtml { .. } => String::new(),
    }
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

fn clean_raw_html(source: &str) -> String {
    RAW_HTML_SANITIZER.clean(source).to_string()
}

fn neutralize_internal_markers(source: &str) -> Cow<'_, str> {
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

#[derive(Clone, Copy)]
enum ResourceUse {
    Link,
    Image,
}

fn resolve_resource(
    url: &str,
    context: &RenderContext,
    resource_use: ResourceUse,
) -> Result<String, RenderError> {
    if let Some(reference) = context.resolved_resources.get(url) {
        if reference.starts_with("fleximark-asset:")
            && reference["fleximark-asset:".len()..].len() == 64
            && reference["fleximark-asset:".len()..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Ok(reference.clone());
        }
        return Err(RenderError::ResourceRejected(reference.clone()));
    }
    let lower = url.trim().to_ascii_lowercase();
    let safe_data_image = matches!(resource_use, ResourceUse::Image)
        && [
            "data:image/png;base64,",
            "data:image/jpeg;base64,",
            "data:image/gif;base64,",
            "data:image/webp;base64,",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
    let is_protocol_relative = lower.starts_with("//") || lower.starts_with("\\\\");
    let has_forbidden_character = lower.chars().any(char::is_control) || lower.contains('\\');
    let is_relative_asset = matches!(resource_use, ResourceUse::Image)
        && !lower.starts_with('/')
        && !lower.starts_with("data:")
        && !lower.starts_with("http://")
        && !lower.starts_with("https://")
        && !lower.contains(':');
    let allowed = (!is_relative_asset || context.target == HtmlTarget::Portable)
        && !is_protocol_relative
        && !has_forbidden_character
        && (lower.starts_with('#')
            || lower.starts_with('/')
            || lower.starts_with("./")
            || lower.starts_with("../")
            || lower.starts_with("mailto:")
            || ((lower.starts_with("https://") || lower.starts_with("http://"))
                && (matches!(resource_use, ResourceUse::Link) || context.allow_remote_resources))
            || (safe_data_image && context.allow_data_resources)
            || !lower.contains(':'));
    if allowed {
        Ok(url.to_owned())
    } else {
        Err(RenderError::ResourceRejected(url.to_owned()))
    }
}

fn is_youtube_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://youtu.be/")
        || lower.starts_with("https://www.youtube.com/watch?")
        || lower.starts_with("https://youtube.com/watch?")
}

fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn escape_attribute(value: &str) -> String {
    escape_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
#[cfg(test)]
mod tests;
