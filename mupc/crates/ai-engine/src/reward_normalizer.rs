//! 奖励函数动态自适应归一化模块 (v2.13)
//!
//! 使用滑动统计量（RunningStats）对奖励子项进行自适应归一化，
//! 消除硬编码系数，增强跨台区泛化能力。
//!
//! 归一化公式：z = (r - μ) / (σ + ε)，然后 clip(z, -1, 1)
//!
//! 每个奖励子项维护独立的 RunningStats

use std::collections::HashMap;
use std::sync::RwLock;

/// 滑动统计量（Welford 在线算法）
///
/// 用于计算滑动均值和标准差，支撑自适应归一化。
///
/// # 数学原理
/// Welford 在线算法：
/// - count += 1
/// - delta = value - mean
/// - mean += delta / count
/// - delta2 = value - mean
/// - m2 += delta * delta2
/// - variance = m2 / (count - 1)
#[derive(Debug, Clone)]
pub struct RunningStats {
    /// 滑动均值 μ
    mean: f64,
    /// 用于计算方差的 M2（离差平方和）
    m2: f64,
    /// 样本计数
    count: usize,
}

impl RunningStats {
    /// 创建新的滑动统计量
    pub fn new() -> Self {
        Self {
            mean: 0.0,
            m2: 0.0,
            count: 0,
        }
    }

    /// 更新统计量（单样本）
    pub fn update(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;
    }

    /// 批量更新统计量
    pub fn update_batch(&mut self, values: &[f64]) {
        for &v in values {
            self.update(v);
        }
    }

    /// 计算标准差
    ///
    /// count < 2 时返回 1.0（避免除零）
    pub fn std(&self) -> f64 {
        if self.count < 2 {
            return 1.0;
        }
        (self.m2 / (self.count - 1) as f64).sqrt()
    }

    /// 获取样本计数
    pub fn count(&self) -> usize {
        self.count
    }

    /// 获取滑动均值
    pub fn mean(&self) -> f64 {
        self.mean
    }

    /// 重置统计量
    pub fn reset(&mut self) {
        self.mean = 0.0;
        self.m2 = 0.0;
        self.count = 0;
    }
}

impl Default for RunningStats {
    fn default() -> Self {
        Self::new()
    }
}

/// 归一化结果
#[derive(Debug, Clone)]
pub struct NormalizedReward {
    /// 归一化后的值
    pub value: f64,
    /// 原始均值
    pub raw_mean: f64,
    /// 原始标准差
    pub raw_std: f64,
}

/// 奖励函数归一化器
///
/// 维护 HashMap<String, RunningStats>，对每个奖励子项独立统计并归一化。
///
/// # 归一化公式
/// z = (r - μ) / (σ + ε)，然后 clip(z, -1, 1)
///
/// 其中 ε = 1e-6 用于防止除零
#[derive(Debug)]
pub struct RewardNormalizer {
    /// 各奖励子项的滑动统计量
    stats: RwLock<HashMap<String, RunningStats>>,
    /// 归一化 epsilon（防止除零）
    epsilon: f64,
}

impl RewardNormalizer {
    /// 创建新的奖励归一化器
    pub fn new() -> Self {
        Self {
            stats: RwLock::new(HashMap::new()),
            epsilon: 1e-6,
        }
    }

    /// 创建带 epsilon 的奖励归一化器
    pub fn with_epsilon(epsilon: f64) -> Self {
        Self {
            stats: RwLock::new(HashMap::new()),
            epsilon,
        }
    }

    /// 获取或创建指定子项的统计量
    fn get_or_create_stats(&self, key: &str) -> RunningStats {
        let stats = self.stats.read().unwrap();
        stats.get(key).cloned().unwrap_or_default()
    }

    /// 归一化单个奖励值
    ///
    /// # 参数
    /// - key: 奖励子项标识符
    /// - value: 原始奖励值
    ///
    /// # 返回
    /// 归一化后的值和统计信息
    pub fn normalize(&self, key: &str, value: f64) -> NormalizedReward {
        let mut stats = self.stats.write().unwrap();
        let entry = stats.entry(key.to_string()).or_default();

        let mean = entry.mean();
        let std = entry.std();

        // 更新统计量
        entry.update(value);

        // 计算归一化值
        let z = if std > self.epsilon {
            (value - mean) / (std + self.epsilon)
        } else {
            0.0
        };
        let clamped = z.clamp(-1.0, 1.0);

        tracing::debug!(
            "normalize {}: raw={:.4}, mean={:.4}, std={:.4}, z={:.4}, normalized={:.4}",
            key,
            value,
            mean,
            std,
            z,
            clamped
        );

        NormalizedReward {
            value: clamped,
            raw_mean: mean,
            raw_std: std,
        }
    }

    /// 批量归一化奖励值
    pub fn normalize_batch(&self, key: &str, values: &[f64]) -> Vec<NormalizedReward> {
        values.iter().map(|&v| self.normalize(key, v)).collect()
    }

    /// 获取指定子项的统计信息
    pub fn get_stats(&self, key: &str) -> Option<(f64, f64, usize)> {
        let stats = self.stats.read().unwrap();
        stats.get(key).map(|s| (s.mean(), s.std(), s.count()))
    }

    /// 检查是否已收集足够样本（count >= min_samples）
    pub fn is_ready(&self, key: &str, min_samples: usize) -> bool {
        let stats = self.stats.read().unwrap();
        stats
            .get(key)
            .map(|s| s.count() >= min_samples)
            .unwrap_or(false)
    }

