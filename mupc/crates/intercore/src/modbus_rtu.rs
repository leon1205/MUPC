//! 核间 Modbus RTU 寄存器映射与编解码
//!
//! 实时控制模块（Slave）暴露保持寄存器区，管理模块（Master）写控制区（FC16）、
//! 读执行确认区与状态区（FC03）。编码统一 int32 缩放（ADR-010）。

/// 控制区寄存器地址
pub const REG_CMD_CTRL: u16 = 0x0000;
pub const REG_PROTOCOL_VERSION: u16 = 0x0001;
pub const REG_P_REF: u16 = 0x0010;
pub const REG_K_DROOP: u16 = 0x0012;
pub const REG_PHASE_P_A: u16 = 0x0020;
pub const REG_PHASE_Q_A: u16 = 0x0026;
/// 执行确认区（从站写、master 读）
pub const REG_EXEC_SEQ: u16 = 0x0030;
pub const REG_EXEC_STATUS: u16 = 0x0032;
pub const REG_EXEC_ERROR: u16 = 0x0033;
/// 状态/心跳区
pub const REG_HEARTBEAT: u16 = 0x0100;
pub const REG_DEVICE_STATUS: u16 = 0x0101;
pub const REG_CPU_TEMP: u16 = 0x0102;
pub const REG_MEM_USAGE: u16 = 0x0104;

/// 协议版本
pub const PROTOCOL_VERSION: u16 = 1;

/// 缩放因子
pub const SCALE_POWER: f64 = 0.01;
pub const SCALE_K_DROOP: f64 = 0.001;

/// exec_status 取值
pub const EXEC_IDLE: u16 = 0;
pub const EXEC_RUNNING: u16 = 1;
pub const EXEC_SUCCESS: u16 = 2;
pub const EXEC_FAILED: u16 = 3;
pub const EXEC_TIMEOUT: u16 = 4;

/// f64 → 按 scale 缩放的 i32
pub fn encode_scaled(v: f64, scale: f64) -> i32 {
    (v / scale).round() as i32
}

/// i32（按 scale 缩放）→ f64
pub fn decode_scaled(raw: i32, scale: f64) -> f64 {
    raw as f64 * scale
}

/// i32 → 2 个大端 u16 寄存器（[高16, 低16]）
pub fn i32_to_regs(v: i32) -> [u16; 2] {
    [((v >> 16) & 0xFFFF) as u16, (v & 0xFFFF) as u16]
}

/// 2 个大端 u16 寄存器 → i32
pub fn regs_to_i32(regs: &[u16]) -> i32 {
    ((regs[0] as i32) << 16) | (regs[1] as i32)
}

/// f64 功率 → 2 寄存器（0.01 缩放）
pub fn power_to_regs(v: f64) -> [u16; 2] {
    i32_to_regs(encode_scaled(v, SCALE_POWER))
}

/// 2 寄存器 → f64 功率
pub fn regs_to_power(regs: &[u16]) -> f64 {
    decode_scaled(regs_to_i32(regs), SCALE_POWER)
}

/// 打包 cmd_ctrl：低字节 bit0 cmd_valid / bit1-3 strategy_mode / bit4 ai_ready；高字节 cmd_seq
pub fn pack_cmd_ctrl(cmd_seq: u8, strategy_mode: u8, ai_ready: bool, cmd_valid: bool) -> u16 {
    let mut low: u16 = 0;
    if cmd_valid {
        low |= 0x0001;
    }
    low |= ((strategy_mode as u16) & 0x07) << 1;
    if ai_ready {
        low |= 0x0010;
    }
    ((cmd_seq as u16) << 8) | low
}

pub fn cmd_seq_of(reg: u16) -> u8 {
    ((reg >> 8) & 0xFF) as u8
}

pub fn strategy_mode_of(reg: u16) -> u8 {
    ((reg >> 1) & 0x07) as u8
}

pub fn ai_ready_of(reg: u16) -> bool {
    reg & 0x0010 != 0
}

pub fn cmd_valid_of(reg: u16) -> bool {
    reg & 0x0001 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i32_regs_roundtrip() {
        for v in [-60000i32, -1, 0, 1, 32767, 60000] {
            let regs = i32_to_regs(v);
            assert_eq!(regs_to_i32(&regs), v, "i32 roundtrip {}", v);
        }
    }

    #[test]
    fn test_scale_encode_decode() {
        let raw = encode_scaled(-50.0, SCALE_POWER);
        assert_eq!(raw, -5000);
        assert!((decode_scaled(raw, SCALE_POWER) - (-50.0)).abs() < 1e-9);
        assert_eq!(encode_scaled(0.5, SCALE_K_DROOP), 500);
    }

    #[test]
    fn test_power_regs_roundtrip() {
        for v in [-50.0f64, -2.0, 0.0, 23.45, 60.0] {
            let regs = power_to_regs(v);
            assert!((regs_to_power(&regs) - v).abs() < 0.005);
        }
    }

    #[test]
    fn test_cmd_ctrl_pack_unpack() {
        let reg = pack_cmd_ctrl(7, 2, false, true);
        assert_eq!(cmd_seq_of(reg), 7);
        assert_eq!(strategy_mode_of(reg), 2);
        assert!(!ai_ready_of(reg));
        assert!(cmd_valid_of(reg));
        assert_eq!(reg & 0x0001, 0x0001);
        assert_eq!(reg & 0xFF00, 7 << 8);
    }
}
