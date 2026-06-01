//! 系统指标分析器
//!
//! 提供趋势分析、阈值检测和 MTBF 计算。

use crate::collectors::SystemSnapshot;
use crate::errors::MonitorError;
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// 分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub analyzer: String,
    pub severity: AnalysisSeverity,
    pub findings: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnalysisSeverity {
    Normal,
    Warning,
    Critical,
}

// ============================================================================
// 趋势分析器
// ============================================================================

pub struct TrendAnalyzer {
    pub window_size: usize,
    pub anomaly_threshold: f64,
}

impl TrendAnalyzer {
    pub fn new(window_size: usize, anomaly_threshold: f64) -> Self {
        Self {
            window_size,
            anomaly_threshold,
        }
    }

    /// 基于滑动窗口的异常检测
    ///
    /// 计算最近 window_size 个采样点的 CPU 均值与标准差，
    /// 当前值偏离均值超过 anomaly_threshold 倍标准差时判定为异常。
    pub fn analyze(&self, history: &[SystemSnapshot]) -> Result<AnalysisResult, MonitorError> {
        let window = if history.len() > self.window_size {
            &history[history.len() - self.window_size..]
        } else {
            history
        };

        if window.is_empty() {
            return Ok(AnalysisResult {
                timestamp: Utc::now(),
                analyzer: "trend".into(),
                severity: AnalysisSeverity::Normal,
                findings: vec!["数据不足，无法进行趋势分析".into()],
                recommendations: vec![],
            });
        }

        let cpu_values: Vec<f32> = window.iter().map(|s| s.cpu.usage_percent).collect();
        let mean = cpu_values.iter().sum::<f32>() / cpu_values.len() as f32;
        let variance: f32 = cpu_values
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f32>()
            / cpu_values.len() as f32;
        let std_dev = variance.sqrt() as f64;

        let latest = window.last().unwrap();
        let current = latest.cpu.usage_percent as f64;
        let deviation = (current - mean as f64).abs();

        let mut findings = Vec::new();
        let mut recommendations = Vec::new();
        let severity;

        if deviation > self.anomaly_threshold * std_dev && std_dev > 0.0 {
            severity = AnalysisSeverity::Warning;
            findings.push(format!(
                "CPU 使用率异常: 当前 {:.1}%, 均值 {:.1}%, 偏离 {:.1}σ",
                current,
                mean,
                deviation / std_dev.max(0.01)
            ));
            recommendations.push("检查高负载进程".into());
        } else if latest.memory.usage_percent > 90.0 {
            severity = AnalysisSeverity::Critical;
            findings.push(format!(
                "内存使用率过高: {:.1}%",
                latest.memory.usage_percent
            ));
            recommendations.push("清理缓存或增加内存".into());
        } else {
            severity = AnalysisSeverity::Normal;
        }

        Ok(AnalysisResult {
            timestamp: Utc::now(),
            analyzer: "trend".into(),
            severity,
            findings,
            recommendations,
        })
    }
}

// ============================================================================
// 阈值分析器
// ============================================================================

pub struct ThresholdAnalyzer {
    pub cpu_warning: f32,
    pub cpu_critical: f32,
    pub memory_warning: f32,
    pub memory_critical: f32,
    pub disk_warning: f32,
    pub disk_critical: f32,
    pub temp_warning: f32,
    pub temp_critical: f32,
}

impl Default for ThresholdAnalyzer {
    fn default() -> Self {
        Self {
            cpu_warning: 80.0,
            cpu_critical: 95.0,
            memory_warning: 80.0,
            memory_critical: 95.0,
            disk_warning: 85.0,
            disk_critical: 95.0,
            temp_warning: 75.0,
            temp_critical: 85.0,
        }
    }
}

