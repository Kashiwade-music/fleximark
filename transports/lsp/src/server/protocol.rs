//! Role: Convert transport DTOs into native document and preview values.
use super::state::*;
use super::wire::*;
use fleximark_application::DocumentAttachmentRequest;

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
        };
        Some(match self.registry.attach(&request) {
            Ok(result) => response_value(Response::success(
                id.clone(),
                fleximark_protocol::AttachDocumentResult {
                    document_session_id: result.document_session_id,
                    document_version: match result.document_version.try_into() {
                        Ok(version) => version,
                        Err(_) => {
                            return Some(boundary_error(
                                id,
                                ApplicationError::IntegerRange(
                                    result.document_version.unsigned_abs(),
                                ),
                            ));
                        }
                    },
                },
            )),
            Err(error) => session_error(id, error),
        })
    }
}

pub(super) fn native_position(
    position: &fleximark_protocol::TextPosition,
) -> Result<fleximark_application::Position, ApplicationError> {
    Ok(fleximark_application::Position {
        line: position
            .line
            .get()
            .try_into()
            .map_err(|_| ApplicationError::InvalidRange)?,
        character: position
            .character
            .get()
            .try_into()
            .map_err(|_| ApplicationError::InvalidRange)?,
    })
}

pub(super) fn counter(value: u64) -> Result<JsSafeU64, fleximark_application::ApplicationError> {
    crate::preview_http::protocol::counter(value)
}
pub(super) fn range(
    range: &fleximark_model::SourceRange,
    source: &str,
    encoding: PositionEncoding,
) -> Result<fleximark_protocol::SourceRange, fleximark_application::ApplicationError> {
    crate::preview_http::protocol::source_range(
        range,
        &fleximark_model::SourceIndex::new(source),
        encoding,
    )
}
pub(super) fn wire_frame(
    frame: &fleximark_application::RenderFrame,
) -> Result<fleximark_protocol::RenderFrame, fleximark_application::ApplicationError> {
    crate::preview_http::protocol::frame(frame)
}
