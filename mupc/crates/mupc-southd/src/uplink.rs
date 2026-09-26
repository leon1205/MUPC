//! 上送点表**机械生成** + BMS 聚合**运行期求值**（U-74 / 01 设计 §9.2.1）。
//!
//! **本模块是"消灭手写 IOA 常量"的落点**（01 PRD §8.3.2 末注 / 01 设计 §9.0 现状 #1）：
//! 逐点 IOA **不由开发者手写**，而由本生成器从南向点表的两项唯一真源机械推导——
//! [`crate::points::expand`]（点名与块内序）与 [`crate::point_table`]（位语义 / 中文名）。
//! 放在 `mupc-southd` 的理由（§9.2.1）：**校验期与运行期同一份常量**
//! （与 `points.rs` 的"同一函数"不变量同源），且**不新增任何依赖边**（输出是纯数据）。
//!
//! # 生成算法（§9.2.1「生成算法（唯一，可机械执行）」逐条对应）
//!
//! ```text
//! for 站 in cfg.stations（按 cfg 顺序）:
//!     role == MeterGrid → 段 0，固定派生 6 点（不按 points::expand）
//!     否则 → 标量 = blocks.flat_map(points::expand).filter(Scalar)，按 (块 addr, 块内偏移) 升序
//!            位点 = blocks.flat_map(points::expand).filter(Bit)，按位地址升序
//!            按 role 施加子集规则（下表）：
//!              Battery   → 标量全量(BOTH) + 位点 MQTT-only + 15 聚合(IEC104-only，段 4)
//!              MeterBatt → 标量全量(BOTH)
//!              Pcs       → 标量全量(BOTH)（站不在 cfg ⇒ 整段不产条目）
//!              Fire      → fire_sys 13 点(BOTH) + 其余块 MQTT-only
//!              Hvac      → discrete 块 31 点(BOTH) + 其余块 MQTT-only
//!     IEC104 段内序号 = 1..n（标量在前、位点在后；段 4 的 15 聚合即段内序 1..15）
//!     IOA = 段基址 + 段内序号（**仅 IEC104 侧计数**；MQTT-only 点不占 IOA ⇒ ioa = 0）
//! ```
//!
//! # 两条通道的点集**不同**（§9.2.1.0，冲突 C-16）
//!
//! | 通道 | 点集 | 点数（PCS 未启用 / 启用） |
//! |------|------|--------------------------|
//! | IEC 104 | 段 1–7（标量子集 + 15 聚合） | **162 / 234** |
//! | MQTT（北向） | 全量（含 288 BMS 位、114 探测器等） | **552 / 624** |
//! | 并集（本函数产出） | IEC104 ∪ MQTT | **567 / 639** |
//!
//! ⇒ 消费方**必须**按 `UplinkPoint::channels` 过滤，**不得**假定"同一份点表"。
//!
//! # 生成期自检（§9.2.1.3，**订正后口径**）
//!
//! G-1 引用存在 / G-2 组间互斥 / G-3 扣减后全覆盖（**集合相等**，非"覆盖"）/
//! G-4 排除表一致 / G-5 通道点数。任一失败 ⇒ [`build_uplink_points`] 返回 `Err`
//! （错误文案含站 id / 块名 / 点名或位地址 + 实测值与期望值）⇒ 装配期**拒启动**。
//!
//! > ⚠️ 原稿写"并集必须覆盖 `Alarm ∪ State` **全部位**"，与排除表（78 位）自相矛盾 ——
//! > 按字面实现必 `Err` ⇒ **拒启动**。本节按 §9.2.1.3 的**订正口径**实现：
//! > `(Alarm ∪ State) − BMS_AGGR_EXCLUDED == ∪组`（集合相等；等价计数式 `221 == 143 + 78`）。

use std::collections::BTreeSet;

use crate::config::{RegFunc, Role, SouthStationsConfig, StationConf};
use crate::point_table::{self, BitClass, RegPointKind};
use crate::points::{self, PointKind};
use mupc_data_processing::latest_values::PointQuality;

// ───────────────────────────── 段基址（§9.2.1；与 PRD §8.3.2 分段表一致） ─────────────────────────────

/// 段 1：总表 `meter_grid`（**既有 IOA 1–6，现场追认，不得改号**）。
pub const SEG_GRID: u32 = 0;
/// 段 2：储能表 `meter_batt`（101–140）。
pub const SEG_METER_BATT: u32 = 100;
/// 段 3：BMS 遥测（201–257）。
pub const SEG_BMS_TELEM: u32 = 200;
/// 段 4：BMS 遥信（**15 个聚合**，301–315）。
pub const SEG_BMS_ALARM: u32 = 300;
/// 段 5：PCS（**条件性**，401–472）。
pub const SEG_PCS: u32 = 400;
/// 段 6：消防系统态（501–513）。
pub const SEG_FIRE: u32 = 500;
/// 段 7：空调告警位（601–631）。
pub const SEG_HVAC: u32 = 600;

// ───────────────────────────── 通道掩码 / 档位 / 条目类型 ─────────────────────────────

/// 上送通道：两条北向通道的点集**不同**，故逐点带掩码（§9.2.1 / C-16）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelMask(pub u8);

impl ChannelMask {
    /// 调度主站（IEC 104）子集。
    pub const IEC104: ChannelMask = ChannelMask(0b01);
    /// 物联平台（MQTT 北向）全量。
    pub const MQTT: ChannelMask = ChannelMask(0b10);
    /// 两条通道都要。
    pub const BOTH: ChannelMask = ChannelMask(0b11);

    /// 本掩码是否含某通道。
    pub fn has(self, c: ChannelMask) -> bool {
        self.0 & c.0 != 0
    }
}

/// 上送档位（PRD §8.6.3 A/B/C 档，**唯一分档真源**，落在本模块）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataClass {
    /// A 档：关键/控制相关，1 s（grid 6 + meter_batt 9 + bms 4（+ PCS 3））。
    A,
    /// B 档：其余标量遥测，5 s。
    B,
    /// C 档：位点 / 状态枚举 / 消防系统态，变化上送（COS，≤ 1 s）。
    C,
}

/// 上送点形态（决定 IEC 104 的 TypeID：`Scalar` ⇒ TI=36，`Bit` ⇒ TI=30，§9.2.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UplinkKind {
    /// 标量点（16/32 位工程值）。
    Scalar,
    /// 位点（0/1；聚合点亦为位）。
    Bit,
}

/// 上送条目（**含逐点 IOA 与通道归属**，§9.2.1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UplinkPoint {
    /// IEC104 段内 IOA；**仅当 `channels.has(IEC104)` 时有意义**（MQTT-only 点为 0）。
    pub ioa: u32,
    /// 站 id（MQTT 分片用）。
    pub station: String,
    /// 南向点名（与 02 PRD §9.4.2.2 逐字一致）或聚合点名。
    pub metric: String,
    /// 形态。
    pub kind: UplinkKind,
    /// 档位。
    pub class: DataClass,
    /// 通道归属。
    pub channels: ChannelMask,
    /// 语义标签（中文，供对点清单与日志）。来自 [`point_table::label`]（查不到 ⇒ `""`）
    /// 或本模块的固定表（grid 派生点名 / 聚合点名）。
    pub label: &'static str,
}

// ───────────────────────────── 段 1：grid 派生 6 点（固定表，不得改号） ─────────────────────────────

/// 段 1 固定表：`(IOA, 点名, 语义标签)`。
///
/// 这 6 点是 `DataPackage.electrical` 的**派生量**（既有"现场追认"口径），**不按
/// `points::expand` 产出**（该站配置展开为 16 点，见 §9.7 C-1：分相 15 点不进上送）。
/// 点名（`active_power` 等）是**点名的唯一例外**（§9.7 C-17 ②），**不得**改为
/// `p_total_1` / `p_1` 等位置式名。
#[rustfmt::skip]
pub const GRID_DERIVED_6: &[(u32, &str, &str)] = &[
    (1, "active_power", "总有功功率（来源 DataPackage.electrical.active_power）"),
    (2, "reactive_power", "总无功功率（来源 .reactive_power）"),
    (3, "voltage", "电压（现取 A 相 u[0]，§9.7 C-13 如实登记）"),
    (4, "current", "电流（现取 A 相幅值，§9.7 C-13）"),
    (5, "cos_phi", "功率因数（现取 A 相，§9.7 C-13）"),
    (6, "frequency", "频率（现为常量 50.0，§9.7 C-13）"),
];

// ───────────────────────────── A 档表（§9.2.2 的"唯一表驱动"） ─────────────────────────────

/// A 档点名表（逐点可枚举，§9.2.2）：`(role, 点名)`，**共 22 项**。
///
/// A 档 = grid 6 + meter_batt 9 + bms 4 + PCS 3。PCS 未启用时自然只有 19 项参与
/// （其 3 项无对应站 ⇒ 无对应点），**故本表恒为 22 项**，而"实测 A 档点数"随配置为
/// **19 / 22**（生成期断言，见 [`check_channel_counts`]）。
///
/// ⚠️ 点名写错 ⇒ 该点落到 B 档 ⇒ 生成期 A 档点数断言失败（拒启动）——这是本表的防错机制。
#[rustfmt::skip]
const CLASS_A: &[(Role, &str)] = &[
    // ① grid（6）
    (Role::MeterGrid, "active_power"),
    (Role::MeterGrid, "reactive_power"),
    (Role::MeterGrid, "voltage"),
    (Role::MeterGrid, "current"),
    (Role::MeterGrid, "cos_phi"),
    (Role::MeterGrid, "frequency"),
    // ② meter_batt（9）：总 P / 总 Q / 相电压 3 / 相电流 3 / 频率
    (Role::MeterBatt, "mb_power_7"),
    (Role::MeterBatt, "mb_power_15"),
    (Role::MeterBatt, "mb_ui_1"),
    (Role::MeterBatt, "mb_ui_2"),
    (Role::MeterBatt, "mb_ui_3"),
    (Role::MeterBatt, "mb_ui_4"),
    (Role::MeterBatt, "mb_ui_5"),
    (Role::MeterBatt, "mb_ui_6"),
    (Role::MeterBatt, "mb_freq_line_1"),
    // ③ bms（4）：SOC / 簇组电压 / 簇组电流 / 实时充放电功率
    (Role::Battery, "soc"),
    (Role::Battery, "bms_io_16"),
    (Role::Battery, "bms_io_17"),
    (Role::Battery, "bms_meta_6"),
    // ④ PCS 启用后另加（3）
    (Role::Pcs, "pcs_3zone_14"),
    (Role::Pcs, "pcs_3zone_33"),
    (Role::Pcs, "pcs_3zone_37"),
];

