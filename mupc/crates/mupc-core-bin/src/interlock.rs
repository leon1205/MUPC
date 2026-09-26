//! 安全联锁状态机（纯逻辑，便于单测）。
//!
//! 输入 = 各 DI **去抖后有效态** + PCS run_state/在线；输出 = latch 决策 + DO1/DO2
//! 目标电平（active 高低电平由装配换算，本模块只输出逻辑电平）。
//!
//! 语义出处：核间通信设计文档 §12.3（DI/DO 安全联锁 BECG-3568）/ §12.4（`io:` 配置段）。
//!
//! - pcs_stop 源（急停 Estop / 水浸 Flood / 消防 Fire）**有效沿** → 触发 latch
//!   （返回 `Action::TriggerLatch`，装配负责 PCS 停机指令 + DB 持久化 restore）。
//! - 释放须：触发源全部复位 && 保持 >= `release_hold_secs`；`auto_release=false` 时还须外部
//!   `request_release`（`web_release_pending`），`auto_release=true` 则保持后自动释放——
//!   **但自动释放要求停机已确认（`!stop_failed`）**，stop() 未确认（PCS 仍运行）时只许人工
//!   web release 放行，防联锁静默失效。
//! - 门禁 Door 仅上报事件，不计入 pcs_stop 源集合（装配发事件，不参与 latch）。
//! - DO1 运行灯 = run_state∈{1,2,3} 且 !latch；DO2 故障灯 = 持久条件
//!   （latch || stop_failed || !pcs_online || M1 停机）。
//! - fail-safe：GPIO 读失败由装配按"触发态"处理（本模块只吃 bool 有效态 tripped）。
//!
//! 去抖：raw+active_low → tripped 换算及简单去抖由装配完成（`DiConf.debounce`），本机
//! 内部仅对 pcs_stop 源做**二次边沿判定**（`prev` 字段跨 tick 保持上拍生效集）。
//! 职责划分：本机 tick 直接更新 `InterlockState` 的源/安全记时字段，命令
//! （Trigger/Release）返回给装配执行 transport/DB 动作——状态与动作分离，测试只断言
//! `s` + `Action`。
//!
//! ── Runner / 装配层（Task 7）──
//! 文件后半为 `InterlockController`：把纯逻辑接到真实世界。语义/取舍见其模块 doc 与
//! 各方法注释（配置层 vs 状态机的差异、name→source 启发式、fail-safe 处理均记录在此）。

use async_trait::async_trait;
use mupc_io::{DigitalIn, DigitalOut, IoError, SysfsIn, SysfsOut};
use mupc_southd::pcs::PcsHandle;
use mupc_storage::{EventRepository, SystemEvent};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

// 联锁读写接口 + 结构化拒绝原因 —— **迁移到契约**（设计 §4.6，单元 J）。
//
// 迁移前：`use mupc_web_api::app_state::{InterlockApi, ...}`，错误是 `Result<(), String>`
// （字符串只够打日志、不够驱动 UI；EDGE-12 / IL-02 / IL-03 要求「明示**具体**拒绝原因」）。
// 迁移后：
// - 视图 = `InterlockView`（即帧内 `interlock` 段的**同一类型**，契约单一真源防双源漂移）；
//   `InterlockStatus` / `InterlockSourceStatus` 是契约里的迁移别名 ⇒ 既有调用点 `use` 不变；
// - 错误 = `InterlockReject`，**每条拒绝路径逐一映射到具体变体**（见 `do_request_release` /
//   `ack_m1` 的「拒绝路径 → 变体」对照表）。
//
// ⚠️ **单元 K 已完成收口**：web-api 的旧 trait 与装配层的过渡适配器**都已删除**，本模块现在
// **只**实现契约 trait（上面那个 `use` 就是全部实现面）——不再存在"同一份逻辑两套签名"。
use mupc_display_proto::{InterlockApi, InterlockReject, InterlockSourceStatus, InterlockStatus};

use crate::core_config::IoConfig;

/// 数字输入触发源。门禁仅产生事件，不计入 pcs_stop 源集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterlockSource {
    /// 急停（pcs_stop）
    Estop,
    /// 水浸（pcs_stop）
    Flood,
    /// 消防（pcs_stop）
    Fire,
    /// 门禁（event，仅上报，不触发 latch）
    Door,
}

/// 故障灯原因（诊断/事件上报用，DO2 输出目标另见 `tick` 返回值 fault_lamp）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultReason {
    /// 无故障
    None,
    /// 联锁触发（latch）或停机失败（stop_failed）
    Interlock,
    /// PCS 链路离线
    PcsOffline,
    /// M1：PCS 在线但 run_state 非 {1,2,3}（未 latch 时）
    M1Stopped,
}

/// tick 一次后需要装配执行的命令（transport/DB 动作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 无需动作
    None,
    /// 触发联锁：装配发 PCS 停机指令 + DB 持久化 latch（restore true）
    TriggerLatch,
    /// 释放联锁：装配清 DB latch（restore false）等
    ReleaseLatch,
}

/// 共享联锁状态（跨 tick 持久，测试可直接构造/断言）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterlockState {
    /// 是否处于联锁（禁启）锁存态
    pub latched: bool,
    /// PCS 停机确认失败（由装配置位；停机确认成功由装配清除——本机 auto 释放要求 !stop_failed，
    /// 仅人工 web release 可放行 stop_failed 态；新触发沿复位重试）
    pub stop_failed: bool,
    /// 本拍仍处触发态的 pcs_stop 源位图（Estop=1 / Flood=2 / Fire=4）
    pub active_pcs_stop_sources: u32,
    /// 全部触发源复位后的 tick（秒）；复位前为 None
    pub source_safe_since: Option<u64>,
    /// 本拍故障灯原因（DO2 诊断）
    pub last_fault_lamp_reason: FaultReason,
}

impl Default for InterlockState {
    fn default() -> Self {
        Self {
            latched: false,
            stop_failed: false,
            active_pcs_stop_sources: 0,
            source_safe_since: None,
            last_fault_lamp_reason: FaultReason::None,
        }
    }
}

/// pcs_stop 源位图常量
const BIT_ESTOP: u32 = 1;
const BIT_FLOOD: u32 = 2;
const BIT_FIRE: u32 = 4;

/// 源 → 位图；门禁无位（不计入 pcs_stop 集合）。
fn source_bit(src: InterlockSource) -> Option<u32> {
    match src {
        InterlockSource::Estop => Some(BIT_ESTOP),
        InterlockSource::Flood => Some(BIT_FLOOD),
        InterlockSource::Fire => Some(BIT_FIRE),
        InterlockSource::Door => None,
    }
}

/// 安全联锁状态机（纯逻辑，可 `std::thread` 单测，不依赖 tokio/transport）。
pub struct StateMachine {
    /// 触发源回安全态并保持 ≥ release_hold_secs 后是否自动释放（false=须人工 web 确认）
    pub auto_release: bool,
    /// 触发源全复位后须保持的时长（秒），人工解除前置校验
    pub release_hold_secs: u64,
    /// 上拍 pcs_stop 生效源位图（二次边沿判定，跨 tick 保持）
    prev: u32,
}

impl StateMachine {
    pub fn new(auto_release: bool, release_hold_secs: u64) -> Self {
        Self {
            auto_release,
            release_hold_secs,
            prev: 0,
        }
    }

    /// tick 一次。
    ///
    /// 入参：
    /// - `s`：可变共享状态（本机直接记账 latch/源集/安全计时）
    /// - `tripped`：各源**去抖后有效态**（tripped=true 表示该源处触发态）
    /// - `web_release_pending`：人工 release 请求是否待处理（HTTP 置位，装配消费）
    /// - `run_state_ok`：PCS run_state ∈ {1,2,3}（装配由 last_run_state 换算）
    /// - `pcs_online`：PCS 链路在线
    /// - `now`：当前秒级时间
    ///
    /// 返回 `(Action, run_lamp, fault_lamp)`——DO1/DO2 两灯各自目标电平。
    pub fn tick(
        &mut self,
        s: &mut InterlockState,
        tripped: &[(InterlockSource, bool)],
        web_release_pending: bool,
        run_state_ok: bool,
        pcs_online: bool,
        now: u64,
    ) -> (Action, bool, bool) {
        // ── 1. pcs_stop 源本拍生效集 + 触发沿（门禁不入集）──
        let mut cur: u32 = 0;
        for (src, tr) in tripped {
            if let Some(b) = source_bit(*src) {
                if *tr {
                    cur |= b;
                }
            }
        }
        // 触发沿 = 本拍新增位（上拍 prev 未含）
        let new_bits = cur & !self.prev;
        self.prev = cur;
        s.active_pcs_stop_sources = cur;

        let mut action = Action::None;

        if !s.latched && new_bits != 0 {
            // pcs_stop 源首次有效（有效沿）→ 触发 latch；装配执行 PCS 停机 + DB 持久化
            s.latched = true;
            s.stop_failed = false;
            s.source_safe_since = None;
            action = Action::TriggerLatch;
        } else if s.latched {
            if cur == 0 {
                // 触发源全复位：记录安全起点
                if s.source_safe_since.is_none() {
                    s.source_safe_since = Some(now);
                }
                let held = now.saturating_sub(s.source_safe_since.unwrap_or(now))
                    >= self.release_hold_secs;
                // 释放门槛：人工 web 确认（操作员明确放行，可容忍 stop_failed 态），或
                // auto_release **且停机已确认**（!stop_failed）。stop_failed 由装配在停机确认
                // **超时**后置位（stop_confirm_ms 内未转 0）；一旦置位即不再自动解 latch——
                // 否则联锁静默失效（源复位即自动复位，故障灯熄灭但 PCS 从未真正停机）。
                // 确认窗口内的自动释放由装配超时兜底（超时即置 stop_failed）。
                let manual_ok = web_release_pending;
                let auto_ok = self.auto_release && !s.stop_failed;
                if held && (manual_ok || auto_ok) {
                    s.latched = false;
                    s.stop_failed = false;
                    s.source_safe_since = None;
                    action = Action::ReleaseLatch;
                }
                // else：维持 latch（等待 hold 到达 / 人工确认 / 停机确认）
            } else {
                // 仍有触发源 active（含 latched 期间重新触发）→ 重置安全计时，保持 latch
                s.source_safe_since = None;
            }
        }
        // !s.latched 且 new_bits==0：无 pcs_stop 源新触发，无事可做（保持释放态）

        // ── 2. DO1/DO2 决策 ──
        // M1：在线但 run_state 非 {1,2,3} 且未 latch（离线不计 M1，因离线已由 PcsOffline 亮故障）
        let m1 = pcs_online && !run_state_ok && !s.latched;
        let fault = s.latched || s.stop_failed || !pcs_online || m1;

        s.last_fault_lamp_reason = if s.latched || s.stop_failed {
            FaultReason::Interlock
        } else if !pcs_online {
            FaultReason::PcsOffline
        } else if m1 {
            FaultReason::M1Stopped
        } else {
            FaultReason::None
        };

        let run_lamp = run_state_ok && !s.latched;
        let fault_lamp = fault;

        (action, run_lamp, fault_lamp)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Runner / 装配层 —— 安全联锁控制器（Task 7，S2）
// ═══════════════════════════════════════════════════════════════════════════
// 把上方纯逻辑状态机接到真实世界：DI(active_low + per-DI 去抖) → tick → Action →
// InterlockPort(停机/锁存/重启授权) + DO(运行/故障灯) + DB 事件(events) + SSE。
//
// 设计取舍（记录依据/局限，供 spec/quality review 裁决）：
// 1) name→source 启发式：io schema `DiConf` 只有 action 字段（pcs_stop/event），没有
//    Estop/Flood/Fire 三源区分（Task4 schema 已双审，不新增字段）。controller 按 DI name
//    含词启发式映射（见 `classify_source`），其它 pcs_stop 一律归 Estop——局限：仅能按
//    通道命名约定区分水浸/消防，模板（§12.4 di1=急停/di2=水浸/di3=消防）天然满足。
// 2) pcs_online := `port.last_run_state().is_some()`（Task1 无独立在线查询；有心跳值=在线）。
// 3) DO 顺序约定：do_out[0]=运行灯、do_out[1]=故障灯（§12.4 模板即此序；配置无 role 字段）。
// 4) request_release **同步执行**（前置校验 + 清 latch）：HTTP 操作员点按钮即时生效，比等
//    runner 下一帧直觉；状态机的 web_release_pending 沿由纯逻辑单测覆盖，runner 不引入新沿
//    （状态机 auto_release=false 路径与同步释放等价，且 stop_failed 人工放行有审计事件）。
// 5) GPIO init 失败：不 panic、不静默、绝无「无 latch 运行」——单个 DI/DO 打开失败用
//    fail-safe stub 占位（pcs_stop DI → 恒读失败 → 按触发处理；DO → no-op），且任一 pcs_stop
//    DI 失败即把 InterlockState 预置 latched=true + stop_failed=true（视触发+停机未确认），
//    启动记 gpio_init_failed 事件。局限：失败的 DI 恒触发 → 只能重启或修好硬件后 reboot 清。
// 6) GPIO 读失败按触发处理（§12.3）；每个失败源首次记一次事件防刷屏（连续失败只报一次）。
// 7) 停机确认（**逐帧非阻塞**，S2 Task7 Important 修复）：触发沿只发**一次** stop() 总线写并记录
//    last_stop_attempt；不做内联自旋轮询（旧版自旋至 stop_confirm_ms 会整条阻塞 run_loop，期间
//    DI/DO/门禁不采样）。确认/超时/重试全部由 `post_stop_maintenance` 每帧（poll_ms）检查：
//    run 转 Some(0) → clear_stop_failed_once（延迟确认清除）；距上次尝试 ≥ stop_confirm_ms 且
//    仍未确认 → 首次置 state.stop_failed（幂等，false→true 记一次事件）+ 重发单次 stop。tcp 仿真
//    通道 `last_run_state` 恒 None 无法确认 → 会置 stop_failed（fail-safe 保守；仿真不用作生产联锁）。
// 8) DB 读回：启动时 latest_by_type("interlock.triggered") 比 ("interlock.cleared")
//    的 timestamp——最新 triggered 且无后续 cleared → restore latched（置 stop_failed=true
//    表示停机未确认，post_stop_maintenance 会补发停机并确认）。
//
// 测试性取舍：GPIO 用 `Box<dyn DigitalIn/Out>` 注入（mupc_io::MockIn/Out 或本地 stub）；
// transport 用薄 trait `InterlockPort`（Arc<PcsHandle> 真机转发，测试注入 fake 记调用）；
// DB/告警投递用真实 trait 对象 `Arc<dyn EventRepository>` / `Arc<AlertFeed>`（测试注入内存 fake）。
// ⚠️ 单元 K：原 `Arc<SsePushService>`（web-api）已换成 `crate::alert_feed::AlertFeed`
//    ——事件名与文案**逐字不变**，只是换了承载（设计 §4.7）。

/// 触发动作映射（与 `DiConf.action` 对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiAction {
    /// pcs_stop：急停/水浸/消防（触发 latch）
    PcsStop,
    /// event：门禁等仅上报事件
    Event,
}

fn action_of(s: &str) -> DiAction {
    if s == "event" {
        DiAction::Event
    } else {
        DiAction::PcsStop
    }
}

/// name→source 启发式（取舍 1）。event → Door（仅事件）；pcs_stop 按 name 含词归类，
/// 无法识别的一律归 Estop（急停语义最保守）。局限：依赖通道命名约定，见模块 doc。
fn classify_source(name: &str, action: &str) -> InterlockSource {
    if action == "event" {
        return InterlockSource::Door;
    }
    let n = name.to_lowercase();
    if n.contains("水浸") || n.contains("淹") || n.contains("flood") {
        InterlockSource::Flood
    } else if n.contains("消防") || n.contains("火警") || n.contains("烟") || n.contains("fire")
    {
        InterlockSource::Fire
    } else {
        InterlockSource::Estop
    }
}

/// 源 → web 展示 token（estop/flood/fire/door）
fn source_token(s: InterlockSource) -> &'static str {
    match s {
        InterlockSource::Estop => "estop",
        InterlockSource::Flood => "flood",
        InterlockSource::Fire => "fire",
        InterlockSource::Door => "door",
    }
}

