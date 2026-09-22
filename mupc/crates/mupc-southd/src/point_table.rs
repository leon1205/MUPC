//! 点表登记注册表（S3b-2 §11.4.4）。
//!
//! **为什么需要它**：PRD §9.4.3「符号性声明一致性」第 ② 条的可判定性以"校验器内的点表
//! 登记值常量表"（`(role, addr) → {format, scale, offset, 来源}`，由设计阶段按 §9.5 逐点
//! 转写）为前提；查不到该表，第 ② 条与第 ① 条同样"无源可判"。
//!
//! **覆盖范围（§11.4.4）**：按 PRD §9.5 逐点转写 618 点（= §9.4.1 参考配置 n=20 展开后的
//! 点数），其中**消防探测器区**（`fire_det`，地址 17 起、每只 6 寄存器、步进
//! `(n−1)*6+11`）登记为 **6 条"组内语义模板"**（`+0 地址`/`+1 状态`/`+2 数据 1`/`+3 CO`/
//! `+4 VOC`/`+5 H2`），运行期按 `(addr − 17) % 6` 取模板（见 [`lookup_in`]）。
//! 故静态数组 = **510 行**，按 n=20 展开后 == 618 行（校对见
//! `tests/point_table_vs_reference_config.rs`）。
//!
//! **查表键是 `(role, space, addr)` 而不是设计的 `(role, addr)`**（[`AddrSpace`]）：
//! 位地址与寄存器地址是两个独立编址的空间，"同站按功能码空间分别判"出自 **PRD §9.4.2.1
//! 第 6 条 / §9.4.3 的「区间与重叠」行**（"规则 14"这个**编号**出自**设计 §11.5.1 的
//! 落点表第 14 行**，PRD §9.4.3 本身无编号），`hvac` 站两者在同批
//! 地址上真实共存；只用 `(role, addr)` 会互相遮蔽（已上报设计，见 T4 汇报）。
//!
//! **强制口径（§11.4.4，随表生效）**：① `format ∈ {uint16,int16}` 且 `offset ≠ 0`，
//! 而 `lookup` **命中一行**、但该行 `sym_src` 为空 → 拒；② `lookup` 命中且 `offset` 与
//! 登记值不等（含"漏配 → 缺省 0"）→ 拒。`format` / `scale` 与登记值不一致**不强制**
//!（Q-4/Q-16/Q-20 的现场裁定会合法改变它们，强校验会把校准后的正确配置拒在启动期）。
//! **查不到行 → 放行**（§11.4.4 的 P0-2 裁定，理由见 `config.rs::check_symbolicity_row`）。
//!
//! **"入表即产事件"（消防字级信号，§11.4.7.1）**：[`PointReg::signals`] **只登记产事件的
//! 信号**；未登记的位/枚举一律只落 telemetry。消防信号清单逐条对表 PRD §9.5.4 的位定义。

use crate::config::Role;
use mupc_data_processing::meter_regs::RegFormat;

/// 符号性来源（PRD §9.5 前言的三分类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymSrc {
    /// 厂方逐点明写（PCS、空调、ADL400 的 **4 字节功率类/PF**、ADL400 的 **6 个总电能
    /// （0x0000/0x000A/0x0014/0x001E/0x0028/0x0032）与 3 个分相电能（0x0087–0x008C）**
    /// —— 后两类原文备注「**整形**」= 厂方标注的无符号 4 字节量，**不是**工程判断；
    /// 另有 ADL400 的不平衡度 0x0093/0x0094，原文备注「整型」（PRD §9.5.3 逐点来源表））
    Vendor,
    /// 厂方标注 + 推断订正（BMS 的 `UNIT` → `UINT`）
    VendorTypo,
    /// 工程判断（消防、ADL400 的 **2 字节无量类型字样点**：电压/电流/频率/线电压/零序/PT/CT
    /// —— **不含** 0x0093/0x0094 的不平衡度，后者厂方备注「整型」，见 [`SymSrc::Vendor`]）
    Engineer,
}

/// 位点分类（决定是否产出告警事件，见 §11.7.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitClass {
    /// 告警位：**0→1** 跳变产事件（离散位块唯一产事件的类别）
    Alarm,
    /// 普通状态位：只落 telemetry
    State,
    /// 保留位：只落 telemetry（PRD §9.7.6"预留位不产事件"）
    Reserved,
}

/// 登记行的值形态（§11.4.4 的 `RegPointKind`）。
///
/// **不 derive `Eq`**：载荷 `RegFormat`（定义在 `mupc-data-processing::meter_regs`）只派生
/// 了 `PartialEq`；本 crate 不改其派生，故此处也只能到 `PartialEq`（已够查表与断言）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RegPointKind {
    /// 标量（16/32 位，占 1/2 寄存器）
    Scalar(RegFormat),
    /// 离散位（`discrete` 块，`addr` = 位地址）
    Bit(BitClass),
}

/// **地址空间**：寄存器空间（FC03 保持 / FC04 输入）与位空间（FC02 离散输入）。
///
/// 两个空间**地址各自从 0 编址、互不相干**（PRD §9.4.2.1 第 6 条 / §9.4.3 的「区间与重叠」
/// 行明确"同站按功能码空间分别判重叠"；该条在设计 §11.5.1 落点表中的编号是 14 —— 引用时
/// 勿写成"PRD §9.4.3 规则 14"，PRD §9.4.3 的表是无编号表）。
/// 故 `(role, addr)` **不足以**定位一行：`hvac` 站的 `hvac_in`（FC04，寄存器 0/2/3）
/// 与 `hvac_di`（FC02，位 0..30）在同一批地址上**真实共存**。登记表因此按
/// `(role, space, addr)` 索引（设计 §11.4.4 的"`(role, addr)`"在实现上须补 space 维度，
/// 否则 HVAC 的位 0 与寄存器 0 会互相遮蔽 —— 已上报，见 T4 汇报）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrSpace {
    /// 寄存器空间（FC03 / FC04）
    Reg,
    /// 位空间（FC02）
    Bit,
}

impl RegPointKind {
    /// 本行所属地址空间。
    pub fn space(self) -> AddrSpace {
        match self {
            RegPointKind::Scalar(_) => AddrSpace::Reg,
            RegPointKind::Bit(_) => AddrSpace::Bit,
        }
    }
}

/// 字级信号：一个整字（或一位）上的**可跃迁量**，**入表即产事件**。仅消防站使用
///（其状态位/枚举全在保持寄存器整字内，无 `discrete` 块）—— 详见 §11.4.7.1。
///
/// **口径（避免"登记了却不产事件"的歧义）**：`signals` **只登记产事件的信号**；
/// 未登记的位/枚举**一律只落 telemetry**（等价于离散位块的 `State`/`Reserved` 语义）。
/// 故**不设 `class` 字段**（本轮无"登记为 State"的形态；将来若需要，再以新变体扩展）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalSpec {
    /// 信号键（事件名后缀 `<点名>@<信号键>`），**同一行内唯一**
    pub key: &'static str,
    /// 活跃判据
    pub pick: SignalPick,
}

/// 信号的活跃判据（§11.4.7.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalPick {
    /// 位图：整字 & `mask` ≠ 0 视为活跃。`mask` 应为**单一或一组同类位**，不得跨位图混装。
    /// **极性反转位不建信号**（如消防探测器 bit15「1 = 在线 / 0 = 离线」）—— 不为其造判据。
    WordBit {
        /// 位掩码（`1 << n` 或同类位组合）
        mask: u16,
    },
    /// 枚举：整字值 ∈ `active` 视为活跃（活跃值之间跃迁亦产事件，如 1 一级报警 → 2 二级火警）
    WordEnum {
        /// 活跃值集合 `(值, 文案)`
        active: &'static [(u16, &'static str)],
    },
}

impl SignalPick {
    /// 活跃判据求值（§11.4.7.1）：`WordBit` = `字 & mask ≠ 0`；`WordEnum` = `字 ∈ active`。
    ///
    /// **只读整字、不改写遥测值**（事件层只做位/枚举判定，不做字节拆解 —— G-6 的三层边界，§11.10）。
    pub fn is_active(&self, word: u16) -> bool {
        match self {
            SignalPick::WordBit { mask } => word & mask != 0,
            SignalPick::WordEnum { active } => active.iter().any(|(v, _)| *v == word),
        }
    }
}

/// 点表登记行（§11.4.4 全字段）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointReg {
    /// 所属 role
    pub role: Role,
    /// 绝对寄存器地址（`discrete` 块为**位地址**）
    pub addr: u16,
    /// 值形态（标量格式 / 位点分类）
    pub kind: RegPointKind,
    /// 登记标度（**不强制**，见模块头）
    pub scale: f64,
    /// 登记换算偏移（规则 6 ② 的期望值）
    pub offset: f64,
    /// 该点的符号性来源；**`offset ≠ 0` 的标量行必须登记**（否则规则 6 ① 命中也拒）
    pub sym_src: Option<SymSrc>,
    /// 中文名（事件 message / 展示 / RC-1 逐点核对清单用）
    pub label: &'static str,
    /// 字级信号（**入表即产事件**；无信号 = 空切片）
    pub signals: &'static [SignalSpec],
}

// ───────────────────────────── 行构造子（让 510 行的表可读、可核对） ─────────────────────────────

/// 标量行（无信号）。
const fn sc(
    role: Role,
    addr: u16,
    format: RegFormat,
    scale: f64,
    offset: f64,
    sym_src: SymSrc,
    label: &'static str,
) -> PointReg {
    PointReg {
        role,
        addr,
        kind: RegPointKind::Scalar(format),
        scale,
        offset,
        sym_src: Some(sym_src),
        label,
        signals: &[],
    }
}

/// **消防**寄存器行（PRD §9.5.4 全表同一口径：`uint16` / `scale 1.0` / `offset 0` /
/// 来源 = 工程判断）+ 字级信号（**入表即产事件**，见 [`SignalSpec`]）。
const fn fire_sig(addr: u16, label: &'static str, signals: &'static [SignalSpec]) -> PointReg {
    PointReg {
        role: Role::Fire,
        addr,
        kind: RegPointKind::Scalar(RegFormat::Uint16),
        scale: 1.0,
        offset: 0.0,
        sym_src: Some(SymSrc::Engineer),
        label,
        signals,
    }
}

