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
| v3.6 | 2026-09-16 | 项目经理 | 新增 U-29：`system-monitor` 非 Linux 硬编码 / Linux 读失败回退的**假值输出**（12-显示终端单元 F 整改建议 4.1，只登记不改） |
| v3.7 | 2026-09-23 | 项目经理 | 端到端数据流审查登记（**U-64 ~ U-74**，含 2 项**新需求**）+ **订正 §5 两处台账失真**（消息总线 MQTT「✅ 已完成」→「⚠️ 未接通」；故障录波「4 个方法为 stub」→「查询/导出已修复、上报链路受 U-25 阻塞、生产零触发」）|
| v3.8 | 2026-09-26 | 项目经理 | 新增 **§8.7 Linux 环境交接清单（BECG-3588 联调 + 真机验收累计项）**——承接 §8.6，并把 U-73 外设上屏整链 / U-74 上云 / 落库 / FLS-04 压测的**全部真机档未验项**（M-1…M-9）汇成一页；同时**回写 §8.6 的 L-1/L-2/L-3/L-5 已闭合**（附复核证据）、**L-4/L-6/L-7 仍未闭合** |
| v3.9 | 2026-09-26 | 项目经理 | 新增 **§6.13 PCS 迁入南向的未结项登记**（**U-75 ~ U-81**：写审计未落地 / intercore TCP 无消费者且客户端只发不收 / fail-fast 覆盖形式与其局限 / AI 侧降级口径不对称 / 停机写 500=0 的真机验收项 / `remote_addr` 待裁定 / Task 10 实现状态登记）+ **§8.7 追加 M-10（PCS 迁移后的真机复核项 M-10.1…M-10.8）**。来源 = 02 号设计 **§13**（v1.14，**未获门禁标记**）落地 T1–T12 |

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
| 消息总线 | AMQP/MQTT 进程间通信 | 中 | ⚠️ **未接通**（桥接/插件代码存在，但**无生产者、无配置承载**）—— ⚠️ **本行 2026-09-23 订正**：原文为「**✅ MQTT 已完成**」，与代码空转事实不符。事实：`MqttBridge::publish` 的唯二实现 **无任何调用方**、`core_config.rs:307` 的 `MqttBridgeConfig` **仅 2 个 bool**（缺省双 false）、两份部署 YAML **无 `mqtt_bridge` 段**、`mqtt-plugin::start()` **空实现**（`mqtt-plugin/src/lib.rs:75`）⇒ 北向/本地两个出口均空转（而 `01 设计文档 §4` 有完整两层 MQTT 设计）。详见 **§6.12 U-71**。**订正缘由**：端到端数据流审查（2026-09-23）判为「缺陷·台账失真」——本表此前的"已完成"是**功能清单式**计数（模块/代码存在），不是接线后的可用性 |
| 安全增强 | MQTT over TLS、国密 SM2/SM4 | 中 | ⚠️ 部分实现（SM3/SM4 CBC 完成；SM2签名/SM4 GCM 待 gmsm 0.14） |
| 协议扩展 | IEC 61850-7-420 | 中 | ⚠️ 部分实现（原始TCP+自研ASN.1，待 libIEC61850 FFI） |
| AI 优化引擎 | LSTM/TCN 预测、MADDPG/PPO 决策 | 低 | ✅ 已完成 |
| 在线微调 | PER/KL/影子模型/热切换 | 中 | ✅ 已完成（2026-07） |
| OTA 升级 | 差分升级、安全启动 | 低 | ⚠️ 模型OTA完成；固件OTA骨架就位（A/B分区/签名待实现） |
| 故障录波 | 完整实现 | 中 | ⚠️ **能力就绪、链路未接通** —— ⚠️ **本行 2026-09-23 订正**：原文为「⚠️ 4个方法为 stub（get_waveform等）」，与 §6.3 U-20「✅ 已修复（接入 WaveformReader + Comtrade/Csv 导出）」**互相矛盾**，且与本表 U-25「🔴 阻塞 — 需 gateway IEC 104 TI=122 + MQTT 主题发布先完成」三处口径不一。**订正后的一致口径**：① **查询/导出已修复**（§6.3 U-20，本次复核属实）；② **上报链路仍阻塞**（U-25，依赖 IEC 104 文件传输 + MQTT 主题发布，见 §8.5）；③ **新增如实登记：生产路径零触发** —— `FaultRecorderImpl` 仅在 `mupc-core-bin/src/startup.rs:755` **构造保留**（避免创建后立即 drop），全仓**无采集触发调用点**（`data-processing/src/fault_recorder_impl.rs` 的调用者仅测试）⇒ 即便前两项齐备，录波当前也不会被**生产**触发。**订正缘由**：端到端数据流审查（2026-09-23）判为「台账自相矛盾」（报告 §三 缺口 #5 / §五 C 类） |
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

### 6.7 跨模块缺陷登记（12-本地显示终端 单元 F 整改，2026-09-16）

> 来源：12-本地显示终端工作单元 F 代码评审（`REQUEST_CHANGES`）建议 4.1。**本项只登记、不改**（`system-monitor` 不在本单元授权范围）。

| # | 类别 | 严重程度 | 问题 | 依据/处置 |
|---|------|----------|------|-----------|
| U-29 | 假值输出 | **P1** | `mupc_system_monitor` 的采集器在**非 Linux 目标**返回**硬编码常量**（`MemoryCollector` 8192 / 4096 / 4096 MB / 50%，`crates/system-monitor/src/collectors.rs:190-195`；`TemperatureCollector` 48.0 ℃，同文件 `:294`）；**Linux 读失败时也回退 45.0 ℃**（同文件 `:282`）。该值**无任何"不可得"标记**，任何采信方都会把它当**真值**上屏/上报，违反本项目「显 `--`、**严禁补 0 / 不造假值**」口径 | 现状：12-显示终端已**绕开**该模块（`display_host.rs` 直读 `/sys/class/thermal/thermal_zone0/temp` 与 `/proc/meminfo`，非 Linux 一律 `None`，见 `device_source_no_stub_metrics_on_non_linux` 用例）。**根因未修**：`system-monitor` 的 `MetricSnapshot` 缺「字段不可得」表达（现为裸 `f64`）⇒ 需把回退值改为 `Option<f64>`（或加 `available` 标记）再谈采信；在此之前**不得**把该模块的 CPU 温度 / 内存值直接上屏或上报 |

---

### 6.8 G-2 整改登记（12-本地显示终端 配置写路径，2026-09-16）

> 来源：12-本地显示终端工作单元 G-2 代码评审（`REQUEST_CHANGES`）的 3 阻塞 + 2 重要 + 6 建议整改。
> 阻塞项**已修**（见 `mupc/crates/mupc-core-bin/src/{idempotency,config_service,console_host}.rs`
> 的对应用例与注释）；本表只登记**本轮不修**的残余与跨模块项。

| # | 类别 | 严重程度 | 问题 | 依据/处置 |
|---|------|----------|------|-----------|
| U-30 | 既有测试失败 | **P2** | `mupc-data-processing` 的 `waveform::trigger::tests::test_cooldown` **确定性失败**（评审已复核为**既有**失败，且**不在** CLAUDE.md「已知测试失败」表内）：`crates/data-processing/src/waveform/trigger.rs:481` 断言 `left: None / right: OverVoltage`（冷却窗口判定未触发） | **只登记不修**（不在本单元授权范围）。复现：`cargo test -p mupc-data-processing -j 2 --lib -- waveform::trigger::tests::test_cooldown` |
| U-31 | 保留式编辑能力边界 | **P1** | `yaml_edit` **不支持"段/键缺失时追加标量行"**：`locate_scalar` 找不到段/键即 `Err` ⇒ 部署 yaml **缺某段**时，该段字段的**首次**写入必然落到整体回写（`WriteMode::FullRewrite`）⇒ **该文件全部注释与未建模段丢失**。评审阻塞 3 的实证：`deploy/config/*.yaml` 原先**都没有 `gateway:` 段**，而"设 IEC-104 监听地址"是**投运必做动作**（L2+ 强确认） | **缓解（已做）**：① 两份 deploy yaml 补 `gateway:` 段；② 以**真实配置文件**为输入的往返用例 `config_service::tests::deploy_configs_support_preserve_edit_for_every_editable_field`，逐 `editable` 键断言"可定位 + 除目标行外逐字节不变 + 注释行数守恒"。**根治**（支持建段/追加）登记为后续单元或 PM 裁定：建段须定插入位置、缩进风格、行尾风格，并与"段内已有同名键"的判重语义对齐 |
| U-32 | 写入侧崩溃窗口（残余） | **P2** | `atomic_write` 的「`rename(真源→.bak)` → `rename(.tmp→真源)`」两步之间存在"真源不存在"窗口；此刻掉电/被 kill ⇒ **恢复前的那次启动**读不到配置 | **已补**：启动期加载 `config_service::load_config_with_backup_recovery`（真源缺失而 `.bak` 在 ⇒ 恢复并 `eprintln!` 响亮记录），单测 `startup_load_recovers_the_source_from_backup_after_a_crash_window`。**残余**：恢复是**事后**的（窗口内这次启动仍失败）；彻底消除需改写序（先 `rename(tmp→path)` 再复制 `.bak`），那会放松 `.bak` 与真源的对应关系 ⇒ 登记为后续/PM 裁定 |
| U-33 | 落盘健壮性（未实现） | **P2** | ① `rename` 后**目录 fsync 未做**（掉电理论上可能丢 rename）；② `.tmp` 由 `File::create` 创建 ⇒ **权限取 umask**（典型 0644），现场 0600 的真源会被写成 0644 | **如实登记、本单元不实现**：两项都**只对 `cfg(unix)` 有意义**（Windows 的 `sync_all` 对目录句柄不可用、`set_permissions` 只表达只读位），而本单元验证环境是 Windows ⇒ 写了也**给不出可红回归**。落点已在 `config_service.rs::atomic_write` 的文档注释里写明（③ 之后 / ① 与 ② 之间） |
| U-34 | 跨模块缺口（渲染层） | **P1** | **屏面在反向陈述**：`local-display/src/ui/pages/p2_config.rs:114`（常驻说明）与 `:159`（`TEXT_IMPACT_SAVE`）都写「修改保存后立即生效 · **无需重启装置**」，而 **7 个可写字段里有 6 个**事实上需重启；且成功分支（`:1845-1848`）**丢弃**后端 `message`（只用固定「保存成功 · 已生效」）⇒ 后端（回执 `message` / 审计 `reason` / `tracing::warn!`）三处都说真话，**用户在受理端看不到**。设计 §4.3.5 明写该降级须 **PM 同意并回写 PRD（CF-04 降级）**，不得静默实施 | **本单元只登记不改**（`local-display/**` 不在授权范围）。**PM 裁定项**：最小改法 = ① 两处文案改为"连接类参数保存后**需重启 mupcd** 生效（日志级别立即生效）"；② 成功分支把 `resp.message` 并入 Toast 文案（`full_rewrite` 已有单条 Toast 的 PD10 口径可循） |
| U-35 | 测试覆盖（未做） | **P2** | **真实磁盘故障注入用例缺失**：`atomic_write` 的 ENOSPC / 权限拒绝 / 备份 rename 失败等路径未覆盖（现有真实失败注入只有"审计目录不可建"，属 `console_audit` 侧） | 登记。可行落点：注入一个"只读目录"或"真源路径是目录"的真实失败形态（跨平台），断言 `.tmp` 不残留、`.bak` 不动、真源内容不变（EDGE-10） |

---

### 6.9 单元 K 整改登记（`web-api` 整 crate 移除，2026-09-18）

> 来源：单元 K 整改，两轮评审均为 `APPROVE_WITH_CONCERNS`（**无阻塞**）——
> 第一轮规格评审 3 重要 + 4 建议；第二轮代码质量评审 4 重要 + 8 建议。
> 已在**代码/文档就地订正**的项不在此表（第一轮 ②/③ 与第二轮 ①/②/③ 的订正动作见对应文件注记）；
> 本表只登记**取舍得当但会"只活在注释里"**、以及**本轮不动**的残余。

