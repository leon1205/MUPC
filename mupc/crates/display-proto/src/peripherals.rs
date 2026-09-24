//! U-73 外设数值上屏（F20–F24）——帧内**外设段**的 DTO + 帧预算守卫（设计 §15.2）。
//!
//! 本文件落设计 §15.2.2（段类型，**逐字照该节编码**）与 §15.2.4（容量守卫常量与守卫函数）。
//!
//! **元数据（中文名 / 单位 / 小数位 / 位语义 / 枚举文案）不在本段**——走设计 §15.3 的
//! catalog 只读端点，因元数据入帧是**结构性爆帧**：仅元数据一项即
//! `555 × 118 B = 65,490 B = 63.96 KiB` ≈ 整帧上限，整帧（n=100）**≈102.7 KiB ≫ 64 KiB**
//! （§15.2.4 反证）。屏侧行数与 catalog 行数恒等，靠"白名单内每一点都在段内"保证
//! （`flag != Valid` 时 `v = None`，**不补 0、不沿用旧值**）。
//!
//! ⚠️ **命名冲突提醒**：本模块的 [`PointValue`]（**线格式**点值：`at` / `v` / `flag`）与
//! 01 设计 §9.1.2 的 `mupc_data_processing::latest_values::PointValue`（**内存快照**：
//! `value` / `ts_ms` / `quality`）**同名不同物**。两处在 `display_host.rs` 内会同时可见
//! ⇒ 实现**必须**用限定路径或别名（`use display_proto::PointValue as FramePoint`），
//! **不得**靠 `use` 通配把两者混在一处。
//!
//! 帧预算常量（`POINT_JSON_BYTES_*` / `PERIPH_FIXED_JSON_BYTES` / `EXISTING_SEGMENTS_RESERVE` /
//! `MAX_PERIPH_BYTES`）按设计 §15.11 #1 的落点**定义在 [`crate::frame`]**（与
//! [`crate::frame::MAX_FRAME_BYTES`] 同处，单一真源）；本模块只消费并派生守卫取值。

use crate::frame::{
    DisplayFrame, FieldFlag, MAX_PERIPH_BYTES, PERIPH_FIXED_JSON_BYTES, POINT_JSON_BYTES_UPPER,
};

/// U-73 外设段（F20–F24）。**元数据不在本段**（见模块文档）。
///
/// `Default` = `available=false` ⇒ 屏显「外设数据不可用」（EDGE-22），**绝不伪装**：
/// 旧帧（v2）无本段 / 慢拍未接线 / 快照不可得，都落在这个缺省上（设计 §15.2.1 / §15.2.3）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PeripheralsSection {
    /// 本段组帧时刻（Unix ms；0 = 未采集）。
    pub ts_ms: u64,
    /// 段可用性；`Default = false` ⇒ 「外设数据不可用」（EDGE-22），绝不伪装。
    pub available: bool,
    /// 元数据版本（§15.3.1）；屏侧据此决定是否重取 catalog。
    pub catalog_rev: u32,
    /// 被帧预算守卫裁剪的块（形如 `"fire_det:119→111"`，**左 = 登记只数、右 = 实际携带只数**；
    /// `119→111` 的 `n` 与 `k_max` 取自 §15.2.4「守卫取值」表，二者必须一致）。
    /// 常态为空（PRD 上限 n=100 不触发）。**屏侧必须显式提示，不得静默**（F21.4）。
    pub truncated: Vec<String>,
    /// 站点，顺序 = 站配置顺序（稳定不跳位，PRD §4.2.4）。
    pub stations: Vec<PeripheralStation>,
}

/// 外设站（一个南向站一段）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PeripheralStation {
    /// 南向站 id（`south_stations.stations[].id`，如 "hvac" / "fire" / "bms" /
    /// "meter_batt" / "pcs"）。
    pub id: String,
    /// role 字面量（snake_case；见 [`PeriphRole`]）。
    pub role: PeriphRole,
    /// 站在线。**采集侧按 `stale_timeout_s = 5 s` 判定后的结论**；屏侧**不重判、不另立门限**
    /// （F25.1：新鲜度判据单一真源）。
    pub online: bool,
    /// 最后一次采集成功时刻（Unix ms；0 = 从未成功）→ 屏显「最后成功 12:03:44」。
    ///
    /// **来源 = 01 设计 §9.1 的 `station_last_poll_ms(station)`**（`station_poll_ms` 私有字段的
    /// 公开读口，**新增接口要求 R-38**）。未落地 ⇒ **本字段恒 0**（屏显 `--`，**不臆造**）。
    /// **禁止**由 `display_host` 自维护第二份"最后成功时刻"（同一事实不记两份）。
    pub last_ok_ms: u64,
    /// 消防「钢瓶气压未配置」标志（EDGE-23；**仅 role=Fire 有意义**）：
    /// `Some(false)` ⇒ 屏显「未配置」（**忽略 `v`**，不得显 `0 kPa`，不产告警）；
    /// `Some(true)` ⇒ 正常按值展示；`None` ⇒ 不可得（非消防站 / 接缝未接线）⇒ 按值正常展示。
    pub cylinder_configured: Option<bool>,
    /// 块，顺序 = 站配置 `regs` 顺序。
    pub blocks: Vec<PeripheralBlock>,
}

/// 外设 role。与 `mupc-southd::config::Role` 的 serde 名（`rename_all = "snake_case"`）
/// 逐字对应；**不含 `MeterGrid`**——台区总表不在本增量内（§15 范围外 #3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriphRole {
    /// 空调。
    Hvac,
    /// 消防。
    Fire,
    /// 电池（BMS）。
    Battery,
    /// 储能表。
    MeterBatt,
    /// PCS。
    Pcs,
    /// 不可得 / 未登记（**缺省态**，不臆造 role）。
    #[default]
    Unknown,
}

/// 外设块（点名前缀 + 点值表；块内点**全量携带**，取数成败只改 `flag` / `v`，不改行数）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PeripheralBlock {
    /// 块名（点名前缀）：`hvac_in` / `hvac_di` / `fire_sys` / `fire_det` / `bms_io` /
    /// `bms_energy` / `bms_meta` / `bms_term` / `bms_cap` / `bms_alarm` / `mb_ui` /
    /// `mb_freq_line` / `mb_power` / `mb_phase` / `mb_e_act_*` / `pcs_3zone`。
    pub name: String,
    /// 本块最后一次成功读取时刻（Unix ms；0 = 未读）→ 屏显「最近更新 …」（F25.2）。
    pub ts_ms: u64,
    /// 显式点名覆盖：`(at, name)`（如 `(7, "fire_det_count")`、`(19, "soc")`）；空 = 全位置式。
    /// **由组帧侧从站配置的 `PointConf.name` 直接投影**（无第二份命名规则）。
    pub renames: Vec<(u16, String)>,
    /// 点值，顺序 = `at` 升序。**白名单内每一点都在**（含未取到的点，其 `flag != Valid`、
    /// `v = None`）⇒ 屏侧行数与 catalog 行数恒等，**不因取数成败而变行**（F25 / §4.2.4）。
    pub values: Vec<PointValue>,
}

