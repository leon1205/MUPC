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
use crate::data_fusion::{normalize_observation, DataFusionEngine, FusedSystemState};
use crate::dynamic_config_loader::DynamicConfigLoader;
use crate::error::AiEngineError;
use crate::lstm_model::{LstmInput, LstmModel, LstmOutput, ProbabilisticLoadOutput, QuantilePrediction};
use crate::mode_selector::{ModeSelector, RunningMode, SwitchSource};
use crate::model_registry::{ModelRegistry, SceneModelState};
use crate::pipeline_config::EnhancementLevel;
use crate::prediction_pipeline::PredictionPipeline;
use crate::reward_calculator::RewardCalculator;
use crate::online_updater::{
    DataPoint, DefaultPerformanceMonitor, DefaultSafetyChecker, OnlineUpdater, SafeOnlineUpdater,
};
use crate::rl_model::ActionOutput;
use crate::safety_wrapper::{SafetyRLWrapper, SafetyWrapperEvent};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 在线微调安全阈值（0-100，权重方差越小评分越高）
const ONLINE_UPDATE_SAFETY_THRESHOLD: f32 = 70.0;
/// 在线微调性能阈值（影子模型性能 / 当前模型性能 ≥ 此值才接受更新）
const ONLINE_UPDATE_PERFORMANCE_THRESHOLD: f32 = 0.95;

/// 模型状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStatus {
    Unloaded,
    Loading,
    Ready,
    Error,
}

/// v3.0: 历史样本（7 维特征）
///
/// 与 MUPC-AI2 训练管线 `prepare_data()` 的 7 维特征一一对应：
/// `[pv_power, load_power, ghi, temp, sin_hour, cos_hour, yesterday_pv]`
#[derive(Debug, Clone)]
pub struct HistorySample {
    pub pv_power: f64,
    pub load_power: f64,
    pub solar_irradiance: f64,
    pub temperature: f64,
    pub sin_hour: f64,
    pub cos_hour: f64,
    pub yesterday_pv: f64,
}

impl HistorySample {
    /// 展平为 7 个 f32（按训练管线特征顺序）
    pub fn to_features(&self) -> [f32; 7] {
        [
            self.pv_power as f32,
            self.load_power as f32,
            self.solar_irradiance as f32,
            self.temperature as f32,
            self.sin_hour as f32,
            self.cos_hour as f32,
            self.yesterday_pv as f32,
        ]
    }
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
    /// v3.0: LSTM 历史缓冲（7 维样本，容量 = input_size + yesterday_offset）
    ///
    /// 容量 = max(input_window_steps + yesterday_offset_steps, 120)
    /// 用于构建 (T, 7) 多特征输入 + 提供 96 步前的 yesterday_pv 特征。
    lstm_history: Arc<RwLock<VecDeque<HistorySample>>>,
    /// LSTM 输入窗口大小（步数，即缓冲中用于构建输入的最近样本数）
    ///
    /// v2.16: 计算公式 `input_window_secs / step_seconds`
    lstm_input_size: usize,
    /// v3.0: LSTM 历史缓冲总容量（步数）
    lstm_history_capacity: usize,
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
    /// v3.1: 安全事件 broadcast Receiver（供 bin crate / SSE 订阅）
    safety_event_rx: tokio::sync::broadcast::Receiver<SafetyWrapperEvent>,
    /// v1.0: 预测增强管线（VMD + Attention 编排器）
    /// None 表示增强未启用，回退到基线 LSTM 推理路径
    prediction_pipeline: Arc<RwLock<Option<PredictionPipeline>>>,
    /// v3.1: 在线数据收集器（PER 缓冲区 + 场景隔离）
    online_updater: Arc<RwLock<OnlineUpdater>>,
    /// v3.1: 安全在线微调编排器（影子模型验证 + 渐进式切换）
    safe_updater: Arc<SafeOnlineUpdater>,
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
        let lstm_input_size = (config.lstm.input_window_secs / config.lstm.step_seconds) as usize;

