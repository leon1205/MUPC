//! role → DataPackage 组装（S3a Task 4；§10.5 mapper）。
//!
//! 读操作由 scheduler 经 StationBus 完成，本模块为**纯转换**（无 IO，可单测）：
//! 输入为「每站一次 poll 的原始寄存器读结果」（`BlockReads`），输出为 `PollResult`。
//! role 语义：`meter_grid` 完整移植 core-bin startup.rs `read_master_meter`
//! （196-261）的分相语义（同 canned 寄存器 → 同 DataPackage.phase，回归等价）；
//! `battery` 若含 soc 块填 `battery.soc`；其它 role 无语义点表（厂方待 S3b）→
//! 返回最小 DataPackage 作"站活着"信号，真实遥测值由 scheduler 经
//! [`telemetry_points`] 直接落库（不须经 DataPackage）。

use crate::config::{RegBlockConf, Role};
use mupc_data_processing::meter_regs::decode_regs;
use mupc_data_processing::telemetry::PhaseElectricalData;
use mupc_data_processing::{
    BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
};

/// 每站一次 poll 的原始读结果：每个 regs 块一个条目（该块读失败为 Err）。
///
/// scheduler 读回原始 u16 寄存器后交 mapper 内部 decode，使 meter_grid 的
/// 「某块读失败/长度不足 → 整周期失败」语义能在 mapper 内表达。
pub type BlockReads = Vec<(RegBlockConf, Result<Vec<u16>, String>)>;

/// 站一次 poll 的组装结果（scheduler 分发用）。
// DataPackage 体量远大于 Failed(String)；scheduler 以引用持有结果，未装箱保持
// 规格声明的接口形状（每站轮询 1s 量级，体量可忽略），故允许 large_enum_variant。
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum PollResult {
    /// 成功组包（grid=完整分相；battery 有 soc 填 battery.soc）。
    Data(DataPackage),
    /// 该站本轮失败（关键块读失败/解码不足）——沿用旧数据，站 offline 计数。
    Failed(String),
}

/// 解码相量块：6 寄存器（3 相×2reg，高字在前）→ `[f64; 3]`；不足 6 返回 None（该块失败）。
///
/// 与 startup.rs `read_meter_phases` 逐寄存器对 decode 等价：多余寄存器忽略
/// （取前 6），不足即整块失败。
fn decode_phase_block(regs: &[u16], b: &RegBlockConf) -> Option<[f64; 3]> {
    if regs.len() < 6 {
        return None;
    }
    Some([
        decode_regs(&regs[0..2], b.format, b.scale),
        decode_regs(&regs[2..4], b.format, b.scale),
        decode_regs(&regs[4..6], b.format, b.scale),
    ])
}

/// 从块读结果中取某 name 的相量块；缺块或读失败 → None（整周期失败）。
fn phase_block(reads: &BlockReads, name: &str) -> Option<[f64; 3]> {
    let (b, res) = reads.iter().find(|(b, _)| b.name == name)?;
    match res {
        Ok(r) => decode_phase_block(r, b),
        Err(_) => None,
    }
}

/// 取 `p_total` 标量块（2 寄存器）：缺块/读失败 → None（调用方降级分相和）。
fn scalar_total(reads: &BlockReads) -> Option<f64> {
    let (b, res) = reads.iter().find(|(b, _)| b.name == "p_total")?;
    match res {
        Ok(r) if r.len() >= 2 => Some(decode_regs(&r[..2], b.format, b.scale)),
        _ => None,
    }
}

/// 最小 DataPackage（非 grid/battery 站"活着"信号；electrical 缺省 + battery 空）。
fn empty_package() -> DataPackage {
    DataPackage {
        timestamp: chrono::Utc::now().timestamp() as u64,
        electrical: ElectricalData::default(),
        battery: BatteryData {
            soc: None,
            soh: None,
            temperature: None,
        },
        device_status: DeviceStatus {
            inverter_status: InverterStatus::Running,
            pv_power: None,
            load_power: None,
            ev_charger_power: None,
        },
    }
}

