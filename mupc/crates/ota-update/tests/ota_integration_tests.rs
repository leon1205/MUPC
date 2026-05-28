//! OTA 模块集成测试
//!
//! Phase 3C.2 OTA 模型自动更新模块的集成测试
//! 测试完整的 OTA 流程：版本查询、状态转换、配置验证、调度器

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use mupc_ota_update::{
    OtaConfig, OtaError, OtaManager, OtaManagerImpl, OtaScheduler, OtaState,
    SchedulerCommand, ModelType, ModelVersion, UpdateInfo, VersionQueryResponse,
};

// ============================================================================
// 辅助函数
// ============================================================================

/// 创建测试用 OtaConfig
fn create_test_config(temp_dir: &TempDir) -> OtaConfig {
    let models_dir = temp_dir.path().join("models");
    std::fs::create_dir_all(&models_dir).ok();

    // 创建公钥文件
    let public_key_path = temp_dir.path().join("public_key.pem");
    std::fs::write(&public_key_path, b"test public key").ok();

    // 创建版本文件
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

/// 创建测试用 OtaManagerImpl
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

    // 验证响应结构
    assert!(!response.device_id.is_empty());
    assert!(response.timestamp <= chrono::Utc::now());

    // 验证模型版本信息
    for model in &response.models {
        assert!(!model.version.is_empty());
        assert!(!model.md5.is_empty());
        assert!(model.size > 0);
        match model.model_type {
            ModelType::Lstm | ModelType::Maddpg => {}
        }
    }

    tracing::info!("版本查询响应: {:?}", response);
}

#[tokio::test]
async fn test_query_versions_contains_all_model_types() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    let response = manager.query_versions().await.unwrap();

    let model_types: Vec<ModelType> = response.models.iter().map(|m| m.model_type).collect();
    assert!(model_types.contains(&ModelType::Lstm));
    assert!(model_types.contains(&ModelType::Maddpg));
}

// ============================================================================
// 状态转换测试
// ============================================================================

#[tokio::test]
async fn test_state_transitions_valid() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    // 初始状态: Idle
    let state = manager.get_state().await;
    assert_eq!(state, OtaState::Idle);

    // Idle -> Checking (check_updates)
    manager.transition_state(OtaState::Checking).await.unwrap();
    let state = manager.get_state().await;
    assert_eq!(state, OtaState::Checking);

    // Checking -> Idle (check_updates 完成)
    manager.transition_state(OtaState::Idle).await.unwrap();
    let state = manager.get_state().await;
    assert_eq!(state, OtaState::Idle);
}

#[tokio::test]
async fn test_state_transitions_downloading() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    // 转换到 Downloading
    manager.transition_state(OtaState::Downloading { progress: 0 }).await.unwrap();
    assert_eq!(manager.get_state().await, OtaState::Downloading { progress: 0 });

    // 更新进度
    manager.transition_state(OtaState::Downloading { progress: 50 }).await.unwrap();
    assert_eq!(manager.get_state().await, OtaState::Downloading { progress: 50 });

    // Downloading -> Verifying
    manager.transition_state(OtaState::Verifying).await.unwrap();
    assert_eq!(manager.get_state().await, OtaState::Verifying);
}

#[tokio::test]
async fn test_state_transitions_failure_recovery() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    // 设置为失败状态
    manager.transition_state(OtaState::Failed { error: "test error".to_string() }).await.unwrap();
    assert!(matches!(manager.get_state().await, OtaState::Failed { .. }));

    // Failed -> Idle (重试)
    manager.transition_state(OtaState::Idle).await.unwrap();
    assert_eq!(manager.get_state().await, OtaState::Idle);
}

#[tokio::test]
async fn test_state_transitions_full_flow() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    // 模拟完整 OTA 流程
    let transitions = vec![
        OtaState::Checking,
        OtaState::Downloading { progress: 0 },
        OtaState::Downloading { progress: 50 },
        OtaState::Downloading { progress: 100 },
        OtaState::Verifying,
        OtaState::Applying,
        OtaState::Applied,
        OtaState::Completed,
        OtaState::Idle,
    ];

    let mut prev_state = OtaState::Idle;
    for next_state in transitions {
        if OtaManagerImpl::can_transition(&prev_state, &next_state) {
            manager.transition_state(next_state).await.unwrap();
            prev_state = manager.get_state().await;
        }
    }

    assert_eq!(prev_state, OtaState::Idle);
}

