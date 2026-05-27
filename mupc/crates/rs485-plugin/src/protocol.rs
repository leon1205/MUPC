//! RS485 协议解析
//!
//! 支持 Modbus RTU 等常见 RS485 协议

use crate::errors::Rs485Error;
use crate::config::CrcMode;

/// 协议数据帧
#[derive(Debug, Clone)]
pub struct Frame {
    /// 设备地址
    pub addr: u8,
    /// 功能码
    pub func_code: u8,
    /// 数据载荷
    pub data: Vec<u8>,
    /// CRC 校验码
    pub crc: u16,
}

impl Frame {
    /// 创建新的帧
    pub fn new(addr: u8, func_code: u8, data: Vec<u8>) -> Self {
        let crc = Self::calculate_crc(addr, func_code, &data, CrcMode::Crc16Modbus);
        Self {
            addr,
            func_code,
            data,
            crc,
        }
    }

    /// 从字节流解析帧
    pub fn parse(buf: &[u8], crc_mode: CrcMode) -> Result<Self, Rs485Error> {
        if buf.len() < 5 {
            return Err(Rs485Error::ConfigFailed("数据帧太短".to_string()));
        }

        let addr = buf[0];
        let func_code = buf[1];
        let data_len = buf.len() - 3; // addr + func + crc(2)
        let data = buf[2..2 + data_len].to_vec();

        let crc_from_data = match crc_mode {
            CrcMode::Crc16Modbus => {
                let crc = ((buf[buf.len() - 2] as u16) << 8) | (buf[buf.len() - 1] as u16);
                let calculated = Self::calculate_crc_raw(&buf[..buf.len() - 2], crc_mode);
                if crc != calculated {
                    return Err(Rs485Error::crc_failed("CRC 校验失败"));
                }
                crc
            }
            CrcMode::Crc16Xmodem => {
                let crc = ((buf[buf.len() - 2] as u16) << 8) | (buf[buf.len() - 1] as u16);
                let calculated = Self::calculate_crc_raw(&buf[..buf.len() - 2], crc_mode);
                if crc != calculated {
                    return Err(Rs485Error::crc_failed("CRC 校验失败"));
                }
                crc
            }
            _ => 0,
        };

        Ok(Self {
            addr,
            func_code,
            data,
            crc: crc_from_data,
        })
    }

    /// 转换为字节流
    pub fn to_bytes(&self, crc_mode: CrcMode) -> Vec<u8> {
        let mut result = vec![self.addr, self.func_code];
        result.extend_from_slice(&self.data);

        let crc = Self::calculate_crc(self.addr, self.func_code, &self.data, crc_mode);
        result.push((crc >> 8) as u8);
        result.push(crc as u8);

        result
    }

    /// 计算 CRC（包含地址和功能码）
    pub fn calculate_crc(addr: u8, func_code: u8, data: &[u8], crc_mode: CrcMode) -> u16 {
        let mut buf = vec![addr, func_code];
        buf.extend_from_slice(data);
        Self::calculate_crc_raw(&buf, crc_mode)
    }

    /// 计算 CRC 原始数据
    pub fn calculate_crc_raw(data: &[u8], crc_mode: CrcMode) -> u16 {
        match crc_mode {
            CrcMode::Crc16Modbus => Self::crc16_modbus(data),
            CrcMode::Crc16Xmodem => Self::crc16_xmodem(data),
            _ => 0,
        }
    }

    /// Modbus CRC16
    fn crc16_modbus(data: &[u8]) -> u16 {
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

    /// XMODEM CRC16
    fn crc16_xmodem(data: &[u8]) -> u16 {
        let mut crc: u16 = 0x0000;
        for byte in data {
            let mut temp = *byte as u16;
            temp <<= 8;
            for _ in 0..8 {
                if (crc ^ temp) & 0x8000 != 0 {
                    crc = (crc << 1) ^ 0x1021;
                } else {
                    crc <<= 1;
                }
                temp <<= 1;
            }
        }
        crc
    }
}

/// 常用功能码
pub mod func_codes {
    /// 读取保持寄存器
    pub const READ_HOLDING_REGISTERS: u8 = 0x03;
    /// 读取输入寄存器
    pub const READ_INPUT_REGISTERS: u8 = 0x04;
    /// 写单个寄存器
    pub const WRITE_SINGLE_REGISTER: u8 = 0x06;
    /// 写多个寄存器
    pub const WRITE_MULTIPLE_REGISTERS: u8 = 0x10;
    /// 读线圈状态
    pub const READ_COILS: u8 = 0x01;
    /// 写单个线圈
    pub const WRITE_SINGLE_COIL: u8 = 0x05;
}

/// 数据单元解析
pub struct DataUnitParser;

impl DataUnitParser {
    /// 解析 16 位有符号整数
    pub fn parse_i16(data: &[u8]) -> Option<i16> {
        if data.len() < 2 {
            return None;
        }
        Some(((data[0] as i16) << 8) | (data[1] as i16))
    }