impl ThresholdAnalyzer {
    /// 对单个快照执行阈值检查
    pub fn analyze(&self, snapshot: &SystemSnapshot) -> Result<AnalysisResult, MonitorError> {
        let mut findings = Vec::new();
        let mut recommendations = Vec::new();
        let mut severity = AnalysisSeverity::Normal;

        // CPU 检查
        if snapshot.cpu.usage_percent >= self.cpu_critical {
            severity = AnalysisSeverity::Critical;
            findings.push(format!(
                "CPU 使用率达到 {}% (临界阈值 {}%)",
                snapshot.cpu.usage_percent, self.cpu_critical
            ));
            recommendations.push("立即检查高负载进程，考虑限流".into());
        } else if snapshot.cpu.usage_percent >= self.cpu_warning {
            if severity != AnalysisSeverity::Critical {
                severity = AnalysisSeverity::Warning;
            }
            findings.push(format!(
                "CPU 使用率偏高: {}% (警告阈值 {}%)",
                snapshot.cpu.usage_percent, self.cpu_warning
            ));
        }

        // 内存检查
        if snapshot.memory.usage_percent >= self.memory_critical {
            severity = AnalysisSeverity::Critical;
            findings.push(format!(
                "内存使用率达到 {}% (临界阈值 {}%)",
                snapshot.memory.usage_percent, self.memory_critical
            ));
            recommendations.push("清理缓存或重启服务".into());
        } else if snapshot.memory.usage_percent >= self.memory_warning {
            if severity != AnalysisSeverity::Critical {
                severity = AnalysisSeverity::Warning;
            }
            findings.push(format!(
                "内存使用率偏高: {}% (警告阈值 {}%)",
                snapshot.memory.usage_percent, self.memory_warning
            ));
        }

        // 磁盘检查
        if snapshot.disk.usage_percent >= self.disk_critical {
            severity = AnalysisSeverity::Critical;
            findings.push(format!("磁盘使用率达到 {}%", snapshot.disk.usage_percent));
            recommendations.push("立即清理磁盘空间".into());
        } else if snapshot.disk.usage_percent >= self.disk_warning {
            if severity != AnalysisSeverity::Critical {
                severity = AnalysisSeverity::Warning;
            }
            findings.push(format!("磁盘使用率偏高: {}%", snapshot.disk.usage_percent));
        }

        // 温度检查
        if snapshot.temperature.cpu_temp_c >= self.temp_critical {
            severity = AnalysisSeverity::Critical;
            findings.push(format!("CPU 温度过高: {}°C", snapshot.temperature.cpu_temp_c));
            recommendations.push("降频或增加散热".into());
        } else if snapshot.temperature.cpu_temp_c >= self.temp_warning {
            if severity != AnalysisSeverity::Critical {
                severity = AnalysisSeverity::Warning;
            }
            findings.push(format!("CPU 温度偏高: {}°C", snapshot.temperature.cpu_temp_c));
        }

        if findings.is_empty() {
            findings.push("所有指标正常".into());
        }

        Ok(AnalysisResult {
            timestamp: Utc::now(),
            analyzer: "threshold".into(),
            severity,
            findings,
            recommendations,
        })
    }
}

// ============================================================================
// MTBF 计算器
// ============================================================================

pub struct MtbfCalculator {
    pub failure_events: Vec<chrono::DateTime<chrono::Utc>>,
    pub total_uptime_secs: u64,
}

impl Default for MtbfCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl MtbfCalculator {
    pub fn new() -> Self {
        Self {
            failure_events: Vec::new(),
            total_uptime_secs: 0,
        }
    }

    pub fn record_failure(&mut self, timestamp: chrono::DateTime<chrono::Utc>) {
        self.failure_events.push(timestamp);
    }

