//! Role: Check cancellation before application state is committed.
use crate::CancellationToken;
use crate::SessionError;

pub(crate) fn check(token: &CancellationToken, _stage: &'static str) -> Result<(), SessionError> {
    #[cfg(test)]
    checkpoint(_stage);
    if token.is_cancelled() {
        Err(SessionError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn checkpoint(stage: &'static str) {
    let thread = std::thread::current().id();
    let mut pending = CANCEL_AT.lock().unwrap();
    if let Some(index) = pending
        .iter()
        .position(|(owner, target, _)| *owner == thread && *target == stage)
    {
        let (_, _, token) = pending.remove(index);
        token.cancel();
    }
}

#[cfg(test)]
static CANCEL_AT: std::sync::Mutex<Vec<(std::thread::ThreadId, &'static str, CancellationToken)>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub(crate) fn cancel_at(stage: &'static str, token: &CancellationToken) {
    let thread = std::thread::current().id();
    let mut pending = CANCEL_AT.lock().unwrap();
    pending.retain(|(owner, _, _)| *owner != thread);
    pending.push((thread, stage, token.clone()));
}
