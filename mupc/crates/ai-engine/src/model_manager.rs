//! 模型管理器
//!
//! 统一调度 LSTM 预测、数据融合、RL 决策、动作校验和奖励计算。
//!
//! v2.3: rl_model 替换为 model_registry（5 个场景独立 RL 模型，双缓冲热切换）。
//! v2.4: full_decision_cycle 集成 LSTM 预测，预测结果注入融合状态供 RL 使用。
//! v2.5: 动作空间参数可配置化（ActionSpaceConfig）

use crate::action_space::ActionSpaceConfig;
use crate::action_validator::ActionValidator;
use crate::config::{AiEngineConfig, ModeConfig};
use crate::data_fusion::{DataFusionEngine, FusedSystemState};
use crate::dynamic_config_loader::DynamicConfigLoader;
use crate::error::AiEngineError;
use crate::lstm_model::{LstmInput, LstmModel, LstmOutput, ProbabilisticLoadOutput};
use crate::mode_selector::{ModeSelector, RunningMode, SwitchSource};
use crate::model_registry::{ModelRegistry, SceneModelState};
use crate::reward_calculator::RewardCalculator;
use crate::rl_model::ActionOutput;
use crate::safety_wrapper::SafetyRLWrapper;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 模型状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStatus {
    Unloaded,
    Loading,
    Ready,
    Error,
}

/// 模型管理器 — AI 引擎统一调度入口
pub struct ModelManager {
    config: AiEngineConfig,
    lstm_model: Arc<RwLock<Option<LstmModel>>>,
    /// v2.3: 替代原来的单一 rl_model，管理 5 个场景 RL 模型
    model_registry: Arc<RwLock<Option<Arc<ModelRegistry>>>>,
    data_fusion: Arc<RwLock<Option<DataFusionEngine>>>,
    reward_calculator: Arc<RwLock<Option<RewardCalculator>>>,
    action_validator: Arc<RwLock<Option<ActionValidator>>>,
    status: Arc<RwLock<ModelStatus>>,
    /// v2.3: 使用 RwLock 包裹以支持初始化阶段注入 registry
    mode_selector: Arc<RwLock<ModeSelector>>,
    /// LSTM 历史缓冲（pv_power, load_power 样本，容量 = input_window_secs/step_seconds）
    ///
    /// v2.16: 容量计算改用 step_seconds（默认 15 分钟步长）
    lstm_history: Arc<RwLock<VecDeque<(f64, f64)>>>,
    /// LSTM 输入窗口大小（步数，即缓冲容量）
    ///
    /// v2.16: 计算公式从 `input_window_secs / 60` 改为 `input_window_secs / step_seconds`
    lstm_input_size: usize,
    /// LSTM 采样周期数（每 fusion_period_secs × counter_period 步压入一个样本）
    ///
    /// v2.16 新增：用于按 15 分钟步长降采样历史缓冲。
    /// 计算公式：step_seconds / fusion_period_secs
    lstm_sample_period: u64,
    /// LSTM 采样周期计数器
    lstm_sample_counter: Arc<RwLock<u64>>,
    /// v2.5: 动作空间配置（可配置化）
    action_space_config: Arc<RwLock<ActionSpaceConfig>>,
    /// v2.6: 动态配置加载器（延迟初始化，storage 就绪后注入）
    dynamic_config_loader: Arc<RwLock<Option<Arc<DynamicConfigLoader>>>>,
    /// v2.17: 安全 RL 包装器（物理模型前置过滤器）
    safety_wrapper: Arc<SafetyRLWrapper>,
}

