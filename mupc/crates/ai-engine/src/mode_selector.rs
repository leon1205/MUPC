//! 运行场景模式选择器
//!
//! 5 种预设运行场景，互斥选择，支持远程控制（IEC 104/61850）
//! 和本地选择（Web UI/配置文件）。同一时刻仅 1 种模式生效。
//!
//! v2.3: 场景切换联动 ModelRegistry 热切换 RL 模型。
//! v2.10 R3: 场景切换平滑过渡（Linear Interpolation）

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

use crate::config::SceneWeights;
use crate::error::AiEngineError;
use crate::model_registry::{ModelRegistry, SceneModelState, SceneSwitchResult};

/// 预设运行场景（互斥，无 Default 兜底）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum RunningMode {
    /// MODE-01：台区季节性负荷模式 — 夏季灌溉/炒茶/冬季空调等季节性负荷管理
    SeasonalLoadManagement = 1,
    /// MODE-02：自主套利模式 — 最大化峰谷电价差收益
    CommercialArbitrage = 2,
    /// MODE-03：需量控制模式 — 减免需量罚金
    DemandControl = 3,
    /// MODE-04：虚拟电厂模式 — 最大化辅助服务收益
    VirtualPowerPlant = 4,
    /// MODE-05：极致绿色模式 — 最大化绿电消纳
    UltraGreen = 5,
}

impl RunningMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::SeasonalLoadManagement),
            2 => Some(Self::CommercialArbitrage),
            3 => Some(Self::DemandControl),
            4 => Some(Self::VirtualPowerPlant),
            5 => Some(Self::UltraGreen),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::SeasonalLoadManagement => "台区季节性负荷模式",
            Self::CommercialArbitrage => "自主套利模式",
            Self::DemandControl => "需量控制模式",
            Self::VirtualPowerPlant => "虚拟电厂模式",
            Self::UltraGreen => "极致绿色模式",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::SeasonalLoadManagement => "最大化光伏消纳 + 防止变压器过载 + 电池寿命保护",
            Self::CommercialArbitrage => "最大化峰谷电价差收益 + 最小化电池损耗",
            Self::DemandControl => "减免需量罚金 + 最小化舒适度损失",
            Self::VirtualPowerPlant => "最大化辅助服务收益 + 响应精度",
            Self::UltraGreen => "最大化绿电消纳比例 + 最小化碳排放",
        }
    }

    pub fn all() -> &'static [RunningMode] {
        &[
            Self::SeasonalLoadManagement,
            Self::CommercialArbitrage,
            Self::DemandControl,
            Self::VirtualPowerPlant,
            Self::UltraGreen,
        ]
    }
}

impl std::fmt::Display for RunningMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// 切换来源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwitchSource {
    /// 调度主站远程切换（IEC 104 / IEC 61850）
    RemoteDispatch { protocol: String, address: String },
    /// 本地 Web UI 切换
    LocalWeb { username: String },
    /// 配置文件加载（系统启动时）
    LocalConfig,
}

impl std::fmt::Display for SwitchSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RemoteDispatch { protocol, .. } => write!(f, "远程调度({})", protocol),
            Self::LocalWeb { username } => write!(f, "本地Web({})", username),
            Self::LocalConfig => write!(f, "配置文件"),
        }
    }
}

/// 模式切换事件
#[derive(Debug, Clone, Serialize)]
pub struct ModeSwitchEvent {
    pub previous: RunningMode,
    pub current: RunningMode,
    pub source: SwitchSource,
    pub timestamp: i64,
}

// ============================================================================
// v2.10 R3: 场景切换平滑过渡
// ============================================================================

/// 平滑过渡配置
#[derive(Debug, Clone)]
pub struct TransitionConfig {
    /// 过渡步数（默认 10）
    pub transition_steps: usize,
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self {
            transition_steps: 10,
        }
    }
}

/// 平滑过渡状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionState {
    Idle,
    InProgress,
    Completed,
}

