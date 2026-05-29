use crate::errors::MonitorError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 系统指标快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub timestamp: DateTime<Utc>,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub disk: DiskMetrics,
    pub temperature: TemperatureMetrics,
    pub processes: Vec<ProcessInfo>,
}

/// CPU 指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub usage_percent: f32,
    pub per_core: Vec<f32>,
    pub load_avg_1m: f32,
    pub load_avg_5m: f32,
    pub load_avg_15m: f32,
    pub temperature_c: Option<f32>, // NPU/CPU 温度
}

/// 内存指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub usage_percent: f32,
}

/// 磁盘指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskMetrics {
    pub total_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
    pub usage_percent: f32,
    pub read_iops: u64,
    pub write_iops: u64,
}

/// 温度指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemperatureMetrics {
    pub cpu_temp_c: f32,
    pub npu_temp_c: Option<f32>,
    pub board_temp_c: f32,
    pub ambient_temp_c: Option<f32>,
}

/// 进程信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub uptime_secs: u64,
}

// ============================================================================
// 采集器 trait 和实现
// ============================================================================

/// 指标采集器 trait
#[async_trait::async_trait]
pub trait MetricCollector: Send + Sync {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError>;
    fn name(&self) -> &'static str;
    fn collection_interval_ms(&self) -> u64;
}

/// CPU 采集器（Phase 2+ 实现）
pub struct CpuCollector {
    interval_ms: u64,
}

impl CpuCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for CpuCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        todo!("Phase 2+")
    }
    fn name(&self) -> &'static str {
        "cpu"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

/// 内存采集器（Phase 2+ 实现）
pub struct MemoryCollector {
    interval_ms: u64,
}

impl MemoryCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for MemoryCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        todo!("Phase 2+")
    }
    fn name(&self) -> &'static str {
        "memory"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

/// 磁盘采集器（Phase 2+ 实现）
pub struct DiskCollector {
    interval_ms: u64,
}

impl DiskCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for DiskCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        todo!("Phase 2+")
    }
    fn name(&self) -> &'static str {
        "disk"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

/// 温度采集器（Phase 2+ 实现）
pub struct TemperatureCollector {
    interval_ms: u64,
}

impl TemperatureCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for TemperatureCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        todo!("Phase 2+")
    }
    fn name(&self) -> &'static str {
        "temperature"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

/// 进程采集器（Phase 2+ 实现）
pub struct ProcessCollector {
    interval_ms: u64,
}

impl ProcessCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for ProcessCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        todo!("Phase 2+")
    }
    fn name(&self) -> &'static str {
        "process"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}
