//! 系统指标采集器
//!
//! Linux: 从 /proc 和 /sys 文件系统采集 CPU/内存/磁盘/温度数据。
//! 非 Linux: 使用模拟数据用于开发测试。

use crate::errors::MonitorError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 系统指标快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSnapshot {
    pub timestamp: DateTime<Utc>,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    /// 磁盘指标：`None` = **不可用**（采集失败）。
    ///
    /// ⚠️ 是 `Option` 而不是"填个默认值"：旧实现在 `df` 失败/解析失败时回退成
    /// `32768 MB / 16384 MB / 50%` 这类**硬编码数字**，屏上与告警分析会把它们当真实用量
    /// （技术债 U-29 只覆盖了"采集值不可采信"的一般面，**未**覆盖这几处磁盘假值回退）。
    /// 消费侧（`analyzers.rs` / `startup.rs` 日志）遇到 `None` 必须**不展示 / 不判定**。
    pub disk: Option<DiskMetrics>,
    pub temperature: TemperatureMetrics,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub usage_percent: f32,
    pub per_core: Vec<f32>,
    pub load_avg_1m: f32,
    pub load_avg_5m: f32,
    pub load_avg_15m: f32,
    pub temperature_c: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub usage_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskMetrics {
    pub total_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
    pub usage_percent: f32,
    pub read_iops: u64,
    pub write_iops: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemperatureMetrics {
    pub cpu_temp_c: f32,
    pub npu_temp_c: Option<f32>,
    pub board_temp_c: f32,
    pub ambient_temp_c: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_mb: f64,
    pub uptime_secs: u64,
}

// ============================================================================
// 采集器 trait
// ============================================================================

#[async_trait::async_trait]
pub trait MetricCollector: Send + Sync {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError>;
    fn name(&self) -> &'static str;
    fn collection_interval_ms(&self) -> u64;
}

// ============================================================================
// 系统指标采集（跨平台）
// ============================================================================

fn read_cpu_metrics() -> Result<CpuMetrics, MonitorError> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string("/proc/stat")
            .map_err(|e| MonitorError::CollectionError(format!("读取 /proc/stat 失败: {}", e)))?;

        let mut total = 0u64;
        let mut idle = 0u64;
        for line in stat.lines() {
            if line.starts_with("cpu ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    for (i, val) in parts.iter().enumerate().skip(1) {
                        let v: u64 = val.parse().unwrap_or(0);
                        total += v;
                        if i == 4 {
                            idle = v;
                        }
                    }
                }
                break;
            }
        }

        let usage = if total > 0 {
            ((total - idle) as f32 / total as f32) * 100.0
        } else {
            0.0
        };

        let loadavg = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
        let loads: Vec<f32> = loadavg
            .split_whitespace()
            .take(3)
            .filter_map(|s| s.parse().ok())
            .collect();

        let temp = read_cpu_temp();

        Ok(CpuMetrics {
            usage_percent: (usage * 10.0).round() / 10.0,
            per_core: vec![usage],
            load_avg_1m: loads.first().copied().unwrap_or(0.0),
            load_avg_5m: loads.get(1).copied().unwrap_or(0.0),
            load_avg_15m: loads.get(2).copied().unwrap_or(0.0),
            temperature_c: temp,
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(CpuMetrics {
            usage_percent: 25.0,
            per_core: vec![25.0, 30.0, 20.0, 28.0],
            load_avg_1m: 0.5,
            load_avg_5m: 0.8,
            load_avg_15m: 0.6,
            temperature_c: Some(45.0),
        })
    }
}

