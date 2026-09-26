//! S3b-2 **T4** —— 点表 `POINT_REGS` ↔ 参考配置的**双向漂移核对**（设计 §11.4.4 末、
//! 风险 R-2；PRD §9.4.3 明文要求"点表登记值常量表"）。
//!
//! **为什么必须有它**（§11.4.4）：`POINT_REGS`（510 静态行 + 探测器区 6 条模板）与
//! 参考配置（= PRD §9.4.1 的 YAML；**Task 6 起为两段**：`south_stations_s3b2.yaml`
//! 5 站 546 点 + `south_pcs_s3b2.yaml` 72 点）是**同一份点表的两次转录**，
//! 任一侧单独改动都会造成"校验器拿旧表判新配置"的漂移。本用例把两侧逐行对齐：
//!
//! 1. **展开侧 → 表**：配置的每一块经 `points::expand`（**校验期与运行期同一函数**）
//!    展开后，**每个点**都必须在表内命中一行，且行的 `kind`（格式 / 位）、`scale`、
//!    `offset` 与该点解出的 `RegDecode` **逐字段相等**；
//! 2. **表 → 展开侧**：`POINT_REGS` 的**每一行**都必须被展开结果**恰好命中一次**
//!    （防"表里多了一行无人引用"的孤儿登记，也防同一地址重复登记）；
//! 3. **规模**：展开后总点数 == **618**，且 5 个设备逐一对上 PRD §9.8.3 的分设备点数；
//! 4. **`label` 反查**：每个展开点的点名都能经 `point_table::label` 反查到**同一行**的
//!    中文名（即"位置式命名 ↔ 块布局表"一致）。
//!
//! **本用例真能抓到的错**（举例，均为转写高发错误）：登记行地址写错一个 ⇒ ①命中不到
//! 而报"缺行"；两行撞同一地址 ⇒ ③的重复登记断言或②的"恰好一次"断言失败；`scale`
//! 抄成 0.01 而配置是 0.1 ⇒ ①的等值断言失败；`format` 把 `uint16` 抄成 `int16` ⇒
//! ①的 `kind` 断言失败；整行漏抄 ⇒ ①报缺行且②报未命中。

use mupc_data_processing::meter_regs::RegFormat;
use mupc_southd::config::{RegBlockConf, RegFunc, Role, SouthPcsConfig, SouthStationsConfig};
use mupc_southd::point_table::{self, AddrSpace, RegPointKind, SymSrc};
use mupc_southd::points::{self, PointKind};
use serde::Deserialize;

#[derive(Deserialize)]
struct Wrapper {
    south_stations: SouthStationsConfig,
}

/// PRD §9.4.1 的参考配置——**站级段**（Task 6 起 5 站 546 点；PCS 已迁至 `south_pcs`）
const REF: &str = include_str!("fixtures/south_stations_s3b2.yaml");

/// PCS 独立顶层段（Task 6 / ADR-016；72 点）—— 与站级段**同一份点表的另一次转录**
const REF_PCS: &str = include_str!("fixtures/south_pcs_s3b2.yaml");

fn stations() -> SouthStationsConfig {
    serde_yaml::from_str::<Wrapper>(REF)
        .expect("参考配置解析失败")
        .south_stations
}

/// `south_pcs` 段（裸结构，无外层键——见该 fixture 头部注释）
fn pcs_segment() -> SouthPcsConfig {
    serde_yaml::from_str::<SouthPcsConfig>(REF_PCS).expect("south_pcs 参考配置解析失败")
}

/// PRD §9.8.3 / §9.5 的分设备点数（n=20 展开后）
const EXPECTED_POINTS: &[(Role, usize)] = &[
    (Role::Battery, 345),
    (Role::Pcs, 72),
    (Role::MeterBatt, 40),
    (Role::Fire, 127),
    (Role::Hvac, 34),
];

/// 参考配置中**不产出任何点**的站（既有 `grid_meter` 走 `decode_phase_block`，不经点展开）
fn is_collected(role: Role) -> bool {
    role != Role::MeterGrid
}

/// 展开配置得到的 `(role, 点名, 地址, 配置侧形态, scale, offset, 块名)` 全清单。
///
/// `fmt = None` 表示位点（配置里没有"位分类"信息 —— 分类是点表独有的事实，由点表侧断言）。
#[derive(Debug)]
struct Expanded {
    role: Role,
    metric: String,
    space: AddrSpace,
    addr: u16,
    fmt: Option<RegFormat>,
    scale: f64,
    offset: f64,
    block: String,
}

