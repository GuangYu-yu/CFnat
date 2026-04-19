use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use tokio::task::JoinHandle;

use crate::core::{IpPool, LoadBalancer, HttpingConfig, build_hyper_client, parse_url, run_continuous_httping, run_forward, CancellationToken};
use crate::core::types::{StatusInfo, ConfigOverrides};
use crate::log::{push_log, reset_start_time, get_log_buffer};

pub struct ServiceState {
    pub running: AtomicBool,
    pub ip_pool: RwLock<Option<Arc<IpPool>>>,
    pub loadbalancer: RwLock<Option<Arc<LoadBalancer>>>,
    pub config: RwLock<ServiceConfig>,
    pub cancel_token: RwLock<Option<CancellationToken>>,
    pub start_time: RwLock<Option<Instant>>,
    pub task_handles: RwLock<Vec<JoinHandle<()>>>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ServiceConfig {
    pub ip_file: String,
    pub http: String,
    pub delay_limit: u64,
    pub tlr: f64,
    pub ips: usize,
    pub threads: usize,
    pub tls_port: u16,
    pub http_port: u16,
    pub colo: Option<Vec<String>>,
    pub addr: SocketAddr,
    pub max_sticky_slots: usize,
}

impl ServiceConfig {
    pub fn apply_overrides(&mut self, overrides: &ConfigOverrides) {
        if let Some(v) = &overrides.ip_file { self.ip_file = v.clone(); }
        if let Some(v) = &overrides.http { self.http = v.clone(); }
        if let Some(v) = overrides.delay_limit { self.delay_limit = v; }
        if let Some(v) = overrides.tlr { self.tlr = v; }
        if let Some(v) = overrides.ips { self.ips = v; }
        if let Some(v) = overrides.threads { self.threads = v; }
        if let Some(v) = overrides.tls_port { self.tls_port = v; }
        if let Some(v) = overrides.http_port { self.http_port = v; }
        if let Some(v) = &overrides.colo { self.colo = Some(v.clone()); }
        if let Some(v) = overrides.addr { self.addr = v; }
        if let Some(v) = overrides.max_sticky_slots { self.max_sticky_slots = v; }
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            ip_file: "ip.txt".to_string(),
            http: "http://cp.cloudflare.com/cdn-cgi/trace".to_string(),
            delay_limit: 500,
            tlr: 0.1,
            ips: 10,
            threads: 16,
            tls_port: 443,
            http_port: 80,
            colo: None,
            addr: "127.6.6.6:1234".parse().unwrap(),
            max_sticky_slots: 5,
        }
    }
}

impl Default for ServiceState {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceState {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            ip_pool: RwLock::new(None),
            loadbalancer: RwLock::new(None),
            config: RwLock::new(ServiceConfig::default()),
            cancel_token: RwLock::new(None),
            start_time: RwLock::new(None),
            task_handles: RwLock::new(Vec::new()),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn get_config(&self) -> ServiceConfig {
        self.config.read().clone()
    }

    pub fn update_config(&self, new_config: ServiceConfig) {
        *self.config.write() = new_config;
    }

    pub fn get_uptime_secs(&self) -> u64 {
        if let Some(start) = self.start_time.read().as_ref() {
            start.elapsed().as_secs()
        } else {
            0
        }
    }

    pub fn build_full_status(&self) -> StatusInfo {
        let running = self.is_running();
        let uptime_secs = self.get_uptime_secs();

        if let Some(lb) = self.loadbalancer.read().as_ref() {
            let mut info = StatusInfo::from_loadbalancer(lb);
            info.running = running;
            info.uptime_secs = uptime_secs;
            info
        } else {
            let mut info = StatusInfo::empty();
            info.running = running;
            info.uptime_secs = uptime_secs;
            info
        }
    }

    pub fn start(&self) -> Result<(), String> {
        if self.is_running() {
            return Err("服务已在运行".to_string());
        }

        let config = self.get_config();
        
        let ip_pool = Arc::new(IpPool::from_file(&config.ip_file));
        if ip_pool.total_count() == 0 {
            return Err("未找到有效的 IP".to_string());
        }

        self.start_with_pool(ip_pool)
    }

    pub fn start_with_ips(&self, ip_file: Option<&str>, ip_content: Option<&[String]>) -> Result<(), String> {
        if self.is_running() {
            return Err("服务已在运行".to_string());
        }

        let mut all_ips = Vec::new();
        
        if let Some(file) = ip_file
            && !file.is_empty()
            && let Ok(f) = std::fs::File::open(file)
        {
            use std::io::{BufRead, BufReader};
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    all_ips.push(line.to_string());
                }
            }
        }
        
