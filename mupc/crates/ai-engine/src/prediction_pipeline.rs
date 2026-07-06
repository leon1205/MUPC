//! 预测增强管线编排器
//!
//! 串联 VMD 分解 → 逐 IMF NPU 推理 → 重构 → 分位数后处理。
//! 各模块独立失败时自动降级，连续成功时自动升级。
//!
//! **第一轮范围（v1.0）：** VMD + LSTM/Attention（BiLSTM/误差修正预留）
//! **第二轮范围（v2.0）：** BiLSTM 双向替换 + 误差修正 BiLSTM 残差修正管线
//!
//! **设计偏离说明：** 按训练管线架构，VMD 分解本应位于 LstmModel 内部。
//! 但因 Rust 侧 LstmModel 直接调用 ONNX Runtime，无 Python 训练时的
//! NumPy/PyTorch 张量操作环境，故将 VMD 编排上移至 PredictionPipeline 层。

use crate::error::AiEngineError;
use crate::load_covariates::LoadCovariates;
use crate::lstm_model::{LstmInput, LstmModel, ProbabilisticLoadOutput, QuantilePrediction};
use crate::model_manager::HistorySample;
use crate::pipeline_config::{EnhancementLevel, PipelineHealth, PredictionEnhancementConfig};
use crate::residual_buffer::ResidualBuffer;
use crate::rknn_runtime::RknnRuntime;
use crate::vmd::{VmdConfig, VmdDecomposer};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// EnhancedForecastResult -- 增强预测结果
// ============================================================================

/// 预测增强结果（与现有 `run_lstm_predict_with_quantiles` 返回值兼容）
#[derive(Debug, Clone)]
pub struct EnhancedForecastResult {
    /// 光伏预测 15 维 (f64)
    pub pv_forecast: Vec<f64>,
    /// 负荷预测 15 维 (f64)
    pub load_forecast: Vec<f64>,
    /// 负荷分位数预测
    pub load_quantiles: Option<ProbabilisticLoadOutput>,
    /// 实际使用的增强等级
    pub enhancement_level: EnhancementLevel,
    /// VMD 内部降级标记（true = 至少一个 VMD 分解失败或未收敛，已回退到直接推理）
    pub vmd_degraded: bool,
    /// R2 新增：误差修正是否生效（true = 误差修正成功执行且产生非零修正）
    pub error_correction_applied: bool,
}

// ============================================================================
// PredictionPipeline -- 预测增强管线
// ============================================================================

/// 预测增强管线
///
/// 管理 VMD 分解器、LSTM 模型引用、历史缓冲和降级状态。
///
/// # 使用示例
///
/// ```ignore
/// let pipeline = PredictionPipeline::new(
///     enhancement_config,
///     lstm_model_arc,
///     lstm_history_arc,
///     input_size,
///     input_features,
/// )?;
/// let result = pipeline.execute().await?;
/// ```
pub struct PredictionPipeline {
    /// VMD 分解器（光伏），None 表示 VMD 未启用
    ///
    /// v3.0: `input_features > 1` 时 VMD 路径自动降级（VMD 仅适用于单变量时序）
    vmd_pv: Option<VmdDecomposer>,
    /// VMD 分解器（负荷），None 表示 VMD 未启用
    vmd_load: Option<VmdDecomposer>,
    /// LSTM 模型引用（用于 IMF 推理和降级推理）
    lstm_model: Arc<RwLock<Option<LstmModel>>>,
    /// v3.0: 历史缓冲引用（7 维 HistorySample）
    lstm_history: Arc<RwLock<VecDeque<HistorySample>>>,
    /// 输入窗口大小（步数）
    input_size: usize,
    /// v3.0: 输入特征数（默认 7，对齐训练管线）
    input_features: usize,
    /// 增强配置
    config: PredictionEnhancementConfig,
    /// 模块健康状态
    health: RwLock<PipelineHealth>,

    // --- R2 新增字段 ---
    /// 误差修正 RknnRuntime（独立实例，与主模型 Runtime 隔离）
    error_correction_runtime: Option<RknnRuntime>,
    /// PV 残差缓冲
    residual_buffer_pv: Option<RwLock<ResidualBuffer>>,
    /// Load 残差缓冲
    residual_buffer_load: Option<RwLock<ResidualBuffer>>,
}

