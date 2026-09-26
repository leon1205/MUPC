//! 外设遥测「最新值快照 + 变更通知」入口的**公开面**用例（01 设计 §9.1 / §9.5）。
//!
//! 覆盖 PRD 01 §8.5（LV-1～LV-6）与 AC-U74-12/13：
//! - LV-1/LV-2：入口进程内可读、零 DB（本类型不持任何仓储句柄，结构性保证）
//! - LV-3：`ts_ms` + `quality` + `is_fresh`（判据真源 = 注入的 `stale_timeout_s`）
//! - LV-4：`subscribe()` 变更通知（`broadcast` 容量 64；`Lagged` 不 panic、可全量重读）
//! - LV-5：键 = 南向点名（`PointId.metric` 逐字），位点同走本入口
//! - LV-6：不可得显式表达（`value: None` + 显式 `quality`），**严禁补 0**
//!
//! 白盒用例（`HashMap` 容量预留断言）在 `src/latest_values.rs` 的内联 `#[cfg(test)]` 内
//! ——私有字段在集成测试不可见，公开面无法观测容量。

use std::sync::Arc;

use mupc_data_processing::latest_values::{
    ChangeBatch, LatestValues, PointId, PointQuality, PointValue, PointView,
};

/// 测试用点位主键。
fn pid(station: &str, metric: &str) -> PointId {
    PointId {
        station: station.to_string(),
        metric: metric.to_string(),
    }
}

/// 测试用点值（`value: Some`）。
fn pv(value: f64, ts_ms: u64, quality: PointQuality) -> PointValue {
    PointValue {
        value: Some(value),
        ts_ms,
        quality,
    }
}

const STALE_S: u64 = 5;

// ── ① 写入后可读到正确值与时标 ─────────────────────────────────────────

#[test]
fn apply_then_get_returns_value_and_ts() {
    let lv = LatestValues::new(STALE_S);
    let id = pid("meter_grid", "active_power");

    assert!(lv
        .apply(vec![(id.clone(), pv(12.5, 1_000, PointQuality::Ok))])
        .is_some());

    let got: PointView = lv.get(&id);
    assert_eq!(got.id, id, "读结果必须回带主键（消费方按站分片用）");
    assert_eq!(got.value.value, Some(12.5), "值必须逐字读回");
    assert_eq!(
        got.value.ts_ms, 1_000,
        "时标 = 采集时刻（不得用读取时刻顶替）"
    );
    assert_eq!(got.value.quality, PointQuality::Ok);
}

#[test]
fn station_snapshot_returns_only_that_station() {
    let lv = LatestValues::new(STALE_S);
    lv.apply(vec![
        (pid("bms", "soc"), pv(55.0, 100, PointQuality::Ok)),
        (pid("bms", "bms_io_1"), pv(1.0, 100, PointQuality::Ok)),
        (pid("hvac", "hvac_in_1"), pv(3.0, 100, PointQuality::Ok)),
    ]);

    let bms = lv.station_snapshot("bms");
    assert_eq!(bms.len(), 2, "只返回该站已登记点，跨站不串");
    assert!(bms.iter().all(|v| v.id.station == "bms"));

    let hvac = lv.station_snapshot("hvac");
    assert_eq!(hvac.len(), 1);

    assert!(
        lv.station_snapshot("pcs").is_empty(),
        "未登记站返回空集（点缺 = 消费方 NotRead，不臆造）"
    );
}

#[test]
fn all_returns_every_registered_point() {
    let lv = LatestValues::new(STALE_S);
    lv.apply(vec![
        (pid("bms", "soc"), pv(55.0, 100, PointQuality::Ok)),
        (pid("hvac", "hvac_in_1"), pv(3.0, 100, PointQuality::Ok)),
    ]);
    let mut metrics: Vec<String> = lv.all().into_iter().map(|v| v.id.metric).collect();
    metrics.sort();
    assert_eq!(metrics, vec!["hvac_in_1".to_string(), "soc".to_string()]);
}

// ── ② 不可得点的表达（禁补 0）───────────────────────────────────────────

#[test]
fn never_registered_point_is_explicitly_unavailable_not_zero() {
    let lv = LatestValues::new(STALE_S);
    let got = lv.get(&pid("bms", "never_seen"));

    assert_eq!(
        got.value.value, None,
        "从未采集过的点必须显式不可得（严禁以 0.0 顶替，LV-6）"
    );
    assert_ne!(got.value.value, Some(0.0));
    assert_eq!(got.value.quality, PointQuality::Unconfigured);
    assert_eq!(got.value.ts_ms, 0, "无采集事实 ⇒ 时标 0（不臆造时刻）");
}

