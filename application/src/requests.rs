//! Role: Carry native full-document updates and versioned attachments.
#[derive(Clone, Debug)]
pub struct DocumentUpdate {
    pub uri: String,
    pub version: u64,
    pub source: String,
}
#[derive(Clone, Debug)]
pub struct DocumentAttachment {
    pub document_session_id: String,
    pub document_version: u64,
}
