//! MUPC 数据处理模块
//!
//! Phase 3B: 集成 mqtt-bridge 支持 MQTT 通信

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