impl PeripheralBlock {
    /// **点键的唯一构造点**：显式 `name` 覆盖优先，否则 `<块名>_<at>`（位置式口径）。
    /// 屏侧**不得**自行拼接或重命名。
    pub fn key(&self, pv: &PointValue) -> String {
        match self.renames.iter().find(|(at, _)| *at == pv.at) {
            Some((_, n)) => n.clone(),
            None => format!("{}_{}", self.name, pv.at),
        }
    }

    /// 已携带的探测器件数（`fire_det` 专用：每只 [`FIRE_DET_POINTS_PER_UNIT`] 点）。
    /// 非 `fire_det` 块调用无意义（守卫只对 `fire_det` 生效，§15.2.4 步骤 4）。
    pub fn fire_det_units(&self) -> usize {
        self.values.len() / FIRE_DET_POINTS_PER_UNIT
    }
}

/// 单点值（**线格式**，与 `latest_values::PointValue` 同名不同物，见模块文档）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PointValue {
    /// 块内偏移 + 1（位置式口径）。
    pub at: u16,
    /// 工程值：**量纲已由采集侧按登记 `scale` / `offset` 消解，屏侧不重算**
    /// （F20.1 / F22.4 / F24.1）；位点以 `0.0` / `1.0` 表达。
    /// **`flag != Valid` 时必须为 `None`**（不补 0、不沿用旧值）。
    pub v: Option<f64>,
    /// 点级有效 / 降级标志。**四态语义与取值不得改**（F26.7）。
    pub flag: FieldFlag,
}

// ─────────────────────────────────────────────────────────────────────────────
// 帧预算守卫（设计 §15.2.4）—— 常量 + **纯函数**
//
// **归属说明（必须读）**：设计 §15.11 #6 把"预算守卫"的**调用点**安排在
// `mupc-core-bin/src/display_host.rs::build_frame` 内（组帧侧硬要求），本 crate 属**契约层**
// ⇒ 这里只提供**可调用的纯函数 + 常量**（无 I/O、不改任何状态），由组帧侧按
// §15.2.4 的步骤 1→5 顺序调用。组帧侧接线属后续增量（本任务只动 `display-proto`）。
// ─────────────────────────────────────────────────────────────────────────────

/// `fire_det` 块名（块名 = 点名前缀，§15.2.2 `PeripheralBlock::name`）。
pub const FIRE_DET_BLOCK_NAME: &str = "fire_det";

/// 每只探测器在帧内占的点数（模板 6 行/只，§15.2.4「fire_det 寄存器数 = 6×(n−1)」）。
pub const FIRE_DET_POINTS_PER_UNIT: usize = 6;

/// 非 `fire_det` 白名单点数（`555 − 6×19 = 441`，**与 n 无关**；§15.2.4 步骤 3/4）。
pub const PERIPH_NON_FIRE_DET_POINTS: usize = 441;

/// 非 `fire_det` 块的**上界**字节数 = `441 × 48 B + 4 KiB = 21,168 + 4,096 = 25,264 B`
/// （§15.2.4 步骤 4，按 [`POINT_JSON_BYTES_UPPER`] 而非典型值）。
pub const PERIPH_NON_FIRE_DET_UPPER_BYTES: usize =
    PERIPH_NON_FIRE_DET_POINTS * POINT_JSON_BYTES_UPPER + PERIPH_FIXED_JSON_BYTES;

/// `fire_det` 可携带的**最大探测器件数**（`k_max`，§15.2.4「守卫取值」表）：
///
/// ```text
/// k_max = ⌊(MAX_PERIPH_BYTES − 非 fire_det 上界) / (6 × POINT_JSON_BYTES_UPPER)⌋
///       = ⌊(57,344 − 25,264) / (6 × 48)⌋ = ⌊32,080 / 288⌋ = 111 只
/// ```
///
/// **可机械校验的关键性质**：`k_max = 111 ≥ 99 只`（= n=100 时 `fire_det` 的只数）
/// ⇒ **PRD 上限内不裁剪**（§15.2.4 结论 ②，T-4b）。
pub const K_MAX_FIRE_DET: usize =
    (MAX_PERIPH_BYTES - PERIPH_NON_FIRE_DET_UPPER_BYTES) / (FIRE_DET_POINTS_PER_UNIT * POINT_JSON_BYTES_UPPER);

/// **裁剪触发的最小 n**（每只探测器的寄存器数）：只数 = `n − 1 > K_MAX_FIRE_DET` ⇒ `n ≥ 113`
/// （§15.2.4「裁剪触发条件」：超出 PRD 上限 100 达 13 只才裁）。
pub const FIRE_DET_TRUNCATE_MIN_N: usize = K_MAX_FIRE_DET + 2;

/// 裁剪决策（纯函数）：`fire_det` 实到只数 → 实际保留只数（`min(只数, k_max)`）。
pub const fn fire_det_keep_units(carried_units: usize) -> usize {
    if carried_units > K_MAX_FIRE_DET {
        K_MAX_FIRE_DET
    } else {
        carried_units
    }
}

/// **前缀截断**（§15.2.4 步骤 4：探测器按地址升序，前缀截断，保证可复现）：
/// 把 `values` 截到前 `keep_units` 只（= `keep_units × 6` 点），返回被丢弃的点数（0 = 未裁）。
pub fn truncate_fire_det_prefix(values: &mut Vec<PointValue>, keep_units: usize) -> usize {
    let keep_points = keep_units * FIRE_DET_POINTS_PER_UNIT;
    if values.len() <= keep_points {
        return 0;
    }
    let dropped = values.len() - keep_points;
    values.truncate(keep_points);
    dropped
}

/// `truncated` 条目的**唯一构造点**：`"fire_det:<登记只数>→<实际携带只数>"`（§15.2.2 字段注）。
pub fn fire_det_truncated_note(registered_units: usize, carried_units: usize) -> String {
    format!("{FIRE_DET_BLOCK_NAME}:{registered_units}→{carried_units}")
}

