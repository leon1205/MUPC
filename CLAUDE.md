# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

MUPC 微电网特种调控装置通信管理模块是"异构双核心模块主控架构"中的**非实时处理核心**（大脑），与"实时控制核心模块"（小脑）协同工作。

**核心职责：**

- 北向通信：与调度主站（IEC 104）、配电自动化（IEC 61850）、物联平台（MQTT）通信
- 南向通信：与台区设备（TTU、光伏逆变器、充电桩、柔性负荷）通信
- 本地策略引擎（AI 失效时的兜底）
- AI 边缘优化引擎（预测、强化学习决策）
- OTA 升级与远程维护

**目标平台：** Linux (openEuler)、RK3588 硬件
**编程语言：** Rust
**Rust 版本：** >= 1.75（workspace 使用 workspace-inheritance）
**异步运行时：** Tokio
**网络框架：** Tower + tokio-net

## 项目结构

```
mupc/
├── Cargo.toml              # Workspace 配置（20 个 crate）
├── config/
│   └── mupc_env_config.yaml # AI 引擎动态配置（v2.6，对齐训练管线）
├── crates/
│   ├── common/             # 公共库：日志、错误类型
│   ├── core/               # 核心组件
│   ├── gateway/            # 北向通信网关（IEC 104）
│   ├── intercore/          # 核间通信（TCP/RJ45）
│   ├── data-processing/    # 遥测数据采集
│   ├── strategy-engine/    # 本地策略引擎 + AI 集成
│   ├── ai-engine/          # AI 优化引擎（LSTM/MADDPG/RKNN Runtime）
│   │   └── src/
│   │       ├── safety_config.rs      # 安全约束配置（v2.6 新增）
│   │       ├── env_config.rs         # YAML 配置结构（v2.6 新增）
│   │       ├── dynamic_config_loader.rs  # 动态配置加载器（v2.6 新增）
│   │       ├── action_space.rs       # 动作空间配置（v2.6 扩展）
│   │       └── ...
│   ├── security/           # 安全模块
│   ├── web-api/            # Web API（Axum REST + SSE）
│   ├── storage/            # 持久化存储（SQLite/sqlx）
│   ├── ota-update/         # OTA 固件/模型升级
│   ├── system-monitor/     # 系统资源监控
│   ├── wireless/           # 无线通信（WiFi/ECDH 密钥协商）
│   ├── plugin-loader/      # 插件加载器
│   ├── iec61850-plugin/    # IEC 61850 协议插件
│   ├── mqtt-plugin/        # MQTT 协议插件
│   ├── rs485-plugin/       # RS485 通信插件
│   ├── hplc-plugin/        # HPLC 通信插件
│   ├── mqtt-bridge/        # MQTT 桥接
│   └── device-trait/       # 设备特性抽象
└── tests/                  # 集成测试
```

## 开发状态

- **Phase 1**: 核心架构完成
- **Phase 3C**: AI 优化引擎已完成（LSTM 预测、MADDPG/PPO 决策、RKNN Runtime 推理）
- **Phase 3C 补充**: 跨项目动态配置系统 v2.6（YAML 配置加载、分层加载、版本指纹校验）
- **Phase 2+**: IEC 61850-7-420、MQTT over TLS、SM2/SM4 国密（规划中）
- 技术债清单见 `docs/technical-debt.md`

## 开发命令

- 所有 cargo 命令必须在 `mupc/` 目录下执行：`cd mupc && cargo build --release`
- 构建：`cargo build --release`
- 测试：`cargo test`
- 代码检查：`cargo clippy`
- 单个测试：`cargo test -p <crate> <test_name>`
- 单 crate 构建：`cargo build -p <crate>`
- 格式化：`cargo fmt`

## 项目协作配置

本项目采用一套自定义的AI代理（Agents）协作框架进行开发。该框架定义了完整的角色、工作流程和质量门禁。

## 如何启用AI团队

当您需要开发新功能、修复Bug或进行任何代码变更时，**请直接提出您的需求**。

例如：

- “我们需要开发一个用户登录功能。”
- “修复首页图片无法加载的问题。”
- “根据这份PRD，开始进行开发。”

