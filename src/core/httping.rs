use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::Method;
use tokio::task::JoinSet;

use crate::core::cancel::CancellationToken;
use crate::core::config::get_global_config;
use crate::core::hyper::{parse_url, send_request};
use crate::core::ip::IpPool;
use crate::core::loadbalancer::{AddResult, LoadBalancer};
use crate::core::pool::GLOBAL_LIMITER;
use crate::log::push_log;

type PingResult = Option<(SocketAddr, f32, Option<String>, u8, bool)>;

fn extract_colo(resp: &hyper::Response<hyper::body::Incoming>) -> Option<String> {
    resp.headers()
        .get("cf-ray")?
        .to_str()
        .ok()?
        .rsplit('-')
        .next()
        .map(str::to_owned)
}

async fn single_ping(
    client: &crate::core::hyper::MyHyperClient,
    host: &str,
    uri: &http::Uri,
    timeout_ms: u64,
) -> Option<(f32, Option<String>)> {
    let start = Instant::now();

    let resp = send_request(client, host, uri.clone(), Method::HEAD, timeout_ms).await?;

    let delay = start.elapsed().as_secs_f32() * 1000.0;

    let colo = extract_colo(&resp);

    Some((delay, colo))
}

#[derive(Clone, Debug)]
pub struct PingConfig {
    pub tls_port: u16,
    pub http_port: u16,
    pub client: Arc<crate::core::hyper::MyHyperClient>,
    pub host: Arc<str>,
    pub scheme: Arc<str>,
    pub path: Arc<str>,
    pub timeout_ms: u64,
    pub colo_filter: Option<Arc<Vec<String>>>,
}

pub struct PingResultDetail {
    pub addr: SocketAddr,
    pub delay: f32,
    pub colo: Option<String>,
    pub success_count: u8,
    pub colo_mismatch: bool,
}

pub async fn http_ping_multi(
    ip: std::net::IpAddr,
    config: &PingConfig,
) -> Option<PingResultDetail> {
    let _permit = GLOBAL_LIMITER.get()?.acquire().await;

    let port = if &*config.scheme == "https" { config.tls_port } else { config.http_port };
    let addr = SocketAddr::new(ip, port);
    let uri: http::Uri = format!("{}://{}{}", &*config.scheme, addr, &*config.path).parse().ok()?;

    let mut total_delay = 0.0f32;
    let mut success_count = 0u8;
    let mut colo: Option<String> = None;
    let mut colo_mismatch = false;

    for _ in 0..get_global_config().ping_times {
        if colo_mismatch {
            break;
        }

        if let Some((delay, c)) = single_ping(&config.client, &config.host, &uri, config.timeout_ms).await {
            total_delay += delay;
            success_count += 1;

            if colo.is_none() {
                if let Some(ref filter) = config.colo_filter {
                    if let Some(ref dc) = c {
                        if !filter.iter().any(|f| f.eq_ignore_ascii_case(dc)) {
                            colo_mismatch = true;
                        }
                    } else {
                        colo_mismatch = true;
                    }
                }
                colo = c;
            }

            if !colo_mismatch {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }

    if success_count == 0 {
        return None;
    }

    let avg_delay = total_delay / success_count as f32;

    Some(PingResultDetail {
        addr,
        delay: avg_delay,
        colo,
        success_count,
        colo_mismatch,
    })
}

pub struct HttpingConfig {
    pub tls_port: u16,
    pub http_port: u16,
    pub timeout_ms: u64,
    pub delay_limit: u64,
    pub colo_filter: Option<Arc<Vec<String>>>,
    pub client: Arc<crate::core::hyper::MyHyperClient>,
}

pub async fn run_continuous_httping(
    ip_pool: Arc<IpPool>,
    lb: Arc<LoadBalancer>,
    url: &str,
    config: HttpingConfig,
    mut notify_rx: tokio::sync::watch::Receiver<bool>,
    cancel_token: CancellationToken,
) {
    let Some((_, host, scheme, path)) = parse_url(url) else {
        push_log("ERROR", "URL 解析失败");
        return;
    };

    let ping_config = PingConfig {
        tls_port: config.tls_port,
        http_port: config.http_port,
        client: config.client,
        host: Arc::from(host.as_str()),
        scheme: Arc::from(scheme),
        path: Arc::from(path.as_str()),
        timeout_ms: config.timeout_ms,
        colo_filter: config.colo_filter,
    };

    push_log("INFO", "测速启动...");

    let concurrency = GLOBAL_LIMITER.get().unwrap().max_concurrent();
    let mut tasks: JoinSet<PingResult> = JoinSet::new();

    let spawn_task = |ip: std::net::IpAddr, cfg: PingConfig| {
        async move { 
            http_ping_multi(ip, &cfg).await.map(|r| (r.addr, r.delay, r.colo, r.success_count, r.colo_mismatch))
        }
    };

    let mut result_cache: VecDeque<(SocketAddr, f32, Option<String>, u8)> = VecDeque::new();

    let try_fill_from_cache = |cache: &mut VecDeque<(SocketAddr, f32, Option<String>, u8)>| {
        while let Some((addr, delay, colo, success_count)) = cache.pop_front() {
            let result = lb.try_add_backend(addr, delay, colo.as_deref());
            
            match result {
                AddResult::AddedToPrimary | AddResult::AddedToBackup => {}
                AddResult::QueueFull | AddResult::AlreadyExists => {
                    cache.push_front((addr, delay, colo, success_count));
                    return;
                }
            }
        }
    };

    loop {
        if cancel_token.is_cancelled() {
            tasks.shutdown().await;
            break;
        }

        try_fill_from_cache(&mut result_cache);

        if lb.should_pause() {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    tasks.shutdown().await;
                    break;
                }
                _ = notify_rx.changed() => {
                    continue;
                }
            }
        }

        while tasks.len() < concurrency {
            let Some(ip) = ip_pool.pop() else {
                break;
            };

            if lb.contains(ip) {
                continue;
            }

            tasks.spawn(spawn_task(ip, ping_config.clone()));
        }

        tokio::select! {
            _ = cancel_token.cancelled() => {
                tasks.shutdown().await;
                break;
            }
            result = tasks.join_next() => {
                if let Some(Ok(result)) = result
                    && let Some((addr, delay, colo, success_count, colo_mismatch)) = result
                    && !colo_mismatch
                    && delay <= config.delay_limit as f32
                {
                    let add_result = lb.try_add_backend(addr, delay, colo.as_deref());

                    if let AddResult::QueueFull = add_result
                        && result_cache.len() < get_global_config().sample_window as usize
                    {
                        result_cache.push_back((addr, delay, colo, success_count));
                    }
                }
            }
        }
    }
}