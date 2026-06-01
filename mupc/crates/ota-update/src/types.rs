//! OTA 更新数据类型
//!
//! Phase 3C.2 OTA 模型自动更新模块数据类型定义

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// 任务 ID 类型别名
pub type TaskId = String;

/// 模型类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    #[serde(rename = "lstm")]
    Lstm,
    #[serde(rename = "maddpg")]
    Maddpg,
}

impl fmt::Display for ModelType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelType::Lstm => write!(f, "lstm"),
            ModelType::Maddpg => write!(f, "maddpg"),
        }
    }
}

/// 模型版本信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVersion {
    pub model_type: ModelType,
    pub version: String,
    pub updated_at: DateTime<Utc>,
    pub md5: String,
    pub size: u64,
}

/// 更新信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub model_type: ModelType,
    pub current_version: String,
    pub available_version: String,
    pub size: u64,
    pub checksum: String,
    pub signature: String,
    pub url: String,
    pub is_incremental: bool,
    pub base_version: Option<String>,
}

/// OTA 更新状态
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OtaState {
    /// 空闲状态
    #[default]
    Idle,
    /// 检查更新中
    Checking,
    /// 下载中
    Downloading { progress: u8 },
    /// 验证中
    Verifying,
    /// 应用中
    Applying,
    /// 已应用（模型已替换，等待策略引擎加载）
    Applied,
    /// 回滚中
    RollingBack,
    /// 失败
    Failed { error: String },
    /// 已完成
    Completed,
}

impl fmt::Display for OtaState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OtaState::Idle => write!(f, "Idle"),
            OtaState::Checking => write!(f, "Checking"),
            OtaState::Downloading { progress } => write!(f, "Downloading({}%)", progress),
            OtaState::Verifying => write!(f, "Verifying"),
            OtaState::Applying => write!(f, "Applying"),
            OtaState::Applied => write!(f, "Applied"),
            OtaState::RollingBack => write!(f, "RollingBack"),
            OtaState::Failed { error } => write!(f, "Failed: {}", error),
            OtaState::Completed => write!(f, "Completed"),
        }
    }
}

/// OTA 更新任务
#[derive(Debug, Clone)]
pub struct OtaTask {
    pub task_id: String,
    pub model_type: ModelType,
    pub from_version: String,
    pub to_version: String,
    pub state: OtaState,
    pub progress: u8,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 更新记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecord {
    pub task_id: String,
    pub model_type: ModelType,
    pub from_version: String,
    pub to_version: String,
    pub status: OtaState,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

/// 北向指令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaUpdateCommand {
    pub cmd: String,
    pub task_id: String,
    pub model_type: ModelType,
    pub version: String,
    pub url: String,
    pub signature: String,
    pub checksum: String,
}

/// 北向响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaUpdateResponse {
    pub task_id: String,
    pub model_type: ModelType,
    pub status: OtaState,
    pub progress: Option<u8>,
    pub error_message: Option<String>,
}

/// 版本查询响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionQueryResponse {
    pub models: Vec<ModelVersion>,
    pub device_id: String,
    pub timestamp: DateTime<Utc>,
}

