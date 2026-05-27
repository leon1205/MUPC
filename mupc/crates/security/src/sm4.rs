//! SM4 国密对称加密算法实现
//!
//! SM4是国家密码管理局发布的对称加密算法
//!
//! # 警告：模拟实现
//!
//! 当前实现使用 ring 库的 AES-256-GCM，而非真正的 SM4 算法。
//! 这仅用于演示和测试目的。
//!
//! 在实际部署中，应使用GmSSL库或支持SM4的库（如 gmsm crate）。
//!
//! 生产环境需要替换为真正的国密算法实现。

use crate::errors::{GmError, Result};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use std::cmp::min;

/// SM4 密钥结构（32字节，256位）
#[derive(Debug, Clone)]
pub struct Sm4Key {
    key: [u8; 32],
}

impl Sm4Key {
    /// 从字节数组创建 SM4 密钥
    pub fn from_bytes(key: &[u8]) -> Result<Self> {
        if key.len() != 32 {
            return Err(GmError::InvalidFormat("SM4密钥长度必须为32字节".to_string()));
        }
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(key);
        Ok(Self { key: key_array })
    }

    /// 从十六进制字符串创建 SM4 密钥
    pub fn from_hex(hex: &str) -> Result<Self> {
        let bytes = hex::Engine::decode(&hex::engine::general_purpose::STANDARD, hex)
            .map_err(|e| GmError::InvalidFormat(format!("Hex解码失败: {}", e)))?;
        Self::from_bytes(&bytes)
    }

    /// 获取密钥字节
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }
}

/// SM4 GCM 加密（模拟实现）
///
/// 使用 AES-256-GCM 作为 SM4 的模拟实现
/// 注意：这是 AEAD 模式，不需要 PKCS7 填充
/// 函数名已修正为反映实际使用的 GCM 模式
pub fn sm4_gcm_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    if key.len() != 32 {
        return Err(GmError::InvalidFormat("密钥长度必须为32字节".to_string()));
    }
    if iv.len() != 16 {
        return Err(GmError::InvalidFormat("IV长度必须为16字节".to_string()));
    }

    // 使用 ring 库的 AES-256-GCM（SM4 兼容模式）
    let unbound_key = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|e| GmError::EncryptFailed(format!("密钥创建失败: {:?}", e)))?;

    let less_safe_key = LessSafeKey::new(unbound_key);

    // 使用 IV 作为 nonce
    let mut in_out = data.to_vec();

    // 使用 AES-GCM 作为 SM4 的模拟实现
    let tag = less_safe_key
        .seal_in_place_separate_tag(
            Nonce::assume_unique_is_key(iv.try_into().unwrap()),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|e| GmError::EncryptFailed(format!("加密失败: {:?}", e)))?;

    // 追加认证标签
    let mut result = in_out;
    result.extend_from_slice(tag.as_ref());

    Ok(result)
}

/// SM4 GCM 解密（模拟实现）
///
/// 使用 AES-256-GCM 作为 SM4 的模拟实现
/// 函数名已修正为反映实际使用的 GCM 模式
pub fn sm4_gcm_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    if key.len() != 32 {
        return Err(GmError::InvalidFormat("密钥长度必须为32字节".to_string()));
    }
    if iv.len() != 16 {
        return Err(GmError::InvalidFormat("IV长度必须为16字节".to_string()));
    }
    if data.len() < 16 {
        return Err(GmError::InvalidFormat("密文长度不足".to_string()));
    }

    let unbound_key = UnboundKey::new(&AES_256_GCM, key)
        .map_err(|e| GmError::DecryptFailed(format!("密钥创建失败: {:?}", e)))?;

    let less_safe_key = LessSafeKey::new(unbound_key);

    // 分离密文和标签
    let ciphertext_len = data.len() - 16;
    let mut in_out = data[..ciphertext_len].to_vec();
    let _tag = &data[ciphertext_len..];

    less_safe_key
        .open_in_place(
            Nonce::assume_unique_is_key(iv.try_into().unwrap()),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|e| GmError::DecryptFailed(format!("解密失败: {:?}", e)))?;

    Ok(in_out)
}

/// 生成随机 IV
pub fn generate_iv() -> Vec<u8> {
    let mut iv = vec![0u8; 16];
    getrandom::getrandom(&mut iv).unwrap_or_default();
    iv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sm4_key_from_hex() {
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let key = Sm4Key::from_hex(hex_key).unwrap();
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn test_sm4_key_invalid_length() {
        let result = Sm4Key::from_bytes &[1, 2, 3];
        assert!(result.is_err());
    }

    #[test]
    fn test_sm4_encrypt_decrypt() {
        let key = [0u8; 32];
        let iv = [0u8; 16];
        let plaintext = b"Hello, SM4!";

        let ciphertext = sm4_gcm_encrypt(plaintext, &key, &iv).unwrap();
        let decrypted = sm4_gcm_decrypt(&ciphertext, &key, &iv).unwrap();

        assert_eq!(plaintext.to_vec(), decrypted);
    }
}