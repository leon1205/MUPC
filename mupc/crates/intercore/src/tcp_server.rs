//! 核间通信 TCP 服务器

use mupc_common::{ErrorCode, MupcError};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};

use super::{HeartbeatManager, IntercoreFrame, IntercoreFrameType, FRAME_FIXED_LENGTH};
use crate::transport::{IntercoreTransport, TcpTransport};

/// 单次 `read()` 的暂存缓冲大小（仅用于把字节搬进累积缓冲，与帧长无关）
const READ_CHUNK_LEN: usize = 512;

/// 安全覆盖触发原因的默认值
const SAFETY_OVERRIDE_REASON_UNKNOWN: &str = "unknown";
/// 安全覆盖恢复条件的默认值
const SAFETY_OVERRIDE_RECOVERY_TIMER_EXPIRED: &str = "timer_expired";

/// 核间通信配置
#[derive(Debug, Clone)]
pub struct IntercoreConfig {
    /// 监听地址
    pub listen_addr: String,
    /// 监听端口
    pub listen_port: u16,
    /// 心跳间隔（毫秒）
    pub heartbeat_interval_ms: u64,
    /// 看门狗超时（毫秒）
    pub watchdog_timeout_ms: u64,
    /// 最大电池放电功率 (kW)，用于安全覆盖时的功率限制
    pub max_batt_power_kw: f64,
}

impl Default for IntercoreConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 2500,
            heartbeat_interval_ms: 1000,
            watchdog_timeout_ms: 10000,
            max_batt_power_kw: 50.0,
        }
    }
}

// ============================================================================
// P2-15: ControlCmd JSON Payload 解析
// ============================================================================

/// 控制指令 JSON Payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCmdPayload {
    #[serde(rename = "p_batt_set")]
    pub p_batt_set: Option<f64>,
    #[serde(rename = "q_batt_set")]
    pub q_batt_set: Option<f64>,
    #[serde(rename = "ai_ready")]
    pub ai_ready: Option<bool>,
    #[serde(rename = "strategy_mode")]
    pub strategy_mode: Option<String>,
    #[serde(rename = "timestamp_ms")]
    pub timestamp_ms: Option<u64>,
}

impl ControlCmdPayload {
    /// 从 JSON 字节解析
    pub fn from_json(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    /// 序列化为 JSON 字节
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}

/// 控制指令 JSON Payload v2.0（双参数模式）
///
/// v2.0 变更：
/// - p_batt_set → p_ref（有功基准点）
/// - q_batt_set → k_droop（电压-有功下垂系数）
/// - 新增 frame_version 字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCmdPayloadV2 {
    #[serde(rename = "p_ref")]
    pub p_ref: Option<f64>,
    #[serde(rename = "k_droop")]
    pub k_droop: Option<f64>,
    /// 注：load_shedding 和 pv_limit 不通过核间通信发送，
    /// 它们通过 SouthCommandDispatcher 发送到南向设备（光伏逆变器、负荷控制装置）
    #[serde(rename = "ai_ready")]
    pub ai_ready: Option<bool>,
    #[serde(rename = "strategy_mode")]
    pub strategy_mode: Option<String>,
    #[serde(rename = "timestamp_ms")]
    pub timestamp_ms: Option<u64>,
    /// 帧版本号，用于区分 v1.x 和 v2.0
    #[serde(rename = "frame_version")]
    pub frame_version: Option<u8>,
}

impl Default for ControlCmdPayloadV2 {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlCmdPayloadV2 {
    pub const FRAME_VERSION: u8 = 2;

    pub fn new() -> Self {
        Self {
            p_ref: None,
            k_droop: None,
            ai_ready: None,
            strategy_mode: None,
            timestamp_ms: None,
            frame_version: Some(Self::FRAME_VERSION),
        }
    }

    /// 从 JSON 字节解析
    pub fn from_json(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    /// 序列化为 JSON 字节
    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// 检测帧版本
    pub fn detect_version(data: &[u8]) -> Result<u8, serde_json::Error> {
        match Self::from_json(data) {
            Ok(payload) => Ok(payload.frame_version.unwrap_or(1)),
            Err(_) => Ok(1), // 解析失败假设为 v1.x
        }
    }
}

/// 控制指令 JSON Payload v3.0（分相模式）
///
/// v3.0 新增：分相 P/Q 设定（台区储能治理策略下发），兼容 v2 双参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCmdPayloadV3 {
    #[serde(rename = "frame_version")]
    pub frame_version: Option<u8>, // = 3
    #[serde(rename = "p_ref")]
    pub p_ref: Option<f64>, // 兼容 v2 双参数（分相模式可为 None）
    #[serde(rename = "k_droop")]
    pub k_droop: Option<f64>,
    #[serde(rename = "phase_p_set")]
    pub phase_p_set: Option<[f64; 3]>, // 分相有功 (kW)，索引 0/1/2 = A/B/C 相
    #[serde(rename = "phase_q_set")]
    pub phase_q_set: Option<[f64; 3]>, // 分相无功 (kVAr)，索引 0/1/2 = A/B/C 相
    #[serde(rename = "ai_ready")]
    pub ai_ready: Option<bool>,
    #[serde(rename = "strategy_mode")]
    pub strategy_mode: Option<String>,
    #[serde(rename = "timestamp_ms")]
    pub timestamp_ms: Option<u64>,
}

impl ControlCmdPayloadV3 {
    pub const FRAME_VERSION: u8 = 3;

    pub fn new() -> Self {
        Self {
            frame_version: Some(Self::FRAME_VERSION),
            p_ref: None,
            k_droop: None,
            phase_p_set: None,
            phase_q_set: None,
            ai_ready: None,
            strategy_mode: None,
            timestamp_ms: None,
        }
    }

