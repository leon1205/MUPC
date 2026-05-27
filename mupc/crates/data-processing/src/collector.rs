//! 数据收集器实现
//! 从 intercore 模块接收实时控制模块的数据

use crate::errors::DataProcessingError;
use crate::telemetry::{DataPackage, ElectricalData, BatteryData, DeviceStatus, InverterStatus};
use async_trait::async_trait;
use mupc_common::MupcError;
use std::sync::Arc;
use tokio::sync::mpsc;

/// 数据收集器实现
/// 从 intercore 模块接收实时控制模块的数据
pub struct DataCollectorImpl {
    /// 数据接收通道（从 intercore）
    receiver: Option<mpsc::Receiver<DataPackage>>,
    /// 最新数据缓存
    latest_data: Arc<std::sync::Mutex<Option<DataPackage>>>,
}

impl DataCollectorImpl {
    pub fn new() -> Self {
        Self {
            receiver: None,
            latest_data: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// 从 intercore 接收数据（模拟实现）
    pub async fn try_collect(&mut self) -> Result<DataPackage, DataProcessingError> {
        if let Some(receiver) = &mut self.receiver {
            if let Some(data) = receiver.recv().await {
                let mut latest = self.latest_data.lock().unwrap();
                *latest = Some(data.clone());
                return Ok(data);
            }
        }
        // 模拟数据（实际从 intercore 接收）
        Ok(self.generate_mock_data())
    }

    fn generate_mock_data() -> DataPackage {
        DataPackage {
            electrical: ElectricalData {
                voltage: Some(380.0),
                current: Some(100.0),
                active_power: Some(50.0),
                reactive_power: Some(10.0),
                cos_phi: Some(0.98),
                frequency: Some(50.0),
            },
            battery: BatteryData {
                soc: Some(75.0),
                soh: Some(95.0),
                temperature: Some(35.0),
            },
            device_status: DeviceStatus {
                inverter_status: InverterStatus::Running,
                pv_power: Some(30.0),
                load_power: Some(40.0),
                ev_charger_power: Some(10.0),
            },
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }

    pub fn get_latest_data(&self) -> Option<DataPackage> {
        self.latest_data.lock().unwrap().clone()
    }
}

impl Default for DataCollectorImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl mupc_data_processing::telemetry::DataCollector for DataCollectorImpl {
    async fn collect(&self) -> Result<DataPackage, MupcError> {
        // 实现逻辑
        Ok(self.generate_mock_data())
    }

    fn name(&self) -> &str {
        "DataCollectorImpl"
    }
}