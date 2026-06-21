//! 模型文件校验器
//!
//! 校验 .rknn 模型文件的完整性、元数据一致性和类型匹配。
//! 在模型加载阶段（`ModelManager::load_models()`）执行。
//!
//! ## 校验项
//!
//! | 校验项 | 时机 | 失败处理 |
//! |--------|------|----------|
//! | 文件存在性 | 模型加载 | 必须模型拒绝启动，可选模型跳过 |
//! | SHA256 完整性 | 模型加载 | 触发 OTA 备份恢复 |
//! | 文件大小 > 0 | 模型加载 | 拒绝加载 |
//! | 模型类型匹配 | 模型加载 | 拒绝部署或自动回退 |

use crate::error::AiEngineError;
use std::path::Path;

// ============================================================================
// PredictionModelType -- 模型类型枚举
// ============================================================================

/// 预测增强模型类型
///
/// 与 ONNX `metadata_props` 中的 `mupc_model_type` 对应。
/// 用于启动时交叉校验配置文件与模型文件的一致性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionModelType {
    /// 单向 LSTM + Attention 主预测模型（`lstm_attn.rknn`）
    LstmAttention,
    /// BiLSTM + Attention 主预测模型（`bilstm_attn.rknn`）
    BiLstmAttention,
    /// 误差修正 BiLSTM 模型（`error_correction.rknn`）
    ErrorCorrection,
}

impl PredictionModelType {
    /// 返回用户友好的名称
    pub fn name(&self) -> &'static str {
        match self {
            PredictionModelType::LstmAttention => "LSTM+Attention",
            PredictionModelType::BiLstmAttention => "BiLSTM+Attention",
            PredictionModelType::ErrorCorrection => "误差修正BiLSTM",
        }
    }

    /// 从 ONNX metadata `mupc_model_type` 字符串解析
    pub fn from_metadata(s: &str) -> Option<Self> {
        match s {
            "lstm" => Some(PredictionModelType::LstmAttention),
            "bilstm" => Some(PredictionModelType::BiLstmAttention),
            "error_correction" => Some(PredictionModelType::ErrorCorrection),
            _ => None,
        }
    }
}

// ============================================================================
// 模型校验函数
// ============================================================================