impl Expanded {
    fn row(&self) -> &'static point_table::PointReg {
        point_table::lookup_in(self.role, self.space, self.addr).unwrap_or_else(|| {
            panic!(
                "站 {:?} 块 {} 的点 {}（{:?} addr={}）在 POINT_REGS 中**无登记行**——转写缺失或地址抄错",
                self.role, self.block, self.metric, self.space, self.addr
            )
        })
    }

    fn key(&self) -> (Role, AddrSpace, u16) {
        (self.role, self.space, self.addr)
    }
}

/// 把一批块经 `points::expand` 展开为 `Expanded`（**站级段与 `south_pcs` 段共用**，
/// 保证两段走的是同一条展开路径）。
fn expand_blocks(owner: &str, role: Role, regs: &[RegBlockConf], out: &mut Vec<Expanded>) {
    for blk in regs {
        let pts =
            points::expand(blk).unwrap_or_else(|e| panic!("{owner} 块 {} 展开失败: {e}", blk.name));
        for p in pts {
            let (fmt, scale, offset) = match p.kind {
                PointKind::Scalar { decode, .. } => {
                    (Some(decode.format), decode.scale, decode.offset)
                }
                PointKind::Bit { .. } => (None, 0.0, 0.0),
            };
            let space = match blk.func {
                RegFunc::Discrete => AddrSpace::Bit,
                RegFunc::Holding | RegFunc::Input => AddrSpace::Reg,
            };
            out.push(Expanded {
                role,
                metric: p.metric,
                space,
                addr: blk.addr + p.kind.offset(),
                fmt,
                scale,
                offset,
                block: blk.name.clone(),
            });
        }
    }
}

/// 两段合并展开：`south_stations`（5 站 546 点）+ `south_pcs`（3 区 72 点）= **618**。
fn expand_reference() -> Vec<Expanded> {
    let mut out = Vec::new();
    for st in &stations().stations {
        if !is_collected(st.role) {
            // 既有 `grid_meter` 站不经点展开（走 `decode_phase_block`），其块必须无 `points`
            assert!(
                !st.regs.iter().any(|b| !b.points.is_empty()),
                "站 {} 不在采集范围却有逐点声明",
                st.id
            );
            continue;
        }
        expand_blocks(&format!("站 {}", st.id), st.role, &st.regs, &mut out);
    }
    // PCS 段（Task 6 / ADR-016）：独立顶层段，但点表角色仍是 `Role::Pcs`
    let pcs = pcs_segment();
    expand_blocks("south_pcs", Role::Pcs, &pcs.regs, &mut out);
    out
}

/// ① 展开侧 → 表：逐点命中且 `kind`（格式 / 位）、`scale`、`offset` 逐字段相等
#[test]
fn every_expanded_point_hits_registry_row_with_same_kind_scale_offset() {
    let pts = expand_reference();
    assert!(!pts.is_empty(), "参考配置应展开出点位");
    for p in &pts {
        let row = p.row();
        match (p.fmt, row.kind) {
            (Some(fmt), RegPointKind::Scalar(rfmt)) => {
                assert_eq!(
                    fmt, rfmt,
                    "点 {}（addr={}）格式不符：配置解出 {:?}，点表登记 {:?}",
                    p.metric, p.addr, fmt, rfmt
                );
                assert_eq!(
                    p.scale, row.scale,
                    "点 {}（addr={}）scale 漂移：配置 {} vs 点表 {}",
                    p.metric, p.addr, p.scale, row.scale
                );
                assert_eq!(
                    p.offset, row.offset,
                    "点 {}（addr={}）offset 漂移（规则 6 ② 的期望值）：配置 {} vs 点表 {}",
                    p.metric, p.addr, p.offset, row.offset
                );
            }
            (None, RegPointKind::Bit(_)) => {}
            (f, k) => panic!(
                "点 {}（addr={}）形态不符：配置 {:?} vs 点表 {:?}",
                p.metric, p.addr, f, k
            ),
        }
    }
}

/// ② 表 → 展开侧：每一登记行都被**恰好命中一次**
#[test]
fn every_registry_row_is_claimed_exactly_once() {
    let pts = expand_reference();
    let mut claimed: Vec<(Role, AddrSpace, u16)> = Vec::new();
    for p in &pts {
        let key = p.key();
        assert!(
            !claimed.contains(&key),
            "地址 {:?} {}(role {:?}) 被展开出**两次**（点名重复或块区间重叠）",
            key.1,
            key.2,
            key.0
        );
        claimed.push(key);
    }
    for row in point_table::POINT_REGS {
        let key = (row.role, row.kind.space(), row.addr);
        assert!(
            claimed.contains(&key),
            "点表登记行 {:?}/{:?} addr={}（{}）**未被参考配置的任何点命中**——孤儿登记或地址抄错",
            row.role,
            row.kind.space(),
            row.addr,
            row.label
        );
    }
    // 108 的推导：探测器区在 n=20 下展开 **114 点**（19 只 × 6 寄存器），而静态表只登记
    // **6** 条组内语义模板 ⇒ 静态表比"逐点登记"少 `114 − 6 = 108` 行。
    assert_eq!(
        point_table::POINT_REGS.len() + 108,
        618,
        "静态行数须 = 618 − (探测器区 114 点 − 6 条模板)"
    );
}

