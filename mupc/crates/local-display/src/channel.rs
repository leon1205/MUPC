//! 通道客户端：`DisplayChannelClient`——对 mupcd 回环发布端点做最小 HTTP/1.1 GET。
//!
//! 对齐 `[DESIGN_APPROVED]` 设计 §3.1/§5.3/§5.5：
//! - 形态：TCP 回环 `127.0.0.1:<port>`，`GET /v1/display/latest` → 200 + JSON 帧。
//! - 语义：每次取到即最新有效帧；单次 GET 超时 **2 s**；失败返回 `Err`，由上层（`state`/`App`）
//!   判通道断（`Err` 是正常展示态之一，**不是**致命错误）。
//! - **非阻塞状态机**（§5.5 钉死，B3-2a 改造）：`set_nonblocking(true)` + `write → read`
//!   两阶段推进，由 [`DisplayChannelClient::tick`] **每拍推进一次**，超时以 `Instant`
//!   截止时刻强制（到期即 abort、计一次通道失败）。**禁止 `block_on`、禁止在 `tick` 内同步
//!   等待** —— 这是 §5.2 不变量 3（阻塞 I/O 不得发生在回调/事件循环内）的唯一实现形态。
//! - 实现：**裸 `std::net::TcpStream` + 手写 HTTP/1.1**（无 reqwest/ureq/tokio 重依赖）。
//!   请求头带 `Connection: close`；响应解析先读头到 `\r\n\r\n`，有 `Content-Length` 按长度
//!   精确读，否则读到 EOF 作为 body（对我们的 LoopbackHttpPublisher / 测试桩均回
//!   `Content-Length`，语义正确且可 mock）。
//! - 无 TLS（仅回环，设计 D1：本地可信、不暴露外网）。
//!
//! # 与 `console.rs`（B3-1）**同构**（刻意为之，非巧合）
//!
//! | 维度 | 本文件 | `console.rs` |
//! |------|--------|--------------|
//! | 推进 | [`DisplayChannelClient::tick`]（绝不阻塞、步数封顶） | `ConsoleClient::tick` |
//! | 产出 | [`Progress`]（`Pending` / `Done`） | `console::Progress`（**同形、不同载荷**，见下） |
//! | 截止 | `Instant` 绝对截止，循环每次迭代开头判一次 | 同 |
//! | 上限 | 头 64 KiB / 体 64 KiB，**预检在分配之前** | 头 64 KiB / 体 256 KiB（依据不同，见该文件） |
//! | 连接 | 一次性工作线程跑 `connect_timeout`（有界） | 同（**同一条**已登记的偏差，见下） |
//!
//! ## ⚠️ 与设计 §5.5 字面的偏差（**继承 B3-1 的登记，不另开一份**）
//!
//! 设计写「`set_nonblocking(true)` + `connect → write → read` 三阶段状态机」。但
//! `std::net::TcpStream` **没有**可移植的非阻塞 `connect`（`connect_timeout(.., ZERO)`
//! 会**丢弃** socket 句柄 ⇒ 无法续接），真正的非阻塞 connect 需 `socket2`/裸 `libc`
//! （本单元禁引新依赖、且 Windows 本机必须可跑）。故与 `console.rs` **取同一形态**：
//! **连接**交一次性工作线程（只做 `connect_timeout`，拿到 socket 立刻退出），
//! **写 / 读**在 `tick` 内以 `set_nonblocking(true)` 推进。
//!
//! 不变量（§5.2 条 3）**完全成立**：`tick` 内只有 `try_recv` / `write` / `read` 三种
//! **非阻塞**调用，无任何等待（见用例 `tick_never_blocks_on_a_silent_peer`）。
//!
//! ## host 必须是 IP 字面量（**比 v1.0 收紧，响亮失败**）
//!
//! v1.0 用 `tokio::net::TcpStream::connect(&str)`（带 DNS、可无界等待）。本实现与
//! `console.rs` 同纪律：`try_new` 在**入口**把 `host:port` 解析成 [`SocketAddr`]，
//! 非 IP 字面量（如 `localhost`）**启动即报错**（[`Error::BadUrl`]，不 panic）。
//! 理由：回环直连字面量是唯一生产方式（设计 §1.4 PL-8），而"名字解析失败"若不在入口挡掉，
//! 就会退化成运行期的无界阻塞 `connect`（超出 2 s 截止存活）。
//!
//! ## 为什么 `Progress` 有**两份**（登记的判断，非疏漏）
//!
//! 两个通道的"每拍产出"是**同形但不同载荷**的两件事：
//! 读通道的完成值 = `Result<DisplayFrame>`（单一载荷），
//! 控制通道的完成值 = `Result<ConsoleOutcome<T>>`（回执信封 + 裸 DTO 两分支，且 `T` 由
//! 端点决定）。抽成一个共享枚举需要**两个**泛型参数（`Progress<D>` 只是壳，语义会糊），
//! 而 `console.rs` 是已评审交付物：为一个 6 行的壳类型改动它的公开签名与全部用例，
//! 收益不抵风险。故：**形态对齐（见上表）、类型各自持有**，两处均有用例钉住「无进展 /
//! 本拍结束」两分支。

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use mupc_display_proto::DisplayFrame;

/// 单次 GET 超时（设计 §5.3/§5.5：失败记一次，由上层按「连续失败 / 无成功 >3s」判通道断）。
pub const GET_TIMEOUT: Duration = Duration::from_secs(2);

/// 响应头读取上限（64 KiB）。
pub const MAX_HEAD_BYTES: usize = 64 * 1024;

/// 响应体读取上限（**64 KiB**）。标称帧 JSON 约数百字节，余量充分；超限即 `Err`（计入失败），
/// 杜绝畸形/异常对端用巨额 `Content-Length` 触发内存失控（PRD 4.4.3：渲染端不得因通道对端
/// 行为异常而崩溃或内存失控）。
///
/// **与 `console.rs` 的 256 KiB 不再要求相等**（B3-1 已登记的口径订正）：读通道的帧结构固定、
/// **无自由文本**，按帧长度域推算即可；控制通道的回包含变长 JSON 文本（`LogEntry.message`），
/// 依据不同、服务不同端点。
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// 单拍内状态机最多推进的步数（防「对端持续可写 / 可读」把事件循环饿死）。
/// 耗尽即收工、下一拍续推 —— **进度不丢**（已写字节 / 已读字节都留在 `Pending` 里）。
const MAX_STEPS_PER_TICK: u32 = 64;

/// 非阻塞读的块大小（头部累积与体累积共用）。
const READ_CHUNK: usize = 4096;

/// 帧响应声明的媒体类型（`Content-Type`；发布端 `mupc-core-bin/src/display_host.rs` 恒发此值）。
pub const JSON_CONTENT_TYPE: &str = "application/json";