/// 当前 epoch 秒（状态机 tick 的 now）
fn now_secs() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

/// runner 依赖薄 trait（可测）：由装配方把 `Arc<PcsHandle>` 适配进来。
/// 真机实现见 `impl InterlockPort for Arc<PcsHandle>`；测试注入 fake 记调用序列。
///
/// ⚠️ **trait 的 4 个方法签名与状态机本体零改动**（Task 10 / 设计 §13.5.2）：适配器只换了
/// 被适配的门面类型（`intercore::IntercoreClient` → `southd::pcs::PcsHandle`），方法名/语义
/// 一一对应（`stop` / `restore_interlock_latched` / `last_run_state` / `authorize_restart`）。
#[async_trait]
pub trait InterlockPort: Send + Sync {
    /// PCS 停机（Modbus 写 REG_START_STOP=0；Tcp no-op）
    async fn stop(&self) -> Result<(), String>;
    /// 置/清联锁 latch（C-1 唯一入口；transport 运行期挡启动）
    async fn restore_latched(&self, latched: bool) -> Result<(), String>;
    /// 最新解码 RUN_STATE(1013)；离线 None
    fn last_run_state(&self) -> Option<u16>;
    /// M1 保护跳闸/停机人工授权重启（单次）；latch 期间 Err
    async fn authorize_restart(&self) -> Result<(), String>;
}

#[async_trait]
impl InterlockPort for Arc<PcsHandle> {
    async fn stop(&self) -> Result<(), String> {
        (**self).stop().await
    }
    async fn restore_latched(&self, latched: bool) -> Result<(), String> {
        (**self).restore_interlock_latched(latched).await
    }
    fn last_run_state(&self) -> Option<u16> {
        (**self).last_run_state()
    }
    async fn authorize_restart(&self) -> Result<(), String> {
        (**self).authorize_restart().await
    }
}

/// fail-safe：DI GPIO 打开失败/运行时读失败用的占位——恒返回 Err → runner 恒按触发处理。
/// 使「初始化失败」也绝不静默落入无 latch 运行。
struct DiReadFailStub;
impl DigitalIn for DiReadFailStub {
    fn read_level(&self) -> Result<bool, IoError> {
        Err(IoError::Io(
            "interlock.di(fail-safe)".into(),
            "DI GPIO 不可用，恒按触发处理".into(),
        ))
    }
}

/// fail-safe：DO GPIO 打开失败占位——no-op（灯不可点亮属降级，非安全链；构造时已记事件）。
struct DoNoopStub;
impl DigitalOut for DoNoopStub {
    fn set_level(&self, _high: bool) -> Result<(), IoError> {
        Ok(())
    }
}

/// runner 帧内可变运行数据（跨 tick 保持；与共享 `InterlockState` 分离，避免与 web 读竞争）
struct DiRuntime {
    /// 各 DI 连续有效计数（per-DI 去抖，§12.3）
    debounce: Vec<u32>,
    /// 上拍门禁/event DI 生效态（沿判定：上拍低→本拍高 = 触发事件）
    door_prev: Vec<bool>,
    /// 该 DI 是否已报过「读失败」事件（防每帧刷屏）
    read_fail_logged: Vec<bool>,
    /// 上次停机写尝试时刻：post_stop_maintenance 据此逐帧判定——距上次 ≥ stop_confirm_ms 且未确认
    /// → 超时置 stop_failed + 退避重发；None（如 DB 读回 latch 首帧，本进程未发过停机）→ 补发首停。
    last_stop_attempt: Option<Instant>,
}

/// 安全联锁控制器（runner + 契约 `InterlockApi` 实现）
pub struct InterlockController {
    cfg: IoConfig,
    /// 共享联锁状态（runner 写 / 控制通道读 / dispatch 抑制读）
    state: Arc<RwLock<InterlockState>>,
    /// 纯逻辑状态机（prev 边沿跨 tick；`request_release` 同步清 latch 时会重置以防旧沿残留）
    sm: Mutex<StateMachine>,
    /// 各 DI（与 cfg.di 对齐；含 fail-safe stub）
    ins: Vec<Box<dyn DigitalIn>>,
    /// 各 DO（与 cfg.do_out 对齐；含 no-op stub）
    outs: Vec<Box<dyn DigitalOut>>,
    /// PCS 停机/锁存/重启授权（真机 `Arc<PcsHandle>`，测试 fake）
    port: Box<dyn InterlockPort>,
    /// 事件落库（DB；测试注入内存 fake）
    events: Arc<dyn EventRepository>,
    /// 告警即时投递（联锁 major 事件；⚠️ 非 F7 真源，见 `alert_feed` 模块头）
    alert_feed: Arc<crate::alert_feed::AlertFeed>,
    /// 帧内可变数据（去抖/门禁沿/退避）
    runtime: Mutex<DiRuntime>,
    /// 构造期 GPIO 初始化失败待上报事件（run_loop 启动时 flush）
    pending_init_events: Mutex<Vec<(String, String)>>,
    /// P2-1：fail-safe init 预置标记——`new()` 同步构造只能预置本地 `state.latched`；transport
    /// `restore_latched(true)` 与 DB triggered（重启读回依据）是 async，延至 `run_loop` 首帧
    /// `sync_failsafe_latch()` 补做（C-1 双 latch 兜底）。
    failsafe_preset: Mutex<bool>,
    /// **写操作互斥位**（单元 J）：`release` / `ack_m1` 二者共用一把"有人在写"的闸。
    ///
    /// 为什么需要：两条写路径都是 async（`restore_latched` / `authorize_restart` 有 await 点），
    /// 控制通道是**多 worker** 的 axum ⇒ 两个并发请求会在 await 点交错，可能"看着同一个 before
    /// 各写一次"（例如 release 清 latch 与 ack_m1 授权重启并发）。闸被占用时后到者**立刻**回
    /// [`InterlockReject::Busy`]（不排队、不改任何状态）——与幂等表的 `Busy`（同 `request_id`
    /// 重放）是**两件事**：这里拦的是"不同请求的并发"。
    ///
    /// ⚠️ **限度（A-1，独立评审，必须说清）：这把闸只互斥「写 × 写」。** 本控制器的并发**不只**
    /// 这一族——`run_loop` / `tick_frame`（runner）在**相邻任务**里跑、**不占**这把闸，却同样读写
    /// 共享状态（`tick` 会置/清 `latched`、`on_trigger` 会写 transport）。故任何**跨 await 的
    /// check-then-act**（"读共享态 → await → 据旧读数改共享态"）都**不能**靠这把闸排除 runner 的
    /// 插入。既有实例：`do_request_release` 的源复位检查与 `restore_latched(false)` 之间的窗口
    /// （**A-1**，已修：清态前的同锁内复检，见该函数步骤 4）；`mark_stop_failed` 的
    /// check-then-act（**I-2**，已修：`latched` 门装在写锁内）。新增写路径时请照此办理。
    op_in_flight: AtomicBool,
}

/// 写操作占位守卫：`Drop` 保证**任何**返回路径（含 `?` 提前返回、panic 展开）都释放闸，
/// 不会把控制器永久卡在 `Busy`。
struct OpGuard<'a>(&'a AtomicBool);

impl Drop for OpGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl InterlockController {
    /// 生产构造：逐个 sysfs 打开 DI/DO；任一 pcs_stop DI 打开失败 → fail-safe（取舍 5）。
    pub fn new(
        cfg: IoConfig,
        port: Box<dyn InterlockPort>,
        events: Arc<dyn EventRepository>,
        alert_feed: Arc<crate::alert_feed::AlertFeed>,
    ) -> Self {
        let mut ins: Vec<Box<dyn DigitalIn>> = Vec::with_capacity(cfg.di.len());
        let mut outs: Vec<Box<dyn DigitalOut>> = Vec::with_capacity(cfg.do_out.len());
        let mut pending: Vec<(String, String)> = Vec::new();
        let mut any_pcs_failed = false;

        for d in &cfg.di {
            match SysfsIn::new(d.gpio) {
                Ok(g) => ins.push(Box::new(g) as Box<dyn DigitalIn>),
                Err(e) => {
                    tracing::error!(
                        "DI gpio{} ({}) sysfs 初始化失败: {} —— fail-safe：该源恒按触发处理",
                        d.gpio,
                        d.name,
                        e
                    );
                    pending.push((
                        d.name.clone(),
                        format!("DI gpio{} ({}) sysfs 初始化失败: {}", d.gpio, d.name, e),
                    ));
                    if d.action == "pcs_stop" {
                        any_pcs_failed = true;
                    }
                    ins.push(Box::new(DiReadFailStub) as Box<dyn DigitalIn>);
                }
            }
        }
        for do_ in &cfg.do_out {
            match SysfsOut::new(do_.gpio) {
                Ok(g) => outs.push(Box::new(g) as Box<dyn DigitalOut>),
                Err(e) => {
                    tracing::error!(
                        "DO gpio{} ({}) sysfs 初始化失败: {} —— 该灯降级不可点亮",
                        do_.gpio,
                        do_.name,
                        e
                    );
                    pending.push((
                        do_.name.clone(),
                        format!("DO gpio{} ({}) sysfs 初始化失败: {}", do_.gpio, do_.name, e),
                    ));
                    outs.push(Box::new(DoNoopStub) as Box<dyn DigitalOut>);
                }
            }
        }

        let ctl = Self::new_with_io(cfg, ins, outs, port, events, alert_feed);
        if any_pcs_failed {
            // 首 tick 前即呈现锁存（无人值守也无未联锁运行的窗口）；transport/DB 同步延至
            // run_loop 首帧（new 为同步构造无法 await，见 `sync_failsafe_latch` P2-1）。
            ctl.preset_failsafe();
            tracing::error!("存在 pcs_stop DI 初始化失败 —— 联锁已预置触发锁存（fail-safe）");
        }
        *ctl.pending_init_events.lock().unwrap() = pending;
        ctl
    }

    /// fail-safe 预置：本地立即 latch（dispatch 抑制即时生效）+ 置 `failsafe_preset` 标记
    /// （run_loop 首帧据此补 transport restore(true) 与 DB triggered）。
    fn preset_failsafe(&self) {
        {
            let mut st = self.state.write().unwrap();
            st.latched = true;
            st.stop_failed = true;
            st.last_fault_lamp_reason = FaultReason::Interlock;
        }
        *self.failsafe_preset.lock().unwrap() = true;
    }

    /// 可测构造：外部直接注入 ins/outs/port/events/alert_feed（不触碰 sysfs）。
    /// 仅供本模块测试与 `new` 内部使用。
    fn new_with_io(
        cfg: IoConfig,
        ins: Vec<Box<dyn DigitalIn>>,
        outs: Vec<Box<dyn DigitalOut>>,
        port: Box<dyn InterlockPort>,
        events: Arc<dyn EventRepository>,
        alert_feed: Arc<crate::alert_feed::AlertFeed>,
    ) -> Self {
        let n = cfg.di.len();
        let do_n = cfg.do_out.len();
        let auto_release = cfg.auto_release;
        let release_hold_secs = cfg.release_hold_secs;
        if do_n < 2 {
            tracing::warn!(
                "io.do_out 仅 {} 个 DO（<2）—— 依赖顺序 do_out[0]=运行灯/do_out[1]=故障灯；缺故障灯则安全信号降级",
                do_n
            );
        }
        tracing::info!(
            "安全联锁控制器就绪：DI={} DO={} poll_ms={} auto_release={} release_hold_secs={} stop_confirm_ms={}",
            n, do_n, cfg.poll_ms, cfg.auto_release, cfg.release_hold_secs, cfg.stop_confirm_ms
        );
        Self {
            cfg,
            state: Arc::new(RwLock::new(InterlockState::default())),
            sm: Mutex::new(StateMachine::new(auto_release, release_hold_secs)),
            ins,
            outs,
            port,
            events,
            alert_feed,
            runtime: Mutex::new(DiRuntime {
                debounce: vec![0; n],
                door_prev: vec![false; n],
                read_fail_logged: vec![false; n],
                last_stop_attempt: None,
            }),
            pending_init_events: Mutex::new(Vec::new()),
            failsafe_preset: Mutex::new(false),
            op_in_flight: AtomicBool::new(false),
        }
    }

    // ── web/dispatch 共享查询 ──

    /// dispatch 前 latch 抑制查询（读共享 state，同步非阻塞）
    pub fn is_latched_now(&self) -> bool {
        self.state.read().unwrap().latched
    }

