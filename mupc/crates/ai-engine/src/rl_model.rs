//! MADDPG/PPO 强化学习决策模型
//!
//! 用于微电网能量管理决策
//! 输入：系统状态 (SOC, PV, Load, Grid, Transformer, Battery Power, Voltage_A/B/C)
//! 输出：最优动作 (p_batt_set, q_batt_set, load_shedding, pv_limit, confidence)
//! v2.5 动作空间参数可配置化：parse_action_output 增加 ActionSpaceConfig 参数

use crate::action_space::ActionSpaceConfig;
use crate::config::{ModelType, RlAlgorithm, RlConfig};
use crate::data_fusion::{validate_input_vector, FusedSystemState};
use crate::error::AiEngineError;
use crate::rknn_runtime::RknnRuntime;

/// 系统状态输入（9 维: soc/pv/load/grid/trafo/batt_power/va/vb/vc）
#[derive(Debug, Clone)]
pub struct SystemState {
    pub battery_soc: f64,
    pub pv_power: f64,
    pub load_power: f64,
    pub grid_power: f64,
    pub transformer_load: f64,
    pub battery_power: f64,
    pub voltage_phase_a: f64,
    pub voltage_phase_b: f64,
    pub voltage_phase_c: f64,
}

impl SystemState {
    pub fn from_features(features: &[f32]) -> Option<Self> {
        if features.len() < 9 {
            return None;
        }
        Some(Self {
            battery_soc: features[0] as f64,
            pv_power: features[1] as f64,
            load_power: features[2] as f64,
            grid_power: features[3] as f64,
            transformer_load: features[4] as f64,
            battery_power: features[5] as f64,
            voltage_phase_a: features[6] as f64,
            voltage_phase_b: features[7] as f64,
            voltage_phase_c: features[8] as f64,
        })
    }

    pub fn to_features(&self) -> Vec<f32> {
        vec![
            self.battery_soc as f32,
            self.pv_power as f32,
            self.load_power as f32,
            self.grid_power as f32,
            self.transformer_load as f32,
            self.battery_power as f32,
            self.voltage_phase_a as f32,
            self.voltage_phase_b as f32,
            self.voltage_phase_c as f32,
        ]
    }
}

/// 动作输出结构体（v2.0 双参数模式）
///
/// 有功功率基准点 + 电压-有功下垂系数双参数模式：
/// - p_ref：有功功率基准点 (kW)，负值=充电，正值=放电
/// - k_droop：电压-有功下垂系数 (kW/V)，电压每升高 1V，输出功率减少 k_droop kW
#[derive(Debug, Clone)]
pub struct ActionOutput {
    /// 有功功率基准点 (kW), 范围由 ActionSpaceConfig 确定
    /// 负值=充电，正值=放电
    pub p_ref: f64,
    /// 电压-有功下垂系数 (kW/V), 范围由实时控制模块提供
    /// 电压每升高 1V，输出功率减少 k_droop kW（下垂公式: P = P_ref - k_droop × ΔV）
    pub k_droop: f64,
    /// 可中断负荷切除量 (kW), [0.0, max_load_shedding]
    pub load_shedding: f64,
    /// 光伏限功率比例, [0.0, 1.0]
    pub pv_limit: f64,
    /// 决策置信度 (0.0 ~ 1.0)
    pub confidence: f64,
}

/// 动作输出结构体（v1.x 单参数模式，legacy）
///
/// 仅用于兼容旧模式，正常情况下不使用
#[derive(Debug, Clone)]
pub struct ActionOutputLegacy {
    /// 电池有功功率设定值 (kW), 负=充电, 正=放电（废弃）
    pub p_batt_set: f64,
    /// 可中断负荷切除量 (kW), [0.0, max_load_shedding]
    pub load_shedding: f64,
    /// 光伏限功率比例, [0.0, 1.0]
    pub pv_limit: f64,
    /// 决策置信度 (0.0 ~ 1.0)
    pub confidence: f64,
}

/// 解析 RL 模型原始输出为 ActionOutput（双参数模式）
///
/// 输出格式: [p_ref, k_droop, load_shedding, pv_limit, confidence]
/// v2.0 双参数模式：p_ref（有功基准）+ k_droop（电压-有功下垂系数）
pub fn parse_action_output(raw: &[f32], config: &ActionSpaceConfig) -> Option<ActionOutput> {
    if raw.len() < 5 {
        return None;
    }

    // 获取 k_droop 范围（默认值为安全边界）
    let k_min = config.k_droop_min.unwrap_or(-100.0);
    let k_max = config.k_droop_max.unwrap_or(100.0);

    let mut action = ActionOutput {
        p_ref: (raw[0] as f64).clamp(
            -config.max_batt_discharge_power,
            config.max_batt_charge_power,
        ),
        k_droop: (raw[1] as f64).clamp(k_min, k_max),
        load_shedding: (raw[2] as f64).clamp(0.0, config.max_load_shedding),
        pv_limit: (raw[3] as f64).clamp(0.0, 1.0),
        confidence: raw.get(4).copied().unwrap_or(0.5) as f64,
    };

    action.confidence = action.confidence.clamp(0.0, 1.0);
    Some(action)
}

