//! MADDPG/PPO 强化学习决策模型
//!
//! 用于微电网能量管理决策
//! 输入：系统状态 (SOC, PV, Load, Grid, Transformer)
//! 输出：最优动作 (电池功率, 负荷切除, PV限制)

use crate::config::{ModelType, RlAlgorithm, RlConfig};
use crate::error::AiEngineError;
use crate::rknn_runtime::RknnRuntime;

/// 系统状态输入
#[derive(Debug, Clone)]
pub struct SystemState {
    /// 电池 SOC (0.0-1.0)
    pub battery_soc: f64,
    /// 光伏功率 (kW)
    pub pv_power: f64,
    /// 负荷功率 (kW)
    pub load_power: f64,
    /// 电网功率 (kW)
    pub grid_power: f64,
    /// 变压器负载 (kW)
    pub transformer_load: f64,
}

impl SystemState {
    /// 从特征向量构建状态
    pub fn from_features(features: &[f32]) -> Option<Self> {
        if features.len() < 5 {
            return None;
        }
        Some(Self {
            battery_soc: features[0] as f64,
            pv_power: features[1] as f64,
            load_power: features[2] as f64,
            grid_power: features[3] as f64,
            transformer_load: features[4] as f64,
        })
    }

    /// 转换为特征向量
    pub fn to_features(&self) -> Vec<f32> {
        vec![
            self.battery_soc as f32,
            self.pv_power as f32,
            self.load_power as f32,
            self.grid_power as f32,
            self.transformer_load as f32,
        ]
    }
}

/// RL 模型输出（决策动作）
#[derive(Debug, Clone)]
pub struct ActionOutput {
    /// 电池功率设定 (kW)
    pub p_batt_set: f64,
    /// 负荷切除 (kW)
    pub load_shedding: f64,
    /// PV 限功率 (0.0-1.0)
    pub pv_limit: f64,
    /// 决策置信度 (0.0-1.0)
    pub confidence: f64,
}

/// MADDPG/PPO 决策模型
pub struct RLModel {
    config: RlConfig,
    runtime: RknnRuntime,
}

impl RLModel {
    /// 创建 RL 模型
    pub fn new(config: RlConfig) -> Result<Self, AiEngineError> {
        let runtime = RknnRuntime::new(&config.model_path)?;
        Ok(Self { config, runtime })
    }

    /// 加载模型
    pub async fn load(&mut self) -> Result<(), AiEngineError> {
        self.runtime.load().await
    }

    /// 执行决策
    ///
    /// 输入：当前系统状态
    /// 输出：最优动作建议
    pub async fn decide(&self, state: &SystemState) -> Result<ActionOutput, AiEngineError> {
        // 检查模型是否已加载
        if !self.runtime.is_loaded().await {
            return Err(AiEngineError::ModelNotLoaded);
        }

        // 转换为特征向量
        let input = state.to_features();

        // 执行推理
        let output = self.runtime.run(&input).await?;

        // 解析输出
        // 输出格式: [p_batt_set, load_shedding, pv_limit, confidence, ...]
        let p_batt_set = output[0] as f64;
        let load_shedding = output[1] as f64;
        let pv_limit = (output[2] as f64).clamp(0.0, 1.0);
        let confidence = (output.get(3).copied().unwrap_or(0.8) as f64).clamp(0.0, 1.0);

        Ok(ActionOutput {
            p_batt_set,
            load_shedding,
            pv_limit,
            confidence,
        })
    }

    /// 获取模型类型
    pub fn model_type(&self) -> ModelType {
        match self.config.algorithm {
            RlAlgorithm::MADDPG => ModelType::MADDPG,
            RlAlgorithm::PPO => ModelType::PPO,
        }
    }

    /// 获取算法类型
    pub fn algorithm(&self) -> RlAlgorithm {
        self.config.algorithm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> RlConfig {
        RlConfig {
            model_path: std::path::PathBuf::from("/tmp/test_rl.rknn"),
            algorithm: RlAlgorithm::MADDPG,
            quantization: crate::config::QuantizationType::INT8,
        }
    }

    fn create_test_state() -> SystemState {
        SystemState {
            battery_soc: 0.5,
            pv_power: 10.0,
            load_power: 5.0,
            grid_power: 2.0,
            transformer_load: 20.0,
        }
    }

    #[test]
    fn test_rl_model_creation() {
        let config = create_test_config();
        let model = RLModel::new(config);
        assert!(model.is_ok());
    }

    #[test]
    fn test_rl_model_type() {
        let config = create_test_config();
        let model = RLModel::new(config).unwrap();
        assert_eq!(model.model_type(), ModelType::MADDPG);
    }

    #[test]
    fn test_system_state_to_features() {
        let state = create_test_state();
        let features = state.to_features();
        assert_eq!(features.len(), 5);
        assert_eq!(features[0], 0.5);
    }

    #[test]
    fn test_system_state_from_features() {
        let features = vec![0.5_f32, 10.0, 5.0, 2.0, 20.0];
        let state = SystemState::from_features(&features);
        assert!(state.is_some());
        let state = state.unwrap();
        assert_eq!(state.battery_soc, 0.5);
        assert_eq!(state.pv_power, 10.0);
    }
}
