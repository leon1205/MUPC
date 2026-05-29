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
├── Cargo.toml              # Workspace 配置（18 个 crate）
├── crates/
│   ├── common/             # 公共库：日志、错误类型
│   ├── core/               # 核心组件
│   ├── gateway/            # 北向通信网关（IEC 104）
│   ├── intercore/           # 核间通信（TCP/RJ45）
│   ├── data-processing/     # 遥测数据采集
│   ├── strategy-engine/    # 本地策略引擎 + AI 集成
│   ├── ai-engine/          # AI 优化引擎（LSTM/MADDPG/RKNN Runtime）
│   ├── security/           # 安全模块
│   ├── web-api/             # Web API
│   ├── plugin-loader/       # 插件加载器
│   ├── iec61850-plugin/    # IEC 61850 协议插件
│   ├── mqtt-plugin/         # MQTT 协议插件
│   ├── rs485-plugin/        # RS485 通信插件
│   ├── mqtt-bridge/         # MQTT 桥接
│   └── device-trait/        # 设备特性抽象
└── tests/                  # 集成测试
```

## 开发状态

- **Phase 1**: 核心架构完成
- **Phase 3C**: AI 优化引擎已完成（LSTM 预测、MADDPG/PPO 决策、RKNN Runtime 推理）
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
- **`mqtt-bridge`**：目录名为 `mqtt-bridge`，但 Cargo.toml name = `mupc_mqtt_bridge`（下划线），在 `Cargo.toml` 依赖中引用时必须用下划线

## 约束

1. 全部使用中文进行回复
2. 修改文件前需要先描述方案，等同意再动手
3. 需求不清晰时请先提问澄清
4. **强制评审**：未完成评审禁止推进下一阶段，违规者代码回退并重新走评审流程

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

| 方向 | 组件 | 说明 |
|------|------|------|
| 北向↑ | gateway → data-processing → strategy-engine | 调度数据处理 |
| 南向↓ | strategy-engine → rs485-plugin/hplc-plugin | 设备控制 |
| 核间↕ | intercore (TCP/RJ45) | 与实时控制模块通信 |
| AI → | strategy-engine ← ai-engine | LSTM 预测 + RL 决策 |

### 关键组件

| 模块 | 职责 |
|------|------|
| **common** | 日志（tracing）、统一错误类型、通用工具 |
| **gateway** | 北向 IEC 104 协议通信、连接管理、数据收发 |
| **intercore** | TCP 网络通信、指令下发、数据读取、心跳/看门狗 |
| **ai-engine** | LSTM 时序预测、MADDPG/PPO 强化学习决策、RKNN Runtime（NPU 推理） |
| **strategy-engine** | 兜底策略（削峰填谷、需量控制、防逆流），AI 指令安全校验 |

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

| Trait | 说明 |
|-------|------|
| `SouthDevice` | 统一南向设备接口（RS485/HPLC） |
| `ProtocolHandler` | 协议处理器注入（Modbus/TTU/逆变器/充电桩） |
| `HplcDriver` | HPLC 芯片驱动抽象（预留 FFI） |

**协议处理器（ProtocolHandler）：**

| 处理器 | 协议 | 支持设备 |
|--------|------|----------|
| `ModbusHandler` | Modbus RTU | 通用 Modbus 设备 |
| `TtuHandler` | TTU 专用 | 配变终端 |
| `InverterHandler` | 厂商私有 | 光伏逆变器 |
| `ChargerHandler` | GB/T 27930 | 充电桩 |

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

## 重构验证

代码变更后，必须按以下清单验证：

**编译与测试**
- [ ] `cargo build --release` 编译成功
- [ ] `cargo clippy` 无警告
- [ ] `cargo test` 所有测试通过
- [ ] `cargo fmt` 格式化通过

**功能回归**（根据变更模块选择验证）

| 模块 | 验证项 |
|------|--------|
| 通信网关 | IEC 104/IEC 61850/MQTT 连接建立、协议转换数据一致性 |
| 数据处理 | 遥测数据上送频率 ≥1Hz、故障录波触发 |
| 策略引擎 | 削峰填谷、需量控制、防逆流策略 |
| 南向通信 | RS485 协议处理器、ProtocolHandler 注入、HPLC 驱动 |
| AI 引擎 | LSTM 预测 <1s、RL 决策 <1s、RKNN Runtime NPU 推理 |
| 核间通信 | `ai_ready`、`strategy_mode`、`control_cmd` 信号 |

**安全验证**
- [ ] 无硬编码密钥（检查 SM2/SM4 密钥残留）
- [ ] 无新增 `unsafe` 块
- [ ] 错误类型实现 `std::error::Error`

## 技术债

Phase 1/3C 已完成的技术债更新（记录于 `docs/technical-debt.md`）：

| Phase | 内容 | 状态 |
|-------|------|------|
| Phase 1 | 核心架构（gateway、intercore、data-processing、strategy-engine） | ✅ 完成 |
| Phase 3C | AI 优化引擎（LSTM 预测、MADDPG/PPO 决策、RKNN Runtime 推理） | ✅ 完成 |
| Phase 2+ | IEC 61850-7-420、MQTT over TLS、SM2/SM4 国密 | 规划中 |
| Phase 2+ | 南向通信（RS485/HPLC）、OTA 升级、安全启动 | 规划中 |

## 文档管理原则（2026-05-29 生效）

### 二级文档体系

```
docs/superpowers/
├── specs/
│   ├── PROJECT-MUPC-项目需求主文档.md     ← 项目级需求入口（跨模块交互、模块索引）
│   └── modules/
│       ├── 01-MUPC-通信网关-PRD.md         ← 模块级需求（详细功能、用户故事、验收标准）
│       ├── 02-MUPC-南向通信-PRD.md
│       ├── ...
│       └── 10-MUPC-核间通信-PRD.md
├── plans/
│   ├── PROJECT-MUPC-项目设计主文档.md     ← 项目级设计入口（架构总览、跨模块决策）
│   └── modules/
│       ├── 01-MUPC-通信网关-设计文档.md     ← 模块级设计（详细架构、接口、数据流）
│       ├── ...
│       └── 10-MUPC-核间通信-设计文档.md
└── reports/                               ← 历史报告（只读归档，不删除）
```

### 强制规则

1. **新增需求** → 在对应模块 PRD (`modules/XX-模块名-PRD.md`) 内直接追加章节，不得创建独立 PRD 文件
2. **新增设计** → 在对应模块设计文档 (`modules/XX-模块名-设计文档.md`) 内直接追加章节，不得创建独立设计文件
3. **跨模块变更** → 更新 `PROJECT-MUPC-项目需求主文档.md` 或 `PROJECT-MUPC-项目设计主文档.md` 中的跨模块交互章节
4. **版本管理** → 模块文档内部维护版本号和修订记录，项目主文档同步更新模块状态
5. **禁止重复** → 不再创建 `specs/YYYY-MM-DD-功能名-PRD.md` 或 `plans/YYYY-MM-DD-功能名-设计文档.md` 格式的独立文件
6. **历史文档** → `reports/` 目录下的报告为历史归档，只读保留，不删除
7. **实施计划** → `plans/` 下按日期命名的实施计划文件为历史记录，新计划在模块设计文档的"实施计划"章节中描述

### 文档评审流程

- 模块 PRD 顶部标注 `[REVIEWED: PASS]` 表示需求合同已履行
- 模块设计文档顶部标注 `[DESIGN_APPROVED]` 表示设计合同已履行
- 评审通过后版本号递增（v1.0 → v1.1 → v2.0）
- 未评审文档标记为"草稿"，禁止进入开发阶段
