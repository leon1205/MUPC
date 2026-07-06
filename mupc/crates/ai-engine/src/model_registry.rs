//! 多场景模型注册表
//!
//! v2.3 新增 — 管理 5 个场景 RL 模型的加载/卸载/双缓冲热切换。
//!
//! 核心设计：
//! - active 槽：当前激活的场景模型，处理所有推理请求
//! - standby 槽：热切换目标模型，加载完成后原子交换为 active
//! - manifest.json：持久化的模型清单（场景 → 文件名 + SHA256）
//! - 出厂预装 1 个场景模型，OTA 按需推送其余场景模型
//!
//! ## 锁顺序约束
//! 当多个锁需要同时持有时，必须按以下顺序获取（避免死锁）：
//! 1. scene_states (std::sync::RwLock)
//! 2. standby (tokio::sync::RwLock)
//! 3. active (tokio::sync::RwLock)
//! 任何违反此顺序的代码路径都会导致死锁。

use crate::action_space::ActionSpaceConfig;
use crate::config::RlAlgorithm;
use crate::error::AiEngineError;
use crate::mode_selector::RunningMode;
use crate::rknn_runtime::RknnRuntime;
use crate::rl_model::{parse_action_output, ActionOutput};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 场景模型状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneModelState {
    NotLoaded,
    Loading,
    Ready,
    Error,
}

/// 模型清单条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifestEntry {
    pub file_name: String,
    pub sha256: String,
    pub file_size_bytes: u64,
    pub version: String,
}

/// 场景切换结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneSwitchResult {
    Switched,
    Downloading,
}

/// manifest.json 文件格式（models 字段值复用 ModelManifestEntry）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFile {
    version: String,
    updated_at: String,
    models: HashMap<String, ModelManifestEntry>,
}

/// 场景模型注册表
pub struct ModelRegistry {
    active: Arc<RwLock<(RunningMode, RknnRuntime)>>,
    standby: Arc<RwLock<Option<(RunningMode, RknnRuntime)>>>,
    model_dir: PathBuf,
    manifest: Arc<RwLock<HashMap<RunningMode, ModelManifestEntry>>>,
    manifest_path: PathBuf,
    /// 场景状态（使用 std RwLock：操作是纯 HashMap 插入，无 await，避免 fire-and-forget 竞态）
    scene_states: Arc<std::sync::RwLock<HashMap<RunningMode, SceneModelState>>>,
    /// RL 训练算法（所有场景模型共用，Phase 3C.2 在线微调使用）
    #[allow(dead_code)]
    algorithm: RlAlgorithm,
    #[allow(dead_code)]
    quantization: crate::config::QuantizationType,
}

impl ModelRegistry {
    /// 创建注册表，初始加载出厂场景模型
    pub async fn new(
        model_dir: &Path,
        manifest_path: &Path,
        factory_scene: RunningMode,
        algorithm: RlAlgorithm,
        quantization: crate::config::QuantizationType,
    ) -> Result<Self, AiEngineError> {
        let manifest = Self::load_manifest(manifest_path).await.unwrap_or_default();

        // 初始化所有场景为 NotLoaded
        let mut scene_states: HashMap<RunningMode, SceneModelState> = HashMap::new();
        for mode in RunningMode::all() {
            scene_states.insert(*mode, SceneModelState::NotLoaded);
        }

        // 标记出厂场景为 Loading
        scene_states.insert(factory_scene, SceneModelState::Loading);

        // 查找出厂场景的模型文件
        let entry = manifest.get(&factory_scene).ok_or_else(|| {
            AiEngineError::ModelLoadFailed(format!(
                "出厂场景 {} 的模型不在清单中",
                factory_scene.display_name()
            ))
        })?;

        let model_path = model_dir.join(&entry.file_name);

        // SHA256 校验
        Self::verify_sha256(&model_path, &entry.sha256).await?;

        // 加载出厂场景模型
        let runtime = RknnRuntime::new(&model_path, Some(&entry.sha256))?;
        runtime
            .load()
            .await
            .map_err(|e| AiEngineError::ModelLoadFailed(format!("出厂场景模型加载失败: {}", e)))?;

        scene_states.insert(factory_scene, SceneModelState::Ready);

        Ok(Self {
            active: Arc::new(RwLock::new((factory_scene, runtime))),
            standby: Arc::new(RwLock::new(None)),
            model_dir: model_dir.to_path_buf(),
            manifest: Arc::new(RwLock::new(manifest)),
            manifest_path: manifest_path.to_path_buf(),
            scene_states: Arc::new(std::sync::RwLock::new(scene_states)),
            algorithm,
            quantization,
        })
    }

