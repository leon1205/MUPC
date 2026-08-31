# 技术债清单

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-27 | 项目经理 | 记录中 |
| v2.0 | 2026-06-15 | 项目经理 | 审计更新 — 新增文档-代码一致性审计发现 |
| v2.1 | 2026-06-18 | 项目经理 | v2.17 新增：SafetyRLWrapper broadcast channel 组装（U-15）|
| v2.2 | 2026-06-21 | 项目经理 | v3.0 新增：训练-部署输入维度不一致（U-16，P0 已修复）+ 根因分析 |
| v3.0 | 2026-07-06 | 项目经理 | v3.1 审计后更新：U-15 已修复、4 轮代码审查归档、在线微调/交叉编译/部署体系完成、新增 P0 编译错误 + P1 存根项 |
| v3.1 | 2026-07-06 | 项目经理 | P0 紧急修复 + P2 清理：U-17/U-18 编译错误、4.3 Windows 日志路径、U-21~U-24（reward_config/dynamic_loader/TLS/OTA 请求） |
| v3.2 | 2026-08-14 | 项目经理 | 第二轮深度审视：新增流程改进教训（闭环验证 / 状态虚报 / 死代码门控）|
| v3.3 | 2026-08-31 | 项目经理 | 台区储能治理策略投运前置项：U-26 分相数据源未接线 / U-27 Q 核相未验证 / U-28 AI 恢复分相清零未定 |
| v3.4 | 2026-08-31 | 项目经理 | U-17 回归再修复：safety_wrapper 测试 predict() 补 p_cur 实参（阻塞全 workspace 测试的编译错误解除） |
| v3.5 | 2026-08-31 | 项目经理 | 策略引擎精简为单一台区储能治理策略；三策略废弃，pv_limit/load_shedding 移除 |

---

## 1. 概述

本文档记录 MUPC Phase 1 开发过程中发现的技术债务，包括代码问题、设计缺陷和待优化项。

**记录来源：**
- 代码评审报告 (`mupc/CODE_REVIEW.md`)
- 开发过程中的发现
- 评审过程中的遗留问题

---

## 2. 严重问题（需立即修复）

### 2.1 IEC 104 TypeID 支持不完整

| 属性 | 内容 |
|------|------|
| **问题描述** | TypeId 枚举仅实现 7 个类型，缺少基础遥测类型和基础命令类型 |
| **发现位置** | `crates/gateway/src/iec104/protocol.rs` |
| **影响范围** | 与调度主站的兼容性，可能导致部分数据无法解析 |
| **优先级** | 高 |
| **修复建议** | 添加缺失的 TypeID：M_SP_NA_1(1), M_DP_NA_1(3), M_ME_NA_1(9), M_ME_NC_1(13), C_SC_NA_1(45), C_DC_NA_1(46), C_SE_NA_1(48) |
| **状态** | ✅ 已修复 |

---

### 2.2 未定义的构建时环境变量

| 属性 | 内容 |
|------|------|
| **问题描述** | 代码引用 `env!("BUILT_TIME_RAW")` 环境变量，但该变量未在构建时定义 |
| **发现位置** | `crates/web-api/src/routes/status.rs:53` |
| **影响范围** | 会导致编译失败 |
| **优先级** | 高 |
| **修复建议** | 使用 `chrono` 库在运行时获取构建时间，或在 `Cargo.toml` 中通过 build script 定义该变量 |
| **状态** | ✅ 已修复 |

---

## 3. 警告问题（建议修复）

### 3.1 帧格式未达到定长 64 字节

| 属性 | 内容 |
|------|------|
| **问题描述** | intercore 帧格式未添加填充字节，不满足定长 64 字节要求 |
| **发现位置** | `crates/intercore/src/protocol.rs` |
| **影响范围** | 协议解析可能出现问题 |
| **优先级** | 中 |
| **修复建议** | 在 `IntercoreFrame::to_bytes()` 中添加填充逻辑，确保总帧长度为 64 字节 |
| **状态** | ✅ 已修复 |

---

### 3.2 Session 过期检查时序问题

