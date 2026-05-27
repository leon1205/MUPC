/// 削峰填谷配置
#[derive(Debug, Clone)]
pub struct PeakShavingConfig {
    /// 峰时时段
    pub peak_hours: Vec<(u8, u8)>,
    /// 谷时时段
    pub valley_hours: Vec<(u8, u8)>,
    /// SOC 充电上限
    pub soc_charge_max: f64,
    /// SOC 充电下限
    pub soc_charge_min: f64,
    /// 电池容量 (kWh)
    pub battery_capacity: f64,
}

impl Default for PeakShavingConfig {
    fn default() -> Self {
        Self {
            peak_hours: vec![(8, 11), (18, 21)], // 08:00-11:00, 18:00-21:00
            valley_hours: vec![(23, 7)],         // 23:00-07:00
            soc_charge_max: 80.0,
            soc_charge_min: 20.0,
            battery_capacity: 100.0,
        }
    }
}

/// 需量控制配置
#[derive(Debug, Clone)]
pub struct DemandControlConfig {
    /// 变压器容量 (kVA)
    pub transformer_capacity: f64,
    /// 需量因子
    pub demand_factor: f64,
    /// 预警阈值
    pub warning_threshold: f64,
    /// 行动阈值
    pub action_threshold: f64,
    /// 紧急阈值
    pub emergency_threshold: f64,
}

impl Default for DemandControlConfig {
    fn default() -> Self {
        Self {
            transformer_capacity: 500.0,
            demand_factor: 0.85,
            warning_threshold: 0.80,
            action_threshold: 0.90,
            emergency_threshold: 0.95,
        }
    }
}

/// 防逆流配置
#[derive(Debug, Clone)]
pub struct AntiReverseConfig {
    /// 逆功率阈值 (kW)
    pub reverse_power_threshold: f64,
    /// 光伏限制步长
    pub pv_limit_step: f64,
    /// 最大充电功率 (kW)
    pub max_charge_power: f64,
    /// SOC 充电上限
    pub soc_charge_max: f64,
}

impl Default for AntiReverseConfig {
    fn default() -> Self {
        Self {
            reverse_power_threshold: -0.1,
            pv_limit_step: 0.10,
            max_charge_power: 50.0,
            soc_charge_max: 80.0,
        }
    }
}