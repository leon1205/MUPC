//! SM2 单元测试
//!
//! # 警告
//! 本仓库**没有**真国密 SM2 签名/验签实现（gmsm 0.1.0 无签名 API）⇒ `sm2_sign` /
//! `sm2_verify` 一律返回 `Unsupported`。此处把它们**显式**钉成"不可用"，防止后来者
//! 用 ring ECDSA 兜底把"自签自验恒通过"重新引回来（D-7：框架态假性合规）。

use mupc_security::{signature_to_rs, sm2_sign, sm2_verify, GmError, Sm2Signature};

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

/// 判别力：`sm2_sign` 必须是 **`Unsupported`**，而**不是**"读不到密钥文件"之类的偶然失败。
///
/// 旧实现走 ring ECDSA：对一个不存在的 PEM 会返回 `KeyLoadFailed`（也是 `Err`，但语义完全不同）。
/// 本用例按**变体**断言 ⇒ 旧实现红（它给的是 `KeyLoadFailed`，且若真给了密钥文件还会**真的签出
/// 一个 ECDSA 签名**）。
#[test]
fn sm2_sign_is_unsupported_not_a_ring_ecdsa_stand_in() {
    match sm2_sign(b"payload", "/nonexistent/private_key.pem") {
        Err(GmError::Unsupported(msg)) => {
            assert!(
                msg.contains("gmsm") || msg.contains("SM2 签名"),
                "拒绝理由须可定位: {msg}"
            );
        }
        other => panic!("必须显式 Unsupported（不得返回签名/其它错误），实际: {other:?}"),
    }
}

/// 判别力：`sm2_verify` 同样必须 `Unsupported`；**不得**返回 `Ok(false)`（那是"签名不对"的主张）
/// 也**不得**返回 `Ok(true)`（框架态假性通过）。
#[test]
fn sm2_verify_is_unsupported_not_a_ring_ecdsa_stand_in() {
    match sm2_verify(b"payload", &[0u8; 64], "/nonexistent/public_key.pem") {
        Err(GmError::Unsupported(msg)) => {
            assert!(
                msg.contains("gmsm") || msg.contains("SM2 验签"),
                "拒绝理由须可定位: {msg}"
            );
        }
        other => panic!("必须显式 Unsupported，实际: {other:?}"),
    }
}

/// 结构性反证：本仓库**不可能**出现"自己签自己验通过"这一自洽对（旧实现的恒通过路径）。
#[test]
fn a_self_consistent_sign_then_verify_pair_is_structurally_impossible() {
    let signed = sm2_sign(b"payload", "/nonexistent/private_key.pem");
    assert!(
        signed.is_err(),
        "签名侧不可用 ⇒ 不存在可喂给验签侧的签名"
    );
    assert!(
        sm2_verify(b"payload", &[0u8; 64], "/nonexistent/public_key.pem").is_err(),
        "验签侧不可用 ⇒ 不存在'框架态通过'"
    );
}