/// 已解析的回环通道端点（`http://host:port/path`；host 为字面量，用于直连）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEndpoint {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl ChannelEndpoint {
    /// 解析通道 URL。仅支持 `http://`（无 TLS）；host 不做 DNS（回环直连字面量）；
    /// 端口缺省 80；路径缺省 `/`。
    pub fn parse(url: &str) -> Result<Self> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| Error::BadUrl(url.into(), "仅支持 http://（回环无 TLS）".into()))?;
        if let Some((host_port, path)) = rest.split_once('/') {
            let mut endpoint = Self::parse_host_port(url, host_port)?;
            endpoint.path = format!("/{path}");
            Ok(endpoint)
        } else {
            let mut endpoint = Self::parse_host_port(url, rest)?;
            endpoint.path = "/".to_string();
            Ok(endpoint)
        }
    }

    fn parse_host_port(url: &str, host_port: &str) -> Result<Self> {
        let (host, port) = match host_port.rsplit_once(':') {
            Some((h, p)) => {
                let port: u16 = p
                    .parse()
                    .map_err(|_| Error::BadUrl(url.into(), format!("非法端口 `{p}`")))?;
                (h.to_string(), port)
            }
            None => (host_port.to_string(), 80),
        };
        if host.is_empty() {
            return Err(Error::BadUrl(url.into(), "host 为空".into()));
        }
        Ok(Self {
            host,
            port,
            path: String::new(),
        })
    }

    /// 解析为可直连的 [`SocketAddr`]（**不做 DNS**）——失败即"入口拒绝，
    /// 绝不让无界 `connect` 在运行期出现"。
    pub fn socket_addr(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse::<SocketAddr>()
            .map_err(|_| {
                Error::BadUrl(
                    format!("http://{}:{}{}", self.host, self.port, self.path),
                    format!(
                        "host `{}` 不是 IP 字面量（本设计不做 DNS；IPv6 须写成 `http://[::1]:9811`）",
                        self.host
                    ),
                )
            })
    }

    /// 组装请求行 Host 头（回环字面量）。
    fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// 回环发布端点默认 URL（display-proto 共享常量）。
pub const DEFAULT_CHANNEL_URL: &str = mupc_display_proto::DEFAULT_CHANNEL_URL;

// ═══════════════════════════════════════════════════════════════════════════
// 1. 每拍推进的产出（`console.rs` 复用同一类型）
// ═══════════════════════════════════════════════════════════════════════════

/// 每拍推进的产出（两个通道客户端**共用**的类型，见模块头「`Progress` 只有一份定义」）。
///
/// 刻意用枚举而非 `Option<Result<..>>`：`Option` 会把「无在途请求」与「仍在途未完成」
/// 混为一谈，而调用方对两者的处置不同（前者可发起下一拍请求、后者必须等）。
#[derive(Debug)]
pub enum Progress<T> {
    /// 无进展：无在途请求，或在途且未结束（**调用方下一拍再来**）。
    Pending,
    /// 在途请求本拍结束（成功或失败）。
    Done(T),
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. 节拍判定（纯逻辑，跨平台可测）
// ═══════════════════════════════════════════════════════════════════════════

/// 本拍是否该**发起**下一次 GET（纯逻辑，与 `--poll-ms` 节拍一一对应）。
///
/// `next_at_ms = None`（尚未发过）⇒ 立即发起（**首拍不等 500 ms**：渲染可先于 mupcd 启动，
/// 且首帧越早上屏越好，设计 §4.3.4）。
pub fn poll_due(now_ms: u64, next_at_ms: Option<u64>) -> bool {
    // 用 `match` 而不是 `Option::is_none_or`（后者 1.82 才稳定，而本 crate 的 MSRV 是 1.75）
    // 也不用 `map_or(true, ..)`（clippy 的 `unnecessary_map_or` 会指向 `is_none_or`）。
    match next_at_ms {
        None => true,
        Some(t) => now_ms >= t,
    }
}

/// 下一次可发起请求的时刻（`now_ms + interval_ms`，饱和加法不 panic）。
pub fn next_poll_at(now_ms: u64, interval_ms: u64) -> u64 {
    now_ms.saturating_add(interval_ms)
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 状态机内部
// ═══════════════════════════════════════════════════════════════════════════

/// 状态机阶段（仅作推进标记；`TcpStream` 由 [`Pending::stream`] 持有）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// 工作线程正在 `connect_timeout`。
    Connecting,
    /// 正在写出请求报文。
    Writing,
    /// 正在读入响应。
    Reading,
}

/// 在途请求的推进状态。
#[derive(Debug)]
struct Pending {
    /// 完整 HTTP 请求报文。
    raw: Vec<u8>,
    /// `host:port`（错误串用）。
    addr: String,
    /// 超时截止时刻（`Instant`，单次 [`GET_TIMEOUT`]）。
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
    /// `Content-Length`；`None` 且 `head_done` ⇒ 读到 EOF 为止（§3.1 同口径）。
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
    Failed(Error),
}

/// 渲染侧通道客户端。无连接态持有——每轮 `begin` 新建连接（设计 §3.1：服务端不依赖保活）。
#[derive(Debug)]
pub struct DisplayChannelClient {
    endpoint: ChannelEndpoint,
    /// 目标地址（**解析后的唯一真源**）：入口解析一次，[`Self::addr`] 与连接线程都从它派生
    /// ⇒ 不存在"两处 `format!` 必须永远一致"的隐式契约（与 `console.rs` 同款整改）。
    addr: SocketAddr,
    /// 单次 GET 超时（默认 [`GET_TIMEOUT`]；测试注入更短值）。
    timeout: Duration,
    pending: Option<Pending>,
    /// 连续失败数（超时 / 连接 / HTTP / 解码；成功即清零）。
    fail_streak: u32,
}

impl Default for DisplayChannelClient {
    fn default() -> Self {
        Self::new(DEFAULT_CHANNEL_URL)
    }
}

impl DisplayChannelClient {
    /// 由 URL 建客户端（解析失败时回退默认端点；生产请用 [`Self::try_new`] 拿明确错误）。
    pub fn new(url: &str) -> Self {
        Self::try_new(url).unwrap_or_else(|_| {
            // `DEFAULT_CHANNEL_URL` 是常量，其可解析性由
            // `client_default_points_at_loopback_publisher` 直接覆盖 ⇒ 此处不可达。
            Self::try_new(DEFAULT_CHANNEL_URL).expect("DEFAULT_CHANNEL_URL 是合法回环 URL")
        })
    }

