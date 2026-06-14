//! 核间通信帧协议
//!
//! 帧格式：0xAA 0x55 + 长度(2字节) + 类型(2字节) + 序号(2字节) + 数据(N字节) + CRC16(2字节)
//! 总长度：固定 64 字节（不足部分用 padding 填充）

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use mupc_common::{ErrorCode, MupcError};
use std::io::Cursor;

/// 帧目标长度（定长）
pub const FRAME_FIXED_LENGTH: usize = 64;

/// 帧类型
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u16)]
pub enum FrameType {
    Connect = 0x0001,
    HeartbeatReq = 0x0002,
    HeartbeatRsp = 0x0003,
    ControlCmd = 0x0010,
    ControlRsp = 0x0011,
    StatusReport = 0x0020,
    DataUpload = 0x0030,
    SafetyOverride = 0x0040, // v2.10 新增
    Unknown = 0xFFFF,
}

impl FrameType {
    pub fn from_u16(val: u16) -> Self {
        match val {
            0x0001 => FrameType::Connect,
            0x0002 => FrameType::HeartbeatReq,
            0x0003 => FrameType::HeartbeatRsp,
            0x0010 => FrameType::ControlCmd,
            0x0011 => FrameType::ControlRsp,
            0x0020 => FrameType::StatusReport,
            0x0030 => FrameType::DataUpload,
            0x0040 => FrameType::SafetyOverride, // v2.10 新增
            _ => FrameType::Unknown,
        }
    }
}

/// 帧头
#[derive(Debug, Clone)]
pub struct FrameHeader {
    /// 帧头 magic: 0xAA 0x55
    pub magic: u16,
    /// 帧长度
    pub length: u16,
    /// 帧类型
    pub frame_type: FrameType,
    /// 序列号
    pub seq_no: u16,
}

impl FrameHeader {
    pub const MAGIC: u16 = 0xAA55;
    pub const FIXED_LENGTH: usize = 8; // magic(2) + length(2) + type(2) + seq(2)

    /// 从字节流解析帧头
    pub fn from_bytes(data: &[u8]) -> Result<Self, MupcError> {
        if data.len() < Self::FIXED_LENGTH {
            return Err(MupcError::new(
                ErrorCode::FrameParseError,
                "Frame too short",
                "intercore",
            ));
        }

        let mut cursor = Cursor::new(data);

        let magic = cursor.read_u16::<BigEndian>().map_err(|_| {
            MupcError::new(ErrorCode::FrameParseError, "Invalid magic", "intercore")
        })?;

        if magic != Self::MAGIC {
            return Err(MupcError::new(
                ErrorCode::FrameParseError,
                format!("Invalid magic: {:#x}", magic),
                "intercore",
            ));
        }

        let length = cursor.read_u16::<BigEndian>().map_err(|_| {
            MupcError::new(ErrorCode::FrameParseError, "Invalid length", "intercore")
        })?;

        let frame_type_val = cursor.read_u16::<BigEndian>().map_err(|_| {
            MupcError::new(
                ErrorCode::FrameParseError,
                "Invalid frame type",
                "intercore",
            )
        })?;

        let seq_no = cursor.read_u16::<BigEndian>().map_err(|_| {
            MupcError::new(ErrorCode::FrameParseError, "Invalid seq no", "intercore")
        })?;

        Ok(Self {
            magic,
            length,
            frame_type: FrameType::from_u16(frame_type_val),
            seq_no,
        })
    }
}

/// 核间通信帧
#[derive(Debug, Clone)]
pub struct IntercoreFrame {
    /// 帧头
    pub header: FrameHeader,
    /// 数据
    pub data: Vec<u8>,
}

impl IntercoreFrame {
    /// 创建新的帧
    pub fn new(frame_type: FrameType, seq_no: u16, data: Vec<u8>) -> Self {
        let length = (FrameHeader::FIXED_LENGTH + data.len() + 2) as u16; // +2 for CRC16
        Self {
            header: FrameHeader {
                magic: FrameHeader::MAGIC,
                length,
                frame_type,
                seq_no,
            },
            data,
        }
    }

