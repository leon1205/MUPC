//! 跨域转换：`mupc-data-processing::latest_values::PointQuality` → 落库用的 `i32`
//! （03 设计 §9.1.4 / §9.6 序 10 / §9.8 D-8）。
//!
//! # 为什么在**装配层**（本文件），不在 `mupc-storage`
//!
//! `storage` **不依赖** `data-processing`（`storage/Cargo.toml` 无该依赖，反向也无）⇒
//! 把映射放进 `storage` 会**新增 `storage → data-processing` 依赖边**（纯为一次枚举映射，
//! 代价不成比例）。`core-bin` 同时依赖两者 ⇒ 落这里**零新增边**。依赖分工由此固定：
//!
//! - `storage` 只拥有 `Quality`（**落库记录形态**的唯一所有者），**不认识** `PointQuality`；
//! - `data-processing` 只拥有 `PointQuality`（内存快照语义），**不认识** `Quality`；
//! - 转换**只在装配层**发生（与「装配层是跨域转换点」的既有口径一致）。
//!
//! # 映射表（一一对应，不新增语义）
//!
//! | `PointQuality` | `storage::Quality` | 落库值 |
//! |----------------|--------------------|--------|
//! | `Ok` | `Good` | **0**（既有写入值，保持不变 ⇒ 零行为变化） |
//! | `Invalid` | `Invalid` | 2 |
//! | `Stale` | `Stale` | 3 |
//! | `Unconfigured` | `Unconfigured` | 4 |
//!
//! `NoData`（1）**不由**本函数产出：它只属于「本周期该通道无有效采样」的**聚合缺测行**
//! （`GridAggregator` 直接给 `Quality::NoData`），内存快照侧没有对应态（`Unconfigured` 才是
//! 「从未采集过」）。

use mupc_data_processing::latest_values::PointQuality;
use mupc_storage::Quality;

/// `PointQuality` → `telemetry.quality` 的整型编码。
pub fn quality_from_point_quality(q: PointQuality) -> i32 {
    let mapped = match q {
        PointQuality::Ok => Quality::Good,
        PointQuality::Invalid => Quality::Invalid,
        PointQuality::Stale => Quality::Stale,
        PointQuality::Unconfigured => Quality::Unconfigured,
    };
    mapped.code()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 映射表逐项断言（+「`Ok → 0` 保持既有写入值」这条零行为变化的机械判据）。
    #[test]
    fn quality_mapping_matches_design_table() {
        assert_eq!(quality_from_point_quality(PointQuality::Ok), 0);
        assert_eq!(quality_from_point_quality(PointQuality::Invalid), 2);
        assert_eq!(quality_from_point_quality(PointQuality::Stale), 3);
        assert_eq!(quality_from_point_quality(PointQuality::Unconfigured), 4);
        // `NoData`(1) 只能由聚合缺测行产出，**不得**被本映射占用
        assert_ne!(quality_from_point_quality(PointQuality::Ok), 1);
        assert_ne!(quality_from_point_quality(PointQuality::Invalid), 1);
        assert_ne!(quality_from_point_quality(PointQuality::Stale), 1);
        assert_ne!(quality_from_point_quality(PointQuality::Unconfigured), 1);
    }
}
