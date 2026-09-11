use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt as _, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions as CapOpenOptions;
use cap_std::{ambient_authority, fs::Dir};
use fleximark_engine::{RenderAsset, RenderConfig, RenderStyle, ResolvedRenderAsset};
use fleximark_model::{Block, BlockKind, Document, InlineKind, Node};
use fleximark_plugin_host::{ExecutionLimits, HostPolicy, PluginHost, VerifiedPluginPackage};
use fleximark_plugin_sdk::{FlexiMarkConfig, RawHtmlRenderPolicy};
use fleximark_protocol::{CommandMessage, CommandResult, GetNoteOptionsResult};
use fleximark_render_html::{HtmlTarget, RawHtmlPolicy, RenderContext};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const CONFIG: &str = "# FlexiMark workspace configuration\nschema_version = 1\n\n[security]\nraw_html_preview = \"escape\"\nraw_html_export = \"reject\"\n";
const THEME: &str = "/* FlexiMark workspace theme */\n:root { color-scheme: light dark; }\n";

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("workspace URI must be a local file URI")]
    InvalidWorkspaceUri,
    #[error("refusing a workspace whose .fleximark path is a symbolic link")]
    LinkedControlDirectory,
    #[error("workspace is not initialized; run initializeWorkspace first")]
    NotInitialized,
    #[error(".fleximark/config.toml is not a valid version 1 configuration")]
    InvalidConfig,
    #[error("refusing a symbolic link or non-file at a FlexiMark-owned path")]
    InvalidControlPath,
    #[error("refusing an unmanaged non-empty export destination")]
    UnmanagedExport,
    #[error("export ownership marker and registry do not match")]
    ExportOwnershipConflict,
    #[error("a managed export file was changed outside FlexiMark")]
    ExportContentConflict,
    #[error("reserved export path collision")]
    ReservedExportPath,
    #[error("export destination must have an existing local parent directory")]
    InvalidExportDestination,
    #[error("plugin configuration is invalid: {0}")]
    PluginConfig(String),
    #[error("export recovery journal is invalid or conflicts with filesystem state")]
    ExportRecoveryConflict,
    #[error("the previous export must be acknowledged after opening and validation")]
    ExportAwaitingAcknowledgement,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn initialize_workspace(workspace_uri: &str) -> Result<CommandResult, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    let control = open_control_directory(&root, true)?.expect("created control directory");
    write_new_at(&control, "config.toml", CONFIG)?;
    validate_config(&root.join(".fleximark/config.toml"))?;
    write_new_at(&control, "theme.css", THEME)?;
    let config = root.join(".fleximark/config.toml");
    Ok(CommandResult {
        message: Some(CommandMessage {
            level: "info",
            text: "Initialized .fleximark/config.toml and .fleximark/theme.css".into(),
        }),
        open_uri: Some(path_to_file_uri(&config)?),
    })
}

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

pub fn get_note_options(workspace_uri: &str) -> Result<GetNoteOptionsResult, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    let config = validate_config(&root.join(".fleximark/config.toml"))?;
    Ok(GetNoteOptionsResult {
        categories: config.notes.categories.keys().cloned().collect(),
        templates: config.notes.templates.keys().cloned().collect(),
    })
}

pub fn create_note(workspace_uri: &str) -> Result<CommandResult, ServiceError> {
    create_note_with_options(workspace_uri, None, None)
}

pub fn create_note_with_options(
    workspace_uri: &str,
    category: Option<&str>,
    template: Option<&str>,
) -> Result<CommandResult, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    open_control_directory(&root, false)?.ok_or(ServiceError::NotInitialized)?;
    let config = root.join(".fleximark/config.toml");
    if !config.is_file() {
        return Err(ServiceError::NotInitialized);
    }
    let config = validate_config(&config)?;
    let mut notes = root.join("notes");
    reject_link(&notes)?;
    fs::create_dir_all(&notes)?;
    if let Some(category) = category {
        let relative = config
            .notes
            .categories
            .get(category)
            .ok_or(ServiceError::InvalidConfig)?;
        if !safe_workspace_relative_path(relative) {
            return Err(ServiceError::InvalidConfig);
        }
        for component in Path::new(relative).components() {
            let std::path::Component::Normal(component) = component else {
                return Err(ServiceError::InvalidConfig);
            };
            notes.push(component);
            reject_link(&notes)?;
            fs::create_dir(&notes).or_else(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(error)
                }
            })?;
        }
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let prefix = expand_note_text(&config.notes.file_name_prefix);
    let suffix = expand_note_text(&config.notes.file_name_suffix);
    if !safe_filename_fragment(&prefix) || !safe_filename_fragment(&suffix) {
        return Err(ServiceError::InvalidConfig);
    }
    let note = notes.join(format!("{prefix}note-{timestamp}{suffix}.md"));
    let template = template
        .or_else(|| {
            config
                .notes
                .templates
                .contains_key("default")
                .then_some("default")
        })
        .map(|name| {
            config
                .notes
                .templates
                .get(name)
                .ok_or(ServiceError::InvalidConfig)
        })
        .transpose()?;
    let contents = template
        .map(|lines| expand_note_text(&lines.join("\n")))
        .unwrap_or_else(|| "# New note".into());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&note)?;
    file.write_all(contents.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(CommandResult {
        message: Some(CommandMessage {
            level: "info",
            text: "Created a new note".into(),
        }),
        open_uri: Some(path_to_file_uri(&note)?),
    })
}

fn expand_note_text(text: &str) -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let (year, month, day) = civil_date(days as i64);
    let dated = text
        .replace("${CURRENT_YEAR}", &format!("{year:04}"))
        .replace("${CURRENT_MONTH}", &format!("{month:02}"))
        .replace("${CURRENT_DATE}", &format!("{day:02}"));
    let mut output = String::new();
    let mut rest = dated.as_str();
    while let Some(index) = rest.find('$') {
        output.push_str(&rest[..index]);
        rest = &rest[index..];
        if let Some(after) = rest.strip_prefix("${") {
            if let Some(end) = after.find('}') {
                let placeholder = &after[..end];
                if placeholder
                    .split_once(':')
                    .is_some_and(|(number, _)| number.bytes().all(|byte| byte.is_ascii_digit()))
                {
                    output.push_str(placeholder.split_once(':').unwrap().1);
                    rest = &after[end + 1..];
                    continue;
                }
                if placeholder.bytes().all(|byte| byte.is_ascii_digit()) {
                    rest = &after[end + 1..];
                    continue;
                }
            }
        } else {
            let digits = rest[1..]
                .bytes()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if digits > 0 {
                rest = &rest[digits + 1..];
                continue;
            }
        }
        output.push('$');
        rest = &rest[1..];
    }
    output.push_str(rest);
    output
}

fn civil_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_piece = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_piece + 2) / 5 + 1;
    let month = month_piece + if month_piece < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn safe_filename_fragment(value: &str) -> bool {
    !value.contains(['/', '\\', ':', '\0']) && value != "." && value != ".."
}

