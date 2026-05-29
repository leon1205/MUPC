//! 指令处理

use async_trait::async_trait;
use mupc_common::MupcError;

/// 控制命令
#[derive(Debug, Clone)]
pub struct ControlCommand {
    /// 指令 ID
    pub cmd_id: u16,
    /// 命令类型
    pub cmd_type: CommandType,
    /// 有功设定值 (kW)
    pub p_set: Option<f64>,
    /// 无功设定值 (kVar)
    pub q_set: Option<f64>,
    /// 开关状态
    pub switch_state: Option<bool>,
    /// 优先级
    pub priority: u8,
    /// 一次调频 K 值
    pub k_value: Option<f64>,
    /// 一次调频死区 (Hz)
    pub deadband: Option<f64>,
}

/// 命令类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandType {
    SwitchControl,      // 开关控制
    PowerRegulation,    // 功率调节
    ChargeDischarge,    // 充放电控制
}

/// 命令处理器 trait
#[async_trait]
pub trait CommandHandler: Send + Sync {
    /// 处理控制命令
    async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError>;

    /// 获取处理器名称
    fn name(&self) -> &str;
}

/// 命令响应
#[derive(Debug, Clone)]
pub struct CommandResponse {
    /// 指令 ID
    pub cmd_id: u16,
    /// 是否成功
    pub success: bool,
    /// 响应消息
    pub message: String,
    /// 时间戳
    pub timestamp: u64,
}