/// **§15.2.4 步骤 4 的预检与裁剪**（纯函数，就地改 `section`）：
/// 对每个 `fire_det` 块按 [`POINT_JSON_BYTES_UPPER`] 的上界口径预检，超 `k_max` 只则前缀截断
/// 并 push `truncated` 条目。
///
/// 返回是否发生了裁剪。**幂等**：已裁到 `≤ k_max` 只的段再次调用返回 `false` 且不再 push。
/// 调用方：`display_host::build_frame`（组帧侧）——**不得**在帧路径做 I/O（§15.1.1）。
pub fn enforce_fire_det_budget(section: &mut PeripheralsSection) -> bool {
    let PeripheralsSection {
        truncated,
        stations,
        ..
    } = section;
    let mut trimmed = false;
    for station in stations.iter_mut() {
        for block in station.blocks.iter_mut() {
            if block.name != FIRE_DET_BLOCK_NAME {
                continue;
            }
            let carried = block.fire_det_units();
            let keep = fire_det_keep_units(carried);
            if keep == carried {
                continue;
            }
            let dropped = truncate_fire_det_prefix(&mut block.values, keep);
            debug_assert!(dropped > 0);
            // 左 = 登记只数（裁前实到只数）· 右 = 实际携带只数（F21.4：不得静默）
            truncated.push(fire_det_truncated_note(carried, keep));
            trimmed = true;
        }
    }
    trimmed
}

/// **§15.2.4 步骤 5（出口守卫）的结论**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitGuardOutcome {
    /// 整帧可编码（未降级）——**注意**：本变体不等于"发布必定成功"，非尺寸类错误
    /// （如 [`crate::Error::AlarmMessageTooLong`]）不归本守卫管。
    NoChange,
    /// 外设段超帧预算 ⇒ 已置 `available=false` 并清空载荷；**该帧照常发布**，
    /// 既有段可用性不受影响（屏显「外设数据不可用」，不黑屏）。
    PeripheralsDowngraded,
    /// 外设段已降级但整帧仍超 `MAX_FRAME_BYTES`（既有段自身超预算）⇒ 调用方**不得发布**该帧。
    StillTooLarge,
}

/// **§15.2.4 步骤 5 的出口守卫**（纯函数）：对**既有唯一出口** [`DisplayFrame::to_json_slice`]
/// 的 `Err(FrameTooLarge)` 兜底——把外设段置不可用，使既有段仍能照常发布。
///
/// **为什么必须同时清空载荷**（实现要点，设计只写了 `available=false`）：仅翻转
/// `available` 一个布尔位**不减少任何字节**，整帧仍会被 `MAX_FRAME_BYTES` 拒 ⇒
/// "该帧照常发布"无法达成。消费者在 `available=false` 时按 §15.2.3「段缺失 / 未采集 ⇒
/// **整段**显『外设数据不可用』」**不读段体**，故清空 `stations` / `truncated` 与设计语义等价。
/// `ts_ms` / `catalog_rev` 保留（诊断用，且不占可观测语义）。
///
/// **`Err` 判据只认 `FrameTooLarge`**：其它错误（告警消息超长等）不属于本守卫职责，原样返回
/// [`ExitGuardOutcome::NoChange`]，避免"张冠李戴地丢掉外设数据"。
pub fn enforce_exit_guard(frame: &mut DisplayFrame) -> ExitGuardOutcome {
    let too_large = matches!(
        frame.to_json_slice(),
        Err(crate::Error::FrameTooLarge { .. })
    );
    if !too_large {
        return ExitGuardOutcome::NoChange;
    }
    frame.peripherals.available = false;
    frame.peripherals.truncated.clear();
    frame.peripherals.stations.clear();
    match frame.to_json_slice() {
        Ok(_) => ExitGuardOutcome::PeripheralsDowngraded,
        // **§15.2.4 步骤 5 只有两种结论：可发（`PeripheralsDowngraded`）与不可发。**
        // 尺寸仍超 ⇒ 不可发（`StillTooLarge` 的既有语义）。
        Err(crate::Error::FrameTooLarge { .. }) => ExitGuardOutcome::StillTooLarge,
        // 非尺寸类编码错误（如告警文本超长）**同样不可发**——清空外设段并不减少告警文本，
        // 故清空后理论上不会新出现这类错误；但若真出现（将来新增编码错误），
        // `to_json_slice()` 非 `Ok` ⇒ 该帧**不得发布**。归入"不可发"而不是谎报"未降级"
        // （T19 评审残留 ③②：`Err(_) => NoChange` 是"已改状态却报未改"的误导标签）。
        Err(_) => ExitGuardOutcome::StillTooLarge,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 元数据（点表目录）与明细下钻的三个只读端点 DTO —— 设计 §15.3.2（**逐字照抄该节的类型定义**）
//
// **为什么元数据不进帧**（§15.3.1）：仅元数据一项即 `555 × 118 B = 63.96 KiB` ≈ 整帧上限，
// n=100 整帧 **≈102.7 KiB ≫ 64 KiB** ⇒ 结构性爆帧。故走**控制通道**的只读 GET 端点
// （一次性 / 带参 / 有限额的受控读，§3.4），**不进 PL-1 审计**（无副作用 ⇒ 无审计条目）。
// ─────────────────────────────────────────────────────────────────────────────

/// 外设点表目录（**静态元数据**，一次性读取；不进主帧）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PeripheralCatalog {
    /// `= catalog_rev(自身)`；与帧内 [`PeripheralsSection::catalog_rev`] **同源同值**。
    pub rev: u32,
    /// 构建时刻（Unix ms）。
    pub generated_ms: u64,
    /// 站点，顺序 = 站配置顺序。
    pub stations: Vec<CatalogStation>,
}

/// 目录中的一站。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CatalogStation {
    /// 南向站 id。
    pub id: String,
    /// role 字面量。
    pub role: PeriphRole,
    /// 该站是否在 `south_stations` 中启用（false ⇒ 屏侧在 P6「装置」段的站状态表显「未启用」）。
    pub enabled: bool,
    /// 块，顺序 = 站配置 `regs` 顺序。
    pub blocks: Vec<CatalogBlock>,
}

