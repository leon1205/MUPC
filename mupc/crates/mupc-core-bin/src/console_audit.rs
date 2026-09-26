//! 控制通道审计（设计 §4.5 / §3.3 管线第 5、7 步；PL-1 / PL-2）。
//!
//! 本文件承载**两侧**（同一落点、同一份 JSONL）：
//!
//! | 侧 | 开发单元 | 落点 |
//! |----|----------|------|
//! | **写**（intent / outcome 双写、fail-closed） | G-2 | 本文件上半（`ConsoleAuditSink` / `FileAuditSink`） |
//! | **读**（F19 查询：筛选 / 倒序 / 分页 / `newest_ts_ms` / 不可用降级） | **I** | 本文件下半（[`ConsoleAuditService`]，见行内「读侧」段落） |
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
use mupc_display_proto::{
    AuditPage, AuditResult, ConsoleAuditEntry, ConsoleOp, OpOption, AUDIT_PAGE_SIZE,
    CONSOLE_OPERATOR,
};
use mupc_security::audit::{AuditEventType, AuditLogger, AuditSeverity};

use crate::bounded_io::{BoundedLine, BoundedLineReader};
// `from`/`to`/`cursor` 的毫秒解析**只有一份实现**（本轮上收，见 `log_service::parse_ms`）：
// 原先本文件与 `log_service.rs` 各抄一份、**错误文案还不同**（"不是非负整数毫秒" vs
// "不是合法的非负毫秒数"）—— 同一类参数在两页上给出两种 400 文案，现场对拍时会被当成两类错。
use crate::log_service::parse_ms;

// ═══════════════════════════════════════════════════════════════════════════
// 落点口径（**写侧与读侧的单一真源**）
// ═══════════════════════════════════════════════════════════════════════════

/// 审计文件名前缀 / 后缀：`console-audit-YYYY-MM-DD.jsonl`（设计 §4.5 落点）。
///
/// ⚠️ **必须是唯一拼名点**：写侧（[`FileAuditSink::console_file`]）与读侧
/// （[`ConsoleAuditService::page`] 的"按日期定位文件"）各写一份字面量的话，改一处漏一处就是
/// "审计页**永远查不到**新记录"——而且**静默**（读侧会把空目录读成空态「当前筛选条件下无审计记录」，
/// 而不是报错）。故抽成常量 + [`console_audit_file_name`]。
const CONSOLE_AUDIT_PREFIX: &str = "console-audit-";
/// 见 [`CONSOLE_AUDIT_PREFIX`]。
const CONSOLE_AUDIT_SUFFIX: &str = ".jsonl";
/// 文件名里的日期格式（UTC）——写侧由条目的 `ts_ms` 推出、读侧由窗口端点推出，两侧必须同格式。
///
/// 私有（本轮收窄）：全 crate 只有本文件用（写侧的 `console_file` 与读侧的 `day_of_ms`）。
/// 曾经是 `pub(crate)`，但**没有任何跨模块使用者** —— 敞着一个没人用的口子只会让下次改动
/// 以为"别处在依赖它"（真实的跨模块面是 [`ConsoleAuditService`] / [`AuditQuery`] /
/// [`parse_query`] / [`FileAuditSink`] / [`ConsoleAuditSink`]，那几个保持 `pub(crate)`）。
const AUDIT_DAY_FORMAT: &str = "%Y-%m-%d";

/// `YYYY-MM-DD` ⇒ 审计文件名。私有，理由见 [`AUDIT_DAY_FORMAT`]。
fn console_audit_file_name(day: &str) -> String {
    format!("{CONSOLE_AUDIT_PREFIX}{day}{CONSOLE_AUDIT_SUFFIX}")
}

/// 审计文件名 ⇒ `YYYY-MM-DD`；**不匹配即 `None`**（目录里别的东西——例如既有哈希链的
/// `audit_*.jsonl`——必须被**忽略而不是**解析，见写侧 `a_second_sink_in_the_same_dir_continues_the_existing_chain`
/// 的同款风险）。
///
/// 形状**逐位校验**（`NNNN-NN-NN`）：只按前后缀取子串的话，`console-audit-zzz.jsonl` 会拿到一个
/// 假日期并参与字符串比较（可能落进窗口 ⇒ 被打开 ⇒ 把整页打成"不可用"）。
fn console_audit_day(name: &str) -> Option<&str> {
    let day = name
        .strip_prefix(CONSOLE_AUDIT_PREFIX)?
        .strip_suffix(CONSOLE_AUDIT_SUFFIX)?;
    let b = day.as_bytes();
    if b.len() != 10 {
        return None;
    }
    let shape_ok = b.iter().enumerate().all(|(i, c)| match i {
        4 | 7 => *c == b'-',
        _ => c.is_ascii_digit(),
    });
    shape_ok.then_some(day)
}

/// Unix 毫秒 ⇒ UTC 日期串（`YYYY-MM-DD`）。
///
/// 越界值**夹到** [`AUDIT_TS_MAX_MS`]（而不是回落到"今天"）：读侧若拿"今天"去顶一个越界窗口端点，
/// 会得到一个**假的窗口**（把用户没要的日子读进来）。写侧 `FileAuditSink::console_file` 保留它
/// 原有的 `unwrap_or_else(Utc::now)`——那是另一条路径，本轮**不动它**。
/// ⚠️ **不 panic**（评审建议 ⑥）：这里曾经是 `.expect("已夹进 [0, AUDIT_TS_MAX_MS] ⇒ 必为合法时戳")`
/// —— 该断言在当前常量下**确实不可达**，但它把"常量取值"与"用户可控输入不得打 panic"绑在了一起：
/// 谁把 [`AUDIT_TS_MAX_MS`] 调过 8_210_266_876_799_999，一次查询就会 panic（**输入打挂进程**）。
/// 现在改成兜底 [`AUDIT_TS_MAX_DAY`]（形状合法、语义仍是"夹取后的那一天"）。
fn day_of_ms(ms: u64) -> String {
    let ms = ms.min(AUDIT_TS_MAX_MS) as i64;
    match DateTime::<Utc>::from_timestamp_millis(ms) {
        Some(dt) => dt.format(AUDIT_DAY_FORMAT).to_string(),
        // 不可达（见 [`AUDIT_TS_MAX_MS`] 的硬约束）。**不得**回落 `Utc::now`：那会把一个越界
        // 端点变成一个**用户没要的窗口**（凭空多给/少给），而这里只需给一个"夹取上界"的日名。
        None => AUDIT_TS_MAX_DAY.to_string(),
    }
}

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
    /// 要防的事（设计 §3.3：审计是唯一操作凭据）。装配点（`startup::console_write_paths` 的
    /// `Err` 分支）据此让**两条写路径整体**不可用：配置写
    /// [`crate::console_host::ApplySource::Unavailable`] + 联锁写
    /// [`crate::console_host::InterlockOpsSource::AuditUnavailable`]（单元 J 起是**两条**写
    /// 路径；不得只降级其中一条）。
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
            .format(AUDIT_DAY_FORMAT)
            .to_string();
        // 拼名走**共用**的 [`console_audit_file_name`]（读侧的"按日期定位文件"是同一个真源）。
        self.dir.join(console_audit_file_name(&day))
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
        let line = serde_json::to_string(entry).map_err(|e| format!("序列化审计条目失败: {e}"))?;
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

// ═══════════════════════════════════════════════════════════════════════════
// 读侧：审计查询（F19 / PL-2）—— 开发单元 **I**（`ConsoleAuditService`）
// ═══════════════════════════════════════════════════════════════════════════
//
// # 只读铁律（PL-02 / F19.5）
//
// 本段**没有任何**删除 / 清空 / 导出 / 改写的入口：`page` 只做 `read_dir` + `File::open`
// （`tokio::fs::File::open` ⇒ `O_RDONLY`），**且不创建目录**。对照写侧 `FileAuditSink::open`
// 的 `create_dir_all`：读侧若也建目录，就会把"审计根本没在跑"伪装成"审计跑起来了"
// （`log_service.rs` 模块头记过同款事故的变体）。
//
// # 「不可用」走 `AuditPage{available:false}`（HTTP 200），**不走** 503 —— 本单元的裁决
//
// 设计 §4.5 / EDGE-17 的原文是「目录不可读/解析失败 → **结构化错误** → UI 显『审计记录不可用』」。
// 两种落地都能"表达"不可用；本轮**选 `available=false`（200）**，三条理由按强弱排序：
//
// 1. **503 在本架构里到不了那个 UI 态**（决定性）：`ConsoleClient` 对非 2xx 落
//    `ConsoleError::HttpStatus`（设计 §3.4 补注明写：**不解析错误体**）⇒ `control_route::route`
//    不会被调用（读端点也要 `outcome.query()`）⇒ `P5AuditPage::set_page` **一次都不会被调用**
//    ⇒ 列表区**不会**进入 `ListView::Unavailable`，只会飘一条通用"通道失败" Toast。而
//    200 + `available=false` 经 `set_page` **直达**页面（`p5_audit.rs::list_view_of` 的
//    `!available` 分支优先于行数）——**那才是** EDGE-17 要求的那个态。
// 2. 契约**专门为它留了字段**，并注明缺省 `false` 是**安全方向**（`display-proto/src/audit.rs`
//    的 `AuditPage::available` 文档）⇒ 契约作者预期的表达方式就是"在页对象里说"。
// 3. 与 `LogService`（单元 H）的 503 **不矛盾**：日志契约**没有** availability 字段 ⇒ 它只能
//    走状态码；审计契约有 ⇒ 按契约走。**先例不能照搬，因为两侧契约不同构**。
//
// "结构化"在这里的含义：响应体是**结构化的 `AuditPage`**（含显式可用性标志），不是裸 5xx /
// 散文错误——这是 §4.5 那句话在本契约下**可落地**的那一半。⚠️ **如实登记**：本裁决把
// "不可用"从 HTTP 状态码挪进了 body，若 PM 认为必须同时给非 2xx，则须**先改渲染端**
// （否则屏上反而退化成通用失败提示），属跨端裁定，非本单元可自了。
//
// # 与「确实没有」的结构性区分（EDGE-17 的验收面）
//
// | 事实 | 响应 |
// |------|------|
// | 目录可读、窗口内确实没有记录 | `200` + `available=true ∧ entries=[]`（UI：空态「当前筛选条件下无审计记录」） |
// | 目录读不出来 / 文件读不出来 / 某行解析不了 / 预算耗尽 | `200` + `available=false`（UI：「审计记录不可用」） |
//
// 两条各有**能变红**的用例，且有一条用例直接把两种页对象 `assert_ne!` 比出来。
//
// # 窗口与"不扫全库"
//
// 设计 §4.5：「按日期定位文件（**不扫全库**）→ 解析 → 按 `from/to/ops` 过滤 → 倒序 → 分页 20
// → 返回 `has_more` 与 `newest_ts_ms`」。落地方式：**先 `read_dir` 只按文件名日期筛出候选**，
// 窗口外的日期**根本不 `open`**（用例 `only_the_requested_days_files_are_opened` 用"窗口外放一个
// 同名目录 + 一个**行数远超行预算**的大文件"证明它没被打开），再逐文件解析、按 `from/to` + `ops`
// 过滤、页内倒序。
//
// ⚠️ 措辞订正（代码质量评审点名）：原文写"一个**超长**文件"，而该用例实际造的是"**行数**超预算的
// 大文件"——"超长"极易读成"超长**行**"，而后者的处置是 [`AUDIT_LINE_MAX_BYTES`] 那条"整页不可用"、
// 与前者（[`AUDIT_SCAN_MAX_LINES`]）语义完全不同。两条闸的用例都在 ⑦ 里，别混。
//
// # ⚠️ 与渲染端的口径缺口（**如实登记，本单元不改渲染端**）
//
// `control_route.rs::audit_query_string` 在 `range == Custom` 时才发 `from`/`to`；`H1` 与 `H24`
// 发出的查询串**逐字节相同**（都不带时间参数）⇒ 服务端**无法**区分"最近 1 小时"与"最近 24 小时"。
// 故缺省窗口取**两者中较宽的那个**（24 小时）：对 H1 是**多给**（用户可自行缩小），对 H24 是**精确**；
// **不会**出现"用户选了 24 小时、服务端却只给 1 小时"这种**静默少给**（那正是本项目禁止的
// "把存在的记录说成没有"）。要真正区分档位，须在契约 §3.4 的审计请求列补 `range`（跨端裁定）。
//
// ⚠️ **缺口的具体形态（登记 ①，跨端待裁）**：`p5_audit.rs` 的 `AuditQuery::default().range`
// 就是 **`LogRange::H1`**（`p5_audit.rs:878-881`），而 `audit_query_string` 对 `H1` 与 `H24`
// **一个字都不发**（`control_route.rs:388-400`）⇒ **用户一进 P5，chip 上写的是「最近 1h」，
// 列表给的却是 24h 的行**（两侧都"没说谎"，但**屏上自相矛盾**）。本单元取 24h 是
// "绝不静默少给"的**正确方向**（多给可自行缩小），但**要真正区分档位**，只能二选一：
// ① 契约 §3.4 的审计请求列**补 `range`**；② 渲染端在 `H1` 时改发 `from`/`to`。
// 两条都要动**别的单元** ⇒ 登记为**跨端待裁**，本单元不自行改渲染端或契约。
//
// # 登记清单（规格评审本轮点名；**只登记、不改语义**，每条给理由）
//
// | # | 事项 | 处置与理由 |
// |---|------|-----------|
// | ① | **缺省 24 h 与渲染端 chip 的缺口**（跨端） | 见上一段：`AuditQuery::default().range == H1` ⇒ chip「最近 1h」/列表 24h。取宽是正确的**方向**（不静默少给），但真正区分档位须改契约 §3.4 请求列或渲染端 ⇒ **跨端待裁** |
// | ② | **设计 §4.5「结构化错误」与 `200 + available=false` 的字面张力** | PRD EDGE-17 原文是「提供审计存储**可用性标志**；查询失败返回**结构化错误**」——**两半都要**。本实现两半都给了：可用性标志在 `AuditPage::available`，结构化在响应体本身就是**结构化的 `AuditPage`**（不是裸 5xx / 散文）⇒ **不算违**。但设计 §4.5 那句「目录不可读/解析失败 → 结构化错误」**字面上**读起来像"必须是非 2xx" ⇒ **建议设计侧补一句消歧**（"结构化错误"指响应体的形状，不是状态码）；本单元只登记 |
// | ③ | **`page` 越界形态** | **前提**：本页扫描在预算内完成（候选文件 ≤ [`AUDIT_SCAN_MAX_FILES`]、行 ≤ [`AUDIT_SCAN_MAX_LINES`]、命中载荷 ≤ [`AUDIT_MATCHED_MAX_BYTES`]）。此时只有 1 页却 `page=2` ⇒ `available=true ∧ entries=[] ∧ has_more=false`（**落空态**，不是不可用）。⚠️ **预算耗尽时越界页同样落 `available=false`**（措辞订正，评审实测 + 本轮复跑的**隔离构型**：【最新日文件 21 条命中 + 其后 365 个空日文件】⇒ `page=1` = `available=true, entries=20`（读到第 1 个文件即 `matched=21 >= need=21` 早停）；`page=2` = `available=false, entries=0`（`need=41`，早停失效 ⇒ 一路扫到文件闸））——机理是越界页把 `need = (page−1)×20 + 21` 抬高（对 `page=2` 是 **41**）⇒ `matched.len() >= need` 更难成立 ⇒ **文件边界上的早停失效** ⇒ 一路扫到闸耗尽。这不是"越界页特判成不可用"，而是"这次扫描本来就超预算" ⇒ 与"窗口内确实没有"同一处置。渲染端只在 `has_more=true` 时才请求下一页（`p5_audit` 的翻页前提）⇒ 生产**不可达**；且**不造契约没有的信号**（契约里没有"页码越界"这个错误位）⇒ 不新增 4xx/不可用，只登记。用例 ⑰ 顺手钉住该形态（末页之后恒空，且其语料远在预算内） |
// | ④ | **`newest_ts_ms` 是「窗口内」最新、且忽略 `ops` 筛选** | 见 [`ConsoleAuditService::scan`] 内联注释：F19.8 的用途原文是"判断审计链路是否在**持续写入**" = **链路活性信号**。取全库最新要在窗口外再做 IO（把 F19.4 的有界代价变成无界），且会让**窗口外**文件的损坏把本页打成不可用 ⇒ 语义定为"本次窗口内最新一条"，**不是**"全库最新一条" |
// | ⑤ | **PRD F19.8 写「首页展示」最近一条审计时间戳，实际落在 P5 页眉** | 渲染端 `p5_audit.rs`（`TEXT_NEWEST_PREFIX`）在 P5 页眉展示该值时，`p1_status.rs` **一处 audit 引用都没有**；UI 设计 §3.6 也把该行画在 P5 头部 ⇒ **PRD 与 UI 文档口径不一致**，非本单元问题 ⇒ 登记（服务端只提供值，落点由渲染端决定） |

