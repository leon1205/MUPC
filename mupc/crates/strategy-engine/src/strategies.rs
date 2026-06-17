//! 策略定义
//!
//! Phase 1 仅定义接口

use async_trait::async_trait;
use mupc_common::MupcError;
use mupc_data_processing::telemetry::DataPackage;

/// 策略类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrategyType {
    Basic,       // 基础策略
    Intelligent, // 智能策略 (AI)
    Fallback,    // 兜底策略
}

/// 控制命令
#[derive(Debug, Clone)]
pub struct ControlCommand {
    /// 命令 ID
    pub cmd_id: u16,
    /// 命令类型
    pub cmd_type: CommandType,
    /// 电池有功设定 (kW)
    #[deprecated(since = "v2.0", note = "使用 p_ref 替代（双参数模式）")]
    pub p_batt_set: Option<f64>,
    /// 有功功率基准点 (kW)，负值=充电，正值=放电
    pub p_ref: Option<f64>,
    /// 电压-有功下垂系数 (kW/V)
    pub k_droop: Option<f64>,
    /// 电池无功设定 (kVar)
    pub q_batt_set: Option<f64>,
    /// 分相补偿系数
    pub phase_compensation: Option<[f64; 3]>,
    /// 启停命令
    pub start_stop: Option<bool>,
    /// 优先级
    pub priority: u8,
    /// PV 限功率比例 (0.0-1.0)，v2.15起由本地防逆流策略设置，非AI动作维度
    pub pv_limit: Option<f64>,
    /// 负荷切除功率 (kW)，v2.15起由本地需量控制策略设置，非AI动作维度
    pub load_shedding: Option<f64>,
}

/// 命令类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandType {
    SwitchControl,   // 开关控制
    PowerRegulation, // 功率调节
    ChargeDischarge, // 充放电控制
}

/// 兜底策略 trait
#[async_trait]
pub trait FallbackStrategy: Send + Sync {
    /// 评估数据并生成控制命令
    async fn evaluate(&self, data: &DataPackage) -> Result<ControlCommand, MupcError>;

    /// 获取策略类型
    fn strategy_type(&self) -> StrategyType;

    /// 获取策略名称
    fn name(&self) -> &str;
}

/// AI 指令校验接口
#[async_trait]
pub trait AiCommandValidator: Send + Sync {
    /// 校验 AI 命令
    async fn validate(&self, cmd: &ControlCommand) -> ValidationResult;

    /// 获取校验器名称
    fn name(&self) -> &str;
}

/// 校验结果
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// 是否通过
    pub valid: bool,
    /// 错误消息
    pub message: String,
    /// 建议的命令（如果校验不通过）
    pub suggested_command: Option<ControlCommand>,
}

impl ValidationResult {
    /// 创建通过结果
    pub fn valid() -> Self {
        Self {
            valid: true,
            message: String::new(),
            suggested_command: None,
        }
    }

    /// 创建失败结果
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            valid: false,
            message: message.into(),
            suggested_command: None,
        }
    }

    /// 创建降级通过结果（数据不可用但允许通过，保守安全策略）
    pub fn degraded_pass(reason: impl Into<String>) -> Self {
        Self {
            valid: true,
            message: format!("[降级通过] {}", reason.into()),
            suggested_command: None,
        }
    }
}

/// AI 命令
#[derive(Debug, Clone)]
pub struct AiCommand {
    /// 命令 ID
    pub cmd_id: u16,
    /// 有功设定值 (kW)
    pub p_set: f64,
    /// 无功设定值 (kVar)
    pub q_set: f64,
    /// 优先级
    pub priority: u8,
    /// 原始命令 JSON
    pub raw_command: String,
}
