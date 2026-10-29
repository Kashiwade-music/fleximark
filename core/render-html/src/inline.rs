//! Role: Render inline content and nested block sequences.

use crate::escaping::{escape_attribute, escape_text};
use crate::resource_policy::{ResourceUse, resolve_resource};
use crate::sanitize::{
    FragmentMode, ProtectedFragments, clean_raw_html, neutralize_internal_markers,
};
use crate::types::{HtmlTarget, RenderContext, RenderError};
use fleximark_model::syntax::BuiltinSyntaxKind;
use fleximark_model::{Inline, InlineKind};

pub(crate) fn render_inline_sequence(
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
    let mut fragments = ProtectedFragments::new(FragmentMode::Inline);
    for inline in inlines {
        if let InlineKind::RawHtml { html } = &inline.kind {
            fragments.raw(html);
        } else {
            fragments.protect(render_inline(inline, context)?);
        }
    }
    Ok(fragments.finish())
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
            let label = nested(children)?;
            let title = escape_attribute(title);
            match resolve_resource(destination, context, ResourceUse::Link) {
                Ok(destination) => format!(
                    "<a href=\"{}\" title=\"{title}\">{label}</a>",
                    escape_attribute(&destination),
                ),
                Err(RenderError::ResourceRejected(_)) => {
                    format!("<a title=\"{title}\">{label}</a>")
                }
                Err(error) => return Err(error),
            }
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
            "<span data-fleximark-kind=\"{}\">{}</span>",
            BuiltinSyntaxKind::Math.render_kind(),
            escape_text(source)
        ),
        InlineKind::SoftBreak => "\n".to_owned(),
        InlineKind::HardBreak => "<br>\n".to_owned(),
        InlineKind::RawHtml { html } => clean_raw_html(&neutralize_internal_markers(html)),
    })
}

fn inline_text(inline: &Inline) -> String {
    struct Text(String);
    impl fleximark_model::Visitor for Text {
        fn visit_inline(&mut self, inline: &Inline) {
            match &inline.kind {
                InlineKind::Text { value } | InlineKind::Code { value } => self.0.push_str(value),
                InlineKind::Math { source } => self.0.push_str(source),
                InlineKind::SoftBreak | InlineKind::HardBreak => self.0.push(' '),
                _ => {}
            }
        }
    }
    let mut text = Text(String::new());
    fleximark_model::walk_inlines(std::slice::from_ref(inline), &mut text);
    text.0
}
