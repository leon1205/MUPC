//! U-73 外设**白名单 + 短标签表 + 分组标题 + UI 固定文案**（设计 §15.11 #3 的**完整落点**）。
//!
//! 本文件承载四件事，**同一批字面量、同一处源码**：
//! - [`PERIPH_WHITELIST`]：白名单（`(role, block, at)` 常量数组）——W-1（结构性排除项不在表内）
//!   与 W-4（"行数相等"断言的可写前提）由本表成立；
//! - [`PERIPH_LABELS`]：**屏用短标签表**（与白名单**同序、同长**：逐行 `(短标签, 单位)`）——
//!   W-2（`unit` 唯一真源）/ W-3（每一点都有屏上文案）/ H-3（行数相等）由两表对齐成立；
//!   [`label_for`] / [`unit_for`] 是它的**位置式查询口**（catalog 构建器只走这两个口）；
//! - [`group_of`] / [`GROUP_TITLES`]：页内分组键 → 中文分组标题（§15.4 / §15.5.2 / §15.7.3）；
//! - [`ui_text`] 与消防枚举 / 位语义 / 拆解常量：**UI 固定文案与其余上屏中文**（§15.7.3）。
//!
//! **为什么全部放在本 crate**（F-6 / §15.7）：上屏中文若以**运行时字符串**（帧 / catalog）
//! 抵达 HMI，`local-display` 既有的码表覆盖率扫描（只扫 `ui/**` + `state.rs`）**看不见**
//! ⇒ 真机上出豆腐块而门禁全绿。把上屏中文收回 `display-proto`（`local-display` 的**依赖**）
//! 后，T-23 的待查集合 = 短标签表 ∪ 分组标题 ∪ UI 固定文案，盲区**结构性消除**。
//! 同理，消防的枚举 / 位语义 / 拆解文案（T20 曾落在 `mupc-core-bin`）也必须住在这里。
//!
//! **与登记 `label` 的分工**（F-4 / D22）：`PERIPH_LABELS` 是**精炼短标签**（无全角括号、
//! 无登记说明文本、无单位后缀）；`mupc-southd::point_table` 的登记 `label` 只作**交叉佐证**，
//! **不得**直上屏（设计阶段实测缺口 **189** 码位，且界面噪音大）。
//!
//! ⚠️ **两条不可回退的纪律**：
//! 1. `PERIPH_WHITELIST` 与 `PERIPH_LABELS` **同序同长**（各 447 行）——任一侧增删必须在
//!    **同一次改动**里同步另一侧，否则 W-3 的"行数相等"断言与人工逐块比对同时失效
//!    （H-3 的断言在下方 `#[cfg(test)]` 内）；
//! 2. `fire_det` 块在两表中都是**模板 6 行**（`at` 1..=6），运行期按 `at = 6(k−2)+j` 展开
//!    ⇒ [`label_for`] / [`unit_for`] **自带归约**（调用方可直接传展开后的 `at`）；
//!    [`contains`] 保持原口径（要求调用方先归约）——两者语义不同，**不要**顺手统一。

use crate::peripherals::PeriphRole;

/// 未登记 / 不在白名单点的分组键（**不臆造**；HMI 侧无对应标题 ⇒ 该点不上屏）。
pub const GROUP_UNKNOWN: &str = "unknown";

