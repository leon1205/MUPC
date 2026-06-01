//! MMS 客户端封装
//!
//! 使用 libIEC61850 实现 MMS 协议客户端

use crate::config::MmsConfig;
use crate::errors::{Iec61850Error, Result};
use crate::mms_types::{MmsRequest, MmsResponse};
use crate::asn1_utils::{encode_mms_request, decode_mms_response};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

/// MMS 客户端状态
#[derive(Debug, Clone, PartialEq)]
pub enum MmsClientState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// MMS 客户端
pub struct MmsClient {
    config: MmsConfig,
    state: Arc<parking_lot::RwLock<MmsClientState>>,
}

impl MmsClient {
    /// 创建 MMS 客户端
    pub fn new(config: MmsConfig) -> Self {
        Self {
            config,
            state: Arc::new(parking_lot::RwLock::new(MmsClientState::Disconnected)),
        }
    }

    /// 连接到 IED（短连接模式）
    pub async fn connect(&self) -> Result<()> {
        *self.state.write() = MmsClientState::Connecting;

        let addr = format!("{}:{}", self.config.remote_ip, self.config.remote_port);

        // 短连接：每次请求建立新连接
        let connect_result = timeout(
            Duration::from_millis(self.config.connect_timeout_ms),
            TcpStream::connect(&addr),
        )
        .await;

        match connect_result {
            Ok(Ok(_stream)) => {
                *self.state.write() = MmsClientState::Connected;
                Ok(())
            }
            Ok(Err(e)) => {
                *self.state.write() = MmsClientState::Error(e.to_string());
                Err(Iec61850Error::MmsConnectFailed(format!("连接失败: {}", e)))
            }
            Err(_) => {
                *self.state.write() = MmsClientState::Error("连接超时".to_string());
                Err(Iec61850Error::MmsTimeout("连接超时".into()))
            }
        }
    }

    /// 断开连接
    pub fn disconnect(&self) {
        *self.state.write() = MmsClientState::Disconnected;
    }

    /// 获取客户端状态
    pub fn get_state(&self) -> MmsClientState {
        self.state.read().clone()
    }

    /// 发送 MMS 请求
    async fn send_request(&self, request: MmsRequest) -> Result<MmsResponse> {
        if self.get_state() != MmsClientState::Connected {
            return Err(Iec61850Error::MmsConnectFailed("未连接".into()));
        }

        // 使用 ASN.1 编码请求
        let req_data = encode_mms_request(&request)
            .map_err(|e| Iec61850Error::Asn1EncodeFailed(e.to_string()))?;

        // 建立短连接
        let addr = format!("{}:{}", self.config.remote_ip, self.config.remote_port);
        let mut stream = timeout(
            Duration::from_millis(self.config.connect_timeout_ms),
            TcpStream::connect(&addr),
        )
        .await
        .map_err(|_| Iec61850Error::MmsTimeout("连接超时".into()))?
        .map_err(|e| Iec61850Error::MmsConnectFailed(format!("连接失败: {}", e)))?;

        // 发送请求
        stream.write_all(&req_data).await
            .map_err(|e| Iec61850Error::ProtocolError(format!("发送失败: {}", e)))?;

        // 读取响应
        let mut resp_buf = vec![0u8; 8192];
        let read_result = timeout(
            Duration::from_millis(self.config.read_timeout_ms),
            stream.read(&mut resp_buf),
        )
        .await;

        match read_result {
            Ok(Ok(n)) => {
                decode_mms_response(&resp_buf[..n])
            }
            Ok(Err(e)) => {
                Err(Iec61850Error::ProtocolError(format!("读取失败: {}", e)))
            }
            Err(_) => {
                Err(Iec61850Error::MmsTimeout("读取超时".into()))
            }
        }
    }

    /// 读取数据对象（Read 服务）
    pub async fn read_do(&self, ln: &str, do_name: &str) -> Result<Vec<u8>> {
        let request = MmsRequest::read(ln, do_name);

        let response = self.send_request(request).await?;

        if response.success {
            Ok(response.data)
        } else {
            Err(Iec61850Error::DataObjectNotFound(
                response.error.unwrap_or_default(),
            ))
        }
    }

    /// 写入数据对象（Write 服务）
    pub async fn write_do(&self, ln: &str, do_name: &str, value: &[u8]) -> Result<()> {
        let request = MmsRequest::write(ln, do_name, value.to_vec());

        let response = self.send_request(request).await?;

        if response.success {
            Ok(())
        } else {
            Err(Iec61850Error::WriteFailed(
                response.error.unwrap_or_default(),
            ))
        }
    }
}

/// MMS 客户端 trait（用于测试 mock）
#[async_trait]
pub trait MmsClientTrait: Send + Sync {
    async fn connect(&self) -> Result<()>;
    fn disconnect(&self);
    fn get_state(&self) -> MmsClientState;
    async fn read_do(&self, ln: &str, do_name: &str) -> Result<Vec<u8>>;
    async fn write_do(&self, ln: &str, do_name: &str, value: &[u8]) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mms_client_creation() {
        let config = MmsConfig::default();
        let client = MmsClient::new(config);
        assert_eq!(client.get_state(), MmsClientState::Disconnected);
    }

    #[test]
    fn test_mms_client_state_transitions() {
        let config = MmsConfig::default();
        let client = MmsClient::new(config);

        assert_eq!(client.get_state(), MmsClientState::Disconnected);

        client.disconnect();
        assert_eq!(client.get_state(), MmsClientState::Disconnected);
    }

    #[tokio::test]
    async fn test_mms_client_read_do_not_connected() {
        let config = MmsConfig::default();
        let client = MmsClient::new(config);

        let result = client.read_do("LLN0", "ST$Pos").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mms_client_write_do_not_connected() {
        let config = MmsConfig::default();
        let client = MmsClient::new(config);

        let result = client.write_do("LLN0", "ST$Pos", &[0x01]).await;
        assert!(result.is_err());
    }
}