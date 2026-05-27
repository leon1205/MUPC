//! MUPC 策略引擎模块
//!
//! Phase 1 仅定义接口

pub mod strategies;

pub use strategies::{FallbackStrategy, AiCommandValidator, StrategyType, ControlCommand, ValidationResult};