# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

MUPC 微电网特种调控装置通信管理模块是"异构双核心模块主控架构"中的**非实时处理核心**（大脑），与"实时控制核心模块"（小脑）协同工作。

**核心职责：**

- 北向通信：与调度主站（IEC 104）、配电自动化（IEC 61850）、物联平台（MQTT）通信
- 南向通信：与台区设备（TTU、光伏逆变器、充电桩、柔性负荷）通信
- 本地策略引擎（台区储能治理）——**2026-09-09 起为唯一默认下发引擎**（AI 暂停期唯一出口）
- AI 边缘优化引擎（预测、强化学习决策）——**框架保留、引擎停用**（2026-09-09「平台目标调整」）
- 本地显示终端（触摸式本地 HMI，12 号模块）
- OTA 升级与远程维护

**目标平台：** Linux (openEuler 22.03 / Ubuntu 20.04+)、RK3588 硬件（主控 / AI 推理）；BECG-3568（RK3568）用于本地显示 / 南向站级
**编程语言：** Rust
**Rust 版本：** >= 1.88（交叉编译）；>= 1.75（本机）。workspace `rust-version = "1.75"`（`mupc/Cargo.toml`）
**异步运行时：** Tokio
**网络框架：** Axum 0.7（**仅本地 HMI 控制通道**，`mupc-core-bin/src/console_host.rs`）；Web 访问栈（Tower / tower-http / hyper / hyper-util）随 `web-api` crate 删除

## 项目结构

```
mupc/
├── Cargo.toml              # Workspace 配置（26 个成员，含 crates/local-display/lvgl-sys 子 crate）
├── config/
│   └── mupc_env_config.yaml # AI 引擎动态配置（v2.6，对齐训练管线；引擎停用期间不加载）
├── crates/
│   ├── common/             # 公共库：日志、错误类型
│   ├── core/               # 核心组件（含 message_bus 进程内总线）
│   ├── gateway/            # 北向通信网关（IEC 104）
│   ├── intercore/          # 核间通信（TCP/RJ45）—— 仅核间帧协议（PCS 语义面已迁出）
│   ├── data-processing/    # 遥测数据采集
│   ├── strategy-engine/    # 本地策略引擎（台区储能治理）+ AI 集成
│   ├── ai-engine/          # AI 优化引擎（LSTM/MADDPG/RKNN Runtime）—— 框架保留、引擎停用
│   │   └── src/
│   │       ├── safety_config.rs      # 安全约束配置（v2.6 新增）
│   │       ├── env_config.rs         # YAML 配置结构（v2.6 新增）
│   │       ├── dynamic_config_loader.rs  # 动态配置加载器（v2.6 新增）
│   │       ├── action_space.rs       # 动作空间配置（v2.6 扩展）
│   │       └── ...
│   ├── security/           # 安全模块（国密只留框架，审计）
│   ├── mupc-southd/        # 站级南向调度 + PCS 通信与控制（bin: pcs_slave）
│   ├── mupc-io/            # 数字 IO 抽象（BECG-3568 DI/DO，sysfs）
│   ├── display-proto/      # 12 号显示终端跨进程契约（DisplayFrame v3）
│   ├── local-display/      # 12 号本地显示终端渲染端（bin: mupc-local-display）
│   │   └── lvgl-sys/       # LVGL C 库 FFI 薄层（bindgen + allowlist）
│   ├── storage/            # 持久化存储（SQLite/sqlx）
│   ├── ota-update/         # OTA 固件/模型升级
│   ├── system-monitor/     # 系统资源监控
│   ├── wireless/           # 无线通信（WiFi/ECDH 密钥协商）
│   ├── plugin-loader/      # 插件加载器
│   ├── iec61850-plugin/    # IEC 61850 协议插件
│   ├── mqtt-plugin/        # MQTT 协议插件
│   ├── rs485-plugin/       # RS485 通信插件
│   ├── hplc-plugin/        # HPLC 通信插件
│   ├── mqtt-bridge/        # MQTT 桥接（外设数据上云）
│   ├── sim-bridge/         # 仿真桥接代理（HIL 测试，bin: mupc-sim-bridge）
│   ├── mupc-core-bin/      # 主控进程入口 (mupcd)
│   └── device-trait/       # 设备特性抽象
├── cmake/                  # CMake 模块 (FindRKNN, toolchain)
├── deploy/                 # 部署配置 (systemd, udev, 启停脚本, local-display.md)
├── docker/                 # Docker 交叉编译环境
├── vendor/                 # 第三方库 (librknnrt.so)
└── tests/                  # 集成测试
```

