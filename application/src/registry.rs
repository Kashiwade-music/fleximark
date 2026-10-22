//! Role: Hold native document and workspace authority state.
use crate::index::SessionIndex;
use crate::workspace::WorkspaceAuthority;
use fleximark_model::PositionEncoding;
pub struct SessionRegistry {
    pub(super) config_generation: u64,
    pub(super) daemon_instance_id: String,
    pub(super) position_encoding: PositionEncoding,
    pub(super) index: SessionIndex,
    pub(super) workspaces: WorkspaceAuthority,
}
