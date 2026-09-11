//! 帧数据模型（跨进程契约）——与设计文档 §3.1 逐字段对齐（`[DESIGN_APPROVED]`）。
//!
//! 语义要点（摘自设计）：
//! - 本文件是 mupcd（DisplayDataProvider 发布方）与 mupc-local-display（渲染订阅方）
//!   以及测试桩共享的**帧契约单一真源**。
//! - 所有数值展示仅当对应 `FieldFlag == Valid`；源不可得一律显式打标，禁止补 0 / 沿用
//!   陈旧值冒充实时（PRD "不造假值"）。
//! - `run_state` 在帧内值域保证 0..=3（`RunState` 枚举 + `from_raw` 越界滤除），渲染端
//!   `Option<RunState>` match 穷尽四态 + None 即可，无枚举外值分支。
//! - v2 新增 [`DeviceSection`] / [`AlarmsSection`] / [`InfoSection`] / [`InterlockSection`]
//!   四段，全部 `#[serde(default)]`：**旧帧（v1）反序列化到 v2 类型时新段取 `Default`**，
//!   其 `available=false` 等缺省值恰好落在 §9 降级语义上（显「不可用」而非伪装正常）。
//! - 慢拍段的 `Option` 字段 `None` 一律表示「不可得」→ 渲染端显式降级（`--` / 「未提供」），
//!   **严禁补 0 或臆造**（PRD §5 总原则，EDGE-16 等）。

/// 帧协议版本。
///
/// v1 → v2：段扩展（新增 `device` / `alarms` / `info` / `interlock` 四段）。
/// 消费方**必须**校验本值（[`DisplayFrame::check_version`] / [`DisplayFrame::from_json_slice`]）：
/// 不一致即拒绝该帧，不得静默按旧语义展示（设计 §3.5 条 1 / PRD §4.4.1）。
pub const PROTO_VERSION: u8 = 2;

/// 渲染端判「数据过期」阈值（PRD F5.3：当前时间 − 帧时间戳 > 2s 判过期）。
pub const DEFAULT_STALE_MS: u64 = 2000;

/// 单帧 JSON 体上限（设计 §3.5 条 3：畸形帧防护，客户端对帧大小设上限 64 KB）。
///
/// **单一常量真源**：解码（[`DisplayFrame::from_json_slice`]）与编码
/// （[`DisplayFrame::to_json_slice`]）共用本值，两侧判据不得各写一份而漂移。
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// 单条告警消息文本上限（字节，UTF-8）。
///
/// 设计未规定单条上限，此处为契约层自定的编码侧守卫：F7 告警为**单行** UI 文案
/// （10 条/页），1 KiB 已远超实际需求，且 10 × 1 KiB ≈ 10 KiB ≪ [`MAX_FRAME_BYTES`]。
/// 无此约束时，发布方 10 条 × 7 KB 消息 = 70 KB 会「正常发出」，
/// HMI 端整帧 [`crate::Error::FrameTooLarge`] 丢弃 → 画面停在旧帧且**无法定位责任方**。
pub const MAX_ALARM_MESSAGE_BYTES: usize = 1024;

/// 读通道唯一端点路径（设计 §3.2）。`config::DEFAULT_CHANNEL_URL` 以本常量为路径。
pub const LATEST_PATH: &str = "/v1/display/latest";

/// 点级字段有效/降级标志。渲染端据 flag 决定显示数值或 `--` + 对应角标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldFlag {
    /// 正常展示。
    Valid,
    /// "未取数"（点表/采集未覆盖，PRD 6.4）。
    NotRead,
    /// "源离线"（PCS 离线/核间读失败，PRD 6.1）。
    Offline,
    /// "数据异常"（域值化量程/有限性校验不过，PRD 6.5）。
    RangeError,
}

/// 单数值字段（工程值 + 点级 valid 标志）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Field {
    /// 工程值；`flag != Valid` 时通常 `None`（不补 0，PRD 不造假值）。
    pub v: Option<f64>,
    /// 点级有效/降级标志。
    pub flag: FieldFlag,
}

/// 运行状态（F2，主判据 REG1013）。枚举化保证值域 0..=3：
/// **越界态在帧内不可达**——采集侧心跳已将 1013 越界读数按坏读数滤除（改判离线）。
/// JSON 以判别数 u8 传输（如 `"run_state": 2`），serde 以手写 u8 映射实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RunState {
    /// 停机。
    Stop = 0,
    /// 待机。
    Standby = 1,
    /// 充电。
    Charge = 2,
    /// 放电。
    Discharge = 3,
}

impl RunState {
    /// 由原始寄存器读数（REG1013 UInt16）构造；越界（>3）返回 `None`（采集侧滤除语义）。
    pub fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            0 => Some(Self::Stop),
            1 => Some(Self::Standby),
            2 => Some(Self::Charge),
            3 => Some(Self::Discharge),
            _ => None,
        }
    }

    /// UI 展示名（F2 文字态；色板由渲染 layout 决定，此处仅给文字）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Stop => "停机",
            Self::Standby => "待机",
            Self::Charge => "充电",
            Self::Discharge => "放电",
        }
    }
}

impl std::fmt::Display for RunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

