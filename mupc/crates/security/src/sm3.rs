//! SM3 国密消息摘要算法实现
//!
//! 实现 GB/T 32907-2016《SM3 密码杂凑算法》
//!
//! 使用 RustCrypto `sm3` crate（真国密 SM3，支持二进制数据）。
//! 不再依赖 gmsm 0.1.0 的 `sm3_byte`（其仅接受 &str，无法哈希二进制）。

use crate::errors::{GmError, Result};

/// SM3 消息摘要（32 字节）
pub fn sm3_hash(data: &[u8]) -> Result<Vec<u8>> {
    use sm3::{Digest, Sm3};
    let mut hasher = Sm3::new();
    hasher.update(data);
    Ok(hasher.finalize().to_vec())
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
