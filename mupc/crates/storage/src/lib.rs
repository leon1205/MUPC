pub mod errors;
pub mod grid_aggregate;
pub mod models;
pub mod repository;
pub mod services;
pub mod write_gate;

pub use errors::StorageError;
pub use grid_aggregate::{
    AggregateRow, ChannelSpec, GridAggregator, GridSample, Quality, CHANNELS,
};
pub use models::*;
pub use repository::{
    init_pool, integrity_check, AssetRepository, DecisionRepository, EventRepository,
    FaultRepository, SqliteAssetRepo, SqliteDecisionRepo, SqliteEventRepo, SqliteFaultRepo,
    SqliteTelemetryRepo, TelemetryRepository,
};
pub use services::{
    run_migrations, RetentionManager, RetentionReport, StorageService, WriteBuffer,
};
pub use write_gate::{classify_disk_usage, DiskLevel, WriteGate};
