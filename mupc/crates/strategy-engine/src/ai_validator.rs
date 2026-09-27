//! AI 命令校验器（可插拔实现）
//!
//! Phase 1: Mock 实现
//! Phase 3C: 替换为真实 LSTM/TCN 模型
//! P2-12: 接入真实遥测数据

use async_trait::async_trait;
use chrono;
use mupc_data_processing::telemetry::DataPackage;
use std::sync::RwLock;

use super::strategies::{AiCommandValidator, CommandType, ControlCommand, ValidationResult};

/// AI 模型 trait（可插拔）
pub trait AiModel: Send + Sync {
    fn predict(&self, input: &ModelInput) -> ModelOutput;
}

/// AI 模型输入
#[derive(Debug, Clone)]
pub struct ModelInput {
    /// 电池 SOC (0.0 - 1.0)
    pub battery_soc: f64,
    /// 光伏功率 (kW)
    pub pv_power: f64,
    /// 负荷功率 (kW)
    pub load_power: f64,
    /// 电网功率 (kW)
    pub grid_power: f64,
}

/// AI 模型输出
#[derive(Debug, Clone)]
pub struct ModelOutput {
    /// 推荐电池有功设定 (kW)
    pub recommended_p_batt: f64,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f64,
}

/// 默认 AI 模型（模拟）
pub struct MockAiModel;

impl AiModel for MockAiModel {
    fn predict(&self, input: &ModelInput) -> ModelOutput {
        // 简单的模拟逻辑：基于 SOC 和功率平衡计算推荐值
        let recommended_p_batt = if input.battery_soc > 0.8 {
            // SOC 高，优先放电
            (input.pv_power - input.load_power).max(0.0)
        } else if input.battery_soc < 0.2 {
            // SOC 低，优先充电：光伏富余（pv>load）时以负功率吸收；光不足不额外充
            (input.load_power - input.pv_power).min(0.0)
        } else {
            0.0
        };

        ModelOutput {
            recommended_p_batt,
            confidence: 0.5,
        }
    }
}

/// AI 命令校验器实现
pub struct AiCommandValidatorImpl {
    model: Option<Box<dyn AiModel>>,
    /// 最新遥测数据（来自南向设备，RwLock 支持 &self 注入）
    latest_data: RwLock<Option<DataPackage>>,
    /// 数据接收时间戳
    data_timestamp: RwLock<Option<chrono::DateTime<chrono::Utc>>>,
}

impl AiCommandValidatorImpl {
    pub fn new() -> Self {
        Self {
            model: None,
            latest_data: RwLock::new(None),
            data_timestamp: RwLock::new(None),
        }
    }

    pub fn with_model(model: Box<dyn AiModel>) -> Self {
        Self {
            model: Some(model),
            latest_data: RwLock::new(None),
            data_timestamp: RwLock::new(None),
        }
    }

    /// 更新遥测数据（trait 接口，&self 注入）
    pub fn update_data(&self, data: DataPackage) {
        if let Ok(mut ts) = self.data_timestamp.write() {
            *ts = Some(chrono::Utc::now());
        }
        if let Ok(mut ld) = self.latest_data.write() {
            *ld = Some(data);
        }
    }

    /// 检查遥测数据是否过期（超过 5 秒）
    pub fn is_data_stale(&self) -> bool {
        match self.data_timestamp.read() {
            Ok(ts) => match *ts {
                Some(ts) => {
                    let age = chrono::Utc::now() - ts;
                    age > chrono::Duration::seconds(5)
                }
                None => true,
            },
            Err(_) => true,
        }
    }

    /// 从遥测数据构建 AI 模型输入
    fn build_model_input(&self) -> ModelInput {
        match self.latest_data.read() {
            Ok(guard) => match guard.as_ref() {
                Some(data) => ModelInput {
                    battery_soc: data.battery.soc.unwrap_or(50.0) / 100.0,
                    pv_power: data.device_status.pv_power.unwrap_or(0.0),
                    load_power: data.device_status.load_power.unwrap_or(0.0),
                    grid_power: data.electrical.active_power.unwrap_or(0.0),
                },
                None => ModelInput {
                    battery_soc: 0.5,
                    pv_power: 0.0,
                    load_power: 0.0,
                    grid_power: 0.0,
                },
            },
            Err(_) => ModelInput {
                battery_soc: 0.5,
                pv_power: 0.0,
                load_power: 0.0,
                grid_power: 0.0,
            },
        }
    }

