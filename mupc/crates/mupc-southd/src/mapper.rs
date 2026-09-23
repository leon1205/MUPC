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

/// battery 分支的 SOC 取值结果（设计 §11.4.6 v1.3：把"底块读失败"等四种情形**显式化**）。
///
/// 四情形与 [`poll_to_result`] 的对应（逐条实现、逐条测）：
///
/// | 情形 | 判据 | [`SocOutcome`] | `pkg.battery.soc` | `PollResult` |
/// |------|------|----------------|-------------------|--------------|
/// | ① 底块读成功且含 `soc` 点 | 该块 `Ok` 且展开后 `metric == "soc"` | [`SocOutcome::Value`] | 域内 `Some(值)` / **域外 `None`** | `Data` |
/// | ② **底块读失败** | 该块 `Err(e)` | [`SocOutcome::BlockFailed`] | `None` | **`Failed(e)`** |
/// | ③ 全站无任何块含 `soc` 点 | 展开后无 `metric == "soc"` | [`SocOutcome::NoSuchPoint`] | `None` | `Data`（空占位） |
/// | ④ 非承载 `soc` 的其它块读失败 | 任一其它块 `Err` | （本枚举不表达） | `None` | **`Failed(e)`** |
///
/// **②③④ 的差别是刻意的**：②④ 属「通信/链路」故障 ⇒ 整站失败（可退避自愈）；③ 属「配置」
/// 错误 ⇒ 配置期（规则 4）已拒，此处的 `Data` 只为"调度器单测可绕过 validate"而保留。
/// 二者混为一谈会让"配置错"在运行期表现为"站离线"（PRD §9.7.2 第 5 条禁止）。
///
/// 注意 [`SocOutcome::Value`] **不做域检查**（域检查是调用方的事，见 [`soc_in_domain`]）
/// ——"解出值"与"该值可否进控制链"是两件事，`telemetry` 要的是前者、控制链要的是后者。
#[derive(Debug, Clone, PartialEq)]
pub enum SocOutcome {
    /// ① 该块读成功且 `soc` 点可完整解码 → 原始解码值（**未做域检查**）
    Value(f64),
    /// ③ 全站无任何块含 `soc` 点（该形态在配置期已被规则 4 拒；此处只为单测可绕过 validate）
    NoSuchPoint,
    /// ② 承载 `soc` 点的块读失败（含"读回长度装不下该点"）——整站失败，**不得**只丢 SOC
    BlockFailed(String),
}

/// battery 的 SOC 域检查（PRD §9.6.3）：`0 ≤ v ≤ 100` 且有限 ⇒ `true`。
///
/// `soc` 是唯一进控制链的南向采集点 ⇒ 解出的值**先做域检查**，越界视为该点无效：
/// ① 不推 AiIntegrator（回落核间 SOC，避免以坏值剪带）；② 产告警事件（scheduler，
/// §11.4.7 事件 ①）；③ telemetry **仍按原值落库**（保留证据，不掩盖 —— 由
/// [`telemetry_points`] 独立完成，不看本判据）。
pub fn soc_in_domain(v: f64) -> bool {
    v.is_finite() && (0.0..=100.0).contains(&v)
}

/// battery 分支的 SOC 取值（四情形见 [`SocOutcome`] 表）。
///
/// 查找键 = **点名 `soc`**（不再按块名；PRD §9.4.3 规则 4 的消费契约），解码用该点的
/// `RegDecode`（块级缺省已在 [`points::expand`] 折算）。
pub fn battery_soc(reads: &BlockReads) -> SocOutcome {
    match soc_point(reads) {
        None => SocOutcome::NoSuchPoint,
        Some((res, offset, decode)) => match res {
            Err(e) => SocOutcome::BlockFailed(e),
            Ok(r) => {
                let start = offset as usize;
                if r.len() >= start + decode.width() {
                    SocOutcome::Value(decode.decode(&r[start..]))
                } else {
                    // 读回长度装不下该点（响应被截断）⇒ 属"该块读失败"（②），**不得**静默
                    // 退化为"站在线但 SOC 缺失"——那正是 §11.4.6 ② 明文禁止的形态。
                    // ⚠️ 设计未列此形态（四情形表不含"长度不足"），本实现按 ② 同策处理（**待评审追认**）。
                    SocOutcome::BlockFailed(format!(
                        "soc 点读回长度不足：块内偏移 {start} + 宽度 {} > 实际寄存器数 {}",
                        decode.width(),
                        r.len()
                    ))
                }
            }
        },
    }
}