impl serde::Serialize for RunState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // JSON 以判别数 u8 传输（设计 §3.3，如 `"run_state": 2`）。
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> serde::Deserialize<'de> for RunState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = u16::deserialize(deserializer)?;
        Self::from_raw(raw)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid RunState raw value: {raw}")))
    }
}

/// SOC 展示源标注（三态，对齐 UI 源标签）。不含「双源一致」态——SOC 源裁决是
/// 「优先级+回落」的**单源化**（Bms fresh → Bms；否则活读核间 REG1010 → PcsReg1010；
/// 双失 → Lost），任一时该值实际取值源唯一（设计 §3.3 末落地解释）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocSource {
    /// BMS 站 SOC（优先源）。
    Bms,
    /// 核间活读 PCS REG1010 SOC（回落源）。
    PcsReg1010,
    /// 双源皆失/无 fresh 源（"SOC 源失效"，警示胶囊）。
    Lost,
}

impl SocSource {
    /// UI 源标签展示名（设计 §6.3 F1 行：源胶囊 BMS / PCS / 失效）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Bms => "BMS",
            Self::PcsReg1010 => "PCS(REG1010)",
            Self::Lost => "SOC 源失效",
        }
    }
}

impl std::fmt::Display for SocSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

/// 链路状态（F6 装置整体状态：IEC 104 / 核间 / 数据通道）。
///
/// `Default = Unknown`：缺省**不得**落在 `Connected`——F6.5 要求「连接状态不可得时显示
/// 「未配置 / 未知」，**不显示为「正常」**」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// 已连接。
    Connected,
    /// 连接中 / 等待。
    Connecting,
    /// 断开 / 错误。
    Disconnected,
    /// 未配置。
    NotConfigured,
    /// 未知（不可得；**缺省态**）。
    #[default]
    Unknown,
}

impl LinkState {
    /// UI 文字态（设计 §5.4：`LinkState → LedView{state, text}`；颜色由 theme 决定）。
    /// F6.5：`Unknown` / `NotConfigured` 显式给出「未知」/「未配置」，绝非「正常」。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Connected => "已连接",
            Self::Connecting => "连接中",
            Self::Disconnected => "断开",
            Self::NotConfigured => "未配置",
            Self::Unknown => "未知",
        }
    }
}

/// 当前控制源（F6）。AI 停用期为固定语义（PRD §3.1 F6 备注）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    /// 本地策略引擎（AI 停用期唯一默认下发源）。
    LocalStrategy,
    /// AI 引擎已停用（固定文案，见 [`ControlSource::display_name`]）。
    AiDisabled,
    /// 未知（不可得；**缺省态**，不臆造）。
    #[default]
    Unknown,
}

impl ControlSource {
    /// UI 文字态。`AiDisabled` 取 PRD §3.1 F6 备注的固定文案。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::LocalStrategy => "本地策略引擎",
            Self::AiDisabled => "AI 引擎已停用，本地策略引擎为默认下发源",
            Self::Unknown => "未知",
        }
    }
}

/// 服务可达范围（设计 §3.1：**当前恒为** [`ServiceScope::LoopbackOnly`]；枚举化以便
/// 未来若开管理面时有显式声明点）。UI 展示时**必须**与 `mgmt_ipv4` 分列，不得让现场
/// 据此认为 HMI 接口可从远端访问（EDGE-24 / PM 裁定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceScope {
    /// 仅本机回环 127.0.0.1（安全红线，PL-4）。
    #[default]
    LoopbackOnly,
}

impl ServiceScope {
    /// UI 文字态（与「设备管理 IP」严格分列）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::LoopbackOnly => "仅回环 127.0.0.1",
        }
    }
}

/// F6 装置整体状态段（设计 §3.1；慢拍 3 s 采集；端到端 ≤3.85 s）。
///
/// `Default` 全字段不可得（`uptime_secs` 等 `None`、链路态 `Unknown`、控制源 `Unknown`）——
/// 旧帧（v1）无该段时取缺省 → UI 显式降级，不伪装正常。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DeviceSection {
    /// 本段采集时刻（Unix 毫秒；0 = 未采集）。
    pub ts_ms: u64,
    /// 进程运行时长（秒）；`None` = 不可得。
    pub uptime_secs: Option<u64>,
    /// CPU 温度（℃）；`None` = 不可得。
    pub cpu_temp_c: Option<f64>,
    /// 内存使用率（%）；`None` = 不可得。
    pub mem_used_pct: Option<f64>,
    /// 调度主站（IEC 104）链路。
    pub iec104: LinkState,
    /// 核间（实时控制模块）链路。
    pub intercore: LinkState,
    /// 跨进程数据通道（**HMI 侧自判并本地覆盖**，服务端给 `Unknown`，见设计 §5.5）。
    pub hmi_channel: LinkState,
    /// 当前控制源。
    pub control_source: ControlSource,
}

/// 告警级别（F7.2：颜色 + 文字双通道）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlarmLevel {
    /// 错误。
    Error,
    /// 警告。
    Warn,
    /// 提示。
    Info,
}

impl AlarmLevel {
    /// UI 文字态（PRD F7 以 `ERROR` / `WARN` / `INFO` 字面量给出口径，此处对齐）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
        }
    }
}

/// 单条告警（F7.1：时间戳 + 级别 + 消息）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AlarmItem {
    /// 告警发生时刻（Unix 毫秒）。
    pub ts_ms: u64,
    /// 级别。
    pub level: AlarmLevel,
    /// 消息文本。
    pub message: String,
}