/// **消防**寄存器行（同上，无字级信号 ⇒ 只落 telemetry）。
const fn fire(addr: u16, label: &'static str) -> PointReg {
    PointReg {
        role: Role::Fire,
        addr,
        kind: RegPointKind::Scalar(RegFormat::Uint16),
        scale: 1.0,
        offset: 0.0,
        sym_src: Some(SymSrc::Engineer),
        label,
        signals: &[],
    }
}

/// 位行（`discrete` 块；`addr` = **位地址**）。
const fn bit(role: Role, addr: u16, class: BitClass, label: &'static str) -> PointReg {
    PointReg {
        role,
        addr,
        kind: RegPointKind::Bit(class),
        scale: 0.0,
        offset: 0.0,
        sym_src: None,
        label,
        signals: &[],
    }
}

// ───────────────────────────── 消防字级信号表（§11.4.7.1） ─────────────────────────────

/// 系统状态（addr 4）：4 条故障位 + 2 条灭火动作位。
///
/// **不登记**：bit15 工作模式 / bit12 充电状态 / bit7–0 备电电量（普通状态量与连续量）。
static SIG_FIRE_SYS_STATUS: [SignalSpec; 6] = [
    SignalSpec {
        key: "main_power_fault",
        pick: SignalPick::WordBit { mask: 1 << 14 },
    },
    SignalSpec {
        key: "backup_power_fault",
        pick: SignalPick::WordBit { mask: 1 << 13 },
    },
    SignalSpec {
        key: "drive_circuit_fault",
        pick: SignalPick::WordBit { mask: 1 << 11 },
    },
    SignalSpec {
        key: "pressure_sensor_fault",
        pick: SignalPick::WordBit { mask: 1 << 10 },
    },
    SignalSpec {
        key: "spray_fired",
        pick: SignalPick::WordBit { mask: 1 << 8 },
    },
    SignalSpec {
        key: "valve_open",
        pick: SignalPick::WordBit { mask: 1 << 9 },
    },
];

/// 烟/温/可燃状态（addr 6/7/8）：`mask = 0b11`（bit1 复合探测器触发 + bit0 干接点触发）。
/// **bit2（点型，预留未启用）不纳入 mask**（PRD §9.7.6 明文"采集成点但不产出告警事件"）。
static SIG_FIRE_SMOKE: [SignalSpec; 1] = [SignalSpec {
    key: "smoke_trigger",
    pick: SignalPick::WordBit { mask: 0b11 },
}];
static SIG_FIRE_TEMP: [SignalSpec; 1] = [SignalSpec {
    key: "temp_trigger",
    pick: SignalPick::WordBit { mask: 0b11 },
}];
static SIG_FIRE_COMBUSTIBLE: [SignalSpec; 1] = [SignalSpec {
    key: "combustible_trigger",
    pick: SignalPick::WordBit { mask: 0b11 },
}];

/// 火警状态（addr 9，枚举）：值 3 预留不入 `active`；值 0 工作正常 = 非活跃。
static SIG_FIRE_LEVEL: [SignalSpec; 4] = [
    SignalSpec {
        key: "level1",
        pick: SignalPick::WordEnum {
            active: &[(1, "一级报警")],
        },
    },
    SignalSpec {
        key: "level2",
        pick: SignalPick::WordEnum {
            active: &[(2, "二级火警")],
        },
    },
    SignalSpec {
        key: "emg_start",
        pick: SignalPick::WordEnum {
            active: &[(4, "紧急启动")],
        },
    },
    SignalSpec {
        key: "emg_stop",
        pick: SignalPick::WordEnum {
            active: &[(5, "紧急停止")],
        },
    },
];

/// 探测器状态（addr 12 / 探测器组 `+1`）：**只取 bit12 报警总状态与 bit14 故障总状态**。
///
/// **不登记**：bit15 通信状态（**0 = 离线，与其余位极性相反**）/ bit13 电磁阀 / bit10–11 反馈
/// 输入（极性反转位与反馈位无明确告警语义 ⇒ 不猜、不造判据）/ bit0–4 传感器细分
///（逐传感器事件会让事件源数 ×5，且 bit12 已覆盖"该探测器报警"）。
static SIG_FIRE_DETECTOR: [SignalSpec; 2] = [
    SignalSpec {
        key: "alarm",
        pick: SignalPick::WordBit { mask: 1 << 12 },
    },
    SignalSpec {
        key: "fault",
        pick: SignalPick::WordBit { mask: 1 << 14 },
    },
];

// ───────────────────────────── POINT_REGS（510 静态行） ─────────────────────────────