/// 消防钢瓶气压的**绝对寄存器地址**（PRD §9.5.4：addr 5 = 钢瓶气压 kPa）。
const FIRE_CYLINDER_PRESSURE_ADDR: u16 = 5;
/// 消防登记数的**点名**（PRD §9.4.1/§9.5.4 v1.7 定名的跨文档契约点；禁按块名 + 硬编码地址查）。
const FIRE_DET_COUNT_METRIC: &str = "fire_det_count";

/// 取**绝对寄存器地址 `addr` 上的那个标量点**的解码值（点位起始地址 == `addr`）。
///
/// **取数方式与配置解耦**（与 [`fire_detector_addr_order_violation`] 的链首取数同取向，
/// §11.4.6）：不依赖块名、不依赖点位名，现场改块划分/改名都不会让判据失配；
/// 块读失败 / 无点落在该地址 / 解码长度不足 → `None`。
///
/// **边界（刻意的）**：只认"点的**起始**地址等于 `addr`"，故若某 32 位点**跨**该地址
/// （一个点占 `addr-1`/`addr` 两寄存器）⇒ 视为无判据。消防站的这些量全是 16 位
/// （`uint16`/`scale 1.0`，PRD §9.5.4）⇒ 本边界在适用域内不可达。
fn scalar_at(reads: &BlockReads, addr: u16) -> Option<f64> {
    for (b, res) in reads {
        let Some(regs) = res.as_ref().ok().and_then(|d| d.regs()) else {
            continue;
        };
        let Ok(pts) = points::expand(b) else {
            continue;
        };
        for p in pts {
            let PointKind::Scalar { offset, decode } = p.kind else {
                continue;
            };
            if b.addr.wrapping_add(offset) != addr {
                continue;
            }
            let start = offset as usize;
            return if regs.len() >= start + decode.width() {
                Some(decode.decode(&regs[start..]))
            } else {
                None
            };
        }
    }
    None
}

/// 探测器 1 的地址号所在的**绝对寄存器地址**（PRD §9.10 Q-9："以**寄存器 11** 读回的
/// 地址值交叉校验"）。
const FIRE_DET1_ADDR_REG: u16 = 11;

/// 升序链的**链首** = 探测器 1 的地址号（[`fire_detector_addr_order_violation`]），同时
/// 也是"配置是否覆盖探测器 1"的判据（[`fire_detector_mismatch`] 的容量式据它决定是否 +1）。
///
/// 取 `reads` 中**读成功且覆盖寄存器 11** 的寄存器块的块内偏移 `11 − addr` ——
/// 与块名、点位名**解耦**（§11.4.6），现场改块划分/改名都不会让判据失配。
/// **配置未覆盖 11 / 覆盖它的块读失败 / 该块是位块 ⇒ `None` = 链首不可得。**
///
/// 两个判据**共用本函数** ⇒ "链首不可得"的降级口径在两者间**结构性一致**（不会各自漂移）。
fn fire_chain_head(reads: &BlockReads) -> Option<u16> {
    reads.iter().find_map(|(b, res)| {
        let off = usize::from(FIRE_DET1_ADDR_REG).checked_sub(usize::from(b.addr))?;
        if off >= usize::from(b.count) {
            return None; // 该块不覆盖寄存器 11
        }
        res.as_ref().ok()?.regs()?.get(off).copied()
    })
}

