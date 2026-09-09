//! 异常工况鲁棒性管理模块
//!
//! v2.9 新增：电压骤升/骤降、电池异常、通信中断应急策略

use crate::rl_model::ActionOutput;

/// 异常类型枚举
///
/// 注意：`CommunicationTimeout` 已移除（dead code）。
/// `FusedSystemState` 尚无通信时间戳字段，无法实现超时检测。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyType {
    /// 电压骤降（< 0.90 p.u.，阈值见 RobustnessManager::new）
    VoltageSag,
    /// 电压骤升（> 1.10 p.u.，阈值见 RobustnessManager::new）
    VoltageSurge,
    /// 电池SOC极低（< 5%）
    BatterySocCritical,
    /// 电池SOC过充（> 95%）
    BatterySocOverfull,
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
}

impl RobustnessManager {
    /// 创建新的 RobustnessManager 实例
    pub fn new() -> Self {
        Self {
            voltage_sag_threshold: 0.90,
            voltage_surge_threshold: 1.10,
            soc_critical_low: 0.10,
            soc_critical_high: 0.95,
        }
    }

    /// 检测异常类型
    ///
    /// 检测电压骤降（< 0.90 p.u.）、电压骤升（> 1.10 p.u.）、
    /// 电池SOC极低（< 10%）、电池SOC过充（> 95%）
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
            k_droop: 30.0,
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
    ///
    /// 注意：pv_limit = 0.0 停止所有光伏输入，用于防止电池过充。
    /// 在实际部署中可根据电网条件调整此值。
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
        // k_droop 语义与 sag 对称同为 +30（反向下垂体现在 p_ref 充电视符号上，非系数取负）：
        // v2.15 后 k_droop 动作区间默认 [0,30]，负值超出动作空间范围，故取正为设计意图。
        assert_eq!(action.k_droop, 30.0);
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

    #[test]
    fn test_no_anomaly_in_normal_range() {
        let rm = RobustnessManager::new();
        let state = make_state(1.0, 0.5); // normal voltage, normal SOC
        let anomalies = rm.detect_anomaly(&state);
        assert!(anomalies.is_empty(), "正常工况应无异常检测");
    }

    #[test]
    fn test_boundary_voltage_sag_threshold() {
        let rm = RobustnessManager::new();
        // 实际阈值 0.90（new() 取值），检测用严格小于：等于阈值不应触发
        let state = make_state(0.90, 0.5);
        let anomalies = rm.detect_anomaly(&state);
        assert!(
            !anomalies.contains(&AnomalyType::VoltageSag),
            "0.90 为阈值边界（< 才触发），不应触发"
        );
        // 略低于阈值（0.895 < 0.90）应触发 sag
        let state = make_state(0.895, 0.5);
        let anomalies = rm.detect_anomaly(&state);
        assert!(
            anomalies.contains(&AnomalyType::VoltageSag),
            "0.895 低于阈值 0.90 应触发"
        );
    }

    #[test]
    fn test_boundary_voltage_surge_threshold() {
        let rm = RobustnessManager::new();
        // 实际阈值 1.10（new() 取值），检测用严格大于：等于阈值不应触发
        let state = make_state(1.10, 0.5);
        let anomalies = rm.detect_anomaly(&state);
        assert!(
            !anomalies.contains(&AnomalyType::VoltageSurge),
            "1.10 为阈值边界（> 才触发），不应触发"
        );
        // 略高于阈值（1.105 > 1.10）应触发 surge
        let state = make_state(1.105, 0.5);
        let anomalies = rm.detect_anomaly(&state);
        assert!(
            anomalies.contains(&AnomalyType::VoltageSurge),
            "1.105 高于阈值 1.10 应触发"
        );
    }

    #[test]
    fn test_multiple_anomalies() {
        let rm = RobustnessManager::new();
        // 同时触发电压骤降和电池SOC极低
        let state = make_state(0.80, 0.03);
        let anomalies = rm.detect_anomaly(&state);
        assert!(anomalies.contains(&AnomalyType::VoltageSag));
        assert!(anomalies.contains(&AnomalyType::BatterySocCritical));
    }
}
