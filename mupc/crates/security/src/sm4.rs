//! SM4 国密对称加密算法实现
//!
//! 实现 GB/T 32907-2016《SM4 分组密码算法》
//!
//! # gmsm 0.1.0 能力说明
//! - 支持 ECB/CBC 模式加密/解密
//! - 不支持 GCM 模式
//!
//! # 安全警告
//! IV 严禁重用！
//!
//! SM4：CBC 真国密保留；GCM 走 ring 兜底（framework-only，勿作合规交付）。

use crate::errors::{GmError, Result};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// SM4 密钥结构（16字节，128位）
///
/// # 密钥不落 Debug、drop 即清零
/// - `Debug` **手写**（不是 derive）：`#[derive(Debug)]` 会把明文密钥写进任何
///   `tracing`/日志/panic 输出 —— 与同 crate 的 [`crate::sm2::Sm2KeyPair`] 保持**同一口径**
///   （那边也是手写 Debug 并刻意丢弃字段）。
/// - [`ZeroizeOnDrop`]：密钥生命周期结束时清零其内存，缩小"密钥残留在堆/栈上"的窗口。
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Sm4Key {
    key: [u8; 16],
}

impl std::fmt::Debug for Sm4Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 刻意**不**打印 `key`（见结构体文档）。
        f.debug_struct("Sm4Key").finish()
    }
}

impl Sm4Key {
    /// 从字节数组创建 SM4 密钥
    pub fn from_bytes(key: &[u8]) -> Result<Self> {
        if key.len() != 16 {
            return Err(GmError::InvalidKeyLength(format!(
                "SM4密钥必须为16字节，当前为 {} 字节",
                key.len()
            )));
        }
        let mut key_array = [0u8; 16];
        key_array.copy_from_slice(key);
        Ok(Self { key: key_array })
    }

    /// 从十六进制字符串创建 SM4 密钥
    pub fn from_hex(hex_str: &str) -> Result<Self> {
        let bytes = hex::decode(hex_str)
            .map_err(|e| GmError::InvalidFormat(format!("Hex解码失败: {}", e)))?;
        Self::from_bytes(&bytes)
    }

    /// 获取密钥字节
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.key
    }
}

/// SM4 CBC 模式加密
pub fn sm4_cbc_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    #[cfg(feature = "real_gmsm")]
    {
        if key.len() != 16 {
            return Err(GmError::InvalidKeyLength(format!(
                "SM4密钥必须为16字节，当前为 {} 字节",
                key.len()
            )));
        }
        if iv.len() != 16 {
            return Err(GmError::InvalidParam(format!(
                "IV必须为16字节，当前为 {} 字节",
                iv.len()
            )));
        }
        Ok(gmsm::sm4::sm4_cbc_encrypt_byte(data, key, iv))
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        Err(GmError::InvalidParam("SM4 CBC 加密需要 gmsm 库".into()))
    }
}

/// SM4 CBC 模式解密
pub fn sm4_cbc_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    #[cfg(feature = "real_gmsm")]
    {
        if key.len() != 16 {
            return Err(GmError::InvalidKeyLength(format!(
                "SM4密钥必须为16字节，当前为 {} 字节",
                key.len()
            )));
        }
        if iv.len() != 16 {
            return Err(GmError::InvalidParam(format!(
                "IV必须为16字节，当前为 {} 字节",
                iv.len()
            )));
        }
        Ok(gmsm::sm4::sm4_cbc_decrypt_byte(data, key, iv))
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        Err(GmError::InvalidParam("SM4 CBC 解密需要 gmsm 库".into()))
    }
}

/// SM4 GCM 模式加密（带认证标签）
///
/// gmsm 0.1.0 不支持 GCM 模式，使用 ring AES-128-GCM 模拟（SM4 与 AES-128 密钥长度一致）。
pub fn sm4_gcm_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_128_GCM};

    if key.len() != 16 {
        return Err(GmError::InvalidKeyLength(format!(
            "SM4密钥必须为16字节，当前为 {} 字节",
            key.len()
        )));
    }
    if iv.len() < 12 {
        return Err(GmError::InvalidParam(format!(
            "GCM IV 必须至少为 12 字节，当前为 {} 字节",
            iv.len()
        )));
    }

    let unbound_key = UnboundKey::new(&AES_128_GCM, key)
        .map_err(|e| GmError::EncryptFailed(format!("密钥创建失败: {:?}", e)))?;
    let less_safe_key = LessSafeKey::new(unbound_key);

    let mut in_out = data.to_vec();
    let tag = less_safe_key
        .seal_in_place_separate_tag(
            Nonce::assume_unique_for_key(iv[..12].try_into().unwrap()),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|e| GmError::EncryptFailed(format!("加密失败: {:?}", e)))?;

    let mut result = in_out;
    result.extend_from_slice(tag.as_ref());
    Ok(result)
}

/// SM4 GCM 模式解密
///
/// gmsm 0.1.0 不支持 GCM 模式，使用 ring AES-128-GCM 模拟。
pub fn sm4_gcm_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_128_GCM};

    if key.len() != 16 {
        return Err(GmError::InvalidKeyLength(format!(
            "SM4密钥必须为16字节，当前为 {} 字节",
            key.len()
        )));
    }
    if iv.len() < 12 {
        return Err(GmError::InvalidParam(format!(
            "GCM IV 必须至少为 12 字节，当前为 {} 字节",
            iv.len()
        )));
    }
    if data.len() < 16 {
        return Err(GmError::InvalidFormat("密文长度不足".to_string()));
    }

    let unbound_key = UnboundKey::new(&AES_128_GCM, key)
        .map_err(|e| GmError::DecryptFailed(format!("密钥创建失败: {:?}", e)))?;
    let less_safe_key = LessSafeKey::new(unbound_key);

    // open_in_place 需要完整的密文+标签作为输入
    let mut in_out = data.to_vec();

    let plaintext = less_safe_key
        .open_in_place(
            Nonce::assume_unique_for_key(iv[..12].try_into().unwrap()),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|e| GmError::DecryptFailed(format!("解密失败: {:?}", e)))?;

    Ok(plaintext.to_vec())
}

