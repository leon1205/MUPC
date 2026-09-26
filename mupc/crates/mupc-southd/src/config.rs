//! `south_stations:` 配置段类型（core_config 嵌入用；§10.3）。
//!
//! 定义站级南向统一调度的配置结构：轮询周期、新鲜度门限、站（role/port/slave/parity）
//! 与每站寄存器块（块 = 一次读事务；块内 `points[]` = 逐点换算口径，S3b-2 §11.4.1）。
//! 校验仅限段内（跨段/互斥在 core-bin validate——Task 6）。

use mupc_data_processing::meter_regs::RegFormat;
use serde::{Deserialize, Serialize};

// S3b-3（T8 返工）：探测器 1 的**地址寄存器**号的**唯一真源**在 mapper（与 `fire_chain_head`
// 同源）—— 本文件**不得**再定义一份同值常量（此前 `FIRE_CHAIN_HEAD_REG` 即被删除的双定义）
use crate::mapper::FIRE_DET1_ADDR_REG;
use crate::points::{self, PointKind, PointSpec};
// S3b-3（T8）：块级周期分组与「组 → 块集」映射的**唯一实现**在 scheduler（配置期与调度期共用）
use crate::scheduler::{read_groups_of, ReadGroup};

/// 32 位值的字序 —— **全项目唯一一处定义在 `mupc_data_processing::meter_regs`**
/// （设计 §11.4.2：它与 `RegDecode` 同居，解码原语与字序参数不可分离）。
/// 本 crate 只 `pub use` 复用，**不得**再定义一份（v1.2 的双定义即被否掉的 B3 缺陷）。
pub use mupc_data_processing::meter_regs::WordOrder;

pub const DEFAULT_POLL_MS: u64 = 1000;
pub const DEFAULT_STALE_TIMEOUT_S: u64 = 5;
pub const DEFAULT_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_BAUD_RATE: u32 = 9600;
/// 策略 5s 数据新鲜度共享常量落点（M-6）：单一真源在 data-processing
/// （`mupc_data_processing::DATA_FRESHNESS_MS`），此处别名引用避免双定义漂移。
pub const DATA_FRESHNESS_MS: u64 = mupc_data_processing::DATA_FRESHNESS_MS;

/// "最快允许轮询节奏"的**唯一常量**（PRD §10.3.2 **C2**；设计 §12.7 常量段）：
/// 站级 `pcs` 周期下界（规则 18）与**块级**周期下界（规则 20 的 C2 分支）**共用同一常量**
/// —— 二者同源同值（项目内"最快允许轮询节奏"的唯一先例），防双定义漂移。
pub const MIN_POLL_INTERVAL_MS: u64 = 500;

/// `pcs` 站轮询周期下界（PRD §9.3.2.2(2) + §9.8.1 末条；设计 §11.5.1 规则 18）。
/// `pcs` 无 `< 5000` 上界（不参与控制决策），只有这条下界——防"误配的超短周期打满总线"。
/// **既有名保留为别名**（设计 §12.7：「**不得删除**：既有单测与文档引用它」）。
pub const PCS_MIN_INTERVAL_MS: u64 = MIN_POLL_INTERVAL_MS;

/// 站级串口校验位（PRD §9.4.1 `parity`；YAML: `none` 缺省 / `even` / `odd`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StationParity {
    /// 无校验（缺省，与 rs485 `Config::default()` 一致）
    #[default]
    None,
    /// 偶校验（空调厂方默认，§9.10 Q-14/RC-6）
    Even,
    /// 奇校验
    Odd,
}

/// 站类型角色（南向调度语义划分，YAML 用 snake_case）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 并网点主表（策略 phase 真源）
    MeterGrid,
    /// 电池侧电表（能量流核算）
    MeterBatt,
    /// 电池簇 BMS 采集
    Battery,
    /// 温控/空调（消防/热管理联动）
    Hvac,
    /// 消防子系统（联动/联锁）
    Fire,
    /// 两级式 PCS（只读 3 区，S3b-2 §9.3.2；不参与控制决策，不推进 5s 闸门）
    Pcs,
}

/// 单站配置（role + 端口 + 从站地址 + 采集间隔 + 寄存器块）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StationConf {
    pub id: String,
    pub role: Role,
    pub port: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub slave: u8,
    /// 口波特率（同口各站必须一致——物理共享口波特率；缺省 9600）
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    /// 串口校验位（同口各站必须一致——物理共享校验位；缺省 none）
    #[serde(default)]
    pub parity: StationParity,
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
    #[serde(default)]
    pub regs: Vec<RegBlockConf>,
}

/// 寄存器块读取功能码（YAML: `holding` / `input` / `discrete`）。
/// 默认 FC03 保持寄存器；FC04 输入寄存器供厂方点表用 input regs 的设备（解码同构）；
/// FC02 离散输入（S3b-2 G-3）——此时 `count` 语义为**位数**、`addr` 为**位地址**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegFunc {
    /// 保持寄存器（FC0x03，默认）
    Holding,
    /// 输入寄存器（FC0x04）
    Input,
    /// 离散输入（FC0x02，位块）
    Discrete,
}

/// 寄存器块配置（一次读事务：起始地址 + 传输口径 + **块级缺省换算** + 可选逐点清单）。
///
/// S3b-2 新增字段**全部 `#[serde(default)]` 且缺省 = 既有行为**（设计 §11.4.1），
/// 故既有 `meter_grid` 写法（`{ name: p, addr: 0x1000, format: int32_scaled, scale: 0.01,
/// count: 6 }`）的解析结果与改动前**逐字段相同**。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegBlockConf {
    pub name: String,
    pub addr: u16,
    /// 功能码（FC03 保持 / FC04 输入 / FC02 离散输入；缺省 FC03）
    #[serde(default = "default_reg_func")]
    pub func: RegFunc,
    /// **注意：RegFormat 无 Default**，故用 `#[serde(default = "default_reg_format")]`
    /// （serde(default) 需要 Default 实现，此处不可用）。
    #[serde(default = "default_reg_format")]
    pub format: RegFormat,
    #[serde(default)]
    pub scale: f64,
    #[serde(default = "default_reg_count")]
    pub count: u16,
    // ── S3b-2 新增（PRD §9.4.2.4 块级字段表；缺省 = 既有行为）──
    /// 换算偏移：`值 = raw × scale + offset`（G-2；`offset` 只表示零点平移，与符号性无关，
    /// 见 PRD §9.4.2.4「换算与符号性」）

    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub offset: f64,
    /// 逐寄存器字节低-高互换（G-5；PCS 专用，PRD §9.7.3）
    #[serde(default, skip_serializing_if = "is_false")]
    pub byte_swap: bool,
    /// 逐点换算口径清单（G-4）；空 = 窗口内**每个值槽** 1 点（PRD §9.4.2.1 第 4 条）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<PointConf>,
    /// 现场分片豁免标记：`true` 仅豁免"块落地极大性"（规则 15），不豁免其它各条
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_slice: bool,
    // ── S3b-3 新增（PRD §10.3.1；设计 §12.2.1，**唯一新增字段**）──
    /// **块级采集周期覆盖**：`Some(v)` = 本块按 v ms 轮询；`None`（**缺省**）= 继承站级
    /// `interval_ms`。**缺省 ⇒ 既有配置与既有行为零变化**（PRD §10.3.1 定性 1；回归锚
    /// `block_interval_serde_roundtrip_is_unchanged_when_absent`）。
    ///
    /// 取值由 PRD §10.3.2 的 C1–C5 约束（本 crate 落 [`validate_block_intervals`] 规则 20/21）
    /// 与 C6/C7（规则 22，按 `R(role)` 判）；`skip_serializing_if` 使**既有 YAML 往返逐字不变**。
    /// **`None` 与 `Some(站周期)` 在校验语义上等价、在配置意图上不同**——与点级
    /// `scale`/`offset` 用 `Option<T>` 区分"未声明"与"显式值"是同一取向（设计 §12.2.1）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
}

/// 块的有效采集周期 —— **唯一求值点**（校验期与调度期共用同一函数，防"两处各算一次"漂移）。
///
/// `None`（缺省）= 继承站级 `interval_ms`；`Some(v)` = 本块显式声明 v ms。
impl RegBlockConf {
    pub fn effective_interval_ms(&self, station_interval_ms: u64) -> u64 {
        self.interval_ms.unwrap_or(station_interval_ms)
    }
}

/// 点级换算口径（PRD §9.4.2.4 点级字段表）。
///
/// **为什么用 `Option<T>` 而不是"缺省值语义"**：`scale`/`offset` 的"未声明"与"显式 0"
/// 是**两种不同事实**——前者须继承块级，后者是配置错误（`scale == 0` 拒）。用 `Option`
/// 让"继承"与"显式 0"在类型上可区分，校验器才能既拒 `scale: 0` 又不误伤"继承块级 scale"的点。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PointConf {
    /// 块内**寄存器/位偏移 + 1**（1 起）；32 位点填其**低地址寄存器**的序号
    pub at: u16,
    /// 自 `at` 起连续产出 `count` 个点（同换算、地址递增）；**32 位点必须 1**
    #[serde(default = "default_point_count")]
    pub count: u16,
    /// 显式点名（站内唯一）；用于跨文档契约点（如 `soc`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 点级格式；`None` = 继承块级
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<RegFormat>,
    /// 点级比例；`None` = 继承块级
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// 点级零点平移；`None` = 继承块级
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    /// 32 位值的字序（仅 32 位格式生效；块级无该字段，故无"继承"概念）
    #[serde(default, skip_serializing_if = "is_default_word_order")]
    pub word_order: WordOrder,
}

fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn is_default_word_order(w: &WordOrder) -> bool {
    *w == WordOrder::HiLo
}

/// PCS（两级式 PCS = 实时控制模块）配置段（设计 §13.7，ADR-016）。
///
/// **独立顶层段**而非并入 `south_stations`：`StationConf` 是纯采集语义（`regs` 只有读数），
/// 无处安放控制面参数（响应超时 / 采集兼心跳周期）；且 PCS 由 `PcsHandle` **独占该口**
/// （校验规则 P-2），与站级调度器的"一口多站"模型不同。
///
/// 缺省 `enabled: false` ⇒ 既有部署 yaml **零改动即零行为变化**。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SouthPcsConfig {
    /// 是否启用 PCS 通道。`false` ⇒ 不构造 `PcsHandle`、不开该口。
    #[serde(default)]
    pub enabled: bool,
    /// 串口设备（**须独占**，不得与任何 `south_stations` 站同口 —— 校验规则 P-2）。
    #[serde(default = "default_serial_port_pcs")]
    pub port: String,
    /// 协议处理器名（与站级同源，缺省 `modbus`）。
    #[serde(default = "default_protocol")]
    pub protocol: String,
    /// PCS 从站地址（拨码，默认 1；有效 1..=247）。
    #[serde(default = "default_slave_addr_pcs")]
    pub slave: u8,
    /// 波特率（PCS 线格式 V1.3：19200 N-8-1）。
    #[serde(default = "default_pcs_baud_rate")]
    pub baud_rate: u32,
    /// 数据位。
    #[serde(default = "default_data_bits_pcs")]
    pub data_bits: u8,
    /// 停止位。
    #[serde(default = "default_stop_bits_pcs")]
    pub stop_bits: u8,
    /// 校验位（缺省 none）。
    #[serde(default)]
    pub parity: StationParity,
    /// **采集周期（兼心跳职能）**：每拍 FC04 读一次 3 区块，同时产出 SOC / 运行态 / 三相 /
    /// 全部点（设计 §13.4 —— 三条读路径合并为一条）。下界 `PCS_MIN_INTERVAL_MS`。
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
    /// 单次读写响应超时（毫秒）。缺省 200（原 `intercore.modbus_rtu.response_timeout_ms` 默认值）。
    #[serde(default = "default_pcs_response_timeout_ms")]
    pub response_timeout_ms: u64,
    /// 3 区只读点表（`pcs_3zone` 等）—— 与站级 `regs` **同一类型、同一批校验函数**
    /// （规则 P-4；实际覆盖项见 [`SouthPcsConfig::validate`] 的文档）。
    #[serde(default)]
    pub regs: Vec<RegBlockConf>,
}

impl Default for SouthPcsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_serial_port_pcs(),
            protocol: default_protocol(),
            slave: default_slave_addr_pcs(),
            baud_rate: default_pcs_baud_rate(),
            data_bits: default_data_bits_pcs(),
            stop_bits: default_stop_bits_pcs(),
            parity: StationParity::None,
            interval_ms: DEFAULT_INTERVAL_MS,
            response_timeout_ms: default_pcs_response_timeout_ms(),
            regs: Vec::new(),
        }
    }
}

impl SouthPcsConfig {
    /// 段内校验（跨段规则 **P-1/P-2** 在 core-bin —— 需同时看 `south_stations` 与
    /// `intercore`；**P-3** 在本文件 [`SouthStationsConfig::validate`]）。
    ///
    /// **P-4 的覆盖范围（设计 §13.8 订正行后的实际口径，勿夸大）**：
    /// - **点级**：`points::expand` —— `count/at ≥ 1`、`name`×`count>1` 护栏、
    ///   规则 7（点位越界）/ 8（块内点位重叠）/ 9（32 位点 `count == 1` 且不跨窗口末尾）。
    /// - **块级**：规则 5 [`validate_scale`]（`scale == 0`）/ 规则 12
    ///   [`validate_count_and_discrete_bits`] / 规则 19 [`validate_width_multiple`] /
    ///   规则 11 [`validate_anchoring`]（空洞 ≤ 4 + 首尾锚定）/ 规则 14
    ///   [`validate_block_spans`]（跨块区间重叠，须吃整段切片）。
    /// - **明确不覆盖（如实登记，不假装覆盖）**：规则 15（极大性，[`validate_maximality`]
    ///   真需 `&[StationConf]`）/ 规则 6（符号性）与规则 13（`addr == 0` 按 role）——
    ///   后两条依赖 `Role` 口径，而 `south_pcs` 段无 `role` 字段；规则 10（点**名**唯一）
    ///   也不在此（它在 [`validate_station_regs`] 的**汇聚**阶段跨块判，`expand` 只查
    ///   地址重叠、不查点名 —— 参考形态是单块段，此时与规则 8 等价走 `expand`）。
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if self.port.trim().is_empty() {
            return Err("south_pcs: port 为空".into());
        }
        if !(1..=247).contains(&self.slave) {
            return Err(format!("south_pcs: slave 越界: {}", self.slave));
        }
        if self.interval_ms < PCS_MIN_INTERVAL_MS {
            return Err(format!(
                "south_pcs: interval_ms={} 须 ≥ {}ms（防超短周期打满总线）",
                self.interval_ms, PCS_MIN_INTERVAL_MS
            ));
        }
        if self.response_timeout_ms == 0 {
            return Err("south_pcs: response_timeout_ms 须 > 0".into());
        }
        if self.regs.is_empty() {
            return Err("south_pcs: regs 为空（必填点表 —— 空点表 = 采集恒空转的静默死配）".into());
        }
        // 规则 P-4（设计 §13.8 订正行）：块的**结构规则**与站级**同一批函数**（§11.4.3 的
        // "校验期与运行期同一函数"不变量；不另起一套）。
        //
        // **顺序**：先 `expand`（点级：规则 7/8/9 + `count/at` 护栏），再逐块块级
        // （5/12/19/11），最后 `validate_block_spans` 吃**整段切片**（规则 14 跨块区间重叠）。
        //
        // ⚠️ **覆盖范围如实登记（2026-09-26 按实测重写；勿夸大）**：
        // - `points::expand` 只落"把点映射到寄存器的那一刻才能判"的**点位级**规则
        //   （7 越界 / 8 块内重叠 / 9 32 位对齐）+ 点级 `count/at`、`name`×`count` 护栏；
        //   **不含**规则 10（点**名**唯一——它在 `validate_station_regs` 的**汇聚**阶段判，
        //   须跨块收集展开结果；`expand` 只查**地址**重叠）。
        // - 5（零 scale，**最尖**：漏写 `scale` ⇒ serde 缺省 0.0 ⇒ 整块 raw×0 静默全 0）/
        //   11（空洞 ≤ 4 + 首尾锚定）/ 12（`count>0` + 位块上限）/ 14（跨块区间重叠）/
        //   19（无 points 块的宽度整数倍）**均已接线**（5/11/14 直接调用站级既有函数；
        //   12/19 原为 `validate_station_regs` 的**内联体**，已抽出为
        //   [`validate_count_and_discrete_bits`] / [`validate_width_multiple`] 后两处共用）。
        // - **明确不覆盖**（差异如实登记，不假装覆盖）：**15**（块落地极大性 ——
        //   [`validate_maximality`] 真需 `&[StationConf]`）/ **6**（符号性）/ **13**
        //   （`addr == 0` 按 role）—— 6/13 依赖 `Role` 口径，`south_pcs` 段无 `role` 字段。
        // - `prefix` 用段名占位（无站 id —— 本段是单值段），使错误文案可定位。
        let prefix = "south_pcs: ";
        for blk in &self.regs {
            let pts = crate::points::expand(blk).map_err(|e| format!("{prefix}{e}"))?;
            validate_scale(prefix, blk)?;
            validate_count_and_discrete_bits(prefix, blk)?;
            validate_width_multiple(prefix, blk)?;
            validate_anchoring(prefix, blk, &pts)?;
        }
        validate_block_spans(prefix, &self.regs)?;
        Ok(())
    }
}

fn default_serial_port_pcs() -> String {
    "/dev/ttyS0".into()
}
fn default_slave_addr_pcs() -> u8 {
    1
}
fn default_pcs_baud_rate() -> u32 {
    19200
}
fn default_data_bits_pcs() -> u8 {
    8
}
fn default_stop_bits_pcs() -> u8 {
    1
}
fn default_pcs_response_timeout_ms() -> u64 {
    200
}

/// 顶层配置段：轮询周期、数据过期门限与站表
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SouthStationsConfig {
    #[serde(default = "default_poll_ms")]
    pub poll_ms: u64,
    #[serde(default = "default_stale_timeout_s")]
    pub stale_timeout_s: u64,
    #[serde(default)]
    pub stations: Vec<StationConf>,
}

