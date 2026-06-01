//! 纵向加密认证管理 (LEA - Longitudinal Encryption Authentication)
//!
//! 实现电力系统纵向加密认证，用于网关与调度主站之间的安全通信。
//! 管理 IPSec VPN 隧道的建立、密钥更新和状态监控。

use crate::errors::SecurityError;
use chrono::{DateTime, Utc};

/// 隧道状态
#[derive(Debug, Clone, PartialEq)]
pub enum TunnelState {
    Disconnected,
    Connecting,
    Established,
    Rekeying,
    Error(String),
}

/// 纵向加密配置
#[derive(Debug, Clone)]
pub struct LeaConfig {
    pub local_ip: String,
    pub remote_ip: String,
    pub pre_shared_key: Vec<u8>,
    pub rekey_interval_secs: u64,
    pub retry_interval_secs: u64,
}

impl Default for LeaConfig {
    fn default() -> Self {
        Self {
            local_ip: "0.0.0.0".into(),
            remote_ip: "0.0.0.0".into(),
            pre_shared_key: vec![],
            rekey_interval_secs: 3600,
            retry_interval_secs: 30,
        }
    }
}

/// 纵向加密管理器
pub struct LeaManager {
    config: LeaConfig,
    state: TunnelState,
    established_at: Option<DateTime<Utc>>,
    last_rekey_at: Option<DateTime<Utc>>,
}

impl LeaManager {
    pub fn new(config: LeaConfig) -> Self {
        Self {
            config,
            state: TunnelState::Disconnected,
            established_at: None,
            last_rekey_at: None,
        }
    }

    /// 建立加密隧道
    ///
    /// Phase 2+ 当前为 stub 实现，标记隧道已建立。
    /// 后续集成 strongSwan VICI 完成实际 IPSec 隧道建立。
    pub fn establish_tunnel(&mut self) -> Result<(), SecurityError> {
        if self.config.pre_shared_key.is_empty() {
            return Err(SecurityError::ConfigError(
                "预共享密钥未配置，无法建立加密隧道".into(),
            ));
        }

        tracing::info!(
            local = %self.config.local_ip,
            remote = %self.config.remote_ip,
            "纵向加密隧道已建立"
        );

        self.state = TunnelState::Established;
        self.established_at = Some(Utc::now());
        Ok(())
    }

    /// 关闭加密隧道
    pub fn close_tunnel(&mut self) -> Result<(), SecurityError> {
        if self.state == TunnelState::Disconnected {
            return Ok(());
        }

        tracing::info!("纵向加密隧道已关闭");
        self.state = TunnelState::Disconnected;
        self.established_at = None;
        Ok(())
    }

    /// 密钥更新
    ///
    /// 触发 IPSec IKE 重新协商。
    pub fn rekey(&mut self) -> Result<(), SecurityError> {
        if self.state != TunnelState::Established {
            return Err(SecurityError::TunnelError(
                "隧道未建立，无法执行密钥更新".into(),
            ));
        }

        tracing::info!("执行纵向加密密钥更新");
        self.state = TunnelState::Rekeying;
        self.last_rekey_at = Some(Utc::now());

        // Phase 2+: 通过 VICI 触发 strongSwan rekey
        tracing::info!("密钥更新完成");
        self.state = TunnelState::Established;
        Ok(())
    }

    /// 获取当前隧道状态
    pub fn tunnel_state(&self) -> &TunnelState {
        &self.state
    }

    /// 检查是否需要密钥更新
    pub fn needs_rekey(&self) -> bool {
        if let Some(last) = self.last_rekey_at {
            let elapsed = Utc::now() - last;
            elapsed.num_seconds() as u64 >= self.config.rekey_interval_secs
        } else if let Some(established) = self.established_at {
            let elapsed = Utc::now() - established;
            elapsed.num_seconds() as u64 >= self.config.rekey_interval_secs
        } else {
            false
        }
    }

    /// 加密传输层数据包
    ///
    /// Phase 2+: 通过 IPSec ESP 封装实现。
    /// 当前 stub 直接返回原始数据。
    pub fn encrypt_packet(&self, plaintext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        if self.state != TunnelState::Established {
            return Err(SecurityError::TunnelError(
                "隧道未建立，无法加密数据包".into(),
            ));
        }
        Ok(plaintext.to_vec())
    }

    /// 解密传输层数据包
    pub fn decrypt_packet(&self, ciphertext: &[u8]) -> Result<Vec<u8>, SecurityError> {
        if self.state != TunnelState::Established {
            return Err(SecurityError::TunnelError(
                "隧道未建立，无法解密数据包".into(),
            ));
        }
        Ok(ciphertext.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> LeaConfig {
        LeaConfig {
            local_ip: "192.168.1.1".into(),
            remote_ip: "10.0.0.1".into(),
            pre_shared_key: vec![1, 2, 3, 4],
            rekey_interval_secs: 3600,
            retry_interval_secs: 30,
        }
    }

    #[test]
    fn test_establish_and_close_tunnel() {
        let mut mgr = LeaManager::new(test_config());
        assert_eq!(*mgr.tunnel_state(), TunnelState::Disconnected);

        mgr.establish_tunnel().unwrap();
        assert_eq!(*mgr.tunnel_state(), TunnelState::Established);

        mgr.close_tunnel().unwrap();
        assert_eq!(*mgr.tunnel_state(), TunnelState::Disconnected);
    }

    #[test]
    fn test_establish_without_psk_fails() {
        let config = LeaConfig {
            pre_shared_key: vec![],
            ..test_config()
        };
        let mut mgr = LeaManager::new(config);
        assert!(mgr.establish_tunnel().is_err());
    }

    #[test]
    fn test_rekey_updates_state() {
        let mut mgr = LeaManager::new(test_config());
        mgr.establish_tunnel().unwrap();
        mgr.rekey().unwrap();
        assert_eq!(*mgr.tunnel_state(), TunnelState::Established);
    }

    #[test]
    fn test_rekey_without_tunnel_fails() {
        let mut mgr = LeaManager::new(test_config());
        assert!(mgr.rekey().is_err());
    }
}
