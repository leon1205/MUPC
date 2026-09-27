# MUPC 微电网特种调控装置通信管理模块 — 项目需求主文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 项目经理 | **[REVIEWED: PASS]** |
| v3.1 | 2026-07-05 | LEON | **[REVIEWED: PASS]** — v3.1 构建体系/部署/子系统初始化更新 |
| v3.2 | 2026-09-27 | LEON | 待评审 — 内容与现状对齐（08 号 SUPERSEDED / 补 12 号 / 模块编号订正 / crate 归属按 workspace 26 成员订正）；本笔为内容对齐，未经评审，故不沿用门禁标记 |
| v3.3 | 2026-09-27 | LEON | 待评审 — §4.1 数据流图与 §4.2 接口表按**数据面现状**重画/改判：采集上行改由 02-南向通信（`mupc-southd`，经装配层 `SouthSink`）承担；10-核间通信改为「保留待接」（生产路径无消费者，现存消费者只有 `sim-bridge`）；原「核间通信 → 03-数据处理」接口行标作废（`data-processing` 实测不依赖 `intercore`）。同为内容对齐，未经评审 |

> v3.2 更新依据（均可在仓库内逐条核对）：**模块编号以 `docs/superpowers/specs/modules/XX-…-PRD.md` 的文件编号为准**；crate 归属以 `mupc/Cargo.toml` 的 `members`（**26 个成员**，含 `crates/local-display/lvgl-sys` 子 crate）为准；各模块门禁标记以对应 PRD 头部原文为准。

---

## 1. 项目概述

MUPC 微电网特种调控装置通信管理模块是"异构双核心模块主控架构"中的**非实时处理核心**（大脑），与"实时控制核心模块"（小脑）协同工作。

### 核心职责

- **北向通信**：与调度主站（IEC 104）、配电自动化（IEC 61850）、物联平台（MQTT）通信
- **南向通信**：与台区设备（TTU、光伏逆变器、充电桩、柔性负荷）通信
- **本地策略引擎**（台区储能治理；**2026-09-09 起为唯一默认下发引擎**）
- **AI 边缘优化引擎**（预测、强化学习决策）——**框架保留、引擎停用**（2026-09-09「平台目标调整」，见 `docs/superpowers/plans/archive/2026-09-09-平台目标调整-AI引擎停用与国密框架化.md`）
- **OTA 升级**与远程维护
- **本地显示终端**（触摸式本地 HMI，12 号模块）

### 目标平台

| 项目 | 要求 |
|------|------|
| 硬件（主控 / AI 推理） | RK3588 (NPU: 6 TOPS) |
| 硬件（本地显示 / 南向站级） | BECG-3568（RK3568，aarch64 Linux），HDMI 外接 8 寸 1024×768 触摸屏，无显示服务器（无 X11 / Wayland）——见 `plans/modules/12-MUPC-本地显示终端-设计文档.md` 目标平台行 |
| 操作系统 | Linux（openEuler 22.03+ / Ubuntu 20.04+） |
| 编程语言 | Rust >= 1.88（交叉编译）；>= 1.75（本机）。workspace `rust-version = "1.75"`（`mupc/Cargo.toml:37`） |
| 异步运行时 | Tokio |
| 网络框架 | Axum 0.7（**仅本地 HMI 控制通道**，`mupc-core-bin/src/console_host.rs`）；Web 访问栈（Tower/tower-http/hyper/hyper-util）随 `web-api` crate 删除 |

---

## 2. 构建与部署

### 2.1 构建方式

> 以下路径均为**仓库 `mupc/` 目录下**的相对路径（脚本实际位于 `mupc/deploy/scripts/`，仓库根无 `deploy/` 目录）。

