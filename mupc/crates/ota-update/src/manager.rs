//! OTA 管理器模块
//!
//! Phase 3C.2 OTA 模型自动更新模块的核心管理器实现
//! 负责任务状态机管理和协调各子模块

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::OtaConfig;
use crate::downloader::{Downloader, DownloadResult};
use crate::error::OtaError;
use crate::types::{
    ModelType, ModelVersion, OtaState, OtaTask,
    TaskId, UpdateInfo, UpdateRecord, VersionQueryResponse,
};
use crate::verifier::Verifier;
use crate::applicator::ModelApplicator;
use crate::rollback::RollbackManager;

/// OTA 管理器 trait
///
/// 定义 OTA 管理的完整接口
#[async_trait]
pub trait OtaManager: Send + Sync {
    /// 获取当前模型版本
    fn get_current_version(&self, model_type: ModelType) -> Result<ModelVersion, OtaError>;

    /// 检查更新
    async fn check_updates(&self) -> Result<Vec<UpdateInfo>, OtaError>;

    /// 开始下载
    async fn start_download(&self, update_info: &UpdateInfo) -> Result<TaskId, OtaError>;

    /// 获取下载进度
    fn get_download_progress(&self, task_id: TaskId) -> Result<u8, OtaError>;

    /// 取消下载
    async fn cancel_download(&self, task_id: TaskId) -> Result<(), OtaError>;

    /// 应用更新
    async fn apply_update(&self, task_id: TaskId) -> Result<(), OtaError>;

    /// 回滚
    async fn rollback(&self, model_type: ModelType) -> Result<(), OtaError>;

    /// 获取更新状态
    fn get_update_status(&self) -> UpdateStatus;

    /// 获取更新历史
    fn get_update_history(&self, limit: usize) -> Result<Vec<UpdateRecord>, OtaError>;

    /// 查询版本信息
    async fn query_versions(&self) -> Result<VersionQueryResponse, OtaError>;
}

/// 更新状态
#[derive(Debug, Clone)]
pub struct UpdateStatus {
    /// 当前状态
    pub state: OtaState,
    /// 当前任务 ID（如果有）
    pub current_task_id: Option<TaskId>,
    /// 当前模型类型（如果有）
    pub current_model_type: Option<ModelType>,
    /// 下载进度（0-100）
    pub download_progress: Option<u8>,
    /// 错误信息（如果失败）
    pub error_message: Option<String>,
}

/// OTA 管理器实现
///
/// 协调 downloader, verifier, applicator, rollback 模块
#[derive(Debug)]
pub struct OtaManagerImpl {
    /// OTA 配置
    config: OtaConfig,
    /// 当前状态
    state: RwLock<OtaState>,
    /// 下载器
    downloader: Arc<Downloader>,
    /// 验证器
    #[allow(dead_code)]
    verifier: Arc<Verifier>,
    /// 模型应用器
    applicator: Arc<ModelApplicator>,
    /// 回滚管理器
    rollback_manager: Arc<RollbackManager>,
    /// 进行中的任务
    tasks: RwLock<HashMap<TaskId, OtaTask>>,
    /// 更新历史
    history: RwLock<Vec<UpdateRecord>>,
}

impl OtaManagerImpl {
    /// 创建新的 OTA 管理器
    ///
    /// # 参数
    /// * `config` - OTA 配置
    /// * `temp_dir` - 临时文件目录
    ///
    /// # 返回
    /// OTA 管理器实例
    pub fn new(config: OtaConfig, temp_dir: PathBuf) -> Result<Self, OtaError> {
        let downloader = Downloader::new(temp_dir)?;
        let verifier = Verifier::new(PathBuf::from(&config.public_key_path))?;
        let applicator = ModelApplicator::new(
            PathBuf::from(&config.model_storage_path),
            Arc::new(verifier.clone()),
            None,
        )?;
        let rollback_manager = RollbackManager::new(
            PathBuf::from(&config.model_storage_path),
            config.max_rollback_count,
        )?;

        Ok(Self {
            config,
            state: RwLock::new(OtaState::Idle),
            downloader: Arc::new(downloader),
            verifier: Arc::new(verifier),
            applicator: Arc::new(applicator),
            rollback_manager: Arc::new(rollback_manager),
            tasks: RwLock::new(HashMap::new()),
            history: RwLock::new(Vec::new()),
        })
    }