| 属性 | 内容 |
|------|------|
| **问题描述** | `is_expired()` 方法使用 `Utc::now()` 比较时间，在高并发场景下可能出现时序问题 |
| **发现位置** | `crates/web-api/src/auth.rs:48-50` |
| **影响范围** | Session 过期判断可能不准确 |
| **优先级** | 低 |
| **修复建议** | 考虑使用 `Duration` 比较或添加一个时间窗口容差 |
| **状态** | ✅ 已修复 |

---

### 3.3 硬编码 IP 地址

| 属性 | 内容 |
|------|------|
| **问题描述** | 配置中存在硬编码的默认 IP 地址 `192.168.1.100` |
| **发现位置** | `crates/web-api/src/routes/config.rs:76` |
| **影响范围** | 部署灵活性降低 |
| **优先级** | 低 |
| **修复建议** | 将默认值改为 `0.0.0.0` 或从环境变量读取 |
| **状态** | ✅ 已修复 |

---

### 3.4 连接处理任务未正确清理

| 属性 | 内容 |
|------|------|
| **问题描述** | 连接处理完成后未从 `connections` 列表中移除已关闭的连接 |
| **发现位置** | `crates/gateway/src/iec104/server.rs:169-170` |
| **影响范围** | 长期运行后可能导致连接列表膨胀 |
| **优先级** | 中 |
| **修复建议** | 在连接关闭时显式调用清理逻辑，从 `connections` 列表中移除已关闭的连接 |
| **状态** | ✅ 已修复 |

---

## 4. 优化建议（可选修复）

### 4.1 测试覆盖率不足

| 属性 | 内容 |
|------|------|
| **问题描述** | 集成测试仅为空的 TODO 实现 |
| **发现位置** | `tests/integration/*.rs` |
| **影响范围** | 无法验证核心功能 |
| **优先级** | 中 |
| **修复建议** | 实现实际的集成测试用例，覆盖正常流程和异常流程 |
| **状态** | ✅ 已修复 (v3.1) |
| **修复日期** | 2026-06-21 |
| **修复内容** | 新增 `core_pipeline_integration.rs`（13 个集成测试）：78 维序列化、NaN/Inf 校验、动作反归一化、降级层级顺序、下垂公式符号、SceneWeights 维度、缺失值 HLV 语义、冲击保守系数、WCET 预算 |

---

### 4.2 代码重复 - MupcError 创建模式

| 属性 | 内容 |
|------|------|
| **问题描述** | 各模块重复使用 `MupcError::new()` 创建错误，缺少统一的错误创建宏 |
| **发现位置** | 多个模块 |
| **影响范围** | 代码可维护性 |
| **优先级** | 低 |
| **修复建议** | 扩展 `define_error!` 宏的使用，为每个模块生成专用错误创建函数 |
| **状态** | ✅ 已修复 (v3.1) |
| **修复内容** | `define_error_with_source!` 宏新增；`impl_module_error_ext!` 宏新增；`data-processing` 通过 `mupc_errors` 子模块统一 11 处调用 |

---

### 4.3 日志配置路径不兼容 Windows

| 属性 | 内容 |
|------|------|
| **问题描述** | 默认日志路径 `/var/log/mupc` 使用 Unix 风格路径 |
| **发现位置** | `crates/common/src/logging.rs:34` |
| **影响范围** | 开发环境（Windows）不兼容 |
| **优先级** | 低 |
| **修复建议** | 根据 `std::env::consts::OS` 选择不同的默认路径 |
| **状态** | ✅ 已修复 (v3.1) — `LogConfig::default()` 已通过 `cfg!(windows)` 编译时检测自动选择路径 |

---

## 5. 已识别待实现功能（Phase 2+）