| 方式 | 命令 | 适用场景 |
|------|------|---------|
| Cargo 本机 | `cargo build -p mupc-core-bin --release` | x86_64 开发 |
| Cargo 交叉 | `cargo build --workspace --release --features npu --target aarch64-unknown-linux-gnu` | ARM64 交叉编译（**`npu` 为显式开关**） |
| CMake 编排 | `cmake -B build && cmake --build build` | CI/CD |
| 一键脚本 | `./deploy/scripts/build-for-rk3588.sh --cross` | 开发者（实测存在：`mupc/deploy/scripts/build-for-rk3588.sh`） |

### 2.2 外部依赖

| 依赖 | 用途 | 自动安装 |
|------|------|:--:|
| `gcc-aarch64-linux-gnu` | ARM64 交叉编译器 | `./scripts/setup-deps.sh`（实测存在：`mupc/scripts/setup-deps.sh`） |
| `external/openssl-4.0.1` | SSL/TLS ARM64 静态库 | `./scripts/setup-deps.sh --all` |
| `external/liblzma-master` | XZ 压缩 ARM64 库 | `./scripts/setup-deps.sh --all` |
| `rknn-toolkit2-2.3.2` | RK3588 NPU 运行时 | 手动下载 + 解压（**AI 引擎停用期间 `npu` 开关仅供框架保留**） |

### 2.3 部署方式

- **一键脚本**: `./deploy/scripts/deploy.sh <target_ip> --full`（实测存在：`mupc/deploy/scripts/deploy.sh`）
- **仿真部署**: `./deploy/scripts/deploy-sim.sh <target_ip> --build --generate-data --start`（实测存在）
- **systemd 服务**: `mupc/deploy/systemd/mupcd.service`
- **部署文档**: `mupc/deploy/deploy.md`（实测存在；仓库根无 `deploy/deploy.md`）
- **bin 产物**: `mupcd`（`mupc-core-bin`）、`mupc-local-display`（`local-display`）、`pcs_slave`（`mupc-southd`，feature `pcs-slave-bin` 门控）、`mupc-sim-bridge`（`sim-bridge`，`src/main.rs`）

## 3. 模块需求索引

本主文档为 MUPC 项目的需求入口。每个模块的详细需求请参见对应的模块需求文档。

> **编号口径**：**模块编号 = 其 PRD 文件编号**（`specs/modules/XX-…-PRD.md`）。

| 编号 | 模块名称 | 对应 Crate | 模块 PRD | 状态（按 PRD 头部门禁标记原文） |
|------|----------|-----------|---------|------|
| 01 | 通信网关（北向） | gateway, iec61850-plugin, mqtt-plugin | [01-MUPC-通信网关-PRD.md](modules/01-MUPC-通信网关-PRD.md) | `[REVIEWED: PASS]`（2026-09-23 §8 增量） |
| 02 | 南向通信 | rs485-plugin, hplc-plugin, device-trait, **mupc-southd**, **mupc-io** | [02-MUPC-南向通信-PRD.md](modules/02-MUPC-南向通信-PRD.md) | `[REVIEWED: PASS]`（2026-09-21 终审）+ `[REVIEWED: PASS]`（2026-09-23 §10） |
| 03 | 数据处理与存储 | data-processing, storage | [03-MUPC-数据处理与存储-PRD.md](modules/03-MUPC-数据处理与存储-PRD.md) | `[REVIEWED: PASS]`（2026-09-23 增量） |
| 04 | 策略引擎 | strategy-engine | [04-MUPC-策略引擎-PRD.md](modules/04-MUPC-策略引擎-PRD.md) | 无门禁标记；头部标注「2026-09-09 台区储能治理为默认下发引擎（AI 暂停期唯一出口）」 |
| 05 | AI 优化引擎 | ai-engine（**框架保留、引擎停用**） | [05-MUPC-AI引擎-PRD.md](modules/05-MUPC-AI引擎-PRD.md) | 无门禁标记；头部标注「2026-09-09 AI 引擎暂停，本地策略引擎为唯一默认下发引擎」 |
| 06 | 安全模块 | security（**国密只留框架**） | [06-MUPC-安全-PRD.md](modules/06-MUPC-安全-PRD.md) | 无门禁标记 |
| 07 | OTA 与系统可靠性 | ota-update, system-monitor | [07-MUPC-OTA与系统可靠性-PRD.md](modules/07-MUPC-OTA与系统可靠性-PRD.md) | 无门禁标记 |
| 08 | ~~Web 管理与 AI 可视化~~ | ~~web-api~~（**crate 已删除**，不在 workspace `members` 内） | [08-MUPC-Web管理与AI可视化-PRD.md](modules/08-MUPC-Web管理与AI可视化-PRD.md) | **`[SUPERSEDED: 2026-09-10]`** —— 需求并入 12 号本地显示终端；本文件不再作为实施依据 |
| 09 | 本地运维通信 | wireless | [09-MUPC-本地运维通信-PRD.md](modules/09-MUPC-本地运维通信-PRD.md) | 无门禁标记（草稿） |
| 10 | 核间通信 | intercore（**仅核间 TCP 帧协议**；PCS 语义面已于 2026-09-26 迁出） | [10-MUPC-核间通信-PRD.md](modules/10-MUPC-核间通信-PRD.md) | 无门禁标记 |
| 11 | 仿真测试环境 | sim-bridge | [11-MUPC-仿真测试环境-PRD.md](modules/11-MUPC-仿真测试环境-PRD.md) | `[REVIEWED: PASS]`（2026-07-10） |
| 12 | 本地显示终端（触摸式本地 HMI） | display-proto, local-display | [12-MUPC-本地显示终端-PRD.md](modules/12-MUPC-本地显示终端-PRD.md) | `[REVIEWED: PASS]`（2026-09-10，v2.0）+ `[REVIEWED: PASS]`（2026-09-23，v2.1 增量） |