/// 目录中的一块。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CatalogBlock {
    /// 块名（点名前缀）。
    pub name: String,
    /// 块形态（决定屏侧呈现：数值行 vs 位行）。
    pub kind: CatalogBlockKind,
    /// 与 [`PeripheralBlock::renames`] **同源同值**（组帧侧从站配置 `PointConf.name` 直接投影）。
    pub renames: Vec<(u16, String)>,
    /// 白名单点，顺序 = `at` 升序。
    pub points: Vec<CatalogPoint>,
}

/// 块形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogBlockKind {
    /// 标量块（寄存器读数）。
    #[default]
    Scalar,
    /// 离散位块（`discrete`，逐位产点）。
    Discrete,
}

/// 目录中的一点（屏用短标签 + 单位 + 小数位 + 位语义 + 枚举文案 + 拆解规格 + 分组键）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CatalogPoint {
    /// 块内偏移 + 1（位置式口径）。
    pub at: u16,
    /// **屏用短标签**。真源 = `display-proto` 短标签表白名单（设计 §15.3.2 / §15.7 F-4），
    /// **不是** `point_table` 的登记 `label`（后者是含全角括号的登记说明文本）。
    pub label: String,
    /// 单位（如 `℃` / `%RH` / `V` / `A` / `kW` / `kvar` / `kVA` / `kPa` / `ppm` / `dB/M` /
    /// `kWh` / `kvarh` / `kΩ` / `Hz`）；`None` = 无量纲（如功率因数）。
    pub unit: Option<String>,
    /// 小数位。**由登记 `scale` 派生**（[`decimals_from_scale`]）；屏侧不得自行决定。
    pub decimals: u8,
    /// 位语义（两种位形态统一表达，见 [`BitMeta`]）。
    pub bits: Vec<BitMeta>,
    /// 枚举文案（值 → 文案）；空 = 非枚举 / 文案未登记（屏侧显「模式 `<值>`」，**不猜**）。
    pub enum_labels: Vec<(u16, String)>,
    /// 展示层拆解规格（F21.5「数据 1」；空 = 不拆解、按整字显示）。
    pub decompose: Vec<Decompose>,
    /// 页内分组键（固定枚举字面量，见设计 §15.4 / §15.5.2 分组表）。
    pub group: String,
}

/// 位语义。两种承载形态统一表达（位点与标量在帧内同构）：
/// - **离散位块**的点（`hvac_di` / `bms_alarm`）：[`CatalogPoint::bits`] 恰含 1 项，
///   `index` = 该点自身位号（= `at − 1`）；
/// - **字内位图**的点（消防 `fire_sys_1` / `_3` / `_4` / `_5`、探测器 `+1 状态` 整字）：
///   `bits` 含该字的**全部 16 位**；**未定义位 `defined = false`** ⇒ 屏显「**未定义位 n**」，
///   **不猜语义**（F21.1 / EX-09）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BitMeta {
    /// 位号（0 起）。
    pub index: u8,
    /// 已定义位的中文名；未定义位为空串。
    pub label: String,
    /// 位分类。
    pub class: CatalogBitClass,
    /// 是否已定义（false ⇒ 屏显「未定义位 n」，**不得**为凑满 16 位编造语义）。
    pub defined: bool,
    /// 该位的活跃语义（屏侧文案用）；`None` = 用通用「活跃 / 非活跃」。
    pub active_text: Option<String>,
    /// 与其余位**极性相反**的位（如探测器 bit15「0 = 离线」）。
    ///
    /// **R-41 裁定前无生产者**：catalog 构建器一律填 `false`；探测器 bit15 走
    /// `defined: false` ⇒ 屏显「未定义位 15」。若产品 / 厂方追认「通信状态」，**只改白名单 +
    /// 本字段**即可启用（屏侧代码零改动）。
    pub inverted: bool,
}

/// 位分类（与 `mupc-southd::point_table::BitClass` 语义一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogBitClass {
    /// 告警位。
    Alarm,
    /// 状态位。
    State,
    /// 保留位（`defined = false` 的来源之一）。
    #[default]
    Reserved,
}

/// 展示层拆解（F21.5）。[`DecodeFrom`] 是**唯一**的字节语义来源，屏侧不自行猜位序。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Decompose {
    /// 拆解项名（如「烟雾」/「温度」）。
    pub label: String,
    /// 单位。
    pub unit: Option<String>,
    /// 小数位。
    pub decimals: u8,
    /// 取值来源（整字 / 高字节 / 低字节）。
    pub from: DecodeFrom,
}

/// 拆解的字节语义来源。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecodeFrom {
    /// 整字。
    Whole {
        /// 标度。
        scale: f64,
        /// 偏移。
        offset: f64,
    },
    /// 高字节。
    HighByte {
        /// 标度。
        scale: f64,
        /// 偏移。
        offset: f64,
    },
    /// 低字节。
    LowByte {
        /// 标度。
        scale: f64,
        /// 偏移。
        offset: f64,
    },
}

/// 探测器明细分页（F21.4）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FireDetectorPage {
    /// 页码（1 起）。
    pub page: u32,
    /// 单页条数。
    pub page_size: u32,
    /// 登记数（`fire_det_count`）；`None` = 未取数（屏显 `--`）。
    pub total: Option<u16>,
    /// 实际可读只数（由配置展开决定）。`expanded != total` ⇒ 屏侧**显式提示不一致**，
    /// **不得静默裁剪**（F21.4 / EX-12）。
    pub expanded: u16,
    /// 是否还有下一页。
    pub has_more: bool,
    /// 消防源可用性。`false` ⇒ 「消防源不可用」，**不得**显为"无探测器"（EDGE-09 同口径）。
    pub available: bool,
    /// 本页条目。
    pub items: Vec<FireDetectorItem>,
}

/// 探测器明细单条（6 点/只）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FireDetectorItem {
    /// 探测器序号（1 起，与登记数对齐）。
    pub index: u16,
    /// `+0 地址`（1–254）。
    pub addr: PointValue,
    /// `+1 状态`（整字；位语义见 catalog `bits`）。
    pub state: PointValue,
    /// `+2 数据 1`（整字；拆解见 catalog `decompose`，F21.5）。
    pub data1: PointValue,
    /// `+3` CO ppm。
    pub co: PointValue,
    /// `+4` VOC ppm。
    pub voc: PointValue,
    /// `+5` H₂ ppm。
    pub h2: PointValue,
}