| 功能 | 说明 | 优先级 | 状态 |
|------|------|--------|------|
| 动态插件加载 | libloading 加载 .so 插件 | 高 | ✅ 已完成 |
| 南向通信 | RS485/HPLC 驱动 | 高 | ✅ 基本完成（4 个测试待修） |
| 消息总线 | AMQP/MQTT 进程间通信 | 中 | ✅ MQTT 已完成 |
| 安全增强 | MQTT over TLS、国密 SM2/SM4 | 中 | ⚠️ 部分实现（SM3/SM4 CBC 完成；SM2签名/SM4 GCM 待 gmsm 0.14） |
| 协议扩展 | IEC 61850-7-420 | 中 | ⚠️ 部分实现（原始TCP+自研ASN.1，待 libIEC61850 FFI） |
| AI 优化引擎 | LSTM/TCN 预测、MADDPG/PPO 决策 | 低 | ✅ 已完成 |
| 在线微调 | PER/KL/影子模型/热切换 | 中 | ✅ 已完成（2026-07） |
| OTA 升级 | 差分升级、安全启动 | 低 | ⚠️ 模型OTA完成；固件OTA骨架就位（A/B分区/签名待实现） |
| 故障录波 | 完整实现 | 中 | ⚠️ 4个方法为stub（get_waveform等） |
| mupc-core-bin | 主控进程入口（14 子系统编排） | 高 | ✅ 基本完成（2 个 Phase 2+ TODO 遗留） |
| aarch64 交叉编译 | CMake + build.rs RKNN 检测 + Docker | 高 | ✅ 已完成（2026-07） |
| 部署体系 | deploy.sh + setup-deps.sh + deploy.md | 中 | ✅ 已完成（2026-07） |
| 数据库迁移幂等化 | ALTER TABLE 前检查列是否存在 | 中 | ✅ 已完成（2026-07） |
| npu feature 自动检测 | Linux 默认启用、Windows 自动 stub | 中 | ✅ 已完成（2026-07） |

---

## 6. 文档-代码一致性审计发现（2026-06-15）

> 来源：全量 PRD/设计文档 vs 代码库一致性审计。已修复项标注状态，未修复项记录于此。

### 6.1 已修复（本轮）

| # | 模块 | 问题 | 修复方式 |
|---|------|------|----------|
| ~~F-01~~ | 05 AI引擎 | `decide_fused()` 断言 48 维，应为 78 维 | ✅ 已修复 (v3.1) |
| ~~F-02~~ | 05 AI引擎 | `to_input_vector()` capacity 76，应为 78 | ✅ 已修复 (v3.1) |
| F-03 | 04 策略引擎 | `ControlCommand` 缺 `p_ref`/`k_droop`，用旧字段 `p_batt_set` | 代码：新增 p_ref+k_droop，p_batt_set 标记 deprecated |
| F-04 | 02 南向通信 | `DeviceType` 枚举缺 `Hplc` 变体 | 代码：新增 Hplc |
| F-05 | 03 数据处理 | `FaultRecorder` trait 缺 `update_trigger_config()`/`get_trigger_config()` | 代码：新增默认实现 |
| F-06 | 06 安全 | 文档声称 SmCryptoProvider/CertManager/安全启动已实现，实际为存根 | 文档：添加 Phase 2+ 状态说明 |
| F-07 | 09 运维通信 | 文档描述 WiFi/NearLink/BLE/ECDH 为已实现，实际为 NoOp 占位 | 文档：添加 Phase 2+ 状态说明 |
| F-08 | 07 OTA | 文档描述 A/B分区切换/cgroup v2/硬件看门狗/OOM 为已实现，实际未实现 | 文档：添加 Phase 2+ 状态说明 |
| F-09 | 03 存储 | 文档描述 alarm_log/event_log/TELEMETRY_HISTORY 分区表存在，实际不存在 | 文档：记录实际 schema |
| F-10 | 08 Web | SSE 缺 `RewardsUpdate`/`FinetuningUpdate` 事件类型 | 代码：添加 TODO |
| F-11 | 05 AI引擎 | v3.0 训练-部署输入维度不一致（ONNX shape (T,7) vs Rust (24,)） | ✅ 已修复（v3.0）：`LstmConfig` 新增 `input_features`，`HistorySample` 7 字段，VMD 多特征自动降级 |

### 6.2 未修复 — 架构级变更（需独立规划）

