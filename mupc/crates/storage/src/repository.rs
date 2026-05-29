use crate::errors::StorageError;
use crate::models::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait TelemetryRepository: Send + Sync {
    async fn insert(&self, _point: &TelemetryPoint) -> Result<i64, StorageError> {
        todo!("Phase 2+")
    }
    async fn query_range(
        &self,
        _device_id: &str,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<Vec<TelemetryPoint>, StorageError> {
        todo!("Phase 2+")
    }
    async fn get_latest(
        &self,
        _device_id: &str,
        _metric: &str,
    ) -> Result<Option<TelemetryPoint>, StorageError> {
        todo!("Phase 2+")
    }
    async fn delete_older_than(&self, _before: DateTime<Utc>) -> Result<usize, StorageError> {
        todo!("Phase 2+")
    }
}

#[async_trait]
pub trait FaultRepository: Send + Sync {
    async fn insert(&self, _event: &FaultEvent) -> Result<i64, StorageError> {
        todo!("Phase 2+")
    }
    async fn query_range(
        &self,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<Vec<FaultEvent>, StorageError> {
        todo!("Phase 2+")
    }
    async fn acknowledge(&self, _id: i64) -> Result<(), StorageError> {
        todo!("Phase 2+")
    }
}

#[async_trait]
pub trait DecisionRepository: Send + Sync {
    async fn insert(&self, _record: &AiDecisionRecord) -> Result<i64, StorageError> {
        todo!("Phase 2+")
    }
    async fn query_recent(&self, _limit: usize) -> Result<Vec<AiDecisionRecord>, StorageError> {
        todo!("Phase 2+")
    }
    async fn get_by_scene(
        &self,
        _scene_type: &str,
        _limit: usize,
    ) -> Result<Vec<AiDecisionRecord>, StorageError> {
        todo!("Phase 2+")
    }
}

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn insert(&self, _event: &SystemEvent) -> Result<i64, StorageError> {
        todo!("Phase 2+")
    }
    async fn query_range(
        &self,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Result<Vec<SystemEvent>, StorageError> {
        todo!("Phase 2+")
    }
    async fn purge_older_than(&self, _before: DateTime<Utc>) -> Result<usize, StorageError> {
        todo!("Phase 2+")
    }
}

#[async_trait]
pub trait AssetRepository: Send + Sync {
    async fn upsert(&self, _asset: &AssetRecord) -> Result<i64, StorageError> {
        todo!("Phase 2+")
    }
    async fn get_by_device_id(
        &self,
        _device_id: &str,
    ) -> Result<Option<AssetRecord>, StorageError> {
        todo!("Phase 2+")
    }
    async fn list_all(&self) -> Result<Vec<AssetRecord>, StorageError> {
        todo!("Phase 2+")
    }
    async fn list_by_type(&self, _device_type: &str) -> Result<Vec<AssetRecord>, StorageError> {
        todo!("Phase 2+")
    }
}
