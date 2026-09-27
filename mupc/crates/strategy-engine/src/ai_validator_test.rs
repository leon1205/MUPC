//! AI 命令校验器测试

use mupc_data_processing::telemetry::{
    BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
};

use crate::ai_validator::{AiCommandValidatorImpl, AiModel, MockAiModel, ModelInput};
use crate::strategies::{AiCommandValidator, CommandType, ControlCommand};

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

/// **D-11**：无模型 ⇒ 不可校验 ⇒ fail-closed（不得因无遥测/超时走 `degraded_pass` 放行）。
/// 改坏实现会怎样红：恢复"无模型时默认通过"或把模型判定移到数据早退之后 ⇒ `!valid` 红。
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
    let result = validator.validate_sync(&cmd);
    assert!(!result.valid, "无模型时不得放行（fail-closed）: {result:?}");
    assert!(
        result.message.contains("无模型") && result.message.contains("不可校验"),
        "文案须点明不可校验：{}",
        result.message
    );
}

#[test]
fn test_validator_with_model() {
    let model = Box::new(MockAiModel);
    let validator = AiCommandValidatorImpl::with_model(model);
    // P2-12: 需要注入遥测数据，否则会降级通过
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
    // 实算：SOC=85% ≥0.8 ⇒ MockAiModel 推荐放电 = pv−load = 50−30 = 20kW；cmd=10kW
    // ⇒ diff = 10.0，判据是 `diff > 10.0`（严格大于）⇒ 恰好不触发 invalid ⇒ valid。
    // （原为 `assert!(!result.valid || result.valid)` 的零判别力占位断言，一并订正。）
    assert!(
        result.valid,
        "diff=10.0 未越严格阈值 ⇒ 应通过，实得 invalid: {}",
        result.message
    );
}

#[test]
fn test_validator_switch_command_passthrough() {
    // D-11 后须带模型（无模型一律 fail-closed，开关控制也不例外）
    let validator = AiCommandValidatorImpl::with_model(Box::new(MockAiModel));
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
fn test_validator_name() {
    let validator = AiCommandValidatorImpl::new();
    assert_eq!(validator.name(), "AiCommandValidatorImpl");
}

#[tokio::test]
async fn test_validator_async_validate() {
    // D-11 后须带模型：有模型 + 无遥测 = 显式降级通过（无模型则 fail-closed）
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
    let result = validator.validate(&cmd).await;
    assert!(result.valid);
}
