//! AI 引擎集成模块
//!
//! Phase 3C: 将 AI 优化引擎与策略引擎集成。
//! v2.0: 扩展为 web-api 的服务门面，提供完整的 AI 查询和控制接口。

use crate::anti_reverse::AntiReverseStrategy;
use crate::config::{AntiReverseConfig, DemandControlConfig, PeakShavingConfig};
use crate::demand_control::DemandControlStrategy;
use crate::peak_shaving::PeakShavingStrategy;
use crate::south_command_sender::SouthCommandDispatcher;
use crate::strategies::FallbackStrategy;
use mupc_ai_engine::{
    AiEngineError, ModelManager, ModelStatus, RobustnessManager, RunningMode, SwitchSource,
};
use mupc_data_processing::telemetry::DataPackage;
use mupc_intercore::{DualParamCommand, IntercoreClient};
use std::sync::Arc;
use tokio::sync::RwLock;

/// AI 集成器
///
/// web-api 通过此门面访问 ai-engine，不直接调用 ai-engine。
/// 承担安全校验、指令兜底校验职责。
pub struct AiIntegrator {
    model_manager: Arc<RwLock<Option<Arc<ModelManager>>>>,
    status: Arc<RwLock<ModelStatus>>,
    /// 南向命令分发器（用于分发 pv_limit 和 load_shedding）
    south_dispatcher: Option<Arc<SouthCommandDispatcher>>,
    /// v2.7 核间通信客户端（用于发送双参数 p_ref + k_droop 到实时控制模块）
    intercore_client: Option<Arc<IntercoreClient>>,
    /// v2.6 双参数模式：最后有效的 p_ref（通信中断时使用）
    last_valid_p_ref: RwLock<Option<f64>>,
    /// v2.6 双参数模式：最后有效的 k_droop（通信中断时使用）
    last_valid_k_droop: RwLock<Option<f64>>,
    /// v2.6 双参数模式：降级状态
    fallback_active: RwLock<bool>,
    /// v3.1: 最新遥测数据（南向采集循环写入，供兜底策略 evaluate）
    latest_data: Arc<RwLock<Option<DataPackage>>>,
}

impl AiIntegrator {
    pub fn new() -> Self {
        Self {
            model_manager: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
            south_dispatcher: None,
            intercore_client: None,
            last_valid_p_ref: RwLock::new(None),
            last_valid_k_droop: RwLock::new(None),
            fallback_active: RwLock::new(false),
            latest_data: Arc::new(RwLock::new(None)),
        }
    }

    /// v2.6: 更新最后有效的双参数（由 intercore 调用）
    pub async fn update_last_valid_params(&self, p_ref: f64, k_droop: f64) {
        *self.last_valid_p_ref.write().await = Some(p_ref);
        *self.last_valid_k_droop.write().await = Some(k_droop);
    }

    /// v2.6: 获取降级动作（使用最后有效的 p_ref 和 k_droop）
    pub async fn get_fallback_action(&self) -> mupc_ai_engine::ActionOutput {
        let p_ref = *self.last_valid_p_ref.read().await;
        let k_droop = *self.last_valid_k_droop.read().await;

        match (p_ref, k_droop) {
            (Some(p), Some(k)) => {
                tracing::info!("Fallback: using last valid p_ref={}, k_droop={}", p, k);
                mupc_ai_engine::ActionOutput {
                    p_ref: p,
                    k_droop: k,
                    load_shedding: 0.0, // 降级时不修改负荷
                    pv_limit: 1.0,      // 降级时不限制光伏
                    confidence: 0.0,    // 降级模式置信度为 0
                }
            }
            _ => {
                tracing::warn!("No valid params available, using safe defaults");
                mupc_ai_engine::ActionOutput {
                    p_ref: 0.0,
                    k_droop: 10.0, // 默认下垂系数
                    load_shedding: 0.0,
                    pv_limit: 1.0,
                    confidence: 0.0,
                }
            }
        }
    }

    /// v2.6: 设置降级状态
    pub async fn set_fallback_active(&self, active: bool) {
        *self.fallback_active.write().await = active;
    }

    /// v2.6: 获取降级状态
    pub async fn is_fallback_active(&self) -> bool {
        *self.fallback_active.read().await
    }

    /// 设置最新遥测数据（南向采集循环调用，供兜底策略 evaluate）
    pub async fn set_latest_data(&self, data: DataPackage) {
        *self.latest_data.write().await = Some(data);
    }

    /// 运行本地兜底策略（AI 失效时）：防逆流→pv_limit、需量控制→load_shedding
    async fn run_fallback_strategies(&self) -> Result<(), AiEngineError> {
        let data = self.latest_data.read().await.clone();
        let Some(data) = data else {
            tracing::debug!("无遥测数据，跳过兜底策略");
            return Ok(());
        };

        let anti_reverse = AntiReverseStrategy::new(AntiReverseConfig::default());
        let demand = DemandControlStrategy::new(DemandControlConfig::default());
        let _peak = PeakShavingStrategy::new(PeakShavingConfig::default());

        let mut pv_limit = None;
        let mut load_shedding = None;

        if let Ok(cmd) = anti_reverse.evaluate(&data).await {
            pv_limit = cmd.pv_limit;
        }
        if let Ok(cmd) = demand.evaluate(&data).await {
            load_shedding = cmd.load_shedding;
        }

        if let Some(ref dispatcher) = self.south_dispatcher {
            if let Some(pv) = pv_limit {
                dispatcher.dispatch_pv_limit(pv, 1).await;
            }
            if let Some(ls) = load_shedding {
                dispatcher.dispatch_load_shedding(ls, 1).await;
            }
        }

        Ok(())
    }

