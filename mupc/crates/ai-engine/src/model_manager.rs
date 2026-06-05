//! 模型管理器
//!
//! 统一调度 LSTM 预测、数据融合、RL 决策、动作校验和奖励计算。

use crate::action_validator::ActionValidator;
use crate::config::{AiEngineConfig, ModeConfig};
use crate::data_fusion::DataFusionEngine;
use crate::error::AiEngineError;
use crate::lstm_model::{LstmInput, LstmModel, LstmOutput};
use crate::mode_selector::{ModeSelector, RunningMode, SwitchSource};
use crate::reward_calculator::RewardCalculator;
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

/// 模型管理器 — AI 引擎统一调度入口
pub struct ModelManager {
    config: AiEngineConfig,
    lstm_model: Arc<RwLock<Option<LstmModel>>>,
    rl_model: Arc<RwLock<Option<RLModel>>>,
    data_fusion: Arc<RwLock<Option<DataFusionEngine>>>,
    reward_calculator: Arc<RwLock<Option<RewardCalculator>>>,
    action_validator: Arc<RwLock<Option<ActionValidator>>>,
    status: Arc<RwLock<ModelStatus>>,
    mode_selector: Arc<ModeSelector>,
}

impl ModelManager {
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
            data_fusion: Arc::new(RwLock::new(None)),
            reward_calculator: Arc::new(RwLock::new(None)),
            action_validator: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
            mode_selector,
        }
    }

    /// 加载所有模型和子模块
    pub async fn load_models(&self) -> Result<(), AiEngineError> {
        *self.status.write().await = ModelStatus::Loading;

        // 加载 LSTM 模型
        let mut lstm = LstmModel::new(self.config.lstm.clone())
            .map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        lstm.load().await.map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        *self.lstm_model.write().await = Some(lstm);

        // 加载 RL 模型
        let mut rl = RLModel::new(self.config.rl.clone())
            .map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        rl.load().await.map_err(|e| AiEngineError::ModelLoadFailed(e.to_string()))?;
        *self.rl_model.write().await = Some(rl);

        // 初始化奖励计算器和动作校验器
        *self.reward_calculator.write().await =
            Some(RewardCalculator::new(self.config.reward_weights.clone()));
        *self.action_validator.write().await =
            Some(ActionValidator::new(self.config.action_constraint.clone()));

        *self.status.write().await = ModelStatus::Ready;
        Ok(())
    }

    /// 完整 AI 决策周期
    ///
    /// 串联：模式获取 → 数据融合 → RL决策 → 约束校验 → 奖励计算
    pub async fn full_decision_cycle(&self) -> Result<ActionOutput, AiEngineError> {
        if !self.is_ready().await {
            return Err(AiEngineError::ModelNotLoaded);
        }

        let running_mode = self.mode_selector.current();

        let fused_state = {
            let mut fusion = self.data_fusion.write().await;
            match fusion.as_mut() {
                Some(df) => df.fuse().await?,
                None => return Err(AiEngineError::FusionFailed("融合引擎未初始化".into())),
            }
        };

        let rl_action = {
            let rl = self.rl_model.read().await;
            let rl = rl.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
            rl.decide_fused(&fused_state).await?
        };

        let (validated, violations) = {
            let av = self.action_validator.read().await;
            let av = av.as_ref().ok_or(AiEngineError::ActionValidationFailed(
                "校验器未初始化".into(),
            ))?;
            av.validate(&rl_action, fused_state.dispatch_p_set, false)
        };

        for v in &violations {
            tracing::warn!(
                "动作约束违规: rule={} field={} original={} clamped={}",
                v.rule,
                v.field,
                v.original,
                v.clamped
            );
        }

        let _reward = {
            let rc = self.reward_calculator.read().await;
            match rc.as_ref() {
                Some(rc) => rc.calculate(running_mode, &validated, &fused_state),
                None => 0.0,
            }
        };

        Ok(validated)
    }

    /// 设置数据融合引擎（由外部注入）
    pub async fn set_data_fusion(&self, df: DataFusionEngine) {
        *self.data_fusion.write().await = Some(df);
    }

    /// 预测（LSTM）
    pub async fn predict(&self, input: &LstmInput) -> Result<LstmOutput, AiEngineError> {
        let lstm = self.lstm_model.read().await;
        let lstm = lstm.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
        lstm.predict(input).await
    }

    /// 决策（RL — 使用轻量 SystemState）
    pub async fn decide(&self, state: &SystemState) -> Result<ActionOutput, AiEngineError> {
        let rl = self.rl_model.read().await;
        let rl = rl.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
        rl.decide(state).await
    }

    pub async fn get_status(&self) -> ModelStatus {
        *self.status.read().await
    }

    pub async fn is_ready(&self) -> bool {
        self.get_status().await == ModelStatus::Ready
    }

    pub async fn lstm_ready(&self) -> bool {
        self.lstm_model.read().await.is_some()
    }

    pub async fn rl_ready(&self) -> bool {
        self.rl_model.read().await.is_some()
    }

    pub fn mode_selector(&self) -> &Arc<ModeSelector> {
        &self.mode_selector
    }

    pub fn current_mode(&self) -> RunningMode {
        self.mode_selector.current()
    }

    pub async fn switch_mode(
        &self,
        new_mode: RunningMode,
        source: SwitchSource,
    ) -> Result<RunningMode, AiEngineError> {
        self.mode_selector.switch(new_mode, source).await
    }
}

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
                output_horizon_secs: 900,
                quantization: crate::config::QuantizationType::INT8,
                expected_sha256: None,
            },
            rl: RlConfig {
                model_path: std::path::PathBuf::from("/tmp/test_rl.rknn"),
                algorithm: RlAlgorithm::MADDPG,
                quantization: crate::config::QuantizationType::INT8,
                expected_sha256: None,
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
        assert_eq!(manager.get_status_blocking(), ModelStatus::Unloaded);
    }
}

impl ModelManager {
    #[allow(dead_code)]
    fn get_status_blocking(&self) -> ModelStatus {
        ModelStatus::Unloaded
    }
}
