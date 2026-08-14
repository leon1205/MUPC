//! 加密审计日志
//!
//! 使用 SM3 哈希链保证审计日志防篡改，支持 JSONL 格式持久化。
//! 当前使用 SHA-256 替代 SM3（Phase 2+ 替换为国密 SM3）。
//!
//! ## 存储规范
//! - 日志目录：`/var/log/mupc/audit/`
//! - 文件命名：`audit_YYYY-MM-DD.jsonl`（按天分文件）
//! - 每行一条 JSON 格式的审计记录
//!
//! ## 哈希链
//! - 链首哈希 = SHA-256(genesis_seed)
//! - 每条日志的 sm3_chain_hash = SHA-256(prev_hash || entry_json)

use crate::errors::SecurityError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// 审计负载（用于哈希计算，不含哈希字段自身）
#[derive(Debug, Serialize)]
struct AuditPayload {
    sequence: u64,
    timestamp: DateTime<Utc>,
    event_type: AuditEventType,
    severity: AuditSeverity,
    source: String,
    message: String,
    operator: String,
    ip_address: String,
}

/// 哈希链创世种子（Phase 2+ 从安全存储读取）
const GENESIS_SEED: &[u8] = b"MUPC_AUDIT_GENESIS_SEED_V1";

/// 审计日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    /// 日志序号（从 1 开始递增）
    pub sequence: u64,
    /// 操作时间戳
    pub timestamp: DateTime<Utc>,
    /// 事件类型
    pub event_type: AuditEventType,
    /// 严重级别
    pub severity: AuditSeverity,
    /// 事件来源（如 "web-api", "gateway", "strategy-engine"）
    pub source: String,
    /// 操作描述
    pub message: String,
    /// 操作者标识（用户或系统组件）
    pub operator: String,
    /// 客户端 IP 地址（如适用）
    pub ip_address: String,
    /// SM3/SHA-256 链式哈希
    pub sm3_chain_hash: String,
    /// 前一条日志的哈希
    pub prev_sm3_hash: String,
}

/// 审计事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEventType {
    /// 证书导入
    CertImported,
    /// 证书即将过期
    CertExpiring,
    /// 证书吊销
    CertRevoked,
    /// 隧道建立
    TunnelEstablished,
    /// 隧道关闭
    TunnelClosed,
    /// 密钥更新完成
    RekeyCompleted,
    /// 策略变更
    PolicyChanged,
    /// 安全启动失败
    SecureBootFailed,
    /// 完整性违规
    IntegrityViolation,
    /// 未授权访问
    UnauthorizedAccess,
    /// 合规检查失败
    ComplianceCheckFailed,
    /// 用户登录
    UserLogin,
    /// 用户登出
    UserLogout,
    /// 配置变更
    ConfigChanged,
    /// 设备控制操作
    DeviceControl,
    /// 固件升级
    FirmwareUpdate,
    /// 系统启动
    SystemStartup,
    /// 系统关闭
    SystemShutdown,
    /// 通用操作
    GenericOperation,
}

/// 审计严重级别
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// 审计日志记录器（JSONL + SHA-256 哈希链）
///
/// # 示例
/// ```no_run
/// use mupc_security::audit::{AuditLogger, AuditEventType, AuditSeverity};
///
/// let mut logger = AuditLogger::new("/var/log/mupc/audit").unwrap();
/// logger.log(
///     AuditEventType::UserLogin,
///     AuditSeverity::Info,
///     "web-api",
///     "用户 admin 登录成功",
///     "admin",
///     "192.168.1.100",
/// ).unwrap();
/// ```
pub struct AuditLogger {
    /// 日志目录路径
    log_dir: PathBuf,
    /// 当前日志文件路径
    current_file: PathBuf,
    /// 当前日志文件句柄
    file_handle: Option<File>,
    /// 当前哈希链的最后一个哈希值
    chain_hash: String,
    /// 当前序列号
    sequence: u64,
    /// 距上次 flush 的条目数
    entries_since_flush: usize,
    /// 当前日志文件对应的日期（用于跨天切换）
    current_date: String,
}

