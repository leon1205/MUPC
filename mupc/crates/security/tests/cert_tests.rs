//! 证书管理单元测试

use mupc_security::cert::{CertStore, GmCert};

#[test]
fn test_cert_store_empty() {
    let store = CertStore::new();
    assert_eq!(store.ca_cert_count(), 0);
    assert!(store.get_client_cert().is_none());
}

#[test]
fn test_cert_verify_chain_empty() {
    let store = CertStore::new();
    let dummy_cert = GmCert {
        subject: "CN=test".to_string(),
        issuer: "CN=test".to_string(),
        serial: "1".to_string(),
        not_before: "2024-01-01".to_string(),
        not_after: "2025-01-01".to_string(),
        raw: vec![],
    };

    let result = store.verify_cert_chain(&dummy_cert);
    assert!(result.is_ok());
}