//! 自适应权重优化器 (v2.11)
//!
//! 基于元学习(Meta-Learning)的权重优化器，根据历史性能数据自动调整 RL 奖励函数权重。
//!
//! # 架构
//! - `MetaLearner`: 基于性能特征预测最优权重调整方向
//! - `PerformanceCollector`: 收集历史性能数据
//! - `WeightBoundsEnforcer`: 应用物理约束剪裁
//!
//! # 约束
//! - 权重必须为正数 (min: 0.01)
//! - 权重比例归一化 (sum: 8.3)
//! - 单次调整幅度限制 (max: 20%)

use crate::config::{AdaptiveOptimizerConfig, SceneWeights};
use crate::error::AiEngineError;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// 数据结构
// ============================================================================

/// 性能特征（用于元学习器输入）
#[derive(Debug, Clone)]
pub struct PerformanceFeatures {
    /// 光伏消纳率 [0.0, 1.0]
    pub pv_utilization_rate: f64,
    /// 电池循环次数
    pub battery_cycle_count: u32,
    /// 变压器平均负载率 [0.0, 1.0]
    pub trafo_avg_load: f64,
    /// 需量超标次数
    pub demand_violation_count: u32,
    /// 累积奖励
    pub cumulative_reward: f64,
}

/// 权重调整量
#[derive(Debug, Clone)]
pub struct WeightAdjustment {
    /// 各场景权重调整量
    pub seasonal_load_delta: [f64; 8],
    pub commercial_arbitrage_delta: [f64; 2],
    pub demand_control_delta: [f64; 2],
    pub virtual_power_plant_delta: [f64; 3],
    pub ultra_green_delta: [f64; 2],
}

impl Default for WeightAdjustment {
    fn default() -> Self {
        Self {
            seasonal_load_delta: [0.0; 8],
            commercial_arbitrage_delta: [0.0; 2],
            demand_control_delta: [0.0; 2],
            virtual_power_plant_delta: [0.0; 3],
            ultra_green_delta: [0.0; 2],
        }
    }
}

/// 历史性能数据
#[derive(Debug, Clone)]
pub struct HistoricalPerformance {
    pub features: PerformanceFeatures,
    pub timestamp: i64,
}

// ============================================================================
// PerformanceCollector trait
// ============================================================================

/// 性能指标收集器 trait
pub trait PerformanceCollector: Send + Sync {
    /// 收集历史性能数据
    fn collect_historical(&self) -> Result<HistoricalPerformance, AiEngineError>;
    /// 收集当前性能快照
    fn collect_current(&self) -> Result<PerformanceFeatures, AiEngineError>;
}

// ============================================================================
// AdaptiveWeightOptimizer
// ============================================================================

/// 自适应权重优化器
pub struct AdaptiveWeightOptimizer {
    config: AdaptiveOptimizerConfig,
    /// 当前权重快照
    current_weights: RwLock<SceneWeights>,
    /// 权重调整历史
    adjustment_history: RwLock<Vec<WeightAdjustment>>,
    /// 性能指标收集器引用
    performance_collector: Arc<dyn PerformanceCollector>,
}

impl AdaptiveWeightOptimizer {
    /// 创建优化器
    pub fn new(
        config: AdaptiveOptimizerConfig,
        initial_weights: SceneWeights,
        performance_collector: Arc<dyn PerformanceCollector>,
    ) -> Self {
        Self {
            config,
            current_weights: RwLock::new(initial_weights),
            adjustment_history: RwLock::new(Vec::new()),
            performance_collector,
        }
    }

