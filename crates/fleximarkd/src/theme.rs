use std::fs;
use std::path::Path;

use fleximark_engine::{RenderConfig, RenderStyle};
use fleximark_plugin_host::PluginHost;
use fleximark_plugin_sdk::{FlexiMarkConfig, RawHtmlRenderPolicy};
use fleximark_protocol::CommandResult;
use fleximark_render_html::RawHtmlPolicy;

use crate::plugins::reject_linked_path;
use crate::workspace::open_control_directory;
use crate::{ServiceError, path_to_file_uri, workspace_path};

pub fn edit_theme(workspace_uri: &str) -> Result<CommandResult, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    open_control_directory(&root, false)?.ok_or(ServiceError::NotInitialized)?;
    let theme = root.join(".fleximark/theme.css");
    if !fs::symlink_metadata(&theme)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
        return Err(ServiceError::NotInitialized);
    }
    Ok(CommandResult {
        message: None,
        open_uri: Some(path_to_file_uri(&theme)?),
    })
}

pub(crate) fn render_config(
    workspace: &Path,
    config: &FlexiMarkConfig,
    host: &PluginHost,
    trusted: bool,
) -> Result<RenderConfig, ServiceError> {
    let mut context = RenderConfig::default().context;
    context.raw_html = match config.security.raw_html_preview {
        RawHtmlRenderPolicy::Escape => RawHtmlPolicy::Escape,
        RawHtmlRenderPolicy::Reject => RawHtmlPolicy::Reject,
    };
    context.allow_remote_resources = false;
    let style = if trusted {
        load_theme_style(workspace)?
    } else {
        None
    };
    Ok(RenderConfig::for_plugins(context, style, host))
}

fn load_theme_style(workspace: &Path) -> Result<Option<RenderStyle>, ServiceError> {
    const MAX_THEME_BYTES: u64 = 256 * 1024;
    let relative = Path::new(".fleximark/theme.css");
    match fs::symlink_metadata(workspace.join(relative)) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    reject_linked_path(workspace, relative, false)?;
    let path = workspace.join(relative);
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(workspace) {
        return Err(ServiceError::InvalidControlPath);
    }
    let metadata = fs::symlink_metadata(&canonical)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_THEME_BYTES
    {
        return Err(ServiceError::InvalidControlPath);
    }
    let css = fs::read_to_string(canonical)?;
    if validate_theme_css(&css).is_err() {
        return Ok(None);
    }
    Ok(Some(RenderStyle::from_validated_css(css)))
}

fn validate_theme_css(css: &str) -> Result<(), ServiceError> {
    let normalized = normalize_css(css)?;
    let compact = normalized
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if compact.contains("url(")
        || compact.contains("@import")
        || compact.contains("expression(")
        || compact.contains("-moz-binding")
        || css.bytes().any(|byte| byte == 0)
    {
        return Err(ServiceError::InvalidControlPath);
    }
    let mut braces = 0_i32;
    for character in normalized.chars() {
        match character {
            '{' => braces += 1,
            '}' => {
                braces -= 1;
                if braces < 0 {
                    return Err(ServiceError::InvalidControlPath);
                }
            }
            _ => {}
        }
    }
    if braces != 0 {
        return Err(ServiceError::InvalidControlPath);
    }
    Ok(())
}

fn normalize_css(css: &str) -> Result<String, ServiceError> {
    let characters = css.chars().collect::<Vec<_>>();
    let mut normalized = String::with_capacity(css.len());
    let mut index = 0;
    while index < characters.len() {
        if characters[index] == '/' && characters.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < characters.len()
                && !(characters[index] == '*' && characters[index + 1] == '/')
            {
                index += 1;
            }
            if index + 1 >= characters.len() {
                return Err(ServiceError::InvalidControlPath);
            }
            index += 2;
            continue;
        }
        if characters[index] == '\\' {
            index += 1;
            if index >= characters.len() || matches!(characters[index], '\n' | '\r') {
                return Err(ServiceError::InvalidControlPath);
            }
            let start = index;
            while index < characters.len()
                && index - start < 6
                && characters[index].is_ascii_hexdigit()
            {
                index += 1;
            }
            if index > start {
                let scalar =
                    u32::from_str_radix(&characters[start..index].iter().collect::<String>(), 16)
                        .map_err(|_| ServiceError::InvalidControlPath)?;
                normalized.push(char::from_u32(scalar).ok_or(ServiceError::InvalidControlPath)?);
                if characters
                    .get(index)
                    .is_some_and(|value| value.is_whitespace())
                {
                    index += 1;
                }
            } else {
                normalized.push(characters[index]);
                index += 1;
            }
            continue;
        }
        normalized.push(characters[index]);
        index += 1;
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::sha256;
    use crate::test_support::test_workspace;
    use crate::{export_render_context, initialize_workspace, load_plugin_host};
    #[test]
    fn trusted_theme_is_typed_and_css_url_bypasses_are_rejected() {
        let root = test_workspace("theme-policy-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/theme.css"),
            ":root { --accent: blue; }",
        )
        .unwrap();
        let (_, configured) = load_plugin_host(&uri, true, 1).unwrap();
        let style = configured.style.expect("trusted canonical theme is loaded");
        assert_eq!(style.css(), ":root { --accent: blue; }");
        assert_eq!(style.fingerprint(), sha256(style.css().as_bytes()));

        for bypass in [
            "a { background: u/**/rl(https://evil.example/x) }",
            "a { background: u\\72 l(https://evil.example/x) }",
            "@\\69mport 'https://evil.example/x';",
        ] {
            fs::write(root.join(".fleximark/theme.css"), bypass).unwrap();
            let (_, rejected) = load_plugin_host(&uri, true, 2).unwrap();
            assert!(rejected.style.is_none());
        }
        let (_, untrusted) = load_plugin_host(&uri, false, 3).unwrap();
        assert!(untrusted.style.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canonical_raw_html_policies_drive_preview_and_export_separately() {
        let root = test_workspace("raw-html-policy-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            "schema_version = 1\n[security]\nraw_html_preview = \"reject\"\nraw_html_export = \"escape\"\n",
        )
        .unwrap();
        let (_, preview) = load_plugin_host(&uri, true, 1).unwrap();
        assert_eq!(preview.context.raw_html, RawHtmlPolicy::Reject);
        assert_eq!(
            export_render_context(&uri).unwrap().raw_html,
            RawHtmlPolicy::Escape
        );
        fs::remove_dir_all(root).unwrap();
    }
}