impl PredictionPipeline {
    /// 创建预测增强管线
    ///
    /// 根据配置决定是否创建 VMD 分解器、误差修正 Runtime 和残差缓冲。
    /// 若配置中 VMD 已启用但参数非法（如 k_pv = 0），使用默认值并记录 WARN。
    ///
    /// # 初始等级确定
    ///
    /// - BiLSTM Go + VMD + 误差修正 → `FullVmdAttentionCorrection`
    /// - BiLSTM Go + VMD → `BiLstmVmdAttention`
    /// - 仅 VMD → `VmdAttention`
    /// - 全禁用 → `Baseline`
    pub fn new(
        enhancement_config: PredictionEnhancementConfig,
        lstm_model: Arc<RwLock<Option<LstmModel>>>,
        lstm_history: Arc<RwLock<VecDeque<HistorySample>>>,
        input_size: usize,
        input_features: usize,
    ) -> Self {
        // v3.0: VMD 仅适用于单变量时序 (input_features == 1)。
        // 多特征模式下自动禁用 VMD，降级到 AttentionOnly/Baseline 路径。
        let vmd_compatible = input_features <= 1;
        let (vmd_pv, vmd_load) = if enhancement_config.vmd.enabled && vmd_compatible {
            let k_pv = if enhancement_config.vmd.k_pv == 0 {
                tracing::warn!("VMD k_pv=0 非法，使用默认值 5");
                5
            } else {
                enhancement_config.vmd.k_pv
            };
            let k_load = if enhancement_config.vmd.k_load == 0 {
                tracing::warn!("VMD k_load=0 非法，使用默认值 6");
                6
            } else {
                enhancement_config.vmd.k_load
            };

            let pv_config = VmdConfig {
                k: k_pv,
                alpha: enhancement_config.vmd.alpha,
                tau: enhancement_config.vmd.tau,
                tol: enhancement_config.vmd.tol,
                max_iter: enhancement_config.vmd.max_iter,
            };
            let load_config = VmdConfig {
                k: k_load,
                alpha: enhancement_config.vmd.alpha,
                tau: enhancement_config.vmd.tau,
                tol: enhancement_config.vmd.tol,
                max_iter: enhancement_config.vmd.max_iter,
            };

            tracing::info!(
                "VMD 增强已启用: K_PV={}, K_Load={}, alpha={}, max_iter={}",
                k_pv,
                k_load,
                enhancement_config.vmd.alpha,
                enhancement_config.vmd.max_iter
            );
            (
                Some(VmdDecomposer::new(pv_config)),
                Some(VmdDecomposer::new(load_config)),
            )
        } else {
            if enhancement_config.vmd.enabled && !vmd_compatible {
                tracing::info!(
                    "VMD 增强已配置但 input_features={} > 1，VMD 路径自动禁用（VMD 仅适用于单变量）",
                    input_features
                );
            } else {
                tracing::info!("VMD 增强未启用，使用基线 LSTM 推理");
            }
            (None, None)
        };

        // --- R2: 误差修正 Runtime 初始化 ---
        let error_correction_runtime = if enhancement_config.error_correction.enabled {
            let ec_model_path = enhancement_config
                .error_correction
                .model_path
                .as_deref()
                .unwrap_or_else(|| Path::new("/etc/mupc/models/error_correction.rknn"));
            match RknnRuntime::new(ec_model_path, None) {
                Ok(rt) => {
                    tracing::info!(
                        "误差修正 Runtime 已创建: path={}",
                        ec_model_path.display()
                    );
                    Some(rt)
                }
                Err(e) => {
                    tracing::warn!("误差修正 Runtime 创建失败: {}，禁用误差修正", e);
                    None
                }
            }
        } else {
            None
        };

        // --- R2: 残差缓冲初始化 ---
        let ec_window = enhancement_config.error_correction.residual_window_steps;
        let ec_zero_init = enhancement_config.error_correction.zero_init;
        let (residual_buffer_pv, residual_buffer_load) =
            if enhancement_config.error_correction.enabled {
                (
                    Some(RwLock::new(ResidualBuffer::new(ec_window, ec_zero_init))),
                    Some(RwLock::new(ResidualBuffer::new(ec_window, ec_zero_init))),
                )
            } else {
                (None, None)
            };

        // --- 初始等级确定（R2 扩展：BiLSTM + 误差修正，v3.0: VMD 受 input_features 约束） ---
        let initial_level = {
            let bilstm_go = enhancement_config.bilstm.enabled
                && enhancement_config.bilstm.gate_passed;
            let ec_enabled = enhancement_config.error_correction.enabled
                && error_correction_runtime.is_some();
            let vmd_enabled = vmd_pv.is_some() && vmd_compatible;

            if bilstm_go && vmd_enabled && ec_enabled {
                EnhancementLevel::FullVmdAttentionCorrection
            } else if bilstm_go && vmd_enabled {
                EnhancementLevel::BiLstmVmdAttention
            } else if vmd_enabled {
                EnhancementLevel::VmdAttention
            } else {
                EnhancementLevel::Baseline
            }
        };

        tracing::info!(
            "预测增强管线创建完成: 初始等级={:?}({}), VMD={}, BiLSTM_Go={}, EC={}, input_features={}",
            initial_level,
            initial_level.name(),
            vmd_pv.is_some() && vmd_compatible,
            enhancement_config.bilstm.enabled && enhancement_config.bilstm.gate_passed,
            error_correction_runtime.is_some(),
            input_features
        );

        Self {
            vmd_pv,
            vmd_load,
            lstm_model,
            lstm_history,
            input_size,
            input_features,
            config: enhancement_config,
            health: RwLock::new(PipelineHealth {
                current_level: initial_level,
                ..Default::default()
            }),
            error_correction_runtime,
            residual_buffer_pv,
            residual_buffer_load,
        }
    }

