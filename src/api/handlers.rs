use axum::{
    extract::State,
    Json,
};

use super::{AppState, ApiResponse, StartRequest, StatusResponse};
use crate::core::{ServiceConfig, types::ConfigOverrides};
use crate::log::get_log_buffer;

pub async fn get_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let info = state.service.build_full_status();
    Json(StatusResponse::from(info))
}

pub async fn get_config(State(state): State<AppState>) -> Json<ServiceConfig> {
    Json(state.service.get_config())
}

pub async fn start_service(
    State(state): State<AppState>,
    Json(req): Json<StartRequest>,
) -> Json<ApiResponse> {
    let mut config = state.service.get_config();

    config.apply_overrides(&ConfigOverrides::from(&req));

    state.service.update_config(config);

    let result = state.service.start_with_ips(
        req.ip_file.as_deref(),
        req.ip_content.as_deref(),
    );

    match result {
        Ok(_) => Json(ApiResponse {
            success: true,
            message: "服务已启动".to_string(),
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            message: e,
        }),
    }
}

pub async fn stop_service(State(state): State<AppState>) -> Json<ApiResponse> {
    match state.service.stop().await {
        Ok(_) => Json(ApiResponse {
            success: true,
            message: "服务已停止".to_string(),
        }),
        Err(e) => Json(ApiResponse {
            success: false,
            message: e,
        }),
    }
}

pub async fn health_check() -> Json<ApiResponse> {
    Json(ApiResponse {
        success: true,
        message: "服务运行正常".to_string(),
    })
}

pub async fn get_logs() -> Json<Vec<crate::log::LogEntry>> {
    Json(get_log_buffer().get_all())
}

pub async fn clear_logs() -> Json<ApiResponse> {
    get_log_buffer().clear();
    Json(ApiResponse {
        success: true,
        message: "日志已清空".to_string(),
    })
}