| # | 模块 | 严重程度 | 问题 | 阻塞原因 |
|---|------|----------|------|----------|
| U-01 | 08/05 Web/AI | **P0** | 角色鉴权未实现 — `RequireRole` 硬编码返回 `Role::Admin`，`login()` 硬编码 `role: "operator"` | 需设计完整 RBAC 中间件（token管理、角色存储、权限矩阵） |
| U-02 | 03 存储 | **P0** | sqlx/rusqlite 双库并存 — `storage` crate 用 sqlx，`data-processing` 用 rusqlite | 需统一迁移方案，评估 sqlx 对故障录波 rusqlite 功能的覆盖度 |
| U-03 | 06 安全 | **P0** | SM2 签名/SM4 GCM/SM3 HKDF/SM2 ECDH 实际使用 ring 模拟，非真国密 | 需 gmsm crate 升级至 0.14（当前 0.1.0 缺少签名/GCM/HKDF/ECDH API） |
| U-04 | 06 安全 | **P0** | `SmCryptoProvider` 未实现 `rustls::CryptoProvider` trait，无法用于 TLS | 需完成 SM2/SM3/SM4 的 rustls 密码学套件适配 |
| U-05 | 06 安全 | **P0** | 安全启动全部为存根 — `verify_boot_chain()` 直接返回 `Verified` | 需 RK3588 硬件信任根（OTP/eFuse）驱动支持 |
| U-06 | 09 运维通信 | **P0** | WiFi/NearLink/BLE 全部 NoOp 驱动，ECDH 为 XOR 占位 | 需 Hi2821 硬件 + hostapd/wpa_supplicant 集成 |
| U-07 | 07 OTA | **P1** | 固件 OTA — A/B 分区 `switch_to_standby()` 仅打印日志，SM2 验签返回 `SignatureInvalid` | 需 bootloader 环境变量写入权限 + SM2 验签就绪 |
| U-08 | 07 监控 | **P1** | cgroup v2 管理、网络 I/O 监控、硬件看门狗、OOM score_adj 均未实现 | 需内核配置验证 + 硬件看门狗驱动 |
| U-09 | 03 存储 | **P1** | SQLite schema 与 PRD 不符 — 缺 alarm_log/event_log/device_nameplate/maintenance_record 表，遥测无按月分区 | ✅ 已修复 — 新增 5 张表（含索引）+ 遥测按月分区函数 |
| U-10 | 03 数据 | **P1** | `TriggerConfig` 仅单个 `enabled: bool`，设计文档要求 7 个独立启用标志 | ✅ 已修复 — 新增 `TriggerEnableMask`（7 位独立使能） |
| U-11 | 06 安全 | **P1** | `CertManager` API 与设计文档对齐 — 缺 import_cert/import_crl/reload/list_certs | ✅ 已修复 — 新增 4 个方法 + CertType 枚举 |
| U-12 | 08 Web | **P1** | SSE 基于 query 的过滤 (`types=`) 未实现 | ✅ 已修复 — sse_handler 支持 ?types= 参数过滤 |
| U-13 | 05 AI引擎 | **P2** | `RunningMode` 代码用 `SeasonalLoadManagement`，PRD 文档用 `AgriculturalIrrigation` | ✅ 已修复 — 统一采用 `SeasonalLoadManagement`（代码+文档） |
| U-14 | 07 OTA | **P2** | crate 目录名 `ota-update/`，设计文档用 `update-engine` | ✅ 已修复 — 设计文档统一为 `ota-update`（ADR-007 决议更新） |
| U-15 | 05 AI引擎 | **P1** | v2.17 SafetyRLWrapper `event_sender` 未连接 — broadcast channel 需 main.rs 组装，当前 `ModelManager::new()` 传入 `None`，违规时无 SSE 实时推送 | ✅ 已修复（2026-06-22）：`ModelManager::new()` 创建 broadcast::channel(64)，`subscribe_safety_events()` 暴露给 SSE 推送链路 |
| U-17 | 05 AI引擎 | **P0** | `safety_wrapper.rs` 测试编译失败 — `predict()` 签名为 `(state, action, p_cur)`（3 参数），但 `:461`、`:482` 处测试仅传 2 个参数 | ✅ 已修复（2026-07-06）：两处测试调用补 `0.0` 作为 `p_cur` 实参 |
| U-18 | 10 MQTT | **P0** | `mqtt-plugin/tests/mqtt_tests.rs` 编译失败 — `use mupc_mqtt_plugin::` 引用错误 crate 名，应为 `mqtt_plugin` | ✅ 已修复（2026-07-06）：`use mqtt_plugin::` 对齐 Cargo.toml lib name |

### 6.3 v3.0 新增存根/TODO 项（P1/P2）

