//! 动作空间配置（可配置化）
//!
//! 提供 `ActionSpaceConfig` 结构体及完整校验规则（ASC-01~05）。
//! 向后兼容：未配置时使用默认值。

use serde::{Deserialize, Serialize};

/// 动作空间配置（可配置化）
///
/// 用于约束 RL 模型输出的动作值域，支持按台区（transformer_id）独立配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSpaceConfig {
    /// 台区 ID
    pub transformer_id: String,
    /// 正值，电池充电功率上限 (kW)
    pub max_batt_charge_power: f64,
    /// 正值，电池放电功率上限 (kW)
    pub max_batt_discharge_power: f64,
    /// 非负，切负荷上限 (kW)
    pub max_load_shedding: f64,
    /// 视在功率上限 (kVA)
    pub max_apparent_power_kva: f64,
    /// 有功变化率限制 (kW/s)
    pub p_batt_ramp_limit_kw: f64,
    /// 无功变化率限制 (kVar/s)
    pub q_batt_ramp_limit_kvar: f64,
    /// 光伏限功率下限
    pub pv_limit_min: f64,

    // === v2.6 新增字段 ===

    /// 变压器额定容量 (kVA)，从 YAML 锁定
    pub transformer_kva: f64,
    /// 电池总容量 (kWh)，从 YAML 锁定
    pub battery_capacity_kwh: f64,
    /// SOC 下限，从 YAML 锁定但可被 DB 覆盖
    pub soc_min: f64,
    /// SOC 上限，从 YAML 锁定但可被 DB 覆盖
    pub soc_max: f64,
    /// 变压器过载阈值，从 YAML 锁定但可被 DB 覆盖
    pub overload_threshold: f64,
}

impl ActionSpaceConfig {
    /// ASC-01: max_batt_charge_power > 0 && max_batt_discharge_power > 0
    pub fn asc_01(&self) -> bool {
        self.max_batt_charge_power > 0.0 && self.max_batt_discharge_power > 0.0
    }

    /// ASC-02: max_load_shedding >= 0
    pub fn asc_02(&self) -> bool {
        self.max_load_shedding >= 0.0
    }

    /// ASC-03: p_batt_ramp_limit_kw > 0 && q_batt_ramp_limit_kvar > 0
    pub fn asc_03(&self) -> bool {
        self.p_batt_ramp_limit_kw > 0.0 && self.q_batt_ramp_limit_kvar > 0.0
    }

    /// ASC-04: max_batt_charge_power <= max_apparent_power_kva
    pub fn asc_04(&self) -> bool {
        self.max_batt_charge_power <= self.max_apparent_power_kva
    }

    /// ASC-05: max_batt_discharge_power <= max_apparent_power_kva
    pub fn asc_05(&self) -> bool {
        self.max_batt_discharge_power <= self.max_apparent_power_kva
    }

    /// 完整校验（ASC-01 ~ ASC-05）
    ///
    /// 返回 Ok(()) 若全部通过，返回 Err(with violated rule) 若有校验失败。
    pub fn validate(&self) -> Result<(), String> {
        if !self.asc_01() {
            return Err(
                "ASC-01 failed: max_batt_charge_power > 0 && max_batt_discharge_power > 0".into(),
            );
        }
        if !self.asc_02() {
            return Err("ASC-02 failed: max_load_shedding >= 0".into());
        }
        if !self.asc_03() {
            return Err(
                "ASC-03 failed: p_batt_ramp_limit_kw > 0 && q_batt_ramp_limit_kvar > 0".into(),
            );
        }
        if !self.asc_04() {
            return Err("ASC-04 failed: max_batt_charge_power <= max_apparent_power_kva".into());
        }
        if !self.asc_05() {
            return Err("ASC-05 failed: max_batt_discharge_power <= max_apparent_power_kva".into());
        }
        Ok(())
    }