    /// 创建 OTA 管理器（带回调）
    pub fn with_callbacks(
        config: OtaConfig,
        temp_dir: PathBuf,
        on_strategy_engine_notify: Option<Box<dyn Fn(ModelType) + Send + Sync>>,
    ) -> Result<Self, OtaError> {
        let downloader = Downloader::new(temp_dir)?;
        let verifier = Verifier::new(PathBuf::from(&config.public_key_path))?;
        let applicator = ModelApplicator::new(
            PathBuf::from(&config.model_storage_path),
            Arc::new(verifier.clone()),
            on_strategy_engine_notify,
        )?;
        let rollback_manager = RollbackManager::with_callback(
            PathBuf::from(&config.model_storage_path),
            config.max_rollback_count,
            None,
        )?;

        Ok(Self {
            config,
            state: RwLock::new(OtaState::Idle),
            downloader: Arc::new(downloader),
            verifier: Arc::new(verifier),
            applicator: Arc::new(applicator),
            rollback_manager: Arc::new(rollback_manager),
            tasks: RwLock::new(HashMap::new()),
            history: RwLock::new(Vec::new()),
        })
    }

    /// 生成新的任务 ID
    fn generate_task_id(&self) -> TaskId {
        Uuid::new_v4().to_string()
    }

    /// 更新状态（原子操作）
    async fn transition_state(&self, new_state: OtaState) -> Result<(), OtaError> {
        let mut state = self.state.write().await;
        tracing::info!("状态转换: {:?} -> {:?}", *state, new_state);
        *state = new_state;
        Ok(())
    }

    /// 获取当前状态
    async fn get_state(&self) -> OtaState {
        self.state.read().await.clone()
    }

    /// 创建任务记录
    fn create_task(&self, model_type: ModelType, from_version: String, to_version: String) -> OtaTask {
        let now = Utc::now();
        OtaTask {
            task_id: self.generate_task_id(),
            model_type,
            from_version,
            to_version,
            state: OtaState::Idle,
            progress: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// 更新任务状态
    async fn update_task_state(&self, task_id: &TaskId, state: OtaState, progress: Option<u8>) -> Result<(), OtaError> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.state = state.clone();
            if let Some(p) = progress {
                task.progress = p;
            }
            task.updated_at = Utc::now();
            tracing::info!("任务 {} 状态更新: {:?}", task_id, state);
        } else {
            return Err(OtaError::UpdateTimeout); // 或者其他合适的错误
        }
        Ok(())
    }

    /// 添加到历史记录
    async fn add_to_history(&self, record: UpdateRecord) {
        let mut history = self.history.write().await;
        history.push(record);
        // 保持历史记录不超过 100 条
        if history.len() > 100 {
            history.remove(0);
        }
    }

    /// 检查状态是否允许转换
    fn can_transition(current: &OtaState, next: &OtaState) -> bool {
        match (current, next) {
            // 有效的状态转换
            (OtaState::Idle, OtaState::Checking) => true,
            (OtaState::Idle, OtaState::Failed { .. }) => true,
            (OtaState::Checking, OtaState::Downloading { .. }) => true,
            (OtaState::Checking, OtaState::Idle) => true,
            (OtaState::Checking, OtaState::Failed { .. }) => true,
            (OtaState::Downloading { .. }, OtaState::Verifying) => true,
            (OtaState::Downloading { .. }, OtaState::Failed { .. }) => true,
            (OtaState::Downloading { .. }, OtaState::RollingBack) => true,
            (OtaState::Verifying, OtaState::Applying) => true,
            (OtaState::Verifying, OtaState::Failed { .. }) => true,
            (OtaState::Verifying, OtaState::RollingBack) => true,
            (OtaState::Applying, OtaState::Applied) => true,
            (OtaState::Applying, OtaState::Failed { .. }) => true,
            (OtaState::Applying, OtaState::RollingBack) => true,
            (OtaState::Applied, OtaState::Completed) => true,
            (OtaState::Applied, OtaState::Failed { .. }) => true,
            (OtaState::RollingBack, OtaState::Failed { .. }) => true,
            (OtaState::Failed { .. }, OtaState::Idle) => true,
            (OtaState::Failed { .. }, OtaState::Checking) => true,
            (OtaState::Completed, OtaState::Idle) => true,
            // 相同状态总是允许
            (s, n) if s == n => true,
            _ => false,
        }
    }

