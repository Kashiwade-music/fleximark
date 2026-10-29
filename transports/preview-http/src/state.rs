//! Role: Hold authenticated browser delivery state and injected resources.
use crate::assets::BrowserAssets;
use crate::delivery::PreviewPage;
use fleximark_protocol::{
    CommandResult, WorkspaceStateRequest, WorkspaceStateResult, WorkspaceUiCommandRequest,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
    pub(crate) streams: Arc<crate::streaming::StreamState>,
    pub pages: Arc<Mutex<HashMap<String, PreviewPage>>>,
}
