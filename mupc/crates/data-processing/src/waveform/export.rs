//! COMTRADE / CSV 导出
//!
//! 支持将故障录波数据导出为 IEEE C37.111 COMTRADE 格式和 CSV 格式。
//! 导出为非实时操作，按需生成。
//!
//! # COMTRADE 格式
//!
//! IEEE Std C37.111-1999，生成三个文件：
//! - `.cfg` — 配置文件（通道定义、采样率等）
//! - `.dat` — 数据文件（ASCII 或二进制格式）
//! - `.hdr` — 头文件（设备信息、故障描述）
//!
//! # CSV 格式
//!
//! UTF-8 with BOM，首行为通道名称，每行一个采样点。

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use super::storage::WaveformMeta;

/// 导出错误类型
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("不支持的通道数量: {0}")]
    UnsupportedChannelCount(usize),
    #[error("数据为空")]
    EmptyData,
}

/// COMTRADE 导出器
///
/// 生成符合 IEEE C37.111-1999 标准的 COMTRADE 文件。
#[allow(dead_code)]
pub struct ComtradeExporter {
    /// 波形文件源目录
    waveforms_dir: PathBuf,
    /// 导出目标目录
    export_dir: PathBuf,
    /// 设备标识
    device_id: String,
}

impl ComtradeExporter {
    /// 创建 COMTRADE 导出器
    ///
    /// # 参数
    ///
    /// * `waveforms_dir` - 波形文件存储目录
    /// * `export_dir` - 导出文件输出目录
    /// * `device_id` - 设备标识字符串
    pub fn new(waveforms_dir: PathBuf, export_dir: PathBuf, device_id: String) -> Self {
        Self {
            waveforms_dir,
            export_dir,
            device_id,
        }
    }

    /// 导出 COMTRADE .cfg 配置文件
    ///
    /// # 参数
    ///
    /// * `output_path` - 输出文件路径
    /// * `meta` - 波形元数据
    /// * `channel_names` - 通道名称列表
    ///
    /// # COMTRADE .cfg 格式示例
    ///
    /// ```text
    /// MUPC_DEVICE,20260529_143022
    /// 10,3A,0D
    /// 1,Ua,,,V,0.007629,0,0,65535,1,0,p
    /// ...
    /// 4000
    /// 4800
    /// 2026-05-29,14:30:22.000000
    /// 2026-05-29,14:30:23.200000
    /// ASCII
    /// ```
    pub fn export_cfg(
        &self,
        output_path: &Path,
        meta: &WaveformMeta,
        channel_names: &[String],
    ) -> Result<PathBuf, ExportError> {
        self.ensure_dir(output_path)?;

        let mut file = BufWriter::new(File::create(output_path)?);

        // 站点名称, 录波设备标识
        let station_name = format!(
            "{},{}",
            self.device_id,
            self.timestamp_to_filename(meta.timestamp)
        );
        writeln!(file, "{}", station_name)?;

        // 通道总数, 模拟通道数(A), 数字通道数(D)
        let total_channels = channel_names.len(); // 无数字通道
        writeln!(file, "{},{}A,{}D", total_channels, channel_names.len(), 0)?;

        // 模拟通道定义
        for (i, name) in channel_names.iter().enumerate() {
            let (a, b) = self.get_comtrade_coefficients(name);
            writeln!(file, "{},{},,,V,{},{},0,65535,1,0,p", i + 1, name, a, b)?;
        }

        // 采样率
        writeln!(file, "{}", meta.sample_rate)?;

        // 总样本数
        writeln!(file, "{}", meta.sample_count)?;

        // 触发时间
        let trigger_dt = self.timestamp_to_datetime(meta.timestamp);
        writeln!(file, "{}", trigger_dt)?;

        // 结束时间
        let duration_us = (meta.sample_count as f64 / meta.sample_rate as f64 * 1_000_000.0) as i64;
        let end_dt = self.timestamp_to_datetime(meta.timestamp + duration_us);
        writeln!(file, "{}", end_dt)?;

        // 文件类型: ASCII
        writeln!(file, "ASCII")?;

        file.flush()?;
        Ok(output_path.to_path_buf())
    }