| # | 类别 | 严重程度 | 问题 | 依据/处置 |
|---|------|----------|------|-----------|
| U-36 | 线格式余量（取舍登记） | **P3** | `mupc_common::ErrorCode` 的 `0x0400-0x04FF` 段（原「Web API 错误」）在 `web-api` 删除后**只余 1 个使用者**：`AuthFailed = 0x0400` / `InvalidSession = 0x0401` 全仓无调用点，仅 `ConfigError = 0x0402` 仍被配置加载路径真实使用 | **本轮取值：整段保留、不删变体**（理由：删变体 = 改**线格式编号**，须同步 `ErrorCode::from_u16` 双向映射、`mupc/deploy/deploy.md` 的 `0x0402` 排障检索口径，以及 `display-proto` / 核间帧的序列化兼容——**不属于单元 K 授权范围**）。⚠️ **登记目的**：该取舍原先**只写在 `crates/common/src/error.rs:40-46` 的代码注释里**，无文档落点（评审建议 S-1）。后续若要回收该段编号，须按"协议版本 + 两端同时升级"处理 |
| U-37 | 文档与契约不一致（登记） | **P2** | `mupc/deploy/deploy.md` §十 的核对表把 `min_publish_interval_ms` 约束写成 **`∈ [200, publish_ms]`**，而**契约**（唯一真源）`display-proto/src/config.rs::MIN_MERGE_WINDOW_MS = 100`、设计 §4.9/§11.1 均为 **`∈ [100, publish_ms]`** | **已订正为 `[100, publish_ms]`**（本轮同步订正：该表正是 `display:` 段的现场核对清单，与本次补 yaml `display:` 段同一处；仅改这一个数值，未动表结构）。若此类"文档比契约严/松"的偏差应统一按"登记 + 独立单元"处理，请评审否决本处订正并回退。**⚠️ 补记（K 收尾 Q-1）**：**同一句话的孪生残留**在 `mupc/crates/mupc-core-bin/src/core_config.rs:544`（`validate_display()` 的文档注释——它描述的正是同一套 fail-fast 门禁，是维护者读的第一手文档），也已订正为 `∈ [100, publish_ms]` 并注明"下界 = 契约 `MIN_MERGE_WINDOW_MS`"。**全仓清点**（`min_publish_interval_ms` 的**约束描述**）：`deploy.md` §10.2 表 / 两份 `deploy/config/*.yaml` 注释 / `core_config.rs:544` / 契约 `display-proto/src/config.rs:37` / 设计 §4.9·§11.1 —— 全部为 `[100, …]`，**订正后全仓无 `[200, …]` 残留**。**⚠️ 再次变动（A-2，2026-09-18，见 §6.11 / U-44）**：本行所依据的"契约 = `[100, …]`"**已作废**——设计 §4.2.1 约束 2 要求 `≥250 ms`，PM 裁定契约取 **250**，故本行提到的**全部落点**（deploy.md §10.2 / 两份 yaml 注释 / `core_config.rs` 文档注释 / 契约常量与注释 / 设计 §4.2.1·§4.9·§11.1）已**同步订正为 `[250, publish_ms]`**；本行的历史叙述保留（它记录的是 K 轮那次"200→100"的订正动作，不改写历史） |
| U-38 | 文档与实现不一致（登记） | **P3** | 设计 §4.9 的字面稿写 `tracing::info!("[10/14] 初始化本地 HMI 后端...")`，而实现里**没有该行日志**、且序号体系已重排（HMI 装配并入步骤 8「策略引擎+决策循环之后」，`[10/14]` 现为 OTA） | **本轮已补日志行**（`startup.rs` 的 `if config.display.enabled {` 分支首行，**不带序号**——带 `[10/14]` 会与现编号体系冲突）。**残余**：设计 §4.9 那段仍是"步骤 10 整块替换"的草案口径，与实现的 14 步编号不同源 ⇒ 登记待后续统一（改设计编号属跨单元文档动作） |
| U-39 | 公开 API 余量（登记备查，第二轮 S-7） | **P3** | `web-api` 删除后，`mupc_ai_engine::ModelManager` 的两个取用器在**全仓**失去消费者：`online_updater()`（`crates/ai-engine/src/model_manager.rs:558`）**零调用点**；`mode_selector_arc()`（同文件 `:997`）**仅剩 1 处**（`crates/strategy-engine/src/ai_integration.rs:524`）。二者的原始用途正是 web-api 的在线微调 / 模式查询路由 | **本轮不删、只登记**：删公开 API 属"能力删除"，超出单元 K 授权；且 AI 引擎为暂停项，去留应与 AI 恢复计划一并裁。**后续动作（独立单元）**：在"删除 / 保留并如实标注 `#[allow(dead_code)]` / 按新需求重新接线"三者中择一。**在此之前不得**为消 `dead_code` 告警而伪造调用点 |
| U-40 | 悬空文档引用（K 收尾 Q-4 登记） | **P3** | 代码注释里的"**见（…）交付报告**"指向**不随仓库分发**的文档：`mupc-core-bin/src/{config_service.rs:1648, console_host.rs:2568, log_service.rs:92, log_service.rs:2400}` 四处（全仓 `docs/` 下只有 `docs/superpowers/reports/2026-05-27-{Phase1,Phase2}-交付报告.md` **两份**，与这些引用无关）⇒ 维护者照引用**找不到任何东西** | **本轮处置：4 处全部改为自足表述**（就地把依据写清：`config_service.rs` 指向设计 §4.3.5 末段「残余」+ UI 文档 PD24；`console_host.rs` 改为"下面四条是注入式破坏，逐条对应一处变红"；`log_service.rs:92` 就地写清"需 PM 裁定 = 契约为'向前翻历史'新增独立参数（如 `before_seq`）"；`log_service.rs:2400` 就地写出实测数字 `SCAN_READ_BUDGET_LINES + 1 = 200 001`）。**残余（登记，非本轮范围）**：`local-display` crate 下另有 **25 处**同形引用（`src/app.rs` / `src/ui/shell.rs` / `src/ui/tests.rs` / `tests/{control_channel,offscreen_smoke}.rs`，措辞如"改什么会让本条变红（已实测，见交付报告）"）——属另一模块的整改范围，本轮**不动**（其中部分"实测数字"需重跑探针才能补全，不宜凭记忆回填）⇒ 该模块下次重开时按同一口径收口：**要么就地写清依据，要么改指仓库内文档的锚点** |
| U-41 | 评审标签不可独立审计（K 收尾 Q-6 登记） | **P3** | 多轮整改的注释里散落 `S-n` 形式的交叉引用（如 `S-4` / `S-5` / `S-7`），而**每轮评审的 `S-n` 编号各自从 1 起**⇒ **同号跨轮重号**（本轮 `S-5` 与上一轮 `S-5` 指的不是同一条建议）。后果：读者**无法从标签定位**到"哪一轮评审的哪一条"，交叉引用**不能独立审计**（评审复核时只能靠上下文猜） | **本轮不回头重编号历史注释**（改全部历史注释属大规模文本动作，且会让既有评审记录与代码对不上号）——**只登记**。**新写注释即日起改口径**：一律写「**轮次 + 标签**」（本单元 K 收尾的订正写作 `K 收尾 Q-n`，上一轮写作 `第二轮 S-n`），并在**同处一句话**内说明该标签指向什么（不依赖读者去翻报告）。若后续要彻底恢复可审计性，应作为**独立单元**统一改写（含历史注释），不得顺手零敲碎打 |

---

### 6.10 单元 L 收尾整改登记（真字库下 P 值截断 + `LONG_DOTS` 文本缓冲改写，2026-09-18）

> 来源：工作单元 L（测试与交付）两项收尾整改。两条事实**同源**：都**只在真字库在位**时暴露
> （`--features noto-font` + 先跑 `fonts/gen_fonts.sh` 生成 `fonts/lv_font_noto_sc_*.c`），
> **默认构建（`default = []`）下不可见** ⇒ 本地门禁全绿与真机表现**不一致**。
> **本轮口径（PM 裁定）：只登记，不改版式常量** —— 加宽槽位 / 缩小字号 / 换字形由 UI 设计裁定；
> 登记内容与渲染端落点同源（`local-display/src/ui/pages/p1_status.rs` 的 `PHASE_P_SLOT_W` 文档注释）。

| # | 类别 | 严重程度 | 问题 | 依据/处置 |
|---|------|----------|------|-----------|
| U-42 | 渲染端缺陷（真字库下**数据被截断展示**）＋同源的"注释为假" | **P1** | ① **P1 页三相卡带符号 P 值装不下槽宽**：`PHASE_P_SLOT_W = 2 × TextSlot::PhasePower.px()`（`mupc/crates/local-display/src/ui/pages/p1_status.rs:321`；本单元 L 收尾把登记块插在其文档注释里 ⇒ 该常量由 `:279` 移到 `:321`，`12-本地显示终端-测试报告.md` 与本表旧引用已同步订正）= **2 em**（该槽档位 = 64 px，见 `theme.rs` 的 `TextSlot::PhasePower => FontSize::S64` ⇒ 槽宽 **128 px**），而带符号的 1 位小数数值需 ≈ **2.50 em**（`U+2212` 的 `adv_w` **逐档恒等于一个数字**）⇒ **任何负数都越槽**，与数值大小无关。**实测**（真源 `mupc/crates/local-display/fonts/lv_font_metrics.txt`，单位 1/16 px：`−` 与数字 = 568、`.` = 285）：64 px 档 `−12.3` = 4×568+285 = 2557/16 = **159.8 px** > 槽宽 **128 px**（越槽 **31.8 px**）；对照 `12.5` = 124.3 px（恰好放得下，余 3.7 px）、`123.4` = 159.8 px（越槽）。比值**逐档恒定**（≈2.50 em vs 2 em，10 档全部越槽），按 28 px 档折算即 **70.1 px > 56 px**。**后果**：真机上**充电方向（P < 0）的相 P 被 `LongMode::DOTS` 截断成 `−1...`**（label 建在 `p1_status.rs:785-795`；该形式按 DOTS 分支逐字节推算——同路径正值的实测见下条）。② **同源事实：LVGL v9.5.0 的 `LONG_DOTS` 就地改写 label 文本缓冲** —— `mupc/vendor/lvgl/src/widgets/label/lv_label.c:1368` 的 `lv_label_set_dots()`（由同文件 `:1314-1348` 的 DOTS 分支调起）把 `text[dot_begin + i]` 覆写成 `.` 并把尾部置 `\0`（被覆盖字符先存 `dot[]`），仅由 `lv_label_revert_dots()`（`:1357`）在**下一次** `lv_label_refr_text`（`:1111`）/ `lv_label_set_text_vfmt`（`:154`）/ `set_text_internal`（`:988`）时才还原 ⇒ **`lv_label_get_text()` 读回的就是截断串**，"`DOTS` 只影响绘制"**为假**。 | **触发前提**：`noto-font`（**非默认 feature** —— `mupc/crates/local-display/Cargo.toml:54`；`default = []` 见 `:47`）+ 先跑 `fonts/gen_fonts.sh` 生成 10 档 `.c`（不入库，见 `.gitignore:60`）。**为何默认构建看不出来**：默认下 `Font::of(..)` 恒 `None` ⇒ 走 `Font::fallback()`（LVGL 内置 `lv_font_montserrat_14`），`123.4` 在这 2 em 里**放得下**、不触发 DOTS ⇒ **本地全绿是"字库缺席"撑起来的伪绿**，只能由真机 / `--features noto-font` 暴露。**连带必须一并订正的假话**：`ui/tests.rs::pages_chain` 的断言「`` `DOTS` 只影响**绘制**，文本属性仍完整（截断不是丢数据）``」（**工作树 `mupc/crates/local-display/src/ui/tests.rs:3692`**；HEAD 版 `:3268`）在真字库下**必红** —— **本轮实测**（2026-09-18，`cargo test -p local-display --features noto-font -j 2`）：**`368 passed; 1 failed`，唯一红点就是本条**，`left: Some("12...") / right: Some("123.4")`（与默认构建的 `369 passed` 对照，差值即这一条）；`mupc/crates/local-display/src/ui/controls.rs` 第 4 条薄层缺能力原文「`LongMode::DOTS` 只影响**绘制**，`Ipv4Stepper::text` 仍返回完整串 ⇒ 离屏断言结构上抓不到"可视截断"」同属此列（**该假设正是"溢出保护网在真机上恒空转"的原因**）—— **该假前提已于 L 收尾就地订正**（工作树 `controls.rs:50-61`；原文保留在 `:53-54` 的引号内并标注"该前提为假"）。**⚠️ P1 / P4 真机验收硬门禁：本项未处置前不得通过**（与已登记的"字体豆腐块 23 码位"批**同批**处置，那批的登记见 `mupc/crates/mupc-core-bin/src/console_host.rs:752` 与 `interlock_ops.rs:45`）。处置时**须一并裁定**"截断后读回文本"的语义（接受失真 / 改判据 / 另存原文），否则测试口径悬空 |

---

### 6.11 独立评审 A-1/A-2 处置 + 建议 3–10 登记（12 号本地显示终端 v2.0，2026-09-18）

> 来源：对 `origin/master = 91346cb` 的**独立评审**（结论 `APPROVE_WITH_CONCERNS`）。评审抓到
> **2 条确定性可复现的真缺陷**（A-1 / A-2，**均非本次交付引入**）+ 8 条建议（编号 3–10）+ 1 条
> "未发现第四条漏网"的实话（`fs::read_to_string` 无上界）。**PM 裁定**：A-1「修」、A-2「契约下界
> 提到 250」，其余**能就地改的就改、改不了的登记**。逐条落点见下表。