pub const PERIPH_WHITELIST: &[(PeriphRole, &str, u16)] = &[
    // ── Hvac / hvac_in（3 点）──
    (PeriphRole::Hvac, "hvac_in", 1),
    (PeriphRole::Hvac, "hvac_in", 3),
    (PeriphRole::Hvac, "hvac_in", 4),
    // ── Hvac / hvac_di（30 点）──
    (PeriphRole::Hvac, "hvac_di", 1),
    (PeriphRole::Hvac, "hvac_di", 2),
    (PeriphRole::Hvac, "hvac_di", 3),
    (PeriphRole::Hvac, "hvac_di", 4),
    (PeriphRole::Hvac, "hvac_di", 5),
    (PeriphRole::Hvac, "hvac_di", 6),
    (PeriphRole::Hvac, "hvac_di", 7),
    (PeriphRole::Hvac, "hvac_di", 8),
    (PeriphRole::Hvac, "hvac_di", 9),
    (PeriphRole::Hvac, "hvac_di", 10),
    (PeriphRole::Hvac, "hvac_di", 11),
    (PeriphRole::Hvac, "hvac_di", 12),
    (PeriphRole::Hvac, "hvac_di", 13),
    (PeriphRole::Hvac, "hvac_di", 14),
    (PeriphRole::Hvac, "hvac_di", 15),
    (PeriphRole::Hvac, "hvac_di", 16),
    (PeriphRole::Hvac, "hvac_di", 17),
    (PeriphRole::Hvac, "hvac_di", 18),
    (PeriphRole::Hvac, "hvac_di", 19),
    (PeriphRole::Hvac, "hvac_di", 20),
    (PeriphRole::Hvac, "hvac_di", 21),
    (PeriphRole::Hvac, "hvac_di", 22),
    (PeriphRole::Hvac, "hvac_di", 23),
    (PeriphRole::Hvac, "hvac_di", 24),
    (PeriphRole::Hvac, "hvac_di", 25),
    (PeriphRole::Hvac, "hvac_di", 27),
    (PeriphRole::Hvac, "hvac_di", 28),
    (PeriphRole::Hvac, "hvac_di", 29),
    (PeriphRole::Hvac, "hvac_di", 30),
    (PeriphRole::Hvac, "hvac_di", 31),
    // ── Fire / fire_sys（13 点）──
    (PeriphRole::Fire, "fire_sys", 1),
    (PeriphRole::Fire, "fire_sys", 2),
    (PeriphRole::Fire, "fire_sys", 3),
    (PeriphRole::Fire, "fire_sys", 4),
    (PeriphRole::Fire, "fire_sys", 5),
    (PeriphRole::Fire, "fire_sys", 6),
    (PeriphRole::Fire, "fire_sys", 7),
    (PeriphRole::Fire, "fire_sys", 8),
    (PeriphRole::Fire, "fire_sys", 9),
    (PeriphRole::Fire, "fire_sys", 10),
    (PeriphRole::Fire, "fire_sys", 11),
    (PeriphRole::Fire, "fire_sys", 12),
    (PeriphRole::Fire, "fire_sys", 13),
    // ── Fire / fire_det（6 点）──
    (PeriphRole::Fire, "fire_det", 1),
    (PeriphRole::Fire, "fire_det", 2),
    (PeriphRole::Fire, "fire_det", 3),
    (PeriphRole::Fire, "fire_det", 4),
    (PeriphRole::Fire, "fire_det", 5),
    (PeriphRole::Fire, "fire_det", 6),
    // ── Battery / bms_io（17 点）──
    (PeriphRole::Battery, "bms_io", 2),
    (PeriphRole::Battery, "bms_io", 3),
    (PeriphRole::Battery, "bms_io", 16),
    (PeriphRole::Battery, "bms_io", 17),
    (PeriphRole::Battery, "bms_io", 18),
    (PeriphRole::Battery, "bms_io", 20),
    (PeriphRole::Battery, "bms_io", 21),
    (PeriphRole::Battery, "bms_io", 22),
    (PeriphRole::Battery, "bms_io", 23),
    (PeriphRole::Battery, "bms_io", 24),
    (PeriphRole::Battery, "bms_io", 25),
    (PeriphRole::Battery, "bms_io", 26),
    (PeriphRole::Battery, "bms_io", 27),
    (PeriphRole::Battery, "bms_io", 28),
    (PeriphRole::Battery, "bms_io", 29),
    (PeriphRole::Battery, "bms_io", 30),
    (PeriphRole::Battery, "bms_io", 31),
    // ── Battery / bms_energy（9 点）──
    (PeriphRole::Battery, "bms_energy", 1),
    (PeriphRole::Battery, "bms_energy", 3),
    (PeriphRole::Battery, "bms_energy", 5),
    (PeriphRole::Battery, "bms_energy", 7),
    (PeriphRole::Battery, "bms_energy", 9),
    (PeriphRole::Battery, "bms_energy", 11),
    (PeriphRole::Battery, "bms_energy", 13),
    (PeriphRole::Battery, "bms_energy", 17),
    (PeriphRole::Battery, "bms_energy", 19),
    // ── Battery / bms_meta（8 点）──
    (PeriphRole::Battery, "bms_meta", 1),
    (PeriphRole::Battery, "bms_meta", 2),
    (PeriphRole::Battery, "bms_meta", 3),
    (PeriphRole::Battery, "bms_meta", 4),
    (PeriphRole::Battery, "bms_meta", 5),
    (PeriphRole::Battery, "bms_meta", 6),
    (PeriphRole::Battery, "bms_meta", 7),
    (PeriphRole::Battery, "bms_meta", 9),
    // ── Battery / bms_term（4 点）──
    (PeriphRole::Battery, "bms_term", 1),
    (PeriphRole::Battery, "bms_term", 2),
    (PeriphRole::Battery, "bms_term", 3),
    (PeriphRole::Battery, "bms_term", 4),
    // ── Battery / bms_cap（4 点）──
    (PeriphRole::Battery, "bms_cap", 1),
    (PeriphRole::Battery, "bms_cap", 3),
    (PeriphRole::Battery, "bms_cap", 5),
    (PeriphRole::Battery, "bms_cap", 6),
    // ── Battery / bms_alarm（288 点）──
    (PeriphRole::Battery, "bms_alarm", 1),
    (PeriphRole::Battery, "bms_alarm", 2),
    (PeriphRole::Battery, "bms_alarm", 3),
    (PeriphRole::Battery, "bms_alarm", 4),
    (PeriphRole::Battery, "bms_alarm", 5),
    (PeriphRole::Battery, "bms_alarm", 6),
    (PeriphRole::Battery, "bms_alarm", 7),
    (PeriphRole::Battery, "bms_alarm", 8),
    (PeriphRole::Battery, "bms_alarm", 9),
    (PeriphRole::Battery, "bms_alarm", 10),
    (PeriphRole::Battery, "bms_alarm", 11),
    (PeriphRole::Battery, "bms_alarm", 12),
    (PeriphRole::Battery, "bms_alarm", 13),
    (PeriphRole::Battery, "bms_alarm", 14),
    (PeriphRole::Battery, "bms_alarm", 15),
    (PeriphRole::Battery, "bms_alarm", 16),
    (PeriphRole::Battery, "bms_alarm", 17),
    (PeriphRole::Battery, "bms_alarm", 18),
    (PeriphRole::Battery, "bms_alarm", 19),
    (PeriphRole::Battery, "bms_alarm", 20),
    (PeriphRole::Battery, "bms_alarm", 21),
    (PeriphRole::Battery, "bms_alarm", 22),
    (PeriphRole::Battery, "bms_alarm", 23),
    (PeriphRole::Battery, "bms_alarm", 24),
    (PeriphRole::Battery, "bms_alarm", 25),
    (PeriphRole::Battery, "bms_alarm", 26),
    (PeriphRole::Battery, "bms_alarm", 27),
    (PeriphRole::Battery, "bms_alarm", 28),
    (PeriphRole::Battery, "bms_alarm", 29),
    (PeriphRole::Battery, "bms_alarm", 30),
    (PeriphRole::Battery, "bms_alarm", 31),
    (PeriphRole::Battery, "bms_alarm", 32),
    (PeriphRole::Battery, "bms_alarm", 33),
    (PeriphRole::Battery, "bms_alarm", 34),
    (PeriphRole::Battery, "bms_alarm", 35),
    (PeriphRole::Battery, "bms_alarm", 36),
    (PeriphRole::Battery, "bms_alarm", 37),
    (PeriphRole::Battery, "bms_alarm", 38),
    (PeriphRole::Battery, "bms_alarm", 39),
    (PeriphRole::Battery, "bms_alarm", 40),
    (PeriphRole::Battery, "bms_alarm", 41),
    (PeriphRole::Battery, "bms_alarm", 42),
    (PeriphRole::Battery, "bms_alarm", 43),
    (PeriphRole::Battery, "bms_alarm", 44),
    (PeriphRole::Battery, "bms_alarm", 45),
    (PeriphRole::Battery, "bms_alarm", 46),
    (PeriphRole::Battery, "bms_alarm", 47),
    (PeriphRole::Battery, "bms_alarm", 48),
    (PeriphRole::Battery, "bms_alarm", 49),
    (PeriphRole::Battery, "bms_alarm", 50),
    (PeriphRole::Battery, "bms_alarm", 51),
    (PeriphRole::Battery, "bms_alarm", 52),
    (PeriphRole::Battery, "bms_alarm", 53),
    (PeriphRole::Battery, "bms_alarm", 54),
    (PeriphRole::Battery, "bms_alarm", 55),
    (PeriphRole::Battery, "bms_alarm", 56),
    (PeriphRole::Battery, "bms_alarm", 57),
    (PeriphRole::Battery, "bms_alarm", 58),
    (PeriphRole::Battery, "bms_alarm", 59),
    (PeriphRole::Battery, "bms_alarm", 60),
    (PeriphRole::Battery, "bms_alarm", 61),
    (PeriphRole::Battery, "bms_alarm", 62),
    (PeriphRole::Battery, "bms_alarm", 63),
    (PeriphRole::Battery, "bms_alarm", 64),
    (PeriphRole::Battery, "bms_alarm", 65),
    (PeriphRole::Battery, "bms_alarm", 66),
    (PeriphRole::Battery, "bms_alarm", 67),
    (PeriphRole::Battery, "bms_alarm", 68),
    (PeriphRole::Battery, "bms_alarm", 69),
    (PeriphRole::Battery, "bms_alarm", 70),
    (PeriphRole::Battery, "bms_alarm", 71),
    (PeriphRole::Battery, "bms_alarm", 72),
    (PeriphRole::Battery, "bms_alarm", 73),
    (PeriphRole::Battery, "bms_alarm", 74),
    (PeriphRole::Battery, "bms_alarm", 75),
    (PeriphRole::Battery, "bms_alarm", 76),
    (PeriphRole::Battery, "bms_alarm", 77),
    (PeriphRole::Battery, "bms_alarm", 78),
    (PeriphRole::Battery, "bms_alarm", 79),
    (PeriphRole::Battery, "bms_alarm", 80),
    (PeriphRole::Battery, "bms_alarm", 81),
    (PeriphRole::Battery, "bms_alarm", 82),
    (PeriphRole::Battery, "bms_alarm", 83),
    (PeriphRole::Battery, "bms_alarm", 84),
    (PeriphRole::Battery, "bms_alarm", 85),
    (PeriphRole::Battery, "bms_alarm", 86),
    (PeriphRole::Battery, "bms_alarm", 87),
    (PeriphRole::Battery, "bms_alarm", 88),
    (PeriphRole::Battery, "bms_alarm", 89),
    (PeriphRole::Battery, "bms_alarm", 90),
    (PeriphRole::Battery, "bms_alarm", 91),
    (PeriphRole::Battery, "bms_alarm", 92),
    (PeriphRole::Battery, "bms_alarm", 93),
    (PeriphRole::Battery, "bms_alarm", 94),
    (PeriphRole::Battery, "bms_alarm", 95),
    (PeriphRole::Battery, "bms_alarm", 96),
    (PeriphRole::Battery, "bms_alarm", 97),
    (PeriphRole::Battery, "bms_alarm", 98),
    (PeriphRole::Battery, "bms_alarm", 99),
    (PeriphRole::Battery, "bms_alarm", 100),
    (PeriphRole::Battery, "bms_alarm", 101),
    (PeriphRole::Battery, "bms_alarm", 102),
    (PeriphRole::Battery, "bms_alarm", 103),
    (PeriphRole::Battery, "bms_alarm", 104),
    (PeriphRole::Battery, "bms_alarm", 105),
    (PeriphRole::Battery, "bms_alarm", 106),
    (PeriphRole::Battery, "bms_alarm", 107),
    (PeriphRole::Battery, "bms_alarm", 108),
    (PeriphRole::Battery, "bms_alarm", 109),
    (PeriphRole::Battery, "bms_alarm", 110),
    (PeriphRole::Battery, "bms_alarm", 111),
    (PeriphRole::Battery, "bms_alarm", 112),
    (PeriphRole::Battery, "bms_alarm", 113),
    (PeriphRole::Battery, "bms_alarm", 114),
    (PeriphRole::Battery, "bms_alarm", 115),
    (PeriphRole::Battery, "bms_alarm", 116),
    (PeriphRole::Battery, "bms_alarm", 117),
    (PeriphRole::Battery, "bms_alarm", 118),
    (PeriphRole::Battery, "bms_alarm", 119),
    (PeriphRole::Battery, "bms_alarm", 120),
    (PeriphRole::Battery, "bms_alarm", 121),
    (PeriphRole::Battery, "bms_alarm", 122),
    (PeriphRole::Battery, "bms_alarm", 123),
    (PeriphRole::Battery, "bms_alarm", 124),
    (PeriphRole::Battery, "bms_alarm", 125),
    (PeriphRole::Battery, "bms_alarm", 126),
    (PeriphRole::Battery, "bms_alarm", 127),
    (PeriphRole::Battery, "bms_alarm", 128),
    (PeriphRole::Battery, "bms_alarm", 129),
    (PeriphRole::Battery, "bms_alarm", 130),
    (PeriphRole::Battery, "bms_alarm", 131),
    (PeriphRole::Battery, "bms_alarm", 132),
    (PeriphRole::Battery, "bms_alarm", 133),
    (PeriphRole::Battery, "bms_alarm", 134),
    (PeriphRole::Battery, "bms_alarm", 135),
    (PeriphRole::Battery, "bms_alarm", 136),
    (PeriphRole::Battery, "bms_alarm", 137),
    (PeriphRole::Battery, "bms_alarm", 138),
    (PeriphRole::Battery, "bms_alarm", 139),
    (PeriphRole::Battery, "bms_alarm", 140),
    (PeriphRole::Battery, "bms_alarm", 141),
    (PeriphRole::Battery, "bms_alarm", 142),
    (PeriphRole::Battery, "bms_alarm", 143),
    (PeriphRole::Battery, "bms_alarm", 144),
    (PeriphRole::Battery, "bms_alarm", 145),
    (PeriphRole::Battery, "bms_alarm", 146),
    (PeriphRole::Battery, "bms_alarm", 147),
    (PeriphRole::Battery, "bms_alarm", 148),
    (PeriphRole::Battery, "bms_alarm", 149),
    (PeriphRole::Battery, "bms_alarm", 150),
    (PeriphRole::Battery, "bms_alarm", 151),
    (PeriphRole::Battery, "bms_alarm", 152),
    (PeriphRole::Battery, "bms_alarm", 153),
    (PeriphRole::Battery, "bms_alarm", 154),
    (PeriphRole::Battery, "bms_alarm", 155),
    (PeriphRole::Battery, "bms_alarm", 156),
    (PeriphRole::Battery, "bms_alarm", 157),
    (PeriphRole::Battery, "bms_alarm", 158),
    (PeriphRole::Battery, "bms_alarm", 159),
    (PeriphRole::Battery, "bms_alarm", 160),
    (PeriphRole::Battery, "bms_alarm", 161),
    (PeriphRole::Battery, "bms_alarm", 162),
    (PeriphRole::Battery, "bms_alarm", 163),
    (PeriphRole::Battery, "bms_alarm", 164),
    (PeriphRole::Battery, "bms_alarm", 165),
    (PeriphRole::Battery, "bms_alarm", 166),
    (PeriphRole::Battery, "bms_alarm", 167),
    (PeriphRole::Battery, "bms_alarm", 168),
    (PeriphRole::Battery, "bms_alarm", 169),
    (PeriphRole::Battery, "bms_alarm", 170),
    (PeriphRole::Battery, "bms_alarm", 171),
    (PeriphRole::Battery, "bms_alarm", 172),
    (PeriphRole::Battery, "bms_alarm", 173),
    (PeriphRole::Battery, "bms_alarm", 174),
    (PeriphRole::Battery, "bms_alarm", 175),
    (PeriphRole::Battery, "bms_alarm", 176),
    (PeriphRole::Battery, "bms_alarm", 177),
    (PeriphRole::Battery, "bms_alarm", 178),
    (PeriphRole::Battery, "bms_alarm", 179),
    (PeriphRole::Battery, "bms_alarm", 180),
    (PeriphRole::Battery, "bms_alarm", 181),
    (PeriphRole::Battery, "bms_alarm", 182),
    (PeriphRole::Battery, "bms_alarm", 183),
    (PeriphRole::Battery, "bms_alarm", 184),
    (PeriphRole::Battery, "bms_alarm", 185),
    (PeriphRole::Battery, "bms_alarm", 186),
    (PeriphRole::Battery, "bms_alarm", 187),
    (PeriphRole::Battery, "bms_alarm", 188),
    (PeriphRole::Battery, "bms_alarm", 189),
    (PeriphRole::Battery, "bms_alarm", 190),
    (PeriphRole::Battery, "bms_alarm", 191),
    (PeriphRole::Battery, "bms_alarm", 192),
    (PeriphRole::Battery, "bms_alarm", 193),
    (PeriphRole::Battery, "bms_alarm", 194),
    (PeriphRole::Battery, "bms_alarm", 195),
    (PeriphRole::Battery, "bms_alarm", 196),
    (PeriphRole::Battery, "bms_alarm", 197),
    (PeriphRole::Battery, "bms_alarm", 198),
    (PeriphRole::Battery, "bms_alarm", 199),
    (PeriphRole::Battery, "bms_alarm", 200),
    (PeriphRole::Battery, "bms_alarm", 201),
    (PeriphRole::Battery, "bms_alarm", 202),
    (PeriphRole::Battery, "bms_alarm", 203),
    (PeriphRole::Battery, "bms_alarm", 204),
    (PeriphRole::Battery, "bms_alarm", 205),
    (PeriphRole::Battery, "bms_alarm", 206),
    (PeriphRole::Battery, "bms_alarm", 207),
    (PeriphRole::Battery, "bms_alarm", 208),
    (PeriphRole::Battery, "bms_alarm", 209),
    (PeriphRole::Battery, "bms_alarm", 210),
    (PeriphRole::Battery, "bms_alarm", 211),
    (PeriphRole::Battery, "bms_alarm", 212),
    (PeriphRole::Battery, "bms_alarm", 213),
    (PeriphRole::Battery, "bms_alarm", 214),
    (PeriphRole::Battery, "bms_alarm", 215),
    (PeriphRole::Battery, "bms_alarm", 216),
    (PeriphRole::Battery, "bms_alarm", 217),
    (PeriphRole::Battery, "bms_alarm", 218),
    (PeriphRole::Battery, "bms_alarm", 219),
    (PeriphRole::Battery, "bms_alarm", 220),
    (PeriphRole::Battery, "bms_alarm", 221),
    (PeriphRole::Battery, "bms_alarm", 222),
    (PeriphRole::Battery, "bms_alarm", 223),
    (PeriphRole::Battery, "bms_alarm", 224),
    (PeriphRole::Battery, "bms_alarm", 225),
    (PeriphRole::Battery, "bms_alarm", 226),
    (PeriphRole::Battery, "bms_alarm", 227),
    (PeriphRole::Battery, "bms_alarm", 228),
    (PeriphRole::Battery, "bms_alarm", 229),
    (PeriphRole::Battery, "bms_alarm", 230),
    (PeriphRole::Battery, "bms_alarm", 231),
    (PeriphRole::Battery, "bms_alarm", 232),
    (PeriphRole::Battery, "bms_alarm", 233),
    (PeriphRole::Battery, "bms_alarm", 234),
    (PeriphRole::Battery, "bms_alarm", 235),
    (PeriphRole::Battery, "bms_alarm", 236),
    (PeriphRole::Battery, "bms_alarm", 237),
    (PeriphRole::Battery, "bms_alarm", 238),
    (PeriphRole::Battery, "bms_alarm", 239),
    (PeriphRole::Battery, "bms_alarm", 240),
    (PeriphRole::Battery, "bms_alarm", 241),
    (PeriphRole::Battery, "bms_alarm", 242),
    (PeriphRole::Battery, "bms_alarm", 243),
    (PeriphRole::Battery, "bms_alarm", 244),
    (PeriphRole::Battery, "bms_alarm", 245),
    (PeriphRole::Battery, "bms_alarm", 246),
    (PeriphRole::Battery, "bms_alarm", 247),
    (PeriphRole::Battery, "bms_alarm", 248),
    (PeriphRole::Battery, "bms_alarm", 249),
    (PeriphRole::Battery, "bms_alarm", 250),
    (PeriphRole::Battery, "bms_alarm", 251),
    (PeriphRole::Battery, "bms_alarm", 252),
    (PeriphRole::Battery, "bms_alarm", 253),
    (PeriphRole::Battery, "bms_alarm", 254),
    (PeriphRole::Battery, "bms_alarm", 255),
    (PeriphRole::Battery, "bms_alarm", 256),
    (PeriphRole::Battery, "bms_alarm", 257),
    (PeriphRole::Battery, "bms_alarm", 258),
    (PeriphRole::Battery, "bms_alarm", 259),
    (PeriphRole::Battery, "bms_alarm", 260),
    (PeriphRole::Battery, "bms_alarm", 261),
    (PeriphRole::Battery, "bms_alarm", 262),
    (PeriphRole::Battery, "bms_alarm", 263),
    (PeriphRole::Battery, "bms_alarm", 264),
    (PeriphRole::Battery, "bms_alarm", 265),
    (PeriphRole::Battery, "bms_alarm", 266),
    (PeriphRole::Battery, "bms_alarm", 267),
    (PeriphRole::Battery, "bms_alarm", 268),
    (PeriphRole::Battery, "bms_alarm", 269),
    (PeriphRole::Battery, "bms_alarm", 270),
    (PeriphRole::Battery, "bms_alarm", 271),
    (PeriphRole::Battery, "bms_alarm", 272),
    (PeriphRole::Battery, "bms_alarm", 273),
    (PeriphRole::Battery, "bms_alarm", 274),
    (PeriphRole::Battery, "bms_alarm", 275),
    (PeriphRole::Battery, "bms_alarm", 276),
    (PeriphRole::Battery, "bms_alarm", 277),
    (PeriphRole::Battery, "bms_alarm", 278),
    (PeriphRole::Battery, "bms_alarm", 279),
    (PeriphRole::Battery, "bms_alarm", 280),
    (PeriphRole::Battery, "bms_alarm", 281),
    (PeriphRole::Battery, "bms_alarm", 282),
    (PeriphRole::Battery, "bms_alarm", 283),
    (PeriphRole::Battery, "bms_alarm", 284),
    (PeriphRole::Battery, "bms_alarm", 285),
    (PeriphRole::Battery, "bms_alarm", 286),
    (PeriphRole::Battery, "bms_alarm", 287),
    (PeriphRole::Battery, "bms_alarm", 288),
    // ── MeterBatt / mb_ui（6 点）──
    (PeriphRole::MeterBatt, "mb_ui", 1),
    (PeriphRole::MeterBatt, "mb_ui", 2),
    (PeriphRole::MeterBatt, "mb_ui", 3),
    (PeriphRole::MeterBatt, "mb_ui", 4),
    (PeriphRole::MeterBatt, "mb_ui", 5),
    (PeriphRole::MeterBatt, "mb_ui", 6),
    // ── MeterBatt / mb_freq_line（4 点）──
    (PeriphRole::MeterBatt, "mb_freq_line", 1),
    (PeriphRole::MeterBatt, "mb_freq_line", 2),
    (PeriphRole::MeterBatt, "mb_freq_line", 3),
    (PeriphRole::MeterBatt, "mb_freq_line", 4),
    // ── MeterBatt / mb_phase（6 点）──
    (PeriphRole::MeterBatt, "mb_phase", 1),
    (PeriphRole::MeterBatt, "mb_phase", 3),
    (PeriphRole::MeterBatt, "mb_phase", 5),
    (PeriphRole::MeterBatt, "mb_phase", 12),
    (PeriphRole::MeterBatt, "mb_phase", 13),
    (PeriphRole::MeterBatt, "mb_phase", 14),
    // ── MeterBatt / mb_power（16 点）──
    (PeriphRole::MeterBatt, "mb_power", 1),
    (PeriphRole::MeterBatt, "mb_power", 3),
    (PeriphRole::MeterBatt, "mb_power", 5),
    (PeriphRole::MeterBatt, "mb_power", 7),
    (PeriphRole::MeterBatt, "mb_power", 9),
    (PeriphRole::MeterBatt, "mb_power", 11),
    (PeriphRole::MeterBatt, "mb_power", 13),
    (PeriphRole::MeterBatt, "mb_power", 15),
    (PeriphRole::MeterBatt, "mb_power", 17),
    (PeriphRole::MeterBatt, "mb_power", 19),
    (PeriphRole::MeterBatt, "mb_power", 21),
    (PeriphRole::MeterBatt, "mb_power", 23),
    (PeriphRole::MeterBatt, "mb_power", 25),
    (PeriphRole::MeterBatt, "mb_power", 26),
    (PeriphRole::MeterBatt, "mb_power", 27),
    (PeriphRole::MeterBatt, "mb_power", 28),
    // ── MeterBatt / mb_e_act_comb（1 点）──
    (PeriphRole::MeterBatt, "mb_e_act_comb", 1),
    // ── MeterBatt / mb_e_act_fwd（1 点）──
    (PeriphRole::MeterBatt, "mb_e_act_fwd", 1),
    // ── MeterBatt / mb_e_act_rev（1 点）──
    (PeriphRole::MeterBatt, "mb_e_act_rev", 1),
    // ── MeterBatt / mb_e_rea_comb（1 点）──
    (PeriphRole::MeterBatt, "mb_e_rea_comb", 1),
    // ── MeterBatt / mb_e_rea_fwd（1 点）──
    (PeriphRole::MeterBatt, "mb_e_rea_fwd", 1),
    // ── MeterBatt / mb_e_rea_rev（1 点）──
    (PeriphRole::MeterBatt, "mb_e_rea_rev", 1),
    // ── Pcs / pcs_3zone（27 点）──
    (PeriphRole::Pcs, "pcs_3zone", 9),
    (PeriphRole::Pcs, "pcs_3zone", 10),
    (PeriphRole::Pcs, "pcs_3zone", 12),
    (PeriphRole::Pcs, "pcs_3zone", 13),
    (PeriphRole::Pcs, "pcs_3zone", 19),
    (PeriphRole::Pcs, "pcs_3zone", 20),
    (PeriphRole::Pcs, "pcs_3zone", 21),
    (PeriphRole::Pcs, "pcs_3zone", 22),
    (PeriphRole::Pcs, "pcs_3zone", 26),
    (PeriphRole::Pcs, "pcs_3zone", 27),
    (PeriphRole::Pcs, "pcs_3zone", 28),
    (PeriphRole::Pcs, "pcs_3zone", 29),
    (PeriphRole::Pcs, "pcs_3zone", 34),
    (PeriphRole::Pcs, "pcs_3zone", 35),
    (PeriphRole::Pcs, "pcs_3zone", 36),
    (PeriphRole::Pcs, "pcs_3zone", 37),
    (PeriphRole::Pcs, "pcs_3zone", 38),
    (PeriphRole::Pcs, "pcs_3zone", 39),
    (PeriphRole::Pcs, "pcs_3zone", 40),
    (PeriphRole::Pcs, "pcs_3zone", 41),
    (PeriphRole::Pcs, "pcs_3zone", 42),
    (PeriphRole::Pcs, "pcs_3zone", 43),
    (PeriphRole::Pcs, "pcs_3zone", 45),
    (PeriphRole::Pcs, "pcs_3zone", 67),
    (PeriphRole::Pcs, "pcs_3zone", 72),
    (PeriphRole::Pcs, "pcs_3zone", 73),
    (PeriphRole::Pcs, "pcs_3zone", 75),
];

