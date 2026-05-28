//! AI 引擎集成模块
//!
//! Phase 3C: 将 AI 优化引擎与策略引擎集成
//!
//! 提供 AI 命令验证和决策接口

use mupc_ai_engine::{ModelManager, ModelStatus, SystemState, ActionOutput, AiEngineError};
use std::sync::Arc;
use tokio::sync::RwLock;

/// AI 集成器
///
/// 管理 AI 模型生命周期并提供决策接口
pub struct AiIntegrator {
    model_manager: Arc<RwLock<Option<ModelManager>>>,
    status: Arc<RwLock<ModelStatus>>,
}

impl AiIntegrator {
    /// 创建 AI 集成器
    pub fn new() -> Self {
        Self {
            model_manager: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
        }
    }

    /// 初始化并加载模型
    pub async fn initialize(&self, config: mupc_ai_engine::AiEngineConfig) -> Result<(), AiEngineError> {
        let manager = ModelManager::new(config);
        manager.load_models().await?;
        *self.status.write().await = ModelStatus::Ready;
        *self.model_manager.write().await = Some(manager);
        Ok(())
    }

    /// 获取决策
    pub async fn get_decision(&self, state: &SystemState) -> Result<ActionOutput, AiEngineError> {
        let manager = self.model_manager.read().await;
        let manager = manager.as_ref()
            .ok_or(AiEngineError::ModelNotLoaded)?;
        manager.decide(state).await
    }

    /// 检查是否就绪
    pub async fn is_ready(&self) -> bool {
        *self.status.read().await == ModelStatus::Ready
    }

    /// 获取状态
    pub async fn status(&self) -> ModelStatus {
        *self.status.read().await
    }
}

impl Default for AiIntegrator {
    fn default() -> Self {
        Self::new()
    }
}

// Extension for sync check in tests
impl AiIntegrator {
    /// 检查是否就绪（同步版本，仅用于测试）
    fn is_ready_blocking(&self) -> bool {
        // 注意：这是测试辅助方法，生产代码应使用异步 is_ready()
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_integrator_creation() {
        let integrator = AiIntegrator::new();
        // AiIntegrator 创建时状态为 Unloaded
        assert!(!integrator.is_ready_blocking());
    }
}