    /// 进入一次写操作（互斥）。已被占用 ⇒ `None`（调用方回 [`InterlockReject::Busy`]）。
    ///
    /// 临界区 = 整个写方法体（含 await 点）：写路径的每个 await 都是"先改本地、后写 transport"
    /// 这类**必须独占**的序列（`do_request_release` 的取舍 4：transport 先释放成功再清本地）。
    fn try_begin_op(&self) -> Option<OpGuard<'_>> {
        self.op_in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| OpGuard(&self.op_in_flight))
    }

    /// 当前是否有写操作在处理中（**测试用的那把尺子**：并发用例据此确定性地等到闸被占住）。
    #[cfg(test)]
    pub(crate) fn op_in_flight(&self) -> bool {
        self.op_in_flight.load(Ordering::SeqCst)
    }

    // ── run_loop ──

    /// 常驻轮询循环（poll_ms）。启动时 flush 构造期 GPIO 失败事件 + 同步 fail-safe latch。
    pub async fn run_loop(self: Arc<Self>) {
        self.flush_init_events().await;
        self.sync_failsafe_latch().await;
        let poll = Duration::from_millis(self.cfg.poll_ms.max(1));
        loop {
            let started = Instant::now();
            self.tick_frame().await;
            let remain = poll.saturating_sub(started.elapsed());
            if !remain.is_zero() {
                tokio::time::sleep(remain).await;
            }
        }
    }

    async fn flush_init_events(&self) {
        let evts = std::mem::take(&mut *self.pending_init_events.lock().unwrap());
        for (src, msg) in evts {
            self.record_event("interlock.gpio_init_failed", &src, &msg)
                .await;
        }
    }

    /// P2-1：把 fail-safe init 预置（`new()`/`preset_failsafe`，同步只能设本地 `state.latched`）
    /// 补同步到 transport `stopped_latched`（C-1 双 latch 的 transport 兜底，latch 期间 send 拒写）
    /// 与 DB triggered 事件（重启 `restore_from_db` 读回锁存的依据）。单次幂等：跑一次即清标记。
    async fn sync_failsafe_latch(&self) {
        let preset = std::mem::take(&mut *self.failsafe_preset.lock().unwrap());
        if !preset {
            return;
        }
        if let Err(e) = self.port.restore_latched(true).await {
            tracing::warn!(
                "联锁 fail-safe latch 同步 transport 失败: {}（本地 latch 已生效，dispatch 抑制中）",
                e
            );
        }
        let msg = "联锁锁存：GPIO pcs_stop DI 初始化失败 fail-safe 预置（transport/DB 同步）";
        self.record_event("interlock.triggered", "interlock", msg)
            .await;
        self.alert_feed.push_interlock("triggered", msg);
        tracing::warn!("{}", msg);
    }

    /// 单帧：读 DI → 去抖 → tick → DO → 事件/动作/停机维护。
    /// 锁序：仅 sync 临界区持 std 锁（tick/去抖），跨 `.await` 一律不持锁。
    async fn tick_frame(&self) {
        let n = self.cfg.di.len();

        // ── 1. 读 DI：raw 依 active_low 反相得有效态；读失败=触发（fail-safe，取舍 6）──
        let mut active = Vec::with_capacity(n);
        let mut read_fail = vec![false; n];
        for (i, di) in self.cfg.di.iter().enumerate() {
            let tr = match self.ins[i].read_level() {
                Ok(high) => high != di.active_low,
                Err(e) => {
                    read_fail[i] = true;
                    tracing::error!("DI {} 读失败: {}", di.name, e);
                    true
                }
            };
            active.push(tr);
        }

        // ── 2. per-DI 去抖 + 门禁沿/读失败事件采集（sync，勿持锁跨 await）──
        let (tripped, trigger_names, door_events, fail_events) = {
            let mut rt = self.runtime.lock().unwrap();
            let mut tripped: Vec<(InterlockSource, bool)> = Vec::with_capacity(n);
            let mut trigger_names: Vec<String> = Vec::new();
            let mut door_events: Vec<String> = Vec::new();
            let mut fail_events: Vec<(String, String)> = Vec::new();
            for i in 0..n {
                let di = &self.cfg.di[i];
                if active[i] {
                    rt.debounce[i] = rt.debounce[i].saturating_add(1);
                } else {
                    rt.debounce[i] = 0;
                }
                let tr = active[i] && rt.debounce[i] >= di.debounce;
                if read_fail[i] && !rt.read_fail_logged[i] {
                    rt.read_fail_logged[i] = true;
                    fail_events.push((
                        di.name.clone(),
                        format!(
                            "DI {} gpio{} 读失败，按触发处理（fail-safe）",
                            di.name, di.gpio
                        ),
                    ));
                }
                if action_of(&di.action) == DiAction::Event {
                    if tr && !rt.door_prev[i] {
                        door_events.push(format!("通道 {} 触发", di.name));
                    } else if !tr && rt.door_prev[i] {
                        door_events.push(format!("通道 {} 复位", di.name));
                    }
                    rt.door_prev[i] = tr;
                } else if tr {
                    trigger_names.push(di.name.clone());
                }
                tripped.push((classify_source(&di.name, &di.action), tr));
            }
            (tripped, trigger_names, door_events, fail_events)
        };

        // ── 3. run_state / 在线（取舍 2：pcs_online = last_run_state().is_some()）──
        let run_state = self.port.last_run_state();
        let pcs_online = run_state.is_some();
        let run_state_ok = matches!(run_state, Some(1..=3));

        // ── 4. tick（纯 sync，持两锁短临界）──
        let (action, run_lamp, fault_lamp) = {
            let mut st = self.state.write().unwrap();
            let mut m = self.sm.lock().unwrap();
            m.tick(
                &mut st,
                &tripped,
                false,
                run_state_ok,
                pcs_online,
                now_secs(),
            )
        };

        // ── 5. 写 DO（取舍 3：do_out[0]=运行灯 / [1]=故障灯）；DO 非安全链，写失败仅记录 ──
        self.drive_lamps(run_lamp, fault_lamp);

        // ── 6. 事件落库/SSE（async；此刻不持任何锁）──
        for msg in door_events {
            self.record_event("interlock.di", "interlock", &msg).await;
        }
        for (src, msg) in fail_events {
            self.record_event("interlock.di_read_failed", &src, &msg)
                .await;
        }

        // ── 7. Action 副作用 ──
        match action {
            Action::TriggerLatch => self.on_trigger(&trigger_names).await,
            Action::ReleaseLatch => self.on_auto_release().await,
            Action::None => {}
        }

        // ── 8. 停机维护：stop_failed 重试 + DB 读回 latch 时对仍在运行的 PCS 补发停机 ──
        self.post_stop_maintenance().await;
    }

    /// 写运行/故障灯（目标逻辑电平 → active_high 换算物理电平）
    fn drive_lamps(&self, run_lamp: bool, fault_lamp: bool) {
        for (i, out) in self.outs.iter().enumerate() {
            let lit = match i {
                0 => Some(run_lamp),
                1 => Some(fault_lamp),
                _ => None,
            };
            let Some(lit) = lit else { continue };
            let high = if self.cfg.do_out[i].active_high {
                lit
            } else {
                !lit
            };
            if let Err(e) = out.set_level(high) {
                tracing::error!(
                    "DO {} gpio{} 写失败: {}（灯驱动降级）",
                    self.cfg.do_out[i].name,
                    self.cfg.do_out[i].gpio,
                    e
                );
            }
        }
    }

    /// 触发沿副作用：restore(true) → 事件/SSE → **单次**停机写（非阻塞，不等待确认）。
    /// 状态机已在 tick 中把 state.latched=true。
    /// 确认/超时/重试由 `post_stop_maintenance` 逐帧完成（S2 Task7 Important：不在 run_loop 帧内
    /// 自旋阻塞——latch+离线/PCS 不停时每帧仍须继续采样 DI/DO/门禁）。
    async fn on_trigger(&self, trigger_names: &[String]) {
        if let Err(e) = self.port.restore_latched(true).await {
            tracing::warn!(
                "联锁锁存写 transport 失败: {}（本地 latch 仍生效，dispatch 由 runner 抑制）",
                e
            );
        }
        let summary = if trigger_names.is_empty() {
            "interlock".to_string()
        } else {
            trigger_names.join(",")
        };
        let msg = format!("联锁触发（源：{}），下发 PCS 停机", summary);
        self.record_event("interlock.triggered", &summary, &msg)
            .await;
        self.alert_feed.push_interlock("triggered", &msg);
        self.stop_once().await;
    }

    /// 状态机自动释放（auto_release && 停机已确认 && 源复位 && hold 满）：同步 transport latch。
    async fn on_auto_release(&self) {
        if let Err(e) = self.port.restore_latched(false).await {
            tracing::warn!(
                "联锁释放写 transport 失败: {}（本地已释放，需人工确认 transport 状态）",
                e
            );
        }
        let msg = "联锁自动释放（触发源已复位且保持期满，停机已确认）";
        self.record_event("interlock.cleared", "interlock", msg)
            .await;
        self.alert_feed.push_interlock("cleared", msg);
    }

    /// 单次停机写（**非阻塞**：只发一次 `port.stop()` + 记录 last_stop_attempt，不轮询等待确认）。
    /// 写失败即时 mark_stop_failed；写成功不在此确认——确认/超时/重试全交由 `post_stop_maintenance`
    /// 每帧非阻塞完成（S2 Task7 Important：移除旧版内联自旋，避免 latch 未停时整条 run_loop 阻塞）。
    /// on_trigger（触发沿首次停机）与 post_stop_maintenance（周期补发）共用本辅助。
    async fn stop_once(&self) {
        if let Err(e) = self.port.stop().await {
            self.mark_stop_failed(&format!("停机指令失败: {e}")).await;
        }
        self.runtime.lock().unwrap().last_stop_attempt = Some(Instant::now());
    }

    /// 置 stop_failed（幂等：仅在首次 false→true 记事件/SSE/error）。
    ///
    /// # ⚠️ `latched` 门（**单元 J 第二轮整改 I-2**）——把 `stop_failed ⟹ latched` 从不变量
    /// 断言变成**构造性保证**
    ///
    /// 本方法是**唯一**写 `stop_failed` 而不碰 `latched` 的地方。改前它只有一个**跨线程
    /// check-then-act** 的软保证：调用方（`post_stop_maintenance` 的 `if !latched { return }`
    /// / `on_trigger`）先读 `latched`，再由本方法写 `stop_failed`；两件事分处两次加锁 ⇒ 多线程
    /// runtime 下（runner 一帧一任务，写路径在 axum worker）**并发 `do_request_release` 可在
    /// 该窗口内完成**（`restore_latched(false).await` 后于 `do_request_release` 的收尾块
    /// （`had_stop_failed` 处）把两标志一起清）⇒ 随后
    /// 本方法把 `stop_failed` 置回 true ⇒ 造出 `!latched && stop_failed`。
    ///
    /// ⚠️ **该窗口的并发方不只是"写路径"（A-1 顺带订正 J 轮措辞）**：`op_in_flight` 那把闸只
    /// 互斥 `release × ack_m1`，**不覆盖 runner** ⇒ 能在本方法"读 `latched`"与"写 `stop_failed`"
    /// 之间插入的，除并发写路径外还有**相邻任务里的 `tick_frame`**（`StateMachine::tick` 的释放
    /// 分支同样两标志一起清）。I-2 用"门装在**写锁内**"统一关掉了**两族**（判定与写入变成同一个
    /// 临界区 ⇒ 谁插入都不再成立），A-1 的修法（清态前同锁内复检）是同一思路的第二个实例。
    ///
    /// 那是个**可用性缺口**（不是安全漏洞：两个结局都是拒绝）：该态**没有任何清除路径**——
    /// `StateMachine::tick` 的清零分支要求 `s.latched`、`clear_stop_failed_once` 只在
    /// `post_stop_maintenance` 的 `latched` 门后才可达、`do_request_release` 在 `!latched` 时
    /// 提前 `Ok(())` 且不清 `stop_failed` ⇒ `ack_m1` **长期**回 `StopPending`（屏上「PCS 停机
    /// 未确认 · 暂不可授权重启」）、故障灯常亮，只能靠一次新的物理触发或重启进程解开。
    ///
    /// 修法：**在读同一把写锁内**判定 `latched`（check-then-act 变成同一临界区内的原子判定）
    /// ⇒ `!latched` 时**不写**。三个调用点当前都在 `latched` 下（`on_trigger` 紧接 tick 置位、
    /// `post_stop_maintenance` 的 `latched` 门）⇒ **正常语义不变**；被这道门挡住的只可能是
    /// "latch 已被并发 release 释放"这一种情形，而"人工放行 override 即清 `stop_failed`"本就是
    /// 状态机的既有取舍（`do_request_release` 的无条件清除）⇒ 门与语义同向，不是新口径。
    async fn mark_stop_failed(&self, reason: &str) {
        let (was_new, gated) = {
            let mut st = self.state.write().unwrap();
            if !st.latched {
                // 与并发 release 撞车：latch 已释放（release 语义 = 人工 override，两标志同清）
                // ⇒ 停机失败标记在此刻已无对象（自动释放放行 / 授权重启的前置都已随 latch 一并
                // 消失）。写下去就是这个态**永远解不开**（见上：无清除路径）⇒ 如实丢弃。
                (false, true)
            } else {
                let new = !st.stop_failed;
                st.stop_failed = true;
                (new, false)
            }
        };
        if gated {
            // **`warn!` 而非 `debug!`**（单元 J 第二轮整改 A2）：走进这个分支意味着**一次真实的
            // `port.stop()` 失败**既没进 `stop_failed`、也没进事件库 / 审计 / SSE（上面那条
            // `error!` + `record_event` 都在 `gated` 的**另一侧**）⇒ 若不在此留痕，该次失败就
            // 只剩调试日志，生产上等于**消失**。
            tracing::warn!(
                "联锁：PCS 停机失败但标记**未写入** —— 该次停机失败**未记入 `stop_failed`**\
                 （latch 已被并发 release 合法清掉，写下去将成为解不开的粘住态）；原因：{}",
                reason
            );
        }
        if was_new {
            tracing::error!("联锁：PCS 停机确认失败 —— {}", reason);
            let msg = format!("PCS 停机失败/未确认：{reason}（自动释放被禁止，须人工确认放行）");
            self.record_event("interlock.stop_failed", "interlock", &msg)
                .await;
            self.alert_feed.push_interlock("stop_failed", &msg);
        }
    }

    /// 停机延迟确认成功 → 清 stop_failed（幂等；成功后由状态机按其 auto 逻辑放行自动释放）
    async fn clear_stop_failed_once(&self, reason: &str) {
        let was_set = {
            let mut st = self.state.write().unwrap();
            let was = st.stop_failed;
            st.stop_failed = false;
            was
        };
        if was_set {
            tracing::info!("联锁：{}", reason);
            self.record_event("interlock.stopped", "interlock", reason)
                .await;
        }
    }

    /// 周期停机维护——**唯一**的确认/超时/重试权威（S2 Task7 Important：全程非阻塞、无内联自旋，
    /// 每帧只做快检查，绝不 await 长窗口）。
    ///
    /// 逐帧（tick_frame 末尾）在 latch 下做下列判断之一：
    /// - `run == Some(0)` → 延迟确认成功 → clear_stop_failed_once（幂等，清曾超时的残留标记）。
    /// - 曾发停机（Some）且距上次尝试 ≥ stop_confirm_ms 仍未确认 → **真超时**：mark_stop_failed
    ///   （幂等，仅首次 false→true 记一次事件/SSE），随后重发单次停机写并刷新 last_stop_attempt。
    /// - 无本进程停机尝试（None，如 DB 读回 latch 首帧）→ 仅补发首停（stop_failed 已由 restore
    ///   置位），不据窗口误判超时。
    /// - 窗口内（距上次尝试 < stop_confirm_ms）→ 不动（下帧再查）。
    ///
    /// 语义与原阻塞版等价但非阻塞：stop 写 Ok 但 PCS 未转 0 → 首个确认窗口结束时置 stop_failed
    /// （一次事件）→ 之后每 ≥ stop_confirm_ms 重发直至 run 转 Some(0)（届时清除）。stop_failed 置位
    /// 时机从「触发自旋后」变为「维护帧检查」，多 ≤1 帧(poll_ms)。
    async fn post_stop_maintenance(&self) {
        if !self.state.read().unwrap().latched {
            return;
        }
        let run = self.port.last_run_state();
        if run == Some(0) {
            // PCS 已确认停机（延迟确认清除路径）：有残留 stop_failed（曾超时后来转 0）→ 清除
            self.clear_stop_failed_once("PCS 已确认停机，清除此前停机失败标记")
                .await;
            return;
        }
        // 未确认（离线 None 或仍在 1|2|3）：判定是否越过确认窗口（或无本进程尝试需补发）
        let (attempt_due, timed_out) = {
            let rt = self.runtime.lock().unwrap();
            match rt.last_stop_attempt {
                // 无本进程停机尝试（如 DB 读回 latch 首帧，stop_failed 已由 restore 置位）→ 补发首停，
                // 无超时可言（勿据"窗口已过"误判超时）
                None => (true, false),
                Some(t) => {
                    let due = t.elapsed() >= Duration::from_millis(self.cfg.stop_confirm_ms);
                    (due, due)
                }
            }
        };
        if attempt_due {
            if timed_out {
                // 真超时（曾发停机但窗口内未确认）→ 首次置 stop_failed（幂等：仅 false→true 记一次）
                self.mark_stop_failed(&format!(
                    "停机确认超时(>={}ms) RUN_STATE 未转 0",
                    self.cfg.stop_confirm_ms
                ))
                .await;
            }
            // 重发单次停机写（写失败即时 mark_stop_failed；写成功只更新 last_stop_attempt）
            self.stop_once().await;
        }
        // 窗口内：不动，下帧再查
    }

    // ── DB 事件辅助 ──

    async fn record_event(&self, event_type: &str, source: &str, message: &str) {
        let ev = SystemEvent {
            id: None,
            timestamp: chrono::Utc::now(),
            event_type: event_type.to_string(),
            source: source.to_string(),
            message: message.to_string(),
        };
        if let Err(e) = self.events.insert(&ev).await {
            tracing::warn!("联锁事件落库失败 event_type={}: {}", event_type, e);
        }
    }

    // ── Step 5: DB 读回（启动 restore）──

    /// 启动时按 DB 最近 triggered/cleared 恢复 latch。
    /// 最新 triggered 无后续 cleared → 恢复 latched（停机未确认态）+ restore_latched(true)。
    /// 否则归一化 restore_latched(false)（transport 重启后 memory latch 清空）。
    pub async fn restore_from_db(&self) {
        let triggered = self
            .events
            .latest_by_type("interlock.triggered")
            .await
            .unwrap_or(None);
        let cleared = self
            .events
            .latest_by_type("interlock.cleared")
            .await
            .unwrap_or(None);
        let latched = match (&triggered, &cleared) {
            (Some(t), Some(c)) => t.timestamp > c.timestamp,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if latched {
            {
                let mut st = self.state.write().unwrap();
                st.latched = true;
                st.stop_failed = true; // 崩溃恢复，停机未确认（fail-safe；后续补发停机并确认）
                st.active_pcs_stop_sources = 0;
                st.source_safe_since = None;
                st.last_fault_lamp_reason = FaultReason::Interlock;
            }
            if let Err(e) = self.port.restore_latched(true).await {
                tracing::error!("联锁 DB 读回恢复 latch：restore(true) 失败: {}", e);
            }
            let msg = "启动 DB 读回：检测到联锁触发且无后续释放，恢复锁存（fail-safe）";
            tracing::warn!("{}", msg);
            self.record_event("interlock.restored", "interlock", msg)
                .await;
        } else {
            if let Err(e) = self.port.restore_latched(false).await {
                tracing::warn!("联锁 DB 读回归一化：restore(false) 失败: {}", e);
            }
            tracing::info!("启动 DB 读回：无待恢复的联锁锁存");
        }
    }
}

