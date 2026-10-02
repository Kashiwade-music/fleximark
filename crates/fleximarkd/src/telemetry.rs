use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::preview_http::random_token;

pub(crate) struct OperationalTrace {
    correlation_id: String,
    uri_hash: Option<String>,
    session_id: Option<String>,
    document_version: Option<i64>,
    started: Instant,
}

impl OperationalTrace {
    pub(crate) fn new(
        uri: Option<&str>,
        session_id: Option<&str>,
        document_version: Option<i64>,
    ) -> Self {
        Self {
            correlation_id: random_token().unwrap_or_else(|_| "rng-unavailable".into()),
            uri_hash: uri.map(|uri| format!("{:x}", Sha256::digest(uri.as_bytes()))),
            session_id: session_id.map(str::to_owned),
            document_version,
            started: Instant::now(),
        }
    }

    pub(crate) fn event(
        &self,
        stage: &str,
        stage_duration: Duration,
        render_revision: Option<u64>,
        plugin_failure: bool,
        recovery: bool,
    ) -> Value {
        json!({
            "event":"fleximark.operation",
            "stage":stage,
            "correlationId":self.correlation_id,
            "uriHash":self.uri_hash,
            "documentSessionId":self.session_id,
            "documentVersion":self.document_version,
            "renderRevision":render_revision,
            "elapsedMs":self.started.elapsed().as_secs_f64() * 1_000.0,
            "stageDurationMs":stage_duration.as_secs_f64() * 1_000.0,
            "pluginFailure":plugin_failure,
            "restart":false,
            "recovery":recovery
        })
    }

    pub(crate) fn log(&self, stage: &str) {
        eprintln!("{}", self.event(stage, Duration::ZERO, None, false, false));
    }
}

pub(crate) fn log_operational_event(
    event: &str,
    uri: Option<&str>,
    session_id: Option<&str>,
    document_version: Option<i64>,
) {
    OperationalTrace::new(uri, session_id, document_version).log(event);
}
