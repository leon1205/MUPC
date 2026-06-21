//! LSTM 时序预测模型
//!
//! 用于光伏出力/负荷预测
//! 输入：历史时间序列数据
//! 输出：未来预测值及置信度

use crate::config::LstmConfig;
use crate::error::AiEngineError;
use crate::load_covariates::LoadCovariates;
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

// ============================================================================
// v2.11 分位数预测结构体
// ============================================================================

/// 分位数预测（v2.11 新增）
#[derive(Debug, Clone)]
pub struct QuantilePrediction {
    /// 分位数（0.0 ~ 1.0）
    pub quantile: f32,
    /// 预测值 (kW)
    pub value: f32,
}

/// 概率负荷预测输出（v2.11 新增）
#[derive(Debug, Clone)]
pub struct ProbabilisticLoadOutput {
    /// 预测时间戳
    pub timestamp: i64,
    /// 各分位数预测值
    pub quantiles: Vec<QuantilePrediction>,
    /// 基础负荷（50% 分位数）
    pub base_load: f32,
    /// 冲击负荷发生概率
    pub shock_probability: f64,
    /// 预测置信度
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
        let runtime = RknnRuntime::new(&config.model_path, config.expected_sha256.as_deref())?;
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
        // output_horizon_secs = 900s = 15 分钟（默认）
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

    // ============================================================================
    // v2.11 分位数预测方法
    // ============================================================================

    /// v2.11: 分位数预测
    ///
    /// 输入：历史时间序列 + 协变量
    /// 输出：多分位数预测结果
    pub async fn predict_quantiles(
        &self,
        input: &LstmInput,
        covariates: &LoadCovariates,
    ) -> Result<ProbabilisticLoadOutput, AiEngineError> {
        // 1. 获取多个分位数的预测值
        let quantile_values = self.predict_multi_quantile(input, covariates).await?;

        // 2. 提取基础负荷（50% 分位数）
        let base_load = quantile_values
            .iter()
            .find(|q| (q.quantile - 0.5).abs() < 0.01)
            .map(|q| q.value)
            .unwrap_or(0.0);

        // 3. 计算冲击负荷概率
        let high_quantile = quantile_values
            .iter()
            .find(|q| (q.quantile - 0.9).abs() < 0.01)
            .map(|q| q.value)
            .unwrap_or(base_load);

        let shock_probability = self.calculate_shock_probability(base_load, high_quantile);

        // 4. 计算置信度
        let confidence = self.calculate_quantile_confidence(&quantile_values);

        Ok(ProbabilisticLoadOutput {
            timestamp: input.timestamp,
            quantiles: quantile_values,
            base_load,
            shock_probability,
            confidence,
        })
    }

    /// v2.11: 预测多分位数（基于协变量的分位数预测）
    ///
    /// 使用协变量（温度、季节、时段）调整分位数预测：
    /// - 高温时（空调季）负荷不确定性增大，P90 向上偏移
    /// - 夜间时段负荷更稳定，分位数间距收窄
    /// - 节假日/周末负荷模式不同，基线调整
    async fn predict_multi_quantile(
        &self,
        input: &LstmInput,
        covariates: &LoadCovariates,
    ) -> Result<Vec<QuantilePrediction>, AiEngineError> {
        let output = self.predict(input).await?;
        let base = output.predictions.first().copied().unwrap_or(0.0);

        // 计算协变量调整因子
        let (base_multiplier, spread_multiplier) = Self::calculate_covariate_adjustment(covariates);

        // P50 (基线): 应用基础协变量调整
        let p50 = base * base_multiplier;

        // P10 (低分位数): 考虑下界不确定性
        // 基础分位数间距 * 季节/时段扩展因子
        let p10_spread = (p50 - base) * spread_multiplier;
        let p10 = p50 - p10_spread;

        // P90 (高分位数): 考虑上界不确定性（高温/峰值时段不确定性更大）
        let p90_spread = (base - p50) * spread_multiplier;
        let p90 = p50 + p90_spread;

        Ok(vec![
            QuantilePrediction {
                quantile: 0.1,
                value: p10.max(0.0),
            },
            QuantilePrediction {
                quantile: 0.5,
                value: p50.max(0.0),
            },
            QuantilePrediction {
                quantile: 0.9,
                value: p90.max(0.0),
            },
        ])
    }

