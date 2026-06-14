//! OnlineUpdater Integration Tests (v2.3: scene-aware)

use mupc_ai_engine::config::{GradualSwitchConfig, OnlineUpdateConfig};
use mupc_ai_engine::online_updater::{DataPoint, OnlineUpdater};

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> OnlineUpdateConfig {
        OnlineUpdateConfig {
            enabled: false,
            batch_size: 32,
            learning_rate: 0.001,
            gradual_switch: GradualSwitchConfig::default(),
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

        let data = DataPoint::new(1000, vec![1.0, 2.0], vec![0.5]);
        updater.add_sample(data);
        assert_eq!(updater.buffer_size(), 1);

        updater.clear_buffer();
        assert_eq!(updater.buffer_size(), 0);
    }

    #[test]
    fn test_scene_isolation() {
        let config = create_test_config();
        let mut updater = OnlineUpdater::new(config);

        use mupc_ai_engine::mode_selector::RunningMode;

        updater.add_sample_for_scene(
            RunningMode::SeasonalLoadManagement,
            DataPoint::new(1, vec![1.0], vec![0.1]),
        );
        updater.add_sample_for_scene(
            RunningMode::CommercialArbitrage,
            DataPoint::new(2, vec![2.0], vec![0.2]),
        );

        assert_eq!(
            updater.scene_sample_count(RunningMode::SeasonalLoadManagement),
            1
        );
        assert_eq!(
            updater.scene_sample_count(RunningMode::CommercialArbitrage),
            1
        );
    }
}
