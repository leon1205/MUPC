use thiserror::Error;

#[derive(Debug, Error)]
pub enum SimBridgeError {
    #[error("配置错误: {0}")]
    Config(String),

    #[error("MQTT 错误: {0}")]
    Mqtt(String),

    #[error("MQTT EventLoop 异常退出")]
    MqttEventLoopLost,

    #[error("TCP 错误: {0}")]
    Tcp(#[from] std::io::Error),

    #[error("Python 引擎错误: {0}")]
    PyEngine(String),

    #[error("Python 引擎超时")]
    PyEngineTimeout,

    #[error("Python 引擎 EOF (stdout 关闭)")]
    PyEngineEof,

    #[error("Python 引擎重启次数超限 ({0})")]
    PyEngineMaxRestarts(u32),

    #[error("协议错误: {0}")]
    Protocol(String),

    #[error("CRC 校验失败: expected={expected:#06x}, actual={actual:#06x}")]
    CrcMismatch { expected: u16, actual: u16 },

    #[error("序列化错误: {0}")]
    Serialize(#[from] serde_json::Error),
}
