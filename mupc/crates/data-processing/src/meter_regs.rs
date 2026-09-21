//! 台区总表 Modbus 寄存器解码
//!
//! 台区总表（配变计量）以 RS485 Modbus 保持寄存器暴露分相电气量。多寄存器数值
//! 按 Modbus 惯例**高字在前（大端）**。本模块提供纯函数解码，供采集装配层
//! （mupc-core-bin）把总表寄存器快照转成 `PhaseElectricalData`（U-26）。

use serde::{Deserialize, Serialize};

/// 寄存器数值格式（YAML 序列化为字符串：`float32` / `int32_scaled`）
///
/// `Serialize` 为 12-本地显示终端 §4.3.2.1 的配置整体回写（回退路径）所需：本类型是
/// `south_stations.stations[].regs[].format` 的承载，而 `CoreConfig: Serialize` 要求
/// 全链可序列化（该回退路径与往返单测是唯一的消费方，不影响解析语义）。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegFormat {
    /// IEEE754 f32，占 2 寄存器，大端
    Float32,
    /// 有符号 i32 × scale，占 2 寄存器，大端
    Int32Scaled,
    /// 无符号 u16，占 1 寄存器（S3b-2 / PRD §9.1.1 G-1）
    Uint16,
    /// 有符号 i16，占 1 寄存器（S3b-2 / PRD §9.1.1 G-1）
    Int16,
}

impl RegFormat {
    /// 单值占用的寄存器数（16 位 = 1，32 位 = 2）—— **"宽度"的唯一定义**。
    ///
    /// PRD 的"32 位点对齐/占 2 寄存器""无 `points` 的块按宽度步进产点"等规则
    /// 一律引用本函数，不再散落魔数（设计 §11.2.2）。
    pub fn reg_width(self) -> usize {
        match self {
            RegFormat::Uint16 | RegFormat::Int16 => 1,
            RegFormat::Float32 | RegFormat::Int32Scaled => 2,
        }
    }
}

/// 32 位值的字序 —— **全项目唯一一处定义**（设计 §11.4.2）。
///
/// - `HiLo`（缺省）：高字在低地址，与既有 `regs_to_u32_be` 逐位等价
/// - `LoHi`：低字在低地址（PCS 32 位电量，PRD §9.7.3）
///
/// serde 形态与同文件的 `RegFormat` 同构（`rename_all = "snake_case"` + 全链
/// `Serialize` 供配置回写）；`mupc-southd::config` 以 `pub use` 复用本类型，
/// **不得**在别处再定义一份（v1.2 在 `config`/`meter_regs` 各定义一次 = 被否掉的 B3 缺陷）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WordOrder {
    /// 高字在低地址（缺省；既有 `regs_to_u32_be` 语义）
    #[default]
    HiLo,
    /// 低字在低地址（PCS 32 位电量）
    LoHi,
}

/// 解码规格：格式 + 换算 + 字节序/字序（"通用解码原语"的载体，设计 §11.2.2 选型 B1）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegDecode {
    /// 数值格式（决定宽度与位模式解释）
    pub format: RegFormat,
    /// 比例系数；`Float32` 忽略（沿用既有语义：float32 不乘 scale）
    pub scale: f64,
    /// 零点平移；`Float32` 忽略（浮点原值无零点平移概念）
    pub offset: f64,
    /// 字序；仅 32 位格式生效
    pub word_order: WordOrder,
    /// 字节序还原开关（逐寄存器 `u16::swap_bytes()`）；16/32 位格式均生效
    pub byte_swap: bool,
}

impl RegDecode {
    /// 本次解码需要的寄存器数（= `format.reg_width()`）
    pub fn width(&self) -> usize {
        self.format.reg_width()
    }

