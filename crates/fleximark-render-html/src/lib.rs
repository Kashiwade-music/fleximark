use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use fleximark_model::{
    Block, BlockKind, Document, Inline, InlineKind, NavigationEntry, Node, NodeId,
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlTarget {
    Preview,
    Portable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawHtmlPolicy {
    Sanitize,
    Escape,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderContext {
    pub target: HtmlTarget,
    pub raw_html: RawHtmlPolicy,
    pub allow_remote_resources: bool,
    pub allow_data_resources: bool,
    /// Canonical source spelling to an opaque, already-validated asset reference.
    pub resolved_resources: BTreeMap<String, String>,
}

impl Default for RenderContext {
    fn default() -> Self {
        Self {
            target: HtmlTarget::Preview,
            raw_html: RawHtmlPolicy::Sanitize,
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RenderError {
    #[error("duplicate NodeId at render boundary: {0}")]
    DuplicateNodeId(String),
    #[error("raw HTML is rejected by the target policy")]
    RawHtmlRejected,
    #[error("resource URL is rejected by the target policy: {0}")]
    ResourceRejected(String),
}

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
            render_raw_html(html, context)?
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
                    .map(|node| match node {
                        Node::Inline(inline) => inline,
                        Node::Block(_) => unreachable!("inline run ended at the first block"),
                    })
                    .collect::<Vec<_>>();
                html.push_str(&render_inline_sequence(&inlines, context)?);
                index = end;
            }
        }
    }
    Ok(html)
}

fn render_inline_sequence(
    inlines: &[&Inline],
    context: &RenderContext,
) -> Result<String, RenderError> {
    if context.raw_html != RawHtmlPolicy::Sanitize
        || !inlines
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
            candidate.push_str(&neutralize_fragment_markers(html));
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
        InlineKind::RawHtml { html } => render_raw_html(html, context)?,
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

fn render_raw_html(html: &str, context: &RenderContext) -> Result<String, RenderError> {
    match context.raw_html {
        RawHtmlPolicy::Sanitize => Ok(sanitize_raw_html(html).html),
        RawHtmlPolicy::Escape => Ok(escape_text(html)),
        RawHtmlPolicy::Reject => Err(RenderError::RawHtmlRejected),
    }
}

/// The result of applying FlexiMark's deterministic raw-HTML allowlist.
///
/// `modified` is suitable for diagnostics: accepted HTML remains quiet, while
/// content whose structure or attributes were removed can be surfaced to the
/// author without preventing the rest of the document from rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SanitizedHtml {
    pub html: String,
    pub modified: bool,
}

static RAW_HTML_SANITIZER: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut builder = ammonia::Builder::default();
    builder
        .tags(raw_html_tags())
        .generic_attributes(raw_html_generic_attributes())
        .tag_attributes(raw_html_tag_attributes())
        .url_schemes(
            ["http", "https", "mailto"]
                .into_iter()
                .collect::<HashSet<_>>(),
        )
        .url_relative(ammonia::UrlRelative::Custom(Box::new(
            sanitize_relative_url,
        )))
        .clean_content_tags(
            [
                "iframe", "math", "noscript", "object", "script", "style", "svg", "template",
            ]
            .into_iter()
            .collect::<HashSet<_>>(),
        )
        // `target` is not allowed, so reverse-tabnabbing is impossible and
        // adding `rel` would create noisy diagnostics for otherwise safe links.
        .link_rel(None)
        .id_prefix(Some("user-content-"));
    builder
});

const INTERNAL_FRAGMENT_TAG: &str = "fleximark-safe-fragment";

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

/// Sanitizes author-provided HTML independently of browser CSP.
///
/// The allowlist intentionally contains semantic and presentational document
/// elements only. Elements capable of executing code, embedding another
/// browsing context, submitting data, or fetching subresources are excluded.
/// Event handlers, inline styles, and `data-*` attributes are never accepted.
pub fn sanitize_raw_html(source: &str) -> SanitizedHtml {
    let html = clean_raw_html(&neutralize_fragment_markers(source));
    SanitizedHtml {
        modified: html != source,
        html,
    }
}

fn clean_raw_html(source: &str) -> String {
    RAW_HTML_SANITIZER.clean(source).to_string()
}