    pub fn from_json(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// 按 frame_version 字段检测版本（1/2/3）；解析失败返回 Err，由调用方回退为 v1
    pub fn detect_version(data: &[u8]) -> Result<u8, serde_json::Error> {
        let v: serde_json::Value = serde_json::from_slice(data)?;
        Ok(v["frame_version"].as_u64().map(|x| x as u8).unwrap_or(1))
    }
}

impl Default for ControlCmdPayloadV3 {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// v2.10: DataUploadPayload 和 SafetyOverridePayload
// ============================================================================

/// 数据上传 Payload（v2.10 新增）
///
/// 实时控制模块通过 DataUpload 帧上报系统状态，
/// 包括 q_realtime_margin（实时模块剩余无功容量比例）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataUploadPayload {
    #[serde(rename = "frame_version")]
    pub frame_version: Option<u8>,
    #[serde(rename = "timestamp_ms")]
    pub timestamp_ms: Option<u64>,
    /// 实时模块剩余无功容量比例 [0.0, 1.0]
    /// 0 = 无功打满，1 = 完全空闲
    #[serde(rename = "q_realtime_margin")]
    pub q_realtime_margin: Option<f64>,
    #[serde(rename = "battery_soc")]
    pub battery_soc: Option<f64>,
    #[serde(rename = "voltage_phase_a")]
    pub voltage_phase_a: Option<f64>,
    #[serde(rename = "voltage_phase_b")]
    pub voltage_phase_b: Option<f64>,
    #[serde(rename = "voltage_phase_c")]
    pub voltage_phase_c: Option<f64>,
    #[serde(rename = "battery_power")]
    pub battery_power: Option<f64>,
}

impl DataUploadPayload {
    pub const FRAME_VERSION: u8 = 1;

    /// 从 JSON 字节解析
    pub fn from_json(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    /// 获取 q_realtime_margin，超时返回 None
    pub fn q_realtime_margin(&self) -> Option<f64> {
        self.q_realtime_margin
    }

    /// 校验并获取 q_realtime_margin（clamp 到 [0.0, 1.0]）
    pub fn q_realtime_margin_clamped(&self) -> Option<f64> {
        self.q_realtime_margin.map(|v| v.clamp(0.0, 1.0))
    }
}

/// 安全覆盖 Payload（v2.10 新增）
///
/// 当实时控制模块检测到电压越限且无功耗尽时，
/// 临时覆盖 AI 有功指令的紧急事件帧。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyOverridePayload {
    #[serde(rename = "frame_version")]
    pub frame_version: Option<u8>,
    #[serde(rename = "timestamp_ms")]
    pub timestamp_ms: Option<u64>,
    /// 触发原因
    #[serde(rename = "trigger_reason")]
    pub trigger_reason: Option<String>,
    #[serde(rename = "voltage_phase_a")]
    pub voltage_phase_a: Option<f64>,
    #[serde(rename = "voltage_phase_b")]
    pub voltage_phase_b: Option<f64>,
    #[serde(rename = "voltage_phase_c")]
    pub voltage_phase_c: Option<f64>,
    /// 无功裕度（几乎耗尽）
    #[serde(rename = "q_realtime_margin")]
    pub q_realtime_margin: Option<f64>,
    /// 强制放电功率 (kW)
    #[serde(rename = "override_p_ref")]
    pub override_p_ref: Option<f64>,
    /// 覆盖持续时间 (ms)
    #[serde(rename = "override_duration_ms")]
    pub override_duration_ms: Option<u64>,
    /// 恢复条件
    #[serde(rename = "recovery_condition")]
    pub recovery_condition: Option<String>,
}

impl SafetyOverridePayload {
    pub const FRAME_VERSION: u8 = 1;

    /// 从 JSON 字节解析
    pub fn from_json(data: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(data)
    }

    pub fn trigger_reason(&self) -> &str {
        self.trigger_reason
            .as_deref()
            .unwrap_or(SAFETY_OVERRIDE_REASON_UNKNOWN)
    }

    pub fn is_active(&self) -> bool {
        self.override_p_ref.is_some()
    }

    /// 校验 override_p_ref 不超过 max_batt_discharge_power
    pub fn clamp_override_p_ref(&self, max_batt_discharge_power: f64) -> f64 {
        self.override_p_ref
            .map(|v| v.clamp(-max_batt_discharge_power, max_batt_discharge_power))
            .unwrap_or(0.0)
    }

    /// 校验 override_duration_ms 不超过 10000ms
    pub fn clamp_override_duration_ms(&self) -> u64 {
        self.override_duration_ms
            .map(|v| v.min(10000))
            .unwrap_or(5000)
    }
}

/// 核间连接状态的**一致性快照**（E-16）
///
/// 字段间不变式（快照内恒成立）：
/// - `safety_override_active == safety_override_reason.is_some()`
///   —— 修前 12 把独立 `RwLock` 逐字段读取，会读到 `active=true` 而 `reason=None`
///   的撕裂值；单锁后快照一次取全，不再撕裂。
#[derive(Debug, Clone, Default)]
pub struct IntercoreConnectionSnapshot {
    /// 最后收到的有效 p_ref
    pub last_valid_p_ref: Option<f64>,
    /// 最后收到的有效 k_droop
    pub last_valid_k_droop: Option<f64>,
    /// 最后心跳时间戳
    pub last_heartbeat_ms: u64,
    /// 连接状态
    pub connected: bool,
    // v2.10 新增字段
    /// 最后收到的 q_realtime_margin
    pub last_q_realtime_margin: Option<f64>,
    /// q_realtime_margin 连续缺失计数
    pub q_margin_missing_count: u32,
    /// 安全覆盖激活标志
    pub safety_override_active: bool,
    /// 安全覆盖触发原因
    pub safety_override_reason: Option<String>,
    /// 安全覆盖强制放电功率 (kW)
    pub safety_override_p_ref: Option<f64>,
    /// 安全覆盖持续时间 (ms)
    pub safety_override_duration_ms: u64,
    /// 安全覆盖恢复条件
    pub safety_override_recovery: Option<String>,
    /// 安全覆盖触发计数（用于频率限制）
    pub safety_override_count: u32,
    /// 安全覆盖首次触发时间戳（用于 1 分钟窗口计算）
    pub safety_override_first_ts: Option<i64>,
}

/// 核间通信状态（用于通信中断检测和降级）
///
/// **单锁**（E-16）：全部字段同处一把 `RwLock` 下。修前每个字段各持一把 `RwLock`，
/// 「读一遍全部字段」不是原子操作 ⇒ 快照可撕裂。现所有读写方法都在**同一次**加锁内
/// 完成，并提供 [`Self::snapshot`] 一次取全。
pub struct IntercoreConnectionState {
    inner: RwLock<IntercoreConnectionSnapshot>,
}

impl Default for IntercoreConnectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl IntercoreConnectionState {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(IntercoreConnectionSnapshot::default()),
        }
    }

