/// storage crate 集成测试 — 覆盖全部 5 个 Repository + WriteBuffer + RetentionManager
use chrono::{Duration, Utc};
use mupc_storage::errors::StorageError;
use mupc_storage::models::*;
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

fn make_telemetry(device: &str, metric: &str, value: f64) -> TelemetryPoint {
    TelemetryPoint {
        id: None,
        device_id: device.to_string(),
        timestamp: Utc::now(),
        metric_name: metric.to_string(),
        // `value` 自 03 设计 §9.1.4 起可空（`None` = 缺测）；本文件的写入方一律**有值**。
        value: Some(value),
        quality: 0,
    }
}

fn make_fault(device: &str, fault_type: &str, severity: i32) -> FaultEvent {
    FaultEvent {
        id: None,
        device_id: device.to_string(),
        timestamp: Utc::now(),
        fault_type: fault_type.to_string(),
        severity,
        waveform_path: None,
        acknowledged: false,
    }
}

fn make_decision(scene: &str, action: &str) -> AiDecisionRecord {
    AiDecisionRecord {
        id: None,
        timestamp: Utc::now(),
        scene_type: scene.to_string(),
        action_json: action.to_string(),
        confidence: 0.95,
        model_version: "v1.0".to_string(),
    }
}

fn make_event(event_type: &str, source: &str, msg: &str) -> SystemEvent {
    SystemEvent {
        id: None,
        timestamp: Utc::now(),
        event_type: event_type.to_string(),
        source: source.to_string(),
        message: msg.to_string(),
    }
}

fn make_asset(device_id: &str, device_type: &str) -> AssetRecord {
    AssetRecord {
        id: None,
        device_id: device_id.to_string(),
        device_type: device_type.to_string(),
        manufacturer: "test".to_string(),
        model: "test".to_string(),
        firmware_version: "1.0".to_string(),
        installed_at: Utc::now(),
        last_maintenance: None,
    }
}

// ── TelemetryRepository ──

#[tokio::test]
async fn telemetry_insert_and_query() {
    let (_, svc) = setup().await;
    let id = svc
        .telemetry
        .insert(&make_telemetry("dev1", "voltage", 220.0))
        .await
        .unwrap();
    assert!(id > 0);

    let latest = svc.telemetry.get_latest("dev1", "voltage").await.unwrap();
    assert!(latest.is_some());
    // `value` 可空化后须先解出 `Option`（既有写入方都是有值 ⇒ `expect` 即原断言语义）。
    assert!((latest.unwrap().value.expect("有值") - 220.0).abs() < 0.01);
}

#[tokio::test]
async fn telemetry_query_range() {
    let (_, svc) = setup().await;
    let now = Utc::now();
    let t1 = TelemetryPoint {
        id: None,
        device_id: "dev1".into(),
        timestamp: now - Duration::hours(1),
        metric_name: "v".into(),
        value: Some(1.0),
        quality: 0,
    };
    let t2 = TelemetryPoint {
        id: None,
        device_id: "dev1".into(),
        timestamp: now,
        metric_name: "v".into(),
        value: Some(2.0),
        quality: 0,
    };
    svc.telemetry.insert(&t1).await.unwrap();
    svc.telemetry.insert(&t2).await.unwrap();

    let results = svc
        .telemetry
        .query_range("dev1", now - Duration::hours(2), now + Duration::minutes(1))
        .await
        .unwrap();
    assert!(!results.is_empty());
}

