//! 模型应用器模块
//!
//! Phase 3C.2 OTA 模型自动更新模块的模型应用器实现
//! 负责将下载并验证通过的模型文件应用到生产环境

use once_cell::sync::Lazy;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::io::Read;
use tar::Archive;
use tokio::fs::{self, File};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::OtaError;
use crate::types::{ModelType, ModelVersion};
use crate::verifier::Verifier;

/// 常量：模型文件名
const MODEL_FILENAME: &str = "model.rknn";

/// 缓存的正则表达式：用于从路径提取版本号
static VERSION_PATTERN: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"v?(\d+\.\d+\.\d+)").unwrap());

/// `fsync` 一个目录（`rename` 的持久化面）。
///
/// Windows 不允许以只读方式打开目录 ⇒ 该平台返回 `Err`，由调用方**降级为告警**
/// （见 [`ModelApplicator::copy_to_current`] 第 ④ 步）。
fn fsync_dir(dir: &Path) -> std::io::Result<()> {
    let d = std::fs::File::open(dir)?;
    d.sync_all()
}

/// 策略引擎通知回调类型
type StrategyEngineNotifyFn = Box<dyn Fn(ModelType) + Send + Sync>;

/// 模型应用器
///
/// 负责将下载并验证通过的模型文件应用到生产环境
pub struct ModelApplicator {
    /// 模型存储根目录
    model_storage_path: PathBuf,
    /// 回滚备份目录
    rollback_path: PathBuf,
    /// 验证器
    verifier: Arc<Verifier>,
    /// 策略引擎通知回调
    notify_strategy_engine: Option<StrategyEngineNotifyFn>,
}

impl std::fmt::Debug for ModelApplicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelApplicator")
            .field("model_storage_path", &self.model_storage_path)
            .field("rollback_path", &self.rollback_path)
            .field("verifier", &self.verifier)
            .field(
                "notify_strategy_engine",
                &self.notify_strategy_engine.as_ref().map(|_| "Fn(...)"),
            )
            .finish()
    }
}

impl ModelApplicator {
    /// 创建新的模型应用器
    ///
    /// # 参数
    /// * `model_storage_path` - 模型存储根目录
    /// * `verifier` - 验证器
    /// * `notify_callback` - 策略引擎通知回调（可选）
    pub fn new(
        model_storage_path: PathBuf,
        verifier: Arc<Verifier>,
        notify_callback: Option<StrategyEngineNotifyFn>,
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
            verifier,
            notify_strategy_engine: notify_callback,
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

    /// 获取版本信息文件路径
    fn version_file_path(&self) -> PathBuf {
        self.model_storage_path.join("version.json")
    }

    /// 应用模型更新
    ///
    /// 完整流程：
    /// 1. backup_current_model - 备份旧模型到 rollback/{version}/
    /// 2. decompress_if_needed - 解压更新包（如需要）
    /// 3. verify - 验证新模型（完整性、签名、格式、平台兼容性）
    /// 4. copy_to_current - 复制新模型到 current/ 目录
    /// 5. warmup_model - 执行一次推理预热
    /// 6. notify_strategy_engine - 通知策略引擎加载新模型
    /// 7. calculate_checksum - 计算新模型校验和
    /// 8. 返回新版本信息
    ///
    /// # 参数
    /// * `model_type` - 模型类型
    /// * `update_package` - 更新包路径
    /// * `expected_hash` - 期望的 SHA-256 哈希值
    pub async fn apply(
        &self,
        model_type: ModelType,
        update_package: &Path,
        expected_hash: &str,
    ) -> Result<ModelVersion, OtaError> {
        tracing::info!(
            "开始应用模型更新: model_type={}, package={}",
            model_type,
            update_package.display()
        );

        // 1. 备份当前模型
        let rollback_dir = self.backup_current_model(model_type).await?;
        tracing::info!("已备份旧模型到: {}", rollback_dir.display());

        // 2. 解压更新包
        let decompressed_path = self.decompress_if_needed(update_package).await?;
        tracing::info!("已解压更新包到: {}", decompressed_path.display());

        // 3. 验证新模型
        self.verifier
            .verify(&decompressed_path, expected_hash, &[])
            .await?;

        // 4. 复制新模型到 current 目录
        self.copy_to_current(model_type, &decompressed_path).await?;

        // 5. 预热模型
        self.warmup_model(model_type).await?;

        // 6. 通知策略引擎
        self.notify_strategy_engine(model_type);

        // 7. 计算新模型校验和
        let current_model_path = self.current_dir(model_type).join(MODEL_FILENAME);
        let checksum = self.calculate_checksum(&current_model_path).await?;

        // 8. 获取新版本信息
        let metadata = fs::metadata(&current_model_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("获取模型文件元数据失败: {}", e)))?;

        let new_version = ModelVersion {
            model_type,
            version: extract_version_from_path(update_package)?,
            updated_at: Utc::now(),
            md5: checksum,
            size: metadata.len(),
        };

        // 更新版本信息文件
        self.save_version_info(&new_version).await?;

        tracing::info!(
            "模型更新应用成功: model_type={}, version={}",
            model_type,
            new_version.version
        );

        Ok(new_version)
    }