/// 权重更新事件（用于通知 ModelManager 权重更新）
#[derive(Debug, Clone)]
pub struct WeightUpdateEvent {
    pub blended_weights: Vec<f32>,
    pub step: usize,
    pub total_steps: usize,
}

/// 平滑过渡器
///
/// v2.10 R3 新增：场景切换时对权重进行线性插值，避免突变。
///
/// 数学定义：
/// ```text
/// alpha = step_counter / transition_steps
/// weight_i = (1 - alpha) * current_weight_i + alpha * target_weight_i
/// ```
pub struct SmoothSceneTransition {
    config: TransitionConfig,
    current_weights: Option<Vec<f32>>,
    target_weights: Option<Vec<f32>>,
    step_counter: usize,
    state: TransitionState,
}

impl SmoothSceneTransition {
    /// 创建平滑过渡器
    pub fn new(config: TransitionConfig) -> Self {
        Self {
            config,
            current_weights: None,
            target_weights: None,
            step_counter: 0,
            state: TransitionState::Idle,
        }
    }

    /// 创建带权重的平滑过渡器
    pub fn new_with_weights(config: TransitionConfig, current: Vec<f32>, target: Vec<f32>) -> Self {
        let state = if current == target {
            TransitionState::Completed
        } else {
            TransitionState::InProgress
        };
        Self {
            config,
            current_weights: Some(current),
            target_weights: Some(target),
            step_counter: 0,
            state,
        }
    }

    /// 场景切换时调用（设置起始和目标权重）
    pub fn on_scene_switch(&mut self, current: Vec<f32>, target: Vec<f32>) {
        self.current_weights = Some(current);
        self.target_weights = Some(target);
        self.step_counter = 0;
        self.state = TransitionState::InProgress;
    }

    /// 获取插值权重（每决策周期调用一次）
    ///
    /// 返回当前步的线性插值权重，同时步进计数器。
    /// 过渡完成后持续返回目标权重。
    pub fn get_interpolated_weights(&mut self) -> &[f32] {
        let Some(ref current) = self.current_weights else {
            unreachable!("current_weights must be set before calling get_interpolated_weights");
        };
        let Some(ref target) = self.target_weights else {
            unreachable!("target_weights must be set before calling get_interpolated_weights");
        };

        if self.state == TransitionState::Completed {
            return target;
        }

        let total_steps = self.config.transition_steps;
        let alpha = self.step_counter as f32 / total_steps as f32;

        // 预分配结果向量（避免每次分配）
        let min_len = current.len().min(target.len());
        let result: Vec<f32> = (0..min_len)
            .map(|i| {
                let c = current[i] as f32;
                let t = target[i] as f32;
                (1.0 - alpha) * c + alpha * t
            })
            .collect();

        // 更新状态
        self.step_counter += 1;
        if self.step_counter >= total_steps {
            self.state = TransitionState::Completed;
            // 返回目标权重
            return target;
        }

        // 临时存储结果（下次调用时覆盖）
        self.current_weights = Some(result);
        &self.current_weights.as_ref().unwrap()
    }

    /// 当前过渡状态
    pub fn state(&self) -> TransitionState {
        self.state
    }

    /// 剩余步数
    pub fn remaining_steps(&self) -> usize {
        match self.state {
            TransitionState::Idle => self.config.transition_steps,
            TransitionState::InProgress => self
                .config
                .transition_steps
                .saturating_sub(self.step_counter),
            TransitionState::Completed => 0,
        }
    }

    /// 当前步数
    pub fn current_step(&self) -> usize {
        self.step_counter
    }

