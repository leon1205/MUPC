//! 块 → 点展开与点位占位（S3b-2 §11.4.3）。
//!
//! **校验期与运行期调用同一函数**（设计 §11.4.3）：防"配置校验通过但运行期展开不同"。
//!
//! 展开规则与 PRD §9.4.2.1 第 4 条、§9.4.2.2 命名规则一一对应：
//!
//! | 情形 | 产出 |
//! |------|------|
//! | `func: discrete` + 无 `points` | `count` 位各 1 点，`metric = <块名>_<k+1>` |
//! | `func: discrete` + 有 `points` | 仅列出的位（`at` + 点级 `count` 展开），同上命名 |
//! | 标量块 + 无 `points` | 窗口内**每个值槽** 1 点（按 `format.reg_width()` 步进），序号 = 低地址寄存器的块内偏移 + 1 |
//! | 标量块 + 有 `points` | 仅列出的点；点级 `format/scale/offset/word_order` 覆盖块级缺省（`None` = 继承） |
//! | 点级 `name` | 覆盖位置命名；**仅允许 `count == 1`**（PRD 未定义"命名序列"，禁用比发明安全） |
//!
//! 展开同时承担规则 7（点位越界）/ 8（点位重叠）/ 9（32 位点对齐）的判定——它们在
//! "把点映射到寄存器"的那一刻才能判，故落在本函数（§11.5.1 的落点列）。

use crate::config::{PointConf, RegBlockConf, RegFunc, Role};
use mupc_data_processing::meter_regs::{RegDecode, WordOrder};

/// 设备单次读的**保守**寄存器上限（PRD §9.5.4 自己的分片口径：BMS ≤120、消防每片 ≤120）。
/// 比 Modbus 标准上限 125 留 5 寄存器余量；规则 15 判据 ④ 与分片口径共用本常量。
pub const MAX_SINGLE_READ_REGS: u16 = 120;
/// 块内未声明寄存器的**连续空洞上限**（PRD §9.4.2.1 第 3 条：补读 `N` 寄存器有 `2N ≤ 8`
/// ⇒ `N ≤ 4`；规则 11 与规则 15 判据 ③ 共用本常量）。
pub const MAX_HOLE_REGS: u16 = 4;
/// `discrete` 位块的位数上限（PRD §9.4.3 规则 12：BMS ≤ 2000 位）。
pub const MAX_DISCRETE_BITS: u16 = 2000;

/// 一个可产出的遥测点（16/32 位标量 或 1 个位）。
#[derive(Debug, Clone, PartialEq)]
pub struct PointSpec {
    /// 点位名（PRD §9.4.2.2 唯一命名规则：显式 `name` 优先，否则 `<块名>_<序号>`）
    pub metric: String,
    /// 点位形态
    pub kind: PointKind,
}

/// 点位形态；块内偏移均为 **0 基**（而配置里的 `at` / 点位名序号均为 1 基）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointKind {
    /// 标量点：块内寄存器偏移 + 解码规格
    Scalar {
        /// 块内寄存器偏移（0 基）
        offset: u16,
        /// 解码规格（已折算块级缺省）
        decode: RegDecode,
    },
    /// 位点：块内位偏移（0 基）
    Bit {
        /// 块内位偏移（0 基）
        offset: u16,
    },
}

impl PointKind {
    /// 块内起始偏移（寄存器 / 位，均 0 基）
    pub fn offset(self) -> u16 {
        match self {
            PointKind::Scalar { offset, .. } | PointKind::Bit { offset } => offset,
        }
    }

    /// 占用的地址单位数（标量 = 寄存器数，位 = 1）
    pub fn width(self) -> u16 {
        match self {
            PointKind::Scalar { decode, .. } => decode.width() as u16,
            PointKind::Bit { .. } => 1,
        }
    }
}

/// 块 → 完整点清单（校验期与运行期**同一函数**）。
pub fn expand(block: &RegBlockConf) -> Result<Vec<PointSpec>, String> {
    if block.func == RegFunc::Discrete {
        expand_discrete(block)
    } else {
        expand_scalar(block)
    }
}

