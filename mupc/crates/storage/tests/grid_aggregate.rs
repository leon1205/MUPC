//! 总表电气量「1 分钟聚合落库」的**集成/库内**验收（03 设计 §9.1 / §9.7；PRD §11.2 / §11.7.1，U-69）。
//!
//! 纯逻辑单测（GRD-02/03/05 表断言、断连/重启/flush 等）在 `src/grid_aggregate.rs` 的
//! `#[cfg(test)] mod tests`；**本文件只放需要真库的断言**（GRD-01/04/05 库内/06/09、STG-05 结构）。
//!
//! `telemetry.value` 自 03 设计 §9.1.4 起**可空**：缺测行落**真 NULL**（不是 0），
//! 与「真 0 值」（`value = 0.0` + `quality = 0`）在库内可区分 —— 这正是 PRD R-11.2-E 的机械判据。

use mupc_storage::grid_aggregate::{GridAggregator, GridSample, Quality, CHANNELS};
use mupc_storage::services::*;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::sync::Arc;

async fn setup() -> (Arc<SqlitePool>, StorageService) {
    let pool = Arc::new(
        SqlitePoolOptions::new()
            .max_connections(2)
            .connect("sqlite::memory:")
            .await
            .expect("创建内存数据库"),
    );
    run_migrations(&pool).await.expect("迁移执行");
    let service = StorageService::new(pool.clone());
    (pool, service)
}

/// 18 通道全有值的样本（`p_total` / `q_total` 由分相和给出 ⇒ 极值可精确断言）。
fn full_sample(base: f64) -> GridSample {
    let p = [base, base + 1.0, base + 2.0];
    let q = [base + 3.0, base + 4.0, base + 5.0];
    GridSample {
        u: [Some(base + 10.0), Some(base + 11.0), Some(base + 12.0)],
        i: [Some(base + 13.0), Some(base + 14.0), Some(base + 15.0)],
        p: [Some(p[0]), Some(p[1]), Some(p[2])],
        q: [Some(q[0]), Some(q[1]), Some(q[2])],
        pf: [Some(base + 16.0), Some(base + 17.0), Some(base + 18.0)],
        p_total: Some(p.iter().sum()),
        q_total: Some(q.iter().sum()),
    }
}

/// 走装配层同一路径落库：`to_telemetry_point("grid_meter")` → `telemetry.insert`。
async fn sink_rows(svc: &StorageService, rows: &[mupc_storage::AggregateRow]) {
    for r in rows {
        svc.telemetry
            .insert(&r.to_telemetry_point("grid_meter"))
            .await
            .unwrap();
    }
}

/// **GRD-01**：每 1 分钟恰 1 个聚合周期（按 `timestamp` 分组计数 == 分钟数）。
///
/// ⚠️ **偏离声明**：PRD 的验证方法写「运行 ≥ 3 分钟」；此处用**模拟时标**回放 3 分钟
/// （每秒 1 个样本、共 181 个采样 ⇒ 恰 3 个闭合周期），以免给测试套件加 3 分钟墙钟耗时。
/// 周期边界只看 `ts_ms`（`observe` 不读时钟）⇒ 逻辑等价；**未覆盖**真挂钟下的调度抖动，
/// 留给实机联调。
#[tokio::test]
async fn grd01_three_minutes_yield_exactly_three_periods() {
    let (_, svc) = setup().await;
    let period = 60_000u64;
    let mut agg = GridAggregator::new(period);
    let mut rows = Vec::new();
    for step in 0..=180u64 {
        rows.extend(agg.observe(step * 1_000, &full_sample(0.0)));
    }
    sink_rows(&svc, &rows).await;

    // 时间戳是**周期起点**（模拟时标从 0 起 ⇒ epoch 附近），故查询区间按模拟时标给。
    let queried = svc
        .telemetry
        .query_range(
            "grid_meter",
            chrono::DateTime::from_timestamp_millis(0).unwrap(),
            chrono::DateTime::from_timestamp_millis(600_000).unwrap(),
        )
        .await
        .unwrap();
    let mut starts: Vec<i64> = queried
        .iter()
        .map(|p| p.timestamp.timestamp_millis())
        .collect();
    starts.sort_unstable();
    starts.dedup();
    assert_eq!(
        starts,
        vec![0, period as i64, 2 * period as i64],
        "3 分钟恰 3 个聚合周期（按时间戳分组）"
    );
    assert_eq!(queried.len(), 3 * 22, "每周期 22 行");
}

