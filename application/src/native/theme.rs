//! Role: Native workspace theme operations and validation.
use std::fs;
use std::path::Path;

use crate::outcomes::ThemeResource;
use fleximark_engine::{RenderConfig, RenderStyle};

use crate::native::paths::reject_linked_path;
use crate::native::workspace::open_control_directory;
use crate::native::{ApplicationError, path_to_file_uri, workspace_path};

pub fn edit_theme(workspace_uri: &str) -> Result<ThemeResource, ApplicationError> {
    let root = workspace_path(workspace_uri)?;
    open_control_directory(&root, false)?.ok_or(ApplicationError::NotInitialized)?;
    let theme = root.join(".fleximark/theme.css");
    if !fs::symlink_metadata(&theme)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    {
        return Err(ApplicationError::NotInitialized);
    }
    Ok(ThemeResource {
        uri: path_to_file_uri(&theme)?,
    })
}

pub fn configured_render_config(
    workspace_uri: &str,
    trusted: bool,
    generation: u64,
) -> Result<RenderConfig, ApplicationError> {
    use super::workspace::{CONFIG, open_regular_nofollow};
    use fleximark_config::FlexiMarkConfig;
    use std::io::Read;
    let workspace = workspace_path(workspace_uri)?;
    let source = if trusted {
        match open_control_directory(&workspace, false)? {
            Some(control) => match open_regular_nofollow(&control, Path::new("config.toml")) {
                Ok(mut file) => {
                    let mut source = String::new();
                    file.read_to_string(&mut source)?;
                    source
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => CONFIG.to_owned(),
                Err(error) => return Err(error.into()),
            },
            None => CONFIG.to_owned(),
        }
    } else {
        CONFIG.to_owned()
    };
    FlexiMarkConfig::from_toml(&source)?;
    render_config(
        &workspace,
        super::integrity::sha256(source.as_bytes()),
        generation,
        trusted,
    )
}

pub(crate) fn render_config(
    workspace: &Path,
    config_hash: String,
    generation: u64,
    trusted: bool,
) -> Result<RenderConfig, ApplicationError> {
    let mut context = RenderConfig::default().context;
    context.allow_remote_resources = false;
    let style = if trusted {
        load_theme_style(workspace)?
    } else {
        None
    };
    let mut config = RenderConfig::default();
    config.context = context;
    config.style = style;
    config.config_hash = config_hash;
    config.config_generation = generation;
    Ok(config)
}

fn load_theme_style(workspace: &Path) -> Result<Option<RenderStyle>, ApplicationError> {
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
        return Err(ApplicationError::InvalidControlPath);
    }
    let metadata = fs::symlink_metadata(&canonical)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_THEME_BYTES
    {
        return Err(ApplicationError::InvalidControlPath);
    }
    let css = fs::read_to_string(canonical)?;
    if validate_theme_css(&css).is_err() {
        return Ok(None);
    }
    Ok(Some(RenderStyle::from_validated_css(css)))
}

fn validate_theme_css(css: &str) -> Result<(), ApplicationError> {
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
        return Err(ApplicationError::InvalidControlPath);
    }
    let mut braces = 0_i32;
    for character in normalized.chars() {
        match character {
            '{' => braces += 1,
            '}' => {
                braces -= 1;
                if braces < 0 {
                    return Err(ApplicationError::InvalidControlPath);
                }
            }
            _ => {}
        }
    }
    if braces != 0 {
        return Err(ApplicationError::InvalidControlPath);
    }
    Ok(())
}

fn normalize_css(css: &str) -> Result<String, ApplicationError> {
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
                return Err(ApplicationError::InvalidControlPath);
            }
            index += 2;
            continue;
        }
        if characters[index] == '\\' {
            index += 1;
            if index >= characters.len() || matches!(characters[index], '\n' | '\r') {
                return Err(ApplicationError::InvalidControlPath);
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
                        .map_err(|_| ApplicationError::InvalidControlPath)?;
                normalized
                    .push(char::from_u32(scalar).ok_or(ApplicationError::InvalidControlPath)?);
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
    //! Role: Verify workspace operations and security invariants.
    use super::*;
    use crate::native::integrity::sha256;
    use crate::native::test_support::test_workspace;
    use crate::native::{configured_render_config, initialize_workspace};
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
        let configured = configured_render_config(&uri, true, 1).unwrap();
        let style = configured.style.expect("trusted canonical theme is loaded");
        assert_eq!(style.css(), ":root { --accent: blue; }");
        assert_eq!(style.fingerprint(), sha256(style.css().as_bytes()));

        for bypass in [
            "a { background: u/**/rl(https://evil.example/x) }",
            "a { background: u\\72 l(https://evil.example/x) }",
            "@\\69mport 'https://evil.example/x';",
        ] {
            fs::write(root.join(".fleximark/theme.css"), bypass).unwrap();
            let rejected = configured_render_config(&uri, true, 2).unwrap();
            assert!(rejected.style.is_none());
        }
        let untrusted = configured_render_config(&uri, false, 3).unwrap();
        assert!(untrusted.style.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
