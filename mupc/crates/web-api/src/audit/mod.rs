//! Web 操作审计日志
//!
//! 记录所有 Web API 操作，委托到 security crate 的 JSONL + SM3 哈希链审计方案。
//!
//! 本模块定义了 Web 层专用的 `WebAuditEntry` 结构体，并提供了到
//! `mupc_security::audit::AuditLogEntry` 格式的转换。
//!
//! 实际存储由 `mupc_security::audit::AuditLogger` 负责。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use mupc_security::audit::{AuditEventType, AuditSeverity, AuditLogEntry};

/// 审计日志条目（Web 层专用格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuditEntry {
    /// 日志条目唯一标识
    pub id: String,
    /// 操作时间戳
    pub timestamp: DateTime<Utc>,
    /// 操作用户
    pub user: String,
    /// 用户角色
    pub role: String,
    /// 操作动作
    pub action: String,
    /// 操作的资源路径
    pub resource: String,
    /// HTTP 方法
    pub method: String,
    /// HTTP 状态码
    pub status_code: u16,
    /// 客户端 IP 地址
    pub ip_address: String,
    /// 客户端 User-Agent
    pub user_agent: String,
}

impl WebAuditEntry {
    /// 转换为 security crate 的 AuditLogEntry 格式
    ///
    /// 字段映射：
    /// - `user` + `role` -> `operator`
    /// - `action` + `resource` + `method` + `status_code` -> `message`
    /// - `ip_address` -> 直接映射
    /// - `audit_log_entry` 中缺失的字段（sequence, sm3_chain_hash, prev_sm3_hash）
    ///   由 security::AuditLogger 自行填充
    pub fn to_security_entry(&self) -> AuditLogEntry {
        let message = format!(
            "[{} {} {}] {} {} ({} {} -> {})",
            self.method,
            self.resource,
            self.status_code,
            self.action,
            if self.user_agent.is_empty() {
                String::new()
            } else {
                format!("[{}]", self.user_agent)
            },
            self.user,
            self.role,
            self.ip_address
        );

        AuditLogEntry {
            sequence: 0, // 将由 security::AuditLogger 填充
            timestamp: self.timestamp,
            event_type: AuditEventType::GenericOperation,
            severity: audit_severity_from_status(self.status_code),
            source: "web-api".to_string(),
            message,
            operator: format!("{} ({})", self.user, self.role),
            ip_address: self.ip_address.clone(),
            sm3_chain_hash: String::new(), // 将由 security::AuditLogger 填充
            prev_sm3_hash: String::new(),  // 将由 security::AuditLogger 填充
        }
    }
}

/// 审计日志记录器
///
/// 包装 `mupc_security::audit::AuditLogger`，提供 Web 层专用的便捷接口。
pub struct AuditLogger {
    inner: mupc_security::audit::AuditLogger,
}

impl AuditLogger {
    /// 创建审计日志记录器
    ///
    /// `log_dir`: 审计日志目录路径（例如 `/var/log/mupc/audit`）
    pub fn new(log_dir: &str) -> Result<Self, String> {
        let inner = mupc_security::audit::AuditLogger::new(log_dir)
            .map_err(|e| format!("创建审计日志记录器失败: {}", e))?;
        Ok(Self { inner })
    }

    /// 记录一条审计日志
    ///
    /// 将 `WebAuditEntry` 映射到 security 的日志格式并写入 JSONL 文件。
    pub async fn log(&self, entry: WebAuditEntry) -> Result<(), String> {
        // 注意：security::AuditLogger::log 需要 &mut self
        // 但由于 Web 层并发访问，这里使用内部可变性策略：
        // 直接调用 security 的 log，内部状态通过代码控制
        //
        // 实际使用中，AuditLogger 应该被 Arc<Mutex<>> 包裹使用
        tracing::warn!(
            "审计日志记录需要通过 Arc<Mutex<AuditLogger>> 调用，直接调用暂不支持。条目: {:?}",
            entry.id
        );
        Err("AuditLogger 需要可变引用，请使用 Arc<Mutex<AuditLogger>> 包装".to_string())
    }

    /// 记录一条审计日志（可变引用版本）
    ///
    /// 此方法需要 `&mut self`，适用于单线程或已加锁的上下文。
    pub fn log_sync(&mut self, entry: WebAuditEntry) -> Result<(), String> {
        let security_entry = entry.to_security_entry();
        self.inner
            .log(
                security_entry.event_type,
                security_entry.severity,
                &security_entry.source,
                &security_entry.message,
                &security_entry.operator,
                &security_entry.ip_address,
            )
            .map_err(|e| format!("审计日志写入失败: {}", e))
    }