/// **GRD-04（库内断言）**：无采样周期 ⇒ 22 行存在、`quality = 1(NoData)`、`value IS NULL`；
/// 对比：真实 0 值采样 ⇒ `quality = 0`、`value = 0.0` ⇒ **二者在库内可区分**。
#[tokio::test]
async fn grd04_nodata_rows_are_null_and_distinguishable_from_real_zero() {
    let (pool, svc) = setup().await;
    // ① 无采样周期：tick 锚定 + 跨 1 周期（站离线在采）
    let mut agg = GridAggregator::new(60_000);
    assert!(agg.tick(0).is_empty());
    let nodata_rows = agg.tick(60_000);
    assert_eq!(nodata_rows.len(), 22);
    sink_rows(&svc, &nodata_rows).await;

    // ② 真实 0 值采样周期（另一 device_id，避免与 ① 混在同一时间戳）
    let zero = GridSample {
        p_total: Some(0.0),
        q_total: Some(0.0),
        ..Default::default()
    };
    let mut agg2 = GridAggregator::new(60_000);
    agg2.observe(0, &zero);
    for r in &agg2.tick(60_000) {
        svc.telemetry
            .insert(&r.to_telemetry_point("grid_meter_zero"))
            .await
            .unwrap();
    }

    // ① 库内：22 行、全为真 NULL、quality 全 1
    let raw = sqlx::query_as::<_, (i64, Option<i64>, i32)>(
        "SELECT COUNT(*), SUM(value IS NULL), MIN(quality) FROM telemetry
         WHERE device_id = 'grid_meter' AND timestamp = 0",
    )
    .fetch_one(pool.as_ref())
    .await
    .unwrap();
    assert_eq!(raw.0, 22, "无采样周期仍产出 22 行（断档可查，不静默少行）");
    assert_eq!(raw.1, Some(22), "22 行的 value 全为真 NULL");
    assert_eq!(raw.2, 1, "quality 全为 NoData(1)");

    // ② 对比：同一周期内，「**真 0 值**」的通道（本样本只给了 p_total/q_total）与
    //    「缺测」的通道（未给的 u_a 等）在**同一张表里可区分**。
    let zero_row = sqlx::query_as::<_, (Option<f64>, i32)>(
        "SELECT value, quality FROM telemetry
         WHERE device_id = 'grid_meter_zero' AND timestamp = 0 AND metric_name = 'p_total'",
    )
    .fetch_one(pool.as_ref())
    .await
    .unwrap();
    assert_eq!(zero_row, (Some(0.0), 0), "真 0 值：value = 0.0 + Good");
    let missing_row = sqlx::query_as::<_, (Option<f64>, i32)>(
        "SELECT value, quality FROM telemetry
         WHERE device_id = 'grid_meter_zero' AND timestamp = 0 AND metric_name = 'u_a'",
    )
    .fetch_one(pool.as_ref())
    .await
    .unwrap();
    assert_eq!(missing_row, (None, 1), "缺测：value IS NULL + NoData(1)");
    // 22 行里 6 行有值（p_total/q_total 的均值与极值），其余 16 行为真 NULL
    let zero_null: Option<i64> = sqlx::query_scalar(
        "SELECT SUM(value IS NULL) FROM telemetry
         WHERE device_id = 'grid_meter_zero' AND timestamp = 0",
    )
    .fetch_one(pool.as_ref())
    .await
    .unwrap();
    assert_eq!(zero_null, Some(16));

    // ③ 仓储层读回：NULL 行 `value == None`，与 `Some(0.0)` 可区分
    let nodata = svc
        .telemetry
        .get_latest("grid_meter", "p_total")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(nodata.value, None);
    assert_eq!(nodata.quality, 1);
    let zero_back = svc
        .telemetry
        .get_latest("grid_meter_zero", "p_total")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(zero_back.value, Some(0.0));
    assert_eq!(zero_back.quality, 0);
}