impl Default for SouthStationsConfig {
    fn default() -> Self {
        Self {
            poll_ms: DEFAULT_POLL_MS,
            stale_timeout_s: DEFAULT_STALE_TIMEOUT_S,
            stations: Vec::new(),
        }
    }
}

impl SouthStationsConfig {
    /// 段内校验（跨段/互斥在 core-bin validate）。
    ///
    /// **判定顺序（设计 §11.5.1「顺序」+ §11.5.3.4.1 实现约束，不得调整）**：
    /// ① 站级基础校验（id 非空唯一 / port / slave / interval_ms / baud_rate /
    ///    **规则 P-3（`role: pcs` 拒，指向 `south_pcs` 段）** / meter_grid·battery 新鲜度上界）
    ///    **+ 既有 `meter_grid` 整组校验**（缺相量块 / `count ≥ 6` / `int32_scaled` 显式
    ///    `scale > 0` / **块名唯一** / `addr > 0` / 区间不重叠——**原地不动，不得后移**）；
    /// ② [`validate_station_regs`]（通用规则 4/5/7/8/9/10/11/12/13/14/19，含点展开）；
    /// **②′** [`validate_block_intervals`]（S3b-3 规则 20–24，按口聚合，**须在②之后、③之前**）；
    /// ③ 跨站（单站约束计数（规则 2/3，`pcs` 两条随 P-3 不可达）、同口一致性（规则 16）、
    ///    块落地极大性（规则 15））。
    ///
    /// **为什么②′必须排在②之后（设计 §12.7 落点表注）**：②（含点展开）先给出**更具体**的文案
    ///（点位越界/重叠/点名重复等）；若先跑②′的"判据完整性"检查，可能对同一份坏配置先报出
    /// "`R(role)` 块异周期"这类**次生**结论，破既有回归锚的文案断言。
    ///
    /// **为什么①必须整组先于②**：既有 `meter_grid` 校验对同一份坏配置给出**更具体**的文案
    ///（`块名重复` / `addr 不能为 0` / `寄存器区间重叠` / `count 须 ≥ 6` / `须显式 scale>0`），
    /// 而 S3b-1c 校验语义的回归锚（§11.5.3.4 C2）逐条断言了这些文案；顺序一换即失配。
    /// 逐站短路返回首个 Err（错误消息即定位信息）。
    pub fn validate(&self) -> Result<(), String> {
        let mut ids: Vec<&str> = Vec::new();
        for s in &self.stations {
            if s.id.trim().is_empty() {
                return Err("south_stations: station id 为空".into());
            }
            if ids.contains(&s.id.as_str()) {
                return Err(format!("south_stations: 站 id 重复: {}", s.id));
            }
            ids.push(s.id.as_str());
            if s.port.trim().is_empty() {
                return Err(format!("south_stations: 站 {} port 为空", s.id));
            }
            if !(1..=247).contains(&s.slave) {
                return Err(format!(
                    "south_stations: 站 {} slave 越界: {}",
                    s.id, s.slave
                ));
            }
            if s.interval_ms == 0 {
                return Err(format!("south_stations: 站 {} interval_ms 须 > 0", s.id));
            }
            // 口波特率下界/上界：0 会静默穿透同口一致性检查（整口皆 0 时放行），
            // 到 open 才报错；>4_000_000 亦为异常值，一并在此拒。
            if s.baud_rate == 0 || s.baud_rate > 4_000_000 {
                return Err(format!(
                    "south_stations: 站 {} baud_rate 越界: {}（须 1..=4000000）",
                    s.id, s.baud_rate
                ));
            }
            // 规则 P-3（设计 §13.8 / ADR-016）：PCS 走独立顶层段 `south_pcs`，
            // 站级段**不再接受** role: pcs —— 否则两处都能配同一台 PCS（双 master）。
            // ⚠️ 本注入点必须在既有 pcs 规则之前，否则旧文案先命中、误导配置者。
            if s.role == Role::Pcs {
                return Err(format!(
                    "south_stations: 站 {} 的 role: pcs 已迁移 —— PCS 请配到顶层段 south_pcs（设计 §13.7/ADR-016）",
                    s.id
                ));
            }
            // 规则 18（`pcs` 周期下界，PRD §9.3.2.2(2) + §9.8.1 末条）随 P-3 迁移：
            // 站级 pcs 站已不可达，该下界改由 [`SouthPcsConfig::validate`] 承担
            // （`PCS_MIN_INTERVAL_MS` 常量保留复用）。
            if s.role == Role::MeterGrid && s.interval_ms >= DATA_FRESHNESS_MS {
                return Err(format!(
                    "south_stations: meter_grid 站 {} interval_ms 须 < {}ms",
                    s.id, DATA_FRESHNESS_MS
                ));
            }
            if s.role == Role::Battery && s.interval_ms >= DATA_FRESHNESS_MS {
                return Err(format!(
                    "south_stations: battery 站 {} interval_ms 须 < {}ms（BMS SOC 新鲜度窗口 5s，防源周期翻转）",
                    s.id, DATA_FRESHNESS_MS
                ));
            }
            // ① 既有 `meter_grid` 整组（S3b-1c 语义，原地不动——见本方法文档与 §11.5.3.4.1）
            if s.role == Role::MeterGrid {
                validate_meter_grid_regs(s)?;
            }
            // ② 通用站内规则（S3b-2 §11.5.1，含点展开）
            validate_station_regs(s)?;
        }
        // ②′ 块级采集周期覆盖（S3b-3 §12.7 规则 20–24；**须在②之后、③之前**——见本方法文档）
        validate_block_intervals(self)?;
        // ③ 跨站：单站约束计数（规则 2/3）
        let mut grid_seen = false;
        let mut battery_seen = false;
        for s in &self.stations {
            match s.role {
                Role::MeterGrid => {
                    if grid_seen {
                        return Err(
                            "south_stations: 至多一个 meter_grid 站（AiIntegrator 单写方约束）"
                                .into(),
                        );
                    }
                    grid_seen = true;
                }
                Role::Battery => {
                    if battery_seen {
                        return Err(
                            "south_stations: 至多一个 battery 站（BMS SOC 单源约束，AiIntegrator bms_soc 单槽）"
                                .into(),
                        );
                    }
                    battery_seen = true;
                }
                // `Role::Pcs` 分支随 P-3 一并删除：站级 pcs 站在 ① 即被拒（规则 2「至多一个
                // pcs 站」与规则 3「pcs regs 非空」随之不可达）。两条规则的新落点：前者由
                // P-3 单段化天然满足（`south_pcs` 是单值段，不可能配两台）；后者迁入
                // [`SouthPcsConfig::validate`] 的 `regs 为空` 分支。
                _ => {}
            }
        }
        // ③ 跨站：同口一致性（规则 16：baud_rate 与 parity——物理共享口参数被静默忽略的防线）
        let mut ports: Vec<(&str, &str, u32, StationParity)> = Vec::new();
        for s in &self.stations {
            if let Some((_, first_id, first_baud, first_parity)) =
                ports.iter().find(|(p, ..)| *p == s.port.as_str())
            {
                if *first_baud != s.baud_rate {
                    return Err(format!(
                        "south_stations: 站 {} port {} baud_rate={} 与同口首站 {}（baud_rate={}）不一致——同口共享物理波特率，须统一",
                        s.id, s.port, s.baud_rate, first_id, first_baud
                    ));
                }
                if *first_parity != s.parity {
                    return Err(format!(
                        "south_stations: 站 {} port {} parity={:?} 与同口首站 {}（parity={:?}）不一致——同口共享物理校验位，须统一（否则被静默忽略）",
                        s.id, s.port, s.parity, first_id, first_parity
                    ));
                }
            } else {
                ports.push((s.port.as_str(), s.id.as_str(), s.baud_rate, s.parity));
            }
        }
        // ③ 跨站：块落地极大性（规则 15）
        validate_maximality(&self.stations)?;
        Ok(())
    }

    /// role=grid 站（策略 phase 真源站；唯一 grid 形态——master_meter 段已删收敛，S3b-1c）。
    pub fn grid_station(&self) -> Option<&StationConf> {
        self.stations.iter().find(|s| s.role == Role::MeterGrid)
    }
}

/// 既有 `meter_grid` 站内完整性校验（S3b-1c；**整组原地不动**——见 [`SouthStationsConfig::validate`]）。
///
/// 相量块 `p/q/pf/u/i` 齐备 / `count ≥ 6` / `p_total count ≥ 2` / `int32_scaled` 块须显式
/// `scale > 0` / 块名唯一 / `addr > 0` / 半开区间不重叠。
fn validate_meter_grid_regs(s: &StationConf) -> Result<(), String> {
    // 相量块语义（mapper 按 name 找块，缺失/读失败 → Failed→offline 是运行期；
    // 此处配置期拦截缺失与地址错误）。空 regs 亦落入缺 p 分支被拒。
    for required in ["p", "q", "pf", "u", "i"] {
        if !s.regs.iter().any(|b| b.name == required) {
            return Err(format!(
                "south_stations: meter_grid 站 {} regs 缺相量块 {}（总表 phase 真源须 p/q/pf/u/i）",
                s.id, required
            ));
        }
    }
    // count 语义：相量块须 count>=6（3 相×2 寄存器三相连续，mapper decode_phase_block
    // 硬性 ≥6）；p_total（可选）count>=2。count 配错（漏配→default 2，或 4）时
    // name/addr/重叠都过、启动绿灯，但 scheduler 每轮只读 blk.count → 读回 <6 →
    // decode None → meter_grid 永久 offline（phase 断供），只在运行期暴露；配置期须拦截。
    for b in &s.regs {
        // int32_scaled 块须显式 scale>0：RegBlockConf.scale serde 默认 0.0，
        // 漏写会 raw×0 整块解 0（p/q/pf/i 全 0 静默喂策略，decode_regs 语义）。
        if b.format == RegFormat::Int32Scaled && b.scale == 0.0 {
            return Err(format!(
                "south_stations: meter_grid 站 {} 块 {} format=int32_scaled 须显式 scale>0（默认 0.0 会整块解 0）",
                s.id, b.name
            ));
        }
        let is_phase = ["p", "q", "pf", "u", "i"].contains(&b.name.as_str());
        if is_phase && b.count < 6 {
            return Err(format!(
                "south_stations: meter_grid 站 {} 相量块 {} count={} 须 ≥ 6（3 相×2 寄存器）",
                s.id, b.name, b.count
            ));
        }
        if b.name == "p_total" && b.count < 2 {
            return Err(format!(
                "south_stations: meter_grid 站 {} p_total 块 count={} 须 ≥ 2",
                s.id, b.count
            ));
        }
    }
    // 块名唯一：mapper 按 name 取首块（`.find`），异 addr 同名不重叠时后者静默死配置。
    let mut names: Vec<&str> = Vec::new();
    for b in &s.regs {
        if names.contains(&b.name.as_str()) {
            return Err(format!(
                "south_stations: meter_grid 站 {} regs 块名重复: {}（mapper 按 name 取首块，后者静默失效）",
                s.id, b.name
            ));
        }
        names.push(&b.name);
    }
    // addr>0 + 区间不重叠（含 p_total；width 取块 count，至少 1）
    let mut seen: Vec<(&str, u16, u32)> = Vec::new();
    for b in &s.regs {
        if b.addr == 0 {
            return Err(format!(
                "south_stations: meter_grid 站 {} regs 块 {} addr 不能为 0",
                s.id, b.name
            ));
        }
        let width = (b.count as u32).max(1);
        for (name, addr, w) in &seen {
            let ai = b.addr as u32;
            let aj = *addr as u32;
            if ai < aj + w && aj < ai + width {
                return Err(format!(
                    "south_stations: meter_grid 站 {} regs 块 {} 与 {} 寄存器区间重叠（{}@{:#x} 与 {}@{:#x}）",
                    s.id, b.name, name, b.name, b.addr, name, addr
                ));
            }
        }
        seen.push((&b.name, b.addr, width));
    }
    Ok(())
}

/// 站内通用校验（S3b-2 §11.5.1 规则 4/5/7/8/9/10/11/12/13/14/19）。
///
/// 落点说明（逐条对齐 §11.5.1 的"落点函数"列）：
/// - 规则 7/8/9（越界 / 重叠 / 32 位对齐）由 [`points::expand`] 在展开时报错（校验期与
///   运行期**同一函数**，防"校验通过但运行期展开不同"）；
/// - 规则 11（空洞上限 + 窗口首尾锚定）由 [`validate_anchoring`] 基于 `expand` 结果判；
/// - 规则 6（符号性一致性）由 [`check_symbolicity`] 逐展开点对 `point_table` 登记值判。
fn validate_station_regs(s: &StationConf) -> Result<(), String> {
    let prefix = format!("south_stations: 站 {} ", s.id);
    // 规则 12（`count` 有效性 + `discrete` 位块上限）
    for b in &s.regs {
        validate_count_and_discrete_bits(&prefix, b)?;
        // 规则 13（地址有效性：addr == 0 仅 meter_batt / hvac 合法；meter_grid 的
        // addr>0 已由①的既有整组校验先判，行为不变）
        if b.addr == 0 && !matches!(s.role, Role::MeterBatt | Role::Hvac) {
            return Err(format!(
                "{}regs 块 {} addr 不能为 0（仅 meter_batt/hvac 首址可为 0）",
                prefix, b.name
            ));
        }
    }
    // 规则 5（格式与标度）+ 规则 19（无 points 块的宽度护栏）
    for b in &s.regs {
        validate_scale(&prefix, b)?;
        validate_width_multiple(&prefix, b)?;
    }
    // 规则 14（区间与重叠，按 func 空间分别判）
    validate_block_spans(&prefix, &s.regs)?;
    // 展开（规则 7/8/9）→ 规则 11 → 规则 6 → 汇聚点名（规则 4/10）
    let mut metrics: Vec<String> = Vec::new();
    for b in &s.regs {
        let pts = points::expand(b).map_err(|e| format!("{prefix}{e}"))?;
        validate_anchoring(&prefix, b, &pts)?;
        for p in &pts {
            if let PointKind::Scalar { offset, decode } = p.kind {
                check_symbolicity(
                    s.role,
                    &p.metric,
                    b.addr + offset,
                    decode.format,
                    decode.offset,
                )
                .map_err(|e| format!("{prefix}{e}"))?;
            }
        }
        metrics.extend(pts.into_iter().map(|p| p.metric));
    }
    // 规则 4（`soc` 点契约）：消费方按**点名**查找，缺名即静默不推 SOC（对齐 meter_grid
    // 缺相量块的既有拦截力度）
    if s.role == Role::Battery && !metrics.iter().any(|m| m == "soc") {
        return Err(format!(
            "{}battery 站无任何点位名为 `soc`（PRD §9.4.3 规则 4 `soc` 点契约）——SOC 控制链路会静默断供（消费方按点名查找）",
            prefix
        ));
    }
    // 规则 10（点名唯一：含自动位置点名与显式 `name` 相撞；meter_grid 的同名**块**由①先拒，文案不同）
    let mut seen: Vec<&str> = Vec::new();
    for m in &metrics {
        if seen.contains(&m.as_str()) {
            return Err(format!(
                "{}regs 点名重复: {}（遥测键冲突，指标相互覆盖）",
                prefix, m
            ));
        }
        seen.push(m.as_str());
    }
    Ok(())
}

/// 规则 5（格式与标度）：`int32_scaled`/`uint16`/`int16` 的**块级或点级** `scale == 0` → 拒。
///
/// 点级 `scale` 为 `None` 时按"继承块级"判**一次**（不重复报同一点）；`float32` 忽略
/// `scale`（既有语义）；`discrete` 块不适用（位值恒 0/1，声明换算即无意义配置）。
fn validate_scale(prefix: &str, b: &RegBlockConf) -> Result<(), String> {
    if b.func == RegFunc::Discrete {
        return Ok(());
    }
    if is_integer_format(b.format) && b.scale == 0.0 {
        return Err(format!(
            "{}regs 块 {} format={:?} 须显式 scale>0（serde 默认 0.0 会 raw×0 整块解 0）",
            prefix, b.name, b.format
        ));
    }
    for p in &b.points {
        let format = p.format.unwrap_or(b.format);
        let scale = p.scale.unwrap_or(b.scale);
        if is_integer_format(format) && scale == 0.0 {
            return Err(format!(
                "{}regs 块 {} 点 at={} format={:?} 须显式 scale>0（点级未声明时继承块级 scale={}）",
                prefix, b.name, p.at, format, b.scale
            ));
        }
    }
    Ok(())
}

/// 整数类格式（须显式非零 `scale`）：`int32_scaled` / `uint16` / `int16`。
fn is_integer_format(f: RegFormat) -> bool {
    matches!(
        f,
        RegFormat::Int32Scaled | RegFormat::Uint16 | RegFormat::Int16
    )
}

/// 规则 12（`count` 有效性 + `discrete` 位块上限）。
///
/// **为何抽成独立函数（2026-09-26，Task 6 返工的 P-4）**：原为 [`validate_station_regs`] 的
/// 内联检查；`south_pcs` 段（[`SouthPcsConfig::validate`]）须复用**同一判定**（规则 P-4
/// "不另起一套"）。本条与 `role` 无关，故只取 `prefix`（站名占位）即可两处共用 ——
/// 复制粘贴会立刻产生两份口径。
fn validate_count_and_discrete_bits(prefix: &str, b: &RegBlockConf) -> Result<(), String> {
    if b.count == 0 {
        return Err(format!(
            "{}regs 块 {} count 须 > 0（显式取值须为正；serde 层不拦截 0）",
            prefix, b.name
        ));
    }
    if b.func == RegFunc::Discrete && b.count > points::MAX_DISCRETE_BITS {
        return Err(format!(
            "{}discrete 块 {} count={} 超位块上限 {}（PRD §9.4.3 规则 12）",
            prefix,
            b.name,
            b.count,
            points::MAX_DISCRETE_BITS
        ));
    }
    Ok(())
}

