//! LSTM Model Integration Tests

use mupc_ai_engine::config::LstmConfig;
use mupc_ai_engine::lstm_model::LstmModel;
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> LstmConfig {
        // 测试用配置：使用 1 分钟步长，便于构造小规模测试数据
        // v2.16 修订：明确此处 input/output 步长为测试专用值（24步/15步），
        // 不等同于生产默认值（21600/22500）
        LstmConfig {
            model_path: PathBuf::from("/tmp/test_lstm.rknn"),
            input_window_secs: 3600,
            output_horizon_secs: 900,
            step_seconds: 60, // 测试用 1 分钟步长（生产默认 900s = 15 分钟）
            quantization: mupc_ai_engine::config::QuantizationType::INT8,
            expected_sha256: None,
        }
    }

    #[test]
    fn test_lstm_config_default() {
        let config = create_test_config();
        // 测试专用值：1 分钟步长下的输入 60 点 / 输出 15 点
        // 生产默认值（v2.16）= 21600/22500 + step_seconds=900，见 ai_engine_tests.rs
        assert_eq!(config.input_window_secs, 3600);
        assert_eq!(config.output_horizon_secs, 900);
        assert_eq!(config.step_seconds, 60);
    }

    /// v2.16: 验证生产默认配置
    #[test]
    fn test_lstm_config_production_default() {
        let config = LstmConfig::default();
        assert_eq!(config.step_seconds, 900, "v2.16 默认步长 900s（15 分钟）");
        assert_eq!(
            config.input_window_secs / config.step_seconds,
            24,
            "v2.16 默认输入步数 24"
        );
        assert_eq!(
            config.output_horizon_secs / config.step_seconds,
            15,
            "v2.16 默认输出步数 15"
        );
    }
}
