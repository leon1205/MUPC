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
| **AI 边缘优化引擎** | LSTM 分位数预测 + MADDPG/PPO 强化学习决策 + RK3588 NPU 推理 + 自适应权重优化器 + SafetyOverride 安全覆盖（v2.15：2 维动作空间 p_ref + k_droop） |
| **OTA 升级** | 固件与 AI 模型的远程更新与版本管理 |

---

## 技术栈

| 项目 | 选型 |
|------|------|
| **编程语言** | Rust >= 1.75 |
| **异步运行时** | Tokio |
| **网络框架** | Tower + Axum |
| **AI 推理** | RKNN Runtime (RK3588 NPU, 6 TOPS) + LSTM 分位数预测（v2.11）+ 奖励函数精细化（v2.13） |
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
                                              ↓
南向设备 ←→ rs485-plugin/hplc-plugin ←─── ProtocolHandler 注入
```

### 数据流

| 方向 | 组件 | 说明 |
|------|------|------|
| 北向 ↑ | gateway → data-processing → strategy-engine | 调度数据处理与上送 |
| 南向 ↓ | strategy-engine → rs485-plugin/hplc-plugin | 设备控制指令下发（pv_limit、load_shedding） |
| 核间 ↔ | intercore (TCP/RJ45) | 与实时控制模块数据交换（p_ref、k_droop） |
| AI → | strategy-engine ← ai-engine | LSTM 预测 + RL 决策（2 维动作空间：p_ref + k_droop） |

### 核间通信指令（实时控制模块）

| 指令 | 方向 | 说明 |
|------|------|------|
| `p_ref` | AI → 实时 | 有功基准点 (kW)，用于下垂控制公式 |
| `k_droop` | AI → 实时 | 下垂系数 (kW/V)，用于下垂控制公式 |
| `ai_ready` | AI → 实时 | AI 引擎就绪状态 |
| `strategy_mode` | AI → 实时 | 当前策略模式 |
| `q_realtime_margin` | 实时 → AI | 实时模块无功裕度 [0,1]（DataUpload 帧） |
| `voltage_phase_*` | 实时 → AI | 三相电压标幺值（DataUpload 帧） |
| `SafetyOverride` | 实时 → AI | 安全覆盖触发事件（v2.10） |

### 南向设备指令

| 指令 | 目标设备 | 说明 |
|------|----------|------|
| `pv_limit` | 光伏逆变器 | 光伏限功率比例 [0.0, 1.0] |
| `load_shedding` | 负荷控制装置 | 可中断负荷切除量 (kW) |

---

## 开发状态

| Phase | 内容 | 状态 |
|-------|------|------|
| Phase 1 | 核心架构（gateway、intercore、data-processing、strategy-engine） | ✅ 完成 |
| Phase 2A | 南向通信（RS485/HPLC）核心架构 | ✅ 基本完成（4 个测试待修） |
| Phase 2B | MQTT over TLS | ✅ 完成 |
| Phase 2B | SM2/SM4 国密 | ⚠️ SM3/SM4 CBC 真国密；SM2签名/SM4 GCM 待 gmsm 0.14 |
| Phase 3C | AI 优化引擎（LSTM、MADDPG/PPO、RKNN Runtime） | ✅ 完成 |
| Phase 3C 补充 | 跨项目动态配置系统 v2.6 | ✅ 完成 |
| v2.7 ~ v2.9 | 双参数下垂控制、P-Q 协同度奖励、RobustnessManager 应急策略 | ✅ 完成 |
| v2.10 | 安全增强（SafetyOverride 帧 0x0040 + q_realtime_margin 数据通道） | ✅ 完成 |
| v2.11 | 自适应权重优化器（NSGA-II）+ LSTM 分位数预测（P10/P50/P90） | ✅ 完成 |
| v2.12 | 奖励函数 R-01~R-07（标准化、塑造奖励、SOC均衡、过载分段、动态权重） | ✅ 完成（R-06 在 v2.13 重构为冲击负荷预备度奖励） |
| v2.13 | 奖励函数精细化（Sigmoid平滑、动态归一化、状态改善率、PER+KL、策略混合） | ✅ 完成 |
| v2.14 | SafetyOverride 惩罚重构、FusedSystemState 扩展至 78 维 | ✅ 完成 |
| v2.15 | 动作空间精简 5维→2维（p_ref + k_droop），load_shedding/pv_limit 下沉策略引擎 | ✅ 完成 |
| Phase 2+ | IEC 61850-7-420（libIEC61850 FFI 待接入） | ⚠️ 骨架就位 |
| Phase 2+ | OTA 固件升级（A/B 分区待实现）、安全启动（存根） | ⚠️ 模型OTA完成 |
| Phase 2+ | WiFi/NearLink/BLE 驱动、RBAC 鉴权中间件 | 📋 规划中 |

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
- **`storage`**：目录名为 `storage`，Cargo.toml name = `mupc_storage`（下划线），在 `Cargo.toml` 依赖中引用时必须用 `mupc_storage`

---

## 许可证

MIT © MUPC Team
