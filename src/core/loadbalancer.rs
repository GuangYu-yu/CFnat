use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;

use crate::core::backend::Backend;
use crate::core::cancel::CancellationToken;
use crate::core::config::get_global_config;
use crate::core::httping::{PingConfig, PingResultDetail};
use crate::log::push_log;

/// 连续失败次数达到该值时判定为超限
const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// 主队列健康检查间隔相对于备选队列的倍数
const PRIMARY_CHECK_INTERVAL_MULT: u32 = 4;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[derive(Debug)]
struct StickySlot {
    backend: Arc<Backend>,
    last_switch: Instant,
    last_access_ms: AtomicU64,
    interval: Duration,
}

#[derive(Clone)]
struct HealthCheckConfig {
    ping_config: PingConfig,
    delay_threshold: f32,
    loss_threshold: f32,
}

#[derive(Debug)]
struct BalancerInner {
    primary: Vec<Arc<Backend>>,
    backup: Vec<Arc<Backend>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PoolType {
    Primary,
    Backup,
}

impl PoolType {
    fn as_str(self) -> &'static str {
        match self {
            PoolType::Primary => "主队列",
            PoolType::Backup => "备选",
        }
    }
}

#[derive(Debug)]
pub struct LoadBalancer {
    inner: RwLock<BalancerInner>,
    ip_set: RwLock<HashSet<std::net::IpAddr>>,
    primary_index: AtomicUsize,
    primary_target: AtomicUsize,
    backup_target: AtomicUsize,
    min_active_target: usize,
    health_check_url: String,
    tls_port: u16,
    http_port: u16,
    timeout_ms: u64,
    notify_tx: Option<tokio::sync::watch::Sender<bool>>,
    delay_threshold: AtomicU32,
    loss_threshold: AtomicU32,
    client: RwLock<Option<Arc<crate::core::hyper::MyHyperClient>>>,
    colo_filter: Option<Arc<Vec<String>>>,
    sticky_slots: RwLock<Vec<StickySlot>>,
    last_expand_ms: AtomicU64,
    cancel_token: CancellationToken,
    next_health_check_ms: AtomicU64,
    max_sticky_slots: usize,
}

pub enum AddResult {
    AddedToPrimary,
    AddedToBackup,
    QueueFull,
    AlreadyExists,
}

impl LoadBalancer {
    pub fn new(primary_target: usize) -> Self {
        let backup_target = ((primary_target as f32 / 2.0).ceil() as usize).min(get_global_config().max_backup_target).max(2);
        let min_active_target = (primary_target as f32 / 2.0).ceil() as usize;
        Self {
            inner: RwLock::new(BalancerInner {
                primary: Vec::new(),
                backup: Vec::new(),
            }),
            ip_set: RwLock::new(HashSet::new()),
            primary_index: AtomicUsize::new(0),
            primary_target: AtomicUsize::new(primary_target),
            backup_target: AtomicUsize::new(backup_target),
            min_active_target,
            health_check_url: String::new(),
            tls_port: 443,
            http_port: 80,
            timeout_ms: 2000,
            notify_tx: None,
            delay_threshold: AtomicU32::new(0.0f32.to_bits()),
            loss_threshold: AtomicU32::new(0.0f32.to_bits()),
            client: RwLock::new(None),
            colo_filter: None,
            sticky_slots: RwLock::new(Vec::new()),
            last_expand_ms: AtomicU64::new(now_ms()),
            cancel_token: CancellationToken::new(),
            next_health_check_ms: AtomicU64::new(now_ms()),
            max_sticky_slots: get_global_config().max_sticky_slots,
        }
    }

