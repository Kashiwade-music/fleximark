use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{ambient_authority, fs::Dir};
use fleximark_engine::{RenderAsset, RenderStyle, ResolvedRenderAsset};
use fleximark_model::{BlockKind, Document, InlineKind, Node};
use fleximark_plugin_sdk::FlexiMarkConfig;

use crate::export::{ObjectIdentity, sha256};
use crate::plugins::reject_linked_path;
use crate::uri::percent_decode;
use crate::workspace::{CONFIG, safe_workspace_relative_path, validate_config};
use crate::{ServiceError, workspace_path};

pub struct ExportAsset {
    pub(crate) source: Option<PathBuf>,
    pub(crate) source_identity: Option<ObjectIdentity>,
    pub(crate) bytes: Vec<u8>,
    pub(crate) path: String,
    pub(crate) content_hash: String,
}

pub struct ResolvedExportAssets {
    pub html: String,
    pub assets: Vec<ExportAsset>,
}

pub fn compose_portable_html(
    rendered_html: &str,
    style: Option<&RenderStyle>,
    common_runtime: &str,
) -> Result<String, ServiceError> {
    const KATEX_CSS: &str = include_str!("../../../web/preview-client/katex.css");
    let node_ids = rendered_html
        .split("data-fleximark-node-id=\"")
        .skip(1)
        .filter_map(|tail| tail.split_once('"').map(|(id, _)| id.to_owned()))
        .collect::<Vec<_>>();
    if node_ids.is_empty() {
        return Err(ServiceError::ExportContentConflict);
    }
    let frame = serde_json::json!({
        "previewSessionId":"portable-export",
        "documentVersion":1,
        "renderRevision":1,
        "rendererFingerprint":sha256(rendered_html.as_bytes()),
        "navigation":[],
        "style":style,
        "assets":[],
        "blocks":[{"id":node_ids[0],"nodeIds":node_ids,"html":rendered_html}],
        "annotations":{},
    });
    let frames = serde_json::to_string(&vec![frame])?
        .replace('<', "\\u003c")
        .replace('&', "\\u0026");
    let runtime = common_runtime
        .replace("</script", "<\\/script")
        .replace("</SCRIPT", "<\\/SCRIPT");
    Ok(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; font-src data:; img-src 'self' data: blob:; media-src 'self' blob:; frame-src https://www.youtube-nocookie.com; object-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'\"><style>{KATEX_CSS}.fleximark-token-keyword{{color:#8959a8}}.fleximark-token-string{{color:#718c00}}.fleximark-token-number{{color:#f5871f}}.fleximark-token-comment{{color:#8e908c}}</style></head><body><main id=\"preview\"></main><script>{runtime}</script><script id=\"fleximark-frame\" type=\"application/json\">{frames}</script><script>window.FlexiMarkPreview.boot(JSON.parse(document.getElementById('fleximark-frame').textContent));</script></body></html>"
    ))
}

pub fn resolve_export_assets(
    source_uri: &str,
    workspace_uri: &str,
    html: &str,
    render_assets: &[RenderAsset],
) -> Result<ResolvedExportAssets, ServiceError> {
    let source = workspace_path(source_uri)?;
    let workspace = workspace_path(workspace_uri)?;
    validate_config(&workspace.join(".fleximark/config.toml"))?;
    if !source.starts_with(&workspace) {
        return Err(ServiceError::InvalidExportDestination);
    }
    let mut rewritten = html.to_owned();
    let mut assets: Vec<ExportAsset> = Vec::new();
    for asset in render_assets {
        let bytes = BASE64
            .decode(&asset.data)
            .map_err(|_| ServiceError::ExportContentConflict)?;
        if sha256(&bytes) != asset.content_hash || bytes.len() as u64 != asset.byte_length {
            return Err(ServiceError::ExportContentConflict);
        }
        let extension = match asset.media_type.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "audio/mpeg" => "mp3",
            "audio/ogg" => "ogg",
            "audio/wav" => "wav",
            _ => return Err(ServiceError::ExportContentConflict),
        };
        let export_path = format!("assets/{}.{}", asset.content_hash, extension);
        rewritten = rewritten.replace(&asset.reference, &export_path);
        if !assets.iter().any(|existing| existing.path == export_path) {
            assets.push(ExportAsset {
                source: None,
                source_identity: None,
                bytes,
                path: export_path,
                content_hash: asset.content_hash.clone(),
            });
        }
    }
    Ok(ResolvedExportAssets {
        html: rewritten,
        assets,
    })
}

