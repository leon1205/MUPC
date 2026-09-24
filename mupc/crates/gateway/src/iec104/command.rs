//! 指令处理

use async_trait::async_trait;
use mupc_common::MupcError;

use super::protocol::{encode_me_tf1, encode_sp_tb1};

/// 上送条目类型（**协议层**，§9.2.5）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryKind {
    /// 标量（测量值）⇒ `M_ME_TF_1`(TI=36)
    Scalar,
    /// 位（单点遥信）⇒ `M_SP_TB_1`(TI=30)
    Bit,
}

/// 上送条目（**协议层类型，定义在 gateway**——避免 gateway 依赖 `data-processing`/`mupc-southd`，
/// 01 设计 §9.0「依赖方向约束」明确禁止该新增依赖边）。
///
/// 由 core-bin 的 `CommandHandler` 实现（T13）从最新值快照转换而来；
/// **无有效值的点不出现**（PRD GI-3：不得以 0 或旧值顶替）。
#[derive(Debug, Clone, Copy)]
pub struct TelemetryItem {
    /// IEC 104 信息对象地址（§9.2.1 的段基址 + 段内 1 基序号）
    pub ioa: u32,
    /// 条目类型（决定 TypeID）
    pub kind: TelemetryKind,
    /// 值；`Bit` ⇒ 0.0 / 1.0
    pub value: f32,
    /// **采集时刻**（不是发送时刻）——§8.4：不得用响应时刻重新打时标
    pub ts_ms: u64,
    /// 传输原因：周期 = 1 / 突发 = 3 / 总召响应 = 20
    pub cot: u8,
}

impl TelemetryItem {
    /// 编码为监视方向 ASDU（**不含 I 帧头**）：`Scalar ⇒ TI=36`、`Bit ⇒ TI=30`，均带 CP56Time2a。
    pub fn encode_asdu(&self) -> Vec<u8> {
        match self.kind {
            TelemetryKind::Scalar => encode_me_tf1(self.ioa, self.value, self.ts_ms, self.cot),
            TelemetryKind::Bit => encode_sp_tb1(self.ioa, self.value != 0.0, self.ts_ms, self.cot),
        }
    }
}

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
    SwitchControl,   // 开关控制
    PowerRegulation, // 功率调节
    ChargeDischarge, // 充放电控制
}

/// 命令处理器 trait
#[async_trait]
pub trait CommandHandler: Send + Sync {
    /// 处理控制命令
    async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError>;

    /// 获取处理器名称
    fn name(&self) -> &str;

    /// **站召唤数据源**（§9.2.5，新增；**默认实现返回空** ⇒ 既有 impl 不改也编译通过）。
    ///
    /// core-bin 的 `StrategyCommandHandler`（T13）覆写：从 `LatestValues::all()` 过滤
    /// `channels.has(IEC104) && is_fresh(..)` 的点（**位点走站级活性判据**），
    /// 按 §9.2.1 的 IOA 表转成 [`TelemetryItem`]。**无有效值的点不出现**（PRD GI-3）。
    async fn on_interrogation(&self) -> Vec<TelemetryItem> {
        Vec::new()
    }

    /// **连接初始快照数据源**（§9.2.5 GI-5，新增；默认空）。
    ///
    /// 与 [`CommandHandler::on_interrogation`] **同源**（同一份"当前值"的两种触发方式），
    /// 默认实现直接转发以保持**一个实现两处调用**，防两套口径。
    async fn on_connection_snapshot(&self) -> Vec<TelemetryItem> {
        self.on_interrogation().await
    }
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
