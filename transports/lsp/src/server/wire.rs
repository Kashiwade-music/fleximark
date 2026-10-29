//! Role: Encode JSON-RPC responses and notifications for the current contract.
use fleximark_application::ApplicationError;
use fleximark_protocol::{
    Notification, PreviewChangedParams, Response, ServerPreviewEventParams, method,
};
use serde_json::{Value, json};

pub(super) fn boundary_error(id: Value, error: ApplicationError) -> Value {
    session_error(id, error)
}
pub(super) fn invalid_params(id: Value, error: impl std::fmt::Display) -> Value {
    response_value(Response::error(
        id,
        fleximark_protocol::error_code::INVALID_PARAMS,
        format!("invalid params: {error}"),
    ))
}

pub(super) fn deserialize_params<P: serde::de::DeserializeOwned>(
    id: &Value,
    params: Value,
) -> Result<P, Value> {
    serde_json::from_value(params).map_err(|error| invalid_params(id.clone(), error))
}

pub(crate) fn application_error_code(kind: fleximark_application::ErrorKind) -> i64 {
    use fleximark_application::ErrorKind;
    use fleximark_protocol::error_code as code;
    match kind {
        ErrorKind::NotFound | ErrorKind::Invalid => code::INVALID_PARAMS,
        ErrorKind::Denied => code::WRITE_DENIED,
        ErrorKind::Conflict => code::CONTENT_MODIFIED,
        ErrorKind::Cancelled => code::REQUEST_CANCELLED,
        ErrorKind::Internal => code::INTERNAL_ERROR,
    }
}

pub(crate) fn session_error(id: Value, error: ApplicationError) -> Value {
    let mut response = Response::error(id, application_error_code(error.kind()), error.to_string());
    if matches!(error, ApplicationError::Engine(_)) {
        if let Some(error) = response.error.as_mut() {
            error.data = Some(json!({"kind":"engine"}));
        }
    }
    response_value(response)
}

pub(crate) fn response_value(response: Response) -> Value {
    serde_json::to_value(response).expect("response is serializable")
}

pub(crate) fn preview_event_notification(params: ServerPreviewEventParams) -> Value {
    serde_json::to_value(Notification {
        jsonrpc: "2.0",
        method: method::PREVIEW_EVENT,
        params,
    })
    .expect("preview event notification is serializable")
}

pub(crate) fn preview_changed_notification(params: PreviewChangedParams) -> Value {
    serde_json::to_value(Notification {
        jsonrpc: "2.0",
        method: method::PREVIEW_CHANGED,
        params,
    })
    .expect("preview changed notification is serializable")
}
