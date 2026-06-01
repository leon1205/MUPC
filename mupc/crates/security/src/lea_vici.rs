//! strongSwan VICI 协议对接
//!
//! 通过 Unix Socket 与 strongSwan IKE 守护进程通信，管理 IPsec 隧道。
//!
//! VICI (Versatile IKE Configuration Interface) 是 strongSwan 的管理接口，
//! 使用 JSON 格式通过 Unix Socket 进行命令交互。

use crate::errors::SecurityError;
use crate::lea::TunnelState;

/// VICI 命令
#[derive(Debug)]
#[allow(dead_code)]
enum ViciCommand {
    ListSas,
    Initiate { peer: String },
    Terminate { peer: String },
    ReloadSettings,
    Version,
}

impl ViciCommand {
    fn to_message(&self) -> String {
        match self {
            ViciCommand::ListSas => "list-sas".into(),
            ViciCommand::Initiate { peer } => format!("initiate {{ child: \"{}\" }}", peer),
            ViciCommand::Terminate { peer } => format!("terminate {{ child: \"{}\" }}", peer),
            ViciCommand::ReloadSettings => "reload-settings".into(),
            ViciCommand::Version => "version".into(),
        }
    }
}

/// strongSwan VICI 客户端
///
/// Phase 2+ 当前为 stub 实现，提供命令行回退方式。
/// 后续完成 Unix Socket JSON 协议对接。
pub struct ViciClient {
    socket_path: String,
    connected: bool,
}

impl ViciClient {
    pub fn new(socket_path: &str) -> Self {
        Self {
            socket_path: socket_path.to_string(),
            connected: false,
        }
    }

    /// 连接到 strongSwan VICI socket
    ///
    /// Phase 2+: 建立 Unix Socket 连接，验证 strongSwan 版本。
    pub fn connect(&mut self) -> Result<(), SecurityError> {
        tracing::info!(
            socket = %self.socket_path,
            "连接 strongSwan VICI socket"
        );

        // Phase 2+: Unix Socket 连接实现
        // let stream = UnixStream::connect(&self.socket_path)?;
        self.connected = true;
        Ok(())
    }

    /// 断开 VICI 连接
    pub fn disconnect(&mut self) -> Result<(), SecurityError> {
        self.connected = false;
        tracing::info!("VICI 连接已断开");
        Ok(())
    }

    /// 列出所有安全关联 (SA)
    ///
    /// 每个 IKE SA 及其子 SA 的状态汇总。
    pub fn list_sas(&self) -> Result<Vec<TunnelState>, SecurityError> {
        if !self.connected {
            return Err(SecurityError::TunnelError("VICI 未连接".into()));
        }

        // Phase 2+: 解析 `list-sas` 命令返回的 JSON
        // 当前 stub: 返回空列表（无活跃 SA）
        tracing::debug!("查询活跃 SA 列表");
        Ok(vec![])
    }

    /// 初始化与指定对端的 IPsec 连接
    pub fn initiate(&self, peer: &str) -> Result<(), SecurityError> {
        if !self.connected {
            return Err(SecurityError::TunnelError("VICI 未连接".into()));
        }

        let cmd = ViciCommand::Initiate {
            peer: peer.to_string(),
        };
        tracing::info!(cmd = %cmd.to_message(), "发起 IPsec 连接");
        Ok(())
    }

    /// 终止与指定对端的 IPsec 连接
    pub fn terminate(&self, peer: &str) -> Result<(), SecurityError> {
        if !self.connected {
            return Err(SecurityError::TunnelError("VICI 未连接".into()));
        }

        let cmd = ViciCommand::Terminate {
            peer: peer.to_string(),
        };
        tracing::info!(cmd = %cmd.to_message(), "终止 IPsec 连接");
        Ok(())
    }

    /// 重新加载 strongSwan 配置
    pub fn reload_settings(&self) -> Result<(), SecurityError> {
        if !self.connected {
            return Err(SecurityError::TunnelError("VICI 未连接".into()));
        }

        tracing::info!("重新加载 strongSwan 配置");
        Ok(())
    }

    /// 检查连接状态
    pub fn is_connected(&self) -> bool {
        self.connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vici_connect_disconnect() {
        let mut client = ViciClient::new("/var/run/charon.vici");
        assert!(!client.is_connected());
        client.connect().unwrap();
        assert!(client.is_connected());
        client.disconnect().unwrap();
        assert!(!client.is_connected());
    }

    #[test]
    fn test_operations_require_connection() {
        let client = ViciClient::new("/var/run/charon.vici");
        assert!(client.list_sas().is_err());
        assert!(client.initiate("peer1").is_err());
        assert!(client.terminate("peer1").is_err());
        assert!(client.reload_settings().is_err());
    }
}
