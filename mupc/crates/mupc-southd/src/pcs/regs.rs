//! 两级式 PCS 设备 Modbus 点表与编解码（协议 V1.3，v2.2）
//!
//! PCS = 实时控制模块，MUPC 作 EMS/Modbus Master 直连（南向 `south_pcs` 段；设计 §13）。
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

/// 恒功率通道（4 区 1001/1002）**设备量程**限幅（kW/kVar）。
///
/// **取值来源**：协议 V1.3 的 4 区**写**点表逐行明写取值范围 —— 1001「输出有功功率设置 /
/// 恒功率模式输出设置 / ×1kW / **−100~100**」、1002 无功同区间；而 1006–1011 分相逐行写
/// 「**−25~25**」（即 [`PHASE_LIMIT_KW`]）。故本常量与 `PHASE_LIMIT_KW` **同源同理**：
/// 都是"该寄存器自己那一行的声明量程"，**不是**设备/电池的额定功率（60 kW，其权威源在
/// 策略侧 `p_cap` —— 不在此复制，免生第二份真源）。
///
/// ⚠️ **勿把恒功率通道钳到 ±25**：25 是**单相**量程，恒功率 1001 是**三相总有功**设定。
/// 上游 IEC104 `p_set` 已按额定 `p_cap`（缺省 60）钳过（`startup.rs:270`），拿 ±25 再钳
/// 会把 60 kW 的**合法**指令静默降到 25 kW —— 那是功能回归，不是纵深防护。
pub const CONST_POWER_LIMIT_KW: f64 = 100.0;

/// 3 区 三相展示读数（显示采集，协议 V1.3 / 12-设计文档 §4.1）：
/// 三相输出电流 A/B/C = 1022-1024、三相输出有功 A/B/C = 1029-1031、设备总有功 = 1032。
/// 迁移前这些私有常量在 `intercore::transport::modbus`；将于 Task 11（删除 intercore
/// PCS 面）收敛为单一真源，届时"PCS 有哪些寄存器"只此一处可查（ADR-014，§13.2）。
pub const REG_I_A: u16 = 1022;
pub const REG_P_A: u16 = 1029;
pub const REG_P_TOTAL: u16 = 1032;
/// 三相电流/有功统一量纲（0.1）。
pub const SCALE_3PH: f64 = 0.1;

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

/// 恒功率 P/Q 值 clamp 到 ±[`CONST_POWER_LIMIT_KW`] kW/kVar
/// （与 [`clamp_phase`] 对称：各自钳各自寄存器的声明量程）。
pub fn clamp_const_power(v: f64) -> f64 {
    v.clamp(-CONST_POWER_LIMIT_KW, CONST_POWER_LIMIT_KW)
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
    fn test_clamp_const_power() {
        assert_eq!(clamp_const_power(150.0), 100.0);
        assert_eq!(clamp_const_power(-150.0), -100.0);
        // 两侧都判：只测上限会漏掉"符号侧接反/取下限"的实现。
        assert_eq!(clamp_const_power(100.0), 100.0);
        assert_eq!(clamp_const_power(-100.0), -100.0);
        // **判别力锚**：恒功率量程是 ±100（协议 4 区 1001/1002），**不是**分相的 ±25 ——
        // 额定 60 kW 必须原样通过（误用 clamp_phase 会让本行必红）。
        assert_eq!(
            clamp_const_power(60.0),
            60.0,
            "额定 60kW 不得被单相 ±25 量程误钳"
        );
    }

    #[test]
    fn test_point_constants() {
        assert_eq!(REG_MODE, 1000);
        assert_eq!(REG_PHASE_P_A, 1006);
        assert_eq!(REG_SOC, 1010);
        assert_eq!(REG_RUN_STATE, 1013);
        assert_eq!(MODE_PHASE_SPLIT, 2);
    }

    #[test]
    fn test_three_phase_register_layout_is_self_consistent() {
        // 照 intercore transport 侧 `test_register_layout_three_phase` 的取向：钉**结构性
        // 关系 / 段长自洽**，而非把每个常量再抄一遍自己的字面量。布局（协议 V1.3 / §4.1）：
        // 电流段 1022-1024（3 字）、有功段 1029-1031（3 字）、设备总有功 1032（1 字）；
        // 中间 1025-1028 为点表未命名寄存器 —— 故两段分读、不赌整段读。
        assert_eq!(REG_I_A, 1022, "电流段基址（无法由其它常量导出的协议锚点）");
        assert_eq!(
            REG_P_A,
            REG_I_A + 7,
            "有功段基址：与电流段相距 7（隔 1025-1028 未命名 4 字）"
        );
        assert_eq!(
            REG_P_TOTAL,
            REG_P_A + 3,
            "总有功 = 有功段 3 字之后的第 1 字"
        );
        // 量纲 0.1 是本文件唯一的浮点魔数：钉精确值而非区间 0<x<1 —— 改错它会把三相读数
        // **静默**整体放大/缩小 10 倍（无编译错、无越界、无缺帧，只有读数不对）；区间断言
        // 被精确值断言完全包含，留下只会是冗余噪声，故不写。
        assert_eq!(SCALE_3PH, 0.1);
    }
}
