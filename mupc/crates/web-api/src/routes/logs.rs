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

    /// 获取日志列表（v3.1: 从日志目录读取最近条目）
    pub async fn get_logs(&self, query: LogQuery) -> Result<LogListResponse, MupcError> {
        let limit = query.limit.unwrap_or(100).min(10000);
        let offset = query.offset.unwrap_or(0);

        let mut entries = Vec::new();

        // 从日志目录读取 .log 文件
        if let Ok(dir) = std::fs::read_dir(&self.log_directory) {
            let mut log_files: Vec<_> = dir
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "log"))
                .collect();
            // 按修改时间排序（最新的在前）
            log_files.sort_by(|a, b| {
                b.metadata()
                    .and_then(|m| m.modified())
                    .cmp(&a.metadata().and_then(|m| m.modified()))
            });

            for file in log_files.iter().take(3) {
                // 只读最新 3 个日志文件
                if let Ok(content) = std::fs::read_to_string(file.path()) {
                    for line in content.lines().rev().take(limit + offset) {
                        // 简易解析: 跳过不足的行
                        if entries.len() >= limit + offset {
                            break;
                        }
                        // 尝试解析 tracing 格式的日志行
                        if let Some(entry) = Self::parse_log_line(line) {
                            // 按级别过滤
                            if let Some(ref lvl) = query.level {
                                if entry.level != format!("{:?}", lvl).to_uppercase() {
                                    continue;
                                }
                            }
                            // 按关键词过滤
                            if let Some(ref kw) = query.keyword {
                                if !entry.message.contains(kw.as_str()) {
                                    continue;
                                }
                            }
                            entries.push(entry);
                        }
                    }
                }
                if entries.len() >= limit + offset {
                    break;
                }
            }
        }

        let total = entries.len();
        let entries = entries
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect();

        Ok(LogListResponse { total, entries })
    }

    /// 解析单行日志为 LogEntry（简易解析，兼容 tracing 默认格式）
    fn parse_log_line(line: &str) -> Option<LogEntry> {
        // tracing 默认格式: "2026-06-26T10:30:00.123Z  INFO module: message"
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return None;
        }
        let timestamp = parts[0].to_string();
        let rest = parts[1].trim();

        let rest_parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let level = rest_parts.first()?.to_string();
        let module_msg = rest_parts.get(1).unwrap_or(&"");

        let msg_parts: Vec<&str> = module_msg.splitn(2, ':').collect();
        let (module, message) = if msg_parts.len() == 2 {
            (msg_parts[0].trim().to_string(), msg_parts[1].trim().to_string())
        } else {
            (String::new(), module_msg.trim().to_string())
        };

        Some(LogEntry {
            timestamp,
            level,
            module,
            message,
        })
    }

    /// 导出日志（v3.1: 读取并打包为文本返回）
    pub async fn export_logs(&self, query: LogQuery) -> Result<Vec<u8>, MupcError> {
        let response = self.get_logs(query).await?;
        let mut output = String::new();
        for entry in &response.entries {
            output.push_str(&format!(
                "{} {} {}: {}\n",
                entry.timestamp, entry.level, entry.module, entry.message
            ));
        }
        Ok(output.into_bytes())
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
