use std::borrow::Borrow;
use std::collections::HashMap;

use crate::DocumentSession;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DocumentKey(String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SessionKey(String);

impl Borrow<str> for DocumentKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for SessionKey {
    fn borrow(&self) -> &str {
        &self.0
    }
}

#[derive(Default)]
pub(super) struct SessionIndex {
    documents: HashMap<DocumentKey, DocumentSession>,
    session_uris: HashMap<SessionKey, DocumentKey>,
}

impl SessionIndex {
    pub(super) fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    pub(super) fn insert(&mut self, session: DocumentSession) {
        let uri = DocumentKey(session.uri.clone());
        let session_id = SessionKey(session.id.clone());
        if let Some(previous) = self.documents.remove(&uri) {
            self.session_uris.remove(previous.id.as_str());
        }
        if let Some(previous_uri) = self.session_uris.insert(session_id, uri.clone()) {
            self.documents.remove(&previous_uri);
        }
        self.documents.insert(uri, session);
    }

    pub(super) fn remove_by_uri(&mut self, uri: &str) -> Option<DocumentSession> {
        let session = self.documents.remove(uri)?;
        self.session_uris.remove(session.id.as_str());
        Some(session)
    }

    pub(super) fn by_uri(&self, uri: &str) -> Option<&DocumentSession> {
        self.documents.get(uri)
    }

    pub(super) fn by_uri_mut(&mut self, uri: &str) -> Option<&mut DocumentSession> {
        self.documents.get_mut(uri)
    }

    pub(super) fn by_session(&self, session_id: &str) -> Option<&DocumentSession> {
        let uri = self.session_uris.get(session_id)?;
        self.documents.get(uri)
    }

    pub(super) fn by_session_mut(&mut self, session_id: &str) -> Option<&mut DocumentSession> {
        let uri = self.session_uris.get(session_id)?;
        self.documents.get_mut(uri)
    }

    pub(super) fn uri_for_session(&self, session_id: &str) -> Option<&str> {
        self.session_uris.get(session_id).map(|uri| uri.0.as_str())
    }

    pub(super) fn session_id_for_uri(&self, uri: &str) -> Option<&str> {
        self.by_uri(uri).map(|session| session.id.as_str())
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, &DocumentSession)> {
        self.documents
            .iter()
            .map(|(uri, session)| (uri.0.as_str(), session))
    }
}