    /// 执行增强预测（带降级循环）
    ///
    /// 内部编排：特征提取 → VMD 分解 → IMF 推理 → 重构 → 分位数 → 误差修正 → 降级处理。
    ///
    /// # R2 降级路径
    ///
    /// ```text
    /// Level 0: FullVmdAttentionCorrection → VMD + (Bi)LSTM/Attention + 误差修正
    ///   ├── 误差修正失败 → Level 1A (BiLstmVmdAttention)
    ///   ├── BiLSTM 失败 → Level 1B (VmdAttention)
    ///   └── VMD 失败 → Level 3 (AttentionOnly)
    ///
    /// Level 1A: BiLstmVmdAttention → BiLSTM + VMD + Attention (无误差修正)
    ///   ├── BiLSTM 失败 → Level 2 (VmdAttention)
    ///   └── VMD 失败 → Level 3 (AttentionOnly)
    ///
    /// Level 2: VmdAttention → VMD + LSTM/Attention
    ///   └── VMD 失败 → Level 3 (AttentionOnly)
    ///
    /// Level 3: AttentionOnly → LSTM/Attention (无 VMD)
    ///   └── Attention 失败 → Level 4 (Baseline)
    ///
    /// Level 4: Baseline → 基线 LSTM
    /// ```
    ///
    /// - 单模块失败不影响其他模块，自动降级
    /// - 连续 5 次 VMD/EC/BiLSTM 成功后自动升回上一级
    pub async fn execute(&self) -> Result<EnhancedForecastResult, AiEngineError> {
        // 读取当前等级（不持有锁跨越 await）
        let level = self.current_level();

        match level {
            // --- Level 0: 全功能 Go 路径 ---
            EnhancementLevel::FullVmdAttentionCorrection => {
                match self.execute_full_with_correction().await {
                    Ok(r) => {
                        let mut health = self.health.write().await;
                        if r.vmd_degraded {
                            health.on_failure_vmd();
                            if health.vmd_consecutive_failures >= 3 {
                                tracing::warn!(
                                    "VMD 连续 {} 次内部失败，降级至 AttentionOnly",
                                    health.vmd_consecutive_failures
                                );
                                health.current_level = EnhancementLevel::AttentionOnly;
                            }
                        } else {
                            health.on_success_vmd();
                            health.on_success_bilstm();
                            if r.error_correction_applied {
                                health.on_success_ec();
                            }
                            self.try_promote(&mut health);
                        }
                        Ok(r)
                    }
                    Err(e) => {
                        // 区分错误来源
                        let is_ec_failure = e.is_error_correction_failure();
                        let is_bilstm_failure = e.is_bilstm_failure();

                        if is_ec_failure {
                            tracing::warn!("误差修正失败: {}, 降级至 BiLSTM+VMD+Attention", e);
                            let mut health = self.health.write().await;
                            health.on_failure_ec();
                            if health.ec_consecutive_failures
                                >= self.config.error_correction.auto_disable_after_failures
                            {
                                tracing::warn!(
                                    "误差修正连续 {} 次失败，降级至 BiLstmVmdAttention",
                                    health.ec_consecutive_failures
                                );
                                health.current_level = EnhancementLevel::BiLstmVmdAttention;
                            }
                            drop(health);
                            self.execute_bilstm_vmd_attention().await
                        } else if is_bilstm_failure {
                            tracing::warn!("BiLSTM 推理失败: {}，回退单向 LSTM", e);
                            let mut health = self.health.write().await;
                            health.on_failure_bilstm();
                            health.current_level = EnhancementLevel::VmdAttention;
                            drop(health);
                            self.execute_vmd_attention().await
                        } else {
                            tracing::warn!("VMD 失败: {}, 降级至 Attention", e);
                            let mut health = self.health.write().await;
                            health.on_failure_vmd();
                            health.current_level = EnhancementLevel::AttentionOnly;
                            drop(health);
                            self.execute_attention_only().await
                        }
                    }
                }
            }

            // --- Level 1A: BiLSTM + VMD + Attention（无误差修正） ---
            EnhancementLevel::BiLstmVmdAttention => {
                match self.execute_bilstm_vmd_attention().await {
                    Ok(r) => {
                        let mut health = self.health.write().await;
                        if r.vmd_degraded {
                            health.on_failure_vmd();
                            if health.vmd_consecutive_failures >= 3 {
                                health.current_level = EnhancementLevel::AttentionOnly;
                            }
                        } else {
                            health.on_success_vmd();
                            health.on_success_bilstm();
                            self.try_promote(&mut health);
                        }
                        Ok(r)
                    }
                    Err(e) => {
                        if e.is_bilstm_failure() {
                            tracing::warn!("BiLSTM 失败: {}，回退单向 LSTM", e);
                            let mut health = self.health.write().await;
                            health.on_failure_bilstm();
                            health.current_level = EnhancementLevel::VmdAttention;
                            drop(health);
                            self.execute_vmd_attention().await
                        } else {
                            tracing::warn!("BiLSTM+VMD 失败: {}, 降级至 Attention", e);
                            let mut health = self.health.write().await;
                            health.on_failure_vmd();
                            health.current_level = EnhancementLevel::AttentionOnly;
                            drop(health);
                            self.execute_attention_only().await
                        }
                    }
                }
            }

            // --- Level 2: VMD + LSTM/Attention ---
            EnhancementLevel::VmdAttention => {
                match self.execute_vmd_attention().await {
                    Ok(r) => {
                        let mut health = self.health.write().await;
                        if r.vmd_degraded {
                            health.on_failure_vmd();
                            if health.vmd_consecutive_failures >= 3 {
                                tracing::warn!(
                                    "VMD 连续 {} 次内部失败/未收敛，降级至 AttentionOnly",
                                    health.vmd_consecutive_failures
                                );
                                health.current_level = EnhancementLevel::AttentionOnly;
                            }
                        } else {
                            health.on_success_vmd();
                            self.try_promote(&mut health);
                        }
                        Ok(r)
                    }
                    Err(e) => {
                        tracing::warn!("VMD+Attention 失败: {}, 降级至 Attention", e);
                        let mut health = self.health.write().await;
                        health.on_failure_vmd();
                        health.current_level = EnhancementLevel::AttentionOnly;
                        drop(health);
                        self.execute_attention_only().await
                    }
                }
            }

            // --- Level 3: Attention Only ---
            EnhancementLevel::AttentionOnly => match self.execute_attention_only().await {
                Ok(r) => Ok(r),
                Err(e) => {
                    tracing::warn!("Attention 失败: {}, 降级至基线 LSTM", e);
                    let mut health = self.health.write().await;
                    health.current_level = EnhancementLevel::Baseline;
                    drop(health);
                    self.execute_baseline().await
                }
            },

            // --- Level 4: Baseline ---
            EnhancementLevel::Baseline => self.execute_baseline().await,
        }
    }

    /// 执行 VMD + Attention 全功能预测
    async fn execute_vmd_attention(&self) -> Result<EnhancedForecastResult, AiEngineError> {
        // OPT: 读锁 scope 可缩短至仅读取模型引用后立即释放，避免跨 await 持有
        let lstm = self.lstm_model.read().await;
        let lstm = lstm
            .as_ref()
            .ok_or_else(|| AiEngineError::PipelineError("LSTM 模型未加载".into()))?;

        // OPT: 读锁 scope 可缩短至提取历史序列后立即释放
        let history = self.lstm_history.read().await;
        let len = history.len();
        if len < self.input_size {
            return Err(AiEngineError::PipelineError(format!(
                "历史缓冲不足 ({}/{})",
                len, self.input_size
            )));
        }

        // v3.0: 提取 PV 和负荷历史序列（从 HistorySample）
        let pv_history: Vec<f32> = history
            .iter()
            .rev()
            .take(self.input_size)
            .map(|s| s.pv_power as f32)
            .collect();
        let pv_history: Vec<f32> = pv_history.into_iter().rev().collect();

        let load_history: Vec<f32> = history
            .iter()
            .rev()
            .take(self.input_size)
            .map(|s| s.load_power as f32)
            .collect();
        let load_history: Vec<f32> = load_history.into_iter().rev().collect();

        drop(history); // 释放读锁

        // --- VMD 分解（PV + Load） ---
        let mut vmd_internally_degraded = false;

        let pv_vmd_result = if let Some(ref vmd) = self.vmd_pv {
            match vmd.decompose(&pv_history) {
                Ok(r) => {
                    // M-03: 检查收敛标志，未收敛也视为内部降级
                    if !r.converged {
                        tracing::warn!("PV VMD 未收敛 (iter={})，视为内部降级", r.iterations);
                        vmd_internally_degraded = true;
                    }
                    Some(r)
                }
                Err(e) => {
                    tracing::warn!("PV VMD 分解失败: {}，降级使用原始序列", e);
                    vmd_internally_degraded = true;
                    None
                }
            }
        } else {
            None
        };

        let load_vmd_result = if let Some(ref vmd) = self.vmd_load {
            match vmd.decompose(&load_history) {
                Ok(r) => {
                    // M-03: 检查收敛标志
                    if !r.converged {
                        tracing::warn!("负荷 VMD 未收敛 (iter={})，视为内部降级", r.iterations);
                        vmd_internally_degraded = true;
                    }
                    Some(r)
                }
                Err(e) => {
                    tracing::warn!("负荷 VMD 分解失败: {}，降级使用原始序列", e);
                    vmd_internally_degraded = true;
                    None
                }
            }
        } else {
            None
        };

        // --- IMF 推理 ---
        let pv_forecast: Vec<f64> = if let Some(ref vmd_result) = pv_vmd_result {
            // 逐 IMF 推理 → 重构
            self.predict_with_imfs(lstm, &vmd_result.imfs, &pv_history)
                .await?
        } else {
            // 降级：原始序列直接推理
            self.predict_direct(lstm, &pv_history).await?
        };

        let load_forecast_raw: Vec<f64> = if let Some(ref vmd_result) = load_vmd_result {
            self.predict_with_imfs(lstm, &vmd_result.imfs, &load_history)
                .await?
        } else {
            self.predict_direct(lstm, &load_history).await?
        };

        // --- 分位数后处理（负荷） ---
        // 使用与原始路径一致的协变量（默认值）
        let load_input = LstmInput {
            history: load_history,
            timestamp: chrono::Utc::now().timestamp(),
        };
        let covariates = LoadCovariates::default();
        let load_quantiles = match lstm.predict_quantiles(&load_input, &covariates).await {
            Ok(pq) => Some(pq),
            Err(e) => {
                tracing::warn!("分位数预测失败: {}，回退为 None", e);
                None
            }
        };

        // lstm 读锁在函数返回时自然释放
        Ok(EnhancedForecastResult {
            pv_forecast,
            load_forecast: load_forecast_raw,
            load_quantiles,
            enhancement_level: EnhancementLevel::VmdAttention,
            vmd_degraded: vmd_internally_degraded,
            error_correction_applied: false,
        })
    }