    /// 基于元学习优化权重
    ///
    /// # 输入
    /// - historical_performance: 历史性能指标
    ///
    /// # 输出
    /// - 优化后的 SceneWeights
    pub async fn optimize_weights(
        &self,
        historical_performance: &HistoricalPerformance,
    ) -> Result<SceneWeights, AiEngineError> {
        // 1. 提取性能特征
        let features = self.extract_features(historical_performance);

        // 2. 元学习器预测最优权重调整方向（简化实现）
        let adjustment = self.meta_learn_predict(&features)?;

        // 3. 应用调整（带约束剪裁）
        let new_weights = self.apply_adjustment(&adjustment).await?;

        // 4. 记录调整历史
        self.record_adjustment(historical_performance, &adjustment).await;

        Ok(new_weights)
    }

    /// 提取性能特征
    fn extract_features(&self, perf: &HistoricalPerformance) -> PerformanceFeatures {
        perf.features.clone()
    }

    /// 元学习器预测（简化版，实际应调用小型神经网络）
    ///
    /// 简化实现：基于规则的调整
    /// - 如果光伏消纳率低，增加 w1（光伏消纳权重）
    /// - 如果需量超标次数多，增加 w1（需量控制权重）
    fn meta_learn_predict(
        &self,
        features: &PerformanceFeatures,
    ) -> Result<WeightAdjustment, AiEngineError> {
        let mut delta = WeightAdjustment::default();

        // 根据性能特征调整权重
        // 如果光伏消纳率低，增加 w1（光伏消纳权重）
        if features.pv_utilization_rate < 0.7 {
            delta.seasonal_load_delta[0] = 0.1;
        }

        // 如果需量超标次数多，增加 w1（需量控制权重）
        if features.demand_violation_count > 3 {
            delta.demand_control_delta[0] = 0.15;
        }

        // 如果电池循环次数过多，降低 w2（电池损耗权重相对值）
        if features.battery_cycle_count > 10 {
            delta.seasonal_load_delta[1] = -0.05;
        }

        // 如果变压器过载，降低 w3 或增加惩罚
        if features.trafo_avg_load > 0.85 {
            delta.seasonal_load_delta[2] = 0.1;
        }

        Ok(delta)
    }

    /// 应用权重调整（带物理约束剪裁）
    async fn apply_adjustment(
        &self,
        adjustment: &WeightAdjustment,
    ) -> Result<SceneWeights, AiEngineError> {
        let current = self.current_weights.read().await.clone();
        let mut new_weights = current.clone();

        // 约束1：权重必须为正
        for (i, w) in adjustment.seasonal_load_delta.iter().enumerate() {
            let new_val = current.seasonal_load_management[i] + w;
            new_weights.seasonal_load_management[i] = new_val.max(self.config.weight_bounds.min);
        }

        // 约束2：权重比例合理性（归一化）
        let sum: f64 = new_weights.seasonal_load_management.iter().sum();
        if sum > 0.0 {
            let scale = self.config.constraints.sum_normalized / sum;
            for w in &mut new_weights.seasonal_load_management {
                *w *= scale;
            }
        }

        // 约束3：单次调整幅度限制
        for i in 0..8 {
            let diff = (new_weights.seasonal_load_management[i] - current.seasonal_load_management[i]).abs();
            let max_change = self.config.constraints.max_adjustment_per_update * current.seasonal_load_management[i];
            if diff > max_change {
                let sign = if new_weights.seasonal_load_management[i] > current.seasonal_load_management[i] {
                    1.0
                } else {
                    -1.0
                };
                new_weights.seasonal_load_management[i] =
                    current.seasonal_load_management[i] + sign * max_change;
            }
        }

        // 其他场景权重的简化处理
        for (i, w) in adjustment.commercial_arbitrage_delta.iter().enumerate() {
            let new_val = current.commercial_arbitrage[i] + w;
            new_weights.commercial_arbitrage[i] = new_val.max(self.config.weight_bounds.min);
        }
        for (i, w) in adjustment.demand_control_delta.iter().enumerate() {
            let new_val = current.demand_control[i] + w;
            new_weights.demand_control[i] = new_val.max(self.config.weight_bounds.min);
        }
        for (i, w) in adjustment.virtual_power_plant_delta.iter().enumerate() {
            let new_val = current.virtual_power_plant[i] + w;
            new_weights.virtual_power_plant[i] = new_val.max(self.config.weight_bounds.min);
        }
        for (i, w) in adjustment.ultra_green_delta.iter().enumerate() {
            let new_val = current.ultra_green[i] + w;
            new_weights.ultra_green[i] = new_val.max(self.config.weight_bounds.min);
        }

        Ok(new_weights)
    }

