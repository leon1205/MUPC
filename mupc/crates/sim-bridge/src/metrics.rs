use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Episode 指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeMetrics {
    pub scenario: String,
    pub start_time: String,
    pub end_time: String,
    pub total_steps: u64,
    pub total_reward: f64,
    pub avg_step_latency_ms: f64,
    pub min_step_latency_ms: f64,
    pub max_step_latency_ms: f64,
    pub p99_step_latency_ms: f64,
    pub safety_override_count: u32,
    pub soc_violations: u32,
    pub voltage_violations: u32,
}

/// 仿真运行指标收集器
pub struct MetricsCollector {
    current: EpisodeMetrics,
    latency_samples: Vec<u64>,
    history: Vec<EpisodeMetrics>,
}

impl MetricsCollector {
    pub fn new(scenario: &str) -> Self {
        Self {
            current: EpisodeMetrics {
                scenario: scenario.to_string(),
                start_time: Utc::now().to_rfc3339(),
                end_time: String::new(),
                total_steps: 0,
                total_reward: 0.0,
                avg_step_latency_ms: 0.0,
                min_step_latency_ms: f64::MAX,
                max_step_latency_ms: 0.0,
                p99_step_latency_ms: 0.0,
                safety_override_count: 0,
                soc_violations: 0,
                voltage_violations: 0,
            },
            latency_samples: Vec::new(),
            history: Vec::new(),
        }
    }

    pub fn record_step(&mut self, latency_ms: u64, reward: f64, info: &serde_json::Value) {
        self.current.total_steps += 1;
        self.current.total_reward += reward;
        self.latency_samples.push(latency_ms);

        if latency_ms < self.current.min_step_latency_ms as u64 {
            self.current.min_step_latency_ms = latency_ms as f64;
        }
        if latency_ms > self.current.max_step_latency_ms as u64 {
            self.current.max_step_latency_ms = latency_ms as f64;
        }

        if let Some(so) = info.get("safety_override_count").and_then(|v| v.as_u64()) {
            self.current.safety_override_count = so as u32;
        }
        if let Some(soc) = info.get("soc_violations").and_then(|v| v.as_u64()) {
            self.current.soc_violations = soc as u32;
        }
    }

    pub fn reset_episode(&mut self, scenario: &str) {
        // Compute stats before reset
        let n = self.latency_samples.len();
        if n > 0 {
            self.latency_samples.sort_unstable();
            self.current.avg_step_latency_ms =
                self.latency_samples.iter().sum::<u64>() as f64 / n as f64;
            let p99_idx = ((n as f64) * 0.99).ceil() as usize - 1;
            self.current.p99_step_latency_ms = self.latency_samples[p99_idx] as f64;
        }

        self.current.end_time = Utc::now().to_rfc3339();
        self.history.push(self.current.clone());

        // Reset for next episode
        self.current = EpisodeMetrics {
            scenario: scenario.to_string(),
            start_time: Utc::now().to_rfc3339(),
            ..Self::new(scenario).current
        };
        self.latency_samples.clear();
    }

    pub fn export(&self, path: &str) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "sim_metrics": {
                "current": &self.current,
                "history": &self.history,
            }
        }))?;
        std::fs::write(path, json)?;
        tracing::info!("指标已导出: {}", path);
        Ok(())
    }
}
