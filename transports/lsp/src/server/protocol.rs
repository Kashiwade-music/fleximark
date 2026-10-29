//! Role: Convert transport DTOs into native document and preview values.
use super::state::*;
use super::wire::*;
impl Server {
    pub(super) fn attach_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        let id = id?;
        let params = match deserialize_params::<AttachDocumentParams>(&id, params) {
            Ok(p) => p,
            Err(e) => return Some(e),
        };
        let result = (|| {
            self.verify_daemon(&params.daemon_instance_id)?;
            let version = u64::try_from(params.expected_document_version.get())
                .map_err(|_| ApplicationError::VersionMismatch)?;
            let attached = self.registry.attach(&params.uri, version)?;
            Ok::<_, ApplicationError>(fleximark_protocol::AttachDocumentResult {
                document_session_id: attached.document_session_id,
                document_version: i64::try_from(attached.document_version)
                    .ok()
                    .and_then(|version| fleximark_protocol::JsSafeI64::new(version).ok())
                    .ok_or(ApplicationError::IntegerRange(attached.document_version))?,
            })
        })();
        Some(match result {
            Ok(result) => response_value(Response::success(id, result)),
            Err(error) => session_error(id, error),
        })
    }
}

pub(super) fn native_position(
    position: &fleximark_protocol::TextPosition,
) -> Result<super::lsp_types::Position, ApplicationError> {
    Ok(super::lsp_types::Position {
        line: position.line.get(),
        character: position.character.get(),
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
