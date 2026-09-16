//! 控制通道审计（设计 §4.5 / §3.3 管线第 5、7 步；PL-1 / PL-2）——开发单元 **G-2**。
//!
//! # 两条通道、两种凭据（为什么是"双写"）
//!
//! | 记录 | 落点 | 作用 |
//! |------|------|------|
//! | **intent**（执行前） | 既有**哈希链**审计 `mupc_security::audit::AuditLogger` | **唯一操作凭据**（T-3 无登录）⇒ 必须在**执行之前**落盘并 fsync；写不成 ⇒ 整个写操作被拒（fail-closed） |
//! | **outcome**（执行后） | `{log_dir}/audit/console-audit-YYYY-MM-DD.jsonl`（`ConsoleAuditEntry`，append-only）+ 哈希链条目 | F19 审计页的查询真源；含 before / after / result / reason |
//!
//! # ⚠️ 契约缺口（本单元如实处置，**未改契约**）
//!
//! `ConsoleAuditEntry` 的 `result` 是**必填**（`AuditResult::Ok | Failed`）——它**表达不了
//! "只写了 intent、结果尚未产生"**这一态。故 intent 记录**不能**塞进 `ConsoleAuditEntry`
//! 形状（塞进去就得给 `result` 编一个假值 ⇒ 谎报），改为只写**哈希链**（其 schema 无 result
//! 字段，天然适配"操作已声明、结果未知"）。这也正是哈希链存在的意义：它是**先于执行**的凭据。
//!
//! # 为什么是同步 IO（而不是 `spawn_blocking`）
//!
//! 审计写是**人触发**的（屏上一次保存 ≤ 1 次），单条 ≤ 数百字节，且 intent **必须**在执行
//! 之前落盘并 fsync —— 与"执行"的先后关系是本模块的**语义核心**，用 `spawn_blocking` 只会
//! 把这次 fsync 挪到另一个线程上等，不改变量级。故此处刻意同步写（并由 `record_intent`
//! 的调用点保证"失败即不执行"）。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use mupc_display_proto::{AuditResult, ConsoleAuditEntry, ConsoleOp, CONSOLE_OPERATOR};
use mupc_security::audit::{AuditEventType, AuditLogger, AuditSeverity};

/// 审计写入的失败原因（人读；进 `tracing` 与回执 `message` 的**内部**日志，不上屏）。
pub type AuditError = String;

/// **intent 前置记录**（管线第 5 步）：执行之前必须落盘的那一条。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditIntent {
    /// 操作类型（配置保存 / 恢复默认值）。
    pub op: ConsoleOp,
    /// 操作目标（逐字段键的联接串，如 `"system.log_level,gateway.listen_port"`）。
    pub target: String,
    /// 请求幂等键的一半（现场对拍用）。
    pub request_id: String,
    /// 请求摘要（人读一行）。
    pub summary: String,
}

/// 审计落点（可注入 ⇒ fail-closed 有**可实测的**失败注入点）。
///
/// 生产唯一实现是 [`FileAuditSink`]；测试用"目录不可写"的真实失败路径（不用 mock，
/// 因为要证的是**真实代码**在审计失败时的行为，而不是测试桩自己的行为）。
pub trait ConsoleAuditSink: Send + Sync {
    /// 管线第 5 步：intent 前置写入。**失败 ⇒ 调用方必须终止执行**（fail-closed）。
    fn record_intent(&self, intent: &AuditIntent) -> Result<(), AuditError>;

    /// 管线第 7 步：结果审计（`entry.id` 即回执里的 `audit_id`）。
    fn record_outcome(&self, entry: &ConsoleAuditEntry) -> Result<(), AuditError>;
}