    /// 一致性快照：一次加锁读全，字段间不变式成立（E-16）
    pub async fn snapshot(&self) -> IntercoreConnectionSnapshot {
        self.inner.read().await.clone()
    }

    /// 更新收到的双参数
    pub async fn update_valid_params(&self, p_ref: f64, k_droop: f64) {
        let mut s = self.inner.write().await;
        s.last_valid_p_ref = Some(p_ref);
        s.last_valid_k_droop = Some(k_droop);
    }

    /// 获取最后有效的双参数（通信中断时使用）
    pub async fn get_last_valid_params(&self) -> (Option<f64>, Option<f64>) {
        let s = self.inner.read().await;
        (s.last_valid_p_ref, s.last_valid_k_droop)
    }

    /// 检查是否已收到有效参数
    pub async fn has_valid_params(&self) -> bool {
        let s = self.inner.read().await;
        s.last_valid_p_ref.is_some() && s.last_valid_k_droop.is_some()
    }

    /// 设置连接状态
    pub async fn set_connected(&self, connected: bool) {
        self.inner.write().await.connected = connected;
    }

    /// 获取连接状态
    pub async fn is_connected(&self) -> bool {
        self.inner.read().await.connected
    }

    /// 更新 q_realtime_margin（v2.10）——置值与清零计数在同一次加锁内完成
    pub async fn update_q_margin(&self, q_margin: f64) {
        let mut s = self.inner.write().await;
        s.last_q_realtime_margin = Some(q_margin);
        s.q_margin_missing_count = 0;
    }

    /// 增加 q_margin 缺失计数（v2.10）
    pub async fn increment_q_margin_missing(&self) -> u32 {
        let mut s = self.inner.write().await;
        s.q_margin_missing_count += 1;
        s.q_margin_missing_count
    }

    /// 获取最后有效的 q_margin（v2.10）
    pub async fn get_last_q_margin(&self) -> Option<f64> {
        self.inner.read().await.last_q_realtime_margin
    }

    /// 更新安全覆盖状态（v2.10）——5 个字段在一次加锁内成对写入
    pub async fn update_safety_override(
        &self,
        reason: &str,
        p_ref: f64,
        duration_ms: u64,
        recovery: &str,
    ) {
        let mut s = self.inner.write().await;
        s.safety_override_active = true;
        s.safety_override_reason = Some(reason.to_string());
        s.safety_override_p_ref = Some(p_ref);
        s.safety_override_duration_ms = duration_ms;
        s.safety_override_recovery = Some(recovery.to_string());
    }

    /// 清除安全覆盖状态（v2.10）——5 个字段在一次加锁内成对清除
    pub async fn clear_safety_override(&self) {
        let mut s = self.inner.write().await;
        s.safety_override_active = false;
        s.safety_override_reason = None;
        s.safety_override_p_ref = None;
        s.safety_override_duration_ms = 0;
        s.safety_override_recovery = None;
    }

    /// 检查并增加安全覆盖计数，返回是否超过频率限制（v2.10）
    /// 1 分钟内最多 3 次
    pub async fn check_and_increment_safety_override(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        let mut s = self.inner.write().await;

        // 检查 1 分钟窗口
        match s.safety_override_first_ts {
            Some(ts) if now - ts > 60000 => {
                // 窗口过期，重置计数
                s.safety_override_count = 0;
                s.safety_override_first_ts = Some(now);
            }
            None => s.safety_override_first_ts = Some(now),
            _ => {}
        }

        s.safety_override_count += 1;
        s.safety_override_count > 3 // 1 分钟内最多 3 次
    }
}

// ============================================================================
// P2-16: 指令超时重试和断连缓存
// ============================================================================

/// 指令发送配置
#[derive(Debug, Clone)]
pub struct CommandConfig {
    /// 超时时间（毫秒）
    pub timeout_ms: u64,
    /// 最大重试次数
    pub max_retries: u32,
}

impl Default for CommandConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            max_retries: 2,
        }
    }
}

/// 指令队列（支持断连缓存）
///
/// # ⚠️ 未接线（审查 E-10 / 技术债 U-76）
///
/// 本类型**没有驱动循环**：「发送 → 等 `ControlRsp` → 超时重试」的闭环在全仓不存在
/// 调用点（`grep CommandQueue` 仅命中 `lib.rs` 的重导出）。原因是核间 TCP 通道在生产
/// 路径暂无消费者，且 sim-bridge/HIL 侧不回 `ControlRsp`。与 10 号 PRD §4 的处理一致，
/// **保留为待接入设计**而非补一个无人调用的驱动循环；`#[allow(dead_code)]` 为显式标注
/// （本类型 `pub` 且经 `lib.rs` 重导出，编译器本不会报 dead_code）。
///
/// 接入时**必须**同时落地：超时源（`CommandConfig::timeout_ms`）、响应匹配（`seq_no`）
/// 与退避策略，否则 `retry_or_drop` 仍只是空转。
#[allow(dead_code)]
pub struct CommandQueue {
    pending: VecDeque<(Vec<u8>, u32)>, // (payload, retries_left)
    config: CommandConfig,
}

