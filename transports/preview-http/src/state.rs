//! Role: Hold authenticated browser delivery state and injected resources.
use crate::assets::BrowserAssets;
use fleximark_application::RenderFrame;
use fleximark_protocol::{
    CommandResult, JsSafeU64, WorkspaceStateRequest, WorkspaceStateResult,
    WorkspaceUiCommandRequest,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
#[derive(Clone, Debug)]
pub enum WorkspaceRequest {
    State(WorkspaceStateRequest),
    Command(WorkspaceUiCommandRequest),
}
#[derive(Clone, Debug)]
pub enum WorkspaceResponse {
    State(WorkspaceStateResult),
    Command(CommandResult),
}
pub(crate) type Handler = Arc<
    dyn Fn(WorkspaceRequest) -> Result<WorkspaceResponse, fleximark_application::ApplicationError>
        + Send
        + Sync,
>;
#[derive(Clone)]
pub(crate) struct WorkspaceService {
    pub(crate) assets: BrowserAssets,
    pub(crate) sessions: Arc<Mutex<HashMap<String, WorkspaceStateRequest>>>,
    pub(crate) handler: Arc<Mutex<Option<Handler>>>,
}
pub struct PreviewServer {
    pub(crate) assets: BrowserAssets,
    pub(crate) workspace: WorkspaceService,
    pub(crate) port: u16,
    pub pages: Arc<Mutex<HashMap<String, PreviewPage>>>,
}

#[derive(Clone)]
pub struct PreviewPage {
    pub shell: String,
    pub daemon_instance_id: String,
    pub preview_session_id: String,
    pub frame: RenderFrame,
    pub change_event: StoredEvent,
    pub navigation_event: Option<StoredEvent>,
    pub durable_navigation: Option<StoredEvent>,
    pub next_sequence: u64,
    pub document_version: JsSafeU64,
    pub last_browser_event: Option<Instant>,
}

#[derive(Clone)]
pub struct StoredEvent {
    pub encoded: String,
    pub sequence: u64,
}
