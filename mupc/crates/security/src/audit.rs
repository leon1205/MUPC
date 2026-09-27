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
//! - 链首哈希 = SHA-256(**链锚** || `genesis`)
//! - 每条日志的 sm3_chain_hash = SHA-256(prev_hash || entry_json)
//!
//! ## 链锚（为什么链首不能是源码里的常量）
//! 链首若由源码内公开常量派生，任何有写权限的人都能**整链重算**（逐条按公开算法补哈希），
//! 校验照样通过。故链锚改为**部署期密钥**（环境变量 [`AUDIT_CHAIN_KEY_ENV`]，或
//! [`AuditLogger::new_with_anchor`] 显式注入）。**未配置时显式降级**：
//! [`AuditLogger::verify_chain`] 返回的 [`ChainVerification::anchored`] 为 `false`，
//! `detail` 明写"无密钥锚，链只防意外损坏、不防篡改" —— 不假装安全。
//!
//! ## 尾部缺失（D-12）
//! 逐条校验前驱哈希只能覆盖"已存在的条目"：**删掉最后一个文件**后前缀仍然自洽 ⇒ 旧实现返回
//! 校验通过。故新增链头元数据文件 [`CHAIN_META_FILENAME`]（记录最新序号/哈希 + 清理水位）：
//! - 实际末序号 < 元数据链头序号 ⇒ **尾部缺失**，报错；
//! - `purge_old` 按保留策略删文件时写入**清理水位**，使幸存链首的 `prev_sm3_hash` 有据可查
//!   （否则例行保留策略会把链判成"被篡改"）。

use crate::errors::SecurityError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
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

/// 哈希链创世种子（**公开常量 ⇒ 无锚保护**）
///
/// ⚠️ 这是**源码里人人可见**的常量。仅当部署期未提供 [`AUDIT_CHAIN_KEY_ENV`] 时用作降级链首，
/// 此时链**只防意外损坏、不防有写权限者整链重算**（见 [`ChainVerification::anchored`]）。
const GENESIS_SEED: &[u8] = b"MUPC_AUDIT_GENESIS_SEED_V1";

/// 部署期链锚密钥的环境变量名（64 位十六进制 = 32 字节）。
///
/// 未设置 / 格式非法 ⇒ **显式降级**（不 panic、不静默装成已锚定）。
pub const AUDIT_CHAIN_KEY_ENV: &str = "MUPC_AUDIT_CHAIN_KEY";

/// 链头/清理水位元数据文件名（扩展名 `.json` ⇒ `list_audit_files` 不会把它当审计日志）。
const CHAIN_META_FILENAME: &str = "audit_chain.meta.json";

/// 链头与清理水位（`audit_chain.meta.json`）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChainMeta {
    /// 已写入的**最后一条**的序号（尾部缺失的对照点）
    #[serde(default)]
    head_sequence: u64,
    /// 已写入的最后一条的链式哈希
    #[serde(default)]
    head_hash: String,
    /// `purge_old` 的清理水位：序号 ≤ 此值的条目已按保留策略删除
    #[serde(default)]
    purged_through_sequence: u64,
    /// 水位处**最后一条被删条目**的哈希（= 幸存链首条目应有的 `prev_sm3_hash`）
    #[serde(default)]
    purged_anchor_hash: String,
}

/// 链校验结果
///
/// `valid == true` **且** `anchored == false` 的含义是："链内部自洽（没被意外损坏 / 没被单条
/// 篡改），但**没有密钥锚** ⇒ 有写权限者可整链重算，故**不能**当作抗篡改凭据。"
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainVerification {
    /// 链是否自洽且未检出尾部缺失
    pub valid: bool,
    /// 是否受**部署期密钥锚**保护（`false` = 降级，见类型文档）
    pub anchored: bool,
    /// 人读说明（降级/失败原因；不得被当作"通过"信号）
    pub detail: String,
}

/// 审计文件的落盘原语。
///
/// 抽这层**只为一件事**：让"`flush()` 到底调的是 `Write::flush`（对 `std::fs::File` 是 no-op）
/// 还是 `File::sync_all`（真 `fsync`/`FlushFileBuffers`）"成为**可判别**的事实（D-3）。
/// 生产唯一实现见下方 [`AuditFile`] 对 `File` 的实现。
trait AuditFile: Write + Send {
    /// 把已写字节真正压到**持久介质**。
    fn sync_all(&mut self) -> io::Result<()>;
}

