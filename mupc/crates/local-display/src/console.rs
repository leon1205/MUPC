//! 控制通道客户端 —— `/v1/console/*`（设计 §5.5「通道客户端（读 + 控制）」+ §3.3 / §3.4）。
//!
//! ## 职责
//!
//! - **信封构造**：`request_id`（UUID v4）、`issued_at_ms`、`op` 校验（**封闭清单** = §3.4 的
//!   8 个端点里 3 个写操作的路径末段）。查询用 GET（参数在 query），写操作用 POST（JSON body）。
//! - **非阻塞推进**：单次请求超时 **5 s**（设计 §5.5），由调用方**每拍推进一次**
//!   （[`ConsoleClient::tick`]）；`tick` **绝不阻塞**（不 `block_on`、不 `sleep`、不同步等待）。
//! - **幂等重试**：[`ConsoleClient::retry`] **原样重发同一份请求报文** ⇒ `request_id` 不变 ⇒
//!   服务端按 `(op, request_id)` 命中并返回**首次结果**（`duplicate=true`）。`duplicate=true`
//!   **不是错误**，原样上抛（[`OutcomeKind::Response`]）。
//! - **错误分类**：与 `channel.rs` 同口径 —— Connect / Timeout / Io / HttpStatus / Json，
//!   外加头/体上限、`request_id` 错位等；**任何路径不 panic**。
//!
//! ## ⚠️ 与设计 §5.5 的**逐字偏差**（B3-1 登记，供评审 / PM 裁定）
//!
//! 1. **`set_nonblocking(true)` 覆盖「写 → 读」两阶段；连接阶段用一次性工作线程**。
//!    设计写的是「`set_nonblocking(true)` + `connect → write → read` 三阶段状态机」。但
//!    `std::net::TcpStream` **没有**可移植的非阻塞 `connect`（`TcpStream::connect` /
//!    `connect_timeout` 都是阻塞语义，且 `connect_timeout(.., Duration::ZERO)` 会**丢弃**
//!    socket 句柄 ⇒ 无法续接）；真正的非阻塞 `connect` 需 `socket2` / 裸 `libc`（本单元禁引
//!    新依赖，且 Windows 本机必须可跑）。故：**连接**交一次性工作线程（它只做 `connect`，
//!    拿到 `TcpStream` 后立刻退出），**写 / 读**在 `tick` 内以 `set_nonblocking(true)` 推进。
//!    设计不变量（§5.2 条 3：阻塞 I/O 不得阻塞事件循环）**完全成立** —— `tick` 内只有
//!    `try_recv` / `write` / `read` 三种**非阻塞**调用，无任何等待（见
//!    `tick_never_blocks_on_a_silent_peer` 用例）。
//! 2. **枚举式返回**代替 `Option<Result<..>>`：`tick` 返回 [`Progress`]，「在途未完成」与
//!    「本拍结束」在类型上分开（`Option` 会把「无在途」与「仍在途」混为一谈）。
//! 3. **解析形状按端点区分**：§3.4 表里 GET 端点返回**裸** DTO（`ConfigView` / `LogPage` /
//!    `AuditPage` / `Vec<..>`），只有写操作走 `ControlResponse` 信封。故 [`OutcomeKind`] 两分支，
//!    不强行统一。设计已明确：**GET 返回裸 DTO、POST 走 `ControlResponse` 信封**；§3.3 原文
//!    「回执（**所有写操作**共用）」的作用域即 POST，与 §3.4 表格**不冲突**。**唯一真实缺口**
//!    是 `GET` **失败**时的错误通道设计未定义 —— 本实现按 HTTP 状态码分类并落在
//!    [`ConsoleError::HttpStatus`]（属合理填缺，已在模块头登记）。
//! 4. **重放窗口：`begin_*` 不校验，`retry` 校验**。`begin_*` 里用同一时刻既生成 `issued_at_ms`
//!    又校验它只会得到**恒真断言**（本项目明令禁止的伪门禁），故只校验**可判且能挡误路由**的
//!    两条 —— **操作名白名单**与**端点存在性**。但 [`ConsoleClient::retry`] 重发的是**先前签发**
//!    的信封，此刻注入时钟已前进 ⇒ 判据有真实语义：`now − issued_at_ms ≥ `[`REPLAY_WINDOW_MS`]
//!    时**响亮失败、不发包**（[`ConsoleError::RetryWindowExpired`]）。理由见下节。
//! 5. **不实现 `Transfer-Encoding: chunked`**：本仓库的服务端是自控的（回 `Content-Length`），
//!    分块解码属额外复杂度；遇到 chunked **响亮失败**（[`ConsoleError::ChunkedUnsupported`]），
//!    不让分块框架字节混进 JSON 再报一个看不懂的解码错。
//! 6. **〔登记，不修〕`RetryWindowExpired` 的出路指引"上了锁但看不见"**（B3-1 评审建议 9）。
//!    该错误的出路（作为**新操作**重发 + 按 T-3 **重新确认**）目前**只**写在它的英文
//!    `Display` 文案里；而接线层 [`crate::state::ControlState::record_transport_failure`] 对
//!    **所有**传输层失败一律弹同一句「操作失败」Toast（`TRANSPORT_FAIL_TEXT`）⇒ **B3-2 接线后
//!    用户看不到该指引**，只会看到"失败"（而首次操作**可能已生效**，正是本分支要防的静默偏差）。
//!    **待 B3-2 为 `RetryWindowExpired` 单列一条上屏路径**（专用文案 + "作为新操作重试"入口）。
//!    本单元不修：`console.rs` 无 UI 出口，改 `state.rs` 的通用失败文案会波及全部失败分支。
//!
//! ## 幂等 × 防重放窗口的**互相矛盾**（服务端面；客户端**在入口挡掉**）
//!
//! 服务端管线顺序是「2 信封 / 窗口 → 3 幂等查表」（§3.3）。于是**晚于 30 s 的重试**会先被窗口拒
//! （`OutsideReplayWindow` / `RejectedValidation`），**拿不到** `duplicate=true` 的首次结果 ——
//! 幂等重试与防重放窗口在 30 s 边界上互斥。本客户端单次超时 5 s，正常重试落在窗口内；
//! 但**连续重试**（5 s × 7 次）会越过边界。**后者正是 [`ConsoleClient::retry`] 硬拒绝的场景**：
//! 若照发，客户端会把回执的 `RejectedValidation` 显示成「操作失败」，而**首次操作很可能已经生效**
//! —— 那是**静默语义偏差**（比"拒绝重试"危险得多）。故客户端在窗口到期后**响亮失败**，并把出路
//! 写进错误文案：作为**新操作**（新 uuid）重新发起 + 按 T-3 **重新走确认**。
//!
//! 服务端侧收口建议（本单元不可解，登记备裁）：把幂等查表提到窗口校验之前；或重试时刷新
//! `issued_at_ms`（后者会改变幂等键的语义，须 PM 裁定）。

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mupc_display_proto::{
    ConsoleEndpoint, ConsoleMethod, ControlRequest, ControlResponse, REPLAY_WINDOW_MS,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// 单次请求超时（设计 §5.5：`console.rs` 单次超时 5 s）。
pub const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

/// 单拍内状态机最多推进的步数（防「对端持续可写 / 可读」把事件循环饿死）。
/// 耗尽即收工、下一拍续推 —— **进度不丢**（已写字节 / 已读字节都留在 `Pending` 里）。
const MAX_STEPS_PER_TICK: u32 = 64;

/// 响应头读取上限（64 KiB，与 `channel.rs` 同口径）。
///
/// **超额判定粒度（2026-09-15 补注）**：头是在读到「本次读块里出现 `\r\n\r\n`」或「本次读块后
/// 仍未出现」时才判的，块大小固定 **4096 B** ⇒ **判定点 ≤ 上限 + 4096**。⚠️ **实测口径**：
/// `HeadTooLarge` 报出的字节数**恰为** `65536 + 4096 = 69632`，即比本上限大 **4096**（不是 4095）
/// —— 独立复核实测订正了先前 off-by-one 的表述。即"略超上限"的头**一定会**被拒，但报出的数可能
/// 略大于上限本身。
pub const MAX_HEAD_BYTES: usize = 64 * 1024;

/// **单条日志消息（`LogEntry.message`）长度上限的假定值 = 1 KiB**（2026-09-15 补）。
///
/// `display-proto` 的 `LogEntry.message` 是**无长度约束的 `String`**（proto 侧未设上限），
/// 故"最大日志页有多大"只能靠一条**假定**来推算 —— 本条常量就是那个假定，它同时是
/// [`MAX_BODY_BYTES`] 的编译期门禁输入。
///
/// ⚠️ **该假定须由后端（`mupcd` 日志服务）保证**：单条消息超 1 KiB 即**截断**（并在条目上
/// 标注截断），否则两侧上限必须**一起上调**（改本条 + [`MAX_BODY_BYTES`]，编译期门禁会兜住）。
pub const ASSUMED_MAX_MESSAGE_BYTES: usize = 1024;

/// 响应体读取上限（**256 KiB**）。
///
/// 依据（B3-1 评审**重要 2** 订正）：上一版取 64 KiB，理由写作「`LogPage` 200 条 × 约 200 B
/// ≈ 40 KB」，**该估算不成立** —— `LogPage` 的 `LogEntry.message` 在 `display-proto` 里
/// **没有任何长度上限**，200 条 × 1 KiB = 200 KB 的**合法**回包会被判 [`ConsoleError::BodyTooLarge`]
/// （日志页直接不可用）。故按**真实契约常量**重新推算：
/// `LOG_PAGE_LIMIT_MAX`（= 200，`display-proto/src/log.rs`）× [`ASSUMED_MAX_MESSAGE_BYTES`]
/// （= 1 KiB）⇒ 200 KiB，取 **256 KiB** 留余量；门禁见下方编译期 `assert!`。
///
/// **与 `channel.rs` 的关系（勿再绑回"同口径"）**：读通道（帧）的 64 KiB 是**另一件事** —— 帧
/// 结构固定、**无自由文本**，其上限按帧长度域推算即可；控制通道的回包含**变长 JSON 文本**
/// （`LogEntry.message`），两者**服务不同端点、依据不同**，因此**不再要求两数相等**。
/// 要让两侧数值相同，是偶然而非约束。
///
/// **超额判定粒度（2026-09-15 补注）**：① 有 `Content-Length` 时**预检**，判定点 == 上限
/// （精确）；② 无长度头（读到 EOF 为止）时按读块累加后判，块大小固定 4096 B ⇒
/// **判定点 ≤ 上限 + 4096**。⚠️ 同 [`MAX_HEAD_BYTES`]：实测报出的数**可以正好大 4096**
/// （不是 4095），先前"最多大 4095"的表述已按实测订正。
pub const MAX_BODY_BYTES: usize = 256 * 1024;

/// 编译期门禁：上限必须放得下**已知最大回包**（`LogPage` 满页 = `LOG_PAGE_LIMIT_MAX` 条 ×
/// 单条 `message` 上限假定）。低于它就等于把正常回包判成 [`ConsoleError::BodyTooLarge`]
/// （响亮但错误的失败）。**门禁挂在真实契约常量上**，而非写死的"40 KB"数字。
const _: () = assert!(
    MAX_BODY_BYTES >= mupc_display_proto::log::LOG_PAGE_LIMIT_MAX * ASSUMED_MAX_MESSAGE_BYTES
);

/// 查询串长度上限（防调用方把超大串编进请求行）。8 KiB 远大于已知筛选件的最长 query
/// （`targets` ≤ 50 项 × 约 30 B ≈ 1.5 KB + `levels` 多值 + 时间范围 + 游标）。
pub const MAX_QUERY_BYTES: usize = 8 * 1024;

/// 控制通道默认基址（`display.control_bind_addr` 的客户端侧形态）。
pub const DEFAULT_CONSOLE_BASE_URL: &str = mupc_display_proto::DEFAULT_CONTROL_BASE_URL;

// ═══════════════════════════════════════════════════════════════════════════
// 1. 错误（分类口径与 `channel.rs` 一致；**本模块不 panic**）
// ═══════════════════════════════════════════════════════════════════════════

/// 控制通道错误。
///
/// `Connect` / `Timeout` / `Io` / `HttpStatus` / `Json` 五类与 `channel.rs` 逐条同口径
/// （同一套排障直觉）；其余为控制通道特有分支。**每个分支都有出口**（不静默吞）。
#[derive(Debug, thiserror::Error)]
pub enum ConsoleError {
    /// 基址 URL 非法（仅支持 `http://` 回环）。
    #[error("invalid console base url `{0}`: {1}")]
    BadUrl(String, String),

    /// TCP 连接失败（mupcd 未起 / 端口不存在 / 被拒）。
    #[error("console connect to `{0}` failed: {1}")]
    Connect(String, std::io::Error),

    /// 请求写入 / 响应读取失败。
    #[error("console io error on `{0}`: {1}")]
    Io(String, std::io::Error),

    /// 单次请求超时（5 s 截止到期，**在途请求当场作废**）。
    ///
    /// **语义边界（B3-1 评审建议 4）**：本分支**只**表示"截止到期且**没有**可归因的收包证据"。
    /// 若对端**已经回了字节**、只是响应头里始终凑不出 `\r\n\r\n`（如对端只发 `\n`），
    /// 收口为 [`ConsoleError::HeadIncomplete`] —— 否则诊断会指向"对端没回"，而真相是"对端回了，
    /// 是我方解析不出终止符"（根因误导）。
    #[error("console request `{0}` timed out")]
    Timeout(String),

    /// **响应头未完成**：截止到期前**已收到 `k` 字节**，但其中始终没有 `\r\n\r\n`。
    ///
    /// 典型成因：对端用 **LF-only**（只发 `\n`）作行结束符，或头被截断后对端保持连接。
    /// 与 [`ConsoleError::Timeout`] 分开，是为了让排障一眼看出"对端**回了**字节"。
    #[error(
        "console response header from `{0}` is incomplete: {1} byte(s) received but no CRLFCRLF \
         terminator (LF-only line endings or truncated head?)"
    )]
    HeadIncomplete(String, usize),

    /// 非 200 响应（含 404 / 5xx）。
    #[error("console http status {0} from `{1}`")]
    HttpStatus(u16, String),

    /// 请求体编码失败（`ControlRequest` 序列化；正常载荷不可达，留作出路而非静默）。
    #[error("console request encode error for `{0}`: {1}")]
    Encode(String, serde_json::Error),

    /// 响应体解码失败（含回执缺语义字段 —— `field_errors` / `duplicate` 是必需字段）。
    #[error("console body decode error from `{0}`: {1}")]
    Json(String, serde_json::Error),

    /// 响应头超过上限。
    #[error("console response header too large from `{0}`: {1} bytes > limit")]
    HeadTooLarge(String, usize),

    /// 响应体超过上限（`Content-Length` 预检，或无长度头时的读取封顶）。
    #[error("console body too large from `{0}`: {1} bytes > limit")]
    BodyTooLarge(String, usize),

    /// 分块传输（`Transfer-Encoding: chunked`）不支持 —— 响亮失败（见模块头偏差 5）。
    #[error("console response from `{0}` is chunked (unsupported; needs Content-Length)")]
    ChunkedUnsupported(String),

    /// 操作名不在**封闭清单**内（§3.4 的 8 个端点里 3 个写操作）。
    #[error("unknown console op `{0}` (not in the closed op list)")]
    UnknownOp(String),

    /// 对**查询端点**发起了写操作 —— 防误路由。
    #[error("console endpoint `{0:?}` is not a write endpoint")]
    NotAWriteEndpoint(ConsoleEndpoint),

    /// 对**写端点**发起了查询。
    #[error("console endpoint `{0:?}` is a write endpoint, not a query endpoint")]
    NotAQueryEndpoint(ConsoleEndpoint),

    /// 查询串含非法字符（控制字符 / 空格 / `#`）—— 防请求行注入。
    #[error("illegal console query string `{0}`")]
    BadQuery(String),

    /// 查询串过长（超过 [`MAX_QUERY_BYTES`]）—— 防超大串被编进请求行。
    #[error("console query string too long: {0} bytes > limit")]
    QueryTooLarge(usize),

    /// 回执 `request_id` 与在途请求不符（错位回执）—— 响亮失败，**不**静默当成本次结果。
    #[error("console response request_id `{got}` does not match in-flight `{expected}`")]
    RequestIdMismatch {
        /// 在途请求的 `request_id`。
        expected: String,
        /// 回执声明的 `request_id`。
        got: String,
    },

    /// 已有在途请求（同一客户端同时只允许一条在飞；调用方须等它结束或 [`ConsoleClient::cancel`]）。
    #[error("console client is busy with `{0}`")]
    Busy(String),

    /// 无在途请求（`retry` 的**响亮失败**，不是静默 no-op）。
    #[error("console client has no in-flight request")]
    Idle,

    /// **重试已过期**：重发的信封印于 `issued_at_ms`，此刻距其已 ≥ 服务端防重放窗口
    /// （`REPLAY_WINDOW_MS` = 30 s）。**原样重发必被服务端窗口先拒**（§3.3 管线「2 窗口 →
    /// 3 幂等表」）⇒ 客户端会把「**首次操作可能已生效**」显示成「操作失败」（静默语义偏差）。
    ///
    /// 出路（**唯一**）：把该操作当作**新操作**重新发起（新 `request_id`），并按 T-3
    /// **重新走一次确认**（长按 / 双步确认完成前不得发包）。本分支**不发任何包**。
    #[error(
        "console retry of `{op}` is stale: issued_at_ms is {age_ms} ms old (>= {window_ms} ms replay \
         window) - it must be re-sent as a NEW op (new uuid) and re-confirmed (T-3), not replayed"
    )]
    RetryWindowExpired {
        /// 被封的写操作名（查询端点无信封 ⇒ 不会走到本分支）。
        op: String,
        /// 原封的签发时刻（Unix ms）。
        issued_at_ms: u64,
        /// 距签发时刻的时长（ms）。
        age_ms: u64,
        /// 服务端防重放窗口（[`REPLAY_WINDOW_MS`]）。
        window_ms: u64,
    },
}