/// meter_grid 分相组包（与 startup.rs `read_master_meter` 逐字段等价）。
///
/// `p_total_raw` 为 `p_total` 独立块解码值（Option）：None 时降级为分相有功和
/// （best-effort，不整周期失败）。电流方向由分相有功符号承载：p>=0 相为正幅值，
/// p<0 相为负幅值（P2-3：p≈0 取正，显式 >= 而非 signum）。
///
/// 独立私有 fn，便于回归锚直接测（同 canned 输入 → 同输出）。
fn build_grid_package(
    p: [f64; 3],
    q: [f64; 3],
    pf: [f64; 3],
    u: [f64; 3],
    i_mag: [f64; 3],
    p_total_raw: Option<f64>,
) -> DataPackage {
    let p_total = p_total_raw.unwrap_or_else(|| p.iter().sum());
    let i = [
        if p[0] >= 0.0 {
            i_mag[0].abs()
        } else {
            -i_mag[0].abs()
        },
        if p[1] >= 0.0 {
            i_mag[1].abs()
        } else {
            -i_mag[1].abs()
        },
        if p[2] >= 0.0 {
            i_mag[2].abs()
        } else {
            -i_mag[2].abs()
        },
    ];
    let phase = PhaseElectricalData {
        voltage: [Some(u[0]), Some(u[1]), Some(u[2])],
        current: [Some(i[0]), Some(i[1]), Some(i[2])],
        active_power: [Some(p[0]), Some(p[1]), Some(p[2])],
        reactive_power: [Some(q[0]), Some(q[1]), Some(q[2])],
        cos_phi: [Some(pf[0]), Some(pf[1]), Some(pf[2])],
    };
    DataPackage {
        timestamp: chrono::Utc::now().timestamp() as u64,
        electrical: ElectricalData {
            voltage: Some(u[0]),
            current: Some(i_mag[0].abs()),
            active_power: Some(p_total),
            reactive_power: Some(q.iter().sum()),
            cos_phi: Some(pf[0]),
            frequency: Some(50.0),
            phase: Some(phase),
        },
        battery: BatteryData {
            soc: None,
            soh: None,
            temperature: None,
        },
        device_status: DeviceStatus {
            inverter_status: InverterStatus::Running,
            pv_power: None,
            load_power: None,
            ev_charger_power: None,
        },
    }
}

/// 站一次 poll 的结果组装。role 语义：
/// - `MeterGrid`：p/q/pf/u/i 五相量块缺任一或读失败 → Failed（沿用旧数据）；p_total
///   独立块缺省/失败 → 降级分相和，不整周期失败。
/// - `Battery`：若含 soc 块且读失败 → Failed；读成功 → 填 battery.soc；无 soc 块
///   或长度不足 → battery 空（占位，不 Failed）。
/// - 其它 role（MeterBatt/Hvac/Fire）：无语义点表 → 最小 DataPackage（"站活着"信号）。
pub fn poll_to_result(role: Role, reads: &BlockReads) -> PollResult {
    match role {
        Role::MeterGrid => {
            let p = match phase_block(reads, "p") {
                Some(v) => v,
                None => return PollResult::Failed("meter_grid p 块缺失或读失败".into()),
            };
            let q = match phase_block(reads, "q") {
                Some(v) => v,
                None => return PollResult::Failed("meter_grid q 块缺失或读失败".into()),
            };
            let pf = match phase_block(reads, "pf") {
                Some(v) => v,
                None => return PollResult::Failed("meter_grid pf 块缺失或读失败".into()),
            };
            let u = match phase_block(reads, "u") {
                Some(v) => v,
                None => return PollResult::Failed("meter_grid u 块缺失或读失败".into()),
            };
            let i = match phase_block(reads, "i") {
                Some(v) => v,
                None => return PollResult::Failed("meter_grid i 块缺失或读失败".into()),
            };
            let p_total_raw = scalar_total(reads);
            PollResult::Data(build_grid_package(p, q, pf, u, i, p_total_raw))
        }
        Role::Battery => {
            let mut pkg = empty_package();
            match reads.iter().find(|(b, _)| b.name == "soc") {
                None => {} // 无 soc 块：battery 空占位
                Some((b, res)) => match res {
                    Ok(r) if r.len() >= 2 => {
                        pkg.battery.soc = Some(decode_regs(&r[..2], b.format, b.scale))
                    }
                    Ok(_) => {} // soc 块长度不足：保持 None（best-effort，不 Failed）
                    Err(e) => return PollResult::Failed(format!("battery soc 块读失败: {e}")),
                },
            }
            PollResult::Data(pkg)
        }
        Role::MeterBatt | Role::Hvac | Role::Fire => PollResult::Data(empty_package()),
    }
}

