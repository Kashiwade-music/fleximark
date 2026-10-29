//! Role: Enforce target resource URL and opaque asset reference policy.

use crate::types::{HtmlTarget, RenderContext, RenderError};

#[derive(Clone, Copy)]
pub(crate) enum ResourceUse {
    Link,
    Image,
}

pub(crate) fn resolve_resource(
    url: &str,
    context: &RenderContext,
    resource_use: ResourceUse,
) -> Result<String, RenderError> {
    ResourcePolicy { context }.resolve(url, resource_use)
}

struct ResourcePolicy<'a> {
    context: &'a RenderContext,
}

impl ResourcePolicy<'_> {
    fn resolve(&self, url: &str, resource_use: ResourceUse) -> Result<String, RenderError> {
        let context = self.context;
        if let Some(reference) = context
            .resolved_resources
            .get(url)
            .filter(|_| matches!(resource_use, ResourceUse::Image))
        {
            if let Ok(reference) = fleximark_model::AssetRef::try_from(reference.as_str()) {
                return Ok(reference.as_str().to_owned());
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
                "data:image/avif;base64,",
            ]
            .iter()
            .any(|prefix| lower.starts_with(prefix));
        let is_protocol_relative = lower.starts_with("//") || lower.starts_with("\\\\");
        let has_forbidden_character = url.chars().any(char::is_control) || lower.contains('\\');
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
                    && (matches!(resource_use, ResourceUse::Link)
                        || context.allow_remote_resources))
                || (safe_data_image && context.allow_data_resources)
                || !lower.contains(':'));
        if allowed {
            Ok(url.to_owned())
        } else {
            Err(RenderError::ResourceRejected(url.to_owned()))
        }
    }
}
