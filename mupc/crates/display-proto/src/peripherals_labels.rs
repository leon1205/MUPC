//! U-73 外设**白名单与分组键**（设计 §15.3.2 W-1/W-4 / §15.4 / §15.5.2 分组表）。
//!
//! 本文件是设计 §15.11 #3 落点的**第一部分**：
//! - [`PERIPH_WHITELIST`]：白名单（`(role, block, at)` 常量数组，**可机械枚举**）——
//!   W-1（结构性排除项不在表内）与 W-4（"行数相等"断言的前提）由本表成立；
//! - [`group_of`] / [`GROUP_UNKNOWN`]：页内分组键（HMI 用**静态 map** 映射到中文分组标题，标题是 UI 字面量，受既有码表扫描覆盖）。
//!
//! ⚠️ **本文件暂缺 §15.11 #3 的第二部分（屏用短标签表白名单 `label_for`）——如实登记**：
//!
//! 设计 §15.3.1 / §15.7 **F-4** 要求 catalog 的 `label` 取值 = **本 crate 的短标签表白名单**
//! （"值 = 精炼短标签"），**不得**直接用 `point_table` 的登记 `label`（后者是含全角括号的登记说明文本，设计阶段实测字库缺口 **189** 码位）。短标签表是 **M 级数据工程**（555 条逐点文案），且按 §15.7 **H-1** 必须与字库扩充（`fonts/font_subset_charset.txt` + 重跑 `gen_fonts.sh` + 同步 `lv_font_cmap.txt`）**同批提交**——故 **T20 不产出该表**：catalog 的 `label` 暂取 `point_table` 登记 `label`（`mupcd` 侧投影）。**该偏离已登记**（F-4 / W-3 / H-3 **未闭合**，须与 H-1 同批收口）。
//!
//! 本文件完整提供白名单 ⇒ **取数、组帧、容量守卫、负向验收（EX-08 / EX-21 / EX-23）的结构性判据全部可用**，与短标签表是否就绪**解耦**。

use crate::peripherals::PeriphRole;

/// 未登记 / 不在白名单点的分组键（**不臆造**；HMI 侧无对应标题 ⇒ 该点不上屏）。
pub const GROUP_UNKNOWN: &str = "unknown";

/// **外设上屏白名单**：`(role, 块名, at)` 常量数组（设计 §15.5.2 字段表逐条枚举；§15.4 消防）。
///
/// 口径（**W-1 / W-4**）：
/// - **只有本表内的点进帧 / 进 catalog** ⇒ 排除项（`hvac_di_26` 保留位、`mb_phase_7/_8` PT/CT、`pcs_3zone_47..66` = 1046–1065、`bms_io` 未列入项、台区总表）**结构性不上屏**——EX-08 / EX-21 / EX-23 的"屏上无该字段"因此**由表成立**，无需屏侧过滤；
/// - `fire_det` 随 n 展开 ⇒ 本表内以**模板 6 行**（`at` 1..6 = `+0 地址`…`+5 H₂`）登记，运行期按键公式 `fire_det_{6(k−2)+j}` 展开（与 `point_table.rs` 的 `FIRE_DET_TEMPLATE_START = 17` / `FIRE_DET_STRIDE = 6` 同源）；
/// - 合计 **447** 行 = 非 `fire_det` **441** + 模板 **6**（与 §15.2.4 的容量分子同值）。
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
}
