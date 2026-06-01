//! LSTM 时序预测模型
//!
//! 用于光伏出力/负荷预测
//! 输入：历史时间序列数据
//! 输出：未来预测值及置信度

use crate::config::LstmConfig;
use crate::error::AiEngineError;
use crate::rknn_runtime::RknnRuntime;

/// LSTM 模型输入
#[derive(Debug, Clone)]
pub struct LstmInput {
    /// 历史时间序列数据（按时间顺序）
    pub history: Vec<f32>,
    /// 时间戳（UTC 秒）
    pub timestamp: i64,
}

/// LSTM 模型输出
#[derive(Debug, Clone)]
pub struct LstmOutput {
    /// 预测值（未来 N 个时间步）
    pub predictions: Vec<f32>,
    /// 置信度 (0.0-1.0)
    pub confidence: f64,
}

/// LSTM 预测模型
pub struct LstmModel {
    config: LstmConfig,
    runtime: RknnRuntime,
}

impl LstmModel {
    /// 创建 LSTM 模型
    pub fn new(config: LstmConfig) -> Result<Self, AiEngineError> {
        let runtime = RknnRuntime::new(&config.model_path)?;
        Ok(Self { config, runtime })
    }

    /// 加载模型
    pub async fn load(&mut self) -> Result<(), AiEngineError> {
        self.runtime.load().await
    }

    /// 执行预测
    ///
    /// 输入：历史时间序列（通常为 60 分钟数据点）
    /// 输出：未来预测值（通常为 30 分钟）
    pub async fn predict(&self, input: &LstmInput) -> Result<LstmOutput, AiEngineError> {
        // 检查模型是否已加载
        if !self.runtime.is_loaded() {
            return Err(AiEngineError::ModelNotLoaded);
        }

        // 计算输入大小：每分钟一个数据点
        // input_window_secs = 3600s = 60 分钟
        let input_size = self.config.input_window_secs as usize / 60;

        // 验证输入长度
        if input.history.len() != input_size {
            return Err(AiEngineError::InputShapeMismatch {
                expected: vec![1, input_size as i32],
                actual: vec![1, input.history.len() as i32],
            });
        }

        // 执行推理
        let output = self.runtime.run(&input.history).await?;

        // 计算输出步数：每分钟一个预测点
        // output_horizon_secs = 1800s = 30 分钟
        let output_size = self.config.output_horizon_secs as usize / 60;
        let predictions: Vec<f32> = output.into_iter().take(output_size).collect();

        // 简化置信度计算：基于输出方差
        // 实际应使用贝叶斯方法或集成方法
        let variance = if predictions.len() > 1 {
            let mean = predictions.iter().sum::<f32>() / predictions.len() as f32;
            predictions.iter().map(|p| (p - mean).powi(2)).sum::<f32>() / predictions.len() as f32
        } else {
            0.0
        };
        let confidence = (1.0 - variance.min(1.0)).max(0.0);

        Ok(LstmOutput {
            predictions,
            confidence: confidence as f64,
        })
    }

    /// 获取模型类型
    pub fn model_type(&self) -> crate::config::ModelType {
        crate::config::ModelType::LSTM
    }

    /// 获取输入窗口大小（秒）
    pub fn input_window_secs(&self) -> u64 {
        self.config.input_window_secs
    }

    /// 获取输出预测范围（秒）
    pub fn output_horizon_secs(&self) -> u64 {
        self.config.output_horizon_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> LstmConfig {
        LstmConfig {
            model_path: std::path::PathBuf::from("/tmp/test_lstm.rknn"),
            input_window_secs: 3600,   // 60 分钟
            output_horizon_secs: 1800, // 30 分钟
            quantization: crate::config::QuantizationType::INT8,
        }
    }

    #[test]
    fn test_lstm_model_creation() {
        let config = create_test_config();
        let model = LstmModel::new(config);
        assert!(model.is_ok());
    }

    #[test]
    fn test_lstm_model_type() {
        let config = create_test_config();
        let model = LstmModel::new(config).unwrap();
        assert_eq!(model.model_type(), crate::config::ModelType::LSTM);
    }

    #[test]
    fn test_lstm_input_window() {
        let config = create_test_config();
        let model = LstmModel::new(config).unwrap();
        assert_eq!(model.input_window_secs(), 3600);
        assert_eq!(model.output_horizon_secs(), 1800);
    }
}
