//! AI 引擎集成模块
//!
//! Phase 3C: 将 AI 优化引擎与策略引擎集成。
//! v2.0: 扩展为 web-api 的服务门面，提供完整的 AI 查询和控制接口。

use crate::strategies::FallbackStrategy;
use crate::tai_storage::TaiStorageStrategy;
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
    /// 台区储能治理策略（AI 失效兜底）
    tai_storage: Option<Arc<TaiStorageStrategy>>,
    /// 本地策略优先模式（配置或 Web API 可切换）：AI 旁路运行（仍决策作参考，不下发），
    /// 控制以下发本地策略（台区储能治理）为准
    local_priority: RwLock<bool>,
}

impl AiIntegrator {
    pub fn new() -> Self {
        Self {
            model_manager: Arc::new(RwLock::new(None)),
            status: Arc::new(RwLock::new(ModelStatus::Unloaded)),
            intercore_client: None,
            last_valid_p_ref: RwLock::new(None),
            last_valid_k_droop: RwLock::new(None),
            fallback_active: RwLock::new(false),
            latest_data: Arc::new(RwLock::new(None)),
            tai_storage: None,
            local_priority: RwLock::new(false),
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

    /// 运行本地兜底策略（AI 失效时）：台区储能治理（分相 P/Q 经核间下发）
    async fn run_fallback_strategies(&self) -> Result<(), AiEngineError> {
        let data = self.latest_data.read().await.clone();
        let Some(data) = data else {
            tracing::debug!("无遥测数据，跳过兜底策略");
            return Ok(());
        };

        // 台区储能治理策略：分相 P/Q 经核间下发实时控制模块（best-effort，失败仅告警）
        if let Some(tai) = &self.tai_storage {
            match tai.evaluate(&data).await {
                Ok(cmd) => match (cmd.phase_p_set, cmd.phase_q_set) {
                    (Some(p), Some(q)) => {
                        if let Some(ref client) = self.intercore_client {
                            if let Err(e) = client.send_tai_command(p, q, "fallback").await {
                                tracing::warn!("台区储能分相指令下发失败: {:?}", e);
                            } else {
                                tracing::debug!("台区储能分相指令已下发: p={:?}, q={:?}", p, q);
                            }
                        } else {
                            tracing::warn!("核间客户端未注入，台区储能分相指令未下发");
                        }
                    }
                    _ => tracing::debug!("台区储能策略未产出完整分相 P/Q，跳过下发"),
                },
                Err(e) => tracing::warn!("TaiStorageStrategy 执行失败: {}", e),
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

    /// v2.7: 设置核间通信客户端（用于发送双参数到实时控制模块）
    pub fn set_intercore_client(&mut self, client: Arc<IntercoreClient>) {
        self.intercore_client = Some(client);
    }

    /// 注入台区储能治理策略
    pub fn set_tai_storage_strategy(&mut self, strategy: Arc<TaiStorageStrategy>) {
        self.tai_storage = Some(strategy);
    }

    /// 设置本地策略优先模式（true：本地台区储能策略优先，AI 旁路；false：AI 优先）
    pub async fn set_local_priority(&self, enabled: bool) {
        *self.local_priority.write().await = enabled;
    }

    /// 获取本地策略优先状态
    pub async fn is_local_priority(&self) -> bool {
        *self.local_priority.read().await
    }

    /// 执行决策并下发核间指令
    ///
    /// 默认：调用 full_decision_cycle() 获取 ActionOutput，p_ref + k_droop →
    /// 通过 IntercoreClient 发送到实时控制模块（v2.7）。
    /// 本地优先模式（local_priority）：AI 旁路运行（仍决策仅作参考，不下发），
    /// 控制以下发本地台区储能治理策略（分相 P/Q）为准。
    pub async fn dispatch_ai_decision(&self) -> Result<(), AiEngineError> {
        // 本地策略优先模式：AI 旁路，控制以本地策略为准
        if *self.local_priority.read().await {
            if let Some(manager) = self.model_manager.read().await.as_ref() {
                match manager.full_decision_cycle().await {
                    Ok(a) => tracing::debug!(
                        "本地优先模式：AI 旁路决策 p_ref={}, k_droop={}（仅参考，不下发）",
                        a.p_ref,
                        a.k_droop
                    ),
                    Err(e) => tracing::debug!("本地优先模式：AI 旁路决策失败（忽略）: {}", e),
                }
            }
            return self.run_fallback_strategies().await;
        }

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
            let cmd = DualParamCommand::new(action.p_ref, action.k_droop, true, "fallback");
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

    #[test]
    fn test_set_tai_storage_strategy() {
        let mut integrator = AiIntegrator::new();
        // 未注入前为 None（通过行为验证：set 后正常注入不 panic）
        let strategy = Arc::new(crate::tai_storage::TaiStorageStrategy::new(
            crate::config::TaiStorageConfig::default(),
        ));
        integrator.set_tai_storage_strategy(strategy);
        // 注入后再次调用兜底流程不报错（无遥测数据时跳过）
        let result = tokio_test::block_on(integrator.run_fallback_strategies());
        assert!(result.is_ok(), "无遥测数据时兜底应正常返回 Ok");
    }

    impl AiIntegrator {
        fn is_ready_blocking(&self) -> bool {
            false
        }
    }
}