/// 某块点清单的地址占位（校验空洞 / 极大性用）：返回 `[(起始偏移, 长度)]`，
/// 偏移与长度均以**地址单位**计（标量 = 寄存器，`discrete` = 位）。
pub fn footprint(block: &RegBlockConf) -> Result<Vec<(u16, u16)>, String> {
    Ok(expand(block)?
        .into_iter()
        .map(|p| (p.kind.offset(), p.kind.width()))
        .collect())
}

/// **字级信号**：把某块的整字读数按 `point_table::SignalSpec` 求值为"活跃/非活跃"
///（设计 §11.4.3 / §11.4.7.1）。返回 `(<点名>@<信号键>, 是否活跃)`；`scheduler` 用它喂
/// [`crate::scheduler::EdgeTracker`]。
///
/// **只读整字、不改写遥测值**；未登记信号的点**不产出**（未入表 = 只落 telemetry）；
/// 无信号的块 → 空 `Vec`（绝大多数块如此）。
///
/// 口径与边界：
/// - 逐点按 **`点内偏移`** 取该点所在的那个寄存器（`字号 = block.addr + offset`），
///   再按 `(role, 字号)` 查登记信号 —— 与 `point_table` 的寄存器空间查表键一致；
/// - 消防全部状态点均为 **16 位**（`format: uint16`，宽 1 寄存器）⇒ 一个点 = 一个整字；
///   32 位点本轮**没有**登记信号（不引入"跨字位图"这一无定义形态）；
/// - 块读失败（`Err`）不传入本函数（调用方只对 `Ok` 的寄存器块求值）。
pub fn signals_of_block(role: Role, block: &RegBlockConf, regs: &[u16]) -> Vec<(String, bool)> {
    let Ok(pts) = expand(block) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for p in pts {
        // 位点无字级信号（位点走 EdgeTracker 的另一条臂）；只处理标量点
        let PointKind::Scalar { offset, .. } = p.kind else {
            continue;
        };
        let specs = crate::point_table::signals_of(role, block.addr.wrapping_add(offset));
        if specs.is_empty() {
            continue;
        }
        let Some(&word) = regs.get(offset as usize) else {
            continue; // 读回长度不足（防御；正常路径由块读长度保证）
        };
        for s in specs {
            out.push((format!("{}@{}", p.metric, s.key), s.pick.is_active(word)));
        }
    }
    out
}

/// 位置式点名（PRD §9.4.2.2）：`<块名>_<序号>`，序号 = 块内偏移 + 1。
fn positional(block_name: &str, offset: u16) -> String {
    format!("{}_{}", block_name, offset + 1)
}

/// 点级 `count` / `at` 的共用守卫（0 会静默不产点 / 落到块外）。
fn point_base(block: &RegBlockConf, p: &PointConf) -> Result<(u32, u16), String> {
    if p.count == 0 {
        return Err(format!(
            "块 {} 点级 count 须 ≥ 1（0 会静默不产点）",
            block.name
        ));
    }
    if p.at == 0 {
        return Err(format!(
            "块 {} 点级 at 须 ≥ 1（1 起，= 块内偏移 + 1）",
            block.name
        ));
    }
    Ok(((p.at - 1) as u32, p.count))
}

/// 显式 `name` 与 `count > 1` 并存 → 配置期拒（PRD §9.4.2.2 只定义"点名 ↔ 序号"的单点
/// 形态，未定义"命名序列"；设计 §11.5.2(4) 的保守口径）。
fn check_name_count(block: &RegBlockConf, p: &PointConf) -> Result<(), String> {
    if p.name.is_some() && p.count > 1 {
        return Err(format!(
            "块 {} 点级 name 与 count={} 并存：PRD §9.4.2.2 未定义命名序列（配置期拒）",
            block.name, p.count
        ));
    }
    Ok(())
}

