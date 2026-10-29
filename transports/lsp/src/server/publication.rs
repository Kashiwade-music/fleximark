//! Role: Deliver and coalesce complete preview publications in one sequence.
use super::protocol::wire_frame;
use super::state::*;
use super::wire::preview_changed_notification;
impl Server {
    pub(super) fn publish_frame(
        &mut self,
        preview_id: &str,
        frame: &fleximark_application::RenderFrame,
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
        if let Some(state) = self.preview_states.get_mut(preview_id) {
            state.delivered_revision = frame.render_revision;
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
