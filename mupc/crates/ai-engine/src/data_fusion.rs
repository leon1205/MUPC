//! 多源数据融合引擎
//!
//! DataFusionEngine 以 1Hz 频率从 5 个数据源采集数据，
//! 融合为 FusedSystemState（6 大类 24 字段），
//! 序列化为 48 维向量供 RL 模型推理。
//!
//! 关键入口：
//! - `FusedSystemState::to_input_vector()` — 48 维序列化
//! - `validate_input_vector()` — NaN/Inf 安全检查（PRD 9.5）
//! - `DataFusionEngine::fuse()` — 并行采集 + 融合

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// 融合系统状态（6 大类，24 + 3 RL 字段 = 27 字段，v2.10）
#[derive(Debug, Clone)]
pub struct FusedSystemState {
    // ── D1: 实时数据 (9 RL + 1 aux) ──
    pub timestamp: i64, // 辅助
    pub battery_soc: f64,
    pub pv_power: f64,
    pub load_power: f64,
    pub grid_power: f64,
    pub transformer_load: f64,
    pub battery_power: f64,
    pub voltage_phase_a: f64,
    pub voltage_phase_b: f64,
    pub voltage_phase_c: f64,
    // ── D2: 预测数据 (2 vectors) ──
    pub pv_forecast_15min: Vec<f64>,
    pub load_forecast_15min: Vec<f64>,
    // ── D3: 电价 (3 RL + 2 aux) ──
    pub current_electricity_price: f64,
    pub next_period_price: f64,
    pub price_tariff_id: u8,
    pub peak_price: f64,   // 辅助
    pub valley_price: f64, // 辅助
    // ── D4: 需量 (3) ──
    pub current_demand: f64,
    pub contract_demand: f64,
    pub peak_demand_this_month: f64,
    // ── D5: 气象 (2) ──
    pub solar_irradiance: f64,
    pub temperature: f64,
    // ── D6: 调度指令 (2) ──
    pub dispatch_p_set: Option<f64>,
    pub dispatch_q_set: Option<f64>,
    // ── D7: 实时模块状态 (1 RL) ──
    /// 实时模块剩余无功容量比例 [0.0, 1.0]
    /// 0 = 无功打满，1 = 完全空闲
    pub q_realtime_margin: f64,
    // ── D8: 季节时段 (8 维) ──
    /// 季节 one-hot 编码（6 维）：[灌溉季, 炒茶季, 空调季, 常规季, 保留, 保留]
    pub season_encoding: [f64; 6],
    /// 时段 one-hot 编码（2 维）：[白天, 夜间]
    pub time_period_encoding: [f64; 2],
    // ── D9: 安全覆盖状态 (v2.10 新增, 5 维) ──
    /// 安全覆盖激活标志
    /// true = 实时模块正在覆盖 AI 有功指令
    pub safety_override_active: bool,
    /// 安全覆盖触发原因（仅在 active=true 时有效）
    pub safety_override_reason: Option<String>,
    /// 安全覆盖强制放电功率 (kW)（仅在 active=true 时有效）
    pub safety_override_p_ref: Option<f64>,
    /// 安全覆盖连续触发次数（v2.14 新增）
    pub safety_override_consecutive: u32,
    /// 安全覆盖滑动窗口内覆盖比例（v2.14 新增，范围 [0.0, 1.0]）
    pub safety_override_ratio: f64,
    // ── D10: 概率负荷预测 (v2.11 新增, 3 维) ──
    /// 分位数负荷预测（15 维，对应 15 分钟预测窗口）
    pub load_forecast_quantiles: Vec<f64>,
    /// 冲击负荷发生概率
    pub shock_load_probability: f64,
    /// 基础负荷（50% 分位数）
    pub base_load: f64,
}

