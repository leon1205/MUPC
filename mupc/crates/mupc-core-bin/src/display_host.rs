//! 本地显示终端数据提供层（mupcd 侧，12-本地显示终端 设计 §4.2/§5，core-bin 内模块）。
//!
//! 组件（命名对齐设计 §3.2）：
//! - [`DisplayDataProvider`]：采样 + 组帧 + 发布。
//!   - **主拍** `publish_ms`（默认 1 s，`display-proto` `DEFAULT_PUBLISH_MS`）：读**内存缓存**
//!     组一帧 [`DisplayFrame`]——SOC 取 AiIntegrator 裁决快照（§4.3 唯一裁决入口）、
//!     run_state/pcs_online/三相取 intercore（`read_three_phase`/`last_run_state`/
//!     `is_connected`），原子写入共享 `latest`（`Arc<Mutex<Option<DisplayFrame>>>`，§3.5）。
//!   - **慢拍四段**（§4.2，独立任务、互不阻塞、各自失败各自降级）：`device`（3 s）/
//!     `alarms`（0.5 s）/ `interlock`（0.5 s）写入 [`SlowCaches`]，`info` 启动时一次性；
//!     **内容变化**即 `Notify` 唤醒主拍提前组帧（合并窗口 `min_publish_interval_ms`）。
//!   - **帧路径零阻塞 I/O、零 DB 查询**（设计 §2.1 不变量 / D6）：`build_frame` 只读缓存，
//!     绝不 await 慢源——慢源抖动不拖累帧率，慢源全挂也只退化为「1 Hz 主拍 + 各段不可用」。
//! - [`LoopbackHttpPublisher`]：127.0.0.1 回环 HTTP 短轮询端点 `GET /v1/display/latest`
//!   （§3.1/§5 决策 A1），返回最新帧 JSON；未就绪返回 503（渲染端视同无新帧重试）。
//!
//! 「不造假值」总原则（§8）：所有数值展示仅当对应 [`FieldFlag`] == `Valid`；源不可得一律显式
//! 打标（`Offline`=PCS 离线/核间读失败 / `NotRead`=transport 不支持 / `RangeError`=量程越界），
//! 值置 `None`——不补 0、不沿用陈旧值冒充实时。SOC 双源皆失时冻结值仅在控制内部、不送上屏。
//!
//! 四段慢拍的**「不可用」与「无」语义分离**（EDGE-09 / IL-01.6 / EDGE-16）：
//! - `alarms.available == false` ⇒ 屏显「告警源不可用」，**不是**「无告警」；
//! - `interlock.available == false` ⇒ 屏显「联锁状态不可用」，**不是**「未联锁」
//!   （`latched` 此刻同为 `false`，但渲染端的判据只能是 `available`）；
//! - 各段字段 `None` / `LinkState::Unknown` ⇒ 屏显「未知」/「未提供」，不臆造。
//!
//! # 已知真源缺口（本单元**如实登记**，未臆造补齐）
//! - ~~`device.iec104`~~：**已补齐**（U-59 / L-5，2026-09-19）——`gateway` 新增
//!   `Iec104Server::link_state()` 聚合内部连接表，本层经
//!   [`map_iec104_link_state`] 1:1 映射（未装配服务器 ⇒「未配置」，不再恒 `Unknown`）。
//! - `info.serial`：无可靠真源 ⇒ 恒 `None`（「未提供」，EDGE-16 / 设计 §4.1 F8 行）。
//! - `info.mgmt_ipv4`：设计指定 `getifaddrs`；core-bin 无 `libc` 依赖（workspace 亦未声明
//!   `nix`）且本仓库安全清单要求「无新增 `unsafe` 块」⇒ 改用纯 std 的 UDP 选路求本机对外
//!   IPv4（语义与限度见 [`primary_ipv4`] 文档）。
//! - `interlock` 的「调用失败 → `available=false`」在现有 `InterlockApi::status()`
//!   （返回非 `Result`，无错误通道）下**不可达**；`available=false` 仍可达（首采未到 /
//!   未接线），三种「非已启用」语义的区分见 [`InterlockWiring`]。
//! - `alarms` 的「最近 10 条（倒序）」**只做到窗口内的最近 10 条**：存储侧
//!   `EventRepository` **没有**「最近 N 条」API（只有 `query_range(start, end)`，且
//!   `crates/storage/src/repository.rs:362-380` 的 SQL 内硬编码 `LIMIT 10000`），故本层取
//!   [`ALARM_LOOKBACK_MS`]（= **7 天**）窗口内的最近 `alarm_page_size` 条。
//!   **后果（如实登记）**：库静默满一周后，窗口内无行 ⇒ 告警区显**空**，而真实语义是
//!   「窗口取不到」而非「无告警」（`available` 仍为 `true` = 真源可用、真 0 条）。这正是
//!   「最近 10 条」的**有界化落地**，不是等价实现；根因在存储层缺少「最近 N 条」入口。
//! - `alarms.level`：设计/PRD 均**未定义** `event_type` → 级别的映射，本单元给出**显式、
//!   可审**的规则表（见 [`alarm_level_of`]），兜底 `Warn`（契约 `AlarmLevel` 无「未知」态）。

use mupc_data_processing::latest_values::{self, ChangeBatch};
use mupc_display_proto::peripherals::PointValue as FramePoint;
use mupc_display_proto::{
    AlarmItem, AlarmLevel, AlarmsSection, ControlSource, DeviceSection, DisplayConfig, DisplayFrame,
    DisplayRange, ExitGuardOutcome, Field, FieldFlag, InfoSection, InterlockSection, LinkState,
    PeriphRole, PeripheralBlock, PeripheralStation, PeripheralsSection, RunState, ServiceScope,
    SocSource, PROTO_VERSION,
};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::Notify;

/// 帧共享存储：provider 每 tick 原子更新；publisher 每请求 clone 返回。
/// 设计 §3.5：HTTP 路径不做任何 modbus 读（采集在专用 1s task，避免并发总线抖动）。
pub type SharedLatest = Arc<Mutex<Option<DisplayFrame>>>;

/// 最新帧端点路径（设计 §3.1，与 `display-proto` `DEFAULT_CHANNEL_URL` 尾部一致）。
pub const LATEST_PATH: &str = "/v1/display/latest";

/// 回环 publisher 单连接请求头读超时（O3：连接后不发数据的对端不得长期占用 task）。
pub const HEAD_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// F7 告警回溯窗口（ms，**7 天**）。
///
/// 契约与设计只说「最近 10 条（倒序）」，但现有存储侧入口 `EventRepository::query_range`
/// 强制时间区间，且 `events` 表只有 `(event_type, timestamp)` 复合索引、SQL 内硬编码
/// `LIMIT 10000`（`crates/storage/src/repository.rs:362-380`）——**没有**可取「最近 N 条」的
/// API（`latest_by_type` 只按**具体类型**取单条，不是「最近 N 条」）。为免每 0.5 s 把上万行
/// 事件物化成 `Vec`，本单元取「该窗口内最近 `alarm_page_size` 条」：窗口外的旧事件不上屏。
///
/// 这是对「最近 10 条」的**有界化落地**（**非**等价实现）⇒ 已在本模块文档头的
/// 「已知真源缺口」表中**逐条登记**（含「静默满一周后告警区显空」的后果），见本文件顶部
/// `//! - alarms 的「最近 10 条（倒序）」…` 一段。
pub const ALARM_LOOKBACK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// 慢拍段缓存：采样任务写、组帧任务读（设计 §4.2 的 `*_cache`）。
///
/// 三把 `std::sync::RwLock` 只做短临界区的整段替换/克隆——组帧路径**不持锁做 I/O**。
#[derive(Clone, Default)]
pub struct SlowCaches {
    device: Arc<RwLock<DeviceSection>>,
    alarms: Arc<RwLock<AlarmsSection>>,
    interlock: Arc<RwLock<InterlockSection>>,
    /// 外设段（慢拍 D，U-73 §15.1.1）。未接线时保持 `Default` ⇒ `available=false`
    /// （「外设数据不可用」，EDGE-22）——**绝不是**"空段正常"。
    peripherals: Arc<RwLock<PeripheralsSection>>,
}

/// 读缓存（毒化不 panic，同 `latest` 的 O2 口径）。
fn read_cache<T: Clone>(lock: &RwLock<T>) -> T {
    lock.read().unwrap_or_else(|e| e.into_inner()).clone()
}

// ── 「内容变化」判据（设计 §4.2「变更即组帧」；**排除 `ts_ms`**）──
//
// 段采集时刻每拍必变；若把 `ts_ms` 算作内容，慢拍将退化为**恒定 4 Hz** 唤醒组帧
// （发布率上界虽不破，但「合并突发」的语义已失效，且平白抬高 HMI 轮询负载）。
// 故只比较**展示内容**。以下三个函数与各段字段一一对应，字段增删由 `content_change_predicates_*`
// 用例逐字段钉死（漏字段即变红）。

/// 装置段内容变化（不含 `ts_ms`）。
fn device_changed(a: &DeviceSection, b: &DeviceSection) -> bool {
    a.uptime_secs != b.uptime_secs
        || a.cpu_temp_c != b.cpu_temp_c
        || a.mem_used_pct != b.mem_used_pct
        || a.iec104 != b.iec104
        || a.intercore != b.intercore
        || a.hmi_channel != b.hmi_channel
        || a.control_source != b.control_source
}

/// 告警段内容变化（不含 `ts_ms`）。
fn alarms_changed(a: &AlarmsSection, b: &AlarmsSection) -> bool {
    a.available != b.available || a.items != b.items
}

/// 联锁段内容变化（不含 `ts_ms`）。
fn interlock_changed(a: &InterlockSection, b: &InterlockSection) -> bool {
    a.available != b.available
        || a.enabled != b.enabled
        || a.latched != b.latched
        || a.stop_failed != b.stop_failed
        || a.sources != b.sources
        || a.fault_lamp != b.fault_lamp
        || a.run_lamp != b.run_lamp
        || a.release_hold_secs != b.release_hold_secs
}

// ═══════════════════════════════════════════════════════════════════════════
// 慢拍源（三路可注入 seam：生产实现 + 测试桩；设计 §4.2 表 A/B/C 行）
// ═══════════════════════════════════════════════════════════════════════════

/// F6 装置状态源（慢拍 3 s）。字段级不可得 ⇒ 各字段自带 `None` / `Unknown`，**不是**整段失败。
#[async_trait::async_trait]
pub trait DeviceSource: Send + Sync {
    /// 采一次装置状态（`ts_ms` 由实现填采集时刻）。
    async fn read_device(&self) -> DeviceSection;
}

/// F7 告警源（慢拍 0.5 s）。
///
/// `Err(原因)` ⇒ `alarms.available=false`（屏显「告警源不可用」），**不是**「无告警」（EDGE-09）。
#[async_trait::async_trait]
pub trait AlarmSource: Send + Sync {
    /// 取按时间**倒序**的最近若干条（实现自行截断到 `alarm_page_size`）。
    async fn read_alarms(&self) -> Result<Vec<AlarmItem>, String>;
}

/// F16 联锁状态源（慢拍 0.5 s）。
///
/// `Err(原因)` ⇒ `interlock.available=false`（屏显「联锁状态不可用」），**不是**「未联锁」
/// （IL-01.6：二者语义不同、不得互替）。
#[async_trait::async_trait]
pub trait InterlockSource: Send + Sync {
    /// 读一次联锁段（`ts_ms` 由实现填采集时刻）。
    async fn read_interlock(&self) -> Result<InterlockSection, String>;
}

/// 联锁段的接线形态（设计 §4.2 表 C 行的三种「非已启用」语义，**不得互替**）。
#[derive(Clone)]
pub enum InterlockWiring {
    /// `io.enabled=false`：联锁功能**未启用**——这是**已知状态**（不是"源坏了"也不是"未联锁"），
    /// 段为 `available=true, enabled=false` ⇒ 屏显「联锁功能未启用」（设计 §4.2 表 C 行）。
    Disabled,
    /// 未接线（本单元默认）：缓存保持 `Default` ⇒ `available=false` ⇒ 屏显「联锁状态不可用」。
    Unwired,
    /// 已接线（`io.enabled=true`）：按 `interlock_poll_ms` 采集。
    Wired(Arc<dyn InterlockSource>),
}

/// 由装配点参数决定联锁接线形态（**纯函数**，可单测；三分支语义见 [`InterlockWiring`]）。
///
/// ⚠️ **三分支互斥且不可互替**——尤其 `(None, true)`：
/// - `(Some(api), true)` ⇒ [`InterlockWiring::Wired`]（正常接线，按 `interlock_poll_ms` 采集）；
/// - `(None, true)` ⇒ [`InterlockWiring::Unwired`]（`io.enabled=true` 却拿不到控制器 ⇒ 屏显
///   「**联锁状态不可用**」）。**不得**归到 `Disabled`——那会把"该有却没有"（装配失败 / 被跳过）
///   **谎报**成"本来就没开"（`Disabled` 的屏文「联锁功能未启用」是一条**正面事实**）；
/// - `io.enabled=false` ⇒ [`InterlockWiring::Disabled`]（**已知状态**「功能未启用」，
///   `available=true / enabled=false`），既不是「不可用」也不是「未联锁」。
///
/// 入参 `api` 的类型是**契约** `mupc_display_proto::InterlockApi`（单元 K：原为 web-api 的旧
/// trait，随 crate 删除迁移到契约——两份签名合一，装配点与读通道现在吃同一个 `Arc<dyn>`）。
pub fn interlock_wiring_for(
    api: Option<Arc<dyn mupc_display_proto::InterlockApi>>,
    io_enabled: bool,
    release_hold_secs: u64,
) -> InterlockWiring {
    match (api, io_enabled) {
        (Some(api), true) => InterlockWiring::Wired(Arc::new(InterlockApiSource::new(
            api,
            release_hold_secs,
        ))),
        (None, true) => InterlockWiring::Unwired,
        (_, false) => InterlockWiring::Disabled,
    }
}

/// 生产装置状态源：进程 uptime + intercore 链路 + 控制源 + 本机温度/内存。
pub struct SystemDeviceSource {
    /// uptime 零点——**由调用方注入**，取 `main()` 进程入口最顶部的 `Instant::now()`。
    ///
    /// 设计 §4.1 明写 uptime「以 **`mupcd` 进程启动时刻**为准」。若在本结构体的**构造时刻**
    /// 取零点，则零点落在 `initialize_all` 完成 DB / intercore / gateway / AI / security 全部
    /// 装配**之后**（约数百 ms~数 s）⇒ 屏上 uptime **系统性偏小**。故零点上移到进程入口，
    /// 由调用方传入；构造签名保留**显式零点形参**，测试可注入人工零点，不依赖真实进程起点。
    started_at: Instant,
    intercore: Arc<mupc_intercore::IntercoreClient>,
    ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    /// IEC 104 服务器句柄（设计 §4.1 #1）：`link_state()` 即 F6「IEC 104 连接状态」真源。
    ///
    /// `None` = **本进程未装配该服务器**（如 `display.enabled` 而网关未起）⇒ 该字段报
    /// [`LinkState::NotConfigured`]（「未配置」，F6.5 语义），**不**报 `Unknown` 也不臆造。
    iec104: Option<Arc<mupc_gateway::iec104::server::Iec104Server>>,
}

impl SystemDeviceSource {
    /// `started_at` = 进程启动零点（生产取 `main()` 最顶部的 `Instant::now()`，见字段注释）。
    pub fn new(
        intercore: Arc<mupc_intercore::IntercoreClient>,
        ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
        iec104: Option<Arc<mupc_gateway::iec104::server::Iec104Server>>,
        started_at: Instant,
    ) -> Self {
        Self {
            started_at,
            intercore,
            ai_integrator,
            iec104,
        }
    }

    /// IEC 104 链路状态（U-59 / L-5）：装配了服务器就问它，未装配即「未配置」。
    async fn iec104_link_state(&self) -> LinkState {
        match &self.iec104 {
            Some(server) => map_iec104_link_state(server.link_state().await),
            None => LinkState::NotConfigured,
        }
    }
}

/// gateway 聚合态 → 显示契约态（**1:1、无损**；两侧枚举一一对应，见各自的文档）。
///
/// 为什么不让 gateway 直接返回 `display_proto::LinkState`：`gateway` 是协议侧 crate，
/// 不应反向依赖 HMI 的显示契约（设计 §4.1 #1 亦明确"改动局限在 server.rs"）。
fn map_iec104_link_state(s: mupc_gateway::iec104::server::LinkState) -> LinkState {
    use mupc_gateway::iec104::server::LinkState as Gw;
    match s {
        Gw::Connected => LinkState::Connected,
        Gw::Connecting => LinkState::Connecting,
        Gw::Disconnected => LinkState::Disconnected,
        Gw::NotConfigured => LinkState::NotConfigured,
    }
}

#[async_trait::async_trait]
impl DeviceSource for SystemDeviceSource {
    async fn read_device(&self) -> DeviceSection {
        // 核间链路：有心跳连接即 Connected，否则 Disconnected（**不**写 Unknown——本项真源存在）
        let intercore = if self.intercore.is_connected().await {
            LinkState::Connected
        } else {
            LinkState::Disconnected
        };
        DeviceSection {
            ts_ms: now_ms(),
            uptime_secs: Some(self.started_at.elapsed().as_secs()),
            cpu_temp_c: read_cpu_temp_c(),
            mem_used_pct: read_mem_used_pct().await,
            // 真源已补齐（U-59 / L-5，2026-09-19）：`Iec104Server::link_state()` 聚合内部
            // 连接表 ⇒ 未装配服务器报「未配置」，其余四态由服务器给出（不再恒 `Unknown`）。
            iec104: self.iec104_link_state().await,
            intercore,
            // 契约硬要求：本通道状态由 **HMI 本地覆盖**（设计 §5.5）；服务端**必须**给 Unknown，
            // 否则就是"由服务端报告客户端自己的连接状态"这一语义倒置。
            hmi_channel: LinkState::Unknown,
            control_source: self.control_source().await,
        }
    }
}

impl SystemDeviceSource {
    /// 当前控制源（设计 §4.1「F6 当前控制源」行）：
    /// 本地优先已置位 ⇒ [`ControlSource::LocalStrategy`]（AI 停用期的生产唯一态）；
    /// 否则 AI 引擎未加载 ⇒ [`ControlSource::AiDisabled`]（固定文案）；两者皆不成立 ⇒ `Unknown`
    /// （**不臆造**下发源）。
    async fn control_source(&self) -> ControlSource {
        if self.ai_integrator.is_local_priority().await {
            ControlSource::LocalStrategy
        } else if !self.ai_integrator.engine_status().await.ai_engine_enabled {
            ControlSource::AiDisabled
        } else {
            ControlSource::Unknown
        }
    }
}

/// CPU 温度（℃）——**仅 Linux 真读**。
///
/// 直读 `/sys/class/thermal/thermal_zone0/temp`（与 `mupc_system_monitor` 的 `read_cpu_temp`
/// 同源），**不**经 `TemperatureCollector`：后者在 Linux 读失败时回退 **45.0**、在非 Linux
/// 目标返回硬编码常量（`crates/system-monitor/src/collectors.rs:279-300`），采信它即把假值送
/// 上屏。读不到 ⇒ `None` ⇒ 屏显「未知」（F6.5：不可得不得显为正常）。
fn read_cpu_temp_c() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()?;
        let milli_c: f64 = raw.trim().parse().ok()?;
        let c = milli_c / 1000.0;
        if c.is_finite() && (-50.0..=200.0).contains(&c) {
            Some(c)
        } else {
            None
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // 非 Linux 目标 system-monitor 只有硬编码桩值 ⇒ 宁可「未知」也不上假值
        None
    }
}

/// 内存使用率（%）——**仅 Linux 真读**（`/proc/meminfo`，经 `MemoryCollector`）。
/// 非 Linux 目标的 `MemoryCollector` 返回硬编码 8192/4096/50%
/// （`crates/system-monitor/src/collectors.rs:186-196`）⇒ 本单元一律取 `None`。
async fn read_mem_used_pct() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        use mupc_system_monitor::MetricCollector;
        let snap = mupc_system_monitor::MemoryCollector::new(0).collect().await.ok()?;
        let pct = snap.memory.usage_percent;
        if pct.is_finite() && (0.0..=100.0).contains(&pct) {
            // `usage_percent` 是 `f32`（system-monitor 侧口径），本函数对外口径是 `f64`
            // （`DeviceSection.mem_used_pct`）—— f32→f64 为无损加宽，直接提升。
            Some(f64::from(pct))
        } else {
            None
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// 事件类型 → 告警级别（设计未定义映射，本单元给出**显式、可审**的规则表）。
///
/// 规则（按序匹配 `event_type`，大小写不敏感）：
/// 1. 含 `triggered` / `offline` / `failed` / `error` / `fault` / `trip` → [`AlarmLevel::Error`]；
/// 2. 含 `cleared` / `online` / `restored` / `stopped` / `recovered` / `ack` → [`AlarmLevel::Info`]；
/// 3. 其余 → [`AlarmLevel::Warn`]。
///
/// 兜底取 `Warn`（不取 `Info`）：未知事件既不静默降级为提示，也不冒称错误。
/// 契约 `AlarmLevel` 无「未知」态、设计亦未给规则 ⇒ 已登记为缺口（见本文件顶部模块文档
/// 「已知真源缺口」表的 `alarms.level` 条）。
fn alarm_level_of(event_type: &str) -> AlarmLevel {
    let t = event_type.to_ascii_lowercase();
    const ERROR: [&str; 6] = ["triggered", "offline", "failed", "error", "fault", "trip"];
    const INFO: [&str; 6] = ["cleared", "online", "restored", "stopped", "recovered", "ack"];
    if ERROR.iter().any(|k| t.contains(k)) {
        AlarmLevel::Error
    } else if INFO.iter().any(|k| t.contains(k)) {
        AlarmLevel::Info
    } else {
        AlarmLevel::Warn
    }
}

/// 超长告警消息的**可见截断标记**（ASCII `...`，**不是** `…` U+2026）。
///
/// 依据：`…`（U+2026）**实测不在** `crates/local-display/fonts/font_subset_charset.txt`
/// 与生成字体的 cmap 内（屏上是豆腐块），本仓既有处置一律取 ASCII 三点——见
/// `ui/pages/p5_audit.rs::TEXT_ELLIPSIS`（「`…` 不在 cmap 内 ⇒ 取 ASCII `.`」）与
/// `ui/pages/p3_logs.rs` LG1 行同款处置。**不造新上屏字**（§3.6 全屏用字表内的 `.`）。
const ALARM_TRUNCATION_MARK: &str = "...";

/// 单条告警消息按 [`MAX_ALARM_MESSAGE_BYTES`] **字符边界安全**地截断，并加**可见**截断标记。
///
/// 为何在 **ingest 侧**截断（而非靠发布侧的编码守卫兜）：契约 §3.5 条 3 的编码守卫一触发就是
/// **整帧**编码失败。而 HMI 侧（`crates/local-display/src/channel.rs:450`）走的是**裸
/// `serde_json::from_slice`**、**不经**契约的严格解码器，只受**传输层 64 KiB**
/// （`channel.rs:78/:675`）限制——即一帧里含一条 2 KiB 消息时 HMI **本可正常显示**。
/// 若只留编码守卫，**局部**（一条消息）超限会升级成**整帧 500 / 画面冻在旧帧**，把局部超限
/// 放大为**整屏不可用**（与 EDGE-09 / F7 的取向相反）。故在本层把消息截到上限内，使帧
/// **恒可编码**；发布侧的 `to_json_slice` 守卫**保留但退化为纯兜底**（防 JSON 转义膨胀等
/// 本层管不到的膨胀源）。
///
/// 截断**可见、不静默**：尾部加 [`ALARM_TRUNCATION_MARK`]（屏上显示为 `...` 收尾）。
fn truncate_alarm_message(msg: &str) -> String {
    if msg.len() <= mupc_display_proto::MAX_ALARM_MESSAGE_BYTES {
        return msg.to_string();
    }
    // 为标记留出字节预算；从预算处**向左回退到最近的字符边界**（不切断多字节 UTF-8）
    let budget = mupc_display_proto::MAX_ALARM_MESSAGE_BYTES - ALARM_TRUNCATION_MARK.len();
    let mut end = budget;
    while end > 0 && !msg.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(mupc_display_proto::MAX_ALARM_MESSAGE_BYTES);
    out.push_str(&msg[..end]);
    out.push_str(ALARM_TRUNCATION_MARK);
    out
}

/// `SystemEvent` → [`AlarmItem`]（`ts_ms` = 事件时间，非采集时间）。
///
/// `message` 超 [`MAX_ALARM_MESSAGE_BYTES`] ⇒ 按字符边界截断 + 可见标记（见
/// [`truncate_alarm_message`]）。
fn alarm_item_of(ev: &mupc_storage::SystemEvent) -> AlarmItem {
    AlarmItem {
        ts_ms: ev.timestamp.timestamp_millis().max(0) as u64,
        level: alarm_level_of(&ev.event_type),
        message: truncate_alarm_message(&ev.message),
    }
}

/// 生产告警源：`storage.events` 最近若干条（设计 §4.1 #3 裁决 D11：以 `SystemEvent` 为 F7 唯一真源）。
pub struct StorageAlarmSource {
    events: Arc<dyn mupc_storage::EventRepository>,
    page_size: usize,
}

impl StorageAlarmSource {
    /// `page_size` = `display.alarm_page_size`（设计 §4.9）。
    pub fn new(events: Arc<dyn mupc_storage::EventRepository>, page_size: usize) -> Self {
        Self { events, page_size }
    }
}

#[async_trait::async_trait]
impl AlarmSource for StorageAlarmSource {
    async fn read_alarms(&self) -> Result<Vec<AlarmItem>, String> {
        if self.page_size == 0 {
            return Ok(Vec::new()); // 配置层应拒（validate），此处仅保证不 panic
        }
        let end = chrono::Utc::now();
        let start = end - chrono::Duration::milliseconds(ALARM_LOOKBACK_MS);
        let rows = self
            .events
            .query_range(start, end)
            .await
            .map_err(|e| e.to_string())?;
        // 存储实现已 `ORDER BY timestamp DESC`，但该顺序**非 trait 契约**（仅一种实现）⇒
        // 本层显式排序，避免换实现后静默取到最旧的 N 条（时间倒序是契约硬要求）。
        let mut refs: Vec<&mupc_storage::SystemEvent> = rows.iter().collect();
        refs.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then(b.id.unwrap_or(0).cmp(&a.id.unwrap_or(0)))
        });
        Ok(refs
            .into_iter()
            .take(self.page_size)
            .map(alarm_item_of)
            .collect())
    }
}