impl AuditLogger {
    /// 创建审计日志记录器
    ///
    /// - 创建日志目录（递归）
    /// - 读取最后一条已存在的日志记录以恢复哈希链状态
    /// - 如果日志目录为空，从创世种子初始化哈希链
    pub fn new(log_dir: &str) -> Result<Self, SecurityError> {
        let log_dir = PathBuf::from(log_dir);

        // 创建日志目录
        fs::create_dir_all(&log_dir).map_err(|e| {
            SecurityError::IoError(format!("创建审计日志目录失败 {}: {}", log_dir.display(), e))
        })?;

        tracing::info!("审计日志目录已就绪: {}", log_dir.display());

        // 获取当前日期
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let current_file = log_dir.join(format!("audit_{}.jsonl", today));

        let mut logger = AuditLogger {
            log_dir,
            current_file,
            file_handle: None,
            chain_hash: String::new(),
            sequence: 0,
            entries_since_flush: 0,
            current_date: today,
        };

        // 尝试从最新日志文件中恢复哈希链状态
        logger.recover_chain_state()?;

        // 打开发布日的日志文件（追加模式）
        logger.open_current_file()?;

        tracing::info!(
            "审计日志记录器初始化完成: 序列号={}, 哈希链已就绪",
            logger.sequence
        );

        Ok(logger)
    }

    /// 记录一条审计日志
    pub fn log(
        &mut self,
        event_type: AuditEventType,
        severity: AuditSeverity,
        source: &str,
        message: &str,
        operator: &str,
        ip_address: &str,
    ) -> Result<(), SecurityError> {
        // 检查是否需要切换日期文件
        self.check_date_rollover()?;

        let timestamp = Utc::now();
        self.sequence += 1;

        let prev_hash = self.chain_hash.clone();

        // 构建审计负载（用于计算链式哈希，不含哈希字段自身）
        let payload = AuditPayload {
            sequence: self.sequence,
            timestamp,
            event_type: event_type.clone(),
            severity: severity.clone(),
            source: source.to_string(),
            message: message.to_string(),
            operator: operator.to_string(),
            ip_address: ip_address.to_string(),
        };

        let payload_str = serde_json::to_string(&payload)
            .map_err(|e| SecurityError::AuditError(format!("序列化审计条目失败: {}", e)))?;

        // 计算链式哈希: SHA-256(prev_hash || payload_json)
        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(payload_str.as_bytes());
        let chain_hash = hex::encode(hasher.finalize());

        // 构建完整条目（包含哈希字段）
        let full_entry = AuditLogEntry {
            sequence: self.sequence,
            timestamp,
            event_type: event_type.clone(),
            severity: severity.clone(),
            source: source.to_string(),
            message: message.to_string(),
            operator: operator.to_string(),
            ip_address: ip_address.to_string(),
            sm3_chain_hash: chain_hash.clone(),
            prev_sm3_hash: prev_hash,
        };

        let full_json = serde_json::to_string(&full_entry)
            .map_err(|e| SecurityError::AuditError(format!("序列化完整条目失败: {}", e)))?;

        // 追加写入 JSONL 文件
        let file = self
            .file_handle
            .as_mut()
            .ok_or_else(|| SecurityError::AuditError("审计日志文件未打开".to_string()))?;

        writeln!(file, "{}", full_json)
            .map_err(|e| SecurityError::IoError(format!("写入审计日志失败: {}", e)))?;

        // 更新哈希链状态
        self.chain_hash = chain_hash;
        self.entries_since_flush += 1;

        // 每 10 条日志自动 flush
        if self.entries_since_flush >= 10 {
            self.flush()?;
        }

        tracing::debug!(
            "审计日志已记录: seq={}, type={:?}, severity={:?}",
            self.sequence,
            event_type,
            severity
        );

        Ok(())
    }

    /// 刷新审计日志到磁盘（fsync）
    pub fn flush(&mut self) -> Result<(), SecurityError> {
        if let Some(file) = self.file_handle.as_mut() {
            file.flush()
                .map_err(|e| SecurityError::IoError(format!("fsync 审计日志失败: {}", e)))?;
            self.entries_since_flush = 0;

            tracing::debug!("审计日志已刷新到磁盘");
        }
        Ok(())
    }