    /// 导出 COMTRADE .dat 数据文件（ASCII 格式）
    ///
    /// # 参数
    ///
    /// * `output_path` - 输出文件路径
    /// * `channels` - 各通道的采样数据
    ///
    /// 每行一个采样点，各通道值用逗号分隔。
    pub fn export_dat(
        &self,
        output_path: &Path,
        channels: &[Vec<f64>],
        sample_rate: u32,
        pre_trigger_samples: u32,
    ) -> Result<PathBuf, ExportError> {
        if channels.is_empty() || channels[0].is_empty() {
            return Err(ExportError::EmptyData);
        }

        self.ensure_dir(output_path)?;

        let mut file = BufWriter::new(File::create(output_path)?);
        let sample_count = channels[0].len();
        let channel_count = channels.len();

        let us_per_sample = 1_000_000.0 / sample_rate as f64;

        for sample_idx in 0..sample_count {
            // 序号
            write!(file, "{}", sample_idx + 1)?;
            // 时间戳（微秒偏移，触发前为负）
            let time_offset =
                (sample_idx as i64 - pre_trigger_samples as i64) * (us_per_sample as i64);
            write!(file, ",{}", time_offset)?;

            // 各通道值
            for item in channels.iter().take(channel_count) {
                write!(file, ",{}", item[sample_idx])?;
            }
            writeln!(file)?;
        }

        file.flush()?;
        Ok(output_path.to_path_buf())
    }

    /// 获取 COMTRADE 转换系数
    ///
    /// 返回 (a, b) 系数对，用于 COMTRADE 的线性转换 y = a*x + b
    fn get_comtrade_coefficients(&self, channel_name: &str) -> (f64, f64) {
        match channel_name {
            "Ua" | "Ub" | "Uc" => (500.0 / 65536.0, 0.0), // 电压 0~500V
            "Ia" | "Ib" | "Ic" => (2000.0 / 65536.0, 0.0), // 电流 0~2000A
            "U0" => (100.0 / 65536.0, 0.0),               // 零序电压 0~100V
            "I0" => (200.0 / 65536.0, 0.0),               // 零序电流 0~200A
            "P" | "Q" => (5000.0 / 65536.0, 0.0),         // 功率 0~5000kW
            _ => (1.0, 0.0),                              // 默认
        }
    }

    /// 时间戳转 COMTRADE 日期时间格式
    fn timestamp_to_datetime(&self, timestamp_us: i64) -> String {
        // 微秒时间戳转为日期时间字符串
        let secs = timestamp_us / 1_000_000;
        let micros = timestamp_us % 1_000_000;

        // 简单的秒转日期（UNIX epoch based）
        let days = secs / 86400;
        let remaining_secs = secs % 86400;

        // 计算年月日（简化算法）
        let (year, month, day) = self.days_to_date(days);
        let hours = remaining_secs / 3600;
        let minutes = (remaining_secs % 3600) / 60;
        let seconds = remaining_secs % 60;

        format!(
            "{:04}-{:02}-{:02},{:02}:{:02}:{:02}.{:06}",
            year, month, day, hours, minutes, seconds, micros
        )
    }