    pub fn try_add_backend(
        &self,
        addr: SocketAddr,
        delay: f32,
        colo: Option<&str>,
    ) -> AddResult {
        if self.contains(addr.ip()) {
            return AddResult::AlreadyExists;
        }
        
        let primary_count = self.get_primary_count();
        let backup_count = self.get_backup_count();
        
        let primary_target = self.primary_target.load(Ordering::Relaxed);
        let backup_target = self.backup_target.load(Ordering::Relaxed);
        
        let colo_string = colo.map(|s| s.to_string());
        
        if primary_count < primary_target {
            self.add_to_primary(addr, delay, 0.0, colo_string);
            push_log("INFO", &format!("[+] {} {:.0}ms [{}]", addr, delay, colo.unwrap_or("")));
            AddResult::AddedToPrimary
        } else if backup_count < backup_target {
            self.add_to_backup(addr, delay, 0.0, colo_string);
            push_log("INFO", &format!("[+] {} {:.0}ms [{}] (备选)", addr, delay, colo.unwrap_or("")));
            AddResult::AddedToBackup
        } else {
            AddResult::QueueFull
        }
    }

    pub fn with_delay_threshold(self, delay_threshold: f32) -> Self {
        self.delay_threshold.store(delay_threshold.to_bits(), Ordering::Relaxed);
        self
    }

    pub fn with_loss_threshold(self, loss_threshold: f32) -> Self {
        self.loss_threshold.store(loss_threshold.to_bits(), Ordering::Relaxed);
        self
    }

    pub fn with_colo_filter(mut self, colo_filter: Option<Vec<String>>) -> Self {
        self.colo_filter = colo_filter.map(Arc::new);
        self
    }

    pub fn with_health_check_url(mut self, url: String) -> Self {
        self.health_check_url = url;
        self
    }

