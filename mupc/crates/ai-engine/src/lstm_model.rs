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
///
/// v2.16: 删除 `confidence` 字段（原基于预测序列方差，数学上无意义）。
/// grep 全工程无任何代码读取该字段，属于死代码。
#[derive(Debug, Clone)]
pub struct LstmOutput {
    /// 预测值（未来 N 个时间步，长度 = output_horizon_secs / step_seconds）
    pub predictions: Vec<f32>,
}

// ============================================================================
// v2.11 分位数预测结构体（v2.16 重构为 15 步结构）
// ============================================================================

/// 单分位数预测（v2.11）
#[derive(Debug, Clone)]
pub struct QuantilePrediction {
    /// 分位数（0.0 ~ 1.0）
    pub quantile: f32,
    /// 预测值 (kW)
    pub value: f32,
}

/// 单步分位数预测（v2.16 新增）
///
/// 每个未来时间步含 P10/P50/P90 三个分位数值。
/// 配合 15 分钟步长，15 步覆盖未来 3.75 小时。
#[derive(Debug, Clone)]
pub struct StepQuantiles {
    /// 步索引（0..14）
    pub step_index: usize,
    /// P10 分位数预测值
    pub p10: f32,
    /// P50 分位数预测值
    pub p50: f32,
    /// P90 分位数预测值
    pub p90: f32,
}

impl StepQuantiles {
    /// 创建单步分位数预测
    pub fn new(step_index: usize, p10: f32, p50: f32, p90: f32) -> Self {
        Self { step_index, p10, p50, p90 }
    }
}

/// 概率负荷预测输出（v2.11 新增，v2.16 重构）
///
/// 15 个未来时间步 × 3 分位数 + 基础负荷 + 冲击概率。
/// `quantile_steps` 长度固定为 15（与 PRD §6.2 D10 一致）。
#[derive(Debug, Clone)]
pub struct ProbabilisticLoadOutput {
    /// 预测时间戳
    pub timestamp: i64,
    /// 15 个未来时间步的分位数预测
    pub quantile_steps: Vec<StepQuantiles>,
    /// 基础负荷（第 1 步 P50）
    pub base_load: f32,
    /// 冲击负荷发生概率
    pub shock_probability: f64,
    /// 分位数预测置信度（基于分位数间距，与 LstmOutput.confidence 无关）
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
    /// v2.16: 步长计算改用 `config.step_seconds`（默认 900s = 15 分钟），
    /// 与 MUPC-AI2 训练管线对齐。新增输出维度校验（原静默截断）。
    ///
    /// 输入：历史时间序列（长度 = input_window_secs / step_seconds）
    /// 输出：未来预测值（长度 = output_horizon_secs / step_seconds）
    pub async fn predict(&self, input: &LstmInput) -> Result<LstmOutput, AiEngineError> {
        // 检查模型是否已加载
        if !self.runtime.is_loaded() {
            return Err(AiEngineError::ModelNotLoaded);
        }

        // v2.16: 使用 step_seconds 统一计算（默认 900s = 15 分钟步长）
        let input_size = self.config.input_window_secs as usize / self.config.step_seconds as usize;
        let output_size =
            self.config.output_horizon_secs as usize / self.config.step_seconds as usize;

        // 验证输入长度
        if input.history.len() != input_size {
            return Err(AiEngineError::InputShapeMismatch {
                expected: vec![1, input_size as i32],
                actual: vec![1, input.history.len() as i32],
            });
        }

        // 执行推理
        let output = self.runtime.run(&input.history).await?;

        // v2.16: 输出维度校验（原静默 take 截断）
        if output.len() < output_size {
            return Err(AiEngineError::OutputShapeMismatch);
        }
        let predictions: Vec<f32> = output.into_iter().take(output_size).collect();

        Ok(LstmOutput { predictions })
    }

    // ============================================================================
    // v2.11 分位数预测方法（v2.16 重构：复用 predict() 结果，消除冗余推理）
    // ============================================================================