| # | 类别 | 严重程度 | 问题 | 依据/处置 |
|---|------|----------|------|-----------|
| U-43 | **安全相关缺陷（TOCTOU）** | **P1** | **急停释放的 check-then-act 跨 await**：`do_request_release`（`mupc/crates/mupc-core-bin/src/interlock.rs`）先查「`pcs_stop` 源是否复位」（步骤 1），再 `await restore_latched(false)`（步骤 3），**最后**在同一把写锁内清 `latched/stop_failed`（步骤 4）。而 runner 的 `tick_frame` 在**相邻任务**里跑、**不占** `op_in_flight` 闸（那把闸只互斥 `release × ack_m1`）⇒ 操作员在该 await 窗口内**重新按下急停**时，本次 release 仍返回 `Ok(())`、把 latch 清掉、并记一条「触发源已复位且保持期满」的**假审计**（屏上显「已释放」，而急停物理上仍按着）。**探针实测**（`REVY-PROBE A-1`）：`release Ok=true / 锁存被清=true / 而 pin=true`；窗口内那一帧 `transport 动作=[]`（latch 尚未被清 ⇒ tick 无动作）正是它隐蔽的原因。下一帧会自愈（步骤 5 重置状态机边沿 ⇒ 重新触发），但那一拍 `is_latched_now()` 读到 `false`（1 Hz 轮询）⇒ 可能**放行一拍下发** | **已修**（最小修法）：**把「源仍复位」复检挪进清态的那把写锁内**，且用**与步骤 1 同一个函数/同一个数据源**（新抽的 `unsettled_pcs_stop_sources()`）；不满足 ⇒ 回 `SourcesNotReset{remaining}`，**不谎报成功**。因拒绝路径上 transport 已收到本次 `restore(false)`，另补一次 `restore_latched(true)` 回写以防「本机 latched=true / transport latched=false」的状态分裂。常驻用例 `release_rechecks_sources_inside_clear_critical_section`（含 ⑥ 源真复位后仍能成功的正对照）；**破坏性验证**：摘掉复检 ⇒ 红（`left: Ok(())`）。正常语义未变：`interlock::` 34 条全绿（原 33 条 + 新增 1 条）。**顺带订正**：`op_in_flight` / `mark_stop_failed` / `ack_m1` 三处文档原把并发故事讲成**只存在「写 × 写」**，已改为实话（并发还含 **runner × 写**） |
| U-44 | 契约缺陷（安全/验收相关） | **P1** | **契约下限比设计松**：`MIN_MERGE_WINDOW_MS = 100`（`display-proto/src/config.rs`），而设计 §4.2.1 **约束 2** 明写 `min_publish_interval_ms ≥ 250 ms` ⇒ `min ∈ [100, 249]` 一族**静默放行**（§4.2.1 的算式前提被自己放开） | **已修**：契约取 **250**（PM 裁定，唯一真源）。同步处：契约常量注释 / `config.rs` 用例（`at_lower`、新增成对边界用例 `merge_window_lower_bound_250_paired_boundary`：**249 拒 / 250 过**）/ `core_config.rs:544` 文档注释 / 两份 `deploy/config/*.yaml` 注释 / `deploy/deploy.md` §十 核对表 / 设计 §4.2.1·§4.9·§11.1（就地记「以契约 250 为准」）。**破坏性验证**：常量改回 100 ⇒ 红。**现场核对**：仓库两份 yaml 均为 `min=250 / publish=1000`，**无配置落在 [100,249]** |
| U-45 | 设计缺项（**已关闭**） | **P2** | **`publish_ms` 无上界**（`display-proto/src/config.rs::validate` 只有下界 `MIN_PUBLISH_MS = 100`）⇒ 评审实测的 `min = publish = 60_000` **在 A-2 之后仍能过校验**（端到端 ≈61 s，验收 ≤2 s）。A-2 抬的是**下界**，关闭的是 `min ∈ [100,249]` 一族，**没有**关掉这个退化组合 | **已关闭（B3-2d，2026-09-18）**：**PM 裁定 `publish_ms` 上界 = 4000** ⇒ 契约新增 `display-proto/src/config.rs::MAX_PUBLISH_MS = 4000`，`validate()` 在**下界之后**补拒判（越界报 `display.publish_ms`，文案含键名 / 上界值 / 「上界」字样）。**依据**：设计 §4.2.1 的验收是上屏 ≤2 s，而主拍慢于**最慢的一段采集**（`device_poll_ms` ≤4000）时该节的拆解表失效 ⇒ **上界大于最慢的一段采集没有任何验收意义**，它的价值是把 `min = publish = 60_000` 这类退化组合**挡在启动期**；取值与 `MAX_DEVICE_POLL_MS` **同量级**（数值相同属量级对齐，**非耦合**——两条各自服务不同指标：本条服务 §4.2.1 的 ≤2 s，那条服务 F6.3 的 ≤5 s，任一指标重算只动自己那条）。**同步处**：契约常量与文档注释 / `publish_ms` 字段文档 / `display-proto/src/lib.rs` 再导出 / 新增成对边界用例 `publish_ms_upper_bound_4000_paired_boundary`（**4000 过 / 4001 拒**）/ 原 `degenerate` 断言**改为新口径**（`min = publish = 60_000` 现在**必须被拒**，非放宽而是收紧）/ `core_config.rs` 文档注释 + 转发网用例（`display_non_address_invariants_now_gate_startup` 增 `publish_ms: 4001` 一条，5→6 条）/ `deploy/deploy.md` §10.2 核对表与段内 yaml 样例注释 / 两份 `deploy/config/*.yaml` 注释 / 设计 §4.2.1（新增**约束 6**）·§4.9·§11.1。**破坏性验证**：摘掉上界拒判 ⇒ `publish_ms_upper_bound_4000_paired_boundary` 与 `merge_window_lower_bound_250_paired_boundary` **同时变红**（实测 `72 passed; 2 failed`）；`cp` 还原后逐字节 `cmp` 一致、复绿。**现场核对**：仓库两份 yaml 均为 `publish=1000 / min=250`，**落在 [250, 4000] 内** ⇒ 升级后不会因本上界启动失败 |
| U-46 | 孤儿义务（**改挂 U 号**） | **P2** | `console_host.rs` 模块头原写「**收口点（登记给单元 K）**：第三条写管线出现前把 `ConfigService::apply` 与 `InterlockService::handle` 的步骤 2–8 编排骨架抽成公共件」——而 **K 已收尾且范围不含此事**，该义务**未进任何登记表**，触发条件（"第三条写管线出现"）可能**永不触发** ⇒ 无人持有、无人复核 | **改挂本表 U-46**（不结案：触发条件原文保留、不弱化——「写管线条数」确是这件事真正变质的判据）。**为什么选"改挂"而不是"显式结案"**：结案等于断言"不需要做"，而评审判定的是"**现在不该做**"，不是"**永远不**做"；第三条写管线一旦出现，"各写一遍"就从两处冗余变成**系统性分叉**，届时必须有人做。改挂后义务有唯一文档落点、可在技术债盘点时被周期性重估。代码侧已同步改指（`console_host.rs` 模块头） |
| U-47 | 文案过期（**已就地订正**） | **P3** | `console_host.rs::not_implemented`（501 兜底）的响应体写 `(G-1 scope: GET /v1/console/config)`，而该端点在 G-2 就已实现、8 条契约端点现**全部有 handler** ⇒ 这句会把 501 的成因指向一个**根本不会返回 501** 的端点（误导排障） | **已就地订正**：改为只说成因（"route wired into the console router without a handler; all 8 contract endpoints of the v2.0 set are implemented"），不点具体端点；`not_implemented_handler_is_a_bare_501_never_a_parseable_envelope` 仍绿（该用例只钉"非空 + 非 JSON 信封"）。**为什么保留该 handler 本身**：它是"将来往 `console_router()` 里新增路由时"的兜底（生产不可达），删掉会让新增路由**静默 404**（= 假装没这条路径，与"诚实 501"的口径相反） |
| U-48 | 死字段 + 屏面失真（登记） | **P3** | `InfoSection.service_scope`（契约字段：`display-proto/src/frame.rs:336`）：生产端**恒** `LoopbackOnly`（`mupc-core-bin/src/display_host.rs:563`），消费端**零引用**（P6 页用的是**编译期常量** `DEFAULT_BIND` / `DEFAULT_CONTROL_BIND`，见 `local-display/src/ui/pages/p6_system.rs:619` 的 `local_addr_text()`；`p6_system.rs:645` 的 `service_scope_text()` 自己也注明"仅供评审核对口径、**不直上屏**"，见 `ui/tests.rs:2375`） | **登记·不删**：删字段 = 改**线格式**（v2.0 契约已冻结、渲染端与 mupcd 同步升级才行）⇒ 超出本单元授权。⚠️ **此前无人登记的关键一点**：因为 P6 取的是**编译期常量**，**`display.bind_addr` 一旦被改（yaml 或控制通道热写），屏上「本机服务地址」一行立刻失真**（仍显示契约默认端点）——该失真与 `service_scope` 死字段**同源**（都源于"该字段没有真正接到配置"）⇒ 一并登记。修法候选（后续单元）：把 P6 该行改为取**真实绑定结果**（帧内新增字段或 `DEFAULT_*` 之外的真源），届时 `service_scope` 才有对象 |
| U-49 | 死字段 + 缺并发控制（登记） | **P3** | ① `ConfigView.revision`（`display-proto/src/control.rs:411`）由写服务产出（`config_service::ConfigService::revision`）、**渲染端生产代码零消费**（只有用例引用，如 `local-display/src/control_route.rs:508`）⇒ 死字段；② 配置写**无乐观并发**：`ConfigPatch`（`display-proto/src/control.rs:570-575`）只有 `changes` / `from`，**没有** `observed_revision` ⇒ 两台客户端（或"屏 + 未来某个通道"）并发改同一份 yaml 时，后写者**不会**因"我看到的是旧版本"被拒（联锁写路径已有 EDGE-19 的 `observed_latched` / `observed_sources` 同款前置，配置写**没有**） | **登记·不扩面**：`revision` 是**乐观并发的天然载体**（写服务已在递增），故两项**应一并处置**（单独删 `revision` 会把这个载体删掉；单独加 `observed_revision` 又要改**已冻结的 v2.0 契约**）⇒ 作为**契约 v2.1 候选项**登记，待 PM 裁定"本地单屏场景下是否值得"（当前唯一写者是本机屏，冲突面窄，故不升级为 P1） |
| U-50 | 检查半边无判别力（登记） | **P3** | EDGE-19 乐观并发的 `sources` 半边**当前无判别力**：`mupc-core-bin/src/interlock_ops.rs:236-242` 的比对是 `observed_sources`（渲染端取的**全量源名**，IL16）vs `st.sources.iter().map(|s| s.name)`（`interlock.rs::status_sources()` 返回**全部已配置源**，含 `tripped` 但**比对时被丢弃**）——两边都是**全量名集合**、都不含"是否触发" ⇒ **只能检出"配置变了"（`cfg.di` 增删源），检不出状态变化**。真正的状态判据只有同一条件里的 `observed_latched` | **登记**（措辞如实：**该半边当前无判别力**，不是"有缺陷"——它挡住的是一类真实但少见的漂移：`cfg.di` 在运行期被改）。若要让它有判别力，须把 `InterlockSourceStatus.tripped` 纳入比对（**但**见 U-51：`tripped` 是**未去抖**原始电平，直接比会引入抖动假冲突）⇒ 两项应**合并裁定** |
| U-51 | 判据不同源（登记） | **P3** | 屏上 `sources[].tripped` 取的是**未去抖原始电平**（`interlock.rs::status_sources()` → `live_active_channel()`，逐 DI 直读 GPIO），而帧内 `latched` 是**去抖后**状态机的产物（`tick_frame` 步骤 2 的 per-DI `debounce` 计数 → `StateMachine::tick`）⇒ 抖动期（`debounce` 计数未满）会出现"某源 `tripped=true` 而 `latched=false`"的**自相一致但口径不同源**的组合；EDGE-19 若按 U-50 的建议纳入 `tripped`，会把抖动当成"状态已变化"而**拒绝合法提交** | **登记**：与 U-50 **成对**（"要不要纳入 tripped"与"tripped 用哪个口径"必须一起定）。候选口径：① 契约里 `InterlockSourceStatus` 增加"去抖后"字段（改线格式）；② 只在**服务端**比对去抖后状态（不动线格式，但渲染端与服务的口径就不同名同义）；③ 维持现状（`tripped` 只作**展示**用，明确标注"未去抖、不作判据"）——**待 PM 裁定** |
| U-52 | 部署前提未登记 | **P1**（部署侧） | **无登录控制通道的成立条件此前全仓无登记**：`:9811` 的写通道（配置写 + **联锁释放 / 授权重启**）**只凭"能连上回环端口"授权**（无登录/无会话/无 RBAC；审计操作者恒为常量 `local-console`）。代码里能读到"无登录"，但**部署侧没有任何一句写下它的前提** ⇒ "端口不得被 iptables/nginx 转发、本机不得有不受信进程"这两条**实际的安全边界**只存在于设计者脑中 | **已登记（就地落在部署文档）**：`mupc/deploy/deploy.md` §**10.3**「控制通道（`:9811`）的部署前提」——逐条写明三条前提（不得转发 / 本机不得有不受信进程 / 屏所在柜体上锁）+ 为什么代码兜不住（`control_bind_addr` 校验只管"绑在哪"，管不了"谁把它转出去"）+ "将来要远程写能力必须**先补 RBAC/会话**，不得靠转发端口实现"。本表此项为**唯一技术债落点**（指向 deploy.md §10.3） |
| U-53 | 纵深防御缺一层（**已就地补上**） | **P2** | 读通道 `LoopbackHttpPublisher::serve`（`display_host.rs`）**缺**"按实际绑定结果复查回环"的第二层：控制通道 `ConsoleHost::serve` 有（`local_addr().is_loopback()`，非回环即 `Err`），读通道**没有** ⇒ 将来某条路径自行 `bind` 后直接 `serve`（绕开 `CoreConfig` 校验）时，读通道会把最新帧发到任意地址 | **已补**（评审许可"若成本极低"）：`serve` 开头按 `listener.local_addr()` 复查，非回环 ⇒ `error!` 留痕并**立即返回**（不进入 accept 循环）。**签名保持 `()`**（改为 `Result` 会波及 startup + 3 条用例；而"拒绝"在读通道上本就是"不提供数据"，fail-safe 等价）。用例 `read_channel_serve_refuses_non_loopback_listener_by_actual_bind`（非回环 ⇒ future 立刻结束；**回环正对照** ⇒ 持续运行，防"拒绝一切"式假绿）；**破坏性验证**：摘掉复查 ⇒ 红 |
| U-54 | 输入无显式上界（登记一句） | **P3** | `config_service.rs` 的 `std::fs::read_to_string(&self.path)`（保留式编辑读**整个** yaml）**无显式上界**。评审判定**可接受**（输入是操作员配置、非网络输入；且同文件已有 `bounded_io` 一族的读上界工具） | **登记一句·不改**：与 U-45 同族（"无上界"类），但风险等级不同——本项的输入面是**本地文件**（操作员可控），U-45 的输入面是**现场 yaml 的节拍值**（直接影响验收时延）。若将来 yaml 大小失控（如被日志/注释撑大），按 `bounded_io` 的既有口径补一条上界即可 |
| U-55 | **部署沙箱阻断配置写（已修）** | **P0**（部署侧） | `deploy/systemd/mupcd.service` 在 `ProtectSystem=strict` 下**未把 `/opt/mupc/config` 列入 `ReadWritePaths`**，且**显式列入 `ReadOnlyPaths`** ⇒ 配置目录对内只读。而本地屏「配置保存」的原子落盘（tmp → fsync → `.bak` → rename）目标正是 `--config` 指向的 `/opt/mupc/config/mupc_core_config.yaml` ⇒ **生产部署下每一次保存都必然失败**（`ApplyFailed`），PRD F9 / CF-01 / CF-04 / PL-1 全线不成立。设计 §12.2 与 §14 **R-11 已明确要求该行**，且设计**自己承认**"mupcd 的 unit 未逐行核对，实施前须确认"——**该确认从未执行**。⚠️ 单元测试结构性测不到（测试在 systemd 之外跑，`ProtectSystem` 是内核级挂载） | **已修**（2026-09-19）：`ReadWritePaths` 增 `/opt/mupc/config`、`ReadOnlyPaths` 移除该路径，并在 unit 内注明"文件属主仍需可写（`chown mupc:mupc`）"+ 指向 R-11 的由来；`deploy/local-display.md` 部署步骤同步补该前提。**真机验收项**：部署后必须实测一次"屏上改一个键 → 保存成功 → 装置重启后值仍在" |
| U-56 | **部署权限缺失致触摸不可用（已修）** | **P0**（部署侧） | `deploy/systemd/mupc-display.service` 的 `SupplementaryGroups=dialout video render` **缺 `input`**（设计 §12.2 明写 `dialout video input`），且 `ExecStart` **未传 `--touch-device`**，仓库内**无 udev 规则文件**（设计 §12.2 给了规则内容但从未落成资产）。三者叠加 ⇒ `/dev/input/event*`（`root:input 0660`）打不开，进程仍启动但**退化为只读展示 + 「触摸不可用」角标** ⇒ **PRD §0 B1「屏有触摸」与 v2.0 相对 v1.0 的全部增量（F9/F10/F15/F17/F18/F19 的触摸入口）在真机上不可达** | **已修**（2026-09-19）：unit 补 `input` 组 + `--touch-device /dev/mupc-touch`（并注明 `input` 不可省、`dialout` 本进程并不使用）；**新增资产 `deploy/udev/99-mupc-touch.rules`**（规则默认注释——`ATTRS{}` 条件必须按真机 `idVendor`/`idProduct` 填写，照抄过宽匹配比不加更糟；文件内含四步操作指引）；`deploy/local-display.md` 部署步骤与权限章节同步。**真机验收项**：`ls -l /dev/mupc-touch` 指向 `eventN`，且 `sudo -u mupc … --touch-device /dev/mupc-touch` 能点动 |
| U-57 | **CI 从不构建 HMI（已修）** | **P0**（构建侧） | `.github/workflows/build-ubuntu.yml` 的 build job **只 `cross build -p mupc-core-bin`**，**从不编译 `local-display`/`lvgl-sys`** ⇒ 设计 R-20/R-23 列为「**实施第一步门禁**」的 `cargo build --target aarch64-unknown-linux-gnu -p lvgl-sys` **在流水线上根本不存在**；同时 lint/test job 覆盖全 workspace 但 checkout **未带 `submodules: true`**（`mupc/vendor/lvgl` 是 mode 160000 的 submodule，pin `85aa60d`=v9.5.0 ⇒ CI 上为空目录）、未装 libclang、未跑 `gen_fonts.sh` ⇒ 该两 job **结构性无法通过**。**后果**：全部 `#[cfg(target_os="linux")]` 代码（`FbCanvas` 的 fb0 mmap/像素格式探测、`FdPoller`、evdev 触摸、信号处理）**一次都没过编译器**，且 R-05/R-24/R-25 的全部真机指标至今为空。<br>**⚠️ 另经 2026-09-19 复核得到的一条旁证**：CI 的 lint job 用 `cargo clippy --workspace -- -D warnings`，而**按同一口径实测本工作区有 93 条既有告警**（分布在 `ai-engine/rknn_runtime_sys.rs`、`pareto_optimizer.rs`、`strategy-engine/ai_integration.rs`、`rs485-plugin/device.rs`、`mupc-core-bin/startup.rs` 等**与本模块无关**的既有代码中）⇒ **该 gate 在本次改动之前就不可能通过**，进一步说明 CI 事实上从未把关过本仓库（而非"只是没覆盖 HMI"）。该 93 条属**仓库级 CI 卫生问题**、**非 12 号模块债务**，此处只作旁证登记，处置由 Linux 侧统一裁定（清账 or 调整 gate 口径） | **已修**（2026-09-19）：见 workflow 的 `hmi` job 与各 job 的 `submodules: recursive` + libclang + `gen_fonts.sh` 前置。⚠️ **验证状态：未验证**——本机（Windows）**无法**验证 workflow 语义，须在 Linux/CI 上首次跑通后转硬门禁。**首跑预期会红**（Linux 版 `local-display` 首次真编），这正是要暴露的东西 |
| U-58 | 内存安全（**登记·机理未定**） | **P2** | `CallbackHandle::detach`（`local-display/src/lvgl/event.rs`）在**宿主删除后**被调用时会对同一 `Ctx` 指针 `Box::from_raw` 两次 ⇒ double free。**本轮实测到一个更硬的信号**：给 `Ctx` 加一个"回收令牌"字段、并在 `reclaim` 入口写 `(*p).live.set(false)` 后，`lvgl_core_bridge_chain` 出现**间歇 `STATUS_ACCESS_VIOLATION`**（定向 20 次出现 2 次；撤掉该写入 20/20 稳定；"只保留该写入、`event.rs` 取 HEAD"的对照组 0/20）——说明**存在"`reclaim` 收到已释放 `p`"的路径**：`reclaim` 的**延迟分支原本根本不读 `p`**（只把指针推进 `pending`），故这份重复回收请求被**推迟且静默**；一旦在入口写 `p`，就变成"当场写已释放内存" ⇒ 堆损坏 | **本轮只留下"禁令 + 记录"，两种幂等判据均被否掉、未采纳**：<br>① **`reclaim` 入口写令牌** —— 实测引入 AV（如上），**已撤**；<br>② **用 `remove_event_cb` 的返回值判 `removed == 0` 则提前返回** —— 独立评审判定它在**可达路径上是纯防御性的**（单回调窗口内的双回收早已被 `reclaim` 的 `pending.contains` 去重覆盖），却**换来一条潜在静默泄漏**（若某条非 DELETE 的摘除路径清掉 dsc 而 DELETE 从未派发，提前返回会使该 `Ctx` **再无回收点**）；另评审判定它**也挡不住** `lv_obj_tree.c:677-680` 的 `INVALID` 分支（DELETE 已回收但列表未清 ⇒ `removed > 0` ⇒ 仍二次 `from_raw`）。⇒ **已回退为 A1 原实现**。<br>**保留的两项资产**：（a）`reclaim` 函数头的**禁令注释**——"**本函数不得在任何分支读/写 `(*p)` 内容（`Box::from_raw` 除外）**"并附上述实测；（b）**本登记**。<br>⚠️ **残留未定性（须 Linux 环境收口）**：那条"`reclaim` 收到已释放 `p`"的路径**具体触发序列尚未定位**（本机无 ASAN/valgrind 级工具）；`INVALID` 分支现值不可达（依赖 `LV_EVENT_BUBBLE`，而 `mod.rs` 明确本批无 `ui/**` 消费者），**一旦该 flag 有了消费者即变为可达 ⇒ 必须与 ASAN 定位一起收口**<br>**Linux 侧收口（2026-09-19，交接清单 L-3）**：① 那条「`reclaim` 收到已释放 `p`」的路径**本轮未复现** —— `reclaim`/`trampoline` 都是 Rust 代码，ASAN 会直接命中 `Box<Ctx>` 的 double-free/UAF，而实测报的是 C 侧 SEGV（另一处缺陷，见 ②）；② **ASAN 首跑即抓到一条确凿 UAF**：`Indev::drop` → `lv_indev_delete()` 时，LVGL v9.5.0 的**惯性抛掷动画仍在全局动画表里**（其 `var` 就是 indev 指针，而 `lv_indev_delete` 只清 read_timer/事件表/链表项，**不清理动画**）⇒ 下一次 `lv_anim_refr_now` 对已释放 indev 解引用（`indev_scroll_throw_anim_cb` → `lv_indev_scroll_throw_handler` → `lv_obj_has_flag(野指针)` → SEGV）。**证据链**：原始代码 ASAN 3 跑 2 崩；对照实验（`mem::forget(indev)` 不释放）**5/5 零崩溃**；修复（allowlist 新增 `lv_anim_delete` + `Indev::drop` 先删动画再删 indev，与对象侧 `lv_obj_destruct` → `lv_anim_delete(obj, NULL)` 对称）后 **20/20 零崩溃**，且常规测试 369 passed / 0 failed。**推论（假说，如实标注）**：Windows 上「加回收令牌 ⇒ 间歇 AV」很可能是**堆布局扰动**把这条 indev UAF 提前暴露，而非 reclaim 真二次释放 ⇒ **维持不引入「回收幂等闸」**（当时两种判据被否的理由依旧成立）|
| U-59 | 真源缺口（登记） | **P2** | **PRD F6 的「IEC 104 连接状态」恒为「未知」**：设计 §4.1 #1 明确要求 `gateway` crate **新增 `Iec104Server::link_state() -> LinkState`**（`ConnectionState` 的 5 个变体 `Disconnected/Connecting/WaitingStartDt/Connected/Stopped` 已就绪，映射干净），而全仓 **`grep "fn link_state"` 零命中** ⇒ `display_host` 的 `device.iec104` **硬为 `LinkState::Unknown`**（`display_host.rs:245`，代码注释已如实登记）。PRD F6 表列的四个态（已连接/连接中/断开/未配置）**本功能域不可达**，ST-12/ST-13 中该行不成立 | **登记·本轮不做**（PM 裁定：属**功能新增**而非缺陷修复，须走「实现 → 规格评审 → 代码质量评审」，且要跨 3 文件接线：gateway 新增方法 + `display_host` 注入点 + `startup.rs` 传参；`display_host` 当前**完全不持有 gateway 句柄**）⇒ 拆独立单元，**Linux 环境下实施**。F6.5 允许显示「未知」，故**不构成假值**（详见本轮审查报告 3a-A1）<br>**已实现（2026-09-19，交接清单 L-5）**：`gateway/src/iec104/server.rs` 新增对外聚合枚举 `LinkState{NotConfigured,Disconnected,Connecting,Connected}` 与 `Iec104Server::link_state()`（判据与设计 §4.1 #1 逐字一致：未启动→NotConfigured；已启动无连接→Disconnected；有连接但均未 `Connected`→Connecting；任一连 `Connected`→Connected；新增 `started: AtomicBool` 区分「未启动」与「无连接」）；`display_host` 增 `Option<Arc<Iec104Server>>` 注入点 + 1:1 映射 `map_iec104_link_state`（未装配 ⇒「未配置」）；`startup.rs` 把服务器实例**提前构造**（步骤 9 只做 `start()`）并接线。**测试**：gateway 3 例（未启动/已启动无连接/连接表聚合）+ display_host 2 例（未装配=未配置、映射逐变体自证）全部通过 |
| U-60 | 语义收窄（登记·待 PM 确认） | **P3** | P3 日志页顶部的「实时日志已连接/已断开」通道条，其判据是**控制通道**可达性（`control_route.rs:303`），**不反映 1 Hz 读帧链路**。⇒ 「读链路已断、控制链路仍活」时（EDGE-20 场景），屏上会**同时**出现「实时日志已连接」与**整屏降级遮罩**，语义上易被现场误读为"日志正常"。设计 §6.3 原文即按控制通道判定（"断开由控制通道请求失败判定"），故属**设计如此**、非实现偏离 | **登记·待 PM 确认**：若认为该并存态会误导现场，候选改法 = 通道条改按**读帧链路**判定，或文案改为「日志服务已连接」（点明它指的是日志服务而非数据通道）。本轮**不改**（涉及上屏文案，须走 §3.6 收口）|
| U-61 | FFI 边界两处既存缺口（登记） | **P2** | 由 2026-09-19 项目级审查的独立代码质量评审发现，**均非本轮引入、此前无人登记**：<br>① **用户闭包的 `Drop` panic 未被 `catch_unwind` 覆盖**：`event.rs` 的 `catch_unwind` 只包住闭包**调用**（`f(Event{..})`），而 `Ctx`（含用户捕获值）的**析构**发生在 `ReentryGuard::drop` 的 drain 与 `reclaim` 的立即分支里，**两者都在 `catch_unwind` 之外** ⇒ 若捕获值的 `Drop` panic，panic 从 C 蹦床逃进 C 帧 = **UB**（设计 §5.2 不变量 5 的同一理由）。<br>② **DELETE 分支未把 `p` 登记进 `active`**：只有非 DELETE 分支（`event.rs` 的 `active.push(p)`）登记；⇒ 若用户的 DELETE 回调内对**同一宿主**同步派发事件（如 `scroll_to_y`），嵌套蹦床会在 `f` 已被 `take` 前再取 `&mut *p`，与第 246 行的可变借用形成别名。实际危害**有限**：`f.take()` 后嵌套调用在 `f == None` 处提前 return，且模块文档所称"同闭包重入跳过"的保护在 DELETE 侧并不成立 | **登记**（如实标注"既存 + 现值不可达/危害有限"，**不夸大**）。建议收口方式：① 把 `Ctx` 的析构也纳入 `catch_unwind`（或把 `Box::from_raw` 的 drop 包进 `catch_unwind`）—— 与本轮 `diag` 的处置同一类；② DELETE 分支也推 `active`（或明确写清"DELETE 期间不支持对同宿主重入派发"的契约）。**须与本表 U-58 的 ASAN 定位同批做**（三者都在同一段延迟回收逻辑上）<br>**已收口（2026-09-19，交接清单 L-3）**：① `ReentryGuard::drop` 的 drain 循环与 `reclaim` 的立即分支现在都用 `catch_unwind` 包住 `Box::from_raw` 的析构（捕获值 `Drop` panic 只记录、不跨 FFI 展开）；② DELETE 分支派发前也把 `p` 推入 `active`（与闭包调用同等登记，嵌套调用命中 `is_executing` 直接返回），消除 `f` 被 `take` 后嵌套蹦床再取 `&mut *p` 的别名。**验证**：常规测试 369 passed / 0 failed；ASAN 20 次运行零报告 |
| U-62 | 平台身份**从未被校验**（登记） | **P2** | 由 2026-09-20 评审发现（既存，非本轮引入）：① `ota-update/src/verifier.rs::verify_platform_compatibility` 的头部布局注释写了"魔数(4) + 版本(4) + **平台标识(4)** + 保留(20)"，但实现**只读** 魔数 与 版本(4..8)，**从不解析平台标识(8..12)**；`platform_version` 形参仅用于在 `PLATFORM_MIN_VERSION` 里取最低版本。② OTA 包元数据的 `target_platform`（`ota-update/src/firmware/mupc_package.rs:48`）全仓**只被赋值、从不校验**（构造处均为空串）。⇒ 为其他 Rockchip 平台构建、版本号达标的模型/包会被接受，直到设备上 `rknn_init` 才失败 | **登记**（不臆造校验）：真正的平台校验需要先有 **.rknn 头部规格**（设计文档未定义该布局，现有注释是本模块自造的约定）或改为校验 OTA 包的 `target_platform`（设计 §`target_platform` 行给了取值口径 `rk3588-openeuler`，但设备侧身份来源需一并定：`/proc/device-tree/model` 与包内字符串的**匹配口径**必须显式定义，否则严格比较会误拒真实包）。**已把"平台不被校验"钉成用例**（`verifier.rs::test_verify_platform_compatibility_valid` 故意写入不符的平台标识并断言仍通过），防后来者误以为它已生效 |
| U-63 | 回滚后**未通知策略引擎**（登记） | **P2** | 设计 §2.9.2 回滚流程第 5 步要求"重启策略引擎加载旧模型"，而：① `mupcd` 用 `OtaManagerImpl::new`（无回调）构造 OTA 管理器；② 策略引擎侧**没有**模型重载/通知入口（`AiIntegrator` 只有 `set_model_manager`）⇒ 自动回滚完成后，策略引擎会继续持有被否决的模型直到进程重启。**管理器侧的转发缺陷已修**（2026-09-20 评审：`with_callbacks` 早先只把回调给了 `ModelApplicator`、给 `RollbackManager` 传 `None`；现同份转发给两者，并有回归用例 + 变异验证钉住） | **半修**：转发路径已可用且被用例覆盖；**剩余的接线阻塞点**是在策略引擎补一个"模型已变更/请重载"入口（属功能新增，须走实现→规格评审→质量评审），随后把 `startup.rs` 的 `OtaManagerImpl::new` 换成 `with_callbacks(..., Some(cb))`。`startup.rs` 该处已加注释说明现状与改法 |