/// 生产联锁源：`InterlockController`（以**契约** `mupc_display_proto::InterlockApi` 擦除注入，
/// 设计 §4.2 表 C 行；单元 K：入参类型由 web-api 旧 trait 换成契约）。
pub struct InterlockApiSource {
    api: Arc<dyn mupc_display_proto::InterlockApi>,
    /// `io.release_hold_secs`（UI 提示「须保持 N 秒」，契约 `InterlockSection::release_hold_secs`）。
    release_hold_secs: u64,
}

impl InterlockApiSource {
    pub fn new(
        api: Arc<dyn mupc_display_proto::InterlockApi>,
        release_hold_secs: u64,
    ) -> Self {
        Self {
            api,
            release_hold_secs,
        }
    }
}

#[async_trait::async_trait]
impl InterlockSource for InterlockApiSource {
    async fn read_interlock(&self) -> Result<InterlockSection, String> {
        // 契约 `InterlockApi::status()` 的返回类型**就是**帧内联锁段的同一类型（`InterlockView`
        // = `InterlockSection`，单一真源）⇒ 不再需要旧 DTO → 契约的字段搬运。字段就是 `Arc<dyn
        // mupc_display_proto::InterlockApi>`，`status()` 经 dyn 直接可调，无需再 `use` trait。
        let view = self.api.status().await;
        Ok(interlock_section_of(&view, self.release_hold_secs, now_ms()))
    }
}

/// 契约 `InterlockView` → 帧内 [`InterlockSection`]：**只补两处本层才知道的事**。
///
/// 契约视图已是段本体，本函数**不做字段搬运**（没有第二套字段可搬），只改写：
/// - `ts_ms` = **本层的取数时刻**（`now_ms()`）。控制器自己的 `ts_ms` 是"控制器算完那一刻"，
///   而帧的语义是"**采集时刻**"（设计 §3.1）⇒ 以本层为准（两者仅差微秒级，但语义不可混）；
/// - `release_hold_secs` = **注入值**（装配点取的 `io.release_hold_secs`，与控制器 `cfg` 同源）。
///
/// 其余字段**逐字透传**，包括：
/// - `available` / `enabled`：契约字段如实带走（**不再硬编码 `true`**）——`available=false`
///   必须能被屏读成「联锁状态不可用」，不得被本层吞成"可用"（IL-01.6）；
/// - `fault_lamp` / `run_lamp`：`Option<bool>` **如实透传**，`None` = 「灯未知」。
///   ⚠️ 迁出前的 web-api 适配器把这两个槽 `unwrap_or(false)`（旧 DTO 根本没有"未知"槽），
///   单元 K 迁移时按该适配器留的交接说明**如实透传**，不再吞 `None`。语义限度不变：它是
///   **目标电平**而非 DO 回读（DO 写失败时实际灯态可能不同，控制器只在日志记错）。
fn interlock_section_of(
    view: &mupc_display_proto::interlock::InterlockView,
    release_hold_secs: u64,
    ts_ms: u64,
) -> InterlockSection {
    InterlockSection {
        release_hold_secs,
        ts_ms,
        ..view.clone()
    }
}

/// 编译时间戳（RFC3339，UTC）：读 `build.rs` 注入的 `BUILD_TIMESTAMP_EPOCH`
/// （设计 §4.8；`SOURCE_DATE_EPOCH` 优先 ⇒ 可复现）。
///
/// **语义口径（如实写清）**：该值是 **`build.rs` 上次运行的时刻**，**不是**「源码最后修改时刻」
/// ——`build.rs` 声明了 `rerun-if-changed=build.rs`（护增量缓存），故**改其他源文件不会重跑它**
/// ⇒ 屏上「编译时间」会**静默变旧**、且无提示。契约只要求「编译时间」，本实现满足该口径；
/// 要「源码最后修改时刻」须换真源（本单元不改）。
///
/// 变量缺失或越界 ⇒ `None` ⇒ 屏显「未提供」（EDGE-16）。**不**用 `env!`：那会让「未注入」
/// 直接编译失败，与契约的 `Option<String>` 降级语义不符。构建期若**时钟早于 epoch**，
/// `build.rs` **不注入**（不补 0 伪装成 1970-01-01）⇒ 此处自然得 `None`。
fn build_time_rfc3339() -> Option<String> {
    build_time_from_raw(option_env!("BUILD_TIMESTAMP_EPOCH"))
}

/// [`build_time_rfc3339`] 的**纯函数内核**（可单测，不依赖编译期环境变量）：原始字符串 →
/// RFC3339。任何「不可得」形态一律 `None`：
/// - 变量缺失（`None`）/ 非数字 / 空白 / 越出 `chrono` 可表示范围 ⇒ `None`。
///
/// ⚠️ **不得**把末端写成 `.unwrap_or(0)`（本单元整改前 `build.rs` 的形态）：那会把「取不到
/// 时间」**冒充**成一个**合法**值 1970-01-01，屏上显成「有值」而实际是假的——与项目
/// 「显 `--`、**严禁补 0**」口径相反（同 `p5_audit::side_text` 的 C1 级前车之鉴）。
/// `.parse().ok()?` + `from_timestamp(..)`（`Option`）即「不可得 ⇒ `None`」的正确链路；
/// 该口径由 `build_time_none_when_epoch_absent_or_malformed` 逐形态钉死。
fn build_time_from_raw(raw: Option<&str>) -> Option<String> {
    let secs: i64 = raw?.trim().parse().ok()?;
    chrono::DateTime::from_timestamp(secs, 0).map(|dt| dt.to_rfc3339())
}

/// 装置型号：`/proc/device-tree/model`（设计 §6.6）。
///
/// device-tree 的 `model` 是 **NUL 结尾**的字节串，须裁尾零。非 Linux / 无设备树 / 空串 ⇒
/// `None` ⇒ 屏显「未提供」（EDGE-16，不臆造）。
fn read_device_model() -> Option<String> {
    let raw = std::fs::read("/proc/device-tree/model").ok()?;
    let s = String::from_utf8_lossy(&raw)
        .trim_end_matches('\0')
        .trim()
        .to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 设备管理 IP：本机用于访问**外部网络**的 IPv4（设计 §6.6 / PM 裁定 U-1）。
///
/// 实现偏离设计指定的 `getifaddrs`：core-bin 无 `libc` 依赖（workspace `Cargo.toml` 亦未声明
/// `nix`），且本仓库安全清单要求「无新增 `unsafe` 块」。改用纯 std 的**选路探测**：向
/// RFC 5737 TEST-NET-1 地址 `connect` 一个 UDP 套接字（UDP `connect` **不发包**，只让内核按
/// 路由表选本端地址），读回 `local_addr()`。
///
/// 语义限度（如实登记）：这是「内核认为的本机对外地址」，**不保证**等于「首个 UP 的非回环
/// IPv4」（多网卡时未必是管理网那张卡）；无默认路由 / 无可用网卡 ⇒ `None` ⇒ 屏显「未提供」，
/// **不臆造** IP。与 `service_scope`（服务只监听回环）是**两个不同概念**，UI 必须分列（EDGE-24）。
fn primary_ipv4() -> Option<std::net::Ipv4Addr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    match sock.local_addr().ok()? {
        std::net::SocketAddr::V4(a) if !a.ip().is_loopback() => Some(*a.ip()),
        _ => None,
    }
}

/// P6 装置信息段（设计 §6.6，「来源分列」）——**启动时一次性采集**，不随帧刷新。
///
/// 装置类字段取真源（`env!` / build.rs / device-tree / 选路），本地屏专属项取编译期常量；
/// 取不到一律 `None`（「未提供」）。**序列号无可靠真源 ⇒ 恒 `None`**（EDGE-16，不臆造）。
pub fn collect_info() -> InfoSection {
    InfoSection {
        firmware_version: env!("CARGO_PKG_VERSION").to_string(),
        // 编译时间戳（`crates/mupc-core-bin/build.rs` 发出）；取不到 ⇒ None ⇒「未提供」
        build_time: build_time_rfc3339(),
        model: read_device_model(),
        serial: None,
        service_scope: ServiceScope::LoopbackOnly,
        mgmt_ipv4: primary_ipv4().map(|ip| ip.to_string()),
    }
}

/// 当前 Unix 毫秒（各段 `ts_ms` 用）。
fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

// ═══════════════════════════════════════════════════════════════════════════
// 外设段（慢拍 D）—— U-73 设计 §15.1.1 / §15.6.1
//
// **消费形态**（§15.1.1 D 方案，与既有慢拍 A/B/C 逐条同构）：
//   `latest_values` 变更广播（`broadcast` 容量 64、**会丢**） ∪ 兜底 tick（`periph_poll_ms`）
//   ⇒ 触发源只影响"多久跑一次"，**不影响正确性**：任一次采样都是
//   「逐站读全量 → 重建整段」的**无状态**操作 ⇒ 丢一次广播 = 晚 ≤`periph_poll_ms` 收敛。
//
// **边界（硬）**：① 取数一律走 `latest_values` 的**公开只读面**（`station_snapshot` /
//   `station_is_active` / `station_last_poll_ms`），**禁轮询 DB / telemetry 表**（RQ-9.0-1）、
//   **禁自建第二真源**（"最后成功时刻"只有一个来源：R-38 的 getter）；② 帧路径零 I/O
//   （组帧只 `read_cache` 克隆，§2.1 / D6 不变量不破）。
// ═══════════════════════════════════════════════════════════════════════════

/// 外设段源（**可注入 seam**，与 [`DeviceSource`] / [`AlarmSource`] 同范式）。
///
/// 实现方**必须**满足：`snapshot` 是「读全量 → 重建整段」的纯内存操作（无 I/O、不阻塞、
/// 不返回 `Result`——不可得在段内以逐点 `flag` 表达）。
pub trait PeripheralSource: Send + Sync {
    /// 重建一次外设段（`now_ms` 由调用方给，便于假时钟单测）。
    fn snapshot(&self, now_ms: u64) -> PeripheralsSection;

    /// 订阅 `latest_values` 变更广播（容量 64；**落后即丢**，`Lagged` 只意味着"提前量没了"）。
    /// `None` = 该源无变更通知 ⇒ 只靠兜底 tick（§15.1.1 的"丢失由兜底 tick 收敛"在此退化为常态）。
    fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<ChangeBatch>>;
}

/// 外设段的白名单计划：**站 → 块 → 白名单点**（由站配置 + `display-proto` 白名单投影而来）。
///
/// **为什么要有这一层**：取数与组帧必须**只携带白名单内的点**（§15.5.2 W-1：屏侧行数与
/// catalog 行数恒等，`flag != Valid` 只改值不改行数）。计划在**装配期**由配置一次性算出
/// （纯函数、可单测），运行期只做内存读。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheralStationPlan {
    /// 南向站 id。
    pub id: String,
    /// 显示契约 role。
    pub role: PeriphRole,
    /// 块计划（顺序 = 站配置 `regs` 顺序）。
    pub blocks: Vec<PeripheralBlockPlan>,
}

/// 单个块的计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheralBlockPlan {
    /// 块名（点名前缀）。
    pub name: String,
    /// 白名单内的 `at` 列表（升序；`fire_det` 已按配置 `count` 展开）。
    pub ats: Vec<u16>,
    /// 显式点名覆盖 `(at, name)`（与帧内 `renames` 同源 = 站配置 `PointConf.name`）。
    pub renames: Vec<(u16, String)>,
    /// 是否位块（`discrete`）——**位点跳过"本轮未更新"判据**（§15.1.2）。
    pub is_bit: bool,
}

/// 南向 role → 显示契约 role（设计 §15.2.2 注：两侧 serde 名逐字对应）。
///
/// `MeterGrid`（台区关口总表）**不在本增量内**（§15 范围外 #3）⇒ 映射为
/// [`PeriphRole::Unknown`]，其块**不进计划**（不上屏，也不进 catalog）。
pub fn periph_role_of(role: mupc_southd::config::Role) -> PeriphRole {
    use mupc_southd::config::Role as R;
    match role {
        R::Hvac => PeriphRole::Hvac,
        R::Fire => PeriphRole::Fire,
        R::Battery => PeriphRole::Battery,
        R::MeterBatt => PeriphRole::MeterBatt,
        R::Pcs => PeriphRole::Pcs,
        R::MeterGrid => PeriphRole::Unknown,
    }
}

/// `fire_det` 块名（与 `display-proto` 的守卫块名**同源同值**，不得另写一份字面量）。
const FIRE_DET_BLOCK: &str = mupc_display_proto::peripherals::FIRE_DET_BLOCK_NAME;

/// 由站配置投影出**白名单计划**（纯函数，可单测；装配期一次性调用）。
///
/// 规则（逐条）：
/// - 站 role 映射不过（`meter_grid`）⇒ **整站跳过**（§15 范围外 #3）；
/// - 每块的 `at` 取自 `display-proto` 的 [`mupc_display_proto::PERIPH_WHITELIST`]（W-1 单一真源）；
/// - `fire_det`：白名单里只有**模板 6 行**，按配置 `count`（= 寄存器数 = 点数）展开成
///   `at ∈ 1..=count`（`count = 6×(n−1)`，n = 探测器只数 + 1）——**与 `point_table` 的
///   `FIRE_DET_TEMPLATE_START(17)` / `FIRE_DET_STRIDE(6)` 同源**（位置式点名 `at = addr − 16`）；
/// - 配置里没有的块（如现场未配 `bms_alarm`）⇒ **不进计划**（⇒ 站内该块缺席，屏侧按 catalog 行数
///   与帧内块数比对即可发现——"BMS 告警源不可用"的判据在 catalog/屏侧，不在本层臆造）；
/// - 白名单有、而**该块在配置内存在但某 `at` 未配**（如 `point_table` 未登记）⇒ 仍带该点，
///   值走 `NotRead`（§15.1.2 第 2 条：点缺 ⇒ `NotRead`，不补 0）。
pub fn peripheral_plan(cfg: &mupc_southd::config::SouthStationsConfig) -> Vec<PeripheralStationPlan> {
    let mut out = Vec::new();
    for st in &cfg.stations {
        let role = periph_role_of(st.role);
        if role == PeriphRole::Unknown {
            continue; // meter_grid：不在本增量内（§15 范围外 #3）
        }
        let mut blocks = Vec::new();
        for blk in &st.regs {
            // 白名单内本块的 at（模板行对 fire_det 只给 1..=6，下面按 count 展开）
            let mut ats: Vec<u16> = mupc_display_proto::PERIPH_WHITELIST
                .iter()
                .filter(|(r, b, _)| *r == role && *b == blk.name)
                .map(|(_, _, at)| *at)
                .collect();
            if ats.is_empty() {
                continue; // 该块整体不在白名单（如 pcs_3zone 之外的 PCS 块）
            }
            if blk.name == FIRE_DET_BLOCK {
                // 运行期展开：整块逐寄存器 1 点（`count` = 点数 = 6 × 只数）
                ats = (1..=blk.count.max(1)).collect();
            } else {
                ats.sort_unstable();
                ats.dedup();
            }
            let renames: Vec<(u16, String)> = blk
                .points
                .iter()
                .filter_map(|pt| pt.name.clone().map(|n| (pt.at, n)))
                .collect();
            blocks.push(PeripheralBlockPlan {
                name: blk.name.clone(),
                ats,
                renames,
                is_bit: matches!(blk.func, mupc_southd::config::RegFunc::Discrete),
            });
        }
        if blocks.is_empty() {
            continue; // 该站没有任何白名单块（现场未启用）⇒ 整站不进计划（catalog 会显「未启用」）
        }
        out.push(PeripheralStationPlan {
            id: st.id.clone(),
            role,
            blocks,
        });
    }
    out
}

/// **单点的 `flag` / `v` 判定**（设计 §15.1.2 伪码逐行；**不引入任何时间阈值**——只比"同一轮"）。
///
/// 返回 `(v, flag)`，**恒满足 `flag != Valid ⇒ v == None`**（不补 0、不沿用旧值）。
fn point_field(
    is_bit: bool,
    station_active: bool,
    last_poll_ms: Option<u64>,
    metric: &std::collections::HashMap<String, latest_values::PointValue>,
    key: &str,
) -> (Option<f64>, FieldFlag) {
    if !station_active {
        return (None, FieldFlag::Offline); // ① 站离线 ⇒ 该站全部点 Offline
    }
    let Some(pv) = metric.get(key) else {
        return (None, FieldFlag::NotRead); // ② 点缺（`station_snapshot` 无此 metric 键）
    };
    // ③ 标量点「本轮该点未更新」：`point.ts_ms < 站最后成功时刻`（含越界被 mapper 滤除的点）。
    //    位点跳过本判据（位点 `ts_ms` = 最后变化时刻，天然旧，§9.1.2/§9.1.3）。
    //    ⚠️ `station_last_poll_ms` 未落地（`None`）时该判据**不可判** ⇒ 退化（R-30：可能把陈旧值
    //    当实时值展示），**不得**用 `now − ts > X` 之类的时间阈值去补（那是第二套新鲜度判据）。
    if !is_bit {
        if let Some(lp) = last_poll_ms {
            if pv.ts_ms < lp {
                return (None, FieldFlag::NotRead);
            }
        }
    }
    if pv.quality != latest_values::PointQuality::Ok {
        return (None, FieldFlag::NotRead); // ④ 01 已判过陈旧 / 未配置 ⇒ 本节不重判
    }
    match pv.value {
        None => (None, FieldFlag::NotRead), // 不可得（严禁以 0 顶替）
        Some(v) if v.is_finite() => (Some(v), FieldFlag::Valid),
        Some(_) => (None, FieldFlag::RangeError), // ⑤ 非有限（NaN / ±Inf）
    }
}

/// 消防钢瓶气压「是否配置」的**取数接缝**（设计 §15.2.2 的 `cylinder_configured`；
/// 适配器落点 = §15.11 #7 的 `startup.rs`，本 trait 只定契约）。
///
/// 三态语义（**逐字锁定，不得改**——§15.2.2 明文）：
/// - `None`        = **不可得**（非消防站 / 接缝未接线）⇒ 屏侧**按值正常展示**；
/// - `Some(false)` = **未配置** ⇒ 屏显「**未配置**」（**忽略 `v`**、断言不含 `0 kPa`、不产告警）；
/// - `Some(true)`  = 已配置 ⇒ 屏侧按值正常展示（此后恒 0 也按真实 0 展示）。
///
/// **实现侧不得臆造**：查不到权威结论时必须回 `None`（"不可得"），**不得**用"值为 0"之类的
/// 启发式去猜 `Some(false)`——那会把「接缝未接线」伪装成「未配置」（EDGE-23 / EX-11）。
pub trait CylinderPressureQuery: Send + Sync {
    /// 该站的钢瓶气压配置态；`station_id` = 南向站 id（`south_stations.stations[].id`）。
    ///
    /// **实现方义务**：`role != Fire` 的站一律回 `None`（"非消防站"这一 `None` 成因由**本层**
    /// 判定 —— 调用方 [`StationPeripheralSource`] 是**纯透传**，不做 role 过滤）；接缝还没接上时
    /// 也回 `None`，**不得**回 `Some(false)`（那会把「不可得」伪装成「未配置」）。
    fn cylinder_configured(&self, station_id: &str) -> Option<bool>;
}

/// 生产外设源：把 `latest_values` 的逐站快照投影成 [`PeripheralsSection`]。
pub struct StationPeripheralSource {
    latest: Arc<latest_values::LatestValues>,
    plan: Vec<PeripheralStationPlan>,
    /// 帧内 `catalog_rev` = catalog 端点的 `rev`（**同源同值**，§15.3.1）。
    catalog_rev: u32,
    /// 消防钢瓶气压取数接缝（§15.2.2 / §15.11 #7）。`None` = **接缝未接线**
    /// ⇒ 消防站的 `cylinder_configured` 恒 `None`（"不可得"）⇒ 屏侧按值正常展示
    /// （**不造假**：现场真"未配置"时会显数值或 `--`，属降级非错值）。
    cylinder: Option<Arc<dyn CylinderPressureQuery>>,
}

impl StationPeripheralSource {
    /// `catalog_rev` 由装配点从**同一份** catalog 求出（`display_proto::catalog_rev`）；
    /// 未接线 catalog（如未启用控制通道）⇒ 传 0（屏侧按"从未取到名称表"处理，不臆造）。
    ///
    /// 钢瓶气压接缝**默认不接线**（`cylinder = None`）⇒ 消防站恒「不可得」；需要
    /// EDGE-23「未配置」可达时用 [`Self::with_cylinder_query`] 注入（装配点 = `startup.rs`）。
    pub fn new(
        latest: Arc<latest_values::LatestValues>,
        plan: Vec<PeripheralStationPlan>,
        catalog_rev: u32,
    ) -> Self {
        Self {
            latest,
            plan,
            catalog_rev,
            cylinder: None,
        }
    }