#[tokio::test]
async fn telemetry_get_latest_none() {
    let (_, svc) = setup().await;
    let result = svc.telemetry.get_latest("nonexistent", "v").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn telemetry_delete_older_than() {
    let (_, svc) = setup().await;
    let old = TelemetryPoint {
        id: None,
        device_id: "dev1".into(),
        timestamp: Utc::now() - Duration::days(100),
        metric_name: "v".into(),
        value: Some(0.0),
        quality: 0,
    };
    svc.telemetry.insert(&old).await.unwrap();
    let deleted = svc
        .telemetry
        .delete_older_than(Utc::now() - Duration::days(50))
        .await
        .unwrap();
    assert!(deleted > 0);
}

// ── FaultRepository ──

#[tokio::test]
async fn fault_insert_and_acknowledge() {
    let (_, svc) = setup().await;
    let id = svc
        .faults
        .insert(&make_fault("dev1", "overcurrent", 3))
        .await
        .unwrap();
    assert!(id > 0);

    svc.faults.acknowledge(id).await.unwrap();

    let results = svc
        .faults
        .query_range(
            Utc::now() - Duration::hours(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert!(!results.is_empty());
    assert!(results[0].acknowledged);
}

#[tokio::test]
async fn fault_acknowledge_not_found() {
    let (_, svc) = setup().await;
    let err = svc.faults.acknowledge(99999).await.unwrap_err();
    match err {
        StorageError::NotFound(_) => {}
        _ => panic!("expected NotFound"),
    }
}

#[tokio::test]
async fn fault_query_empty() {
    let (_, svc) = setup().await;
    let results = svc
        .faults
        .query_range(Utc::now() - Duration::days(1), Utc::now())
        .await
        .unwrap();
    assert!(results.is_empty());
}

// ── DecisionRepository ──

#[tokio::test]
async fn decision_insert_and_query() {
    let (_, svc) = setup().await;
    svc.decisions
        .insert(&make_decision(
            "AgriculturalIrrigation",
            r#"{"p_batt": 10}"#,
        ))
        .await
        .unwrap();
    svc.decisions
        .insert(&make_decision("DemandControl", r#"{"p_batt": 5}"#))
        .await
        .unwrap();

    let recent = svc.decisions.query_recent(10).await.unwrap();
    assert_eq!(recent.len(), 2);

    let by_scene = svc
        .decisions
        .get_by_scene("AgriculturalIrrigation", 10)
        .await
        .unwrap();
    assert_eq!(by_scene.len(), 1);
}

// ── EventRepository ──

#[tokio::test]
async fn event_insert_and_query() {
    let (_, svc) = setup().await;
    svc.events
        .insert(&make_event("startup", "system", "boot complete"))
        .await
        .unwrap();

    let results = svc
        .events
        .query_range(
            Utc::now() - Duration::hours(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert!(!results.is_empty());
}

#[tokio::test]
async fn event_latest_by_type_returns_most_recent() {
    let (_, svc) = setup().await;
    let now = Utc::now();
    let older = SystemEvent {
        id: None,
        timestamp: now - Duration::hours(1),
        event_type: "interlock.triggered".into(),
        source: "pcs".into(),
        message: "older".into(),
    };
    let newer = SystemEvent {
        id: None,
        timestamp: now,
        event_type: "interlock.triggered".into(),
        source: "pcs".into(),
        message: "newer".into(),
    };
    let cleared = SystemEvent {
        id: None,
        timestamp: now + Duration::minutes(1),
        event_type: "interlock.cleared".into(),
        source: "pcs".into(),
        message: "cleared".into(),
    };
    svc.events.insert(&older).await.unwrap();
    let id_newer = svc.events.insert(&newer).await.unwrap();
    svc.events.insert(&cleared).await.unwrap();
    // 诱饵：**后插入但 timestamp 更早**（id 更大、ts 介于 older/newer）——隔离「按 timestamp 排序」
    // 与「按 id/插入序排序」语义：若实现退化为 ORDER BY id DESC 应返回此诱饵而非 newer。
    svc.events
        .insert(&SystemEvent {
            id: None,
            timestamp: now - Duration::minutes(30),
            event_type: "interlock.triggered".into(),
            source: "pcs".into(),
            message: "bait".into(),
        })
        .await
        .unwrap();

    // 同类型多条 → 取 timestamp 最大的一条（须为 newer，而非后插入的 bait）
    let latest = svc
        .events
        .latest_by_type("interlock.triggered")
        .await
        .unwrap()
        .expect("应返回 triggered 最新一条");
    assert_eq!(latest.id, Some(id_newer));
    assert_eq!(latest.message, "newer");

    // 不同事件类型互不干扰
    let latest_cleared = svc
        .events
        .latest_by_type("interlock.cleared")
        .await
        .unwrap()
        .expect("应返回 cleared 一条");
    assert_eq!(latest_cleared.message, "cleared");
}

#[tokio::test]
async fn event_latest_by_type_none_when_absent() {
    let (_, svc) = setup().await;
    let result = svc
        .events
        .latest_by_type("interlock.triggered")
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn event_purge() {
    let (_, svc) = setup().await;
    let old = SystemEvent {
        id: None,
        timestamp: Utc::now() - Duration::days(200),
        event_type: "old".into(),
        source: "s".into(),
        message: "m".into(),
    };
    svc.events.insert(&old).await.unwrap();
    let deleted = svc
        .events
        .purge_older_than(Utc::now() - Duration::days(100))
        .await
        .unwrap();
    assert!(deleted > 0);
}

// ── AssetRepository ──

#[tokio::test]
async fn asset_upsert_and_get() {
    let (_, svc) = setup().await;
    let asset = make_asset("dev-001", "inverter");
    let id = svc.assets.upsert(&asset).await.unwrap();
    assert!(id > 0);

    let found = svc.assets.get_by_device_id("dev-001").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().device_type, "inverter");
}

#[tokio::test]
async fn asset_upsert_update_existing() {
    let (_, svc) = setup().await;
    svc.assets
        .upsert(&make_asset("dev-002", "ttu"))
        .await
        .unwrap();
    let mut updated = make_asset("dev-002", "ttu");
    updated.firmware_version = "2.0".to_string();
    svc.assets.upsert(&updated).await.unwrap();

    let found = svc
        .assets
        .get_by_device_id("dev-002")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.firmware_version, "2.0");
}

#[tokio::test]
async fn asset_list_by_type() {
    let (_, svc) = setup().await;
    svc.assets
        .upsert(&make_asset("dev-a", "inverter"))
        .await
        .unwrap();
    svc.assets
        .upsert(&make_asset("dev-b", "inverter"))
        .await
        .unwrap();
    svc.assets
        .upsert(&make_asset("dev-c", "charger"))
        .await
        .unwrap();

    let inverters = svc.assets.list_by_type("inverter").await.unwrap();
    assert_eq!(inverters.len(), 2);
}

#[tokio::test]
async fn asset_list_all() {
    let (_, svc) = setup().await;
    svc.assets.upsert(&make_asset("d1", "t")).await.unwrap();
    svc.assets.upsert(&make_asset("d2", "t")).await.unwrap();
    assert_eq!(svc.assets.list_all().await.unwrap().len(), 2);
}

// ── WriteBuffer ──

#[tokio::test]
async fn writebuffer_flush_on_capacity() {
    let (pool, svc) = setup().await;
    let wb = WriteBuffer::new(3, 1000, pool);
    for i in 0..5 {
        wb.buffer_telemetry(make_telemetry("dev-wb", "v", i as f64))
            .await
            .unwrap();
    }

    let results = svc
        .telemetry
        .query_range(
            "dev-wb",
            Utc::now() - Duration::minutes(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert!(!results.is_empty());
}

/// P0-1 ①（时间触发半边）：**未凑满容量**，仅靠时间窗口到期即提交。
///
/// 设计 03:1321「100ms 或积累 100 条触发批量事务提交（**先到先执行**）」——本用例钉的是
/// 「先到」里的**时间**这一半。修复前 `flush_interval_ms` 无读取方 ⇒ 只写 1 点（容量 1000
/// 远未到）时数据永远停在内存里 ⇒ 本用例红。
#[tokio::test]
async fn writebuffer_flush_timer_commits_without_full_capacity() {
    let (pool, svc) = setup().await;
    // capacity=1000（远未达到）+ flush_interval=50ms
    let wb = Arc::new(WriteBuffer::new(1000, 50, pool));
    wb.buffer_telemetry(make_telemetry("dev-timer", "v", 1.0))
        .await
        .unwrap();

    // 停机信号只发不收（U-64）：本用例只验周期触发，不发停机 ⇒ 传一个"永不停机"的接收端。
    let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let timer = wb.clone().spawn_flush_timer(stop_rx);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    timer.abort();

    let results = svc
        .telemetry
        .query_range(
            "dev-timer",
            Utc::now() - Duration::minutes(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert_eq!(
        results.len(),
        1,
        "50ms 时间窗到期即应提交（容量 1000 远未达到 ⇒ 只可能是时间触发）"
    );
    assert_eq!(
        wb.flush().await.unwrap(),
        0,
        "定时任务应已排空缓冲，显式 flush 无残留可提交"
    );
}

/// P0-1 ①（周期**可重复**，不是一次性）：连续两个周期各自提交各自的批次。
#[tokio::test]
async fn writebuffer_flush_timer_is_periodic() {
    let (pool, svc) = setup().await;
    let wb = Arc::new(WriteBuffer::new(1000, 50, pool));
    let (_stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let timer = wb.clone().spawn_flush_timer(stop_rx);

    let wide = (
        Utc::now() - Duration::minutes(1),
        Utc::now() + Duration::minutes(1),
    );

    wb.buffer_telemetry(make_telemetry("dev-periodic", "v", 1.0))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let first = svc
        .telemetry
        .query_range("dev-periodic", wide.0, wide.1)
        .await
        .unwrap();
    assert_eq!(first.len(), 1, "第一个周期应已提交");

    wb.buffer_telemetry(make_telemetry("dev-periodic", "v", 2.0))
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    timer.abort();
    let second = svc
        .telemetry
        .query_range("dev-periodic", wide.0, wide.1)
        .await
        .unwrap();
    assert_eq!(
        second.len(),
        2,
        "第二个周期同样应生效（定时任务不是一次性）"
    );
}

#[tokio::test]
async fn writebuffer_manual_flush() {
    let (pool, svc) = setup().await;
    let wb = WriteBuffer::new(100, 1000, pool);
    wb.buffer_telemetry(make_telemetry("dev-fl", "v", 1.0))
        .await
        .unwrap();
    wb.buffer_telemetry(make_telemetry("dev-fl", "v", 2.0))
        .await
        .unwrap();

    let count = wb.flush().await.unwrap();
    assert_eq!(count, 2);

    let results = svc
        .telemetry
        .query_range(
            "dev-fl",
            Utc::now() - Duration::minutes(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn writebuffer_empty_flush() {
    let (pool, _svc) = setup().await;
    let wb = WriteBuffer::new(100, 1000, pool);
    let count = wb.flush().await.unwrap();
    assert_eq!(count, 0);
}

// ── WriteBuffer 失败语义（U-68③：落库失败不得静默丢弃）──
//
// 失败注入方式 = **真实失败**（文件库里**不建表** ⇒ `INSERT INTO telemetry` 必 Err），
// 不用 mock：要证的是真实提交路径的行为，而不是测试桩自己的行为。
// 为什么用文件库而不是 `sqlite::memory:`：内存库与连接一一对应，`run_migrations` 只建在
// 其中一条连接上，"先失败、后恢复"的用例会因换连接而变成非确定性（假红/假绿）。

/// 文件型 SQLite 池（**未跑迁移**）：插 `telemetry` 必失败。
async fn bare_file_pool(tag: &str) -> (Arc<SqlitePool>, std::path::PathBuf) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "mupc_storage_wb_{tag}_{}_{nanos}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    // sqlx 的 SQLite 缺省打开模式不会**创建**文件（生产装配同样先 `File::create`）⇒ 必须先建空文件
    std::fs::File::create(&path).expect("建空 db 文件");
    let pool = mupc_storage::init_pool(path.to_str().unwrap())
        .await
        .expect("建文件库");
    (Arc::new(pool), path)
}

fn values_of(rows: &[TelemetryPoint]) -> Vec<f64> {
    // `value` 自 03 设计 §9.1.4 起可空：本文件的写入方**全部有值** ⇒ `filter_map` 与旧行为
    // 等价（若哪天写了 `None`，下面的 `assert_eq!(values_of(..), vec![..])` 会因条数少而红）。
    let mut v: Vec<f64> = rows.iter().filter_map(|p| p.value).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// **U-68③ ①**：写失败 ⇒ 该批**不丢**。故障排除后，**同一批**（而非重采的新点）被写入。
///
/// 改什么会让本条红：把 `flush_batch` 的失败分支改回"整批丢弃"（drain 走掉就不还）⇒
/// `buffered_points()` 归零、第二次 flush 提交 0 条、库里 0 行。
#[tokio::test]
async fn writebuffer_flush_failure_keeps_batch_until_next_success() {
    let (pool, path) = bare_file_pool("keep").await;
    let svc = StorageService::new(pool.clone());
    // capacity=100 ⇒ 2 个点不会触发容量提交
    let wb = WriteBuffer::new(100, 1000, pool.clone());
    wb.buffer_telemetry(make_telemetry("dev-keep", "v", 1.0))
        .await
        .unwrap();
    wb.buffer_telemetry(make_telemetry("dev-keep", "v", 2.0))
        .await
        .unwrap();

    let err = wb.flush().await.expect_err("未建表 ⇒ 提交必须失败");
    assert!(
        err.to_string().contains("telemetry") || err.to_string().contains("no such table"),
        "失败原因应确为「表不存在」（证明注入的是真实失败）: {err}"
    );
    assert_eq!(
        wb.buffered_points(),
        2,
        "写失败后这 2 条必须仍在缓冲（不得整批丢弃）"
    );
    assert_eq!(wb.dropped_points(), 0, "未超上限 ⇒ 不得丢点");
    assert_eq!(wb.requeued_batches(), 1, "失败批次必须回填（可观测）");

    // 故障排除（建表）后重试 ⇒ 回填的那一批被写入
    run_migrations(&pool).await.unwrap();
    assert_eq!(
        wb.flush().await.unwrap(),
        2,
        "重试必须提交回填的同一批（不是重采）"
    );

    let rows = svc
        .telemetry
        .query_range(
            "dev-keep",
            Utc::now() - Duration::minutes(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert_eq!(values_of(&rows), vec![1.0, 2.0], "两条原值都必须落库");
    assert_eq!(wb.buffered_points(), 0, "提交成功后缓冲排空");
    let _ = std::fs::remove_file(path);
}

/// **U-68③ ②**：超上限 ⇒ 丢**最旧**的 + 计数递增（而不是丢新来的、也不是无限增长）。
///
/// 改什么会让本条红：去掉上限 ⇒ `buffered_points()` 涨到 5（第 1 点仍在）、`dropped_points()`
/// 恒 0；或改成丢**最新**的 ⇒ 落库的值会是 `[1,2,3,4]` 而不是 `[2,3,4,5]`。
#[tokio::test]
async fn writebuffer_drops_oldest_and_counts_when_over_limit() {
    let (pool, path) = bare_file_pool("overflow").await;
    let svc = StorageService::new(pool.clone());
    // 上限 4 点；capacity=100 ⇒ 只由"超上限"触发裁剪，不受容量触发干扰
    let wb = WriteBuffer::new_with_max_points(100, 1000, pool.clone(), 4);
    assert_eq!(wb.max_points(), 4);

    for v in 1..=3 {
        wb.buffer_telemetry(make_telemetry("dev-ovf", "v", v as f64))
            .await
            .unwrap();
    }
    assert!(wb.flush().await.is_err(), "首批必失败（未建表）");
    assert_eq!(wb.buffered_points(), 3, "失败批次回填后为 3 点");

    wb.buffer_telemetry(make_telemetry("dev-ovf", "v", 4.0))
        .await
        .unwrap();
    assert_eq!(wb.buffered_points(), 4, "正好到上限");
    assert_eq!(wb.dropped_points(), 0, "未超上限不得丢任何点");

    wb.buffer_telemetry(make_telemetry("dev-ovf", "v", 5.0))
        .await
        .unwrap();
    assert_eq!(wb.buffered_points(), 4, "超上限后仍被钳在上限");
    assert_eq!(wb.dropped_points(), 1, "只丢溢出的 1 条");
    assert_eq!(wb.dropped_batches(), 1, "丢弃事件计数（供观测/告警）递增");

    run_migrations(&pool).await.unwrap();
    assert_eq!(wb.flush().await.unwrap(), 4);
    let rows = svc
        .telemetry
        .query_range(
            "dev-ovf",
            Utc::now() - Duration::minutes(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert_eq!(
        values_of(&rows),
        vec![2.0, 3.0, 4.0, 5.0],
        "丢的必须是**最旧**的（1.0），留下的按序落库"
    );
    let _ = std::fs::remove_file(path);
}

/// **U-68③（边界次序）**：容量触发那一刻**不得**因为上限而先裁——那批马上要尝试提交，
/// 裁掉等于"丢掉本可以入库的点"（正常库上就会无谓丢最旧一条）。
///
/// 判据：`max_points=2`、`capacity=3`、**库正常**。第 3 个 push 触发提交 ⇒ 3 条都该入库、
/// 丢弃计数为 0。（若实现改成"先 trim 再 drain"，则第 1 条被无谓丢掉 ⇒ 库里只有 2 条。）
#[tokio::test]
async fn writebuffer_healthy_commit_is_not_preempted_by_limit_trim() {
    let (pool, path) = bare_file_pool("order").await;
    run_migrations(&pool).await.unwrap();
    let svc = StorageService::new(pool.clone());
    let wb = WriteBuffer::new_with_max_points(3, 1000, pool, 2);

    for v in 1..=3 {
        wb.buffer_telemetry(make_telemetry("dev-order", "v", v as f64))
            .await
            .unwrap();
    }

    assert_eq!(wb.dropped_points(), 0, "这批正要提交 ⇒ 不得裁剪丢弃");
    let rows = svc
        .telemetry
        .query_range(
            "dev-order",
            Utc::now() - Duration::minutes(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert_eq!(values_of(&rows), vec![1.0, 2.0, 3.0], "3 条全部入库");
    let _ = std::fs::remove_file(path);
}

/// **U-68③（活锁防护）**：失败后**不是每个 push 都触发一次注定失败的大批提交** ——
/// 容量触发按「**自上次尝试以来新 push 的点数**」计，失败回填的老点不重复计入。
///
/// 判据：capacity=2、落库持续失败。4 次 push 只应产生 **2 次**提交尝试
/// （第 2、4 次 push 各一次），而不是每 push 一次；同时**没有任何点被丢**（还在等重试）。
#[tokio::test]
async fn writebuffer_failed_commit_is_not_retried_on_every_push() {
    let (pool, path) = bare_file_pool("backoff").await;
    let wb = WriteBuffer::new_with_max_points(2, 1000, pool, 1000);

    // 容量触发的那两次 push 会把提交失败**上抛**（既有 API 语义不变：调用方据 Err 记 warn）；
    // 点不会因此丢（回填）⇒ 本用例只看缓冲状态与"尝试了几次"。
    for v in 1..=4 {
        let _ = wb
            .buffer_telemetry(make_telemetry("dev-bo", "v", v as f64))
            .await;
    }

    assert_eq!(
        wb.requeued_batches(),
        2,
        "4 次 push / 容量 2 ⇒ 只应尝试 2 次（每 push 都试 = 失败风暴）"
    );
    assert_eq!(wb.buffered_points(), 4, "失败的点全部留着等重试");
    assert_eq!(wb.dropped_points(), 0);
    let _ = std::fs::remove_file(path);
}

/// **U-64 子情形**（"生产者正卡在写库中途"）：`flush` 已 `drain` 出缓冲、卡在提交 await 上时
/// 任务被 **abort** ⇒ 这一批**必须回填**（否则 future 被丢弃、连 `Err` 分支都走不到、静默消失）。
///
/// 用**真实阻塞**制造窗口（不用 mock）：另一条连接上的事务已写入 `telemetry` 未提交 ⇒ WAL 下
/// 另一个写者的 INSERT 会按 `busy_timeout`（5 s）等待 ⇒ 200 ms 后 flush 任务**必然仍在提交中**。
///
/// 改什么会让本条红：去掉 `BatchGuard`（回到"drain 走掉就不还"）⇒ 被 abort 的 2 条永久消失。
#[tokio::test]
async fn writebuffer_abort_mid_commit_keeps_drained_batch() {
    let (pool, path) = bare_file_pool("abort").await;
    run_migrations(&pool).await.unwrap();
    let wb = Arc::new(WriteBuffer::new_with_max_points(
        100,
        1000,
        pool.clone(),
        1000,
    ));
    wb.buffer_telemetry(make_telemetry("dev-abort", "v", 1.0))
        .await
        .unwrap();
    wb.buffer_telemetry(make_telemetry("dev-abort", "v", 2.0))
        .await
        .unwrap();

    // 阻塞源：持写锁的未提交事务
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO telemetry (device_id, timestamp, metric_name, value, quality)
         VALUES ('blocker', 0, 'x', 0.0, 0)",
    )
    .execute(&mut *blocker)
    .await
    .unwrap();

    let wb2 = wb.clone();
    let flush_task = tokio::spawn(async move {
        let _ = wb2.flush().await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(
        wb.buffered_points(),
        0,
        "此刻缓冲已被 drain（这正是丢点窗口的现场：点已离开缓冲、还没进库）"
    );

    flush_task.abort();
    let _ = flush_task.await; // 等 future 真被丢弃（Drop 回填在此刻发生）

    assert_eq!(
        wb.buffered_points(),
        2,
        "被 abort 的批次必须回填缓冲（bug 形态：这 2 条无日志、无计数地永久消失）"
    );
    assert_eq!(wb.requeued_batches(), 1, "abort 回填也要可观测");

    blocker.rollback().await.unwrap();

    // 释放写锁后，退出路径的那次 flush 应能把回填的数据真正落盘
    assert_eq!(
        wb.flush().await.unwrap(),
        2,
        "回填的数据必须能被后续 flush 提交"
    );
    let svc = StorageService::new(pool.clone());
    let rows = svc
        .telemetry
        .query_range(
            "dev-abort",
            Utc::now() - Duration::minutes(1),
            Utc::now() + Duration::minutes(1),
        )
        .await
        .unwrap();
    assert_eq!(values_of(&rows), vec![1.0, 2.0], "两条都落库");
    let _ = std::fs::remove_file(path);
}

// ── RetentionManager ──

#[tokio::test]
async fn retention_enforce() {
    let (_, svc) = setup().await;
    let rm = RetentionManager::new(30, 30);
    let report = rm.enforce(&svc).await.unwrap();
    assert_eq!(report.telemetry_deleted + report.events_deleted, 0);
}

// ── Health Check ──

#[tokio::test]
async fn health_check_passes() {
    let (_, svc) = setup().await;
    assert!(svc.health_check().await.unwrap());
}