    /// 热切换到目标场景模型（双缓冲）
    ///
    /// 1. 目标 == 当前 → 幂等返回
    /// 2. 目标模型文件缺失 → 返回 Downloading
    /// 3. 目标模型加载到 standby 槽 → 原子交换 → 返回 Switched
    pub async fn switch_to(&self, mode: RunningMode) -> Result<SceneSwitchResult, AiEngineError> {
        let current_mode = self.active.read().await.0;

        if current_mode == mode {
            return Ok(SceneSwitchResult::Switched);
        }

        // 检查目标模型清单条目
        let entry = {
            let manifest = self.manifest.read().await;
            manifest.get(&mode).cloned()
        };

        let entry = match entry {
            Some(e) => e,
            None => {
                // 重新加载清单后重试
                if let Ok(()) = self.reload_manifest().await {
                    let manifest = self.manifest.read().await;
                    match manifest.get(&mode).cloned() {
                        Some(e) => e,
                        None => return Ok(SceneSwitchResult::Downloading),
                    }
                } else {
                    return Ok(SceneSwitchResult::Downloading);
                }
            }
        };

        let model_path = self.model_dir.join(&entry.file_name);

        // 检查文件存在性
        if !model_path.exists() {
            return Ok(SceneSwitchResult::Downloading);
        }

        // SHA256 校验
        if let Err(e) = Self::verify_sha256(&model_path, &entry.sha256).await {
            self.set_scene_state(mode, SceneModelState::Error);
            return Err(AiEngineError::ChecksumMismatch {
                expected: entry.sha256.clone(),
                actual: e.to_string(),
            });
        }

        // 标记目标场景为 Loading
        self.set_scene_state(mode, SceneModelState::Loading);

        // 加载到 standby 槽
        let new_runtime = RknnRuntime::new(&model_path, Some(&entry.sha256)).map_err(|e| {
            self.set_scene_state(mode, SceneModelState::Error);
            AiEngineError::ModelLoadFailed(format!("模型初始化失败: {}", e))
        })?;

        new_runtime.load().await.map_err(|e| {
            self.set_scene_state(mode, SceneModelState::Error);
            AiEngineError::ModelLoadFailed(format!("模型加载到 NPU 失败: {}", e))
        })?;

        // 原子交换 active ↔ standby（严格遵守锁顺序：先 standby 后 active）
        {
            let mut standby = self.standby.write().await;
            let mut active = self.active.write().await;

            // 将当前 active 模型移到 standby（延迟释放）
            let old_active = std::mem::replace(&mut *active, (mode, new_runtime));
            *standby = Some(old_active);
        }

        // 标记目标场景为 Ready，旧场景状态不变（仍为 Ready，模型文件在本地）
        self.set_scene_state(mode, SceneModelState::Ready);

        tracing::info!(
            "场景模型热切换完成: {} → {}",
            current_mode.display_name(),
            mode.display_name()
        );

        // 延迟 30s 后释放 standby 中的旧模型（等待进行中的推理完成）
        let standby_arc = self.standby.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let mut guard = standby_arc.write().await;
            if let Some((old_mode, _)) = guard.take() {
                tracing::debug!("延迟释放旧场景模型: {}", old_mode.display_name());
            }
        });

        Ok(SceneSwitchResult::Switched)
    }

    /// 查询指定场景的模型状态
    pub fn model_state(&self, mode: RunningMode) -> SceneModelState {
        let states = self.scene_states.read().unwrap();
        states
            .get(&mode)
            .copied()
            .unwrap_or(SceneModelState::NotLoaded)
    }

    /// 获取所有场景的模型状态列表
    pub fn all_model_states(&self) -> Vec<(RunningMode, SceneModelState)> {
        let states = self.scene_states.read().unwrap();
        let mut result: Vec<_> = states.iter().map(|(&k, &v)| (k, v)).collect();
        result.sort_by_key(|(mode, _)| *mode as u8);
        result
    }

    /// 后台预加载模型到 standby 槽（闲时调用）
    pub async fn preload(&self, mode: RunningMode) -> Result<(), AiEngineError> {
        // 仅在 standby 为空时才预加载
        if self.standby.read().await.is_some() {
            return Ok(());
        }

        let state = self.model_state(mode);
        if state != SceneModelState::Ready {
            return Ok(());
        }

        let entry = {
            let manifest = self.manifest.read().await;
            manifest.get(&mode).cloned()
        };

        let entry = match entry {
            Some(e) => e,
            None => return Ok(()),
        };

        let model_path = self.model_dir.join(&entry.file_name);
        if !model_path.exists() {
            return Ok(());
        }

        match RknnRuntime::new(&model_path, Some(&entry.sha256)) {
            Ok(rt) => {
                if rt.load().await.is_ok() {
                    let mut standby = self.standby.write().await;
                    *standby = Some((mode, rt));
                    tracing::debug!("后台预加载完成: {}", mode.display_name());
                }
            }
            Err(e) => {
                tracing::warn!("后台预加载失败 ({}): {}", mode.display_name(), e);
            }
        }

        Ok(())
    }

    /// 从 OTA 下载模型文件
    ///
    /// 下载完成后更新 manifest.json 并标记场景为 Ready。
    /// 当前为占位实现，真实 OTA 集成待 Phase 2+。
    pub async fn download_model(&self, _mode: RunningMode) -> Result<(), AiEngineError> {
        Err(AiEngineError::ModelLoadFailed(
            "OTA 模型下载功能待 Phase 2+ 实现".into(),
        ))
    }

    /// 获取当前激活的场景
    pub async fn current_mode(&self) -> RunningMode {
        self.active.read().await.0
    }

    /// 执行推理（委托给当前 active 模型）
    /// v2.5: 动作空间参数可配置化，接收 ActionSpaceConfig 参数
    pub async fn decide(
        &self,
        input_vector: &[f32],
        action_space_config: &ActionSpaceConfig,
    ) -> Result<ActionOutput, AiEngineError> {
        let active = self.active.read().await;
        active.1.run(input_vector).await.and_then(|output| {
            parse_action_output(&output, action_space_config)
                .ok_or_else(|| AiEngineError::InferenceFailed("输出维度不足".into()))
        })
    }

    /// 重新加载模型清单
    pub async fn reload_manifest(&self) -> Result<(), AiEngineError> {
        let new_manifest = Self::load_manifest(&self.manifest_path).await?;
        let mut manifest = self.manifest.write().await;

        // 更新场景状态：如果新清单中出现了之前的缺失项，标记为 Ready
        for mode in RunningMode::all() {
            if !manifest.contains_key(mode) && new_manifest.contains_key(mode) {
                let model_path = self.model_dir.join(&new_manifest[mode].file_name);
                if model_path.exists() {
                    self.set_scene_state(*mode, SceneModelState::Ready);
                }
            }
        }

        *manifest = new_manifest;
        Ok(())
    }

    /// 检查指定场景的模型文件是否存在且 SHA256 校验通过
    pub async fn verify_model(&self, mode: RunningMode) -> Result<(), AiEngineError> {
        let manifest = self.manifest.read().await;
        let entry = manifest
            .get(&mode)
            .ok_or_else(|| AiEngineError::ModelLoadFailed("模型不在清单中".into()))?;

        let model_path = self.model_dir.join(&entry.file_name);
        if !model_path.exists() {
            return Err(AiEngineError::ModelLoadFailed("模型文件不存在".into()));
        }

        Self::verify_sha256(&model_path, &entry.sha256).await
    }

    /// 强制卸载 standby 槽中的模型
    pub async fn evict_standby(&self) {
        let mut standby = self.standby.write().await;
        if let Some((mode, _)) = standby.take() {
            tracing::debug!("手动释放 standby 模型: {}", mode.display_name());
        }
    }

    // ─── 内部方法 ───

    fn set_scene_state(&self, mode: RunningMode, state: SceneModelState) {
        let mut states = self.scene_states.write().unwrap();
        states.insert(mode, state);
    }

    async fn load_manifest(
        path: &Path,
    ) -> Result<HashMap<RunningMode, ModelManifestEntry>, AiEngineError> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| AiEngineError::ModelLoadFailed(format!("读取模型清单失败: {}", e)))?;

        let mf: ManifestFile = serde_json::from_str(&content).map_err(|e| {
            AiEngineError::ModelLoadFailed(format!("模型清单 JSON 解析失败: {}", e))
        })?;

        let mut map = HashMap::new();
        for (key, entry) in mf.models {
            let mode = match key.as_str() {
                // MODE-01: 兼容旧 manifest key
                "SeasonalLoadManagement" | "AgriculturalIrrigation" => {
                    RunningMode::SeasonalLoadManagement
                }
                "CommercialArbitrage" => RunningMode::CommercialArbitrage,
                "DemandControl" => RunningMode::DemandControl,
                "VirtualPowerPlant" => RunningMode::VirtualPowerPlant,
                "UltraGreen" => RunningMode::UltraGreen,
                _ => {
                    tracing::warn!("清单中存在未知场景: {}", key);
                    continue;
                }
            };
            map.insert(mode, entry);
        }

        Ok(map)
    }

    async fn verify_sha256(path: &Path, expected: &str) -> Result<(), AiEngineError> {
        // 空 SHA256 表示未配置校验值（默认清单占位），跳过校验
        if expected.is_empty() {
            tracing::warn!("模型文件 {} 的 SHA256 为空，跳过校验", path.display());
            return Ok(());
        }

        use sha2::{Digest, Sha256};
        let data = tokio::fs::read(path)
            .await
            .map_err(|e| AiEngineError::ModelLoadFailed(format!("读取模型文件失败: {}", e)))?;

        let mut hasher = Sha256::new();
        hasher.update(&data);
        let result = hasher.finalize();
        let actual = format!("{:x}", result);

        if actual != expected {
            return Err(AiEngineError::ChecksumMismatch {
                expected: expected.to_string(),
                actual,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_model_state_debug() {
        assert_eq!(format!("{:?}", SceneModelState::Ready), "Ready");
        assert_eq!(format!("{:?}", SceneModelState::NotLoaded), "NotLoaded");
    }

    #[test]
    fn test_scene_switch_result_eq() {
        assert_eq!(SceneSwitchResult::Switched, SceneSwitchResult::Switched);
        assert_ne!(SceneSwitchResult::Switched, SceneSwitchResult::Downloading);
    }

    #[test]
    fn test_manifest_entry_serialization() {
        let entry = ModelManifestEntry {
            file_name: "rl_agricultural.rknn".into(),
            sha256: "abc123".into(),
            file_size_bytes: 4823456,
            version: "2.3.0".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("rl_agricultural.rknn"));
        assert!(json.contains("abc123"));
    }

    #[test]
    fn test_all_modes_have_default_state() {
        let modes = RunningMode::all();
        assert_eq!(modes.len(), 5);
        // 验证所有模式枚举值不重复
        let mut seen = std::collections::HashSet::new();
        for &m in modes {
            assert!(seen.insert(m as u8), "重复的模式 ID: {}", m as u8);
        }
    }
}
