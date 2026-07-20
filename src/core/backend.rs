use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::time::Instant;

use parking_lot::Mutex;

use crate::core::config::get_global_config;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum BackendState {
    Warming = 0,
    Active = 1,
    Isolated = 2,
    Removed = 3,
}

#[derive(Debug)]
pub struct Backend {
    pub addr: SocketAddr,
    pub colo: Mutex<Option<String>>,
    connections: AtomicUsize,
    avg_delay: AtomicU32,
    avg_loss: AtomicU32,
    sample_count: AtomicUsize,
    state: AtomicU8,
    entered_state_at: Mutex<Instant>,
    consecutive_failures: AtomicU32,
    fast_fail_count: AtomicU32,
}

impl Backend {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            colo: Mutex::new(None),
            connections: AtomicUsize::new(0),
            avg_delay: AtomicU32::new((-1.0_f32).to_bits()),
            avg_loss: AtomicU32::new((-1.0_f32).to_bits()),
            sample_count: AtomicUsize::new(0),
            state: AtomicU8::new(BackendState::Warming as u8),
            entered_state_at: Mutex::new(Instant::now()),
            consecutive_failures: AtomicU32::new(0),
            fast_fail_count: AtomicU32::new(0),
        }
    }

    pub fn new_with_initial(addr: SocketAddr, initial_delay: f32, initial_loss: f32, colo: Option<String>) -> Self {
        Self {
            addr,
            colo: Mutex::new(colo),
            connections: AtomicUsize::new(0),
            avg_delay: AtomicU32::new(initial_delay.to_bits()),
            avg_loss: AtomicU32::new(initial_loss.to_bits()),
            sample_count: AtomicUsize::new(1),
            state: AtomicU8::new(BackendState::Warming as u8),
            entered_state_at: Mutex::new(Instant::now()),
            consecutive_failures: AtomicU32::new(0),
            fast_fail_count: AtomicU32::new(0),
        }
    }

    pub fn set_colo(&self, colo: Option<String>) {
        *self.colo.lock() = colo;
    }

    pub fn get_colo(&self) -> Option<String> {
        self.colo.lock().clone()
    }

    pub fn record(&self, delay_ms: Option<f32>, is_loss: bool) {
        let is_first = self.sample_count.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            Some(if count == 0 { 1 } else { (count + 1).min(get_global_config().sample_window as usize) })
        }).map(|old| old == 0).unwrap();

        let alpha = get_global_config().alpha;
        if let Some(delay) = delay_ms {
            let _ = self.avg_delay.fetch_update(Ordering::AcqRel, Ordering::Acquire, |bits| {
                let current = f32::from_bits(bits);
                let new_val = if is_first { delay } else { (current * (1.0 - alpha)) + (delay * alpha) };
                Some(new_val.to_bits())
            });
        }

        let loss = if is_loss { 1.0 } else { 0.0 };
        let _ = self.avg_loss.fetch_update(Ordering::AcqRel, Ordering::Acquire, |bits| {
            let current = f32::from_bits(bits);
            let new_val = if is_first { loss } else { (current * (1.0 - alpha)) + (loss * alpha) };
            Some(new_val.to_bits())
        });
    }

    pub fn get_avg_delay(&self) -> f32 {
        f32::from_bits(self.avg_delay.load(Ordering::Acquire)).max(0.0)
    }

    pub fn get_loss_rate(&self) -> f32 {
        f32::from_bits(self.avg_loss.load(Ordering::Acquire)).max(0.0)
    }

    pub fn get_sample_count(&self) -> usize {
        self.sample_count.load(Ordering::Relaxed)
    }

    pub fn is_removed(&self) -> bool {
        self.state.load(Ordering::Relaxed) == BackendState::Removed as u8
    }

    pub fn is_warming(&self) -> bool {
        self.state.load(Ordering::Relaxed) == BackendState::Warming as u8
    }

    pub fn is_active(&self) -> bool {
        self.state.load(Ordering::Relaxed) == BackendState::Active as u8
    }

    pub fn is_isolated(&self) -> bool {
        self.state.load(Ordering::Relaxed) == BackendState::Isolated as u8
    }

    pub fn is_selectable(&self) -> bool {
        match self.state.load(Ordering::Relaxed) {
            s if s == BackendState::Active as u8 => true,
            s if s == BackendState::Warming as u8 => self.sample_count.load(Ordering::Relaxed) > 0,
            _ => false,
        }
    }

    pub fn mark_removed(&self) {
        self.state.store(BackendState::Removed as u8, Ordering::Relaxed);
        *self.entered_state_at.lock() = Instant::now();
    }

    pub fn mark_active(&self) {
        self.state.store(BackendState::Active as u8, Ordering::Relaxed);
        *self.entered_state_at.lock() = Instant::now();
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    pub fn mark_isolated(&self) {
        self.state.store(BackendState::Isolated as u8, Ordering::Relaxed);
        *self.entered_state_at.lock() = Instant::now();
    }

    pub fn reset_metrics(&self) {
        self.sample_count.store(0, Ordering::Relaxed);
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    pub fn record_failure(&self) {
        self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }

    pub fn record_fast_fail(&self) {
        self.fast_fail_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reset_fast_fail(&self) {
        self.fast_fail_count.store(0, Ordering::Relaxed);
    }

    pub fn get_fast_fail_count(&self) -> u32 {
        self.fast_fail_count.load(Ordering::Relaxed)
    }

    pub fn check_warming_expired(&self) -> bool {
        if self.is_warming() {
            let elapsed = self.entered_state_at.lock().elapsed().as_secs();
            elapsed >= get_global_config().warming_duration.as_secs()
        } else {
            false
        }
    }

    /// 延迟差值超过此阈值才认为有显著差异（毫秒）
    const DELAY_COMPARE_THRESHOLD: f32 = 20.0;
    /// 丢包率差值超过此阈值才认为有显著差异
    const LOSS_COMPARE_THRESHOLD: f32 = 0.01;

    pub fn cmp_eviction(a: &Backend, b: &Backend) -> std::cmp::Ordering {
        // 级联淘汰比较：Less = a 比 b 差（优先淘汰 a）
        // 延迟高的优先淘汰，延迟接近则丢包高的淘汰，都接近则连接数少的淘汰
        let da = a.get_avg_delay();
        let db = b.get_avg_delay();
        if (da - db).abs() > Self::DELAY_COMPARE_THRESHOLD {
            return db.total_cmp(&da);
        }
        let la = a.get_loss_rate();
        let lb = b.get_loss_rate();
        if (la - lb).abs() > Self::LOSS_COMPARE_THRESHOLD {
            return lb.total_cmp(&la);
        }
        a.connections().cmp(&b.connections())
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::Relaxed)
    }

    pub fn fetch_add_connection(&self, val: usize) {
        self.connections.fetch_add(val, Ordering::Relaxed);
    }

    pub fn fetch_sub_connection(&self, val: usize) {
        self.connections.fetch_sub(val, Ordering::Relaxed);
    }
}