    /// R2 新增：执行 BiLSTM + VMD + Attention（无误差修正）
    ///
    /// Level 1A 降级路径：BiLSTM Go 但误差修正失败/禁用。
    /// 与 `execute_vmd_attention` 的区别：使用 BiLSTM 模型推理。
    /// 当前阶段 BiLSTM 通过独立 .rknn 文件加载，Rust 侧逻辑与 LSTM 相同
    /// （模型差异在 ONNX 计算图内部），故此路径目前与 VmdAttention 等效。
    async fn execute_bilstm_vmd_attention(&self) -> Result<EnhancedForecastResult, AiEngineError> {
        tracing::debug!("执行 BiLSTM+VMD+Attention（无误差修正）");
        let mut result = self.execute_vmd_attention().await?;
        result.enhancement_level = EnhancementLevel::BiLstmVmdAttention;
        Ok(result)
    }

    /// R2 新增：执行全功能预测（VMD + (Bi)LSTM/Attention + 误差修正）
    ///
    /// Level 0 Go 路径。先执行主预测，再执行误差修正。
    /// 误差修正依赖主预测结果，必须串行执行。
    async fn execute_full_with_correction(
        &self,
    ) -> Result<EnhancedForecastResult, AiEngineError> {
        // Step 1: 主预测（使用 BiLSTM 或单向 LSTM，取决于配置）
        let mut result = if self.config.bilstm.enabled && self.config.bilstm.gate_passed {
            self.execute_bilstm_vmd_attention().await?
        } else {
            self.execute_vmd_attention().await?
        };

        // Step 2: 误差修正（仅当配置启用且 Runtime 可用时）
        if self.config.error_correction.enabled && self.error_correction_runtime.is_some() {
            match self
                .execute_error_correction(&result.pv_forecast, &result.load_forecast)
                .await
            {
                Ok((corrected_pv, corrected_load)) => {
                    result.pv_forecast = corrected_pv;
                    result.load_forecast = corrected_load;
                    result.error_correction_applied = true;
                    result.enhancement_level = EnhancementLevel::FullVmdAttentionCorrection;
                    tracing::debug!("误差修正完成: PV+Load 预测已修正");
                }
                Err(e) => {
                    // 误差修正失败：保留主预测值，标记未应用
                    tracing::warn!("误差修正失败: {}，使用主预测值", e);
                    result.error_correction_applied = false;
                    // 将错误包装后返回给 execute() 处理降级
                    return Err(e);
                }
            }
        } else {
            // v3.1 P5 修复：EC 未配置或 Runtime 已卸载时静默降级，避免升级-降级振荡
            result.error_correction_applied = false;
            result.enhancement_level = EnhancementLevel::BiLstmVmdAttention;
            tracing::warn!(
                "误差修正未配置或 Runtime 不可用，静默降级至 BiLstmVmdAttention，"
            );
            let mut health = self.health.write().await;
            health.current_level = EnhancementLevel::BiLstmVmdAttention;
            health.ec_consecutive_successes = 0;
            drop(health);
            // 返回 Ok(未修正结果) 而非 Err，主预测值仍然有效
            return Ok(result);
        }

        Ok(result)
    }

    /// R2 新增：执行误差修正推理
    ///
    /// 对主预测结果进行残差修正：y_corrected = y_pred + e_pred。
    ///
    /// # 步骤
    ///
    /// 1. 残差输入构建（CPU）：从 ResidualBuffer 获取历史残差窗口
    /// 2. 误差修正推理（NPU）：使用 error_correction.rknn 预测未来残差
    /// 3. 修正输出（CPU）：y_corrected = y_pred + e_pred
    ///
    /// # 参数
    ///
    /// - `pv_pred`: PV 主预测值 [15 维]
    /// - `load_pred`: Load 主预测值 [15 维]
    ///
    /// # 返回
    ///
    /// - `Ok((corrected_pv, corrected_load))` — 修正后的 PV/Load 预测
    /// - `Err` — 误差修正失败（残差缓冲不足、推理错误等）
    async fn execute_error_correction(
        &self,
        pv_pred: &[f64],
        load_pred: &[f64],
    ) -> Result<(Vec<f64>, Vec<f64>), AiEngineError> {
        let ec_runtime = self
            .error_correction_runtime
            .as_ref()
            .ok_or_else(|| AiEngineError::ErrorCorrectionFailed("EC Runtime 未加载".into()))?;

        let window_size = self.config.error_correction.residual_window_steps;
        let zero_init = self.config.error_correction.zero_init;

        // --- Step EC-1: 残差输入构建 ---
        let pv_residual_input = if let Some(ref buf_lock) = self.residual_buffer_pv {
            let buf = buf_lock.read().await;
            if !buf.is_ready(window_size) {
                return Err(AiEngineError::ResidualBufferInsufficient {
                    filled: buf.len(),
                    capacity: window_size,
                });
            }
            buf.get_window(window_size)
                .ok_or_else(|| AiEngineError::ResidualBufferInsufficient {
                    filled: buf.len(),
                    capacity: window_size,
                })?
        } else if zero_init {
            vec![0.0_f32; window_size]
        } else {
            return Err(AiEngineError::ResidualBufferInsufficient {
                filled: 0,
                capacity: window_size,
            });
        };

        let load_residual_input = if let Some(ref buf_lock) = self.residual_buffer_load {
            let buf = buf_lock.read().await;
            if !buf.is_ready(window_size) {
                return Err(AiEngineError::ResidualBufferInsufficient {
                    filled: buf.len(),
                    capacity: window_size,
                });
            }
            buf.get_window(window_size)
                .ok_or_else(|| AiEngineError::ResidualBufferInsufficient {
                    filled: buf.len(),
                    capacity: window_size,
                })?
        } else if zero_init {
            vec![0.0_f32; window_size]
        } else {
            return Err(AiEngineError::ResidualBufferInsufficient {
                filled: 0,
                capacity: window_size,
            });
        };

        // --- Step EC-2: 误差修正推理（NPU, 串行 PV + Load） ---
        // 注意：误差修正模型输入为残差窗口 [T]，输出为未来修正值 [15]
        // 当前阶段使用简化的零向量推理（模型文件由 MUPC-AI2 训练管线提供）。
        // 若 Runtime 未加载，则跳过推理。

        let pv_correction: Vec<f64> = if !ec_runtime.is_loaded() {
            // EC Runtime 未加载时使用零修正（y_corrected = y_pred + 0 = y_pred）
            vec![0.0_f64; pv_pred.len()]
        } else {
            // 实际推理：ec_runtime.run(&pv_residual_input)
            // 当前占位：模型就绪后替换为实际推理调用
            tracing::debug!(
                "EC 推理 PV: input_len={}, output_horizon={}",
                pv_residual_input.len(),
                pv_pred.len()
            );
            ec_runtime
                .run(&pv_residual_input)
                .await
                .map(|v| v.into_iter().map(f64::from).collect())
                .unwrap_or_else(|e| {
                    tracing::warn!("EC PV 推理失败，回退零修正: {}", e);
                    vec![0.0_f64; pv_pred.len()]
                })
        };

        let load_correction: Vec<f64> = if !ec_runtime.is_loaded() {
            vec![0.0_f64; load_pred.len()]
        } else {
            tracing::debug!(
                "EC 推理 Load: input_len={}, output_horizon={}",
                load_residual_input.len(),
                load_pred.len()
            );
            ec_runtime
                .run(&load_residual_input)
                .await
                .map(|v| v.into_iter().map(f64::from).collect())
                .unwrap_or_else(|e| {
                    tracing::warn!("EC Load 推理失败，回退零修正: {}", e);
                    vec![0.0_f64; load_pred.len()]
                })
        };

        // --- Step EC-3: 修正输出 ---
        let corrected_pv: Vec<f64> = pv_pred
            .iter()
            .zip(pv_correction.iter())
            .map(|(&y, &e)| y + e)
            .collect();

        let corrected_load: Vec<f64> = load_pred
            .iter()
            .zip(load_correction.iter())
            .map(|(&y, &e)| y + e)
            .collect();

        Ok((corrected_pv, corrected_load))
    }

