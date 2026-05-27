//! SM4 单元测试

use mupc_security::{sm4_gcm_decrypt, sm4_gcm_encrypt, Sm4Key};

#[test]
fn test_sm4_key_creation() {
    let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let key = Sm4Key::from_hex(hex_key);
    assert!(key.is_ok());
}

#[test]
fn test_sm4_encrypt_decrypt() {
    let key = [0u8; 32];
    let iv = [0u8; 16];
    let plaintext = b"Test data for SM4 encryption";

    let encrypted = sm4_gcm_encrypt(plaintext, &key, &iv);
    assert!(encrypted.is_ok());

    let decrypted = sm4_gcm_decrypt(&encrypted.unwrap(), &key, &iv);
    assert!(decrypted.is_ok());
    assert_eq!(decrypted.unwrap(), plaintext.to_vec());
}

#[test]
fn test_sm4_invalid_key_length() {
    let short_key = [0u8; 16];
    let iv = [0u8; 16];
    let plaintext = b"Test";

    let result = sm4_gcm_encrypt(plaintext, &short_key, &iv);
    assert!(result.is_err());
}

#[test]
fn test_sm4_invalid_iv_length() {
    let key = [0u8; 32];
    let short_iv = [0u8; 8];
    let plaintext = b"Test";

    let result = sm4_gcm_encrypt(plaintext, &key, &short_iv);
    assert!(result.is_err());
}