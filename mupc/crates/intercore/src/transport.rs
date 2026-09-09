//! 核间传输抽象（可插拔：Tcp / ModbusRtu）
use async_trait::async_trait;
use mupc_common::MupcError;

pub mod modbus;
pub mod tcp;

use crate::protocol::{FrameType as IntercoreFrameType, IntercoreFrame};
use crate::tcp_server::{ControlCmdPayloadV2, ControlCmdPayloadV3, DualParamCommand};

/// 三相展示读数（PCS 3 区输入寄存器 1022-1032 解码结果）。
///
/// intercore **自有类型**，与 display-proto 解耦（12-本地显示终端-设计文档 §4.1：intercore
/// 不得依赖 display-proto，上层 DisplayDataProvider 再转 display 域类型/打 FieldFlag）。
/// 字段已按 0.1 量纲缩放为工程值：电流 A / 有功 kW（原始 Int16 × 0.1，经 pcs.rs 字节互换
/// 回解有符号 i16）。值域语义 **正=放电(输出)/负=充电**（协议 V1.3；极性追认前仅佐证，
/// 12-设计文档 §11 待确认项 3）。
/// 各字段 `Option`：`None` = 该段（电流段 / 有功+总段）读取失败或读数无效 → 上层据此打
/// Offline/NotRead 角标；点级独立降级（§3.4 F5.5）由上层按字段消费。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ThreePhaseRead {
    /// 三相输出电流 [A, B, C]，单位 A
    pub i_phase: Option<[f64; 3]>,
    /// 三相输出有功 [A, B, C]，单位 kW（正放负充）
    pub p_phase: Option<[f64; 3]>,
    /// 设备总有功，单位 kW（正放负充）
    pub p_total: Option<f64>,
}

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
    /// 停机原语：Modbus 写 REG_START_STOP=0（PCS 停机）；成功复位 started/mode 缓存。
    /// 实现**不**负责置/清 stopped_latched（联锁 latch 仅由 restore_interlock_latched 管理）。
    /// Tcp 通道降级（no-op + 记录，无 PCS 500 语义）。⚠️ 下行 latch gate（send/stop 的 500 语义）
    /// 仅 Modbus 实现；TCP 仅表达 latch 状态、`send`/`stop` 是否抑制由上层（IntercoreClient/
    /// 联锁流程）决定（M-4）。
    async fn stop(&self) -> Result<(), String>;
    /// 联锁锁存查询（transport 运行期兜底是否挡启动）
    async fn is_interlock_stopped(&self) -> bool;
    /// 置/清联锁 latch（C-1 唯一入口：触发沿 restore(true)；release/DB 读回 restore(false)）
    async fn restore_interlock_latched(&self, latched: bool) -> Result<(), String>;
    /// 最新解码的 RUN_STATE(1013)（心跳维护，0=停/1=待机/2=充/3=放）；离线/mark_offline 后为 None
    fn last_run_state(&self) -> Option<u16>;
    /// M1 保护跳闸/停机人工授权重启（ack_m1 语义，**单次**）：!stopped_latched 时复位 started
    /// **并置 restart_authorized**（Modbus），放行下次 send 的 ensure_started 在 RUN_STATE=0
    /// 停机稳态下重写 500=1 一次（S-4 停机守卫旁路）；授权经 S-4 消费分支或正常启动路径清除。
    /// stopped_latched 时 Err（须先 release 清 latch）。⚠️ 语义挂起：PCS 停机后 run_state=0
    /// 稳态下重启 = 人工授权后 S-4 放行一次；500 电平/边沿时序以厂方答复为准（§11.11 待确认）。
    async fn authorize_restart(&self) -> Result<(), String>;
    /// 三相展示读数（PCS 3 区输入寄存器 1022-1032，FC04）。Modbus 实现有效；Tcp/sim 无
    /// PCS 3 区点表 → 默认 None（上层打 NotRead）。由显示采集独立 1s 任务调用，与心跳
    /// SOC 读同走 bus 锁（W3 半双工互斥），**不进联锁抑制链**（12-设计文档 §4.1）。
    async fn read_three_phase(&self) -> Option<ThreePhaseRead> {
        None
    }
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