/// ③ 规模：展开后总计 618 点，且分设备点数与 PRD §9.8.3 一致
#[test]
fn expanded_point_counts_match_prd() {
    let pts = expand_reference();
    for (role, want) in EXPECTED_POINTS {
        let got = pts.iter().filter(|p| p.role == *role).count();
        assert_eq!(got, *want, "设备 {role:?} 点数不符（PRD §9.8.3）");
    }
    assert_eq!(
        pts.len(),
        618,
        "全站点数须 == 618（PRD §9.8.3；探测器区按 n=20 展开）"
    );
    // **段级口径**（Task 6 / 设计 §13.7）：618 的归属由"6 站"变为
    // 「`south_stations` 5 站 = **546** + `south_pcs` = **72**」，总数不变。
    // 两段分别展开再相加 —— 防"合计对得上但归属错"（如 PCS 72 点被误搬到站级段）。
    let mut st_part = Vec::new();
    for st in &stations().stations {
        if is_collected(st.role) {
            expand_blocks(&format!("站 {}", st.id), st.role, &st.regs, &mut st_part);
        }
    }
    let mut pcs_part = Vec::new();
    expand_blocks("south_pcs", Role::Pcs, &pcs_segment().regs, &mut pcs_part);
    assert_eq!(
        st_part.len(),
        546,
        "south_stations 段展开 546 点（设计 §13.7）"
    );
    assert_eq!(pcs_part.len(), 72, "south_pcs 段展开 72 点（设计 §13.7）");
    assert_eq!(
        st_part.len() + pcs_part.len(),
        pts.len(),
        "两段之和须 == 全量展开（否则合并侧对数口径与分写侧不一致）"
    );
    // 第 20 只（n=20 的最后一只）= (20−1)*6+11 = 125 ⇒ 末寄存器 130，须可由模板命中
    assert!(
        point_table::lookup(Role::Fire, 130).is_some(),
        "探测器区第 20 只的末寄存器须可由模板命中"
    );
}

/// ④ `label` 反查：每个展开点的点名都能反查到**同一行**的中文名
#[test]
fn label_resolves_every_expanded_metric_to_same_row() {
    for p in &expand_reference() {
        let lbl = point_table::label(p.role, &p.metric).unwrap_or_else(|| {
            panic!(
                "点名 {}（站 {:?} 块 {}）无法经 point_table::label 反查——BLOCK_SPANS 与配置漂移",
                p.metric, p.role, p.block
            )
        });
        assert_eq!(
            lbl,
            p.row().label,
            "点名 {} 反查到的中文名与 {:?} addr={} 的登记行不符",
            p.metric,
            p.space,
            p.addr
        );
        assert!(!lbl.is_empty(), "点名 {} 的中文名不得为空", p.metric);
    }
}

/// 表自身的内部一致性（与配置无关的护栏）：
/// ① 无重复 `(role, space, addr)`；② 标量行 `offset ≠ 0` 必登记 `sym_src`
///（否则规则 6 ① 会拒掉**合法**配置——该形态只能是表自身的转录错误，§11.4.4）；
/// ③ 位行不携带 scale/offset/sym_src；④ 标签非空。
#[test]
fn registry_internal_invariants() {
    let mut seen: Vec<(Role, AddrSpace, u16)> = Vec::new();
    for row in point_table::POINT_REGS {
        assert!(
            !seen.contains(&(row.role, row.kind.space(), row.addr)),
            "重复登记 {:?}/{:?}/{}（查表结果不确定）",
            row.role,
            row.kind.space(),
            row.addr
        );
        seen.push((row.role, row.kind.space(), row.addr));
        assert!(
            !row.label.is_empty(),
            "登记行 {:?}/{} 缺中文名",
            row.role,
            row.addr
        );
        match row.kind {
            RegPointKind::Scalar(_) => {
                if row.offset != 0.0 {
                    assert!(
                        row.sym_src.is_some(),
                        "登记行 {:?}/{}（{}）offset={} 却未登记符号性来源——规则 6 ① 会拒掉合法配置",
                        row.role,
                        row.addr,
                        row.label,
                        row.offset
                    );
                }
            }
            RegPointKind::Bit(_) => {
                assert_eq!(
                    row.scale, 0.0,
                    "位行 {:?}/{} 不应带 scale",
                    row.role, row.addr
                );
                assert_eq!(
                    row.offset, 0.0,
                    "位行 {:?}/{} 不应带 offset",
                    row.role, row.addr
                );
                assert!(row.sym_src.is_none(), "位行不适用符号性");
            }
        }
    }
    assert_eq!(seen.len(), 510, "静态登记行数须为 510（618 − 108）");
}