impl CommandQueue {
    pub fn new(config: CommandConfig) -> Self {
        Self {
            pending: VecDeque::new(),
            config,
        }
    }

    /// 添加指令到队列
    pub fn enqueue(&mut self, payload: Vec<u8>) {
        self.pending.push_back((payload, self.config.max_retries));
    }

    /// 获取下一个待发送指令
    pub fn dequeue(&mut self) -> Option<Vec<u8>> {
        self.pending.pop_front().map(|(p, _)| p)
    }

    /// 指令发送失败：重试计数 -1，**仍有余量则压回队首**，耗尽才丢弃。
    ///
    /// 修复前本方法体是 `let _ = payload;`（无条件静默丢弃），与其方法名/文档不符
    /// —— 属于「看起来在工作」的空壳（E-10）。现按文档语义实现；但因本类型**无驱动
    /// 循环**（见类型文档），生产路径仍不会调用它。
    pub fn retry_or_drop(&mut self, payload: Vec<u8>) {
        // 匹配队列中第一条同内容的待发项（简化：Phase 2+ 可改为按 seq_no 精确匹配）
        let found = self
            .pending
            .iter()
            .position(|(p, _)| p.as_slice() == payload.as_slice());
        if let Some(idx) = found {
            let (p, retries_left) = self.pending.remove(idx).expect("idx 来自 position");
            if retries_left > 0 {
                self.pending.push_front((p, retries_left - 1));
            }
        }
    }

    /// 待发送指令数
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// 队列是否为空
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// 核间通信服务器
pub struct IntercoreServer {
    config: IntercoreConfig,
    shutdown_tx: broadcast::Sender<()>,
    /// 指令发送配置（P2-16）
    cmd_config: CommandConfig,
}

impl IntercoreServer {
    /// 创建核间通信服务器
    pub fn new(config: IntercoreConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            shutdown_tx,
            cmd_config: CommandConfig::default(),
        }
    }