提出需求后，**本项目配置的‘项目经理’（Manager）Agent将被自动触发**。他将根据 `/.claude/agents/` 目录下的角色定义和 `/.claude/agents/AI_WORKFLOW/02_WORKFLOW.md` 定义的流程，调度需求分析师、架构师、开发工程师等角色，带领团队完成从需求分析到测试交付的全过程。

## 框架核心

- **角色定义**：所有Agent角色定义文件位于 `/.claude/agents/` 目录下。
- **工作流**：遵循 **合同与路径驱动** 流程，定义在 `/.claude/agents/AI_WORKFLOW/02_WORKFLOW.md`。项目经理将根据项目特征选择“标准”、“简单”或“纯端”路径执行。
- **术语**：核心概念和技能定义在 `/.claude/agents/AI_WORKFLOW/05_GLOSSARY.md`。

## Crate 命名注意

- 大部分 crate 使用 `mupc-` 前缀（如 `mupc-common`、`mupc-ai-engine`）
- 无前缀的 crate：`device-trait`、`plugin-loader`、`rs485-plugin`、`hplc-plugin`
- **`mqtt-bridge`**：目录名为 `mqtt-bridge`，Cargo.toml name = `mupc_mqtt_bridge`（下划线），依赖引用时必须用下划线
- **`storage`**：目录名为 `storage`，Cargo.toml name = `mupc_storage`（下划线），依赖引用时必须用 `mupc_storage`

## 约束

1. 全部使用中文进行回复
2. 修改文件前需要先描述方案（Why + How），等同意再动手
3. 需求不清晰时请先提问澄清
4. **强制评审**：未完成评审禁止推进下一阶段，违规者代码回退并重新走评审流程
5. 方案描述格式：**背景（Why）** → **方案（How）** → **改动点（What）**

## 核心架构

### 架构模式

```
调度主站 ←→ gateway (IEC 104) ←→ data-processing ←→ strategy-engine
                                              ↓              ↑
                              intercore (TCP/RJ45) ←→ 实时控制模块
                                              ↑
南向设备 ←→ rs485-plugin/hplc-plugin ←─── ProtocolHandler 注入
```

### 数据流

| 方向   | 组件                                          | 说明                |
| ------ | --------------------------------------------- | ------------------- |
| 北向↑ | gateway → data-processing → strategy-engine | 调度数据处理        |
| 南向↓ | strategy-engine → rs485-plugin/hplc-plugin   | 设备控制            |
| 核间↕ | intercore (TCP/RJ45)                          | 与实时控制模块通信  |
| AI →  | strategy-engine ← ai-engine                  | LSTM 预测 + RL 决策 |

### 关键组件


| 模块                | 职责                                                             | 关键代码路径 |
| ------------------- | ---------------------------------------------------------------- | ------------ |
| **common**          | 日志（tracing）、统一错误类型、通用工具                          | `common/src/` |
| **gateway**         | 北向 IEC 104 协议通信、连接管理、数据收发                        | `gateway/src/` |
| **intercore**       | TCP 网络通信、指令下发、数据读取、心跳/看门狗                    | `intercore/src/` |
| **ai-engine**       | LSTM 时序预测、MADDPG/PPO 强化学习决策、RKNN Runtime（NPU 推理） | `ai-engine/src/model_manager.rs` |
| **ai-engine**       | 动态配置加载器（YAML 分层加载、版本指纹校验、操作参数热重载）     | `ai-engine/src/dynamic_config_loader.rs` |
| **ai-engine**       | 安全约束配置（SOC 硬约束、变压器过载阈值）                      | `ai-engine/src/safety_config.rs` |
| **ai-engine**       | 环境配置结构（EnvConfig/PhysicalConfig/OperationalConfig）       | `ai-engine/src/env_config.rs` |
| **strategy-engine** | 兜底策略（削峰填谷、需量控制、防逆流），AI 指令安全校验          | `strategy-engine/src/ai_integration.rs` |

### 核间通信

与实时控制模块通过 **TCP Socket (RJ45)** 交互。

**关键信号（通过 TCP 帧传输）：**

- `ai_ready`：AI 引擎可用状态
- `strategy_mode`：当前策略模式（基础/智能/兜底）
- `control_cmd`：下发给实时控制模块的指令

### 南向通信架构

```
rs485-plugin/hplc-plugin ←→ device-trait (统一抽象) ←→ strategy-engine
```

