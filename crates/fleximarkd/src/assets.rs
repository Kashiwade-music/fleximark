use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{ambient_authority, fs::Dir};
use fleximark_engine::{
    AssetDiagnostic, AssetDiagnosticKind, RenderAsset, RenderStyle, ResolvedAssets,
    ResolvedRenderAsset,
};
use fleximark_model::{BlockKind, Document, InlineKind, Node, SourceRange};
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
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta name=\"referrer\" content=\"strict-origin-when-cross-origin\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; base-uri 'none'; form-action 'none'; font-src data:; img-src 'self' https://i.ytimg.com data: blob:; media-src 'self' blob:; frame-src https://www.youtube-nocookie.com; object-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-fleximark'\"><style>{KATEX_CSS}.fleximark-token-keyword{{color:#8959a8}}.fleximark-token-string{{color:#718c00}}.fleximark-token-number{{color:#f5871f}}.fleximark-token-comment{{color:#8e908c}}</style></head><body><main id=\"preview\"></main><script nonce=\"fleximark\">{runtime}</script><script nonce=\"fleximark\" id=\"fleximark-frame\" type=\"application/json\">{frames}</script><script nonce=\"fleximark\">window.FlexiMarkPreview.boot(JSON.parse(document.getElementById('fleximark-frame').textContent));</script></body></html>"
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
) -> Result<ResolvedAssets, ServiceError> {
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

    let mut references: BTreeMap<String, Vec<Option<SourceRange>>> = BTreeMap::new();
    let mut blocks = document.blocks.iter().collect::<Vec<_>>();
    let mut inlines = Vec::new();
    while let Some(block) = blocks.pop() {
        if let BlockKind::Media { source } = &block.kind {
            references
                .entry(source.clone())
                .or_default()
                .push(block.provenance.navigation_range());
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
                references
                    .entry(source.clone())
                    .or_default()
                    .push(inline.provenance.navigation_range());
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
    let mut assets = Vec::new();
    let mut diagnostics = Vec::new();
    let mut total_bytes = 0_usize;
    for (reference, occurrences) in references {
        if reference.starts_with('#')
            || reference.starts_with('/')
            || reference.contains("://")
            || reference.starts_with("data:")
            || reference.starts_with("mailto:")
        {
            continue;
        }
        match resolve_render_asset(
            &reference,
            &source,
            &workspace,
            &workspace_dir,
            &allowed_roots,
        ) {
            Ok(asset) => {
                let is_new_content = !assets.iter().any(|existing: &ResolvedRenderAsset| {
                    existing.published().reference == asset.published().reference
                });
                let byte_length = asset.published().byte_length.get() as usize;
                if is_new_content
                    && total_bytes.saturating_add(byte_length) > MAX_RENDER_ASSETS_BYTES
                {
                    diagnostics.extend(occurrences.into_iter().map(|source_range| {
                        asset_diagnostic(
                            &reference,
                            AssetDiagnosticKind::TotalLimit,
                            "loading it would exceed the 8 MiB document asset limit",
                            source_range,
                        )
                    }));
                } else {
                    if is_new_content {
                        total_bytes += byte_length;
                    }
                    assets.push(asset);
                }
            }
            Err((kind, detail)) => diagnostics.extend(
                occurrences
                    .into_iter()
                    .map(|source_range| asset_diagnostic(&reference, kind, detail, source_range)),
            ),
        }
    }
    diagnostics.sort_by(|left, right| {
        let left_start = left
            .source_range
            .as_ref()
            .map_or(u64::MAX, |range| range.byte_start.get());
        let right_start = right
            .source_range
            .as_ref()
            .map_or(u64::MAX, |range| range.byte_start.get());
        left_start
            .cmp(&right_start)
            .then_with(|| left.source.cmp(&right.source))
    });
    Ok(ResolvedAssets {
        assets,
        diagnostics,
    })
}

fn asset_diagnostic(
    reference: &str,
    kind: AssetDiagnosticKind,
    detail: &str,
    source_range: Option<SourceRange>,
) -> AssetDiagnostic {
    AssetDiagnostic {
        source: reference.to_owned(),
        kind,
        message: format!("Cannot load asset `{reference}`: {detail}"),
        source_range,
    }
}

const MAX_RENDER_ASSET_BYTES: u64 = 1024 * 1024;
const MAX_RENDER_ASSETS_BYTES: usize = 8 * 1024 * 1024;

fn resolve_render_asset(
    reference: &str,
    source: &Path,
    workspace: &Path,
    workspace_dir: &Dir,
    allowed_roots: &[PathBuf],
) -> Result<ResolvedRenderAsset, (AssetDiagnosticKind, &'static str)> {
    let path_text = reference.split(['?', '#']).next().unwrap_or(reference);
    let decoded = percent_decode(path_text).map_err(|_| {
        (
            AssetDiagnosticKind::InvalidReference,
            "the path contains invalid percent encoding",
        )
    })?;
    let decoded = Path::new(&decoded);
    if decoded.as_os_str().is_empty()
        || decoded.is_absolute()
        || decoded
            .components()
            .any(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
    {
        return Err((
            AssetDiagnosticKind::InvalidReference,
            "the path is not a relative local path",
        ));
    }
    let candidate = source
        .parent()
        .ok_or((
            AssetDiagnosticKind::Unreadable,
            "the document has no parent directory",
        ))?
        .join(decoded);
    let lexical_candidate = normalize_lexically(&candidate);
    if !lexical_candidate.starts_with(workspace)
        || !allowed_roots
            .iter()
            .any(|root| lexical_candidate.starts_with(root))
    {
        return Err((
            AssetDiagnosticKind::OutsideRoot,
            "the file is outside the configured asset roots",
        ));
    }
    if path_contains_symlink(
        source.parent().ok_or((
            AssetDiagnosticKind::Unreadable,
            "the document has no parent directory",
        ))?,
        decoded,
    ) {
        return Err((
            AssetDiagnosticKind::Symlink,
            "symbolic links are not allowed for preview assets",
        ));
    }
    let canonical = candidate.canonicalize().map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            (AssetDiagnosticKind::Missing, "the file does not exist")
        } else {
            (
                AssetDiagnosticKind::Unreadable,
                "the path cannot be resolved",
            )
        }
    })?;
    if !canonical.starts_with(workspace)
        || !allowed_roots.iter().any(|root| canonical.starts_with(root))
    {
        return Err((
            AssetDiagnosticKind::OutsideRoot,
            "the file is outside the configured asset roots",
        ));
    }
    let path_metadata = std::fs::symlink_metadata(&canonical).map_err(|_| {
        (
            AssetDiagnosticKind::Unreadable,
            "the file metadata cannot be read",
        )
    })?;
    if !path_metadata.is_file() {
        return Err((
            AssetDiagnosticKind::NotAFile,
            "the path does not name a regular file",
        ));
    }
    let relative = canonical.strip_prefix(workspace).map_err(|_| {
        (
            AssetDiagnosticKind::OutsideRoot,
            "the file is outside the workspace",
        )
    })?;
    reject_linked_path(workspace, relative, false).map_err(|_| {
        (
            AssetDiagnosticKind::Symlink,
            "symbolic links are not allowed for preview assets",
        )
    })?;
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = workspace_dir.open_with(relative, &options).map_err(|_| {
        (
            AssetDiagnosticKind::Unreadable,
            "the file cannot be opened safely",
        )
    })?;
    let metadata = file.metadata().map_err(|_| {
        (
            AssetDiagnosticKind::Unreadable,
            "the file metadata cannot be read",
        )
    })?;
    if !metadata.is_file() {
        return Err((
            AssetDiagnosticKind::NotAFile,
            "the path does not name a regular file",
        ));
    }
    if metadata.len() > MAX_RENDER_ASSET_BYTES {
        return Err((
            AssetDiagnosticKind::Oversize,
            "the file exceeds the 1 MiB per-asset limit",
        ));
    }
    let byte_length = metadata.len() as usize;
    let mut bytes = Vec::with_capacity(byte_length);
    file.read_to_end(&mut bytes).map_err(|_| {
        (
            AssetDiagnosticKind::Unreadable,
            "the file contents cannot be read",
        )
    })?;
    let media_type = media_type_from_signature(&bytes).ok_or((
        AssetDiagnosticKind::Unsupported,
        "the file format is not supported or its signature is invalid",
    ))?;
    ResolvedRenderAsset::from_validated_bytes(reference.to_owned(), media_type.into(), &bytes)
        .map_err(|_| {
            (
                AssetDiagnosticKind::Unreadable,
                "the file could not be validated",
            )
        })
}