    /// v2.16 重构：分位数预测
    ///
    /// 输入：历史时间序列 + 协变量
    /// 输出：15 步 × 3 分位数 + 基础负荷 + 冲击概率
    ///
    /// 性能优化：仅触发 1 次 NPU 推理（原 2 次），后处理全部在 CPU 上完成。
    pub async fn predict_quantiles(
        &self,
        input: &LstmInput,
        covariates: &LoadCovariates,
    ) -> Result<ProbabilisticLoadOutput, AiEngineError> {
        // v2.16: 仅 1 次 NPU 推理（原 predict_multi_quantile + predict_quantiles 共 2 次）
        let point_output = self.predict(input).await?;

        // 计算协变量调整因子（全局共享，每步使用相同因子，仅基线随步变化）
        let (base_multiplier, spread_multiplier) =
            Self::calculate_covariate_adjustment(covariates);

        // v2.16: 对每个未来时间步分别计算 P10/P50/P90
        let quantile_steps: Vec<StepQuantiles> = point_output
            .predictions
            .iter()
            .enumerate()
            .map(|(step_index, &base)| {
                let p50 = (base * base_multiplier).max(0.0);
                // spread = (p50 - base) * spread_multiplier = base * (base_multiplier - 1.0) * spread_multiplier
                // v2.16.1: 修复 C-01 spread 公式错误（原 `p50 - base * base_multiplier` 在 base_multiplier≠1 时退化为 0）
                let spread = (base * (base_multiplier - 1.0)).abs() * spread_multiplier;
                let p10 = (p50 - spread).max(0.0);
                let p90 = p50 + spread;
                StepQuantiles::new(step_index, p10, p50, p90)
            })
            .collect();

        // 提取基础负荷（第 1 步 P50，向后兼容）
        let base_load = quantile_steps.first().map(|s| s.p50).unwrap_or(0.0);

        // 计算冲击负荷概率（基于第 1 步 P50/P90）
        let high_quantile = quantile_steps.first().map(|s| s.p90).unwrap_or(base_load);

        let shock_probability = Self::calculate_shock_probability(base_load, high_quantile);

        // 计算分位数预测置信度（基于第 1 步 P50/P90 间距）
        let confidence = Self::calculate_quantile_confidence(base_load, high_quantile);

        Ok(ProbabilisticLoadOutput {
            timestamp: input.timestamp,
            quantile_steps,
            base_load,
            shock_probability,
            confidence,
        })
    }