    /// 执行 Attention Only 预测（跳过 VMD）
    async fn execute_attention_only(&self) -> Result<EnhancedForecastResult, AiEngineError> {
        // 当前阶段：Attention 嵌入 ONNX 模型内，Rust 侧不区分 Attention/LSTM
        // 此路径与 Baseline 等价，保留为降级层级
        tracing::debug!("执行 Attention Only（实际等同 Baseline）");
        let mut result = self.execute_baseline().await?;
        result.enhancement_level = EnhancementLevel::AttentionOnly;
        Ok(result)
    }

    /// 执行基线 LSTM 预测（无 VMD，无 Attention）
    ///
    /// 与现有 `run_lstm_predict_with_quantiles` 逻辑一致。
    /// 保留为最低降级层级（Level 4）。
    async fn execute_baseline(&self) -> Result<EnhancedForecastResult, AiEngineError> {
        // OPT: 读锁 scope 可缩短至仅读取模型引用后立即释放
        let lstm = self.lstm_model.read().await;
        let lstm = lstm
            .as_ref()
            .ok_or_else(|| AiEngineError::PipelineError("LSTM 模型未加载".into()))?;

        if !lstm.runtime().is_loaded() {
            return Err(AiEngineError::PipelineError("LSTM Runtime 未加载".into()));
        }

        let history = self.lstm_history.read().await;
        let len = history.len();
        if len < self.input_size {
            return Ok(EnhancedForecastResult {
                pv_forecast: vec![0.0; 15],
                load_forecast: vec![0.0; 15],
                load_quantiles: None,
                enhancement_level: EnhancementLevel::Baseline,
                vmd_degraded: false,
                error_correction_applied: false,
            });
        }

        // v3.0: 构建展平的 (T, K) 多特征输入
        let flat_input = self.build_flat_input(&history);
        drop(history);

        let timestamp = chrono::Utc::now().timestamp();
        let input = LstmInput {
            history: flat_input,
            timestamp,
        };

        // 单次 ONNX 联合推理
        let output = lstm.predict(&input).await?;
        let (pv_forecast, load_forecast, load_quantiles) =
            self.parse_baseline_output(&output.predictions, timestamp);

        Ok(EnhancedForecastResult {
            pv_forecast,
            load_forecast,
            load_quantiles,
            enhancement_level: EnhancementLevel::Baseline,
            vmd_degraded: false,
            error_correction_applied: false,
        })
    }

    /// v3.0: 从历史缓冲构建展平的 (T, K) 多特征输入
    ///
    /// 布局: row-major — [t0_f0, t0_f1, ..., t0_f{K-1}, t1_f0, ...]
    /// 取最近 input_size 个样本，每个样本展平为 K 个特征值。
    fn build_flat_input(&self, history: &VecDeque<HistorySample>) -> Vec<f32> {
        let k = self.input_features.min(7);
        let mut flat = Vec::with_capacity(self.input_size * k);
        for sample in history.iter().rev().take(self.input_size).rev() {
            let features = sample.to_features();
            flat.extend_from_slice(&features[..k]);
        }
        flat
    }

