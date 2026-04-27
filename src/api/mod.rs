mod handlers;
mod routes;
mod sse;

pub use handlers::*;
pub use routes::create_router;
pub use sse::stream_updates;

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;

#[cfg(feature = "web")]
use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
#[cfg(feature = "web")]
use rust_embed::RustEmbed;

use crate::core::ServiceState;

pub struct ApiConfig {
    pub api_addr: SocketAddr,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            api_addr: "127.0.0.1:0".parse().unwrap(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<ServiceState>,
}

impl From<crate::core::StatusInfo> for StatusResponse {
    fn from(info: crate::core::StatusInfo) -> Self {
        Self {
            running: info.running,
            uptime_secs: info.uptime_secs,
            next_health_check: info.next_health_check,
            health_check_interval: info.health_check_interval,
            primary_count: info.primary_count,
            primary_target: info.primary_target,
            backup_count: info.backup_count,
            backup_target: info.backup_target,
            sticky_ips: info.sticky_ips,
            primary_ips: info.primary_ips,
            backup_ips: info.backup_ips,
        }
    }
}

impl From<&StartRequest> for crate::core::types::ConfigOverrides {
    fn from(req: &StartRequest) -> Self {
        Self {
            ip_file: req.ip_file.clone(),
            ip_content: req.ip_content.clone(),
            http: req.http.clone(),
            delay_limit: req.delay_limit,
            tlr: req.tlr,
            ips: req.ips,
            threads: req.threads,
            tls_port: req.tls_port,
            http_port: req.http_port,
            colo: req.colo.clone(),
            addr: req.addr,
            max_sticky_slots: req.max_sticky_slots,
            #[cfg(feature = "web")]
            api_addr: None,
        }
    }
}

#[derive(Serialize, PartialEq)]
pub struct StatusResponse {
    pub running: bool,
    pub uptime_secs: u64,
    pub next_health_check: u64,
    pub health_check_interval: u64,
    pub primary_count: usize,
    pub primary_target: usize,
    pub backup_count: usize,
    pub backup_target: usize,
    pub sticky_ips: Vec<String>,
    pub primary_ips: Vec<crate::core::IpInfo>,
    pub backup_ips: Vec<crate::core::IpInfo>,
}

#[derive(Deserialize)]
pub struct StartRequest {
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
}

#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
}

#[cfg(all(feature = "web", not(feature = "flutter-web")))]
#[derive(RustEmbed)]
#[folder = "web"]
struct Assets;

#[cfg(all(feature = "web", feature = "flutter-web"))]
#[derive(RustEmbed)]
#[folder = "flutter/build/web"]
struct Assets;

#[cfg(feature = "web")]
fn get_mime_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("js") => "application/javascript",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(feature = "web")]
pub async fn serve_embedded_files(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    
    if path.is_empty() || path == "index.html" {
        return match Assets::get("index.html") {
            Some(content) => (
                [(header::CONTENT_TYPE, "text/html")],
                content.data.into_owned(),
            ).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "text/html")],
                "<h1>404 - 页面未找到</h1><p>请先构建Flutter Web界面</p>".as_bytes().to_vec(),
            ).into_response(),
        };
    }
    
    match Assets::get(path) {
        Some(content) => {
            let mime = get_mime_type(path);
            ([(header::CONTENT_TYPE, mime)], content.data.into_owned()).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}