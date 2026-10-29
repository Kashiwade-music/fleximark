//! Role: Invalidate published document capabilities when authoritative state changes.
use crate::ApplicationError;
use std::sync::RwLock;
#[derive(Clone, Copy, Debug)]
pub(crate) struct DocumentAuthorityState {
    pub version: u64,
    pub source_epoch: u64,
    pub in_sync: bool,
    pub open: bool,
}
#[derive(Debug)]
pub(crate) struct DocumentAuthority(RwLock<DocumentAuthorityState>);
impl DocumentAuthority {
    pub(crate) fn new(version: u64) -> Self {
        Self(RwLock::new(DocumentAuthorityState {
            version,
            source_epoch: 0,
            in_sync: true,
            open: true,
        }))
    }
    pub(crate) fn state(&self) -> Result<DocumentAuthorityState, ApplicationError> {
        self.0.read().map(|state| *state).map_err(|_| {
            ApplicationError::InternalState("document authority lock is poisoned".into())
        })
    }
    pub(crate) fn check_advance(&self) -> Result<(), ApplicationError> {
        self.state()?
            .source_epoch
            .checked_add(1)
            .map(|_| ())
            .ok_or(ApplicationError::RevisionExhausted)
    }
    #[cfg(test)]
    pub(crate) fn exhaust_epoch(&self) {
        self.0.write().unwrap().source_epoch = u64::MAX;
    }
    pub(crate) fn commit(&self, version: u64) -> Result<(), ApplicationError> {
        let mut state = self.0.write().map_err(|_| {
            ApplicationError::InternalState("document authority lock is poisoned".into())
        })?;
        let epoch = state
            .source_epoch
            .checked_add(1)
            .ok_or(ApplicationError::RevisionExhausted)?;
        *state = DocumentAuthorityState {
            version,
            source_epoch: epoch,
            in_sync: true,
            open: true,
        };
        Ok(())
    }
    pub(crate) fn invalidate(&self) {
        if let Ok(mut state) = self.0.write() {
            state.in_sync = false;
        }
    }
    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.0.write() {
            state.open = false;
            state.in_sync = false;
        }
    }
}
