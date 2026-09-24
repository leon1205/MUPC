use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 遥测数据点
///
/// ⚠️ **`value: Option<f64>`（2026-09-24 起，03 设计 §9.1.4 / PRD R-11.2-E）**：
/// `None` = **无数据**，库内落为**真 `NULL`**（`telemetry.value` 已改为可空，见
/// `services::run_migrations` 的幂等可空化迁移）。**只有总表聚合的缺测行会写 `None`**
/// （`GridAggregator` 的无有效采样通道）；其余写入方（核间 `TelemetryData`、外设遥测、
/// 电池/告警派生点）一律写 `Some(v)` —— **`None` 不是「值为 0」，也不是「写入方偷懒」**。
///
/// **查询契约（防「NULL 行进统计」）**：`AVG/SUM` 天然跳过 `NULL` ⇒ 无需额外过滤即不会污染
/// 统计；但按 `quality` 做**存在性/计数**查询时须显式过滤（非 `Good` 的才是「无数据」）。
/// 库内二者可区分：真 0 值是 `value = 0.0` 且 `quality = 0(Good)`，缺测是 `value IS NULL`
/// 且 `quality = 1(NoData)`（03 设计 §9.1.4 的机械判据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryPoint {
    pub id: Option<i64>,
    pub device_id: String,
    pub timestamp: DateTime<Utc>,
    pub metric_name: String,
    pub value: Option<f64>,
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
