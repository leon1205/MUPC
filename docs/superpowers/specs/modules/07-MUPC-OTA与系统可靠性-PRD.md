# MUPC OTA 升级与系统可靠性产品需求文档（PRD）

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 需求分析师 | 已合并 |

> 本文档为 OTA 升级与系统可靠性模块的权威需求文档。历史来源文档已在 v1.0 文档体系重构中合并，不再单独维护。

---

## 1. 产品概述

### 1.1 项目背景

MUPC 通信管理模块已完成 Phase 3C AI 优化引擎（LSTM 时序预测、MADDPG/PPO 强化学习决策、RKNN Runtime 推理）。在此基础上，本模块实现两个功能缺口：

1. **模型 OTA 更新**：使 AI 模型能够在现场运行时接收并应用更新，无需人工干预，支持断点续传、增量更新、自动回滚。
2. **固件 OTA 升级**：建立覆盖操作系统用户空间程序、系统库、容器化服务的完整远程升级能力，与模型 OTA 构成互补关系。
3. **系统可靠性保障**：为非功能性需求 MTBF >= 50,000 小时提供可量化、可测试的设计依据，涵盖内存管理、进程监控、资源防护、自愈机制。

### 1.2 适用范围

| 项目 | 说明 |
|------|------|
| 设备 | MUPC 微电网特种调控装置 |
| 平台 | Linux (openEuler)、RK3588 |
| 硬件 | ARM64、NPU (RKNN) |
| 通信 | 北向通信网关（IEC 104 / MQTT / IEC 61850） |
| OTA 服务器 | HTTPS 协议，支持 RESTful API |

### 1.3 固件 OTA 与模型 OTA 的边界定义

| 维度 | 固件 OTA | 模型 OTA |
|------|----------|----------|
| 更新对象 | 操作系统用户空间程序、系统库、容器镜像、内核模块 | AI 模型权重文件（.rknn、.onnx） |
| 更新粒度 | 完整固件包或差分包（子系统级别） | 单个模型文件 |
| 存储路径 | `/opt/mupc/`, `/usr/bin/`, `/etc/mupc/` | `/models/` |
| 备份策略 | 双分区（A/B 分区）或完整系统快照 | `rollback` 目录 |
| 升级影响 | 需重启进程或系统服务 | 运行时模型切换，无需重启 |
| 回滚方式 | 分区切换或全量还原 | 目录级文件替换 |
| 灰度发布 | 按设备 ID 分批，支持暂停/回退 | 未来扩展（Phase 4） |

### 1.4 总体目标

- 模型 OTA 全流程（检查+下载+验证+应用）可靠运行，支持自动回滚保障业务连续性
- 固件 OTA 全过程（下载+验证+升级）设备离线时间不超过 120 秒
- 固件升级失败自动回滚成功率 >= 99%
- 系统 MTBF >= 50,000 小时（连续运行不宕机）
- 资源泄露类告警 >= 30 分钟内自动触发并记录
- 异常自愈场景（OOM、进程崩溃）在 60 秒内完成恢复

---

## 2. 模型 OTA 更新

### 2.1 更新检查

#### 2.1.1 触发方式

| 方式 | 触发条件 | 优先级 |
|------|----------|--------|
| 定时检查 | 每小时自动检查（可配置） | 低 |
| 手动触发 | 北向收到更新指令 | 高 |
| 启动检查 | 设备启动时检查一次 | 中 |

#### 2.1.2 检查流程

```
1. 连接到 OTA 服务器（地址可配置）
2. 发送当前模型版本信息
3. 服务器返回最新版本信息
4. 比较版本号判断是否需要更新
5. 如果需要更新，下载版本清单
```

**版本号格式**：`major.minor.patch`（例如 `1.2.0`）

#### 2.1.3 性能要求

- 设备能在 **30 秒内**完成一次更新检查（OTA-01）

### 2.2 更新包下载

#### 2.2.1 下载流程

```
1. 解析版本清单，获取更新包 URL
2. 计算本地可用存储空间
3. 下载更新包到临时存储区
4. 支持断点续传（HTTP Range 请求）
5. 下载完成后校验文件哈希
```

#### 2.2.2 断点续传

- 使用 HTTP Range 请求（HTTP 206 Partial Content）
- 记录已下载字节数到本地持久化文件
- 断电恢复后继续下载，已下载部分不丢失
- 临时文件完整性在续传前再次校验，损坏则重新下载
- 连续 10 次断点续传测试均成功

#### 2.2.3 下载参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| timeout | 300 秒 | 单次下载超时 |
| retry_count | 3 | 下载失败重试次数 |
| chunk_size | 1 MB | 分块大小 |
| min_free_space | 500 MB | 最低可用空间 |

### 2.3 更新包验证

#### 2.3.1 验证流程

```
1. 文件完整性校验（SHA-256）
2. 模型签名验证（SM2 / Ed25519）
3. 模型格式校验（RKNN / ONNX）
4. 模型兼容性校验（平台版本匹配）
```

#### 2.3.2 签名验证

- 使用国密 SM2 或 Ed25519 签名算法
- 公钥存储在设备安全区域
- 签名包格式：`| signature(64B) | model_data |`
- 签名验证不通过的更新包 100% 被拒绝
- 伪造签名包 100% 被拒绝

### 2.4 模型应用

#### 2.4.1 应用流程

```
1. 备份当前模型到 rollback 目录
2. 解压更新包到模型目录
3. 通知策略引擎加载新模型
4. 新模型通过 RKNN Runtime 加载
5. 执行模型预热（推理一次）
6. 更新版本记录
```

