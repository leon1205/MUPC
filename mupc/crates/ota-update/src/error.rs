//! OTA 更新错误类型
//!
//! Phase 3C.2 OTA 模型自动更新模块的错误类型定义

use thiserror::Error;

/// OTA 更新错误类型
#[derive(Error, Debug)]
pub enum OtaError {
    /// 网络连接失败
    #[error("网络连接失败: {0}")]
    NetworkError(String),

    /// 下载失败
    #[error("下载失败: {0}")]
    DownloadFailed(String),

    /// 下载空间不足
    #[error("下载空间不足: 需要 {need} 字节, 可用 {available} 字节")]
    InsufficientSpace { need: u64, available: u64 },

    /// 校验失败
    #[error("校验失败: {0}")]
    VerificationFailed(String),

    /// 签名验证失败
    #[error("签名验证失败")]
    SignatureInvalid,

    /// 模型加载失败
    #[error("模型加载失败: {0}")]
    ModelLoadFailed(String),

    /// 版本不兼容
    #[error("版本不兼容: 当前 {current}, 需要 {required}")]
    VersionIncompatible { current: String, required: String },

    /// 更新超时
    #[error("更新超时")]
    UpdateTimeout,

    /// 回滚失败
    #[error("回滚失败: {0}")]
    RollbackFailed(String),

    /// 回滚次数超限
    #[error("回滚次数超限")]
    RollbackLimitExceeded,

    /// 解压失败
    #[error("解压失败: {0}")]
    DecompressionFailed(String),

    /// 版本查询失败
    #[error("版本查询失败: {0}")]
    VersionQueryFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn test_network_error() {
        let err = OtaError::NetworkError("连接超时".to_string());
        assert!(err.to_string().contains("网络连接失败"));
        assert!(err.to_string().contains("连接超时"));
        assert!(err.source().is_none());
    }

    #[test]
    fn test_download_failed() {
        let err = OtaError::DownloadFailed("404 Not Found".to_string());
        assert!(err.to_string().contains("下载失败"));
        assert!(err.to_string().contains("404 Not Found"));
    }

    #[test]
    fn test_insufficient_space() {
        let err = OtaError::InsufficientSpace {
            need: 1024 * 1024 * 100,
            available: 1024 * 1024 * 50,
        };
        let msg = err.to_string();
        assert!(msg.contains("下载空间不足"));
        assert!(msg.contains("需要"));
        assert!(msg.contains("可用"));
        assert!(msg.contains("104857600")); // 100MB in bytes
        assert!(msg.contains("52428800"));  // 50MB in bytes
    }

    #[test]
    fn test_verification_failed() {
        let err = OtaError::VerificationFailed("SHA-256 校验和不匹配".to_string());
        assert!(err.to_string().contains("校验失败"));
        assert!(err.to_string().contains("SHA-256 校验和不匹配"));
    }

    #[test]
    fn test_signature_invalid() {
        let err = OtaError::SignatureInvalid;
        assert_eq!(err.to_string(), "签名验证失败");
    }

    #[test]
    fn test_model_load_failed() {
        let err = OtaError::ModelLoadFailed("RKNN 运行时初始化失败".to_string());
        assert!(err.to_string().contains("模型加载失败"));
        assert!(err.to_string().contains("RKNN 运行时初始化失败"));
    }

    #[test]
    fn test_version_incompatible() {
        let err = OtaError::VersionIncompatible {
            current: "1.0.0".to_string(),
            required: "1.2.0".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("版本不兼容"));
        assert!(msg.contains("当前"));
        assert!(msg.contains("需要"));
        assert!(msg.contains("1.0.0"));
        assert!(msg.contains("1.2.0"));
    }

    #[test]
    fn test_update_timeout() {
        let err = OtaError::UpdateTimeout;
        assert_eq!(err.to_string(), "更新超时");
    }

    #[test]
    fn test_rollback_failed() {
        let err = OtaError::RollbackFailed("备份文件不存在".to_string());
        assert!(err.to_string().contains("回滚失败"));
        assert!(err.to_string().contains("备份文件不存在"));
    }

    #[test]
    fn test_rollback_limit_exceeded() {
        let err = OtaError::RollbackLimitExceeded;
        assert_eq!(err.to_string(), "回滚次数超限");
    }

    #[test]
    fn test_decompression_failed() {
        let err = OtaError::DecompressionFailed("无效的压缩格式".to_string());
        assert!(err.to_string().contains("解压失败"));
        assert!(err.to_string().contains("无效的压缩格式"));
    }

    #[test]
    fn test_version_query_failed() {
        let err = OtaError::VersionQueryFailed("OTA 服务器无响应".to_string());
        assert!(err.to_string().contains("版本查询失败"));
        assert!(err.to_string().contains("OTA 服务器无响应"));
    }

    #[test]
    fn test_error_trait_impl() {
        // 验证所有错误类型都实现了 std::error::Error trait
        let errors: Vec<Box<dyn Error>> = vec![
            Box::new(OtaError::NetworkError("test".to_string())),
            Box::new(OtaError::DownloadFailed("test".to_string())),
            Box::new(OtaError::InsufficientSpace { need: 1, available: 0 }),
            Box::new(OtaError::VerificationFailed("test".to_string())),
            Box::new(OtaError::SignatureInvalid),
            Box::new(OtaError::ModelLoadFailed("test".to_string())),
            Box::new(OtaError::VersionIncompatible {
                current: "1.0".to_string(),
                required: "2.0".to_string(),
            }),
            Box::new(OtaError::UpdateTimeout),
            Box::new(OtaError::RollbackFailed("test".to_string())),
            Box::new(OtaError::RollbackLimitExceeded),
            Box::new(OtaError::DecompressionFailed("test".to_string())),
            Box::new(OtaError::VersionQueryFailed("test".to_string())),
        ];

        for err in errors {
            // 验证每个错误都可以转换为字符串
            let _ = err.to_string();
            // 验证 source() 方法可用
            let _ = err.source();
        }
    }

    #[test]
    fn test_error_display_format() {
        // 测试错误消息格式一致性
        let err = OtaError::NetworkError("connection refused".to_string());
        let msg = format!("{}", err);
        assert!(msg.starts_with("网络连接失败:"));

        let err = OtaError::DownloadFailed("http error".to_string());
        let msg = format!("{}", err);
        assert!(msg.starts_with("下载失败:"));

        let err = OtaError::VerificationFailed("checksum mismatch".to_string());
        let msg = format!("{}", err);
        assert!(msg.starts_with("校验失败:"));
    }

    #[test]
    fn test_structural_error_messages() {
        // 测试结构化错误消息的格式化
        let space_err = OtaError::InsufficientSpace {
            need: 1000,
            available: 500,
        };
        assert!(space_err.to_string().contains("1000"));
        assert!(space_err.to_string().contains("500"));

        let version_err = OtaError::VersionIncompatible {
            current: "v1.1.0".to_string(),
            required: "v1.2.0".to_string(),
        };
        let msg = version_err.to_string();
        assert!(msg.contains("v1.1.0"));
        assert!(msg.contains("v1.2.0"));
    }
}