/// 校验 .rknn 模型文件
///
/// # 校验项
///
/// 1. 文件存在性（`std::fs::metadata`）
/// 2. 文件大小 > 0
/// 3. SHA256 校验（若 `expected_sha256` 为 `Some`）
///
/// # 参数
///
/// - `model_path`: .rknn 文件路径
/// - `expected_type`: 期望的模型类型（仅用于日志和错误消息）
/// - `expected_sha256`: SHA256 期望值（None 时跳过校验）
///
/// # 返回
///
/// - `Ok(())` — 校验通过
/// - `Err(ModelValidationFailed)` — 任一校验项失败
pub fn validate_rknn_model(
    model_path: &Path,
    expected_type: PredictionModelType,
    expected_sha256: Option<&str>,
) -> Result<(), AiEngineError> {
    let path_str = model_path.display().to_string();

    // 1. 文件存在性检查
    let metadata = std::fs::metadata(model_path).map_err(|e| {
        AiEngineError::ModelValidationFailed {
            model_path: path_str.clone(),
            reason: format!("文件不存在或无法访问: {}", e),
        }
    })?;

    // 2. 文件大小检查
    if metadata.len() == 0 {
        return Err(AiEngineError::ModelValidationFailed {
            model_path: path_str,
            reason: "文件大小为 0".to_string(),
        });
    }

    tracing::debug!(
        "模型文件存在: path={}, size={}B, type={}",
        path_str,
        metadata.len(),
        expected_type.name()
    );

    // 3. SHA256 校验
    if let Some(expected) = expected_sha256 {
        let actual = compute_sha256(model_path)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(AiEngineError::ChecksumMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
        tracing::debug!(
            "SHA256 校验通过: path={}, sha256={}",
            path_str,
            &actual[..16]
        );
    }

    Ok(())
}

/// 校验模型文件类型与配置是否一致
///
/// 用于启动时交叉校验：若配置期望 BiLSTM 但模型类型为单向 LSTM（或反之），
/// 则返回错误或自动回退。
///
/// # 返回
///
/// - `Ok(())` — 类型匹配
/// - `Err(ModelValidationFailed)` — 类型不匹配
pub fn validate_model_type_consistency(
    model_path: &Path,
    expected_type: PredictionModelType,
    actual_type_metadata: &str,
) -> Result<(), AiEngineError> {
    let actual = PredictionModelType::from_metadata(actual_type_metadata).ok_or_else(|| {
        AiEngineError::ModelValidationFailed {
            model_path: model_path.display().to_string(),
            reason: format!(
                "无法识别的模型类型元数据: '{}'（期望 {}）",
                actual_type_metadata,
                expected_type.name()
            ),
        }
    })?;

    if actual != expected_type {
        return Err(AiEngineError::ModelValidationFailed {
            model_path: model_path.display().to_string(),
            reason: format!(
                "模型类型不匹配: 期望 {}，实际 {}",
                expected_type.name(),
                actual.name()
            ),
        });
    }

    tracing::debug!(
        "模型类型校验通过: path={}, type={}",
        model_path.display(),
        expected_type.name()
    );
    Ok(())
}

/// 计算文件的 SHA256 哈希
///
/// 使用 `sha2::Sha256` 进行流式哈希计算。
fn compute_sha256(path: &Path) -> Result<String, AiEngineError> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|e| {
        AiEngineError::ModelValidationFailed {
            model_path: path.display().to_string(),
            reason: format!("无法打开文件进行 SHA256 校验: {}", e),
        }
    })?;

    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file.read(&mut buffer).map_err(|e| {
            AiEngineError::ModelValidationFailed {
                model_path: path.display().to_string(),
                reason: format!("读取文件失败: {}", e),
            }
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // MV-01: PredictionModelType 从 metadata 字符串解析
    // ========================================================================

    #[test]
    fn test_model_type_from_metadata() {
        assert_eq!(
            PredictionModelType::from_metadata("lstm"),
            Some(PredictionModelType::LstmAttention)
        );
        assert_eq!(
            PredictionModelType::from_metadata("bilstm"),
            Some(PredictionModelType::BiLstmAttention)
        );
        assert_eq!(
            PredictionModelType::from_metadata("error_correction"),
            Some(PredictionModelType::ErrorCorrection)
        );
        assert_eq!(PredictionModelType::from_metadata("unknown"), None);
        assert_eq!(PredictionModelType::from_metadata(""), None);
    }

    // ========================================================================
    // MV-02: 文件不存在时返回错误
    // ========================================================================

    #[test]
    fn test_validate_nonexistent_file() {
        let path = std::path::PathBuf::from("/tmp/mupc_nonexistent_model_test.rknn");
        // 确保文件不存在
        let _ = std::fs::remove_file(&path);

        let result = validate_rknn_model(&path, PredictionModelType::LstmAttention, None);
        assert!(result.is_err(), "不存在的文件应返回错误");
        match result.unwrap_err() {
            AiEngineError::ModelValidationFailed { model_path, reason } => {
                assert!(model_path.contains("nonexistent"));
                assert!(reason.contains("不存在") || reason.contains("无法访问"));
            }
            _ => panic!("应返回 ModelValidationFailed"),
        }
    }

    // ========================================================================
    // MV-03: 空文件时返回错误
    // ========================================================================

    #[test]
    fn test_validate_empty_file() {
        let path = std::path::PathBuf::from("/tmp/mupc_empty_model_test.rknn");
        std::fs::write(&path, b"").unwrap();

        let result = validate_rknn_model(&path, PredictionModelType::ErrorCorrection, None);
        assert!(result.is_err(), "空文件应返回错误");
        match result.unwrap_err() {
            AiEngineError::ModelValidationFailed { reason, .. } => {
                assert!(reason.contains("大小为 0"));
            }
            _ => panic!("应返回 ModelValidationFailed"),
        }

        // 清理
        let _ = std::fs::remove_file(&path);
    }

    // ========================================================================
    // MV-04: 有效文件校验通过
    // ========================================================================

    #[test]
    fn test_validate_valid_file() {
        let path = std::path::PathBuf::from("/tmp/mupc_valid_model_test.rknn");
        std::fs::write(&path, b"dummy rknn model content").unwrap();

        let result = validate_rknn_model(&path, PredictionModelType::LstmAttention, None);
        assert!(result.is_ok(), "有效文件应通过校验");

        // 清理
        let _ = std::fs::remove_file(&path);
    }

    // ========================================================================
    // MV-05: SHA256 校验不匹配
    // ========================================================================

    #[test]
    fn test_validate_sha256_mismatch() {
        let path = std::path::PathBuf::from("/tmp/mupc_sha256_test.rknn");
        std::fs::write(&path, b"test content").unwrap();

        // 随机期望值（不会匹配）
        let result = validate_rknn_model(
            &path,
            PredictionModelType::BiLstmAttention,
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"),
        );
        assert!(result.is_err(), "SHA256 不匹配应返回错误");

        // 清理
        let _ = std::fs::remove_file(&path);
    }

    // ========================================================================
    // MV-06: 模型类型一致性校验
    // ========================================================================

    #[test]
    fn test_model_type_consistency() {
        let path = std::path::PathBuf::from("/tmp/mupc_type_test.rknn");

        // 匹配
        assert!(validate_model_type_consistency(
            &path,
            PredictionModelType::BiLstmAttention,
            "bilstm"
        )
        .is_ok());

        // 不匹配
        assert!(validate_model_type_consistency(
            &path,
            PredictionModelType::LstmAttention,
            "bilstm"
        )
        .is_err());

        // 未知类型
        assert!(validate_model_type_consistency(
            &path,
            PredictionModelType::LstmAttention,
            "transformer"
        )
        .is_err());
    }

    // ========================================================================
    // MV-07: PredictionModelType name 方法
    // ========================================================================

    #[test]
    fn test_model_type_names() {
        assert_eq!(PredictionModelType::LstmAttention.name(), "LSTM+Attention");
        assert_eq!(PredictionModelType::BiLstmAttention.name(), "BiLSTM+Attention");
        assert_eq!(PredictionModelType::ErrorCorrection.name(), "误差修正BiLSTM");
    }
}
