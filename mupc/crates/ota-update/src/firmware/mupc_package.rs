//! .mupc 固件包容器格式
//!
//! MUPC 统一的固件包容器格式解析器。
//!
//! # 二进制布局
//!
//! ```text
//! Magic:      4B  "MUPC"
//! Version:    2B  u16 LE
//! HeaderSize: 4B  u32 LE
//! HeaderJSON: N 字节 UTF-8 JSON
//! Signature:  64B SM2 签名
//! Payload:    N 字节 (tar.gz 或 bsdiff)
//! ```

use serde::{Deserialize, Serialize};

use crate::error::OtaError;

pub const MUPC_MAGIC: [u8; 4] = *b"MUPC";
pub const MUPC_CONTAINER_VERSION: u16 = 1;
pub const SIGNATURE_SIZE: usize = 64;

/// 包类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageType {
    Full,
    Incremental,
}

/// .mupc 固件包头信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MupcHeader {
    pub firmware_version: String,
    pub target_hardware: String,
    pub min_required_version: String,
    pub partition: String,
    pub payload_size: u64,
    pub payload_sha256: String,
    pub use_bsdiff: bool,
    pub base_version: Option<String>,
    #[serde(default = "default_package_type")]
    pub package_type: PackageType,
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub target_platform: String,
    #[serde(default)]
    pub min_bootloader_version: String,
}

fn default_package_type() -> PackageType {
    PackageType::Full
}

/// 固件包中的单个文件条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub mode: u32,
    pub checksum: String,
}

/// .mupc 固件包
pub struct MupcPackage {
    pub header: MupcHeader,
    pub signature: Vec<u8>,
    pub payload: Vec<u8>,
}

impl MupcPackage {
    /// 解析 .mupc 二进制数据
    pub fn parse(data: &[u8]) -> Result<Self, OtaError> {
        if data.len() < 10 {
            return Err(OtaError::VerificationFailed(
                "数据长度不足，无法解析 .mupc 头部".into(),
            ));
        }

        // 1. 验证魔数
        if data[..4] != MUPC_MAGIC {
            return Err(OtaError::VerificationFailed(
                "无效的 .mupc 文件魔数".into(),
            ));
        }

        // 2. 读取版本号
        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != MUPC_CONTAINER_VERSION {
            return Err(OtaError::VersionIncompatible {
                current: format!("v{}", version),
                required: format!("v{}", MUPC_CONTAINER_VERSION),
            });
        }

        // 3. 读取 HeaderSize
        let header_size = u32::from_le_bytes([data[6], data[7], data[8], data[9]]) as usize;
        let header_end = 10 + header_size;
        if data.len() < header_end + SIGNATURE_SIZE {
            return Err(OtaError::VerificationFailed(
                "数据长度不足，无法提取 HeaderJSON 和签名".into(),
            ));
        }

        // 4. 解析 HeaderJSON
        let header_json = std::str::from_utf8(&data[10..header_end]).map_err(|e| {
            OtaError::VerificationFailed(format!("HeaderJSON 编码无效: {}", e))
        })?;
        let header: MupcHeader = serde_json::from_str(header_json).map_err(|e| {
            OtaError::VerificationFailed(format!("HeaderJSON 解析失败: {}", e))
        })?;

        // 5. 提取签名（64 字节）
        let sig_start = header_end;
        let sig_end = sig_start + SIGNATURE_SIZE;
        let signature = data[sig_start..sig_end].to_vec();

        // 6. 提取 payload
        let payload = data[sig_end..].to_vec();

        // 7. 验证 payload 大小
        if payload.len() as u64 != header.payload_size {
            return Err(OtaError::VerificationFailed(format!(
                "Payload 大小不匹配: 声明 {} 字节，实际 {} 字节",
                header.payload_size,
                payload.len()
            )));
        }

        // 8. 验证 payload SHA-256
        let actual_hash = {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(&payload);
            hex::encode(hasher.finalize())
        };
        if actual_hash != header.payload_sha256 {
            return Err(OtaError::VerificationFailed(format!(
                "Payload SHA-256 校验失败: 期望 {}, 实际 {}",
                header.payload_sha256, actual_hash
            )));
        }

        Ok(Self {
            header,
            signature,
            payload,
        })
    }