/// **BMS 全部 `offset ≠ 0` 的点逐条核对**（残余风险最高的一族：无符号编码 + 负偏移，
/// 抄错一个就整族量值偏 40 或 1600；Q-20 现场判别的**登记侧**依据）。
#[test]
fn bms_nonzero_offset_rows_match_prd() {
    let want: &[(u16, f64, SymSrc, &str)] = &[
        (116, -1600.0, SymSrc::VendorTypo, "簇组电流"),
        (117, -40.0, SymSrc::VendorTypo, "簇组模块温度"),
        (122, -40.0, SymSrc::VendorTypo, "簇平均单体温度"),
        (127, -40.0, SymSrc::VendorTypo, "簇最高单体温度"),
        (129, -40.0, SymSrc::VendorTypo, "簇最低单体温度"),
        (155, -40.0, SymSrc::VendorTypo, "簇最高单体极柱温度"),
        (157, -40.0, SymSrc::VendorTypo, "簇最低单体极柱温度"),
        (2991, -40.0, SymSrc::VendorTypo, "簇端子温度 001"),
        (2992, -40.0, SymSrc::VendorTypo, "簇端子温度 002"),
        (2993, -40.0, SymSrc::VendorTypo, "簇端子温度 003"),
        (2994, -40.0, SymSrc::VendorTypo, "簇端子温度 004"),
    ];
    let mut got: Vec<(u16, f64)> = Vec::new();
    for row in point_table::POINT_REGS {
        if row.role == Role::Battery && row.offset != 0.0 {
            got.push((row.addr, row.offset));
            assert_eq!(row.sym_src, Some(SymSrc::VendorTypo), "addr={}", row.addr);
        }
    }
    let mut want_pairs: Vec<(u16, f64)> = want.iter().map(|w| (w.0, w.1)).collect();
    got.sort_by_key(|g| g.0);
    want_pairs.sort_by_key(|w| w.0);
    assert_eq!(
        got, want_pairs,
        "BMS 全表非零 offset 的行集合须与 PRD §9.5.1 一致"
    );
    for (addr, _, _, name) in want {
        let row = point_table::lookup(Role::Battery, *addr).expect("已在上表断言");
        assert!(
            row.label.contains(name),
            "addr={addr} 的中文名应含 {name:?}，实际 {:?}",
            row.label
        );
    }
}

/// 位点分类：`Alarm`（产事件）/ `State` / `Reserved`（只落 telemetry）的**边界逐条钉**。
///
/// `Reserved` 区段直接决定"保留位不产事件"（PRD §9.7.6），是 D2 落库口径的输入。
#[test]
fn bit_classes_match_prd_reserved_regions() {
    use point_table::BitClass::*;
    // BMS：364–423 定制保留 / 484–487 保留 / 200、292、297 单点保留
    for a in 364..=423 {
        assert_eq!(class_of(Role::Battery, a), Reserved, "位 {a} 应为保留");
    }
    for a in 484..=487 {
        assert_eq!(class_of(Role::Battery, a), Reserved, "位 {a} 应为保留");
    }
    assert_eq!(class_of(Role::Battery, 200), Reserved, "位 200 标注为保留");
    assert_eq!(class_of(Role::Battery, 292), Reserved, "位 292 标注为保留");
    assert_eq!(class_of(Role::Battery, 297), Reserved, "位 297 标注为保留");
    // BMS：一级/二级/三级告警必须采集且为告警（设计 §11.7.2 举例锚点）
    for a in 424..=426 {
        assert_eq!(class_of(Role::Battery, a), Alarm, "位 {a} 为分级告警");
    }
    // BMS：运行态标识为 State（不产事件）
    for a in [288u16, 291, 293, 295, 296] {
        assert_eq!(class_of(Role::Battery, a), State, "位 {a} 为运行态");
    }
    // 空调：位 25 保留；位 0–8 与 30 为状态；其余告警
    assert_eq!(class_of(Role::Hvac, 25), Reserved);
    for a in [0u16, 1, 7, 8, 30] {
        assert_eq!(class_of(Role::Hvac, a), State, "位 {a} 为输出/状态量");
    }
    for a in [9u16, 10, 12, 21, 23, 26, 28, 29] {
        assert_eq!(class_of(Role::Hvac, a), Alarm, "位 {a} 为告警/故障");
    }
}

