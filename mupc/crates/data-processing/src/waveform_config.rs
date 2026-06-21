//! 波形触发配置
//!
//! 集中定义故障录波的触发条件配置结构体，
//! 供 `waveform::trigger` 子模块和上层模块使用。
//!
//! # Re-export
//!
//! 本模块将 `waveform::trigger` 中的核心配置类型重新导出，
//! 提供统一的导入路径 `crate::waveform_config::TriggerConfig`。

pub use crate::waveform::trigger::{ChannelMask, TriggerConfig, TriggerResult};

/// 获取默认触发配置
pub fn default_trigger_config() -> TriggerConfig {
    TriggerConfig::default()
}

/// 获取测试用触发配置（防抖+冷却最小化）
#[cfg(test)]
pub fn test_trigger_config() -> TriggerConfig {
    let mut config = TriggerConfig::default();
    config.debounce_samples = 1;
    config.cool_down_ms = 0;
    config.enabled = true;
    config
}
