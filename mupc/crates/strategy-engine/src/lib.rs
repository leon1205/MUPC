//! MUPC 策略引擎模块
//!
//! 精简为单一兜底策略：台区储能治理（AI 失效时经核间下发分相 P/Q）。
//! 原三策略（削峰填谷/需量控制/防逆流）已废弃（文件保留于 src/ 但不再编译）。

pub mod ai_validator;
pub mod config;
pub mod errors;
pub mod south_command_sender;
pub mod strategies;
pub mod tai_storage;

// AI Engine integration
pub mod ai_integration;

pub use ai_integration::{AiEngineStatusInfo, AiIntegrator, ModeInfo};
pub use ai_validator::{AiCommandValidatorImpl, AiModel, MockAiModel, ModelInput, ModelOutput};
pub use config::TaiStorageConfig;
pub use errors::StrategyError;
pub use south_command_sender::{
    get_dispatcher, set_dispatcher, LoadSheddingCommand, MockSouthCommandSender, PvLimitCommand,
    SouthCommandDispatcher, SouthCommandSender, SouthCommandType, SouthSendResult,
};
pub use strategies::{
    AiCommandValidator, CommandType, ControlCommand, FallbackStrategy, StrategyType,
    ValidationResult,
};
pub use tai_storage::{control, MeterData, TaiControllerState, TaiState, TaiStorageStrategy};

// AI Engine re-exports
pub use mupc_ai_engine::{ModelManager, ModelStatus};

#[cfg(test)]
mod ai_validator_test;
#[cfg(test)]
mod south_command_sender_test;
#[cfg(test)]
mod tai_storage_test;
