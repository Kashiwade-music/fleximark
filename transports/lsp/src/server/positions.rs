//! Role: Resolve negotiated LSP positions through the shared UTF-8 source index.
use super::lsp_types::Position;
use super::state::Server;
use fleximark_application::ApplicationError;
impl Server {
    pub(super) fn position_offset(
        &self,
        uri: &str,
        position: &Position,
    ) -> Result<usize, ApplicationError> {
        let source = self.registry.source_for_uri(uri)?;
        fleximark_model::SourceIndex::new(source)
            .position_to_byte(&fleximark_model::SourcePosition {
                line: position.line,
                character: position.character,
                encoding: self.registry.position_encoding(),
            })
            .ok_or(ApplicationError::InvalidRange)
    }
    pub(super) fn navigation_at_position(
        &self,
        session: &str,
        version: u64,
        position: &Position,
    ) -> Result<Option<fleximark_model::NavigationEntry>, ApplicationError> {
        let uri = &self.registry.document(session, version)?.uri;
        self.registry.navigation_at_source_offset(
            session,
            version,
            self.position_offset(uri, position)? as u64,
        )
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