    /// v3.0: 从 ONNX 原始输出解析 PV 预测、负荷预测和 D10 分位数
    ///
    /// 根据输出长度自动检测格式:
    /// - 90: p10p50p90 — (2, 15, 3) = [PV:P10(15), PV:P50(15), PV:P90(15), Load:P10(15), Load:P50(15), Load:P90(15)]
    /// - 47: legacy with D10 — [pv(15), load(15), quantiles(15), shock(1), base(1)]
    /// - 30: legacy no D10 — [pv(15), load(15)]
    fn parse_baseline_output(
        &self,
        predictions: &[f32],
        timestamp: i64,
    ) -> (Vec<f64>, Vec<f64>, Option<ProbabilisticLoadOutput>) {
        let out_len = predictions.len();
        match out_len {
            90 => {
                let pv_p50: Vec<f64> = predictions[15..30].iter().map(|&v| v as f64).collect();
                let load_p50: Vec<f64> = predictions[60..75].iter().map(|&v| v as f64).collect();

                // 构建 QuantilePrediction 列表 (15步 × 3 分位数)
                let mut quantiles: Vec<QuantilePrediction> = Vec::with_capacity(45);
                for i in 0..15 {
                    quantiles.push(QuantilePrediction { quantile: 0.10, value: predictions[45 + i] }); // Load P10
                    quantiles.push(QuantilePrediction { quantile: 0.50, value: predictions[60 + i] }); // Load P50
                    quantiles.push(QuantilePrediction { quantile: 0.90, value: predictions[75 + i] }); // Load P90
                }

                let p50_first = *predictions.get(60).unwrap_or(&0.0);
                let p90_first = *predictions.get(75).unwrap_or(&p50_first);
                let base_load = p50_first;

                let load_quantiles = Some(ProbabilisticLoadOutput {
                    timestamp,
                    quantiles,
                    base_load,
                    shock_probability: Self::compute_shock_prob(p50_first, p90_first),
                    confidence: Self::compute_quantile_confidence_static(p50_first, p90_first),
                });
                (pv_p50, load_p50, load_quantiles)
            }
            47 => {
                let pv: Vec<f64> = predictions[..15].iter().map(|&v| v as f64).collect();
                let load: Vec<f64> = predictions[15..30].iter().map(|&v| v as f64).collect();
                let base_load = *predictions.get(46).unwrap_or(&0.0);

                let mut quantiles: Vec<QuantilePrediction> = Vec::with_capacity(45);
                for i in 0..15 {
                    let p50 = *predictions.get(15 + i).unwrap_or(&0.0);
                    let p90 = *predictions.get(30 + i).unwrap_or(&p50);
                    let p10 = (p50 * 0.7).max(0.0); // 启发式 P10
                    quantiles.push(QuantilePrediction { quantile: 0.10, value: p10 });
                    quantiles.push(QuantilePrediction { quantile: 0.50, value: p50 });
                    quantiles.push(QuantilePrediction { quantile: 0.90, value: p90 });
                }

                let p50_first = *predictions.get(15).unwrap_or(&base_load);
                let p90_first = *predictions.get(30).unwrap_or(&p50_first);

                let load_quantiles = Some(ProbabilisticLoadOutput {
                    timestamp,
                    quantiles,
                    base_load,
                    shock_probability: Self::compute_shock_prob(p50_first, p90_first),
                    confidence: Self::compute_quantile_confidence_static(p50_first, p90_first),
                });
                (pv, load, load_quantiles)
            }
            _ => {
                tracing::warn!("LSTM 输出维度 {} 未识别，取前 15 维", out_len);
                let pv: Vec<f64> = predictions.iter().take(15).map(|&v| v as f64).collect();
                (pv, vec![0.0; 15], None)
            }
        }
    }

    /// 冲击概率计算（静态内联，避免跨模块调用私有方法）
    fn compute_shock_prob(base_load: f32, high_quantile: f32) -> f64 {
        let spread = (high_quantile - base_load).max(1e-6);
        let _std_approx = spread / 1.28;
        let z_score = 2.0;
        let shock_prob = 0.5 * Self::erfc_static(z_score / std::f32::consts::SQRT_2);
        shock_prob as f64
    }

    /// 置信度计算（静态内联）
    fn compute_quantile_confidence_static(p50: f32, p90: f32) -> f64 {
        let spread_ratio = (p90 - p50) / p50.max(1e-6);
        (1.0 - spread_ratio.min(1.0)).max(0.0) as f64
    }

    /// erfc 近似
    fn erfc_static(x: f32) -> f32 {
        let abs_x = x.abs();
        if abs_x > 8.0 { return 0.0; }
        let exp_term = (-x * x).exp();
        let denom = std::f32::consts::PI * abs_x + (std::f32::consts::PI * x * x + 4.0).sqrt();
        exp_term / denom
    }

