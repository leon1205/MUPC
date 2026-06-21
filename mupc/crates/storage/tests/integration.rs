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
        value,
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
    assert!((latest.unwrap().value - 220.0).abs() < 0.01);
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
        value: 1.0,
        quality: 0,
    };
    let t2 = TelemetryPoint {
        id: None,
        device_id: "dev1".into(),
        timestamp: now,
        metric_name: "v".into(),
        value: 2.0,
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
        value: 0.0,
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
