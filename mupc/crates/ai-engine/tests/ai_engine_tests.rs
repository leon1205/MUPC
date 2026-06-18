//! AI Engine Integration Tests

#[cfg(test)]
mod tests {
    use mupc_ai_engine::*;

    #[test]
    fn test_ai_engine_config_default() {
        let lstm = LstmConfig::default();
        // v2.16: 默认值已变更（对齐 MUPC-AI2 训练管线 15 分钟步长）
        assert_eq!(lstm.input_window_secs, 21600);  // 6 小时
        assert_eq!(lstm.output_horizon_secs, 22500); // 225 分钟 = 15 步 × 15 分钟
        assert_eq!(lstm.step_seconds, 900);          // 15 分钟步长

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