/// 文件审计（控制台 JSONL + 既有哈希链双写）。
pub struct FileAuditSink {
    /// 审计目录（`{system.log_dir}/audit`）。
    dir: PathBuf,
    /// 既有哈希链审计（`security/src/audit.rs`；内部持文件句柄 ⇒ 需 `Mutex` 串行化）。
    ///
    /// **为什么不另建一个**：`AuditLogger::new` 会**恢复既有链**（读目录里最后一条的哈希），
    /// 两个实例各持一条链会各自算 `sequence` ⇒ 链断。故全进程**唯一实例**，从这里注入。
    chain: Mutex<AuditLogger>,
}

impl FileAuditSink {
    /// 打开（必要时创建）审计落点。
    ///
    /// **失败即"审计不可用"**：调用方**不得**降级成"没有审计也照跑"——那正是 fail-closed
    /// 要防的事（设计 §3.3：审计是唯一操作凭据）。装配点据此让写路径整体
    /// [`crate::console_host::ApplySource::Unavailable`]。
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, AuditError> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("创建审计目录 {} 失败: {e}", dir.display()))?;
        let dir_str = dir
            .to_str()
            .ok_or_else(|| format!("审计目录路径非 UTF-8: {}", dir.display()))?
            .to_string();
        let chain = AuditLogger::new(&dir_str).map_err(|e| format!("初始化哈希链审计失败: {e}"))?;
        Ok(Self {
            dir,
            chain: Mutex::new(chain),
        })
    }

    /// 当天的控制台审计文件（`console-audit-YYYY-MM-DD.jsonl`）。
    ///
    /// 日期取**条目自身**的 `ts_ms`（而不是"写的时候读一次钟"）：同一批 intent/outcome 落在
    /// 同一天，跨零点保存不会把一对记录拆到两个文件里。
    fn console_file(&self, ts_ms: u64) -> PathBuf {
        let day = DateTime::from_timestamp_millis(ts_ms as i64)
            .unwrap_or_else(Utc::now)
            .format("%Y-%m-%d")
            .to_string();
        self.dir.join(format!("console-audit-{day}.jsonl"))
    }

    /// append-only 追加一行（**打开即追加、写完 fsync**；不做任何"读改写"）。
    fn append_line(path: &Path, line: &str) -> Result<(), AuditError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建审计目录 {} 失败: {e}", parent.display()))?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("打开审计文件 {} 失败: {e}", path.display()))?;
        writeln!(f, "{line}").map_err(|e| format!("写审计文件 {} 失败: {e}", path.display()))?;
        f.sync_all()
            .map_err(|e| format!("fsync 审计文件 {} 失败: {e}", path.display()))?;
        Ok(())
    }

    /// 哈希链写一条 + flush（flush 失败也算失败：**未落盘的凭据不算凭据**）。
    fn chain_write(&self, severity: AuditSeverity, message: &str) -> Result<(), AuditError> {
        let mut chain = self
            .chain
            .lock()
            .map_err(|_| "哈希链审计锁被毒化（前次写入 panic）".to_string())?;
        chain
            .log(
                AuditEventType::GenericOperation,
                severity,
                "local-console",
                message,
                CONSOLE_OPERATOR,
                "-",
            )
            .map_err(|e| format!("哈希链审计写入失败: {e}"))?;
        chain
            .flush()
            .map_err(|e| format!("哈希链审计 fsync 失败: {e}"))
    }
}

impl ConsoleAuditSink for FileAuditSink {
    fn record_intent(&self, intent: &AuditIntent) -> Result<(), AuditError> {
        let msg = format!(
            "控制台写操作 intent: op={:?} target={} request_id={} | {}",
            intent.op, intent.target, intent.request_id, intent.summary
        );
        self.chain_write(AuditSeverity::Info, &msg)
    }