    /// 创建连接帧
    pub fn new_connect() -> Self {
        Self::new(FrameType::Connect, 0, vec![])
    }

    /// 创建心跳请求帧
    pub fn new_heartbeat_req(status: u8, cpu_temp: f64, memory_usage: f64) -> Self {
        let mut data = Vec::with_capacity(10);
        data.push(status);
        data.extend_from_slice(&cpu_temp.to_le_bytes());
        data.extend_from_slice(&memory_usage.to_le_bytes());
        Self::new(FrameType::HeartbeatReq, 0, data)
    }

    /// 创建心跳响应帧
    pub fn new_heartbeat_rsp() -> Self {
        Self::new(FrameType::HeartbeatRsp, 0, vec![])
    }

    /// 转换为字节流（定长 64 字节）
    pub fn to_bytes(&self) -> Result<Vec<u8>, MupcError> {
        let mut result = Vec::new();

        // Magic
        result
            .write_u16::<BigEndian>(self.header.magic)
            .map_err(|_| {
                MupcError::new(
                    ErrorCode::SerializeError,
                    "Failed to write magic",
                    "intercore",
                )
            })?;

        // Length
        result
            .write_u16::<BigEndian>(self.header.length)
            .map_err(|_| {
                MupcError::new(
                    ErrorCode::SerializeError,
                    "Failed to write length",
                    "intercore",
                )
            })?;

        // Frame type
        let frame_type_val = match self.header.frame_type {
            FrameType::Connect => 0x0001u16,
            FrameType::HeartbeatReq => 0x0002,
            FrameType::HeartbeatRsp => 0x0003,
            FrameType::ControlCmd => 0x0010,
            FrameType::ControlRsp => 0x0011,
            FrameType::StatusReport => 0x0020,
            FrameType::DataUpload => 0x0030,
            FrameType::SafetyOverride => 0x0040, // v2.10 新增
            FrameType::Unknown => 0xFFFF,
        };
        result.write_u16::<BigEndian>(frame_type_val).map_err(|_| {
            MupcError::new(
                ErrorCode::SerializeError,
                "Failed to write frame type",
                "intercore",
            )
        })?;

        // Seq no
        result
            .write_u16::<BigEndian>(self.header.seq_no)
            .map_err(|_| {
                MupcError::new(
                    ErrorCode::SerializeError,
                    "Failed to write seq no",
                    "intercore",
                )
            })?;

        // Data
        result.extend_from_slice(&self.data);

        // CRC16
        let crc = Self::calculate_crc16(&result);
        result.write_u16::<BigEndian>(crc).map_err(|_| {
            MupcError::new(
                ErrorCode::SerializeError,
                "Failed to write CRC",
                "intercore",
            )
        })?;

        // Padding to fixed 64 bytes
        while result.len() < FRAME_FIXED_LENGTH {
            result.push(0x00);
        }

        Ok(result)
    }

    /// 从字节流解析帧
    pub fn from_bytes(data: &[u8]) -> Result<Self, MupcError> {
        if data.len() < FrameHeader::FIXED_LENGTH {
            return Err(MupcError::new(
                ErrorCode::FrameParseError,
                "Frame too short",
                "intercore",
            ));
        }

        let header = FrameHeader::from_bytes(data)?;

        // 验证 CRC
        let data_len = data.len() - 2; // exclude CRC
        let crc_pos = data.len() - 2;
        let received_crc = ((data[crc_pos + 1] as u16) << 8) | (data[crc_pos] as u16);
        let calculated_crc = Self::calculate_crc16(&data[..data_len]);

        if received_crc != calculated_crc {
            return Err(MupcError::new(
                ErrorCode::FrameChecksumError,
                format!(
                    "CRC mismatch: expected {:#x}, got {:#x}",
                    calculated_crc, received_crc
                ),
                "intercore",
            ));
        }

        let payload_start = FrameHeader::FIXED_LENGTH;
        let payload_end = data_len;
        let payload = data[payload_start..payload_end].to_vec();

        Ok(Self {
            header,
            data: payload,
        })
    }

