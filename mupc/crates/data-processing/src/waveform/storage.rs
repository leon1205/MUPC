//! 波形文件存储
//!
//! 采用自定义二进制 .wave 格式进行波形数据的持久化存储。
//! 格式包含 64 字节文件头 + 通道数据体 + 时间戳数组 + CRC64 校验和。
//!
//! # 文件格式
//!
//! ```text
//! Header (64 bytes): magic + version + channel_count + sample_count + ...
//! Channel 0 samples (N × f64)
//! Channel 1 samples (N × f64)
//! ...
//! Timestamps (N × i64)
//! Footer: CRC64 checksum (8 bytes)
//! ```

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// .wave 文件魔数: "WAVE" = 0x57415645
const WAVE_MAGIC: u32 = 0x5741_5645;
/// 文件格式版本号
const WAVE_VERSION: u16 = 0x0001;
/// 文件头大小（字节）
const HEADER_SIZE: u64 = 64;
/// CRC64 校验和大小（字节）
const FOOTER_SIZE: u64 = 8;

/// 波形元数据
///
/// 描述一次故障录波的基本信息。
#[derive(Debug, Clone)]
pub struct WaveformMeta {
    /// 关联的故障事件 ID
    pub event_id: i64,
    /// 触发时间戳（微秒）
    pub timestamp: i64,
    /// 采样率 (Hz)
    pub sample_rate: u32,
    /// 通道数量
    pub channel_count: u16,
    /// 每通道采样点数
    pub sample_count: u64,
    /// 触发类型名称
    pub trigger_type: String,
    /// 故障前采样点数
    pub pre_trigger_samples: u32,
    /// 故障后采样点数
    pub post_trigger_samples: u32,
    /// 通道启用掩码
    pub channel_mask: u32,
    /// 数据质量: 0=good, 1=gap_detected, 2=major_gap
    pub data_quality: u8,
    /// 时间质量: 0=synchronized, 1=unsynchronized
    pub time_quality: u8,
}

impl Default for WaveformMeta {
    fn default() -> Self {
        Self {
            event_id: 0,
            timestamp: 0,
            sample_rate: 4000,
            channel_count: 10,
            sample_count: 0,
            trigger_type: String::new(),
            pre_trigger_samples: 0,
            post_trigger_samples: 0,
            channel_mask: 0x3FF,
            data_quality: 0,
            time_quality: 0,
        }
    }
}

/// 波形文件写入器
///
/// 支持流式写入：先写文件头，逐通道写数据，最后写时间戳和校验和。
pub struct WaveformWriter {
    /// 输出文件
    writer: BufWriter<File>,
    /// 输出路径
    path: PathBuf,
    /// CRC64 累加器
    crc: crc64::Digest,
}

impl WaveformWriter {
    /// 创建新的波形文件并写入文件头
    ///
    /// # 参数
    ///
    /// * `path` - 输出文件路径
    /// * `meta` - 波形元数据
    ///
    /// # 错误
    ///
    /// 文件创建失败或写入错误时返回 `std::io::Error`
    pub fn create(path: &Path, meta: &WaveformMeta) -> std::io::Result<Self> {
        // 确保父目录存在
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        let mut crc = crc64::Digest::new();

        // 写入文件头 (64 bytes)
        let header = build_header(meta);
        writer.write_all(&header)?;
        crc.write(&header);

        // 确保文件头占满 64 字节
        let pos = writer.stream_position()?;
        if pos < HEADER_SIZE {
            let padding = vec![0u8; (HEADER_SIZE - pos) as usize];
            writer.write_all(&padding)?;
            crc.write(&padding);
        }

        writer.flush()?;

        Ok(Self {
            writer,
            path: path.to_path_buf(),
            crc,
        })
    }

    /// 写入一个通道的全部采样数据
    ///
    /// # 参数
    ///
    /// * `samples` - 该通道的采样数据（f64 数组）
    pub fn write_channel(&mut self, samples: &[f64]) -> std::io::Result<()> {
        let bytes = f64_slice_to_bytes(samples);
        self.writer.write_all(&bytes)?;
        self.crc.write(&bytes);
        Ok(())
    }

