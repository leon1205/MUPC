//! 负荷预测协变量（v2.11 新增）
//!
//! 用于 LSTM 分位数预测的输入协变量，包含温度、日期类型、季节等信息。

/// 负荷预测协变量（v2.11 新增）
///
/// 用于 LSTM 分位数预测的输入协变量结构体。
#[derive(Debug, Clone)]
pub struct LoadCovariates {
    /// 温度（摄氏度）
    pub temperature: f32,
    /// 日期类型（0=工作日，1=周末，2=节假日）
    pub date_type: u8,
    /// 是否灌溉季
    pub is_irrigation_season: bool,
    /// 小时（0-23）
    pub hour: u8,
}

impl Default for LoadCovariates {
    /// v3.1: 默认值代表典型工况（工作日正午 25°C，非灌溉季）。
    /// 实际推理时应通过 `WeatherService` trait 注入实时温湿度等协变量，
    /// 以改善分位数预测的协变量调整精度。
    fn default() -> Self {
        Self {
            temperature: 25.0,
            date_type: 0,   // 工作日
            is_irrigation_season: false,
            hour: 12,       // 正午
        }
    }
}

/// WeatherService trait（PLF-05 数据源定义）
///
/// 用于获取温度等气象协变量数据，供给 LSTM 分位数预测使用。
/// 实现者可对接气象局 API、本地传感器或 data_fusion.rs 的气象融合数据。
pub trait WeatherService: Send + Sync {
    /// 获取当前温度（摄氏度）
    fn get_current_temperature(&self) -> Result<f32, crate::error::AiEngineError>;

    /// 获取指定时间范围的温度预测
    ///
    /// # 参数
    /// - hours_ahead: 向前预测的小时数
    ///
    /// # 返回
    /// - 温度预测数组（按小时排序）
    fn get_temperature_forecast(
        &self,
        _hours_ahead: u32,
    ) -> Result<Vec<f32>, crate::error::AiEngineError> {
        // 默认实现：返回空数组，子类可重写
        Ok(Vec::new())
    }
}

/// WeatherService 默认实现（使用静态温度值）
pub struct DefaultWeatherService {
    default_temperature: f32,
}

impl DefaultWeatherService {
    pub fn new(default_temperature: f32) -> Self {
        Self {
            default_temperature,
        }
    }
}

impl WeatherService for DefaultWeatherService {
    fn get_current_temperature(&self) -> Result<f32, crate::error::AiEngineError> {
        Ok(self.default_temperature)
    }
}

/// data_fusion.rs 需提供此 trait（PLF-05）
pub trait DataFusionProvider: Send + Sync {
    fn get_fused_state(
        &self,
    ) -> Result<crate::data_fusion::FusedSystemState, crate::error::AiEngineError>;
}

/// WeatherService 实现（从 data_fusion.rs 获取气象融合数据）
pub struct DataFusionWeatherAdapter {
    data_fusion: std::sync::Arc<dyn DataFusionProvider>,
}

impl DataFusionWeatherAdapter {
    pub fn new(data_fusion: std::sync::Arc<dyn DataFusionProvider>) -> Self {
        Self { data_fusion }
    }
}

impl WeatherService for DataFusionWeatherAdapter {
    fn get_current_temperature(&self) -> Result<f32, crate::error::AiEngineError> {
        // 从 data_fusion 获取融合后的气象数据
        let state = self.data_fusion.get_fused_state()?;
        Ok(state.temperature as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_covariates_default() {
        let covariates = LoadCovariates::default();
        assert_eq!(covariates.temperature, 25.0);
        assert_eq!(covariates.date_type, 0);
        assert!(!covariates.is_irrigation_season);
        assert_eq!(covariates.hour, 12);
    }

    #[test]
    fn test_default_weather_service() {
        let service = DefaultWeatherService::new(30.0);
        let temp = service.get_current_temperature();
        assert!(temp.is_ok());
        assert_eq!(temp.unwrap(), 30.0);
    }
}
