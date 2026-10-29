//! Role: Record correlated operations and redacted failures across transports.
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn random_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; 24];
    getrandom::getrandom(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub struct OperationalTrace {
    correlation_id: String,
    uri_hash: Option<String>,
    session_id: Option<String>,
    document_version: Option<i64>,
    started: Instant,
}

impl OperationalTrace {
    pub fn new(uri: Option<&str>, session_id: Option<&str>, document_version: Option<i64>) -> Self {
        Self {
            correlation_id: random_token().unwrap_or_else(|_| "rng-unavailable".into()),
            uri_hash: uri.map(|uri| format!("{:x}", Sha256::digest(uri.as_bytes()))),
            session_id: session_id.map(str::to_owned),
            document_version,
            started: Instant::now(),
        }
    }

    pub fn event(
        &self,
        stage: &str,
        stage_duration: Duration,
        render_revision: Option<u64>,
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
            "restart":false,
            "recovery":recovery
        })
    }

    pub fn failure_event<E: std::error::Error + 'static>(&self, stage: &str, error: &E) -> Value {
        let mut event = self.event(stage, Duration::ZERO, None, false);
        event["errorType"] = json!(std::any::type_name::<E>());
        event["errorHash"] = json!(format!(
            "{:x}",
            Sha256::digest(error.to_string().as_bytes())
        ));
        let cause: &(dyn std::error::Error + 'static) = error;
        let kind = cause
            .downcast_ref::<crate::ApplicationError>()
            .map_or(crate::ErrorKind::Internal, crate::ApplicationError::kind);
        event["errorKind"] = json!(format!("{kind:?}"));
        event
    }

    pub fn failure<E: std::error::Error + 'static>(&self, stage: &str, error: &E) {
        eprintln!("{}", self.failure_event(stage, error));
    }

    pub fn log(&self, stage: &str) {
        eprintln!("{}", self.event(stage, Duration::ZERO, None, false));
    }
}

pub fn log_operational_event(
    event: &str,
    uri: Option<&str>,
    session_id: Option<&str>,
    document_version: Option<i64>,
) {
    OperationalTrace::new(uri, session_id, document_version).log(event);
}

pub fn log_operational_failure<E: std::error::Error + 'static>(
    event: &str,
    error: &E,
    uri: Option<&str>,
    session_id: Option<&str>,
    document_version: Option<i64>,
) {
    OperationalTrace::new(uri, session_id, document_version).failure(event, error);
}

#[cfg(test)]
mod tests {
    //! Role: Verify failure metadata never exposes document or credential contents.
    use super::*;
    #[test]
    fn failure_event_hashes_private_details_and_keeps_error_classification() {
        let uri = "file:///private/document.md";
        let error = crate::ApplicationError::InvalidState("private source and token=secret".into());
        let event = OperationalTrace::new(Some(uri), Some("session-1"), Some(2))
            .failure_event("sync-failed", &error);
        assert_eq!(event["errorKind"], "Invalid");
        assert_eq!(event["errorHash"].as_str().unwrap().len(), 64);
        assert_eq!(event["uriHash"].as_str().unwrap().len(), 64);
        assert!(
            event["errorType"]
                .as_str()
                .unwrap()
                .contains("ApplicationError")
        );
        let encoded = event.to_string();
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains(uri));
    }
}
