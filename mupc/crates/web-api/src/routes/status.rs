//! 状态查看 API

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use mupc_common::MupcError;

/// 系统状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub firmware_version: String,
    pub build_time: String,
    pub uptime_secs: u64,
    pub cpu_temperature: Option<f64>,
    pub memory_usage: Option<f64>,
    pub connections: ConnectionStatus,
}

/// 连接状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub iec104_connected: bool,
    pub iec104_count: usize,
    pub intercore_connected: bool,
}

/// 状态处理器
#[derive(Clone)]
pub struct StatusHandler {
    start_time: std::time::Instant,
}

impl StatusHandler {
    pub fn new() -> Self {
        Self {
            start_time: std::time::Instant::now(),
        }
    }

    /// 获取系统状态
    pub async fn get_status(&self) -> Result<SystemStatus, MupcError> {
        let uptime = self.start_time.elapsed().as_secs();

        Ok(SystemStatus {
            firmware_version: env!("CARGO_PKG_VERSION").to_string(),
            build_time: env!("CARGO_PKG_VERSION").to_string(),
            uptime_secs: uptime,
            cpu_temperature: None, // TODO: 从系统获取
            memory_usage: None, // TODO: 从系统获取
            connections: ConnectionStatus {
                iec104_connected: false, // TODO: 从 gateway 获取
                iec104_count: 0,
                intercore_connected: false, // TODO: 从 intercore 获取
            },
        })
    }
}

/// GET /api/v1/status - 获取系统状态
async fn get_status(
    State(handler): State<StatusHandler>,
) -> Result<Json<SystemStatus>, StatusCode> {
    handler
        .get_status()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// 创建状态路由
pub fn create_router(handler: StatusHandler) -> Router {
    Router::new()
        .route("/api/v1/status", get(get_status))
        .with_state(handler)
}