impl AuditFile for File {
    fn sync_all(&mut self) -> io::Result<()> {
        File::sync_all(self)
    }
}

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
    /// 事件来源（如 "hmi_backend", "gateway", "strategy-engine"）
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
///     "hmi_backend",
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
    file_handle: Option<Box<dyn AuditFile>>,
    /// 当前哈希链的最后一个哈希值
    chain_hash: String,
    /// 当前序列号
    sequence: u64,
    /// 距上次 flush 的条目数
    entries_since_flush: usize,
    /// 当前日志文件对应的日期（用于跨天切换）
    current_date: String,
    /// 链头 / 清理水位（尾部缺失检测的对照点）
    meta: ChainMeta,
    /// 部署期链锚密钥（`None` ⇒ **降级**，见 [`ChainVerification::anchored`]）
    anchor: Option<[u8; 32]>,
}

impl AuditLogger {
    /// 创建审计日志记录器（链锚取自环境变量 [`AUDIT_CHAIN_KEY_ENV`]）
    ///
    /// - 创建日志目录（递归）
    /// - 读取最后一条已存在的日志记录以恢复哈希链状态
    /// - 如果日志目录为空，从**链锚**（未配置则为公开创世种子）初始化哈希链
    pub fn new(log_dir: &str) -> Result<Self, SecurityError> {
        Self::new_with_anchor(log_dir, anchor_from_env())
    }

    /// 创建审计日志记录器（**显式**注入链锚；`None` = 降级）
    pub fn new_with_anchor(log_dir: &str, anchor: Option<[u8; 32]>) -> Result<Self, SecurityError> {
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
            meta: ChainMeta::default(),
            anchor,
        };

        if logger.anchor.is_none() {
            tracing::warn!(
                env = AUDIT_CHAIN_KEY_ENV,
                "审计链**无密钥锚**（降级）：链只防意外损坏 / 单条篡改，**不防**有写权限者整链重算；\
                 不得将该状态当作抗篡改凭据"
            );
        }

        logger.meta = logger.read_chain_meta();
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