/// **GRD-05（库内逐通道）**：落库通道集合 == 表（18 均值 + 4 极值）；缺测通道
/// `quality = NoData` 且**无 0 冒充**。
#[tokio::test]
async fn grd05_persisted_channel_set_equals_table_and_missing_is_nodata() {
    let (pool, svc) = setup().await;
    let mut agg = GridAggregator::new(60_000);
    let mut s = full_sample(0.0);
    s.i[1] = None; // i_b 本周期缺测
    s.p_total = Some(7.0);
    agg.observe(0, &s);
    sink_rows(&svc, &agg.tick(60_000)).await;

    let names: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT metric_name FROM telemetry WHERE device_id = 'grid_meter'
         ORDER BY metric_name",
    )
    .fetch_all(pool.as_ref())
    .await
    .unwrap();
    let mut expected: Vec<String> = CHANNELS.iter().map(|c| c.metric.to_string()).collect();
    expected.push("p_total_max".into());
    expected.push("p_total_min".into());
    expected.push("q_total_max".into());
    expected.push("q_total_min".into());
    expected.sort();
    assert_eq!(names, expected, "落库通道集合 == 表（18 均值 + 4 极值）");

    // 缺测通道：NULL + NoData（**不是 0**）
    let ib = svc
        .telemetry
        .get_latest("grid_meter", "i_b")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ib.value, None);
    assert_eq!(ib.quality, Quality::NoData.code());
    // 有值通道：Good
    let ia = svc
        .telemetry
        .get_latest("grid_meter", "i_a")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ia.value, Some(13.0));
    assert_eq!(ia.quality, Quality::Good.code());
    // 极值行有值（p_total 恒定 7.0）
    let p_max = svc
        .telemetry
        .get_latest("grid_meter", "p_total_max")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p_max.value, Some(7.0));
}

/// **GRD-06**：行数下降可量化（同口径：1 分钟聚合 vs 21 通道逐点 @1 s）。
///
/// PRD 方法写「连续 1 h 实测外推」；此处按**同一算术口径**直接断言（1260 → 22 行/分钟，
/// 下降 98.3% ≥ 95%），不引入墙钟依赖。实机外推项留给联调。
#[test]
fn grd06_row_count_reduction_is_at_least_95_percent() {
    let per_point_rows_per_minute = 21 * 60; // 21 通道 × 60 次/分钟（PRD §11.2-F 口径）
    let aggregated_rows_per_minute = GridAggregator::new(60_000).rows_per_period();
    assert_eq!(aggregated_rows_per_minute, 22);
    let drop = 1.0 - (aggregated_rows_per_minute as f64) / (per_point_rows_per_minute as f64);
    assert!(drop >= 0.95, "行数下降 {drop:.3} 必须 ≥ 95%");
}