    /// 验证 SM2 签名
    ///
    /// Phase 2+: 集成 security crate 的 SM2 验签功能。
    /// 当前返回 NotImplemented 错误，防止未经验证的固件被接受。
    pub fn verify_signature(&self, _public_key: &[u8]) -> Result<bool, OtaError> {
        let _ = &self.signature;
        tracing::warn!("SM2 签名验证暂未实现，拒绝固件包");
        Err(OtaError::SignatureInvalid)
    }

    pub fn package_type(&self) -> PackageType {
        self.header.package_type
    }

    pub fn is_incremental(&self) -> bool {
        self.header.use_bsdiff || matches!(self.header.package_type, PackageType::Incremental)
    }

    pub fn firmware_version(&self) -> &str {
        &self.header.firmware_version
    }

    pub fn payload_size(&self) -> u64 {
        self.header.payload_size
    }

    pub fn validate_payload_size(&self) -> bool {
        self.payload.len() as u64 == self.header.payload_size
    }

    /// 序列化为 .mupc 二进制格式
    pub fn to_bytes(header: &MupcHeader, signature: &[u8], payload: &[u8]) -> Result<Vec<u8>, OtaError> {
        let header_json = serde_json::to_string(header).map_err(|e| {
            OtaError::IoError(format!("HeaderJSON 序列化失败: {}", e))
        })?;
        let header_bytes = header_json.as_bytes();
        let header_size = header_bytes.len() as u32;

        let total = 4 + 2 + 4 + header_size as usize + 64 + payload.len();
        let mut buf = Vec::with_capacity(total);

        buf.extend_from_slice(&MUPC_MAGIC);
        buf.extend_from_slice(&MUPC_CONTAINER_VERSION.to_le_bytes());
        buf.extend_from_slice(&header_size.to_le_bytes());
        buf.extend_from_slice(header_bytes);

        if signature.len() != 64 {
            return Err(OtaError::SignatureInvalid);
        }
        buf.extend_from_slice(signature);
        buf.extend_from_slice(payload);

        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_package() -> Vec<u8> {
        let payload = b"test firmware payload data".to_vec();
        let payload_sha256 = {
            use sha2::Digest;
            hex::encode(sha2::Sha256::digest(&payload))
        };

        let header = MupcHeader {
            firmware_version: "1.2.0".into(),
            target_hardware: "RK3588".into(),
            min_required_version: "1.0.0".into(),
            partition: "boot_b".into(),
            payload_size: payload.len() as u64,
            payload_sha256,
            use_bsdiff: false,
            base_version: None,
            package_type: PackageType::Full,
            timestamp: 0,
            target_platform: String::new(),
            min_bootloader_version: String::new(),
        };

        let signature = vec![0u8; 64];
        MupcPackage::to_bytes(&header, &signature, &payload).unwrap()
    }

    #[test]
    fn test_parse_valid_package() {
        let data = create_test_package();
        let pkg = MupcPackage::parse(&data).unwrap();
        assert_eq!(pkg.firmware_version(), "1.2.0");
        assert!(!pkg.is_incremental());
        assert!(pkg.validate_payload_size());
    }

    #[test]
    fn test_parse_invalid_magic() {
        let mut data = create_test_package();
        data[0] = b'X';
        assert!(MupcPackage::parse(&data).is_err());
    }

    #[test]
    fn test_parse_truncated() {
        assert!(MupcPackage::parse(b"MUPC").is_err());
    }

    #[test]
    fn test_mupc_magic_constant() {
        assert_eq!(&MUPC_MAGIC, b"MUPC");
    }

    #[test]
    fn test_roundtrip() {
        let original = create_test_package();
        let pkg = MupcPackage::parse(&original).unwrap();
        let rebuilt = MupcPackage::to_bytes(&pkg.header, &pkg.signature, &pkg.payload).unwrap();
        assert_eq!(original, rebuilt);
    }

    #[test]
    fn test_mupc_header_serialization() {
        let header = MupcHeader {
            firmware_version: "1.2.0".to_string(),
            target_hardware: "RK3588".to_string(),
            min_required_version: "1.0.0".to_string(),
            partition: "boot_b".to_string(),
            payload_size: 100,
            payload_sha256: "a".repeat(64),
            use_bsdiff: false,
            base_version: None,
            package_type: PackageType::Full,
            timestamp: 0,
            target_platform: String::new(),
            min_bootloader_version: String::new(),
        };
        let json = serde_json::to_string(&header).unwrap();
        let restored: MupcHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.firmware_version, "1.2.0");
    }
}