/// BMS 告警位下钻分页（F22.3）。**名称由 catalog 按下标提供**，本 DTO 不重复携带。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BmsAlarmPage {
    /// 页码（1 起）。
    pub page: u32,
    /// 单页条数。
    pub page_size: u32,
    /// 位数总量（288；供窗口化列表算总高）。
    pub total: u32,
    /// 当前活跃位数。
    pub active_total: u32,
    /// 是否还有下一页。
    pub has_more: bool,
    /// 告警源可用性：`false` ⇒ 「BMS 告警源不可用」（**≠** 「无活跃告警位」，EDGE-24）。
    pub available: bool,
    /// 本页条目。
    pub items: Vec<BmsAlarmItem>,
}

/// BMS 告警位单条。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BmsAlarmItem {
    /// 点内偏移 + 1（键 = `bms_alarm_{at}`）。
    pub at: u16,
    /// 该位是否活跃。
    pub active: bool,
}

/// **`decimals_from_scale` 的唯一实现**（设计 §15.3.2 映射表）：
/// `decimals = 0 if scale >= 1.0 else (-log10(scale)).round() as u8`。
///
/// 覆盖四值：`1.0 → 0` / `0.1 → 1` / `0.01 → 2` / `0.001 → 3`（登记表已核对无其它值）。
/// 非有限 / 非正值（登记表不该出现）保守取 0——**不 panic**（catalog 构建路径不得因单点脏值
/// 打挂整个端点；脏值本身由 `point_table` 的登记约束负责）。
pub fn decimals_from_scale(scale: f64) -> u8 {
    if !scale.is_finite() || scale >= 1.0 {
        return 0;
    }
    if scale <= 0.0 {
        return 0;
    }
    (-scale.log10()).round() as u8
}

/// 元数据版本 = catalog **稳定文本**的 CRC32。
///
/// **单一真源**：同一函数同时产出 [`PeripheralCatalog::rev`] 与
/// [`PeripheralsSection::catalog_rev`]（设计 §15.3.1）。
///
/// 「稳定文本」的定义（必须**排除**易变字段，否则同一份点表每次构建都得到不同的 `rev`，
/// 屏侧会无休止重取 catalog）：`rev` 与 `generated_ms` 之外的**全部内容**，
/// 按 [`serde_json::to_vec`] 的确定性字段序序列化后取 CRC32。
/// `serde_json` 对结构体按**声明序**输出、`Vec` 保序 ⇒ 同一份内容恒得同一 `rev`。
pub fn catalog_rev(cat: &PeripheralCatalog) -> u32 {
    let mut stable = cat.clone();
    stable.rev = 0;
    stable.generated_ms = 0;
    match serde_json::to_vec(&stable) {
        Ok(bytes) => crc32_ieee(&bytes),
        // 契约层自洽（上述类型全为可序列化 POD）⇒ 不可达；取 0 = 「未取得」语义，不 panic
        Err(_) => 0,
    }
}