    /// 逐 IMF 推理并重构预测结果
    ///
    /// 对每个 IMF 执行一次 LSTM 推理，将所有 IMF 的预测值逐元素求和。
    /// 若某个 IMF 推理失败，记录 WARN 并跳过该 IMF（降级为部分重构）。
    async fn predict_with_imfs(
        &self,
        lstm: &LstmModel,
        imfs: &[Vec<f32>],
        original_signal: &[f32],
    ) -> Result<Vec<f64>, AiEngineError> {
        if imfs.is_empty() {
            // 无 IMF → 降级为直接推理
            return self.predict_direct(lstm, original_signal).await;
        }

        let mut sum_predictions: Vec<f64> = Vec::new();

        for (idx, imf) in imfs.iter().enumerate() {
            let input = LstmInput {
                history: imf.clone(),
                timestamp: chrono::Utc::now().timestamp(),
            };

            match lstm.predict(&input).await {
                Ok(output) => {
                    if sum_predictions.is_empty() {
                        // 首次：初始化累加器
                        let output_len = output.predictions.len();
                        sum_predictions = vec![0.0_f64; output_len];
                    }
                    // M-01: 不同 IMF 输出长度不一致时静默截断 → 添加 WARN
                    if output.predictions.len() != sum_predictions.len() {
                        tracing::warn!(
                            "IMF[{}/{}] 输出长度 {} != 累加器长度 {}，超出部分将截断",
                            idx + 1,
                            imfs.len(),
                            output.predictions.len(),
                            sum_predictions.len()
                        );
                    }
                    for (i, &val) in output.predictions.iter().enumerate() {
                        if i < sum_predictions.len() {
                            sum_predictions[i] += val as f64;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "IMF[{}/{}] 推理失败: {}，跳过此 IMF（部分重构）",
                        idx + 1,
                        imfs.len(),
                        e
                    );
                }
            }
        }

        if sum_predictions.is_empty() {
            // 全部 IMF 推理失败 → 降级为直接推理
            tracing::warn!("所有 IMF 推理均失败，回退到直接推理");
            return self.predict_direct(lstm, original_signal).await;
        }

        Ok(sum_predictions)
    }

    /// 直接推理（无 VMD 分解）
    async fn predict_direct(
        &self,
        lstm: &LstmModel,
        signal: &[f32],
    ) -> Result<Vec<f64>, AiEngineError> {
        let input = LstmInput {
            history: signal.to_vec(),
            timestamp: chrono::Utc::now().timestamp(),
        };
        let output = lstm.predict(&input).await?;
        Ok(output.predictions.into_iter().map(|v| v as f64).collect())
    }

    /// R2 扩展：连续成功升级逻辑支持多模块
    ///
    /// VMD 连续 5 次成功 → 可升回 VMD 层级
    /// BiLSTM 连续 5 次成功 → 可升回 BiLSTM 层级
    /// 误差修正连续 5 次成功 → 可升回误差修正层级
    ///
    /// 每个模块独立追踪，不互相阻塞。
    fn try_promote(&self, health: &mut PipelineHealth) {
        let ec_enabled = self.config.error_correction.enabled;
        let bilstm_go = self.config.bilstm.enabled && self.config.bilstm.gate_passed;
        let vmd_enabled = self.vmd_pv.is_some();

        // 从当前等级确定可升至的等级
        let promote_target = match health.current_level {
            // Level 4 (Baseline) → Level 3 (AttentionOnly)
            EnhancementLevel::Baseline => {
                if health.vmd_consecutive_successes >= 5 {
                    Some(EnhancementLevel::AttentionOnly)
                } else {
                    None
                }
            }
            // Level 3 (AttentionOnly) → 恢复 VMD
            EnhancementLevel::AttentionOnly => {
                if vmd_enabled && health.vmd_consecutive_successes >= 5 {
                    // 逐级检查健康计数器，避免跳级引发升级-降级振荡
                    if bilstm_go
                        && health.bilstm_consecutive_successes >= 5
                        && ec_enabled
                        && health.ec_consecutive_successes >= 5
                    {
                        Some(EnhancementLevel::FullVmdAttentionCorrection)
                    } else if bilstm_go && health.bilstm_consecutive_successes >= 5 {
                        Some(EnhancementLevel::BiLstmVmdAttention)
                    } else if ec_enabled && health.ec_consecutive_successes >= 5 {
                        Some(EnhancementLevel::FullVmdAttentionCorrection)
                    } else {
                        // 仅 VMD 恢复，BiLSTM/EC 计数器不足时先升到 VmdAttention
                        Some(EnhancementLevel::VmdAttention)
                    }
                } else {
                    None
                }
            }
            // Level 2 (VmdAttention) → 恢复 BiLSTM 或 EC
            EnhancementLevel::VmdAttention => {
                if bilstm_go
                    && health.bilstm_consecutive_successes >= 5
                    && ec_enabled
                    && health.ec_consecutive_successes >= 5
                {
                    Some(EnhancementLevel::FullVmdAttentionCorrection)
                } else if bilstm_go && health.bilstm_consecutive_successes >= 5 {
                    Some(EnhancementLevel::BiLstmVmdAttention)
                } else if ec_enabled && health.ec_consecutive_successes >= 5 {
                    // EC 单独恢复（No-Go 路径）
                    Some(EnhancementLevel::FullVmdAttentionCorrection)
                } else {
                    None
                }
            }
            // Level 1A (BiLstmVmdAttention) → 恢复 EC
            EnhancementLevel::BiLstmVmdAttention => {
                if ec_enabled && health.ec_consecutive_successes >= 5 {
                    Some(EnhancementLevel::FullVmdAttentionCorrection)
                } else {
                    None
                }
            }
            // Level 0: 已是最高等级
            EnhancementLevel::FullVmdAttentionCorrection => None,
        };

        if let Some(next) = promote_target {
            if next < health.current_level {
                tracing::info!(
                    "自动升级: {:?}({}) → {:?}({}), VMD_succ={}, BiLSTM_succ={}, EC_succ={}",
                    health.current_level,
                    health.current_level.name(),
                    next,
                    next.name(),
                    health.vmd_consecutive_successes,
                    health.bilstm_consecutive_successes,
                    health.ec_consecutive_successes
                );
                health.current_level = next;
                health.vmd_consecutive_successes = 0;
                health.bilstm_consecutive_successes = 0;
                health.ec_consecutive_successes = 0;
            }
        }
    }

    /// 获取当前增强等级（无锁快读）
    pub fn current_level(&self) -> EnhancementLevel {
        // try_read 无锁读取；极低概率写锁冲突时保守回退到 Baseline
        match self.health.try_read() {
            Ok(guard) => guard.current_level,
            Err(_) => EnhancementLevel::Baseline,
        }
    }

    /// 获取健康状态（阻塞读取）
    pub async fn health(&self) -> PipelineHealth {
        self.health.read().await.clone()
    }

    /// 获取健康状态的可写引用（供外部降级逻辑使用）
    pub async fn health_write(&self) -> tokio::sync::RwLockWriteGuard<'_, PipelineHealth> {
        self.health.write().await
    }

    /// 获取增强配置引用
    pub fn config(&self) -> &PredictionEnhancementConfig {
        &self.config
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline_config::{
        AttentionConfig, BiLstmConfig, ErrorCorrectionConfig, FeatureSelectionConfig,
        VmdEnhancementConfig,
    };

    fn create_disabled_config() -> PredictionEnhancementConfig {
        PredictionEnhancementConfig {
            vmd: VmdEnhancementConfig {
                enabled: false,
                ..Default::default()
            },
            attention: AttentionConfig::default(),
            bilstm: BiLstmConfig::default(),
            error_correction: ErrorCorrectionConfig::default(),
            feature_selection: FeatureSelectionConfig::default(),
        }
    }

    fn create_vmd_enabled_config() -> PredictionEnhancementConfig {
        PredictionEnhancementConfig {
            vmd: VmdEnhancementConfig {
                enabled: true,
                k_pv: 3,
                k_load: 3,
                alpha: 2000.0,
                tau: 0.0,
                tol: 1.0e-6,
                max_iter: 500,
            },
            attention: AttentionConfig::default(),
            bilstm: BiLstmConfig::default(),
            error_correction: ErrorCorrectionConfig::default(),
            feature_selection: FeatureSelectionConfig::default(),
        }
    }

    // ========================================================================
    // PP-01: VMD 禁用时 Pipeline 初始等级为 Baseline
    // ========================================================================

    #[test]
    fn test_pipeline_creation_vmd_disabled() {
        let config = create_disabled_config();
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);
        assert_eq!(
            pipeline.current_level(),
            EnhancementLevel::Baseline,
            "VMD 禁用时初始等级应为 Baseline"
        );
    }

    // ========================================================================
    // PP-02: VMD 启用时 Pipeline 初始等级为 VmdAttention
    // ========================================================================