    /// 注入消防钢瓶气压取数接缝（设计 §15.11 #7）。装配点在 `startup.rs`。
    pub fn with_cylinder_query(mut self, q: Arc<dyn CylinderPressureQuery>) -> Self {
        self.cylinder = Some(q);
        self
    }

    /// **逐站重建**区间（读全量 → 重建整段；无状态 ⇒ 可重复调用、可丢通知）。
    pub fn build_section(&self, now_ms: u64) -> PeripheralsSection {
        let mut stations = Vec::with_capacity(self.plan.len());
        for st in &self.plan {
            // ① 站级：活性（`stale_timeout_s` 的唯一持有者）+ 最后成功时刻（R-38 getter）
            let active = self.latest.station_is_active(&st.id, now_ms);
            let last_ok_ms = self.latest.station_last_poll_ms(&st.id).unwrap_or(0);
            // 点的原始快照（**单次持读锁克隆该站全部点**，不逐点取锁）
            let raw: std::collections::HashMap<String, latest_values::PointValue> = self
                .latest
                .station_snapshot(&st.id)
                .into_iter()
                .map(|pv| (pv.id.metric, pv.value))
                .collect();

            let mut blocks = Vec::with_capacity(st.blocks.len());
            for b in &st.blocks {
                let mut values = Vec::with_capacity(b.ats.len());
                let mut block_ts = 0u64;
                for at in &b.ats {
                    let key = rename_or(&b.renames, *at, &b.name);
                    if let Some(pv) = raw.get(&key) {
                        if pv.ts_ms > block_ts {
                            block_ts = pv.ts_ms; // 块级"最近更新"= 块内点的最新采集时刻（信息性，F25.2）
                        }
                    }
                    let (v, flag) = point_field(
                        b.is_bit,
                        active,
                        Some(last_ok_ms).filter(|x| *x > 0),
                        &raw,
                        &key,
                    );
                    debug_assert!(
                        flag == FieldFlag::Valid || v.is_none(),
                        "不变量：flag != Valid ⇒ v 必须 None（不补 0）"
                    );
                    values.push(FramePoint {
                        at: *at,
                        v,
                        flag,
                    });
                }
                blocks.push(PeripheralBlock {
                    name: b.name.clone(),
                    ts_ms: block_ts,
                    renames: b.renames.clone(),
                    values,
                });
            }
            stations.push(PeripheralStation {
                id: st.id.clone(),
                role: st.role,
                // 站在线 = 采集侧结论（本节**不重判、不另立门限**）
                online: active,
                last_ok_ms,
                // EDGE-23 的钢瓶气压接缝（§15.2.2 / §15.11 #7）：**纯透传**——接缝未接线
                // （`cylinder == None`）时恒 `None`（本段不臆造、不猜），其余两态
                // （`Some(false)` / `Some(true)`）由注入的 [`CylinderPressureQuery`] 给权威结论
                // （"非消防站 ⇒ None" 的 role 判据在适配器内，与 southd 的 `Role` 同源，只写一处）。
                // `Some(false)` ⇒ 屏显「未配置」（忽略 `v`、不得含 `0 kPa`、不产告警）。
                cylinder_configured: self
                    .cylinder
                    .as_ref()
                    .and_then(|q| q.cylinder_configured(&st.id)),
                blocks,
            });
        }
        PeripheralsSection {
            // 段级 `ts_ms` = 本段**重建**时刻（**非**严格"组帧时刻"；与 device / alarms /
            // interlock 三段逐段同构——采样侧取 `now_ms()`、组帧只克隆）。评审 T20 (B) D3
            // 已裁定：接受该口径、仅订正措辞（无消费者：内容比较显式忽略时标）。
            ts_ms: now_ms,
            // 段可用性：本源每次采样都**真的**重建了段（内存读，不可能失败）⇒ `true`；
            // 未接线（`DisplayDataProvider` 无源）时缓存保持 `Default` ⇒ `available=false`
            // （EDGE-22「外设数据不可用」），两条路径语义不同、不得互相顶替。
            available: true,
            catalog_rev: self.catalog_rev,
            truncated: Vec::new(), // 裁剪痕迹由组帧侧守卫填（§15.2.4 步骤 4）
            stations,
        }
    }
}

/// 键的唯一构造点（与 `display_proto::PeripheralBlock::key` **同口径**：显式 `name` 优先，
/// 否则 `<块名>_<at>`）——两侧必须逐字一致，否则帧内键与 `latest_values` 的 metric 对不上。
fn rename_or(renames: &[(u16, String)], at: u16, block: &str) -> String {
    match renames.iter().find(|(a, _)| *a == at) {
        Some((_, n)) => n.clone(),
        None => format!("{block}_{at}"),
    }
}

impl PeripheralSource for StationPeripheralSource {
    fn snapshot(&self, now_ms: u64) -> PeripheralsSection {
        self.build_section(now_ms)
    }

    fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<ChangeBatch>> {
        Some(self.latest.subscribe())
    }
}

/// 外设段**内容真变化**判定（设计 §15.1.1，对齐 N-16 的既有范式）。
///
/// **忽略** `ts_ms` / `last_ok_ms` / `block.ts_ms` / `catalog_rev` / `truncated`：
/// 兜底 tick 每 500 ms 都会让这些时标前进；若计入"内容"，慢拍 D 将退化为**恒定 4 Hz**
/// 唤醒组帧（发布率上界虽不破，但"合并突发"的语义失效，且平白抬高 HMI 轮询负载）。
/// 比较的是**展示内容**：段可用性 / 站集合与在线态 / 块集合与点名覆盖 / 点的 `(at, v, flag)`。
fn peripherals_changed(a: &PeripheralsSection, b: &PeripheralsSection) -> bool {
    if a.available != b.available || a.stations.len() != b.stations.len() {
        return true;
    }
    for (x, y) in a.stations.iter().zip(b.stations.iter()) {
        if x.id != y.id
            || x.role != y.role
            || x.online != y.online
            || x.cylinder_configured != y.cylinder_configured
            || x.blocks.len() != y.blocks.len()
        {
            return true;
        }
        for (bx, by) in x.blocks.iter().zip(y.blocks.iter()) {
            if bx.name != by.name || bx.renames != by.renames || bx.values != by.values {
                return true;
            }
        }
    }
    false
}

// ═══════════════════════════════════════════════════════════════════════════
// 发布节拍器（纯逻辑：无 I/O 无时钟读取，可用**假时钟**推进单测；设计 §4.2.1 验证要求）
// ═══════════════════════════════════════════════════════════════════════════

/// 组帧触发源 = ① 主拍心跳（`publish_ms`）∪ ② 慢拍内容变更（合并窗口 `min_window`）。
///
/// 设计 §4.2.1 约束 4：慢拍段**必须**走「变更即组帧」，退化为纯主拍会使 F7.3/F16.5 变为
/// `0.5 + 1.0 + 0.5 + 0.1 = 2.1 s` 超差。本类型即该机制的**唯一判据点**（可被假时钟钉死）。
#[derive(Debug)]
pub struct PublishPacer {
    publish: std::time::Duration,
    min_window: std::time::Duration,
    /// 下一次主拍到期时刻
    next_main: Instant,
    /// 上次实际发布时刻（合并窗口基准）
    last_publish: Instant,
    /// 有未发布的变更在等合并窗口
    pending_change: bool,
}

impl PublishPacer {
    /// `publish_ms` 主拍周期；`min_window_ms` 合并窗口（≤ `publish_ms`，配置层 validate 保证）。
    pub fn new(now: Instant, publish_ms: u64, min_window_ms: u64) -> Self {
        Self {
            publish: std::time::Duration::from_millis(publish_ms.max(1)),
            min_window: std::time::Duration::from_millis(min_window_ms),
            next_main: now + std::time::Duration::from_millis(publish_ms.max(1)),
            last_publish: now,
            pending_change: false,
        }
    }

    /// 慢拍内容变更（由采样任务在**内容真的变了**时调用；`ts_ms` 变化不算，见 `section_changed`）。
    pub fn on_change(&mut self) {
        self.pending_change = true;
    }

    /// 下一次应当醒来的时刻：主拍到期 ∪ （有待发布变更时的）合并窗口到期，取较早者。
    pub fn next_deadline(&self) -> Instant {
        if self.pending_change {
            (self.last_publish + self.min_window).min(self.next_main)
        } else {
            self.next_main
        }
    }

    /// 醒来一次：返回**是否应当发布**（`true` 时内部时刻前推）。
    pub fn on_wake(&mut self, now: Instant) -> bool {
        let main_due = now >= self.next_main;
        let window_due =
            self.pending_change && now.duration_since(self.last_publish) >= self.min_window;
        if !main_due && !window_due {
            return false;
        }
        self.last_publish = now;
        self.pending_change = false;
        self.next_main = now + self.publish;
        true
    }
}

/// 采集+组帧+发布组件（设计 §4.2 DisplayDataProvider）。
pub struct DisplayDataProvider {
    /// AiIntegrator（SOC 唯一裁决入口，只读快照，不参与控制态）。
    ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    /// IntercoreClient（三相展示读 + run_state/连接态查询）。
    intercore: Arc<mupc_intercore::IntercoreClient>,
    /// 域值化量程（设计 §4.4：越界 → RangeError）。
    range: DisplayRange,
    /// 采集/组帧/发布周期 ms（设计 §3.5，标称 1Hz）。
    publish_ms: u64,
    /// transport==modbus_rtu 时三相缺段读 = `Offline`（PCS 离线/读失败）；否则（tcp/sim
    /// 无 PCS 3 区点表）transport 不支持 = `NotRead`（设计 §4.1/§4.4）。
    modbus_transport: bool,
    /// 单调发布序号（重启清零；渲染端判连续/重排，§3.3）。
    seq: u64,
    /// 最新帧共享存储（与 LoopbackHttpPublisher 共享同一 Arc）。
    latest: SharedLatest,
    /// 「变更即组帧」合并窗口 ms（`display.min_publish_interval_ms`，设计 §4.2.1 约束 2）。
    min_publish_interval_ms: u64,
    /// F6 装置状态源（`None` = 未接线 ⇒ 该段保持 `Default` = 各字段「未知」）。
    device_source: Option<Arc<dyn DeviceSource>>,
    /// F7 告警源（`None` = 未接线 ⇒ `available=false` = 「告警源不可用」）。
    alarm_source: Option<Arc<dyn AlarmSource>>,
    /// F16 联锁接线形态（三态语义见 [`InterlockWiring`]）。
    interlock_wiring: InterlockWiring,
    /// 慢拍节拍 ms（`display.{device,alarm,interlock}_poll_ms`；上界为**时延红线**，见 §4.2.1）。
    device_poll_ms: u64,
    alarm_poll_ms: u64,
    interlock_poll_ms: u64,
    /// F7 告警条数上限（`display.alarm_page_size`，契约「items ≤10」的组帧侧兜底）。
    alarm_page_size: usize,
    /// 外设段源（U-73 慢拍 D；`None` = 未接线 ⇒ 段保持 `Default` = 「外设数据不可用」）。
    peripheral_source: Option<Arc<dyn PeripheralSource>>,
    /// 外设段兜底 tick 周期（`display.periph_poll_ms`，设计 §15.1.1）。
    periph_poll_ms: u64,
    /// 慢拍段缓存（采样任务写 / 组帧任务读——**帧路径只读内存**）。
    caches: SlowCaches,
    /// 慢拍内容变更唤醒（合并窗口内合并为一次发布）。
    notify: Arc<Notify>,
    /// P6 装置信息（**启动时一次性**采集，设计 §6.6「不随数据刷新跳动」）。
    info: InfoSection,
}

impl DisplayDataProvider {
    /// 创建提供层（`modbus_transport` = `config.intercore.transport=="modbus_rtu"`，
    /// 启动侧从 core_config 判定传入，用于 Offline/NotRead 区分）。
    pub fn new(
        ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
        intercore: Arc<mupc_intercore::IntercoreClient>,
        cfg: &DisplayConfig,
        modbus_transport: bool,
        latest: SharedLatest,
    ) -> Self {
        Self {
            ai_integrator,
            intercore,
            range: cfg.range.clone(),
            publish_ms: cfg.publish_ms.max(50), // 防 0/极小周期空耗（KISS 下限 50ms）
            modbus_transport,
            seq: 0,
            latest,
            min_publish_interval_ms: cfg.min_publish_interval_ms,
            device_source: None,
            alarm_source: None,
            interlock_wiring: InterlockWiring::Unwired,
            // 慢拍节拍下界沿用 publish_ms 的同款守卫（0/极小周期会空耗内核）
            device_poll_ms: cfg.device_poll_ms.max(50),
            alarm_poll_ms: cfg.alarm_poll_ms.max(50),
            interlock_poll_ms: cfg.interlock_poll_ms.max(50),
            alarm_page_size: cfg.alarm_page_size,
            peripheral_source: None,
            // 兜底 tick 下界沿用 publish_ms 的同款守卫（0/极小周期会空耗内核）
            periph_poll_ms: cfg.periph_poll_ms.max(50),
            caches: SlowCaches::default(),
            notify: Arc::new(Notify::new()),
            info: collect_info(),
        }
    }

    /// 接入慢拍四段源（设计 §4.2）。**生产装配必调**——不调则四段保持「不可用」缺省
    /// （`device` 各字段「未知」/ `alarms.available=false` / `interlock.available=false`），
    /// 屏显「不可用」而**非**「无告警」「未联锁」。
    pub fn with_slow_sources(
        mut self,
        device: Option<Arc<dyn DeviceSource>>,
        alarms: Option<Arc<dyn AlarmSource>>,
        interlock: InterlockWiring,
    ) -> Self {
        // `io.enabled=false` 是**已知状态**（功能未启用），构造即写入，不必等首个采样周期——
        // 否则首 0.5 s 会误显「联锁状态不可用」（与「功能未启用」语义不同）。
        if let InterlockWiring::Disabled = interlock {
            let mut g = self
                .caches
                .interlock
                .write()
                .unwrap_or_else(|e| e.into_inner());
            *g = InterlockSection {
                ts_ms: now_ms(),
                available: true,
                enabled: false,
                ..Default::default()
            };
        }
        self.device_source = device;
        self.alarm_source = alarms;
        self.interlock_wiring = interlock;
        self
    }

    /// 接入外设段源（U-73 §15.1.1 慢拍 D）。**不调则外设段保持「不可用」缺省**
    /// （`available=false` ⇒ 屏显「外设数据不可用」），**不是**"没有外设"。
    pub fn with_peripheral_source(mut self, src: Arc<dyn PeripheralSource>) -> Self {
        self.peripheral_source = Some(src);
        self
    }

    /// 外设段缓存快照（组帧用；`available=false` = 段不可用，EDGE-22）。
    pub fn peripherals(&self) -> PeripheralsSection {
        read_cache(&self.caches.peripherals)
    }

    /// 后台主循环（startup 装配 `config.display.enabled` 时 spawn）。
    ///
    /// `tokio::join!`：主拍与三路慢拍并发于**同一 task**（随 outer task abort 一同终止，无需
    /// 额外 guard）。慢拍各自失败各自降级，**不阻塞**主拍（设计 §4.2 / D6）。
    pub async fn run(mut self) {
        let caches = self.caches.clone();
        let notify = self.notify.clone();
        let dev = self.device_source.clone();
        let alm = self.alarm_source.clone();
        let ilk = match &self.interlock_wiring {
            InterlockWiring::Wired(src) => Some(src.clone()),
            _ => None,
        };
        let (dp, ap, ip) = (
            self.device_poll_ms,
            self.alarm_poll_ms,
            self.interlock_poll_ms,
        );
        let page_size = self.alarm_page_size;
        let periph = self.peripheral_source.clone();
        let pp = self.periph_poll_ms;
        tokio::join!(
            self.run_publish_loop(),
            Self::run_device_sampler(dev, caches.device.clone(), notify.clone(), dp),
            Self::run_alarm_sampler(alm, caches.alarms.clone(), notify.clone(), ap, page_size),
            Self::run_interlock_sampler(ilk, caches.interlock.clone(), notify.clone(), ip),
            // 慢拍 D（外设段）：自有 `notify` 副本（`join!` 第四路），既有三路不变
            Self::run_periph_sampler(periph, caches.peripherals.clone(), notify, pp),
        );
    }

    /// 一次慢拍循环体（三路共用）：取新段 → 与旧段比 **展示内容**（`ts_ms` 除外，见
    /// [`device_changed`] 等）→ 写缓存 → 内容变才唤醒组帧。抽成独立函数便于测试**确定性**
    /// 驱动（免 sleep，设计 §4.2.1「以假时钟推进」）。
    ///
    /// 返回「内容是否变化」（仅供测试判别；生产循环不关心）。
    async fn slow_tick<T, Fut>(
        cache: &RwLock<T>,
        notify: &Notify,
        fetch: Fut,
        changed: fn(&T, &T) -> bool,
    ) -> bool
    where
        T: Clone,
        Fut: std::future::Future<Output = T>,
    {
        let fresh = fetch.await;
        let changed = {
            let mut g = cache.write().unwrap_or_else(|e| e.into_inner());
            let c = changed(&g, &fresh);
            *g = fresh;
            c
        };
        if changed {
            notify.notify_one();
        }
        changed
    }

