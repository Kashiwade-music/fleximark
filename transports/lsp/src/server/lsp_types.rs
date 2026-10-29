//! Role: Describe standard LSP capabilities and source features as typed payloads.
use serde::Serialize;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Initialization {
    pub capabilities: Capabilities,
    pub server_info: ServerInfo,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Capabilities {
    pub position_encoding: &'static str,
    pub text_document_sync: DocumentSync,
    pub completion_provider: CompletionProvider,
    pub hover_provider: bool,
    pub document_symbol_provider: bool,
    pub code_action_provider: bool,
    pub execute_command_provider: ExecuteCommandProvider,
    pub experimental: Experimental,
    pub semantic_tokens_provider: SemanticTokensProvider,
    pub diagnostic_provider: DiagnosticProvider,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DocumentSync {
    pub open_close: bool,
    pub change: u8,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CompletionProvider {
    pub trigger_characters: Vec<&'static str>,
}
#[derive(Serialize)]
pub(super) struct ExecuteCommandProvider {
    pub commands: Vec<&'static str>,
}
#[derive(Serialize)]
pub(super) struct Experimental {
    pub fleximark: fleximark_protocol::InitializeResult,
}
#[derive(Serialize)]
pub(super) struct SemanticTokensProvider {
    pub legend: Legend,
    pub full: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Legend {
    pub token_types: &'static [&'static str],
    pub token_modifiers: Vec<&'static str>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DiagnosticProvider {
    pub inter_file_dependencies: bool,
    pub workspace_diagnostics: bool,
}
#[derive(Serialize)]
pub(super) struct ServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}
#[derive(Clone, Debug, serde::Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Position {
    pub line: u64,
    pub character: u64,
}
impl From<fleximark_model::SourcePosition> for Position {
    fn from(value: fleximark_model::SourcePosition) -> Self {
        Self {
            line: value.line,
            character: value.character,
        }
    }
}
#[derive(Clone, Serialize)]
pub(super) struct Range {
    pub start: Position,
    pub end: Position,
}
#[derive(Serialize)]
pub(super) struct Diagnostic {
    pub range: Range,
    pub severity: u8,
    pub code: &'static str,
    pub source: &'static str,
    pub message: String,
}
#[derive(Serialize)]
pub(super) struct DiagnosticReport {
    pub kind: &'static str,
    pub items: Vec<Diagnostic>,
}
#[derive(Serialize)]
pub(super) struct PublishDiagnostics<'a> {
    pub uri: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Symbol {
    pub name: String,
    pub kind: u8,
    pub range: Range,
    pub selection_range: Range,
}
#[derive(Serialize)]
pub(super) struct Hover {
    pub contents: MarkupContent,
}
#[derive(Serialize)]
pub(super) struct MarkupContent {
    pub kind: &'static str,
    pub value: String,
}