fn neutralize_fragment_markers(source: &str) -> Cow<'_, str> {
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
        let Some(name) = tail.get(..INTERNAL_FRAGMENT_TAG.len()) else {
            continue;
        };
        let boundary = tail[INTERNAL_FRAGMENT_TAG.len()..].chars().next();
        if name.eq_ignore_ascii_case(INTERNAL_FRAGMENT_TAG)
            && boundary
                .is_none_or(|value| value.is_ascii_whitespace() || matches!(value, '/' | '>'))
        {
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

fn raw_html_tags() -> HashSet<&'static str> {
    [
        "a",
        "abbr",
        "address",
        "article",
        "aside",
        "b",
        "bdi",
        "bdo",
        "blockquote",
        "br",
        "caption",
        "cite",
        "code",
        "col",
        "colgroup",
        "dd",
        "del",
        "details",
        "dfn",
        "div",
        "dl",
        "dt",
        "em",
        "figcaption",
        "figure",
        "footer",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "header",
        "hr",
        "i",
        "kbd",
        "li",
        "main",
        "mark",
        "nav",
        "ol",
        "p",
        "pre",
        "q",
        "rp",
        "rt",
        "ruby",
        "s",
        "samp",
        "section",
        "small",
        "span",
        "strong",
        "sub",
        "summary",
        "sup",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "tr",
        "u",
        "ul",
        "var",
        "wbr",
        INTERNAL_FRAGMENT_TAG,
    ]
    .into_iter()
    .collect()
}

fn raw_html_generic_attributes() -> HashSet<&'static str> {
    [
        "aria-describedby",
        "aria-details",
        "aria-expanded",
        "aria-hidden",
        "aria-label",
        "aria-labelledby",
        "aria-live",
        "aria-current",
        "class",
        "dir",
        "id",
        "lang",
        "role",
        "title",
    ]
    .into_iter()
    .collect()
}