    /// 计算 MTBF（平均故障间隔时间），单位：小时
    pub fn calculate_mtbf_hours(&self) -> f64 {
        if self.failure_events.len() < 2 {
            return if self.total_uptime_secs > 0 {
                self.total_uptime_secs as f64 / 3600.0
            } else {
                0.0
            };
        }

        let mut sorted = self.failure_events.clone();
        sorted.sort();

        let mut total_interval_secs = 0i64;
        for i in 1..sorted.len() {
            let interval = sorted[i] - sorted[i - 1];
            total_interval_secs += interval.num_seconds();
        }

        let intervals = sorted.len() - 1;
        if intervals > 0 {
            total_interval_secs as f64 / intervals as f64 / 3600.0
        } else {
            0.0
        }
    }

    /// 计算可用性百分比
    pub fn calculate_availability(&self) -> f64 {
        if self.total_uptime_secs == 0 {
            return 100.0;
        }

        let downtime_secs = self.failure_events.len() as u64 * 300; // 估算每次故障恢复 5 分钟
        let uptime = self.total_uptime_secs.saturating_sub(downtime_secs);
        (uptime as f64 / self.total_uptime_secs as f64) * 100.0
    }

    /// 检查是否达到目标 MTBF
    pub fn meets_target(&self, target_hours: f64) -> bool {
        self.calculate_mtbf_hours() >= target_hours
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::*;

    fn create_snapshot(cpu_pct: f32, mem_pct: f32, disk_pct: f32, temp: f32) -> SystemSnapshot {
        SystemSnapshot {
            timestamp: Utc::now(),
            cpu: CpuMetrics {
                usage_percent: cpu_pct,
                per_core: vec![],
                load_avg_1m: 0.0,
                load_avg_5m: 0.0,
                load_avg_15m: 0.0,
                temperature_c: None,
            },
            memory: MemoryMetrics {
                total_mb: 8192,
                used_mb: (8192.0 * mem_pct / 100.0) as u64,
                available_mb: 4096,
                swap_total_mb: 0,
                swap_used_mb: 0,
                usage_percent: mem_pct,
            },
            disk: DiskMetrics {
                total_mb: 65536,
                used_mb: (65536.0 * disk_pct / 100.0) as u64,
                available_mb: 32768,
                usage_percent: disk_pct,
                read_iops: 0,
                write_iops: 0,
            },
            temperature: TemperatureMetrics {
                cpu_temp_c: temp,
                npu_temp_c: None,
                board_temp_c: temp - 5.0,
                ambient_temp_c: Some(35.0),
            },
            processes: vec![],
        }
    }

    #[test]
    fn test_threshold_analyzer_normal() {
        let analyzer = ThresholdAnalyzer::default();
        let snapshot = create_snapshot(30.0, 50.0, 40.0, 50.0);
        let result = analyzer.analyze(&snapshot).unwrap();
        assert_eq!(result.severity, AnalysisSeverity::Normal);
    }

    #[test]
    fn test_threshold_analyzer_warning() {
        let analyzer = ThresholdAnalyzer::default();
        let snapshot = create_snapshot(85.0, 50.0, 40.0, 50.0);
        let result = analyzer.analyze(&snapshot).unwrap();
        assert_eq!(result.severity, AnalysisSeverity::Warning);
    }

    #[test]
    fn test_threshold_analyzer_critical() {
        let analyzer = ThresholdAnalyzer::default();
        let snapshot = create_snapshot(96.0, 50.0, 40.0, 50.0);
        let result = analyzer.analyze(&snapshot).unwrap();
        assert_eq!(result.severity, AnalysisSeverity::Critical);
    }

    #[test]
    fn test_trend_analyzer_empty() {
        let analyzer = TrendAnalyzer::new(10, 2.0);
        let result = analyzer.analyze(&[]).unwrap();
        assert_eq!(result.severity, AnalysisSeverity::Normal);
    }

    #[test]
    fn test_mtbf_calculator() {
        let mut calc = MtbfCalculator::new();
        calc.total_uptime_secs = 86400 * 30; // 30 days
        let mtbf = calc.calculate_mtbf_hours();
        assert!(mtbf > 0.0);
        let avail = calc.calculate_availability();
        assert!(avail > 90.0);
    }
}