/// 遥测点提取：每块读成功取首值（前 2 寄存器 decode 一个标量），metric 名 = 块名。
///
/// 相量块（float32 三相）也取前 2 寄存器单值——对 S3b 语义点表补全前的占位 role
/// 够 scheduler 直接落库（不须经 DataPackage）。块读失败/长度不足 → 跳过。
pub fn telemetry_points(reads: &BlockReads) -> Vec<(String, f64)> {
    reads
        .iter()
        .filter_map(|(b, res)| match res {
            Ok(r) if r.len() >= 2 => {
                Some((b.name.clone(), decode_regs(&r[..2], b.format, b.scale)))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_data_processing::meter_regs::RegFormat;

    fn blk(name: &str) -> RegBlockConf {
        RegBlockConf {
            name: name.into(),
            addr: 0,
            format: RegFormat::Float32,
            scale: 0.0,
            count: 6,
        }
    }

    /// f32 → 大端 u16 寄存器对（避免手算字节脆弱）
    fn f32_regs(v: f32) -> [u16; 2] {
        let b = v.to_bits();
        [(b >> 16) as u16, b as u16]
    }

    /// 三相 float32 → 6 寄存器
    fn phase_regs(a: f32, b: f32, c: f32) -> Vec<u16> {
        [f32_regs(a), f32_regs(b), f32_regs(c)].concat()
    }

    /// 相量块条目（读成功）
    fn fblock(name: &str, a: f32, b: f32, c: f32) -> (RegBlockConf, Result<Vec<u16>, String>) {
        (blk(name), Ok(phase_regs(a, b, c)))
    }

    /// 标量块条目（读成功，count=2）
    fn sblock(name: &str, v: f32) -> (RegBlockConf, Result<Vec<u16>, String>) {
        let mut b = blk(name);
        b.count = 2;
        (b, Ok(f32_regs(v).to_vec()))
    }

    /// 测值均选 f32 可精确表示（1.0/0.5/0.25/220.0/5.5…），decode f32→f64 无误差。
    fn grid_reads() -> BlockReads {
        vec![
            fblock("p", 1.0, 2.0, 3.0),
            sblock("p_total", 5.5),
            fblock("q", 0.5, 0.25, 0.125),
            fblock("pf", 0.75, 0.75, 0.75),
            fblock("u", 220.0, 221.0, 222.0),
            fblock("i", 10.0, 11.0, 12.0),
        ]
    }

    fn grid_reads_no_ptotal() -> BlockReads {
        let mut r = grid_reads();
        r.retain(|(b, _)| b.name != "p_total");
        r
    }

    fn unwrap_data(res: PollResult) -> DataPackage {
        match res {
            PollResult::Data(pkg) => pkg,
            PollResult::Failed(e) => panic!("期望 Data，实际 Failed: {e}"),
        }
    }

    /// 回归锚 1：全正相量 → 分相/顶层各量逐字段（同 startup 196-260）
    #[test]
    fn meter_grid_phase_matches_legacy_semantics() {
        let pkg = build_grid_package(
            [1.0, 2.0, 3.0],
            [0.5, 0.25, 0.125],
            [0.75, 0.8, 0.9],
            [220.0, 221.0, 222.0],
            [10.0, 11.0, 12.0],
            None,
        );
        let phase = pkg.electrical.phase.expect("grid 应有分相数据");
        assert_eq!(phase.active_power, [Some(1.0), Some(2.0), Some(3.0)]);
        assert_eq!(phase.voltage, [Some(220.0), Some(221.0), Some(222.0)]);
        assert_eq!(pkg.electrical.voltage, Some(220.0));
        assert_eq!(pkg.electrical.current, Some(10.0));
        assert_eq!(pkg.electrical.active_power, Some(6.0)); // p_total=None → Σp
        assert_eq!(pkg.electrical.cos_phi, Some(0.75));
        assert_eq!(pkg.electrical.frequency, Some(50.0));
        assert!(pkg.timestamp > 0);
        assert_eq!(pkg.battery.soc, None);
        assert_eq!(pkg.device_status.inverter_status, InverterStatus::Running);
    }

    /// 回归锚 2（P2-3）：负有功相电流取负向；p=0 相显式取正幅值（非 signum 丢幅值）
    #[test]
    fn meter_grid_negative_p_dir_signs_current() {
        let pkg = build_grid_package(
            [-1.0, 2.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.75, 0.75, 0.75],
            [220.0, 220.0, 220.0],
            [10.0, 11.0, 12.0],
            None,
        );
        let phase = pkg.electrical.phase.unwrap();
        assert_eq!(phase.current, [Some(-10.0), Some(11.0), Some(12.0)]);
        // 顶层 current 恒为幅值（取 i_mag[0].abs()）
        assert_eq!(pkg.electrical.current, Some(10.0));
    }

    /// p_total 独立块给出原始值 → active_power 用原始值（非 Σp）
    #[test]
    fn meter_grid_p_total_uses_raw_when_present() {
        let pkg = unwrap_data(poll_to_result(Role::MeterGrid, &grid_reads()));
        assert_eq!(pkg.electrical.active_power, Some(5.5));
    }

    /// p_total 块缺失 → 降级为 Σp（不整周期失败）
    #[test]
    fn meter_grid_p_total_fallback_on_missing() {
        let pkg = unwrap_data(poll_to_result(Role::MeterGrid, &grid_reads_no_ptotal()));
        // Σp = 1+2+3 = 6.0；分相仍完整（p_total 缺失不影响 phase）
        assert_eq!(pkg.electrical.active_power, Some(6.0));
        assert!(pkg.electrical.phase.is_some());
    }

    /// meter_grid 任一相量块缺失/读失败 → Failed
    #[test]
    fn poll_to_result_grid_missing_phase_block_returns_failed() {
        let mut missing_p = grid_reads();
        missing_p.retain(|(b, _)| b.name != "p");
        assert!(matches!(
            poll_to_result(Role::MeterGrid, &missing_p),
            PollResult::Failed(_)
        ));

        let mut err_i = grid_reads();
        for it in err_i.iter_mut() {
            if it.0.name == "i" {
                it.1 = Err("io 超时".into());
            }
        }
        assert!(matches!(
            poll_to_result(Role::MeterGrid, &err_i),
            PollResult::Failed(_)
        ));
    }

    /// battery：soc 块读成功 → battery.soc；读失败 → Failed
    #[test]
    fn poll_to_result_battery_soc_block_maps_soc() {
        let ok = vec![sblock("soc", 65.5), sblock("temp", 25.0)];
        let pkg = unwrap_data(poll_to_result(Role::Battery, &ok));
        assert_eq!(pkg.battery.soc, Some(65.5));
        // 其余占位
        assert_eq!(pkg.battery.temperature, None);
        assert_eq!(pkg.device_status.inverter_status, InverterStatus::Running);

        let mut err = ok;
        err[0].1 = Err("io".into());
        assert!(matches!(
            poll_to_result(Role::Battery, &err),
            PollResult::Failed(_)
        ));

        // 无 soc 块 → Data 占位（不 Failed）
        let empty: BlockReads = Vec::new();
        let pkg2 = unwrap_data(poll_to_result(Role::Battery, &empty));
        assert_eq!(pkg2.battery.soc, None);
    }

    /// telemetry_points：每块取首值，metric=块名；读失败块跳过
    #[test]
    fn telemetry_points_first_value_per_block() {
        let reads = vec![
            fblock("p", 1.0, 2.0, 3.0),
            fblock("u", 220.0, 221.0, 222.0),
            (blk("bad"), Err("io".into())),
        ];
        let pts = telemetry_points(&reads);
        assert_eq!(pts, vec![("p".to_string(), 1.0), ("u".to_string(), 220.0)]);
    }

    /// 非 grid/battery role → 最小 DataPackage（Running + battery 空 + electrical 缺省）
    #[test]
    fn poll_to_result_other_role_returns_minimal_data() {
        let reads = vec![sblock("temp", 23.5)];
        for role in [Role::Hvac, Role::Fire, Role::MeterBatt] {
            let pkg = unwrap_data(poll_to_result(role, &reads));
            assert_eq!(pkg.device_status.inverter_status, InverterStatus::Running);
            assert_eq!(pkg.device_status.pv_power, None);
            assert_eq!(pkg.battery.soc, None);
            assert_eq!(pkg.electrical.voltage, None);
            assert!(pkg.electrical.phase.is_none());
            assert!(pkg.timestamp > 0);
        }
    }
}