> **已删除**：`web-api` crate（Web API / Axum REST + SSE）不在 workspace `members` 内，目录已不存在；Web 访问机制取消，需求并入 **12 号本地显示终端**（08 号模块标 SUPERSEDED）。
> `sim-env/`（Python 仿真引擎 Grid2Op）位于**仓库根**，不在 `mupc/` 下。

## 开发状态

- **Phase 1**: 核心架构完成
- **Phase 2A**: 南向通信（RS485/HPLC）核心架构已完成，剩余 4 个单元测试待修复
- **Phase 2B**: MQTT over TLS 已完成；**国密只留框架**（`security/Cargo.toml` 注明 framework-only，2026-09-09）；真实依赖 `gmsm 0.1.0`（**非 0.14**），SM3/SM4-CBC 为真国密，SM2 签名 / SM4-GCM / HKDF / ECDH 未实现，现由 `ring`（国际算法）兜底，**不可作国密合规交付**
- **Phase 3C**: AI 优化引擎已完成（LSTM 预测、MADDPG/PPO 决策、RKNN Runtime 推理）——**2026-09-09 起引擎停用**（模型不加载、观测空间停采；框架保留，`npu` 仍为显式开关）
- **Phase 3C 补充**: 跨项目动态配置系统 v2.6（YAML 配置加载、分层加载、版本指纹校验）
- **v2.14**: SafetyOverride 奖励函数重构、FusedSystemState 扩展至 78 维（统一版 PRD 已发布）
- **v2.15**: 动作空间精简 5维→2维（p_ref + k_droop），load_shedding/pv_limit 下沉至策略引擎本地兜底
- **v3.0**: LSTM 输入升级为 7 维多特征（HistorySample），PredictionPipeline 预测增强管线（VMD+Attention+BiLSTM+误差修正），MSSA 超参自动优化工具
- **v3.1**: 双边审计 P0/P1 修复（观测 MinMax 归一化、动作 2 维反归一化、训练-部署 Gap 消除）；在线微调 PER/KL/影子模型/渐进式切换集成完成；全项目需求-设计-实现三方审计通过（21 差异中 18 修复）；aarch64 交叉编译体系搭建（CMake + build.rs RKNN 自动检测 + Docker）
- **Phase 2+**: IEC 61850-7-420（libIEC61850 FFI 待接入）、OTA 固件升级（A/B 分区待实现）、安全启动（硬件信任根待适配）、BLE/NearLink/WiFi 无线驱动（NoOp 占位）
- **2026-09 平台目标调整**：AI 引擎彻底停用（本地策略唯一默认）+ 国密 / 安全启动只留框架；依据 `docs/superpowers/plans/archive/2026-09-09-平台目标调整-AI引擎停用与国密框架化.md`
- **2026-09 PCS 迁入南向**：PCS 通信与控制整体迁入 `mupc-southd`（02 号设计 §13 / ADR-014·015·016）；`intercore` 收敛为纯核间 TCP 帧协议。设计 §13 **未获门禁标记**
- 技术债清单见 `docs/technical-debt.md`（v2.0，含 2026-06-15 文档-代码一致性审计结果）
- 全项目审计报告见 `docs/TODO/全项目需求-设计-实现三方差异审计报告-2026-06-26.md`
- AI 引擎审计报告见 `docs/TODO/需求-设计-实现三方差异审计报告-2026-06-26.md`
- MUPC-AI2 改造要求见 `docs/TODO/` 目录（7 份吸收方案 + 在线微调闭环对接 + MUPC 推理侧改造清单）