#[tokio::test]
async fn test_invalid_state_transition_rejected() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    // 设置为 Idle
    assert_eq!(manager.get_state().await, OtaState::Idle);

    // Idle -> Applying 应该被 can_transition 拒绝
    assert!(!OtaManagerImpl::can_transition(&OtaState::Idle, &OtaState::Applying));

    // Idle -> Completed 也应该被拒绝
    assert!(!OtaManagerImpl::can_transition(&OtaState::Idle, &OtaState::Completed));
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
    assert_eq!(config.download_timeout, 300);
    assert_eq!(config.retry_count, 3);
    assert_eq!(config.max_rollback_count, 3);
}

#[test]
fn test_ota_config_download_window_validation() {
    // 正常窗口
    let mut config = OtaConfig::default();
    config.download_window_start = "02:00".to_string();
    config.download_window_end = "05:00".to_string();
    assert!(config.is_in_download_window(2, 0));
    assert!(config.is_in_download_window(3, 30));
    assert!(!config.is_in_download_window(6, 0));

    // 跨午夜窗口
    let mut config = OtaConfig::default();
    config.download_window_start = "22:00".to_string();
    config.download_window_end = "06:00".to_string();
    assert!(config.is_in_download_window(23, 0));
    assert!(config.is_in_download_window(0, 0));
    assert!(config.is_in_download_window(5, 59));
    assert!(!config.is_in_download_window(7, 0));
}

#[test]
fn test_ota_config_validate_method() {
    let config = OtaConfig::default();
    assert!(config.validate().is_ok());

    // 无效配置: check_interval 为 0
    let mut invalid_config = OtaConfig::default();
    invalid_config.check_interval = 0;
    assert!(invalid_config.validate().is_err());

    // 无效配置: retry_count 为 0
    let mut invalid_config = OtaConfig::default();
    invalid_config.retry_count = 0;
    assert!(invalid_config.validate().is_err());
}

#[test]
fn test_ota_config_debug_format() {
    let config = OtaConfig::default();
    let debug_str = format!("{:?}", config);

    assert!(debug_str.contains("OtaConfig"));
    assert!(debug_str.contains("check_interval"));
    assert!(debug_str.contains("server_url"));
}

// ============================================================================
// 调度器测试
// ============================================================================

