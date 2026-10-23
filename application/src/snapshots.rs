//! Role: Frame immutable render snapshots with preview identity and source context.
use fleximark_engine::RenderSnapshot;
use fleximark_model::PositionEncoding;
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PreviewSessionId(pub String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderFrame {
    pub preview_session_id: PreviewSessionId,
    pub render_revision: u64,
    pub snapshot: Arc<RenderSnapshot>,
    pub source: Arc<str>,
    pub position_encoding: PositionEncoding,
}
impl std::ops::Deref for RenderFrame {
    type Target = RenderSnapshot;
    fn deref(&self) -> &Self::Target {
        &self.snapshot
    }
}
impl RenderFrame {
    pub fn from_snapshot(id: PreviewSessionId, revision: u64, snapshot: RenderSnapshot) -> Self {
        Self {
            preview_session_id: id,
            render_revision: revision,
            snapshot: Arc::new(snapshot),
            source: Arc::from(""),
            position_encoding: PositionEncoding::Utf8,
        }
    }
    pub fn html(&self) -> String {
        self.snapshot.html()
    }
}