/// 控制通道结果类型。
pub type ConsoleResult<T> = std::result::Result<T, ConsoleError>;

// ═══════════════════════════════════════════════════════════════════════════
// 2. 操作名封闭清单（§3.4）
// ═══════════════════════════════════════════════════════════════════════════

/// 全部写操作名（**唯一真源** = [`ConsoleEndpoint`] 的路径末段，不另抄一份字面量）。
///
/// 共 3 条（`apply` / `release` / `ack_m1`）。
pub fn write_ops() -> impl Iterator<Item = &'static str> {
    ConsoleEndpoint::ALL.into_iter().filter_map(|e| e.op_name())
}

/// 操作名 → 写端点（不在清单内 → `None`）。
///
/// 这是"操作名是否在封闭清单内"的**唯一**判据（B3-1 评审阻塞 1 同类项：曾另有一个
/// `is_known_op` 公开函数，职责与本函数完全重合且只有测试引用 ⇒ 已删，测试改用本函数）。
pub fn endpoint_for_op(op: &str) -> Option<ConsoleEndpoint> {
    ConsoleEndpoint::ALL
        .into_iter()
        .find(|e| e.op_name() == Some(op))
}

/// 查询串的百分号编码（保留 `A-Za-z0-9-_.~`，其余含非 ASCII 逐字节 `%XX`）。
///
/// 供 P3 / P5 的筛选件拼 `levels=` / `targets=` / `from=` / `to=` 等参数；
/// 拼好后交给 [`ConsoleClient::begin_query`]（它仍会做请求行注入校验）。
pub fn encode_query(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        push_encoded(&mut out, k);
        out.push('=');
        push_encoded(&mut out, v);
    }
    out
}

/// 逐字节百分号编码（非保留字符原样）。
fn push_encoded(out: &mut String, s: &str) {
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push('%');
            out.push(hex_digit(b >> 4));
            out.push(hex_digit(b & 0x0f));
        }
    }
}

/// 半字节 → 大写十六进制字符。
///
/// **入参先掩到低 4 位**（B3-1 评审建议 7）：本函数签名收任意 `u8`，此前 `nibble > 15` 时
/// `b'A' + (nibble - 10)` 会 `u8` 溢出 ⇒ **debug 构建 panic**（release 静默回绕）。当前调用点
/// 传的都是 `b >> 4` / `b & 0x0f`（必然 ≤ 15，不可达），但"不可达"是**调用点的性质**而非本函数
/// 的性质 ⇒ 在函数内自守，取低 4 位（`0xff` → `'F'`）。
fn hex_digit(nibble: u8) -> char {
    let n = nibble & 0x0f;
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'A' + (n - 10)) as char,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 注入时钟（本模块**不读**系统时钟做业务判断）
// ═══════════════════════════════════════════════════════════════════════════

/// 发起一次请求所需的**注入时钟**（一次给两个时钟源，免调用方各自读表）。
#[derive(Debug, Clone, Copy)]
pub struct ConsoleClock {
    /// Unix 毫秒 —— 写进信封 `issued_at_ms`（服务端据此判 ±30 s 重放窗口）。
    pub wall_ms: u64,
    /// 单调时钟 —— 超时截止（[`CONTROL_TIMEOUT`]）的起点。
    pub start: Instant,
}

