//! 统一错误类型定义
//!
//! 所有南向通信模块的错误类型都实现 std::error::Error

use thiserror::Error;

/// 设备错误
#[derive(Debug, Error)]
#[error("设备离线: {0}")]
pub struct DeviceOfflineError(pub String);

/// 设备错误
#[derive(Debug, Error)]
pub enum DeviceError {
    /// 设备离线
    #[error("设备离线: {0}")]
    Offline(String),

    /// 通信超时
    #[error("通信超时: {0}")]
    Timeout(String),

    /// 数据校验失败
    #[error("数据校验失败: {0}")]
    ChecksumFailed(String),

    /// 协议错误
    #[error("协议错误: {0}")]
    ProtocolError(String),

    /// 设备忙
    #[error("设备忙: {0}")]
    Busy(String),

    /// 串口操作失败
    #[error("串口操作失败: {0}")]
    IoError(#[from] std::io::Error),

    /// 其他设备错误
    #[error("设备错误: {0}")]
    Other(String),
}

impl DeviceError {
    /// 创建离线错误
    pub fn offline(device_id: impl Into<String>) -> Self {
        Self::Offline(device_id.into())
    }

    /// 创建超时错误
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout(msg.into())
    }

    /// 创建校验失败错误
    pub fn checksum_failed(msg: impl Into<String>) -> Self {
        Self::ChecksumFailed(msg.into())
    }

    /// 创建协议错误
    pub fn protocol_error(msg: impl Into<String>) -> Self {
        Self::ProtocolError(msg.into())
    }

    /// 创建设备忙错误
    pub fn busy(device_id: impl Into<String>) -> Self {
        Self::Busy(device_id.into())
    }
}

/// 插件错误
#[derive(Debug, Error)]
pub enum PluginError {
    /// 插件加载失败
    #[error("插件加载失败: {0}")]
    LoadFailed(String),

    /// 插件初始化失败
    #[error("插件初始化失败: {0}")]
    InitFailed(String),

    /// 插件启动失败
    #[error("插件启动失败: {0}")]
    StartFailed(String),

    /// 插件停止失败
    #[error("插件停止失败: {0}")]
    StopFailed(String),

    /// 插件不存在
    #[error("插件不存在: {0}")]
    NotFound(String),

    /// 插件元信息错误
    #[error("插件元信息错误: {0}")]
    MetaError(String),

    /// 其他插件错误
    #[error("插件错误: {0}")]
    Other(String),
}

impl PluginError {
    /// 创建加载失败错误
    pub fn load_failed(msg: impl Into<String>) -> Self {
        Self::LoadFailed(msg.into())
    }

    /// 创建初始化失败错误
    pub fn init_failed(msg: impl Into<String>) -> Self {
        Self::InitFailed(msg.into())
    }

    /// 创建启动失败错误
    pub fn start_failed(msg: impl Into<String>) -> Self {
        Self::StartFailed(msg.into())
    }

    /// 创建停止失败错误
    pub fn stop_failed(msg: impl Into<String>) -> Self {
        Self::StopFailed(msg.into())
    }

    /// 创建不存在错误
    pub fn not_found(name: impl Into<String>) -> Self {
        Self::NotFound(name.into())
    }
}

/// 总线错误
#[derive(Debug, Error)]
pub enum BusError {
    /// 主题不存在
    #[error("主题不存在: {0}")]
    TopicNotFound(String),

    /// 发布失败
    #[error("发布失败: {0}")]
    PublishFailed(String),

    /// 订阅失败
    #[error("订阅失败: {0}")]
    SubscribeFailed(String),

    /// 取消订阅失败
    #[error("取消订阅失败: {0}")]
    UnsubscribeFailed(String),

    /// 其他总线错误
    #[error("总线错误: {0}")]
    Other(String),
}

impl BusError {
    /// 创建主题不存在错误
    pub fn topic_not_found(topic: impl Into<String>) -> Self {
        Self::TopicNotFound(topic.into())
    }

    /// 创建发布失败错误
    pub fn publish_failed(msg: impl Into<String>) -> Self {
        Self::PublishFailed(msg.into())
    }

    /// 创建订阅失败错误
    pub fn subscribe_failed(msg: impl Into<String>) -> Self {
        Self::SubscribeFailed(msg.into())
    }
}

/// 注册表错误
#[derive(Debug, Error)]
pub enum RegistryError {
    /// 设备已存在
    #[error("设备已存在: {0}")]
    AlreadyExists(String),

    /// 设备不存在
    #[error("设备不存在: {0}")]
    NotFound(String),

    /// 注册失败
    #[error("注册失败: {0}")]
    RegisterFailed(String),

    /// 注销失败
    #[error("注销失败: {0}")]
    UnregisterFailed(String),

    /// 其他注册表错误
    #[error("注册表错误: {0}")]
    Other(String),
}

impl RegistryError {
    /// 创建已存在错误
    pub fn already_exists(device_id: impl Into<String>) -> Self {
        Self::AlreadyExists(device_id.into())
    }

    /// 创建不存在错误
    pub fn not_found(device_id: impl Into<String>) -> Self {
        Self::NotFound(device_id.into())
    }

    /// 创建注册失败错误
    pub fn register_failed(msg: impl Into<String>) -> Self {
        Self::RegisterFailed(msg.into())
    }
}