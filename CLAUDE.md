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
**异步运行时：** Tokio
**网络框架：** Tower + tokio-net

## 项目结构

```
mupc/
├── Cargo.toml              # Workspace 配置
├── crates/
│   ├── common/             # 公共库：日志、错误类型
│   ├── gateway/             # 北向通信网关（IEC 104）
│   ├── intercore/           # 核间通信（TCP/RJ45）
│   ├── data-processing/     # 数据处理（接口预留 Phase 1）
│   └── strategy-engine/      # 本地策略引擎（接口预留 Phase 1）
└── tests/                  # 集成测试
```

## 开发状态

- **Phase 1** 设计和 PRD 已通过评审（`docs/superpowers/specs/`）
- 源码尚未创建（项目初始化阶段）
- 技术债清单见 `docs/technical-debt.md`

## 开发命令

> 注：Phase 1 阶段源码尚未创建，以下为项目初始化后的占位命令。

- 构建：`cargo build --release`
- 测试：`cargo test`
- 代码检查：`cargo clippy`
- 单个测试：`cargo test <test_name>`
- 格式化：`cargo fmt`

## 开发工作流（强制）

所有任务必须完成评审才能进入下一阶段，未完成评审禁止推进。

| 阶段 | 评审 | 标记 |
|------|------|------|
| 需求阶段 | 需求评审 | `[REVIEWED: PASS]` |
| 设计阶段 | 设计评审 | `[DESIGN_APPROVED]` |
| 开发阶段 | 代码评审 | `[CODE_REVIEWED: PASS]` |
| 测试阶段 | 测试评审 | `[TEST_PASSED]` |

流程定义：`AI_WORKFLOW/02_WORKFLOW.md`
AI Agent 角色定义：`AI_WORKFLOW/01_AGENTS.md`（包含需求分析师、架构师、开发者、代码评审员等角色职责）
AI 协作 prompt 模板：`prompts/` 目录

## 约束

1. 全部使用中文进行回复
2. 修改文件前需要先描述方案，等同意再动手
3. 需求不清晰时请先提问澄清
4. **强制评审**：未完成评审禁止推进下一阶段，违规者代码回退并重新走评审流程

## 核心架构

### 架构模式

```
调度主站 ←→ gateway (IEC 104) ←→ data-processing ←→ strategy-engine
                                              ↓
                              intercore (TCP/RJ45) ←→ 实时控制模块
```

### 关键组件

| 模块 | 职责 |
|------|------|
| **common** | 日志（tracing）、统一错误类型、通用工具 |
| **gateway** | 北向 IEC 104 协议通信、连接管理、数据收发 |
| **intercore** | TCP 网络通信、指令下发、数据读取、心跳/看门狗 |
| **data-processing** | 遥测数据采集、高频上报（≥1Hz）、故障录波（接口预留） |
| **strategy-engine** | 兜底策略（削峰填谷、需量控制、防逆流），AI 指令安全校验（接口预留） |

### 核间通信

与实时控制模块通过 **TCP Socket (RJ45)** 交互。

**关键信号（通过 TCP 帧传输）：**
- `ai_ready`：AI 引擎可用状态
- `strategy_mode`：当前策略模式（基础/智能/兜底）
- `control_cmd`：下发给实时控制模块的指令

## 重构验证

代码变更后，必须按以下清单验证：

**编译与测试**
- [ ] `cargo build` 编译成功
- [ ] `cargo clippy` 无警告
- [ ] `cargo test` 所有测试通过
- [ ] `cargo fmt` 格式化通过

**功能回归**（根据变更模块选择验证）
- 通信网关：IEC 104/IEC 61850/MQTT 连接建立、协议转换数据一致性
- 数据处理：遥测数据上送频率 ≥1Hz、故障录波触发
- 策略引擎：削峰填谷、需量控制、防逆流策略
- 核间通信：`ai_ready`、`strategy_mode`、`control_cmd` 信号

**安全验证**
- 无硬编码密钥（检查 SM2/SM4 密钥残留）
- 无新增 `unsafe` 块
- 错误类型实现 `std::error::Error`

## 技术债

Phase 1 已识别的技术债（记录于 `docs/technical-debt.md`）：
- data-processing 和 strategy-engine 仅定义接口，实现延后
- IEC 61850-7-420、MQTT over TLS、国密 SM2/SM4（Phase 2+）
- AI 优化引擎（LSTM/TCN、MADDPG/PPO）（Phase 2+）
- 南向通信（RS485/HPLC）、OTA 升级、安全启动（Phase 2+）