    /// 带指令配置创建服务器
    pub fn with_command_config(config: IntercoreConfig, cmd_config: CommandConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            shutdown_tx,
            cmd_config,
        }
    }

    /// 启动服务器
    pub async fn start(&self) -> Result<Arc<RwLock<HeartbeatManager>>, MupcError> {
        let addr = format!("{}:{}", self.config.listen_addr, self.config.listen_port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            MupcError::new(
                ErrorCode::ConnectionFailed,
                format!("Failed to bind {}: {}", addr, e),
                "intercore",
            )
        })?;

        info!("Intercore server listening on {}", addr);

        let heartbeat_manager = Arc::new(RwLock::new(HeartbeatManager::new(
            self.config.heartbeat_interval_ms,
            self.config.watchdog_timeout_ms,
        )));

        let shutdown_rx = self.shutdown_tx.subscribe();
        let cmd_config = self.cmd_config.clone();
        let max_batt_power_kw = self.config.max_batt_power_kw;

        // clone heartbeat_manager before it's moved into spawns
        let hb_for_listener = heartbeat_manager.clone();
        let hb_for_runner = heartbeat_manager.clone();

        // 接受连接任务
        let _listener_handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;

            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                info!("New intercore connection from {}", addr);
                                let heartbeat = hb_for_listener.clone();
                                let cfg = cmd_config.clone();
                                let intercore_state = Arc::new(IntercoreConnectionState::new());
                                let state_for_conn = intercore_state.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(stream, addr, heartbeat, cfg, state_for_conn, max_batt_power_kw).await {
                                        error!("Connection error from {}: {}", addr, e);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Accept error: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Intercore server shutting down");
                        break;
                    }
                }
            }
        });

        // 启动心跳管理器
        tokio::spawn(async move {
            hb_for_runner.read().await.run().await;
        });

        Ok(heartbeat_manager)
    }

    /// 处理连接
    async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        heartbeat: Arc<RwLock<HeartbeatManager>>,
        cmd_config: CommandConfig,
        intercore_state: Arc<IntercoreConnectionState>, // v2.10 新增
        max_batt_power_kw: f64,
    ) -> Result<(), MupcError> {
        let (read_half, mut write_half) = tokio::io::split(stream);

        // 发送连接注册
        let connect_frame = IntercoreFrame::new_connect();
        let frame_data = connect_frame.to_bytes()?;
        Self::send_with_timeout(&mut write_half, &frame_data, cmd_config.timeout_ms).await?;

        heartbeat.read().await.register_connection(addr);

        // 读取循环
        //
        // E-09：**必须跨 read 累积到完整帧再解析**。TCP 是字节流，单次 `read()` 完全
        // 可能只返回半帧（内核缓冲区边界与帧边界无关）；旧实现把 `read` 的返回值 `n`
        // 直接当帧长交给 `from_bytes`，半帧即被丢弃、且后续字节错位 ⇒ **流永久失步**。
        let mut reader = tokio::io::BufReader::new(read_half);
        let mut acc: Vec<u8> = Vec::with_capacity(FRAME_FIXED_LENGTH);

        loop {
            let frame_bytes = match Self::read_fixed_frame(&mut reader, &mut acc).await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    info!("Connection closed: {}", addr);
                    heartbeat.read().await.unregister_connection(addr);
                    break;
                }
                Err(e) => {
                    error!("Read error from {}: {}", addr, e);
                    heartbeat.read().await.unregister_connection(addr);
                    break;
                }
            };

            match IntercoreFrame::from_bytes(&frame_bytes) {
                Ok(frame) => {
                    match frame.header.frame_type {
                        IntercoreFrameType::HeartbeatReq | IntercoreFrameType::HeartbeatRsp => {
                            heartbeat.read().await.receive_heartbeat(addr).await;
                        }
                        IntercoreFrameType::ControlCmd => {
                            info!("Received control command from {}", addr);
                            if !frame.data.is_empty() {
                                // 统一版本分派：3=V3 分相，2=V2 双参数，其余=V1
                                let ver =
                                    ControlCmdPayloadV3::detect_version(&frame.data).unwrap_or(1);
                                match ver {
                                    3 => match ControlCmdPayloadV3::from_json(&frame.data) {
                                        Ok(payload) => {
                                            info!(
                                                            "ControlCmd v3 parsed: phase_p={:?}, phase_q={:?}, strategy_mode={:?}",
                                                            payload.phase_p_set, payload.phase_q_set, payload.strategy_mode
                                                        );
                                        }
                                        Err(e) => {
                                            warn!("Failed to parse ControlCmd V3 payload: {}", e)
                                        }
                                    },
                                    2 => match ControlCmdPayloadV2::from_json(&frame.data) {
                                        Ok(payload) => {
                                            info!(
                                                            "ControlCmd v2 parsed: p_ref={:?}, k_droop={:?}, ai_ready={:?}, strategy_mode={:?}",
                                                            payload.p_ref, payload.k_droop, payload.ai_ready, payload.strategy_mode
                                                        );
                                        }
                                        Err(e) => {
                                            warn!("Failed to parse ControlCmd V2 payload: {}", e)
                                        }
                                    },
                                    _ => match ControlCmdPayload::from_json(&frame.data) {
                                        Ok(payload) => {
                                            info!(
                                                            "ControlCmd v1 parsed: p_batt_set={:?}, q_batt_set={:?}, ai_ready={:?}, strategy_mode={:?}",
                                                            payload.p_batt_set, payload.q_batt_set, payload.ai_ready, payload.strategy_mode
                                                        );
                                        }
                                        Err(e) => {
                                            warn!("Failed to parse ControlCmd V1 payload: {}", e)
                                        }
                                    },
                                }
                            }
                        }
                        IntercoreFrameType::ControlRsp => {
                            info!("Received control response from {}", addr);
                        }
                        IntercoreFrameType::StatusReport => {
                            info!("Received status report from {}", addr);
                        }
                        IntercoreFrameType::DataUpload => {
                            info!("Received data upload from {}", addr);
                            // v2.10: 解析 DataUpload JSON payload
                            if !frame.data.is_empty() {
                                match DataUploadPayload::from_json(&frame.data) {
                                    Ok(payload) => {
                                        // v2.10: 更新 q_realtime_margin
                                        if let Some(q_margin) = payload.q_realtime_margin_clamped()
                                        {
                                            let missing_count =
                                                intercore_state.increment_q_margin_missing().await;
                                            intercore_state.update_q_margin(q_margin).await;
                                            if missing_count >= 3 {
                                                warn!(
                                                    "q_realtime_margin missing for {} cycles",
                                                    missing_count
                                                );
                                            }
                                        } else {
                                            let missing_count =
                                                intercore_state.increment_q_margin_missing().await;
                                            if missing_count >= 3 {
                                                warn!(
                                                    "q_realtime_margin missing for {} cycles",
                                                    missing_count
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to parse DataUpload JSON payload: {}", e);
                                    }
                                }
                            }
                        }
                        // v2.10 新增：SafetyOverride 帧处理
                        IntercoreFrameType::SafetyOverride => {
                            info!("Received safety override from {}", addr);
                            if !frame.data.is_empty() {
                                match SafetyOverridePayload::from_json(&frame.data) {
                                    Ok(payload) => {
                                        // 频率限制检查
                                        if intercore_state
                                            .check_and_increment_safety_override()
                                            .await
                                        {
                                            error!("SafetyOverride rate limit exceeded, rejecting frame");
                                            continue;
                                        }

                                        let max_batt_power = max_batt_power_kw;
                                        let clamped_p_ref =
                                            payload.clamp_override_p_ref(max_batt_power);
                                        let clamped_duration = payload.clamp_override_duration_ms();

                                        intercore_state
                                            .update_safety_override(
                                                payload.trigger_reason(),
                                                clamped_p_ref,
                                                clamped_duration,
                                                payload.recovery_condition.as_deref().unwrap_or(
                                                    SAFETY_OVERRIDE_RECOVERY_TIMER_EXPIRED,
                                                ),
                                            )
                                            .await;

                                        info!(
                                                    "SafetyOverride active: reason={}, p_ref={}, duration={}ms",
                                                    payload.trigger_reason(),
                                                    clamped_p_ref,
                                                    clamped_duration
                                                );
                                    }
                                    Err(e) => {
                                        warn!("Failed to parse SafetyOverride JSON payload: {}", e);
                                    }
                                }
                            }
                        }
                        IntercoreFrameType::Connect => {
                            info!("Received connect from {}", addr);
                        }
                        IntercoreFrameType::Unknown => {
                            warn!("Unknown frame type from {}", addr);
                        }
                    }

                    // 回复心跳响应
                    if frame.header.frame_type == IntercoreFrameType::HeartbeatReq {
                        let rsp = IntercoreFrame::new_heartbeat_rsp();
                        let rsp_data = rsp.to_bytes()?;
                        Self::send_with_timeout(&mut write_half, &rsp_data, cmd_config.timeout_ms)
                            .await?;
                    }
                }
                Err(e) => {
                    error!("Frame parse error from {}: {}", addr, e);
                }
            }
        }

        Ok(())
    }

    /// 从字节流累积读取**一个完整定长帧**（E-09）。
    ///
    /// - `acc` 是**跨 `read` 存活**的累积缓冲：不足 [`FRAME_FIXED_LENGTH`] 时继续读，
    ///   凑满后按整帧切出（`drain`），多读到的字节留在 `acc` 里供下一次调用使用。
    /// - 返回 `Ok(None)` 表示对端正常关闭（EOF）；`Err` 为底层 IO 错误。
    ///
    /// 为何以 [`FRAME_FIXED_LENGTH`] 而非 `header.length` 作为累积目标：PRD 10 §2.2 /
    /// IC-AC-04 规定线格式为**定长 64 字节**，`header.length` 只覆盖「帧头+载荷+CRC」
    /// （= 8 + N + 2），**不含 padding**。故定长帧的累积目标恒为 64。
    async fn read_fixed_frame<R>(
        reader: &mut R,
        acc: &mut Vec<u8>,
    ) -> std::io::Result<Option<Vec<u8>>>
    where
        R: AsyncRead + Unpin,
    {
        let mut chunk = [0u8; READ_CHUNK_LEN];
        while acc.len() < FRAME_FIXED_LENGTH {
            let n = reader.read(&mut chunk).await?;
            if n == 0 {
                return Ok(None); // EOF
            }
            acc.extend_from_slice(&chunk[..n]);
        }
        Ok(Some(acc.drain(..FRAME_FIXED_LENGTH).collect()))
    }

    /// 带超时的发送操作（P2-16）
    async fn send_with_timeout(
        writer: &mut (impl AsyncWriteExt + Unpin),
        data: &[u8],
        timeout_ms: u64,
    ) -> Result<(), MupcError> {
        timeout(Duration::from_millis(timeout_ms), writer.write_all(data))
            .await
            .map_err(|_| {
                MupcError::new(
                    ErrorCode::IntercoreTimeout,
                    format!("Send timed out after {}ms", timeout_ms),
                    "intercore",
                )
            })?
            .map_err(|e| {
                MupcError::new(
                    ErrorCode::SendFailed,
                    format!("Send error: {}", e),
                    "intercore",
                )
            })
    }

    /// 停止服务器
    pub async fn shutdown(&self) -> Result<(), MupcError> {
        let _ = self.shutdown_tx.send(());
        Ok(())
    }
}