        // v3.0: 缓冲容量 = max(input_size + yesterday_offset_steps, 120)
        // 确保有足够历史数据构建 7 维输入 + yesterday_pv 特征
        let lstm_history_capacity = (lstm_input_size + config.lstm.yesterday_offset_steps).max(120);

        // v2.16: 采样周期 = step_seconds / fusion_period_secs
        // 例如 step=900s, fusion_period=1s → 每 900 步压入一个样本（15 分钟）
        // v2.16.1 (C-03 修复): 加 max(1) 边界保护，避免 step < fusion 时退化为 0（每周期都采样）
        let lstm_sample_period =
            (config.lstm.step_seconds / config.fusion.fusion_period_secs.max(1)).max(1);

        // v2.17/v3.1: 创建 broadcast channel 并注入 SafetyRLWrapper
        let (safety_event_tx, safety_event_rx) = tokio::sync::broadcast::channel::<SafetyWrapperEvent>(64);
        let safety_wrapper = Arc::new(SafetyRLWrapper::new(
            config.safety_wrapper.clone(),
            Some(safety_event_tx),
        ));

        // v1.0: 创建 LSTM 模型和历史缓冲的 Arc（供 PredictionPipeline 复用）
        let lstm_model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let lstm_history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(lstm_history_capacity)));

        // v1.0: 创建预测增强管线（如果配置了 prediction_enhancement）
        //
        // v3.0: VMD 路径与多特征 (input_features > 1) 不兼容（VMD 仅适用于单变量时序）。
        // PredictionPipeline 内部在 input_features > 1 时自动将 VMD 等级降级到 AttentionOnly。
        let prediction_pipeline = if let Some(ref enh_config) = config.prediction_enhancement {
            let pipeline = PredictionPipeline::new(
                enh_config.clone(),
                lstm_model.clone(),
                lstm_history.clone(),
                lstm_input_size,
                config.lstm.input_features,
            );
            tracing::info!(
                "预测增强管线已启用: 初始等级={:?}, input_features={}",
                pipeline.current_level(),
                config.lstm.input_features
            );
            Some(pipeline)
        } else {
            tracing::info!("预测增强未配置，使用基线 LSTM 推理路径");
            None
        };

        // v3.1: 构造数据融合引擎（预测由 full_decision_cycle 的 run_enhanced_predict 负责）
        let fusion_engine = DataFusionEngine::new();

        // v3.1: 初始化在线微调组件
        let online_updater = Arc::new(RwLock::new(OnlineUpdater::new(
            config.online_update.clone(),
        )));
        let safety_checker = Arc::new(DefaultSafetyChecker::new(
            ONLINE_UPDATE_SAFETY_THRESHOLD,
        ));
        let perf_monitor = Arc::new(DefaultPerformanceMonitor::new(
            ONLINE_UPDATE_PERFORMANCE_THRESHOLD,
        ));
        let safe_updater = Arc::new(SafeOnlineUpdater::new(
            config.online_update.clone(),
            safety_checker,
            perf_monitor,
            ONLINE_UPDATE_SAFETY_THRESHOLD,
            ONLINE_UPDATE_PERFORMANCE_THRESHOLD,
            config.online_update.gradual_switch.clone(),
            Vec::new(), // initial_weights: 待模型加载后更新
        ));

        Self {
            config,
            lstm_model,
            model_registry: Arc::new(RwLock::new(None)),
            data_fusion: Arc::new(RwLock::new(Some(fusion_engine))),
            reward_calculator: Arc::new(RwLock::new(None)),
            action_validator: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
            mode_selector,
            lstm_history,
            lstm_input_size,
            lstm_history_capacity,
            lstm_sample_period,
            safety_wrapper,
            safety_event_rx,
            prediction_pipeline: Arc::new(RwLock::new(prediction_pipeline)),
            lstm_sample_counter: Arc::new(RwLock::new(0)),
            action_space_config: Arc::new(RwLock::new(ActionSpaceConfig::default_config())),
            dynamic_config_loader: Arc::new(RwLock::new(None)),
            online_updater,
            safe_updater,
        }
    }

    /// v3.1: 订阅安全事件流（供 bin crate / web-api SSE 推送使用）
    ///
    /// 返回一个新的 broadcast Receiver，可多次调用创建多个独立订阅。
    /// 每个 Receiver 通过 `recv()` 异步接收 SafetyWrapperEvent。
    pub fn subscribe_safety_events(&mut self) -> tokio::sync::broadcast::Receiver<SafetyWrapperEvent> {
        // resubscribe() 创建新的 Receiver 订阅同一 Sender
        self.safety_event_rx.resubscribe()
    }

    /// 加载 RKNN 模型到 NPU
    ///
    /// 仅在 `npu` feature 启用时实际执行 RKNN Runtime 加载。
    /// 无 NPU 时记录 WARN 日志并返回 Ok（不阻塞启动）。
    ///
    /// # 参数
    /// - `model_path`: .rknn 模型文件路径
    /// - `expected_sha256`: 可选的 SHA256 期望值（None 时跳过校验）
    #[cfg(feature = "npu")]
    pub async fn load_rknn_model(
        &self,
        model_path: &std::path::Path,
        expected_sha256: Option<&str>,
    ) -> Result<(), AiEngineError> {
        use crate::rknn_runtime::RknnRuntime;

        tracing::info!(
            "加载 RKNN 模型: {} (NPU enabled)",
            model_path.display()
        );

        let runtime = RknnRuntime::new(model_path, expected_sha256)?;
        runtime.load().await?;

        tracing::info!("RKNN 模型加载成功: {}", model_path.display());
        Ok(())
    }

    /// 无 NPU 时的回退：记录 WARN 日志，走 CPU LSTM 推理路径
    #[cfg(not(feature = "npu"))]
    pub async fn load_rknn_model(
        &self,
        _model_path: &std::path::Path,
        _expected_sha256: Option<&str>,
    ) -> Result<(), AiEngineError> {
        tracing::warn!("npu feature 未启用，RKNN 模型加载跳过。使用纯 CPU 推理。");
        Ok(())
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
        *self.action_validator.write().await = Some(ActionValidator::new_dual(
            self.config.action_constraint.clone(),
            0.0,
            30.0,
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
        let current_ghi = fused_state.solar_irradiance;
        let current_temp = fused_state.temperature;

        // LSTM 预测：使用历史缓冲区的数据预测未来光伏/负荷（含分位数）
        //
        // v1.0: 优先使用预测增强管线（VMD + Attention），失败时自动降级到基线路径
        // v2.16 (C-02 修复): 在生产路径调用 predict_quantiles 并接通 D10 数据流
        let (pv_forecast, load_forecast, load_quantiles) = self
            .run_enhanced_predict()
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
        // v3.0 (P0-1): 对观测向量做 MinMax 归一化后再送入 ONNX，对齐训练管线 normalize_obs()
        let rl_action = {
            let registry = self.model_registry.read().await;
            let registry = registry.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
            let raw_vector = fused_state_with_forecast.to_input_vector();
            let normalized_vector = normalize_observation(&raw_vector);
            let action_space_config = self.action_space_config.read().await;
            registry.decide(&normalized_vector, &action_space_config).await?
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
            av.validate_dual(
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
        // v3.0: 采集全部 7 维特征，包括 GHI、温度、时间编码和昨日 PV
        {
            let should_sample = {
                let mut counter = self.lstm_sample_counter.write().await;
                *counter += 1;
                *counter % self.lstm_sample_period == 0
            };
            if should_sample {
                // v3.0: 计算时间编码
                let now = chrono::Utc::now();
                let hour = (now.timestamp() % 86400) as f64 / 3600.0;
                let sin_hour = (2.0 * std::f64::consts::PI * hour / 24.0).sin();
                let cos_hour = (2.0 * std::f64::consts::PI * hour / 24.0).cos();

                // v3.0: 从缓冲头部提取 96 步前的 PV 作为 yesterday_pv
                // 冷启动时（缓冲不足 yesterday_offset_steps）用当前 PV 回退（与训练侧一致）
                let yesterday_offset = self.config.lstm.yesterday_offset_steps;
                let mut history = self.lstm_history.write().await;
                let yesterday_pv = if history.len() >= yesterday_offset {
                    history.get(history.len() - yesterday_offset)
                        .map(|s| s.pv_power)
                        .unwrap_or(current_pv)
                } else {
                    current_pv // 冷启动 fallback
                };

                let sample = HistorySample {
                    pv_power: current_pv,
                    load_power: current_load,
                    solar_irradiance: current_ghi,
                    temperature: current_temp,
                    sin_hour,
                    cos_hour,
                    yesterday_pv,
                };
                history.push_back(sample);
                while history.len() > self.lstm_history_capacity {
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

        // v3.1: 在线微调数据收集（写入 OnlineUpdater 的 PER 缓冲区）
        if self.config.online_update.enabled {
            let raw_vector = fused_state_with_forecast.to_input_vector();
            let output = vec![validated.p_ref as f32, validated.k_droop as f32];
            let data = DataPoint::new(chrono::Utc::now().timestamp(), raw_vector, output);
            self.online_updater.write().await.add_sample(data);
        }

        Ok(validated)
    }

    /// v3.1: 尝试触发在线微调更新周期
    ///
    /// 当 PER 缓冲区积累足够样本（≥ batch_size）时，执行一次 mini-batch 更新：
    /// PER 采样 → KL 正则化 → 安全校验 → 渐进式切换。
    ///
    /// 当前权重更新部分受限于 RKNN 私有格式（无法原地更新权重），
    /// 完整的权重热切换需等待 Phase 3C.2 RKNN SDK 适配或走 OTA 全量替换路径。
    ///
    /// 返回 `Ok(true)` 表示更新已触发，`Ok(false)` 表示样本不足跳过。
    pub async fn try_online_update(&self) -> Result<bool, AiEngineError> {
        if !self.config.online_update.enabled {
            return Ok(false);
        }

        let batch_size = self.config.online_update.batch_size;
        let sample_count = {
            let updater = self.online_updater.read().await;
            updater.per_buffer().len()
        };

        if sample_count < batch_size {
            tracing::debug!(
                "在线微调样本不足: per={}, need={}",
                sample_count,
                batch_size
            );
            return Ok(false);
        }

        tracing::info!(
            "在线微调数据就绪: per={} batch={}",
            sample_count,
            batch_size
        );

        // Phase 3C.2 完整实现路径：
        // 1. 从 .rknn 模型文件提取当前权重（需 RKNN SDK API 或训练侧同步导出 weights.bin）
        // 2. PerBuffer.sample(batch_size) → mini-batch 梯度更新 → 新权重
        // 3. SafeOnlineUpdater.safe_update(new_weights) → 安全校验 → 渐进式切换
        // 4. ModelRegistry.hot_swap_weights() → 写入临时 .rknn → 重载 session
        //
        // 当前：数据收集和 PER 管理已就绪，等待 RKNN 权重更新 API
        tracing::info!(
            "在线微调数据收集完成（{} 样本），等待 Phase 3C.2 RKNN 权重更新适配",
            sample_count
        );

        Ok(false)
    }

    /// v3.1: 获取在线微调器（供外部监控数据收集状态）
    pub fn online_updater(&self) -> &Arc<RwLock<OnlineUpdater>> {
        &self.online_updater
    }

    /// v3.1: 获取安全微调编排器（供外部查询切换状态）
    pub fn safe_updater(&self) -> &Arc<SafeOnlineUpdater> {
        &self.safe_updater
    }

    /// 执行 LSTM 预测（使用历史缓冲区构建 7 维联合输入）
    ///
    /// v3.0: 构建展平的 (T, 7) 输入，单次 ONNX 推理联合输出 PV + Load + D10 分位数。
    /// v2.16 兼容: `input_features=1` 时回退到分别预测（PV/Load 各一次推理）。
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

        let input_features = self.config.lstm.input_features.max(1);

        // v3.0: 构建展平的 (T, K) 多特征输入
        // 布局: row-major — [t0_f0, t0_f1, ..., t0_f6, t1_f0, ..., t_{T-1}_f_{K-1}]
        let mut flat_input = Vec::with_capacity(self.lstm_input_size * input_features);
        for sample in history.iter().rev().take(self.lstm_input_size).rev() {
            let features = sample.to_features();
            // v3.0: 取前 input_features 个特征（支持 input_features < 7 的子集模式）
            flat_input.extend_from_slice(&features[..input_features.min(7)]);
        }

        let timestamp = chrono::Utc::now().timestamp();
        let input = LstmInput {
            history: flat_input,
            timestamp,
        };

        // 单次 ONNX 联合推理（替代 v2.16 的 PV/Load 分别推理）
        let output = lstm.predict(&input).await?;
        let output_len = output.predictions.len();

        // v3.0: 根据输出维度自动检测格式并解析
        match output_len {
            90 => {
                // p10p50p90 格式: (2, 15, 3) = [PV:P10(15), PV:P50(15), PV:P90(15),
                //                                Load:P10(15), Load:P50(15), Load:P90(15)]
                let pv_p50: Vec<f64> = output.predictions[15..30].iter().map(|&v| v as f64).collect();
                let load_p50: Vec<f64> = output.predictions[60..75].iter().map(|&v| v as f64).collect();

                // 从同一输出构建 D10 分位数
                let load_quantiles = self.build_quantiles_from_output(
                    &output.predictions, timestamp, output_len,
                );

                Ok((pv_p50, load_p50, load_quantiles))
            }
            47 | 30 => {
                // legacy 格式: [pv(15), load(15), (quantiles(15), shock(1), base(1))]
                let pv_forecast: Vec<f64> = output.predictions[..15].iter().map(|&v| v as f64).collect();
                let load_forecast: Vec<f64> = output.predictions[15..30].iter().map(|&v| v as f64).collect();

                let load_quantiles = if output_len >= 47 {
                    self.build_quantiles_from_output(
                        &output.predictions, timestamp, output_len,
                    )
                } else {
                    None
                };

                Ok((pv_forecast, load_forecast, load_quantiles))
            }
            _ => {
                // 未知格式：取前 15 维作为 PV，回退
                tracing::warn!(
                    "LSTM 输出维度 {} 未识别，使用前 15 维作为 PV 预测",
                    output_len
                );
                let pv: Vec<f64> = output.predictions.iter().take(15).map(|&v| v as f64).collect();
                Ok((pv, vec![0.0; 15], None))
            }
        }
    }

    /// v3.0: 从 ONNX 输出构建 ProbabilisticLoadOutput
    ///
    /// 根据输出长度自动选择 p10p50p90（90维）或 legacy（47维）解析路径。
    fn build_quantiles_from_output(
        &self,
        predictions: &[f32],
        timestamp: i64,
        output_len: usize,
    ) -> Option<ProbabilisticLoadOutput> {
        if output_len >= 90 {
            // p10p50p90: Load P10/P50/P90 分别在 [45..60), [60..75), [75..90)
            let base_load = *predictions.get(60).unwrap_or(&0.0);
            let p50_first = *predictions.get(60).unwrap_or(&0.0);
            let p90_first = *predictions.get(75).unwrap_or(&p50_first);

            let mut quantiles: Vec<QuantilePrediction> = Vec::with_capacity(45);
            for i in 0..15 {
                quantiles.push(QuantilePrediction { quantile: 0.10, value: predictions[45 + i] });
                quantiles.push(QuantilePrediction { quantile: 0.50, value: predictions[60 + i] });
                quantiles.push(QuantilePrediction { quantile: 0.90, value: predictions[75 + i] });
            }

            Some(ProbabilisticLoadOutput {
                timestamp,
                quantiles,
                base_load,
                shock_probability: self.compute_shock_static(p50_first, p90_first),
                confidence: self.compute_conf_static(p50_first, p90_first),
            })
        } else if output_len >= 47 {
            // legacy: [pv(15), load(15), quantiles(15), shock(1), base(1)]
            let base_load = *predictions.get(46).unwrap_or(&0.0);
            let p50_first = *predictions.get(15).unwrap_or(&base_load);
            let p90_first = *predictions.get(30).unwrap_or(&p50_first);

            let mut quantiles: Vec<QuantilePrediction> = Vec::with_capacity(45);
            for i in 0..15 {
                let p50 = *predictions.get(15 + i).unwrap_or(&0.0);
                let p90 = *predictions.get(30 + i).unwrap_or(&p50);
                let p10 = (p50 * 0.7).max(0.0);
                quantiles.push(QuantilePrediction { quantile: 0.10, value: p10 });
                quantiles.push(QuantilePrediction { quantile: 0.50, value: p50 });
                quantiles.push(QuantilePrediction { quantile: 0.90, value: p90 });
            }

            Some(ProbabilisticLoadOutput {
                timestamp,
                quantiles,
                base_load,
                shock_probability: self.compute_shock_static(p50_first, p90_first),
                confidence: self.compute_conf_static(p50_first, p90_first),
            })
        } else {
            None
        }
    }

    /// Static inline shock probability
    fn compute_shock_static(&self, base_load: f32, high_quantile: f32) -> f64 {
        let spread = (high_quantile - base_load).max(1e-6);
        let _std_approx = spread / 1.28;
        (0.5 * Self::erfc_helper(2.0 / std::f32::consts::SQRT_2)) as f64
    }

    /// Static inline confidence
    fn compute_conf_static(&self, p50: f32, p90: f32) -> f64 {
        let spread_ratio = (p90 - p50) / p50.max(1e-6);
        (1.0 - spread_ratio.min(1.0)).max(0.0) as f64
    }

    /// erfc approximation
    fn erfc_helper(x: f32) -> f32 {
        let abs_x = x.abs();
        if abs_x > 8.0 { return 0.0; }
        let exp_term = (-x * x).exp();
        let denom = std::f32::consts::PI * abs_x + (std::f32::consts::PI * x * x + 4.0).sqrt();
        exp_term / denom
    }

    /// v1.0: 增强预测入口（Pipeline 优先 → 基线降级）
    ///
    /// 若预测增强管线可用，优先执行 VMD + Attention 增强预测；
    /// 否则回退到基线 `run_lstm_predict_with_quantiles()` 路径。
    ///
    /// 返回值与 `run_lstm_predict_with_quantiles()` 保持兼容：
    /// `(pv_forecast, load_forecast, Option<ProbabilisticLoadOutput>)`
    async fn run_enhanced_predict(
        &self,
    ) -> Result<(Vec<f64>, Vec<f64>, Option<ProbabilisticLoadOutput>), AiEngineError> {
        // 尝试预测增强管线
        let pipeline_guard = self.prediction_pipeline.read().await;
        if let Some(ref pipeline) = *pipeline_guard {
            match pipeline.execute().await {
                Ok(result) => {
                    let level = result.enhancement_level;
                    tracing::debug!(
                        "增强预测完成: 等级={:?}({}), PV预测={}步, 负荷预测={}步",
                        level,
                        level.name(),
                        result.pv_forecast.len(),
                        result.load_forecast.len()
                    );
                    return Ok((
                        result.pv_forecast,
                        result.load_forecast,
                        result.load_quantiles,
                    ));
                }
                Err(e) => {
                    tracing::warn!("预测增强管线执行失败: {}，降级到基线 LSTM 路径", e);
                    // 继续执行基线路径
                }
            }
        }
        drop(pipeline_guard);

        // 降级：基线 LSTM 路径
        self.run_lstm_predict_with_quantiles().await
    }

    /// v1.0: 获取当前增强等级（用于监控/日志）
    ///
    /// 若增强管线未启用，返回 None。
    pub async fn enhancement_level(&self) -> Option<EnhancementLevel> {
        let pipeline = self.prediction_pipeline.read().await;
        pipeline.as_ref().map(|p| p.current_level())
    }

    /// v2.0: 获取当前增强等级名称字符串
    pub async fn enhancement_level_name(&self) -> Option<&'static str> {
        self.enhancement_level().await.map(|l| l.name())
    }

    /// v2.0: 加载误差修正模型
    ///
    /// 在 PredictionPipeline 创建后、首次推理前调用。
    /// 若误差修正模型文件不存在或校验失败，记录 WARN 并禁用误差修正。
    ///
    /// # 参数
    ///
    /// - `ec_model_path`: 误差修正 .rknn 模型文件路径
    /// - `expected_sha256`: SHA256 期望值（None 时跳过校验）
    pub async fn load_error_correction_model(
        &self,
        ec_model_path: &std::path::Path,
        expected_sha256: Option<&str>,
    ) -> Result<(), AiEngineError> {
        use crate::model_validator::{validate_rknn_model, PredictionModelType};

        // 校验模型文件
        validate_rknn_model(ec_model_path, PredictionModelType::ErrorCorrection, expected_sha256)?;

        // 加载 EC Runtime（通过 PredictionPipeline 内部管理）
        // 当前阶段：EC Runtime 在 PredictionPipeline::new() 中已创建。
        // 若需要热加载（OTA 升级后），可在此处重新创建 Runtime。

        tracing::info!(
            "误差修正模型加载完成: path={}",
            ec_model_path.display()
        );
        Ok(())
    }

    /// v2.0: BiLSTM 模型选择逻辑
    ///
    /// 根据双重门控（`enabled` AND `gate_passed`）决定加载哪个模型。
    ///
    /// # 返回值
    ///
    /// - `true` — Go 路径，加载 BiLSTM 模型
    /// - `false` — No-Go 路径，回退单向 LSTM
    pub async fn select_bilstm_model(&self) -> bool {
        let pipeline = self.prediction_pipeline.read().await;
        if let Some(ref p) = *pipeline {
            let cfg = p.config();
            let bilstm_go = cfg.bilstm.enabled && cfg.bilstm.gate_passed;

            if cfg.bilstm.enabled && !cfg.bilstm.gate_passed {
                tracing::warn!(
                    "BiLSTM 配置启用但 gate_passed=false（未通过 RK3588 延迟摸底），回退到单向 LSTM"
                );
            }

            bilstm_go
        } else {
            false
        }
    }

    /// v2.0: 模型热切换：从 BiLSTM 降级到单向 LSTM
    ///
    /// BiLSTM 推理连续失败后调用此方法。
    /// 降级通过修改 PipelineHealth 中的等级实现，不涉及模型文件重新加载。
    ///
    /// # 恢复条件
    ///
    /// 运维修复后手动设 `gate_passed=true` 并重启（或 OTA 下发新版 `bilstm_attn.rknn`）。
    pub async fn degrade_bilstm_to_lstm(&self) {
        let pipeline = self.prediction_pipeline.read().await;
        if let Some(ref p) = *pipeline {
            let mut health = p.health_write().await;
            if health.current_level == EnhancementLevel::FullVmdAttentionCorrection
                || health.current_level == EnhancementLevel::BiLstmVmdAttention
            {
                tracing::warn!(
                    "BiLSTM 模型降级: {:?} → VmdAttention (回退单向LSTM)",
                    health.current_level
                );
                health.current_level = EnhancementLevel::VmdAttention;
                health.bilstm_consecutive_failures = 0;
                health.bilstm_consecutive_successes = 0;
            }
        }
    }

    /// v2.0: 获取 BiLSTM 准入状态
    ///
    /// 返回 `(enabled, gate_passed)` 用于监控/日志。
    pub async fn bilstm_gate_status(&self) -> Option<(bool, bool)> {
        let pipeline = self.prediction_pipeline.read().await;
        pipeline.as_ref().map(|p| {
            let cfg = p.config();
            (cfg.bilstm.enabled, cfg.bilstm.gate_passed)
        })
    }

    /// v2.0: 获取误差修正启用状态
    pub async fn error_correction_status(&self) -> Option<bool> {
        let pipeline = self.prediction_pipeline.read().await;
        pipeline
            .as_ref()
            .map(|p| p.config().error_correction.enabled)
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
            .quantiles
            .iter()
            .map(|q| q.value as f64)
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
                input_features: 1,
                yesterday_offset_steps: 96,
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
