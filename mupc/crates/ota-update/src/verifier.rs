//! 验证器模块 - 签名和完整性校验
//!
//! Phase 3C.2 OTA 模型自动更新模块的验证器实现
//! 支持 SHA-256 文件完整性校验、Ed25519/SM2 签名验证

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncReadExt;

use crate::error::OtaError;

/// 签名算法类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    Ed25519,
    SM2,
}

/// OTA 验证器
///
/// 负责模型文件的签名验证和完整性校验
#[derive(Debug, Clone)]
pub struct Verifier {
    /// 公钥文件路径
    #[allow(dead_code)]
    public_key_path: PathBuf,
}

/// RKNN 文件魔数标识（文件头前 16 字节）
/// RKNN 文件通常以特定字节序列开头，用于标识文件格式
#[allow(dead_code)]
const RKNN_MAGIC: &[u8] = b"RKNN";
#[allow(dead_code)]
const RKNN_MODEL_MAGIC_ALT: &[u8] = b"RKNM"; // 某些变体可能使用此标识

/// 文件大小约束（字节）
const MIN_RKNN_SIZE: u64 = 1_048_576; // 1MB
const MAX_RKNN_SIZE: u64 = 524_288_000; // 500MB

/// 平台最低版本要求（platform_version -> 最低版本号）
const PLATFORM_MIN_VERSION: &[(&str, u32)] =
    &[("RK3588", 8), ("RK3588S", 8), ("RK3568", 6), ("RK3568S", 6)];

/// 检查 RKNN 魔数是否有效
///
/// # 参数
/// * `header` - 文件头前 16+ 字节
fn is_valid_rknn_magic(header: &[u8]) -> bool {
    header.starts_with(b"RKNN")
        || header.starts_with(b"RKNM")
        || (header[0] == 0x52 && header[1] == 0x4B && header[2] == 0x4E && header[3] == 0x4E) // "RKNN" 的另一种字节序
        || (header[0] == 0x52 && header[1] == 0x4B && header[2] == 0x4E && header[3] == 0x4D)
    // "RKNM"
}

impl Verifier {
    /// 创建新的验证器实例
    ///
    /// # 参数
    /// * `public_key_path` - 公钥文件路径（Ed25519 公钥或 SM2 公钥）
    pub fn new(public_key_path: PathBuf) -> Result<Self, OtaError> {
        // 验证公钥文件是否存在
        if !public_key_path.exists() {
            return Err(OtaError::VerificationFailed(format!(
                "公钥文件不存在: {}",
                public_key_path.display()
            )));
        }
        Ok(Self { public_key_path })
    }

