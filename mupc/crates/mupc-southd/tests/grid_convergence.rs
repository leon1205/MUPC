//! 收敛回归锚：master_meter → meter_grid（§10.9）。
//! 总表回归 = S3a 收敛闸门——mapper meter_grid 输出与 legacy read_master_meter 语义等价。
//! 若此后改动 legacy 语义须同步此锚（Task 4 mapper 单测锚 1 已逐字段等价 legacy）。
//!
//! 与 Task 4 mapper 内部单测的差别：此处经 **pub API**（poll_to_result）端到端构造，
//! 输入经 f32 寄存器编码 → decode 回环，钉住「配置寄存器字节 → DataPackage 字段」全链。
//! 测值全部为 f32 可精确表示的 dyadic 值（0.8/0.9 二进制非终止、f32 加宽后与 f64
//! 字面量差 ~1.19e-8 无法精确相等，故 pf 用 dyadic 值保精确断言，见 §10.9 决议）。

use mupc_data_processing::meter_regs::RegFormat;
use mupc_southd::config::{RegBlockConf, RegFunc, Role};
use mupc_southd::mapper::{poll_to_result, PollResult};

/// f32 → 大端 u16 寄存器对（高字在前；与 decode_regs 字节序一致）
fn f32_regs(v: f32) -> [u16; 2] {
    let b = v.to_bits();
    [(b >> 16) as u16, b as u16]
}

/// 三相 float32 → 6 寄存器
fn phase_regs(a: f32, b: f32, c: f32) -> Vec<u16> {
    [f32_regs(a), f32_regs(b), f32_regs(c)].concat()
}

/// 一块配置 + 该块读成功结果
fn block(name: &str, addr: u16, format: RegFormat, scale: f64, count: u16, regs: Vec<u16>) -> (RegBlockConf, Result<Vec<u16>, String>) {
    (
        RegBlockConf { name: name.into(), addr, func: RegFunc::Holding, format, scale, count },
        Ok(regs),
    )
}

/// 全正相量 canned 输入（读成功后取 Data）
fn unwrap_data(res: PollResult) -> mupc_data_processing::DataPackage {
    match res {
        PollResult::Data(pkg) => pkg,
        PollResult::Failed(e) => panic!("grid 应成功: {e}"),
    }
}

/// 已知输入：p=[1.0,2.0,3.0] q=[0.5,0.25,0.125] pf=[0.75,0.875,0.9375]
///           u=[220,221,222] i=[10,11,12]（kW/V/A 量级，均 f32 精确）
/// p_total 缺失 → 降级 Σp=6.0；电流方向 p>=0 → +幅值。
#[test]
fn meter_grid_phase_matches_legacy_semantics_canned() {
    let reads: Vec<(RegBlockConf, Result<Vec<u16>, String>)> = vec![
        block("p", 0x0000, RegFormat::Float32, 1.0, 6, phase_regs(1.0, 2.0, 3.0)),
        block("q", 0x0006, RegFormat::Float32, 1.0, 6, phase_regs(0.5, 0.25, 0.125)),
        block("pf", 0x000C, RegFormat::Float32, 1.0, 6, phase_regs(0.75, 0.875, 0.9375)),
        block("u", 0x0012, RegFormat::Float32, 1.0, 6, phase_regs(220.0, 221.0, 222.0)),
        block("i", 0x0018, RegFormat::Float32, 1.0, 6, phase_regs(10.0, 11.0, 12.0)),
        // p_total 块故意缺——验证降级 Σp
    ];
    let pkg = unwrap_data(poll_to_result(Role::MeterGrid, &reads));
    let ph = pkg.electrical.phase.expect("meter_grid 须含分相");
    assert_eq!(ph.active_power, [Some(1.0), Some(2.0), Some(3.0)]);
    assert_eq!(ph.voltage, [Some(220.0), Some(221.0), Some(222.0)]);
    assert_eq!(ph.reactive_power, [Some(0.5), Some(0.25), Some(0.125)]);
    assert_eq!(ph.cos_phi, [Some(0.75), Some(0.875), Some(0.9375)]);
    assert_eq!(ph.current, [Some(10.0), Some(11.0), Some(12.0)]); // p≥0 → +幅值
    // 顶层量（与 legacy read_master_meter 逐字段一致）
    assert_eq!(pkg.electrical.active_power, Some(6.0)); // Σp（p_total 缺失降级）
    assert_eq!(pkg.electrical.reactive_power, Some(0.875)); // Σq=0.5+0.25+0.125
    assert_eq!(pkg.electrical.voltage, Some(220.0));
    assert_eq!(pkg.electrical.current, Some(10.0)); // i_mag[0].abs()
    assert_eq!(pkg.electrical.cos_phi, Some(0.75));
    assert_eq!(pkg.electrical.frequency, Some(50.0));
}