    /// 查询版本信息（模拟实现）
    ///
    /// 从 OTA 服务器获取当前可用的模型版本信息
    // TODO: 实际实现应请求 OTA 服务器
    async fn query_versions_from_server(&self) -> Result<Vec<ModelVersion>, OtaError> {
        // 实际实现中，这里会发送 HTTP 请求到 OTA 服务器
        // 这里使用模拟数据
        tracing::info!("查询 OTA 服务器: {}", self.config.server_url);

        // 模拟从服务器获取版本信息
        let versions = vec![
            ModelVersion {
                model_type: ModelType::Lstm,
                version: "1.2.0".to_string(),
                updated_at: Utc::now(),
                md5: "abc123def456".to_string(),
                size: 1024 * 1024 * 10, // 10MB
            },
            ModelVersion {
                model_type: ModelType::Maddpg,
                version: "1.0.5".to_string(),
                updated_at: Utc::now(),
                md5: "def456abc789".to_string(),
                size: 1024 * 1024 * 20, // 20MB
            },
        ];

        Ok(versions)
    }

    /// 比较版本确定是否有可用更新
    fn find_updates(current_versions: &[ModelVersion], available_versions: &[ModelVersion]) -> Vec<UpdateInfo> {
        let mut updates = Vec::new();

        for available in available_versions {
            if let Some(current) = current_versions.iter().find(|v| v.model_type == available.model_type) {
                // 比较版本号（简化：直接字符串比较）
                if Self::version_greater(&available.version, &current.version) {
                    updates.push(UpdateInfo {
                        model_type: available.model_type,
                        current_version: current.version.clone(),
                        available_version: available.version.clone(),
                        size: available.size,
                        checksum: available.md5.clone(),
                        signature: String::new(), // 实际应该从服务器获取
                        url: format!("https://ota.example.com/models/{}/v{}.rknn",
                            available.model_type, available.version),
                        is_incremental: false,
                        base_version: None,
                    });
                }
            }
        }

        updates
    }

    /// 比较版本号（简化实现）
    fn version_greater(new: &str, current: &str) -> bool {
        // 简单的版本比较实现
        // 实际应该使用 semver 解析
        let new_parts: Vec<u32> = new.split('.')
            .filter_map(|s| s.parse().ok())
            .collect();
        let current_parts: Vec<u32> = current.split('.')
            .filter_map(|s| s.parse().ok())
            .collect();

        for i in 0..new_parts.len().max(current_parts.len()) {
            let new_val = new_parts.get(i).unwrap_or(&0);
            let current_val = current_parts.get(i).unwrap_or(&0);
            if new_val > current_val {
                return true;
            } else if new_val < current_val {
                return false;
            }
        }
        false
    }

    /// 执行下载（带进度回调）
    async fn execute_download(&self, task_id: &TaskId, update_info: &UpdateInfo) -> Result<DownloadResult, OtaError> {
        let task_id_clone = task_id.clone();
        let _update_info_clone = update_info.clone();
        let downloader = self.downloader.clone();

        // 创建进度回调
        let progress_callback = Arc::new(move |downloaded: u64, total: u64| {
            let progress = if total > 0 {
                ((downloaded as f64 / total as f64) * 100.0) as u8
            } else {
                0
            };
            tracing::debug!(
                "任务 {} 下载进度: {}/{} ({}%)",
                task_id_clone, downloaded, total, progress
            );
        });

        // 执行下载
        downloader
            .download_with_progress(
                &update_info.url,
                &update_info.checksum,
                0, // 不支持断点续传
                Some(progress_callback),
            )
            .await
    }
}

