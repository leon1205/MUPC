//! AI 优化引擎模块
//!
//! Phase 3C 实现：
//! - LSTM 时序预测
//! - MADDPG/PPO 强化学习决策（v2.3: 5 个场景独立模型）
//! - RKNN Runtime 推理（RK3588 NPU）
//! - 多源数据融合（DataFusionEngine）
//! - 动作约束校验（ActionValidator）
//! - 奖励函数计算（RewardCalculator）
//! - 模型注册表（v2.3: ModelRegistry + 双缓冲热切换）
//! - v2.5 动作空间参数可配置化（ActionSpaceConfig + ConfigLoader）
//! - v1.0 预测增强管线（VMD + Attention + 预测管线编排）

pub mod action_space;
pub mod action_validator;
pub mod adaptive_weight_optimizer;
pub mod config;
pub mod config_loader;
pub mod data_fusion;
pub mod dynamic_config_loader;
pub mod env_config;
pub mod error;
pub mod load_covariates;
pub mod lstm_model;
pub mod mode_selector;
pub mod model_manager;
pub mod model_registry;
pub mod online_updater;
pub mod pareto_optimizer;
pub mod performance_collector;
pub mod model_validator;
pub mod pipeline_config;
pub mod prediction_pipeline;
pub mod residual_buffer;
pub mod reward_calculator;
pub mod reward_normalizer;
pub mod rknn_runtime;
pub mod rknn_runtime_sys;
pub mod rknn_types;
pub mod rl_model;
pub mod robustness_manager;
pub mod safety_config;
pub mod safety_wrapper;
pub mod vmd;

pub use action_space::ActionSpaceConfig;
pub use action_validator::{ActionValidator, ViolationRecord};
pub use adaptive_weight_optimizer::{
    AdaptiveWeightOptimizer, HistoricalPerformance, PerformanceCollector, PerformanceFeatures,
    WeightAdjustment,
};
pub use config::{
    ActionConstraintConfig, AdaptiveOptimizerConfig, AiEngineConfig, FusionConfig, LstmConfig,
    ModeConfig, ModelType, NpuConfig, OnlineUpdateConfig, ParetoOptimizerConfig, QuantizationType,
    RlAlgorithm, RlConfig, SafetyWrapperConfig, SceneWeights, WeightBounds, WeightConstraints,
};
pub use config_loader::ConfigLoader;
pub use data_fusion::{
    normalize_observation, validate_input_vector, DataFusionEngine, DataSourceAdapter,
    FusedSystemState, HealthStatus, SourceHealth, SourceType,
};
pub use dynamic_config_loader::DynamicConfigLoader;
pub use env_config::{EnvConfig, EnvConfigMetadata, OperationalConfig, PhysicalConfig};
pub use error::AiEngineError;
pub use load_covariates::{
    DataFusionWeatherAdapter, DefaultWeatherService, LoadCovariates, WeatherService,
};
pub use lstm_model::{
    LstmInput, LstmModel, LstmOutput, ProbabilisticLoadOutput, QuantilePrediction,
};
pub use mode_selector::{
    parse_mode_name, DualStrategyHead, DualStrategyState, ModeSelector, ModeSwitchEvent,
    RunningMode, SwitchSource,
};
pub use model_manager::{HistorySample, ModelManager, ModelStatus};
pub use model_registry::{ModelManifestEntry, ModelRegistry, SceneModelState, SceneSwitchResult};
pub use online_updater::{DataPoint, OnlineUpdater};
pub use pareto_optimizer::{
    OptimizationObjective, ParetoSolution, ParetoWeightOptimizer, WeightCandidate,
};
pub use performance_collector::PerformanceCollectorImpl;
pub use reward_calculator::RewardCalculator;
pub use reward_normalizer::{NormalizedReward, RewardNormalizer, RunningStats};
pub use rknn_runtime::RknnRuntime;
pub use rl_model::{parse_action_output, ActionOutput, RLModel, SystemState};
pub use robustness_manager::{AnomalyType, RobustnessManager};
pub use safety_config::SafetyConfig;
pub use safety_wrapper::{
    CheckResult, LineImpedance, LinearSensitivityPredictor, PredictionResult, SafetyBounds,
    SafetyEventSender, SafetyEventType, SafetyRLWrapper, SafetyStats, SafetyViolation,
    SafetyWrapperEvent,
};

// --- v1.0 预测增强管线 re-exports ---

pub use model_validator::{
    validate_model_type_consistency, validate_rknn_model, PredictionModelType,
};
pub use pipeline_config::{
    AttentionConfig, AttentionScoreType, BiLstmConfig, EnhancementLevel, ErrorCorrectionConfig,
    FeatureSelectionConfig, PipelineHealth, PredictionEnhancementConfig, VmdEnhancementConfig,
};
pub use prediction_pipeline::{EnhancedForecastResult, PredictionPipeline};
pub use residual_buffer::ResidualBuffer;
pub use vmd::{VmdConfig, VmdDecomposer, VmdResult};
