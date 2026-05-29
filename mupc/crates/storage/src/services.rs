use crate::errors::StorageError;
use crate::models::TelemetryPoint;
use crate::repository::*;
use std::sync::Arc;

/// 存储服务 — 统一入口
pub struct StorageService {
    pub telemetry: Arc<dyn TelemetryRepository>,
    pub faults: Arc<dyn FaultRepository>,
    pub decisions: Arc<dyn DecisionRepository>,
    pub events: Arc<dyn EventRepository>,
    pub assets: Arc<dyn AssetRepository>,
}

impl StorageService {
    pub fn new(
        _telemetry: Arc<dyn TelemetryRepository>,
        _faults: Arc<dyn FaultRepository>,
        _decisions: Arc<dyn DecisionRepository>,
        _events: Arc<dyn EventRepository>,
        _assets: Arc<dyn AssetRepository>,
    ) -> Self {
        todo!("Phase 2+")
    }

    pub async fn health_check(&self) -> Result<bool, StorageError> {
        todo!("Phase 2+")
    }
}

/// 写入缓冲管理器
pub struct WriteBuffer {
    capacity: usize,
    flush_interval_ms: u64,
}

impl WriteBuffer {
    pub fn new(capacity: usize, flush_interval_ms: u64) -> Self {
        todo!("Phase 2+")
    }

    pub async fn buffer_telemetry(&self, _point: TelemetryPoint) -> Result<(), StorageError> {
        todo!("Phase 2+")
    }

    pub async fn flush(&self) -> Result<usize, StorageError> {
        todo!("Phase 2+")
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn flush_interval_ms(&self) -> u64 {
        self.flush_interval_ms
    }
}

/// 数据保留策略管理器
pub struct RetentionManager {
    telemetry_retention_days: u32,
    event_retention_days: u32,
}

impl RetentionManager {
    pub fn new(telemetry_days: u32, event_days: u32) -> Self {
        todo!("Phase 2+")
    }

    pub async fn enforce(
        &self,
        _service: &StorageService,
    ) -> Result<RetentionReport, StorageError> {
        todo!("Phase 2+")
    }

    pub fn telemetry_retention_days(&self) -> u32 {
        self.telemetry_retention_days
    }

    pub fn event_retention_days(&self) -> u32 {
        self.event_retention_days
    }
}

pub struct RetentionReport {
    pub telemetry_deleted: usize,
    pub events_deleted: usize,
}
