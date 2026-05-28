# MUPC Phase 3C.2 规格文档 - 模型自动更新（OTA）

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-28 | 需求分析师 | ✅ 已评审 |

---

[REVIEWED: PASS]

---

## 1. 概述

### 1.1 项目背景

MUPC Phase 3C 实现 AI 优化引擎，已完成：
- LSTM 时序预测模型
- MADDPG/PPO 强化学习决策模型
- RKNN Runtime 推理（RK3588 NPU）

Phase 3C.2 在 Phase 3C 基础上实现**模型自动更新（OTA）**功能，使 AI 模型能够在现场运行时接收并应用更新，无需人工干预。

### 1.2 适用范围

| 项目 | 说明 |
|------|------|
| 设备 | MUPC 微电网特种调控装置 |
| 平台 | Linux (openEuler)、RK3588 |
| 硬件 | ARM64、NPU (RKNN) |
| 通信 | 北向通信网关（IEC 104 / MQTT / 61850）|

### 1.3 目标

- 实现可靠的模型 OTA 更新机制
- 支持断点续传和增量更新
- 确保模型安全性（签名验证）
- 提供自动回滚机制保障业务连续性

---

## 2. 功能列表

### 2.1 OTA 更新流程

#### 2.1.1 更新检查

**触发方式**：
| 方式 | 触发条件 | 优先级 |
|------|----------|--------|
| 定时检查 | 每小时自动检查（可配置） | 低 |
| 手动触发 | 北向收到更新指令 | 高 |
| 启动检查 | 设备启动时检查一次 | 中 |

**检查流程**：
```
1. 连接到 OTA 服务器（地址可配置）
2. 发送当前模型版本信息
3. 服务器返回最新版本信息
4. 比较版本号判断是否需要更新
5. 如果需要更新，下载版本清单
```

**版本号格式**：`major.minor.patch`（例如 `1.2.0`）

#### 2.1.2 更新包下载

**下载流程**：
```
1. 解析版本清单，获取更新包 URL
2. 计算本地可用存储空间
3. 下载更新包到临时存储区
4. 支持断点续传（HTTP Range 请求）
5. 下载完成后校验文件哈希
```

**断点续传**：
- 使用 HTTP Range 请求
- 记录已下载字节数
- 断电恢复后继续下载

**下载参数**：
| 参数 | 默认值 | 说明 |
|------|--------|------|
| timeout | 300 秒 | 单次下载超时 |
| retry_count | 3 | 下载失败重试次数 |
| chunk_size | 1 MB | 分块大小 |
| min_free_space | 500 MB | 最低可用空间 |

#### 2.1.3 更新包验证

**验证流程**：
```
1. 文件完整性校验（SHA-256）
2. 模型签名验证（SM2 / Ed25519）
3. 模型格式校验（RKNN / ONNX）
4. 模型兼容性校验（平台版本匹配）
```

**签名验证**：
- 使用国密 SM2 或 Ed25519 签名算法
- 公钥存储在设备安全区域
- 签名包格式：`| signature(64B) | model_data |`

#### 2.1.4 模型应用

**应用流程**：
```
1. 备份当前模型到 rollback 目录
2. 解压更新包到模型目录
3. 通知策略引擎加载新模型
4. 新模型通过 RKNN Runtime 加载
5. 执行模型预热（推理一次）
6. 更新版本记录
```

**加载顺序**：
```
旧模型 → 备份 → 新模型加载 → 预热 → 切换 → 删除旧模型
```

### 2.2 模型版本管理

#### 2.2.1 版本存储

**存储结构**：
```
/models/
├── current/           # 当前运行模型
│   ├── lstm/model.rknn
│   └── maddpg/model.rknn
├── update/            # 下载的更新包
│   └── v1.2.0/
├── rollback/          # 回滚备份
│   └── v1.1.0/
└── version.json       # 版本信息
```

