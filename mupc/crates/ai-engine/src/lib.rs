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
pub mod reward_calculator;
pub mod rknn_runtime;
pub mod rknn_runtime_sys;
pub mod rknn_types;
pub mod rl_model;
pub mod robustness_manager;
pub mod safety_config;

pub use action_space::ActionSpaceConfig;
pub use action_validator::{ActionValidator, ViolationRecord};
pub use adaptive_weight_optimizer::{
    AdaptiveWeightOptimizer, HistoricalPerformance, PerformanceCollector, PerformanceFeatures,
    WeightAdjustment,
};
pub use config::{
    ActionConstraintConfig, AdaptiveOptimizerConfig, AiEngineConfig, FusionConfig, LstmConfig,
    ModeConfig, ModelType, NpuConfig, OnlineUpdateConfig, QuantizationType, RlAlgorithm, RlConfig,
    SceneWeights, WeightBounds, WeightConstraints, ParetoOptimizerConfig,
};
pub use config_loader::ConfigLoader;
pub use data_fusion::{
    validate_input_vector, DataFusionEngine, DataSourceAdapter, FusedSystemState, HealthStatus,
    SourceHealth, SourceType,
};
pub use dynamic_config_loader::DynamicConfigLoader;
pub use env_config::{EnvConfig, EnvConfigMetadata, OperationalConfig, PhysicalConfig};
pub use error::AiEngineError;
pub use load_covariates::{DataFusionWeatherAdapter, DefaultWeatherService, LoadCovariates, WeatherService};
pub use lstm_model::{LstmInput, LstmModel, LstmOutput, ProbabilisticLoadOutput, QuantilePrediction};
pub use mode_selector::{
    parse_mode_name, ModeSelector, ModeSwitchEvent, RunningMode, SwitchSource,
};
pub use model_manager::{ModelManager, ModelStatus};
pub use model_registry::{ModelManifestEntry, ModelRegistry, SceneModelState, SceneSwitchResult};
pub use online_updater::{DataPoint, OnlineUpdater};
pub use pareto_optimizer::{OptimizationObjective, ParetoSolution, ParetoWeightOptimizer, WeightCandidate};
pub use performance_collector::PerformanceCollectorImpl;
pub use reward_calculator::RewardCalculator;
pub use rknn_runtime::RknnRuntime;
pub use rl_model::{parse_action_output, ActionOutput, RLModel, SystemState};
pub use robustness_manager::{AnomalyType, RobustnessManager};
pub use safety_config::SafetyConfig;