#### 2.4.2 加载顺序

```
旧模型 → 备份 → 新模型加载 → 预热 → 切换 → 删除旧模型
```

#### 2.4.3 性能要求

- 新模型能在 **60 秒内**通过 RKNN Runtime 加载完成
- 模型推理预热在 **30 秒内**完成

### 2.5 模型版本管理

#### 2.5.1 存储结构

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

#### 2.5.2 version.json 格式

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

#### 2.5.3 版本查询接口

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

- 北向能通过接口查询当前所有模型的版本信息

### 2.6 增量更新支持

#### 2.6.1 增量包格式

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

### 2.7 回滚机制

#### 2.7.1 自动回滚触发条件

| 条件 | 阈值 | 说明 |
|------|------|------|
| 新模型加载失败 | - | RKNN Runtime 加载异常 |
| 模型推理失败 | 连续 3 次 | 推理结果异常 |
| 模型校验失败 | - | 签名/哈希校验不通过 |
| 模型预热超时 | 30 秒 | 预热推理超时 |

#### 2.7.2 回滚流程

```
1. 检测到回滚触发条件
2. 停止策略引擎
3. 删除新模型
4. 从 rollback 目录恢复旧模型
5. 重启策略引擎加载旧模型
6. 记录回滚事件到日志
7. 发送回滚通知到北向
```

#### 2.7.3 性能要求

- 新模型加载失败后 **10 秒内**触发自动回滚
- 回滚后设备恢复正常运行
- 回滚通知在 **1 分钟内**发送到北向

#### 2.7.4 回滚限制

- 回滚次数上限：连续 **3 次**
- 超过限制后进入**安全模式**（使用兜底策略）
- 回滚记录保存 **30 天**
- 回滚成功率 >= 99%

### 2.8 更新策略

#### 2.8.1 定时更新

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

#### 2.8.2 手动更新

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

#### 2.8.3 更新状态机

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

## 3. 固件 OTA 升级

### 3.1 固件包管理

#### 3.1.1 固件版本管理

系统管理员在 OTA 服务器上管理固件版本，每个版本包含版本号、变更说明、目标硬件平台、依赖的最低 bootloader 版本、校验证书。

**验收标准**：
- 服务端固件包上传后 **60 秒内**完成校验和计算与签名
- 每个固件版本包含必填字段：`version`（semver 格式 `major.minor.patch`）、`checksum`（SHA-256）、`signature`（SM2）、`target_platform`（固定值 `rk3588-openeuler`）、`min_bootloader_version`
- 支持查询全部历史版本列表，接口响应时间 <= 500ms
- 同一版本号不允许重复上传

#### 3.1.2 固件包格式

统一的固件包容器格式，在单个 `.mupc` 文件中包含元信息、签名、负载数据。

**容器格式定义**：
```
| magic(4B: "MUPC") | version(1B) | header_len(4B) | header_json | signature(64B) | payload |
```

**header_json 包含**：`package_type`（`full` / `incremental`）、`target_version`、`base_version`（增量包填写）、`checksum`、`file_list`（数组，每个元素为 `{path, size, mode}`）、`timestamp`

**payload** 为 tar.gz 格式固件数据（增量包为 bsdiff 格式差分数据）。

设备端需验证容器格式的所有字段合法性。

#### 3.1.3 差分包生成

对于小版本升级（patch 版本变化），OTA 服务器自动计算基准版本与目标版本的差异，生成差分包以节省带宽。

**验收标准**：
- patch 版本差异的差分包大小不超过全量包大小的 **30%**
- 差分包生成在服务器端 **120 秒内**完成（基准包大小 <= 200MB 时）
- 差分包头部包含 `base_version`、`target_version`、`checksum`、`patch_size` 字段
- 设备端差分合并后生成的完整固件 SHA-256 与全量包一致

#### 3.1.4 完整性校验

固件包从生成到应用的全链路完整性保护。每个环节（上传、存储、下载、应用前）均执行校验。

**验收标准**：
- 上传完成时 OTA 服务器自动计算 SHA-256
- 设备端下载完成后重新计算 SHA-256，必须与服务器值一致
- 设备端应用固件前，使用 SM2 公钥验证固件包的签名
- SHA-256 或 SM2 签名任一校验失败时，固件包被拒绝并记录错误日志
- 伪造签名或损坏的固件包 **100%** 被拒绝

### 3.2 固件下载与验证

#### 3.2.1 断点续传

固件下载支持 HTTP Range 断点续传。设备记录已下载进度，当网络中断或设备重启后恢复下载时，从断点处继续，无需重新下载已完成部分。

**验收标准**：
- 下载过程中记录下载进度到本地持久化文件（`/var/lib/mupc/ota/download_progress.json`）
- 重启后已下载的临时文件不丢失，下载从断点恢复
- 断点续传支持 HTTP 206 Partial Content 响应
- 连续 **10 次**断点续传测试均成功
- 临时文件完整性在续传前再次校验，损坏则重新下载

#### 3.2.2 签名验证

设备端使用内置 SM2 公钥对固件包的 SM2 数字签名进行验证，确保固件来源可信且未被篡改。

**验收标准**：
- 公钥存储在 `/etc/mupc/security/ota_public_key.pem`，文件权限 600
- 签名验证在 **5 秒内**完成（固件包 <= 200MB）
- 签名算法固定为 **SM2-with-SM3**
- 签名验证失败时，升级流程终止并记录审计日志（包含签名指纹、时间戳、固件版本）
- 公钥通过安全 OTA 方式更新（公钥更新本身也需签名验证）