/// 同块内两点覆盖同一寄存器 / 位 → 拒（规则 8；32 位点占 2 寄存器，与相邻 16 位点重叠也算）。
fn check_overlap(block: &RegBlockConf, spans: &[(u16, u16, String)]) -> Result<(), String> {
    let mut sorted: Vec<&(u16, u16, String)> = spans.iter().collect();
    sorted.sort_by_key(|s| s.0);
    for w in sorted.windows(2) {
        let (a_start, a_len, a_metric) = &w[0];
        let (b_start, b_len, b_metric) = &w[1];
        if *b_start < a_start + a_len {
            return Err(format!(
                "块 {} 点位重叠：{}（偏移 {}+{}）与 {}（偏移 {}+{}）覆盖同一地址",
                block.name, a_metric, a_start, a_len, b_metric, b_start, b_len
            ));
        }
    }
    Ok(())
}

/// 标量块展开（`holding` / `input`）。
fn expand_scalar(block: &RegBlockConf) -> Result<Vec<PointSpec>, String> {
    let mut out: Vec<PointSpec> = Vec::new();
    let mut spans: Vec<(u16, u16, String)> = Vec::new();

    // 无 `points`：窗口内每个值槽 1 点（按宽度步进；rule 19 已在配置期拦住非整倍数的 count）
    if block.points.is_empty() {
        let width = block.format.reg_width() as u16;
        let decode = RegDecode {
            format: block.format,
            scale: block.scale,
            offset: block.offset,
            word_order: WordOrder::HiLo,
            byte_swap: block.byte_swap,
        };
        let mut off: u16 = 0;
        while off + width <= block.count {
            let metric = positional(&block.name, off);
            out.push(PointSpec {
                metric: metric.clone(),
                kind: PointKind::Scalar { offset: off, decode },
            });
            spans.push((off, width, metric));
            off += width;
        }
        return Ok(out);
    }

    for p in &block.points {
        check_name_count(block, p)?;
        let (base, count) = point_base(block, p)?;
        let decode = RegDecode {
            format: p.format.unwrap_or(block.format),
            scale: p.scale.unwrap_or(block.scale),
            offset: p.offset.unwrap_or(block.offset),
            word_order: p.word_order,
            byte_swap: block.byte_swap,
        };
        let width = decode.width() as u32;
        // 规则 9：32 位点必须 `count == 1`（多值 32 位点无定义）
        if width == 2 && count != 1 {
            return Err(format!(
                "块 {} 点 at={} 为 32 位格式（占 2 寄存器），点级 count 须为 1（实为 {}）",
                block.name, p.at, count
            ));
        }
        for j in 0..count as u32 {
            let off32 = base + j * width;
            let end32 = off32 + width;
            // 规则 7（越界）+ 规则 9（32 位点跨窗口末尾）合并判：覆盖区间须整体落在块内
            if end32 > block.count as u32 {
                return Err(format!(
                    "块 {} 点{} 点位越界：at={} 起算覆盖块内偏移 {}..{}，超出 count={}（32 位点须整值落在窗口内）",
                    block.name,
                    match &p.name {
                        Some(n) => format!("（name={n}）"),
                        None => String::new(),
                    },
                    p.at,
                    off32,
                    end32,
                    block.count
                ));
            }
            let off = off32 as u16;
            let metric = match &p.name {
                Some(n) => n.clone(),
                None => positional(&block.name, off),
            };
            out.push(PointSpec {
                metric: metric.clone(),
                kind: PointKind::Scalar { offset: off, decode },
            });
            spans.push((off, width as u16, metric));
        }
    }
    check_overlap(block, &spans)?;
    Ok(out)
}

