//! LSTM Model Integration Tests

use mupc_ai_engine::config::LstmConfig;
use mupc_ai_engine::lstm_model::LstmModel;
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> LstmConfig {
        LstmConfig {
            model_path: PathBuf::from("/tmp/test_lstm.rknn"),
            input_window_secs: 3600,
            output_horizon_secs: 900,
            step_seconds: 60,
            quantization: mupc_ai_engine::config::QuantizationType::INT8,
            expected_sha256: None,
            input_features: 1,
            yesterday_offset_steps: 96,
        }
    }

    #[test]
    fn test_lstm_config_default() {
        let config = create_test_config();
        assert_eq!(config.input_window_secs, 3600);
        assert_eq!(config.output_horizon_secs, 900);
    }
}