    /// 验证文件完整性（SHA-256）
    ///
    /// 计算文件的 SHA-256 哈希值并与期望值比较
    ///
    /// # 参数
    /// * `file_path` - 文件路径
    /// * `expected_hash` - 期望的 SHA-256 哈希值（十六进制字符串）
    pub async fn verify_integrity(
        &self,
        file_path: &Path,
        expected_hash: &str,
    ) -> Result<(), OtaError> {
        let mut file = File::open(file_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("打开文件失败: {}", e)))?;

        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 8192];

        loop {
            let bytes_read = file
                .read(&mut buffer)
                .await
                .map_err(|e| OtaError::VerificationFailed(format!("读取文件失败: {}", e)))?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let result = hasher.finalize();
        let actual_hash = format!("{:x}", result);

        if actual_hash.eq_ignore_ascii_case(expected_hash) {
            Ok(())
        } else {
            Err(OtaError::VerificationFailed(format!(
                "SHA-256 校验失败: 期望 {} 实际 {}",
                expected_hash, actual_hash
            )))
        }
    }

    /// 验证签名（Ed25519）
    ///
    /// 使用 Ed25519 算法验证数据签名
    ///
    /// # 参数
    /// * `data` - 待验证数据
    /// * `signature` - 签名字节数组（64 字节）
    #[cfg(feature = "ed25519")]
    pub async fn verify_signature_ed25519(
        &self,
        data: &[u8],
        signature: &[u8],
    ) -> Result<(), OtaError> {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let key_bytes = tokio::fs::read(&self.public_key_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("读取公钥文件失败: {}", e)))?;

        let verifying_key =
            VerifyingKey::from_bytes(key_bytes.as_slice().try_into().map_err(|_| {
                OtaError::VerificationFailed("无效的 Ed25519 公钥格式".to_string())
            })?)
            .map_err(|e| OtaError::VerificationFailed(format!("解析 Ed25519 公钥失败: {}", e)))?;

        let sig = Signature::from_slice(signature)
            .map_err(|_| OtaError::VerificationFailed("无效的 Ed25519 签名格式".to_string()))?;

        verifying_key
            .verify(data, &sig)
            .map_err(|_| OtaError::SignatureInvalid)
    }

    /// 验证签名（SM2 国密算法）
    ///
    /// 使用 SM2 算法验证数据签名
    ///
    /// # 参数
    /// * `data` - 待验证数据
    /// * `signature` - 签名字节数组（64 字节）
    #[cfg(feature = "sm2")]
    pub async fn verify_signature_sm2(
        &self,
        data: &[u8],
        signature: &[u8],
    ) -> Result<(), OtaError> {
        use sm2::signature::Verifier;

        let key_bytes = tokio::fs::read(&self.public_key_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("读取公钥文件失败: {}", e)))?;

        let public_key = sm2::PublicKey::from_bytes(key_bytes.as_slice())
            .map_err(|e| OtaError::VerificationFailed(format!("解析 SM2 公钥失败: {}", e)))?;

        let sig = sm2::Signature::from_bytes(signature)
            .map_err(|e| OtaError::VerificationFailed(format!("解析 SM2 签名失败: {}", e)))?;

        public_key
            .verify(data, &sig)
            .map_err(|_| OtaError::SignatureInvalid)
    }

    /// 验证签名（统一入口）
    ///
    /// 根据指定算法选择 Ed25519 或 SM2 进行签名验证
    ///
    /// # 参数
    /// * `file_path` - 模型文件路径
    /// * `signature` - 签名字节数组（64 字节）
    pub async fn verify_signature(
        &self,
        file_path: &Path,
        signature: &[u8],
    ) -> Result<(), OtaError> {
        // 读取文件数据
        let data = tokio::fs::read(file_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("读取文件失败: {}", e)))?;

        // 默认使用 Ed25519（如果可用），否则尝试 SM2
        #[cfg(feature = "ed25519")]
        {
            return self.verify_signature_ed25519(&data, signature).await;
        }

        #[cfg(feature = "sm2")]
        {
            #[cfg(not(feature = "ed25519"))]
            {
                return self.verify_signature_sm2(&data, signature).await;
            }
        }

        #[cfg(not(any(feature = "ed25519", feature = "sm2")))]
        {
            let _ = (&data, signature);
            Err(OtaError::VerificationFailed(
                "无可用的签名验证算法，请启用 ed25519 或 sm2 feature".to_string(),
            ))
        }
    }

    /// 验证模型格式
    ///
    /// 检查文件扩展名并验证 RKNN 文件头魔数
    ///
    /// # 参数
    /// * `file_path` - 模型文件路径
    pub async fn verify_model_format(&self, file_path: &Path) -> Result<(), OtaError> {
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");

        if !extension.eq_ignore_ascii_case("rknn") {
            return Err(OtaError::VerificationFailed(format!(
                "无效的模型格式: 期望 .rknn，实际 .{}",
                extension
            )));
        }

        // 读取文件前 16 字节进行魔数校验
        let mut file = File::open(file_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("打开文件失败: {}", e)))?;

        let mut header = [0u8; 16];
        file.read_exact(&mut header)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("读取文件头失败: {}", e)))?;

        // 检查是否为有效的 RKNN 魔数
        if !is_valid_rknn_magic(&header) {
            return Err(OtaError::VerificationFailed(
                "无效的 RKNN 文件头：魔数不匹配".to_string(),
            ));
        }

        Ok(())
    }

    /// 验证平台兼容性
    ///
    /// 检查模型文件大小和 RKNN 头部信息以验证与 RK3588 平台的兼容性
    ///
    /// # 参数
    /// * `file_path` - 模型文件路径
    /// * `platform_version` - 目标平台版本
    pub async fn verify_platform_compatibility(
        &self,
        file_path: &Path,
        platform_version: &str,
    ) -> Result<(), OtaError> {
        // 读取文件元数据
        let metadata = tokio::fs::metadata(file_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("获取文件元数据失败: {}", e)))?;

        let file_size = metadata.len();

        // 检查文件大小是否在合理范围内
        if file_size < MIN_RKNN_SIZE {
            return Err(OtaError::VerificationFailed(format!(
                "文件大小 {} 字节低于最小要求 {} 字节（1MB），可能不是有效的 RKNN 模型",
                file_size, MIN_RKNN_SIZE
            )));
        }

        if file_size > MAX_RKNN_SIZE {
            return Err(OtaError::VerificationFailed(format!(
                "文件大小 {} 字节超过最大限制 {} 字节（500MB）",
                file_size, MAX_RKNN_SIZE
            )));
        }

        // 读取 RKNN 文件头部信息
        let mut file = File::open(file_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("打开文件失败: {}", e)))?;

        let mut header = [0u8; 32];
        file.read_exact(&mut header)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("读取 RKNN 头部失败: {}", e)))?;

        // 验证 RKNN 魔数
        if !is_valid_rknn_magic(&header) {
            return Err(OtaError::VerificationFailed(
                "平台兼容性验证失败：文件头魔数不匹配，可能不是有效的 RKNN 模型".to_string(),
            ));
        }

        // 从头部提取平台版本信息（偏移量 8-15 字节为版本字段）
        // 格式：魔数(4) + 版本(4) + 平台标识(4) + 保留(20)
        let version_bytes = &header[4..8];

        // 解析版本信息（大端序）
        let version = u32::from_be_bytes([
            version_bytes[0],
            version_bytes[1],
            version_bytes[2],
            version_bytes[3],
        ]);

        tracing::info!(
            "平台兼容性校验: 文件 {:?}, 平台版本 {}, RKNN 头部版本 {}",
            file_path,
            platform_version,
            version
        );

        // 查询最低版本要求
        let min_version = PLATFORM_MIN_VERSION
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(platform_version))
            .map(|(_, min)| *min);

        // 如果是已知的平台，进行版本校验
        if let Some(min_ver) = min_version {
            if version < min_ver {
                return Err(OtaError::VersionIncompatible {
                    current: version.to_string(),
                    required: min_ver.to_string(),
                });
            }
        } else {
            tracing::info!("未知的平台版本 {}，跳过详细校验", platform_version);
        }

        Ok(())
    }

    /// 完整验证（一次性执行所有校验）
    ///
    /// 按顺序执行：文件完整性校验 -> 签名验证 -> 模型格式校验 -> 平台兼容性校验
    ///
    /// # 参数
    /// * `file_path` - 模型文件路径
    /// * `expected_hash` - 期望的 SHA-256 哈希值
    /// * `signature` - 签名字节数组（64 字节）
    pub async fn verify(
        &self,
        file_path: &Path,
        expected_hash: &str,
        signature: &[u8],
    ) -> Result<(), OtaError> {
        // 1. 文件完整性校验（SHA-256）
        self.verify_integrity(file_path, expected_hash).await?;

        // 2. 签名验证
        self.verify_signature(file_path, signature).await?;

        // 3. 模型格式校验
        self.verify_model_format(file_path).await?;

        // 4. 平台兼容性校验
        self.verify_platform_compatibility(file_path, "RK3588")
            .await?;

        tracing::info!("模型文件 {:?} 验证通过", file_path);
        Ok(())
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ========== Verifier 创建测试 ==========

    #[test]
    fn test_verifier_new_valid_path() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");

        // 创建空的公钥文件
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path.clone());
        assert!(verifier.is_ok());
        assert_eq!(verifier.unwrap().public_key_path, key_path);
    }

    #[test]
    fn test_verifier_new_invalid_path() {
        let verifier = Verifier::new(PathBuf::from("/nonexistent/path/key.pem"));
        assert!(verifier.is_err());
    }

    // ========== verify_integrity 测试 ==========

    #[tokio::test]
    async fn test_verify_integrity_success() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("test_file.txt");

        // 写入测试数据 "hello world"
        tokio::fs::write(&file_path, b"hello world").await.unwrap();

        // "hello world" 的 SHA-256 哈希值
        let expected_hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

        let result = verifier.verify_integrity(&file_path, expected_hash).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_verify_integrity_failure() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("test_file.txt");

        tokio::fs::write(&file_path, b"hello world").await.unwrap();

        // 使用错误的哈希值
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";

        let result = verifier.verify_integrity(&file_path, wrong_hash).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_verify_integrity_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();

        let result = verifier
            .verify_integrity(&PathBuf::from("/nonexistent/file.txt"), "anyhash")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_verify_integrity_empty_file() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("empty_file.txt");

        tokio::fs::write(&file_path, b"").await.unwrap();

        // 空文件的 SHA-256 哈希值
        let expected_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

        let result = verifier.verify_integrity(&file_path, expected_hash).await;
        assert!(result.is_ok());
    }

    // ========== verify_model_format 测试 ==========

    #[tokio::test]
    async fn test_verify_model_format_valid() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("model.rknn");

        // 写入有效的 RKNN 文件头
        tokio::fs::write(&file_path, b"RKNN\x00\x00\x00\x08RK3588 model data")
            .await
            .unwrap();

        let result = verifier.verify_model_format(&file_path).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_verify_model_format_invalid_extension() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("model.onnx");

        tokio::fs::write(&file_path, b"model data").await.unwrap();

        let result = verifier.verify_model_format(&file_path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_verify_model_format_invalid_magic() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("model.rknn");

        // 写入无效的魔数
        tokio::fs::write(&file_path, b"INVALID_HEADER_DATA")
            .await
            .unwrap();

        let result = verifier.verify_model_format(&file_path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_verify_model_format_no_extension() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("modelfile");

        tokio::fs::write(&file_path, b"RKNN\x00\x00\x00\x08RK3588")
            .await
            .unwrap();

        let result = verifier.verify_model_format(&file_path).await;
        assert!(result.is_err());
    }

    // ========== verify_platform_compatibility 测试 ==========

    #[tokio::test]
    async fn test_verify_platform_compatibility_valid() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("model.rknn");

        // 创建大于 1MB 的有效 RKNN 文件
        let mut data = vec![0u8; 1_048_576 + 100];
        data[0..4].copy_from_slice(b"RKNN");
        data[4..8].copy_from_slice(&8u32.to_be_bytes()); // 版本 8
        data[8..16].copy_from_slice(b"RK3588    "); // 平台标识
        tokio::fs::write(&file_path, data).await.unwrap();

        let result = verifier
            .verify_platform_compatibility(&file_path, "RK3588")
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_verify_platform_compatibility_too_small() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("model.rknn");

        // 创建小于 1MB 的文件
        tokio::fs::write(&file_path, b"RKNN\x00\x00\x00\x08")
            .await
            .unwrap();

        let result = verifier
            .verify_platform_compatibility(&file_path, "RK3588")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_verify_platform_compatibility_invalid_magic() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("model.rknn");

        // 创建大于 1MB 但魔数无效的文件
        let mut data = vec![0u8; 1_048_576 + 100];
        data[0..8].copy_from_slice(b"INVALID ");
        tokio::fs::write(&file_path, data).await.unwrap();

        let result = verifier
            .verify_platform_compatibility(&file_path, "RK3588")
            .await;
        assert!(result.is_err());
    }

    // ========== SignatureAlgorithm 测试 ==========

    #[test]
    fn test_signature_algorithm_debug() {
        assert_eq!(format!("{:?}", SignatureAlgorithm::Ed25519), "Ed25519");
        assert_eq!(format!("{:?}", SignatureAlgorithm::SM2), "SM2");
    }

    #[test]
    fn test_signature_algorithm_copy() {
        let alg = SignatureAlgorithm::Ed25519;
        let _copy = alg;
        assert_eq!(alg, SignatureAlgorithm::Ed25519);
    }

    // ========== Verifier Debug 测试 ==========

    #[test]
    fn test_verifier_debug() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path.clone()).unwrap();
        let debug_str = format!("{:?}", verifier);

        assert!(debug_str.contains("Verifier"));
        assert!(debug_str.contains("public_key_path"));
    }

    // ========== 集成场景测试 ==========

    #[tokio::test]
    async fn test_verify_without_signature_feature() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let key_path = temp_dir.join("public_key.pem");
        std::fs::write(&key_path, b"test key").unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let file_path = temp_dir.join("model.rknn");

        tokio::fs::write(&file_path, b"hello world").await.unwrap();

        // 在没有 ed25519 和 sm2 feature 时，verify_signature 应返回错误
        #[cfg(not(any(feature = "ed25519", feature = "sm2")))]
        {
            let result = verifier.verify_signature(&file_path, &[]).await;
            assert!(result.is_err());
        }
    }
}