/// 消防探测器登记数交叉校验（PRD §9.5.4"登记数交叉校验（**强制**）"；设计 §11.4.6）。
///
/// 判据：把**点名 `fire_det_count` 的点**（= 寄存器 10，v1.7 定名；**按点名查找**，
/// 不得按"块名 `fire_det` + 硬编码寄存器 10" —— 那属设备特判，违反 PRD G-5）读回的值，
/// 与**配置登记的探测器只数**比对；**不一致 → 返回读回值**（作为事件的诊断量），
/// 一致 → `None`。
///
/// **容量式 = 链首存在时 `1 + Σ(fire_det 前缀块 count) / 6`**（架构师订正口径）：
/// 探测器 1 的 6 个寄存器在 **`fire_sys`** 块内（addr 11–16），**不在** `fire_det*` 区
/// ⇒ 只数 `fire_det` 前缀块会**漏计探测器 1**：按 §9.4.1 参考配置（n=20：
/// `fire_det.count = 114` ⇒ 19 组）得 **19**，而寄存器 10 读回 **20** ⇒ 判据**恒真**
/// ⇒ 永久假告警（首次观测 1 条 + 每次站恢复再 1 条），**PRD §9.5.4 的强制交叉校验随之
/// 永久失效**（RC-5 失效）。
///
/// **降级口径（与地址序校验一致）**：`fire_sys`（或任何读成功的寄存器块）**未覆盖寄存器
/// 11** ⇒ 链首不可得 ⇒ **不加 1**，只按 `Σ/6` 计 —— 与
/// [`fire_detector_addr_order_violation`] 的"链首不可得 ⇒ 退化为只校 `fire_det*` 区"
/// 同源（两者共用 [`fire_chain_head`]）。此时"设备报了 20 只、配置只采到 19 只"**正是本
/// 校验要抓的盲区** ⇒ 判不一致是**期望行为**，不是误报。
///
/// 探测器增减须人工复核配置，防"新增探测器未被采集"的静默盲区 —— 这是 PRD 标"强制"的
/// 原因。非 `fire` 站 / 无 `fire_det_count` 点（含承载它的块读失败）/ 块展开失败 → `None`
///（**无判据可依时不臆断**，与地址序校验同策）。
pub fn fire_detector_mismatch(role: Role, reads: &BlockReads) -> Option<f64> {
    if role != Role::Fire {
        return None;
    }
    let read_back = reads.iter().find_map(|(b, res)| {
        let regs = res.as_ref().ok()?.regs()?;
        let pts = points::expand(b).ok()?;
        pts.into_iter().find_map(|p| {
            if p.metric != FIRE_DET_COUNT_METRIC {
                return None;
            }
            let PointKind::Scalar { offset, decode } = p.kind else {
                return None;
            };
            let start = offset as usize;
            (regs.len() >= start + decode.width()).then(|| decode.decode(&regs[start..]))
        })
    })?;
    let groups: u32 = reads
        .iter()
        .filter(|(b, _)| b.name.starts_with("fire_det"))
        .map(|(b, _)| u32::from(b.count))
        .sum::<u32>()
        / 6;
    // 链首存在（配置覆盖寄存器 11）⇒ 容量含探测器 1；链首不可得 ⇒ 退化为只按 `fire_det*` 区。
    let capacity = groups + u32::from(fire_chain_head(reads).is_some());
    (read_back != f64::from(capacity)).then_some(read_back)
}

/// 钢瓶气压「是否配置」（PRD §9.7.6；设计 §11.4.6/§11.7.3）：`ever_nonzero` = 本站生命周期内
/// 该点是否出现过非 0 值（由 scheduler 每站一个 `bool` 记忆维护，一旦为真**不再回退** ——
/// 钢瓶气压不会在业务上"变回未配置"）。
///
/// 从未出现过非 0（含本轮仍为 0）⇒ `false`：**展示层标"未配置"，不得显示 "0 kPa"、
/// 不得据此判"气压异常/泄漏"**；一旦出现过 ⇒ `true`（此后恒 0 按真实 0 展示）。
///
/// **该点不产任何事件**（PRD §9.7.6 明令）：本函数只是**展示口径**的判据，与事件层零重叠。
/// 取数按**绝对寄存器地址 5**（不依赖块名/点位名，见 [`scalar_at`]）。
pub fn cylinder_pressure_configured(reads: &BlockReads, ever_nonzero: bool) -> bool {
    ever_nonzero || scalar_at(reads, FIRE_CYLINDER_PRESSURE_ADDR).is_some_and(|v| v != 0.0)
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
            match battery_soc(reads) {
                // ① 正常路径。**域外 ⇒ `pkg.battery.soc` 保持 `None`**（控制链拿不到坏值，
                // PRD §9.6.3 ①：回落核间 SOC）；telemetry 仍按**原值**落库（§9.6.3 ③，
                // 由 `telemetry_points` 独立完成 —— 它不看本判据），越界告警事件由 scheduler 发。
                SocOutcome::Value(v) => {
                    if soc_in_domain(v) {
                        pkg.battery.soc = Some(v);
                    }
                }
                // ③ 无 `soc` 点：battery 空占位（该形态配置期已被规则 4 拒）
                SocOutcome::NoSuchPoint => {}
                // ② 承载 `soc` 点的块读失败 → 整站本轮失败（不退化为"只丢 SOC"）
                SocOutcome::BlockFailed(e) => {
                    return PollResult::Failed(format!("battery soc 点所在块读失败: {e}"));
                }
            }
            // ④ 非承载 `soc` 的其它块读失败 ⇒ 整站失败（§10.7"任一块失败 = 整站本轮失败、
            // 无部分交付"）：避免"站在线而其余量静默缺失"的半个站。
            if let Some((b, Err(e))) = reads.iter().find(|(_, r)| r.is_err()) {
                return PollResult::Failed(format!("battery 站块 {} 读失败: {e}", b.name));
            }
            PollResult::Data(pkg)
        }
        Role::MeterBatt | Role::Hvac | Role::Fire | Role::Pcs => {
            PollResult::Data(empty_package())
        }
    }
}

