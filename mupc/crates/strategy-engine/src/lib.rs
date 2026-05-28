//! MUPC 策略引擎模块
//!
//! Phase 3C: 集成 AI 优化引擎

pub mod strategies;
pub mod peak_shaving;
pub mod demand_control;
pub mod anti_reverse;
pub mod ai_validator;
pub mod config;
pub mod errors;

// AI Engine integration
pub mod ai_integration;

pub use peak_shaving::PeakShavingStrategy;
pub use demand_control::DemandControlStrategy;
pub use anti_reverse::AntiReverseStrategy;
pub use ai_validator::{AiCommandValidatorImpl, AiModel, ModelInput, ModelOutput, MockAiModel};
pub use config::{PeakShavingConfig, DemandControlConfig, AntiReverseConfig};
pub use errors::StrategyError;
pub use strategies::{FallbackStrategy, AiCommandValidator, StrategyType, ControlCommand, CommandType, ValidationResult};

// AI Engine re-exports
pub use mupc_ai_engine::{ModelManager, ModelStatus, LstmModel, RLModel, SystemState, ActionOutput, LstmInput, LstmOutput};

#[cfg(test)]
mod strategies_test;
#[cfg(test)]
mod peak_shaving_test;
#[cfg(test)]
mod demand_control_test;
#[cfg(test)]
mod anti_reverse_test;
#[cfg(test)]
mod ai_validator_test;
#[cfg(test)]
mod config_test;