    /// 验证 SM3 哈希链完整性
    ///
    /// 重新计算所有审计日志的链式哈希，验证是否被篡改。
    /// 返回 true 表示完整性验证通过。
    pub fn verify_chain(&self) -> Result<bool, SecurityError> {
        tracing::info!("开始验证审计日志哈希链...");

        let mut expected_hash = compute_genesis_hash();

        // 收集所有日志文件并按日期排序
        let files = self.list_audit_files()?;

        for file_path in &files {
            let f = File::open(file_path).map_err(|e| {
                SecurityError::IoError(format!("打开审计日志 {} 失败: {}", file_path.display(), e))
            })?;

            let reader = BufReader::new(f);
            for (line_no, line) in reader.lines().enumerate() {
                let line = line.map_err(|e| {
                    SecurityError::IoError(format!(
                        "读取审计日志 {} 行 {} 失败: {}",
                        file_path.display(),
                        line_no + 1,
                        e
                    ))
                })?;

                if line.trim().is_empty() {
                    continue;
                }

                // 解析完整的 AuditLogEntry
                let entry: AuditLogEntry = serde_json::from_str(&line).map_err(|e| {
                    SecurityError::AuditError(format!(
                        "解析审计日志 {} 行 {} 失败: {}",
                        file_path.display(),
                        line_no + 1,
                        e
                    ))
                })?;

                // 验证前驱哈希连续性
                if entry.prev_sm3_hash != expected_hash {
                    tracing::error!(
                        "哈希链断裂: 文件={}, 行={}, 期望前驱={}, 实际前驱={}",
                        file_path.display(),
                        line_no + 1,
                        expected_hash,
                        entry.prev_sm3_hash
                    );
                    return Ok(false);
                }

                // 重新计算哈希以检测负载篡改
                // 使用与 log() 完全相同的 AuditPayload 序列化方式
                let payload = AuditPayload {
                    sequence: entry.sequence,
                    timestamp: entry.timestamp,
                    event_type: entry.event_type,
                    severity: entry.severity,
                    source: entry.source,
                    message: entry.message,
                    operator: entry.operator,
                    ip_address: entry.ip_address,
                };
                let payload_str = serde_json::to_string(&payload)
                    .map_err(|e| SecurityError::AuditError(format!("序列化验证负载失败: {}", e)))?;

                let mut hasher = Sha256::new();
                hasher.update(expected_hash.as_bytes());
                hasher.update(payload_str.as_bytes());
                let computed_hash = hex::encode(hasher.finalize());

                if computed_hash != entry.sm3_chain_hash {
                    tracing::error!(
                        "哈希不匹配: 文件={}, 行={}, 计算={}, 存储={}",
                        file_path.display(),
                        line_no + 1,
                        computed_hash,
                        entry.sm3_chain_hash
                    );
                    return Ok(false);
                }

                expected_hash = entry.sm3_chain_hash;
            }
        }

        tracing::info!("审计日志哈希链验证通过: 共 {} 个文件", files.len());
        Ok(true)
    }

