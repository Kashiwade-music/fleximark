use std::collections::BTreeSet;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use fleximark_engine::{RenderAsset, RenderConfig, RenderStyle, ResolvedRenderAsset};
use fleximark_model::Document;
use fleximark_plugin_host::PluginHost;
use fleximark_protocol::{CommandResult, GetNoteOptionsResult};
use fleximark_render_html::RenderContext;
use fleximark_service::{
    ExportAsset, ResolvedExportAssets, ServiceError, acknowledge_export, collect_admonitions,
    compose_portable_html, create_note, create_note_with_options, default_export_destination,
    document_is_in_workspace, edit_theme, export_html, export_html_with_safety,
    export_render_context, get_note_options, initialize_workspace, load_plugin_host,
    path_to_file_uri, preflight_export, resolve_export_assets, resolve_render_assets,
    workspace_for_document, workspace_path,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const DEFAULT_CONFIG: &str = "# FlexiMark workspace configuration\nschema_version = 1\n\n[security]\nraw_html_preview = \"escape\"\nraw_html_export = \"reject\"\n";
const DEFAULT_THEME: &str =
    "/* FlexiMark workspace theme */\n:root { color-scheme: light dark; }\n";

type ServiceResult<T> = Result<T, ServiceError>;
type CreateNoteWithOptionsFn = fn(&str, Option<&str>, Option<&str>) -> ServiceResult<CommandResult>;
type ExportFn = fn(&str, &str, &str, &str, &[ExportAsset]) -> ServiceResult<CommandResult>;
type ExportWithSafetyFn =
    fn(&str, &str, &str, &str, &[ExportAsset], bool) -> ServiceResult<CommandResult>;
type LoadPluginHostFn = fn(&str, bool, u64) -> ServiceResult<(PluginHost, RenderConfig)>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContractExportFile {
    path: String,
    kind: String,
    content_hash: String,
    object_identity: ContractObjectIdentity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContractOwnershipPayload {
    format_version: u32,
    destination_id: String,
    generation: u64,
    source_identity: String,
    workspace_identity: String,
    destination_identity: String,
    destination_object_identity: ContractObjectIdentity,
    unsafe_output_used: bool,
    files: Vec<ContractExportFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ContractOwnershipMarker {
    payload: ContractOwnershipPayload,
    digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ContractRegistryRecord {
    destination_id: String,
    generation: u64,
    digest: String,
    source_identity: String,
    workspace_identity: String,
    destination_identity: String,
    destination_object_identity: ContractObjectIdentity,
    unsafe_output_used: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
enum ContractExportJournalState {
    Prepared,
    OldMoveIntent,
    OldMoved,
    InstallIntent,
    NewInstalled,
    RegistryIntent,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ContractObjectIdentity {
    platform_id: String,
    kind: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ContractRecordedObject {
    relative_path: String,
    identity: ContractObjectIdentity,
    content_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ContractExportJournal {
    transaction_id: String,
    state: ContractExportJournalState,
    destination: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    registry: PathBuf,
    journal_identity: ContractObjectIdentity,
    registry_staging: Option<PathBuf>,
    registry_staging_identity: Option<ContractObjectIdentity>,
    previous_registry: Option<ContractRegistryRecord>,
    next_registry: ContractRegistryRecord,
    staging_identity: ContractObjectIdentity,
    staging_marker_identity: ContractObjectIdentity,
    generated_identities: Vec<ContractRecordedObject>,
    previous_destination_objects: Vec<ContractRecordedObject>,
    previous_destination_identity: Option<ContractObjectIdentity>,
    previous_registry_identity: Option<ContractObjectIdentity>,
    backup_identity: Option<ContractObjectIdentity>,
    installed_identity: Option<ContractObjectIdentity>,
    installed_marker_identity: Option<ContractObjectIdentity>,
    next_registry_identity: Option<ContractObjectIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContractExportJournalRecord {
    journal: ContractExportJournal,
    previous_record_digest: Option<String>,
    record_digest: String,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

struct TestWorkspace {
    root: PathBuf,
}

impl Deref for TestWorkspace {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.root
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn test_workspace(label: &str) -> TestWorkspace {
    let root = std::env::temp_dir().join(format!(
        "fleximark-service-contract-{label}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos()
    ));
    fs::create_dir(&root).expect("create contract test workspace");
    TestWorkspace { root }
}

#[cfg(windows)]
struct JunctionGuard(PathBuf);

#[cfg(windows)]
impl Drop for JunctionGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

#[cfg(windows)]
fn create_junction(path: &Path, target: &Path) -> JunctionGuard {
    use std::process::Command;

    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(path)
        .arg(target)
        .status()
        .expect("invoke mklink");
    assert!(status.success(), "create required test junction");
    JunctionGuard(path.to_owned())
}

fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .expect("metadata node must be an object")
        .keys()
        .map(String::as_str)
        .collect()
}

#[test]
fn public_facade_exports_every_service_operation_at_the_crate_root() {
    let _: fn(&str) -> Result<CommandResult, ServiceError> = initialize_workspace;
    let _: fn(&str) -> Result<CommandResult, ServiceError> = edit_theme;
    let _: fn(&str) -> Result<GetNoteOptionsResult, ServiceError> = get_note_options;
    let _: fn(&str) -> Result<CommandResult, ServiceError> = create_note;
    let _: CreateNoteWithOptionsFn = create_note_with_options;
    let _: fn(&Document, &str, &str) -> Result<CommandResult, ServiceError> = collect_admonitions;
    let _: fn(&str, Option<&RenderStyle>, &str) -> Result<String, ServiceError> =
        compose_portable_html;
    let _: fn(&str) -> Result<String, ServiceError> = default_export_destination;
    let _: fn(&str) -> Result<RenderContext, ServiceError> = export_render_context;
    let _: fn(&str, &str, &str, &[RenderAsset]) -> Result<ResolvedExportAssets, ServiceError> =
        resolve_export_assets;
    let _: fn(&str, &str, &Document) -> Result<Vec<ResolvedRenderAsset>, ServiceError> =
        resolve_render_assets;
    let _: ExportFn = export_html;
    let _: ExportWithSafetyFn = export_html_with_safety;
    let _: fn(&str, &str, &str) -> Result<(), ServiceError> = preflight_export;
    let _: fn(&str, &str, &str) -> Result<(), ServiceError> = acknowledge_export;
    let _: fn(&str) -> Result<PathBuf, ServiceError> = workspace_path;
    let _: fn(&str, &str) -> Result<bool, ServiceError> = document_is_in_workspace;
    let _: fn(&str) -> Result<String, ServiceError> = workspace_for_document;
    let _: LoadPluginHostFn = load_plugin_host;
    let _: fn(&Path) -> Result<String, ServiceError> = path_to_file_uri;
}

#[test]
fn initialization_preserves_default_bytes_and_command_result() {
    let root = test_workspace("defaults");
    let workspace_uri = path_to_file_uri(&root).expect("workspace URI");

    let result = initialize_workspace(&workspace_uri).expect("initialize workspace");

    assert_eq!(
        fs::read_to_string(root.join(".fleximark/config.toml")).expect("read default config"),
        DEFAULT_CONFIG
    );
    assert_eq!(
        fs::read_to_string(root.join(".fleximark/theme.css")).expect("read default theme"),
        DEFAULT_THEME
    );
    let message = result.message.expect("initialization message");
    assert_eq!(message.level, "info");
    assert_eq!(
        message.text,
        "Initialized .fleximark/config.toml and .fleximark/theme.css"
    );
    assert_eq!(
        result.open_uri.expect("config open URI"),
        path_to_file_uri(&root.join(".fleximark/config.toml")).expect("config URI")
    );
}

#[test]
fn export_metadata_preserves_marker_registry_and_journal_wire_schema() {
    let root = test_workspace("metadata");
    let workspace_uri = path_to_file_uri(&root).expect("workspace URI");
    initialize_workspace(&workspace_uri).expect("initialize workspace");
    let source = root.join("document.md");
    fs::write(&source, "# document\n").expect("write source");
    let source_uri = path_to_file_uri(&source).expect("source URI");
    let destination = root.join("public");
    let destination_uri = path_to_file_uri(&destination).expect("destination URI");

    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>first</p>",
        &[],
    )
    .expect("first export");
    let marker_bytes = fs::read(destination.join(".fleximark-export.json")).expect("marker bytes");
    assert!(marker_bytes.starts_with(b"{\n  \"payload\": {"));
    assert!(!marker_bytes.ends_with(b"\n"));
    let typed_marker: ContractOwnershipMarker =
        serde_json::from_slice(&marker_bytes).expect("typed marker JSON");
    assert_eq!(
        marker_bytes,
        serde_json::to_vec_pretty(&typed_marker).expect("serialize typed marker")
    );
    assert_eq!(
        typed_marker.digest,
        sha256(&serde_json::to_vec(&typed_marker.payload).expect("serialize marker payload"))
    );
    let marker: Value = serde_json::from_slice(&marker_bytes).expect("marker JSON");
    assert_eq!(keys(&marker), BTreeSet::from(["digest", "payload"]));
    let payload = &marker["payload"];
    assert_eq!(
        keys(payload),
        BTreeSet::from([
            "destinationId",
            "destinationIdentity",
            "destinationObjectIdentity",
            "files",
            "formatVersion",
            "generation",
            "sourceIdentity",
            "unsafeOutputUsed",
            "workspaceIdentity",
        ])
    );
    assert_eq!(
        keys(&payload["files"][0]),
        BTreeSet::from(["contentHash", "kind", "objectIdentity", "path"])
    );
    acknowledge_export(&source_uri, &workspace_uri, &destination_uri)
        .expect("acknowledge first export");

    let registry_path = fs::read_dir(root.join(".fleximark/export-targets"))
        .expect("registry directory")
        .next()
        .expect("registry entry")
        .expect("registry path")
        .path();
    let registry_bytes = fs::read(registry_path).expect("registry bytes");
    assert!(!registry_bytes.ends_with(b"\n"));
    let typed_registry: ContractRegistryRecord =
        serde_json::from_slice(&registry_bytes).expect("typed registry JSON");
    assert_eq!(
        registry_bytes,
        serde_json::to_vec_pretty(&typed_registry).expect("serialize typed registry")
    );
    let registry: Value = serde_json::from_slice(&registry_bytes).expect("registry JSON");
    assert_eq!(
        keys(&registry),
        BTreeSet::from([
            "destinationId",
            "destinationIdentity",
            "destinationObjectIdentity",
            "digest",
            "generation",
            "sourceIdentity",
            "unsafeOutputUsed",
            "workspaceIdentity",
        ])
    );

    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>second</p>",
        &[],
    )
    .expect("second export");
    let journal_bytes =
        fs::read(root.join(".public.fleximark-export-journal.json")).expect("journal bytes");
    assert!(journal_bytes.ends_with(b"\n"));
    let record_lines = journal_bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let records = record_lines
        .iter()
        .map(|line| serde_json::from_slice::<Value>(line).expect("journal record JSON"))
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 7);
    assert_eq!(
        keys(&records[0]),
        BTreeSet::from(["journal", "previousRecordDigest", "recordDigest"])
    );
    assert_eq!(records[0]["previousRecordDigest"], Value::Null);
    assert_eq!(records[0]["journal"]["state"], "Prepared");
    assert_eq!(
        records.last().expect("committed record")["journal"]["state"],
        "Committed"
    );
    let typed_records = record_lines
        .iter()
        .map(|line| {
            serde_json::from_slice::<ContractExportJournalRecord>(line)
                .expect("typed journal record JSON")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        typed_records
            .iter()
            .map(|record| &record.journal.state)
            .collect::<Vec<_>>(),
        vec![
            &ContractExportJournalState::Prepared,
            &ContractExportJournalState::OldMoveIntent,
            &ContractExportJournalState::OldMoved,
            &ContractExportJournalState::InstallIntent,
            &ContractExportJournalState::NewInstalled,
            &ContractExportJournalState::RegistryIntent,
            &ContractExportJournalState::Committed,
        ]
    );
    let mut previous_digest = None;
    for (line, record) in record_lines.iter().zip(&typed_records) {
        assert_eq!(
            line.to_vec(),
            serde_json::to_vec(record).expect("serialize typed journal record")
        );
        assert_eq!(
            record.previous_record_digest.as_ref(),
            previous_digest.as_ref()
        );
        assert_eq!(
            record.record_digest,
            sha256(
                &serde_json::to_vec(&(&record.journal, record.previous_record_digest.as_deref(),))
                    .expect("serialize journal digest input")
            )
        );
        previous_digest = Some(record.record_digest.clone());
    }
}

// Unix symlink creation is unprivileged and therefore mandatory. Windows exercises the managed
// export boundary with the dedicated junction contract below.
#[cfg(unix)]
#[test]
fn managed_export_rejects_a_symlinked_entry_without_touching_the_target() {
    let root = test_workspace("symlinked-entry");
    let workspace_uri = path_to_file_uri(&root).expect("workspace URI");
    initialize_workspace(&workspace_uri).expect("initialize workspace");
    let source = root.join("document.md");
    fs::write(&source, "# document\n").expect("write source");
    let source_uri = path_to_file_uri(&source).expect("source URI");
    let destination = root.join("public");
    let destination_uri = path_to_file_uri(&destination).expect("destination URI");
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>first</p>",
        &[],
    )
    .expect("first export");
    acknowledge_export(&source_uri, &workspace_uri, &destination_uri)
        .expect("acknowledge first export");

    let outside = root.join("user-owned.txt");
    fs::write(&outside, "user bytes").expect("write user file");
    fs::remove_file(destination.join("index.html")).expect("remove generated index");
    std::os::unix::fs::symlink(&outside, destination.join("index.html"))
        .expect("create required managed-export symlink");

    assert!(matches!(
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>second</p>",
            &[],
        ),
        Err(ServiceError::ExportContentConflict)
    ));
    assert_eq!(
        fs::read_to_string(&outside).expect("read user file"),
        "user bytes"
    );
    assert!(
        fs::symlink_metadata(destination.join("index.html"))
            .expect("symlink metadata")
            .file_type()
            .is_symlink()
    );
}

#[cfg(windows)]
#[test]
fn managed_export_rejects_a_junctioned_entry_without_touching_the_target() {
    let root = test_workspace("junctioned-export-entry");
    let outside = test_workspace("junctioned-export-target");
    let workspace_uri = path_to_file_uri(&root).expect("workspace URI");
    initialize_workspace(&workspace_uri).expect("initialize workspace");
    let source = root.join("document.md");
    fs::write(&source, "# document\n").expect("write source");
    let source_uri = path_to_file_uri(&source).expect("source URI");
    let destination = root.join("public");
    let destination_uri = path_to_file_uri(&destination).expect("destination URI");
    export_html(
        &source_uri,
        &workspace_uri,
        &destination_uri,
        "<p>first</p>",
        &[],
    )
    .expect("first export");
    acknowledge_export(&source_uri, &workspace_uri, &destination_uri)
        .expect("acknowledge first export");

    fs::write(outside.join("sentinel.txt"), "user bytes").expect("write target sentinel");
    let index = destination.join("index.html");
    fs::remove_file(&index).expect("remove generated index");
    let _junction_guard = create_junction(&index, &outside);

    assert!(matches!(
        export_html(
            &source_uri,
            &workspace_uri,
            &destination_uri,
            "<p>second</p>",
            &[],
        ),
        Err(ServiceError::ExportContentConflict)
    ));
    assert_eq!(
        fs::canonicalize(&index).expect("resolve surviving export junction"),
        fs::canonicalize(&*outside).expect("resolve junction target")
    );
    assert_eq!(
        fs::read_to_string(outside.join("sentinel.txt")).expect("read target sentinel"),
        "user bytes"
    );
}

#[cfg(windows)]
#[test]
fn workspace_control_junction_is_rejected_without_touching_its_target() {
    let root = test_workspace("junction-root");
    let outside = test_workspace("junction-target");
    fs::write(outside.join("sentinel.txt"), "user bytes").expect("write target sentinel");
    fs::write(
        outside.join("config.toml"),
        "schema_version = 1\n[security]\nraw_html_preview = \"escape\"\nraw_html_export = \"reject\"\n",
    )
    .expect("write target config");
    let junction = root.join(".fleximark");
    let _junction_guard = create_junction(&junction, &outside);

    let workspace_uri = path_to_file_uri(&root).expect("workspace URI");
    assert!(matches!(
        initialize_workspace(&workspace_uri),
        Err(ServiceError::LinkedControlDirectory)
    ));
    assert_eq!(
        fs::read_to_string(outside.join("sentinel.txt")).expect("read target sentinel"),
        "user bytes"
    );
    assert!(!outside.join("theme.css").exists());
}
