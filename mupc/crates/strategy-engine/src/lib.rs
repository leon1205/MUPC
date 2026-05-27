//! MUPC 策略引擎模块
//!
//! Phase 1 仅定义接口

pub mod config;
pub mod errors;
pub mod peak_shaving;
pub mod strategies;

pub use config::{AntiReverseConfig, DemandControlConfig, PeakShavingConfig};
pub use errors::StrategyError;
pub use peak_shaving::PeakShavingStrategy;
pub use strategies::{AiCommandValidator, CommandType, ControlCommand, FallbackStrategy, StrategyType, ValidationResult};

#[cfg(test)]
mod peak_shaving_test;