// ── InterlockApi impl（Step 6）──

impl InterlockController {
    /// web 各触发源状态：按 cfg.di 顺序取 distinct source token，tripped = 该源任一通道实时触发。
    fn status_sources(&self) -> Vec<InterlockSourceStatus> {
        let mut out: Vec<(&'static str, bool)> = Vec::new();
        for (i, di) in self.cfg.di.iter().enumerate() {
            let tok = source_token(classify_source(&di.name, &di.action));
            let tr = live_active_channel(&self.ins, i, di.active_low);
            if let Some(entry) = out.iter_mut().find(|(t, _)| *t == tok) {
                entry.1 |= tr;
            } else {
                out.push((tok, tr));
            }
        }
        out.into_iter()
            .map(|(name, tripped)| InterlockSourceStatus {
                name: name.to_string(),
                tripped,
            })
            .collect()
    }

    /// 仍未复位的 `pcs_stop` 源 token 列表（去重、保持 `cfg.di` 顺序）。
    ///
    /// **单一判据（A-1 修法的关键约束）**：`do_request_release` 的**前置检查**（步骤 1）与
    /// **清态前的复检**（步骤 4）共用本函数 —— **同一个数据源**（`live_active_channel` 的实时
    /// 电平）、**同一套归类**（`classify_source` → `source_token`），复检因此不引入第二套口径。
    fn unsettled_pcs_stop_sources(&self) -> Vec<String> {
        let mut remaining: Vec<String> = Vec::new();
        for (i, di) in self.cfg.di.iter().enumerate() {
            if action_of(&di.action) == DiAction::PcsStop
                && live_active_channel(&self.ins, i, di.active_low)
            {
                let tok = source_token(classify_source(&di.name, &di.action)).to_string();
                if !remaining.contains(&tok) {
                    remaining.push(tok);
                }
            }
        }
        remaining
    }

    /// 手动释放：前置（全部 pcs_stop 源复位 + 保持期满）满足才清 latch；
    /// **同步执行**（取舍 4）——transport 先释放成功再清本地，防状态分裂。stop_failed 放行记审计。
    ///
    /// # 拒绝路径 → [`InterlockReject`] 变体（**逐路径映射表**，EDGE-12 的直接落点）
    ///
    /// | 拒绝路径（本函数的顺序） | 变体 | 说明 |
    /// |--------------------------|------|------|
    /// | `io.enabled=false` | [`InterlockReject::NotEnabled`] | 结构性事实，优先于一切 |
    /// | 闸被占用（并发写） | [`InterlockReject::Busy`] | 不排队、不改状态 |
    /// | 任一 pcs_stop 源仍触发 | [`InterlockReject::SourcesNotReset{remaining}`] | 列出**全部**未复位源 token（不是第一个） |
    /// | 同上，但在 `restore_latched(false)` 的 await 窗口内**重新触发** | [`InterlockReject::SourcesNotReset{remaining}`] | A-1：清态前的**同锁内复检**拦下（步骤 4），不谎报成功 |
    /// | 保持计时未起（`source_safe_since=None`） | [`InterlockReject::HoldNotElapsed{need,remaining=need}`] | 已复位但一帧都还没守住 ⇒ 剩余 = 全额 |
    /// | 保持未满 | [`InterlockReject::HoldNotElapsed{need,remaining=need-elapsed}`] | 带**剩余秒数** |
    /// | transport 释放失败 | [`InterlockReject::Internal`] | 透传具体原因，不吞 |
    ///
    /// **不是拒绝的情况**：未 latch ⇒ `Ok(())`（幂等：重复释放不报错，F17.5）。
    /// 修复前这些路径全是 `Result<(), String>` 的中文串 ⇒ 屏上/上层只能整串转发，无法按变体分派。
    ///
    /// ⚠️ （单元 J 第二轮整改 I-2）该提前返回**不清 `stop_failed`**——本整改之前这是**缺口的
    /// 一半**（`mark_stop_failed` 可造出 `!latched && stop_failed`，而这条路径不解它 ⇒ 粘住）。
    /// I-2 把缺口关在**上游**：`mark_stop_failed` 加 `latched` 门后 `!latched ⇒ !stop_failed`
    /// 按构造成立 ⇒ 这条提前返回**本来就没有残留标记要清**，故此处**保持原样**（不改语义）。
    async fn do_request_release(&self) -> Result<(), InterlockReject> {
        if !self.cfg.enabled {
            return Err(InterlockReject::NotEnabled);
        }
        // ⚠️ 绑到 `_guard`（**不是** `_`/`_op`）：名字带下划线只是为了不必在函数体里读它，
        // 它**不是**"立刻丢弃"——这把闸（`OpGuard`）**活到函数末尾**，`Busy` 语义全靠它。
        // （单元 J 第二轮整改建议 5：原写成 `_op` 会让读者误以为是立即丢弃的占位。）
        let Some(_guard) = self.try_begin_op() else {
            return Err(InterlockReject::Busy);
        };
        let now = now_secs();
        // 前置：当前状态快照
        let latched = self.state.read().unwrap().latched;
        if !latched {
            return Ok(());
        }
        // 1) 任一 pcs_stop 源仍触发 → 拒绝（列出**全部**仍未复位的源 token）
        let remaining = self.unsettled_pcs_stop_sources();
        if !remaining.is_empty() {
            return Err(InterlockReject::SourcesNotReset { remaining });
        }
        // 2) 保持期满校验（release_hold_secs>0 时；source_safe_since 由 runner tick 维护）
        let hold = self.cfg.release_hold_secs;
        if hold > 0 {
            let (since, latched_again) = {
                let st = self.state.read().unwrap();
                (st.source_safe_since, st.latched)
            };
            if !latched_again {
                return Ok(());
            }
            match since {
                // 计时尚未起步（触发后还没有一帧看到"全复位"）⇒ 已守 0 s、余 `hold` s。
                None => {
                    return Err(InterlockReject::HoldNotElapsed {
                        need_secs: hold,
                        remaining_secs: hold,
                    })
                }
                Some(t0) => {
                    let elapsed = now.saturating_sub(t0);
                    if elapsed < hold {
                        return Err(InterlockReject::HoldNotElapsed {
                            need_secs: hold,
                            remaining_secs: hold - elapsed,
                        });
                    }
                }
            }
        }
        // 3) transport 先释放（成功才清本地，防状态分裂）
        self.port
            .restore_latched(false)
            .await
            .map_err(|e| InterlockReject::Internal(format!("释放联锁失败: {e}")))?;
        // 4) 清本地 + 审计（stop_failed 人工放行留痕）
        //
        // ⚠️ **A-1（独立评审，安全相关）：清态与"源仍复位"复检必须在同一临界区内。**
        //
        // 缺陷（修前，已确定性复现）：源复位检查只做在**步骤 1**（进入函数时），而步骤 3 的
        // `await restore_latched(false)` 是**挂起点**。runner 的 `tick_frame` 在**相邻任务**里跑、
        // **不占** `op_in_flight` 闸（那把闸只互斥两条写路径 release×ack_m1，见 `op_in_flight`
        // 的文档）⇒ 操作员在这个 await 窗口内**重新按下急停**时，runner 会照常采样、照常把
        // 物理事实记进 `state`。于是本次 release 会在"急停仍按着"的事实上返回 `Ok(())`、把
        // latch 清掉、并记一条「触发源已复位且保持期满」的**假审计**（屏上显「已释放」）。
        // 探针实测：`release Ok=true / 锁存被清=true / 而 pin=true`。
        // 下一帧虽会自愈（步骤 5 重置状态机边沿 ⇒ 重新产生触发沿），但那一拍里
        // `is_latched_now()` 会读到 `false`（1 Hz 轮询）⇒ 可能**放行一拍下发**。
        //
        // 修法（最小）：复检挪进**清态的那把写锁**内，且用**与步骤 1 同一个**函数/数据源
        // （`unsettled_pcs_stop_sources`）。不满足 ⇒ 如实回 `SourcesNotReset`，**不谎报成功**。
        // 正常语义不变：源确实全复位时，复检恒过 ⇒ 成功路径与修前逐字节等价。
        let recheck = {
            let mut st = self.state.write().unwrap();
            let remaining = self.unsettled_pcs_stop_sources();
            if remaining.is_empty() {
                let was = st.stop_failed;
                st.latched = false;
                st.stop_failed = false;
                st.source_safe_since = None;
                st.last_fault_lamp_reason = FaultReason::None;
                Ok(was)
            } else {
                Err(remaining)
            }
        };
        let had_stop_failed = match recheck {
            Ok(was) => was,
            Err(remaining) => {
                // 复检被"窗口内重新触发"推翻 ⇒ **不清态**、如实拒绝。
                //
                // 但此刻 transport 已经收到本次 `restore_latched(false)`（步骤 3 成功）⇒ 必须把它
                // 拉回锁存态，否则本机 `latched=true` / transport `latched=false` **状态分裂**
                // （C-1 双 latch 的 transport 兜底失效：latch 期间 send 拒写这一层没了）。
                // 注意：本路径**不能**指望"下一帧自愈"补写 transport —— 本机 latch 仍为 true，
                // `StateMachine::tick` 走的是"仍有源 active"分支（无 `TriggerLatch` 动作 ⇒ 不
                // `on_trigger`）⇒ 不补写就长期分裂。回写方向安全（置锁存 = 更保守），失败仅告警
                // （本机 latch 已生效，dispatch 仍由 runner 抑制）。
                if let Err(e) = self.port.restore_latched(true).await {
                    tracing::warn!(
                        "联锁释放复检被推翻：回写 transport 锁存失败: {}（本机 latch 仍生效，dispatch 由 runner 抑制）",
                        e
                    );
                }
                return Err(InterlockReject::SourcesNotReset { remaining });
            }
        };
        // 5) 重置状态机边沿（防旧 prev 使下次同源触发不产生新沿）
        *self.sm.lock().unwrap() =
            StateMachine::new(self.cfg.auto_release, self.cfg.release_hold_secs);
        let msg = if had_stop_failed {
            "人工放行联锁（停机未确认 override，PCS 运行态须人工确认后重启）"
        } else {
            "人工释放联锁（触发源已复位且保持期满）"
        };
        // 操作者口径（单元 J 订正）：本路径的**唯一**调用方是本地屏控制通道
        // （`console_host` 的两条写端点），T-3 已裁定「无登录 / 无会话 / 无 RBAC」⇒
        // 操作者恒为契约常量 `local-console`。修复前这里写 `admin(session)`——那句在
        // web 前端退役后**已是假话**（无 session、无 admin 角色），故一并订正。
        // 结构性审计（before/after + result + request_id）由写管线的 `ConsoleAuditSink` 承担。
        let msg = format!(
            "{}（operator: {}）",
            msg,
            mupc_display_proto::CONSOLE_OPERATOR
        );
        self.record_event("interlock.cleared", "interlock", &msg)
            .await;
        self.alert_feed.push_interlock("cleared", &msg);
        Ok(())
    }
}

/// 读第 i 个 DI 实时有效态（active_low 反相）；越界/读失败=触发（fail-safe）
fn live_active_channel(ins: &[Box<dyn DigitalIn>], i: usize, active_low: bool) -> bool {
    match ins.get(i) {
        Some(g) => match g.read_level() {
            Ok(high) => high != active_low,
            Err(_) => true, // 读失败按触发（fail-safe）
        },
        None => true,
    }
}

#[async_trait]
impl InterlockApi for InterlockController {
    /// 联锁总态（F16）：**即帧内 `interlock` 段**（契约单一真源）。
    ///
    /// 字段口径（逐字段的真源 / 限度）：
    /// - `enabled`：取 `cfg.enabled`（**修复前硬编码 `true`**）——`io.enabled=false` 是
    ///   **已知状态**（设计 §4.2 表 C：「联锁功能未启用」），谎报成 `true` 会让屏上把
    ///   "功能没开"读成"开了但没联锁"（IL-01.6 同一族）。生产装配在 `io.enabled=false` 时
    ///   **不构造**控制器（屏上由 `InterlockWiring::Disabled` 表达），该字段是对**构造入参**
    ///   的忠实回读 + 写路径的 `NotEnabled` 判据；
    /// - `available`：`status()` 无错误通道 ⇒ 能被调用即"源可用"（设计 §4.6）；
    /// - `ts_ms`：**本控制器**的取数时刻（帧侧 `display_host` 另按采集时刻覆写，见其注释）；
    /// - `release_hold_secs`：取 `cfg.release_hold_secs`（UI 用于提示「须保持 N 秒」）；
    /// - `fault_lamp` / `run_lamp`：`Some(..)` = 控制器给出的**目标电平**（非 DO 回读，
    ///   语义限度见 `display_host::interlock_section_of` 的同款项）。
    async fn status(&self) -> InterlockStatus {
        let (latched, stop_failed) = {
            let st = self.state.read().unwrap();
            (st.latched, st.stop_failed)
        };
        let run_state = self.port.last_run_state();
        let pcs_online = run_state.is_some();
        let run_state_ok = matches!(run_state, Some(1..=3));
        let m1 = pcs_online && !run_state_ok && !latched;
        let fault_lamp = latched || stop_failed || !pcs_online || m1;
        let run_lamp = run_state_ok && !latched;
        InterlockStatus {
            ts_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            available: true,
            enabled: self.cfg.enabled,
            latched,
            stop_failed,
            sources: self.status_sources(),
            fault_lamp: Some(fault_lamp),
            run_lamp: Some(run_lamp),
            release_hold_secs: self.cfg.release_hold_secs,
        }
    }

    async fn request_release(&self) -> Result<(), InterlockReject> {
        self.do_request_release().await
    }

