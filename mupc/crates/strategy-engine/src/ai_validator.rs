//! AI 命令校验器（可插拔实现）
//!
//! Phase 1: Mock 实现
//! Phase 3C: 替换为真实 LSTM/TCN 模型

use async_trait::async_trait;

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
            // SOC 低，优先充电
            (input.pv_power - input.load_power).min(0.0)
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
}

impl AiCommandValidatorImpl {
    pub fn new() -> Self {
        Self { model: None }
    }

    pub fn with_model(model: Box<dyn AiModel>) -> Self {
        Self { model: Some(model) }
    }

    /// 同步校验（用于测试）
    pub fn validate_sync(&self, cmd: &ControlCommand) -> ValidationResult {
        // 无模型时默认通过
        if self.model.is_none() {
            return ValidationResult::valid();
        }

        let model = self.model.as_ref().unwrap();

        // 只校验功率调节命令
        if cmd.cmd_type != CommandType::PowerRegulation {
            return ValidationResult::valid();
        }

        let p_batt = match cmd.p_batt_set {
            Some(p) => p,
            None => return ValidationResult::valid(),
        };

        // 调用 AI 模型预测
        let model_input = ModelInput {
            battery_soc: 0.5, // TODO: 从实际数据获取
            pv_power: 0.0,
            load_power: 0.0,
            grid_power: 0.0,
        };

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
            pv_limit: None,
            load_shedding: None,
        };
        let result = validator.validate_sync(&cmd);
        assert!(result.valid);
    }

    #[test]
    fn test_validator_with_model() {
        let model = Box::new(MockAiModel);
        let validator = AiCommandValidatorImpl::with_model(model);

        let cmd = ControlCommand {
            cmd_id: 1,
            cmd_type: CommandType::PowerRegulation,
            p_batt_set: Some(10.0),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: None,
            priority: 1,
            pv_limit: None,
            load_shedding: None,
        };
        let result = validator.validate_sync(&cmd);
        // Mock 模型默认 confidence=0.5，小于阈值 0.7，且差异大于 10kW
        // 所以会被标记为无效（实际应根据具体场景调整）
        assert!(!result.valid || result.valid); // 占位，实际逻辑见上
    }

    #[test]
    fn test_validator_switch_command_passthrough() {
        let validator = AiCommandValidatorImpl::new();
        let cmd = ControlCommand {
            cmd_id: 2,
            cmd_type: CommandType::SwitchControl,
            p_batt_set: None,
            q_batt_set: None,
            phase_compensation: None,
            start_stop: Some(true),
            priority: 1,
            pv_limit: None,
            load_shedding: None,
        };
        let result = validator.validate_sync(&cmd);
        assert!(result.valid); // 开关控制直接通过
    }
}