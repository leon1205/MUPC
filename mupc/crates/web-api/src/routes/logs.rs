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
use std::sync::Arc;

use crate::AppState;
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

/// tracing JSON 日志行结构（用于解析 mupc.log 文件）
#[derive(Debug, Deserialize)]
struct JsonLogLine {
    timestamp: Option<String>,
    level: Option<String>,
    target: Option<String>,
    fields: Option<serde_json::Map<String, serde_json::Value>>,
}

/// 日志处理器
#[derive(Clone)]
pub struct LogsHandler {
    log_directory: PathBuf,
}

impl LogsHandler {
    pub fn new(log_directory: impl Into<PathBuf>) -> Self {
        Self {
            log_directory: log_directory.into(),
        }
    }

    /// 列出日志目录下的 mupc.log 文件（按名称排序）
    async fn list_log_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        let mut entries = match tokio::fs::read_dir(&self.log_directory).await {
            Ok(e) => e,
            Err(_) => return files,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("mupc.log") {
                        files.push(path);
                    }
                }
            }
        }
        files.sort();
        files
    }

    /// 获取日志列表
    pub async fn get_logs(&self, query: LogQuery) -> Result<LogListResponse, MupcError> {
        let limit = query.limit.unwrap_or(100).min(10000);
        let offset = query.offset.unwrap_or(0);

        let files = self.list_log_files().await;
        let mut entries: Vec<LogEntry> = Vec::new();

        // 从最新文件开始读（文件按名称排序，最新的在后）
        for file in files.iter().rev() {
            let content = tokio::fs::read_to_string(file).await.unwrap_or_default();
            for line in content.lines() {
                let Ok(json) = serde_json::from_str::<JsonLogLine>(line) else {
                    continue;
                };
                let timestamp = json.timestamp.unwrap_or_default();
                let level = json.level.unwrap_or_default();
                let module = json.target.unwrap_or_default();
                let message = json
                    .fields
                    .and_then(|f| f.get("message").and_then(|v| v.as_str().map(|s| s.to_string())))
                    .unwrap_or_default();

                // 级别过滤
                if let Some(lvl) = query.level {
                    let lvl_str = format!("{:?}", lvl).to_lowercase();
                    if !level.eq_ignore_ascii_case(&lvl_str) {
                        continue;
                    }
                }
                // 关键字过滤
                if let Some(kw) = &query.keyword {
                    if !message.contains(kw.as_str()) && !module.contains(kw.as_str()) {
                        continue;
                    }
                }
                // 时间范围过滤（ISO8601 字符串比较）
                if let Some(start) = &query.start_time {
                    if timestamp.as_str() < start.as_str() {
                        continue;
                    }
                }
                if let Some(end) = &query.end_time {
                    if timestamp.as_str() > end.as_str() {
                        continue;
                    }
                }

                entries.push(LogEntry {
                    timestamp,
                    level,
                    module,
                    message,
                });
            }
        }

        // 按时间戳倒序（新在前）
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        let total = entries.len();

        let paged = entries.into_iter().skip(offset).take(limit).collect();

        Ok(LogListResponse {
            total,
            entries: paged,
        })
    }

    /// 导出日志（拼接所有日志文件内容）
    pub async fn export_logs(&self, _query: LogQuery) -> Result<Vec<u8>, MupcError> {
        let files = self.list_log_files().await;
        let mut output = Vec::new();
        for file in files {
            if let Ok(content) = tokio::fs::read_to_string(file).await {
                output.extend_from_slice(content.as_bytes());
                output.push(b'\n');
            }
        }
        Ok(output)
    }
}

/// GET /api/v1/logs - 获取日志列表
async fn get_logs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LogQuery>,
) -> Result<Json<LogListResponse>, StatusCode> {
    state
        .logs_handler
        .get_logs(query)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// 创建日志路由
pub fn create_router() -> Router<Arc<AppState>> {
    Router::new().route("/api/v1/logs", get(get_logs))
}