// ─────────────────────────────────────────────────────────────────────────────
// 屏用短标签表白名单（设计 §15.4 / §15.5.2 的「显示项（短标签）」列；F-4 / W-2 / W-3 / H-3）
// ─────────────────────────────────────────────────────────────────────────────

/// **屏用短标签表白名单**：与 [`PERIPH_WHITELIST`] **同序同长（447 行）**，
/// 逐项 = `(屏用短标签, 单位)`；`None` = **无量纲**。
///
/// 口径：
/// - **逐行对齐** [`PERIPH_WHITELIST`]（同分块注释、同 `at` 升序）⇒ `PERIPH_LABELS[i]`
///   即 `PERIPH_WHITELIST[i]` 的短标签与单位（H-3 的"行数相等"由此表可机械断言）；
/// - 文案真源 = 设计 §15.4（消防：`group` 表 + 明细列规则）+ §15.5.2（P6 各段「显示项（短标签）」
///   列**逐条**）；`bms_alarm` 288 位与 `fire_sys_1..7` 等设计未逐点列名处，取
///   `point_table` 登记 `label` 的**精炼短标签**（去全角括号内容、去单位后缀）；
/// - **不得含全角括号**（`（` `）`）——那是登记说明文本的标记，入表即说明误用了登记 `label`
///   （T-7 断言项）；
/// - 单位真源（W-2）：**只此一处**，catalog 构建器不得另写单位 / 数值字面表。
pub const PERIPH_LABELS: &[(&str, Option<&str>)] = &[
    // ── Hvac / hvac_in（3 点）──
    ("柜内温度", Some("℃")),
    ("内盘管温度", Some("℃")),
    ("柜内湿度", Some("%RH")),
    // ── Hvac / hvac_di（30 点）──
    ("内风机", None),
    ("应急风机", None),
    ("制冷", None),
    ("加热", None),
    ("制冷除湿", None),
    ("加热除湿", None),
    ("自检", None),
    ("系统运行", None),
    ("报警继电器输出", None),
    ("柜内温感故障", None),
    ("柜内高温", None),
    ("柜内低温", None),
    ("柜外温感故障", None),
    ("柜外高温", None),
    ("柜外低温", None),
    ("柜内湿感故障", None),
    ("柜内高湿", None),
    ("柜内低湿", None),
    ("柜外湿感故障", None),
    ("柜外高湿", None),
    ("柜外低湿", None),
    ("压缩机高压", None),
    ("压缩机低压", None),
    ("制冷失效", None),
    ("制热失效", None),
    ("内盘管温感故障", None),
    ("内盘管低温", None),
    ("三相电报警", None),
    ("接管回风温差", None),
    ("循环模式", None),
    // ── Fire / fire_sys（13 点）──
    ("系统状态", None),
    ("灭火瓶压力", Some("kPa")),
    ("烟感状态", None),
    ("温感状态", None),
    ("可燃状态", None),
    ("火警等级", None),
    ("登记数", Some("只")),
    ("地址", None),
    ("状态", None),
    ("数据 1", None),
    ("CO", Some("ppm")),
    ("VOC", Some("ppm")),
    ("H₂", Some("ppm")),
    // ── Fire / fire_det（6 点）──
    ("地址", None),
    ("状态", None),
    ("数据 1", None),
    ("CO", Some("ppm")),
    ("VOC", Some("ppm")),
    ("H₂", Some("ppm")),
    // ── Battery / bms_io（17 点）──
    ("允许充电最大功率", Some("kW")),
    ("允许放电最大功率", Some("kW")),
    ("簇组电压", Some("V")),
    ("簇组电流", Some("A")),
    ("簇组模块温度", Some("℃")),
    ("SOH", Some("%")),
    ("绝缘电阻", Some("kΩ")),
    ("平均单体电压", Some("V")),
    ("平均单体温度", Some("℃")),
    ("最高单体电压", Some("V")),
    ("最高单体电压对应点", None),
    ("最低单体电压", Some("V")),
    ("最低单体电压对应点", None),
    ("最高单体温度", Some("℃")),
    ("最高单体温度对应点", None),
    ("最低单体温度", Some("℃")),
    ("最低单体温度对应点", None),
    // ── Battery / bms_energy（9 点）──
    ("累计充电电量", Some("kWh")),
    ("累计放电电量", Some("kWh")),
    ("单次累计充电", Some("kWh")),
    ("单次累计放电", Some("kWh")),
    ("可充电量", Some("kWh")),
    ("可放电量", Some("kWh")),
    ("最高单体温升", Some("℃")),
    ("最高单体极柱温度", Some("℃")),
    ("最低单体极柱温度", Some("℃")),
    // ── Battery / bms_meta（8 点）──
    ("主控程序版本号", None),
    ("从控数量", Some("个")),
    ("SOE", Some("%")),
    ("单体温度极差", Some("℃")),
    ("单体电压极差", Some("V")),
    ("簇实时充放电功率", Some("kW")),
    ("PACK 组压最高", Some("V")),
    ("PACK 组压最低", Some("V")),
    // ── Battery / bms_term（4 点）──
    ("端子温度 001", Some("℃")),
    ("端子温度 002", Some("℃")),
    ("端子温度 003", Some("℃")),
    ("端子温度 004", Some("℃")),
    // ── Battery / bms_cap（4 点）──
    ("累计充电容量", Some("Ah")),
    ("累计放电容量", Some("Ah")),
    ("单次累计充电容量", Some("Ah")),
    ("单次累计放电容量", Some("Ah")),
    // ── Battery / bms_alarm（288 点）──
    ("簇主控通讯失联", None),
    ("簇端电压欠压·轻", None),
    ("簇端电压欠压·中", None),
    ("簇端电压欠压·重", None),
    ("簇端电压过压·轻", None),
    ("簇端电压过压·中", None),
    ("簇端电压过压·重", None),
    ("簇端充电电流·轻", None),
    ("簇端充电电流·中", None),
    ("簇端充电电流·重", None),
    ("簇端放电电流·轻", None),
    ("簇端放电电流·中", None),
    ("簇端放电电流·重", None),
    ("簇单体欠压·轻", None),
    ("簇单体欠压·中", None),
    ("簇单体欠压·重", None),
    ("簇单体过压·轻", None),
    ("簇单体过压·中", None),
    ("簇单体过压·重", None),
    ("簇单体欠温·轻", None),
    ("簇单体欠温·中", None),
    ("簇单体欠温·重", None),
    ("簇单体过温·轻", None),
    ("簇单体过温·中", None),
    ("簇单体过温·重", None),
    ("簇 SOC 低·轻", None),
    ("簇 SOC 低·中", None),
    ("簇 SOC 低·重", None),
    ("簇 SOH 低·轻", None),
    ("簇 SOH 低·中", None),
    ("簇 SOH 低·重", None),
    ("簇单体压差·轻", None),
    ("簇单体压差·中", None),
    ("簇单体压差·重", None),
    ("簇单体温差·轻", None),
    ("簇单体温差·中", None),
    ("簇单体温差·重", None),
    ("簇从控 1 通讯失联", None),
    ("簇从控 2 通讯失联", None),
    ("簇从控 3 通讯失联", None),
    ("簇从控 4 通讯失联", None),
    ("簇从控 5 通讯失联", None),
    ("簇从控 6 通讯失联", None),
    ("簇从控 7 通讯失联", None),
    ("簇从控 8 通讯失联", None),
    ("簇从控 9 通讯失联", None),
    ("簇从控 10 通讯失联", None),
    ("簇从控 11 通讯失联", None),
    ("簇从控 12 通讯失联", None),
    ("簇从控 13 通讯失联", None),
    ("簇从控 14 通讯失联", None),
    ("簇从控 15 通讯失联", None),
    ("簇从控 16 通讯失联", None),
    ("簇从控 17 通讯失联", None),
    ("簇从控 18 通讯失联", None),
    ("簇从控 19 通讯失联", None),
    ("簇从控 20 通讯失联", None),
    ("簇从控 21 通讯失联", None),
    ("簇从控 22 通讯失联", None),
    ("簇从控 23 通讯失联", None),
    ("簇从控 24 通讯失联", None),
    ("簇从控 25 通讯失联", None),
    ("簇从控 26 通讯失联", None),
    ("簇从控 27 通讯失联", None),
    ("簇从控 28 通讯失联", None),
    ("簇从控 29 通讯失联", None),
    ("簇从控 30 通讯失联", None),
    ("簇从控 31 通讯失联", None),
    ("簇从控 32 通讯失联", None),
    ("簇从控 33 通讯失联", None),
    ("簇从控 34 通讯失联", None),
    ("簇从控 35 通讯失联", None),
    ("簇从控 36 通讯失联", None),
    ("簇从控 37 通讯失联", None),
    ("簇从控 38 通讯失联", None),
    ("簇从控 39 通讯失联", None),
    ("簇从控 40 通讯失联", None),
    ("簇端子温度过高·轻", None),
    ("簇端子温度过高·中", None),
    ("簇端子温度过高·重", None),
    ("簇 pack 电压过高·轻", None),
    ("簇 pack 电压过高·中", None),
    ("簇 pack 电压过高·重", None),
    ("簇 pack 电压过低·轻", None),
    ("簇 pack 电压过低·中", None),
    ("簇 pack 电压过低·重", None),
    ("簇单体电压采集故障", None),
    ("簇单体温度采集故障", None),
    ("簇初始状态", None),
    ("簇充电", None),
    ("簇放电", None),
    ("簇就绪", None),
    ("簇维护", None),
    ("簇禁充", None),
    ("簇禁放", None),
    ("簇充放禁止", None),
    ("簇故障", None),
    ("簇测试模式", None),
    ("簇高压箱状态", None),
    ("簇从控 DI 告警状态", None),
    ("簇从控 1 DI 定制告警", None),
    ("簇从控 2 DI 定制告警", None),
    ("簇从控 3 DI 定制告警", None),
    ("簇从控 4 DI 定制告警", None),
    ("簇从控 5 DI 定制告警", None),
    ("簇从控 6 DI 定制告警", None),
    ("簇从控 7 DI 定制告警", None),
    ("簇从控 8 DI 定制告警", None),
    ("簇从控 9 DI 定制告警", None),
    ("簇从控 10 DI 定制告警", None),
    ("簇从控 11 DI 定制告警", None),
    ("簇从控 12 DI 定制告警", None),
    ("簇从控 13 DI 定制告警", None),
    ("簇从控 14 DI 定制告警", None),
    ("簇从控 15 DI 定制告警", None),
    ("簇从控 16 DI 定制告警", None),
    ("簇从控 17 DI 定制告警", None),
    ("簇从控 18 DI 定制告警", None),
    ("簇从控 19 DI 定制告警", None),
    ("簇从控 20 DI 定制告警", None),
    ("簇从控 21 DI 定制告警", None),
    ("簇从控 22 DI 定制告警", None),
    ("簇从控 23 DI 定制告警", None),
    ("簇从控 24 DI 定制告警", None),
    ("簇从控 25 DI 定制告警", None),
    ("簇从控 26 DI 定制告警", None),
    ("簇从控 27 DI 定制告警", None),
    ("簇从控 28 DI 定制告警", None),
    ("簇从控 29 DI 定制告警", None),
    ("簇从控 30 DI 定制告警", None),
    ("簇从控 31 DI 定制告警", None),
    ("簇从控 32 DI 定制告警", None),
    ("簇从控 33 DI 定制告警", None),
    ("簇从控 34 DI 定制告警", None),
    ("簇从控 35 DI 定制告警", None),
    ("簇从控 36 DI 定制告警", None),
    ("簇从控 37 DI 定制告警", None),
    ("簇从控 38 DI 定制告警", None),
    ("簇从控 39 DI 定制告警", None),
    ("簇从控 40 DI 定制告警", None),
    ("簇单体充电过温·轻", None),
    ("簇单体充电过温·中", None),
    ("簇单体充电过温·重", None),
    ("簇单体充电欠温·轻", None),
    ("簇单体充电欠温·中", None),
    ("簇单体充电欠温·重", None),
    ("簇单体放电过温·轻", None),
    ("簇单体放电过温·中", None),
    ("簇单体放电过温·重", None),
    ("簇单体放电欠温·轻", None),
    ("簇单体放电欠温·中", None),
    ("簇单体放电欠温·重", None),
    ("簇单体温升过大·轻", None),
    ("簇单体温升过大·中", None),
    ("簇单体温升过大·重", None),
    ("簇单体极柱温度过温·轻", None),
    ("簇单体极柱温度过温·中", None),
    ("簇单体极柱温度过温·重", None),
    ("簇单体极柱温度欠温·轻", None),
    ("簇单体极柱温度欠温·中", None),
    ("簇单体极柱温度欠温·重", None),
    ("簇单体电压变化过大·轻", None),
    ("簇单体电压变化过大·中", None),
    ("簇单体电压变化过大·重", None),
    ("定制保留 001", None),
    ("定制保留 002", None),
    ("定制保留 003", None),
    ("定制保留 004", None),
    ("定制保留 005", None),
    ("定制保留 006", None),
    ("定制保留 007", None),
    ("定制保留 008", None),
    ("定制保留 009", None),
    ("定制保留 010", None),
    ("定制保留 011", None),
    ("定制保留 012", None),
    ("定制保留 013", None),
    ("定制保留 014", None),
    ("定制保留 015", None),
    ("定制保留 016", None),
    ("定制保留 017", None),
    ("定制保留 018", None),
    ("定制保留 019", None),
    ("定制保留 020", None),
    ("定制保留 021", None),
    ("定制保留 022", None),
    ("定制保留 023", None),
    ("定制保留 024", None),
    ("定制保留 025", None),
    ("定制保留 026", None),
    ("定制保留 027", None),
    ("定制保留 028", None),
    ("定制保留 029", None),
    ("定制保留 030", None),
    ("定制保留 031", None),
    ("定制保留 032", None),
    ("定制保留 033", None),
    ("定制保留 034", None),
    ("定制保留 035", None),
    ("定制保留 036", None),
    ("定制保留 037", None),
    ("定制保留 038", None),
    ("定制保留 039", None),
    ("定制保留 040", None),
    ("定制保留 041", None),
    ("定制保留 042", None),
    ("定制保留 043", None),
    ("定制保留 044", None),
    ("定制保留 045", None),
    ("定制保留 046", None),
    ("定制保留 047", None),
    ("定制保留 048", None),
    ("定制保留 049", None),
    ("定制保留 050", None),
    ("定制保留 051", None),
    ("定制保留 052", None),
    ("定制保留 053", None),
    ("定制保留 054", None),
    ("定制保留 055", None),
    ("定制保留 056", None),
    ("定制保留 057", None),
    ("定制保留 058", None),
    ("定制保留 059", None),
    ("定制保留 060", None),
    ("簇一级告警", None),
    ("簇二级告警", None),
    ("簇三级告警", None),
    ("簇端子温度过低·轻", None),
    ("簇端子温度过低·中", None),
    ("簇端子温度过低·重", None),
    ("MOS 过温·轻", None),
    ("MOS 过温·中", None),
    ("MOS 过温·重", None),
    ("MOS 欠温·轻", None),
    ("MOS 欠温·中", None),
    ("MOS 欠温·重", None),
    ("簇 SOE 低·轻", None),
    ("簇 SOE 低·中", None),
    ("簇 SOE 低·重", None),
    ("簇单体正极柱温度过温·轻", None),
    ("簇单体正极柱温度过温·中", None),
    ("簇单体正极柱温度过温·重", None),
    ("簇单体正极柱温度欠温·轻", None),
    ("簇单体正极柱温度欠温·中", None),
    ("簇单体正极柱温度欠温·重", None),
    ("簇单体负极柱温度过温·轻", None),
    ("簇单体负极柱温度过温·中", None),
    ("簇单体负极柱温度过温·重", None),
    ("簇单体负极柱温度欠温·轻", None),
    ("簇单体负极柱温度欠温·中", None),
    ("簇单体负极柱温度欠温·重", None),
    ("簇绝缘检测低·轻", None),
    ("簇绝缘检测低·中", None),
    ("簇绝缘检测低·重", None),
    ("总正继电器粘连故障", None),
    ("总负继电器粘连故障", None),
    ("预充继电器粘连故障", None),
    ("风扇继电器粘连故障", None),
    ("休眠继电器粘连故障", None),
    ("断路器粘连故障", None),
    ("AFE 故障", None),
    ("单体温度短路故障", None),
    ("单体温度断路故障", None),
    ("MOS 温度故障", None),
    ("均衡 MOS 故障", None),
    ("从控通讯故障", None),
    ("从控供电故障", None),
    ("从控风扇故障", None),
    ("从控程序升级故障", None),
    ("从控参数设置故障", None),
    ("从控供电过压故障·轻", None),
    ("从控供电过压故障·中", None),
    ("从控供电过压故障·重", None),
    ("主控供电故障", None),
    ("主控程序升级故障", None),
    ("主控供电过压故障·轻", None),
    ("主控供电过压故障·中", None),
    ("主控供电过压故障·重", None),
    ("EEPROM 存储故障", None),
    ("地址编码故障", None),
    ("CAN 电流采集故障", None),
    ("485-1 通讯失联故障", None),
    ("485-2 通讯失联故障", None),
    ("PCS 失联故障", None),
    ("保留", None),
    ("保留", None),
    ("保留", None),
    ("保留", None),
    // ── MeterBatt / mb_ui（6 点）──
    ("储能表·A 相电压", Some("V")),
    ("储能表·B 相电压", Some("V")),
    ("储能表·C 相电压", Some("V")),
    ("储能表·A 相电流", Some("A")),
    ("储能表·B 相电流", Some("A")),
    ("储能表·C 相电流", Some("A")),
    // ── MeterBatt / mb_freq_line（4 点）──
    ("储能表·频率", Some("Hz")),
    ("储能表·线电压 A-B", Some("V")),
    ("储能表·线电压 C-B", Some("V")),
    ("储能表·线电压 A-C", Some("V")),
    // ── MeterBatt / mb_phase（6 点）──
    ("储能表·正向有功电能 A", Some("kWh")),
    ("储能表·正向有功电能 B", Some("kWh")),
    ("储能表·正向有功电能 C", Some("kWh")),
    ("储能表·零序电流", Some("A")),
    ("储能表·电压不平衡度", Some("%")),
    ("储能表·电流不平衡度", Some("%")),
    // ── MeterBatt / mb_power（16 点）──
    ("储能表·A 相有功", Some("kW")),
    ("储能表·B 相有功", Some("kW")),
    ("储能表·C 相有功", Some("kW")),
    ("储能表·总有功", Some("kW")),
    ("储能表·A 相无功", Some("kvar")),
    ("储能表·B 相无功", Some("kvar")),
    ("储能表·C 相无功", Some("kvar")),
    ("储能表·总无功", Some("kvar")),
    ("储能表·A 相视在", Some("kVA")),
    ("储能表·B 相视在", Some("kVA")),
    ("储能表·C 相视在", Some("kVA")),
    ("储能表·总视在", Some("kVA")),
    ("储能表·A 相功率因数", None),
    ("储能表·B 相功率因数", None),
    ("储能表·C 相功率因数", None),
    ("储能表·总功率因数", None),
    // ── MeterBatt / mb_e_act_comb（1 点）──
    ("储能表·组合有功总电能", Some("kWh")),
    // ── MeterBatt / mb_e_act_fwd（1 点）──
    ("储能表·正向有功总电能", Some("kWh")),
    // ── MeterBatt / mb_e_act_rev（1 点）──
    ("储能表·反向有功总电能", Some("kWh")),
    // ── MeterBatt / mb_e_rea_comb（1 点）──
    ("储能表·组合无功总电能", Some("kvarh")),
    // ── MeterBatt / mb_e_rea_fwd（1 点）──
    ("储能表·正向无功总电能", Some("kvarh")),
    // ── MeterBatt / mb_e_rea_rev（1 点）──
    ("储能表·反向无功总电能", Some("kvarh")),
    // ── Pcs / pcs_3zone（27 点）──
    ("BMS 系统总电压", Some("V")),
    ("BMS 系统总电流", Some("A")),
    ("直流母线电压", Some("V")),
    ("直流中点电压", Some("V")),
    ("电网 A 相电压", Some("V")),
    ("电网 B 相电压", Some("V")),
    ("电网 C 相电压", Some("V")),
    ("交流母线频率", Some("Hz")),
    ("视在功率 A", Some("kVA")),
    ("视在功率 B", Some("kVA")),
    ("视在功率 C", Some("kVA")),
    ("总视在功率", Some("kVA")),
    ("无功功率 A", Some("kvar")),
    ("无功功率 B", Some("kvar")),
    ("无功功率 C", Some("kvar")),
    ("总无功功率", Some("kvar")),
    ("功率因数 A", None),
    ("功率因数 B", None),
    ("功率因数 C", None),
    ("总功率因数", None),
    ("PCS 温度", Some("℃")),
    ("交流累计充电电量", Some("kWh")),
    ("交流累计放电电量", Some("kWh")),
    ("工作模式", None),
    ("DCDC 温度", Some("℃")),
    ("直流累计充电电量", Some("kWh")),
    ("直流累计放电电量", Some("kWh")),
];

