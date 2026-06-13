//! 核间通信 TCP 服务器

use mupc_common::{ErrorCode, MupcError};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};

use super::{HeartbeatManager, IntercoreFrame, IntercoreFrameType};

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
}

impl Default for IntercoreConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 2500,
            heartbeat_interval_ms: 1000,
            watchdog_timeout_ms: 10000,
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
    #[serde(rename = "load_shedding")]
    pub load_shedding: Option<f64>,
    #[serde(rename = "pv_limit")]
    pub pv_limit: Option<f64>,
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
            load_shedding: None,
            pv_limit: None,
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
}

impl IntercoreConnectionState {
    pub fn new() -> Self {
        Self {
            last_valid_p_ref: RwLock::new(None),
            last_valid_k_droop: RwLock::new(None),
            last_heartbeat_ms: RwLock::new(0),
            connected: RwLock::new(false),
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
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(stream, addr, heartbeat, cfg).await {
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
                                    // P2-15: 解析 JSON payload
                                    if !frame.data.is_empty() {
                                        match ControlCmdPayload::from_json(&frame.data) {
                                            Ok(payload) => {
                                                info!(
                                                    "ControlCmd parsed: p_batt_set={:?}, q_batt_set={:?}, ai_ready={:?}, strategy_mode={:?}",
                                                    payload.p_batt_set,
                                                    payload.q_batt_set,
                                                    payload.ai_ready,
                                                    payload.strategy_mode,
                                                );
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "Failed to parse ControlCmd JSON payload: {}",
                                                    e
                                                );
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
