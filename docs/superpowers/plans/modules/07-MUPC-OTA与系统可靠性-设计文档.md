# MUPC OTA 升级与系统可靠性技术设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 架构师 | 合并稿 |

> 本文档为 OTA 升级与系统可靠性模块的权威设计文档。历史来源文档已在 v1.0 文档体系重构中合并，不再单独维护。

[DESIGN_APPROVED] — 模型 OTA 部分已完成评审并获得批准
固件 OTA 与系统可靠性部分为草稿状态，待评审

---

## 目录

1. [模块架构](#1-模块架构)
2. [模型 OTA 设计](#2-模型-ota-设计)
3. [固件 OTA 设计](#3-固件-ota-设计)
4. [系统监控设计](#4-系统监控设计)
5. [MTBF 计算设计](#5-mtbf-计算设计)
6. [异常自愈设计](#6-异常自愈设计)
7. [与 intercore 协同设计](#7-与-intercore-协同设计)
8. [接口定义](#8-接口定义)
9. [文件结构](#9-文件结构)
10. [技术决策记录](#10-技术决策记录)

---

## 1. 模块架构

### 1.1 总体架构

本模块覆盖两个功能域（模型 OTA、固件 OTA）和一个系统可靠性保障（系统监控），共涉及两个新增/改造 crate：

```
mupc/
├── crates/
│   ├── ota-update/              # [改造] 从纯模型OTA扩展为 update-engine（固件+模型双模式OTA）
│   └── system-monitor/          # [新增] 系统可靠性守护进程（五维监控+MTBF+自愈）
```

### 1.2 系统级架构关系

```
┌─────────────────────────────────────────────────────────────┐
│                    system-monitor (新 crate)                  │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐  │
│  │ 内存监控  │ │ 进程监控  │ │ 磁盘监控  │ │  CPU/网络监控  │  │
│  │ 30s周期   │ │ 15s周期   │ │ 60s周期   │ │  30s/60s周期  │  │
│  └─────┬────┘ └────┬─────┘ └────┬─────┘ └───────┬───────┘  │
│        └────────────┼────────────┼────────────────┘          │
│                     ▼            ▼                           │
│  ┌──────────────────────────────────────────────────────┐    │
│  │             自愈引擎 (SelfHealEngine)                  │    │
│  │  看门狗 | cgroup v2 限制 | OOM 保护 | 进程重启 | 磁盘清理 │    │
│  └──────────┬───────────────────────────────────────────┘    │
│             │                                                │
│  ┌──────────▼───────────────────────────────────────────┐    │
│  │              MTBF 计算引擎                              │    │
│  │  365天滚动窗口 | 故障计数器 | uptime 追踪              │    │
│  └──────────┬───────────────────────────────────────────┘    │
└─────────────┼───────────────────────────────────────────────┘
              │ 集成
              ▼
┌─────────────────────────────────────────────────────────────┐
│               update-engine (ota-update 改造)                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────┐  │
│  │ 固件升级  │ │ 模型OTA  │ │ A/B分区  │ │  升级状态机    │  │
│  │ 状态机    │ │(保留原有) │ │ 管理器   │ │  验证回滚      │  │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └───────┬───────┘  │
│       └────────────┼────────────┼────────────────┘          │
│                    ▼            ▼                           │
│  ┌──────────────────────────────────────────────────────┐    │
│  │  .mupc 包处理器 | bsdiff 应用 | SM2 验证 | 下载器     │    │
│  │  (断点续传+限速)                                     │    │
│  └──────────────────────────────────────────────────────┘    │
└─────────────┬───────────────────────────────────────────────┘
              │ 协同
              ▼
┌─────────────────────────────────────────────────────────────┐
│                    intercore                                 │
│  升级前发送 strategy_mode=basic + ai_ready=false 信号        │
│  升级后发送 ai_ready=true + strategy_mode=smart 恢复信号      │
└─────────────────────────────────────────────────────────────┘
```

### 1.3 模块依赖关系

```
update-engine (改造后)
├── common (错误类型、日志)
├── gateway (北向通信、接收指令、上报状态)
├── strategy-engine (模型加载通知)
├── intercore (状态上报、升级信号)
├── security (SM2 签名验证)
└── web-api (状态查询 REST API)

system-monitor (新增)
├── common (错误类型、日志)
├── web-api (REST 接口暴露、WebSocket 推送)
├── intercore (核间通信延迟监控)
└── security (OOM 事件签名验证 -- 预留)
```

### 1.4 数据流关系

```
OTA 服务器                               MUPC 设备
┌──────────┐   HTTPS (RESTful API)     ┌──────────────────┐
│ 固件包    │ ◄────────────────────────► │ update-engine    │
│ 管理      │   版本查询/下载/状态上报    │ 下载→验证→写入   │
│ 灰度发布  │                           │ A/B 分区切换     │
│ 差分包    │                           └──────┬───────────┘
│ 生成      │                                  │ 升级前/后
└──────────┘                                   │ 信号交互
                                              ▼
                                       ┌──────────────────┐
                                       │ intercore         │
                                       │ strategy_mode     │
                                       │ ai_ready 信号     │
                                       └──────────────────┘

系统可靠性数据流：
┌──────────────┐   采集数据    ┌──────────────┐   HTTP/WS   ┌──────────┐
│ /proc 文件系统 │ ◄───────── │ system-monitor │ ◄────────► │ web-api  │
│ /sys/cgroup   │              │ 内存/CPU/磁盘   │            │ 前端仪表  │
│ /dev/watchdog │              │ 网络/进程       │            │ 盘/告警   │
└──────────────┘              └──────┬─────────┘            └──────────┘
                                     │ 自愈动作
                                     ▼
                              ┌──────────────┐
                              │ 进程重启      │
                              │ 磁盘清理      │
                              │ cgroup v2 资源限制│
                              │ 硬件看门狗喂狗 │
                              └──────────────┘
```

### 1.5 模型 OTA 内部架构 (Phase 3C.2 保留)

```
┌─────────────────────────────────────────────────────────────────┐
│                    update-engine 模型 OTA 子系统                    │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │ Scheduler   │  │  Manager    │  │  Config     │            │
│  │ (定时调度)   │  │ (核心状态机) │  │ (配置管理)   │            │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘            │
│         │                │                │                    │
│         └────────────────┼────────────────┘                    │
│                          ▼                                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│  │ Downloader  │  │  Verifier   │  │ Applicator  │            │
│  │(断点续传)    │  │(签名/哈希)   │  │ (模型应用)   │            │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘            │
│         │                │                │                    │
│         └────────────────┼────────────────┘                    │
│                          ▼                                      │
│                   ┌─────────────┐                              │
│                   │  Rollback   │                              │
│                   │ (回滚管理)   │                              │
│                   └─────────────┘                              │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. 模型 OTA 设计

[DESIGN_APPROVED]

### 2.1 需求概述

Phase 3C 已实现 AI 优化引擎（LSTM 预测、MADDPG/PPO 决策），模型 OTA 在此基础上实现**模型自动更新（OTA）**功能，使 AI 模型能够在现场运行时接收并应用更新，无需人工干预。

**核心目标：**
1. 实现可靠的模型 OTA 更新机制
2. 支持断点续传和增量更新
3. 确保模型安全性（签名验证）
4. 提供自动回滚机制保障业务连续性

**技术栈：**

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

### 2.2 模型版本管理

#### 2.2.1 版本号格式

`major.minor.patch`（例如 `1.2.0`）

#### 2.2.2 存储结构

```
/models/
├── current/                    # 当前运行模型
│   ├── lstm/
│   │   └── model.rknn
│   └── maddpg/
│       └── model.rknn
├── update/                    # 下载的更新包
│   └── v1.2.0/
│       ├── lstm.rknn
│       └── maddpg.rknn
├── rollback/                  # 回滚备份
│   └── v1.1.0/
│       ├── lstm/
│       └── maddpg/
└── version.json               # 版本信息
```

#### 2.2.3 version.json 格式

```json
{
  "lstm": {
    "version": "1.1.0",
    "updated_at": "2026-05-28T10:00:00Z",
    "md5": "abc123...",
    "size": 104857600,
    "path": "/models/current/lstm/model.rknn"
  },
  "maddpg": {
    "version": "1.0.5",
    "updated_at": "2026-05-27T08:30:00Z",
    "md5": "def456...",
    "size": 52428800,
    "path": "/models/current/maddpg/model.rknn"
  }
}
```

### 2.3 模型 OTA 状态机

#### 2.3.1 状态定义

```rust
/// OTA 更新状态 -- 模型 OTA
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OtaState {
    #[default]
    Idle,                                    // 空闲状态
    Checking,                                // 检查更新中
    Downloading { progress: u8 },            // 下载中
    Verifying,                               // 验证中
    Applying,                                // 应用中
    Applied,                                 // 已应用（模型已替换，等待策略引擎加载）
    Completed,                               // 已完成
    RollingBack,                             // 回滚中
    Failed { reason: String },               // 失败
}
```

#### 2.3.2 状态转换图

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
        │COMPLETE  │  │ ROLLING    │     │
        └──────────┘  │ BACK       │     │
                      └──────┬─────┘     │
                             │            │
                             ▼            ▼
                       ┌──────────┐  ┌──────────┐
                       │COMPLETE  │  │  FAILED  │
                       └──────────┘  └──────────┘
```

#### 2.3.3 状态转换规则

| 当前状态 | 事件 | 下一状态 | 动作 |
|---------|------|---------|------|
| Idle | check_updates() | Checking | - |
| Checking | found_update | Downloading | 初始化下载器 |
| Checking | no_update | Idle | - |
| Checking | error | Failed | 记录错误 |
| Downloading | download_complete | Verifying | 停止下载器 |
| Downloading | retry_failed | Failed | 增加重试计数 |
| Downloading | cancel() | Idle | 清理临时文件 |
| Verifying | verify_success | Applying | 启动模型应用器 |
| Verifying | verify_failed | Failed | 清理更新包 |
| Applying | apply_success | Applied | 通知策略引擎加载模型 |
| Applying | apply_failed | RollingBack | 触发回滚 |
| Applied | model_loaded | Completed | 更新 version.json |
| Applied | load_failed | RollingBack | 触发回滚 |
| RollingBack | rollback_success | Idle | 记录回滚事件 |
| RollingBack | rollback_failed | Failed | 进入安全模式 |

### 2.4 更新检查

#### 2.4.1 触发方式

| 方式 | 触发条件 | 优先级 |
|------|----------|--------|
| 定时检查 | 每小时自动检查（可配置） | 低 |
| 手动触发 | 北向收到更新指令 | 高 |
| 启动检查 | 设备启动时检查一次 | 中 |

**性能要求：** 设备能在 **30 秒内**完成一次更新检查。

#### 2.4.2 检查流程

```
1. 连接到 OTA 服务器（地址可配置）
2. 发送当前模型版本信息
3. 服务器返回最新版本信息
4. 比较版本号判断是否需要更新
5. 如果需要更新，下载版本清单
```

### 2.5 更新包下载

#### 2.5.1 下载流程

```
1. 解析版本清单，获取更新包 URL
2. 计算本地可用存储空间（最小 500MB）
3. 下载更新包到临时存储区
4. 支持断点续传（HTTP Range 请求）
5. 下载完成后校验文件哈希
```

#### 2.5.2 断点续传

- 使用 HTTP Range 请求（HTTP 206 Partial Content）
- 记录已下载字节数到本地持久化文件
- 断电恢复后继续下载，已下载部分不丢失
- 临时文件完整性在续传前再次校验，损坏则重新下载
- 连续 10 次断点续传测试均成功

#### 2.5.3 下载参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| timeout | 300 秒 | 单次下载超时 |
| retry_count | 3 | 下载失败重试次数 |
| chunk_size | 1 MB | 分块大小 |
| min_free_space | 500 MB | 最低可用空间 |

### 2.6 增量更新

#### 2.6.1 增量包格式

```
| header(64B) | base_version(16B) | target_version(16B) | diff_data | patch_info |
```

**header 格式：**

| 字段 | 长度 | 说明 |
|------|------|------|
| magic | 4B | 固定值 `OTAP` |
| version | 1B | 增量包格式版本 |
| base_version | 16B | 基础版本号 |
| target_version | 16B | 目标版本号 |
| diff_size | 4B | 差分包大小 |
| checksum | 32B | SHA-256 校验 |

#### 2.6.2 增量更新流程

```
1. 检查当前版本是否支持增量更新
2. 下载增量包
3. 验证增量包版本信息
4. 应用差分补丁生成新模型（使用 bsdiff 算法）
5. 全量校验新模型完整性
```

#### 2.6.3 应用场景

- 模型权重微调（< 10MB 增量）
- 策略参数更新（< 1MB 增量）
- 全量更新（首次安装或跨版本）

#### 2.6.4 性能要求

- 单模型全量更新 < 10 分钟（100MB 模型）
- 单模型增量更新 < 2 分钟（10MB 增量）

### 2.7 更新包验证

#### 2.7.1 验证流程

```
1. 文件完整性校验（SHA-256）
2. 模型签名验证（SM2 / Ed25519）
3. 模型格式校验（RKNN / ONNX）
4. 模型兼容性校验（平台版本匹配）
```

#### 2.7.2 签名验证

- 使用国密 SM2 或 Ed25519 签名算法
- 公钥存储在设备安全区域
- 签名包格式：`| signature(64B) | model_data |`
- 签名验证不通过的更新包 100% 被拒绝

### 2.8 模型应用

#### 2.8.1 应用流程

```
1. 备份当前模型到 rollback 目录
2. 解压更新包到模型目录
3. 通知策略引擎加载新模型
4. 新模型通过 RKNN Runtime 加载
5. 执行模型预热（推理一次）
6. 更新版本记录
```

#### 2.8.2 加载顺序

```
旧模型 → 备份 → 新模型加载 → 预热 → 切换 → 删除旧模型
```

#### 2.8.3 性能要求

- 新模型能在 **60 秒内**通过 RKNN Runtime 加载完成
- 模型推理预热在 **30 秒内**完成

### 2.9 回滚机制

#### 2.9.1 自动回滚触发条件

| 条件 | 阈值 | 说明 |
|------|------|------|
| 新模型加载失败 | - | RKNN Runtime 加载异常 |
| 模型推理失败 | 连续 3 次 | 推理结果异常 |
| 模型校验失败 | - | 签名/哈希校验不通过 |
| 模型预热超时 | 30 秒 | 预热推理超时 |

#### 2.9.2 回滚流程

```
1. 检测到回滚触发条件
2. 停止策略引擎
3. 删除新模型
4. 从 rollback 目录恢复旧模型
5. 重启策略引擎加载旧模型
6. 记录回滚事件到日志
7. 发送回滚通知到北向
```

#### 2.9.3 性能要求

- 新模型加载失败后 **10 秒内**触发自动回滚
- 回滚后设备恢复正常运行
- 回滚通知在 **1 分钟内**发送到北向

#### 2.9.4 回滚限制

- 回滚次数上限：连续 **3 次**
- 超过限制后进入 **安全模式**（使用兜底策略）
- 回滚记录保存 **30 天**
- 回滚成功率 >= 99%

### 2.10 更新策略

#### 2.10.1 定时更新

| 参数 | 默认值 | 说明 |
|------|--------|------|
| check_interval | 3600 秒 | 检查间隔 |
| download_window_start | 02:00 | 下载窗口开始时间 |
| download_window_end | 05:00 | 下载窗口结束时间 |
| auto_download | true | 检查到更新自动下载 |

**定时更新流程：**
```
1. 定时器触发检查
2. 检查当前时间是否在下载窗口内
3. 如果在窗口内，执行更新检查
4. 下载完成，进入等待应用状态
5. 下次设备空闲时应用更新
```

#### 2.10.2 手动更新

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

- 北向收到指令后 **5 秒内**开始下载

### 2.11 与现有模块集成

**与 ai-engine/strategy-engine 集成：**
```rust
// 更新完成后通知 strategy-engine 切换模型
strategy_engine.notify_model_updated(model_type, new_version).await?;
```

**与 gateway 集成：**
```rust
// OTA 状态通过 gateway 北向上报
gateway.report_ota_status(task_id, status, progress).await?;
```

**与 intercore 集成：**
```rust
// 通过 intercore 上报 AI 引擎状态变化
intercore.report_ai_ready(ai_ready).await?;
```

---

## 3. 固件 OTA 设计

### 3.1 A/B 双分区方案

#### 3.1.1 RK3588 分区布局

基于 RK3588 平台 eMMC 存储的物理分区规划（推荐 eMMC >= 32GB）：

```
eMMC 布局:
┌──────────────────────────────────────────────────────────────┐
│ 分区名称        | 大小    | 挂载点          | 文件系统 | 说明  │
├──────────────────────────────────────────────────────────────┤
│ bootloader      | 8MB     | -               | raw     | U-Boot  |
│ env             | 8MB     | -               | raw     | U-Boot env (含 boot_partition) │
│ trust           | 8MB     | -               | raw     | OP-TEE  │
│ boot            | 256MB   | /boot           | ext4    | 内核+DTB+initrd │
│ system-a        | 2GB     | /               | ext4    | 主系统分区 A │
│ system-b        | 2GB     | (备用)          | ext4    | 主系统分区 B │
│ data            | 剩余    | /data           | ext4    | 持久化数据 │
│ ota_scratch     | 512MB   | /ota            | ext4    | OTA 临时文件/差分包 │
│ models          | 1GB     | /models         | ext4    | AI 模型存储 │
│ logs            | 2GB     | /var/log        | ext4    | 系统日志 │
└──────────────────────────────────────────────────────────────┘
```

#### 3.1.2 Bootloader 分区选择逻辑

U-Boot 环境变量 `boot_partition` 控制从哪个分区启动：

```
启动决策树:
1. U-Boot 读取 env 分区中的 `boot_partition` 变量
   ├── "a" → 从 system-a 分区启动
   └── "b" → 从 system-b 分区启动
2. 如果 `boot_partition` 不存在或无效:
   └── 默认从 system-a 分区启动
3. 如果指定分区无法挂载 (e.g. ext4 fs 损坏):
   └── 回退到另一个分区并设置 `ota_status=rollback`
```

**env 分区关键变量定义：**

| 变量名 | 类型 | 说明 |
|--------|------|------|
| `boot_partition` | string | `"a"` 或 `"b"`，当前启动分区 |
| `boot_attempts` | u32 | 当前分区启动尝试次数 |
| `max_boot_attempts` | u32 | 最大启动尝试次数（默认 3） |
| `ota_status` | string | `"idle"`/`"updated"`/`"rollback"`/`"safe"` |

> **Phase 2+ 状态：** A/B 分区切换当前为占位（`switch_to_standby()` 仅记录日志，未通过 BootloaderEnv 实际设置 boot_partition）。下文原子性保证流程为 Phase 2+ 目标设计。

#### 3.1.3 分区切换原子性保证

```
升级切换流程:
1. update-engine 写入 system-b 完成
2. 计算 system-b 全分区 SHA-256 → 写入 /ota/b_checksum
3. 写入 env: boot_partition = "b" (带 fsync)
4. 写入 env: ota_status = "updated" (带 fsync)
5. 如果第 3 步失败 → 保持 "a"，上报 bootloader_update_failed
6. 如果第 4 步失败 → 不影响启动，只是丢失状态标记
```

**关键可靠性措施：**
- U-Boot env 写入后执行 `saveenv` + 等待存储写回确认
- env 分区使用双备份（主 env + 冗余 env），一个损坏时使用另一个
- `boot_partition` 写入前校验 system-b 分区的完整性

### 3.2 `.mupc` 固件包容器格式

统一的固件包容器格式，在单个 `.mupc` 文件中包含元信息、签名、负载数据。

**二进制布局：**
```
┌─────────────────────────────────────────┐
│ Magic:         4B  "MUPC"               │
│ Version:       1B  容器格式版本 (0x01)   │
│ HeaderLen:     4B  LittleEndian u32     │
│ HeaderJSON:    N 字节 JSON 字符串        │
│ Padding:       变长 对齐到 4 字节        │
│ Signature:     64B SM2-with-SM3 签名    │
│ Payload:       N 字节 (tar.gz 或 bsdiff)│
└─────────────────────────────────────────┘
```

#### 头部 JSON 结构 (MupcHeader)

| 字段 | 类型 | 说明 |
|------|------|------|
| `package_type` | string | `"full"` 或 `"incremental"` |
| `target_version` | string | 目标固件版本 (semver) |
| `base_version` | string? | 基准版本 (增量包必填) |
| `checksum` | string | Payload SHA-256 校验和 |
| `file_list` | FileEntry[] | 文件清单 |
| `timestamp` | u64 | 时间戳 (Unix 毫秒) |
| `target_platform` | string | 目标平台（`rk3588-openeuler`） |
| `min_bootloader_version` | string | 最低 bootloader 版本 |

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub mode: u32,        // Unix 文件权限
    pub checksum: String,  // 单个文件 SHA-256
}
```

**payload** 为 tar.gz 格式固件数据（增量包为 bsdiff 格式差分数据）。

**验证要求：**
- 上传完成时 OTA 服务器自动计算 SHA-256
- 设备端下载完成后重新计算 SHA-256，必须与服务器值一致
- 设备端应用固件前，使用 SM2 公钥验证固件包的签名
- SHA-256 或 SM2 签名任一校验失败时，固件包被拒绝并记录错误日志

### 3.3 差分包生成 (bsdiff)

#### 3.3.1 算法选型

| 算法 | 压缩率 (patch版本) | 内存需求 | 速度 | Rust 生态 |
|------|-------------------|---------|------|-----------|
| **bsdiff** | 基准 (patch包约全量30%) | 高 (需加载完整新旧文件) | 快 | `bsdiff-rs` / `bspatch` |
| xdelta3 | 略低于 bsdiff | 中 | 中 | `xdelta3` |
| rdedup | 适合固件有大量重复块 | 中 | 慢 | `rdedup` |

**选型决定：bsdiff** — 二进制差分率最优，patch 版本差分包不超过全量包 30%，
120 秒内生成（基准包 <= 200MB），Rust 生态存在 `bspatch` 库可集成。

#### 3.3.2 bsdiff 应用流程

```
OTA 服务器端 (生成差分包):
1. 获取基准固件包 (base_version) 和目标固件包 (target_version)
2. 调用 bsdiff 生成差分数据
3. 将差分数据打包为 incremental 类型 .mupc 包
4. 签名并发布

设备端 (应用差分包):
1. 下载 incremental .mupc 包
2. 从当前运行分区读取基准固件数据
3. 调用 bspatch 合并 → 得到完整固件数据
4. SHA-256 校验合并结果
5. 将完整固件写入备用分区
```

### 3.4 固件升级状态机

#### 3.4.1 17 状态定义

固件 OTA 状态机与模型 OTA 分离，提供更细粒度的升级流程控制：

```rust
/// 固件 OTA 状态 — 细化状态机，与模型 OTA 状态分离
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FwOtaState {
    Idle,                                         // 空闲
    Checking,                                     // 版本查询中
    WaitingWindow,                                // 等待升级窗口
    PreCheck,                                     // 预检查中
    SendingDowngradeSignal,                       // 发送 ai_ready=false + strategy_mode=basic
    WaitingControlConfirm,                        // 等待实时控制模块确认
    Downloading { progress: u8 },                 // 下载中
    DownloadPaused,                               // 下载暂停（等待重试）
    IntegrityCheck,                               // SHA-256 完整性验证中
    SignatureVerify,                              // SM2 签名验证中
    WritingPartition,                             // 写入备用分区
    PartitionVerify,                              // 分区验证中
    UpdatingBootloader,                           // 更新 Bootloader 配置
    WaitingReboot,                                // 等待重启
    PostVerify,                                   // 重启后验证中
    MarkingSuccess,                               // 标记升级成功
    SendingRestoreSignal,                         // 发送 ai_ready=true 恢复信号
    RollingBack,                                  // 回滚中
    Completed,                                    // 升级成功完成
    Failed { error: String, phase: String },      // 失败
    SafeMode,                                     // 安全模式（连续 3 次失败）
}
```

#### 3.4.2 完整状态转换图

```
                ┌──────────────────────────────────────────────┐
                │                                              │
                ▼                                              │
          ┌──────────┐                                        │
          │   Idle   │ ◄──────────────────────────────┐       │
          └────┬─────┘                                │       │
               │ 检查更新                               │       │
               ▼                                       │       │
          ┌──────────┐                                │       │
          │ Checking │                                │       │
          └────┬─────┘                                │       │
               │ 有可用固件                              │       │
               ▼                                       │       │
          ┌──────────────┐                             │       │
          │ WaitingWindow│ ◄──── 不在升级窗口内，等待     │       │
          └──────┬───────┘                             │       │
                 │ 在窗口内                              │       │
                 ▼                                     │       │
          ┌──────────┐                                 │       │
          │ PreCheck │ ◄──── 检查失败→Failed            │       │
          └────┬─────┘                                 │       │
               │ 所有检查通过                            │       │
               ▼                                       │       │
    ┌─────────────────────┐                            │       │
    │ SendingDowngradeSig │                            │       │
    └──────────┬──────────┘                            │       │
               │ intercore 确认                         │       │
               ▼                                       │       │
    ┌──────────────────────┐                           │       │
    │ WaitingControlConfirm│                           │       │
    └──────────┬───────────┘                           │       │
               │ 收到确认                               │       │
               ▼                                       │       │
          ┌──────────────┐                             │       │
          │ Downloading  │──── 失败/暂停 → DownloadPaused│       │
          └──────┬───────┘        │ (重试3次)           │       │
                 │ 下载完成       ▼                     │       │
                 │            ┌────────────────┐       │       │
                 │            │DownloadPaused   │       │       │
                 │            └───────┬────────┘       │       │
                 │               重试成功               │       │
                 ▼                   │                  │       │
          ┌────────────────┐         │                  │       │
          │ IntegrityCheck │◄────────┘                  │       │
          └──────┬─────────┘                            │       │
                 │ SHA-256 通过                          │       │
                 ▼                                       │       │
          ┌────────────────┐                             │       │
          │ SignatureVerify│                             │       │
          └──────┬─────────┘                             │       │
                 │ SM2 验证通过                           │       │
                 ▼                                       │       │
          ┌──────────────────┐                           │       │
          │ WritingPartition │                           │       │
          └──────┬───────────┘                           │       │
                 │ 写入完成                               │       │
                 ▼                                       │       │
          ┌──────────────────┐                           │       │
          │ PartitionVerify  │                           │       │
          └──────┬───────────┘                           │       │
                 │ SHA-256 匹配                           │       │
                 ▼                                       │       │
          ┌─────────────────────┐                        │       │
          │ UpdatingBootloader  │                        │       │
          └──────┬──────────────┘                        │       │
                 │ env 写入并 fsync 确认                    │       │
                 ▼                                       │       │
          ┌──────────────┐                               │       │
          │ WaitingReboot│                               │       │
          └──────┬───────┘                               │       │
                 │ 系统重启 (由外部触发)                     │       │
                 ▼                                       │       │
          ┌──────────────┐                               │       │
          │ PostVerify   │─── 所有验证通过                  │       │
          └──┬───┬───────┘                               │       │
     失败触发 │    │ 通过                                 │       │
       回滚   │    ▼                                     │       │
             │  ┌────────────────┐                       │       │
             │  │ MarkingSuccess │                       │       │
             │  └──────┬─────────┘                       │       │
             │         ▼                                 │       │
             │  ┌────────────────────────┐               │       │
             │  │ SendingRestoreSignal   │               │       │
             │  │ ai_ready=true           │               │       │
             │  └──────┬─────────────────┘               │       │
             │         ▼                                 │       │
             │    ┌──────────┐                            │       │
             │    │Completed │────────────────────────────┘       │
             │    └──────────┘                                    │
             │                                                    │
             ▼                                                    │
       ┌──────────────┐                                           │
       │ RollingBack  │                                           │
       └──┬───┬───────┘                                           │
          │   │                                                    │
          │   ▼                                                    │
          │ ┌───────────┐                                         │
          │ │ SafeMode  │ (连续3次失败)                             │
          │ └───────────┘                                         │
          ▼                                                        │
    ┌──────────┐                                                   │
    │ Failed   │───────────────────────────────────────────────────┘
    └──────────┘
```

#### 3.4.3 状态转换守卫

- **PreCheck 守卫**：检查连续失败次数，连续 3 次进入 SafeMode
- **Downloading 守卫**：检查当前时间是否在下载窗口内
- **任意状态 → Failed**：超时或失败，带阶段标识

### 3.5 升级前检查

设备在应用固件升级前执行检查组，任一检查项不通过则终止升级。

| 检查项 | 阈值 | 不通过处理 |
|--------|------|-----------|
| 磁盘剩余空间 | >= 500MB | 终止升级并上报 |
| 电池/电源状态 | 非电池供电或电池电量 >= 30% | 终止升级并上报 |
| CPU 负载 | <= 80% | 等待并重试，超时 5 分钟后终止 |
| 系统进程健康 | 所有关键进程运行正常 | 终止升级 |
| 实时控制模块状态 | 心跳正常 | 终止升级 |
| 固件兼容性 | 平台字段匹配 `rk3588-openeuler` | 终止升级 |
| 升级窗口 | 当前时间在配置的升级窗口内 | 等待至窗口时间 |

**性能要求：** 检查清单全部项目在 **30 秒内**执行完毕。

### 3.6 升级后验证

设备从新分区启动后，运行升级后验证。验证通过则标记升级成功；验证不通过则自动触发回滚。

| 验证项 | 通过条件 |
|--------|---------|
| 固件版本 | `cat /etc/mupc-version` 输出的版本号等于目标版本 |
| 关键进程存活 | gateway、intercore、strategy-engine、data-processing 四进程运行中 |
| 核间通信 | 与实时控制模块的心跳回复在 3 秒内 |
| 北向连接 | IEC 104 网关 TCP 连接建立成功 |
| 日志系统 | tracing 日志正常输出至 `/var/log/mupc/` |
| AI 引擎 | ai-engine 进程（如存在）运行中，推理接口正常 |
| 南向通信 | rs485-plugin（如配置）守护线程运行中 |

**性能要求：** 升级后验证在 **60 秒内**完成。

### 3.7 升级失败回滚与恢复

#### 3.7.1 A/B 分区回滚

- 回滚指令触发后，bootloader 配置在 **5 秒内**恢复指向原分区
- 回滚后系统重启并在 **120 秒内**恢复至升级前运行状态
- 回滚前保留 B 分区数据（用于离线分析）
- B 分区保留时间：**7 天**或直到下次升级
- 连续 **3 次**升级失败后，系统进入安全模式：停止 OTA 自动检查，仅允许手动确认后升级

#### 3.7.2 故障恢复场景

| 故障场景 | 系统行为 | 验收条件 |
|----------|---------|----------|
| 升级过程中掉电 | 上电后从原分区启动，下载进度和临时文件保留 | 恢复后 120 秒内进入正常运行状态 |
| B 分区写入过程中掉电 | 上电后 bootloader 检查分区状态，回退至 A 分区 | A 分区完整可启动，不受影响 |
| bootloader 配置写入失败 | A/B 分区状态均未变更，从 A 分区正常启动 | 上报 `bootloader_update_failed` 告警 |
| 新分区启动后关键进程崩溃 | 升级后验证失败，自动回滚至 A 分区 | 回滚后系统运行正常，上报回滚记录 |

### 3.8 分批升级策略（灰度发布）

#### 3.8.1 升级批次管理

| 维度 | 要求 |
|------|------|
| 批次数量 | 支持最多 **10** 个批次 |
| 每批次设备数 | 1 ~ 10,000 台 |
| 批次间隔 | 可配置：0 ~ 168 小时（7 天） |
| 观察期 | 可配置：0 ~ 168 小时 |
| 暂停 | 管理员可在任意时刻暂停整个灰度计划 |
| 回退 | 暂停后支持一键回退所有已升级设备至升级前版本 |
| 自动暂停条件 | 升级失败率 >= **10%** 或回滚率 >= **5%** |

#### 3.8.2 升级指令与状态上报

- 升级指令下发方式支持：MQTT 消息、IEC 104 遥控、定时轮询
- 设备端从收到指令到开始下载的延迟 <= **10 秒**
- 状态上报间隔：下载阶段每 **10 秒**上报一次进度，其他阶段每 **30 秒**一次
- 状态上报字段：`task_id`、`state`、`progress`（0-100）、`error_message`、`estimated_remaining_seconds`、`current_partition`（A/B）

---

## 4. 系统监控设计

> **Phase 2+ 状态：** `system-monitor` crate 当前为骨架实现（JSONL 文件存储指标，无 cgroup v2 资源限制、无网络 I/O 监控、无硬件看门狗喂狗、无 OOM 保护）。下文五维监控、cgroup v2、硬件看门狗、OOM 保护等为 Phase 2+ 目标设计。

### 4.1 整体架构

system-monitor crate 采用**采集器-分析器-自愈**三层架构：

```
┌───────────────────────────────────────────────────────────────────┐
│                    system-monitor Daemon                           │
│                                                                   │
│  ┌─────────────┐    ┌──────────────┐    ┌──────────────────────┐  │
│  │ 采集调度器    │───►│ 分析管道      │───►│ 告警/自愈/存储       │  │
│  │ (tokio定时器) │    │ (channels)   │    │ (fan-out)           │  │
│  └──────┬──────┘    └──────────────┘    └──────────────────────┘  │
│         │                                                        │
│  ┌──────▼─────────────────────────────────────────────────────┐  │
│  │ 采集器池 (tick 调度)                                        │  │
│  │  ┌─────────┐ ┌──────────┐ ┌────────┐ ┌─────────┐ ┌──────┐│  │
│  │  │ Memory  │ │ Process  │ │ Disk   │ │ CPU     │ │Network││  │
│  │  │ 30s     │ │ 15s      │ │ 60s    │ │ 30s     │ │60s   ││  │
│  │  └─────────┘ └──────────┘ └────────┘ └─────────┘ └──────┘│  │
│  └────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────┘
```

**守护进程主结构：**

```rust
pub struct SystemMonitorDaemon {
    config: MonitorConfig,
    collectors: Vec<Box<dyn Collector>>,       // 采集层
    analyzers: Vec<Box<dyn Analyzer>>,          // 分析层
    heal_engine: Arc<SelfHealEngine>,           // 自愈层
    mtbf_engine: Arc<MtbfEngine>,               // MTBF 计算
    storage: Arc<TimeseriesDb>,                 // 存储层
    reporter: Arc<WebApiReporter>,              // 上报层
    watchdog: Option<WatchdogFeeder>,           // 看门狗
    shutdown: tokio::sync::watch::Sender<bool>,
}
```

### 4.2 五维监控指标

#### 4.2.1 内存监控 (30s 周期)

| 指标 | 采集频率 | 单位 | 告警阈值 |
|------|---------|------|---------|
| 系统总内存使用率 | 30 秒 | % | 连续 5 次 >= 85%（WARNING）；>= 92%（CRITICAL） |
| 各进程 RSS | 30 秒 | MB | 超配额的 1.5 倍且持续 3 分钟 |
| Swap 使用量 | 60 秒 | MB | > 0（WARNING）；>= 512MB（CRITICAL） |
| OOM Killer 事件 | 事件驱动 | - | 每次 OOM 触发 CRITICAL 告警 |
| 堆内存分配增量 | 60 秒 | MB/小时 | 持续 6 小时增长趋势 > 5%（疑似泄漏） |

**内存泄漏检测：** 基于进程 RSS 长时间序列分析，使用线性回归斜率判断增长趋势，
置信度要求 R^2 >= 0.7。6 小时内持续增长超过 5% 且无下降趋势时，
标记为疑似内存泄漏并自动重启该进程。

- 疑似泄漏判定后 **30 秒内**记录告警，保存 `/proc/[pid]/smaps` 快照
- 自动重启前等待 **120 秒**（给管理员手动干预窗口）
- 同一进程 **24 小时内**最多自动重启 **3 次**

#### 4.2.2 进程监控 (15s 周期)

- 关键进程列表：gateway、intercore、strategy-engine、data-processing、web-api、ai-engine（如启用）、rs485-plugin（如启用）、mqtt-plugin（如启用）
- 检测方法：pid 文件 + `/proc/[pid]/status` 状态检查，二者均通过才判定存活
- 每个关键进程存活检查时间 <= 200ms
- 进程缺失 **15 秒内**产生 WARNING 事件
- 进程缺失 **30 秒后**触发自动重启
- 从检测到进程缺失到自动重启成功，总耗时 <= **60 秒**
- 单进程连续 **5 次**重启失败后，不再自动重启，升级为 CRITICAL 告警

#### 4.2.3 磁盘监控 (60s 周期)

| 分区 | 告警阈值 | 处置动作 |
|------|---------|---------|
| `/` | >= 85%（WARNING）；>= 92%（CRITICAL） | CRITICAL：自动清理临时文件 |
| `/var/log` | >= 80%（WARNING）；>= 90%（CRITICAL） | CRITICAL：触发日志轮转压缩 |
| `/opt/mupc` | >= 85%（WARNING）；>= 92%（CRITICAL） | CRITICAL：停止非关键数据写入 |
| `/models` | >= 85%（WARNING） | 禁止新的模型/固件下载 |
| 系统 inode | >= 85%（WARNING） | 检查小文件堆积并触发清理 |

**自动磁盘分级清理：**
- 第一级（WARNING）：轮转并压缩 **7 天前**的日志文件，保留期限 **90 天**
- 第二级（CRITICAL）：删除 **30 天前**的日志文件
- 第三级（CRITICAL 持续 1 小时）：删除 /tmp 下超过 24 小时未访问的临时文件

#### 4.2.4 CPU 监控 (30s 周期)

| 指标 | 采集频率 | 告警阈值 |
|------|---------|---------|
| 系统总 CPU 使用率 | 30 秒 | 连续 5 次 >= 90% |
| 单进程 CPU 使用率 | 30 秒 | 超过配额 3 倍持续 5 分钟 |
| 1 分钟平均负载 | 30 秒 | >= CPU 核心数 * 2 |
| 15 分钟平均负载 | 30 秒 | >= CPU 核心数 * 1.5 持续 30 分钟 |

#### 4.2.5 网络监控 (60s 周期)

| 指标 | 采集频率 | 告警阈值 |
|------|---------|---------|
| 北向接口收发带宽 | 60 秒 | 使用率 >= 80% 持续 5 分钟 |
| TCP 重传率 | 60 秒 | >= 5% 持续 3 分钟 |
| 核间通信延迟 | 15 秒（ping 心跳） | >= 100ms（WARNING）；>= 500ms（CRITICAL） |
| TCP 连接计数 | 60 秒 | 活跃连接 >= 100 个 |

### 4.3 采集器统一接口

```rust
/// 采集器统一接口
#[async_trait]
pub trait Collector: Send + Sync {
    fn name(&self) -> &'static str;
    fn interval(&self) -> Duration;
    async fn collect(&self) -> Result<Vec<MetricSample>>;
}

/// 指标样本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub timestamp: u64,
    pub metric_name: String,
    pub value: f64,
    pub unit: String,
    pub labels: HashMap<String, String>,
}
```

### 4.4 时序数据存储

基于 SQLite 轮转存储，保留期 30 天，每日凌晨清理。

```sql
CREATE TABLE metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,
    metric_name TEXT NOT NULL,
    value REAL NOT NULL,
    unit TEXT NOT NULL,
    labels TEXT  -- JSON
);
CREATE INDEX idx_metric_timestamp ON metrics(metric_name, timestamp);
```

### 4.5 告警系统

**告警去重：** 同一指标同级别 1 小时内只发送一次。

**告警分发链路：**
1. 去重检查
2. 记录到本地告警日志
3. 通过 web-api REST 接口暴露
4. 通过 WebSocket 实时推送到前端

### 4.6 数据保留策略

| 数据类型 | 保留期限 | 清理策略 |
|---------|---------|---------|
| 内存/CPU/磁盘监控历史 | **30 天** | 每日凌晨清理 |
| 进程重启记录 | **90 天** | 按时间戳轮转 |
| 模型 OTA 更新历史 | **30 天** | 按记录数轮转 |
| 固件升级历史 | **3 年** | 最多保留 1000 条 |
| 告警记录 | **1 年** | 按日期分文件 |
| 运行日志（tracing） | **90 天** | 每日轮转，压缩后保留 |
| B 分区保留（回滚前） | **7 天**或到下次升级 | 空间回收 |

---

## 5. MTBF 计算设计

### 5.1 uptime 追踪

系统守护进程记录每次启动时间、上次关机时间、原因（正常关机 / 异常重启 / 看门狗复位）。
根据运行时间和异常中断次数计算 MTBF。

```rust
/// uptime 历史记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UptimeRecord {
    pub boot_time: u64,
    pub shutdown_time: Option<u64>,
    pub shutdown_type: ShutdownType,
    pub running_duration: Option<u64>,
}

/// 关机类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShutdownType {
    Normal,     // 正常关机/重启
    Crash,      // 异常崩溃
    Watchdog,   // 看门狗复位
    Oom,        // OOM 被杀
    Panic,      // 内核 panic
    PowerFail,  // 掉电
}
```

**文件路径：** `/var/lib/mupc/systemd/uptime_history.json`
**启动时记录：** boot_time、shutdown_type
**异常重启 100% 被检测并分类**

### 5.2 故障计数器

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FaultType {
    ProcessCrash,    // 进程崩溃
    OomKill,         // OOM 杀死
    WatchdogReset,   // 看门狗复位
    KernelPanic,     // 内核恐慌
    PowerLoss,       // 掉电
    SystemHang,      // 系统挂起
}
```

**文件路径：** `/var/lib/mupc/systemd/fault_history.json`

### 5.3 MTBF 计算公式

```
MTBF = sum(running_durations) / crash_count
```

- **滚动窗口**：最近 **365 天**
- **计算频率**：每 **24 小时**计算一次
- **crash_count**：窗口内异常中断次数（排除正常关机）
- 当 crash_count = 0 时，MTBF = 当前总运行时间

**分类：**

| MTBF 值 | 状态 |
|---------|------|
| >= 50,000 小时 | Healthy |
| 10,000 ~ 50,000 小时 | Warning |
| < 10,000 小时 | Critical — 触发告警 |
| 数据不足 | InsufficientData |

**性能要求：**
- 正常运行累计时间精度：秒级
- MTBF 值每 **24 小时**计算一次并写入 `/var/lib/mupc/systemd/mtbf.json`
- 当 MTBF < **10,000 小时**时，触发 WARNING 告警

### 5.4 MTBF 目标汇总

| 指标 | 目标值 | 测量方法 |
|------|-------|---------|
| MTBF | >= **50,000 小时** | 最近 365 天滚动计算 |
| 单次异常恢复时间 | <= **120 秒**（自动恢复） | 从异常发生到关键进程全部恢复 |
| 计划内宕机（升级） | 每年不超过 **4 次** | 年度累计升级次数 |
| 非计划宕机 | 每年不超过 **2 次** | 异常崩溃/重启次数 |

---

## 6. 异常自愈设计

### 6.1 硬件看门狗

系统启用硬件看门狗（`/dev/watchdog`）。守护进程每 30 秒喂狗一次。

```rust
pub struct WatchdogFeeder {
    device_path: PathBuf,           // /dev/watchdog
    feed_interval: Duration,        // 30s
    timeout: Duration,              // 60s
    consecutive_failures: AtomicU32,
}
```

- 看门狗超时时间：**60 秒**
- 守护进程正常时每 **30 秒**写入 `/dev/watchdog`（写入魔术字符 `"V"`）
- 守护进程连续 **3 次**喂狗失败后，由看门狗自动复位
- 复位后系统启动时记录 `shutdown_type = watchdog` 到 uptime 历史

### 6.2 cgroup v2 资源限制

通过 cgroup v2 设置进程级资源限制，防止单进程耗尽全系统资源。

```
cgroup 配置路径: /sys/fs/cgroup/mupc/
```

| 资源类型 | 限制 | 超限处置 |
|---------|------|---------|
| 进程 RSS（按角色） | gateway: 256MB, intercore: 128MB, strategy-engine: 512MB, data-processing: 256MB, ai-engine: 1024MB, web-api: 128MB | 打印堆栈后重启该进程 |
| 文件描述符数（单进程） | 4096 | 记录 WARNING 告警 |
| 打开文件数（全系统） | 65536 | 记录 CRITICAL 告警 |
| /tmp 占用 | 1GB | 清理 24 小时前临时文件 |
| /var/log 单文件大小 | 100MB | 自动轮转 |

- 所有资源限制通过 cgroup v2 在守护进程启动时设置
- 超限处置在检测到时间点 **10 秒内**执行

### 6.3 OOM 保护

配置 `oom_score_adj` 确保守护进程和关键网络进程在 OOM 时不被优先杀死。

| 进程角色 | oom_score_adj | 说明 |
|---------|--------------|------|
| gateway, intercore | **-500** | 最低被杀概率 |
| strategy-engine, ai-engine | **-200** | 重要决策进程 |
| data-processing, web-api | **0** | 中等优先级 |
| 辅助进程（日志轮转等） | **500** | 优先被杀 |

- OOM 事件发生后 **30 秒内**产生告警，记录被杀死进程名、RSS 使用量、系统可用内存

### 6.4 进程自动重启

```rust
pub struct ProcessRestarter {
    processes: RwLock<HashMap<String, ProcessEntry>>,
    alert_dispatcher: AlertDispatcher,
}
```

**重启流程：**
1. 检查重启次数限制（每日上限 3 次，连续失败上限 5 次）
2. 停止进程（SIGTERM → 等待 5s → SIGKILL）
3. 清理资源
4. 重新启动
5. 确认存活
6. 更新计数器

**批量重启优先级：** gateway（最高）> intercore > strategy-engine > data-processing > web-api

### 6.5 磁盘自动清理

分级清理策略，在磁盘空间不足时自动触发：

- **第一级**（WARNING 触发）：轮转并压缩 7 天前的日志文件，保留 90 天
- **第二级**（CRITICAL 触发）：删除 30 天前的日志文件
- **第三级**（CRITICAL 持续 1 小时）：删除 /tmp 下超过 24 小时未访问的临时文件

### 6.6 系统可靠性边界条件

| 场景 | 系统行为 |
|------|---------|
| 内存泄漏累积 72 小时未重启 | 进程 RSS 超限触发自动重启；ai-engine 先热切换至兜底策略再重启 |
| 同时 3 个进程崩溃 | 守护进程按优先级排序重启（gateway 最高），全部在 120 秒内恢复 |
| 磁盘写入失败（设备故障） | 降级运行：停止日志写入（降级为 stderr），继续执行控制指令 |
| 核间通信中断 | 守护进程连续 3 次心跳未回复后，发送复位信号至实时控制模块 |
| 多次自动重启仍失败 | 单进程连续 5 次重启失败后，转为 CRITICAL 告警，等待管理员介入 |
| 守护进程自身崩溃 | 硬件看门狗在 60 秒后复位系统 |
| /var 分区只读 | 守护进程降级输出至 syslog，核心控制功能不中断 |

---

## 7. 与 intercore 协同设计

### 7.1 升级前信号发送

固件升级开始前，ota-update 通过 intercore 向实时控制模块发送降级信号：

```rust
pub struct UpgradeSignalManager {
    intercore_client: IntercoreClient,
}

impl UpgradeSignalManager {
    /// 发送降级信号（升级前）
    /// 1. strategy_mode = "basic"   → 实时控制模块切换至兜底策略
    /// 2. ai_ready = false          → 实时控制模块停止等待 AI 决策
    /// 3. 等待实时控制模块确认（超时 10 秒）
    pub async fn send_downgrade_signals(&self) -> Result<()>;

    /// 发送恢复信号（升级成功验证后）
    /// 1. ai_ready = true
    /// 2. strategy_mode = "smart"   → 恢复正常智能模式
    pub async fn send_restore_signals(&self) -> Result<()>;
}
```

**帧格式（复用 intercore 现有协议）：**

```json
{
  "cmd": "set_strategy_mode",
  "mode": "basic"          // 或 "smart"（恢复时）
}
{
  "ai_ready": false         // 或 true（恢复时）
}
```

### 7.2 核间通信恢复

升级重启后，ota-update 自动重建核间通信连接：

| 参数 | 值 |
|------|-----|
| connect_timeout | 10 秒 |
| retry_interval | 3 秒 |
| max_retries | 5 次 |

### 7.3 升级期间 intercore 信号概要

| 阶段 | 发送信号 | 说明 |
|------|---------|------|
| 升级前检查 | 查询 intercore 心跳 | 确认实时控制模块在线 |
| 升级开始 | strategy_mode = basic | 切换至兜底策略 |
| 升级开始 | ai_ready = false | 停止 AI 决策 |
| 等待确认 | - | 等待实时控制模块 ACK（10s 超时） |
| 升级后验证 | 查询 intercore 心跳 | 确认核间通信恢复 |
| 升级完成 | ai_ready = true | 恢复 AI 决策 |
| 升级完成 | strategy_mode = smart | 恢复智能模式 |

### 7.4 与现有系统集成点总览

| 集成点 | 说明 |
|--------|------|
| SM2 签名验证 | 固件 OTA 复用 security crate 的 SM2 签名验证能力 |
| 公钥管理 | 独立 SM2 密钥对，公钥路径 `/etc/mupc/security/ota_public_key.pem` |
| 审计日志 | 所有固件升级操作写入安全审计日志 |
| intercore 升级前通知 | 发送 `strategy_mode = basic` + `ai_ready = false` |
| intercore 升级后恢复 | 发送 `ai_ready = true` + `strategy_mode = smart` |
| web-api REST API | 升级状态、资源监控、MTBF 报告 |
| OTA 服务器 HTTP API | 版本查询、下载、状态上报、灰度指令 |

---

## 8. 接口定义

### 8.1 错误类型

```rust
/// OTA 更新错误类型
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

    #[error("状态机错误: {0}")]
    StateMachineError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("无效状态转换: 从 {from} 到 {to}")]
    InvalidStateTransition { from: String, to: String },

    #[error("安全模式已激活")]
    SafeModeEngaged,

    #[error("核间通信重连失败: {0}")]
    IntercoreReconnectFailed(String),
}
```

### 8.2 模型 OTA 核心类型

```rust
/// 模型类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    #[serde(rename = "lstm")]
    Lstm,
    #[serde(rename = "maddpg")]
    Maddpg,
}

/// 模型版本信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelVersion {
    pub model_type: ModelType,
    pub version: String,
    pub updated_at: DateTime<Utc>,
    pub md5: String,
    pub size: u64,
    pub path: PathBuf,
}

/// 更新信息
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// 更新记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateRecord {
    pub task_id: String,
    pub model_type: ModelType,
    pub from_version: String,
    pub to_version: String,
    pub status: OtaState,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}
```

### 8.3 OTA 管理器接口

```rust
#[async_trait]
pub trait OtaManager: Send + Sync {
    fn get_current_version(&self, model_type: ModelType) -> Result<ModelVersion, OtaError>;

    async fn check_updates(&self) -> Result<Vec<UpdateInfo>, OtaError>;

    async fn start_download(&self, update_info: &UpdateInfo) -> Result<String, OtaError>;

    fn get_download_progress(&self, task_id: &str) -> Result<u8, OtaError>;

    async fn cancel_download(&self, task_id: &str) -> Result<(), OtaError>;

    async fn apply_update(&self, task_id: &str) -> Result<(), OtaError>;

    async fn rollback(&self, model_type: ModelType) -> Result<(), OtaError>;

    fn get_state(&self) -> OtaState;

    fn get_current_task(&self) -> Option<OtaTask>;

    fn get_update_history(&self, limit: usize) -> Result<Vec<UpdateRecord>, OtaError>;

    fn query_versions(&self) -> Result<Vec<VersionQueryResponse>, OtaError>;

    async fn handle_command(&self, cmd: OtaUpdateCommand) -> Result<OtaUpdateResponse, OtaError>;
}
```

### 8.4 北向通信接口

```rust
/// OTA 指令（接收）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaUpdateCommand {
    pub cmd: String,
    pub task_id: String,
    pub model_type: ModelType,
    pub version: String,
    pub url: String,
    pub signature: String,
    pub checksum: String,
}

/// OTA 响应（发送）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaUpdateResponse {
    pub task_id: String,
    pub model_type: ModelType,
    pub status: OtaState,
    pub progress: Option<u8>,
    pub error_message: Option<String>,
}

/// 版本查询响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionQueryResponse {
    pub models: Vec<ModelVersion>,
    pub device_id: String,
    pub timestamp: DateTime<Utc>,
}
```

### 8.5 配置接口

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaConfig {
    pub server_url: String,
    pub check_interval: u64,           // 检查间隔（秒）
    pub download_window_start: String, // "02:00"
    pub download_window_end: String,   // "05:00"
    pub auto_download: bool,
    pub auto_apply: bool,
    pub download_timeout: u64,
    pub retry_count: u32,
    pub max_rollback_count: u32,
    pub public_key_path: String,
    pub model_storage_path: PathBuf,
    pub min_free_space: u64,
    pub warmup_timeout_secs: u64,
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
            model_storage_path: PathBuf::from("/models"),
            min_free_space: 500 * 1024 * 1024,
            warmup_timeout_secs: 30,
        }
    }
}
```

### 8.6 固件 OTA 接口

#### 8.6.1 PartitionManager

```rust
pub struct PartitionManager {
    current: PartitionInfo,
    standby: PartitionInfo,
    env_device: String,   // "/dev/mmcblk0p2" (env 分区)
}

impl PartitionManager {
    /// 检测当前和备用分区
    pub fn detect() -> Result<Self>;

    /// 挂载备用分区
    pub async fn mount_standby(&self) -> Result<MountGuard>;

    /// 写入备用分区
    pub async fn write_standby(&self, payload: &[u8], file_list: &[FileEntry]) -> Result<()>;

    /// 验证备用分区完整性
    pub async fn verify_standby_integrity(&self, expected_checksum: &str) -> Result<bool>;

    /// 切换 boot_partition 到备用分区
    pub async fn switch_to_standby(&self) -> Result<()>;

    /// 回滚到原分区
    pub async fn rollback_to_current(&self) -> Result<()>;
}
```

#### 8.6.2 BootloaderEnv

```rust
pub struct BootloaderEnv {
    env_device: String,
    use_tools: bool,
}

impl BootloaderEnv {
    pub fn read(&self, key: &str) -> Result<Option<String>>;
    pub fn write(&self, key: &str, value: &str) -> Result<()>;
    pub fn batch_write(&self, pairs: &[(&str, &str)]) -> Result<()>;
    pub fn current_boot_partition(&self) -> Result<String>;
    pub fn set_boot_partition(&self, partition: &str) -> Result<()>;
}
```

#### 8.6.3 SM2 验证器

```rust
pub struct Sm2FirmwareVerifier {
    public_key_path: PathBuf,
    public_key: Option<Vec<u8>>,
}

impl Sm2FirmwareVerifier {
    pub fn new(public_key_path: PathBuf) -> Result<Self>;

    /// 验证 .mupc 包签名 (SM2-with-SM3)
    pub async fn verify_package(&self, package_data: &[u8], signature: &[u8; 64]) -> Result<bool>;

    /// 更新公钥（自身也需要签名验证）
    pub async fn update_public_key(&self, new_key_pem: &str, signature: &[u8; 64], old_key: &Sm2FirmwareVerifier) -> Result<()>;
}
```

#### 8.6.4 断点续传持久化

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub url: String,
    pub target_version: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub temp_file: String,
    pub checksum: String,
    pub started_at: u64,
    pub last_updated: u64,
}

pub struct DownloadProgressManager {
    progress_file: PathBuf,  // /var/lib/mupc/ota/download_progress.json
}

impl DownloadProgressManager {
    pub async fn save(&self, progress: &DownloadProgress) -> Result<()>;
    pub async fn load(&self) -> Result<Option<DownloadProgress>>;
    pub async fn clear(&self) -> Result<()>;
    pub async fn resume(&self, downloader: &Downloader, url: &str) -> Result<Option<DownloadResult>>;
}
```

### 8.7 系统监控接口

```rust
/// 采集器统一接口
#[async_trait]
pub trait Collector: Send + Sync {
    fn name(&self) -> &'static str;
    fn interval(&self) -> Duration;
    async fn collect(&self) -> Result<Vec<MetricSample>>;
}

/// 分析器接口
#[async_trait]
pub trait Analyzer: Send + Sync {
    fn name(&self) -> &'static str;
    async fn analyze(&self, samples: &[MetricSample]) -> Result<Vec<AlertEvent>>;
}

/// 指标样本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub timestamp: u64,
    pub metric_name: String,
    pub value: f64,
    pub unit: String,
    pub labels: HashMap<String, String>,
}

/// 告警事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub id: String,
    pub timestamp: u64,
    pub level: AlertLevel,
    pub metric_name: String,
    pub current_value: f64,
    pub threshold: f64,
    pub duration_seconds: u64,
    pub message: String,
    pub affected_processes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertLevel {
    Warning,
    Critical,
}
```

### 8.8 MTBF 接口

```rust
/// MTBF 计算结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtbfReport {
    pub report_time: u64,
    pub period_start: u64,
    pub period_end: u64,
    pub total_running_duration: u64,
    pub crash_count: u32,
    pub mtbf_hours: f64,
    pub fault_distribution: HashMap<String, u32>,
    pub current_uptime: u64,
    pub status: MtbfStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MtbfStatus {
    Healthy,             // MTBF >= 50,000
    Warning,             // 10,000 < MTBF < 50,000
    Critical,            // MTBF < 10,000
    InsufficientData,    // 数据不足
}
```

### 8.9 OTA 服务器接口

| 接口 | 说明 |
|------|------|
| `GET /api/v1/firmware/versions?device_id={id}&current_version={ver}` | 版本查询 |
| `GET /api/v1/firmware/download/{version}` | 下载固件包（支持 HTTP Range） |
| `POST /api/v1/firmware/status` | 状态上报 |
| `POST /api/v1/devices/{id}/firmware/update` | 灰度指令 |
| `GET /api/v1/firmware/batches?plan_id={plan_id}` | 批次查询 |

### 8.10 Web API REST 接口

| 接口 | 说明 |
|------|------|
| `GET /api/v1/ota/firmware/status` | 升级状态查询 |
| `GET /api/v1/ota/firmware/history` | 升级历史查询 |
| `GET /api/v1/system/resources` | 资源监控指标（内存/CPU/磁盘/网络） |
| `GET /api/v1/system/mtbf` | MTBF 报告 |
| WebSocket | 升级进度/告警事件实时推送 |

---

## 9. 文件结构

### 9.1 update-engine crate 文件结构 （改造 ota-update）

```
update-engine/
├── Cargo.toml
└── src/
    ├── lib.rs                          # 模块导出
    ├── config.rs                       # 增强配置（+固件OTA参数）
    ├── error.rs                        # 增强错误类型
    ├── types.rs                        # 统一类型（固件+模型）
    │
    ├── firmware/                       # [新增] 固件升级
    │   ├── mod.rs
    │   ├── partition.rs                # A/B 分区管理
    │   ├── bootloader.rs               # Bootloader env 操作
    │   ├── mupc_package.rs             # .mupc 容器格式解析
    │   └── bsdiff_applier.rs           # [新增] bsdiff 差分包应用
    │
    ├── download/                       # 保留/增强
    │   ├── mod.rs
    │   ├── downloader.rs               # 增强：限速、断点续传持久化
    │   └── progress_manager.rs         # 下载进度持久化管理
    │
    ├── verify/                         # 保留/增强
    │   ├── mod.rs
    │   ├── verifier.rs                 # 增强：SM2 + SHA-256
    │   └── sm2_verifier.rs             # [新增] 独立SM2验证封装
    │
    ├── state_machine/                  # [新增] 固件升级状态机
    │   ├── mod.rs
    │   ├── fw_state.rs                 # 固件专用状态定义
    │   └── fw_machine.rs               # 状态转换逻辑
    │
    ├── rollback/                       # 保留/增强
    │   ├── mod.rs
    │   ├── rollback_manager.rs         # 模型回滚（保留）
    │   └── fw_rollback.rs              # [新增] 固件回滚
    │
    ├── model/                          # 模型 OTA (保留 Phase 3C.2)
    │   ├── mod.rs
    │   ├── model_manager.rs
    │   └── model_rollback.rs
    │
    └── manager.rs                      # 统一 OTA 管理器
```

### 9.2 system-monitor crate 文件结构

```
system-monitor/
├── Cargo.toml
└── src/
    ├── lib.rs                          # 模块导出 + 统一守护进程入口
    ├── config.rs                       # 监控配置
    ├── error.rs                        # 错误类型
    │
    ├── collector/                      # [采集层] 指标采集器
    │   ├── mod.rs
    │   ├── memory_collector.rs         # 内存指标采集 (30s)
    │   ├── process_collector.rs        # 进程存活检测 (15s)
    │   ├── disk_collector.rs           # 磁盘空间采集 (60s)
    │   ├── cpu_collector.rs            # CPU 使用率采集 (30s)
    │   └── network_collector.rs        # 网络统计采集 (60s)
    │
    ├── analyzer/                       # [分析层] 指标分析与告警
    │   ├── mod.rs
    │   ├── memory_leak_detector.rs     # 内存泄漏检测 (线性回归)
    │   ├── threshold_analyzer.rs       # 阈值告警分析
    │   └── trend_analyzer.rs           # 趋势分析工具
    │
    ├── storage/                        # [存储层] 时序数据
    │   ├── mod.rs
    │   ├── timeseries_db.rs            # 时序数据库封装 (SQLite 轮转)
    │   ├── metric_cleaner.rs           # 数据清理 (30天保留)
    │   └── alert_log.rs               # 告警日志记录
    │
    ├── self_heal/                      # [自愈层] 异常自愈
    │   ├── mod.rs
    │   ├── watchdog_feeder.rs          # 硬件看门狗喂狗 (30s)
    │   ├── process_restarter.rs        # 进程重启管理器
    │   ├── disk_cleaner.rs             # 磁盘自动清理
    │   ├── cgroup_manager.rs           # cgroup v2 资源控制
    │   └── oom_protector.rs            # OOM 保护 (oom_score_adj)
    │
    ├── mtbf/                           # [MTBF] 运行时间与故障统计
    │   ├── mod.rs
    │   ├── uptime_tracker.rs           # uptime 追踪与持久化
    │   ├── fault_counter.rs            # 故障计数与分类
    │   └── mtbf_calculator.rs          # MTBF 计算 (365天滚动窗口)
    │
    ├── reporter/                       # [上报层] 数据上报与 API
    │   ├── mod.rs
    │   ├── web_api_integration.rs      # 对接 web-api REST/WS
    │   └── alert_dispatcher.rs         # 告警分发 (本地日志+web-api)
    │
    └── daemon.rs                       # 守护进程主循环
```

### 9.3 Cargo.toml

#### update-engine/Cargo.toml

```toml
[package]
name = "mupc-update-engine"
version = "0.2.0"
edition = "2021"
description = "MUPC firmware OTA and model OTA update engine"

[dependencies]
tokio = { workspace = true }
async-trait = { workspace = true }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
sha2 = "0.10"
mupc-security = { path = "../security" }
reqwest = { version = "0.12", features = ["json"] }
flate2 = "1.0"
tar = "0.4"
chrono = { workspace = true }
uuid = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
parking_lot = { workspace = true }
mupc-intercore = { path = "../intercore" }

[features]
default = []
sm2 = []
```

#### system-monitor/Cargo.toml

```toml
[package]
name = "mupc-system-monitor"
version = "0.1.0"
edition = "2021"
description = "MUPC system reliability monitor daemon"

[dependencies]
tokio = { workspace = true, features = ["full"] }
async-trait = { workspace = true }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
parking_lot = { workspace = true }
rusqlite = { version = "0.31", features = ["bundled"] }
mupc-web-api = { path = "../web-api" }
mupc-intercore = { path = "../intercore" }

[dev-dependencies]
tempfile = "3.10"
tokio-test = "0.4"
```

### 9.4 配置文件

**`/etc/mupc/ota-firmware.toml`（固件 OTA 配置）：**
```toml
[server]
url = "https://ota.mupc.example.com"
check_interval_sec = 3600
download_timeout_sec = 300
download_max_speed_bps = 1_048_576  # 1MB/s
max_retries = 3
retry_interval_sec = 30

[firmware]
target_platform = "rk3588-openeuler"
min_bootloader_version = "1.0.0"
max_package_size_mb = 200

[download_window]
start_hour = 2
start_minute = 0
end_hour = 5
end_minute = 0
```

**`/etc/mupc/system-monitor.toml`（系统监控配置）：**
```toml
[collect]
memory_interval_sec = 30
process_interval_sec = 15
disk_interval_sec = 60
cpu_interval_sec = 30
network_interval_sec = 60

[watchdog]
device = "/dev/watchdog"
timeout_sec = 60
feed_interval_sec = 30

[process]
pid_dir = "/var/run/mupc"
restart_priority = ["gateway", "intercore", "strategy-engine", "data-processing", "web-api"]

[cgroup]
enabled = true
memory_max = "2G"
pids_max = 100

[oom_score_adj]
gateway = -500
intercore = -500
strategy_engine = -200
ai_engine = -200
data_processing = 0
web_api = 0

[storage]
retention_days = 30
db_path = "/var/lib/mupc/monitor/timeseries.db"
```

### 9.5 运行时文件路径汇总

```
路径                                                | 用途
---------------------------------------------------|-----------------------------------
/var/lib/mupc/ota/download_progress.json           | 下载进度持久化
/var/lib/mupc/ota/mupc_packages/                   | 下载的 .mupc 包临时存储
/var/lib/mupc/systemd/uptime_history.json          | uptime 历史
/var/lib/mupc/systemd/fault_history.json           | 故障事件历史
/var/lib/mupc/systemd/mtbf.json                    | MTBF 计算报告
/var/lib/mupc/monitor/timeseries.db                | 时序数据库 (SQLite)
/var/lib/mupc/monitor/alert_history.json           | 告警历史
/etc/mupc/security/ota_public_key.pem              | OTA 公钥 (权限 600)
/etc/mupc/ota-firmware.toml                        | 固件 OTA 配置
/etc/mupc/system-monitor.toml                      | 系统监控配置
/var/log/mupc/system-monitor.log                   | 监控守护进程日志
/dev/watchdog                                      | 硬件看门狗设备
/sys/fs/cgroup/mupc/                               | cgroup v2 控制组
```

### 9.6 预估代码量

| 模块 | 源文件数 | 预估代码行 (Rust) |
|------|---------|-----------------|
| **update-engine** (改造) | | |
| 核心类型/配置/错误 | 3 | ~500 |
| 固件升级 (分区/bootloader/包处理) | 4 | ~1,200 |
| 下载 (增强断点续传/限速) | 2 | ~400 |
| 验证 (SM2/完整性) | 2 | ~400 |
| 状态机 | 2 | ~600 |
| 回滚 | 2 | ~400 |
| 模型 OTA (保留) | 3 | ~500 |
| 管理器+集成 | 2 | ~600 |
| **小计** | **20** | **~4,600** |
| **system-monitor** (新增) | | |
| 核心/配置/错误 | 3 | ~400 |
| 采集器 (5个) | 5 | ~1,500 |
| 分析器 (泄漏检测/阈值/趋势) | 3 | ~800 |
| 时序存储 | 3 | ~600 |
| 自愈 (看门狗/重启/磁盘清理/cgroup/OOM) | 5 | ~1,500 |
| MTBF (uptime/故障计数/计算) | 3 | ~700 |
| 上报/告警 | 2 | ~400 |
| 守护进程 | 1 | ~300 |
| **小计** | **25** | **~6,200** |
| **总计** | **45** | **~10,800** |

---

## 10. 技术决策记录

### 10.1 架构决策

| ID | 决策 | 选项 | 选择 | 理由 |
|----|------|------|------|------|
| ADR-001 | 分区方案 | A/B 双分区 vs 全量镜像 | **A/B 双分区** | RK3588 原生支持，升级零中断 |
| ADR-002 | 差分算法 | bsdiff / xdelta3 / rdedup | **bsdiff** | 二进制差分率最优，Rust 生态有绑定 |
| ADR-003 | 时序存储 | SQLite vs 轮转 JSON | **SQLite** | 基础 SQL 查询支持，清理方便 |
| ADR-004 | 监控模块 | 独立 crate vs 合并到 core | **独立 crate** | 职责单一，编译隔离 |
| ADR-005 | 状态机 | 统一 vs 分离（固件/模型） | **分离** | 状态差异大（模型 9 状态 vs 固件 17 状态），分离降低复杂度 |
| ADR-006 | cgroup 版本 | v1 vs v2 | **v2** | openEuler 默认 cgroup v2 |
| ADR-007 | crate 命名 | ota-update 保留 vs 改名为 update-engine | **改造为 update-engine** | 功能从纯模型 OTA 扩展为固件+模型双模式 |
| ADR-008 | 签名算法 | SM2 vs Ed25519 | **模型 OTA: 双选；固件 OTA: SM2-with-SM3** | 模型 OTA 兼容两种算法；固件 OTA 强制国密 |
| ADR-009 | OTA 服务器接口 | RESTful vs gRPC | **RESTful** | 与现有北向网关（IEC 104 / MQTT）更易集成，调试方便 |
| ADR-010 | 进程监控方法 | pid 文件 + /proc vs systemd 通知 | **pid 文件 + /proc** | 不依赖 systemd，便于容器化部署 |

### 10.2 安全设计决策

| ID | 决策 | 说明 |
|----|------|------|
| SEC-01 | 签名验证 | 所有模型和固件必须通过签名验证才能应用 |
| SEC-02 | 公钥保护 | OTA 公钥存储在安全区域，权限 600，公钥更新自身也需签名 |
| SEC-03 | 传输加密 | 下载使用 HTTPS/TLS 1.2+ |
| SEC-04 | 篡改检测 | 文件哈希校验失败的包 100% 被拒绝 |
| SEC-05 | 日志审计 | 所有更新操作记录完整审计日志 |

### 10.3 PRD 验收标准覆盖矩阵

| PRD 验收 | 对应模块 | 实现文件 |
|----------|---------|---------|
| OTA-01 ~ 03 | model/model_manager.rs | 检查/触发 |
| OTA-04 | download/downloader.rs | 断点续传 |
| OTA-05 ~ 06 | verify/verifier.rs | 校验/签名 |
| OTA-07 ~ 08 | model/model_manager.rs | 加载/预热 |
| OTA-09 ~ 11 | rollback/rollback_manager.rs | 回滚 |
| OTA-12 ~ 14 | manager.rs | 查询/历史 |
| OTA-15 ~ 18 | download/downloader.rs + manager.rs | 进度/取消/上报 |
| FW-OTA-01 ~ 02 | firmware/mupc_package.rs | 包格式 |
| FW-OTA-03 | download/downloader.rs + progress_manager.rs | 断点续传 |
| FW-OTA-04 | verify/sm2_verifier.rs | SM2 验证 |
| FW-OTA-05 | state_machine/fw_machine.rs | 预检查 |
| FW-OTA-06 ~ 08 | firmware/partition.rs, bootloader.rs | A/B 分区 |
| FW-OTA-09 | firmware/ + download/ | 掉电恢复 |
| FW-OTA-10 ~ 12 | manager.rs | 灰度/安全模式 |
| FW-OTA-13 ~ 14 | download/downloader.rs | 进度上报 |
| REL-01 ~ 03 | collector/memory_collector.rs, analyzer/memory_leak_detector.rs | 内存监控 |
| REL-04 ~ 06 | self_heal/process_restarter.rs | 进程重启 |
| REL-07 ~ 08 | collector/disk_collector.rs, self_heal/disk_cleaner.rs | 磁盘监控 |
| REL-09 | collector/cpu_collector.rs | CPU 监控 |
| REL-10 ~ 11 | collector/network_collector.rs | 网络监控 |
| REL-12 ~ 13 | mtbf/mtbf_calculator.rs | MTBF |
| REL-14 | self_heal/watchdog_feeder.rs | 看门狗 |
| REL-15 | self_heal/cgroup_manager.rs | cgroup |
| REL-16 ~ 17 | self_heal/oom_protector.rs | OOM |
| REL-18 | mtbf/uptime_tracker.rs | uptime |

---

## 附录 A：关键术语表

| 术语 | 定义 |
|------|------|
| A/B 分区 | 两个独立系统分区，当前运行的为 A 分区，升级写入 B 分区，下次重启切换 |
| 差分包 | 仅包含新旧版本差异化数据的固件包 |
| 灰度发布 | 按批次逐步推送升级的策略 |
| MTBF | Mean Time Between Failures，平均无故障工作时间 |
| RSS | Resident Set Size，常驻内存集 |
| OOM | Out Of Memory，系统内存耗尽事件 |
| oom_score_adj | Linux 内核 OOM Killer 优先级调整参数，越小越不易被杀 |
| 看门狗 | 硬件定时器，超时触发系统复位 |
| bootloader | 负责系统启动引导和分区选择的底层软件 |
| RKNN | Rockchip Neural Network，RK3588 NPU 推理格式 |
| bsdiff | 二进制差分算法，用于生成和应用增量更新包 |
| 安全模式 | 连续多次升级/回滚失败后进入的模式，停止自动 OTA 检查，仅允许手动干预 |
| cgroup v2 | Linux 控制组 v2，用于资源隔离和限制 |
| .mupc | MUPC 固件包容器格式，包含元信息、签名和负载数据 |

---

## 附录 B：说明

历史来源文档已在 v1.0 文档体系重构中合并至本文档，不再单独维护。

---

**文档版本**：v1.0（合并稿）
**最后更新**：2026-05-29
**核心模块数**：2 个 crate（update-engine 改造 + system-monitor 新增），约 45 个源文件，预估 ~10,800 行 Rust