/// GCM IV 长度（PRD 06 §3.8：每次随机 12 B；GCM 也只取前 12 B）。
pub const IV_LEN: usize = 12;

/// 生成随机 IV
///
/// # fail-closed
/// 每次调用都必须**真实取自熵源**。熵源失败 ⇒ 返回 `Err` —— **绝不**退化为全零/可预测 IV
/// （IV 重用/全零会让 GCM 的机密性与完整性同时失效，且**静默**）。
pub fn generate_iv() -> Result<Vec<u8>> {
    generate_iv_with(|buf| getrandom::getrandom(buf).map_err(|e| e.to_string()))
}

/// [`generate_iv`] 的**可注入 RNG** 版本。
///
/// 唯一目的是让"熵源失败"这条 fail-closed 路径**可被实测**（否则该分支只能靠读代码相信）。
/// 生产入口 [`generate_iv`] 即此函数 + `getrandom`。
fn generate_iv_with<F>(mut fill: F) -> Result<Vec<u8>>
where
    F: FnMut(&mut [u8]) -> std::result::Result<(), String>,
{
    let mut iv = vec![0u8; IV_LEN];
    fill(&mut iv).map_err(|e| {
        GmError::CryptoError(format!(
            "随机 IV 生成失败（fail-closed：绝不用全零 IV 兜底）: {e}"
        ))
    })?;
    Ok(iv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sm4_key_from_hex() {
        let hex_key = "0123456789abcdef0123456789abcdef";
        let key = Sm4Key::from_hex(hex_key).unwrap();
        assert_eq!(key.as_bytes().len(), 16);
    }

    #[test]
    fn test_sm4_key_invalid_length() {
        let result = Sm4Key::from_bytes(&[1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_sm4_key_from_bytes() {
        let key_bytes = [0u8; 16];
        let key = Sm4Key::from_bytes(&key_bytes).unwrap();
        assert_eq!(key.as_bytes().len(), 16);
    }

    #[test]
    fn test_sm4_gcm_encrypt_decrypt() {
        let key = [0u8; 16];
        let iv = [0u8; 12];
        let plaintext = b"Hello, SM4!";

        let ciphertext = sm4_gcm_encrypt(plaintext, &key, &iv).unwrap();
        let decrypted = sm4_gcm_decrypt(&ciphertext, &key, &iv).unwrap();

        assert_eq!(plaintext.to_vec(), decrypted);
    }

    /// 判别力：`Debug` 里**不得**出现密钥字节。
    ///
    /// 旧实现 `#[derive(Debug)]` 会打印 `Sm4Key { key: [222, 173, ...] }` ⇒ 本用例红。
    #[test]
    fn debug_never_prints_the_key_bytes() {
        use zeroize::Zeroize as _;

        let mut key = Sm4Key::from_hex("deadbeef00112233445566778899aabb").unwrap();
        let printed = format!("{:?}", key);
        // 逐字节都不许出现在 Debug 输出里（`222`/`173` 是 `deadbeef` 的前两字节）
        for byte in key.as_bytes() {
            assert!(
                !printed.contains(&byte.to_string()),
                "Debug 输出泄漏了密钥字节 {byte}: {printed}"
            );
        }
        assert_eq!(printed, "Sm4Key", "只印结构名");
        assert_eq!(format!("{:#?}", key), "Sm4Key");

        // 顺带钉住 Zeroize：显式清零后缓冲必须全零（若 `Zeroize` 被摘掉，本段编译不过）
        assert!(key.as_bytes().iter().any(|&b| b != 0));
        key.zeroize();
        assert!(
            key.as_bytes().iter().all(|&b| b == 0),
            "显式 zeroize 后密钥缓冲必须全零"
        );
    }

    /// 判别力：熵源失败必须 `Err`，**不得**给全零 IV。
    ///
    /// 旧实现 `getrandom(..).unwrap_or_default()` 在失败时返回全零 IV ⇒ 本用例红。
    #[test]
    fn rng_failure_must_err_not_fall_back_to_a_zero_iv() {
        let failed = generate_iv_with(|_| Err("entropy source unavailable".to_string()));
        match failed {
            Err(GmError::CryptoError(msg)) => {
                assert!(msg.contains("fail-closed"), "文案须点明 fail-closed: {msg}");
            }
            other => panic!("熵源失败必须 fail-closed（Err），实际: {other:?}"),
        }

        // 反证：成功路径给的是 **IV_LEN** 字节且（几乎必然）非全零的实际随机值
        let ok = generate_iv().expect("本机熵源可用");
        assert_eq!(ok.len(), IV_LEN, "IV 长度按 PRD 06 §3.8 为 12 B");
        assert!(ok.iter().any(|&b| b != 0), "随机 IV 不应恒为全零");
    }

    #[cfg(feature = "real_gmsm")]
    #[test]
    fn test_sm4_cbc_encrypt_decrypt() {
        let key = [0u8; 16];
        let iv = [0u8; 16];
        let plaintext = b"Hello, SM4 CBC Mode!";

        let ciphertext = sm4_cbc_encrypt(plaintext, &key, &iv).unwrap();
        let decrypted = sm4_cbc_decrypt(&ciphertext, &key, &iv).unwrap();

        assert_eq!(plaintext.to_vec(), decrypted);
    }
}