impl ModelManager {
    pub fn new(config: AiEngineConfig) -> Self {
        let initial_mode = parse_initial_mode(&config.mode);
        let persist_path = if config.mode.persist_path.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(&config.mode.persist_path))
        };
        let mode_selector = Arc::new(RwLock::new(ModeSelector::new(initial_mode, persist_path)));

        // v2.16: 使用 step_seconds 统一计算输入窗口步数
        let lstm_input_size =
            (config.lstm.input_window_secs / config.lstm.step_seconds) as usize;

        // v2.16: 采样周期 = step_seconds / fusion_period_secs
        // 例如 step=900s, fusion_period=1s → 每 900 步压入一个样本（15 分钟）
        // v2.16.1 (C-03 修复): 加 max(1) 边界保护，避免 step < fusion 时退化为 0（每周期都采样）
        let lstm_sample_period = (config.lstm.step_seconds
            / config.fusion.fusion_period_secs.max(1))
        .max(1);

        // v2.17: 创建 SafetyRLWrapper（无 event_sender，SSE 推送待 main.rs 注入）
        let safety_wrapper = Arc::new(SafetyRLWrapper::new(
            config.safety_wrapper.clone(),
            None,
        ));

        Self {
            config,
            lstm_model: Arc::new(RwLock::new(None)),
            model_registry: Arc::new(RwLock::new(None)),
            data_fusion: Arc::new(RwLock::new(None)),
            reward_calculator: Arc::new(RwLock::new(None)),
            action_validator: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
            mode_selector,
            lstm_history: Arc::new(RwLock::new(VecDeque::with_capacity(lstm_input_size))),
            lstm_input_size,
            lstm_sample_period,
            safety_wrapper,
            lstm_sample_counter: Arc::new(RwLock::new(0)),
            action_space_config: Arc::new(RwLock::new(ActionSpaceConfig::default_config())),
            dynamic_config_loader: Arc::new(RwLock::new(None)),
        }
    }

    /// 加载所有模型和子模块
    ///
    /// v2.3: 使用 ModelRegistry 替代 RLModel，支持 5 个场景独立模型
    pub async fn load_models(&self) -> Result<(), AiEngineError> {
        *self.status.write().await = ModelStatus::Loading;

        // 加载 LSTM 模型（1 个通用模型）
        let mut lstm = LstmModel::new(self.config.lstm.clone())
            .map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        lstm.load()
            .await
            .map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        *self.lstm_model.write().await = Some(lstm);

        // 加载出厂场景 RL 模型（ModelRegistry）
        let factory_scene = parse_initial_mode(&self.config.mode);
        let model_dir = std::path::PathBuf::from(&self.config.mode.model_dir);
        let manifest_path = std::path::PathBuf::from(&self.config.mode.model_manifest);

        // 确保模型目录和清单目录存在
        if let Some(parent) = manifest_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AiEngineError::ModelLoadFailed(format!("创建清单目录失败: {}", e)))?;
        }
        tokio::fs::create_dir_all(&model_dir)
            .await
            .map_err(|e| AiEngineError::ModelLoadFailed(format!("创建模型目录失败: {}", e)))?;

        // 初始化清单文件（如果不存在）
        if !manifest_path.exists() {
            Self::init_default_manifest(&manifest_path, factory_scene).await?;
        }

        let registry = ModelRegistry::new(
            &model_dir,
            &manifest_path,
            factory_scene,
            self.config.rl.algorithm,
            self.config.rl.quantization,
        )
        .await?;

        let registry = Arc::new(registry);

        // 将 registry 注入 ModeSelector
        {
            let mut selector = self.mode_selector.write().await;
            selector.set_registry(registry.clone());
        }

        *self.model_registry.write().await = Some(registry);

        // 初始化奖励计算器和动作校验器
        *self.reward_calculator.write().await =
            Some(RewardCalculator::new(self.config.reward_weights.clone()));
        *self.action_validator.write().await = Some(ActionValidator::new_v2_4(
            self.config.action_constraint.clone(),
        ));

        *self.status.write().await = ModelStatus::Ready;
        Ok(())
    }

    /// 完整 AI 决策周期
    ///
    /// 串联：模式获取 → 数据融合 → LSTM预测 → RL决策 → 约束校验 → 奖励计算
    /// LSTM 预测结果注入融合状态供 RL 使用。
    pub async fn full_decision_cycle(&self) -> Result<ActionOutput, AiEngineError> {
        if !self.is_ready().await {
            return Err(AiEngineError::ModelNotLoaded);
        }

        let running_mode = { self.mode_selector.read().await.current() };

        let fused_state = {
            let mut fusion = self.data_fusion.write().await;
            match fusion.as_mut() {
                Some(df) => df.fuse().await?,
                None => return Err(AiEngineError::FusionFailed("融合引擎未初始化".into())),
            }
        };

        // 记录当前实时数据用于 LSTM 历史缓冲（在融合数据之后、更新缓冲之前）
        let current_pv = fused_state.pv_power;
        let current_load = fused_state.load_power;

        // LSTM 预测：使用历史缓冲区的数据预测未来光伏/负荷（含分位数）
        // v2.16 (C-02 修复): 在生产路径调用 predict_quantiles 并接通 D10 数据流
        let (pv_forecast, load_forecast, load_quantiles) = self
            .run_lstm_predict_with_quantiles()
            .await
            .unwrap_or_else(|_| (vec![0.0; 15], vec![0.0; 15], None));

        // 将 LSTM 预测结果注入融合状态（克隆以避免借用冲突）
        let mut fused_state_with_forecast = fused_state.clone();
        fused_state_with_forecast.pv_forecast_15min = pv_forecast.clone();
        fused_state_with_forecast.load_forecast_15min = load_forecast.clone();

        // v2.16: 接通 D10 分位数数据流（PRD §3.6 变更 3）
        if let Some(ref prob_output) = load_quantiles {
            self.update_fused_state_quantiles(&mut fused_state_with_forecast, prob_output)
                .await;
        }

        // v2.3: 通过 ModelRegistry 执行推理（委托给当前 active 的场景模型）
        let rl_action = {
            let registry = self.model_registry.read().await;
            let registry = registry.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
            let input_vector = fused_state_with_forecast.to_input_vector();
            let action_space_config = self.action_space_config.read().await;
            registry.decide(&input_vector, &action_space_config).await?
        };

        // Step 6.5: v2.17 安全包装器检查（RL 决策后、ActionValidator 前）
        // 基于戴维南等效电路预测电压变化，提前拒绝高风险动作
        let (safe_action, check_result) = self
            .safety_wrapper
            .check_and_fallback(&fused_state, &rl_action)
            .await;

        if check_result.is_rejected() {
            tracing::warn!(
                target = "safety_wrapper",
                "动作被安全包装器拒绝，回退到 last_safe_action"
            );
        }

        let (validated, violations) = {
            let av = self.action_validator.read().await;
            let av = av.as_ref().ok_or(AiEngineError::ActionValidationFailed(
                "校验器未初始化".into(),
            ))?;
            let action_space_config = self.action_space_config.read().await;
            av.validate(
                &safe_action,
                fused_state.dispatch_p_set,
                false,
                &action_space_config,
            )
        };

        for v in &violations {
            tracing::warn!(
                "动作约束违规: rule={} field={} original={} clamped={}",
                v.rule,
                v.field,
                v.original,
                v.clamped
            );
        }

        // 更新 LSTM 历史缓冲（决策完成后再更新，避免用到本周期数据）
        // v2.16: 按 lstm_sample_period 降采样（每 step_seconds/fusion_period_secs 步压入一个样本）
        {
            let should_sample = {
                let mut counter = self.lstm_sample_counter.write().await;
                *counter += 1;
                *counter % self.lstm_sample_period == 0
            };
            if should_sample {
                let mut history = self.lstm_history.write().await;
                history.push_back((current_pv, current_load));
                while history.len() > self.lstm_input_size {
                    history.pop_front();
                }
            }
        }

        let _reward = {
            let rc = self.reward_calculator.read().await;
            match rc.as_ref() {
                Some(rc) => rc.calculate(running_mode, &validated, &fused_state),
                None => 0.0,
            }
        };

        // 更新奖励计算器中的上一周期电池功率（用于下一周期 R_ramp 计算）
        if let Some(rc) = self.reward_calculator.read().await.as_ref() {
            rc.update_last_p_batt(validated.p_ref);
        }

        Ok(validated)
    }

    /// 执行 LSTM 预测（使用历史缓冲区的 pv_power / load_power 样本，含分位数）
    ///
    /// v2.16 (C-02 修复): 返回值扩展为 (pv_forecast, load_forecast, Option<ProbabilisticLoadOutput>)。
    /// 第三项为负荷分位数预测结果（用于接通 D10 数据流）。
    ///
    /// 返回值约定：
    /// - LSTM 未就绪或缓冲不足时，第三项为 None
    /// - 正常预测时，第三项为 Some(ProbabilisticLoadOutput)
    async fn run_lstm_predict_with_quantiles(
        &self,
    ) -> Result<(Vec<f64>, Vec<f64>, Option<ProbabilisticLoadOutput>), AiEngineError> {
        let lstm = self.lstm_model.read().await;
        let lstm = match lstm.as_ref() {
            Some(m) => m,
            None => return Ok((vec![0.0; 15], vec![0.0; 15], None)),
        };

        if !lstm.runtime().is_loaded() {
            return Ok((vec![0.0; 15], vec![0.0; 15], None));
        }

        let history = self.lstm_history.read().await;
        let len = history.len();

        // 需要至少 input_size 个样本才能构建有效输入
        if len < self.lstm_input_size {
            tracing::debug!(
                "LSTM 历史缓冲不足 ({}/{})，跳过本周期预测",
                len,
                self.lstm_input_size
            );
            return Ok((vec![0.0; 15], vec![0.0; 15], None));
        }

        // 构建 PV 历史输入（取最近的 input_size 个样本）
        let pv_history: Vec<f32> = history
            .iter()
            .rev()
            .take(self.lstm_input_size)
            .map(|&(pv, _)| pv as f32)
            .collect();
        let pv_history: Vec<f32> = pv_history.into_iter().rev().collect();

        let load_history: Vec<f32> = history
            .iter()
            .rev()
            .take(self.lstm_input_size)
            .map(|&(_, load)| load as f32)
            .collect();
        let load_history: Vec<f32> = load_history.into_iter().rev().collect();

        // 预测 PV（使用 PV 历史）
        let pv_input = LstmInput {
            history: pv_history,
            timestamp: chrono::Utc::now().timestamp(),
        };
        let pv_output = lstm.predict(&pv_input).await?;
        let pv_forecast: Vec<f64> = pv_output
            .predictions
            .into_iter()
            .map(|v| v as f64)
            .collect();

        // 预测负荷（使用负荷历史 + 分位数）
        let load_input = LstmInput {
            history: load_history,
            timestamp: chrono::Utc::now().timestamp(),
        };
        let load_output = lstm.predict(&load_input).await?;
        let load_forecast: Vec<f64> = load_output
            .predictions
            .into_iter()
            .map(|v| v as f64)
            .collect();

        // v2.16: 计算分位数预测（使用默认协变量，C-02 数据流通）
        // TODO: 后续可从 LoadCovariates 数据源注入真实协变量
        let covariates = crate::load_covariates::LoadCovariates::default();
        let load_quantiles = match lstm.predict_quantiles(&load_input, &covariates).await {
            Ok(pq) => Some(pq),
            Err(e) => {
                tracing::warn!("LSTM 分位数预测失败，回退为零向量: {}", e);
                None
            }
        };

        Ok((pv_forecast, load_forecast, load_quantiles))
    }

    /// 设置数据融合引擎（由外部注入）
    pub async fn set_data_fusion(&self, df: DataFusionEngine) {
        *self.data_fusion.write().await = Some(df);
    }

    /// v2.9: 获取当前系统状态（用于异常检测）
    ///
    /// 返回最近一次融合的 FusedSystemState。如果融合引擎未初始化或无数据，返回 None。
    pub async fn get_current_state(&self) -> Option<FusedSystemState> {
        let fusion = self.data_fusion.read().await;
        match fusion.as_ref() {
            Some(df) => {
                let state = df.last_fused_state.read().await;
                state.clone()
            }
            None => None,
        }
    }

    /// 预测（LSTM）
    pub async fn predict(&self, input: &LstmInput) -> Result<LstmOutput, AiEngineError> {
        let lstm = self.lstm_model.read().await;
        let lstm = lstm.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
        lstm.predict(input).await
    }

    /// v2.16: 将 LSTM 分位数预测结果写入 FusedSystemState.D10
    ///
    /// 修复 D10 数据流未接通的 bug（model_manager 之前未调用 predict_quantiles，
    /// 导致 FusedSystemState.load_forecast_quantiles 始终为空向量，RL 输入全 0）。
    ///
    /// 行为：
    /// - `load_forecast_quantiles` = 15 步 P90 值（与 `reward_calculator.rs:586` 注释一致）
    /// - `base_load` = 第 1 步 P50
    /// - `shock_load_probability` = 冲击概率
    ///
    /// 调用方应在 `full_decision_cycle` 中融合 → LSTM 预测之后插入此调用。
    pub async fn update_fused_state_quantiles(
        &self,
        state: &mut FusedSystemState,
        prob_output: &ProbabilisticLoadOutput,
    ) {
        state.load_forecast_quantiles = prob_output
            .quantile_steps
            .iter()
            .map(|s| s.p90 as f64)
            .collect();
        state.base_load = prob_output.base_load as f64;
        state.shock_load_probability = prob_output.shock_probability;
    }

    /// 决策（通过 ModelRegistry 委托给当前 active 场景模型）
    pub async fn decide(&self, input_vector: &[f32]) -> Result<ActionOutput, AiEngineError> {
        let registry = self.model_registry.read().await;
        let registry = registry.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
        let action_space_config = self.action_space_config.read().await;
        registry.decide(input_vector, &action_space_config).await
    }

    pub async fn get_status(&self) -> ModelStatus {
        *self.status.read().await
    }

    pub async fn is_ready(&self) -> bool {
        self.get_status().await == ModelStatus::Ready
    }

    pub async fn lstm_ready(&self) -> bool {
        self.lstm_model.read().await.is_some()
    }

    /// v2.3: 检查 ModelRegistry 是否就绪（替代原 rl_ready）
    pub async fn registry_ready(&self) -> bool {
        self.model_registry.read().await.is_some()
    }

    /// v2.3: 获取当前激活场景的模型状态
    pub async fn active_scene_model_state(&self) -> Option<SceneModelState> {
        let registry = self.model_registry.read().await;
        match registry.as_ref() {
            Some(r) => {
                let current_mode = self.mode_selector.read().await.current();
                Some(r.model_state(current_mode))
            }
            None => None,
        }
    }

    /// 获取 ModeSelector 的只读 guard
    pub async fn mode_selector(&self) -> tokio::sync::RwLockReadGuard<'_, ModeSelector> {
        self.mode_selector.read().await
    }

    /// v2.3: 获取 ModeSelector 的 Arc<RwLock<ModeSelector>> 引用（供外部持有）
    pub fn mode_selector_arc(&self) -> Arc<RwLock<ModeSelector>> {
        self.mode_selector.clone()
    }

    /// v2.5: 设置动作空间配置（可配置化）
    pub async fn set_action_space_config(&self, config: ActionSpaceConfig) {
        let mut cfg = self.action_space_config.write().await;
        *cfg = config;
    }

    /// v2.6: 注入 DynamicConfigLoader（storage 就绪后调用）
    pub async fn set_dynamic_config_loader(&self, loader: DynamicConfigLoader) {
        *self.dynamic_config_loader.write().await = Some(Arc::new(loader));
    }

    /// v2.6: 校验配置指纹与模型指纹一致性
    pub async fn validate_config_fingerprint(
        &self,
        model_fingerprint: &str,
    ) -> Result<(), AiEngineError> {
        let loader = self.dynamic_config_loader.read().await;
        if let Some(ref loader) = *loader {
            loader.validate_fingerprint(model_fingerprint).await?;
        }
        Ok(())
    }

    pub async fn current_mode(&self) -> RunningMode {
        self.mode_selector.read().await.current()
    }

    pub async fn switch_mode(
        &self,
        new_mode: RunningMode,
        source: SwitchSource,
    ) -> Result<RunningMode, AiEngineError> {
        self.mode_selector
            .write()
            .await
            .switch(new_mode, source)
            .await
    }

    /// v2.3: 获取 ModelRegistry 引用
    pub async fn registry(&self) -> Option<Arc<ModelRegistry>> {
        self.model_registry.read().await.clone()
    }

    /// 初始化默认的 manifest.json 文件
    async fn init_default_manifest(
        manifest_path: &std::path::Path,
        factory_scene: RunningMode,
    ) -> Result<(), AiEngineError> {
        let scene_key = match factory_scene {
            RunningMode::SeasonalLoadManagement => "SeasonalLoadManagement",
            RunningMode::CommercialArbitrage => "CommercialArbitrage",
            RunningMode::DemandControl => "DemandControl",
            RunningMode::VirtualPowerPlant => "VirtualPowerPlant",
            RunningMode::UltraGreen => "UltraGreen",
        };

        let file_name = match factory_scene {
            RunningMode::SeasonalLoadManagement => "rl_seasonal.rknn",
            RunningMode::CommercialArbitrage => "rl_arbitrage.rknn",
            RunningMode::DemandControl => "rl_demand.rknn",
            RunningMode::VirtualPowerPlant => "rl_vpp.rknn",
            RunningMode::UltraGreen => "rl_green.rknn",
        };

        let manifest = serde_json::json!({
            "version": "1.0",
            "updated_at": chrono::Utc::now().to_rfc3339(),
            "models": {
                (scene_key): {
                    "file_name": file_name,
                    "sha256": "",
                    "file_size_bytes": 0,
                    "version": "0.1.0"
                }
            }
        });

        let content = serde_json::to_string_pretty(&manifest)
            .map_err(|e| AiEngineError::ModelLoadFailed(format!("序列化清单失败: {}", e)))?;

        tokio::fs::write(manifest_path, content)
            .await
            .map_err(|e| AiEngineError::ModelLoadFailed(format!("写入清单文件失败: {}", e)))?;

        tracing::info!("已创建默认模型清单: {}", manifest_path.display());
        Ok(())
    }
}

