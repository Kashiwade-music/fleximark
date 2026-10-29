//! Role: Render typed block nodes into trusted HTML fragments.

use crate::escaping::{escape_attribute, escape_text};
use crate::resource_policy::{ResourceUse, resolve_resource};
use crate::sanitize::{clean_raw_html, neutralize_internal_markers};
use crate::types::{HtmlTarget, RenderContext, RenderError};
use fleximark_model::syntax::{BuiltinSyntaxKind, youtube_video_id};
use fleximark_model::{Block, BlockKind, Node};

/// Render a block's own markup; the orchestrator supplies child-sequence rendering.
pub(crate) fn render_block(
    block: &Block,
    context: &RenderContext,
    render_children: impl Fn(&[Node]) -> Result<String, RenderError>,
) -> Result<String, RenderError> {
    let id = escape_attribute(&block.id.0);
    let children = || render_children(&block.children);
    Ok(match &block.kind {
        BlockKind::Paragraph => paragraph(&id, children()?),
        BlockKind::Heading { level } => heading(&id, *level, children()?),
        BlockKind::Quote => quote(&id, children()?),
        BlockKind::List {
            ordered,
            start,
            tight,
        } => list_block(&id, *ordered, *start, *tight, children()?),
        BlockKind::ListItem { checked } => list_item(&id, *checked, children()?),
        BlockKind::Table => table(&id, children()?),
        BlockKind::TableRow { .. } => table_row(&id, children()?),
        BlockKind::TableCell { header } => table_cell(&id, *header, children()?),
        BlockKind::CodeBlock {
            language,
            title,
            line_numbers,
            code,
        } => code_block(
            &id,
            language.as_deref(),
            title.as_deref(),
            *line_numbers,
            code,
        ),
        BlockKind::Mermaid { source } => {
            special_block(&id, BuiltinSyntaxKind::Mermaid.render_kind(), source)?
        }
        BlockKind::AbcNotation { source } => {
            special_block(&id, BuiltinSyntaxKind::Abc.render_kind(), source)?
        }
        BlockKind::Math { source } => math_block(&id, source),
        BlockKind::ThematicBreak => thematic_break(&id),
        BlockKind::Admonition { kind, title } => admonition(&id, kind, title, children()?),
        BlockKind::Tabs => tabs(&id, children()?),
        BlockKind::Tab { label } => tab(&id, label, children()?),
        BlockKind::Details { summary } => details(&id, summary, children()?),
        BlockKind::Media { source } => media_block(&id, source, context)?,
        BlockKind::RawHtml { html } => raw_html(&id, html),
        BlockKind::Directive { name } => format!(
            "<div data-fleximark-node-id=\"{id}\" data-fleximark-directive=\"{}\">{}</div>\n",
            escape_attribute(name),
            children()?
        ),
    })
}

fn special_block(id: &str, kind: &str, source: &str) -> Result<String, RenderError> {
    let payload = serde_json::to_string(source)
        .map_err(|_| RenderError::InvalidRawHtmlBoundary)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    Ok(format!(
        "<div data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"{kind}\"><script type=\"application/json\">{}</script></div>\n",
        payload
    ))
}

fn list_block(id: &str, ordered: bool, start: u64, tight: bool, content: String) -> String {
    let tight = if tight {
        " data-fleximark-tight=\"true\""
    } else {
        ""
    };
    if ordered {
        format!(
            "<ol data-fleximark-node-id=\"{id}\" start=\"{start}\"{tight}>{}</ol>\n",
            content
        )
    } else {
        format!(
            "<ul data-fleximark-node-id=\"{id}\"{tight}>{}</ul>\n",
            content
        )
    }
}

fn list_item(id: &str, checked: Option<bool>, content: String) -> String {
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
        content
    )
}

