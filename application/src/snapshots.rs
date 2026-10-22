//! Role: Frame core snapshots with application-owned preview identities and revisions.
use fleximark_engine::{RenderAsset, RenderBlock, RenderSnapshot, RenderStyle};
use fleximark_model::NavigationEntry;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PreviewSessionId(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderFrame {
    pub preview_session_id: PreviewSessionId,
    pub document_version: u64,
    pub render_revision: u64,
    pub renderer_fingerprint: String,
    pub style: Option<RenderStyle>,
    pub assets: Vec<RenderAsset>,
    pub blocks: Vec<RenderBlock>,
    pub navigation: Vec<NavigationEntry>,
    pub annotations: BTreeMap<String, String>,
}

impl RenderFrame {
    pub fn from_snapshot(id: PreviewSessionId, revision: u64, snapshot: RenderSnapshot) -> Self {
        Self {
            preview_session_id: id,
            render_revision: revision,
            document_version: snapshot.document_version,
            renderer_fingerprint: snapshot.renderer_fingerprint,
            style: snapshot.style,
            assets: snapshot.assets,
            blocks: snapshot.blocks,
            navigation: snapshot.navigation,
            annotations: snapshot.annotations,
        }
    }

    pub fn html(&self) -> String {
        RenderSnapshot {
            document_version: self.document_version,
            renderer_fingerprint: self.renderer_fingerprint.clone(),
            style: self.style.clone(),
            assets: self.assets.clone(),
            blocks: self.blocks.clone(),
            navigation: self.navigation.clone(),
            annotations: self.annotations.clone(),
        }
        .html()
    }
}