fn raw_html_tag_attributes() -> HashMap<&'static str, HashSet<&'static str>> {
    HashMap::from([
        ("a", ["href", "hreflang"].into_iter().collect()),
        ("blockquote", ["cite"].into_iter().collect()),
        ("col", ["span"].into_iter().collect()),
        ("colgroup", ["span"].into_iter().collect()),
        ("del", ["cite", "datetime"].into_iter().collect()),
        ("details", ["open"].into_iter().collect()),
        ("ol", ["reversed", "start", "type"].into_iter().collect()),
        ("q", ["cite"].into_iter().collect()),
        (INTERNAL_FRAGMENT_TAG, ["data-index"].into_iter().collect()),
        (
            "td",
            ["colspan", "rowspan", "headers"].into_iter().collect(),
        ),
        (
            "th",
            ["abbr", "colspan", "rowspan", "headers", "scope"]
                .into_iter()
                .collect(),
        ),
    ])
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
mod tests {
    use super::*;
    use fleximark_model::DocumentUri;
    use fleximark_parser::parse;

    #[test]
    fn renders_typed_special_blocks_with_owned_identity() {
        let source = "# Hi\n\n| A |\n|---|\n| B |\n\n```mermaid\ngraph TD; A-->B\n```\n";
        let document = parse(DocumentUri("file:///demo.md".into()), 1, source).unwrap();
        let html = HtmlRenderer
            .render(&document, &RenderContext::default())
            .unwrap();
        assert!(html.contains("data-fleximark-node-id=\"pending-0\""));
        assert!(html.contains("data-fleximark-kind=\"mermaid\""));
        assert!(html.contains("<th data-fleximark-node-id="));
        assert!(html.contains(r#"type="application/json">"graph TD; A--\u003eB\n""#));
    }

    #[test]
    fn marks_only_tight_lists_for_compact_preview_spacing() {
        let tight = parse(DocumentUri("file:///tight.md".into()), 1, "- one\n- two\n").unwrap();
        let loose = parse(
            DocumentUri("file:///loose.md".into()),
            1,
            "- one\n\n- two\n",
        )
        .unwrap();

        let tight_html = HtmlRenderer
            .render(&tight, &RenderContext::default())
            .unwrap();
        let loose_html = HtmlRenderer
            .render(&loose, &RenderContext::default())
            .unwrap();
        assert!(tight_html.contains("data-fleximark-tight=\"true\""));
        assert!(!loose_html.contains("data-fleximark-tight"));
    }

    #[test]
    fn renders_accessible_tabs_details_and_visible_code_metadata() {
        let source = "::::tabs\n:::tab[First]\none\n:::\n:::tab[Second]\ntwo\n:::\n::::\n\n:::details[Read more]\ninside\n:::\n\n```rust title='Demo' line-numbers\nfn main() {}\nlet x = 2;\n```\n";
        let document = parse(DocumentUri("file:///ui.md".into()), 1, source).unwrap();
        let html = HtmlRenderer
            .render(&document, &RenderContext::default())
            .unwrap();
        assert!(html.contains("data-fleximark-kind=\"tab\" data-tab-label=\"First\""));
        assert!(html.contains("<summary>Read more</summary>"));
        assert!(html.contains("<figcaption class=\"fleximark-code-title\">Demo</figcaption>"));
        assert!(html.contains("class=\"fleximark-code-line\" data-line=\"1\""));
        assert!(html.contains("class=\"fleximark-code-line\" data-line=\"2\""));
    }

    #[test]
    fn sanitizes_raw_html_links_and_image_resources() {
        let raw = parse(
            DocumentUri("file:///raw.md".into()),
            1,
            "<script>alert(1)</script>\n",
        )
        .unwrap();
        let sanitized = HtmlRenderer
            .render(&raw, &RenderContext::default())
            .unwrap();
        assert!(!sanitized.contains("script"));
        assert!(!sanitized.contains("alert(1)"));
        let link = parse(
            DocumentUri("file:///link.md".into()),
            1,
            "[bad](javascript:alert(1))\n",
        )
        .unwrap();
        assert!(matches!(
            HtmlRenderer.render(&link, &RenderContext::default()),
            Err(RenderError::ResourceRejected(_))
        ));
        let permissive = RenderContext {
            allow_remote_resources: true,
            allow_data_resources: true,
            ..RenderContext::default()
        };
        assert!(resolve_resource("//evil.example/x", &permissive, ResourceUse::Image).is_err());
        assert!(
            resolve_resource("data:text/html,<script>", &permissive, ResourceUse::Image).is_err()
        );
        assert!(
            resolve_resource("data:image/svg+xml,<svg>", &permissive, ResourceUse::Image).is_err()
        );
        assert!(
            resolve_resource(
                "data:image/png,percent-encoded",
                &permissive,
                ResourceUse::Image
            )
            .is_err()
        );
        assert!(
            resolve_resource(
                "data:image/avif;base64,AA==",
                &permissive,
                ResourceUse::Image
            )
            .is_err()
        );
        assert!(
            resolve_resource(
                "data:image/png;base64,AA==",
                &permissive,
                ResourceUse::Image
            )
            .is_ok()
        );
        assert!(
            resolve_resource("data:image/png;base64,AA==", &permissive, ResourceUse::Link).is_err()
        );
    }

    #[test]
    fn preserves_useful_raw_html_and_inline_tag_pairs() {
        let source = "Press <kbd class=\"key\">Ctrl</kbd> and <mark>Enter</mark>.\n\n<details open><summary>More</summary><table><tr><th scope=\"col\">A</th></tr><tr><td>1</td></tr></table></details>\n";
        let document = parse(DocumentUri("file:///safe-html.md".into()), 1, source).unwrap();
        let html = HtmlRenderer
            .render(&document, &RenderContext::default())
            .unwrap();

        assert!(html.contains("<kbd class=\"key\">Ctrl</kbd>"));
        assert!(html.contains("<mark>Enter</mark>"));
        assert!(html.contains("<details open=\"\"><summary>More</summary>"));
        assert!(html.contains("<th scope=\"col\">A</th>"));
    }

    #[test]
    fn author_html_cannot_collide_with_internal_inline_fragment_markers() {
        let source = "Attempt <FLEXIMARK-SAFE-FRAGMENT data-index=\"0\"></FLEXIMARK-SAFE-FRAGMENT> then <kbd>Ctrl</kbd> and **trusted**.\n";
        let document = parse(DocumentUri("file:///marker.md".into()), 1, source).unwrap();
        let html = HtmlRenderer
            .render(&document, &RenderContext::default())
            .unwrap();

        assert!(html.contains("&lt;FLEXIMARK-SAFE-FRAGMENT"));
        assert_eq!(html.matches("<strong>trusted</strong>").count(), 1);
        assert!(html.contains("<kbd>Ctrl</kbd>"));
    }

    #[test]
    fn raw_html_sanitizer_removes_executable_embedded_and_resource_loading_content() {
        let source = concat!(
            "<div id=\"preview\" class=\"box\" style=\"background:url(https://evil.example)\" onclick=\"steal()\">",
            "<script>steal()</script><style>@import 'https://evil.example/x';</style>",
            "<iframe src=\"https://evil.example\">fallback</iframe>",
            "<svg onload=\"steal()\"><script>steal()</script></svg>",
            "<img src=\"https://evil.example/pixel\">",
            "<a href=\"javascript:steal()\" data-fleximark-node-id=\"forged\">bad</a>",
            "<a href=\"//evil.example/path\">protocol-relative</a>",
            "<a href=\"../guide\">relative</a>",
            "<a href=\"https://example.com\">good</a></div>"
        );
        let result = sanitize_raw_html(source);

        assert!(result.modified);
        assert!(result.html.contains("id=\"user-content-preview\""));
        assert!(result.html.contains("class=\"box\""));
        assert!(!result.html.contains("steal"));
        assert!(!result.html.contains("evil.example"));
        assert!(!result.html.contains("<iframe"));
        assert!(!result.html.contains("<svg"));
        assert!(!result.html.contains("<img"));
        assert!(!result.html.contains("style="));
        assert!(!result.html.contains("data-fleximark-node-id"));
        assert!(result.html.contains("<a>bad</a>"));
        assert!(result.html.contains("<a>protocol-relative</a>"));
        assert!(result.html.contains("<a href=\"../guide\">relative</a>"));
        assert!(
            result
                .html
                .contains("<a href=\"https://example.com\">good</a>")
        );
        assert!(!sanitize_raw_html("<a href=\"https://example.com\">good</a>").modified);
    }

    #[test]
    fn escape_and_reject_remain_available_as_explicit_stricter_policies() {
        let document = parse(
            DocumentUri("file:///strict-html.md".into()),
            1,
            "<kbd>Ctrl</kbd>\n",
        )
        .unwrap();
        let escaped = HtmlRenderer
            .render(
                &document,
                &RenderContext {
                    raw_html: RawHtmlPolicy::Escape,
                    ..RenderContext::default()
                },
            )
            .unwrap();
        assert!(escaped.contains("&lt;kbd&gt;"));
        assert!(matches!(
            HtmlRenderer.render(
                &document,
                &RenderContext {
                    raw_html: RawHtmlPolicy::Reject,
                    ..RenderContext::default()
                }
            ),
            Err(RenderError::RawHtmlRejected)
        ));
    }

    #[test]
    fn local_images_require_a_resolved_opaque_asset_reference() {
        let document = parse(
            DocumentUri("file:///asset.md".into()),
            1,
            "![diagram](images/diagram.png)\n",
        )
        .unwrap();
        let unresolved = HtmlRenderer
            .render(&document, &RenderContext::default())
            .unwrap();
        assert!(unresolved.contains("data-fleximark-kind=\"asset-placeholder\""));
        assert!(!unresolved.contains("images/diagram.png"));

        let mut context = RenderContext::default();
        context.resolved_resources.insert(
            "images/diagram.png".to_owned(),
            format!("fleximark-asset:{}", "a".repeat(64)),
        );
        let html = HtmlRenderer.render(&document, &context).unwrap();
        assert!(html.contains(&format!("src=\"fleximark-asset:{}\"", "a".repeat(64))));
        assert!(!html.contains("images/diagram.png"));

        let portable = RenderContext {
            target: HtmlTarget::Portable,
            ..RenderContext::default()
        };
        let html = HtmlRenderer.render(&document, &portable).unwrap();
        assert!(html.contains("src=\"images/diagram.png\""));
        assert!(matches!(
            resolve_resource(
                "https://outside.example/diagram.png",
                &portable,
                ResourceUse::Image
            ),
            Err(RenderError::ResourceRejected(_))
        ));

        let document = parse(
            DocumentUri("file:///assets.md".into()),
            1,
            "![works](images/diagram.png)\n\n![blocked](../outside.png)\n",
        )
        .unwrap();
        let html = HtmlRenderer.render(&document, &context).unwrap();
        assert!(html.contains(&format!("src=\"fleximark-asset:{}\"", "a".repeat(64))));
        assert!(html.contains("aria-label=\"blocked\""));
        assert!(!html.contains("../outside.png"));
    }

    #[test]
    fn rendered_metadata_contains_valid_navigation_for_nested_unicode_blocks() {
        let source = "- Héllo\n";
        let document = parse(DocumentUri("file:///nested.md".into()), 1, source).unwrap();
        document.validate(source).unwrap();
        let blocks = HtmlRenderer
            .render_blocks(&document, &RenderContext::default())
            .unwrap();
        let navigation = blocks
            .iter()
            .flat_map(|block| &block.navigation)
            .collect::<Vec<_>>();
        assert_eq!(navigation.len(), blocks[0].node_ids.len());
        assert!(navigation.iter().any(|entry| entry.depth >= 2));
        assert!(navigation.iter().all(|entry| {
            entry.source_range.byte_end <= source.len() as u64
                && source.is_char_boundary(entry.source_range.byte_start.get() as usize)
                && source.is_char_boundary(entry.source_range.byte_end.get() as usize)
        }));
    }
}
