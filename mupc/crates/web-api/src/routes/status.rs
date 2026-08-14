//! 状态查看 API

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;
use mupc_common::MupcError;

/// 系统状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub firmware_version: String,
    pub build_time: String,
    pub uptime_secs: u64,
    pub cpu_temperature: Option<f64>,
    pub memory_usage: Option<f64>,
    pub iec104_status: String,
    pub intercore_status: String,
    pub ai_engine_status: String,
    pub strategy_mode: String,
    pub recent_alarms: Vec<AlarmEntry>,
}

/// 告警条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmEntry {
    pub time: String,
    pub level: String,
    pub message: String,
}

/// 状态处理器
#[derive(Clone)]
pub struct StatusHandler {
    start_time: std::time::Instant,
}

impl Default for StatusHandler {
    fn default() -> Self {
        Self::new()
    }
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
            cpu_temperature: None,
            memory_usage: None,
            iec104_status: "unknown".to_string(),
            intercore_status: "unknown".to_string(),
            ai_engine_status: "unknown".to_string(),
            strategy_mode: "unknown".to_string(),
            recent_alarms: Vec::new(),
        })
    }
}

/// GET /api/v1/status - 获取系统状态
async fn get_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SystemStatus>, StatusCode> {
    state
        .status_handler
        .get_status()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// 创建状态路由
pub fn create_router() -> Router<Arc<AppState>> {
    Router::new().route("/api/v1/status", get(get_status))
}
