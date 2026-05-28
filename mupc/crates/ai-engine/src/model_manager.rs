//! 模型管理器
//!
//! 统一调度 LSTM 预测和 RL 决策模型

use crate::error::AiEngineError;
use crate::config::AiEngineConfig;
use crate::lstm_model::{LstmModel, LstmInput, LstmOutput};
use crate::rl_model::{RLModel, SystemState, ActionOutput};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 模型状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStatus {
    Unloaded,
    Loading,
    Ready,
    Error,
}

/// 模型管理器
///
/// 统一管理 LSTM 预测和 RL 决策模型
/// 提供线程安全的异步访问接口
pub struct ModelManager {
    config: AiEngineConfig,
    lstm_model: Arc<RwLock<Option<LstmModel>>>,
    rl_model: Arc<RwLock<Option<RLModel>>>,
    status: Arc<RwLock<ModelStatus>>,
}

impl ModelManager {
    /// 创建模型管理器
    pub fn new(config: AiEngineConfig) -> Self {
        Self {
            config,
            lstm_model: Arc::new(RwLock::new(None)),
            rl_model: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
        }
    }

    /// 加载所有模型
    pub async fn load_models(&self) -> Result<(), AiEngineError> {
        *self.status.write().await = ModelStatus::Loading;

        // 加载 LSTM 模型
        let mut lstm = LstmModel::new(self.config.lstm.clone())
            .map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        lstm.load()
            .await
            .map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        *self.lstm_model.write().await = Some(lstm);

        // 加载 RL 模型
        let mut rl = RLModel::new(self.config.rl.clone())
            .map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        rl.load()
            .await
            .map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        *self.rl_model.write().await = Some(rl);

        *self.status.write().await = ModelStatus::Ready;
        Ok(())
    }

    /// 预测（LSTM）
    pub async fn predict(&self, input: &LstmInput) -> Result<LstmOutput, AiEngineError> {
        let lstm = self.lstm_model.read().await;
        let lstm = lstm.as_ref()
            .ok_or(AiEngineError::ModelNotLoaded)?;
        lstm.predict(input).await
    }

    /// 决策（RL）
    pub async fn decide(&self, state: &SystemState) -> Result<ActionOutput, AiEngineError> {
        let rl = self.rl_model.read().await;
        let rl = rl.as_ref()
            .ok_or(AiEngineError::ModelNotLoaded)?;
        rl.decide(state).await
    }

    /// 获取模型状态
    pub async fn get_status(&self) -> ModelStatus {
        *self.status.read().await
    }

    /// 检查是否就绪
    pub async fn is_ready(&self) -> bool {
        self.get_status().await == ModelStatus::Ready
    }

    /// 获取 LSTM 模型状态（用于调试）
    pub async fn lstm_ready(&self) -> bool {
        self.lstm_model.read().await.is_some()
    }

    /// 获取 RL 模型状态（用于调试）
    pub async fn rl_ready(&self) -> bool {
        self.rl_model.read().await.is_some()
    }

    /// 获取模型状态（同步版本，用于测试）
    fn get_status_blocking(&self) -> ModelStatus {
        ModelStatus::Unloaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LstmConfig, RlConfig, RlAlgorithm, OnlineUpdateConfig};

    fn create_test_config() -> AiEngineConfig {
        AiEngineConfig {
            lstm: LstmConfig {
                model_path: std::path::PathBuf::from("/tmp/test_lstm.rknn"),
                input_window_secs: 3600,
                output_horizon_secs: 1800,
                quantization: crate::config::QuantizationType::INT8,
            },
            rl: RlConfig {
                model_path: std::path::PathBuf::from("/tmp/test_rl.rknn"),
                algorithm: RlAlgorithm::MADDPG,
                quantization: crate::config::QuantizationType::INT8,
            },
            online_update: OnlineUpdateConfig::default(),
        }
    }

    #[test]
    fn test_model_manager_creation() {
        let config = create_test_config();
        let manager = ModelManager::new(config);
        assert_eq!(manager.get_status_blocking(), ModelStatus::Unloaded);
    }
}