### 3.3 升级前检查

设备在应用固件升级前执行检查组，任一检查项不通过则终止升级。

| 检查项 | 阈值 | 不通过处理 |
|--------|------|-----------|
| 磁盘剩余空间 | >= 500MB | 终止升级并上报 |
| 电池/电源状态 | 非电池供电或电池电量 >= 30% | 终止升级并上报 |
| CPU 负载 | <= 80% | 等待并重试，超时 5 分钟后终止 |
| 系统进程健康 | 所有关键进程（gateway、intercore、strategy-engine）运行正常 | 终止升级 |
| 实时控制模块状态 | 心跳正常 | 终止升级 |
| 固件兼容性 | 固件平台字段匹配 `rk3588-openeuler` | 终止升级 |
| 升级窗口 | 当前时间在配置的升级窗口内 | 等待至窗口时间 |

**验收标准**：
- 检查清单全部项目在 **30 秒内**执行完毕
- 任一检查不通过时，升级任务状态转换为 `Failed`，错误信息包含具体检查项和当前值
- 检查不通过的情况上报至 OTA 服务器和本地日志

### 3.4 升级执行（A/B 分区）

#### 3.4.1 升级执行流程

设备通过 A/B 双分区机制执行固件升级。升级过程中不影响当前运行系统（A 分区），固件写入 B 分区，下次重启时从 B 分区启动。

**验收标准**：
- 固件写入 B 分区期间，A 分区承载的系统不受影响，关键服务（gateway、strategy-engine）持续运行
- 升级过程超时阈值为 **120 秒**，超时触发回滚
- B 分区写入完成后，更新 bootloader 配置的 `boot_partition` 指向 B 分区
- B 分区写入完成后，执行分区级完整性校验（全量 SHA-256）
- bootloader 配置更新后进行持久化写回确认（fsync）
- 系统重启计划在升级窗口结束前至少预留 60 秒

#### 3.4.2 升级后验证

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

**验收标准**：
- 升级后验证在 **60 秒内**完成
- 验证通过后，OTA 状态上报至 OTA 服务器，标记为 `Completed`
- 验证失败时，系统在 **30 秒内**触发回滚

### 3.5 升级失败回滚与恢复

#### 3.5.1 A/B 分区回滚

当升级后验证失败或用户主动触发回滚时，系统将 bootloader 配置切换回原启动分区（A 分区），从 A 分区启动恢复至升级前状态。

**验收标准**：
- 回滚指令触发后，bootloader 配置在 **5 秒内**恢复指向原分区
- 回滚后系统重启并在 **120 秒内**恢复至升级前运行状态
- 回滚前保留 B 分区数据（用于离线分析）
- B 分区保留时间：**7 天**或直到下次升级
- 连续 **3 次**升级失败后，系统进入安全模式：停止 OTA 自动检查，仅允许手动确认后升级

#### 3.5.2 故障恢复场景

| 故障场景 | 系统行为 | 验收条件 |
|----------|---------|----------|
| 升级过程中掉电 | 上电后从原分区启动，下载进度和临时文件保留 | 恢复后 120 秒内进入正常运行状态 |
| B 分区写入过程中掉电 | 上电后 bootloader 检查分区状态，回退至 A 分区 | A 分区完整可启动，不受影响 |
| bootloader 配置写入失败 | A/B 分区状态均未变更，从 A 分区正常启动 | 上报 `bootloader_update_failed` 告警 |
| 新分区启动后关键进程崩溃 | 升级后验证失败，自动回滚至 A 分区 | 回滚后系统运行正常，上报回滚记录 |

### 3.6 分批升级策略（灰度发布）

#### 3.6.1 升级批次管理

系统管理员在 OTA 服务端将设备划分为多个升级批次，按批次逐步推送固件升级。每个批次可设置观察期，观察期内发现异常可暂停或回退。

| 维度 | 要求 |
|------|------|
| 批次数量 | 支持最多 **10** 个批次 |
| 每批次设备数 | 1 ~ 10,000 台，按设备 ID 列表或百分比分配 |
| 批次间隔 | 可配置：0 ~ 168 小时（7 天） |
| 观察期 | 可配置：0 ~ 168 小时 |
| 暂停 | 管理员可在任意时刻暂停整个灰度计划 |
| 回退 | 暂停后支持一键回退所有已升级设备至升级前版本 |
| 自动暂停条件 | 升级失败率 >= **10%** 或回滚率 >= **5%**，自动暂停灰度并通知 |

**验收标准**：
- 每批升级完成后，观察期结束后才能自动进入下一批
- 灰度计划暂停后，未升级的设备停止接收升级指令
- 灰度计划回退后，已升级设备收到明确的回退指令

#### 3.6.2 升级指令与状态上报

OTA 服务器向设备下发升级指令，设备执行过程中持续上报状态进度。

**验收标准**：
- 升级指令下发方式支持：MQTT 消息、IEC 104 遥控、定时轮询
- 设备端从收到指令到开始下载的延迟 <= **10 秒**
- 状态上报间隔：下载阶段每 **10 秒**上报一次进度，其他阶段每 **30 秒**一次
- 状态上报字段：`task_id`、`state`、`progress`（0-100）、`error_message`、`estimated_remaining_seconds`、`current_partition`（A/B）

### 3.7 与现有系统的集成点

