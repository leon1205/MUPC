//! SM4 单元测试

use mupc_security::{sm4_gcm_decrypt, sm4_gcm_encrypt, Sm4Key};

#[test]
fn test_sm4_key_creation() {
    // SM4 密钥为 16 字节（128 位），hex 字符串为 32 字符
    let hex_key = "0123456789abcdef0123456789abcdef";
    let key = Sm4Key::from_hex(hex_key);
    assert!(key.is_ok());
}

#[test]
fn test_sm4_encrypt_decrypt() {
    let key = [0u8; 16]; // SM4 密钥为 16 字节
    let iv = [0u8; 12];  // GCM IV 为 12 字节
    let plaintext = b"Test data for SM4 encryption";

    let encrypted = sm4_gcm_encrypt(plaintext, &key, &iv);
    assert!(encrypted.is_ok());

    let decrypted = sm4_gcm_decrypt(&encrypted.unwrap(), &key, &iv);
    assert!(decrypted.is_ok());
    assert_eq!(decrypted.unwrap(), plaintext.to_vec());
}

#[test]
fn test_sm4_invalid_key_length() {
    // 15 字节是无效的（SM4 需要 16 字节）
    let short_key = [0u8; 15];
    let iv = [0u8; 12];
    let plaintext = b"Test";

    let result = sm4_gcm_encrypt(plaintext, &short_key, &iv);
    assert!(result.is_err());
}

#[test]
fn test_sm4_invalid_iv_length() {
    let key = [0u8; 16]; // SM4 密钥为 16 字节
    let short_iv = [0u8; 8]; // GCM IV 需要 12 或 16 字节，8 字节无效
    let plaintext = b"Test";

    let result = sm4_gcm_encrypt(plaintext, &key, &short_iv);
    assert!(result.is_err());
}