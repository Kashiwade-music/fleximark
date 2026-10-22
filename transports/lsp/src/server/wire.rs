//! Role: Encode JSON-RPC responses and notifications for the current contract.
use fleximark_application::SessionError;
use fleximark_protocol::{
    CONTENT_MODIFIED, Notification, PreviewChangedParams, Response, ServerPreviewEventParams,
    method,
};
use serde_json::{Value, json};

pub(super) fn boundary_error(id: Value, error: impl std::fmt::Display) -> Value {
    response_value(Response::error(id, -32603, error.to_string()))
}
pub(super) fn invalid_params(id: Value, error: impl std::fmt::Display) -> Value {
    response_value(Response::error(
        id,
        -32602,
        format!("invalid params: {error}"),
    ))
}

pub(super) fn deserialize_params<P: serde::de::DeserializeOwned>(
    id: &Value,
    params: Value,
) -> Result<P, Value> {
    serde_json::from_value(params).map_err(|error| invalid_params(id.clone(), error))
}

pub(crate) fn session_error(id: Value, error: SessionError) -> Value {
    let engine_rejection = matches!(error, SessionError::Engine(_));
    let code = if matches!(
        error,
        SessionError::ContentModified | SessionError::StaleVersion | SessionError::VersionMismatch
    ) {
        CONTENT_MODIFIED
    } else {
        -32602
    };
    let mut response = Response::error(id, code, error.to_string());
    if engine_rejection {
        response.error.as_mut().expect("error response").data = Some(json!({"kind":"engine"}));
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
