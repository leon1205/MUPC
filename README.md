# MUPC — 微电网特种调控装置通信管理模块

[![Rust](https://img.shields.io/badge/rust-%3E%3D1.75-orange)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Linux%20(openEuler)%20%7C%20RK3588-lightgrey)]()

MUPC（Microgrid Universal Power Controller）通信管理模块是"异构双核心模块主控架构"中的**非实时处理核心**（大脑），与"实时控制核心模块"（小脑）协同工作，实现微电网的智能调度与优化运行。

---

## 核心职责

| 职责 | 说明 |
|------|------|
| **北向通信** | 与调度主站（IEC 104）、配电自动化（IEC 61850）、物联平台（MQTT）通信 |
| **南向通信** | 与台区设备（TTU、光伏逆变器、充电桩、柔性负荷）通信 |
| **本地策略引擎** | 削峰填谷、需量控制、防逆流 — AI 失效时的兜底保障 |
| **AI 边缘优化引擎** | LSTM 时序预测 + MADDPG/PPO 强化学习决策 + RK3588 NPU 推理 |
| **OTA 升级** | 固件与 AI 模型的远程更新与版本管理 |

---

## 技术栈

| 项目 | 选型 |
|------|------|
| **编程语言** | Rust >= 1.75 |
| **异步运行时** | Tokio |
| **网络框架** | Tower + Axum |
| **AI 推理** | RKNN Runtime (RK3588 NPU, 6 TOPS) |
| **目标平台** | Linux (openEuler 22.03+), ARM64 |
| **硬件** | Rockchip RK3588 |
| **许可证** | MIT |

---

## 项目结构

```
mupc/
├── Cargo.toml                   # Workspace 配置 (20 crates)
├── crates/
│   ├── common/                  # 公共库：日志 (tracing)、统一错误类型
│   ├── core/                    # 核心组件
│   ├── gateway/                 # 北向通信网关 (IEC 104)
│   ├── iec61850-plugin/         # IEC 61850 协议插件
│   ├── mqtt-plugin/             # MQTT 协议插件
│   ├── data-processing/         # 遥测数据采集与处理
│   ├── strategy-engine/         # 本地策略引擎 + AI 集成门面
│   ├── ai-engine/               # AI 优化引擎 (LSTM/MADDPG/PPO/RKNN)
│   ├── intercore/               # 核间通信 (TCP/RJ45)
│   ├── security/                # 安全模块 (SM2/SM4 国密, 审计)
│   ├── web-api/                 # Web 管理 API (Axum)
│   ├── rs485-plugin/            # RS485 通信插件
│   ├── hplc-plugin/             # HPLC 通信插件
│   ├── device-trait/            # 设备特性抽象层
│   ├── plugin-loader/           # 动态插件加载器
│   ├── mqtt-bridge/             # MQTT 桥接
│   ├── wireless/                # 本地无线通信 (WiFi/BLE/NearLink)
│   ├── ota-update/              # OTA 固件升级
│   ├── system-monitor/          # 系统监控
│   └── storage/                 # 持久化存储
├── tests/                       # 集成测试
└── docs/                        # 项目文档
```

---

## 快速开始

### 环境要求

- Rust >= 1.75（推荐使用 [rustup](https://rustup.rs) 管理）
- Linux (openEuler 22.03+) 或 Windows 10+（开发调试）
- RK3588 硬件（生产部署）

### 构建

```bash
cd mupc
cargo build --release
```

### 测试

```bash
# 全部测试
cargo test

# 单个 crate 测试
cargo test -p mupc-ai-engine

# 带输出
cargo test -- --nocapture
```

### 代码质量

```bash
cargo fmt                    # 格式化
cargo clippy                 # 静态检查
cargo build --release        # 发布构建
```

---

## 架构概览

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
| 北向 ↑ | gateway → data-processing → strategy-engine | 调度数据处理与上送 |
| 南向 ↓ | strategy-engine → rs485-plugin/hplc-plugin | 设备控制指令下发 |
| 核间 ↔ | intercore (TCP/RJ45) | 与实时控制模块数据交换 |
| AI → | strategy-engine ← ai-engine | LSTM 预测 + RL 决策 |

---

## 开发状态

| Phase | 内容 | 状态 |
|-------|------|------|
| Phase 1 | 核心架构（gateway、intercore、data-processing、strategy-engine） | ✅ 完成 |
| Phase 3C | AI 优化引擎（LSTM 预测、MADDPG/PPO 决策、RKNN Runtime 推理） | ✅ 完成 |
| Phase 2+ | IEC 61850-7-420、MQTT over TLS、SM2/SM4 国密 | 规划中 |
| Phase 2+ | 南向通信（RS485/HPLC）、OTA 升级、安全启动 | 规划中 |

技术债详见 [`docs/technical-debt.md`](docs/technical-debt.md)

---

## 文档体系

本项目采用**二级文档结构**，详见 [`CLAUDE.md`](CLAUDE.md) 中的文档管理原则。

| 层级 | 路径 | 说明 |
|------|------|------|
| **项目主文档** | `docs/superpowers/specs/PROJECT-MUPC-项目需求主文档.md` | 项目级需求入口 |
| | `docs/superpowers/plans/PROJECT-MUPC-项目设计主文档.md` | 项目级设计入口 |
| **模块文档** | `docs/superpowers/specs/modules/` | 10 个模块的详细 PRD |
| | `docs/superpowers/plans/modules/` | 10 个模块的详细设计 |
| **历史报告** | `docs/superpowers/reports/` | 审查报告、交付报告（归档） |

---

## AI 协作开发

本项目配置了一套 AI Agent 协作框架。当需要开发新功能或修复 Bug 时，直接向 AI 助手描述需求，项目经理 Agent 将自动调度需求分析师、架构师、开发工程师等角色，按"合同与路径驱动"流程完成交付。

完整工作流定义见 [`CLAUDE.md`](CLAUDE.md) 和 `/.claude/agents/` 目录。

---

## 命名约定

- 大部分 crate 使用 `mupc-` 前缀（如 `mupc-common`、`mupc-ai-engine`）
- 无前缀的 crate：`device-trait`、`plugin-loader`、`rs485-plugin`、`hplc-plugin`
- **`mqtt-bridge`**：目录名为 `mqtt-bridge`，Cargo.toml name = `mupc_mqtt_bridge`（下划线），在 `Cargo.toml` 依赖中引用时必须用下划线

---

## 许可证

MIT © MUPC Team
