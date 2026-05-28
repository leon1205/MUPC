//! SM3 国密消息摘要算法实现
//!
//! 实现 GB/T 32907-2016《SM3 密码杂凑算法》

use crate::errors::{GmError, Result};

/// SM3 消息摘要
pub fn sm3_hash(data: &[u8]) -> Result<Vec<u8>> {
    #[cfg(feature = "real_gmsm")]
    {
        use gmsm::Sm3;
        let mut hasher = Sm3::new();
        hasher.update(data);
        Ok(hasher.finalize())
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        Err(GmError::InvalidParam("SM3 需要 gmsm 库".into()))
    }
}

/// 使用 SM3 进行密钥派生（HKDF-SM3）
pub fn sm3_derive_key(input_key: &[u8], salt: &[u8], info: &[u8], output_len: usize) -> Result<Vec<u8>> {
    #[cfg(feature = "real_gmsm")]
    {
        use gmsm::HkdfSm3;
        let hk = HkdfSm3::new(Some(salt), input_key);
        let mut okm = vec![0u8; output_len];
        hk.expand(info, &mut okm)
            .map_err(|e| GmError::InvalidParam(format!("HKDF 扩展失败: {:?}", e)))?;
        Ok(okm)
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        Err(GmError::InvalidParam("HKDF-SM3 需要 gmsm 库".into()))
    }
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