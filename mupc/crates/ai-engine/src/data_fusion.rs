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

/// 融合系统状态（6 大类，21 RL 字段 + 3 辅助 = 24 字段）
#[derive(Debug, Clone)]
pub struct FusedSystemState {
    // ── D1: 实时数据 (9 RL + 1 aux) ──
    pub timestamp: i64,           // 辅助
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
    pub peak_price: f64,          // 辅助
    pub valley_price: f64,        // 辅助
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
            contract_demand: 500.0,
            peak_demand_this_month: 0.0,
            solar_irradiance: 0.0,
            temperature: 25.0,
            dispatch_p_set: None,
            dispatch_q_set: None,
        }
    }
}

impl FusedSystemState {
    /// 序列化为 48 维输入向量
    ///
    /// 布局:
    ///   [0..9]   D1 (9 标量)
    ///   [9..24]  D2 pv_forecast (15 维)
    ///   [24..39] D2 load_forecast (15 维)
    ///   [39..42] D3 (3 维)
    ///   [42..45] D4 (3 维)
    ///   [45..47] D5 (2 维)
    ///   [47]     D6 dispatch_p_set (None→0.0)
    pub fn to_input_vector(&self) -> Vec<f32> {
        let mut v = Vec::with_capacity(48);

        // [0..9] D1
        v.push(self.battery_soc as f32);
        v.push(self.pv_power as f32);
        v.push(self.load_power as f32);
        v.push(self.grid_power as f32);
        v.push(self.transformer_load as f32);
        v.push(self.battery_power as f32);
        v.push(self.voltage_phase_a as f32);
        v.push(self.voltage_phase_b as f32);
        v.push(self.voltage_phase_c as f32);

        // [9..24] D2 pv_forecast
        let pv = pad_or_truncate(&self.pv_forecast_15min, 15);
        v.extend(pv.iter().map(|&x| x as f32));

        // [24..39] D2 load_forecast
        let load = pad_or_truncate(&self.load_forecast_15min, 15);
        v.extend(load.iter().map(|&x| x as f32));

        // [39..42] D3
        v.push(self.current_electricity_price as f32);
        v.push(self.next_period_price as f32);
        v.push(self.price_tariff_id as f32);

        // [42..45] D4
        v.push(self.current_demand as f32);
        v.push(self.contract_demand as f32);
        v.push(self.peak_demand_this_month as f32);

        // [45..47] D5
        v.push(self.solar_irradiance as f32);
        v.push(self.temperature as f32);

        // [47] D6
        v.push(self.dispatch_p_set.unwrap_or(0.0) as f32);

        debug_assert_eq!(v.len(), 48, "输入向量必须为 48 维");
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
                        self.copy_fields_from_prev(&mut fused, prev_state, self.sources[i].source_type());
                    }
                }
            }
        }

        *self.last_fused_state.write().await = Some(fused.clone());
        Ok(fused)
    }

    fn copy_fields_from_prev(&self, fused: &mut FusedSystemState, prev: &FusedSystemState, st: SourceType) {
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
    fn test_to_input_vector_48_dim() {
        let mut state = FusedSystemState::default();
        state.pv_forecast_15min = vec![0.1; 15];
        state.load_forecast_15min = vec![0.2; 15];
        let v = state.to_input_vector();
        assert_eq!(v.len(), 48);
        // D1 [0] = soc
        assert!((v[0] - 0.5_f32).abs() < 1e-6);
        // D1 [6] = voltage_a
        assert!((v[6] - 1.0_f32).abs() < 1e-6);
        // D2 pv_forecast [9]
        assert!((v[9] - 0.1_f32).abs() < 1e-6);
        // D6 [47] = dispatch_p_set (None → 0.0)
        assert!((v[47] - 0.0_f32).abs() < 1e-6);
    }

    #[test]
    fn test_validate_input_vector_clean() {
        let v = vec![1.0_f32; 48];
        assert!(validate_input_vector(&v).is_ok());
    }

    #[test]
    fn test_validate_input_vector_nan_rejected() {
        let mut v = vec![1.0_f32; 48];
        v[23] = f32::NAN;
        assert!(validate_input_vector(&v).is_err());
    }

    #[test]
    fn test_validate_input_vector_inf_rejected() {
        let mut v = vec![1.0_f32; 48];
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
}