fn read_memory_metrics() -> Result<MemoryMetrics, MonitorError> {
    #[cfg(target_os = "linux")]
    {
        let meminfo = std::fs::read_to_string("/proc/meminfo").map_err(|e| {
            MonitorError::CollectionError(format!("读取 /proc/meminfo 失败: {}", e))
        })?;

        let mut total_kb = 0u64;
        let mut available_kb = 0u64;
        let mut swap_total_kb = 0u64;
        let mut swap_free_kb = 0u64;

        for line in meminfo.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = parse_kb_value(line);
            } else if line.starts_with("MemAvailable:") {
                available_kb = parse_kb_value(line);
            } else if line.starts_with("SwapTotal:") {
                swap_total_kb = parse_kb_value(line);
            } else if line.starts_with("SwapFree:") {
                swap_free_kb = parse_kb_value(line);
            }
        }

        let total_mb = total_kb / 1024;
        let available_mb = available_kb / 1024;
        let used = total_mb.saturating_sub(available_mb);

        Ok(MemoryMetrics {
            total_mb,
            used_mb: used,
            available_mb,
            swap_total_mb: swap_total_kb / 1024,
            swap_used_mb: (swap_total_kb - swap_free_kb) / 1024,
            usage_percent: if total_mb > 0 {
                (used as f32 / total_mb as f32) * 100.0
            } else {
                0.0
            },
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(MemoryMetrics {
            total_mb: 8192,
            used_mb: 4096,
            available_mb: 4096,
            swap_total_mb: 2048,
            swap_used_mb: 512,
            usage_percent: 50.0,
        })
    }
}

#[cfg(target_os = "linux")]
fn parse_kb_value(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn read_cpu_temp() -> Option<f32> {
    std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .map(|v| v / 1000.0)
}

/// 采集磁盘指标（Linux：根分区）。
///
/// 返回 `Ok(None)` = **不可用**（见 [`SystemSnapshot::disk`]）。
///
/// TODO(D-15b)：`df` 子进程应改为直读 `statvfs(3)`（免 fork、免解析本地化输出）。本轮**未做**：
/// 那需要 `libc`/`nix` 依赖或 `unsafe` FFI，而本仓库的变更自查清单明写"无新增 `unsafe` 块"
/// ⇒ 记 TODO 并在报告中说明边界（本轮已消除的是"失败返回假值"这一更危险的问题）。
fn read_disk_metrics() -> Result<Option<DiskMetrics>, MonitorError> {
    #[cfg(target_os = "linux")]
    {
        read_disk_metrics_at("/")
    }

    #[cfg(not(target_os = "linux"))]
    {
        // 非 Linux 是**开发平台**：保留模拟值（**不是**失败兜底 —— Linux 路径失败返回 None）。
        Ok(Some(DiskMetrics {
            total_mb: 65536,
            used_mb: 32768,
            available_mb: 32768,
            usage_percent: 50.0,
            read_iops: 1200,
            write_iops: 800,
        }))
    }
}

/// 采集指定挂载点的磁盘指标；**采不到就 `Ok(None)`，绝不返回假值**。
///
/// `mount_point` 是**失败注入点**（生产传 `/`）：传一个不存在的路径 ⇒ `df` 非零退出 ⇒
/// `Ok(None)`，用例 `disk_collection_failure_is_unavailable_not_a_fake_number` 据此钉住语义。
fn read_disk_metrics_at(mount_point: &str) -> Result<Option<DiskMetrics>, MonitorError> {
    let output = std::process::Command::new("df")
        .arg("-BM")
        .arg(mount_point)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            tracing::warn!(
                mount_point,
                status = ?o.status,
                "df 非零退出 ⇒ 磁盘指标不可用（不回退假值）"
            );
            return Ok(None);
        }
        Err(e) => {
            tracing::warn!(mount_point, error = %e, "df 无法执行 ⇒ 磁盘指标不可用（不回退假值）");
            return Ok(None);
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    match parse_df_output(&stdout) {
        Some(metrics) => Ok(Some(metrics)),
        None => {
            tracing::warn!(
                mount_point,
                "df 输出无法解析 ⇒ 磁盘指标不可用（不回退假值）"
            );
            Ok(None)
        }
    }
}

/// `df -BM` 输出 ⇒ [`DiskMetrics`]；**任何字段解析失败即 `None`**（不逐字段填默认值）。
///
/// 抽成纯函数 ⇒ 可直接用"合法行 / 畸形行"钉住判别力（不必依赖本机真有 `df`）。
/// `df` 的表头行（`Filesystem 1M-blocks Used Available Use% Mounted on`）会被跳过。
fn parse_df_output(stdout: &str) -> Option<DiskMetrics> {
    let line = stdout.lines().skip(1).find(|l| !l.trim().is_empty())?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }

    let total = parts[1].trim_end_matches('M').parse::<u64>().ok()?;
    let used = parts[2].trim_end_matches('M').parse::<u64>().ok()?;
    let available = parts[3].trim_end_matches('M').parse::<u64>().ok()?;
    let usage_pct = parts[4].trim_end_matches('%').parse::<f32>().ok()?;

    Some(DiskMetrics {
        total_mb: total,
        used_mb: used,
        available_mb: available,
        usage_percent: usage_pct,
        read_iops: 0,
        write_iops: 0,
    })
}