| # | 模块 | 严重程度 | 问题 | 说明 |
|---|------|----------|------|------|
| U-19 | 05 AI引擎 | **P1** | `prediction_pipeline.rs` EC runtime 推理存根 — `ec_runtime.run()` 调用点为 `TODO`，返回 `vec![0.0]` | ✅ 已修复（2026-07-06）：接入 `ec_runtime.run()` 实际推理，f32→f64 转换，失败时优雅降级为零修正 |
| U-20 | 03 数据 | **P1** | `fault_recorder_impl.rs` 4 个方法为存根：分页查询、waveform 元数据、文件系统读取、统计聚合 | ✅ 已修复（2026-07-06）：接入 WaveformReader 读取 .wave 文件 + ComtradeExporter/CsvExporter 实际导出 + 通道统计计算 |
| U-21 | 05 AI引擎 | **P2** | `reward_calculator.rs` v2.8 参数硬编码 — `pv_high_voltage_penalty`、`smooth_lambda` 等未从配置加载 | ✅ 已修复（2026-07-06）：`RewardThresholdConfig` 新增 3 字段，`new_with_thresholds()` 从配置读取 |
| U-22 | 05 AI引擎 | **P2** | `dynamic_config_loader.rs` 使用旧 API — `TODO(Task 6): 替换为 update_action_space_config_full` | ✅ 已修复（2026-07-06）：替换为 `update_action_space_config_full`，补 5 个缺失参数 |
| U-23 | 10 MQTT | **P2** | `mqtt-plugin/src/client.rs` TLS 证书加载存根 — `TODO: Implement proper certificate loading from config paths` | ✅ 已修复（2026-07-06）：`build_tls_configuration()` 从 `MqttConfig` 路径加载 CA/客户端证书/密钥 |
| U-24 | 07 OTA | **P2** | `ota-update/src/manager.rs` OTA 服务器请求存根 — 返回硬编码模拟数据 | ✅ 已修复（2026-07-06）：`query_versions_from_server()` 通过 reqwest 发 HTTP GET 请求，网络不可达时优雅降级 |
| U-25 | 03 数据 | **P2** | `waveform/report.rs` 4 个 TODO — IEC 104 文件传输、MQTT 桥接对接 | 🔴 阻塞 — 需 gateway IEC 104 TI=122 文件传输 + MQTT 主题发布先完成 |

### 6.4 U-16 根因分析

**发现过程**：2026-06-21 启动新一轮完整训练，重新审视训练-部署全链路对齐，逐一比对 `/work/MUPC-AI` Python 训练管线和 `/work/MUPC` Rust 推理侧的数据流，发现两者 ONNX input shape 不一致。

**为什么之前的 review 没发现**：

| 审阅节点 | 审了什么 | 为什么漏了 |
|----------|----------|-----------|
| v2.16 技术债审计 (6/15) | `to_input_vector` 48→78 维、FusedSystemState 字段对齐 | 审的是 **Rust 内部**一致性，未对比 Python 训练管线 |
| 专家评审 (6/14) | 奖励函数、状态空间、安全约束、训练收敛 | 审的是"**策略质量**"，输入特征维度不在评审范围 |
| v2.17 VMD/PredictionPipeline 重构 (6/18-19) | VMD 分解、Attention、BiLSTM、误差修正 | VMD 是在**单变量 1D 假设**上叠加信号处理增强，未质疑这个假设 |
| CNN-LSTM TODO (6/19) | **明确写了** 7 维 vs 1 维 Gap | 但定位为"改造要求文档"，不是"已存在的 bug 必须修复" |
| 各次 PR review | 单侧仓库内的 diff | 无人同时打开两边代码比对 |

**根因**：`LstmInput.history: Vec<f32>` 类型签名太弱。编译期不区分 `24 个 f32（单变量）` 和 `168 个 f32（7 维 × 24 步）`。如果 Rust 类型系统在编译期就表达 `(T, K)` 维度约束，这个不一致会在编译时暴露。

**教训**：
1. 跨仓库接口契约（ONNX input/output shape）应有**编译期或 CI 自动化校验**，不能依赖人工 review
2. TODO/设计文档中标记的 Gap 应纳入评审门禁，不能长期停留在"已知待改"
3. 类型签名应携带维度语义：`Vec<f32>` → `ndarray::Array2<f32>` 或 newtype wrapper