/// 该 `(role, 点名)` 是否属 A 档。
fn is_class_a(role: Role, metric: &str) -> bool {
    CLASS_A.iter().any(|(r, m)| *r == role && *m == metric)
}

/// 标量点的 `A / B` 分档（C 档由调用点显式指定，见 §9.2.2：C = 位点 + 消防系统态 + 聚合）。
fn class_ab(role: Role, metric: &str) -> DataClass {
    if is_class_a(role, metric) {
        DataClass::A
    } else {
        DataClass::B
    }
}

// ───────────────────────────── 段 4：BMS 聚合组（15 组 / 143 位） ─────────────────────────────

/// 一个 BMS 聚合组（§9.2.1.2）。
#[derive(Debug, Clone, Copy)]
pub struct AggrGroup {
    /// 聚合点名（`bms_aggr_*`）。
    pub metric: &'static str,
    /// 聚合的**位地址**列表（逐位展开，覆盖合计 143 位）。
    pub bits: &'static [u16],
    /// 语义标签。
    pub label: &'static str,
}

/// BMS 告警/状态聚合组表（**15 组**，段 4 = IOA 301–315；§9.2.1.2）。
///
/// > 命名说明：§9.2.1 的"子集规则"表把它写作 `BMS_ALARM_GROUPS`，§9.2.1.1 的求值规则写作
/// > `BMS_AGGR_GROUPS` —— 同一张表的两个称谓。本实现**只落一份**，取名 `BMS_AGGR_GROUPS`
/// > （求值侧口径）。
///
/// 组内位地址与 `point_table.rs` 的 `BitClass` 逐位一致（生成期 G-1 断言）；
/// 第 13–15 组来自 2026-09-23 用户**裁定 A**（簇级告警 3 + 继电器粘连 6 + AFE 故障 1）。
#[rustfmt::skip]
pub const BMS_AGGR_GROUPS: &[AggrGroup] = &[
    AggrGroup {
        metric: "bms_aggr_cluster_voltage",
        bits: &[201, 202, 203, 204, 205, 206],
        label: "簇端电压 欠压/过压 × 轻/中/重",
    },
    AggrGroup {
        metric: "bms_aggr_cluster_current",
        bits: &[207, 208, 209, 210, 211, 212],
        label: "簇端充/放电电流 × 轻/中/重",
    },
    AggrGroup {
        metric: "bms_aggr_cell_voltage",
        bits: &[213, 214, 215, 216, 217, 218],
        label: "单体 欠压/过压 × 轻/中/重",
    },
    AggrGroup {
        metric: "bms_aggr_cell_temp",
        bits: &[219, 220, 221, 222, 223, 224],
        label: "单体 欠温/过温 × 轻/中/重",
    },
    AggrGroup {
        metric: "bms_aggr_soc_low",
        bits: &[225, 226, 227],
        label: "SOC 低 × 轻/中/重",
    },
    AggrGroup {
        metric: "bms_aggr_soh_low",
        bits: &[228, 229, 230],
        label: "SOH 低 × 轻/中/重",
    },
    AggrGroup {
        metric: "bms_aggr_cell_spread",
        bits: &[231, 232, 233, 234, 235, 236],
        label: "单体压差 + 温差 × 轻/中/重",
    },
    AggrGroup {
        metric: "bms_aggr_slave_comm_lost",
        bits: &[
            237, 238, 239, 240, 241, 242, 243, 244, 245, 246, 247, 248, 249, 250, 251, 252, 253,
            254, 255, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265, 266, 267, 268, 269, 270,
            271, 272, 273, 274, 275, 276,
        ],
        label: "从控 1–40 通讯失联（逐从控位聚合）",
    },
    AggrGroup {
        metric: "bms_aggr_terminal_pack",
        bits: &[277, 278, 279, 280, 281, 282, 283, 284, 285],
        label: "端子/箱体温度过高 ×3 + PACK 电压过/低 ×3",
    },
    AggrGroup {
        metric: "bms_aggr_acquire_fault",
        bits: &[286, 287],
        label: "单体电压/温度采集故障",
    },
    AggrGroup {
        metric: "bms_aggr_slave_di",
        bits: &[
            299, 300, 301, 302, 303, 304, 305, 306, 307, 308, 309, 310, 311, 312, 313, 314, 315,
            316, 317, 318, 319, 320, 321, 322, 323, 324, 325, 326, 327, 328, 329, 330, 331, 332,
            333, 334, 335, 336, 337, 338, 339,
        ],
        label: "从控 DI 告警（风扇/气溶胶/MSD）+ 逐从控 DI 定制告警",
    },
    AggrGroup {
        metric: "bms_aggr_run_abnormal",
        bits: &[289, 293, 294, 295, 296],
        label: "运行异常：充电态 + 禁充 + 禁放 + 充放禁止 + 故障",
    },
    AggrGroup {
        metric: "bms_aggr_cluster_level",
        bits: &[424, 425, 426],
        label: "簇级告警 一级/二级/三级（裁定 A 新增）",
    },
    AggrGroup {
        metric: "bms_aggr_relay_stuck",
        bits: &[454, 455, 456, 457, 458, 459],
        label: "继电器粘连：总正/总负/预充/风扇/休眠/断路器（裁定 A 新增）",
    },
    AggrGroup {
        metric: "bms_aggr_afe_fault",
        bits: &[460],
        label: "AFE 故障（裁定 A 新增）",
    },
];

/// `∪组` 覆盖位数（§9.2.1.3：143 = 138 Alarm + 5 State）。
const AGGR_COVERED_BITS: usize = 143;
/// `BMS_AGGR_EXCLUDED` 的**逐位展开**基数（§9.2.1.3：78 = 74 Alarm + 4 State）。
const AGGR_EXCLUDED_BITS: usize = 78;
/// `card(Alarm ∪ State)` 的等价计数式右端（§9.2.1.3：`221 == 143 + 78`）。
const ALARM_UNION_STATE_BITS: usize = 221;

/// 排除表构造子（让 78 行可读、可逐位核对）。
const fn ex(addr: u16, class: BitClass, reason: &'static str) -> (u16, BitClass, &'static str) {
    (addr, class, reason)
}