#### 3.7.1 安全模块（签名验证）

| 集成点 | 说明 |
|--------|------|
| SM2 签名验证 | 固件 OTA 复用安全模块的 SM2 签名验证能力 |
| 公钥管理 | 固件 OTA 使用独立的 SM2 密钥对，公钥路径 `/etc/mupc/security/ota_public_key.pem` |
| 审计日志 | 所有固件升级操作写入安全审计日志 |

#### 3.7.2 intercore（核间通信协同升级）

| 集成点 | 说明 |
|--------|------|
| 升级前通知 | 固件升级开始前发送 `strategy_mode = basic` 信号 |
| `ai_ready` 信号 | 升级期间发送 `ai_ready = false`，升级完成恢复后发送 `ai_ready = true` |
| 实时控制模块状态检查 | 升级前检查通过 intercore 查询心跳 |
| 核间通信恢复 | 升级重启后自动重新建立 TCP 连接（超时 10 秒，重试间隔 3 秒，最多 5 次） |

#### 3.7.3 web-api（状态上报）

| 集成点 | 说明 |
|--------|------|
| 升级状态 REST API | `GET /api/v1/ota/firmware/status` 和 `GET /api/v1/ota/firmware/history` |
| 资源监控 REST API | `GET /api/v1/system/resources` 返回内存、CPU、磁盘、网络指标 |
| MTBF 报告 REST API | `GET /api/v1/system/mtbf` 返回 MTBF 计算值和运行时间统计 |
| WebSocket 推送 | 升级进度、告警事件通过 WebSocket 实时推送到前端 |

#### 3.7.4 OTA 服务器接口

| 接口 | 说明 |
|------|------|
| 版本查询 | `GET /api/v1/firmware/versions?device_id={id}&current_version={ver}` |
| 下载固件包 | `GET /api/v1/firmware/download/{version}` 支持 HTTP Range 头 |
| 状态上报 | `POST /api/v1/firmware/status` |
| 灰度指令 | `POST /api/v1/devices/{id}/firmware/update` |
| 批次查询 | `GET /api/v1/firmware/batches?plan_id={plan_id}` |

---

## 4. 系统监控

### 4.1 内存监控与管理

#### 4.1.1 内存使用监控

系统守护进程每 30 秒采集一次系统级和进程级内存使用数据。

| 指标 | 采集频率 | 单位 | 告警阈值 |
|------|---------|------|---------|
| 系统总内存使用率 | 30 秒 | % | 连续 5 次 >= 85%（WARNING）；连续 5 次 >= 92%（CRITICAL） |
| 各进程 RSS | 30 秒 | MB | 超配额的 1.5 倍且持续 3 分钟 |
| Swap 使用量 | 60 秒 | MB | > 0（WARNING）；>= 512MB（CRITICAL） |
| OOM Killer 事件 | 事件驱动 | - | 每次 OOM 触发 CRITICAL 告警 |
| 堆内存分配增量（每进程） | 60 秒 | MB/小时 | 持续 6 小时增长趋势 > 5%（疑似泄漏） |

**验收标准**：
- 全部指标采集周期误差不超过 5 秒
- 历史数据保留 **30 天**，每日滚动清理
- 每条告警记录包含：时间戳、指标名、当前值、阈值、持续时长、影响进程列表

#### 4.1.2 内存泄漏检测

基于进程 RSS 的长时间序列分析，识别持续增长的进程。当某进程 RSS 在 6 小时内持续增长超过 5% 且无下降趋势时，标记为疑似内存泄漏并自动重启该进程。

**验收标准**：
- 检测算法使用线性回归斜率判断增长趋势，置信度要求 R^2 >= 0.7
- 疑似泄漏判定后 **30 秒内**记录告警
- 疑似泄漏判定后，采集并保存一次该进程的全量 `/proc/[pid]/smaps` 快照
- 自动重启前等待 **120 秒**（给管理员手动干预窗口）
- 同一进程 **24 小时内**最多自动重启 **3 次**，超过后转为 CRITICAL 告警且不再自动重启

### 4.2 进程健康监控

#### 4.2.1 进程存活检测

系统守护进程每 **15 秒**检查一组关键进程的运行状态。关键进程列表：gateway、intercore、strategy-engine、data-processing、web-api、ai-engine（如启用）、rs485-plugin（如启用）、mqtt-plugin（如启用）。

**验收标准**：
- 每个关键进程存活检查时间 <= 200ms
- 进程检测方法：pid 文件 + `/proc/[pid]/status` 状态检查，二者均通过才判定存活
- 进程缺失 **15 秒内**产生 WARNING 事件
- 进程缺失 **30 秒后**触发自动重启
- 自动重启后 **15 秒内**再次检查，如不存活则再次重启
- 单进程连续 **5 次**重启失败后，不再自动重启，升级为 CRITICAL 告警

#### 4.2.2 进程自动重启

关键进程崩溃或失联时，由守护进程按预设顺序执行重启。

**验收标准**：
- 从检测到进程缺失到自动重启成功，总耗时 <= **60 秒**
- 守护进程本身连续 **3 次**重启失败后，触发硬件看门狗复位
- 重启记录包含：进程名、崩溃时间、重启成功/失败时间、重启次数计数器
- 与实时控制模块通信的 intercore 崩溃重启时，需发送 `ai_ready = false` 信号

### 4.3 磁盘空间监控与告警

#### 4.3.1 磁盘监控

系统守护进程每 **60 秒**采集磁盘分区使用数据。