## 开发命令

- 所有 cargo 命令必须在 `mupc/` 目录下执行
- 详细构建指南见 `mupc/build.md`

```bash
# === 本机开发 (x86_64, 无 NPU) ===
cargo build -p mupc-core-bin --release
# 三个 --exclude 仍必要：这 3 个 crate 各有预存失败用例，见「已知测试失败」（2026-09-27 复核仍失败）
cargo test --workspace --exclude mupc-iec61850-plugin --exclude rs485-plugin --exclude device-trait
cargo clippy --workspace
cargo fmt --all

# === ARM64 交叉编译 (需 aarch64-linux-gnu 工具链) ===
# 前置: sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
#       rustup target add aarch64-unknown-linux-gnu
#       export OPENSSL_DIR=/work/MUPC/external/openssl-4.0.1/aarch64-install

export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
# ⚠️ ARM64 **必须显式 --features npu**（npu 是显式开关；漏带 ⇒ 构建成功但产物是 stub）
cargo build --workspace --release --features npu --target aarch64-unknown-linux-gnu \
  --exclude mupc-iec61850-plugin --exclude device-trait

# === CMake 构建 ===
cmake -B build -DRKNN_SDK_ROOT=/work/MUPC/rknn-toolkit2-2.3.2
cmake --build build

# === 一键构建脚本 ===
./deploy/scripts/build-for-rk3588.sh --cross

# === 单个 crate ===
cargo build -p <crate>
cargo test -p <crate> <test_name>
```

### npu feature 行为

`npu` 是**显式开关**（`mupc-ai-engine` 的 `default = []`，且 `mupc-core-bin` /
`mupc-ota-update` / `mupc-strategy-engine` 三个依赖方都是 `default-features = false`）
—— 不开就是 stub，开了才按目标架构决定真实链接：

| 场景 | 命令 | 实际链接 |
|------|------|------|
| **部署（aarch64）** | `--features npu`（`build-for-rk3588.sh` / CMake `ENABLE_NPU` / CI 的 build job 都已带） | 真实 `librknnrt.so`。**缺库 = 构建失败，但失败点在链接期**：`build.rs` 只**警告**「未找到 …跳过 SHA256 校验」就返回，真正的硬失败是随后链接报 `cannot find -lrknnrt`（排查时别去 build.rs 的输出里找 panic） |
| aarch64 漏了开关 | 无 `--features npu` | stub —— build.rs 会显式警告"部署构建请加 `--features npu`" |
| 本机开发 / CI lint+test（x86_64） | 默认 | stub（FFI 返回 -1；Rockchip 不发布 x86_64 的 `librknnrt.so`，真 FFI 仅 aarch64 编译） |
| Windows（开发） | 默认 | stub |

> 为什么必须这样接线（2026-09-19）：`cargo --workspace` 类命令会把**每个成员自身的
> default features** 打开（与依赖方是否 `default-features = false` 无关），而
> `#[link(name = "rknnrt")]` 原先只按 `target_os = "linux"` 判定 ⇒ CI 的 lint/test job
> （x86_64）会去链接 aarch64 的 .so 而**结构性不可跑**（`is incompatible with elf64-x86-64`）。

### 仿真测试环境

```bash
# 一键部署 + 启动仿真
./deploy/scripts/deploy-sim.sh 192.168.3.118 --build --generate-data --start

# 仅编译 sim-bridge
cargo build -p mupc-sim-bridge --release

# 仅部署 (不启动)
./deploy/scripts/deploy-sim.sh 192.168.3.118 --build
```