    /// 链锚状态（`false` = 降级，见 [`ChainVerification::anchored`]）
    pub fn is_anchored(&self) -> bool {
        self.anchor.is_some()
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

        // 更新链头（尾部缺失检测的对照点）：**每条都更新**（不只是 flush 时），
        // 否则"崩溃前最后几条"的丢失无法与"尾部被删"区分。写失败只告警不阻断审计写入
        // （校验侧另有判据；此处失败会表现为链头滞后 ⇒ 校验按"滞后可接受、回退即失败"处置）。
        self.meta.head_sequence = self.sequence;
        self.meta.head_hash = self.chain_hash.clone();
        self.write_chain_meta();

        // 每 10 条日志自动 flush（fsync）
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

    /// 刷新审计日志到磁盘（**真 fsync**）
    ///
    /// ⚠️ 这里必须是 `File::sync_all`（POSIX `fsync` / Windows `FlushFileBuffers`），
    /// **不是** `Write::flush` —— 后者对 `std::fs::File` 是 no-op（不发起任何系统调用），
    /// 掉电即丢。intent 记录是控制台写操作的**唯一操作凭据**（见 `console_audit.rs`），
    /// 丢了就等于操作无凭据。判别力测试：`flush_does_a_real_sync_all_not_a_noop_flush`
    /// 与 `flush_propagates_sync_all_errors`。
    pub fn flush(&mut self) -> Result<(), SecurityError> {
        let synced = if let Some(file) = self.file_handle.as_mut() {
            file.sync_all()
                .map_err(|e| SecurityError::IoError(format!("fsync 审计日志失败: {}", e)))?;
            true
        } else {
            false
        };

        if synced {
            self.entries_since_flush = 0;
            self.write_chain_meta();
            tracing::debug!("审计日志已 fsync 到磁盘");
        }
        Ok(())
    }

    /// 验证哈希链（逐条前驱哈希 + 序号连续性 + **尾部缺失**）
    ///
    /// 返回 [`ChainVerification`]：`valid` 为链自身是否自洽；`anchored` 表明是否受部署期密钥锚
    /// 保护（见类型文档 —— `anchored = false` 时**不得**把结果当抗篡改凭据）。
    ///
    /// 判据（任一不成立 ⇒ `valid = false`，说明写入 `detail`）：
    /// 1. 每条 `prev_sm3_hash` 等于前一条的链式哈希（链首取**链锚/清理水位**）；
    /// 2. 序号**连续**（跨文件亦连续）—— 整段文件缺失 ⇒ 序号跳变；
    /// 3. 实际末序号 **≥** 元数据链头序号（**小于 ⇒ 尾部缺失/回退**）；
    ///    等于时哈希必须逐字相同。
    pub fn verify_chain(&self) -> Result<ChainVerification, SecurityError> {
        let anchored = self.anchor.is_some();
        let detail_suffix = if anchored {
            "链锚=部署期密钥".to_string()
        } else {
            format!(
                "⚠️ 无密钥锚（未配置 {}）：链只防意外损坏/单条篡改，**不防**有篡改权限者整链重算",
                AUDIT_CHAIN_KEY_ENV
            )
        };

        tracing::info!("开始验证审计日志哈希链...");

        // 链首：有清理水位则从水位处续，否则从链锚（未配置锚 = 公开创世种子的降级态）
        let mut expected_hash = if self.meta.purged_through_sequence > 0
            && !self.meta.purged_anchor_hash.is_empty()
        {
            self.meta.purged_anchor_hash.clone()
        } else {
            self.genesis_hash()
        };
        let mut expected_sequence = self.meta.purged_through_sequence + 1;
        let mut last_sequence = 0u64;
        let mut last_hash = String::new();

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

                // ① 序号连续性（整段文件被删 ⇒ 跳变）
                if entry.sequence != expected_sequence {
                    let detail = format!(
                        "链序号不连续: 文件={}, 行={}, 期望序号={}, 实际={}（文件整段缺失或尾部被裁）",
                        file_path.display(),
                        line_no + 1,
                        expected_sequence,
                        entry.sequence
                    );
                    tracing::error!("{detail}");
                    return Ok(ChainVerification {
                        valid: false,
                        anchored,
                        detail: format!("{detail} | {detail_suffix}"),
                    });
                }

                // ② 前驱哈希连续性
                if entry.prev_sm3_hash != expected_hash {
                    let detail = format!(
                        "哈希链断裂: 文件={}, 行={}, 期望前驱={}, 实际前驱={}",
                        file_path.display(),
                        line_no + 1,
                        expected_hash,
                        entry.prev_sm3_hash
                    );
                    tracing::error!("{detail}");
                    return Ok(ChainVerification {
                        valid: false,
                        anchored,
                        detail: format!("{detail} | {detail_suffix}"),
                    });
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
                    let detail = format!(
                        "哈希不匹配: 文件={}, 行={}, 计算={}, 存储={}",
                        file_path.display(),
                        line_no + 1,
                        computed_hash,
                        entry.sm3_chain_hash
                    );
                    tracing::error!("{detail}");
                    return Ok(ChainVerification {
                        valid: false,
                        anchored,
                        detail: format!("{detail} | {detail_suffix}"),
                    });
                }

                last_hash = entry.sm3_chain_hash.clone();
                expected_hash = entry.sm3_chain_hash;
                expected_sequence = entry.sequence + 1;
                last_sequence = entry.sequence;
            }
        }

        // ③ 尾部缺失/回退：实际末序号**小于**元数据链头 ⇒ 末尾文件（或条目）被删。
        //    （大于 = 崩溃前未 flush 的滞后，属正常，放行。）
        if last_sequence < self.meta.head_sequence {
            let detail = format!(
                "尾部缺失: 元数据链头序号={}, 实际末序号={}（最后一个审计文件被删除或截断）",
                self.meta.head_sequence, last_sequence
            );
            tracing::error!("{detail}");
            return Ok(ChainVerification {
                valid: false,
                anchored,
                detail: format!("{detail} | {detail_suffix}"),
            });
        }
        if last_sequence == self.meta.head_sequence
            && !self.meta.head_hash.is_empty()
            && last_hash != self.meta.head_hash
        {
            let detail = format!(
                "链头哈希不符: 元数据={}, 实际={}（末条被改写）",
                self.meta.head_hash, last_hash
            );
            tracing::error!("{detail}");
            return Ok(ChainVerification {
                valid: false,
                anchored,
                detail: format!("{detail} | {detail_suffix}"),
            });
        }