    /// M1 保护跳闸 / 停机人工授权重启（仅 `!latched` 且停机已确认时生效）。
    ///
    /// # 拒绝路径 → 变体
    ///
    /// | 拒绝路径（本函数的顺序） | 变体 |
    /// |--------------------------|------|
    /// | `io.enabled=false` | [`InterlockReject::NotEnabled`] |
    /// | 闸被占用（并发写） | [`InterlockReject::Busy`] |
    /// | latch 态 | [`InterlockReject::Latched`] |
    /// | 停机未确认（`stop_failed`，**纵深防御**门：不变量被打破时才可达，见下） | [`InterlockReject::StopPending`] |
    /// | transport 授权失败 | [`InterlockReject::Internal`] |
    ///
    /// ⚠️ **`StopPending` 这道门的可达性（单元 J 第二轮整改 I-2 如实订正）**：
    /// `!latched && stop_failed` 在**第一轮整改之后、第二轮之前**是**生产可达性极低但非零**
    /// 的态（需 `mark_stop_failed` 的跨线程 check-then-act 被**别的任务**抢占——见
    /// [`InterlockController::mark_stop_failed`] 的文档：抢占方**不只是**并发写路径
    /// `do_request_release`，还包括**不占** `op_in_flight` 闸的 runner `tick_frame`），
    /// 而且**一旦到达会粘住**——该态
    /// **没有任何清除路径**（`StateMachine::tick` 的清零分支要求 `s.latched`；
    /// `clear_stop_failed_once` 只在 `post_stop_maintenance` 的 `latched` 门后可达；
    /// `do_request_release` 在 `!latched` 时提前 `Ok(())` 且不清 `stop_failed`）⇒ `ack_m1` 会
    /// **长期**回 `StopPending`、故障灯常亮（同时 `run_lamp=true`），只能靠一次新的物理触发或
    /// 重启进程解开。**该缺口已由 `mark_stop_failed` 的 `latched` 门关闭**（门装在**同一把
    /// 写锁内** ⇒ `stop_failed` 只在 `latched` 时写入按构造成立，不再是"断言不变量"）。
    ///
    /// ⚠️ 第一轮整改此处曾写「生产**不可达**」——**说满了**（评审读码实证：那是**断言**，不是
    /// **保证**，且结论"该门不可达"依赖一个可被抢占的软不变量）。现在的口径是两层：
    /// ① 可达性由 `mark_stop_failed` 的门**构造性关闭**；② 万一将来不变量仍被打破，本门仍
    /// 如实映射到 `StopPending`（不静默放行）。**两层都在**，互不替代。
    ///
    /// `stop_failed ⟹ latched` 现在是**构造性不变量**（每个写点都在同一临界区内同时满足）：
    /// - `stop_failed` 的**全部写入点**：`preset_failsafe`（GPIO fail-safe，同一临界区同置两
    ///   标志）、`restore_from_db`（DB 读回，同置两标志）、`mark_stop_failed`（唯一写
    ///   `stop_failed` 而不碰 `latched` 的地方——**已加 `latched` 门，同一把写锁内判定**）；
    /// - `latched` 的**全部清除点**都**同时**清 `stop_failed`——`StateMachine::tick` 的释放
    ///   分支（两标志一起清）与 `do_request_release`（两标志一起清）；
    /// - `InterlockState` 字段 `pub` 但 `state` 句柄私有无外部写入口。
    ///
    /// ⇒ 真实可达态是 `latched && stop_failed`：此时 `ack_m1` 走的是上面的 `latched` 门，
    /// 返回的是 **[`InterlockReject::Latched`]**。**真正挡住「PCS 可能仍在运行却授权重启」
    /// 这一危险动作的，是状态机不变量 + `latched` 门，不是本门。**
    ///
    /// 保留本门 = **纵深防御**：若将来不变量被打破（新增一处 `stop_failed` 写入点却未同时
    /// latch），本门兜底，把该态如实映射到契约变体 `StopPending`（文案「PCS 停机未确认，
    /// **暂不可授权重启**」），而不是静默放行。对应用例
    /// `ack_m1_rejects_stop_pending_when_stop_unconfirmed` 是**不可达态防御测试**
    /// （直接构造该状态，非生产可达路径；I-2 的缺口与门由
    /// `stop_failed_never_sticks_when_a_concurrent_release_wins` 独立覆盖）。
    ///
    /// 物理判据：`stop_failed` = 停机写失败/未确认 ⇒ PCS 可能仍在运行 ⇒ 此时"授权重启"无意义。
    /// **latch 优先于 `stop_failed`**：latch 态必须先 release（PRD F18.4 明写「仅非 latch 态
    /// 可授权」）。
    async fn ack_m1(&self) -> Result<(), InterlockReject> {
        if !self.cfg.enabled {
            return Err(InterlockReject::NotEnabled);
        }
        // 同 `do_request_release`：`_guard` 活到函数末尾（`Busy` 语义靠它），不是立即丢弃。
        let Some(_guard) = self.try_begin_op() else {
            return Err(InterlockReject::Busy);
        };
        let (latched, stop_failed) = {
            let st = self.state.read().unwrap();
            (st.latched, st.stop_failed)
        };
        if latched {
            return Err(InterlockReject::Latched);
        }
        if stop_failed {
            return Err(InterlockReject::StopPending);
        }
        self.port
            .authorize_restart()
            .await
            .map_err(|e| InterlockReject::Internal(format!("M1 重启授权失败: {e}")))?;
        // 操作者口径（单元 J 订正，同 `do_request_release`）：T-3 无登录 ⇒ 恒 `local-console`。
        self.record_event(
            "interlock.ack_m1",
            "interlock",
            &format!(
                "M1 保护跳闸/停机人工授权重启（operator: {}）",
                mupc_display_proto::CONSOLE_OPERATOR
            ),
        )
        .await;
        self.alert_feed
            .push_interlock("ack_m1", "M1 保护跳闸/停机人工授权重启");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const E: InterlockSource = InterlockSource::Estop;
    const FLOOD: InterlockSource = InterlockSource::Flood;
    const FIRE: InterlockSource = InterlockSource::Fire;
    const DOOR: InterlockSource = InterlockSource::Door;

    fn pcs_ok() -> (bool, bool) {
        (true, true) // (run_state_ok, pcs_online)
    }

    #[test]
    fn test_trigger_edge_latches_and_lights_fault() {
        let mut m = StateMachine::new(false, 5);
        let mut s = InterlockState::default();
        let (act, run_lamp, fault_lamp) = m.tick(&mut s, &[(E, true)], false, true, true, 1000);

        assert_eq!(act, Action::TriggerLatch);
        assert!(s.latched);
        assert_eq!(s.active_pcs_stop_sources, BIT_ESTOP);
        assert!(s.source_safe_since.is_none());
        assert!(!run_lamp);
        assert!(fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::Interlock);
    }

    #[test]
    fn test_continuous_trip_does_not_repeat_trigger() {
        let mut m = StateMachine::new(false, 5);
        let mut s = InterlockState::default();
        let (a1, _, _) = m.tick(&mut s, &[(E, true)], false, true, true, 1000);
        assert_eq!(a1, Action::TriggerLatch);

        // 持续 tripped：不再重复触发
        let (a2, _, fault_lamp) = m.tick(&mut s, &[(E, true)], false, true, true, 1001);
        assert_eq!(a2, Action::None);
        assert!(s.latched);
        assert!(fault_lamp);
    }

    #[test]
    fn test_auto_release_after_hold_and_lamp_recovers() {
        let mut m = StateMachine::new(true, 3);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();

        let (a0, _, _) = m.tick(&mut s, &[(E, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);

        // t=1 源复位 → 记录 safe_since=1，hold 未满
        let (a1, _, fl1) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);
        assert!(s.latched);
        assert!(fl1);
        assert_eq!(s.source_safe_since, Some(1));

        // t=4 hold=3 满 + auto → 释放
        let (a2, run_lamp, fault_lamp) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 4);
        assert_eq!(a2, Action::ReleaseLatch);
        assert!(!s.latched);
        assert!(s.source_safe_since.is_none());
        assert!(run_lamp);
        assert!(!fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::None);
    }

    #[test]
    fn test_auto_release_blocked_when_stop_unconfirmed() {
        // Important（质量评审）：stop() 从未确认（stop_failed=true，PCS 仍运行）时，即使
        // auto_release=true + 源复位 + hold 满，**也不得**自动解 latch 清 stop_failed——否则
        // 联锁静默失效（故障灯熄、run 灯亮，但 PCS 从未真正停机）。仅人工 web release 可放行。
        let mut m = StateMachine::new(true, 1);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();
        let (a0, _, _) = m.tick(&mut s, &[(E, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);
        s.stop_failed = true; // 装配：停机确认超时（PCS 未转 0）

        // t=1 源复位、t=3 hold=1 满 + auto → 仍维持 latch（禁止静默失效）
        let (a1, _, _) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);
        let (a2, _, fault_lamp) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 3);
        assert_eq!(a2, Action::None);
        assert!(s.latched);
        assert!(s.stop_failed);
        assert!(fault_lamp);

        // 人工 web release 可放行 stop_failed 态（操作员明确确认）
        let (a3, run_lamp, _) = m.tick(&mut s, &[(E, false)], true, run_ok, online, 3);
        assert_eq!(a3, Action::ReleaseLatch);
        assert!(!s.latched);
        assert!(!s.stop_failed);
        assert!(run_lamp);
    }

    #[test]
    fn test_non_auto_without_web_request_stays_latched() {
        let mut m = StateMachine::new(false, 2);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();

        let (a0, _, _) = m.tick(&mut s, &[(FLOOD, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);

        let (a1, _, _) = m.tick(&mut s, &[(FLOOD, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);
        assert_eq!(s.source_safe_since, Some(1));

        // t=3 hold=2 满，但 auto=false 且无 web 请求 → 维持 latch
        let (a2, _, fault_lamp) = m.tick(&mut s, &[(FLOOD, false)], false, run_ok, online, 3);
        assert_eq!(a2, Action::None);
        assert!(s.latched);
        assert!(fault_lamp);
    }

    #[test]
    fn test_web_release_pending_allows_release() {
        let mut m = StateMachine::new(false, 2);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();

        let (a0, _, _) = m.tick(&mut s, &[(FIRE, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);
        let (a1, _, _) = m.tick(&mut s, &[(FIRE, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);

        // hold 满且 web_release_pending=true → 释放
        let (a2, _, fault_lamp) = m.tick(&mut s, &[(FIRE, false)], true, run_ok, online, 3);
        assert_eq!(a2, Action::ReleaseLatch);
        assert!(!s.latched);
        assert!(!fault_lamp);
    }

    #[test]
    fn test_m1_online_but_run_state_bad_lights_fault() {
        let mut m = StateMachine::new(true, 0);
        let mut s = InterlockState::default();
        // pcs_online=true, run_state_ok=false, 未 latch
        let (act, run_lamp, fault_lamp) = m.tick(&mut s, &[], false, false, true, 1000);

        assert_eq!(act, Action::None);
        assert!(!run_lamp);
        assert!(fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::M1Stopped);
    }

    #[test]
    fn test_run_ok_and_not_latched_lights_run_only() {
        let mut m = StateMachine::new(true, 0);
        let mut s = InterlockState::default();
        let (act, run_lamp, fault_lamp) = m.tick(&mut s, &[], false, true, true, 1000);

        assert_eq!(act, Action::None);
        assert!(run_lamp);
        assert!(!fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::None);
    }

    #[test]
    fn test_stop_failed_lights_fault() {
        let mut m = StateMachine::new(true, 0);
        let mut s = InterlockState::default();
        s.stop_failed = true; // 装配在停机确认超时后置位
        let (_, _, fault_lamp) = m.tick(&mut s, &[], false, true, true, 1000);

        assert!(fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::Interlock);
    }

    #[test]
    fn test_pcs_offline_lights_fault_reason_pcs_offline() {
        let mut m = StateMachine::new(true, 0);
        let mut s = InterlockState::default();
        let (_, _, fault_lamp) = m.tick(&mut s, &[], false, true, false, 1000);

        assert!(fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::PcsOffline);
    }

    #[test]
    fn test_door_is_event_only_never_latches() {
        let mut m = StateMachine::new(false, 5);
        let mut s = InterlockState::default();
        // 门禁 tripped 不计入 pcs_stop 源集合 → 不触发 latch
        let (act, _, fault_lamp) = m.tick(&mut s, &[(DOOR, true)], false, true, true, 1000);

        assert_eq!(act, Action::None);
        assert!(!s.latched);
        assert_eq!(s.active_pcs_stop_sources, 0);
        assert!(!fault_lamp);
    }

    #[test]
    fn test_relatch_after_release_requires_new_edge() {
        let mut m = StateMachine::new(true, 1);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();

        // 触发 → 复位 hold 1s → 自动释放
        let (a0, _, _) = m.tick(&mut s, &[(E, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);
        let (a1, _, _) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);
        let (a2, _, _) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 2);
        assert_eq!(a2, Action::ReleaseLatch);
        assert!(!s.latched);

        // 释放后源再次 tripped → 新沿 → 重新触发
        let (a3, _, fl) = m.tick(&mut s, &[(E, true)], false, run_ok, online, 3);
        assert_eq!(a3, Action::TriggerLatch);
        assert!(s.latched);
        assert!(fl);
    }
}

#[cfg(test)]
mod runner_tests {
    use super::*;
    use crate::alert_feed::AlertFeed;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use mupc_io::{IoError, MockIn, MockOut};
    use mupc_storage::StorageError;
    use std::sync::{Arc, Mutex};

    use crate::core_config::{DiConf, DoConf, IoConfig};

    // ── 测试替身 ──

    /// 可外部改电平的 DI（包 Arc<MockIn>）
    struct SharedIn(Arc<MockIn>);
    impl DigitalIn for SharedIn {
        fn read_level(&self) -> Result<bool, IoError> {
            self.0.read_level()
        }
    }
    /// 可外部断言电平的 DO（包 Arc<MockOut>）
    struct SharedOut(Arc<MockOut>);
    impl DigitalOut for SharedOut {
        fn set_level(&self, high: bool) -> Result<(), IoError> {
            self.0.set_level(high)
        }
    }

    #[derive(Default)]
    struct FakePortInner {
        calls: Vec<String>,
        run_state: Option<u16>,
        stop_ok: bool,
        stop_sets_zero: bool,
        /// `restore_latched` 是否成功（默认 `true`；置 `false` 驱动 `Internal` 分支）。
        restore_ok: bool,
        /// `authorize_restart` 是否成功（默认 `true`）。
        authorize_ok: bool,
        /// 置位后 `restore_latched` 会**先**等一个 `Notify` 才返回 —— 供并发用例把
        /// 「写操作闸已被占住」这一瞬间**确定性地**造出来（不靠 sleep 猜时序）。
        gate: Option<Arc<tokio::sync::Notify>>,
        /// 已进入 `restore_latched`（等待 `gate` 前）—— 并发用例据此确认抢占先于第二次调用。
        gated_in: bool,
    }

    struct FakePort {
        inner: Arc<Mutex<FakePortInner>>,
    }
    impl FakePort {
        fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(FakePortInner {
                    restore_ok: true,
                    authorize_ok: true,
                    ..Default::default()
                })),
            }
        }
        fn set_run_state(&self, s: Option<u16>) {
            self.inner.lock().unwrap().run_state = s;
        }
        fn inner(&self) -> Arc<Mutex<FakePortInner>> {
            self.inner.clone()
        }
    }

    #[async_trait]
    impl InterlockPort for FakePort {
        async fn stop(&self) -> Result<(), String> {
            let mut g = self.inner.lock().unwrap();
            g.calls.push("stop".into());
            if !g.stop_ok {
                return Err("fake: stop 写失败".into());
            }
            if g.stop_sets_zero {
                g.run_state = Some(0);
            }
            Ok(())
        }
        async fn restore_latched(&self, latched: bool) -> Result<(), String> {
            let gate = {
                let mut g = self.inner.lock().unwrap();
                g.calls.push(format!("restore_latched({})", latched));
                g.gated_in = true;
                let ok = g.restore_ok;
                if !ok {
                    // 失败注入：状态与调用记录都留着（调用方据此断言"确实尝试过"）
                    return Err("fake: restore_latched 写失败".into());
                }
                g.gate.clone()
            };
            if let Some(n) = gate {
                // `notify_one`（不是 `notify_waiters`）：前者在"尚无等待者"时**存一个许可**
                // ⇒ 放闸方先发信号也不会丢（并发用例里放闸点在断言之后，必然晚于注册）。
                n.notified().await;
            }
            Ok(())
        }
        fn last_run_state(&self) -> Option<u16> {
            self.inner.lock().unwrap().run_state
        }
        async fn authorize_restart(&self) -> Result<(), String> {
            let mut g = self.inner.lock().unwrap();
            g.calls.push("authorize_restart".into());
            if !g.authorize_ok {
                return Err("fake: authorize_restart 写失败".into());
            }
            Ok(())
        }
    }

    /// 内存 EventRepository（断言事件 + latest_by_type）
    #[derive(Default)]
    struct FakeEventRepo {
        inner: Arc<Mutex<Vec<SystemEvent>>>,
    }
    impl FakeEventRepo {
        fn has(&self, event_type: &str) -> bool {
            self.inner
                .lock()
                .unwrap()
                .iter()
                .any(|e| e.event_type == event_type)
        }
        fn count(&self, event_type: &str) -> usize {
            self.inner
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.event_type == event_type)
                .count()
        }
        fn msgs(&self, event_type: &str) -> Vec<String> {
            self.inner
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.event_type == event_type)
                .map(|e| e.message.clone())
                .collect()
        }
    }
    #[async_trait]
    impl EventRepository for FakeEventRepo {
        async fn insert(&self, event: &SystemEvent) -> Result<i64, StorageError> {
            let mut g = self.inner.lock().unwrap();
            g.push(event.clone());
            Ok(g.len() as i64)
        }
        async fn query_range(
            &self,
            _start: DateTime<Utc>,
            _end: DateTime<Utc>,
        ) -> Result<Vec<SystemEvent>, StorageError> {
            Ok(vec![])
        }
        async fn purge_older_than(&self, _before: DateTime<Utc>) -> Result<usize, StorageError> {
            Ok(0)
        }
        async fn latest_by_type(
            &self,
            event_type: &str,
        ) -> Result<Option<SystemEvent>, StorageError> {
            let g = self.inner.lock().unwrap();
            Ok(g.iter()
                .filter(|e| e.event_type == event_type)
                .max_by_key(|e| e.timestamp)
                .cloned())
        }
    }

    // ── 构造辅助 ──

    fn io_cfg(auto_release: bool, stop_confirm_ms: u64) -> IoConfig {
        IoConfig {
            enabled: true,
            poll_ms: 5,
            auto_release,
            release_hold_secs: 0,
            stop_confirm_ms,
            di: vec![DiConf {
                name: "急停".into(),
                gpio: 1,
                active_low: false,
                debounce: 1,
                action: "pcs_stop".into(),
            }],
            do_out: vec![
                DoConf {
                    name: "运行灯".into(),
                    gpio: 8,
                    active_high: true,
                },
                DoConf {
                    name: "故障灯".into(),
                    gpio: 9,
                    active_high: true,
                },
            ],
        }
    }

    /// 建单 DI(急停,pcs_stop,active_high 触发)+DO(运行/故障) 控制器，返回句柄
    #[allow(clippy::type_complexity)]
    fn estop_board(
        cfg: IoConfig,
        port: FakePort,
        events: Arc<FakeEventRepo>,
    ) -> (
        Arc<InterlockController>,
        Arc<MockIn>,
        Arc<MockOut>,
        Arc<MockOut>,
        Arc<Mutex<FakePortInner>>,
    ) {
        let pin = Arc::new(MockIn::new());
        let run = Arc::new(MockOut::new());
        let fault = Arc::new(MockOut::new());
        let alert_feed = Arc::new(AlertFeed::new());
        let inner = port.inner();
        let ctl = Arc::new(InterlockController::new_with_io(
            cfg,
            vec![Box::new(SharedIn(pin.clone()))],
            vec![
                Box::new(SharedOut(run.clone())),
                Box::new(SharedOut(fault.clone())),
            ],
            Box::new(port),
            events,
            alert_feed,
        ));
        (ctl, pin, run, fault, inner)
    }

    // ── 测试 ──

    /// review 关注：自动释放要求停机确认；停机确认成功 + 源复位 → 自动释放 + 灯恢复
    #[tokio::test]
    async fn trigger_then_auto_release_after_stop_confirmed() {
        let cfg = io_cfg(true, 1000);
        let port = FakePort::new();
        port.set_run_state(Some(2)); // PCS 在线运行
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = true; // stop 后 run_state 转 0
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, run, fault, inner) = estop_board(cfg, port, events.clone());

        // 帧1：DI 有效 → 触发 latch + 停机(确认成功)
        pin.set(true);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now(), "触发后应 latch");
        assert!(!run.get(), "latch 时运行灯应灭");
        assert!(fault.get(), "latch 时故障灯应亮");
        assert!(events.has("interlock.triggered"));
        let calls = { inner.lock().unwrap().calls.clone() };
        let i_restore = calls
            .iter()
            .position(|c| c == "restore_latched(true)")
            .expect("restore(true)");
        let i_stop = calls.iter().position(|c| c == "stop").expect("stop");
        assert!(
            i_restore < i_stop,
            "restore(true) 必须先于 stop，实际 {:?}",
            calls
        );

        // 帧2：源复位（DI 无效）+ auto_release + 停机已确认 → 自动释放
        pin.set(false);
        ctl.tick_frame().await;
        assert!(!ctl.is_latched_now(), "auto_release+确认后应释放");
        let calls = { inner.lock().unwrap().calls.clone() };
        assert!(
            calls.iter().any(|c| c == "restore_latched(false)"),
            "释放应 restore(false)"
        );
        assert!(events.has("interlock.cleared"));

        // 释放后 PCS 处于 run_state=0（停机），非 {1,2,3} → 故障灯亮 M1Stopped（M1：在线但停机）
        assert!(fault.get(), "在线但 PCS 停机（run_state=0）→ 故障灯(M1)");
        assert!(!run.get(), "PCS 未在运行态，运行灯不应亮");

        // 人工重启 PCS 回 run_state=2 → 灯恢复（运行灯亮、故障灯灭）
        inner.lock().unwrap().run_state = Some(2);
        ctl.tick_frame().await;
        assert!(run.get(), "PCS 恢复运行且未 latch → 运行灯亮");
        assert!(!fault.get(), "PCS 运行正常 → 故障灯灭");
    }

    /// stop() 写失败 → stop_failed；即使 auto_release=true 也不自动释放；人工放行出审计事件
    #[tokio::test]
    async fn stop_write_failure_blocks_auto_and_manual_release_audits_override() {
        let cfg = io_cfg(true, 40);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = false; // stop 写失败
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, fault, inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now());
        assert!(
            ctl.state.read().unwrap().stop_failed,
            "stop 写失败应置 stop_failed"
        );
        assert!(events.has("interlock.stop_failed"));
        assert!(fault.get(), "stop_failed 应亮故障灯");

        // 源复位：auto=true 但 stop_failed → 不自动释放
        pin.set(false);
        ctl.tick_frame().await;
        assert!(
            ctl.is_latched_now(),
            "stop_failed 时禁止自动释放（防联锁静默失效）"
        );
        assert!(ctl.state.read().unwrap().stop_failed);

        // 人工放行 → Ok + 审计（override）
        let r = ctl.request_release().await;
        assert!(r.is_ok(), "人工放行应成功: {:?}", r.err());
        assert!(!ctl.is_latched_now());
        let msgs = events.msgs("interlock.cleared");
        assert!(
            msgs.iter().any(|m| m.contains("override")),
            "stop_failed 人工放行应留 override 审计，实际 {:?}",
            msgs
        );
        let calls = { inner.lock().unwrap().calls.clone() };
        assert!(calls.iter().any(|c| c == "restore_latched(false)"));
    }

    /// 停机确认超时（stop Ok 但 run_state 未转 0）→ stop_failed + 禁止自动释放。
    /// S2 Task7 Important 非阻塞语义：触发帧只发**一次**停机写（不自旋），stop_failed 由
    /// `post_stop_maintenance` 在越过 stop_confirm_ms 窗口后的维护帧首次置位（多 ≤1 帧 poll_ms）。
    #[tokio::test]
    async fn stop_confirm_timeout_sets_stop_failed_and_blocks_auto() {
        let cfg = io_cfg(true, 100);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = false; // PCS 拒不转 0
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        let stop_calls = |inner: &Arc<Mutex<FakePortInner>>| {
            inner
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|c| c.as_str() == "stop")
                .count()
        };

        pin.set(true);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now());
        assert_eq!(stop_calls(&inner), 1, "触发帧只发一次停机写（无内联自旋）");
        assert!(
            !ctl.state.read().unwrap().stop_failed,
            "窗口未过不应立即置 stop_failed"
        );

