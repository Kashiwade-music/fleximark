//! Role: Publish preview snapshots and reject stale navigation.
use crate::CancellationToken;
use crate::document::DocumentSession;
use crate::error::{ApplicationError, engine_error};
use crate::snapshots::{PreviewHandle, PreviewSessionId, RenderFrame};
use fleximark_model::{NavigationEntry, NodeId};

impl DocumentSession {
    pub(crate) fn publish(
        &mut self,
        id: &str,
        cancellation: &CancellationToken,
    ) -> Result<PreviewHandle, ApplicationError> {
        let revision = self.previews.get(id).map_or(Ok(1), |previous| {
            previous
                .render_revision
                .checked_add(1)
                .ok_or(ApplicationError::RevisionExhausted)
        })?;
        let snapshot = (|| {
            crate::cancellation_checks::check(cancellation, "render_start")?;
            let snapshot = self
                .engine
                .render(&self.engine.render_config().context)
                .map_err(engine_error)?;
            crate::cancellation_checks::check(cancellation, "render_commit")?;
            Ok::<_, ApplicationError>(snapshot)
        })();
        let snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if let Some(previous) = self.previews.get(id) {
                    previous.invalidate_publication();
                }
                return Err(error);
            }
        };
        let mut frame =
            RenderFrame::from_snapshot(PreviewSessionId(id.to_owned()), revision, snapshot);
        frame.source = std::sync::Arc::from(self.engine.source());
        frame.position_encoding = self.engine.position_encoding();
        let publication = self.previews.get(id).map(PreviewHandle::publication);
        let handle =
            PreviewHandle::new(frame, std::sync::Arc::clone(&self.authority), publication)?;
        self.previews.insert(id.to_owned(), handle.clone());
        Ok(handle)
    }

    pub(crate) fn navigate(
        &self,
        id: &str,
        revision: u64,
        node: &NodeId,
    ) -> Result<Option<NavigationEntry>, ApplicationError> {
        if self.engine.is_out_of_sync() {
            return Err(ApplicationError::ContentModified);
        }
        let frame = self
            .previews
            .get(id)
            .ok_or(ApplicationError::UnknownSession)?;
        if !frame.is_current()
            || frame.render_revision != revision
            || frame.document_version != self.document().document_version
        {
            return Err(ApplicationError::ContentModified);
        }
        Ok(frame
            .navigation
            .iter()
            .find(|entry| &entry.node_id == node)
            .cloned())
    }
}