fn safe_workspace_relative_path(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

pub fn collect_admonitions(
    document: &Document,
    source: &str,
    workspace_uri: &str,
) -> Result<CommandResult, ServiceError> {
    fn collect<'a>(blocks: &[Block], source: &'a str, output: &mut Vec<&'a str>) {
        for block in blocks {
            if matches!(block.kind, BlockKind::Admonition { .. }) {
                if let Some(range) = block.provenance.primary_range() {
                    if let (Ok(start), Ok(end)) = (
                        usize::try_from(range.byte_start),
                        usize::try_from(range.byte_end),
                    ) {
                        if let Some(fragment) = source.get(start..end) {
                            output.push(fragment);
                        }
                    }
                }
            }
            for child in &block.children {
                if let Node::Block(child) = child {
                    collect(std::slice::from_ref(child), source, output);
                }
            }
        }
    }

    let root = workspace_path(workspace_uri)?;
    open_control_directory(&root, false)?.ok_or(ServiceError::NotInitialized)?;
    validate_config(&root.join(".fleximark/config.toml"))?;
    let mut fragments = Vec::new();
    collect(&document.blocks, source, &mut fragments);
    let notes = root.join("notes");
    reject_link(&notes)?;
    fs::create_dir_all(&notes)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let output = notes.join(format!("admonitions-{timestamp}.md"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    file.write_all(b"# Collected admonitions\n\n")?;
    file.write_all(fragments.join("\n\n").as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(CommandResult {
        message: Some(CommandMessage {
            level: "info",
            text: format!("Collected {} admonition(s)", fragments.len()),
        }),
        open_uri: Some(path_to_file_uri(&output)?),
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportFile {
    path: String,
    kind: String,
    content_hash: String,
    object_identity: ObjectIdentity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipPayload {
    format_version: u32,
    destination_id: String,
    generation: u64,
    source_identity: String,
    workspace_identity: String,
    destination_identity: String,
    destination_object_identity: ObjectIdentity,
    unsafe_output_used: bool,
    files: Vec<ExportFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OwnershipMarker {
    payload: OwnershipPayload,
    digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RegistryRecord {
    destination_id: String,
    generation: u64,
    digest: String,
    source_identity: String,
    workspace_identity: String,
    destination_identity: String,
    destination_object_identity: ObjectIdentity,
    unsafe_output_used: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum ExportJournalState {
    Prepared,
    OldMoveIntent,
    OldMoved,
    InstallIntent,
    NewInstalled,
    RegistryIntent,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ExportJournal {
    transaction_id: String,
    state: ExportJournalState,
    destination: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    registry: PathBuf,
    journal_identity: ObjectIdentity,
    registry_staging: Option<PathBuf>,
    registry_staging_identity: Option<ObjectIdentity>,
    previous_registry: Option<RegistryRecord>,
    next_registry: RegistryRecord,
    staging_identity: ObjectIdentity,
    staging_marker_identity: ObjectIdentity,
    generated_identities: Vec<RecordedObject>,
    previous_destination_objects: Vec<RecordedObject>,
    previous_destination_identity: Option<ObjectIdentity>,
    previous_registry_identity: Option<ObjectIdentity>,
    backup_identity: Option<ObjectIdentity>,
    installed_identity: Option<ObjectIdentity>,
    installed_marker_identity: Option<ObjectIdentity>,
    next_registry_identity: Option<ObjectIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ObjectIdentity {
    platform_id: String,
    kind: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct RecordedObject {
    relative_path: String,
    identity: ObjectIdentity,
    content_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportJournalRecord {
    journal: ExportJournal,
    previous_record_digest: Option<String>,
    record_digest: String,
}

struct ManagedExport {
    marker: OwnershipMarker,
    registry: RegistryRecord,
}

pub struct ExportAsset {
    source: Option<PathBuf>,
    source_identity: Option<ObjectIdentity>,
    bytes: Vec<u8>,
    path: String,
    content_hash: String,
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
    let node_ids = rendered_html
        .split("data-fleximark-node-id=\"")
        .skip(1)
        .filter_map(|tail| tail.split_once('"').map(|(id, _)| id.to_owned()))
        .collect::<Vec<_>>();
    if node_ids.is_empty() {
        return Err(ServiceError::ExportContentConflict);
    }
    let publication = serde_json::json!({
        "type":"full",
        "previewSessionId":"portable-export",
        "documentVersion":1,
        "resultRenderRevision":1,
        "rendererFingerprint":sha256(rendered_html.as_bytes()),
        "nodeIds":node_ids,
        "navigation":[],
        "style":style,
        "assets":[],
        "html":rendered_html,
    });
    let publication = serde_json::to_string(&vec![publication])?
        .replace('<', "\\u003c")
        .replace('&', "\\u0026");
    let runtime = common_runtime
        .replace("</script", "<\\/script")
        .replace("</SCRIPT", "<\\/SCRIPT");
    Ok(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self' data: blob:; media-src 'self' blob:; frame-src https://www.youtube-nocookie.com; object-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'\"><style>.fleximark-token-keyword{{color:#8959a8}}.fleximark-token-string{{color:#718c00}}.fleximark-token-number{{color:#f5871f}}.fleximark-token-comment{{color:#8e908c}}</style></head><body><main id=\"preview\"></main><script>{runtime}</script><script id=\"fleximark-publication\" type=\"application/json\">{publication}</script><script>window.FlexiMarkPreview.boot(JSON.parse(document.getElementById('fleximark-publication').textContent));</script></body></html>"
    ))
}

pub fn default_export_destination(document_uri: &str) -> Result<String, ServiceError> {
    let source = workspace_path(document_uri)?;
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::InvalidExportDestination)?;
    let destination = source
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?
        .join(format!("{stem}.fleximark-export"));
    path_to_file_uri_unchecked(&destination)
}

pub fn export_render_context(workspace_uri: &str) -> Result<RenderContext, ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    let config = validate_config(&workspace.join(".fleximark/config.toml"))?;
    Ok(RenderContext {
        target: HtmlTarget::Portable,
        raw_html: match config.security.raw_html_export {
            RawHtmlRenderPolicy::Escape => RawHtmlPolicy::Escape,
            RawHtmlRenderPolicy::Reject => RawHtmlPolicy::Reject,
        },
        allow_remote_resources: false,
        allow_data_resources: false,
        resolved_resources: Default::default(),
    })
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

pub fn export_html(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    html: &str,
    assets: &[ExportAsset],
) -> Result<CommandResult, ServiceError> {
    export_html_with_safety(
        source_uri,
        workspace_uri,
        destination_uri,
        html,
        assets,
        false,
    )
}

pub fn export_html_with_safety(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    html: &str,
    assets: &[ExportAsset],
    unsafe_output_used: bool,
) -> Result<CommandResult, ServiceError> {
    let marked_html = if unsafe_output_used {
        format!(
            "<!-- fleximark-output-safety: unsafe-plugin-html --><aside role=\"alert\" data-fleximark-output-safety=\"unsafe-plugin-html\">This export contains HTML produced by an explicitly granted unsafe plugin.</aside>{html}"
        )
    } else {
        html.to_owned()
    };
    export_html_transaction(
        source_uri,
        workspace_uri,
        destination_uri,
        &marked_html,
        assets,
        unsafe_output_used,
        |_| Ok(()),
    )
}

fn verify_export_assets(assets: &[ExportAsset]) -> Result<(), ServiceError> {
    for asset in assets {
        let (Some(source), Some(source_identity)) = (&asset.source, &asset.source_identity) else {
            continue;
        };
        if object_identity(source).ok().as_ref() != Some(source_identity) {
            return Err(ServiceError::ExportContentConflict);
        }
        let parent = Dir::open_ambient_dir(
            source
                .parent()
                .ok_or(ServiceError::InvalidExportDestination)?,
            ambient_authority(),
        )?;
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = parent.open_with(
            source
                .file_name()
                .ok_or(ServiceError::InvalidExportDestination)?,
            &options,
        )?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        if bytes != asset.bytes || sha256(&bytes) != asset.content_hash {
            return Err(ServiceError::ExportContentConflict);
        }
    }
    Ok(())
}

fn export_html_transaction(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
    html: &str,
    assets: &[ExportAsset],
    unsafe_output_used: bool,
    mut checkpoint: impl FnMut(&'static str) -> Result<(), ServiceError>,
) -> Result<CommandResult, ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    validate_config(&workspace.join(".fleximark/config.toml"))?;
    let source = workspace_path(source_uri)?;
    if source == workspace || !source.starts_with(&workspace) {
        return Err(ServiceError::InvalidExportDestination);
    }
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ServiceError::InvalidExportDestination)?;
    let parent_dir = Dir::open_ambient_dir(&parent, ambient_authority())?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ServiceError::InvalidExportDestination)?,
    );
    if destination == workspace
        || !destination.starts_with(&workspace)
        || destination.starts_with(workspace.join(".fleximark"))
    {
        return Err(ServiceError::ReservedExportPath);
    }
    if fs::symlink_metadata(&destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ServiceError::InvalidExportDestination);
    }

    let destination_identity = destination.to_string_lossy().into_owned();
    let workspace_identity = workspace.to_string_lossy().into_owned();
    let source_identity = source.to_string_lossy().into_owned();
    let registry_key = sha256(destination_identity.as_bytes());
    let registry = workspace
        .join(".fleximark/export-targets")
        .join(format!("{registry_key}.json"));
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::InvalidExportDestination)?;
    let journal_path = parent.join(format!(".{name}.fleximark-export-journal.json"));
    recover_export(&journal_path, &destination, &registry, false)?;
    reject_link(
        registry
            .parent()
            .ok_or(ServiceError::InvalidExportDestination)?,
    )?;

    let previous = inspect_export(&destination, &registry)?;
    if previous.as_ref().is_some_and(|managed| {
        managed.registry.source_identity != source_identity
            || managed.registry.workspace_identity != workspace_identity
            || managed.registry.destination_identity != destination_identity
    }) {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    let generation = previous
        .as_ref()
        .map_or(1, |managed| managed.registry.generation + 1);
    let destination_id = previous
        .as_ref()
        .map(|managed| managed.registry.destination_id.clone())
        .map(Ok)
        .unwrap_or_else(random_id)?;
    let transaction = random_id()?;
    let staging = parent.join(format!(".{name}.fleximark-export-staging-{transaction}"));
    let backup = parent.join(format!(".{name}.fleximark-export-backup-{transaction}"));
    if staging.exists() || backup.exists() {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let previous_destination_identity = fs::symlink_metadata(&destination)
        .ok()
        .map(|_| object_identity(&destination))
        .transpose()?;
    let previous_destination_objects = if destination.exists() {
        record_tree(&destination)?
    } else {
        Vec::new()
    };
    let previous_registry_identity = fs::symlink_metadata(&registry)
        .ok()
        .map(|_| object_identity(&registry))
        .transpose()?;
    let destination_name = destination
        .file_name()
        .ok_or(ServiceError::InvalidExportDestination)?
        .to_os_string();
    let staging_name = staging
        .file_name()
        .ok_or(ServiceError::InvalidExportDestination)?
        .to_os_string();
    let backup_name = backup
        .file_name()
        .ok_or(ServiceError::InvalidExportDestination)?
        .to_os_string();
    parent_dir.create_dir(&staging_name)?;
    let stage_result = (|| {
        if let Some(managed) = previous.as_ref() {
            copy_user_files(&destination, &staging, managed)?;
        }
        write_synced(&staging.join("index.html"), html.as_bytes(), true)?;
        for asset in assets {
            let target = staging.join(&asset.path);
            fs::create_dir_all(target.parent().ok_or(ServiceError::ReservedExportPath)?)?;
            if sha256(&asset.bytes) != asset.content_hash {
                return Err(ServiceError::ExportContentConflict);
            }
            write_synced(&target, &asset.bytes, true)?;
        }
        sync_directory(&staging)
    })();
    if let Err(error) = stage_result {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let destination_object_identity = object_identity(&staging)?;
    let mut files = vec![ExportFile {
        path: "index.html".into(),
        kind: "file".into(),
        content_hash: sha256(html.as_bytes()),
        object_identity: object_identity(&staging.join("index.html"))?,
    }];
    if !assets.is_empty() {
        files.push(ExportFile {
            path: "assets".into(),
            kind: "directory".into(),
            content_hash: String::new(),
            object_identity: object_identity(&staging.join("assets"))?,
        });
    }
    files.extend(
        assets
            .iter()
            .map(|asset| {
                Ok(ExportFile {
                    path: asset.path.clone(),
                    kind: "file".into(),
                    content_hash: asset.content_hash.clone(),
                    object_identity: object_identity(&staging.join(&asset.path))?,
                })
            })
            .collect::<Result<Vec<_>, ServiceError>>()?,
    );
    let payload = OwnershipPayload {
        format_version: 1,
        destination_id: destination_id.clone(),
        generation,
        source_identity: source_identity.clone(),
        workspace_identity: workspace_identity.clone(),
        destination_identity: destination_identity.clone(),
        destination_object_identity: destination_object_identity.clone(),
        unsafe_output_used,
        files,
    };
    let marker = OwnershipMarker {
        digest: payload_digest(&payload)?,
        payload,
    };
    if let Err(error) = write_json(&staging.join(".fleximark-export.json"), &marker, true)
        .and_then(|_| sync_directory(&staging))
    {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    let next_registry = RegistryRecord {
        destination_id,
        generation,
        digest: marker.digest.clone(),
        source_identity,
        workspace_identity,
        destination_identity,
        destination_object_identity,
        unsafe_output_used,
    };
    let staging_identity = object_identity(&staging)?;
    let staging_marker_identity = object_identity(&staging.join(".fleximark-export.json"))?;
    let generated_identities = record_tree(&staging)?;
    let journal_identity = create_journal(&journal_path)?;
    let mut journal = ExportJournal {
        transaction_id: transaction,
        state: ExportJournalState::Prepared,
        destination: destination.clone(),
        staging,
        backup,
        registry,
        journal_identity,
        registry_staging: None,
        registry_staging_identity: None,
        previous_registry: previous.map(|managed| managed.registry),
        next_registry,
        staging_identity,
        staging_marker_identity,
        generated_identities,
        previous_destination_objects,
        previous_destination_identity,
        previous_registry_identity,
        backup_identity: None,
        installed_identity: None,
        installed_marker_identity: None,
        next_registry_identity: None,
    };
    let mut journal_digest = append_journal(&journal_path, &journal, None)?;
    sync_directory(&parent)?;
    verify_export_assets(assets)?;
    if journal.previous_destination_identity.is_some() {
        verify_directory_tree(&destination, &journal.previous_destination_objects)?;
    }
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    journal.state = ExportJournalState::OldMoveIntent;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    checkpoint("old-move-intent")?;
    if destination.exists() {
        parent_dir.rename(&destination_name, &parent_dir, &backup_name)?;
        let backup_identity = object_identity(&journal.backup)?;
        if Some(&backup_identity) != journal.previous_destination_identity.as_ref() {
            return Err(ServiceError::ExportRecoveryConflict);
        }
        journal.backup_identity = Some(backup_identity);
        sync_directory(&parent)?;
    }
    checkpoint("old-moved")?;
    journal.state = ExportJournalState::OldMoved;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    journal.state = ExportJournalState::InstallIntent;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    checkpoint("install-intent")?;
    parent_dir.rename(&staging_name, &parent_dir, &destination_name)?;
    journal.installed_identity = Some(object_identity(&destination)?);
    journal.installed_marker_identity = Some(object_identity(
        &destination.join(".fleximark-export.json"),
    )?);
    if journal.installed_identity.as_ref() != Some(&journal.staging_identity)
        || journal.installed_marker_identity.as_ref() != Some(&journal.staging_marker_identity)
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    checkpoint("installed")?;
    journal.state = ExportJournalState::NewInstalled;
    journal_digest = append_journal(&journal_path, &journal, Some(journal_digest))?;
    sync_directory(&parent)?;
    commit_installed_export(&mut journal, &journal_path, journal_digest, &mut checkpoint)?;
    Ok(CommandResult {
        message: Some(CommandMessage {
            level: "info",
            text: format!("Exported generation {generation}"),
        }),
        open_uri: Some(path_to_file_uri(&index_for_destination(&destination))?),
    })
}

pub fn preflight_export(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
) -> Result<(), ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    validate_config(&workspace.join(".fleximark/config.toml"))?;
    let source = workspace_path(source_uri)?;
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ServiceError::InvalidExportDestination)?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ServiceError::InvalidExportDestination)?,
    );
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    let registry_key = sha256(destination.to_string_lossy().as_bytes());
    let registry = workspace
        .join(".fleximark/export-targets")
        .join(format!("{registry_key}.json"));
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::InvalidExportDestination)?;
    let journal = parent.join(format!(".{name}.fleximark-export-journal.json"));
    recover_export(&journal, &destination, &registry, false)?;
    let existing = inspect_export(&destination, &registry)?;
    if existing.as_ref().is_some_and(|managed| {
        managed.registry.source_identity != source.to_string_lossy()
            || managed.registry.workspace_identity != workspace.to_string_lossy()
            || managed.registry.destination_identity != destination.to_string_lossy()
    }) {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    Ok(())
}

pub fn acknowledge_export(
    source_uri: &str,
    workspace_uri: &str,
    destination_uri: &str,
) -> Result<(), ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    let source = workspace_path(source_uri)?;
    let destination = file_uri_path(destination_uri)?;
    let parent = destination
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?
        .canonicalize()
        .map_err(|_| ServiceError::InvalidExportDestination)?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or(ServiceError::InvalidExportDestination)?,
    );
    revalidate_export_paths(&workspace, &source, &destination, &parent)?;
    let registry_key = sha256(destination.to_string_lossy().as_bytes());
    let registry = workspace
        .join(".fleximark/export-targets")
        .join(format!("{registry_key}.json"));
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::InvalidExportDestination)?;
    let journal = parent.join(format!(".{name}.fleximark-export-journal.json"));
    if !journal.exists() {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    recover_export(&journal, &destination, &registry, true)
}

fn inspect_export(
    destination: &Path,
    registry_path: &Path,
) -> Result<Option<ManagedExport>, ServiceError> {
    if !destination.exists() {
        return if registry_path.exists() {
            Err(ServiceError::ExportOwnershipConflict)
        } else {
            Ok(None)
        };
    }
    let directory = Dir::open_ambient_dir(destination, ambient_authority())?;
    let destination_identity = object_identity_from_metadata(&directory.metadata(".")?)?;
    if destination_identity.kind != "directory" {
        return Err(ServiceError::InvalidExportDestination);
    }
    if directory
        .symlink_metadata(".fleximark-export.json")
        .is_err()
    {
        return if directory.entries()?.next().is_none() {
            Ok(None)
        } else {
            Err(ServiceError::UnmanagedExport)
        };
    }
    let marker: OwnershipMarker = serde_json::from_slice(&read_regular_at(
        &directory,
        Path::new(".fleximark-export.json"),
    )?)?;
    if marker.payload.format_version != 1 || payload_digest(&marker.payload)? != marker.digest {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    if destination_identity != marker.payload.destination_object_identity {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    let registry: RegistryRecord = read_json_regular(registry_path)?;
    if registry.destination_id != marker.payload.destination_id
        || registry.generation != marker.payload.generation
        || registry.digest != marker.digest
        || registry.source_identity != marker.payload.source_identity
        || registry.workspace_identity != marker.payload.workspace_identity
        || registry.destination_identity != marker.payload.destination_identity
        || registry.destination_object_identity != marker.payload.destination_object_identity
    {
        return Err(ServiceError::ExportOwnershipConflict);
    }
    let mut owned_paths = std::collections::HashSet::new();
    for file in &marker.payload.files {
        if !matches!(file.kind.as_str(), "file" | "directory")
            || !safe_relative_path(&file.path)
            || !owned_paths.insert(file.path.as_str())
        {
            return Err(ServiceError::ExportOwnershipConflict);
        }
        let relative = Path::new(&file.path);
        let (parent, name) = open_relative_parent(&directory, relative)
            .map_err(|_| ServiceError::ExportContentConflict)?;
        let metadata = parent
            .symlink_metadata(&name)
            .map_err(|_| ServiceError::ExportContentConflict)?;
        if metadata.file_type().is_symlink()
            || (file.kind == "file" && !metadata.is_file())
            || (file.kind == "directory" && !metadata.is_dir())
            || object_identity_from_metadata(&metadata)? != file.object_identity
        {
            return Err(ServiceError::ExportContentConflict);
        }
        if file.kind == "file"
            && sha256(&read_regular_at(&directory, relative)?) != file.content_hash
        {
            return Err(ServiceError::ExportContentConflict);
        }
    }
    Ok(Some(ManagedExport { marker, registry }))
}

fn revalidate_export_paths(
    workspace: &Path,
    source: &Path,
    destination: &Path,
    expected_parent: &Path,
) -> Result<(), ServiceError> {
    let parent = destination
        .parent()
        .ok_or(ServiceError::InvalidExportDestination)?;
    if parent.canonicalize()? != expected_parent
        || !expected_parent.starts_with(workspace)
        || destination == workspace
        || destination.starts_with(workspace.join(".fleximark"))
    {
        return Err(ServiceError::InvalidExportDestination);
    }
    let mut current = workspace.to_path_buf();
    for component in expected_parent
        .strip_prefix(workspace)
        .map_err(|_| ServiceError::InvalidExportDestination)?
        .components()
    {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ServiceError::InvalidExportDestination);
        }
    }
    let source_metadata = fs::symlink_metadata(source)?;
    if !source_metadata.is_file() || source_metadata.file_type().is_symlink() {
        return Err(ServiceError::InvalidExportDestination);
    }
    if fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(ServiceError::InvalidExportDestination);
    }
    Ok(())
}

fn copy_user_files(
    source: &Path,
    destination: &Path,
    previous: &ManagedExport,
) -> Result<(), ServiceError> {
    let owned = previous
        .marker
        .payload
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut pending = vec![(
        Dir::open_ambient_dir(source, ambient_authority())?,
        Dir::open_ambient_dir(destination, ambient_authority())?,
        String::new(),
    )];
    while let Some((from, to, prefix)) = pending.pop() {
        for entry in from.entries()? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ServiceError::ReservedExportPath)?;
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if relative == ".fleximark-export.json" {
                continue;
            }
            if reserved_export_path(&relative) {
                return Err(ServiceError::ReservedExportPath);
            }
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(ServiceError::ExportContentConflict);
            }
            if file_type.is_dir() {
                to.create_dir(&name)?;
                pending.push((entry.open_dir()?, to.open_dir_nofollow(&name)?, relative));
            } else if file_type.is_file() {
                if owned.contains(relative.as_str()) {
                    continue;
                }
                let mut read_options = CapOpenOptions::new();
                read_options.read(true).follow(FollowSymlinks::No);
                let mut input = entry.open_with(&read_options)?;
                let before = object_identity_from_metadata(&input.metadata()?)?;
                let mut bytes = Vec::new();
                input.read_to_end(&mut bytes)?;
                input.rewind()?;
                let mut repeated = Vec::new();
                input.read_to_end(&mut repeated)?;
                if bytes != repeated || object_identity_from_metadata(&input.metadata()?)? != before
                {
                    return Err(ServiceError::ExportContentConflict);
                }
                let mut write_options = CapOpenOptions::new();
                write_options
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .follow(FollowSymlinks::No);
                let mut output = to.open_with(&name, &write_options)?;
                output.write_all(&bytes)?;
                output.sync_all()?;
                output.rewind()?;
                let mut copied = Vec::new();
                output.read_to_end(&mut copied)?;
                if sha256(&copied) != sha256(&bytes) {
                    return Err(ServiceError::ExportContentConflict);
                }
            } else {
                return Err(ServiceError::ExportContentConflict);
            }
        }
    }
    Ok(())
}

fn commit_installed_export(
    journal: &mut ExportJournal,
    journal_path: &Path,
    previous_digest: String,
    checkpoint: &mut dyn FnMut(&'static str) -> Result<(), ServiceError>,
) -> Result<(), ServiceError> {
    verify_installed(&journal.destination, &journal.next_registry)?;
    verify_directory_tree(&journal.destination, &journal.generated_identities)?;
    if object_identity(&journal.destination).ok().as_ref() != journal.installed_identity.as_ref()
        || object_identity(&journal.destination.join(".fleximark-export.json"))
            .ok()
            .as_ref()
            != journal.installed_marker_identity.as_ref()
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let registry_parent = journal
        .registry
        .parent()
        .ok_or(ServiceError::ExportRecoveryConflict)?
        .to_owned();
    reject_link(&registry_parent)?;
    fs::create_dir_all(&registry_parent)?;
    if fs::symlink_metadata(&journal.registry).is_ok()
        && object_identity(&journal.registry).ok().as_ref()
            != journal.previous_registry_identity.as_ref()
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let (registry_staging, registry_staging_identity) =
        stage_json(&registry_parent, &journal.next_registry)?;
    journal.registry_staging = Some(registry_staging);
    journal.registry_staging_identity = Some(registry_staging_identity);
    journal.state = ExportJournalState::RegistryIntent;
    let intent_digest = append_journal(journal_path, journal, Some(previous_digest))?;
    sync_directory(&registry_parent)?;
    checkpoint("registry-intent")?;
    install_staged_registry(journal)?;
    sync_directory(&registry_parent)?;
    checkpoint("registry-replaced")?;
    journal.state = ExportJournalState::Committed;
    append_journal(journal_path, journal, Some(intent_digest))?;
    sync_directory(
        journal_path
            .parent()
            .ok_or(ServiceError::ExportRecoveryConflict)?,
    )?;
    checkpoint("committed")?;
    Ok(())
}

fn install_staged_registry(journal: &mut ExportJournal) -> Result<(), ServiceError> {
    let staging = journal
        .registry_staging
        .as_ref()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    let expected = journal
        .registry_staging_identity
        .as_ref()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    if object_identity(staging).ok().as_ref() != Some(expected) {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    replace_staged_json(staging, &journal.registry)?;
    let installed = object_identity(&journal.registry)?;
    if &installed != expected {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    journal.next_registry_identity = Some(installed);
    Ok(())
}

fn recover_export(
    journal_path: &Path,
    expected_destination: &Path,
    expected_registry: &Path,
    acknowledge: bool,
) -> Result<(), ServiceError> {
    if !journal_path.exists() {
        return Ok(());
    }
    let (mut journal, journal_digest) = read_journal(journal_path)?;
    let parent = journal_path
        .parent()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    let name = journal
        .destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    let valid = journal.destination == expected_destination
        && journal.registry == expected_registry
        && journal.destination.parent() == Some(parent)
        && journal.staging.parent() == Some(parent)
        && journal.backup.parent() == Some(parent)
        && journal
            .staging
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with(&format!(".{name}.fleximark-export-staging-")))
        && journal
            .backup
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.starts_with(&format!(".{name}.fleximark-export-backup-")))
        && journal.registry_staging.as_ref().is_none_or(|staging| {
            staging.parent() == journal.registry.parent()
                && staging
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.starts_with(".registry-") && value.ends_with(".tmp"))
        });
    if !valid {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    if object_identity(journal_path).ok() != Some(journal.journal_identity.clone()) {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    match journal.state {
        ExportJournalState::Prepared => {
            if object_identity(&journal.staging).ok() != Some(journal.staging_identity.clone())
                || object_identity(&journal.staging.join(".fleximark-export.json")).ok()
                    != Some(journal.staging_marker_identity.clone())
                || journal.backup.exists()
                || object_identity(&journal.destination).ok()
                    != journal.previous_destination_identity
            {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            verify_directory_tree(&journal.staging, &journal.generated_identities)?;
            remove_recorded_directory(
                &journal.staging,
                &journal.staging_identity,
                &journal.generated_identities,
            )?;
            remove_recorded_file(journal_path, &journal.journal_identity)?;
        }
        ExportJournalState::OldMoveIntent => {
            let destination_identity = object_identity(&journal.destination).ok();
            let backup_identity = object_identity(&journal.backup).ok();
            if destination_identity == journal.previous_destination_identity
                && backup_identity.is_none()
            {
                discard_staging(&journal, journal_path)?;
            } else if destination_identity.is_none()
                && backup_identity == journal.previous_destination_identity
                && journal.previous_destination_identity.is_some()
            {
                restore_previous_destination(&journal)?;
                discard_staging(&journal, journal_path)?;
            } else if destination_identity.is_none()
                && backup_identity.is_none()
                && journal.previous_destination_identity.is_none()
            {
                discard_staging(&journal, journal_path)?;
            } else {
                return Err(ServiceError::ExportRecoveryConflict);
            }
        }
        ExportJournalState::OldMoved => {
            if journal.destination.exists()
                || object_identity(&journal.staging).ok() != Some(journal.staging_identity.clone())
                || object_identity(&journal.backup).ok() != journal.backup_identity
            {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            if journal.previous_destination_identity.is_some() {
                restore_previous_destination(&journal)?;
            }
            discard_staging(&journal, journal_path)?;
        }
        ExportJournalState::InstallIntent => {
            let staging_identity = object_identity(&journal.staging).ok();
            let destination_identity = object_identity(&journal.destination).ok();
            if staging_identity == Some(journal.staging_identity.clone())
                && destination_identity.is_none()
            {
                if journal.previous_destination_identity.is_some() {
                    restore_previous_destination(&journal)?;
                }
                discard_staging(&journal, journal_path)?;
            } else if staging_identity.is_none()
                && destination_identity == Some(journal.staging_identity.clone())
            {
                journal.installed_identity = Some(journal.staging_identity.clone());
                journal.installed_marker_identity = Some(journal.staging_marker_identity.clone());
                verify_directory_tree(&journal.destination, &journal.generated_identities)?;
                journal.state = ExportJournalState::NewInstalled;
                let digest = append_journal(journal_path, &journal, Some(journal_digest))?;
                commit_installed_export(&mut journal, journal_path, digest, &mut |_| Ok(()))?;
                return Err(ServiceError::ExportAwaitingAcknowledgement);
            } else {
                return Err(ServiceError::ExportRecoveryConflict);
            }
        }
        ExportJournalState::NewInstalled => {
            if object_identity(&journal.destination).ok() != journal.installed_identity
                || object_identity(&journal.destination.join(".fleximark-export.json")).ok()
                    != journal.installed_marker_identity
                || journal.staging.exists()
                || (journal.backup.exists()
                    && object_identity(&journal.backup).ok() != journal.backup_identity)
            {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            verify_directory_tree(&journal.destination, &journal.generated_identities)?;
            commit_installed_export(&mut journal, journal_path, journal_digest, &mut |_| Ok(()))?;
            return Err(ServiceError::ExportAwaitingAcknowledgement);
        }
        ExportJournalState::RegistryIntent => {
            verify_installed(&journal.destination, &journal.next_registry)?;
            verify_directory_tree(&journal.destination, &journal.generated_identities)?;
            let registry_staging = journal
                .registry_staging
                .as_ref()
                .ok_or(ServiceError::ExportRecoveryConflict)?;
            let staged_identity = journal
                .registry_staging_identity
                .as_ref()
                .ok_or(ServiceError::ExportRecoveryConflict)?;
            let current_registry = object_identity(&journal.registry).ok();
            if object_identity(registry_staging).ok().as_ref() == Some(staged_identity)
                && current_registry == journal.previous_registry_identity
            {
                install_staged_registry(&mut journal)?;
                sync_directory(
                    journal
                        .registry
                        .parent()
                        .ok_or(ServiceError::ExportRecoveryConflict)?,
                )?;
            } else if !registry_staging.exists()
                && current_registry.as_ref() == Some(staged_identity)
            {
                journal.next_registry_identity = current_registry;
            } else {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            journal.state = ExportJournalState::Committed;
            append_journal(journal_path, &journal, Some(journal_digest))?;
            return Err(ServiceError::ExportAwaitingAcknowledgement);
        }
        ExportJournalState::Committed => {
            if object_identity(&journal.destination).ok() != journal.installed_identity
                || object_identity(&journal.destination.join(".fleximark-export.json")).ok()
                    != journal.installed_marker_identity
                || object_identity(&journal.registry).ok() != journal.next_registry_identity
                || (journal.backup.exists()
                    && object_identity(&journal.backup).ok() != journal.backup_identity)
            {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            verify_directory_tree(&journal.destination, &journal.generated_identities)?;
            let managed = inspect_export(&journal.destination, &journal.registry)?
                .ok_or(ServiceError::ExportRecoveryConflict)?;
            if managed.registry != journal.next_registry {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            if !acknowledge {
                return Err(ServiceError::ExportAwaitingAcknowledgement);
            }
            if journal.backup.exists() {
                remove_recorded_directory(
                    &journal.backup,
                    journal
                        .backup_identity
                        .as_ref()
                        .ok_or(ServiceError::ExportRecoveryConflict)?,
                    &journal.previous_destination_objects,
                )?;
            }
            remove_recorded_file(journal_path, &journal.journal_identity)?;
        }
    }
    sync_directory(parent)
}

fn verify_installed(destination: &Path, expected: &RegistryRecord) -> Result<(), ServiceError> {
    let directory = Dir::open_ambient_dir(destination, ambient_authority())?;
    let marker: OwnershipMarker = serde_json::from_slice(&read_regular_at(
        &directory,
        Path::new(".fleximark-export.json"),
    )?)?;
    if payload_digest(&marker.payload)? != marker.digest
        || marker.digest != expected.digest
        || marker.payload.destination_id != expected.destination_id
        || marker.payload.generation != expected.generation
        || marker.payload.source_identity != expected.source_identity
        || marker.payload.workspace_identity != expected.workspace_identity
        || marker.payload.destination_identity != expected.destination_identity
        || marker.payload.destination_object_identity != expected.destination_object_identity
        || object_identity_from_metadata(&directory.metadata(".")?)?
            != expected.destination_object_identity
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let mut owned_paths = std::collections::HashSet::new();
    for file in &marker.payload.files {
        let relative = Path::new(&file.path);
        let (parent, name) = open_relative_parent(&directory, relative)?;
        let metadata = parent.symlink_metadata(&name).ok();
        if !safe_relative_path(&file.path)
            || !owned_paths.insert(file.path.as_str())
            || metadata
                .as_ref()
                .is_none_or(|metadata| metadata.file_type().is_symlink())
            || (file.kind == "file"
                && (metadata.as_ref().is_none_or(|metadata| !metadata.is_file())
                    || sha256(&read_regular_at(&directory, relative)?) != file.content_hash))
            || (file.kind == "directory"
                && metadata.as_ref().is_none_or(|metadata| !metadata.is_dir()))
            || object_identity_from_metadata(
                &metadata.ok_or(ServiceError::ExportRecoveryConflict)?,
            )? != file.object_identity
            || !matches!(file.kind.as_str(), "file" | "directory")
        {
            return Err(ServiceError::ExportRecoveryConflict);
        }
    }
    Ok(())
}

fn create_journal(path: &Path) -> Result<ObjectIdentity, ServiceError> {
    let (parent, name) = open_parent(path)?;
    let mut options = CapOpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let file = parent.open_with(&name, &options)?;
    file.sync_all()?;
    object_identity_from_metadata(&file.metadata()?)
}

fn append_journal(
    path: &Path,
    journal: &ExportJournal,
    previous_record_digest: Option<String>,
) -> Result<String, ServiceError> {
    let (parent, name) = open_parent(path)?;
    let mut options = CapOpenOptions::new();
    options.read(true).append(true).follow(FollowSymlinks::No);
    let mut file = parent.open_with(&name, &options)?;
    if object_identity_from_metadata(&file.metadata()?)? != journal.journal_identity {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let digest = sha256(&serde_json::to_vec(&(
        journal,
        previous_record_digest.as_deref(),
    ))?);
    let record = ExportJournalRecord {
        journal: journal.clone(),
        previous_record_digest,
        record_digest: digest.clone(),
    };
    serde_json::to_writer(&mut file, &record)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(digest)
}

fn read_journal(path: &Path) -> Result<(ExportJournal, String), ServiceError> {
    let (parent, name) = open_parent(path)?;
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = parent.open_with(&name, &options)?;
    let identity = object_identity_from_metadata(&file.metadata()?)?;
    if identity.kind != "file" {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    if object_identity_from_metadata(&file.metadata()?)? != identity {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let mut previous: Option<ExportJournalRecord> = None;
    for line in text.lines() {
        let record: ExportJournalRecord =
            serde_json::from_str(line).map_err(|_| ServiceError::ExportRecoveryConflict)?;
        let expected = sha256(&serde_json::to_vec(&(
            &record.journal,
            record.previous_record_digest.as_deref(),
        ))?);
        if expected != record.record_digest
            || record.previous_record_digest
                != previous.as_ref().map(|record| record.record_digest.clone())
        {
            return Err(ServiceError::ExportRecoveryConflict);
        }
        if let Some(prior) = &previous {
            let transition = matches!(
                (&prior.journal.state, &record.journal.state),
                (
                    ExportJournalState::Prepared,
                    ExportJournalState::OldMoveIntent
                ) | (
                    ExportJournalState::OldMoveIntent,
                    ExportJournalState::OldMoved
                ) | (
                    ExportJournalState::OldMoved,
                    ExportJournalState::InstallIntent
                ) | (
                    ExportJournalState::InstallIntent,
                    ExportJournalState::NewInstalled
                ) | (
                    ExportJournalState::NewInstalled,
                    ExportJournalState::RegistryIntent
                ) | (
                    ExportJournalState::RegistryIntent,
                    ExportJournalState::Committed
                )
            );
            let registry_staging_is_stable = if matches!(
                (&prior.journal.state, &record.journal.state),
                (
                    ExportJournalState::NewInstalled,
                    ExportJournalState::RegistryIntent
                )
            ) {
                prior.journal.registry_staging.is_none()
                    && prior.journal.registry_staging_identity.is_none()
                    && record.journal.registry_staging.is_some()
                    && record.journal.registry_staging_identity.is_some()
            } else {
                record.journal.registry_staging == prior.journal.registry_staging
                    && record.journal.registry_staging_identity
                        == prior.journal.registry_staging_identity
            };
            let stable = record.journal.transaction_id == prior.journal.transaction_id
                && record.journal.destination == prior.journal.destination
                && record.journal.staging == prior.journal.staging
                && record.journal.backup == prior.journal.backup
                && record.journal.registry == prior.journal.registry
                && record.journal.journal_identity == prior.journal.journal_identity
                && record.journal.previous_registry == prior.journal.previous_registry
                && record.journal.next_registry == prior.journal.next_registry
                && record.journal.staging_identity == prior.journal.staging_identity
                && record.journal.staging_marker_identity == prior.journal.staging_marker_identity
                && record.journal.generated_identities == prior.journal.generated_identities
                && record.journal.previous_destination_objects
                    == prior.journal.previous_destination_objects
                && record.journal.previous_destination_identity
                    == prior.journal.previous_destination_identity
                && record.journal.previous_registry_identity
                    == prior.journal.previous_registry_identity
                && registry_staging_is_stable;
            if !transition || !stable {
                return Err(ServiceError::ExportRecoveryConflict);
            }
        } else if record.journal.state != ExportJournalState::Prepared {
            return Err(ServiceError::ExportRecoveryConflict);
        }
        previous = Some(record);
    }
    previous
        .map(|record| (record.journal, record.record_digest))
        .ok_or(ServiceError::ExportRecoveryConflict)
}

fn stage_json<T: Serialize>(
    parent: &Path,
    value: &T,
) -> Result<(PathBuf, ObjectIdentity), ServiceError> {
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".registry-{}.tmp", random_id()?));
    write_json(&temporary, value, true)?;
    let identity = object_identity(&temporary)?;
    sync_directory(parent)?;
    Ok((temporary, identity))
}

fn replace_staged_json(temporary: &Path, path: &Path) -> Result<(), ServiceError> {
    let parent = path.parent().ok_or(ServiceError::ExportRecoveryConflict)?;
    if temporary.parent() != Some(parent) {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let directory = Dir::open_ambient_dir(parent, ambient_authority())?;
    directory.rename(
        temporary
            .file_name()
            .ok_or(ServiceError::ExportRecoveryConflict)?,
        &directory,
        path.file_name()
            .ok_or(ServiceError::ExportRecoveryConflict)?,
    )?;
    sync_directory(parent)
}

fn read_json_regular<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ServiceError> {
    let bytes = read_regular_from_parent(path).map_err(|error| match error {
        ServiceError::Json(error) => ServiceError::Json(error),
        _ => ServiceError::ExportOwnershipConflict,
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_json<T: Serialize>(path: &Path, value: &T, create_new: bool) -> Result<(), ServiceError> {
    write_synced(path, &serde_json::to_vec_pretty(value)?, create_new)
}

fn write_synced(path: &Path, bytes: &[u8], create_new: bool) -> Result<(), ServiceError> {
    let (parent, name) = open_parent(path)?;
    let mut options = CapOpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    options.follow(FollowSymlinks::No);
    let mut file = parent.open_with(&name, &options)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn payload_digest(payload: &OwnershipPayload) -> Result<String, ServiceError> {
    Ok(sha256(&serde_json::to_vec(payload)?))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn open_parent(path: &Path) -> Result<(Dir, std::ffi::OsString), ServiceError> {
    let parent = path.parent().ok_or(ServiceError::ExportRecoveryConflict)?;
    let name = path
        .file_name()
        .ok_or(ServiceError::ExportRecoveryConflict)?
        .to_owned();
    Ok((Dir::open_ambient_dir(parent, ambient_authority())?, name))
}

fn read_regular_from_parent(path: &Path) -> Result<Vec<u8>, ServiceError> {
    let (parent, name) = open_parent(path)?;
    read_regular_at(&parent, Path::new(&name))
}

fn open_relative_parent(
    root: &Dir,
    path: &Path,
) -> Result<(Dir, std::ffi::OsString), ServiceError> {
    use std::path::Component;
    let mut components = path.components().peekable();
    let mut directory = root.try_clone()?;
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(ServiceError::ExportRecoveryConflict);
        };
        if components.peek().is_none() {
            return Ok((directory, name.to_owned()));
        }
        directory = directory.open_dir_nofollow(name)?;
    }
    Err(ServiceError::ExportRecoveryConflict)
}

fn read_regular_at(root: &Dir, path: &Path) -> Result<Vec<u8>, ServiceError> {
    let (parent, name) = open_relative_parent(root, path)?;
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = parent.open_with(&name, &options)?;
    let before = object_identity_from_metadata(&file.metadata()?)?;
    if before.kind != "file" {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if object_identity_from_metadata(&file.metadata()?)? != before {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    Ok(bytes)
}

fn object_identity_from_metadata(
    metadata: &cap_std::fs::Metadata,
) -> Result<ObjectIdentity, ServiceError> {
    let kind = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else {
        return Err(ServiceError::ExportRecoveryConflict);
    };
    Ok(ObjectIdentity {
        platform_id: format!("{}:{}", metadata.dev(), metadata.ino()),
        kind: kind.into(),
    })
}

fn object_identity(path: &Path) -> Result<ObjectIdentity, ServiceError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let kind = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else {
        return Err(ServiceError::ExportRecoveryConflict);
    };
    #[cfg(windows)]
    let platform_id = {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let handle = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
        if unsafe { GetFileInformationByHandle(handle.as_raw_handle().cast(), &mut information) }
            == 0
        {
            return Err(ServiceError::Io(std::io::Error::last_os_error()));
        }
        format!(
            "{}:{}",
            information.dwVolumeSerialNumber,
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow)
        )
    };
    #[cfg(unix)]
    let platform_id = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", metadata.dev(), metadata.ino())
    };
    #[cfg(not(any(unix, windows)))]
    return Err(ServiceError::ExportRecoveryConflict);
    Ok(ObjectIdentity {
        platform_id,
        kind: kind.into(),
    })
}

fn record_tree(root: &Path) -> Result<Vec<RecordedObject>, ServiceError> {
    let mut objects = Vec::new();
    let mut pending = vec![(
        Dir::open_ambient_dir(root, ambient_authority())?,
        String::new(),
    )];
    while let Some((directory, prefix)) = pending.pop() {
        for entry in directory.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(ServiceError::ExportRecoveryConflict)?;
            if name.contains('/') || name.contains('\\') || matches!(name, "." | "..") {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            let relative_path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(ServiceError::ExportRecoveryConflict);
            }
            let (identity, content_hash) = if file_type.is_dir() {
                let child = entry.open_dir()?;
                let identity = object_identity_from_metadata(&child.metadata(".")?)?;
                pending.push((child, relative_path.clone()));
                (identity, None)
            } else if file_type.is_file() {
                let mut options = CapOpenOptions::new();
                options.read(true).follow(FollowSymlinks::No);
                let mut file = entry.open_with(&options)?;
                let before = object_identity_from_metadata(&file.metadata()?)?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                if object_identity_from_metadata(&file.metadata()?)? != before {
                    return Err(ServiceError::ExportRecoveryConflict);
                }
                (before, Some(sha256(&bytes)))
            } else {
                return Err(ServiceError::ExportRecoveryConflict);
            };
            objects.push(RecordedObject {
                relative_path,
                content_hash,
                identity,
            });
        }
    }
    objects.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(objects)
}

fn remove_recorded_directory(
    path: &Path,
    expected: &ObjectIdentity,
    objects: &[RecordedObject],
) -> Result<(), ServiceError> {
    let directory = Dir::open_ambient_dir(path, ambient_authority())?;
    if object_identity_from_metadata(&directory.metadata(".")?)? != *expected
        || expected.kind != "directory"
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let actual = record_tree(path)?;
    if actual != objects {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let mut leaf_first = objects.to_vec();
    leaf_first.sort_by_key(|object| std::cmp::Reverse(object.relative_path.matches('/').count()));
    for object in leaf_first {
        let (parent, name) = open_relative_parent(&directory, Path::new(&object.relative_path))?;
        if object_identity_from_metadata(&parent.symlink_metadata(&name)?)? != object.identity {
            return Err(ServiceError::ExportRecoveryConflict);
        }
        if object.identity.kind == "directory" {
            parent.remove_dir(&name)?;
        } else {
            parent.remove_file(&name)?;
        }
    }
    drop(directory);
    let (parent, name) = open_parent(path)?;
    if object_identity_from_metadata(&parent.symlink_metadata(&name)?)? != *expected {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    parent.remove_dir(&name)?;
    Ok(())
}

fn remove_recorded_file(path: &Path, expected: &ObjectIdentity) -> Result<(), ServiceError> {
    let (parent, name) = open_parent(path)?;
    if object_identity_from_metadata(&parent.symlink_metadata(&name)?)? != *expected
        || expected.kind != "file"
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    parent.remove_file(&name)?;
    Ok(())
}

fn verify_directory_tree(root: &Path, expected: &[RecordedObject]) -> Result<(), ServiceError> {
    if record_tree(root)? == expected {
        Ok(())
    } else {
        Err(ServiceError::ExportRecoveryConflict)
    }
}

fn restore_previous_destination(journal: &ExportJournal) -> Result<(), ServiceError> {
    let expected = journal
        .backup_identity
        .as_ref()
        .or(journal.previous_destination_identity.as_ref())
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    if journal.destination.exists()
        || object_identity(&journal.backup).ok().as_ref() != Some(expected)
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    verify_directory_tree(&journal.backup, &journal.previous_destination_objects)?;
    let backup_parent = journal
        .backup
        .parent()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    let destination_parent = journal
        .destination
        .parent()
        .ok_or(ServiceError::ExportRecoveryConflict)?;
    if backup_parent != destination_parent {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    let parent = Dir::open_ambient_dir(backup_parent, ambient_authority())?;
    parent.rename(
        journal
            .backup
            .file_name()
            .ok_or(ServiceError::ExportRecoveryConflict)?,
        &parent,
        journal
            .destination
            .file_name()
            .ok_or(ServiceError::ExportRecoveryConflict)?,
    )?;
    if object_identity_from_metadata(
        &parent.symlink_metadata(
            journal
                .destination
                .file_name()
                .ok_or(ServiceError::ExportRecoveryConflict)?,
        )?,
    )? != *expected
    {
        return Err(ServiceError::ExportRecoveryConflict);
    }
    Ok(())
}

fn discard_staging(journal: &ExportJournal, journal_path: &Path) -> Result<(), ServiceError> {
    verify_directory_tree(&journal.staging, &journal.generated_identities)?;
    remove_recorded_directory(
        &journal.staging,
        &journal.staging_identity,
        &journal.generated_identities,
    )?;
    remove_recorded_file(journal_path, &journal.journal_identity)
}

fn random_id() -> Result<String, ServiceError> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|error| {
        ServiceError::Io(std::io::Error::other(format!(
            "OS random source failed: {error}"
        )))
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn sync_directory(path: &Path) -> Result<(), ServiceError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{
            CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
        };
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FlushFileBuffers, OPEN_EXISTING,
        };
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(ServiceError::Io(std::io::Error::last_os_error()));
        }
        let flushed = unsafe { FlushFileBuffers(handle) };
        let flush_error = if flushed == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        unsafe { CloseHandle(handle) };
        if let Some(error) = flush_error {
            return Err(ServiceError::Io(error));
        }
    }
    #[cfg(not(windows))]
    OpenOptions::new().read(true).open(path)?.sync_all()?;
    Ok(())
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !reserved_export_path(path)
        && Path::new(path)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn reserved_export_path(path: &str) -> bool {
    path.split('/').any(|part| {
        part == ".fleximark-export.json"
            || part.contains(".fleximark-export-staging-")
            || part.contains(".fleximark-export-backup-")
            || part.ends_with(".fleximark-export-journal.json")
    })
}

fn index_for_destination(destination: &Path) -> PathBuf {
    destination.join("index.html")
}

pub fn workspace_path(uri: &str) -> Result<PathBuf, ServiceError> {
    file_uri_path(uri)?.canonicalize().map_err(ServiceError::Io)
}

fn file_uri_path(uri: &str) -> Result<PathBuf, ServiceError> {
    let encoded = uri
        .strip_prefix("file://")
        .ok_or(ServiceError::InvalidWorkspaceUri)?;
    let decoded = percent_decode(encoded)?;
    #[cfg(windows)]
    let decoded = decoded.strip_prefix('/').unwrap_or(&decoded);
    let path = PathBuf::from(decoded);
    if !path.is_absolute() {
        return Err(ServiceError::InvalidWorkspaceUri);
    }
    Ok(path)
}

pub fn document_is_in_workspace(
    document_uri: &str,
    workspace_uri: &str,
) -> Result<bool, ServiceError> {
    let document = workspace_path(document_uri)?;
    let workspace = workspace_path(workspace_uri)?;
    Ok(document != workspace && document.starts_with(workspace))
}

pub fn workspace_for_document(document_uri: &str) -> Result<String, ServiceError> {
    let document = workspace_path(document_uri)?;
    for parent in document
        .parent()
        .ok_or(ServiceError::InvalidWorkspaceUri)?
        .ancestors()
    {
        if parent.join(".fleximark/config.toml").is_file() {
            validate_config(&parent.join(".fleximark/config.toml"))?;
            return path_to_file_uri(parent);
        }
    }
    Err(ServiceError::NotInitialized)
}

pub fn load_plugin_host(
    workspace_uri: &str,
    trusted: bool,
    generation: u64,
) -> Result<(PluginHost, RenderConfig), ServiceError> {
    let workspace = workspace_path(workspace_uri)?;
    let control = open_control_directory(&workspace, false)?;
    let config_source = if let Some(control) = &control {
        match open_regular_nofollow(control, Path::new("config.toml")) {
            Ok(mut file) => {
                let mut text = String::new();
                file.read_to_string(&mut text)?;
                text
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => CONFIG.to_owned(),
            Err(error) => return Err(error.into()),
        }
    } else {
        CONFIG.to_owned()
    };
    let config =
        FlexiMarkConfig::from_toml(&config_source).map_err(|_| ServiceError::InvalidConfig)?;
    let config_hash = sha256(config_source.as_bytes());
    let mut host = PluginHost::configured(
        ExecutionLimits::default(),
        HostPolicy {
            workspace_trusted: trusted,
            workspace_root: workspace.to_string_lossy().into_owned(),
        },
        config_hash.clone(),
        generation,
    )
    .map_err(|error| ServiceError::PluginConfig(error.to_string()))?;
    if !trusted || config.plugins.iter().all(|plugin| !plugin.enabled) {
        let render_config = render_config(&workspace, &config, &host, trusted)?;
        return Ok((host, render_config));
    }
    let plugin_root = workspace.join(".fleximark/plugins");
    reject_linked_path(&workspace, Path::new(".fleximark/plugins"), true)?;
    let canonical_plugin_root = plugin_root.canonicalize().map_err(|error| {
        ServiceError::PluginConfig(format!("plugin directory is unavailable: {error}"))
    })?;
    if !canonical_plugin_root.starts_with(&workspace) {
        return Err(ServiceError::PluginConfig(
            "plugin directory escapes the workspace".to_owned(),
        ));
    }
    let directory = Dir::open_ambient_dir(&canonical_plugin_root, ambient_authority())?;
    for (config_order, plugin) in config
        .plugins
        .iter()
        .filter(|plugin| plugin.enabled)
        .enumerate()
    {
        let manifest = read_plugin_file(&directory, &canonical_plugin_root, &plugin.manifest)?;
        let wasm = read_plugin_file(&directory, &canonical_plugin_root, &plugin.wasm)?;
        let signature = read_plugin_file(&directory, &canonical_plugin_root, &plugin.signature)?;
        host.register_verified(VerifiedPluginPackage {
            configured_id: &plugin.id,
            config_order,
            manifest_bytes: &manifest,
            expected_manifest_sha256: &plugin.manifest_sha256,
            wasm_bytes: &wasm,
            signature_bytes: &signature,
            signer_public_key: &plugin.signer_public_key,
            grants: plugin.grants.clone(),
            environment: plugin.environment.clone(),
        })
        .map_err(|error| ServiceError::PluginConfig(error.to_string()))?;
    }
    let render_config = render_config(&workspace, &config, &host, trusted)?;
    Ok((host, render_config))
}

fn render_config(
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

fn read_plugin_file(directory: &Dir, root: &Path, relative: &str) -> Result<Vec<u8>, ServiceError> {
    let relative = Path::new(relative);
    reject_linked_path(root, relative, false)?;
    let absolute = root.join(relative).canonicalize()?;
    if !absolute.starts_with(root) {
        return Err(ServiceError::PluginConfig(
            "plugin package path escapes .fleximark/plugins".to_owned(),
        ));
    }
    let metadata = fs::symlink_metadata(&absolute)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ServiceError::PluginConfig(
            "plugin package entry is not a regular file".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    directory.open(relative)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn reject_linked_path(
    root: &Path,
    relative: &Path,
    final_directory: bool,
) -> Result<(), ServiceError> {
    let mut current = root.to_path_buf();
    for (index, component) in relative.components().enumerate() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink()
            || (index + 1 == relative.components().count() && final_directory && !metadata.is_dir())
        {
            return Err(ServiceError::PluginConfig(
                "plugin package contains a linked or invalid path component".to_owned(),
            ));
        }
    }
    Ok(())
}

pub fn path_to_file_uri(path: &Path) -> Result<String, ServiceError> {
    let path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or(ServiceError::InvalidWorkspaceUri)?
                .canonicalize()?;
            parent.join(path.file_name().ok_or(ServiceError::InvalidWorkspaceUri)?)
        }
        Err(error) => return Err(error.into()),
    };
    path_to_file_uri_unchecked(&path)
}

fn path_to_file_uri_unchecked(path: &Path) -> Result<String, ServiceError> {
    if !path.is_absolute() {
        return Err(ServiceError::InvalidWorkspaceUri);
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let normalized = normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_owned();
    let encoded = normalized
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect::<String>();
    Ok(if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    })
}

fn percent_decode(value: &str) -> Result<String, ServiceError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .ok_or(ServiceError::InvalidWorkspaceUri)?;
            let text = std::str::from_utf8(hex).map_err(|_| ServiceError::InvalidWorkspaceUri)?;
            let byte =
                u8::from_str_radix(text, 16).map_err(|_| ServiceError::InvalidWorkspaceUri)?;
            if byte == 0 {
                return Err(ServiceError::InvalidWorkspaceUri);
            }
            output.push(byte);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| ServiceError::InvalidWorkspaceUri)
}

fn open_control_directory(root: &Path, create: bool) -> Result<Option<Dir>, ServiceError> {
    let workspace = Dir::open_ambient_dir(root, ambient_authority())?;
    match workspace.open_dir_nofollow(".fleximark") {
        Ok(control) => Ok(Some(control)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            workspace.create_dir(".fleximark")?;
            workspace
                .open_dir_nofollow(".fleximark")
                .map(Some)
                .map_err(ServiceError::Io)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ServiceError::LinkedControlDirectory),
    }
}

fn write_new_at(directory: &Dir, path: &str, contents: &str) -> Result<(), ServiceError> {
    let mut options = cap_std::fs::OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    match directory.open_with(path, &options) {
        Ok(mut file) => {
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = directory.symlink_metadata(path)?;
            if metadata.is_file() && !metadata.file_type().is_symlink() {
                Ok(())
            } else {
                Err(ServiceError::InvalidControlPath)
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn open_regular_nofollow(directory: &Dir, path: &Path) -> std::io::Result<cap_std::fs::File> {
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = directory.open_with(path, &options)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn reject_link(path: &Path) -> Result<(), ServiceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(ServiceError::InvalidControlPath)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn validate_config(path: &Path) -> Result<FlexiMarkConfig, ServiceError> {
    let control_path = path.parent().ok_or(ServiceError::InvalidConfig)?;
    let root = control_path.parent().ok_or(ServiceError::InvalidConfig)?;
    let control = open_control_directory(root, false)?.ok_or(ServiceError::NotInitialized)?;
    let mut file = open_regular_nofollow(&control, Path::new("config.toml")).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ServiceError::NotInitialized
        } else {
            ServiceError::InvalidConfig
        }
    })?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|_| ServiceError::InvalidConfig)?;
    FlexiMarkConfig::from_toml(&text).map_err(|_| ServiceError::InvalidConfig)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_workspace(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "fleximark-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn rejects_non_file_workspace_uris() {
        assert!(matches!(
            workspace_path("https://example.test/x"),
            Err(ServiceError::InvalidWorkspaceUri)
        ));
    }

    #[test]
    fn initializes_new_workspace_layout() {
        let root = test_workspace("service-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        assert!(root.join(".fleximark/config.toml").is_file());
        assert!(root.join(".fleximark/theme.css").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linked_control_directory_never_reads_or_writes_outside_the_workspace() {
        let root = test_workspace("linked-control-test");
        let outside = test_workspace("linked-control-outside");
        fs::write(outside.join("sentinel"), "unchanged").unwrap();
        fs::write(outside.join("config.toml"), CONFIG).unwrap();
        let linked = root.join(".fleximark");
        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &linked);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_dir(&outside, &linked);
        if link_result.is_err() {
            fs::remove_dir_all(root).unwrap();
            fs::remove_dir_all(outside).unwrap();
            return;
        }
        let uri = path_to_file_uri(&root).unwrap();
        assert!(matches!(
            initialize_workspace(&uri),
            Err(ServiceError::LinkedControlDirectory)
        ));
        assert!(matches!(
            get_note_options(&uri),
            Err(ServiceError::LinkedControlDirectory)
        ));
        assert!(matches!(
            load_plugin_host(&uri, false, 1),
            Err(ServiceError::LinkedControlDirectory)
        ));
        assert_eq!(
            fs::read_to_string(outside.join("sentinel")).unwrap(),
            "unchanged"
        );
        assert!(!outside.join("theme.css").exists());
        fs::remove_file(&linked).unwrap_or_else(|_| fs::remove_dir(&linked).unwrap());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn old_workspace_layout_is_unsupported_and_never_loaded() {
        let root = test_workspace("old-layout-unsupported");
        let control = root.join(".fleximark");
        fs::create_dir(&control).unwrap();
        fs::write(control.join("fleximark.json"), "not valid json").unwrap();
        fs::write(
            control.join("parserPlugin.js"),
            "throw new Error('executed')",
        )
        .unwrap();
        fs::write(
            control.join("fleximark.css"),
            "@import 'https://example.test'",
        )
        .unwrap();
        let uri = path_to_file_uri(&root).unwrap();
        let (_, config) = load_plugin_host(&uri, true, 1).unwrap();
        assert!(config.style.is_none());
        initialize_workspace(&uri).unwrap();
        assert!(control.join("config.toml").is_file());
        assert!(control.join("theme.css").is_file());
        assert_eq!(
            fs::read_to_string(control.join("parserPlugin.js")).unwrap(),
            "throw new Error('executed')"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canonical_config_parser_rejects_unknown_fields() {
        let root = test_workspace("config-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            "schema_version = 1\nunknown = true\n",
        )
        .unwrap();
        assert!(matches!(
            create_note(&uri),
            Err(ServiceError::InvalidConfig)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn edit_theme_opens_canonical_theme() {
        let root = test_workspace("edit-theme-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        let result = edit_theme(&uri).unwrap();
        assert_eq!(
            workspace_path(result.open_uri.as_deref().unwrap()).unwrap(),
            root.join(".fleximark/theme.css").canonicalize().unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn create_note_applies_category_template_filename_date_and_snippets() {
        let root = test_workspace("note-options-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            r##"schema_version = 1

[notes]
file_name_prefix = "${CURRENT_YEAR}-"
file_name_suffix = "-draft"

[notes.categories]
reports = "work/reports"

[notes.templates]
daily = ["# ${1:Title}", "Created ${CURRENT_YEAR}-${CURRENT_MONTH}-${CURRENT_DATE}", "$0"]
"##,
        )
        .unwrap();
        let options = get_note_options(&uri).unwrap();
        assert_eq!(options.categories, ["reports"]);
        assert_eq!(options.templates, ["daily"]);
        let result = create_note_with_options(&uri, Some("reports"), Some("daily")).unwrap();
        let note = workspace_path(result.open_uri.as_deref().unwrap()).unwrap();
        assert_eq!(
            note.parent().unwrap(),
            root.join("notes/work/reports").canonicalize().unwrap()
        );
        let name = note.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with("-draft.md"));
        let contents = fs::read_to_string(note).unwrap();
        assert!(contents.starts_with("# Title\nCreated "));
        assert!(!contents.contains("${") && !contents.contains("$0"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn collect_admonitions_writes_only_typed_admonitions() {
        let root = test_workspace("collect-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        let source = "# Keep out\n\n:::info\nkeep in\n:::\n\nTrailing\n";
        let session = fleximark_engine::DocumentSession::open(
            fleximark_model::DocumentUri(format!("{uri}/source.md")),
            1,
            source.to_owned(),
            fleximark_model::PositionEncoding::Utf8,
        )
        .unwrap();
        let result = collect_admonitions(session.document(), source, &uri).unwrap();
        assert_eq!(
            result.message.as_ref().unwrap().text,
            "Collected 1 admonition(s)"
        );
        let output =
            fs::read_to_string(workspace_path(result.open_uri.as_deref().unwrap()).unwrap())
                .unwrap();
        assert!(output.contains(":::info\nkeep in\n:::"));
        assert!(!output.contains("# Keep out") && !output.contains("Trailing"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_loader_reads_verified_package_files() {
        let root = test_workspace("plugin-loader-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        let plugin_root = root.join(".fleximark/plugins");
        fs::create_dir(&plugin_root).unwrap();
        let wasm = b"\0asm\x01\0\0\0";
        fs::write(plugin_root.join("sample.wasm"), wasm).unwrap();
        let manifest = format!(
            "schema_version = 1\n[plugin]\nid = \"sample.plugin\"\napi_version = 1\nrequired = false\n[artifact]\nwasm_sha256 = \"{}\"\n[capabilities]\n",
            sha256(wasm)
        );
        fs::write(plugin_root.join("sample.toml"), &manifest).unwrap();
        fs::write(plugin_root.join("sample.sig"), [0_u8; 64]).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            format!(
                "schema_version = 1\n[[plugins]]\nid = \"sample.plugin\"\nwasm = \"sample.wasm\"\nmanifest = \"sample.toml\"\nsignature = \"sample.sig\"\nmanifest_sha256 = \"{}\"\nsigner_public_key = \"{}\"\n",
                sha256(manifest.as_bytes()),
                "00".repeat(32)
            ),
        )
        .unwrap();
        assert!(matches!(
            load_plugin_host(&uri, true, 1),
            Err(ServiceError::PluginConfig(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

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
        assert!(html.contains("fleximark-publication"));
        assert!(html.contains("document-root") && html.contains("math-1"));
        assert!(html.contains(style.fingerprint()));
        assert!(html.contains(":root { color: red; }"));
        assert!(html.contains("default-src 'none'"));
    }

    #[test]
    fn export_requires_ownership_and_preserves_user_files() {
        let root = test_workspace("export-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# hello\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("user.txt"), "keep").unwrap();
        let destination_uri = path_to_file_uri(&destination).unwrap();
        assert!(matches!(
            export_html(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                "<p>first</p>",
                &[]
            ),
            Err(ServiceError::UnmanagedExport)
        ));
        fs::remove_dir(&destination).unwrap_err();
        fs::remove_file(destination.join("user.txt")).unwrap();
        fs::remove_dir(&destination).unwrap();

        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>first</p>",
            &[],
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        fs::write(destination.join("user.txt"), "keep").unwrap();
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>second</p>",
            &[],
        )
        .unwrap();
        let journal = root.join(".public.fleximark-export-journal.json");
        assert!(
            journal.exists(),
            "committed journal is retained for verification"
        );
        assert!(fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".public.fleximark-export-backup-")
        }));
        assert!(matches!(
            preflight_export(&source_uri, &workspace_uri, &destination_uri),
            Err(ServiceError::ExportAwaitingAcknowledgement)
        ));
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        assert!(!journal.exists());
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".public.fleximark-export-backup-")
        }));
        assert_eq!(
            fs::read_to_string(destination.join("user.txt")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(destination.join("index.html")).unwrap(),
            "<p>second</p>"
        );
        let marker: OwnershipMarker =
            read_json_regular(&destination.join(".fleximark-export.json")).unwrap();
        assert_eq!(marker.payload.generation, 2);

        fs::write(destination.join("index.html"), "tampered").unwrap();
        assert!(matches!(
            export_html(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                "<p>third</p>",
                &[]
            ),
            Err(ServiceError::ExportContentConflict)
        ));
        assert_eq!(
            fs::read_to_string(destination.join("index.html")).unwrap(),
            "tampered"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_ownership_ids_are_random_and_cannot_be_copied_to_another_destination() {
        let root = test_workspace("export-random-ownership-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# export\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let first = root.join("first");
        let second = root.join("second");
        let first_uri = path_to_file_uri_unchecked(&first).unwrap();
        let second_uri = path_to_file_uri_unchecked(&second).unwrap();

        export_html(&source_uri, &workspace_uri, &first_uri, "<p>first</p>", &[]).unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &first_uri).unwrap();
        export_html(
            &source_uri,
            &workspace_uri,
            &second_uri,
            "<p>second</p>",
            &[],
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &second_uri).unwrap();
        let first_marker: OwnershipMarker =
            read_json_regular(&first.join(".fleximark-export.json")).unwrap();
        let second_marker: OwnershipMarker =
            read_json_regular(&second.join(".fleximark-export.json")).unwrap();
        assert_ne!(
            first_marker.payload.destination_id,
            second_marker.payload.destination_id
        );
        assert_eq!(first_marker.payload.destination_id.len(), 32);

        fs::write(
            second.join(".fleximark-export.json"),
            serde_json::to_vec(&first_marker).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            preflight_export(&source_uri, &workspace_uri, &second_uri),
            Err(ServiceError::ExportOwnershipConflict) | Err(ServiceError::ExportContentConflict)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_export_output_is_marked_in_html_marker_and_registry() {
        let root = test_workspace("unsafe-export-marker");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("note.md");
        fs::write(&source, "# Note\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        export_html_with_safety(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "PLUGIN OUTPUT WITHOUT AN HTML HEAD",
            &[],
            true,
        )
        .unwrap();
        let html = fs::read_to_string(destination.join("index.html")).unwrap();
        assert!(html.starts_with("<!-- fleximark-output-safety: unsafe-plugin-html -->"));
        assert!(html.contains("role=\"alert\""));
        let marker: OwnershipMarker =
            read_json_regular(&destination.join(".fleximark-export.json")).unwrap();
        assert!(marker.payload.unsafe_output_used);
        let registry_path = fs::read_dir(root.join(".fleximark/export-targets"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let registry: RegistryRecord = read_json_regular(&registry_path).unwrap();
        assert!(registry.unsafe_output_used);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn acknowledged_export_rejects_a_swapped_destination_directory() {
        let root = test_workspace("export-directory-swap-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# export\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>original</p>",
            &[],
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();

        let original = root.join("original-public");
        fs::rename(&destination, &original).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::copy(original.join("index.html"), destination.join("index.html")).unwrap();
        fs::copy(
            original.join(".fleximark-export.json"),
            destination.join(".fleximark-export.json"),
        )
        .unwrap();
        assert!(matches!(
            preflight_export(&source_uri, &workspace_uri, &destination_uri),
            Err(ServiceError::ExportOwnershipConflict)
        ));
        assert_eq!(
            fs::read_to_string(original.join("index.html")).unwrap(),
            "<p>original</p>"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_resolves_and_manifests_only_contained_assets() {
        let root = test_workspace("export-assets-test");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        let asset = root.join("image.png");
        fs::write(&source, "![image](image.png)\n").unwrap();
        let png = b"\x89PNG\r\n\x1a\nasset";
        fs::write(&asset, png).unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let render_asset =
            ResolvedRenderAsset::from_validated_bytes("image.png".into(), "image/png".into(), png)
                .unwrap()
                .published()
                .clone();
        let resolved = resolve_export_assets(
            &source_uri,
            &workspace_uri,
            &format!("<img src=\"{}\">", render_asset.reference),
            &[render_asset],
        )
        .unwrap();
        assert_eq!(resolved.assets.len(), 1);
        assert!(resolved.html.contains("src=\"assets/"));
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        preflight_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            &resolved.html,
            &resolved.assets,
        )
        .unwrap();
        let marker: OwnershipMarker =
            read_json_regular(&destination.join(".fleximark-export.json")).unwrap();
        assert!(
            marker
                .payload
                .files
                .iter()
                .any(|file| file.path == "assets")
        );
        let exported_asset = marker
            .payload
            .files
            .iter()
            .find(|file| file.path.starts_with("assets/") && file.kind == "file")
            .unwrap();
        assert_eq!(
            fs::read(destination.join(&exported_asset.path)).unwrap(),
            png
        );
        fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn old_moved_journal_recovers_the_previous_destination() {
        let root = test_workspace("recovery-test");
        let destination = root.join("public");
        let staging = root.join(".public.fleximark-export-staging-test");
        let backup = root.join(".public.fleximark-export-backup-test");
        let journal_path = root.join(".public.fleximark-export-journal.json");
        fs::create_dir(&staging).unwrap();
        fs::create_dir(&backup).unwrap();
        fs::write(backup.join("old.txt"), "old").unwrap();
        let record = RegistryRecord {
            destination_id: "id".into(),
            generation: 2,
            digest: "digest".into(),
            source_identity: "source".into(),
            workspace_identity: "workspace".into(),
            destination_identity: destination.to_string_lossy().into_owned(),
            destination_object_identity: object_identity(&staging).unwrap(),
            unsafe_output_used: false,
        };
        let staging_identity = object_identity(&staging).unwrap();
        let backup_identity = object_identity(&backup).unwrap();
        let journal_identity = create_journal(&journal_path).unwrap();
        let mut journal = ExportJournal {
            transaction_id: "test".into(),
            state: ExportJournalState::Prepared,
            destination: destination.clone(),
            staging: staging.clone(),
            backup: backup.clone(),
            registry: root.join("registry.json"),
            journal_identity,
            registry_staging: None,
            registry_staging_identity: None,
            previous_registry: None,
            next_registry: record,
            staging_identity,
            staging_marker_identity: ObjectIdentity {
                platform_id: "unused".into(),
                kind: "file".into(),
            },
            generated_identities: Vec::new(),
            previous_destination_objects: vec![RecordedObject {
                relative_path: "old.txt".into(),
                identity: object_identity(&backup.join("old.txt")).unwrap(),
                content_hash: Some(sha256(b"old")),
            }],
            previous_destination_identity: Some(backup_identity.clone()),
            previous_registry_identity: None,
            backup_identity: None,
            installed_identity: None,
            installed_marker_identity: None,
            next_registry_identity: None,
        };
        let digest = append_journal(&journal_path, &journal, None).unwrap();
        journal.state = ExportJournalState::OldMoveIntent;
        let digest = append_journal(&journal_path, &journal, Some(digest)).unwrap();
        journal.state = ExportJournalState::OldMoved;
        journal.backup_identity = Some(backup_identity);
        append_journal(&journal_path, &journal, Some(digest)).unwrap();
        recover_export(
            &journal_path,
            &destination,
            &root.join("registry.json"),
            false,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(destination.join("old.txt")).unwrap(),
            "old"
        );
        assert!(!staging.exists() && !journal_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn journal_records_are_append_only_and_digest_chained() {
        let root = test_workspace("journal-digest-test");
        let path = root.join("journal.json");
        let registry = RegistryRecord {
            destination_id: "id".into(),
            generation: 1,
            digest: "digest".into(),
            source_identity: "source".into(),
            workspace_identity: "workspace".into(),
            destination_identity: "destination".into(),
            destination_object_identity: ObjectIdentity {
                platform_id: "destination-object".into(),
                kind: "directory".into(),
            },
            unsafe_output_used: false,
        };
        let journal_identity = create_journal(&path).unwrap();
        let mut journal = ExportJournal {
            transaction_id: "transaction".into(),
            state: ExportJournalState::Prepared,
            destination: root.join("output"),
            staging: root.join("staging"),
            backup: root.join("backup"),
            registry: root.join("registry"),
            journal_identity,
            registry_staging: None,
            registry_staging_identity: None,
            previous_registry: None,
            next_registry: registry,
            staging_identity: ObjectIdentity {
                platform_id: "staging".into(),
                kind: "directory".into(),
            },
            staging_marker_identity: ObjectIdentity {
                platform_id: "marker".into(),
                kind: "file".into(),
            },
            generated_identities: Vec::new(),
            previous_destination_objects: Vec::new(),
            previous_destination_identity: None,
            previous_registry_identity: None,
            backup_identity: None,
            installed_identity: None,
            installed_marker_identity: None,
            next_registry_identity: None,
        };
        let first = append_journal(&path, &journal, None).unwrap();
        journal.state = ExportJournalState::OldMoveIntent;
        append_journal(&path, &journal, Some(first)).unwrap();
        assert_eq!(
            read_journal(&path).unwrap().0.state,
            ExportJournalState::OldMoveIntent
        );
        let tampered = fs::read_to_string(&path)
            .unwrap()
            .replacen("transaction", "forged", 1);
        fs::write(&path, tampered).unwrap();
        assert!(matches!(
            read_journal(&path),
            Err(ServiceError::ExportRecoveryConflict)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_refuses_swapped_staging_identity_without_deleting_it() {
        let root = test_workspace("journal-swap-test");
        let staging = root.join(".output.fleximark-export-staging-test");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join(".fleximark-export.json"), "original").unwrap();
        let journal_path = root.join(".output.fleximark-export-journal.json");
        let journal_identity = create_journal(&journal_path).unwrap();
        let journal = ExportJournal {
            transaction_id: "transaction".into(),
            state: ExportJournalState::Prepared,
            destination: root.join("output"),
            staging: staging.clone(),
            backup: root.join(".output.fleximark-export-backup-test"),
            registry: root.join("registry.json"),
            journal_identity,
            registry_staging: None,
            registry_staging_identity: None,
            previous_registry: None,
            next_registry: RegistryRecord {
                destination_id: "id".into(),
                generation: 1,
                digest: "digest".into(),
                source_identity: "source".into(),
                workspace_identity: "workspace".into(),
                destination_identity: "destination".into(),
                destination_object_identity: ObjectIdentity {
                    platform_id: "destination-object".into(),
                    kind: "directory".into(),
                },
                unsafe_output_used: false,
            },
            staging_identity: object_identity(&staging).unwrap(),
            staging_marker_identity: object_identity(&staging.join(".fleximark-export.json"))
                .unwrap(),
            generated_identities: Vec::new(),
            previous_destination_objects: Vec::new(),
            previous_destination_identity: None,
            previous_registry_identity: None,
            backup_identity: None,
            installed_identity: None,
            installed_marker_identity: None,
            next_registry_identity: None,
        };
        append_journal(&journal_path, &journal, None).unwrap();
        fs::remove_dir_all(&staging).unwrap();
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join(".fleximark-export.json"), "replacement").unwrap();
        assert!(matches!(
            recover_export(
                &journal_path,
                &root.join("output"),
                &root.join("registry.json"),
                false
            ),
            Err(ServiceError::ExportRecoveryConflict)
        ));
        assert_eq!(
            fs::read_to_string(staging.join(".fleximark-export.json")).unwrap(),
            "replacement"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_refuses_replaced_journal_identity_without_touching_staging() {
        let root = test_workspace("journal-identity-test");
        let staging = root.join(".output.fleximark-export-staging-test");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join(".fleximark-export.json"), "original").unwrap();
        let journal_path = root.join(".output.fleximark-export-journal.json");
        let journal_identity = create_journal(&journal_path).unwrap();
        let journal = ExportJournal {
            transaction_id: "transaction".into(),
            state: ExportJournalState::Prepared,
            destination: root.join("output"),
            staging: staging.clone(),
            backup: root.join(".output.fleximark-export-backup-test"),
            registry: root.join("registry.json"),
            journal_identity,
            registry_staging: None,
            registry_staging_identity: None,
            previous_registry: None,
            next_registry: RegistryRecord {
                destination_id: "id".into(),
                generation: 1,
                digest: "digest".into(),
                source_identity: "source".into(),
                workspace_identity: "workspace".into(),
                destination_identity: "destination".into(),
                destination_object_identity: ObjectIdentity {
                    platform_id: "destination-object".into(),
                    kind: "directory".into(),
                },
                unsafe_output_used: false,
            },
            staging_identity: object_identity(&staging).unwrap(),
            staging_marker_identity: object_identity(&staging.join(".fleximark-export.json"))
                .unwrap(),
            generated_identities: Vec::new(),
            previous_destination_objects: Vec::new(),
            previous_destination_identity: None,
            previous_registry_identity: None,
            backup_identity: None,
            installed_identity: None,
            installed_marker_identity: None,
            next_registry_identity: None,
        };
        append_journal(&journal_path, &journal, None).unwrap();
        let records = fs::read(&journal_path).unwrap();
        let replacement = root.join("replacement-journal.json");
        fs::write(&replacement, records).unwrap();
        fs::remove_file(&journal_path).unwrap();
        fs::rename(replacement, &journal_path).unwrap();
        assert!(matches!(
            recover_export(
                &journal_path,
                &root.join("output"),
                &root.join("registry.json"),
                false
            ),
            Err(ServiceError::ExportRecoveryConflict)
        ));
        assert_eq!(
            fs::read_to_string(staging.join(".fleximark-export.json")).unwrap(),
            "original"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_export_intent_recovers_idempotently_after_an_injected_crash() {
        for fault in [
            "old-move-intent",
            "old-moved",
            "install-intent",
            "installed",
            "registry-intent",
            "registry-replaced",
            "committed",
        ] {
            let root = test_workspace(&format!("export-crash-{fault}"));
            let workspace_uri = path_to_file_uri(&root).unwrap();
            initialize_workspace(&workspace_uri).unwrap();
            let source = root.join("doc.md");
            fs::write(&source, "# export\n").unwrap();
            let source_uri = path_to_file_uri(&source).unwrap();
            let destination = root.join("public");
            let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
            export_html(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                "<p>old</p>",
                &[],
            )
            .unwrap();
            acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();

            let result = export_html_transaction(
                &source_uri,
                &workspace_uri,
                &destination_uri,
                "<p>new</p>",
                &[],
                false,
                |checkpoint| {
                    if checkpoint == fault {
                        Err(ServiceError::Io(std::io::Error::other("injected crash")))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err(), "fault {fault} was not reached");

            let preflight = preflight_export(&source_uri, &workspace_uri, &destination_uri);
            if matches!(fault, "old-move-intent" | "old-moved" | "install-intent") {
                preflight.unwrap();
                assert_eq!(
                    fs::read_to_string(destination.join("index.html")).unwrap(),
                    "<p>old</p>"
                );
            } else {
                assert!(matches!(
                    preflight,
                    Err(ServiceError::ExportAwaitingAcknowledgement)
                ));
                assert_eq!(
                    fs::read_to_string(destination.join("index.html")).unwrap(),
                    "<p>new</p>"
                );
                acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
            }
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn acknowledgement_refuses_unrecorded_backup_entries_without_deleting_them() {
        let root = test_workspace("export-backup-injection");
        let workspace_uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&workspace_uri).unwrap();
        let source = root.join("doc.md");
        fs::write(&source, "# export\n").unwrap();
        let source_uri = path_to_file_uri(&source).unwrap();
        let destination = root.join("public");
        let destination_uri = path_to_file_uri_unchecked(&destination).unwrap();
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>old</p>",
            &[],
        )
        .unwrap();
        acknowledge_export(&source_uri, &workspace_uri, &destination_uri).unwrap();
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>new</p>",
            &[],
        )
        .unwrap();
        let backup = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(".public.fleximark-export-backup-"))
            })
            .unwrap();
        fs::write(backup.join("unrecorded.txt"), "do not delete").unwrap();
        assert!(matches!(
            acknowledge_export(&source_uri, &workspace_uri, &destination_uri),
            Err(ServiceError::ExportRecoveryConflict)
        ));
        assert_eq!(
            fs::read_to_string(backup.join("unrecorded.txt")).unwrap(),
            "do not delete"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
