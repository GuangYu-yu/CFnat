use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

#[derive(Clone, Debug)]
pub struct CancellationToken {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    cancelled: AtomicBool,
    notifier: Notify,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                cancelled: AtomicBool::new(false),
                notifier: Notify::new(),
            }),
        }
    }

    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notifier.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        let wait = self.inner.notifier.notified();
        if self.inner.cancelled.load(Ordering::Acquire) {
            return;
        }
        wait.await;
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}