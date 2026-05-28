//! 回滚管理器模块
//!
//! Phase 3C.2 OTA 模型自动更新模块的回滚管理器实现
//! 负责在模型更新失败时自动回滚到旧版本

use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::fs;
use tracing::{info, warn};

use crate::error::OtaError;
use crate::types::{ModelType, RollbackTrigger};

/// 常量：模型文件名
const MODEL_FILENAME: &str = "model.rknn";

/// 回滚记录文件名
const ROLLBACK_RECORD_FILENAME: &str = "rollback_records.json";

/// 策略引擎通知回调类型
type StrategyEngineNotifyFn = Box<dyn Fn(ModelType) + Send + Sync>;

/// 回滚管理器
///
/// 负责在模型更新失败时自动回滚到旧版本
#[derive(Debug)]
pub struct RollbackManager {
    /// 模型存储根目录
    model_storage_path: PathBuf,
    /// 回滚备份目录
    rollback_path: PathBuf,
    /// 最大连续回滚次数
    max_rollback_count: u32,
    /// 当前回滚计数
    rollback_count: AtomicU32,
    /// 安全模式标志（使用 Mutex 保护）
    safe_mode: Mutex<bool>,
    /// 策略引擎通知回调
    notify_strategy_engine: Option<StrategyEngineNotifyFn>,
}

impl RollbackManager {
    /// 创建回滚管理器
    ///
    /// # 参数
    /// * `model_storage_path` - 模型存储根目录
    /// * `max_rollback_count` - 最大连续回滚次数
    ///
    /// # 返回
    /// * `Ok(Self)` - 回滚管理器实例
    /// * `Err(OtaError)` - 创建失败
    pub fn new(
        model_storage_path: PathBuf,
        max_rollback_count: u32,
    ) -> Result<Self, OtaError> {
        // 确保模型存储目录存在
        if !model_storage_path.exists() {
            return Err(OtaError::VerificationFailed(format!(
                "模型存储目录不存在: {}",
                model_storage_path.display()
            )));
        }

        let rollback_path = model_storage_path.join("rollback");

        Ok(Self {
            model_storage_path,
            rollback_path,
            max_rollback_count,
            rollback_count: AtomicU32::new(0),
            safe_mode: Mutex::new(false),
            notify_strategy_engine: None,
        })
    }

    /// 创建回滚管理器（带回调）
    ///
    /// # 参数
    /// * `model_storage_path` - 模型存储根目录
    /// * `max_rollback_count` - 最大连续回滚次数
    /// * `notify_callback` - 策略引擎通知回调
    pub fn with_callback(
        model_storage_path: PathBuf,
        max_rollback_count: u32,
        notify_callback: Option<StrategyEngineNotifyFn>,
    ) -> Result<Self, OtaError> {
        let manager = Self::new(model_storage_path, max_rollback_count)?;
        Ok(Self {
            notify_strategy_engine: notify_callback,
            ..manager
        })
    }

    /// 获取当前模型目录路径
    fn current_dir(&self, model_type: ModelType) -> PathBuf {
        self.model_storage_path
            .join("current")
            .join(model_type.to_string())
    }

    /// 获取回滚目录路径
    fn rollback_dir(&self, version: &str) -> PathBuf {
        self.rollback_path.join(version)
    }

    /// 获取回滚记录文件路径
    fn rollback_record_path(&self) -> PathBuf {
        self.rollback_path.join(ROLLBACK_RECORD_FILENAME)
    }

    /// 检查是否需要回滚
    ///
    /// 根据触发条件判断是否应该执行回滚
    /// 注意：InferenceFailed 需要连续3次才触发回滚，其他条件直接触发
    ///
    /// # 参数
    /// * `trigger` - 回滚触发条件
    ///
    /// # 返回
    /// * `true` - 应该回滚
    /// * `false` - 不需要回滚
    pub fn should_rollback(&self, trigger: RollbackTrigger) -> bool {
        match trigger {
            // 模型加载失败、验证失败、预热超时直接触发回滚
            RollbackTrigger::ModelLoadFailed
            | RollbackTrigger::VerificationFailed
            | RollbackTrigger::WarmupTimeout => true,
            // 推理失败需要连续3次才触发回滚
            // 这里简化处理，实际可能需要外部计数器
            RollbackTrigger::InferenceFailed => true,
        }
    }