/// 遥测点样本（S3b-2 §11.4.6）：比 `(String, f64)` 多一个**类别标志**，供 scheduler 做
/// **位点的变化沿过滤**（标量点每轮全量落库、位点仅在与上轮不同时落库，§11.7.2 第 1/2 条
/// 的 D2 口径）。`kind` 用枚举而非 `bool`（设计 v1.3 订正）：将来若加"字级信号"不破签名。
#[derive(Debug, Clone, PartialEq)]
pub struct TelemetrySample {
    /// 点位名（PRD §9.4.2.2）
    pub metric: String,
    /// 遥测值（位点 = `1.0` / `0.0`）
    pub value: f64,
    /// 点位形态（决定 scheduler 侧是否走变化沿过滤）
    pub kind: SampleKind,
}

/// 点位形态：`Scalar` = 标量点（16/32 位）、`Bit` = 位点（`func: discrete`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    Scalar,
    Bit,
}

/// 遥测点提取（S3b-2 §11.2.1 的**刻意行为变更**）：对**未声明 `points` 的块**由
/// "取前 2 寄存器 1 点、metric = 块名"改为"**每个值槽 1 点、metric = `<块名>_<序号>`**"
/// （PRD §9.4.2.1 第 4 条 + §9.4.2.2 命名规则），由 [`points::expand`] 统一展开
/// ——**校验期与运行期同一函数**。
///
/// **返回全部点**（含**位点**，S3b-2 T6 补齐）：标量与位一视同仁地产出（位点 `value` =
/// `1.0`/`0.0`、`kind = Bit`），AC-6 ①"点产出"在 mapper 层断言；**变化沿过滤在 scheduler**
/// （§11.4.7）——"点产出"与"落库节流"是两层，互不混淆（§11.2.4 末注）。
///
/// 该函数**只被非 grid 站调用**（grid 走 `on_grid_package`，分相语义不经本函数）；
/// 块读失败 / 该点形态与块数据形态不符 / 长度不足 → 该点跳过。
pub fn telemetry_points(role: Role, reads: &BlockReads) -> Vec<TelemetrySample> {
    // 只读断言：本函数只服务非 grid 站（`role` 参数在此形态下**无判据作用** —— 点的形态
    // 与取值只由「块 + 配置」决定；它的存在是为保持 §11.4.6 的接口形状，且把"grid 站不得
    // 走本函数"这条分流口径写成可执行断言，防将来把分相站也接进逐点展开）。
    debug_assert_ne!(
        role,
        Role::MeterGrid,
        "meter_grid 站走 on_grid_package（分相语义），不得经 telemetry_points"
    );
    let mut out = Vec::new();
    for (b, res) in reads {
        let Ok(d) = res else { continue };
        let Ok(pts) = points::expand(b) else {
            continue;
        };
        for p in pts {
            match p.kind {
                PointKind::Scalar { offset, decode } => {
                    let Some(r) = d.regs() else { continue };
                    let start = offset as usize;
                    if r.len() >= start + decode.width() {
                        out.push(TelemetrySample {
                            metric: p.metric,
                            value: decode.decode(&r[start..]),
                            kind: SampleKind::Scalar,
                        });
                    }
                }
                PointKind::Bit { offset } => {
                    let Some(bits) = d.bits() else { continue };
                    // 读回位数不足 → 该位按 0（防御；正常路径由块读长度保证）
                    let active = bits.get(offset as usize).copied().unwrap_or(false);
                    out.push(TelemetrySample {
                        metric: p.metric,
                        value: if active { 1.0 } else { 0.0 },
                        kind: SampleKind::Bit,
                    });
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
/// 覆盖 11）。**该降级由 [`fire_chain_head`] 与 [`fire_detector_mismatch`] 的容量式共用**
/// ⇒ 两条判据对"链首是否存在"的判断**结构性一致**。寄存器空间按 `BlockData::Regs` 统一
/// 处理（不区分 FC03/FC04 —— 与 `fire_det*` 区的既有处理同口径；§9.4.1 参考形态中该
/// 寄存器在保持寄存器块内）。
pub fn fire_detector_addr_order_violation(role: Role, reads: &BlockReads) -> Option<(usize, u16)> {
    if role != Role::Fire {
        return None;
    }
    // 链首：探测器 1 的地址号（寄存器 11）。取首个**可读且覆盖**它的寄存器块
    // （与 `fire_detector_mismatch` 的容量式**共用**，⇒ 降级口径一致）。
    let head: Option<u16> = fire_chain_head(reads);
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
            interval_ms: None,
        }
    }

    /// 位块（`func: discrete`，`count` = 位数）—— 位点产出用例用。
    fn dblk(name: &str, count: u16) -> RegBlockConf {
        let mut b = blk(name);
        b.func = RegFunc::Discrete;
        b.count = count;
        b
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
            interval_ms: None,
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

    /// **Battery 分支的四情形**（设计 §11.4.6 v1.3 表：逐条实现、逐条测）。
    ///
    /// ②④ 属"通信/链路"故障 ⇒ 整站失败（可退避自愈）；③ 属"配置"错误 ⇒ 配置期已拒，
    /// 运行期只保留空占位（不把配置错表现为"站离线"，PRD §9.7.2 第 5 条）。
    #[test]
    fn battery_soc_four_cases() {
        // ① 底块读成功且含 `soc` 点（域内）→ Value + pkg.battery.soc = Some
        let ok = vec![(
            named_soc_block(),
            Ok(BlockData::Regs(f32_regs(65.5).to_vec())),
        )];
        assert_eq!(battery_soc(&ok), SocOutcome::Value(65.5));
        assert_eq!(
            unwrap_data(poll_to_result(Role::Battery, &ok)).battery.soc,
            Some(65.5)
        );

        // ①′ 域外（1234.0 > 100）→ **仍解出 Value**（telemetry 用原值落库），
        //     但 `pkg.battery.soc` = None（控制链拿不到坏值，PRD §9.6.3 ①）
        let out = vec![(
            named_soc_block(),
            Ok(BlockData::Regs(f32_regs(1234.0).to_vec())),
        )];
        assert_eq!(battery_soc(&out), SocOutcome::Value(1234.0));
        assert_eq!(
            unwrap_data(poll_to_result(Role::Battery, &out)).battery.soc,
            None,
            "域外值不得进控制链（回落核间 SOC）"
        );

        // ② 承载 soc 点的块读失败 → BlockFailed + PollResult::Failed（整站失败）
        let bad = vec![(named_soc_block(), Err("io 超时".into()))];
        assert!(matches!(
            battery_soc(&bad),
            SocOutcome::BlockFailed(ref e) if e.contains("io 超时")
        ));
        assert!(matches!(
            poll_to_result(Role::Battery, &bad),
            PollResult::Failed(_)
        ));

        // ③ 全站无任何块含 `soc` 点 → NoSuchPoint + Data 空占位（不 Failed）
        assert_eq!(battery_soc(&vec![]), SocOutcome::NoSuchPoint);
        assert_eq!(
            unwrap_data(poll_to_result(Role::Battery, &vec![]))
                .battery
                .soc,
            None
        );

        // ④ 非承载 soc 的其它块读失败（soc 所在块 Ok）→ 整站失败（不得"半个站"）
        let other_bad = vec![
            (
                named_soc_block(),
                Ok(BlockData::Regs(f32_regs(65.5).to_vec())),
            ),
            (blk("other"), Err("io".into())),
        ];
        assert_eq!(
            battery_soc(&other_bad),
            SocOutcome::Value(65.5),
            "② 与 ④ 的分界：soc 所在块本身是 Ok"
        );
        let e = match poll_to_result(Role::Battery, &other_bad) {
            PollResult::Failed(e) => e,
            PollResult::Data(_) => panic!("④ 其它块失败应整站失败"),
        };
        assert!(e.contains("other"), "失败文案须定位到块: {e}");
    }

    /// **读回长度装不下 `soc` 点**（响应被截断）→ 按 ② 同策（`BlockFailed`）。
    /// **不得**静默退化为 `None` —— 那正是 §11.4.6 ② 明文禁止的"站在线但 SOC 静默缺失"。
    ///
    /// ⚠️ 该形态**不在设计四情形表内**（T6 就地登记，待评审追认；口径见 [`battery_soc`] 注释）。
    #[test]
    fn battery_soc_short_read_is_block_failure() {
        let short = vec![(named_soc_block(), Ok(BlockData::Regs(vec![0u16])))];
        assert!(matches!(
            battery_soc(&short),
            SocOutcome::BlockFailed(ref e) if e.contains("长度不足")
        ));
        assert!(matches!(
            poll_to_result(Role::Battery, &short),
            PollResult::Failed(_)
        ));
    }

    /// SOC 域检查边界（PRD §9.6.3）：闭区间 `[0, 100]` + 有限性。
    #[test]
    fn soc_domain_boundaries() {
        for v in [0.0, 1.0, 50.0, 99.9, 100.0] {
            assert!(soc_in_domain(v), "{v} 应在域内");
        }
        for v in [
            -0.1,
            100.1,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1600.0,
        ] {
            assert!(!soc_in_domain(v), "{v} 应在域外");
        }
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
        let pts = telemetry_points(Role::Hvac, &reads);
        let got: Vec<(String, f64)> = pts.iter().map(|s| (s.metric.clone(), s.value)).collect();
        assert_eq!(
            got,
            vec![
                ("p_1".to_string(), 1.0),
                ("p_3".to_string(), 2.0),
                ("p_5".to_string(), 3.0),
                ("u_1".to_string(), 220.0),
                ("u_3".to_string(), 221.0),
                ("u_5".to_string(), 222.0),
            ]
        );
        assert!(
            pts.iter().all(|s| s.kind == SampleKind::Scalar),
            "寄存器块产出的都是标量点"
        );
    }

    /// **位点也落 telemetry**（S3b-2 T6，§11.7.2 第 2 条）：`discrete` 块逐位产点，
    /// `value` = 1.0/0.0、`kind = Bit`（scheduler 据此只对位点做变化沿过滤）。
    /// 位块与寄存器块**混排**时两类点按块序产出。
    #[test]
    fn telemetry_points_includes_bit_points() {
        let reads = vec![
            (
                dblk("hvac_di", 3),
                Ok(BlockData::Bits(vec![true, false, true])),
            ),
            sblock("temp", 23.5),
        ];
        let pts = telemetry_points(Role::Hvac, &reads);
        assert_eq!(
            pts,
            vec![
                TelemetrySample {
                    metric: "hvac_di_1".to_string(),
                    value: 1.0,
                    kind: SampleKind::Bit,
                },
                TelemetrySample {
                    metric: "hvac_di_2".to_string(),
                    value: 0.0,
                    kind: SampleKind::Bit,
                },
                TelemetrySample {
                    metric: "hvac_di_3".to_string(),
                    value: 1.0,
                    kind: SampleKind::Bit,
                },
                TelemetrySample {
                    metric: "temp_1".to_string(),
                    value: 23.5,
                    kind: SampleKind::Scalar,
                },
            ]
        );
    }

    /// **位点的形态与数据错配 → 跳过**（防御）：位块数据（`Bits`）喂给标量点视图 ⇒ 该点
    /// 不产；反之寄存器块数据喂给位点视图同理（块 `func` 与 `BlockData` 由 scheduler 保证
    /// 对应，此处只钉住"不 panic、不误产"）。
    #[test]
    fn telemetry_points_skips_shape_mismatch() {
        let reads = vec![
            (blk("scalar_blk"), Ok(BlockData::Bits(vec![true, false]))),
            (dblk("bit_blk", 2), Ok(BlockData::Regs(vec![7, 8]))),
        ];
        assert!(telemetry_points(Role::Hvac, &reads).is_empty());
    }

    /// `Role::Pcs` 与其它非 grid/battery role 同臂 → 最小 `DataPackage`（"站活着"信号）：
    /// PCS 的全部点只走 `telemetry_points`/`on_station_telemetry`，**不触发** `on_battery_soc`
    /// （N-1）与 `on_grid_package`（N-2）——后两条由 scheduler 的 role 判断结构性保证。
    #[test]
    fn pcs_role_returns_minimal_data_package() {
        let reads = vec![sblock("pcs_3zone", 1.0)];
        let pkg = unwrap_data(poll_to_result(Role::Pcs, &reads));
        assert_eq!(pkg.device_status.inverter_status, InverterStatus::Running);
        assert_eq!(pkg.battery.soc, None, "N-1：PCS 转述 SOC 不得进控制链");
        assert_eq!(pkg.electrical.voltage, None);
        assert!(
            pkg.electrical.phase.is_none(),
            "N-2：phase 真源唯一 = meter_grid"
        );
    }

    /// 消防块（`addr` 起 `count` 个寄存器，逐点声明并把 `at: 7` 命名为契约点
    /// `fire_det_count` = 寄存器 10）。**块名故意取 `whatever`**：登记数按**点名**查找，
    /// 越依赖块名越容易写成设备特判（PRD G-5）。
    fn fire_count_block(addr: u16, count: u16) -> RegBlockConf {
        let mut b = blk("whatever");
        b.addr = addr;
        b.count = count;
        b.format = RegFormat::Uint16;
        b.scale = 1.0;
        b.points = (1..=count)
            .map(|k| PointConf {
                at: k,
                count: 1,
                name: (k == 7).then(|| FIRE_DET_COUNT_METRIC.to_string()),
                format: None,
                scale: None,
                offset: None,
                word_order: WordOrder::HiLo,
            })
            .collect();
        b
    }

    /// fire 站一次 poll（**链首不可得的降级形态**）：块只覆盖寄存器 4..10（`count: 7`）——
    /// **不含**探测器 1 的地址寄存器 11 ⇒ 链首不可得、容量不加 1。`count_reg10` 写进
    /// 偏移 6 = 寄存器 10；另有 `fire_det` 探测器区（`det_groups` 组 × 6 寄存器）。
    fn fire_count_reads(count_reg10: u16, det_groups: u16) -> BlockReads {
        let mut sys = vec![0u16; 7];
        sys[6] = count_reg10;
        vec![
            (fire_count_block(4, 7), Ok(BlockData::Regs(sys))),
            (
                rblk("fire_det", 17, 6 * det_groups),
                Ok(BlockData::Regs(vec![0u16; 6 * det_groups as usize])),
            ),
        ]
    }

    /// fire 站一次 poll（**§9.4.1 参考配置形态，链首可得**）：`fire_sys` 覆盖寄存器 4..16
    /// （`count: 13`）—— 含探测器 1 的 6 个寄存器（11–16）⇒ **容量含探测器 1（+1）**。
    ///
    /// `det1_addr` 写进块内偏移 7 = 寄存器 11（探测器 1 的地址号，升序链首的载体）。
    fn fire_count_reads_ref(count_reg10: u16, det_groups: u16, det1_addr: u16) -> BlockReads {
        let mut sys = vec![0u16; 13];
        sys[6] = count_reg10; // 偏移 6 = 寄存器 10 = 契约点 `fire_det_count`
        sys[7] = det1_addr; // 偏移 7 = 寄存器 11 = 探测器 1 的地址号
        vec![
            (fire_count_block(4, 13), Ok(BlockData::Regs(sys))),
            (
                rblk("fire_det", 17, 6 * det_groups),
                Ok(BlockData::Regs(vec![0u16; 6 * det_groups as usize])),
            ),
        ]
    }

    /// **消防登记数交叉校验**（PRD §9.5.4"强制" / §11.4.6）：一致 → `None`；不一致 →
    /// `Some(读回登记数)`。查找键 = **点名**（块名无关）。
    ///
    /// 本用例的 fixture **不覆盖寄存器 11** ⇒ 走**降级口径**：容量 = `fire_det` 前缀块
    /// count 之和 ÷ 6（**不加 1**），与地址序校验的降级同口径。
    #[test]
    fn fire_detector_count_mismatch_by_point_name() {
        // 一致：读回 3 == 容量 18/6 = 3（链首不可得 ⇒ 不加 1）
        assert_eq!(
            fire_detector_mismatch(Role::Fire, &fire_count_reads(3, 3)),
            None
        );
        // 不一致：读回 5 != 3 ⇒ 返回读回值（事件诊断量）
        assert_eq!(
            fire_detector_mismatch(Role::Fire, &fire_count_reads(5, 3)),
            Some(5.0)
        );
        // 非 fire 站无此判据（不臆断）
        assert_eq!(
            fire_detector_mismatch(Role::Battery, &fire_count_reads(5, 3)),
            None
        );
        // 无 `fire_det` 前缀块 ⇒ 容量 0；读回 0 ⇒ 一致
        let no_det: BlockReads = vec![(fire_count_block(4, 7), Ok(BlockData::Regs(vec![0u16; 7])))];
        assert_eq!(fire_detector_mismatch(Role::Fire, &no_det), None);
        // 承载登记数的块读失败 ⇒ 无判据可依，不臆断
        let failed: BlockReads = vec![(fire_count_block(4, 7), Err("io".into()))];
        assert_eq!(fire_detector_mismatch(Role::Fire, &failed), None);
    }

    /// **容量式含探测器 1（链首存在 ⇒ +1）** —— "漏 +1" 的回归锚。
    ///
    /// §9.4.1 参考配置形态（n=20：`fire_det.count = 114` ⇒ 19 组；探测器 1 在 `fire_sys`
    /// 的 11–16）⇒ 容量 = `1 + 114/6` = **20**，与寄存器 10 读回 **20** **一致** ⇒ 判据不
    /// 成立、不产事件（PRD §9.5.4 的强制交叉校验由此**恢复有效**）。
    ///
    /// **若把 `+1` 去掉**：容量 = 19 ≠ 读回 20 ⇒ 恒判不一致 ⇒ 永久假告警 ⇒ **本用例必红**
    /// （首次观测 1 条 + 每次站恢复再 1 条，且 RC-5 交叉校验永久失效）。
    #[test]
    fn fire_detector_count_reference_config_includes_detector_one() {
        // 参考配置形态：读回 20 == 容量 20（1 + 114/6）⇒ 一致、不产事件
        assert_eq!(
            fire_detector_mismatch(Role::Fire, &fire_count_reads_ref(20, 19, 1)),
            None,
            "链首存在 ⇒ 容量含探测器 1（1 + 114/6 = 20），与读回 20 一致"
        );
        // 反向：设备报 19 只 ⇒ 19 != 20 ⇒ 不一致（+1 生效的另一面：确能抓到少报）
        assert_eq!(
            fire_detector_mismatch(Role::Fire, &fire_count_reads_ref(19, 19, 1)),
            Some(19.0),
            "读回 19 != 容量 20 ⇒ 判不一致"
        );
        // **降级对照**：同样的探测器区容量，但块**不覆盖寄存器 11**（`fire_sys` count 7）
        // ⇒ 链首不可得 ⇒ 不加 1 ⇒ 容量 19 ≠ 读回 20 ⇒ 判不一致。
        // 语义上这是**期望行为**（"设备报 20 只、配置只采到 19 只"正是本校验要抓的盲区），
        // 而非误报 —— 与地址序校验"链首不可得 ⇒ 只校 fire_det* 区"同口径。
        assert_eq!(
            fire_detector_mismatch(Role::Fire, &fire_count_reads(20, 19)),
            Some(20.0),
            "未覆盖寄存器 11 ⇒ 退化为 Σ/6 = 19（不加 1）"
        );
    }

    /// 钢瓶气压「是否配置」（PRD §9.7.6 / §11.7.3）：取数按**绝对寄存器 5**（块名无关）；
    /// 从未非 0 ⇒ `false`（展示层标"未配置"）；出现过非 0（或本轮非 0）⇒ `true`。
    #[test]
    fn cylinder_pressure_configured_by_absolute_addr() {
        // 块覆盖寄存器 4..6（`uint16` ⇒ 每寄存器 1 点），偏移 1 = 寄存器 5（钢瓶气压）
        let with_pressure = |p: u16| -> BlockReads {
            let mut b = rblk("sys", 4, 3);
            b.format = RegFormat::Uint16;
            b.scale = 1.0; // `blk()` 的 scale 缺省是 0.0（既有夹具口径），此处须为 1.0
            vec![(b, Ok(BlockData::Regs(vec![0, p, 0])))]
        };
        assert!(
            !cylinder_pressure_configured(&with_pressure(0), false),
            "从未非 0 ⇒ 未配置"
        );
        assert!(
            cylinder_pressure_configured(&with_pressure(0), true),
            "曾非 0 ⇒ 已配置（不回退）"
        );
        assert!(
            cylinder_pressure_configured(&with_pressure(123), false),
            "本轮非 0 ⇒ 已配置"
        );
        // 无块覆盖寄存器 5 ⇒ 无判据：未配置
        assert!(!cylinder_pressure_configured(&vec![], false));
        // 覆盖块读失败 ⇒ 同样视作"本轮未见非 0"（记忆不回退由 scheduler 保证）
        let failed: BlockReads = vec![(rblk("sys", 4, 3), Err("io".into()))];
        assert!(!cylinder_pressure_configured(&failed, false));
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
