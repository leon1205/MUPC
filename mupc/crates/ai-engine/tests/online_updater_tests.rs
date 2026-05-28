//! OnlineUpdater Integration Tests

use mupc_ai_engine::config::OnlineUpdateConfig;
use mupc_ai_engine::online_updater::{OnlineUpdater, DataPoint};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> OnlineUpdateConfig {
        OnlineUpdateConfig {
            enabled: false,
            batch_size: 32,
            learning_rate: 0.001,
        }
    }

    #[test]
    fn test_online_updater_enabled() {
        let config = create_test_config();
        let updater = OnlineUpdater::new(config);
        assert!(!updater.is_enabled());
    }

    #[test]
    fn test_online_updater_clear_buffer() {
        let mut config = create_test_config();
        config.enabled = true;

        let mut updater = OnlineUpdater::new(config);

        let data = DataPoint {
            timestamp: 1000,
            input: vec![1.0, 2.0],
            output: vec![0.5],
        };
        updater.add_sample(data);
        assert_eq!(updater.buffer_size(), 1);

        updater.clear_buffer();
        assert_eq!(updater.buffer_size(), 0);
    }
}