/// 位空间查分类（**必须走 `lookup_bit`**：`hvac` 的位 0/2/3 与寄存器 0/2/3 同址，走寄存器
/// 空间会取到标量行 —— 这正是"登记表须按 `(role, space, addr)` 索引"的活例子）。
fn class_of(role: Role, addr: u16) -> point_table::BitClass {
    match point_table::lookup_bit(role, addr)
        .unwrap_or_else(|| panic!("{role:?}/位 {addr} 无登记"))
        .kind
    {
        RegPointKind::Bit(c) => c,
        other => panic!("{role:?}/位 {addr} 应为位点，实际 {other:?}"),
    }
}

// ═══════════════ AC-6：点展开（设计 §11.11.2 的 ①–⑦，用参考配置逐条钉住）═══════════════

/// AC-6 全条 —— **点产出与位置式命名**（T4 的 DoD 之一）。
#[test]
fn ac6_point_production_matches_design() {
    let pts = expand_reference();
    let metrics_of = |block: &str| -> Vec<String> {
        pts.iter()
            .filter(|p| p.block == block)
            .map(|p| p.metric.clone())
            .collect()
    };

    // ① 无 `points` 块按"每值槽 1 点"：bms_term → 4 点、bms_alarm → 288 点
    assert_eq!(
        metrics_of("bms_term"),
        ["bms_term_1", "bms_term_2", "bms_term_3", "bms_term_4"]
    );
    assert_eq!(metrics_of("bms_alarm").len(), 288);
    assert_eq!(
        metrics_of("bms_alarm")[0],
        "bms_alarm_1",
        "位地址 200 ⇒ 序号 1"
    );
    assert_eq!(
        metrics_of("bms_alarm")[287],
        "bms_alarm_288",
        "位地址 487 ⇒ 序号 288"
    );

    // ② 有 `points` 块**只产列出的点**：bms_meta 8 点，188（at 9 之外的 at 8）不产
    let meta = metrics_of("bms_meta");
    assert_eq!(meta.len(), 8, "bms_meta 声明 8 点，实际 {meta:?}");
    assert!(meta.contains(&"bms_meta_9".to_string()), "189 在列");
    assert!(
        !meta.contains(&"bms_meta_8".to_string()),
        "188 未声明 ⇒ 不产"
    );

    // ③ 点级 `count: N` 连续产出：bms_io `at: 8, count: 8` → bms_io_8..15
    let io = metrics_of("bms_io");
    let seg: Vec<&String> = io.iter().filter(|m| m.starts_with("bms_io_")).collect();
    assert!(io.contains(&"bms_io_8".to_string()) && io.contains(&"bms_io_15".to_string()));
    assert_eq!(io.len(), 31, "bms_io 恰 31 点（31 个寄存器各 1 点）");
    assert_eq!(
        seg.len(),
        30,
        "其余点取位置式命名（at 19 被显式 `soc` 覆盖）"
    );

    // ④ 点级 `name` 覆盖位置命名：`soc`（bms_io at 19）、`fire_det_count`（fire_sys at 7）
    let soc = pts
        .iter()
        .find(|p| p.metric == "soc")
        .expect("`soc` 契约点须被产出");
    assert_eq!(soc.addr, 118, "`soc` = 寄存器 118");
    assert!(
        !io.contains(&"bms_io_19".to_string()),
        "显式 name 覆盖后不再产出位置名"
    );
    assert!(pts
        .iter()
        .any(|p| p.metric == "fire_det_count" && p.addr == 10));
    assert!(!metrics_of("fire_sys").contains(&"fire_sys_7".to_string()));

    // ⑤ 32 位点占 2 寄存器、产 **1** 点：pcs_3zone 76 寄存器 → 72 点；`pcs_3zone_43` = 1042–1043
    let pcs = metrics_of("pcs_3zone");
    assert_eq!(pcs.len(), 72, "76 寄存器 − 4 个高半字 = 72 点");
    let p43 = pts
        .iter()
        .find(|p| p.metric == "pcs_3zone_43")
        .expect("1042 的点");
    assert_eq!(p43.addr, 1042, "序号锚定低地址寄存器（999+43）");
    assert_eq!(p43.fmt, Some(RegFormat::Int32Scaled), "32 位点");
    assert_eq!(p43.scale, 0.1);
    assert_eq!(p43.row().offset, 0.0);
    assert!(
        !pcs.contains(&"pcs_3zone_44".to_string()),
        "1043 是高半字，不产点"
    );

    // ⑥ 单寄存器点必产 1 点：hvac_in 声明 3 点（30001/30003/30004）
    assert_eq!(
        metrics_of("hvac_in"),
        ["hvac_in_1", "hvac_in_3", "hvac_in_4"]
    );
    assert_eq!(metrics_of("hvac_di").len(), 31, "31 位各 1 点");

    // ⑦ 无 `points` 的 32 位块产 `count / 2` 点：meter_batt 的 6 个总电能块（count 2 → 1 点）
    for blk in [
        "mb_e_act_comb",
        "mb_e_act_fwd",
        "mb_e_act_rev",
        "mb_e_rea_comb",
        "mb_e_rea_fwd",
        "mb_e_rea_rev",
    ] {
        assert_eq!(
            metrics_of(blk).len(),
            1,
            "块 {blk}（count 2、32 位）应产 1 点"
        );
    }
}

