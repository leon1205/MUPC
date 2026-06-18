use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 遥测数据点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryPoint {
    pub id: Option<i64>,
    pub device_id: String,
    pub timestamp: DateTime<Utc>,
    pub metric_name: String,
    pub value: f64,
    pub quality: i32,
}

/// 故障事件记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultEvent {
    pub id: Option<i64>,
    pub device_id: String,
    pub timestamp: DateTime<Utc>,
    pub fault_type: String,
    pub severity: i32,
    pub waveform_path: Option<String>,
    pub acknowledged: bool,
}

/// AI 决策记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiDecisionRecord {
    pub id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub scene_type: String,
    pub action_json: String,
    pub confidence: f64,
    pub model_version: String,
}

/// 系统事件日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub source: String,
    pub message: String,
}

/// 资产/设备台账
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRecord {
    pub id: Option<i64>,
    pub device_id: String,
    pub device_type: String,
    pub manufacturer: String,
    pub model: String,
    pub firmware_version: String,
    pub installed_at: DateTime<Utc>,
    pub last_maintenance: Option<DateTime<Utc>>,
}

/// v2.17 安全包装器违规记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyViolation {
    pub id: Option<i64>,
    pub timestamp: i64,
    pub reason: String,
    pub proposed_p_ref: f64,
    pub proposed_k_droop: f64,
    pub fallback_p_ref: f64,
    pub fallback_k_droop: f64,
    pub v_predicted: f64,
    pub latency_us: i64,
}
