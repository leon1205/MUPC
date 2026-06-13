//! 异常工况鲁棒性管理模块
//!
//! v2.9 新增：电压骤升/骤降、电池异常、通信中断应急策略

use crate::rl_model::ActionOutput;

/// 异常类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyType {
    /// 电压骤降（< 0.85 p.u.）
    VoltageSag,
    /// 电压骤升（> 1.15 p.u.）
    VoltageSurge,
    /// 电池SOC极低（< 5%）
    BatterySocCritical,
    /// 电池SOC过充（> 95%）
    BatterySocOverfull,
    /// 通信超时（> 30s）
    CommunicationTimeout,
}

/// 异常工况检测与自适应策略管理器
pub struct RobustnessManager {
    /// 电压骤降阈值（p.u.）
    voltage_sag_threshold: f64,
    /// 电压骤升阈值（p.u.）
    voltage_surge_threshold: f64,
    /// SOC 极低阈值
    soc_critical_low: f64,
    /// SOC 过充阈值
    soc_critical_high: f64,
    /// 通信超时阈值（秒）
    comm_timeout_secs: u64,
}

impl RobustnessManager {
    pub fn new() -> Self {
        Self {
            voltage_sag_threshold: 0.85,
            voltage_surge_threshold: 1.15,
            soc_critical_low: 0.05,
            soc_critical_high: 0.95,
            comm_timeout_secs: 30,
        }
    }

    /// 检测异常类型
    pub fn detect_anomaly(&self, state: &crate::data_fusion::FusedSystemState) -> Vec<AnomalyType> {
        let mut anomalies = Vec::new();
        let v_avg = (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;

        if v_avg < self.voltage_sag_threshold {
            anomalies.push(AnomalyType::VoltageSag);
        } else if v_avg > self.voltage_surge_threshold {
            anomalies.push(AnomalyType::VoltageSurge);
        }

        if state.battery_soc < self.soc_critical_low {
            anomalies.push(AnomalyType::BatterySocCritical);
        } else if state.battery_soc > self.soc_critical_high {
            anomalies.push(AnomalyType::BatterySocOverfull);
        }

        anomalies
    }

    /// 获取应急动作（电压骤降：全功率放电 + 高灵敏度下垂）
    pub fn voltage_sag_action(&self) -> ActionOutput {
        ActionOutput {
            p_ref: 50.0,
            k_droop: 30.0,
            load_shedding: 0.0,
            pv_limit: 1.0,
            confidence: 1.0,
        }
    }

    /// 获取应急动作（电压骤升：全功率充电 + 反向下垂）
    pub fn voltage_surge_action(&self) -> ActionOutput {
        ActionOutput {
            p_ref: -50.0,
            k_droop: -30.0,
            load_shedding: 0.0,
            pv_limit: 0.5,
            confidence: 1.0,
        }
    }

    /// 获取电池SOC极低应急动作
    pub fn battery_soc_critical_action(&self) -> ActionOutput {
        ActionOutput {
            p_ref: 50.0, // 强制放电
            k_droop: 10.0,
            load_shedding: 0.0,
            pv_limit: 1.0,
            confidence: 1.0,
        }
    }

    /// 获取电池SOC过充应急动作
    pub fn battery_soc_overfull_action(&self) -> ActionOutput {
        ActionOutput {
            p_ref: -50.0, // 强制充电
            k_droop: 10.0,
            load_shedding: 0.0,
            pv_limit: 0.0, // 停止光伏输入
            confidence: 1.0,
        }
    }

    /// 根据异常类型返回应急动作
    pub fn get_robust_action(&self, anomaly: AnomalyType) -> ActionOutput {
        match anomaly {
            AnomalyType::VoltageSag => self.voltage_sag_action(),
            AnomalyType::VoltageSurge => self.voltage_surge_action(),
            AnomalyType::BatterySocCritical => self.battery_soc_critical_action(),
            AnomalyType::BatterySocOverfull => self.battery_soc_overfull_action(),
            AnomalyType::CommunicationTimeout => {
                // 通信中断：保持最后有效参数
                ActionOutput {
                    p_ref: 0.0,
                    k_droop: 10.0,
                    load_shedding: 0.0,
                    pv_limit: 1.0,
                    confidence: 0.0,
                }
            }
        }
    }
}

impl Default for RobustnessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(voltage: f64, soc: f64) -> crate::data_fusion::FusedSystemState {
        crate::data_fusion::FusedSystemState {
            voltage_phase_a: voltage,
            voltage_phase_b: voltage,
            voltage_phase_c: voltage,
            battery_soc: soc,
            ..Default::default()
        }
    }

    #[test]
    fn test_detect_voltage_sag() {
        let rm = RobustnessManager::new();
        let state = make_state(0.80, 0.5);
        let anomalies = rm.detect_anomaly(&state);
        assert!(anomalies.contains(&AnomalyType::VoltageSag));
    }

    #[test]
    fn test_detect_voltage_surge() {
        let rm = RobustnessManager::new();
        let state = make_state(1.20, 0.5);
        let anomalies = rm.detect_anomaly(&state);
        assert!(anomalies.contains(&AnomalyType::VoltageSurge));
    }

    #[test]
    fn test_voltage_sag_action_values() {
        let rm = RobustnessManager::new();
        let action = rm.voltage_sag_action();
        assert_eq!(action.p_ref, 50.0);
        assert_eq!(action.k_droop, 30.0);
        assert_eq!(action.confidence, 1.0);
    }

    #[test]
    fn test_voltage_surge_action_values() {
        let rm = RobustnessManager::new();
        let action = rm.voltage_surge_action();
        assert_eq!(action.p_ref, -50.0);
        assert_eq!(action.k_droop, -30.0);
        assert_eq!(action.pv_limit, 0.5);
    }

    #[test]
    fn test_battery_soc_critical_action() {
        let rm = RobustnessManager::new();
        let action = rm.battery_soc_critical_action();
        assert_eq!(action.p_ref, 50.0); // 强制放电
    }

    #[test]
    fn test_battery_soc_overfull_action() {
        let rm = RobustnessManager::new();
        let action = rm.battery_soc_overfull_action();
        assert_eq!(action.p_ref, -50.0); // 强制充电
        assert_eq!(action.pv_limit, 0.0); // 停止光伏
    }
}