/// 解析 RL 模型原始输出为 ActionOutputLegacy（单参数模式，legacy）
///
/// 输出格式: [p_batt_set, load_shedding, pv_limit, confidence]
/// v1.x 单参数模式：p_batt_set 直接作为有功设定值
pub fn parse_action_output_legacy(
    raw: &[f32],
    config: &ActionSpaceConfig,
) -> Option<ActionOutputLegacy> {
    if raw.len() < 4 {
        return None;
    }

    Some(ActionOutputLegacy {
        p_batt_set: (raw[0] as f64).clamp(
            -config.max_batt_discharge_power,
            config.max_batt_charge_power,
        ),
        load_shedding: (raw[1] as f64).clamp(0.0, config.max_load_shedding),
        pv_limit: (raw[2] as f64).clamp(0.0, 1.0),
        confidence: raw.get(3).copied().unwrap_or(0.5) as f64,
    })
}

/// MADDPG/PPO 决策模型
pub struct RLModel {
    config: RlConfig,
    runtime: RknnRuntime,
    /// v2.5: 动作空间配置（可配置化），用于 parse_action_output 值域 clamp
    action_space_config: ActionSpaceConfig,
}

impl RLModel {
    pub fn new(
        config: RlConfig,
        action_space_config: ActionSpaceConfig,
    ) -> Result<Self, AiEngineError> {
        let runtime = RknnRuntime::new(&config.model_path, config.expected_sha256.as_deref())?;
        Ok(Self {
            config,
            runtime,
            action_space_config,
        })
    }

    pub async fn load(&mut self) -> Result<(), AiEngineError> {
        self.runtime.load().await
    }

    /// 执行决策（使用 SystemState）
    pub async fn decide(&self, state: &SystemState) -> Result<ActionOutput, AiEngineError> {
        if !self.runtime.is_loaded() {
            return Err(AiEngineError::ModelNotLoaded);
        }
        let input = state.to_features();
        let output = self.runtime.run(&input).await?;
        parse_action_output(&output, &self.action_space_config)
            .ok_or_else(|| AiEngineError::InferenceFailed("输出维度不足".into()))
    }

    /// 执行决策（使用完整融合状态 FusedSystemState）
    pub async fn decide_fused(
        &self,
        state: &FusedSystemState,
    ) -> Result<ActionOutput, AiEngineError> {
        if !self.runtime.is_loaded() {
            return Err(AiEngineError::ModelNotLoaded);
        }
        let input = state.to_input_vector();
        debug_assert_eq!(input.len(), 48, "输入维度必须为 48");
        validate_input_vector(&input)?;
        let output = self.runtime.run(&input).await?;
        parse_action_output(&output, &self.action_space_config)
            .ok_or_else(|| AiEngineError::InferenceFailed("输出维度不足".into()))
    }

    pub fn model_type(&self) -> ModelType {
        match self.config.algorithm {
            RlAlgorithm::MADDPG => ModelType::MADDPG,
            RlAlgorithm::PPO => ModelType::PPO,
        }
    }

