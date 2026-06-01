//! 安全事件告警
//!
//! 安全事件告警管理，支持多通道上报（日志/MQTT/IEC 104）

use crate::errors::SecurityError;
use chrono::{DateTime, Utc};
use uuid::Uuid;

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

impl AlertType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertType::UnauthorizedAccess => "unauthorized_access",
            AlertType::CertificateExpiring => "cert_expiring",
            AlertType::CertificateExpired => "cert_expired",
            AlertType::TunnelDown => "tunnel_down",
            AlertType::IntegrityViolation => "integrity_violation",
            AlertType::PolicyViolation => "policy_violation",
            AlertType::ComplianceFailed => "compliance_failed",
            AlertType::SecureBootFailed => "secure_boot_failed",
            AlertType::RateLimitExceeded => "ratelimit_exceeded",
            AlertType::RekeyFailed => "rekey_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl AlertSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertSeverity::Low => "low",
            AlertSeverity::Medium => "medium",
            AlertSeverity::High => "high",
            AlertSeverity::Critical => "critical",
        }
    }
}

/// 告警接收通道 trait
pub trait AlertSink: Send + Sync {
    fn send(&self, event: &AlertEvent) -> Result<(), SecurityError>;
    fn name(&self) -> &str;
}

/// 日志告警通道
pub struct LogAlertSink;

impl AlertSink for LogAlertSink {
    fn send(&self, event: &AlertEvent) -> Result<(), SecurityError> {
        match event.severity {
            AlertSeverity::Critical => {
                tracing::error!(
                    alert_id = %event.alert_id,
                    alert_type = %event.alert_type.as_str(),
                    severity = %event.severity.as_str(),
                    source = %event.source,
                    description = %event.description,
                    "安全告警"
                );
            }
            AlertSeverity::High => {
                tracing::warn!(
                    alert_id = %event.alert_id,
                    alert_type = %event.alert_type.as_str(),
                    source = %event.source,
                    "安全告警"
                );
            }
            _ => {
                tracing::info!(
                    alert_id = %event.alert_id,
                    alert_type = %event.alert_type.as_str(),
                    source = %event.source,
                    "安全告警"
                );
            }
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "log"
    }
}

/// 告警管理器
pub struct AlertManager {
    sinks: Vec<Box<dyn AlertSink>>,
    history: Vec<AlertEvent>,
    max_history: usize,
}

impl AlertManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            sinks: Vec::new(),
            history: Vec::with_capacity(max_history),
            max_history,
        }
    }

    pub fn register_sink(&mut self, sink: Box<dyn AlertSink>) {
        self.sinks.push(sink);
    }

    pub fn raise(
        &mut self,
        alert_type: AlertType,
        severity: AlertSeverity,
        source: &str,
        description: &str,
    ) -> Result<(), SecurityError> {
        let event = AlertEvent {
            alert_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            alert_type,
            severity,
            source: source.to_string(),
            description: description.to_string(),
            acknowledged: false,
        };

        for sink in &self.sinks {
            if let Err(e) = sink.send(&event) {
                tracing::warn!(
                    sink = sink.name(),
                    error = %e,
                    "告警发送到通道失败"
                );
            }
        }

        if self.history.len() >= self.max_history {
            self.history.remove(0);
        }
        self.history.push(event);
        Ok(())
    }

    pub fn acknowledge(&mut self, alert_id: &str) -> Result<(), SecurityError> {
        let event = self
            .history
            .iter_mut()
            .find(|e| e.alert_id == alert_id)
            .ok_or_else(|| SecurityError::NotFound(format!("告警 {} 不存在", alert_id)))?;
        event.acknowledged = true;
        Ok(())
    }

    pub fn get_active_alerts(&self) -> Vec<&AlertEvent> {
        self.history.iter().filter(|e| !e.acknowledged).collect()
    }

    pub fn get_history(&self) -> &[AlertEvent] {
        &self.history
    }
}
