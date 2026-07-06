//! 配置文件元数据与完整环境配置结构
//!
//! v2.6: 对齐训练管线 YAML 配置结构

use serde::{Deserialize, Serialize};

use super::safety_config::SafetyConfig;

/// 配置文件版本指纹
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvConfigMetadata {
    pub fingerprint: String,
    pub source: String,
}

impl Default for EnvConfigMetadata {
    fn default() -> Self {
        Self {
            fingerprint: "unknown".into(),
            source: "unknown".into(),
        }
    }
}

/// 物理常量配置（RL 核心参数）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalConfig {
    pub transformer_kva: f64,
    pub battery_capacity_kwh: f64,
    pub p_batt_max_kw: f64,
    pub load_shed_max_kw: f64,
}

impl Default for PhysicalConfig {
    fn default() -> Self {
        Self {
            transformer_kva: 200.0,
            battery_capacity_kwh: 100.0,
            p_batt_max_kw: 50.0,
            load_shed_max_kw: 60.0,
        }
    }
}

/// 操作调优参数配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalConfig {
    pub p_batt_ramp_limit_kw: f64,
    pub q_batt_ramp_limit_kvar: f64,
    pub pv_limit_min: f64,
}

impl Default for OperationalConfig {
    fn default() -> Self {
        Self {
            p_batt_ramp_limit_kw: 50.0,
            q_batt_ramp_limit_kvar: 30.0,
            pv_limit_min: 0.10,
        }
    }
}

/// 完整环境配置（YAML 结构）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvConfig {
    pub version: EnvConfigMetadata,
    pub physical: PhysicalConfig,
    pub safety: SafetyConfig,
    pub operational: OperationalConfig,
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            version: EnvConfigMetadata::default(),
            physical: PhysicalConfig::default(),
            safety: SafetyConfig::default(),
            operational: OperationalConfig::default(),
        }
    }
}

impl EnvConfig {
    /// 从 YAML 文件加载
    ///
    /// Phase 2+ 启用：需要 AiEngineError::ConfigLoadFailed 变体支持。
    /// 当前由 DynamicConfigLoader 接管 YAML 加载职责。
    #[allow(dead_code)]
    pub fn from_file(path: &std::path::PathBuf) -> Result<Self, crate::error::AiEngineError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| crate::error::AiEngineError::ConfigLoadFailed(format!("读取文件失败: {}", e)))?;
        serde_yaml::from_str(&content)
            .map_err(|e| crate::error::AiEngineError::ConfigLoadFailed(format!("YAML 解析失败: {}", e)))
    }

    /// 获取版本指纹
    pub fn fingerprint(&self) -> &str {
        &self.version.fingerprint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_env_config() {
        let cfg = EnvConfig::default();
        assert_eq!(cfg.physical.p_batt_max_kw, 50.0);
        assert_eq!(cfg.safety.soc_min, 0.10);
        assert_eq!(cfg.operational.p_batt_ramp_limit_kw, 50.0);
    }

    #[test]
    fn test_fingerprint() {
        let cfg = EnvConfig::default();
        assert_eq!(cfg.fingerprint(), "unknown");
    }
}