    /// 主拍循环：`PublishPacer` 决定何时组帧——主拍到期 ∪ 慢拍变更（受合并窗口约束）。
    async fn run_publish_loop(&mut self) {
        let mut pacer = PublishPacer::new(
            Instant::now(),
            self.publish_ms,
            self.min_publish_interval_ms,
        );
        loop {
            let deadline = tokio::time::Instant::from_std(pacer.next_deadline());
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {}
                _ = self.notify.notified() => {
                    pacer.on_change();
                    continue; // 重新计算到期时刻（合并窗口可能更早）
                }
            }
            if pacer.on_wake(Instant::now()) {
                self.sample_once().await;
            }
        }
    }

    /// 慢拍 A：装置状态（设计 §4.2 表 A 行）。
    async fn run_device_sampler(
        src: Option<Arc<dyn DeviceSource>>,
        cache: Arc<RwLock<DeviceSection>>,
        notify: Arc<Notify>,
        period_ms: u64,
    ) {
        let Some(src) = src else { return }; // 未接线：缓存保持 Default（字段「未知」）
        let period = std::time::Duration::from_millis(period_ms);
        loop {
            Self::slow_tick(&cache, &notify, src.read_device(), device_changed).await;
            tokio::time::sleep(period).await;
        }
    }

    /// 慢拍 B：告警（设计 §4.2 表 B 行）。`Err` ⇒ `available=false`（「告警源不可用」，EDGE-09）。
    async fn run_alarm_sampler(
        src: Option<Arc<dyn AlarmSource>>,
        cache: Arc<RwLock<AlarmsSection>>,
        notify: Arc<Notify>,
        period_ms: u64,
        page_size: usize,
    ) {
        let Some(src) = src else { return };
        let period = std::time::Duration::from_millis(period_ms);
        loop {
            let fresh = Self::sample_alarms(src.as_ref(), page_size).await;
            Self::slow_tick(&cache, &notify, async { fresh }, alarms_changed).await;
            tokio::time::sleep(period).await;
        }
    }

    /// 告警段**单次采集**：`Err` ⇒ `available=false`（「告警源不可用」，EDGE-09）。
    async fn sample_alarms(src: &dyn AlarmSource, page_size: usize) -> AlarmsSection {
        match src.read_alarms().await {
            Ok(items) => {
                let mut sec = AlarmsSection {
                    ts_ms: now_ms(),
                    available: true, // 源可用（哪怕 0 条 = 真「无告警」）
                    items,
                };
                // 契约「items ≤ `alarm_page_size`」的**组帧侧兜底**（源实现亦已截断）：
                // `cap_items` 只截断、**不动 `available`**——「源不可用」与「条目被裁到上限」
                // 是两个语义（EDGE-09）。
                sec.cap_items(page_size);
                sec
            }
            Err(reason) => {
                tracing::warn!(
                    "display 告警源读取失败: {reason}（本段置不可用，屏显「告警源不可用」）"
                );
                AlarmsSection {
                    ts_ms: now_ms(),
                    available: false,
                    items: Vec::new(),
                }
            }
        }
    }

    /// 慢拍 C：联锁（设计 §4.2 表 C 行）。`Err` ⇒ `available=false`（「联锁状态不可用」，
    /// **不是**「未联锁」，IL-01.6）。
    async fn run_interlock_sampler(
        src: Option<Arc<dyn InterlockSource>>,
        cache: Arc<RwLock<InterlockSection>>,
        notify: Arc<Notify>,
        period_ms: u64,
    ) {
        let Some(src) = src else { return };
        let period = std::time::Duration::from_millis(period_ms);
        loop {
            let fresh = Self::sample_interlock(src.as_ref()).await;
            Self::slow_tick(&cache, &notify, async { fresh }, interlock_changed).await;
            tokio::time::sleep(period).await;
        }
    }

    /// 慢拍 D：外设段（设计 §15.1.1 / §15.6.1）。触发 = **变更广播 ∪ 兜底 tick**。
    ///
    /// **正确性不依赖广播**：每次采样都是「逐站读全量 → 重建整段」的无状态操作 ⇒ 广播
    /// 丢一次（`Lagged`）只意味着"提前量没了"，最坏晚 `periph_poll_ms` 收敛
    /// （§15.6.1 约束 1 的收敛由本 tick 保证，**不**压在"通知必达"上）。
    async fn run_periph_sampler(
        src: Option<Arc<dyn PeripheralSource>>,
        cache: Arc<RwLock<PeripheralsSection>>,
        notify: Arc<Notify>,
        period_ms: u64,
    ) {
        let Some(src) = src else { return }; // 未接线：缓存保持 Default（「外设数据不可用」）
        let period = std::time::Duration::from_millis(period_ms);
        let mut rx = src.subscribe();
        loop {
            let fresh = src.snapshot(now_ms());
            Self::slow_tick(&cache, &notify, async { fresh }, peripherals_changed).await;
            match rx.as_mut() {
                Some(r) => {
                    tokio::select! {
                        // 兜底 tick：**丢帧收敛的唯一保证**（§15.6.1 约束 1）
                        _ = tokio::time::sleep(period) => {}
                        msg = r.recv() => match msg {
                            // Ok（有变更）/ Lagged（落后丢帧）都只是"提前一拍"：下一次采样本就是
                            // 全量重建 ⇒ `Lagged` **无需**额外补救（§9.1.5 的"全量重读"天然成立）。
                            // Closed ⇒ 通知源消失 ⇒ 退化为纯兜底 tick。
                            Ok(_) | Err(RecvError::Lagged(_)) => {}
                            Err(RecvError::Closed) => rx = None,
                        },
                    }
                }
                None => tokio::time::sleep(period).await,
            }
        }
    }

    /// 联锁段**单次采集**：`Err` ⇒ `available=false`（「联锁状态不可用」，**不是**「未联锁」）。
    async fn sample_interlock(src: &dyn InterlockSource) -> InterlockSection {
        match src.read_interlock().await {
            Ok(sec) => sec,
            Err(reason) => {
                tracing::warn!(
                    "display 联锁状态源读取失败: {reason}（本段置不可用，屏显「联锁状态不可用」）"
                );
                InterlockSection {
                    ts_ms: now_ms(),
                    available: false,
                    ..Default::default()
                }
            }
        }
    }

    /// 采一帧并发布（帧 seq 递增、ts=now、原子写 latest），返回该帧（供测试单拍断言）。
    pub async fn sample_once(&mut self) -> DisplayFrame {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let mut frame = self.build_frame(now_ms).await;
        frame.seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        // §15.2.4 步骤 5：出口守卫把**既有段自身超预算**的帧标为不可发布 ⇒ 此时**不覆盖**
        // `latest`（屏侧继续拿上一帧：画面冻结，而非黑屏 / 半帧）。seq 仍递增（序 = 组帧次数）。
        if frame.to_json_slice().is_err() {
            tracing::error!("本帧不可编码（超 MAX_FRAME_BYTES）⇒ 不更新 latest（保留上一帧）");
            return frame;
        }
        // O2：毒化不 panic——仓库既有风格取回内部值（采集路径不得因一次 panic 永久失效）。
        let mut g = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        *g = Some(frame.clone());
        frame
    }

    /// 组帧（域值化 + 逐字段打 flag + 6.6 一致性）。seq 由 sample_once 写入。
    async fn build_frame(&self, now_ms: u64) -> DisplayFrame {
        // ── F1 SOC：AiIntegrator 唯一裁决入口（不重判源；双失/无 fresh → Lost 不上冻结值）──
        let snap = self.ai_integrator.soc_display_snapshot().await;
        let (soc, soc_source, soc_flag) = if snap.dual_lost || snap.value_pct.is_none() {
            (None, SocSource::Lost, FieldFlag::Offline)
        } else {
            let source = match snap.source {
                mupc_strategy_engine::ai_integration::SocSourceKind::Bms => SocSource::Bms,
                mupc_strategy_engine::ai_integration::SocSourceKind::PcsReg1010 => {
                    SocSource::PcsReg1010
                }
                mupc_strategy_engine::ai_integration::SocSourceKind::None => SocSource::Lost,
            };
            // 裁决值由 resolve 天然规避非有限/越 0..100（§4.4）→ 能展示即 Valid
            (snap.value_pct, source, FieldFlag::Valid)
        };

        // ── F2 run_state（1013 心跳维护）+ 核间链路在线 ──
        let run_state = self.intercore.last_run_state().and_then(RunState::from_raw);
        let online = self.intercore.is_connected().await;
        let pcs_online = online && run_state.is_some();

        // ── F3/F4 三相（1022-1032 已 ×0.1 工程值，intercore 侧解码；量程校验在本层）──
        let three = self.intercore.read_three_phase().await;
        // transport 缺 PCS 3 区点表 → NotRead；modbus 读失败 → Offline（§4.4）
        let missing = if self.modbus_transport {
            FieldFlag::Offline
        } else {
            FieldFlag::NotRead
        };
        let (p_phase, p_total, i_phase) = match three {
            Some(tr) => {
                let p = Self::phase_fields(tr.p_phase, self.range.phase_power_max_kw, missing);
                let i = Self::phase_fields(tr.i_phase, self.range.current_max_a, missing);
                let total = match tr.p_total {
                    None => Field { v: None, flag: missing },
                    Some(v) => Self::scalar_field(v, self.range.total_power_max_kw),
                };
                (p, total, i)
            }
            None => (
                [Field { v: None, flag: missing }; 3],
                Field { v: None, flag: missing },
                [Field { v: None, flag: missing }; 3],
            ),
        };

        // ── 6.6 一致性：run∈{充/放} 且 Σp_phase 与预期方向显著反向 → true（仅佐证，主状态仍 1013）──
        let inconsistency =
            Self::check_inconsistency(run_state, &p_phase, self.range.inconsistency_threshold_kw);

        // ── v3 外设段（U-73 §15.2.4 步骤 2–5；顺序**不得调换**）──
        // 步骤 2：段可用性——源未接线 / 快照不可得 ⇒ 缓存保持 `Default`（`available=false`，
        //         屏显「外设数据不可用」EDGE-22，**不伪装**）。
        // 步骤 3：非 `fire_det` 块全量携带（白名单 441 点，与 n 无关）——取样时已按白名单带上。
        // 步骤 4：`fire_det` 预算预检（**按 `POINT_JSON_BYTES_UPPER` 上界口径**）⇒ 超 `k_max`
        //         只数即**前缀截断**并 push `truncated`（屏侧必须显式提示，F21.4）。
        let mut peripherals = self.peripherals();
        if mupc_display_proto::enforce_fire_det_budget(&mut peripherals) {
            tracing::warn!(
                truncated = ?peripherals.truncated,
                "外设 fire_det 明细超出帧预算，已按地址升序前缀截断（F21.4：屏侧须显式提示）"
            );
        }

        let mut frame = DisplayFrame {
            version: PROTO_VERSION,
            peripherals,
            seq: 0, // sample_once 写入真实 seq
            ts_ms: now_ms,
            soc,
            soc_source,
            soc_flag,
            run_state,
            pcs_online,
            p_phase,
            p_total,
            i_phase,
            inconsistency,
            // ── v2 四段：读**内存缓存**（慢拍任务写，本路径零阻塞 I/O、零 DB 查询，§2.1/D6）──
            // 各段均带自己的采集时刻与可用性；未采集/未接线时保持 `Default`——即
            // `alarms.available=false`（「告警源不可用」）/ `interlock.available=false`
            // （「联锁状态不可用」）/ 装置字段「未知」，**绝不**退化成「无告警」「未联锁」
            // （PRD EDGE-09 / IL-01.6 / F1.4 / §9 边界）。
            device: self.device(),
            alarms: self.alarms(),
            info: self.info.clone(),
            interlock: self.interlock(),
        };

        // 步骤 5：出口守卫（**唯一出口** `DisplayFrame::to_json_slice`，不得绕开）——估算
        // 失准时的兜底。调用点在此（`build_frame`，设计 §15.11 #6）；`tracing` 由调用方补
        // （契约层 `display-proto` 无 tracing 依赖，T19 评审残留 ③①）。
        match mupc_display_proto::enforce_exit_guard(&mut frame) {
            ExitGuardOutcome::NoChange => {}
            ExitGuardOutcome::PeripheralsDowngraded => tracing::error!(
                "peripherals 段超帧预算，本段置不可用（不重试、不发超限帧）；既有段照常发布"
            ),
            // 清空外设段后仍不可编码（既有段自身超预算）⇒ **本帧不得发布**（§15.2.4 步骤 5）
            ExitGuardOutcome::StillTooLarge => tracing::error!(
                "整帧超 MAX_FRAME_BYTES（既有段自身超预算）⇒ 本帧不发布，保留上一帧（不黑屏）"
            ),
        }
        frame
    }

    /// F6 装置段（缓存快照；`info` 之外唯一来源）。
    fn device(&self) -> DeviceSection {
        read_cache(&self.caches.device)
    }

    /// F7 告警段（缓存快照）。
    fn alarms(&self) -> AlarmsSection {
        read_cache(&self.caches.alarms)
    }

    /// F16 联锁段（缓存快照）。
    fn interlock(&self) -> InterlockSection {
        read_cache(&self.caches.interlock)
    }

    /// 一段三相读数 → [Field;3]：段缺失全打 missing（Offline/NotRead）；元素量程/有限性校验
    /// 越界 → RangeError（值 None，不补 0）。
    fn phase_fields(raw: Option<[f64; 3]>, max: f64, missing: FieldFlag) -> [Field; 3] {
        match raw {
            None => [Field { v: None, flag: missing }; 3],
            Some(arr) => {
                let mut out = [Field { v: None, flag: missing }; 3];
                for (i, v) in arr.iter().enumerate() {
                    out[i] = Self::scalar_field(*v, max);
                }
                out
            }
        }
    }

    /// 单值域值化：有限且 |v| ≤ 量程 → Valid；否则 RangeError（值 None）。
    fn scalar_field(v: f64, max: f64) -> Field {
        if v.is_finite() && v.abs() <= max {
            Field {
                v: Some(v),
                flag: FieldFlag::Valid,
            }
        } else {
            Field {
                v: None,
                flag: FieldFlag::RangeError,
            }
        }
    }

    /// 6.6 佐证一致性（纯逻辑，可单测）：三相有功均有效才求 Σ；run=充(2) 时 Σ 显著为正、
    /// run=放(3) 时 Σ 显著为负 → 方向不一致 true；其余 false（含任一相缺失 → 无佐证输入）。
    fn check_inconsistency(run: Option<RunState>, p: &[Field; 3], threshold: f64) -> bool {
        let mut sum = 0.0;
        for f in p.iter() {
            match f.v {
                Some(v) => sum += v,
                None => return false, // 任一相无效 → 无可靠 Σ 佐证，不妄断方向
            }
        }
        match run {
            Some(RunState::Charge) => sum > threshold,
            Some(RunState::Discharge) => sum < -threshold,
            _ => false,
        }
    }
}

/// 回环 HTTP 发布组件（设计 §3.2 LoopbackHttpPublisher）：127.0.0.1 仅回环，GET 最新帧 JSON。
pub struct LoopbackHttpPublisher {
    latest: SharedLatest,
}

impl LoopbackHttpPublisher {
    pub fn new(latest: SharedLatest) -> Self {
        Self { latest }
    }

    /// 常驻 accept 循环（startup 装配时 spawn）。每连接独立 task（KISS，逐连接短读短写，
    /// 渲染端每轮新建连接，不依赖 keep-alive，§3.1）。
    ///
    /// # 回环复查（第二层，**独立评审建议 9，2026-09-18 补**）
    ///
    /// 与**控制通道** [`crate::console_host::ConsoleHost::serve`] 对称：配置校验
    /// （契约 `DisplayConfig::validate()` → 启动期 fail-fast）只保证**配置串**是字面量回环；
    /// 本函数按**实际绑定结果**再判一次（`listener.local_addr()`，`0.0.0.0` / `::` 一律拒）。
    /// 防的是"配置校验被绕过 / 被新增调用路径跳过"（将来某条路径自行 `bind` 后直接 `serve`）。
    ///
    /// ⚠️ **本函数签名保持返回 `()`**（不像控制通道那样 `Result`）：既有 4 处调用点（`startup`
    /// 与 3 条用例）都是 `tokio::spawn(...)`，改成 `Result` 会波及调用点与用例形态；而"拒绝"在
    /// **读**通道上本就是"不提供数据"（无写能力、无授权面），**立即返回 + `error!` 留痕**即可
    /// 达到同样的 fail-safe 效果（用例据"future 立刻结束/持续运行"这一可观察差异定红绿）。
    pub async fn serve(self, listener: TcpListener) {
        if let Ok(addr) = listener.local_addr() {
            if !addr.ip().is_loopback() {
                tracing::error!(
                    "display 读通道拒绝服务非回环地址 {addr}——读通道仅允许 127.0.0.1/::1（PL-4 安全红线）"
                );
                return;
            }
        }
        loop {
            match listener.accept().await {
                Ok((stream, _peer)) => {
                    let latest = self.latest.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle(stream, latest).await {
                            tracing::debug!("display loopback 应答失败: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("display loopback accept 失败: {}（退避 100ms）", e);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// 处理单连接：读至请求头结束 → 校验 `GET /v1/display/latest` → 200 + 最新帧 JSON /
    /// 503（未就绪）/ 404（其它路径）。`Connection: close`（每次连接即关，渲染端每轮新建连接亦可）。
    /// 读到 `\r\n\r\n`（头结束）再应答：避免收到缓冲残留未读数据时 close 触发 Windows RST，
    /// 保证客户端得到干净的 FIN/EOF。
    async fn handle(mut stream: TcpStream, latest: SharedLatest) -> std::io::Result<()> {
        // O3：整段请求头读取套**总时限**（非每字节各自计时）——慢速滴字节的对端同样无法长期
        // 占用本 task（每连接独立 task，accept 无并发上限，此超时是最廉价的兜底）。
        let head = match tokio::time::timeout(HEAD_READ_TIMEOUT, Self::read_head(&mut stream)).await {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Ok(()), // 超时：直接关闭连接（渲染端本就有 2s GET 超时）
        };
        let head_owned = String::from_utf8_lossy(&head);
        let mut parts = head_owned.split_whitespace();
        let method = parts.next().unwrap_or("");
        let raw_path = parts.next().unwrap_or("");
        let path = match raw_path.find('?') {
            Some(i) => &raw_path[..i],
            None => raw_path,
        };

        // 短锁 clone 后出锁再序列化（HTTP 路径不持锁做序列化/IO；设计 §3.5 不在 HTTP 读 modbus）
        let frame = latest.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let (status, body) = if method == "GET" && path == LATEST_PATH {
            match frame {
                // O4：序列化失败明确 500 + 空体（原实现 200 + 空体，对端会当成功帧却解不出）。
                // 走契约的 `to_json_slice`（**不得**裸 `serde_json::to_vec`）：契约 §3.5 条 3 的
                // 编码侧守卫要求发布方先自检（逐条告警长度 ≤1 KiB、整帧 ≤64 KiB）。
                //
                // ⚠️ **实测事实（2026-09-16 订正，勿再引"HMI 端整帧丢弃"）**：HMI 侧的帧解码是
                // **裸 `serde_json::from_slice`**（`crates/local-display/src/channel.rs:450`），
                // **不经**契约的严格解码器 ⇒ HMI 只受**传输层 64 KiB**（`channel.rs:78/:675`）
                // 限制，不会因单条告警超 1 KiB 就丢帧。故本守卫的真实覆盖面是：
                // ① 整帧 >64 KiB —— **改进**（此前 200 发出去 HMI 必丢，现改 500 响亮失败）；
                // ② 单条告警 >1 KiB —— 若只靠本守卫就是**净回退**（把**局部**超限放大成**整帧**
                //    不可用、画面冻在旧帧）；该半边已在 ingest 侧截断（见 `truncate_alarm_message`）
                //    予以消除，**本守卫只作纯兜底**（防 JSON 转义膨胀等本层未覆盖的膨胀源）。
                Some(f) => match f.to_json_slice() {
                    Ok(b) => ("200 OK", b),
                    Err(e) => {
                        tracing::warn!("display 帧编码失败: {e}（应答 500，不计为成功帧）");
                        ("500 Internal Server Error", Vec::new())
                    }
                },
                // 未就绪 → 503，渲染端视同无新帧重试（§3.1）
                None => ("503 Service Unavailable", Vec::new()),
            }
        } else {
            ("404 Not Found", Vec::new())
        };
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).await?;
        if !body.is_empty() {
            stream.write_all(&body).await?;
        }
        stream.flush().await?;
        Ok(())
    }

    /// 逐字节收至请求头结束（GET 无 body；最多 4096 字节防异常长头）。EOF → 返回已收内容。
    /// 由 [`Self::handle`] 套总时限（[`HEAD_READ_TIMEOUT`]）调用（O3）。
    async fn read_head(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        let mut head = Vec::with_capacity(128);
        let mut b = [0u8; 1];
        loop {
            if stream.read(&mut b).await? == 0 {
                break;
            }
            head.push(b[0]);
            if head.ends_with(b"\r\n\r\n") || head.len() > 4096 {
                break;
            }
        }
        Ok(head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_display_proto::{
        AlarmsSection, DeviceSection, DisplayConfig, DisplayRange, FieldFlag, InfoSection,
        InterlockSection, SocSource,
    };
    use std::time::Instant;

    // ── 测试桩：核间 transport（可控三相/run_state/连接态），下行接口返回默认 ──
    #[derive(Clone)]
    struct StubIntercore {
        soc: Option<(f64, Instant)>,
        run: Option<u16>,
        connected: bool,
        three: Option<mupc_intercore::transport::ThreePhaseRead>,
    }

    #[async_trait::async_trait]
    impl mupc_intercore::IntercoreTransport for StubIntercore {
        async fn send_dual_param(
            &self,
            _c: &mupc_intercore::DualParamCommand,
        ) -> Result<(), mupc_common::MupcError> {
            Ok(())
        }
        async fn send_tai_command(
            &self,
            _p: [f64; 3],
            _q: [f64; 3],
            _m: &str,
        ) -> Result<(), mupc_common::MupcError> {
            Ok(())
        }
        async fn is_connected(&self) -> bool {
            self.connected
        }
        async fn shutdown(&self) -> Result<(), mupc_common::MupcError> {
            Ok(())
        }
        async fn latest_soc(&self) -> Option<(f64, Instant)> {
            self.soc
        }
        async fn stop(&self) -> Result<(), String> {
            Ok(())
        }
        async fn is_interlock_stopped(&self) -> bool {
            false
        }
        async fn restore_interlock_latched(&self, _b: bool) -> Result<(), String> {
            Ok(())
        }
        fn last_run_state(&self) -> Option<u16> {
            self.run
        }
        async fn authorize_restart(&self) -> Result<(), String> {
            Ok(())
        }
        async fn read_three_phase(&self) -> Option<mupc_intercore::transport::ThreePhaseRead> {
            self.three
        }
    }

    fn stub_client(
        run: Option<u16>,
        connected: bool,
        three: Option<mupc_intercore::transport::ThreePhaseRead>,
    ) -> Arc<mupc_intercore::IntercoreClient> {
        Arc::new(mupc_intercore::IntercoreClient::with_transport(Arc::new(
            StubIntercore {
                soc: None,
                run,
                connected,
                three,
            },
        )))
    }

    fn cfg() -> DisplayConfig {
        let mut c = DisplayConfig::default();
        c.publish_ms = 50; // 测试单拍调用不依赖 run 循环；小周期仅供 run 冒烟
        c
    }

    fn valid_three() -> mupc_intercore::transport::ThreePhaseRead {
        mupc_intercore::transport::ThreePhaseRead {
            i_phase: Some([22.5, 22.1, 22.3]),
            p_phase: Some([12.3, 11.8, 12.0]),
            p_total: Some(36.1),
        }
    }

    /// BMS fresh SOC + PCS 在线(放 3) + 三相全有效 → 帧各字段 Valid、pcs_online、一致性 false
    #[tokio::test]
    async fn frame_all_valid_bms_soc_discharge_consistent() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        ai.set_battery_soc(65.5).await; // fresh BMS → snapshot=Bms
        let mut provider = DisplayDataProvider::new(
            ai.clone(),
            stub_client(Some(3), true, Some(valid_three())),
            &cfg(),
            true,
            Arc::new(Mutex::new(None)),
        );
        let f = provider.sample_once().await;
        assert_eq!(f.version, PROTO_VERSION);
        assert_eq!(f.seq, 0);
        assert_eq!(f.soc, Some(65.5));
        assert_eq!(f.soc_source, SocSource::Bms);
        assert_eq!(f.soc_flag, FieldFlag::Valid);
        assert_eq!(f.run_state, Some(RunState::Discharge));
        assert!(f.pcs_online);
        assert_eq!(f.p_phase[0], Field { v: Some(12.3), flag: FieldFlag::Valid });
        assert_eq!(f.p_total, Field { v: Some(36.1), flag: FieldFlag::Valid });
        assert_eq!(f.i_phase[2], Field { v: Some(22.3), flag: FieldFlag::Valid });
        // 放(3) + Σp 显著为正 → 方向一致
        assert!(!f.inconsistency);
    }

    /// 双源皆失 SOC + PCS 离线（run None/connected false/三相缺段, modbus）→
    /// soc=None/Lost、run None、pcs_online false、三相 Offline（值 None 不造假；冻结 SOC 不上屏）
    #[tokio::test]
    async fn frame_dual_lost_and_offline_flags_not_faked() {
        use mupc_data_processing::telemetry::{
            BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
        };
        // 复用同一 intercore 桩：作为 AiIntegrator 的活读 client（latest_soc=None → 无 fresh）
        // 与 provider 的三相/run_state/连接源（PCS 离线）。BMS 未注入 + existing 冻结 30 → 双源皆失
        let shared_client = stub_client(None, false, None);
        let mut ai = mupc_strategy_engine::AiIntegrator::new();
        ai.set_intercore_client(shared_client.clone());
        ai.set_latest_data(DataPackage {
            timestamp: 0,
            electrical: ElectricalData::default(),
            device_status: DeviceStatus {
                inverter_status: InverterStatus::Running,
                pv_power: None,
                load_power: None,
                ev_charger_power: None,
            },
            battery: BatteryData {
                soc: Some(30.0),
                soh: None,
                temperature: None,
            },
        })
        .await;

        let mut provider = DisplayDataProvider::new(
            Arc::new(ai),
            shared_client,
            &cfg(),
            true, // modbus_rtu
            Arc::new(Mutex::new(None)),
        );
        let f = provider.sample_once().await;
        assert_eq!(f.soc, None, "双源皆失 → soc=None（冻结值 30 不上屏）");
        assert_eq!(f.soc_source, SocSource::Lost);
        assert_eq!(f.run_state, None);
        assert!(!f.pcs_online);
        for ph in &f.p_phase {
            assert_eq!(ph.v, None);
            assert_eq!(ph.flag, FieldFlag::Offline);
        }
        assert_eq!(f.p_total.flag, FieldFlag::Offline);
        assert!(!f.inconsistency);
    }

    /// transport=tcp（无 PCS 3 区点表）：三相 read None → NotRead（值 None），SOC 仍可看（BMS）
    #[tokio::test]
    async fn frame_tcp_transport_three_phase_not_read() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        ai.set_battery_soc(50.0).await;
        let mut provider = DisplayDataProvider::new(
            ai.clone(),
            stub_client(None, true, None), // tcp 通道：无 run_state、无三相点表
            &cfg(),
            false, // 非 modbus → NotRead
            Arc::new(Mutex::new(None)),
        );
        let f = provider.sample_once().await;
        assert_eq!(f.soc, Some(50.0));
        assert_eq!(f.soc_source, SocSource::Bms);
        for ph in &f.p_phase {
            assert_eq!(ph.flag, FieldFlag::NotRead);
            assert_eq!(ph.v, None);
        }
        assert_eq!(f.p_total.flag, FieldFlag::NotRead);
    }

    /// 量程越界（p_phase 1000kW > phase_power_max 100）→ RangeError 值 None；未越界相仍 Valid
    #[tokio::test]
    async fn frame_range_error_on_out_of_range_phase() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        ai.set_battery_soc(50.0).await;
        let three = mupc_intercore::transport::ThreePhaseRead {
            i_phase: Some([500.0, 22.1, 22.3]), // 500A > current_max 300 → RangeError
            p_phase: Some([1000.0, 11.8, 12.0]), // 1000kW > phase_power_max 100 → RangeError
            p_total: Some(1000.0),               // 1000kW > total_power_max 300 → RangeError
        };
        let mut provider = DisplayDataProvider::new(
            ai.clone(),
            stub_client(Some(0), true, Some(three)),
            &cfg(),
            true,
            Arc::new(Mutex::new(None)),
        );
        let f = provider.sample_once().await;
        assert_eq!(f.p_phase[0], Field { v: None, flag: FieldFlag::RangeError });
        assert_eq!(f.p_phase[1], Field { v: Some(11.8), flag: FieldFlag::Valid });
        assert_eq!(f.i_phase[0], Field { v: None, flag: FieldFlag::RangeError });
        assert_eq!(f.i_phase[1], Field { v: Some(22.1), flag: FieldFlag::Valid });
        assert_eq!(f.p_total, Field { v: None, flag: FieldFlag::RangeError });
    }

    // ── 6.6 一致性纯函数（run 与 Σp 方向）──
    fn pf(v: f64) -> Field {
        Field { v: Some(v), flag: FieldFlag::Valid }
    }

    #[test]
    fn inconsistency_detects_direction_mismatch() {
        let charge = Some(RunState::Charge);
        let discharge = Some(RunState::Discharge);
        // 充(2) 却 Σp 显著为正（输出）→ 方向不一致
        assert!(DisplayDataProvider::check_inconsistency(
            charge,
            &[pf(2.0), pf(1.0), pf(1.0)],
            3.0
        ));
        // 放(3) 却 Σp 显著为负（吸收）→ 方向不一致
        assert!(DisplayDataProvider::check_inconsistency(
            discharge,
            &[pf(-2.0), pf(-1.0), pf(-1.0)],
            3.0
        ));
        // 阈值下（|Σ|=3.0 不 > 3.0）→ 一致
        assert!(!DisplayDataProvider::check_inconsistency(
            charge,
            &[pf(1.0), pf(1.0), pf(1.0)],
            3.0
        ));
        // 停机/待机 不判方向
        assert!(!DisplayDataProvider::check_inconsistency(
            Some(RunState::Stop),
            &[pf(5.0), pf(5.0), pf(5.0)],
            3.0
        ));
        // 任一相缺失 → 无佐证输入 → false（不妄断）
        let miss = Field { v: None, flag: FieldFlag::Offline };
        assert!(!DisplayDataProvider::check_inconsistency(
            charge,
            &[pf(2.0), pf(2.0), miss],
            3.0
        ));
    }

    // ── LoopbackHttpPublisher：GET 最新帧 / 未就绪 503 / 错误路径 404 ──

    fn sample_frame(soc: Option<f64>) -> DisplayFrame {
        let missing = Field { v: None, flag: FieldFlag::NotRead };
        DisplayFrame {
            version: PROTO_VERSION,
            // v3 新增段（§15.2.2）：本用例不涉外设 ⇒ 取默认（available=false）
            peripherals: Default::default(),
            seq: 7,
            ts_ms: 1_757_412_000_000,
            soc,
            soc_source: if soc.is_some() { SocSource::Bms } else { SocSource::Lost },
            soc_flag: if soc.is_some() { FieldFlag::Valid } else { FieldFlag::Offline },
            run_state: Some(RunState::Charge),
            pcs_online: true,
            p_phase: [missing; 3],
            p_total: missing,
            i_phase: [missing; 3],
            inconsistency: false,
            // v2 的四段（`display-proto` §3.1）。本模块的用例只验证 HTTP 发行通路
            // （GET 最新帧 / 未就绪 503 / 错误路径 404），**不**断言段内容 ⇒ 取各段缺省值
            // （四段都是 `#[serde(default)]` + `Default`：「旧帧无该段」时 UI 显式降级、
            // 不伪装正常，与契约语义一致）。
            device: DeviceSection::default(),
            alarms: AlarmsSection::default(),
            info: InfoSection::default(),
            interlock: InterlockSection::default(),
        }
    }

    async fn connect_and_get(addr: std::net::SocketAddr, req: &str) -> Vec<u8> {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        out
    }

    /// **建议 9（独立评审）：读通道的"按实际绑定结果复查回环"第二层。**
    ///
    /// 与控制通道 `ConsoleHost::serve`（`console_host.rs::non_loopback_listener_is_refused`）
    /// 对称。判据可观察且**确定性**：非回环 listener 上 `serve` 的 future **立刻结束**；
    /// 回环 listener 上它**持续运行**（accept 循环）。
    ///
    /// **改什么会让本条变红**：删掉 `serve` 开头那段 `local_addr().is_loopback()` 复查
    /// （摘掉后非回环那份会一直跑 ⇒ 下面的 `timeout` 超时 ⇒ 红）。
    #[tokio::test]
    async fn read_channel_serve_refuses_non_loopback_listener_by_actual_bind() {
        // ① 非回环（0.0.0.0）⇒ 立刻拒绝（future 结束、不进 accept 循环）
        let latest: SharedLatest = Arc::new(Mutex::new(None));
        let bad = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let bad_addr = bad.local_addr().unwrap();
        assert!(!bad_addr.ip().is_loopback(), "本用例前提：0.0.0.0 非回环");
        let out = tokio::time::timeout(
            Duration::from_millis(500),
            LoopbackHttpPublisher::new(latest.clone()).serve(bad),
        )
        .await;
        assert!(
            out.is_ok(),
            "非回环 listener 必须被**立即拒绝**（serve 不得进入 accept 循环）——控制通道同款第二层"
        );

        // ② 正对照：回环 listener ⇒ 持续服务（超时到点 = 仍在 accept 循环里）
        let good = TcpListener::bind("127.0.0.1:0").await.unwrap();
        assert!(good.local_addr().unwrap().ip().is_loopback());
        let mut handle = tokio::spawn(LoopbackHttpPublisher::new(latest).serve(good));
        let out = tokio::time::timeout(Duration::from_millis(200), &mut handle).await;
        assert!(
            out.is_err(),
            "回环 listener 不得被拒（正对照，防「拒绝一切」式的假绿）"
        );
        handle.abort();
    }

    #[tokio::test]
    async fn http_get_returns_latest_frame_json() {
        let latest: SharedLatest = Arc::new(Mutex::new(Some(sample_frame(Some(65.5)))));
        let publisher = LoopbackHttpPublisher::new(latest.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve = tokio::spawn(publisher.serve(listener));

        let req = format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", LATEST_PATH);
        let resp = connect_and_get(addr, &req).await;
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "响应应为 200: {text:?}");
        // 解析 body（header 与 body 以空行分隔）
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
        let frame: DisplayFrame = serde_json::from_str(body).unwrap();
        assert_eq!(frame.soc, Some(65.5));
        assert_eq!(frame.seq, 7);

        serve.abort();
    }

    #[tokio::test]
    async fn http_get_not_ready_returns_503() {
        let latest: SharedLatest = Arc::new(Mutex::new(None)); // 未发布过帧
        let publisher = LoopbackHttpPublisher::new(latest.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve = tokio::spawn(publisher.serve(listener));

        let req = format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", LATEST_PATH);
        let resp = connect_and_get(addr, &req).await;
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 503"), "未就绪应 503: {text:?}");

        serve.abort();
    }

    #[tokio::test]
    async fn http_wrong_path_returns_404() {
        let latest: SharedLatest = Arc::new(Mutex::new(Some(sample_frame(None))));
        let publisher = LoopbackHttpPublisher::new(latest.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve = tokio::spawn(publisher.serve(listener));

        let resp = connect_and_get(addr, "GET /nope HTTP/1.1\r\nHost: x\r\n\r\n").await;
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 404"), "错误路径应 404: {text:?}");

        serve.abort();
    }

    // DisplayRange/DisplayConfig serde(default) 与 mupcd yaml display 段对齐（KISS 冒烟）
    #[test]
    fn default_range_matches_display_proto() {
        let r = DisplayRange::default();
        assert_eq!(r.current_max_a, 300.0);
        assert_eq!(r.phase_power_max_kw, 100.0);
        assert_eq!(r.total_power_max_kw, 300.0);
        assert_eq!(r.pcs_total_rated_kw, 60.0);
        assert_eq!(r.inconsistency_threshold_kw, 3.0);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 慢拍四段（设计 §4.2；工作单元 F）
    // ═══════════════════════════════════════════════════════════════════════
    //
    // 本段用例外**只断言** v2 四段与节拍机制；上方既有 9 条（v1 字段 + HTTP 通路）不动。

    use mupc_display_proto::{
        AlarmItem, AlarmLevel, ControlSource, InterlockSourceItem, LinkState, ServiceScope,
    };
    use mupc_storage::{EventRepository, StorageError, SystemEvent};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    // ── 桩源：调用计数 + 可改内容 + 可失败（三路各自独立）──

    /// 装置段桩。
    ///
    /// **`ts_ms` 每次读自增**（`1 + 调用序号`），刻意与生产口径一致（真源每拍填采集时刻）。
    /// 这使「`ts_ms` 不计入内容变更判据」这条**在集成层真的被钉死**：把 `ts_ms` 计入
    /// [`device_changed`] ⇒ `run_does_not_republish_when_slow_content_unchanged` 立即变红
    /// （此前桩恒返回 `ts_ms=1`，该用例的注释与实现不符、实际没钉住这条）。
    struct StubDevice {
        calls: Arc<AtomicUsize>,
        section: Arc<Mutex<DeviceSection>>,
        delay: Duration,
    }

    impl StubDevice {
        fn new(section: DeviceSection) -> Self {
            Self::slow(section, Duration::ZERO)
        }
        fn slow(section: DeviceSection, delay: Duration) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                section: Arc::new(Mutex::new(section)),
                delay,
            }
        }
        fn set(&self, s: DeviceSection) {
            *self.section.lock().unwrap() = s;
        }
        fn count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl DeviceSource for StubDevice {
        async fn read_device(&self) -> DeviceSection {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            let mut s = self.section.lock().unwrap().clone();
            // 每拍必变（模仿生产「真源每拍填采集时刻」）；**不得**被内容变更判据採纳
            s.ts_ms = 1 + n as u64;
            s
        }
    }

    /// 告警段桩：`Err` 表示源读失败。
    struct StubAlarms {
        calls: Arc<AtomicUsize>,
        result: Arc<Mutex<Result<Vec<AlarmItem>, String>>>,
    }

    impl StubAlarms {
        fn ok(items: Vec<AlarmItem>) -> Self {
            Self::new(Ok(items))
        }
        fn failing(reason: &str) -> Self {
            Self::new(Err(reason.to_string()))
        }
        fn new(result: Result<Vec<AlarmItem>, String>) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                result: Arc::new(Mutex::new(result)),
            }
        }
        fn count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl AlarmSource for StubAlarms {
        async fn read_alarms(&self) -> Result<Vec<AlarmItem>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.lock().unwrap().clone()
        }
    }

    /// 联锁段桩：`Err` 表示源读失败。
    struct StubInterlock {
        calls: Arc<AtomicUsize>,
        result: Arc<Mutex<Result<InterlockSection, String>>>,
    }

    impl StubInterlock {
        fn ok(sec: InterlockSection) -> Self {
            Self::new(Ok(sec))
        }
        fn failing(reason: &str) -> Self {
            Self::new(Err(reason.to_string()))
        }
        fn new(result: Result<InterlockSection, String>) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                result: Arc::new(Mutex::new(result)),
            }
        }
        fn count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl InterlockSource for StubInterlock {
        async fn read_interlock(&self) -> Result<InterlockSection, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.lock().unwrap().clone()
        }
    }

    /// 内存 `EventRepository`：`query_range` 按**插入顺序**返回（非倒序）——用于证明
    /// 采集层自己排序，不依赖存储实现的具体顺序（该顺序非 trait 契约）。
    #[derive(Default)]
    struct FakeEventRepo {
        rows: Arc<Mutex<Vec<SystemEvent>>>,
        fail: bool,
    }

    impl FakeEventRepo {
        fn with_event(&self, ts: chrono::DateTime<chrono::Utc>, event_type: &str, msg: &str) {
            self.rows.lock().unwrap().push(SystemEvent {
                id: None,
                timestamp: ts,
                event_type: event_type.to_string(),
                source: "test".to_string(),
                message: msg.to_string(),
            });
        }
    }

    #[async_trait::async_trait]
    impl EventRepository for FakeEventRepo {
        async fn insert(&self, event: &SystemEvent) -> Result<i64, StorageError> {
            let mut g = self.rows.lock().unwrap();
            g.push(event.clone());
            Ok(g.len() as i64)
        }
        async fn query_range(
            &self,
            start: chrono::DateTime<chrono::Utc>,
            end: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<SystemEvent>, StorageError> {
            if self.fail {
                return Err(StorageError::DatabaseError("fake: 查询失败".into()));
            }
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.timestamp >= start && e.timestamp <= end)
                .cloned()
                .collect())
        }
        async fn purge_older_than(
            &self,
            _before: chrono::DateTime<chrono::Utc>,
        ) -> Result<usize, StorageError> {
            Ok(0)
        }
        async fn latest_by_type(
            &self,
            _event_type: &str,
        ) -> Result<Option<SystemEvent>, StorageError> {
            Ok(None)
        }
    }

    /// 联锁 `InterlockApi` 桩（**契约形态**——单元 K：原实现 web-api 旧 trait）。
    ///
    /// 桩形态必须与生产实现**同一个 trait**（契约 `mupc_display_proto::InterlockApi`），
    /// 否则测的是"另一条只在测试里存在的路径"。
    struct FakeInterlockApi(mupc_display_proto::interlock::InterlockView);

    #[async_trait::async_trait]
    impl mupc_display_proto::InterlockApi for FakeInterlockApi {
        async fn status(&self) -> mupc_display_proto::interlock::InterlockView {
            self.0.clone()
        }
        async fn request_release(
            &self,
        ) -> Result<(), mupc_display_proto::InterlockReject> {
            Ok(())
        }
        async fn ack_m1(&self) -> Result<(), mupc_display_proto::InterlockReject> {
            Ok(())
        }
    }

    // ── 通用辅助 ──

    /// 带节拍参数的配置（测试用小周期，避免用例真等秒级）。
    fn cfg_slow(publish_ms: u64, min_window_ms: u64, poll_ms: u64) -> DisplayConfig {
        DisplayConfig {
            publish_ms,
            min_publish_interval_ms: min_window_ms,
            device_poll_ms: poll_ms,
            alarm_poll_ms: poll_ms,
            interlock_poll_ms: poll_ms,
            ..Default::default()
        }
    }

    /// 空载 provider（三相/run_state 全缺，四段未接线）。
    fn bare_provider(c: &DisplayConfig) -> DisplayDataProvider {
        DisplayDataProvider::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            stub_client(None, true, None),
            c,
            true,
            Arc::new(Mutex::new(None)),
        )
    }