impl ConsoleClock {
    /// 生产用：读系统时钟（`SystemTime` → Unix ms，`Instant::now()` → 单调起点）。
    ///
    /// 时钟读失败（系统时间早于 1970）取 `0`，**不 panic**（服务端会以窗口校验拒绝，属响亮失败）。
    pub fn now() -> Self {
        let wall_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        Self {
            wall_ms,
            start: Instant::now(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 完成结果
// ═══════════════════════════════════════════════════════════════════════════

/// 一次请求的完成结果（解析后的形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleOutcome<T> {
    /// 本次请求的目标端点（调用方据此分流：配置 / 日志 / 审计 / 联锁…）。
    pub endpoint: ConsoleEndpoint,
    /// 载荷本体。
    pub kind: OutcomeKind<T>,
}

/// 载荷形态（写操作 = 控制回执信封；查询 = §3.4 的裸 DTO）。
#[derive(Debug, Clone, PartialEq)]
pub enum OutcomeKind<T> {
    /// 写操作：控制回执信封（`applied` 已按 `T` 解码）。
    Response(ControlResponse<T>),
    /// 查询：GET 端点的裸载荷。
    Query(T),
}

impl<T> ConsoleOutcome<T> {
    /// 控制回执（查询结果 → `None`）。
    pub fn response(&self) -> Option<&ControlResponse<T>> {
        match &self.kind {
            OutcomeKind::Response(r) => Some(r),
            OutcomeKind::Query(_) => None,
        }
    }

    /// 查询载荷（写回执 → `None`）。
    pub fn query(&self) -> Option<&T> {
        match &self.kind {
            OutcomeKind::Query(v) => Some(v),
            OutcomeKind::Response(_) => None,
        }
    }
}

/// 每拍推进的产出。
#[derive(Debug)]
pub enum Progress<T> {
    /// 无进展：无在途请求，或在途且未结束（**调用方下一拍再来**）。
    Pending,
    /// 在途请求本拍结束（成功或失败）。
    Done(ConsoleResult<ConsoleOutcome<T>>),
}

/// 一次请求的描述（`retry` 需要它原样重发）。
#[derive(Debug, Clone)]
struct RequestSpec {
    /// 完整 HTTP 请求报文（**含 `request_id`** ⇒ 重发即幂等键不变）。
    raw: Vec<u8>,
    /// 目标端点。
    endpoint: ConsoleEndpoint,
    /// 信封 `request_id`（GET 无信封 ⇒ `None`）。
    request_id: Option<String>,
    /// 信封 `issued_at_ms`（GET 无信封 ⇒ `None`，**不参与**重放窗口判定）。
    issued_at_ms: Option<u64>,
}

/// 状态机阶段（仅作推进标记；`TcpStream` 由 [`Pending::stream`] 持有）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// 工作线程正在 `connect`。
    Connecting,
    /// 正在写出请求报文。
    Writing,
    /// 正在读入响应。
    Reading,
}

/// 在途请求的推进状态。
#[derive(Debug)]
struct Pending {
    spec: RequestSpec,
    /// `host:port`（错误串用）。
    addr: String,
    /// 超时截止时刻（`Instant`，单次 [`CONTROL_TIMEOUT`]）。
    deadline: Instant,
    phase: Phase,
    /// 连接阶段的接收端（拿到 socket 后置 `None`）。
    rx: Option<Receiver<std::io::Result<TcpStream>>>,
    /// 已建立的连接（此后一律非阻塞写 / 读）。
    stream: Option<TcpStream>,
    /// 已写出字节数。
    sent: usize,
    /// 已读入的响应头。
    head: Vec<u8>,
    /// 响应头是否已读全（`\r\n\r\n` 已出现）。
    head_done: bool,
    /// `Content-Length`；`None` 且 `head_done` ⇒ 读到 EOF 为止（与 `channel.rs` 同口径）。
    content_length: Option<usize>,
    /// 已读入的响应体。
    body: Vec<u8>,
}

/// 状态机单步结果。
enum Step {
    /// 阶段推进，同一拍内继续。
    Continue,
    /// 本拍到此为止（对端暂时不可写 / 不可读）。
    Blocked,
    /// 完成（体已读全）。
    Finished,
    /// 失败（响亮）。
    Failed(ConsoleError),
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 客户端
// ═══════════════════════════════════════════════════════════════════════════

/// 控制通道客户端（**同一时刻至多一条在途请求**）。
///
/// 驱动方式（设计 §5.5 不变量 3）。
///
/// 取 `no_run`（**编译**、不在测试期真连网）而非 `ignore`：`ignore` 的代码块**永不编译**，
/// 会与实现**静默漂移**（B3-1 评审重要 6）。`no_run` 让接口签名（`begin_write` / `tick` /
/// `Progress` / `InterlockOpAck`）始终受编译器看管；`use` 亦已补齐。
///
/// ```no_run
/// use mupc_display_proto::InterlockOpPayload;
/// use mupc_local_display::console::{
///     ConsoleClient, ConsoleClock, Progress, DEFAULT_CONSOLE_BASE_URL,
/// };
/// use std::time::Instant;
///
/// let mut c = ConsoleClient::new(DEFAULT_CONSOLE_BASE_URL)?;
/// let payload = InterlockOpPayload {
///     observed_latched: false,
///     observed_sources: Vec::new(),
/// };
/// c.begin_write("release", &payload, ConsoleClock::now())?;
/// loop {
///     match c.tick::<mupc_display_proto::InterlockOpAck>(Instant::now()) {  // ← 永不阻塞
///         Progress::Pending => { /* 顺带做别的：LVGL / 触摸 / 帧 */ }
///         Progress::Done(r) => {
///             let _ = r; // 结果填 state + Toast
///             break;
///         }
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct ConsoleClient {
    /// 目标地址（**解析后的唯一真源**，B3-1 评审建议 8）。
    ///
    /// 上一版存 `host: String` + `port: u16`，于是「入口校验用的 `format!("{host}:{port}")`」与
    /// 「[`Self::addr`] 里的 `format!("{}:{}", ..)`」是**两处必须永远逐字一致的字面量** —— 属
    /// **隐式契约**（将来给 host 加归一化就会无声破坏），且 [`connect`] 里那条"不可达"的无界
    /// 兜底分支只有靠"两处一致"才成立。现存 `SocketAddr`：入口 `parse` 一次，`addr()` 与
    /// `connect` 都从它派生 ⇒ **表达式中只有一处 `format!`**，不可能漂移；无界兜底分支随之
    /// **整体删除**（不再需要"靠构造保证不可达"的结构性防线）。
    addr: SocketAddr,
    timeout: Duration,
    pending: Option<Pending>,
    /// 最近一次请求的描述（`retry` 原样重发 ⇒ 幂等键不变）。
    last: Option<RequestSpec>,
    /// 连续失败数（超时 / 连接 / HTTP / 解码；成功即清零）。
    fail_streak: u32,
}

/// 默认地址（[`DEFAULT_CONSOLE_BASE_URL`] 的解析结果，编译期即确定 ⇒ `Default` 不需要 `unwrap`）。
const DEFAULT_CONSOLE_ADDR: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 9811);

impl Default for ConsoleClient {
    fn default() -> Self {
        Self::new(DEFAULT_CONSOLE_BASE_URL).unwrap_or_else(|_| Self {
            // 默认 URL 是常量、解析必成功 ⇒ 此分支不可达；写成不 panic 的兜底而非 `expect`。
            addr: DEFAULT_CONSOLE_ADDR,
            timeout: CONTROL_TIMEOUT,
            pending: None,
            last: None,
            fail_streak: 0,
        })
    }
}

impl ConsoleClient {
    /// 由基址 URL 建客户端（仅 `http://`；host **必须是可作 `SocketAddr` 的 IP 字面量**，不做 DNS
    /// —— 与 `channel.rs` 同纪律）。
    ///
    /// **host 校验在入口（B3-1 评审重要 4；复核 B4 订正）**：非 IP 字面量会让 [`connect`] 退化为
    /// **无界**阻塞 `TcpStream::connect`（工作线程可超出 5 s 截止存活）。这里**响亮拒绝**
    /// （错误走既有的 [`ConsoleError::BadUrl`]，**不 panic**）。
    ///
    /// ⚠️ **解析结果上收到类型层（B3-1 评审建议 8）**：这里**一次性**把 `host:port` 解析成
    /// [`SocketAddr`] 存进结构体，[`Self::addr`] 与 [`connect`] 都从它派生 ⇒ 不存在"两处
    /// `format!` 必须永远一致"的隐式契约（给 host 加归一化也不会无声破坏），`connect` 也
    /// **只剩有界分支**。上一版按"剥掉方括号后是不是 `IpAddr`"判定，**不足以**保证不可达：
    /// `http://::1:9811` 能过那个判据，但 `addr()` 得到 `"::1:9811"` 解析失败 ⇒ 仍落到无界分支
    /// （独立复核实测点名）—— 现在这种写法在入口就**必被拒**，因为解析就发生在这里。
    pub fn new(base_url: &str) -> ConsoleResult<Self> {
        let ep = crate::channel::ChannelEndpoint::parse(base_url)
            .map_err(|e| ConsoleError::BadUrl(base_url.to_string(), e.to_string()))?;
        let addr = format!("{}:{}", ep.host, ep.port)
            .parse::<SocketAddr>()
            .map_err(|_| {
                ConsoleError::BadUrl(
                    base_url.to_string(),
                    format!(
                        "host `{}` 无法与端口拼成可解析的 SocketAddr（本设计不做 DNS；\
                         IPv6 必须写成带方括号的 `http://[::1]:9811`）",
                        ep.host
                    ),
                )
            })?;
        Ok(Self {
            addr,
            timeout: CONTROL_TIMEOUT,
            pending: None,
            last: None,
            fail_streak: 0,
        })
    }

    /// 覆盖单次超时（**测试专用**；生产恒为 [`CONTROL_TIMEOUT`]）。
    #[cfg(test)]
    pub(crate) fn with_timeout(base_url: &str, timeout: Duration) -> ConsoleResult<Self> {
        let mut c = Self::new(base_url)?;
        c.timeout = timeout;
        Ok(c)
    }

    /// 目标 `host:port`（由结构体的 `addr` 字段直接格式化 ⇒ 与 [`connect`] 的入参**同源**）。
    pub fn addr(&self) -> String {
        self.addr.to_string()
    }

    /// 单次超时。
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// 是否有在途请求。
    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    /// 在途请求的 `request_id`（GET 无信封 ⇒ `None`）。
    pub fn inflight_request_id(&self) -> Option<&str> {
        self.pending
            .as_ref()
            .and_then(|p| p.spec.request_id.as_deref())
    }

    /// **最近一次**请求的信封 `issued_at_ms`（查询端点无信封 ⇒ `None`）。
    ///
    /// 与 [`Self::retry`] 判的是**同一份** spec ⇒ 接线层可用它预先决定"还值不值得重试"，
    /// 而不必先触发一次失败。
    pub fn issued_at_ms(&self) -> Option<u64> {
        self.last.as_ref().and_then(|spec| spec.issued_at_ms)
    }

    /// 最近一次请求的信封**年龄**（`now_ms − issued_at_ms`，ms；无信封 ⇒ `None`）。
    ///
    /// 时钟回拨时按 0 计（`saturating_sub`），不做 panic 也不隐藏负值。
    pub fn request_age_ms(&self, now_ms: u64) -> Option<u64> {
        self.issued_at_ms().map(|t| now_ms.saturating_sub(t))
    }

    /// 连续失败数（成功即清零）。
    pub fn fail_streak(&self) -> u32 {
        self.fail_streak
    }

    /// 本拍/本次可观察的阶段名（**诊断用**：`idle` / `connecting` / `writing` / `reading`）。
    pub fn phase_name(&self) -> &'static str {
        match self.pending.as_ref().map(|p| p.phase) {
            None => "idle",
            Some(Phase::Connecting) => "connecting",
            Some(Phase::Writing) => "writing",
            Some(Phase::Reading) => "reading",
        }
    }

    /// 放弃在途请求（本地**即刻**丢弃状态；**不发任何应用层字节**）。
    ///
    /// 用于「用户切了筛选条件，旧查询作废」。无在途请求时是**幂等 no-op**。
    ///
    /// ⚠️ **"不发包"的边界（B3-1 评审建议 5 订正）**：上一版称"不发任何包"，**过强**。
    /// 按阶段如实说：
    ///
    /// - **连接阶段**：工作线程**已经在跑 `connect`**，TCP SYN **可能早已发出**（`connect(2)`
    ///   不可撤回）；本方法只丢弃接收端。线程最多再存活一个 [`CONTROL_TIMEOUT`]（`connect_timeout`
    ///   的上界）后自行退出，**期间不写任何应用层字节**（HTTP 请求只由 `tick` 的写阶段发出，
    ///   而状态已被丢弃）。若连接最终成功，句柄随 `tx.send` 失败一并析构 ⇒ 对端会看到
    ///   一次**建立后立刻关闭**的连接（SYN → RST/FIN），而**不是**一次完整请求。
    /// - **写阶段**：已写出的字节**留在内核发送缓冲里**，对端可能读到**半截**请求报文
    ///   （对端按自己的超时 / 头部不完整收口）。本方法**不能**撤回它们。
    /// - **读阶段**：只丢本地缓冲 + 析构 socket ⇒ **应用层不再发任何字节**；但析构 `TcpStream`
    ///   时 **TCP 层仍会发 FIN**（与连接阶段的口径一致 —— 对端会看到连接被关闭，而不是"什么都没发生"）。
    ///   对端若还持有它那条连接，会按自己的超时收口。
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// 发起**写操作**（POST JSON body）。
    ///
    /// - `op` 必须是封闭清单内的写操作名（[`write_ops`]）；否则 [`ConsoleError::UnknownOp`]。
    /// - `request_id` 由本方法生成（UUID v4），`issued_at_ms` 取 `clock.wall_ms`。
    /// - 返回本次的 `request_id`（调用方留档 / 与回执对拍用）。
    pub fn begin_write<P: Serialize>(
        &mut self,
        op: &str,
        payload: &P,
        clock: ConsoleClock,
    ) -> ConsoleResult<String> {
        let endpoint = endpoint_for_op(op).ok_or_else(|| ConsoleError::UnknownOp(op.to_string()))?;
        if !endpoint.is_write() {
            // 结构性防线：`op_name()` 只对写端点返回 `Some` ⇒ 此处恒假；若将来其语义被改宽，
            // 这里立刻响亮失败，而不是发出「GET + body」这种四不像请求。
            return Err(ConsoleError::NotAWriteEndpoint(endpoint));
        }
        let request_id = uuid::Uuid::new_v4().to_string();
        let request = ControlRequest::new(request_id.clone(), clock.wall_ms, op, payload);
        let body = serde_json::to_vec(&request)
            .map_err(|e| ConsoleError::Encode(self.addr(), e))?;
        let mut raw = self.head_bytes(endpoint, None, Some(body.len()));
        raw.extend_from_slice(&body);
        self.start(
            RequestSpec {
                raw,
                endpoint,
                request_id: Some(request_id.clone()),
                issued_at_ms: Some(clock.wall_ms),
            },
            clock,
        )?;
        Ok(request_id)
    }

    /// 发起**查询**（GET；参数在 query，`query` 由 [`encode_query`] 生成或调用方自拼）。
    ///
    /// `query` 为空表示无参。
    ///
    /// **长度上限**（B3-1 评审建议 7）：`query` 的**字面**长度不得超过 [`MAX_QUERY_BYTES`]
    /// （8 KiB）。未经上限时，1 MB 的 query 会被编进请求行、且经 [`encode_query`] 百分号编码后
    /// 膨胀到约 3 MB，一路进到内核发送缓冲 —— 属"调用方传错"却表现为网络层怪象。
    pub fn begin_query(
        &mut self,
        endpoint: ConsoleEndpoint,
        query: &str,
        clock: ConsoleClock,
    ) -> ConsoleResult<()> {
        if endpoint.is_write() {
            return Err(ConsoleError::NotAQueryEndpoint(endpoint));
        }
        if query.len() > MAX_QUERY_BYTES {
            return Err(ConsoleError::QueryTooLarge(query.len()));
        }
        if query
            .bytes()
            .any(|b| b <= 0x20 || b == 0x7f || b == b'#')
        {
            return Err(ConsoleError::BadQuery(query.to_string()));
        }
        let raw = self.head_bytes(endpoint, (!query.is_empty()).then_some(query), None);
        self.start(
            RequestSpec {
                raw,
                endpoint,
                request_id: None,
                // GET **无信封** ⇒ 服务端没有可裁决的 `issued_at_ms` ⇒ 重放窗口不适用。
                issued_at_ms: None,
            },
            clock,
        )
    }

    /// **幂等重试**：原样重发**上一次**请求（同一 `request_id`、同一 `issued_at_ms`）。
    ///
    /// 服务端据 `(op, request_id)` 命中并返回首次结果（`duplicate=true`）。
    /// 无历史请求 → [`ConsoleError::Idle`]；已有在途 → [`ConsoleError::Busy`]（**不静默排队**）。
    ///
    /// **重放窗口硬拒绝（B3-1 评审阻塞 2）**：写操作的信封签发时刻距 `clock.wall_ms`
    /// **≥ [`REPLAY_WINDOW_MS`]** 时返回 [`ConsoleError::RetryWindowExpired`]，**不发任何包**。
    /// 理由见模块头「幂等 × 防重放窗口」——服务端管线是「2 窗口 → 3 幂等表」，过期重发**先被
    /// 窗口拒**，客户端会把「首次操作可能已生效」显示成「操作失败」（静默语义偏差）。
    ///
    /// **边界口径**：客户端取 `≥`，服务端（`ControlRequest::validate_envelope`）取 `>`。
    /// 客户端更保守 —— 客户端判定与服务端裁决之间还有网络与处理时延，边界上必然已越界；
    /// 宁可让用户"作为新操作重发"，也不把可能已生效的操作报成失败。
    ///
    /// 不变更既有语义：成功路径仍是**原样重发同一份报文**（`request_id` 与 `issued_at_ms`
    /// 逐字节不变 —— 那是幂等的前提）。本方法**不做任何自动重发**。
    pub fn retry(&mut self, clock: ConsoleClock) -> ConsoleResult<()> {
        if let Some(p) = self.pending.as_ref() {
            return Err(ConsoleError::Busy(p.spec.endpoint.op_name().unwrap_or("query").into()));
        }
        let spec = self.last.clone().ok_or(ConsoleError::Idle)?;
        // 查询端点无信封（`issued_at_ms = None`）⇒ 服务端没有窗口可判 ⇒ 不做本校验。
        if let Some(issued_at_ms) = spec.issued_at_ms {
            let age_ms = clock.wall_ms.saturating_sub(issued_at_ms);
            if age_ms >= REPLAY_WINDOW_MS {
                return Err(ConsoleError::RetryWindowExpired {
                    op: spec.endpoint.op_name().unwrap_or("query").to_string(),
                    issued_at_ms,
                    age_ms,
                    window_ms: REPLAY_WINDOW_MS,
                });
            }
        }
        self.start(spec, clock)
    }

    /// 每拍推进一次（**绝不阻塞**：只有 `try_recv` 与非阻塞 `write` / `read`）。
    ///
    /// `T` 为**本次端点**的载荷类型：写操作为 `applied` 的类型，查询为裸 DTO 类型。
    pub fn tick<T: DeserializeOwned>(&mut self, now: Instant) -> Progress<T> {
        let Some(pending) = self.pending.as_mut() else {
            return Progress::Pending;
        };

        // 截止判定**只有下面 `for` 循环里那一处**（循环每次迭代开头先判）。循环外**再写一遍**
        // 是同一判据的重复，行为等价 ⇒ 属死等价代码，B3-1 评审重要 3 已删（保留的那份见下）。

        // 借用范围内的推进；结果带出借用后再改 `self`（避免「持借用改 self」）。
        let step_out: ConsoleResult<bool> = {
            let p = pending;
            let mut out: ConsoleResult<bool> = Ok(false);
            for _ in 0..MAX_STEPS_PER_TICK {
                if now >= p.deadline {
                    out = Err(timeout_or_head_incomplete(p));
                    break;
                }
                match advance(p) {
                    Step::Continue => continue,
                    Step::Blocked => return Progress::Pending,
                    Step::Finished => {
                        out = Ok(true);
                        break;
                    }
                    Step::Failed(e) => {
                        out = Err(e);
                        break;
                    }
                }
            }
            out
        };

        match step_out {
            // 步数用尽但未结束：**进度保留**，下一拍续推。
            Ok(false) => Progress::Pending,
            Err(e) => {
                self.pending = None;
                self.fail_streak = self.fail_streak.saturating_add(1);
                Progress::Done(Err(e))
            }
            Ok(true) => match self.pending.take() {
                Some(p) => self.parse::<T>(p),
                None => Progress::Pending,
            },
        }
    }

    // ── 内部 ──────────────────────────────────────────────────────────────

    /// 解析已读全的响应体（借用已释放，可自由改 `self.fail_streak`）。
    fn parse<T: DeserializeOwned>(&mut self, p: Pending) -> Progress<T> {
        let addr = p.addr;
        let endpoint = p.spec.endpoint;
        let expected_id = p.spec.request_id;
        let body = p.body;
        let parsed: ConsoleResult<ConsoleOutcome<T>> = if endpoint.is_write() {
            serde_json::from_slice::<ControlResponse<T>>(&body)
                .map_err(|e| ConsoleError::Json(addr.clone(), e))
                .and_then(|resp| match &expected_id {
                    Some(expected) if resp.request_id != *expected => {
                        Err(ConsoleError::RequestIdMismatch {
                            expected: expected.clone(),
                            got: resp.request_id.clone(),
                        })
                    }
                    _ => Ok(ConsoleOutcome {
                        endpoint,
                        kind: OutcomeKind::Response(resp),
                    }),
                })
        } else {
            serde_json::from_slice::<T>(&body)
                .map(|payload| ConsoleOutcome {
                    endpoint,
                    kind: OutcomeKind::Query(payload),
                })
                .map_err(|e| ConsoleError::Json(addr, e))
        };
        match parsed {
            Ok(o) => {
                self.fail_streak = 0;
                Progress::Done(Ok(o))
            }
            Err(e) => {
                self.fail_streak = self.fail_streak.saturating_add(1);
                Progress::Done(Err(e))
            }
        }
    }

    /// 组装请求行 + 全部请求头（**含结尾空行**；body 由调用方追加）。
    fn head_bytes(
        &self,
        endpoint: ConsoleEndpoint,
        query: Option<&str>,
        content_length: Option<usize>,
    ) -> Vec<u8> {
        let mut path = endpoint.path().to_string();
        if let Some(q) = query {
            path.push('?');
            path.push_str(q);
        }
        let method = match endpoint.method() {
            ConsoleMethod::Get => "GET",
            ConsoleMethod::Post => "POST",
        };
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n",
            self.addr()
        );
        if let Some(len) = content_length {
            head.push_str("Content-Type: application/json\r\n");
            head.push_str(&format!("Content-Length: {len}\r\n"));
        }
        head.push_str("\r\n");
        head.into_bytes()
    }

    /// 起一条在途请求（连接工作线程 + 状态机初值）。
    fn start(&mut self, spec: RequestSpec, clock: ConsoleClock) -> ConsoleResult<()> {
        if let Some(p) = self.pending.as_ref() {
            return Err(ConsoleError::Busy(p.spec.endpoint.op_name().unwrap_or("query").into()));
        }
        let addr = self.addr();
        let deadline = clock.start + self.timeout;

        // 连接只在工作线程里做（原因 = 模块头偏差 1）；它拿到 socket 后立刻退出。
        let (tx, rx) = mpsc::channel();
        let connect_addr = self.addr; // `SocketAddr` 是 `Copy`；与 `addr()` 同源、不会漂移
        let budget = self.timeout;
        std::thread::Builder::new()
            .name("mupc-console-connect".to_string())
            .spawn(move || {
                let res = connect(connect_addr, budget);
                // 接收端可能已因超时被丢弃 ⇒ send 失败即静默结束（已无在途请求需要它）。
                let _ = tx.send(res);
            })
            .map_err(|e| ConsoleError::Io(addr.clone(), e))?;

        self.pending = Some(Pending {
            spec: spec.clone(),
            addr,
            deadline,
            phase: Phase::Connecting,
            rx: Some(rx),
            stream: None,
            sent: 0,
            head: Vec::new(),
            head_done: false,
            content_length: None,
            body: Vec::new(),
        });
        // `last` 只在**成功起请求之后**更新 ⇒ `retry` 面对的永远是"上一次真的发出去过的那份"。
        self.last = Some(spec);
        Ok(())
    }
}