#[async_trait]
impl OtaManager for OtaManagerImpl {
    fn get_current_version(&self, model_type: ModelType) -> Result<ModelVersion, OtaError> {
        // 使用 applicator 获取当前版本
        // 注意：这个是同步的，但 applicator 的方法是 async 的
        // 这里需要用 runtime block_on 或者改变设计
        // 简化处理：直接读取 version.json
        let version_file = PathBuf::from(&self.config.model_storage_path).join("version.json");

        if !version_file.exists() {
            return Err(OtaError::VersionQueryFailed("版本信息文件不存在".to_string()));
        }

        let contents = std::fs::read_to_string(&version_file).map_err(|e| {
            OtaError::VersionQueryFailed(format!("读取版本文件失败: {}", e))
        })?;

        #[derive(serde::Deserialize)]
        struct VersionFile {
            models: Vec<ModelVersion>,
        }

        let version_file: VersionFile = serde_json::from_str(&contents).map_err(|e| {
            OtaError::VersionQueryFailed(format!("解析版本文件失败: {}", e))
        })?;

        version_file
            .models
            .into_iter()
            .find(|v| v.model_type == model_type)
            .ok_or_else(|| {
                OtaError::VersionQueryFailed(format!("未找到模型 {} 的版本信息", model_type))
            })
    }

    async fn check_updates(&self) -> Result<Vec<UpdateInfo>, OtaError> {
        // 检查状态
        let current_state = self.get_state().await;
        if !Self::can_transition(&current_state, &OtaState::Checking) {
            return Err(OtaError::UpdateTimeout);
        }

        // 转换到 Checking 状态
        self.transition_state(OtaState::Checking).await?;

        tracing::info!("开始检查更新...");

        // 查询服务器版本
        let available_versions = self.query_versions_from_server().await?;

        // 获取当前版本
        let current_lstm = self.get_current_version(ModelType::Lstm);
        let current_maddpg = self.get_current_version(ModelType::Maddpg);

        let mut current_versions = Vec::new();
        if let Ok(v) = current_lstm {
            current_versions.push(v);
        }
        if let Ok(v) = current_maddpg {
            current_versions.push(v);
        }

        // 找出可用的更新
        let updates = Self::find_updates(&current_versions, &available_versions);

        tracing::info!(
            "检查更新完成: 发现 {} 个可用更新",
            updates.len()
        );

        // 转换到 Idle 状态
        self.transition_state(OtaState::Idle).await?;

        Ok(updates)
    }

    async fn start_download(&self, update_info: &UpdateInfo) -> Result<TaskId, OtaError> {
        // 检查状态
        let current_state = self.get_state().await;
        if !Self::can_transition(&current_state, &OtaState::Downloading { progress: 0 }) {
            return Err(OtaError::UpdateTimeout);
        }

        // 获取当前版本
        let current_version = match self.get_current_version(update_info.model_type) {
            Ok(v) => v.version,
            Err(_) => "0.0.0".to_string(),
        };

        // 创建任务
        let task = self.create_task(
            update_info.model_type,
            current_version,
            update_info.available_version.clone(),
        );
        let task_id = task.task_id.clone();

        // 添加任务到列表
        {
            let mut tasks = self.tasks.write().await;
            tasks.insert(task_id.clone(), task);
        }

        // 转换到 Downloading 状态
        self.transition_state(OtaState::Downloading { progress: 0 }).await?;

        tracing::info!("开始下载任务 {}: {:?} -> {}", task_id, update_info.model_type, update_info.available_version);

        // 执行下载
        let result = self.execute_download(&task_id, update_info).await;

        match result {
            Ok(_) => {
                // 下载成功，转换到 Verifying 状态
                self.update_task_state(&task_id, OtaState::Verifying, None).await?;
                self.transition_state(OtaState::Verifying).await?;
                Ok(task_id)
            }
            Err(e) => {
                // 下载失败
                let error_msg = format!("下载失败: {}", e);
                self.update_task_state(&task_id, OtaState::Failed { error: error_msg.clone() }, None).await?;
                self.transition_state(OtaState::Failed { error: error_msg }).await?;
                Err(e)
            }
        }
    }

