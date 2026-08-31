//! 核间通信 TCP 服务器

use mupc_common::{ErrorCode, MupcError};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};

use super::{HeartbeatManager, IntercoreFrame, IntercoreFrameType};

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
    pub frame_version: Option<u8>,          // = 3
    #[serde(rename = "p_ref")]
    pub p_ref: Option<f64>,                 // 兼容 v2 双参数（分相模式可为 None）
    #[serde(rename = "k_droop")]
    pub k_droop: Option<f64>,
    #[serde(rename = "phase_p_set")]
    pub phase_p_set: Option<[f64; 3]>,      // 分相有功 (kW)，索引 0/1/2 = A/B/C 相
    #[serde(rename = "phase_q_set")]
    pub phase_q_set: Option<[f64; 3]>,      // 分相无功 (kVAr)，索引 0/1/2 = A/B/C 相
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

/// 核间通信状态（用于通信中断检测和降级）
pub struct IntercoreConnectionState {
    /// 最后收到的有效 p_ref
    pub last_valid_p_ref: RwLock<Option<f64>>,
    /// 最后收到的有效 k_droop
    pub last_valid_k_droop: RwLock<Option<f64>>,
    /// 最后心跳时间戳
    pub last_heartbeat_ms: RwLock<u64>,
    /// 连接状态
    pub connected: RwLock<bool>,
    // v2.10 新增字段
    /// 最后收到的 q_realtime_margin
    pub last_q_realtime_margin: RwLock<Option<f64>>,
    /// q_realtime_margin 连续缺失计数
    pub q_margin_missing_count: RwLock<u32>,
    /// 安全覆盖激活标志
    pub safety_override_active: RwLock<bool>,
    /// 安全覆盖触发原因
    pub safety_override_reason: RwLock<Option<String>>,
    /// 安全覆盖强制放电功率 (kW)
    pub safety_override_p_ref: RwLock<Option<f64>>,
    /// 安全覆盖持续时间 (ms)
    pub safety_override_duration_ms: RwLock<u64>,
    /// 安全覆盖恢复条件
    pub safety_override_recovery: RwLock<Option<String>>,
    /// 安全覆盖触发计数（用于频率限制）
    pub safety_override_count: RwLock<u32>,
    /// 安全覆盖首次触发时间戳（用于 1 分钟窗口计算）
    pub safety_override_first_ts: RwLock<Option<i64>>,
}

impl IntercoreConnectionState {
    pub fn new() -> Self {
        Self {
            last_valid_p_ref: RwLock::new(None),
            last_valid_k_droop: RwLock::new(None),
            last_heartbeat_ms: RwLock::new(0),
            connected: RwLock::new(false),
            // v2.10 新增字段
            last_q_realtime_margin: RwLock::new(None),
            q_margin_missing_count: RwLock::new(0),
            safety_override_active: RwLock::new(false),
            safety_override_reason: RwLock::new(None),
            safety_override_p_ref: RwLock::new(None),
            safety_override_duration_ms: RwLock::new(0),
            safety_override_recovery: RwLock::new(None),
            safety_override_count: RwLock::new(0),
            safety_override_first_ts: RwLock::new(None),
        }
    }

    /// 更新收到的双参数
    pub async fn update_valid_params(&self, p_ref: f64, k_droop: f64) {
        *self.last_valid_p_ref.write().await = Some(p_ref);
        *self.last_valid_k_droop.write().await = Some(k_droop);
    }

    /// 获取最后有效的双参数（通信中断时使用）
    pub async fn get_last_valid_params(&self) -> (Option<f64>, Option<f64>) {
        let p_ref = *self.last_valid_p_ref.read().await;
        let k_droop = *self.last_valid_k_droop.read().await;
        (p_ref, k_droop)
    }

    /// 检查是否已收到有效参数
    pub async fn has_valid_params(&self) -> bool {
        self.last_valid_p_ref.read().await.is_some()
            && self.last_valid_k_droop.read().await.is_some()
    }

    /// 设置连接状态
    pub async fn set_connected(&self, connected: bool) {
        *self.connected.write().await = connected;
    }

    /// 获取连接状态
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// 更新 q_realtime_margin（v2.10）
    pub async fn update_q_margin(&self, q_margin: f64) {
        *self.last_q_realtime_margin.write().await = Some(q_margin);
        *self.q_margin_missing_count.write().await = 0;
    }

