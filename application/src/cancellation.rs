//! Role: Share cooperative cancellation across request and workspace lifetimes.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Clone, Default)]
pub struct CancellationToken {
    flag: Arc<AtomicBool>,
    parents: Arc<[CancellationToken]>,
}
impl CancellationToken {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire) || self.parents.iter().any(Self::is_cancelled)
    }
    pub(crate) fn linked(request: &Self, scope: &Self) -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            parents: Arc::from([request.clone(), scope.clone()]),
        }
    }
    pub(crate) fn same_scope(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.flag, &other.flag)
    }
}
