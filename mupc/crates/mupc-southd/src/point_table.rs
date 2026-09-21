//! 点表登记注册表（S3b-2 §11.4.4）。
//!
//! **为什么需要它**：PRD §9.4.3「符号性声明一致性」第 ② 条的可判定性以"校验器内的点表
//! 登记值常量表"（`(role, addr) → {format, scale, offset, 来源}`，由设计阶段按 §9.5 逐点
//! 转写）为前提；查不到该表，第 ② 条与第 ① 条同样"无源可判"。
//!
//! **本模块的交付状态**：S3b-2 **T3** 只落**规则 6 所需的最小骨架** —— 类型定义、
//! 查表函数与空的 [`POINT_REGS`]；**618 行逐点转写**（含探测器区模板）与 `label` /
//! `signals`（消防字级信号）属 **T4**（§11.13 的 T4 行）。空表下的行为即 §11.4.4 的
//! "**查不到行 → 放行**"分支（现场 RC-3 会合法改 `addr` 基准，不得误拒）。
//!
//! **强制口径（§11.4.4，随表生效）**：① `format ∈ {uint16,int16}` 且 `offset ≠ 0`，
//! 而 `lookup` **命中一行**、但该行 `sym_src` 为空 → 拒；② `lookup` 命中且 `offset` 与
//! 登记值不等（含"漏配 → 缺省 0"）→ 拒。`format` / `scale` 与登记值不一致**不强制**
//!（Q-4/Q-16/Q-20 的现场裁定会合法改变它们，强校验会把校准后的正确配置拒在启动期）。

use crate::config::Role;

/// 符号性来源（PRD §9.5 前言的三分类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymSrc {
    /// 厂方逐点明写（PCS、空调、ADL400 的 4 字节功率类/PF）
    Vendor,
    /// 厂方标注 + 推断订正（BMS 的 `UNIT` → `UINT`）
    VendorTypo,
    /// 工程判断（消防、ADL400 2 字节点）
    Engineer,
}

/// 点表登记行（§11.4.4 的 `PointReg` 子集；T4 补齐 `kind`/`scale`/`label`/`signals`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointReg {
    /// 所属 role
    pub role: Role,
    /// 绝对寄存器地址（`discrete` 块为**位地址**）
    pub addr: u16,
    /// 登记换算偏移（规则 6 ② 的期望值）
    pub offset: f64,
    /// 该点的符号性来源；**`offset ≠ 0` 的行必须登记**（否则规则 6 ① 命中也拒）
    pub sym_src: Option<SymSrc>,
}

/// 逐点登记表（按 §9.5 转写；**T4 落 618 行**，本 Task 为空表 = "查不到行 → 放行"）。
pub const POINT_REGS: &[PointReg] = &[];

/// 按 `(role, addr)` 查登记行；查不到 → `None`（调用方**放行**，见模块头）。
pub fn lookup(role: Role, addr: u16) -> Option<&'static PointReg> {
    POINT_REGS.iter().find(|r| r.role == role && r.addr == addr)
}