fn parse_initial_mode(config: &ModeConfig) -> RunningMode {
    crate::mode_selector::parse_mode_name(&config.factory_scene)
        .unwrap_or(RunningMode::SeasonalLoadManagement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LstmConfig, ModeConfig, OnlineUpdateConfig, RlAlgorithm, RlConfig};

    fn create_test_config() -> AiEngineConfig {
        AiEngineConfig {
            lstm: LstmConfig {
                model_path: std::path::PathBuf::from("/tmp/test_lstm.rknn"),
                input_window_secs: 3600,
                output_horizon_secs: 900,
                step_seconds: 60, // 测试用 1 分钟步长
                quantization: crate::config::QuantizationType::INT8,
                expected_sha256: None,
            },
            rl: RlConfig {
                model_path: std::path::PathBuf::from("/tmp/test_rl.rknn"),
                algorithm: RlAlgorithm::MADDPG,
                quantization: crate::config::QuantizationType::INT8,
                expected_sha256: None,
            },
            online_update: OnlineUpdateConfig::default(),
            mode: ModeConfig::default(),
            ..Default::default()
        }
    }

    #[test]
    fn test_model_manager_creation() {
        let config = create_test_config();
        let manager = ModelManager::new(config);
        assert_eq!(manager.get_status_blocking(), ModelStatus::Unloaded);
    }

    #[test]
    fn test_parse_initial_mode_uses_factory_scene() {
        let config = ModeConfig {
            factory_scene: "DemandControl".to_string(),
            ..Default::default()
        };
        assert_eq!(parse_initial_mode(&config), RunningMode::DemandControl);
    }
}

impl ModelManager {
    #[allow(dead_code)]
    fn get_status_blocking(&self) -> ModelStatus {
        // 使用 try_read 无锁读取实际状态，仅在极低概率写锁冲突时回退
        match self.status.try_read() {
            Ok(guard) => *guard,
            Err(_) => tokio::task::block_in_place(|| {
                futures::executor::block_on(async { *self.status.read().await })
            }),
        }
    }
}