    /// 写入时间戳数组
    ///
    /// # 参数
    ///
    /// * `timestamps` - 微秒时间戳数组
    pub fn write_timestamps(&mut self, timestamps: &[i64]) -> std::io::Result<()> {
        let bytes = i64_slice_to_bytes(timestamps);
        self.writer.write_all(&bytes)?;
        self.crc.write(&bytes);
        Ok(())
    }

    /// 完成写入，追加 CRC64 校验和并关闭文件
    ///
    /// # 返回
    ///
    /// 包含文件路径和校验和的结构体
    pub fn finalize(mut self) -> std::io::Result<WaveformFileInfo> {
        let checksum = self.crc.sum64();
        self.writer.write_all(&checksum.to_le_bytes())?;
        self.writer.flush()?;

        let file_size = fs::metadata(&self.path)?.len();

        Ok(WaveformFileInfo {
            path: self.path,
            file_size,
            checksum,
        })
    }
}

/// 波形文件读取器
///
/// 支持按需读取：读取元数据、读取指定通道、读取全部数据，以及 CRC 校验。
pub struct WaveformReader {
    /// 输入文件
    reader: BufReader<File>,
    /// 文件路径
    path: PathBuf,
    /// 波形元数据
    pub meta: WaveformMeta,
}

impl WaveformReader {
    /// 打开波形文件并读取文件头
    ///
    /// # 参数
    ///
    /// * `path` - 波形文件路径
    ///
    /// # 错误
    ///
    /// 文件不存在、格式错误或读取错误时返回 `std::io::Error`
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();
        let mut reader = BufReader::new(file);

        // 读取文件头
        let mut header_buf = [0u8; 64];
        reader.read_exact(&mut header_buf)?;

        let meta = parse_header(&header_buf)?;

        // 验证魔数
        let magic = u32::from_le_bytes([header_buf[0], header_buf[1], header_buf[2], header_buf[3]]);
        if magic != WAVE_MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("无效的 .wave 文件魔数: 0x{:08X}, 期望 0x{:08X}", magic, WAVE_MAGIC),
            ));
        }

        Ok(Self {
            reader,
            path: path.to_path_buf(),
            meta,
        })
    }

    /// 读取全部通道数据和对应的时间戳
    ///
    /// # 返回
    ///
    /// `(通道数据列表, 时间戳列表)`
    pub fn read_all(&mut self) -> std::io::Result<(Vec<Vec<f64>>, Vec<i64>)> {
        let channel_count = self.meta.channel_count as usize;
        let sample_count = self.meta.sample_count as usize;

        // 定位到文件头之后
        self.reader.seek(SeekFrom::Start(HEADER_SIZE))?;

        let mut channels = Vec::with_capacity(channel_count);
        for _ in 0..channel_count {
            let mut samples = vec![0.0f64; sample_count];
            let bytes = sample_count * 8;
            let mut buf = vec![0u8; bytes];
            self.reader.read_exact(&mut buf)?;
            bytes_to_f64_slice(&buf, &mut samples);
            channels.push(samples);
        }

        // 读取时间戳
        let mut timestamps = vec![0i64; sample_count];
        let ts_bytes = sample_count * 8;
        let mut ts_buf = vec![0u8; ts_bytes];
        self.reader.read_exact(&mut ts_buf)?;
        bytes_to_i64_slice(&ts_buf, &mut timestamps);

        Ok((channels, timestamps))
    }

    /// 读取指定通道的数据
    ///
    /// # 参数
    ///
    /// * `channel_index` - 通道索引 (0-based)
    ///
    /// # 返回
    ///
    /// 该通道的完整采样数据
    pub fn read_channel(&mut self, channel_index: usize) -> std::io::Result<Vec<f64>> {
        if channel_index >= self.meta.channel_count as usize {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "通道索引 {} 超出范围 [0, {})",
                    channel_index, self.meta.channel_count
                ),
            ));
        }

        let sample_count = self.meta.sample_count as usize;
        let offset = HEADER_SIZE + (channel_index * sample_count * 8) as u64;

        self.reader.seek(SeekFrom::Start(offset))?;

        let mut samples = vec![0.0f64; sample_count];
        let bytes = sample_count * 8;
        let mut buf = vec![0u8; bytes];
        self.reader.read_exact(&mut buf)?;
        bytes_to_f64_slice(&buf, &mut samples);

        Ok(samples)
    }

    /// 验证 CRC64 校验和
    ///
    /// 重新计算文件内容的 CRC64 并与存储的校验和比对。
    ///
    /// # 返回
    ///
    /// `true` 表示校验通过，`false` 表示校验失败
    pub fn verify_checksum(&mut self) -> std::io::Result<bool> {
        let file_size = self.path.metadata()?.len();
        if file_size < HEADER_SIZE + FOOTER_SIZE {
            return Ok(false);
        }

        let data_size = file_size - FOOTER_SIZE;

        // 读取所有数据部分计算 CRC
        self.reader.seek(SeekFrom::Start(0))?;
        let mut crc = crc64::Digest::new();
        let mut remaining = data_size;
        let mut buf = [0u8; 8192];

        while remaining > 0 {
            let to_read = remaining.min(buf.len() as u64) as usize;
            let n = self.reader.read(&mut buf[..to_read])?;
            if n == 0 {
                break;
            }
            crc.write(&buf[..n]);
            remaining -= n as u64;
        }

        // 读取存储的校验和
        self.reader.seek(SeekFrom::Start(data_size))?;
        let mut stored_checksum_buf = [0u8; 8];
        self.reader.read_exact(&mut stored_checksum_buf)?;
        let stored_checksum = u64::from_le_bytes(stored_checksum_buf);

        Ok(crc.sum64() == stored_checksum)
    }
}