/// 回滚触发条件
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RollbackTrigger {
    /// 模型加载失败
    ModelLoadFailed,
    /// 签名验证失败
    VerificationFailed,
    /// 预热超时
    WarmupTimeout,
    /// 推理失败
    InferenceFailed,
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ========== ModelType 测试 ==========

    #[test]
    fn test_model_type_display() {
        assert_eq!(format!("{}", ModelType::Lstm), "lstm");
        assert_eq!(format!("{}", ModelType::Maddpg), "maddpg");
    }

    #[test]
    fn test_model_type_serde() {
        let model_type = ModelType::Lstm;

        let json = serde_json::to_string(&model_type).unwrap();
        assert_eq!(json, "\"lstm\"");

        let parsed: ModelType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ModelType::Lstm);
    }

    #[test]
    fn test_model_type_copy() {
        let mt = ModelType::Lstm;
        let _copy = mt;
        assert_eq!(mt, ModelType::Lstm);
    }

    // ========== ModelVersion 测试 ==========

    #[test]
    fn test_model_version_serde() {
        let version = ModelVersion {
            model_type: ModelType::Lstm,
            version: "1.2.0".to_string(),
            updated_at: Utc.with_ymd_and_hms(2026, 5, 28, 10, 0, 0).unwrap(),
            md5: "abc123".to_string(),
            size: 1024,
        };

        let json = serde_json::to_string(&version).unwrap();
        let parsed: ModelVersion = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.model_type, ModelType::Lstm);
        assert_eq!(parsed.version, "1.2.0");
        assert_eq!(parsed.md5, "abc123");
        assert_eq!(parsed.size, 1024);
    }

    // ========== UpdateInfo 测试 ==========

    #[test]
    fn test_update_info_serde() {
        let info = UpdateInfo {
            model_type: ModelType::Maddpg,
            current_version: "1.0.0".to_string(),
            available_version: "1.1.0".to_string(),
            size: 5000,
            checksum: "def456".to_string(),
            signature: "sig_data".to_string(),
            url: "https://ota.example.com/model.rknn".to_string(),
            is_incremental: true,
            base_version: Some("1.0.0".to_string()),
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: UpdateInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.model_type, ModelType::Maddpg);
        assert_eq!(parsed.current_version, "1.0.0");
        assert_eq!(parsed.available_version, "1.1.0");
        assert!(parsed.is_incremental);
        assert_eq!(parsed.base_version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_update_info_no_base_version() {
        let info = UpdateInfo {
            model_type: ModelType::Lstm,
            current_version: "1.0.0".to_string(),
            available_version: "2.0.0".to_string(),
            size: 10000,
            checksum: "xyz789".to_string(),
            signature: "sig".to_string(),
            url: "https://ota.example.com/v2.rknn".to_string(),
            is_incremental: false,
            base_version: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: UpdateInfo = serde_json::from_str(&json).unwrap();

        assert!(parsed.base_version.is_none());
        assert!(!parsed.is_incremental);
    }

    // ========== OtaState 测试 ==========

    #[test]
    fn test_ota_state_default() {
        let state = OtaState::default();
        assert_eq!(state, OtaState::Idle);
    }

    #[test]
    fn test_ota_state_display() {
        assert_eq!(format!("{}", OtaState::Idle), "Idle");
        assert_eq!(format!("{}", OtaState::Checking), "Checking");
        assert_eq!(format!("{}", OtaState::Downloading { progress: 50 }), "Downloading(50%)");
        assert_eq!(format!("{}", OtaState::Verifying), "Verifying");
        assert_eq!(format!("{}", OtaState::Applying), "Applying");
        assert_eq!(format!("{}", OtaState::Applied), "Applied");
        assert_eq!(format!("{}", OtaState::RollingBack), "RollingBack");
        assert_eq!(format!("{}", OtaState::Failed { error: "test error".to_string() }), "Failed: test error");
        assert_eq!(format!("{}", OtaState::Completed), "Completed");
    }

    #[test]
    fn test_ota_state_serde() {
        let state = OtaState::Downloading { progress: 75 };

        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "{\"downloading\":{\"progress\":75}}");

        let parsed: OtaState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, OtaState::Downloading { progress: 75 });
    }

    #[test]
    fn test_ota_state_failed_serde() {
        let state = OtaState::Failed { error: "checksum mismatch".to_string() };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: OtaState = serde_json::from_str(&json).unwrap();

        match parsed {
            OtaState::Failed { error } => assert_eq!(error, "checksum mismatch"),
            _ => panic!("Expected Failed state"),
        }
    }

    #[test]
    fn test_ota_state_clone() {
        let state = OtaState::Applying;
        let copy = state.clone();
        assert_eq!(state, OtaState::Applying);
        assert_eq!(copy, OtaState::Applying);
    }

    // ========== OtaTask 测试 ==========

    #[test]
    fn test_ota_task_debug() {
        let task = OtaTask {
            task_id: "task-123".to_string(),
            model_type: ModelType::Lstm,
            from_version: "1.0.0".to_string(),
            to_version: "1.1.0".to_string(),
            state: OtaState::Downloading { progress: 30 },
            progress: 30,
            created_at: Utc.with_ymd_and_hms(2026, 5, 28, 10, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 5, 28, 10, 5, 0).unwrap(),
        };

        let debug_str = format!("{:?}", task);
        assert!(debug_str.contains("task-123"));
        assert!(debug_str.contains("Lstm"));
    }

    // ========== UpdateRecord 测试 ==========

    #[test]
    fn test_update_record_serde() {
        let record = UpdateRecord {
            task_id: "task-456".to_string(),
            model_type: ModelType::Maddpg,
            from_version: "1.0.0".to_string(),
            to_version: "1.1.0".to_string(),
            status: OtaState::Completed,
            started_at: Utc.with_ymd_and_hms(2026, 5, 28, 10, 0, 0).unwrap(),
            completed_at: Some(Utc.with_ymd_and_hms(2026, 5, 28, 10, 30, 0).unwrap()),
            error_message: None,
        };

        let json = serde_json::to_string(&record).unwrap();
        let parsed: UpdateRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.task_id, "task-456");
        assert_eq!(parsed.status, OtaState::Completed);
        assert!(parsed.completed_at.is_some());
        assert!(parsed.error_message.is_none());
    }

    #[test]
    fn test_update_record_with_error() {
        let record = UpdateRecord {
            task_id: "task-789".to_string(),
            model_type: ModelType::Lstm,
            from_version: "1.0.0".to_string(),
            to_version: "1.1.0".to_string(),
            status: OtaState::Failed { error: "network timeout".to_string() },
            started_at: Utc.with_ymd_and_hms(2026, 5, 28, 10, 0, 0).unwrap(),
            completed_at: None,
            error_message: Some("network timeout".to_string()),
        };

        let json = serde_json::to_string(&record).unwrap();
        let parsed: UpdateRecord = serde_json::from_str(&json).unwrap();

        assert!(parsed.completed_at.is_none());
        assert!(parsed.error_message.is_some());
    }

    // ========== OtaUpdateCommand 测试 ==========

    #[test]
    fn test_ota_update_command_serde() {
        let cmd = OtaUpdateCommand {
            cmd: "ota_update".to_string(),
            task_id: "task-001".to_string(),
            model_type: ModelType::Lstm,
            version: "2.0.0".to_string(),
            url: "https://ota.example.com/v2.lstm.rknn".to_string(),
            signature: "signature_data".to_string(),
            checksum: "checksum_data".to_string(),
        };

        let json = serde_json::to_string(&cmd).unwrap();
        let parsed: OtaUpdateCommand = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.cmd, "ota_update");
        assert_eq!(parsed.version, "2.0.0");
    }

    // ========== OtaUpdateResponse 测试 ==========

    #[test]
    fn test_ota_update_response_serde() {
        let response = OtaUpdateResponse {
            task_id: "task-002".to_string(),
            model_type: ModelType::Maddpg,
            status: OtaState::Downloading { progress: 60 },
            progress: Some(60),
            error_message: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: OtaUpdateResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.progress, Some(60));
    }

    // ========== VersionQueryResponse 测试 ==========

    #[test]
    fn test_version_query_response_serde() {
        let response = VersionQueryResponse {
            models: vec![
                ModelVersion {
                    model_type: ModelType::Lstm,
                    version: "1.2.0".to_string(),
                    updated_at: Utc.with_ymd_and_hms(2026, 5, 28, 10, 0, 0).unwrap(),
                    md5: "md5_lstm".to_string(),
                    size: 1024,
                },
                ModelVersion {
                    model_type: ModelType::Maddpg,
                    version: "1.0.5".to_string(),
                    updated_at: Utc.with_ymd_and_hms(2026, 5, 27, 8, 30, 0).unwrap(),
                    md5: "md5_maddpg".to_string(),
                    size: 2048,
                },
            ],
            device_id: "device-001".to_string(),
            timestamp: Utc.with_ymd_and_hms(2026, 5, 28, 12, 0, 0).unwrap(),
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: VersionQueryResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.models.len(), 2);
        assert_eq!(parsed.device_id, "device-001");
    }

    // ========== RollbackTrigger 测试 ==========

    #[test]
    fn test_rollback_trigger_debug() {
        assert_eq!(format!("{:?}", RollbackTrigger::ModelLoadFailed), "ModelLoadFailed");
        assert_eq!(format!("{:?}", RollbackTrigger::VerificationFailed), "VerificationFailed");
        assert_eq!(format!("{:?}", RollbackTrigger::WarmupTimeout), "WarmupTimeout");
        assert_eq!(format!("{:?}", RollbackTrigger::InferenceFailed), "InferenceFailed");
    }

    #[test]
    fn test_rollback_trigger_copy() {
        let trigger = RollbackTrigger::ModelLoadFailed;
        let _copy = trigger;
        assert_eq!(trigger, RollbackTrigger::ModelLoadFailed);
    }

    // ========== TaskId 测试 ==========

    #[test]
    fn test_task_id_type() {
        let task_id: TaskId = "task-12345".to_string();
        assert_eq!(task_id, "task-12345");

        let json = serde_json::to_string(&task_id).unwrap();
        assert_eq!(json, "\"task-12345\"");

        let parsed: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, "task-12345");
    }

    // ========== OtaState 相等性测试 ==========

    #[test]
    fn test_ota_state_partial_eq() {
        assert_eq!(OtaState::Idle, OtaState::Idle);
        assert_eq!(OtaState::Applied, OtaState::Applied);
        assert_ne!(OtaState::Idle, OtaState::Checking);
        assert_ne!(OtaState::Downloading { progress: 10 }, OtaState::Downloading { progress: 20 });
    }

    // ========== OtaState Applied 状态测试 (PRD 要求) ==========

    #[test]
    fn test_ota_state_has_applied() {
        // 验证 OtaState 枚举包含 Applied 状态（PRD 要求）
        let state = OtaState::Applied;
        assert_eq!(format!("{}", state), "Applied");

        // 验证 Applied 状态可以序列化/反序列化
        let json = serde_json::to_string(&state).unwrap();
        let parsed: OtaState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, OtaState::Applied);
    }
}