### 6.5 第二轮深度审视 — 流程改进教训（2026-08-14）

> 来源：`docs/TODO/全项目实施深度审视报告-2026-08-14.md` 第二轮深度审查。本轮聚焦「接线层消费者是否为存根/空壳」，发现 3 个 P0（AI 决策输入非真实遥测、命令下发消费者恒失败、核间通信半通道）。以下为审查方法论层面的改进要求，非具体代码 bug。

| # | 类别 | 严重程度 | 改进要求 | 背景/依据 |
|---|------|----------|----------|-----------|
| P-01 | 审查方法 | **P0** | 建立「闭环验证」——跨模块通道必须验证「生产者 + 消费者 + 被调用 + 结果正确」，而非仅确认「有调用」 | 上一轮仅验证「接线」存在，未验证消费者是否产生正确结果，导致 `fuse()` 空状态、命令下发恒失败、核间半通道未被发现 |
| P-02 | 状态管理 | **P0** | 消除状态虚报——`register_service(Running)` 前必须确认组件真实运行 | security/wireless/data_processing/message_bus 实际空壳却上报 Running |
| P-03 | 代码清理 | **P1** | 清理或标注死代码——约 20+ 个孤儿模块应明确「门控停损」或删除 | 区分「真正冗余的重复实现」与「未被接线的接口预留」，删除前确认后续设计完整覆盖其意图（见深度审视报告第六节架构演进说明） |

### 6.6 台区储能治理策略投运前置项（2026-08-31）

> 来源：`docs/superpowers/plans/2026-08-31-台区储能治理策略-实施计划.md` 最终代码审查（b136a03..86acefa 范围）。策略代码已完成并通过终审（可合并），以下为**现网启用前必须关闭的前置项**——未关闭前该策略在生产中为空转/未验证。

| # | 类别 | 严重程度 | 问题 | 依据/处置 |
|---|------|----------|------|-----------|
| U-26 | 数据接线 | **P1** | `ElectricalData.phase` 无生产代码填充——startup.rs/collector.rs/reporter.rs 均设 `phase: None`，仅测试与 `tai_replay` bin 填充分相数据 → 运行时 `data_to_meter` 恒全零，三目标（降返送/降不平衡/提 PF）现网零达成 | 需接通台区总表分相数据源（设计 04 文档 §15.7 待接依赖）；安全（零设定不误动作），但投运必办 |
| U-27 | 现场验证 | **P1** | Q 通道方向符号 `s_q_sign=1.0` 未经验证——若 PCS 无功符号约定相反，Q 积分器正反馈发散至 ±q_i_max 并恶化 PF | 设计 §11 第 6 条「现场核相」为强制前置（小幅 Q 阶跃 + 分相注流验证符号），投运必办 |
| U-28 | 协议语义 | **P2** | AI 恢复后无分相设定清零——兜底期间下发的 V3 分相 P/Q 在智能路径只发 V2 双参数、从不发 V3 零设定复位，若实时控制模块按帧序后者覆盖前者则无碍，否则残留分相设定与 AI `p_ref/k_droop` 并存冲突 | 需与实时控制模块确认 V3/V2 帧优先级语义；必要时 AI 恢复时显式下发 `[0,0,0]` 清零 |

---

## 7. 技术债统计

| 类别 | 数量 | 已修复 | 待修复 | 状态 |
|------|------|--------|--------|------|
| 严重问题 (Phase 1) | 2 | 2 | 0 | ✅ 全部修复 |
| 警告问题 (Phase 1) | 4 | 4 | 0 | ✅ 全部修复 |
| 优化建议 (Phase 1) | 3 | 3 | 0 | ✅ 全部修复 |
| 审计发现-已修复 (本轮) | 11 | 11 | 0 | ✅ 全部修复 |
| 审计发现-未修复 (P0) | 6 | 0 | 6 | 🔴 待规划 |
| 审计发现-未修复 (P1) | 6 | 4 | 2 | 🟡 待排期 |
| v2.17 新增 (P1) | 1 | 1 | 0 | ✅ 已修复 |
| 审计发现-未修复 (P2) | 2 | 2 | 0 | ✅ 全部修复 |
| v3.0 新增 — P0 编译错误 | 2 | 2 | 0 | ✅ 已修复 |
| v3.0 新增 — P1 存根/TODO | 2 | 2 | 0 | ✅ 全部修复 |
| v3.0 新增 — P2 存根/TODO | 5 | 4 | 1 | 🔵 1 项被依赖阻塞 |
| 流程改进教训 (第二轮 2026-08-14) | 3 | 0 | 3 | 🟡 待落地 |
| 台区储能投运前置项 (2026-08-31) | 3 | 0 | 3 | 🔵 投运前必办（U-26/U-27/U-28） |
| **总计** | **50** | **35** | **15** | |