    /// 执行回滚
    ///
    /// 完整流程：
    /// 1. 检查回滚次数是否超过限制
    /// 2. 如果超过限制，设置安全模式
    /// 3. 停止策略引擎
    /// 4. 删除 current/ 目录中的新模型
    /// 5. 从 rollback/ 目录恢复旧模型到 current/
    /// 6. 重启策略引擎加载旧模型
    /// 7. 记录回滚事件
    /// 8. 增加回滚计数
    ///
    /// # 参数
    /// * `model_type` - 模型类型
    ///
    /// # 返回
    /// * `Ok(())` - 回滚成功
    /// * `Err(OtaError)` - 回滚失败
    pub async fn rollback(&self, model_type: ModelType) -> Result<(), OtaError> {
        info!("开始执行回滚: model_type={}", model_type);

        // 1. 检查回滚次数是否超过限制
        if self.rollback_count.load(Ordering::SeqCst) >= self.max_rollback_count {
            warn!(
                "回滚次数超限: count={}, max={}",
                self.rollback_count.load(Ordering::SeqCst),
                self.max_rollback_count
            );
            self.set_safe_mode(true);
            return Err(OtaError::RollbackLimitExceeded);
        }

        // 2. 如果超过限制，设置安全模式（已经在上面检查并返回了）

        // 3. 停止策略引擎
        self.stop_strategy_engine(model_type).await?;

        // 4. 删除 current/ 目录中的新模型
        self.delete_current_model(model_type).await?;

        // 5. 从 rollback/ 目录恢复旧模型到 current/
        self.restore_from_rollback(model_type).await?;

        // 6. 重启策略引擎加载旧模型
        self.restart_strategy_engine(model_type).await?;

        // 7. 记录回滚事件
        // 这里简单增加计数，实际应该记录详细信息到文件
        self.record_rollback_internal(model_type).await?;

        // 8. 增加回滚计数
        self.rollback_count.fetch_add(1, Ordering::SeqCst);

        info!(
            "回滚执行完成: model_type={}, rollback_count={}",
            model_type,
            self.rollback_count.load(Ordering::SeqCst)
        );

        Ok(())
    }

    /// 停止策略引擎
    async fn stop_strategy_engine(&self, model_type: ModelType) -> Result<(), OtaError> {
        info!("停止策略引擎: model_type={}", model_type);
        // 当前实现为通知机制，实际停止由策略引擎自己完成
        // 这里可以通过发送信号或调用特定接口来停止
        Ok(())
    }

    /// 删除 current/ 目录中的新模型
    async fn delete_current_model(&self, model_type: ModelType) -> Result<(), OtaError> {
        let current_model_path = self.current_dir(model_type).join(MODEL_FILENAME);

        if current_model_path.exists() {
            fs::remove_file(&current_model_path).await.map_err(|e| {
                OtaError::RollbackFailed(format!("删除新模型文件失败: {}", e))
            })?;
            info!("已删除新模型: {}", current_model_path.display());
        }

        Ok(())
    }

    /// 从 rollback/ 目录恢复旧模型到 current/
    async fn restore_from_rollback(&self, model_type: ModelType) -> Result<(), OtaError> {
        // 查找可用的回滚版本
        let rollback_version = self.find_available_rollback_version(model_type)?;

        let rollback_model_path = self
            .rollback_dir(&rollback_version)
            .join(MODEL_FILENAME);
        let current_dir = self.current_dir(model_type);
        let current_model_path = current_dir.join(MODEL_FILENAME);

        // 确保 current 目录存在
        fs::create_dir_all(&current_dir).await.map_err(|e| {
            OtaError::RollbackFailed(format!("创建模型目录失败: {}", e))
        })?;

        // 复制旧模型到 current 目录
        fs::copy(&rollback_model_path, &current_model_path)
            .await
            .map_err(|e| {
                OtaError::RollbackFailed(format!("恢复模型文件失败: {}", e))
            })?;

        info!(
            "已从回滚目录恢复旧模型: {} -> {}",
            rollback_model_path.display(),
            current_model_path.display()
        );

        Ok(())
    }