| 文档 | 说明 |
|------|------|
| `docs/superpowers/specs/modules/11-MUPC-仿真测试环境-PRD.md` | 仿真环境 PRD `[REVIEWED: PASS]` |
| `docs/superpowers/plans/modules/11-MUPC-仿真测试环境-设计文档.md` | 仿真环境设计 `[DESIGN_APPROVED]` |

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
- 无前缀的 crate：`device-trait`、`plugin-loader`、`rs485-plugin`、`hplc-plugin`、`display-proto`、`local-display`
- **`mqtt-bridge`**：目录名为 `mqtt-bridge`，Cargo.toml name = `mupc_mqtt_bridge`（下划线），依赖引用时必须用下划线
- **`storage`**：目录名为 `storage`，Cargo.toml name = `mupc_storage`（下划线），依赖引用时必须用 `mupc_storage`
- 其余目录名 ≠ package name 的（引用时必须用 package name）：`system-monitor` → `mupc_system_monitor`；`wireless` → `mupc_wireless`；`sim-bridge` → `mupc-sim-bridge`
- **`mupc-io`** / **`mupc-southd`**：目录名与 package name 一致（`mupc-io` / `mupc-southd`）；`mupc-southd` 的 bin `pcs_slave` 受 feature `pcs-slave-bin` 门控
- **已删除**：`web-api`（不在 workspace `members` 内，目录不存在）

## 约束

1. 全部使用中文进行回复
2. 修改文件前需要先描述方案（Why + How），等同意再动手
3. 需求不清晰时请先提问澄清
4. **强制评审**：未完成评审禁止推进下一阶段，违规者代码回退并重新走评审流程
5. 方案描述格式：**背景（Why）** → **方案（How）** → **改动点（What）**

## Git 提交规范（防止并行开发覆盖）

### 强制规则

1. **推送前必须 rebase**：`git pull --rebase origin master` 后确认 `cargo check --workspace` 通过再推送
2. **禁止 `git push --force`** 到 `master` 分支
3. **推送前检查编译**：`cargo check --workspace` 必须 0 errors
4. **大范围文件操作必须走 PR**：涉及 >5 个文件或 >200 行的变更，禁止直接 push 到 master

### 并行开发保护

```
开发前:          git pull --rebase origin master  ← 获取最新代码
开发中:          在自己的分支上工作
推送前:          git pull --rebase origin master  ← 再次同步，解决冲突
                cargo check --workspace          ← 确认编译通过
                git push origin master           ← 推送
```

### 常见违规与后果

| 违规 | 后果 | 预防 |
|------|------|------|
| 不 rebase 直接 push | 远程旧代码覆盖本地新代码 | `git pull --rebase` 先同步 |
| 不检查编译直接 push | 推送无法编译的代码 | `cargo check --workspace` |
| 基于旧基线并行开发 | 合并时丢失他人修复 | 开发前同步 + PR review |

## Git 提交规范（2026-07-06 生效）

本次本次提交（495033b）因 `git add -u` 无差别暂存导致包含未预期的工作区旧版本文件，造成 model_manager.rs 669 行回归等 4 严重 + 5 警告问题。以下规则强制执行以杜绝此类事故。

### 暂存规则

1. **禁止** `git add -u`、`git add -A`、`git add .` 等无差别暂存命令。
2. 只允许 `git add <path> <path> ...` 逐个文件精确暂存。
3. 暂存前必须 `git diff --stat` 确认每个文件都是本次意图变更。

### 提交流程

1. `git status` → 确认工作区范围
2. `git diff --stat` → 逐文件确认变更意图
3. `git add <specific-files>` → 精确暂存
4. `git diff --cached --stat` → 再次确认暂存区范围
5. `git commit` → 提交
6. 绝对禁止 `git commit -a`

### 开始工作前检查

- 如果 `git status` 不干净（有既有未提交改动），先 `git stash` 隔离，确保本次工作基于干净的工作区。

## 核心架构

### 架构模式

