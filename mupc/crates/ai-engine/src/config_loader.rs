//! 动作空间配置加载器
//!
//! 负责从数据库加载 `ActionSpaceConfig`，若无记录则使用默认配置。
//! 提供内存缓存以避免频繁查询数据库。

use crate::action_space::ActionSpaceConfig;
use crate::error::AiEngineError;
use mupc_storage::StorageService;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 动作空间配置加载器
///
/// 数据库优先加载，无则用默认配置；提供内存缓存。
pub struct ConfigLoader {
    /// 内存缓存（transformer_id -> ActionSpaceConfig）
    configs: Arc<RwLock<HashMap<String, ActionSpaceConfig>>>,
    /// 存储服务引用
    storage: Arc<StorageService>,
}

impl ConfigLoader {
    /// 创建配置加载器
    pub fn new(storage: Arc<StorageService>) -> Self {
        Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            storage,
        }
    }

    /// 从数据库加载配置，无则用默认配置
    ///
    /// 流程：
    /// 1. 查询缓存
    /// 2. 缓存命中则返回
    /// 3. 缓存未命中则查数据库
    /// 4. 数据库有记录则写入缓存并返回
    /// 5. 数据库无记录则用默认配置，写入缓存并返回
    pub async fn load(&self, transformer_id: &str) -> Result<ActionSpaceConfig, AiEngineError> {
        // 1. 检查缓存
        if let Some(cached) = self.get_config(transformer_id).await {
            return Ok(cached);
        }

        // 2. 查询数据库
        let config = self.load_from_db(transformer_id).await?.unwrap_or_else(|| {
            let mut cfg = ActionSpaceConfig::default_config();
            cfg.transformer_id = transformer_id.to_string();
            cfg
        });

        // 3. 写入缓存
        {
            let mut cache = self.configs.write().await;
            cache.insert(transformer_id.to_string(), config.clone());
        }

        Ok(config)
    }

    /// 获取配置（带缓存，仅查内存）
    pub async fn get_config(&self, transformer_id: &str) -> Option<ActionSpaceConfig> {
        let cache = self.configs.read().await;
        cache.get(transformer_id).cloned()
    }

    /// 更新配置（仅内存，持久化需调用 save）
    ///
    /// 注意：此方法仅更新内存缓存，不写入数据库。
    /// 若需持久化，请调用 `save()`。
    pub async fn update_config(&self, config: ActionSpaceConfig) -> Result<(), AiEngineError> {
        let mut cache = self.configs.write().await;
        cache.insert(config.transformer_id.clone(), config);
        Ok(())
    }

    /// 保存配置到数据库
    ///
    /// 包含 upsert 语义：若已存在则更新，若不存在则插入。
    pub async fn save(&self, config: &ActionSpaceConfig) -> Result<(), AiEngineError> {
        self.storage
            .update_action_space_config(
                &config.transformer_id,
                config.max_batt_charge_power,
                config.max_batt_discharge_power,
                config.max_apparent_power_kva,
                config.p_batt_ramp_limit_kw,
                config.q_batt_ramp_limit_kvar,
            )
            .await
            .map_err(|e| AiEngineError::ActionValidationFailed(format!("数据库操作失败: {}", e)))?;

        // 更新缓存
        {
            let mut cache = self.configs.write().await;
            cache.insert(config.transformer_id.clone(), config.clone());
        }

        Ok(())
    }

    /// 从数据库加载配置记录
    async fn load_from_db(
        &self,
        transformer_id: &str,
    ) -> Result<Option<ActionSpaceConfig>, AiEngineError> {
        let row: Option<DbActionSpaceConfig> = sqlx::query_as(
            "SELECT transformer_id, max_batt_charge_power, max_batt_discharge_power,
                    max_apparent_power_kva, p_batt_ramp_limit_kw,
                    q_batt_ramp_limit_kvar,
                    transformer_kva, battery_capacity_kwh, soc_min, soc_max, overload_threshold
             FROM action_space_config WHERE transformer_id = ?",
        )
        .bind(transformer_id)
        .fetch_optional(self.storage.pool().as_ref())
        .await
        .map_err(|e| AiEngineError::ActionValidationFailed(format!("数据库查询失败: {}", e)))?;

        Ok(row.map(|r| {
            let defaults = ActionSpaceConfig::default_config();
            ActionSpaceConfig {
                transformer_id: r.transformer_id,
                max_batt_charge_power: r.max_batt_charge_power,
                max_batt_discharge_power: r.max_batt_discharge_power,
                max_load_shedding: defaults.max_load_shedding,
                max_apparent_power_kva: r.max_apparent_power_kva,
                p_batt_ramp_limit_kw: r.p_batt_ramp_limit_kw,
                q_batt_ramp_limit_kvar: r.q_batt_ramp_limit_kvar,
                pv_limit_min: defaults.pv_limit_min,
                transformer_kva: r.transformer_kva,
                battery_capacity_kwh: r.battery_capacity_kwh,
                soc_min: r.soc_min,
                soc_max: r.soc_max,
                overload_threshold: r.overload_threshold,
                k_droop_min: Some(-100.0),
                k_droop_max: Some(100.0),
            }
        }))
    }

    /// 清除所有缓存（用于测试或配置重置）
    pub async fn clear_cache(&self) {
        let mut cache = self.configs.write().await;
        cache.clear();
    }
}

/// 数据库行映射结构体
#[derive(Debug, sqlx::FromRow)]
struct DbActionSpaceConfig {
    transformer_id: String,
    max_batt_charge_power: f64,
    max_batt_discharge_power: f64,
    max_apparent_power_kva: f64,
    p_batt_ramp_limit_kw: f64,
    q_batt_ramp_limit_kvar: f64,
    // v2.6 新增字段
    transformer_kva: f64,
    battery_capacity_kwh: f64,
    soc_min: f64,
    soc_max: f64,
    overload_threshold: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_valid_values() {
        let cfg = ActionSpaceConfig::default_config();
        assert!(cfg.asc_01());
        assert!(cfg.asc_02());
        assert!(cfg.asc_03());
        assert!(cfg.asc_04());
        assert!(cfg.asc_05());
        assert!(cfg.validate().is_ok());
    }
}