pub fn resolve_render_assets(
    source_uri: &str,
    workspace_uri: &str,
    document: &Document,
) -> Result<Vec<ResolvedRenderAsset>, ServiceError> {
    let source = workspace_path(source_uri)?;
    let workspace = workspace_path(workspace_uri)?;
    if !source.starts_with(&workspace) {
        return Err(ServiceError::InvalidWorkspaceUri);
    }
    let config = match validate_config(&workspace.join(".fleximark/config.toml")) {
        Ok(config) => config,
        Err(ServiceError::NotInitialized) => {
            FlexiMarkConfig::from_toml(CONFIG).map_err(|_| ServiceError::InvalidConfig)?
        }
        Err(error) => return Err(error),
    };
    let workspace_dir = Dir::open_ambient_dir(&workspace, ambient_authority())?;
    let mut allowed_roots = vec![
        source
            .parent()
            .ok_or(ServiceError::InvalidWorkspaceUri)?
            .canonicalize()?,
    ];
    for root in config.assets.roots {
        if !safe_workspace_relative_path(&root) {
            return Err(ServiceError::InvalidConfig);
        }
        reject_linked_path(&workspace, Path::new(&root), true)?;
        let canonical = workspace.join(root).canonicalize()?;
        if !canonical.starts_with(&workspace) || !canonical.is_dir() {
            return Err(ServiceError::InvalidConfig);
        }
        allowed_roots.push(canonical);
    }

    let mut references = Vec::new();
    let mut blocks = document.blocks.iter().collect::<Vec<_>>();
    let mut inlines = Vec::new();
    while let Some(block) = blocks.pop() {
        if let BlockKind::Media { source } = &block.kind {
            references.push(source.clone());
        }
        for child in &block.children {
            match child {
                Node::Block(block) => blocks.push(block),
                Node::Inline(inline) => inlines.push(inline),
            }
        }
    }
    while let Some(inline) = inlines.pop() {
        let children = match &inline.kind {
            InlineKind::Image {
                source, children, ..
            } => {
                references.push(source.clone());
                children
            }
            InlineKind::Link { children, .. }
            | InlineKind::Emphasis { children }
            | InlineKind::Strong { children }
            | InlineKind::Strikethrough { children } => children,
            _ => continue,
        };
        inlines.extend(children);
    }
    references.sort();
    references.dedup();

    let mut assets = Vec::new();
    for reference in references {
        if reference.starts_with('#')
            || reference.starts_with('/')
            || reference.contains("://")
            || reference.starts_with("data:")
            || reference.starts_with("mailto:")
        {
            continue;
        }
        let path_text = reference.split(['?', '#']).next().unwrap_or(&reference);
        let decoded = percent_decode(path_text)?;
        if Path::new(&decoded).is_absolute()
            || Path::new(&decoded).components().any(|component| {
                matches!(
                    component,
                    std::path::Component::Prefix(_) | std::path::Component::RootDir
                )
            })
        {
            return Err(ServiceError::InvalidControlPath);
        }
        let candidate = source
            .parent()
            .ok_or(ServiceError::InvalidWorkspaceUri)?
            .join(decoded);
        let canonical = candidate.canonicalize()?;
        if !canonical.starts_with(&workspace)
            || !allowed_roots.iter().any(|root| canonical.starts_with(root))
        {
            return Err(ServiceError::InvalidControlPath);
        }
        let relative = canonical
            .strip_prefix(&workspace)
            .map_err(|_| ServiceError::InvalidControlPath)?;
        reject_linked_path(&workspace, relative, false)?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = workspace_dir
            .open_with(relative, &options)
            .map_err(|_| ServiceError::InvalidControlPath)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > 1024 * 1024 {
            return Err(ServiceError::InvalidControlPath);
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let media_type =
            media_type_from_signature(&bytes).ok_or(ServiceError::InvalidControlPath)?;
        let asset = ResolvedRenderAsset::from_validated_bytes(reference, media_type.into(), &bytes)
            .map_err(|error| ServiceError::PluginConfig(error.to_string()))?;
        assets.push(asset);
    }
    Ok(assets)
}

fn media_type_from_signature(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"OggS") {
        Some("audio/ogg")
    } else if bytes.starts_with(b"ID3")
        || bytes
            .get(..2)
            .is_some_and(|prefix| prefix[0] == 0xff && prefix[1] & 0xe0 == 0xe0)
    {
        Some("audio/mpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        Some("audio/wav")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::test_workspace;
    use crate::{initialize_workspace, path_to_file_uri};
    #[test]
    fn portable_html_boots_the_common_client_with_typed_style() {
        let style = RenderStyle::from_validated_css(":root { color: red; }".into());
        let html = compose_portable_html(
            "<div data-fleximark-node-id=\"document-root\"><div data-fleximark-node-id=\"math-1\" data-fleximark-block=\"math\"></div></div>",
            Some(&style),
            "window.FlexiMarkPreview={boot(value){window.publications=value}};",
        )
        .unwrap();
        assert!(html.contains("window.FlexiMarkPreview.boot"));
        assert!(html.contains("fleximark-frame"));
        assert!(html.contains("document-root") && html.contains("math-1"));
        assert!(html.contains(style.fingerprint()));
        assert!(html.contains(":root { color: red; }"));
        assert!(html.contains("default-src 'none'"));
        assert!(html.contains("font-src data:"));
        assert!(html.contains(".katex{font:"));
    }

    #[test]
    fn live_assets_are_taken_from_typed_image_nodes_not_links_or_raw_html() {
        let root = test_workspace("live-assets-ir-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        fs::write(root.join("image.png"), b"\x89PNG\r\n\x1a\ncontent").unwrap();
        fs::write(root.join("other.md"), "# linked document\n").unwrap();
        let source = "[document](other.md)\n\n![inline](image.png \"title\")\n\n![reference][asset]\n\n[asset]: image.png \"reference title\"\n\n<img src=\"ignored.png\">\n";
        let document_path = root.join("doc.md");
        fs::write(&document_path, source).unwrap();
        let document_uri = path_to_file_uri(&document_path).unwrap();
        let session = fleximark_engine::DocumentSession::open(
            fleximark_model::DocumentUri(document_uri.clone()),
            1,
            source.into(),
            fleximark_model::PositionEncoding::Utf8,
        )
        .unwrap();
        let assets = resolve_render_assets(&document_uri, &workspace_uri, session.document())
            .expect("ordinary links and escaped raw HTML are not asset requests");
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].source(), "image.png");
        assert_eq!(assets[0].published().media_type, "image/png");

        let traversal = fleximark_engine::DocumentSession::open(
            fleximark_model::DocumentUri(document_uri.clone()),
            2,
            "![outside](../outside.png)\n".into(),
            fleximark_model::PositionEncoding::Utf8,
        )
        .unwrap();
        assert!(
            resolve_render_assets(&document_uri, &workspace_uri, traversal.document()).is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_assets_use_default_config_outside_a_fleximark_workspace() {
        let root = test_workspace("uninitialized-live-assets-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        fs::write(root.join("image.png"), b"\x89PNG\r\n\x1a\ncontent").unwrap();
        let source = "# Ordinary Markdown\n\n![inline](image.png)\n";
        let document_path = root.join("doc.md");
        fs::write(&document_path, source).unwrap();
        let document_uri = path_to_file_uri(&document_path).unwrap();
        let session = fleximark_engine::DocumentSession::open(
            fleximark_model::DocumentUri(document_uri.clone()),
            1,
            source.into(),
            fleximark_model::PositionEncoding::Utf8,
        )
        .unwrap();

        let assets = resolve_render_assets(&document_uri, &workspace_uri, session.document())
            .expect("ordinary Markdown preview uses the default workspace configuration");

        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].source(), "image.png");
        fs::remove_dir_all(root).unwrap();
    }
}
