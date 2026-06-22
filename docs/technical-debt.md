# 技术债清单

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-27 | 项目经理 | 记录中 |
| v2.0 | 2026-06-15 | 项目经理 | 审计更新 — 新增文档-代码一致性审计发现 |
| v2.1 | 2026-06-18 | 项目经理 | v2.17 新增：SafetyRLWrapper broadcast channel 组装（U-15）|
| v2.2 | 2026-06-21 | 项目经理 | v3.0 新增：训练-部署输入维度不一致（U-16，P0 已修复）+ 根因分析 |

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
| **状态** | 🔲 待优化 |

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
| OTA 升级 | 差分升级、安全启动 | 低 | ⚠️ 模型OTA完成；固件OTA骨架就位（A/B分区/签名待实现） |
| 故障录波 | 完整实现 | 中 | ⚠️ 4个方法为stub（get_waveform等） |

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
| U-15 | 05 AI引擎 | **P1** | v2.17 SafetyRLWrapper `event_sender` 未连接 — broadcast channel 需 main.rs 组装，当前 `ModelManager::new()` 传入 `None`，违规时无 SSE 实时推送 | 需系统集成入口（bin crate）创建 broadcast channel，Sender 注入 SafetyRLWrapper，Receiver 转发到 SsePushService（~5 行胶水代码） |

### 6.3 U-16 根因分析

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

---

## 7. 技术债统计

| 类别 | 数量 | 已修复 | 待修复 | 状态 |
|------|------|--------|--------|------|
| 严重问题 (Phase 1) | 2 | 2 | 0 | ✅ 全部修复 |
| 警告问题 (Phase 1) | 4 | 4 | 0 | ✅ 全部修复 |
| 优化建议 (Phase 1) | 3 | 2 | 1 | 🔄 1 项待优化 |
| 审计发现-已修复 (本轮) | 11 | 11 | 0 | ✅ 全部修复 |
| 审计发现-未修复 (P0) | 6 | 0 | 6 | 🔴 待规划 |
| 审计发现-未修复 (P1) | 6 | 4 | 2 | 🟡 待排期 |
| v2.17 新增 (P1) | 1 | 0 | 1 | 🟡 待排期 |
| 审计发现-未修复 (P2) | 2 | 2 | 0 | ✅ 全部修复 |
| **总计** | **35** | **25** | **10** | |

---

## 8. 修复计划

### 7.1 短期修复（1-2天）

| 问题 | 负责人 | 预计时间 |
|------|--------|----------|
| U-01 RBAC 鉴权中间件 | 2天 | 需确认角色模型和权限矩阵 |
| U-02 sqlx/rusqlite 双库统一 | 3天 | 需评估 sqlx 功能覆盖度 |

### 8.2 中期修复（P1 — 本月）

| 问题 | 预计时间 | 阻塞 |
|------|----------|------|
| U-07 固件 OTA 分区切换 + SM2 验签 | 3天 | 需 SM2 签名就绪 |
| U-08 系统监控完善 | 2天 | 需硬件看门狗驱动 |

### 8.3 长期修复（外部依赖）

| 问题 | 预计时间 | 阻塞 |
|------|----------|------|
| U-03 SM2/SM4 真国密 | 5天 | gmsm 0.14 发布 |
| U-04 rustls CryptoProvider | 3天 | 依赖 U-03 |
| U-05 安全启动信任链 | 5天 | RK3588 OTP/eFuse 驱动 |
| U-06 WiFi/NearLink/BLE | 10天 | Hi2821 硬件 + 内核驱动 |

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
**版本**: v1.0