    /// 备份当前模型
    async fn backup_current_model(&self, model_type: ModelType) -> Result<PathBuf, OtaError> {
        let current_dir = self.current_dir(model_type);
        let current_model_path = current_dir.join(MODEL_FILENAME);

        // 获取当前版本
        let current_version = self.get_current_version(model_type).await?;
        let rollback_dir = self.rollback_dir(&current_version.version);

        // 创建回滚目录
        fs::create_dir_all(&rollback_dir)
            .await
            .map_err(|e| OtaError::RollbackFailed(format!("创建回滚目录失败: {}", e)))?;

        // 如果当前模型存在，复制到回滚目录
        if current_model_path.exists() {
            let backup_path = rollback_dir.join(MODEL_FILENAME);
            fs::copy(&current_model_path, &backup_path)
                .await
                .map_err(|e| OtaError::RollbackFailed(format!("备份模型文件失败: {}", e)))?;

            // 同时备份 version.json
            let version_file = self.version_file_path();
            if version_file.exists() {
                let backup_version_file = rollback_dir.join("version.json");
                let _ = fs::copy(&version_file, &backup_version_file).await;
            }

            tracing::info!(
                "已备份模型 {} v{} 到 {}",
                model_type,
                current_version.version,
                backup_path.display()
            );
        }

        Ok(rollback_dir)
    }

    /// 解压更新包（如需要）
    ///
    /// 支持的格式：gz, tgz, zip, xz
    async fn decompress_if_needed(&self, package: &Path) -> Result<PathBuf, OtaError> {
        let extension = package
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let _stem = package.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        // 检查是否需要解压
        let needs_decompress = matches!(extension.as_str(), "gz" | "tgz" | "zip" | "xz");

        if !needs_decompress {
            // 不需要解压，直接返回原文件路径
            return Ok(package.to_path_buf());
        }

        // 创建临时解压目录
        let temp_dir = self
            .model_storage_path
            .join("update")
            .join("temp_decompress");
        fs::create_dir_all(&temp_dir)
            .await
            .map_err(|e| OtaError::DecompressionFailed(format!("创建临时解压目录失败: {}", e)))?;

        match extension.as_str() {
            "gz" => {
                // 处理 .gz 单文件
                self.decompress_gzip(package, &temp_dir).await?;
            }
            "tgz" => {
                // 处理 .tar.gz
                self.decompress_tar_gz(package, &temp_dir).await?;
            }
            "zip" => {
                // 处理 .zip
                self.decompress_zip(package, &temp_dir).await?;
            }
            "xz" => {
                // 处理 .xz 单文件
                self.decompress_xz(package, &temp_dir).await?;
            }
            _ => {
                return Err(OtaError::DecompressionFailed(format!(
                    "不支持的压缩格式: {}",
                    extension
                )));
            }
        }

        // 查找解压后的模型文件
        let model_file = self.find_model_file(&temp_dir)?;

        Ok(model_file)
    }

