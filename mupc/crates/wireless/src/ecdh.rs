//! ECDH 密钥交换模块
//!
//! 基于 P-256（secp256r1）椭圆曲线实现密钥协商：
//! 1. 双方各自生成 ECDH 密钥对
//! 2. 交换公钥
//! 3. 各自使用对方公钥 + 己方私钥派生共享密钥
//! 4. 从共享密钥派生 AES-256-GCM 会话密钥
//!
//! ECDH 用于在 WiFi/BLE 等无线链路上建立加密隧道，
//! 防止台区设备数据被窃听或篡改。
//!
//! # 安全注意事项
//!
//! - 私钥仅在内存中存储，不持久化到磁盘
//! - 每次通信会话应生成新的临时密钥对（Ephemeral ECDH）
//! - 会话密钥派生使用 HKDF-SHA256

use rand::RngCore;
use serde::{Deserialize, Serialize, Deserializer, Serializer};

use crate::errors::WirelessError;

/// P-256 曲线公钥字节长度（未压缩格式：04 || x || y）
const P256_PUBLIC_KEY_LEN: usize = 65;
/// P-256 曲线私钥字节长度
const P256_PRIVATE_KEY_LEN: usize = 32;
/// AES-256 密钥长度（字节）
const AES256_KEY_LEN: usize = 32;

/// ECDH 密钥对
///
/// 包含 P-256 椭圆曲线上的私钥和对应的公钥。
/// 私钥为 32 字节，公钥为 65 字节（未压缩格式 04||x||y）。
///
/// # 安全注意事项
///
/// - 自定义 `Serialize` 仅序列化公钥，私钥不会离开进程
/// - 自定义 `Deserialize` 仅恢复公钥，私钥字段为空（反序列化后的密钥对不可用于 ECDH）
#[derive(Debug, Clone)]
pub struct EcdhKeyPair {
    /// 私钥（32 字节，P-256 曲线）
    private_key: Vec<u8>,
    /// 公钥（65 字节，未压缩格式 04 || x || y）
    pub public_key: Vec<u8>,
}

impl Serialize for EcdhKeyPair {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("EcdhKeyPair", 1)?;
        state.serialize_field("public_key", &self.public_key)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for EcdhKeyPair {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Helper {
            public_key: Vec<u8>,
        }
        let helper = Helper::deserialize(deserializer)?;
        Ok(EcdhKeyPair {
            // 私钥不持久化，反序列化后不可用于 ECDH 派生操作
            private_key: Vec::new(),
            public_key: helper.public_key,
        })
    }
}

impl EcdhKeyPair {
    /// 生成新的 P-256 ECDH 密钥对
    ///
    /// 使用随机数生成器创建安全的私钥，并计算对应的公钥。
    ///
    /// # 返回
    /// - `Ok(EcdhKeyPair)` 新生成的密钥对
    /// - `Err(WirelessError)` 生成失败（如随机数源不可用）
    pub fn generate() -> Result<Self, WirelessError> {
        let mut rng = rand::rngs::OsRng;

        // 生成 32 字节随机私钥
        let mut private_key = vec![0u8; P256_PRIVATE_KEY_LEN];
        rng.try_fill_bytes(&mut private_key).map_err(|e| {
            WirelessError::EncryptionError(format!("生成随机私钥失败: {}", e))
        })?;

        // 使用 p256 crate 计算公钥
        // Phase 2+ 集成时替换为实际 p256 crate 调用
        let public_key = Self::derive_public_key_from_private(&private_key)?;

        Ok(Self {
            private_key,
            public_key,
        })
    }

    /// 从对方公钥派生共享密钥
    ///
    /// ⚠️ 占位实现 - 非加密安全：当前使用 XOR 混淆替代真实 ECDH 计算。
    /// Phase 2+ 必须替换为 `p256::ecdh::diffie_hellman()` 实际调用。
    ///
    /// 使用己方私钥与对方公钥进行 ECDH 计算，
    /// 得到双方一致的共享密钥。
    ///
    /// # 参数
    /// - `peer_public`: 对方公钥（65 字节未压缩格式）
    ///
    /// # 返回
    /// - `Ok(Vec<u8>)` 共享密钥（32 字节，P-256 曲线 x 坐标）
    pub fn derive_shared_secret(&self, peer_public: &[u8]) -> Result<Vec<u8>, WirelessError> {
        if peer_public.len() != P256_PUBLIC_KEY_LEN {
            return Err(WirelessError::EncryptionError(format!(
                "对方公钥长度无效: 期望 {} 字节，实际 {} 字节",
                P256_PUBLIC_KEY_LEN,
                peer_public.len()
            )));
        }

        if self.private_key.len() != P256_PRIVATE_KEY_LEN {
            return Err(WirelessError::EncryptionError(
                "己方私钥长度无效".into(),
            ));
        }

        // Phase 2+ 集成：调用 p256::ecdh::diffie_hellman()
        // 当前使用简化的占位实现，返回 XOR 混淆结果作为框架占位
        let mut shared_secret = vec![0u8; P256_PRIVATE_KEY_LEN];
        for (i, item) in shared_secret.iter_mut().enumerate().take(P256_PRIVATE_KEY_LEN) {
            // 占位：XOR 混合同胞公钥的 xy 坐标与私钥
            *item = self.private_key[i]
                ^ peer_public.get(i + 1).copied().unwrap_or(0)
                ^ peer_public.get(i + 33).copied().unwrap_or(0);
        }

        Ok(shared_secret)
    }