```
调度主站 ←→ gateway (IEC 104) ←→ data-processing ←→ strategy-engine
                                              ↓              ↑
                              intercore (TCP/RJ45) ←→ 实时控制模块
                                              ↑
南向设备 ←→ rs485-plugin/hplc-plugin/mupc-southd ←─ ProtocolHandler 注入
                    ↑
        PCS 通信与控制（mupc-southd::pcs，2026-09-26 由 intercore 迁入）

  主控进程 (mupcd) ──display-proto(TCP 回环)──▶ local-display（12 号本地屏）
```

### 数据流

| 方向   | 组件                                          | 说明                |
| ------ | --------------------------------------------- | ------------------- |
| 北向↑ | gateway → data-processing → strategy-engine | 调度数据处理        |
| 南向↓ | strategy-engine → mupc-southd / rs485-plugin / hplc-plugin | 设备控制 |
| 核间↕ | intercore (TCP/RJ45)                          | 仅核间 TCP 帧协议（生产路径暂无消费者） |
| PCS↕  | strategy-engine ↔ mupc-southd::pcs::PcsHandle | PCS 采集 + 控制（RS485 Modbus RTU 从站） |
| 显示→ | mupcd → local-display                         | `display-proto` 帧 v3，TCP 回环；写走 Axum `/v1/console/*` |
| AI →  | strategy-engine ← ai-engine                  | **引擎停用期间不生效**（默认 `ai_engine.local_priority = true` ⇒ 本地策略优先） |

### 关键组件


| 模块                | 职责                                                             | 关键代码路径 |
| ------------------- | ---------------------------------------------------------------- | ------------ |
| **common**          | 日志（tracing）、统一错误类型、通用工具                          | `common/src/` |
| **gateway**         | 北向 IEC 104 协议通信、连接管理、数据收发                        | `gateway/src/` |
| **intercore**       | 核间 TCP 帧协议（帧类型 + 服务端 + 传输门面）；心跳/看门狗        | `intercore/src/`（PCS 语义面已迁至 `mupc-southd/src/pcs/`） |
| **mupc-southd**     | 站级南向调度（多口多从站） + PCS 通信与控制（`PcsHandle`）        | `mupc-southd/src/scheduler.rs`、`mupc-southd/src/pcs/` |
| **mupc-io**         | 数字 IO 抽象（BECG-3568 DI/DO，sysfs 后端）                       | `mupc-io/src/lib.rs` |
| **display-proto**   | 12 号跨进程契约（`DisplayFrame` v2/v3、外设段、帧预算守卫）        | `display-proto/src/frame.rs`、`display-proto/src/peripherals.rs` |
| **local-display**   | 12 号渲染端（通道状态机 + LVGL 会话 + 六页外壳）                   | `local-display/src/app.rs`、`local-display/src/channel.rs` |
| **ai-engine**       | LSTM 时序预测、MADDPG/PPO 强化学习决策、RKNN Runtime（NPU 推理）——**引擎停用** | `ai-engine/src/model_manager.rs` |
| **ai-engine**       | 动态配置加载器（YAML 分层加载、版本指纹校验、操作参数热重载）     | `ai-engine/src/dynamic_config_loader.rs` |
| **ai-engine**       | 安全约束配置（SOC 硬约束、变压器过载阈值）                      | `ai-engine/src/safety_config.rs` |
| **ai-engine**       | 环境配置结构（EnvConfig/PhysicalConfig/OperationalConfig）       | `ai-engine/src/env_config.rs` |
| **strategy-engine** | 兜底策略（台区储能治理），AI 指令安全校验          | `strategy-engine/src/ai_integration.rs` |

### 核间通信

与实时控制模块通过 **TCP Socket (RJ45)** 交互。

**现状（2026-09-26 起）**：`intercore` **仅保留核间 TCP 帧协议**（帧类型 + 服务端 + 传输门面，`intercore/src/{protocol,tcp_server,transport}.rs`）；PCS 的 `pcs` / `pcs_sim` / `modbus_rtu` 模块与 `transport::modbus` **已删除**（头注见 `mupc/crates/intercore/src/lib.rs`），该通道**在生产路径暂无消费者**。