fn path_contains_symlink(base: &Path, relative: &Path) -> bool {
    let mut current = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(component) => current.push(component),
            Component::Prefix(_) | Component::RootDir => return false,
        }
        if std::fs::symlink_metadata(&current)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return true;
        }
    }
    false
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(component) => normalized.push(component),
        }
    }
    normalized
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
        assert!(html.contains("base-uri 'none'"));
        assert!(html.contains("form-action 'none'"));
        assert!(html.contains("img-src 'self' https://i.ytimg.com"));
        assert!(html.contains("name=\"referrer\" content=\"strict-origin-when-cross-origin\""));
        assert!(html.contains("script-src 'nonce-fleximark'"));
        assert_eq!(html.matches("nonce=\"fleximark\"").count(), 3);
        assert!(!html.contains("script-src 'unsafe-inline'"));
        assert!(html.contains("object-src 'none'"));
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
        assert_eq!(assets.assets.len(), 1);
        assert!(assets.diagnostics.is_empty());
        assert_eq!(assets.assets[0].source(), "image.png");
        assert_eq!(assets.assets[0].published().media_type, "image/png");

        let traversal = fleximark_engine::DocumentSession::open(
            fleximark_model::DocumentUri(document_uri.clone()),
            2,
            "![outside](../outside.png)\n".into(),
            fleximark_model::PositionEncoding::Utf8,
        )
        .unwrap();
        let traversal =
            resolve_render_assets(&document_uri, &workspace_uri, traversal.document()).unwrap();
        assert!(traversal.assets.is_empty());
        assert_eq!(
            traversal.diagnostics[0].kind,
            AssetDiagnosticKind::OutsideRoot
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

        assert_eq!(assets.assets.len(), 1);
        assert!(assets.diagnostics.is_empty());
        assert_eq!(assets.assets[0].source(), "image.png");
        fs::remove_dir_all(root).unwrap();
    }

    fn parsed_document(uri: &str, source: &str) -> fleximark_engine::DocumentSession {
        fleximark_engine::DocumentSession::open(
            fleximark_model::DocumentUri(uri.to_owned()),
            1,
            source.to_owned(),
            fleximark_model::PositionEncoding::Utf8,
        )
        .unwrap()
    }

    #[test]
    fn asset_failures_are_diagnostics_and_do_not_discard_valid_assets() {
        let root = test_workspace("asset-diagnostics-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        fs::write(root.join("valid.png"), b"\x89PNG\r\n\x1a\nvalid").unwrap();
        fs::write(root.join("unsupported.bin"), b"not a supported file").unwrap();
        fs::write(root.join("large.png"), vec![0_u8; 1024 * 1024 + 1]).unwrap();
        fs::create_dir(root.join("directory.png")).unwrap();
        let source = "![first missing](missing.png)\n\n![valid](valid.png)\n\n![second missing](missing.png)\n\n![unsupported](unsupported.bin)\n\n![large](large.png)\n\n![directory](directory.png)\n\n![valid again](valid.png)\n";
        let document_path = root.join("doc.md");
        fs::write(&document_path, source).unwrap();
        let document_uri = path_to_file_uri(&document_path).unwrap();
        let session = parsed_document(&document_uri, source);

        let resolved =
            resolve_render_assets(&document_uri, &workspace_uri, session.document()).unwrap();

        assert_eq!(resolved.assets.len(), 1);
        assert_eq!(resolved.assets[0].source(), "valid.png");
        assert_eq!(resolved.diagnostics.len(), 5);
        assert_eq!(
            resolved
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.kind == AssetDiagnosticKind::Missing)
                .count(),
            2
        );
        assert!(
            resolved
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == AssetDiagnosticKind::Unsupported)
        );
        assert!(
            resolved
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == AssetDiagnosticKind::Oversize)
        );
        assert!(
            resolved
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.kind == AssetDiagnosticKind::NotAFile)
        );
        let missing_lines = resolved
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.kind == AssetDiagnosticKind::Missing)
            .map(|diagnostic| {
                diagnostic
                    .source_range
                    .as_ref()
                    .expect("parsed images carry source ranges")
                    .start
                    .line
                    .get()
            })
            .collect::<Vec<_>>();
        assert_eq!(missing_lines, [0, 4]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn document_asset_budget_produces_a_diagnostic_instead_of_failing_resolution() {
        let root = test_workspace("asset-total-budget-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let mut source = String::new();
        for index in 0..9 {
            let name = format!("asset-{index}.png");
            let mut bytes = vec![index as u8; 1024 * 1024];
            bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
            fs::write(root.join(&name), bytes).unwrap();
            source.push_str(&format!("![{index}]({name})\n\n"));
        }
        let document_path = root.join("doc.md");
        fs::write(&document_path, &source).unwrap();
        let document_uri = path_to_file_uri(&document_path).unwrap();
        let session = parsed_document(&document_uri, &source);

        let resolved =
            resolve_render_assets(&document_uri, &workspace_uri, session.document()).unwrap();

        assert_eq!(resolved.assets.len(), 8);
        assert_eq!(resolved.diagnostics.len(), 1);
        assert_eq!(
            resolved.diagnostics[0].kind,
            AssetDiagnosticKind::TotalLimit
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_workspace_asset_configuration_remains_fatal() {
        let root = test_workspace("asset-config-fatal-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            "schema_version = 2\n[assets]\nroots = [\"missing-root\"]\n",
        )
        .unwrap();
        let document_path = root.join("doc.md");
        fs::write(&document_path, "![asset](image.png)\n").unwrap();
        let document_uri = path_to_file_uri(&document_path).unwrap();
        let session = parsed_document(&document_uri, "![asset](image.png)\n");

        assert!(resolve_render_assets(&document_uri, &workspace_uri, session.document()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_assets_are_reported_separately() {
        use std::os::unix::fs::symlink;

        let root = test_workspace("asset-symlink-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        fs::write(root.join("target.png"), b"\x89PNG\r\n\x1a\ntarget").unwrap();
        symlink(root.join("target.png"), root.join("linked.png")).unwrap();
        let document_path = root.join("doc.md");
        fs::write(&document_path, "![asset](linked.png)\n").unwrap();
        let document_uri = path_to_file_uri(&document_path).unwrap();
        let session = parsed_document(&document_uri, "![asset](linked.png)\n");

        let resolved =
            resolve_render_assets(&document_uri, &workspace_uri, session.document()).unwrap();
        assert!(resolved.assets.is_empty());
        assert_eq!(resolved.diagnostics[0].kind, AssetDiagnosticKind::Symlink);
        fs::remove_dir_all(root).unwrap();
    }
}
