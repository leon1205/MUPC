//! 服务协调器
//!
//! Phase 1 仅定义接口

use mupc_common::MupcError;

/// 服务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

/// 服务协调器 trait
pub trait ServiceCoordinator: Send + Sync {
    /// 启动所有服务
    fn start(&self) -> Result<(), MupcError>;

    /// 停止所有服务
    fn stop(&self) -> Result<(), MupcError>;

    /// 获取服务状态
    fn status(&self) -> ServiceStatus;

    /// 获取服务列表
    fn list_services(&self) -> Vec<String>;

    /// 获取特定服务状态
    fn service_status(&self, name: &str) -> Option<ServiceStatus>;
}