    /// 解码 1 个值。
    ///
    /// 算法（唯一、有序，设计 §11.4.2）：
    /// 1. 长度守卫：`regs.len() < width()` → `0.0`
    /// 2. **字节序还原**：对参与本次解码的每个寄存器做 `swap_bytes()`（`byte_swap` 时）
    /// 3. 位模式组装：宽度 1 直取；宽度 2 按 `word_order` 拼 u32
    /// 4. 物理解算：`Float32` → `f32`；其余 → `raw as f64 * scale + offset`
    /// 5. 非有限守卫 → `0.0`（沿用既有 U-26 审查 P2-3 语义）
    ///
    /// ⚠️ **第 2 步在第 3 步之前**（"先逐寄存器 swap，再按字序拼"）不是风格选择：
    /// 该顺序被 AC-2 的 PCS 32 位电量算例唯一钉死（`6563.6 kWh`，见 PRD §9.9.1 / 设计 §11.6）。
    pub fn decode(&self, regs: &[u16]) -> f64 {
        let width = self.width();
        if regs.len() < width {
            return 0.0;
        }

        // 2) 字节序还原（先 swap，后拼字）
        let mut buf = [0u16; 2];
        for (i, slot) in buf.iter_mut().take(width).enumerate() {
            *slot = if self.byte_swap {
                regs[i].swap_bytes()
            } else {
                regs[i]
            };
        }

        let v = match self.format {
            // 3) 位模式组装
            RegFormat::Float32 | RegFormat::Int32Scaled => {
                let u32_bits = match self.word_order {
                    // HiLo：与既有 regs_to_u32_be 逐位等价
                    WordOrder::HiLo => ((buf[0] as u32) << 16) | (buf[1] as u32),
                    WordOrder::LoHi => ((buf[1] as u32) << 16) | (buf[0] as u32),
                };
                // 4) 物理解算
                match self.format {
                    RegFormat::Float32 => f32::from_bits(u32_bits) as f64,
                    _ => (u32_bits as i32) as f64 * self.scale + self.offset,
                }
            }
            RegFormat::Uint16 => buf[0] as f64 * self.scale + self.offset,
            RegFormat::Int16 => (buf[0] as i16) as f64 * self.scale + self.offset,
        };

        // 5) 非有限守卫——表计坏值（NaN/Inf）按 0 处理，防 NaN 进入控制链
        if v.is_finite() {
            v
        } else {
            0.0
        }
    }
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

/// 按格式解码一段寄存器（长度不足返回 0.0；非有限值——NaN/±Inf——返回 0.0）。
///
/// **薄包装**（设计 §11.2.2 选型 B1）：语义 = `RegDecode { 无 swap、`HiLo`、`offset 0` }`，
/// 既有调用点（`meter_grid` 等）**零改动、行为不变**。
pub fn decode_regs(r: &[u16], format: RegFormat, scale: f64) -> f64 {
    RegDecode {
        format,
        scale,
        offset: 0.0,
        word_order: WordOrder::HiLo,
        byte_swap: false,
    }
    .decode(r)
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
    fn test_non_finite_returns_zero() {
        // U-26 审查 P2-3: NaN（0x7FC00000）与 ±Inf（0x7F800000 / 0xFF800000）坏值 → 0.0
        assert_eq!(
            decode_regs(&u16s(0x7FC0, 0x0000), RegFormat::Float32, 1.0),
            0.0
        );
        assert_eq!(
            decode_regs(&u16s(0x7F80, 0x0000), RegFormat::Float32, 1.0),
            0.0
        );
        assert_eq!(
            decode_regs(&u16s(0xFF80, 0x0000), RegFormat::Float32, 1.0),
            0.0
        );
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

/// S3b-2（PRD §9.9.1 **AC-2** / 设计 §11.6）解码正确性 L1 向量。
///
/// 全部期望值取自 PRD §9.9.1 AC-2 与设计 §11.6 的**回原文复算表**，逐条实现；
/// 上方 `tests` 模块的 6 个既有用例**一字未改**（回归锚）。
#[cfg(test)]
mod ac2_vectors {
    use super::*;

    /// 期望值比对（复算值均为有限十进制小数，1e-9 远大于 f64 舍入误差）
    fn approx(v: f64, want: f64) -> bool {
        (v - want).abs() < 1e-9
    }

    /// 16 位无符号：格式 + 换算（无 swap、无字序概念）
    fn u16(fmt: RegFormat, scale: f64, offset: f64) -> RegDecode {
        RegDecode {
            format: fmt,
            scale,
            offset,
            word_order: WordOrder::HiLo,
            byte_swap: false,
        }
    }

    // ── 宽度：唯一定义（设计 §11.2.2 单点定义） ─────────────────────────

    #[test]
    fn ac2_reg_width_single_source() {
        assert_eq!(RegFormat::Uint16.reg_width(), 1);
        assert_eq!(RegFormat::Int16.reg_width(), 1);
        assert_eq!(RegFormat::Float32.reg_width(), 2);
        assert_eq!(RegFormat::Int32Scaled.reg_width(), 2);
        // RegDecode::width() 委托同一处定义
        assert_eq!(u16(RegFormat::Uint16, 1.0, 0.0).width(), 1);
        assert_eq!(u16(RegFormat::Int32Scaled, 1.0, 0.0).width(), 2);
    }

    // ── BMS ────────────────────────────────────────────────────────────

    #[test]
    fn ac2_bms_soc_118_uint16() {
        // raw 0x0041(65) → 65.0 %
        assert!(approx(
            u16(RegFormat::Uint16, 1.0, 0.0).decode(&[0x0041]),
            65.0
        ));
    }

    #[test]
    fn ac2_bms_cluster_voltage_115_uint16_scaled() {
        // raw 0x1403(5123) → 512.3 V
        assert!(approx(
            u16(RegFormat::Uint16, 0.1, 0.0).decode(&[0x1403]),
            512.3
        ));
    }

    #[test]
    fn ac2_bms_cluster_module_temp_117_uint16_offset_neg40() {
        // raw 65 → 65×1.0 + (−40) → 25.0 ℃（v1.2 的 raw=650 是错的）
        assert!(approx(
            u16(RegFormat::Uint16, 1.0, -40.0).decode(&[65]),
            25.0
        ));
    }

    #[test]
    fn ac2_bms_terminal_temp_2991_same_encoding() {
        // bms_term_1（2991）raw=65 → 25.0 ℃（与 117 同编码）
        assert!(approx(
            u16(RegFormat::Uint16, 1.0, -40.0).decode(&[65]),
            25.0
        ));
    }

    #[test]
    fn ac2_bms_116_cluster_current_uint16_zero_shift() {
        // PRD §9.7.5 / Q-20: uint16 + 0.1 + offset −1600.0，满量程 [−1600.0, +4953.5] A
        let d = u16(RegFormat::Uint16, 0.1, -1600.0);
        assert!(approx(d.decode(&[16000]), 0.0), "raw 16000 → 零点 0.0 A");
        assert!(approx(d.decode(&[15000]), -100.0), "raw 15000 → −100.0 A");
        assert!(
            approx(d.decode(&[0]), -1600.0),
            "raw 0 → 量程下限 −1600.0 A"
        );
        assert!(
            approx(d.decode(&[65535]), 4953.5),
            "raw 65535 → +4953.5 A（判别点）"
        );
    }

    #[test]
    fn ac2_bms_116_discriminates_uint16_vs_int16() {
        // AC-2 的判别用例：同一 raw=65535，两种解释**必须不同**且各自可钉死
        let as_u16 = u16(RegFormat::Uint16, 0.1, -1600.0).decode(&[65535]);
        let as_i16 = u16(RegFormat::Int16, 0.1, -1600.0).decode(&[65535]);
        assert!(approx(as_u16, 4953.5), "uint16 解 +4953.5 A，实得 {as_u16}");
        assert!(approx(as_i16, -1600.1), "int16 解 −1600.1 A，实得 {as_i16}");
        assert_ne!(as_u16, as_i16, "两种解释必须分道扬镳");
    }

    // ── PCS（byte_swap / word_order） ───────────────────────────────────

    #[test]
    fn ac2_pcs_soc_1010_byte_swap() {
        // 注入 0x4100 → swap(0x4100)=0x0041 → 65.0 %
        let d = RegDecode {
            format: RegFormat::Uint16,
            scale: 1.0,
            offset: 0.0,
            word_order: WordOrder::HiLo,
            byte_swap: true,
        };
        assert!(approx(d.decode(&[0x4100]), 65.0));
    }

    #[test]
    fn ac2_pcs_accumulated_charge_energy_lo_hi_with_byte_swap() {
        // 注入 [0x6400, 0x0100]：先逐寄存器 swap → [0x0064, 0x0001]，再按 lo_hi 拼
        // → 0x00010064 = 65636 → ×0.1 = 6563.6 kWh
        let lo_hi = RegDecode {
            format: RegFormat::Int32Scaled,
            scale: 0.1,
            offset: 0.0,
            word_order: WordOrder::LoHi,
            byte_swap: true,
        };
        let v = lo_hi.decode(&[0x6400, 0x0100]);
        assert!(approx(v, 6563.6), "lo_hi + swap 应得 6563.6 kWh，实得 {v}");

        // 误用 hi_lo 得 655360.1 —— 必须不一致（钉死"先 swap 再拼字"的顺序语义）
        let misused = RegDecode {
            word_order: WordOrder::HiLo,
            ..lo_hi
        }
        .decode(&[0x6400, 0x0100]);
        assert!(
            approx(misused, 655360.1),
            "误用 hi_lo 应得 655360.1，实得 {misused}"
        );
        assert_ne!(v, misused);
    }

    // ── ADL400 ─────────────────────────────────────────────────────────

    #[test]
    fn ac2_adl400_phase_a_current_0x0064() {
        // raw 946 → 9.46 A
        assert!(approx(
            u16(RegFormat::Uint16, 0.01, 0.0).decode(&[946]),
            9.46
        ));
    }

    #[test]
    fn ac2_adl400_total_active_energy_0x0000() {
        // [0x0000, 0x3026] → hi_lo → 12326 × 0.01 = 123.26 kWh
        assert!(approx(
            u16(RegFormat::Int32Scaled, 0.01, 0.0).decode(&[0x0000, 0x3026]),
            123.26
        ));
    }

    #[test]
    fn ac2_adl400_total_reactive_power_0x0172() {
        // [0x0000, 0x3B6C] → 15212 × 0.001 = 15.212 kvar
        assert!(approx(
            u16(RegFormat::Int32Scaled, 0.001, 0.0).decode(&[0x0000, 0x3B6C]),
            15.212
        ));
    }

    // ── 消防 ───────────────────────────────────────────────────────────

    #[test]
    fn ac2_fire_alarm_state_9_enum_value() {
        // 火警状态 = 2 → 二级火警（枚举语义，值即 2.0）
        assert!(approx(u16(RegFormat::Uint16, 1.0, 0.0).decode(&[2]), 2.0));
    }

    #[test]
    fn ac2_fire_detector_data1_13_whole_word() {
        // G-6 延后：整字原值落 telemetry（0x4150 = 16720），拆解在展示层
        assert!(approx(
            u16(RegFormat::Uint16, 1.0, 0.0).decode(&[0x4150]),
            16720.0
        ));
    }

    // ── 空调 ───────────────────────────────────────────────────────────

    #[test]
    fn ac2_hvac_30001_int16_and_30004_uint16() {
        // 30001 有符号 raw 258 → 25.8 ℃；30004 无符号 raw 602 → 60.2 %
        assert!(approx(u16(RegFormat::Int16, 0.1, 0.0).decode(&[258]), 25.8));
        assert!(approx(
            u16(RegFormat::Uint16, 0.1, 0.0).decode(&[602]),
            60.2
        ));
    }

    // ── 负值 / 守卫 ────────────────────────────────────────────────────

    #[test]
    fn ac2_int16_negative_raw() {
        // int16 补码：0xFFFF = −1 → ×0.1 = −0.1
        assert!(approx(
            u16(RegFormat::Int16, 0.1, 0.0).decode(&[0xFFFF]),
            -0.1
        ));
    }

    #[test]
    fn ac2_short_regs_returns_zero_for_16bit() {
        // 长度守卫按 width()：16 位格式空切片 → 0.0；32 位格式 1 寄存器 → 0.0
        assert_eq!(u16(RegFormat::Uint16, 1.0, 0.0).decode(&[]), 0.0);
        assert_eq!(u16(RegFormat::Int16, 1.0, 0.0).decode(&[]), 0.0);
        assert_eq!(u16(RegFormat::Int32Scaled, 1.0, 0.0).decode(&[0x0001]), 0.0);
    }

    // ── 薄包装等价性（设计 §11.2.2 B1 的唯一代价，须靠测试钉住） ────────

    #[test]
    fn ac2_decode_regs_wrapper_is_equivalent() {
        let cases: &[(&[u16], RegFormat, f64)] = &[
            (&[0x4248, 0x0000], RegFormat::Float32, 1.0),
            (&[0xFFFF, 0xFB1E], RegFormat::Int32Scaled, 0.01),
            (&[0x0000, 0x3026], RegFormat::Int32Scaled, 0.01),
            (&[0x0041], RegFormat::Uint16, 1.0),
            (&[0x1403], RegFormat::Uint16, 0.1),
            (&[0xFFFE], RegFormat::Int16, 1.0),
        ];
        for (regs, fmt, scale) in cases {
            let wrapper = decode_regs(regs, *fmt, *scale);
            let spec = RegDecode {
                format: *fmt,
                scale: *scale,
                offset: 0.0,
                word_order: WordOrder::HiLo,
                byte_swap: false,
            }
            .decode(regs);
            assert_eq!(wrapper, spec, "薄包装须与显式规格逐位等价");
        }
    }

    // ── 非有限守卫对 16 位同样生效 ──────────────────────────────────────

    #[test]
    fn ac2_non_finite_guard_applies_to_16bit() {
        // scale 为 NaN 时结果非有限 ⇒ 0.0（沿用既有 U-26 守卫）
        assert_eq!(u16(RegFormat::Uint16, f64::NAN, 0.0).decode(&[0x0041]), 0.0);
        assert_eq!(u16(RegFormat::Int16, f64::INFINITY, 0.0).decode(&[1]), 0.0);
    }
}
