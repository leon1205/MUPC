use crate::telemetry::HighFrequencyTelemetry;
use async_trait::async_trait;
use mupc_common::MupcError;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// 高频遥测实现
/// 以 >=1Hz 频率上报遥测数据，内存缓冲 60 条
#[allow(dead_code)]
pub struct HighFreqTelemetryImpl {
    /// 上报周期 (ms)
    period_ms: u64,
    /// 是否运行 (使用 AtomicBool 以支持通过 &self 修改)
    running: AtomicBool,
    /// 内存缓冲 (Ring Buffer, 60 条)
    buffer: Arc<Mutex<VecDeque<TelemetryPoint>>>,
    /// 发送通道
    sender: Option<mpsc::Sender<TelemetryPoint>>,
}

/// 遥测数据点
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct TelemetryPoint {
    timestamp: u64,
    battery_soc: f64,
    battery_power: f64,
    pv_output: f64,
    load_power: f64,
    grid_power: f64,
    transformer_load: f64,
}

impl HighFreqTelemetryImpl {
    pub fn new(period_ms: u64) -> Self {
        Self {
            period_ms,
            running: AtomicBool::new(false),
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(60))),
            sender: None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn with_channel(period_ms: u64, sender: mpsc::Sender<TelemetryPoint>) -> Self {
        Self {
            period_ms,
            running: AtomicBool::new(false),
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(60))),
            sender: Some(sender),
        }
    }

    #[allow(dead_code)]
    fn push_to_buffer(&self, point: TelemetryPoint) {
        let mut buffer = self.buffer.lock().unwrap();
        if buffer.len() >= 60 {
            buffer.pop_front(); // Ring Buffer: 移除最旧的
        }
        buffer.push_back(point);
    }

    pub fn get_current_value(&self, point_name: &str) -> Option<f64> {
        let buffer = self.buffer.lock().unwrap();
        buffer.back().and_then(|p| {
            match point_name {
                "battery_soc" => Some(p.battery_soc),
                "battery_power" => Some(p.battery_power),
                "pv_output" => Some(p.pv_output),
                "load_power" => Some(p.load_power),
                "grid_power" => Some(p.grid_power),
                "transformer_load" => Some(p.transformer_load),
                _ => None,
            }
        })
    }
}

#[async_trait]
impl HighFrequencyTelemetry for HighFreqTelemetryImpl {
    async fn start(&self, _period_ms: u64) -> Result<(), MupcError> {
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn stop(&self) -> Result<(), MupcError> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn period(&self) -> u64 {
        self.period_ms
    }
}