/// **显式排除表**：只列 `Alarm ∪ State` 中"**有意不入聚合**"的位（Reserved 不在判据内，不列）。
///
/// 每行 `(位地址, 期望 BitClass, 理由)`；`期望 BitClass` **必须与 `point_table` 实际值相等**，
/// 否则生成期 `Err`（防"把 Alarm 误标 Reserved"这类静默漏报 = §9.2.1.3 的 P2 缺陷）。
///
/// **78 行逐位展开**（区间只是文档的简写，代码里不得用区间糊过去 —— G-4 ③ 断言基数 == 78）。
/// 其中 4 位 `State` 为正常态、24 位（340–363）与 50 位（427–453、461–483）为有意不入的 Alarm
/// （`Q-A` 默认不入 / `Q-B` 裁定后其余 50 位维持不入；若产品后续要求纳入，**改表不改码**）。
#[rustfmt::skip]
pub const BMS_AGGR_EXCLUDED: &[(u16, BitClass, &str)] = &[
    // ── 4 位 State：正常态而非异常，或待产品确认 ──
    ex(288, BitClass::State, "簇初始状态：正常态而非异常（充/放由组 312 表达）"),
    ex(290, BitClass::State, "簇放电：正常态而非异常"),
    ex(291, BitClass::State, "簇就绪：正常态而非异常"),
    ex(298, BitClass::State, "簇高压箱状态：待产品确认（D-Q3；若需上主站则扩段 4）"),
    // ── 24 位 Alarm（340–363）：单体充/放电过温欠温、温升、极柱温度、电压变化；Q-A 默认不入 ──
    ex(340, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入（改表不改码）"),
    ex(341, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(342, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(343, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(344, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(345, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(346, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(347, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(348, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(349, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(350, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(351, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(352, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(353, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(354, BitClass::Alarm, "Q-A：与 304 单体温度语义重叠，默认不入"),
    ex(355, BitClass::Alarm, "Q-A：极柱温度与 304 重叠，默认不入"),
    ex(356, BitClass::Alarm, "Q-A：极柱温度与 304 重叠，默认不入"),
    ex(357, BitClass::Alarm, "Q-A：极柱温度与 304 重叠，默认不入"),
    ex(358, BitClass::Alarm, "Q-A：极柱温度与 304 重叠，默认不入"),
    ex(359, BitClass::Alarm, "Q-A：极柱温度与 304 重叠，默认不入"),
    ex(360, BitClass::Alarm, "Q-A：极柱温度与 304 重叠，默认不入"),
    ex(361, BitClass::Alarm, "Q-A：电压变化过大与 303 重叠，默认不入"),
    ex(362, BitClass::Alarm, "Q-A：电压变化过大与 303 重叠，默认不入"),
    ex(363, BitClass::Alarm, "Q-A：电压变化过大与 303 重叠，默认不入"),
    // ── 50 位 Alarm（427–453 + 461–483）：Q-B 裁定"其余 50 位维持不入"（主站为安全总貌量） ──
    ex(427, BitClass::Alarm, "Q-B：端子温度过低，维持不入（主站为安全总貌量）"),
    ex(428, BitClass::Alarm, "Q-B：端子温度过低，维持不入"),
    ex(429, BitClass::Alarm, "Q-B：端子温度过低，维持不入"),
    ex(430, BitClass::Alarm, "Q-B：MOS 过温，维持不入"),
    ex(431, BitClass::Alarm, "Q-B：MOS 过温，维持不入"),
    ex(432, BitClass::Alarm, "Q-B：MOS 过温，维持不入"),
    ex(433, BitClass::Alarm, "Q-B：MOS 欠温，维持不入"),
    ex(434, BitClass::Alarm, "Q-B：MOS 欠温，维持不入"),
    ex(435, BitClass::Alarm, "Q-B：MOS 欠温，维持不入"),
    ex(436, BitClass::Alarm, "Q-B：SOE 低，维持不入"),
    ex(437, BitClass::Alarm, "Q-B：SOE 低，维持不入"),
    ex(438, BitClass::Alarm, "Q-B：SOE 低，维持不入"),
    ex(439, BitClass::Alarm, "Q-B：单体正极柱温度，维持不入"),
    ex(440, BitClass::Alarm, "Q-B：单体正极柱温度，维持不入"),
    ex(441, BitClass::Alarm, "Q-B：单体正极柱温度，维持不入"),
    ex(442, BitClass::Alarm, "Q-B：单体正极柱温度，维持不入"),
    ex(443, BitClass::Alarm, "Q-B：单体正极柱温度，维持不入"),
    ex(444, BitClass::Alarm, "Q-B：单体正极柱温度，维持不入"),
    ex(445, BitClass::Alarm, "Q-B：单体负极柱温度，维持不入"),
    ex(446, BitClass::Alarm, "Q-B：单体负极柱温度，维持不入"),
    ex(447, BitClass::Alarm, "Q-B：单体负极柱温度，维持不入"),
    ex(448, BitClass::Alarm, "Q-B：单体负极柱温度，维持不入"),
    ex(449, BitClass::Alarm, "Q-B：单体负极柱温度，维持不入"),
    ex(450, BitClass::Alarm, "Q-B：单体负极柱温度，维持不入"),
    ex(451, BitClass::Alarm, "Q-B：绝缘检测低，维持不入"),
    ex(452, BitClass::Alarm, "Q-B：绝缘检测低，维持不入"),
    ex(453, BitClass::Alarm, "Q-B：绝缘检测低，维持不入"),
    ex(461, BitClass::Alarm, "Q-B：单体温度短路，维持不入（检修/自检类）"),
    ex(462, BitClass::Alarm, "Q-B：单体温度断路，维持不入（检修/自检类）"),
    ex(463, BitClass::Alarm, "Q-B：MOS 温度故障，维持不入"),
    ex(464, BitClass::Alarm, "Q-B：均衡 MOS 故障，维持不入"),
    ex(465, BitClass::Alarm, "Q-B：从控通讯故障，维持不入"),
    ex(466, BitClass::Alarm, "Q-B：从控供电故障，维持不入"),
    ex(467, BitClass::Alarm, "Q-B：从控风扇故障，维持不入"),
    ex(468, BitClass::Alarm, "Q-B：从控程序升级故障，维持不入"),
    ex(469, BitClass::Alarm, "Q-B：从控参数设置故障，维持不入"),
    ex(470, BitClass::Alarm, "Q-B：从控供电过压，维持不入"),
    ex(471, BitClass::Alarm, "Q-B：从控供电过压，维持不入"),
    ex(472, BitClass::Alarm, "Q-B：从控供电过压，维持不入"),
    ex(473, BitClass::Alarm, "Q-B：主控供电故障，维持不入"),
    ex(474, BitClass::Alarm, "Q-B：主控程序升级故障，维持不入"),
    ex(475, BitClass::Alarm, "Q-B：主控供电过压，维持不入"),
    ex(476, BitClass::Alarm, "Q-B：主控供电过压，维持不入"),
    ex(477, BitClass::Alarm, "Q-B：主控供电过压，维持不入"),
    ex(478, BitClass::Alarm, "Q-B：EEPROM 存储故障，维持不入（检修类）"),
    ex(479, BitClass::Alarm, "Q-B：地址编码故障，维持不入（检修类）"),
    ex(480, BitClass::Alarm, "Q-B：CAN 电流采集故障，维持不入（检修类）"),
    ex(481, BitClass::Alarm, "Q-B：485-1 通讯失联，维持不入"),
    ex(482, BitClass::Alarm, "Q-B：485-2 通讯失联，维持不入"),
    ex(483, BitClass::Alarm, "Q-B：PCS 失联，维持不入"),
];

// ───────────────────────────── 站内点展开（中间形态） ─────────────────────────────

/// 展开后的站内一点（生成器的中间形态；不对消费方暴露）。
struct StationPoint {
    metric: String,
    kind: UplinkKind,
    block_name: String,
    block_addr: u16,
    block_func: RegFunc,
    /// 标量 = 寄存器块内偏移；位 = 位块内偏移（0 基）。
    offset: u16,
    label: &'static str,
}

impl StationPoint {
    /// 位地址（仅位点有意义）：块 `addr` + 块内位偏移。
    fn bit_addr(&self) -> u16 {
        self.block_addr.wrapping_add(self.offset)
    }
}

/// 站内全部点（点名 / 形态 / 所属块 / 中文名）。**校验期与运行期同一函数**
/// （[`points::expand`]）—— 防"配置校验通过但运行期展开不同"。
fn expand_station(st: &StationConf) -> Result<Vec<StationPoint>, String> {
    let mut out = Vec::new();
    for b in &st.regs {
        let pts = points::expand(b)
            .map_err(|e| format!("south_stations: 站 {} 块 {} 点展开失败：{e}", st.id, b.name))?;
        for p in pts {
            let kind = match p.kind {
                PointKind::Scalar { .. } => UplinkKind::Scalar,
                PointKind::Bit { .. } => UplinkKind::Bit,
            };
            let label = point_table::label(st.role, &p.metric).unwrap_or("");
            out.push(StationPoint {
                metric: p.metric,
                kind,
                block_name: b.name.clone(),
                block_addr: b.addr,
                block_func: b.func,
                offset: p.kind.offset(),
                label,
            });
        }
    }
    Ok(out)
}

/// 标量子集，按 `(块 addr, 块内偏移)` 升序（§9.2.1 生成算法）。
fn scalars_sorted(pts: &[StationPoint]) -> Vec<&StationPoint> {
    let mut v: Vec<&StationPoint> = pts
        .iter()
        .filter(|p| p.kind == UplinkKind::Scalar)
        .collect();
    v.sort_by_key(|p| (p.block_addr, p.offset));
    v
}

/// 位点子集，按位地址升序（§9.2.1 生成算法；位地址 = 块 `addr` + 块内偏移）。
fn bits_sorted(pts: &[StationPoint]) -> Vec<&StationPoint> {
    let mut v: Vec<&StationPoint> = pts.iter().filter(|p| p.kind == UplinkKind::Bit).collect();
    v.sort_by_key(|p| (p.bit_addr(), p.block_addr, p.offset));
    v
}

// ───────────────────────────── 生成器 ─────────────────────────────

/// 生成**并集**上送条目（含 15 个 IEC104 独有聚合）。**失败即拒启动**（配置/点表漂移的
/// fail-fast 点，与 `validate_south_stations` 同范式）。
///
/// **确定性**：同一输入多次调用产出**逐字节相同**的列表（按 `cfg.stations` 顺序 + 站内
/// `(块 addr, 块内偏移)` / 位地址升序，全程无 `HashMap` 迭代）。
pub fn build_uplink_points(cfg: &SouthStationsConfig) -> Result<Vec<UplinkPoint>, String> {
    let mut out: Vec<UplinkPoint> = Vec::new();
    // 站内位地址宇宙（仅用于 G-1 的"引用存在"判据；无 battery 站 ⇒ `None` ⇒ 跳过该半条）
    let mut bit_universe: Option<BTreeSet<u16>> = None;

    for st in &cfg.stations {
        let pts = expand_station(st)?;
        match st.role {
            Role::MeterGrid => {
                // 段 1：**固定派生 6 点**（不按 points::expand —— §9.7 C-1）
                for (ioa, metric, label) in GRID_DERIVED_6 {
                    out.push(UplinkPoint {
                        ioa: SEG_GRID + *ioa,
                        station: st.id.clone(),
                        metric: (*metric).to_string(),
                        kind: UplinkKind::Scalar,
                        class: DataClass::A,
                        channels: ChannelMask::BOTH,
                        label,
                    });
                }
            }
            Role::MeterBatt => {
                let scalars = scalars_sorted(&pts);
                expect_count(st, "段 2（储能表标量）", scalars.len(), 40)?;
                push_iec_segment(&mut out, st, SEG_METER_BATT, &scalars, |p| {
                    class_ab(st.role, &p.metric)
                });
            }
            Role::Battery => {
                let scalars = scalars_sorted(&pts);
                let bits = bits_sorted(&pts);
                expect_count(st, "段 3（BMS 遥测标量）", scalars.len(), 57)?;
                push_iec_segment(&mut out, st, SEG_BMS_TELEM, &scalars, |p| {
                    class_ab(st.role, &p.metric)
                });
                // 段 4：15 个聚合（**IEC104-only**，段内序 1..15 ⇒ IOA 301–315）
                for (i, g) in BMS_AGGR_GROUPS.iter().enumerate() {
                    out.push(UplinkPoint {
                        ioa: SEG_BMS_ALARM + 1 + i as u32,
                        station: st.id.clone(),
                        metric: g.metric.to_string(),
                        kind: UplinkKind::Bit,
                        class: DataClass::C,
                        channels: ChannelMask::IEC104,
                        label: g.label,
                    });
                }
                // 288 个告警位：**MQTT-only**（PRD §8.3.1：主站无逐从控/三级粒度需求）
                expect_count(st, "BMS 告警位（MQTT-only）", bits.len(), 288)?;
                push_iec_segment(&mut out, st, 0, &bits, |_| DataClass::C);
                let universe: BTreeSet<u16> = bits.iter().map(|p| p.bit_addr()).collect();
                bit_universe = Some(universe);
            }
            Role::Pcs => {
                let scalars = scalars_sorted(&pts);
                expect_count(st, "段 5（PCS）", scalars.len(), 72)?;
                push_iec_segment(&mut out, st, SEG_PCS, &scalars, |p| {
                    class_ab(st.role, &p.metric)
                });
            }
            Role::Fire => {
                // 仅 `fire_sys` 块进 IEC104；`fire_det`（114 探测器明细）为 MQTT-only
                let mut iec: Vec<&StationPoint> = Vec::new();
                let mut mqtt_only: Vec<&StationPoint> = Vec::new();
                for p in scalars_sorted(&pts) {
                    if p.block_name == "fire_sys" {
                        iec.push(p);
                    } else {
                        mqtt_only.push(p);
                    }
                }
                expect_count(st, "段 6（消防系统态 fire_sys）", iec.len(), 13)?;
                // 消防系统态 13 点走**变化上送**（PRD §8.3.2）⇒ C 档
                push_iec_segment(&mut out, st, SEG_FIRE, &iec, |_| DataClass::C);
                // 探测器明细：MQTT-only，B 档遥测
                push_iec_segment(&mut out, st, 0, &mqtt_only, |_| DataClass::B);
            }
            Role::Hvac => {
                // 仅 `func: discrete` 块（31 个告警位）进 IEC104；温湿度 3 点为 MQTT-only
                let mut iec: Vec<&StationPoint> = Vec::new();
                let mut mqtt_only: Vec<&StationPoint> = Vec::new();
                for p in pts.iter() {
                    if p.block_func == RegFunc::Discrete {
                        iec.push(p);
                    } else {
                        mqtt_only.push(p);
                    }
                }
                iec.sort_by_key(|p| (p.bit_addr(), p.block_addr, p.offset));
                mqtt_only.sort_by_key(|p| (p.block_addr, p.offset));
                expect_count(st, "段 7（空调告警位 discrete 块）", iec.len(), 31)?;
                push_iec_segment(&mut out, st, SEG_HVAC, &iec, |_| DataClass::C);
                push_iec_segment(&mut out, st, 0, &mqtt_only, |_| DataClass::B);
            }
        }
    }

    check_aggr_consistency(BMS_AGGR_GROUPS, BMS_AGGR_EXCLUDED, bit_universe.as_ref())?;
    check_channel_counts(&out, cfg.stations.iter().any(|s| s.role == Role::Pcs))?;
    Ok(out)
}

/// 某段点数断言（失败即拒启动，文案含站 id / 段名 / 实测值与期望值）。
fn expect_count(st: &StationConf, seg: &str, actual: usize, expected: usize) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "south_stations: 站 {}（role={:?}）{seg} 实测 {} 点、期望 {} 点 —— 点表漂移，生成期\
         拒启动（01 设计 §9.2.1「子集规则与自检」）",
        st.id, st.role, actual, expected
    ))
}

/// 推入一段条目。`seg == 0` ⇒ **MQTT-only**（不占 IOA，`ioa = 0`）。
fn push_iec_segment(
    out: &mut Vec<UplinkPoint>,
    st: &StationConf,
    seg: u32,
    pts: &[&StationPoint],
    class_of: impl Fn(&StationPoint) -> DataClass,
) {
    let channels = if seg == 0 {
        ChannelMask::MQTT
    } else {
        ChannelMask::BOTH
    };
    for (i, p) in pts.iter().enumerate() {
        let ioa = if channels.has(ChannelMask::IEC104) {
            seg + 1 + i as u32
        } else {
            0
        };
        out.push(UplinkPoint {
            ioa,
            station: st.id.clone(),
            metric: p.metric.clone(),
            kind: p.kind,
            class: class_of(p),
            channels,
            label: p.label,
        });
    }
}

// ───────────────────────────── 生成期自检（§9.2.1.3） ─────────────────────────────

/// G-5：三通道点数 + 三档点数断言（§9.2.1.0 表 / §9.2.2 表）。
///
/// 期望值随"PCS 站是否配置"二选一（EX-7：站不在 `cfg` ⇒ 72 点**不产条目**，
/// 无需运行期特判）。
fn check_channel_counts(points: &[UplinkPoint], has_pcs: bool) -> Result<(), String> {
    let idx = usize::from(has_pcs);

    let iec104 = points
        .iter()
        .filter(|p| p.channels.has(ChannelMask::IEC104))
        .count();
    let mqtt = points
        .iter()
        .filter(|p| p.channels.has(ChannelMask::MQTT))
        .count();
    let union = points.len();
    let expect = [[162usize, 552, 567], [234, 624, 639]][idx];
    if [iec104, mqtt, union] != expect {
        return Err(format!(
            "south_stations: 生成期自检 G-5（通道点数）失败 —— 实测 IEC104={iec104} / MQTT={mqtt} / 并集={union}，\
             期望 IEC104={} / MQTT={} / 并集={}（PCS {}）〔01 设计 §9.2.1.0〕",
            expect[0], expect[1], expect[2],
            if has_pcs { "已启用" } else { "未启用" }
        ));
    }

    // 三档之和必须等于通道点数（§9.2.2 的恒等式；只对 IEC104 侧计数 —— 档位表定义的就是它）
    let mut abc = [0usize; 3];
    for p in points
        .iter()
        .filter(|p| p.channels.has(ChannelMask::IEC104))
    {
        abc[match p.class {
            DataClass::A => 0,
            DataClass::B => 1,
            DataClass::C => 2,
        }] += 1;
    }
    let expect_abc = [[19usize, 84, 59], [22, 153, 59]][idx];
    if abc != expect_abc {
        return Err(format!(
            "south_stations: 生成期自检 G-5（档位点数）失败 —— 实测 A/B/C={:?}，期望 {:?}（PCS {}）\
             〔01 设计 §9.2.2；A 档表 CLASS_A 共 {} 项〕",
            abc,
            expect_abc,
            if has_pcs { "已启用" } else { "未启用" },
            CLASS_A.len()
        ));
    }
    Ok(())
}

/// `point_table` 中 `Role::Battery` 的位点分类集合：`(Alarm ∪ State, Reserved)`。
fn battery_bit_classes() -> (BTreeSet<u16>, BTreeSet<u16>) {
    let mut aset = BTreeSet::new();
    let mut rset = BTreeSet::new();
    for row in point_table::POINT_REGS
        .iter()
        .filter(|r| r.role == Role::Battery)
    {
        if let RegPointKind::Bit(class) = row.kind {
            match class {
                BitClass::Alarm | BitClass::State => {
                    aset.insert(row.addr);
                }
                BitClass::Reserved => {
                    rset.insert(row.addr);
                }
            }
        }
    }
    (aset, rset)
}

/// G-1～G-4：聚合组表与排除表的一致性自检（§9.2.1.3 订正后口径）。
///
/// `bit_universe` = 站配置里位块展开出的位地址集合（`None` ⇒ 无 battery 站 / 无位块 ⇒
/// **跳过 G-1 的"引用存在于 `points::expand(bms_alarm)`"半条**，此时点数由 G-5 兜底）。
fn check_aggr_consistency(
    groups: &[AggrGroup],
    excluded: &[(u16, BitClass, &str)],
    bit_universe: Option<&BTreeSet<u16>>,
) -> Result<(), String> {
    // ── G-1 引用存在 ──
    let mut covered: BTreeSet<u16> = BTreeSet::new();
    for g in groups {
        for &addr in g.bits {
            if let Some(universe) = bit_universe {
                if !universe.contains(&addr) {
                    return Err(format!(
                        "south_stations: 生成期自检 G-1 失败 —— 聚合组 {} 引用的位地址 {} 不在站配置的位块中\
                         （points::expand 未产出该位）〔01 设计 §9.2.1.3〕",
                        g.metric, addr
                    ));
                }
            }
            match point_table::lookup_bit(Role::Battery, addr) {
                Some(row) => match row.kind {
                    RegPointKind::Bit(BitClass::Alarm) | RegPointKind::Bit(BitClass::State) => {}
                    RegPointKind::Bit(other) => {
                        return Err(format!(
                            "south_stations: 生成期自检 G-1 失败 —— 聚合组 {} 引用的位地址 {} 在 point_table 中为 \
                             {other:?}（须为 Alarm 或 State）〔01 设计 §9.2.1.3〕",
                            g.metric, addr
                        ));
                    }
                    RegPointKind::Scalar(_) => {
                        return Err(format!(
                            "south_stations: 生成期自检 G-1 失败 —— 聚合组 {} 引用的地址 {} 在 point_table 中\
                             不是位点〔01 设计 §9.2.1.3〕",
                            g.metric, addr
                        ));
                    }
                },
                None => {
                    return Err(format!(
                        "south_stations: 生成期自检 G-1 失败 —— 聚合组 {} 引用的位地址 {} 在 point_table 中\
                         查不到登记行〔01 设计 §9.2.1.3〕",
                        g.metric, addr
                    ));
                }
            }
        }
    }

    // ── G-2 组间互斥 ──
    let sum_of_sizes: usize = groups.iter().map(|g| g.bits.len()).sum();
    let mut union: BTreeSet<u16> = BTreeSet::new();
    for g in groups {
        for &addr in g.bits {
            covered.insert(addr);
            union.insert(addr);
        }
    }
    if covered.len() != sum_of_sizes {
        return Err(format!(
            "south_stations: 生成期自检 G-2 失败 —— {} 组的位集合两两相交（各组基数之和 {} ≠ ∪组基数 {}）\
             〔01 设计 §9.2.1.3〕",
            groups.len(),
            sum_of_sizes,
            covered.len()
        ));
    }
    if union.len() != AGGR_COVERED_BITS {
        return Err(format!(
            "south_stations: 生成期自检 G-2 失败 —— ∪组 基数 {} ≠ {}〔01 设计 §9.2.1.3〕",
            union.len(),
            AGGR_COVERED_BITS
        ));
    }

    // ── G-4 排除表一致 ──
    let excluded_set: BTreeSet<u16> = excluded.iter().map(|e| e.0).collect();
    if excluded.len() != AGGR_EXCLUDED_BITS || excluded_set.len() != AGGR_EXCLUDED_BITS {
        return Err(format!(
            "south_stations: 生成期自检 G-4 ③ 失败 —— 排除表逐位展开基数 {}（去重后 {}）≠ {}（不得用区间糊过去）\
             〔01 设计 §9.2.1.3〕",
            excluded.len(),
            excluded_set.len(),
            AGGR_EXCLUDED_BITS
        ));
    }
    for &addr in &excluded_set {
        if union.contains(&addr) {
            return Err(format!(
                "south_stations: 生成期自检 G-4 ① 失败 —— 位地址 {addr} 同时在 ∪组与排除表中\
                 （二者必须不交）〔01 设计 §9.2.1.3〕"
            ));
        }
    }
    for (addr, expected, _reason) in excluded {
        match point_table::lookup_bit(Role::Battery, *addr).map(|row| row.kind) {
            Some(RegPointKind::Bit(actual)) if actual == *expected => {}
            Some(RegPointKind::Bit(actual)) => {
                return Err(format!(
                    "south_stations: 生成期自检 G-4 ② 失败 —— 排除表位地址 {addr} 期望类别 {expected:?}、\
                     point_table 实际类别 {actual:?}（防把 Alarm 误标 Reserved 导致静默漏报）〔01 设计 §9.2.1.3〕"
                ));
            }
            _ => {
                return Err(format!(
                    "south_stations: 生成期自检 G-4 ② 失败 —— 排除表位地址 {addr} 在 point_table 中无对应的位点登记行\
                     〔01 设计 §9.2.1.3〕"
                ));
            }
        }
    }

    // ── G-3 扣减后全覆盖（**集合相等**，不是"覆盖"） ──
    let (alarm_state, _reserved) = battery_bit_classes();
    let after_deduct: BTreeSet<u16> = alarm_state.difference(&excluded_set).copied().collect();
    if after_deduct != union {
        let missing: Vec<u16> = union.difference(&after_deduct).copied().collect();
        let extra: Vec<u16> = after_deduct.difference(&union).copied().collect();
        return Err(format!(
            "south_stations: 生成期自检 G-3 失败 —— (Alarm ∪ State) − EXCLUDED ≠ ∪组（{} ≠ {}）；\
             未被聚合覆盖的位 {:?}；超出覆盖范围的位 {:?}〔01 设计 §9.2.1.3〕",
            after_deduct.len(),
            union.len(),
            missing,
            extra
        ));
    }
    if alarm_state.len() != ALARM_UNION_STATE_BITS
        || alarm_state.len() != AGGR_COVERED_BITS + AGGR_EXCLUDED_BITS
    {
        return Err(format!(
            "south_stations: 生成期自检 G-3 失败 —— card(Alarm ∪ State) = {}，期望 {} == {} + {}（等价计数式）\
             〔01 设计 §9.2.1.3〕",
            alarm_state.len(),
            ALARM_UNION_STATE_BITS,
            AGGR_COVERED_BITS,
            AGGR_EXCLUDED_BITS
        ));
    }
    Ok(())
}

// ───────────────────────────── 运行期：BMS 15 组聚合求值 ─────────────────────────────

/// `PointQuality` 的**降级序**（§9.1.2：`Ok < Stale < Invalid < Unconfigured`）。
fn quality_rank(q: PointQuality) -> u8 {
    match q {
        PointQuality::Ok => 0,
        PointQuality::Stale => 1,
        PointQuality::Invalid => 2,
        PointQuality::Unconfigured => 3,
    }
}

/// **BMS 15 组聚合的运行期求值**（纯函数，无 IO / 无锁；§9.2.1.1）。
///
/// 入参 `lookup(位地址) -> Option<(值, 质量)>`：`None` = **不可得**（快照中无该位）；
/// 出参 = `(聚合点名, 值可选, 质量)`，可直接喂 `LatestValues::apply`
/// （写入侧以 `PointId { station: 站 id, metric: 聚合点名 }` 落快照）。
///
/// # 求值规则（§9.2.1.1，逐条可机械核对）
///
/// ```text
/// 对每组 g:
///   可读 = { (v, q) | lookup(位地址) == Some((v,q)) }，None = 不可得
///   若 可读 为空      → (metric, None, Unconfigured)          // 全组不可得 ⇒ 严禁写 0
///   否则              值 = Some(OR(可读位的值))                // 任一位 1 ⇒ 聚合 1
///                     质量 = Ok（当且仅当"无缺位 且 可读位全为 Ok"）
///                            否则 worst(可读位质量, 缺位→Unconfigured)
/// ```
///
/// - **OR 语义** = "只并粒度、不丢语义"（任一位 1 ⇒ 聚合 1），**不是**"取最重一级"。
/// - **不可得的位不当作 0**：不参与 OR，且通过**质量降级**表达"这一组未读全"——
///   否则"某位读不到 ⇒ 聚合算成 0 ⇒ 主站看到无告警"就是一次**静默漏报**
///   （PRD §8.3.2 末注 / §8.3.2「不得因聚合而漏报」/ EX-6）。
/// - **全组不可得 ⇒ `value: None`**（**严禁写 0**）；写入侧按 EX-1 保留原值与原始时标。
/// - 质量降级时**保留原值**由**写入侧**执行（`PointValue { 上轮值, 上轮时标, 降级后质量 }`），
///   本函数只负责给出"降级后的质量"。
///
/// > ⚠️ **设计的两处口径差**（已登记，见任务汇报）：① §9.2.1.1 的伪码写"全部**可读位**
/// > `q == Ok` ⇒ `Ok`"（未提缺位），而同节的"不可得位不当作 0（…通过质量降级表达"这一组
/// > 未读全"）"与 §9.5 的用例清单（"组内部分缺位 ⇒ **质量降级**且不写 0"）要求**有缺位即降级**。
/// > 本实现取**后者**（安全侧：宁可标降级，不可把"未读全"报成 `Ok`）。
/// > ② 伪码"全组不可得 ⇒ **站位质量**"，但本函数签名（设计给定）**拿不到站位质量**
/// > ⇒ 取 `Unconfigured`（§9.1.2：点未配置/从未采集过），**写入侧可按站级状态覆盖**。
pub fn evaluate_bms_aggregates(
    lookup: &dyn Fn(u16) -> Option<(f64, PointQuality)>,
) -> Vec<(&'static str, Option<f64>, PointQuality)> {
    let mut out: Vec<(&'static str, Option<f64>, PointQuality)> =
        Vec::with_capacity(BMS_AGGR_GROUPS.len());
    for g in BMS_AGGR_GROUPS {
        let mut readable: Vec<(f64, PointQuality)> = Vec::with_capacity(g.bits.len());
        let mut missing = 0usize;
        for &addr in g.bits {
            match lookup(addr) {
                Some(pair) => readable.push(pair),
                None => missing += 1,
            }
        }
        if readable.is_empty() {
            out.push((g.metric, None, PointQuality::Unconfigured));
            continue;
        }
        let value = if readable.iter().any(|(v, _)| *v != 0.0) {
            1.0
        } else {
            0.0
        };
        let mut quality = PointQuality::Ok;
        for (_, q) in &readable {
            if quality_rank(*q) > quality_rank(quality) {
                quality = *q;
            }
        }
        if missing > 0 && quality_rank(PointQuality::Unconfigured) > quality_rank(quality) {
            quality = PointQuality::Unconfigured;
        }
        out.push((g.metric, Some(value), quality));
    }
    out
}

// ───────────────────────────── 单测（01 设计 §9.5「mupc-southd 单测」行） ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 参考配置（`tests/fixtures/south_stations_s3b2.yaml`）—— 与
    /// `tests/point_table_vs_reference_config.rs` 同一输入（不新建第二份点表真源）。
    /// **Task 6（ADR-016）起站级段为 5 站**（546 点），PCS 迁至独立顶层段 [`REF_PCS`]
    /// （72 点）⇒ 合计仍 618 点、上云契约零变化（设计 §13.9）。
    const REF_STATIONS: &str = include_str!("../tests/fixtures/south_stations_s3b2.yaml");

    /// PCS 独立顶层段（Task 6）。
    const REF_PCS: &str = include_str!("../tests/fixtures/south_pcs_s3b2.yaml");

    #[derive(serde::Deserialize)]
    struct Wrapper {
        south_stations: SouthStationsConfig,
    }

    /// 参考配置 = 站级段 5 站 **+ 由 `south_pcs` 段合成的 `Role::Pcs` 站**。
    ///
    /// 合成理由：`build_uplink_points` 的入参仍是 `&SouthStationsConfig`（`south_pcs` → 上云
    /// 的接线属后续 Task；§13.9 声明上云契约零变化），而本组用例要钉的正是"**PCS 启用**时
    /// 的三通道 / 档位点数"（639 点口径）。
    ///
    /// **合成走共用函数** [`SouthPcsConfig::station_shell`]（设计 §13.9 末要求③：生产与测试
    /// 共用同一函数）—— 此前本处手写、与 `mupc-core-bin` / `s3b2_decode_e2e` 两处各写一份，
    /// 会与生产漂移。
    fn cfg_ref() -> SouthStationsConfig {
        let mut cfg: SouthStationsConfig = serde_yaml::from_str::<Wrapper>(REF_STATIONS)
            .expect("参考配置解析失败")
            .south_stations;
        let pcs: crate::config::SouthPcsConfig =
            serde_yaml::from_str(REF_PCS).expect("south_pcs 参考配置解析失败");
        assert!(pcs.enabled, "参考 PCS 段须 enabled");
        cfg.stations.push(pcs.station_shell());
        cfg
    }

    /// 去掉 PCS 站（EX-7：站未启用 ⇒ 72 点不产条目）。
    fn cfg_ref_no_pcs() -> SouthStationsConfig {
        let mut cfg = cfg_ref();
        cfg.stations.retain(|s| s.role != Role::Pcs);
        cfg
    }

    fn count(points: &[UplinkPoint], mask: ChannelMask) -> usize {
        points.iter().filter(|p| p.channels.has(mask)).count()
    }

    fn get<'a>(points: &'a [UplinkPoint], station: &str, metric: &str) -> &'a UplinkPoint {
        points
            .iter()
            .find(|p| p.station == station && p.metric == metric)
            .unwrap_or_else(|| panic!("点表中未找到 {station}/{metric}"))
    }

    // ── 三通道点数 / 档位点数（G-5 正例） ──

    #[test]
    fn reference_config_pcs_enabled_matches_design_channel_counts() {
        let pts = build_uplink_points(&cfg_ref())
            .expect("参考配置（站级 5 站 + 合成 PCS 段）应通过全部生成期自检");
        assert_eq!(
            count(&pts, ChannelMask::IEC104),
            234,
            "IEC104 启用 PCS ⇒ 234"
        );
        assert_eq!(
            count(&pts, ChannelMask::MQTT),
            624,
            "MQTT 全量启用 PCS ⇒ 624"
        );
        assert_eq!(pts.len(), 639, "并集（快照规模）⇒ 639");

        let abc = [DataClass::A, DataClass::B, DataClass::C].map(|c| {
            pts.iter()
                .filter(|p| p.channels.has(ChannelMask::IEC104) && p.class == c)
                .count()
        });
        assert_eq!(
            abc,
            [22, 153, 59],
            "A/B/C 档（PCS 启用）须为 22/153/59（§9.2.2）"
        );

        // 逐站点数（PRD §8.3.1 表）：MQTT = 6/40/345/72/127/34 = 624
        //                        并集 = 6/40/360/72/127/34 = 639（bms 多 15 个 IEC104-only 聚合）
        for (station, mqtt_n, union_n) in [
            ("grid_meter", 6, 6),
            ("meter_batt", 40, 40),
            ("bms", 345, 360),
            ("pcs", 72, 72),
            ("fire", 127, 127),
            ("hvac", 34, 34),
        ] {
            let mqtt = pts
                .iter()
                .filter(|p| p.station == station && p.channels.has(ChannelMask::MQTT))
                .count();
            let all = pts.iter().filter(|p| p.station == station).count();
            assert_eq!(
                (mqtt, all),
                (mqtt_n, union_n),
                "站 {station} 的 MQTT / 并集点数"
            );
        }
    }

    #[test]
    fn reference_config_without_pcs_matches_design_channel_counts() {
        let pts =
            build_uplink_points(&cfg_ref_no_pcs()).expect("5 站（无 PCS）应通过全部生成期自检");
        assert_eq!(
            count(&pts, ChannelMask::IEC104),
            162,
            "IEC104 未启用 PCS ⇒ 162"
        );
        assert_eq!(count(&pts, ChannelMask::MQTT), 552, "MQTT 未启用 PCS ⇒ 552");
        assert_eq!(pts.len(), 567, "并集（快照规模）⇒ 567");

        let abc = [DataClass::A, DataClass::B, DataClass::C].map(|c| {
            pts.iter()
                .filter(|p| p.channels.has(ChannelMask::IEC104) && p.class == c)
                .count()
        });
        assert_eq!(
            abc,
            [19, 84, 59],
            "A/B/C 档（PCS 未启用）须为 19/84/59（§9.2.2）"
        );
        assert_eq!(
            CLASS_A.len(),
            22,
            "A 档表恒 22 项（PCS 三项在站缺失时不产点）"
        );
        assert!(!pts
            .iter()
            .any(|p| (SEG_PCS + 1..=SEG_PCS + 72).contains(&p.ioa)));
    }

    // ── 段基址 / 段内序（块 addr 升序） ──

    #[test]
    fn segment_bases_and_in_segment_order_follow_design() {
        let pts = build_uplink_points(&cfg_ref()).unwrap();

        // 段 1：grid 固定派生 6 点（IOA 1–6，现场追认、不得改号）
        for (ioa, metric, _) in GRID_DERIVED_6 {
            let p = get(&pts, "grid_meter", metric);
            assert_eq!(p.ioa, *ioa, "grid {metric} 须为 IOA {ioa}");
            assert_eq!(p.channels, ChannelMask::BOTH);
            assert_eq!(p.class, DataClass::A);
        }

        // 段 2：meter_batt 40 点按 (块 addr, 块内偏移) 升序 ⇒ 101–140
        // （§9.2.1.3「逐点 IOA（其余段）」表逐行核对）
        for (metric, ioa) in [
            ("mb_e_act_comb_1", 101),
            ("mb_e_rea_rev_1", 106),
            ("mb_ui_1", 107),
            ("mb_ui_6", 112),
            ("mb_freq_line_1", 113),
            ("mb_freq_line_4", 116),
            ("mb_phase_1", 117),
            ("mb_phase_14", 124),
            ("mb_power_1", 125),
            ("mb_power_25", 137),
            ("mb_power_28", 140),
        ] {
            assert_eq!(
                get(&pts, "meter_batt", metric).ioa,
                ioa,
                "meter_batt {metric}"
            );
        }

        // 段 3：bms 遥测 57 点 ⇒ 201–257（bms_io 31 / bms_energy 10 / bms_meta 8 / bms_term 4 / bms_cap 4）
        for (metric, ioa) in [
            ("bms_io_1", 201),
            ("bms_io_31", 231),
            ("bms_energy_1", 232),
            ("bms_energy_19", 241),
            ("bms_meta_1", 242),
            ("bms_meta_9", 249),
            ("bms_term_1", 250),
            ("bms_term_4", 253),
            ("bms_cap_1", 254),
            ("bms_cap_6", 257),
        ] {
            assert_eq!(get(&pts, "bms", metric).ioa, ioa, "bms 遥测 {metric}");
        }
        // `soc` = `bms_io_19` 的显式名（块内偏移 18 ⇒ 段内序 19）
        assert_eq!(get(&pts, "bms", "soc").ioa, 219, "soc 为 bms_io 第 19 点");
        assert!(
            !pts.iter().any(|p| p.metric == "bms_io_19"),
            "显式 name 覆盖位置命名 ⇒ 不得同时存在 bms_io_19"
        );

        // 段 5/6/7
        assert_eq!(get(&pts, "pcs", "pcs_3zone_1").ioa, 401);
        assert_eq!(
            get(&pts, "pcs", "pcs_3zone_75").ioa,
            472,
            "PCS 末点 = 段内序 72"
        );
        assert_eq!(get(&pts, "fire", "fire_sys_1").ioa, 501);
        assert_eq!(
            get(&pts, "fire", "fire_det_count").ioa,
            507,
            "fire_det_count = 配置 at:7"
        );
        assert_eq!(get(&pts, "fire", "fire_sys_13").ioa, 513);
        assert_eq!(get(&pts, "hvac", "hvac_di_1").ioa, 601);
        assert_eq!(get(&pts, "hvac", "hvac_di_31").ioa, 631);
    }

    #[test]
    fn iec104_points_have_nonzero_ioa_and_mqtt_only_are_zero() {
        let pts = build_uplink_points(&cfg_ref()).unwrap();
        for p in &pts {
            if p.channels.has(ChannelMask::IEC104) {
                assert!(
                    p.ioa > 0,
                    "{}/{} 进 IEC104 必须有非 0 IOA",
                    p.station,
                    p.metric
                );
            } else {
                assert_eq!(
                    p.ioa, 0,
                    "{}/{} 为 MQTT-only ⇒ ioa = 0",
                    p.station, p.metric
                );
            }
        }
        let mut ioas: Vec<u32> = pts
            .iter()
            .filter(|p| p.channels.has(ChannelMask::IEC104))
            .map(|p| p.ioa)
            .collect();
        ioas.sort_unstable();
        let before = ioas.len();
        ioas.dedup();
        assert_eq!(ioas.len(), before, "IEC104 IOA 不得重复");
    }

    // ── 子集规则 ──

    #[test]
    fn subset_rules_exclude_fire_det_hvac_in_and_battery_bits_from_iec104() {
        let pts = build_uplink_points(&cfg_ref()).unwrap();

        // fire：仅 fire_sys 13 点进 IEC104；fire_det 114 点为 MQTT-only
        let fire_iec: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.station == "fire" && p.channels.has(ChannelMask::IEC104))
            .collect();
        assert_eq!(fire_iec.len(), 13);
        assert!(fire_iec
            .iter()
            .all(|p| p.metric.starts_with("fire_sys") || p.metric == "fire_det_count"));
        let fire_det: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.metric.starts_with("fire_det_") && p.metric != "fire_det_count")
            .collect();
        assert_eq!(fire_det.len(), 114, "探测器明细 114 点（n=20）");
        assert!(
            fire_det
                .iter()
                .all(|p| p.channels == ChannelMask::MQTT && p.ioa == 0),
            "fire_det 不得进 IEC104（§9.2.1 子集规则）"
        );

        // hvac：仅 discrete 块 31 位进 IEC104；hvac_in 3 点为 MQTT-only
        let hvac_iec: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.station == "hvac" && p.channels.has(ChannelMask::IEC104))
            .collect();
        assert_eq!(hvac_iec.len(), 31);
        assert!(hvac_iec.iter().all(|p| p.metric.starts_with("hvac_di_")));
        let hvac_in: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.metric.starts_with("hvac_in_"))
            .collect();
        assert_eq!(hvac_in.len(), 3, "温湿度 3 点（PRD Q4：仅 MQTT）");
        assert!(hvac_in
            .iter()
            .all(|p| p.channels == ChannelMask::MQTT && p.ioa == 0));

        // bms：288 个告警位不进 IEC104（MQTT-only），15 个聚合为 IEC104-only
        let bms_bits: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.metric.starts_with("bms_alarm_"))
            .collect();
        assert_eq!(bms_bits.len(), 288);
        assert!(bms_bits
            .iter()
            .all(|p| p.channels == ChannelMask::MQTT && p.ioa == 0));
        let aggr: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.metric.starts_with("bms_aggr_"))
            .collect();
        assert_eq!(aggr.len(), 15);
        assert!(aggr.iter().all(|p| p.channels == ChannelMask::IEC104));
        assert!(
            aggr.iter()
                .all(|p| p.class == DataClass::C && p.kind == UplinkKind::Bit),
            "聚合点 = 位点 + C 档（COS 上送）"
        );
    }

    // ── 15 组聚合：IOA 301–315 / 逐组点名 ──

    #[test]
    fn aggregates_are_iec104_only_on_ioa_301_to_315_in_group_order() {
        let pts = build_uplink_points(&cfg_ref()).unwrap();
        let expected = [
            "bms_aggr_cluster_voltage",
            "bms_aggr_cluster_current",
            "bms_aggr_cell_voltage",
            "bms_aggr_cell_temp",
            "bms_aggr_soc_low",
            "bms_aggr_soh_low",
            "bms_aggr_cell_spread",
            "bms_aggr_slave_comm_lost",
            "bms_aggr_terminal_pack",
            "bms_aggr_acquire_fault",
            "bms_aggr_slave_di",
            "bms_aggr_run_abnormal",
            "bms_aggr_cluster_level",
            "bms_aggr_relay_stuck",
            "bms_aggr_afe_fault",
        ];
        assert_eq!(BMS_AGGR_GROUPS.len(), 15);
        for (i, metric) in expected.iter().enumerate() {
            let p = get(&pts, "bms", metric);
            assert_eq!(
                p.ioa,
                SEG_BMS_ALARM + 1 + i as u32,
                "聚合 {metric} 段内序 {}",
                i + 1
            );
        }
        assert_eq!(get(&pts, "bms", expected[14]).ioa, 315, "段 4 末点 IOA 315");
    }

    /// **裁定 A** 的 10 位：424–426 / 454–459 / 460 —— 逐位入组（313/314/315）且**不在**排除表。
    #[test]
    fn ruling_a_ten_bits_are_in_groups_and_absent_from_excluded() {
        assert_eq!(BMS_AGGR_GROUPS[12].metric, "bms_aggr_cluster_level");
        assert_eq!(BMS_AGGR_GROUPS[12].bits, &[424, 425, 426]);
        assert_eq!(BMS_AGGR_GROUPS[13].metric, "bms_aggr_relay_stuck");
        assert_eq!(BMS_AGGR_GROUPS[13].bits, &[454, 455, 456, 457, 458, 459]);
        assert_eq!(BMS_AGGR_GROUPS[14].metric, "bms_aggr_afe_fault");
        assert_eq!(BMS_AGGR_GROUPS[14].bits, &[460]);
        for addr in 424..=426u16 {
            assert!(
                !BMS_AGGR_EXCLUDED.iter().any(|(a, ..)| *a == addr),
                "{addr} 应已移出排除表"
            );
        }
        for addr in 454..=460u16 {
            assert!(
                !BMS_AGGR_EXCLUDED.iter().any(|(a, ..)| *a == addr),
                "{addr} 应已移出排除表"
            );
        }
        // 这 10 位在 point_table 中均为 Alarm（逐位回源）
        for addr in [424u16, 425, 426, 454, 455, 456, 457, 458, 459, 460] {
            let row = point_table::lookup_bit(Role::Battery, addr).expect("登记行须存在");
            assert_eq!(
                row.kind,
                RegPointKind::Bit(BitClass::Alarm),
                "位 {addr} 应为 Alarm"
            );
        }
    }

    // ── G-1～G-5 正例（表级） ──

    #[test]
    fn g2_groups_cover_143_bits_and_are_pairwise_disjoint() {
        let sum: usize = BMS_AGGR_GROUPS.iter().map(|g| g.bits.len()).sum();
        assert_eq!(sum, AGGR_COVERED_BITS, "各组基数之和 == 143");
        let mut union: BTreeSet<u16> = BTreeSet::new();
        for g in BMS_AGGR_GROUPS {
            for &b in g.bits {
                assert!(union.insert(b), "位 {b} 被两组引用（G-2 互斥失败）");
            }
        }
        assert_eq!(union.len(), 143);
    }

    #[test]
    fn g4_excluded_table_has_78_rows_matching_point_table_classes() {
        let distinct: BTreeSet<u16> = BMS_AGGR_EXCLUDED.iter().map(|(a, ..)| *a).collect();
        assert_eq!(
            BMS_AGGR_EXCLUDED.len(),
            AGGR_EXCLUDED_BITS,
            "逐位展开 78 行"
        );
        assert_eq!(distinct.len(), AGGR_EXCLUDED_BITS, "无重复位");
        for (addr, expected, reason) in BMS_AGGR_EXCLUDED {
            assert!(!reason.is_empty(), "位 {addr} 须给出不入聚合的理由");
            let row = point_table::lookup_bit(Role::Battery, *addr).expect("登记行须存在");
            assert_eq!(
                row.kind,
                RegPointKind::Bit(*expected),
                "排除表期望类别须与 point_table 一致（位 {addr}）"
            );
        }
    }

    #[test]
    fn g3_deducted_union_equals_alarm_union_state_minus_excluded() {
        let (alarm_state, reserved) = battery_bit_classes();
        assert_eq!(
            alarm_state.len(),
            ALARM_UNION_STATE_BITS,
            "card(Alarm ∪ State) == 221"
        );
        assert_eq!(reserved.len(), 67, "Reserved 67 位不参与任何判据");
        assert_eq!(alarm_state.len() + reserved.len(), 288, "位块合计 288 位");
        assert_eq!(
            alarm_state.len(),
            AGGR_COVERED_BITS + AGGR_EXCLUDED_BITS,
            "221 == 143 + 78"
        );

        let mut covered: BTreeSet<u16> = BTreeSet::new();
        for g in BMS_AGGR_GROUPS {
            covered.extend(g.bits.iter().copied());
        }
        let excluded: BTreeSet<u16> = BMS_AGGR_EXCLUDED.iter().map(|(a, ..)| *a).collect();
        let after: BTreeSet<u16> = alarm_state.difference(&excluded).copied().collect();
        assert_eq!(
            after, covered,
            "(Alarm ∪ State) − EXCLUDED 必须**等于** ∪组（集合相等）"
        );
        assert!(covered.is_disjoint(&excluded), "G-4 ①：∪组 与排除表不交");
    }

    // ── G-1～G-4 反例（注入合成表，逐条证明自检会 Err） ──

    /// 合成一张"引用 Reserved 位 200"的组表（G-1 的 `BitClass` 判据）。
    /// 200 在 `point_table` 中为 `Reserved`；G-1 只接受 `Alarm / State`。
    fn reserved_bit_groups() -> Vec<AggrGroup> {
        let mut g: Vec<AggrGroup> = BMS_AGGR_GROUPS.to_vec();
        g.push(AggrGroup {
            metric: "bms_aggr_bad_reserved_ref",
            bits: &[200],
            label: "合成反例：位 200 为 Reserved",
        });
        g
    }

    #[test]
    fn g1_rejects_bit_outside_config_bit_universe() {
        let universe: BTreeSet<u16> = (100..=387u16).collect(); // 模拟 bms_alarm addr 被改成 100
        let e = check_aggr_consistency(BMS_AGGR_GROUPS, BMS_AGGR_EXCLUDED, Some(&universe))
            .expect_err("G-1 应拒");
        // 100..387 覆盖 201–236 等前段组；首个落空的是第 13 组的 424
        assert!(e.contains("G-1") && e.contains("424"), "实际: {e}");

        let ok: BTreeSet<u16> = (200..=487u16).collect();
        assert!(check_aggr_consistency(BMS_AGGR_GROUPS, BMS_AGGR_EXCLUDED, Some(&ok)).is_ok());
    }

    #[test]
    fn g1_rejects_reserved_bit_reference() {
        let e = check_aggr_consistency(&reserved_bit_groups(), BMS_AGGR_EXCLUDED, None)
            .expect_err("G-1 应拒 Reserved 位引用");
        assert!(e.contains("G-1") && e.contains("Reserved"), "实际: {e}");
    }

    #[test]
    fn g2_rejects_overlapping_groups() {
        let mut g: Vec<AggrGroup> = BMS_AGGR_GROUPS.to_vec();
        g[14].bits = BMS_AGGR_GROUPS[13].bits; // 末组与第 14 组相交
        let e = check_aggr_consistency(&g, BMS_AGGR_EXCLUDED, None).expect_err("G-2 应拒");
        assert!(e.contains("G-2"), "实际: {e}");
    }

    #[test]
    fn g4_rejects_excluded_row_inside_groups() {
        let mut ex: Vec<(u16, BitClass, &str)> = BMS_AGGR_EXCLUDED.to_vec();
        ex[4] = (201, BitClass::Alarm, "合成反例：与 ∪组 相交");
        let e = check_aggr_consistency(BMS_AGGR_GROUPS, &ex, None).expect_err("G-4 ① 应拒");
        assert!(e.contains("G-4") && e.contains("①"), "实际: {e}");
    }

    #[test]
    fn g4_rejects_excluded_row_with_wrong_expected_class() {
        let mut ex: Vec<(u16, BitClass, &str)> = BMS_AGGR_EXCLUDED.to_vec();
        // 288 在 point_table 中实为 State；期望写成 Alarm ⇒ 必须被 G-4 ② 拒
        // （就地改第 0 行，避免引入重复位而先撞上 G-4 ③）
        ex[0] = (288, BitClass::Alarm, "合成反例：类别误标");
        let e = check_aggr_consistency(BMS_AGGR_GROUPS, &ex, None).expect_err("G-4 ② 应拒");
        assert!(e.contains("G-4") && e.contains("②"), "实际: {e}");
    }

    #[test]
    fn g4_rejects_excluded_table_with_wrong_cardinality() {
        let mut ex: Vec<(u16, BitClass, &str)> = BMS_AGGR_EXCLUDED.to_vec();
        ex.truncate(77);
        let e = check_aggr_consistency(BMS_AGGR_GROUPS, &ex, None).expect_err("G-4 ③ 应拒");
        assert!(e.contains("G-4") && e.contains("78"), "实际: {e}");

        // 用重复行凑够 78 行（"凑数糊过去"）同样拒
        let mut dup: Vec<(u16, BitClass, &str)> = BMS_AGGR_EXCLUDED.to_vec();
        dup[5] = dup[4];
        let e2 = check_aggr_consistency(BMS_AGGR_GROUPS, &dup, None).expect_err("G-4 ③ 应拒");
        assert!(e2.contains("去重后"), "实际: {e2}");
    }

    #[test]
    fn g3_rejects_coverage_gap() {
        // 用 1 位 Reserved（200，非 Alarm∪State）占用排除表的一行 ⇒ 扣减后集合比 ∪组 多 1 位
        // （144 ≠ 143）；G-1/G-2/G-4 三条均通过 ⇒ 唯 G-3 拒。
        let mut ex: Vec<(u16, BitClass, &str)> = BMS_AGGR_EXCLUDED.to_vec();
        ex[4] = (200, BitClass::Reserved, "合成反例：非 Alarm∪State 位占位");
        let e = check_aggr_consistency(BMS_AGGR_GROUPS, &ex, None).expect_err("G-3 应拒");
        assert!(e.contains("G-3"), "实际: {e}");
    }

    // ── 生成期自检：端到端（配置漂移 ⇒ 拒启动） ──

    #[test]
    fn g1_rejects_battery_alarm_block_address_drift() {
        let mut cfg = cfg_ref();
        let bms = cfg
            .stations
            .iter_mut()
            .find(|s| s.role == Role::Battery)
            .unwrap();
        let blk = bms.regs.iter_mut().find(|b| b.name == "bms_alarm").unwrap();
        blk.addr = 100; // 位地址宇宙变成 100..387 ⇒ 聚合引用的 388..460 落空
        let e = build_uplink_points(&cfg).expect_err("配置漂移应拒启动");
        assert!(e.contains("G-1"), "实际: {e}");
    }

    #[test]
    fn g5_rejects_config_with_missing_station() {
        let mut cfg = cfg_ref();
        cfg.stations.retain(|s| s.role != Role::Hvac);
        let e = build_uplink_points(&cfg).expect_err("缺 hvac 站应拒启动");
        // 缺 hvac（31 位 + 3 温湿度）⇒ IEC104 234−31 = 203、MQTT 624−34 = 590、并集 605
        assert!(
            e.contains("G-5") && e.contains("203") && e.contains("590") && e.contains("605"),
            "实际: {e}"
        );
    }

    #[test]
    fn per_role_point_table_drift_is_rejected_with_counts() {
        // 段 3 少 1 点（bms_cap 去掉 at:6）⇒ 电池遥测 56 ≠ 57，拒启动（防手写常量回归）
        let mut cfg = cfg_ref();
        let bms = cfg
            .stations
            .iter_mut()
            .find(|s| s.role == Role::Battery)
            .unwrap();
        let blk = bms.regs.iter_mut().find(|b| b.name == "bms_cap").unwrap();
        blk.points.pop();
        let e = build_uplink_points(&cfg).expect_err("点数漂移应拒");
        assert!(
            e.contains("段 3") && e.contains("实测 56") && e.contains("期望 57"),
            "实际: {e}"
        );
    }

    // ── IOA 由配置**机械生成**（非手写常量） ──

    #[test]
    fn ioa_is_derived_from_config_block_order() {
        // 把 mb_freq_line 的块地址挪到 mb_ui 之前 ⇒ 段内序随之改变（证明 IOA 来自配置）
        let mut cfg = cfg_ref();
        let mb = cfg
            .stations
            .iter_mut()
            .find(|s| s.role == Role::MeterBatt)
            .unwrap();
        let blk = mb
            .regs
            .iter_mut()
            .find(|b| b.name == "mb_freq_line")
            .unwrap();
        blk.addr = 0x0040; // 原 0x0077（在 mb_ui 0x0061 之后）
        let pts = build_uplink_points(&cfg).expect("点数未变，仍应通过自检");
        assert_eq!(
            get(&pts, "meter_batt", "mb_freq_line_1").ioa,
            107,
            "块序提前 ⇒ IOA 提前"
        );
        assert_eq!(get(&pts, "meter_batt", "mb_ui_1").ioa, 111);
        assert_eq!(count(&pts, ChannelMask::IEC104), 234, "总点数不因块序变化");
    }

    // ── 确定性 ──

    #[test]
    fn build_uplink_points_is_deterministic() {
        let cfg = cfg_ref();
        let a = build_uplink_points(&cfg).unwrap();
        let b = build_uplink_points(&cfg).unwrap();
        assert_eq!(a, b, "同一输入两次调用须相等");
        assert_eq!(
            format!("{a:?}"),
            format!("{b:?}"),
            "逐字节（Debug 渲染）相同 ⇒ 顺序稳定"
        );
    }

    // ── `evaluate_bms_aggregates`（§9.2.1.1） ──

    /// 构造 `位地址 → (值, 质量)` 的查询闭包（缺席位 ⇒ `None` = 不可得）。
    fn lookup_of(
        map: &BTreeMap<u16, (f64, PointQuality)>,
    ) -> impl Fn(u16) -> Option<(f64, PointQuality)> + '_ {
        move |addr| map.get(&addr).copied()
    }

    /// 只给"某组的部分位"填值（未列出的位 ⇒ `None`）。
    fn partial(
        group_index: usize,
        vals: &[(u16, f64, PointQuality)],
    ) -> BTreeMap<u16, (f64, PointQuality)> {
        let mut m: BTreeMap<u16, (f64, PointQuality)> = BTreeMap::new();
        for (a, v, q) in vals {
            assert!(
                BMS_AGGR_GROUPS[group_index].bits.contains(a),
                "反例位 {a} 须属第 {group_index} 组"
            );
            m.insert(*a, (*v, *q));
        }
        m
    }

    #[test]
    fn evaluate_returns_15_groups_in_group_order() {
        let all: BTreeMap<u16, (f64, PointQuality)> = BMS_AGGR_GROUPS
            .iter()
            .flat_map(|g| g.bits.iter().map(|&b| (b, (0.0, PointQuality::Ok))))
            .collect();
        let out = evaluate_bms_aggregates(&lookup_of(&all));
        assert_eq!(out.len(), 15);
        for (i, (metric, ..)) in out.iter().enumerate() {
            assert_eq!(*metric, BMS_AGGR_GROUPS[i].metric);
        }
    }

    #[test]
    fn evaluate_or_semantics_sets_one_when_any_bit_is_one() {
        let m = partial(
            0,
            &[(201, 0.0, PointQuality::Ok), (204, 1.0, PointQuality::Ok)],
        );
        let out = evaluate_bms_aggregates(&lookup_of(&m));
        assert_eq!(
            out[0].1,
            Some(1.0),
            "任一位 1 ⇒ 聚合 1（只并粒度、不丢语义）"
        );
        assert_eq!(out[0].2, PointQuality::Unconfigured, "缺位仍在 ⇒ 质量降级");

        let six: Vec<(u16, f64, PointQuality)> =
            (201..=206).map(|a| (a, 0.0, PointQuality::Ok)).collect();
        let out0 = evaluate_bms_aggregates(&lookup_of(&partial(0, &six)));
        assert_eq!(out0[0].1, Some(0.0));
        assert_eq!(out0[0].2, PointQuality::Ok, "无缺位且全部可读位 Ok ⇒ Ok");
    }

    #[test]
    fn evaluate_partial_missing_degrades_quality_and_never_fabricates_zero() {
        // 组内部分缺位：值 = 可读位的 OR（此处全 0 ⇒ 0.0），但质量必须降级（不得报 Ok）
        let m = partial(1, &[(207, 0.0, PointQuality::Ok)]);
        let out = evaluate_bms_aggregates(&lookup_of(&m));
        assert_eq!(out[1].0, "bms_aggr_cluster_current");
        assert_eq!(
            out[1].1,
            Some(0.0),
            "可读位 OR = 0（缺位**不当作 0 参与 OR**）"
        );
        assert_eq!(
            out[1].2,
            PointQuality::Unconfigured,
            "缺位 ⇒ 质量降级（这一组未读全）"
        );
        assert_ne!(out[1].2, PointQuality::Ok, "缺口不得被报成健康态");

        // 缺 1 位 + 另 1 位 Invalid ⇒ 取最差（Unconfigured 秩最高）
        let m2 = partial(1, &[(207, 0.0, PointQuality::Invalid)]);
        let out2 = evaluate_bms_aggregates(&lookup_of(&m2));
        assert_eq!(out2[1].2, PointQuality::Unconfigured);
    }

    #[test]
    fn evaluate_all_missing_returns_none_not_zero() {
        let empty: BTreeMap<u16, (f64, PointQuality)> = BTreeMap::new();
        let out = evaluate_bms_aggregates(&lookup_of(&empty));
        assert_eq!(out.len(), 15);
        for (metric, v, q) in &out {
            assert!(
                v.is_none(),
                "{metric} 全组不可得 ⇒ value = None（严禁写 0）"
            );
            assert_eq!(*q, PointQuality::Unconfigured);
        }
        // 闭包恒 `None` 亦同
        let out2 = evaluate_bms_aggregates(&|_addr: u16| None);
        assert!(out2.iter().all(|(_, v, _)| v.is_none()));
        assert_eq!(out2.len(), 15);
    }

    #[test]
    fn evaluate_worst_quality_wins_within_group() {
        let m = partial(
            0,
            &[
                (201, 0.0, PointQuality::Ok),
                (202, 0.0, PointQuality::Ok),
                (203, 0.0, PointQuality::Ok),
                (204, 0.0, PointQuality::Ok),
                (205, 0.0, PointQuality::Ok),
                (206, 0.0, PointQuality::Invalid),
            ],
        );
        let out = evaluate_bms_aggregates(&lookup_of(&m));
        assert_eq!(
            out[0].2,
            PointQuality::Invalid,
            "Ok < Stale < Invalid < Unconfigured"
        );

        let m2 = partial(
            0,
            &[
                (201, 0.0, PointQuality::Ok),
                (202, 0.0, PointQuality::Ok),
                (203, 0.0, PointQuality::Ok),
                (204, 0.0, PointQuality::Ok),
                (205, 0.0, PointQuality::Stale),
                (206, 0.0, PointQuality::Invalid),
            ],
        );
        let out2 = evaluate_bms_aggregates(&lookup_of(&m2));
        assert_eq!(
            out2[0].2,
            PointQuality::Invalid,
            "取最差（Stale 与 Invalid 之间取 Invalid）"
        );
    }

    #[test]
    fn evaluate_reports_single_bit_group_correctly() {
        // 第 15 组（AFE 故障）只有 1 位：460
        let mut m: BTreeMap<u16, (f64, PointQuality)> = BTreeMap::new();
        m.insert(460, (1.0, PointQuality::Ok));
        let out = evaluate_bms_aggregates(&lookup_of(&m));
        assert_eq!(out[14], ("bms_aggr_afe_fault", Some(1.0), PointQuality::Ok));
        assert_eq!(
            out.iter().filter(|(_, v, _)| v.is_none()).count(),
            14,
            "其余 14 组全不可得"
        );
    }
}