    /// 距离 UNIX epoch 的天数转日期（简化版）
    fn days_to_date(&self, days: i64) -> (i64, u32, u32) {
        // 从 1970-01-01 开始计算
        let mut y = 1970i64;
        let mut d = days;

        loop {
            let days_in_year = if self.is_leap(y) { 366 } else { 365 };
            if d < days_in_year {
                break;
            }
            d -= days_in_year;
            y += 1;
        }

        // 计算月份和日期
        let months_days: [i64; 12] = if self.is_leap(y) {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        let mut m = 1u32;
        for md in months_days.iter() {
            if d < *md {
                break;
            }
            d -= *md;
            m += 1;
        }

        (y, m, (d + 1) as u32)
    }

    fn is_leap(&self, year: i64) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    /// 时间戳转文件名友好的日期时间字符串
    fn timestamp_to_filename(&self, timestamp_us: i64) -> String {
        let (y, m, d) = self.days_to_date(timestamp_us / 1_000_000 / 86400);
        let remaining = (timestamp_us / 1_000_000) % 86400;
        let h = remaining / 3600;
        let min = (remaining % 3600) / 60;
        let s = remaining % 60;
        format!("{:04}{:02}{:02}_{:02}{:02}{:02}", y, m, d, h, min, s)
    }

    /// 确保输出目录存在
    fn ensure_dir(&self, path: &Path) -> Result<(), ExportError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

/// CSV 导出器
///
/// 将波形数据导出为 CSV 格式（UTF-8 with BOM）。
#[allow(dead_code)]
pub struct CsvExporter {
    /// 导出目标目录
    export_dir: PathBuf,
}

impl CsvExporter {
    /// 创建 CSV 导出器
    pub fn new(export_dir: PathBuf) -> Self {
        Self { export_dir }
    }

    /// 导出 CSV 文件
    ///
    /// # 参数
    ///
    /// * `output_path` - 输出文件路径
    /// * `channels` - 各通道的采样数据
    /// * `channel_names` - 通道名称列表
    /// * `sample_rate` - 采样率 (Hz)，用于计算相对时间偏移
    ///
    /// # CSV 格式
    ///
    /// ```csv
    /// Timestamp_ms,Ua,Ub,Uc,Ia,Ib,Ic,U0,I0,P,Q
    /// -40.000,220.1,220.3,219.8,10.5,10.3,10.7,0.1,0.0,4850.0,120.0
    /// -39.750,220.2,220.4,219.9,10.6,10.4,10.8,0.1,0.0,4855.0,118.0
    /// ...
    /// ```
    pub fn export_csv(
        &self,
        output_path: &Path,
        channels: &[Vec<f64>],
        channel_names: &[String],
        sample_rate: u32,
    ) -> Result<PathBuf, ExportError> {
        if channels.is_empty() || channels[0].is_empty() {
            return Err(ExportError::EmptyData);
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = BufWriter::new(File::create(output_path)?);

        // UTF-8 BOM
        file.write_all(&[0xEF, 0xBB, 0xBF])?;

        // 表头
        write!(file, "Timestamp_ms")?;
        for name in channel_names {
            write!(file, ",{}", name)?;
        }
        writeln!(file)?;

        // 数据行
        let sample_count = channels[0].len();
        let dt_ms = 1000.0 / sample_rate as f64;

        for sample_idx in 0..sample_count {
            // 相对时间偏移 (ms) — 从第一个样本开始计数
            let time_offset = sample_idx as f64 * dt_ms;
            write!(file, "{:.3}", time_offset)?;

            for item in channels {
                write!(file, ",{}", item[sample_idx])?;
            }
            writeln!(file)?;
        }

        file.flush()?;
        Ok(output_path.to_path_buf())
    }
}

/// 默认的 10 通道名称列表
pub const DEFAULT_CHANNEL_NAMES: [&str; 10] = [
    "Ua", "Ub", "Uc", // 三相电压
    "Ia", "Ib", "Ic", // 三相电流
    "U0", // 零序电压
    "I0", // 零序电流
    "P",  // 有功功率
    "Q",  // 无功功率
];

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_export_comtrade_cfg() {
        let dir = tempdir().unwrap();
        let exporter = ComtradeExporter::new(
            PathBuf::from("/tmp/waveforms"),
            dir.path().to_path_buf(),
            "MUPC001".to_string(),
        );

        let meta = WaveformMeta {
            event_id: 1,
            timestamp: 1700000000_000000,
            sample_rate: 4000,
            channel_count: 10,
            sample_count: 4800,
            trigger_type: "OVER_VOLTAGE".to_string(),
            ..Default::default()
        };

        let names: Vec<String> = DEFAULT_CHANNEL_NAMES
            .iter()
            .map(|s| s.to_string())
            .collect();
        let cfg_path = dir.path().join("test.cfg");
        let result = exporter.export_cfg(&cfg_path, &meta, &names);
        assert!(result.is_ok());
        assert!(cfg_path.exists());

        let content = fs::read_to_string(&cfg_path).unwrap();
        assert!(content.contains("MUPC001"));
        assert!(content.contains("10A,0D"));
        assert!(content.contains("4000"));
        assert!(content.contains("4800"));
        assert!(content.contains("ASCII"));
    }

    #[test]
    fn test_export_csv() {
        let dir = tempdir().unwrap();
        let exporter = CsvExporter::new(dir.path().to_path_buf());

        let channels: Vec<Vec<f64>> = vec![vec![220.0, 221.0, 222.0], vec![10.0, 11.0, 12.0]];
        let names: Vec<String> = vec!["Ua".to_string(), "Ia".to_string()];

        let csv_path = dir.path().join("test.csv");
        let result = exporter.export_csv(&csv_path, &channels, &names, 4000);
        assert!(result.is_ok());
        assert!(csv_path.exists());

        let content = fs::read_to_string(&csv_path).unwrap();
        assert!(content.contains("Timestamp_ms,Ua,Ia"));
        assert!(content.contains("220,10"));
        assert!(content.contains("221,11"));
        assert!(content.contains("222,12"));
    }
}
