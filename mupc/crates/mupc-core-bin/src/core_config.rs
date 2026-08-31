//! 主配置文件 `mupc_core_config.yaml` 结构定义
//!
//! 定义 mupcd 守护进程的完整配置结构，包括系统参数、
//! 核间通信、Web API、AI 引擎和插件配置。

use serde::Deserialize;
use std::path::PathBuf;

/// 主配置文件顶层结构
#[derive(Debug, Clone, Deserialize)]
pub struct CoreConfig {
    /// 配置版本号（用于兼容性校验）
    pub version: String,
    /// 系统级配置
    pub system: SystemConfig,
    /// 核间通信配置
    pub intercore: InterCoreConfig,
    /// Web API 配置
    pub web_api: WebApiConfig,
    /// AI 引擎配置
    pub ai_engine: AiEngineConfig,
    /// 插件配置
    pub plugins: PluginsConfig,
}

/// 系统级配置
#[derive(Debug, Clone, Deserialize)]
pub struct SystemConfig {
    /// 日志级别: "info" / "debug" / "warn" / "error"
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// 日志输出目录
    #[serde(default = "default_log_dir")]
    pub log_dir: PathBuf,
    /// 持久化数据目录
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// 插件搜索目录
    #[serde(default = "default_plugin_dir")]
    pub plugin_dir: PathBuf,
    /// TLS 证书目录
    #[serde(default = "default_cert_dir")]
    pub cert_dir: PathBuf,
    /// 优雅退出超时（秒），默认 30
    #[serde(default = "default_shutdown_timeout_sec")]
    pub shutdown_timeout_sec: u64,
}

/// 核间通信配置（与实时核心 TCP 连接）
#[derive(Debug, Clone, Deserialize)]
pub struct InterCoreConfig {
    /// 实时核心 IP 地址
    #[serde(default = "default_intercore_host")]
    pub host: String,
    /// 实时核心端口，默认 9100
    #[serde(default = "default_intercore_port")]
    pub port: u16,
    /// 心跳间隔（秒），默认 5
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_sec: u64,
    /// 重连间隔（秒），默认 3
    #[serde(default = "default_reconnect_interval")]
    pub reconnect_interval_sec: u64,
}

/// Web API 配置
#[derive(Debug, Clone, Deserialize)]
pub struct WebApiConfig {
    /// 监听地址，如 "0.0.0.0:8080"
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    /// 是否启用 HTTPS（Phase 2+）
    #[serde(default = "default_enable_https")]
    pub enable_https: bool,
    /// TLS 证书路径
    pub tls_cert: Option<PathBuf>,
    /// TLS 私钥路径
    pub tls_key: Option<PathBuf>,
}

/// AI 引擎配置
#[derive(Debug, Clone, Deserialize)]
pub struct AiEngineConfig {
    /// 模型文件目录
    #[serde(default = "default_model_dir")]
    pub model_dir: PathBuf,
    /// AI 引擎配置文件路径（mupc_env_config.yaml）
    #[serde(default = "default_env_config_file")]
    pub config_file: PathBuf,
    /// 是否启用 NPU
    #[serde(default = "default_enable_npu")]
    pub enable_npu: bool,
    /// 推理超时（毫秒），默认 500
    #[serde(default = "default_inference_timeout_ms")]
    pub inference_timeout_ms: u64,
    /// 本地策略优先模式（默认 false = AI 优先；true = 本地台区储能治理策略优先，AI 旁路）
    #[serde(default = "default_local_priority")]
    pub local_priority: bool,
}

/// 插件配置
#[derive(Debug, Clone, Deserialize)]
pub struct PluginsConfig {
    /// 插件搜索路径
    #[serde(default = "default_plugin_search_paths")]
    pub search_paths: Vec<PathBuf>,
    /// 自动加载的插件名列表
    #[serde(default = "default_auto_load")]
    pub auto_load: Vec<String>,
}

// ── 默认值函数 ──

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/logs")
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/data")
}

fn default_plugin_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/lib/plugins")
}

fn default_cert_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/certs")
}

fn default_shutdown_timeout_sec() -> u64 {
    30
}

fn default_intercore_host() -> String {
    "127.0.0.1".to_string()
}

fn default_intercore_port() -> u16 {
    9100
}

fn default_heartbeat_interval() -> u64 {
    5
}

fn default_reconnect_interval() -> u64 {
    3
}

fn default_listen_addr() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_enable_https() -> bool {
    false
}