> ⚠️ **统计口径提示（本轮如实标注）**：本表第 7 节的既存行**未计入** U-40 / U-41 / U-42
> （已核对：单元 K 行只列了 4 条、单元 L 的 U-42 无对应行）⇒ 该节的「总计」**本就偏低 ≥3**。
> 本单元**不改既存行的计数口径**（属他人账目），只把 U-43 ~ U-54 **一行计 12 条**并入。

> ⚠️ **统计口径提示 2（2026-09-19 项目级审查修复批）**：新增 **U-55 ~ U-61 共 7 条**，同样
> **不动第 7 节的既存计数**。其中 **U-55 / U-56 / U-57 的等级为「部署/构建侧 P0」**——判定依据是
> 「本地测试结构性看不到、而生产形态下功能不可用」，**不是**代码质量等级；习惯只按代码面读本表
> 等级列的人请注意这一差别（这三条若按代码面看都"没有 bug"）。
> **U-58 / U-61 是同一段"延迟回收"逻辑上的三条内存安全事项**（含 U-58 的未定机理），
> **应同批在 Linux 用 ASAN/valgrind 定位**，不要分散处理。

---

### 6.12 端到端数据流审查登记（外设采集 → 本地显示 → 就地保存 → 云端上传，2026-09-23）

> 来源：`docs/superpowers/reports/端到端数据流审查-外设采集到显示存储上云-2026-09-23.md`
> （只读审查；含 P0 修复批次 `dcc0012` / `b0e7083` / `5998f0d` 的双门禁核验与遗留清单）。
> **审查方法**：对每一个"落库 / 上送 / 上屏"动作**反向 grep「谁写它、谁调它」**，不以"模块存在 / 编译
> 通过 / 测试全绿"作为通过依据（即 §6.5 **P-01「闭环验证」**口径）。已随 P0 批次就地修复的两项
> （#1 定时 + 退出 flush、#2 读路径 CRC/从站号校验）**不在本表**；本表登记**本轮不修**的残余与新发现。
>
> ⚠️ **同批已就地订正两处台账失真（在 §5，不新增 U 号）**：① **消息总线 MQTT**（原文"✅ MQTT 已完成"
> → "⚠️ 未接通"）—— 事实依据即本表 **U-71**；② **故障录波**（原文"4 个方法为 stub" → "查询/导出已修复、
> 上报链路受 U-25 阻塞、生产路径零触发"）—— 两处订正均按 §6.11 U-37 的「就地改口径 + 加注缘由」惯例。

