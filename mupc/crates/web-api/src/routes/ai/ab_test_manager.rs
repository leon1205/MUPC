//! A/B 测试管理器
//!
//! 内存 CRUD 管理 AI 模型 A/B 测试配置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

/// A/B 测试记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbTest {
    pub id: String,
    pub model_type: String,
    pub control_version: String,
    pub experiment_version: String,
    pub traffic_percent: u8,
    pub started_at: String,
    pub estimated_end_at: String,
    pub status: String,
}

/// A/B 测试管理器
pub struct AbTestManager {
    tests: RwLock<HashMap<String, AbTest>>,
}

impl AbTestManager {
    pub fn new() -> Self {
        Self {
            tests: RwLock::new(HashMap::new()),
        }
    }

    pub async fn list_active(&self) -> Vec<AbTest> {
        self.tests
            .read()
            .await
            .values()
            .filter(|t| t.status == "running")
            .cloned()
            .collect()
    }

    pub async fn create(&self, test: AbTest) {
        self.tests.write().await.insert(test.id.clone(), test);
    }

    pub async fn stop(&self, id: &str) -> Option<AbTest> {
        let mut tests = self.tests.write().await;
        if let Some(test) = tests.get_mut(id) {
            test.status = "stopped".to_string();
            Some(test.clone())
        } else {
            None
        }
    }

    pub async fn total_count(&self) -> usize {
        self.tests.read().await.len()
    }
}

impl Default for AbTestManager {
    fn default() -> Self {
        Self::new()
    }
}