    /// 总步数
    pub fn total_steps(&self) -> usize {
        self.config.transition_steps
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// v2.13: 双策略头机制（策略混合替代权重混合）
// ─────────────────────────────────────────────────────────────────────────────

use crate::rl_model::ActionOutput;

/// 双策略头状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DualStrategyState {
    /// 仅运行旧策略
    OldOnly,
    /// 过渡中（α 线性增长）
    Transitioning,
    /// 仅运行新策略
    NewOnly,
}

/// 双策略头管理器（v2.13 新增）
///
/// 在场景切换过渡期内同时运行旧策略和新策略，
/// 最终动作 = (1-α) * a_old + α * a_new
///
/// 公式：a_blended = (1 - α) * a_old + α * a_new
/// 其中 α = step_counter / transition_steps（从 0 线性增长到 1）
pub struct DualStrategyHead {
    /// 旧策略动作（上一场景的策略）
    old_action: Option<ActionOutput>,
    /// 新策略动作（目标场景的策略）
    new_action: Option<ActionOutput>,
    /// 当前混合比例 α
    alpha: f32,
    /// 过渡状态
    state: DualStrategyState,
    /// 过渡步数计数器
    step_counter: usize,
    /// 过渡总步数
    transition_steps: usize,
}

impl DualStrategyHead {
    /// 创建双策略头
    pub fn new(transition_steps: usize) -> Self {
        Self {
            old_action: None,
            new_action: None,
            alpha: 0.0,
            state: DualStrategyState::OldOnly,
            step_counter: 0,
            transition_steps,
        }
    }

    /// 设置旧策略动作
    pub fn set_old_action(&mut self, action: ActionOutput) {
        self.old_action = Some(action);
    }

    /// 设置新策略动作
    pub fn set_new_action(&mut self, action: ActionOutput) {
        self.new_action = Some(action);
    }

    /// 更新混合比例 α
    ///
    /// α 从 0 线性增长到 1
    fn update_alpha(&mut self) {
        if self.step_counter >= self.transition_steps {
            self.alpha = 1.0;
            self.state = DualStrategyState::NewOnly;
        } else {
            self.alpha = self.step_counter as f32 / self.transition_steps as f32;
            self.state = DualStrategyState::Transitioning;
        }
    }

    /// 步进计数器
    pub fn step(&mut self) {
        self.step_counter += 1;
        self.update_alpha();
    }

    /// 混合动作
    ///
    /// 公式：a_blended = (1 - α) * a_old + α * a_new
    pub fn blend_actions(&self) -> Option<ActionOutput> {
        let a_old = self.old_action.as_ref()?;
        let a_new = self.new_action.as_ref()?;

        let alpha = self.alpha as f64;
        let one_minus_alpha = 1.0 - alpha;

        Some(ActionOutput {
            p_ref: one_minus_alpha * a_old.p_ref + alpha * a_new.p_ref,
            k_droop: one_minus_alpha * a_old.k_droop + alpha * a_new.k_droop,
            load_shedding: one_minus_alpha * a_old.load_shedding + alpha * a_new.load_shedding,
            pv_limit: one_minus_alpha * a_old.pv_limit + alpha * a_new.pv_limit,
            confidence: (one_minus_alpha * a_old.confidence + alpha * a_new.confidence).min(1.0),
        })
    }

    /// 获取当前混合比例 α
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// 获取当前状态
    pub fn state(&self) -> DualStrategyState {
        self.state
    }

    /// 重置过渡状态
    pub fn reset(&mut self) {
        self.old_action = None;
        self.new_action = None;
        self.alpha = 0.0;
        self.step_counter = 0;
        self.state = DualStrategyState::OldOnly;
    }

    /// 是否已完成过渡
    pub fn is_completed(&self) -> bool {
        self.state == DualStrategyState::NewOnly
    }
}

impl Default for DualStrategyHead {
    fn default() -> Self {
        Self::new(10) // 默认 10 步过渡
    }
}

/// 模式选择器（线程安全，互斥保证）
pub struct ModeSelector {
    current_mode: Arc<Mutex<RunningMode>>,
    switch_tx: broadcast::Sender<ModeSwitchEvent>,
    persist_path: Option<PathBuf>,
    /// v2.3 新增：模型注册表引用（场景切换时联动热切换 RL 模型）
    registry: Option<Arc<ModelRegistry>>,
    /// v2.10 R3 新增：场景权重映射（用于平滑过渡插值）
    weights: Arc<SceneWeights>,
    /// v2.10 R3 新增：平滑过渡器
    smooth_transition: Option<SmoothSceneTransition>,
    /// v2.10 R3 新增：平滑过渡配置
    transition_config: TransitionConfig,
}

impl ModeSelector {
    pub fn new(initial: RunningMode, persist_path: Option<PathBuf>) -> Self {
        let (switch_tx, _) = broadcast::channel(64);
        Self {
            current_mode: Arc::new(Mutex::new(initial)),
            switch_tx,
            persist_path,
            registry: None,
            weights: Arc::new(SceneWeights::default()),
            smooth_transition: None,
            transition_config: TransitionConfig::default(),
        }
    }