/// 缺省窗口（**无 `from` / `to`**）：最近 24 小时。
///
/// 取值理由见上面「与渲染端的口径缺口」：`H1` / `H24` 在线上不可区分，取**较宽**者 ⇒
/// 任何相对档位下都**不会**静默少给记录。⛔ **不得**把它调窄（会重新引入静默少给）。
const AUDIT_DEFAULT_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;

/// 时戳上界（`9999-12-31T23:59:59.999Z`）：`from`/`to` 超出即**夹到**此处。
///
/// 理由：`u64 → i64` 的裸转会在大值上**变号**（`u64::MAX as i64 = -1` ⇒ 推成 1969 年），
/// 与"窗口端点"语义完全相反。夹取后的行为是显式且单调的（任何更大的 `to` 都等价于"到 9999 年"）。
///
/// ⛔ **硬约束：必须 ≤ 8_210_266_876_799_999**（`chrono` 能表示的毫秒上界；越过它
/// `DateTime::from_timestamp_millis` 返回 `None`）。**超过不会 panic** —— [`day_of_ms`] 已改成
/// 不 panic 的写法（评审建议 ⑥）—— 但会落进它的兜底分支（回 [`AUDIT_TS_MAX_DAY`]）。私有，
/// 理由见 [`AUDIT_DAY_FORMAT`]。
const AUDIT_TS_MAX_MS: u64 = 253_402_300_799_999;

/// [`AUDIT_TS_MAX_MS`] 对应的 UTC 日期 —— [`day_of_ms`] 的**不 panic 兜底**返回值。
///
/// 只有"有人把上界调到 chrono 区间之外"才用得到（当前常量下**不可达**）。取值与上界**必须
/// 同步修改**：它表达的是"窗口端点被夹到哪一天"，不是"今天的日期"（回落到今天会凭空造出一个
/// 用户没要的窗口 —— 见 [`day_of_ms`] 里"为什么回落今天是不行的"）。形状（`NNNN-NN-NN`）
/// 必须合法，否则 [`console_audit_day`] 会把这个日名当成无关文件忽略掉。
const AUDIT_TS_MAX_DAY: &str = "9999-12-31";

/// 单次请求最多打开多少个**窗口内**的审计文件（安全阀）。
///
/// - **正常路径摸不到它**：缺省窗口（24 小时）最多覆盖 2 个日期文件；自定义窗口下，读到凑够
///   本页所需条数即停（`need = page × 20 + 1`）⇒ 通常只开 1 个文件。
/// - 只有"自定义一个很宽的窗口 **且** 筛选后一条都没有"（比如查一个从未发生过的操作类型）才会
///   一路走到这里。取 **365**（≈1 年日切文件）：超过一年的历史倒查在本 UI 上并不现实
///   （20 条/页、无跳页）⇒ 本服务的**最坏代价因此可算**：
///   **≤ 365 次 `open` + ≤ [`AUDIT_SCAN_MAX_LINES`] 行 + ≤ [`AUDIT_MATCHED_MAX_BYTES`] 命中载荷**
///   （三道闸同款处置：超限一律可见拒绝，见 [`AUDIT_MATCHED_MAX_BYTES`] 里"为什么必须三道"）。
/// - 触发时**回 `available=false`**（不可用），**不**静默只扫一部分：审计是合规凭据，
///   "只扫了一部分"绝不能伪装成"没有记录"（EDGE-17 的同一口径）。
const AUDIT_SCAN_MAX_FILES: usize = 365;

/// 单次请求最多读入多少行（**含窗口外的行**——整文件读，行是代价单位）。见 [`AUDIT_SCAN_MAX_FILES`]。
///
/// 5 万条控制台审计 = 5 万次人触发的写操作，现实里远超一年；这道闸兜的是"目录里被塞进了
/// 别的东西/被压过的二进制"这类**读不懂或读不完**的输入（与 `log_service.rs` 的硬代价闸同口径）。
const AUDIT_SCAN_MAX_LINES: usize = 50_000;

/// 单次 `read` 的块大小（字节）。
const AUDIT_READ_CHUNK_BYTES: usize = 64 * 1024;

/// 单行字节上限：**超过即整行丢弃 ⇒ 整页不可用**（复用 `crate::bounded_io` 的有界读）。
///
/// 正常审计条目 ≤ 数百字节（`before`/`after` 是小标量）；64 KiB 的下限口径与
/// `log_service` 的 `/logs/targets` 相同（足够装下任何 sane 记录，又不让单次分配无界）。
/// ⚠️ **为什么不"跳过这一行继续"**：那会把一条**确实存在**的记录静默抹掉，页面却仍说
/// `available=true`——即"把存在的说成不存在"。取"整页不可用"（可见、可排查）符合
/// `log_service.rs` 立下的同一取舍：「宁可可见拒绝，不可静默丢条」。
const AUDIT_LINE_MAX_BYTES: usize = 64 * 1024;

/// 单次请求**命中条目**的累计字节上限（代码质量评审「重要 ①」）。
///
/// # 为什么必须有这道闸（行闸挡不住它）
///
/// [`AUDIT_SCAN_MAX_LINES`] 只约束**读入的行数**；单行上限 [`AUDIT_LINE_MAX_BYTES`] = 64 KiB
/// ⇒ 两者相乘就是**行闸隐含的最坏载荷**：**50 000 × 64 KiB ≈ 3.05 GiB**
/// （= 3 276 800 000 B ≈ 3.28 GB；**输入字节**口径，两个口径之别见下一节）
/// （同一条路径上日志侧是"行闸 + 字节闸"双闸，审计侧此前只有行闸 —— 评审用一个"单文件 3 万条
/// 合法条目 + 末行畸形、窗口只需 21 条"的探针实测：`available=false`，即实现**确实把整文件
/// 读完、`matched` 一路涨**）。
/// 更糟的是它**不可见**：`:need` 的早停只在**文件边界**判断 ⇒ 单文件内 `matched` 不受 `need`
/// 约束，涨到多少取决于文件里恰好有多少条**合法且命中**的记录。
///
/// # ⚠️ 两个口径必须分开读：本闸计的是**输入字节**，不是**常驻**（代码质量评审点名）
///
/// `matched_bytes` 累加的是每行的**原始字节数**（`scan_one_file` 里 `bytes.len()`）——那是
/// **输入**给的量，也正是"人工塞巨型条目"能撑大的那个量。而 `matched` 里那个
/// `Vec<ConsoleAuditEntry>` 的**实际常驻**是**另一回事**：每条是 5 个 `String`
/// （`id` / `operator` / `target` / `reason` / `request_id`）加各自堆上的字节，再加 2 个
/// `serde_json::Value`（`before` / `after`），外加 `Vec` 容量按 2 的幂翻倍与分配器对齐
/// ⇒ 粗估是**输入字节的 1.5–2×**。同一件事因此有两个数：
///
/// | 场景 | 输入字节（**本闸计的就是这个**） | 实际常驻（粗估 1.5–2×） |
/// |------|----------------------------------|--------------------------|
/// | 合法最坏：50 000 条**小**条目（⑱(a) 必须照常服务） | ≈ **10.8 MB** | ≈ **15–20 MB** |
/// | 本闸触发前的那一刻（32 MiB 输入被吃满） | **32 MiB** | ≈ **50–65 MB** |
///
/// ⚠️ 那个 1.5–2× 只对**小条目**成立（固定开销占比高）；条目越大，该比值越趋近 1×
/// ⇒ **不对闸前的算术最坏外推**。**结论不变**（本闸仍把最坏挡在远小于闸前的量级上），
/// 但**别把 10.8 MB 读成常驻** —— 它是**输入字节**。
///
/// # 取值论证（不拍脑袋；下表一律是**输入字节**口径）
///
/// | 事实 | 数 |
/// |------|-----|
/// | 本文件测试语料 `ent()` 的单条序列化长度（**实测**：一次性测量所得，测量代码未入库） | **215 B** |
/// | ⑱(a) 钉住的合法边界：恰好 50 000 行、行行命中 ⇒ 必须照常服务 | 50 000 × 215 B ≈ **10.8 MB** |
/// | 闸值 | **32 MiB**（= 33 554 432）⇒ 对上面的合法最坏有 **≈3.1× 余量** |
/// | 闸前的算术最坏（行闸 × 单行上限） | ≈ 3.05 GiB ⇒ 本闸把它压到 **≈1/97.6** |
///
/// 下限口径与 [`AUDIT_LINE_MAX_BYTES`] 同源：**必须大于"行闸吃满时正常条目的总尺寸"**，
/// 否则一条**合法**查询（50 000 行小条目）会被它误伤 —— 那正是 ⑱(a) 会变红的情形。
/// 上限口径：生产单条 ≤ 数百字节（`before`/`after` 是小标量），32 MiB ≈ 15 万条正常条目，
/// 远超"一年日切文件 × 人触发写操作"的现实规模；只有"人工塞进来的巨型条目"才够得着它。
///
/// # 触发时的处置：与另两道闸**逐字同款**
///
/// `Err` ⇒ [`ConsoleAuditService::page`] 落 `available=false`（**可见拒绝**），
/// **不**静默截断成"只回前 N 条"：审计是合规凭据，"只回了一部分"伪装成"就这些"是红线
/// （EDGE-17 的同一口径）。
const AUDIT_MATCHED_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// 解析后的审计查询（设计 §3.4：`from`/`to`(ms) `ops`(多值) `page`(1-based)；页大小恒为契约常量）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditQuery {
    /// 窗口起始（Unix ms，**含**）；`None` = 用缺省窗口（见 [`AUDIT_DEFAULT_WINDOW_MS`]）。
    pub from_ms: Option<u64>,
    /// 窗口结束（Unix ms，**含**）；`None` 同上。
    pub to_ms: Option<u64>,
    /// 操作类型多选（**空 = 不筛**）。
    pub ops: Vec<ConsoleOp>,
    /// 页码（**1-based**）。
    ///
    /// ⚠️ **`0` 不是合法值**（[`parse_query`] 直接拒），但本字段是 `pub` ⇒ 不变式**不能**只写在
    /// 注释里：请求路径上的 `page` 一律走 `u64::from(q.page).saturating_sub(1)`
    /// （见 [`ConsoleAuditService::scan`]），让手搓的 `page = 0` **饱和成第 1 页**而不是
    /// `u64` 下溢（debug 下 panic / release 下回绕成 `u64::MAX`）。用例 ⑰ 末尾钉住该行为。
    pub page: u32,
    // ⚠️ **为什么没有 `page_size` 字段**（原字段已于本轮删除，见下）：契约把它钉死为
    // [`AUDIT_PAGE_SIZE`]（`parse_query` 对线上的 `page_size ≠ 20` 一律 400）。把它**存进本结构**
    // 就造出了一个**能被 `pub` 字段改掉的第二真源**——而服务端只有一种真实行为（恒回 20 条/页）。
    // 两个选项里选"删字段"而不是"让 `page()` 用它"：后者会让一个手搓的 `page_size = 5` 既影响
    // 分页、又被回显进 `AuditPage.page_size`，等于**让非法请求体决定契约响应**；删掉之后
    // "页大小"只剩一个真源（契约常量），本结构只承载**真正会变的东西**。
}

impl Default for AuditQuery {
    fn default() -> Self {
        Self {
            from_ms: None,
            to_ms: None,
            ops: Vec::new(),
            page: 1,
        }
    }
}

impl AuditQuery {
    /// 生效窗口（**含两端**）。
    ///
    /// `from`/`to` 由 [`parse_query`] 保证**齐备或齐缺**（半截窗口即拒），故此处只需两分支。
    pub fn window(&self, now_ms: u64) -> (u64, u64) {
        match (self.from_ms, self.to_ms) {
            (Some(f), Some(t)) => (f, t),
            _ => (now_ms.saturating_sub(AUDIT_DEFAULT_WINDOW_MS), now_ms),
        }
    }
}

