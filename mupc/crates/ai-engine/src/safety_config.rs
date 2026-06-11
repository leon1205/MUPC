//! 安全约束配置
//!
//! v2.6: 对齐训练管线 safety 配置

use serde::{Deserialize, Serialize};

/// 安全约束配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    /// SOC 下限硬约束（低于此值强制停机）
    pub soc_min: f64,
    /// SOC 上限硬约束（高于此值停止充电）
    pub soc_max: f64,
    /// 变压器过载阈值（额定容量百分比）
    pub overload_threshold: f64,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            soc_min: 0.10,
            soc_max: 0.90,
            overload_threshold: 0.85,
        }
    }
}

impl SafetyConfig {
    /// 校验安全约束有效性
    pub fn validate(&self) -> Result<(), String> {
        if self.soc_min <= 0.0 || self.soc_min >= 1.0 {
            return Err("soc_min must be in (0, 1)".into());
        }
        if self.soc_max <= 0.0 || self.soc_max >= 1.0 {
            return Err("soc_max must be in (0, 1)".into());
        }
        if self.soc_min >= self.soc_max {
            return Err("soc_min must be less than soc_max".into());
        }
        if self.overload_threshold <= 0.0 || self.overload_threshold > 1.0 {
            return Err("overload_threshold must be in (0, 1]".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_safety_config() {
        let cfg = SafetyConfig::default();
        assert_eq!(cfg.soc_min, 0.10);
        assert_eq!(cfg.soc_max, 0.90);
        assert_eq!(cfg.overload_threshold, 0.85);
    }

    #[test]
    fn test_validate_valid() {
        let cfg = SafetyConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_validate_soc_min_ge_soc_max() {
        let cfg = SafetyConfig {
            soc_min: 0.5,
            soc_max: 0.3,
            overload_threshold: 0.85,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_validate_overload_out_of_range() {
        let cfg = SafetyConfig {
            soc_min: 0.10,
            soc_max: 0.90,
            overload_threshold: 1.5,
        };
        assert!(cfg.validate().is_err());
    }
}