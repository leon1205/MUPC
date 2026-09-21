//! role → DataPackage 组装（S3a Task 4；§10.5 mapper）。
//!
//! 读操作由 scheduler 经 StationBus 完成，本模块为**纯转换**（无 IO，可单测）：
//! 输入为「每站一次 poll 的原始寄存器读结果」（`BlockReads`），输出为 `PollResult`。
//! role 语义：`meter_grid` = 已删 legacy `read_master_meter` 等价语义（master_meter 段
//! 收敛删除后唯一总表形态，S3b-1c；回归等价锚 = grid_convergence.rs canned 测）；
//! `battery` 若含 soc 块填 `battery.soc`；其它 role 无语义点表（厂方待 S3b）→
//! 返回最小 DataPackage 作"站活着"信号，真实遥测值由 scheduler 经
//! [`telemetry_points`] 直接落库（不须经 DataPackage）。

use crate::config::{RegBlockConf, Role};
use crate::points::{self, PointKind};
use mupc_data_processing::meter_regs::{decode_regs, RegDecode};
use mupc_data_processing::telemetry::PhaseElectricalData;
use mupc_data_processing::{
    BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
};

/// 一块的原始读结果（S3b-2 T5 新增，设计 §11.4.7「块读分发」行）：
/// 寄存器块（FC03 保持 / FC04 输入）→ [`BlockData::Regs`]；位块（FC02 离散输入）→
/// [`BlockData::Bits`]。
///
/// **为什么扩成枚举而不是并行的 `BitReads` 通道**（设计的取舍）：`discrete` 块与寄存器块
/// 共用同一条「逐块读 → mapper 判定 → 分发」管线，若另开一条通道，则 mapper 与 scheduler
/// 各出现一份"逐块遍历 + 整站失败"逻辑（两条入口 = 两处可漂移）。扩枚举后**只有一处**。
#[derive(Debug, Clone, PartialEq)]
pub enum BlockData {
    /// 寄存器块读数（FC03/FC04）：`count` 个寄存器。
    Regs(Vec<u16>),
    /// 位块读数（FC02）：`count` **位**（`bit k` = 第 `k%8` 字节的第 `k%8` 位，bit0 = LSB）。
    Bits(Vec<bool>),
}

impl BlockData {
    /// 寄存器视图；位块 → `None`（调用方按块的 `func` 已知形态，此处只做类型防御）。
    pub fn regs(&self) -> Option<&[u16]> {
        match self {
            BlockData::Regs(r) => Some(r),
            BlockData::Bits(_) => None,
        }
    }

    /// 位视图；寄存器块 → `None`。
    pub fn bits(&self) -> Option<&[bool]> {
        match self {
            BlockData::Bits(b) => Some(b),
            BlockData::Regs(_) => None,
        }
    }
}

/// 每站一次 poll 的原始读结果：每个 regs 块一个条目（该块读失败为 Err）。
///
/// scheduler 读回原始寄存器后交 mapper 内部 decode，使 meter_grid 的
/// 「某块读失败/长度不足 → 整周期失败」语义能在 mapper 内表达。
pub type BlockReads = Vec<(RegBlockConf, Result<BlockData, String>)>;

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
/// 逐寄存器对 decode（与已删 legacy `read_meter_phases` 等价）：多余寄存器忽略
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
    decode_phase_block(res.as_ref().ok()?.regs()?, b)
}