        // 越过 stop_confirm_ms 窗口 → 维护帧首次置 stop_failed
        tokio::time::sleep(Duration::from_millis(150)).await;
        ctl.tick_frame().await;
        assert!(
            ctl.state.read().unwrap().stop_failed,
            "确认超时应置 stop_failed"
        );
        assert_eq!(events.count("interlock.stop_failed"), 1);

        pin.set(false);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now(), "stop_failed 时 auto 不放行");

        // 人工放行
        assert!(ctl.request_release().await.is_ok());
        assert!(!ctl.is_latched_now());
    }

    /// S2 Task7（Important 2 ①③）：stop Ok 但 run 未转 0 → 触发帧不置位；窗口内不重复发不置位；
    /// 越过窗口首次 mark（一次事件）；之后每 ≥ 窗口退避重发（写计数递增）但 mark 幂等事件不重复。
    #[tokio::test]
    async fn unconfirmed_stop_timeout_marks_once_and_backoffs_retry() {
        let cfg = io_cfg(true, 100);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = false; // PCS 拒不转 0
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        let stop_calls = |inner: &Arc<Mutex<FakePortInner>>| {
            inner
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|c| c.as_str() == "stop")
                .count()
        };

        // 帧0：触发 → latch + 单次停机写；窗口未过 → 不置 stop_failed
        pin.set(true);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now());
        assert_eq!(stop_calls(&inner), 1, "触发帧只发一次停机写");
        assert!(
            !ctl.state.read().unwrap().stop_failed,
            "窗口内不应置 stop_failed"
        );

        // 窗口内再 tick（源仍触发）→ 不重发、不置位
        ctl.tick_frame().await;
        assert_eq!(stop_calls(&inner), 1, "确认窗口内不应重发 stop");
        assert!(!ctl.state.read().unwrap().stop_failed);

        // 越过窗口 → 首次置 stop_failed（一次事件）+ 重发一次
        tokio::time::sleep(Duration::from_millis(150)).await;
        ctl.tick_frame().await;
        assert!(
            ctl.state.read().unwrap().stop_failed,
            "确认超时首次置 stop_failed"
        );
        assert_eq!(events.count("interlock.stop_failed"), 1, "超时事件只记一次");
        assert_eq!(stop_calls(&inner), 2, "超时后补发一次停机");

        // 越过下一窗口 → mark 幂等（事件不重复），但每窗口退避仍重发
        tokio::time::sleep(Duration::from_millis(150)).await;
        ctl.tick_frame().await;
        assert!(ctl.state.read().unwrap().stop_failed);
        assert_eq!(
            events.count("interlock.stop_failed"),
            1,
            "mark 幂等事件不重复"
        );
        assert_eq!(stop_calls(&inner), 3, "每 ≥ 窗口退避重发一次停机");
    }

    /// S2 Task7（Important 2 ②延迟确认清除）：stop_failed 已置（曾超时）后 run 转 Some(0)
    /// → post_stop_maintenance 清除 stop_failed（一次 stopped 事件）；源复位后自动释放放行。
    #[tokio::test]
    async fn delayed_confirm_clears_stop_failed_when_run_returns_zero() {
        let cfg = io_cfg(true, 100);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = false;
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        ctl.tick_frame().await;
        // 越过窗口置 stop_failed
        tokio::time::sleep(Duration::from_millis(150)).await;
        ctl.tick_frame().await;
        assert!(ctl.state.read().unwrap().stop_failed);
        assert_eq!(events.count("interlock.stop_failed"), 1);

        // PCS 转 0（延迟确认）→ 清除
        inner.lock().unwrap().run_state = Some(0);
        ctl.tick_frame().await;
        assert!(
            !ctl.state.read().unwrap().stop_failed,
            "run 转 Some(0) 应清除 stop_failed"
        );
        assert_eq!(
            events.count("interlock.stopped"),
            1,
            "清除应记一次 stopped 事件"
        );

        // 源复位 + auto_release + !stop_failed → 自动释放
        pin.set(false);
        ctl.tick_frame().await;
        assert!(!ctl.is_latched_now(), "确认清除后源复位可自动释放");
    }

    /// S2 Task7（Important 2 liveness ④）：latch + PCS 拒不转 0 时，单帧 tick_frame 不得内联自旋阻塞
    /// （旧版 stop_and_confirm 会自旋满 stop_confirm_ms）。用超长窗口 + timeout 断言每帧即时返回。
    #[tokio::test]
    async fn latched_tick_frame_does_not_block_on_stop_confirmation() {
        let cfg = io_cfg(true, 60_000); // 窗口 60s：若残留内联自旋必致 timeout Err
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = false; // 永不停
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, _inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        let res = tokio::time::timeout(Duration::from_millis(300), ctl.tick_frame()).await;
        assert!(res.is_ok(), "触发帧不得内联自旋阻塞 run_loop");
        assert!(ctl.is_latched_now());
        // 连续快速多帧（latch 下每帧 post_stop_maintenance）都应即时返回，验证 run_loop liveness
        for _ in 0..5 {
            let res = tokio::time::timeout(Duration::from_millis(300), ctl.tick_frame()).await;
            assert!(res.is_ok(), "维护帧不得阻塞（逐帧非阻塞确认）");
        }
    }

    /// request_release 前置：触发源未复位 → `SourcesNotReset`（列出**具体**源 token）
    ///
    /// 迁移前断言 `err.contains("未复位")`（字符串）；迁移后断言**结构化变体 + 字段值**——
    /// 这正是 EDGE-12「明示具体拒绝原因」在契约层的判别力所在。
    #[tokio::test]
    async fn request_release_rejected_while_source_still_tripped() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = true;
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, _inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now());

        // 源仍触发 → 拒绝
        let err = ctl.request_release().await.expect_err("源未复位应 Err");
        assert_eq!(
            err,
            InterlockReject::SourcesNotReset {
                remaining: vec!["estop".to_string()]
            },
            "必须点名**具体**未复位源（不是一句『未复位』）"
        );
        assert!(ctl.is_latched_now(), "拒绝时 latch 应保持");

        // 源复位后 → 成功
        pin.set(false);
        assert!(ctl.request_release().await.is_ok());
        assert!(!ctl.is_latched_now());
    }

    /// **保持时间不足** ⇒ `HoldNotElapsed`，且**带剩余秒数**（UI 倒计时用）。
    ///
    /// 造法：`release_hold_secs=30`，触发 → 复位后**不 tick**（`source_safe_since` 仍为
    /// `None`）⇒ 已守 0 s、余 30 s。这条同时钉死「计时未起步」这一路径（修复前是另一句
    /// 中文串"触发源未完全复位"）。
    #[tokio::test]
    async fn request_release_rejects_hold_not_elapsed_with_remaining_secs() {
        let cfg = io_cfg(false, 1000); // io_cfg 的 release_hold_secs=0 ⇒ 本用例改成 30
        let mut cfg = cfg;
        cfg.release_hold_secs = 30;
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = true;
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, _inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now());
        // 源复位但**不 tick**：状态机还没记下 `source_safe_since`
        pin.set(false);
        let err = ctl.request_release().await.expect_err("保持未满应 Err");
        assert_eq!(
            err,
            InterlockReject::HoldNotElapsed {
                need_secs: 30,
                remaining_secs: 30
            },
            "计时未起步 ⇒ 剩余 = 全额（不臆造『已守若干秒』）"
        );
        assert!(ctl.is_latched_now(), "拒绝时 latch 应保持");

        // tick 一帧 → 记下 safe_since（now-0s）⇒ 剩余仍是 30 s（**不是**变成可释放）
        ctl.tick_frame().await;
        let err2 = ctl.request_release().await.expect_err("刚复位即释放应 Err");
        assert!(
            matches!(err2, InterlockReject::HoldNotElapsed { remaining_secs, .. } if remaining_secs >= 1),
            "复位后须守满 30 s，实际 {err2:?}"
        );
    }

    /// **未启用**（`io.enabled=false`）⇒ `NotEnabled`（两条写路径**都**要挡）。
    ///
    /// 生产装配在 `io.enabled=false` 时**不构造**控制器（屏上由 `InterlockWiring::Disabled`
    /// 表达）⇒ 本用例所钉的这条路径**在生产装配下同样不可达**；它钉的是**第二道防线**：
    /// 即便构造出来（或被将来某条装配路径漏判），写路径也必须回**具体**的 `NotEnabled`，
    /// 而不是"看不见地成功"。（与 `ack_m1` 的 `StopPending` 门同一性质：防御性，非可达判据。）
    #[tokio::test]
    async fn write_ops_reject_not_enabled_when_io_disabled() {
        let mut cfg = io_cfg(false, 1000);
        cfg.enabled = false;
        let port = FakePort::new();
        port.set_run_state(Some(2));
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        // 先造出"已 latch"（否则 release 会走"未 latch ⇒ Ok"的幂等分支，测不到 NotEnabled）
        pin.set(true);
        ctl.tick_frame().await;

        assert_eq!(
            ctl.request_release().await.unwrap_err(),
            InterlockReject::NotEnabled
        );
        assert_eq!(ctl.ack_m1().await.unwrap_err(), InterlockReject::NotEnabled);
        assert!(
            !inner
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c.starts_with("restore_latched(false)") || c == "authorize_restart"),
            "未启用 ⇒ 一个 transport 动作都不许发（不得静默执行）"
        );
        // 状态视图也必须如实报未启用（不得硬编码 true）
        assert!(!ctl.status().await.enabled, "enabled 须回读 cfg.enabled");
    }

    /// **不可达态防御测试**：直接构造 `!latched && stop_failed` 这一**按构造不可达**的状态
    /// （`stop_failed ⟹ latched` 由 [`InterlockController::mark_stop_failed`] 的 `latched` 门
    /// 保证，见 `InterlockApi::ack_m1` 文档——真实可达态下 `ack_m1` 先被 `latched` 门挡，
    /// 回的是 [`InterlockReject::Latched`]），钉住纵深防御门：一旦将来不变量被打破，该态必须
    /// 如实映射到 `StopPending`（契约文案「暂不可授权重启」），而不是静默放行。
    ///
    /// ⚠️ **本用例不是"危险动作被本门挡住"的证据**——挡住它的是状态机不变量 + `latched` 门。
    /// **也不是"该态生产不可达"的证据**：本用例只证"若到达则如实映射"；不可达性由
    /// `stop_failed_never_sticks_when_a_concurrent_release_wins` 独立证明。
    #[tokio::test]
    async fn ack_m1_rejects_stop_pending_when_stop_unconfirmed() {
        let cfg = io_cfg(false, 40);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = false; // 停机写失败 ⇒ stop_failed
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        ctl.tick_frame().await;
        assert!(ctl.state.read().unwrap().stop_failed);
        // 源复位 + 人工 release 放行 stop_failed ⇒ 此时 !latched 但 stop_failed 仍为 false
        // ……不成立：release 会一并清 stop_failed。故本用例直接构造"未 latch + stop_failed"：
        pin.set(false);
        ctl.tick_frame().await;
        {
            let mut st = ctl.state.write().unwrap();
            st.latched = false;
            st.stop_failed = true;
        }
        assert_eq!(
            ctl.ack_m1().await.unwrap_err(),
            InterlockReject::StopPending,
            "防御门：人工构造的不可达态必须被如实映射到 StopPending（不得静默放行）"
        );
        assert!(
            !inner
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c == "authorize_restart"),
            "被拒 ⇒ 不得发出授权（不得先发后拒）"
        );

        // **不变量对照（单元 J 第一轮整改新增）**：真实可达态是 `latched && stop_failed`
        // （`stop_failed ⟹ latched`，两标志同置同清）⇒ 此时走的是 **`latched` 门**、回
        // `Latched`——这正是"危险动作被挡"的真实归因（不变量 + `latched` 门），
        // 而非上面那道**防御性**的 `StopPending` 门（其可达性已由 I-2 的门构造性关闭）。
        {
            let mut st = ctl.state.write().unwrap();
            st.latched = true;
            st.stop_failed = true;
        }
        assert_eq!(
            ctl.ack_m1().await.unwrap_err(),
            InterlockReject::Latched,
            "真实可达态（stop_failed ⟹ latched）先被 latched 门挡 ⇒ StopPending 门在该态不可达"
        );
        assert!(
            !inner
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c == "authorize_restart"),
            "latched 态同样不得发出授权"
        );
        // 回到正对照的起点（未 latch + 停机已确认）
        {
            let mut st = ctl.state.write().unwrap();
            st.latched = false;
        }
        // 正对照（证明上面那条不是"恒 Err"）：停机确认 + 未 latch ⇒ 放行
        ctl.state.write().unwrap().stop_failed = false;
        assert!(ctl.ack_m1().await.is_ok());
        assert!(inner
            .lock()
            .unwrap()
            .calls
            .iter()
            .any(|c| c == "authorize_restart"));
    }

    /// **I-2（单元 J 第二轮整改）：`mark_stop_failed` 的 `latched` 门 ⇒ 该态不再粘住。**
    ///
    /// # 造法（**确定性**，不靠 sleep 猜时序）
    ///
    /// 闸（`Notify`）装到 `restore_latched` 上 ⇒ 触发帧的 `on_trigger` 停在
    /// `restore_latched(true)` 的 await 点：此刻 `tick` 已把 `latched=true` 写进共享状态，
    /// 但触发沿**还没走到** `stop_once`。趁着这个窗口，本任务完成一次"并发 release 抢先"
    /// ——等价于 `do_request_release` 的收尾块（`had_stop_failed` 处，**两标志一起清**），并把
    /// `stop_ok` 改成 `false`（让迟到的停机写失败）。放闸后触发沿继续 ⇒ **迟到的
    /// `stop_once` → `mark_stop_failed` 在 `!latched` 下被调用**——这正是评审指出的
    /// 跨线程 check-then-act 窗口，本用例把它**确定性**地钉住。
    ///
    /// # 修法前 / 后
    ///
    /// - 修法前：`stop_failed` 被置回 `true` ⇒ `!latched && stop_failed` 成立且**粘住**
    ///   （该态**无任何清除路径**：`tick` 的清零分支要求 `latched`、`clear_stop_failed_once`
    ///   只在 `post_stop_maintenance` 的 `latched` 门后可达、`do_request_release` 在
    ///   `!latched` 时提前 `Ok(())`）⇒ 下面两条断言全红（故障灯常亮 + `ack_m1` 长期
    ///   `StopPending`）。
    /// - 修法后：门在**同一把写锁内**判定 ⇒ 不写 ⇒ `ack_m1` 正常放行。
    ///
    /// **改什么会让本条变红**：删掉 `mark_stop_failed` 里的 `if !st.latched` 门
    /// （破坏性验证记录见交付说明）。
    #[tokio::test]
    async fn stop_failed_never_sticks_when_a_concurrent_release_wins() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        // 闸装到 `restore_latched` 上：`on_trigger` 的第一个 await 点（先于 `stop_once`）
        let gate = Arc::new(tokio::sync::Notify::new());
        inner.lock().unwrap().gate = Some(gate.clone());

        pin.set(true);
        let frame = {
            let c = ctl.clone();
            tokio::spawn(async move { c.tick_frame().await })
        };
        for _ in 0..1000 {
            if inner.lock().unwrap().gated_in {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            inner.lock().unwrap().gated_in,
            "前提：触发沿已停在 restore_latched(true) 的 await 点上"
        );
        assert!(ctl.is_latched_now(), "前提：tick 已置 latched=true");

        // 让**迟到**的那次停机写失败（它发生在闸后）⇒ 触发沿会走到 mark_stop_failed
        inner.lock().unwrap().stop_ok = false;
        // **并发 release 抢先**：等价于 do_request_release 的收尾块（had_stop_failed 处，两标志一起清）
        {
            let mut st = ctl.state.write().unwrap();
            st.latched = false;
            st.stop_failed = false;
        }
        gate.notify_one();
        frame.await.unwrap();

        // ① `!latched` ⇒ 停机失败标记**不得**被写入（写下去就是粘住态，无清除路径）
        {
            let st = ctl.state.read().unwrap();
            assert!(
                !st.latched,
                "前提：并发 release 已把 latch 清掉（与 do_request_release 收尾块 had_stop_failed 处同款）"
            );
            assert!(
                !st.stop_failed,
                "`!latched && stop_failed` 是**粘住态**（无清除路径 ⇒ ack_m1 长期 StopPending + \
                 故障灯常亮）；`mark_stop_failed` 的 `latched` 门之后不得出现"
            );
        }
        assert!(
            !events.has("interlock.stop_failed"),
            "门挡下 ⇒ 也不得记一条「停机失败」事件（如实：该标记此刻已无对象）"
        );

        // ② 用户可见面：`ack_m1` **不再长期 StopPending**（缺口态下这里恒 Err 且永不自愈）
        assert!(
            ctl.ack_m1().await.is_ok(),
            "修法后必须能正常授权重启（修法前恒 StopPending）"
        );
        assert!(
            inner
                .lock()
                .unwrap()
                .calls
                .iter()
                .any(|c| c == "authorize_restart"),
            "正对照：放行必须**真的**发出授权（上面那条不是恒真的 Ok）"
        );
    }

    /// **并发写** ⇒ `Busy`（闸被占住时后到者立刻返回，不排队、不改状态）。
    ///
    /// 造法：`FakePort::restore_latched` 被要求先等一个 `Notify` ⇒ 第一个 `request_release`
    /// 卡在 await 点（**闸已占**），此时同任务再调一次 ⇒ 必须 `Busy`。
    ///
    /// ⚠️ 闸**必须在触发帧之后**才装上：`Action::TriggerLatch` 也会调 `restore_latched(true)`，
    /// 提前装闸会让 `tick_frame` 自己卡死（本用例第一次实现就踩了这个坑）。
    #[tokio::test]
    async fn concurrent_write_ops_report_busy_without_second_effect() {
        let cfg = io_cfg(false, 1000); // hold=0 ⇒ 源复位即可释放
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = true;
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        ctl.tick_frame().await;
        pin.set(false);
        // 触发帧已过 ⇒ 现在装闸：之后的 `restore_latched` 都会停在 await 点上
        let gate = Arc::new(tokio::sync::Notify::new());
        inner.lock().unwrap().gate = Some(gate.clone());

        // 第一次释放：卡在 `restore_latched` 的 await 点（闸已占）
        let a = {
            let c = ctl.clone();
            tokio::spawn(async move { c.request_release().await })
        };
        for _ in 0..1000 {
            if ctl.op_in_flight() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(ctl.op_in_flight(), "前提：第一次写操作已占住闸");

        // 第二次（并发）：立刻 Busy，且**不**产生第二个 transport 动作
        assert_eq!(
            ctl.request_release().await.unwrap_err(),
            InterlockReject::Busy,
            "并发提交必须回 Busy（不排队、不重复执行）"
        );
        assert_eq!(
            ctl.ack_m1().await.unwrap_err(),
            InterlockReject::Busy,
            "两条写路径共用同一把闸"
        );
        let restore_calls = |inner: &Arc<Mutex<FakePortInner>>| {
            inner
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|c| c.starts_with("restore_latched(false)"))
                .count()
        };
        assert_eq!(restore_calls(&inner), 1, "Busy 不得产生第二个释放动作");

        // 放闸 ⇒ 第一次正常完成；闸**必须**已释放（否则控制器永久 Busy）
        gate.notify_one();
        assert!(a.await.unwrap().is_ok());
        assert!(!ctl.op_in_flight(), "闸必须随操作结束释放");
        assert!(!ctl.is_latched_now());
    }

    /// transport 失败 ⇒ `Internal(具体原因)`（**透传**，不吞、不降级成"成功"）。
    #[tokio::test]
    async fn transport_failures_map_to_internal_with_reason() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = true;
        port.inner().lock().unwrap().restore_ok = false; // 释放写 transport 失败
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        ctl.tick_frame().await;
        pin.set(false);

        let err = ctl
            .request_release()
            .await
            .expect_err("transport 失败应 Err");
        assert!(
            matches!(&err, InterlockReject::Internal(d) if d.contains("释放联锁失败")),
            "必须透传具体原因，实际 {err:?}"
        );
        assert!(
            ctl.is_latched_now(),
            "transport 未成功 ⇒ 本地 latch 不得被清"
        );

        // ack_m1 侧同款：停机已确认但授权写失败
        {
            let mut st = ctl.state.write().unwrap();
            st.latched = false;
            st.stop_failed = false;
        }
        inner.lock().unwrap().authorize_ok = false;
        let err = ctl.ack_m1().await.expect_err("授权失败应 Err");
        assert!(
            matches!(&err, InterlockReject::Internal(d) if d.contains("M1 重启授权失败")),
            "必须透传具体原因，实际 {err:?}"
        );
    }

    /// `status()` 迁移后的**新字段**口径：`available` / `release_hold_secs` / 灯态 `Some(..)`
    /// （契约 `InterlockView` 比旧 web-api `InterlockStatus` 多了这几项）。
    #[tokio::test]
    async fn status_view_carries_available_hold_and_lamp_options() {
        let mut cfg = io_cfg(false, 1000);
        cfg.release_hold_secs = 42;
        let port = FakePort::new();
        port.set_run_state(Some(2));
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, _pin, _run, _fault, _inner) = estop_board(cfg, port, events.clone());

        let v = ctl.status().await;
        assert!(v.available, "能被调用即源可用（设计 §4.6）");
        assert!(v.enabled);
        assert_eq!(v.release_hold_secs, 42, "UI 提示『须保持 N 秒』的真源");
        assert_eq!(v.fault_lamp, Some(false), "灯态是**明确值**，不是『未知』");
        assert_eq!(v.run_lamp, Some(true));
        assert!(v.ts_ms > 0, "取数时刻须有值（帧侧另按采集时刻覆写）");
        assert_eq!(v.sources.len(), 1);
        assert_eq!(v.sources[0].name, "estop");
    }

    /// 门禁/event DI：只记事件，不参与 latch
    #[tokio::test]
    async fn door_event_only_never_latches() {
        let mut cfg = io_cfg(false, 1000);
        cfg.di = vec![DiConf {
            name: "门禁".into(),
            gpio: 3,
            active_low: false,
            debounce: 1,
            action: "event".into(),
        }];
        let port = FakePort::new();
        port.set_run_state(Some(2));
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, _inner) = estop_board(cfg, port, events.clone());

        pin.set(true);
        ctl.tick_frame().await;
        assert!(!ctl.is_latched_now(), "门禁事件不应 latch");
        assert_eq!(events.count("interlock.di"), 1, "触发沿应记一次");

        pin.set(false);
        ctl.tick_frame().await;
        assert_eq!(events.count("interlock.di"), 2, "复位沿应再记一次");
    }

    /// GPIO 运行时读失败 → 按触发处理（fail-safe）+ 记一次读失败事件
    #[tokio::test]
    async fn di_read_failure_is_treated_as_trip_and_latches() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        port.inner().lock().unwrap().stop_sets_zero = true;
        let events = Arc::new(FakeEventRepo::default());
        let alert_feed = Arc::new(AlertFeed::new());
        let ctl = Arc::new(InterlockController::new_with_io(
            cfg,
            vec![Box::new(DiReadFailStub)],
            vec![],
            Box::new(port),
            events.clone(),
            alert_feed,
        ));
        ctl.tick_frame().await;
        assert!(
            ctl.is_latched_now(),
            "DI 读失败应按触发处理（fail-safe latch）"
        );
        assert!(events.has("interlock.di_read_failed"));
        // 读失败只记一次（防刷屏）
        ctl.tick_frame().await;
        assert_eq!(events.count("interlock.di_read_failed"), 1);
    }

    /// DB 读回：最新 triggered 无 cleared → restore latch + restore_latched(true) + restored 事件
    #[tokio::test]
    async fn db_readback_restores_latch_when_triggered_newer_than_cleared() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        port.set_run_state(Some(0)); // PCS 已停
        let events = Arc::new(FakeEventRepo::default());
        // 预置：triggered（无 cleared）
        events.inner.lock().unwrap().push(SystemEvent {
            id: None,
            timestamp: Utc::now() - chrono::Duration::seconds(60),
            event_type: "interlock.triggered".into(),
            source: "急停".into(),
            message: "历史触发".into(),
        });
        let (ctl, _pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());
        ctl.restore_from_db().await;
        assert!(ctl.state.read().unwrap().latched, "DB 读回应恢复 latch");
        assert!(
            ctl.state.read().unwrap().stop_failed,
            "恢复态停机未确认（fail-safe）"
        );
        let calls = { inner.lock().unwrap().calls.clone() };
        assert!(calls.iter().any(|c| c == "restore_latched(true)"));
        assert!(events.has("interlock.restored"));
    }

    /// DB 读回：cleared 最新 → 归一化不 latch + restore_latched(false)
    #[tokio::test]
    async fn db_readback_normalizes_cleared_latest() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        let events = Arc::new(FakeEventRepo::default());
        events.inner.lock().unwrap().push(SystemEvent {
            id: None,
            timestamp: Utc::now() - chrono::Duration::seconds(120),
            event_type: "interlock.triggered".into(),
            source: "急停".into(),
            message: "历史触发".into(),
        });
        events.inner.lock().unwrap().push(SystemEvent {
            id: None,
            timestamp: Utc::now() - chrono::Duration::seconds(60),
            event_type: "interlock.cleared".into(),
            source: "interlock".into(),
            message: "历史释放".into(),
        });
        let (ctl, _pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());
        ctl.restore_from_db().await;
        assert!(!ctl.state.read().unwrap().latched);
        let calls = { inner.lock().unwrap().calls.clone() };
        assert!(calls.iter().any(|c| c == "restore_latched(false)"));
    }

    /// ack_m1：latch 时 `Latched`；非 latch 时授权重启 + 事件
    #[tokio::test]
    async fn ack_m1_gated_by_latch() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, _inner) = estop_board(cfg, port, events.clone());

        // 非 latch：授权
        assert!(ctl.ack_m1().await.is_ok());
        assert!(events.has("interlock.ack_m1"));

        // latch：拒绝（结构化变体；契约文案「处于 latch 态，须先释放联锁」）
        pin.set(true);
        ctl.tick_frame().await;
        let err = ctl.ack_m1().await.expect_err("latch 时应 Err");
        assert_eq!(err, InterlockReject::Latched);
        assert!(err.user_message().contains("先释放联锁"), "上屏文案");
    }

    /// status()：暴露 sources / latch / 灯位
    #[tokio::test]
    async fn status_reflects_state_and_sources() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        port.set_run_state(Some(2));
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, _inner) = estop_board(cfg, port, events.clone());

        let st0 = ctl.status().await;
        assert!(st0.enabled);
        assert!(!st0.latched);
        assert_eq!(st0.run_lamp, Some(true), "在线运行且未 latch → 运行灯");

        pin.set(true);
        ctl.tick_frame().await;
        let st1 = ctl.status().await;
        assert!(st1.latched);
        assert_eq!(st1.run_lamp, Some(false));
        assert_eq!(st1.fault_lamp, Some(true));
        assert!(st1.sources.iter().any(|s| s.name == "estop" && s.tripped));
    }

    /// P2-1：fail-safe init 预置（`new()` 同步构造只设本地 state.latched）后，`run_loop` 首帧
    /// `sync_failsafe_latch()` 补 transport restore(true) + DB triggered（C-1 双 latch 的 transport
    /// 兜底 / 重启 `restore_from_db` 读回依据）。同时确认 stub 恒触发不致重复 TriggerLatch 刷屏。
    #[tokio::test]
    async fn failsafe_preset_syncs_transport_and_db_on_loop_start() {
        let cfg = io_cfg(false, 1000);
        let port = FakePort::new();
        port.inner().lock().unwrap().run_state = Some(2); // PCS 在线运行
        let events = Arc::new(FakeEventRepo::default());
        let run = Arc::new(MockOut::new());
        let fault = Arc::new(MockOut::new());
        let inner = port.inner();
        let alert_feed = Arc::new(AlertFeed::new());
        let ctl = Arc::new(InterlockController::new_with_io(
            cfg,
            vec![Box::new(DiReadFailStub) as Box<dyn DigitalIn>], // pcs_stop DI init 失败 stub
            vec![
                Box::new(SharedOut(run.clone())),
                Box::new(SharedOut(fault.clone())),
            ],
            Box::new(port),
            events.clone(),
            alert_feed,
        ));
        // 模拟 new() 的 any_pcs_failed 分支：预置本地 latch + 标记（transport/DB 尚未同步）
        ctl.preset_failsafe();
        assert!(
            ctl.is_latched_now(),
            "fail-safe 预置应立即 latch（dispatch 抑制生效）"
        );

        // run_loop 首帧：sync 补 transport restore(true) + DB triggered（P2-1）
        ctl.sync_failsafe_latch().await;
        let calls = { inner.lock().unwrap().calls.clone() };
        assert!(
            calls.iter().any(|c| c == "restore_latched(true)"),
            "fail-safe 应补 transport latch（C-1 兜底），实际 {:?}",
            calls
        );
        assert!(
            events.has("interlock.triggered"),
            "fail-safe 应补 DB triggered（重启读回依据）"
        );
        assert!(ctl.is_latched_now(), "sync 后仍 latch");

        // 幂等：第二次 sync（标记已清）不重复 restore/事件
        let before = { inner.lock().unwrap().calls.len() };
        let ev_before = events.count("interlock.triggered");
        ctl.sync_failsafe_latch().await;
        assert_eq!(
            inner.lock().unwrap().calls.len(),
            before,
            "sync 幂等：不应重复 restore(true)"
        );
        assert_eq!(
            events.count("interlock.triggered"),
            ev_before,
            "sync 幂等：不应重复 DB 事件"
        );
    }

    /// **A-1（独立评审，安全相关）：`do_request_release` 的 TOCTOU —— 清态前的同锁内复检。**
    ///
    /// # 造法（**确定性**，用既有的 `FakePortInner.gate`：`Notify` 把 `restore_latched` 卡住）
    ///
    /// 1. 触发帧 ⇒ `latched`；源复位帧 ⇒ `source_safe_since` 起计、状态机 `prev` 归零；
    /// 2. 装闸 ⇒ `request_release` 停在**步骤 3** `restore_latched(false)` 的 await 点
    ///    （此刻步骤 1 的"源已复位"检查**已经通过**）；
    /// 3. **窗口内**：物理急停重新按下 + runner 跑一帧 `tick_frame` —— 它**不占** `op_in_flight`
    ///    闸（那把闸只互斥 release×ack_m1 两条写路径），故与写路径天然并发；
    /// 4. 放闸 ⇒ release 走完。
    ///
    /// # 修前 / 修后（复现探针实测，探针已转为下面的常驻断言、不留临时件）
    ///
    /// - 修前：`release Ok=true / 锁存被清=true / 而 pin=true` —— 并在 DB 留一条
    ///   「人工释放联锁（触发源已复位且保持期满）」的**假审计**；屏上显「已释放」，而急停
    ///   物理上仍按着。窗口内那帧 `transport 动作=[]`（latch 尚未被清 ⇒ tick 走"仍有源
    ///   active"分支、无动作）正是它隐蔽的原因。下一帧会自愈（步骤 5 重置状态机边沿 ⇒
    ///   重新 TriggerLatch ⇒ 再写 `restore_latched(true)`），但那一拍 `is_latched_now()`
    ///   读到 `false`，1 Hz 轮询可能**放行一拍下发**。
    /// - 修后：同锁内复检发现源已重新触发 ⇒ 回 `SourcesNotReset{remaining:["estop"]}`、
    ///   **latch 不被清**、**无 cleared 审计**，并把 transport 拉回锁存态（防 `latched=true`
    ///   本机 / `latched=false` transport 的状态分裂）。
    ///
    /// **改什么会让本条变红**：摘掉步骤 4 的复检（或把它挪到写锁**外**、挪回步骤 1）。
    #[tokio::test]
    async fn release_rechecks_sources_inside_clear_critical_section() {
        let cfg = io_cfg(false, 1000); // auto_release=false、hold=0 ⇒ 源复位即可人工释放
        let port = FakePort::new();
        port.set_run_state(Some(2));
        port.inner().lock().unwrap().stop_ok = true;
        let events = Arc::new(FakeEventRepo::default());
        let (ctl, pin, _run, _fault, inner) = estop_board(cfg, port, events.clone());

        // 帧1：急停按下 → latch
        pin.set(true);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now());
        // 帧2：急停复位 → 状态机 prev 归零、source_safe_since 起计（步骤 1 的检查据此通过）
        pin.set(false);
        ctl.tick_frame().await;
        assert!(ctl.is_latched_now());
        assert!(ctl.state.read().unwrap().source_safe_since.is_some());

        // 闸装到 `restore_latched`：release 会停在 `restore_latched(false)` 的 await 点
        let gate = Arc::new(tokio::sync::Notify::new());
        {
            let mut g = inner.lock().unwrap();
            g.gate = Some(gate.clone());
            // ⚠️ `gated_in` 在触发帧的 `restore_latched(true)` 就已置位（它**无条件**置位）
            // ⇒ 必须清零，否则下面的等待循环会立刻返回、用例变成"抢跑"（写本用例时实测踩到）。
            g.gated_in = false;
        }

        let r = {
            let c = ctl.clone();
            tokio::spawn(async move { c.request_release().await })
        };
        for _ in 0..1000 {
            if inner.lock().unwrap().gated_in {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            inner.lock().unwrap().gated_in,
            "前提：release 已通过步骤 1 的源检查、停在 `restore_latched(false)` 的 await 点上"
        );

        // ── 窗口内：物理急停被**重新按下** + runner 在**相邻任务**里跑一帧 ──
        let calls_before = { inner.lock().unwrap().calls.len() };
        pin.set(true);
        ctl.tick_frame().await;
        assert!(
            ctl.is_latched_now(),
            "前提：窗口内 runner 帧不影响 latch（它本来就没被清）"
        );
        let window_calls = {
            let g = inner.lock().unwrap();
            g.calls[calls_before..].to_vec()
        };
        assert!(
            window_calls.is_empty(),
            "前提（也是缺陷隐蔽的原因）：窗口内那一帧 latch 仍为 true ⇒ tick 走「仍有源 active」\
             分支、**不**产生 transport 动作；正是「没有 transport 动作」让这次 release 看起来\
             平安无事。实际动作：{window_calls:?}"
        );

        // 放闸——但**不能只放一次**：本路径会**两次**进 `restore_latched`（先 `false`，被复检
        // 推翻后回写 `true`），单发 `notify_one` 会让第二次永远等下去（写本用例时实测挂死）。
        // 用一个"闸泵"：反复 `notify_one`（`Notify` 不累积许可，最多存 1 个），挨个放行。
        let pump = {
            let g = gate.clone();
            tokio::spawn(async move {
                loop {
                    g.notify_one();
                    tokio::task::yield_now().await;
                }
            })
        };
        let res = r.await.unwrap();

        // ① 如实拒绝：源在 await 窗口内被重新置起 ⇒ `SourcesNotReset`（不是 Ok）
        assert_eq!(
            res,
            Err(InterlockReject::SourcesNotReset {
                remaining: vec!["estop".to_string()]
            }),
            "清态前复检必须拦下：此刻 pin=true，回 Ok 就是谎报成功"
        );
        // ② 不谎报成功：latch 不得被清（这是本缺陷的**安全要害**：`is_latched_now()` 会放行一拍）
        assert!(
            ctl.is_latched_now(),
            "复检拦下 ⇒ 本机 latch 必须保持（修前这里被清 ⇒ 1 Hz 轮询可能放行一拍下发）"
        );
        assert!(
            !ctl.state.read().unwrap().stop_failed,
            "本用例未涉 stop_failed（停机写成功）"
        );
        // ③ 无假审计：不得记「已释放」
        assert_eq!(
            events.count("interlock.cleared"),
            0,
            "拒绝路径不得留「人工释放联锁（触发源已复位且保持期满）」的假痕，实际 {:?}",
            events.msgs("interlock.cleared")
        );
        // ④ 无状态分裂：transport 已收到本次 restore(false) ⇒ 必须被拉回锁存态
        let calls = { inner.lock().unwrap().calls.clone() };
        assert_eq!(
            calls.last().map(String::as_str),
            Some("restore_latched(true)"),
            "拒绝后必须以回写 `restore_latched(true)` 收尾（C-1 双 latch 不得分裂），实际 {calls:?}"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|c| c.starts_with("restore_latched(false)"))
                .count(),
            1,
            "只此一次释放写；实际 {calls:?}"
        );
        // ⑤ 闸随 `OpGuard` 释放（拒绝路径不得把控制器永久卡成 Busy）
        assert!(!ctl.op_in_flight(), "拒绝路径必须释放写闸");

        // ⑥ 对照：急停真正复位后，同一次操作**能**成功（修法没有把 release 变成恒 Err）
        pin.set(false);
        ctl.tick_frame().await;
        assert!(
            ctl.request_release().await.is_ok(),
            "源真复位后 release 必须恢复成功语义（闸泵仍在放行）"
        );
        assert!(!ctl.is_latched_now(), "成功路径语义不变：latch 被清");
        assert_eq!(events.count("interlock.cleared"), 1);
        pump.abort();
    }
}