impl Default for FusedSystemState {
    fn default() -> Self {
        Self {
            timestamp: 0,
            battery_soc: 0.5,
            pv_power: 0.0,
            load_power: 0.0,
            grid_power: 0.0,
            transformer_load: 1.0,
            battery_power: 0.0,
            voltage_phase_a: 1.0,
            voltage_phase_b: 1.0,
            voltage_phase_c: 1.0,
            pv_forecast_15min: vec![],
            load_forecast_15min: vec![],
            current_electricity_price: 0.5,
            next_period_price: 0.5,
            price_tariff_id: 0,
            peak_price: 0.8,
            valley_price: 0.3,
            current_demand: 0.0,
            contract_demand: 200.0,
            peak_demand_this_month: 0.0,
            solar_irradiance: 0.0,
            temperature: 25.0,
            dispatch_p_set: None,
            dispatch_q_set: None,
            q_realtime_margin: 0.5,
            season_encoding: [0.0, 0.0, 0.0, 1.0, 0.0, 0.0], // 默认常规季
            time_period_encoding: [1.0, 0.0],                // 默认白天
            // v2.10 新增字段
            safety_override_active: false,
            safety_override_reason: None,
            safety_override_p_ref: None,
            // v2.14 新增字段
            safety_override_consecutive: 0,
            safety_override_ratio: 0.0,
            // v2.11 新增字段
            load_forecast_quantiles: vec![],
            shock_load_probability: 0.0,
            base_load: 0.0,
        }
    }
}

impl FusedSystemState {
    /// 序列化为 78 维输入向量（v2.14, v3.1 修复 D1 重复推送）
    ///
    /// 布局:
    ///   [0..8]   D1 实时数据 (9 维): soc/pv/load/grid/transformer_load/battery_power/va/vb/vc
    ///   [9..24]  D2 pv_forecast (15 维)
    ///   [24..39] D2 load_forecast (15 维)
    ///   [39..42] D3 电价 (3 维)
    ///   [42..45] D4 需量 (3 维)
    ///   [45..47] D5 气象 (2 维)
    ///   [47]     D6 dispatch_p_set (None→0.0)
    ///   [48]     D7 q_realtime_margin
    ///   [49..57] D8 season_encoding(6) + time_period_encoding(2)
    ///   [57..61] D9 safety_override (4 维)
    ///   [61..76] D10 load_forecast_quantiles (15 维)
    ///   [76]     D10 shock_load_probability
    ///   [77]     D10 base_load
    pub fn to_input_vector(&self) -> Vec<f32> {
        let mut v = Vec::with_capacity(78);

        // D1 [0..8] 9 维实时数据
        v.push(self.battery_soc as f32);
        v.push(self.pv_power as f32);
        v.push(self.load_power as f32);
        v.push(self.grid_power as f32);
        v.push(self.transformer_load as f32);
        v.push(self.battery_power as f32);
        v.push(self.voltage_phase_a as f32);
        v.push(self.voltage_phase_b as f32);
        v.push(self.voltage_phase_c as f32);

        // D2 pv_forecast [9..24] 15 维
        let pv = pad_or_truncate(&self.pv_forecast_15min, 15);
        v.extend(pv.iter().map(|&x| x as f32));

        // D2 load_forecast [24..39] 15 维
        let load = pad_or_truncate(&self.load_forecast_15min, 15);
        v.extend(load.iter().map(|&x| x as f32));

        // D3 [39..42] 3 维
        v.push(self.current_electricity_price as f32);
        v.push(self.next_period_price as f32);
        v.push(self.price_tariff_id as f32);

        // D4 [42..45] 3 维
        v.push(self.current_demand as f32);
        v.push(self.contract_demand as f32);
        v.push(self.peak_demand_this_month as f32);

        // D5 [45..47] 2 维
        v.push(self.solar_irradiance as f32);
        v.push(self.temperature as f32);

        // D6 [47] 1 维
        v.push(self.dispatch_p_set.unwrap_or(0.0) as f32);

        // D7 [48] 1 维
        v.push(self.q_realtime_margin as f32);

        // D8 [49..57] 8 维
        for &s in &self.season_encoding {
            v.push(s as f32);
        }
        for &t in &self.time_period_encoding {
            v.push(t as f32);
        }

        // D9 [57..61] 4 维
        v.push(if self.safety_override_active {
            1.0
        } else {
            0.0
        });
        v.push(self.safety_override_p_ref.unwrap_or(0.0) as f32);
        v.push(self.safety_override_consecutive as f32);
        v.push(self.safety_override_ratio as f32);

        // D10 [61..76] 15 维分位数负荷预测
        let quantiles = pad_or_truncate(&self.load_forecast_quantiles, 15);
        v.extend(quantiles.iter().map(|&x| x as f32));

        // D10 [76] 冲击负荷概率
        v.push(self.shock_load_probability as f32);

        // D10 [77] 基础负荷
        v.push(self.base_load as f32);

        debug_assert_eq!(v.len(), 78, "输入向量必须为 78 维");
        v
    }
}

