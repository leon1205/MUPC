//! **写入闸门**：磁盘水位分级 + DB 完整性降级的两条"停止写入"判据（U-74 审查 B-1 + A-7）。
//!
//! # 为什么在 `storage`（而不是 core-bin）
//!
//! "停写"的**执行点**全在本 crate：遥测走 [`crate::WriteBuffer`]、事件/故障/决策/台账走
//! [`crate::repository`] 的 5 个 `Sqlite*Repo`。把判据放装配层（core-bin）就只能"喊停"而
//! 无法在**每个写入口**兜住 ⇒ 必然漏一两个入口。闸门对象在 `storage` 内、由 core-bin
//! **喂数据**（磁盘占用率来自 `mupc_system_monitor`；完整性自检由 core-bin 在启动期跑
//! `PRAGMA quick_check`）—— 判据与执行同侧，数据与决策分离。
//!
//! # 两条判据（口径逐条对 PRD，**不加码**）
//!
//! | 判据 | 依据 | 动作 |
//! |------|------|------|
//! | 磁盘水位 ≥95% | 03 PRD §8.1「critical」 | **停时序写入**（不再接收新遥测点） |
//! | 磁盘水位 ≥98% | 03 PRD §8.1 | **停全部写入**（含事件/故障/决策/台账） |
//! | DB 完整性自检失败 | 03 PRD §7.6 / 03 设计 §4.5.5「降级模式」 | **停全部写入**（拒写 + 告警） |
//!
//! ⚠️ **不做"自动修复"**（WAL 重建 / `REINDEX` / 备份恢复）：那是独立立项，本轮只做
//! 「检测 → 明确降级 → 告警」，**不静默带病运行**。
//!
//! # 「采集不可用 ⇒ 不判定」（**不得按 0% 处理**）
//!
//! [`WriteGate::set_disk_usage`] 收 `Option<f32>`：`None` = 磁盘指标**采集失败**
//! （`mupc_system_monitor::SystemSnapshot::disk == None`，D-15 口径）⇒ **保持上一次判定、
//! 不臆造 0%**。把"采不到"当成"空盘"会让高水位保护在最需要它的时候（采集链路本身出问题时）
//! 失效。

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

/// 磁盘水位等级（03 PRD §8.1 的四档 + `Normal`）。
///
/// 判别式（左闭右开，避免档位边界歧义）：
/// `Normal < 85 ≤ Warn < 90 ≤ Minor < 95 ≤ Critical < 98 ≤ Emergency`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum DiskLevel {
    /// `< 85%`：正常。
    Normal = 0,
    /// `[85%, 90%)`：WARN（仅告警）。
    Warn = 1,
    /// `[90%, 95%)`：minor（仅告警）。
    Minor = 2,
    /// `[95%, 98%)`：critical ⇒ **停时序写入**。
    Critical = 3,
    /// `≥ 98%`：**停全部写入**。
    Emergency = 4,
}

impl DiskLevel {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Warn,
            2 => Self::Minor,
            3 => Self::Critical,
            4 => Self::Emergency,
            _ => Self::Normal,
        }
    }

    /// 稳定短名（日志/告警文案用；**不随本地化变化**，便于 grep 与外部采集）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warn => "warn",
            Self::Minor => "minor",
            Self::Critical => "critical",
            Self::Emergency => "emergency",
        }
    }
}

/// 按磁盘占用率分级；`None` = **采集不可用 ⇒ 不判定**（调用方须保持上一次等级）。
///
/// `usage_percent` 为 `f32`（`mupc_system_monitor::DiskMetrics::usage_percent` 的类型）；
/// `NaN` 亦按"不可用"处理（`NaN` 的比较全 false ⇒ 若直接走阈值链会**落到 `Normal`**，
/// 那正是"把不可用当空盘"的同一种错误）。
pub fn classify_disk_usage(usage_percent: Option<f32>) -> Option<DiskLevel> {
    let p = usage_percent?;
    if !p.is_finite() {
        return None;
    }
    Some(if p < 85.0 {
        DiskLevel::Normal
    } else if p < 90.0 {
        DiskLevel::Warn
    } else if p < 95.0 {
        DiskLevel::Minor
    } else if p < 98.0 {
        DiskLevel::Critical
    } else {
        DiskLevel::Emergency
    })
}