/// F7 告警段（设计 §3.1；慢拍 0.5 s + 变更即组帧；端到端 ≤1.35 s；`items` ≤10，时间倒序）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AlarmsSection {
    /// 本段采集时刻（Unix 毫秒；0 = 未采集）。
    pub ts_ms: u64,
    /// 源可用性。`false` → 屏显「告警源不可用」，**不得**显「无告警」（EDGE-09）。
    /// `Default = false`：旧帧 / 未采集 → 显式不可用，绝不伪装成「无告警」。
    pub available: bool,
    /// 告警列表（≤10 条，时间倒序）。
    pub items: Vec<AlarmItem>,
}

impl AlarmsSection {
    /// 按 `max`（= `display.alarm_page_size`，默认 10）截断条目，保持时间倒序。
    ///
    /// 落地设计 §3.1 的「items ≤10」：截断而非丢弃整个列表，且**不动 `available`**——
    /// 「源不可用」（`available=false`）与「条目被裁到上限」是两个不同语义（EDGE-09）。
    pub fn cap_items(&mut self, max: usize) {
        if self.items.len() > max {
            self.items.truncate(max);
        }
    }
}

/// F8 版本与装置信息段（设计 §3.1；页面加载一次性）。
///
/// 缺失口径：`build_time` / `model` / `serial` / `mgmt_ipv4` 为 `None` → 屏显「未提供」
/// （EDGE-16，**不臆造**）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct InfoSection {
    /// 固件版本号（`env!("CARGO_PKG_VERSION")`）。
    pub firmware_version: String,
    /// 编译时间戳；`None` = 未提供（EDGE-16）。
    pub build_time: Option<String>,
    /// 装置型号；`None` = 未提供。
    pub model: Option<String>,
    /// 序列号；`None` = 未提供（无可靠真源，不臆造）。
    pub serial: Option<String>,
    /// 服务监听口径（恒 `LoopbackOnly`，机器可读的「对外不可达」声明）。
    /// 缺省即 [`ServiceScope::LoopbackOnly`]（该枚举仅此一值）。
    pub service_scope: ServiceScope,
    /// 设备管理 IP（`getifaddrs` 取首个 UP 的非回环 IPv4）；`None` = 未提供。
    /// 与 `service_scope` 是**两个不同概念**，UI 必须分列（PM 裁定 / EDGE-24）。
    pub mgmt_ipv4: Option<String>,
}

/// 联锁触发源条目（F16）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InterlockSourceItem {
    /// 触发源标识（如 estop / flood / fire / door）。
    pub name: String,
    /// 是否处于触发态。
    pub tripped: bool,
}

/// F16 联锁状态段（设计 §3.1；慢拍 0.5 s + 变更即组帧；端到端 ≤1.35 s）。
///
/// `Default`：`available=false` → 屏显「联锁状态不可用」，**不得**显「未联锁」
/// （IL-01.6 / EDGE-12）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct InterlockSection {
    /// 本段采集时刻（Unix 毫秒；0 = 未采集）。
    pub ts_ms: u64,
    /// 状态源可用性。`false` → 显「联锁状态不可用」，**不得**显「未联锁」。
    pub available: bool,
    /// 联锁功能是否启用。
    pub enabled: bool,
    /// 是否已触发锁存（latch）。
    pub latched: bool,
    /// PCS 停机是否失败 / 未确认。
    pub stop_failed: bool,
    /// 各触发源明细。
    pub sources: Vec<InterlockSourceItem>,
    /// 故障灯（语义名优先，**不绑 DO 号**，见设计 §4.6 备注 / §14 R-09）；`None` = 未知。
    pub fault_lamp: Option<bool>,
    /// 运行灯（语义名优先，不绑 DO 号）；`None` = 未知。
    pub run_lamp: Option<bool>,
    /// 释放前置条件：须保持的秒数（UI 用于提示「须保持 N 秒」）。
    pub release_hold_secs: u64,
}

/// 一帧展示数据（每 1s 由 mupcd 域值化后发布）。字段名/语义与设计 §3.1 逐字对齐。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisplayFrame {
    /// = `PROTO_VERSION`。
    pub version: u8,
    /// 单调递增发布序号（重启清零；渲染端判连续/重排）。
    pub seq: u64,
    /// 域值化时刻（Unix 毫秒；新鲜度判据 F5.3）。
    pub ts_ms: u64,
    /// F1: 裁决后 SOC(%)。`None` ↔ `soc_source = Lost`（双源皆失）→ 屏显 `--` + "SOC 源失效"，禁沿用旧值。
    pub soc: Option<f64>,
    /// F1: 三态源标注；渲染据此画源胶囊（Lost → 警示色）。
    pub soc_source: SocSource,
    /// F1: 所用源点级异常（如 PCS 源 RangeError）辅助。
    pub soc_flag: FieldFlag,
    /// F2: 运行状态（1013 主判据；值域保证 0..=3）。`None` = PCS 离线/心跳无有效态（PRD 6.1）。
    pub run_state: Option<RunState>,
    /// 核间 modbus 链路在线（心跳维护；PCS 离线整体提示 6.1）。
    pub pcs_online: bool,
    /// F3: 三相有功(kW) + 设备总有功(kW)——已 ×0.1，渲染端不再换算。`[A, B, C]`。
    pub p_phase: [Field; 3],
    /// F3: 设备总有功(kW)。
    pub p_total: Field,
    /// F4: 三相电流(A)——已 ×0.1。`[A, B, C]`。
    pub i_phase: [Field; 3],
    /// 6.6 佐证一致性: 1013(充/放) 与 Σp_phase 方向显著反向 → true（渲染端加"方向不一致"角标，主状态仍以 1013 展示）。
    pub inconsistency: bool,

    // ── v2 新增分节（全部 serde(default)，旧客户端可容忍；v1 帧 → v2 类型取 Default）──
    /// F6 装置整体状态。
    #[serde(default)]
    pub device: DeviceSection,
    /// F7 告警列表。
    #[serde(default)]
    pub alarms: AlarmsSection,
    /// F8 版本与装置信息。
    #[serde(default)]
    pub info: InfoSection,
    /// F16 联锁状态。
    #[serde(default)]
    pub interlock: InterlockSection,
}