    fn record_outcome(&self, entry: &ConsoleAuditEntry) -> Result<(), AuditError> {
        let line = serde_json::to_string(entry)
            .map_err(|e| format!("序列化审计条目失败: {e}"))?;
        // ① 控制台 JSONL（F19 查询真源）
        Self::append_line(&self.console_file(entry.ts_ms), &line)?;
        // ② 既有哈希链（统一合规凭据）——失败视为整体失败（上抛，由调用方按"结果审计失败"处置）
        let level = match entry.result {
            AuditResult::Ok => AuditSeverity::Info,
            AuditResult::Failed => AuditSeverity::Warning,
        };
        let msg = format!(
            "控制台写操作 outcome: id={} op={:?} target={} result={:?} reason={} request_id={}",
            entry.id,
            entry.op,
            entry.target,
            entry.result,
            entry.reason.as_deref().unwrap_or("-"),
            entry.request_id
        );
        self.chain_write(level, &msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn entry(id: &str, result: AuditResult, reason: Option<&str>) -> ConsoleAuditEntry {
        ConsoleAuditEntry {
            id: id.to_string(),
            // 固定时刻：文件名的日期必须由**条目自身**推出（可复现，不依赖跑测试的当天）
            ts_ms: 1_757_412_000_000,
            operator: CONSOLE_OPERATOR.to_string(),
            op: ConsoleOp::ConfigApply,
            target: "intercore.port".to_string(),
            before: Some(serde_json::json!(9100)),
            after: Some(serde_json::json!(2405)),
            result,
            reason: reason.map(str::to_string),
            request_id: "rid-1".to_string(),
        }
    }

    /// intent 进哈希链、outcome 进控制台 JSONL；两条**都能**被独立读到。
    #[test]
    fn intent_goes_to_the_hash_chain_and_outcome_to_the_console_jsonl() {
        let t = TempDir::new("audit-ok");
        let sink = FileAuditSink::open(t.path()).unwrap();
        sink.record_intent(&AuditIntent {
            op: ConsoleOp::ConfigApply,
            target: "intercore.port".into(),
            request_id: "rid-1".into(),
            summary: "changes=1 from=edit".into(),
        })
        .unwrap();
        sink.record_outcome(&entry("a1", AuditResult::Ok, None)).unwrap();

        // 控制台 JSONL：**一行**，且能按契约类型解回来（G-3 查询端同款路径）
        let p = t.join("console-audit-2025-09-09.jsonl");
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(text.lines().count(), 1, "一条 outcome = 一行");
        let back: ConsoleAuditEntry = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(back, entry("a1", AuditResult::Ok, None));
        assert_eq!(back.operator, CONSOLE_OPERATOR);

        // 哈希链：intent + outcome = 两条（且 seq 递增 ⇒ 链真的在写）
        let chain_files: Vec<PathBuf> = std::fs::read_dir(t.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("audit_"))
            })
            .collect();
        assert_eq!(chain_files.len(), 1, "哈希链当日文件应恰好一个");
        let chain_text = std::fs::read_to_string(&chain_files[0]).unwrap();
        let chain_lines: Vec<&str> = chain_text.lines().collect();
        assert_eq!(chain_lines.len(), 2, "intent 与 outcome 各一条链上记录");
        assert!(chain_lines[0].contains("\"sequence\":1"), "intent 是链上第一条");
        assert!(chain_lines[0].contains("intent"), "intent 记录须可辨认");
        assert!(chain_lines[1].contains("\"sequence\":2"), "outcome 是链上第二条");
        assert!(chain_lines[1].contains("outcome"));
        assert!(chain_lines[1].contains("rid-1"), "链上记录须带 request_id（现场对拍）");
    }