/// 解析 `(键, 值)` 序列（**重复键 = 多值**，设计 §3.4 补注；**不是**逗号拼接）。
///
/// # 校验口径（逐条给理由；与 `log_service::parse_query` 同风格，便于现场对照）
///
/// | 情形 | 处置 | 理由 |
/// |------|------|------|
/// | 未知键 | **拒**（400） | 静默忽略一个筛选参数 = 在"用户以为已筛"的画面下给**未筛**的数据（本项目最忌的静默失实） |
/// | `ops=config_apply,interlock_release` | **拒** | §3.4 补注：多值一律**重复键**；容错地把逗号当分隔符等于自造第二种编码 |
/// | `ops=`（空值） | **拒** | 空值匹配不到任何条目 ⇒ 静默空页；渲染端不发空值键（空集 = 不发键） |
/// | `from` / `to` 齐缺 | 取缺省窗口 | 缺省不是非法（渲染端相对档位下就是不发的） |
/// | 只给 `from` 或只给 `to` | **拒** | 半截窗口无意义，且"从 X 到现在"与"从 1970 到 Y"是**两种**猜测，不能替用户选 |
/// | `from > to` | **拒** | 空窗口是"静默无结果"的伪装（应为显式非法） |
/// | `page` 缺省 | 取 `1` | 1-based 的首屏 |
/// | `page = 0` | **拒** | 契约明写 1-based（服务侧另有一道 `saturating_sub` 兜底，防的是**绕过本函数**直接构造 `AuditQuery`） |
/// | `page_size` 缺省 | 合法（就是 [`AUDIT_PAGE_SIZE`]） | 契约只允许 20 ⇒ 缺省即那唯一的合法值；值本身**不入 `AuditQuery`**（见该结构里的"为什么没有 `page_size` 字段"） |
/// | `page_size ≠ 20` | **拒** | 契约把它钉死为 20。静默改小/改大会让调用方以为拿到了它要的页大小（同 `limit` 的既有口径） |
pub fn parse_query(pairs: &[(String, String)]) -> Result<AuditQuery, String> {
    let mut q = AuditQuery::default();
    let (mut from, mut to) = (None, None);
    let mut page: Option<u32> = None;

    for (k, v) in pairs {
        match k.as_str() {
            "from" => from = Some(parse_ms(v, "from")?),
            "to" => to = Some(parse_ms(v, "to")?),
            "ops" => {
                if v.is_empty() {
                    return Err("ops 的值不得为空（空集应不发该键）".to_string());
                }
                q.ops.push(parse_op(v)?);
            }
            "page" => {
                let n: u32 = v.parse().map_err(|_| format!("page 不是正整数: {v}"))?;
                if n == 0 {
                    // `0` 与"缺省"不同：缺省 = 第一页（1-based），显式 0 是**非法**取值（不是"第 0 页"）。
                    return Err("page 从 1 开始（1-based），0 非法".to_string());
                }
                page = Some(n);
            }
            "page_size" => {
                let n: u32 = v
                    .parse()
                    .map_err(|_| format!("page_size 不是正整数: {v}"))?;
                if n as usize != AUDIT_PAGE_SIZE {
                    return Err(format!(
                        "page_size 只能是 {AUDIT_PAGE_SIZE}（契约常量）: {n}"
                    ));
                }
                // 值通过校验即可：唯一合法值就是契约常量，故**不**存进 `AuditQuery`
                // （存了就是一个能被 `pub` 字段改掉的第二真源，见该结构的字段说明）。
            }
            other => return Err(format!("未知查询参数: {other}")),
        }
    }

    match (from, to) {
        (None, None) => {}
        (Some(f), Some(t)) => {
            if f > t {
                return Err(format!("窗口非法: from({f}) > to({t})"));
            }
            q.from_ms = Some(f);
            q.to_ms = Some(t);
        }
        _ => {
            return Err(
                "from / to 必须同时给出（半截窗口无意义：只给一端时窗口的另一端无处可取）"
                    .to_string(),
            )
        }
    }

    q.page = page.unwrap_or(1);
    Ok(q)
}

/// `ops` 的单个值 ⇒ 契约枚举。
///
/// **借 serde 反序列化**（而不是在本文件再列一份线名表）：契约的 `#[serde(rename_all = "snake_case")]`
/// 就是线名的唯一真源 ⇒ 将来契约增收/改名时，这里**自动**跟随，不会出现"第二份线名表"漂移。
fn parse_op(v: &str) -> Result<ConsoleOp, String> {
    serde_json::from_value::<ConsoleOp>(serde_json::Value::String(v.to_string())).map_err(|_| {
        format!(
            "ops 非法: {v}（合法: config_apply|config_reset_default|interlock_release|interlock_ack_m1；\
             多值用重复键）"
        )
    })
}

/// **审计查询服务**（设计 §3.4 的承接组件 `ConsoleAuditService`；F19 / PL-2）。
///
/// 无状态（只持目录）⇒ 天然只读、天然并发安全（与 `LogService` 同款）。
#[derive(Debug, Clone)]
pub struct ConsoleAuditService {
    /// 审计目录（`{system.log_dir}/audit`）——**与写侧 `FileAuditSink` 同一个值**。
    dir: PathBuf,
}

impl ConsoleAuditService {
    /// 构造（**不做任何 I/O**：目录不存在时**不**在此建目录、也**不**在此报错 ⇒
    /// "读不出来"在**请求期**以 `available=false` 表达，而不是在装配期把整个控制通道打挂）。
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `/v1/console/audit/ops` 的返回（设计 §3.4：`Vec<{op, label}>`）。
    ///
    /// **纯契约常量、零 I/O**：选项集合是"本期须审计的写操作"（PL-1 定稿的 4 类），**不是**
    /// 从审计文件里统计出来的 ⇒ 审计源不可用时它**照旧可得**（否则现场连"能筛什么"都看不到）。
    /// 渲染端 `p5_audit::canonical_ops()` 在未注入时也取同一集合 ⇒ 两侧逐字一致。
    pub fn op_options(&self) -> Vec<OpOption> {
        ConsoleOp::ALL.iter().copied().map(OpOption::from).collect()
    }

    /// `/v1/console/audit`：**恒回 `AuditPage`**（不可用经 `available=false` 表达，不经 `Err`/5xx
    /// ——理由见本段模块注释）。
    ///
    /// `now_ms` = 假时钟注入点（缺省窗口 = `now − 24h`）；由 handler 取 `config_service::now_ms()`。
    pub async fn page(&self, q: &AuditQuery, now_ms: u64) -> AuditPage {
        match self.scan(q, now_ms).await {
            Ok((entries, has_more, newest)) => AuditPage {
                entries,
                page: q.page,
                page_size: AUDIT_PAGE_SIZE as u32,
                has_more,
                newest_ts_ms: newest,
                available: true,
            },
            Err(reason) => {
                tracing::error!(
                    dir = %self.dir.display(),
                    reason,
                    "审计源不可用 ⇒ GET /v1/console/audit 回 available=false\
                     （EDGE-17：屏上显「审计记录不可用」，不得显「无审计记录」）"
                );
                AuditPage {
                    // 不可用 ⇒ 其余字段**一个都不主张**（`entries` 空不是"没有记录"，`newest` 空
                    // 不是"链路没写过"）；渲染端 `list_view_of` 的 `!available` 分支优先于行数，
                    // `footer_text(_, 0)` 亦不入屏 ⇒ 这些默认值**不会**被读成事实。
                    entries: Vec::new(),
                    page: q.page,
                    page_size: AUDIT_PAGE_SIZE as u32,
                    has_more: false,
                    newest_ts_ms: None,
                    available: false,
                }
            }
        }
    }

    /// 扫描 + 过滤 + 倒序 + 分页。
    ///
    /// 返回 `(本页条目, has_more, newest_ts_ms)`；任何"读不全 / 读不懂"的情形一律 `Err`
    /// （由 [`ConsoleAuditService::page`] 落成 `available=false`）。
    ///
    /// # 结构（本轮重构：133 行 / 嵌套 4 层 ⇒ 只剩"取窗口 → 枚举筛日 → 逐文件 → 分页"）
    ///
    /// 目录枚举搬进 [`ConsoleAuditService::candidate_files`]、单文件的行循环搬进
    /// [`scan_one_file`]、累计态与**三道闸**收进 [`ScanState`]。
    /// **动机不是好看**：代码质量评审指出"命中载荷闸"正是从那个 4 层嵌套里漏出去的
    /// （`matched` 只被行闸间接约束 ⇒ 算术最坏 3.05 GiB **输入字节**，口径见该常量）。现在"加一条命中记录"必须经过
    /// [`ScanState::push_entry`] 这**一个**入口，而三道闸就写在那个入口旁边。
    async fn scan(
        &self,
        q: &AuditQuery,
        now_ms: u64,
    ) -> Result<(Vec<ConsoleAuditEntry>, bool, Option<u64>), String> {
        let (from, to) = q.window(now_ms);
        // `from`/`to` 是时戳，文件名是**它所属的 UTC 日** ⇒ 端点所在的**整天**都在候选内
        // （窗口内某条记录不一定落在端点的当天，故必须按"整天"取文件，不能按"当天"取）。
        let (from_day, to_day) = (day_of_ms(from), day_of_ms(to));
        let files = self.candidate_files(&from_day, &to_day).await?;

        // 逐文件解析（新 → 旧），凑够本页所需即停。
        let page_size = AUDIT_PAGE_SIZE as u64;
        // ⚠️ **饱和减法**，不是裸 `- 1`：`page` 是 `pub` 字段 ⇒ "必须 ≥ 1"这条不变式
        // 只由 `parse_query` 守着，**拦不住直接构造**（单元 J / 测试都可以）。裸减法在
        // debug 下 `attempt to subtract with overflow` **panic**，release 下回绕成 `u64::MAX`
        // ⇒ `start`/`need` 极大 ⇒ 早停恒不成立 ⇒ 白扫到闸耗尽（静默落空）。
        // 饱和后 `page = 0` 退化成"第 1 页"（确定且不 panic，用例 ⑰ 末尾钉住）。
        let start = u64::from(q.page)
            .saturating_sub(1)
            .saturating_mul(page_size);
        let need = start.saturating_add(page_size).saturating_add(1); // 多要 1 条判 has_more
        let mut acc = ScanState::default();

        for (_, path) in &files {
            if acc.matched.len() as u64 >= need {
                break; // 本页已够 + 已能判 has_more；更旧的文件不可能含更新的记录
            }
            scan_one_file(path, from, to, &q.ops, &mut acc).await?;
        }

        // 文件内可能非严格按 `ts_ms` 落盘（跨零点重放 / 时钟回跳）⇒ 页内再排一次（`sort_by_key`
        // 是**稳定排序**：同 `ts_ms` 保持"新文件在前"的既有相对次序）。
        acc.matched.sort_by_key(|e| std::cmp::Reverse(e.ts_ms));
        let has_more = acc.matched.len() as u64 > start.saturating_add(page_size);
        let page_entries: Vec<ConsoleAuditEntry> = acc
            .matched
            .into_iter()
            .skip(start as usize)
            .take(AUDIT_PAGE_SIZE)
            .collect();
        Ok((page_entries, has_more, acc.newest))
    }

    /// ① 只**列目录**、按文件名日期筛出**窗口内**的候选文件，日期**倒序**。
    ///
    /// 窗口外的日期**根本不 `open`**（"不扫全库"：本次查询的代价与目录里躺了多少历史无关）。
    async fn candidate_files(
        &self,
        from_day: &str,
        to_day: &str,
    ) -> Result<Vec<(String, PathBuf)>, String> {
        let mut rd = tokio::fs::read_dir(&self.dir)
            .await
            .map_err(|e| format!("读审计目录 {} 失败: {e}", self.dir.display()))?;
        let mut files: Vec<(String, PathBuf)> = Vec::new();
        loop {
            let ent = rd
                .next_entry()
                .await
                .map_err(|e| format!("读审计目录 {} 失败: {e}", self.dir.display()))?;
            let Some(ent) = ent else { break };
            let name = ent.file_name().to_string_lossy().into_owned();
            // 非本类文件（哈希链 `audit_*.jsonl`、编辑器残留……）**一律忽略**：它们不是
            // `ConsoleAuditEntry`，去解析它们只会把整页打成"不可用"。
            let Some(day) = console_audit_day(&name) else {
                continue;
            };
            if day < from_day || day > to_day {
                continue; // 窗口外 ⇒ **不打开**（代价与"目录里有多少历史"无关）
            }
            let ft = ent
                .file_type()
                .await
                .map_err(|e| format!("取 {} 的文件类型失败: {e}", ent.path().display()))?;
            if !ft.is_file() {
                // 名字落在**本服务的命名空间**里却不是普通文件 ⇒ 无法保证读全 ⇒ 不可用
                // （读侧的"不可用"只是页对象上的一个标志，不会像 503 那样打挂别的通道）。
                return Err(format!(
                    "{} 与审计文件同名但不是普通文件（无法保证读全）",
                    ent.path().display()
                ));
            }
            files.push((day.to_string(), ent.path()));
        }
        // 日期倒序 ⇒ **跨文件的 ts 亦为倒序**：文件名日期由条目自身的 ts 推出（写侧 `console_file`），
        // 故 D1 > D2 时 D1 里任一条的 ts 必大于 D2 里任一条 ⇒ 可以"凑够就停"而不漏更新的记录。
        files.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(files)
    }
}

/// 一次扫描的**累计态 + 三重预算**（文件 / 行 / 命中载荷）。
///
/// 为什么把三个计数器与 `matched` / `newest` 放在一起：三道闸守护的正是这两个字段，而它们
/// 此前散落在 `scan` 的 4 层嵌套里 ⇒ "多了一处 `push` 却没过闸"在结构上是**可能**发生的
/// （评审实测指出的那条 3.05 GiB **输入字节**路线就是这么漏出去的）。现在命中条目的唯一写入口是
/// [`ScanState::push_entry`]，而**载荷闸就写在它旁边**。
#[derive(Default)]
struct ScanState {
    /// 本页候选（窗口内 ∧ 命中 `ops`）。
    matched: Vec<ConsoleAuditEntry>,
    /// 窗口内最新一条（**不受 `ops` 筛选影响**）。
    newest: Option<u64>,
    /// 已打开的候选文件数（闸见 [`AUDIT_SCAN_MAX_FILES`]）。
    files_read: usize,
    /// 已读入的行数（**含**窗口外 / 未命中的行 —— 行是代价单位；闸见 [`AUDIT_SCAN_MAX_LINES`]）。
    lines_read: usize,
    /// 已命中条目的累计字节（闸见 [`AUDIT_MATCHED_MAX_BYTES`]）。
    matched_bytes: u64,
}

impl ScanState {
    /// 打开下一个候选文件前的**文件闸**。
    fn enter_file(&mut self) -> Result<(), String> {
        self.files_read += 1;
        if self.files_read > AUDIT_SCAN_MAX_FILES {
            return Err(format!(
                "本次窗口内的审计文件数超过单次请求上限 {AUDIT_SCAN_MAX_FILES}（已读 {}）\
                 ⇒ 不静默只扫一部分；请缩小时间范围",
                self.files_read
            ));
        }
        Ok(())
    }

    /// 每读入一行（**含**窗口外 / 未命中的行）后的**行闸**。
    fn count_line(&mut self) -> Result<(), String> {
        self.lines_read += 1;
        if self.lines_read > AUDIT_SCAN_MAX_LINES {
            return Err(format!(
                "本次请求读入行数超过上限 {AUDIT_SCAN_MAX_LINES} ⇒ 不静默截断；请缩小时间范围"
            ));
        }
        Ok(())
    }