**version.json 格式**：
```json
{
  "lstm": {
    "version": "1.1.0",
    "updated_at": "2026-05-28T10:00:00Z",
    "md5": "abc123..."
  },
  "maddpg": {
    "version": "1.0.5",
    "updated_at": "2026-05-27T08:30:00Z",
    "md5": "def456..."
  }
}
```

#### 2.2.2 版本查询

**接口**：
```rust
trait OtaManager {
    // 获取当前模型版本
    fn get_current_version(&self, model_type: ModelType) -> Result<ModelVersion, OtaError>;

    // 获取可用更新列表
    async fn check_updates(&self) -> Result<Vec<UpdateInfo>, OtaError>;

    // 获取更新状态
    fn get_update_status(&self) -> UpdateStatus;
}
```

### 2.3 增量更新支持

#### 2.3.1 增量包格式

**增量包结构**：
```
| header(64B) | base_version | target_version | diff_data | patch_info |
```

**header 格式**：
| 字段 | 长度 | 说明 |
|------|------|------|
| magic | 4B | 固定值 `OTAP` |
| version | 1B | 增量包格式版本 |
| base_version | 16B | 基础版本号 |
| target_version | 16B | 目标版本号 |
| diff_size | 4B | 差分包大小 |
| checksum | 32B | SHA-256 校验 |

#### 2.3.2 增量更新流程

```
1. 检查当前版本是否支持增量更新
2. 下载增量包
3. 验证增量包版本信息
4. 应用差分补丁生成新模型
5. 全量校验新模型完整性
```

**应用场景**：
- 模型权重微调（< 10MB 增量）
- 策略参数更新（< 1MB 增量）
- 全量更新（首次安装或跨版本）

### 2.4 回滚机制

#### 2.4.1 自动回滚触发条件

| 条件 | 阈值 | 说明 |
|------|------|------|
| 新模型加载失败 | - | RKNN Runtime 加载异常 |
| 模型推理失败 | 连续 3 次 | 推理结果异常 |
| 模型校验失败 | - | 签名/哈希校验不通过 |
| 模型预热超时 | 30 秒 | 预热推理超时 |

#### 2.4.2 回滚流程

```
1. 检测到回滚触发条件
2. 停止策略引擎
3. 删除新模型
4. 从 rollback 目录恢复旧模型
5. 重启策略引擎加载旧模型
6. 记录回滚事件到日志
7. 发送回滚通知到北向
```

#### 2.4.3 回滚限制

- 回滚次数上限：连续 3 次
- 超过限制后进入安全模式（使用兜底策略）
- 回滚记录保存 30 天

### 2.5 更新策略

#### 2.5.1 定时更新

**配置参数**：
| 参数 | 默认值 | 说明 |
|------|--------|------|
| check_interval | 3600 秒 | 检查间隔 |
| download_window_start | 02:00 | 下载窗口开始时间 |
| download_window_end | 05:00 | 下载窗口结束时间 |
| auto_download | true | 检查到更新自动下载 |

**定时更新流程**：
```
1. 定时器触发检查
2. 检查当前时间是否在下载窗口内
3. 如果在窗口内，执行更新检查
4. 下载完成，进入等待应用状态
5. 下次设备空闲时应用更新
```

#### 2.5.2 手动更新

**北向指令格式**（IEC 104 / MQTT）：
```json
{
  "cmd": "ota_update",
  "model_type": "lstm",
  "version": "1.2.0",
  "url": "https://ota.example.com/models/lstm/v1.2.0.rknn",
  "signature": "...",
  "checksum": "..."
}
```

**手动更新响应**：
```json
{
  "cmd": "ota_update_ack",
  "task_id": "ota_20260528_001",
  "status": "downloading",
  "progress": 45
}
```

#### 2.5.3 更新状态机