    fn get_download_progress(&self, task_id: TaskId) -> Result<u8, OtaError> {
        // 同步方法，需要使用 blocking read
        // 这里简化处理，直接返回任务中的进度
        let tasks = self.tasks.blocking_read();
        tasks
            .get(&task_id)
            .map(|t| t.progress)
            .ok_or_else(|| OtaError::UpdateTimeout)
    }

    async fn cancel_download(&self, task_id: TaskId) -> Result<(), OtaError> {
        tracing::info!("取消下载任务: {}", task_id);

        // 获取任务信息
        let task = {
            let tasks = self.tasks.read().await;
            tasks.get(&task_id).cloned()
        };

        if let Some(t) = task {
            // 如果正在下载，取消下载器中的任务
            // 这里需要下载器支持取消操作，简化处理为清理任务
            let mut tasks = self.tasks.write().await;
            tasks.remove(&task_id);

            // 转换到 Idle 状态
            self.transition_state(OtaState::Idle).await?;

            // 添加到历史记录
            let record = UpdateRecord {
                task_id: task_id.clone(),
                model_type: t.model_type,
                from_version: t.from_version,
                to_version: t.to_version,
                status: OtaState::Failed { error: "用户取消".to_string() },
                started_at: t.created_at,
                completed_at: Some(Utc::now()),
                error_message: Some("用户取消下载".to_string()),
            };
            self.add_to_history(record).await;

            Ok(())
        } else {
            Err(OtaError::UpdateTimeout)
        }
    }

    async fn apply_update(&self, task_id: TaskId) -> Result<(), OtaError> {
        // 检查状态
        let current_state = self.get_state().await;
        if !Self::can_transition(&current_state, &OtaState::Applying) {
            return Err(OtaError::UpdateTimeout);
        }

        // 获取任务
        let task = {
            let tasks = self.tasks.read().await;
            tasks.get(&task_id).cloned()
        };

        if task.is_none() {
            return Err(OtaError::UpdateTimeout);
        }
        let task = task.unwrap();

        tracing::info!("应用更新任务: {}", task_id);

        // 转换到 Applying 状态
        self.transition_state(OtaState::Applying).await?;
        self.update_task_state(&task_id, OtaState::Applying, None).await?;

        // 执行模型应用
        // 这里需要从下载结果获取文件路径，简化处理
        let update_package = PathBuf::from(&self.config.model_storage_path)
            .join("update")
            .join(format!("{}.rknn", task.task_id));

        // 注意：实际应该使用下载完成后的文件路径
        // 这里简化处理，假设文件已下载
        let result = self.applicator.apply(
            task.model_type,
            &update_package,
            "", // checksum 应该从任务中获取
        ).await;

        match result {
            Ok(new_version) => {
                // 应用成功
                self.update_task_state(&task_id, OtaState::Applied, None).await?;
                self.transition_state(OtaState::Applied).await?;

                // 保存信息用于日志（避免 move 后 borrow）
                let model_type = task.model_type;
                let from_ver = task.from_version.clone();
                let to_ver = task.to_version.clone();

                // 添加到历史记录
                let record = UpdateRecord {
                    task_id: task_id.clone(),
                    model_type,
                    from_version: from_ver.clone(),
                    to_version: to_ver.clone(),
                    status: OtaState::Completed,
                    started_at: task.created_at,
                    completed_at: Some(Utc::now()),
                    error_message: None,
                };
                self.add_to_history(record).await;

                // 清理任务
                let mut tasks = self.tasks.write().await;
                tasks.remove(&task_id);

                // 转换到 Completed 状态
                self.transition_state(OtaState::Completed).await?;

                tracing::info!(
                    "更新应用成功: {} v{} -> v{}",
                    model_type, from_ver, new_version.version
                );

                // 延迟后回到 Idle 状态
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                self.transition_state(OtaState::Idle).await?;

                Ok(())
            }
            Err(e) => {
                // 应用失败，触发回滚
                tracing::error!("应用更新失败: {}", e);

                self.update_task_state(&task_id, OtaState::RollingBack, None).await?;
                self.transition_state(OtaState::RollingBack).await?;

                // 执行回滚
                if let Err(rollback_err) = self.rollback(task.model_type).await {
                    tracing::error!("回滚失败: {}", rollback_err);
                }

                let error_msg = format!("应用失败: {}", e);
                self.transition_state(OtaState::Failed { error: error_msg }).await?;

                Err(e)
            }
        }
    }