    /// 轮询 `latest` 直到有帧或超时（真实时钟；仅用于 run 级联调用例）。
    async fn wait_for_frame(latest: &SharedLatest, within: Duration) -> Option<DisplayFrame> {
        let deadline = std::time::Instant::now() + within;
        while std::time::Instant::now() < deadline {
            if let Some(f) = latest.lock().unwrap().clone() {
                return Some(f);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        latest.lock().unwrap().clone()
    }

    fn alarm(msg: &str) -> AlarmItem {
        AlarmItem {
            ts_ms: 1,
            level: AlarmLevel::Info,
            message: msg.to_string(),
        }
    }

    /// `Iec104Server::start()` 需要一个 `CommandHandler`；本模块只验链路状态、不验命令，
    /// 故给最小桩（收到命令即回失败——测试中不会有真连接发命令）。
    struct StubCommandHandler;

    #[async_trait::async_trait]
    impl mupc_gateway::iec104::command::CommandHandler for StubCommandHandler {
        async fn handle_command(
            &self,
            cmd: mupc_gateway::iec104::command::ControlCommand,
        ) -> Result<mupc_gateway::iec104::command::CommandResponse, mupc_common::MupcError> {
            Ok(mupc_gateway::iec104::command::CommandResponse {
                cmd_id: cmd.cmd_id,
                success: false,
                message: "display_host 测试桩".to_string(),
                timestamp: 0,
            })
        }

        fn name(&self) -> &str {
            "stub"
        }
    }

    // ── A 装置段（F6）──

    /// 装置源：intercore 在线 → `Connected` / 离线 → `Disconnected`；**`hmi_channel` 恒
    /// `Unknown`**（设计 §5.5：由 HMI 本地覆盖，服务端不得自称已知——否则是"由服务端报告
    /// 客户端自己的连接状态"的语义倒置）；`iec104` 未装配服务器 ⇒ `NotConfigured`
    /// （U-59 / L-5 补齐后的语义：真源已存在，**未接线**即「未配置」，仍不臆造）。
    #[tokio::test]
    async fn device_source_links_and_hmi_channel_never_faked() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        let online = SystemDeviceSource::new(
            stub_client(None, true, None),
            ai.clone(),
            None,
            Instant::now(),
        );
        let d = online.read_device().await;
        assert_eq!(d.intercore, LinkState::Connected);
        assert_eq!(
            d.hmi_channel,
            LinkState::Unknown,
            "hmi_channel 必须 Unknown（HMI 侧本地覆盖）；服务端自称 Connected 即为语义倒置"
        );
        assert_eq!(
            d.iec104,
            LinkState::NotConfigured,
            "未装配 Iec104Server ⇒「未配置」（不得是 Unknown/Connected）"
        );
        assert!(d.uptime_secs.is_some(), "uptime 真源存在（零点由调用方传入）");
        assert_ne!(d.hmi_channel, LinkState::Connected, "缺省/不可得绝不落在已连接");

        let offline =
            SystemDeviceSource::new(stub_client(None, false, None), ai, None, Instant::now());
        assert_eq!(offline.read_device().await.intercore, LinkState::Disconnected);
    }

    /// **IEC 104 链路状态接线**（U-59 / L-5）：装配了服务器即问它，枚举 1:1 映射，
    /// 未启动的服务器报「未配置」而非「断开」。
    #[tokio::test]
    async fn device_source_iec104_follows_server_link_state() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        let server = Arc::new(mupc_gateway::iec104::server::Iec104Server::new(
            mupc_gateway::iec104::server::Iec104Config {
                listen_addr: "127.0.0.1".to_string(),
                listen_port: 0,
                ..Default::default()
            },
        ));
        let src = SystemDeviceSource::new(
            stub_client(None, true, None),
            ai,
            Some(server.clone()),
            Instant::now(),
        );
        assert_eq!(
            src.read_device().await.iec104,
            LinkState::NotConfigured,
            "服务器尚未 start() ⇒「未配置」"
        );

        src.read_device().await;
        assert_eq!(
            map_iec104_link_state(server.link_state().await),
            LinkState::NotConfigured
        );

        // 启动（回环 + 临时端口：测试不占固定端口）⇒ 无连接 =「断开」
        let handler = Arc::new(StubCommandHandler);
        server
            .start(handler)
            .await
            .expect("bind 127.0.0.1:0 必成功");
        assert_eq!(
            src.read_device().await.iec104,
            LinkState::Disconnected,
            "已启动且无连接 ⇒「断开」"
        );
    }

    /// 映射函数**逐变体**自证（防"少映射一个变体"这类静默偏差）。
    #[test]
    fn iec104_link_state_mapping_is_total() {
        use mupc_gateway::iec104::server::LinkState as Gw;
        for (gw, expect) in [
            (Gw::Connected, LinkState::Connected),
            (Gw::Connecting, LinkState::Connecting),
            (Gw::Disconnected, LinkState::Disconnected),
            (Gw::NotConfigured, LinkState::NotConfigured),
        ] {
            assert_eq!(map_iec104_link_state(gw), expect, "{gw:?} 映射错误");
        }
    }

