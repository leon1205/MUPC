//! 核间传输抽象（可插拔：Tcp / ModbusRtu）
use async_trait::async_trait;
use mupc_common::MupcError;

pub mod modbus;
pub mod tcp;

use crate::protocol::{FrameType as IntercoreFrameType, IntercoreFrame};
use crate::tcp_server::{ControlCmdPayloadV2, ControlCmdPayloadV3, DualParamCommand};

/// 核间传输通道（上层经 IntercoreClient 门面调用，接口不随通道变化）
#[async_trait]
pub trait IntercoreTransport: Send + Sync {
    /// 下发 AI 双参数（p_ref/k_droop）
    async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError>;
    /// 下发台区储能分相 P/Q
    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], mode: &str) -> Result<(), MupcError>;
    /// 连接状态
    async fn is_connected(&self) -> bool;
    async fn shutdown(&self) -> Result<(), MupcError>;
    /// 实时模块上送的最近 SOC（%，含上送时刻）；无上送能力（如 Modbus 备选）或未收到返回 None
    async fn latest_soc(&self) -> Option<(f64, std::time::Instant)>;
}

/// 构造 V2 ControlCmd 帧字节（TcpTransport 用）
pub(crate) fn v2_control_frame_bytes(cmd: &DualParamCommand) -> Result<Vec<u8>, MupcError> {
    let payload = ControlCmdPayloadV2 {
        p_ref: Some(cmd.p_ref),
        k_droop: Some(cmd.k_droop),
        ai_ready: Some(cmd.ai_ready),
        strategy_mode: Some(cmd.strategy_mode.clone()),
        timestamp_ms: Some(chrono::Utc::now().timestamp_millis() as u64),
        frame_version: Some(ControlCmdPayloadV2::FRAME_VERSION),
    };
    let bytes = payload.to_json().map_err(|e| {
        MupcError::new(mupc_common::ErrorCode::SerializeError, format!("serialize V2: {}", e), "intercore")
    })?;
    // to_bytes() 已返回 Result<_, MupcError>，直接作为尾表达式
    IntercoreFrame::new(IntercoreFrameType::ControlCmd, 0, bytes).to_bytes()
}

/// 构造 V3 分相帧字节
pub(crate) fn v3_control_frame_bytes(p: [f64; 3], q: [f64; 3], mode: &str) -> Result<Vec<u8>, MupcError> {
    let payload = ControlCmdPayloadV3 {
        frame_version: Some(ControlCmdPayloadV3::FRAME_VERSION),
        p_ref: None,
        k_droop: None,
        phase_p_set: Some(p),
        phase_q_set: Some(q),
        ai_ready: Some(false),
        strategy_mode: Some(mode.to_string()),
        timestamp_ms: Some(chrono::Utc::now().timestamp_millis() as u64),
    };
    let bytes = payload.to_json().map_err(|e| {
        MupcError::new(mupc_common::ErrorCode::SerializeError, format!("serialize V3: {}", e), "intercore")
    })?;
    // to_bytes() 已返回 Result<_, MupcError>，直接作为尾表达式
    IntercoreFrame::new(IntercoreFrameType::ControlCmd, 0, bytes).to_bytes()
}

pub use modbus::{ModbusRtuSettings, ModbusRtuTransport};
pub use tcp::TcpTransport;