// ============================================================================
// P2-17: IntercoreClient 主动发送双参数到实时控制模块
// ============================================================================

/// 双参数命令（用于发送到实时控制模块，v2.7）
///
/// 注意：load_shedding 和 pv_limit 不通过此命令发送，
/// 它们通过 SouthCommandDispatcher 发送到南向设备。
#[derive(Debug, Clone)]
pub struct DualParamCommand {
    /// 有功功率基准点 (kW)
    pub p_ref: f64,
    /// 电压-有功下垂系数 (kW/V)
    pub k_droop: f64,
    /// AI 就绪状态
    pub ai_ready: bool,
    /// 当前策略模式
    pub strategy_mode: String,
}

impl DualParamCommand {
    /// 创建双参数命令
    ///
    /// 注意：load_shedding 和 pv_limit 不通过核间通信发送，
    /// 它们通过 SouthCommandDispatcher 发送到南向设备。
    pub fn new(p_ref: f64, k_droop: f64, ai_ready: bool, strategy_mode: &str) -> Self {
        Self {
            p_ref,
            k_droop,
            ai_ready,
            strategy_mode: strategy_mode.to_string(),
        }
    }
}

/// 核间通信客户端（传输门面）
///
/// 与 IntercoreServer 不同，Client 主动连接到实时控制模块，
/// 并发送 AI 引擎输出的 p_ref 和 k_droop 双参数。
///
/// 本类型为传输门面：不直接持有 TcpStream，而是委托给
/// `Arc<dyn IntercoreTransport>`（默认 TcpTransport，其他实现可注入）。
pub struct IntercoreClient {
    /// 底层传输通道（可插拔；当前唯一实现为 TcpTransport）
    transport: Arc<dyn IntercoreTransport>,
    /// 传输描述（供 remote_addr() 查询）：new()/with_config() 为真实 TCP 目标地址；
    /// with_transport() 因 IntercoreTransport 未暴露自描述接口，只能填传输类型占位 "tcp"
    remote_addr: String,
    /// 最后发送的 p_ref（用于通信中断检测）
    last_p_ref: RwLock<Option<f64>>,
    /// 最后发送的 k_droop
    last_k_droop: RwLock<Option<f64>>,
}

impl IntercoreClient {
    /// 创建默认 TCP 客户端（保持现有调用兼容）
    pub fn new(remote_addr: String) -> Self {
        Self {
            transport: Arc::new(TcpTransport::new(remote_addr.clone())),
            remote_addr,
            last_p_ref: RwLock::new(None),
            last_k_droop: RwLock::new(None),
        }
    }

    /// 带配置创建客户端（兼容签名；超时已由 TcpTransport 默认 5000ms 承载）
    pub fn with_config(remote_addr: String, _cmd_config: CommandConfig) -> Self {
        Self::new(remote_addr)
    }

    /// 注入自定义传输（生产注入点为 TcpTransport）
    pub fn with_transport(transport: Arc<dyn IntercoreTransport>) -> Self {
        Self {
            transport,
            // 传输类型占位：IntercoreTransport 未暴露自描述接口，此处不作具体通道名假设
            remote_addr: "tcp".to_string(),
            last_p_ref: RwLock::new(None),
            last_k_droop: RwLock::new(None),
        }
    }

    /// 发送双参数到实时控制模块（v2.7）
    ///
    /// 委托给底层 transport（TcpTransport 封装为 TCP v2.0 帧）。
    pub async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
        self.transport.send_dual_param(cmd).await?;

        // 更新最后发送的参数
        *self.last_p_ref.write().await = Some(cmd.p_ref);
        *self.last_k_droop.write().await = Some(cmd.k_droop);