fn default_model_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/models")
}

fn default_env_config_file() -> PathBuf {
    PathBuf::from("/opt/mupc/config/mupc_env_config.yaml")
}

fn default_enable_npu() -> bool {
    true
}

fn default_inference_timeout_ms() -> u64 {
    500
}

fn default_local_priority() -> bool {
    false
}

fn default_plugin_search_paths() -> Vec<PathBuf> {
    vec![PathBuf::from("/opt/mupc/lib/plugins")]
}

fn default_auto_load() -> Vec<String> {
    vec![
        "rs485_plugin".to_string(),
        "hplc_plugin".to_string(),
        "mqtt_plugin".to_string(),
    ]
}

impl CoreConfig {
    /// 从 YAML 文件加载配置
    pub fn load(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: CoreConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// 校验配置完整性
    pub fn validate(&self) -> Result<(), String> {
        if self.version.is_empty() {
            return Err("version 字段不能为空".to_string());
        }
        if self.system.log_level.is_empty() {
            return Err("system.log_level 不能为空".to_string());
        }
        if self.intercore.host.is_empty() {
            return Err("intercore.host 不能为空".to_string());
        }
        if self.intercore.port == 0 {
            return Err("intercore.port 不能为 0".to_string());
        }
        if self.web_api.listen_addr.is_empty() {
            return Err("web_api.listen_addr 不能为空".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_config_deserialize_minimal() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "debug"
intercore:
  host: "192.168.1.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:9000"
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.version, "1.0");
        assert_eq!(config.system.log_level, "debug");
        assert_eq!(config.intercore.host, "192.168.1.1");
        assert_eq!(config.web_api.listen_addr, "0.0.0.0:9000");
        // 默认值校验
        assert_eq!(config.system.shutdown_timeout_sec, 30);
        assert_eq!(config.ai_engine.model_dir, PathBuf::from("/opt/mupc/models"));
        assert_eq!(config.intercore.heartbeat_interval_sec, 5);
    }

    #[test]
    fn test_core_config_validate_success() {
        let config = CoreConfig {
            version: "1.0".into(),
            system: SystemConfig {
                log_level: "info".into(),
                log_dir: PathBuf::from("/tmp/logs"),
                data_dir: PathBuf::from("/tmp/data"),
                plugin_dir: PathBuf::from("/tmp/plugins"),
                cert_dir: PathBuf::from("/tmp/certs"),
                shutdown_timeout_sec: 30,
            },
            intercore: InterCoreConfig {
                host: "127.0.0.1".into(),
                port: 9100,
                heartbeat_interval_sec: 5,
                reconnect_interval_sec: 3,
            },
            web_api: WebApiConfig {
                listen_addr: "0.0.0.0:8080".into(),
                enable_https: false,
                tls_cert: None,
                tls_key: None,
            },
            ai_engine: AiEngineConfig {
                model_dir: PathBuf::from("/tmp/models"),
                config_file: PathBuf::from("/tmp/config.yaml"),
                enable_npu: true,
                inference_timeout_ms: 500,
                local_priority: false,
            },
            plugins: PluginsConfig {
                search_paths: vec![PathBuf::from("/tmp/plugins")],
                auto_load: vec!["rs485_plugin".into()],
            },
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_core_config_validate_empty_version() {
        let config = CoreConfig {
            version: "".into(),
            system: SystemConfig {
                log_level: "info".into(),
                log_dir: PathBuf::from("/tmp"),
                data_dir: PathBuf::from("/tmp"),
                plugin_dir: PathBuf::from("/tmp"),
                cert_dir: PathBuf::from("/tmp"),
                shutdown_timeout_sec: 30,
            },
            intercore: InterCoreConfig {
                host: "127.0.0.1".into(),
                port: 9100,
                heartbeat_interval_sec: 5,
                reconnect_interval_sec: 3,
            },
            web_api: WebApiConfig {
                listen_addr: "0.0.0.0:8080".into(),
                enable_https: false,
                tls_cert: None,
                tls_key: None,
            },
            ai_engine: AiEngineConfig {
                model_dir: PathBuf::from("/tmp"),
                config_file: PathBuf::from("/tmp"),
                enable_npu: false,
                inference_timeout_ms: 500,
                local_priority: false,
            },
            plugins: PluginsConfig {
                search_paths: vec![],
                auto_load: vec![],
            },
        };
        assert!(config.validate().is_err());
    }
}
