use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct ConcurrencyLimiter {
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
}

impl ConcurrencyLimiter {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_concurrent,
        }
    }

    pub async fn acquire(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.semaphore.acquire().await.unwrap()
    }

    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }
}

static GLOBAL_LIMITER: RwLock<Option<ConcurrencyLimiter>> = RwLock::new(None);

pub fn init_global_limiter(max_concurrent: usize) {
    *GLOBAL_LIMITER.write() = Some(ConcurrencyLimiter::new(max_concurrent));
}

pub fn get_global_limiter() -> Option<ConcurrencyLimiter> {
    GLOBAL_LIMITER.read().clone()
}