        tracing::debug!(
            "Sent dual-param ControlCmd: p_ref={}, k_droop={}, ai_ready={}, strategy_mode={}",
            cmd.p_ref,
            cmd.k_droop,
            cmd.ai_ready,
            cmd.strategy_mode
        );

        Ok(())
    }

    /// 发送台区储能分相 P/Q 设定到实时控制模块（v3 分相模式）
    ///
    /// 委托给底层 transport（TcpTransport 封装为 TCP v3.0 分相帧）。
    pub async fn send_tai_command(
        &self,
        p: [f64; 3],
        q: [f64; 3],
        strategy_mode: &str,
    ) -> Result<(), MupcError> {
        self.transport.send_tai_command(p, q, strategy_mode).await
    }

    /// 获取最后发送的双参数（用于降级判断）
    pub async fn get_last_params(&self) -> (Option<f64>, Option<f64>) {
        let p_ref = *self.last_p_ref.read().await;
        let k_droop = *self.last_k_droop.read().await;
        (p_ref, k_droop)
    }

    /// 检查连接状态
    pub async fn is_connected(&self) -> bool {
        self.transport.is_connected().await
    }

    /// 获取传输描述：new()/with_config() 为真实 TCP 目标地址，with_transport() 为
    /// 传输类型占位 "tcp"（见字段说明）。
    ///
    /// ⚠️ 当前**零调用方**（旧调用者随 PCS 面删除）；保留仅为兼容既有 API 面，
    /// 「是否删除该字段+访问器」登记为待裁定项。
    pub fn remote_addr(&self) -> &str {
        &self.remote_addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::FrameType;
    use std::collections::VecDeque;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::ReadBuf;

    /// 把字节流按指定分片大小喂出的 mock `AsyncRead`（模拟 TCP 半帧到达）。
    struct FragmentedReader {
        chunks: VecDeque<Vec<u8>>,
    }

    impl FragmentedReader {
        /// 按 `sizes` 逐段切分 `data`；不足一段的余量作为最后一段。
        fn split(data: &[u8], sizes: &[usize]) -> Self {
            let mut chunks = VecDeque::new();
            let mut pos = 0usize;
            for &sz in sizes {
                if pos >= data.len() {
                    break;
                }
                let end = (pos + sz).min(data.len());
                chunks.push_back(data[pos..end].to_vec());
                pos = end;
            }
            if pos < data.len() {
                chunks.push_back(data[pos..].to_vec());
            }
            Self { chunks }
        }
    }

    impl AsyncRead for FragmentedReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let me = self.as_mut().get_mut();
            match me.chunks.pop_front() {
                Some(chunk) => {
                    let n = chunk.len().min(buf.remaining());
                    if n == 0 {
                        // 调用方缓冲已满：把整段放回，等下次 poll
                        me.chunks.push_front(chunk);
                        return Poll::Ready(Ok(()));
                    }
                    buf.put_slice(&chunk[..n]);
                    if n < chunk.len() {
                        me.chunks.push_front(chunk[n..].to_vec());
                    }
                    Poll::Ready(Ok(()))
                }
                None => Poll::Ready(Ok(())), // EOF
            }
        }
    }

    /// E-09 判别力测试：**帧被拆成多段到达**（没有任何一次 read 能拿到整帧）时，
    /// 必须靠跨 read 的累积缓冲拼出完整帧，且连续多帧不丢帧、不失步。
    ///
    /// 改坏方式（必须变红）：把 `read_fixed_frame` 换回"单次 `read` 进 64 B 缓冲直接解析"
    /// ⇒ 第一段只有 7 字节，返回的"帧"与期望帧不等价。
    #[tokio::test]
    async fn test_read_fixed_frame_accumulates_across_partial_reads() {
        let f1 = IntercoreFrame::new_connect().to_bytes().unwrap();
        let f2 = IntercoreFrame::new_heartbeat_rsp().to_bytes().unwrap();
        let f3 = IntercoreFrame::new(FrameType::ControlCmd, 9, vec![1, 2, 3])
            .to_bytes()
            .unwrap();
        assert_eq!(f1.len(), FRAME_FIXED_LENGTH);

        let all: Vec<u8> = [f1.clone(), f2.clone(), f3.clone()].concat();
        // 分片刻意跨帧边界：7 / 13 / 51 / 3 / 9 / 77 ⇒ 每段都小于 64、也都不与帧对齐
        let mut reader = FragmentedReader::split(&all, &[7, 13, 51, 3, 9, 77]);
        let mut acc: Vec<u8> = Vec::new();

        for expect in [&f1, &f2, &f3] {
            let got = IntercoreServer::read_fixed_frame(&mut reader, &mut acc)
                .await
                .unwrap()
                .expect("应读出完整帧");
            assert_eq!(&got, expect, "累积读出的帧必须与原始帧逐字节相同");
        }
        // 流耗尽 ⇒ EOF
        assert!(IntercoreServer::read_fixed_frame(&mut reader, &mut acc)
            .await
            .unwrap()
            .is_none());
    }

    /// 分片边界落在帧内部时，帧仍能逐个解析（模拟 BufReader 之后仍可能半帧）。
    #[tokio::test]
    async fn test_read_fixed_frame_parses_each_frame_after_fragmenting() {
        let f1 = IntercoreFrame::new_heartbeat_req(1, 45.5, 0.75)
            .to_bytes()
            .unwrap();
        let f2 = IntercoreFrame::new(FrameType::StatusReport, 3, vec![7; 20])
            .to_bytes()
            .unwrap();
        let all: Vec<u8> = [f1.clone(), f2.clone()].concat();
        let mut reader = FragmentedReader::split(&all, &[1, 1, 62, 2]);
        let mut acc: Vec<u8> = Vec::new();

        let a = IntercoreServer::read_fixed_frame(&mut reader, &mut acc)
            .await
            .unwrap()
            .unwrap();
        let b = IntercoreServer::read_fixed_frame(&mut reader, &mut acc)
            .await
            .unwrap()
            .unwrap();
        let pa = IntercoreFrame::from_bytes(&a).unwrap();
        let pb = IntercoreFrame::from_bytes(&b).unwrap();
        assert_eq!(pa.header.frame_type, FrameType::HeartbeatReq);
        assert_eq!(pb.header.frame_type, FrameType::StatusReport);
        assert_eq!(pb.data, vec![7; 20]);
        assert_eq!(pb.header.seq_no, 3);
    }

    // ========== E-16: 连接状态快照原子性 ==========

    /// 快照字段间不变式：`active == reason.is_some()`（确定性版本）。
    #[tokio::test]
    async fn test_snapshot_safety_override_invariant() {
        let st = IntercoreConnectionState::new();

        let s = st.snapshot().await;
        assert!(!s.safety_override_active);
        assert_eq!(s.safety_override_active, s.safety_override_reason.is_some());

        st.update_safety_override("voltage_high", 30.0, 5000, "timer_expired")
            .await;
        let s = st.snapshot().await;
        assert_eq!(s.safety_override_active, s.safety_override_reason.is_some());
        assert!(s.safety_override_active && s.safety_override_reason.is_some());
        assert_eq!(s.safety_override_p_ref, Some(30.0));
        assert_eq!(s.safety_override_duration_ms, 5000);

        st.clear_safety_override().await;
        let s = st.snapshot().await;
        assert_eq!(s.safety_override_active, s.safety_override_reason.is_some());
        assert!(!s.safety_override_active && s.safety_override_reason.is_none());
    }

    /// **并发撕裂判别力测试**：写侧不停 update/clear，读侧反复取快照并断言不变式。
    ///
    /// 改坏方式（必须变红）：把状态退回 12 把独立 `RwLock`（`snapshot()` 逐字段 `read()`）
    /// ⇒ 读侧会取到 `active=true` 而 `reason=None`（或反之）的撕裂值。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_snapshot_never_tears_safety_override_under_concurrency() {
        use std::sync::Arc;
        const ROUNDS: usize = 5_000;

        let st = Arc::new(IntercoreConnectionState::new());
        let writer = {
            let st = st.clone();
            tokio::spawn(async move {
                for _ in 0..ROUNDS {
                    st.update_safety_override("v", 1.0, 100, "t").await;
                    tokio::task::yield_now().await;
                    st.clear_safety_override().await;
                    tokio::task::yield_now().await;
                }
            })
        };

        for i in 0..ROUNDS {
            let s = st.snapshot().await;
            assert_eq!(
                s.safety_override_active,
                s.safety_override_reason.is_some(),
                "第 {} 次快照撕裂: active={} reason={:?}",
                i,
                s.safety_override_active,
                s.safety_override_reason
            );
        }
        writer.await.unwrap();
    }

    // ========== E-10: 指令队列（未接线）==========

    /// `retry_or_drop` 必须真的按重试计数处理，而不是静默丢弃。
    ///
    /// `max_retries = 2` ⇒ 共 3 次尝试：前 2 次失败留队，第 3 次失败丢弃。
    #[test]
    fn test_retry_or_drop_requeues_until_retries_exhausted() {
        let mut q = CommandQueue::new(CommandConfig {
            timeout_ms: 5000,
            max_retries: 2,
        });
        q.enqueue(vec![1, 2, 3]);
        assert_eq!(q.pending_count(), 1);

        // 第 1 次失败：额度 2 → 1 ⇒ 留队
        q.retry_or_drop(vec![1, 2, 3]);
        assert_eq!(q.pending_count(), 1, "第 1 次失败必须留队");
        // 第 2 次失败：额度 1 → 0 ⇒ 仍留队（还能再试一次）
        q.retry_or_drop(vec![1, 2, 3]);
        assert_eq!(q.pending_count(), 1, "第 2 次失败仍有余量，必须留队");
        // 第 3 次失败：额度已耗尽 ⇒ 丢弃
        q.retry_or_drop(vec![1, 2, 3]);
        assert_eq!(q.pending_count(), 0, "重试额度耗尽后必须丢弃");

        // 不匹配的 payload 不得误伤队列
        q.enqueue(vec![9, 9]);
        q.retry_or_drop(vec![8, 8]);
        assert_eq!(q.pending_count(), 1, "不匹配的 payload 不得影响队列");
    }

    #[test]
    fn test_v3_payload_roundtrip() {
        let p = ControlCmdPayloadV3 {
            frame_version: Some(3),
            p_ref: Some(10.0),
            k_droop: Some(5.0),
            phase_p_set: Some([1.0, 2.0, 3.0]),
            phase_q_set: Some([0.5, 0.5, 0.5]),
            ai_ready: Some(false),
            strategy_mode: Some("fallback".into()),
            timestamp_ms: Some(1_700_000_000_000),
        };
        let bytes = p.to_json().unwrap();
        let parsed = ControlCmdPayloadV3::from_json(&bytes).unwrap();
        assert_eq!(parsed.phase_p_set, Some([1.0, 2.0, 3.0]));
        assert_eq!(parsed.phase_q_set, Some([0.5, 0.5, 0.5]));
        assert_eq!(ControlCmdPayloadV3::detect_version(&bytes).unwrap(), 3);
    }

    #[test]
    fn test_v3_payload_missing_phase_ok() {
        let p = ControlCmdPayloadV3 {
            frame_version: Some(3),
            p_ref: Some(10.0),
            k_droop: Some(5.0),
            phase_p_set: None,
            phase_q_set: None,
            ai_ready: Some(true),
            strategy_mode: Some("intelligent".into()),
            timestamp_ms: None,
        };
        let bytes = p.to_json().unwrap();
        let parsed = ControlCmdPayloadV3::from_json(&bytes).unwrap();
        assert!(parsed.phase_p_set.is_none());
    }
}