/// 逐点登记表（按 PRD §9.5 转写；探测器区为 6 条组内模板，见模块头）。
#[rustfmt::skip]
pub const POINT_REGS: &[PointReg] = &[
    // ══════════════ 1) 储能主控模块 BMS（`Role::Battery`，345 点）══════════════
    // ── A. 输入寄存器 FC04：`bms_io`（100–130，31 点）──
    sc(Role::Battery, 100, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇电池簇状态（枚举：0x00 初始/0x01 充电/0x02 放电/0x03 待机/0x08 故障）"),
    sc(Role::Battery, 101, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "簇允许充电最大功率 kW"),
    sc(Role::Battery, 102, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "簇允许放电最大功率 kW"),
    sc(Role::Battery, 103, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "簇允许充电最大电压 V"),
    sc(Role::Battery, 104, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "簇允许放电最大电压 V"),
    sc(Role::Battery, 105, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "簇允许充电最大电流 A"),
    sc(Role::Battery, 106, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "簇允许放电最大电流 A"),
    sc(Role::Battery, 107, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控 DI1（主正继电器反馈）"),
    sc(Role::Battery, 108, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控 DI2（主负继电器反馈）"),
    sc(Role::Battery, 109, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控 DI3（预充继电器反馈）"),
    sc(Role::Battery, 110, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控 DI4（预留）"),
    sc(Role::Battery, 111, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控 DI5（断路器反馈）"),
    sc(Role::Battery, 112, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控 DI6（预留）"),
    sc(Role::Battery, 113, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控 DI7（预留）"),
    sc(Role::Battery, 114, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控 DI8（预留）"),
    sc(Role::Battery, 115, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "簇组电压 V"),
    sc(Role::Battery, 116, RegFormat::Uint16, 0.1, -1600.0, SymSrc::VendorTypo, "簇组电流 A（无符号编码 + 负偏移平移；Q-20 现场判别）"),
    sc(Role::Battery, 117, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇组模块温度 ℃"),
    sc(Role::Battery, 118, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇组 SOC %（控制输入；点名 `soc`）"),
    sc(Role::Battery, 119, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇组 SOH %"),
    sc(Role::Battery, 120, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇组绝缘电阻 kΩ"),
    sc(Role::Battery, 121, RegFormat::Uint16, 0.001, 0.0, SymSrc::VendorTypo, "簇平均单体电压 V"),
    sc(Role::Battery, 122, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇平均单体温度 ℃"),
    sc(Role::Battery, 123, RegFormat::Uint16, 0.001, 0.0, SymSrc::VendorTypo, "簇最高单体电压 V"),
    sc(Role::Battery, 124, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇最高单体电压对应点（整字，Q-18）"),
    sc(Role::Battery, 125, RegFormat::Uint16, 0.001, 0.0, SymSrc::VendorTypo, "簇最低单体电压 V"),
    sc(Role::Battery, 126, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇最低单体电压对应点（整字，Q-18）"),
    sc(Role::Battery, 127, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇最高单体温度 ℃"),
    sc(Role::Battery, 128, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇最高单体温度对应点（整字，Q-18）"),
    sc(Role::Battery, 129, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇最低单体温度 ℃"),
    sc(Role::Battery, 130, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇最低单体温度对应点（整字，Q-18）"),
    // ── `bms_energy`（139–157，10 点；140/142/…/156 为 32 位高半字，非点）──
    sc(Role::Battery, 139, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::VendorTypo, "簇累计充电电量 kWh"),
    sc(Role::Battery, 141, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::VendorTypo, "簇累计放电电量 kWh"),
    sc(Role::Battery, 143, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::VendorTypo, "簇单次累计充电电量 kWh"),
    sc(Role::Battery, 145, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::VendorTypo, "簇单次累计放电电量 kWh"),
    sc(Role::Battery, 147, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::VendorTypo, "簇可充电量 kWh"),
    sc(Role::Battery, 149, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::VendorTypo, "簇可放电量 kWh"),
    sc(Role::Battery, 151, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇最高单体温升 ℃"),
    sc(Role::Battery, 153, RegFormat::Uint16, 0.001, 0.0, SymSrc::VendorTypo, "簇最高单体最高电压变化 V"),
    sc(Role::Battery, 155, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇最高单体极柱温度 ℃"),
    sc(Role::Battery, 157, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇最低单体极柱温度 ℃"),
    // ── `bms_meta`（181–189，8 点；188 为对应点，不采）──
    sc(Role::Battery, 181, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "主控程序版本号（RC-12 核对依据）"),
    sc(Role::Battery, 182, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "从控数量 个"),
    sc(Role::Battery, 183, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇组 SOE %"),
    sc(Role::Battery, 184, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "单体温度极差 ℃"),
    sc(Role::Battery, 185, RegFormat::Uint16, 0.001, 0.0, SymSrc::VendorTypo, "单体电压极差 V"),
    sc(Role::Battery, 186, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "簇实时充放电功率 kW（残余风险最高项，Q-20 首日核对）"),
    sc(Role::Battery, 187, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "PACK 组压最高电压 V"),
    sc(Role::Battery, 189, RegFormat::Uint16, 0.1, 0.0, SymSrc::VendorTypo, "PACK 组压最低电压 V"),
    // ── `bms_term`（2991–2994，4 点；块级 offset −40）──
    sc(Role::Battery, 2991, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇端子温度 001（箱体 T1）℃"),
    sc(Role::Battery, 2992, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇端子温度 002（箱体 T2）℃"),
    sc(Role::Battery, 2993, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇端子温度 003（箱体 T3）℃"),
    sc(Role::Battery, 2994, RegFormat::Uint16, 1.0, -40.0, SymSrc::VendorTypo, "簇端子温度 004（箱体 T4）℃"),
    // ── `bms_cap`（4000–4005，4 点；4001/4003 不采，Q-16 现场裁定）──
    sc(Role::Battery, 4000, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇累计充电容量 Ah（Q-16：16/32 位未明确）"),
    sc(Role::Battery, 4002, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇累计放电容量 Ah"),
    sc(Role::Battery, 4004, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇单次累计充电容量 Ah"),
    sc(Role::Battery, 4005, RegFormat::Uint16, 1.0, 0.0, SymSrc::VendorTypo, "簇单次累计放电容量 Ah"),
    // ── B. 离散输入 FC02：`bms_alarm`（位 200–487，288 点）──
    bit(Role::Battery, 200, BitClass::Reserved, "簇主控通讯失联（保留）"),
    bit(Role::Battery, 201, BitClass::Alarm, "簇端电压欠压·轻"),
    bit(Role::Battery, 202, BitClass::Alarm, "簇端电压欠压·中"),
    bit(Role::Battery, 203, BitClass::Alarm, "簇端电压欠压·重"),
    bit(Role::Battery, 204, BitClass::Alarm, "簇端电压过压·轻"),
    bit(Role::Battery, 205, BitClass::Alarm, "簇端电压过压·中"),
    bit(Role::Battery, 206, BitClass::Alarm, "簇端电压过压·重"),
    bit(Role::Battery, 207, BitClass::Alarm, "簇端充电电流·轻"),
    bit(Role::Battery, 208, BitClass::Alarm, "簇端充电电流·中"),
    bit(Role::Battery, 209, BitClass::Alarm, "簇端充电电流·重"),
    bit(Role::Battery, 210, BitClass::Alarm, "簇端放电电流·轻"),
    bit(Role::Battery, 211, BitClass::Alarm, "簇端放电电流·中"),
    bit(Role::Battery, 212, BitClass::Alarm, "簇端放电电流·重"),
    bit(Role::Battery, 213, BitClass::Alarm, "簇单体欠压·轻"),
    bit(Role::Battery, 214, BitClass::Alarm, "簇单体欠压·中"),
    bit(Role::Battery, 215, BitClass::Alarm, "簇单体欠压·重"),
    bit(Role::Battery, 216, BitClass::Alarm, "簇单体过压·轻"),
    bit(Role::Battery, 217, BitClass::Alarm, "簇单体过压·中"),
    bit(Role::Battery, 218, BitClass::Alarm, "簇单体过压·重"),
    bit(Role::Battery, 219, BitClass::Alarm, "簇单体欠温·轻"),
    bit(Role::Battery, 220, BitClass::Alarm, "簇单体欠温·中"),
    bit(Role::Battery, 221, BitClass::Alarm, "簇单体欠温·重"),
    bit(Role::Battery, 222, BitClass::Alarm, "簇单体过温·轻"),
    bit(Role::Battery, 223, BitClass::Alarm, "簇单体过温·中"),
    bit(Role::Battery, 224, BitClass::Alarm, "簇单体过温·重"),
    bit(Role::Battery, 225, BitClass::Alarm, "簇 SOC 低·轻"),
    bit(Role::Battery, 226, BitClass::Alarm, "簇 SOC 低·中"),
    bit(Role::Battery, 227, BitClass::Alarm, "簇 SOC 低·重"),
    bit(Role::Battery, 228, BitClass::Alarm, "簇 SOH 低·轻"),
    bit(Role::Battery, 229, BitClass::Alarm, "簇 SOH 低·中"),
    bit(Role::Battery, 230, BitClass::Alarm, "簇 SOH 低·重"),
    bit(Role::Battery, 231, BitClass::Alarm, "簇单体压差·轻"),
    bit(Role::Battery, 232, BitClass::Alarm, "簇单体压差·中"),
    bit(Role::Battery, 233, BitClass::Alarm, "簇单体压差·重"),
    bit(Role::Battery, 234, BitClass::Alarm, "簇单体温差·轻"),
    bit(Role::Battery, 235, BitClass::Alarm, "簇单体温差·中"),
    bit(Role::Battery, 236, BitClass::Alarm, "簇单体温差·重"),
    bit(Role::Battery, 237, BitClass::Alarm, "簇从控 1 通讯失联"),
    bit(Role::Battery, 238, BitClass::Alarm, "簇从控 2 通讯失联"),
    bit(Role::Battery, 239, BitClass::Alarm, "簇从控 3 通讯失联"),
    bit(Role::Battery, 240, BitClass::Alarm, "簇从控 4 通讯失联"),
    bit(Role::Battery, 241, BitClass::Alarm, "簇从控 5 通讯失联"),
    bit(Role::Battery, 242, BitClass::Alarm, "簇从控 6 通讯失联"),
    bit(Role::Battery, 243, BitClass::Alarm, "簇从控 7 通讯失联"),
    bit(Role::Battery, 244, BitClass::Alarm, "簇从控 8 通讯失联"),
    bit(Role::Battery, 245, BitClass::Alarm, "簇从控 9 通讯失联"),
    bit(Role::Battery, 246, BitClass::Alarm, "簇从控 10 通讯失联"),
    bit(Role::Battery, 247, BitClass::Alarm, "簇从控 11 通讯失联"),
    bit(Role::Battery, 248, BitClass::Alarm, "簇从控 12 通讯失联"),
    bit(Role::Battery, 249, BitClass::Alarm, "簇从控 13 通讯失联"),
    bit(Role::Battery, 250, BitClass::Alarm, "簇从控 14 通讯失联"),
    bit(Role::Battery, 251, BitClass::Alarm, "簇从控 15 通讯失联"),
    bit(Role::Battery, 252, BitClass::Alarm, "簇从控 16 通讯失联"),
    bit(Role::Battery, 253, BitClass::Alarm, "簇从控 17 通讯失联"),
    bit(Role::Battery, 254, BitClass::Alarm, "簇从控 18 通讯失联"),
    bit(Role::Battery, 255, BitClass::Alarm, "簇从控 19 通讯失联"),
    bit(Role::Battery, 256, BitClass::Alarm, "簇从控 20 通讯失联"),
    bit(Role::Battery, 257, BitClass::Alarm, "簇从控 21 通讯失联"),
    bit(Role::Battery, 258, BitClass::Alarm, "簇从控 22 通讯失联"),
    bit(Role::Battery, 259, BitClass::Alarm, "簇从控 23 通讯失联"),
    bit(Role::Battery, 260, BitClass::Alarm, "簇从控 24 通讯失联"),
    bit(Role::Battery, 261, BitClass::Alarm, "簇从控 25 通讯失联"),
    bit(Role::Battery, 262, BitClass::Alarm, "簇从控 26 通讯失联"),
    bit(Role::Battery, 263, BitClass::Alarm, "簇从控 27 通讯失联"),
    bit(Role::Battery, 264, BitClass::Alarm, "簇从控 28 通讯失联"),
    bit(Role::Battery, 265, BitClass::Alarm, "簇从控 29 通讯失联"),
    bit(Role::Battery, 266, BitClass::Alarm, "簇从控 30 通讯失联"),
    bit(Role::Battery, 267, BitClass::Alarm, "簇从控 31 通讯失联"),
    bit(Role::Battery, 268, BitClass::Alarm, "簇从控 32 通讯失联"),
    bit(Role::Battery, 269, BitClass::Alarm, "簇从控 33 通讯失联"),
    bit(Role::Battery, 270, BitClass::Alarm, "簇从控 34 通讯失联"),
    bit(Role::Battery, 271, BitClass::Alarm, "簇从控 35 通讯失联"),
    bit(Role::Battery, 272, BitClass::Alarm, "簇从控 36 通讯失联"),
    bit(Role::Battery, 273, BitClass::Alarm, "簇从控 37 通讯失联"),
    bit(Role::Battery, 274, BitClass::Alarm, "簇从控 38 通讯失联"),
    bit(Role::Battery, 275, BitClass::Alarm, "簇从控 39 通讯失联"),
    bit(Role::Battery, 276, BitClass::Alarm, "簇从控 40 通讯失联"),
    bit(Role::Battery, 277, BitClass::Alarm, "簇端子（箱体）温度过高·轻"),
    bit(Role::Battery, 278, BitClass::Alarm, "簇端子（箱体）温度过高·中"),
    bit(Role::Battery, 279, BitClass::Alarm, "簇端子（箱体）温度过高·重"),
    bit(Role::Battery, 280, BitClass::Alarm, "簇 pack 电压过高·轻"),
    bit(Role::Battery, 281, BitClass::Alarm, "簇 pack 电压过高·中"),
    bit(Role::Battery, 282, BitClass::Alarm, "簇 pack 电压过高·重"),
    bit(Role::Battery, 283, BitClass::Alarm, "簇 pack 电压过低·轻"),
    bit(Role::Battery, 284, BitClass::Alarm, "簇 pack 电压过低·中"),
    bit(Role::Battery, 285, BitClass::Alarm, "簇 pack 电压过低·重"),
    bit(Role::Battery, 286, BitClass::Alarm, "簇单体电压采集故障"),
    bit(Role::Battery, 287, BitClass::Alarm, "簇单体温度采集故障"),
    bit(Role::Battery, 288, BitClass::State, "簇初始状态"),
    bit(Role::Battery, 289, BitClass::State, "簇充电"),
    bit(Role::Battery, 290, BitClass::State, "簇放电"),
    bit(Role::Battery, 291, BitClass::State, "簇就绪"),
    bit(Role::Battery, 292, BitClass::Reserved, "簇维护（保留）"),
    bit(Role::Battery, 293, BitClass::State, "簇禁充"),
    bit(Role::Battery, 294, BitClass::State, "簇禁放"),
    bit(Role::Battery, 295, BitClass::State, "簇充放禁止"),
    bit(Role::Battery, 296, BitClass::State, "簇故障"),
    bit(Role::Battery, 297, BitClass::Reserved, "簇测试模式（保留）"),
    bit(Role::Battery, 298, BitClass::State, "簇高压箱状态"),
    bit(Role::Battery, 299, BitClass::Alarm, "簇从控 DI 告警状态（风扇/气溶胶/MSD）"),
    bit(Role::Battery, 300, BitClass::Alarm, "簇从控 1 DI 定制告警"),
    bit(Role::Battery, 301, BitClass::Alarm, "簇从控 2 DI 定制告警"),
    bit(Role::Battery, 302, BitClass::Alarm, "簇从控 3 DI 定制告警"),
    bit(Role::Battery, 303, BitClass::Alarm, "簇从控 4 DI 定制告警"),
    bit(Role::Battery, 304, BitClass::Alarm, "簇从控 5 DI 定制告警"),
    bit(Role::Battery, 305, BitClass::Alarm, "簇从控 6 DI 定制告警"),
    bit(Role::Battery, 306, BitClass::Alarm, "簇从控 7 DI 定制告警"),
    bit(Role::Battery, 307, BitClass::Alarm, "簇从控 8 DI 定制告警"),
    bit(Role::Battery, 308, BitClass::Alarm, "簇从控 9 DI 定制告警"),
    bit(Role::Battery, 309, BitClass::Alarm, "簇从控 10 DI 定制告警"),
    bit(Role::Battery, 310, BitClass::Alarm, "簇从控 11 DI 定制告警"),
    bit(Role::Battery, 311, BitClass::Alarm, "簇从控 12 DI 定制告警"),
    bit(Role::Battery, 312, BitClass::Alarm, "簇从控 13 DI 定制告警"),
    bit(Role::Battery, 313, BitClass::Alarm, "簇从控 14 DI 定制告警"),
    bit(Role::Battery, 314, BitClass::Alarm, "簇从控 15 DI 定制告警"),
    bit(Role::Battery, 315, BitClass::Alarm, "簇从控 16 DI 定制告警"),
    bit(Role::Battery, 316, BitClass::Alarm, "簇从控 17 DI 定制告警"),
    bit(Role::Battery, 317, BitClass::Alarm, "簇从控 18 DI 定制告警"),
    bit(Role::Battery, 318, BitClass::Alarm, "簇从控 19 DI 定制告警"),
    bit(Role::Battery, 319, BitClass::Alarm, "簇从控 20 DI 定制告警"),
    bit(Role::Battery, 320, BitClass::Alarm, "簇从控 21 DI 定制告警"),
    bit(Role::Battery, 321, BitClass::Alarm, "簇从控 22 DI 定制告警"),
    bit(Role::Battery, 322, BitClass::Alarm, "簇从控 23 DI 定制告警"),
    bit(Role::Battery, 323, BitClass::Alarm, "簇从控 24 DI 定制告警"),
    bit(Role::Battery, 324, BitClass::Alarm, "簇从控 25 DI 定制告警"),
    bit(Role::Battery, 325, BitClass::Alarm, "簇从控 26 DI 定制告警"),
    bit(Role::Battery, 326, BitClass::Alarm, "簇从控 27 DI 定制告警"),
    bit(Role::Battery, 327, BitClass::Alarm, "簇从控 28 DI 定制告警"),
    bit(Role::Battery, 328, BitClass::Alarm, "簇从控 29 DI 定制告警"),
    bit(Role::Battery, 329, BitClass::Alarm, "簇从控 30 DI 定制告警"),
    bit(Role::Battery, 330, BitClass::Alarm, "簇从控 31 DI 定制告警"),
    bit(Role::Battery, 331, BitClass::Alarm, "簇从控 32 DI 定制告警"),
    bit(Role::Battery, 332, BitClass::Alarm, "簇从控 33 DI 定制告警"),
    bit(Role::Battery, 333, BitClass::Alarm, "簇从控 34 DI 定制告警"),
    bit(Role::Battery, 334, BitClass::Alarm, "簇从控 35 DI 定制告警"),
    bit(Role::Battery, 335, BitClass::Alarm, "簇从控 36 DI 定制告警"),
    bit(Role::Battery, 336, BitClass::Alarm, "簇从控 37 DI 定制告警"),
    bit(Role::Battery, 337, BitClass::Alarm, "簇从控 38 DI 定制告警"),
    bit(Role::Battery, 338, BitClass::Alarm, "簇从控 39 DI 定制告警"),
    bit(Role::Battery, 339, BitClass::Alarm, "簇从控 40 DI 定制告警"),
    bit(Role::Battery, 340, BitClass::Alarm, "簇单体充电过温·轻"),
    bit(Role::Battery, 341, BitClass::Alarm, "簇单体充电过温·中"),
    bit(Role::Battery, 342, BitClass::Alarm, "簇单体充电过温·重"),
    bit(Role::Battery, 343, BitClass::Alarm, "簇单体充电欠温·轻"),
    bit(Role::Battery, 344, BitClass::Alarm, "簇单体充电欠温·中"),
    bit(Role::Battery, 345, BitClass::Alarm, "簇单体充电欠温·重"),
    bit(Role::Battery, 346, BitClass::Alarm, "簇单体放电过温·轻"),
    bit(Role::Battery, 347, BitClass::Alarm, "簇单体放电过温·中"),
    bit(Role::Battery, 348, BitClass::Alarm, "簇单体放电过温·重"),
    bit(Role::Battery, 349, BitClass::Alarm, "簇单体放电欠温·轻"),
    bit(Role::Battery, 350, BitClass::Alarm, "簇单体放电欠温·中"),
    bit(Role::Battery, 351, BitClass::Alarm, "簇单体放电欠温·重"),
    bit(Role::Battery, 352, BitClass::Alarm, "簇单体温升过大·轻"),
    bit(Role::Battery, 353, BitClass::Alarm, "簇单体温升过大·中"),
    bit(Role::Battery, 354, BitClass::Alarm, "簇单体温升过大·重"),
    bit(Role::Battery, 355, BitClass::Alarm, "簇单体极柱温度过温·轻"),
    bit(Role::Battery, 356, BitClass::Alarm, "簇单体极柱温度过温·中"),
    bit(Role::Battery, 357, BitClass::Alarm, "簇单体极柱温度过温·重"),
    bit(Role::Battery, 358, BitClass::Alarm, "簇单体极柱温度欠温·轻"),
    bit(Role::Battery, 359, BitClass::Alarm, "簇单体极柱温度欠温·中"),
    bit(Role::Battery, 360, BitClass::Alarm, "簇单体极柱温度欠温·重"),
    bit(Role::Battery, 361, BitClass::Alarm, "簇单体电压变化过大·轻"),
    bit(Role::Battery, 362, BitClass::Alarm, "簇单体电压变化过大·中"),
    bit(Role::Battery, 363, BitClass::Alarm, "簇单体电压变化过大·重"),
    bit(Role::Battery, 364, BitClass::Reserved, "定制保留 001（无消费方）"),
    bit(Role::Battery, 365, BitClass::Reserved, "定制保留 002（无消费方）"),
    bit(Role::Battery, 366, BitClass::Reserved, "定制保留 003（无消费方）"),
    bit(Role::Battery, 367, BitClass::Reserved, "定制保留 004（无消费方）"),
    bit(Role::Battery, 368, BitClass::Reserved, "定制保留 005（无消费方）"),
    bit(Role::Battery, 369, BitClass::Reserved, "定制保留 006（无消费方）"),
    bit(Role::Battery, 370, BitClass::Reserved, "定制保留 007（无消费方）"),
    bit(Role::Battery, 371, BitClass::Reserved, "定制保留 008（无消费方）"),
    bit(Role::Battery, 372, BitClass::Reserved, "定制保留 009（无消费方）"),
    bit(Role::Battery, 373, BitClass::Reserved, "定制保留 010（无消费方）"),
    bit(Role::Battery, 374, BitClass::Reserved, "定制保留 011（无消费方）"),
    bit(Role::Battery, 375, BitClass::Reserved, "定制保留 012（无消费方）"),
    bit(Role::Battery, 376, BitClass::Reserved, "定制保留 013（无消费方）"),
    bit(Role::Battery, 377, BitClass::Reserved, "定制保留 014（无消费方）"),
    bit(Role::Battery, 378, BitClass::Reserved, "定制保留 015（无消费方）"),
    bit(Role::Battery, 379, BitClass::Reserved, "定制保留 016（无消费方）"),
    bit(Role::Battery, 380, BitClass::Reserved, "定制保留 017（无消费方）"),
    bit(Role::Battery, 381, BitClass::Reserved, "定制保留 018（无消费方）"),
    bit(Role::Battery, 382, BitClass::Reserved, "定制保留 019（无消费方）"),
    bit(Role::Battery, 383, BitClass::Reserved, "定制保留 020（无消费方）"),
    bit(Role::Battery, 384, BitClass::Reserved, "定制保留 021（无消费方）"),
    bit(Role::Battery, 385, BitClass::Reserved, "定制保留 022（无消费方）"),
    bit(Role::Battery, 386, BitClass::Reserved, "定制保留 023（无消费方）"),
    bit(Role::Battery, 387, BitClass::Reserved, "定制保留 024（无消费方）"),
    bit(Role::Battery, 388, BitClass::Reserved, "定制保留 025（无消费方）"),
    bit(Role::Battery, 389, BitClass::Reserved, "定制保留 026（无消费方）"),
    bit(Role::Battery, 390, BitClass::Reserved, "定制保留 027（无消费方）"),
    bit(Role::Battery, 391, BitClass::Reserved, "定制保留 028（无消费方）"),
    bit(Role::Battery, 392, BitClass::Reserved, "定制保留 029（无消费方）"),
    bit(Role::Battery, 393, BitClass::Reserved, "定制保留 030（无消费方）"),
    bit(Role::Battery, 394, BitClass::Reserved, "定制保留 031（无消费方）"),
    bit(Role::Battery, 395, BitClass::Reserved, "定制保留 032（无消费方）"),
    bit(Role::Battery, 396, BitClass::Reserved, "定制保留 033（无消费方）"),
    bit(Role::Battery, 397, BitClass::Reserved, "定制保留 034（无消费方）"),
    bit(Role::Battery, 398, BitClass::Reserved, "定制保留 035（无消费方）"),
    bit(Role::Battery, 399, BitClass::Reserved, "定制保留 036（无消费方）"),
    bit(Role::Battery, 400, BitClass::Reserved, "定制保留 037（无消费方）"),
    bit(Role::Battery, 401, BitClass::Reserved, "定制保留 038（无消费方）"),
    bit(Role::Battery, 402, BitClass::Reserved, "定制保留 039（无消费方）"),
    bit(Role::Battery, 403, BitClass::Reserved, "定制保留 040（无消费方）"),
    bit(Role::Battery, 404, BitClass::Reserved, "定制保留 041（无消费方）"),
    bit(Role::Battery, 405, BitClass::Reserved, "定制保留 042（无消费方）"),
    bit(Role::Battery, 406, BitClass::Reserved, "定制保留 043（无消费方）"),
    bit(Role::Battery, 407, BitClass::Reserved, "定制保留 044（无消费方）"),
    bit(Role::Battery, 408, BitClass::Reserved, "定制保留 045（无消费方）"),
    bit(Role::Battery, 409, BitClass::Reserved, "定制保留 046（无消费方）"),
    bit(Role::Battery, 410, BitClass::Reserved, "定制保留 047（无消费方）"),
    bit(Role::Battery, 411, BitClass::Reserved, "定制保留 048（无消费方）"),
    bit(Role::Battery, 412, BitClass::Reserved, "定制保留 049（无消费方）"),
    bit(Role::Battery, 413, BitClass::Reserved, "定制保留 050（无消费方）"),
    bit(Role::Battery, 414, BitClass::Reserved, "定制保留 051（无消费方）"),
    bit(Role::Battery, 415, BitClass::Reserved, "定制保留 052（无消费方）"),
    bit(Role::Battery, 416, BitClass::Reserved, "定制保留 053（无消费方）"),
    bit(Role::Battery, 417, BitClass::Reserved, "定制保留 054（无消费方）"),
    bit(Role::Battery, 418, BitClass::Reserved, "定制保留 055（无消费方）"),
    bit(Role::Battery, 419, BitClass::Reserved, "定制保留 056（无消费方）"),
    bit(Role::Battery, 420, BitClass::Reserved, "定制保留 057（无消费方）"),
    bit(Role::Battery, 421, BitClass::Reserved, "定制保留 058（无消费方）"),
    bit(Role::Battery, 422, BitClass::Reserved, "定制保留 059（无消费方）"),
    bit(Role::Battery, 423, BitClass::Reserved, "定制保留 060（无消费方）"),
    // 424–426：一级/二级/三级告警（PRD §9.5.1 B 表标注"一级报警与保护标识（必须采集）"）
    bit(Role::Battery, 424, BitClass::Alarm, "簇一级告警"),
    bit(Role::Battery, 425, BitClass::Alarm, "簇二级告警"),
    bit(Role::Battery, 426, BitClass::Alarm, "簇三级告警"),
    bit(Role::Battery, 427, BitClass::Alarm, "簇端子（箱体）温度过低·轻"),
    bit(Role::Battery, 428, BitClass::Alarm, "簇端子（箱体）温度过低·中"),
    bit(Role::Battery, 429, BitClass::Alarm, "簇端子（箱体）温度过低·重"),
    bit(Role::Battery, 430, BitClass::Alarm, "MOS 过温·轻"),
    bit(Role::Battery, 431, BitClass::Alarm, "MOS 过温·中"),
    bit(Role::Battery, 432, BitClass::Alarm, "MOS 过温·重"),
    bit(Role::Battery, 433, BitClass::Alarm, "MOS 欠温·轻"),
    bit(Role::Battery, 434, BitClass::Alarm, "MOS 欠温·中"),
    bit(Role::Battery, 435, BitClass::Alarm, "MOS 欠温·重"),
    bit(Role::Battery, 436, BitClass::Alarm, "簇 SOE 低·轻"),
    bit(Role::Battery, 437, BitClass::Alarm, "簇 SOE 低·中"),
    bit(Role::Battery, 438, BitClass::Alarm, "簇 SOE 低·重"),
    bit(Role::Battery, 439, BitClass::Alarm, "簇单体正极柱温度过温·轻"),
    bit(Role::Battery, 440, BitClass::Alarm, "簇单体正极柱温度过温·中"),
    bit(Role::Battery, 441, BitClass::Alarm, "簇单体正极柱温度过温·重"),
    bit(Role::Battery, 442, BitClass::Alarm, "簇单体正极柱温度欠温·轻"),
    bit(Role::Battery, 443, BitClass::Alarm, "簇单体正极柱温度欠温·中"),
    bit(Role::Battery, 444, BitClass::Alarm, "簇单体正极柱温度欠温·重"),
    bit(Role::Battery, 445, BitClass::Alarm, "簇单体负极柱温度过温·轻"),
    bit(Role::Battery, 446, BitClass::Alarm, "簇单体负极柱温度过温·中"),
    bit(Role::Battery, 447, BitClass::Alarm, "簇单体负极柱温度过温·重"),
    bit(Role::Battery, 448, BitClass::Alarm, "簇单体负极柱温度欠温·轻"),
    bit(Role::Battery, 449, BitClass::Alarm, "簇单体负极柱温度欠温·中"),
    bit(Role::Battery, 450, BitClass::Alarm, "簇单体负极柱温度欠温·重"),
    bit(Role::Battery, 451, BitClass::Alarm, "簇绝缘检测低·轻"),
    bit(Role::Battery, 452, BitClass::Alarm, "簇绝缘检测低·中"),
    bit(Role::Battery, 453, BitClass::Alarm, "簇绝缘检测低·重"),
    bit(Role::Battery, 454, BitClass::Alarm, "总正继电器粘连故障"),
    bit(Role::Battery, 455, BitClass::Alarm, "总负继电器粘连故障"),
    bit(Role::Battery, 456, BitClass::Alarm, "预充继电器粘连故障"),
    bit(Role::Battery, 457, BitClass::Alarm, "风扇继电器粘连故障"),
    bit(Role::Battery, 458, BitClass::Alarm, "休眠继电器粘连故障"),
    bit(Role::Battery, 459, BitClass::Alarm, "断路器粘连故障"),
    bit(Role::Battery, 460, BitClass::Alarm, "AFE 故障"),
    bit(Role::Battery, 461, BitClass::Alarm, "单体温度短路故障"),
    bit(Role::Battery, 462, BitClass::Alarm, "单体温度断路故障"),
    bit(Role::Battery, 463, BitClass::Alarm, "MOS 温度故障"),
    bit(Role::Battery, 464, BitClass::Alarm, "均衡 MOS 故障"),
    bit(Role::Battery, 465, BitClass::Alarm, "从控通讯故障"),
    bit(Role::Battery, 466, BitClass::Alarm, "从控供电故障"),
    bit(Role::Battery, 467, BitClass::Alarm, "从控风扇故障"),
    bit(Role::Battery, 468, BitClass::Alarm, "从控程序升级故障"),
    bit(Role::Battery, 469, BitClass::Alarm, "从控参数设置故障"),
    bit(Role::Battery, 470, BitClass::Alarm, "从控供电过压故障·轻"),
    bit(Role::Battery, 471, BitClass::Alarm, "从控供电过压故障·中"),
    bit(Role::Battery, 472, BitClass::Alarm, "从控供电过压故障·重"),
    bit(Role::Battery, 473, BitClass::Alarm, "主控供电故障"),
    bit(Role::Battery, 474, BitClass::Alarm, "主控程序升级故障"),
    bit(Role::Battery, 475, BitClass::Alarm, "主控供电过压故障·轻"),
    bit(Role::Battery, 476, BitClass::Alarm, "主控供电过压故障·中"),
    bit(Role::Battery, 477, BitClass::Alarm, "主控供电过压故障·重"),
    bit(Role::Battery, 478, BitClass::Alarm, "EEPROM 存储故障"),
    bit(Role::Battery, 479, BitClass::Alarm, "地址编码故障"),
    bit(Role::Battery, 480, BitClass::Alarm, "CAN 电流采集故障"),
    bit(Role::Battery, 481, BitClass::Alarm, "485-1 通讯失联故障"),
    bit(Role::Battery, 482, BitClass::Alarm, "485-2 通讯失联故障（RC-11 总线归属旁证）"),
    bit(Role::Battery, 483, BitClass::Alarm, "PCS 失联故障（仅事件，不参与联锁）"),
    bit(Role::Battery, 484, BitClass::Reserved, "保留（484，无消费方）"),
    bit(Role::Battery, 485, BitClass::Reserved, "保留（485，无消费方）"),
    bit(Role::Battery, 486, BitClass::Reserved, "保留（486，无消费方）"),
    bit(Role::Battery, 487, BitClass::Reserved, "保留（487，无消费方）"),

    // ══════════════ 2) 两级式 PCS（`Role::Pcs`，只读 3 区 FC04，72 点）══════════════
    // `pcs_3zone`（1000–1075，76 寄存器，72 点为值槽；1043/1045/1073/1075 为高半字，非点）。
    // 全表 `byte_swap: true`（§9.7.3）；`format` 逐字照抄厂方 `Int16`/`UInt16`（来源 = Vendor）。
    sc(Role::Pcs, 1000, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "模块故障告警 1（位图）"),
    sc(Role::Pcs, 1001, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "模块故障告警 2（位图）"),
    sc(Role::Pcs, 1002, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "模块故障告警 3（位图）"),
    sc(Role::Pcs, 1003, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "模块故障告警 4（位图）"),
    sc(Role::Pcs, 1004, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "模块故障告警 5（位图；bit0–7 文档未明确 Q-5）"),
    sc(Role::Pcs, 1005, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "BMS 工作状态（枚举）"),
    sc(Role::Pcs, 1006, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "BMS 可接受的最大充电电流 A"),
    sc(Role::Pcs, 1007, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "BMS 可接受的最大放电电流 A"),
    sc(Role::Pcs, 1008, RegFormat::Uint16, 0.1, 0.0, SymSrc::Vendor, "BMS 系统总电压 V（转述值）"),
    sc(Role::Pcs, 1009, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "BMS 系统总电流 A（方向文档未注明 Q-3）"),
    sc(Role::Pcs, 1010, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "BMS 系统 SOC %（转述值，不得作控制源 N-1）"),
    sc(Role::Pcs, 1011, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "直流母线电压 V"),
    sc(Role::Pcs, 1012, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "中点电压 V"),
    sc(Role::Pcs, 1013, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "模块运行状态（枚举；与 intercore 心跳同含义）"),
    sc(Role::Pcs, 1014, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "模块故障状态（枚举）"),
    sc(Role::Pcs, 1015, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "模块降额状态（枚举）"),
    sc(Role::Pcs, 1016, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "并离网状态（枚举）"),
    sc(Role::Pcs, 1017, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "故障告警代码（1–68；**不是**版本号 RC-12）"),
    sc(Role::Pcs, 1018, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "电网 A 相电压 V"),
    sc(Role::Pcs, 1019, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "电网 B 相电压 V"),
    sc(Role::Pcs, 1020, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "电网 C 相电压 V"),
    sc(Role::Pcs, 1021, RegFormat::Int16, 0.01, 0.0, SymSrc::Vendor, "交流母线频率 Hz"),
    sc(Role::Pcs, 1022, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出电流 A 相 A"),
    sc(Role::Pcs, 1023, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出电流 B 相 A"),
    sc(Role::Pcs, 1024, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出电流 C 相 A"),
    sc(Role::Pcs, 1025, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出视在功率 A 相 kVA"),
    sc(Role::Pcs, 1026, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出视在功率 B 相 kVA"),
    sc(Role::Pcs, 1027, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出视在功率 C 相 kVA"),
    sc(Role::Pcs, 1028, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "设备总视在功率输出 kVA"),
    sc(Role::Pcs, 1029, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出有功功率 A 相 kW（正放负充）"),
    sc(Role::Pcs, 1030, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出有功功率 B 相 kW"),
    sc(Role::Pcs, 1031, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出有功功率 C 相 kW"),
    sc(Role::Pcs, 1032, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "设备总有功功率输出 kW"),
    sc(Role::Pcs, 1033, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出无功功率 A 相 kvar"),
    sc(Role::Pcs, 1034, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出无功功率 B 相 kvar"),
    sc(Role::Pcs, 1035, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "输出无功功率 C 相 kvar"),
    sc(Role::Pcs, 1036, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "设备总无功功率输出 kvar"),
    sc(Role::Pcs, 1037, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "A 相功率因数"),
    sc(Role::Pcs, 1038, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "B 相功率因数"),
    sc(Role::Pcs, 1039, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "C 相功率因数"),
    sc(Role::Pcs, 1040, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "总功率因数"),
    sc(Role::Pcs, 1041, RegFormat::Int16, 1.0, 0.0, SymSrc::Vendor, "PCS 温度 ℃"),
    sc(Role::Pcs, 1042, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::Vendor, "交流累计充电电量 kWh（低字在低地址，word_order lo_hi）"),
    sc(Role::Pcs, 1044, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::Vendor, "交流累计放电电量 kWh（word_order lo_hi）"),
    sc(Role::Pcs, 1046, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "STS 网侧电压 A 相 V"),
    sc(Role::Pcs, 1047, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "STS 网侧电压 B 相 V"),
    sc(Role::Pcs, 1048, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "STS 网侧电压 C 相 V"),
    sc(Role::Pcs, 1049, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "STS 电网电压幅值 V"),
    sc(Role::Pcs, 1050, RegFormat::Int16, 0.01, 0.0, SymSrc::Vendor, "STS 电网电压频率 Hz"),
    sc(Role::Pcs, 1051, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载电流 A 相 A"),
    sc(Role::Pcs, 1052, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载电流 B 相 A"),
    sc(Role::Pcs, 1053, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载电流 C 相 A"),
    sc(Role::Pcs, 1054, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载视在功率 A 相 kVA"),
    sc(Role::Pcs, 1055, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载视在功率 B 相 kVA"),
    sc(Role::Pcs, 1056, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载视在功率 C 相 kVA"),
    sc(Role::Pcs, 1057, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载有功功率 A 相 kW"),
    sc(Role::Pcs, 1058, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载有功功率 B 相 kW"),
    sc(Role::Pcs, 1059, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载有功功率 C 相 kW"),
    sc(Role::Pcs, 1060, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载无功功率 A 相 kvar"),
    sc(Role::Pcs, 1061, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载无功功率 B 相 kvar"),
    sc(Role::Pcs, 1062, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "负载无功功率 C 相 kvar"),
    sc(Role::Pcs, 1063, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "负载功率因数 A 相"),
    sc(Role::Pcs, 1064, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "负载功率因数 B 相"),
    sc(Role::Pcs, 1065, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "负载功率因数 C 相"),
    sc(Role::Pcs, 1066, RegFormat::Uint16, 1.0, 0.0, SymSrc::Vendor, "工作模式判断（枚举）"),
    sc(Role::Pcs, 1067, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "低压总电流 A"),
    sc(Role::Pcs, 1068, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "高压总电流 A"),
    sc(Role::Pcs, 1069, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "低压外部总电压 V"),
    sc(Role::Pcs, 1070, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "低压总功率 kW"),
    sc(Role::Pcs, 1071, RegFormat::Int16, 1.0, 0.0, SymSrc::Vendor, "DCDC 温度 ℃"),
    sc(Role::Pcs, 1072, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::Vendor, "直流累计充电电量 kWh（word_order lo_hi）"),
    sc(Role::Pcs, 1074, RegFormat::Int32Scaled, 0.1, 0.0, SymSrc::Vendor, "直流累计放电电量 kWh（word_order lo_hi）"),

    // ══════════════ 3) 储能电能表 ADL400（`Role::MeterBatt`，FC03，40 点）══════════════
    // 来源混合（**逐点判定，见 PRD §9.5.3 的逐点来源表**）：
    //   · Vendor    —— 6 个总电能 + 3 个分相电能（原文备注「整形」）、4 字节功率类/PF
    //                  （原文备注「有符号整形」）、不平衡度 0x0093/0x0094（原文备注「整型」）；
    //   · Engineer  —— 其余 2 字节点（电压/电流/频率/线电压/零序/PT/CT），原文**无任何类型字样**。
    sc(Role::MeterBatt, 0x0000, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "当前组合有功总电能 kWh"),
    sc(Role::MeterBatt, 0x000A, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "当前正向总有功电能 kWh"),
    sc(Role::MeterBatt, 0x0014, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "当前反向总有功电能 kWh"),
    sc(Role::MeterBatt, 0x001E, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "当前组合无功总电能 kvarh"),
    sc(Role::MeterBatt, 0x0028, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "当前正向总无功电能 kvarh"),
    sc(Role::MeterBatt, 0x0032, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "当前反向总无功电能 kvarh"),
    sc(Role::MeterBatt, 0x0061, RegFormat::Uint16, 0.1, 0.0, SymSrc::Engineer, "A 相电压 V"),
    sc(Role::MeterBatt, 0x0062, RegFormat::Uint16, 0.1, 0.0, SymSrc::Engineer, "B 相电压 V"),
    sc(Role::MeterBatt, 0x0063, RegFormat::Uint16, 0.1, 0.0, SymSrc::Engineer, "C 相电压 V"),
    sc(Role::MeterBatt, 0x0064, RegFormat::Uint16, 0.01, 0.0, SymSrc::Engineer, "A 相电流 A"),
    sc(Role::MeterBatt, 0x0065, RegFormat::Uint16, 0.01, 0.0, SymSrc::Engineer, "B 相电流 A"),
    sc(Role::MeterBatt, 0x0066, RegFormat::Uint16, 0.01, 0.0, SymSrc::Engineer, "C 相电流 A"),
    sc(Role::MeterBatt, 0x0077, RegFormat::Uint16, 0.01, 0.0, SymSrc::Engineer, "频率 Hz"),
    sc(Role::MeterBatt, 0x0078, RegFormat::Uint16, 0.1, 0.0, SymSrc::Engineer, "A-B 线电压 V"),
    sc(Role::MeterBatt, 0x0079, RegFormat::Uint16, 0.1, 0.0, SymSrc::Engineer, "C-B 线电压 V"),
    sc(Role::MeterBatt, 0x007A, RegFormat::Uint16, 0.1, 0.0, SymSrc::Engineer, "A-C 线电压 V"),
    sc(Role::MeterBatt, 0x0087, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "A 相正向有功电能 kWh"),
    sc(Role::MeterBatt, 0x0089, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "B 相正向有功电能 kWh"),
    sc(Role::MeterBatt, 0x008B, RegFormat::Int32Scaled, 0.01, 0.0, SymSrc::Vendor, "C 相正向有功电能 kWh"),
    sc(Role::MeterBatt, 0x008D, RegFormat::Uint16, 1.0, 0.0, SymSrc::Engineer, "电压变比 PT（只读对照，写侧归写 Task）"),
    sc(Role::MeterBatt, 0x008E, RegFormat::Uint16, 1.0, 0.0, SymSrc::Engineer, "电流变比 CT（只读对照）"),
    sc(Role::MeterBatt, 0x0092, RegFormat::Uint16, 0.01, 0.0, SymSrc::Engineer, "零序电流 A（可为负的疑点，同 Q-20 判别）"),
    // 0x0093/0x0094：PRD §9.5.3 逐点来源表明确其来源为「**厂方标「整型」**」（0x0093 备注
    // 「整型 单位0.1%」、0x0094 无备注承前）⇒ `Vendor`，**不是**工程判断（§9.5 前言的
    // "工程判断"行 ADL400 清单只列电压/电流/频率/线电压/零序/PT/CT，不含不平衡度）。
    sc(Role::MeterBatt, 0x0093, RegFormat::Uint16, 0.1, 0.0, SymSrc::Vendor, "电压不平衡度 %"),
    sc(Role::MeterBatt, 0x0094, RegFormat::Uint16, 0.1, 0.0, SymSrc::Vendor, "电流不平衡度 %"),
    sc(Role::MeterBatt, 0x0164, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "A 相有功功率 kW"),
    sc(Role::MeterBatt, 0x0166, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "B 相有功功率 kW"),
    sc(Role::MeterBatt, 0x0168, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "C 相有功功率 kW"),
    sc(Role::MeterBatt, 0x016A, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "总有功功率 kW"),
    sc(Role::MeterBatt, 0x016C, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "A 相无功功率 kvar"),
    sc(Role::MeterBatt, 0x016E, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "B 相无功功率 kvar"),
    sc(Role::MeterBatt, 0x0170, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "C 相无功功率 kvar"),
    sc(Role::MeterBatt, 0x0172, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "总无功功率 kvar"),
    sc(Role::MeterBatt, 0x0174, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "A 相视在功率 kVA"),
    sc(Role::MeterBatt, 0x0176, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "B 相视在功率 kVA"),
    sc(Role::MeterBatt, 0x0178, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "C 相视在功率 kVA"),
    sc(Role::MeterBatt, 0x017A, RegFormat::Int32Scaled, 0.001, 0.0, SymSrc::Vendor, "总视在功率 kVA"),
    sc(Role::MeterBatt, 0x017C, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "A 相功率因数"),
    sc(Role::MeterBatt, 0x017D, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "B 相功率因数"),
    sc(Role::MeterBatt, 0x017E, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "C 相功率因数"),
    sc(Role::MeterBatt, 0x017F, RegFormat::Int16, 0.001, 0.0, SymSrc::Vendor, "总功率因数"),

    // ══════════════ 4) 工商储火灾报警控制器（`Role::Fire`，FC03）══════════════
    // 全部 16 位点为 `uint16`，来源 = **工程判断**（厂方无类型列，取值范围全非负，§9.5.4）。
    // ── `fire_sys`（4–16，13 点；含第 1 只探测器 11–16）──
    fire_sig(4, "系统状态（位图；bit14 主电故障/bit13 备电故障/bit11 驱动电路/bit10 压力传感器/bit9 电磁阀/bit8 喷洒标记）", &SIG_FIRE_SYS_STATUS),
    fire(5, "钢瓶气压 kPa（部分产品无此功能：恒 0 不得判异常，**不产事件**）"),
    fire_sig(6, "烟感状态（位图；bit1 复合探测器触发/bit0 干接点触发；bit2 预留不产事件）", &SIG_FIRE_SMOKE),
    fire_sig(7, "温感状态（位图；bit1 复合/bit0 干接点；bit2 预留不产事件）", &SIG_FIRE_TEMP),
    fire_sig(8, "可燃状态（位图；bit1 复合/bit0 干接点；bit2 预留不产事件）", &SIG_FIRE_COMBUSTIBLE),
    fire_sig(9, "火警状态（枚举：0 正常/1 一级报警/2 二级火警/3 预留/4 紧急启动/5 紧急停止）", &SIG_FIRE_LEVEL),
    fire(10, "复合探测器登记数量 个（跨文档契约点；点名 `fire_det_count`）"),
    fire(11, "探测器 1：地址（1–254，Q-9 地址序核对源）"),
    fire_sig(12, "探测器 1：状态（位图；bit12 报警总状态/bit14 故障总状态）", &SIG_FIRE_DETECTOR),
    fire(13, "探测器 1：数据 1（整字；高字节烟雾 0.1 dB/M、低字节温度 raw−55 ℃，拆解在展示层 G-6）"),
    fire(14, "探测器 1：数据 2 CO 浓度 ppm"),
    fire(15, "探测器 1：数据 3 VOC 浓度 ppm"),
    fire(16, "探测器 1：数据 4 H2 浓度 ppm"),
    // ── `fire_det` 组内语义模板（6 条；运行期按 `(addr − 17) % 6` 取，n=20 时展开 114 点）──
    // 第 n 只探测器首地址 = (n−1)*6+11（PRD §9.5.4 块寻址规则；探测器按地址号升序排列）。
    fire(17, "探测器 n：地址（模板 +0；Q-9 地址序核对源）"),
    fire_sig(18, "探测器 n：状态（模板 +1；bit12 报警总状态/bit14 故障总状态）", &SIG_FIRE_DETECTOR),
    fire(19, "探测器 n：数据 1（模板 +2；整字，拆解在展示层 G-6，**不产事件**）"),
    fire(20, "探测器 n：数据 2 CO 浓度 ppm（模板 +3）"),
    fire(21, "探测器 n：数据 3 VOC 浓度 ppm（模板 +4）"),
    fire(22, "探测器 n：数据 4 H2 浓度 ppm（模板 +5）"),

    // ══════════════ 5) 风冷空调机组（`Role::Hvac`，34 点）══════════════
    // FC04 温湿度：`format` 逐字照抄厂方（`16位有符号`/`16位无符号`）⇒ 来源 = Vendor。
    sc(Role::Hvac, 0, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "柜内测量温度 ℃（厂方明写 16 位有符号）"),
    sc(Role::Hvac, 2, RegFormat::Int16, 0.1, 0.0, SymSrc::Vendor, "内盘管测量温度 ℃（文档自相矛盾 Q-10，投运须比对）"),
    sc(Role::Hvac, 3, RegFormat::Uint16, 0.1, 0.0, SymSrc::Vendor, "柜内测量湿度 %（厂方明写 16 位无符号）"),
    // FC02 位块 `hvac_di`（位 0–30）：`Alarm` = 告警/故障类；`State` = 运行状态与输出；`Reserved` = 保留位。
    bit(Role::Hvac, 0, BitClass::State, "内风机（0 停止/1 运行）"),
    bit(Role::Hvac, 1, BitClass::State, "应急风机（0 停止/1 运行）"),
    bit(Role::Hvac, 2, BitClass::State, "制冷状态（0 停止/1 运行）"),
    bit(Role::Hvac, 3, BitClass::State, "加热状态（0 停止/1 运行）"),
    bit(Role::Hvac, 4, BitClass::State, "制冷除湿状态（0 停止/1 运行）"),
    bit(Role::Hvac, 5, BitClass::State, "加热除湿状态（0 停止/1 运行）"),
    bit(Role::Hvac, 6, BitClass::State, "系统自检状态（0 停止/1 运行）"),
    bit(Role::Hvac, 7, BitClass::State, "系统运行状态（0 停止/1 运行）"),
    bit(Role::Hvac, 8, BitClass::State, "报警继电器输出（0 关闭/1 吸合；输出回路状态，非告警条件）"),
    bit(Role::Hvac, 9, BitClass::Alarm, "柜内温感故障"),
    bit(Role::Hvac, 10, BitClass::Alarm, "柜内高温告警"),
    bit(Role::Hvac, 11, BitClass::Alarm, "柜内低温告警"),
    bit(Role::Hvac, 12, BitClass::Alarm, "柜外温感故障"),
    bit(Role::Hvac, 13, BitClass::Alarm, "柜外高温告警"),
    bit(Role::Hvac, 14, BitClass::Alarm, "柜外低温告警"),
    bit(Role::Hvac, 15, BitClass::Alarm, "柜内湿感故障"),
    bit(Role::Hvac, 16, BitClass::Alarm, "柜内高湿告警"),
    bit(Role::Hvac, 17, BitClass::Alarm, "柜内低湿告警"),
    bit(Role::Hvac, 18, BitClass::Alarm, "柜外湿感故障"),
    bit(Role::Hvac, 19, BitClass::Alarm, "柜外高湿告警"),
    bit(Role::Hvac, 20, BitClass::Alarm, "柜外低湿告警"),
    bit(Role::Hvac, 21, BitClass::Alarm, "压缩机高压告警"),
    bit(Role::Hvac, 22, BitClass::Alarm, "压缩机低压告警"),
    bit(Role::Hvac, 23, BitClass::Alarm, "制冷失效告警"),
    bit(Role::Hvac, 24, BitClass::Alarm, "制热失效告警"),
    bit(Role::Hvac, 25, BitClass::Reserved, "保留（无消费方）"),
    bit(Role::Hvac, 26, BitClass::Alarm, "内盘管温感故障"),
    bit(Role::Hvac, 27, BitClass::Alarm, "内盘管低温告警"),
    bit(Role::Hvac, 28, BitClass::Alarm, "三相电报警"),
    bit(Role::Hvac, 29, BitClass::Alarm, "接管/回风温差报警"),
    bit(Role::Hvac, 30, BitClass::State, "循环模式（0 非循环/1 循环）"),
];

/// 消防探测器区的组内模板起点（PRD §9.5.4：第 n 只探测器首地址 = `(n−1)*6+11`，n=2 → 17）。
pub const FIRE_DET_TEMPLATE_START: u16 = 17;
/// 组内模板宽度（每只探测器占 6 个连续寄存器）。
pub const FIRE_DET_STRIDE: u16 = 6;

/// 按 `(role, space, addr)` 查登记行；查不到 → `None`（调用方**放行**，见模块头）。
///
/// **消防探测器区（模板 + 运行期展开）**：`space = Reg` 且地址 ≥ 17 时按 `(addr − 17) % 6`
/// 取那 6 条组内语义模板（`+0 地址`/`+1 状态`/`+2 数据 1`/`+3 CO`/`+4 VOC`/`+5 H2`）。
pub fn lookup_in(role: Role, space: AddrSpace, addr: u16) -> Option<&'static PointReg> {
    let find = |a: u16| {
        POINT_REGS
            .iter()
            .find(|r| r.role == role && r.addr == a && r.kind.space() == space)
    };
    if let Some(row) = find(addr) {
        return Some(row);
    }
    if space == AddrSpace::Reg && role == Role::Fire && addr >= FIRE_DET_TEMPLATE_START {
        let idx = (addr - FIRE_DET_TEMPLATE_START) % FIRE_DET_STRIDE;
        return find(FIRE_DET_TEMPLATE_START + idx);
    }
    None
}

/// **寄存器空间**查登记行（规则 6 的唯一生产调用方：它只作用于标量点，故无需 space 参数）。
pub fn lookup(role: Role, addr: u16) -> Option<&'static PointReg> {
    lookup_in(role, AddrSpace::Reg, addr)
}

/// **位空间**查登记行（`discrete` 块的 `addr` 是位地址，与寄存器地址不同空间）。
pub fn lookup_bit(role: Role, addr: u16) -> Option<&'static PointReg> {
    lookup_in(role, AddrSpace::Bit, addr)
}

/// 某 `(role, addr)` 上的字级信号（无信号 → 空切片）。字级信号仅消防使用，而消防**没有**
/// `discrete` 块（全部状态量在保持寄存器整字内）⇒ 只查寄存器空间，签名与设计一致。
pub fn signals_of(role: Role, addr: u16) -> &'static [SignalSpec] {
    match lookup(role, addr) {
        Some(row) => row.signals,
        None => &[],
    }
}

// ───────────────────────────── 点名反查（`label`，事件 message / RC-1 用） ─────────────────────────────

/// 块布局（`label` 反查点名的依据）：`(role, 块名, 地址空间, 首地址, 末地址)`。
///
/// 点位名为**位置式**（PRD §9.4.2.2）：`<块名>_<块内偏移 + 1>`，故由块名与首地址即可反推
/// 地址。末地址取"该块能覆盖的最大地址"（`fire_det` 按 PRD 上限 n=100 → `17 + 6*99 − 1`），
/// 使现场扩容到上限时 `label` 仍可解析。
struct BlockSpan {
    role: Role,
    name: &'static str,
    space: AddrSpace,
    start: u16,
    end: u16,
}

const BLOCK_SPANS: &[BlockSpan] = &[
    BlockSpan {
        role: Role::Battery,
        name: "bms_io",
        space: AddrSpace::Reg,
        start: 100,
        end: 130,
    },
    BlockSpan {
        role: Role::Battery,
        name: "bms_energy",
        space: AddrSpace::Reg,
        start: 139,
        end: 157,
    },
    BlockSpan {
        role: Role::Battery,
        name: "bms_meta",
        space: AddrSpace::Reg,
        start: 181,
        end: 189,
    },
    BlockSpan {
        role: Role::Battery,
        name: "bms_term",
        space: AddrSpace::Reg,
        start: 2991,
        end: 2994,
    },
    BlockSpan {
        role: Role::Battery,
        name: "bms_cap",
        space: AddrSpace::Reg,
        start: 4000,
        end: 4005,
    },
    BlockSpan {
        role: Role::Battery,
        name: "bms_alarm",
        space: AddrSpace::Bit,
        start: 200,
        end: 487,
    },
    BlockSpan {
        role: Role::Pcs,
        name: "pcs_3zone",
        space: AddrSpace::Reg,
        start: 1000,
        end: 1075,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_e_act_comb",
        space: AddrSpace::Reg,
        start: 0x0000,
        end: 0x0001,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_e_act_fwd",
        space: AddrSpace::Reg,
        start: 0x000A,
        end: 0x000B,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_e_act_rev",
        space: AddrSpace::Reg,
        start: 0x0014,
        end: 0x0015,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_e_rea_comb",
        space: AddrSpace::Reg,
        start: 0x001E,
        end: 0x001F,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_e_rea_fwd",
        space: AddrSpace::Reg,
        start: 0x0028,
        end: 0x0029,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_e_rea_rev",
        space: AddrSpace::Reg,
        start: 0x0032,
        end: 0x0033,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_ui",
        space: AddrSpace::Reg,
        start: 0x0061,
        end: 0x0066,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_freq_line",
        space: AddrSpace::Reg,
        start: 0x0077,
        end: 0x007A,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_phase",
        space: AddrSpace::Reg,
        start: 0x0087,
        end: 0x0094,
    },
    BlockSpan {
        role: Role::MeterBatt,
        name: "mb_power",
        space: AddrSpace::Reg,
        start: 0x0164,
        end: 0x017F,
    },
    BlockSpan {
        role: Role::Fire,
        name: "fire_sys",
        space: AddrSpace::Reg,
        start: 4,
        end: 16,
    },
    BlockSpan {
        role: Role::Fire,
        name: "fire_det",
        space: AddrSpace::Reg,
        start: 17,
        end: 610,
    },
    BlockSpan {
        role: Role::Hvac,
        name: "hvac_in",
        space: AddrSpace::Reg,
        start: 0,
        end: 3,
    },
    BlockSpan {
        role: Role::Hvac,
        name: "hvac_di",
        space: AddrSpace::Bit,
        start: 0,
        end: 30,
    },
];

/// **跨文档契约点**（显式 `name`，非位置式命名）：`(role, 点名, 地址)`。
/// 只有它们不遵守 `<块名>_<序号>` 的位置式命名（PRD §9.4.2.2"显式 `name` 优先"）。
const EXPLICIT_NAMES: &[(Role, &str, u16)] = &[
    (Role::Battery, "soc", 118),
    (Role::Fire, "fire_det_count", 10),
];

/// 点位名 → 中文名（事件 message 补名用；查不到 → `None`，调用方用原名）。
///
/// 例：`label(Role::Battery, "bms_alarm_225")` = `"簇一级告警"`（点名序号 225 = 位地址 424，
/// **勿**与位地址 225「簇 SOC 低·轻」（点名 `bms_alarm_26`）混淆，§11.7.2 第 4 条）。
pub fn label(role: Role, metric: &str) -> Option<&'static str> {
    for (r, m, addr) in EXPLICIT_NAMES {
        if *r == role && *m == metric {
            return lookup(role, *addr).map(|row| row.label);
        }
    }
    for b in BLOCK_SPANS {
        if b.role != role {
            continue;
        }
        let Some(rest) = metric.strip_prefix(b.name) else {
            continue;
        };
        let Some(num) = rest.strip_prefix('_') else {
            continue;
        };
        let Ok(k) = num.parse::<u16>() else {
            continue;
        };
        if k == 0 {
            continue;
        }
        let addr = b.start + (k - 1);
        if addr > b.end {
            continue;
        }
        if let Some(row) = lookup_in(role, b.space, addr) {
            return Some(row.label);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 探测器区的模板命中（含**静态 6 行之外**的地址）与组内步进：
    /// `fire_det_<k> ↔ 16+k`，`+0..+5` 语义不串位。
    #[test]
    fn fire_detector_template_covers_beyond_static_rows() {
        for k in 1..=114u16 {
            let addr = 16 + k;
            let row = lookup(Role::Fire, addr)
                .unwrap_or_else(|| panic!("fire_det_{k}（addr={addr}）应由模板命中"));
            assert_eq!(
                row.addr,
                FIRE_DET_TEMPLATE_START + (addr - FIRE_DET_TEMPLATE_START) % 6
            );
        }
        // 组内语义：+0 无信号 / +1 带 alarm+fault / +2..+5 无信号
        assert!(signals_of(Role::Fire, 17).is_empty(), "+0 地址不产事件");
        let s = signals_of(Role::Fire, 18);
        assert_eq!(s.len(), 2, "+1 状态带 alarm/fault");
        assert_eq!(s[0].key, "alarm");
        assert_eq!(s[1].key, "fault");
        // +2..+5（数据 1/CO/VOC/H2）在任何探测器组上都不产事件（含第 31 组）
        for off in [2u16, 3, 4, 5] {
            let a = FIRE_DET_TEMPLATE_START + 6 * 30 + off;
            assert!(
                signals_of(Role::Fire, a).is_empty(),
                "addr {a}（+{off}）不产事件"
            );
        }
    }

    /// **`label` 的地址空间消歧**：`hvac` 的位 0 与寄存器 0 同址，必须各自反查到正确的行、
    /// 且 CH 两块的模板不得互相遮蔽（这是"登记表须按 `(role, space, addr)` 索引"的直接钉法）。
    #[test]
    fn label_disambiguates_register_and_bit_space() {
        let reg = lookup(Role::Hvac, 0).unwrap();
        let bit = lookup_bit(Role::Hvac, 0).unwrap();
        assert_ne!(reg.label, bit.label);
        assert_eq!(reg.label, "柜内测量温度 ℃（厂方明写 16 位有符号）");
        assert_eq!(bit.label, "内风机（0 停止/1 运行）");
        assert_eq!(
            label(Role::Hvac, "hvac_in_1"),
            Some("柜内测量温度 ℃（厂方明写 16 位有符号）")
        );
        assert_eq!(
            label(Role::Hvac, "hvac_di_1"),
            Some("内风机（0 停止/1 运行）")
        );
        // 位空间的模板命中不落回寄存器行
        assert!(matches!(
            lookup_bit(Role::Hvac, 30).map(|r| r.kind),
            Some(RegPointKind::Bit(_))
        ));
        assert!(lookup(Role::Hvac, 30).is_none(), "hvac 寄存器 30 无登记行");
    }

    /// 块布局表的**上界**：探测器区按 PRD 上限 n=100 登记（末寄存器 = 17 + 6×99 − 1 = 610），
    /// 故现场扩容到上限时 `label` 仍可解析；超出上限 → `None`（不静默给错名）。
    #[test]
    fn label_resolves_up_to_prd_upper_bound() {
        assert!(
            label(Role::Fire, "fire_det_594").is_some(),
            "n=100 的最后一点"
        );
        assert!(
            label(Role::Fire, "fire_det_595").is_none(),
            "超出 n=100 上限"
        );
        assert!(label(Role::Battery, "bms_alarm_288").is_some());
        assert!(label(Role::Battery, "bms_alarm_289").is_none());
        assert!(
            label(Role::Battery, "bms_energy_2").is_none(),
            "140 是高半字，非点"
        );
        assert!(label(Role::Fire, "no_such_metric").is_none());
    }

    /// 跨文档契约点的显式 `name` 必须可反查（改名即断供消费链，PRD §9.4.2.2）。
    #[test]
    fn explicit_contract_names_resolve() {
        assert_eq!(
            label(Role::Battery, "soc"),
            lookup(Role::Battery, 118).map(|r| r.label)
        );
        assert_eq!(
            label(Role::Fire, "fire_det_count"),
            lookup(Role::Fire, 10).map(|r| r.label)
        );
        // `fire_sys_7` 是同一地址（10）的**位置式别名**：它不再被产出（配置里声明了显式
        // `name`），但若历史事件里残留旧名，`label` 仍能解析到同一行 —— 不静默丢名。
        assert_eq!(
            label(Role::Fire, "fire_sys_7"),
            label(Role::Fire, "fire_det_count"),
            "位置式别名与契约名指向同一行"
        );
    }

    /// 消防字级信号的**席位**（掩码与活跃集逐条对表 PRD §9.5.4；与配置无关的自足断言）。
    #[test]
    fn fire_signal_specs_match_prd_bits() {
        let keys: Vec<&str> = signals_of(Role::Fire, 4).iter().map(|s| s.key).collect();
        assert_eq!(
            keys,
            [
                "main_power_fault",
                "backup_power_fault",
                "drive_circuit_fault",
                "pressure_sensor_fault",
                "spray_fired",
                "valve_open"
            ]
        );
        for (addr, key) in [
            (6u16, "smoke_trigger"),
            (7, "temp_trigger"),
            (8, "combustible_trigger"),
        ] {
            match signals_of(Role::Fire, addr)[0].pick {
                SignalPick::WordBit { mask } => assert_eq!(mask, 0b11, "addr {addr} {key}"),
                other => panic!("addr {addr} 应为 WordBit，实际 {other:?}"),
            }
        }
        // 火警状态：4 条枚举，活跃集互斥且不含 0/3
        let mut actives = Vec::new();
        for s in signals_of(Role::Fire, 9) {
            match s.pick {
                SignalPick::WordEnum { active } => actives.extend(active.iter().map(|(v, _)| *v)),
                other => panic!("火警状态应为 WordEnum，实际 {other:?}"),
            }
        }
        assert_eq!(actives, vec![1, 2, 4, 5]);
        // 探测器状态：恰 bit12/bit14
        let union: u16 = signals_of(Role::Fire, 12)
            .iter()
            .map(|s| match s.pick {
                SignalPick::WordBit { mask } => mask,
                _ => 0,
            })
            .fold(0, |a, b| a | b);
        assert_eq!(union, (1 << 12) | (1 << 14));
    }
}