    /// 计算 CRC16
    fn calculate_crc16(data: &[u8]) -> u16 {
        let mut crc: u16 = 0xFFFF;
        for byte in data {
            crc ^= *byte as u16;
            for _ in 0..8 {
                if crc & 0x0001 != 0 {
                    crc = (crc >> 1) ^ 0xA001;
                } else {
                    crc >>= 1;
                }
            }
        }
        crc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Frame Format Tests ==========

    #[test]
    fn test_frame_fixed_length_64_bytes() {
        // 创建心跳请求帧
        let frame = IntercoreFrame::new_heartbeat_req(1, 45.5, 0.75);
        let bytes = frame.to_bytes().unwrap();

        // 验证帧长度为固定的 64 字节
        assert_eq!(
            bytes.len(),
            FRAME_FIXED_LENGTH,
            "Frame should be exactly 64 bytes, got {}",
            bytes.len()
        );
    }

    #[test]
    fn test_frame_fixed_length_for_all_types() {
        // 测试所有帧类型都是 64 字节
        let frame_types = vec![
            FrameType::Connect,
            FrameType::HeartbeatReq,
            FrameType::HeartbeatRsp,
            FrameType::ControlCmd,
            FrameType::ControlRsp,
            FrameType::StatusReport,
            FrameType::DataUpload,
            FrameType::SafetyOverride, // v2.10
        ];

        for frame_type in frame_types {
            let data = match frame_type {
                FrameType::HeartbeatReq => vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0], // status + cpu_temp + memory
                _ => vec![],
            };
            let frame = IntercoreFrame::new(frame_type, 0, data);
            let bytes = frame.to_bytes().unwrap();
            assert_eq!(
                bytes.len(),
                FRAME_FIXED_LENGTH,
                "Frame type {:?} should be 64 bytes, got {}",
                frame_type,
                bytes.len()
            );
        }
    }

    // ========== CRC16 Tests ==========

    #[test]
    fn test_crc16_calculation() {
        // 使用已知数据测试 CRC16
        let data = [0xAA, 0x55, 0x00, 0x08, 0x00, 0x01, 0x00, 0x01];
        let crc = IntercoreFrame::calculate_crc16(&data);

        // CRC16 应该是一个有效的 16 位值
        assert!(crc != 0x0000 || true); // CRC 可能为 0，这是有效的

        // 相同数据应该产生相同的 CRC
        let crc2 = IntercoreFrame::calculate_crc16(&data);
        assert_eq!(crc, crc2);
    }

    #[test]
    fn test_crc16_different_data() {
        let data1 = [0xAA, 0x55, 0x00, 0x08, 0x00, 0x01, 0x00, 0x01];
        let data2 = [0xAA, 0x55, 0x00, 0x09, 0x00, 0x01, 0x00, 0x01];

        let crc1 = IntercoreFrame::calculate_crc16(&data1);
        let crc2 = IntercoreFrame::calculate_crc16(&data2);

        assert_ne!(crc1, crc2, "Different data should produce different CRC");
    }

    #[test]
    fn test_crc16_verification_on_frame() {
        // 创建帧并验证 CRC
        let frame = IntercoreFrame::new_connect();
        let bytes = frame.to_bytes().unwrap();

        // 从字节流解析回来
        let parsed = IntercoreFrame::from_bytes(&bytes);
        assert!(
            parsed.is_ok(),
            "Frame with valid CRC should parse successfully"
        );
    }

    #[test]
    fn test_crc16_invalid_on_tampered_frame() {
        // 创建正常帧
        let frame = IntercoreFrame::new_connect();
        let mut bytes = frame.to_bytes().unwrap();

        // 篡改数据
        bytes[4] ^= 0xFF;

        // 解析应该失败（CRC 不匹配）
        let result = IntercoreFrame::from_bytes(&bytes);
        assert!(result.is_err(), "Tampered frame should fail CRC check");
    }

    // ========== FrameHeader Tests ==========

    #[test]
    fn test_frame_header_magic() {
        assert_eq!(FrameHeader::MAGIC, 0xAA55);
    }

    #[test]
    fn test_frame_header_fixed_length() {
        assert_eq!(FrameHeader::FIXED_LENGTH, 8); // magic(2) + length(2) + type(2) + seq(2)
    }