/// p_total 块存在 → active_power 用原始块值（非 Σp）
#[test]
fn meter_grid_p_total_raw_when_present() {
    let reads: Vec<(RegBlockConf, Result<Vec<u16>, String>)> = vec![
        block("p", 0x0000, RegFormat::Float32, 1.0, 6, phase_regs(1.0, 2.0, 3.0)),
        block("q", 0x0006, RegFormat::Float32, 1.0, 6, phase_regs(0.5, 0.25, 0.125)),
        block("pf", 0x000C, RegFormat::Float32, 1.0, 6, phase_regs(0.75, 0.875, 0.9375)),
        block("u", 0x0012, RegFormat::Float32, 1.0, 6, phase_regs(220.0, 221.0, 222.0)),
        block("i", 0x0018, RegFormat::Float32, 1.0, 6, phase_regs(10.0, 11.0, 12.0)),
        block("p_total", 0x001E, RegFormat::Float32, 1.0, 2, f32_regs(5.5).to_vec()),
    ];
    let pkg = unwrap_data(poll_to_result(Role::MeterGrid, &reads));
    assert_eq!(pkg.electrical.active_power, Some(5.5));
}

/// 缺相量块（如 q）→ 整周期 Failed（沿用旧数据语义，§10.7）
#[test]
fn meter_grid_missing_phase_block_returns_failed() {
    let reads: Vec<(RegBlockConf, Result<Vec<u16>, String>)> = vec![
        block("p", 0x0000, RegFormat::Float32, 1.0, 6, phase_regs(1.0, 2.0, 3.0)),
        block("u", 0x0012, RegFormat::Float32, 1.0, 6, phase_regs(220.0, 221.0, 222.0)),
        block("i", 0x0018, RegFormat::Float32, 1.0, 6, phase_regs(10.0, 11.0, 12.0)),
        // 缺 q/pf
    ];
    assert!(matches!(poll_to_result(Role::MeterGrid, &reads), PollResult::Failed(_)));
}

/// 负 p → 电流方向取负（带符号电流差模判据；p≈0 相取正——P2-3）
#[test]
fn meter_grid_negative_p_direction_signs_current() {
    let reads: Vec<(RegBlockConf, Result<Vec<u16>, String>)> = vec![
        block("p", 0x0000, RegFormat::Float32, 1.0, 6, phase_regs(-1.0, 2.0, 0.0)),
        block("q", 0x0006, RegFormat::Float32, 1.0, 6, phase_regs(0.5, 0.25, 0.125)),
        block("pf", 0x000C, RegFormat::Float32, 1.0, 6, phase_regs(0.75, 0.875, 0.9375)),
        block("u", 0x0012, RegFormat::Float32, 1.0, 6, phase_regs(220.0, 221.0, 222.0)),
        block("i", 0x0018, RegFormat::Float32, 1.0, 6, phase_regs(10.0, 11.0, 12.0)),
    ];
    let pkg = unwrap_data(poll_to_result(Role::MeterGrid, &reads));
    let ph = pkg.electrical.phase.unwrap();
    assert_eq!(ph.current, [Some(-10.0), Some(11.0), Some(12.0)]); // p=-1→-, p=2→+, p=0→+(P2-3 显式>=)
    assert_eq!(pkg.electrical.active_power, Some(1.0)); // Σp = -1+2+0
}
