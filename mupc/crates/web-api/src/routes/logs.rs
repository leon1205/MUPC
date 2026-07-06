//! 日志查看 API

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use mupc_common::MupcError;

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// 日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub module: String,
    pub message: String,
}

/// 日志查询参数
#[derive(Debug, Clone, Deserialize)]
pub struct LogQuery {
    pub level: Option<LogLevel>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub keyword: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

/// 日志列表响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogListResponse {
    pub total: usize,
    pub entries: Vec<LogEntry>,
}

/// 日志处理器
#[derive(Clone)]
pub struct LogsHandler {
    #[allow(dead_code)]
    log_directory: PathBuf,
}

impl LogsHandler {
    pub fn new(log_directory: impl Into<PathBuf>) -> Self {
        Self {
            log_directory: log_directory.into(),
        }
    }

    /// 获取日志列表
    pub async fn get_logs(&self, query: LogQuery) -> Result<LogListResponse, MupcError> {
        let _limit = query.limit.unwrap_or(100).min(10000);
        let _offset = query.offset.unwrap_or(0);

        // TODO: 实际读取日志文件
        // 目前返回模拟数据
        let entries = Vec::new();

        Ok(LogListResponse { total: 0, entries })
    }

    /// 导出日志
    pub async fn export_logs(&self, _query: LogQuery) -> Result<Vec<u8>, MupcError> {
        // TODO: 实际导出日志文件
        Ok(Vec::new())
    }
}

/// GET /api/v1/logs - 获取日志列表
async fn get_logs(
    State(handler): State<LogsHandler>,
    Query(query): Query<LogQuery>,
) -> Result<Json<LogListResponse>, StatusCode> {
    handler
        .get_logs(query)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// 创建日志路由
pub fn create_router(handler: LogsHandler) -> Router {
    Router::new()
        .route("/api/v1/logs", get(get_logs))
        .with_state(handler)
}