/// 验证输入向量无 NaN/Inf（PRD 9.5 安全要求）
///
/// 在将输入向量传入 RKNN Runtime 之前调用。
/// 检测到 NaN 或 Inf 时记录 ERROR 日志并返回错误。
pub fn validate_input_vector(v: &[f32]) -> Result<(), crate::error::AiEngineError> {
    for (i, &val) in v.iter().enumerate() {
        if val.is_nan() || val.is_infinite() {
            tracing::error!("输入张量第 {} 维包含 NaN/Inf: {}", i, val);
            return Err(crate::error::AiEngineError::InferenceFailed(format!(
                "输入张量第 {} 维包含 NaN/Inf",
                i
            )));
        }
    }
    Ok(())
}

/// v3.0 (P0-1): MinMax 归一化观测向量，镜像 `mupc_env/observation.py:normalize_obs()`
///
/// 将 78 维原始观测归一化到 [0, 1]，公式: `(x - lo) / (hi - lo + 1e-9)`。
/// 与 MUPC-AI2 训练管线严格对齐，确保 ONNX 模型接收的输入分布与训练时一致。
///
/// 归一化范围来自 `mupc_env/constants.py`。
/// identity 维度（one-hot、已在 [0,1] 的维度）保持不变。
pub fn normalize_observation(v: &[f32]) -> Vec<f32> {
    debug_assert_eq!(v.len(), 78, "观测向量必须为 78 维");
    let mut out = vec![0.0_f32; 78];

    // D1 [0..8] 9 维实时数据
    out[0] = minmax(v[0], 0.0, 1.0);       // SOC
    out[1] = minmax(v[1], 0.0, 150.0);     // PV power
    out[2] = minmax(v[2], 0.0, 60.0);      // Load power
    out[3] = minmax(v[3], -200.0, 200.0);  // Grid power
    out[4] = v[4];                         // Transformer load: identity [0,1]
    out[5] = minmax(v[5], -50.0, 50.0);    // Battery power
    out[6] = minmax(v[6], 0.85, 1.15);     // V_a
    out[7] = minmax(v[7], 0.85, 1.15);     // V_b
    out[8] = minmax(v[8], 0.85, 1.15);     // V_c

    // D2 [9..23] pv_forecast 15 维
    for i in 0..15 {
        out[9 + i] = minmax(v[9 + i], 0.0, 150.0);
    }

    // D2 [24..38] load_forecast 15 维
    for i in 0..15 {
        out[24 + i] = minmax(v[24 + i], 0.0, 60.0);
    }

    // D3 [39..41] 3 维电价
    out[39] = minmax(v[39], 0.0, 1.5);     // current_price
    out[40] = minmax(v[40], 0.0, 1.5);     // next_price
    out[41] = minmax(v[41], 0.0, 3.0);     // tariff_id

    // D4 [42..44] 3 维需量
    out[42] = minmax(v[42], 0.0, 500.0);
    out[43] = minmax(v[43], 0.0, 500.0);
    out[44] = minmax(v[44], 0.0, 500.0);

    // D5 [45..46] 2 维气象
    out[45] = minmax(v[45], 0.0, 1500.0);  // solar_irradiance
    out[46] = minmax(v[46], -20.0, 60.0);  // temperature

    // D6 [47] dispatch_p_set
    out[47] = minmax(v[47], -200.0, 200.0);

    // D7 [48] q_realtime_margin: identity [0,1]
    out[48] = v[48];

    // D8 [49..56] season + time one-hot: identity
    out[49..57].copy_from_slice(&v[49..57]);

    // D9 [57..60] safety_override: identity
    out[57..61].copy_from_slice(&v[57..61]);

    // D10 [61..75] load_forecast_quantiles 15 维
    for i in 0..15 {
        out[61 + i] = minmax(v[61 + i], 0.0, 60.0);
    }

    // D10 [76] shock_load_probability: identity [0,1]
    out[76] = v[76];

    // D10 [77] base_load
    out[77] = minmax(v[77], 0.0, 60.0);

    out
}

/// MinMax 归一化: `(x - lo) / (hi - lo + 1e-9)`, clamp 到 [0, 1]
fn minmax(x: f32, lo: f32, hi: f32) -> f32 {
    let clipped = x.clamp(lo, hi);
    ((clipped - lo) / (hi - lo + 1e-9)).clamp(0.0, 1.0)
}

fn pad_or_truncate(vec: &[f64], target_len: usize) -> Vec<f64> {
    let mut result: Vec<f64> = vec.iter().take(target_len).copied().collect();
    while result.len() < target_len {
        result.push(0.0);
    }
    result
}