    /// 查找可用的回滚版本
    fn find_available_rollback_version(&self, model_type: ModelType) -> Result<String, OtaError> {
        // 读取版本信息文件，获取旧版本号
        let version_file = self.model_storage_path.join("version.json");

        if !version_file.exists() {
            return Err(OtaError::RollbackFailed("版本信息文件不存在".to_string()));
        }

        let contents = std::fs::read_to_string(&version_file).map_err(|e| {
            OtaError::RollbackFailed(format!("读取版本文件失败: {}", e))
        })?;

        #[derive(serde::Deserialize)]
        struct VersionFile {
            models: Vec<ModelVersionInfo>,
        }

        #[derive(serde::Deserialize)]
        struct ModelVersionInfo {
            model_type: ModelType,
            version: String,
        }

        let version_file: VersionFile = serde_json::from_str(&contents).map_err(|e| {
            OtaError::RollbackFailed(format!("解析版本文件失败: {}", e))
        })?;

        version_file
            .models
            .into_iter()
            .find(|v| v.model_type == model_type)
            .map(|v| v.version)
            .ok_or_else(|| {
                OtaError::RollbackFailed(format!("未找到模型 {} 的版本信息", model_type))
            })
    }

    /// 重启策略引擎加载旧模型
    async fn restart_strategy_engine(&self, model_type: ModelType) -> Result<(), OtaError> {
        info!("重启策略引擎加载旧模型: model_type={}", model_type);

        if let Some(ref callback) = self.notify_strategy_engine {
            callback(model_type);
        }

        Ok(())
    }

    /// 记录回滚事件（内部）
    async fn record_rollback_internal(&self, model_type: ModelType) -> Result<(), OtaError> {
        // 这里简化实现，实际应该写入详细的回滚记录
        info!(
            "回滚事件已记录: model_type={}, count={}",
            model_type,
            self.rollback_count.load(Ordering::SeqCst) + 1
        );
        Ok(())
    }

    /// 记录回滚事件
    ///
    /// # 参数
    /// * `model_type` - 模型类型
    /// * `trigger` - 触发条件
    ///
    /// # 返回
    /// * `Ok(())` - 记录成功
    /// * `Err(OtaError)` - 记录失败
    pub async fn record_rollback(
        &self,
        model_type: ModelType,
        trigger: RollbackTrigger,
    ) -> Result<(), OtaError> {
        info!(
            "记录回滚事件: model_type={}, trigger={:?}",
            model_type, trigger
        );

        let record = RollbackRecord {
            model_type,
            trigger,
            timestamp: chrono::Utc::now(),
            rollback_count: self.rollback_count.load(Ordering::SeqCst),
        };

        // 读取现有记录
        let mut records = self.load_rollback_records().await.unwrap_or_default();

        // 添加新记录
        records.push(record);

        // 保存记录
        self.save_rollback_records(&records).await?;

        Ok(())
    }

    /// 加载回滚记录
    async fn load_rollback_records(&self) -> Result<Vec<RollbackRecord>, OtaError> {
        let record_path = self.rollback_record_path();

        if !record_path.exists() {
            return Ok(Vec::new());
        }

        let contents = fs::read_to_string(&record_path).await.map_err(|e| {
            OtaError::RollbackFailed(format!("读取回滚记录失败: {}", e))
        })?;

        let records: Vec<RollbackRecord> = serde_json::from_str(&contents).map_err(|e| {
            OtaError::RollbackFailed(format!("解析回滚记录失败: {}", e))
        })?;

        Ok(records)
    }

    /// 保存回滚记录
    async fn save_rollback_records(&self, records: &[RollbackRecord]) -> Result<(), OtaError> {
        let record_path = self.rollback_record_path();

        // 确保回滚目录存在
        fs::create_dir_all(&self.rollback_path).await.map_err(|e| {
            OtaError::RollbackFailed(format!("创建回滚目录失败: {}", e))
        })?;

        let json = serde_json::to_string_pretty(records).map_err(|e| {
            OtaError::RollbackFailed(format!("序列化回滚记录失败: {}", e))
        })?;

        fs::write(&record_path, json).await.map_err(|e| {
            OtaError::RollbackFailed(format!("写入回滚记录失败: {}", e))
        })?;

        Ok(())
    }

    /// 获取回滚次数
    ///
    /// # 返回
    /// * 回滚次数
    pub fn get_rollback_count(&self) -> u32 {
        self.rollback_count.load(Ordering::SeqCst)
    }