```
                    ┌─────────────┐
                    │   IDLE     │
                    └──────┬──────┘
                           │ check_updates()
                           ▼
                    ┌─────────────┐
         ┌───────────│  CHECKING  │──────────┐
         │          └──────┬──────┘          │
         │                 │                 │
         │ no_update       │ need_update     │ error
         ▼                 ▼                 ▼
   ┌──────────┐     ┌─────────────┐    ┌──────────┐
   │  IDLE    │     │ DOWNLOADING │    │  FAILED  │
   └──────────┘     └──────┬──────┘    └──────────┘
                           │                 │
                           │ download_ok     │ retry_exhausted
                           ▼                 │
                    ┌─────────────┐           │
                    │ VERIFYING   │───────────┤
                    └──────┬──────┘           │
                           │                 │
                           │ verify_ok       │ verify_failed
                           ▼                 │
                    ┌─────────────┐           │
                    │ APPLYING    │───────────┘
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              │ apply_ok   │ apply_fail  │
              ▼            ▼             │
        ┌──────────┐  ┌────────────┐     │
        │ APPLIED  │  │ ROLLING    │     │
        └──────────┘  │ BACK       │     │
                      └──────┬─────┘     │
                             │            │
                             ▼            ▼
                       ┌──────────┐  ┌──────────┐
                       │ COMPLETE │  │  FAILED  │
                       └──────────┘  └──────────┘
```

---

## 3. 用户故事

### 3.1 远程推送模型更新

**角色**：运维人员

**场景**：运维人员在云端平台发现新版本 LSTM 模型，期望推送到现场 MUPC 设备

**流程**：
1. 运维人员在云端平台创建模型更新任务
2. 选择目标设备（单台或批量）
3. 上传新版本模型文件
4. 系统自动签名并分发到 OTA 服务器
5. 运维人员发送远程更新指令到 MUPC
6. MUPC 接收指令，开始下载并应用更新
7. 运维人员收到更新完成通知

**验收标准**：
- [ ] 运维人员可在云端平台创建更新任务
- [ ] MUPC 能在 5 分钟内响应远程更新指令
- [ ] 更新进度实时同步到云端平台
- [ ] 更新完成后云端收到成功通知

### 3.2 现场设备自动更新

**角色**：MUPC 设备

**场景**：MUPC 设备在凌晨空闲时段自动检查并应用模型更新

**流程**：
1. MUPC 每天凌晨 02:00 自动检查更新
2. 发现新版本模型，校验可用空间
3. 在 02:00-05:00 窗口内下载更新包
4. 断点续传支持（模拟断电续传）
5. 下载完成后验证签名和完整性
6. 设备空闲时自动应用新模型
7. 新模型预热完成后切换

**验收标准**：
- [ ] 设备每小时检查一次更新（可配置）
- [ ] 设备在下载窗口内完成下载
- [ ] 断电后恢复下载，已下载部分不丢失
- [ ] 更新后模型推理结果正确
- [ ] 更新过程不影响其他模块运行

### 3.3 更新失败自动回滚

**角色**：MUPC 设备

**场景**：MUPC 设备在模型更新过程中检测到异常，自动回滚到旧版本

**流程**：
1. MUPC 下载并验证新模型
2. 新模型加载时检测到 RKNN 格式不兼容
3. 自动触发回滚机制
4. 恢复 rollback 目录中的旧模型
5. 策略引擎重新加载旧模型
6. 发送回滚通知到北向
7. 记录回滚事件到本地日志

**验收标准**：
- [ ] 新模型加载失败时 10 秒内触发回滚
- [ ] 回滚后设备恢复正常运行
- [ ] 回滚通知在 1 分钟内发送到北向
- [ ] 回滚次数超过 3 次后进入安全模式
- [ ] 回滚事件记录完整可查询

---

## 4. 接口定义

### 4.1 OTA 管理器接口