/// 规则 19（无 `points` 块的宽度护栏）：`count` 须为 `format` 宽度的整数倍 —— 否则步进的
/// 尾槽装不下一个完整值，运行期**静默少产点**。`discrete` 位块不适用（位宽恒 1）；
/// 声明了 `points` 的块由规则 11 的锚定判（不重复判）。
///
/// **为何抽成独立函数**：同 [`validate_count_and_discrete_bits`] —— `south_pcs` 段复用同一判定。
fn validate_width_multiple(prefix: &str, b: &RegBlockConf) -> Result<(), String> {
    if b.func != RegFunc::Discrete && b.points.is_empty() {
        let width = b.format.reg_width() as u16;
        if b.count % width != 0 {
            return Err(format!(
                "{}regs 块 {} count={} 非 format={:?} 宽度 {} 的整数倍——步进的尾槽装不下一个完整值，会**静默少产点**（设计补落点规则 19）",
                prefix, b.name, b.count, b.format, width
            ));
        }
    }
    Ok(())
}

/// 规则 11（空洞上限，仅作用于**声明了 `points` 的标量块**）。
///
/// 两条锚定 + 一条空洞：① 首个声明寄存器须落在块内偏移 **0**（不得含前导未声明寄存器）；
/// ② 最后一个声明寄存器的**末端须恰好等于 `count`**（末尾未声明寄存器不计入 count）；
/// ③ 块内未声明寄存器的**连续空洞 ≤ 4**（PRD §9.4.2.1 第 3 条：补读量不得超过一次请求帧）。
/// `discrete` 位块不受本条限制（位块整窗口产出）。
fn validate_anchoring(prefix: &str, b: &RegBlockConf, pts: &[PointSpec]) -> Result<(), String> {
    if b.func == RegFunc::Discrete || b.points.is_empty() {
        return Ok(());
    }
    let mut spans: Vec<(u16, u16)> = pts
        .iter()
        .map(|p| (p.kind.offset(), p.kind.width()))
        .collect();
    spans.sort_unstable();
    let first = spans[0].0;
    let last_end = spans.iter().map(|(o, w)| *o + *w).max().unwrap_or(0);
    if first != 0 {
        return Err(format!(
            "{}regs 块 {} 声明点窗口首尾锚定：首个声明寄存器落在块内偏移 {}（须为 0，不得含前导未声明寄存器）",
            prefix, b.name, first
        ));
    }
    if last_end != b.count {
        return Err(format!(
            "{}regs 块 {} 声明点窗口首尾锚定：末个声明寄存器末端={} 与 count={} 不等（末尾未声明寄存器不计入 count）",
            prefix, b.name, last_end, b.count
        ));
    }
    let mut cursor: u16 = 0;
    for (off, w) in &spans {
        if *off > cursor {
            let hole = *off - cursor;
            if hole > points::MAX_HOLE_REGS {
                return Err(format!(
                    "{}regs 块 {} 块内未声明寄存器连续空洞 {} > {}（块内偏移 {}..{} 未声明；须在该空洞处拆块）",
                    prefix,
                    b.name,
                    hole,
                    points::MAX_HOLE_REGS,
                    cursor,
                    off
                ));
            }
        }
        cursor = cursor.max(off + w);
    }
    Ok(())
}

/// 规则 14（区间与重叠）：同站**按功能码空间**（holding / input / discrete 三套地址空间）
/// 分别判半开区间重叠。PCS 的 3 区/4 区同址不同 func 由此天然放行。
fn validate_block_spans(prefix: &str, regs: &[RegBlockConf]) -> Result<(), String> {
    for (i, a) in regs.iter().enumerate() {
        for b in regs.iter().skip(i + 1) {
            if a.func != b.func {
                continue;
            }
            let (ai, aw) = (a.addr as u32, a.count as u32);
            let (bi, bw) = (b.addr as u32, b.count as u32);
            if ai < bi + bw && bi < ai + aw {
                return Err(format!(
                    "{}regs 块 {} 与 {} 寄存器区间重叠（{:?} 空间 {}@{:#x}+{} 与 {}@{:#x}+{}）",
                    prefix,
                    a.name,
                    b.name,
                    a.func,
                    a.name,
                    a.addr,
                    a.count,
                    b.name,
                    b.addr,
                    b.count
                ));
            }
        }
    }
    Ok(())
}

/// 规则 6（符号性一致性）：查点表登记值判"可追溯"与"不漂移"。
///
/// **查不到行 → 放行**（§11.4.4 P0-2 裁定）：现场 RC-3 会合法改 `addr` 基准，
/// 此时按基准登记的键自然全部失配；若"无行即拒"会把**合法的现场校准配置拒在启动期**。
fn check_symbolicity(
    role: Role,
    metric: &str,
    addr: u16,
    format: RegFormat,
    offset: f64,
) -> Result<(), String> {
    check_symbolicity_row(
        metric,
        addr,
        format,
        offset,
        crate::point_table::lookup(role, addr),
    )
}

/// 规则 6 的判定核心（纯函数，供单测直接注入登记行）：
/// ① `offset ≠ 0` 而登记行未登记符号性来源（`sym_src` 空）→ 拒；
/// ② `offset` 与登记值不等（含"漏配 → 缺省 0 ≠ −40"）→ 拒；无行 → 放行。
/// 仅对 `format ∈ {uint16, int16}` 生效（PRD §9.4.3；`float32`/`int32_scaled` 不参与）。
fn check_symbolicity_row(
    metric: &str,
    addr: u16,
    format: RegFormat,
    offset: f64,
    row: Option<&crate::point_table::PointReg>,
) -> Result<(), String> {
    if !matches!(format, RegFormat::Uint16 | RegFormat::Int16) {
        return Ok(());
    }
    let Some(row) = row else {
        return Ok(());
    };
    if (row.offset - offset).abs() > f64::EPSILON {
        return Err(format!(
            "点 {} (addr={:#06x}, format={:?}) offset={} 与点表登记值 {} 不一致——配置与 §9.5 点表漂移",
            metric, addr, format, offset, row.offset
        ));
    }
    if offset != 0.0 && row.sym_src.is_none() {
        return Err(format!(
            "点 {} (addr={:#06x}, format={:?}) 携带 offset={} 但点表未登记符号性来源（raw 的解释方式无人可考）",
            metric, addr, format, offset
        ));
    }
    Ok(())
}

/// 规则 15（块落地极大性，PRD §9.4.3 v1.8 订正后的适用域与判据）。
///
/// **适用域（先行过滤）**：仅当被考察的两个块**都声明了 `points`** 且**均非 `discrete`**
/// 时才参与判定；任一块未声明 `points` → 跳过不判（既有 `meter_grid` 六相量块、
/// `discrete` 位块均属此列——否则会拒掉既有 `grid_meter`，现场启动 fail-fast）。
///
/// **判据（同时满足才拒）**：① `func`/`byte_swap` 相同；② 地址严格相邻
///（`b.addr == a.addr + a.count`）；③ 合并窗口内连续空洞 ≤ 4；④ 合并后
/// `count ≤ MAX_SINGLE_READ_REGS(120)`；且**两块均未标 `read_slice: true`** → `Err`。
///
/// **判定顺序（§11.5.2(1) 建议）**：先判 ④（超上限 ⇒ 本就该分片 ⇒ **提前放行**），
/// 再判 ③，最后 ①② 与 `read_slice` 豁免。三条判据对"拒绝"是**合取**关系，故顺序不影响
/// 判定结果；按建议顺序实现是为了让"本就该分片"的形态（如 `fire` 的 13+114=127）一眼可见地
/// 走放行分支，不被后续条件误拒。
///
/// **与块书写顺序无关（S1 订正）**：相邻对按**地址序**（`addr` 升序）枚举，不按 `regs` 列表的
/// 书写顺序 —— 逆序书写的相邻块同样会被判"应合并"（回归钉子见
/// `tests/s3b2_config.rs::ac1_rule15_maximality_independent_of_written_order`）。
fn validate_maximality(stations: &[StationConf]) -> Result<(), String> {
    for s in stations {
        // **枚举顺序：按地址序，不得按书写序**（S1）。判据 ② 说的是"**地址**严格相邻"，而
        // YAML 里块的**书写顺序**是自由的（现场/工具生成的配置可能逆序书写）；若直接按列表
        // 顺序两两配对，逆序书写的相邻块会让 `b.addr == a.addr + a.count` 永不成立 ⇒ 该判
        // 形态被放行、极大性形同虚设（首轮评审实测）。按 `addr` 升序预处理后，配对结果与
        // 书写顺序无关。
        //
        // 同 `func` 空间内块两两不重叠（规则 14 已先拒），故"满足 ② 的对"在升序下必然
        // **低地址块在前**；`merged_max_hole` 的"(a = 低地址, b = 紧邻其后的高地址)"假设
        // 亦由本次排序保证（否则空洞计算会错位）。
        let mut blocks: Vec<&RegBlockConf> = s.regs.iter().collect();
        blocks.sort_by_key(|b| b.addr);
        for (i, a) in blocks.iter().enumerate() {
            for b in blocks.iter().skip(i + 1) {
                // 适用域过滤
                if a.points.is_empty() || b.points.is_empty() {
                    continue;
                }
                if a.func == RegFunc::Discrete || b.func == RegFunc::Discrete {
                    continue; // 保守读法（§11.12.2 Δ-8）：discrete 块一律不参与
                }
                // ① func / byte_swap 相同（func 同空间 + 显式同 swap）
                if a.func != b.func || a.byte_swap != b.byte_swap {
                    continue;
                }
                // ② 地址严格相邻
                if b.addr as u32 != a.addr as u32 + a.count as u32 {
                    continue;
                }
                let merged: u32 = a.count as u32 + b.count as u32;
                // ④ 合并后超设备单次读上限 ⇒ 本就该分片 ⇒ 放行
                if merged > points::MAX_SINGLE_READ_REGS as u32 {
                    continue;
                }
                // ③ 合并窗口内连续空洞 ≤ 4
                if merged_max_hole(a, b)? > points::MAX_HOLE_REGS {
                    continue;
                }
                // 豁免：任一块显式 read_slice（现场按实测上限分片，PRD §9.4.2.4）
                if a.read_slice || b.read_slice {
                    continue;
                }
                return Err(format!(
                    "south_stations: 站 {} regs 块 {}@{:#x}(count={}) 与 {}@{:#x}(count={}) 同 func/同 byte_swap、地址连续、合并后 {} ≤ {} 且未标 read_slice ⇒ 应合并为一块（PRD §9.4.3 规则 15 块落地极大性）",
                    s.id, a.name, a.addr, a.count, b.name, b.addr, b.count, merged,
                    points::MAX_SINGLE_READ_REGS
                ));
            }
        }
    }
    Ok(())
}

/// 合并窗口（`[a.addr, b.addr + b.count)`）内未声明寄存器的**最大连续空洞**（寄存器数）。
fn merged_max_hole(a: &RegBlockConf, b: &RegBlockConf) -> Result<u16, String> {
    let len = a.count as usize + b.count as usize;
    let mut covered = vec![false; len];
    for (blk, base) in [(a, 0usize), (b, a.count as usize)] {
        for (off, w) in points::footprint(blk)? {
            for k in 0..w as usize {
                let idx = base + off as usize + k;
                if idx < len {
                    covered[idx] = true;
                }
            }
        }
    }
    let (mut max_hole, mut cur) = (0u16, 0u16);
    for c in covered {
        if c {
            cur = 0;
        } else {
            cur += 1;
            max_hole = max_hole.max(cur);
        }
    }
    Ok(max_hole)
}

// ══════════════════════════ S3b-3：块级采集周期覆盖（T8）══════════════════════════

/// 口占用上界（PRD §10.3.2 **C9** / §10.5 第 2 行）：`U_口 ≤ 0.5`（留 ≥2× 余量吸收抖动/重试）。
const MAX_PORT_UTILIZATION: f64 = 0.5;

/// 组周期相对下界系数（PRD §10.3.2 **C4** / §10.5 第 1 行：`组周期 ≥ 1.5 × T_组`）。
const GROUP_INTERVAL_HEADROOM: f64 = 1.5;

/// 规则 23/24 的**口内组视图**：`(所属站, 该站的一个读组, 该组的整组耗时 T_组(ms))`。
/// `T_组` 按设计 §12.6 公式用**该站** `baud_rate` 复算（同口 `baud_rate` 已由既有规则 16 强制一致）。
type PortGroup<'a> = (&'a StationConf, ReadGroup, f64);

/// `tx_time_ms` 的帧结构常量（设计 §12.6 公式 / PRD §9.8.1 带宽算式）：
/// **请求帧 `8` 字节**（从站地址 1 + 功能码 1 + 起始地址 2 + 数量 2 + CRC 2）。
const FRAME_REQ_BYTES: u32 = 8;

/// 响应帧的**固定开销** `5` 字节（从站地址 1 + 功能码 1 + 字节数 1 + CRC 2），
/// **不含**数据字节 `D`（`D` 由功能码与数量算出）。
const RESP_FIXED_BYTES: u32 = 5;

/// 每事务的**从站周转耗时** `4 ms`（设计 §12.6 公式的加项：从站处理 + 总线换向）。
const TURNAROUND_MS: f64 = 4.0;

/// UART **每字节的位开销** `10 bit/字节`（1 起始 + 8 数据 + 1 停止；设计 §12.6 / PRD §9.8.1
/// 的 `10 / baud_rate` 秒/字节，9600 bps ⇒ 1.04 ms）。
/// **勿与** `div_ceil(8)`（位块的"8 位/字节"打包口径）混同：二者是不同用途的 8 与 10。
const BITS_PER_BYTE: f64 = 10.0;

/// `T_组` 估值用的**1 字节耗时**（ms；设计 §12.6 公式的第一项）。
///
/// §12.6 写死「1 字节 = `10 / baud_rate` 秒（9600 bps ⇒ **1.04 ms**）」，且设计表内**全部**
/// 数字（`21.68` / `25.84` / `267.12` / `801.36` / `1202.04`）都按该取值复算 ⇒ 本函数按
/// **2 位小数**取该系数（规则 20/23/24 的**唯一求值点**）。
///
/// **为什么必须取 2 位小数**：若用未取整的 `1.0416667`，AC-8-5 的 C4 一例会算成
/// `1.5 × T_组 = 1203.94`，与 PRD **AC-8-5**「文案含 `1202`」的机械判据不符。
fn byte_time_ms(baud_rate: u32) -> f64 {
    // `baud_rate == 0` 已被 `validate` 的①拒（此处 `.max(1)` 仅为"绝不出 `inf`"的防御）
    let ms = BITS_PER_BYTE / f64::from(baud_rate.max(1)) * 1000.0;
    (ms * 100.0).round() / 100.0
}

/// **单事务耗时**（ms；设计 §12.6）：`T = 帧字节 × 10/baud_rate × 1000 + 4`，
/// 其中帧字节 = 请求 [`FRAME_REQ_BYTES`] + 响应 [`RESP_FIXED_BYTES`] + `D`，
/// `D`（数据字节）= **FC02 `ceil(位数 / 8)`**、**FC03/FC04 `2 × 寄存器数`**；
/// 每事务另加 [`TURNAROUND_MS`] 从站周转，字节耗时用 [`BITS_PER_BYTE`]。
fn tx_time_ms(baud_rate: u32, func: RegFunc, count: u16) -> f64 {
    let data_bytes = match func {
        // 位块：位 → 字节按 **8 位/字节**向上取整（与 `BITS_PER_BYTE` 的 UART 10 bit/字节无关）
        RegFunc::Discrete => u32::from(count).div_ceil(8),
        RegFunc::Holding | RegFunc::Input => 2 * u32::from(count),
    };
    f64::from(FRAME_REQ_BYTES + RESP_FIXED_BYTES + data_bytes) * byte_time_ms(baud_rate)
        + TURNAROUND_MS
}

/// 某读组的**整组耗时 `T_组`**（ms）—— 规则 20 与规则 23/24 的**同一口径**
/// （设计 §12.7「`T_组` 定义写死」注 + §12.10.3 **R-13**）：**逐事务按 §12.6 公式累加**，
/// **不是单块耗时**（否则多块组会被低估而漏拒）。
fn group_tx_time_ms(s: &StationConf, g: &ReadGroup) -> f64 {
    g.blk_indices
        .iter()
        .filter_map(|&i| s.regs.get(i))
        .map(|b| tx_time_ms(s.baud_rate, b.func, b.count))
        .sum()
}

/// 块下标 → 其所属读组（`read_groups_of` 的"块 → 组 1:1"不变量保证唯一命中；
/// 不命中返回 `None`，调用方按"无组"处理，**不 panic**）。
fn group_of_block(groups: &[ReadGroup], blk: usize) -> Option<&ReadGroup> {
    groups.iter().find(|g| g.blk_indices.contains(&blk))
}