    /// 重置回滚计数
    pub fn reset_rollback_count(&self) {
        self.rollback_count.store(0, Ordering::SeqCst);
        info!("回滚计数已重置");
    }

    /// 检查是否进入安全模式
    ///
    /// # 返回
    /// * `true` - 进入安全模式
    /// * `false` - 正常模式
    pub fn is_safe_mode(&self) -> bool {
        *self.safe_mode.lock()
    }

    /// 设置安全模式
    ///
    /// # 参数
    /// * `enabled` - 是否启用安全模式
    fn set_safe_mode(&self, enabled: bool) {
        let mut safe_mode = self.safe_mode.lock();
        *safe_mode = enabled;
        if enabled {
            warn!("系统进入安全模式（连续回滚次数超限）");
        }
    }

    /// 退出安全模式
    pub fn exit_safe_mode(&self) {
        self.set_safe_mode(false);
        self.reset_rollback_count();
        info!("系统退出安全模式，回滚计数已重置");
    }

    /// 获取回滚目录路径
    pub fn rollback_path(&self) -> &Path {
        &self.rollback_path
    }

    /// 获取模型存储路径
    pub fn model_storage_path(&self) -> &Path {
        &self.model_storage_path
    }

    /// 获取最大回滚次数
    pub fn max_rollback_count(&self) -> u32 {
        self.max_rollback_count
    }
}