    /// 增加 q_margin 缺失计数（v2.10）
    pub async fn increment_q_margin_missing(&self) -> u32 {
        let count = *self.q_margin_missing_count.read().await + 1;
        *self.q_margin_missing_count.write().await = count;
        count
    }

    /// 获取最后有效的 q_margin（v2.10）
    pub async fn get_last_q_margin(&self) -> Option<f64> {
        *self.last_q_realtime_margin.read().await
    }

    /// 更新安全覆盖状态（v2.10）
    pub async fn update_safety_override(
        &self,
        reason: &str,
        p_ref: f64,
        duration_ms: u64,
        recovery: &str,
    ) {
        *self.safety_override_active.write().await = true;
        *self.safety_override_reason.write().await = Some(reason.to_string());
        *self.safety_override_p_ref.write().await = Some(p_ref);
        *self.safety_override_duration_ms.write().await = duration_ms;
        *self.safety_override_recovery.write().await = Some(recovery.to_string());
    }

    /// 清除安全覆盖状态（v2.10）
    pub async fn clear_safety_override(&self) {
        *self.safety_override_active.write().await = false;
        *self.safety_override_reason.write().await = None;
        *self.safety_override_p_ref.write().await = None;
        *self.safety_override_duration_ms.write().await = 0;
        *self.safety_override_recovery.write().await = None;
    }

