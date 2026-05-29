//! 遥测数据接口
//!
//! Phase 1 仅定义接口

use mupc_common::{MupcError, Value};
use async_trait::async_trait;

/// 数据包
#[derive(Debug, Clone)]
pub struct DataPackage {
    /// 电气量：U、I、P、Q、cosφ、频率
    pub electrical: ElectricalData,
    /// 电池数据：SOC、SOH、温度
    pub battery: BatteryData,
    /// 设备状态
    pub device_status: DeviceStatus,
    /// 时间戳（UTC）
    pub timestamp: u64,
}

/// 电气数据
#[derive(Debug, Clone)]
pub struct ElectricalData {
    pub voltage: Option<f64>,      // 电压 (V)
    pub current: Option<f64>,      // 电流 (A)
    pub active_power: Option<f64>, // 有功功率 (kW)
    pub reactive_power: Option<f64>, // 无功功率 (kVar)
    pub cos_phi: Option<f64>,      // 功率因数
    pub frequency: Option<f64>,    // 频率 (Hz)
}

/// 电池数据
#[derive(Debug, Clone)]
pub struct BatteryData {
    pub soc: Option<f64>,          // 荷电状态 (%)
    pub soh: Option<f64>,          // 健康状态 (%)
    pub temperature: Option<f64>,   // 温度 (°C)
}

/// 设备状态
#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub inverter_status: InverterStatus,
    pub pv_power: Option<f64>,      // 光伏功率 (kW)
    pub load_power: Option<f64>,    // 负荷功率 (kW)
    pub ev_charger_power: Option<f64>, // 充电桩功率 (kW)
}

/// 逆变器状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InverterStatus {
    Running,
    Stopped,
    Fault,
    Unknown,
}

/// 数据汇聚接口
///
/// 与设计文档对齐：
/// - `start()` — 启动数据采集
/// - `stop()` — 停止数据采集
/// - `get_latest_data()` — 获取最新遥测数据
///
/// 注意：原 `collect()` 方法保留为内部方法，不再作为 trait 方法暴露。
#[async_trait]
pub trait DataCollector: Send + Sync {
    /// 启动数据采集
    async fn start(&mut self) -> Result<(), MupcError>;

    /// 停止数据采集
    async fn stop(&mut self) -> Result<(), MupcError>;

    /// 获取最新遥测数据
    fn get_latest_data(&self) -> Option<TelemetryData>;
}

/// 遥测数据（与 DataPackage 区分，用于外部接口返回）
#[derive(Debug, Clone)]
pub struct TelemetryData {
    /// 电气量
    pub electrical: ElectricalData,
    /// 电池数据
    pub battery: BatteryData,
    /// 设备状态
    pub device_status: DeviceStatus,
    /// 时间戳（UTC）
    pub timestamp: u64,
}

impl From<DataPackage> for TelemetryData {
    fn from(pkg: DataPackage) -> Self {
        Self {
            electrical: pkg.electrical,
            battery: pkg.battery,
            device_status: pkg.device_status,
            timestamp: pkg.timestamp,
        }
    }
}

/// 高频遥测接口
#[async_trait]
pub trait HighFrequencyTelemetry: Send + Sync {
    /// 启动高频遥测
    async fn start(&self, period_ms: u64) -> Result<(), MupcError>;

    /// 停止高频遥测
    async fn stop(&self) -> Result<(), MupcError>;

    /// 获取当前状态
    fn is_running(&self) -> bool;

    /// 获取采集周期
    fn period(&self) -> u64;
}

/// 数据上报接口
///
/// 注意：此接口实现延后到 Phase 3B（消息总线扩展 AMQP/MQTT）
/// Phase 3A 仅保留接口定义
#[async_trait]
pub trait DataReporter: Send + Sync {
    /// 上报数据
    async fn report(&self, data: &DataPackage) -> Result<(), MupcError>;

    /// 获取协议类型
    fn protocol(&self) -> &str;
}

/// 故障条件
#[derive(Debug, Clone)]
pub struct FaultCondition {
    /// 过压阈值 (V)
    pub over_voltage: Option<f64>,
    /// 欠压阈值 (V)
    pub under_voltage: Option<f64>,
    /// 过流阈值 (A)
    pub over_current: Option<f64>,
    /// 频率异常阈值 (Hz)
    pub frequency_abnormal: Option<f64>,
}

/// 波形数据
#[derive(Debug, Clone)]
pub struct WaveformData {
    /// 通道数据
    pub channels: Vec<Vec<f64>>,
    /// 采样率
    pub sample_rate: u64,
    /// 触发时间戳
    pub trigger_timestamp: u64,
    /// 持续时间 (ms)
    pub duration_ms: u64,
}