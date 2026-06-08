//! 在线微调模块
//!
//! Phase 3C.2 实现完整功能
//! 当前为框架实现，支持数据收集和缓冲区管理
//!
//! v2.3: add_sample 添加 running_mode 参数，支持按场景隔离数据。

use crate::config::OnlineUpdateConfig;
use crate::error::AiEngineError;
use crate::mode_selector::RunningMode;

/// 增量数据点
#[derive(Debug, Clone)]
pub struct DataPoint {
    /// 时间戳（UTC 秒）
    pub timestamp: i64,
    /// 输入特征向量
    pub input: Vec<f32>,
    /// 输出（标签）向量
    pub output: Vec<f32>,
    /// v2.3: 所属运行场景
    pub scene: RunningMode,
}

impl DataPoint {
    /// 创建数据点（兼容旧接口，默认场景为农网灌溉）
    pub fn new(timestamp: i64, input: Vec<f32>, output: Vec<f32>) -> Self {
        Self {
            timestamp,
            input,
            output,
            scene: RunningMode::SeasonalLoadManagement,
        }
    }
}

/// 在线微调器
///
/// 用于持续学习：收集增量数据，定期微调模型权重
///
/// v2.3: 按场景隔离数据缓冲区，场景切换时自动保存/加载检查点
pub struct OnlineUpdater {
    config: OnlineUpdateConfig,
    /// 所有场景共享的数据缓冲区（v2.3: 按 scene 字段过滤）
    buffer: Vec<DataPoint>,
    /// 当前活跃的场景（v2.3 新增）
    active_scene: RunningMode,
    /// 各场景的检查点目录（v2.3 新增，Phase 3C.2 实现）
    #[allow(dead_code)]
    checkpoint_dir: Option<std::path::PathBuf>,
}

impl OnlineUpdater {
    /// 创建在线微调器
    pub fn new(config: OnlineUpdateConfig) -> Self {
        Self {
            config,
            buffer: Vec::new(),
            active_scene: RunningMode::SeasonalLoadManagement,
            checkpoint_dir: None,
        }
    }

    /// v2.3: 创建带检查点目录的微调器
    pub fn new_with_checkpoint_dir(
        config: OnlineUpdateConfig,
        checkpoint_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            config,
            buffer: Vec::new(),
            active_scene: RunningMode::SeasonalLoadManagement,
            checkpoint_dir: Some(checkpoint_dir),
        }
    }

    /// 设置当前活跃场景（v2.3 新增）
    pub fn set_active_scene(&mut self, scene: RunningMode) {
        if self.active_scene != scene {
            tracing::info!(
                "在线微调场景切换: {} → {}",
                self.active_scene.display_name(),
                scene.display_name()
            );
            self.active_scene = scene;
            // Phase 3C.2: 场景切换时保存旧检查点、加载新检查点
        }
    }

    /// 获取当前活跃场景（v2.3 新增）
    pub fn active_scene(&self) -> RunningMode {
        self.active_scene
    }

    /// 添加数据点（兼容旧接口，使用当前活跃场景）
    pub fn add_sample(&mut self, data: DataPoint) {
        let capacity = self.config.batch_size * 10;
        if self.buffer.len() >= capacity {
            self.buffer.remove(0);
        }
        self.buffer.push(data);
    }

    /// v2.3: 添加数据点（显式指定场景）
    pub fn add_sample_for_scene(&mut self, scene: RunningMode, data: DataPoint) {
        let mut tagged = data;
        tagged.scene = scene;
        self.add_sample(tagged);
    }

    /// 获取指定场景的数据点数量（v2.3 新增）
    pub fn scene_sample_count(&self, scene: RunningMode) -> usize {
        self.buffer.iter().filter(|d| d.scene == scene).count()
    }

    /// 执行微调
    ///
    /// Phase 3C.2 实现：使用收集的数据执行增量训练
    ///
    /// v2.3: 仅微调当前活跃场景的模型
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

    /// 获取缓冲区大小（所有场景合计）
    pub fn buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// 检查是否启用
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// 清空指定场景的缓冲区（v2.3 新增）
    pub fn clear_scene_buffer(&mut self, scene: RunningMode) {
        self.buffer.retain(|d| d.scene != scene);
    }

    /// 清空全部缓冲区
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
        assert_eq!(updater.active_scene(), RunningMode::SeasonalLoadManagement);
    }

    #[test]
    fn test_online_updater_add_sample() {
        let config = create_test_config();
        let mut updater = OnlineUpdater::new(config);

        let data = DataPoint::new(1000, vec![1.0, 2.0, 3.0], vec![0.5]);

        updater.add_sample(data);
        assert_eq!(updater.buffer_size(), 1);
    }

    #[test]
    fn test_online_updater_scene_isolation() {
        let config = create_test_config();
        let mut updater = OnlineUpdater::new(config);

        updater.add_sample_for_scene(
            RunningMode::SeasonalLoadManagement,
            DataPoint::new(1, vec![1.0], vec![0.1]),
        );
        updater.add_sample_for_scene(
            RunningMode::CommercialArbitrage,
            DataPoint::new(2, vec![2.0], vec![0.2]),
        );
        updater.add_sample_for_scene(
            RunningMode::SeasonalLoadManagement,
            DataPoint::new(3, vec![3.0], vec![0.3]),
        );

        assert_eq!(
            updater.scene_sample_count(RunningMode::SeasonalLoadManagement),
            2
        );
        assert_eq!(
            updater.scene_sample_count(RunningMode::CommercialArbitrage),
            1
        );
        assert_eq!(updater.scene_sample_count(RunningMode::DemandControl), 0);
    }

    #[test]
    fn test_online_updater_set_active_scene() {
        let config = create_test_config();
        let mut updater = OnlineUpdater::new(config);

        updater.set_active_scene(RunningMode::VirtualPowerPlant);
        assert_eq!(updater.active_scene(), RunningMode::VirtualPowerPlant);
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
            let data = DataPoint::new(i as i64, vec![i as f32], vec![i as f32 * 0.1]);
            updater.add_sample(data);
        }

        // 缓冲区应保持 20 个数据点（移除最旧的 5 个）
        assert_eq!(updater.buffer_size(), 20);
    }

    #[test]
    fn test_clear_scene_buffer() {
        let config = create_test_config();
        let mut updater = OnlineUpdater::new(config);

        updater.add_sample_for_scene(
            RunningMode::SeasonalLoadManagement,
            DataPoint::new(1, vec![1.0], vec![0.1]),
        );
        updater.add_sample_for_scene(
            RunningMode::CommercialArbitrage,
            DataPoint::new(2, vec![2.0], vec![0.2]),
        );

        updater.clear_scene_buffer(RunningMode::SeasonalLoadManagement);
        assert_eq!(
            updater.scene_sample_count(RunningMode::SeasonalLoadManagement),
            0
        );
        assert_eq!(
            updater.scene_sample_count(RunningMode::CommercialArbitrage),
            1
        );
    }
}
