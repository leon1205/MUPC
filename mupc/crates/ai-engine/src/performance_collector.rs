//! 性能指标收集器 (v2.11)
//!
//! 收集系统性能指标，用于元学习器训练和权重优化。
//!
//! # 数据来源
//! - 电池循环次数：从电池管理系统获取
//! - 光伏消纳率：计算公式
//! - 变压器负载率：从数据融合获取
//! - 需量超标次数：从计费系统获取
//! - 累积奖励：从奖励计算器获取

use crate::adaptive_weight_optimizer::{HistoricalPerformance, PerformanceFeatures};
use crate::error::AiEngineError;
use std::sync::{Arc, RwLock};

// ============================================================================
// PerformanceCollectorImpl
// ============================================================================

/// 性能指标收集器实现
pub struct PerformanceCollectorImpl {
    /// 缓存的最近性能数据
    cached_features: RwLock<Option<PerformanceFeatures>>,
    /// 缓存的历史性能数据
    cached_historical: RwLock<Option<HistoricalPerformance>>,
}

impl PerformanceCollectorImpl {
    /// 创建性能收集器
    pub fn new() -> Self {
        Self {
            cached_features: RwLock::new(None),
            cached_historical: RwLock::new(None),
        }
    }

    /// 从 storage 收集历史性能数据（简化实现）
    ///
    /// TODO: 实现从 storage 查询历史性能数据
    /// 目前返回默认数据，实际应从数据库查询
    fn collect_from_storage(&self) -> Result<HistoricalPerformance, AiEngineError> {
        // 检查缓存
        if let Some(cached) = self.cached_historical.read().unwrap().as_ref() {
            return Ok(cached.clone());
        }

        // 默认数据（实际应从 storage 查询）
        let historical = HistoricalPerformance {
            features: PerformanceFeatures {
                pv_utilization_rate: 0.8,
                battery_cycle_count: 0,
                trafo_avg_load: 0.5,
                demand_violation_count: 0,
                cumulative_reward: 0.0,
            },
            timestamp: chrono::Utc::now().timestamp(),
        };

        // 更新缓存
        {
            let mut cache = self.cached_historical.write().unwrap();
            *cache = Some(historical.clone());
        }

        Ok(historical)
    }

    /// 收集当前性能快照
    fn collect_current_internal(&self) -> Result<PerformanceFeatures, AiEngineError> {
        // 检查缓存
        if let Some(cached) = self.cached_features.read().unwrap().as_ref() {
            return Ok(cached.clone());
        }

        // 默认数据（实际应从各个子系统收集）
        let features = PerformanceFeatures {
            pv_utilization_rate: 0.8,
            battery_cycle_count: 0,
            trafo_avg_load: 0.5,
            demand_violation_count: 0,
            cumulative_reward: 0.0,
        };

        // 更新缓存
        {
            let mut cache = self.cached_features.write().unwrap();
            *cache = Some(features.clone());
        }

        Ok(features)
    }

    /// 更新缓存的性能数据（用于测试或模拟）
    pub fn update_cached_features(&self, features: PerformanceFeatures) {
        let mut cache = self.cached_features.write().unwrap();
        *cache = Some(features);
    }

    /// 更新缓存的历史性能数据（用于测试或模拟）
    pub fn update_cached_historical(&self, historical: HistoricalPerformance) {
        let mut cache = self.cached_historical.write().unwrap();
        *cache = Some(historical);
    }
}

impl Default for PerformanceCollectorImpl {
    fn default() -> Self {
        Self::new()
    }
}

// 实现 PerformanceCollector trait
impl crate::adaptive_weight_optimizer::PerformanceCollector for PerformanceCollectorImpl {
    fn collect_historical(&self) -> Result<HistoricalPerformance, AiEngineError> {
        self.collect_from_storage()
    }