    /// 解析失败时保留错误可见的构造（供 CLI 层给用户明确报错）。成功返回 Ok(Client)。
    pub fn try_new(url: &str) -> Result<Self> {
        let endpoint = ChannelEndpoint::parse(url)?;
        let addr = endpoint.socket_addr()?;
        Ok(Self {
            endpoint,
            addr,
            timeout: GET_TIMEOUT,
            pending: None,
            fail_streak: 0,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_timeout(url: &str, timeout: Duration) -> Result<Self> {
        let mut c = Self::try_new(url)?;
        c.timeout = timeout;
        Ok(c)
    }

    pub fn endpoint(&self) -> &ChannelEndpoint {
        &self.endpoint
    }

    /// 目标 `host:port`（由结构体的 `addr` 字段直接格式化 ⇒ 与连接线程入参**同源**）。
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

    /// 连续失败数（成功即清零）。
    pub fn fail_streak(&self) -> u32 {
        self.fail_streak
    }

    /// 本拍可观察的阶段名（**诊断用**：`idle` / `connecting` / `writing` / `reading`）。
    ///
    /// 消费点：`--smoke` 日志与用例 `tick_never_blocks_on_a_silent_peer`（该用例必须**先**确认
    /// 真的进了 `reading` 才会开始计时 —— 连接未交回时测的是空路径，见 I-1）。
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
    /// 用于「上层决定不再需要这一拍的结果」。已写出的字节留在内核发送缓冲里、**不能撤回**
    /// （与 `console.rs::cancel` 的边界说明逐条同口径）；析构 `TcpStream` 时 TCP 层仍会发 FIN。
    /// 无在途请求时是**幂等 no-op**。
    ///
    /// **保留理由（B3-2a 质量评审 I-5 裁定）**：生产路径当前无调用者 —— 它属于 **B3-2b 的
    /// 关停路径**（收到 SIGTERM 后事件循环退出前作废在途 GET，避免 connect 工作线程把结果
    /// 投给已无人接收的接收端）。不删。
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// 发起一次 GET（**无在途请求时**）。已有在途 ⇒ [`Error::Busy`]（**不静默排队**）。
    ///
    /// `now` 为注入的单调时刻（截止 = `now + timeout`），使超时链路可确定性单测。
    pub fn begin(&mut self, now: Instant) -> Result<()> {
        if let Some(p) = self.pending.as_ref() {
            return Err(Error::Busy(p.addr.clone()));
        }
        let addr_str = self.addr();
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: {JSON_CONTENT_TYPE}\r\nConnection: close\r\n\r\n",
            self.endpoint.path,
            self.endpoint.authority()
        );
        let deadline = now + self.timeout;

        // 连接只在工作线程里做（原因 = 模块头偏差）；拿到 socket 后立刻退出。
        let (tx, rx) = mpsc::channel();
        let connect_addr = self.addr; // `SocketAddr` 是 `Copy`；与 `addr()` 同源、不会漂移
        let budget = self.timeout;
        std::thread::Builder::new()
            .name("mupc-display-connect".to_string())
            .spawn(move || {
                // 接收端可能已因超时被丢弃 ⇒ send 失败即静默结束（已无在途请求需要它）。
                let _ = tx.send(connect_timeout(connect_addr, budget));
            })
            .map_err(|e| Error::Io(addr_str.clone(), e))?;

        self.pending = Some(Pending {
            raw: request.into_bytes(),
            addr: addr_str,
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
        Ok(())
    }

    /// 每拍推进一次（**绝不阻塞**：只有 `try_recv` 与非阻塞 `write` / `read`）。
    ///
    /// 返回 [`Progress::Done`] 即本拍在途请求结束（成功 = 已验证版本的 `DisplayFrame`）。
    pub fn tick(&mut self, now: Instant) -> Progress<Result<DisplayFrame>> {
        let Some(pending) = self.pending.as_mut() else {
            return Progress::Pending;
        };

        // 截止判定**只有下面 `for` 循环里那一处**（循环每次迭代开头先判；与 `console.rs` 同款，
        // 循环外不重复同一判据）。`step` 带出借用后再改 `self`（避免「持借用改 self」）。
        let step_out: Result<bool> = {
            let p = pending;
            let mut out: Result<bool> = Ok(false);
            for _ in 0..MAX_STEPS_PER_TICK {
                if now >= p.deadline {
                    out = Err(Error::Timeout(p.addr.clone()));
                    break;
                }
                match advance(p) {
                    Step::Continue => continue,
                    // 对端暂时不可读写：**进度保留**（已写 / 已读字节都在 `Pending` 里）。
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
                Some(p) => self.parse(p),
                None => Progress::Pending,
            },
        }
    }

    // ── 内部 ──────────────────────────────────────────────────────────────

    /// 解析已读全的响应体（借用已释放，可自由改 `self.fail_streak`）。
    fn parse(&mut self, p: Pending) -> Progress<Result<DisplayFrame>> {
        let addr = p.addr;
        let res: Result<DisplayFrame> = serde_json::from_slice::<DisplayFrame>(&p.body)
            .map_err(|e| Error::Json(addr.clone(), e))
            .and_then(|frame| {
                // W3：版本一致性校验（设计 §3.3/PRD 4.4.1）——不符即明确错误（计入失败），
                // 绝不静默按旧语义展示。
                if frame.version != mupc_display_proto::PROTO_VERSION {
                    Err(Error::ProtoVersion(
                        addr,
                        frame.version,
                        mupc_display_proto::PROTO_VERSION,
                    ))
                } else {
                    Ok(frame)
                }
            });
        match res {
            Ok(f) => {
                self.fail_streak = 0;
                Progress::Done(Ok(f))
            }
            Err(e) => {
                self.fail_streak = self.fail_streak.saturating_add(1);
                Progress::Done(Err(e))
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 状态机单步（全部**非阻塞**）
// ═══════════════════════════════════════════════════════════════════════════

/// 连接（**在工作线程里调用**；**恒有界** —— `timeout` 上界，不用裸 `connect` 的无界等待）。
fn connect_timeout(addr: SocketAddr, timeout: Duration) -> std::io::Result<TcpStream> {
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
                        return Step::Failed(Error::Io(p.addr.clone(), e));
                    }
                    p.stream = Some(stream);
                    p.rx = None;
                    p.phase = Phase::Writing;
                    Step::Continue
                }
                Ok(Err(e)) => Step::Failed(Error::Connect(p.addr.clone(), e)),
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
            let rest = p.raw.get(p.sent..).unwrap_or(&[]);
            if rest.is_empty() {
                p.phase = Phase::Reading;
                return Step::Continue;
            }
            match stream.write(rest) {
                Ok(0) => Step::Failed(io_err(&p.addr, "write returned 0")),
                Ok(n) => {
                    let total = p.raw.len();
                    if note_written(&mut p.sent, n, total) {
                        p.phase = Phase::Reading;
                    }
                    Step::Continue
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => Step::Blocked,
                Err(e) if e.kind() == ErrorKind::Interrupted => Step::Continue,
                Err(e) => Step::Failed(Error::Io(p.addr.clone(), e)),
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
/// 存在的理由（与 `console.rs` 同款，B3-1 评审重要 1）：`TcpStream::write` 允许**部分写**
/// （`0 < n < 剩余`），此时必须**累加**而不能直接赋值，否则请求报文会被**静默截断**。
/// 而本机（Windows 回环）内核发送缓冲一次吞下整块 ⇒ 真 socket 上**触发不了**部分写，
/// 故把记账抽成纯函数，用纯逻辑用例覆盖「部分写 / 恰好写完 / 一次写完」三种形态。
fn note_written(sent: &mut usize, n: usize, total: usize) -> bool {
    *sent = sent.saturating_add(n);
    *sent >= total
}

/// 非阻塞读一步：`Ok(true)` = 体已读全；`Ok(false)` = 本拍暂无更多数据。
fn read_step(p: &mut Pending) -> Result<bool> {
    loop {
        let Some(stream) = p.stream.as_mut() else {
            return Err(io_err(&p.addr, "stream missing in reading phase"));
        };
        let mut buf = [0u8; READ_CHUNK];
        match stream.read(&mut buf) {
            Ok(0) => {
                // EOF：有长度头却没读够 ⇒ 截断（响亮失败）；否则即「读到 EOF 作 body」（§3.1 同口径）。
                if !p.head_done {
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
                    // 无长度头（读到 EOF 为止）时**同样封顶**：防无限流撑爆内存。
                    if p.body.len() > MAX_BODY_BYTES {
                        return Err(Error::BodyTooLarge(p.addr.clone(), p.body.len()));
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
                                return Err(Error::HeadTooLarge(p.addr.clone(), pos));
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
                            // W2：**分配之前**的上限判定（`body` 是追加读入的、
                            // 不是按 `Content-Length` 预分配的 ⇒ 结构上不可能巨额预分配）。
                            if p.body.len() > MAX_BODY_BYTES {
                                return Err(Error::BodyTooLarge(
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
                                return Err(Error::HeadTooLarge(
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
            Err(e) => return Err(Error::Io(p.addr.clone(), e)),
        }
    }
}

/// 解析状态行 + 头（`pos` = `\r\n\r\n` 起始下标）。
///
/// **安全语义三条**（§5.5 / PRD 4.4.3；逐条有用例）：
/// 1. 非 200 ⇒ [`Error::HttpStatus`]（503 = 尚未就绪，同样计入失败，不静默当空帧）；
/// 2. `Content-Length` **超上限在此当场拒绝**（[`Error::BodyTooLarge`]）—— 在读写任何体字节
///    **之前**，从而结构上不可能出现"按畸形长度预分配"；
/// 3. 声明的 `Content-Type` 必须是 JSON（见 [`check_content_type`]）。
fn parse_head(p: &mut Pending, pos: usize) -> Result<()> {
    let text = String::from_utf8_lossy(p.head.get(..pos).unwrap_or(&[])).into_owned();
    let (status_line, rest) = text.split_once("\r\n").unwrap_or((text.as_str(), ""));
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if status != 200 {
        return Err(Error::HttpStatus(status, p.addr.clone()));
    }
    for line in rest.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if k.eq_ignore_ascii_case("content-length") {
            let n = v
                .parse::<usize>()
                .map_err(|_| io_err(&p.addr, &format!("bad Content-Length header `{v}`")))?;
            if n > MAX_BODY_BYTES {
                return Err(Error::BodyTooLarge(p.addr.clone(), n));
            }
            p.content_length = Some(n);
        } else if k.eq_ignore_ascii_case("content-type") {
            check_content_type(&p.addr, v)?;
        }
    }
    Ok(())
}

/// `Content-Type` 校验（声明了就必须是 JSON 媒体类型）。
///
/// **规则（有意宽松处已注明）**：
/// - **未声明** ⇒ 放行（最小桩 / 只回 `Content-Length` 的对端仍可用；body 仍要过
///   `serde_json` 与版本校验两道关，不会"静默当有效帧"）；
/// - **声明了** ⇒ 媒体类型（`;` 之前、去空白、忽略大小写）必须等于 [`JSON_CONTENT_TYPE`]；
///   否则 [`Error::Io`] + `InvalidData`（**响亮失败**，理由：把 `text/html` 错误页当 JSON
///   喂给解码器，会把"对端根本不是发布端"误报成"帧格式不匹配"，根因误导）。
fn check_content_type(addr: &str, value: &str) -> Result<()> {
    let media = value.split(';').next().unwrap_or("").trim();
    if media.eq_ignore_ascii_case(JSON_CONTENT_TYPE) {
        return Ok(());
    }
    Err(io_err(
        addr,
        &format!("unexpected Content-Type `{media}` (expected `{JSON_CONTENT_TYPE}`)"),
    ))
}

/// 造一条 `InvalidData` 的 IO 错误（本模块不 panic，一律走 `Err`）。
fn io_err(addr: &str, msg: &str) -> Error {
    Error::Io(
        addr.to_string(),
        std::io::Error::new(ErrorKind::InvalidData, msg.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    // ── 断言的时钟基准：状态机只比较 `Instant`，故测试用「起拍时刻 + 固定步长」推进，
    //    无需真等（超时用例除外，见 `timeout_is_forced_at_the_deadline`）。

    #[test]
    fn endpoint_parse_full_url() {
        let e = ChannelEndpoint::parse("http://127.0.0.1:9810/v1/display/latest").unwrap();
        assert_eq!(e.host, "127.0.0.1");
        assert_eq!(e.port, 9810);
        assert_eq!(e.path, "/v1/display/latest");
    }

    #[test]
    fn endpoint_parse_defaults() {
        let e = ChannelEndpoint::parse("http://localhost/v1").unwrap();
        assert_eq!(e.host, "localhost");
        assert_eq!(e.port, 80);
        assert_eq!(e.path, "/v1");
    }

    #[test]
    fn endpoint_parse_rejects_non_http() {
        assert!(ChannelEndpoint::parse("https://127.0.0.1:1/x").is_err());
        assert!(ChannelEndpoint::parse("unix:///tmp/x").is_err());
    }

    /// **改什么会让本条变红**：把 `try_new` 里的 `endpoint.socket_addr()?` 换成不校验
    /// （如 `new()` 的静默回退）⇒ 这里拿到 `Ok` ⇒ 断言失败。
    #[test]
    fn client_rejects_non_ip_literal_host() {
        let e = DisplayChannelClient::try_new("http://localhost:9810/v1/display/latest")
            .unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("localhost"), "错误须点名非法 host：{msg}");
        assert!(msg.contains("DNS"), "错误须给出原因：{msg}");
    }

    #[test]
    fn client_default_points_at_loopback_publisher() {
        let c = DisplayChannelClient::default();
        assert_eq!(c.endpoint().host, "127.0.0.1");
        assert_eq!(c.endpoint().port, 9810);
        assert_eq!(c.endpoint().path, "/v1/display/latest");
        assert!(!c.is_busy());
        assert_eq!(c.phase_name(), "idle");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 桩服务端：本机真实 `TcpListener`（全平台可跑 —— 不依赖 tokio / evdev / fb0）
    // ═══════════════════════════════════════════════════════════════════════

    /// HTTP 桩：对第 `i`（0 基）个连接调 `reply(i, 请求报文)`；返回 `None` ⇒ **静默**
    /// （收下请求不回包、连接保持打开 —— 用于超时 / 不阻塞用例）。
    struct Stub {
        url: String,
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
                url: format!("http://127.0.0.1:{port}/v1/display/latest"),
                requests,
                accepted,
            }
        }

        fn client(&self) -> DisplayChannelClient {
            DisplayChannelClient::try_new(&self.url).expect("stub url")
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().map(|v| v.clone()).unwrap_or_default()
        }
    }

    /// 读到请求头结束（GET 无 body），避免与服务端 read 相互等待。
    fn read_request(sock: &mut std::net::TcpStream) -> std::io::Result<String> {
        let mut one = [0u8; 1];
        let mut req = Vec::new();
        loop {
            if sock.read_exact(&mut one).is_err() {
                return Ok(String::from_utf8_lossy(&req).into_owned());
            }
            req.push(one[0]);
            if req.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&req).into_owned())
    }

    /// 200 + JSON 帧响应（`Content-Length` 定长 + 声明 JSON）。
    fn ok_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {JSON_CONTENT_TYPE}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// 原始响应（可造畸形 Content-Length / Content-Type / 版本）。
    fn raw_response(head: &str, body: &str) -> String {
        format!("{head}{body}")
    }

    fn sample_frame_json() -> String {
        // v2 契约：`version` 必须等于 `PROTO_VERSION`（=2），否则帧被拒（设计 §3.5 条 1）。
        r#"{"version":2,"seq":7,"ts_ms":1700000000000,"soc":65.0,"soc_source":"pcs_reg1010",
            "soc_flag":"valid","run_state":2,"pcs_online":true,
            "p_phase":[{"v":12.3,"flag":"valid"},{"v":11.8,"flag":"valid"},{"v":12.0,"flag":"valid"}],
            "p_total":{"v":36.1,"flag":"valid"},
            "i_phase":[{"v":22.5,"flag":"valid"},{"v":22.1,"flag":"valid"},{"v":22.3,"flag":"valid"}],
            "inconsistency":false}"#
            .to_string()
    }

    /// 驱动用例的真实时间预算（**不是**性能阈值；失败形态是"对端不应答时挂死"）。
    ///
    /// 注入时钟自带上界（下述 `steps`），故本预算只兜"实现的真实等待" —— 真等满一个
    /// [`GET_TIMEOUT`] 也不会到 5 s（但下一个用例会用 1 s 的严格预算判它）。
    const DRIVE_BUDGET: Duration = Duration::from_secs(5);

    /// 推一趟直到结束（`now` 由起拍时刻 + 每步 1 ms 注入）。
    ///
    /// 双重有界（B3-2a 质量评审 建议 I-3；原实现每拍无条件 `sleep(1ms)`，把真实墙钟拖进了
    /// 每条用例的正常路径）：
    /// - **注入时钟**：步数取 `timeout − 1 ms` ⇒ 本函数**自己**永远不会产出 `Timeout`
    ///   （结论只能来自状态机推进，不再受"重载机器上先触 `Timeout`"的干扰）；
    /// - **真实时间**：[`DRIVE_BUDGET`] 兜"对端不应答时挂死"（同 `console.rs::drive` 的断言）。
    ///
    /// 让出 CPU 的形态：**每 8 拍**真 `sleep(1ms)`、其余 `yield_now()`。理由（实测）：
    /// 纯 `yield_now` 在 Windows 上等价 `SwitchToThread`（只在**本核**有就绪线程时才切换）
    /// ⇒ 连接工作线程可能整趟拿不到 CPU（`drive` 返回 `None` ⇒ 用例假红）；而**每拍都 sleep**
    /// 又把真实时间拖进正常路径（原实现的病）。睡眠只影响"工作线程何时被调度"，**不影响**
    /// 状态机判断（`now` 是注入的）⇒ 结论仍是确定性的。
    ///
    /// 返回最后一拍的产出；步数上限用尽仍 `Pending` ⇒ 返回 `None`（由调用方断言）。
    fn drive(c: &mut DisplayChannelClient, now0: Instant) -> Option<Result<DisplayFrame>> {
        let wall = Instant::now();
        let steps = (c.timeout().as_millis() as u64).saturating_sub(1).max(1);
        for i in 0..steps {
            match c.tick(now0 + Duration::from_millis(i)) {
                Progress::Pending => {}
                Progress::Done(r) => return Some(r),
            }
            assert!(
                wall.elapsed() < DRIVE_BUDGET,
                "驱动 {steps} 拍（注入时钟）超 {DRIVE_BUDGET:?} 真实预算未完成：\
                 当前阶段 {}（对端不应答不应让驱动线程挂住）",
                c.phase_name()
            );
            if i % 8 == 7 {
                std::thread::sleep(Duration::from_millis(1));
            } else {
                std::thread::yield_now();
            }
        }
        None
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 正常路径
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn tick_fetches_and_parses_frame_from_stub() {
        let stub = Stub::spawn(|_i, _req| Some(ok_response(&sample_frame_json())));
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        assert!(c.is_busy(), "发起后应有在途请求");
        let got = drive(&mut c, t0).expect("应在本拍序列内完成");
        let f = got.expect("桩回 200 + 合法帧");
        assert_eq!(f.seq, 7);
        assert_eq!(f.soc, Some(65.0));
        assert_eq!(f.run_state, Some(mupc_display_proto::RunState::Charge));
        assert_eq!(f.p_phase[0].v, Some(12.3));
        assert!(!c.is_busy(), "完成后应回到 idle");
        assert_eq!(c.fail_streak(), 0);
        // 请求行 / Host / 连接关闭语义（回环契约）
        let req = stub.requests().join("");
        assert!(req.starts_with("GET /v1/display/latest HTTP/1.1\r\n"), "{req}");
        assert!(req.contains("Host: 127.0.0.1:"), "{req}");
        assert!(req.contains("Connection: close"), "{req}");
        assert!(req.ends_with("\r\n\r\n"), "{req}");
        assert_eq!(stub.accepted.load(Ordering::SeqCst), 1);
    }

    /// 连续推进的拍数（判别预算的分子，见 [`NEVER_BLOCK_BUDGET`]）。
    const TICKS: usize = 2_000;

    /// "`tick` 不阻塞"的判别预算：1 s / [`TICKS`] 拍（判别力与裕度的完整说明见下条用例）。
    const NEVER_BLOCK_BUDGET: Duration = Duration::from_secs(1);

    /// **看门狗**：驱动体整体（含"进入读阶段"）超过它 ⇒ 判"挂死"（**不是**性能阈值）。
    ///
    /// 与 [`NEVER_BLOCK_BUDGET`] 分工：后者判**计时段**（2000 拍）的耗时，前者只负责把
    /// "永不返回"变成**红**而不是让 harness 挂住（`console.rs` 的 `HANG_GUARD` 同款口径）。
    const HANG_WATCHDOG: Duration = Duration::from_secs(10);

    /// "进入读阶段"的真实时间上界（连接由工作线程完成，正常为**亚毫秒**级）。
    const ENTER_READING_BUDGET: Duration = Duration::from_secs(3);

    /// **"`tick` 不阻塞"的判别预算 = 1 s / [`TICKS`] 拍**（B3-2a 质量评审 **重要 I-1** 整改）。
    ///
    /// # 这条判据抓什么、抓不到什么（**按实测口径如实写**，不得声称会红而实测不会红）
    ///
    /// 抓：
    /// 1. **挂死** —— `tick` 内出现无上界的同步等待（`rx.recv()` / 读到有数据为止 / socket 被
    ///    设回**阻塞**：`set_nonblocking(false)`）。对端持有连接且静默 ⇒ 阻塞读**永不返回**。
    ///    本用例把整个驱动体放进工作线程，主线程用 `recv_timeout` 看门狗收结果 ⇒ **挂死 = 红**
    ///    （而不是把 harness 拖到超时）。
    /// 2. **首拍耗掉整个超时** —— 把 `WouldBlock` 分支写成"自旋到截止"⇒ 第一拍就 ≥ [`GET_TIMEOUT`]
    ///    （2 s）≫ 1 s。
    /// 3. **`tick` 内 sleep** —— 休眠是**每拍**发生的：2 000 × 1 ms = 2 s > 1 s ⇒ 红
    ///    （这条旧版用 200 拍 × 150 ms 阈值也能抓，但旧版**测错了路径**，见下）。
    ///
    /// 抓不到（如实登记）：**每拍 ≤ 0.4 ms 的轻微等待**（2 000 × 0.4 ms = 0.8 s < 1 s）。
    /// 阈值必须留足以免在重载 CI 上假红（与 `console.rs::NEVER_BLOCK_BUDGET` 同口径：
    /// 真实实现为**亚毫秒~数毫秒**级，裕度 ≥ 10²）。
    ///
    /// # ⚠️ 旧版为什么**实测抓不到它点名的退化**（评审实测 3/3）
    /// 旧版一 `begin` 就开始计时，而此刻连接工作线程**还没把 socket 交回** ⇒
    /// `phase_name() == "connecting"`：200 拍只走了 `try_recv` 的空路径（实测 27.9/35.6/86 µs），
    /// 于是把 socket 设回阻塞后**仍然全绿**。本版**先驱动到 `phase_name() == "reading"`**
    /// （即 socket 已交回、请求已写出、真的在读），**再**开始计时 ⇒ 测的是读路径。
    #[test]
    fn tick_never_blocks_on_a_silent_peer() {
        let stub = Stub::spawn(|_i, _req| None); // 收下请求但永不回包，且**持有**连接
        let url = stub.url.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut c = DisplayChannelClient::try_new(&url).expect("client");
            let t0 = Instant::now();
            c.begin(t0).expect("begin");
            // ① **先进入读阶段**（此前 lib 侧还没拿到 socket ⇒ 测的是空路径，见上面的说明）。
            //    连接由工作线程完成，故这里必须真让出 CPU；注入时钟停在 500 ms（远未到截止）。
            let entry_wall = Instant::now();
            let mut entered = false;
            for i in 0..20_000u32 {
                if c.phase_name() == "reading" {
                    entered = true;
                    break;
                }
                let _ = c.tick(t0 + Duration::from_millis(500));
                if i % 8 == 7 {
                    // 见 `drive` 的说明：纯 `yield_now` 在 Windows 上可能整趟不切换线程。
                    std::thread::sleep(Duration::from_millis(1));
                } else {
                    std::thread::yield_now();
                }
                if entry_wall.elapsed() > ENTER_READING_BUDGET {
                    break;
                }
            }
            if !entered {
                let _ = tx.send(Err(format!(
                    "未能进入 reading 阶段（当前 {}，已等 {:?}）—— 用例前提不成立，结论无意义",
                    c.phase_name(),
                    entry_wall.elapsed()
                )));
                return;
            }
            // ② **从真正进入读阶段之后才开始计时**。注入时钟恒定（远未到 2 s 截止）
            //    ⇒ 唯一可能的耗时来自 tick 内的真实等待。
            let wall = Instant::now();
            let mut pends = 0usize;
            for _ in 0..TICKS {
                match c.tick(t0 + Duration::from_millis(500)) {
                    Progress::Pending => pends += 1,
                    Progress::Done(r) => {
                        let _ = tx.send(Err(format!("静默对端不得产出结果：{r:?}")));
                        return;
                    }
                }
            }
            let _ = tx.send(Ok((pends, wall.elapsed())));
        });
        // 看门狗：挂死（阻塞 read 永不返回）⇒ 收不到结果 ⇒ **红**（而不是 harness 超时）。
        match rx.recv_timeout(HANG_WATCHDOG) {
            Err(e) => panic!(
                "驱动体未在 {HANG_WATCHDOG:?}（看门狗）内返回：tick 内出现同步等待（{e}）；\
                 对端静默且持有连接时，阻塞读/写/connect 的失败形态是**永不返回**"
            ),
            Ok(Err(msg)) => panic!("{msg}"),
            Ok(Ok((pends, elapsed))) => {
                assert_eq!(pends, TICKS);
                assert!(
                    elapsed < NEVER_BLOCK_BUDGET,
                    "tick 内出现同步等待：{TICKS} 拍耗时 {elapsed:?} ≥ {NEVER_BLOCK_BUDGET:?}"
                );
            }
        }
    }

    /// 截止到期 ⇒ 明确 `Timeout`（**不是** `Pending` 拖到天荒地老）。
    #[test]
    fn timeout_is_forced_at_the_deadline() {
        let stub = Stub::spawn(|_i, _req| None);
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        // 注入时钟越过 `t0 + GET_TIMEOUT` ⇒ 第一拍即超时。
        let r = match c.tick(t0 + GET_TIMEOUT + Duration::from_millis(1)) {
            Progress::Done(r) => r,
            Progress::Pending => panic!("越过截止时刻必须触界（不得继续 Pending）"),
        };
        assert!(matches!(r, Err(Error::Timeout(_))), "got {r:?}");
        assert_eq!(c.fail_streak(), 1);
        assert!(!c.is_busy(), "超时后必须清空在途状态（否则永久 busy）");
    }

    /// 短超时 + 静默对端：**真实**等满超时后收 `Timeout`（验证截止是由注入时钟强制，
    /// 而不是靠对端行为）。
    ///
    /// 预算取 **1 s**（I-3：原为 120 ms，重载机器上"注入时钟已过期但真实时间还没走到"这类
    /// 抖动会改变结论）；真实时间的等待上界再放宽到 `deadline + 2 s` —— 判据是**结论类型**
    /// （`Timeout` 而非 `Pending`/`Io`），不是"多久返回"。
    #[test]
    fn short_timeout_expires_after_real_deadline() {
        let stub = Stub::spawn(|_i, _req| None);
        let budget = Duration::from_secs(1);
        let mut c = DisplayChannelClient::with_timeout(&stub.url, budget).expect("client");
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let deadline = t0 + budget;
        let mut out = None;
        while Instant::now() < deadline + Duration::from_secs(2) {
            if let Progress::Done(r) = c.tick(Instant::now()) {
                out = Some(r);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let r = out.expect("应在超时后结束");
        assert!(matches!(r, Err(Error::Timeout(_))), "got {r:?}");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 错误分类（每条都有明确出口，不静默吞）
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn http_503_is_error_not_empty_frame() {
        let stub = Stub::spawn(|_i, _req| {
            Some(
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string(),
            )
        });
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(matches!(r, Err(Error::HttpStatus(503, _))), "got {r:?}");
        assert_eq!(c.fail_streak(), 1);
    }

    /// 端点契约：错误路径 → 404 → 明确 `HttpStatus`（不静默当作有效帧）。
    #[test]
    fn wrong_path_404_is_error() {
        let stub = Stub::spawn(|_i, req| {
            let ok = req.starts_with("GET /v1/display/latest ");
            Some(if ok {
                ok_response(&sample_frame_json())
            } else {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_string()
            })
        });
        let mut c = DisplayChannelClient::try_new(&stub.url.replace("/latest", "/wrong"))
            .expect("client");
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(matches!(r, Err(Error::HttpStatus(404, _))), "got {r:?}");
    }

    #[test]
    fn connection_refused_is_connect_error() {
        // **端口 0**：`connect` 被当场拒绝（Windows `WSAEADDRNOTAVAIL`、Linux
        // `EADDRNOTAVAIL`/`ECONNREFUSED`）⇒ 确定性地走 `Connect` 分支。
        //
        // 为什么**不**用「bind 一个临时端口再 drop」：那依赖"拒绝在几秒内回包"，而 Windows 对
        // 无监听端口的 SYN 回包**在机器有负载时可拖到数秒**（`console.rs` 同类用例已注过这一条）
        // ⇒ 会被 5 s 截止掩盖成 `Timeout`，成为**偶发红**（同目录 `console.rs` 的同型用例在
        // 并发压力下实测确有偶发）。端口 0 无此依赖，也不与其它用例抢端口。
        let mut c = DisplayChannelClient::with_timeout(
            "http://127.0.0.1:0/v1/display/latest",
            Duration::from_secs(5),
        )
        .expect("client");
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(matches!(r, Err(Error::Connect(_, _))), "got {r:?}");
    }

    /// W3：帧 `version` 与 `PROTO_VERSION` 不符 ⇒ `Err(ProtoVersion)`（不静默按旧语义展示）。
    #[test]
    fn proto_version_mismatch_is_error() {
        let body = sample_frame_json().replace("\"version\":2", "\"version\":99");
        let stub = Stub::spawn(move |_i, _req| Some(ok_response(&body)));
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(matches!(r, Err(Error::ProtoVersion(_, 99, _))), "got {r:?}");
        assert_eq!(c.fail_streak(), 1);
    }

    /// W2：`Content-Length` 超上限 ⇒ **在读到任何体字节之前**就拒绝。
    ///
    /// **判据的判别力**：桩只发头、**永不发体**（且发完即关连接）。少了预检时实现会先记下
    /// 这个巨额长度、再去补体字节，于是只能拿到"体没读完 / 读不到"那类 **Io** 结论 ——
    /// 与 `BodyTooLarge` 是**两类**结论。
    ///
    /// **改什么会让本条变红**（实测）：把 `parse_head` 里的 `if n > MAX_BODY_BYTES` 预检删掉
    /// ⇒ 实得 `Err(Io(.., "connection closed before Content-Length was satisfied"))`
    /// （桩发完头就关连接；`BodyTooLarge` 不再产生）。
    ///
    /// 超时预算取 **5 s**（I-3：原为 300 ms ⇒ 重载机器上可能先触 `Timeout` 而非 `BodyTooLarge`，
    /// 判据变成"机器快不快"）。配合 `drive` 的注入时钟上界，本条现在**结构上不可能**产出
    /// `Timeout` —— 结论只可能来自 `parse_head` 的预检。
    #[test]
    fn oversized_content_length_rejected_before_reading_body() {
        let huge = MAX_BODY_BYTES + 1;
        let stub = Stub::spawn(move |_i, _req| {
            Some(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {JSON_CONTENT_TYPE}\r\nContent-Length: {huge}\r\nConnection: close\r\n\r\n"
            ))
        });
        let mut c = DisplayChannelClient::with_timeout(&stub.url, Duration::from_secs(5))
            .expect("client");
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(
            matches!(r, Err(Error::BodyTooLarge(_, n)) if n == huge),
            "超限 Content-Length 应 Err(BodyTooLarge) 而非 Timeout，实际: {r:?}"
        );
    }

    /// W2：无 `Content-Length` 时读到 EOF 亦按上限封顶（防无限流）。
    #[test]
    fn oversized_body_without_content_length_is_capped() {
        let stub = Stub::spawn(|_i, _req| {
            Some(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {JSON_CONTENT_TYPE}\r\nConnection: close\r\n\r\n{}",
                "x".repeat(MAX_BODY_BYTES + 10)
            ))
        });
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(matches!(r, Err(Error::BodyTooLarge(_, _))), "got {r:?}");
    }

    /// 响应头超限 ⇒ `HeadTooLarge`（防对端用无穷头撑爆内存）。
    #[test]
    fn oversized_head_is_rejected() {
        let stub = Stub::spawn(|_i, _req| {
            let mut head = String::from("HTTP/1.1 200 OK\r\n");
            let filler = format!("X-Filler: {}\r\n", "y".repeat(1000));
            while head.len() <= MAX_HEAD_BYTES + READ_CHUNK {
                head.push_str(&filler);
            }
            Some(format!("{head}Content-Length: 0\r\n\r\n"))
        });
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(matches!(r, Err(Error::HeadTooLarge(_, _))), "got {r:?}");
    }

    /// **声明的** `Content-Type` 非 JSON ⇒ 响亮失败（把 HTML 错误页当 JSON 解码会误导根因）。
    #[test]
    fn declared_non_json_content_type_is_rejected() {
        let stub = Stub::spawn(|_i, _req| {
            Some(raw_response(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5\r\nConnection: close\r\n\r\n",
                "<html",
            ))
        });
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(matches!(r, Err(Error::Io(_, _))), "got {r:?}");
        assert!(r.unwrap_err().to_string().contains("Content-Type"));
    }

    /// **未声明** `Content-Type` ⇒ 放行（最小桩仍可用；body 仍过 JSON + 版本两道关）。
    ///
    /// **改什么会让本条变红**：把校验从"声明了才查"改成"必须存在" ⇒ 本条 `Err`。
    #[test]
    fn absent_content_type_is_tolerated() {
        let stub = Stub::spawn(|_i, _req| {
            let body = sample_frame_json();
            Some(raw_response(
                &format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()),
                &body,
            ))
        });
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert_eq!(r.expect("无 Content-Type 时应正常解析").seq, 7);
    }

    /// 带参数与大小写变体的 JSON 媒体类型 ⇒ 放行（`; charset=utf-8` / `APPLICATION/JSON`）。
    #[test]
    fn json_content_type_with_parameters_is_accepted() {
        let stub = Stub::spawn(|_i, _req| {
            let body = sample_frame_json();
            Some(raw_response(
                &format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: APPLICATION/JSON; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                ),
                &body,
            ))
        });
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert_eq!(r.expect("带参数的 JSON 类型应放行").seq, 7);
    }

    #[test]
    fn malformed_json_is_error() {
        let stub = Stub::spawn(|_i, _req| Some(ok_response("{not json")));
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let r = drive(&mut c, t0).expect("should finish");
        assert!(matches!(r, Err(Error::Json(_, _))), "got {r:?}");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 并发 / 生命周期 / 纯逻辑
    // ═══════════════════════════════════════════════════════════════════════

    /// 同一客户端**至多一条在飞**：重复 `begin` 响亮失败（`Busy`），不静默排队。
    #[test]
    fn begin_twice_is_busy() {
        let stub = Stub::spawn(|_i, _req| None);
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("first begin");
        let e = c.begin(t0).unwrap_err();
        assert!(matches!(e, Error::Busy(_)), "got {e:?}");
        assert!(e.to_string().contains("127.0.0.1"));
    }

    /// `cancel` 丢弃在途状态（幂等 no-op），此后可立即重新 `begin`。
    #[test]
    fn cancel_discards_inflight_and_allows_rebegin() {
        let stub = Stub::spawn(|_i, _req| None);
        let mut c = stub.client();
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        assert!(c.is_busy());
        c.cancel();
        assert!(!c.is_busy());
        c.cancel(); // 幂等
        assert!(!c.is_busy());
        assert_eq!(c.tick(t0).len_hint(), 0); // 无在途 ⇒ Pending（见 len_hint 说明）
        c.begin(t0).expect("cancel 后必须能重新发起");
    }

    /// `Progress` 的判别辅助（测试内：`Pending` ⇒ 0，`Done` ⇒ 1）——避免在本文件里
    /// 为断言写 `matches!` 的重复样板。
    trait LenHint {
        fn len_hint(&self) -> usize;
    }
    impl<T> LenHint for Progress<T> {
        fn len_hint(&self) -> usize {
            match self {
                Progress::Pending => 0,
                Progress::Done(_) => 1,
            }
        }
    }

    /// **写阶段记账**（纯函数；真 socket 上触发不了部分写，见 `note_written` 文档）。
    ///
    /// **改什么会让本条变红**：把 `saturating_add` 改成直接赋值 (`*sent = n`)
    /// ⇒ 部分写用例拿到 `false`（报文被判"尚未写完"或被静默截断）。
    #[test]
    fn write_accounting_handles_partial_writes() {
        let mut sent = 0usize;
        // 部分写：未写完
        assert!(!note_written(&mut sent, 3, 10));
        assert_eq!(sent, 3);
        assert!(!note_written(&mut sent, 4, 10));
        assert_eq!(sent, 7, "必须**累加**，不能直接赋值");
        // 恰好写完
        assert!(note_written(&mut sent, 3, 10));
        assert_eq!(sent, 10);
        // 一次写完
        let mut s2 = 0usize;
        assert!(note_written(&mut s2, 10, 10));
        // 内核报出比剩余更多（不可能情形）：不 panic、不绕回
        let mut s3 = usize::MAX - 1;
        assert!(note_written(&mut s3, 5, 10));
    }

    /// 节拍判定（纯逻辑）：首拍立即发起；此后按 `--poll-ms` 节拍。
    #[test]
    fn poll_due_fires_immediately_then_on_interval() {
        assert!(poll_due(0, None), "首拍不等节拍");
        assert!(!poll_due(499, Some(500)));
        assert!(poll_due(500, Some(500)), "到点即发起");
        assert!(poll_due(501, Some(500)), "迟到也发起（不变死锁）");
        assert_eq!(next_poll_at(500, 500), 1000);
        assert_eq!(next_poll_at(u64::MAX, 500), u64::MAX, "饱和加法不 panic");
    }

    /// 失败的通道请求必须**上抛给状态层**（`DisplayState` 累计失败并在无成功 >3s 时切断开态）。
    ///
    /// 失败形态取 `Connect`（端口 0 ⇒ 当场拒绝，见
    /// [`connection_refused_is_connect_error`](self)）；其余分支（`Timeout` / `HttpStatus` /
    /// `Json` / `ProtoVersion`）也各自有用例 ⇒ 状态层看到的是"任一 `Err`"，与 v1.0
    /// `DisplayState::record_fail` 的口径逐条一致。
    ///
    /// **改什么会让本条变红**（实测）：把 `tick` 的失败分支改成"静默吞掉"（返回 `Pending`）⇒
    /// 驱动最终无结论 ⇒ `drive` 返回 `None` ⇒ `expect("should finish")` 红。
    ///
    /// ⚠️ **口径订正（B3-2a 质量评审 建议 I-5）**：本条原先经 `DisplayState::update`
    /// （`Ok/Err` 转发）落状态层，而该转发**生产零调用**（`App::absorb` 必须自己匹配 `Result`
    /// 才能分别累计 `frames_ok`/`frames_fail`）⇒ `update` 已删。本条**拆成两句各守一半**：
    /// ① 传输层确实产出 `Err`；② 该 `Err` 的语义后果（`record_fail` ⇒ 「通道断」展示态）。
    #[test]
    fn transport_failures_surface_for_channel_down_state() {
        let mut c = DisplayChannelClient::with_timeout(
            "http://127.0.0.1:0/v1/display/latest",
            Duration::from_secs(5),
        )
        .expect("client");
        let t0 = Instant::now();
        c.begin(t0).expect("begin");
        let res = drive(&mut c, t0).expect("should finish");
        // ① 传输层必须把失败**上抛**（不得静默吞成"空帧"）。
        let e = res.expect_err("端口 0 必被拒绝 ⇒ 必须是 Err");
        assert!(matches!(e, Error::Connect(_, _)), "got {e:?}");
        // ② 失败的状态层后果：累计失败 + 无成功超过 3 s ⇒ 「与主进程数据通道断开」。
        let mut st = crate::state::DisplayState::new();
        st.record_fail(0);
        assert_eq!(st.fail_streak(), 1);
        assert_eq!(
            st.screen_mode(crate::state::CHANNEL_DOWN_MS),
            crate::state::ScreenMode::ChannelDown
        );
    }
}