**不带模块编号的条目（无对应 PRD，按 crate 归属登记）：**

| 条目 | 对应 Crate | 模块文档 | 状态 |
|------|-----------|---------|------|
| 主控进程 / 装配层 | mupc-core-bin（bin `mupcd`） | —（编排见 `mupc/crates/mupc-core-bin/src/startup.rs`、`console_host.rs`） | 无 PRD、无门禁标记 |

> 原 v3.1 表将「主控进程」编为 **11 号**，与 `11-MUPC-仿真测试环境-PRD.md` 的 11 号冲突；本版按「编号 = PRD 文件编号」订正，主控进程退回**不带编号**条目。

---

## 4. 跨模块交互

### 4.1 数据流

```
调度主站 ←→ 01-通信网关 (IEC 104) ←→ 03-数据处理 ←→ 04-策略引擎
                                        ▲                     │
                              采集结果  │                     │ 控制指令
                                        │                     ▼
南向设备 ←→ 02-南向通信 ──▶ PCS（= 实时控制模块）
        (rs485/hplc/device-trait/plugin-loader/mupc-southd/mupc-io)
                    │           └ mupc-southd::pcs::PcsHandle
                    │             （2026-09-26 由 intercore 迁入，走 RS485 Modbus RTU）
                    └─ 四条通路：插件化单设备 / 站级多从站调度 / PCS 通信与控制 / 数字 IO（mupc-io）

  10-核间通信（intercore，仅核间 TCP 帧协议）┈┈ 保留待接：生产路径暂无消费者（现存消费者只有 sim-bridge）

  主控进程 (mupc-core-bin / mupcd) ──display-proto(TCP 回环)──▶ 12-本地显示终端
                                    └── mqtt-bridge ──▶ 物联平台（外设数据上云）
```

### 4.2 关键跨模块接口

