//! Role: Hold authoritative document state and application-owned preview snapshots.
use crate::snapshots::RenderFrame;
use fleximark_engine::DocumentEngine as EngineSession;
use fleximark_model::{NavigationEntry, PositionEncoding};
pub struct DocumentSession {
    pub id: String,
    pub uri: String,
    pub content_hash: String,
    pub(crate) workspace_uri: Option<String>,
    pub(crate) engine: EngineSession,
    pub(crate) previews: std::collections::HashMap<String, RenderFrame>,
}

impl DocumentSession {
    pub fn engine(&self) -> &EngineSession {
        &self.engine
    }

    pub fn document(&self) -> &fleximark_model::Document {
        self.engine.document()
    }

    pub fn source(&self) -> &str {
        self.engine.source()
    }

    pub fn node_at_source_offset(&self, byte_offset: u64) -> Option<NavigationEntry> {
        self.engine.node_at_source_offset(byte_offset)
    }

    pub fn position_encoding(&self) -> PositionEncoding {
        self.engine.position_encoding()
    }

    pub fn workspace_uri(&self) -> Option<&str> {
        self.workspace_uri.as_deref()
    }

    pub fn asset_diagnostics(&self) -> &[fleximark_engine::AssetDiagnostic] {
        self.engine.asset_diagnostics()
    }

    pub fn plugin_diagnostics(&self) -> &[fleximark_plugin_api::PluginDiagnostic] {
        self.engine.plugin_diagnostics()
    }
}