    pub fn with_ports(mut self, tls_port: u16, http_port: u16) -> Self {
        self.tls_port = tls_port;
        self.http_port = http_port;
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn with_notify(mut self, tx: tokio::sync::watch::Sender<bool>) -> Self {
        self.notify_tx = Some(tx);
        self
    }

    pub(crate) fn with_client(self, client: Arc<crate::core::hyper::MyHyperClient>) -> Self {
        *self.client.write() = Some(client);
        self
    }

    pub fn rebuild_client(&self) {
        *self.client.write() = Some(Arc::new(crate::core::hyper::build_hyper_client(self.timeout_ms)));
        push_log("INFO", "[重建] HTTP 客户端已重建");
    }

    pub fn reset_all_metrics(&self) {
        let inner = self.inner.read();
        for backend in inner.primary.iter() {
            backend.reset_metrics();
        }
        for backend in inner.backup.iter() {
            backend.reset_metrics();
        }
    }

    pub fn with_max_sticky_slots(mut self, max_sticky_slots: usize) -> Self {
        self.max_sticky_slots = max_sticky_slots;
        self
    }

    pub fn contains(&self, ip: std::net::IpAddr) -> bool {
        self.ip_set.read().contains(&ip)
    }

    pub fn primary_full(&self) -> bool {
        self.get_primary_count() >= self.primary_target.load(Ordering::Relaxed)
    }

    pub fn backup_full(&self) -> bool {
        self.get_backup_count() >= self.backup_target.load(Ordering::Relaxed)
    }

    pub fn should_pause(&self) -> bool {
        self.primary_full() && self.backup_full()
    }

    pub fn notify_resume(&self) {
        if let Some(tx) = &self.notify_tx {
            let _ = tx.send(true);
        }
    }

    pub fn select(&self) -> Option<Arc<Backend>> {
        let slots = self.sticky_slots.read();
        // 只从仍可用的槽中选取；已移除/隔离等不可用节点跳过，交给池路径处理
        let usable: Vec<&StickySlot> = slots.iter().filter(|s| s.backend.is_selectable()).collect();

        if !usable.is_empty() {
            return self.select_from_slots(&usable, usable.len());
        }
        drop(slots);

        let inner = self.inner.read();
        if !inner.primary.is_empty() {
            return self.select_from_pool(&inner.primary, &self.primary_index);
        }
        if !inner.backup.is_empty() {
            return self.select_from_pool(&inner.backup, &self.primary_index);
        }
        None
    }

    fn select_from_slots(&self, slots: &[&StickySlot], len: usize) -> Option<Arc<Backend>> {
        let current_ms = now_ms();
        let on_select = |slot: &StickySlot| {
            slot.last_access_ms.store(current_ms, Ordering::Release);
        };

        if len == 1 {
            on_select(slots[0]);
            slots[0].backend.fetch_add_connection(1);
            return Some(slots[0].backend.clone());
        }

        Self::select_by_connection(slots, &self.primary_index, |s| &s.backend, |s| s.backend.clone(), |s: &&StickySlot| on_select(s))
    }

    fn select_from_pool(&self, pool: &[Arc<Backend>], index: &AtomicUsize) -> Option<Arc<Backend>> {
        // 优先选正常后端，没有则保底选隔离的
        let selectable: Vec<&Arc<Backend>> = pool.iter()
            .filter(|b| b.is_selectable())
            .collect();
        let fallback: Vec<&Arc<Backend>> = pool.iter()
            .filter(|b| b.is_isolated())
            .collect();

        let candidates = if !selectable.is_empty() { &selectable } else { &fallback };
        let len = candidates.len();
        if len == 0 {
            return None;
        }
        
        if len == 1 {
            candidates[0].fetch_add_connection(1);
            return Some(candidates[0].clone());
        }
        
        Self::select_by_connection(candidates, index, |b| b.as_ref(), |b| (*b).clone(), |_| {})
    }

    fn select_by_connection<T>(
        items: &[T],
        index: &AtomicUsize,
        get_backend: impl Fn(&T) -> &Backend,
        get_arc: impl Fn(&T) -> Arc<Backend>,
        on_select: impl Fn(&T),
    ) -> Option<Arc<Backend>> {
        let len = items.len();
        let i = index.fetch_add(1, Ordering::Relaxed) % len;
        let j = (i + len / 2) % len;
        
        let item_i = items.get(i)?;
        let item_j = items.get(j)?;
        
        let backend_i = get_backend(item_i);
        let backend_j = get_backend(item_j);
        
        let idx = if backend_i.connections() <= backend_j.connections() {
            i
        } else {
            j
        };
        
        let item = items.get(idx)?;
        on_select(item);
        get_backend(item).fetch_add_connection(1);
        
        Some(get_arc(item))
    }

    fn maintain_sticky_slots(&self, pool: &[Arc<Backend>]) {
        let now = Instant::now();
        let current_ms = now_ms();
        let ttl_ms = get_global_config().sticky_slot_ttl.as_millis() as u64;
        let expand_interval_ms = get_global_config().sticky_slot_expand_interval.as_millis() as u64;
        
        let mut slots = self.sticky_slots.write();
        
        slots.retain(|s| {
            if s.backend.is_isolated() || s.backend.is_removed() {
                return false;
            }
            let last_access = s.last_access_ms.load(Ordering::Acquire);
            current_ms.saturating_sub(last_access) < ttl_ms
        });
        slots.shrink_to_fit();
        
        let get_active_unused = |used_addrs: &HashSet<SocketAddr>| {
            pool.iter()
                .filter(|b| b.is_selectable() && !used_addrs.contains(&b.addr))
                .min_by_key(|b| b.connections())
                .cloned()
        };
        
        let mut used_addrs: HashSet<_> = slots.iter().map(|s| s.backend.addr).collect();
        
        for s in slots.iter_mut().filter(|s| now.duration_since(s.last_switch) >= s.interval) {
            if let Some(new_b) = get_active_unused(&used_addrs) {
                used_addrs.insert(new_b.addr);
                s.backend = new_b;
                s.last_switch = now;
            }
        }
        
        let total_conns: usize = slots.iter().map(|s| s.backend.connections()).sum();
        let last_expand = self.last_expand_ms.load(Ordering::Relaxed);
        let should_expand = slots.is_empty() || (
            slots.len() < self.max_sticky_slots && (
                current_ms.saturating_sub(last_expand) >= expand_interval_ms ||
                slots.len() * slots.len() < total_conns
            )
        );
        
        if should_expand {
            let used_addrs: HashSet<_> = slots.iter().map(|s| s.backend.addr).collect();
            if let Some(b) = get_active_unused(&used_addrs) {
                let interval = get_global_config().sticky_base_interval + 
                    Duration::from_secs((slots.len() as u64) * get_global_config().sticky_increment_interval.as_secs());
                slots.push(StickySlot {
                    backend: b,
                    last_switch: now,
                    last_access_ms: AtomicU64::new(current_ms),
                    interval,
                });
                self.last_expand_ms.store(current_ms, Ordering::Relaxed);
            }
        }
    }

    fn start_sticky_maintainer(self: Arc<Self>) {
        let cancel_token = self.cancel_token.clone();
        let lb = self.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.tick().await;
            
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        break;
                    }
                    _ = interval.tick() => {
                        let inner = lb.inner.read();
                        if !inner.primary.is_empty() {
                            lb.maintain_sticky_slots(&inner.primary);
                        } else if !inner.backup.is_empty() {
                            lb.maintain_sticky_slots(&inner.backup);
                        }
                    }
                }
            }
        });
    }
    
    fn check_warming_backends(&self, pool: &mut [Arc<Backend>]) {
        for backend in pool.iter() {
            if backend.check_warming_expired() {
                backend.mark_active();
            }
        }
    }

    fn cleanup_removed(&self) {
        let mut inner = self.inner.write();
        
        let removed_ips: Vec<_> = inner.primary.iter()
            .chain(inner.backup.iter())
            .filter(|b| b.is_removed())
            .map(|b| b.addr.ip())
            .collect();
        
        let before = inner.primary.len();
        inner.primary.retain(|b| !b.is_removed());
        inner.primary.shrink_to_fit();
        let primary_removed = before - inner.primary.len();
        
        let backup_before = inner.backup.len();
        inner.backup.retain(|b| !b.is_removed());
        inner.backup.shrink_to_fit();
        let backup_removed = backup_before - inner.backup.len();

        drop(inner);
        
        for ip in removed_ips {
            self.ip_set.write().remove(&ip);
        }

        let removed_count = primary_removed + backup_removed;
        if removed_count > 0 {
            push_log("INFO", &format!("[-] 清理 {} 个失效节点", removed_count));
            self.notify_resume();
        }
    }

    pub fn release(&self, backend: &Backend) {
        backend.fetch_sub_connection(1);
    }

    pub fn record(&self, backend: &Backend, delay_ms: Option<f32>, is_loss: bool) {
        backend.record(delay_ms, is_loss);
    }

    pub fn check_and_evict(&self, backend: &Backend) -> bool {
        if backend.is_isolated() || backend.is_removed() {
            return false;
        }

        let sample_count = backend.get_sample_count();
        
        if sample_count < get_global_config().evict_threshold {
            return false;
        }
        
        let avg_delay = backend.get_avg_delay();
        let loss_rate = backend.get_loss_rate();

        let delay_threshold = f32::from_bits(self.delay_threshold.load(Ordering::Relaxed));
        let loss_threshold = f32::from_bits(self.loss_threshold.load(Ordering::Relaxed));

        if avg_delay > delay_threshold || loss_rate > loss_threshold {
            if self.try_deactivate() {
                backend.mark_isolated();
                push_log("WARN", &format!("[→] {} 进入隔离状态 (延迟{:.0}ms 丢包{:.0}%)", 
                    backend.addr, avg_delay, loss_rate * 100.0));
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// 单一收口：检查是否允许减少一个可选后端。
    /// 达到下限时尝试从备池补位。返回 true 表示允许操作。
    pub(crate) fn try_deactivate(&self) -> bool {
        let selectable = self.inner.read().primary.iter()
            .filter(|b| b.is_selectable()).count();

        if selectable > self.min_active_target {
            return true;
        }

        // 达到下限，尝试用备池补一个
        let mut inner = self.inner.write();
        if let Some(pos) = inner.backup.iter().position(|b| b.is_selectable()) {
            let replacement = inner.backup.remove(pos);
            let promoted_addr = replacement.addr;
            replacement.mark_active();
            inner.primary.push(replacement);
            drop(inner);
            self.notify_resume();
            push_log("INFO", &format!("[↑] {} 从备选补充到负载均衡队列", promoted_addr));
            true
        } else {
            false
        }
    }

    pub fn refill_from_backup(&self) {
        let primary_target = self.primary_target.load(Ordering::Relaxed);

        let mut inner = self.inner.write();
        let mut promoted = 0;
        while inner.primary.len() < primary_target && !inner.backup.is_empty() {
            let backend = inner.backup.remove(0);
            inner.primary.push(backend);
            promoted += 1;
        }
        drop(inner);
        
        if promoted > 0 {
            push_log("INFO", &format!("[↑] {} 个备选提升到主队列", promoted));
        }

        self.notify_resume();
    }

    pub fn get_backup_count(&self) -> usize {
        self.inner.read().backup.len()
    }

    pub fn get_primary_count(&self) -> usize {
        self.inner.read().primary.len()
    }

    pub fn get_primary_target(&self) -> usize {
        self.primary_target.load(Ordering::Relaxed)
    }

    pub fn get_backup_target(&self) -> usize {
        self.backup_target.load(Ordering::Relaxed)
    }

    pub fn add_to_primary(&self, addr: SocketAddr, initial_delay: f32, initial_loss: f32, colo: Option<String>) {
        self.add_to_list(addr, initial_delay, initial_loss, colo, &mut self.inner.write().primary);
    }

    pub fn add_to_backup(&self, addr: SocketAddr, initial_delay: f32, initial_loss: f32, colo: Option<String>) {
        self.add_to_list(addr, initial_delay, initial_loss, colo, &mut self.inner.write().backup);
    }

    fn add_to_list(&self, addr: SocketAddr, initial_delay: f32, initial_loss: f32, colo: Option<String>, target: &mut Vec<Arc<Backend>>) {
        let backend = Arc::new(Backend::new_with_initial(addr, initial_delay, initial_loss, colo));
        target.push(backend);
        self.ip_set.write().insert(addr.ip());
    }

    pub fn get_primary_backends(&self) -> Vec<Arc<Backend>> {
        self.inner.read().primary.clone()
    }

    pub fn get_backup_backends(&self) -> Vec<Arc<Backend>> {
        self.inner.read().backup.clone()
    }

    pub fn get_sticky_ips(&self) -> Vec<std::net::IpAddr> {
        self.sticky_slots
            .read()
            .iter()
            .map(|s| s.backend.addr.ip())
            .collect()
    }

    pub fn stop(&self) {
        self.cancel_token.cancel();
    }

    pub fn get_next_health_check_secs(&self) -> u64 {
        let next_ms = self.next_health_check_ms.load(Ordering::Relaxed);
        let current_ms = now_ms();
        if next_ms > current_ms {
            (next_ms - current_ms) / 1000
        } else {
            0
        }
    }

    fn sort_backup(&self) {
        let mut inner = self.inner.write();
        inner.backup.sort_by(|a, b| Backend::cmp_eviction(b, a));
    }

    pub fn start_health_check(self: Arc<Self>) {
        let Some(client) = self.client.read().clone() else { return };

        self.clone().start_sticky_maintainer();

        let cancel_token = self.cancel_token.clone();

        let Some((_, host, scheme, path)) = crate::core::hyper::parse_url(&self.health_check_url) else { return };
        
        let health_check_config = HealthCheckConfig {
            ping_config: PingConfig {
                tls_port: self.tls_port,
                http_port: self.http_port,
                client,
                host: Arc::from(host.as_str()),
                scheme: Arc::from(scheme),
                path: Arc::from(path.as_str()),
                timeout_ms: self.timeout_ms,
                colo_filter: self.colo_filter.clone(),
            },
            delay_threshold: f32::from_bits(self.delay_threshold.load(Ordering::Relaxed)),
            loss_threshold: f32::from_bits(self.loss_threshold.load(Ordering::Relaxed)),
        };

        let current_ms = now_ms();
        let hc = get_global_config().health_check_interval;
        self.next_health_check_ms.store(current_ms + hc.as_millis() as u64, Ordering::Relaxed);

        let lb = self.clone();
        tokio::spawn(async move {
            let mut backup_interval = tokio::time::interval(get_global_config().health_check_interval);
            backup_interval.tick().await;
            
            let mut primary_interval = tokio::time::interval(get_global_config().health_check_interval.saturating_mul(PRIMARY_CHECK_INTERVAL_MULT));
            primary_interval.tick().await;

            let mut last_tick = std::time::SystemTime::now();
            const TIME_JUMP_THRESHOLD: Duration = Duration::from_secs(60);

            loop {
                let is_primary = tokio::select! {
                    _ = cancel_token.cancelled() => {
                        push_log("INFO", "[健康检查] 收到停止信号，退出");
                        break;
                    }
                    _ = backup_interval.tick() => false,
                    _ = primary_interval.tick() => true,
                };

                let now = std::time::SystemTime::now();
                if now.duration_since(last_tick).unwrap_or_default() > TIME_JUMP_THRESHOLD {
                    push_log("INFO", "[健康检查] 检测到时间跳跃，重建客户端并重置后端指标");
                    lb.rebuild_client();
                    lb.reset_all_metrics();
                }
                last_tick = now;

                if is_primary {
                    lb.run_primary_health_check(health_check_config.clone());
                } else {
                    lb.run_backup_health_check(health_check_config.clone());
                }
            }
        });
    }

    fn run_health_check_for_pool(
        self: &Arc<Self>,
        config: HealthCheckConfig,
        pool_type: PoolType,
    ) {
        let is_primary = pool_type == PoolType::Primary;
        
        let backends = {
            let mut inner = self.inner.write();
            let pool = if is_primary { &mut inner.primary } else { &mut inner.backup };
            self.check_warming_backends(pool);
            pool.clone()
        };
        
        if backends.is_empty() {
            if !is_primary {
                let next_ms = now_ms() + get_global_config().health_check_interval.as_millis() as u64;
                self.next_health_check_ms.store(next_ms, Ordering::Relaxed);
            }
            return;
        }

        let lb = self.clone();
        let pool_type_str = pool_type.as_str();
        let concurrency = get_global_config().health_check_concurrency;
        
        tokio::spawn(async move {
            let mut join_set = tokio::task::JoinSet::new();
            let mut removed_count = 0usize;

            let ping_config = config.ping_config;
            let delay_threshold = config.delay_threshold;
            let loss_threshold = config.loss_threshold;

            for backend in backends {
                if backend.is_removed() {
                    continue;
                }

                let pc = ping_config.clone();
                let lb = lb.clone();

                join_set.spawn(async move {
                    let result = crate::core::httping::http_ping_multi(
                        backend.addr.ip(),
                        &pc,
                    ).await;
                    (backend, result, delay_threshold, loss_threshold, lb, pool_type)
                });

                if join_set.len() >= concurrency
                    && let Some(res) = join_set.join_next().await
                    && let Ok((backend, result, delay_threshold, loss_threshold, lb, pt)) = res
                    && Self::handle_health_check_result(backend, result, delay_threshold, loss_threshold, lb, pt)
                {
                    removed_count += 1;
                }
            }

            while let Some(res) = join_set.join_next().await
                && let Ok((backend, result, delay_threshold, loss_threshold, lb, pt)) = res
                && Self::handle_health_check_result(backend, result, delay_threshold, loss_threshold, lb, pt)
            {
                removed_count += 1;
            }

            lb.cleanup_removed();
            
            if removed_count > 0 {
                push_log("INFO", &format!("[{}] 检查完成，移除 {} 个", pool_type_str, removed_count));
            }
            
            if is_primary {
                lb.refill_from_backup();
            } else {
                lb.sort_backup();
                let current_ms = now_ms();
                lb.next_health_check_ms.store(current_ms + get_global_config().health_check_interval.as_millis() as u64, Ordering::Relaxed);
            }
        });
    }

    fn run_backup_health_check(self: &Arc<Self>, config: HealthCheckConfig) {
        self.run_health_check_for_pool(config, PoolType::Backup);
    }

    fn run_primary_health_check(self: &Arc<Self>, config: HealthCheckConfig) {
        self.run_health_check_for_pool(config, PoolType::Primary);
    }

    fn handle_health_check_result(
        backend: Arc<Backend>,
        result: Option<PingResultDetail>,
        delay_threshold: f32,
        loss_threshold: f32,
        lb: Arc<LoadBalancer>,
        pool_type: PoolType,
    ) -> bool {
        let is_primary = pool_type == PoolType::Primary;

        enum Action {
            None,
            Recover,
            Isolate,
            Remove(String),
        }

        let action = match result {
            Some(detail) if detail.colo_mismatch => {
                Action::Remove(format!("数据中心[{}]不匹配", detail.colo.as_deref().unwrap_or("未知")))
            }
            Some(detail) => {
                backend.record(Some(detail.delay), detail.success_count < get_global_config().ping_times);
                if let Some(c) = detail.colo {
                    backend.set_colo(Some(c));
                }

                if backend.get_sample_count() < get_global_config().sample_window as usize {
                    Action::None
                } else if backend.get_avg_delay() > delay_threshold || backend.get_loss_rate() > loss_threshold {
                    if backend.is_isolated() || !is_primary {
                        Action::Remove("隔离后仍不达标".into())
                    } else {
                        Action::Isolate
                    }
                } else {
                    Action::Recover
                }
            }
            None => {
                backend.record(None, true);
                backend.record_failure();

                let loss_rate = backend.get_loss_rate();
                let failures = backend.consecutive_failures();
                let over_limit = loss_rate > loss_threshold || failures >= MAX_CONSECUTIVE_FAILURES;

                if backend.get_sample_count() >= get_global_config().sample_window as usize && over_limit {
                    if backend.is_isolated() || !is_primary {
                        Action::Remove("无响应".into())
                    } else {
                        Action::Isolate
                    }
                } else {
                    Action::None
                }
            }
        };

        match action {
            Action::Isolate => {
                let isolated = lb.try_deactivate();
                if isolated {
                    backend.mark_isolated();
                    push_log("WARN", &format!("[→] {} 隔离 (延迟{:.0}ms 丢包{:.0}%)",
                        backend.addr, backend.get_avg_delay(), backend.get_loss_rate() * 100.0));
                }
                isolated
            }
            Action::Remove(reason) => {
                if lb.try_deactivate() {
                    backend.mark_removed();
                    push_log("WARN", &format!("[-] {} 移除 ({})", backend.addr, reason));
                    if is_primary {
                        lb.refill_from_backup();
                    }
                    true
                } else {
                    false
                }
            }
            Action::Recover => {
                backend.record_success();
                if backend.is_isolated() {
                    backend.mark_active();
                    push_log("INFO", &format!("[←] {} 恢复正常", backend.addr));
                }
                false
            }
            Action::None => false,
        }
    }
}