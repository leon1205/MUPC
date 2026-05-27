//! MUPC 数据处理模块
//!
//! Phase 1 仅定义接口

pub mod telemetry;
pub mod recorder;
pub mod collector;
pub mod high_freq_telemetry;
pub mod fault_recorder_impl;
pub mod database;
pub mod errors;

pub use collector::DataCollectorImpl;
pub use high_freq_telemetry::HighFreqTelemetryImpl;
pub use fault_recorder_impl::FaultRecorderImpl;
pub use errors::DataProcessingError;
pub use telemetry::{DataCollector, HighFrequencyTelemetry, DataReporter, DataPackage, FaultCondition, WaveformData};
pub use recorder::FaultRecorder;

#[cfg(test)]
mod telemetry_test;
#[cfg(test)]
mod recorder_test;
#[cfg(test)]
mod collector_test;
#[cfg(test)]
mod high_freq_telemetry_test;
#[cfg(test)]
mod fault_recorder_impl_test;
#[cfg(test)]
mod database_test;
#[cfg(test)]
mod errors_test;