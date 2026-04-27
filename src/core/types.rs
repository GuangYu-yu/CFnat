use serde::Serialize;
use std::net::SocketAddr;

#[derive(Serialize, Clone, PartialEq)]
pub struct IpInfo {
    pub ip: String,
    pub colo: Option<String>,
    pub delay: f64,
    pub loss: f64,
    pub samples: usize,
}

impl IpInfo {
    pub fn from_backend(backend: &std::sync::Arc<crate::core::backend::Backend>) -> Self {
        Self {
            ip: backend.addr.ip().to_string(),
            colo: backend.get_colo(),
            delay: backend.get_avg_delay() as f64,
            loss: backend.get_loss_rate() as f64,
            samples: backend.get_sample_count(),
        }
    }
}

#[derive(Serialize, Clone, PartialEq)]
pub struct StatusInfo {
    pub version: u64,
    pub running: bool,
    pub uptime_secs: u64,
    pub health_check_interval: u64,
    pub next_health_check: u64,
    pub primary_count: usize,
    pub primary_target: usize,
    pub backup_count: usize,
    pub backup_target: usize,
    pub primary_ips: Vec<IpInfo>,
    pub backup_ips: Vec<IpInfo>,
    pub sticky_ips: Vec<String>,
}

static STATUS_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn increment_status_version() -> u64 {
    STATUS_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

impl StatusInfo {
    pub fn from_loadbalancer(lb: &crate::core::loadbalancer::LoadBalancer) -> Self {
        let primary_backends = lb.get_primary_backends();
        let backup_backends = lb.get_backup_backends();

        let primary_ips: Vec<IpInfo> = primary_backends
            .iter()
            .map(IpInfo::from_backend)
            .collect();

        let backup_ips: Vec<IpInfo> = backup_backends
            .iter()
            .map(IpInfo::from_backend)
            .collect();

        let sticky_ips: Vec<String> = lb.get_sticky_ips()
            .into_iter()
            .map(|ip| ip.to_string())
            .collect();

        Self {
            version: increment_status_version(),
            running: false,
            uptime_secs: 0,
            health_check_interval: crate::core::config::get_global_config().health_check_interval.as_secs(),
            next_health_check: lb.get_next_health_check_secs(),
            primary_count: lb.get_primary_count(),
            primary_target: lb.get_primary_target(),
            backup_count: lb.get_backup_count(),
            backup_target: lb.get_backup_target(),
            primary_ips,
            backup_ips,
            sticky_ips,
        }
    }

    pub fn empty() -> Self {
        Self {
            version: 0,
            running: false,
            uptime_secs: 0,
            health_check_interval: crate::core::config::get_global_config().health_check_interval.as_secs(),
            next_health_check: 0,
            primary_count: 0,
            primary_target: 0,
            backup_count: 0,
            backup_target: 0,
            primary_ips: vec![],
            backup_ips: vec![],
            sticky_ips: vec![],
        }
    }
}

#[derive(Clone, Default)]
pub struct ConfigOverrides {
    pub ip_file: Option<String>,
    pub ip_content: Option<Vec<String>>,
    pub http: Option<String>,
    pub delay_limit: Option<u64>,
    pub tlr: Option<f64>,
    pub ips: Option<usize>,
    pub threads: Option<usize>,
    pub tls_port: Option<u16>,
    pub http_port: Option<u16>,
    pub colo: Option<Vec<String>>,
    pub addr: Option<SocketAddr>,
    pub max_sticky_slots: Option<usize>,
    #[cfg(feature = "web")]
    pub api_addr: Option<SocketAddr>,
}