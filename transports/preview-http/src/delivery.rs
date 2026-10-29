//! Role: Describe shared browser frames and ordered navigation publications.
use fleximark_application::PreviewHandle;
use std::time::Instant;

#[derive(Clone)]
pub struct PreviewPage {
    pub shell: String,
    pub daemon_instance_id: String,
    pub preview_session_id: String,
    pub frame: PreviewHandle,
    pub frame_sequence: u64,
    pub navigation_event: Option<StoredEvent>,
    pub durable_navigation: Option<StoredEvent>,
    pub next_sequence: u64,
    pub last_browser_event: Option<Instant>,
}

#[derive(Clone)]
pub struct StoredEvent {
    pub encoded: String,
    pub sequence: u64,
}