**核心 trait：**


| Trait             | 说明                                       |
| ----------------- | ------------------------------------------ |
| `SouthDevice`     | 统一南向设备接口（RS485/HPLC）             |
| `ProtocolHandler` | 协议处理器注入（Modbus/TTU/逆变器/充电桩） |
| `HplcDriver`      | HPLC 芯片驱动抽象（预留 FFI）              |

**协议处理器（ProtocolHandler）：**


| 处理器            | 协议       | 支持设备         |
| ----------------- | ---------- | ---------------- |
| `ModbusHandler`   | Modbus RTU | 通用 Modbus 设备 |
| `TtuHandler`      | TTU 专用   | 配变终端         |
| `InverterHandler` | 厂商私有   | 光伏逆变器       |
| `ChargerHandler`  | GB/T 27930 | 充电桩           |

**配置文件格式：**

- RS485：`handler` 字段指定协议类型（modbus/ttu/inverter/charger）
- HPLC：`serial_port`（Linux=/dev/ttyUSB0, Windows=COM3）
- RS485 半双工：DE/RE GPIO 控制

### 插件系统

```
plugin-loader (动态加载 .so/.dll)
├── device-trait::Plugin trait
├── FFI 规范：create_plugin() + plugin_meta()
└── 内置插件：rs485-plugin、hplc-plugin、iec61850-plugin
```

**FFI 导出函数：**

- `create_plugin()` → `*mut dyn Plugin`（插件工厂）
- `plugin_meta()` → `PluginMeta`（获取插件元信息）

**插件生命周期：** Load → Init → Start → Stop → Unload

### AI 引擎与策略引擎集成

```rust
// strategy-engine/src/ai_integration.rs
strategy-engine ←→ AiIntegrator ←→ ai-engine::ModelManager
                                  ├── LSTM 预测 → 供 RL 模型使用
                                  ├── MADDPG/PPO 决策 → ActionOutput
                                  └── RKNN Runtime → RK3588 NPU 推理
```

**数据流：**

1. LSTM 时序预测（光伏出力/负荷）
2. RL 模型基于预测结果决策
3. AiValidator 校验 AI 指令安全性
4. 通过 intercore 下发给实时控制模块

### Web API 架构

Web API 基于 Axum 0.7，路由位于 `web-api/src/routes/ai_routes.rs`。

**路由类型约定：**

```rust
// ai_routes() 返回 Router<Arc<AppState>>，类型参数编译时保证
pub fn ai_routes() -> Router<Arc<AppState>> { ... }

// 所有 handler 提取 State<Arc<AppState>>
async fn handler(State(state): State<Arc<AppState>>) -> ...
```

**数据源注入模式：** handler 不通过 AiIntegrator 访问数据，而是在 AppState 直接注入：
- `storage: Arc<StorageService>` — 决策记录、事件查询
- `ota_manager: Arc<dyn OtaManager>` — 模型版本、回滚
- `online_updater: Arc<Mutex<OnlineUpdater>>` — 在线微调状态
- `ab_test_manager: Arc<AbTestManager>` — A/B 测试 CRUD

**AI 端点：** 27 个 handler，22 个接入真实数据源，5 个保持占位（predictions×3、weights×2，因上游 AiIntegrator 缺少对应 API）。

**认证：** `RequireRole` 提取器（`X-Session-Id` 头）已接入 `post_rollback` 和 `delete_ab_test`。

## 已知测试失败

以下测试为预存失败，非近期引入：

| Crate | 测试 | 原因 |
|-------|------|------|
| device-trait | `test_modbus_crc_calculation`、`test_modbus_handler_encode_decode`、`test_inverter_handler_encode_decode` | south_device 实现不完整 |
| rs485-plugin | `test_config_with_gpio` | 配置反序列化字段缺失 |
| mupc-iec61850-plugin | `test_parse_goose_pdu` | GOOSE PDU 解析未完成 |

## 重构验证

代码变更后，必须按以下清单验证：

**编译与测试**

- [ ]  `cargo build --release` 编译成功
- [ ]  `cargo clippy` 无警告
- [ ]  `cargo test` 所有测试通过
- [ ]  `cargo fmt` 格式化通过

**功能回归**（根据变更模块选择验证）


