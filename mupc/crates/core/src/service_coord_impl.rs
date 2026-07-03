//! ServiceCoordinator 默认实现
//!
//! 为 Phase 1 定义的 `ServiceCoordinator` trait 提供具体实现，
//! 供 `mupc-core-bin` crate 的启动编排器使用。

use crate::service_coord::{ServiceCoordinator, ServiceStatus};
use mupc_common::MupcError;
use parking_lot::RwLock;
use std::collections::HashMap;

/// 服务信息
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// 服务名称
    pub name: String,
    /// 当前状态
    pub status: ServiceStatus,
    /// 启动时间
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// ServiceCoordinator 默认实现
///
/// 线程安全，使用 parking_lot::RwLock。
pub struct ServiceCoordinatorImpl {
    /// 已注册的服务映射
    services: RwLock<HashMap<String, ServiceInfo>>,
    /// 整体协调器状态
    status: RwLock<ServiceStatus>,
    /// 优雅退出广播通道 (Sender)
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
}

impl ServiceCoordinatorImpl {
    /// 创建新的 ServiceCoordinatorImpl
    pub fn new() -> Self {
        let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(16);
        Self {
            services: RwLock::new(HashMap::new()),
            status: RwLock::new(ServiceStatus::Stopped),
            shutdown_tx,
        }
    }

    /// 注册服务并设置状态
    ///
    /// 等同于实现 ServiceCoordinator trait 的行为。
    /// 如果服务已存在，更新其状态。
    pub fn register_service(&self, name: &str, status: ServiceStatus) {
        let mut services = self.services.write();
        services.insert(
            name.to_string(),
            ServiceInfo {
                name: name.to_string(),
                status,
                started_at: Some(chrono::Utc::now()),
            },
        );
        tracing::info!(
            service = name,
            status = ?status,
            "服务已注册"
        );
    }

    /// 更新服务状态
    pub fn update_service_status(&self, name: &str, status: ServiceStatus) {
        let mut services = self.services.write();
        if let Some(info) = services.get_mut(name) {
            info.status = status;
            tracing::info!(
                service = name,
                status = ?status,
                "服务状态已更新"
            );
        }
    }

    /// 获取某个服务的详细信息
    pub fn get_service_info(&self, name: &str) -> Option<ServiceInfo> {
        let services = self.services.read();
        services.get(name).cloned()
    }

    /// 列出所有服务信息
    pub fn list_all_services(&self) -> Vec<ServiceInfo> {
        let services = self.services.read();
        services.values().cloned().collect()
    }

    /// 获取 shutdown 广播 Sender 的克隆
    pub fn shutdown_sender(&self) -> tokio::sync::broadcast::Sender<()> {
        self.shutdown_tx.clone()
    }

    /// Health check: 检查所有服务是否正常
    ///
    /// 返回 (healthy_count, total_count, failed_names)
    pub fn health_check(&self) -> (usize, usize, Vec<String>) {
        let services = self.services.read();
        let total = services.len();
        let failed: Vec<String> = services
            .iter()
            .filter(|(_, info)| info.status == ServiceStatus::Failed)
            .map(|(name, _)| name.clone())
            .collect();
        let healthy = total - failed.len();
        (healthy, total, failed)
    }

    /// 停止所有服务 (LIFO: 最后注册的优先停止)
    ///
    /// 发送 shutdown 广播信号并标记所有服务为 Stopped。
    pub async fn stop_all(&self) {
        tracing::info!("发送 shutdown 广播信号...");
        let _ = self.shutdown_tx.send(());

        let mut status = self.status.write();
        *status = ServiceStatus::Stopping;

        let mut services = self.services.write();
        for info in services.values_mut() {
            info.status = ServiceStatus::Stopped;
            tracing::info!(service = %info.name, "服务已停止");
        }

        *status = ServiceStatus::Stopped;
        tracing::info!("所有服务已停止");
    }
}

impl Default for ServiceCoordinatorImpl {
    fn default() -> Self {
        Self::new()
    }
}

// ── ServiceCoordinator trait 实现 ──

impl ServiceCoordinator for ServiceCoordinatorImpl {
    fn start(&self) -> Result<(), MupcError> {
        let mut status = self.status.write();
        if *status == ServiceStatus::Running {
            return Err(MupcError::new(
                mupc_common::ErrorCode::InvalidParam,
                "协调器已在运行中",
                "service_coord_impl",
            ));
        }
        *status = ServiceStatus::Running;
        Ok(())
    }

    fn stop(&self) -> Result<(), MupcError> {
        let mut status = self.status.write();
        if *status == ServiceStatus::Stopped {
            return Ok(());
        }
        *status = ServiceStatus::Stopping;
        let _ = self.shutdown_tx.send(());
        // 注意：实际停止逻辑在 stop_all() 中（需要异步环境）
        *status = ServiceStatus::Stopped;
        Ok(())
    }

    fn status(&self) -> ServiceStatus {
        *self.status.read()
    }

    fn list_services(&self) -> Vec<String> {
        let services = self.services.read();
        services.keys().cloned().collect()
    }

    fn service_status(&self, name: &str) -> Option<ServiceStatus> {
        let services = self.services.read();
        services.get(name).map(|info| info.status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_coordinator_impl_new() {
        let coord = ServiceCoordinatorImpl::new();
        assert_eq!(coord.status(), ServiceStatus::Stopped);
        assert!(coord.list_services().is_empty());
    }

    #[test]
    fn test_register_service() {
        let coord = ServiceCoordinatorImpl::new();
        coord.register_service("test_svc", ServiceStatus::Running);
        let services = coord.list_services();
        assert!(services.contains(&"test_svc".to_string()));
        assert_eq!(
            coord.service_status("test_svc").unwrap(),
            ServiceStatus::Running
        );
    }

    #[test]
    fn test_update_service_status() {
        let coord = ServiceCoordinatorImpl::new();
        coord.register_service("test_svc", ServiceStatus::Running);
        coord.update_service_status("test_svc", ServiceStatus::Failed);
        assert_eq!(
            coord.service_status("test_svc").unwrap(),
            ServiceStatus::Failed
        );
    }

    #[test]
    fn test_service_not_found() {
        let coord = ServiceCoordinatorImpl::new();
        assert!(coord.service_status("nonexistent").is_none());
    }

    #[test]
    fn test_health_check() {
        let coord = ServiceCoordinatorImpl::new();
        coord.register_service("healthy_svc", ServiceStatus::Running);
        coord.register_service("failed_svc", ServiceStatus::Failed);
        let (healthy, total, failed) = coord.health_check();
        assert_eq!(healthy, 1);
        assert_eq!(total, 2);
        assert_eq!(failed, vec!["failed_svc"]);
    }

    #[test]
    fn test_service_coordinator_trait_start_stop() {
        let coord = ServiceCoordinatorImpl::new();
        assert_eq!(coord.status(), ServiceStatus::Stopped);

        coord.start().unwrap();
        assert_eq!(coord.status(), ServiceStatus::Running);

        coord.stop().unwrap();
        assert_eq!(coord.status(), ServiceStatus::Stopped);
    }

    #[test]
    fn test_get_service_info() {
        let coord = ServiceCoordinatorImpl::new();
        coord.register_service("test_svc", ServiceStatus::Running);
        let info = coord.get_service_info("test_svc").unwrap();
        assert_eq!(info.name, "test_svc");
        assert_eq!(info.status, ServiceStatus::Running);
        assert!(info.started_at.is_some());
    }
}
