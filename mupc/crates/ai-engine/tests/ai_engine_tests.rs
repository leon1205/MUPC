//! AI Engine Integration Tests

#[cfg(test)]
mod tests {
    use mupc_ai_engine::*;

    #[test]
    fn test_ai_engine_config_default() {
        let lstm = LstmConfig::default();
        assert_eq!(lstm.input_window_secs, 3600);
        assert_eq!(lstm.output_horizon_secs, 900);

        let rl = RlConfig::default();
        assert_eq!(rl.algorithm, RlAlgorithm::MADDPG);

        let online = OnlineUpdateConfig::default();
        assert!(!online.enabled);
    }

    #[test]
    fn test_quantization_type() {
        assert_eq!(QuantizationType::INT8, QuantizationType::INT8);
    }

    #[test]
    fn test_model_type() {
        assert_eq!(ModelType::LSTM, ModelType::LSTM);
    }
}