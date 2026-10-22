//! Role: Convert transport DTOs into native document and preview values.
use super::state::*;
use super::wire::*;
use fleximark_application::{DocumentAttachmentRequest, DocumentCheckpointRequest};

impl Server {
    pub(super) fn attach_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<AttachDocumentParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        let request = DocumentAttachmentRequest {
            daemon_instance_id: params.daemon_instance_id,
            uri: params.uri,
            expected_document_version: params.expected_document_version.get(),
            content_hash: params.content_hash,
        };
        Some(match self.registry.attach(&request) {
            Ok(result) => response_value(Response::success(
                id.clone(),
                fleximark_protocol::AttachDocumentResult {
                    document_session_id: result.document_session_id,
                    document_version: match result.document_version.try_into() {
                        Ok(version) => version,
                        Err(error) => return Some(boundary_error(id, error)),
                    },
                    content_hash: result.content_hash,
                },
            )),
            Err(error) => session_error(id, error),
        })
    }

    pub(super) fn checkpoint_document(
        &mut self,
        id: Option<Value>,
        params: Value,
    ) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<CheckpointDocumentParams>(&id, params) {
            Ok(params) => params,
            Err(response) => return Some(response),
        };
        let request = DocumentCheckpointRequest {
            daemon_instance_id: params.daemon_instance_id,
            document_session_id: params.document_session_id,
            document_version: params.document_version.get(),
            content_hash: params.content_hash,
        };
        Some(match self.registry.checkpoint(&request) {
            Ok(result) => response_value(Response::success(
                id.clone(),
                fleximark_protocol::CheckpointDocumentResult {
                    document_version: match result.document_version.try_into() {
                        Ok(version) => version,
                        Err(error) => return Some(boundary_error(id, error)),
                    },
                    content_hash: result.content_hash,
                },
            )),
            Err(error) => session_error(id, error),
        })
    }
}

pub(super) fn native_position(
    position: &fleximark_protocol::TextPosition,
) -> Result<fleximark_application::Position, SessionError> {
    Ok(fleximark_application::Position {
        line: position
            .line
            .get()
            .try_into()
            .map_err(|_| SessionError::InvalidRange)?,
        character: position
            .character
            .get()
            .try_into()
            .map_err(|_| SessionError::InvalidRange)?,
    })
}

pub(super) fn counter(
    value: u64,
) -> Result<JsSafeU64, crate::preview_http::protocol::ConversionError> {
    crate::preview_http::protocol::counter(value)
}
pub(super) fn range(
    range: &fleximark_model::SourceRange,
) -> Result<fleximark_protocol::SourceRange, crate::preview_http::protocol::ConversionError> {
    crate::preview_http::protocol::source_range(range)
}
pub(super) fn wire_frame(
    frame: &fleximark_application::RenderFrame,
) -> Result<fleximark_protocol::RenderFrame, crate::preview_http::protocol::ConversionError> {
    crate::preview_http::protocol::frame(frame)
}