// ─────────────────────────────────────────────────────────────────────────────
// 短标签 / 单位的**位置式查询口**（catalog 构建器的唯一入口）
// ─────────────────────────────────────────────────────────────────────────────

/// 查白名单点的**屏用短标签**（W-3 的存在性判据）。
///
/// - 命中 ⇒ `Some(短标签)`；不在白名单 / `at` 越界 ⇒ `None`（**不臆造**）；
/// - `fire_det` 的 `at` **自带归约**（`at = 6(k−2)+j` → 模板位 `((at−1) % 6) + 1`）⇒
///   调用方可直接传运行期展开后的 `at`（与 `PeripheralBlock::key` 的展开口径同源）；
/// - **与 [`unit_for`] 的分工（W-2 只要求单位真源来自本表，不要求额外 API）**：
///   本函数的 `Option` 表达"该键是否在表内"；[`unit_for`] 的 `None` **同时**表示
///   "无量纲"与"键不存在"——需要区分时用**本函数**判存在，`unit_for` 不作存在性判据。
///
/// `mupc-core-bin` 的 catalog 构建器只遍历白名单 ⇒ 本函数**应当**恒命中；
/// 若真的返回 `None`，那是 W-3 契约破损（构建器侧按 §15.11 #8 显式失败，**不得**回退成登记 label）。
pub fn label_for(role: PeriphRole, block: &str, at: u16) -> Option<&'static str> {
    let at = normalize_fire_det_at(block, at);
    index_of(role, block, at).and_then(|i| PERIPH_LABELS.get(i).map(|(label, _)| *label))
}

