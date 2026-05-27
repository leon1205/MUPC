//! RS485 驱动错误类型

use thiserror::Error;

/// RS485 错误
#[derive(Debug, Error)]
pub enum Rs485Error {
    /// 串口打开失败
    #[error("串口打开失败: {0}")]
    OpenFailed(String),

    /// 串口配置失败
    #[error("串口配置失败: {0}")]
    ConfigFailed(String),

    /// 数据发送失败
    #[error("数据发送失败: {0}")]
    SendFailed(String),

    /// 数据接收失败
    #[error("数据接收失败: {0}")]
    RecvFailed(String),

    /// 串口读写超时
    #[error("串口读写超时")]
    Timeout,

    /// CRC 校验失败
    #[error("CRC 校验失败: {0}")]
    CrcFailed(String),

    /// 设备未连接
    #[error("设备未连接: {0}")]
    NotConnected(String),

    /// 串口 IO 错误
    #[error("串口 IO 错误: {0}")]
    IoError(#[from] std::io::Error),
}

impl Rs485Error {
    /// 创建打开失败错误
    pub fn open_failed(port: impl Into<String>) -> Self {
        Self::OpenFailed(format!("串口 {} 打开失败", port.into()))
    }

    /// 创建配置失败错误
    pub fn config_failed(msg: impl Into<String>) -> Self {
        Self::ConfigFailed(msg.into())
    }

    /// 创建发送失败错误
    pub fn send_failed(msg: impl Into<String>) -> Self {
        Self::SendFailed(msg.into())
    }

    /// 创建接收失败错误
    pub fn recv_failed(msg: impl Into<String>) -> Self {
        Self::RecvFailed(msg.into())
    }

    /// 创建 CRC 校验失败错误
    pub fn crc_failed(data: impl Into<String>) -> Self {
        Self::CrcFailed(data.into())
    }

    /// 创建未连接错误
    pub fn not_connected(device_id: impl Into<String>) -> Self {
        Self::NotConnected(device_id.into())
    }

    /// 创建协议错误
    pub fn new_protocol_error(msg: impl Into<String>) -> Self {
        Self::ConfigFailed(msg.into())
    }
}