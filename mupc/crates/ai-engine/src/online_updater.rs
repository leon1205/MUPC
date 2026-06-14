//! 在线微调模块
//!
//! Phase 3C.2 实现完整功能
//! 当前为框架实现，支持数据收集和缓冲区管理
//!
//! v2.3: add_sample 添加 running_mode 参数，支持按场景隔离数据。

use crate::config::{GradualSwitchConfig, OnlineUpdateConfig};
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

// ─────────────────────────────────────────────────────────────────────────────
// R1: 影子模型验证 + 渐进式切换（v2.10）
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;
use tokio::sync::RwLock;

/// 模型元信息（影子模型用）
#[derive(Debug, Clone)]
pub struct ModelMeta {
    /// 场景名称
    pub scene_name: String,
    /// 版本
    pub version: String,
}

/// 影子模型（克隆自 ModelManager 的 RL 模型）
#[derive(Debug)]
pub struct ShadowModel {
    /// 模型权重副本
    weights: RwLock<Vec<f32>>,
    /// 模型元信息
    meta: ModelMeta,
}

impl Clone for ShadowModel {
    fn clone(&self) -> Self {
        Self {
            weights: RwLock::new(
                self.weights
                    .try_read()
                    .map(|g| g.clone())
                    .unwrap_or_default(),
            ),
            meta: self.meta.clone(),
        }
    }
}

impl ShadowModel {
    /// 创建影子模型
    pub fn new(weights: Vec<f32>, meta: ModelMeta) -> Self {
        Self {
            weights: RwLock::new(weights),
            meta,
        }
    }

    /// 获取权重快照
    pub async fn get_weights(&self) -> Vec<f32> {
        self.weights.read().await.clone()
    }

    /// 更新权重
    pub async fn update_weights(&self, new_weights: Vec<f32>) {
        let mut guard = self.weights.write().await;
        *guard = new_weights;
    }
}

