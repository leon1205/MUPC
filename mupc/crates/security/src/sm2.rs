//! SM2 国密签名算法实现
//!
//! SM2是国家密码管理局发布的椭圆曲线公钥密码算法
//!
//! # 警告：模拟实现
//!
//! 当前实现使用 ring 库的 ECDSA P-256 曲线，而非真正的 SM2 曲线。
//! 这仅用于演示和测试目的。
//!
//! 在实际部署中，应使用GmSSL库或支持SM2曲线的库（如 gmsm crate）。
//!
//! 生产环境需要替换为真正的国密算法实现。

use crate::errors::{GmError, Result};
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING, ECDSA_P256_SHA256_FIXED_VERIFICATION};
use std::fs;

/// SM2 签名结构
#[derive(Debug, Clone)]
pub struct Sm2Signature {
    pub r: Vec<u8>,
    pub s: Vec<u8>,
}

/// 从 PEM 文件加载 SM2 私钥
pub fn load_sm2_private_key(path: &str) -> Result<Vec<u8>> {
    let pem_data = fs::read_to_string(path)
        .map_err(|e| GmError::KeyLoadFailed(format!("读取私钥文件失败: {}", e)))?;

    // 解析 PEM 格式（支持 SM2/ECDSA P-256）
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
///
/// 使用 SM2/ECDSA P-256 曲线进行签名
pub fn sm2_sign(data: &[u8], private_key_pem: &str) -> Result<Vec<u8>> {
    // 由于 ring 库不支持 SM2 曲线，我们使用 P-256 作为模拟实现
    // 在实际部署中应使用GmSSL库或支持SM2曲线的库

    let private_key_bytes = load_sm2_private_key(private_key_pem)?;

    // 使用 ECDSA P-256 进行签名（仅用于演示）
    let ecdsa_key_pair = EcdsaKeyPair::from_pkcs8(
        &ECDSA_P256_SHA256_FIXED_SIGNING,
        &ECDSA_P256_SHA256_FIXED_VERIFICATION,
        &private_key_bytes,
    ).map_err(|e| GmError::SignFailed(format!("密钥解析失败: {:?}", e)))?;

    let signature = ring::signature::sign(&ecdsa_key_pair, data)
        .map_err(|e| GmError::SignFailed(format!("签名失败: {:?}", e)))?;

    Ok(signature.as_ref().to_vec())
}

/// SM2 验签
///
/// 验证 SM2 签名
pub fn sm2_verify(data: &[u8], signature: &[u8], public_key_pem: &str) -> Result<bool> {
    let public_key_bytes = load_sm2_public_key(public_key_pem)?;

    let public_key = ring::signature::UnparsedPublicKey::new(
        &ECDSA_P256_SHA256_FIXED_VERIFICATION,
        public_key_bytes,
    );

    public_key
        .verify(data, signature)
        .map(|_| true)
        .map_err(|e| GmError::VerifyFailed(format!("验签失败: {:?}", e)))
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