```rust
use async_trait::async_trait;

// 模型类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelType {
    Lstm,
    Maddpg,
}

// 模型版本信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelVersion {
    pub model_type: ModelType,
    pub version: String,          // 例如 "1.2.0"
    pub updated_at: DateTime<Utc>,
    pub md5: String,
    pub size: u64,               // 字节数
}

// 更新信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateInfo {
    pub model_type: ModelType,
    pub current_version: String,
    pub available_version: String,
    pub size: u64,
    pub checksum: String,
    pub signature: String,
    pub url: String,
    pub is_incremental: bool,
    pub base_version: Option<String>,
}

// 更新状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Downloading { progress: u8 },
    Verifying,
    Applying,
    Applied,
    RollingBack,
    Failed { error: String },
    Completed,
}

// OTA 错误类型
#[derive(Error, Debug)]
pub enum OtaError {
    #[error("网络连接失败: {0}")]
    NetworkError(String),

    #[error("下载失败: {0}")]
    DownloadFailed(String),

    #[error("下载空间不足: 需要 {need} 字节, 可用 {available} 字节")]
    InsufficientSpace { need: u64, available: u64 },

    #[error("校验失败: {0}")]
    VerificationFailed(String),

    #[error("签名验证失败")]
    SignatureInvalid,

    #[error("模型加载失败: {0}")]
    ModelLoadFailed(String),

    #[error("版本不兼容: 当前 {current}, 需要 {required}")]
    VersionIncompatible { current: String, required: String },

    #[error("更新超时")]
    UpdateTimeout,

    #[error("回滚失败: {0}")]
    RollbackFailed(String),

    #[error("回滚次数超限")]
    RollbackLimitExceeded,
}

// OTA 管理器接口
#[async_trait]
pub trait OtaManager: Send + Sync {
    // 获取当前模型版本
    fn get_current_version(&self, model_type: ModelType) -> Result<ModelVersion, OtaError>;

    // 检查可用更新
    async fn check_updates(&self) -> Result<Vec<UpdateInfo>, OtaError>;

    // 启动更新下载
    async fn start_download(&self, update_info: &UpdateInfo) -> Result<TaskId, OtaError>;

    // 获取下载进度
    fn get_download_progress(&self, task_id: TaskId) -> Result<u8, OtaError>;

    // 取消下载
    async fn cancel_download(&self, task_id: TaskId) -> Result<(), OtaError>;

    // 应用已下载的更新
    async fn apply_update(&self, task_id: TaskId) -> Result<(), OtaError>;

    // 触发回滚
    async fn rollback(&self, model_type: ModelType) -> Result<(), OtaError>;

    // 获取更新状态
    fn get_update_status(&self) -> UpdateStatus;

    // 获取更新历史
    fn get_update_history(&self, limit: usize) -> Result<Vec<UpdateRecord>, OtaError>;
}

// 更新记录
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateRecord {
    pub task_id: String,
    pub model_type: ModelType,
    pub from_version: String,
    pub to_version: String,
    pub status: UpdateStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}
```

### 4.2 北向通信接口

```rust
// OTA 指令（接收）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OtaUpdateCommand {
    pub cmd: String,              // "ota_update"
    pub task_id: String,
    pub model_type: ModelType,
    pub version: String,
    pub url: String,
    pub signature: String,
    pub checksum: String,
}

// OTA 响应（发送）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OtaUpdateResponse {
    pub task_id: String,
    pub model_type: ModelType,
    pub status: UpdateStatus,
    pub progress: Option<u8>,     // 0-100
    pub error_message: Option<String>,
}

// 版本查询响应
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VersionQueryResponse {
    pub models: Vec<ModelVersion>,
    pub device_id: String,
    pub timestamp: DateTime<Utc>,
}
```

### 4.3 配置接口

```rust
// OTA 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OtaConfig {
    // OTA 服务器地址
    pub server_url: String,

    // 检查更新间隔（秒）
    pub check_interval: u64,

    // 下载窗口
    pub download_window_start: String,  // "02:00"
    pub download_window_end: String,    // "05:00"

    // 自动下载
    pub auto_download: bool,

    // 自动应用
    pub auto_apply: bool,

    // 下载超时（秒）
    pub download_timeout: u64,

    // 重试次数
    pub retry_count: u32,

    // 回滚次数上限
    pub max_rollback_count: u32,

    // 签名公钥路径
    pub public_key_path: String,

    // 模型存储路径
    pub model_storage_path: String,
}

impl Default for OtaConfig {
    fn default() -> Self {
        Self {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: "/etc/mupc/ota_public_key.pem".to_string(),
            model_storage_path: "/models".to_string(),
        }
    }
}
```

