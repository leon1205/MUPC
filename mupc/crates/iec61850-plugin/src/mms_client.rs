//! MMS 客户端封装
//!
//! 实现 MMS 协议客户端，用于 IEC 61850 数据访问

use crate::config::MmsConfig;
use crate::errors::{Iec61850Error, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;

/// MMS 客户端状态
#[derive(Debug, Clone, PartialEq)]
pub enum MmsClientState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// MMS 请求
#[derive(Debug, Clone)]
pub struct MmsRequest {
    pub service: MmsService,
    pub object: String,
    pub payload: Vec<u8>,
}

/// MMS 服务类型
#[derive(Debug, Clone)]
pub enum MmsService {
    Read,
    Write,
    DefineVariableAccess,
    GetDataAccessAttributes,
}

/// MMS 响应
#[derive(Debug, Clone)]
pub struct MmsResponse {
    pub success: bool,
    pub data: Vec<u8>,
    pub error: Option<String>,
}

/// MMS 客户端
pub struct MmsClient {
    config: MmsConfig,
    state: Arc<RwLock<MmsClientState>>,
    connection: Arc<RwLock<Option<TcpStream>>>,
}

impl MmsClient {
    /// 创建 MMS 客户端
    pub fn new(config: MmsConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(MmsClientState::Disconnected)),
            connection: Arc::new(RwLock::new(None)),
        }
    }

    /// 连接到远程 IED
    pub async fn connect(&self) -> Result<()> {
        let addr = format!("{}:{}", self.config.remote_ip, self.config.remote_port);

        let stream = tokio::net::TcpStream::connect(&addr)
            .await
            .map_err(|e| Iec61850Error::MmsConnectFailed(format!("连接失败: {}", e)))?;

        let mut state = self.state.write().await;
        *state = MmsClientState::Connected;

        let mut conn = self.connection.write().await;
        *conn = Some(stream);

        Ok(())
    }

    /// 断开连接
    pub async fn disconnect(&self) -> Result<()> {
        let mut state = self.state.write().await;
        *state = MmsClientState::Disconnected;

        let mut conn = self.connection.write().await;
        *conn = None;

        Ok(())
    }

    /// 获取客户端状态
    pub async fn get_state(&self) -> MmsClientState {
        self.state.read().await.clone()
    }

    /// 发送 MMS 请求
    pub async fn send_request(&self, request: MmsRequest) -> Result<MmsResponse> {
        let state = self.state.read().await;
        if *state != MmsClientState::Connected {
            return Err(Iec61850Error::MmsConnectFailed("未连接".to_string()));
        }
        drop(state);

        let mut conn = self.connection.write().await;
        if let Some(ref mut stream) = *conn {
            // 序列化请求
            let req_data = self.encode_request(&request)?;
            stream.write_all(&req_data).await
                .map_err(|e| Iec61850Error::ProtocolError(format!("发送失败: {}", e)))?;

            // 读取响应
            let mut resp_buf = [0u8; 4096];
            let n = stream.read(&mut resp_buf).await
                .map_err(|e| Iec61850Error::ProtocolError(format!("读取失败: {}", e)))?;

            self.decode_response(&resp_buf[..n])
        } else {
            Err(Iec61850Error::MmsConnectFailed("连接不存在".to_string()))
        }
    }

    /// 编码 MMS 请求
    fn encode_request(&self, _request: &MmsRequest) -> Result<Vec<u8>> {
        // 简化实现：构建简单 MMS PDU
        let mut buf = Vec::new();
        buf.push(0x81); // MMS PDU tag: confirmed-RequestPDU
        buf.push(0x01); // invokeId present
        Ok(buf)
    }

    /// 解码 MMS 响应
    fn decode_response(&self, data: &[u8]) -> Result<MmsResponse> {
        if data.is_empty() {
            return Err(Iec61850Error::MmsInvalidResponse("空响应".to_string()));
        }

        // 检查响应类型
        if data[0] == 0x81 {
            Ok(MmsResponse {
                success: true,
                data: data.to_vec(),
                error: None,
            })
        } else {
            Err(Iec61850Error::MmsInvalidResponse(format!("未知响应类型: {:02x}", data[0])))
        }
    }

    /// 读取数据对象（Read 服务）
    pub async fn read_do(&self, ln: &str, do_name: &str) -> Result<Vec<u8>> {
        let request = MmsRequest {
            service: MmsService::Read,
            object: format!("{}/{}", ln, do_name),
            payload: Vec::new(),
        };

        let response = self.send_request(request).await?;

        if response.success {
            Ok(response.data)
        } else {
            Err(Iec61850Error::DataObjectNotFound(response.error.unwrap_or_default()))
        }
    }

    /// 写入数据对象（Write 服务）
    pub async fn write_do(&self, ln: &str, do_name: &str, value: &[u8]) -> Result<()> {
        let request = MmsRequest {
            service: MmsService::Write,
            object: format!("{}/{}", ln, do_name),
            payload: value.to_vec(),
        };

        let response = self.send_request(request).await?;

        if response.success {
            Ok(())
        } else {
            Err(Iec61850Error::WriteFailed(response.error.unwrap_or_default()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mms_client_creation() {
        let config = MmsConfig::default();
        let client = MmsClient::new(config);
        assert_eq!(config.local_port, 102);
    }

    #[tokio::test]
    async fn test_mms_client_disconnected_state() {
        let config = MmsConfig::default();
        let client = MmsClient::new(config);
        let state = client.get_state().await;
        assert_eq!(state, MmsClientState::Disconnected);
    }

    #[test]
    fn test_encode_request() {
        let config = MmsConfig::default();
        let client = MmsClient::new(config);
        let request = MmsRequest {
            service: MmsService::Read,
            object: "LLN0$ST$Pos".to_string(),
            payload: vec![],
        };

        let encoded = client.encode_request(&request);
        assert!(encoded.is_ok());
        assert!(!encoded.unwrap().is_empty());
    }
}