// ── 数据源适配器 ──

/// 数据源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    Realtime,
    Prediction,
    Price,
    Weather,
    Dispatch,
}

/// 数据源采集结果
#[derive(Debug, Clone)]
pub struct SourceData {
    pub source_type: SourceType,
    pub fetch_ts: i64,
}

/// 数据源适配器 trait
#[async_trait::async_trait]
pub trait DataSourceAdapter: Send + Sync {
    fn name(&self) -> &str;
    async fn fetch(&self) -> Result<SourceData, crate::error::AiEngineError>;
    fn source_type(&self) -> SourceType;
    fn timeout_ms(&self) -> u64;
}

/// 数据源健康状态
#[derive(Debug, Clone)]
pub struct SourceHealth {
    pub source_name: String,
    pub last_success_ts: i64,
    pub consecutive_failures: u32,
    pub status: HealthStatus,
}

impl SourceHealth {
    pub fn mark_success(&mut self) {
        self.consecutive_failures = 0;
        self.status = HealthStatus::Healthy;
    }

    pub fn mark_failure(&mut self) {
        self.consecutive_failures += 1;
        self.status = if self.consecutive_failures >= 10 {
            HealthStatus::Failed
        } else if self.consecutive_failures >= 3 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Failed,
}

/// 多源数据融合引擎
pub struct DataFusionEngine {
    pub fusion_period: Duration,
    pub last_fused_state: Arc<RwLock<Option<FusedSystemState>>>,
    pub sources: Vec<Box<dyn DataSourceAdapter>>,
    pub source_health: Vec<SourceHealth>,
    pub health_monitoring: bool,
}

impl DataFusionEngine {
    /// 单次融合：并行采集所有数据源，超时不影响其他源，缺失用上一周期值回填
    pub async fn fuse(&mut self) -> Result<FusedSystemState, crate::error::AiEngineError> {
        let mut fused = FusedSystemState::default();

        // 并行采集，各自超时独立
        let handles: Vec<_> = self
            .sources
            .iter()
            .map(|src| {
                let timeout = Duration::from_millis(src.timeout_ms());
                async move { tokio::time::timeout(timeout, src.fetch()).await }
            })
            .collect();

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            results.push(handle.await);
        }

        // 逐源填充 + 健康更新 + 缺失回填
        let prev = self.last_fused_state.read().await.clone();
        for (i, result) in results.iter().enumerate() {
            match result {
                Ok(Ok(_data)) => {
                    // 实际实现中根据 source_type 填充 fused 对应字段
                    self.source_health[i].mark_success();
                }
                _ => {
                    self.source_health[i].mark_failure();
                    // 使用上一周期值回填
                    if let Some(ref prev_state) = prev {
                        self.copy_fields_from_prev(
                            &mut fused,
                            prev_state,
                            self.sources[i].source_type(),
                        );
                    }
                }
            }
        }

        *self.last_fused_state.write().await = Some(fused.clone());
        Ok(fused)
    }

