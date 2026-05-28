//! 在线微调模块
//!
//! Phase 3C.2 实现完整功能
//! 当前为框架实现，支持数据收集和缓冲区管理

use crate::config::OnlineUpdateConfig;
use crate::error::AiEngineError;

/// 增量数据点
#[derive(Debug, Clone)]
pub struct DataPoint {
    /// 时间戳（UTC 秒）
    pub timestamp: i64,
    /// 输入特征向量
    pub input: Vec<f32>,
    /// 输出（标签）向量
    pub output: Vec<f32>,
}

/// 在线微调器
///
/// 用于持续学习：收集增量数据，定期微调模型权重
///
/// Phase 3C.2 将实现：
/// - 增量梯度下降
/// - 经验回放缓冲区
/// - 模型权重更新
pub struct OnlineUpdater {
    config: OnlineUpdateConfig,
    buffer: Vec<DataPoint>,
}

impl OnlineUpdater {
    /// 创建在线微调器
    pub fn new(config: OnlineUpdateConfig) -> Self {
        Self {
            config,
            buffer: Vec::new(),
        }
    }

    /// 添加数据点
    ///
    /// 自动管理缓冲区大小，超过容量时移除最旧的数据
    pub fn add_sample(&mut self, data: DataPoint) {
        // 缓冲区容量：batch_size * 10
        let capacity = self.config.batch_size * 10;

        // 如果缓冲区已满，移除最旧的数据
        if self.buffer.len() >= capacity {
            self.buffer.remove(0);
        }

        self.buffer.push(data);
    }

    /// 执行微调
    ///
    /// Phase 3C.2 实现：使用收集的数据执行增量训练
    ///
    /// 当前实现：返回错误（功能未启用）
    pub fn update(&self) -> Result<(), AiEngineError> {
        if !self.config.enabled {
            return Err(AiEngineError::OnlineUpdateFailed(
                "在线微调未启用".to_string(),
            ));
        }

        // Phase 3C.2 实现
        Err(AiEngineError::OnlineUpdateFailed(
            "待 Phase 3C.2 实现".to_string(),
        ))
    }

    /// 获取缓冲区大小
    pub fn buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// 检查是否启用
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// 清空缓冲区
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// 获取配置
    pub fn config(&self) -> &OnlineUpdateConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> OnlineUpdateConfig {
        OnlineUpdateConfig {
            enabled: false,
            batch_size: 32,
            learning_rate: 0.001,
        }
    }

    #[test]
    fn test_online_updater_creation() {
        let config = create_test_config();
        let updater = OnlineUpdater::new(config);
        assert_eq!(updater.buffer_size(), 0);
        assert!(!updater.is_enabled());
    }

    #[test]
    fn test_online_updater_add_sample() {
        let config = create_test_config();
        let mut updater = OnlineUpdater::new(config);

        let data = DataPoint {
            timestamp: 1000,
            input: vec![1.0, 2.0, 3.0],
            output: vec![0.5],
        };

        updater.add_sample(data);
        assert_eq!(updater.buffer_size(), 1);
    }

    #[test]
    fn test_online_updater_disabled() {
        let config = create_test_config();
        let updater = OnlineUpdater::new(config);
        let result = updater.update();
        assert!(result.is_err());
    }

    #[test]
    fn test_online_updater_buffer_overflow() {
        let mut config = create_test_config();
        config.batch_size = 2; // 容量 = 2 * 10 = 20

        let mut updater = OnlineUpdater::new(config);

        // 添加 25 个数据点
        for i in 0..25 {
            let data = DataPoint {
                timestamp: i as i64,
                input: vec![i as f32],
                output: vec![i as f32 * 0.1],
            };
            updater.add_sample(data);
        }

        // 缓冲区应保持 20 个数据点（移除最旧的 5 个）
        assert_eq!(updater.buffer_size(), 20);
    }
}