/// 波形文件信息
#[derive(Debug, Clone)]
pub struct WaveformFileInfo {
    /// 文件路径
    pub path: PathBuf,
    /// 文件大小（字节）
    pub file_size: u64,
    /// CRC64 校验和
    pub checksum: u64,
}

// === 辅助函数 ===

/// 构建 64 字节文件头
fn build_header(meta: &WaveformMeta) -> Vec<u8> {
    let mut header = vec![0u8; 64];

    // 0..4: magic
    header[0..4].copy_from_slice(&WAVE_MAGIC.to_le_bytes());
    // 4..6: version
    header[4..6].copy_from_slice(&WAVE_VERSION.to_le_bytes());
    // 6..8: channel_count
    header[6..8].copy_from_slice(&meta.channel_count.to_le_bytes());
    // 8..12: channel_mask
    header[8..12].copy_from_slice(&meta.channel_mask.to_le_bytes());
    // 12..16: reserved
    // (already zeros)
    // 16..24: sample_count
    header[16..24].copy_from_slice(&meta.sample_count.to_le_bytes());
    // 24..32: sample_rate
    header[24..32].copy_from_slice(&(meta.sample_rate as u64).to_le_bytes());
    // 32..40: trigger_timestamp
    header[32..40].copy_from_slice(&meta.timestamp.to_le_bytes());
    // 40..48: trigger_offset (保留)
    // (already zeros)
    // 48..52: pre_trigger_nsamples
    header[48..52].copy_from_slice(&meta.pre_trigger_samples.to_le_bytes());
    // 52..56: post_trigger_nsamples
    header[52..56].copy_from_slice(&meta.post_trigger_samples.to_le_bytes());
    // 56..60: event_id
    header[56..60].copy_from_slice(&(meta.event_id as u32).to_le_bytes());
    // 60: data_quality
    header[60] = meta.data_quality;
    // 61: time_quality
    header[61] = meta.time_quality;
    // 62..64: reserved
    // (already zeros)

    header
}

/// 解析 64 字节文件头
fn parse_header(header: &[u8; 64]) -> std::io::Result<WaveformMeta> {
    let channel_count = u16::from_le_bytes([header[6], header[7]]);
    let channel_mask = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    let sample_count = u64::from_le_bytes([
        header[16], header[17], header[18], header[19],
        header[20], header[21], header[22], header[23],
    ]);
    let sample_rate = u64::from_le_bytes([
        header[24], header[25], header[26], header[27],
        header[28], header[29], header[30], header[31],
    ]);
    let timestamp = i64::from_le_bytes([
        header[32], header[33], header[34], header[35],
        header[36], header[37], header[38], header[39],
    ]);
    let pre_trigger_samples = u32::from_le_bytes([header[48], header[49], header[50], header[51]]);
    let post_trigger_samples = u32::from_le_bytes([header[52], header[53], header[54], header[55]]);
    let event_id = u32::from_le_bytes([header[56], header[57], header[58], header[59]]) as i64;
    let data_quality = header[60];
    let time_quality = header[61];

    Ok(WaveformMeta {
        event_id,
        timestamp,
        sample_rate: sample_rate as u32,
        channel_count,
        sample_count,
        trigger_type: String::new(),
        pre_trigger_samples,
        post_trigger_samples,
        channel_mask,
        data_quality,
        time_quality,
    })
}

