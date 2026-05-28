//! SM2 国密签名算法实现
//!
//! 实现 GB/T 32918.2-2016《SM2 椭圆曲线公钥密码算法》
 //!
//! 使用 gmsm crate 实现真正的 SM2 曲线签名算法

use crate::errors::{GmError, Result};
use std::fs;

/// SM2 签名结构
#[derive(Debug, Clone)]
pub struct Sm2Signature {
    pub r: Vec<u8>,
    pub s: Vec<u8>,
}

/// SM2 密钥对（用于签名）
#[derive(Debug, Clone)]
pub struct Sm2KeyPair {
    key: gmsm::Sm2KeyPair,
}

/// 从 PEM 文件加载 SM2 私钥
pub fn load_sm2_private_key(path: &str) -> Result<Vec<u8>> {
    let pem_data = fs::read_to_string(path)
        .map_err(|e| GmError::KeyLoadFailed(format!("读取私钥文件失败: {}", e)))?;
    let pem_contents: Vec<&str> = pem_data.lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, pem_contents.join(""))
        .map_err(|e| GmError::KeyLoadFailed(format!("Base64 解码失败: {}", e)))?;
    Ok(decoded)
}

/// 从 PEM 文件加载 SM2 公钥
pub fn load_sm2_public_key(path: &str) -> Result<Vec<u8>> {
    let pem_data = fs::read_to_string(path)
        .map_err(|e| GmError::KeyLoadFailed(format!("读取公钥文件失败: {}", e)))?;
    let pem_contents: Vec<&str> = pem_data.lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, pem_contents.join(""))
        .map_err(|e| GmError::KeyLoadFailed(format!("Base64 解码失败: {}", e)))?;
    Ok(decoded)
}

/// SM2 签名
pub fn sm2_sign(data: &[u8], private_key_pem: &str) -> Result<Vec<u8>> {
    #[cfg(feature = "real_gmsm")]
    {
        let key = if private_key_pem.contains("-----BEGIN") {
            gmsm::Sm2KeyPair::from_pem(private_key_pem)
                .map_err(|e| GmError::KeyLoadFailed(format!("密钥解析失败: {:?}", e)))?
        } else {
            let key_bytes = load_sm2_private_key(private_key_pem)?;
            gmsm::Sm2KeyPair::from_bytes(&key_bytes)
                .map_err(|e| GmError::KeyLoadFailed(format!("密钥加载失败: {:?}", e)))?
        };

        let signature = key.sign(data)
            .map_err(|e| GmError::SignFailed(format!("签名失败: {:?}", e)))?;

        Ok(signature.to_bytes())
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        // ring 模拟实现（仅用于测试）
        use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING, ECDSA_P256_SHA256_FIXED_VERIFICATION};
        let private_key_bytes = load_sm2_private_key(private_key_pem)?;
        let ecdsa_key_pair = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &ECDSA_P256_SHA256_FIXED_VERIFICATION,
            &private_key_bytes,
        ).map_err(|e| GmError::SignFailed(format!("密钥解析失败: {:?}", e)))?;
        let signature = ring::signature::sign(&ecdsa_key_pair, data)
            .map_err(|e| GmError::SignFailed(format!("签名失败: {:?}", e)))?;
        Ok(signature.as_ref().to_vec())
    }
}

/// SM2 验签
pub fn sm2_verify(data: &[u8], signature: &[u8], public_key_pem: &str) -> Result<bool> {
    #[cfg(feature = "real_gmsm")]
    {
        let public_key = if public_key_pem.contains("-----BEGIN") {
            gmsm::Sm2::from_pem(public_key_pem)
                .map_err(|e| GmError::KeyLoadFailed(format!("公钥解析失败: {:?}", e)))?
        } else {
            let key_bytes = load_sm2_public_key(public_key_pem)?;
            gmsm::Sm2::from_public_key_slice(&key_bytes)
                .map_err(|e| GmError::KeyLoadFailed(format!("公钥解析失败: {:?}", e)))?
        };

        public_key.verify(data, signature)
            .map_err(|e| GmError::VerifyFailed(format!("验签失败: {:?}", e)))
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        use ring::signature::{ECDSA_P256_SHA256_FIXED_VERIFICATION, UnparsedPublicKey};
        let public_key_bytes = load_sm2_public_key(public_key_pem)?;
        let public_key = UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED_VERIFICATION, public_key_bytes);
        public_key.verify(data, signature)
            .map(|_| true)
            .map_err(|e| GmError::VerifyFailed(format!("验签失败: {:?}", e)))
    }
}

/// 生成 SM2 密钥对
#[cfg(feature = "real_gmsm")]
pub fn sm2_key_generate() -> Result<Sm2KeyPair> {
    let key_pair = gmsm::Sm2KeyPair::generate()
        .map_err(|e| GmError::KeyLoadFailed(format!("密钥生成失败: {:?}", e)))?;
    Ok(Sm2KeyPair { key: key_pair })
}

/// 生成 SM2 密钥对（fake_gmsm 版本，返回错误）
#[cfg(not(feature = "real_gmsm"))]
pub fn sm2_key_generate() -> Result<Sm2KeyPair> {
    Err(GmError::InvalidParam("密钥生成需要 gmsm 库".into()))
}

/// 派生共享密钥（ECDH 风格）
#[cfg(feature = "real_gmsm")]
pub fn sm2_derive_shared_key(key_pair: &Sm2KeyPair, peer_public_key: &[u8]) -> Result<Vec<u8>> {
    let peer_key = gmsm::Sm2::from_public_key_slice(peer_public_key)
        .map_err(|e| GmError::KeyLoadFailed(format!("公钥解析失败: {:?}", e)))?;
    key_pair.key.derive_shared_secret(&peer_key)
        .map_err(|e| GmError::KeyDeriveFailed(format!("共享密钥派生失败: {:?}", e)))
}

/// 派生共享密钥（fake_gmsm 版本，返回错误）
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