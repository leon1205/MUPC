//! RL Model Integration Tests

use mupc_ai_engine::config::{RlConfig, RlAlgorithm, QuantizationType};
use mupc_ai_engine::rl_model::{RLModel, SystemState};
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> RlConfig {
        RlConfig {
            model_path: PathBuf::from("/tmp/test_rl.rknn"),
            algorithm: RlAlgorithm::MADDPG,
            quantization: QuantizationType::INT8,
        }
    }

    #[test]
    fn test_rl_config_creation() {
        let config = create_test_config();
        assert_eq!(config.algorithm, RlAlgorithm::MADDPG);
    }

    #[test]
    fn test_system_state_conversion() {
        let state = SystemState {
            battery_soc: 0.75,
            pv_power: 15.0,
            load_power: 8.0,
            grid_power: 1.0,
            transformer_load: 25.0,
        };
        let features = state.to_features();
        assert_eq!(features.len(), 5);

        let state2 = SystemState::from_features(&features);
        assert!(state2.is_some());
        assert_eq!(state2.unwrap().battery_soc, 0.75);
    }
}