**帧类型（`intercore/src/protocol.rs` `FrameType`）**：`Connect 0x0001` / `HeartbeatReq 0x0002` / `HeartbeatRsp 0x0003` / `ControlCmd 0x0010` / `ControlRsp 0x0011` / `StatusReport 0x0020` / `DataUpload 0x0030` / `SafetyOverride 0x0040`（v2.10）。

**PCS 侧信号（现由 `mupc-southd::pcs::PcsDualParam` 承载，`mupc-southd/src/pcs/mod.rs`）**：

- `p_ref` / `k_droop`：下垂控制双参数
- `ai_ready`：AI 引擎可用状态
- `strategy_mode`：当前策略模式

> `q_realtime_margin` / `voltage_phase_*` 属 **AI 引擎观测空间**（`ai-engine/src/data_fusion.rs` 的 `FusedSystemState`），PRD 记为「来源为核间 DataUpload 帧」（05 号 PRD:1052）；**AI 引擎停用期间观测空间停采、不生效**。

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

**数据流（AI 引擎停用期间全部不生效）：**

1. LSTM 时序预测（光伏出力/负荷）
2. RL 模型基于预测结果决策
3. AiValidator 校验 AI 指令安全性
4. ~~通过 intercore 下发给实时控制模块~~ → 现经 `mupc-southd::pcs::PcsHandle` 下发至 PCS

**现状（2026-09-09 起）**：AI 引擎**停用**——模型不加载、观测空间停采；本地策略引擎（台区储能治理）为**唯一默认下发引擎**。部署默认 `ai_engine.local_priority = true`（`mupc-core-bin/src/core_config.rs` 的 `default_local_priority()`），`AiIntegrator::local_priority` 为 `false` 的分支在暂停期不可达（`ai_integration.rs:583` 注释）。需 AI 控制时改配置后重启——**无运行时切换端点**（原 Web API 出口已随 crate 删除）。

### 本地显示终端（12 号模块）

原 **Web API 架构**（`web-api` crate，Axum REST + SSE + `RequireRole` 鉴权）**已整体删除**（不在 workspace `members` 内，08 号模块标 **SUPERSEDED**）。本地人机交互由 **12 号本地显示终端**承接：

| 组成 | Crate / 位置 | 说明 |
|------|-------------|------|
| 跨进程契约 | `display-proto` | `DisplayFrame` v2/v3、外设段、帧预算守卫常量与纯函数；mupcd（发布方）与 `mupc-local-display`（订阅方）共享 |
| 渲染端 | `local-display`（bin `mupc-local-display`） | LVGL（C，软件渲染器）+ 自研 fbdev / evdev 输入；六页 IA |
| LVGL FFI | `local-display/lvgl-sys` | bindgen + `allowlist.txt` 逐符号白名单（禁 `lv_*` 通配） |
| 读通道 | mupcd | TCP 回环 `GET /v1/display/latest`（无 TLS），`display.*` 配置段 |
| 写通道 | `mupc-core-bin/src/console_host.rs` | Axum 0.7 `/v1/console/*`（`display.control_bind_addr`）；**校验 → 幂等(request_id) → 审计(fail-closed) → 执行 → 回执** |

**权限模型（12 号 PRD §0 B5）**：**无登录 + 审计 + 二次确认**；物理在场即授权，**无 Session / PIN / RBAC**。写操作（配置保存 / 联锁释放 / M1 授权 / 任何下发）须二次确认并记审计（`ConsoleAuditService`：JSONL 追加 + SHA-256 审计链双写）。**模式切换为暂停项、本期无界面入口**。

> 目标平台 BECG-3568（RK3568），HDMI 外接 8 寸 1024×768 触摸屏，**无浏览器 / 无显示服务器（无 X11 / Wayland）**。三份文档：`specs/modules/12-MUPC-本地显示终端-PRD.md`（v2.2）、`plans/modules/12-MUPC-本地显示终端-设计文档.md`、`plans/modules/12-MUPC-本地显示终端-UI设计文档.md`。