/// 组内块名清单（错误文案用；空块集 = `regs` 为空的退化组 ⇒ 标 `<空块集>`）。
fn group_block_names(s: &StationConf, g: &ReadGroup) -> String {
    if g.blk_indices.is_empty() {
        return "<空块集>".into();
    }
    g.blk_indices
        .iter()
        .filter_map(|&i| s.regs.get(i))
        .map(|b| b.name.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// 本口全部组的明细（段文案，规则 23/24 共用）。
fn groups_detail(groups: &[PortGroup]) -> String {
    groups
        .iter()
        .map(|(st, g, t)| {
            format!(
                "站 {} 块[{}] T_组={:.2}ms 周期={}ms",
                st.id,
                group_block_names(st, g),
                t,
                g.interval_ms
            )
        })
        .collect::<Vec<_>>()
        .join("；")
}

/// 站级语义判据所需块集合 **`R(role)` 的块下标集**（PRD §10.3.2 的 `R(role)` 定义表）
/// —— 本 crate 的**唯一定义载体（配置侧）**。
///
/// **T9 的 `judges_evaluable`（02 设计 §12.4.5）是同一定义的运行期对偶**（在 `BlockReads`
/// 上求值），**不得另立第二套定义**。
///
/// 逐 role（PRD §10.3.2 原文口径）：
/// - `meter_grid` = 块名 ∈ `{p, q, pf, u, i, p_total}`（`mapper::poll_to_result` 的 MeterGrid
///   分支按**块名**取数，缺任一 → `Failed`；`p_total` 供 `scalar_total` 降级求和）；
/// - `battery` = **承载点名 `soc` 的那一块**（`mapper::battery_soc` 按**点名**查找）；
/// - `fire` = **覆盖寄存器 11 的寄存器块**（`func != discrete`，与 `mapper::fire_chain_head`
///   的 `res.regs()?` 同判）∪ **块名前缀 `fire_det` 的块**（两类判据共用这两类输入）；
/// - `meter_batt` / `hvac` / `pcs` = **∅**（`mapper::poll_to_result` 返回空包，无站级判据）。
pub(crate) fn criterion_block_indices(s: &StationConf) -> Vec<usize> {
    match s.role {
        Role::MeterGrid => s
            .regs
            .iter()
            .enumerate()
            .filter(|(_, b)| matches!(b.name.as_str(), "p" | "q" | "pf" | "u" | "i" | "p_total"))
            .map(|(i, _)| i)
            .collect(),
        Role::Battery => s
            .regs
            .iter()
            .enumerate()
            .filter(|(_, b)| block_carries_point(b, "soc"))
            .map(|(i, _)| i)
            .collect(),
        Role::Fire => s
            .regs
            .iter()
            .enumerate()
            .filter(|(_, b)| is_fire_criterion_block(b))
            .map(|(i, _)| i)
            .collect(),
        Role::MeterBatt | Role::Hvac | Role::Pcs => Vec::new(),
    }
}

/// 块是否承载点名 `metric` 的**标量点**——与 `mapper::soc_point` 的判据（`points::expand` +
/// `PointKind::Scalar` + 点名相等）**同源**；展开失败（坏块）按"不承载"处理。
fn block_carries_point(b: &RegBlockConf, metric: &str) -> bool {
    match points::expand(b) {
        Ok(pts) => pts
            .iter()
            .any(|p| p.metric == metric && matches!(p.kind, PointKind::Scalar { .. })),
        Err(_) => false,
    }
}

/// `fire` 的判据块（PRD §10.3.2）：覆盖**探测器 1 的地址寄存器**（[`FIRE_DET1_ADDR_REG`]，
/// = 寄存器 11）的**寄存器块**（`func != discrete`）∪ 块名前缀 `fire_det` 的块。
///
/// **不得**改写成"块名 == `fire_sys`"（那属设备特判，违反 G-5）；寄存器块的限定与
/// `mapper::fire_chain_head` 的 `res.regs()?`（位块取不到寄存器 ⇒ 不能当链首）**同判**，
/// 且**复用同一常量**（模块头 `use crate::mapper::FIRE_DET1_ADDR_REG` ⇒ 单一真源）。
fn is_fire_criterion_block(b: &RegBlockConf) -> bool {
    if b.name.starts_with("fire_det") {
        return true;
    }
    let det1 = FIRE_DET1_ADDR_REG;
    b.func != RegFunc::Discrete
        && b.addr <= det1
        && u32::from(b.addr) + u32::from(b.count) > u32::from(det1)
}

/// **块级采集周期覆盖的配置期校验**（S3b-3；PRD §10.3.2 的 C1–C9 与 §10.5 第 3 行；设计 §12.7）。
///
/// **落点**：`SouthStationsConfig::validate` 的 **②′** —— 在 ②[`validate_station_regs`] 之后、
/// ③ 跨站之前（见该方法文档的落点图）。**② 的文案更具体，故本函数不得前移**。
///
/// **判据顺序（设计 §12.7 的注，S-2 之一的前提）**：按**规则号升序**逐条求值，
/// **首个失败即 `Err` 返回**；同一规则内部按 `iv == 0` → `iv < MIN_POLL_INTERVAL_MS`
/// → `iv < 1.5 × T_组` 的顺序。该顺序使 PRD AC-8-5 的"文案含 `1202`"能**机械证明**
/// 触发者是**规则 20 的 C4 分支**（规则 23 的文案只写 `U` 值与触发的组，**不写** `1.5 × T_组`）。
///
/// 规则 ↔ 约束：**20** = C1+C2+C4；**21** = C3+C5；**22** = C6+C7；**23** = C9（口预算）；
/// **24** = PRD §10.5 第 3 行（单轮最坏耗时，PRD 侧无 C 编号）。
/// **规则 20/21/22 只判"显式声明了 `interval_ms` 的块"**（`None` ⇒ 继承站周期，其下界/上界
/// 已由站级既有规则覆盖）；**规则 23/24 对全部组求值**（未声明的块也参与口预算）。
///
/// `T_组` 一律 = **该块所在读组**的整组耗时（[`group_tx_time_ms`]），组由 [`read_groups_of`]
/// （**唯一分组实现**，配置期与调度期共用）给出 ⇒ 同组内各块 `eff` 相同、定义**无循环**。
pub fn validate_block_intervals(cfg: &SouthStationsConfig) -> Result<(), String> {
    // ── 规则 20（C1 + C2 + C4）：逐块，仅显式声明者；`iv == 0` → `< 500` → `< 1.5×T_组` ──
    for s in &cfg.stations {
        let groups = read_groups_of(s);
        for (bi, b) in s.regs.iter().enumerate() {
            let Some(iv) = b.interval_ms else { continue };
            if iv == 0 {
                return Err(format!(
                    "south_stations: 站 {} 块 {} interval_ms=0 须 > 0（PRD §10.3.2 C1 正数）",
                    s.id, b.name
                ));
            }
            if iv < MIN_POLL_INTERVAL_MS {
                return Err(format!(
                    "south_stations: 站 {} 块 {} interval_ms={} 须 ≥ {}ms（PRD §10.3.2 C2 绝对下界，与 pcs 站周期下界同源）",
                    s.id, b.name, iv, MIN_POLL_INTERVAL_MS
                ));
            }
            // C4：本块**所在读组**的整组耗时（单块组时退化为 T_块）
            let grp = group_of_block(&groups, bi);
            // 「块 → 组 1:1」不变量（`read_groups_of`）保证唯一命中；不命中即不变量被破坏
            // ⇒ **debug 构建下即刻可见**（`debug_assert!` 在 release 下不生效 ⇒ 不命中时
            // 行为与改动前**逐字一致**：仍按"无组"取 `t = 0.0` 继续，**不 panic**）。
            debug_assert!(
                grp.is_some(),
                "站 {} 的块下标 {}（块 {}）未命中任何读组：read_groups_of 的「块 → 组 1:1」不变量被破坏",
                s.id,
                bi,
                b.name
            );
            let t = grp.map_or(0.0, |g| group_tx_time_ms(s, g));
            let lower = GROUP_INTERVAL_HEADROOM * t;
            if (iv as f64) < lower {
                return Err(format!(
                    "south_stations: 站 {} 块 {} interval_ms={} 须 ≥ 1.5 × T_组 = {:.2}ms（PRD §10.3.2 C4 相对下界；本块所在读组 [{}] 的单轮耗时 T_组={:.2}ms）",
                    s.id,
                    b.name,
                    iv,
                    lower,
                    grp.map_or_else(String::new, |g| group_block_names(s, g)),
                    t
                ));
            }
        }
    }

    // ── 规则 21（C3 + C5）：仍只判显式声明者；C3（`% poll_ms`）→ C5（`≤ 站周期`） ──
    // `poll_ms == 0` 时取 1（与 `scheduler::spawn` 的 `.max(1)` 同款防御；`% 0` 会 panic）
    let poll_ms = cfg.poll_ms.max(1);
    for s in &cfg.stations {
        for b in &s.regs {
            let Some(iv) = b.interval_ms else { continue };
            if iv % poll_ms != 0 {
                return Err(format!(
                    // 文案打印**实际用于判定的那个值** `poll_ms`（= `cfg.poll_ms.max(1)`），
                    // 与判据同源（此前打印原始 `cfg.poll_ms` ⇒ `poll_ms == 0` 时文案与判据不一致）
                    "south_stations: 站 {} 块 {} interval_ms={} 须为 poll_ms={} 的整数倍（PRD §10.3.2 C3 tick 网格对齐；非整数倍会被网格静默量化、配置名不副实）",
                    s.id, b.name, iv, poll_ms
                ));
            }
            if iv > s.interval_ms {
                return Err(format!(
                    "south_stations: 站 {} 块 {} interval_ms={} 须 ≤ 站周期 {}ms（PRD §10.3.2 C5 只提速不降速；降速诉求应由站级 interval_ms 表达）",
                    s.id, b.name, iv, s.interval_ms
                ));
            }
        }
    }

    // ── 规则 22（C6 + C7）：`R(role)` 的判据块；C6（eff 须全相同）→ C7（eff 须 ≥ 站周期） ──
    for s in &cfg.stations {
        let mut effs: Vec<(&RegBlockConf, u64)> = Vec::new();
        for i in criterion_block_indices(s) {
            if let Some(b) = s.regs.get(i) {
                effs.push((b, b.effective_interval_ms(s.interval_ms)));
            }
        }
        // `R(role) = ∅`（meter_batt / hvac / pcs，或判据块尚缺）⇒ 无站级判据，本条不适用
        let Some(&(b0, e0)) = effs.first() else {
            continue;
        };
        if let Some(&(b1, e1)) = effs.iter().find(|&&(_, e)| e != e0) {
            return Err(format!(
                "south_stations: 站 {} role={:?} 的判据块（R(role)）有效周期不一致：{} 的 eff={}ms 与 {} 的 eff={}ms（PRD §10.3.2 C6 判据完整性——站级判据按一次读集求值，拆到不同组会缺块误判）",
                s.id, s.role, b0.name, e0, b1.name, e1
            ));
        }
        if let Some(&(b, e)) = effs.iter().find(|&&(_, e)| e < s.interval_ms) {
            return Err(format!(
                "south_stations: 站 {} role={:?} 的判据块 {} 有效周期 eff={}ms 须 ≥ 站周期 {}ms（PRD §10.3.2 C7 判据块不得提速——会静默改变控制链 cadence）",
                s.id, s.role, b.name, e, s.interval_ms
            ));
        }
    }

    // ── 规则 23 + 24：按 **port 聚合**（同口全部站的组并起来算；口按首次出现序，同规则 16）──
    let mut port_groups: Vec<(&str, Vec<PortGroup>)> = Vec::new();
    for s in &cfg.stations {
        if port_groups.iter().any(|(p, _)| *p == s.port.as_str()) {
            continue;
        }
        let mut groups: Vec<PortGroup> = Vec::new();
        for st in cfg.stations.iter().filter(|x| x.port == s.port) {
            for g in read_groups_of(st) {
                let t = group_tx_time_ms(st, &g);
                groups.push((st, g, t));
            }
        }
        port_groups.push((s.port.as_str(), groups));
    }

    // ── 规则 23（C9）：`U_口 = Σ_组 (T_组 / 组周期) > 0.5` ⇒ `Err`
    //    文案含 port + U 值 + 触发的组（站 id / 块名 / T / 周期），**不写** `1.5 × T_组`
    //    （该值只由规则 20 的 C4 分支写 ⇒ "文案含 `1202`" 可机械区分触发者） ──
    for (port, groups) in &port_groups {
        // 组周期恒 > 0（显式声明为 0 的块已被规则 20 先拒；站级 `interval_ms > 0` 由①保证）
        let u: f64 = groups
            .iter()
            .map(|(_, g, t)| t / g.interval_ms as f64)
            .sum();
        if u > MAX_PORT_UTILIZATION {
            return Err(format!(
                "south_stations: port {} 口预算 U_口 = Σ_组(T_组 / 组周期) = {:.4} > {}（PRD §10.3.2 C9 上界；本口各组：{}）",
                port,
                u,
                MAX_PORT_UTILIZATION,
                groups_detail(groups)
            ));
        }
    }

    // ── 规则 24（PRD §10.5 第 3 行，**PRD 侧无 C 编号**）：单轮最坏耗时
    //    `Σ T_组 > 1.5 × 最小非零组周期` ⇒ `Err`（同口全部到期组串行；各站用**本站** baud_rate）
    //    **正当性**：C9 的违规域（`iv < 2×T_组`）真包含 C4 的（`iv < 1.5×T_组`）⇒ C9 **不能**蕴含本条
    //    （设计 §12.7 的 W-1 构造：`U = 0.447 ≤ 0.5` 但 `Σ T = 2156.6 > 1500`） ──
    for (port, groups) in &port_groups {
        let sum_t: f64 = groups.iter().map(|(_, _, t)| t).sum();
        let mut min_iv: Option<u64> = None;
        for (_, g, _) in groups {
            min_iv = Some(min_iv.map_or(g.interval_ms, |m: u64| m.min(g.interval_ms)));
        }
        if let Some(mi) = min_iv {
            let threshold = GROUP_INTERVAL_HEADROOM * mi as f64;
            if sum_t > threshold {
                let source = groups
                    .iter()
                    .find(|(_, g, _)| g.interval_ms == mi)
                    .map_or_else(String::new, |(st, g, _)| {
                        format!("站 {} 块[{}]", st.id, group_block_names(st, g))
                    });
                return Err(format!(
                    "south_stations: port {} 单轮最坏耗时 Σ T_组 = {:.2}ms > 1.5 × 最小非零组周期 = {:.2}ms（最小非零组周期 {}ms 来自 {}；本口各组：{}）",
                    port,
                    sum_t,
                    threshold,
                    mi,
                    source,
                    groups_detail(groups)
                ));
            }
        }
    }

    // ── C8 的**配置异味**（PRD §10.6 第 8 条；设计 §12.7 末段）**不构成本函数的拒配置分支**
    //    ⇒ 判定由纯函数 [`block_interval_hints`] 给出（本函数只做 C1–C9 的 `Err` 判定），
    //    **发射在 startup 装配期**（理由见该函数与 `core_config.rs:511-512` 的同款说明）──
    Ok(())
}

/// **C8 的配置异味判定 —— 纯函数，不发射日志、不改判定、不拒配置**（PRD §10.6 第 8 条；
/// 设计 §12.7 末段）：站 `regs` **非空且每块都显式声明了 `interval_ms`** ⇒ 该站站级
/// `interval_ms` 已**不再描述该站的实际采集节奏**（C8 本身**是确定性规则**：站级承载组
/// = 周期最大的组，仍唯一确定）⇒ **不产事件、不拒配置、无 `Err` 分支**。返回**每站一条**
/// 提示文案（含站 id、role、块数、站级周期）供调用方发射。
///
/// **本函数只做判定；发射点在 startup 装配期**（`mupc-core-bin` 的南向站装配处
/// `startup.rs` 的 `block_interval_hints(&config.south_stations)` 循环）
/// —— 理由与落点照 `crates/mupc-core-bin/src/core_config.rs:512-514` 的既有成文约定：
/// **判定放配置期、发射放 startup 装配期**。不可在此处直接 `tracing::debug!`：
/// 本 crate 的唯一生产调用链是 `SouthStationsConfig::validate`（`config.rs:302`）→
/// `core_config.rs::validate_south_stations` → `CoreConfig::validate`，由 **main Phase 1
/// 配置加载**（`crates/mupc-core-bin/src/main.rs:105`）调用；而 `tracing_subscriber` 到
/// **Phase 2**（`main.rs:164`）才 `try_init()` ⇒ **此处发射的日志在整条生产路径上都没有
/// 订阅者、事件被直接丢弃**（既不可达也测不到）。故拆成"可测的纯判定 + 装配期发射"。
pub fn block_interval_hints(cfg: &SouthStationsConfig) -> Vec<String> {
    cfg.stations
        .iter()
        .filter(|s| !s.regs.is_empty() && s.regs.iter().all(|b| b.interval_ms.is_some()))
        .map(|s| {
            format!(
                "southd 站 {} role={:?} 站内 {} 个块全部声明了 interval_ms（C8 不拒绝）：该站站级 interval_ms={}ms 已不再描述实际采集节奏（PRD §10.6 第 8 条，配置异味）",
                s.id,
                s.role,
                s.regs.len(),
                s.interval_ms
            )
        })
        .collect()
}

fn default_poll_ms() -> u64 {
    DEFAULT_POLL_MS
}

fn default_stale_timeout_s() -> u64 {
    DEFAULT_STALE_TIMEOUT_S
}
fn default_protocol() -> String {
    "modbus".into()
}
fn default_interval_ms() -> u64 {
    DEFAULT_INTERVAL_MS
}
fn default_reg_count() -> u16 {
    2
}
fn default_point_count() -> u16 {
    1
}
fn default_reg_format() -> RegFormat {
    RegFormat::Float32
}
fn default_baud_rate() -> u32 {
    DEFAULT_BAUD_RATE
}
fn default_reg_func() -> RegFunc {
    RegFunc::Holding
}

#[cfg(test)]
mod tests {
    use super::*;

    /// YAML 反序列化外层键容器（模拟 core_config 嵌入为结构体字段）
    #[derive(Deserialize)]
    struct Wrapper {
        south_stations: SouthStationsConfig,
    }

    /// 合法 5 站示例 YAML（role 走 snake_case）。
    ///
    /// S3b-2 T3 订正（设计 §11.5.3.2 **A1**）：① `battery_1` 的块改为**点名式**
    /// （`name: soc` 的块名式在规则 4「`soc` 点契约」下会被拒——契约锚定**点名**）；
    /// ② `fire_1` 的 `addr: 0` → `4`（规则 13：`addr == 0` 仅 `meter_batt`/`hvac` 合法）。
    /// **本 fixture 的 meter_grid 段仍保留 legacy 写法**（块名 `soc` 的旧形态见
    /// `default_reg_format_used_when_omitted` 等解析类用例，§11.5.3.4 C3）。
    const VALID_5_STATION_YAML: &str = r#"
south_stations:
  poll_ms: 1000
  stale_timeout_s: 5
  stations:
    - id: meter_grid
      role: meter_grid
      port: /dev/ttyS1
      slave: 1
      interval_ms: 1000
      regs:
        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: p_total, addr: 0x1006, format: float32, count: 2 }
        - { name: q, addr: 0x1008, format: float32, count: 6 }
        - { name: pf, addr: 0x100E, format: float32, count: 6 }
        - { name: u, addr: 0x1014, format: float32, count: 6 }
        - { name: i, addr: 0x101A, format: float32, count: 6 }
    - id: meter_batt
      role: meter_batt
      port: /dev/ttyS1
      slave: 2
      interval_ms: 1000
      regs: [{ name: p, addr: 0, format: float32, count: 2 }]
    - id: battery_1
      role: battery
      port: /dev/ttyS2
      slave: 1
      interval_ms: 1000
      regs: [{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }]
    - id: hvac_1
      role: hvac
      port: /dev/ttyS3
      slave: 3
      interval_ms: 2000
      regs: [{ name: temp, addr: 0, format: int32_scaled, scale: 0.01, count: 2 }]
    - id: fire_1
      role: fire
      port: /dev/ttyS4
      slave: 1
      interval_ms: 2000
      regs: [{ name: alarm, addr: 4, format: int32_scaled, scale: 1.0, count: 2 }]
"#;

    /// 含点名 `soc` 点的 battery 侧 regs（设计 §11.5.3.2 A2–A5/A15/A16 的共用订正片段：
    /// 规则 4 的 `soc` 点契约要求 battery 站必须有点名为 `soc` 的点）。
    const BATTERY_SOC_REGS: &str =
        "regs: [{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }]";

    #[test]
    fn parses_valid_5_station_yaml() {
        let w: Wrapper = serde_yaml::from_str(VALID_5_STATION_YAML).expect("解析失败");
        let cfg = w.south_stations;
        assert_eq!(cfg.poll_ms, 1000);
        assert_eq!(cfg.stale_timeout_s, 5);
        assert_eq!(cfg.stations.len(), 5);

        let grid = &cfg.stations[0];
        assert_eq!(grid.id, "meter_grid");
        assert_eq!(grid.role, Role::MeterGrid);
        assert_eq!(grid.slave, 1);
        assert_eq!(grid.interval_ms, 1000);
        assert_eq!(grid.port, "/dev/ttyS1");

        assert_eq!(cfg.stations[1].role, Role::MeterBatt);
        assert_eq!(cfg.stations[1].slave, 2);
        assert_eq!(cfg.stations[2].role, Role::Battery);
        assert_eq!(cfg.stations[3].role, Role::Hvac);
        assert_eq!(cfg.stations[3].interval_ms, 2000);
        assert_eq!(cfg.stations[4].role, Role::Fire);
    }

    #[test]
    fn meter_grid_regs_contain_pq_pfu_i_p_total() {
        let w: Wrapper = serde_yaml::from_str(VALID_5_STATION_YAML).expect("解析失败");
        let grid = &w.south_stations.stations[0];

        let names: Vec<&str> = grid.regs.iter().map(|b| b.name.as_str()).collect();
        for expect in ["p", "q", "pf", "u", "i", "p_total"] {
            assert!(names.contains(&expect), "缺寄存器块: {expect}");
        }

        // 相量块 count=6（float32），标量 p_total count=2
        let p = grid.regs.iter().find(|b| b.name == "p").unwrap();
        assert_eq!(p.format, RegFormat::Float32);
        assert_eq!(p.count, 6);
        let p_total = grid.regs.iter().find(|b| b.name == "p_total").unwrap();
        assert_eq!(p_total.format, RegFormat::Float32);
        assert_eq!(p_total.count, 2);
    }

    #[test]
    fn validate_passes_for_valid_config() {
        let w: Wrapper = serde_yaml::from_str(VALID_5_STATION_YAML).expect("解析失败");
        assert!(w.south_stations.validate().is_ok());
        assert!(w.south_stations.grid_station().is_some());
        assert_eq!(w.south_stations.grid_station().unwrap().id, "meter_grid");
    }

    /// **S2 顺带排查订正**：首站是 battery 而 fixture 无 `soc` 点 ⇒ **先被规则 4 拒**
    /// （2026-09-22 实测 `Err = "…站 a battery 站无任何点位名为 \`soc\`…"`），`is_err()` 虽绿
    /// 却**测不到 id 重复**。补 `soc` 点 regs 后本用例的 `Err` 才真正来自 id 重复。
    /// **断言不变**（仍 `is_err()`）。
    #[test]
    fn validate_rejects_duplicate_id() {
        let yaml = format!(
            "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: 1, {} }}\n    - {{ id: a, role: hvac, port: t2, slave: 2 }}",
            BATTERY_SOC_REGS
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    /// **S2 顺带排查订正**：两站均须配**完整相量块**，否则先被①的既有 `meter_grid` 整组校验
    /// （"缺相量块 p"）拒 ⇒ `is_err()` 虽绿却**测不到规则 2（至多一个 meter_grid）**。
    /// **断言不变**（仍 `is_err()`）。
    #[test]
    fn validate_rejects_multiple_meter_grid() {
        let regs = meter_grid_full_regs_yaml();
        let station = |id: &str, port: &str, slave: u8| {
            format!(
                "    - id: {id}\n      role: meter_grid\n      port: {port}\n      slave: {slave}\n      interval_ms: 1000\n      regs:\n{regs}\n"
            )
        };
        let yaml = format!(
            "south_stations:\n  stations:\n{}{}",
            station("mg1", "t1", 1),
            station("mg2", "t2", 2)
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(
            w.south_stations.validate().is_err(),
            "两个 meter_grid 站应被拒绝（AiIntegrator 单写方）"
        );
    }

    /// **S2 顺带排查订正**：两站均补 `soc` 点 regs，否则首站先被规则 4 拒
    /// （2026-09-22 实测），`is_err()` 虽绿却**测不到规则 3（至多一个 battery）**。
    /// **断言不变**（仍 `is_err()`）。
    #[test]
    fn validate_rejects_multiple_battery() {
        let yaml = format!(
            "south_stations:\n  stations:\n    - {{ id: b1, role: battery, port: t1, slave: 1, interval_ms: 1000, {} }}\n    - {{ id: b2, role: battery, port: t2, slave: 2, interval_ms: 1000, {} }}",
            BATTERY_SOC_REGS, BATTERY_SOC_REGS
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(
            w.south_stations.validate().is_err(),
            "两个 battery 站应被拒绝（BMS SOC 单源约束，AiIntegrator bms_soc 单槽）"
        );
    }

    #[test]
    fn validate_rejects_empty_id() {
        let yaml = r#"
south_stations:
  stations:
    - { id: "", role: battery, port: t1, slave: 1 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_port() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: "", slave: 1 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    /// **C6（配置期侧，设计 §11.5.3.4）**：battery 站点名集合无 `soc` → 规则 4 拒
    /// （`soc` 点契约，PRD §9.4.3；消费方**按点名**查找，缺名即静默不推 SOC）。
    ///
    /// 与 `scheduler.rs::battery_station_without_soc_block_does_not_push` 的**分工**：那一例是
    /// **运行期**纵深防御（不调 `validate`、直接构造 `StationConf` ⇒ 仍绿，保"调度器不依赖配置
    /// 校验"），本用例是**配置期**拦截（启动即 fail-fast）——两层各有其测。
    /// AC-1 ③ 的规则 4 验收用例（含"块名为 `soc` 已不是契约"等 3 种形态）见
    /// `tests/s3b2_config.rs::ac1_rule4_battery_soc_point_contract`（本用例只钉最小负向形态）。
    #[test]
    fn validate_rejects_battery_without_soc_point() {
        let yaml = r#"
south_stations:
  stations:
    - id: bms
      role: battery
      port: t1
      slave: 1
      interval_ms: 1000
      regs:
        - { name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("soc"),
            "battery 站无点名 soc 的点应被规则 4 拒，实际: {err}"
        );
    }

    /// A3（fixture 订正）：battery 站补含 `soc` 点的 `regs`，使本用例真正测到 slave 上界
    /// （否则会先被规则 4 拒——"测试通过却没测到"的隐患，设计 §11.5.3.5）。
    #[test]
    fn validate_accepts_max_slave() {
        let yaml = format!(
            "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: 247, {} }}",
            BATTERY_SOC_REGS
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(
            w.south_stations.validate().is_ok(),
            "slave=247 为合法上界，应通过: {:?}",
            w.south_stations.validate()
        );
    }

    #[test]
    fn validate_meter_grid_interval_boundary() {
        // 钉死 >= 边界语义：4999(<5000) 合法，5000(==DATA_FRESHNESS_MS) 拒绝。
        // meter_grid 须配完整 regs（S3b-1c 校验），否则 4999 也会因缺相量块被拒——隔离 interval 语义。
        for (iv, ok) in [(4999u64, true), (5000u64, false)] {
            let yaml = format!(
                "south_stations:\n  stations:\n    - id: mg\n      role: meter_grid\n      port: t1\n      slave: 1\n      interval_ms: {iv}\n      regs:\n        - {{ name: p, addr: 0x1000, format: float32, count: 6 }}\n        - {{ name: q, addr: 0x1006, format: float32, count: 6 }}\n        - {{ name: pf, addr: 0x100C, format: float32, count: 6 }}\n        - {{ name: u, addr: 0x1012, format: float32, count: 6 }}\n        - {{ name: i, addr: 0x1018, format: float32, count: 6 }}"
            );
            let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
            assert_eq!(
                w.south_stations.validate().is_ok(),
                ok,
                "meter_grid interval_ms={iv} 期望 ok={ok}"
            );
        }
    }

    /// A2（fixture 订正）：battery 侧补含 `soc` 点的 `regs`，使本用例真正测到 interval 上界
    /// （否则会先被规则 4 拒）。断言**不变**。
    #[test]
    fn validate_battery_interval_boundary() {
        // Battery 站与 meter_grid 对称：interval 须 < DATA_FRESHNESS_MS——battery 是 BMS SOC
        // 真源，若采集间隔 > fresh 窗口 5s，BMS 每轮仅前 5s fresh、余下回落核间 → SOC 源周期
        // 翻转、soc_protect 剪带震荡。4999(<5000) 合法，6000(>=5000) 拒绝。
        for (iv, ok) in [(4999u64, true), (6000u64, false)] {
            let yaml = format!(
                "south_stations:\n  stations:\n    - {{ id: bat, role: battery, port: t1, slave: 1, interval_ms: {iv}, {} }}",
                BATTERY_SOC_REGS
            );
            let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
            assert_eq!(
                w.south_stations.validate().is_ok(),
                ok,
                "battery interval_ms={iv} 期望 ok={ok}: {:?}",
                w.south_stations.validate()
            );
        }
    }

    /// A15（fixture 订正，v1.4 从"期望 Err 类"移入）：补合法 `regs`，使本用例仍由
    /// **slave 下界**规则拒（而非新规则 4）——断言 `is_err()` 不变。
    #[test]
    fn validate_rejects_slave_out_of_range() {
        for bad in [0u16, 248u16] {
            let yaml = format!(
                "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: {bad}, {} }}",
                BATTERY_SOC_REGS
            );
            let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
            assert!(w.south_stations.validate().is_err(), "slave={bad} 应被拒绝");
        }
    }

    /// A16（fixture 订正，同上）：补合法 `regs`，使本用例仍由 interval 下界规则拒。
    #[test]
    fn validate_rejects_zero_interval() {
        let yaml = format!(
            "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: 1, interval_ms: 0, {} }}",
            BATTERY_SOC_REGS
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_rejects_meter_grid_slow_poll() {
        let yaml = r#"
south_stations:
  stations:
    - { id: mg, role: meter_grid, port: t1, slave: 1, interval_ms: 6000 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    /// 完整 meter_grid regs（addr 非 0、半开区间不重叠、p_total 可选含）
    fn meter_grid_full_regs_yaml() -> &'static str {
        // 注意：与上面注释同口径——相量块 count 6，p_total count 2
        r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }
        - { name: p_total, addr: 0x101E, format: float32, count: 2 }"#
    }

    /// 单 meter_grid 站 yaml（regs_body 为已缩进 8 空格的列表行，原样内插）
    fn meter_grid_only_yaml(regs_body: &str) -> String {
        "south_stations:\n  stations:\n    - id: mg\n      role: meter_grid\n      port: t1\n      slave: 1\n      interval_ms: 1000\n      regs:\n".to_string()
            + regs_body
    }

    /// S3b-1c：meter_grid regs 缺相量块（这里缺 q）→ validate Err（总表 phase 真源须完整）
    #[test]
    fn validate_rejects_meter_grid_missing_phase_block() {
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("缺相量块 q"),
            "缺 q 应报缺块（含 q 名），实际: {err}"
        );
    }

    /// S3b-1c：空 regs 的 meter_grid 亦被拒（落入缺 p 分支；空 regs = 站永久 offline、phase 断供）
    #[test]
    fn validate_rejects_meter_grid_empty_regs() {
        let regs = "";
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("缺相量块 p"),
            "空 regs 应报缺 p 块，实际: {err}"
        );
    }

    /// S3b-1c：meter_grid regs 某块 addr=0 → validate Err（承接 legacy validate_reg_map 拒 0）
    #[test]
    fn validate_rejects_meter_grid_zero_addr() {
        let regs = r#"        - { name: p, addr: 0, format: float32, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("addr 不能为 0") && err.contains("p"),
            "p addr=0 应报 addr 不能为 0，实际: {err}"
        );
    }

    /// S3b-1c：meter_grid regs 两块半开区间重叠 → validate Err
    #[test]
    fn validate_rejects_meter_grid_overlapping_regs() {
        // q addr 0x1002 落入 p 块 [0x1000,0x1006) 区间内
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: q, addr: 0x1002, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("区间重叠") && err.contains("p") && err.contains("q"),
            "p 与 q 区间重叠应报重叠（含两块名），实际: {err}"
        );
    }

    /// S3b-1c：完整 p/q/pf/u/i + p_total（addr 非 0 不重叠）→ validate Ok
    #[test]
    fn validate_accepts_meter_grid_complete_regs() {
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(meter_grid_full_regs_yaml()))
            .expect("解析失败");
        assert!(
            w.south_stations.validate().is_ok(),
            "完整 meter_grid regs 应通过: {:?}",
            w.south_stations.validate()
        );
    }

    /// S3b-1c：相量块 p count:2（<6，count 语义错）→ Err 含 count（name/addr/重叠均过，
    /// 仅 count 不够；mapper decode_phase_block 硬性 ≥6，漏配 default 2 → 读回 <6 → 永久 offline）
    #[test]
    fn validate_rejects_meter_grid_phase_block_count_lt_6() {
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 2 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("count") && err.contains("p"),
            "相量块 p count=2 应报 count 须 ≥6，实际: {err}"
        );
    }

    /// S3b-1c：meter_grid int32_scaled 块 scale 漏写（serde 默认 0.0）→ Err 含 "scale"
    /// （raw×0 整块解 0 静默喂策略——防静默失真；float32 块 scale 默认 0.0 不受影响）
    #[test]
    fn validate_rejects_meter_grid_int32_scaled_zero_scale() {
        let regs = r#"        - { name: p, addr: 0x1000, format: int32_scaled, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("scale") && err.contains("p"),
            "p int32_scaled 漏写 scale 应报须 scale>0，实际: {err}"
        );
    }

    /// S3b-1c：同块名重复（异 addr 不重叠也死配）→ Err 含"块名重复"（mapper `.find` 取首块，后者静默失效）
    #[test]
    fn validate_rejects_meter_grid_duplicate_block_name() {
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: p, addr: 0x1006, format: float32, count: 6 }
        - { name: q, addr: 0x100C, format: float32, count: 6 }
        - { name: pf, addr: 0x1012, format: float32, count: 6 }
        - { name: u, addr: 0x1018, format: float32, count: 6 }
        - { name: i, addr: 0x101E, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("块名重复") && err.contains("p"),
            "两块同名 p 应报块名重复，实际: {err}"
        );
    }

    /// S3b-1c：完整 p/q/pf/u/i（无 p_total，count 各 6）→ Ok——p_total 可选正向
    #[test]
    fn validate_accepts_meter_grid_without_p_total() {
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        assert!(
            w.south_stations.validate().is_ok(),
            "无 p_total 的完整 meter_grid regs 应通过（p_total 可选）: {:?}",
            w.south_stations.validate()
        );
    }

    // ── 规则 6（符号性一致性）的判定核心：`check_symbolicity_row` 逐条 ──
    //
    // 配置级用例（对**真实** `POINT_REGS` 的命中/未命中）见 `tests/s3b2_config.rs` 的
    // `ac1_rule6_registry_offset_drift_rejected`（②，T4 落表后可达）与
    // `ac1_rule6_scope_boundaries`（role 隔离 / 非 16 位格式）；**① 在真实表下结构性
    // 不可达**（所有 `offset ≠ 0` 的标量行都已登记 `sym_src`，全表不变量由
    // `tests/point_table_vs_reference_config.rs::registry_internal_invariants` 钉住），
    // 故此处对判定核心**注入**登记行，把 ①（来源未登记）单独钉死。

    use crate::point_table::{PointReg, RegPointKind, SymSrc};

    fn row(offset: f64, sym_src: Option<SymSrc>) -> PointReg {
        PointReg {
            role: Role::Battery,
            addr: 116,
            kind: RegPointKind::Scalar(RegFormat::Uint16),
            scale: 0.1,
            offset,
            sym_src,
            label: "簇组电流 A",
            signals: &[],
        }
    }

    /// ① 命中行、`offset ≠ 0`，而该行 `sym_src` 为空 → 拒（raw 的解释方式无人可考）
    #[test]
    fn symbolicity_rejects_untraceable_offset() {
        let r = row(-1600.0, None);
        let err =
            check_symbolicity_row("x", 116, RegFormat::Uint16, -1600.0, Some(&r)).unwrap_err();
        assert!(err.contains("未登记符号性来源"), "实际: {err}");
        // 同值但已登记来源 → 放行
        let r = row(-1600.0, Some(SymSrc::VendorTypo));
        assert!(check_symbolicity_row("x", 116, RegFormat::Uint16, -1600.0, Some(&r)).is_ok());
    }

    /// ② 命中行而 `offset` 与登记值不等（含"漏配 → 缺省 0 ≠ −40"）→ 拒（配置与点表漂移）
    #[test]
    fn symbolicity_rejects_registry_drift() {
        let r = row(-40.0, Some(SymSrc::VendorTypo));
        // 漏配 offset（缺省 0 ≠ −40）
        let err = check_symbolicity_row("x", 117, RegFormat::Uint16, 0.0, Some(&r)).unwrap_err();
        assert!(err.contains("与点表登记值"), "实际: {err}");
        // 配错值
        assert!(check_symbolicity_row("x", 117, RegFormat::Uint16, -50.0, Some(&r)).is_err());
        // 一致且已登记来源 → 放行
        assert!(check_symbolicity_row("x", 117, RegFormat::Uint16, -40.0, Some(&r)).is_ok());
    }

    /// ③ 查不到行 → **放行**（§11.4.4 P0-2 裁定：现场 RC-3 合法改 `addr` 基准不得被拒）；
    /// 非 16 位格式不参与本条（PRD §9.4.3 只对 `uint16`/`int16` 要求符号性可追溯）。
    #[test]
    fn symbolicity_passes_when_no_row_or_wide_format() {
        assert!(check_symbolicity_row("x", 116, RegFormat::Uint16, -1600.0, None).is_ok());
        let r = row(-1600.0, None);
        assert!(check_symbolicity_row("x", 116, RegFormat::Float32, -1600.0, Some(&r)).is_ok());
        assert!(check_symbolicity_row("x", 116, RegFormat::Int32Scaled, -1600.0, Some(&r)).is_ok());
    }

    // ── 规则 15 判据 ③（合并后空洞 ≤ 4）的**直接单测**：直接钉 `merged_max_hole` 的契约，
    // 使 ③ 不再只经由端到端用例（`tests/s3b2_config.rs` 的极大性形态）间接覆盖。──

    /// 构造一个声明了点位的块（`ats` 为 1 起的点偏移，各占 1 寄存器）。
    /// 注意：多数构造形态会被规则 11 的首尾锚定拒——**无妨**，本组用例直测纯函数
    /// `merged_max_hole`（它按定义必须能对任意点清单算出合并窗口内的最大连续空洞）。
    fn pts_blk(name: &str, addr: u16, count: u16, ats: &[u16]) -> RegBlockConf {
        RegBlockConf {
            name: name.into(),
            addr,
            func: RegFunc::Holding,
            format: RegFormat::Uint16,
            scale: 1.0,
            count,
            offset: 0.0,
            byte_swap: false,
            points: ats
                .iter()
                .map(|at| PointConf {
                    at: *at,
                    count: 1,
                    name: None,
                    format: None,
                    scale: None,
                    offset: None,
                    word_order: WordOrder::HiLo,
                })
                .collect(),
            read_slice: false,
            interval_ms: None,
        }
    }

    /// 契约 = 合并窗口 `[a.addr, b.addr + b.count)` 内**未声明寄存器的最大连续段**（寄存器数）。
    #[test]
    fn merged_max_hole_measures_the_merged_window() {
        // ① 两块各自满覆盖（偏移 0/1 各声明）⇒ 合并窗口无空洞
        let a = pts_blk("a", 10, 2, &[1, 2]);
        let b = pts_blk("b", 12, 2, &[1, 2]);
        assert_eq!(merged_max_hole(&a, &b).unwrap(), 0);

        // ② **接缝空洞必须计入**：a 只声明其偏移 0、b 只声明其偏移 1（= 合并窗口偏移 3）
        //    ⇒ 窗口 0..4 中偏移 [1,3) 未声明 ⇒ 空洞 2（若只看单块内部空洞会误算成 0）
        let a = pts_blk("a", 10, 2, &[1]);
        let b = pts_blk("b", 12, 2, &[2]);
        assert_eq!(
            merged_max_hole(&a, &b).unwrap(),
            2,
            "判据 ③ 说的是**合并后**的窗口，接缝处空洞须计入"
        );

        // ③ 边界：空洞恰为 MAX_HOLE_REGS(4) ⇒ 不触发（`>` 语义）；取两块内部空洞的最大者
        let a = pts_blk("a", 10, 6, &[1, 6]); // 内部空洞 4（偏移 1..5）
        let b = pts_blk("b", 16, 2, &[1, 2]); // 无空洞
        assert_eq!(merged_max_hole(&a, &b).unwrap(), points::MAX_HOLE_REGS);
        // 空洞 5（偏移 1..6）⇒ 超上限
        let a = pts_blk("a", 10, 7, &[1, 7]);
        assert_eq!(merged_max_hole(&a, &b).unwrap(), 5);
        assert!(merged_max_hole(&a, &b).unwrap() > points::MAX_HOLE_REGS);
    }

    #[test]
    fn default_field_fallbacks_apply() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let st = &w.south_stations.stations[0];
        // 缺省项：protocol/slave/interval/regs/reg format
        assert_eq!(st.protocol, "modbus");
        assert_eq!(st.slave, 0);
        assert_eq!(st.interval_ms, DEFAULT_INTERVAL_MS);
        assert!(st.regs.is_empty());
    }

    #[test]
    fn default_reg_format_used_when_omitted() {
        let yaml = r#"
south_stations:
  stations:
    - id: a
      role: battery
      port: t1
      slave: 1
      regs:
        - { name: soc, addr: 100 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let blk = &w.south_stations.stations[0].regs[0];
        assert_eq!(blk.format, RegFormat::Float32);
        assert_eq!(blk.scale, 0.0);
        assert_eq!(blk.count, 2);
    }

    #[test]
    fn default_baud_and_func_apply_when_omitted() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let st = &w.south_stations.stations[0];
        assert_eq!(st.baud_rate, DEFAULT_BAUD_RATE);
    }

    #[test]
    fn reg_block_func_parses_holding_input() {
        let yaml = r#"
south_stations:
  stations:
    - id: a
      role: battery
      port: t1
      slave: 1
      regs:
        - { name: soc, addr: 100, func: input }
        - { name: temp, addr: 200 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let regs = &w.south_stations.stations[0].regs;
        assert_eq!(regs[0].func, RegFunc::Input);
        assert_eq!(regs[1].func, RegFunc::Holding); // 缺省 FC03
    }

    /// **S2 订正（规则 16 的 baud 分支，改法与 A4/A5/A15/A16 同）**：battery 侧补含 `soc` 点的
    /// `regs` —— 否则**先被规则 4 拒**（`Err = "…battery 站无任何点位名为 \`soc\`…"`，
    /// 2026-09-22 实测），`is_err()` 虽绿却**测不到规则 16**（假绿）。补后本用例的 `Err` 即来自
    /// 规则 16（同口 baud 不一致，消息含 `baud_rate`）。**断言不变**（仍 `is_err()`）。
    #[test]
    fn validate_rejects_same_port_mixed_baud() {
        let yaml = format!(
            "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: 1, baud_rate: 9600, {} }}\n    - {{ id: b, role: hvac,    port: t1, slave: 2, baud_rate: 19200 }}",
            BATTERY_SOC_REGS
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    /// A4（fixture 订正）：battery 侧补 `regs`（否则先被规则 4 拒）。断言不变。
    #[test]
    fn validate_accepts_same_port_same_baud() {
        let yaml = format!(
            "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: 1, baud_rate: 9600, {} }}\n    - {{ id: b, role: hvac,    port: t1, slave: 2, baud_rate: 9600 }}",
            BATTERY_SOC_REGS
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(
            w.south_stations.validate().is_ok(),
            "同口同 baud/同 parity 应通过: {:?}",
            w.south_stations.validate()
        );
    }

    /// **S2 订正（同 [`validate_rejects_same_port_mixed_baud`]）**：battery 侧补 `soc` 点 regs，
    /// 否则先被规则 4 拒（同前实测），`is_err()` 虽绿却测不到规则 16。**断言不变**。
    #[test]
    fn validate_rejects_same_port_mixed_baud_3_station() {
        // 第 3 站异 baud → 应被拒（同口物理共享波特率）
        let yaml = format!(
            "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: 1, baud_rate: 9600, {} }}\n    - {{ id: b, role: hvac,    port: t1, slave: 2, baud_rate: 9600 }}\n    - {{ id: c, role: fire,    port: t1, slave: 3, baud_rate: 19200 }}",
            BATTERY_SOC_REGS
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    /// A5（fixture 订正）：battery 侧补 `regs`（否则先被规则 4 拒）。断言不变。
    #[test]
    fn validate_accepts_diff_ports_same_baud() {
        // 不同口独立物理口，同 baud 无冲突 → Ok
        let yaml = format!(
            "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: 1, baud_rate: 9600, {} }}\n    - {{ id: b, role: hvac,    port: t2, slave: 2, baud_rate: 9600 }}",
            BATTERY_SOC_REGS
        );
        let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_baud_rate() {
        // baud_rate=0 整口皆 0 时同口一致性检查放行，必须靠下界拦截
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, baud_rate: 0 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_baud_same_port_all_zero() {
        // 整口 3 站 baud_rate 全为 0：同口一致性检查放行，须被下界拦截
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, baud_rate: 0 }
    - { id: b, role: hvac,    port: t1, slave: 2, baud_rate: 0 }
    - { id: c, role: fire,    port: t1, slave: 3, baud_rate: 0 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    // ═════ S3b-3（T8）：块级采集周期覆盖的配置期校验（设计 §12.7 / §12.8 的 4 条用例）═════

    /// 现网参考配置（**Task 6 起站级段 5 站** —— 原第 2 站 `pcs` 已按 ADR-016 迁至独立顶层段
    /// `south_pcs`；PRD §9.4.1 / §10.5 的现网复核输入；CRLF 逐字，故下方 `.replace` 的锚串
    /// 须带 `\r\n`）。常量名**不带站数**（原 `FIELD_6_STATION_YAML` 的"6"随拆分失效）。
    ///
    /// **PCS 站迁出对本组（规则 20–24）的影响：无关**（2026-09-26 逐条核对）。理由：
    /// ① 规则 20–22（块级周期覆盖）**只判显式声明了 `interval_ms` 的块** —— 原 `pcs` 站
    ///    的 `pcs_3zone` 块**未声明**块级周期（逐字见 cca8a20^ 的 fixture），故本来就"不判"；
    /// ② 规则 23/24（口预算）按 `port` 聚合，原 `pcs` 站独占 `/dev/ttyS7`（规则 P-2 要求），
    ///    其单块/单站形态的口占用率天然极小（U ≈ 0.1、Σ T_组 ≈ 97ms），对"通过"断言无判别力
    ///    —— 该形态由 `probe_free_hvac_first_case`（单站独占口）真实承担；
    /// ③ 故本组的 `Ok` 断言**不因少一个 pcs 站而变弱**（少扫一个恒过的口）。
    /// PCS 段自身的连接/周期/块结构校验另有独立锚：`tests/s3b2_config.rs` 的
    /// `ac1_rule18_pcs_interval_lower_bound` 与单测 `south_pcs_validate_applies_block_level_rules`。
    /// ⚠️ **残留（结构性的，非缺陷）**：`validate_block_intervals` 的入参是 `&SouthStationsConfig`
    /// ⇒ PCS 段**不参与**规则 20–24 的口预算扫描；其独占口 + 单块使其预算天然远低于阈值，
    /// 是否接线属 Task 7（`PcsHandle`）/ Task 10（core-bin 跨段规则）范畴。
    const FIELD_REF_STATION_YAML: &str = include_str!("../tests/fixtures/south_stations_s3b2.yaml");

    /// 解析 + 段内校验（本组用例的统一入口）。`Wrapper` = 模拟 core_config 的外层嵌入键。
    fn validated(yaml: &str) -> Result<(), String> {
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        w.south_stations.validate()
    }

    /// 单站探针 YAML（`hvac`，`R(role) = ∅` ⇒ **规则 22 不介入**）：一块 FC04（`count: 4`、
    /// 无 `points` ⇒ 规则 11/19 均不介入），块级周期可选声明（`None` = 不写该行 = 继承站周期）。
    fn probe_hvac_yaml(station_iv: u64, blk_iv: Option<u64>) -> String {
        let line = blk_iv.map_or(String::new(), |v| format!("\n          interval_ms: {v}"));
        format!(
            "south_stations:\n  poll_ms: 1000\n  stations:\n    - id: probe\n      role: hvac\n      port: /dev/ttyP0\n      slave: 1\n      baud_rate: 9600\n      interval_ms: {station_iv}\n      regs:\n        - name: probe_blk\n          func: input\n          addr: 0\n          count: 4\n          format: int16\n          scale: 0.1{line}\n"
        )
    }

    /// **AC-8-5（PRD §10.7）**：非法块级周期逐条拒绝 —— C1（`0`）/ C2（`300`）/
    /// C3（`1500`，`poll_ms = 1000`）/ C5（`6000 > 站周期`）/ C6（`fire` 的 `R` 内异周期）/
    /// C7（`battery` 的 `soc` 块提速）/ **C4**（`1.5 × T_组` 相对下界）⇒ **均 `Err`**，
    /// 各类文案含**站 id + 块名**（+ 实际取值 + 期望下界）。
    #[test]
    fn block_interval_constraints_rejected() {
        // C1 / C2 / C3 / C5：同一探针配置，只改块级周期一个值
        let check = |blk_iv: u64, station_iv: u64, marker: &str, value: &str| {
            let err = validated(&probe_hvac_yaml(station_iv, Some(blk_iv))).unwrap_err();
            assert!(
                err.contains("probe") && err.contains("probe_blk"),
                "{marker} 文案须含站 id 与块名，实际: {err}"
            );
            assert!(
                err.contains(value),
                "{marker} 文案须含实际取值 {value}，实际: {err}"
            );
            assert!(err.contains(marker), "{marker} 分支标记缺失，实际: {err}");
            err
        };
        let e1 = check(0, 5000, "C1", "interval_ms=0");
        assert!(e1.contains("> 0"), "C1 文案须写期望下界，实际: {e1}");
        let e2 = check(300, 5000, "C2", "interval_ms=300");
        assert!(e2.contains("500"), "C2 文案须写期望下界 500，实际: {e2}");
        let e3 = check(1500, 5000, "C3", "interval_ms=1500");
        assert!(
            e3.contains("poll_ms=1000"),
            "C3 文案须写 poll_ms 当前值，实际: {e3}"
        );
        let e5 = check(6000, 5000, "C5", "interval_ms=6000");
        assert!(e5.contains("5000"), "C5 文案须写站周期，实际: {e5}");

        // C6（R(role) 内块异周期）：fire 的 R = {覆盖寄存器 11 的寄存器块 fire_sys}
        // ∪ {块名前缀 fire_det 的块}；fire_det 声明 1000、fire_sys 继承站周期 5000
        let c6 = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: fire_probe
      role: fire
      port: /dev/ttyP1
      slave: 1
      baud_rate: 9600
      interval_ms: 5000
      regs:
        - { name: fire_sys, func: holding, addr: 4, count: 13, format: uint16, scale: 1.0 }
        - { name: fire_det, func: holding, addr: 17, count: 114, format: uint16, scale: 1.0, interval_ms: 1000 }
"#;
        let err = validated(c6).unwrap_err();
        assert!(
            err.contains("fire_probe") && err.contains("fire_sys") && err.contains("fire_det"),
            "C6 文案须含站 id 与冲突的两个块名，实际: {err}"
        );
        assert!(
            err.contains("5000") && err.contains("1000") && err.contains("C6"),
            "C6 文案须含两个块的取值，实际: {err}"
        );

        // C7（R(role) 内提速）：battery 的 R = {承载点名 soc 的那一块}；该块声明 1000 < 站周期 2000
        let c7 = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: bms_probe
      role: battery
      port: /dev/ttyP2
      slave: 1
      baud_rate: 9600
      interval_ms: 2000
      regs:
        - { name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, interval_ms: 1000, points: [{ at: 1, name: soc }] }
"#;
        let err = validated(c7).unwrap_err();
        assert!(
            err.contains("bms_probe") && err.contains("bms_io"),
            "C7 文案须含站 id 与判据块名，实际: {err}"
        );
        assert!(
            err.contains("1000") && err.contains("2000") && err.contains("C7"),
            "C7 文案须含块的取值与站周期，实际: {err}"
        );

        // C4（`1.5 × T_组` 相对下界）：独立站（9600、`poll_ms = 1000`、站周期 2000）内
        // **3 个** FC04 `count: 120` 块**均声明** `interval_ms: 1000`（三块 `eff` 相同 ⇒
        // **仍为同一读组**，V-1；只声明一块则不触发 C4 —— 该块自成一组、`T_组` 只剩 267.12ms）：
        // `T_块 = (8 + (5 + 240)) × 1.04 + 4 = 267.12ms` ⇒ `T_组 = 3 × 267.12 = 801.36ms`
        // ⇒ `1.5 × T_组 = 1202.04ms > 1000` ⇒ **C4 拒**（文案含 `1202`）
        let c4 = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: c4_probe
      role: hvac
      port: /dev/ttyP3
      slave: 1
      baud_rate: 9600
      interval_ms: 2000
      regs:
        - { name: c4_blk_1, func: input, addr: 0, count: 120, format: uint16, scale: 1.0, interval_ms: 1000 }
        - { name: c4_blk_2, func: input, addr: 120, count: 120, format: uint16, scale: 1.0, interval_ms: 1000 }
        - { name: c4_blk_3, func: input, addr: 240, count: 120, format: uint16, scale: 1.0, interval_ms: 1000 }
"#;
        assert_eq!(
            c4.matches("interval_ms: 1000").count(),
            3,
            "C4 的构造要求**三块均声明**（否则该组只剩 1 块、C4 反而通过）"
        );
        let err = validated(c4).unwrap_err();
        assert!(
            err.contains("c4_probe") && err.contains("c4_blk_1"),
            "C4 文案须含站 id 与块名，实际: {err}"
        );
        assert!(
            err.contains("1202"),
            "C4 文案须含 `1.5 × T_组` 的计算值 1202.04，实际: {err}"
        );
        // 判据顺序（设计 §12.7 的注）：规则号升序、首个失败即返回 ⇒ 该 `Err` 必来自
        // **规则 20 的 C4 分支**，而不是规则 23（后者的文案写 `U_口`、**不写** `1.5 × T_组`）
        assert!(
            !err.contains("U_口"),
            "规则 20 须先于规则 23 返回（否则无法机械证明 C4 被求值），实际: {err}"
        );
        // 用例形态说明（评审确认项）：各子例**各自**调 `validated()`、**无提前 return**
        // （C4 确被执行），但单函数串行的代价是 **C1–C7 的失败不可同时观测** —— 首条
        // `assert` panic 即掩盖其后各条（设计 §12.8 即把 AC-8-5 列作**一条**用例，故不拆分）。
    }

    /// **`interval_ms: None`（缺省 = 继承站周期）路径**（PRD §10.3.1 定性 1；设计 §12.2.1/§12.7）：
    /// `None` ⇒ 规则 20/21/22 **均不判**该块（其下界/上界已由站级既有规则覆盖），
    /// 规则 23/24 仍按"该块所在组"参与口预算 ⇒ 本配置须 `validate()` **`Ok`**。
    ///
    /// 本用例是探针 `probe_hvac_yaml` 的 **`blk_iv: None` 形参路径的唯一调用点**（此前该形参
    /// 无调用点 ⇒ 该路径无覆盖）。形参**不删**：`None`/`Some` 两态共用同一 fixture 才有判别力。
    ///
    /// **与 `accepts_first_case_hvac_fast_bit_block` 不重复**（取舍理由）：后者是**首例逐字配置**
    /// （2 块、块级周期真实取值、`eff` 分属两组的完整形态）；本用例是**单块探针**、只改
    /// `blk_iv` 一个变量 ⇒ 钉住的是"`None` **不被当成** 0 / 不被跳过"这一条**形参语义**
    /// （若把 `None` 误当 0 或强行代入下界，本用例即红，而首例用例对此完全不敏感）。
    #[test]
    fn block_interval_none_inherits_station_period_and_passes() {
        // 站周期 5000：若 `None` 被误当作 0 ⇒ 踩 C1；被误当作 1 ⇒ 踩 C2；被误当作未对齐值
        // ⇒ 踩 C3（`poll_ms = 1000`）。三者都不成立 ⇒ 才 `Ok`。
        assert_eq!(
            validated(&probe_hvac_yaml(5000, None)),
            Ok(()),
            "块级 interval_ms 缺省 ⇒ 继承站周期、规则 20/21/22 不判 ⇒ 不得拒绝"
        );
        // 判别力对照：同一站、同一块的**显式**取值走判定路径（`Some`）⇒ 与 `None` 非同一真值域
        assert_eq!(
            validated(&probe_hvac_yaml(5000, Some(1000))),
            Ok(()),
            "显式 1000（≥ C2 下界、mod poll_ms 对齐、≤ 站周期）⇒ 亦通过"
        );
    }

    /// **首例（HVAC 位块快采）配置可通过**（设计 §12.8；PRD §10.3.3 的示例配置逐字）。
    #[test]
    fn accepts_first_case_hvac_fast_bit_block() {
        let yaml = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: hvac
      role: hvac
      port: /dev/ttyS3
      slave: 1
      baud_rate: 9600
      parity: even
      interval_ms: 5000
      regs:
        - name: hvac_in
          func: input
          addr: 0
          count: 4
          format: int16
          scale: 0.1
          points:
            - { at: 1 }
            - { at: 3 }
            - { at: 4, format: uint16 }
        - name: hvac_di
          func: discrete
          addr: 0
          count: 31
          interval_ms: 1000
"#;
        assert_eq!(
            validated(yaml),
            Ok(()),
            "首例 YAML 须通过（含 S3b-3 新增的规则 20–24）"
        );
    }

    /// **AC-8-6（PRD §10.7）**：口预算与单轮最坏耗时可复算 ——
    /// ① 现网参考配置（Task 6 起 5 站，含改造后的 hvac）⇒ `Ok`（**零新增拒绝**）；
    /// ② 构造 `U_口 > 0.5` ⇒ `Err`（规则 23）；
    /// ③ **规则 24 的正反两例**：通过例 = hvac 首例（`Σ T_组 = 47.52ms ≤ 1500ms`）；
    ///    拒绝例 = 设计 §12.7 的 W-1 构造（同口两组，`U = 0.447 ≤ 0.5` **但**
    ///    `Σ T_组 = 2156.56ms > 1500ms`）⇒ 必须 `Err`。
    /// **该例在只有规则 23 时必然 `Ok`** ⇒ 是本条存在的**判别锚**（规则 24 ≠ C9 的推论）。
    #[test]
    fn bus_budget_accepts_field_config_and_rejects_overload() {
        // ①-a 现网参考配置（fixture 逐字；hvac 尚未含 S3b-3 的新增行）
        assert_eq!(
            validated(FIELD_REF_STATION_YAML),
            Ok(()),
            "现网 5 站不得被规则 20–24 新增拒绝"
        );
        // ①-b 改造后的 hvac（`hvac_di` 加 `interval_ms: 1000`；§12.6 的迁移行）⇒ 仍 `Ok`
        let fast = FIELD_REF_STATION_YAML.replace(
            "          func: discrete\r\n          addr: 0\r\n          count: 31\r\n",
            "          func: discrete\r\n          addr: 0\r\n          count: 31\r\n          interval_ms: 1000   # S3b-3 首例（测试注入）\r\n",
        );
        assert_eq!(
            fast.matches("S3b-3 首例（测试注入）").count(),
            1,
            "注入须恰好命中 hvac_di 一处（锚串失配即本用例失效）"
        );
        assert_eq!(
            validated(&fast),
            Ok(()),
            "改造后的现网 5 站（hvac 位块 1000ms）不得被规则 20–24 拒"
        );

        // ② `U_口 > 0.5`：组周期夹到 ≈`T_组`（两个 FC04 count 120 块声明 1000 同组）
        let overload = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: overload
      role: hvac
      port: /dev/ttyP9
      slave: 1
      baud_rate: 9600
      interval_ms: 2000
      regs:
        - { name: ov_1, func: input, addr: 0, count: 120, format: uint16, scale: 1.0, interval_ms: 1000 }
        - { name: ov_2, func: input, addr: 120, count: 120, format: uint16, scale: 1.0, interval_ms: 1000 }
        - { name: ov_3, func: input, addr: 240, count: 120, format: uint16, scale: 1.0 }
"#;
        let err = validated(overload).unwrap_err();
        assert!(
            err.contains("U_口"),
            "② 须由规则 23（C9 口预算）拒，实际: {err}"
        );
        assert!(
            err.contains("/dev/ttyP9") && err.contains("overload") && err.contains("ov_1"),
            "规则 23 文案须含 port + 触发的组（站 id / 块名），实际: {err}"
        );
        // 复算（与实现同一纯函数）：`U = 534.24/1000 + 267.12/2000 = 0.6678 > 0.5`
        {
            let w: Wrapper = serde_yaml::from_str(overload).expect("解析失败");
            let s = &w.south_stations.stations[0];
            let gs = read_groups_of(s);
            let u: f64 = gs
                .iter()
                .map(|g| group_tx_time_ms(s, g) / g.interval_ms as f64)
                .sum();
            assert!(
                u > MAX_PORT_UTILIZATION,
                "U={u} 须 > 0.5（本用例的构造前提）"
            );
        }

        // ③-通过例：hvac 首例（`Σ T_组 = 21.68 + 25.84 = 47.52ms ≤ 1.5 × 1000 = 1500ms`）
        let first_case = probe_free_hvac_first_case();
        assert_eq!(
            validated(&first_case),
            Ok(()),
            "规则 24 通过例：首例 Σ T_组 = 47.52ms ≤ 1500ms"
        );
        {
            let w: Wrapper = serde_yaml::from_str(&first_case).expect("解析失败");
            let s = &w.south_stations.stations[0];
            let gs = read_groups_of(s);
            assert_eq!(gs.len(), 2, "首例 = 快组（1000）+ 站周期组（5000）");
            // 逐组钉周期：只断言"2 组"时，快/慢组周期互换仍会绿（AC-8-6 的锚会失守）
            let mut periods: Vec<u64> = gs.iter().map(|g| g.interval_ms).collect();
            periods.sort_unstable();
            assert_eq!(
                periods,
                vec![1000, 5000],
                "首例两组周期须为 1000（hvac_di 快采）与 5000（站周期），实际 {periods:?}"
            );
            let sum: f64 = gs.iter().map(|g| group_tx_time_ms(s, g)).sum();
            assert!(
                (sum - 47.52).abs() < 1e-9,
                "Σ T_组 须 = 47.52ms（21.68 + 25.84），实际 {sum}"
            );
            assert!(sum <= 1.5 * 1000.0);
            // AC-8-6 的第二个可测口径：口占用率（PRD §10.5「口占用上界」的复算值）
            let u: f64 = gs
                .iter()
                .map(|g| group_tx_time_ms(s, g) / g.interval_ms as f64)
                .sum();
            assert!(
                (u - 0.026848).abs() < 1e-6,
                "首例 U_口 须 = 0.026848（= 21.68/1000 + 25.84/5000），实际 {u}"
            );
            assert!(u <= MAX_PORT_UTILIZATION);
        }

        // ③-拒绝例：同口两组 —— 组 A（周期 1000、`T_组 = 19.6ms`）+ 组 B（周期 5000、
        // `T_组 = 8 × 267.12 = 2136.96ms`）⇒ `Σ T_组 = 2156.56ms > 1.5 × 1000 = 1500ms` ⇒ `Err`
        let rule24 = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: rule24_fast
      role: hvac
      port: /dev/ttyP8
      slave: 1
      baud_rate: 9600
      interval_ms: 1000
      regs:
        - { name: f1, func: input, addr: 0, count: 1, format: uint16, scale: 1.0, interval_ms: 1000 }
    - id: rule24_slow
      role: hvac
      port: /dev/ttyP8
      slave: 2
      baud_rate: 9600
      interval_ms: 5000
      regs:
        - { name: s1, func: input, addr: 0, count: 120, format: uint16, scale: 1.0 }
        - { name: s2, func: input, addr: 120, count: 120, format: uint16, scale: 1.0 }
        - { name: s3, func: input, addr: 240, count: 120, format: uint16, scale: 1.0 }
        - { name: s4, func: input, addr: 360, count: 120, format: uint16, scale: 1.0 }
        - { name: s5, func: input, addr: 480, count: 120, format: uint16, scale: 1.0 }
        - { name: s6, func: input, addr: 600, count: 120, format: uint16, scale: 1.0 }
        - { name: s7, func: input, addr: 720, count: 120, format: uint16, scale: 1.0 }
        - { name: s8, func: input, addr: 840, count: 120, format: uint16, scale: 1.0 }
"#;
        // 先机械复算两个口径（与实现同一纯函数）：`U = 0.446992 ≤ 0.5` **但** `Σ T > 1500`
        {
            let w: Wrapper = serde_yaml::from_str(rule24).expect("解析失败");
            let (sa, sb) = (&w.south_stations.stations[0], &w.south_stations.stations[1]);
            let ga = read_groups_of(sa);
            let gb = read_groups_of(sb);
            assert_eq!((ga.len(), gb.len()), (1, 1), "本例每站单组");
            let ta = group_tx_time_ms(sa, &ga[0]);
            let tb = group_tx_time_ms(sb, &gb[0]);
            assert!((ta - 19.6).abs() < 1e-9, "T_组(A) 须 = 19.6ms，实际 {ta}");
            assert!(
                (tb - 2136.96).abs() < 1e-9,
                "T_组(B) 须 = 8 × 267.12 = 2136.96ms，实际 {tb}"
            );
            let u = ta / 1000.0 + tb / 5000.0;
            assert!(
                u <= MAX_PORT_UTILIZATION,
                "U = {u} 须 ≤ 0.5 —— 这证明规则 24 不是规则 23（C9）的推论"
            );
            assert!(
                ta + tb > 1.5 * 1000.0,
                "Σ T_组 = {} 须 > 1.5 × 最小非零组周期 = 1500ms",
                ta + tb
            );
        }
        let err = validated(rule24).unwrap_err();
        assert!(
            err.contains("Σ T_组") && err.contains("最小非零组周期"),
            "③ 须由规则 24 拒，实际: {err}"
        );
        assert!(
            !err.contains("U_口"),
            "规则 24 须独立于规则 23（U ≤ 0.5 仍拒），实际: {err}"
        );
        assert!(
            err.contains("/dev/ttyP8") && err.contains("2156") && err.contains("1500"),
            "规则 24 文案须含 port + 实测 Σ T_组 + 阈值，实际: {err}"
        );
        assert!(
            err.contains("最小非零组周期 1000ms") && err.contains("rule24_fast"),
            "规则 24 文案须给出最小周期的取值与来源组，实际: {err}"
        );
    }

    /// 首例配置（`accepts_first_case_hvac_fast_bit_block` 的同一份 YAML；本处供 `bus_budget_*`
    /// 的规则 24 通过例做**数值复算**用）。
    fn probe_free_hvac_first_case() -> String {
        r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: hvac
      role: hvac
      port: /dev/ttyS3
      slave: 1
      baud_rate: 9600
      parity: even
      interval_ms: 5000
      regs:
        - name: hvac_in
          func: input
          addr: 0
          count: 4
          format: int16
          scale: 0.1
          points:
            - { at: 1 }
            - { at: 3 }
            - { at: 4, format: uint16 }
        - name: hvac_di
          func: discrete
          addr: 0
          count: 31
          interval_ms: 1000
"#
        .to_string()
    }

    /// **兼容性承诺（设计 §12.2.1）**：既有 YAML（无该字段）⇒ `None`；
    /// `serde_yaml::to_string` 往返**不出现**块级 `interval_ms` 键（`skip_serializing_if`）。
    #[test]
    fn block_interval_serde_roundtrip_is_unchanged_when_absent() {
        let w: Wrapper = serde_yaml::from_str(VALID_5_STATION_YAML).expect("解析失败");
        let cfg = w.south_stations;
        assert!(
            cfg.stations
                .iter()
                .flat_map(|s| s.regs.iter())
                .all(|b| b.interval_ms.is_none()),
            "既有 YAML 未声明该字段 ⇒ 全部块须解析为 None（缺省 = 继承站周期）"
        );
        // 块级键不落盘：单看块列表（站级 `interval_ms` 是既有字段、恒被序列化，
        // 故不能对整段用 `!contains` —— 那会把站级键误判为块级键）
        let blocks = serde_yaml::to_string(&cfg.stations[0].regs).expect("序列化失败");
        assert!(
            !blocks.contains("interval_ms"),
            "skip_serializing_if 须使 None 不落盘，实际: {blocks}"
        );
        // 整段往返：`interval_ms` 出现次数 == 站数（每站恰一处**站级**键），块级一处也不多
        let whole = serde_yaml::to_string(&cfg).expect("序列化失败");
        assert_eq!(
            whole.matches("interval_ms").count(),
            cfg.stations.len(),
            "整段往返不得新增块级 interval_ms 键，实际: {whole}"
        );
        // `whole` 是**段内**结构（`SouthStationsConfig`）的序列化，**不含**外层
        // `south_stations:` 嵌入键 ⇒ 回读须用同一层类型（用 `Wrapper` 会因缺键而失败）
        let cfg2: SouthStationsConfig = serde_yaml::from_str(&whole).expect("往返解析失败");
        assert_eq!(cfg2.stations.len(), cfg.stations.len());
        assert!(cfg2
            .stations
            .iter()
            .flat_map(|s| s.regs.iter())
            .all(|b| b.interval_ms.is_none()));
    }

    /// **PRD §10.6 第 8 条（C8）/ 设计 §12.7 末段**：站内**全部**块都声明了 `interval_ms`
    /// ⇒ **不拒绝**（C8 是确定性规则：站级承载组 = 周期最大的组，仍唯一确定），在
    /// **startup 装配期**发一条 `tracing::debug!` 提示（此时站级 `interval_ms` 已不再描述
    /// 该站的实际节奏）。
    ///
    /// 本用例钉**两半**：
    /// ① `validate()` 须 `is_ok()`（不拒绝）；
    /// ② **提示的判定本身**可测 —— 断言纯函数 [`block_interval_hints`] 的**返回值**
    ///   （全声明站 ⇒ 恰 1 条、文案含站 id；未全声明站 ⇒ 不出现、列表为空）。
    /// **不再依赖日志订阅器**：`validate_block_intervals` 里原先那段"只记日志"的分支已按
    /// 评审返工删除（在 main Phase 1 无订阅者、不可达），判定与发射已拆开（判定 = 本函数，
    /// 发射 = `core-bin/startup.rs` 南向站装配处）。
    #[test]
    fn all_blocks_declared_is_accepted_with_debug_hint() {
        // 首例配置（HVAC 位块快采）的**退化形态**：`hvac_in` 也显式声明站周期 5000
        let yaml = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: hvac
      role: hvac
      port: /dev/ttyS5
      slave: 1
      baud_rate: 9600
      parity: even
      interval_ms: 5000
      regs:
        - name: hvac_in
          func: input
          addr: 0
          count: 4
          format: int16
          scale: 0.1
          interval_ms: 5000
          points:
            - { at: 1 }
            - { at: 3 }
            - { at: 4, format: uint16 }
        - name: hvac_di
          func: discrete
          addr: 0
          count: 31
          interval_ms: 1000
"#;
        // 构造前提：站内**每块**都声明了 `interval_ms`（= C8 的配置异味，故会走到该提示）
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        {
            let s = &w.south_stations.stations[0];
            assert!(
                !s.regs.is_empty() && s.regs.iter().all(|b| b.interval_ms.is_some()),
                "前提：站内全部块都声明了 interval_ms（本用例若不满足即为构造失效）"
            );
        }
        assert_eq!(
            validated(yaml),
            Ok(()),
            "全声明 ⇒ 不得拒绝（PRD §10.6 第 8 条：C8 不设配置期拒绝）"
        );
        // ① 提示**本身**可测（不再依赖日志订阅器）：全声明站 ⇒ **恰 1 条**、文案含站 id
        let hints = block_interval_hints(&w.south_stations);
        assert_eq!(
            hints.len(),
            1,
            "全声明站须恰产 1 条 C8 提示（每站一条），实际: {hints:?}"
        );
        assert!(
            hints[0].contains("hvac"),
            "提示文案须含站 id，实际: {}",
            hints[0]
        );
        // ② 反例（未全声明）：同形配置但 `hvac_in` **不声明**（继承站周期）⇒ 不构成
        //    "站内全部块都声明" ⇒ 该站**不出现在**列表里（列表为空）
        let partial = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: hvac_partial
      role: hvac
      port: /dev/ttyS7
      slave: 1
      baud_rate: 9600
      parity: even
      interval_ms: 5000
      regs:
        - { name: hvac_in, func: input, addr: 0, count: 4, format: int16, scale: 0.1, points: [{ at: 1 }, { at: 3 }, { at: 4, format: uint16 }] }
        - { name: hvac_di, func: discrete, addr: 0, count: 31, interval_ms: 1000 }
"#;
        let wp: Wrapper = serde_yaml::from_str(partial).expect("解析失败");
        assert_eq!(
            wp.south_stations.stations[0]
                .regs
                .iter()
                .filter(|b| b.interval_ms.is_some())
                .count(),
            1,
            "反例的构造前提：仅 `hvac_di` 声明（另一块继承站周期）"
        );
        assert_eq!(
            validated(partial),
            Ok(()),
            "反例须是**合法**配置（否则「提示为空」会因整段被拒而失去判别力）"
        );
        assert_eq!(
            block_interval_hints(&wp.south_stations),
            Vec::<String>::new(),
            "只有部分块声明 ⇒ 不构成 C8 配置异味 ⇒ 提示列表须为空"
        );
    }

    /// **设计 §12.10.3 的 R-10**：`T_组` 的复算必须用**本站**的 `baud_rate` —— 同口一致
    /// （规则 16 强制），但**跨口不同**（PCS 站是 19200）⇒ 复算须逐站取本站值。
    ///
    /// 直接单测私有纯函数 `tx_time_ms`（与本 `mod tests` 同文件 ⇒ 可直接调用）：
    /// - `9600` + `FC02(discrete)` + `count = 31` ⇒ 数据字节 `ceil(31/8) = 4` ⇒ 帧字节
    ///   `8 + 5 + 4 = 17` ⇒ 字节耗时 `10/9600 s = 1.0416667… → **1.04ms**`
    ///   （`byte_time_ms` 的 2 位小数口径）⇒ `17 × 1.04 + 4 = **21.68ms**`；
    /// - `19200` + `FC04(input)` + `count = 4` ⇒ 数据字节 `2 × 4 = 8` ⇒ 帧字节
    ///   `8 + 5 + 8 = 21` ⇒ 字节耗时 `10/19200 s = 0.5208333… → **0.52ms**`（同一取整口径）
    ///   ⇒ `21 × 0.52 + 4 = **14.92ms**`。
    ///
    /// **判别力**：两例的字节耗时不同（1.04 ≠ 0.52）⇒ 实现若误把某个 **固定** baud_rate
    /// （或另一站的 19200）代入复算，两例中必有一例失配。
    #[test]
    fn tx_time_uses_station_baud_rate() {
        let t9600 = tx_time_ms(9600, RegFunc::Discrete, 31);
        assert!(
            (t9600 - 21.68).abs() < 0.01,
            "9600/FC02/count=31 ⇒ 17 字节 × 1.04ms + 4 = 21.68ms，实际 {t9600}"
        );
        let t19200 = tx_time_ms(19200, RegFunc::Input, 4);
        assert!(
            (t19200 - 14.92).abs() < 0.01,
            "19200/FC04/count=4 ⇒ 21 字节 × 0.52ms + 4 = 14.92ms，实际 {t19200}"
        );
    }

    /// **判据钉**：`meter_grid` 的 `R(role)` **含 `p_total`**（PRD §10.3.2 的定义表）——
    /// 把 `p_total` 从 `criterion_block_indices` 的 `meter_grid` 集合里删掉，本用例**必红**。
    ///
    /// **构造法（为什么是"六块一律提速"而不是"只提速 `p_total`"）**：规则 22 的 **C6 先于
    /// C7** 求值，且 C6 要求 `R(role)` 内 `eff` 全相同；只提速 `p_total` 的配置**必然**先被
    /// C6 拒（文案含 `C6`、**不含** `C7`）⇒ 永远到不了 C7。故这里把 `R(role)` 的六块**一律**
    /// 声明为同一 `eff = 1000ms`（C6 恒真、"全部块都声明"仅触发 C8 配置异味提示 —— 该提示的
    /// 发射在 startup 装配期、**不构成拒配置分支**），站周期
    /// `2000ms`（`meter_grid` 须 `< 5000ms`，见①的新鲜度上界）⇒ **只触发 C7（判据块不得提速）**。
    ///
    /// **为什么 `p_total` 写在 `regs` 首位**：C7 的文案只写**首个**违反块的名字，而
    /// `criterion_block_indices` 按 `regs` 书写序返回 ⇒ 首位即 C7 的指名对象。
    /// **判别力**（若 `p_total` 不在 `R(role)` 内）：`R(role)` = {p,q,pf,u,i}（仍全 1000 ⇒ C7
    /// 仍拒）**但文案改指 `p`** ⇒ 下方 `contains("p_total")` 失配 ⇒ 本用例红。
    #[test]
    fn meter_grid_criterion_block_cannot_be_speeded_up() {
        let yaml = r#"
south_stations:
  poll_ms: 1000
  stations:
    - id: grid_probe
      role: meter_grid
      port: /dev/ttyP6
      slave: 1
      baud_rate: 9600
      interval_ms: 2000
      regs:
        - { name: p_total, addr: 0x1006, format: float32, count: 2, interval_ms: 1000 }
        - { name: p,       addr: 0x1000, format: float32, count: 6, interval_ms: 1000 }
        - { name: q,       addr: 0x1008, format: float32, count: 6, interval_ms: 1000 }
        - { name: pf,      addr: 0x100E, format: float32, count: 6, interval_ms: 1000 }
        - { name: u,       addr: 0x1014, format: float32, count: 6, interval_ms: 1000 }
        - { name: i,       addr: 0x101A, format: float32, count: 6, interval_ms: 1000 }
"#;
        // 构造前提（机械复算，与实现同一批纯函数）：六块 `eff` 全 = 1000ms 且相同
        // ⇒ 规则 22 的 C6 不介入；规则 20/21 亦通过（`1000 % poll_ms == 0`、
        // `1000 ≤ 站周期 2000`、`1.5 × T_组 = 1.5 × 171.68 = 257.52 ≤ 1000`）
        // ⇒ 唯一可能拒的条件就是 C7（`eff < 站周期`）。
        {
            let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
            let s = &w.south_stations.stations[0];
            let idx = criterion_block_indices(s);
            assert!(
                idx.iter().any(|&i| s.regs[i].name == "p_total"),
                "前提：p_total 须属 R(role)（本用例的被钉对象）"
            );
            assert_eq!(idx.len(), 6, "前提：R(role) 须为 p/q/pf/u/i/p_total 六块");
            assert!(
                idx.iter()
                    .all(|&i| s.regs[i].effective_interval_ms(s.interval_ms) == 1000),
                "前提：六块 eff 须全相同（否则 C6 先拒、到不了 C7 分支）"
            );
        }
        let err = validated(yaml).unwrap_err();
        assert!(
            err.contains("p_total"),
            "C7 文案须指名 R(role) 首位的判据块 p_total，实际: {err}"
        );
        assert!(
            err.contains("C7"),
            "本配置须由 C7（判据块不得提速）拒，实际: {err}"
        );
        assert!(
            !err.contains("C6"),
            "不得由 C6（判据块异周期）拒 —— 它先于 C7 求值，出现即说明本用例构造失效，实际: {err}"
        );
    }
}

// ═══════════ Task 6（设计 §13.7/§13.8，ADR-016）：`south_pcs` 顶层段 ═══════════

#[cfg(test)]
mod south_pcs_tests {
    use super::*;

    /// `PointConf` 的紧凑构造（只给 `at`/`count`，其余缺省）—— 供本模块 fixture 用。
    fn pt(at: u16, count: u16) -> PointConf {
        PointConf {
            at,
            count,
            name: None,
            format: None,
            scale: None,
            offset: None,
            word_order: Default::default(),
        }
    }

    /// 合法块：`pcs_3zone` **76 寄存器**，`points` **锚定覆盖满窗口**（首偏移 0、末末端 = count）。
    ///
    /// ⚠️ **本 fixture 曾被写成"76 寄存器块只声明 `{at:1, count:6}`"—— 那是不真实的形态**
    /// （活体证据，2026-09-26 返工）：在规则 11（首尾锚定，`last_end = 6 ≠ count = 76`）下
    /// **必被拒**，而旧版本恰把它断言为 `Ok` —— 正因当时的覆盖只有 `points::expand`（不含
    /// 规则 11）才没爆。真实参考形态见 `tests/fixtures/south_pcs_s3b2.yaml`（31 条 `points`
    /// 恰好覆盖 76 寄存器），本 fixture 取其**等价简化形**（够锚定 + 够触发规则 5/9/11 的
    /// 判定路径）。
    fn pcs_block() -> RegBlockConf {
        RegBlockConf {
            name: "pcs_3zone".into(),
            addr: 1000,
            func: RegFunc::Input,
            format: RegFormat::Uint16,
            scale: 1.0,
            count: 76,
            offset: 0.0,
            byte_swap: true,
            points: vec![
                pt(1, 6), // 1000–1005 告警/工作状态
                PointConf {
                    at: 7,
                    format: Some(RegFormat::Int32Scaled),
                    scale: Some(0.1),
                    word_order: WordOrder::LoHi,
                    ..pt(7, 1)
                }, // 1006–1007 32 位点（lo_hi）
                pt(9, 68), // 1008–1075 余下窗口（68 个 16 位槽）
            ],
            read_slice: false,
            interval_ms: None,
        }
    }

    #[test]
    fn south_pcs_config_defaults_are_disabled() {
        let c = SouthPcsConfig::default();
        assert!(
            !c.enabled,
            "缺省必须 disabled —— 既有部署 yaml 零改动即零行为变化"
        );
        assert_eq!(c.interval_ms, DEFAULT_INTERVAL_MS);
    }

    #[test]
    fn south_stations_rejects_pcs_role() {
        let mut c = SouthStationsConfig::default();
        c.stations.push(StationConf {
            id: "pcs".into(),
            role: Role::Pcs,
            port: "/dev/ttyS7".into(),
            protocol: "modbus".into(),
            slave: 1,
            baud_rate: 19200,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![pcs_block()],
        });
        let err = c.validate().unwrap_err();
        assert!(err.contains("south_pcs"), "必须明确指向新段，实际: {err}");
    }

    /// 启用后的段内校验：空 `regs` / `interval_ms` 下界 / `slave` 越界 / 空 `port` /
    /// 零超时 / 块结构非法（规则 P-4：`points::expand` 的**点级**规则）逐条拒绝；合法配置放行。
    /// **块级**规则（5/11/12/14/19）的覆盖见 [`south_pcs_validate_applies_block_level_rules`]。
    #[test]
    fn south_pcs_validate_rejects_dead_or_illegal_configs() {
        let ok = SouthPcsConfig {
            enabled: true,
            regs: vec![pcs_block()],
            ..SouthPcsConfig::default()
        };
        assert_eq!(ok.validate(), Ok(()), "合法 south_pcs 段");

        // `enabled: false` ⇒ 段内一律放行（既有部署零改动的前提）
        let disabled = SouthPcsConfig {
            enabled: false,
            regs: Vec::new(),
            ..SouthPcsConfig::default()
        };
        assert_eq!(disabled.validate(), Ok(()), "未启用则不校验内容");

        let reject = |c: SouthPcsConfig, needle: &str, what: &str| {
            let err = c.validate().expect_err(&format!("{what} 应被拒"));
            assert!(
                err.contains(needle),
                "{what}：Err 应含 {needle:?}，实际 {err}"
            );
        };
        // 空点表 = 采集恒空转的静默死配（原站级规则 3 的落点）
        reject(
            SouthPcsConfig {
                enabled: true,
                ..SouthPcsConfig::default()
            },
            "regs 为空",
            "空 regs",
        );
        // 周期下界（原站级规则 18 的落点）：499 → Err、500 → Ok
        reject(
            SouthPcsConfig {
                enabled: true,
                interval_ms: 499,
                regs: vec![pcs_block()],
                ..SouthPcsConfig::default()
            },
            "须 ≥ 500ms",
            "interval_ms=499",
        );
        assert_eq!(
            SouthPcsConfig {
                enabled: true,
                interval_ms: 500,
                regs: vec![pcs_block()],
                ..SouthPcsConfig::default()
            }
            .validate(),
            Ok(()),
            "interval_ms=500（下界内）"
        );
        // 地址空间：0 / 248 均越界（1..=247）
        for slave in [0u8, 248] {
            reject(
                SouthPcsConfig {
                    enabled: true,
                    slave,
                    regs: vec![pcs_block()],
                    ..SouthPcsConfig::default()
                },
                "slave 越界",
                &format!("slave={slave}"),
            );
        }
        reject(
            SouthPcsConfig {
                enabled: true,
                port: "  ".into(),
                regs: vec![pcs_block()],
                ..SouthPcsConfig::default()
            },
            "port 为空",
            "空 port",
        );
        reject(
            SouthPcsConfig {
                enabled: true,
                response_timeout_ms: 0,
                regs: vec![pcs_block()],
                ..SouthPcsConfig::default()
            },
            "response_timeout_ms 须 > 0",
            "零超时",
        );
        // 规则 P-4（点级）：块结构非法由 `points::expand`（与采集同一函数）拒 —— 点越界
        let mut bad_blk = pcs_block();
        bad_blk.points[0].at = 78; // 点 at=78（块内偏移 77）起算 6 个 → 越出 count=76
        reject(
            SouthPcsConfig {
                enabled: true,
                regs: vec![bad_blk],
                ..SouthPcsConfig::default()
            },
            "越界",
            "点越界块",
        );
    }

    /// **规则 P-4（块级）的常驻锚**（2026-09-26 扩覆盖）：逐条证 5 / 11 / 12 / 14 / 19 在
    /// `south_pcs` 段**真能红**。其中**规则 5 与规则 11 最尖**：
    ///
    /// - 规则 5：YAML 漏写 `scale` ⇒ serde 缺省 `0.0` ⇒ 整块 `raw × 0` **静默全 0**
    ///   （旧覆盖下 `expand` 完全不看 scale ⇒ 本形态会**绿**）；
    /// - 规则 11：`points` 未覆盖满 `count` ⇒ 旧覆盖下同样**绿**（这正是原 fixture 的活体证据）。
    ///
    /// 本用例即"扩覆盖前必绿、扩覆盖后必红"的对照锚，故**常驻**（不止一次性探针）。
    /// 明确不覆盖的两条（15 极大性 / 6·13 依赖 role）**不在此**，见 `validate` 的文档。
    #[test]
    fn south_pcs_validate_applies_block_level_rules() {
        let cfg = |regs: Vec<RegBlockConf>| SouthPcsConfig {
            enabled: true,
            regs,
            ..SouthPcsConfig::default()
        };
        let reject = |regs: Vec<RegBlockConf>, needle: &str, what: &str| {
            let err = cfg(regs).validate().expect_err(&format!("{what} 应被拒"));
            assert!(
                err.contains(needle),
                "{what}：Err 应含 {needle:?}，实际 {err}"
            );
            assert!(
                err.contains("south_pcs") && err.contains("pcs_3zone"),
                "{what}：文案须可定位（段名 + 块名），实际 {err}"
            );
        };

        // 规则 5：块级 `scale: 0.0`（= YAML 漏写 scale 的 serde 缺省）⇒ 拒
        let mut scale0 = pcs_block();
        scale0.scale = 0.0;
        reject(vec![scale0], "须显式 scale>0", "块级 scale=0.0");
        // 规则 5：点级显式 `scale: 0.0`（不被块级掩盖）⇒ 拒
        let mut pt0 = pcs_block();
        pt0.points[1].scale = Some(0.0);
        reject(vec![pt0], "点 at=7", "点级 scale=0.0");

        // 规则 11：`points` 未覆盖满 `count`（76 寄存器只声明 6 点）⇒ 拒（原 fixture 的形态）
        let mut unanchored = pcs_block();
        unanchored.points = vec![pt(1, 6)];
        reject(
            vec![unanchored],
            "首尾锚定",
            "points 未锚定（末末端 ≠ count）",
        );

        // 规则 12：位块位数超上限（2001 > MAX_DISCRETE_BITS=2000）⇒ 拒
        let bits = RegBlockConf {
            name: "pcs_3zone".into(),
            addr: 0,
            func: RegFunc::Discrete,
            format: RegFormat::Uint16,
            scale: 1.0,
            count: 2001,
            offset: 0.0,
            byte_swap: false,
            points: Vec::new(),
            read_slice: false,
            interval_ms: None,
        };
        reject(vec![bits], "超位块上限", "discrete count=2001");

        // 规则 14：同 func 空间跨块区间重叠 ⇒ 拒
        let mut overlap = pcs_block();
        overlap.addr = 1050; // 与 pcs_3zone（1000..1076）重叠
        reject(vec![pcs_block(), overlap], "寄存器区间重叠", "跨块区间重叠");

        // 规则 19：无 `points` 块的 `count` 非 format 宽度整数倍 ⇒ 拒
        let wide = RegBlockConf {
            name: "pcs_3zone".into(),
            addr: 1000,
            func: RegFunc::Input,
            format: RegFormat::Int32Scaled,
            scale: 0.1,
            count: 3, // 3 % 2 != 0
            offset: 0.0,
            byte_swap: false,
            points: Vec::new(),
            read_slice: false,
            interval_ms: None,
        };
        reject(vec![wide], "整数倍", "count 非宽度整数倍");
    }
}