#[test]
fn unavailable_sample_stays_none_and_is_distinguishable_from_a_real_zero() {
    let lv = LatestValues::new(STALE_S);
    let zero = pid("meter_grid", "active_power");
    let missing = pid("meter_grid", "reactive_power");

    // 合法 0 值采样：`Some(0.0)` + `Ok`
    lv.apply(vec![(zero.clone(), pv(0.0, 100, PointQuality::Ok))]);
    // 不可得：`None` + `Invalid`
    lv.apply(vec![(
        missing.clone(),
        PointValue {
            value: None,
            ts_ms: 100,
            quality: PointQuality::Invalid,
        },
    )]);

    let a = lv.get(&zero);
    let b = lv.get(&missing);
    assert_eq!(a.value.value, Some(0.0), "合法 0 值必须如实保留为 0.0");
    assert_eq!(a.value.quality, PointQuality::Ok);
    assert_eq!(b.value.value, None, "不可得必须保持 None，不得被 0 顶替");
    assert_eq!(b.value.quality, PointQuality::Invalid);
    assert_ne!(
        a.value, b.value,
        "「值为 0」与「不可得」必须在读面上可区分（AC-U74-05 的机械判据）"
    );
}

#[test]
fn sample_quality_is_preserved_verbatim() {
    let lv = LatestValues::new(STALE_S);
    let id = pid("bms", "soc");
    lv.apply(vec![(id.clone(), pv(55.0, 100, PointQuality::Stale))]);
    assert_eq!(lv.get(&id).value.quality, PointQuality::Stale);
}

// ── ③ 变更通知 ────────────────────────────────────────────────────────

#[tokio::test]
async fn subscriber_receives_change_batch() {
    let lv = LatestValues::new(STALE_S);
    let mut rx = lv.subscribe();

    let seq = lv
        .apply(vec![(pid("bms", "soc"), pv(55.0, 100, PointQuality::Ok))])
        .expect("首轮登记即为变更 ⇒ 必须产生变更批");

    let batch: ChangeBatch = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("通知时延上界 1 s（LV-4）")
        .expect("订阅者必须收到变更批");
    assert_eq!(batch.seq, seq, "批序号必须回带，供消费方对齐");
    assert_eq!(batch.changed, vec![pid("bms", "soc")]);
}

#[tokio::test]
async fn unchanged_value_is_not_broadcast() {
    let lv = LatestValues::new(STALE_S);
    let id = pid("meter_grid", "voltage");
    let mut rx = lv.subscribe();

    assert!(lv
        .apply(vec![(id.clone(), pv(220.0, 100, PointQuality::Ok))])
        .is_some());
    let _ = rx.recv().await.expect("首轮变更必须广播");

    assert_eq!(
        lv.apply(vec![(id.clone(), pv(220.0, 200, PointQuality::Ok))]),
        None,
        "值不变 ⇒ 不产生变更批（COS 语义实现点，§9.1.5）"
    );
    assert!(
        rx.try_recv().is_err(),
        "值不变时不得广播（否则 COS 每轮假变位）"
    );
}

#[test]
fn unchanged_value_still_refreshes_ts_and_quality() {
    let lv = LatestValues::new(STALE_S);
    let id = pid("meter_grid", "frequency");

    lv.apply(vec![(id.clone(), pv(50.0, 100, PointQuality::Ok))]);
    assert_eq!(lv.get(&id).value.ts_ms, 100);

    // 同一值再来一轮：不广播，但「本轮读到」这一采集事实必须刷新
    assert_eq!(
        lv.apply(vec![(id.clone(), pv(50.0, 900, PointQuality::Ok))]),
        None
    );
    let got = lv.get(&id);
    assert_eq!(
        got.value.ts_ms, 900,
        "值不变 ≠ 不刷新时标（冻结时标会让恒定值 5 s 后集体变过期，§9.1.5）"
    );
    assert_eq!(got.value.value, Some(50.0));
}