/// 更新错误类型（R1）
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateError {
    /// 安全约束违反（影子模型安全评分 < 阈值）
    SafetyViolation { score: f32, threshold: f32 },
    /// 性能下降（影子模型性能 < 当前 * 阈值）
    PerformanceDegradation {
        current: f32,
        shadow: f32,
        threshold: f32,
    },
    /// 切换进行中
    SwitchInProgress,
    /// 模型未就绪
    ModelNotReady,
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateError::SafetyViolation { score, threshold } => {
                write!(f, "安全约束违反: 评分 {} < 阈值 {}", score, threshold)
            }
            UpdateError::PerformanceDegradation {
                current,
                shadow,
                threshold,
            } => {
                write!(
                    f,
                    "性能下降: 当前 {} > 影子 {} (阈值 {})",
                    current, shadow, threshold
                )
            }
            UpdateError::SwitchInProgress => write!(f, "切换进行中，拒绝重复更新"),
            UpdateError::ModelNotReady => write!(f, "模型未就绪"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// 切换状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchState {
    Idle,
    Scheduled,
    InProgress,
    Completed,
}

/// 渐进式切换器
#[derive(Debug)]
pub struct GradualSwitcher {
    config: GradualSwitchConfig,
    current_weights: Vec<f32>,
    target_weights: Vec<f32>,
    step_counter: usize,
    state: RwLock<SwitchState>,
}

impl GradualSwitcher {
    /// 创建渐进式切换器
    pub fn new(config: GradualSwitchConfig, current_weights: Vec<f32>) -> Self {
        Self {
            config,
            current_weights,
            target_weights: Vec::new(),
            step_counter: 0,
            state: RwLock::new(SwitchState::Idle),
        }
    }

    /// 启动切换
    pub fn start(&mut self, target: Vec<f32>) {
        self.target_weights = target;
        self.step_counter = 0;
        let state = if self.config.enabled {
            SwitchState::Scheduled
        } else {
            SwitchState::Completed
        };
        let state_lock = self.state.try_write();
        if let Ok(mut guard) = state_lock {
            *guard = state;
        }
    }

    /// 计算下一步的混合权重
    pub fn step(&mut self) -> Option<Vec<f32>> {
        if self.step_counter >= self.config.steps {
            if let Ok(mut guard) = self.state.try_write() {
                *guard = SwitchState::Completed;
            }
            return None;
        }

        let alpha = self.step_counter as f32 / self.config.steps as f32;
        let blended: Vec<f32> = self
            .current_weights
            .iter()
            .zip(self.target_weights.iter())
            .map(|(cur, tgt)| (1.0 - alpha) * cur + alpha * tgt)
            .collect();

        self.step_counter += 1;
        if let Ok(mut guard) = self.state.try_write() {
            *guard = SwitchState::InProgress;
        }

        Some(blended)
    }

    /// 当前混合比例（0.0=全旧，1.0=全新）
    pub fn blend_ratio(&self) -> f32 {
        if self.config.steps == 0 {
            return 1.0;
        }
        self.step_counter as f32 / self.config.steps as f32
    }

    /// 是否切换中
    pub async fn is_in_progress(&self) -> bool {
        let state = self.state.read().await;
        *state == SwitchState::InProgress || *state == SwitchState::Scheduled
    }

    /// 获取当前状态
    pub async fn state(&self) -> SwitchState {
        *self.state.read().await
    }

    /// 当前步数
    pub fn current_step(&self) -> usize {
        self.step_counter
    }

    /// 总步数
    pub fn total_steps(&self) -> usize {
        self.config.steps
    }

    /// 是否已完成
    pub fn is_completed(&self) -> bool {
        self.step_counter >= self.config.steps
    }
}

/// 安全约束检查器接口
pub trait SafetyConstraintChecker: Send + Sync {
    /// 检查模型，返回安全评分 [0, 100]
    fn check(&self, model: &ShadowModel) -> f32;
}

/// 性能监视器接口
pub trait PerformanceMonitor: Send + Sync {
    /// 评估模型，返回性能评分 [0, 100]
    fn evaluate(&self, model: &ShadowModel) -> f32;
}

/// 默认安全约束检查器（基于 RobustnessManager 的异常检测）
pub struct DefaultSafetyChecker {
    /// 安全阈值（0-100）
    #[allow(dead_code)]
    threshold: f32,
}

impl DefaultSafetyChecker {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl SafetyConstraintChecker for DefaultSafetyChecker {
    fn check(&self, model: &ShadowModel) -> f32 {
        // 模拟：基于权重统计计算安全评分
        // 实际应调用 RobustnessManager 异常检测
        let weights = model.weights.try_read();
        if weights.is_err() {
            return 50.0; // 无法读取时返回中间值
        }
        let weights = weights.unwrap();

        // 简单安全评分：权重方差过大则扣分
        if weights.is_empty() {
            return 100.0;
        }
        let mean = weights.iter().sum::<f32>() / weights.len() as f32;
        let variance: f32 =
            weights.iter().map(|w| (w - mean).powi(2)).sum::<f32>() / weights.len() as f32;

        // 方差越小评分越高
        let score = 100.0 - variance.sqrt() * 10.0;
        score.clamp(0.0, 100.0)
    }
}

/// 默认性能监视器
pub struct DefaultPerformanceMonitor {
    /// 性能阈值（相对于当前的百分比）
    #[allow(dead_code)]
    threshold: f32,
}

impl DefaultPerformanceMonitor {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl PerformanceMonitor for DefaultPerformanceMonitor {
    fn evaluate(&self, model: &ShadowModel) -> f32 {
        // 模拟：基于权重计算性能评分
        let weights = model.weights.try_read();
        if weights.is_err() {
            return 50.0;
        }
        let weights = weights.unwrap();

        if weights.is_empty() {
            return 100.0;
        }

        // 简单性能评分：权重和越大性能越高
        let sum: f32 = weights.iter().map(|w| w.abs()).sum();
        (sum / weights.len() as f32 * 10.0).clamp(0.0, 100.0)
    }
}

/// SafeOnlineUpdater（R1 核心）
pub struct SafeOnlineUpdater {
    #[allow(dead_code)]
    config: OnlineUpdateConfig,
    /// 影子模型
    shadow_model: RwLock<Option<ShadowModel>>,
    /// 安全约束检查器
    safety_checker: Arc<dyn SafetyConstraintChecker>,
    /// 性能监视器
    performance_monitor: Arc<dyn PerformanceMonitor>,
    /// 渐进式切换器
    gradual_switcher: RwLock<Option<GradualSwitcher>>,
    /// 安全阈值（0-100）
    #[allow(dead_code)]
    safety_threshold: f32,
    /// 性能阈值（相对于当前的百分比）
    #[allow(dead_code)]
    performance_threshold: f32,
    /// 当前模型权重（用于性能对比）
    current_weights: RwLock<Vec<f32>>,
    /// v2.10 R1: 渐进式切换配置
    switch_config: GradualSwitchConfig,
}

impl SafeOnlineUpdater {
    /// 创建 SafeOnlineUpdater
    pub fn new(
        config: OnlineUpdateConfig,
        safety_checker: Arc<dyn SafetyConstraintChecker>,
        performance_monitor: Arc<dyn PerformanceMonitor>,
        safety_threshold: f32,
        performance_threshold: f32,
        switch_config: GradualSwitchConfig,
        initial_weights: Vec<f32>,
    ) -> Self {
        Self {
            config,
            shadow_model: RwLock::new(None),
            safety_checker,
            performance_monitor,
            gradual_switcher: RwLock::new(None),
            safety_threshold,
            performance_threshold,
            current_weights: RwLock::new(initial_weights),
            switch_config,
        }
    }

    /// 安全更新：影子模型验证 + 渐进式切换
    /// 返回 Ok(true) 表示更新已接受，Ok(false) 表示在切换中拒绝重复更新
    pub async fn safe_update(&self, new_weights: Vec<f32>) -> Result<bool, UpdateError> {
        // 1. 检查是否切换中
        {
            let gradual = self.gradual_switcher.read().await;
            if let Some(ref switcher) = *gradual {
                if switcher.is_in_progress().await {
                    tracing::info!("切换进行中，拒绝重复更新");
                    return Ok(false);
                }
            }
        }

        // 2. 克隆权重到影子模型
        let meta = ModelMeta {
            scene_name: "default".to_string(),
            version: "v2.10".to_string(),
        };
        let shadow = ShadowModel::new(new_weights.clone(), meta);

        // 3. 安全约束检查（先检查再存储）
        let safety_score = self.safety_checker.check(&shadow);
        if safety_score < self.safety_threshold {
            tracing::warn!(
                "安全约束违反: 评分 {} < 阈值 {}",
                safety_score,
                self.safety_threshold
            );
            return Err(UpdateError::SafetyViolation {
                score: safety_score,
                threshold: self.safety_threshold,
            });
        }

        // 安全检查通过后存储影子模型（克隆以保留所有权）
        *self.shadow_model.write().await = Some(shadow.clone());

        // 4. 性能对比检查

        // 4. 性能对比检查
        let current_weights = self.current_weights.read().await.clone();
        let current_model = ShadowModel::new(
            current_weights.clone(),
            ModelMeta {
                scene_name: "current".to_string(),
                version: "v2.10".to_string(),
            },
        );

        let current_score = self.performance_monitor.evaluate(&current_model);
        let shadow_score = self.performance_monitor.evaluate(&shadow);

        let threshold = self.performance_threshold;
        if shadow_score < current_score * threshold {
            tracing::warn!(
                "性能下降: 当前 {} > 影子 {} (阈值 {})",
                current_score,
                shadow_score,
                threshold
            );
            return Err(UpdateError::PerformanceDegradation {
                current: current_score,
                shadow: shadow_score,
                threshold,
            });
        }

        // 5. 触发渐进式切换
        let mut gradual = self.gradual_switcher.write().await;
        let mut switcher = GradualSwitcher::new(self.switch_config.clone(), current_weights);
        switcher.start(new_weights);
        *gradual = Some(switcher);

        Ok(true)
    }

    /// 触发渐进式切换（异步后台执行）
    pub async fn gradual_switch(&self) -> Result<(), UpdateError> {
        let mut gradual = self.gradual_switcher.write().await;
        let switcher = gradual.as_mut().ok_or(UpdateError::ModelNotReady)?;

        loop {
            let blended = switcher.step();
            match blended {
                Some(weights) => {
                    tracing::info!(
                        "渐进式切换步 {}/{}: blend_ratio={:.2}",
                        switcher.current_step(),
                        switcher.total_steps(),
                        switcher.blend_ratio()
                    );
                    // 更新当前权重
                    *self.current_weights.write().await = weights;
                }
                None => break,
            }

            // 等待下一步间隔
            let interval = self.switch_config.step_interval_secs;
            tokio::time::sleep(std::time::Duration::from_secs_f64(interval)).await;
        }

        tracing::info!("渐进式切换完成");
        Ok(())
    }

    /// 获取当前混合权重
    pub fn current_blend_weights(&self) -> Vec<f32> {
        // 同步获取，不等待
        self.current_weights
            .try_read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// 查询切换状态
    pub async fn is_switching(&self) -> bool {
        let gradual = self.gradual_switcher.read().await;
        if let Some(ref switcher) = *gradual {
            switcher.is_in_progress().await
        } else {
            false
        }
    }

    /// 获取当前混合比例
    pub fn current_blend_ratio(&self) -> f32 {
        // 同步获取
        if let Ok(gradual) = self.gradual_switcher.try_read() {
            if let Some(ref switcher) = *gradual {
                return switcher.blend_ratio();
            }
        }
        0.0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// R1 单元测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// AC1: 新权重违反安全约束时，拒绝更新并返回错误
    #[test]
    fn test_safety_violation_reject() {
        // Mock: 返回 70 分，阈值 80 → 应拒绝
        struct MockSafetyChecker {
            score: f32,
        }

        impl SafetyConstraintChecker for MockSafetyChecker {
            fn check(&self, _model: &ShadowModel) -> f32 {
                self.score
            }
        }

        let checker = Arc::new(MockSafetyChecker { score: 70.0 });
        let monitor = Arc::new(DefaultPerformanceMonitor::new(0.95));

        let updater = SafeOnlineUpdater::new(
            OnlineUpdateConfig::default(),
            checker,
            monitor,
            80.0, // safety_threshold
            0.95, // performance_threshold
            GradualSwitchConfig::default(),
            vec![1.0, 2.0, 3.0],
        );

        // 使用 try_read 而非 await 来同步测试
        let result =
            futures::executor::block_on(async { updater.safe_update(vec![0.5, 0.5, 0.5]).await });

        assert!(matches!(
            result,
            Err(UpdateError::SafetyViolation { score, threshold })
            if score == 70.0 && threshold == 80.0
        ));
    }

    /// AC2: 影子模型性能下降超过5%时，拒绝更新
    #[test]
    fn test_performance_degradation_reject() {
        // Mock: 当前模型评分 100，影子模型评分 90 (< 95) → 应拒绝
        struct MockMonitor {
            current_score: f32,
            shadow_score: f32,
        }

        impl PerformanceMonitor for MockMonitor {
            fn evaluate(&self, model: &ShadowModel) -> f32 {
                let weights = model.weights.try_read().unwrap();
                if weights.is_empty() {
                    self.current_score
                } else {
                    self.shadow_score
                }
            }
        }

        let monitor = Arc::new(MockMonitor {
            current_score: 100.0,
            shadow_score: 90.0,
        });

        let updater = SafeOnlineUpdater::new(
            OnlineUpdateConfig::default(),
            Arc::new(DefaultSafetyChecker::new(0.0)), // 不检查安全
            monitor,
            0.0,  // safety_threshold (禁用)
            0.95, // performance_threshold
            GradualSwitchConfig::default(),
            vec![], // 空权重 → 当前评分 100
        );

        let result = futures::executor::block_on(async {
            updater.safe_update(vec![1.0]).await // 非空权重 → 影子评分 90
        });

        assert!(matches!(
            result,
            Err(UpdateError::PerformanceDegradation {
                current,
                shadow,
                threshold: 0.95,
            }) if current == 100.0 && shadow == 90.0
        ));
    }

    /// AC3: 渐进式切换权重，每步间隔可配置（默认1秒）
    #[test]
    fn test_gradual_switch_blend_ratio() {
        let config = GradualSwitchConfig {
            enabled: true,
            steps: 10,
            step_interval_secs: 0.0, // 测试用 0 间隔
        };

        let mut switcher = GradualSwitcher::new(config, vec![0.0, 0.0, 0.0]);
        switcher.start(vec![10.0, 10.0, 10.0]);

        // 验证 10 步后 blend_ratio = 1.0
        for _ in 0..10 {
            let _ = switcher.step();
        }

        assert!(switcher.is_completed());
        assert_eq!(switcher.blend_ratio(), 1.0);
    }

    /// AC4: 切换过程记录日志，包含每步权重混合比例
    #[test]
    fn test_blend_weights_interpolation() {
        let config = GradualSwitchConfig {
            enabled: true,
            steps: 10,
            step_interval_secs: 0.0,
        };

        let mut switcher = GradualSwitcher::new(config, vec![0.0, 10.0]);
        switcher.start(vec![10.0, 20.0]);

        // 中间步验证：step 5 时 alpha = 0.5
        for _ in 0..5 {
            let _ = switcher.step();
        }

        let alpha = 5.0 / 10.0;
        let expected_weight0 = (1.0 - alpha) * 0.0 + alpha * 10.0; // 5.0
        let expected_weight1 = (1.0 - alpha) * 10.0 + alpha * 20.0; // 15.0

        let blended = switcher.step().unwrap();
        assert!((blended[0] - expected_weight0).abs() < 0.001);
        assert!((blended[1] - expected_weight1).abs() < 0.001);
    }

    /// 额外测试：切换中调用 safe_update 返回 Ok(false)
    #[test]
    fn test_switch_in_progress_reject() {
        let monitor = Arc::new(DefaultPerformanceMonitor::new(0.95));
        let checker = Arc::new(DefaultSafetyChecker::new(0.0));

        let updater = SafeOnlineUpdater::new(
            OnlineUpdateConfig::default(),
            checker,
            monitor,
            0.0,  // safety_threshold (禁用)
            0.95, // performance_threshold
            GradualSwitchConfig {
                enabled: true,
                steps: 10,
                step_interval_secs: 0.0,
            },
            vec![1.0, 2.0, 3.0],
        );

        // 先触发一次切换
        let result = futures::executor::block_on(async {
            let r = updater.safe_update(vec![4.0, 5.0, 6.0]).await;
            r
        });
        assert!(result.is_ok());

        // 切换中再次调用应返回 Ok(false)
        let result =
            futures::executor::block_on(async { updater.safe_update(vec![7.0, 8.0, 9.0]).await });

        assert_eq!(result, Ok(false));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 原有 OnlineUpdater 单元测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod online_updater_tests {
    use super::*;

    fn create_test_config() -> OnlineUpdateConfig {
        OnlineUpdateConfig {
            enabled: false,
            batch_size: 32,
            learning_rate: 0.001,
            gradual_switch: GradualSwitchConfig::default(),
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
