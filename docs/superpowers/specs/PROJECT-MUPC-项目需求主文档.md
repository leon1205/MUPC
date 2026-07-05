# MUPC 微电网特种调控装置通信管理模块 — 项目需求主文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 项目经理 | **[REVIEWED: PASS]** |
| v3.1 | 2026-07-05 | LEON | **[REVIEWED: PASS]** — v3.1 构建体系/部署/子系统初始化更新 |

---

## 1. 项目概述

MUPC 微电网特种调控装置通信管理模块是"异构双核心模块主控架构"中的**非实时处理核心**（大脑），与"实时控制核心模块"（小脑）协同工作。

### 核心职责

- **北向通信**：与调度主站（IEC 104）、配电自动化（IEC 61850）、物联平台（MQTT）通信
- **南向通信**：与台区设备（TTU、光伏逆变器、充电桩、柔性负荷）通信
- **本地策略引擎**（AI 失效时的兜底）
- **AI 边缘优化引擎**（预测、强化学习决策）
- **OTA 升级**与远程维护

### 目标平台

| 项目 | 要求 |
|------|------|
| 硬件 | RK3588 (NPU: 6 TOPS) |
| 操作系统 | Linux (openEuler) |
| 编程语言 | Rust >= 1.75 |
| 异步运行时 | Tokio |
| 网络框架 | Tower + tokio-net |

---

## 2. 构建与部署

### 2.1 构建方式

| 方式 | 命令 | 适用场景 |
|------|------|---------|
| Cargo 本机 | `cargo build -p mupc-core-bin --release` | x86_64 开发 |
| Cargo 交叉 | `cargo build --target aarch64-unknown-linux-gnu` | ARM64 交叉编译 |
| CMake 编排 | `cmake -B build && cmake --build build` | CI/CD |
| 一键脚本 | `./deploy/scripts/build-for-rk3588.sh --cross` | 开发者 |

### 2.2 外部依赖

| 依赖 | 用途 | 自动安装 |
|------|------|:--:|
| `gcc-aarch64-linux-gnu` | ARM64 交叉编译器 | `scripts/setup-deps.sh` |
| `external/openssl-4.0.1` | SSL/TLS ARM64 静态库 | `scripts/setup-deps.sh --all` |
| `external/liblzma-master` | XZ 压缩 ARM64 库 | `scripts/setup-deps.sh --all` |
| `rknn-toolkit2-2.3.2` | RK3588 NPU 运行时 | 手动下载 + 解压 |

### 2.3 部署方式

- **一键脚本**: `./deploy/scripts/deploy.sh <target_ip> --full`
- **systemd 服务**: `deploy/systemd/mupcd.service`
- **部署文档**: `deploy/deploy.md`

## 3. 模块需求索引

本主文档为 MUPC 项目的需求入口。每个模块的详细需求请参见对应的模块需求文档。

| 编号 | 模块名称 | 对应 Crate | 模块 PRD | 状态 |
|------|----------|-----------|---------|------|
| 01 | 通信网关（北向） | gateway, iec61850-plugin, mqtt-plugin | [01-MUPC-通信网关-PRD.md](modules/01-MUPC-通信网关-PRD.md) | [REVIEWED: PASS] |
| 02 | 南向通信 | rs485-plugin, hplc-plugin, device-trait | [02-MUPC-南向通信-PRD.md](modules/02-MUPC-南向通信-PRD.md) | [REVIEWED: PASS] |
| 03 | 数据处理与存储 | data-processing, storage | [03-MUPC-数据处理与存储-PRD.md](modules/03-MUPC-数据处理与存储-PRD.md) | [REVIEWED: PASS] |
| 04 | 策略引擎 | strategy-engine | [04-MUPC-策略引擎-PRD.md](modules/04-MUPC-策略引擎-PRD.md) | [REVIEWED: PASS] |
| 05 | AI 优化引擎 | ai-engine | [05-MUPC-AI引擎-PRD.md](modules/05-MUPC-AI引擎-PRD.md) | v2.0 |
| 06 | 安全模块 | security | [06-MUPC-安全-PRD.md](modules/06-MUPC-安全-PRD.md) | [REVIEWED: PASS] |
| 07 | OTA 与系统可靠性 | ota-update, system-monitor | [07-MUPC-OTA与系统可靠性-PRD.md](modules/07-MUPC-OTA与系统可靠性-PRD.md) | [REVIEWED: PASS] |
| 08 | Web 管理与 AI 可视化 | web-api | [08-MUPC-Web管理与AI可视化-PRD.md](modules/08-MUPC-Web管理与AI可视化-PRD.md) | v1.1 |
| 09 | 本地运维通信 | wireless | [09-MUPC-本地运维通信-PRD.md](modules/09-MUPC-本地运维通信-PRD.md) | 草稿 |
| 10 | 核间通信 | intercore | [10-MUPC-核间通信-PRD.md](modules/10-MUPC-核间通信-PRD.md) | v1.0 |
| 11 | 主控进程 | mupc-core-bin | — | v3.1 |

---

## 4. 跨模块交互

### 4.1 数据流

```
调度主站 ←→ 01-通信网关 (IEC 104) ←→ 03-数据处理 ←→ 04-策略引擎
                                              ↓              ↑
                              10-核间通信 (TCP/RJ45) ←→ 实时控制模块
                                              ↑
南向设备 ←→ 02-南向通信 ←─── ProtocolHandler 注入
```

### 4.2 关键跨模块接口

| 接口 | 生产方 | 消费方 | 说明 |
|------|--------|--------|------|
| 控制指令下发 | 04-策略引擎 | 02-南向通信 | 策略决策 → 设备控制 |
| AI 决策输入 | 03-数据处理 | 05-AI引擎 | 融合数据供 AI 推理 |
| AI 决策输出 | 05-AI引擎 | 04-策略引擎 | AI 决策经安全校验后执行 |
| 核间通信 | 10-核间通信 | 03-数据处理 | 与实时控制模块数据交换 |
| 运行模式切换 | 01-通信网关 / 08-Web管理 | 05-AI引擎 | 远程/本地切换运行场景 |
| **子系统编排** | **11-主控进程** | **全部** | **14 步依赖顺序初始化，级联清理** |

### 4.3 文档更新规则

1. **新功能需求** → 写入对应模块 PRD，在本主文档第 4 章记录变更
2. **跨模块需求** → 在本主文档第 3 章描述交互，各模块 PRD 描述自身部分
3. **删除功能** → 在对应模块 PRD 标注废弃，本主文档第 4 章记录
4. **所有变更** → 模块 PRD 版本号递增，本主文档同步更新模块状态

---

## 5. 变更记录

| 日期 | 版本 | 变更内容 |
|------|------|----------|
| 2026-07-05 | v3.1 | 新增构建与部署章节；新增模块 11 主控进程；跨模块接口新增子系统编排 |
| 2026-05-29 | v1.0 | 文档体系重构：建立主文档+模块文档二级结构，删除 29 份重复文档 |

---

**文档状态：** **[REVIEWED: PASS]**

**文档管理原则：**
- 项目级需求 → 本主文档
- 模块级需求 → `modules/XX-MUPC-模块名-PRD.md`
- 不再创建独立功能的 PRD，所有需求在模块文档内更新
- 设计文档遵循同样原则：`plans/PROJECT-MUPC-项目设计主文档.md` + `plans/modules/XX-MUPC-模块名-设计文档.md`
