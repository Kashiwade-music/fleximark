use std::collections::HashMap;
use std::sync::Mutex;

use fleximark_plugin_host::CancellationToken;
use fleximark_protocol::{IncomingMessage, method};
use serde_json::Value;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum DocumentKey {
    Uri(String),
    Session(String),
}

pub(crate) struct WorkPermit {
    work_id: u64,
    request_id: Option<String>,
    document: Option<DocumentKey>,
    generation: u64,
    is_mutation: bool,
    pub(crate) token: CancellationToken,
    pub(crate) publication_token: CancellationToken,
}

#[derive(Default)]
pub(crate) struct CancellationCoordinator {
    state: Mutex<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    next_work_id: u64,
    generations: HashMap<DocumentKey, u64>,
    session_uris: HashMap<String, String>,
    work: HashMap<u64, (Option<DocumentKey>, CancellationToken, CancellationToken)>,
    requests: HashMap<String, (u64, CancellationToken, CancellationToken)>,
}

impl CancellationCoordinator {
    pub(crate) fn prepare(&self, message: &IncomingMessage) -> Option<WorkPermit> {
        let mut state = self.state.lock().expect("cancellation state poisoned");
        if message.method == "$/cancelRequest" {
            if let Some(id) = message.params.get("id").and_then(rpc_id_key)
                && let Some((_, token, publication_token)) = state.requests.get(&id)
            {
                token.cancel();
                publication_token.cancel();
            }
            return None;
        }

        let document = document_key(message);
        let is_mutation = is_document_mutation(&message.method);
        if is_mutation && let Some(document) = document.as_ref() {
            let canonical = canonical_document(&state, document);
            let generation = state.generations.entry(canonical.clone()).or_default();
            *generation += 1;
            for (active_document, _, publication_token) in state.work.values() {
                if active_document
                    .as_ref()
                    .is_some_and(|active| canonical_document(&state, active) == canonical)
                {
                    publication_token.cancel();
                }
            }
        }

        state.next_work_id += 1;
        let work_id = state.next_work_id;
        let generation = document
            .as_ref()
            .map(|document| {
                let canonical = canonical_document(&state, document);
                state.generations.get(&canonical).copied().unwrap_or(0)
            })
            .unwrap_or(0);
        let token = CancellationToken::default();
        let publication_token = if is_mutation {
            CancellationToken::default()
        } else {
            token.clone()
        };
        let request_id = message.id.as_ref().and_then(rpc_id_key);
        state.work.insert(
            work_id,
            (document.clone(), token.clone(), publication_token.clone()),
        );
        if let Some(request_id) = &request_id {
            state.requests.insert(
                request_id.clone(),
                (work_id, token.clone(), publication_token.clone()),
            );
        }
        Some(WorkPermit {
            work_id,
            request_id,
            document,
            generation,
            is_mutation,
            token,
            publication_token,
        })
    }

    pub(crate) fn should_execute(&self, permit: &WorkPermit) -> bool {
        !permit.token.is_cancelled() && (permit.is_mutation || self.is_current(permit))
    }

    pub(crate) fn is_current(&self, permit: &WorkPermit) -> bool {
        if permit.publication_token.is_cancelled() {
            return false;
        }
        let state = self.state.lock().expect("cancellation state poisoned");
        permit.document.as_ref().is_none_or(|document| {
            let canonical = canonical_document(&state, document);
            state.generations.get(&canonical).copied().unwrap_or(0) == permit.generation
        })
    }

    pub(crate) fn finish(&self, permit: &WorkPermit) {
        let mut state = self.state.lock().expect("cancellation state poisoned");
        state.work.remove(&permit.work_id);
        if let Some(request_id) = &permit.request_id
            && state
                .requests
                .get(request_id)
                .is_some_and(|(work_id, _, _)| *work_id == permit.work_id)
        {
            state.requests.remove(request_id);
        }
    }

    pub(crate) fn bind(&self, session_id: &str, uri: &str) {
        let mut state = self.state.lock().expect("cancellation state poisoned");
        state
            .session_uris
            .insert(session_id.to_owned(), uri.to_owned());
        let session_key = DocumentKey::Session(session_id.to_owned());
        let uri_key = DocumentKey::Uri(uri.to_owned());
        let generation = state
            .generations
            .get(&session_key)
            .copied()
            .unwrap_or(0)
            .max(state.generations.get(&uri_key).copied().unwrap_or(0));
        state.generations.insert(uri_key, generation);
        state.generations.remove(&session_key);
    }
}

fn canonical_document(state: &CancellationState, document: &DocumentKey) -> DocumentKey {
    match document {
        DocumentKey::Session(session_id) => state
            .session_uris
            .get(session_id)
            .cloned()
            .map(DocumentKey::Uri)
            .unwrap_or_else(|| document.clone()),
        DocumentKey::Uri(_) => document.clone(),
    }
}

fn rpc_id_key(value: &Value) -> Option<String> {
    match value {
        Value::String(_) | Value::Number(_) => serde_json::to_string(value).ok(),
        _ => None,
    }
}

fn document_key(message: &IncomingMessage) -> Option<DocumentKey> {
    message
        .params
        .pointer("/textDocument/uri")
        .or_else(|| message.params.get("uri"))
        .and_then(Value::as_str)
        .map(|uri| DocumentKey::Uri(uri.to_owned()))
        .or_else(|| {
            message
                .params
                .get("documentSessionId")
                .and_then(Value::as_str)
                .map(|session| DocumentKey::Session(session.to_owned()))
        })
}

fn is_document_mutation(method_name: &str) -> bool {
    matches!(
        method_name,
        "textDocument/didOpen"
            | "textDocument/didChange"
            | "textDocument/didClose"
            | method::OPEN_DOCUMENT
            | method::CHANGE_DOCUMENT
            | method::CLOSE_DOCUMENT
    )
}
