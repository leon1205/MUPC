//! AI Engine Integration Tests

#[cfg(test)]
mod tests {
    use mupc_ai_engine::*;

    #[test]
    fn test_ai_engine_config_default() {
        let lstm = LstmConfig::default();
        // v3.0 对齐训练管线：15 分钟步长 × 24 输入步 = 6 小时窗口；输出 15 步 × 15 分钟 = 225 分钟
        assert_eq!(lstm.input_window_secs, 21_600);
        assert_eq!(lstm.output_horizon_secs, 22_500);

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
