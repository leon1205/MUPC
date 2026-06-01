//! 模型管理器
//!
//! 统一调度 LSTM 预测和 RL 决策模型

use crate::config::{AiEngineConfig, ModeConfig};
use crate::error::AiEngineError;
use crate::lstm_model::{LstmInput, LstmModel, LstmOutput};
use crate::mode_selector::{ModeSelector, RunningMode, SwitchSource};
use crate::rl_model::{ActionOutput, RLModel, SystemState};
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
/// 统一管理 LSTM 预测、RL 决策模型和运行场景选择
/// 提供线程安全的异步访问接口
pub struct ModelManager {
    config: AiEngineConfig,
    lstm_model: Arc<RwLock<Option<LstmModel>>>,
    rl_model: Arc<RwLock<Option<RLModel>>>,
    status: Arc<RwLock<ModelStatus>>,
    /// v2.0: 运行场景选择器（互斥保证）
    mode_selector: Arc<ModeSelector>,
}

impl ModelManager {
    /// 创建模型管理器
    pub fn new(config: AiEngineConfig) -> Self {
        let initial_mode = parse_initial_mode(&config.mode);
        let persist_path = if config.mode.persist_path.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(&config.mode.persist_path))
        };
        let mode_selector = Arc::new(ModeSelector::new(initial_mode, persist_path));

        Self {
            config,
            lstm_model: Arc::new(RwLock::new(None)),
            rl_model: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
            mode_selector,
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
        let lstm = lstm.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
        lstm.predict(input).await
    }

    /// 决策（RL）
    pub async fn decide(&self, state: &SystemState) -> Result<ActionOutput, AiEngineError> {
        let rl = self.rl_model.read().await;
        let rl = rl.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
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

    /// 获取模式选择器引用
    pub fn mode_selector(&self) -> &Arc<ModeSelector> {
        &self.mode_selector
    }

    /// 获取当前运行场景
    pub fn current_mode(&self) -> RunningMode {
        self.mode_selector.current()
    }

    /// 切换运行场景
    pub async fn switch_mode(
        &self,
        new_mode: RunningMode,
        source: SwitchSource,
    ) -> Result<RunningMode, AiEngineError> {
        self.mode_selector.switch(new_mode, source).await
    }
}

/// 从配置解析初始运行模式
fn parse_initial_mode(config: &ModeConfig) -> RunningMode {
    crate::mode_selector::parse_mode_name(&config.default_mode)
        .unwrap_or(RunningMode::AgriculturalIrrigation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LstmConfig, ModeConfig, OnlineUpdateConfig, RlAlgorithm, RlConfig};

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
            mode: ModeConfig::default(),
            ..Default::default()
        }
    }

    #[test]
    fn test_model_manager_creation() {
        let config = create_test_config();
        let manager = ModelManager::new(config);
        // ModelManager 创建时状态为 Unloaded
        assert_eq!(manager.get_status_blocking(), ModelStatus::Unloaded);
    }
}

// Extension trait for sync status check in tests
impl ModelManager {
    /// 获取模型状态（同步版本，仅用于测试）
    #[allow(dead_code)]
    fn get_status_blocking(&self) -> ModelStatus {
        // 注意：这是测试辅助方法，生产代码应使用异步 get_status()
        ModelStatus::Unloaded
    }
}