| 分区 | 告警阈值 | 处置动作 |
|------|---------|---------|
| `/` | 使用率 >= 85%（WARNING）；>= 92%（CRITICAL） | WARNING：记录告警；CRITICAL：自动清理临时文件 |
| `/var/log` | 使用率 >= 80%（WARNING）；>= 90%（CRITICAL） | WARNING：记录告警；CRITICAL：触发日志轮转压缩 |
| `/opt/mupc` | 使用率 >= 85%（WARNING）；>= 92%（CRITICAL） | CRITICAL：停止非关键数据写入 |
| `/models` | 使用率 >= 85%（WARNING） | 禁止新的模型/固件下载 |
| 系统 inode | 使用率 >= 85%（WARNING） | 检查小文件堆积并触发清理 |

**验收标准**：
- 磁盘使用率采集精度：整数百分比
- 告警去重：同一指标在 1 小时内只发送一次同一级别的告警
- 磁盘监控指标上报至 web-api，供前端仪表盘使用

#### 4.3.2 自动磁盘清理

当 `/var/log` 或临时目录空间不足时，系统自动执行分级清理策略。

**验收标准**：
- 第一级（WARNING 触发）：轮转并压缩 **7 天前**的日志文件，保留期限 **90 天**
- 第二级（CRITICAL 触发）：删除 **30 天前**的日志文件
- 第三级（CRITICAL 持续 1 小时）：删除 /tmp 下的过期临时文件（超过 24 小时未访问）
- 清理记录计入审计日志

### 4.4 CPU 使用率监控

系统守护进程每 **30 秒**采集系统级和进程级 CPU 使用率。提供 5 分钟和 15 分钟平均负载。

| 指标 | 采集频率 | 告警阈值 | 处置 |
|------|---------|---------|------|
| 系统总 CPU 使用率 | 30 秒 | 连续 5 次 >= 90% | 记录 CRITICAL 告警 |
| 单进程 CPU 使用率 | 30 秒 | 超过配额 3 倍持续 5 分钟 | 记录 WARNING，采样进程堆栈 |
| 1 分钟平均负载 | 30 秒 | >= CPU 核心数 * 2 | 记录 WARNING |
| 15 分钟平均负载 | 30 秒 | >= CPU 核心数 * 1.5 持续 30 分钟 | 记录 CRITICAL |

### 4.5 网络资源监控

系统守护进程每 **60 秒**采集网络接口统计信息，包括收发速率、错包率、重传率。监控北向（IEC 104/MQTT/61850）与核间通信（RJ45）两条链路的 TCP 连接状态。

| 指标 | 采集频率 | 告警阈值 |
|------|---------|---------|
| 北向接口收发带宽 | 60 秒 | 使用率 >= 80% 持续 5 分钟 |
| TCP 重传率 | 60 秒 | >= 5% 持续 3 分钟 |
| 核间通信延迟 | 15 秒（ping 心跳） | >= 100ms（WARNING）；>= 500ms（CRITICAL） |
| TCP 连接计数 | 60 秒 | 活跃连接 >= 100 个 |

---

## 5. MTBF >= 50,000 小时

### 5.1 系统运行时间统计与 MTBF 计算

#### 5.1.1 uptime 追踪

系统守护进程记录每次启动时间、上次关机时间、原因（正常关机 / 异常重启 / 看门狗复位）。根据运行时间和异常中断次数计算 MTBF。

**验收标准**：
- 每次启动时记录 `/var/lib/mupc/systemd/uptime_history.json`，包含 `boot_time`、`shutdown_time`、`shutdown_type`（`normal`/`crash`/`watchdog`）
- MTBF 计算公式：`MTBF = sum(running_durations) / crash_count`（滚动窗口：最近 **365 天**）
- 正常运行累计时间精度：秒级
- 异常重启 **100%** 被检测并分类（看门狗触发 / OOM / 进程 panic / 内核 panic / 掉电）
- MTBF 值每 **24 小时**计算一次并写入 `/var/lib/mupc/systemd/mtbf.json`

#### 5.1.2 MTBF 达标告警

**验收标准**：
- 当滚动窗口（365 天）计算的 MTBF < **50,000 小时**时，每月生成一次报告
- 当 MTBF < **10,000 小时**时，触发 WARNING 告警
- MTBF 报告通过 web-api 可查询，包含：报告期起止时间、总运行时间、异常中断次数、各中断类型占比、MTBF 计算值

### 5.2 MTBF 目标汇总

| 指标 | 目标值 | 测量方法 |
|------|-------|---------|
| MTBF | >= **50,000 小时** | 最近 365 天滚动计算，`sum(running_duration)/crash_count` |
| 单次异常恢复时间 | <= **120 秒**（自动恢复） | 从异常发生到关键进程全部恢复 |
| 计划内宕机（升级） | 每年不超过 **4 次** | 年度累计升级次数 |
| 非计划宕机 | 每年不超过 **2 次** | 异常崩溃/重启次数 |

---

## 6. 异常自愈机制

### 6.1 硬件看门狗

系统启用硬件看门狗（`/dev/watchdog`）。守护进程每 30 秒喂狗一次。守护进程异常退出或系统挂起时，看门狗超时触发硬件复位。

**验收标准**：
- 看门狗超时时间：**60 秒**
- 守护进程正常时每 **30 秒**写入 `/dev/watchdog`
- 守护进程连续 **3 次**喂狗失败后，由看门狗自动复位
- 复位后系统启动时记录 `shutdown_type = watchdog` 到 uptime 历史