/// 取 `p_total` 标量块（2 寄存器）：缺块/读失败/长度不足 → None（调用方降级分相和）。
fn scalar_total(reads: &BlockReads) -> Option<f64> {
    let (b, res) = reads.iter().find(|(b, _)| b.name == "p_total")?;
    let r = res.as_ref().ok()?.regs()?;
    (r.len() >= 2).then(|| decode_regs(&r[..2], b.format, b.scale))
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

/// meter_grid 分相组包（与已删 legacy `read_master_meter` 逐字段等价）。
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

/// 找点名 `soc` 的点（PRD §9.4.3 规则 4 的消费侧查找键）：返回
/// `(该点所在块的读结果, 点内偏移, 解码规格)`；无该点 / 块展开失败 → `None`。
fn soc_point(reads: &BlockReads) -> Option<(Result<Vec<u16>, String>, u16, RegDecode)> {
    for (b, res) in reads {
        let Ok(pts) = points::expand(b) else {
            continue;
        };
        for p in pts {
            if p.metric == "soc" {
                if let PointKind::Scalar { offset, decode } = p.kind {
                    // 位块（`func: discrete`）展开只产 `PointKind::Bit`，故走到这里的块必为寄存器块；
                    // `regs()` 为 None 时按"该块无有效读数"跳过（不误报 Failed）。
                    let regs_res = match res.as_ref() {
                        Ok(d) => match d.regs() {
                            Some(r) => Ok(r.to_vec()),
                            None => continue,
                        },
                        Err(e) => Err(e.clone()),
                    };
                    return Some((regs_res, offset, decode));
                }
            }
        }
    }
    None
}

/// 站一次 poll 的结果组装。role 语义：
/// - `MeterGrid`：p/q/pf/u/i 五相量块缺任一或读失败 → Failed（沿用旧数据）；p_total
///   独立块缺省/失败 → 降级分相和，不整周期失败。
/// - `Battery`：按**点名**找 `soc` 点（PRD §9.4.3 规则 4）；该点所在块读失败 → Failed；
///   读成功 → 填 battery.soc；无 `soc` 点或长度不足 → battery 空（占位，不 Failed）。
/// - 其它 role（MeterBatt/Hvac/Fire/**Pcs**）：无语义点表 → 最小 DataPackage（"站活着"信号）。
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
            // PRD §9.4.3「`soc` 点契约」（v1.3 修订）：按**点名**查找（不再按块名），
            // 解码用该点的 `RegDecode`（块级缺省已在 `points::expand` 折算）。
            match soc_point(reads) {
                None => {} // 无 soc 点：battery 空占位（该形态配置期已被规则 4 拒）
                Some((res, offset, decode)) => match res {
                    Ok(r) => {
                        let start = offset as usize;
                        if r.len() >= start + decode.width() {
                            pkg.battery.soc = Some(decode.decode(&r[start..]));
                        }
                        // 长度不足：保持 None（best-effort，不 Failed）——沿用既有语义
                    }
                    Err(e) => return PollResult::Failed(format!("battery soc 点所在块读失败: {e}")),
                },
            }
            PollResult::Data(pkg)
        }
        Role::MeterBatt | Role::Hvac | Role::Fire | Role::Pcs => {
            PollResult::Data(empty_package())
        }
    }
}

/// 遥测点提取（S3b-2 §11.2.1 的**刻意行为变更**）：对**未声明 `points` 的块**由
/// "取前 2 寄存器 1 点、metric = 块名"改为"**每个值槽 1 点、metric = `<块名>_<序号>`**"
/// （PRD §9.4.2.1 第 4 条 + §9.4.2.2 命名规则），由 [`points::expand`] 统一展开
/// ——**校验期与运行期同一函数**。
///
/// 该函数**只被非 grid 站调用**（grid 走 `on_grid_package`）；块读失败 / 长度不足 → 跳过。
///
/// **位点（`discrete`）在此不产点**（S3b-2 T5 状态，落点见 T6）：T5 已接通 FC02 读通路
/// （`BlockData::Bits` 承载位向量）并**在 scheduler 侧**用统一 `EdgeTracker` 产位/信号事件，
/// 但位点的 **telemetry 落库**（§11.7.2 第 2 条"仅在与上轮不同时落库"）是 T6 的
/// `telemetry_points` 重构内容 ⇒ 本函数当前对 `PointKind::Bit` 仍不产点。
pub fn telemetry_points(reads: &BlockReads) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for (b, res) in reads {
        let Ok(d) = res else { continue };
        let Some(r) = d.regs() else { continue };
        let Ok(pts) = points::expand(b) else {
            continue;
        };
        for p in pts {
            if let PointKind::Scalar { offset, decode } = p.kind {
                let start = offset as usize;
                if r.len() >= start + decode.width() {
                    out.push((p.metric, decode.decode(&r[start..])));
                }
            }
        }
    }
    out
}

