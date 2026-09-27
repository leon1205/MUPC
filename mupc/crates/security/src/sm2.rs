//! SM2 国密非对称加密算法实现
//!
//! 实现 GB/T 32918.2-2016《SM2 椭圆曲线公钥密码算法》
//!
//! # gmsm 0.1.0 能力说明
//! - 支持 SM2 加密/解密
//! - 不支持签名/验签（gmsm 0.1.0 未提供签名 API）
//! - 签名/验签**显式不支持**：`sm2_sign` / `sm2_verify` 一律返回 [`GmError::Unsupported`]
//!
//! SM2：加密/密钥/签名框架 —— framework-only（2026-09-09）。签名**不再**走 ring ECDSA 兜底：
//! 那会与同曲线的验签构成"自签自验恒通过"的自洽对，构成框架态假性合规（D-7）。

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
///
/// `#[allow(dead_code)]`：签名/验签已改为显式 `Unsupported`（D-7）⇒ 本函数当前**无调用者**。
/// 保留它是为真国密路径（gmsm 0.14+）接线时使用，**不是**给 ring ECDSA 兜底当输入。
#[allow(dead_code)]
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
///
/// `#[allow(dead_code)]`：同 [`load_sm2_private_key`]（D-7 之后无调用者，供真国密路径接线用）。
#[allow(dead_code)]
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
/// # 为什么**一律**返回 [`GmError::Unsupported`]（而不是 ring 兜底）
/// 本函数此前用 ring 的 ECDSA P-256 实现，而 `sm2_verify` 用**同一条曲线**验 —— 二者构成
/// **自洽对**：自己签自己验**恒通过**，接口层完全区分不出"真 SM2"与"ECDSA"。一旦被接线，
/// 立即可被当成"国密签名已可用"的**假性合规**凭据。故非真国密路径一律显式拒绝。
///
/// 真国密 SM2 签名需要 gmsm 0.14+（当前锁定的 gmsm 0.1.0 不提供签名 API）⇒ 本仓库
/// 属"框架态"，**不得**用国际算法顶替。
pub fn sm2_sign(data: &[u8], private_key_pem: &str) -> Result<Vec<u8>> {
    // 参数保持签名兼容（调用方无需改动）；本路径不读文件、不产生任何签名。
    let _ = (data, private_key_pem);

    #[cfg(feature = "real_gmsm")]
    let msg = "SM2 签名需 gmsm 0.14+ 的真国密实现；gmsm 0.1.0 无签名 API。\
               不得用 ring ECDSA P-256 冒充（与 sm2_verify 同曲线 ⇒ 自签自验恒通过 = 假性合规）";
    #[cfg(not(feature = "real_gmsm"))]
    let msg = "SM2 签名在 fake_gmsm（ring 兜底）路径下不可用：ring ECDSA 不是 SM2，\
               且自签自验恒通过 ⇒ 假性合规。真国密签名需 gmsm 0.14+";

    Err(GmError::Unsupported(msg.to_string()))
}

/// SM2 验签
///
/// 口径同 [`sm2_sign`]：ring 兜底路径**绝不**给出"验证通过"，一律 [`GmError::Unsupported`]
/// （`Ok(false)` 也是一种主张——"签名不对"——而本仓库根本没有能力做出这个判断）。
pub fn sm2_verify(data: &[u8], signature: &[u8], public_key_pem: &str) -> Result<bool> {
    let _ = (data, signature, public_key_pem);

    #[cfg(feature = "real_gmsm")]
    let msg = "SM2 验签需 gmsm 0.14+ 的真国密实现；gmsm 0.1.0 无验签 API。\
               不得用 ring ECDSA P-256 冒充（与 sm2_sign 同曲线 ⇒ 框架态假性通过）";
    #[cfg(not(feature = "real_gmsm"))]
    let msg = "SM2 验签在 fake_gmsm（ring 兜底）路径下不可用：ring ECDSA 不是 SM2。\
               真国密验签需 gmsm 0.14+";

    Err(GmError::Unsupported(msg.to_string()))
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