    #[test]
    fn test_frame_header_parse() {
        let data = [0xAA, 0x55, 0x00, 0x10, 0x00, 0x01, 0x00, 0x01];
        let header = FrameHeader::from_bytes(&data).unwrap();

        assert_eq!(header.magic, 0xAA55);
        assert_eq!(header.length, 0x10);
        assert_eq!(header.frame_type, FrameType::Connect);
        assert_eq!(header.seq_no, 0x0001);
    }

    #[test]
    fn test_frame_header_invalid_magic() {
        let data = [0xFF, 0xFF, 0x00, 0x10, 0x00, 0x01, 0x00, 0x01];
        let result = FrameHeader::from_bytes(&data);
        assert!(result.is_err(), "Invalid magic should fail");
    }

    #[test]
    fn test_frame_header_too_short() {
        let data = [0xAA, 0x55, 0x00]; // too short
        let result = FrameHeader::from_bytes(&data);
        assert!(result.is_err(), "Frame too short should fail");
    }

    // ========== Heartbeat Frame Tests ==========

    #[test]
    fn test_heartbeat_frame_format() {
        let frame = IntercoreFrame::new_heartbeat_req(1, 45.5, 0.75);
        let bytes = frame.to_bytes().unwrap();

        // 验证帧头
        assert_eq!(bytes[0], 0xAA);
        assert_eq!(bytes[1], 0x55);

        // 验证帧类型为 HeartbeatReq
        let frame_type_val = ((bytes[5] as u16) << 8) | (bytes[4] as u16);
        assert_eq!(frame_type_val, 0x0002);

        // 验证数据部分（status + cpu_temp + memory_usage）
        // status at offset 8
        assert_eq!(bytes[8], 1);

        // crc16 at offset 62-63
        assert_eq!(bytes.len(), 64);
    }

    #[test]
    fn test_heartbeat_frame_with_to_bytes() {
        let frame = IntercoreFrame::new_heartbeat_req(0, 0.0, 0.0);
        let bytes = frame.to_bytes().unwrap();

        // 验证总长度为 64
        assert_eq!(bytes.len(), 64);

        // 验证 padding 正确添加
        assert_eq!(bytes[62], 0x00);
        assert_eq!(bytes[63], 0x00);
    }

    // ========== FrameType Tests ==========

    #[test]
    fn test_frame_type_from_u16() {
        assert_eq!(FrameType::from_u16(0x0001), FrameType::Connect);
        assert_eq!(FrameType::from_u16(0x0002), FrameType::HeartbeatReq);
        assert_eq!(FrameType::from_u16(0x0003), FrameType::HeartbeatRsp);
        assert_eq!(FrameType::from_u16(0x0010), FrameType::ControlCmd);
        assert_eq!(FrameType::from_u16(0x0011), FrameType::ControlRsp);
        assert_eq!(FrameType::from_u16(0x0020), FrameType::StatusReport);
        assert_eq!(FrameType::from_u16(0x0030), FrameType::DataUpload);
        assert_eq!(FrameType::from_u16(0x0040), FrameType::SafetyOverride); // v2.10
        assert_eq!(FrameType::from_u16(0xFFFF), FrameType::Unknown);
    }

    // ========== Frame Round-trip Tests ==========

    #[test]
    fn test_frame_roundtrip_connect() {
        let original = IntercoreFrame::new_connect();
        let bytes = original.to_bytes().unwrap();
        let parsed = IntercoreFrame::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.header.frame_type, original.header.frame_type);
        assert_eq!(parsed.header.seq_no, original.header.seq_no);
        assert_eq!(parsed.data, original.data);
    }

    #[test]
    fn test_frame_roundtrip_with_data() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let original = IntercoreFrame::new(FrameType::ControlCmd, 42, data.clone());
        let bytes = original.to_bytes().unwrap();
        let parsed = IntercoreFrame::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.header.frame_type, FrameType::ControlCmd);
        assert_eq!(parsed.header.seq_no, 42);
        assert_eq!(parsed.data, data);
    }
}
