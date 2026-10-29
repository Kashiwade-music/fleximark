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

#[derive(Clone, Debug)]
pub struct PreviewHandle {
    pub(crate) frame: Arc<RenderFrame>,
    authority: Arc<crate::authority::DocumentAuthority>,
    epoch: u64,
    publication: Arc<std::sync::atomic::AtomicU64>,
}
impl PreviewHandle {
    pub(crate) fn new(
        frame: RenderFrame,
        authority: Arc<crate::authority::DocumentAuthority>,
        publication: Option<Arc<std::sync::atomic::AtomicU64>>,
    ) -> Result<Self, crate::ApplicationError> {
        let epoch = authority.state()?.source_epoch;
        let publication =
            publication.unwrap_or_else(|| Arc::new(std::sync::atomic::AtomicU64::new(0)));
        publication.store(frame.render_revision, std::sync::atomic::Ordering::Release);
        Ok(Self {
            frame: Arc::new(frame),
            authority,
            epoch,
            publication,
        })
    }
    pub fn frame_arc(&self) -> &Arc<RenderFrame> {
        &self.frame
    }
    pub fn is_current(&self) -> bool {
        self.authority.state().is_ok_and(|state| {
            state.open
                && state.in_sync
                && state.version == self.frame.document_version
                && state.source_epoch == self.epoch
        }) && self.publication.load(std::sync::atomic::Ordering::Acquire)
            == self.frame.render_revision
    }
    pub(crate) fn publication(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.publication)
    }
    pub(crate) fn invalidate_publication(&self) {
        self.publication
            .store(0, std::sync::atomic::Ordering::Release);
    }
}
impl std::ops::Deref for PreviewHandle {
    type Target = RenderFrame;
    fn deref(&self) -> &Self::Target {
        &self.frame
    }
}
impl PartialEq for PreviewHandle {
    fn eq(&self, other: &Self) -> bool {
        self.epoch == other.epoch
            && Arc::ptr_eq(&self.authority, &other.authority)
            && self.frame == other.frame
    }
}
impl Eq for PreviewHandle {}