impl DisplayFrame {
    /// 版本一致性校验（设计 §3.5 条 1 / PRD §4.4.1）。
    ///
    /// 不一致 → `Err(`[`crate::Error::ProtoVersionMismatch`]`)`：**拒绝该帧**，
    /// 绝不静默按旧语义展示。
    pub fn check_version(&self) -> crate::Result<()> {
        if self.version != PROTO_VERSION {
            return Err(crate::Error::ProtoVersionMismatch {
                got: self.version,
                expected: PROTO_VERSION,
            });
        }
        Ok(())
    }

    /// 从 JSON 字节解码并**先校验上限、再校验版本**（设计 §3.5 条 1/3）。
    ///
    /// 上限先于 `serde_json` 解析生效（畸形对端宣称巨额长度时不预分配、不失控）；
    /// 版本校验后于解析生效（能解析但版本不符同样拒绝）。消费方应统一走本入口，
    /// 避免各侧重复实现导致口径漂移。
    pub fn from_json_slice(body: &[u8]) -> crate::Result<Self> {
        if body.len() > MAX_FRAME_BYTES {
            return Err(crate::Error::FrameTooLarge {
                len: body.len(),
                limit: MAX_FRAME_BYTES,
            });
        }
        let frame: Self = serde_json::from_slice(body)?;
        frame.check_version()?;
        Ok(frame)
    }

