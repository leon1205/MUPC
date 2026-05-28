//! AI 命令校验器测试

use mupc_data_processing::telemetry::DataPackage;

use crate::ai_validator::{AiCommandValidatorImpl, MockAiModel, ModelInput};
use crate::strategies::{AiCommandValidator, CommandType, ControlCommand};

#[test]
fn test_mock_ai_model_predict_high_soc() {
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
}

#[test]
fn test_mock_ai_model_predict_low_soc() {
    let model = MockAiModel;

    // 测试 SOC 低的情况（优先充电）
    let input = ModelInput {
        battery_soc: 0.1,
        pv_power: 50.0,
        load_power: 30.0,
        grid_power: 0.0,
    };
    let output = model.predict(&input);
    assert!(output.recommended_p_batt < 0.0);
}

#[test]
fn test_mock_ai_model_predict_mid_soc() {
    let model = MockAiModel;

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
    // 所以会被标记为无效
    assert!(!result.valid);
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
    };
    let result = validator.validate_sync(&cmd);
    assert!(result.valid); // 开关控制直接通过
}

#[test]
fn test_validator_name() {
    let validator = AiCommandValidatorImpl::new();
    assert_eq!(validator.name(), "AiCommandValidatorImpl");
}

#[tokio::test]
async fn test_validator_async_validate() {
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
    let result = validator.validate(&cmd).await;
    assert!(result.valid);
}