| # | 类别 | 严重程度 | 问题 | 依据/处置 |
|---|------|----------|------|-----------|
| U-64 | 退出路径丢点窗口（评审警告 W3） | **P1** | `WriteBuffer::flush()` 的形态是**先 `drain` 再 `await commit`**（`mupc/crates/storage/src/services.rs:279-287`），而 `StartupContext::shutdown()`（`mupc/crates/mupc-core-bin/src/startup.rs:60-66`）是**先 `abort` 全部后台任务、再 flush**；`abort()` 要到该任务的**下一个 await 点**才生效 ⇒ 被 abort 的任务若恰好停在 `commit_batch` 的 await 上，**已 drain 出缓冲的这一批随 future 被静默丢弃**（事务回滚），连 `flush_batch` 的 error 分支都走不到、**无任何日志**。受影响者：本批新增的**定时 flush 任务**，以及做**容量触发**的生产者任务 | **定级为警告而非严重（有界 / 窄窗 / 非确定性，且严格优于修复前）**：间隔 5000 ms、单次 flush 数 ms ⇒ 命中概率约 **0.1%**、丢失量 **≤1 批**；修复前"不满一批的数据 100% 永久滞留内存、断电即丢"，故本窗口**严格优于修复前**，也优于"先 flush 再 abort"的反序方案（反序会漏掉 flush 之后新产的点）。**另须如实登记**：`startup.rs:59-60` 的注释"…谁先拿到谁生效、**不会漏掉已完成 push 的点**"对"并发 `buffer_telemetry` 新 push"成立，对"**已 drain 未 commit**"**不成立**——该保证过强（本轮只登记，代码注释不在本批授权范围）。**处置建议（下轮）**：改**优雅停机** —— 定时任务 `select!` 退出信号、`shutdown()` **先 join 定时任务再 flush**（生产者仍 abort） |
| U-65 | 协议校验缺口（评审新发现 S3） | **P1** | 三条读路径**只查 `func & 0x80`（异常帧）**，**未校验"响应功能码 == 请求功能码"** ⇒ 设备回**错功能码**时（例如请求 FC02 却回 FC03 帧）帧级校验（长度 + CRC + 从站号 + 异常位）**仍会通过**，随后**按错误掩码解包**（以位解包器去解寄存器帧），产出**貌似合法**的错误数据。落点：`mupc/crates/rs485-plugin/src/device.rs` 的 `validate_read_response`（`:113`，四项校验中无功能码比对）→ `parse_regs_response`（`:160`）/ `parse_bits_response`（`:218`） | **既有缺口，非 P0 批次引入**（`dcc0012` 的合同范围是"补齐 CRC + 从站号"）。Modbus 客户端惯例**应回显校验**；建议与**写路径校验**（评审轮外登记 R1：`write_single_register` 无 CRC/从站号/异常帧检查，且 `strategy-engine/src/south_command_sender.rs:206/235` 的 `send_pv_limit`/`send_load_shedding` 只看 `Ok(_)`、响应字节一字不看，**写静默面更大**）**一并列 P1**。修法：把请求帧的 `func` 传入 `validate_read_response`，不符即拒（与 U-66 可同批收口） |
| U-66 | 输入边界脆弱点（评审 S4 / 建议 W4） | **P2** | `parse_bits_response` 的 `&response[3..3 + byte_count]`（`mupc/crates/rs485-plugin/src/device.rs:241`）仍为**裸切片** | **当前不存在可达 panic**：同函数**紧邻**的长度闸（`:230-232`，保证 `len >= 3 + byte_count + 2`）兜住，且 `byte_count` 源自 `u8`（≤255）⇒ `3 + b + 2` 不可能溢出 `usize`。这与已收口的 `response[2]`（改为 `get(2)`）**性质根本不同**：后者安全性依赖**另一个函数**（`validate_read_response`）的前置，被短路/重排即 panic（评审探针 A3b 实证）。**本轮登记、下轮一行收口**：`response.get(3..3 + byte_count).ok_or_else(…)?`，与 `get(2)` 口径统一（成本极低） |
| U-67 | 需求-实现缺口（评审待裁 D2） | **P2** | `core_config.rs` **无 storage 段任何字段**（仅 intercore 心跳/重连、gateway `min_publish_interval_ms` 等）⇒ 与 `03 PRD:693`（HST-ELEC-01）「按**可配置的**存储周期（默认 1 分钟）将电气量数据持久化存储」冲突；实现为**硬编码常量**：遥测缓冲容量 **1000 条** / 间隔 **5000 ms**（`mupc/crates/mupc-core-bin/src/startup.rs:661-664` 的 `WriteBuffer::new(1000, 5000, …)`） | **只登记，本轮不新增配置项**。同批已在 `03 设计文档` §4.4.2 补**概念消歧**：「**flush 窗口**（容量 OR 时间，先到先执行）＝缓冲区**提交节拍**」≠「**存储周期**（PRD §693，默认 1 分钟）＝**采样/落库聚合周期**」——**两个量、不可相互推导**，不得合并成一个配置键。后续动作：产品裁定"存储周期是否真需可配置 + 落在 YAML 还是 DB"，再定是否把两个参数收进 `core_config.rs`。**与 U-69 强耦合**（总表落库周期就是本条的"存储周期"）⇒ **应合并裁定** |
| U-68 | 设计-实现漂移（评审待裁 D3） | **P2** | ① `03 设计文档` §4.4.2 的 API 写作 `write_telemetry` / `write_battery` / `write_alarm` / `write_event`，且形态为"tokio mpsc + **独立 Writer/Reader 连接（≤4）**"，而实现是 **`buffer_telemetry` + `Mutex<Vec>` + 同一连接池**（`mupc/crates/storage/src/services.rs:218/279`）；② §4.4.3 的**按月分区表**（`telemetry_202605`）**未落地** —— `run_migrations` 只建单张 `telemetry` 表（设计 `:1129` 已自述该 Target/落地差异，**非新发现**）；③ **"落库失败即整批丢弃"是否可接受待设计裁定** —— `flush_batch`（`services.rs:293-310`）只把丢弃条数**响亮化**，**无重试/背压**（设计对**上送**侧有背压明文 `01 设计文档:232`，落库侧**无明文**） | **登记待设计确认**（三项都要动设计文档，不在本批文档任务授权范围）。注：**flush 的数值口径**已在同批（2026-09-23）就地改齐到实现现状（1000 条 / 5000 ms，见 `03 设计文档` §4.4.2 的"口径对齐"块）⇒ **本条记的是"形态与分区"，与数值口径分开记**，勿混为一谈 |
| U-69 | 需求断裂（审查缺陷 #3） | **P1** | `03 PRD:18` 要求持久化"**周期性电气量数据**"，而 **`meter_grid`（台区总表）电气量只进 `AiIntegrator` + IEC 104 北向广播、不落库**：`startup.rs:376` 起的 `broadcast_grid_iec104` 只 `set_latest_data` + 广播；全仓 `buffer_telemetry` 仅 **2 处**调用点（`startup.rs:451` SouthSink / `:1233` 南向模拟环），**均非总表电气量** ⇒ 总表数据断电即丢、历史查询无源 | **属真实缺陷**（PRD 明文要求，实现断裂），但**修法需先回答"总表电气量以什么周期聚合落库"**——那正是 U-67 的**存储周期**概念（默认 1 分钟）⇒ **U-67 与 U-69 应合并裁定**（先定周期/配置口径，再接线落库），避免落一个与 HST-ELEC-01 对不上的硬编码周期 |
| U-70 | 需求侧消费方失效（审查缺陷 #9） | **P2** | `03 PRD:61` 把遥测历史的消费方写成 **web-api**（"data-processing → web-api ｜ 历史数据、台账、告警查询 ｜ REST API 查询接口"），而 **`web-api` crate 已整删**（§6.9）⇒ 该消费方**当前不存在**。现状唯二消费方：12 号本地屏读的是 `storage.events`（**不是 telemetry**，`display_host.rs:419-427`，设计 §4.1 #3 裁决 D11）与**尚未实现**的"上云"（U-74） | **需求侧须改判消费方**：回写 `03 PRD:61`，删/替 web-api 一行，明确 telemetry 表的真实查询方（上云点表 / 本地屏新增页 / 导出，三选一或并列）。**属需求文档动作，不在本批授权范围**，仅登记。**在改判前**，telemetry 表"只写不读"应作为**已知设计余量**陈述，不得当成"查询功能已实现" |
| U-71 | 台账失真 + 功能空转（审查缺陷 #12） | **P1** | MQTT 两条链路（北向 `emqx:8883` / 本地 `mosquitto:1883`）**全链无生产者**：`MqttBridge::publish` 的唯二实现（`mupc/crates/mqtt-bridge/src/north_client.rs:143`、`local_client.rs:147`）**无任何调用方**；`core_config.rs:307` 的 `MqttBridgeConfig` **仅 2 个 bool**（`north_enabled`/`local_enabled`，缺省双 false，`:308-313`）、两份部署 YAML **无 `mqtt_bridge` 段**；`mqtt-plugin::start()` 为**空实现**（`mupc/crates/mqtt-plugin/src/lib.rs:75`）；`message_bus` 已实例化但**零生产者零消费者**（`DataReporterImpl` 仅定义于 `data-processing/src/reporter.rs:119`，全仓**无实例化点**）。而 `01 设计文档 §4` 有**完整的两层 MQTT 设计** | **与 §5 就地订正的"消息总线"行同源**（原文"✅ MQTT 已完成"已订正为"⚠️ 未接通"，2026-09-23）。**"功能真做" vs "台账如实降级"二者择一，须 PM 裁定**：本轮已**在台账如实降级**（§5），功能接线**单列**（涉及 `core_config` 补段 + 生产者接线 + 部署 YAML 补段，属**功能新增**，须走实现 → 规格评审 → 质量评审）。⚠️ 与 U-74 一起看：**"外设上云"目前既无 MQTT 生产者、也无 IEC 104 点表**，两条通道**都**不通 |
| U-72 | 部署口径不一（审查缺陷 #13） | **P2** | `deploy/scripts/build-for-rk3588.sh:21` 的 `CARGO_FLAGS="--release -p mupc-core-bin"` **只编主控进程**，**不含 `local-display`**（真字库还需 `--features noto-font`）；构建屏程序的手工步骤只存在于 `deploy/local-display.md:38` ⇒ **一键产物可能缺屏程序**，是现场"屏不亮"的直接成因之一 | **登记**。修法（成本低）：脚本增一步 `-p local-display --release --features noto-font --target aarch64-unknown-linux-gnu`（或加 `--with-display` 开关），并把 `deploy/local-display.md` 的构建步骤**指向脚本**（单一真源）。**注意**：屏程序是**独立 systemd unit**（`mupc-display.service`），脚本改完须连同产物落点与 §6.11 U-56 的触摸/权限前提一起核 |
| U-73 | **新需求**（审查 #10，非缺陷） | **待立需求** | 12 号 PRD 的 6 页功能项为：P1 主状态页 F1 SOC / F2 PCS 状态 / F3 三相有功 / F4 三相电流 / F5 刷新 / F6 装置状态 / F7 告警列表；P2 F9 参数读写；P3 F10 日志；P4 F16–F18 联锁；P5 F19 审计；P6 F8 版本信息 —— **无一项涉及 HVAC / 储能表（`meter_batt`）数值** ⇒ 屏帧（`display-proto/src/frame.rs`）里**根本没有对应字段**，不是"采集了没显示"而是"契约里没有" | **须走需求/设计流程，不得当 bug 修**（审查报告 §五 B 类口径）。落点三处：① 12 号 PRD 新增功能项（含验收口径）；② UI 设计定页位与**槽宽**（须一并考虑 §6.10 U-42 的"真字库下带符号 P 值装不下槽宽"同类风险）；③ `frame.rs` **线格式扩展**（v2.0 契约已冻结 ⇒ 须按版本化处理，两端同步升级）。**副产品**：`hvac` 温湿度与 `hvac`/`bms` 的 DI 状态位同理（当前仅进 telemetry 表） |
| U-74 | **新需求**（审查 #11，非缺陷） | **待立需求** | `01 PRD §2.3/§2.4` 与 `01 设计文档:227-232` 只定义了"周期上送遥信/遥测"的**机制**（默认 1s、≥1Hz、背压），**未定义点表**；现有 IOA 表出自 `plans/2026-09-09-修复批次-审查P1P2-R1R2R3.md:276`（**仅 6 点，且标注"占位常量、现场追认"**）⇒ **外设（HVAC / 储能表 / 消防）上送从未被定义**。且 IEC 104 当前**无总召**（`gateway/src/iec104/connection.rs:242-274` 只解析遥控/调节）、**连接前无快照**（`server.rs:298-303` 只发已连接的 `tx`）⇒ 主站没有"需要时取数"的手段 | **须走需求/设计流程**。落点：① `01 PRD`/`01 设计文档` 补**上送点表**（IOA 分配、量纲、上送周期、外设是否入表）；② 明确**总召（TI=100 / C_IC_NA_1）**策略与"连接建立后的补发/快照"口径。⚠️ **与 U-69 的关系**：U-69 解决"总表数据从哪来（落库）"，本条解决"总表以外的数据怎么出装置"；**与 U-71 的关系**：MQTT 与 IEC 104 是**两条**上云通道，**择一还是并行须先定**（当前两条都不通） |

