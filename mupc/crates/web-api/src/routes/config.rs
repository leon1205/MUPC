//! 配置管理 API

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use mupc_common::MupcError;

/// 应用状态
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub intercore: IntercoreConfig,
    #[serde(default)]
    pub system: SystemConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_listen_port")]
    pub listen_port: u16,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
}

fn default_listen_port() -> u16 {
    2404
}
fn default_heartbeat_interval() -> u64 {
    10
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            listen_port: default_listen_port(),
            heartbeat_interval_secs: default_heartbeat_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntercoreConfig {
    #[serde(default = "default_intercore_port")]
    pub listen_port: u16,
    #[serde(default)]
    pub remote_addr: String,
    #[serde(default = "default_intercore_remote_port")]
    pub remote_port: u16,
}

fn default_intercore_port() -> u16 {
    2500
}
fn default_intercore_remote_port() -> u16 {
    2501
}

impl Default for IntercoreConfig {
    fn default() -> Self {
        Self {
            listen_port: default_intercore_port(),
            remote_addr: "0.0.0.0".to_string(),
            remote_port: default_intercore_remote_port(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            gateway: GatewayConfig {
                listen_port: default_listen_port(),
                heartbeat_interval_secs: default_heartbeat_interval(),
            },
            intercore: IntercoreConfig {
                listen_port: default_intercore_port(),
                remote_addr: "0.0.0.0".to_string(),
                remote_port: default_intercore_remote_port(),
            },
            system: SystemConfig {
                log_level: default_log_level(),
            },
        }
    }
}

/// 配置处理器
#[derive(Clone)]
pub struct ConfigHandler {
    state: AppState,
}

impl Default for ConfigHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigHandler {
    pub fn new() -> Self {
        Self {
            state: AppState {
                config: Arc::new(RwLock::new(AppConfig::default())),
            },
        }
    }

    /// 获取当前配置
    pub async fn get_config(&self) -> Result<AppConfig, MupcError> {
        let config = self.state.config.read().await;
        Ok(config.clone())
    }

    /// 更新配置
    pub async fn update_config(&self, new_config: AppConfig) -> Result<(), MupcError> {
        // 验证配置
        if new_config.gateway.heartbeat_interval_secs < 1
            || new_config.gateway.heartbeat_interval_secs > 60
        {
            return Err(MupcError::new(
                mupc_common::ErrorCode::InvalidParam,
                "heartbeat_interval must be between 1 and 60 seconds",
                "web-api",
            ));
        }

        let mut config = self.state.config.write().await;
        *config = new_config;

        Ok(())
    }
}

/// GET /api/v1/config - 获取配置
async fn get_config(State(handler): State<ConfigHandler>) -> Result<Json<AppConfig>, StatusCode> {
    handler
        .get_config()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// PUT /api/v1/config - 更新配置
async fn update_config(
    State(handler): State<ConfigHandler>,
    Json(new_config): Json<AppConfig>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    handler
        .update_config(new_config)
        .await
        .map(|_| Json(serde_json::json!({ "status": "ok" })))
        .map_err(|e| {
            tracing::error!("Config update error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

/// 创建配置路由
pub fn create_router(handler: ConfigHandler) -> Router {
    Router::new()
        .route("/api/v1/config", get(get_config).put(update_config))
        .with_state(handler)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== AppConfig Default Tests ==========

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();

        assert_eq!(config.gateway.listen_port, 2404);
        assert_eq!(config.gateway.heartbeat_interval_secs, 10);
        assert_eq!(config.intercore.listen_port, 2500);
        assert_eq!(config.intercore.remote_addr, "0.0.0.0");
        assert_eq!(config.intercore.remote_port, 2501);
        assert_eq!(config.system.log_level, "info");
    }

    #[test]
    fn test_gateway_config_defaults() {
        assert_eq!(default_listen_port(), 2404);
        assert_eq!(default_heartbeat_interval(), 10);
    }

    #[test]
    fn test_intercore_config_defaults() {
        assert_eq!(default_intercore_port(), 2500);
        assert_eq!(default_intercore_remote_port(), 2501);
    }

    #[test]
    fn test_system_config_default() {
        assert_eq!(default_log_level(), "info");
    }

    // ========== ConfigHandler Tests ==========

    #[tokio::test]
    async fn test_config_handler_get_config() {
        let handler = ConfigHandler::new();
        let config = handler.get_config().await.unwrap();

        assert_eq!(config.gateway.listen_port, 2404);
    }

    #[tokio::test]
    async fn test_config_handler_update_config_success() {
        let handler = ConfigHandler::new();

        let new_config = AppConfig {
            gateway: GatewayConfig {
                listen_port: 2405,
                heartbeat_interval_secs: 30,
            },
            intercore: IntercoreConfig {
                listen_port: 2501,
                remote_addr: "192.168.1.100".to_string(),
                remote_port: 2502,
            },
            system: SystemConfig {
                log_level: "debug".to_string(),
            },
        };

        let result = handler.update_config(new_config.clone()).await;
        assert!(result.is_ok());

        let config = handler.get_config().await.unwrap();
        assert_eq!(config.gateway.listen_port, 2405);
        assert_eq!(config.gateway.heartbeat_interval_secs, 30);
        assert_eq!(config.intercore.remote_addr, "192.168.1.100");
    }

    #[tokio::test]
    async fn test_config_handler_update_config_invalid_heartbeat_low() {
        let handler = ConfigHandler::new();

        let new_config = AppConfig {
            gateway: GatewayConfig {
                listen_port: 2404,
                heartbeat_interval_secs: 0, // 无效，太小
            },
            ..Default::default()
        };

        let result = handler.update_config(new_config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_handler_update_config_invalid_heartbeat_high() {
        let handler = ConfigHandler::new();

        let new_config = AppConfig {
            gateway: GatewayConfig {
                listen_port: 2404,
                heartbeat_interval_secs: 100, // 无效，太大
            },
            ..Default::default()
        };

        let result = handler.update_config(new_config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_handler_update_config_boundary_values() {
        let handler = ConfigHandler::new();

        // 测试下限边界 (1)
        let new_config = AppConfig {
            gateway: GatewayConfig {
                listen_port: 2404,
                heartbeat_interval_secs: 1,
            },
            ..Default::default()
        };
        let result = handler.update_config(new_config).await;
        assert!(result.is_ok());

        // 测试上限边界 (60)
        let new_config = AppConfig {
            gateway: GatewayConfig {
                listen_port: 2404,
                heartbeat_interval_secs: 60,
            },
            ..Default::default()
        };
        let result = handler.update_config(new_config).await;
        assert!(result.is_ok());
    }
}
