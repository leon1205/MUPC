//! strongSwan VICI 协议对接
//!
//! 通过 Unix Socket 与 strongSwan IKE 守护进程通信，管理 IPsec 隧道

use crate::errors::SecurityError;
use crate::lea::TunnelState;

/// strongSwan VICI 客户端（Phase 2+ 实现）
pub struct ViciClient {
    socket_path: String,
}

impl ViciClient {
    pub fn new(socket_path: &str) -> Self {
        todo!("Phase 2+")
    }

    pub fn connect(&mut self) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn disconnect(&mut self) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn list_sas(&self) -> Result<Vec<TunnelState>, SecurityError> {
        todo!("Phase 2+")
    }

    pub fn initiate(&self, peer: &str) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn terminate(&self, peer: &str) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn reload_settings(&self) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }
}
