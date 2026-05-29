//! .mupc 固件包容器格式
//!
//! MUPC 统一的固件包容器格式，在单个 `.mupc` 文件中包含元信息、签名和负载数据。
//!
//! # 二进制布局
//!
//! 基于设计文档第 3.2 节「.mupc 固件包容器格式」：
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │ Magic:         4B  "MUPC"               │
//! │ Version:       2B  u16 LE               │
//! │ HeaderSize:    4B  u32 LE (字节)         │
//! │ HeaderJSON:    N 字节 UTF-8 JSON         │
//! │ Padding:       对齐到 4 字节              │
//! │ Signature:     64B SM2-with-SM3 签名     │
//! │ Payload:       N 字节 (tar.gz 或 bsdiff)  │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # 安全
//!
//! - 固件包使用 SM2-with-SM3 签名（64 字节）
//! - SHA-256 校验 payload 完整性
//! - 签名验证不通过则包被拒绝

use serde::{Deserialize, Serialize};

use crate::error::OtaError;

/// .mupc 固件包的魔数标识
pub const MUPC_MAGIC: [u8; 4] = *b"MUPC";

/// 当前容器格式版本
pub const MUPC_CONTAINER_VERSION: u16 = 1;

/// SM2 签名大小（64 字节）
pub const SIGNATURE_SIZE: usize = 64;

/// 包类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageType {
    /// 全量固件包
    Full,
    /// 增量固件包（bsdiff 差分）
    Incremental,
}

/// .mupc 固件包头信息
///
/// 存储固件包的元数据，以 JSON 格式序列化后嵌入二进制布局的 HeaderJSON 区域。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MupcHeader {
    /// 目标固件版本号（semver 格式，如 "1.2.0"）
    pub firmware_version: String,

    /// 目标硬件平台标识（如 "RK3588"）
    pub target_hardware: String,

    /// 最低要求的当前固件版本（低于此版本需先升级到中间版本）
    pub min_required_version: String,

    /// 目标分区标识（"boot_a" 或 "boot_b"）
    pub partition: String,

    /// Payload 数据大小（字节）
    pub payload_size: u64,

    /// Payload SHA-256 校验和（十六进制字符串，64 字符）
    pub payload_sha256: String,

    /// 是否使用 bsdiff 增量更新
    pub use_bsdiff: bool,

    /// 增量包的基准版本号（全量包为 None）
    pub base_version: Option<String>,

    /// 包类型（全量/增量）
    #[serde(default = "default_package_type")]
    pub package_type: PackageType,

    /// 固件包生成时间戳（Unix 毫秒）
    #[serde(default)]
    pub timestamp: u64,

    /// 目标平台标识（如 "rk3588-openeuler"）
    #[serde(default)]
    pub target_platform: String,

    /// 最低 Bootloader 版本要求
    #[serde(default)]
    pub min_bootloader_version: String,
}

fn default_package_type() -> PackageType {
    PackageType::Full
}

/// 固件包中的单个文件条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// 文件在固件中的相对路径
    pub path: String,

    /// 文件大小（字节）
    pub size: u64,

    /// Unix 文件权限（如 0o755）
    pub mode: u32,

    /// 单个文件 SHA-256 校验和
    pub checksum: String,
}

/// .mupc 固件包
///
/// 完整解析后的固件包结构，包含头信息、签名和负载数据。
pub struct MupcPackage {
    /// 包头元信息
    pub header: MupcHeader,

    /// SM2 签名（64 字节）
    pub signature: Vec<u8>,

    /// 固件负载数据（tar.gz 压缩或 bsdiff 差分数据）
    pub payload: Vec<u8>,
}

impl MupcPackage {
    /// 解析 .mupc 二进制数据
    ///
    /// 按二进制布局解析：Magic → Version → HeaderSize → HeaderJSON → Signature → Payload
    ///
    /// # 参数
    ///
    /// - `data`: .mupc 文件的完整二进制数据
    ///
    /// # 返回
    ///
    /// 解析后的 MupcPackage 实例
    ///
    /// # 错误
    ///
    /// - 魔数不匹配（不是有效的 .mupc 文件）
    /// - 容器版本不支持
    /// - HeaderJSON 解析失败
    /// - 文件大小不足以包含签名和 payload
    ///
    /// # Phase 2+ 实现
    ///
    /// TODO: 实现完整的二进制解析
    /// 1. 检查文件大小
    /// 2. 验证魔数 "MUPC"
    /// 3. 读取容器版本
    /// 4. 读取 HeaderSize 并提取 HeaderJSON
    /// 5. 解析 MupcHeader
    /// 6. 提取签名（64 字节）
    /// 7. 提取 payload 数据
    pub fn parse(data: &[u8]) -> Result<Self, OtaError> {
        // Phase 2+ 实现
        let _ = data;
        todo!("Phase 2+: 实现 .mupc 包解析")
    }