    /// v2.3 新增：带 ModelRegistry 的构造函数
    pub fn new_with_registry(
        initial: RunningMode,
        persist_path: Option<PathBuf>,
        registry: Arc<ModelRegistry>,
    ) -> Self {
        let (switch_tx, _) = broadcast::channel(64);
        Self {
            current_mode: Arc::new(Mutex::new(initial)),
            switch_tx,
            persist_path,
            registry: Some(registry),
            weights: Arc::new(SceneWeights::default()),
            smooth_transition: None,
            transition_config: TransitionConfig::default(),
        }
    }

    /// 获取当前运行模式（非阻塞读）
    ///
    /// 使用 try_lock 实现零开销读取。写操作极低频（< 1次/分钟），
    /// 几乎无锁竞争。仅在恰好有写操作进行中时短暂自旋等待。
    pub fn current(&self) -> RunningMode {
        match self.current_mode.try_lock() {
            Ok(guard) => *guard,
            Err(_) => {
                let guard = tokio::task::block_in_place(|| self.current_mode.blocking_lock());
                *guard
            }
        }
    }

    /// 切换运行场景（原子操作，互斥保护）
    ///
    /// 幂等：如果 new_mode == current，不触发切换事件，直接返回 Ok。
    ///
    /// v2.3: 切换时联动 ModelRegistry 热切换 RL 模型。
    /// 若目标模型需下载（返回 Downloading），仍更新模式状态，
    /// 但在模型下载完成前 AI 决策使用旧模型。
    ///
    /// v2.10 R3: 触发平滑过渡，权重线性插值。
    pub async fn switch(
        &mut self,
        new_mode: RunningMode,
        source: SwitchSource,
    ) -> Result<RunningMode, AiEngineError> {
        let previous = {
            let mut current = self.current_mode.lock().await;
            if *current == new_mode {
                return Ok(*current);
            }
            let prev = *current;
            *current = new_mode;
            prev
        }; // current 锁在此处释放

        // v2.10 R3: 触发平滑过渡
        self.trigger_smooth_transition(previous, new_mode);

        // v2.3: 先尝试热切换模型，再切换模式状态
        if let Some(ref registry) = self.registry {
            match registry.switch_to(new_mode).await {
                Ok(SceneSwitchResult::Switched) => {
                    tracing::info!(
                        "场景模型已切换: {} → {}",
                        previous.display_name(),
                        new_mode.display_name()
                    );
                }
                Ok(SceneSwitchResult::Downloading) => {
                    tracing::warn!(
                        "目标场景模型需下载: {}，保持当前模型运行",
                        new_mode.display_name()
                    );
                    // 模式状态仍然切换，但模型保持旧的，待下载完成后重新加载
                }
                Err(e) => {
                    tracing::error!("场景模型切换失败: {}", e);
                    return Err(AiEngineError::ModeSwitchFailed(format!(
                        "模型热切换失败: {}",
                        e
                    )));
                }
            }
        }

        if let Some(ref path) = self.persist_path {
            if let Err(e) = self.persist_mode(new_mode, path).await {
                tracing::error!("模式持久化失败: {}", e);
            }
        }

        let event = ModeSwitchEvent {
            previous,
            current: new_mode,
            source,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let _ = self.switch_tx.send(event);

        tracing::info!(
            "运行场景切换: {} → {}",
            previous.display_name(),
            new_mode.display_name()
        );

        Ok(previous)
    }

    /// 订阅模式切换事件
    pub fn subscribe(&self) -> broadcast::Receiver<ModeSwitchEvent> {
        self.switch_tx.subscribe()
    }

    /// v2.3 新增：查询指定场景的模型状态
    pub fn model_state(&self, mode: RunningMode) -> SceneModelState {
        match &self.registry {
            Some(registry) => registry.model_state(mode),
            None => SceneModelState::NotLoaded,
        }
    }

    /// v2.3 新增：获取所有场景的模型状态列表
    pub fn all_model_states(&self) -> Vec<(RunningMode, SceneModelState)> {
        match &self.registry {
            Some(registry) => registry.all_model_states(),
            None => RunningMode::all()
                .iter()
                .map(|&m| (m, SceneModelState::NotLoaded))
                .collect(),
        }
    }

    /// v2.3 新增：设置 ModelRegistry 引用（初始化阶段调用）
    pub fn set_registry(&mut self, registry: Arc<ModelRegistry>) {
        self.registry = Some(registry);
    }

    /// v2.3 新增：获取 ModelRegistry 引用
    pub fn registry(&self) -> Option<&Arc<ModelRegistry>> {
        self.registry.as_ref()
    }

    /// v2.10 R3 新增：设置场景权重映射
    pub fn set_weights(&mut self, weights: Arc<SceneWeights>) {
        self.weights = weights;
    }

    /// v2.10 R3 新增：设置平滑过渡配置
    pub fn set_transition_config(&mut self, config: TransitionConfig) {
        self.transition_config = config;
    }

    /// v2.10 R3 新增：获取当前生效的权重（平滑过渡期间返回插值权重）
    pub fn current_weights(&mut self) -> Vec<f32> {
        if let Some(ref mut transition) = self.smooth_transition {
            if transition.state() == TransitionState::InProgress {
                return transition.get_interpolated_weights().to_vec();
            }
        }
        self.get_scene_weights_internal()
    }

    /// v2.10 R3 新增：获取平滑过渡状态
    pub fn transition_state(&self) -> Option<TransitionState> {
        self.smooth_transition.as_ref().map(|t| t.state())
    }

    /// v2.10 R3 新增：获取剩余过渡步数
    pub fn remaining_transition_steps(&self) -> usize {
        self.smooth_transition
            .as_ref()
            .map(|t| t.remaining_steps())
            .unwrap_or(0)
    }

    /// 内部方法：获取当前场景的权重向量
    fn get_scene_weights_internal(&self) -> Vec<f32> {
        let mode = self.current();
        self.weights_to_vec(&self.weights, mode)
    }

    /// 根据场景获取权重向量
    fn get_weights_for_mode(&self, mode: RunningMode) -> Vec<f32> {
        self.weights_to_vec(&self.weights, mode)
    }

    /// 将 SceneWeights 转换为 Vec<f32>（取较短长度以支持不同维度场景）
    fn weights_to_vec(&self, weights: &SceneWeights, mode: RunningMode) -> Vec<f32> {
        let arr: &[f64] = match mode {
            RunningMode::SeasonalLoadManagement => &weights.seasonal_load_management,
            RunningMode::CommercialArbitrage => &weights.commercial_arbitrage,
            RunningMode::DemandControl => &weights.demand_control,
            RunningMode::VirtualPowerPlant => &weights.virtual_power_plant,
            RunningMode::UltraGreen => &weights.ultra_green,
        };
        arr.iter().map(|&v| v as f32).collect()
    }

    /// 触发平滑过渡（内部方法，由 switch 调用）
    fn trigger_smooth_transition(&mut self, old_mode: RunningMode, new_mode: RunningMode) {
        let current_weights = self.get_weights_for_mode(old_mode);
        let target_weights = self.get_weights_for_mode(new_mode);

        let mut transition = SmoothSceneTransition::new(self.transition_config.clone());
        transition.on_scene_switch(current_weights, target_weights);
        self.smooth_transition = Some(transition);
    }

    async fn persist_mode(&self, mode: RunningMode, path: &PathBuf) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, format!("{}", mode as u8)).await
    }