---

## 5. 架构设计

### 5.1 模块依赖

```
ota-update
├── common (错误类型、日志)
├── gateway (北向通信)
├── strategy-engine (模型加载)
└── intercore (状态上报)

依赖关系：
- ota-update 使用 common 中的错误类型和日志
- ota-update 通过 gateway 接收远程指令和上报状态
- ota-update 通知 strategy-engine 切换模型
- ota-update 通过 intercore 上报更新状态到实时控制模块
```

### 5.2 目录结构

```
crates/
├── ota-update/                    # OTA 更新模块
│   ├── src/
│   │   ├── lib.rs
│   │   ├── manager.rs            # OTA 管理器
│   │   ├── downloader.rs         # 下载器（支持断点续传）
│   │   ├── verifier.rs           # 验证器（签名、哈希）
│   │   ├── applicator.rs         # 模型应用器
│   │   ├── rollback.rs           # 回滚管理器
│   │   ├── scheduler.rs          # 定时任务调度器
│   │   ├── config.rs             # 配置管理
│   │   └── error.rs              # 错误类型
│   ├── tests/
│   └── Cargo.toml
```

### 5.3 关键设计决策

#### 5.3.1 断点续传实现

使用 HTTP Range 请求实现断点续传：

```rust
async fn download_with_resume(
    client: &reqwest::Client,
    url: &str,
    temp_path: &Path,
    offset: u64,
) -> Result<u64, OtaError> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(temp_path)?;

    let response = client
        .get(url)
        .header("Range", format!("bytes={}-", offset))
        .send()
        .await
        .map_err(|e| OtaError::NetworkError(e.to_string()))?;

    let mut stream = response.bytes_stream();
    let mut written = 0u64;

    use tokio::io::AsyncWriteExt;
    let mut async_file = tokio::fs::File::from_std(file);

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| OtaError::DownloadFailed(e.to_string()))?;
        async_file.write_all(&bytes).await?;
        written += bytes.len() as u64;
    }

    Ok(written)
}
```

#### 5.3.2 签名验证实现

使用 P256 (Ed25519) 或国密 SM2：

```rust
async fn verify_signature(
    data: &[u8],
    signature: &[u8],
    public_key: &PublicKey,
) -> Result<bool, OtaError> {
    match public_key.algorithm {
        Algorithm::Ed25519 => {
            use ed25519_dalek::VerifyingKey;
            let key: VerifyingKey = public_key.try_into()?;
            Ok(key.verify(data, &signature.into()).is_ok())
        }
        Algorithm::SM2 => {
            use sm2::signature::Verifier;
            let key: sm2::PublicKey = public_key.try_into()?;
            Ok(key.verify(data, &signature.into()).is_ok())
        }
    }
}
```

#### 5.3.3 增量更新实现

使用 bsdiff/bsdiff-style 差分算法：

```rust
async fn apply_incremental_update(
    base_model: &[u8],
    patch: &[u8],
) -> Result<Vec<u8>, OtaError> {
    // 解析增量包
    let header = IncrementalPatchHeader::decode(patch)?;

    // 应用差分
    let patched = diff::apply(base_model, header.diff_data)
        .map_err(|e| OtaError::VerificationFailed(e.to_string()))?;

    Ok(patched)
}
```

---

## 6. 验收标准

### 6.1 功能验收