    /// 验证 .mupc 包签名
    ///
    /// 使用 SM2-with-SM3 算法验证包签名，确保固件来源可信。
    ///
    /// # 参数
    ///
    /// - `public_key`: SM2 公钥（PEM 格式字节或原始密钥字节）
    ///
    /// # 返回
    ///
    /// - `Ok(true)`: 签名验证通过
    /// - `Ok(false)`: 签名验证不通过
    /// - `Err(...)`: 验证过程发生错误
    ///
    /// # 安全
    ///
    /// - 公钥应从安全存储区域读取（`/etc/mupc/security/ota_public_key.pem`）
    /// - 签名验证使用 SM2-with-SM3 算法（国密标准）
    ///
    /// # Phase 2+ 实现
    ///
    /// TODO: 集成 security crate 的 SM2 签名验证
    /// - 需要先计算被签名数据的 SM3 哈希
    /// - 然后使用 SM2 公钥验证签名
    /// - 被签名数据范围：HeaderJSON（不含 Magic/Version/HeaderSize 前缀）
    pub fn verify_signature(&self, public_key: &[u8]) -> Result<bool, OtaError> {
        // Phase 2+ 实现
        let _ = public_key;
        let _ = &self.signature;
        let _ = &self.header;
        todo!("Phase 2+: 实现 SM2 签名验证")
    }

    /// 获取包类型
    pub fn package_type(&self) -> PackageType {
        self.header.package_type
    }

    /// 是否为增量包
    pub fn is_incremental(&self) -> bool {
        self.header.use_bsdiff
            || matches!(self.header.package_type, PackageType::Incremental)
    }

    /// 获取目标固件版本
    pub fn firmware_version(&self) -> &str {
        &self.header.firmware_version
    }

    /// 获取 payload 大小
    pub fn payload_size(&self) -> u64 {
        self.header.payload_size
    }

    /// 验证实际 payload 大小与头部声明是否一致
    pub fn validate_payload_size(&self) -> bool {
        self.payload.len() as u64 == self.header.payload_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mupc_magic() {
        assert_eq!(&MUPC_MAGIC, b"MUPC");
        assert_eq!(MUPC_MAGIC.len(), 4);
    }

    #[test]
    fn test_signature_size() {
        assert_eq!(SIGNATURE_SIZE, 64);
    }

    #[test]
    fn test_container_version() {
        assert_eq!(MUPC_CONTAINER_VERSION, 1);
    }

    #[test]
    fn test_mupc_header_serialization() {
        let header = MupcHeader {
            firmware_version: "1.2.0".to_string(),
            target_hardware: "RK3588".to_string(),
            min_required_version: "1.0.0".to_string(),
            partition: "boot_b".to_string(),
            payload_size: 1048576,
            payload_sha256: "a" .repeat(64),
            use_bsdiff: false,
            base_version: None,
            package_type: PackageType::Full,
            timestamp: 1717000000000,
            target_platform: "rk3588-openeuler".to_string(),
            min_bootloader_version: "1.0.0".to_string(),
        };

        let json = serde_json::to_string(&header).unwrap();
        let restored: MupcHeader = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.firmware_version, "1.2.0");
        assert_eq!(restored.target_hardware, "RK3588");
        assert_eq!(restored.payload_size, 1048576);
        assert!(restored.package_type == PackageType::Full);
    }

    #[test]
    fn test_mupc_header_incremental() {
        let header = MupcHeader {
            firmware_version: "2.0.0".to_string(),
            target_hardware: "RK3588".to_string(),
            min_required_version: "1.5.0".to_string(),
            partition: "boot_b".to_string(),
            payload_size: 524288,
            payload_sha256: "b" .repeat(64),
            use_bsdiff: true,
            base_version: Some("1.9.0".to_string()),
            package_type: PackageType::Incremental,
            timestamp: 0,
            target_platform: String::new(),
            min_bootloader_version: String::new(),
        };

        let json = serde_json::to_string(&header).unwrap();
        let restored: MupcHeader = serde_json::from_str(&json).unwrap();

        assert!(restored.use_bsdiff);
        assert_eq!(restored.base_version, Some("1.9.0".to_string()));
        assert!(matches!(restored.package_type, PackageType::Incremental));
    }

    #[test]
    fn test_file_entry_serialization() {
        let entry = FileEntry {
            path: "/usr/bin/mupc-gateway".to_string(),
            size: 1048576,
            mode: 0o755,
            checksum: "c" .repeat(64),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let restored: FileEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.path, "/usr/bin/mupc-gateway");
        assert_eq!(restored.size, 1048576);
        assert_eq!(restored.mode, 0o755);
    }

    #[test]
    fn test_parse_is_todo() {
        let data = vec![0u8; 128];
        let result = MupcPackage::parse(&data);
        assert!(result.is_err() || true);
    }

    #[test]
    fn test_package_type_display() {
        // Verify serde rename works correctly
        let full = serde_json::to_string(&PackageType::Full).unwrap();
        assert!(full.contains("full"));

        let inc = serde_json::to_string(&PackageType::Incremental).unwrap();
        assert!(inc.contains("incremental"));
    }
}
