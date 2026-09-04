//! 台区总表 Modbus 寄存器解码
//!
//! 台区总表（配变计量）以 RS485 Modbus 保持寄存器暴露分相电气量。多寄存器数值
//! 按 Modbus 惯例**高字在前（大端）**。本模块提供纯函数解码，供采集装配层
//! （mupc-core-bin）把总表寄存器快照转成 `PhaseElectricalData`（U-26）。

use serde::Deserialize;

/// 寄存器数值格式（YAML 序列化为字符串：`float32` / `int32_scaled`）
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegFormat {
    /// IEEE754 f32，占 2 寄存器，大端
    Float32,
    /// 有符号 i32 × scale，占 2 寄存器，大端
    Int32Scaled,
}

/// 2 个 u16 寄存器（高字在前）→ u32 位模式
pub fn regs_to_u32_be(r: &[u16]) -> u32 {
    ((r[0] as u32) << 16) | (r[1] as u32)
}

/// 2 个 u16 寄存器（高字在前）→ f32
pub fn regs_to_f32_be(r: &[u16]) -> f32 {
    f32::from_bits(regs_to_u32_be(r))
}

/// 2 个 u16 寄存器（高字在前）→ i32
pub fn regs_to_i32_be(r: &[u16]) -> i32 {
    ((r[0] as i32) << 16) | (r[1] as i32)
}

/// 按格式解码一段寄存器（长度不足时返回 0.0）
pub fn decode_regs(r: &[u16], format: RegFormat, scale: f64) -> f64 {
    if r.len() < 2 {
        return 0.0;
    }
    match format {
        RegFormat::Float32 => regs_to_f32_be(r) as f64,
        RegFormat::Int32Scaled => regs_to_i32_be(r) as f64 * scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16s(hi: u16, lo: u16) -> [u16; 2] {
        [hi, lo]
    }

    #[test]
    fn test_u32_be() {
        assert_eq!(regs_to_u32_be(&u16s(0x1234, 0x5678)), 0x1234_5678);
    }

    #[test]
    fn test_f32_be() {
        // 50.0f32 = 0x42480000 → 高字 0x4248 低字 0x0000
        let f = regs_to_f32_be(&u16s(0x4248, 0x0000));
        assert!((f - 50.0).abs() < 1e-3);
    }

    #[test]
    fn test_i32_scaled() {
        // -12.5 kW, scale 0.01 → raw -1250 = 0xFFFFFB1E → 高字 0xFFFF 低字 0xFB1E
        let v = decode_regs(&u16s(0xFFFF, 0xFB1E), RegFormat::Int32Scaled, 0.01);
        assert!((v - (-12.5)).abs() < 1e-9);
    }

    #[test]
    fn test_short_regs_returns_zero() {
        assert_eq!(decode_regs(&[0x4248], RegFormat::Float32, 1.0), 0.0);
        assert_eq!(decode_regs(&[], RegFormat::Int32Scaled, 1.0), 0.0);
    }

    #[test]
    fn test_int32_roundtrip() {
        for v in [-60000i32, -1, 0, 1, 32767] {
            let hi = ((v >> 16) & 0xFFFF) as u16;
            let lo = (v & 0xFFFF) as u16;
            assert_eq!(regs_to_i32_be(&u16s(hi, lo)), v);
        }
    }
}
