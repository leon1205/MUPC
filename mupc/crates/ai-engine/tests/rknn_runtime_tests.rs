//! RKNN Runtime Integration Tests

use mupc_ai_engine::RknnRuntime;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rknn_runtime_creation() {
        let runtime = RknnRuntime::new(Path::new("/tmp/test.rknn"), None);
        assert!(runtime.is_ok());
    }

    #[tokio::test]
    async fn test_rknn_runtime_is_loaded() {
        let runtime = RknnRuntime::new(Path::new("/tmp/test.rknn"), None).unwrap();
        // 未加载时应返回 false
        assert!(!runtime.is_loaded());
    }
}