    /// 编码为 JSON 字节并**在发出前自检尺寸**（Important 3：编码侧守卫，与解码侧同源同值）。
    ///
    /// 发布方（mupcd）**必须**走本入口而非裸 `serde_json::to_vec`：否则
    /// 10 条 × 7 KB 告警消息 = 70 KB 会「正常发出」，对端整帧丢弃后
    /// 画面停在旧帧且发布方**无法定位责任方**。
    ///
    /// 两级判据（都由本函数给出 `Err`，附带可定位信息）：
    /// 1. 逐条告警消息长度 ≤ [`MAX_ALARM_MESSAGE_BYTES`]（定位到 `index`）；
    /// 2. 整帧字节数 ≤ [`MAX_FRAME_BYTES`]（与 [`Self::from_json_slice`] 同值）。
    pub fn to_json_slice(&self) -> crate::Result<Vec<u8>> {
        for (index, item) in self.alarms.items.iter().enumerate() {
            if item.message.len() > MAX_ALARM_MESSAGE_BYTES {
                return Err(crate::Error::AlarmMessageTooLong {
                    index,
                    len: item.message.len(),
                    limit: MAX_ALARM_MESSAGE_BYTES,
                });
            }
        }
        let body = serde_json::to_vec(self)?;
        if body.len() > MAX_FRAME_BYTES {
            return Err(crate::Error::FrameTooLarge {
                len: body.len(),
                limit: MAX_FRAME_BYTES,
            });
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- RunState：from_raw 越界 → None，合法值 → 对应变体 ----
    #[test]
    fn run_state_from_raw_valid_values() {
        assert_eq!(RunState::from_raw(0), Some(RunState::Stop));
        assert_eq!(RunState::from_raw(1), Some(RunState::Standby));
        assert_eq!(RunState::from_raw(2), Some(RunState::Charge));
        assert_eq!(RunState::from_raw(3), Some(RunState::Discharge));
    }

    #[test]
    fn run_state_from_raw_out_of_range_is_none() {
        assert_eq!(RunState::from_raw(4), None);
        assert_eq!(RunState::from_raw(0xFFFF), None);
        // u16 上限
        assert_eq!(RunState::from_raw(65535), None);
    }

    #[test]
    fn run_state_display_name_matches_prd_words() {
        assert_eq!(RunState::Stop.display_name(), "停机");
        assert_eq!(RunState::Standby.display_name(), "待机");
        assert_eq!(RunState::Charge.display_name(), "充电");
        assert_eq!(RunState::Discharge.display_name(), "放电");
    }

    // ---- RunState：JSON 以判别数 u8 传输（设计 §3.3）----
    #[test]
    fn run_state_json_roundtrip_as_number() {
        assert_eq!(serde_json::to_string(&RunState::Charge).unwrap(), "2");
        assert_eq!(serde_json::to_string(&RunState::Stop).unwrap(), "0");
        let back: RunState = serde_json::from_str("3").unwrap();
        assert_eq!(back, RunState::Discharge);
    }

    #[test]
    fn run_state_json_out_of_range_rejected() {
        let err = serde_json::from_str::<RunState>("7").unwrap_err();
        assert!(err.to_string().contains("invalid RunState"));
    }

    // ---- SocSource：JSON snake_case + UI 展示名 ----
    #[test]
    fn soc_source_json_snake_case() {
        assert_eq!(
            serde_json::to_string(&SocSource::PcsReg1010).unwrap(),
            "\"pcs_reg1010\""
        );
        assert_eq!(serde_json::to_string(&SocSource::Bms).unwrap(), "\"bms\"");
        assert_eq!(serde_json::to_string(&SocSource::Lost).unwrap(), "\"lost\"");
    }

    #[test]
    fn soc_source_display_name() {
        assert_eq!(SocSource::Bms.display_name(), "BMS");
        assert_eq!(SocSource::PcsReg1010.display_name(), "PCS(REG1010)");
        assert_eq!(SocSource::Lost.display_name(), "SOC 源失效");
    }

    // ---- FieldFlag：JSON snake_case ----
    #[test]
    fn field_flag_json_snake_case() {
        assert_eq!(serde_json::to_string(&FieldFlag::Valid).unwrap(), "\"valid\"");
        assert_eq!(
            serde_json::to_string(&FieldFlag::NotRead).unwrap(),
            "\"not_read\""
        );
        assert_eq!(
            serde_json::to_string(&FieldFlag::Offline).unwrap(),
            "\"offline\""
        );
        assert_eq!(
            serde_json::to_string(&FieldFlag::RangeError).unwrap(),
            "\"range_error\""
        );
    }

    // ---- Field：None v 序列化为 null ----
    #[test]
    fn field_with_none_v_serializes_null() {
        let f = Field {
            v: None,
            flag: FieldFlag::NotRead,
        };
        assert_eq!(
            serde_json::to_string(&f).unwrap(),
            r#"{"v":null,"flag":"not_read"}"#
        );
    }

    // ---- DisplayFrame：JSON 全往返（含 Option None）----
    /// 设计 §3.1 JSON 示例帧（v2；v1 段 + 四新段，run_state 为数值 2、soc 65.0）。
    const SAMPLE_JSON: &str = r#"{
        "version": 2,
        "seq": 123,
        "ts_ms": 1757412000000,
        "soc": 65.0,
        "soc_source": "pcs_reg1010",
        "soc_flag": "valid",
        "run_state": 2,
        "pcs_online": true,
        "p_phase": [ {"v":12.3,"flag":"valid"}, {"v":11.8,"flag":"valid"}, {"v":12.0,"flag":"valid"} ],
        "p_total": {"v":36.1,"flag":"valid"},
        "i_phase": [ {"v":22.5,"flag":"valid"}, {"v":22.1,"flag":"valid"}, {"v":22.3,"flag":"valid"} ],
        "inconsistency": false,
        "device": {
            "ts_ms": 1757412000000,
            "uptime_secs": 3600,
            "cpu_temp_c": 48.5,
            "mem_used_pct": 31.2,
            "iec104": "connected",
            "intercore": "connecting",
            "hmi_channel": "unknown",
            "control_source": "local_strategy"
        },
        "alarms": {
            "ts_ms": 1757412000000,
            "available": true,
            "items": [ {"ts_ms":1757411995000,"level":"warn","message":"核间链路抖动"} ]
        },
        "info": {
            "firmware_version": "0.1.0",
            "build_time": null,
            "model": "BECG-3568",
            "serial": null,
            "service_scope": "loopback_only",
            "mgmt_ipv4": "192.168.3.118"
        },
        "interlock": {
            "ts_ms": 1757412000000,
            "available": true,
            "enabled": true,
            "latched": false,
            "stop_failed": false,
            "sources": [ {"name":"estop","tripped":false} ],
            "fault_lamp": null,
            "run_lamp": true,
            "release_hold_secs": 30
        }
    }"#;

    #[test]
    fn display_frame_roundtrip_sample() {
        let f: DisplayFrame = serde_json::from_str(SAMPLE_JSON).unwrap();
        // 字段语义断言
        assert_eq!(f.version, PROTO_VERSION);
        assert_eq!(f.seq, 123);
        assert_eq!(f.soc, Some(65.0));
        assert_eq!(f.soc_source, SocSource::PcsReg1010);
        assert_eq!(f.soc_flag, FieldFlag::Valid);
        assert_eq!(f.run_state, Some(RunState::Charge));
        assert!(f.pcs_online);
        assert!(!f.inconsistency);
        assert_eq!(f.p_phase[0].v, Some(12.3));
        assert_eq!(f.i_phase[2].v, Some(22.3));
        // v2 段：device / alarms / info / interlock
        assert_eq!(f.device.uptime_secs, Some(3600));
        assert_eq!(f.device.cpu_temp_c, Some(48.5));
        assert_eq!(f.device.iec104, LinkState::Connected);
        assert_eq!(f.device.intercore, LinkState::Connecting);
        assert_eq!(f.device.hmi_channel, LinkState::Unknown);
        assert_eq!(f.device.control_source, ControlSource::LocalStrategy);
        assert!(f.alarms.available);
        assert_eq!(f.alarms.items.len(), 1);
        assert_eq!(f.alarms.items[0].level, AlarmLevel::Warn);
        assert_eq!(f.alarms.items[0].message, "核间链路抖动");
        assert_eq!(f.info.firmware_version, "0.1.0");
        assert_eq!(f.info.build_time, None, "build_time null → None（EDGE-16）");
        assert_eq!(f.info.serial, None, "serial null → None（不臆造）");
        assert_eq!(f.info.service_scope, ServiceScope::LoopbackOnly);
        assert_eq!(f.info.mgmt_ipv4.as_deref(), Some("192.168.3.118"));
        assert!(f.interlock.available && f.interlock.enabled);
        assert_eq!(f.interlock.fault_lamp, None, "fault_lamp null → None（未知）");
        assert_eq!(f.interlock.run_lamp, Some(true));
        assert_eq!(f.interlock.release_hold_secs, 30);
        assert_eq!(f.interlock.sources[0].name, "estop");
        // 编码后回解码 == 原帧（稳定往返）
        let encoded = serde_json::to_string(&f).unwrap();
        let dec: DisplayFrame = serde_json::from_str(&encoded).unwrap();
        assert_eq!(dec, f);
    }

    /// 版本一致性：帧 `version != PROTO_VERSION` → 必须拒绝（设计 §3.5 条 1 / PRD §4.4.1）。
    #[test]
    fn frame_version_mismatch_is_rejected() {
        // 字面量 JSON：同结构、仅 version 为旧值 1
        let v1_json = SAMPLE_JSON.replace("\"version\": 2", "\"version\": 1");
        let bytes = v1_json.as_bytes();
        let err = DisplayFrame::from_json_slice(bytes).unwrap_err();
        assert!(
            matches!(
                err,
                crate::Error::ProtoVersionMismatch { got: 1, expected: 2 }
            ),
            "版本不一致必须 Err(ProtoVersionMismatch)，实际: {err:?}"
        );
        // 逐字含断言：错误文案暴露 got/expected，便于现场定位
        assert!(err.to_string().contains("mismatch"));

        // 未来版本（version=3）同样拒绝，不得静默接受
        let v3_json = SAMPLE_JSON.replace("\"version\": 2", "\"version\": 3");
        assert!(matches!(
            DisplayFrame::from_json_slice(v3_json.as_bytes()),
            Err(crate::Error::ProtoVersionMismatch { got: 3, .. })
        ));
    }

    /// 畸形帧防护：超过 64 KB 上限 → Err(FrameTooLarge)，且不进入 JSON 解析。
    #[test]
    fn frame_over_size_limit_is_rejected() {
        let huge = vec![b' '; MAX_FRAME_BYTES + 1];
        let err = DisplayFrame::from_json_slice(&huge).unwrap_err();
        assert!(matches!(
            err,
            crate::Error::FrameTooLarge { len, limit }
                if len == MAX_FRAME_BYTES + 1 && limit == MAX_FRAME_BYTES
        ));
    }

    /// Important 3：编码侧尺寸守卫，与解码侧**同源同值**（`MAX_FRAME_BYTES`）。
    /// 恰好在上限内 → Ok（且可被解码侧接受）；超一字节 → Err(FrameTooLarge)。
    #[test]
    fn encode_side_size_guard_shares_single_limit_with_decode() {
        let mut f: DisplayFrame = serde_json::from_str(SAMPLE_JSON).unwrap();
        // 用无长度约束的字符串字段精确填充到限值
        f.info.firmware_version = String::new();
        let base_len = f.to_json_slice().unwrap().len();
        let pad = MAX_FRAME_BYTES - base_len;

        f.info.firmware_version = "x".repeat(pad);
        let body = f.to_json_slice().expect("恰好在上限内须 Ok");
        assert_eq!(body.len(), MAX_FRAME_BYTES, "填充应精确命中上限");
        assert!(
            DisplayFrame::from_json_slice(&body).is_ok(),
            "同一常量下编码产物必须能过解码上限"
        );

        // 超一字节 → Err，且 len/limit 与解码侧判据口径一致
        f.info.firmware_version = "x".repeat(pad + 1);
        assert!(matches!(
            f.to_json_slice(),
            Err(crate::Error::FrameTooLarge { len, limit })
                if len == MAX_FRAME_BYTES + 1 && limit == MAX_FRAME_BYTES
        ));
        // 解码侧对同一超限体同样拒绝（同源同值回归锚点）
        let over = f.to_json_slice();
        assert!(matches!(over, Err(crate::Error::FrameTooLarge { .. })));
    }

    /// Important 3：单条告警消息无长度约束 → 10 条长文可把整帧顶出上限，
    /// 对端整帧丢弃却无法定位责任方。编码入口须先逐条拦截并给出 `index`。
    #[test]
    fn alarm_message_length_is_bounded_at_encode_time() {
        let mk = |n: usize| -> DisplayFrame {
            let mut f: DisplayFrame = serde_json::from_str(SAMPLE_JSON).unwrap();
            f.alarms = AlarmsSection {
                ts_ms: 1,
                available: true,
                items: (0..2)
                    .map(|i| AlarmItem {
                        ts_ms: 1,
                        level: AlarmLevel::Warn,
                        message: if i == 1 {
                            "x".repeat(n)
                        } else {
                            "ok".to_string()
                        },
                    })
                    .collect(),
            };
            f
        };
        // 恰好在上限内 → Ok
        assert!(mk(MAX_ALARM_MESSAGE_BYTES).to_json_slice().is_ok());
        // 超一字节 → Err，且可定位到具体条目下标
        let err = mk(MAX_ALARM_MESSAGE_BYTES + 1).to_json_slice().unwrap_err();
        assert!(matches!(
            err,
            crate::Error::AlarmMessageTooLong { index: 1, len, limit }
                if len == MAX_ALARM_MESSAGE_BYTES + 1 && limit == MAX_ALARM_MESSAGE_BYTES
        ));
    }

    /// Minor 9：必需段（如 `p_phase`）缺失必须 `Err`，**不得补 0** 或补默认结构。
    #[test]
    fn missing_required_segments_are_rejected_not_zero_filled() {
        for key in [
            "p_phase",
            "p_total",
            "i_phase",
            "soc_source",
            "soc_flag",
            "pcs_online",
            "inconsistency",
            "seq",
            "ts_ms",
            "version",
        ] {
            let v: serde_json::Value = serde_json::from_str(SAMPLE_JSON).unwrap();
            let mut obj = v.as_object().unwrap().clone();
            obj.remove(key);
            let res = serde_json::from_value::<DisplayFrame>(serde_json::Value::Object(obj));
            assert!(
                res.is_err(),
                "缺少必需段 `{key}` 必须 Err（不得补 0 / 默认），实际: {res:?}"
            );
        }
        // 三相数组长度不足 3 → Err（不得补零到 3 个「Valid 0.0」）
        let v: serde_json::Value = serde_json::from_str(SAMPLE_JSON).unwrap();
        let mut obj = v.as_object().unwrap().clone();
        obj.insert(
            "p_phase".to_string(),
            serde_json::json!([{"v": 12.3, "flag": "valid"}]),
        );
        assert!(
            serde_json::from_value::<DisplayFrame>(serde_json::Value::Object(obj)).is_err(),
            "三相数组长度 ≠ 3 必须 Err"
        );
    }

    /// 向后兼容（设计 §3.1 JSON 兼容性说明）：v1 帧（无四新段）→ v2 类型，
    /// 新段取 Default，且 `available=false` 落在「不可用」降级语义上（**不**伪装正常 / 无告警）。
    #[test]
    fn v1_frame_deserializes_into_v2_with_degraded_defaults() {
        let v1_json = SAMPLE_JSON.replace("\"version\": 2", "\"version\": 1");
        // 去掉四新段，模拟真实 v1 发布方
        let f: DisplayFrame = {
            let v: serde_json::Value = serde_json::from_str(&v1_json).unwrap();
            let mut obj = v.as_object().unwrap().clone();
            for k in ["device", "alarms", "info", "interlock"] {
                obj.remove(k);
            }
            serde_json::from_value(serde_json::Value::Object(obj)).unwrap()
        };
        assert_eq!(f.soc, Some(65.0), "v1 段原样保留");
        assert!(!f.alarms.available, "告警源缺失 → 不可用（EDGE-09）");
        assert!(f.alarms.items.is_empty());
        assert!(!f.interlock.available, "联锁态缺失 → 不可用（IL-01.6）");
        assert!(!f.interlock.latched, "缺失绝不等于「未联锁」被误读为真值");
        assert_eq!(f.device.iec104, LinkState::Unknown, "链路缺失 → 未知（F6.5）");
        assert_eq!(f.device.control_source, ControlSource::Unknown);
        assert_eq!(f.info.firmware_version, "");
        assert_eq!(f.info.mgmt_ipv4, None);
        // 注意：v1 帧的 version=1 → 消费方须走 check_version 拒绝，而非按 v2 语义展示
        assert!(f.check_version().is_err());
    }

    #[test]
    fn display_frame_none_fields_roundtrip() {
        // 全降级态：soc=None/soc_source=lost、run_state=None、数值 v 全 None
        let f = DisplayFrame {
            version: PROTO_VERSION,
            seq: 0,
            ts_ms: 0,
            soc: None,
            soc_source: SocSource::Lost,
            soc_flag: FieldFlag::Offline,
            run_state: None,
            pcs_online: false,
            p_phase: [Field {
                v: None,
                flag: FieldFlag::Offline,
            }; 3],
            p_total: Field {
                v: None,
                flag: FieldFlag::NotRead,
            },
            i_phase: [Field {
                v: None,
                flag: FieldFlag::Offline,
            }; 3],
            inconsistency: false,
            device: DeviceSection::default(),
            alarms: AlarmsSection::default(),
            info: InfoSection::default(),
            interlock: InterlockSection::default(),
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"soc\":null"));
        assert!(json.contains("\"soc_source\":\"lost\""));
        assert!(json.contains("\"run_state\":null"));
        // 新段 None 同样序列化为 null（渲染端显 `--` / 「未提供」，禁补 0 / 臆造）
        assert!(json.contains("\"uptime_secs\":null"));
        assert!(json.contains("\"build_time\":null"));
        assert!(json.contains("\"mgmt_ipv4\":null"));
        assert!(json.contains("\"fault_lamp\":null"));
        let back: DisplayFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn display_frame_unknown_fields_tolerated() {
        // 设计 §1 前向兼容：未知字段容忍，结构体默认忽略未知键。
        let json = r#"{"version":2,"seq":1,"ts_ms":1,"soc":null,"soc_source":"lost",
            "soc_flag":"offline","run_state":null,"pcs_online":false,
            "p_phase":[{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"}],
            "p_total":{"v":null,"flag":"not_read"},
            "i_phase":[{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"}],
            "inconsistency":false,"future_field":123,
            "device":{"future_sub":1},"alarms":{"future":[]}}"#;
        let f: DisplayFrame = serde_json::from_str(json).unwrap();
        assert_eq!(f.soc_source, SocSource::Lost);
        // 新段内的未知键亦容忍，已知字段取默认
        assert_eq!(f.device.iec104, LinkState::Unknown);
        assert!(!f.alarms.available);
    }

    // ---- v2 枚举：JSON snake_case + 缺省态（不伪装正常）----
    #[test]
    fn link_state_json_and_default_never_means_connected() {
        assert_eq!(
            serde_json::to_string(&LinkState::NotConfigured).unwrap(),
            "\"not_configured\""
        );
        assert_eq!(serde_json::to_string(&LinkState::Connected).unwrap(), "\"connected\"");
        // F6.5：缺省/不可得绝不落在「已连接」
        assert_eq!(LinkState::default(), LinkState::Unknown);
        assert_eq!(LinkState::default().display_name(), "未知");
        assert_eq!(LinkState::NotConfigured.display_name(), "未配置");
        assert_eq!(LinkState::Connected.display_name(), "已连接");
    }

    #[test]
    fn control_source_json_and_text() {
        assert_eq!(
            serde_json::to_string(&ControlSource::LocalStrategy).unwrap(),
            "\"local_strategy\""
        );
        assert_eq!(
            serde_json::to_string(&ControlSource::AiDisabled).unwrap(),
            "\"ai_disabled\""
        );
        // 缺省不臆造下发源
        assert_eq!(ControlSource::default(), ControlSource::Unknown);
        // PRD §3.1 F6 备注的固定文案
        assert_eq!(
            ControlSource::AiDisabled.display_name(),
            "AI 引擎已停用，本地策略引擎为默认下发源"
        );
    }

    #[test]
    fn service_scope_json_is_loopback_only() {
        assert_eq!(
            serde_json::to_string(&ServiceScope::LoopbackOnly).unwrap(),
            "\"loopback_only\""
        );
        assert_eq!(ServiceScope::default(), ServiceScope::LoopbackOnly);
        assert_eq!(ServiceScope::default().display_name(), "仅回环 127.0.0.1");
    }

    #[test]
    fn alarm_level_json_and_text() {
        assert_eq!(serde_json::to_string(&AlarmLevel::Error).unwrap(), "\"error\"");
        assert_eq!(serde_json::to_string(&AlarmLevel::Warn).unwrap(), "\"warn\"");
        assert_eq!(serde_json::to_string(&AlarmLevel::Info).unwrap(), "\"info\"");
        assert_eq!(AlarmLevel::Error.display_name(), "ERROR");
        assert_eq!(AlarmLevel::Warn.display_name(), "WARN");
        assert_eq!(AlarmLevel::Info.display_name(), "INFO");
    }

    /// 降级语义：`available=false` 的段不得被解读成「正常 / 无告警 / 未联锁」
    /// （EDGE-09 / IL-01.6）。
    #[test]
    fn degraded_sections_are_explicitly_unavailable() {
        let a = AlarmsSection::default();
        assert!(!a.available, "默认告警段 = 源不可用（≠ 无告警）");
        let i = InterlockSection::default();
        assert!(!i.available, "默认联锁段 = 状态不可用（≠ 未联锁）");
        let d = DeviceSection::default();
        assert_eq!(d.iec104.display_name(), "未知");
        assert_eq!(d.hmi_channel.display_name(), "未知");
        assert!(d.uptime_secs.is_none() && d.cpu_temp_c.is_none() && d.mem_used_pct.is_none());
    }

    /// `cap_items` 落地「items ≤ alarm_page_size」（设计 §3.1「≤10」/ §4.9 `alarm_page_size`）。
    #[test]
    fn cap_items_enforces_page_size_without_touching_availability() {
        let mk = |n: usize| AlarmsSection {
            ts_ms: 1,
            available: true,
            items: (0..n)
                .map(|i| AlarmItem {
                    ts_ms: 1000 - i as u64,
                    level: AlarmLevel::Warn,
                    message: format!("a{i}"),
                })
                .collect(),
        };
        let mut over = mk(15);
        over.cap_items(10);
        assert_eq!(over.items.len(), 10, "超限须截断");
        assert_eq!(over.items[0].message, "a0", "保留时间倒序首个（最新）");
        assert!(over.available, "截断不得改动 available（≠ 源不可用）");

        let mut under = mk(3);
        under.cap_items(10);
        assert_eq!(under.items.len(), 3, "不足上限不补齐、不丢条目");

        // 边界：恰为上限
        let mut exact = mk(10);
        exact.cap_items(10);
        assert_eq!(exact.items.len(), 10);
        // 0 条上限属配置层拒绝项（validate），此处仅保证不 panic
        let mut empty = AlarmsSection::default();
        empty.cap_items(0);
        assert!(empty.items.is_empty());
    }
}