> ⚠️ **统计口径提示（本批如实标注）**：新增 **U-64 ~ U-74 共 11 条**，其中 **U-73 / U-74 是"新需求"**
> （PRD 从未要求 ⇒ **不得当缺陷修**，处置路径与其余 9 条**不同**，登记在此只为"不丢"）。
> **两条台账失真已在 §5 就地订正**（消息总线 MQTT / 故障录波）—— 订正动作**不新增 U 号**
> （属既有行的口径修正，与 U-37 的处置惯例一致）。本批**不改 §7 既存行的计数口径**（属他人账目），
> 只把本批 11 行并入；**若按"技术债"严格口径统计，应从总数中扣除 U-73/U-74（实际技术债 9 条）**。

---

### 6.13 PCS 迁入南向的未结项登记（2026-09-26，T1–T12）

> **来源**：PCS（两级式 PCS = 实时控制模块）的通信与控制**整体迁入 `mupc-southd`**
> （02 号设计 **§13** / **ADR-014**）的落地批次（**T1–T12**，逐 Task 提交与评审轮次见 02 号设计
> **§13.13**）。**本表只登记"未结项"**（已闭环的不在此列），体例同 §6.11 / §6.12。
>
> ⚠️ **门禁口径**：02 号设计 §13 **本次未获门禁标记**（`[DESIGN_APPROVED]` 待**独立评审**）；
> 其 §13.13 的"已实现"**只指代码落地**。本节的 §8.7 追加项（**M-10**）是其真机侧落点。

| # | 类别 | 严重程度 | 问题 | 依据/处置 |
|---|------|----------|------|-----------|
| U-75 | 合规缺口（写审计） | **P1** | **PCS 写审计事件未落地**：02 号设计 §13.5.3 明列"**须新增的落地项**"—— 每次写序列投一条 `events`（调用方 token / 时刻 / 寄存器+值 / **回读值**），复用既有 `storage.events` + `AlertFeed`。而 `PcsHandle` 的 **4 条写入口**（`mupc/crates/mupc-southd/src/pcs/mod.rs` 的 `stop`（`:186`）/ `restore_interlock_latched`（`:216`）/ `send_dual_param`（`:250`）/ `send_tai_command`（`:281`））**目前无任何"谁/何时/写了什么/回读值"审计事件** | **登记 · 未落地**。回读**本身**已实现（Task 1 的写响应回显校验：从站地址 + 功能码 + 回显值，`rs485-plugin/src/device.rs:824` 的 `write_single_register_from`）⇒ 缺的是**审计事件的落库/告警面**。属**功能新增**，须走「实现 → 规格评审 → 质量评审」。**PRD N-4（v1.14）已如实登记该待办** |
| U-76 | 架构存量（登记，非缺陷） | P2 | **`intercore` TCP 通道保留但生产路径无消费者**：T10 的 6 个注入点已全部改持 `Arc<PcsHandle>`，`IntercoreClient` 装配**保留**（`mupc/crates/intercore/src/lib.rs` 模块文档已如实登记）但**无任何生产调用方**；且**客户端现为"只发不收"** —— 删 PCS 面时其接收能力（原 `TcpTransport` 的回读接收循环）随之消失，**接收原语仅存于 `IntercoreServer` 服务端角色** | **登记为"演进起点"，不是遗漏**（ADR-014 的"供后续演进"）。**但"供后续演进"须先新增客户端接收原语**，否则该通道只能单向下发。落点 = 02 号设计 **Δ-23**；实施计划 §待用户确认 P-2 的裁定。**删/留由产品裁定**（删除会波及 `sim-bridge` 对接） |
| U-77 | 覆盖形式局限（如实登记） | P2 | **fail-fast（`io.enabled && !south_pcs.enabled ⇒ Err`）的自动化覆盖是"源文本静态断言"，不是运行期断言**：`initialize_all` 需 DB / 串口 / sysfs 全套真环境，本机单测起不来 ⇒ 锚点是"生产段源码里那句 `ok_or_else` 还在"（`mupc/crates/mupc-core-bin/src/startup.rs` 的 `task11_interlock_fail_fast_and_pcs_uplink_wiring_are_pinned`，删除该 `ok_or_else` 即红），**它不证运行期返回值** | **如实登记其局限**（与 `ota_manager_is_still_constructed_and_registered` 同款手法与同一局限）。运行期真值须真环境 ⇒ **并入 §8.7 的 M-10**。⚠️ **不得**把"静态断言绿"读成"fail-fast 已实测" |
| U-78 | 可观测性不对称 | P2 | **`south_pcs.enabled=false` 时两条下行链路的降级口径不对称**：IEC104 `p_set` 路径**如实回 `success:false` + 告警**（`mupc/crates/mupc-core-bin/src/startup.rs` 的 `StrategyCommandHandler`）；而 AI 侧的双参数 / 台区储能分相下发是**静默 no-op（仅 `tracing::debug!`，`mupc/crates/strategy-engine/src/ai_integration.rs:687`）** | **登记备裁**（02 号设计 **Δ-24**）。两者都**不越权写**（安全），但**静默臂在"部署忘开 `south_pcs.enabled`"时无告警**。处置建议（本轮不改）：AI 侧 no-op 至少升 `warn!` 或投一条事件 |
| U-79 | 真机验收项 | **P1** | **停机写 `500=0` 路径的真机行为尚无真实覆盖**：`Rs485Device::write_single_register`（旧 per-device 写法，`mupc/crates/rs485-plugin/src/device.rs:814`）在**全仓零调用点**；新链走 `write_single_register_from`（`StationBus::write_single` → `Rs485PortBus`）。⇒ Task 1 的四项回显校验（从站号 / 功能码 / 回显值 / 异常码）**当前只有单测在守**，真机行为要等**装配后**才有真实覆盖 | **在 §8.7 挂显式真机验收项（M-10）**：真机下核对停机写 500=0 的回显、错帧/异常码拒否、以及联锁停机的端到端时延。**不得**以单测绿代替真机结论 |
| U-80 | 待裁定（清理项） | P3 | **`IntercoreClient::remote_addr()` 零调用方**（`mupc/crates/intercore/src/tcp_server.rs:1004`）：T11 收口把该访问器"不再撒谎"（改为如实返回构造时的 `remote_addr`），但**全仓无任何 `.remote_addr()` 调用方** | **待裁定**：**删字段+访问器**（减面）还是**保留**（`IntercoreClient` 的对外完整性 + 未来接收原语可能需要）。实施计划明写保留 ⇒ 本条只登记**去留未定**，不擅自删 |
| U-81 | 实现状态登记（非债务） | — | **Task 10（装配换型）的落地形态**：7 个接线点（`station_shell` / `mqtt_station_roles` / 上云点表 / `south_pcs_port_params` / 策略引擎 / 联锁 `InterlockPort` / display）+ 上云 4 条要求 + fail-fast。其中 **"上云签名显式收 PCS 段"（`build_uplink_points(&SouthStationsConfig, Option<&SouthPcsConfig>)`）是跨 crate 契约变更** | **登记为事实**（便于后来者定位"为什么签名是两参"）：该签名扩参是**编译器强制每个调用点表态**的手段（防 72 个 PCS IOA **静默**从 IEC104/MQTT 点表消失，02 号设计 **Δ-19**）。正确性由 `mupc-core-bin/src/startup.rs` 的 `task10_station_roles_wiring_carries_pcs_role_into_publish_plan` + `southd` 侧用例共守 |

> ⚠️ **统计口径**：本节新增 **U-75 ~ U-81 共 7 条**，其中 **U-81 不属技术债**（是"实现状态登记"，
> 登记在此只为不丢），**U-76 / U-80 属"架构存量 / 待裁定"而非缺陷**。若按"技术债"严格口径，
> 本轮实际新增债务为 **5 条**（U-75 / U-77 / U-78 / U-79，及 U-76 的单向性局限部分）。
> **P0 级 0 条**（无人身/设备安全风险；U-75 / U-79 定 **P1** 因为都落在安全链的**可审计性**上）。

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
| 跨模块缺陷 (2026-09-16) | 1 | 0 | 1 | 🟡 待排期（U-29） |
| G-2 整改登记 (2026-09-16) | 6 | 0 | 6 | 🟡 待排期/PM 裁定（U-30 ~ U-35） |
| 单元 K 整改登记 (2026-09-18) | 4 | 1 | 3 | ✅ 1 项已订正（U-37）/ 🟡 3 项待排期/后续单元（U-36、U-38 残余、U-39）——计数口径：按条目**本轮是否闭环**计，已就地订正的计入「已修复」 |
| 独立评审 A-1/A-2 处置 + 建议 3–10 登记 (2026-09-18) | 12 | 4 | 8 | ✅ 4 项已就地闭环（U-43 安全 TOCTOU / U-44 契约下界 / U-47 501 文案 / U-53 读通道第二层）/ 🟡 8 项待排期或待 PM 裁定（U-45、U-46、U-48 ~ U-52、U-54）——计数口径：按条目**本轮是否闭环**计。**⚠️ 补记（B3-2d，2026-09-18）**：其中 **U-45 已于后续单元 B3-2d 关闭**（PM 裁定 `publish_ms` 上界 = 4000，见该行）；本行 4/8 的计数**不改**（口径是"**本批**是否闭环"，U-45 由后续单元关闭，改写本行即篡改当时账目） |
| 端到端数据流审查登记 (2026-09-23) | 11 | 0 | 11 | 🟡 待排期 / 待 PM 裁定（U-64 ~ U-74）。**其中 2 项（U-73 / U-74）是"新需求"**（PRD 从未要求 ⇒ 须走需求/设计流程，不得当缺陷修）—— 严格口径下本行技术债实为 9 条。**另订正 §5 两处台账失真**（消息总线 MQTT / 故障录波），订正不新增 U 号 —— 计数口径：按条目"本轮是否闭环"计，本批无一闭环 |
| **总计** | **84** | **40** | **44** | ⚠️ 既存行未计入 U-40/U-41/U-42（见 §6.11 末的统计口径提示），实际偏低 ≥3；本批并入 U-64 ~ U-74 共 11 条（**含 2 项新需求**，见上一行；扣除后技术债实为 82 条） |

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


### 8.6 Linux 环境交接清单（12 号本地显示终端，2026-09-19）

> **⚠️ 现状标注（2026-09-26 复核）**：本节的 **L-1 / L-2 / L-3 / L-5 已闭合**（证据见 **§8.7** 开头）；**L-4 / L-6 / L-7 仍未闭合**，已**结转至 §8.7 的 M-8**。下文保留原文，供追溯当时的判定依据。

> **为什么单列一节**：本批（`efd1277` 部署+CI / `81c81c0` 显示终端安全面 / `24e55c6` 文档）**在 Windows 开发机上无法完成或验证**的工作共 6 类。它们共同的特征是「**本地测试结构性看不到**」——`ProtectSystem` 是内核级挂载、`/dev/input` 是设备权限、交叉编译要工具链。**按序执行**：**L-1 不过，不要跳到 L-4**（真机验收依赖交叉产物）。

**L-1 · 首次跑通 CI 的 `hmi` job（P0，最高优先；关联 U-57 · 设计 R-20 / R-23 / §12.1）**

- **为什么**：设计把 `cargo build -p lvgl-sys --target aarch64-unknown-linux-gnu` 列为「**实施第一步门禁**」，而此前 CI 的 build job **只编 `mupc-core-bin`**，从不编译 HMI ⇒ 全部 `#[cfg(target_os="linux")]` 代码（`FbCanvas` 的 fb0 映射与像素格式探测 / `FdPoller` / evdev / 信号处理）**一次都没过编译器**。本批新增了 `hmi` job，但其语义（含 libclang / 字库源 / sysroot 的处理）**在撰写时未经验证**（撰写环境为 Windows）。
- **做法**（二选一）：
  1. 推一次触发 workflow，看 job 日志；或
  2. 在 Linux 构建机上按 job 步骤**手跑**（更快定位）：
     ```bash
     git submodule update --init --recursive
     sudo apt install -y gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libclang-dev
     rustup target add aarch64-unknown-linux-gnu
     export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
     export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
     cd mupc
     cargo build -p lvgl-sys --target aarch64-unknown-linux-gnu      # ← 门禁：一条命令暴露交叉 bindgen 的全部问题
     cargo build -p local-display --release --features noto-font --target aarch64-unknown-linux-gnu
     ```
     前置另需：node（`npx lv_font_conv`）、python3、下载 `NotoSansSC-Regular.otf`（`.otf` 与 `.c` 均不入库）。