/// 消防探测器**地址升序**交叉校验（PRD §9.10 Q-9；设计 §11.4.6 / §11.7.2 第 9 条）。
///
/// 探测器"按地址号从小到大顺序排列"是**位置式点名 ↔ 物理探测器一一对应**的前提
///（PRD §9.5.4）：顺序异常时"第 n 只"不再等于"地址升序的第 n 只"，探测器区点位不可信
/// （telemetry 仍落原值，判据层拒用；事件由 scheduler 产 `fire_detector_addr_order_invalid`）。
///
/// 判据：升序链 = **〔寄存器 11：探测器 1 的地址号〕→〔`fire_det*` 区各组的 `+0`〕**，
/// 每 6 个寄存器一组，各组 **`+0` 寄存器**（地址号）必须**严格升序**（重复/回退均判违规）。
/// 返回 `Some((首个违规组的 1 基序号, 该组的地址值))` —— 组序号从**探测器 1 起算**、
/// 在探测器区内跨分片块连续；升序且唯一 → `None`。
/// 非 `fire` 站 / 无 `fire_det*` 块 / 块读失败 → `None`（无判据可依时不臆断）。
///
/// **链首 = 寄存器 11（v1.7 订正，§11.4.6）**：依据 PRD §9.10 Q-9 原文"以**寄存器 11**
/// 读回的地址值交叉校验" + §9.5.4"第 n 只 = 按地址升序的第 n 只"（该表述对 **n = 1 同样
/// 成立**）—— 探测器 1 不在链里时"第 1 只是否为最小地址"**无判据**。
/// **取数方式与配置解耦**：取 `reads` 中**覆盖寄存器 11 的那一块**的块内偏移 `11 − addr`
///（§9.4.1 参考配置 = `fire_sys`（`addr: 4`）的偏移 7）⇒ **不依赖块名、不依赖点位名**；
/// 该值**只参与升序比较**，不另判其合法域（PRD 未给判据，不猜）。
/// 寄存器 11 未被任何**读成功的寄存器块**覆盖（非 §9.4.1 参考形态）⇒ 链首不可得，
/// 退化为"只校 `fire_det*` 区"（= T5 现状），**不臆断违规**（本设计不新增配置期规则要求
/// 覆盖 11）。寄存器空间按 `BlockData::Regs` 统一处理（不区分 FC03/FC04 —— 与 `fire_det*`
/// 区的既有处理同口径；§9.4.1 参考形态中该寄存器在保持寄存器块内）。
pub fn fire_detector_addr_order_violation(role: Role, reads: &BlockReads) -> Option<(usize, u16)> {
    if role != Role::Fire {
        return None;
    }
    // 链首：探测器 1 的地址号（寄存器 11）。取首个**可读且覆盖**它的寄存器块。
    let head: Option<u16> = reads.iter().find_map(|(b, res)| {
        let off = 11usize.checked_sub(usize::from(b.addr))?;
        if off >= usize::from(b.count) {
            return None; // 该块不覆盖寄存器 11
        }
        res.as_ref().ok()?.regs()?.get(off).copied()
    });
    let mut prev: Option<u16> = head;
    let mut group: usize = usize::from(head.is_some()); // 链首占第 1 组（探测器 1）
    for (b, res) in reads {
        if !b.name.starts_with("fire_det") {
            continue;
        }
        let Some(regs) = res.as_ref().ok().and_then(|d| d.regs()) else {
            continue;
        };
        for addr in regs.iter().step_by(6) {
            group += 1;
            if let Some(p) = prev {
                if *addr <= p {
                    return Some((group, *addr));
                }
            }
            prev = Some(*addr);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PointConf, RegFunc};
    use mupc_data_processing::meter_regs::{RegFormat, WordOrder};

    fn blk(name: &str) -> RegBlockConf {
        RegBlockConf {
            name: name.into(),
            addr: 0,
            func: RegFunc::Holding,
            format: RegFormat::Float32,
            scale: 0.0,
            count: 6,
            offset: 0.0,
            byte_swap: false,
            points: Vec::new(),
            read_slice: false,
        }
    }

    /// 寄存器块（`addr`/`count` 可指定）—— 地址升序校验的用例只需要这两个字段。
    fn rblk(name: &str, addr: u16, count: u16) -> RegBlockConf {
        let mut b = blk(name);
        b.addr = addr;
        b.count = count;
        b
    }

    /// `fire` 站一次 poll 的块读结果：覆盖寄存器 11 的块（**链首载体**，`name`/`addr` 可任意
    /// —— 链首取数不依赖块名/点位名）+ `fire_det*` 区（`addrs` = 各探测器组的 `+0` 地址号，
    /// 每 6 寄存器一组）。
    ///
    /// `reg11 = None` ⇒ 配置**未覆盖寄存器 11**（非 §9.4.1 参考形态）。
    fn fire_order_reads(reg11: Option<(RegBlockConf, u16)>, addrs: &[u16]) -> BlockReads {
        let mut reads: BlockReads = Vec::new();
        if let Some((b, det1)) = reg11 {
            let mut regs = vec![0u16; usize::from(b.count)];
            regs[usize::from(11 - b.addr)] = det1; // 覆盖性由用例传入的块参数保证
            reads.push((b, Ok(BlockData::Regs(regs))));
        }
        if !addrs.is_empty() {
            let mut det = Vec::with_capacity(addrs.len() * 6);
            for a in addrs {
                det.push(*a);
                det.extend([0u16; 5]);
            }
            reads.push((
                rblk("fire_det", 17, 6 * addrs.len() as u16),
                Ok(BlockData::Regs(det)),
            ));
        }
        reads
    }

    /// 点名式块（S3b-2 A12/A13）：块名不再是查找键，`soc` 由**点级 `name`** 声明。
    fn named_soc_block() -> RegBlockConf {
        RegBlockConf {
            name: "bms_io".into(),
            addr: 100,
            func: RegFunc::Holding,
            format: RegFormat::Float32,
            scale: 1.0,
            count: 2,
            offset: 0.0,
            byte_swap: false,
            points: vec![PointConf {
                at: 1,
                count: 1,
                name: Some("soc".into()),
                format: None,
                scale: None,
                offset: None,
                word_order: WordOrder::HiLo,
            }],
            read_slice: false,
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
    fn fblock(name: &str, a: f32, b: f32, c: f32) -> (RegBlockConf, Result<BlockData, String>) {
        (blk(name), Ok(BlockData::Regs(phase_regs(a, b, c))))
    }

    /// 标量块条目（读成功，count=2）
    fn sblock(name: &str, v: f32) -> (RegBlockConf, Result<BlockData, String>) {
        let mut b = blk(name);
        b.count = 2;
        (b, Ok(BlockData::Regs(f32_regs(v).to_vec())))
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

    /// 回归锚 1：全正相量 → 分相/顶层各量逐字段（等价已删 legacy read_master_meter）
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
        assert_eq!(phase.current, [Some(10.0), Some(11.0), Some(12.0)]); // p>=0 → i_mag 幅值
        assert_eq!(phase.reactive_power, [Some(0.5), Some(0.25), Some(0.125)]);
        assert_eq!(phase.cos_phi, [Some(0.75), Some(0.8), Some(0.9)]);
        assert_eq!(pkg.electrical.voltage, Some(220.0));
        assert_eq!(pkg.electrical.current, Some(10.0));
        assert_eq!(pkg.electrical.active_power, Some(6.0)); // p_total=None → Σp
        assert_eq!(pkg.electrical.reactive_power, Some(0.875)); // q.sum()=0.5+0.25+0.125
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

    /// battery：**点名式** `soc` 点所在块读成功 → battery.soc；读失败 → Failed
    /// （A12 fixture 订正：按点名查找后，承载 soc 的块名可任意）
    #[test]
    fn poll_to_result_battery_soc_block_maps_soc() {
        let ok = vec![
            (
                named_soc_block(),
                Ok(BlockData::Regs(f32_regs(65.5).to_vec())),
            ),
            sblock("temp", 25.0),
        ];
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

        // 无 soc 点 → Data 占位（不 Failed）
        let empty: BlockReads = Vec::new();
        let pkg2 = unwrap_data(poll_to_result(Role::Battery, &empty));
        assert_eq!(pkg2.battery.soc, None);
    }

    /// telemetry_points：**每个值槽 1 点**（PRD §9.4.2.1 第 4 条），metric = `<块名>_<序号>`；
    /// 读失败块整块跳过。单块 6 寄存器 / float32 ⇒ 3 个值槽 ⇒ 序号 1/3/5。
    #[test]
    fn telemetry_points_every_value_slot() {
        let reads = vec![
            fblock("p", 1.0, 2.0, 3.0),
            fblock("u", 220.0, 221.0, 222.0),
            (blk("bad"), Err("io".into())),
        ];
        let pts = telemetry_points(&reads);
        assert_eq!(
            pts,
            vec![
                ("p_1".to_string(), 1.0),
                ("p_3".to_string(), 2.0),
                ("p_5".to_string(), 3.0),
                ("u_1".to_string(), 220.0),
                ("u_3".to_string(), 221.0),
                ("u_5".to_string(), 222.0),
            ]
        );
    }

    /// **链首 = 寄存器 11（探测器 1 的地址号，v1.7 §11.4.6 订正）**：探测器 1 也在升序链里
    /// ⇒ 组序号与 PRD §9.5.4"第 n 只 = 按地址升序的第 n 只"的 n 逐一对齐。
    #[test]
    fn fire_addr_order_chain_head_includes_detector_one() {
        // 探测器 1 地址 9 > 探测器 2 地址 3 ⇒ **第 2 组**违规
        assert_eq!(
            fire_detector_addr_order_violation(
                Role::Fire,
                &fire_order_reads(Some((rblk("fire_sys", 4, 13), 9)), &[3, 4])
            ),
            Some((2, 3))
        );
        // 探测器 1 地址 0 < 3 < 4 ⇒ 严格升序，无违规
        assert_eq!(
            fire_detector_addr_order_violation(
                Role::Fire,
                &fire_order_reads(Some((rblk("fire_sys", 4, 13), 0)), &[3, 4])
            ),
            None
        );
        // 探测器 1 地址 4 = 探测器 2 地址 4 ⇒ **重复**亦判违规（链首参与比较）
        assert_eq!(
            fire_detector_addr_order_violation(
                Role::Fire,
                &fire_order_reads(Some((rblk("fire_sys", 4, 13), 4)), &[4, 5])
            ),
            Some((2, 4))
        );
    }

    /// 链首取数**不依赖块名、不依赖点位名**（§11.4.6）：覆盖寄存器 11 的块叫什么都行，
    /// 只要块内偏移是 `11 − addr`；未覆盖寄存器 11 ⇒ 退化为"只校 `fire_det*` 区"
    ///（= T5 现状），**不臆断违规**；非 fire 站 / 无探测器块 ⇒ 无判据可依，同样不臆断。
    #[test]
    fn fire_addr_order_chain_head_is_name_agnostic_with_fallback() {
        // 换个块名（addr 10 / count 3 ⇒ 覆盖寄存器 10..12，偏移 1 = 寄存器 11）
        assert_eq!(
            fire_detector_addr_order_violation(
                Role::Fire,
                &fire_order_reads(Some((rblk("whatever", 10, 3), 9)), &[3])
            ),
            Some((2, 3))
        );
        // 未覆盖寄存器 11：无链首 ⇒ 只校 fire_det 区（组序号回到 1 基起）
        assert_eq!(
            fire_detector_addr_order_violation(Role::Fire, &fire_order_reads(None, &[3, 2])),
            Some((2, 2))
        );
        // 未覆盖 11 且 fire_det 区自身严格升序 ⇒ 无判据可依，不臆断
        assert_eq!(
            fire_detector_addr_order_violation(Role::Fire, &fire_order_reads(None, &[2, 3])),
            None
        );
        // 非 fire 站 / 无 fire_det 块 ⇒ 不产判据
        assert_eq!(
            fire_detector_addr_order_violation(
                Role::Hvac,
                &fire_order_reads(Some((rblk("fire_sys", 4, 13), 9)), &[3, 2])
            ),
            None
        );
        assert_eq!(
            fire_detector_addr_order_violation(
                Role::Fire,
                &fire_order_reads(Some((rblk("fire_sys", 4, 13), 9)), &[])
            ),
            None
        );
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