    /// 查询审计日志
    ///
    /// `start`: 查询起始时间
    /// `end`: 查询截止时间
    /// `user`: 可选的用户名过滤（在结果集中过滤）
    pub async fn query(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        user: Option<&str>,
    ) -> Result<Vec<WebAuditEntry>, String> {
        let entries = self
            .inner
            .query(start, end)
            .map_err(|e| format!("审计日志查询失败: {}", e))?;

        let mut results: Vec<WebAuditEntry> = entries
            .into_iter()
            .map(|e| convert_to_web_entry(&e))
            .collect();

        // 按用户名过滤
        if let Some(u) = user {
            results.retain(|e| e.user == u);
        }

        Ok(results)
    }

    /// 导出审计日志为 CSV 文件
    ///
    /// 委托到 security::AuditLogger::export，返回导出的条目数量。
    pub async fn export_csv(&self, output_path: &str) -> Result<usize, String> {
        self.inner
            .export(output_path)
            .map_err(|e| format!("审计日志导出失败: {}", e))?;

        // export 不返回数量，重新查询来计算
        let entries = self
            .inner
            .query(DateTime::UNIX_EPOCH, Utc::now())
            .map_err(|e| format!("审计日志导出计数查询失败: {}", e))?;

        Ok(entries.len())
    }

    /// 验证审计日志哈希链完整性
    pub fn verify_chain(&self) -> Result<bool, String> {
        self.inner
            .verify_chain()
            .map_err(|e| format!("审计日志哈希链验证失败: {}", e))
    }

    /// 刷新审计日志到磁盘
    pub fn flush(&mut self) -> Result<(), String> {
        self.inner
            .flush()
            .map_err(|e| format!("审计日志刷盘失败: {}", e))
    }

    /// 清理过期日志
    ///
    /// 注意：当前实现不执行实际删除，仅返回 0。
    /// Phase 2+ 实现基于 retention_days 的自动清理。
    pub async fn purge_old(&self, _before: DateTime<Utc>) -> Result<usize, String> {
        tracing::info!("审计日志清理请求（Phase 2+ 实现自动清理）");
        Ok(0)
    }

    /// 获取内部 security::AuditLogger 的引用
    pub fn inner(&self) -> &mupc_security::audit::AuditLogger {
        &self.inner
    }

    /// 获取内部 security::AuditLogger 的可变引用
    pub fn inner_mut(&mut self) -> &mut mupc_security::audit::AuditLogger {
        &mut self.inner
    }
}

// ========== 辅助函数 ==========

/// 根据 HTTP 状态码确定审计严重级别
fn audit_severity_from_status(status_code: u16) -> AuditSeverity {
    match status_code {
        200..=299 => AuditSeverity::Info,
        400..=499 => AuditSeverity::Warning,
        500..=599 => AuditSeverity::Error,
        _ => AuditSeverity::Info,
    }
}

/// 将 security::AuditLogEntry 转换为 WebAuditEntry
fn convert_to_web_entry(entry: &AuditLogEntry) -> WebAuditEntry {
    // 从消息中尝试解析 HTTP 方法和资源
    let (method, resource, status_code) = parse_http_from_message(&entry.message);

    WebAuditEntry {
        id: format!("audit-{:016}", entry.sequence),
        timestamp: entry.timestamp,
        user: entry.operator.clone(),
        role: String::new(),
        action: format!("{:?}", entry.event_type),
        resource,
        method,
        status_code,
        ip_address: entry.ip_address.clone(),
        user_agent: String::new(),
    }
}

/// 从消息中解析 HTTP 方法和资源路径
fn parse_http_from_message(message: &str) -> (String, String, u16) {
    // 消息格式: [GET /api/devices 200] ...
    if let Some(bracket_start) = message.find('[') {
        let bracket_end = message.find(']').unwrap_or(message.len());
        let inside = &message[bracket_start + 1..bracket_end];
        let parts: Vec<&str> = inside.split_whitespace().collect();
        if parts.len() >= 3 {
            let method = parts[0].to_string();
            let resource = parts[1].to_string();
            let status: u16 = parts[2].parse().unwrap_or(0);
            return (method, resource, status);
        }
    }
    (String::new(), String::new(), 0)
}