    /// 解压 gzip 文件
    async fn decompress_gzip(&self, package: &Path, temp_dir: &Path) -> Result<(), OtaError> {
        let contents = fs::read(package)
            .await
            .map_err(|e| OtaError::DecompressionFailed(format!("读取压缩文件失败: {}", e)))?;

        let decompressed = tokio::task::spawn_blocking(move || {
            let mut decoder = GzDecoder::new(contents.as_slice());
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).map(|_| out)
        })
        .await
        .map_err(|e| OtaError::DecompressionFailed(e.to_string()))?
        .map_err(|e| OtaError::DecompressionFailed(format!("解压 gzip 失败: {}", e)))?;

        let output_path = temp_dir.join(MODEL_FILENAME);
        let mut output_file = File::create(&output_path)
            .await
            .map_err(|e| OtaError::DecompressionFailed(format!("创建解压文件失败: {}", e)))?;

        output_file
            .write_all(&decompressed)
            .await
            .map_err(|e| OtaError::DecompressionFailed(format!("写入解压数据失败: {}", e)))?;

        Ok(())
    }

    /// 解压 tar.gz 文件
    async fn decompress_tar_gz(&self, package: &Path, temp_dir: &Path) -> Result<(), OtaError> {
        let contents = fs::read(package)
            .await
            .map_err(|e| OtaError::DecompressionFailed(format!("读取压缩文件失败: {}", e)))?;

        let temp_dir = temp_dir.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let mut decoder = GzDecoder::new(contents.as_slice());
            let mut archive = Archive::new(&mut decoder);
            archive.unpack(&temp_dir)
        })
        .await
        .map_err(|e| OtaError::DecompressionFailed(e.to_string()))?
        .map_err(|e| OtaError::DecompressionFailed(format!("解压 tar.gz 失败: {}", e)))?;

        Ok(())
    }

    /// 解压 zip 文件
    async fn decompress_zip(&self, package: &Path, temp_dir: &Path) -> Result<(), OtaError> {
        let file = File::open(package)
            .await
            .map_err(|e| OtaError::DecompressionFailed(format!("打开压缩文件失败: {}", e)))?;

        let std_file = file.into_std().await;

        let temp_dir = temp_dir.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let mut archive = zip::ZipArchive::new(std_file)
                .map_err(|e| OtaError::DecompressionFailed(format!("解析 zip 文件失败: {}", e)))?;
            archive
                .extract(&temp_dir)
                .map_err(|e| OtaError::DecompressionFailed(format!("解压 zip 失败: {}", e)))?;
            Ok::<_, OtaError>(())
        })
        .await
        .map_err(|e| OtaError::DecompressionFailed(e.to_string()))?
    }

    /// 解压 xz 文件
    async fn decompress_xz(&self, package: &Path, temp_dir: &Path) -> Result<(), OtaError> {
        let contents = fs::read(package)
            .await
            .map_err(|e| OtaError::DecompressionFailed(format!("读取 xz 文件失败: {}", e)))?;

        let decompressed = tokio::task::spawn_blocking(move || {
            let mut output = Vec::new();
            lzma_rs::xz_decompress(&mut std::io::Cursor::new(&contents), &mut output)
                .map(|_| output)
                .map_err(|e| OtaError::DecompressionFailed(format!("解压 xz 失败: {}", e)))
        })
        .await
        .map_err(|e| OtaError::DecompressionFailed(e.to_string()))??;

        let output_path = temp_dir.join(MODEL_FILENAME);
        fs::write(&output_path, decompressed)
            .await
            .map_err(|e| OtaError::DecompressionFailed(format!("写入解压数据失败: {}", e)))?;

        Ok(())
    }

    /// 查找解压目录中的模型文件
    fn find_model_file(&self, dir: &Path) -> Result<PathBuf, OtaError> {
        // 递归查找 .rknn 文件
        self.find_file_by_extension(dir, "rknn")
            .ok_or_else(|| OtaError::DecompressionFailed("未找到模型文件 (.rknn)".to_string()))
    }

    /// 递归查找指定扩展名的文件
    fn find_file_by_extension(&self, dir: &Path, extension: &str) -> Option<PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = self.find_file_by_extension(&path, extension) {
                    return Some(found);
                }
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case(extension) {
                    return Some(path);
                }
            }
        }
        None
    }

    /// 把新模型**原子地**替换进 `current/`
    ///
    /// 顺序：**同目录**临时文件 → 内容 `fsync` → `rename` 覆盖 → 目录 `fsync`。
    ///
    /// # 为什么不能"先删后拷"（D-17）
    /// 旧实现是 `remove_file(dest)` → `fs::copy(source, dest)`：两步之间进程崩溃 / 复制失败
    /// ⇒ `current/` 里**没有模型**，且没有任何自动恢复（策略引擎与 AI 侧下一次加载直接失败）。
    /// `rename` 在**同一文件系统内是原子的**：`dest` 任何时刻要么是旧模型、要么是完整新模型。
    /// 判别力用例：`failed_replacement_keeps_the_old_model`（复制失败后旧模型必须原样在位）。
    async fn copy_to_current(&self, model_type: ModelType, source: &Path) -> Result<(), OtaError> {
        let current_dir = self.current_dir(model_type);

        // 确保 current 目录存在
        fs::create_dir_all(&current_dir)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("创建模型目录失败: {}", e)))?;

        let dest_path = current_dir.join(MODEL_FILENAME);
        // 临时文件与目标**同目录**（跨文件系统的 rename 会退化成"拷贝+删除"，不再原子）。
        let tmp_path = current_dir.join(format!(".{MODEL_FILENAME}.tmp-{}", uuid::Uuid::new_v4()));

        // ① 暂存 + ② 内容 fsync：失败即清理临时文件并上抛（**目标文件一个字节都没动**）
        if let Err(e) = self.stage_and_sync(source, &tmp_path).await {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(e);
        }

        // ③ 原子替换
        if let Err(e) = fs::rename(&tmp_path, &dest_path).await {
            let _ = fs::remove_file(&tmp_path).await;
            return Err(OtaError::VerificationFailed(format!(
                "原子替换模型文件失败（旧模型仍应在位）: {}",
                e
            )));
        }

        // ④ 目录 fsync：让 rename 本身也持久。失败只告警 —— 内容已 fsync，
        //    最坏情形是断电后丢"目录项指向新文件"这一步（旧模型仍在，属可接受降级）。
        if let Err(e) = fsync_dir(&current_dir) {
            tracing::warn!("模型目录 fsync 失败（rename 可能未持久到介质）: {}", e);
        }

        tracing::info!("已原子替换模型到: {}", dest_path.display());

        Ok(())
    }

    /// 暂存到临时文件并把**内容**刷到介质（`rename` 之前完成，见 [`Self::copy_to_current`]）。
    async fn stage_and_sync(&self, source: &Path, tmp_path: &Path) -> Result<(), OtaError> {
        fs::copy(source, tmp_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("复制模型文件失败: {}", e)))?;
        // ⚠️ 必须**带写权限**打开再 fsync：Windows 的 `FlushFileBuffers` 要求句柄可写，
        // 只读句柄会 `ERROR_ACCESS_DENIED`（os error 5）—— 而"刚写进去的字节要刷盘"这件事
        // 本来就该在写句柄上做。
        let staged = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(tmp_path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("打开暂存模型失败: {}", e)))?;
        staged
            .sync_all()
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("暂存模型 fsync 失败: {}", e)))
    }

    /// 预热模型（执行一次推理）
    ///
    /// 调用 ai-engine 的 RKNN Runtime 进行模型预热
    async fn warmup_model(&self, model_type: ModelType) -> Result<(), OtaError> {
        let model_path = self.current_dir(model_type).join(MODEL_FILENAME);

        if !model_path.exists() {
            tracing::warn!("模型文件不存在，跳过预热: {}", model_path.display());
            return Ok(());
        }

        // RknnRuntime 是 FFI stub，当前 Phase 3C.2 为占位实现
        // 实际 RKNN Runtime 集成在 Phase 4 完成
        tracing::info!("模型预热占位: RknnRuntime FFI 待 Phase 4 实现");

        // 验证模型文件存在且可读，作为预热成功的替代
        tokio::fs::metadata(&model_path)
            .await
            .map_err(|e| OtaError::ModelLoadFailed(format!("预热检查失败: {}", e)))?;

        Ok(())
    }

    /// 创建预热输入 tensor
    ///
    /// 根据模型类型生成合适的输入数据
    #[allow(dead_code)]
    async fn create_warmup_input(&self, model_type: ModelType) -> Result<Vec<f32>, OtaError> {
        // 不同模型类型有不同的输入形状
        // 这里使用模型类型的特征维度构造输入
        let features = match model_type {
            ModelType::Lstm => 64,   // LSTM: seq_len=1, features=64
            ModelType::Maddpg => 32, // MADDPG: state_dim=32
        };

        // 填充默认值（实际应使用典型输入）
        Ok(vec![0.0f32; features])
    }

    /// 通知策略引擎加载新模型
    fn notify_strategy_engine(&self, model_type: ModelType) {
        tracing::info!("通知策略引擎加载新模型: {}", model_type);

        if let Some(ref callback) = self.notify_strategy_engine {
            callback(model_type);
        }
    }

    /// 计算文件校验和（SHA-256）
    async fn calculate_checksum(&self, path: &Path) -> Result<String, OtaError> {
        let mut file = File::open(path)
            .await
            .map_err(|e| OtaError::VerificationFailed(format!("打开文件失败: {}", e)))?;

        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 8192];

        loop {
            let bytes_read = file
                .read(&mut buffer)
                .await
                .map_err(|e| OtaError::VerificationFailed(format!("读取文件失败: {}", e)))?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let result = hasher.finalize();

        // 转换为十六进制字符串
        Ok(format!("{:x}", result))
    }

    /// 获取当前模型版本
    pub async fn get_current_version(
        &self,
        model_type: ModelType,
    ) -> Result<ModelVersion, OtaError> {
        let version_file = self.version_file_path();

        if !version_file.exists() {
            return Err(OtaError::VersionQueryFailed(
                "版本信息文件不存在".to_string(),
            ));
        }

        let contents = fs::read_to_string(&version_file)
            .await
            .map_err(|e| OtaError::VersionQueryFailed(format!("读取版本文件失败: {}", e)))?;

        // 解析 JSON，查找对应模型类型的版本
        #[derive(serde::Deserialize)]
        struct VersionFile {
            models: Vec<ModelVersion>,
        }

        let version_file: VersionFile = serde_json::from_str(&contents)
            .map_err(|e| OtaError::VersionQueryFailed(format!("解析版本文件失败: {}", e)))?;

        version_file
            .models
            .into_iter()
            .find(|v| v.model_type == model_type)
            .ok_or_else(|| {
                OtaError::VersionQueryFailed(format!("未找到模型 {} 的版本信息", model_type))
            })
    }

    /// 保存版本信息
    async fn save_version_info(&self, version: &ModelVersion) -> Result<(), OtaError> {
        let version_file = self.version_file_path();

        #[derive(serde::Deserialize, serde::Serialize)]
        struct VersionFile {
            models: Vec<ModelVersion>,
        }

        // 读取现有版本信息
        let mut version_file_data = if version_file.exists() {
            let contents = fs::read_to_string(&version_file)
                .await
                .map_err(|e| OtaError::VersionQueryFailed(format!("读取版本文件失败: {}", e)))?;
            serde_json::from_str::<VersionFile>(&contents).unwrap_or(VersionFile { models: vec![] })
        } else {
            VersionFile { models: vec![] }
        };

        // 更新或添加版本信息
        version_file_data
            .models
            .retain(|v| v.model_type != version.model_type);
        version_file_data.models.push(version.clone());

        // 写入版本文件
        let json = serde_json::to_string_pretty(&version_file_data)
            .map_err(|e| OtaError::VersionQueryFailed(format!("序列化版本信息失败: {}", e)))?;

        fs::write(&version_file, json)
            .await
            .map_err(|e| OtaError::VersionQueryFailed(format!("写入版本文件失败: {}", e)))?;

        Ok(())
    }
}