    /// 记一条**窗口内**的条目：先更新 `newest`，命中 `ops` 时再过**载荷闸**并进 `matched`。
    ///
    /// `bytes` = 该行的**原始字节数**（= 闸要计的**输入**量；⚠️ **不等于** `matched` 里那份的
    /// **常驻**规模 —— 两个口径之别见 [`AUDIT_MATCHED_MAX_BYTES`] 的「两个口径」一节）。
    fn push_entry(
        &mut self,
        entry: ConsoleAuditEntry,
        bytes: usize,
        ops: &[ConsoleOp],
    ) -> Result<(), String> {
        // `newest_ts_ms` 的口径：**窗口内**最新一条，且**不受 `ops` 筛选影响**。
        // 它服务的是"审计链路还在不在写"（F19.8）——按操作类型过滤会把这条信号变成
        // "某类操作最近一次是何时"，与链路活性无关。**登记**：语义是"本次窗口内最新一条"，
        // **不是**"全库最新一条"（后者要在窗口外再做 IO，且会让窗口外文件的损坏把本页打成
        // 不可用；如需改口径，须连同扫描预算一起重新裁定）。
        self.newest = Some(self.newest.map_or(entry.ts_ms, |n| n.max(entry.ts_ms)));
        if !ops.is_empty() && !ops.contains(&entry.op) {
            return Ok(());
        }
        // ⚠️ **载荷闸**（本轮新增，代码质量评审「重要 ①」）：行闸挡不住它 ——
        // 单文件内 `matched` 不受 `need` 约束（早停只在**文件边界**判断），而单行上限 64 KiB
        // ⇒ 算术最坏 50 000 × 64 KiB ≈ 3.05 GiB **输入字节**。取值论证见 [`AUDIT_MATCHED_MAX_BYTES`]。
        // 计**原始行字节**而不是"结构体大小"：前者是**输入**给的量（可被"塞巨型条目"撑大），
        // 后者由 Rust 布局决定（大体固定，闸就永远碰不到）。
        self.matched_bytes = self.matched_bytes.saturating_add(bytes as u64);
        if self.matched_bytes > AUDIT_MATCHED_MAX_BYTES {
            return Err(format!(
                "本次请求命中的审计条目累计超过上限 {AUDIT_MATCHED_MAX_BYTES} 字节\
                 ⇒ 不静默截断；请缩小时间范围"
            ));
        }
        self.matched.push(entry);
        Ok(())
    }
}