    /// append-only：第二次写不得截断第一次的内容（PL-02「仅追加、不可删改」）。
    #[test]
    fn outcomes_are_appended_never_truncated() {
        let t = TempDir::new("audit-append");
        let sink = FileAuditSink::open(t.path()).unwrap();
        sink.record_outcome(&entry("a1", AuditResult::Ok, None)).unwrap();
        sink.record_outcome(&entry("a2", AuditResult::Failed, Some("越界")))
            .unwrap();
        let text = std::fs::read_to_string(t.join("console-audit-2025-09-09.jsonl")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<ConsoleAuditEntry>(lines[0]).unwrap().id,
            "a1",
            "先写的那条必须还在（且未被改动）"
        );
        let second: ConsoleAuditEntry = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second.result, AuditResult::Failed);
        assert_eq!(second.reason.as_deref(), Some("越界"));
    }

    /// **跨进程重启**：同一目录再开一个 sink ⇒ 必须能继续既有哈希链（seq 从 3 开始），
    /// 且**不会**把控制台 JSONL 误当成链文件（`list_audit_files` 只认 `audit_*.jsonl`，
    /// 而控制台文件是 `console-audit-*.jsonl`）。
    ///
    /// 这条网防的是"目录里多了一类 jsonl 就把审计整体打挂"——现场审计目录是**跨重启复用**的，
    /// 若恢复逻辑去解析 `ConsoleAuditEntry`（schema 不同），轻则链断、重则启动失败。
    #[test]
    fn a_second_sink_in_the_same_dir_continues_the_existing_chain() {
        let t = TempDir::new("audit-restart");
        {
            let s = FileAuditSink::open(t.path()).unwrap();
            s.record_intent(&AuditIntent {
                op: ConsoleOp::ConfigApply,
                target: "intercore.port".into(),
                request_id: "rid-1".into(),
                summary: "changes=1".into(),
            })
            .unwrap();
            s.record_outcome(&entry("a1", AuditResult::Ok, None)).unwrap();
        }
        // 控制台 JSONL 已在目录里（跨重启复用场景的真实形态）
        assert!(t.join("console-audit-2025-09-09.jsonl").exists());

        let s2 = FileAuditSink::open(t.path()).expect("同目录二次打开必须成功");
        s2.record_intent(&AuditIntent {
            op: ConsoleOp::ConfigApply,
            target: "system.log_level".into(),
            request_id: "rid-2".into(),
            summary: "changes=1".into(),
        })
        .unwrap();
        let chain_text = std::fs::read_to_string(
            std::fs::read_dir(t.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("audit_"))
                })
                .unwrap(),
        )
        .unwrap();
        let lines: Vec<&str> = chain_text.lines().collect();
        assert_eq!(lines.len(), 3, "重启后新增的 intent 是第 3 条（链续上了）");
        assert!(lines[2].contains("\"sequence\":3"), "序列号必须续接: {}", lines[2]);
        assert!(lines[2].contains("rid-2"));
        // 控制台文件没被链的写入污染（仍是 1 行，且仍是 ConsoleAuditEntry）
        let console =
            std::fs::read_to_string(t.join("console-audit-2025-09-09.jsonl")).unwrap();
        assert_eq!(console.lines().count(), 1);
        assert_eq!(
            serde_json::from_str::<ConsoleAuditEntry>(console.trim()).unwrap().id,
            "a1"
        );
    }

    /// **审计不可用必须响亮**（fail-closed 的失败注入点：目录建不出来）。
    ///
    /// 注入方式是**真实失败**（父路径是个普通文件 ⇒ `create_dir_all` 必失败），不用 mock ——
    /// 要证的是真实实现会 `Err`，而不是测试桩自己会 `Err`。
    #[test]
    fn unavailable_audit_dir_fails_loudly() {
        let t = TempDir::new("audit-bad");
        let blocker = t.write("blocker", "i am a file, not a dir");
        let bad_dir = blocker.join("audit"); // 父是文件 ⇒ 无法建目录
        // 不用 `expect_err`：`FileAuditSink` 没有 `Debug`（它持有文件句柄与 `Mutex`），
        // 而 `expect_err` 要求 `T: Debug`。
        let err = match FileAuditSink::open(&bad_dir) {
            Ok(_) => panic!("审计目录不可建时必须失败（父路径是普通文件）"),
            Err(e) => e,
        };
        assert!(err.contains("审计目录"), "原因须可定位: {err}");
    }
}