> **历史残留已清除**：原 Web API 的 `RequireRole` 提取器（`X-Session-Id`）、`login()` 占位实现（硬编码 `role: "operator"`）、RBAC 鉴权中间件（技术债 U-01）等**均随 `web-api` crate 删除**，不在仓库内；12 号现行权限模型见上。

## 已知测试失败

以下测试为预存失败，非近期引入：

**复核记录（2026-09-27，逐 crate 实跑 `cargo test -p <crate> -j 2`）：**

| Crate | 测试 | 实测结果 | 原因 |
|-------|------|---------|------|
| device-trait | `test_modbus_crc_calculation` | ❌ FAILED（`south_device.rs:590` left=1604 right=196） | south_device 实现不完整 |
| device-trait | `test_modbus_handler_encode_decode` | ❌ FAILED（`south_device.rs:496` assertion `frame.len() > data.len() + 3`） | 同上 |
| device-trait | `test_inverter_handler_encode_decode` | ❌ FAILED（`south_device.rs:524` assertion `result.is_ok()`） | 同上 |
| rs485-plugin | `test_config_with_gpio` | ❌ FAILED（`config.rs:154` missing field `data_bits`） | 配置反序列化字段缺失 |
| mupc-iec61850-plugin | `test_parse_goose_pdu` | ❌ FAILED（`goose.rs:256` assertion `result.is_ok()`） | GOOSE PDU 解析未完成 |

> 上述 5 项于 2026-09-27 复核，**仍全部失败**，与上表描述一致（crate 内其余用例：device-trait 12 passed / rs485-plugin 68 passed / iec61850-plugin 29 passed）。

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
| 策略引擎 | 台区储能治理策略（AI 失效兜底）                      |
| 南向通信 | RS485 协议处理器、ProtocolHandler 注入、HPLC 驱动   |
| AI 引擎  | LSTM 预测 <1s、RL 决策 <1s、RKNN Runtime NPU 推理（**引擎停用期间不适用**） |
| 核间通信 | 帧协议 `FrameType` 编解码、心跳/看门狗；PCS 侧 `p_ref`/`k_droop`/`ai_ready`/`strategy_mode`（现由 `mupc-southd::pcs` 承载） |
| 本地显示 | `display-proto` 帧 v2/v3 兼容与拒帧校验、读通道拉帧、控制通道二次确认 + 审计 |

**安全验证**

- [ ]  无硬编码密钥（检查 SM2/SM4 密钥残留）
- [ ]  无新增 `unsafe` 块
- [ ]  错误类型实现 `std::error::Error`

## 技术债

完整技术债清单见 [`docs/technical-debt.md`](docs/technical-debt.md)（v2.0，2026-06-15 更新）：

| Phase | 内容 | 状态 |
|-------|------|------|
| Phase 1 | 核心架构（gateway、intercore、data-processing、strategy-engine） | ✅ 完成 |
| Phase 2A | 南向通信（RS485/HPLC）核心架构 | ✅ 基本完成（4 个测试待修） |
| Phase 2B | MQTT over TLS | ✅ 完成 |
| Phase 2B | SM2/SM4 国密 | ⚠️ **只留框架**（framework-only，2026-09-09）；真实依赖 `gmsm 0.1.0`（**非 0.14**）。SM3/SM4-CBC 真国密；SM2 签名 / SM4-GCM / HKDF / ECDH 未实现，`ring` 兜底 |
| Phase 3C | AI 优化引擎（LSTM 预测、MADDPG/PPO 决策、RKNN Runtime 推理） | ✅ 完成（**2026-09-09 起引擎停用**；框架保留） |
| Phase 3C 补充 | 跨项目动态配置系统 v2.6 | ✅ 完成 |
| v2.14 | SafetyOverride 奖励函数重构、FusedSystemState 78维 | ✅ 完成 |
| v2.15 | 动作空间精简 5→2 维（p_ref + k_droop），load_shedding/pv_limit 下沉至策略引擎 | ✅ 完成 |
| Phase 2+ | IEC 61850-7-420（libIEC61850 FFI 待接入） | ⚠️ 骨架就位 |
| Phase 2+ | OTA 固件升级（A/B 分区切换待实现）、安全启动（存根） | ⚠️ 模型OTA完成 |
| Phase 2+ | WiFi/NearLink/BLE 驱动 | 📋 规划中（RBAC 鉴权随 `web-api` 删除，不再适用） |
| Phase 2+ | Web 管理与 AI 可视化（08 号）→ 并入 12 号本地显示终端 | ⚠️ SUPERSEDED（web-api crate 已删） |
| Phase 2+ | PCS 通信与控制迁入 `mupc-southd`（02 号设计 §13 / ADR-014·015·016） | 🚧 进行中（§13 未获门禁标记） |

