//! 证书管理单元测试

use mupc_security::cert::CertStore;

#[test]
fn test_cert_store_empty() {
    let store = CertStore::new();
    assert_eq!(store.ca_cert_count(), 0);
    assert!(store.get_client_cert().is_none());
}