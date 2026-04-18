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
        }
    }

    pub fn new_with_initial(addr: SocketAddr, initial_delay: f32, initial_loss: f32, colo: Option<String>) -> Self {
        Self {
            addr,
            colo: Mutex::new(colo),
            connections: AtomicUsize::new(0),
            avg_delay: AtomicU32::new(initial_delay.to_bits()),
            avg_loss: AtomicU32::new(initial_loss.to_bits()),
            sample_count: AtomicUsize::new(0),
            state: AtomicU8::new(BackendState::Warming as u8),
            entered_state_at: Mutex::new(Instant::now()),
            consecutive_failures: AtomicU32::new(0),
        }
    }

    pub fn set_colo(&self, colo: Option<String>) {
        *self.colo.lock() = colo;
    }

    pub fn get_colo(&self) -> Option<String> {
        self.colo.lock().clone()
    }

    pub fn record_delay(&self, delay_ms: f32) {
        let is_first = self.sample_count.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            Some(if count == 0 { 1 } else { (count + 1).min(get_global_config().sample_window as usize) })
        }).map(|old| old == 0).unwrap_or(false);
        
        let alpha = get_global_config().alpha;
        self.avg_delay.fetch_update(Ordering::AcqRel, Ordering::Acquire, |bits| {
            let current = f32::from_bits(bits);
            let new_val = if is_first { delay_ms } else { (current * (1.0 - alpha)) + (delay_ms * alpha) };
            Some(new_val.to_bits())
        }).ok();
    }

    pub fn record_loss(&self, is_loss: bool) {
        let is_first = self.sample_count.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            Some(if count == 0 { 1 } else { (count + 1).min(get_global_config().sample_window as usize) })
        }).map(|old| old == 0).unwrap_or(false);
        
        let alpha = get_global_config().alpha;
        let loss = if is_loss { 1.0 } else { 0.0 };
        self.avg_loss.fetch_update(Ordering::AcqRel, Ordering::Acquire, |bits| {
            let current = f32::from_bits(bits);
            let new_val = if is_first { loss } else { (current * (1.0 - alpha)) + (loss * alpha) };
            Some(new_val.to_bits())
        }).ok();
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
        let state = self.state.load(Ordering::Relaxed);
        state == BackendState::Active as u8 || state == BackendState::Warming as u8
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

    pub fn check_warming_expired(&self) -> bool {
        if self.is_warming() {
            let elapsed = self.entered_state_at.lock().elapsed().as_secs();
            elapsed >= get_global_config().warming_duration.as_secs()
        } else {
            false
        }
    }

    pub fn calculate_score(&self, pool_avg_delay: f32, pool_avg_loss: f32) -> f32 {
        let connections = self.connections.load(Ordering::Relaxed) as f32;
        let beta = (self.get_sample_count() as f32 / get_global_config().sample_window).min(1.0);
        
        let my_perf = self.get_avg_delay() * (1.0 + self.get_loss_rate() * 2.0);
        let pool_perf = (pool_avg_delay * (1.0 + pool_avg_loss * 2.0)).max(1.0);
        
        let ratio = if pool_perf > 0.0 { my_perf / pool_perf } else { 1.0 };
        let smooth_ratio = (1.0 + beta * (ratio - 1.0)).min(get_global_config().max_smooth_ratio);
        
        (connections + 1.0) * smooth_ratio
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