fn read_temperature_metrics() -> Result<TemperatureMetrics, MonitorError> {
    #[cfg(target_os = "linux")]
    {
        let cpu = read_cpu_temp().unwrap_or(45.0);
        Ok(TemperatureMetrics {
            cpu_temp_c: cpu,
            npu_temp_c: None,
            board_temp_c: cpu - 5.0,
            ambient_temp_c: Some(35.0),
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(TemperatureMetrics {
            cpu_temp_c: 48.0,
            npu_temp_c: Some(52.0),
            board_temp_c: 42.0,
            ambient_temp_c: Some(35.0),
        })
    }
}

// ============================================================================
// 采集器实现
// ============================================================================

pub struct CpuCollector {
    interval_ms: u64,
}

impl CpuCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for CpuCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        Ok(SystemSnapshot {
            timestamp: Utc::now(),
            cpu: read_cpu_metrics()?,
            memory: MemoryMetrics {
                total_mb: 0,
                used_mb: 0,
                available_mb: 0,
                swap_total_mb: 0,
                swap_used_mb: 0,
                usage_percent: 0.0,
            },
            // 单指标采集器不采集磁盘 ⇒ `None`（**不是** 0，0 会被读成"磁盘用量 0%"）
            disk: None,
            temperature: TemperatureMetrics {
                cpu_temp_c: 0.0,
                npu_temp_c: None,
                board_temp_c: 0.0,
                ambient_temp_c: None,
            },
            processes: vec![],
        })
    }
    fn name(&self) -> &'static str {
        "cpu"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

pub struct MemoryCollector {
    interval_ms: u64,
}

impl MemoryCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for MemoryCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        Ok(SystemSnapshot {
            timestamp: Utc::now(),
            cpu: CpuMetrics {
                usage_percent: 0.0,
                per_core: vec![],
                load_avg_1m: 0.0,
                load_avg_5m: 0.0,
                load_avg_15m: 0.0,
                temperature_c: None,
            },
            memory: read_memory_metrics()?,
            // 单指标采集器不采集磁盘 ⇒ `None`（**不是** 0，0 会被读成"磁盘用量 0%"）
            disk: None,
            temperature: TemperatureMetrics {
                cpu_temp_c: 0.0,
                npu_temp_c: None,
                board_temp_c: 0.0,
                ambient_temp_c: None,
            },
            processes: vec![],
        })
    }
    fn name(&self) -> &'static str {
        "memory"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

pub struct DiskCollector {
    interval_ms: u64,
}

impl DiskCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for DiskCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        Ok(SystemSnapshot {
            timestamp: Utc::now(),
            cpu: CpuMetrics {
                usage_percent: 0.0,
                per_core: vec![],
                load_avg_1m: 0.0,
                load_avg_5m: 0.0,
                load_avg_15m: 0.0,
                temperature_c: None,
            },
            memory: MemoryMetrics {
                total_mb: 0,
                used_mb: 0,
                available_mb: 0,
                swap_total_mb: 0,
                swap_used_mb: 0,
                usage_percent: 0.0,
            },
            disk: read_disk_metrics()?,
            temperature: TemperatureMetrics {
                cpu_temp_c: 0.0,
                npu_temp_c: None,
                board_temp_c: 0.0,
                ambient_temp_c: None,
            },
            processes: vec![],
        })
    }
    fn name(&self) -> &'static str {
        "disk"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