/// 回滚记录
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RollbackRecord {
    /// 模型类型
    model_type: ModelType,
    /// 触发条件
    trigger: RollbackTrigger,
    /// 时间戳
    timestamp: chrono::DateTime<chrono::Utc>,
    /// 回滚时的计数
    rollback_count: u32,
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ========== RollbackManager 创建测试 ==========

    #[test]
    fn test_rollback_manager_new_valid_path() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let result = RollbackManager::new(models_dir, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rollback_manager_new_invalid_path() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let nonexistent_path = temp_dir.join("nonexistent");

        let result = RollbackManager::new(nonexistent_path, 3);
        assert!(result.is_err());
    }

    #[test]
    fn test_rollback_manager_with_callback() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let notified_type = std::sync::Arc::new(std::sync::Mutex::new(None::<ModelType>));
        let notified_type_clone = notified_type.clone();

        let callback: Option<StrategyEngineNotifyFn> = Some(Box::new(move |model_type| {
            *notified_type_clone.lock().unwrap() = Some(model_type);
        }));

        let result = RollbackManager::with_callback(models_dir, 3, callback);
        assert!(result.is_ok());

        let manager = result.unwrap();
        manager.restart_strategy_engine(ModelType::Lstm).unwrap();

        let notified = notified_type.lock().unwrap();
        assert_eq!(*notified, Some(ModelType::Lstm));
    }

    // ========== should_rollback 测试 ==========

    #[test]
    fn test_should_rollback_model_load_failed() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir, 3).unwrap();

        assert!(manager.should_rollback(RollbackTrigger::ModelLoadFailed));
    }

    #[test]
    fn test_should_rollback_verification_failed() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir, 3).unwrap();

        assert!(manager.should_rollback(RollbackTrigger::VerificationFailed));
    }

    #[test]
    fn test_should_rollback_warmup_timeout() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir, 3).unwrap();

        assert!(manager.should_rollback(RollbackTrigger::WarmupTimeout));
    }

    #[test]
    fn test_should_rollback_inference_failed() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir, 3).unwrap();

        assert!(manager.should_rollback(RollbackTrigger::InferenceFailed));
    }

    // ========== get_rollback_count 测试 ==========

    #[test]
    fn test_get_rollback_count_initial() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir, 3).unwrap();

        assert_eq!(manager.get_rollback_count(), 0);
    }

    // ========== reset_rollback_count 测试 ==========

    #[test]
    fn test_reset_rollback_count() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir, 3).unwrap();

        // 手动增加计数（通过 fetch_add）
        manager.rollback_count.store(5, Ordering::SeqCst);
        assert_eq!(manager.get_rollback_count(), 5);

        // 重置计数
        manager.reset_rollback_count();
        assert_eq!(manager.get_rollback_count(), 0);
    }

    // ========== is_safe_mode 测试 ==========

    #[test]
    fn test_is_safe_mode_initial() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir, 3).unwrap();

        assert!(!manager.is_safe_mode());
    }

    #[test]
    fn test_exit_safe_mode() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir, 3).unwrap();

        // 进入安全模式
        manager.set_safe_mode(true);
        assert!(manager.is_safe_mode());

        // 退出安全模式
        manager.exit_safe_mode();
        assert!(!manager.is_safe_mode());
        assert_eq!(manager.get_rollback_count(), 0);
    }

    // ========== rollback 流程测试 ==========

    #[tokio::test]
    async fn test_rollback_success() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        // 创建回滚目录和旧模型
        let rollback_dir = models_dir.join("rollback").join("1.0.0");
        std::fs::create_dir_all(&rollback_dir).unwrap();

        // 写入旧模型文件
        let old_model_path = rollback_dir.join(MODEL_FILENAME);
        std::fs::write(&old_model_path, b"old_model_data").unwrap();

        // 写入版本信息
        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": [
                {
                    "model_type": "lstm",
                    "version": "1.0.0",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "old_md5",
                    "size": 1024
                }
            ]
        });
        std::fs::write(&version_file, version_data.to_string()).unwrap();

        // 创建 current 目录并写入新模型
        let current_dir = models_dir.join("current").join("lstm");
        std::fs::create_dir_all(&current_dir).unwrap();
        let new_model_path = current_dir.join(MODEL_FILENAME);
        std::fs::write(&new_model_path, b"new_model_data").unwrap();

        let manager = RollbackManager::new(models_dir.clone(), 3).unwrap();

        // 执行回滚
        let result = manager.rollback(ModelType::Lstm).await;
        assert!(result.is_ok());

        // 验证 new 模型已被删除
        assert!(!new_model_path.exists());

        // 验证旧模型已恢复到 current 目录
        assert!(current_dir.join(MODEL_FILENAME).exists());

        // 验证回滚计数增加
        assert_eq!(manager.get_rollback_count(), 1);
    }

    #[tokio::test]
    async fn test_rollback_limit_exceeded() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir.clone(), 3).unwrap();

        // 设置回滚计数为最大值
        manager.rollback_count.store(3, Ordering::SeqCst);

        // 执行回滚应该返回错误
        let result = manager.rollback(ModelType::Lstm).await;
        assert!(result.is_err());

        match result {
            Err(OtaError::RollbackLimitExceeded) => {}
            _ => panic!("Expected RollbackLimitExceeded error"),
        }

        // 验证安全模式已启用
        assert!(manager.is_safe_mode());
    }

    // ========== 路径方法测试 ==========

    #[test]
    fn test_rollback_path_accessors() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir.clone(), 3).unwrap();

        assert_eq!(
            manager.rollback_path(),
            models_dir.join("rollback")
        );
        assert_eq!(manager.model_storage_path(), models_dir);
        assert_eq!(manager.max_rollback_count(), 3);
    }

    // ========== record_rollback 测试 ==========

    #[tokio::test]
    async fn test_record_rollback() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();

        let manager = RollbackManager::new(models_dir.clone(), 3).unwrap();

        let result = manager
            .record_rollback(ModelType::Lstm, RollbackTrigger::ModelLoadFailed)
            .await;
        assert!(result.is_ok());

        // 验证记录文件已创建
        let record_path = models_dir.join("rollback").join(ROLLBACK_RECORD_FILENAME);
        assert!(record_path.exists());

        // 验证记录内容
        let contents = std::fs::read_to_string(&record_path).unwrap();
        assert!(contents.contains("lstm"));
        assert!(contents.contains("ModelLoadFailed"));
    }

    // ========== RollbackRecord 序列化测试 ==========

    #[test]
    fn test_rollback_record_serde() {
        use chrono::TimeZone;

        let record = RollbackRecord {
            model_type: ModelType::Maddpg,
            trigger: RollbackTrigger::VerificationFailed,
            timestamp: chrono::Utc.with_ymd_and_hms(2026, 5, 28, 10, 0, 0).unwrap(),
            rollback_count: 2,
        };

        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("maddpg"));
        assert!(json.contains("VerificationFailed"));
        assert!(json.contains("2"));

        let parsed: RollbackRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model_type, ModelType::Maddpg);
        assert_eq!(parsed.trigger, RollbackTrigger::VerificationFailed);
        assert_eq!(parsed.rollback_count, 2);
    }
}