| ID | 功能 | 验收条件 | 验证方法 |
|----|------|----------|----------|
| OTA-01 | 更新检查 | 设备能在 30 秒内完成一次更新检查 | 单元测试 |
| OTA-02 | 定时检查 | 每小时自动触发一次检查（配置 `check_interval=3600`） | 单元测试 |
| OTA-03 | 手动触发 | 北向收到指令后 5 秒内开始下载 | 集成测试 |
| OTA-04 | 断点续传 | 下载中断后恢复，已下载部分不丢失 | 集成测试 |
| OTA-05 | 文件校验 | SHA-256 校验失败的更新包不被应用 | 单元测试 |
| OTA-06 | 签名验证 | SM2/Ed25519 签名不通过的更新包不被应用 | 单元测试 |
| OTA-07 | 模型加载 | 新模型能在 60 秒内通过 RKNN Runtime 加载 | 集成测试 |
| OTA-08 | 模型预热 | 新模型推理预热在 30 秒内完成 | 集成测试 |
| OTA-09 | 自动回滚 | 模型加载失败后 10 秒内触发自动回滚 | 集成测试 |
| OTA-10 | 回滚恢复 | 回滚后旧模型能正常加载并推理 | 集成测试 |
| OTA-11 | 回滚限制 | 连续 3 次回滚后设备进入安全模式 | 集成测试 |
| OTA-12 | 版本查询 | 北向能查询当前所有模型的版本信息 | 集成测试 |
| OTA-13 | 增量更新 | 增量包能正确应用并生成目标版本模型 | 单元测试 |
| OTA-14 | 更新历史 | 能查询最近 30 天的更新记录 | 单元测试 |
| OTA-15 | 下载进度 | 北向能实时获取下载进度（0-100%）| 集成测试 |
| OTA-16 | 空间检查 | 可用空间小于 500MB 时禁止下载 | 单元测试 |
| OTA-17 | 更新取消 | 下载中的更新任务能被取消 | 集成测试 |
| OTA-18 | 状态上报 | 更新状态变化时实时上报北向 | 集成测试 |

### 6.2 非功能验收

| 类型 | 指标 | 验收条件 |
|------|------|----------|
| 更新时长 | 全量更新 | 单模型 < 10 分钟（100MB 模型） |
| 更新时长 | 增量更新 | 单模型 < 2 分钟（10MB 增量） |
| 系统影响 | 内存占用 | OTA 模块 < 50MB |
| 系统影响 | CPU 峰值 | 下载期间 CPU 峰值 < 30% |
| 可靠性 | 断点续传 | 10 次断电恢复后更新成功 |
| 可靠性 | 回滚成功率 | 回滚成功率 ≥ 99% |
| 安全性 | 签名验证 | 伪造签名包 100% 被拒绝 |
| 安全性 | 数据完整性 | 损坏包 100% 被检测并拒绝 |

### 6.3 安全验收

| ID | 安全项 | 验收条件 |
|----|--------|----------|
| SEC-01 | 签名验证 | 所有模型必须通过签名验证才能应用 |
| SEC-02 | 公钥保护 | OTA 公钥存储在安全区域（/etc/mupc/） |
| SEC-03 | 传输加密 | 下载使用 HTTPS/TLS 1.2+ |
| SEC-04 | 篡改检测 | 文件哈希校验失败的模型不被应用 |
| SEC-05 | 日志审计 | 所有更新操作记录完整审计日志 |

---

## 7. 技术栈

| 组件 | 选择 | 说明 |
|------|------|------|
| 语言 | Rust 1.75+ | |
| 异步运行时 | Tokio 1.x | |
| HTTP 客户端 | reqwest | 支持 HTTPS 和 Range 请求 |
| 差分算法 | bsdiff | 增量更新 |
| 哈希算法 | SHA-256 | 文件完整性校验 |
| 签名算法 | Ed25519 / SM2 | 模型签名验证 |
| 配置格式 | TOML | ota-update.toml |
| 日志 | tracing | |

---

## 8. 未来扩展 (Phase 4)

| Phase | 内容 |
|-------|------|
| 4A | 分批推送（灰度发布）|
| 4B | 更新回滚可视化（云端管理平台）|
| 4C | A/B 测试框架 |

---

**评审状态**：已通过评审
**文档版本**：v1.0
**最后更新**：2026-05-28