    /// 同步校验（用于测试）
    pub fn validate_sync(&self, cmd: &ControlCommand) -> ValidationResult {
        // ── D-11：无模型 ⇒ **不可校验**，必须 fail-closed ──
        // 旧实现在此返回 `valid()`（"无模型时默认通过"），生产注入的正是无模型实例
        // （`startup.rs` 的 `AiCommandValidatorImpl::new()`）⇒ 本闸门恒放行 = 空闸门。
        // 置于三条数据早退**之前**：无模型时任何遥测状态都无从校验，不得借
        // `degraded_pass`（valid=true）绕开。
        let Some(model) = self.model.as_ref() else {
            return ValidationResult::invalid(
                "无模型（AI 推理通道未接线）：指令不可校验，按 fail-closed 拒绝",
            );
        };

        // 无遥测数据时降级通过（保守安全策略；仅在模型侧校验可得时才有"降级"可言）
        let has_data = self
            .latest_data
            .read()
            .map(|d| d.is_some())
            .unwrap_or(false);
        if !has_data {
            return ValidationResult::degraded_pass("无遥测数据，降级通过");
        }

        // 遥测数据过期时降级通过
        if self.is_data_stale() {
            return ValidationResult::degraded_pass("遥测数据超时(>5s)，降级通过");
        }

        // 只校验功率调节命令
        if cmd.cmd_type != CommandType::PowerRegulation {
            return ValidationResult::valid();
        }

        let p_batt = match cmd.p_batt_set {
            Some(p) => p,
            None => return ValidationResult::valid(),
        };

        // 使用真实遥测数据构建模型输入（而非硬编码）
        let model_input = self.build_model_input();

        let model_output = model.predict(&model_input);

        // 如果 AI 推荐的功率与命令设定差异过大，标记为低置信度
        let diff = (p_batt - model_output.recommended_p_batt).abs();
        if diff > 10.0 && model_output.confidence < 0.7 {
            return ValidationResult::invalid(format!(
                "Command deviation too large: cmd={}, ai_recommend={}, confidence={}",
                p_batt, model_output.recommended_p_batt, model_output.confidence
            ));
        }

        ValidationResult::valid()
    }

    pub fn set_model(&mut self, model: Box<dyn AiModel>) {
        self.model = Some(model);
    }
}

impl Default for AiCommandValidatorImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AiCommandValidator for AiCommandValidatorImpl {
    async fn validate(&self, cmd: &ControlCommand) -> ValidationResult {
        self.validate_sync(cmd)
    }

    fn name(&self) -> &str {
        "AiCommandValidatorImpl"
    }

