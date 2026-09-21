//! `south_stations:` 配置段类型（core_config 嵌入用；§10.3）。
//!
//! 定义站级南向统一调度的配置结构：轮询周期、新鲜度门限、站（role/port/slave/parity）
//! 与每站寄存器块（块 = 一次读事务；块内 `points[]` = 逐点换算口径，S3b-2 §11.4.1）。
//! 校验仅限段内（跨段/互斥在 core-bin validate——Task 6）。

use mupc_data_processing::meter_regs::RegFormat;
use serde::{Deserialize, Serialize};

use crate::points::{self, PointKind, PointSpec};

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

/// `pcs` 站轮询周期下界（PRD §9.3.2.2(2) + §9.8.1 末条；设计 §11.5.1 规则 18）。
/// `pcs` 无 `< 5000` 上界（不参与控制决策），只有这条下界——防"误配的超短周期打满总线"。
pub const PCS_MIN_INTERVAL_MS: u64 = 500;

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
    ///    `pcs` 周期下界（规则 18）/ meter_grid·battery 新鲜度上界）
    ///    **+ 既有 `meter_grid` 整组校验**（缺相量块 / `count ≥ 6` / `int32_scaled` 显式
    ///    `scale > 0` / **块名唯一** / `addr > 0` / 区间不重叠——**原地不动，不得后移**）；
    /// ② [`validate_station_regs`]（通用规则 4/5/7/8/9/10/11/12/13/14/19，含点展开）；
    /// ③ 跨站（单站约束计数（规则 2/3）、同口一致性（规则 16）、块落地极大性（规则 15））。
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
                return Err(format!(
                    "south_stations: 站 {} interval_ms 须 > 0",
                    s.id
                ));
            }
            // 口波特率下界/上界：0 会静默穿透同口一致性检查（整口皆 0 时放行），
            // 到 open 才报错；>4_000_000 亦为异常值，一并在此拒。
            if s.baud_rate == 0 || s.baud_rate > 4_000_000 {
                return Err(format!(
                    "south_stations: 站 {} baud_rate 越界: {}（须 1..=4000000）",
                    s.id, s.baud_rate
                ));
            }
            // 规则 18：pcs 站周期下界（PRD §9.3.2.2(2) + §9.8.1 末条；未进 §9.4.3 表，
            // 设计 §11.5.1 #18 补落点）。pcs 无 <5000 上界（不参与控制决策）。
            if s.role == Role::Pcs && s.interval_ms < PCS_MIN_INTERVAL_MS {
                return Err(format!(
                    "south_stations: pcs 站 {} interval_ms={} 须 ≥ {}ms（防超短周期打满总线）",
                    s.id, s.interval_ms, PCS_MIN_INTERVAL_MS
                ));
            }
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
        // ③ 跨站：单站约束计数（规则 2/3——pcs 必填点表）
        let mut grid_seen = false;
        let mut battery_seen = false;
        let mut pcs_seen = false;
        for s in &self.stations {
            match s.role {
                Role::MeterGrid => {
                    if grid_seen {
                        return Err(
                            "south_stations: 至多一个 meter_grid 站（AiIntegrator 单写方约束）".into(),
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
                Role::Pcs => {
                    if pcs_seen {
                        return Err(
                            "south_stations: 至多一个 pcs 站（同设备双站双读、点表冲突）".into(),
                        );
                    }
                    pcs_seen = true;
                    if s.regs.is_empty() {
                        return Err(format!(
                            "south_stations: pcs 站 {} regs 为空（必填点表——空 regs = 站永久 offline 的静默死配）",
                            s.id
                        ));
                    }
                }
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
        self.stations
            .iter()
            .find(|s| s.role == Role::MeterGrid)
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
        if b.func != RegFunc::Discrete && b.points.is_empty() {
            let width = b.format.reg_width() as u16;
            if b.count % width != 0 {
                return Err(format!(
                    "{}regs 块 {} count={} 非 format={:?} 宽度 {} 的整数倍——步进的尾槽装不下一个完整值，会**静默少产点**（设计补落点规则 19）",
                    prefix, b.name, b.count, b.format, width
                ));
            }
        }
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
                check_symbolicity(s.role, &p.metric, b.addr + offset, decode.format, decode.offset)
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
    let mut spans: Vec<(u16, u16)> = pts.iter().map(|p| (p.kind.offset(), p.kind.width())).collect();
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
                    prefix, a.name, b.name, a.func, a.name, a.addr, a.count, b.name, b.addr, b.count
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
    check_symbolicity_row(metric, addr, format, offset, crate::point_table::lookup(role, addr))
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
            assert!(
                w.south_stations.validate().is_err(),
                "slave={bad} 应被拒绝"
            );
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
        let w: Wrapper =
            serde_yaml::from_str(&meter_grid_only_yaml(meter_grid_full_regs_yaml()))
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
    // 配置级用例（对 `POINT_REGS` 的命中/未命中）依赖 §11.4.4 的 618 行登记表，
    // 该表属 **T4**（本 Task 落空表 ⇒ 配置级只覆盖"查不到行 → 放行"）。故此处直接对
    // 判定核心注入登记行，把 ①（来源未登记）与 ②（与登记值不一致）两条钉死。

    use crate::point_table::{PointReg, SymSrc};

    fn row(offset: f64, sym_src: Option<SymSrc>) -> PointReg {
        PointReg {
            role: Role::Battery,
            addr: 116,
            offset,
            sym_src,
        }
    }

    /// ① 命中行、`offset ≠ 0`，而该行 `sym_src` 为空 → 拒（raw 的解释方式无人可考）
    #[test]
    fn symbolicity_rejects_untraceable_offset() {
        let r = row(-1600.0, None);
        let err = check_symbolicity_row("x", 116, RegFormat::Uint16, -1600.0, Some(&r)).unwrap_err();
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
}