    /// **uptime 零点必须来自调用方传入的进程起点，而不是本结构体的构造时刻**（设计 §4.1
    /// 明写「以 `mupcd` 进程启动时刻为准」）。
    ///
    /// 生产零点取自 `main()` 入口最顶部，而构造点落在 `initialize_all` 完成 DB / intercore /
    /// gateway / AI / security 全部装配**之后** ⇒ 若以构造时刻为零点，屏上 uptime 会**系统性
    /// 偏小**（偏差 = 启动装配耗时，屏上不可察觉、无任何证据支撑"约 1 s 内"的说法）。
    ///
    /// **改什么会让本条变红**：把 `Self { started_at, .. }` 写回 `started_at: Instant::now()`
    /// （即忽略传入零点）⇒ 传入 1 h 前的人工零点后 `uptime_secs` 会退化为 ~0 < 3600 ⇒ 红。
    #[tokio::test]
    async fn device_source_uptime_uses_injected_process_start_not_ctor_time() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        // 人工零点：1 小时前（模拟"进程已启动 1 h"）。绝不依赖真实进程起点。
        let zero = Instant::now() - Duration::from_secs(3600);
        let src = SystemDeviceSource::new(stub_client(None, true, None), ai, None, zero);
        let up = src.read_device().await.uptime_secs.expect("uptime 真源存在");
        assert!(
            (3600..3600 + 60).contains(&up),
            "uptime 必须按**传入零点**计（1 h 前 ⇒ ≈3600 s）；按构造时刻计会得到 ~0。实际 {up}"
        );
    }

    /// 控制源：本地优先已置位 ⇒ `LocalStrategy`；否则 AI 引擎未加载 ⇒ `AiDisabled`；
    /// 两者皆不成立 ⇒ `Unknown`（**不臆造**下发源）。
    #[tokio::test]
    async fn device_source_control_source_never_invents() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        let src = SystemDeviceSource::new(
            stub_client(None, true, None),
            ai.clone(),
            None,
            Instant::now(),
        );
        // 默认：local_priority=false 且 ModelStatus::Unloaded（AI 停用期实态）
        assert_eq!(
            src.read_device().await.control_source,
            ControlSource::AiDisabled
        );
        ai.set_local_priority(true).await;
        assert_eq!(
            src.read_device().await.control_source,
            ControlSource::LocalStrategy
        );
    }

    /// 非 Linux 目标：**不采信** system-monitor 的硬编码桩值（温度 48.0℃ / 内存 8192MB-50%，
    /// `crates/system-monitor/src/collectors.rs:186-196/291-299`）⇒ `None`（屏显「未知」）。
    /// 反过来说：本用例若变红，说明有人把桩值直接采信上屏了。
    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn device_source_no_stub_metrics_on_non_linux() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        let d = SystemDeviceSource::new(stub_client(None, true, None), ai, None, Instant::now())
            .read_device()
            .await;
        assert_eq!(d.cpu_temp_c, None, "非 Linux 无真温度源 ⇒「未知」，不得上桩值");
        assert_eq!(d.mem_used_pct, None, "非 Linux 无真内存源 ⇒「未知」，不得上桩值");
    }

    // ── P6 装置信息（F8，来源分列）──

    /// 版本/编译时间取**编译期真源**；序列号**无真源恒 `None`**（不臆造）；`service_scope`
    /// 恒 `LoopbackOnly`（与 `mgmt_ipv4` 分列，EDGE-24）；型号只在设备树存在时给值。
    #[test]
    fn info_section_sources_are_explicit() {
        let info = collect_info();
        assert_eq!(info.firmware_version, env!("CARGO_PKG_VERSION"));
        assert!(
            info.build_time.is_some(),
            "build.rs 已注入 BUILD_TIMESTAMP_EPOCH ⇒ 编译时间应可得（取不到即「未提供」属降级，不是预期）"
        );
        assert_eq!(info.serial, None, "序列号无可靠真源 ⇒「未提供」，不得臆造");
        assert_eq!(info.service_scope, ServiceScope::LoopbackOnly);
        assert_eq!(
            info.model.is_some(),
            std::path::Path::new("/proc/device-tree/model").exists(),
            "型号只应来自设备树（存在才给值）"
        );
        if let Some(ip) = &info.mgmt_ipv4 {
            assert!(
                ip.parse::<std::net::Ipv4Addr>().is_ok_and(|a| !a.is_loopback()),
                "管理 IP 不得是回环地址: {ip}"
            );
        }
    }

    /// 编译时间戳：必须能被 `chrono` 解析为 RFC3339（不是任意字符串），且可复现
    /// （`SOURCE_DATE_EPOCH` 优先由 build.rs 落实）。
    #[test]
    fn build_time_is_rfc3339() {
        let ts = build_time_rfc3339().expect("build.rs 注入的 BUILD_TIMESTAMP_EPOCH 应可解析");
        assert!(
            chrono::DateTime::parse_from_rfc3339(&ts).is_ok(),
            "编译时间戳须为 RFC3339：{ts}"
        );
    }

    /// `info.build_time` 的**「不可得 ⇒ `None`」**口径（EDGE-16）：**绝不**补 0 伪装成
    /// 1970-01-01 这一**合法**值上屏（「显 `--`、严禁补 0」）。
    ///
    /// **改什么会让本条变红**：把 [`build_time_from_raw`] 的 `raw?` / `.parse().ok()?` 换成
    /// `.unwrap_or(0)` 一类补 0 写法（本单元整改前的 `build.rs` 形态）⇒ 前三条 `None` 断言
    /// 立即红。合法时间戳一条同时钉死「可解析的输入仍须正常格式化」（不是一律返回 `None`）。
    #[test]
    fn build_time_none_when_epoch_absent_or_malformed() {
        assert_eq!(
            build_time_from_raw(None),
            None,
            "变量缺失 ⇒ 「未提供」，**不得**补 0 伪装成 1970-01-01"
        );
        assert_eq!(
            build_time_from_raw(Some("not-a-number")),
            None,
            "非数字 ⇒ 「未提供」（不得补 0）"
        );
        assert_eq!(
            build_time_from_raw(Some("   ")),
            None,
            "空白 ⇒ 「未提供」（不得补 0）"
        );
        assert_eq!(
            build_time_from_raw(Some("1757412000")).as_deref(),
            Some("2025-09-09T10:00:00+00:00"),
            "合法秒级时间戳 ⇒ 正常格式化为 RFC3339（可复现）"
        );
    }

    /// 管理 IP：要么是**非回环** IPv4，要么 `None`（「未提供」）——绝不臆造、绝不给回环。
    #[test]
    fn mgmt_ipv4_is_non_loopback_or_absent() {
        if let Some(ip) = primary_ipv4() {
            assert!(!ip.is_loopback(), "管理 IP 不得为回环地址: {ip}");
        }
    }

    /// 事件类型 → 级别的显式规则（设计未定义映射，故本表即实现依据）。
    #[test]
    fn alarm_level_mapping_is_explicit() {
        assert_eq!(alarm_level_of("south_station.s1.offline"), AlarmLevel::Error);
        assert_eq!(alarm_level_of("interlock.triggered"), AlarmLevel::Error);
        assert_eq!(alarm_level_of("interlock.stop_failed"), AlarmLevel::Error);
        assert_eq!(alarm_level_of("interlock.cleared"), AlarmLevel::Info);
        assert_eq!(alarm_level_of("south_station.s1.online"), AlarmLevel::Info);
        assert_eq!(alarm_level_of("interlock.stopped"), AlarmLevel::Info);
        // 未知类型兜底 Warn（既不静默降为 INFO，也不冒称 ERROR）
        assert_eq!(alarm_level_of("some.new.type"), AlarmLevel::Warn);
    }

    // ── B 告警段（F7）──

    /// `storage.events` → 时间**倒序** + 截断到 `alarm_page_size`（插库顺序为升序，若采集层不
    /// 自己排序，本用例会取到最旧的 10 条而变红）。
    #[tokio::test]
    async fn alarm_source_newest_first_and_capped() {
        let repo = Arc::new(FakeEventRepo::default());
        let now = chrono::Utc::now();
        for i in 0..15 {
            repo.with_event(
                now - chrono::Duration::seconds(15 - i),
                "south_station.s1.offline",
                &format!("事件{i}"),
            );
        }
        let src = StorageAlarmSource::new(repo, 10);
        let items = src.read_alarms().await.unwrap();
        assert_eq!(items.len(), 10, "须截断到 alarm_page_size");
        assert_eq!(items[0].message, "事件14", "首条须是最新（时间倒序）");
        assert!(
            items.windows(2).all(|w| w[0].ts_ms >= w[1].ts_ms),
            "必须时间倒序: {:?}",
            items.iter().map(|i| i.ts_ms).collect::<Vec<_>>()
        );
        assert_eq!(items[0].level, AlarmLevel::Error, "offline → ERROR");
        assert_eq!(
            items[0].ts_ms,
            (now - chrono::Duration::seconds(1)).timestamp_millis() as u64,
            "ts_ms 取**事件时间**（非采集时间）"
        );
    }

    /// 回溯窗口外的旧事件不上屏（有界化落地；窗口内为空 ⇒ `available` 仍为 `true` = 真「无告警」）。
    #[tokio::test]
    async fn alarm_source_respects_lookback_window() {
        let repo = Arc::new(FakeEventRepo::default());
        let now = chrono::Utc::now();
        repo.with_event(now - chrono::Duration::days(30), "old.event", "很久以前");
        repo.with_event(now - chrono::Duration::minutes(1), "new.event", "刚刚");
        let items = StorageAlarmSource::new(repo, 10).read_alarms().await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].message, "刚刚");
    }

    /// **本单元最重要的语义网之一**：告警**源不可用 ≠ 无告警**。
    /// 查询失败 ⇒ `available=false` + 空列表（屏显「告警源不可用」）；真 0 条 ⇒ `available=true`
    /// + 空列表（屏显「无告警」）。二者**同形不同义**，只能靠 `available` 区分（EDGE-09）。
    #[tokio::test]
    async fn alarm_source_failure_is_unavailable_not_empty() {
        let failing = StubAlarms::failing("db 挂了");
        let empty = StubAlarms::ok(Vec::new());

        let unavail = DisplayDataProvider::sample_alarms(&failing, 10).await;
        let none = DisplayDataProvider::sample_alarms(&empty, 10).await;

        assert!(!unavail.available, "读失败 ⇒ 源不可用");
        assert!(unavail.items.is_empty());
        assert!(none.available, "真 0 条 ⇒ 源可用（「无告警」）");
        assert!(none.items.is_empty(), "两者 items 同为空 ⇒ 唯有 available 能区分语义");
        assert_ne!(unavail.available, none.available);
    }

    /// 组帧级：未接线（无告警源）时帧内 `alarms.available=false`，**绝不**退化成「无告警」。
    #[tokio::test]
    async fn frame_alarms_unwired_is_unavailable_not_empty() {
        let mut p = bare_provider(&cfg());
        let f = p.sample_once().await;
        assert!(!f.alarms.available, "未接线 ⇒「告警源不可用」，不是「无告警」");
        assert!(f.alarms.items.is_empty());
        assert_eq!(f.alarms.ts_ms, 0, "未采集 ⇒ ts_ms=0（契约：0 = 未采集）");
    }

    /// 源返回超过上限的条数 ⇒ 组帧侧兜底截断，且**不动 `available`**（EDGE-09 的另一半：
    /// 「条目被裁到上限」也不是「源不可用」）。
    #[tokio::test]
    async fn alarm_sampler_caps_items_without_touching_available() {
        let src = StubAlarms::ok((0..15).map(|i| alarm(&format!("a{i}"))).collect());
        let sec = DisplayDataProvider::sample_alarms(&src, 10).await;
        assert_eq!(sec.items.len(), 10);
        assert!(sec.available, "截断不得改动 available");
    }

    /// **超长告警消息不得把整帧顶废**（本单元**重要 2** 的整改网）。
    ///
    /// ① ingest 侧（[`truncate_alarm_message`]）把消息截到 [`MAX_ALARM_MESSAGE_BYTES`] 内且
    /// **带可见截断标记**（ASCII `...`，见 [`ALARM_TRUNCATION_MARK`]——`…` U+2026 不在生成
    /// 字体 cmap 内，屏上是豆腐块）⇒ ② 发布**成功**（不再 500）⇒ ③ 帧仍可被 HMI 侧解码
    /// （`serde_json` 往返；HMI `channel.rs:450` 走的就是裸 `serde_json::from_slice`）。
    ///
    /// **改什么会让本条变红**：把 `alarm_item_of` 里的 `truncate_alarm_message` 去掉 ⇒ 单条
    /// 2 KiB 消息原样进帧 ⇒ ①（`len ≤ 上限`）与 `to_json_slice()` 双双失败 ⇒ 红。
    #[tokio::test]
    async fn oversized_alarm_message_truncated_in_ingest_frame_still_publishes() {
        let repo = Arc::new(FakeEventRepo::default());
        // 2 KiB 纯 ASCII 消息（远超 MAX_ALARM_MESSAGE_BYTES = 1 KiB）
        let long = "x".repeat(2 * mupc_display_proto::MAX_ALARM_MESSAGE_BYTES);
        repo.with_event(chrono::Utc::now(), "south_station.s1.offline", &long);

        // ① ingest 侧：经 StorageAlarmSource（`alarm_item_of` 的唯一生产入口）截断 + 带标记
        let src = StorageAlarmSource::new(repo, 10);
        let items = src.read_alarms().await.unwrap();
        assert_eq!(items.len(), 1);
        let msg = &items[0].message;
        assert!(
            msg.len() <= mupc_display_proto::MAX_ALARM_MESSAGE_BYTES,
            "ingest 侧须把消息截到上限内（实际 {} B）",
            msg.len()
        );
        assert!(
            msg.ends_with(ALARM_TRUNCATION_MARK),
            "截断必须**可见**（尾部标记 `{ALARM_TRUNCATION_MARK}`），不得静默丢字：{msg:?}"
        );
        assert_eq!(
            msg.len(),
            mupc_display_proto::MAX_ALARM_MESSAGE_BYTES,
            "应恰好填满上限（标记已计入预算）"
        );
        assert_eq!(items[0].level, AlarmLevel::Error, "截断不得改动级别映射");

        // ② 发布侧：把该段放进真帧 ⇒ 编码**成功**（不再 500）
        let mut frame = sample_frame(Some(50.0));
        frame.alarms = AlarmsSection {
            ts_ms: 1,
            available: true,
            items: items.clone(),
        };
        let body = frame
            .to_json_slice()
            .expect("ingest 已截断 ⇒ 整帧必须恒可编码（不再 500）");

        // ②' 同一帧真过一遍回环发布端：应答必须是 200（`handle` 的 500/200 判据正是
        //     `to_json_slice` 的成败）
        let latest: SharedLatest = Arc::new(Mutex::new(Some(frame.clone())));
        let publisher = LoopbackHttpPublisher::new(latest);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve = tokio::spawn(publisher.serve(listener));
        let req = format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", LATEST_PATH);
        let resp = connect_and_get(addr, &req).await;
        let text = String::from_utf8_lossy(&resp);
        assert!(
            text.starts_with("HTTP/1.1 200"),
            "ingest 已截断 ⇒ 必须 200（不再整帧 500）：{text:?}"
        );
        serve.abort();

        // ③ 往返：裸 `serde_json`（HMI 侧同款解码路径）与契约严格解码器**都能解出**
        let back: DisplayFrame = serde_json::from_slice(&body).expect("HMI 裸 serde_json 须可解");
        assert_eq!(back.alarms.items[0].message, items[0].message);
        let strict = DisplayFrame::from_json_slice(&body).expect("契约严格解码器须可解");
        assert_eq!(strict.alarms.items[0].message, items[0].message);
    }

    /// 截断按**字符边界**回退，绝不切断多字节 UTF-8（中文消息超限时不得 panic / 出乱码）。
    #[test]
    fn alarm_message_truncation_is_char_boundary_safe() {
        let long = "台区".repeat(mupc_display_proto::MAX_ALARM_MESSAGE_BYTES); // 每字 3 B
        let out = truncate_alarm_message(&long);
        assert!(out.len() <= mupc_display_proto::MAX_ALARM_MESSAGE_BYTES);
        assert!(out.ends_with(ALARM_TRUNCATION_MARK));
        let kept = &out[..out.len() - ALARM_TRUNCATION_MARK.len()];
        assert!(kept.chars().all(|c| c == '台' || c == '区'), "不得截出半个汉字");
        // 恰好等于上限 ⇒ 不动
        let exact = "x".repeat(mupc_display_proto::MAX_ALARM_MESSAGE_BYTES);
        assert_eq!(truncate_alarm_message(&exact), exact);
    }

    // ── C 联锁段（F16）──

    /// 契约 `InterlockView` → 帧内联锁段：**逐字段**（含 `release_hold_secs` 来自 io 配置、
    /// `ts_ms` 换成采集时刻）。这是本层唯一做的两处改写，其余必须逐字透传。
    #[tokio::test]
    async fn interlock_status_maps_every_field() {
        let api: Arc<dyn mupc_display_proto::InterlockApi> =
            Arc::new(FakeInterlockApi(InterlockSection {
                ts_ms: 777, // 控制器的取数时刻：**必须**被本层按采集时刻覆写（见下）
                available: true,
                enabled: true,
                latched: true,
                stop_failed: true,
                sources: vec![InterlockSourceItem {
                    name: "estop".into(),
                    tripped: true,
                }],
                fault_lamp: Some(true),
                run_lamp: Some(false),
                release_hold_secs: 999, // 控制器自报值：**必须**被注入值覆写
            }));
        let src = InterlockApiSource::new(api, 30);
        let sec = src.read_interlock().await.unwrap();
        assert!(sec.available, "契约视图的 available 如实透传（不再硬编码 true）");
        assert!(sec.enabled && sec.latched && sec.stop_failed);
        assert_eq!(sec.release_hold_secs, 30, "来自 io.release_hold_secs（覆写控制器自报的 999）");
        assert_eq!(
            sec.sources,
            vec![InterlockSourceItem {
                name: "estop".into(),
                tripped: true
            }]
        );
        assert_eq!(sec.fault_lamp, Some(true));
        assert_eq!(sec.run_lamp, Some(false));
        assert!(sec.ts_ms > 0, "ts_ms 为采集时刻");
        assert_ne!(sec.ts_ms, 777, "必须换成本层采集时刻，不得沿用控制器自报时刻");
    }

    /// **单元 K 交接项**：灯态 `None`（"未知"）必须**如实上屏**，不得被吞成"灯灭"。
    ///
    /// 迁出前的 web-api 适配器把 `Option<bool>` 强制 `unwrap_or(false)`（旧 DTO 无"未知"槽），
    /// 并在注释里明确要求"单元 K 把读通道换到契约版真源时一并迁移"——本条就是那张网。
    ///
    /// **改什么会让本条变红**：把 `interlock_section_of` 的灯位改回 `Some(view.fault_lamp
    /// .unwrap_or(false))` ⇒ 「未知」被谎报成「灭」，第 1、2 条断言红。
    #[tokio::test]
    async fn interlock_lamp_unknown_is_passed_through_not_flattened_to_false() {
        let api: Arc<dyn mupc_display_proto::InterlockApi> =
            Arc::new(FakeInterlockApi(InterlockSection {
                available: true,
                enabled: true,
                latched: true,
                fault_lamp: None,
                run_lamp: None,
                ..Default::default()
            }));
        let sec = InterlockApiSource::new(api, 30).read_interlock().await.unwrap();
        assert_eq!(sec.fault_lamp, None, "「灯未知」必须如实透传，不得臆造为灭");
        assert_eq!(sec.run_lamp, None, "同上");
        assert_ne!(sec.fault_lamp, Some(false), "未知 ≠ 灭（IL-01.6 同族：语义不得互替）");
    }

    /// **本单元最重要的语义网之二**：联锁**状态不可用 ≠ 未联锁**。
    /// 读取失败 ⇒ `available=false`（屏显「联锁状态不可用」），`latched` 同为 false；真「未联锁」
    /// ⇒ `available=true` + `latched=false`。二者**同形不同义**（IL-01.6 / EDGE-12）。
    #[tokio::test]
    async fn interlock_failure_is_unavailable_not_unlatched() {
        let failing = StubInterlock::failing("io 句柄丢失");
        let unlatched = StubInterlock::ok(InterlockSection {
            ts_ms: 1,
            available: true,
            enabled: true,
            latched: false,
            ..Default::default()
        });

        let unavail = DisplayDataProvider::sample_interlock(&failing).await;
        let ok = DisplayDataProvider::sample_interlock(&unlatched).await;

        assert!(!unavail.available, "读失败 ⇒ 状态不可用");
        assert!(!unavail.latched);
        assert!(ok.available, "真「未联锁」⇒ 源可用");
        assert!(!ok.latched);
        assert_ne!(
            unavail.available, ok.available,
            "同为 latched=false，唯有 available 能区分「不可用」与「未联锁」"
        );
    }

    /// `io.enabled=false` ⇒ **已知状态**「联锁功能未启用」：`available=true, enabled=false`
    /// （设计 §4.2 表 C 行）。**不得**写成 `available=false`（那是「源不可用」，语义不同），
    /// 也**不得**写成 `available=true, enabled=true, latched=false`（那是「未联锁」）。
    #[tokio::test]
    async fn interlock_disabled_is_known_state_not_unavailable() {
        let mut p = bare_provider(&cfg()).with_slow_sources(
            None,
            None,
            InterlockWiring::Disabled,
        );
        let f = p.sample_once().await;
        assert!(f.interlock.available, "「功能未启用」是已知状态，不是「不可用」");
        assert!(!f.interlock.enabled);
        assert!(!f.interlock.latched);
        assert_eq!(f.interlock.sources, Vec::<InterlockSourceItem>::new());
        assert_eq!(f.interlock.fault_lamp, None, "未启用时灯态未知 → None，不臆造");
    }

    /// 装配点三态判定（[`interlock_wiring_for`]）：**三种「非已启用」语义必须互斥**。
    ///
    /// 重点钉 `(None, true)`（`io.enabled=true` 却拿不到控制器）⇒ 必须是
    /// [`InterlockWiring::Unwired`]（屏显「**联锁状态不可用**」），**不得**落到 `Disabled`
    /// （那会把"该有却没有"谎报成"本来就没开"）。
    ///
    /// **改什么会让本条变红**：把 `(None, true)` 合回 `_ => Disabled` ⇒ 第 2 条断言立即红。
    #[tokio::test]
    async fn interlock_wiring_three_way_exclusive() {
        let api: Arc<dyn mupc_display_proto::InterlockApi> =
            Arc::new(FakeInterlockApi(InterlockSection {
                available: true,
                enabled: true,
                latched: false,
                stop_failed: false,
                sources: Vec::new(),
                fault_lamp: Some(false),
                run_lamp: Some(false),
                ..Default::default()
            }));
        assert!(
            matches!(
                interlock_wiring_for(Some(api.clone()), true, 30),
                InterlockWiring::Wired(_)
            ),
            "io.enabled=true 且有控制器 ⇒ 已接线"
        );
        assert!(
            matches!(
                interlock_wiring_for(None, true, 30),
                InterlockWiring::Unwired
            ),
            "io.enabled=true 却没控制器 ⇒ **不可用**，不得谎报成「功能未启用」"
        );
        assert!(
            matches!(
                interlock_wiring_for(None, false, 30),
                InterlockWiring::Disabled
            ),
            "io.enabled=false ⇒ 已知状态「功能未启用」"
        );
        assert!(
            matches!(
                interlock_wiring_for(Some(api), false, 30),
                InterlockWiring::Disabled
            ),
            "io.enabled=false 时即便有控制器也按「功能未启用」处置"
        );
    }

    /// 未接线 ⇒ `available=false`（「联锁状态不可用」），**绝不**退化成「未联锁」。
    #[tokio::test]
    async fn interlock_unwired_is_unavailable_not_unlatched() {
        let mut p = bare_provider(&cfg());
        let f = p.sample_once().await;
        assert!(!f.interlock.available, "未接线 ⇒「联锁状态不可用」，不是「未联锁」");
        assert!(!f.interlock.latched);
        assert_eq!(f.interlock.ts_ms, 0, "未采集 ⇒ ts_ms=0");
    }

    // ── 帧路径零 I/O（设计 D6 / §2.1 不变量）──

    /// **帧路径零 I/O 之网**：`sample_once`/`build_frame` 只读缓存，**不触碰任何慢源**
    /// （桩源的调用计数在多次组帧后仍为 0）；只有慢拍任务驱动一次才 +1。
    /// 若有人把源读取塞回 `build_frame`，计数立刻 >0 ⇒ 变红。
    #[tokio::test]
    async fn frame_reads_caches_only_never_touches_slow_sources() {
        let dev = Arc::new(StubDevice::new(DeviceSection {
            ts_ms: 1,
            uptime_secs: Some(42),
            ..Default::default()
        }));
        let alm = Arc::new(StubAlarms::ok(vec![alarm("a")]));
        let ilk = Arc::new(StubInterlock::ok(InterlockSection {
            ts_ms: 1,
            available: true,
            enabled: true,
            latched: true,
            ..Default::default()
        }));
        let mut p = bare_provider(&cfg()).with_slow_sources(
            Some(dev.clone()),
            Some(alm.clone()),
            InterlockWiring::Wired(ilk.clone()),
        );

        // 组帧多次：三段源调用计数必须全为 0
        for _ in 0..5 {
            let _ = p.sample_once().await;
        }
        assert_eq!(dev.count(), 0, "帧路径不得触碰装置源");
        assert_eq!(alm.count(), 0, "帧路径不得触碰告警源（DB 查询）");
        assert_eq!(ilk.count(), 0, "帧路径不得触碰联锁源");

        // 未采集 ⇒ 四段仍是「不可用」缺省（不是「无告警」「未联锁」）
        let f = p.sample_once().await;
        assert!(f.device.uptime_secs.is_none() && !f.alarms.available && !f.interlock.available);

        // 慢拍驱动一次 ⇒ 计数 +1 且帧内容随之变化
        DisplayDataProvider::slow_tick(
            &p.caches.device,
            &p.notify,
            dev.read_device(),
            device_changed,
        )
        .await;
        DisplayDataProvider::slow_tick(
            &p.caches.alarms,
            &p.notify,
            async { DisplayDataProvider::sample_alarms(alm.as_ref(), 10).await },
            alarms_changed,
        )
        .await;
        DisplayDataProvider::slow_tick(
            &p.caches.interlock,
            &p.notify,
            async { DisplayDataProvider::sample_interlock(ilk.as_ref()).await },
            interlock_changed,
        )
        .await;
        let f = p.sample_once().await;
        assert_eq!(f.device.uptime_secs, Some(42));
        assert_eq!(f.alarms.items.len(), 1);
        assert!(f.alarms.available && f.interlock.available && f.interlock.latched);
        assert_eq!((dev.count(), alm.count(), ilk.count()), (1, 1, 1));
    }

    /// 慢源**不阻塞**帧路径：装置源单次读要 2 s，主拍 50 ms —— 首帧仍应在数百 ms 内发布，
    /// 且该帧的装置段如实为「未采集」（`uptime_secs=None`），**不是**凭空补值。
    /// 若 `build_frame` 去 await 慢源，首帧会被推后到 2 s ⇒ 变红。
    #[tokio::test]
    async fn frame_path_not_blocked_by_slow_source() {
        let latest: SharedLatest = Arc::new(Mutex::new(None));
        let slow = Arc::new(StubDevice::slow(
            DeviceSection {
                ts_ms: 1,
                uptime_secs: Some(7),
                ..Default::default()
            },
            Duration::from_secs(2),
        ));
        let provider = DisplayDataProvider::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            stub_client(None, true, None),
            &cfg_slow(50, 250, 50),
            true,
            latest.clone(),
        )
        .with_slow_sources(
            Some(slow.clone()),
            None,
            InterlockWiring::Unwired,
        );
        let h = tokio::spawn(provider.run());

        let f = wait_for_frame(&latest, Duration::from_millis(500))
            .await
            .expect("慢源未归 ⇒ 主拍仍须按时发布帧（帧路径不得被慢源阻塞）");
        assert_eq!(
            f.device.uptime_secs, None,
            "首帧在慢源返回前发布 ⇒ 装置段如实「未采集」，不得补值"
        );
        assert_eq!(f.alarms.ts_ms, 0);
        h.abort();
    }

    // ── 节拍：假时钟推进（设计 §4.2.1 验证要求：不 sleep）──

    /// 内容变更判据必须覆盖**每个**字段（漏一个 ⇒ 该字段变化不上屏）。同时：**`ts_ms` 不算
    /// 内容**（每拍必变，若计入则慢拍退化为恒定 4 Hz 唤醒组帧）。
    #[test]
    fn content_change_predicates_cover_every_field() {
        // 装置段：7 个展示字段
        let dev = DeviceSection::default();
        let dev_probes = vec![
            DeviceSection {
                uptime_secs: Some(1),
                ..dev.clone()
            },
            DeviceSection {
                cpu_temp_c: Some(40.0),
                ..dev.clone()
            },
            DeviceSection {
                mem_used_pct: Some(30.0),
                ..dev.clone()
            },
            DeviceSection {
                iec104: LinkState::Connected,
                ..dev.clone()
            },
            DeviceSection {
                intercore: LinkState::Connected,
                ..dev.clone()
            },
            DeviceSection {
                hmi_channel: LinkState::Connected,
                ..dev.clone()
            },
            DeviceSection {
                control_source: ControlSource::LocalStrategy,
                ..dev.clone()
            },
        ];
        assert_eq!(
            dev_probes.len(),
            7,
            "装置段字段数变化必须同步本用例（否则新字段的变化不会触发组帧）"
        );
        for p in &dev_probes {
            assert!(device_changed(&dev, p), "装置段字段变化未被捕获: {p:?}");
        }
        let mut ts_only = dev.clone();
        ts_only.ts_ms = 999;
        assert!(
            !device_changed(&dev, &ts_only),
            "ts_ms 每拍必变，不得计入内容变更（否则退化为恒定 4 Hz 组帧）"
        );

        // 告警段：available + items
        let al = AlarmsSection::default();
        let al_avail = AlarmsSection {
            available: true,
            ..al.clone()
        };
        let al_items = AlarmsSection {
            available: true,
            items: vec![alarm("x")],
            ..al.clone()
        };
        assert!(alarms_changed(&al, &al_avail));
        assert!(alarms_changed(&al_avail, &al_items));
        let mut al_ts = al.clone();
        al_ts.ts_ms = 999;
        assert!(!alarms_changed(&al, &al_ts));

        // 联锁段：8 个展示字段
        let il = InterlockSection::default();
        let il_probes = vec![
            InterlockSection {
                available: true,
                ..il.clone()
            },
            InterlockSection {
                enabled: true,
                ..il.clone()
            },
            InterlockSection {
                latched: true,
                ..il.clone()
            },
            InterlockSection {
                stop_failed: true,
                ..il.clone()
            },
            InterlockSection {
                sources: vec![InterlockSourceItem {
                    name: "estop".into(),
                    tripped: true,
                }],
                ..il.clone()
            },
            InterlockSection {
                fault_lamp: Some(true),
                ..il.clone()
            },
            InterlockSection {
                run_lamp: Some(true),
                ..il.clone()
            },
            InterlockSection {
                release_hold_secs: 30,
                ..il.clone()
            },
        ];
        assert_eq!(
            il_probes.len(),
            8,
            "联锁段字段数变化必须同步本用例（否则新字段的变化不会触发组帧）"
        );
        for p in &il_probes {
            assert!(interlock_changed(&il, p), "联锁段字段变化未被捕获: {p:?}");
        }
        let mut il_ts = il.clone();
        il_ts.ts_ms = 999;
        assert!(!interlock_changed(&il, &il_ts));
    }

    /// 主拍：无变更时按 `publish_ms` 发布；变更在**合并窗口后立即**发布（不等主拍）——
    /// 这是 F7.3/F16.5「≤2 s 上屏」的达成机制（设计 §4.2.1 约束 4）。
    #[test]
    fn pacer_publishes_change_after_merge_window_not_waiting_main_tick() {
        let t0 = Instant::now();
        // 主拍 2000ms、合并窗口 250ms
        let mut p = PublishPacer::new(t0, 2000, 250);

        // 无变更：250ms 不该发布
        assert!(!p.on_wake(t0 + Duration::from_millis(250)));
        // 变更：窗口未到（+100ms）不发布，next_deadline 指向窗口到期的 t0+250
        p.on_change();
        assert_eq!(p.next_deadline(), t0 + Duration::from_millis(250));
        assert!(!p.on_wake(t0 + Duration::from_millis(100)));
        // 窗口到期 ⇒ 立即发布（远早于 2000ms 主拍）
        assert!(p.on_wake(t0 + Duration::from_millis(250)));
        // 发布后主拍顺延为 250+2000
        assert_eq!(p.next_deadline(), t0 + Duration::from_millis(2250));
        assert!(!p.on_wake(t0 + Duration::from_millis(2000)));
        assert!(p.on_wake(t0 + Duration::from_millis(2250)));
    }

    /// 突发变更被合并：发布率上界 = `max(1 Hz 主拍, 1/合并窗口)`，慢源抖动打不爆读通道
    /// （设计 §4.2.1 约束 2）。
    #[test]
    fn pacer_merges_burst_and_bounds_publish_rate() {
        let t0 = Instant::now();
        let (publish, window) = (1000u64, 250u64);
        let mut p = PublishPacer::new(t0, publish, window);
        let mut count = 0usize;
        // 10 s 内每 10 ms 来一次变更（1000 次突发）
        let mut t = t0;
        for _ in 0..1000 {
            t += Duration::from_millis(10);
            p.on_change();
            if p.on_wake(t) {
                count += 1;
            }
        }
        // 理论下界 ≈ 10000/250 = 40 次；上界由主拍/窗口双限，留 1 次余量
        assert!(
            (39..=41).contains(&count),
            "持续变更下发布次数应 ≈ 1/合并窗口（40），实际 {count}"
        );
        assert!(
            count <= 1000 / (window as usize / 10) + 1,
            "发布率不得超出合并窗口上界"
        );
    }

    // ── run 级：变更即组帧（端到端）──

    /// `run()` 端到端：慢拍**内容变更**必须在合并窗口后触发一次提前组帧——主拍设 3 s，
    /// 若「变更即组帧」缺失（退化为纯主拍），1.2 s 内不会有任何帧 ⇒ 本用例变红
    /// （这正是 §4.2.1 约束 4 点名的失败形态）。
    #[tokio::test]
    async fn run_publishes_on_slow_change_before_main_tick() {
        let latest: SharedLatest = Arc::new(Mutex::new(None));
        let dev = Arc::new(StubDevice::new(DeviceSection {
            ts_ms: 1,
            uptime_secs: Some(11),
            ..Default::default()
        }));
        let provider = DisplayDataProvider::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            stub_client(None, true, None),
            &cfg_slow(3000, 250, 50), // 主拍 3 s，慢拍 50 ms
            true,
            latest.clone(),
        )
        .with_slow_sources(Some(dev.clone()), None, InterlockWiring::Unwired);
        let h = tokio::spawn(provider.run());

        let f = wait_for_frame(&latest, Duration::from_millis(1200))
            .await
            .expect("慢拍变更应触发提前组帧（纯主拍下首帧要 3 s）");
        assert_eq!(f.device.uptime_secs, Some(11), "提前组帧须带上新采到的装置段");
        assert!(dev.count() >= 1);
        h.abort();
    }

    /// **合并窗口不得吞掉尾部变更**：一轮突发变更结束后，**最后一次**内容必须最终上屏。
    ///
    /// 合并窗口（`min_publish_interval_ms`）只允许「合并」，不允许「丢弃」——评审用临时用例
    /// 实测不丢尾，本条把它固化为常驻用例。若 `PublishPacer` 在窗口到期发布后把后续
    /// `pending_change` 清成 `false` 且不再重算（或 `run_publish_loop` 的 `notified` 分支
    /// 漏掉 `continue` 重算 deadline 的语义），尾值 `uptime_secs=6` 就永远上不了屏 ⇒ 红。
    #[tokio::test]
    async fn run_publishes_tail_change_of_burst_not_swallowed_by_merge_window() {
        let latest: SharedLatest = Arc::new(Mutex::new(None));
        let dev = Arc::new(StubDevice::new(DeviceSection {
            ts_ms: 1,
            uptime_secs: Some(1),
            ..Default::default()
        }));
        // 主拍 3 s、合并窗口 250 ms、慢拍 20 ms —— 突发全落在主拍之间，只能靠"变更即组帧"上屏
        let provider = DisplayDataProvider::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            stub_client(None, true, None),
            &cfg_slow(3000, 250, 20),
            true,
            latest.clone(),
        )
        .with_slow_sources(Some(dev.clone()), None, InterlockWiring::Unwired);
        let h = tokio::spawn(provider.run());

        wait_for_frame(&latest, Duration::from_millis(1500))
            .await
            .expect("首帧应发布");
        // 突发 5 次变更（间隔 20 ms，全在同一个 250 ms 合并窗口内 ⇒ 前 4 次被合并）
        for i in 2..=6u64 {
            dev.set(DeviceSection {
                ts_ms: 1,
                uptime_secs: Some(i),
                ..Default::default()
            });
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // **尾值 6** 必须在合并窗口 + 余量内上屏（主拍要到 3 s 后，本窗口内只有它唯一出路）
        let deadline = std::time::Instant::now() + Duration::from_millis(1500);
        let mut seen = None;
        while std::time::Instant::now() < deadline {
            seen = latest.lock().unwrap().as_ref().map(|f| f.device.uptime_secs);
            if seen == Some(Some(6)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            seen,
            Some(Some(6)),
            "突发结束后的**最后一次**变更必须上屏（合并窗口只许合并、不许丢尾）"
        );
        h.abort();
    }

    /// 发布方**必须**走契约编码入口 `to_json_slice`（§3.5 条 3）：单条告警超 1 KiB ⇒ 编码失败
    /// ⇒ 应答 500（**不得** 200 发出超限帧让发布方**静默**违约）。裸 `serde_json::to_vec` 会让
    /// 本用例变红（它会「成功地」发出超限帧）。
    ///
    /// ⚠️ **订正（2026-09-16，勿再引"HMI 端整帧丢弃"）**：本条曾以"否则对端整帧丢弃"为论据，
    /// 该前提**未经实测**——HMI 侧走裸 `serde_json::from_slice`
    /// （`crates/local-display/src/channel.rs:450`），**不经**契约严格解码器，只受传输层
    /// 64 KiB（`channel.rs:78/:675`）限制 ⇒ 单条超 1 KiB 的帧 HMI **本可正常显示**。故本守卫的
    /// 真实定位是**编码侧契约自检**（超限即响亮 500），**不是** HMI 丢帧防线；生产路径不会靠它
    /// ——超长消息已在 ingest 侧截断，见
    /// `oversized_alarm_message_truncated_in_ingest_frame_still_publishes`。
    #[tokio::test]
    async fn http_get_oversized_alarm_returns_500_not_bogus_200() {
        let mut frame = sample_frame(Some(50.0));
        frame.alarms = AlarmsSection {
            ts_ms: 1,
            available: true,
            items: vec![AlarmItem {
                ts_ms: 1,
                level: AlarmLevel::Error,
                message: "x".repeat(mupc_display_proto::MAX_ALARM_MESSAGE_BYTES + 1),
            }],
        };
        let latest: SharedLatest = Arc::new(Mutex::new(Some(frame)));
        let publisher = LoopbackHttpPublisher::new(latest);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve = tokio::spawn(publisher.serve(listener));

        let req = format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", LATEST_PATH);
        let resp = connect_and_get(addr, &req).await;
        let text = String::from_utf8_lossy(&resp);
        assert!(
            text.starts_with("HTTP/1.1 500"),
            "超限帧必须响亮失败（500），不得谎报 200: {text:?}"
        );

        serve.abort();
    }

    /// 慢拍**内容不变**时不额外组帧：主拍 600 ms + 慢拍 50 ms 的恒定源，1.3 s 观察窗内发布
    /// 次数应 ≈ 主拍节奏（断言 `≤3`），**不是**每拍慢源都触发一次（50 ms 一拍 ⇒ 1.3 s 内
    /// 20+ 次）。
    ///
    /// [`StubDevice`] 的 `ts_ms` **每次读自增**（与生产口径一致）⇒ 本条**真的**钉死了
    /// 「`ts_ms` 不计入内容变更判据」：把 `a.ts_ms != b.ts_ms` 加进 [`device_changed`]
    /// ⇒ 该窗口内仍会额外推进 **5 次**（受 250 ms 合并窗口限速）> 上限 3 ⇒ 红
    /// （2026-09-16 破坏性探针实测；原文写的「20+ 次」未计合并窗口限速，与实现不符，已订正）。
    #[tokio::test]
    async fn run_does_not_republish_when_slow_content_unchanged() {
        let latest: SharedLatest = Arc::new(Mutex::new(None));
        // 恒定内容的慢源：展示字段不动，只有 ts_ms 每拍自增（不得触发组帧）
        let dev = Arc::new(StubDevice::new(DeviceSection {
            ts_ms: 1,
            uptime_secs: Some(5),
            ..Default::default()
        }));
        let provider = DisplayDataProvider::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            stub_client(None, true, None),
            &cfg_slow(600, 250, 50),
            true,
            latest.clone(),
        )
        .with_slow_sources(Some(dev.clone()), None, InterlockWiring::Unwired);
        let h = tokio::spawn(provider.run());

        // 等首帧（首次采样是"从 Default 变内容" ⇒ 会触发一次提前组帧）
        wait_for_frame(&latest, Duration::from_millis(500))
            .await
            .expect("首帧应发布");
        let seq_after_first = latest.lock().unwrap().as_ref().unwrap().seq;
        tokio::time::sleep(Duration::from_millis(600)).await;
        dev.set(DeviceSection {
            uptime_secs: Some(5), // 展示内容与首段完全相同（ts_ms 由桩自增，见上）
            ..Default::default()
        });
        tokio::time::sleep(Duration::from_millis(700)).await;
        let seq_now = latest.lock().unwrap().as_ref().unwrap().seq;
        // 600+700 ms ≈ 2 个主拍 ⇒ 序号推进 ≤3；若 ts_ms 被计入内容变更，会推进 20+ 次
        assert!(
            seq_now - seq_after_first <= 3,
            "内容未变时不得额外组帧（ts_ms 变化不算内容变更），实际推进 {} 次",
            seq_now - seq_after_first
        );
        assert!(dev.count() >= 10, "慢拍本身仍须按 50 ms 采集（只是不触发组帧）");
        h.abort();
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // U-73 外设段（慢拍 D 取数接线 + 组帧守卫；设计 §15.1 / §15.2.4 / §15.8.1 T-8..T-12/T-24）
    // ═══════════════════════════════════════════════════════════════════════════

    use mupc_data_processing::latest_values::{LatestValues, PointId as PId, PointQuality, PointValue};

    /// 测试用南向配置（生产形状裁剪：bms 标量+位块 / meter_batt / fire / hvac + 台区总表）。
    /// 地址与生产同口径（`_k ↔ 起始 addr + k − 1`）⇒ `point_table::lookup_in` 能命中（W-2）。
    const TEST_SOUTH_YAML: &str = r#"
poll_ms: 1000
stale_timeout_s: 5
stations:
  - id: bms
    role: battery
    port: "/dev/ttyS2"
    interval_ms: 1000
    regs:
      - name: bms_io
        func: input
        addr: 100
        count: 31
        format: uint16
        scale: 1.0
        points:
          - { at: 16, scale: 0.1 }
          - { at: 17, scale: 0.1 }
          - { at: 19, name: soc }
          - { at: 1 }
      - name: bms_alarm
        func: discrete
        addr: 200
        count: 288
  - id: meter_batt
    role: meter_batt
    port: "/dev/ttyS5"
    interval_ms: 1000
    regs:
      - { name: mb_ui, func: holding, addr: 0x0061, count: 6, format: uint16, scale: 0.1 }
      - { name: mb_phase, func: holding, addr: 0x0087, count: 14, format: int32_scaled, scale: 0.01 }
  - id: fire
    role: fire
    port: "/dev/ttyS6"
    interval_ms: 1000
    regs:
      - name: fire_sys
        func: holding
        addr: 4
        count: 13
        format: uint16
        scale: 1.0
        points:
          - { at: 7, name: fire_det_count }
      - name: fire_det
        func: holding
        addr: 17
        count: 114
        format: uint16
        scale: 1.0
  - id: hvac
    role: hvac
    port: "/dev/ttyS3"
    interval_ms: 5000
    regs:
      - name: hvac_in
        func: input
        addr: 0
        count: 4
        format: int16
        scale: 0.1
        points:
          - { at: 1 }
          - { at: 3 }
      - name: hvac_di
        func: discrete
        addr: 0
        count: 31
  - id: grid_meter
    role: meter_grid
    port: "/dev/ttyS4"
    interval_ms: 1000
    regs:
      - { name: p, func: holding, addr: 0x1000, count: 6, format: int32_scaled, scale: 0.01 }
"#;

    fn test_south_cfg() -> mupc_southd::config::SouthStationsConfig {
        serde_yaml::from_str(TEST_SOUTH_YAML).expect("测试南向配置可解析")
    }

    /// 逐点写入 `latest_values`：`(station, metric, value, ts_ms, quality)`。
    fn fill(latest: &LatestValues, samples: &[(&str, &str, Option<f64>, u64, PointQuality)]) {
        latest.apply(
            samples
                .iter()
                .map(|(st, m, v, ts, q)| {
                    (
                        PId {
                            station: (*st).to_string(),
                            metric: (*m).to_string(),
                        },
                        PointValue {
                            value: *v,
                            ts_ms: *ts,
                            quality: *q,
                        },
                    )
                })
                .collect(),
        );
    }

    fn ok() -> PointQuality {
        PointQuality::Ok
    }

    /// 段内取点（白名单内每一点都在 ⇒ 取不到即测试自身写错）。
    fn at_of(sec: &PeripheralsSection, role: PeriphRole, block: &str, at: u16) -> FramePoint {
        sec.stations
            .iter()
            .find(|s| s.role == role)
            .unwrap_or_else(|| panic!("{role:?} 站必须在段内"))
            .blocks
            .iter()
            .find(|b| b.name == block)
            .unwrap_or_else(|| panic!("{block} 块必须在段内"))
            .values
            .iter()
            .find(|pv| pv.at == at)
            .copied()
            .unwrap_or_else(|| panic!("{block}_{at} 必须在段内（白名单内每一点都在）"))
    }

    /// **T-9：点级 flag 判定六分支**（§15.1.2 伪码逐行）+ 「`flag != Valid ⇒ v = None`」不变量。
    #[test]
    fn periph_point_flag_six_branches_and_never_zero_fill() {
        let cfg = test_south_cfg();
        let plan = peripheral_plan(&cfg);
        let latest = Arc::new(LatestValues::new(5));
        let now = 1_000_000u64;
        latest.mark_station_polled("bms", now);
        latest.mark_station_polled("meter_batt", now);
        fill(
            &latest,
            &[
                // ① 站离线（fire 从未轮询成功）：写了值也不展示
                ("fire", "fire_sys_2", Some(880.0), now, ok()),
                // ② 标量点「本轮未更新」：ts < 站最后轮询时刻 ⇒ NotRead
                ("bms", "bms_io_16", Some(650.0), now - 5_000, ok()),
                // ③ 位点：ts 恒旧（= 最后变化时刻）但站在窗内 ⇒ **仍 Valid**
                ("bms", "bms_alarm_1", Some(1.0), now - 60_000, ok()),
                // ④ quality != Ok ⇒ NotRead（值即使存在也不展示）
                ("bms", "bms_io_17", Some(12.0), now, PointQuality::Stale),
                // ⑤ 非有限 ⇒ RangeError（v = None）
                ("meter_batt", "mb_ui_1", Some(f64::NAN), now, ok()),
                // ⑥ 正常 ⇒ Valid（工程值原样；量纲已由采集侧消解）
                ("meter_batt", "mb_ui_2", Some(229.6), now, ok()),
            ],
        );
        let sec = StationPeripheralSource::new(latest, plan, 7).build_section(now);

        let fire = sec.stations.iter().find(|s| s.role == PeriphRole::Fire).unwrap();
        assert!(!fire.online, "fire 站从未轮询成功 ⇒ 离线");
        assert_eq!(
            {
                let p = at_of(&sec, PeriphRole::Fire, "fire_sys", 2);
                (p.v, p.flag)
            },
            (None, FieldFlag::Offline),
            "① 站离线 ⇒ Offline 且不展示旧值"
        );
        assert_eq!(
            {
                let p = at_of(&sec, PeriphRole::Battery, "bms_io", 16);
                (p.v, p.flag)
            },
            (None, FieldFlag::NotRead),
            "② 标量点本轮未更新 ⇒ NotRead（不补 0）"
        );
        assert_eq!(
            {
                let p = at_of(&sec, PeriphRole::Battery, "bms_alarm", 1);
                (p.v, p.flag)
            },
            (Some(1.0), FieldFlag::Valid),
            "③ 位点不得按标量口径判旧（位点 ts = 最后变化时刻）"
        );
        assert_eq!(
            {
                let p = at_of(&sec, PeriphRole::Battery, "bms_io", 17);
                (p.v, p.flag)
            },
            (None, FieldFlag::NotRead),
            "④ quality != Ok ⇒ NotRead"
        );
        assert_eq!(
            {
                let p = at_of(&sec, PeriphRole::MeterBatt, "mb_ui", 1);
                (p.v, p.flag)
            },
            (None, FieldFlag::RangeError),
            "⑤ 非有限 ⇒ RangeError"
        );
        assert_eq!(
            {
                let p = at_of(&sec, PeriphRole::MeterBatt, "mb_ui", 2);
                (p.v, p.flag)
            },
            (Some(229.6), FieldFlag::Valid),
            "⑥ 其余 ⇒ Valid"
        );

        // 不变量：**任何** `flag != Valid` 的点 `v` 必须 None（不补 0 / 不沿用旧值，EX-30）
        for st in &sec.stations {
            for b in &st.blocks {
                for pv in &b.values {
                    assert!(
                        pv.flag == FieldFlag::Valid || pv.v.is_none(),
                        "{}:{} 的 flag={:?} 却带值",
                        b.name,
                        pv.at,
                        pv.flag
                    );
                }
            }
        }
    }

    /// **D2（§15.2.2 / §15.11 #7）钢瓶气压接缝**：`Some(false)` / `Some(true)` / `None`
    /// 三态都在，且**未接线不得退化成 `Some(false)`**。
    ///
    /// 语义依据（§15.2.2 明文，**不得改**）：
    /// - `Some(false)` = **未配置** ⇒ 屏显「未配置」（忽略 `v`、不得含 `0 kPa`）；
    /// - `Some(true)`  = 正常按值展示；
    /// - `None`        = **不可得**（非消防站 / 接缝未接线）⇒ 按值正常展示。
    ///
    /// 本用例证的是**帧侧**的两件事：① 源对注入的 query 是**纯透传**（role 过滤在适配器侧，
    /// 由 `startup::SouthCylinderPressureQuery` 的用例覆盖）；② **未接线**（默认构造）时
    /// **全部**站回 `None`——不得把"接缝没接"伪装成"未配置"。
    ///
    /// ⚠️ **帧内不得把 `v` 改掉**（`Some(false)` 仍带真实读数）：`v` 列在 §15.2.3 表里对
    /// EDGE-23 是"任意（由数据侧给）"——「未配置」是**屏侧**语义（HMI 忽略 `v`），
    /// 帧侧若把 `v` 清空，就与 EDGE-19「未取数」混为一谈（违反三语义互异）。
    ///
    /// **改什么会让本条变红**：把 `build_section` 的 `cylinder_configured` 写回常量 `None`
    /// ⇒ 第一段断言红；把未接线缺省改成 `Some(false)` ⇒ 第二段红。
    #[test]
    fn periph_cylinder_configured_is_passthrough_and_defaults_to_none() {
        use std::collections::HashMap;

        /// 桩：按站 id 给答案（`None` = 本桩也不知道 ⇒ 透传后仍是 `None`）。
        struct StubCylinder(HashMap<String, Option<bool>>);
        impl CylinderPressureQuery for StubCylinder {
            fn cylinder_configured(&self, station_id: &str) -> Option<bool> {
                self.0.get(station_id).copied().flatten()
            }
        }

        let cfg = test_south_cfg();
        let plan = peripheral_plan(&cfg);
        let latest = Arc::new(LatestValues::new(5));
        let now = 1_000_000u64;
        latest.mark_station_polled("fire", now);
        // 消防钢瓶气压的真实读数（`fire_sys_2`；未配置时"数据侧"照样会给 0.0）
        fill(&latest, &[("fire", "fire_sys_2", Some(0.0), now, ok())]);

        // ① 已接线 ⇒ 逐站透传（消防站 `Some(false)` = EDGE-23「未配置」可达；另一站 `None`）
        let stub: Arc<dyn CylinderPressureQuery> = Arc::new(StubCylinder(
            [
                ("fire".to_string(), Some(false)),
                ("bms".to_string(), Some(true)),
            ]
            .into_iter()
            .collect(),
        ));
        let sec = StationPeripheralSource::new(latest.clone(), plan.clone(), 7)
            .with_cylinder_query(stub)
            .build_section(now);
        let cfg_of = |role: PeriphRole| {
            sec.stations
                .iter()
                .find(|s| s.role == role)
                .unwrap_or_else(|| panic!("{role:?} 站必须在段内"))
                .cylinder_configured
        };
        assert_eq!(
            cfg_of(PeriphRole::Fire),
            Some(false),
            "已接线 + 未配置 ⇒ Some(false)（屏侧据此显「未配置」）"
        );
        assert_eq!(
            cfg_of(PeriphRole::Battery),
            Some(true),
            "透传（role 过滤不在本层）"
        );
        assert_eq!(
            cfg_of(PeriphRole::MeterBatt),
            None,
            "桩未给答案 ⇒ 透传 None（不可得）"
        );
        // 帧内 `v` 不得被 EDGE-23 改写（「忽略 v」是屏侧动作，帧是值通道）
        let cyl = at_of(&sec, PeriphRole::Fire, "fire_sys", 2);
        assert_eq!(
            (cyl.v, cyl.flag),
            (Some(0.0), FieldFlag::Valid),
            "Some(false) 时帧内仍带数据侧给的真实读数（≠ 把它清成 NotRead）"
        );

        // ② 未接线（默认构造）⇒ **全部**站 `None`（含消防站）⇒ 按值正常展示
        let sec = StationPeripheralSource::new(latest, plan, 7).build_section(now);
        for st in &sec.stations {
            assert_eq!(
                st.cylinder_configured, None,
                "{}：接缝未接线 ⇒ 一律 None（不可得），**不得**退化成 Some(false)",
                st.id
            );
        }
        let cyl = at_of(&sec, PeriphRole::Fire, "fire_sys", 2);
        assert_eq!(
            (cyl.v, cyl.flag),
            (Some(0.0), FieldFlag::Valid),
            "未接线 ⇒ 按值正常展示（不因接缝缺失而改值或改 flag）"
        );
    }

    /// `latest_values` **无该点** ⇒ `NotRead`（点缺 ≠ 0；同段其它点不受影响）。
    #[test]
    fn periph_missing_point_degrades_to_not_read_not_zero() {
        let cfg = test_south_cfg();
        let plan = peripheral_plan(&cfg);
        let latest = Arc::new(LatestValues::new(5));
        let now = 1_000_000u64;
        latest.mark_station_polled("bms", now);
        // 一个点都没写 ⇒ 该站全部白名单点都是"点缺"
        let sec = StationPeripheralSource::new(latest, plan, 0).build_section(now);
        let bms = sec.stations.iter().find(|s| s.role == PeriphRole::Battery).unwrap();
        assert!(bms.online, "站在采集窗口内");
        let bms_io = bms.blocks.iter().find(|b| b.name == "bms_io").unwrap();
        assert_eq!(
            bms_io.values.len(),
            mupc_display_proto::PERIPH_WHITELIST
                .iter()
                .filter(|(r, b, _)| *r == PeriphRole::Battery && *b == "bms_io")
                .count(),
            "块内点数 = 白名单点数（行数与 catalog 恒等）"
        );
        for pv in &bms_io.values {
            assert_eq!(pv.flag, FieldFlag::NotRead, "点缺 ⇒ NotRead");
            assert_eq!(pv.v, None, "点缺**不得**补 0");
        }
        // 位块同样逐点 NotRead（块间互不影响）
        assert_eq!(
            at_of(&sec, PeriphRole::Battery, "bms_alarm", 288).flag,
            FieldFlag::NotRead
        );
    }

    /// **T-10：站离线 ⇒ 该站全部点 `v=None`；其余站不受影响**（站级隔离，EX-04）。
    #[test]
    fn periph_station_offline_isolates_other_stations() {
        let cfg = test_south_cfg();
        let plan = peripheral_plan(&cfg);
        let latest = Arc::new(LatestValues::new(5));
        let now = 1_000_000u64;
        latest.mark_station_polled("hvac", now);
        latest.mark_station_polled("bms", now);
        latest.mark_station_offline("bms");
        fill(&latest, &[("hvac", "hvac_in_1", Some(24.5), now, ok())]);
        let sec = StationPeripheralSource::new(latest, plan, 0).build_section(now);
        let bms = sec.stations.iter().find(|s| s.role == PeriphRole::Battery).unwrap();
        let hvac = sec.stations.iter().find(|s| s.role == PeriphRole::Hvac).unwrap();
        assert!(!bms.online, "离线站 ⇒ online=false");
        assert_eq!(bms.last_ok_ms, 0, "离线后 `station_poll_ms` 被清除 ⇒ 0（屏显 --）");
        assert!(
            bms.blocks.iter().flat_map(|b| &b.values).all(|pv| pv.v.is_none()),
            "站离线 ⇒ 该站全部点 v=None"
        );
        assert!(hvac.online, "其余站不受影响（站级隔离）");
        assert_eq!(
            at_of(&sec, PeriphRole::Hvac, "hvac_in", 1).flag,
            FieldFlag::Valid
        );
    }

    /// **R-38 缺口的退化（T-24）**：无 `station_last_poll_ms` 数据时 `last_ok_ms = 0`
    /// （屏显 `--`，**不臆造、不自维护第二份真源**）；该情形下站也不在采集窗口内 ⇒ 全 Offline。
    #[test]
    fn periph_last_ok_ms_is_zero_when_unavailable_never_invented() {
        let cfg = test_south_cfg();
        let plan = peripheral_plan(&cfg);
        let latest = Arc::new(LatestValues::new(5));
        let now = 1_000_000u64;
        // 只写点、**不** `mark_station_polled`
        fill(&latest, &[("bms", "bms_io_16", Some(650.0), now - 9_000, ok())]);
        let sec = StationPeripheralSource::new(latest, plan, 0).build_section(now);
        let bms = sec.stations.iter().find(|s| s.role == PeriphRole::Battery).unwrap();
        assert_eq!(bms.last_ok_ms, 0, "无「最后成功时刻」⇒ 0（显 --，不臆造）");
        assert!(
            bms.blocks.iter().flat_map(|b| &b.values).all(|pv| pv.flag == FieldFlag::Offline),
            "从未成功轮询 ⇒ 站不 active ⇒ 全部 Offline"
        );
    }

    /// 计划投影 = **白名单唯一真源**：跳过 `meter_grid`、排除项结构性缺席、`fire_det` 按
    /// 配置 `count` 展开、`renames` 与站配置同源。
    #[test]
    fn periph_plan_projects_whitelist_and_skips_meter_grid() {
        let cfg = test_south_cfg();
        let plan = peripheral_plan(&cfg);
        assert!(
            !plan.iter().any(|s| s.id == "grid_meter"),
            "台区总表不在本增量内（§15 范围外 #3）"
        );
        assert_eq!(plan.len(), 4, "bms / meter_batt / fire / hvac");
        let ats = |id: &str, blk: &str| -> Vec<u16> {
            plan.iter()
                .find(|s| s.id == id)
                .and_then(|s| s.blocks.iter().find(|b| b.name == blk))
                .map(|b| b.ats.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            ats("fire", "fire_det"),
            (1..=114).collect::<Vec<_>>(),
            "fire_det 按配置 count（114 = 6×19）展开为逐寄存器点"
        );
        assert!(!ats("hvac", "hvac_di").contains(&26), "位 25 保留位不上屏");
        assert!(!ats("meter_batt", "mb_phase").contains(&7), "PT 对照不上屏");
        assert!(!ats("meter_batt", "mb_phase").contains(&8), "CT 对照不上屏");
        assert_eq!(ats("bms", "bms_io").len(), 17, "bms_io 白名单 17 点（§15.5.2）");
        assert_eq!(ats("bms", "bms_alarm").len(), 288, "告警位 288 点（位 200–487）");
        let bms_io = plan
            .iter()
            .find(|s| s.id == "bms")
            .and_then(|s| s.blocks.iter().find(|b| b.name == "bms_io"))
            .unwrap();
        assert_eq!(bms_io.renames, vec![(19u16, "soc".to_string())], "点名覆盖投影");
        // `soc`（bms_io_19）**不在**本增量白名单内：§15.5.2 段「电池」未列它，且 330 = 17 + 8
        // + 9 + 4 + 4 + 288 的等式只有"bms_io 取 17 点"成立（SOC 仍由 P1 的 F1 展示）
        assert!(!bms_io.ats.contains(&19), "SOC 不在外设白名单（P6 电池段不重复展示 F1）");
        assert!(!bms_io.is_bit, "`input` 块 ⇒ 非位块");
        assert_eq!(ats("bms", "bms_alarm").len(), 288);
        let alarm_blk = plan
            .iter()
            .find(|s| s.id == "bms")
            .and_then(|s| s.blocks.iter().find(|b| b.name == "bms_alarm"))
            .unwrap();
        assert!(alarm_blk.is_bit, "`discrete` 块 ⇒ is_bit（位点跳过「本轮未更新」判据）");
    }

    /// **T-12：内容比较忽略 `ts_ms` / `last_ok_ms` / 块 `ts_ms`**（否则 500 ms 兜底 tick 会把
    /// 发布率打满 4 Hz）；内容真变（值 / 在线态）才判"变更"。
    #[test]
    fn periph_content_compare_ignores_timestamps_only() {
        let cfg = test_south_cfg();
        let plan = peripheral_plan(&cfg);
        let latest = Arc::new(LatestValues::new(5));
        let now = 1_000_000u64;
        latest.mark_station_polled("bms", now);
        fill(&latest, &[("bms", "bms_io_16", Some(650.0), now, ok())]);
        let src = StationPeripheralSource::new(latest.clone(), plan, 3);
        let a = src.build_section(now);
        // 「同一轮成功轮询、值一字未改」——兜底 tick 每 500 ms 都会发生的事：时标前进、内容不变
        latest.mark_station_polled("bms", now + 500);
        fill(&latest, &[("bms", "bms_io_16", Some(650.0), now + 500, ok())]);
        let b = src.build_section(now + 500);
        assert_ne!(a.stations[0].last_ok_ms, b.stations[0].last_ok_ms, "前提：时标确实前进了");
        assert_eq!(
            at_of(&a, PeriphRole::Battery, "bms_io", 16).flag,
            FieldFlag::Valid
        );
        assert!(
            !peripherals_changed(&a, &b),
            "只前进时标（值不变）不得判为内容变更（否则发布率打满 4 Hz）"
        );
        // 值变 ⇒ 判变更
        fill(&latest, &[("bms", "bms_io_16", Some(651.0), now + 500, ok())]);
        let c = src.build_section(now + 500);
        assert!(peripherals_changed(&a, &c), "值变化必须判为内容变更");
        // 在线态变 ⇒ 判变更（站离线要尽快上屏）
        latest.mark_station_offline("bms");
        let d = src.build_section(now + 600);
        assert!(peripherals_changed(&c, &d), "站离线必须判为内容变更");
        // 空段 → 有段 ⇒ 判变更
        assert!(peripherals_changed(
            &PeripheralsSection::default(),
            &src.build_section(now + 700)
        ));
    }

    /// 造一个 `fire_det` 带 `units` 只（每只 6 点）的段（供守卫用例）。
    fn fire_det_section(units: usize) -> PeripheralsSection {
        let mut sec = PeripheralsSection {
            ts_ms: 1,
            available: true,
            catalog_rev: 1,
            truncated: vec![],
            stations: vec![PeripheralStation {
                id: "fire".into(),
                role: PeriphRole::Fire,
                online: true,
                last_ok_ms: 1,
                cylinder_configured: None,
                blocks: vec![PeripheralBlock {
                    name: "fire_det".into(),
                    ts_ms: 1,
                    renames: vec![],
                    values: (1..=units as u16 * 6)
                        .map(|at| FramePoint {
                            at,
                            v: Some(1.0),
                            flag: FieldFlag::Valid,
                        })
                        .collect(),
                }],
            }],
        };
        sec.stations[0].blocks[0].values.truncate(units * 6);
        sec
    }

    /// **T-11 / 守卫顺序**：`build_frame` 的**步骤 4（预算预检 → 前缀截断）必须早于
    /// 步骤 5（出口守卫）**。
    ///
    /// 构造：`fire_det` **119 只**（714 点）+ 441 个非 fire_det 点的上界口径已超
    /// `MAX_PERIPH_BYTES`（119 只 > `k_max = 111`）。
    /// - 若步骤 4 缺席 / 后置 ⇒ `truncated` 空、点数仍是 714；
    /// - 若步骤 5 先跑 ⇒ 整段被置 `available=false` 并**清空载荷**（`stations` 空）；
    /// ⇒ 「`truncated == ["fire_det:119→111"]` ∧ 点数 666 ∧ `available` 仍 true ∧ 帧可编码」
    /// 唯一对应"先裁后守"。
    #[tokio::test]
    async fn build_frame_truncates_before_exit_guard() {
        let mut p = bare_provider(&cfg_slow(1000, 250, 500));
        {
            let mut g = p.caches.peripherals.write().unwrap();
            *g = fire_det_section(119);
        }
        let frame = p.sample_once().await;
        assert!(frame.peripherals.available, "裁剪 ≠ 段不可用（不得整段降级）");
        assert_eq!(
            frame.peripherals.truncated,
            vec!["fire_det:119→111".to_string()],
            "步骤 4 必须裁到 k_max=111 只并留下可复现条目（F21.4 不得静默）"
        );
        let det = frame
            .peripherals
            .stations
            .iter()
            .find(|s| s.role == PeriphRole::Fire)
            .and_then(|s| s.blocks.iter().find(|b| b.name == "fire_det"))
            .expect("裁剪后段体仍在（证明步骤 5 未清载荷）");
        assert_eq!(det.values.len(), 111 * 6, "前缀截断到 111 只 × 6 点");
        assert_eq!(det.values[0].at, 1, "保留地址最小的前缀（可复现）");
        assert!(frame.to_json_slice().is_ok(), "裁剪后整帧必须在 64 KiB 内");
        // 幂等：再组一帧不得重复记条目
        let frame2 = p.sample_once().await;
        assert_eq!(frame2.peripherals.truncated.len(), 1);
    }

    /// **步骤 5 兜底**（T-11 ③）：估算失准（人为构造超预算帧）⇒ 整段置不可用、**既有段
    /// 逐字段不受影响**、帧照常发布（不黑屏）。
    #[tokio::test]
    async fn build_frame_exit_guard_downgrades_section_and_keeps_existing_segments() {
        let mut p = bare_provider(&cfg_slow(1000, 250, 500));
        // 人为把段与告警一起做大：1035 点 × f64 极值形态（≈64 KiB）+ 8 条 1 KiB 告警（≈8 KiB）
        // ⇒ 整帧必然 > MAX_FRAME_BYTES（§15.2.4 的 R-43 失效模式）
        {
            let mut g = p.caches.peripherals.write().unwrap();
            *g = fire_det_section(111); // 666 点
            g.stations[0].blocks[0].values.iter_mut().for_each(|pv| {
                pv.v = Some(f64::MIN); // −1.7976931348623157e308（23 字符/点）
            });
            g.stations[0].blocks.push(PeripheralBlock {
                name: "fire_sys".into(),
                ts_ms: 1,
                renames: vec![],
                values: (1..=369u16)
                    .map(|at| FramePoint {
                        at,
                        v: Some(f64::MIN),
                        flag: FieldFlag::Valid,
                    })
                    .collect(),
            });
        }
        {
            let mut g = p.caches.alarms.write().unwrap();
            for i in 0..8 {
                g.items.push(AlarmItem {
                    ts_ms: 1,
                    level: mupc_display_proto::AlarmLevel::Warn,
                    message: "x".repeat(1024),
                });
                g.available = true;
                let _ = i;
            }
        }
        let frame = p.sample_once().await;
        assert!(
            !frame.peripherals.available,
            "兜底 ⇒ 段置不可用（屏显「外设数据不可用」）"
        );
        assert!(frame.peripherals.stations.is_empty(), "载荷已清空（仅翻布尔位不减字节）");
        assert!(frame.peripherals.truncated.is_empty());
        assert!(frame.to_json_slice().is_ok(), "既有段照常发布（不黑屏）");
        assert_eq!(frame.alarms.items.len(), 8, "既有告警段逐字段不受影响");
        assert!(frame.alarms.available);
        // 既有段与"无外设段"的基线帧逐字段一致
        let mut q = bare_provider(&cfg_slow(1000, 250, 500));
        let baseline = q.sample_once().await;
        assert_eq!(frame.device, baseline.device);
        assert_eq!(frame.info, baseline.info);
        assert_eq!(frame.interlock, baseline.interlock);
        assert_eq!(frame.p_total, baseline.p_total);
        assert_eq!(frame.soc_source, baseline.soc_source);
    }

    /// **T-8：未接线 ⇒ `available=false`**（「外设数据不可用」，**不得**出空段伪装正常）。
    #[tokio::test]
    async fn periph_unwired_keeps_section_unavailable() {
        let mut p = bare_provider(&cfg_slow(1000, 250, 500));
        assert!(!p.peripherals().available, "缓存缺省即「不可用」");
        let frame = p.sample_once().await;
        assert!(!frame.peripherals.available, "EDGE-22：整段「外设数据不可用」");
        assert!(frame.peripherals.stations.is_empty(), "不得出空段伪装正常");
        assert!(frame.to_json_slice().is_ok(), "外设缺失不得拖垮既有段");
    }

    /// §15.3.1 的**同源同值**：帧内 `catalog_rev` = catalog 端点的 `rev`（单一真源）。
    #[tokio::test]
    async fn frame_catalog_rev_matches_catalog_endpoint_rev() {
        let core: crate::core_config::CoreConfig = serde_yaml::from_str(include_str!(
            "../../../deploy/config/mupc_core_config.production.yaml"
        ))
        .expect("生产配置可解析");
        let prod = core.south_stations;
        let plan = peripheral_plan(&prod);
        assert!(!plan.is_empty(), "生产配置必须投影出外设站（否则本用例无鉴别力）");
        let cat = crate::console_host::build_peripheral_catalog(&prod, &plan, 1);
        assert_eq!(cat.rev, mupc_display_proto::catalog_rev(&cat), "rev 自洽");
        assert_ne!(cat.rev, 0, "有内容的目录 rev 不得为 0（0 = 未取得）");
        let latest = Arc::new(LatestValues::new(5));
        let src_obj = Arc::new(StationPeripheralSource::new(
            latest.clone(),
            plan.clone(),
            cat.rev,
        ));
        // 段缓存的内容 = 采样器的产物；此处直接落缓存（等价于慢拍 D 跑过一拍，免 sleep）
        let seed = src_obj.snapshot(now_ms());
        assert_eq!(seed.catalog_rev, cat.rev, "源产出的段必须带装配时的 catalog_rev");
        let src: Arc<dyn PeripheralSource> = src_obj;
        let mut p = bare_provider(&cfg_slow(1000, 250, 500)).with_peripheral_source(src);
        {
            let mut g = p.caches.peripherals.write().unwrap();
            *g = seed;
        }
        let f = p.sample_once().await;
        assert_eq!(
            f.peripherals.catalog_rev, cat.rev,
            "帧内 catalog_rev 必须与端点 rev 同源同值（否则屏侧无休止重取 catalog）"
        );
        assert!(f.peripherals.available);
    }

    /// **兜底 tick 收敛**（§15.1.1 / §15.6.1 约束 1）：变更通知一路**完全丢失**
    /// （`subscribe() → None`）时，外设值仍在 `periph_poll_ms` 量级内收敛到段缓存。
    /// 用短周期真实推进（`period = 50 ms`，用例总时长 ≈150 ms；语义与 500 ms 等价）。
    #[tokio::test]
    async fn periph_fallback_tick_converges_without_broadcast() {
        /// 包装：把订阅面**摘掉**（模拟"广播一路全丢"）。
        struct NoNotify(Arc<StationPeripheralSource>);
        impl PeripheralSource for NoNotify {
            fn snapshot(&self, now_ms: u64) -> PeripheralsSection {
                self.0.snapshot(now_ms)
            }
            fn subscribe(&self) -> Option<tokio::sync::broadcast::Receiver<ChangeBatch>> {
                None
            }
        }
        let cfg = test_south_cfg();
        let plan = peripheral_plan(&cfg);
        let latest = Arc::new(LatestValues::new(5));
        let t0 = now_ms();
        latest.mark_station_polled("hvac", t0);
        fill(&latest, &[("hvac", "hvac_in_1", Some(24.5), t0, ok())]);
        let src = Arc::new(StationPeripheralSource::new(latest.clone(), plan, 0));
        let cache: Arc<RwLock<PeripheralsSection>> =
            Arc::new(RwLock::new(PeripheralsSection::default()));
        let period = 50u64; // = periph_poll_ms（测试取短周期；语义等价）
        let h = tokio::spawn(DisplayDataProvider::run_periph_sampler(
            Some(Arc::new(NoNotify(src)) as Arc<dyn PeripheralSource>),
            cache.clone(),
            Arc::new(Notify::new()),
            period,
        ));
        let read_v = |sec: &PeripheralsSection| -> Option<f64> {
            sec.stations
                .iter()
                .find(|s| s.role == PeriphRole::Hvac)
                .and_then(|s| s.blocks.iter().find(|b| b.name == "hvac_in"))
                .and_then(|b| b.values.iter().find(|pv| pv.at == 1))
                .and_then(|pv| pv.v)
        };
        tokio::time::sleep(Duration::from_millis(period)).await;
        assert_eq!(
            read_v(&read_cache(&cache)),
            Some(24.5),
            "首拍必须把初值带进段缓存"
        );
        // 值变化（**不发任何通知**）
        fill(&latest, &[("hvac", "hvac_in_1", Some(25.5), now_ms(), ok())]);
        // 兜底 tick：≤ 1 拍（这里给 2 拍余量，断言"收敛在上界量级内"）
        tokio::time::sleep(Duration::from_millis(period * 2)).await;
        assert_eq!(
            read_v(&read_cache(&cache)),
            Some(25.5),
            "广播全丢时，兜底 tick（periph_poll_ms）必须保证收敛"
        );
        h.abort();
    }


    /// **南向 role → 显示 role 的运行时映射**（T19 评审残留 ⑤③ 的落点，设计 §15.2.2 注）。
    ///
    /// 两侧 serde 名逐字对应；**`meter_grid`（台区关口总表）不在本增量内** ⇒ 映射为
    /// `Unknown`，其块由 [`peripheral_plan`] 整站跳过（§15 范围外 #3）。
    #[test]
    fn south_role_maps_to_display_role_and_meter_grid_is_excluded() {
        use mupc_southd::config::Role as R;
        for (south, want) in [
            (R::Hvac, PeriphRole::Hvac),
            (R::Fire, PeriphRole::Fire),
            (R::Battery, PeriphRole::Battery),
            (R::MeterBatt, PeriphRole::MeterBatt),
            (R::Pcs, PeriphRole::Pcs),
            (R::MeterGrid, PeriphRole::Unknown),
        ] {
            assert_eq!(periph_role_of(south), want, "{south:?} 映射漂移");
        }
        // 台区总表：即便白名单里存在同名块也不进计划（role = Unknown 的整站跳过）
        assert!(mupc_display_proto::PERIPH_WHITELIST
            .iter()
            .all(|(r, _, _)| *r != PeriphRole::Unknown));
    }

}
