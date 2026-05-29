//! 安全事件告警
//!
//! 安全事件告警管理，支持多通道上报（日志/MQTT/IEC 104）

use crate::errors::SecurityError;
use chrono::{DateTime, Utc};

/// 告警事件
#[derive(Debug, Clone)]
pub struct AlertEvent {
    pub alert_id: String,
    pub timestamp: DateTime<Utc>,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub source: String,
    pub description: String,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertType {
    UnauthorizedAccess,
    CertificateExpiring,
    CertificateExpired,
    TunnelDown,
    IntegrityViolation,
    PolicyViolation,
    ComplianceFailed,
    SecureBootFailed,
    RateLimitExceeded,
    RekeyFailed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// 告警接收通道 trait
pub trait AlertSink: Send + Sync {
    fn send(&self, event: &AlertEvent) -> Result<(), SecurityError>;
    fn name(&self) -> &str;
}

/// 告警管理器（Phase 2+ 实现）
pub struct AlertManager {
    sinks: Vec<Box<dyn AlertSink>>,
    history: Vec<AlertEvent>,
    max_history: usize,
}

impl AlertManager {
    pub fn new(max_history: usize) -> Self {
        todo!("Phase 2+")
    }

    pub fn register_sink(&mut self, sink: Box<dyn AlertSink>) {
        todo!("Phase 2+")
    }

    pub fn raise(
        &mut self,
        alert_type: AlertType,
        severity: AlertSeverity,
        source: &str,
        description: &str,
    ) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn acknowledge(&mut self, alert_id: &str) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn get_active_alerts(&self) -> Vec<&AlertEvent> {
        todo!("Phase 2+")
    }

    pub fn get_history(&self) -> &[AlertEvent] {
        todo!("Phase 2+")
    }
}