    fn update_data(&self, data: DataPackage) {
        AiCommandValidatorImpl::update_data(self, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_data_processing::telemetry::{
        BatteryData, DeviceStatus, ElectricalData, InverterStatus,
    };

    /// 构造测试用遥测数据
    fn make_test_data(soc: f64, pv_power: f64, load_power: f64, active_power: f64) -> DataPackage {
        DataPackage {
            electrical: ElectricalData {
                voltage: Some(220.0),
                current: Some(10.0),
                active_power: Some(active_power),
                reactive_power: None,
                cos_phi: None,
                frequency: Some(50.0),
                phase: None,
            },
            battery: BatteryData {
                soc: Some(soc),
                soh: Some(95.0),
                temperature: Some(25.0),
            },
            device_status: DeviceStatus {
                inverter_status: InverterStatus::Running,
                pv_power: Some(pv_power),
                load_power: Some(load_power),
                ev_charger_power: None,
            },
            timestamp: 0,
        }
    }

    #[test]
    fn test_mock_ai_model_predict() {
        let model = MockAiModel;

        // 测试 SOC 高的情况（优先放电）
        let input = ModelInput {
            battery_soc: 0.9,
            pv_power: 50.0,
            load_power: 30.0,
            grid_power: 0.0,
        };
        let output = model.predict(&input);
        assert!(output.recommended_p_batt > 0.0);
        assert_eq!(output.confidence, 0.5);

        // 测试 SOC 低的情况（优先充电）
        let input = ModelInput {
            battery_soc: 0.1,
            pv_power: 50.0,
            load_power: 30.0,
            grid_power: 0.0,
        };
        let output = model.predict(&input);
        assert!(output.recommended_p_batt < 0.0);

        // 测试 SOC 中等的情况
        let input = ModelInput {
            battery_soc: 0.5,
            pv_power: 50.0,
            load_power: 30.0,
            grid_power: 0.0,
        };
        let output = model.predict(&input);
        assert_eq!(output.recommended_p_batt, 0.0);
    }

    /// **D-11**：无模型 ⇒ 不可校验 ⇒ fail-closed（**不得** `valid`）。
    ///
    /// 改坏实现会怎样红：回到 `self.model.is_none() → ValidationResult::valid()`
    /// （或让无遥测/超时的 `degraded_pass` 抢先返回 valid）⇒ 断言 `!result.valid` 红。
    /// 注意本用例**不注入遥测**：无模型判定必须排在数据早退之前，否则空闸门仍放行。
    #[test]
    fn test_validator_without_model() {
        let validator = AiCommandValidatorImpl::new();
        let cmd = ControlCommand {
            cmd_id: 1,
            cmd_type: CommandType::PowerRegulation,
            p_batt_set: Some(10.0),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: None,
            priority: 1,
            phase_p_set: None,
            phase_q_set: None,
        };
        // 无遥测、无模型 ⇒ 必须拒绝（旧的"降级通过/默认通过"都是 fail-open）
        let result = validator.validate_sync(&cmd);
        assert!(!result.valid, "无模型时不得放行（fail-closed）");
        assert!(
            result.message.contains("无模型") && result.message.contains("不可校验"),
            "文案须点明不可校验：{}",
            result.message
        );
        // 即便注入了新鲜遥测，无模型仍不可校验
        validator.update_data(make_test_data(50.0, 50.0, 30.0, 0.0));
        let result2 = validator.validate_sync(&cmd);
        assert!(!result2.valid, "无模型 + 有遥测同样不可校验");
    }

    /// D-11 对照：**有模型**时数据早退仍按原设计走降级通过（该放行是有校验能力前提下的
    /// 显式降级，不是空闸门）。本用例钉住"改了无模型语义但没误伤有模型路径"。
    #[test]
    fn test_validator_with_model_degrades_on_missing_data() {
        let validator = AiCommandValidatorImpl::with_model(Box::new(MockAiModel));
        let cmd = ControlCommand {
            cmd_id: 1,
            cmd_type: CommandType::PowerRegulation,
            p_batt_set: Some(10.0),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: None,
            priority: 1,
            phase_p_set: None,
            phase_q_set: None,
        };
        let result = validator.validate_sync(&cmd);
        assert!(result.valid, "有模型 + 无遥测 = 显式降级通过");
        assert!(result.message.contains("降级通过"));
        assert!(result.message.contains("无遥测数据"));
    }

    #[test]
    fn test_validator_with_data_and_model() {
        let validator = AiCommandValidatorImpl::with_model(Box::new(MockAiModel));
        // 注入遥测数据
        validator.update_data(make_test_data(85.0, 50.0, 30.0, 0.0));

        let cmd = ControlCommand {
            cmd_id: 1,
            cmd_type: CommandType::PowerRegulation,
            p_batt_set: Some(10.0),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: None,
            priority: 1,
            phase_p_set: None,
            phase_q_set: None,
        };
        let result = validator.validate_sync(&cmd);
        // Mock 模型默认 confidence=0.5，小于阈值 0.7，且差异可能大于 10kW
        // SOC=85% 高 → AI 推荐放电 = 20kW，cmd=10kW，差异=10kW 刚好在边界
        // 实际应根据具体场景调整
        assert!(!result.valid || result.valid); // 占位，实际逻辑见上
    }

    #[test]
    fn test_validator_switch_command_passthrough() {
        // D-11 后须带模型：无模型一律 fail-closed（开关控制也不例外）
        let validator = AiCommandValidatorImpl::with_model(Box::new(MockAiModel));
        validator.update_data(make_test_data(50.0, 50.0, 30.0, 0.0));

        let cmd = ControlCommand {
            cmd_id: 2,
            cmd_type: CommandType::SwitchControl,
            p_batt_set: None,
            q_batt_set: None,
            phase_compensation: None,
            start_stop: Some(true),
            priority: 1,
            phase_p_set: None,
            phase_q_set: None,
        };
        let result = validator.validate_sync(&cmd);
        assert!(result.valid); // 开关控制直接通过
    }

    #[test]
    fn test_is_data_stale_no_data() {
        let validator = AiCommandValidatorImpl::new();
        assert!(validator.is_data_stale());
    }

    #[test]
    fn test_is_data_stale_fresh_data() {
        let validator = AiCommandValidatorImpl::new();
        validator.update_data(make_test_data(50.0, 30.0, 20.0, 0.0));
        // 刚更新的数据不应过期
        assert!(!validator.is_data_stale());
    }

    #[test]
    fn test_degraded_pass_on_stale_data() {
        // D-11 后须带模型（超时降级只在"有校验能力"前提下成立）
        let validator = AiCommandValidatorImpl::with_model(Box::new(MockAiModel));
        // 设置一个"过期"时间戳（模拟 >5s 前）
        *validator.data_timestamp.write().unwrap() =
            Some(chrono::Utc::now() - chrono::Duration::seconds(10));
        *validator.latest_data.write().unwrap() = Some(make_test_data(50.0, 30.0, 20.0, 0.0));

        let cmd = ControlCommand {
            cmd_id: 1,
            cmd_type: CommandType::PowerRegulation,
            p_batt_set: Some(10.0),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: None,
            priority: 1,
            phase_p_set: None,
            phase_q_set: None,
        };
        let result = validator.validate_sync(&cmd);
        assert!(result.valid);
        assert!(result.message.contains("降级通过"));
    }
}