    /// 记录调整历史
    async fn record_adjustment(
        &self,
        _perf: &HistoricalPerformance,
        adjustment: &WeightAdjustment,
    ) {
        let mut history = self.adjustment_history.write().await;
        history.push(adjustment.clone());
        // 保留最近 100 条记录
        if history.len() > 100 {
            history.remove(0);
        }
    }

    /// 获取当前权重
    pub async fn get_current_weights(&self) -> SceneWeights {
        self.current_weights.read().await.clone()
    }

    /// 更新当前权重
    pub async fn update_weights(&self, new_weights: SceneWeights) {
        *self.current_weights.write().await = new_weights;
    }

    /// AWO-06: 验证奖励偏移（优化后奖励函数与原始策略无显著偏离）
    ///
    /// # 输入
    /// - original_reward: 原始策略奖励值
    /// - optimized_reward: 优化后策略奖励值
    ///
    /// # 输出
    /// - true: 偏移 < 5%，验证通过
    /// - false: 偏移 >= 5%，验证失败
    pub async fn validate_reward_drift(
        &self,
        original_reward: f64,
        optimized_reward: f64,
    ) -> bool {
        if original_reward.abs() < 1e-6 {
            // 原始奖励接近零时，使用绝对误差
            return (optimized_reward - original_reward).abs() < 0.05;
        }
        let drift = ((optimized_reward - original_reward) / original_reward).abs();
        drift < 0.05 // 偏移 < 5%
    }

    /// 获取调整历史
    pub async fn get_adjustment_history(&self) -> Vec<WeightAdjustment> {
        self.adjustment_history.read().await.clone()
    }

    /// 获取配置
    pub fn config(&self) -> &AdaptiveOptimizerConfig {
        &self.config
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 简单的性能收集器实现用于测试
    struct MockPerformanceCollector {
        historical: HistoricalPerformance,
    }

    impl MockPerformanceCollector {
        fn new(historical: HistoricalPerformance) -> Self {
            Self { historical }
        }
    }

    impl PerformanceCollector for MockPerformanceCollector {
        fn collect_historical(&self) -> Result<HistoricalPerformance, AiEngineError> {
            Ok(self.historical.clone())
        }

        fn collect_current(&self) -> Result<PerformanceFeatures, AiEngineError> {
            Ok(self.historical.features.clone())
        }
    }

    fn make_default_config() -> AdaptiveOptimizerConfig {
        AdaptiveOptimizerConfig::default()
    }

    fn make_default_weights() -> SceneWeights {
        SceneWeights::default()
    }

    fn make_historical_performance(
        pv_util: f64,
        battery_cycles: u32,
        trafo_load: f64,
        demand_violations: u32,
        cum_reward: f64,
    ) -> HistoricalPerformance {
        HistoricalPerformance {
            features: PerformanceFeatures {
                pv_utilization_rate: pv_util,
                battery_cycle_count: battery_cycles,
                trafo_avg_load: trafo_load,
                demand_violation_count: demand_violations,
                cumulative_reward: cum_reward,
            },
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    // ===== AWO-01: 配置加载测试 =====

    #[test]
    fn test_awo_01_config_load() {
        // AWO-01: 正确加载配置
        let config = AdaptiveOptimizerConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.update_interval_hours, 168);
        assert!((config.meta_learning_rate - 0.001).abs() < 1e-6);
        assert!((config.weight_bounds.min - 0.01).abs() < 1e-6);
        assert!((config.weight_bounds.max - 10.0).abs() < 1e-6);
        assert!((config.constraints.sum_normalized - 8.3).abs() < 1e-6);
        assert!((config.constraints.max_adjustment_per_update - 0.2).abs() < 1e-6);
    }

    #[test]
    fn test_awo_01_weight_bounds_default() {
        let bounds = crate::WeightBounds::default();
        assert!((bounds.min - 0.01).abs() < 1e-6);
        assert!((bounds.max - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_awo_01_weight_constraints_default() {
        let constraints = crate::WeightConstraints::default();
        assert!((constraints.sum_normalized - 8.3).abs() < 1e-6);
        assert!((constraints.max_adjustment_per_update - 0.2).abs() < 1e-6);
    }

    // ===== AWO-02: 元学习器输出权重调整测试 =====

    #[tokio::test]
    async fn test_awo_02_meta_learner_low_pv() {
        // AWO-02: 元学习器可基于历史性能数据输出权重调整
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.5, 0, 0.5, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical.clone()));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights, collector);
        let result = optimizer.optimize_weights(&historical).await;

        assert!(result.is_ok());
        let new_weights = result.unwrap();
        // 光伏消纳率低时，w1 应该增加
        assert!(new_weights.seasonal_load_management[0] >= 1.0);
    }

    #[tokio::test]
    async fn test_awo_02_meta_learner_high_demand_violation() {
        // AWO-02: 需量超标时调整 demand_control 权重
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.8, 0, 0.5, 5, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical.clone()));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights, collector);
        let result = optimizer.optimize_weights(&historical).await;