    fn copy_fields_from_prev(
        &self,
        fused: &mut FusedSystemState,
        prev: &FusedSystemState,
        st: SourceType,
    ) {
        match st {
            SourceType::Realtime => {
                fused.battery_soc = prev.battery_soc;
                fused.pv_power = prev.pv_power;
                fused.load_power = prev.load_power;
                fused.grid_power = prev.grid_power;
                fused.transformer_load = prev.transformer_load;
                fused.battery_power = prev.battery_power;
                fused.voltage_phase_a = prev.voltage_phase_a;
                fused.voltage_phase_b = prev.voltage_phase_b;
                fused.voltage_phase_c = prev.voltage_phase_c;
                fused.current_demand = prev.current_demand;
            }
            SourceType::Prediction => {
                fused.pv_forecast_15min = vec![0.0; 15]; // 全零向量
                fused.load_forecast_15min = vec![0.0; 15];
            }
            SourceType::Price => {
                fused.current_electricity_price = prev.current_electricity_price;
                fused.next_period_price = prev.next_period_price;
                fused.price_tariff_id = prev.price_tariff_id;
                fused.peak_price = prev.peak_price;
                fused.valley_price = prev.valley_price;
            }
            SourceType::Weather => {
                fused.solar_irradiance = prev.solar_irradiance;
                fused.temperature = prev.temperature;
            }
            SourceType::Dispatch => {
                // Dispatch 缺失时保持 None（不继承旧值）
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fused_state_default() {
        let state = FusedSystemState::default();
        assert_eq!(state.battery_soc, 0.5);
        assert_eq!(state.voltage_phase_a, 1.0);
        assert!(state.dispatch_p_set.is_none());
    }

    #[test]
    fn test_to_input_vector_v2_11_layout() {
        let mut state = FusedSystemState::default();
        state.pv_forecast_15min = vec![0.1; 15];
        state.load_forecast_15min = vec![0.2; 15];
        state.load_forecast_quantiles = vec![0.15; 15];
        state.shock_load_probability = 0.3;
        state.base_load = 100.0;
        let v = state.to_input_vector();
        assert_eq!(v.len(), 78); // v2.14: 76 → 78
                                 // D1 [0] = soc
        assert!((v[0] - 0.5_f32).abs() < 1e-6);
        // D1 [6] = voltage_a
        assert!((v[6] - 1.0_f32).abs() < 1e-6);
        // D2 pv_forecast [10]
        assert!((v[10] - 0.1_f32).abs() < 1e-6);
        // D6 [48] = dispatch_p_set (None → 0.0)
        assert!((v[48] - 0.0_f32).abs() < 1e-6);
        // D10 quantiles [61]
        assert!((v[61] - 0.15_f32).abs() < 1e-6);
        // D10 shock_load_probability [76]
        assert!((v[76] - 0.3_f32).abs() < 1e-6);
        // D10 base_load [77]
        assert!((v[77] - 100.0_f32).abs() < 1e-6);
    }

    #[test]
    fn test_validate_input_vector_clean() {
        let v = vec![1.0_f32; 76]; // v2.11: 59 → 76
        assert!(validate_input_vector(&v).is_ok());
    }

    #[test]
    fn test_validate_input_vector_nan_rejected() {
        let mut v = vec![1.0_f32; 76]; // v2.11: 59 → 76
        v[23] = f32::NAN;
        assert!(validate_input_vector(&v).is_err());
    }

    #[test]
    fn test_validate_input_vector_inf_rejected() {
        let mut v = vec![1.0_f32; 76]; // v2.11: 59 → 76
        v[10] = f32::INFINITY;
        assert!(validate_input_vector(&v).is_err());
    }

    #[test]
    fn test_pad_or_truncate() {
        // 短于目标 → 补零
        let v = pad_or_truncate(&[1.0, 2.0], 5);
        assert_eq!(v.len(), 5);
        assert_eq!(v[0], 1.0);
        assert_eq!(v[3], 0.0);
        // 长于目标 → 截断
        let v = pad_or_truncate(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3);
        assert_eq!(v.len(), 3);
        assert_eq!(v[2], 3.0);
    }

    #[test]
    fn test_fused_state_v2_5_new_fields() {
        let state = FusedSystemState::default();
        assert!((state.q_realtime_margin - 0.5).abs() < 1e-6);
        // 常规季默认
        assert!((state.season_encoding[3] - 1.0).abs() < 1e-6);
        // 白天默认
        assert!((state.time_period_encoding[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_to_input_vector_76_dim() {
        let mut state = FusedSystemState::default();
        state.pv_forecast_15min = vec![0.1; 15];
        state.load_forecast_15min = vec![0.2; 15];
        let v = state.to_input_vector();
        assert_eq!(v.len(), 78); // v2.14: 76 → 78
                                 // D7 q_realtime_margin 在索引 9
        assert!((v[9] - 0.5_f32).abs() < 1e-6);
        // D8 season_encoding[0] 在索引 50
        assert!((v[50] - 0.0_f32).abs() < 1e-6);
        // D9 safety_override (4维): active, p_ref, consecutive, ratio
        assert!((v[57] - 0.0_f32).abs() < 1e-6); // 默认 false → 0.0
        assert!((v[58] - 0.0_f32).abs() < 1e-6); // 默认 None → 0.0
        assert!((v[59] - 0.0_f32).abs() < 1e-6); // 默认 consecutive → 0.0
        assert!((v[60] - 0.0_f32).abs() < 1e-6); // 默认 ratio → 0.0
                                                 // D10 shock_load_probability [76]
        assert!((v[76] - 0.0_f32).abs() < 1e-6); // 默认 0.0
                                                 // D10 base_load [77]
        assert!((v[77] - 0.0_f32).abs() < 1e-6); // 默认 0.0
    }
}