    /// v2.11: 基于协变量计算分位数调整因子
    ///
    /// 返回 (base_multiplier, spread_multiplier)：
    /// - base_multiplier: 基础负荷调整系数（考虑温度、季节、时段对平均负荷的影响）
    /// - spread_multiplier: 分位数间距系数（反映不确定性程度）
    fn calculate_covariate_adjustment(covariates: &LoadCovariates) -> (f32, f32) {
        // 1. 温度调整因子
        // 基准温度 25°C，偏离越大负荷变化越大
        let temp_diff = (covariates.temperature - 25.0).abs();
        // 温度偏离基准 10°C 以内：负荷变化 2%/°C
        // 温度偏离超过 10°C（高温）：空调负荷激增，不确定性增大
        let temp_factor = if covariates.temperature > 35.0 {
            // 高温空调季：基线增加 20%，不确定性大幅增加
            1.20
        } else if covariates.temperature > 25.0 {
            // 温和期：基线略增
            1.0 + 0.02 * temp_diff
        } else {
            // 低温期：基线略降（少空调）
            1.0 - 0.01 * temp_diff
        };

        // 2. 时段调整因子（小时 0-23）
        let hour_factor = if covariates.hour >= 7 && covariates.hour <= 22 {
            // 白天时段：标准负荷
            1.0
        } else {
            // 夜间时段：负荷较低且更稳定，不确定性小
            0.7
        };

        // 3. 日期类型调整因子
        let date_factor = match covariates.date_type {
            0 => 1.0,  // 工作日：标准负荷
            1 => 0.85, // 周末：负荷降低
            2 => 0.75, // 节假日：负荷进一步降低
            _ => 1.0,
        };

        // 4. 季节调整因子
        let season_factor = if covariates.is_irrigation_season {
            // 灌溉季：额外基础负荷（灌溉设备）
            1.15
        } else {
            1.0
        };

        // 合成基础调整因子
        let base_multiplier = temp_factor * hour_factor * date_factor * season_factor;

        // 5. 不确定性扩展因子（分位数间距系数）
        // 高温、峰值时段、节假日的不确定性最大
        let mut spread = 1.0;

        // 高温不确定性
        if covariates.temperature > 35.0 {
            spread *= 1.5; // 高温空调季不确定性大幅增加
        } else if covariates.temperature > 30.0 {
            spread *= 1.2;
        }

        // 峰值时段（11-14, 18-21）不确定性
        if (covariates.hour >= 11 && covariates.hour <= 14)
            || (covariates.hour >= 18 && covariates.hour <= 21)
        {
            spread *= 1.3;
        }

        // 夜间不确定性小
        if covariates.hour < 6 || covariates.hour > 22 {
            spread *= 0.7;
        }

        // 节假日/周末不确定性增加
        if covariates.date_type > 0 {
            spread *= 1.2;
        }

        (base_multiplier, spread)
    }

    /// v2.11: 计算冲击负荷发生概率
    ///
    /// P(shock) = 1 - Φ((median - mean) / std)
    /// 其中 std ≈ P90 - P50（高分位数与中位数的差值反映不确定性）
    fn calculate_shock_probability(&self, base_load: f32, high_quantile: f32) -> f64 {
        let spread = (high_quantile - base_load).max(1e-6);

        // 假设负荷服从正态分布，spread ≈ 1.28 * std（P90 对应 1.28σ）
        let std_approx = spread / 1.28;

        // 冲击阈值：超过 base_load + 2σ 视为冲击负荷
        let shock_threshold = base_load + 2.0 * std_approx;

        // 计算 P(load > shock_threshold)
        let z_score = (shock_threshold - base_load) / std_approx.max(1e-6);

        // 使用误差函数近似正态分布 CDF
        let shock_prob = 0.5 * Self::erfc(z_score / std::f32::consts::SQRT_2);

        shock_prob as f64
    }

    /// v2.11: 误差函数近似（用于正态分布 CDF 计算）
    fn erfc(x: f32) -> f32 {
        let abs_x = x.abs();
        if abs_x > 8.0 {
            return 0.0;
        }
        let exp_term = (-x * x).exp();
        let denom = std::f32::consts::PI * abs_x + (std::f32::consts::PI * x * x + 4.0).sqrt();
        exp_term / denom
    }

    /// v2.11: 计算置信度
    fn calculate_quantile_confidence(&self, quantiles: &[QuantilePrediction]) -> f64 {
        if quantiles.len() < 2 {
            return 0.5;
        }
        // 基于分位数间距计算置信度
        // 间距越小，置信度越高
        let p50 = quantiles.iter().find(|q| (q.quantile - 0.5).abs() < 0.01);
        let p90 = quantiles.iter().find(|q| (q.quantile - 0.9).abs() < 0.01);

        if let (Some(p50), Some(p90)) = (p50, p90) {
            let spread_ratio = (p90.value - p50.value) / p50.value.max(1e-6);
            (1.0 - spread_ratio.min(1.0)).max(0.0) as f64
        } else {
            0.5
        }
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

    /// 获取内部 RknnRuntime 引用（用于状态检查）
    pub fn runtime(&self) -> &RknnRuntime {
        &self.runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> LstmConfig {
        LstmConfig {
            model_path: std::path::PathBuf::from("/tmp/test_lstm.rknn"),
            input_window_secs: 3600,  // 60 分钟（测试可保留旧值验证向后兼容）
            output_horizon_secs: 900, // 15 分钟（测试可保留旧值验证向后兼容）
            step_seconds: 60,         // 测试用 1 分钟步长（小步长便于构造数据）
            quantization: crate::config::QuantizationType::INT8,
            expected_sha256: None,
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
        assert_eq!(model.output_horizon_secs(), 900);
    }
}
