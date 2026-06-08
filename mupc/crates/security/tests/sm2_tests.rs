//! SM2 单元测试
//!
//! # 警告
//! SM2 实现使用 P-256 曲线模拟，不是真正的国密 SM2

use mupc_security::{signature_to_rs, sm2_sign, sm2_verify, Sm2Signature};

#[test]
fn test_sm2_signature_conversion() {
    // 测试签名转换功能
    let signature = vec![0u8; 64];
    let result = signature_to_rs(&signature);
    assert!(result.is_ok());

    let (r, s) = result.unwrap();
    assert_eq!(r.len(), 32);
    assert_eq!(s.len(), 32);
}

#[test]
fn test_sm2_signature_struct() {
    // 测试 Sm2Signature 结构
    let sig = Sm2Signature {
        r: vec![1u8; 32],
        s: vec![2u8; 32],
    };
    assert_eq!(sig.r.len(), 32);
    assert_eq!(sig.s.len(), 32);
}
