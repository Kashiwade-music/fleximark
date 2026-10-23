//! Role: Define native document updates, attachment and synchronization values.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenParams {
    pub text_document: TextDocumentItem,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TextDocumentItem {
    pub uri: String,
    pub version: i64,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeParams {
    pub text_document: VersionedTextDocumentIdentifier,
    pub content_changes: Vec<ContentChange>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct VersionedTextDocumentIdentifier {
    pub uri: String,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ContentChange {
    pub range: Option<Range>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidCloseParams {
    pub text_document: TextDocumentIdentifier,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

#[derive(Clone, Debug)]
pub struct DocumentAttachmentRequest {
    pub daemon_instance_id: String,
    pub uri: String,
    pub expected_document_version: i64,
}
#[derive(Clone, Debug)]
pub struct DocumentAttachment {
    pub document_session_id: String,
    pub document_version: i64,
}
