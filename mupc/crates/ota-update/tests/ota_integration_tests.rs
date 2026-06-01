//! OTA 模块集成测试
//!
//! Phase 3C.2 OTA 模型自动更新模块的集成测试
//! 测试 OTA 公共 API：版本查询、配置验证、状态序列化、调度器命令

use std::sync::Arc;

use tempfile::TempDir;

use mupc_ota_update::{
    OtaConfig, OtaError, OtaManager, OtaManagerImpl, OtaScheduler, OtaState,
    SchedulerCommand, ModelType, ModelVersion, UpdateInfo, VersionQueryResponse,
};

// ============================================================================
// 辅助函数
// ============================================================================

fn create_test_config(temp_dir: &TempDir) -> OtaConfig {
    let models_dir = temp_dir.path().join("models");
    std::fs::create_dir_all(&models_dir).ok();

    let public_key_path = temp_dir.path().join("public_key.pem");
    std::fs::write(&public_key_path, b"test public key").ok();

    let version_file = models_dir.join("version.json");
    let version_data = serde_json::json!({
        "models": [
            {
                "model_type": "lstm",
                "version": "1.0.0",
                "updated_at": "2026-05-28T10:00:00Z",
                "md5": "abc123",
                "size": 1024
            },
            {
                "model_type": "maddpg",
                "version": "1.0.0",
                "updated_at": "2026-05-28T10:00:00Z",
                "md5": "def456",
                "size": 2048
            }
        ]
    });
    std::fs::write(&version_file, version_data.to_string()).ok();

    OtaConfig {
        server_url: "https://ota.example.com".to_string(),
        check_interval: 3600,
        download_window_start: "02:00".to_string(),
        download_window_end: "05:00".to_string(),
        auto_download: true,
        auto_apply: true,
        download_timeout: 300,
        retry_count: 3,
        max_rollback_count: 3,
        public_key_path: public_key_path.display().to_string(),
        model_storage_path: models_dir.display().to_string(),
    }
}

async fn create_test_manager(temp_dir: &TempDir) -> OtaManagerImpl {
    let config = create_test_config(temp_dir);
    let temp_path = temp_dir.path().join("temp");
    OtaManagerImpl::new(config, temp_path).unwrap()
}

// ============================================================================
// 版本查询测试
// ============================================================================

#[tokio::test]
async fn test_query_versions_returns_correct_format() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    let result = manager.query_versions().await;
    assert!(result.is_ok());

    let response = result.unwrap();
    assert!(response.models.len() >= 2);
    assert!(!response.device_id.is_empty());
}

#[tokio::test]
async fn test_get_current_version() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    let result = manager.get_current_version(ModelType::Lstm);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().version, "1.0.0");
}

// ============================================================================
// 配置验证测试
// ============================================================================

#[test]
fn test_ota_config_default() {
    let config = OtaConfig::default();
    assert_eq!(config.check_interval, 3600);
    assert_eq!(config.download_window_start, "02:00");
    assert_eq!(config.download_window_end, "05:00");
    assert!(config.auto_download);
    assert!(config.auto_apply);
}

#[test]
fn test_ota_config_validate() {
    let config = OtaConfig::default();
    assert!(config.validate().is_ok());

    let mut invalid = OtaConfig::default();
    invalid.check_interval = 0;
    assert!(invalid.validate().is_err());

    let mut invalid2 = OtaConfig::default();
    invalid2.retry_count = 0;
    assert!(invalid2.validate().is_err());
}

#[test]
fn test_ota_config_download_window() {
    let mut config = OtaConfig::default();
    config.download_window_start = "02:00".to_string();
    config.download_window_end = "05:00".to_string();
    assert!(config.is_in_download_window(2, 0));
    assert!(config.is_in_download_window(3, 30));
    assert!(!config.is_in_download_window(6, 0));
}

// ============================================================================
// 状态与序列化测试
// ============================================================================

#[test]
fn test_ota_state_serde() {
    let states = vec![
        OtaState::Idle,
        OtaState::Checking,
        OtaState::Downloading { progress: 50 },
        OtaState::Verifying,
        OtaState::Applying,
        OtaState::Applied,
        OtaState::RollingBack,
        OtaState::Completed,
        OtaState::Failed { error: "test error".to_string() },
    ];

    for state in states {
        let json = serde_json::to_string(&state).unwrap();
        let parsed: OtaState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }
}

#[test]
fn test_version_query_response_serde() {
    let response = VersionQueryResponse {
        models: vec![ModelVersion {
            model_type: ModelType::Lstm,
            version: "1.2.0".to_string(),
            updated_at: chrono::Utc::now(),
            md5: "abc123".to_string(),
            size: 1024,
        }],
        device_id: "device_001".to_string(),
        timestamp: chrono::Utc::now(),
    };

    let json = serde_json::to_string(&response).unwrap();
    let parsed: VersionQueryResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.device_id, "device_001");
    assert_eq!(parsed.models.len(), 1);
}

// ============================================================================
// 调度器命令测试
// ============================================================================

#[test]
fn test_scheduler_command_clone() {
    assert_eq!(SchedulerCommand::Stop, SchedulerCommand::Stop);
    assert_eq!(SchedulerCommand::TriggerCheck, SchedulerCommand::TriggerCheck);
}

#[test]
fn test_scheduler_command_debug() {
    assert!(format!("{:?}", SchedulerCommand::Stop).contains("Stop"));
    assert!(format!("{:?}", SchedulerCommand::TriggerCheck).contains("TriggerCheck"));
}

// ============================================================================
// get_update_status 测试
// ============================================================================

#[tokio::test]
async fn test_get_update_status_idle() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    let status = manager.get_update_status();
    assert_eq!(status.state, OtaState::Idle);
    assert!(status.current_task_id.is_none());
}

// ============================================================================
// get_update_history 测试
// ============================================================================

#[tokio::test]
async fn test_get_update_history_empty() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    let history = manager.get_update_history(10).unwrap();
    assert!(history.is_empty());
}