    /// 检查并增加安全覆盖计数，返回是否超过频率限制（v2.10）
    /// 1 分钟内最多 3 次
    pub async fn check_and_increment_safety_override(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        let mut first_ts = self.safety_override_first_ts.write().await;
        let mut count = self.safety_override_count.write().await;

        // 检查 1 分钟窗口
        if let Some(ts) = *first_ts {
            if now - ts > 60000 {
                // 窗口过期，重置计数
                *count = 0;
                *first_ts = Some(now);
            }
        } else {
            *first_ts = Some(now);
        }

        *count += 1;
        *count > 3 // 1 分钟内最多 3 次
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

    /// 指令发送失败，减少重试次数或移回队列
    pub fn retry_or_drop(&mut self, payload: Vec<u8>) {
        // 查找失败指令并减少重试计数（简化：丢弃）
        // Phase 2+ 需要更精确的指令匹配
        let _ = payload;
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
        let mut buf = [0u8; 64];
        let mut reader = tokio::io::BufReader::new(read_half);

        loop {
            match reader.read(&mut buf).await {
                Ok(0) => {
                    info!("Connection closed: {}", addr);
                    heartbeat.read().await.unregister_connection(addr);
                    break;
                }
                Ok(n) => {
                    match IntercoreFrame::from_bytes(&buf[..n]) {
                        Ok(frame) => {
                            match frame.header.frame_type {
                                IntercoreFrameType::HeartbeatReq
                                | IntercoreFrameType::HeartbeatRsp => {
                                    heartbeat.read().await.receive_heartbeat(addr).await;
                                }
                                IntercoreFrameType::ControlCmd => {
                                    info!("Received control command from {}", addr);
                                    if !frame.data.is_empty() {
                                        // 统一版本分派：3=V3 分相，2=V2 双参数，其余=V1
                                        let ver = ControlCmdPayloadV3::detect_version(&frame.data)
                                            .unwrap_or(1);
                                        match ver {
                                            3 => {
                                                match ControlCmdPayloadV3::from_json(&frame.data) {
                                                    Ok(payload) => {
                                                        info!(
                                                            "ControlCmd v3 parsed: phase_p={:?}, phase_q={:?}, strategy_mode={:?}",
                                                            payload.phase_p_set, payload.phase_q_set, payload.strategy_mode
                                                        );
                                                    }
                                                    Err(e) => warn!("Failed to parse ControlCmd V3 payload: {}", e),
                                                }
                                            }
                                            2 => {
                                                match ControlCmdPayloadV2::from_json(&frame.data) {
                                                    Ok(payload) => {
                                                        info!(
                                                            "ControlCmd v2 parsed: p_ref={:?}, k_droop={:?}, ai_ready={:?}, strategy_mode={:?}",
                                                            payload.p_ref, payload.k_droop, payload.ai_ready, payload.strategy_mode
                                                        );
                                                    }
                                                    Err(e) => warn!("Failed to parse ControlCmd V2 payload: {}", e),
                                                }
                                            }
                                            _ => {
                                                match ControlCmdPayload::from_json(&frame.data) {
                                                    Ok(payload) => {
                                                        info!(
                                                            "ControlCmd v1 parsed: p_batt_set={:?}, q_batt_set={:?}, ai_ready={:?}, strategy_mode={:?}",
                                                            payload.p_batt_set, payload.q_batt_set, payload.ai_ready, payload.strategy_mode
                                                        );
                                                    }
                                                    Err(e) => warn!("Failed to parse ControlCmd V1 payload: {}", e),
                                                }
                                            }
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
                                                if let Some(q_margin) =
                                                    payload.q_realtime_margin_clamped()
                                                {
                                                    let missing_count = intercore_state
                                                        .increment_q_margin_missing()
                                                        .await;
                                                    intercore_state.update_q_margin(q_margin).await;
                                                    if missing_count >= 3 {
                                                        warn!("q_realtime_margin missing for {} cycles", missing_count);
                                                    }
                                                } else {
                                                    let missing_count = intercore_state
                                                        .increment_q_margin_missing()
                                                        .await;
                                                    if missing_count >= 3 {
                                                        warn!("q_realtime_margin missing for {} cycles", missing_count);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "Failed to parse DataUpload JSON payload: {}",
                                                    e
                                                );
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
                                                let clamped_duration =
                                                    payload.clamp_override_duration_ms();

                                                intercore_state
                                                    .update_safety_override(
                                                        payload.trigger_reason(),
                                                        clamped_p_ref,
                                                        clamped_duration,
                                                        payload
                                                            .recovery_condition
                                                            .as_deref()
                                                            .unwrap_or(
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
                                Self::send_with_timeout(
                                    &mut write_half,
                                    &rsp_data,
                                    cmd_config.timeout_ms,
                                )
                                .await?;
                            }
                        }
                        Err(e) => {
                            error!("Frame parse error from {}: {}", addr, e);
                        }
                    }
                }
                Err(e) => {
                    error!("Read error from {}: {}", addr, e);
                    heartbeat.read().await.unregister_connection(addr);
                    break;
                }
            }
        }

        Ok(())
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
    pub fn new(
        p_ref: f64,
        k_droop: f64,
        ai_ready: bool,
        strategy_mode: &str,
    ) -> Self {
        Self {
            p_ref,
            k_droop,
            ai_ready,
            strategy_mode: strategy_mode.to_string(),
        }
    }
}

/// 核间通信客户端（用于主动发送双参数到实时控制模块）
///
/// 与 IntercoreServer 不同，Client 主动连接到实时控制模块，
/// 并发送 AI 引擎输出的 p_ref 和 k_droop 双参数。
pub struct IntercoreClient {
    /// 目标地址（实时控制模块地址）
    remote_addr: String,
    /// 指令发送配置
    cmd_config: CommandConfig,
    /// 连接状态
    connected: RwLock<bool>,
    /// 持久连接（复用 TcpStream，避免每次新建）
    stream: Arc<Mutex<Option<TcpStream>>>,
    /// 最后发送的 p_ref（用于通信中断检测）
    last_p_ref: RwLock<Option<f64>>,
    /// 最后发送的 k_droop
    last_k_droop: RwLock<Option<f64>>,
}

impl IntercoreClient {
    /// 创建客户端
    pub fn new(remote_addr: String) -> Self {
        Self {
            remote_addr,
            cmd_config: CommandConfig::default(),
            connected: RwLock::new(false),
            last_p_ref: RwLock::new(None),
            last_k_droop: RwLock::new(None),
            stream: Arc::new(Mutex::new(None)),
        }
    }

    /// 带配置创建客户端
    pub fn with_config(remote_addr: String, cmd_config: CommandConfig) -> Self {
        Self {
            remote_addr,
            cmd_config,
            connected: RwLock::new(false),
            last_p_ref: RwLock::new(None),
            last_k_droop: RwLock::new(None),
            stream: Arc::new(Mutex::new(None)),
        }
    }

    /// 发送双参数到实时控制模块（v2.7）
    ///
    /// 将 DualParamCommand 封装为 TCP v2.0 帧并发送
    pub async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
        // 创建 v2.0 Payload
        let payload = ControlCmdPayloadV2 {
            p_ref: Some(cmd.p_ref),
            k_droop: Some(cmd.k_droop),
            ai_ready: Some(cmd.ai_ready),
            strategy_mode: Some(cmd.strategy_mode.clone()),
            timestamp_ms: Some(chrono::Utc::now().timestamp_millis() as u64),
            frame_version: Some(ControlCmdPayloadV2::FRAME_VERSION),
        };

        let payload_bytes = payload.to_json().map_err(|e| {
            MupcError::new(
                ErrorCode::SerializeError,
                format!("Failed to serialize ControlCmdPayloadV2: {}", e),
                "intercore",
            )
        })?;

        // 创建 TCP 帧
        let frame = IntercoreFrame::new(IntercoreFrameType::ControlCmd, 0, payload_bytes);
        let frame_bytes = frame.to_bytes()?;

        // 发送帧（复用持久连接，见 send_frame）
        self.send_frame(&frame_bytes).await?;

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
    pub async fn send_tai_command(
        &self,
        p: [f64; 3],
        q: [f64; 3],
        strategy_mode: &str,
    ) -> Result<(), MupcError> {
        let payload = ControlCmdPayloadV3 {
            frame_version: Some(ControlCmdPayloadV3::FRAME_VERSION),
            p_ref: None,
            k_droop: None,
            phase_p_set: Some(p),
            phase_q_set: Some(q),
            ai_ready: Some(false), // 台区储能治理为兜底场景，AI 未就绪
            strategy_mode: Some(strategy_mode.to_string()),
            timestamp_ms: Some(chrono::Utc::now().timestamp_millis() as u64),
        };
        let payload_bytes = payload.to_json().map_err(|e| {
            MupcError::new(
                ErrorCode::SerializeError,
                format!("Failed to serialize ControlCmdPayloadV3: {}", e),
                "intercore",
            )
        })?;
        let frame = IntercoreFrame::new(IntercoreFrameType::ControlCmd, 0, payload_bytes);
        let frame_bytes = frame.to_bytes()?;
        self.send_frame(&frame_bytes).await
    }

    /// 发送 TCP 帧（获取或建立持久连接，带超时写入）
    ///
    /// 失败时重置连接，下次调用将重连。
    async fn send_frame(&self, frame_bytes: &[u8]) -> Result<(), MupcError> {
        let mut stream_guard = self.stream.lock().await;
        if stream_guard.is_none() {
            match TcpStream::connect(&self.remote_addr).await {
                Ok(s) => *stream_guard = Some(s),
                Err(e) => {
                    return Err(MupcError::new(
                        ErrorCode::ConnectionFailed,
                        format!("Failed to connect to {}: {}", self.remote_addr, e),
                        "intercore",
                    ))
                }
            }
        }
        let stream = stream_guard.as_mut().ok_or_else(|| {
            MupcError::new(ErrorCode::ConnectionFailed, "连接未建立", "intercore")
        })?;
        let write_result = timeout(
            Duration::from_millis(self.cmd_config.timeout_ms),
            stream.write_all(frame_bytes),
        )
        .await;
        match write_result {
            Ok(Ok(())) => {
                *self.connected.write().await = true;
                Ok(())
            }
            Ok(Err(e)) => {
                *stream_guard = None;
                Err(MupcError::new(
                    ErrorCode::SendFailed,
                    format!("Send error: {}", e),
                    "intercore",
                ))
            }
            Err(_) => {
                *stream_guard = None;
                Err(MupcError::new(
                    ErrorCode::IntercoreTimeout,
                    format!("Send timed out after {}ms", self.cmd_config.timeout_ms),
                    "intercore",
                ))
            }
        }
    }

    /// 获取最后发送的双参数（用于降级判断）
    pub async fn get_last_params(&self) -> (Option<f64>, Option<f64>) {
        let p_ref = *self.last_p_ref.read().await;
        let k_droop = *self.last_k_droop.read().await;
        (p_ref, k_droop)
    }

    /// 检查连接状态
    pub async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    /// 获取远程地址
    pub fn remote_addr(&self) -> &str {
        &self.remote_addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