fn code_block(
    id: &str,
    language: Option<&str>,
    title: Option<&str>,
    line_numbers: bool,
    code: &str,
) -> String {
    let language = language.map(escape_attribute).unwrap_or_default();
    let caption = title
        .map(|value| {
            format!(
                "<figcaption class=\"fleximark-code-title\">{}</figcaption>",
                escape_text(value)
            )
        })
        .unwrap_or_default();
    let code = if line_numbers {
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

fn media_block(id: &str, source: &str, context: &RenderContext) -> Result<String, RenderError> {
    Ok(if youtube_video_id(source).is_some() {
        format!(
            "<div data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"{}\" data-source=\"{}\"></div>\n",
            BuiltinSyntaxKind::Youtube.render_kind(),
            escape_attribute(source)
        )
    } else {
        match resolve_resource(source, context, ResourceUse::Image) {
            Ok(source) => format!(
                "<img data-fleximark-node-id=\"{id}\" src=\"{}\" alt=\"\">\n",
                escape_attribute(&source)
            ),
            Err(RenderError::ResourceRejected(_)) if context.target == HtmlTarget::Preview => {
                format!(
                    "<span data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"asset-placeholder\" role=\"img\">Asset unavailable</span>\n"
                )
            }
            Err(error) => return Err(error),
        }
    })
}

fn paragraph(id: &str, content: String) -> String {
    format!("<p data-fleximark-node-id=\"{id}\">{}</p>\n", content)
}

fn heading(id: &str, level: u8, content: String) -> String {
    let level = level.clamp(1, 6);
    format!(
        "<h{level} data-fleximark-node-id=\"{id}\">{}</h{level}>\n",
        content
    )
}

fn quote(id: &str, content: String) -> String {
    format!(
        "<blockquote data-fleximark-node-id=\"{id}\">{}</blockquote>\n",
        content
    )
}

fn table(id: &str, content: String) -> String {
    format!(
        "<table data-fleximark-node-id=\"{id}\"><tbody>{}</tbody></table>\n",
        content
    )
}

fn table_row(id: &str, content: String) -> String {
    format!("<tr data-fleximark-node-id=\"{id}\">{}</tr>\n", content)
}

fn table_cell(id: &str, header: bool, content: String) -> String {
    let tag = if header { "th" } else { "td" };
    format!(
        "<{tag} data-fleximark-node-id=\"{id}\">{}</{tag}>\n",
        content
    )
}

fn math_block(id: &str, source: &str) -> String {
    format!(
        "<div data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"{}\">{}</div>\n",
        BuiltinSyntaxKind::Math.render_kind(),
        escape_text(source)
    )
}

fn thematic_break(id: &str) -> String {
    format!("<hr data-fleximark-node-id=\"{id}\">\n")
}

fn admonition(id: &str, kind: &str, title: &str, content: String) -> String {
    format!(
        "<aside data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"{}\" data-admonition-kind=\"{}\"><header class=\"fleximark-admonition-title\">{}</header>{}</aside>\n",
        BuiltinSyntaxKind::Admonition.render_kind(),
        escape_attribute(kind),
        escape_text(title),
        content
    )
}

fn tabs(id: &str, content: String) -> String {
    format!(
        "<section data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"{}\">{}</section>\n",
        BuiltinSyntaxKind::Tabs.render_kind(),
        content
    )
}

fn tab(id: &str, label: &str, content: String) -> String {
    format!(
        "<section data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"{}\" data-tab-label=\"{}\">{}</section>\n",
        BuiltinSyntaxKind::Tab.render_kind(),
        escape_attribute(label),
        content
    )
}

fn details(id: &str, summary: &str, content: String) -> String {
    format!(
        "<details data-fleximark-node-id=\"{id}\"><summary>{}</summary>{}</details>\n",
        escape_text(summary),
        content
    )
}

fn raw_html(id: &str, html: &str) -> String {
    format!(
        "<div data-fleximark-node-id=\"{id}\" data-fleximark-kind=\"raw-html\">{}</div>\n",
        clean_raw_html(&neutralize_internal_markers(html))
    )
}