- **通过判据**：产出 `target/aarch64-unknown-linux-gnu/release/mupc-local-display`，`file` 报 ARM aarch64。
- **预计会红** —— 设计 §12.1 已给两个候选处置（③ 交叉 bindgen 的 sysroot：`-isystem /usr/aarch64-linux-gnu/include`，或 64 位宿主→64 位目标时**不传 `--target`**），逐条试。
- **跑绿后**：把 `hmi` job 转**硬门禁**；并顺带观察 lint/test job（它们现已真编 Linux 版 `local-display`/`lvgl-sys`）。

**L-2 · `gen_fonts.sh` 执行位根治（小；关联 U-57）**

- `git update-index --chmod=+x crates/local-display/fonts/gen_fonts.sh`（当前 git 模式位是 `100644`；CI 暂用 `bash ./gen_fonts.sh` 规避）。
- **通过判据**：`git ls-files -s crates/local-display/fonts/gen_fonts.sh` 显示 `100755`。

**L-3 · ASAN / valgrind 定位 U-58 的未定路径，并收 U-61（P2；关联 U-58 / U-61 与 `lvgl/event.rs` 的 `reclaim` 禁令注释）**

- **为什么**：U-58 只拿到「**存在一条"`reclaim` 收到已释放 `p`"的路径**」的**存在性证据**（加"回收令牌"必现间歇 AV、不加则静默），**触发序列未定位**；U-61 的两处 FFI 缺口在同一段延迟回收逻辑上。
- **做法**：`RUSTFLAGS="-Zsanitizer=address" cargo +nightly test -p local-display --lib lvgl_core_bridge_chain`（或 valgrind 跑该测试二进制），必要时放大迭代次数——该缺陷是**间歇**的（本机定向 20 次出现 2 次）。
- **通过判据**：给出具体调用序列（或证明其不可达）。**据结论再决定**是否重新引入"回收幂等闸"——本批两次尝试都被否（令牌版引入 AV；返回值版经评审判定纯防御却带来潜在静默泄漏），**那次是没证据，这次要有**。

**L-4 · 真机验收两条 P0 的「实际效果」（P0；关联 U-55 / U-56 · 设计 §12.2 / R-11）**

- **L-4a 配置写入**：部署后**屏上改一个键 → 保存成功 → 重启 `mupcd` 后值仍在**。前置：`chown mupc:mupc /opt/mupc/config/*.yaml`；确认 unit 的 `ReadWritePaths` 含 `/opt/mupc/config`（本批已改）。
- **L-4b 触摸**：`ls -l /dev/mupc-touch` 指向 `eventN`（**须先按真机 `idVendor`/`idProduct` 填写并取消注释 udev 规则**）；`id mupc` 含 `input`；`sudo -u mupc /opt/mupc/bin/mupc-local-display --backend fbdev --touch-device /dev/mupc-touch --width 1024 --height 768` 能点动；页眉**无**「触摸不可用」角标。
- **判据**：这两条是 v2.0 的**核心可用性**——L-4a 不成立则 PRD F9 全线不成立；L-4b 不成立则 v2.0 相对 v1.0 的交互增量全部不可达。

**L-5 · U-59 `gateway::Iec104Server::link_state()`（功能新增，独立单元；关联 U-59 · 设计 §4.1 #1）**

- **为什么**：设计 §4.1 #1 明确要求 `gateway` crate 新增该方法（`ConnectionState` 的 5 个变体已就绪、映射干净），而**全仓零实现** ⇒ `display_host` 的 `device.iec104` 硬为 `Unknown`，PRD F6 的「IEC 104 连接状态」四态本功能域不可达。
- **做法**：跨 3 文件接线（gateway 新增方法 + `display_host` 注入点 + `startup.rs` 传参；`display_host` 当前**完全不持有 gateway 句柄**），须走「实现 → 规格评审 → 代码质量评审」。
- **不做也可**（替代路径）：回写 PRD F6 表并登记降级——F6.5 已允许显「未知」，故**不构成假值**。

**L-6 · 仓库级 CI 卫生（顺带；非 12 号模块债务，关联 U-57 的旁证）**

- CI 的 lint job 用 `cargo clippy --workspace -- -D warnings`，而按**同口径**实测本工作区有 **93 条既有告警**（分布在 `ai-engine` / `strategy-engine` / `rs485-plugin` / `mupc-core-bin/startup.rs` 等**与本模块无关**的代码中）⇒ **该 gate 在本次改动之前就不可能通过**。需裁定：清账，还是调整 gate 口径（如分 crate 白名单）。

**L-7 · 本批未动的既有真机硬门禁（提醒，勿以为已随本批关闭）**

- **字体相关**：U-42（真字库下 P1 页带符号 P 值装不下槽宽 ⇒ 数据被截断展示）及其关联的「**字体豆腐块 23 码位**」批——**本批未动**。
- **既有真机项**：设计 §14 的 R-03（`/dev/fb0` 映射与像素格式）、R-04（触摸协议与校准）、R-05（时延与资源实测）、R-22（滚动 ≥30 fps）**全部仍待真机**。

#### 8.6.1 执行结果（2026-09-19，Linux 构建机首跑）

> 环境：Ubuntu（内核 6.8）/ x86_64 宿主 + `aarch64-linux-gnu` 交叉工具链；`mupc/vendor/lvgl` = v9.5.0（`85aa60d`）。逐项对应上面的 L-1 ~ L-7。

**L-1 · ✅ 门禁已跑绿（首跑确如预期红了）**

- `cargo build -p lvgl-sys --target aarch64-unknown-linux-gnu` → **通过**（1m28s）。**无需 `libclang-dev`**：`LIBCLANG_PATH` 直指已装的 `/usr/lib/llvm-18/lib/libclang.so.1` 即可（CI 的 apt 步骤可保留，只是非必需）。
- `cargo build -p local-display --release --features noto-font --target aarch64-unknown-linux-gnu`：**首跑 27 错**，根因两类，**均为「该代码从未在 Linux 上编译过」**：
  1. **26 处类型错**：bindgen 在 Linux 上把 `lv_event_code_t` 生成为 `c_uint`(u32)、Windows 上为 `i32`，而 `event.rs` 把 `EventCode` 内层类型写死 `i32`。修法：内层改用 FFI 别名 `sys::lv_event_code_t`（其文档本就自称「C 侧无损镜像」），`Ctx.filter` 同步，`obj.rs` 的传参随之对齐。
  2. **1 处名字解析错**：`touch.rs` 的 `#[cfg(target_os = "linux")] mod linux` 调用 `select_index` 却未引入（该模块在 Windows 被 cfg 掉，**从未过编译器**）。修法：补进 `use super::{…}` 清单。
- **通过判据**：产物 `target/aarch64-unknown-linux-gnu/release/mupc-local-display` = `ELF 64-bit LSB pie, ARM aarch64`，解释器 `/lib/ld-linux-aarch64.so.1` ✓
- **顺带（lint/test job 口径）**：宿主 x86_64 `cargo test -p local-display --lib` = **369 passed / 0 failed**。
- **⚠️ 同批发现（P0，原清单未列）**：`mupc-core-bin` 在 Linux 上**根本编译不过** —— `display_host.rs::read_mem_used_pct` 的 Linux 分支把 `f32` 的 `usage_percent` 当 `f64` 返回（E0308，同属「Linux 分支持久未过编译器」）。已修（`f64::from`）。**推论**：`mupcd` 主程序此前从未在 Linux 上编译通过。
- 遗留：8 条 `dead_code`（`p3_logs.rs` / `p5_audit.rs` 的常量只被 `const _` 自证块与 `#[cfg(test)]` 引用），归入 L-6 口径。

**L-2 · ✅ 已根治**：`git update-index --chmod=+x` ⇒ 索引与磁盘均为 `100755`；`./gen_fonts.sh` 可直接执行（缺 `.otf` 时给出清晰报错而非 file-not-found）。CI workflow 已同步把 `bash ./gen_fonts.sh` 改回 `./gen_fonts.sh`。**附**：重跑 `gen_fonts.sh`（10 档 × 326 字符）后，两份入库派生清单 `lv_font_cmap.txt` / `lv_font_metrics.txt` **零差异**（漂移检测仍成立）。

**L-3 · ✅ 已定位并修复（结论与清单原假设不同）**：未复现「`reclaim` 收到已释放 `p`」路径；抓到并修掉的是 **indev 惯性抛掷动画 UAF**。证据链：ASAN 原始代码 3 跑 2 崩 → 对照实验（不释放 indev）5/5 零崩溃 → 修复后 **20/20 零崩溃**。U-61 ①② 已同批收口。详见 U-58 / U-61 行。

**L-4 · ⏳ 未执行**：需真机（本机非目标装置）。

**L-5 · ✅ 已实现**：`Iec104Server::link_state()` + `display_host` 注入点 + `startup.rs` 接线，测试 gateway 3 例 / display_host 2 例全过。详见 U-59 行。

**L-6 · 📋 已实测，待裁定**：按 CI 同口径（`cargo clippy --workspace --message-format short`，本机 stable 1.95.0）实测 **115 条告警**（U-57 登记的「93 条」为当时口径，本批新增代码后有变化）：

| crate | 条数 | | crate | 条数 |
|-------|:---:|---|-------|:---:|
| `local-display` | 49 | | `hplc-plugin` | 4 |
| `ai-engine` | 24 | | `data-processing` | 4 |
| `strategy-engine` | 10 | | `storage` | 3 |
| `mupc-core-bin` | 6 | | `intercore` | 3 |
| `sim-bridge` | 5 | | `core` | 2 |
| `rs485-plugin` | 5 | | | |

其中 **45 条是 `casting to the same type is unnecessary (u32 -> u32)`**（集中在 `local-display/src/lvgl/{widgets,style,obj}.rs`）——**与 L-1 同一根因**：Linux 上 FFI 枚举即 u32，而薄层为跨平台写的 `as sys::lv_xxx_t` 在 Linux 上变成恒等转换 ⇒ **不能一删了之**（删了 Windows 侧可能编不过），需按 `EventCode` 的别名化思路重做。其余为 unused import/variable、`map_or`、`deref`、`clamp` 等常规项。**✅ 已裁定并执行（2026-09-19）：清账** —— 45 条 cast 按 `EventCode` 的别名化思路重做（8 个薄层 newtype 内层类型改为 FFI 别名），其余逐条机械修复或加**带理由**的 `#[allow]`；清后 `cargo clippy --workspace` **0 告警 / 0 错误**，`-D warnings` gate 自此可按 CI 原口径运行（不含 `--tests`；测试目标的既有告警不在该 gate 口径内）。

**L-7 · 📋 提醒（未动）**：U-42 字体门禁与设计 §14 的 R-03 / R-04 / R-05 / R-22 仍待真机。

#### 8.6.2 CI test job 打通 + 既有失败测试修复（2026-09-19 续）

L-1~L-6 完成后，CI 的 `lint` job 可达（clippy 0 告警），但 `test` job 仍**结构性跑不起来**。
本节记录根因与修复：

**① `npu` 构建接线缺陷（P0，阻断 CI test/lint job 与 CMake 的"无 NPU"路径）**

- **现象**：x86_64 上 `cargo test --workspace` 在链接期失败 —— `vendor/rknn/librknnrt.so`
  是 **aarch64** 库，而 `#[link(name = "rknnrt")]` 原先只按 `target_os = "linux"` 判定
  ⇒ `rust-lld: ... is incompatible with elf64-x86-64`。CI（无 vendor/）同理报
  `cannot find -lrknnrt`。
- **第二层缺陷**：`mupc-ai-engine` 的 `default = ["npu"]` 让"关 npu"**不可达** ——
  `cargo --workspace` 类命令会打开**每个成员自身**的 default features（与依赖方是否
  `default-features = false` 无关）⇒ `--no-default-features` 无效（CMakeLists 原注释
  所依赖的假设不成立），CI 的 "no npu" 回退构建同样会链接失败。
- **修复**：(a) 真 FFI 的 cfg 增补 `target_arch = "aarch64"`（Rockchip 只发布 aarch64 的
  .so —— 注：build.rs 的自动探测同时找 `aarch64/` 与 `armhf/`，但新 cfg 只认 aarch64，
  armhf 分支自此只会在 aarch64 目标上误拷 32 位库）；(b) `mupc-ai-engine` 改
  `default = []`，三个依赖方改 `default-features = false` ⇒ **`--features npu` 成为唯一开关**；
  (c) build.rs 补两条安全网警告（aarch64 漏开关 ⇒ 提示"部署请加 --features npu"；非 aarch64
  开 npu ⇒ 提示走 stub）。
- **验证**：x86_64 全量测试**链接错误 0**；aarch64 + `--features npu` 仍链接真实库
  （SHA256 校验通过）；aarch64 不带 npu 时给出上述警告并走 stub。

**② 既有失败测试逐个修复（这些曾让 CI test job 即使能跑也必红）**