    /// 从持久化文件恢复模式
    pub async fn restore_from_file(path: &std::path::Path) -> Result<RunningMode, AiEngineError> {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let v: u8 = content.trim().parse().unwrap_or(0);
                RunningMode::from_u8(v).ok_or_else(|| {
                    AiEngineError::ModeSwitchFailed(format!("持久化文件中无效模式值: {}", v))
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(RunningMode::SeasonalLoadManagement)
            }
            Err(e) => Err(AiEngineError::ModeSwitchFailed(format!(
                "读取持久化文件失败: {}",
                e
            ))),
        }
    }
}

/// 解析模式名称（支持多种格式）
pub fn parse_mode_name(s: &str) -> Option<RunningMode> {
    match s.to_lowercase().as_str() {
        // MODE-01: 兼容旧名，新增新名
        "seasonalloadmanagement"
        | "seasonal_load_management"
        | "mode-01"
        | "1"
        | "agriculturalirrigation"
        | "agricultural_irrigation" => Some(RunningMode::SeasonalLoadManagement),
        "commercialarbitrage" | "commercial_arbitrage" | "mode-02" | "2" => {
            Some(RunningMode::CommercialArbitrage)
        }
        "demandcontrol" | "demand_control" | "mode-03" | "3" => Some(RunningMode::DemandControl),
        "virtualpowerplant" | "virtual_power_plant" | "mode-04" | "4" => {
            Some(RunningMode::VirtualPowerPlant)
        }
        "ultragreen" | "ultra_green" | "mode-05" | "5" => Some(RunningMode::UltraGreen),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_running_mode_from_u8() {
        assert_eq!(
            RunningMode::from_u8(1),
            Some(RunningMode::SeasonalLoadManagement)
        );
        assert_eq!(RunningMode::from_u8(5), Some(RunningMode::UltraGreen));
        assert_eq!(RunningMode::from_u8(0), None);
        assert_eq!(RunningMode::from_u8(6), None);
    }

    #[test]
    fn test_running_mode_display_name() {
        assert_eq!(
            RunningMode::SeasonalLoadManagement.display_name(),
            "台区季节性负荷模式"
        );
        assert_eq!(
            RunningMode::CommercialArbitrage.display_name(),
            "自主套利模式"
        );
    }

    #[test]
    fn test_running_mode_all_count() {
        assert_eq!(RunningMode::all().len(), 5);
    }

    #[test]
    fn test_parse_mode_name() {
        assert_eq!(
            parse_mode_name("seasonalloadmanagement"),
            Some(RunningMode::SeasonalLoadManagement)
        );
        assert_eq!(
            parse_mode_name("1"),
            Some(RunningMode::SeasonalLoadManagement)
        );
        assert_eq!(
            parse_mode_name("mode-01"),
            Some(RunningMode::SeasonalLoadManagement)
        );
        // 兼容旧名
        assert_eq!(
            parse_mode_name("agriculturalirrigation"),
            Some(RunningMode::SeasonalLoadManagement)
        );
        assert_eq!(parse_mode_name("ultragreen"), Some(RunningMode::UltraGreen));
        assert_eq!(parse_mode_name("invalid"), None);
        assert_eq!(parse_mode_name("6"), None);
    }

    #[tokio::test]
    async fn test_mode_selector_initial_current() {
        let selector = ModeSelector::new(RunningMode::SeasonalLoadManagement, None);
        assert_eq!(selector.current(), RunningMode::SeasonalLoadManagement);
    }

    #[tokio::test]
    async fn test_mode_selector_switch() {
        let mut selector = ModeSelector::new(RunningMode::SeasonalLoadManagement, None);
        let prev = selector
            .switch(RunningMode::CommercialArbitrage, SwitchSource::LocalConfig)
            .await;
        assert_eq!(prev, Ok(RunningMode::SeasonalLoadManagement));
        assert_eq!(selector.current(), RunningMode::CommercialArbitrage);
    }

    #[tokio::test]
    async fn test_mode_selector_switch_idempotent() {
        let mut selector = ModeSelector::new(RunningMode::SeasonalLoadManagement, None);
        let prev = selector
            .switch(
                RunningMode::SeasonalLoadManagement,
                SwitchSource::LocalConfig,
            )
            .await;
        assert_eq!(prev, Ok(RunningMode::SeasonalLoadManagement));
    }

    #[tokio::test]
    async fn test_mode_selector_subscribe() {
        let mut selector = ModeSelector::new(RunningMode::SeasonalLoadManagement, None);
        let mut rx = selector.subscribe();
        let _ = selector
            .switch(RunningMode::DemandControl, SwitchSource::LocalConfig)
            .await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.previous, RunningMode::SeasonalLoadManagement);
        assert_eq!(event.current, RunningMode::DemandControl);
    }

    // ========================================================================
    // v2.10 R3: 场景切换平滑过渡测试用例 (CC1-CC4)
    // ========================================================================

    #[test]
    fn test_transition_steps_configurable() {
        // CC1: 场景切换时触发平滑过渡，过渡步数可配置（默认10步）
        let config5 = TransitionConfig {
            transition_steps: 5,
        };
        let mut transition5 = SmoothSceneTransition::new(config5);
        transition5.on_scene_switch(vec![1.0, 2.0], vec![3.0, 4.0]);
        assert_eq!(transition5.state(), TransitionState::InProgress);
        assert_eq!(transition5.total_steps(), 5);
        assert_eq!(transition5.remaining_steps(), 5);

        // 走完 6 次调用后状态变为 Completed（初始调用 + 5 步）
        for _ in 0..6 {
            let _ = transition5.get_interpolated_weights();
        }
        assert_eq!(transition5.state(), TransitionState::Completed);
        assert_eq!(transition5.remaining_steps(), 0);

        // 默认步数为 10
        let config_default = TransitionConfig::default();
        assert_eq!(config_default.transition_steps, 10);
    }

    #[test]
    fn test_linear_interpolation_first_last() {
        // CC2: 每步权重线性插值，确保最终权重与目标一致
        // 初始调用返回 current_weights，第 10 次调用返回 target_weights
        let config = TransitionConfig {
            transition_steps: 10,
        };
        let mut transition = SmoothSceneTransition::new(config);
        transition.on_scene_switch(vec![0.0, 0.0], vec![10.0, 20.0]);

        // 初始调用（Step 0 后）：应该返回 current_weights
        let weights = transition.get_interpolated_weights();
        assert_eq!(weights.len(), 2);
        assert!((weights[0] - 0.0).abs() < 1e-6);
        assert!((weights[1] - 0.0).abs() < 1e-6);

        // 第 10 次调用（Step 10 后）：应该返回 target_weights
        for _ in 0..9 {
            let _ = transition.get_interpolated_weights();
        }
        let weights = transition.get_interpolated_weights();
        assert!((weights[0] - 10.0).abs() < 1e-6);
        assert!((weights[1] - 20.0).abs() < 1e-6);
    }

    #[test]
    fn test_interpolation_middle() {
        // CC2: 中间步线性验证 - step 5 时每权重 = (current + target) / 2
        let config = TransitionConfig {
            transition_steps: 10,
        };
        let mut transition = SmoothSceneTransition::new(config);
        transition.on_scene_switch(vec![0.0, 0.0], vec![10.0, 20.0]);

        // 前进到 step 5
        for _ in 0..5 {
            let _ = transition.get_interpolated_weights();
        }

        let weights = transition.get_interpolated_weights();
        // alpha = 5/10 = 0.5
        // weight_i = (1 - 0.5) * 0 + 0.5 * target = target / 2
        assert!((weights[0] - 5.0).abs() < 1e-6);
        assert!((weights[1] - 10.0).abs() < 1e-6);
    }

    #[test]
    fn test_transition_auto_stop() {
        // CC4: 过渡完成后自动停止插值，返回目标权重
        let config = TransitionConfig {
            transition_steps: 3,
        };
        let mut transition = SmoothSceneTransition::new(config);
        transition.on_scene_switch(vec![0.0], vec![9.0]);

        // 走完 3 步
        for _ in 0..3 {
            let _ = transition.get_interpolated_weights();
        }
        assert_eq!(transition.state(), TransitionState::Completed);

        // 继续调用应返回目标权重
        for _ in 0..10 {
            let weights = transition.get_interpolated_weights();
            assert!((weights[0] - 9.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_no_control_jump() {
        // CC3: 过渡期间控制指令无突变（梯度 < 5%）
        let config = TransitionConfig {
            transition_steps: 20,
        };
        let mut transition = SmoothSceneTransition::new(config);
        transition.on_scene_switch(vec![100.0], vec![0.0]);

        let mut prev_weight = 100.0;
        let mut max_jump = 0.0f32;

        for _step in 0..20 {
            let weights = transition.get_interpolated_weights();
            let current_weight = weights[0];
            let jump = (prev_weight - current_weight).abs() / prev_weight.max(1e-6);
            max_jump = max_jump.max(jump);
            prev_weight = current_weight;
        }

        // 最大跳跃应小于 5%
        assert!(
            max_jump < 0.05,
            "Max jump {}% exceeds 5% threshold",
            max_jump * 100.0
        );
    }

    #[test]
    fn test_smooth_scene_transition_idle_state() {
        // 初始状态为 Idle
        let transition = SmoothSceneTransition::new(TransitionConfig::default());
        assert_eq!(transition.state(), TransitionState::Idle);
        assert_eq!(transition.remaining_steps(), 10);
    }

    #[test]
    fn test_same_weights_immediate_completion() {
        // 相同权重应立即完成
        let config = TransitionConfig {
            transition_steps: 10,
        };
        let transition =
            SmoothSceneTransition::new_with_weights(config, vec![1.0, 2.0], vec![1.0, 2.0]);
        assert_eq!(transition.state(), TransitionState::Completed);
    }

    #[tokio::test]
    async fn test_mode_selector_with_smooth_transition() {
        // 集成测试：ModeSelector 切换时触发平滑过渡
        let mut selector = ModeSelector::new(RunningMode::SeasonalLoadManagement, None);
        selector.set_transition_config(TransitionConfig {
            transition_steps: 5,
        });

        // 切换场景，触发平滑过渡
        let prev = selector
            .switch(RunningMode::CommercialArbitrage, SwitchSource::LocalConfig)
            .await;
        assert_eq!(prev, Ok(RunningMode::SeasonalLoadManagement));

        // 检查平滑过渡状态
        assert_eq!(
            selector.transition_state(),
            Some(TransitionState::InProgress)
        );
        assert_eq!(selector.remaining_transition_steps(), 5);

        // 获取插值权重
        let weights = selector.current_weights();
        assert!(!weights.is_empty());

        // 完成过渡
        for _ in 0..5 {
            let _ = selector.current_weights();
        }
        assert_eq!(
            selector.transition_state(),
            Some(TransitionState::Completed)
        );
        assert_eq!(selector.remaining_transition_steps(), 0);
    }

    #[test]
    fn test_weights_dimension_mismatch() {
        // 不同维度权重取较短长度
        let config = TransitionConfig {
            transition_steps: 10,
        };
        let mut transition = SmoothSceneTransition::new(config);
        // current 有 8 个权重，target 只有 2 个
        transition.on_scene_switch(vec![1.0; 8], vec![10.0, 20.0]);

        // 应该取 2 个（较短长度）
        let weights = transition.get_interpolated_weights();
        assert_eq!(weights.len(), 2);
    }
}