---

## 8. 修复计划

### 8.1 紧急修复（P0 — 编译错误）✅ 已完成（2026-07-06）

| 问题 | 状态 | 说明 |
|------|------|------|
| U-17 safety_wrapper 测试签名不匹配 | ✅ | `predict()` 调用补 `p_cur: 0.0` 参数 |
| U-18 mqtt-plugin 测试 crate 名错误 | ✅ | `use mqtt_plugin::` 替代 `mupc_mqtt_plugin` |

### 8.2 v3.0 P2 清理 ✅ 已完成（2026-07-06）

| 问题 | 状态 | 说明 |
|------|------|------|
| 4.3 Windows 日志路径 | ✅ | 已有 `cfg!(windows)` 编译时检测 |
| U-21 reward_calculator 硬编码参数 | ✅ | `RewardThresholdConfig` 扩展 3 字段 |
| U-22 dynamic_config_loader 旧 API | ✅ | 替换为 `update_action_space_config_full` |
| U-23 mqtt-plugin TLS 证书加载 | ✅ | 从 `MqttConfig` 路径加载 CA/客户端证书 |
| U-24 ota-update OTA 服务器请求 | ✅ | reqwest HTTP GET + 优雅降级 |

### 8.3 短期修复（P0/P1 — 1-2 周）

| 问题 | 预计时间 | 阻塞 |
|------|----------|------|
| U-01 RBAC 鉴权中间件 | 2天 | 需确认角色模型和权限矩阵 |
| U-02 sqlx/rusqlite 双库统一 | 3天 | 需评估 sqlx 功能覆盖度 |
| ~~U-19~~ prediction_pipeline EC runtime 接入 | ✅ | 已接入 `ec_runtime.run()` 实际推理 + 优雅降级 |
| ~~U-20~~ fault_recorder 存根实现 | ✅ | 接入 WaveformReader + ComtradeExporter/CsvExporter |

### 8.4 中期修复（P1 — 本月）

| 问题 | 预计时间 | 阻塞 |
|------|----------|------|
| U-07 固件 OTA 分区切换 + SM2 验签 | 3天 | 需 SM2 签名就绪 |
| U-08 系统监控完善 | 2天 | 需硬件看门狗驱动 |

### 8.5 长期修复（外部依赖）

| 问题 | 预计时间 | 阻塞 |
|------|----------|------|
| U-03 SM2/SM4 真国密 | 5天 | gmsm 0.14 发布 |
| U-04 rustls CryptoProvider | 3天 | 依赖 U-03 |
| U-05 安全启动信任链 | 5天 | RK3588 OTP/eFuse 驱动 |
| U-06 WiFi/NearLink/BLE | 10天 | Hi2821 硬件 + 内核驱动 |
| U-25 waveform/report 对接 | 3天 | 依赖 IEC 104 TI=122 + MQTT 主题发布完成 |

---

## 9. 附录

### 9.1 术语表

| 术语 | 说明 |
|------|------|
| TypeID | IEC 104 协议数据类型标识符 |
| Session | Web 会话管理 |
| CRC16 | 循环冗余校验（16位） |
| MupcError | 统一错误类型 |

### 9.2 参考文档

| 文档 | 路径 |
|------|------|
| 代码评审报告 | `mupc/CODE_REVIEW.md` |
| 技术设计文档 | `docs/superpowers/plans/2026-05-27-MUPC-通信管理模块-技术设计.md` |
| 重构验证清单 | `AI_WORKFLOW/04_REFACTOR_CHECKLIST.md` |

---

**记录人**: 项目经理
**记录日期**: 2026-05-27
**最后更新**: 2026-08-31
**版本**: v3.5