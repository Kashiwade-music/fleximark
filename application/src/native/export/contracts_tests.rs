//! Role: Verify export wire formats and filesystem boundary contracts.
use crate::ApplicationError;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::native::{acknowledge_export, export_html, initialize_workspace, path_to_file_uri};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

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

#[cfg(windows)]
use crate::native::test_support::create_junction;
use crate::native::test_support::test_workspace;
use fleximark_test_support::TempWorkspace as TestWorkspace;

struct ExportFixture {
    root: TestWorkspace,
    workspace_uri: String,
    source_uri: String,
    destination: PathBuf,
    destination_uri: String,
}

impl ExportFixture {
    fn initialized(label: &str) -> Self {
        let root = test_workspace(label);
        let workspace_uri = path_to_file_uri(&root).expect("workspace URI");
        initialize_workspace(&workspace_uri).expect("initialize workspace");
        let source = root.join("document.md");
        fs::write(&source, "# document\n").expect("write source");
        let destination = root.join("public");
        Self {
            source_uri: path_to_file_uri(&source).expect("source URI"),
            destination_uri: path_to_file_uri(&destination).expect("destination URI"),
            destination,
            root,
            workspace_uri,
        }
    }

    fn export(&self, html: &str) -> Result<crate::ExportReceipt, ApplicationError> {
        export_html(
            &self.source_uri,
            &self.workspace_uri,
            &self.destination_uri,
            html,
            &[],
        )
    }

    fn acknowledge(&self) {
        acknowledge_export(&self.source_uri, &self.workspace_uri, &self.destination_uri)
            .expect("acknowledge export");
    }
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
fn export_metadata_preserves_marker_registry_and_journal_wire_schema() {
    let fixture = ExportFixture::initialized("metadata");
    fixture.export("<p>first</p>").expect("first export");
    let marker_bytes =
        fs::read(fixture.destination.join(".fleximark-export.json")).expect("marker bytes");
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
            "workspaceIdentity",
        ])
    );
    assert_eq!(
        keys(&payload["files"][0]),
        BTreeSet::from(["contentHash", "kind", "objectIdentity", "path"])
    );
    fixture.acknowledge();

    let registry_path = fs::read_dir(fixture.root.join(".fleximark/export-targets"))
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
            "workspaceIdentity",
        ])
    );

    fixture.export("<p>second</p>").expect("second export");
    let journal_bytes = fs::read(fixture.root.join(".public.fleximark-export-journal.json"))
        .expect("journal bytes");
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
    let fixture = ExportFixture::initialized("symlinked-entry");
    fixture.export("<p>first</p>").expect("first export");
    fixture.acknowledge();

    let outside = fixture.root.join("user-owned.txt");
    fs::write(&outside, "user bytes").expect("write user file");
    fs::remove_file(fixture.destination.join("index.html")).expect("remove generated index");
    std::os::unix::fs::symlink(&outside, fixture.destination.join("index.html"))
        .expect("create required managed-export symlink");

    assert!(matches!(
        fixture.export("<p>second</p>"),
        Err(ApplicationError::ExportContentConflict)
    ));
    assert_eq!(
        fs::read_to_string(&outside).expect("read user file"),
        "user bytes"
    );
    assert!(
        fs::symlink_metadata(fixture.destination.join("index.html"))
            .expect("symlink metadata")
            .file_type()
            .is_symlink()
    );
}

#[cfg(windows)]
#[test]
fn managed_export_rejects_a_junctioned_entry_without_touching_the_target() {
    let fixture = ExportFixture::initialized("junctioned-export-entry");
    let outside = test_workspace("junctioned-export-target");
    fixture.export("<p>first</p>").expect("first export");
    fixture.acknowledge();

    fs::write(outside.join("sentinel.txt"), "user bytes").expect("write target sentinel");
    let index = fixture.destination.join("index.html");
    fs::remove_file(&index).expect("remove generated index");
    let _junction_guard = create_junction(&index, &outside);

    assert!(matches!(
        fixture.export("<p>second</p>"),
        Err(ApplicationError::ExportContentConflict)
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
