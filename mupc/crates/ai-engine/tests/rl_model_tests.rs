//! RL Model Integration Tests

use mupc_ai_engine::config::{RlConfig, RlAlgorithm, QuantizationType};
use mupc_ai_engine::rl_model::{RLModel, SystemState, parse_action_output};
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
    fn test_parse_action_output_5_fields() {
        let raw = vec![100.0_f32, 50.0, 10.0, 0.8, 0.9];
        let action = parse_action_output(&raw, None).unwrap();
        assert_eq!(action.p_batt_set, 100.0);
        assert_eq!(action.q_batt_set, 50.0);
        assert_eq!(action.load_shedding, 10.0);
        assert_eq!(action.pv_limit, 0.8);
        assert_eq!(action.confidence, 0.9);
    }
}