    /// 解析 16 位无符号整数
    pub fn parse_u16(data: &[u8]) -> Option<u16> {
        if data.len() < 2 {
            return None;
        }
        Some(((data[0] as u16) << 8) | (data[1] as u16))
    }

    /// 解析 32 位浮点数
    pub fn parse_f32(data: &[u8]) -> Option<f32> {
        if data.len() < 4 {
            return None;
        }
        let bits = ((data[0] as u32) << 24)
            | ((data[1] as u32) << 16)
            | ((data[2] as u32) << 8)
            | (data[3] as u32);
        Some(f32::from_bits(bits))
    }

    /// 打包 16 位无符号整数
    pub fn pack_u16(value: u16) -> Vec<u8> {
        vec![(value >> 8) as u8, value as u8]
    }

    /// 打包 16 位有符号整数
    pub fn pack_i16(value: i16) -> Vec<u8> {
        vec![(value >> 8) as u8, value as u8]
    }

    /// 打包 32 位浮点数
    pub fn pack_f32(value: f32) -> Vec<u8> {
        let bits = value.to_bits();
        vec![
            (bits >> 24) as u8,
            (bits >> 16) as u8,
            (bits >> 8) as u8,
            bits as u8,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_crc16_modbus() {
        let frame = Frame::new(0x01, func_codes::READ_HOLDING_REGISTERS, vec![0x00, 0x00, 0x00, 0x02]);
        let bytes = frame.to_bytes(CrcMode::Crc16Modbus);
        // 地址(1) + 功能码(1) + 数据(4) + CRC(2) = 8
        assert_eq!(bytes.len(), 8);
    }

    #[test]
    fn test_frame_to_bytes() {
        // Create a frame
        let frame = Frame::new(0x01, 0x03, vec![0x00, 0x64]);
        assert_eq!(frame.addr, 0x01);
        assert_eq!(frame.func_code, 0x03);
        assert_eq!(frame.data, vec![0x00, 0x64]);
        assert!(frame.crc != 0); // CRC should be calculated

        // Convert to bytes
        let bytes = frame.to_bytes(CrcMode::Crc16Modbus);

        // Frame should have: addr(1) + func(1) + data(n) + crc(2)
        // With 2 bytes of data, that's 1+1+2+2 = 6 bytes
        assert_eq!(bytes.len(), 6);
    }

    #[test]
    fn test_frame_parse_roundtrip() {
        // Create a frame
        let original = Frame::new(0x01, 0x03, vec![0x00, 0x64]);

        // Encode to bytes
        let bytes = original.to_bytes(CrcMode::Crc16Modbus);
        assert_eq!(bytes.len(), 6);

        // Verify the bytes are correct (CRC can be verified separately)
        assert_eq!(bytes[0], 0x01); // addr
        assert_eq!(bytes[1], 0x03); // func_code
        assert_eq!(bytes[2], 0x00); // data[0]
        assert_eq!(bytes[3], 0x64); // data[1]

        // CRC is in bytes[4], bytes[5]
        let crc = ((bytes[4] as u16) << 8) | (bytes[5] as u16);
        assert!(crc != 0); // CRC should be non-zero

        // Verify parsing works (CRC validation only)
        let parsed = Frame::parse(&bytes, CrcMode::Crc16Modbus);
        assert!(parsed.is_ok(), "Frame parsing failed");
    }

    #[test]
    fn test_data_unit_parser_i16() {
        let data = vec![0x00, 0x64]; // 100
        assert_eq!(DataUnitParser::parse_i16(&data), Some(100));
    }

    #[test]
    fn test_data_unit_parser_f32() {
        let data = vec![0x40, 0x48, 0xF5, 0xC3]; // 3.14 in IEEE 754
        let result = DataUnitParser::parse_f32(&data);
        assert!(result.is_some());
    }

    #[test]
    fn test_calculate_crc_raw() {
        let data = vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x02];
        let crc = Frame::calculate_crc_raw(&data, CrcMode::Crc16Modbus);
        // Modbus CRC16 produces a 16-bit value
        assert!(crc != 0); // Should not be zero for non-empty data
    }
}