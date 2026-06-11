//!动态配置加载器
//!
//! 分层加载策略：
//! 1. YAML 加载 → 基准配置（RL 核心参数锁定）
//! 2. DB 查询 → 操作参数覆盖（6 个开放参数）
//! 3. 版本指纹校验 → 启动时校验对齐
//!
//! v2.6: 对齐训练管线配置系统

use crate::action_space::ActionSpaceConfig;
use crate::env_config::{EnvConfig, EnvConfigMetadata};
use crate::error::AiEngineError;
use mupc_storage::StorageService;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 动态配置加载器
pub struct DynamicConfigLoader {
    config_path: PathBuf,
    storage: Arc<StorageService>,
    /// 内存缓存（transformer_id -> ActionSpaceConfig）
    configs: Arc<RwLock<HashMap<String, ActionSpaceConfig>>>,
    /// YAML 元数据（指纹等）
    metadata: Arc<RwLock<Option<EnvConfigMetadata>>>,
    /// YAML 加载的基准配置（用于合并）
    base_config: Arc<RwLock<Option<EnvConfig>>>,
}

impl DynamicConfigLoader {
    /// 创建加载器
    pub fn new(config_path: PathBuf, storage: Arc<StorageService>) -> Self {
        Self {
            config_path,
            storage,
            configs: Arc::new(RwLock::new(HashMap::new())),
            metadata: Arc::new(RwLock::new(None)),
            base_config: Arc::new(RwLock::new(None)),
        }
    }

    /// 加载配置（分层加载 + 版本校验）
    ///
    /// 流程：
    /// 1. YAML 加载 → 基准配置（软校验，不存在则告警+用默认值）
    /// 2. DB 查询 → 操作参数覆盖
    /// 3. 合并配置
    pub async fn load(&self, transformer_id: &str) -> Result<ActionSpaceConfig, AiEngineError> {
        // 1. 加载 YAML（软校验）
        let yaml_config = self.load_yaml().await?;

        // 2. 查询 DB
        let db_config = self.load_from_db(transformer_id).await?;

        // 3. 合并配置
        let merged = self.merge_config(transformer_id, &yaml_config, db_config.as_ref());

        // 4. 写入缓存
        {
            let mut cache = self.configs.write().await;
            cache.insert(transformer_id.to_string(), merged.clone());
        }

        Ok(merged)
    }

    /// 从 YAML 加载配置（软校验：不存在仅告警）
    async fn load_yaml(&self) -> Result<EnvConfig, AiEngineError> {
        if !self.config_path.exists() {
            tracing::warn!(
                "YAML 配置文件不存在: {}，使用内置默认值",
                self.config_path.display()
            );
            let default = EnvConfig::default();
            *self.metadata.write().await = Some(default.version.clone());
            *self.base_config.write().await = Some(default.clone());
            return Ok(default);
        }

        // EnvConfig::from_file 被注释（等待 Task 7），直接使用 serde_yaml解析
        let content = std::fs::read_to_string(&self.config_path)
            .map_err(|e| AiEngineError::ConfigLoadFailed(format!("读取文件失败: {}", e)))?;
        let config: EnvConfig = serde_yaml::from_str(&content)
            .map_err(|e| AiEngineError::ConfigLoadFailed(format!("YAML 解析失败: {}", e)))?;

        *self.metadata.write().await = Some(config.version.clone());
        *self.base_config.write().await = Some(config.clone());
        Ok(config)
    }

    /// 从数据库加载配置记录
    async fn load_from_db(
        &self,
        transformer_id: &str,
    ) -> Result<Option<ActionSpaceConfig>, AiEngineError> {
        let row: Option<DbActionSpaceConfig> = sqlx::query_as(
            "SELECT transformer_id, max_batt_charge_power, max_batt_discharge_power,
                    max_load_shedding, max_apparent_power_kva, p_batt_ramp_limit_kw,
                    q_batt_ramp_limit_kvar, pv_limit_min,
                    transformer_kva, battery_capacity_kwh,
                    soc_min, soc_max, overload_threshold
             FROM action_space_config WHERE transformer_id = ?",
        )
        .bind(transformer_id)
        .fetch_optional(self.storage.pool().as_ref())
        .await
        .map_err(|e| AiEngineError::ConfigLoadFailed(format!("数据库查询失败: {}", e)))?;

        Ok(row.map(|r| ActionSpaceConfig {
            transformer_id: r.transformer_id,
            max_batt_charge_power: r.max_batt_charge_power,
            max_batt_discharge_power: r.max_batt_discharge_power,
            max_load_shedding: r.max_load_shedding,
            max_apparent_power_kva: r.max_apparent_power_kva,
            p_batt_ramp_limit_kw: r.p_batt_ramp_limit_kw,
            q_batt_ramp_limit_kvar: r.q_batt_ramp_limit_kvar,
            pv_limit_min: r.pv_limit_min,
            transformer_kva: r.transformer_kva,
            battery_capacity_kwh: r.battery_capacity_kwh,
            soc_min: r.soc_min,
            soc_max: r.soc_max,
            overload_threshold: r.overload_threshold,
        }))
    }