    /// 按时间范围查询审计日志
    ///
    /// 遍历所有日志文件，筛选出时间范围内的条目。
    pub fn query(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<AuditLogEntry>, SecurityError> {
        let files = self.list_audit_files()?;
        let mut results = Vec::new();

        for file_path in &files {
            // 根据文件名日期快速跳过不需要的文件
            if let Some(file_date) = extract_date_from_filename(file_path) {
                let file_start: DateTime<Utc> = format!("{}T00:00:00Z", file_date)
                    .parse()
                    .unwrap_or(DateTime::UNIX_EPOCH);
                let file_end: DateTime<Utc> = format!("{}T23:59:59Z", file_date)
                    .parse()
                    .unwrap_or(DateTime::UNIX_EPOCH);
                // 如果文件日期不在查询范围内，跳过
                if file_end < start || file_start > end {
                    continue;
                }
            }

            let f = File::open(file_path).map_err(|e| {
                SecurityError::IoError(format!("打开审计日志 {} 失败: {}", file_path.display(), e))
            })?;

            let reader = BufReader::new(f);
            for line in reader.lines() {
                let line =
                    line.map_err(|e| SecurityError::IoError(format!("读取审计日志失败: {}", e)))?;

                if line.trim().is_empty() {
                    continue;
                }

                if let Ok(entry) = serde_json::from_str::<AuditLogEntry>(&line) {
                    if entry.timestamp >= start && entry.timestamp <= end {
                        results.push(entry);
                    }
                }
            }
        }

        // 按时间戳排序
        results.sort_by_key(|e| e.timestamp);

        tracing::info!(
            "审计日志查询完成: 时间范围 {} - {}, 结果 {} 条",
            start,
            end,
            results.len()
        );

        Ok(results)
    }

    /// 将查询结果导出为 CSV 文件
    ///
    /// CSV 包含以下列：
    /// sequence, timestamp, event_type, severity, source, message, operator, ip_address, sm3_chain_hash, prev_sm3_hash
    ///
    /// 返回导出的条目数量。
    pub fn export(&self, output_path: &str) -> Result<usize, SecurityError> {
        let files = self.list_audit_files()?;
        let output_path = Path::new(output_path);

        let mut output = File::create(output_path).map_err(|e| {
            SecurityError::IoError(format!(
                "创建导出文件 {} 失败: {}",
                output_path.display(),
                e
            ))
        })?;

        // 写入 CSV 表头
        writeln!(
            output,
            "sequence,timestamp,event_type,severity,source,message,operator,ip_address,sm3_chain_hash,prev_sm3_hash"
        )
        .map_err(|e| SecurityError::IoError(format!("写入 CSV 表头失败: {}", e)))?;

        let mut count = 0;

        for file_path in &files {
            let f = File::open(file_path).map_err(|e| {
                SecurityError::IoError(format!("打开审计日志 {} 失败: {}", file_path.display(), e))
            })?;

            let reader = BufReader::new(f);
            for line in reader.lines() {
                let line =
                    line.map_err(|e| SecurityError::IoError(format!("读取审计日志失败: {}", e)))?;

                if line.trim().is_empty() {
                    continue;
                }

                if let Ok(entry) = serde_json::from_str::<AuditLogEntry>(&line) {
                    // 转义 CSV 字段中的逗号和引号
                    let csv_line = format!(
                        "{},{},{:?},{:?},{},{},{},{},{},{}",
                        entry.sequence,
                        entry.timestamp.to_rfc3339(),
                        entry.event_type,
                        entry.severity,
                        csv_escape(&entry.source),
                        csv_escape(&entry.message),
                        csv_escape(&entry.operator),
                        csv_escape(&entry.ip_address),
                        entry.sm3_chain_hash,
                        entry.prev_sm3_hash
                    );
                    writeln!(output, "{}", csv_line)
                        .map_err(|e| SecurityError::IoError(format!("写入 CSV 行失败: {}", e)))?;
                    count += 1;
                }
            }
        }

        output
            .flush()
            .map_err(|e| SecurityError::IoError(format!("刷新导出文件失败: {}", e)))?;

        tracing::info!(
            "审计日志已导出到 {}: {} 条记录",
            output_path.display(),
            count
        );

        Ok(count)
    }

    /// 获取当前序列号
    pub fn current_sequence(&self) -> u64 {
        self.sequence
    }

    /// 获取当前哈希链的最后一个哈希值
    pub fn current_chain_hash(&self) -> &str {
        &self.chain_hash
    }
}

// ========== 私有方法 ==========

impl AuditLogger {
    /// 打开发布日的日志文件
    fn open_current_file(&mut self) -> Result<(), SecurityError> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.current_file)
            .map_err(|e| {
                SecurityError::IoError(format!(
                    "打开审计日志文件 {} 失败: {}",
                    self.current_file.display(),
                    e
                ))
            })?;

