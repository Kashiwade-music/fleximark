//! Role: Describe standard LSP payloads exposed by the daemon transport.
use crate::commands::WorkspaceCommandRequest;
use crate::wire_values::TextRange;
use fleximark_wire::JsSafeU64;

pub const SEMANTIC_TOKEN_TYPES: [&str; 5] = ["keyword", "string", "operator", "type", "property"];
pub const SEMANTIC_TOKEN_MODIFIERS: [&str; 0] = [];
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspTextDocument {
    pub uri: String,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspDocumentParams {
    pub text_document: LspTextDocument,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspCodeActionParams {
    pub text_document: LspTextDocument,
    pub range: TextRange,
    pub context: serde_json::Value,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspExecuteCommandParams {
    pub command: String,
    pub arguments: Vec<WorkspaceCommandRequest>,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspEditorCommand {
    pub title: String,
    pub command: String,
    pub arguments: Vec<WorkspaceCommandRequest>,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspCodeAction {
    pub title: String,
    pub kind: String,
    pub command: LspEditorCommand,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspSemanticTokens {
    pub data: Vec<JsSafeU64>,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspShowDocumentParams {
    pub uri: String,
    pub external: bool,
    pub take_focus: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<TextRange>,
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LspLogMessageParams {
    #[serde(rename = "type")]
    pub message_type: JsSafeU64,
    pub message: String,
}
