//! Role: Validate LSP full-document notifications before native store updates.
use super::state::*;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentParams {
    text_document: DocumentItem,
}
#[derive(Deserialize)]
struct DocumentItem {
    uri: String,
    version: i64,
    text: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangeParams {
    text_document: VersionedDocument,
    content_changes: Vec<ContentChange>,
}
#[derive(Deserialize)]
struct VersionedDocument {
    uri: String,
    version: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContentChange {
    text: String,
    #[serde(default, deserialize_with = "present_value")]
    range: Option<Value>,
    #[serde(default, deserialize_with = "present_value")]
    range_length: Option<Value>,
}
fn present_value<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}
impl Server {
    fn notification_id(id: Option<Value>) -> Option<Value> {
        id.map(|id| {
            super::wire::response_value(Response::error(
                id,
                fleximark_protocol::error_code::INVALID_REQUEST,
                "LSP notification must not have an id",
            ))
        })
    }
    pub(super) fn open_lsp_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return Self::notification_id(id);
        }
        let Ok(params) = serde_json::from_value::<DocumentParams>(params) else {
            return None;
        };
        let item = params.text_document;
        let Ok(version) = u64::try_from(item.version) else {
            self.registry.mark_out_of_sync(&item.uri);
            return None;
        };
        if fleximark_protocol::JsSafeI64::new(item.version).is_err() {
            self.registry.mark_out_of_sync(&item.uri);
            return None;
        }
        let uri = item.uri.clone();
        let result = self
            .registry
            .open(
                fleximark_application::DocumentUpdate {
                    uri: item.uri,
                    version,
                    source: item.text,
                },
                &self.work_cancellation,
            )
            .and_then(|_| self.refresh_assets(&uri));
        if let Err(error) = result {
            crate::telemetry::log_operational_failure(
                "document-open-failed",
                &error,
                Some(&uri),
                None,
                None,
            );
        } else {
            self.publish_lsp_diagnostics(&uri);
        }
        None
    }
    pub(super) fn change_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return Self::notification_id(id);
        }
        let attempted_version = params
            .pointer("/textDocument/version")
            .and_then(Value::as_u64);
        let uri = params
            .pointer("/textDocument/uri")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let result = (|| {
            let params: ChangeParams = serde_json::from_value(params)?;
            let version = u64::try_from(params.text_document.version)
                .map_err(|_| ApplicationError::VersionMismatch)?;
            fleximark_protocol::JsSafeI64::new(params.text_document.version)
                .map_err(|_| ApplicationError::VersionMismatch)?;
            if params.content_changes.len() != 1 {
                return Err(ApplicationError::InvalidRange);
            }
            let change = params
                .content_changes
                .into_iter()
                .next()
                .ok_or(ApplicationError::EmptyChange)?;
            if change.range.is_some() || change.range_length.is_some() {
                return Err(ApplicationError::InvalidRange);
            }
            self.registry.change(
                fleximark_application::DocumentUpdate {
                    uri: params.text_document.uri,
                    version,
                    source: change.text,
                },
                &self.work_cancellation,
            )
        })();
        let uri = uri?;
        if let Err(error) = result {
            self.registry.mark_out_of_sync(&uri);
            if let (Some(session), Some(version)) = (
                self.registry.session_id_for_uri(&uri).map(str::to_owned),
                attempted_version.and_then(|version| super::protocol::counter(version).ok()),
            ) {
                self.mark_preview_document_version(&session, version);
            }
            crate::telemetry::log_operational_failure(
                "document-sync-failed",
                &error,
                Some(&uri),
                None,
                None,
            );
            return None;
        }
        let session = self.registry.session_id_for_uri(&uri).map(str::to_owned);
        if let Some(session) = session {
            if let Ok(version) = self
                .registry
                .current_version(&session)
                .and_then(super::protocol::counter)
            {
                self.mark_preview_document_version(&session, version);
            }
        }
        if self.publication_cancellation.is_cancelled() {
            return None;
        }
        let trace = OperationalTrace::new(Some(&uri), None, None);
        if self.refresh_changed_document_assets(&uri, &trace) {
            self.publish_changed_document(&uri);
        }
        None
    }
    pub(super) fn close_document(&mut self, id: Option<Value>, params: Value) -> Option<Value> {
        if id.is_some() {
            return Self::notification_id(id);
        }
        let uri = params
            .pointer("/textDocument/uri")
            .and_then(Value::as_str)?;
        if let Some(session) = self.registry.session_id_for_uri(uri).map(str::to_owned) {
            let expired = self
                .preview_states
                .iter()
                .filter(|(_, state)| state.document_session_id == session)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            for preview in expired {
                if let Some(state) = self.preview_states.remove(&preview) {
                    let _ = self.registry.dispose_preview(&session, &preview);
                    if let Some(token) = state.token {
                        self.previews.remove(&token);
                    }
                }
            }
        }
        self.registry.close(uri);
        self.outgoing_events.push(super::wire::notification_value(
            "textDocument/publishDiagnostics",
            super::lsp_types::PublishDiagnostics {
                uri,
                version: None,
                diagnostics: vec![],
            },
        ));
        None
    }
}