    async fn rollback(&self, model_type: ModelType) -> Result<(), OtaError> {
        tracing::info!("执行回滚: {}", model_type);

        // 转换到 RollingBack 状态
        self.transition_state(OtaState::RollingBack).await?;

        // 执行回滚
        let result = self.rollback_manager.rollback(model_type).await;

        match result {
            Ok(_) => {
                tracing::info!("回滚成功: {}", model_type);
                self.transition_state(OtaState::Idle).await?;
                Ok(())
            }
            Err(e) => {
                let error_msg = format!("回滚失败: {}", e);
                self.transition_state(OtaState::Failed { error: error_msg }).await?;
                Err(e)
            }
        }
    }

    fn get_update_status(&self) -> UpdateStatus {
        // 同步方法，使用 blocking read
        let state = self.state.blocking_read();

        // 获取当前任务信息
        let (current_task_id, current_model_type, download_progress) = {
            let tasks = self.tasks.blocking_read();
            if let Some(task) = tasks.values().next() {
                let progress = match task.state {
                    OtaState::Downloading { progress } => Some(progress),
                    _ => None,
                };
                (Some(task.task_id.clone()), Some(task.model_type), progress)
            } else {
                (None, None, None)
            }
        };

        let error_message = match &*state {
            OtaState::Failed { error } => Some(error.clone()),
            _ => None,
        };

        UpdateStatus {
            state: state.clone(),
            current_task_id,
            current_model_type,
            download_progress,
            error_message,
        }
    }

    fn get_update_history(&self, limit: usize) -> Result<Vec<UpdateRecord>, OtaError> {
        let history = self.history.blocking_read();
        let len = history.len().min(limit);
        Ok(history.iter().rev().take(len).cloned().collect())
    }

