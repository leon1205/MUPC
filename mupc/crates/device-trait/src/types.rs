//! 公共类型定义
//!
//! 包含南向通信模块中使用的数据结构

use serde::{Deserialize, Serialize};
use std::hash::Hash;

/// 设备状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    /// 设备在线
    Online,
    /// 设备离线
    Offline,
    /// 设备故障
    Error(String),
}

impl DeviceStatus {
    /// 判断设备是否在线
    pub fn is_online(&self) -> bool {
        matches!(self, DeviceStatus::Online)
    }
}

/// 数据质量
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataQuality {
    /// 数据有效
    Good,
    /// 数据无效
    Invalid,
    /// 保留
    Reserved,
}

/// 设备数据帧
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFrame {
    /// 设备唯一标识
    pub device_id: String,
    /// 时间戳（毫秒）
    pub timestamp: u64,
    /// 数据载荷
    pub data: Vec<u8>,
    /// 数据质量
    pub quality: DataQuality,
}

impl DataFrame {
    /// 创建新的数据帧
    pub fn new(device_id: String, data: Vec<u8>) -> Self {
        Self {
            device_id,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            data,
            quality: DataQuality::Good,
        }
    }

    /// 创建带有时间戳的数据帧
    pub fn with_timestamp(device_id: String, timestamp: u64, data: Vec<u8>) -> Self {
        Self {
            device_id,
            timestamp,
            data,
            quality: DataQuality::Good,
        }
    }

    /// 设置数据质量
    pub fn with_quality(mut self, quality: DataQuality) -> Self {
        self.quality = quality;
        self
    }
}

/// 设备类型枚举
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    /// 配变终端
    Ttu,
    /// 光伏逆变器
    Inverter,
    /// 充电桩
    Charger,
    /// 柔性负荷
    FlexibleLoad,
    /// 消防控制
    FireAlarm,
    /// 未知类型
    Unknown,
}

impl DeviceType {
    /// 从字符串解析设备类型
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "ttu" => DeviceType::Ttu,
            "inverter" => DeviceType::Inverter,
            "charger" => DeviceType::Charger,
            "flexible_load" | "flexibleload" => DeviceType::FlexibleLoad,
            "fire_alarm" | "firealarm" => DeviceType::FireAlarm,
            _ => DeviceType::Unknown,
        }
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceType::Ttu => "ttu",
            DeviceType::Inverter => "inverter",
            DeviceType::Charger => "charger",
            DeviceType::FlexibleLoad => "flexible_load",
            DeviceType::FireAlarm => "fire_alarm",
            DeviceType::Unknown => "unknown",
        }
    }
}

/// 测量值
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measurement {
    /// 测量点名称
    pub name: String,
    /// 测量值
    pub value: f64,
    /// 单位
    pub unit: Option<String>,
}

impl Measurement {
    /// 创建新的测量值
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value,
            unit: None,
        }
    }

    /// 设置单位
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }
}

/// 消息主题
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Topic(String);

impl Topic {
    /// 创建新的主题
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// 获取主题字符串引用
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 获取主题字符串所有权
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for Topic {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for Topic {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 消息封装
#[derive(Debug, Clone)]
pub struct Message {
    /// 消息主题
    pub topic: Topic,
    /// 消息载荷
    pub payload: Vec<u8>,
    /// 时间戳（毫秒）
    pub timestamp: u64,
}

impl Message {
    /// 创建新的消息
    pub fn new(topic: Topic, payload: Vec<u8>) -> Self {
        Self {
            topic,
            payload,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    /// 从字符串主题创建消息
    pub fn with_topic(topic: impl Into<Topic>, payload: Vec<u8>) -> Self {
        Self {
            topic: topic.into(),
            payload,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        }
    }
}

/// 插件元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    /// 插件名称
    pub name: String,
    /// 插件版本
    pub version: String,
    /// 插件作者
    pub author: String,
    /// 插件描述
    pub description: String,
}

impl PluginMeta {
    /// 创建新的插件元信息
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        author: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            author: author.into(),
            description: description.into(),
        }
    }
}

/// RS485 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rs485Config {
    /// 串口设备路径
    pub port: String,
    /// 波特率
    pub baud_rate: u32,
    /// 数据位
    pub data_bits: u8,
    /// 停止位
    pub stop_bits: u8,
    /// 校验位
    pub parity: Parity,
    /// 通信超时（毫秒）
    pub timeout_ms: u64,
}

impl Default for Rs485Config {
    fn default() -> Self {
        Self {
            port: "/dev/ttyUSB0".to_string(),
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
            timeout_ms: 1000,
        }
    }
}

/// 校验位
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Parity {
    /// 无校验
    None,
    /// 偶校验
    Even,
    /// 奇校验
    Odd,
}

impl Parity {
    /// 转换为 serial 库使用的字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Parity::None => "none",
            Parity::Even => "even",
            Parity::Odd => "odd",
        }
    }
}