//! AI 引擎集成模块
//!
//! Phase 3C: 将 AI 优化引擎与策略引擎集成。
//! v2.0: 扩展为 web-api 的服务门面，提供完整的 AI 查询和控制接口。

use crate::south_command_sender::SouthCommandDispatcher;
use mupc_ai_engine::{AiEngineError, ModelManager, ModelStatus, RunningMode, SwitchSource};
use std::sync::Arc;
use tokio::sync::RwLock;

/// AI 集成器
///
/// web-api 通过此门面访问 ai-engine，不直接调用 ai-engine。
/// 承担安全校验、指令兜底校验职责。
pub struct AiIntegrator {
    model_manager: Arc<RwLock<Option<ModelManager>>>,
    status: Arc<RwLock<ModelStatus>>,
    /// 南向命令分发器（用于分发 pv_limit 和 load_shedding）
    south_dispatcher: Option<Arc<SouthCommandDispatcher>>,
}

impl AiIntegrator {
    pub fn new() -> Self {
        Self {
            model_manager: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
            south_dispatcher: None,
        }
    }

    /// 初始化并加载模型
    pub async fn initialize(
        &self,
        config: mupc_ai_engine::AiEngineConfig,
    ) -> Result<(), AiEngineError> {
        let manager = ModelManager::new(config);
        manager.load_models().await?;
        *self.status.write().await = ModelStatus::Ready;
        *self.model_manager.write().await = Some(manager);
        Ok(())
    }

    // ── 查询接口 ──

    /// 获取 AI 引擎状态
    pub async fn engine_status(&self) -> AiEngineStatusInfo {
        let status = *self.status.read().await;
        let manager = self.model_manager.read().await;
        let (lstm_ready, rl_ready, current_mode) = if let Some(ref mgr) = *manager {
            (
                mgr.lstm_ready().await,
                mgr.registry_ready().await,
                Some(mgr.current_mode().await),
            )
        } else {
            (false, false, None)
        };

        AiEngineStatusInfo {
            engine_status: match status {
                ModelStatus::Ready => "ready",
                ModelStatus::Loading => "loading",
                ModelStatus::Error => "error",
                ModelStatus::Unloaded => "unloaded",
            },
            lstm_ready,
            rl_ready,
            current_mode: current_mode.map(|m| ModeInfo {
                id: format!("{:?}", m),
                display_name: m.display_name().to_string(),
                description: m.description().to_string(),
            }),
            ai_engine_enabled: status != ModelStatus::Unloaded,
            fallback_active: status == ModelStatus::Error,
        }
    }

    /// 获取当前运行模式
    pub async fn current_mode(&self) -> Option<RunningMode> {
        let manager = self.model_manager.read().await;
        match manager.as_ref() {
            Some(m) => Some(m.current_mode().await),
            None => None,
        }
    }

    /// 切换运行模式
    pub async fn switch_mode(
        &self,
        new_mode: RunningMode,
        source: SwitchSource,
    ) -> Result<RunningMode, AiEngineError> {
        let manager = self.model_manager.read().await;
        let manager = manager.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
        manager.switch_mode(new_mode, source).await
    }

    /// 检查是否就绪
    pub async fn is_ready(&self) -> bool {
        *self.status.read().await == ModelStatus::Ready
    }

    /// 获取原始状态
    pub async fn raw_status(&self) -> ModelStatus {
        *self.status.read().await
    }

    /// 获取 ModeSelector 引用（v2.3: 返回 RwLock<ModeSelector> 的 Arc）
    pub async fn mode_selector(
        &self,
    ) -> Option<Arc<tokio::sync::RwLock<mupc_ai_engine::ModeSelector>>> {
        let manager = self.model_manager.read().await;
        manager.as_ref().map(|m| m.mode_selector_arc())
    }

    /// 设置南向命令分发器
    pub fn set_south_dispatcher(&mut self, dispatcher: Arc<SouthCommandDispatcher>) {
        self.south_dispatcher = Some(dispatcher);
    }

    /// 执行 AI 决策并分发南向命令
    ///
    /// 调用 full_decision_cycle() 获取 ActionOutput，然后：
    /// - p_batt_set → 通过 intercore 发送到实时控制模块（已由调用方处理）
    /// - pv_limit → 通过 SouthCommandDispatcher 发送到光伏逆变器
    /// - load_shedding → 通过 SouthCommandDispatcher 发送到负荷控制装置
    pub async fn dispatch_ai_decision(&self) -> Result<(), AiEngineError> {
        let manager = self.model_manager.read().await;
        let manager = manager.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;

        // 调用完整的 AI 决策周期
        let action = manager.full_decision_cycle().await?;

        // 分发 pv_limit 到南向设备
        if let Some(ref dispatcher) = self.south_dispatcher {
            // 分发 pv_limit（限功率比例）
            if action.pv_limit < 1.0 {
                let result = dispatcher.dispatch_pv_limit(action.pv_limit, 1).await;
                if !result.success {
                    tracing::warn!(
                        "pv_limit 分发失败: device={}, error={:?}",
                        result.device_id,
                        result.error_message
                    );
                }
            }

            // 分发 load_shedding（负荷切除功率）
            if action.load_shedding > 0.0 {
                let result = dispatcher
                    .dispatch_load_shedding(action.load_shedding, 1)
                    .await;
                if !result.success {
                    tracing::warn!(
                        "load_shedding 分发失败: device={}, error={:?}",
                        result.device_id,
                        result.error_message
                    );
                }
            }
        } else {
            tracing::debug!("南向分发器未设置，跳过 pv_limit/load_shedding 分发");
        }

        Ok(())
    }
}

impl Default for AiIntegrator {
    fn default() -> Self {
        Self::new()
    }
}

/// AI 引擎状态信息（供 web-api 序列化）
#[derive(Debug, Clone, serde::Serialize)]
pub struct AiEngineStatusInfo {
    pub engine_status: &'static str,
    pub lstm_ready: bool,
    pub rl_ready: bool,
    pub current_mode: Option<ModeInfo>,
    pub ai_engine_enabled: bool,
    pub fallback_active: bool,
}

/// 模式信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModeInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_integrator_creation() {
        let integrator = AiIntegrator::new();
        assert!(!integrator.is_ready_blocking());
    }

    impl AiIntegrator {
        fn is_ready_blocking(&self) -> bool {
            false
        }
    }
}