### 6.2 资源耗尽保护

系统设置进程级资源限制（ulimit、cgroup）。当内存、磁盘、文件描述符超过限制时系统自动执行保护动作，防止单进程耗尽全系统资源。

| 资源类型 | 限制 | 超限处置 |
|---------|------|---------|
| 进程 RSS（按进程角色） | gateway: 256MB, intercore: 128MB, strategy-engine: 512MB, data-processing: 256MB, ai-engine: 1024MB, web-api: 128MB | 打印堆栈后重启该进程 |
| 文件描述符数（单进程） | 4096 | 记录 WARNING 告警 |
| 打开文件数（全系统） | 65536 | 记录 CRITICAL 告警，关闭非关键日志句柄 |
| /tmp 占用 | 1GB | 清理 24 小时前临时文件 |
| /var/log 日志文件单文件大小 | 100MB | 自动轮转 |

**验收标准**：
- 所有资源限制通过 cgroup v2 或 ulimit 在守护进程启动时设置
- 超限处置在检测到时间点 **10 秒内**执行

### 6.3 OOM 保护

配置 `oom_score_adj` 确保守护进程和关键网络进程（gateway、intercore）在 OOM 时不被优先杀死。非关键进程（如 web-api 的静态文件缓存）的 `oom_score_adj` 设为较高值。

**验收标准**：
- gateway 和 intercore 的 `oom_score_adj` = **-500**（低被杀概率）
- strategy-engine 和 ai-engine 的 `oom_score_adj` = **-200**
- data-processing 和 web-api 的 `oom_score_adj` = **0**
- 所有非关键辅助进程（日志轮转、指标采集）的 `oom_score_adj` = **500**（优先被杀）
- OOM 事件发生后 **30 秒内**产生告警，记录被杀死进程名、RSS 使用量、系统可用内存

### 6.4 系统可靠性边界条件

| 场景 | 系统行为 | 验收标准 |
|------|---------|----------|
| 内存泄漏累积 72 小时未重启 | 进程 RSS 超限触发自动重启。如果泄漏进程是 ai-engine，先尝试热切换至兜底策略再重启 | 重启后 RSS 回落到基线值 |
| 同时 3 个进程崩溃 | 守护进程按优先级排序重启：gateway（最高）> intercore > strategy-engine > data-processing > web-api | 全部进程在 120 秒内恢复 |
| 磁盘写入失败（设备故障） | 所有写入操作返回 `IoError`，系统降级运行：停止日志写入（降级为 stderr 输出），继续执行控制指令 | 核心控制功能不受影响 |
| 核间通信中断 | 守护进程连续 3 次心跳未回复后，发送复位信号至实时控制模块 | 核间通信在 30 秒内恢复 |
| 多次自动重启仍失败 | 单进程连续 5 次重启失败后，不再自动重启，转为 CRITICAL 告警 | 守护进程保持存活，等待管理员介入 |
| 守护进程自身崩溃 | 硬件看门狗在 60 秒后复位系统 | 系统重启后守护进程自动拉起 |
| /var 分区只读（文件系统错误） | 守护进程无法写入日志，降级为输出至 syslog | 核心控制功能不中断 |

---

## 7. 非功能性需求

### 7.1 更新性能

| 维度 | 模型 OTA | 固件 OTA |
|------|----------|----------|
| 全量更新时长 | 单模型 < **10 分钟**（100MB 模型） | 中断 <= **120 秒** |
| 增量更新时长 | 单模型 < **2 分钟**（10MB 增量） | - |
| 更新检查时长 | < **30 秒** | - |
| 下载延迟（手动触发） | < **5 秒** | < **10 秒** |

### 7.2 系统影响

| 维度 | 要求 |
|------|------|
| OTA 模块内存占用 | < **50MB**（模型 OTA） |
| 监控守护进程内存 | <= **64MB RSS** |
| 监控守护进程 CPU | <= **3%** 平均 |
| 下载期间 CPU 峰值 | < **30%**（模型 OTA）；< **10%**（固件 OTA） |
| 下载期间带宽占用 | 最高 **1MB/s**（可配置） |
| 升级过程业务中断时间 | <= **120 秒**（从重启到新系统关键进程就绪） |
| 升级期间数据丢失 | **零丢失** |

### 7.3 监控采集频率汇总

| 监控维度 | 采集频率 | 告警响应时间 |
|---------|---------|-------------|
| 系统内存 | 30 秒 | 告警条件满足后 60 秒内推送 |
| 进程存活 | 15 秒 | 进程缺失 30 秒内自动重启 |
| 磁盘空间 | 60 秒 | 超阈值 60 秒内记录告警 |
| CPU 使用率 | 30 秒 | 超条件 60 秒内记录告警 |
| 网络统计 | 60 秒 | 超条件 60 秒内记录告警 |
| 核间心跳 | 15 秒 | 丢失 3 次心跳后触发重连 |
| 看门狗喂狗 | 30 秒 | 超时 60 秒触发硬件复位 |

### 7.4 数据保留

| 数据类型 | 保留期限 | 清理策略 |
|---------|---------|---------|
| 内存/CPU/磁盘监控历史 | **30 天** | 每日凌晨清理 30 天前数据 |
| 进程重启记录 | **90 天** | 按时间戳轮转 |
| 模型 OTA 更新历史 | **30 天** | 按记录数轮转 |
| 固件升级历史 | **3 年** | 最多保留 1000 条 |
| 告警记录 | **1 年** | 按日期分文件 |
| 运行日志（tracing） | **90 天** | 每日轮转，压缩后保留 |
| B 分区保留（回滚前） | **7 天**或到下次升级 | 空间回收 |