/// 查白名单点的**单位**（W-2 的**唯一真源**）：`None` = 无量纲**或**不在白名单
/// （两者的区分见 [`label_for`]）。
pub fn unit_for(role: PeriphRole, block: &str, at: u16) -> Option<&'static str> {
    let at = normalize_fire_det_at(block, at);
    index_of(role, block, at).and_then(|i| PERIPH_LABELS.get(i).and_then(|(_, unit)| *unit))
}

/// 白名单下标（两表**同序** ⇒ 同一个下标同时索引标签与单位）。
fn index_of(role: PeriphRole, block: &str, at: u16) -> Option<usize> {
    PERIPH_WHITELIST
        .iter()
        .position(|(r, b, a)| *r == role && *b == block && *a == at)
}

/// `fire_det` 运行期展开行 → **模板位**（本文件的唯一归约点；非 `fire_det` 原样返回）。
///
/// 与 `point_table::FIRE_DET_TEMPLATE_START(17)` / `FIRE_DET_STRIDE(6)` 的组内口径同源：
/// 第 `k` 只探测器的第 `j` 个寄存器 ↔ 模板位 `j`（`j ∈ 1..=6`）。
fn normalize_fire_det_at(block: &str, at: u16) -> u16 {
    if block == crate::peripherals::FIRE_DET_BLOCK_NAME && at > crate::peripherals::FIRE_DET_POINTS_PER_UNIT as u16 {
        (at - 1) % crate::peripherals::FIRE_DET_POINTS_PER_UNIT as u16 + 1
    } else {
        at
    }
}

/// 白名单判定（W-1 的唯一实现）。注意 `fire_det` 只有模板行：`at ∈ 1..=6` 命中；
/// `at ≥ 7` 属运行期展开的行（调用方须先按 `at = 6(k−2)+j` 归约到模板位再判）。
pub fn contains(role: PeriphRole, block: &str, at: u16) -> bool {
    PERIPH_WHITELIST
        .iter()
        .any(|(r, b, a)| *r == role && *b == block && *a == at)
}

