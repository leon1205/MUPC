//! MUPC 数据处理模块
//!
//! Phase 1 仅定义接口

pub mod errors;
pub mod telemetry;
pub mod recorder;
pub mod collector;

pub use telemetry::{DataCollector, HighFrequencyTelemetry, DataReporter, DataPackage, FaultCondition, WaveformData};
pub use recorder::FaultRecorder;
pub use errors::DataProcessingError;
pub use collector::DataCollectorImpl;