        self.file_handle = Some(file);
        Ok(())
    }

    /// 检查是否需要切换到新日期的文件
    fn check_date_rollover(&mut self) -> Result<(), SecurityError> {
        let today = Utc::now().format("%Y-%m-%d").to_string();

        if today != self.current_date {
            tracing::info!("审计日志日期切换: {} -> {}", self.current_date, today);

            // 刷新旧文件
            self.flush()?;

            // 更新日期和文件路径
            self.current_date = today;
            self.current_file = self
                .log_dir
                .join(format!("audit_{}.jsonl", self.current_date));

            // 打开发布日文件
            self.open_current_file()?;
        }

        Ok(())
    }

    /// 从磁盘中恢复哈希链状态
    ///
    /// 找到最后一条日志记录，读取其哈希值来初始化链状态。
    /// 如果没有任何日志，使用创世哈希。
    fn recover_chain_state(&mut self) -> Result<(), SecurityError> {
        let mut last_hash = compute_genesis_hash();
        let mut last_sequence: u64 = 0;

        // 按日期顺序读取所有日志文件，找到最后一条记录
        let files = self.list_audit_files()?;

        // 打开当前日期的文件以恢复最新状态
        if !files.is_empty() {
            // 读取最后一个文件以恢复状态
            for file_path in &files {
                if let Ok(f) = File::open(file_path) {
                    let reader = BufReader::new(f);
                    for line in reader.lines().map_while(Result::ok) {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Ok(entry) = serde_json::from_str::<AuditLogEntry>(&line) {
                            last_sequence = entry.sequence;
                            last_hash = entry.sm3_chain_hash;
                        }
                    }
                }
            }
        }

        self.chain_hash = last_hash;
        self.sequence = last_sequence;

        if last_sequence > 0 {
            tracing::info!(
                "从磁盘恢复审计日志状态: 序列号={}, 最后哈希={}",
                last_sequence,
                &self.chain_hash[..16]
            );
        }

        Ok(())
    }

    /// 列出所有审计日志文件（按日期排序）
    fn list_audit_files(&self) -> Result<Vec<PathBuf>, SecurityError> {
        let mut files: Vec<PathBuf> = Vec::new();

        let entries = fs::read_dir(&self.log_dir).map_err(|e| {
            SecurityError::IoError(format!(
                "读取审计日志目录 {} 失败: {}",
                self.log_dir.display(),
                e
            ))
        })?;

        for entry in entries {
            let entry =
                entry.map_err(|e| SecurityError::IoError(format!("读取目录条目失败: {}", e)))?;
            let path = entry.path();

            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("audit_") && name.ends_with(".jsonl") {
                        files.push(path);
                    }
                }
            }
        }

        // 按文件名排序（默认即按日期排序）
        files.sort();

        Ok(files)
    }

    /// 清理早于指定时间的审计日志文件（跳过当前正在写入的文件）
    pub fn purge_old(&self, before: DateTime<Utc>) -> Result<usize, SecurityError> {
        let files = self.list_audit_files()?;
        let mut removed = 0;
        for file in files {
            if file == self.current_file {
                continue; // 跳过当前文件
            }
            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let date_str = name.strip_prefix("audit_").and_then(|s| s.strip_suffix(".jsonl"));
            if let Some(date_str) = date_str {
                if let Ok(naive) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                    if let Some(file_date) = naive.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc()) {
                        if file_date < before {
                            if let Err(e) = fs::remove_file(&file) {
                                tracing::warn!("删除审计日志 {} 失败: {}", file.display(), e);
                            } else {
                                removed += 1;
                            }
                        }
                    }
                }
            }
        }
        Ok(removed)
    }
}

impl Drop for AuditLogger {
    fn drop(&mut self) {
        // 尝试在析构时刷盘
        let _ = self.flush();
    }
}

// ========== 辅助函数 ==========

/// 计算创世哈希: SHA-256(genesis_seed)
fn compute_genesis_hash() -> String {
    let mut hasher = Sha256::new();
    hasher.update(GENESIS_SEED);
    hex::encode(hasher.finalize())
}

/// 从文件名中提取日期字符串
/// 文件命名格式: audit_YYYY-MM-DD.jsonl
fn extract_date_from_filename(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    if name.starts_with("audit_") && name.ends_with(".jsonl") {
        let inner = &name[6..name.len() - 6]; // "audit_" 前缀 + ".jsonl" 后缀 = 11 字节
        if inner.len() == 10 && inner.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Some(inner.to_string());
        }
    }
    None
}

