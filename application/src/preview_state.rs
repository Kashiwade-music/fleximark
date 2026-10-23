//! Role: Publish preview snapshots and reject stale navigation.
use crate::document::DocumentSession;
use crate::error::{SessionError, engine_error};
use crate::snapshots::{PreviewSessionId, RenderFrame};
use fleximark_model::{NavigationEntry, NodeId};
use fleximark_plugin_api::CancellationToken;

impl DocumentSession {
    pub(crate) fn publish(
        &mut self,
        id: &str,
        cancellation: &CancellationToken,
    ) -> Result<RenderFrame, SessionError> {
        let revision = self.previews.get(id).map_or(Ok(1), |previous| {
            previous
                .render_revision
                .checked_add(1)
                .ok_or(SessionError::RevisionExhausted)
        })?;
        let snapshot = self
            .engine
            .render_configured(cancellation)
            .map_err(engine_error)?
            .snapshot;
        let mut frame =
            RenderFrame::from_snapshot(PreviewSessionId(id.to_owned()), revision, snapshot);
        frame.source = std::sync::Arc::from(self.engine.source());
        frame.position_encoding = self.engine.position_encoding();
        self.previews.insert(id.to_owned(), frame.clone());
        Ok(frame)
    }

    pub(crate) fn navigate(
        &self,
        id: &str,
        revision: u64,
        node: &NodeId,
    ) -> Result<Option<NavigationEntry>, SessionError> {
        if self.engine.is_out_of_sync() {
            return Err(SessionError::ContentModified);
        }
        let frame = self.previews.get(id).ok_or(SessionError::UnknownSession)?;
        if frame.render_revision != revision
            || frame.document_version != self.document().document_version
        {
            return Err(SessionError::ContentModified);
        }
        Ok(frame
            .navigation
            .iter()
            .find(|entry| &entry.node_id == node)
            .cloned())
    }
}