#[tokio::test]
async fn test_scheduler_command_processing() {
    let temp_dir = TempDir::new().unwrap();
    let config = create_test_config(&temp_dir);

    // 创建 Mock OTA Manager
    let ota_manager: Arc<dyn OtaManager> = Arc::new(MockOtaManager::new());

    let (scheduler, mut command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

    // 发送 TriggerCheck 命令
    scheduler.send_command(SchedulerCommand::TriggerCheck).unwrap();

    // 验证命令被接收
    let cmd = command_rx.recv().await;
    assert!(cmd.is_some());
    assert!(matches!(cmd.unwrap(), SchedulerCommand::TriggerCheck));

    // 发送 Stop 命令
    scheduler.send_command(SchedulerCommand::Stop).unwrap();

    let cmd = command_rx.recv().await;
    assert!(cmd.is_some());
    assert!(matches!(cmd.unwrap(), SchedulerCommand::Stop));
}

#[tokio::test]
async fn test_scheduler_command_buffer_full() {
    let temp_dir = TempDir::new().unwrap();
    let config = create_test_config(&temp_dir);

    let ota_manager: Arc<dyn OtaManager> = Arc::new(MockOtaManager::new());
    let (scheduler, _command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

    // 填满缓冲区 (16 条)
    for _ in 0..16 {
        let result = scheduler.send_command(SchedulerCommand::TriggerCheck);
        assert!(result.is_ok());
    }

    // 再次发送应该失败
    let result = scheduler.send_command(SchedulerCommand::Stop);
    assert!(result.is_err());
}

#[test]
fn test_scheduler_command_clone() {
    let cmd1 = SchedulerCommand::Stop;
    let cmd2 = cmd1.clone();
    assert_eq!(cmd1, cmd2);

    let cmd3 = SchedulerCommand::TriggerCheck;
    let cmd4 = cmd3.clone();
    assert_eq!(cmd3, cmd4);
}

#[test]
fn test_scheduler_command_debug() {
    let stop = SchedulerCommand::Stop;
    let trigger = SchedulerCommand::TriggerCheck;

    assert!(format!("{:?}", stop).contains("Stop"));
    assert!(format!("{:?}", trigger).contains("TriggerCheck"));
}

// ============================================================================
// Mock OtaManager 实现
// ============================================================================

use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct MockOtaManager {
    check_count: AtomicUsize,
}

impl MockOtaManager {
    fn new() -> Self {
        Self {
            check_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl OtaManager for MockOtaManager {
    fn get_current_version(&self, model_type: ModelType) -> Result<ModelVersion, OtaError> {
        Ok(ModelVersion {
            model_type,
            version: "1.0.0".to_string(),
            updated_at: chrono::Utc::now(),
            md5: "mock_md5".to_string(),
            size: 1024,
        })
    }

    async fn check_updates(&self) -> Result<Vec<UpdateInfo>, OtaError> {
        self.check_count.fetch_add(1, Ordering::SeqCst);
        Ok(vec![])
    }

    async fn start_download(&self, _update_info: &UpdateInfo) -> Result<String, OtaError> {
        Ok("mock_task_id".to_string())
    }

    fn get_download_progress(&self, _task_id: String) -> Result<u8, OtaError> {
        Ok(100)
    }

    async fn cancel_download(&self, _task_id: String) -> Result<(), OtaError> {
        Ok(())
    }

    async fn apply_update(&self, _task_id: String) -> Result<(), OtaError> {
        Ok(())
    }

    async fn rollback(&self, _model_type: ModelType) -> Result<(), OtaError> {
        Ok(())
    }

    fn get_update_status(&self) -> mupc_ota_update::UpdateStatus {
        mupc_ota_update::UpdateStatus {
            state: OtaState::Idle,
            current_task_id: None,
            current_model_type: None,
            download_progress: None,
            error_message: None,
        }
    }

    fn get_update_history(&self, _limit: usize) -> Result<Vec<mupc_ota_update::UpdateRecord>, OtaError> {
        Ok(vec![])
    }

    async fn query_versions(&self) -> Result<VersionQueryResponse, OtaError> {
        Ok(VersionQueryResponse {
            models: vec![
                ModelVersion {
                    model_type: ModelType::Lstm,
                    version: "1.2.0".to_string(),
                    updated_at: chrono::Utc::now(),
                    md5: "abc123".to_string(),
                    size: 1024,
                },
                ModelVersion {
                    model_type: ModelType::Maddpg,
                    version: "1.0.5".to_string(),
                    updated_at: chrono::Utc::now(),
                    md5: "def456".to_string(),
                    size: 2048,
                },
            ],
            device_id: "mock_device_id".to_string(),
            timestamp: chrono::Utc::now(),
        })
    }
}

// ============================================================================
// 版本查询响应序列化测试
// ============================================================================

#[test]
fn test_version_query_response_serde() {
    let response = VersionQueryResponse {
        models: vec![
            ModelVersion {
                model_type: ModelType::Lstm,
                version: "1.2.0".to_string(),
                updated_at: chrono::Utc::now(),
                md5: "abc123".to_string(),
                size: 1024,
            },
        ],
        device_id: "device_001".to_string(),
        timestamp: chrono::Utc::now(),
    };

    // 序列化
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("lstm"));
    assert!(json.contains("1.2.0"));
    assert!(json.contains("device_001"));

    // 反序列化
    let parsed: VersionQueryResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.device_id, "device_001");
    assert_eq!(parsed.models.len(), 1);
    assert_eq!(parsed.models[0].model_type, ModelType::Lstm);
}

// ============================================================================
// OtaState 序列化测试
// ============================================================================

#[test]
fn test_ota_state_serde_integration() {
    // 测试各种状态的序列化
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

// ============================================================================
// 端到端流程测试
// ============================================================================

#[tokio::test]
async fn test_end_to_end_version_query_flow() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    // 1. 查询版本
    let response = manager.query_versions().await.unwrap();
    assert!(response.models.len() >= 2);

    // 2. 获取当前版本
    let lstm_version = manager.get_current_version(ModelType::Lstm).unwrap();
    assert_eq!(lstm_version.version, "1.0.0");

    // 3. 检查更新
    let updates = manager.check_updates().await.unwrap();
    tracing::info!("发现 {} 个可用更新", updates.len());

    // 4. 获取更新状态
    let status = manager.get_update_status();
    assert_eq!(status.state, OtaState::Idle);
}

#[tokio::test]
async fn test_end_to_end_multiple_model_types() {
    let temp_dir = TempDir::new().unwrap();
    let manager = create_test_manager(&temp_dir).await;

    // LSTM 版本
    let lstm = manager.get_current_version(ModelType::Lstm).unwrap();
    assert_eq!(lstm.model_type, ModelType::Lstm);

    // MADDPG 版本
    let maddpg = manager.get_current_version(ModelType::Maddpg).unwrap();
    assert_eq!(maddpg.model_type, ModelType::Maddpg);
}