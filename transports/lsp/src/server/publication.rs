//! Role: Deliver and coalesce complete preview publications in one sequence.
use super::state::*;
use super::wire::preview_changed_notification;
use fleximark_wire_mapping::frame as wire_frame;
impl Server {
    pub(super) fn preview_frame(
        &self,
        id: &str,
    ) -> Result<fleximark_application::PreviewHandle, ApplicationError> {
        let state = self
            .preview_states
            .get(id)
            .ok_or(ApplicationError::UnknownSession)?;
        let frame = self
            .registry
            .read_preview(&state.document_session_id, id, None)?
            .ok_or(ApplicationError::UnknownSession)?;
        if !frame.is_current() {
            return Err(ApplicationError::ContentModified);
        }
        Ok(frame)
    }
    pub(super) fn publish_document_previews(&mut self, uri: &str) -> Result<(), ApplicationError> {
        self.publish_lsp_diagnostics(uri);
        let Some(session) = self.registry.session_id_for_uri(uri).map(str::to_owned) else {
            return Ok(());
        };
        let version = self.registry.current_version(&session)?;
        let previews = self
            .preview_states
            .iter()
            .filter(|(_, state)| state.document_session_id == session)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for preview in previews {
            let frame = self.registry.render_with_cancellation(
                &session,
                version,
                &preview,
                &self.work_cancellation,
            )?;
            self.publish_frame(&preview, &frame)?;
        }
        Ok(())
    }
    pub(super) fn publish_frame(
        &mut self,
        preview_id: &str,
        frame: &fleximark_application::PreviewHandle,
    ) -> Result<bool, fleximark_application::ApplicationError> {
        if self.publication_cancellation.is_cancelled() {
            return Ok(false);
        }
        let checked = wire_frame(frame)?;
        if let Some(token) = self
            .preview_states
            .get(preview_id)
            .and_then(|state| state.token.as_deref())
        {
            self.previews.update(token, frame)?;
        }
        self.outgoing_events.retain(|event| {
            event.get("method").and_then(Value::as_str) != Some(method::PREVIEW_CHANGED)
                || event
                    .pointer("/params/previewSessionId")
                    .and_then(Value::as_str)
                    != Some(preview_id)
        });
        self.outgoing_events
            .push(preview_changed_notification(PreviewChangedParams {
                daemon_instance_id: self.daemon_instance_id().to_owned(),
                preview_session_id: preview_id.to_owned(),
                render_revision: checked.render_revision,
                frame: checked,
            }));
        let _ = self.refresh_explicit_jump(preview_id);
        Ok(true)
    }
}