    fn collect_current(&self) -> Result<PerformanceFeatures, AiEngineError> {
        self.collect_current_internal()
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive_weight_optimizer::PerformanceCollector;

    fn make_features(
        pv: f64,
        battery: u32,
        trafo: f64,
        demand: u32,
        reward: f64,
    ) -> PerformanceFeatures {
        PerformanceFeatures {
            pv_utilization_rate: pv,
            battery_cycle_count: battery,
            trafo_avg_load: trafo,
            demand_violation_count: demand,
            cumulative_reward: reward,
        }
    }

    fn make_historical(features: PerformanceFeatures) -> HistoricalPerformance {
        HistoricalPerformance {
            features,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    #[test]
    fn test_collector_new() {
        let collector = PerformanceCollectorImpl::new();
        assert!(collector.collect_historical().is_ok());
        assert!(collector.collect_current().is_ok());
    }

    #[test]
    fn test_collector_default_features() {
        let collector = PerformanceCollectorImpl::new();
        let features = collector.collect_current().unwrap();

        assert!((features.pv_utilization_rate - 0.8).abs() < 1e-6);
        assert_eq!(features.battery_cycle_count, 0);
        assert!((features.trafo_avg_load - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_collector_historical_default() {
        let collector = PerformanceCollectorImpl::new();
        let historical = collector.collect_historical().unwrap();

        assert!((historical.features.pv_utilization_rate - 0.8).abs() < 1e-6);
        assert_eq!(historical.timestamp > 0, true);
    }

    #[test]
    fn test_update_cached_features() {
        let collector = PerformanceCollectorImpl::new();

        let custom_features = make_features(0.6, 5, 0.7, 3, 150.0);
        collector.update_cached_features(custom_features.clone());

        let features = collector.collect_current().unwrap();
        assert!((features.pv_utilization_rate - 0.6).abs() < 1e-6);
        assert_eq!(features.battery_cycle_count, 5);
        assert!((features.trafo_avg_load - 0.7).abs() < 1e-6);
        assert_eq!(features.demand_violation_count, 3);
        assert!((features.cumulative_reward - 150.0).abs() < 1e-6);
    }

    #[test]
    fn test_update_cached_historical() {
        let collector = PerformanceCollectorImpl::new();

        let custom_features = make_features(0.9, 10, 0.4, 0, 200.0);
        let custom_historical = make_historical(custom_features);
        collector.update_cached_historical(custom_historical.clone());

        let historical = collector.collect_historical().unwrap();
        assert!((historical.features.pv_utilization_rate - 0.9).abs() < 1e-6);
        assert_eq!(historical.features.battery_cycle_count, 10);
    }

    #[test]
    fn test_historical_performance_clone() {
        let features = make_features(0.7, 3, 0.6, 1, 100.0);
        let historical = make_historical(features);

        let cloned = historical.clone();
        assert_eq!(
            cloned.features.pv_utilization_rate,
            historical.features.pv_utilization_rate
        );
        assert_eq!(cloned.timestamp, historical.timestamp);
    }

    #[test]
    fn test_performance_features_clone() {
        let features = make_features(0.7, 3, 0.6, 1, 100.0);

        let cloned = features.clone();
        assert_eq!(cloned.pv_utilization_rate, features.pv_utilization_rate);
        assert_eq!(cloned.battery_cycle_count, features.battery_cycle_count);
        assert_eq!(cloned.trafo_avg_load, features.trafo_avg_load);
        assert_eq!(
            cloned.demand_violation_count,
            features.demand_violation_count
        );
        assert_eq!(cloned.cumulative_reward, features.cumulative_reward);
    }

    #[test]
    fn test_collector_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PerformanceCollectorImpl>();
    }

    #[test]
    fn test_collector_trait_object() {
        // 测试 trait 对象是否满足要求
        let collector: Arc<dyn PerformanceCollector> = Arc::new(PerformanceCollectorImpl::new());

        let result = collector.collect_current();
        assert!(result.is_ok());

        let result = collector.collect_historical();
        assert!(result.is_ok());
    }
}