/// CRC32（IEEE 802.3，反射多项式 `0xEDB88320`）——**本地实现**（不新增依赖）。
///
/// 用途仅 `catalog_rev` 的内容指纹（非防碰撞安全用途）。自检向量见单测：
/// `crc32_ieee(b"123456789") == 0xCBF4_3926`。
pub fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for b in data {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn station(id: &str, role: PeriphRole) -> PeripheralStation {
        PeripheralStation {
            id: id.to_string(),
            role,
            online: true,
            last_ok_ms: 1_757_412_000_000,
            cylinder_configured: None,
            blocks: vec![],
        }
    }

    fn point(at: u16, v: Option<f64>, flag: FieldFlag) -> PointValue {
        PointValue { at, v, flag }
    }

    // ---- 缺省语义：默认段 = 不可用（EDGE-22），绝不出"空段正常" ----
    #[test]
    fn default_section_is_explicitly_unavailable() {
        let s = PeripheralsSection::default();
        assert!(!s.available, "缺省 = 「外设数据不可用」，不得伪装");
        assert_eq!(s.ts_ms, 0, "未采集 ⇒ ts_ms = 0（不臆造时刻）");
        assert_eq!(s.catalog_rev, 0);
        assert!(s.truncated.is_empty() && s.stations.is_empty());
    }

    // ---- PeriphRole：serde 名与 southd 的 Role 逐字对应；缺省不臆造 ----
    #[test]
    fn periph_role_json_names_and_default() {
        for (role, name) in [
            (PeriphRole::Hvac, "\"hvac\""),
            (PeriphRole::Fire, "\"fire\""),
            (PeriphRole::Battery, "\"battery\""),
            (PeriphRole::MeterBatt, "\"meter_batt\""),
            (PeriphRole::Pcs, "\"pcs\""),
            (PeriphRole::Unknown, "\"unknown\""),
        ] {
            assert_eq!(serde_json::to_string(&role).unwrap(), name);
        }
        assert_eq!(PeriphRole::default(), PeriphRole::Unknown, "缺省不臆造 role");
        // 范围外：台区总表（MeterGrid）不在本段内 —— 其 JSON 名必须解码失败
        assert!(serde_json::from_str::<PeriphRole>("\"meter_grid\"").is_err());
    }

    // ---- 点键：位置式 + renames 覆盖（屏侧不得自行拼接）----
    #[test]
    fn block_key_positional_and_renames_override() {
        let blk = PeripheralBlock {
            name: "fire_det".to_string(),
            ts_ms: 0,
            renames: vec![(7, "fire_det_count".to_string())],
            values: vec![
                point(1, None, FieldFlag::NotRead),
                point(7, Some(3.0), FieldFlag::Valid),
            ],
        };
        assert_eq!(blk.key(&blk.values[0]), "fire_det_1", "位置式：<块名>_<at>");
        assert_eq!(
            blk.key(&blk.values[1]),
            "fire_det_count",
            "显式 name 覆盖优先"
        );

        let soc_blk = PeripheralBlock {
            name: "bms_meta".to_string(),
            ts_ms: 0,
            renames: vec![(19, "soc".to_string())],
            values: vec![point(19, Some(65.0), FieldFlag::Valid)],
        };
        assert_eq!(soc_blk.key(&soc_blk.values[0]), "soc");

        // 无 renames ⇒ 全位置式
        let plain = PeripheralBlock {
            name: "hvac_in".to_string(),
            ts_ms: 0,
            renames: vec![],
            values: vec![point(3, Some(1.0), FieldFlag::Valid)],
        };
        assert_eq!(plain.key(&plain.values[0]), "hvac_in_3");
    }

    // ---- 四态往返 + v=None 不补 0 ----
    #[test]
    fn point_value_four_flags_roundtrip_and_never_fills_zero() {
        let blk = PeripheralBlock {
            name: "mb_ui".to_string(),
            ts_ms: 1,
            renames: vec![],
            values: vec![
                point(1, Some(1.0), FieldFlag::Valid),
                point(2, None, FieldFlag::NotRead),
                point(3, None, FieldFlag::Offline),
                point(4, None, FieldFlag::RangeError),
            ],
        };
        let json = serde_json::to_string(&blk).unwrap();
        assert!(json.contains(r#"{"at":1,"v":1.0,"flag":"valid"}"#));
        assert!(json.contains(r#"{"at":2,"v":null,"flag":"not_read"}"#));
        assert!(json.contains(r#"{"at":3,"v":null,"flag":"offline"}"#));
        assert!(json.contains(r#"{"at":4,"v":null,"flag":"range_error"}"#));
        // **禁补 0**：降级点的 v 必须是 null，绝不出现 `"v":0`
        assert!(!json.contains(r#""v":0,"#), "降级点不得补 0（EX-30）");
        let back: PeripheralBlock = serde_json::from_str(&json).unwrap();
        assert_eq!(back, blk, "四态稳定往返");
        for pv in &back.values {
            if pv.flag != FieldFlag::Valid {
                assert!(pv.v.is_none(), "flag != Valid ⇒ v 必须 None");
            }
        }
    }

    // ---- 守卫常量：k_max 复算 + 三档口径单调 + PRD 上限内不裁 ----
    #[test]
    fn guard_constants_are_self_consistent() {
        assert_eq!(
            crate::frame::MAX_PERIPH_BYTES,
            crate::frame::MAX_FRAME_BYTES - crate::frame::EXISTING_SEGMENTS_RESERVE,
            "MAX_PERIPH_BYTES 必须由帧上限与既有段预留派生（单一真源）"
        );
        assert_eq!(MAX_PERIPH_BYTES, 56 * 1024, "56 KiB");
        assert_eq!(PERIPH_NON_FIRE_DET_UPPER_BYTES, 25_264);
        // 独立复算 k_max（不引用常量自身）：⌊(57,344 − 25,264) / 288⌋ = 111
        assert_eq!((57_344_usize - 25_264) / (6 * 48), 111);
        assert_eq!(K_MAX_FIRE_DET, 111, "§15.2.4 守卫取值表");
        // 三档口径单调且用途不互替（经局部绑定断言，避免 clippy 的常量断言告警）
        let (typical, upper, abs_max) = (
            crate::frame::POINT_JSON_BYTES_TYPICAL,
            POINT_JSON_BYTES_UPPER,
            crate::frame::POINT_JSON_BYTES_F64_ABS_MAX,
        );
        assert!(typical <= upper, "TYPICAL({typical}) ≤ UPPER({upper})");
        assert!(upper <= abs_max, "UPPER({upper}) ≤ F64_ABS_MAX({abs_max})");
        assert_eq!((typical, upper, abs_max), (33, 48, 60));
        // PRD 上限（n=100 ⇒ 99 只）不触发裁剪的**机械保证**
        let k_max = K_MAX_FIRE_DET;
        let prd_limit_units = 99;
        assert!(k_max >= prd_limit_units, "k_max({k_max}) 必须 ≥ 99 只");
        // 裁剪触发的最小 n：只数 = n−1 > 111 ⇒ n ≥ 113
        assert_eq!(FIRE_DET_TRUNCATE_MIN_N, 113);
        let trigger = FIRE_DET_TRUNCATE_MIN_N;
        assert_eq!(fire_det_keep_units(prd_limit_units), prd_limit_units);
        assert!(trigger - 1 > k_max, "n=112 ⇒ 111 只 > k_max 不成立时说明触发点漂移");
        assert!(trigger - 2 <= k_max, "n=111 ⇒ 110 只 ≤ k_max");
    }

    // ---- 裁剪决策与触发点边界：n=112 不裁 / n=113 裁（只数 111 vs 112）----
    #[test]
    fn keep_units_boundary_at_k_max() {
        assert_eq!(fire_det_keep_units(99), 99, "PRD 上限内不裁");
        assert_eq!(fire_det_keep_units(K_MAX_FIRE_DET), 111, "恰为 k_max 不裁");
        assert_eq!(fire_det_keep_units(K_MAX_FIRE_DET + 1), 111, "112 只 ⇒ 裁到 111");
        // n=112 ⇒ 111 只（不裁）；n=113 ⇒ 112 只（裁）
        assert_eq!(fire_det_keep_units(FIRE_DET_TRUNCATE_MIN_N - 2), 111);
        assert_eq!(fire_det_keep_units(FIRE_DET_TRUNCATE_MIN_N - 1), 111);
    }

    #[test]
    fn truncate_prefix_keeps_ascending_head() {
        // 121 只（726 点）⇒ 裁到 111 只（666 点），丢 60 点
        let mut values: Vec<PointValue> = (1..=726u16)
            .map(|at| point(at, Some(1.0), FieldFlag::Valid))
            .collect();
        let dropped = truncate_fire_det_prefix(&mut values, 111);
        assert_eq!(dropped, 60);
        assert_eq!(values.len(), 666, "111 只 × 6 点");
        assert_eq!(values[0].at, 1, "前缀截断 = 保留最小的 at");
        assert_eq!(values[665].at, 666);
        // 未超 ⇒ 不动、返回 0
        assert_eq!(truncate_fire_det_prefix(&mut values, 111), 0);
        assert_eq!(values.len(), 666);
    }

    // ---- 步骤 4：整段预检（n=100 不裁 / n=120 ⇒ "fire_det:119→111"）----
    #[test]
    fn enforce_budget_no_op_at_prd_limit_and_trims_beyond_k_max() {
        let mk = |units: usize| {
            let mut s = PeripheralsSection {
                ts_ms: 7,
                available: true,
                catalog_rev: 3,
                truncated: vec![],
                stations: vec![station("fire", PeriphRole::Fire)],
            };
            s.stations[0].blocks = vec![PeripheralBlock {
                name: FIRE_DET_BLOCK_NAME.to_string(),
                ts_ms: 7,
                renames: vec![(7, "fire_det_count".to_string())],
                values: (1..=units as u16 * 6)
                    .map(|at| point(at, Some(1.0), FieldFlag::Valid))
                    .collect(),
            }];
            s
        };

        // ① n=100（99 只）⇒ 不裁、truncated 空
        let mut ok = mk(99);
        assert!(!enforce_fire_det_budget(&mut ok));
        assert!(ok.truncated.is_empty(), "PRD 上限内不得留下裁剪痕迹");
        assert_eq!(ok.stations[0].blocks[0].values.len(), 594);

        // ② n=120（119 只 > k_max=111）⇒ 裁到前 111 只，条目格式逐字
        let mut over = mk(119);
        assert!(enforce_fire_det_budget(&mut over));
        assert_eq!(over.truncated, vec!["fire_det:119→111".to_string()]);
        assert_eq!(over.stations[0].blocks[0].values.len(), 666);
        assert_eq!(over.stations[0].blocks[0].fire_det_units(), 111);
        assert_eq!(over.stations[0].blocks[0].values[0].at, 1, "前缀（地址升序）");
        assert!(over.available, "裁剪不得改动 available（≠ 段不可用）");

        // 幂等：已裁过再调用不再 push
        assert!(!enforce_fire_det_budget(&mut over));
        assert_eq!(over.truncated.len(), 1);

        // 非 fire_det 块不受影响（白名单 441 点与 n 无关）
        let mut other = wide_non_fire_det_block();
        assert!(!enforce_fire_det_budget(&mut other));
        assert!(other.truncated.is_empty());
        assert_eq!(other.stations[0].blocks[0].values.len(), 441);
    }

    /// 非 `fire_det` 块即使点数很多也不触发裁剪（守卫只针对 `fire_det`，§15.2.4 步骤 3/4）。
    fn wide_non_fire_det_block() -> PeripheralsSection {
        let mut s = PeripheralsSection {
            ts_ms: 1,
            available: true,
            catalog_rev: 1,
            truncated: vec![],
            stations: vec![station("bms", PeriphRole::Battery)],
        };
        s.stations[0].blocks = vec![PeripheralBlock {
            name: "bms_alarm".to_string(),
            ts_ms: 1,
            renames: vec![],
            values: (1..=441u16)
                .map(|at| point(at, Some(1.0), FieldFlag::Valid))
                .collect(),
        }];
        s
    }

    #[test]
    fn truncated_note_matches_design_literal() {
        assert_eq!(fire_det_truncated_note(119, 111), "fire_det:119→111");
        assert_eq!(fire_det_truncated_note(20, 19), "fire_det:20→19");
    }

    // ---- §15.3.2：catalog / 分页 DTO 的**响应体形态与字段名**（逐字段钉死）----

    /// 三端点响应体的 JSON 字段名**逐字**（改字段名 = 改线上契约 ⇒ 用例当红）。
    #[test]
    fn catalog_dto_json_field_names_are_the_wire_contract() {
        let cat = PeripheralCatalog {
            rev: 7,
            generated_ms: 1_757_412_000_000,
            stations: vec![CatalogStation {
                id: "fire".into(),
                role: PeriphRole::Fire,
                enabled: true,
                blocks: vec![CatalogBlock {
                    name: "fire_sys".into(),
                    kind: CatalogBlockKind::Scalar,
                    renames: vec![(7, "fire_det_count".into())],
                    points: vec![CatalogPoint {
                        at: 6,
                        label: "火警等级".into(),
                        unit: None,
                        decimals: 0,
                        bits: vec![BitMeta {
                            index: 12,
                            label: "报警总状态".into(),
                            class: CatalogBitClass::Alarm,
                            defined: true,
                            active_text: Some("报警".into()),
                            inverted: false,
                        }],
                        enum_labels: vec![(0, "正常".into())],
                        decompose: vec![Decompose {
                            label: "烟雾".into(),
                            unit: Some("dB/M".into()),
                            decimals: 1,
                            from: DecodeFrom::HighByte {
                                scale: 0.1,
                                offset: 0.0,
                            },
                        }],
                        group: "fire_level".into(),
                    }],
                }],
            }],
        };
        let j = serde_json::to_string(&cat).unwrap();
        for key in [
            r#""rev":7"#,
            r#""generated_ms":1757412000000"#,
            r#""stations":[{"id":"fire","role":"fire","enabled":true,"blocks":[{"name":"fire_sys","kind":"scalar","renames":[[7,"fire_det_count"]],"points":[{"at":6,"label":"火警等级","unit":null,"decimals":0,"bits":[{"index":12,"label":"报警总状态","class":"alarm","defined":true,"active_text":"报警","inverted":false}],"enum_labels":[[0,"正常"]],"decompose":[{"label":"烟雾","unit":"dB/M","decimals":1,"from":{"high_byte":{"scale":0.1,"offset":0.0}}}],"group":"fire_level"}]}]}]}"#,
        ] {
            assert!(j.contains(key), "catalog 线格式缺字段/形状不符：{key}\n实得 {j}");
        }
        // 往返稳定（对端解析回同值）
        let back: PeripheralCatalog = serde_json::from_str(&j).unwrap();
        assert_eq!(back, cat);

        // 分页 DTO 字段名 + `#[serde(default)]`（缺键 ⇒ 空页而不是解析失败）
        let fp = FireDetectorPage {
            page: 1,
            page_size: 20,
            total: Some(20),
            expanded: 19,
            has_more: false,
            available: true,
            items: vec![FireDetectorItem {
                index: 1,
                addr: point(1, Some(1.0), FieldFlag::Valid),
                state: point(2, Some(0.0), FieldFlag::Valid),
                data1: point(3, Some(0.0), FieldFlag::Valid),
                co: point(4, None, FieldFlag::NotRead),
                voc: point(5, None, FieldFlag::NotRead),
                h2: point(6, None, FieldFlag::NotRead),
            }],
        };
        let j = serde_json::to_string(&fp).unwrap();
        for key in [
            r#""page":1"#,
            r#""page_size":20"#,
            r#""total":20"#,
            r#""expanded":19"#,
            r#""has_more":false"#,
            r#""available":true"#,
            r#""index":1"#,
            r#""addr":{"at":1,"v":1.0,"flag":"valid"}"#,
            r#""state":{"at":2,"v":0.0,"flag":"valid"}"#,
            r#""data1""#,
            r#""co":{"at":4,"v":null,"flag":"not_read"}"#,
            r#""voc""#,
            r#""h2""#,
        ] {
            assert!(j.contains(key), "fire_detectors 响应缺字段：{key}\n实得 {j}");
        }
        // `expanded != total` **如实返回**（不静默裁剪，F21.4 / EX-12）
        assert_ne!(fp.expanded, fp.total.unwrap());
        assert_eq!(serde_json::from_str::<FireDetectorPage>("{}").unwrap(), FireDetectorPage::default());

        let bp = BmsAlarmPage {
            page: 1,
            page_size: 50,
            total: 288,
            active_total: 3,
            has_more: true,
            available: false,
            items: vec![BmsAlarmItem {
                at: 2,
                active: true,
            }],
        };
        let j = serde_json::to_string(&bp).unwrap();
        for key in [
            r#""page":1"#,
            r#""page_size":50"#,
            r#""total":288"#,
            r#""active_total":3"#,
            r#""has_more":true"#,
            r#""available":false"#,
            r#""items":[{"at":2,"active":true}]"#,
        ] {
            assert!(j.contains(key), "bms_alarms 响应缺字段：{key}\n实得 {j}");
        }
        assert_eq!(serde_json::from_str::<BmsAlarmPage>("{}").unwrap(), BmsAlarmPage::default());
    }

    /// `decimals_from_scale` 四值覆盖 + 越界保守 0（§15.3.2 映射表）。
    #[test]
    fn decimals_from_scale_covers_registered_scales() {
        for (scale, want) in [(1.0, 0u8), (0.1, 1), (0.01, 2), (0.001, 3)] {
            assert_eq!(decimals_from_scale(scale), want, "scale={scale}");
        }
        // 非四值：按公式外推（0.5 → 0；0.0001 → 4）
        assert_eq!(decimals_from_scale(0.5), 0);
        assert_eq!(decimals_from_scale(0.0001), 4);
        // 脏值（不该出现）：保守 0，不 panic
        for bad in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            assert_eq!(decimals_from_scale(bad), 0, "脏值 scale={bad} 保守 0");
        }
    }

    /// `catalog_rev`：① CRC32 自检向量；② 同内容恒同 rev；③ `rev` / `generated_ms` **不入**指纹
    /// （否则每次构建都变 ⇒ 屏侧无休止重取 catalog）；④ 内容真变则 rev 变（键、标签、单位、
    /// 小数位、位语义、枚举、拆解、分组**逐维度**改动都须触发）。
    #[test]
    fn catalog_rev_is_stable_and_content_sensitive() {
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926, "CRC32/IEEE 自检向量");
        assert_eq!(crc32_ieee(b""), 0, "空输入");

        let mk = || PeripheralCatalog {
            rev: 0,
            generated_ms: 0,
            stations: vec![CatalogStation {
                id: "hvac".into(),
                role: PeriphRole::Hvac,
                enabled: true,
                blocks: vec![CatalogBlock {
                    name: "hvac_in".into(),
                    kind: CatalogBlockKind::Scalar,
                    renames: vec![],
                    points: vec![CatalogPoint {
                        at: 1,
                        label: "柜内温度".into(),
                        unit: Some("℃".into()),
                        decimals: 1,
                        bits: vec![],
                        enum_labels: vec![],
                        decompose: vec![],
                        group: "hvac_measure".into(),
                    }],
                }],
            }],
        };

        let a = catalog_rev(&mk());
        // ② `rev` / `generated_ms` 变化**不得**改变指纹（它们不在稳定文本内）
        let mut b = mk();
        b.rev = 99;
        b.generated_ms = 1_757_412_000_000;
        assert_eq!(catalog_rev(&b), a, "rev/generated_ms 不计入指纹");

        // ④ 内容变 ⇒ 指纹变（逐维度）
        let mut c = mk();
        c.stations[0].blocks[0].points[0].label = "柜内温度值".into();
        assert_ne!(catalog_rev(&c), a, "标签变化须改变指纹");
        let mut d = mk();
        d.stations[0].blocks[0].points[0].decimals = 2;
        assert_ne!(catalog_rev(&d), a, "小数位变化须改变指纹");
        let mut e = mk();
        e.stations[0].blocks[0].points[0].unit = None;
        assert_ne!(catalog_rev(&e), a, "单位变化须改变指纹");
        let mut f = mk();
        f.stations[0].blocks[0].kind = CatalogBlockKind::Discrete;
        assert_ne!(catalog_rev(&f), a, "块形态变化须改变指纹");
        let mut g = mk();
        g.stations[0].enabled = false;
        assert_ne!(catalog_rev(&g), a, "站启用态变化须改变指纹");

        // ③ 同内容（不同实例）恒同 rev —— 帧内 `catalog_rev` 与端点 `rev` 的"同源同值"前提
        assert_eq!(catalog_rev(&mk()), a);
    }

    /// 出口守卫的非尺寸类错误分支：清空外设段后仍非 `Ok` ⇒ **不可发布**（不是"未降级"）。
    ///
    /// 构造：告警文本超长（`AlarmMessageTooLong`，清理外设段**不**减少告警字节）⇒ 首次检查
    /// 本就不是 `FrameTooLarge` ⇒ 走 `NoChange`（那是"不归本守卫管"的正确语义）。
    /// 本用例钉的是**判据只在 `FrameTooLarge` 上**（不得"张冠李戴"丢外设数据）。
    #[test]
    fn exit_guard_only_acts_on_frame_too_large() {
        use crate::frame::{
            AlarmItem, AlarmLevel, AlarmsSection, Field, SocSource, PROTO_VERSION,
        };
        let missing = Field {
            v: None,
            flag: FieldFlag::Offline,
        };
        let mut frame = DisplayFrame {
            version: PROTO_VERSION,
            seq: 1,
            ts_ms: 1,
            soc: None,
            soc_source: SocSource::Lost,
            soc_flag: FieldFlag::Offline,
            run_state: None,
            pcs_online: false,
            p_phase: [missing; 3],
            p_total: missing,
            i_phase: [missing; 3],
            inconsistency: false,
            device: Default::default(),
            info: Default::default(),
            interlock: Default::default(),
            // 告警文本超长（`AlarmMessageTooLong`，清理外设段**不**减少告警字节）
            alarms: AlarmsSection {
                ts_ms: 1,
                available: true,
                items: vec![AlarmItem {
                    ts_ms: 1,
                    level: AlarmLevel::Warn,
                    message: "x".repeat(crate::frame::MAX_ALARM_MESSAGE_BYTES + 10),
                }],
            },
            peripherals: PeripheralsSection {
                ts_ms: 1,
                available: true,
                catalog_rev: 9,
                truncated: vec![],
                stations: vec![station("fire", PeriphRole::Fire)],
            },
        };
        let before = frame.peripherals.clone();
        assert_eq!(
            enforce_exit_guard(&mut frame),
            ExitGuardOutcome::NoChange,
            "非尺寸类错误（告警文本超长）不归出口守卫管（判据只在 FrameTooLarge 上）"
        );
        assert_eq!(frame.peripherals, before, "外设数据不得被误丢");
    }
}