/// 读**一个**候选日文件，把"窗口内 ∧ 命中 `ops`"的条目累进 `acc`。
///
/// 从 `scan` 里抽出来的（本轮重构，**纯搬迁**：行循环体逐行未改，只把 `lines_read` 换成
/// `acc.lines_read`、`matched`/`newest` 换成 `acc` 的字段）。
async fn scan_one_file(
    path: &Path,
    from: u64,
    to: u64,
    ops: &[ConsoleOp],
    acc: &mut ScanState,
) -> Result<(), String> {
    acc.enter_file()?;
    let mut f = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("打开审计文件 {} 失败: {e}", path.display()))?;
    let mut r = BoundedLineReader::new(&mut f, AUDIT_READ_CHUNK_BYTES, AUDIT_LINE_MAX_BYTES);
    loop {
        let Some(line) = r
            .next_line()
            .await
            .map_err(|e| format!("读审计文件 {} 失败: {e}", path.display()))?
        else {
            break;
        };
        let bytes = match line {
            BoundedLine::Line(b) => b,
            BoundedLine::Overlong { bytes } => {
                // 超长行**整行已丢**（有界读的语义）⇒ 本页**不可能**完整 ⇒ 不可用。
                return Err(format!(
                    "审计文件 {} 有一行超过 {AUDIT_LINE_MAX_BYTES} 字节（已丢弃整行，读到 {bytes} B）\
                     ⇒ 无法保证不漏记录",
                    path.display()
                ));
            }
        };
        acc.count_line()?;
        // 解析失败 ⇒ **不可用**（设计 §4.5 逐字：「目录不可读/解析失败 → …UI 显『审计记录不可用』」）。
        // 绝不"跳过这一行继续"：那会把一条**确实存在**的记录静默抹掉（本项目硬红线）。
        let entry: ConsoleAuditEntry = serde_json::from_slice(bytes).map_err(|e| {
            format!(
                "审计文件 {} 第 {} 行解析失败（不跳过：跳过=把存在的记录说成不存在）: {e}",
                path.display(),
                acc.lines_read
            )
        })?;
        if entry.ts_ms < from || entry.ts_ms > to {
            continue; // 窗口是**整天取文件、精确到毫秒筛条目**（同一文件里可能只有一部分在窗口内）
        }
        acc.push_entry(entry, bytes.len(), ops)?;
    }
    Ok(())
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
        sink.record_outcome(&entry("a1", AuditResult::Ok, None))
            .unwrap();

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
        assert!(
            chain_lines[0].contains("\"sequence\":1"),
            "intent 是链上第一条"
        );
        assert!(chain_lines[0].contains("intent"), "intent 记录须可辨认");
        assert!(
            chain_lines[1].contains("\"sequence\":2"),
            "outcome 是链上第二条"
        );
        assert!(chain_lines[1].contains("outcome"));
        assert!(
            chain_lines[1].contains("rid-1"),
            "链上记录须带 request_id（现场对拍）"
        );
    }

    /// append-only：第二次写不得截断第一次的内容（PL-02「仅追加、不可删改」）。
    #[test]
    fn outcomes_are_appended_never_truncated() {
        let t = TempDir::new("audit-append");
        let sink = FileAuditSink::open(t.path()).unwrap();
        sink.record_outcome(&entry("a1", AuditResult::Ok, None))
            .unwrap();
        sink.record_outcome(&entry("a2", AuditResult::Failed, Some("越界")))
            .unwrap();
        let text = std::fs::read_to_string(t.join("console-audit-2025-09-09.jsonl")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            serde_json::from_str::<ConsoleAuditEntry>(lines[0])
                .unwrap()
                .id,
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
            s.record_outcome(&entry("a1", AuditResult::Ok, None))
                .unwrap();
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
        assert!(
            lines[2].contains("\"sequence\":3"),
            "序列号必须续接: {}",
            lines[2]
        );
        assert!(lines[2].contains("rid-2"));
        // 控制台文件没被链的写入污染（仍是 1 行，且仍是 ConsoleAuditEntry）
        let console = std::fs::read_to_string(t.join("console-audit-2025-09-09.jsonl")).unwrap();
        assert_eq!(console.lines().count(), 1);
        assert_eq!(
            serde_json::from_str::<ConsoleAuditEntry>(console.trim())
                .unwrap()
                .id,
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

    // ═══════════════════════════════════════════════════════════════════════
    // 单元 I：审计**查询**（`ConsoleAuditService`）
    // ═══════════════════════════════════════════════════════════════════════
    //
    // 口径：**测试不读时钟**（固定时戳 `T0`），文件一律用**真实写侧** [`FileAuditSink`] 落盘
    // ⇒ 验的是"读侧读得回写侧写的东西"，而不是"读侧读得回测试自己造的等价物"。

    /// 固定基准时刻：2025-09-09T10:00:00Z（与上方既有用例同源）。
    const T0: u64 = 1_757_412_000_000;
    /// 一天（毫秒）。
    const DAY: u64 = 86_400_000;

    /// 时刻 + 操作类型 ⇒ 一条审计条目（其余字段填可辨认值）。
    fn ent(ts_ms: u64, op: ConsoleOp) -> ConsoleAuditEntry {
        ConsoleAuditEntry {
            id: format!("id-{ts_ms}"),
            ts_ms,
            operator: CONSOLE_OPERATOR.to_string(),
            op,
            target: "system.log_level".to_string(),
            before: Some(serde_json::json!("info")),
            after: Some(serde_json::json!("debug")),
            result: AuditResult::Ok,
            reason: None,
            request_id: format!("rid-{ts_ms}"),
        }
    }

    /// 用真实写侧把条目落进 `dir`（顺带写下哈希链文件——读侧必须能忽略它）。
    fn write_entries(dir: &Path, entries: &[ConsoleAuditEntry]) {
        let sink = FileAuditSink::open(dir).unwrap();
        for e in entries {
            sink.record_outcome(e).unwrap();
        }
    }

    fn svc(dir: &Path) -> ConsoleAuditService {
        ConsoleAuditService::new(dir)
    }

    /// 覆盖 `T0` 前后各一天的窗口（多数用例用它，避开缺省 24h 窗口的边界）。
    fn q_day() -> AuditQuery {
        AuditQuery {
            from_ms: Some(T0 - DAY),
            to_ms: Some(T0 + DAY),
            page: 1,
            ops: Vec::new(),
        }
    }

    /// ① 倒序 + 分页 + `has_more` 的**边界**（最后一页必须 `has_more=false`）。
    #[tokio::test]
    async fn pages_are_newest_first_and_has_more_is_false_on_the_last_page() {
        let t = TempDir::new("audit-page");
        // 45 条、跨 3 天（每天 15 条）；**写入顺序与时间顺序刻意无关**（先写最旧的）
        let mut all: Vec<ConsoleAuditEntry> = Vec::new();
        for day in (0..3u64).rev() {
            for k in 0..15u64 {
                all.push(ent(T0 - day * DAY + k, ConsoleOp::ConfigApply));
            }
        }
        write_entries(t.path(), &all);
        let s = svc(t.path());
        let q = |page: u32| AuditQuery {
            from_ms: Some(T0 - 3 * DAY),
            to_ms: Some(T0 + DAY),
            page,
            ops: Vec::new(),
        };

        let p1 = s.page(&q(1), T0).await;
        assert!(p1.available && p1.page == 1 && p1.page_size == 20);
        assert_eq!(p1.entries.len(), 20, "满页");
        assert!(p1.has_more, "45 条 > 20 ⇒ 还有下一页");
        let ts: Vec<u64> = p1.entries.iter().map(|e| e.ts_ms).collect();
        let mut desc = ts.clone();
        desc.sort_by(|a, b| b.cmp(a));
        assert_eq!(ts, desc, "必须时间倒序");
        assert_eq!(ts[0], T0 + 14, "最新一条在首位");

        let p2 = s.page(&q(2), T0).await;
        assert_eq!(p2.entries.len(), 20);
        assert!(p2.has_more, "第 2 页仍有下一页");

        let p3 = s.page(&q(3), T0).await;
        assert_eq!(p3.entries.len(), 5, "45 = 20 + 20 + 5");
        assert!(
            !p3.has_more,
            "最后一页必须 has_more=false（否则屏上会一直转「加载中」）"
        );
        assert_eq!(p3.entries[4].ts_ms, T0 - 2 * DAY, "末条是最旧的");
        // 三页拼起来 **不重不漏**（分页若在"过滤之前"做，这里就会少条或重条）
        let mut seen: Vec<u64> = p1
            .entries
            .iter()
            .chain(&p2.entries)
            .chain(&p3.entries)
            .map(|e| e.ts_ms)
            .collect();
        seen.sort_unstable();
        assert_eq!(seen.len(), 45);
        assert!(seen.windows(2).all(|w| w[1] > w[0]), "45 条不重不漏");
    }

    /// ② `ops` 多值筛选：**重复键**（不是逗号）；多值 = 命中任一；空集 = 不筛。
    #[tokio::test]
    async fn ops_repeated_keys_select_any_listed_op_and_empty_means_no_filter() {
        // 解析口径（与 `log_service::parse_query` 同款）
        let q = parse_query(&[
            ("ops".to_string(), "config_apply".to_string()),
            ("ops".to_string(), "interlock_release".to_string()),
        ])
        .unwrap();
        assert_eq!(
            q.ops,
            vec![ConsoleOp::ConfigApply, ConsoleOp::InterlockRelease]
        );
        assert!(
            parse_query(&[(
                "ops".to_string(),
                "config_apply,interlock_release".to_string()
            )])
            .is_err(),
            "逗号拼接必须被拒（§3.4 补注：多值一律重复键）"
        );

        // 端到端：4 类操作各 1 条，只筛其中 2 类
        let t = TempDir::new("audit-ops");
        let all: Vec<ConsoleAuditEntry> = ConsoleOp::ALL
            .iter()
            .enumerate()
            .map(|(i, op)| ent(T0 + i as u64, *op))
            .collect();
        write_entries(t.path(), &all);
        let s = svc(t.path());
        let with = |ops: Vec<ConsoleOp>| AuditQuery { ops, ..q_day() };

        let got = s
            .page(
                &with(vec![ConsoleOp::InterlockRelease, ConsoleOp::InterlockAckM1]),
                T0,
            )
            .await;
        assert!(got.available);
        assert_eq!(got.entries.len(), 2, "只出被选中的两类");
        assert_eq!(got.entries[0].op, ConsoleOp::InterlockAckM1, "仍按时间倒序");
        assert_eq!(got.entries[1].op, ConsoleOp::InterlockRelease);

        assert_eq!(
            s.page(&with(Vec::new()), T0).await.entries.len(),
            4,
            "空集 = 不筛（全部 4 类）"
        );
    }

    /// ③ `from`/`to` 过滤**含两端**，窗口外的条目一律不出现。
    #[tokio::test]
    async fn from_to_window_is_inclusive_at_both_ends() {
        let t = TempDir::new("audit-window");
        let all: Vec<ConsoleAuditEntry> = [-2i64, -1, 0, 1, 2]
            .iter()
            .map(|d| ent((T0 as i64 + d * DAY as i64) as u64, ConsoleOp::ConfigApply))
            .collect();
        write_entries(t.path(), &all);
        let p = svc(t.path()).page(&q_day(), T0).await;
        assert!(p.available);
        let ts: Vec<u64> = p.entries.iter().map(|e| e.ts_ms).collect();
        assert_eq!(
            ts,
            vec![T0 + DAY, T0, T0 - DAY],
            "窗口 = [T0−1d, T0+1d]，**含两端**；±2 天必须被排除"
        );
    }

    /// ④ **「确实没有」与「不可用」必须结构性可分**（EDGE-17 的验收面）。
    ///
    /// 两个页对象在 `entries.len()` 与 `has_more` 上**完全一样** ⇒ 只看行数的代码必然混为一谈；
    /// 只有 `available` 能区分（渲染端 `list_view_of` 的 `!available` 分支优先正是为此）。
    #[tokio::test]
    async fn empty_is_available_while_unreadable_is_unavailable_and_they_differ() {
        // (a) 目录存在、可读，**确实没有记录** ⇒ available=true ∧ entries=[]
        let t = TempDir::new("audit-empty");
        let empty = svc(t.path()).page(&AuditQuery::default(), T0).await;
        assert!(
            empty.available,
            "空目录读得出来 ⇒ 是「确实没有」，不是「不可用」"
        );
        assert!(empty.entries.is_empty() && !empty.has_more && empty.newest_ts_ms.is_none());
        assert_eq!(empty.page, 1);
        assert_eq!(empty.page_size, 20);

        // (b) 目录**读不出来**（父路径是普通文件 ⇒ `read_dir` 必失败；真实失败，不用 mock）
        let bad = TempDir::new("audit-unavail");
        let blocker = bad.write("blocker", "i am a file, not a dir");
        let un = svc(&blocker.join("audit"))
            .page(&AuditQuery::default(), T0)
            .await;
        assert!(!un.available, "目录读不出来必须落「不可用」");
        assert!(un.entries.is_empty(), "不可用页不主张任何条目");

        // **关键**：两者行数、has_more 全同 ⇒ 只有 available 分得开
        assert_eq!(empty.entries.len(), un.entries.len());
        assert_eq!(empty.has_more, un.has_more);
        assert_ne!(empty, un, "「无记录」与「不可用」必须是两个不同的页对象");
        assert!(empty.available && !un.available);

        // (c) 目录**根本不存在**（写侧没跑起来）⇒ 同样落「不可用」，**不得**冒充空态
        let missing = svc(&bad.join("never-created"));
        let un2 = missing.page(&AuditQuery::default(), T0).await;
        assert!(!un2.available && un2.entries.is_empty());
    }

    /// ⑤ `newest_ts_ms`：**窗口内**最新一条，且**不受 `ops` 筛选影响**；窗口内无记录 ⇒ `None`。
    #[tokio::test]
    async fn newest_ts_is_window_scoped_and_ignores_the_ops_filter() {
        let t = TempDir::new("audit-newest");
        write_entries(
            t.path(),
            &[
                ent(T0, ConsoleOp::ConfigApply),
                ent(T0 + 100, ConsoleOp::InterlockRelease),
            ],
        );
        let s = svc(t.path());

        // 选**更旧**的那类：它的条目被筛掉，但"审计链路还在写"的信号必须照常给出（F19.8）
        let p = s
            .page(
                &AuditQuery {
                    ops: vec![ConsoleOp::ConfigApply],
                    ..q_day()
                },
                T0,
            )
            .await;
        assert_eq!(p.entries.len(), 1);
        assert_eq!(
            p.newest_ts_ms,
            Some(T0 + 100),
            "newest 是链路活性信号 ⇒ 不受 ops 筛选影响"
        );

        // 窗口内一条都没有 ⇒ `None`（**不是**"全库最新"，也不是编造的值）
        let p_old = s
            .page(
                &AuditQuery {
                    from_ms: Some(T0 - 2 * DAY),
                    to_ms: Some(T0 - DAY),
                    page: 1,
                    ops: Vec::new(),
                },
                T0,
            )
            .await;
        assert!(p_old.available);
        assert!(p_old.entries.is_empty());
        assert_eq!(p_old.newest_ts_ms, None, "窗口内无记录 ⇒ None");
    }

    /// ⑥ **只打开窗口内日期的文件**（"按日期定位文件，不扫全库"）。
    ///
    /// 用两个"一旦被打开就必然出事"的探针证明它**没被打开**：
    /// ① 窗口外放一个**与审计文件逐字同名的目录**（若被扫 ⇒ 本页立刻不可用）；
    /// ② 窗口外放一个**超过行预算**的大文件（若被读 ⇒ 预算耗尽 ⇒ 不可用）。
    #[tokio::test]
    async fn only_the_requested_days_files_are_opened() {
        let t = TempDir::new("audit-locate");
        write_entries(t.path(), &[ent(T0, ConsoleOp::ConfigApply)]);
        // 探针①：窗口外的日期（2020-01-01）放同名**目录**
        std::fs::create_dir_all(t.join(&console_audit_file_name("2020-01-01"))).unwrap();
        // 探针②：窗口外的日期（2020-01-02）放超预算的大文件
        let big: String = (0..(AUDIT_SCAN_MAX_LINES + 10))
            .map(|i| format!("{i}\n"))
            .collect();
        std::fs::write(t.join(&console_audit_file_name("2020-01-02")), big).unwrap();

        let p = svc(t.path()).page(&q_day(), T0).await;
        assert!(
            p.available,
            "窗口外的目录 / 大文件**不得**被打开（被打开就会把本页打成不可用）"
        );
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].ts_ms, T0);

        // ── 反证（两条探针**各自**都要真有判别力）──
        // 把窗口收到只覆盖探针①的当天 ⇒ 同名目录被打开 ⇒ 必须不可用
        let jan1 = 1_577_836_800_000u64; // 2020-01-01T00:00:00Z
        let narrow = |from: u64, to: u64| AuditQuery {
            from_ms: Some(from),
            to_ms: Some(to),
            page: 1,
            ops: Vec::new(),
        };
        let probe1 = svc(t.path()).page(&narrow(jan1, jan1), T0).await;
        assert!(
            !probe1.available,
            "反证①：把同名目录纳入窗口后本页**确实**会变不可用（否则上面那条断言没有判别力）"
        );
        // 只覆盖探针②的当天 ⇒ 大文件被读 ⇒ 行预算耗尽 ⇒ 不可用
        let probe2 = svc(t.path())
            .page(&narrow(jan1 + DAY, jan1 + DAY), T0)
            .await;
        assert!(
            !probe2.available,
            "反证②：把大文件纳入窗口后本页**确实**会变不可用"
        );
    }

    /// ⑦ 无界输入一律**响亮**（绝不静默丢条）：超长行 / 畸形行 / 行数超预算 / **命中载荷超预算**
    /// （最后一条是本轮新增的第三道闸，见 [`AUDIT_MATCHED_MAX_BYTES`]）。
    ///
    /// ⚠️ 措辞口径：本用例的 ③ 是"**行数**超预算"（每行都很小），④ 是"**命中载荷**超预算"
    /// （行数远不到闸，但每行很大）——**不是**"超长文件"（那个词易与 ① 的"超长行"混淆，
    /// 而两者的处置与语义完全不同）。
    #[tokio::test]
    async fn oversized_malformed_and_over_budget_inputs_are_unavailable_never_silently_dropped() {
        let day_file = console_audit_file_name("2025-09-09");

        // ① 单行超上限（有界读**整行丢弃、不物化**）⇒ 不可用
        let t1 = TempDir::new("audit-huge-line");
        std::fs::write(
            t1.join(&day_file),
            format!("{}\n", "x".repeat(AUDIT_LINE_MAX_BYTES + 1)),
        )
        .unwrap();
        assert!(
            !svc(t1.path()).page(&q_day(), T0).await.available,
            "超长行 ⇒ 不可用（整行已丢 ⇒ 不可能保证不漏记录）"
        );

        // ② 畸形行（一条好的 + 一条被截断）——**不得**"跳过坏行、只回好的那条"
        let t2 = TempDir::new("audit-malformed");
        let good = serde_json::to_string(&ent(T0, ConsoleOp::ConfigApply)).unwrap();
        std::fs::write(t2.join(&day_file), format!("{good}\n{{\"id\":\"trunc")).unwrap();
        let p2 = svc(t2.path()).page(&q_day(), T0).await;
        assert!(
            !p2.available,
            "解析失败 ⇒ 整页不可用（跳过坏行 = 把**确实存在**的记录说成不存在）"
        );
        assert!(p2.entries.is_empty(), "不可用页不主张任何条目");

        // ③ 行数超预算（每行都**合法** ⇒ 只看解析不会触闸，只有预算能挡）
        let t3 = TempDir::new("audit-budget");
        let line = serde_json::to_string(&ent(T0, ConsoleOp::ConfigApply)).unwrap();
        let mut body = String::new();
        for _ in 0..(AUDIT_SCAN_MAX_LINES + 10) {
            body.push_str(&line);
            body.push('\n');
        }
        std::fs::write(t3.join(&day_file), body).unwrap();
        assert!(
            !svc(t3.path()).page(&q_day(), T0).await.available,
            "行数超预算 ⇒ 可见拒绝（**不**静默截断成「只有前 5 万行」）"
        );

        // ④ **命中载荷**超预算（代码质量评审「重要 ①」新增的第三道闸）：
        //    每一行都**合法**、都命中、行数（4 500）也远不到行闸（5 万）⇒ **只有载荷闸能挡**。
        //    单条 8 KiB ⇒ 4 500 × 8 KiB ≈ 36 MB > `AUDIT_MATCHED_MAX_BYTES`（32 MiB）。
        //    这正是"3.05 GiB **输入字节**"路线的入口：闸前 `matched` 只被行闸间接约束，而单行上限 64 KiB
        //    ⇒ 算术最坏 50 000 × 64 KiB。**破坏性验证**：把 `ScanState::push_entry` 里的载荷闸
        //    摘掉 ⇒ 本条变红（页面照常 `available`）。
        let t4 = TempDir::new("audit-matched-bytes");
        let fat_line = {
            let mut e = ent(T0, ConsoleOp::ConfigApply);
            e.target = "x".repeat(8 * 1024); // 单条 ≈8 KiB（仍远小于单行上限 64 KiB）
            serde_json::to_string(&e).unwrap()
        };
        {
            let mut body = String::with_capacity(4_500 * (fat_line.len() + 1));
            for _ in 0..4_500 {
                body.push_str(&fat_line);
                body.push('\n');
            }
            std::fs::write(t4.join(&day_file), body).unwrap();
        }
        assert!(
            !svc(t4.path()).page(&q_day(), T0).await.available,
            "命中载荷超预算 ⇒ 可见拒绝（4 500 个**合法大条目** ≈36 MB：行闸/解析闸都挡不住它）"
        );

        // ⑤ **正对照**（同一道闸的"不该触发"侧）：2 000 条**正常尺寸**的命中条目（≈430 KB，
        //    远低于 32 MiB）必须照常服务 —— 否则 ④ 的断言可能只是"这条路径恒不可用"。
        //    （更极端的正对照是 ⑱(a)：恰好 5 万条正常条目 ≈10.8 MB（**输入字节**口径），也必须
        //     照常服务 ⇒ 闸值必须大于它，见 `AUDIT_MATCHED_MAX_BYTES` 的取值论证。）
        let t5 = TempDir::new("audit-matched-bytes-ok");
        let thin_line = serde_json::to_string(&ent(T0, ConsoleOp::ConfigApply)).unwrap();
        let mut thin = String::with_capacity(2_000 * (thin_line.len() + 1));
        for _ in 0..2_000 {
            thin.push_str(&thin_line);
            thin.push('\n');
        }
        std::fs::write(t5.join(&day_file), thin).unwrap();
        let ok = svc(t5.path()).page(&q_day(), T0).await;
        assert!(
            ok.available && ok.entries.len() == AUDIT_PAGE_SIZE,
            "正常尺寸的命中载荷**不得**触发本闸（拍大了会误伤正常查询，拍小了挡不住 3.05 GiB 输入字节）"
        );
    }

    /// ⑧ **读侧读的就是写侧写的那份**：用真实 [`FileAuditSink`] 落一条带
    /// `before=Some / after=None / reason=Some` 的**失败**条目，读回来必须**逐字段**相等。
    #[tokio::test]
    async fn a_record_written_by_the_sink_reads_back_field_by_field() {
        let t = TempDir::new("audit-fidelity");
        let e = ConsoleAuditEntry {
            id: "audit-id-1".to_string(),
            ts_ms: T0 + 5,
            operator: CONSOLE_OPERATOR.to_string(),
            op: ConsoleOp::InterlockRelease,
            target: "interlock.release".to_string(),
            before: Some(serde_json::json!(true)),
            after: None,
            result: AuditResult::Failed,
            reason: Some("触发源未复位 · estop".to_string()),
            request_id: "rid-fidelity".to_string(),
        };
        write_entries(t.path(), std::slice::from_ref(&e));
        let p = svc(t.path()).page(&q_day(), T0).await;
        assert!(p.available);
        assert_eq!(
            p.entries,
            vec![e],
            "读侧必须逐字段还原（含 Option 的两侧；少一个字段 = 屏上少一列事实）"
        );
        assert_eq!(p.newest_ts_ms, Some(T0 + 5));
    }

    /// ⑨ 目录里**别的 jsonl**（既有哈希链 `audit_*.jsonl`、形状不符的文件）一律**忽略**：
    /// 它们不是 `ConsoleAuditEntry`，去解析只会把整页打成"不可用"。
    #[tokio::test]
    async fn unrelated_files_in_the_same_dir_are_ignored() {
        let t = TempDir::new("audit-others");
        write_entries(t.path(), &[ent(T0, ConsoleOp::ConfigApply)]); // 顺带写下哈希链文件
        assert!(
            std::fs::read_dir(t.path())
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("audit_")),
            "前提：哈希链文件确实与审计文件**同目录**"
        );
        std::fs::write(t.join("console-audit-zzz.jsonl"), "not json at all").unwrap();
        std::fs::write(t.join("console-audit-2020-03-04.extra.jsonl"), "junk").unwrap();

        let p = svc(t.path())
            .page(
                &AuditQuery {
                    from_ms: Some(0),
                    to_ms: Some(T0 + DAY),
                    page: 1,
                    ops: Vec::new(),
                },
                T0,
            )
            .await;
        assert!(p.available, "同目录的无关文件不得让审计页不可用");
        assert_eq!(p.entries.len(), 1);
    }

    /// ⑩ `parse_query` 的逐条校验（设计 §3.4 的请求列 + 本单元拍板的口径表）。
    #[test]
    fn query_validation_rejects_what_it_cannot_honour() {
        let p = |k: &str, v: &str| parse_query(&[(k.to_string(), v.to_string())]);
        assert!(
            p("nope", "1").is_err(),
            "未知键必须拒（静默忽略 = 给未筛的数据）"
        );
        assert!(p("page", "0").is_err(), "page 从 1 开始");
        assert!(p("page", "-1").is_err());
        assert!(p("page", "1.5").is_err());
        assert!(p("page_size", "50").is_err(), "page_size 恒 = 20");
        assert!(p("page_size", "0").is_err());
        assert!(p("ops", "").is_err(), "空值必须拒（空集应不发该键）");
        assert!(
            p("ops", "mode_switch").is_err(),
            "非本期操作集（模式切换为暂停项）"
        );
        assert!(p("from", "5").is_err(), "半截窗口必须拒");
        assert!(p("to", "5").is_err());
        assert!(p("from", "1.5").is_err(), "非整毫秒");
        assert!(
            parse_query(&[
                ("from".to_string(), "9".to_string()),
                ("to".to_string(), "5".to_string())
            ])
            .is_err(),
            "倒置窗口必须拒（空窗口是「静默无结果」的伪装）"
        );

        // 合法形态
        let d = parse_query(&[]).unwrap();
        assert_eq!(
            (d.from_ms, d.to_ms, d.page, d.ops.len()),
            (None, None, 1, 0),
            "齐缺 ⇒ 缺省（缺省窗口由 `window()` 给；页大小不入 `AuditQuery`，见该结构的字段说明）"
        );
        let full = parse_query(&[
            ("from".to_string(), "1".to_string()),
            ("to".to_string(), "2".to_string()),
            ("ops".to_string(), "interlock_ack_m1".to_string()),
            ("page".to_string(), "3".to_string()),
            ("page_size".to_string(), "20".to_string()),
        ])
        .unwrap();
        assert_eq!(full.ops, vec![ConsoleOp::InterlockAckM1]);
        assert_eq!(full.page, 3);
        // `page_size=20` 是**唯一**合法值 ⇒ 解析成功即证明它被接受（值本身不入 `AuditQuery`）；
        // 非法值的拒绝由上一条 `p("page_size", "50")` 钉住。
        assert!(parse_query(&[("page_size".to_string(), "20".to_string())]).is_ok());
        assert!(
            parse_query(&[
                ("from".to_string(), "7".to_string()),
                ("to".to_string(), "7".to_string())
            ])
            .is_ok(),
            "from == to 合法（1 ms 窗口，结果自然是空）"
        );
    }

    /// ⑪ `/audit/ops` 的选项 = **契约常量**（4 类），且**不读存储** ⇒ 审计源不可用时照旧可得。
    ///
    /// ⚠️ **本轮订正（代码质量评审点名）**：原来这条断言写的是
    /// `opts == ConsoleOp::ALL.iter().copied().map(OpOption::from).collect()`
    /// **（旧写法，仅存照 —— 下文引用它只为说明它为何没有判别力；生产/测试路径里已无此表达式，
    /// `grep OpOption::from` 命中本行属于引文，不是活代码）** —— **右边与
    /// `op_options()` 的实现逐字相同**（自引用）⇒ 把 `op_options()` 改成逆序、改成重复、
    /// 改成过滤掉一类，这行断言**全都照样绿**（它只证明"函数等于它自己"）。
    /// 现在改成**字面量期望**（与契约测试 `display-proto/src/audit.rs` 的
    /// `console_op_vocabulary_and_labels` 同风格）：`(op, label)` 四对逐字写死 ⇒ 顺序、条数、
    /// 文案、线名四处任一处漂移都会变红。
    #[test]
    fn op_options_are_the_contract_vocabulary_and_need_no_store() {
        let svc = ConsoleAuditService::new("no-such-dir-and-that-is-fine");
        let opts = svc.op_options();
        let got: Vec<(ConsoleOp, &str)> = opts.iter().map(|o| (o.op, o.label.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (ConsoleOp::ConfigApply, "配置保存"),
                (ConsoleOp::ConfigResetDefault, "恢复默认值"),
                (ConsoleOp::InterlockRelease, "联锁释放"),
                (ConsoleOp::InterlockAckM1, "M1 授权"),
            ],
            "选项集合/顺序/文案必须与契约逐字一致（渲染端 canonical_ops 取同一集合）"
        );
        // 顺带把**线名**也钉一次（`OpsOption` 上屏的是 label，但请求串里发的是 op 的线名）。
        let wire: Vec<String> = opts
            .iter()
            .map(|o| serde_json::to_string(&o.op).unwrap())
            .collect();
        assert_eq!(
            wire,
            vec![
                "\"config_apply\"",
                "\"config_reset_default\"",
                "\"interlock_release\"",
                "\"interlock_ack_m1\"",
            ]
        );
    }

    /// ⑫ 缺省窗口 = **24 小时**（不是 1 小时）：`H1` 与 `H24` 的查询串逐字节相同
    /// （`control_route::audit_query_string` 只在 `Custom` 才发 `from`/`to`）⇒ 取**较宽**者，
    /// 任何相对档位下都**不会**静默少给记录；同时保持**有界**（不是"全部历史"）。
    #[tokio::test]
    async fn missing_window_defaults_to_24h_bounded() {
        let t = TempDir::new("audit-default-1");
        write_entries(t.path(), &[ent(T0 - 5 * 3_600_000, ConsoleOp::ConfigApply)]); // 5 小时前
        let p = svc(t.path()).page(&AuditQuery::default(), T0).await;
        assert!(p.available);
        assert_eq!(
            p.entries.len(),
            1,
            "5 小时前的记录必须在缺省窗口内（若取 1 小时就是静默少给）"
        );

        let t2 = TempDir::new("audit-default-2");
        write_entries(
            t2.path(),
            &[ent(T0 - 25 * 3_600_000, ConsoleOp::ConfigApply)],
        ); // 25 小时前
        let p2 = svc(t2.path()).page(&AuditQuery::default(), T0).await;
        assert!(
            p2.available,
            "窗口外的记录**不是**不可用：这是有界窗口下的「确实没有」"
        );
        assert!(p2.entries.is_empty() && p2.newest_ts_ms.is_none());
    }

    /// 一条**恰好 `want` 字节**的合法条目 JSON（`}` 前补空格 —— JSON 允许 token 之间的空白）。
    fn entry_json_of_len(want: usize) -> String {
        let base = serde_json::to_string(&ent(T0, ConsoleOp::ConfigApply)).unwrap();
        assert!(
            base.len() < want,
            "基准 JSON 比目标长度还长，用例前提不成立"
        );
        let pad = want - base.len();
        format!("{}{}{}", &base[..base.len() - 1], " ".repeat(pad), "}")
    }

    /// ⑭ **行长上限的边界**（恰好 `line_max` 通过、`line_max + 1` 整行丢弃）。
    ///
    /// ⚠️ 本条是**破坏性验证抓出来的**：把 `crate::bounded_io` 的边界从 `>=` 改成 `>`
    /// （单行物化上界由 `line_max` 变成 `line_max + 1`）时，**H 的 45 条用例与本文件其余用例
    /// 全都照样绿** ⇒ 这个边界此前**没有任何用例钉住**（搬迁的回归网在这里有洞）。
    /// 这条网同时守住"搬迁未改行为"里的那一处边界语义。
    #[tokio::test]
    async fn the_line_bound_is_exactly_line_max_bytes() {
        let day = console_audit_file_name("2025-09-09");

        // (a) 恰好 `line_max` ⇒ 仍是一整行（读进来后正常解析上岸）
        let t1 = TempDir::new("audit-bound-ok");
        std::fs::write(
            t1.join(&day),
            format!("{}\n", entry_json_of_len(AUDIT_LINE_MAX_BYTES)),
        )
        .unwrap();
        let p1 = svc(t1.path()).page(&q_day(), T0).await;
        assert!(
            p1.available && p1.entries.len() == 1,
            "恰好 {AUDIT_LINE_MAX_BYTES} 字节的行必须照常读入（边界是「>」而非「≥」之外的口径）"
        );

        // (b) `line_max + 1` ⇒ 整行丢弃 ⇒ 整页不可用（有界读的 Overlong 分支）
        let t2 = TempDir::new("audit-bound-over");
        std::fs::write(
            t2.join(&day),
            format!("{}\n", entry_json_of_len(AUDIT_LINE_MAX_BYTES + 1)),
        )
        .unwrap();
        assert!(
            !svc(t2.path()).page(&q_day(), T0).await.available,
            "{} 字节（= 上限 + 1）必须整行丢弃 ⇒ 不可用",
            AUDIT_LINE_MAX_BYTES + 1
        );
    }

    /// ⑮ **只读铁律（PL-02 / F19.5）**：查询路径**不建目录、不写文件、不改内容**。
    ///
    /// 对照写侧 `FileAuditSink::open` 的 `create_dir_all`：读侧若也建目录，就会把"审计**根本
    /// 没在跑**"伪装成"审计跑起来了"（目录在、永远为空 ⇒ 屏上永远显示空态而不是不可用）。
    /// 设备侧另有 grep 可查（本文件读侧只出现 `read_dir` / `File::open`，**无** `OpenOptions`）。
    #[tokio::test]
    async fn the_query_path_never_creates_or_modifies_anything_on_disk() {
        let t = TempDir::new("audit-ro");
        let dir = t.join("audit-that-must-not-be-created");
        let before = std::fs::read_dir(t.path()).unwrap().count();
        let p = svc(&dir).page(&q_day(), T0).await;
        assert!(
            !p.available,
            "目录不存在 ⇒ 不可用（**不是**建一个空目录后回空态）"
        );
        assert!(!dir.exists(), "查询路径**不得**创建目录（只读铁律）");
        assert_eq!(
            std::fs::read_dir(t.path()).unwrap().count(),
            before,
            "查询路径不得在磁盘上留下任何东西"
        );

        // 也不得改动既有内容：查询一次后审计文件必须**逐字节不变**
        let t2 = TempDir::new("audit-ro-2");
        write_entries(t2.path(), &[ent(T0, ConsoleOp::ConfigApply)]);
        let f = t2.join(&console_audit_file_name("2025-09-09"));
        let bytes = std::fs::read(&f).unwrap();
        let ok = svc(t2.path()).page(&q_day(), T0).await;
        assert!(ok.available && ok.entries.len() == 1);
        assert_eq!(
            std::fs::read(&f).unwrap(),
            bytes,
            "查询后审计文件必须逐字节不变（append-only / 不可删改）"
        );
    }

    /// ⑬ 窗口**按天取文件、按毫秒筛条目**：同一天里窗口外的条目不得出现，
    /// 而"取这个文件"这件事本身是必要的（否则窗口内的条目也会丢）。
    #[tokio::test]
    async fn a_day_file_is_read_but_only_the_entries_inside_the_window_are_returned() {
        let t = TempDir::new("audit-same-day");
        write_entries(
            t.path(),
            &[
                ent(T0 - 3_600_000, ConsoleOp::ConfigApply), // 窗口外（更早）
                ent(T0, ConsoleOp::ConfigApply),             // 窗口内
                ent(T0 + 3_600_000, ConsoleOp::ConfigApply), // 窗口外（更晚）
            ],
        );
        let p = svc(t.path())
            .page(
                &AuditQuery {
                    from_ms: Some(T0),
                    to_ms: Some(T0),
                    page: 1,
                    ops: Vec::new(),
                },
                T0,
            )
            .await;
        assert!(p.available);
        let ts: Vec<u64> = p.entries.iter().map(|e| e.ts_ms).collect();
        assert_eq!(ts, vec![T0], "同日文件的其余条目按毫秒被精确排除");
    }

    /// **挂死护栏**（与 `log_service.rs` 测试模块**同款、同阈值**）：凡涉及大文件 / 大行数 /
    /// 并发写读的用例一律套本护栏 —— 挂死（死循环 / 不收敛 / 读写互锁）必须以 `panic!` 的形式
    /// **变红**，而不是让 `cargo test` **卡住**（本项目已多次遇到"失败表现为挂死"）。
    async fn with_watchdog<F, T>(what: &str, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
            Ok(v) => v,
            Err(_) => panic!("{what}：超过 30 s 仍未返回（疑似不收敛 / 并发读写死锁）"),
        }
    }

    /// ⑯ **F19.4「查询响应 ≤ 2 s」的回归网 —— 断言的是「有界工作量」，不是墙钟**。
    ///
    /// # 为什么不写 `assert!(elapsed < 2s)`
    ///
    /// 开发机 / CI 容器上墙钟受负载、杀毒软件、文件系统冷热缓存影响 ⇒ 计时断言是**天然的
    /// flaky**（本项目已多次为"偶发红"付出代价）；而且 F19.4 的实质**不是"快"**，是
    /// **代价有上界**：查询只读窗口覆盖到的那几个日文件，与目录里躺了多少历史**无关**
    /// （设计 §4.5「按日期定位文件，不扫全库」）。于是把它写成**确定性的**断言 ——
    /// 造一份 **8 万行**（= 40 天 × 2000 条，**远超** `AUDIT_SCAN_MAX_LINES` = 5 万）的语料，
    /// 只要实现**多读了窗口外的文件**，行预算就会触发 ⇒ 本页 `available=false` ⇒ 用例**变红**。
    ///
    /// # 实测墙钟（本机 x86_64、**debug** 构建、`AUDIT_*` 常量与生产同值）
    ///
    /// | 场景 | 规格评审实测 | 本轮复测（3 次） |
    /// |------|--------------|------------------|
    /// | 2000 条（单日文件、页 1） | **≈ 18 ms** | **24.3 / 25.7 / 26.8 ms** |
    /// | 单文件吃满 5 万行行预算（≈11 MB） | **≈ 482 ms** | **535 / 575 / 609 ms** |
    ///
    /// 即**已知最坏**路径也在 F19.4 的 ≤ 2 s 内（余量 ≥ 3×）。两组数字**只登记、不入断言**
    /// （理由同上），由本用例的"有界工作量"结构代替。
    #[tokio::test]
    async fn the_cost_of_one_query_is_bounded_by_the_window_not_by_the_corpus() {
        with_watchdog("8 万行语料下的有界查询", async {
            let t = TempDir::new("audit-bounded-work");
            // 语料：40 个日文件 × 2000 条 = 80,000 行（行预算 50,000 ⇒ 整份语料**读不完**）
            let line = serde_json::to_string(&ent(T0, ConsoleOp::ConfigApply)).unwrap();
            let mut body = String::with_capacity(2000 * (line.len() + 1));
            for _ in 0..2000 {
                body.push_str(&line);
                body.push('\n');
            }
            for d in 0..40u64 {
                let day = day_of_ms(T0 - d * DAY);
                std::fs::write(t.join(&console_audit_file_name(&day)), &body).unwrap();
            }
            let s = svc(t.path());
            // **筛不中任何一条**的 `ops` 是刻意的：命中即会触发"凑够本页就停"的提前收敛
            // （`matched.len() >= need`），那样语料再大也读不了几个文件 —— 也就测不出
            // "代价是否会被窗口外的东西撑大"。滤空正是设计 §4.5 承认的最坏路径。
            let no_match = |from: u64, to: u64| AuditQuery {
                from_ms: Some(from),
                to_ms: Some(to),
                page: 1,
                ops: vec![ConsoleOp::InterlockAckM1],
            };

            // (a) 窗口**只覆盖一天** ⇒ 只该打开 1 个文件（2000 行）
            let p = s.page(&no_match(T0, T0), T0).await;
            assert!(
                p.available,
                "窗口内只有 2000 行 ⇒ 不该撞 5 万行预算；`available=false` 说明查询读了**窗口外**的文件"
            );
            assert!(
                p.entries.is_empty() && p.newest_ts_ms == Some(T0),
                "`newest` 有值 ⇒ 那一个日文件**确实被读进来了**（代价 = 1 个文件，不是 0 个）"
            );

            // (b) 缺省窗口（24 h）最多覆盖 2 个日文件（4000 行）——同样必须有界。
            //     这一条同时守住 `AUDIT_DEFAULT_WINDOW_MS` **不得**被拿去当"全库窗口"。
            let p2 = s
                .page(
                    &AuditQuery {
                        ops: vec![ConsoleOp::InterlockAckM1],
                        ..AuditQuery::default()
                    },
                    T0,
                )
                .await;
            assert!(p2.available, "缺省 24 h 窗口只覆盖 2 个日文件（4000 行）⇒ 不得撞预算");

            // ── 反证（本用例的判别力来源）──
            // 把窗口放到**覆盖全部 40 天**（80,000 行 > 50,000 预算）⇒ 必须**不可用**。
            // 它证明上面的 `available=true` 确实来自"只读了窗口内的文件"，而**不是**
            // "行预算闸根本不起作用"（否则 (a)(b) 就是恒真的假网）。
            assert!(
                !s.page(&no_match(T0 - 39 * DAY, T0 + DAY), T0).await.available,
                "反证：整份 8 万行语料若真被读进来，行预算**必须**触发 ⇒ 否则上面两条断言没有判别力"
            );
        })
        .await;
    }

    /// ⑰ **`has_more` 的"恰满页"边界（20 / 40 / 41 条）** —— 规格评审实测：把
    /// `scan` 里的 `matched.len() as u64 > start + page_size` 改成 `>=`，**全仓库 257 条
    /// 用例全绿（0 failed）** ⇒ 这个边界此前**没有任何用例钉住**。它与 ⑭ 的
    /// "行长边长边界"（`bounded_io` 的 `>=` vs `>`）是**同一类洞**：**比较符的另一侧无人守**。
    ///
    /// 后果（若写成 `>=`）：窗口内记录总数**恰为页大小整数倍**时**谎报"还有更多"** ⇒
    /// 屏上会去请求下一页、拿到一个空页（用户看到一次莫名的空翻页）。
    ///
    /// 三种总量各钉一次：**20**（恰 1 页）/ **40**（恰 2 页）/ **41**（2 页零 1 条）；
    /// 并顺带钉住**末页之后恒空**（= 登记 ③ 的 `page` 越界落空态）与**极大 / 极小页码不 panic**。
    ///
    /// ⚠️ **前提（本轮措辞订正，见登记 ③）**：本用例的"末页之后 = 可用 ∧ 空"是在**扫描在预算内
    /// 完成**的前提下成立的（这里语料 ≤ 41 条、1 个日文件 ⇒ 三道闸都摸不到）。**预算耗尽时越界页
    /// 同样落 `available=false`** —— 越界把 `need = (page−1)×20 + 21` 抬高（`page=2` 是 **41**）⇒
    /// `matched.len() >= need` 更难成立 ⇒ 文件边界的早停失效 ⇒ 一路扫到闸耗尽
    /// （评审实测 + 本轮复跑的**隔离构型**：【最新日文件 21 条命中 + 其后 365 个空日文件】⇒
    /// `page=1` = `available=true, entries=20`；`page=2` = `available=false, entries=0`）。
    /// 生产不可达（渲染端只在 `has_more=true` 时翻页）。
    #[tokio::test]
    async fn has_more_is_false_exactly_when_the_last_entry_is_on_this_page() {
        for total in [20u64, 40, 41] {
            let t = TempDir::new(&format!("audit-exact-{total}"));
            let all: Vec<ConsoleAuditEntry> = (0..total)
                .map(|i| ent(T0 + i, ConsoleOp::ConfigApply))
                .collect();
            write_entries(t.path(), &all);
            let s = svc(t.path());
            let q = |page: u32| AuditQuery {
                from_ms: Some(T0 - DAY),
                to_ms: Some(T0 + DAY),
                page,
                ops: Vec::new(),
            };

            // 页数 = ⌈total / 20⌉；**多查一页**，把"末页之后"也钉住（见下）。
            let pages = total.div_ceil(AUDIT_PAGE_SIZE as u64);
            for page_no in 1..=(pages + 1) {
                let p = s.page(&q(page_no as u32), T0).await;
                assert!(p.available, "{total} 条：第 {page_no} 页必须可用");
                let seen = (page_no - 1) * AUDIT_PAGE_SIZE as u64;
                assert_eq!(
                    p.entries.len(),
                    total.saturating_sub(seen).min(AUDIT_PAGE_SIZE as u64) as usize,
                    "{total} 条：第 {page_no} 页条数不对（20 条的第 2 页就是登记 ③ 的**落空态**）"
                );
                // 判别力所在：总量**恰为 20 的整数倍**时，末页必须 `has_more=false`
                // （写成 `>=` ⇒ 20 条的第 1 页 / 40 条的第 2 页都会谎报"还有更多"）。
                assert_eq!(
                    p.has_more,
                    total > page_no * AUDIT_PAGE_SIZE as u64,
                    "{total} 条：第 {page_no} 页的 has_more 判断错了\
                     （恰满页却谎报「还有更多」⇒ 屏上会去请求下一页、拿到空页）"
                );
                if !p.has_more {
                    // 末页之后恒空（且仍是"可用 + 空"，**不是**"不可用"）：登记 ③ 的越界形态。
                    let after = s.page(&q(page_no as u32 + 1), T0).await;
                    assert!(
                        after.available && after.entries.is_empty() && !after.has_more,
                        "{total} 条：末页之后的页必须是「可用 ∧ 空 ∧ has_more=false」"
                    );
                }
            }
        }

        // 极大页码（`u32::MAX`）不得 panic / 不得溢出（`start` 是 `u64` 上的饱和乘）：
        // 它落在"末页之后"的同一形态上。
        let t = TempDir::new("audit-page-max");
        write_entries(t.path(), &[ent(T0, ConsoleOp::ConfigApply)]);
        let p = svc(t.path())
            .page(
                &AuditQuery {
                    page: u32::MAX,
                    ..q_day()
                },
                T0,
            )
            .await;
        assert!(
            p.available && p.entries.is_empty() && !p.has_more,
            "page=u32::MAX 必须安全落空（不得 panic / 不得溢出成负数页）"
        );

        // `page = 0`：**请求路径上不可达**（`parse_query` 直接拒），但 `AuditQuery::page` 是 `pub`
        // ⇒ 单元 J / 测试可以手搓一个，不变式不能只写在注释里（评审「建议但必须做」③）。
        // 它曾是请求路径上**唯一**的非饱和减法（`q.page as u64 - 1`）：debug 下
        // `attempt to subtract with overflow` **panic**，release 下回绕成 `u64::MAX`（静默落空）。
        // 饱和后 `page = 0` **确定地**退化成第 1 页（不是"越界页"、也不是 panic）。
        // **破坏性验证**：把 `saturating_sub(1)` 改回裸减法 ⇒ 本条在 debug 下 panic 变红。
        let q0 = AuditQuery { page: 0, ..q_day() };
        let q1 = AuditQuery { page: 1, ..q_day() };
        let (p0, p1) = (
            svc(t.path()).page(&q0, T0).await,
            svc(t.path()).page(&q1, T0).await,
        );
        assert_eq!(
            (p0.available, p0.entries.len(), p0.has_more, p0.newest_ts_ms),
            (p1.available, p1.entries.len(), p1.has_more, p1.newest_ts_ms),
            "page=0 必须**饱和成第 1 页**（确定行为），而不是 panic / 回绕成 u64::MAX"
        );
        assert_eq!(p0.page, 0, "回显请求里的 page（不假装它被改写成了 1）");
    }

    /// ⑱ **`scan` 里其余几道"值边界"的成对用例**（`>` 的另一侧逐个钉住）。
    ///
    /// 逐条列出本单元**新增**了哪些可越界的阈值，以及各自有没有网：
    ///
    /// | 边界（代码处） | 边界值 | 网 |
    /// |----------------|--------|-----|
    /// | `matched.len() > start + page_size`（`has_more`） | 20 / 40 / 41 | **⑰（本轮新增）** |
    /// | `lines_read > AUDIT_SCAN_MAX_LINES` | 恰 50,000 | **本用例 (a)**（半网已有：⑦③ 是 +10） |
    /// | `files_read > AUDIT_SCAN_MAX_FILES` | 恰 365 | **本用例 (b)** |
    /// | `matched_bytes > AUDIT_MATCHED_MAX_BYTES` | 恰 33 554 432 B（32 MiB） | **⑳**（半网已有：⑦④ 是 ≈36 MB。此前"不该触发"的那一侧**无人守** —— 评审实测把 `>` 改成 `>=` **零红**；因语料重（两份 32 MiB）而**单列**，没并进本用例） |
    /// | `day_of_ms` 的时戳夹取 | `u64::MAX` | **本用例 (c)** |
    /// | 日名筛选 `day < from_day \|\| day > to_day` | 两端**含** | 已有：⑥（`from_day == to_day` 的那天被打开） |
    /// | 条目的 `ts_ms < from \|\| ts_ms > to` | 两端**含** | 已有：③（±1 天含、±2 天不含）、⑬（同日精确到毫秒） |
    /// | `console_audit_day` 的形状校验（10 位 / 第 4、7 位为 `-`） | 非 `NNNN-NN-NN` 即忽略 | 已有：⑨（`zzz` / `.extra.` 两个反例） |
    /// | `parse_query` 的逐字段校验 | 见口径表 | 已有：⑩ |
    ///
    /// 越界**不 panic**（而不是"报错"）是刻意的：读侧的"读不全"一律落 `available=false`，
    /// 而**边界值本身是合法输入**（恰 365 个文件 / 恰 5 万行 ⇒ 照常服务）。
    #[tokio::test]
    async fn every_threshold_in_scan_is_exact_at_its_own_boundary() {
        with_watchdog("阈值成对边界", async {
            let day = console_audit_file_name("2025-09-09");

            // (a) 行预算：**恰好** AUDIT_SCAN_MAX_LINES 行 ⇒ 必须照常服务
            //     （与 ⑦③ 的 "+10 ⇒ 不可用"成对；把闸写成 `>=` ⇒ 本条变红）
            let t1 = TempDir::new("audit-lines-exact");
            let line = serde_json::to_string(&ent(T0, ConsoleOp::ConfigApply)).unwrap();
            let mut body = String::with_capacity(AUDIT_SCAN_MAX_LINES * (line.len() + 1));
            for _ in 0..AUDIT_SCAN_MAX_LINES {
                body.push_str(&line);
                body.push('\n');
            }
            std::fs::write(t1.join(&day), &body).unwrap();
            let p1 = svc(t1.path()).page(&q_day(), T0).await;
            assert!(
                p1.available && p1.entries.len() == 20,
                "恰好 {AUDIT_SCAN_MAX_LINES} 行是**合法输入**（闸是 `>` 不是 `≥`）⇒ 必须照常服务"
            );

            // (b) 文件预算：**恰好** AUDIT_SCAN_MAX_FILES 个候选文件 ⇒ 照常服务；再多一个 ⇒ 不可用
            let t2 = TempDir::new("audit-files-exact");
            let mk = |n: usize| {
                for i in 0..n {
                    let d = day_of_ms(T0 - i as u64 * DAY);
                    // 空文件：0 行 ⇒ 一条都不命中 ⇒ 扫描**不会**提前收敛，一路走到文件数闸
                    std::fs::write(t2.join(&console_audit_file_name(&d)), "").unwrap();
                }
            };
            mk(AUDIT_SCAN_MAX_FILES);
            let wide = AuditQuery {
                from_ms: Some(T0 - 400 * DAY),
                to_ms: Some(T0 + DAY),
                page: 1,
                ops: Vec::new(),
            };
            let p2 = svc(t2.path()).page(&wide, T0).await;
            assert!(
                p2.available,
                "恰好 {AUDIT_SCAN_MAX_FILES} 个候选日文件是**合法输入**（闸是 `>` 不是 `≥`）"
            );
            mk(AUDIT_SCAN_MAX_FILES + 1); // 第 366 个（`i = 365` 的新日期）
            assert!(
                !svc(t2.path()).page(&wide, T0).await.available,
                "第 {} 个候选文件必须**可见拒绝**（绝不静默只扫前 365 个）",
                AUDIT_SCAN_MAX_FILES + 1
            );

            // (c) 时戳夹取：`u64::MAX` 的端点**不得**让 `from_timestamp_millis` 落空（否则
            //     `.expect` 直接 panic）、也不得把窗口推成负数（`u64 as i64` 变号 ⇒ 1969 年）。
            let t3 = TempDir::new("audit-ts-clamp");
            write_entries(t3.path(), &[ent(T0, ConsoleOp::ConfigApply)]);
            let mix = |from: u64, to: u64| AuditQuery {
                from_ms: Some(from),
                to_ms: Some(to),
                page: 1,
                ops: Vec::new(),
            };
            let all = svc(t3.path()).page(&mix(0, u64::MAX), T0).await;
            assert!(
                all.available && all.entries.len() == 1,
                "窗口 = [0, u64::MAX] 是**合法输入** ⇒ 必须照常服务（端点夹到 9999-12-31，不得 panic）"
            );
            let both_max = svc(t3.path()).page(&mix(u64::MAX, u64::MAX), T0).await;
            assert!(
                both_max.available && both_max.entries.is_empty(),
                "窗口落在 9999-12-31 ⇒ 是「确实没有」（可用 ∧ 空），不是「不可用」"
            );
        })
        .await;
    }

    /// ⑲ **并发写入时读到"写了一半的行"会不会把整页打成「不可用」**（规格评审点名：
    /// "读者确实可能读到只写了 JSON、换行尚未落盘的末行 —— **但这只是推断，我没有实测**"）。
    ///
    /// 写侧 [`FileAuditSink::append_line`] 用 `writeln!(f, \"{line}\")` ⇒ **两次 write 系统调用**
    /// （先 JSON、再 `'\n'`），其间读者看到的文件**末尾就是一条没有 `'\n'` 的完整 JSON**。
    /// 有界读的语义是"文件末行没有 `'\n'` 也照常消费"（对齐 `lines()`）⇒ 该行**是完整 JSON**
    /// ⇒ 应当正常解析上岸，页面**不得**翻成 `available=false`。
    ///
    /// 两段：
    /// * **(a) 确定性**：人工造出该态（完整 JSON、无结尾 `'\n'`）⇒ 断言可用**且该条上屏**；
    ///   这是**不依赖时序**的钉法（会把"末行无换行 ⇒ 判损坏"的改法直接照红）。
    /// * **(b) 真并发**：一个线程用**真实写侧**持续 append，另一个任务持续并发查询，逐次断言
    ///   `available`；并统计"末行无 `'\n'`"的**实测次数**（= 探针**发起**查询时所处的相位）。
    ///   写完之后再整本读回来，断言 **200 条一条不少**（撕裂行**不得吞记录、也不得产生重复**）。
    ///
    /// # 实测（本机 x86_64、debug；`-- --nocapture` 可见打印）
    ///
    /// 下表是**上一轮**的 6 次独立运行；**本轮复跑 1 次**：200 append / **308** 次并发查询 /
    /// **不可用 0** 次 / 窗口内发起 **27** 次 —— 与下表同阶（都落在"0 次不可用"这个结论上）。
    ///
    /// | 轮次 | append | 并发查询 | **不可用** | 在撕裂窗口内**发起**的查询（**不等于**该次查询读到撕裂态） |
    /// |------|--------|----------|-----------|--------------------------------------------------------|
    /// | 6 次独立运行 | 各 200 | 248–272 | **0 / 6 次全部为 0** | 各轮 21–39 次（**只列样本，不作断言**） |
    ///
    /// 单次耗时 **0.54–0.60 s**（非分钟级）。**结论：实测未见撕裂导致 `available=false`。**
    ///
    /// ⚠️ **上一版把这句话说满了（本轮订正）**：原文写"上表第 4 列**证明每轮都撞进去了**"。
    /// 探针能证明的只是"写侧两次 `write` 之间的那个窗口**真实存在**，且我们确实在它的相位里
    /// **发起**了查询"；它**不能**证明"该次查询**读到了**撕裂态"（探针读末字节与 `page()` 读文件
    /// 之间隔着 µs 级间隔，写侧随时可能把 `'\n'` 补上）⇒ 列名与结论按这个强度改写，
    /// 并**只给结论**（"实测未见"），不再给"每轮都撞进去"这种强断言；那个数字也只是**样本区间**，
    /// 不加断言（时序 flaky）。
    ///
    /// ⚠️ **第 4 列不做断言**（只打印）：它依赖"写侧在跑"与"探针采样率"的时序，在**快文件系统**
    /// （tmpfs / 写盘不 fsync 的容器）上写侧会跑得更快、采样窗口更少 ⇒ 断言它会变成一条
    /// **时序 flaky** 的网。真正**能红的**判别力由 (a) 的确定性用例提供（把末行无 `'\n'`
    /// 判成损坏 ⇒ (a) 立刻变红）；(b) 是"真并发下不翻不可用 + 不丢条"的**浸泡网**。
    ///
    /// ⚠️ **本用例不做的事**：它**不**放宽"中间行损坏 ⇒ 整页不可用"（设计 §4.5 明写），
    /// 也不去"修复"尾行 —— 因为尾行的字节**已经在盘上**（append 可见），抹掉它才是
    /// "把存在的记录说成不存在"。§4.5 的"损坏"指的是**内容不成立**，不是"还没写完换行"。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_concurrent_writer_never_makes_the_page_unavailable() {
        with_watchdog("并发写读审计", async {
            // ── (a) 确定性：完整 JSON + 无结尾 '\n'（= 写侧两次 write 之间的那个态）──
            let t = TempDir::new("audit-torn-line");
            let e = ent(T0 + 7, ConsoleOp::ConfigApply);
            let json = serde_json::to_string(&e).unwrap();
            std::fs::write(t.join(&console_audit_file_name(&day_of_ms(T0))), &json).unwrap();
            let p = svc(t.path()).page(&q_day(), T0).await;
            assert!(
                p.available,
                "末行无 '\\n' 但**内容是完整 JSON** ⇒ 是「写了一半」而不是「损坏」，不得翻成不可用"
            );
            assert_eq!(
                p.entries,
                vec![e],
                "该条的字节已在盘上 ⇒ 必须照常上屏（忽略它才是「静默少给」）"
            );

            // ── (b) 真并发 ──
            const ROUNDS: u64 = 200;
            let t2 = TempDir::new("audit-concurrent");
            let dir = t2.path().to_path_buf();
            let day_file = dir.join(console_audit_file_name(&day_of_ms(T0)));
            // 先把当天的空文件建出来（写侧 `append_line` 的 `create(true)` 本来就会在第一条
            // 落盘时建它；这里提前建只为给探针一个**稳定句柄**，且必须在写侧启动**之前**，
            // 免得与写侧抢时序）。
            std::fs::File::create(&day_file).expect("建当天审计文件");
            let sink = std::sync::Arc::new(FileAuditSink::open(&dir).unwrap());
            let writer = {
                let sink = sink.clone();
                // `spawn_blocking`：写侧是**同步 IO**（`append_line` + `sync_all`），
                // 丢到阻塞线程池才与异步读侧**真并行**（否则读侧会把它饿死 ⇒ 测不到并发）。
                tokio::task::spawn_blocking(move || {
                    for i in 0..ROUNDS {
                        sink.record_outcome(&ent(T0 + i, ConsoleOp::ConfigApply))
                            .expect("并发写入不得失败");
                    }
                })
            };

            let s = svc(&dir);
            // **为什么要"先探后查"**：撕裂窗口（JSON 已落盘、`'\n'` 未落盘）在生产里只持续
            // 一两个 write 系统调用的间隔（µs 级），随机时刻发起查询撞上的概率极低 ⇒ 那样
            // "跑 200 轮没红"其实是**没测到**而不是**没问题**。故每轮先**主动探**（纯读文件字节，
            // 极廉价）至多 `SPIN` 次，探到"末字节不是 `'\n'`"**就在那个相位里发起查询**。
            // ⚠️ **口径（本轮订正）**：这只把查询发在**撕裂窗口内**，**不等于**"该次查询读到了
            // 撕裂态"——探针读末字节与 `page()` 读文件之间仍隔着 µs 级间隔。故计数器只打印、
            // 不作断言；真正能红的判别力在 (a) 的确定性用例（把"末行无 `'\n'`"判成损坏 ⇒ 立刻红）。
            // 写侧跑完之后文件恒以 `'\n'` 结尾，此时不再空转探测。
            const SPIN: usize = 32;
            const MIN_QUERIES: u64 = 200;
            // 探针只读**最后 1 个字节**（一个常开句柄 + `seek` + `read`）：`std::fs::read` 每次
            // 都要 open + 整读 + 分配一个随文件增长的 `Vec`，采样率被它压到几千次/秒 ⇒ 撞见
            // µs 级撕裂窗口全靠运气；只读末字节可把采样率抬一个数量级（见实测打印）。
            let mut probe = std::fs::File::open(&day_file).expect("当天审计文件必须已建");
            let ends_with_newline = |f: &mut std::fs::File| -> Option<bool> {
                use std::io::{Read, Seek, SeekFrom};
                if f.seek(SeekFrom::End(0)).ok()? == 0 {
                    return Some(true); // 空文件（第一条还没落盘）⇒ 不算撕裂
                }
                f.seek(SeekFrom::End(-1)).ok()?;
                let mut b = [0u8; 1];
                f.read_exact(&mut b).ok()?;
                Some(b[0] == b'\n')
            };
            let (mut queries, mut unavailable, mut torn_seen) = (0u64, 0u64, 0u64);
            loop {
                let writer_running = !writer.is_finished();
                let mut torn_now = false;
                if writer_running {
                    for _ in 0..SPIN {
                        if ends_with_newline(&mut probe) == Some(false) {
                            torn_now = true;
                            break;
                        }
                    }
                }
                if torn_now {
                    torn_seen += 1;
                }
                let page = s.page(&q_day(), T0).await;
                queries += 1;
                if !page.available {
                    unavailable += 1;
                }
                if !writer_running && queries >= MIN_QUERIES {
                    break;
                }
            }
            writer.await.expect("写侧任务不得 panic");

            println!(
                "[审计并发实测] {ROUNDS} 次 append / {queries} 次并发查询 / 不可用 {unavailable} 次 / \
                 **在撕裂窗口内发起**的查询 {torn_seen} 次（**不等于**该次查询读到撕裂态）"
            );
            assert_eq!(
                unavailable, 0,
                "并发写入期间**任何一次**查询都不得翻成不可用（撕裂行 = 完整 JSON ⇒ 正常上岸）"
            );

            // 收尾：写完之后整本读回来 —— 200 条**一条不少、且不重复**
            let mut ids: Vec<String> = Vec::new();
            for page_no in 1..=12u32 {
                let page = s
                    .page(
                        &AuditQuery {
                            page: page_no,
                            ..q_day()
                        },
                        T0,
                    )
                    .await;
                assert!(page.available, "写完之后第 {page_no} 页必须可用");
                ids.extend(page.entries.iter().map(|e| e.id.clone()));
                if !page.has_more {
                    break;
                }
            }
            ids.sort();
            ids.dedup();
            assert_eq!(
                ids.len() as u64,
                ROUNDS,
                "并发写入的 {ROUNDS} 条必须一条不少、且不重复（撕裂行不得吞记录/造重复）"
            );
        })
        .await;
    }

    /// ⑳ **命中载荷预算（[`AUDIT_MATCHED_MAX_BYTES`]）的成对边界** —— 本单元补的最后一处网。
    ///
    /// ⚠️ **为什么单列一条（而不是并进 ⑱）**：代码质量评审实测，把
    /// `ScanState::push_entry` 里的 `matched_bytes > AUDIT_MATCHED_MAX_BYTES` 改成 **`>=`**
    /// ⇒ **全仓库零红**。即三道闸里只有这一道**"不该触发"的那一侧无人守**
    /// （行闸有成对的 ⑱(a)（恰 5 万 ⇒ 服务）/ ⑦③（+10 ⇒ 拒绝），文件闸有 ⑱(b)，
    /// 载荷闸此前**只有** ⑦④ 的"超预算 ⇒ 不可用"半网）。单列的直接理由是**代价**：
    /// 这一对本用例要造两份 ≈32 MiB 语料（与 ⑦④ 的 36 MB 同量级），并进 ⑱ 会把那个
    /// 30 s 挂死护栏的余量一次吃掉一截 ⇒ 分开挂各自的护栏。
    ///
    /// # 语料（`S` / `N` 怎么取）
    ///
    /// 行长恒为 `S = 8 KiB`（`entry_json_of_len`：合法 JSON，`}` 前垫空白）⇒
    /// `N = AUDIT_MATCHED_MAX_BYTES / S = 4096` 行，累计**恰好** 2^25 B = 33 554 432 B。
    /// 这两个值是**为了让三道闸互相可区分**：`S = 8 KiB` 远小于行上限 64 KiB、
    /// `N = 4096` 远小于行闸 50 000 ⇒ 本用例里**只有载荷闸能红**。
    /// （若贪心把 `S` 取成 64 KiB、或让 `N` 逼近 5 万，这一对就会与另两道闸的边界纠缠，
    /// 破坏性验证时说不清究竟是谁红的。）
    ///
    /// **+1 B** 那一侧：`(N − 1) 行 × S + 1 行 × (S + 1)` = 33 554 433 = **闸值 + 1**
    /// （只多 1 个字节 —— 钉的正是 `>` 与 `≥` 之间那条缝）。
    ///
    /// # 破坏性验证（本轮实做）
    ///
    /// * `>` 改 `>=` ⇒ 跑**整个** `cargo test -p mupc-core-bin`：**263 passed / 1 failed**，
    ///   那 1 条就是本条（报 `恰好 33 554 432 字节（= 32 MiB）是合法输入…必须照常服务`）
    ///   ⇒ 判别力确实只落在本条上（**反证了评审的"零红"**：本网不补，这道闸就是裸的）；
    /// * `cp` 还原（**不用 `git checkout --`**）⇒ 绿；还原后与备份 `cmp` **逐字节相同**。
    ///
    /// # 实测耗时（本机 x86_64、debug、`AUDIT_*` 常量与生产同值）
    ///
    /// 稳态 **1.23 / 1.33 / 1.43 / 1.51 / 1.52 / 1.58 s**（6 次独立运行）——
    /// 主要是两份 32 MiB 语料的落盘、读入与 8 192 行 JSON 解析。
    /// ⚠️ **只登记、不入断言**（墙钟受负载 / 杀毒 / 冷热缓存影响，断言它必然是 flaky）；
    /// 挂死由 `with_watchdog` 的 30 s 兜底（余量 ≥ 19×）。
    #[tokio::test]
    async fn the_matched_byte_budget_is_exact_at_32_mib() {
        with_watchdog("命中载荷预算的成对边界", async {
            let day = console_audit_file_name("2025-09-09");

            // ── 用例前提：这三个 `assert` 保证"只有载荷闸能红"（见上文，不是装饰）──
            const S: usize = 8 * 1024;
            assert_eq!(
                AUDIT_MATCHED_MAX_BYTES as usize % S,
                0,
                "用例前提：行长 S 必须整除闸值（否则凑不出「恰好闸值」的累计）"
            );
            assert!(
                (AUDIT_MATCHED_MAX_BYTES as usize / S) <= AUDIT_SCAN_MAX_LINES,
                "用例前提：语料行数必须远离行闸，否则这一对会与行闸的边界纠缠"
            );
            let n = AUDIT_MATCHED_MAX_BYTES as usize / S;
            let exact_line = entry_json_of_len(S);
            assert_eq!(
                exact_line.len(),
                S,
                "用例前提：`entry_json_of_len` 造出来的行长必须恰好是 S"
            );

            // ── (a) 累计**恰好** = 闸值 ⇒ 合法输入，必须照常服务 ──
            let t1 = TempDir::new("audit-matched-exact");
            let mut exact = String::with_capacity(n * (S + 1));
            for _ in 0..n {
                exact.push_str(&exact_line);
                exact.push('\n');
            }
            std::fs::write(t1.join(&day), &exact).unwrap();
            drop(exact); // 32 MiB 立刻释放（下面还要再造一份同量级语料）
            let p1 = svc(t1.path()).page(&q_day(), T0).await;
            assert!(
                p1.available && p1.entries.len() == AUDIT_PAGE_SIZE,
                "累计**恰好** {AUDIT_MATCHED_MAX_BYTES} 字节（= 32 MiB）是**合法输入**\
                 （闸是 `>` 不是 `≥`）⇒ 必须照常服务"
            );

            // ── (b) 累计 = 闸值 + 1 B ⇒ 必须**可见拒绝** ──
            let t2 = TempDir::new("audit-matched-over");
            let mut over = String::with_capacity(n * (S + 1) + 1);
            for _ in 0..(n - 1) {
                over.push_str(&exact_line);
                over.push('\n');
            }
            over.push_str(&entry_json_of_len(S + 1)); // 末行比 S 长 1 字节
            over.push('\n');
            std::fs::write(t2.join(&day), &over).unwrap();
            assert!(
                !svc(t2.path()).page(&q_day(), T0).await.available,
                "累计 {} 字节（= 闸值 + 1）必须**可见拒绝**（绝不静默截断成「只回前 32 MiB」）",
                AUDIT_MATCHED_MAX_BYTES + 1
            );
        })
        .await;
    }
}
