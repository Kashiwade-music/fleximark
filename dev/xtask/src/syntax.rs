//! Role: Generate browser rendering kind constants from built-in syntax metadata.
use std::collections::BTreeMap;

pub(super) fn typescript() -> Result<String, String> {
    let kinds = fleximark_model::syntax::BUILTIN_SYNTAX
        .iter()
        .map(|descriptor| {
            (
                descriptor.render_kind.replace('-', "_").to_uppercase(),
                descriptor.render_kind,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let value = serde_json::to_string_pretty(&kinds).map_err(|error| error.to_string())?;
    Ok(format!(
        "// Generated from core/model syntax descriptors; DO NOT EDIT.\n\nexport const BUILTIN_RENDER_KINDS = {value} as const;\n"
    ))
}