// ═══════════════ 消防信号清单 ↔ PRD §9.5.4 位定义（逐条对表，T4 的 DoD 之一）═══════════════

/// 取某行的信号键 → 活跃判据
fn sig_map(role: Role, addr: u16) -> Vec<(&'static str, point_table::SignalPick)> {
    point_table::signals_of(role, addr)
        .iter()
        .map(|s| (s.key, s.pick))
        .collect()
}

fn mask_of(role: Role, addr: u16, key: &str) -> u16 {
    sig_map(role, addr)
        .into_iter()
        .find_map(|(k, p)| match (k, p) {
            (k, point_table::SignalPick::WordBit { mask }) if k == key => Some(mask),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{role:?}/{addr} 上无 WordBit 信号 {key}"))
}

fn enum_active_of(role: Role, addr: u16, key: &str) -> Vec<u16> {
    sig_map(role, addr)
        .into_iter()
        .find_map(|(k, p)| match (k, p) {
            (k, point_table::SignalPick::WordEnum { active }) if k == key => {
                Some(active.iter().map(|(v, _)| *v).collect())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{role:?}/{addr} 上无 WordEnum 信号 {key}"))
}

/// 系统状态（addr 4）：**恰 6 条**信号，掩码逐位对表 PRD §9.5.4 的系统状态位定义表。
/// 不登记 bit15（工作模式）/ bit12（充电状态）/ bit7–0（备电电量）。
#[test]
fn fire_system_status_signals_match_prd_bits() {
    let want: &[(&str, u16)] = &[
        ("main_power_fault", 1 << 14),
        ("backup_power_fault", 1 << 13),
        ("drive_circuit_fault", 1 << 11),
        ("pressure_sensor_fault", 1 << 10),
        ("valve_open", 1 << 9),
        ("spray_fired", 1 << 8),
    ];
    let got = sig_map(Role::Fire, 4);
    assert_eq!(got.len(), 6, "系统状态恰登记 6 条信号，实际 {:?}", got);
    for (key, mask) in want {
        assert_eq!(mask_of(Role::Fire, 4, key), *mask, "信号 {key} 掩码不符");
    }
    // 每个掩码必须是**单一 bit**（"不得跨位图混装"，§11.4.4）
    for (key, mask) in want {
        assert_eq!(mask.count_ones(), 1, "信号 {key} 的 mask 须为单一位");
    }
    // 不登记的位：15 / 12 / 7–0
    let union: u16 = want.iter().fold(0u16, |a, (_, m)| a | m);
    for b in [15u16, 12] {
        assert_eq!(union & (1 << b), 0, "bit{b} 不得入信号（普通状态量）");
    }
    assert_eq!(union & 0x00FF, 0, "备电电量 bit7–0 不得入信号");
}

/// 烟/温/可燃（addr 6/7/8）：`mask = 0b11` —— **bit2（点型，预留未启用）不纳入**（PRD §9.7.6）
#[test]
fn fire_trigger_signals_cover_bit1_bit0_only() {
    for (addr, key) in [
        (6u16, "smoke_trigger"),
        (7, "temp_trigger"),
        (8, "combustible_trigger"),
    ] {
        let got = sig_map(Role::Fire, addr);
        assert_eq!(got.len(), 1, "addr {addr} 恰 1 条信号");
        assert_eq!(
            mask_of(Role::Fire, addr, key),
            0b11,
            "addr {addr} 的 mask 须为 0b11"
        );
        assert_eq!(
            mask_of(Role::Fire, addr, key) & 0b100,
            0,
            "bit2 预留位不得入 mask"
        );
    }
}

/// 火警状态（addr 9）：枚举活跃集 = {1,2,4,5}；**值 3 预留不入**、值 0 正常 = 非活跃
#[test]
fn fire_alarm_level_enum_matches_prd_values() {
    assert_eq!(enum_active_of(Role::Fire, 9, "level1"), vec![1]);
    assert_eq!(enum_active_of(Role::Fire, 9, "level2"), vec![2]);
    assert_eq!(enum_active_of(Role::Fire, 9, "emg_start"), vec![4]);
    assert_eq!(enum_active_of(Role::Fire, 9, "emg_stop"), vec![5]);
    let all: Vec<u16> = ["level1", "level2", "emg_start", "emg_stop"]
        .iter()
        .flat_map(|k| enum_active_of(Role::Fire, 9, k))
        .collect();
    assert!(!all.contains(&3), "值 3 预留不得入 active");
    assert!(!all.contains(&0), "值 0 工作正常 = 非活跃，不得入 active");
    assert_eq!(sig_map(Role::Fire, 9).len(), 4, "火警状态恰 4 条枚举信号");
}

/// 探测器状态：**探测器 1（addr 12）与探测器 2..n（模板 +1 = addr 18、运行期任意组）**
/// 都恰登记 bit12 报警总状态 / bit14 故障总状态；极性反转位（bit15 通信状态）与
/// 反馈位（bit10/11）、传感器细分（bit0–4）**一律不入表**。
#[test]
fn fire_detector_signals_only_alarm_and_fault() {
    // 12 = 探测器 1 状态（fire_sys）；18/24/126 = 探测器 2/3/20 的 `+1 状态`（模板命中）
    for addr in [12u16, 18, 24, 126] {
        assert_eq!(
            mask_of(Role::Fire, addr, "alarm"),
            1 << 12,
            "addr {addr} 报警总状态"
        );
        assert_eq!(
            mask_of(Role::Fire, addr, "fault"),
            1 << 14,
            "addr {addr} 故障总状态"
        );
        assert_eq!(
            sig_map(Role::Fire, addr).len(),
            2,
            "addr {addr} 恰 2 条信号"
        );
        let union = mask_of(Role::Fire, addr, "alarm") | mask_of(Role::Fire, addr, "fault");
        for b in [15u16, 13, 11, 10, 4, 3, 2, 1, 0] {
            assert_eq!(
                union & (1 << b),
                0,
                "addr {addr} 的 bit{b} 不得入表（极性反转位/反馈位/传感器细分）"
            );
        }
    }
}

/// **不登记清单**（与登记清单同等重要，防"顺手多产"）：钢瓶气压 / 探测器地址 /
/// 数据 1（G-6 展示层）/ CO·VOC·H2（无阈值口径）**一律零信号**；
/// 非消防 role 的全部点位也零信号（字级信号是消防专用）。
#[test]
fn signals_absent_on_non_event_points_and_non_fire_roles() {
    for addr in [5u16, 10, 11, 13, 14, 15, 16, 17, 19, 20, 21, 22] {
        assert!(
            point_table::signals_of(Role::Fire, addr).is_empty(),
            "消防 addr {addr} 不得登记信号"
        );
    }
    for row in point_table::POINT_REGS {
        if row.role != Role::Fire {
            assert!(
                row.signals.is_empty(),
                "非消防行 {}/{} 不得登记字级信号",
                row.role as u8,
                row.addr
            );
        }
    }
}

/// 消防 `fire_sys` 块的**每行**都要有中文名，且 `fire_det_count` 契约点可反查
/// （消费侧按点名取登记数，改名即断供 §9.5.4 的交叉校验）。
#[test]
fn fire_labels_and_contract_point_resolvable() {
    for addr in 4..=16 {
        let row = point_table::lookup(Role::Fire, addr).expect("fire_sys 逐点登记");
        assert!(!row.label.is_empty(), "fire_sys addr {addr} 缺中文名");
    }
    assert_eq!(
        point_table::label(Role::Fire, "fire_det_count"),
        point_table::lookup(Role::Fire, 10).map(|r| r.label),
        "跨文档契约点 fire_det_count 须可反查"
    );
    assert_eq!(
        point_table::label(Role::Battery, "soc"),
        point_table::lookup(Role::Battery, 118).map(|r| r.label),
        "`soc` 契约点须可反查"
    );
    // 设计 §11.7.2 第 4 条点名的两处**不得混淆**的例子
    assert_eq!(
        point_table::label(Role::Battery, "bms_alarm_225"),
        Some("簇一级告警"),
        "点名序号 225 = 位地址 424 = 簇一级告警"
    );
    assert_eq!(
        point_table::label(Role::Battery, "bms_alarm_26"),
        Some("簇 SOC 低·轻"),
        "位地址 225 = 点名 bms_alarm_26 = 簇 SOC 低·轻"
    );
}

/// 参考配置里**无 `points` 的块**（`bms_term` / `bms_alarm` / `hvac_di` / meter_batt 的
/// 6 个总电能块）在表内仍逐点登记（它们贡献 345 点里的一大半）。
#[test]
fn blocks_without_points_are_fully_registered() {
    // bms_term：4 个寄存器，块级 offset −40
    for addr in 2991..=2994u16 {
        let row = point_table::lookup(Role::Battery, addr).expect("bms_term 逐点登记");
        assert_eq!(row.offset, -40.0);
        assert!(matches!(
            row.kind,
            RegPointKind::Scalar(mupc_data_processing::meter_regs::RegFormat::Uint16)
        ));
    }
    // bms_alarm：288 位一个不缺（位地址即绝对位地址）
    for addr in 200..=487u16 {
        assert!(
            matches!(
                point_table::lookup_bit(Role::Battery, addr).map(|r| r.kind),
                Some(RegPointKind::Bit(_))
            ),
            "bms_alarm 位 {addr} 未登记"
        );
    }
    // hvac_di：31 位（**位空间**：位 0/2/3 与 hvac_in 的寄存器 0/2/3 同址）
    for addr in 0..=30u16 {
        assert!(
            matches!(
                point_table::lookup_bit(Role::Hvac, addr).map(|r| r.kind),
                Some(RegPointKind::Bit(_))
            ),
            "hvac_di 位 {addr} 未登记"
        );
    }
    // meter_batt 的 6 个总电能块（各 1 点、均 int32_scaled/0.01）
    for addr in [0x0000u16, 0x000A, 0x0014, 0x001E, 0x0028, 0x0032] {
        let row = point_table::lookup(Role::MeterBatt, addr).expect("总电能点逐点登记");
        assert_eq!(row.scale, 0.01, "addr {addr:#06x} 的 scale");
        assert_eq!(
            row.kind,
            RegPointKind::Scalar(mupc_data_processing::meter_regs::RegFormat::Int32Scaled),
            "addr {addr:#06x} 的 format"
        );
    }
}

/// 消防探测器区的**模板步进**：(n−1)*6+11 是 PRD §9.5.4 的块寻址规则 ——
/// 第 20 只（n=20 的最后一只）的首地址必须是 131+6=… 逐组校验 `+0..+5` 语义不串位。
#[test]
fn fire_detector_template_stride_matches_prd() {
    // 第 n 只首地址 = (n-1)*6+11；配置块起点 17 = 第 2 只
    for n in 2..=20u16 {
        let base = detector_base(n);
        let addrs: Vec<u16> = (0..6).map(|k| base + k).collect();
        // 模板命中：+0 地址、+1 状态（带 alarm/fault）、+2..+5 数据
        assert!(
            point_table::lookup(Role::Fire, addrs[0]).is_some(),
            "第 {n} 只 +0"
        );
        assert_eq!(
            sig_map(Role::Fire, addrs[1]).len(),
            2,
            "第 {n} 只 +1 状态须带 alarm/fault"
        );
        for a in &addrs[2..] {
            assert!(
                point_table::signals_of(Role::Fire, *a).is_empty(),
                "第 {n} 只 +2..+5 数据寄存器不得带信号"
            );
        }
        // 组内语义不串位：+1 的状态标签与 +2 的数据 1 标签必须不同
        let l1 = point_table::lookup(Role::Fire, addrs[1]).unwrap().label;
        let l2 = point_table::lookup(Role::Fire, addrs[2]).unwrap().label;
        assert_ne!(l1, l2, "第 {n} 只 +1/+2 模板串位");
    }
    // 配置块起点与 PRD 公式一致
    let fire = stations()
        .stations
        .into_iter()
        .find(|s| s.role == Role::Fire)
        .unwrap();
    let det = fire.regs.iter().find(|b| b.name == "fire_det").unwrap();
    assert_eq!(det.func, RegFunc::Holding);
    assert_eq!(
        det.addr,
        detector_base(2),
        "fire_det 块起点 = 第 2 只探测器首地址"
    );
    assert_eq!(det.count, 6 * (20 - 1), "n=20 时探测器块 count = 6×(n−1)");
}

/// PRD §9.5.4 的探测器块寻址公式：第 n 只的首地址 = `(n−1)*6 + 11`。
fn detector_base(n: u16) -> u16 {
    (n - 1) * 6 + 11
}