/// 写入闸门（装配层持 `Arc`，与 `WriteBuffer` / 各 `Sqlite*Repo` 共享）。
pub struct WriteGate {
    /// 当前磁盘等级（[`DiskLevel`] 的 `u8`）。
    disk_level: AtomicU8,
    /// DB 完整性降级位（置位后**不清除**：本轮不做自动修复 ⇒ 降级是终态，重启才复位）。
    degraded: AtomicBool,
    /// 是否**拿到过**可信磁盘数据（`false` ⇒ 从未判定，日志里如实标 `unknown`）。
    disk_known: AtomicBool,
    /// 因水位/降级被拒的**遥测点**条数（累计）。
    rejected_telemetry: AtomicU64,
    /// 因水位/降级被拒的**其它写入**次数（事件/故障/决策/台账，累计）。
    rejected_writes: AtomicU64,
}

impl Default for WriteGate {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteGate {
    /// 全新闸门：水位 `Normal` + 未降级（= 改造前的行为）。
    pub fn new() -> Self {
        Self {
            disk_level: AtomicU8::new(DiskLevel::Normal as u8),
            degraded: AtomicBool::new(false),
            disk_known: AtomicBool::new(false),
            rejected_telemetry: AtomicU64::new(0),
            rejected_writes: AtomicU64::new(0),
        }
    }

    /// 喂一次磁盘占用率，返回**本次**等级（`None` = 采集不可用 ⇒ 等级不变、返回 `None`）。
    ///
    /// 调用方（core-bin 的系统指标采集任务）据此判定"是否发生档位迁移"并发告警。
    pub fn set_disk_usage(&self, usage_percent: Option<f32>) -> Option<DiskLevel> {
        let level = classify_disk_usage(usage_percent)?;
        self.disk_known.store(true, Ordering::Release);
        self.disk_level.store(level as u8, Ordering::Release);
        Some(level)
    }

    /// 进入 **DB 完整性降级**态（03 设计 §4.5.5）：此后**拒一切写入**。
    ///
    /// **幂等**；`reason` 只进日志/告警（闸门不存文本，避免在热路径上分配）。
    pub fn enter_degraded(&self) {
        self.degraded.store(true, Ordering::Release);
    }