#[tokio::test]
async fn lagged_subscriber_does_not_panic_and_can_full_reload() {
    let lv = LatestValues::new(STALE_S);
    let id = pid("bms", "soc");
    let mut rx = lv.subscribe();

    // 远超广播容量（64）的连续变更，且全程不读 ⇒ 必然落后
    for i in 0..200u64 {
        lv.apply(vec![(
            id.clone(),
            pv(i as f64, 1_000 + i, PointQuality::Ok),
        )]);
    }

    let err = rx.recv().await.expect_err("落后 200 批必须报 Lagged");
    assert!(
        matches!(err, tokio::sync::broadcast::error::RecvError::Lagged(_)),
        "丢帧必须表达为 Lagged（允许丢，不允许静默损坏数值）"
    );

    // 消费方契约：Lagged ⇒ 全量重读重建基线（此处即「重读可用」）
    let all = lv.all();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].value.value, Some(199.0), "全量重读读到最新值");
    assert_eq!(lv.get(&id).value.value, Some(199.0));
}

#[test]
fn change_list_is_deduped_and_grouped_by_station() {
    let lv = LatestValues::new(STALE_S);
    let mut rx = lv.subscribe();
    let seq = lv
        .apply(vec![
            (pid("hvac", "hvac_in_1"), pv(1.0, 10, PointQuality::Ok)),
            (pid("bms", "soc"), pv(2.0, 10, PointQuality::Ok)),
            (pid("hvac", "hvac_in_1"), pv(3.0, 20, PointQuality::Ok)), // 同批重复 ⇒ 去重
            (pid("bms", "soh"), pv(4.0, 10, PointQuality::Ok)),
        ])
        .expect("有变更");
    let batch: ChangeBatch = rx
        .try_recv()
        .expect("变更批必须入队（无运行时下用 try_recv）");
    assert_eq!(batch.seq, seq);

    let stations: Vec<&str> = batch.changed.iter().map(|p| p.station.as_str()).collect();
    assert!(
        stations.windows(2).all(|w| w[0] <= w[1]),
        "批内必须按 station 分组（便于分片发布），实际 = {stations:?}"
    );
    assert_eq!(batch.changed.len(), 3, "同批重复点只算一次：{stations:?}");
    // 同批重复 ⇒ 后写覆盖
    let hvac = lv.get(&pid("hvac", "hvac_in_1"));
    assert_eq!(hvac.value.value, Some(3.0));
    assert_eq!(hvac.value.ts_ms, 20);
}

// ── ④ 站活性 / station_last_poll_ms（R-38）────────────────────────────

#[test]
fn mark_station_polled_does_not_produce_change_batch() {
    let lv = LatestValues::new(STALE_S);
    let id = pid("bms", "soc");
    lv.apply(vec![(id, pv(55.0, 100, PointQuality::Ok))]);

    // 只刷新「站在采」这一事实：不改点值、不改时标 ⇒ 无变更批
    lv.mark_station_polled("bms", 5_000);
    assert!(lv.station_is_active("bms", 5_000));
    assert!(
        lv.station_is_active("bms", 10_000),
        "边界 = 恰好 5 s 仍在窗内"
    );
    assert!(
        !lv.station_is_active("bms", 10_001),
        "超过 stale_timeout_s ⇒ 不在窗内"
    );
}

#[test]
fn station_is_active_is_false_for_unknown_station() {
    let lv = LatestValues::new(STALE_S);
    assert!(
        !lv.station_is_active("never_polled", 1_000),
        "从未轮询成功的站 ⇒ false（不以 0 当有效时刻）"
    );
}

#[test]
fn station_last_poll_ms_is_none_before_any_poll_then_some() {
    let lv = LatestValues::new(STALE_S);

    assert_eq!(
        lv.station_last_poll_ms("bms"),
        None,
        "从未采集 ⇒ None（R-38：不臆造、不由消费方自维护第二真源）"
    );

    lv.mark_station_polled("bms", 1_234);
    assert_eq!(lv.station_last_poll_ms("bms"), Some(1_234));

    // 与 station_is_active 同源（二者不得各自判据）
    assert!(lv.station_is_active("bms", 1_234));
}

// ── ⑤ 站离线后的读行为（EX-1）─────────────────────────────────────────