| crate | 失败项 | 根因（实测） | 处置 |
|-------|--------|--------------|------|
| data-processing | `waveform::trigger::test_cooldown` | 用例期望"冷却期过后**持续故障**再次触发"，与设计 §3.3.2 状态机（`Triggered` 仅在完全恢复后回 `Normal`，**回差优先于冷却**）相悖；P1-03 引入回差后该用例一直红 | 按设计**改测试**（补全"恢复→回 Normal→再越限"的完整路径） |
| data-processing | `fault_recorder_tests` | ① 临时库只按 pid 命名 ⇒ 同进程多用例抢锁（`database is locked`）；② `trigger_time` 写入用**秒**、查询/测试用**毫秒**（设计 §3.3.3 与建表注释均规定毫秒）⇒ 时间范围查询永远取不到 | ① 按用例名隔离库文件；② 写入与保留期截止统一改为 `timestamp_millis()` |
| mqtt-plugin | `test_mqtt_client_creation` ×2 | 构造函数经 tokio 通道，而用例是同步 `#[test]` ⇒ "no reactor running" | 改 `#[tokio::test]` |
| mqtt-bridge | `test_qos_mapping` | `LocalMqttClient::new` 把 `connected` 硬编码为 `true`（握手都没做就自称已连接） | 初值改 `false`（真值由事件循环在 ConnAck/断开时置位） |
| ota-update | 13 例 | ① `parse_hhmm` 不强制 `HH:MM` 两位（设计用 "02:00"）；② 同步读取口用 `tokio::RwLock::blocking_read()` 却在 `#[tokio::test]` 内直接调用；③ `generate_temp_path` 缺 `ota_` 前缀、不认 `?file=` 查询参数；④ `Downloader::new` 不校验临时目录；⑤ `rollback_success` 的两个断言指向**同一路径**（自相矛盾）；⑥ `with_callback` 的回调只在回滚时触发而用例断言其已被调用；⑦ `verify_platform_compatibility` 用例 `copy_from_slice` 源/目标长度不匹配；⑧ `validate()` 缺 `retry_count == 0` 下界 | ① 严格两位；② 测试改 `spawn_blocking` / 降为 `#[test]`；③ 补前缀 + 支持 `file=` 参数；④ 构造期 `create_dir_all` 校验；⑤ 改为校验**内容**为旧模型；⑥ 断言改为"未被调用"并注明触发路径；⑦ 按头部布局写 4 字节；⑧ 补下界（新增 `InvalidRetryCount`） |
| hplc-plugin | doctest `HplcConfig::new` | 示例缺 `use`（E0433） | 补 `use hplc_plugin::config::HplcConfig;` |

**结果**：`cargo test --workspace --exclude mupc-iec61850-plugin --exclude rs485-plugin
--exclude device-trait` = **1738 passed / 0 failed**（退出码 0），`cargo clippy --workspace`
= **0 错误 0 告警**。

> ⚠️ **订正（2026-09-20 评审）**：上面两项只证明 `test` job 与 `lint` job 的 **clippy 一步**
> 可跑通。`lint` job 的**第一步**是 `cargo fmt --all -- --check`（workflow:57），而仓库存在
> **既有格式漂移**（实测 114 文件 / 1236 处差异）⇒ **`lint` job 整体仍结构性红**，不可在分支
> 保护里启用（否则所有 PR 卡在 Check formatting）。**此前本节"lint 与 test 两个 job 均可按
> 原口径跑通"的表述不准确，已更正。** 漂移清零是启用 `lint` 的前置。

**⚠️ 环境侧两条（非模块代码，供后续同环境复现参考）**

1. rustup **目录 override** 把 `mupc/` 钉在 **1.86.0**，而拉取后的 `Cargo.lock` 中 `time 0.3.46` 要求 **≥ 1.88** ⇒ 该目录下裸 `cargo` 命令直接报错。本次验证一律用 `cargo +stable`（本机 stable = 1.95.0）。建议 `rustup override set stable`。
2. x86_64 宿主上 `mupc-core-bin` 的**测试**无法链接：`mupc/vendor/rknn/librknnrt.so` 是 **aarch64** 库，而 `ai-engine` 默认 features 启用 `npu` ⇒ `-lrknnrt` 在 x86_64 上必失败（CI 因 `vendor/` 不入库而无此问题）。本次以「临时把 `ai-engine` 的 `default` 置空」跑通，**已还原**。

> **完成后**：回写本节的执行结果，并同步对应 U 行（U-55 ~ U-61）与设计 §14 的 R 项状态。**本清单不是新需求**，只是把已登记的债务/风险按"在哪个环境做"重新排序。

---

### 8.7 Linux 环境交接清单（BECG-3588 联调 + 真机验收累计项，2026-09-26）

> **为什么单列一节**：承接 **§8.6**。先**回写 §8.6 的现状**（本轮复核）：**L-1 / L-2 / L-3 / L-5 已在 Linux 侧执行完毕** —— 证据：`gen_fonts.sh` 的 git 模式位 = **`100755`**（L-2）；`.github/workflows/build-ubuntu.yml` 含 **`hmi`** job（L-1）；`gateway::Iec104Server::link_state()` 已实现于 `mupc/crates/gateway/src/iec104/server.rs:527`（L-5）；L-3（ASAN 定位 U-58）亦已在 Linux 侧收口。**L-4 / L-6 / L-7 仍未闭合**（见 M-8）。
>
> 自那以后又落了 **U-73 外设上屏整链**（帧 v3 / 短标签表与字库门禁 / P4 消防页 / P6「装置与外设」/ HMI 接线 / 部署文档）与 **U-74 上云**（IEC104 装配 / MQTT 真做）—— 它们的**全部结论都来自 x86_64 离屏 + 假时钟 + `MockBus`**，**真机档一条未验**。本节把所有「**本地测试结构性看不到**」的项汇成一页，供 Linux 开发环境**按序推进**。
>
> **权威细节分别在**：`docs/superpowers/plans/2026-09-24-BECG-3588与60kW-PCS测试环境搭建方案.md`（§5 接线映射 / §6 分级步骤 / §7 电气安全 / §9 待厂商项）与 12 号设计 **§15.8.2**（真机档 D-1…D-6）。**本节只做索引与门禁口径，不复制其内容**。

**M-1 · L1 通信联调（P0；需 BECG-3588 到货 + PCS 仅控制上电）**

- **前置三件（缺一不可）**：① `mupc/deploy/config/*.yaml` 按方案 **§5.2 串口重映射表**改（3568 → 3588 是**节点乱序**：A0→ttyS0 / A2→ttyS3 / A3→ttyS7 / A4→ttyS8 / A5→ttyP0 / A6→ttyP1 / A7→ttyP2 / A8→ttyP3）；② §5.3 的 DI/DO 重映射（**DI 为反逻辑**：低 = 1、高 3.3–30 V = 0）；③ §5.4 的 PCS 侧寄存器初始化。
- **判据**：方案 **§1.2 A1–A8 全过**；`stty -F /dev/ttyS0 19200` 等与 §5.2 表**逐条一致**；DI/DO 两态实测**回填 §5.3 表**。
- ⚠️ **电气（引自 PCS 手册，硬约束）**：**断电后必须等 ≥20 min 电容放电**方可操作；其余见 §7.2 硬约束清单与 §7.4 上电/停电顺序（贴墙）。

**M-2 · L2 小功率带电**（方案 §6.2；直流限流接入）—— 判据：充 / 放 / 无功 / 斜坡指令与 3 区反馈一致（误差在量程内）；**急停 / 复位链路**（按下即停、复位需人工）。

**M-3 · L3 并网 / 带载**（方案 §6.3；**可选**，条件清单全满足才做）。

**M-4 · U-73 显示上屏真机验收**（12 号设计 §15.8.2 的 **D-1…D-6**；**未过不得通过 P4 / P6 真机验收**）

- **D-1 / H-4 豆腐块逐字核对**：P4 消防区 + P6 五段的**全部新增中文**（短标签表 447 行 / `ui_text` / 分组标题）真屏逐字无豆腐块。**已知缺口**：`✕ U+2715` / `❚ U+275A` **字库源无字形**（生产未用、仍在码表内）⇒ 真机若发现用到须先改字库或换字符。
- **D-2 / H-5 槽宽**：带符号数值（BMS 簇组电流 `-1600.0`、PCS 有功 `-60.0`）与长标签（「储能表·电压不平衡度」等）**不得越出槽宽**。
- **D-3**：字号 0.5–1.5 m 可读性；1024×768 下实际列数 / 分组排布。
- **D-4 触摸**：分段切换 ≤300 ms、P4/P6 滚动 ≥30 fps、下钻与「收起」可达（≥48×48）。
- **D-5 资源**：`lv_mem_monitor` / `smaps` 复核 `LV_MEM_SIZE = 1 MB` 余量（**当前口径**：常驻 **922** 件 + 惰性 **694** 件 ⇒ 账上稳态 ≈23–31 % 占用，**以真机实测为准**）；稳态 CPU **≤40 % 单核**。
- **D-6 时延探针**：外设值变化 → 屏上生效，**口径 A / B 各测一次**（HVAC 与 BMS 各一例）。

**M-5 · U-74 上云真机（IEC104 / MQTT）**

- **IEC104**：真实链路下核对 —— 序号 `i2/i4` 修正后的**实际收发**行为、**总召**（`on_interrogation` 三段 + `just_connected` 初始快照）、**A/B/C 三档**、子集点数 **162 / 234**。
- **MQTT**：**真 broker** e2e（现有结论来自单测与 `enabled=false ⇒ 零连接尝试`）；**TLS fail-closed** 真验。

**M-6 · 落库真验**

- `telemetry.value` **可空化迁移**在**真表**上跑一遍（含**两个**索引重建 + `sqlite_sequence` 单调恢复）；1 分钟聚合 **22 行 / 周期**（18 均值 + 2 极值×2）；无采样周期须产 `NoData` 而**非 0**。

**M-7 · FLS-04 的 p99 压测（对应评审的 Q-p99）**

- 判据：落库失败期间**采集侧写入 p99 ≤ 10 ms** + **HST-ELEC-02 不回归**（**同一压测内一并复核**）。**本机测不了**（无南向真源 + 无真 DB 负载）⇒ 现有结论只是**结构证据**（`buffer_telemetry` 函数体零 `.await`、零 DB 调用）⇒ **不得据此宣称达标**。

**M-8 · §8.6 的结转项（仍未闭合，勿以为已随本批关闭）**

- **L-4 真机验收**：4a 配置写入（屏上改键 → 保存 → 重启 `mupcd` 后值仍在）；4b 触摸（`/dev/mupc-touch` 指向 `eventN`、页眉**无**「触摸不可用」角标）。
- **L-6**：CI lint job 用 `clippy --workspace -- -D warnings`，而**同口径**下本工作区有 **93 条既有告警**（分布在与模块无关的 crate）⇒ 需裁定「清账」还是「调整 gate 口径」。
- **L-7**：字体豆腐块 **23 码位**批 + **U-42**（真字库下 P1 页带符号 P 值装不下槽宽）。

**M-9 · 待厂商答复（不确认不进入对应阶段）**

- 测试环境方案 **§9 的 Q2–Q9**：BECG 各 DI 路的**阈值与极性实测值**、`ttyP0–3` 驱动是否随镜像（**转为到货验收项**）、PCS 侧寄存器语义 / 地址序 / 单位等。**Q1 已确认**（预装 Ubuntu 22.04）。

**M-10 · PCS 迁入南向后的真机复核项**（2026-09-26；来源 = 02 号设计 **§13** 落地 T1–T12，未结项见 **§6.13**）

> **为什么单列**：PCS 的通信与控制已由 `mupc-intercore` **整体迁入 `mupc-southd`**（`PcsHandle`；02 号设计 §13 / ADR-014），
> 而该批**全部结论都来自 x86_64 离屏 + `MockBus` + 帧级 e2e** ⇒ **真机档一条未验**。以下须在 **M-1（串口联调）之后**
> 逐条复核；**不得以"单测/静态断言全绿"代替**（尤其 M-10.5 的 fail-fast，其自动化锚是**源文本静态断言**，见 §6.13 U-77）。

- **M-10.1 采集循环时延**：`interval_ms`=1000 下，**一拍的端到端耗时**（FC04 读 76 字 + 解码 + 投递）与**拍间隔抖动**实测；判据 = 不超拍、不堆积、RS485 口占用率与设计 §13.4 的量级相符。⇒ 对应 **Δ-16** 的离线判定节奏（3 拍 ≈3 s）。
- **M-10.2 SOC 链路**：`PcsHandle.latest_soc()` 的**时间戳来源**由"调用时刻"变为"本拍采集时刻"（**Δ-18**）⇒ 真机核对 SOC 双源裁决（`AiIntegrator::resolve_soc_core`）在 PCS 掉线/恢复时的取值与告警是否与设计一致（`DATA_STALE_AFTER` = 5 s 不受影响）。
- **M-10.3 三相**：**单块读的降级粒度**（**Δ-17**）真机验证 —— 块读失败时三相**同时** `None`（不再有"电流段成功、有功段失败"的中间态）；12 号 §3.4 F5.5 的**消费侧** Offline/NotRead 角标仍逐字段独立。
- **M-10.4 联锁停机**：短接 DI1 急停 ⇒ PCS **实际停机**且 DO2 故障灯亮；复位 + Web release ⇒ 允许重启。**含 `stop()` 写 500=0 的回显校验真机行为**（错帧 / 异常码须被拒）与停机端到端时延。⇒ 对应 **U-79**（`write_single_register` 全仓零调用点，四项回显校验当前只有单测在守）。
- **M-10.5 fail-fast 运行期**：`io.enabled=true` 且 `south_pcs.enabled=false` ⇒ **启动即报错、不放行**（"能触发却停不了机"的假安全形态不得放行）。⇒ 对应 **U-77**（现有覆盖是**源文本静态断言**，不证运行期）。
- **M-10.6 写审计**：核验每次写序列是否落 **"谁 / 何时 / 写了什么 / 回读值"** 审计事件。⇒ 对应 **U-75** —— **该项当前未落地，预期为空**，不得据此判"已在真机通过"。
- **M-10.7 `south_pcs` 口独占**：`south_pcs.port` 与 `south_stations`（5 站）**不得共口**（规则 **P-2**）；真机上确认打开成功、无第二个所有者、`ttyS0` 无争用（原 `intercore.modbus_rtu` 已删，**部署 YAML 不得再含该子段**，见 `deploy/deploy.md` §9.1/§9.3）。
- **M-10.8 降级口径不对称复核**（可选）：`south_pcs.enabled=false` 时 AI 侧下发**静默 no-op**、IEC104 `p_set` **如实回 `success:false`**（**Δ-24** / **U-78**）⇒ 真机观察两条路径的可观测性差异，供产品裁定是否统一。

**建议顺序**：**§8.6-L-4**（验屏：只需交叉产物 + 真屏）与 **M-1**（验串口：只需 BECG）**可并行**；M-2 需 M-1 全过；M-3 需条件清单；**M-4 可与 M-1/M-2 并行**；**M-10 在 M-1 之后立即做**（PCS 迁入南向的真机复核，见 §6.13）；M-5 / M-6 需 M-1 之后有真实数据；**M-7 最后做**（需南向真源 + 压测负载）。

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
**最后更新**: 2026-09-23
**版本**: v3.7