pub struct TemperatureCollector {
    interval_ms: u64,
}

impl TemperatureCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for TemperatureCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        Ok(SystemSnapshot {
            timestamp: Utc::now(),
            cpu: CpuMetrics {
                usage_percent: 0.0,
                per_core: vec![],
                load_avg_1m: 0.0,
                load_avg_5m: 0.0,
                load_avg_15m: 0.0,
                temperature_c: None,
            },
            memory: MemoryMetrics {
                total_mb: 0,
                used_mb: 0,
                available_mb: 0,
                swap_total_mb: 0,
                swap_used_mb: 0,
                usage_percent: 0.0,
            },
            // 单指标采集器不采集磁盘 ⇒ `None`（**不是** 0，0 会被读成"磁盘用量 0%"）
            disk: None,
            temperature: read_temperature_metrics()?,
            processes: vec![],
        })
    }
    fn name(&self) -> &'static str {
        "temperature"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

pub struct ProcessCollector {
    interval_ms: u64,
}

impl ProcessCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for ProcessCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        #[cfg_attr(not(target_os = "linux"), allow(unused_mut))]
        let mut processes = Vec::new();

        #[cfg(target_os = "linux")]
        {
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if let Ok(pid) = name_str.parse::<u32>() {
                        if let Ok(stat) = std::fs::read_to_string(entry.path().join("comm")) {
                            processes.push(ProcessInfo {
                                pid,
                                name: stat.trim().to_string(),
                                cpu_percent: 0.0,
                                memory_mb: 0.0,
                                uptime_secs: 0,
                            });
                        }
                        if processes.len() >= 20 {
                            break;
                        }
                    }
                }
            }
        }

        Ok(SystemSnapshot {
            timestamp: Utc::now(),
            cpu: CpuMetrics {
                usage_percent: 0.0,
                per_core: vec![],
                load_avg_1m: 0.0,
                load_avg_5m: 0.0,
                load_avg_15m: 0.0,
                temperature_c: None,
            },
            memory: MemoryMetrics {
                total_mb: 0,
                used_mb: 0,
                available_mb: 0,
                swap_total_mb: 0,
                swap_used_mb: 0,
                usage_percent: 0.0,
            },
            // 单指标采集器不采集磁盘 ⇒ `None`（**不是** 0，0 会被读成"磁盘用量 0%"）
            disk: None,
            temperature: TemperatureMetrics {
                cpu_temp_c: 0.0,
                npu_temp_c: None,
                board_temp_c: 0.0,
                ambient_temp_c: None,
            },
            processes,
        })
    }
    fn name(&self) -> &'static str {
        "process"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

/// 完整系统快照采集器（聚合所有子采集器）
pub struct FullCollector {
    interval_ms: u64,
}

impl FullCollector {
    pub fn new(interval_ms: u64) -> Self {
        Self { interval_ms }
    }
}