        if let Some(content) = ip_content {
            all_ips.extend(content.iter().cloned());
        }
        
        if all_ips.is_empty() {
            let config = self.get_config();
            let ip_pool = Arc::new(IpPool::from_file(&config.ip_file));
            if ip_pool.total_count() == 0 {
                return Err("未找到有效的 IP".to_string());
            }
            return self.start_with_pool(ip_pool);
        }

        let ip_pool = Arc::new(IpPool::new(&all_ips));
        self.start_with_pool(ip_pool)
    }

    fn start_with_pool(&self, ip_pool: Arc<IpPool>) -> Result<(), String> {
        // 启动前清空旧日志并重置时间基准
        reset_start_time();
        get_log_buffer().clear();

        let config = self.get_config();

        let (_, host, _, _) = parse_url(&config.http)
            .ok_or("URL 解析失败")?;

        let client = build_hyper_client(config.delay_limit, host.clone())
            .ok_or("创建 HTTP 客户端失败")?;

        let client = Arc::new(client);
        let (notify_tx, notify_rx) = tokio::sync::watch::channel(false);
        let colo_filter = config.colo.clone();

        let cancel_token = CancellationToken::new();
        
        let lb = Arc::new(
            LoadBalancer::new(config.ips)
                .with_delay_threshold(config.delay_limit as f32)
                .with_loss_threshold(config.tlr as f32)
                .with_health_check_url(config.http.clone())
                .with_ports(config.tls_port, config.http_port)
                .with_timeout(1800)
                .with_notify(notify_tx)
                .with_client(client.clone())
                .with_server_name(host.clone())
                .with_colo_filter(colo_filter.clone())
                .with_max_sticky_slots(config.max_sticky_slots),
        );

        crate::core::init_global_limiter(config.threads);

        *self.ip_pool.write() = Some(ip_pool.clone());
        *self.loadbalancer.write() = Some(lb.clone());
        *self.cancel_token.write() = Some(cancel_token.clone());
        *self.start_time.write() = Some(Instant::now());
        self.running.store(true, Ordering::Relaxed);

        let ip_pool_clone = ip_pool.clone();
        let lb_clone = lb.clone();
        let http = config.http.clone();
        let tls_port = config.tls_port;
        let http_port = config.http_port;
        let delay_limit = config.delay_limit;
        let addr = config.addr;
        let cancel_token_for_httping = cancel_token.clone();

        let httping_handle: JoinHandle<()> = tokio::spawn(async move {
            run_continuous_httping(
                ip_pool_clone,
                lb_clone,
                &http,
                HttpingConfig {
                    tls_port,
                    http_port,
                    timeout_ms: 1800,
                    delay_limit,
                    colo_filter: colo_filter.map(Arc::new),
                    client,
                },
                notify_rx,
                cancel_token_for_httping,
            ).await;
        });

        lb.clone().start_health_check();

        let lb_forward = lb.clone();
        let forward_cancel_token = cancel_token.clone();
        
        let forward_handle: JoinHandle<()> = tokio::spawn(async move {
            if let Err(e) = run_forward(addr, lb_forward, tls_port, http_port, forward_cancel_token).await {
                push_log("ERROR", &format!("转发服务错误：{}", e));
            }
        });

        self.task_handles.write().extend([httping_handle, forward_handle]);

        Ok(())
    }

    pub async fn stop(&self) -> Result<(), String> {
        if !self.is_running() {
            return Err("服务未运行".to_string());
        }

        if let Some(token) = self.cancel_token.read().as_ref() {
            token.cancel();
        }

        if let Some(lb) = self.loadbalancer.read().as_ref() {
            lb.stop();
        }
        
        self.running.store(false, Ordering::Relaxed);

        let handles: Vec<JoinHandle<()>> = self.task_handles.write().drain(..).collect();
        for handle in handles {
            let _ = tokio::time::timeout(Duration::from_secs(3), handle).await;
        }

        *self.ip_pool.write() = None;
        *self.loadbalancer.write() = None;
        *self.cancel_token.write() = None;
        *self.start_time.write() = None;

        push_log("INFO", "服务已停止");
        
        Ok(())
    }

    pub fn get_loadbalancer(&self) -> Option<Arc<LoadBalancer>> {
        self.loadbalancer.read().clone()
    }
}