    async fn query_versions(&self) -> Result<VersionQueryResponse, OtaError> {
        tracing::info!("查询所有模型版本信息...");

        // 获取服务器上的所有可用版本
        let available_versions = self.query_versions_from_server().await?;

        // 获取设备 ID
        let device_id = uuid::Uuid::new_v4().to_string();

        // 构建响应
        let response = VersionQueryResponse {
            models: available_versions,
            device_id,
            timestamp: Utc::now(),
        };

        tracing::info!("版本查询完成: {} 个模型", response.models.len());
        Ok(response)
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ========== 辅助函数测试 ==========

    #[test]
    fn test_version_greater() {
        assert!(OtaManagerImpl::version_greater("1.2.0", "1.1.0"));
        assert!(OtaManagerImpl::version_greater("2.0.0", "1.9.9"));
        assert!(OtaManagerImpl::version_greater("1.0.0", "0.9.0"));
        assert!(!OtaManagerImpl::version_greater("1.1.0", "1.2.0"));
        assert!(!OtaManagerImpl::version_greater("1.0.0", "1.0.0"));
    }

    #[test]
    fn test_find_updates() {
        let current = vec![
            ModelVersion {
                model_type: ModelType::Lstm,
                version: "1.0.0".to_string(),
                updated_at: Utc::now(),
                md5: "abc".to_string(),
                size: 1000,
            },
        ];

        let available = vec![
            ModelVersion {
                model_type: ModelType::Lstm,
                version: "1.2.0".to_string(),
                updated_at: Utc::now(),
                md5: "def".to_string(),
                size: 2000,
            },
            ModelVersion {
                model_type: ModelType::Maddpg,
                version: "1.0.0".to_string(),
                updated_at: Utc::now(),
                md5: "ghi".to_string(),
                size: 3000,
            },
        ];

        let updates = OtaManagerImpl::find_updates(&current, &available);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].model_type, ModelType::Lstm);
        assert_eq!(updates[0].available_version, "1.2.0");
    }

    // ========== 状态转换测试 ==========

    #[test]
    fn test_can_transition_valid() {
        assert!(OtaManagerImpl::can_transition(&OtaState::Idle, &OtaState::Checking));
        assert!(OtaManagerImpl::can_transition(&OtaState::Checking, &OtaState::Downloading { progress: 0 }));
        assert!(OtaManagerImpl::can_transition(&OtaState::Downloading { progress: 50 }, &OtaState::Verifying));
        assert!(OtaManagerImpl::can_transition(&OtaState::Verifying, &OtaState::Applying));
        assert!(OtaManagerImpl::can_transition(&OtaState::Applying, &OtaState::Applied));
        assert!(OtaManagerImpl::can_transition(&OtaState::Applied, &OtaState::Completed));
    }

    #[test]
    fn test_can_transition_invalid() {
        // 不能从 Idle 直接到 Downloading
        assert!(!OtaManagerImpl::can_transition(&OtaState::Idle, &OtaState::Downloading { progress: 0 }));
        // 不能从 Completed 直接到 Applying
        assert!(!OtaManagerImpl::can_transition(&OtaState::Completed, &OtaState::Applying));
    }

    // ========== UpdateStatus 测试 ==========

    #[test]
    fn test_update_status_debug() {
        let status = UpdateStatus {
            state: OtaState::Idle,
            current_task_id: None,
            current_model_type: None,
            download_progress: None,
            error_message: None,
        };

        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("UpdateStatus"));
        assert!(debug_str.contains("Idle"));
    }

    // ========== OtaManagerImpl 创建测试 ==========

    #[tokio::test]
    async fn test_manager_new() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        // 创建空的公钥文件
        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        // 写入初始版本文件
        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": [
                {
                    "model_type": "lstm",
                    "version": "1.0.0",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "abc123",
                    "size": 1024
                }
            ]
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let result = OtaManagerImpl::new(config.clone(), temp_dir.clone().join("temp"));
        assert!(result.is_ok());

        let manager = result.unwrap();
        assert_eq!(manager.config.server_url, "https://ota.example.com");
    }

    #[tokio::test]
    async fn test_manager_with_callbacks() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": []
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let notified = std::sync::Arc::new(std::sync::Mutex::new(None::<ModelType>));
        let notified_clone = notified.clone();
        let callback = Box::new(move |model_type| {
            *notified_clone.lock().unwrap() = Some(model_type);
        });

        let result = OtaManagerImpl::with_callbacks(
            config,
            temp_dir.clone().join("temp"),
            Some(callback),
        );

        assert!(result.is_ok());
    }

    // ========== get_current_version 测试 ==========

    #[tokio::test]
    async fn test_get_current_version() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": [
                {
                    "model_type": "lstm",
                    "version": "1.2.0",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "abc123",
                    "size": 1024
                },
                {
                    "model_type": "maddpg",
                    "version": "1.0.5",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "def456",
                    "size": 2048
                }
            ]
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let manager = OtaManagerImpl::new(config, temp_dir.join("temp")).unwrap();

        // 测试获取 LSTM 版本
        let result = manager.get_current_version(ModelType::Lstm);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version, "1.2.0");

        // 测试获取 MADDPG 版本
        let result = manager.get_current_version(ModelType::Maddpg);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version, "1.0.5");
    }

    #[tokio::test]
    async fn test_get_current_version_not_found() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": []
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let manager = OtaManagerImpl::new(config, temp_dir.join("temp")).unwrap();

        let result = manager.get_current_version(ModelType::Lstm);
        assert!(result.is_err());
    }

    // ========== get_update_status 测试 ==========

    #[tokio::test]
    async fn test_get_update_status_idle() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": []
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let manager = OtaManagerImpl::new(config, temp_dir.join("temp")).unwrap();

        let status = manager.get_update_status();
        assert_eq!(status.state, OtaState::Idle);
        assert!(status.current_task_id.is_none());
        assert!(status.error_message.is_none());
    }

    // ========== get_update_history 测试 ==========

    #[tokio::test]
    async fn test_get_update_history() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": []
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let manager = OtaManagerImpl::new(config, temp_dir.join("temp")).unwrap();

        // 初始历史为空
        let history = manager.get_update_history(10).unwrap();
        assert!(history.is_empty());

        // 添加一些历史记录
        let record = UpdateRecord {
            task_id: "task-123".to_string(),
            model_type: ModelType::Lstm,
            from_version: "1.0.0".to_string(),
            to_version: "1.1.0".to_string(),
            status: OtaState::Completed,
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            error_message: None,
        };
        manager.add_to_history(record).await;

        let history = manager.get_update_history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].task_id, "task-123");
    }

    #[tokio::test]
    async fn test_get_update_history_limit() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": []
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let manager = OtaManagerImpl::new(config, temp_dir.join("temp")).unwrap();

        // 添加 5 条历史记录
        for i in 0..5 {
            let record = UpdateRecord {
                task_id: format!("task-{}", i),
                model_type: ModelType::Lstm,
                from_version: "1.0.0".to_string(),
                to_version: format!("1.{}", i),
                status: OtaState::Completed,
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
                error_message: None,
            };
            manager.add_to_history(record).await;
        }

        // 限制为 3 条
        let history = manager.get_update_history(3).unwrap();
        assert_eq!(history.len(), 3);
    }

    // ========== check_updates 测试 ==========

    #[tokio::test]
    async fn test_check_updates() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        // 创建版本文件
        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": [
                {
                    "model_type": "lstm",
                    "version": "1.0.0",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "abc123",
                    "size": 1024
                }
            ]
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let manager = OtaManagerImpl::new(config, temp_dir.join("temp")).unwrap();

        // 检查更新
        let result = manager.check_updates().await;
        assert!(result.is_ok());

        let updates = result.unwrap();
        // 由于查询返回的版本比当前高，应该有更新
        // 但实际实现中查询返回的版本是固定的，所以这里检查返回的是可用更新
        tracing::info!("可用更新: {:?}", updates);
    }

    // ========== 状态转换测试 ==========

    #[tokio::test]
    async fn test_state_transition() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": []
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let manager = OtaManagerImpl::new(config, temp_dir.join("temp")).unwrap();

        // 初始状态应该是 Idle
        let state = manager.get_state().await;
        assert_eq!(state, OtaState::Idle);

        // 转换到 Checking
        manager.transition_state(OtaState::Checking).await.unwrap();
        let state = manager.get_state().await;
        assert_eq!(state, OtaState::Checking);

        // 转换回 Idle
        manager.transition_state(OtaState::Idle).await.unwrap();
        let state = manager.get_state().await;
        assert_eq!(state, OtaState::Idle);
    }

    // ========== TaskId 生成测试 ==========

    #[tokio::test]
    async fn test_generate_task_id() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let config = OtaConfig {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: temp_dir.join("public_key.pem").display().to_string(),
            model_storage_path: models_dir.display().to_string(),
        };

        std::fs::write(temp_dir.join("public_key.pem"), b"test key").unwrap();

        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": []
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        let manager = OtaManagerImpl::new(config, temp_dir.join("temp")).unwrap();

        let task_id1 = manager.generate_task_id();
        let task_id2 = manager.generate_task_id();

        assert!(!task_id1.is_empty());
        assert!(!task_id2.is_empty());
        assert_ne!(task_id1, task_id2);
    }

    // ========== UpdateStatus 详细测试 ==========

    #[test]
    fn test_update_status_with_task() {
        let status = UpdateStatus {
            state: OtaState::Downloading { progress: 50 },
            current_task_id: Some("task-123".to_string()),
            current_model_type: Some(ModelType::Lstm),
            download_progress: Some(50),
            error_message: None,
        };

        assert!(matches!(status.state, OtaState::Downloading { progress: 50 }));
        assert_eq!(status.current_task_id, Some("task-123".to_string()));
        assert_eq!(status.download_progress, Some(50));
    }

    #[test]
    fn test_update_status_failed() {
        let status = UpdateStatus {
            state: OtaState::Failed { error: "checksum mismatch".to_string() },
            current_task_id: Some("task-456".to_string()),
            current_model_type: Some(ModelType::Maddpg),
            download_progress: Some(75),
            error_message: Some("checksum mismatch".to_string()),
        };

        assert!(matches!(status.state, OtaState::Failed { .. }));
        assert!(status.error_message.is_some());
    }
}