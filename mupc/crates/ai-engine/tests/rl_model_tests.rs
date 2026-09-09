//! RL Model Integration Tests

use mupc_ai_engine::action_space::ActionSpaceConfig;
use mupc_ai_engine::config::{QuantizationType, RlAlgorithm, RlConfig};
use mupc_ai_engine::rl_model::{parse_action_output, RLModel, SystemState};
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> RlConfig {
        RlConfig {
            model_path: PathBuf::from("/tmp/test_rl.rknn"),
            algorithm: RlAlgorithm::MADDPG,
            quantization: QuantizationType::INT8,
            expected_sha256: None,
        }
    }

    #[test]
    fn test_rl_config_creation() {
        let config = create_test_config();
        assert_eq!(config.algorithm, RlAlgorithm::MADDPG);
    }

    #[test]
    fn test_system_state_conversion_9_dim() {
        let state = SystemState {
            battery_soc: 0.75,
            pv_power: 15.0,
            load_power: 8.0,
            grid_power: 1.0,
            transformer_load: 25.0,
            battery_power: -50.0,
            voltage_phase_a: 1.0,
            voltage_phase_b: 1.0,
            voltage_phase_c: 1.0,
        };
        let features = state.to_features();
        assert_eq!(features.len(), 9);
        assert_eq!(features[0], 0.75);
        assert_eq!(features[5], -50.0);

        let state2 = SystemState::from_features(&features);
        assert!(state2.is_some());
        assert_eq!(state2.unwrap().battery_soc, 0.75);
    }

    #[test]
    fn test_parse_action_output_2_fields_with_defaults() {
        // v2.15 后动作空间精简为 2 维（p_ref + k_droop），load_shedding/pv_limit 下沉策略引擎：
        // parse_action_output 只消费前 2 个 tanh 归一化输入，多余字段被忽略并返回固定默认值。
        // （原 5 字段断言为 v1.x 语义，已随动作空间演进过时）
        let raw = vec![0.6_f32, 0.2, 10.0, 0.8, 0.9];
        let cfg = ActionSpaceConfig::default_config();
        let action = parse_action_output(&raw, &cfg).unwrap();
        assert!((action.p_ref - 30.0).abs() < 1e-4); // 0.6 * 50（max_batt_discharge_power）
        assert!((action.k_droop - 18.0).abs() < 1e-4); // 0.2*15+15（默认区间 [0,30]）
        assert_eq!(action.load_shedding, 0.0);
        assert_eq!(action.pv_limit, 1.0);
        assert_eq!(action.confidence, 0.5);
    }
}