/// 将 f64 切片转换为字节数组
fn f64_slice_to_bytes(samples: &[f64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 8);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    bytes
}

/// 将字节数组解析为 f64 切片
fn bytes_to_f64_slice(bytes: &[u8], out: &mut [f64]) {
    for (i, chunk) in bytes.chunks_exact(8).enumerate() {
        if i < out.len() {
            out[i] = f64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3],
                chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
        }
    }
}

/// 将 i64 切片转换为字节数组
fn i64_slice_to_bytes(samples: &[i64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 8);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    bytes
}

/// 将字节数组解析为 i64 切片
fn bytes_to_i64_slice(bytes: &[u8], out: &mut [i64]) {
    for (i, chunk) in bytes.chunks_exact(8).enumerate() {
        if i < out.len() {
            out[i] = i64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3],
                chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_write_and_read_waveform() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.wave");

        let meta = WaveformMeta {
            event_id: 1,
            timestamp: 1700000000_000000,
            sample_rate: 4000,
            channel_count: 2,
            sample_count: 100,
            trigger_type: "OVER_VOLTAGE".to_string(),
            pre_trigger_samples: 50,
            post_trigger_samples: 50,
            channel_mask: 0x3,
            data_quality: 0,
            time_quality: 0,
        };

        // 写入
        let mut writer = WaveformWriter::create(&path, &meta).unwrap();

        let channel0: Vec<f64> = (0..100).map(|i| i as f64 * 1.0).collect();
        let channel1: Vec<f64> = (0..100).map(|i| i as f64 * 2.0).collect();
        let timestamps: Vec<i64> = (0..100).map(|i| meta.timestamp + i * 250).collect();

        writer.write_channel(&channel0).unwrap();
        writer.write_channel(&channel1).unwrap();
        writer.write_timestamps(&timestamps).unwrap();

        let info = writer.finalize().unwrap();
        assert!(info.file_size > 0);
        assert!(info.checksum != 0);

        // 读取
        let mut reader = WaveformReader::open(&path).unwrap();
        assert_eq!(reader.meta.event_id, 1);
        assert_eq!(reader.meta.channel_count, 2);
        assert_eq!(reader.meta.sample_count, 100);
        assert_eq!(reader.meta.sample_rate, 4000);

        let (channels, ts) = reader.read_all().unwrap();
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].len(), 100);
        assert_eq!(channels[1].len(), 100);
        assert_eq!(ts.len(), 100);

        // 验证数据
        for i in 0..100 {
            assert!((channels[0][i] - i as f64).abs() < 1e-10);
            assert!((channels[1][i] - (i as f64 * 2.0)).abs() < 1e-10);
        }
    }

    #[test]
    fn test_read_single_channel() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("single_ch.wave");

        let meta = WaveformMeta {
            event_id: 2,
            timestamp: 1700000000_000000,
            sample_rate: 1000,
            channel_count: 3,
            sample_count: 50,
            ..Default::default()
        };

        let mut writer = WaveformWriter::create(&path, &meta).unwrap();
        for ch in 0..3 {
            let samples: Vec<f64> = (0..50).map(|i| (ch * 100 + i) as f64).collect();
            writer.write_channel(&samples).unwrap();
        }
        let timestamps: Vec<i64> = (0..50).map(|i| meta.timestamp + i * 1000).collect();
        writer.write_timestamps(&timestamps).unwrap();
        writer.finalize().unwrap();

        let mut reader = WaveformReader::open(&path).unwrap();
        let ch1 = reader.read_channel(1).unwrap();
        assert_eq!(ch1.len(), 50);
        assert!((ch1[0] - 100.0).abs() < 1e-10);
        assert!((ch1[10] - 110.0).abs() < 1e-10);
    }
}
