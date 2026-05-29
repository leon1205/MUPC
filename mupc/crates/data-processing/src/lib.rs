//! MUPC 数据处理模块
//!
//! Phase 3B: 集成 mqtt-bridge 支持 MQTT 通信
//! P1: 扩展故障录波波形子模块和接口对齐

pub mod telemetry;
pub mod recorder;
pub mod collector;
pub mod high_freq_telemetry;
pub mod reporter;
pub mod fault_recorder_impl;
pub mod database;
pub mod errors;
pub mod waveform;
pub mod waveform_config;
pub mod waveform_reporter;

// 核心实现
pub use collector::DataCollectorImpl;
pub use high_freq_telemetry::HighFreqTelemetryImpl;
pub use fault_recorder_impl::FaultRecorderImpl;
pub use reporter::{DataReporterImpl, MessageBus};

// 错误类型
pub use errors::DataProcessingError;

// 遥测相关
pub use telemetry::{
    BatteryData, DataCollector, DataPackage, DataReporter as LegacyDataReporter,
    DeviceStatus, ElectricalData, FaultCondition, HighFrequencyTelemetry,
    InverterStatus, TelemetryData, WaveformData,
};

// 故障录波
pub use recorder::{
    ChannelStats, ExportResult, FaultEventFilter, FaultRecorder,
    PaginatedEvents, WaveformSummary,
};

// 波形子模块
pub use waveform::{
    ComtradeExporter, CsvExporter, DualBufferManager, RingBuffer,
    TriggerConfig, TriggerEngine, TriggerResult,
    WaveformMeta, WaveformReader, WaveformReporter, WaveformWriter,
};

// 波形配置和上报适配器
pub use waveform_config::{default_trigger_config, ChannelMask};
pub use waveform_reporter::WaveformReporterAdapter;

// Re-export MQTT bridge components for convenience
pub use mupc_mqtt_bridge::{LocalMqttClient, NorthMqttClient, MqttBridge, MqttBridgeError};
pub use mupc_mqtt_bridge::config::{LocalMqttConfig, NorthMqttConfig, MqttConfig};
pub use mupc_mqtt_bridge::topics::{LOCAL_TELEMETRY, LOCAL_STRATEGY_COMMAND, NORTH_TELEMETRY, NORTH_FAULT};

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