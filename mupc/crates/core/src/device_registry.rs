//! 设备注册表
//!
//! Phase 1 仅定义接口

use mupc_common::MupcError;
use std::any::Any;
use std::fmt::Debug;

/// 数据质量
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DataQuality {
    Good,
    Uncertain,
    Bad,
}

/// 数据值
#[derive(Debug, Clone)]
pub enum Value {
    Bool(bool),
    F64(f64),
    String(String),
}

/// 读请求
#[derive(Debug, Clone)]
pub struct ReadRequest {
    pub point_id: String,
    pub timeout_ms: u64,
}

/// 读响应
#[derive(Debug, Clone)]
pub struct ReadResponse {
    pub value: Value,
    pub timestamp: u64,
    pub quality: DataQuality,
}

/// 写请求
#[derive(Debug, Clone)]
pub struct WriteRequest {
    pub point_id: String,
    pub value: Value,
    pub timeout_ms: u64,
}

/// 写响应
#[derive(Debug, Clone)]
pub struct WriteResponse {
    pub success: bool,
    pub timestamp: u64,
}

/// 控制命令
#[derive(Debug, Clone)]
pub struct ControlCommand {
    pub cmd_id: u16,
    pub cmd_type: u8,
    pub priority: u8,
    pub params: String, // JSON
}

/// 控制响应
#[derive(Debug, Clone)]
pub struct ControlResponse {
    pub cmd_id: u16,
    pub success: bool,
    pub message: String,
    pub timestamp: u64,
}

/// 健康状态
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub healthy: bool,
    pub cpu_temp: Option<f64>,
    pub memory_usage: Option<f64>,
    pub details: String,
}

/// 设备错误
#[derive(Debug)]
pub enum DeviceError {
    Timeout,
    DeviceOffline,
    InvalidPoint,
    ProtocolError(String),
    Other(Box<dyn std::error::Error>),
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceError::Timeout => write!(f, "Device operation timeout"),
            DeviceError::DeviceOffline => write!(f, "Device is offline"),
            DeviceError::InvalidPoint => write!(f, "Invalid data point"),
            DeviceError::ProtocolError(msg) => write!(f, "Protocol error: {}", msg),
            DeviceError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for DeviceError {}

/// 设备抽象 trait
pub trait Device: Send + Sync {
    /// 设备类型标识
    fn device_type(&self) -> &'static str;

    /// 设备 ID
    fn device_id(&self) -> &str;

    /// 读取设备数据
    async fn read(&self, req: &ReadRequest) -> Result<ReadResponse, DeviceError>;

    /// 写入设备数据
    async fn write(&self, req: &WriteRequest) -> Result<WriteResponse, DeviceError>;

    /// 下发控制指令
    async fn control(&self, cmd: &ControlCommand) -> Result<ControlResponse, DeviceError>;

    /// 设备健康检查
    async fn health_check(&self) -> Result<HealthStatus, DeviceError>;

    /// 转换为 Any 类型
    fn as_any(&self) -> &dyn Any;
}

/// 设备注册表 trait
pub trait DeviceRegistry: Send + Sync {
    /// 注册设备
    fn register(&self, device: Box<dyn Device>) -> Result<(), MupcError>;

    /// 注销设备
    fn unregister(&self, id: &str) -> Result<(), MupcError>;

    /// 获取设备
    fn get(&self, id: &str) -> Option<Box<dyn Device>>;

    /// 列出所有设备
    fn list(&self) -> Vec<String>;

    /// 检查设备是否存在
    fn contains(&self, id: &str) -> bool;
}