#[async_trait::async_trait]
impl MetricCollector for FullCollector {
    async fn collect(&self) -> Result<SystemSnapshot, MonitorError> {
        Ok(SystemSnapshot {
            timestamp: Utc::now(),
            cpu: read_cpu_metrics()?,
            memory: read_memory_metrics()?,
            disk: read_disk_metrics()?,
            temperature: read_temperature_metrics()?,
            processes: vec![],
        })
    }
    fn name(&self) -> &'static str {
        "full"
    }
    fn collection_interval_ms(&self) -> u64 {
        self.interval_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_metrics_read() {
        let result = read_cpu_metrics();
        assert!(result.is_ok());
        let cpu = result.unwrap();
        assert!(cpu.usage_percent >= 0.0);
    }

    #[test]
    fn test_memory_metrics_read() {
        let result = read_memory_metrics();
        assert!(result.is_ok());
        let mem = result.unwrap();
        assert!(mem.total_mb > 0);
    }

    #[test]
    fn test_disk_metrics_read() {
        // 只断言"不报错"：磁盘**可不可用**取决于本机环境（Windows 开发机就没有 df），
        // 旧断言 `is_ok()` 本身没有判别力 —— 真正的判别力在下一条用例。
        let result = read_disk_metrics();
        assert!(result.is_ok(), "采集本身不得报错（不可用应表达为 Ok(None)）");
    }

    /// 判别力：**采集失败必须是"不可用"（`None`），不是某个数字**。
    ///
    /// 旧实现在失败时返回 `32768 MB / 16384 MB / 50%` 的硬编码值 ⇒ 本用例红。
    #[test]
    fn disk_collection_failure_is_unavailable_not_a_fake_number() {
        // 不存在的挂载点 —— Windows 上 `df` 根本不存在，Linux 上 df 会非零退出；
        // 两条路都必须落 `Ok(None)`（这正是"失败"）。
        let r = read_disk_metrics_at("/definitely/not/a/mount/point/mupc-d15-probe")
            .expect("采集失败不是 Result::Err —— 不可用是 Ok(None) 这一**正常**取值");
        assert!(
            r.is_none(),
            "采集失败必须表达为不可用（None），不得回退成假值: {r:?}"
        );
    }

    /// 判别力：`df` 输出**任何**字段解析不了 ⇒ `None`（旧实现逐字段 `unwrap_or(默认值)` ⇒ 红）。
    #[test]
    fn malformed_df_output_is_unavailable_never_partially_defaulted() {
        // 合法行（`df -BM` 第 2 行）
        let ok = parse_df_output(
            "Filesystem     1M-blocks  Used Available Use% Mounted on\n\
             /dev/mmcblk0p3      57632 12345     42109  23% /\n",
        )
        .expect("合法输出必须解析出指标");
        assert_eq!(ok.total_mb, 57632);
        assert_eq!(ok.used_mb, 12345);
        assert_eq!(ok.available_mb, 42109);
        assert_eq!(ok.usage_percent, 23.0);

        // 畸形行（Used 不是数字）⇒ 必须整体不可用，**不得**给 total=32768/used=0/50% 的拼凑值
        assert!(
            parse_df_output(
                "Filesystem     1M-blocks  Used Available Use% Mounted on\n\
                 /dev/mmcblk0p3      57632  ???     42109  23% /\n"
            )
            .is_none(),
            "任一字段解析失败即不可用（旧实现会给 32768/0/50% 这类假值）"
        );
        // 字段数不足
        assert!(parse_df_output("Filesystem\n/dev/x 1 2\n").is_none());
        // 只有表头
        assert!(parse_df_output("Filesystem 1M-blocks Used Available Use% Mounted on\n").is_none());
    }

    #[test]
    fn test_temperature_metrics_read() {
        let result = read_temperature_metrics();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_full_collector() {
        let collector = FullCollector::new(5000);
        let snapshot = collector.collect().await.unwrap();
        assert!(snapshot.cpu.usage_percent >= 0.0);
        assert!(snapshot.memory.total_mb > 0);
    }

    /// 判别力：单指标采集器**不采集**磁盘 ⇒ `disk` 必须是 `None`（旧实现填全 0 ⇒ 红）。
    #[tokio::test]
    async fn single_metric_collectors_report_no_disk_rather_than_zeros() {
        for collector in [
            Box::new(CpuCollector::new(1000)) as Box<dyn MetricCollector>,
            Box::new(MemoryCollector::new(1000)),
            Box::new(TemperatureCollector::new(1000)),
            Box::new(ProcessCollector::new(1000)),
        ] {
            let snap = collector.collect().await.unwrap();
            assert!(
                snap.disk.is_none(),
                "{} 不采集磁盘 ⇒ disk 必须 None（0% 会被读成真实空盘）",
                collector.name()
            );
        }
    }
}
