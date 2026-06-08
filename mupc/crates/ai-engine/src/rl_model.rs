//! MADDPG/PPO 强化学习决策模型
//!
//! 用于微电网能量管理决策
//! 输入：系统状态 (SOC, PV, Load, Grid, Transformer, Battery Power, Voltage_A/B/C)
//! 输出：最优动作 (p_batt_set, q_batt_set, load_shedding, pv_limit, confidence)

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

/// RL 模型输出（4 维动作 + 置信度 = 5 字段）
#[derive(Debug, Clone)]
pub struct ActionOutput {
    /// A1: 电池有功功率设定值 (kW), [-500.0, 500.0], 负=充电, 正=放电
    pub p_batt_set: f64,
    /// A2: 无功功率设定值 (kVar), [-300.0, 300.0], 负=感性/吸收, 正=容性/释放
    pub q_batt_set: f64,
    /// A3: 可中断负荷切除量 (kW), [0.0, 500.0]
    pub load_shedding: f64,
    /// A4: 光伏限功率比例, [0.0, 1.0]
    pub pv_limit: f64,
    /// 决策置信度 [0.0, 1.0]
    pub confidence: f64,
}

/// 解析 RL 模型原始输出为 ActionOutput
///
/// 输出格式: [p_batt_set, q_batt_set, load_shedding, pv_limit, confidence]
/// 注意：v2.4 分层控制架构下 q_batt_set 和 pv_limit 由实时控制模块管理，
/// 此处仅做值域 clamp，不做 dispatch 约束（由 ActionValidator 统一处理）。
pub fn parse_action_output(raw: &[f32]) -> Option<ActionOutput> {
    if raw.len() < 5 {
        return None;
    }
    let mut action = ActionOutput {
        p_batt_set: (raw[0] as f64).clamp(-500.0, 500.0),
        q_batt_set: (raw[1] as f64).clamp(-300.0, 300.0),
        load_shedding: (raw[2] as f64).clamp(0.0, 500.0),
        pv_limit: (raw[3] as f64).clamp(0.0, 1.0),
        confidence: raw.get(4).copied().unwrap_or(0.5) as f64,
    };
    action.confidence = action.confidence.clamp(0.0, 1.0);
    Some(action)
}

/// MADDPG/PPO 决策模型
pub struct RLModel {
    config: RlConfig,
    runtime: RknnRuntime,
}

impl RLModel {
    pub fn new(config: RlConfig) -> Result<Self, AiEngineError> {
        let runtime = RknnRuntime::new(&config.model_path, config.expected_sha256.as_deref())?;
        Ok(Self { config, runtime })
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
        parse_action_output(&output)
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
        parse_action_output(&output)
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

    #[test]
    fn test_rl_model_creation() {
        let config = create_test_config();
        let model = RLModel::new(config);
        assert!(model.is_ok());
    }

    #[test]
    fn test_rl_model_type() {
        let config = create_test_config();
        let model = RLModel::new(config).unwrap();
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
        let raw = vec![100.0_f32, 50.0, 10.0, 0.8, 0.9];
        let action = parse_action_output(&raw).unwrap();
        assert_eq!(action.p_batt_set, 100.0);
        assert_eq!(action.q_batt_set, 50.0);
        assert_eq!(action.load_shedding, 10.0);
        assert_eq!(action.pv_limit, 0.8);
        assert_eq!(action.confidence, 0.9);
    }

    #[test]
    fn test_parse_action_output_clamp_bounds() {
        // p_batt_set 超出范围应被 clamp
        let raw = vec![600.0_f32, 0.0, 0.0, 1.0, 0.8];
        let action = parse_action_output(&raw).unwrap();
        assert!(action.p_batt_set <= 500.0);
        assert!(action.p_batt_set >= -500.0);
    }

    #[test]
    fn test_parse_action_output_insufficient_dims() {
        assert!(parse_action_output(&[1.0, 2.0]).is_none());
    }

    #[test]
    fn test_action_output_no_compens_factor() {
        // 验证 ActionOutput 不包含 compens_factor 字段
        let action = ActionOutput {
            p_batt_set: 0.0,
            q_batt_set: 0.0,
            load_shedding: 0.0,
            pv_limit: 1.0,
            confidence: 0.8,
        };
        assert_eq!(action.confidence, 0.8);
    }
}