| 模块     | 验证项                                              |
| -------- | --------------------------------------------------- |
| 通信网关 | IEC 104/IEC 61850/MQTT 连接建立、协议转换数据一致性 |
| 数据处理 | 遥测数据上送频率 ≥1Hz、故障录波触发                |
| 策略引擎 | 削峰填谷、需量控制、防逆流策略                      |
| 南向通信 | RS485 协议处理器、ProtocolHandler 注入、HPLC 驱动   |
| AI 引擎  | LSTM 预测 <1s、RL 决策 <1s、RKNN Runtime NPU 推理   |
| 核间通信 | `ai_ready`、`strategy_mode`、`control_cmd` 信号     |

**安全验证**

- [ ]  无硬编码密钥（检查 SM2/SM4 密钥残留）
- [ ]  无新增 `unsafe` 块
- [ ]  错误类型实现 `std::error::Error`

## 技术债

Phase 1/3C 已完成的技术债更新（记录于 `docs/technical-debt.md`）：


| Phase       | 内容                                                             | 状态    |
| ----------- | ---------------------------------------------------------------- | ------- |
| Phase 1     | 核心架构（gateway、intercore、data-processing、strategy-engine） | ✅ 完成 |
| Phase 3C    | AI 优化引擎（LSTM 预测、MADDPG/PPO 决策、RKNN Runtime 推理）     | ✅ 完成 |
| Phase 3C 补充 | 跨项目动态配置系统（YAML 配置加载、分层加载、版本指纹校验）    | ✅ 完成 |
| Phase 2+    | IEC 61850-7-420、MQTT over TLS、SM2/SM4 国密                     | 规划中  |
| Phase 2+    | 南向通信（RS485/HPLC）、OTA 升级、安全启动                       | 规划中  |

## 配置文件

AI 引擎配置文件位于 `mupc/config/mupc_env_config.yaml`，与训练管线 v2.6 对齐。

**配置结构：**

```yaml
version:
  fingerprint: "v2.6-20260611"  # 版本指纹（启动校验）
  source: "mupc-ai2"

physical:                        # RL 核心参数（YAML 锁定）
  transformer_kva: 200.0          # 变压器额定容量
  battery_capacity_kwh: 100.0    # 电池总容量
  p_batt_max_kw: 50.0            # 最大充放电功率
  load_shed_max_kw: 60.0         # 最大切负荷

safety:                          # 安全约束（可被 DB 覆盖）
  soc_min: 0.10                  # SOC 下限
  soc_max: 0.90                  # SOC 上限
  overload_threshold: 0.85       # 过载阈值

operational:                     # 操作调优参数（DB 优先）
  p_batt_ramp_limit_kw: 50.0    # 有功变化率限制
  q_batt_ramp_limit_kvar: 30.0   # 无功变化率限制
  pv_limit_min: 0.10             # 光伏限功率下限
```

**分层加载策略：**

1. YAML 加载 → 基准配置（RL 核心参数锁定）
2. DB 查询 → 操作参数覆盖（6 个开放参数）
3. 版本指纹校验 → 启动时校验对齐

**动态配置组件：**

| 组件                   | 文件                                      | 职责                     |
| ---------------------- | ----------------------------------------- | ------------------------ |
| `DynamicConfigLoader`  | `ai-engine/src/dynamic_config_loader.rs` | 分层加载 + 指纹校验      |
| `SafetyConfig`         | `ai-engine/src/safety_config.rs`         | SOC/过载约束              |
| `EnvConfig`            | `ai-engine/src/env_config.rs`             | YAML 配置结构解析        |
| `ActionSpaceConfig`    | `ai-engine/src/action_space.rs`           | 扩展 5 个新字段（v2.6）  |

## 文档管理原则（2026-05-29 生效）

文档结构见 `docs/superpowers/` 目录。二级体系：项目主文档 + 模块文档（PRD/设计文档），历史报告归档于 `reports/`。

**强制规则：**
- 新增需求/设计 → 追加到对应模块文档内，不得创建独立文件
- 跨模块变更 → 更新项目主文档的跨模块交互章节
- 禁止独立的时间戳文件（`specs/YYYY-MM-DD-功能名-PRD.md` 格式）
- 评审通过后顶部标注 `[REVIEWED: PASS]` 或 `[DESIGN_APPROVED]`，版本号递增