/// **GRD-09**：`telemetry.value` 可空化迁移的**幂等**。
///
/// 老库（`value REAL NOT NULL` + 两个索引 + 既有行）跑 `run_migrations` ⇒
/// ① `value` 变可空、既有行数值**逐行不变**；
/// ② 再跑一次**不重复重建** —— **判别判据 = 表的 `rootpage` 不变**（重建必然换页）。
///    ⚠️ 原稿写的判据"没有 PRAGMA 判定时第二次会因 `telemetry_old` 已存在而报错"**不成立**
///    （T15/T16 评审探针实录：`telemetry_old` 在同一事务里已被 DROP ⇒ 抹掉守卫仍绿
///    ⇒ 那是假保证）；故改用 rootpage 断言把"是否真重建过"钉死。
/// ③ **两个**索引都重放（只重放一个 ⇒ 第二次后 `sqlite_master` 少一条）。
#[tokio::test]
async fn grd09_value_nullable_migration_is_idempotent() {
    // 单连接内存库：手工建**老库形态**再跑迁移（表已存在 ⇒ 建表语句被 IF NOT EXISTS 跳过，
    // 只有可空化迁移会动它 —— 这正是现场升级路径）。
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("创建内存数据库");
    sqlx::query(
        "CREATE TABLE telemetry (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            metric_name TEXT NOT NULL,
            value REAL NOT NULL,
            quality INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    for ddl in [
        "CREATE INDEX idx_telemetry_device_ts ON telemetry(device_id, timestamp)",
        "CREATE INDEX idx_telemetry_metric_ts ON telemetry(metric_name, timestamp)",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    sqlx::query(
        "INSERT INTO telemetry (device_id, timestamp, metric_name, value, quality)
         VALUES ('dev1', 1234, 'v', 42.5, 0), ('dev1', 1235, 'v', 0.0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ① 首次迁移：可空化 + 数据逐行等值
    run_migrations(&pool).await.expect("首次迁移");
    assert_eq!(value_notnull(&pool).await, 0, "迁移后 value 必须可空");
    let rows = sqlx::query_as::<_, (i64, i64, Option<f64>, i32)>(
        "SELECT id, timestamp, value, quality FROM telemetry ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2, "既有行全部保留");
    assert_eq!(rows[0], (1, 1234, Some(42.5), 0), "既有行数值不变（数据语义不变）");
    assert_eq!(rows[1], (2, 1235, Some(0.0), 0), "真 0 值仍是 0.0（不是 NULL）");

    // ② 幂等：二次迁移必须通过且不改变行数/数值
    //    判别判据：`rootpage` 不变 ⇒ 没有发生"搬数据重建"（抹掉 PRAGMA 守卫会换页 ⇒ 本断言红）
    let rootpage_before: i64 =
        sqlx::query_scalar("SELECT rootpage FROM sqlite_master WHERE type='table' AND name='telemetry'")
            .fetch_one(&pool)
            .await
            .unwrap();
    run_migrations(&pool).await.expect("二次迁移必须幂等通过");
    let rootpage_after: i64 =
        sqlx::query_scalar("SELECT rootpage FROM sqlite_master WHERE type='table' AND name='telemetry'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        rootpage_before, rootpage_after,
        "二次迁移不得重建 telemetry（rootpage 换页即说明又搬了一次数据）"
    );
    assert_eq!(value_notnull(&pool).await, 0);
    let count: (i64, Option<i64>) =
        sqlx::query_as("SELECT COUNT(*), SUM(value IS NULL) FROM telemetry")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, (2, Some(0)), "二次迁移后行数不变、无 NULL 行");

    // ②' 自增序列不倒退：重建后新插入的行 id 必须 > 既有最大 id（否则会与既有行撞 id）
    sqlx::query("INSERT INTO telemetry (device_id, timestamp, metric_name, value, quality) VALUES ('dev1', 1236, 'v', 1.0, 0)")
        .execute(&pool)
        .await
        .unwrap();
    let max_id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM telemetry")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(max_id, 3, "可空化重建后 AUTOINCREMENT 序列必须接着既有最大 id（不得从 1 重来）");

    // ③ 两个索引都在（03 设计 §9.1.4 勘误 ②：只重建一个会让 device_id+timestamp 路径丢索引）
    let idx: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'telemetry'
         AND name LIKE 'idx_%' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        idx,
        vec![
            "idx_telemetry_device_ts".to_string(),
            "idx_telemetry_metric_ts".to_string()
        ],
        "可空化重建后两个索引都必须存在"
    );

    // ④ 新装库（新 DDL 建出即可空）再跑迁移：走「notnull == 0 ⇒ 跳过」分支，同样通过
    let (_, svc) = setup().await;
    let _ = svc;
}

/// **GRD-09b**：老库**行已全被清空**时，可空化重建后的 AUTOINCREMENT 序列**仍不得倒退**。
///
/// 这正是 §9.1.4 那条"序列不倒退"断言的**边界**（T15/T16 评审残留②）：`RENAME` + 重建
/// 会把 `sqlite_sequence` 一并带走，而旧表无行可搬 ⇒ 若不显式恢复序列，新表 id 会从 1 重发。
/// 本用例先删空老表再迁移，插一行断言 id > 旧最大 id ⇒ **无"取旧序列并单调恢复"的实现必红**。
#[tokio::test]
async fn grd09b_sequence_preserved_when_old_table_emptied() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("创建内存数据库");
    sqlx::query(
        "CREATE TABLE telemetry (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            metric_name TEXT NOT NULL,
            value REAL NOT NULL,
            quality INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO telemetry (device_id, timestamp, metric_name, value, quality)
         VALUES ('d', 1, 'v', 1.0, 0), ('d', 2, 'v', 2.0, 0), ('d', 3, 'v', 3.0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // 清空（`sqlite_sequence` 保留 3）；老库形态仍是 NOT NULL
    sqlx::query("DELETE FROM telemetry")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(value_notnull(&pool).await, 1, "前提：老库 value 仍为 NOT NULL");

    run_migrations(&pool).await.expect("迁移（空表路径）");
    assert_eq!(value_notnull(&pool).await, 0, "迁移后 value 可空");

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO telemetry (device_id, timestamp, metric_name, value, quality)
         VALUES ('d', 4, 'v', 4.0, 0) RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        id > 3,
        "空表迁移后序列不得倒退（新行 id 应 > 旧最大 id 3，实际 {id}）"
    );
}

async fn value_notnull(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT \"notnull\" FROM pragma_table_info('telemetry') WHERE name = 'value'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// **STG-06**：`storage:` 段**不进 DB** —— 库里没有为此新增表 / 覆写项，聚合记录仍落既有
/// `telemetry` 窄表（靠 `device_id` 区分，§9.1.4「不新建表、不加迁移」）。
#[tokio::test]
async fn stg06_storage_section_creates_no_new_table_or_db_override() {
    let (pool, _) = setup().await;
    let mut tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    )
    .fetch_all(pool.as_ref())
    .await
    .unwrap();
    tables.sort();
    assert_eq!(
        tables,
        vec![
            "action_space_config".to_string(),
            "assets".to_string(),
            "decisions".to_string(),
            "events".to_string(),
            "faults".to_string(),
            "telemetry".to_string(),
        ],
        "`storage:` 段的落地**不新增表**（既没有 storage_config 一类表，也没有覆写项）"
    );
    // 聚合记录走既有窄表：`value` 列的 notnull 判定（可空化的唯一结构变更）
    let notnull: i64 = sqlx::query_scalar(
        "SELECT \"notnull\" FROM pragma_table_info('telemetry') WHERE name = 'value'",
    )
    .fetch_one(pool.as_ref())
    .await
    .unwrap();
    assert_eq!(notnull, 0);
}

/// **STG-05**：两参数**正交**（`batch_capacity`/`flush_interval_ms` = 提交节拍；
/// `grid_aggregate_period_ms` = 记录时间粒度）。
///
/// ① **行为断言**：只改提交节拍 ⇒ 周期粒度不变；只改周期 ⇒ 提交信息面不变。
/// ② **结构断言**：两个类型**互不引用**（源码级 —— 挡住「把其中一个塞进另一个」的演进）。
#[tokio::test]
async fn stg05_commit_tempo_and_record_granularity_are_independent() {
    // 懒连接池：只做**信息面**检查（不真连库）⇒ 不去建/迁移数据库。
    let pool = Arc::new(
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_lazy("sqlite::memory:")
            .expect("懒连接池"),
    );
    // ① 提交节拍只影响 WriteBuffer 自己的信息面
    let wb = WriteBuffer::new(200, 2_000, pool);
    assert_eq!(wb.capacity(), 200);
    assert_eq!(wb.flush_interval_ms(), 2_000);
    // 聚合周期只影响周期起点：300_000 在 60 s 周期下对齐到自身，在 120 s 周期下对齐到 240_000
    let mut a = GridAggregator::new(60_000);
    a.observe(300_000, &full_sample(0.0));
    assert_eq!(a.current_start_ms(), Some(300_000));
    let mut b = GridAggregator::new(120_000);
    b.observe(300_000, &full_sample(0.0));
    assert_eq!(b.current_start_ms(), Some(240_000));
    // ② 结构断言：聚合器不提提交节拍；提交缓冲不提聚合器
    let agg_src = include_str!("../src/grid_aggregate.rs");
    let svc_src = include_str!("../src/services.rs");
    assert!(
        !agg_src.contains("WriteBuffer"),
        "聚合器不得引用提交节拍类型（两量正交由结构保证，03 设计 §9.2.2 末）"
    );
    assert!(
        !svc_src.contains("GridAggregator"),
        "提交缓冲不得引用聚合器（两量正交由结构保证，03 设计 §9.2.2 末）"
    );
}