#[test]
fn mark_station_offline_keeps_value_and_ts_but_flags_invalid() {
    let lv = LatestValues::new(STALE_S);
    let soc = pid("bms", "soc");
    let other = pid("hvac", "hvac_in_1");
    lv.apply(vec![
        (soc.clone(), pv(55.0, 100, PointQuality::Ok)),
        (other.clone(), pv(3.0, 100, PointQuality::Ok)),
    ]);
    lv.mark_station_polled("bms", 100);

    lv.mark_station_offline("bms");

    let got = lv.get(&soc);
    assert_eq!(got.value.quality, PointQuality::Invalid, "全站置 Invalid");
    assert_eq!(
        got.value.value,
        Some(55.0),
        "**保原值**（EX-1，不清空、不补 0）"
    );
    assert_eq!(got.value.ts_ms, 100, "**保原始时标**（EX-1）");

    assert_eq!(lv.station_last_poll_ms("bms"), None, "清站活性");
    assert!(!lv.station_is_active("bms", 100));
    assert!(
        !lv.is_fresh(&soc, &got.value.clone(), false, 100),
        "离线站的点必不新鲜（即使 ts 在窗内）"
    );

    // 站级隔离：其余站不受影响
    assert_eq!(lv.get(&other).value.quality, PointQuality::Ok);
    assert!(
        lv.station_snapshot("bms")
            .iter()
            .all(|v| v.value.quality == PointQuality::Invalid),
        "该站全部点（含从未采集过而缺席者之外的已登记点）整体 Invalid"
    );
}

// ── 新鲜度判据（标量逐点 / 位点走站活性）──────────────────────────────

#[test]
fn is_fresh_scalar_uses_point_ts_and_stale_timeout() {
    let lv = LatestValues::new(STALE_S);
    let id = pid("meter_grid", "voltage");

    let fresh = pv(220.0, 1_000, PointQuality::Ok);
    assert!(lv.is_fresh(&id, &fresh, false, 6_000), "= 5 s 恰好在窗内");
    assert!(!lv.is_fresh(&id, &fresh, false, 6_001), "> 5 s ⇒ 过期");

    let stale_q = pv(220.0, 1_000, PointQuality::Stale);
    assert!(
        !lv.is_fresh(&id, &stale_q, false, 1_000),
        "质量非 Ok ⇒ 一律不新鲜（即使时标是新的）"
    );
}

#[test]
fn is_fresh_bit_depends_on_station_activity_not_point_ts() {
    let lv = LatestValues::new(STALE_S);
    let bit = pid("bms", "bms_io_225");

    // 位点按变化沿交付 ⇒ 时标恒为「最后变化时刻」，可能远旧
    let old_bit = pv(1.0, 0, PointQuality::Ok);

    lv.mark_station_polled("bms", 100_000);
    assert!(
        lv.is_fresh(&bit, &old_bit, true, 100_500),
        "位点可得性 = 站级轮询活性 ∧ quality==Ok（长跑装置稳态位点不得被判过期）"
    );

    // 站级活性出窗 ⇒ 位点不新鲜（即使 quality 仍 Ok、未见离线事件）
    assert!(
        !lv.is_fresh(&bit, &old_bit, true, 106_000),
        "站活性出窗 ⇒ 位点不新鲜"
    );

    // 同一时标下，标量按逐点判据（此处过期）与位点判据的差别正是裁定点
    assert!(!lv.is_fresh(&bit, &old_bit, false, 100_500));
}

// ── 并发（多写多读）────────────────────────────────────────────────────

#[test]
fn concurrent_writers_and_readers_stay_consistent() {
    let lv = Arc::new(LatestValues::new(STALE_S));
    let writers = 4;
    let per_writer = 100u64;

    let mut handles = Vec::new();
    for w in 0..writers {
        let lv = Arc::clone(&lv);
        handles.push(std::thread::spawn(move || {
            for i in 0..per_writer {
                let station = format!("st{w}");
                let metric = format!("m{i}");
                lv.apply(vec![(
                    pid(&station, &metric),
                    pv(i as f64, 1_000 + i, PointQuality::Ok),
                )]);
            }
        }));
    }
    for _ in 0..2 {
        let lv = Arc::clone(&lv);
        handles.push(std::thread::spawn(move || {
            for _ in 0..200 {
                let _ = lv.all();
                let _ = lv.station_snapshot("st0");
                let _ = lv.get(&pid("st1", "m0"));
            }
        }));
    }
    for h in handles {
        h.join().expect("读写线程不得 panic");
    }

    assert_eq!(lv.all().len(), (writers as u64 * per_writer) as usize);
    lv.mark_station_offline("st0");
    assert!(lv
        .station_snapshot("st0")
        .iter()
        .all(|v| v.value.quality == PointQuality::Invalid));
}