/// 页内分组键（设计 §15.4 / §15.5.2 的「分组键」列**逐字**）。
///
/// HMI 侧用**静态 map** 把键映射到中文分组标题（标题是 UI 字面量，受既有码表扫描覆盖）。
/// 白名单外 / 未知块 ⇒ [`GROUP_UNKNOWN`]（**不臆造分组**）。
pub fn group_of(role: PeriphRole, block: &str, at: u16) -> &'static str {
    match (role, block) {
        (PeriphRole::Hvac, "hvac_in") => match at {
            1 => "hvac_measure",
            3..=4 => "hvac_measure",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::Hvac, "hvac_di") => match at {
            8 => "hvac_run",
            10..=25 => "hvac_alarm",
            27..=30 => "hvac_alarm",
            1..=7 => "hvac_state",
            9 => "hvac_state",
            31 => "hvac_state",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::Fire, "fire_sys") => match at {
            1 => "fire_sys_status",
            2 => "fire_cylinder",
            3..=5 => "fire_trigger",
            6 => "fire_level",
            7..=13 => "fire_detector",
            _ => GROUP_UNKNOWN,
        },
        // `fire_det` 是**唯一随 n 展开**的块：白名单只登记模板 6 行，运行期按
        // `at = 6(k−2)+j` 展开 ⇒ **整块同一分组键**（不按 at 分支，否则展开行会落 unknown）
        (PeriphRole::Fire, "fire_det") => "fire_detector",
        (PeriphRole::Battery, "bms_io") => match at {
            16..=18 => "bms_core",
            20..=21 => "bms_health",
            22..=31 => "bms_cell_extreme",
            2..=3 => "bms_power",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::Battery, "bms_energy") => match at {
            13 => "bms_delta",
            1 => "bms_energy",
            3 => "bms_energy",
            5 => "bms_energy",
            7 => "bms_energy",
            9 => "bms_energy",
            11 => "bms_energy",
            17 => "bms_pole_temp",
            19 => "bms_pole_temp",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::Battery, "bms_meta") => match at {
            3 => "bms_health",
            4..=5 => "bms_delta",
            6 => "bms_power",
            1..=2 => "bms_device",
            7 => "bms_device",
            9 => "bms_device",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::Battery, "bms_term") => match at {
            1..=4 => "bms_term",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::Battery, "bms_cap") => match at {
            1 => "bms_energy",
            3 => "bms_energy",
            5..=6 => "bms_energy",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::Battery, "bms_alarm") => match at {
            1..=288 => "bms_alarm",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_ui") => match at {
            1..=6 => "mb_u_i",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_freq_line") => match at {
            1 => "mb_freq",
            2..=4 => "mb_u_i",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_phase") => match at {
            1 => "mb_energy",
            3 => "mb_energy",
            5 => "mb_energy",
            12..=14 => "mb_quality",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_power") => match at {
            1 => "mb_power",
            3 => "mb_power",
            5 => "mb_power",
            7 => "mb_power",
            9 => "mb_power",
            11 => "mb_power",
            13 => "mb_power",
            15 => "mb_power",
            17 => "mb_power",
            19 => "mb_power",
            21 => "mb_power",
            23 => "mb_power",
            25..=28 => "mb_pf",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_e_act_comb") => match at {
            1 => "mb_energy",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_e_act_fwd") => match at {
            1 => "mb_energy",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_e_act_rev") => match at {
            1 => "mb_energy",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_e_rea_comb") => match at {
            1 => "mb_energy",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_e_rea_fwd") => match at {
            1 => "mb_energy",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::MeterBatt, "mb_e_rea_rev") => match at {
            1 => "mb_energy",
            _ => GROUP_UNKNOWN,
        },
        (PeriphRole::Pcs, "pcs_3zone") => match at {
            19..=22 => "pcs_ac_u_f",
            26..=29 => "pcs_ac_power",
            34..=41 => "pcs_ac_power",
            9..=10 => "pcs_dc",
            12..=13 => "pcs_dc",
            42 => "pcs_temp",
            72 => "pcs_temp",
            43 => "pcs_energy",
            45 => "pcs_energy",
            73 => "pcs_energy",
            75 => "pcs_energy",
            67 => "pcs_mode",
            _ => GROUP_UNKNOWN,
        },
        _ => GROUP_UNKNOWN,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 分组标题（设计 §15.4 / §15.5.2「分组标题」列 + §15.7.3 清单）
// ─────────────────────────────────────────────────────────────────────────────

/// **分组键 → 中文分组标题**（设计 §15.4 / §15.5.2 的「分组标题」列**逐字**；§15.7.3 按字面量
/// 列 33 项 ⇒ 去重 **31**）。
///
/// ⚠️ **括号与其中的数字都是字面量的一部分，不得剥掉**（§15.7.3 订正 v2.1-r3 ③）：`告警位（20）`
/// / `告警位（288）` / `辅助状态位（10）` / `电能（累计量）` 四处**必须**保留全角括号与数字；
/// 且 `告警位（20）` 与 `告警位（288）` 字面量不同 ⇒ **不合并去重**（T-23 的待查集合含
/// `（` `）` 与 `10 / 20 / 288`；剥掉就会漏掉真实字库缺口）。
///
/// 键集 = [`group_of`] 的全部返回值（31 个）+ §15.5.2 段「装置」的 `station_status` /
/// `device_info`（这两个是**页面级**分组、**不是** catalog 点 ⇒ [`group_of`] 不产出，
/// 但设计字段表给了分组键）。两处**按字面量重复**（去重的两个对子）：
/// `装置信息` ×2（`bms_device` / `device_info`）、`功率` ×2（`bms_power` / `mb_power`）。
pub const GROUP_TITLES: &[(&str, &str)] = &[
    // P4 安全 · 联锁页（§15.4）
    ("fire_level", "火警等级"),
    ("fire_sys_status", "消防系统状态"),
    ("fire_cylinder", "灭火瓶压力"),
    ("fire_trigger", "探测器触发"),
    ("fire_detector", "探测器"),
    // P6 段「装置」（§15.5.2；页面级分组，非 catalog 点）
    ("station_status", "外设站状态"),
    ("device_info", "装置信息"),
    // P6 段「空调」（§15.5.2，F20）
    ("hvac_measure", "测量值"),
    ("hvac_run", "运行状态"),
    ("hvac_alarm", "告警位（20）"),
    ("hvac_state", "辅助状态位（10）"),
    // P6 段「电池」（§15.5.2，F22）
    ("bms_core", "簇组核心量"),
    ("bms_health", "健康度"),
    ("bms_cell_extreme", "单体极值"),
    ("bms_delta", "极差"),
    ("bms_power", "功率"),
    ("bms_term", "端子温度"),
    ("bms_energy", "累计量"),
    ("bms_pole_temp", "极柱温度"),
    ("bms_device", "装置信息"),
    ("bms_alarm", "告警位（288）"),
    // P6 段「储能表」（§15.5.2，F23）
    ("mb_u_i", "电压电流"),
    ("mb_freq", "频率"),
    ("mb_power", "功率"),
    ("mb_pf", "功率因数"),
    ("mb_energy", "电能（累计量）"),
    ("mb_quality", "质量指标"),
    // P6 段「PCS」（§15.5.2，F24）
    ("pcs_ac_u_f", "交流电压频率"),
    ("pcs_ac_power", "交流功率"),
    ("pcs_dc", "直流侧"),
    ("pcs_temp", "温度"),
    ("pcs_energy", "累计电量"),
    ("pcs_mode", "工作模式"),
];

// ─────────────────────────────────────────────────────────────────────────────
// UI 固定文案常量（设计 §15.7.3「UI 固定文案」表**逐条**；H-2 / T-23 的待查集合之一）
// ─────────────────────────────────────────────────────────────────────────────

/// **UI 固定文案**（§15.7.3 表**逐条落常量**）。
///
/// 纪律：HMI 侧**只许 `use` 本模块的常量**，不得在页面 / `state.rs` 里另写同义中文串 ——
/// 否则同一句话在两处各写一遍，T-23 的覆盖网只看得到源码里的那一份（这正是本模块存在的理由）。
///
/// 分组与 §15.7.3 表的行一一对应；**不新增**该表没有的文案（尤其**不**为负向验收
/// EX-08 / EX-21 / EX-23 加"本页不含…"的可见声明，§15.7.3 的刻意决定）。
pub mod ui_text {
    // ── 站位 / 取值降级（§15.7.3 第 1 行）──
    /// 站离线（站级降级；优先级最高，EDGE-18）。
    pub const STATION_OFFLINE: &str = "站离线";
    /// 未取数（点未采到；`flag = NotRead`）。
    pub const NOT_READ: &str = "未取数";
    /// 数据异常（非有限 / 越界；`flag = RangeError`）。
    pub const RANGE_ERROR: &str = "数据异常";
    /// 未配置（消防钢瓶气压 `cylinder_configured == Some(false)`，EDGE-23）。
    pub const NOT_CONFIGURED: &str = "未配置";
    /// 名称未获取（catalog 未就绪 / `catalog_rev` 未对齐）。
    pub const NAME_UNKNOWN: &str = "名称未获取";
    /// 明细不可用（下钻端点不可用）。
    pub const DETAIL_UNAVAILABLE: &str = "明细不可用";
    /// 不可用（通用兜底；不臆造具体原因）。
    pub const UNAVAILABLE: &str = "不可用";

    // ── 段级降级（§15.7.3 第 2 行）──
    /// 外设数据不可用（外设段 `available = false`，EDGE-22）。
    pub const PERIPH_UNAVAILABLE: &str = "外设数据不可用";
    /// 消防源不可用（消防站不可用，EDGE-09 口径）。
    pub const FIRE_SOURCE_UNAVAILABLE: &str = "消防源不可用";
    /// 无活跃告警位（`available = true` 且活跃数 = 0，EDGE-24 三态之一）。
    pub const NO_ACTIVE_ALARM_BIT: &str = "无活跃告警位";
    /// BMS 告警源不可用（告警块不存在，**≠** 无活跃告警位，EDGE-24）。
    pub const BMS_ALARM_SOURCE_UNAVAILABLE: &str = "BMS 告警源不可用";
    /// 装置与外设（P6 页名；改名依据 §15.5）。
    pub const PAGE_DEVICE_AND_PERIPH: &str = "装置与外设";

    // ── 位语义（§15.7.3 第 3 行）──
    /// 未定义位（其后接位号，如「未定义位 15」；F21.1 / EX-09）。
    pub const BIT_UNDEFINED: &str = "未定义位";
    /// 预留（消防 `fire_sys_3/4/5` 的 bit2）。
    pub const BIT_RESERVED: &str = "预留";
    /// 活跃（通用位活跃文案）。
    pub const BIT_ACTIVE: &str = "活跃";
    /// 非活跃。
    pub const BIT_INACTIVE: &str = "非活跃";
    /// 在线。
    pub const ONLINE: &str = "在线";
    /// 离线。
    pub const OFFLINE: &str = "离线";
    /// 报警总状态（探测器状态整字 bit12）。
    pub const BIT_ALARM_TOTAL: &str = "报警总状态";
    /// 故障总状态（探测器状态整字 bit14）。
    pub const BIT_FAULT_TOTAL: &str = "故障总状态";
    /// 通信状态（探测器状态整字 bit15；R-41 追认前**不上屏**，常量保留以便追认后启用）。
    pub const BIT_COMM_STATE: &str = "通信状态";

    // ── 枚举文案（§15.7.3 第 4 行）──
    /// 正常（火警等级 0）。
    pub const ENUM_NORMAL: &str = "正常";
    /// 一级报警（火警等级 1）。
    pub const ENUM_FIRE_LEVEL1: &str = "一级报警";
    /// 二级火警（火警等级 2）。
    pub const ENUM_FIRE_LEVEL2: &str = "二级火警";
    /// 未定义（火警等级 3 = 预留 ⇒ 显「未定义」；F21.2）。
    pub const ENUM_FIRE_UNDEFINED: &str = "未定义";
    /// 紧急启动（火警等级 4）。
    pub const ENUM_FIRE_EMG_START: &str = "紧急启动";
    /// 紧急停止（火警等级 5）。
    pub const ENUM_FIRE_EMG_STOP: &str = "紧急停止";
    /// 未知（枚举**表外值**；**绝不**落「正常」，F21.2 / EX-10）。
    pub const ENUM_UNKNOWN: &str = "未知";
    /// 停止（HVAC 系统运行位 0）。
    pub const ENUM_STOPPED: &str = "停止";
    /// 运行（HVAC 系统运行位 1）。
    pub const ENUM_RUNNING: &str = "运行";

    // ── 时刻与提示（§15.7.3 第 5 行）──
    /// 最后成功（站状态条的「最后成功 12:03:44」前缀）。
    pub const LAST_OK: &str = "最后成功";
    /// 最近更新（块级时标前缀，F25.2）。
    pub const LAST_UPDATE: &str = "最近更新";
    /// 登记（探测器汇总行「登记 N 只」，F21.4）。
    pub const REGISTERED: &str = "登记";
    /// 可读（探测器汇总行「可读 M 只」，F21.4）。
    pub const READABLE: &str = "可读";
    /// 报警（汇总计数用）。
    pub const COUNT_ALARM: &str = "报警";
    /// 故障（汇总计数用）。
    pub const COUNT_FAULT: &str = "故障";
    /// 名称表可能过期（catalog 版本与帧内 `catalog_rev` 不一致）。
    pub const CATALOG_STALE: &str = "名称表可能过期";
    /// 明细超出帧预算已截断至（`truncated` 非空时的显式提示，F21.4 **不得静默**）。
    pub const TRUNCATED_TO: &str = "明细超出帧预算已截断至";
    /// 枚举文案待厂方追认（`pcs_3zone_67` 旁注，R-32）。
    pub const ENUM_PENDING_VENDOR: &str = "枚举文案待厂方追认";

    // ── 分页与下钻（§15.7.3 第 6 行）──
    /// 查看明细（探测器下钻入口）。
    pub const VIEW_DETAIL: &str = "查看明细";
    /// 查看全部（BMS 288 位下钻入口）。
    pub const VIEW_ALL: &str = "查看全部";
    /// 上一页。
    pub const PREV_PAGE: &str = "上一页";
    /// 下一页。
    pub const NEXT_PAGE: &str = "下一页";
    /// 收起（下钻返回；F11.3「子页有返回」的同位置出口）。
    pub const COLLAPSE: &str = "收起";
    /// 重试（通道 / 端点失败后的出路）。
    pub const RETRY: &str = "重试";
    /// 第（分页指示「第 N / M 页」的前缀）。
    pub const PAGE_PREFIX: &str = "第";
    /// 页（分页指示后缀）。
    pub const PAGE_SUFFIX: &str = "页";

    // ── 版本降级（§15.7.3 第 7 行）──
    /// 屏与主进程版本不匹配（`Incompatible`，EX-29）。
    pub const VERSION_MISMATCH: &str = "屏与主进程版本不匹配";
    /// 请刷同版本固件。
    pub const FLASH_SAME_VERSION: &str = "请刷同版本固件";
    /// 与主进程数据通道断开（重试中）。
    pub const CHANNEL_DOWN_RETRYING: &str = "与主进程数据通道断开（重试中）";
}

// ─────────────────────────────────────────────────────────────────────────────
// 消防枚举 / 位语义 / 拆解（设计 §15.4；**从 `mupc-core-bin` 迁入**，为让 T-23 看得见）
//
// 迁移口径：**文案一字未改**（T20 已锁定措辞）。此前住在 `console_host.rs` ⇒ 以运行时字符串
// 到 HMI ⇒ 码表覆盖率用例（H-2 / T-23）**看不见**（F-5 的盲区）。落点见 §15.11 #3。
// ─────────────────────────────────────────────────────────────────────────────

/// `fire_det` 每只探测器的 6 个模板位的短标签（`+0 地址`…`+5 H₂`，§15.4 明细表列名）。
/// **与 [`PERIPH_LABELS`] 的 `fire_det` 模板 6 行逐字相同**（单测钉住，防两处漂移）。
pub const FIRE_DET_TEMPLATE_LABELS: [&str; 6] = ["地址", "状态", "数据 1", "CO", "VOC", "H₂"];

/// `fire_sys_6`（火警等级）枚举文案：**唯一权威 = PRD §3.9 F21 展示表**
/// （`0 正常 / 1 一级报警 / 2 二级火警 / 3 预留 ⇒ 显「未定义」 / 4 紧急启动 / 5 紧急停止`；
/// **表外值 ⇒ 屏显「未知」**）。⚠️ **不得**引 `SIG_FIRE_LEVEL`（那只有 4 条事件值）。
pub const FIRE_LEVEL_ENUM: [(u16, &str); 6] = [
    (0, "正常"),
    (1, "一级报警"),
    (2, "二级火警"),
    (3, "未定义"),
    (4, "紧急启动"),
    (5, "紧急停止"),
];

/// `fire_sys_1`（系统状态位图）的 6 个已定义位：`(位号, 短标签)`（§15.4 A1 组）。
pub const FIRE_SYS_BITS: [(u8, &str); 6] = [
    (14, "主电故障"),
    (13, "备电故障"),
    (11, "驱动电路"),
    (10, "压力传感器"),
    (9, "电磁阀"),
    (8, "喷洒标记"),
];

/// `fire_sys_3/4/5`（烟感 / 温感 / 可燃状态）的 3 个位（§15.4 A3 组）。
pub const FIRE_TRIGGER_BITS: [(u8, &str); 3] = [(0, "干接点触发"), (1, "复合触发"), (2, "预留")];

/// 探测器状态整字（`fire_sys_9` / `fire_det_{+1}`）的 2 个已定义位（§15.4 明细表）。
/// **bit15「通信状态」不在此列**（点表登记明令"不猜、不造判据"，PRD F21 未要求 ⇒ 显
/// 「未定义位 15」；R-41 追认后才可能启用）。
pub const FIRE_DETECTOR_STATE_BITS: [(u8, &str); 2] = [(12, "报警总状态"), (14, "故障总状态")];

/// 「数据 1」的拆解规格（高字节烟雾 0.1 dB/M；低字节温度 raw−55 ℃；§15.4 明细表）。
///
/// **拆解只发生在展示层**：帧内 `v` 与 `latest_values` 保持**整字**（F21.5 / EX-13）。
pub fn data1_decompose() -> Vec<crate::peripherals::Decompose> {
    use crate::peripherals::{DecodeFrom, Decompose};
    vec![
        Decompose {
            label: "烟雾".into(),
            unit: Some("dB/M".into()),
            decimals: 1,
            from: DecodeFrom::HighByte {
                scale: 0.1,
                offset: 0.0,
            },
        },
        Decompose {
            label: "温度".into(),
            unit: Some("℃".into()),
            decimals: 0,
            from: DecodeFrom::LowByte {
                scale: 1.0,
                offset: -55.0,
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peripherals::PeriphRole;

    /// 白名单规模与设计 §15.2.4 的容量分子**逐项相等**（W-1 / W-4 的机械锚点）。
    ///
    /// - 合计 **447** 行 = 非 `fire_det` **441** + `fire_det` **模板 6** 行（运行期按 n 展开）；
    /// - 逐 role：`hvac` 33 / `battery` 330 / `meter_batt` 38 / `pcs` 27 / `fire` 19。
    #[test]
    fn whitelist_counts_match_design_capacity_numerator() {
        assert_eq!(PERIPH_WHITELIST.len(), 447, "§15.2.4：白名单 447 行（441 + 6 模板）");
        let per_role = |r: PeriphRole| PERIPH_WHITELIST.iter().filter(|(x, _, _)| *x == r).count();
        assert_eq!(per_role(PeriphRole::Hvac), 33, "§15.5.2 段「空调」");
        assert_eq!(per_role(PeriphRole::Battery), 330, "§15.5.2 段「电池」");
        assert_eq!(per_role(PeriphRole::MeterBatt), 38, "§15.5.2 段「储能表」");
        assert_eq!(per_role(PeriphRole::Pcs), 27, "§15.5.2 段「PCS」");
        assert_eq!(per_role(PeriphRole::Fire), 19, "§15.5.2：fire_sys 13 + fire_det 模板 6");
        let fire_det = PERIPH_WHITELIST
            .iter()
            .filter(|(_, b, _)| *b == "fire_det")
            .count();
        assert_eq!(fire_det, 6, "fire_det 在常量表内以模板 6 行登记（W-4）");
        assert_eq!(
            PERIPH_WHITELIST.len() - fire_det,
            crate::peripherals::PERIPH_NON_FIRE_DET_POINTS,
            "非 fire_det 白名单点数必须等于守卫式里的 441（单一真源，§15.2.4）"
        );
        // 同一 (role, block, at) 不得重复（重复会让"行数相等"断言失真）
        let mut seen: Vec<(PeriphRole, &str, u16)> = Vec::new();
        for row in PERIPH_WHITELIST {
            assert!(!seen.contains(row), "白名单不得有重复行：{row:?}");
            seen.push(*row);
        }
    }

    /// W-1 的**结构性排除项**逐条不在表内（设计 §15.5.2「白名单外 ⇒ 结构性不上屏」）。
    #[test]
    fn structurally_excluded_points_are_absent() {
        // 位 25 保留位（`BitClass::Reserved`）
        assert!(!contains(PeriphRole::Hvac, "hvac_di", 26), "hvac_di_26（位 25 保留）不得上屏");
        // 储能表 PT / CT 只读对照
        assert!(!contains(PeriphRole::MeterBatt, "mb_phase", 7), "PT 对照不上屏");
        assert!(!contains(PeriphRole::MeterBatt, "mb_phase", 8), "CT 对照不上屏");
        // PCS 1046–1065（STS / 负载区，含 1049 = `pcs_3zone_50`）—— 依据 = PRD §7 #11
        for at in 47..=66u16 {
            assert!(
                !contains(PeriphRole::Pcs, "pcs_3zone", at),
                "pcs_3zone_{at}（1046–1065）不得上屏"
            );
        }
        // BMS `bms_io` 的未列入项（簇状态枚举 / 允许充放电压电流 / DI 位 / SOC 之外的未列项）
        for at in [1u16, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 19] {
            assert!(!contains(PeriphRole::Battery, "bms_io", at), "bms_io_{at} 未列入白名单");
        }
        // `bms_meta` 的 8 = 「主控程序版本号」等在表内；`bms_energy` 的 15（最高单体最高电压变化）
        // 不在 §15.5.2 的「累计量」清单内 ⇒ 结构性不上屏
        assert!(!contains(PeriphRole::Battery, "bms_energy", 15));
        // `fire_det` 模板只到 +5（H₂）；at ≥ 7 属运行期展开行
        assert!(contains(PeriphRole::Fire, "fire_det", 6));
        assert!(!contains(PeriphRole::Fire, "fire_det", 7));
        // 台区总表（`meter_grid`）不在本增量内（§15 范围外 #3）
        assert!(
            !PERIPH_WHITELIST
                .iter()
                .any(|(_, b, _)| b.contains("grid")),
            "台区关口总表不得进白名单"
        );
    }

    /// 分组键**完备且唯一**：白名单每一点恰有一个分组键，且键都是设计 §15.4 / §15.5.2 的
    /// 字面量（HMI 的静态 map 要有对应标题；漏一点 ⇒ 该点在屏上无处安放）。
    #[test]
    fn every_whitelisted_point_has_a_design_group_key() {
        const DESIGN_KEYS: &[&str] = &[
            // P4（§15.4）
            "fire_level", "fire_sys_status", "fire_cylinder", "fire_trigger", "fire_detector",
            // P6 段「空调」/「电池」/「储能表」/「PCS」（§15.5.2）
            "hvac_measure", "hvac_run", "hvac_alarm", "hvac_state",
            "bms_core", "bms_health", "bms_cell_extreme", "bms_delta", "bms_power", "bms_term",
            "bms_energy", "bms_pole_temp", "bms_device", "bms_alarm",
            "mb_u_i", "mb_freq", "mb_power", "mb_pf", "mb_energy", "mb_quality",
            "pcs_ac_u_f", "pcs_ac_power", "pcs_dc", "pcs_temp", "pcs_energy", "pcs_mode",
        ];
        for (role, block, at) in PERIPH_WHITELIST {
            let g = group_of(*role, block, *at);
            assert_ne!(g, GROUP_UNKNOWN, "白名单点 {role:?}/{block}/{at} 必须有分组键");
            assert!(DESIGN_KEYS.contains(&g), "{block}_{at} 的分组键 `{g}` 不在设计分组表内");
        }
        // 反例：不在白名单的点、未知块 ⇒ 显式 `unknown`（不臆造分组）
        assert_eq!(group_of(PeriphRole::Hvac, "hvac_di", 26), GROUP_UNKNOWN);
        assert_eq!(group_of(PeriphRole::Unknown, "nope", 1), GROUP_UNKNOWN);
        // 同一块内**跨分组**的抽样（防止把整块粗暴归一组）
        assert_eq!(group_of(PeriphRole::Hvac, "hvac_di", 8), "hvac_run");
        assert_eq!(group_of(PeriphRole::Hvac, "hvac_di", 12), "hvac_alarm");
        assert_eq!(group_of(PeriphRole::Hvac, "hvac_di", 9), "hvac_state");
        assert_eq!(group_of(PeriphRole::Battery, "bms_meta", 3), "bms_health");
        assert_eq!(group_of(PeriphRole::Battery, "bms_meta", 4), "bms_delta");
        assert_eq!(group_of(PeriphRole::Battery, "bms_meta", 6), "bms_power");
        assert_eq!(group_of(PeriphRole::MeterBatt, "mb_freq_line", 1), "mb_freq");
        assert_eq!(group_of(PeriphRole::MeterBatt, "mb_freq_line", 2), "mb_u_i");
        assert_eq!(group_of(PeriphRole::MeterBatt, "mb_power", 25), "mb_pf");
        assert_eq!(group_of(PeriphRole::Pcs, "pcs_3zone", 67), "pcs_mode");
        assert_eq!(group_of(PeriphRole::Pcs, "pcs_3zone", 72), "pcs_temp");
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // T-7 / H-3 / W-2 / W-3：短标签表覆盖性、单位真源、分组标题完备性（§15.8 T-7）
    // ═══════════════════════════════════════════════════════════════════════════

    /// **H-3 的两条断言**：`PERIPH_WHITELIST.len() == PERIPH_LABELS.len()`（行数相等），
    /// 且两表**下标一致**（同一下标 ⇒ 同一个 `(role, block, at)` 的标签与单位）。
    ///
    /// "行数相等"要能机械写出来，前提是白名单**可枚举**（W-4 —— 两个表都是常量数组）。
    ///
    /// ⚠️ **本用例的判别力边界（T21a 评审 (A)-1 / P1a·P1b 实测，如实登记）**：下面对
    /// `label_for(白名单[i]) == PERIPH_LABELS[i]` 的那条断言是**由构造恒真**的 ——
    /// `label_for` 内部就是 `PERIPH_LABELS[index_of(白名单[i])]`，而 `index_of` 搜的就是
    /// **同一张**白名单 ⇒ 两表任意错位（相邻互换 / 整表错位一格）它**照绿**。
    /// 它能证明的只是"**行数相等 + 下标口径自洽**"，**不是**"两表单点语义对齐"。
    /// 后者（"短标签表的键必须命中一个登记行或站配置点"，设计 §15.3.2 的 W-2 后半）需要
    /// **独立的第二真源**，落在 `mupc-core-bin` 的
    /// `console_host::tests::short_label_keys_hit_registered_rows_and_registration_wording_is_superset`
    /// （投到 `mupc_southd::point_table` 的登记行）—— **那一条有牙**（错位必红）。
    #[test]
    fn short_label_table_is_row_aligned_with_whitelist() {
        assert_eq!(
            PERIPH_LABELS.len(),
            PERIPH_WHITELIST.len(),
            "H-3：短标签表与白名单必须行数相等（447）"
        );
        assert_eq!(PERIPH_LABELS.len(), 447, "§15.2.4：441 非 fire_det + fire_det 模板 6");
        assert_eq!(PERIPH_WHITELIST.len(), 447);
        for (i, (role, block, at)) in PERIPH_WHITELIST.iter().enumerate() {
            // W-3：白名单**每一项**都有短标签（防"上了帧却没有屏上文案"）
            let label = label_for(*role, block, *at)
                .unwrap_or_else(|| panic!("W-3 漏项：{role:?}/{block}/{at}（两表下标 {i}）"));
            assert!(!label.trim().is_empty(), "{block}_{at} 短标签不得为空");
            // 下标一致性（**由构造保证、本身零判别力**；实质的"逐行同序"校验见 core-bin 的
            // `short_label_keys_hit_registered_rows_and_registration_wording_is_superset`，
            // 那里投到 `point_table` 这条独立真源上 ⇒ 错位必红）
            assert_eq!(
                label,
                PERIPH_LABELS[i].0,
                "两表下标必须一致（下标 {i}：{block}_{at}）"
            );
            assert_eq!(
                unit_for(*role, block, *at),
                PERIPH_LABELS[i].1,
                "单位真源（W-2）：unit_for 必须等于同下标表项（下标 {i}）"
            );
            if let Some(unit) = PERIPH_LABELS[i].1 {
                assert!(!unit.trim().is_empty(), "{block}_{at} 单位不得为空串（无量纲用 None）");
            }
        }
    }

    /// 短标签 **不得**含登记说明字符（全角括号 / 分号）——防 `point_table` 登记 `label` 误入
    /// （T-7 断言项；登记 label 是"含全角括号的说明文本"，直上屏会引入 189 码位缺口）。
    #[test]
    fn short_labels_never_contain_registration_punctuation() {
        for (label, unit) in PERIPH_LABELS {
            for bad in ['（', '）', '；', '，'] {
                assert!(
                    !label.contains(bad),
                    "短标签 `{label}` 含登记说明字符 `{bad}` —— 说明误用了 point_table 登记 label"
                );
            }
            if let Some(unit) = unit {
                assert!(!unit.contains('（') && !unit.contains('）'), "单位 `{unit}` 不得含全角括号");
            }
        }
        // 反例锚点：登记 label 确实含全角括号 ⇒ 本断言有鉴别力（不是恒真的空断言）
        assert!("系统状态（位图；bit14 主电故障）".contains('（'));
    }

    /// `fire_det` 模板 6 行的短标签**逐字** = [`FIRE_DET_TEMPLATE_LABELS`]
    /// （两处字面量必须一致：一处是 run-time 模板常量，一处是短标签表本体）。
    #[test]
    fn fire_det_template_rows_match_locked_literals() {
        assert_eq!(FIRE_DET_TEMPLATE_LABELS, ["地址", "状态", "数据 1", "CO", "VOC", "H₂"]);
        let rows: Vec<&str> = (1..=6u16)
            .map(|at| label_for(PeriphRole::Fire, "fire_det", at).expect("fire_det 模板行必须有短标签"))
            .collect();
        assert_eq!(rows, FIRE_DET_TEMPLATE_LABELS.to_vec());
        // 单位：CO / VOC / H₂ 为 ppm，前三列无量纲
        assert_eq!(unit_for(PeriphRole::Fire, "fire_det", 1), None);
        assert_eq!(unit_for(PeriphRole::Fire, "fire_det", 2), None);
        assert_eq!(unit_for(PeriphRole::Fire, "fire_det", 3), None);
        for at in 4..=6u16 {
            assert_eq!(unit_for(PeriphRole::Fire, "fire_det", at), Some("ppm"), "at={at}");
        }
    }

    /// `label_for` / `unit_for` 对 `fire_det` 的**运行期展开行**自带归约
    /// （`at = 6(k−2)+j` → 模板位 `j`）；非 `fire_det` 块**不**归约；白名单外显式 `None`。
    #[test]
    fn fire_det_expansion_is_normalized_and_unknown_keys_are_none() {
        // k=2 的第 1 只 ⇒ at = 7..12 ↔ 模板 1..6
        for j in 1..=6u16 {
            assert_eq!(
                label_for(PeriphRole::Fire, "fire_det", 6 + j),
                label_for(PeriphRole::Fire, "fire_det", j),
                "展开行 at={} 必须归约到模板位 {j}",
                6 + j
            );
        }
        // n=100 ⇒ 末只末寄存器 at = 6n−6 = 594（与 §15.2.4 的容量口径同源）
        assert_eq!(label_for(PeriphRole::Fire, "fire_det", 594), Some("H₂"));
        // 非 fire_det 块不归约（at 越界即 None）
        assert_eq!(label_for(PeriphRole::Hvac, "hvac_in", 7), None);
        // 不在白名单的键 ⇒ None（既有排除项的短标签同样不得存在）
        assert_eq!(label_for(PeriphRole::Hvac, "hvac_di", 26), None, "hvac_di_26 保留位");
        assert_eq!(unit_for(PeriphRole::Hvac, "hvac_di", 26), None);
        assert_eq!(label_for(PeriphRole::Pcs, "pcs_3zone", 50), None, "1049 STS 电压幅值不上屏");
        assert_eq!(label_for(PeriphRole::Unknown, "nope", 1), None);
    }

    /// 分组标题：`group_of()` 的**全部返回值**都能在 [`GROUP_TITLES`] 里找到标题；
    /// 且键集 = `group_of` 键集 ∪ {`station_status`, `device_info`}（§15.7.3 按字面量 33 ⇒ 去重 31）。
    #[test]
    fn every_group_key_has_a_literal_title() {
        let mut keys_from_group_of: Vec<&str> = Vec::new();
        for (role, block, at) in PERIPH_WHITELIST {
            let g = group_of(*role, block, *at);
            assert_ne!(g, GROUP_UNKNOWN, "白名单点不得落 unknown");
            if !keys_from_group_of.contains(&g) {
                keys_from_group_of.push(g);
            }
        }
        assert_eq!(keys_from_group_of.len(), 31, "group_of 的分组键恰 31 个");

        // ① 每个 group_of 键都有标题
        for key in &keys_from_group_of {
            assert!(
                GROUP_TITLES.iter().any(|(k, _)| k == key),
                "分组键 `{key}` 在 GROUP_TITLES 里没有标题（屏上无处安放）"
            );
        }
        // ② 表内**没有**多余键（除了两个页面级分组）
        for (key, _) in GROUP_TITLES {
            assert!(
                keys_from_group_of.contains(key) || *key == "station_status" || *key == "device_info",
                "GROUP_TITLES 的键 `{key}` 既不在 group_of 键集内，也不是 §15.5.2 装置段的页面级键"
            );
        }
        assert_eq!(GROUP_TITLES.len(), 33, "§15.7.3 按字面量 33 项");
        assert_eq!(
            keys_from_group_of.len() + 2,
            GROUP_TITLES.len(),
            "33 = group_of 的 31 + 装置段 2 个页面级键"
        );

        // ③ 去重后恰 31（`装置信息` ×2 / `功率` ×2 各计 1；告警位两处**不**合并）
        let mut titles: Vec<&str> = GROUP_TITLES.iter().map(|(_, t)| *t).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), 31, "去重后 31（§15.7.3 订正 v2.1-r3 ③）");

        // ④ 括号与数字是**字面量的一部分**（§15.7.3 明令不得剥掉；`告警位（20）` ≠ `告警位（288）`）
        for literal in ["告警位（20）", "告警位（288）", "辅助状态位（10）", "电能（累计量）", "火警等级"] {
            assert!(
                GROUP_TITLES.iter().any(|(_, t)| *t == literal),
                "分组标题 `{literal}` 必须**逐字面量**存在（含全角括号与数字）"
            );
        }
        assert!(!GROUP_TITLES.iter().any(|(_, t)| *t == "告警位"), "不得剥掉括号 ⇒ 不得出现 `告警位`");
        assert!(!GROUP_TITLES.iter().any(|(_, t)| *t == "电能"), "不得剥掉括号 ⇒ 不得出现 `电能`");
        // 键不得重复（重复会让"标题完备"断言失真）
        let mut keys: Vec<&str> = GROUP_TITLES.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "GROUP_TITLES 的键不得重复");
    }

    /// UI 固定文案常量（§15.7.3 表**逐条**）：非空、且设计点名的**必须互不相同**对
    /// 逐对成立（BMS 告警三态 EDGE-24 / 版本降级 vs 通道断开 EX-29）。
    #[test]
    fn ui_text_constants_cover_table_and_keep_mandatory_distinctions() {
        let all: &[&str] = &[
            ui_text::STATION_OFFLINE, ui_text::NOT_READ, ui_text::RANGE_ERROR,
            ui_text::NOT_CONFIGURED, ui_text::NAME_UNKNOWN, ui_text::DETAIL_UNAVAILABLE,
            ui_text::UNAVAILABLE,
            ui_text::PERIPH_UNAVAILABLE, ui_text::FIRE_SOURCE_UNAVAILABLE,
            ui_text::NO_ACTIVE_ALARM_BIT, ui_text::BMS_ALARM_SOURCE_UNAVAILABLE,
            ui_text::PAGE_DEVICE_AND_PERIPH,
            ui_text::BIT_UNDEFINED, ui_text::BIT_RESERVED, ui_text::BIT_ACTIVE,
            ui_text::BIT_INACTIVE, ui_text::ONLINE, ui_text::OFFLINE,
            ui_text::BIT_ALARM_TOTAL, ui_text::BIT_FAULT_TOTAL, ui_text::BIT_COMM_STATE,
            ui_text::ENUM_NORMAL, ui_text::ENUM_FIRE_LEVEL1, ui_text::ENUM_FIRE_LEVEL2,
            ui_text::ENUM_FIRE_UNDEFINED, ui_text::ENUM_FIRE_EMG_START, ui_text::ENUM_FIRE_EMG_STOP,
            ui_text::ENUM_UNKNOWN, ui_text::ENUM_STOPPED, ui_text::ENUM_RUNNING,
            ui_text::LAST_OK, ui_text::LAST_UPDATE, ui_text::REGISTERED, ui_text::READABLE,
            ui_text::COUNT_ALARM, ui_text::COUNT_FAULT, ui_text::CATALOG_STALE,
            ui_text::TRUNCATED_TO, ui_text::ENUM_PENDING_VENDOR,
            ui_text::VIEW_DETAIL, ui_text::VIEW_ALL, ui_text::PREV_PAGE, ui_text::NEXT_PAGE,
            ui_text::COLLAPSE, ui_text::RETRY, ui_text::PAGE_PREFIX, ui_text::PAGE_SUFFIX,
            ui_text::VERSION_MISMATCH, ui_text::FLASH_SAME_VERSION, ui_text::CHANNEL_DOWN_RETRYING,
        ];
        assert_eq!(all.len(), 50, "§15.7.3「UI 固定文案」表逐条 = 50 条");
        for s in all {
            assert!(!s.trim().is_empty(), "固定文案不得为空");
        }
        // BMS 告警三态（EDGE-24 / EX-16）**两两不相等**
        assert_ne!(ui_text::NO_ACTIVE_ALARM_BIT, ui_text::BMS_ALARM_SOURCE_UNAVAILABLE);
        assert_ne!(ui_text::NO_ACTIVE_ALARM_BIT, ui_text::STATION_OFFLINE);
        assert_ne!(ui_text::BMS_ALARM_SOURCE_UNAVAILABLE, ui_text::STATION_OFFLINE);
        // 版本不匹配 vs 通道断开（EX-29 的屏侧一半）
        assert_ne!(ui_text::VERSION_MISMATCH, ui_text::CHANNEL_DOWN_RETRYING);
        // 取值降级五语义（T-14）两两互异（`不可用` 是通用兜底、不在五语义内）
        let five = [ui_text::STATION_OFFLINE, ui_text::NOT_READ, ui_text::RANGE_ERROR,
                    ui_text::NOT_CONFIGURED, ui_text::PERIPH_UNAVAILABLE];
        for (i, a) in five.iter().enumerate() {
            for b in &five[i + 1..] {
                assert_ne!(a, b, "取值降级语义 `{a}` / `{b}` 不得同串");
            }
        }
    }

    /// 消防迁移常量：枚举 6 值（权威 = PRD F21 展示表，**不是** `SIG_FIRE_LEVEL` 的 4 条）、
    /// 位语义三条（含 **bit15 不启用**）、拆解两项（高字节烟雾 / 低字节温度 raw−55）。
    #[test]
    fn fire_enum_bits_and_decompose_are_locked() {
        assert_eq!(
            FIRE_LEVEL_ENUM,
            [(0, "正常"), (1, "一级报警"), (2, "二级火警"), (3, "未定义"), (4, "紧急启动"), (5, "紧急停止")]
        );
        assert_eq!(FIRE_SYS_BITS, [(14, "主电故障"), (13, "备电故障"), (11, "驱动电路"), (10, "压力传感器"), (9, "电磁阀"), (8, "喷洒标记")]);
        assert_eq!(FIRE_TRIGGER_BITS, [(0, "干接点触发"), (1, "复合触发"), (2, "预留")]);
        assert_eq!(FIRE_DETECTOR_STATE_BITS, [(12, "报警总状态"), (14, "故障总状态")]);
        // **bit15「通信状态」不得默默启用**（R-41 追认前无生产者）
        assert!(!FIRE_DETECTOR_STATE_BITS.iter().any(|(i, _)| *i == 15));
        assert_eq!(ui_text::BIT_COMM_STATE, "通信状态", "常量保留以便 R-41 追认后启用");

        let dec = data1_decompose();
        assert_eq!(dec.len(), 2);
        assert_eq!(dec[0].label, "烟雾");
        assert_eq!(dec[0].unit.as_deref(), Some("dB/M"));
        assert_eq!(dec[0].decimals, 1);
        assert_eq!(
            dec[0].from,
            crate::peripherals::DecodeFrom::HighByte { scale: 0.1, offset: 0.0 }
        );
        assert_eq!(dec[1].label, "温度");
        assert_eq!(dec[1].unit.as_deref(), Some("℃"));
        assert_eq!(dec[1].decimals, 0);
        assert_eq!(
            dec[1].from,
            crate::peripherals::DecodeFrom::LowByte { scale: 1.0, offset: -55.0 }
        );
    }
}