    /// 默认配置（向后兼容）
    ///
    /// 默认值：
    /// - max_batt_charge_power = 50.0 kW
    /// - max_batt_discharge_power = 50.0 kW
    /// - max_load_shedding = 60.0 kW
    /// - max_apparent_power_kva = 200.0 kVA
    /// - p_batt_ramp_limit_kw = 50.0 kW/s
    /// - q_batt_ramp_limit_kvar = 30.0 kVar/s
    /// - pv_limit_min = 0.1
    pub fn default_config() -> Self {
        Self {
            transformer_id: String::new(),
            max_batt_charge_power: 50.0,
            max_batt_discharge_power: 50.0,
            max_load_shedding: 60.0,
            max_apparent_power_kva: 200.0,
            p_batt_ramp_limit_kw: 50.0,
            q_batt_ramp_limit_kvar: 30.0,
            pv_limit_min: 0.1,
            // v2.6 新增默认值
            transformer_kva: 200.0,
            battery_capacity_kwh: 100.0,
            soc_min: 0.10,
            soc_max: 0.90,
            overload_threshold: 0.85,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_valid_config() -> ActionSpaceConfig {
        ActionSpaceConfig {
            transformer_id: "trafo_001".into(),
            max_batt_charge_power: 50.0,
            max_batt_discharge_power: 50.0,
            max_load_shedding: 60.0,
            max_apparent_power_kva: 200.0,
            p_batt_ramp_limit_kw: 50.0,
            q_batt_ramp_limit_kvar: 30.0,
            pv_limit_min: 0.1,
            transformer_kva: 200.0,
            battery_capacity_kwh: 100.0,
            soc_min: 0.10,
            soc_max: 0.90,
            overload_threshold: 0.85,
        }
    }

    #[test]
    fn test_asc_01_positive_power() {
        let mut cfg = make_valid_config();
        assert!(cfg.asc_01());

        cfg.max_batt_charge_power = 0.0;
        assert!(!cfg.asc_01());

        cfg.max_batt_charge_power = 50.0;
        cfg.max_batt_discharge_power = 0.0;
        assert!(!cfg.asc_01());
    }

    #[test]
    fn test_asc_02_load_shedding_non_negative() {
        let mut cfg = make_valid_config();
        assert!(cfg.asc_02());

        cfg.max_load_shedding = -1.0;
        assert!(!cfg.asc_02());
    }

    #[test]
    fn test_asc_03_ramp_limits_positive() {
        let mut cfg = make_valid_config();
        assert!(cfg.asc_03());

        cfg.p_batt_ramp_limit_kw = 0.0;
        assert!(!cfg.asc_03());

        cfg.p_batt_ramp_limit_kw = 50.0;
        cfg.q_batt_ramp_limit_kvar = 0.0;
        assert!(!cfg.asc_03());
    }

    #[test]
    fn test_asc_04_charge_within_apparent() {
        let mut cfg = make_valid_config();
        assert!(cfg.asc_04());

        cfg.max_batt_charge_power = 250.0; // 超过 S_max=200
        assert!(!cfg.asc_04());
    }

    #[test]
    fn test_asc_05_discharge_within_apparent() {
        let mut cfg = make_valid_config();
        assert!(cfg.asc_05());

        cfg.max_batt_discharge_power = 250.0; // 超过 S_max=200
        assert!(!cfg.asc_05());
    }

    #[test]
    fn test_validate_all_pass() {
        let cfg = make_valid_config();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_validate_returns_first_failure() {
        let mut cfg = make_valid_config();
        cfg.max_batt_charge_power = 0.0;
        cfg.max_load_shedding = -1.0;
        let result = cfg.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ASC-01"));
    }

    #[test]
    fn test_default_config() {
        let cfg = ActionSpaceConfig::default_config();
        assert_eq!(cfg.max_batt_charge_power, 50.0);
        assert_eq!(cfg.max_batt_discharge_power, 50.0);
        assert_eq!(cfg.max_load_shedding, 60.0);
        assert_eq!(cfg.max_apparent_power_kva, 200.0);
        assert_eq!(cfg.p_batt_ramp_limit_kw, 50.0);
        assert_eq!(cfg.q_batt_ramp_limit_kvar, 30.0);
        assert_eq!(cfg.pv_limit_min, 0.1);
    }
}
