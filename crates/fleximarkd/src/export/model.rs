use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExportFile {
    pub(super) path: String,
    pub(super) kind: String,
    pub(super) content_hash: String,
    pub(super) object_identity: ObjectIdentity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OwnershipPayload {
    pub(super) format_version: u32,
    pub(super) destination_id: String,
    pub(super) generation: u64,
    pub(super) source_identity: String,
    pub(super) workspace_identity: String,
    pub(super) destination_identity: String,
    pub(super) destination_object_identity: ObjectIdentity,
    pub(super) unsafe_output_used: bool,
    pub(super) files: Vec<ExportFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct OwnershipMarker {
    pub(super) payload: OwnershipPayload,
    pub(super) digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct RegistryRecord {
    pub(super) destination_id: String,
    pub(super) generation: u64,
    pub(super) digest: String,
    pub(super) source_identity: String,
    pub(super) workspace_identity: String,
    pub(super) destination_identity: String,
    pub(super) destination_object_identity: ObjectIdentity,
    pub(super) unsafe_output_used: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) enum ExportJournalState {
    Prepared,
    OldMoveIntent,
    OldMoved,
    InstallIntent,
    NewInstalled,
    RegistryIntent,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct ExportJournal {
    pub(super) transaction_id: String,
    pub(super) state: ExportJournalState,
    pub(super) destination: PathBuf,
    pub(super) staging: PathBuf,
    pub(super) backup: PathBuf,
    pub(super) registry: PathBuf,
    pub(super) journal_identity: ObjectIdentity,
    pub(super) registry_staging: Option<PathBuf>,
    pub(super) registry_staging_identity: Option<ObjectIdentity>,
    pub(super) previous_registry: Option<RegistryRecord>,
    pub(super) next_registry: RegistryRecord,
    pub(super) staging_identity: ObjectIdentity,
    pub(super) staging_marker_identity: ObjectIdentity,
    pub(super) generated_identities: Vec<RecordedObject>,
    pub(super) previous_destination_objects: Vec<RecordedObject>,
    pub(super) previous_destination_identity: Option<ObjectIdentity>,
    pub(super) previous_registry_identity: Option<ObjectIdentity>,
    pub(super) backup_identity: Option<ObjectIdentity>,
    pub(super) installed_identity: Option<ObjectIdentity>,
    pub(super) installed_marker_identity: Option<ObjectIdentity>,
    pub(super) next_registry_identity: Option<ObjectIdentity>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObjectIdentity {
    pub(super) platform_id: String,
    pub(super) kind: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct RecordedObject {
    pub(super) relative_path: String,
    pub(super) identity: ObjectIdentity,
    pub(super) content_hash: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExportJournalRecord {
    pub(super) journal: ExportJournal,
    pub(super) previous_record_digest: Option<String>,
    pub(super) record_digest: String,
}

pub(super) struct ManagedExport {
    pub(super) marker: OwnershipMarker,
    pub(super) registry: RegistryRecord,
}
