//! SM2 国密非对称加密算法实现
//!
//! 实现 GB/T 32918.2-2016《SM2 椭圆曲线公钥密码算法》
//!
//! # gmsm 0.1.0 能力说明
//! - 支持 SM2 加密/解密
//! - 不支持签名/验签（gmsm 0.1.0 未提供签名 API）
//! - 签名功能使用 fake_gmsm (ring) 路径

use crate::errors::{GmError, Result};
use base64::Engine;
use std::fs;

/// SM2 签名结构
#[derive(Debug, Clone)]
pub struct Sm2Signature {
    pub r: Vec<u8>,
    pub s: Vec<u8>,
}

/// SM2 密钥对
#[derive(Clone)]
pub struct Sm2KeyPair {
    #[cfg(feature = "real_gmsm")]
    key: gmsm::sm2::Keypair,
}

impl std::fmt::Debug for Sm2KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(feature = "real_gmsm")]
        let _ = &self.key;
        f.debug_struct("Sm2KeyPair").finish()
    }
}

/// 从 PEM 文件加载 SM2 私钥
pub fn load_sm2_private_key(path: &str) -> Result<Vec<u8>> {
    let pem_data = fs::read_to_string(path)
        .map_err(|e| GmError::KeyLoadFailed(format!("读取私钥文件失败: {}", e)))?;
    let pem_contents: Vec<&str> = pem_data
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(pem_contents.join(""))
        .map_err(|e| GmError::KeyLoadFailed(format!("Base64 解码失败: {}", e)))?;
    Ok(decoded)
}

/// 从 PEM 文件加载 SM2 公钥
pub fn load_sm2_public_key(path: &str) -> Result<Vec<u8>> {
    let pem_data = fs::read_to_string(path)
        .map_err(|e| GmError::KeyLoadFailed(format!("读取公钥文件失败: {}", e)))?;
    let pem_contents: Vec<&str> = pem_data
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(pem_contents.join(""))
        .map_err(|e| GmError::KeyLoadFailed(format!("Base64 解码失败: {}", e)))?;
    Ok(decoded)
}

/// SM2 签名
///
/// gmsm 0.1.0 不支持签名，统一使用 ring ECDSA P-256 模拟。
pub fn sm2_sign(data: &[u8], private_key_pem: &str) -> Result<Vec<u8>> {
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
    let rng = SystemRandom::new();
    let private_key_bytes = load_sm2_private_key(private_key_pem)?;
    let ecdsa_key_pair =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &private_key_bytes, &rng)
            .map_err(|e| GmError::SignFailed(format!("密钥解析失败: {:?}", e)))?;
    let signature = ecdsa_key_pair
        .sign(&rng, data)
        .map_err(|e| GmError::SignFailed(format!("签名失败: {:?}", e)))?;
    Ok(signature.as_ref().to_vec())
}

/// SM2 验签
pub fn sm2_verify(data: &[u8], signature: &[u8], public_key_pem: &str) -> Result<bool> {
    use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
    let public_key_bytes = load_sm2_public_key(public_key_pem)?;
    let public_key = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, public_key_bytes);
    public_key
        .verify(data, signature)
        .map(|_| true)
        .map_err(|e| GmError::VerifyFailed(format!("验签失败: {:?}", e)))
}

/// 生成 SM2 密钥对
#[cfg(feature = "real_gmsm")]
pub fn sm2_key_generate() -> Result<Sm2KeyPair> {
    let keypair = gmsm::sm2::sm2_generate_key_hex();
    Ok(Sm2KeyPair { key: keypair })
}

/// 生成 SM2 密钥对（fake_gmsm 版本）
#[cfg(not(feature = "real_gmsm"))]
pub fn sm2_key_generate() -> Result<Sm2KeyPair> {
    Err(GmError::InvalidParam("密钥生成需要 gmsm 库".into()))
}

/// 派生共享密钥（ECDH 风格）
#[cfg(feature = "real_gmsm")]
pub fn sm2_derive_shared_key(_key_pair: &Sm2KeyPair, _peer_public_key: &[u8]) -> Result<Vec<u8>> {
    Err(GmError::Unsupported(
        "共享密钥派生在 gmsm 0.1.0 中不可用".into(),
    ))
}

/// 派生共享密钥（fake_gmsm 版本）
#[cfg(not(feature = "real_gmsm"))]
pub fn sm2_derive_shared_key(_key_pair: &Sm2KeyPair, _peer_public_key: &[u8]) -> Result<Vec<u8>> {
    Err(GmError::InvalidParam("共享密钥派生需要 gmsm 库".into()))
}

/// 将签名结果转换为 R 和 S 分量
pub fn signature_to_rs(signature: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    if signature.len() != 64 {
        return Err(GmError::InvalidFormat("签名长度应为64字节".to_string()));
    }
    Ok((signature[..32].to_vec(), signature[32..].to_vec()))
}

/// 从 R 和 S 分量构建签名
pub fn rs_to_signature(r: &[u8], s: &[u8]) -> Result<Vec<u8>> {
    if r.len() != 32 || s.len() != 32 {
        return Err(GmError::InvalidFormat("R和S分量长度应为32字节".to_string()));
    }
    let mut sig = Vec::with_capacity(64);
    sig.extend_from_slice(r);
    sig.extend_from_slice(s);
    Ok(sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_conversion() {
        let r = vec![1u8; 32];
        let s = vec![2u8; 32];
        let sig = rs_to_signature(&r, &s).unwrap();
        assert_eq!(sig.len(), 64);
        let (r2, s2) = signature_to_rs(&sig).unwrap();
        assert_eq!(r, r2);
        assert_eq!(s, s2);
    }
}
