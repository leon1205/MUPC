//! MUPC 策略引擎模块
//!
//! Phase 3C: 集成 AI 优化引擎

pub mod ai_validator;
pub mod anti_reverse;
pub mod config;
pub mod demand_control;
pub mod errors;
pub mod peak_shaving;
pub mod south_command_sender;
pub mod strategies;

// AI Engine integration
pub mod ai_integration;

pub use ai_integration::{AiEngineStatusInfo, AiIntegrator, ModeInfo};
pub use ai_validator::{AiCommandValidatorImpl, AiModel, MockAiModel, ModelInput, ModelOutput};
pub use anti_reverse::AntiReverseStrategy;
pub use config::{AntiReverseConfig, DemandControlConfig, PeakShavingConfig};
pub use demand_control::DemandControlStrategy;
pub use errors::StrategyError;
pub use peak_shaving::PeakShavingStrategy;
pub use south_command_sender::{
    get_dispatcher, set_dispatcher, LoadSheddingCommand, MockSouthCommandSender, PvLimitCommand,
    Rs485SouthSender, SouthCommandDispatcher, SouthCommandSender, SouthCommandType, SouthSendResult,
};
pub use strategies::{
    AiCommandValidator, CommandType, ControlCommand, FallbackStrategy, StrategyType,
    ValidationResult,
};

// AI Engine re-exports
pub use mupc_ai_engine::{ModelManager, ModelStatus};

#[cfg(test)]
mod ai_validator_test;
#[cfg(test)]
mod anti_reverse_test;
#[cfg(test)]
mod demand_control_test;
#[cfg(test)]
mod peak_shaving_test;
#[cfg(test)]
mod south_command_sender_test;