    /// 重置指定子项的统计量
    pub fn reset(&self, key: &str) {
        let mut stats = self.stats.write().unwrap();
        if let Some(s) = stats.get_mut(key) {
            s.reset();
        }
    }

    /// 重置所有统计量
    pub fn reset_all(&self) {
        let mut stats = self.stats.write().unwrap();
        for s in stats.values_mut() {
            s.reset();
        }
    }

    /// 获取所有子项列表
    pub fn keys(&self) -> Vec<String> {
        let stats = self.stats.read().unwrap();
        stats.keys().cloned().collect()
    }
}

impl Default for RewardNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 单元测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_running_stats_basic() {
        let mut stats = RunningStats::new();
        stats.update(1.0);
        stats.update(2.0);
        stats.update(3.0);

        assert_eq!(stats.count(), 3);
        assert!((stats.mean() - 2.0).abs() < 1e-6);
        // 标准差：[(1-2)² + (2-2)² + (3-2)²] / 2 = [1 + 0 + 1] / 2 = 1
        assert!((stats.std() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_running_stats_single_sample() {
        // count < 2 时 std 应返回 1.0
        let mut stats = RunningStats::new();
        stats.update(5.0);

        assert_eq!(stats.count(), 1);
        assert!((stats.mean() - 5.0).abs() < 1e-6);
        assert!((stats.std() - 1.0).abs() < 1e-6); // 避免除零返回 1.0
    }

    #[test]
    fn test_running_stats_reset() {
        let mut stats = RunningStats::new();
        stats.update(10.0);
        stats.update(20.0);
        stats.reset();

        assert_eq!(stats.count(), 0);
        assert!((stats.mean() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_normalize_basic() {
        let normalizer = RewardNormalizer::new();

        // 第一次归一化
        let result = normalizer.normalize("test_pv", 100.0);
        assert!((result.value - 1.0).abs() < 1e-6); // 第一个样本会被 clamp 到边界

        // 添加更多样本后验证归一化
        let mut stats = RunningStats::new();
        stats.update_batch(&[80.0, 90.0, 100.0, 110.0, 120.0]);
        let mean = stats.mean();
        let std = stats.std();

        // 验证统计量
        assert!((mean - 100.0).abs() < 1e-6);
        assert!(std > 0.0);
    }

    #[test]
    fn test_normalize_clamp_to_range() {
        let normalizer = RewardNormalizer::new();

        // 连续添加相似值，验证 clamp 效果
        let result1 = normalizer.normalize("clamp_test", 50.0);
        let result2 = normalizer.normalize("clamp_test", 51.0);
        let result3 = normalizer.normalize("clamp_test", 49.0);

        // 归一化后的值应该在 [-1, 1] 范围内
        assert!(result1.value >= -1.0 && result1.value <= 1.0);
        assert!(result2.value >= -1.0 && result2.value <= 1.0);
        assert!(result3.value >= -1.0 && result3.value <= 1.0);
    }

    #[test]
    fn test_normalize_multiple_keys() {
        let normalizer = RewardNormalizer::new();

        normalizer.normalize("pv", 100.0);
        normalizer.normalize("battery", 50.0);
        normalizer.normalize("pv", 90.0);
        normalizer.normalize("battery", 60.0);

        let keys = normalizer.keys();
        assert!(keys.contains(&"pv".to_string()));
        assert!(keys.contains(&"battery".to_string()));
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_is_ready() {
        let normalizer = RewardNormalizer::new();

        // 初始状态
        assert!(!normalizer.is_ready("test", 5));

        // 添加 3 个样本
        normalizer.normalize("test", 10.0);
        normalizer.normalize("test", 20.0);
        normalizer.normalize("test", 30.0);

        assert!(!normalizer.is_ready("test", 5)); // 不到 5 个样本

        // 再添加 2 个样本
        normalizer.normalize("test", 40.0);
        normalizer.normalize("test", 50.0);

        assert!(normalizer.is_ready("test", 5)); // 达到 5 个样本
    }

    #[test]
    fn test_reset() {
        let normalizer = RewardNormalizer::new();

        normalizer.normalize("test", 100.0);
        normalizer.normalize("test", 200.0);

        assert!(normalizer.is_ready("test", 2));

        normalizer.reset("test");

        assert!(!normalizer.is_ready("test", 2)); // 已重置
    }

    #[test]
    fn test_reset_all() {
        let normalizer = RewardNormalizer::new();

        normalizer.normalize("key1", 100.0);
        normalizer.normalize("key2", 200.0);

        normalizer.reset_all();

        assert!(!normalizer.is_ready("key1", 1));
        assert!(!normalizer.is_ready("key2", 1));
    }

    #[test]
    fn test_normalized_reward_structure() {
        let normalizer = RewardNormalizer::new();

        let result = normalizer.normalize("test", 50.0);

        assert_eq!(result.value, 1.0); // 第一个样本 clamp 到边界
        assert!(result.raw_mean.is_finite());
        assert!(result.raw_std.is_finite());
    }

    #[test]
    fn test_batch_normalize() {
        let normalizer = RewardNormalizer::new();

        let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let results = normalizer.normalize_batch("batch_test", &values);

        assert_eq!(results.len(), 5);
        for r in results {
            assert!(r.value >= -1.0 && r.value <= 1.0);
        }
    }
}