/// 位块（`discrete`）展开：`count` = **位数**，`addr` = **位地址**。
fn expand_discrete(block: &RegBlockConf) -> Result<Vec<PointSpec>, String> {
    let mut out: Vec<PointSpec> = Vec::new();
    let mut spans: Vec<(u16, u16, String)> = Vec::new();

    if block.points.is_empty() {
        for k in 0..block.count {
            let metric = positional(&block.name, k);
            out.push(PointSpec {
                metric: metric.clone(),
                kind: PointKind::Bit { offset: k },
            });
            spans.push((k, 1, metric));
        }
        return Ok(out);
    }

    for p in &block.points {
        check_name_count(block, p)?;
        let (base, count) = point_base(block, p)?;
        for j in 0..count as u32 {
            let off32 = base + j;
            if off32 >= block.count as u32 {
                return Err(format!(
                    "块 {} 位点越界：at={} + {} 超出位数 count={}",
                    block.name, p.at, j, block.count
                ));
            }
            let off = off32 as u16;
            let metric = match &p.name {
                Some(n) => n.clone(),
                None => positional(&block.name, off),
            };
            out.push(PointSpec {
                metric: metric.clone(),
                kind: PointKind::Bit { offset: off },
            });
            spans.push((off, 1, metric));
        }
    }
    check_overlap(block, &spans)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PointConf, RegFunc};
    use mupc_data_processing::meter_regs::RegFormat;

    fn blk(name: &str, addr: u16, count: u16) -> RegBlockConf {
        RegBlockConf {
            name: name.into(),
            addr,
            func: RegFunc::Holding,
            format: RegFormat::Uint16,
            scale: 1.0,
            count,
            offset: 0.0,
            byte_swap: false,
            points: Vec::new(),
            read_slice: false,
            interval_ms: None,
        }
    }

    fn pt(at: u16, count: u16, name: Option<&str>) -> PointConf {
        PointConf {
            at,
            count,
            name: name.map(|s| s.to_string()),
            format: None,
            scale: None,
            offset: None,
            word_order: WordOrder::HiLo,
        }
    }

    fn metrics(pts: &[PointSpec]) -> Vec<&str> {
        pts.iter().map(|p| p.metric.as_str()).collect()
    }

    /// 无 `points` 的 16 位块：每寄存器 1 点，序号 = 偏移 + 1
    #[test]
    fn scalar_block_without_points_is_every_value_slot() {
        let b = blk("bms_term", 2991, 4);
        let pts = expand(&b).unwrap();
        assert_eq!(metrics(&pts), ["bms_term_1", "bms_term_2", "bms_term_3", "bms_term_4"]);
        assert_eq!(pts[0].kind.width(), 1);
    }

    /// 无 `points` 的 32 位块：每 2 寄存器 1 点（AC-6 ⑦：count 6 → 3 点）
    #[test]
    fn scalar_block_without_points_steps_by_width() {
        let mut b = blk("p", 0x1000, 6);
        b.format = RegFormat::Int32Scaled;
        b.scale = 0.01;
        let pts = expand(&b).unwrap();
        assert_eq!(metrics(&pts), ["p_1", "p_3", "p_5"], "序号锚定低地址寄存器偏移 + 1");
    }

    /// 有 `points`：只产出列出的点（bms_meta 的 8 条 → 8 点，188 不产）
    #[test]
    fn scalar_block_with_points_produces_listed_only() {
        let mut b = blk("bms_meta", 181, 9);
        b.points = vec![pt(1, 1, None), pt(9, 1, None)];
        let pts = expand(&b).unwrap();
        assert_eq!(metrics(&pts), ["bms_meta_1", "bms_meta_9"]);
    }

    /// 点级 `count = N`：连续产出 N 点（同换算、地址递增）
    #[test]
    fn point_count_expands_consecutive_points() {
        let mut b = blk("bms_io", 100, 31);
        b.points = vec![pt(8, 8, None)];
        let pts = expand(&b).unwrap();
        assert_eq!(
            metrics(&pts),
            ["bms_io_8", "bms_io_9", "bms_io_10", "bms_io_11", "bms_io_12", "bms_io_13", "bms_io_14", "bms_io_15"]
        );
    }

    /// 点级 `name` 覆盖位置命名（`soc` 契约点）
    #[test]
    fn point_name_overrides_positional() {
        let mut b = blk("bms_io", 100, 31);
        b.points = vec![pt(19, 1, Some("soc"))];
        let pts = expand(&b).unwrap();
        assert_eq!(metrics(&pts), ["soc"]);
        assert_eq!(pts[0].kind.offset(), 18, "at 1 起 → 块内偏移 18（寄存器 118）");
    }

    /// 32 位点占 2 寄存器、产 1 点，且 `count` 必须 1
    #[test]
    fn scalar_32bit_point_occupies_two_registers_count_must_be_one() {
        let mut b = blk("pcs_3zone", 1000, 4);
        b.points = vec![PointConf {
            at: 1,
            count: 1,
            name: None,
            format: Some(RegFormat::Int32Scaled),
            scale: Some(0.1),
            offset: None,
            word_order: WordOrder::LoHi,
        }];
        let pts = expand(&b).unwrap();
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].kind.width(), 2);
        let mut bad = b.clone();
        bad.points[0].count = 2;
        assert!(expand(&bad).is_err(), "32 位点 count != 1 应拒");
    }

    /// 规则 7：点位越界 → Err（含块名 / at）
    #[test]
    fn point_out_of_window_rejected() {
        let mut b = blk("x", 100, 4);
        b.points = vec![pt(5, 1, None)];
        let e = expand(&b).unwrap_err();
        assert!(e.contains("x") && e.contains("越界"), "实际: {e}");
    }

    /// 规则 7/9：32 位点跨窗口末尾（half value）→ Err
    #[test]
    fn scalar_32bit_point_crossing_window_end_rejected() {
        let mut b = blk("x", 100, 4);
        b.points = vec![PointConf {
            at: 4,
            count: 1,
            name: None,
            format: Some(RegFormat::Int32Scaled),
            scale: Some(1.0),
            offset: None,
            word_order: WordOrder::HiLo,
        }];
        assert!(expand(&b).is_err(), "32 位点跨窗口末尾应拒");
    }

    /// 规则 8：同块内点位重叠 → Err（32 位点与相邻 16 位点重叠亦算）
    #[test]
    fn overlapping_points_rejected() {
        let mut b = blk("x", 100, 4);
        b.points = vec![PointConf {
            at: 1, count: 1, name: None,
            format: Some(RegFormat::Int32Scaled), scale: Some(1.0), offset: None,
            word_order: WordOrder::HiLo,
        }, pt(2, 1, None)];
        let e = expand(&b).unwrap_err();
        assert!(e.contains("点位重叠") && e.contains("x"), "实际: {e}");
    }

    /// 点级 `name` + `count > 1` → 配置期拒（设计补充口径）
    #[test]
    fn point_name_with_multi_count_rejected() {
        let mut b = blk("x", 100, 4);
        b.points = vec![pt(1, 2, Some("soc"))];
        assert!(expand(&b).is_err());
    }

    /// 点级 `count: 0` / `at: 0` → 拒（0 会静默不产点 / 落到块外）
    #[test]
    fn point_zero_count_or_at_rejected() {
        let mut b = blk("x", 100, 4);
        b.points = vec![pt(1, 0, None)];
        assert!(expand(&b).is_err());
        b.points = vec![pt(0, 1, None)];
        assert!(expand(&b).is_err());
    }

    /// 位块无 `points`：每位 1 点，序号 = 位偏移 + 1
    #[test]
    fn discrete_block_without_points_is_every_bit() {
        let mut b = blk("hvac_di", 0, 31);
        b.func = RegFunc::Discrete;
        b.format = RegFormat::Float32; // discrete 块的 format/scale 不参与（位恒 0/1）
        let pts = expand(&b).unwrap();
        assert_eq!(pts.len(), 31);
        assert_eq!(pts[0].metric, "hvac_di_1");
        assert_eq!(pts[30].metric, "hvac_di_31");
        assert!(matches!(pts[0].kind, PointKind::Bit { offset: 0 }));
    }

    /// 位块有 `points`：仅列出的位；越界拒
    #[test]
    fn discrete_block_with_points_lists_bits_only() {
        let mut b = blk("d", 200, 4);
        b.func = RegFunc::Discrete;
        b.points = vec![pt(2, 2, None)];
        let pts = expand(&b).unwrap();
        assert_eq!(metrics(&pts), ["d_2", "d_3"]);
        b.points = vec![pt(4, 2, None)];
        assert!(expand(&b).is_err());
    }

    /// footprint：与 expand 同源的 `(偏移, 长度)`
    #[test]
    fn footprint_matches_expand() {
        let mut b = blk("pcs_3zone", 1000, 4);
        b.points = vec![PointConf {
            at: 1, count: 1, name: None,
            format: Some(RegFormat::Int32Scaled), scale: Some(0.1), offset: None,
            word_order: WordOrder::LoHi,
        }, pt(3, 2, None)];
        assert_eq!(footprint(&b).unwrap(), vec![(0, 2), (2, 1), (3, 1)]);
    }
}