    /// 已废弃（v2.16）：原 predict_multi_quantile 逻辑已并入 predict_quantiles。
    /// 保留此函数签名以避免破坏可能的外部调用。
    #[deprecated(
        since = "2.16.0",
        note = "已合并入 predict_quantiles，请使用 predict_quantiles 直接获取 15 步分位数"
    )]
    #[allow(dead_code)]
    async fn predict_multi_quantile(
        &self,
        _input: &LstmInput,
        _covariates: &LoadCovariates,
    ) -> Result<Vec<QuantilePrediction>, AiEngineError> {
        // 占位：保留函数签名以便编译通过
        Ok(Vec::new())
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
            0 => 1.0,   // 工作日：标准负荷
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
    ///
    /// v2.16: 改为静态方法（不依赖 self 状态），便于单元测试
    fn calculate_shock_probability(base_load: f32, high_quantile: f32) -> f64 {
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
    ///
    /// v2.16 重构（C-04 优化）：签名简化为 `(p50, p90) -> f64`，
    /// 避免每次构造 `Vec<QuantilePrediction>` 的堆分配。
    ///
    /// v2.16.1: 改为静态方法（不依赖 self 状态），便于单元测试。
    fn calculate_quantile_confidence(p50_value: f32, p90_value: f32) -> f64 {
        let spread_ratio = (p90_value - p50_value) / p50_value.max(1e-6);
        (1.0 - spread_ratio.min(1.0)).max(0.0) as f64
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

    fn create_default_config() -> LstmConfig {
        LstmConfig::default()
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

    // ========================================================================
    // v2.16 新增测试
    // ========================================================================

    /// LSTM-06: LstmConfig.step_seconds 字段默认值
    #[test]
    fn test_step_seconds_default() {
        let cfg = create_default_config();
        assert_eq!(cfg.step_seconds, 900, "v2.16 默认步长应为 900 秒（15 分钟）");
    }

    /// LSTM-07: 默认配置下输入输出步数
    #[test]
    fn test_default_step_counts() {
        let cfg = create_default_config();
        let input_size = cfg.input_window_secs / cfg.step_seconds;
        let output_size = cfg.output_horizon_secs / cfg.step_seconds;
        assert_eq!(input_size, 24, "v2.16 默认输入步数应为 24（6 小时 × 4 点/小时）");
        assert_eq!(output_size, 15, "v2.16 默认输出步数应为 15（15 步 × 15 分钟）");
    }

    /// LSTM-08: LstmOutput 不再包含 confidence 字段
    #[test]
    fn test_lstm_output_no_confidence() {
        // 编译期测试：如果 confidence 字段被恢复，此断言失败
        let output = LstmOutput {
            predictions: vec![1.0, 2.0, 3.0],
        };
        assert_eq!(output.predictions.len(), 3);
    }

    /// StepQuantiles::new 构造
    #[test]
    fn test_step_quantiles_new() {
        let sq = StepQuantiles::new(0, 10.0, 15.0, 20.0);
        assert_eq!(sq.step_index, 0);
        assert_eq!(sq.p10, 10.0);
        assert_eq!(sq.p50, 15.0);
        assert_eq!(sq.p90, 20.0);
    }

    /// calculate_covariate_adjustment: 标准工况
    #[test]
    fn test_calculate_covariate_adjustment_normal() {
        let cov = LoadCovariates {
            temperature: 25.0,
            hour: 12,
            date_type: 0,
            is_irrigation_season: false,
        };
        let (base, spread) = LstmModel::calculate_covariate_adjustment(&cov);
        // 标准工况：温度 25、12 点、工作日、非灌溉季 → 基础 1.0，不确定性 1.0
        assert!((base - 1.0).abs() < 0.01, "标准工况 base 应接近 1.0");
        assert!((spread - 1.0).abs() < 0.01, "标准工况 spread 应为 1.0");
    }

    /// calculate_covariate_adjustment: 高温空调季
    #[test]
    fn test_calculate_covariate_adjustment_high_temp() {
        let cov = LoadCovariates {
            temperature: 38.0,
            hour: 14,
            date_type: 0,
            is_irrigation_season: false,
        };
        let (base, spread) = LstmModel::calculate_covariate_adjustment(&cov);
        // 高温空调季：基础 ≥ 1.20，不确定性 ≥ 1.5
        assert!(base >= 1.20, "高温基础因子应 >= 1.20");
        assert!(spread >= 1.5, "高温不确定性应放大");
    }

    /// calculate_covariate_adjustment: 夜间低负荷
    #[test]
    fn test_calculate_covariate_adjustment_night() {
        let cov = LoadCovariates {
            temperature: 20.0,
            hour: 3,
            date_type: 0,
            is_irrigation_season: false,
        };
        let (base, spread) = LstmModel::calculate_covariate_adjustment(&cov);
        // 夜间：基础 0.7，不确定性 0.7
        assert!((base - 0.7).abs() < 0.01, "夜间基础因子应为 0.7");
        assert!((spread - 0.7).abs() < 0.01, "夜间不确定性应 <= 1.0");
    }

    /// calculate_shock_probability: 零间距边界
    #[test]
    fn test_calculate_shock_probability_zero_spread() {
        // base == high → spread 极小 → shock_prob 极小
        let prob = LstmModel::calculate_shock_probability(100.0, 100.0);
        assert!(prob < 0.1, "P50 == P90 时冲击概率应接近 0");
    }

    /// calculate_shock_probability: 大间距
    #[test]
    fn test_calculate_shock_probability_large_spread() {
        // base = 100, high = 200 → spread = 100 → std_approx = 100/1.28 ≈ 78
        // z_score = (200-100)/78 ≈ 1.28 → 1 - Φ(1.28) ≈ 0.10
        let prob = LstmModel::calculate_shock_probability(100.0, 200.0);
        assert!(prob > 0.0, "大间距时冲击概率应 > 0");
        assert!(prob < 1.0, "冲击概率应 < 1");
    }

    /// calculate_quantile_confidence: 零间距高置信
    #[test]
    fn test_calculate_quantile_confidence_zero_spread() {
        let conf = LstmModel::calculate_quantile_confidence(100.0, 100.0);
        assert!((conf - 1.0).abs() < 0.01, "零间距置信度应为 1.0");
    }

    /// calculate_quantile_confidence: 大间距低置信
    #[test]
    fn test_calculate_quantile_confidence_large_spread() {
        let conf = LstmModel::calculate_quantile_confidence(100.0, 200.0);
        assert!(conf < 0.5, "大间距置信度应 < 0.5");
    }

    /// erfc: 边界值验证
    #[test]
    fn test_erfc_zero() {
        // erfc(0) ≈ 1
        let v = LstmModel::erfc(0.0);
        assert!((v - 1.0).abs() < 0.01, "erfc(0) 应接近 1.0");
    }

    /// erfc: 大参数快速衰减
    #[test]
    fn test_erfc_large() {
        let v = LstmModel::erfc(10.0);
        assert_eq!(v, 0.0, "erfc(10) 应快速衰减至 0");
    }
}