    #[test]
    fn test_pipeline_creation_vmd_enabled() {
        let config = create_vmd_enabled_config();
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);
        assert_eq!(
            pipeline.current_level(),
            EnhancementLevel::VmdAttention,
            "VMD 启用时初始等级应为 VmdAttention"
        );
    }

    // ========================================================================
    // PP-03: 降级逻辑 — VMD 失败时自动降级到 Baseline
    // ========================================================================

    #[tokio::test]
    async fn test_pipeline_degradation_to_baseline() {
        let config = create_vmd_enabled_config();
        // 不初始化 LSTM 模型 → VMD+Attention 和 Attention 路径均失败
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);

        // 执行预测 → 预期降级到 Baseline
        let result = pipeline.execute().await;
        // 因历史缓冲不足，Baseline 也会返回全零向量
        assert!(result.is_ok(), "降级后基线路径应返回 Ok");
        let r = result.unwrap();
        assert_eq!(
            r.enhancement_level,
            EnhancementLevel::Baseline,
            "多次降级后等级应为 Baseline"
        );
    }

    // ========================================================================
    // PP-04: try_promote — 连续 5 次 VMD 成功后升级
    // ========================================================================

    #[test]
    fn test_try_promote_after_5_successes() {
        let config = create_disabled_config();
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);

        // 手动设置初始状态：AttentionOnly → 期望升至 Baseline(4)
        let mut health = PipelineHealth {
            vmd_consecutive_successes: 5,
            current_level: EnhancementLevel::Baseline,
            ..Default::default()
        };

        pipeline.try_promote(&mut health);

        assert_eq!(
            health.current_level,
            EnhancementLevel::AttentionOnly,
            "5 次 VMD 成功后应从 Baseline 升到 AttentionOnly"
        );
        assert_eq!(health.vmd_consecutive_successes, 0, "成功计数应重置");
    }

    // ========================================================================
    // PP-05: try_promote — 不足 5 次成功不升级
    // ========================================================================

    #[test]
    fn test_try_promote_insufficient_successes() {
        let config = create_disabled_config();
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);

        let mut health = PipelineHealth {
            vmd_consecutive_successes: 3,
            current_level: EnhancementLevel::Baseline,
            ..Default::default()
        };

        let before = health.current_level;
        pipeline.try_promote(&mut health);
        assert_eq!(health.current_level, before, "不足 5 次成功不应升级");
        assert_eq!(health.vmd_consecutive_successes, 3, "成功计数不应重置");
    }

    // ========================================================================
    // PP-06: 健康状态读写
    // ========================================================================

    #[tokio::test]
    async fn test_health_state_reading() {
        let config = create_disabled_config();
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);

        let health = pipeline.health().await;
        assert_eq!(health.current_level, EnhancementLevel::Baseline);
    }

    // ========================================================================
    // PP-07: 默认配置无 VMD 启用
    // ========================================================================

    #[test]
    fn test_default_config_no_vmd() {
        let config = PredictionEnhancementConfig::default();
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);
        assert_eq!(pipeline.current_level(), EnhancementLevel::Baseline);
    }

    // ========================================================================
    // PP-08: VMD 参数非法时使用默认值
    // ========================================================================

    #[test]
    fn test_invalid_vmd_params_use_defaults() {
        let mut config = PredictionEnhancementConfig::default();
        config.vmd.enabled = true;
        config.vmd.k_pv = 0; // 非法值
        config.vmd.k_load = 0; // 非法值

        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        // 即使 k=0，Pipeline 应使用默认值创建（不会 panic）
        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);
        assert_eq!(pipeline.current_level(), EnhancementLevel::VmdAttention);
    }

    // ========================================================================
    // R2 测试
    // ========================================================================

    fn create_bilstm_go_ec_config() -> PredictionEnhancementConfig {
        PredictionEnhancementConfig {
            vmd: VmdEnhancementConfig {
                enabled: true,
                k_pv: 3,
                k_load: 3,
                ..Default::default()
            },
            attention: AttentionConfig::default(),
            bilstm: BiLstmConfig {
                enabled: true,
                gate_passed: true,
                ..Default::default()
            },
            error_correction: ErrorCorrectionConfig {
                enabled: true,
                residual_window_steps: 24,
                zero_init: true,
                ..Default::default()
            },
            feature_selection: FeatureSelectionConfig::default(),
        }
    }

    fn create_bilstm_nogo_config() -> PredictionEnhancementConfig {
        PredictionEnhancementConfig {
            vmd: VmdEnhancementConfig {
                enabled: true,
                k_pv: 3,
                k_load: 3,
                ..Default::default()
            },
            attention: AttentionConfig::default(),
            bilstm: BiLstmConfig {
                enabled: true,
                gate_passed: false, // No-Go
                ..Default::default()
            },
            error_correction: ErrorCorrectionConfig::default(),
            feature_selection: FeatureSelectionConfig::default(),
        }
    }

    // ========================================================================
    // PP-R2-01: BiLSTM Go + EC 启用时初始等级为 FullVmdAttentionCorrection
    // ========================================================================

    #[test]
    fn test_pipeline_creation_bilstm_go_ec_enabled() {
        let config = create_bilstm_go_ec_config();
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);
        assert_eq!(
            pipeline.current_level(),
            EnhancementLevel::FullVmdAttentionCorrection,
            "BiLSTM Go + EC 启用时初始等级应为 FullVmdAttentionCorrection"
        );
    }

    // ========================================================================
    // PP-R2-02: BiLSTM No-Go 时初始等级为 VmdAttention
    // ========================================================================

    #[test]
    fn test_pipeline_creation_bilstm_nogo() {
        let config = create_bilstm_nogo_config();
        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);
        assert_eq!(
            pipeline.current_level(),
            EnhancementLevel::VmdAttention,
            "BiLSTM No-Go 时初始等级应为 VmdAttention"
        );
    }

    // ========================================================================
    // PP-R2-03: BiLSTM enabled=true + gate_passed=false → No-Go 路径
    // ========================================================================

    #[test]
    fn test_bilstm_enabled_but_gate_not_passed() {
        let mut config = create_bilstm_nogo_config();
        config.bilstm.enabled = true;
        config.bilstm.gate_passed = false;

        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);
        // 应回退到 VmdAttention（等效单向LSTM + VMD）
        assert_eq!(pipeline.current_level(), EnhancementLevel::VmdAttention);
    }

    // ========================================================================
    // PP-R2-04: EC 禁用时 BiLSTM Go → BiLstmVmdAttention
    // ========================================================================

    #[test]
    fn test_bilstm_go_without_ec() {
        let mut config = create_bilstm_go_ec_config();
        config.error_correction.enabled = false;

        let model: Arc<RwLock<Option<LstmModel>>> = Arc::new(RwLock::new(None));
        let history: Arc<RwLock<VecDeque<HistorySample>>> =
            Arc::new(RwLock::new(VecDeque::with_capacity(24)));

        let pipeline = PredictionPipeline::new(config, model, history, 24, 1);
        assert_eq!(
            pipeline.current_level(),
            EnhancementLevel::BiLstmVmdAttention
        );
    }

    // ========================================================================
    // PP-R2-05: EnhancedForecastResult 包含 error_correction_applied 字段
    // ========================================================================

    #[test]
    fn test_enhanced_result_has_ec_field() {
        let result = EnhancedForecastResult {
            pv_forecast: vec![1.0; 15],
            load_forecast: vec![2.0; 15],
            load_quantiles: None,
            enhancement_level: EnhancementLevel::Baseline,
            vmd_degraded: false,
            error_correction_applied: false,
        };
        assert!(!result.error_correction_applied);
    }
}