### 7.5 安全性要求

| ID | 安全项 | 要求 |
|----|--------|------|
| SEC-01 | 签名验证 | 所有模型和固件必须通过签名验证才能应用 |
| SEC-02 | 公钥保护 | OTA 公钥存储在安全区域，固件公钥路径 `/etc/mupc/security/ota_public_key.pem`，权限 600 |
| SEC-03 | 传输加密 | 下载使用 HTTPS/TLS 1.2+ |
| SEC-04 | 篡改检测 | 文件哈希校验失败的包 100% 被拒绝 |
| SEC-05 | 日志审计 | 所有更新操作记录完整审计日志 |

### 7.6 技术约束

| 项目 | 要求 |
|------|------|
| 编程语言 | Rust >= 1.75 |
| 异步运行时 | Tokio |
| HTTP 客户端 | reqwest（支持 HTTPS 和 Range 请求） |
| 差分算法 | bsdiff（模型增量更新） |
| 哈希算法 | SHA-256 |
| 签名算法 | 模型 OTA：Ed25519 / SM2；固件 OTA：SM2-with-SM3 |
| 固件包格式 | `.mupc` 容器（magic + 头部 JSON + SM2 签名 + tar.gz payload） |
| 分区方案 | A/B 双分区（system-a、system-b），每个分区 >= 1GB |
| bootloader | 支持可配置 `boot_partition` 参数 |
| 进程监控 | 通过 cgroup v2 限制和监控 |
| 时序数据 | 本地 SQLite 轮转或 JSON 文件轮转 |
| 系统日志 | tracing（与现有 common crate 一致） |
| 配置格式 | TOML（模型 OTA） |

---

## 8. 验收标准汇总

### 8.1 模型 OTA 验收

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
| OTA-15 | 下载进度 | 北向能实时获取下载进度（0-100%） | 集成测试 |
| OTA-16 | 空间检查 | 可用空间小于 500MB 时禁止下载 | 单元测试 |
| OTA-17 | 更新取消 | 下载中的更新任务能被取消 | 集成测试 |
| OTA-18 | 状态上报 | 更新状态变化时实时上报北向 | 集成测试 |

### 8.2 固件 OTA 验收

| ID | 功能 | 验收条件 | 验证方法 |
|----|------|----------|----------|
| FW-OTA-01 | 固件包上传 | 服务端 60 秒内完成校验和与签名生成 | 集成测试 |
| FW-OTA-02 | 差分包生成 | patch 版本差分包 <= 全量包 30%，120 秒内生成 | 单元测试 |
| FW-OTA-03 | 断点续传 | 10 次断电恢复后固件下载成功 | 集成测试 |
| FW-OTA-04 | 签名验证 | SM2 签名不通过 100% 拒绝 | 单元测试 |
| FW-OTA-05 | 升级前检查 | 30 秒内完成全部检查项，任一不合格则终止 | 集成测试 |
| FW-OTA-06 | 升级执行 | 写入 B 分区时 A 分区运行不受影响 | 集成测试 |
| FW-OTA-07 | 升级后验证 | 60 秒内完成全部验证项 | 集成测试 |
| FW-OTA-08 | A/B 分区回滚 | 回滚后 120 秒内恢复至升级前状态 | 集成测试 |
| FW-OTA-09 | 掉电恢复 | 升级中掉电，恢复后从原分区启动，120 秒正常运行 | 集成测试 |
| FW-OTA-10 | 灰度发布 | 支持最多 10 批，观察期结束后自动进入下一批 | 集成测试 |
| FW-OTA-11 | 自动暂停 | 升级失败率 >= 10% 自动暂停灰度 | 集成测试 |
| FW-OTA-12 | 安全模式 | 连续 3 次升级失败后停止自动 OTA 检查 | 集成测试 |
| FW-OTA-13 | 升级状态上报 | 下载阶段每 10 秒上报，其他阶段每 30 秒上报 | 集成测试 |
| FW-OTA-14 | 指令延迟 | 从收到指令到开始下载 <= 10 秒 | 集成测试 |

### 8.3 系统可靠性验收

| ID | 功能 | 验收条件 | 验证方法 |
|----|------|----------|----------|
| REL-01 | 内存使用监控 | 30 秒采集一次，精度 MB，保留 30 天 | 单元测试 |
| REL-02 | 内存泄漏检测 | 6 小时 RSS 增长 > 5% 触发告警 | 集成测试 |
| REL-03 | 泄漏进程自动重启 | 检测到泄漏 120 秒后自动重启进程 | 集成测试 |
| REL-04 | 进程存活检测 | 15 秒检测一次，缺失 30 秒后自动重启 | 集成测试 |
| REL-05 | 进程重启总耗时 | 从检测到恢复 <= 60 秒 | 集成测试 |
| REL-06 | 连续重启保护 | 单进程连续 5 次重启失败后停止自动重启 | 集成测试 |
| REL-07 | 磁盘空间监控 | 60 秒采集一次，超阈值 60 秒内告警 | 单元测试 |
| REL-08 | 磁盘自动清理 | CRITICAL 告警时自动压缩/删除历史日志 | 集成测试 |
| REL-09 | CPU 使用率监控 | 30 秒采集，超条件 60 秒内告警 | 单元测试 |
| REL-10 | 网络带宽监控 | 60 秒采集带宽和重传率 | 单元测试 |
| REL-11 | 核间通信延迟监控 | 15 秒心跳，>= 100ms 告警 | 集成测试 |
| REL-12 | MTBF 计算 | 24 小时计算一次，滚动窗口 365 天 | 集成测试 |
| REL-13 | MTBF 告警 | MTBF < 10,000 小时触发 WARNING | 集成测试 |
| REL-14 | 硬件看门狗 | 守护进程异常退出后 60 秒硬件复位 | 集成测试 |
| REL-15 | 进程级 RSS 限制 | RSS 超限 10 秒内执行处置 | 集成测试 |
| REL-16 | OOM 保护 | `oom_score_adj` 按角色正确配置 | 集成测试 |
| REL-17 | OOM 事件告警 | 事件发生后 30 秒内记录告警 | 集成测试 |
| REL-18 | uptime 追踪 | 每次启动记录完整，异常重启分类准确 | 单元测试 |