/// CSV 字段转义：如果包含逗号、引号或换行符，用引号包裹
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_logger() -> (AuditLogger, TempDir) {
        let dir = TempDir::new().unwrap();
        let log_dir = dir.path().to_str().unwrap();
        let logger = AuditLogger::new(log_dir).unwrap();
        (logger, dir)
    }

    #[test]
    fn test_new_logger_creates_directory() {
        let dir = TempDir::new().unwrap();
        let log_path = dir.path().join("audit_logs");
        let log_dir = log_path.to_str().unwrap();

        let logger = AuditLogger::new(log_dir).unwrap();
        assert!(log_path.exists());
        assert_eq!(logger.sequence, 0);
        assert!(!logger.chain_hash.is_empty());
    }

    #[test]
    fn test_log_and_verify_chain() {
        let (mut logger, _dir) = create_test_logger();

        // 记录几条日志
        logger
            .log(
                AuditEventType::UserLogin,
                AuditSeverity::Info,
                "web-api",
                "用户 admin 登录成功",
                "admin",
                "192.168.1.100",
            )
            .unwrap();

        logger
            .log(
                AuditEventType::ConfigChanged,
                AuditSeverity::Warning,
                "web-api",
                "配置已更新",
                "admin",
                "192.168.1.100",
            )
            .unwrap();

        logger
            .log(
                AuditEventType::UserLogout,
                AuditSeverity::Info,
                "web-api",
                "用户 admin 登出",
                "admin",
                "192.168.1.100",
            )
            .unwrap();

        logger.flush().unwrap();

        // 验证哈希链
        assert!(logger.verify_chain().unwrap());
        assert_eq!(logger.sequence, 3);
    }

    #[test]
    fn test_query_by_timerange() {
        let (mut logger, _dir) = create_test_logger();

        let before = Utc::now();

        logger
            .log(
                AuditEventType::SystemStartup,
                AuditSeverity::Info,
                "system",
                "系统启动",
                "system",
                "",
            )
            .unwrap();

        let after = Utc::now();

        logger.flush().unwrap();

        // 查询所有日志
        let results = logger.query(DateTime::UNIX_EPOCH, Utc::now()).unwrap();
        assert!(!results.is_empty());

        // 查时间范围外（过去）
        let old_results = logger
            .query(
                DateTime::UNIX_EPOCH,
                DateTime::UNIX_EPOCH + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert!(old_results.is_empty());
    }

    #[test]
    fn test_export_csv() {
        let (mut logger, _dir) = create_test_logger();

        logger
            .log(
                AuditEventType::PolicyChanged,
                AuditSeverity::Error,
                "policy-engine",
                "策略规则变更",
                "system",
                "",
            )
            .unwrap();

        logger.flush().unwrap();

        let csv_path = _dir.path().join("export.csv");
        logger.export(csv_path.to_str().unwrap()).unwrap();

        let csv_content = fs::read_to_string(&csv_path).unwrap();
        assert!(csv_content.starts_with("sequence,timestamp,"));
        assert!(csv_content.contains("PolicyChanged"));
    }

    #[test]
    fn test_chain_recovery() {
        let dir = TempDir::new().unwrap();
        let log_dir = dir.path().to_str().unwrap();

        // 第一次创建并记录
        {
            let mut logger = AuditLogger::new(log_dir).unwrap();
            logger
                .log(
                    AuditEventType::UserLogin,
                    AuditSeverity::Info,
                    "test",
                    "测试消息",
                    "user1",
                    "127.0.0.1",
                )
                .unwrap();
            logger.flush().unwrap();
        }

        // 第二次打开，验证状态恢复
        {
            let logger = AuditLogger::new(log_dir).unwrap();
            assert_eq!(logger.sequence, 1);
            assert!(logger.verify_chain().unwrap());
        }
    }

    #[test]
    fn test_tampered_log_detected() {
        let dir = TempDir::new().unwrap();
        let log_dir = dir.path().to_str().unwrap();

        let mut logger = AuditLogger::new(log_dir).unwrap();
        logger
            .log(
                AuditEventType::UserLogin,
                AuditSeverity::Info,
                "test",
                "原始消息",
                "user1",
                "127.0.0.1",
            )
            .unwrap();
        logger.flush().unwrap();
        drop(logger);

        // 篡改日志文件
        let mut files = fs::read_dir(log_dir).unwrap();
        let first_file = files.next().unwrap().unwrap().path();
        let mut content = fs::read_to_string(&first_file).unwrap();
        content = content.replace("原始消息", "篡改消息");
        let mut f = File::create(&first_file).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        drop(f);

        // 验证应检测到篡改
        let logger = AuditLogger::new(log_dir).unwrap();
        assert!(!logger.verify_chain().unwrap());
    }
}