    /// 合并配置
    ///
    /// 策略：
    /// - RL 核心参数（p_batt_max, load_shed_max, transformer_kva, battery_capacity_kwh）：来自 YAML
    /// - 安全约束（soc_min, soc_max, overload_threshold）：来自 YAML，DB 可覆盖
    /// - 操作调优参数（ramp, pv_limit_min）：DB 优先，无则用 YAML
    fn merge_config(
        &self,
        transformer_id: &str,
        yaml: &EnvConfig,
        db: Option<&ActionSpaceConfig>,
    ) -> ActionSpaceConfig {
        let db = db.cloned().unwrap_or_else(|| {
            // 首次部署：用 YAML 值写入 DB
            let cfg = ActionSpaceConfig {
                transformer_id: transformer_id.to_string(),
                max_batt_charge_power: yaml.physical.p_batt_max_kw,
                max_batt_discharge_power: yaml.physical.p_batt_max_kw,
                max_load_shedding: yaml.physical.load_shed_max_kw,
                max_apparent_power_kva: yaml.physical.transformer_kva,
                p_batt_ramp_limit_kw: yaml.operational.p_batt_ramp_limit_kw,
                q_batt_ramp_limit_kvar: yaml.operational.q_batt_ramp_limit_kvar,
                pv_limit_min: yaml.operational.pv_limit_min,
                transformer_kva: yaml.physical.transformer_kva,
                battery_capacity_kwh: yaml.physical.battery_capacity_kwh,
                soc_min: yaml.safety.soc_min,
                soc_max: yaml.safety.soc_max,
                overload_threshold: yaml.safety.overload_threshold,
            };
            // 异步写入 DB（不阻塞）
            // 注意：update_action_space_config_full 在 Task 6 中添加
            let storage = self.storage.clone();
            let transformer_id = transformer_id.to_string();
            let cfg_clone = cfg.clone();
            tokio::spawn(async move {
                // TODO(Task 6): 替换为 update_action_space_config_full
                let _ = storage
                    .update_action_space_config(
                        &transformer_id,
                        cfg_clone.max_batt_charge_power,
                        cfg_clone.max_batt_discharge_power,
                        cfg_clone.max_load_shedding,
                        cfg_clone.max_apparent_power_kva,
                        cfg_clone.p_batt_ramp_limit_kw,
                        cfg_clone.q_batt_ramp_limit_kvar,
                        cfg_clone.pv_limit_min,
                    )
                    .await;
            });
            cfg
        });

        ActionSpaceConfig {
            // RL 核心参数：来自 YAML（锁定）
            transformer_id: transformer_id.to_string(),
            max_batt_charge_power: yaml.physical.p_batt_max_kw,
            max_batt_discharge_power: yaml.physical.p_batt_max_kw,
            max_load_shedding: yaml.physical.load_shed_max_kw,
            max_apparent_power_kva: yaml.physical.transformer_kva,
            transformer_kva: yaml.physical.transformer_kva,
            battery_capacity_kwh: yaml.physical.battery_capacity_kwh,
            // 安全约束：来自 DB（可覆盖），无则用 YAML
            soc_min: db.soc_min,
            soc_max: db.soc_max,
            overload_threshold: db.overload_threshold,
            // 操作调优参数：来自 DB（优先），无则用 YAML
            p_batt_ramp_limit_kw: db.p_batt_ramp_limit_kw,
            q_batt_ramp_limit_kvar: db.q_batt_ramp_limit_kvar,
            pv_limit_min: db.pv_limit_min,
        }
    }

    /// 校验版本指纹
    pub async fn validate_fingerprint(&self, expected: &str) -> Result<(), AiEngineError> {
        let metadata = self.metadata.read().await;
        if let Some(ref meta) = *metadata {
            if meta.fingerprint != expected {
                return Err(AiEngineError::ConfigMismatch(format!(
                    "指纹不匹配: expected={}, actual={}",
                    expected, meta.fingerprint
               )));
            }
        }
        Ok(())
    }

    /// 获取配置指纹
    pub async fn get_fingerprint(&self) -> Option<String> {
        let metadata = self.metadata.read().await;
        metadata.as_ref().map(|m| m.fingerprint.clone())
    }

    /// 重载操作参数（运行时，不影响 RL 模型）
    pub async fn reload_operational(&self, transformer_id: &str) -> Result<ActionSpaceConfig, AiEngineError> {
        self.load(transformer_id).await
    }

    /// 获取内存缓存中的配置
    pub async fn get_config(&self, transformer_id: &str) -> Option<ActionSpaceConfig> {
        let cache = self.configs.read().await;
        cache.get(transformer_id).cloned()
    }

    /// 清除所有缓存
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
    max_load_shedding: f64,
    max_apparent_power_kva: f64,
    p_batt_ramp_limit_kw: f64,
    q_batt_ramp_limit_kvar: f64,
    pv_limit_min: f64,
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
    fn test_default_env_config_has_valid_fingerprint() {
        let cfg = EnvConfig::default();
        assert_eq!(cfg.fingerprint(), "unknown");
    }
}