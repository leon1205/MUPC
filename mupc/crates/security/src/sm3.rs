//! SM3 国密消息摘要算法实现
//!
//! 实现 GB/T 32907-2016《SM3 密码杂凑算法》
//!
//! # gmsm 0.1.0 能力说明
//! - 支持 SM3 哈希 (sm3_byte/sm3_hex)
//! - 不支持 HKDF-SM3

use crate::errors::{GmError, Result};

/// SM3 消息摘要
///
/// real_gmsm: 使用 gmsm::sm3。
/// fake_gmsm: 使用 SHA-256 作为替代实现。
pub fn sm3_hash(data: &[u8]) -> Result<Vec<u8>> {
    #[cfg(feature = "real_gmsm")]
    {
        let input = String::from_utf8_lossy(data);
        let hash: [u8; 32] = gmsm::sm3::sm3_byte(&input);
        Ok(hash.to_vec())
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(data);
        Ok(hash.to_vec())
    }
}

/// 使用 SM3 进行密钥派生（HKDF-SM3）
///
/// gmsm 0.1.0 不支持 HKDF-SM3，real_gmsm 路径返回 Unsupported。
pub fn sm3_derive_key(
    _input_key: &[u8],
    _salt: &[u8],
    _info: &[u8],
    _output_len: usize,
) -> Result<Vec<u8>> {
    Err(GmError::Unsupported(
        "HKDF-SM3 在 gmsm 0.1.0 中不可用".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sm3_hash() {
        let data = b"test";
        let hash = sm3_hash(data);
        assert!(hash.is_ok());
        assert_eq!(hash.unwrap().len(), 32);
    }
}