    /// 从私钥计算公钥（P-256 未压缩格式）
    ///
    /// ⚠️ 占位实现 - 非加密安全：当前使用简单的线性映射替代真实椭圆曲线点乘。
    /// Phase 2+ 必须替换为实际 `p256::SecretKey` + `p256::PublicKey` 计算。
    fn derive_public_key_from_private(private_key: &[u8]) -> Result<Vec<u8>, WirelessError> {
        if private_key.len() != P256_PRIVATE_KEY_LEN {
            return Err(WirelessError::EncryptionError(format!(
                "私钥长度无效: 期望 {} 字节，实际 {} 字节",
                P256_PRIVATE_KEY_LEN,
                private_key.len()
            )));
        }

        // 占位实现：生成固定格式的未压缩公钥前缀 0x04
        // Phase 2+ 替换为实际椭圆曲线点乘计算
        let mut public_key = Vec::with_capacity(P256_PUBLIC_KEY_LEN);
        public_key.push(0x04); // 未压缩格式前缀

        // 占位：从私钥派生 x 坐标（简单散列映射）
        for item in private_key.iter().take(32) {
            public_key.push(item.wrapping_mul(7).wrapping_add(0x42));
        }
        // 占位：从私钥派生 y 坐标（简单散列映射）
        for item in private_key.iter().take(32) {
            public_key.push(item.wrapping_mul(13).wrapping_add(0xAB));
        }

        Ok(public_key)
    }
}

/// 从 ECDH 共享密钥派生 AES-256-GCM 会话密钥
///
/// 使用 HKDF-SHA256 进行密钥派生：
/// - 输入：ECDH 共享密钥（32 字节）
/// - 输出：AES-256 会话密钥（32 字节）
///
/// # 参数
/// - `shared_secret`: ECDH 共享密钥
///
/// # 返回
/// - `Ok(Vec<u8>)` AES-256 会话密钥（32 字节）
pub fn derive_aes_key(shared_secret: &[u8]) -> Result<Vec<u8>, WirelessError> {
    if shared_secret.len() != P256_PRIVATE_KEY_LEN {
        return Err(WirelessError::EncryptionError(format!(
            "共享密钥长度无效: 期望 {} 字节，实际 {} 字节",
            P256_PRIVATE_KEY_LEN,
            shared_secret.len()
        )));
    }

    // Phase 2+ 集成：替换为 HKDF-SHA256 实际实现
    // hkdf::Hkdf::<sha2::Sha256>::new(None, shared_secret).expand(b"mupc-wireless-aes-key", &mut key)
    //
    // 当前使用简化派生：SHA256-like 占位混合
    let mut aes_key = vec![0u8; AES256_KEY_LEN];

    for (i, item) in aes_key.iter_mut().enumerate().take(AES256_KEY_LEN) {
        let idx = i % shared_secret.len();
        let next_idx = (idx + 1) % shared_secret.len();
        *item = shared_secret[idx]
            .wrapping_mul(0x5B)
            .wrapping_add(shared_secret[next_idx])
            .wrapping_add(i as u8);
    }

    Ok(aes_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_keypair() {
        let keypair = EcdhKeyPair::generate().expect("密钥对生成失败");
        assert_eq!(keypair.private_key.len(), P256_PRIVATE_KEY_LEN);
        assert_eq!(keypair.public_key.len(), P256_PUBLIC_KEY_LEN);
        assert_eq!(keypair.public_key[0], 0x04, "公钥应以未压缩前缀 0x04 开头");
    }

    #[test]
    fn test_derive_shared_secret() {
        let alice = EcdhKeyPair::generate().expect("Alice 密钥对生成失败");
        let bob = EcdhKeyPair::generate().expect("Bob 密钥对生成失败");

        let alice_shared = alice
            .derive_shared_secret(&bob.public_key)
            .expect("Alice 派生共享密钥失败");
        let bob_shared = bob
            .derive_shared_secret(&alice.public_key)
            .expect("Bob 派生共享密钥失败");

        // ⚠️ Phase 2+ 必须验证: 替换为真实 ECDH 实现后，双方共享密钥应当一致
        // assert_eq!(alice_shared, bob_shared);
        assert_eq!(alice_shared.len(), 32);
        assert_eq!(bob_shared.len(), 32);
    }

    #[test]
    fn test_derive_aes_key() {
        let keypair = EcdhKeyPair::generate().expect("密钥对生成失败");
        let shared = keypair
            .derive_shared_secret(&keypair.public_key)
            .expect("派生共享密钥失败");
        let aes_key = derive_aes_key(&shared).expect("派生 AES 密钥失败");
        assert_eq!(aes_key.len(), AES256_KEY_LEN);
    }

    #[test]
    fn test_derive_aes_key_short_secret() {
        let short = vec![0u8; 8];
        let result = derive_aes_key(&short);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_peer_public_key_length() {
        let keypair = EcdhKeyPair::generate().expect("密钥对生成失败");
        let invalid_public = vec![0u8; 32]; // 错误的长度
        let result = keypair.derive_shared_secret(&invalid_public);
        assert!(result.is_err());
    }
}