        tracing::info!("审计日志哈希链验证通过: 共 {} 个文件", files.len());
        Ok(ChainVerification {
            valid: true,
            anchored,
            detail: if anchored {
                format!(
                    "链自洽且受部署期密钥锚保护（{} 条，{} 个文件）| {detail_suffix}",
                    last_sequence,
                    files.len()
                )
            } else {
                format!(
                    "链自洽，但 {detail_suffix}（{} 条，{} 个文件）",
                    last_sequence,
                    files.len()
                )
            },
        })
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

        self.file_handle = Some(Box::new(file));
        Ok(())
    }

    /// 链首哈希：有部署期链锚则由它派生，否则退回公开创世种子（**降级态**）。
    fn genesis_hash(&self) -> String {
        genesis_hash_for(self.anchor.as_ref())
    }

    /// 链头元数据文件路径。
    fn chain_meta_path(&self) -> PathBuf {
        self.log_dir.join(CHAIN_META_FILENAME)
    }

    /// 读链头元数据（缺失 / 损坏 ⇒ 默认值 + 告警，**不 panic**：元数据是**辅助**判据，
    /// 缺了最多退回"无尾部检测"，不该打挂审计）。
    fn read_chain_meta(&self) -> ChainMeta {
        match fs::read_to_string(self.chain_meta_path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("链头元数据解析失败（按空处理）: {}", e);
                ChainMeta::default()
            }),
            Err(_) => ChainMeta::default(),
        }
    }

    /// 原子落盘链头元数据（临时文件 + `rename`）；失败只告警。
    fn write_chain_meta(&self) {
        if let Err(e) = write_meta_atomic(&self.chain_meta_path(), &self.meta) {
            tracing::warn!("链头元数据写入失败（尾部缺失检测将退化）: {}", e);
        }
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
        let mut last_hash = self.genesis_hash();
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

        // 启动即可见的尾部缺失信号（**不**阻断启动：审计要能继续写，缺的是历史）。
        if last_sequence < self.meta.head_sequence {
            tracing::error!(
                "审计链尾部缺失：元数据链头序号={} > 磁盘实际末序号={} —— \
                 末尾审计文件被删除或截断；该缺口不会被自动修补，请人工核验",
                self.meta.head_sequence,
                last_sequence
            );
        }

        self.chain_hash = last_hash;
        self.sequence = last_sequence;

        if last_sequence > 0 {
            tracing::info!(
                "从磁盘恢复审计日志状态: 序列号={}, 最后哈希={}",
                last_sequence,
                &self.chain_hash[..16.min(self.chain_hash.len())]
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
    ///
    /// # 与链锚的冲突（D-12(c)）
    /// 删掉最旧的文件后，幸存链首条目的 `prev_sm3_hash` 指向的是**被删掉的**那一条的哈希 ⇒
    /// 若校验仍以链锚为起点，例行保留策略会被判成"链被篡改"。故删除前**记下**被删条目的
    /// 末序号与哈希（**清理水位**，落进 [`CHAIN_META_FILENAME`]），校验时从水位续起。
    ///
    /// 取 `&mut self`：清理水位是链状态的一部分，必须与内存中的 `meta` 一致。
    pub fn purge_old(&mut self, before: DateTime<Utc>) -> Result<usize, SecurityError> {
        let files = self.list_audit_files()?;
        let mut removed = 0;
        let mut watermark_seq = self.meta.purged_through_sequence;
        let mut watermark_hash = self.meta.purged_anchor_hash.clone();

        for file in files {
            if file == self.current_file {
                continue; // 跳过当前文件
            }
            let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let date_str = name
                .strip_prefix("audit_")
                .and_then(|s| s.strip_suffix(".jsonl"));
            if let Some(date_str) = date_str {
                if let Ok(naive) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                    if let Some(file_date) = naive.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc()) {
                        if file_date < before {
                            // 先记水位（**删除之前**读）：这一条是幸存链首条目的前驱。
                            if let Some(last) = last_entry_of(&file) {
                                if last.sequence > watermark_seq {
                                    watermark_seq = last.sequence;
                                    watermark_hash = last.sm3_chain_hash;
                                }
                            }
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

        if removed > 0 {
            self.meta.purged_through_sequence = watermark_seq;
            self.meta.purged_anchor_hash = watermark_hash;
            self.write_chain_meta();
            tracing::info!(
                "审计日志保留策略已清理 {} 个文件；清理水位=序号 {}（校验从水位续起）",
                removed,
                self.meta.purged_through_sequence
            );
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

/// 计算创世哈希: SHA-256(genesis_seed)（**降级链首**：种子是源码内公开常量）
fn compute_genesis_hash() -> String {
    let mut hasher = Sha256::new();
    hasher.update(GENESIS_SEED);
    hex::encode(hasher.finalize())
}

/// 链首哈希：`Some(key)` ⇒ SHA-256(key || `genesis`)；`None` ⇒ 公开创世种子（降级）。
fn genesis_hash_for(anchor: Option<&[u8; 32]>) -> String {
    match anchor {
        Some(key) => {
            let mut hasher = Sha256::new();
            hasher.update(key);
            hasher.update(b"|mupc-audit-genesis|");
            hex::encode(hasher.finalize())
        }
        None => compute_genesis_hash(),
    }
}

/// 从环境变量读链锚密钥（64 位十六进制 = 32 字节）。
///
/// 未设置 ⇒ `None`（降级）；**设置但非法** ⇒ 也 `None` 并**响亮告警**（不 panic，也不假装已锚定）。
fn anchor_from_env() -> Option<[u8; 32]> {
    let raw = env::var(AUDIT_CHAIN_KEY_ENV).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        tracing::warn!(env = AUDIT_CHAIN_KEY_ENV, "链锚环境变量为空 ⇒ 按未配置处理（降级）");
        return None;
    }
    match hex::decode(raw) {
        Ok(bytes) if bytes.len() == 32 => {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            tracing::info!(env = AUDIT_CHAIN_KEY_ENV, "审计链已启用部署期密钥锚");
            Some(key)
        }
        Ok(bytes) => {
            tracing::warn!(
                env = AUDIT_CHAIN_KEY_ENV,
                len = bytes.len(),
                "链锚长度非法（需 32 字节）⇒ 降级为无锚；链不防整链重算"
            );
            None
        }
        Err(e) => {
            tracing::warn!(
                env = AUDIT_CHAIN_KEY_ENV,
                "链锚不是合法十六进制 ⇒ 降级为无锚；链不防整链重算: {}",
                e
            );
            None
        }
    }
}

/// 原子写入元数据（临时文件 + `rename`，避免读到半截 JSON）。
fn write_meta_atomic(path: &Path, meta: &ChainMeta) -> io::Result<()> {
    let json = serde_json::to_string(meta)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// 读一个审计日志文件的**最后一条**条目（`purge_old` 记清理水位用）。
fn last_entry_of(path: &Path) -> Option<AuditLogEntry> {
    let f = File::open(path).ok()?;
    let reader = BufReader::new(f);
    let mut last = None;
    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<AuditLogEntry>(&line) {
            last = Some(entry);
        }
    }
    last
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
                "hmi_backend",
                "用户 admin 登录成功",
                "admin",
                "192.168.1.100",
            )
            .unwrap();

        logger
            .log(
                AuditEventType::ConfigChanged,
                AuditSeverity::Warning,
                "hmi_backend",
                "配置已更新",
                "admin",
                "192.168.1.100",
            )
            .unwrap();

        logger
            .log(
                AuditEventType::UserLogout,
                AuditSeverity::Info,
                "hmi_backend",
                "用户 admin 登出",
                "admin",
                "192.168.1.100",
            )
            .unwrap();

        logger.flush().unwrap();

        // 验证哈希链
        let v = logger.verify_chain().unwrap();
        assert!(v.valid, "{}", v.detail);
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
            let v = logger.verify_chain().unwrap();
            assert!(v.valid, "{}", v.detail);
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
        let v = logger.verify_chain().unwrap();
        assert!(!v.valid, "篡改必须检出: {}", v.detail);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // D-3：flush() 必须真 fsync（可注入的假 AuditFile：记录 sync_all 是否被调用）
    // ═══════════════════════════════════════════════════════════════════════

    /// 假落盘原语：
    /// - `Write::flush` 与 `std::fs::File` 同口径（**no-op**）—— 旧实现只调它，本桩不会记数；
    /// - `sync_all` 记数（或注入失败）—— 只有新实现才会走到。
    struct FakeAuditFile {
        sync_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        fail: Option<&'static str>,
    }

    impl Write for FakeAuditFile {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            // 刻意 no-op：与 `impl Write for std::fs::File` 的实际行为一致（不落盘）
            Ok(())
        }
    }

    impl AuditFile for FakeAuditFile {
        fn sync_all(&mut self) -> io::Result<()> {
            self.sync_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match self.fail {
                None => Ok(()),
                Some(msg) => Err(io::Error::new(io::ErrorKind::Other, msg)),
            }
        }
    }

    /// 判别力：`flush()` 必须**调用 `sync_all`**。旧实现只调 `Write::flush`（对 `File` 是 no-op）
    /// ⇒ 本用例的计数恒为 0 ⇒ 红。
    #[test]
    fn flush_does_a_real_sync_all_not_a_noop_flush() {
        let (mut logger, _dir) = create_test_logger();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        logger.file_handle = Some(Box::new(FakeAuditFile {
            sync_calls: calls.clone(),
            fail: None,
        }));

        logger.flush().unwrap();
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "flush() 必须落到 File::sync_all（fsync）；只调 Write::flush 时本计数为 0"
        );
        assert_eq!(logger.entries_since_flush, 0);
    }

    /// 判别力：`sync_all` 的错误必须**传播**（旧实现下 `Write::flush` 恒 Ok ⇒ 本用例红）。
    #[test]
    fn flush_propagates_sync_all_errors() {
        let (mut logger, _dir) = create_test_logger();
        logger.file_handle = Some(Box::new(FakeAuditFile {
            sync_calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            fail: Some("模拟磁盘失联"),
        }));

        let err = logger.flush().expect_err("fsync 失败必须上抛（凭据未落盘 = 凭据不存在）");
        let msg = err.to_string();
        assert!(msg.contains("fsync"), "文案须点明 fsync: {msg}");
        assert!(msg.contains("模拟磁盘失联"), "须带上底层原因: {msg}");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // D-12：链锚 + 尾部缺失 + 清理水位
    // ═══════════════════════════════════════════════════════════════════════

    const OLD_FILE_NAME: &str = "audit_2020-01-01.jsonl";

    /// 造一个"跨两个文件"的真实链：第一天一条（改名成 2020-01-01），第二天一条（今天）。
    /// 返回（临时目录, 次日条目序号）。
    fn two_file_chain() -> (TempDir, u64) {
        let dir = TempDir::new().unwrap();
        let log_dir = dir.path().to_str().unwrap();
        {
            let mut l = AuditLogger::new(log_dir).unwrap();
            l.log(
                AuditEventType::SystemStartup,
                AuditSeverity::Info,
                "test",
                "第一天",
                "sys",
                "-",
            )
            .unwrap();
            l.flush().unwrap();
        }
        let today_file = only_audit_file(dir.path(), None);
        fs::rename(&today_file, dir.path().join(OLD_FILE_NAME)).unwrap();
        {
            let mut l = AuditLogger::new(log_dir).unwrap();
            assert_eq!(l.sequence, 1, "链须从 2020 文件续上");
            l.log(
                AuditEventType::ConfigChanged,
                AuditSeverity::Warning,
                "test",
                "第二天",
                "sys",
                "-",
            )
            .unwrap();
            l.flush().unwrap();
        }
        (dir, 2)
    }

    fn only_audit_file(dir: &Path, exclude: Option<&str>) -> PathBuf {
        let mut hits: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().unwrap().to_string_lossy().to_string();
                n.starts_with("audit_") && n.ends_with(".jsonl") && Some(n.as_str()) != exclude
            })
            .collect();
        assert_eq!(hits.len(), 1, "期望恰好一个待选审计文件: {hits:?}");
        hits.pop().unwrap()
    }

    /// 判别力：**删掉末尾（最新）的文件**必须判失败。
    /// 旧实现（无链头对照）对"前缀自洽"照回 true ⇒ 本用例红。
    #[test]
    fn deleting_the_newest_audit_file_is_detected_as_tail_loss() {
        let (dir, seq) = two_file_chain();
        assert_eq!(seq, 2);
        let now = only_audit_file(dir.path(), Some(OLD_FILE_NAME));

        // 前提：两文件齐全时通过（否则下面那条断言没有判别力）
        let v = AuditLogger::new(dir.path().to_str().unwrap())
            .unwrap()
            .verify_chain()
            .unwrap();
        assert!(v.valid, "前提：齐全时必须通过: {}", v.detail);

        fs::remove_file(&now).unwrap();
        let bad = AuditLogger::new(dir.path().to_str().unwrap())
            .unwrap()
            .verify_chain()
            .unwrap();
        assert!(!bad.valid, "删掉末尾文件必须判失败（旧实现返回 true）");
        assert!(
            bad.detail.contains("尾部缺失"),
            "失败原因须是尾部缺失（可定位）: {}",
            bad.detail
        );
    }

    /// 判别力：删掉**最旧**的文件（链首断裂/序号跳变）同样必须判失败。
    #[test]
    fn deleting_the_oldest_audit_file_is_detected() {
        let (dir, _) = two_file_chain();
        fs::remove_file(dir.path().join(OLD_FILE_NAME)).unwrap();
        let bad = AuditLogger::new(dir.path().to_str().unwrap())
            .unwrap()
            .verify_chain()
            .unwrap();
        assert!(!bad.valid, "删掉最旧文件后链首断裂必须判失败");
    }

    /// 判别力：**清理水位**——`purge_old` 删掉旧文件后，链仍须可验。
    /// 旧实现：幸存链首条目的 `prev_sm3_hash` 与被删条目不符（≠ 创世哈希）⇒ 判"被篡改" ⇒ 红。
    #[test]
    fn purge_old_keeps_the_remaining_chain_verifiable() {
        let (dir, _) = two_file_chain();
        let mut l = AuditLogger::new(dir.path().to_str().unwrap()).unwrap();

        let before = DateTime::parse_from_rfc3339("2021-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let removed = l.purge_old(before).unwrap();
        assert_eq!(removed, 1, "只该删掉 2020-01-01 那一个文件");

        let v = l.verify_chain().unwrap();
        assert!(
            v.valid,
            "例行保留策略之后链仍须可验（不得把正常清理判成篡改）: {}",
            v.detail
        );
        // 水位确实被记下（否则上面的通过只可能是"整链为空"的假象）
        assert!(l.meta.purged_through_sequence == 1 && !l.meta.purged_anchor_hash.is_empty());
    }

    /// D-12(a)：链锚来自部署期密钥时，链首哈希必须**不同于**公开常量派生的链首；
    /// 未配置锚时 `verify_chain` 必须**如实标注降级**（不假装安全）。
    #[test]
    fn anchor_changes_the_chain_head_and_degradation_is_reported() {
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();
        let key = [0x5Au8; 32];

        let mut anchored =
            AuditLogger::new_with_anchor(dir_a.path().to_str().unwrap(), Some(key)).unwrap();
        let mut unanchored =
            AuditLogger::new_with_anchor(dir_b.path().to_str().unwrap(), None).unwrap();
        assert!(anchored.is_anchored() && !unanchored.is_anchored());

        for l in [&mut anchored, &mut unanchored] {
            l.log(
                AuditEventType::UserLogin,
                AuditSeverity::Info,
                "test",
                "登录",
                "admin",
                "-",
            )
            .unwrap();
            l.flush().unwrap();
        }

        let head_a = first_line_entry(&dir_a.path().join(only_audit_file(dir_a.path(), None)));
        let head_b = first_line_entry(&dir_b.path().join(only_audit_file(dir_b.path(), None)));
        assert_ne!(
            head_a.prev_sm3_hash, head_b.prev_sm3_hash,
            "链首必须随锚变化（否则锚没生效，源码内公开种子人人可重算）"
        );
        assert_eq!(
            head_b.prev_sm3_hash,
            compute_genesis_hash(),
            "无锚时必须（且只）退回公开创世种子 —— 这正是降级态"
        );

        let va = anchored.verify_chain().unwrap();
        assert!(va.valid && va.anchored, "有锚: {}", va.detail);
        let vb = unanchored.verify_chain().unwrap();
        assert!(vb.valid && !vb.anchored, "无锚仍可自洽: {}", vb.detail);
        assert!(
            vb.detail.contains("无密钥锚") && vb.detail.contains("不防"),
            "降级必须在结果里**明写**（不得读成抗篡改凭据）: {}",
            vb.detail
        );
    }

    fn first_line_entry(path: &Path) -> AuditLogEntry {
        let text = fs::read_to_string(path).unwrap();
        serde_json::from_str(text.lines().next().unwrap()).unwrap()
    }
}