    /// 是否已进入完整性降级态。
    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Acquire)
    }

    /// 当前磁盘等级（从未判定过 ⇒ `Normal`，用 [`Self::disk_known`] 区分"未判定"与"确实正常"）。
    pub fn disk_level(&self) -> DiskLevel {
        DiskLevel::from_u8(self.disk_level.load(Ordering::Acquire))
    }

    /// 是否**拿到过**可信磁盘数据。
    pub fn disk_known(&self) -> bool {
        self.disk_known.load(Ordering::Acquire)
    }

    /// 能否接收**新遥测点**（时序写入）：降级 ⇒ 否；水位 ≥ [`DiskLevel::Critical`] ⇒ 否。
    pub fn allows_telemetry(&self) -> bool {
        !self.is_degraded() && self.disk_level() < DiskLevel::Critical
    }

    /// 能否做**任何**写入（事件/故障/决策/台账，以及缓冲区的 flush）：
    /// 降级 ⇒ 否；水位 ≥ [`DiskLevel::Emergency`] ⇒ 否。
    pub fn allows_any_write(&self) -> bool {
        !self.is_degraded() && self.disk_level() < DiskLevel::Emergency
    }

    /// 记一次被拒的**遥测点**（并返回累计值）。
    pub fn note_rejected_telemetry(&self, n: u64) -> u64 {
        self.rejected_telemetry.fetch_add(n, Ordering::Relaxed) + n
    }

    /// 记一次被拒的**其它写入**（并返回累计值）。
    pub fn note_rejected_write(&self) -> u64 {
        self.rejected_writes.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 累计被拒遥测点数。
    pub fn rejected_telemetry_total(&self) -> u64 {
        self.rejected_telemetry.load(Ordering::Relaxed)
    }

    /// 累计被拒其它写入次数。
    pub fn rejected_writes_total(&self) -> u64 {
        self.rejected_writes.load(Ordering::Relaxed)
    }

    /// 一行可打日志的闸门状态（含"未判定"的显式表达）。
    pub fn status_line(&self) -> String {
        format!(
            "write_gate: disk_level={} disk_known={} degraded={} rejected_telemetry={} rejected_writes={}",
            self.disk_level().as_str(),
            self.disk_known(),
            self.is_degraded(),
            self.rejected_telemetry_total(),
            self.rejected_writes_total(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **B-1 主判据：03 PRD §8.1 的四条档位边界逐个钉死**（84/85/90/95/98）。
    ///
    /// **改坏实现即红**：把任一阈值改成 `<=`（或写成 `p <= 85.0`）⇒ 边界值那两条断言红。
    #[test]
    fn disk_level_boundaries_match_prd_8_1() {
        let cases: &[(f32, DiskLevel)] = &[
            (0.0, DiskLevel::Normal),
            (84.0, DiskLevel::Normal),
            (84.999, DiskLevel::Normal),
            // 85 起 WARN（左闭）
            (85.0, DiskLevel::Warn),
            (89.999, DiskLevel::Warn),
            // 90 起 minor
            (90.0, DiskLevel::Minor),
            (94.999, DiskLevel::Minor),
            // 95 起 critical（停时序写入）
            (95.0, DiskLevel::Critical),
            (97.999, DiskLevel::Critical),
            // 98 起 emergency（停全部写入）
            (98.0, DiskLevel::Emergency),
            (100.0, DiskLevel::Emergency),
        ];
        for (usage, want) in cases {
            assert_eq!(
                classify_disk_usage(Some(*usage)),
                Some(*want),
                "{usage}% 应判为 {want:?}"
            );
        }
    }

    /// **"采集不可用 ⇒ 不判定"**：`None` 与 `NaN` 都不得被判成任何等级
    /// （特别是**不得落 `Normal`** —— 那等于把"采不到"当"空盘"，高水位保护会失效）。
    #[test]
    fn unavailable_disk_metric_is_not_a_verdict() {
        assert_eq!(classify_disk_usage(None), None);
        assert_eq!(classify_disk_usage(Some(f32::NAN)), None);
        assert_eq!(classify_disk_usage(Some(f32::INFINITY)), None);

        let g = WriteGate::new();
        assert_eq!(g.set_disk_usage(None), None);
        assert!(!g.disk_known(), "未判定须如实标 unknown");
        // 先升到 emergency，再来一次"采集失败" ⇒ **等级必须保持**（不得回落 Normal）
        assert_eq!(g.set_disk_usage(Some(99.0)), Some(DiskLevel::Emergency));
        assert_eq!(g.set_disk_usage(None), None);
        assert_eq!(g.disk_level(), DiskLevel::Emergency, "采不到不得改写已有判定");
        assert!(!g.allows_any_write());
    }

    /// **B-1 动作判据**：各档位的允许/禁止组合（≥95 停时序、≥98 停全部）。
    #[test]
    fn gate_actions_follow_level() {
        let g = WriteGate::new();
        for usage in [0.0f32, 84.0, 85.0, 89.9, 90.0, 94.9] {
            g.set_disk_usage(Some(usage));
            assert!(g.allows_telemetry(), "{usage}% 应允许时序写入");
            assert!(g.allows_any_write(), "{usage}% 应允许全部写入");
        }
        g.set_disk_usage(Some(95.0));
        assert!(!g.allows_telemetry(), "≥95% 须停时序写入（PRD §8.1）");
        assert!(g.allows_any_write(), "95–98% 仍允许事件/故障写入");
        g.set_disk_usage(Some(98.0));
        assert!(!g.allows_any_write(), "≥98% 须停全部写入（PRD §8.1）");

        // 水位回落 ⇒ 权限恢复（磁盘清理后不必重启）
        g.set_disk_usage(Some(10.0));
        assert!(g.allows_telemetry() && g.allows_any_write());
    }

    /// **A-7 判据：DB 完整性降级 ⇒ 拒一切写入**，且**幂等、不回退**（本轮不做自动修复）。
    #[test]
    fn integrity_degradation_blocks_every_write_and_is_sticky() {
        let g = WriteGate::new();
        assert!(g.allows_any_write() && g.allows_telemetry());
        g.enter_degraded();
        g.enter_degraded(); // 幂等
        assert!(g.is_degraded());
        assert!(!g.allows_telemetry(), "降级态须停时序写入");
        assert!(!g.allows_any_write(), "降级态须停全部写入");
        // 磁盘水位再"正常"也不解除降级（降级是**完整性**判据，与水位正交）
        g.set_disk_usage(Some(1.0));
        assert!(!g.allows_any_write(), "水位正常不得解除完整性降级");
        assert!(g.status_line().contains("degraded=true"));
    }

    /// 计数器：被拒的点数/写入次数可读且单调（B-1/A-7 的"不静默"凭据）。
    #[test]
    fn rejected_counters_are_readable() {
        let g = WriteGate::new();
        assert_eq!(g.rejected_telemetry_total(), 0);
        assert_eq!(g.rejected_writes_total(), 0);
        assert_eq!(g.note_rejected_telemetry(3), 3);
        assert_eq!(g.note_rejected_telemetry(2), 5);
        assert_eq!(g.note_rejected_write(), 1);
        assert_eq!(g.note_rejected_write(), 2);
        let line = g.status_line();
        assert!(line.contains("rejected_telemetry=5"), "{line}");
        assert!(line.contains("rejected_writes=2"), "{line}");
    }
}