## 配置文件

### 运行时配置

AI 引擎配置文件位于 `mupc/config/mupc_env_config.yaml`，与训练管线对齐。

### 构建配置

| 文件 | 用途 |
|------|------|
| `.cargo/config.toml` | aarch64 linker + rustflags |
| `CMakeLists.txt` | CMake 编排 (RKNN 检测 → Cargo → 打包) |
| `cmake/FindRKNN.cmake` | RKNN SDK 自动查找模块 |
| `cmake/toolchain-aarch64-linux.cmake` | ARM64 交叉编译工具链 |
| `Cross.toml` | cross-rs 容器化编译配置 |
| `docker/Dockerfile.build` | ARM64 Docker 编译环境 |
| `build.md` | 完整构建指南（三种方式） |

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

**当前目录布局：**

| 路径 | 内容 |
|------|------|
| `docs/superpowers/specs/PROJECT-MUPC-项目需求主文档.md` | 项目需求主文档 |
| `docs/superpowers/plans/PROJECT-MUPC-项目设计主文档.md` | 项目设计主文档 |
| `docs/superpowers/specs/modules/` | 模块 PRD（01–12 共 12 份；**08 号已 SUPERSEDED**） |
| `docs/superpowers/plans/modules/` | 模块设计文档（01–12；**12 号另有 UI 设计文档**） |
| `docs/superpowers/specs/modules/archive/` | 归档 PRD（2026-09-27 建立） |
| `docs/superpowers/specs/archive/` | 归档规格 / 历史文档（2026-09-27 建立） |
| `docs/superpowers/plans/archive/` | 归档实施计划（2026-09-27 建立） |
| `docs/superpowers/reports/` | 审查报告、交付报告 |

**模块清单速查：** 01 通信网关 / 02 南向通信（含 `mupc-southd`、`mupc-io`） / 03 数据处理与存储 / 04 策略引擎 / 05 AI 引擎（引擎停用） / 06 安全（国密框架） / 07 OTA 与系统可靠性 / 08 Web 管理与 AI 可视化（**SUPERSEDED**） / 09 本地运维通信 / 10 核间通信 / 11 仿真测试环境 / 12 本地显示终端。**主控进程 `mupc-core-bin` 无模块编号、无独立 PRD。**

> ⚠️ **11 号**是**仿真测试环境**（`specs/modules/11-MUPC-仿真测试环境-PRD.md` + `plans/modules/11-MUPC-仿真测试环境-设计文档.md`），**不是**主控进程（历史主文档曾误编为 11 号）。

**强制规则：**
- 新增需求/设计 → 追加到对应模块文档内，不得创建独立文件
- 跨模块变更 → 更新项目主文档的跨模块交互章节
- 禁止独立的时间戳文件（`specs/YYYY-MM-DD-功能名-PRD.md` 格式）
- 评审通过后顶部标注 `[REVIEWED: PASS]` 或 `[DESIGN_APPROVED]`，版本号递增