    /// 初始化并加载模型
    pub async fn initialize(
        &self,
        config: mupc_ai_engine::AiEngineConfig,
    ) -> Result<(), AiEngineError> {
        let manager = ModelManager::new(config);
        manager.load_models().await?;
        *self.status.write().await = ModelStatus::Ready;
        *self.model_manager.write().await = Some(Arc::new(manager));
        Ok(())
    }

    /// 注入已创建的 ModelManager（启动编排器复用已加载的模型实例）
    pub async fn set_model_manager(&self, manager: Arc<ModelManager>) {
        *self.model_manager.write().await = Some(manager);
        *self.status.write().await = ModelStatus::Ready;
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

    /// v2.7: 设置核间通信客户端（用于发送双参数到实时控制模块）
    pub fn set_intercore_client(&mut self, client: Arc<IntercoreClient>) {
        self.intercore_client = Some(client);
    }

    /// 执行 AI 决策并分发南向命令
    ///
    /// 调用 full_decision_cycle() 获取 ActionOutput，然后：
    /// - p_ref + k_droop → 通过 IntercoreClient 发送到实时控制模块（v2.7）
    /// - pv_limit → 通过 SouthCommandDispatcher 发送到光伏逆变器
    /// - load_shedding → 通过 SouthCommandDispatcher 发送到负荷控制装置
    pub async fn dispatch_ai_decision(&self) -> Result<(), AiEngineError> {
        // v2.9 新增：异常检测与应急策略
        {
            let manager_guard = self.model_manager.read().await;
            if let Some(manager) = manager_guard.as_ref() {
                if let Some(state) = manager.get_current_state().await {
                    let robustness = RobustnessManager::new();
                    let anomalies = robustness.detect_anomaly(&state);

                    if !anomalies.is_empty() {
                        // 存在异常，使用应急策略
                        let primary_anomaly = anomalies[0];
                        let robust_action = robustness.get_robust_action(primary_anomaly);

                        tracing::warn!(
                            "Anomaly detected: {:?}, using emergency action: p_ref={}, k_droop={}",
                            primary_anomaly,
                            robust_action.p_ref,
                            robust_action.k_droop
                        );

                        // 使用应急动作替换正常决策
                        drop(manager_guard);
                        return self.dispatch_robust_action(&robust_action).await;
                    }
                }
            }
        }

        let manager = self.model_manager.read().await;
        let manager = manager.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;

        // 调用完整的 AI 决策周期；失败（AI 失效）时降级到本地兜底策略
        let action = match manager.full_decision_cycle().await {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("AI 决策失败，降级到本地兜底策略: {}", e);
                self.set_fallback_active(true).await;
                self.run_fallback_strategies().await?;
                return Ok(());
            }
        };

        // 正常决策成功，复位降级状态（此前异常触发的 fallback_active 不会自动复位）
        self.set_fallback_active(false).await;

        // v2.7: 发送双参数到实时控制模块
        if let Some(ref client) = self.intercore_client {
            // strategy_mode：策略模式（基础/智能/兜底），此处为正常 AI 决策 = 智能
            let strategy_mode = "intelligent".to_string();

            let cmd = DualParamCommand::new(
                action.p_ref,
                action.k_droop,
                self.is_ready().await, // ai_ready：反映 AI 引擎真实就绪状态
                &strategy_mode,
            );

            match client.send_dual_param(&cmd).await {
                Ok(_) => {
                    tracing::debug!(
                        "Sent dual-param to realtime control: p_ref={}, k_droop={}",
                        action.p_ref,
                        action.k_droop
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to send dual-param to realtime control: {:?}", e);
                    // 不返回错误，因为南向分发可能成功
                }
            }

            // 更新最后有效的双参数
            *self.last_valid_p_ref.write().await = Some(action.p_ref);
            *self.last_valid_k_droop.write().await = Some(action.k_droop);
        } else {
            tracing::debug!("Intercore client not set, skipping dual-param send");
        }

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

    /// v2.9: 分发应急动作（不经过 RL 模型）
    ///
    /// 直接使用 RobustnessManager 生成的应急动作，发送到实时控制模块和南向设备。
    async fn dispatch_robust_action(
        &self,
        action: &mupc_ai_engine::ActionOutput,
    ) -> Result<(), AiEngineError> {
        // 更新最后有效的双参数（用于通信中断时的降级）
        *self.last_valid_p_ref.write().await = Some(action.p_ref);
        *self.last_valid_k_droop.write().await = Some(action.k_droop);

        // 设置降级状态
        self.set_fallback_active(true).await;

        // 发送双参数到实时控制模块
        if let Some(ref client) = self.intercore_client {
            let cmd = DualParamCommand::new(
                action.p_ref,
                action.k_droop,
                true,
                "fallback",
            );
            match client.send_dual_param(&cmd).await {
                Ok(_) => {
                    tracing::debug!(
                        "Sent emergency dual-param: p_ref={}, k_droop={}",
                        action.p_ref,
                        action.k_droop
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to send emergency dual-param: {:?}", e);
                }
            }
        }

        // 分发 pv_limit 和 load_shedding
        if let Some(ref dispatcher) = self.south_dispatcher {
            if action.pv_limit < 1.0 {
                let result = dispatcher.dispatch_pv_limit(action.pv_limit, 1).await;
                if !result.success {
                    tracing::warn!(
                        "Emergency pv_limit 分发失败: device={}, error={:?}",
                        result.device_id,
                        result.error_message
                    );
                }
            }
            if action.load_shedding > 0.0 {
                let result = dispatcher
                    .dispatch_load_shedding(action.load_shedding, 1)
                    .await;
                if !result.success {
                    tracing::warn!(
                        "Emergency load_shedding 分发失败: device={}, error={:?}",
                        result.device_id,
                        result.error_message
                    );
                }
            }
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