        assert!(result.is_ok());
        let new_weights = result.unwrap();
        // 需量超标时，demand_control[0] 应该增加
        assert!(new_weights.demand_control[0] >= 1.0);
    }

    // ===== AWO-04: 权重约束测试 =====

    #[tokio::test]
    async fn test_awo_04_weights_positive() {
        // AWO-04: 权重调整受物理约束约束（正数）
        let config = make_default_config();
        let weights = make_default_weights();
        // 创建一个让电池循环次数过多的性能数据，触发负权重调整
        let historical = make_historical_performance(0.8, 15, 0.5, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical.clone()));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights, collector);
        let result = optimizer.optimize_weights(&historical).await;

        assert!(result.is_ok());
        let new_weights = result.unwrap();
        // 所有权重必须为正
        for w in &new_weights.seasonal_load_management {
            assert!(*w > 0.0, "权重必须为正数");
        }
    }

    #[tokio::test]
    async fn test_awo_04_weights_normalized() {
        // AWO-04: 权重比例合理性（归一化和正确）
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.5, 0, 0.9, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical.clone()));

        let optimizer = AdaptiveWeightOptimizer::new(config.clone(), weights, collector);
        let result = optimizer.optimize_weights(&historical).await;

        assert!(result.is_ok());
        let new_weights = result.unwrap();
        let sum: f64 = new_weights.seasonal_load_management.iter().sum();
        // 归一化和应为 8.3
        assert!((sum - config.constraints.sum_normalized).abs() < 0.01,
            "权重和应为 {}，实际为 {}", config.constraints.sum_normalized, sum);
    }

    // ===== AWO-05: 调整幅度限制测试 =====

    #[tokio::test]
    async fn test_awo_05_adjustment_limit() {
        // AWO-05: 单次更新周期内权重变化不超过 max_adjustment_per_update
        let config = make_default_config();
        let weights = SceneWeights {
            seasonal_load_management: [1.0, 0.5, 2.0, 1.0, 0.5, 0.5, 0.3, 1.0],
            ..Default::default()
        };
        // 创建一个会触发大调整的性能数据
        let historical = make_historical_performance(0.3, 0, 0.95, 10, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical.clone()));

        let optimizer = AdaptiveWeightOptimizer::new(config.clone(), weights.clone(), collector);
        let result = optimizer.optimize_weights(&historical).await;

        assert!(result.is_ok());
        let new_weights = result.unwrap();

        // 检查调整幅度
        for i in 0..8 {
            let diff = (new_weights.seasonal_load_management[i] - weights.seasonal_load_management[i]).abs();
            let max_change = config.constraints.max_adjustment_per_update * weights.seasonal_load_management[i];
            assert!(diff <= max_change * 1.01, // 允许浮点误差
                "权重 {} 变化 {} 超过限制 {}",
                i, diff, max_change);
        }
    }

    // ===== AWO-06: 奖励偏移测试 =====

    #[tokio::test]
    async fn test_awo_06_reward_drift_within_5_percent() {
        // AWO-06: 优化后的奖励函数与原始策略无显著偏离（偏移 < 5%）
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.8, 0, 0.5, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights, collector);

        // 测试案例：原始奖励 100，优化后奖励 102（2% 偏移）
        assert!(optimizer.validate_reward_drift(100.0, 102.0).await);

        // 测试案例：原始奖励 100，优化后奖励 95（5% 偏移，边界）
        assert!(optimizer.validate_reward_drift(100.0, 95.0).await);
    }

    #[tokio::test]
    async fn test_awo_06_reward_drift_exceeds_5_percent() {
        // AWO-06: 偏移 >= 5% 时验证失败
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.8, 0, 0.5, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights, collector);

        // 测试案例：原始奖励 100，优化后奖励 110（10% 偏移）
        assert!(!optimizer.validate_reward_drift(100.0, 110.0).await);
    }

    #[tokio::test]
    async fn test_awo_06_reward_drift_near_zero() {
        // AWO-06: 原始奖励接近零时，使用绝对误差
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.8, 0, 0.5, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights, collector);

        // 原始奖励接近零，优化后奖励 0.04（绝对偏移 0.04 < 0.05）
        assert!(optimizer.validate_reward_drift(0.001, 0.04).await);
    }

    // ===== 其他功能测试 =====

    #[tokio::test]
    async fn test_get_current_weights() {
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.8, 0, 0.5, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights.clone(), collector);
        let current = optimizer.get_current_weights().await;

        assert_eq!(current.seasonal_load_management, weights.seasonal_load_management);
    }

    #[tokio::test]
    async fn test_update_weights() {
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.8, 0, 0.5, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights.clone(), collector);

        let new_weights = SceneWeights {
            seasonal_load_management: [2.0, 0.5, 2.0, 1.0, 0.5, 0.5, 0.3, 1.0],
            ..Default::default()
        };
        optimizer.update_weights(new_weights.clone()).await;

        let current = optimizer.get_current_weights().await;
        assert!((current.seasonal_load_management[0] - 2.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn test_adjustment_history_recorded() {
        let config = make_default_config();
        let weights = make_default_weights();
        let historical = make_historical_performance(0.5, 0, 0.5, 0, 100.0);
        let collector = Arc::new(MockPerformanceCollector::new(historical.clone()));

        let optimizer = AdaptiveWeightOptimizer::new(config, weights, collector);

        // 执行多次优化
        for _ in 0..3 {
            optimizer.optimize_weights(&historical).await.ok();
        }

        let history = optimizer.get_adjustment_history().await;
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_performance_features_clone() {
        let features = PerformanceFeatures {
            pv_utilization_rate: 0.8,
            battery_cycle_count: 5,
            trafo_avg_load: 0.6,
            demand_violation_count: 2,
            cumulative_reward: 100.0,
        };

        let cloned = features.clone();
        assert_eq!(cloned.pv_utilization_rate, 0.8);
        assert_eq!(cloned.battery_cycle_count, 5);
    }

    #[test]
    fn test_weight_adjustment_default() {
        let delta = WeightAdjustment::default();
        assert_eq!(delta.seasonal_load_delta, [0.0; 8]);
        assert_eq!(delta.commercial_arbitrage_delta, [0.0; 2]);
    }
}