    pub fn algorithm(&self) -> RlAlgorithm {
        self.config.algorithm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> RlConfig {
        RlConfig {
            model_path: std::path::PathBuf::from("/tmp/test_rl.rknn"),
            algorithm: RlAlgorithm::MADDPG,
            quantization: crate::config::QuantizationType::INT8,
            expected_sha256: None,
        }
    }

    fn create_test_state() -> SystemState {
        SystemState {
            battery_soc: 0.5,
            pv_power: 10.0,
            load_power: 5.0,
            grid_power: 2.0,
            transformer_load: 20.0,
            battery_power: -50.0,
            voltage_phase_a: 1.0,
            voltage_phase_b: 1.0,
            voltage_phase_c: 1.0,
        }
    }

    fn default_action_space_config() -> ActionSpaceConfig {
        ActionSpaceConfig::default_config()
    }

    #[test]
    fn test_rl_model_creation() {
        let config = create_test_config();
        let action_cfg = default_action_space_config();
        let model = RLModel::new(config, action_cfg);
        assert!(model.is_ok());
    }

    #[test]
    fn test_rl_model_type() {
        let config = create_test_config();
        let action_cfg = default_action_space_config();
        let model = RLModel::new(config, action_cfg).unwrap();
        assert_eq!(model.model_type(), ModelType::MADDPG);
    }

    #[test]
    fn test_system_state_to_features_9_dim() {
        let state = create_test_state();
        let features = state.to_features();
        assert_eq!(features.len(), 9);
        assert_eq!(features[0], 0.5);
        assert_eq!(features[5], -50.0);
        assert_eq!(features[6], 1.0);
    }

    #[test]
    fn test_system_state_from_features() {
        let features = vec![0.5_f32, 10.0, 5.0, 2.0, 20.0, -50.0, 1.0, 1.0, 1.0];
        let state = SystemState::from_features(&features);
        assert!(state.is_some());
        let state = state.unwrap();
        assert_eq!(state.battery_soc, 0.5);
        assert_eq!(state.voltage_phase_a, 1.0);
    }

    #[test]
    fn test_system_state_from_features_insufficient_dims() {
        let features = vec![0.5_f32, 10.0, 5.0, 2.0, 20.0]; // 旧 5 维
        assert!(SystemState::from_features(&features).is_none());
    }

    #[test]
    fn test_parse_action_output_5_fields() {
        let raw = vec![100.0_f32, 20.0, 10.0, 0.8, 0.9];
        let cfg = default_action_space_config();
        let action = parse_action_output(&raw, &cfg).unwrap();
        assert_eq!(action.p_ref, 100.0);
        assert_eq!(action.k_droop, 20.0);
        assert_eq!(action.load_shedding, 10.0);
        assert_eq!(action.pv_limit, 0.8);
        assert_eq!(action.confidence, 0.9);
    }

    #[test]
    fn test_parse_action_output_clamp_bounds() {
        // p_ref 超出范围应被 clamp
        let raw = vec![600.0_f32, 0.0, 0.0, 1.0, 0.8];
        let cfg = default_action_space_config();
        let action = parse_action_output(&raw, &cfg).unwrap();
        assert!(action.p_ref <= 50.0);
        assert!(action.p_ref >= -50.0);
    }

    #[test]
    fn test_parse_action_output_insufficient_dims() {
        let cfg = default_action_space_config();
        assert!(parse_action_output(&[1.0, 2.0], &cfg).is_none());
    }

    #[test]
    fn test_action_output_no_compens_factor() {
        // 验证 ActionOutput 不包含 compens_factor 字段
        let action = ActionOutput {
            p_ref: 0.0,
            k_droop: 0.0,
            load_shedding: 0.0,
            pv_limit: 1.0,
            confidence: 0.8,
        };
        assert_eq!(action.confidence, 0.8);
    }

    #[test]
    fn test_parse_action_output_uses_action_space_config() {
        // 自定义配置：充电上限 30kW，放电上限 40kW，负荷上限 30kW
        let mut cfg = ActionSpaceConfig::default_config();
        cfg.max_batt_charge_power = 30.0;
        cfg.max_batt_discharge_power = 40.0;
        cfg.max_load_shedding = 30.0;

        // p_ref = 100（充电）应被 clamp 到 30
        let raw = vec![100.0_f32, 0.0, 0.0, 1.0, 0.8];
        let action = parse_action_output(&raw, &cfg).unwrap();
        assert!(action.p_ref <= 30.0);

        // p_ref = -100（放电）应被 clamp 到 -40
        let raw = vec![-100.0_f32, 0.0, 0.0, 1.0, 0.8];
        let action = parse_action_output(&raw, &cfg).unwrap();
        assert!(action.p_ref >= -40.0);

        // load_shedding = 100 应被 clamp 到 30
        let raw = vec![0.0_f32, 0.0, 100.0, 1.0, 0.8];
        let action = parse_action_output(&raw, &cfg).unwrap();
        assert!(action.load_shedding <= 30.0);
    }

    #[test]
    fn test_parse_action_output_k_droop_clamp() {
        // k_droop 超出范围应被 clamp
        let mut cfg = default_action_space_config();
        cfg.k_droop_min = Some(-50.0);
        cfg.k_droop_max = Some(50.0);

        // k_droop = 200 应被 clamp 到 50
        let raw = vec![0.0_f32, 200.0, 0.0, 1.0, 0.8];
        let action = parse_action_output(&raw, &cfg).unwrap();
        assert!(action.k_droop <= 50.0);

        // k_droop = -200 应被 clamp 到 -50
        let raw = vec![0.0_f32, -200.0, 0.0, 1.0, 0.8];
        let action = parse_action_output(&raw, &cfg).unwrap();
        assert!(action.k_droop >= -50.0);
    }

    #[test]
    fn test_parse_action_output_legacy() {
        let raw = vec![100.0_f32, 10.0, 0.8, 0.9];
        let cfg = default_action_space_config();
        let action = parse_action_output_legacy(&raw, &cfg).unwrap();
        assert_eq!(action.p_batt_set, 100.0);
        assert_eq!(action.load_shedding, 10.0);
        assert_eq!(action.pv_limit, 0.8);
        assert_eq!(action.confidence, 0.9);
    }

    #[test]
    fn test_parse_action_output_legacy_insufficient_dims() {
        let cfg = default_action_space_config();
        assert!(parse_action_output_legacy(&[1.0, 2.0], &cfg).is_none());
    }
}