/// 连接（**在工作线程里调用**；**恒有界** —— `timeout` 上界，不用裸 `connect` 的无界等待）。
///
/// **无界分支已随类型上收而删除**（B3-1 评审建议 8）：入参是 [`SocketAddr`]，`connect_timeout`
/// 是唯一出口 ⇒ "退化到无界 `TcpStream::connect`"在**类型上**不可能发生，不再需要一条
/// "靠入口校验保证不可达"的兜底代码（那种保证的载体是一对必须永远一致的 `format!` 字面量）。
fn connect(addr: SocketAddr, timeout: Duration) -> std::io::Result<TcpStream> {
    TcpStream::connect_timeout(&addr, timeout)
}

/// 状态机单步（**非阻塞**：只做 `try_recv` / `write` / `read`）。
fn advance(p: &mut Pending) -> Step {
    match p.phase {
        Phase::Connecting => {
            let Some(rx) = p.rx.as_ref() else {
                return Step::Failed(io_err(&p.addr, "connect receiver missing"));
            };
            match rx.try_recv() {
                Ok(Ok(stream)) => {
                    if let Err(e) = stream.set_nonblocking(true) {
                        return Step::Failed(ConsoleError::Io(p.addr.clone(), e));
                    }
                    p.stream = Some(stream);
                    p.rx = None;
                    p.phase = Phase::Writing;
                    Step::Continue
                }
                Ok(Err(e)) => Step::Failed(ConsoleError::Connect(p.addr.clone(), e)),
                Err(TryRecvError::Empty) => Step::Blocked,
                Err(TryRecvError::Disconnected) => Step::Failed(io_err(
                    &p.addr,
                    "connect worker exited without handing over a socket",
                )),
            }
        }
        Phase::Writing => {
            let Some(stream) = p.stream.as_mut() else {
                return Step::Failed(io_err(&p.addr, "stream missing in writing phase"));
            };
            let rest = p.spec.raw.get(p.sent..).unwrap_or(&[]);
            if rest.is_empty() {
                p.phase = Phase::Reading;
                return Step::Continue;
            }
            match stream.write(rest) {
                Ok(0) => Step::Failed(io_err(&p.addr, "write returned 0")),
                Ok(n) => {
                    let total = p.spec.raw.len();
                    if note_written(&mut p.sent, n, total) {
                        p.phase = Phase::Reading;
                    }
                    Step::Continue
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => Step::Blocked,
                Err(e) if e.kind() == ErrorKind::Interrupted => Step::Continue,
                Err(e) => Step::Failed(ConsoleError::Io(p.addr.clone(), e)),
            }
        }
        Phase::Reading => match read_step(p) {
            Ok(true) => Step::Finished,
            Ok(false) => Step::Blocked,
            Err(e) => Step::Failed(e),
        },
    }
}

/// **写阶段记账**（纯函数，可脱离 socket 单测）：把本次写出的 `n` 字节累加进 `sent`，
/// 返回**是否已把全部 `total` 字节写完**。
///
/// 存在的理由（B3-1 评审**重要 1**）：`TcpStream::write` 允许**部分写**（返回 `0 < n < 剩余`），
/// 此时必须**累加**而不能"直接赋值"，否则请求报文会被**静默截断**（对端读到半截请求）。
/// 而本机（Windows 回环）内核发送缓冲一次吞下整块，实测 **6 MB body 仍 `write_steps = 1`**
/// ⇒ **无法在真 socket 上触发部分写**，那段记账逻辑此前**零有效覆盖**（把 `saturating_add`
/// 改成直接赋值，全套测试仍全绿）。故把记账抽成纯函数，用**纯逻辑**用例覆盖部分写 / 恰好写完 /
/// 一次写完三种形态。
///
/// `saturating_add` 兜住"内核报出比剩余更多的字节数"这种不可能情形（不 panic、不绕回）。
fn note_written(sent: &mut usize, n: usize, total: usize) -> bool {
    *sent = sent.saturating_add(n);
    *sent >= total
}

/// 非阻塞读一步：`Ok(true)` = 体已读全；`Ok(false)` = 本拍暂无更多数据。
fn read_step(p: &mut Pending) -> ConsoleResult<bool> {
    loop {
        let Some(stream) = p.stream.as_mut() else {
            return Err(io_err(&p.addr, "stream missing in reading phase"));
        };
        let mut buf = [0u8; 4096];
        match stream.read(&mut buf) {
            Ok(0) => {
                // EOF：有长度头却没读够 ⇒ 截断（响亮失败）；否则即「读到 EOF 作 body」（§3.1 同口径）。
                if !p.head_done {
                    // 已收到字节却仍无 `\r\n\r\n` ⇒ 报告实收字节数（对端**回过**东西），
                    // 与"对端静默后断开"（`received 0 byte(s)`）在文案上可区分。
                    return Err(io_err(
                        &p.addr,
                        &format!(
                            "connection closed before response header completed \
                             ({} byte(s) received, no CRLFCRLF terminator)",
                            p.head.len()
                        ),
                    ));
                }
                if let Some(n) = p.content_length {
                    if p.body.len() < n {
                        return Err(io_err(
                            &p.addr,
                            "connection closed before Content-Length was satisfied",
                        ));
                    }
                }
                return Ok(true);
            }
            Ok(n) => {
                let chunk = buf.get(..n).unwrap_or(&buf);
                if p.head_done {
                    p.body.extend_from_slice(chunk);
                    if p.body.len() > MAX_BODY_BYTES {
                        return Err(ConsoleError::BodyTooLarge(p.addr.clone(), p.body.len()));
                    }
                    if let Some(len) = p.content_length {
                        if p.body.len() >= len {
                            p.body.truncate(len);
                            return Ok(true);
                        }
                    }
                } else {
                    p.head.extend_from_slice(chunk);
                    match p.head.windows(4).position(|w| w == b"\r\n\r\n") {
                        Some(pos) => {
                            if pos > MAX_HEAD_BYTES {
                                return Err(ConsoleError::HeadTooLarge(p.addr.clone(), pos));
                            }
                            parse_head(p, pos)?;
                            p.head_done = true;
                            let rest = p
                                .head
                                .get(pos + 4..)
                                .map(<[u8]>::to_vec)
                                .unwrap_or_default();
                            p.head.clear();
                            p.body.extend_from_slice(&rest);
                            if p.body.len() > MAX_BODY_BYTES {
                                return Err(ConsoleError::BodyTooLarge(
                                    p.addr.clone(),
                                    p.body.len(),
                                ));
                            }
                            if let Some(len) = p.content_length {
                                if p.body.len() >= len {
                                    p.body.truncate(len);
                                    return Ok(true);
                                }
                            }
                        }
                        None => {
                            if p.head.len() > MAX_HEAD_BYTES {
                                return Err(ConsoleError::HeadTooLarge(
                                    p.addr.clone(),
                                    p.head.len(),
                                ));
                            }
                        }
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(ConsoleError::Io(p.addr.clone(), e)),
        }
    }
}

/// 解析状态行 + 头（`pos` = `\r\n\r\n` 起始下标）。
fn parse_head(p: &mut Pending, pos: usize) -> ConsoleResult<()> {
    let text = String::from_utf8_lossy(p.head.get(..pos).unwrap_or(&[])).into_owned();
    let (status_line, rest) = text.split_once("\r\n").unwrap_or((text.as_str(), ""));
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    if status != 200 {
        return Err(ConsoleError::HttpStatus(status, p.addr.clone()));
    }
    for line in rest.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if k.eq_ignore_ascii_case("content-length") {
            let n = v.parse::<usize>().map_err(|_| {
                io_err(&p.addr, &format!("bad Content-Length header `{v}`"))
            })?;
            if n > MAX_BODY_BYTES {
                return Err(ConsoleError::BodyTooLarge(p.addr.clone(), n));
            }
            p.content_length = Some(n);
        } else if k.eq_ignore_ascii_case("transfer-encoding")
            && v.to_ascii_lowercase().contains("chunked")
        {
            return Err(ConsoleError::ChunkedUnsupported(p.addr.clone()));
        }
    }
    Ok(())
}

/// 截止到期的收口分支（B3-1 评审建议 4）：**区分**「对端一个字节都没回」与
/// 「对端回了字节，但响应头里始终凑不出 `\r\n\r\n`」。
///
/// 后者此前被报成 [`ConsoleError::Timeout`]，让排障指向"对端没回包"——而实测对端明明回了
/// 字节（形态是 **LF-only** 行结束符）⇒ **根因误导**。`p.head` 只在 `head_done` 前非空
/// （`parse_head` 之后立刻 `clear`），故判据 `!head_done && head.len() > 0` 精确对应
/// "收到了字节但没看到终止符"。
fn timeout_or_head_incomplete(p: &Pending) -> ConsoleError {
    if !p.head_done && !p.head.is_empty() {
        ConsoleError::HeadIncomplete(p.addr.clone(), p.head.len())
    } else {
        ConsoleError::Timeout(p.addr.clone())
    }
}

/// 造一条 `InvalidData` 的 IO 错误（本模块不 panic，一律走 `Err`）。
fn io_err(addr: &str, msg: &str) -> ConsoleError {
    ConsoleError::Io(
        addr.to_string(),
        std::io::Error::new(ErrorKind::InvalidData, msg.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    // ═══════════════════════════════════════════════════════════════════════
    // 桩服务端：本机真实 `TcpListener`（全平台可跑 —— 不依赖 evdev / poll(2) / fb0）
    // ═══════════════════════════════════════════════════════════════════════

    /// HTTP 桩：对第 `i`（0 基）个连接调 `reply(i, 请求报文)`；返回 `None` ⇒ **静默**
    /// （收下请求不回包、连接保持打开 —— 用于超时 / 不阻塞 / 重试用例）。
    /// 所有收到的请求报文（head + body）按序留档，供「发出去的到底是什么」类断言。
    struct Stub {
        base_url: String,
        requests: Arc<Mutex<Vec<String>>>,
        accepted: Arc<AtomicUsize>,
    }

    impl Stub {
        fn spawn(reply: impl Fn(usize, &str) -> Option<String> + Send + 'static) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stub");
            let port = listener.local_addr().expect("local_addr").port();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let accepted = Arc::new(AtomicUsize::new(0));
            let (reqs, acc) = (Arc::clone(&requests), Arc::clone(&accepted));
            std::thread::spawn(move || {
                // 静默连接**持有**到进程结束（对端 read 恒为 WouldBlock）。
                let mut held: Vec<std::net::TcpStream> = Vec::new();
                for (i, sock) in listener.incoming().enumerate() {
                    let Ok(mut sock) = sock else { break };
                    acc.fetch_add(1, Ordering::SeqCst);
                    let req = read_request(&mut sock).unwrap_or_default();
                    if let Ok(mut v) = reqs.lock() {
                        v.push(req.clone());
                    }
                    match reply(i, &req) {
                        Some(resp) => {
                            let _ = sock.write_all(resp.as_bytes());
                        }
                        None => held.push(sock),
                    }
                }
            });
            Self {
                base_url: format!("http://127.0.0.1:{port}"),
                requests,
                accepted,
            }
        }

        /// **写出原始字节后保持连接不关**（B3-1 评审建议 4 用：模拟"对端回了字节，但头部
        /// 永远凑不出 `\r\n\r\n`"—— LF-only / 头截断且对端不关连接）。返回 `None` 即静默。
        fn spawn_raw_hold(reply: impl Fn(usize, &str) -> Option<Vec<u8>> + Send + 'static) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stub");
            let port = listener.local_addr().expect("local_addr").port();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let accepted = Arc::new(AtomicUsize::new(0));
            let (reqs, acc) = (Arc::clone(&requests), Arc::clone(&accepted));
            std::thread::spawn(move || {
                let mut held: Vec<std::net::TcpStream> = Vec::new();
                for (i, sock) in listener.incoming().enumerate() {
                    let Ok(mut sock) = sock else { break };
                    acc.fetch_add(1, Ordering::SeqCst);
                    let req = read_request(&mut sock).unwrap_or_default();
                    if let Ok(mut v) = reqs.lock() {
                        v.push(req.clone());
                    }
                    match reply(i, &req) {
                        Some(bytes) => {
                            let _ = sock.write_all(&bytes);
                            held.push(sock);
                        }
                        None => held.push(sock),
                    }
                }
            });
            Self {
                base_url: format!("http://127.0.0.1:{port}"),
                requests,
                accepted,
            }
        }

        /// 按脚本回包（`None` = 静默）。
        fn scripted(script: Vec<Option<String>>) -> Self {
            Self::spawn(move |i, _req| script.get(i).cloned().flatten())
        }

        /// **回显式**：把请求里的 `request_id` 回填进回执（真实服务端行为），可指定 `duplicate`。
        fn echoing(duplicate: bool, code: &'static str, ok: bool) -> Self {
            Self::spawn(move |_i, req| {
                let rid = extract_request_id(req).unwrap_or_default();
                Some(http_ok(&response_json(&rid, ok, code, duplicate, "null")))
            })
        }

        fn request(&self, idx: usize) -> String {
            self.requests
                .lock()
                .expect("stub lock")
                .get(idx)
                .cloned()
                .unwrap_or_default()
        }

        fn request_count(&self) -> usize {
            self.requests.lock().expect("stub lock").len()
        }

        fn accepted(&self) -> usize {
            self.accepted.load(Ordering::SeqCst)
        }
    }

    /// 阻塞读一个 HTTP 请求（head 到 `\r\n\r\n`，再按 `Content-Length` 读 body）。
    ///
    /// 读超时用 [`HANG_GUARD`]（30 s）而非旧的 5 s（B3-1 评审重要 3 的同类项）：这是**桩侧**的
    /// 挂起护栏，语义同样是"永不完成才失败"。旧值 5 s 与 `drive` 侧预算同量级，机器一被压住就可能
    /// 先由**桩**放弃读 ⇒ 请求留档为空 ⇒ 表现为 `RequestIdMismatch` 这类**误导性**失败（而非
    /// "超时"），排障成本高。正常路径上数据一到就返回，抬高上界不改变任何时序。
    fn read_request(sock: &mut TcpStream) -> Option<String> {
        sock.set_read_timeout(Some(HANG_GUARD)).ok()?;
        let mut buf = Vec::new();
        let mut one = [0u8; 1];
        loop {
            match sock.read(&mut one) {
                Ok(0) => break,
                Ok(_) => {
                    buf.push(one[0]);
                    if buf.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                Err(_) => return None,
            }
        }
        let head = String::from_utf8_lossy(&buf).into_owned();
        let len = head
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        if len > 0 && sock.read_exact(&mut body).is_err() {
            return None;
        }
        Some(format!("{head}{}", String::from_utf8_lossy(&body)))
    }

    /// 带 `Content-Length` 的 200 响应。
    fn http_ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// 无 body 的指定状态码响应。
    fn http_status(status: u16) -> String {
        format!("HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    /// 回执 JSON（`field_errors` / `duplicate` 是**必需字段**，不得省）。
    fn response_json(
        request_id: &str,
        ok: bool,
        code: &str,
        duplicate: bool,
        applied: &str,
    ) -> String {
        format!(
            "{{\"request_id\":\"{request_id}\",\"ok\":{ok},\"code\":\"{code}\",\"message\":\"M\",\
             \"applied\":{applied},\"field_errors\":[],\"audit_id\":\"aud-1\",\
             \"duplicate\":{duplicate},\"at_ms\":1757412000000}}"
        )
    }

    /// 从请求报文里取出信封 `request_id`（最小解析，仅测试用）。
    fn extract_request_id(req: &str) -> Option<String> {
        let body = req.split_once("\r\n\r\n")?.1;
        let v: serde_json::Value = serde_json::from_str(body).ok()?;
        v.get("request_id")?.as_str().map(str::to_string)
    }

    /// 测试时钟：`wall_ms` 固定；`start` 取真实 `Instant`（超时链路走真实单调时钟，
    /// 但**只有 `tick(now)` 推进它** —— 测试自身不 sleep、不等待）。
    fn clock() -> ConsoleClock {
        ConsoleClock {
            wall_ms: 1_757_412_000_000,
            start: Instant::now(),
        }
    }

    /// **挂起护栏预算 = 30 s**（B3-1 评审**重要 3**）。
    ///
    /// 语义是"**永不完成**才失败"，**不是**"快不快" —— 因此这个数只该"大到不可能被负载耗尽"。
    /// 上一版用 3 s：主控在重编译后紧接着跑全量时**实测到 1 次 FAILED**
    /// （`327 passed; 1 failed`，单跑 8/8 通过、全量 4 次通过）⇒ 偶发、负载敏感。3 s 对这种
    /// "本机回环 + 一次性工作线程"的路径**没有安全裕度**；30 s 对正常的毫秒级路径有 ~10000× 裕度，
    /// 而真正的挂起（死锁 / 无界等待）仍会在 30 s 被抓住。
    ///
    /// ⚠️ 不要把本常量拿去当"性能阈值"用 —— 判别"tick 不阻塞"的是下面 `NEVER_BLOCK_BUDGET`。
    const HANG_GUARD: Duration = Duration::from_secs(30);

    /// **"`tick` 不阻塞"的判别预算 = 1 s / 200 拍**（B3-1 评审**重要 3**）。
    ///
    /// **判别什么失败模式**：`tick` 内出现**同步等待**（把 `rx.try_recv()` 写成 `rx.recv()`、
    /// 把 `WouldBlock` 分支写成"重试到有数据"、或内联做阻塞 `connect`）。
    ///
    /// **该模式会花多久**：对端静默且保持连接时，阻塞读**永不返回**（挂死 ⇒ harness 超时 = 红）；
    /// 若被写成"自旋到截止"，则**第一拍**就会耗掉整个 [`CONTROL_TIMEOUT`]（5 s）⇒ 第一拍即 ≥ 5 s。
    ///
    /// **裕度**：真实实现 200 拍为**微秒级**；失败形态**≥ 1 s**（挂死则 ∞）。故 1 s 阈值对
    /// 正常路径有 **10⁵ 倍以上**裕度、对失败形态有 **≥ 5 倍**裕度。上一版阈值 300 ms 只能抓到
    /// "每拍 ≥ 1.5 ms 的等待"，**裕度过薄**（负载一抖就可能假红，且对"每拍 1 ms 阻塞"这类
    /// 真缺陷可能漏判）。
    const NEVER_BLOCK_BUDGET: Duration = Duration::from_secs(1);

    /// 推进到结束（预算内未完成即失败）。
    ///
    /// ⚠️ **让出必须"混合"**（B3-2a 质量整改同步，`channel.rs` 同款先例）：纯 `yield_now()` 只把
    /// 本线程放回**同优先级**就绪队尾 —— Windows 上（`SwitchToThread`）可能**整趟都不切到连接
    /// 工作线程** ⇒ socket 迟迟不交回 ⇒ 本函数的**挂起护栏**在重载机器上偶发误红（主控实测过一次、
    /// 整改者也复现过一次）。故每 8 拍插一次 `sleep(1ms)`：给出真正的调度机会，同时保持整体推进够快。
    fn drive<T: DeserializeOwned>(
        c: &mut ConsoleClient,
        budget: Duration,
    ) -> ConsoleResult<ConsoleOutcome<T>> {
        let t0 = Instant::now();
        let mut spins: u32 = 0;
        loop {
            match c.tick::<T>(Instant::now()) {
                Progress::Done(r) => return r,
                Progress::Pending => {}
            }
            assert!(
                t0.elapsed() < budget,
                "预算 {budget:?} 内未完成（当前阶段 {}）",
                c.phase_name()
            );
            let_worker_run(&mut spins);
        }
    }

    /// 推进到谓词成立（同上）。
    fn drive_until(c: &mut ConsoleClient, pred: impl Fn(&ConsoleClient) -> bool, budget: Duration) {
        let t0 = Instant::now();
        let mut spins: u32 = 0;
        while !pred(c) {
            let _ = c.tick::<serde_json::Value>(Instant::now());
            assert!(t0.elapsed() < budget, "预算 {budget:?} 内未到达目标阶段");
            let_worker_run(&mut spins);
        }
    }

    /// 每 8 拍让出一次**真调度**（见 [`drive`] 的说明）。抽出来只为两处共用同一节奏。
    fn let_worker_run(spins: &mut u32) {
        *spins = spins.wrapping_add(1);
        if *spins % 8 == 0 {
            std::thread::sleep(Duration::from_millis(1));
        } else {
            std::thread::yield_now();
        }
    }

    /// 推进到本拍结束（用于「已连上后等超时」的场景），返回结果。
    fn drive_to_done(
        c: &mut ConsoleClient,
        budget: Duration,
    ) -> ConsoleResult<ConsoleOutcome<serde_json::Value>> {
        drive::<serde_json::Value>(c, budget)
    }

    fn leaked_port() -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    }

    /// 一个可被 `ControlRequest` 接受的最小载荷（联锁写操作）。
    fn payload() -> serde_json::Value {
        serde_json::json!({"observed_latched": false, "observed_sources": []})
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ① 操作名封闭清单（§3.4）
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn write_ops_are_exactly_the_three_write_endpoints() {
        let mut ops: Vec<&str> = write_ops().collect();
        ops.sort_unstable();
        assert_eq!(
            ops,
            vec!["ack_m1", "apply", "release"],
            "写操作清单必须与 §3.4 的 3 个写端点一一对应（唯一真源 = 端点路径末段）"
        );
        assert_eq!(ops.len(), 3, "另 5 个查询端点无信封 op");
        for known in ["apply", "release", "ack_m1"] {
            assert!(
                endpoint_for_op(known).is_some(),
                "`{known}` 必须在封闭清单内"
            );
        }
        for bogus in ["delete_all", "Apply", "", "config", "logs", "apply "] {
            assert!(
                endpoint_for_op(bogus).is_none(),
                "`{bogus}` 不得在封闭清单内"
            );
        }
    }

    #[test]
    fn endpoint_for_op_maps_to_write_endpoints_only() {
        assert_eq!(endpoint_for_op("apply"), Some(ConsoleEndpoint::ConfigApply));
        assert_eq!(
            endpoint_for_op("release"),
            Some(ConsoleEndpoint::InterlockRelease)
        );
        assert_eq!(
            endpoint_for_op("ack_m1"),
            Some(ConsoleEndpoint::InterlockAckM1)
        );
        assert_eq!(endpoint_for_op("logs"), None, "查询端点无信封 op");
    }

    /// **非清单内的 `op` 必须响亮失败**，且**不得**发出任何请求（未确认 = 无网络动作）。
    ///
    /// **改什么会让本条变红**：把 [`endpoint_for_op`] 的 `find` 换成"兜底取第一个写端点"
    /// （或把 `UnknownOp` 分支改成默认 `release`）⇒ 本条第二条断言（`stub.accepted() == 0`）红。
    #[test]
    fn begin_write_rejects_unknown_op_loudly_and_sends_nothing() {
        let stub = Stub::scripted(vec![]);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        let err = c
            .begin_write("delete_all", &payload(), clock())
            .expect_err("非法 op 必须 Err");
        assert!(
            matches!(&err, ConsoleError::UnknownOp(op) if op == "delete_all"),
            "got {err:?}"
        );
        assert!(!c.is_busy(), "被拒的请求不得进入在途状态");
        assert_eq!(stub.accepted(), 0, "被拒的 op **一个包都不该发出去**");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ② 信封构造：uuid v4 / issued_at_ms / op / 路径 / Content-Length
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn write_request_carries_uuid_v4_and_injected_issued_at() {
        let stub = Stub::scripted(vec![Some(http_ok(&response_json(
            "whatever", true, "ok", false, "null",
        )))]);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        let id = c
            .begin_write("release", &serde_json::json!({"a": 1}), clock())
            .expect("begin");

        // uuid v4 形状（版本号 / 长度 / 连字符）—— `request_id` 是幂等键兼防重放凭据
        let parsed = uuid::Uuid::parse_str(&id).expect("必须是合法 UUID");
        assert_eq!(parsed.get_version_num(), 4, "必须 v4");
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);

        let _ = drive_to_done(&mut c, HANG_GUARD);
        let req = stub.request(0);
        assert!(
            req.starts_with("POST /v1/console/interlock/release HTTP/1.1\r\n"),
            "{req}"
        );
        assert!(req.contains("Content-Type: application/json\r\n"), "{req}");
        assert!(req.contains(&format!("\"request_id\":\"{id}\"")), "{req}");
        assert!(req.contains("\"issued_at_ms\":1757412000000"), "{req}");
        assert!(req.contains("\"op\":\"release\""), "{req}");
        assert!(req.contains("\"payload\":{\"a\":1}"), "{req}");
        // Content-Length 必须等于实际 body 字节数（不等 ⇒ 服务端会挂住 / 截断）
        let (head, body) = req.split_once("\r\n\r\n").expect("head/body");
        let len = head
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .expect("Content-Length");
        assert_eq!(len, body.len(), "Content-Length 必须等于实际 body 字节数");

        // 每次生成的都是**新** id（弱 id / 复用会把两次不同操作判成同一请求 ⇒ 静默丢操作）
        let mut c2 = ConsoleClient::new(&stub.base_url).expect("client");
        let id2 = c2.begin_write("release", &payload(), clock()).expect("begin");
        assert_ne!(id, id2, "两个请求不得共用同一 `request_id`");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ③ 三阶段推进（真实 TCP）
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn phases_advance_from_connecting_to_done() {
        let stub = Stub::echoing(false, "ok", true);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_write("release", &payload(), clock()).expect("begin");
        assert_eq!(c.phase_name(), "connecting", "首拍前处于连接阶段");
        assert!(c.is_busy());
        drive_until(&mut c, |c| c.phase_name() == "reading", HANG_GUARD);
        assert_eq!(c.phase_name(), "reading", "写完之后进入读阶段");
        let out = drive_to_done(&mut c, HANG_GUARD).expect("ok");
        assert_eq!(c.phase_name(), "idle", "结束后回到空闲");
        assert!(!c.is_busy());
        let resp = out.response().expect("写操作必是回执信封");
        assert!(resp.ok);
        assert_eq!(resp.request_id, c_request_id(&stub, 0));
        assert_eq!(c.fail_streak(), 0, "成功必须清零失败计数");
    }

    fn c_request_id(stub: &Stub, idx: usize) -> String {
        extract_request_id(&stub.request(idx)).unwrap_or_default()
    }

    /// `duplicate=true` **原样上抛**（幂等命中不是错误）。
    #[test]
    fn duplicate_true_is_passed_through_not_treated_as_error() {
        for duplicate in [false, true] {
            let stub = Stub::echoing(duplicate, "ok", true);
            let mut c = ConsoleClient::new(&stub.base_url).expect("client");
            let id = c.begin_write("ack_m1", &payload(), clock()).expect("begin");
            let out = drive_to_done(&mut c, HANG_GUARD).expect("必须 Ok");
            assert_eq!(out.endpoint, ConsoleEndpoint::InterlockAckM1);
            let resp = out.response().expect("回执");
            assert!(resp.ok);
            assert_eq!(resp.code, mupc_display_proto::ControlCode::Ok);
            assert_eq!(resp.request_id, id);
            assert_eq!(resp.duplicate, duplicate, "`duplicate` 必须原样上抛");
            assert_eq!(resp.audit_id.as_deref(), Some("aud-1"), "审计 id 两种结果都返回");
        }
    }

    #[test]
    fn audit_unavailable_is_decoded_as_rejection_without_applied() {
        let stub = Stub::echoing(false, "audit_unavailable", false);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_write("apply", &serde_json::json!({}), clock()).expect("begin");
        let out = drive_to_done(&mut c, HANG_GUARD).expect("HTTP 200 ⇒ 传输层 Ok");
        let resp = out.response().expect("回执");
        assert!(!resp.ok, "审计不可写 ⇒ ok=false");
        assert_eq!(resp.code, mupc_display_proto::ControlCode::AuditUnavailable);
        assert!(resp.code.is_rejection());
        assert_eq!(resp.applied, None, "fail-closed ⇒ 操作未生效");
    }

    /// 回执 `request_id` 与在途请求不符 ⇒ **响亮失败**（不得静默当成本次结果）。
    #[test]
    fn mismatched_request_id_is_a_loud_error() {
        let stub = Stub::scripted(vec![Some(http_ok(&response_json(
            "00000000-0000-4000-8000-000000000000",
            true,
            "ok",
            false,
            "null",
        )))]);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_write("release", &payload(), clock()).expect("begin");
        let err = drive_to_done(&mut c, HANG_GUARD).expect_err("错位必须 Err");
        assert!(
            matches!(err, ConsoleError::RequestIdMismatch { .. }),
            "got {err:?}"
        );
        assert!(!c.is_busy(), "出错后不得残留在途状态");
        assert_eq!(c.fail_streak(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ④ 查询（GET；裸载荷，§3.4）
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn query_round_trip_uses_get_and_parses_bare_payload() {
        let stub = Stub::scripted(vec![Some(http_ok(r#"["gateway","intercore"]"#))]);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        let q = encode_query(&[("levels", "error"), ("limit", "200")]);
        c.begin_query(ConsoleEndpoint::Logs, &q, clock())
            .expect("begin");
        let out = drive::<Vec<String>>(&mut c, HANG_GUARD).expect("ok");
        assert_eq!(out.endpoint, ConsoleEndpoint::Logs);
        assert_eq!(
            out.query().expect("查询是裸载荷"),
            &vec!["gateway".to_string(), "intercore".to_string()]
        );
        assert!(out.response().is_none(), "查询结果没有回执信封");
        let req = stub.request(0);
        assert!(
            req.starts_with("GET /v1/console/logs?levels=error&limit=200 HTTP/1.1\r\n"),
            "{req}"
        );
        assert!(!req.contains("Content-Length"), "GET 不带 body：{req}");
    }

    #[test]
    fn query_rejects_injection_and_write_endpoints() {
        let mut c = ConsoleClient::new(DEFAULT_CONSOLE_BASE_URL).expect("client");
        for bad in ["a=1\r\nX: y", "a=1\n", "a b", "a=1#frag", "a=\u{7f}"] {
            let err = c
                .begin_query(ConsoleEndpoint::Logs, bad, clock())
                .expect_err("非法查询串必须 Err");
            assert!(matches!(err, ConsoleError::BadQuery(_)), "got {err:?}");
        }
        let err = c
            .begin_query(ConsoleEndpoint::ConfigApply, "", clock())
            .expect_err("写端点不得走查询");
        assert!(
            matches!(err, ConsoleError::NotAQueryEndpoint(_)),
            "got {err:?}"
        );
        // 查询端点名不在写操作清单内 ⇒ 以 `UnknownOp` 被挡（防误路由）
        let err = c
            .begin_write("config", &serde_json::json!({}), clock())
            .expect_err("x");
        assert!(matches!(err, ConsoleError::UnknownOp(_)), "got {err:?}");
    }

    #[test]
    fn encode_query_percent_encodes_reserved_bytes() {
        assert_eq!(encode_query(&[]), "");
        assert_eq!(encode_query(&[("a", "1")]), "a=1");
        assert_eq!(
            encode_query(&[("levels", "error"), ("levels", "warn")]),
            "levels=error&levels=warn",
            "多值维度必须重复键（§3.4 的 levels/targets 均为多值）"
        );
        assert_eq!(
            encode_query(&[("targets", "核间/gateway")]),
            "targets=%E6%A0%B8%E9%97%B4%2Fgateway"
        );
        assert_eq!(encode_query(&[("t", "a b&c=d")]), "t=a%20b%26c%3Dd");
        // 构造性闭环：编码产物必然能过 `begin_query` 的注入校验
        let mut c = ConsoleClient::new(DEFAULT_CONSOLE_BASE_URL).expect("client");
        let q = encode_query(&[("levels", "error warn"), ("from", "1757412000000")]);
        c.begin_query(ConsoleEndpoint::Logs, &q, clock())
            .expect("编码串必须合法");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ⑤ 错误分类（Connect / Timeout / Io / HttpStatus / Json）
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn http_error_statuses_are_classified() {
        for status in [404u16, 500, 503] {
            let stub = Stub::scripted(vec![Some(http_status(status))]);
            let mut c = ConsoleClient::new(&stub.base_url).expect("client");
            c.begin_write("apply", &serde_json::json!({}), clock())
                .expect("begin");
            let err = drive_to_done(&mut c, HANG_GUARD).expect_err("err");
            assert!(
                matches!(err, ConsoleError::HttpStatus(s, _) if s == status),
                "got {err:?}"
            );
            assert_eq!(c.fail_streak(), 1, "HTTP 错误计一次失败");
        }
    }

    #[test]
    fn malformed_json_is_a_json_error_not_a_silent_default() {
        let stub = Stub::scripted(vec![Some(http_ok("{not json"))]);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_query(ConsoleEndpoint::AuditOps, "", clock())
            .expect("begin");
        let err = drive_to_done(&mut c, HANG_GUARD).expect_err("err");
        assert!(matches!(err, ConsoleError::Json(_, _)), "got {err:?}");
        assert_eq!(c.fail_streak(), 1, "解码失败计一次失败");
    }

    /// 回执缺**必需字段**（`duplicate`）⇒ 整帧 Err（不得静默默认成 `false`）。
    #[test]
    fn response_missing_duplicate_field_is_a_json_error() {
        let stub = Stub::spawn(|_i, req| {
            let rid = extract_request_id(req).unwrap_or_default();
            Some(http_ok(&format!(
                "{{\"request_id\":\"{rid}\",\"ok\":true,\"code\":\"ok\",\"message\":\"M\",\
                 \"applied\":null,\"field_errors\":[],\"audit_id\":null,\"at_ms\":1}}"
            )))
        });
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_write("apply", &serde_json::json!({}), clock())
            .expect("begin");
        let err = drive_to_done(&mut c, HANG_GUARD).expect_err("err");
        assert!(matches!(err, ConsoleError::Json(_, _)), "got {err:?}");
    }

    /// **确定性**写法：连 `127.0.0.1:0` ⇒ `connect` 被**当场**拒绝（Windows
    /// `WSAEADDRNOTAVAIL`、Linux `EADDRNOTAVAIL`/`ECONNREFUSED`），**不依赖**"拒绝在多快内
    /// 回包"。
    ///
    /// # 为什么改（B3-2a 规格评审 建议 6）
    /// 旧写法是「bind 一个临时端口再 drop」（[`leaked_port`]）⇒ 断言依赖 OS 在**秒级内**回
    /// RST，而连接线程的预算是**客户端截止**、线程启动又**晚于**截止起点（`clock().start`）
    /// ⇒ 一旦 `connect` 真耗满预算，`tick` 会**先按截止收口**，断言拿到的是 `Timeout` 而非
    /// `Connect` —— 结构性脆弱（评审 75 次未复现，但属"应修"）。端口 0 无此依赖，也**不与
    /// 其它用例抢端口**（那些用 [`leaked_port`] 的用例保持原样）。
    /// 同 crate 的 `channel.rs::connection_refused_is_connect_error` 已是此写法（那边有长注释）。
    #[test]
    fn connection_refused_is_a_connect_error() {
        let mut c = ConsoleClient::with_timeout("http://127.0.0.1:0", Duration::from_secs(5))
            .expect("client");
        c.begin_write("apply", &serde_json::json!({}), clock())
            .expect("begin");
        let err = drive_to_done(&mut c, HANG_GUARD).expect_err("err");
        assert!(matches!(err, ConsoleError::Connect(_, _)), "got {err:?}");
        assert_eq!(c.fail_streak(), 1);
    }

    /// **有 `Content-Length` 时的判定点是"精确"的**（上限 + 1 即拒，读之前就判）。
    /// 与 [`MAX_BODY_BYTES`] 旁"判定点 ≤ 上限 + 4096"的补注互为补充：那句说的是**无**长度头
    /// （读到 EOF 为止）的情形，那种情形按 4096 B 读块累加后判，故判定点可能略超上限。
    #[test]
    fn oversized_content_length_is_rejected_before_allocation() {
        let stub = Stub::spawn(|_i, _req| {
            Some(format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_BODY_BYTES + 1
            ))
        });
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_query(ConsoleEndpoint::Config, "", clock())
            .expect("begin");
        let err = drive_to_done(&mut c, HANG_GUARD).expect_err("err");
        assert!(
            matches!(err, ConsoleError::BodyTooLarge(_, n) if n == MAX_BODY_BYTES + 1),
            "got {err:?}"
        );
    }

    /// **响应头上限的「行为级」覆盖**（B3-1 质量整改补：替换旧 `body_and_head_limits_match_...`
    /// 时，那条用例里对 `MAX_HEAD_BYTES` 的断言被一并删除，而**行为**没有补回来 ⇒
    /// `MAX_HEAD_BYTES` 与 [`ConsoleError::HeadTooLarge`] 一度**全项目零覆盖**。
    ///
    /// 这里锁的是**行为**（超长头被拒），而不是"两个客户端的常量相等"（后者只是偶然巧合，
    /// 见 [`MAX_BODY_BYTES`] 的说明）。判据取 `n > MAX_HEAD_BYTES` 且**上界受控**
    /// （`≤ MAX_HEAD_BYTES + 读块`），把"判定粒度"也一并锁住。
    ///
    /// **改什么会让本条变红**（**实测**）：把 `parse` 里两处 `pos` / `p.head.len() > MAX_HEAD_BYTES`
    /// 的判定改成恒假 ⇒ 客户端收下超长头 ⇒ 第一条 `expect_err` 红（实测输出
    /// `超长响应头必须被拒: ConsoleOutcome { endpoint: Config, kind: Query(Object {}) }`）。
    ///
    /// ⚠️ **本条不锁"上限的具体数值"**：样本行数由 `MAX_HEAD_BYTES` **派生**（`/1024 + 8`）
    /// ⇒ 抬高上限时样本同步变大，本用例**照绿**。（我最初在此写过"抬高上限也红"—— **实测为假**，
    /// 已按实测订正。写错这条注释的正是本项目的"声称会红但不会红"缺陷类，故如实留痕。）
    /// 若将来要**锁数值**，须改用与常量**无关**的固定样本 —— 那也意味着上限变更会被本用例**故意**拦下。
    #[test]
    fn oversized_response_head_is_rejected_as_head_too_large() {
        // 每行约 1 KB，凑出明显超过 64 KiB 的头（约 72 KB）。
        let line = format!("X-Pad: {}\r\n", "A".repeat(1024));
        let lines = MAX_HEAD_BYTES / 1024 + 8;
        let stub = Stub::spawn(move |_i, _req| {
            let mut resp = String::from("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n");
            for _ in 0..lines {
                resp.push_str(&line);
            }
            resp.push_str("\r\n{}");
            Some(resp)
        });
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_query(ConsoleEndpoint::Config, "", clock())
            .expect("begin");
        let err = drive_to_done(&mut c, HANG_GUARD).expect_err("超长响应头必须被拒");
        match err {
            ConsoleError::HeadTooLarge(_, n) => {
                assert!(n > MAX_HEAD_BYTES, "报出的字节数必须超过上限：{n}");
                assert!(
                    n <= MAX_HEAD_BYTES + 4096,
                    "判定粒度应为「上限 + 一个读块」：{n} 超出 {MAX_HEAD_BYTES} + 4096"
                );
            }
            other => panic!("应为 HeadTooLarge，实得 {other:?}"),
        }
    }

    /// **回包上限必须放得下"满页日志"**（B3-1 评审**重要 2** 订正）。
    ///
    /// 本用例**替换**了旧的 `body_and_head_limits_match_the_read_channel`。替换理由：旧用例
    /// 断言的是 `MAX_BODY_BYTES == channel::MAX_BODY_BYTES`（两个客户端"同口径"），那条断言
    /// **只为"两数相等"而存在** —— 控制通道的响应体含**变长 JSON 文本**
    /// （`LogEntry.message` 无长度上限），读通道的帧结构固定、无自由文本；两者**服务不同端点、
    /// 依据不同**，数值相同是**偶然**而非约束，且它会把错误的 64 KiB 一起固化下来。
    ///
    /// 现在的断言挂在**真实契约常量**上：`LOG_PAGE_LIMIT_MAX`（`display-proto/src/log.rs`）
    /// × [`ASSUMED_MAX_MESSAGE_BYTES`]。判据与 `MAX_BODY_BYTES` 旁的编译期 `assert!` **同源**；
    /// 本运行期用例额外锁住"上限确实 ≥ 满页"这一**可读**事实（编译期那条在类型检查阶段就挡住，
    /// 编译不过时看不到具体数字）。
    ///
    /// **改什么会让本条变红**（**2026-09-15 独立复核实测订正 —— 原表述是错的**）：
    /// - 原写"把 `MAX_BODY_BYTES` 调回 64 KiB ⇒ 红"：实际**不是**本用例先响，而是上面那条
    ///   编译期 `assert!` 先 `E0080` ⇒ **本用例根本不会运行**（要观察运行期行为须**同时**放宽门禁）；
    /// - 原写"把 `ASSUMED_MAX_MESSAGE_BYTES` 调小成 200 ⇒ 红"：**假** —— 调小只会让乘积更小，
    ///   `MAX_BODY_BYTES >= 乘积` 仍成立，本用例**照绿**；
    /// - **真正会红的改法**：把 `ASSUMED_MAX_MESSAGE_BYTES` 调**大**到乘积超过 `MAX_BODY_BYTES`
    ///   （如 `2000` ⇒ 400 000 > 262 144）⇒ 编译期 `E0080`（同样先于本用例）。
    ///   ⇒ 本用例的**运行期**判别力其实很弱，它锁的是"上限 ≥ 满页"这一**可读事实**；
    ///   **真正的门禁是编译期那条**。如实登记，不再声称更强的敏感性。
    #[test]
    fn body_limit_fits_a_full_log_page_with_the_assumed_message_cap() {
        let need = mupc_display_proto::log::LOG_PAGE_LIMIT_MAX * ASSUMED_MAX_MESSAGE_BYTES;
        assert_eq!(
            mupc_display_proto::log::LOG_PAGE_LIMIT_MAX,
            200,
            "契约常量本身若变，本条与 `MAX_BODY_BYTES` 的推算基础一起复核"
        );
        assert!(
            MAX_BODY_BYTES >= need,
            "控制通道响应体上限 {MAX_BODY_BYTES} 放不下一个满页日志（{need} = 200 条 × \
             {ASSUMED_MAX_MESSAGE_BYTES} B）—— 合法回包会被判 BodyTooLarge"
        );
        // 头上限（64 KiB）是**另一件事**：请求/响应头行数有限、无自由文本，按头长推算即可；
        // 与读通道数值相同是**巧合**，不构成约束（旧用例的"同口径"理由已作废）。
        // 该值不再单列运行期断言 —— 编译期常量比较会被 `clippy::assertions_on_constants` 判为
        // 恒真门禁（本项目明令禁止的伪门禁形态），如实删去比压住 lint 更诚实。
    }

    #[test]
    fn chunked_transfer_encoding_fails_loudly() {
        let stub = Stub::spawn(|_i, _req| {
            Some("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".into())
        });
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_query(ConsoleEndpoint::Config, "", clock())
            .expect("begin");
        let err = drive_to_done(&mut c, HANG_GUARD).expect_err("err");
        assert!(
            matches!(err, ConsoleError::ChunkedUnsupported(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn bad_base_url_is_rejected_without_panic() {
        assert!(matches!(
            ConsoleClient::new("https://127.0.0.1:9811").expect_err("非 http 必须 Err"),
            ConsoleError::BadUrl(_, _)
        ));
        assert!(matches!(
            ConsoleClient::new("unix:///tmp/x").expect_err("非法 scheme"),
            ConsoleError::BadUrl(_, _)
        ));
        let c = ConsoleClient::default();
        assert_eq!(c.addr(), "127.0.0.1:9811");
        assert_eq!(c.timeout(), CONTROL_TIMEOUT, "生产超时恒为 5 s（设计 §5.5）");
    }

    /// **host 必须是 IP 字面量**（设计 §3.3「host 为字面量、不做 DNS」）。
    ///
    /// 非字面量会让 [`connect`] 退化为**无界**阻塞 `TcpStream::connect`（工作线程可超出 5 s
    /// 截止存活），而该"平时不可达"分支实际可达（`http://localhost:9811` 会被 URL 解析器接受）
    /// ⇒ 在**入口**响亮拒绝（B3-1 评审重要 4）。
    ///
    /// **改什么会让本条变红**：删掉 `ConsoleClient::new` 里的 `SocketAddr` 解析校验 ⇒
    /// 前四条断言拿到 `Ok` ⇒ 红。
    #[test]
    fn non_ip_literal_host_is_rejected_at_construction() {
        for bad in [
            "http://localhost:9811",
            "http://mupcd:9811",
            "http://example.com",
            "http://1.2.3.4]:9811",
            // 裸 IPv6（无方括号）：RFC 3986 的 authority 里 IPv6 必须带方括号，否则与端口分隔符
            // 歧义 ⇒ `addr()` 得到 `"::1:9811"`，解析不成 `SocketAddr` ⇒ 会被 connect 路径退化。
            // **独立复核 B4 实测点名**：上一版把这种写法当作"必须可用"，把漏洞固化成受保护的期望。
            "http://::1:9811",
        ] {
            let err = ConsoleClient::new(bad).expect_err("非 IP 字面量必须 Err");
            assert!(
                matches!(&err, ConsoleError::BadUrl(url, reason) if url == bad && reason.contains("SocketAddr")),
                "`{bad}` got {err:?}"
            );
        }
        // IPv4 / **带方括号**的 IPv6 必须仍然可用
        for good in ["http://127.0.0.1:9811", "http://[::1]:9811"] {
            let c = ConsoleClient::new(good).unwrap_or_else(|e| panic!("`{good}` 必须可用: {e}"));
            assert!(!c.addr().is_empty());
        }
        // ── **构造即保证的不变量**（复核 B4 要求收紧；B3-1 评审建议 8 上收到类型层）：
        // **每个**成功建出的客户端，其 `addr()` 都必须能解析为 `SocketAddr`。
        // 现在这已由构造**直接**保证 —— 结构体存的就是 `SocketAddr`，`addr()` 只是
        // `to_string()`（`connect` 也直接吃它）⇒ 不存在"两处 `format!` 必须一致"的隐式契约，
        // 也不再有需要靠入口校验去保证不可达的"无界兜底分支"。
        // **改什么会让本条变红**：把 `new()` 里的 `parse::<SocketAddr>()` 换成"剥方括号后
        // 是不是 `IpAddr`"这类弱判据 ⇒ `http://::1:9811` 会被建出来 ⇒ 前面的 `expect_err` 先红。
        for good in ["http://127.0.0.1:9811", "http://[::1]:9811"] {
            let c = ConsoleClient::new(good).expect("client");
            assert!(
                c.addr().parse::<SocketAddr>().is_ok(),
                "入口通过 ⇒ `addr()` 必可解析：{}",
                c.addr()
            );
        }
        let v6 = ConsoleClient::new("http://[::1]:9811").expect("client");
        assert_eq!(v6.addr(), "[::1]:9811");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ⑥ 超时（`Instant` 强制）与「tick 不阻塞」
    // ═══════════════════════════════════════════════════════════════════════

    /// 5 s 截止到期 ⇒ **当场 abort**，记一次失败，且不重复产出。
    ///
    /// **改什么会让本条变红**（实测口径）：删掉 `tick` 的 `for` **循环内**那处
    /// `if now >= p.deadline` 判定 ⇒ 本用例的 `loop` 在预算内拿不到 `Done` ⇒
    /// 断言 `Instant::now() < budget` 红。
    ///
    /// ⚠️ **曾有的不实声称已订正（B3-1 评审重要 3）**：本注释此前点名"tick 顶部截止判定"，
    /// 但那是**同一判据在循环内的重复**（行为等价）—— **单独删它仍是绿的**。该重复已删除，
    /// 截止判定现在**只有循环内那一处**。
    #[test]
    fn deadline_expiry_aborts_inflight_and_records_one_failure() {
        let stub = Stub::scripted(vec![None]); // 收下请求后静默（永不回包）
        let mut c = ConsoleClient::with_timeout(&stub.base_url, Duration::from_millis(120))
            .expect("client");
        c.begin_write("release", &payload(), clock()).expect("begin");
        // 先推进到「已连上、正在等响应」——证明超时**不是**因为连不上。
        drive_until(
            &mut c,
            |c| c.phase_name() == "reading",
            HANG_GUARD,
        );
        let budget = Instant::now() + HANG_GUARD;
        let out = loop {
            match c.tick::<serde_json::Value>(Instant::now()) {
                Progress::Done(r) => break r,
                Progress::Pending => {}
            }
            assert!(Instant::now() < budget, "超时未被强制（5 s 截止失效）");
            std::thread::yield_now();
        };
        assert!(
            matches!(out, Err(ConsoleError::Timeout(_))),
            "got {out:?}"
        );
        assert!(!c.is_busy(), "超时后立刻作废在途请求");
        assert_eq!(c.fail_streak(), 1, "超时记一次失败");
        // 作废后再 tick 不再产出（不重复计失败、不重复报错）
        assert!(matches!(
            c.tick::<serde_json::Value>(Instant::now()),
            Progress::Pending
        ));
        assert_eq!(c.fail_streak(), 1, "同一次失败不得计两遍");
    }

    /// **对端静默时连续推进 200 拍必须立即返回**（`tick` 内无任何等待）。
    ///
    /// 判别口径与裕度见 [`NEVER_BLOCK_BUDGET`]：失败模式是"第一拍耗掉整个 5 s 超时"或"挂死"，
    /// 本阈值 1 s 对失败形态有 ≥ 5 倍裕度、对真实实现（微秒级）有 10⁵ 倍以上裕度。
    ///
    /// **改什么会让本条变红**：把 `read_step` 的 `WouldBlock` 分支改成"重试到有数据"，
    /// 或把 `rx.try_recv()` 换成 `rx.recv()` ⇒ 本用例**挂死**（harness 超时 = 红）；
    /// 改成"自旋到截止"则第一拍 ≥ 5 s > 1 s ⇒ 红。
    #[test]
    fn tick_never_blocks_on_a_silent_peer() {
        let stub = Stub::scripted(vec![None]);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_query(ConsoleEndpoint::Audit, "", clock()).expect("begin");
        drive_until(
            &mut c,
            |c| c.phase_name() == "reading",
            HANG_GUARD,
        );

        let t0 = Instant::now();
        for _ in 0..200 {
            assert!(matches!(
                c.tick::<serde_json::Value>(Instant::now()),
                Progress::Pending
            ));
        }
        let elapsed = t0.elapsed();
        assert!(
            elapsed < NEVER_BLOCK_BUDGET,
            "对端静默时 200 拍耗时 {elapsed:?} —— `tick` 内出现了等待（设计 §5.2 不变量 3）；\
             失败模式应为「第一拍耗掉整个 5 s 超时」或直接挂死，裕度 ≥ 5×"
        );
        assert!(c.is_busy(), "静默对端不改变在途状态（只有截止到期才 abort）");
    }

    /// **首个 tick 也不阻塞**（连接尚未完成时）：连续 200 拍立即返回。
    ///
    /// ⚠️ **判别力的诚实说明（B3-1 评审重要 3 要求逐个复核）**：本条**不是**"`tick` 内没做
    /// 阻塞 connect"的判别式 —— 本机回环上对无监听端口 connect 会被**立即** RST
    /// （`ECONNREFUSED`），因此即使把 connect 内联进 `tick`，第一拍也照样秒回，本条仍会绿。
    /// 本机**无法**构造"connect 真耗时"的可移植场景（需要丢包 / 黑洞路由）。
    ///
    /// 本条实际判的是 **`tick` 不挂死**：若 `tick` 内出现**无上界**等待（`rx.recv()` 无超时、
    /// 或 `TcpStream::connect` 不可达分支那种无界调用），200 拍会**永不返回** ⇒ harness 超时 = 红。
    /// 阈值 [`NEVER_BLOCK_BUDGET`] = 1 s 对"挂死"是 ∞ 倍裕度，对正常路径（微秒级）是 10⁵ 倍以上。
    /// **真正的"tick 不阻塞"判别式是 [`tick_never_blocks_on_a_silent_peer`]**（对端持有连接且静默
    /// ⇒ 阻塞读永不返回），本条不重复承担该职责。
    #[test]
    fn tick_never_blocks_during_connect_phase() {
        let port = leaked_port(); // 无监听 ⇒ 连接过程本身也可能要等 OS
        let mut c = ConsoleClient::with_timeout(
            &format!("http://127.0.0.1:{port}"),
            Duration::from_millis(300),
        )
        .expect("client");
        c.begin_write("apply", &serde_json::json!({}), clock()).expect("begin");
        let t0 = Instant::now();
        for _ in 0..200 {
            let _ = c.tick::<serde_json::Value>(Instant::now());
        }
        assert!(
            t0.elapsed() < NEVER_BLOCK_BUDGET,
            "连接阶段 200 拍耗时 {:?} —— `tick` 内出现了无上界等待（挂死模式应为 ∞）",
            t0.elapsed()
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ⑦ 幂等重试（**复用同一 request_id**）
    // ═══════════════════════════════════════════════════════════════════════

    /// 首次静默超时 → `retry` → 桩回包 ⇒ **两次请求报文的 `request_id` 逐字相同**，
    /// 且回执的 `duplicate=true` 原样上抛。
    ///
    /// **改什么会让本条变红**：让 `retry` 重新生成 `request_id`（或重新序列化信封）⇒
    /// 第二条断言（两次报文的 `request_id` 相等）红 —— 那正是"两次不同操作被服务端当成
    /// 同一请求 / 同一操作被当成两次执行"的静默缺陷。
    #[test]
    fn retry_reuses_the_same_request_id_on_the_wire() {
        let stub = Stub::spawn(|i, req| {
            if i == 0 {
                None // 首次静默 ⇒ 客户端超时
            } else {
                let rid = extract_request_id(req).unwrap_or_default();
                Some(http_ok(&response_json(&rid, true, "ok", true, "null")))
            }
        });
        let mut c = ConsoleClient::with_timeout(&stub.base_url, Duration::from_millis(150))
            .expect("client");
        let id = c.begin_write("release", &payload(), clock()).expect("begin");

        let budget = Instant::now() + HANG_GUARD;
        loop {
            if let Progress::Done(r) = c.tick::<serde_json::Value>(Instant::now()) {
                r.expect_err("首次应超时");
                break;
            }
            assert!(Instant::now() < budget, "首次超时未触发");
            std::thread::yield_now();
        }

        c.retry(clock()).expect("重试");
        assert_eq!(
            c.inflight_request_id(),
            Some(id.as_str()),
            "重试必须复用同一 request_id（否则服务端会当成一次新操作执行）"
        );
        let out = drive_to_done(&mut c, HANG_GUARD).expect("重试成功");
        let resp = out.response().expect("回执");
        assert_eq!(resp.request_id, id, "回执对的是同一 request_id");
        assert!(resp.duplicate, "幂等命中 ⇒ duplicate=true，且**不是**错误");
        assert!(resp.ok);

        // 线上报文比对：两次请求的 request_id 必须逐字相同
        assert_eq!(stub.request_count(), 2, "桩应收到两次请求");
        let (a, b) = (stub.request(0), stub.request(1));
        assert_eq!(
            extract_request_id(&a),
            extract_request_id(&b),
            "重试的 `request_id` 必须与首次**逐字相同**"
        );
        assert!(b.contains("\"issued_at_ms\":1757412000000"), "重发放**原样**信封（含 issued_at_ms）");
        // 首次超时后 `fail_streak` 记 1；重试成功 ⇒ 清零
        assert_eq!(c.fail_streak(), 0, "成功清零失败计数");
    }

    /// 静默桩下推进到**超时收口**，返回该错误（测试自身不 sleep）。
    fn drive_to_timeout(c: &mut ConsoleClient) -> ConsoleError {
        // 先到读阶段：证明超时**不是**因为连不上（也保证桩已 accept 到连接）。
        drive_until(
            c,
            |c| c.phase_name() == "reading",
            HANG_GUARD,
        );
        let budget = Instant::now() + HANG_GUARD;
        loop {
            if let Progress::Done(r) = c.tick::<serde_json::Value>(Instant::now()) {
                return r.expect_err("静默桩 ⇒ 必然超时");
            }
            assert!(Instant::now() < budget, "静默桩下超时未触发");
            std::thread::yield_now();
        }
    }

    /// **过期信封的重发必须在入口被拒**：`retry` 距签发 ≥ 服务端防重放窗口 ⇒
    /// [`ConsoleError::RetryWindowExpired`]，**响亮失败、不发包**，且文案点明出路
    /// （**新操作 / 新 uuid + 重新确认 T-3**）。
    ///
    /// 若照发，服务端会**先被窗口拒**（§3.3 管线「2 窗口 → 3 幂等表」）⇒ 客户端把
    /// 「首次操作可能已生效」显示成「操作失败」（静默语义偏差）。
    ///
    /// **改什么会让本条变红**：删掉 `retry` 里的 `age_ms >= REPLAY_WINDOW_MS` 判定 ⇒
    /// 第一条断言拿到 `Ok(())` ⇒ 红。
    #[test]
    fn retry_past_the_replay_window_fails_loudly_and_sends_nothing() {
        let stub = Stub::scripted(vec![None]);
        let mut c = ConsoleClient::with_timeout(&stub.base_url, Duration::from_millis(150))
            .expect("client");
        let t0 = clock();
        c.begin_write("release", &payload(), t0).expect("begin");
        let first = drive_to_timeout(&mut c);
        assert!(matches!(first, ConsoleError::Timeout(_)), "got {first:?}");
        assert_eq!(stub.accepted(), 1, "首次请求确实发出过");

        // 边界：`now − issued_at_ms == REPLAY_WINDOW_MS` **即拒**（客户端取 `>=`，
        // 比服务端 `validate_envelope` 的 `>` 更保守 —— 判定与裁决之间还有网络时延）。
        let late = ConsoleClock {
            wall_ms: t0.wall_ms + REPLAY_WINDOW_MS,
            start: Instant::now(),
        };
        let err = c.retry(late).expect_err("过期重发必须 Err");
        match &err {
            ConsoleError::RetryWindowExpired {
                op,
                issued_at_ms,
                age_ms,
                window_ms,
            } => {
                assert_eq!(op, "release");
                assert_eq!(*issued_at_ms, t0.wall_ms, "报的是**原封**签发时刻");
                assert_eq!(*age_ms, REPLAY_WINDOW_MS);
                assert_eq!(*window_ms, REPLAY_WINDOW_MS);
            }
            other => panic!("got {other:?}"),
        }
        let text = err.to_string();
        assert!(text.contains("NEW"), "文案须点明「作为新操作重发」：{text}");
        assert!(text.contains("uuid"), "文案须点明「新 uuid」：{text}");
        assert!(text.contains("T-3"), "文案须点明「重新走确认」：{text}");

        assert!(!c.is_busy(), "被拒的重试不得进入在途状态");
        assert_eq!(stub.accepted(), 1, "过期重试**一个包都不该发出去**");
        assert_eq!(stub.request_count(), 1, "桩只该收到首次那一条");
        // 读口（B3-1 评审阻塞 2 要求暴露）：接线层可据此预先判定"还值不值得重试"
        assert_eq!(c.issued_at_ms(), Some(t0.wall_ms));
        assert_eq!(c.request_age_ms(t0.wall_ms + REPLAY_WINDOW_MS), Some(REPLAY_WINDOW_MS));
        assert_eq!(c.request_age_ms(t0.wall_ms - 7), Some(0), "时钟回拨按 0 计");
    }

    /// 窗口**内**的重发仍必须放行（差 1 ms 不算过期）；且**查询端点无信封**
    /// ⇒ `issued_at_ms()` 为 `None`、重放窗口对它不适用。
    ///
    /// 本例只判**重试准入**（`retry` 的返回与 `is_busy`），因此用 `leaked_port` 的**必然拒连**
    /// 端点即可、不牵桩服务端（桩在 Windows 上多连接的 `accept` 时机不确定，会引入 flaky）。
    ///
    /// **改什么会让本条变红**：把 `retry` 的判据放宽一格（`age_ms + 1 >= REPLAY_WINDOW_MS`）
    /// ⇒ 第一条 `expect` 红；把 `begin_query` 的 `issued_at_ms` 写成 `Some(..)` ⇒ 查询那条断言红。
    #[test]
    fn retry_inside_the_window_is_allowed_and_queries_have_no_envelope() {
        let port = leaked_port(); // bind-then-drop ⇒ 连接必然失败；本例不判网络结果
        let base = format!("http://127.0.0.1:{port}");
        let t0 = clock();

        let mut c =
            ConsoleClient::with_timeout(&base, Duration::from_millis(150)).expect("client");
        c.begin_write("release", &payload(), t0).expect("begin");
        assert_eq!(c.issued_at_ms(), Some(t0.wall_ms), "写请求有信封");
        assert_eq!(c.request_age_ms(t0.wall_ms + 1), Some(1), "读口按注入时刻算年龄");
        assert_eq!(c.request_age_ms(t0.wall_ms - 7), Some(0), "时钟回拨按 0 计");
        c.cancel();

        // 边界：差 1 ms 到窗口 ⇒ **仍放行**（客户端判据是 `>=`，不是 `>` 之外的另一套）
        c.retry(ConsoleClock {
            wall_ms: t0.wall_ms + REPLAY_WINDOW_MS - 1,
            start: Instant::now(),
        })
        .expect("窗口内（差 1 ms）必须放行");
        assert!(c.is_busy(), "放行 ⇒ 确实起了在途请求");
        c.cancel();

        // 查询（GET）：无信封 ⇒ 无 `issued_at_ms`，重放窗口**不适用** ⇒ 任意晚都可重发
        let mut q =
            ConsoleClient::with_timeout(&base, Duration::from_millis(150)).expect("client");
        q.begin_query(ConsoleEndpoint::Logs, "", t0).expect("begin");
        assert_eq!(q.issued_at_ms(), None, "查询端点无信封");
        assert_eq!(q.request_age_ms(t0.wall_ms + REPLAY_WINDOW_MS * 10), None);
        // 拒连即收口（在途清空）；`retry` 要求无在途 ⇒ 先等它结束
        let dead = drive::<serde_json::Value>(&mut q, HANG_GUARD);
        assert!(dead.is_err(), "拒连端点必然收口为 Err（不静默）：{dead:?}");
        assert!(!q.is_busy(), "拒连后不得残留在途");
        q.retry(ConsoleClock {
            wall_ms: t0.wall_ms + REPLAY_WINDOW_MS * 10,
            start: Instant::now(),
        })
        .expect("无信封 ⇒ 窗口判据不适用，必须放行");
        assert!(q.is_busy());
        q.cancel();
    }

    #[test]
    fn retry_without_history_is_idle_and_busy_is_refused() {
        let stub = Stub::scripted(vec![None]);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        assert!(matches!(
            c.retry(clock()).expect_err("无历史"),
            ConsoleError::Idle
        ));
        c.begin_query(ConsoleEndpoint::Logs, "", clock())
            .expect("begin");
        assert!(matches!(
            c.begin_query(ConsoleEndpoint::Logs, "", clock())
                .expect_err("busy"),
            ConsoleError::Busy(_)
        ));
        assert!(matches!(
            c.retry(clock()).expect_err("busy"),
            ConsoleError::Busy(_)
        ));
        c.cancel();
        c.cancel();
        assert!(!c.is_busy(), "cancel 幂等");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ⑧ 写阶段记账（B3-1 评审**重要 1**：部分写路径的可测化）
    // ═══════════════════════════════════════════════════════════════════════

    /// **部分写必须累加、只有写完才算完**。
    ///
    /// 背景：本机（Windows 回环）内核发送缓冲一次吞下整块，实测 **6 MB body 仍
    /// `write_steps = 1`** ⇒ **无法**在真 socket 上触发 `0 < n < 剩余` 的部分写，
    /// 此前那段"记账"代码**零有效覆盖**（把 `saturating_add` 改成直接赋值，全套测试仍全绿）。
    /// 故把记账抽成纯函数 [`note_written`]，在这里用**纯逻辑**覆盖三种形态。
    ///
    /// **改什么会让本条变红**：把 `note_written` 里的 `saturating_add` 改成 `*sent = n`
    /// （即"部分写入直接覆盖"）⇒ 第一次部分写后 `sent` 为 4 而非 7，第 2 条断言红；
    /// 且 `sent` 永远停在最后一次的 `n` 上，第 4 条断言（恰好写完）也红。
    /// **这是本项目此前"看起来有覆盖、其实恒真"的那一类断言的对立面。**
    #[test]
    fn note_written_accumulates_partial_writes_and_only_finishes_when_all_sent() {
        // 形态 1：部分写（`0 < n < 剩余`）⇒ 累加、**未完成**
        let mut sent = 0usize;
        assert!(
            !note_written(&mut sent, 3, 10),
            "3 < 10 ⇒ 必须报**未写完**（否则会在请求只发了一半时就转去读响应）"
        );
        assert_eq!(sent, 3);
        assert!(!note_written(&mut sent, 4, 10), "7 < 10 ⇒ 仍未写完");
        assert_eq!(
            sent, 7,
            "部分写必须**累加**（3 + 4）；改成 `*sent = n` 时此处应为 4 ⇒ 本条即变红"
        );

        // 形态 2：恰好写完（`sent + n == total`）⇒ **完成**
        assert!(note_written(&mut sent, 3, 10), "10 == 10 ⇒ 完成");
        assert_eq!(sent, 10);

        // 形态 3：一次写完 / 空写入
        let mut once = 0usize;
        assert!(note_written(&mut once, 10, 10), "一次写完");
        assert_eq!(once, 10);
        let mut zero = 0usize;
        assert!(note_written(&mut zero, 0, 0), "空报文 ⇒ 立即完成（不会卡在写阶段）");

        // 形态 4：内核报出比剩余更多（不可能）⇒ `saturating_add` 兜住，不 panic 不倒绕
        let mut over = usize::MAX - 1;
        assert!(note_written(&mut over, 5, 10));
        assert_eq!(over, usize::MAX, "饱和而非回绕");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ⑨ 上限依据 / 粒度（B3-1 评审**重要 2**）
    // ═══════════════════════════════════════════════════════════════════════

    /// **最坏合法回包（200 条 × 1 KiB 消息的 `LogPage`）必须被接受，不得判 `BodyTooLarge`**。
    ///
    /// 这是 [`MAX_BODY_BYTES`] 从 64 KiB 上调到 256 KiB 的**直接回归**：旧上限下，
    /// `display-proto` 里 `LogEntry.message` **没有长度上限**（`grep` 无 `truncate` /
    /// `max_len`），200 条 × 1 KiB ≈ 200 KB 的**合法**回包会被判超限 ⇒ **日志页直接不可用**。
    ///
    /// **改什么会让本条变红**（**2026-09-15 独立复核实测订正 —— 原表述是错的**）：原写"把
    /// `MAX_BODY_BYTES` 改回 64 KiB ⇒ `drive` 拿到 `BodyTooLarge` ⇒ 红"。实际**编译期**那条
    /// `assert!` 会先 `E0080`，本用例**根本不会运行**。**实测复核给出的两步口径**：
    /// ① 只把上限改回 64 KiB ⇒ 编译失败（本用例不运行）；
    /// ② **同时**放宽编译期门禁 ⇒ 本用例才真正运行，并如原意那条 `expect` 失败。
    #[test]
    fn full_log_page_with_1kib_messages_is_accepted_not_body_too_large() {
        let msg = "核".repeat(ASSUMED_MAX_MESSAGE_BYTES / 3); // 3 B/字 ⇒ ≥ 1023 B（UTF-8）
        let page = mupc_display_proto::LogPage {
            entries: (0..mupc_display_proto::log::LOG_PAGE_LIMIT_MAX)
                .map(|i| mupc_display_proto::LogEntry {
                    seq: i as u64,
                    ts_ms: 1_757_412_000_000 + i as u64,
                    level: mupc_display_proto::LogLevel::Error,
                    target: "gateway".to_string(),
                    message: msg.clone(),
                })
                .collect(),
            next_cursor: None,
            has_more: false,
            range_too_large: false,
        };
        let body = serde_json::to_string(&page).expect("serialize");
        assert!(
            body.len() > crate::channel::MAX_BODY_BYTES,
            "本用例的前提：该合法回包（{} B）必须**大于读通道那套 64 KiB 上限**，\
             否则它证明不了任何事",
            body.len()
        );
        assert!(
            body.len() <= MAX_BODY_BYTES,
            "该回包（{} B）必须落在本轮上调后的上限（{MAX_BODY_BYTES}）之内",
            body.len()
        );

        let stub = Stub::scripted(vec![Some(http_ok(&body))]);
        let mut c = ConsoleClient::new(&stub.base_url).expect("client");
        c.begin_query(ConsoleEndpoint::Logs, "", clock())
            .expect("begin");
        let out = drive::<mupc_display_proto::LogPage>(&mut c, HANG_GUARD)
            .expect("满页合法回包必须被接受（不得 BodyTooLarge）");
        let got = out.query().expect("查询是裸载荷");
        assert_eq!(got.entries.len(), 200);
        assert_eq!(got.entries[0].message.len(), msg.len());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ⑩ LF-only 响应头的可区分诊断（B3-1 评审建议 4）
    // ═══════════════════════════════════════════════════════════════════════

    /// **对端"回了字节但头部凑不出 `\r\n\r\n`"必须与"对端没回"分开报**。
    ///
    /// 失败模式：对端用 **LF-only** 行结束符（只发 `\n`）。此前收口为
    /// [`ConsoleError::Timeout`]，让排障指向"对端没回包"—— **根因误导**（对端明明回了字节）。
    ///
    /// **改什么会让本条变红**：把 `timeout_or_head_incomplete` 改回直接
    /// `ConsoleError::Timeout(p.addr.clone())` ⇒ `matches!` 落到 `other` 分支 ⇒ panic 红。
    #[test]
    fn lf_only_response_is_diagnosed_as_incomplete_head_not_timeout() {
        let stub = Stub::spawn_raw_hold(|_i, _req| {
            // 合法的状态行与 Content-Length，但**行结束符全是 LF**（无 CR）
            Some(b"HTTP/1.1 200 OK\nContent-Type: application/json\nContent-Length: 2\n\n{}".to_vec())
        });
        let mut c = ConsoleClient::with_timeout(&stub.base_url, Duration::from_millis(150))
            .expect("client");
        c.begin_query(ConsoleEndpoint::Config, "", clock())
            .expect("begin");
        let err = drive_to_done(&mut c, HANG_GUARD).expect_err("LF-only 必须响亮失败");
        match err {
            ConsoleError::HeadIncomplete(_, n) => assert!(
                n >= 40,
                "必须报告**实收字节数**（对端回过东西的证据），got {n}"
            ),
            other => panic!("必须与 `Timeout` 可区分（对端已回 {other:?}）"),
        }
        assert!(!c.is_busy(), "收口后不得残留在途");
    }

    /// **对端一个字节都不回**时仍必须是 [`ConsoleError::Timeout`]（诊断分支不得误伤）。
    #[test]
    fn silent_peer_still_reports_timeout_not_incomplete_head() {
        let stub = Stub::scripted(vec![None]);
        let mut c = ConsoleClient::with_timeout(&stub.base_url, Duration::from_millis(150))
            .expect("client");
        c.begin_query(ConsoleEndpoint::Config, "", clock())
            .expect("begin");
        let out = drive_to_done(&mut c, HANG_GUARD);
        assert!(
            matches!(out, Err(ConsoleError::Timeout(_))),
            "零字节 ⇒ 仍是 Timeout，got {out:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ⑪ 查询串长度上限 / 半字节掩码（B3-1 评审建议 7）
    // ═══════════════════════════════════════════════════════════════════════

    /// **超长查询串必须在入口被拒**（未加此限时，1 MB query 会被编进请求行、且经百分号编码
    /// 膨胀到约 3 MB 一路进内核发送缓冲 —— 属"调用方传错"却表现为网络层怪象）。
    ///
    /// **改什么会让本条变红**：删掉 `begin_query` 里的 `query.len() > MAX_QUERY_BYTES` 判定 ⇒
    /// 第一条 `expect_err` 拿到 `Ok(())` ⇒ 红。
    #[test]
    fn oversized_query_string_is_rejected_at_entry() {
        let mut c = ConsoleClient::new(DEFAULT_CONSOLE_BASE_URL).expect("client");
        let huge = "a".repeat(MAX_QUERY_BYTES + 1);
        let err = c
            .begin_query(ConsoleEndpoint::Logs, &huge, clock())
            .expect_err("超长 query 必须 Err");
        assert!(
            matches!(err, ConsoleError::QueryTooLarge(n) if n == MAX_QUERY_BYTES + 1),
            "got {err:?}"
        );
        assert!(!c.is_busy(), "被拒的查询不得进入在途状态");
        // 边界：**恰好** MAX_QUERY_BYTES 必须放行（不是"小于"）
        let at_limit = "a".repeat(MAX_QUERY_BYTES);
        c.begin_query(ConsoleEndpoint::Logs, &at_limit, clock())
            .expect("恰好到上限必须放行");
        assert!(c.is_busy());
        c.cancel();
    }

    /// **未知的百分号编码产物**：`encode_query` 的编码器只对超长串设限，不改变逐字节语义。
    /// 本条锁住"编码产物"与"入口注入校验"的闭环（超长 ⇒ 先撞长度限，而不是被编码器撑爆）。
    #[test]
    fn encode_query_of_a_large_pair_is_bounded_by_the_length_check() {
        let q = encode_query(&[("targets", &"核间".repeat(MAX_QUERY_BYTES / 3))]);
        assert!(
            q.len() > MAX_QUERY_BYTES,
            "百分号编码会膨胀（3 B/字 ⇒ {} B），本用例的前提",
            q.len()
        );
        let mut c = ConsoleClient::new(DEFAULT_CONSOLE_BASE_URL).expect("client");
        assert!(
            matches!(
                c.begin_query(ConsoleEndpoint::Logs, &q, clock())
                    .expect_err("编码后超限必须被入口挡下"),
                ConsoleError::QueryTooLarge(_)
            ),
            "编码产物必须先撞长度限，不得一路进请求行"
        );
    }

    /// **半字节掩码**：本函数签名收任意 `u8`，`nibble > 15` 时旧实现 `b'A' + (nibble - 10)`
    /// 会 `u8` 溢出 ⇒ **debug 构建 panic**（release 静默回绕）。
    ///
    /// 当前调用点传的都是 `b >> 4` / `b & 0x0f`（必然 ≤ 15），但"不可达"是**调用点的性质**，
    /// 不是本函数的性质 ⇒ 在函数内自守。
    ///
    /// **改什么会让本条变红**：把 `hex_digit` 改回不掩码的版本 ⇒ 第一条断言在 debug 下
    /// **panic**（`attempt to add with overflow`）⇒ 红。
    #[test]
    fn hex_digit_masks_to_low_nibble_without_overflow() {
        for (n, want) in [(0u8, '0'), (9, '9'), (10, 'A'), (15, 'F')] {
            assert_eq!(hex_digit(n), want, "hex_digit({n})");
        }
        // 越界（在本函数签名下是合法入参）：取低 4 位，**不 panic、不回绕**
        assert_eq!(hex_digit(0x1A), 'A');
        assert_eq!(hex_digit(0xff), 'F');
        assert_eq!(hex_digit(0x20), '0');
    }
}
