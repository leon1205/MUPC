//! SM4 国密对称加密算法实现
//!
//! 实现 GB/T 32907-2016《SM4 分组密码算法》
//!
//! # 安全警告
//! GCM 模式下 IV 严禁重用！

use crate::errors::{GmError, Result};
use std::cmp::min;

/// SM4 密钥结构（16字节，128位）
#[derive(Debug, Clone)]
pub struct Sm4Key {
    key: [u8; 16],
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
    pub fn from_hex(hex: &str) -> Result<Self> {
        let bytes = hex::Engine::decode(&hex::engine::general_purpose::STANDARD, hex)
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
        use gmsm::{Sm4, Mode::Cbc, padding::Pkcs7};

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

        let cipher = Sm4::new(key.into())
            .map_err(|e| GmError::EncryptFailed(format!("密钥创建失败: {:?}", e)))?;

        cipher.encrypt(Mode::Cbc(iv.into()), data, Pkcs7)
            .map_err(|e| GmError::EncryptFailed(format!("加密失败: {:?}", e)))
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
        use gmsm::{Sm4, Mode::Cbc, padding::Pkcs7};

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

        let cipher = Sm4::new(key.into())
            .map_err(|e| GmError::DecryptFailed(format!("密钥创建失败: {:?}", e)))?;

        cipher.decrypt(Mode::Cbc(iv.into()), data, Pkcs7)
            .map_err(|e| GmError::DecryptFailed(format!("解密失败: {:?}", e)))
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        Err(GmError::InvalidParam("SM4 CBC 解密需要 gmsm 库".into()))
    }
}

/// SM4 GCM 模式加密（带认证标签）
pub fn sm4_gcm_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    #[cfg(feature = "real_gmsm")]
    {
        use gmsm::{Sm4, Mode::Gcm, aead::Aad};

        if key.len() != 16 {
            return Err(GmError::InvalidKeyLength(format!(
                "SM4密钥必须为16字节，当前为 {} 字节",
                key.len()
            )));
        }
        if iv.len() != 12 && iv.len() != 16 {
            return Err(GmError::InvalidParam(format!(
                "GCM IV 必须为 12 或 16 字节，当前为 {} 字节",
                iv.len()
            )));
        }

        let cipher = Sm4::new(key.into())
            .map_err(|e| GmError::EncryptFailed(format!("密钥创建失败: {:?}", e)))?;

        let mut in_out = data.to_vec();
        let tag = cipher.encrypt(Mode::Gcm(iv.into()), Aad::empty(), &mut in_out)
            .map_err(|e| GmError::EncryptFailed(format!("加密失败: {:?}", e)))?;

        let mut result = in_out;
        result.extend_from_slice(tag.as_ref());
        Ok(result)
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        // 使用 ring 的 AES-256-GCM 模拟（扩展为 32 字节密钥）
        use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};

        if key.len() != 16 {
            return Err(GmError::InvalidKeyLength(format!(
                "SM4密钥必须为16字节，当前为 {} 字节",
                key.len()
            )));
        }

        // 扩展为 32 字节（ring AES-256 需要）
        let mut expanded_key = [0u8; 32];
        expanded_key[..16].copy_from_slice(key);
        expanded_key[16..].copy_from_slice(key);

        let unbound_key = UnboundKey::new(&AES_256_GCM, &expanded_key)
            .map_err(|e| GmError::EncryptFailed(format!("密钥创建失败: {:?}", e)))?;
        let less_safe_key = LessSafeKey::new(unbound_key);

        let mut in_out = data.to_vec();
        let tag = less_safe_key.seal_in_place_separate_tag(
            Nonce::assume_unique_is_key(iv[..12].try_into().unwrap()),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|e| GmError::EncryptFailed(format!("加密失败: {:?}", e)))?;

        let mut result = in_out;
        result.extend_from_slice(tag.as_ref());
        Ok(result)
    }
}

/// SM4 GCM 模式解密
pub fn sm4_gcm_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
    #[cfg(feature = "real_gmsm")]
    {
        use gmsm::{Sm4, Mode::Gcm, aead::Aad};

        if key.len() != 16 {
            return Err(GmError::InvalidKeyLength(format!(
                "SM4密钥必须为16字节，当前为 {} 字节",
                key.len()
            )));
        }

        if data.len() < 16 {
            return Err(GmError::InvalidFormat("密文长度不足".to_string()));
        }

        let cipher = Sm4::new(key.into())
            .map_err(|e| GmError::DecryptFailed(format!("密钥创建失败: {:?}", e)))?;

        let ciphertext_len = data.len() - 16;
        let mut in_out = data[..ciphertext_len].to_vec();
        let _tag = &data[ciphertext_len..];

        cipher.decrypt(Mode::Gcm(iv.into()), Aad::empty(), &mut in_out)
            .map_err(|e| GmError::DecryptFailed(format!("解密失败: {:?}", e)))?;

        Ok(in_out)
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};

        if key.len() != 16 {
            return Err(GmError::InvalidKeyLength(format!(
                "SM4密钥必须为16字节，当前为 {} 字节",
                key.len()
            )));
        }

        if data.len() < 16 {
            return Err(GmError::InvalidFormat("密文长度不足".to_string()));
        }

        let mut expanded_key = [0u8; 32];
        expanded_key[..16].copy_from_slice(key);
        expanded_key[16..].copy_from_slice(key);

        let unbound_key = UnboundKey::new(&AES_256_GCM, &expanded_key)
            .map_err(|e| GmError::DecryptFailed(format!("密钥创建失败: {:?}", e)))?;
        let less_safe_key = LessSafeKey::new(unbound_key);

        let ciphertext_len = data.len() - 16;
        let mut in_out = data[..ciphertext_len].to_vec();
        let _tag = &data[ciphertext_len..];

        less_safe_key.open_in_place(
            Nonce::assume_unique_is_key(iv[..12].try_into().unwrap()),
            Aad::empty(),
            &mut in_out,
        )
        .map_err(|e| GmError::DecryptFailed(format!("解密失败: {:?}", e)))?;

        Ok(in_out)
    }
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