| 接口 | 生产方 | 消费方 | 说明 |
|------|--------|--------|------|
| 控制指令下发 | 04-策略引擎 | 02-南向通信 | 策略决策 → 设备控制 |
| AI 决策输入 | 03-数据处理 | 05-AI引擎 | 融合数据供 AI 推理（**AI 引擎停用期间观测空间停采**，见 05 号 PRD 头部） |
| AI 决策输出 | 05-AI引擎 | 04-策略引擎 | AI 决策经安全校验后执行（**AI 引擎停用期间不生效**；默认 `ai_engine.local_priority = true` ⇒ 本地策略优先） |
| ~~核间通信~~ | ~~10-核间通信~~ | ~~03-数据处理~~ | **已作废（2026-09-26/27）**：PCS（= 实时控制模块）的采集与控制现由 **02-南向通信（`mupc-southd`）** 承担，经装配层 `SouthSink` 投递至 03；`mupc-data-processing` **不依赖 `mupc-intercore`**（`Cargo.toml` 无此边）。核间 TCP 帧协议**生产路径暂无消费者**，现存真实消费者只有 11 号仿真的 `sim-bridge` |
| **PCS 通信与控制** | **02-南向通信（`mupc-southd::pcs`）** | **04-策略引擎 / 主控进程** | PCS 三相读数 / SOC 采集 + 启停 / 联锁 / 重启授权（2026-09-26 由 `intercore` 迁入；见 02 号设计 §13 / ADR-014） |
| **本地显示帧** | **主控进程（mupcd）** | **12-本地显示终端（`local-display`）** | `display-proto` 帧模型 v3，TCP 回环 `GET /v1/display/latest`（无 TLS） |
| **本地控制通道** | **12-本地显示终端** | **主控进程** | Axum `/v1/console/*`（`mupc-core-bin/src/console_host.rs`）；写操作须二次确认 + 审计 |
| 运行模式切换 | ~~01-通信网关 / 08-Web管理~~ | 05-AI引擎 | **需求已收敛**：Web 出口随 `web-api` crate 删除；12 号 PRD §0 B5 规定「模式切换=暂停项、本期无界面入口」 |
| **子系统编排** | **主控进程（`mupc-core-bin`，无模块编号）** | **全部** | **14 步依赖顺序初始化，级联清理**（`mupc/crates/mupc-core-bin/src/startup.rs:3`） |

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
| 2026-09-27 | v3.2 | 内容与现状对齐：① 模块编号口径订正为「编号 = PRD 文件编号」，「主控进程」由 11 号改为**不带编号**条目（11 号归还 `11-MUPC-仿真测试环境`）；② 08 号（Web 管理与 AI 可视化）标 **SUPERSEDED**、`web-api` crate 已删；③ 补 **12 号本地显示终端**（PRD + 设计 + UI 三份）；④ §3 各模块「状态」列按 PRD 头部门禁标记原文订正；⑤ crate 归属补 `mupc-southd` / `mupc-io` / `display-proto` / `local-display` / `sim-bridge`，按 workspace **26 成员**口径；⑥ §1 目标平台补 BECG-3568、Rust 版本与网络框架据实订正；⑦ §2 构建/部署路径订正为仓库 `mupc/` 下的真实路径；⑧ §4 数据流与接口表补 12 号、PCS 迁出、AI 停用；⑨ 核心职责补 AI「框架保留、引擎停用」与本地显示终端 |
| 2026-09-27 | v3.3 | §4.1 数据流图：上游由「10-核间通信 ←→ 实时控制模块」改为**南向采集上行**（02-南向通信 → PCS，含 `PcsHandle`），10-核间通信单列为**虚线「保留待接」**并注明生产路径无消费者、现存消费者只有 `sim-bridge`；补南向四条通路。§4.2 接口表：「核间通信 → 03-数据处理」行标**作废**（`data-processing` 实测不依赖 `intercore`） |

---

**文档状态：** **[REVIEWED: PASS]**

**文档管理原则：**
- 项目级需求 → 本主文档
- 模块级需求 → `modules/XX-MUPC-模块名-PRD.md`
- 不再创建独立功能的 PRD，所有需求在模块文档内更新
- 设计文档遵循同样原则：`plans/PROJECT-MUPC-项目设计主文档.md` + `plans/modules/XX-MUPC-模块名-设计文档.md`
