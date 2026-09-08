//! 两级式 PCS 设备 Modbus 点表与编解码（协议 V1.3，v2.2）
//!
//! PCS = 实时控制模块，MUPC 作 EMS/Modbus Master 直连（transport=modbus_rtu）。
//! 4 区保持寄存器（FC03 读 / FC06 写单）；3 区输入寄存器（FC04 读）。
//! ⚠️ 高 8 位/低 8 位互换：寄存器 u16 收发须 swap_bytes。

/// 4 区 模块启停（0 停机 / 1 运行）
pub const REG_START_STOP: u16 = 500;
/// 4 区 有功模式
pub const REG_MODE: u16 = 1000;
pub const MODE_CONST_POWER: u16 = 0; // 交流恒功率
pub const MODE_PHASE_SPLIT: u16 = 2; // 交流分相
/// 4 区 恒功率有功/无功设置（Int16 *1kW，正放负充）
pub const REG_CONST_P_SET: u16 = 1001;
pub const REG_CONST_Q_SET: u16 = 1002;
/// 4 区 单 A/B/C 有功（1006-1008）与无功（1009-1011），Int16 *1kW，单相 ±25
pub const REG_PHASE_P_A: u16 = 1006;
pub const REG_PHASE_Q_A: u16 = 1009;
/// 3 区 BMS 系统 SOC（*1%）
pub const REG_SOC: u16 = 1010;
/// 3 区 模块运行状态（0 停机 / 1 待机 / 2 充电 / 3 放电）
pub const REG_RUN_STATE: u16 = 1013;
/// 单相功率限幅（kW）
pub const PHASE_LIMIT_KW: f64 = 25.0;

/// PCS 端 int16 值 → u16（含字节互换：高 8/低 8 位互换）
pub fn to_pcs_reg(v: f64) -> u16 {
    (v.round() as i16 as u16).swap_bytes()
}

/// PCS 端 u16 → f64（含字节互换回解）
pub fn from_pcs_reg(r: u16) -> f64 {
    (r.swap_bytes() as i16) as f64
}

/// 分相 P/Q 值 clamp 到 ±25 kW/kVar
pub fn clamp_phase(v: f64) -> f64 {
    v.clamp(-PHASE_LIMIT_KW, PHASE_LIMIT_KW)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_swap() {
        assert_eq!(to_pcs_reg(4660.0).swap_bytes() as i16, 4660); // 4660=0x1234
    }

    #[test]
    fn test_int16_roundtrip() {
        for v in [-25.0, -1.0, 0.0, 23.5, 25.0] {
            let r = to_pcs_reg(v);
            assert!((from_pcs_reg(r) - v).abs() < 1.0);
        }
    }

    #[test]
    fn test_clamp_phase() {
        assert_eq!(clamp_phase(30.0), 25.0);
        assert_eq!(clamp_phase(-30.0), -25.0);
        assert_eq!(clamp_phase(10.0), 10.0);
    }

    #[test]
    fn test_point_constants() {
        assert_eq!(REG_MODE, 1000);
        assert_eq!(REG_PHASE_P_A, 1006);
        assert_eq!(REG_SOC, 1010);
        assert_eq!(REG_RUN_STATE, 1013);
        assert_eq!(MODE_PHASE_SPLIT, 2);
    }
}