### 8.4 非功能验收

| 类型 | 指标 | 验收条件 |
|------|------|----------|
| 更新时长（模型 OTA） | 全量更新 | 单模型 < 10 分钟（100MB 模型） |
| 更新时长（模型 OTA） | 增量更新 | 单模型 < 2 分钟（10MB 增量） |
| 升级业务中断（固件 OTA） | 固件升级 | 中断 <= 120 秒 |
| 系统影响（模型 OTA） | 内存占用 | OTA 模块 < 50MB |
| 系统影响（固件 OTA） | 监控守护进程内存 | <= 64MB RSS |
| 系统影响（固件 OTA） | 监控守护进程 CPU | <= 3% 平均 |
| 系统影响（模型 OTA） | CPU 峰值 | 下载期间 CPU 峰值 < 30% |
| 系统影响（固件 OTA） | 下载期间 CPU | <= 10% |
| 系统影响 | 下载期间带宽 | 最高 1MB/s（可配置） |
| 可靠性 | 断点续传 | 10 次断电恢复后更新成功 |
| 可靠性 | 回滚成功率 | 回滚成功率 >= 99% |
| MTBF | 系统 MTBF | >= 50,000 小时 |
| 单次非计划宕机 | 恢复时间 | <= 120 秒 |
| 非计划宕机 | 年化次数 | <= 2 次 |
| 安全性 | 签名验证 | 伪造签名包 100% 被拒绝 |
| 安全性 | 数据完整性 | 损坏包 100% 被检测并拒绝 |

---

## 附录 A：模型 OTA 接口定义

### A.1 OTA 管理器接口

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

### A.2 北向通信接口

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

### A.3 OTA 配置接口

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

## 附录 B：架构设计

### B.1 模块依赖

```
ota-update
├── common (错误类型、日志)
├── gateway (北向通信)
├── strategy-engine (模型加载)
└── intercore (状态上报)

系统监控守护进程
├── common (错误类型、日志)
├── web-api (状态上报)
├── intercore (核间通信)
└── security (签名验证)

依赖关系：
- ota-update 通过 gateway 接收远程指令和上报状态
- ota-update 通知 strategy-engine 切换模型
- ota-update 通过 intercore 上报更新状态到实时控制模块
- 监控守护进程通过 web-api 暴露 REST 接口
- 监控守护进程通过 intercore 监控核间通信状态
```

### B.2 目录结构

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

### B.3 关键设计决策实现

#### B.3.1 断点续传（模型 OTA）

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

#### B.3.2 签名验证

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

#### B.3.3 增量更新（模型 OTA）

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

## 附录 C：术语表

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

---

## 附录 D：用户故事

### D.1 远程推送模型更新

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
- 运维人员可在云端平台创建更新任务
- MUPC 能在 5 分钟内响应远程更新指令
- 更新进度实时同步到云端平台
- 更新完成后云端收到成功通知

### D.2 现场设备自动更新

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
- 设备每小时检查一次更新（可配置）
- 设备在下载窗口内完成下载
- 断电后恢复下载，已下载部分不丢失
- 更新后模型推理结果正确
- 更新过程不影响其他模块运行

### D.3 更新失败自动回滚（模型 OTA）

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
- 新模型加载失败时 10 秒内触发回滚
- 回滚后设备恢复正常运行
- 回滚通知在 1 分钟内发送到北向
- 回滚次数超过 3 次后进入安全模式
- 回滚事件记录完整可查询

### D.4 固件灰度升级

**角色**：系统管理员

**场景**：管理员需要对 100 台 MUPC 升级 v1.2.0 固件。创建灰度计划按批次推送。

**流程**：
1. 管理员创建灰度计划：第一批 10 台（24 小时观察期）
2. 第一批升级完成，观察期内无异常
3. 自动进入第二批 30 台
4. 第二批中有 2 台报告升级后通信异常
5. 管理员暂停灰度发布并回退

**验收标准**：
- 支持最多 10 个批次
- 观察期结束后自动进入下一批
- 升级失败率 >= 10% 自动暂停

## 附录 E：未来扩展（Phase 4）

| Phase | 内容 |
|-------|------|
| 4A | 分批推送（灰度发布）- 模型 OTA |
| 4B | 更新回滚可视化（云端管理平台） |
| 4C | A/B 测试框架 |

---

**文档版本**：v1.0
**最后更新**：2026-05-29
**合并状态**：已完成，保留两份源文档的所有 [REVIEWED: PASS] 功能需求
