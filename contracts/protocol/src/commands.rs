//! Role: Define typed workspace commands, outcomes and their shared capabilities.
use crate::command_values::{CreatePreviewResult, PreviewTarget};
use fleximark_wire::{JsSafeI64, JsSafeU64};
use serde::{Deserialize, Deserializer, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WorkspaceUiError {
    pub error: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceCommandRequest {
    #[serde(deserialize_with = "nonempty")]
    #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
    pub daemon_instance_id: String,
    pub context: CommandContext,
    pub operation: WorkspaceCommand,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandContext {
    #[serde(
        default,
        deserialize_with = "optional_nonempty",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "contract-schema", schemars(transform = crate::schema::non_null, extend("minLength" = 1)))]
    pub workspace_uri: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "contract-schema", schemars(transform = crate::schema::non_null))]
    pub document: Option<DocumentCommandContext>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentCommandContext {
    #[serde(deserialize_with = "nonempty")]
    #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
    pub document_session_id: String,
    pub expected_document_version: JsSafeI64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "command",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceCommand {
    InitializeWorkspace {},
    EditTheme {},
    CreateNote {
        #[serde(
            default,
            deserialize_with = "category_path",
            skip_serializing_if = "Option::is_none"
        )]
        #[cfg_attr(feature = "contract-schema", schemars(transform = crate::schema::non_null, extend("minItems" = 1, "maxItems" = 32, "items" = {"type":"string","minLength":1})))]
        category_path: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template: Option<String>,
        #[serde(
            default,
            deserialize_with = "note_name",
            skip_serializing_if = "Option::is_none"
        )]
        #[cfg_attr(feature = "contract-schema", schemars(transform = crate::schema::non_null, extend("minLength" = 1, "maxLength" = 255)))]
        name: Option<String>,
    },
    GetNoteOptions {},
    CollectAdmonitions {},
    ExportHtml {
        destination_uri: Option<String>,
    },
    AcknowledgeExport {
        destination_uri: Option<String>,
    },
    PlanMigration {
        settings_uri: Option<String>,
        recover: bool,
    },
    ApplyMigration {
        plan_id: String,
    },
    OpenPreview {
        target: PreviewTarget,
        source_offset: Option<JsSafeU64>,
    },
    OpenWorkspace {},
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum CommandKind {
    InitializeWorkspace,
    EditTheme,
    CreateNote,
    GetNoteOptions,
    CollectAdmonitions,
    ExportHtml,
    AcknowledgeExport,
    PlanMigration,
    ApplyMigration,
    OpenPreview,
    OpenWorkspace,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDescriptor {
    pub kind: CommandKind,
    pub name: &'static str,
    pub lsp_command: &'static str,
    pub title: &'static str,
    pub requires_document: bool,
    pub requires_workspace: bool,
    pub requires_trust: bool,
    pub code_action: bool,
    pub browser_ui: bool,
    pub cli: bool,
}
pub const COMMAND_DESCRIPTORS: &[CommandDescriptor] = &[
    CommandDescriptor {
        kind: CommandKind::InitializeWorkspace,
        name: "initializeWorkspace",
        lsp_command: "fleximark.initializeWorkspace",
        title: "Initialize workspace",
        requires_document: false,
        requires_workspace: true,
        requires_trust: true,
        code_action: true,
        browser_ui: true,
        cli: true,
    },
    CommandDescriptor {
        kind: CommandKind::EditTheme,
        name: "editTheme",
        lsp_command: "fleximark.editTheme",
        title: "Edit theme",
        requires_document: false,
        requires_workspace: true,
        requires_trust: true,
        code_action: true,
        browser_ui: true,
        cli: true,
    },
    CommandDescriptor {
        kind: CommandKind::CreateNote,
        name: "createNote",
        lsp_command: "fleximark.createNote",
        title: "Create note",
        requires_document: false,
        requires_workspace: true,
        requires_trust: true,
        code_action: true,
        browser_ui: true,
        cli: true,
    },
    CommandDescriptor {
        kind: CommandKind::GetNoteOptions,
        name: "getNoteOptions",
        lsp_command: "fleximark.getNoteOptions",
        title: "Get note options",
        requires_document: false,
        requires_workspace: true,
        requires_trust: true,
        code_action: false,
        browser_ui: true,
        cli: false,
    },
    CommandDescriptor {
        kind: CommandKind::CollectAdmonitions,
        name: "collectAdmonitions",
        lsp_command: "fleximark.collectAdmonitions",
        title: "Collect admonitions",
        requires_document: true,
        requires_workspace: true,
        requires_trust: true,
        code_action: true,
        browser_ui: true,
        cli: true,
    },
    CommandDescriptor {
        kind: CommandKind::ExportHtml,
        name: "exportHtml",
        lsp_command: "fleximark.exportHtml",
        title: "Export HTML",
        requires_document: true,
        requires_workspace: true,
        requires_trust: true,
        code_action: true,
        browser_ui: true,
        cli: true,
    },
    CommandDescriptor {
        kind: CommandKind::AcknowledgeExport,
        name: "acknowledgeExport",
        lsp_command: "fleximark.acknowledgeExport",
        title: "Acknowledge export",
        requires_document: true,
        requires_workspace: true,
        requires_trust: true,
        code_action: false,
        browser_ui: true,
        cli: true,
    },
    CommandDescriptor {
        kind: CommandKind::PlanMigration,
        name: "planMigration",
        lsp_command: "fleximark.planMigration",
        title: "Plan migration",
        requires_document: false,
        requires_workspace: true,
        requires_trust: true,
        code_action: true,
        browser_ui: true,
        cli: true,
    },
    CommandDescriptor {
        kind: CommandKind::ApplyMigration,
        name: "applyMigration",
        lsp_command: "fleximark.applyMigration",
        title: "Apply migration",
        requires_document: false,
        requires_workspace: true,
        requires_trust: true,
        code_action: false,
        browser_ui: true,
        cli: true,
    },
    CommandDescriptor {
        kind: CommandKind::OpenPreview,
        name: "openPreview",
        lsp_command: "fleximark.openPreview",
        title: "Preview document",
        requires_document: true,
        requires_workspace: false,
        requires_trust: false,
        code_action: true,
        browser_ui: false,
        cli: false,
    },
    CommandDescriptor {
        kind: CommandKind::OpenWorkspace,
        name: "openWorkspace",
        lsp_command: "fleximark.openWorkspace",
        title: "Open FlexiMark workspace",
        requires_document: false,
        requires_workspace: true,
        requires_trust: false,
        code_action: true,
        browser_ui: false,
        cli: false,
    },
];
impl WorkspaceCommand {
    pub const fn kind(&self) -> CommandKind {
        match self {
            Self::InitializeWorkspace {} => CommandKind::InitializeWorkspace,
            Self::EditTheme {} => CommandKind::EditTheme,
            Self::CreateNote { .. } => CommandKind::CreateNote,
            Self::GetNoteOptions {} => CommandKind::GetNoteOptions,
            Self::CollectAdmonitions {} => CommandKind::CollectAdmonitions,
            Self::ExportHtml { .. } => CommandKind::ExportHtml,
            Self::AcknowledgeExport { .. } => CommandKind::AcknowledgeExport,
            Self::PlanMigration { .. } => CommandKind::PlanMigration,
            Self::ApplyMigration { .. } => CommandKind::ApplyMigration,
            Self::OpenPreview { .. } => CommandKind::OpenPreview,
            Self::OpenWorkspace {} => CommandKind::OpenWorkspace,
        }
    }
    pub const fn descriptor(&self) -> &'static CommandDescriptor {
        self.kind().descriptor()
    }
}
impl CommandKind {
    pub const fn descriptor(self) -> &'static CommandDescriptor {
        match self {
            Self::InitializeWorkspace => &COMMAND_DESCRIPTORS[0],
            Self::EditTheme => &COMMAND_DESCRIPTORS[1],
            Self::CreateNote => &COMMAND_DESCRIPTORS[2],
            Self::GetNoteOptions => &COMMAND_DESCRIPTORS[3],
            Self::CollectAdmonitions => &COMMAND_DESCRIPTORS[4],
            Self::ExportHtml => &COMMAND_DESCRIPTORS[5],
            Self::AcknowledgeExport => &COMMAND_DESCRIPTORS[6],
            Self::PlanMigration => &COMMAND_DESCRIPTORS[7],
            Self::ApplyMigration => &COMMAND_DESCRIPTORS[8],
            Self::OpenPreview => &COMMAND_DESCRIPTORS[9],
            Self::OpenWorkspace => &COMMAND_DESCRIPTORS[10],
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CommandResult {
    WorkspaceInitialized {
        config_uri: String,
        theme_uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    ThemeEdited {
        uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    NoteCreated {
        uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    NoteOptions {
        options: GetNoteOptionsResult,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    AdmonitionsCollected {
        uri: String,
        count: JsSafeU64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    HtmlExported {
        output_uri: String,
        generation: JsSafeU64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    ExportAcknowledged {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    MigrationPlanned {
        plan: MigrationPlanResult,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    MigrationApplied {
        backup_uri: Option<String>,
        recovered: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    PreviewOpened {
        preview: CreatePreviewResult,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
    WorkspaceOpened {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<CommandMessage>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationPlanResult {
    pub plan_id: String,
    pub workspace_uri: String,
    pub status: MigrationStatus,
    pub outputs: Vec<String>,
    pub warnings: Vec<String>,
    pub config: String,
    pub recovery: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum MigrationStatus {
    Ready,
    AlreadyCurrent,
    RecoveryRequired,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteCategoryOption {
    #[cfg_attr(feature = "contract-schema", schemars(length(min = 1)))]
    pub name: String,
    pub children: Vec<NoteCategoryOption>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetNoteOptionsResult {
    #[cfg_attr(feature = "contract-schema", schemars(extend("uniqueItems" = true)))]
    pub categories: Vec<NoteCategoryOption>,
    #[cfg_attr(feature = "contract-schema", schemars(extend("uniqueItems" = true)))]
    pub templates: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandMessage {
    pub level: CommandMessageLevel,
    pub text: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CommandMessageLevel {
    Info,
    Warning,
    Error,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceStateRequest {
    pub context: WorkspaceUiContext,
    pub command: Option<CommandKind>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceUiContext {
    #[serde(
        default,
        deserialize_with = "optional_nonempty",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "contract-schema", schemars(transform = crate::schema::non_null, extend("minLength" = 1)))]
    pub workspace_uri: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_nonempty",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "contract-schema", schemars(transform = crate::schema::non_null, extend("minLength" = 1)))]
    pub document_uri: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_present",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "contract-schema", schemars(transform = crate::schema::non_null))]
    pub document_version: Option<JsSafeI64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceStateResult {
    pub workspaces: Vec<WorkspaceUiWorkspace>,
    pub documents: Vec<WorkspaceUiDocument>,
    pub note_options: Option<GetNoteOptionsResult>,
    pub command: Option<CommandKind>,
    pub context: WorkspaceUiContext,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceUiWorkspace {
    pub uri: String,
    pub name: String,
    pub trusted: bool,
    pub note_options: Option<GetNoteOptionsResult>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceUiDocument {
    pub uri: String,
    pub version: JsSafeI64,
    pub workspace_uri: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceUiCommandRequest {
    pub context: WorkspaceUiContext,
    pub operation: WorkspaceCommand,
}
fn nonempty<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom("identifier must not be empty"));
    }
    Ok(value)
}
fn optional_nonempty<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    nonempty(deserializer).map(Some)
}
fn note_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty()
        || value.chars().count() > 255
        || value.contains(['/', '\\'])
        || value == "."
        || value == ".."
    {
        return Err(serde::de::Error::custom(
            "note name must be a safe name of 1 to 255 characters",
        ));
    }
    Ok(Some(value))
}
fn category_path<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error> {
    let path = Vec::<String>::deserialize(deserializer)?;
    if path.is_empty()
        || path.len() > 32
        || path.iter().any(|part| {
            part.is_empty() || part == "." || part == ".." || part.contains(['/', '\\'])
        })
    {
        return Err(serde::de::Error::custom(
            "category path requires 1 to 32 safe nonempty segments",
        ));
    }
    Ok(Some(path))
}

fn optional_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests;