/// 从路径中提取版本号
fn extract_version_from_path(path: &Path) -> Result<String, OtaError> {
    // 尝试从目录名或文件名中提取版本号
    // 格式通常是 v1.2.0 或类似版本号

    let path_str = path.display().to_string();

    // 查找 v 开头的版本号模式（使用缓存的正则）
    if let Some(captures) = VERSION_PATTERN.captures(&path_str) {
        if let Some(version) = captures.get(1) {
            return Ok(version.as_str().to_string());
        }
    }

    // 如果无法提取版本号，使用时间戳作为版本号
    let timestamp: DateTime<Utc> = Utc::now();
    Ok(timestamp.format("%Y%m%d%H%M%S").to_string())
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
    fn test_extract_version_from_path() {
        // 测试从目录路径提取版本号
        let path = PathBuf::from("/models/update/v1.2.0/model.rknn");
        let version = extract_version_from_path(&path).unwrap();
        assert_eq!(version, "1.2.0");

        // 测试带 v 前缀
        let path = PathBuf::from("/models/update/v2.0.0/model.rknn");
        let version = extract_version_from_path(&path).unwrap();
        assert_eq!(version, "2.0.0");

        // 测试文件名提取
        let path = PathBuf::from("/models/update/model-v1.5.0.rknn");
        let version = extract_version_from_path(&path).unwrap();
        assert_eq!(version, "1.5.0");

        // 测试时间戳版本号（无法提取时）
        let path = PathBuf::from("/models/update/package");
        let version = extract_version_from_path(&path).unwrap();
        assert!(version.len() > 0);
    }

    // ========== ModelApplicator 创建测试 ==========

    #[tokio::test]
    async fn test_applicator_new_valid_path() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let result = ModelApplicator::new(models_dir.clone(), verifier, None);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_applicator_new_invalid_path() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let nonexistent_path = temp_dir.join("nonexistent");

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let result = ModelApplicator::new(nonexistent_path, verifier, None);
        assert!(result.is_err());
    }

    // ========== copy_to_current 原子性测试（D-17） ==========

    /// 造一个 applicator（模型根目录已存在）。
    async fn make_applicator(root: &Path) -> ModelApplicator {
        let key_path = root.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();
        let verifier = Arc::new(Verifier::new(key_path).unwrap());
        ModelApplicator::new(root.to_path_buf(), verifier, None).unwrap()
    }

    /// 判别力：替换**失败**时旧模型必须**原样在位**。
    ///
    /// 旧实现是"先 `remove_file` 再 `copy`"：复制一失败，`current/model.rknn` 已经没了 ⇒
    /// 本用例红（断言"旧内容仍在"）。新实现先写临时文件再 `rename` ⇒ 旧文件从未被删。
    #[tokio::test]
    async fn failed_replacement_keeps_the_old_model() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let root = temp_dir.join("models");
        fs::create_dir_all(&root).await.unwrap();
        let applicator = make_applicator(&root).await;

        // 布置"旧模型"
        let current_dir = root.join("current").join(ModelType::Lstm.to_string());
        fs::create_dir_all(&current_dir).await.unwrap();
        let dest = current_dir.join(MODEL_FILENAME);
        fs::write(&dest, b"OLD-MODEL-BYTES").await.unwrap();

        // 源文件不存在 ⇒ 复制必失败（这就是"中断/失败"）
        let missing = temp_dir.join("no-such-update.rknn");
        let err = applicator
            .copy_to_current(ModelType::Lstm, &missing)
            .await
            .expect_err("源不存在时替换必须失败");
        assert!(
            err.to_string().contains("复制") || err.to_string().contains("文件"),
            "失败原因须可定位: {err}"
        );

        // 关键断言：旧模型**仍在且未变**
        assert!(
            dest.exists(),
            "替换失败后旧模型必须仍在（旧实现此处已被删除）"
        );
        assert_eq!(
            fs::read(&dest).await.unwrap(),
            b"OLD-MODEL-BYTES",
            "旧模型内容必须逐字节不变"
        );
        // 且不留下任何暂存文件（半截状态）
        let leftovers: Vec<String> = std::fs::read_dir(&current_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "失败后不得残留暂存文件: {leftovers:?}"
        );
    }

    /// 成功路径：内容被替换成新模型，且不残留暂存文件。
    #[tokio::test]
    async fn successful_replacement_swaps_content_atomically() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let root = temp_dir.join("models");
        fs::create_dir_all(&root).await.unwrap();
        let applicator = make_applicator(&root).await;

        let current_dir = root.join("current").join(ModelType::Maddpg.to_string());
        fs::create_dir_all(&current_dir).await.unwrap();
        let dest = current_dir.join(MODEL_FILENAME);
        fs::write(&dest, b"OLD").await.unwrap();

        let src = temp_dir.join("v9.9.9.rknn");
        fs::write(&src, b"NEW-MODEL-BYTES").await.unwrap();

        applicator
            .copy_to_current(ModelType::Maddpg, &src)
            .await
            .unwrap();

        assert_eq!(fs::read(&dest).await.unwrap(), b"NEW-MODEL-BYTES");
        let leftovers: Vec<String> = std::fs::read_dir(&current_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "不得残留暂存文件: {leftovers:?}");
    }

    // ========== calculate_checksum 测试 ==========

    #[tokio::test]
    async fn test_calculate_checksum() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir, verifier, None).unwrap();

        let test_file = temp_dir.join("test_file.txt");
        fs::write(&test_file, b"hello world").await.unwrap();

        let checksum = applicator.calculate_checksum(&test_file).await.unwrap();

        // "hello world" 的 SHA-256 哈希值
        assert_eq!(
            checksum,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[tokio::test]
    async fn test_calculate_checksum_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir, verifier, None).unwrap();

        let result = applicator
            .calculate_checksum(&PathBuf::from("/nonexistent/file.txt"))
            .await;
        assert!(result.is_err());
    }

    // ========== get_current_version 测试 ==========

    #[tokio::test]
    async fn test_get_current_version_not_exists() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir, verifier, None).unwrap();

        let result = applicator.get_current_version(ModelType::Lstm).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_current_version_valid() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir.clone(), verifier, None).unwrap();

        // 创建版本文件
        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": [
                {
                    "model_type": "lstm",
                    "version": "1.2.0",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "abc123",
                    "size": 1024
                }
            ]
        });
        fs::write(&version_file, version_data.to_string())
            .await
            .unwrap();

        let result = applicator.get_current_version(ModelType::Lstm).await;
        assert!(result.is_ok());
        let version = result.unwrap();
        assert_eq!(version.version, "1.2.0");
        assert_eq!(version.md5, "abc123");
    }

    #[tokio::test]
    async fn test_get_current_version_model_not_found() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir.clone(), verifier, None).unwrap();

        // 创建版本文件，但不包含 maddpg 模型
        let version_file = models_dir.join("version.json");
        let version_data = serde_json::json!({
            "models": [
                {
                    "model_type": "lstm",
                    "version": "1.2.0",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "abc123",
                    "size": 1024
                }
            ]
        });
        fs::write(&version_file, version_data.to_string())
            .await
            .unwrap();

        let result = applicator.get_current_version(ModelType::Maddpg).await;
        assert!(result.is_err());
    }

    // ========== save_version_info 测试 ==========

    #[tokio::test]
    async fn test_save_version_info_new_file() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir.clone(), verifier, None).unwrap();

        let version = ModelVersion {
            model_type: ModelType::Lstm,
            version: "1.3.0".to_string(),
            updated_at: Utc::now(),
            md5: "new_md5".to_string(),
            size: 2048,
        };

        let result = applicator.save_version_info(&version).await;
        assert!(result.is_ok());

        // 验证文件已创建
        let version_file = models_dir.join("version.json");
        assert!(version_file.exists());

        // 验证内容
        let contents = fs::read_to_string(&version_file).await.unwrap();
        assert!(contents.contains("1.3.0"));
        assert!(contents.contains("lstm"));
    }

    #[tokio::test]
    async fn test_save_version_info_update_existing() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir.clone(), verifier, None).unwrap();

        // 创建初始版本文件
        let version_file = models_dir.join("version.json");
        let initial_data = serde_json::json!({
            "models": [
                {
                    "model_type": "lstm",
                    "version": "1.2.0",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "old_md5",
                    "size": 1024
                },
                {
                    "model_type": "maddpg",
                    "version": "1.0.0",
                    "updated_at": "2026-05-28T10:00:00Z",
                    "md5": "maddpg_md5",
                    "size": 2048
                }
            ]
        });
        fs::write(&version_file, initial_data.to_string())
            .await
            .unwrap();

        // 更新 lstm 版本
        let new_version = ModelVersion {
            model_type: ModelType::Lstm,
            version: "1.3.0".to_string(),
            updated_at: Utc::now(),
            md5: "new_md5".to_string(),
            size: 2048,
        };

        let result = applicator.save_version_info(&new_version).await;
        assert!(result.is_ok());

        // 验证更新
        let contents = fs::read_to_string(&version_file).await.unwrap();
        assert!(contents.contains("1.3.0"));
        assert!(contents.contains("maddpg")); // maddpg 应保留
    }

    // ========== notify_strategy_engine 测试 ==========

    #[tokio::test]
    async fn test_notify_strategy_engine_with_callback() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let notified_type = std::sync::Arc::new(std::sync::Mutex::new(None::<ModelType>));
        let notified_type_clone = notified_type.clone();

        let callback: Option<StrategyEngineNotifyFn> = Some(Box::new(move |model_type| {
            *notified_type_clone.lock().unwrap() = Some(model_type);
        }));

        let applicator = ModelApplicator::new(models_dir, verifier, callback).unwrap();

        applicator.notify_strategy_engine(ModelType::Lstm);

        let notified = notified_type.lock().unwrap();
        assert_eq!(*notified, Some(ModelType::Lstm));
    }

    #[tokio::test]
    async fn test_notify_strategy_engine_without_callback() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir, verifier, None).unwrap();

        // 不应 panic，即使没有回调
        applicator.notify_strategy_engine(ModelType::Lstm);
        applicator.notify_strategy_engine(ModelType::Maddpg);
    }

    // ========== 目录路径测试 ==========

    #[tokio::test]
    async fn test_directory_paths() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let models_dir = temp_dir.join("models");
        fs::create_dir_all(&models_dir).await.unwrap();

        let key_path = temp_dir.join("public_key.pem");
        fs::write(&key_path, b"test key").await.unwrap();

        let verifier = Verifier::new(key_path).unwrap();
        let verifier = Arc::new(verifier);

        let applicator = ModelApplicator::new(models_dir.clone(), verifier, None).unwrap();

        // 测试 current_dir
        let lstm_dir = applicator.current_dir(ModelType::Lstm);
        assert_eq!(lstm_dir, models_dir.join("current").join("lstm"));

        let maddpg_dir = applicator.current_dir(ModelType::Maddpg);
        assert_eq!(maddpg_dir, models_dir.join("current").join("maddpg"));

        // 测试 rollback_dir
        let rollback_dir = applicator.rollback_dir("v1.2.0");
        assert_eq!(rollback_dir, models_dir.join("rollback").join("v1.2.0"));

        // 测试 version_file_path
        let version_path = applicator.version_file_path();
        assert_eq!(version_path, models_dir.join("version.json"));
    }
}
