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
| **本地策略引擎** | 台区储能治理 —— **2026-09-09 起为唯一默认下发引擎**（AI 暂停期唯一出口）；原三策略（削峰填谷 / 需量控制 / 防逆流）已废弃（代码保留不编译） |
| **AI 边缘优化引擎** | LSTM 分位数预测 + MADDPG/PPO 强化学习决策 + RK3588 NPU 推理 —— **框架保留、引擎停用**（2026-09-09「平台目标调整」，模型不加载、观测空间停采） |
| **本地显示终端** | 触摸式本地 HMI（12 号模块，1024×768，LVGL）；6 页 IA；**无登录 + 审计 + 二次确认** |
| **OTA 升级与系统可靠性** | 固件与 AI 模型的远程更新与版本管理；可靠性为**三层**：**进程级**（委托 systemd `Restart=`/`StartLimitBurst`/`MemoryMax`）、**服务级**（`ServiceCoordinator` + `mupc-core-bin/src/service_health.rs`，周期 15 s、边沿告警）、**跨进程**（仅显示通道连接态） |

---

## 技术栈

| 项目 | 选型 |
|------|------|
| **编程语言** | Rust >= 1.88 (交叉编译)；>= 1.75 (本机) |
| **异步运行时** | Tokio |
| **网络框架** | Axum 0.7（**仅本地 HMI 控制通道**，`mupc-core-bin/src/console_host.rs`）。Web 访问栈（Tower / tower-http / hyper / hyper-util）随 `web-api` crate 删除 |
| **AI 推理** | RKNN Runtime v2.3.2 (RK3588 NPU, 6 TOPS) —— **引擎停用期间不加载模型**；`npu` 为**显式 feature 开关**（默认关闭） |
| **目标平台** | Linux (Ubuntu 20.04+ / openEuler 22.03+), ARM64 |
| **硬件** | **BECG-3588 BOX**（瑞芯微 RK3588 八核 4×A76+4×A55 @2.4GHz、NPU 6TOPS / LPDDR5 / HDMI 8K）；`mupcd` 与本地显示同在该板上运行。此前为 BECG-3568（RK3568），**2026-09-29 换代** |
| **许可证** | MIT |

---

## 项目结构

```
mupc/
├── Cargo.toml                   # Workspace 配置（26 个成员，含 crates/local-display/lvgl-sys 子 crate）
├── crates/
│   ├── common/                  # 公共库：日志 (tracing)、统一错误类型、消息总线
│   ├── core/                    # 核心组件
│   ├── gateway/                 # 北向通信网关 (IEC 104)
│   ├── iec61850-plugin/         # IEC 61850 协议插件
│   ├── mqtt-plugin/             # MQTT 协议插件
│   ├── mqtt-bridge/             # MQTT 桥接（外设数据上云）
│   ├── data-processing/         # 遥测数据采集与处理
│   ├── strategy-engine/         # 本地策略引擎（台区储能治理）+ AI 集成门面
│   ├── ai-engine/               # AI 优化引擎 (LSTM/MADDPG/PPO/RKNN) —— 框架保留、引擎停用
│   ├── intercore/               # 核间通信 (TCP/RJ45) —— 仅核间帧协议（PCS 语义面已迁出）；生产路径无读取方
│   ├── mupc-southd/             # 站级南向调度 + PCS 通信与控制（bin: pcs_slave）
│   ├── mupc-io/                 # 数字 IO 抽象 (BECG-3588 DI/DO, sysfs)
│   ├── security/                # 安全模块（国密只留框架，审计）
│   ├── rs485-plugin/            # RS485 通信插件
│   ├── hplc-plugin/             # HPLC 通信插件
│   ├── device-trait/            # 设备特性抽象层
│   ├── display-proto/           # 12 号显示终端跨进程契约（DisplayFrame v3）
│   ├── local-display/           # 12 号本地显示终端渲染端（bin: mupc-local-display）
│   │   └── lvgl-sys/            # LVGL C 库 FFI 薄层（bindgen + allowlist）
│   ├── plugin-loader/           # 动态插件加载器
│   ├── wireless/                # 本地无线通信 (WiFi/BLE/NearLink)
│   ├── ota-update/              # OTA 固件升级
│   ├── system-monitor/          # 系统监控
│   ├── mupc-core-bin/           # 主控进程入口 (mupcd)
│   ├── sim-bridge/              # 仿真桥接代理 (HIL 测试)
│   └── storage/                 # 持久化存储
├── cmake/                       # CMake 模块 (FindRKNN, toolchain)
├── deploy/                      # 部署配置 (systemd, 启停脚本, udev, config)
├── docker/                      # Docker 交叉编译环境
├── tests/                       # 集成测试
└── docs/                        # 项目文档
```

> **注**：`web-api` crate 已删除（不在 workspace `members` 内），Web 访问机制取消，需求并入 12 号本地显示终端。
> Python 仿真引擎（Grid2Op，`engine.py`）位于**仓库根** `sim-env/`，不在 `mupc/` 下。

---

## 快速开始

### 环境要求

- Rust >= 1.88（交叉编译）/ >= 1.75（本机）（推荐使用 [rustup](https://rustup.rs) 管理）
- Linux (Ubuntu 20.04+ / openEuler 22.03+) 或 Windows 10+（开发调试）
- **BECG-3588 BOX**（RK3588）—— `mupcd` 与本地显示的运行硬件（2026-09-29 由 BECG-3568 换代）
### 外部依赖安装

```bash
# 一键安装所有外部依赖
./scripts/setup-deps.sh --all
```

> 详细指南见 [`mupc/build.md`](mupc/build.md)，包含三种构建方式（Cargo/CMake/脚本）、
> 交叉编译工具链下载、RKNN SDK 获取方式、OpenSSL 编译说明、外部依赖矩阵。

### 本机构建

```bash
cd mupc
cargo build -p mupc-core-bin --release
```

### ARM64 交叉编译 (x86_64 → RK3588)

```bash
# 前置条件（也可用 ./scripts/setup-deps.sh --all 自动安装）
sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu

# 编译 OpenSSL（首次，setup-deps.sh 可自动完成）
cd ../external/openssl-4.0.1
./Configure linux-aarch64 --cross-compile-prefix=aarch64-linux-gnu- \
    --prefix=$(pwd)/aarch64-install no-shared
make -j$(nproc) && make install_sw

# 编译 MUPC
cd ../../mupc
export OPENSSL_DIR=../external/openssl-4.0.1/aarch64-install
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
# ⚠️ ARM64 必须显式 --features npu（npu 是显式开关；漏带 ⇒ 构建成功但产物是 stub）
cargo build --workspace --release --features npu --target aarch64-unknown-linux-gnu \
    --exclude mupc-iec61850-plugin --exclude device-trait
```

### 仿真测试环境 (HIL)

> ⚠️ **动作闭环未实现（2026-09-27 裁定：不重建）**：`deploy-sim.sh` 拉起的 sim-bridge 会
> **阻塞在 `accept()`**（MUPC 侧无 TCP 客户端），且 `mupc/sim/observation` **无订阅方**
> ⇒ 当前只产出仿真观测、**不构成闭环**。详见 11 PRD §1.4 / 11 设计 §0 /
> `docs/technical-debt.md` §6.19 U-163。协议链（PCS / 南向）的验证不走这里，
> 见 `mupc-southd/tests/pcs_e2e.rs`。

```bash
# 一键部署: PC(仿真) + 嵌入式(MUPC) 全栈
./deploy/scripts/deploy-sim.sh 192.168.3.118 --build --generate-data --start

# 仅启动仿真 (已编译)
./deploy/scripts/deploy-sim.sh 192.168.3.118 --start

# 详细文档
#   PRD:  docs/superpowers/specs/modules/11-MUPC-仿真测试环境-PRD.md
#   设计: docs/superpowers/plans/modules/11-MUPC-仿真测试环境-设计文档.md
```

产物：`target/aarch64-unknown-linux-gnu/release/mupcd`

### 测试

```bash
cargo test --workspace --exclude mupc-iec61850-plugin --exclude rs485-plugin --exclude device-trait

# 单个 crate
cargo test -p mupc-strategy-engine

# 带输出
cargo test -- --nocapture
```

### 代码质量

```bash
cargo fmt --all     # 格式化（⚠️ 见下方提示）
cargo clippy        # 静态检查
```

> ⚠️ **关于 `cargo fmt --all`（2026-09-29 实测；其中第 2 条是既存的 CI 红灯）**
>
> **背景**：仓级 rustfmt 基线建立于 2026-09-26（`031084c` 一次性收口 **120 文件**，配置见
> `mupc/rustfmt.toml`，钉 `max_width = 100`）。
>
> **现状（漂移）**：**基线之后被改动的文件会重新漂移** —— 本机 `cargo fmt --all -- --check`
> 现报 **69 处 diff / 24 个文件**，且**逐条核对：24/24 全部是基线提交之后被改过的文件**，
> 无一是 rustfmt 版本差异或 CRLF 行尾所致（已用「基线覆盖且之后未改的文件」作对照，其
> `--check` 为 0；CRLF→LF 转换后 diff 数不变）。
>
> **两条实际影响**：
> 1. **只想格式化自己的改动时不要用 `--all`** —— 它会连带重排那 24 个无关文件、淹没真实变更。
>    改用 `cargo fmt -p <crate>`（或 `rustfmt --edition 2021 <file>`）并逐个核对；判据是
>    `cargo fmt --all -- --check` 的 **diff 条数不增加**。
> 2. ⚠️ **CI 的 `lint` job 会跑 `cargo fmt --all -- --check`**（`.github/workflows/build-ubuntu.yml`，
>    `working-directory: mupc`，且 `test` job `needs: lint`）⇒ 这份漂移会让该门禁**长期为红**。
>    收敛它 = 一次全仓 `cargo fmt --all`（**24 文件**，按项目规则「>5 文件须走 PR」**不得直接
>    push master**），属独立议题，不要夹在功能提交里顺手做。

---

## 架构概览

```
调度主站 ←→ gateway (IEC 104) ←→ data-processing ←→ strategy-engine
                                        ▲                     │
                              采集结果  │                     │ 控制指令
                                        │                     ▼
南向设备 ←→ rs485-plugin/hplc-plugin/mupc-southd ──▶ PCS（= 实时控制模块）
                    │           └ mupc-southd::pcs::PcsHandle
                    │             （2026-09-26 由 intercore 迁入，走 RS485 Modbus RTU）
                    └─ 四条通路：插件化单设备 / 站级多从站调度 / PCS 通信与控制 / 数字 IO（mupc-io）

  intercore（核间 TCP 帧协议）┈┈  保留待接：mupcd 启动仍装配 IntercoreClient，但
                                    StartupContext.intercore 字段无读取方（详见下文数据流表）；
                                    实质消费者只有 sim-bridge，而 HIL 闭环未实现

  主控进程 (mupcd) ──display-proto(TCP 回环)──▶ local-display（12 号本地屏）
```

### 数据流

| 方向 | 组件 | 说明 |
|------|------|------|
| 北向 ↑ | gateway → data-processing → strategy-engine | 调度数据处理与上送 |
| 南向 ↓ | strategy-engine → mupc-southd / rs485-plugin / hplc-plugin | 设备控制指令下发 |
| 核间 ↔ | intercore (TCP/RJ45) | 仅保留核间 TCP 帧协议（帧协议 + 服务端 + 传输门面）；**该通道在生产路径无读取方**（`mupcd` 启动仍装配 `IntercoreClient`，但 `StartupContext.intercore` 字段现无消费者；`intercore.transport` 非 `tcp` 会**启动即报错**） |
| PCS ↕ | strategy-engine ↔ mupc-southd::pcs::PcsHandle | PCS 采集（三相读数 / SOC）+ 控制（启停 / 联锁 / 重启授权），走 RS485 Modbus RTU 从站 |
| 显示 → | mupcd → local-display | `display-proto` 帧 v3，TCP 回环 `GET /v1/display/latest`；写操作走 Axum `/v1/console/*` |
| AI → | strategy-engine ← ai-engine | **AI 引擎停用期间不生效**（框架保留；默认 `ai_engine.local_priority = true` ⇒ 本地策略优先） |

### PCS 通信与控制（mupc-southd）

PCS 为 **RS485 Modbus 从站**，其通信与控制已整体迁入南向（02 号设计 §13 / ADR-014，2026-09-26）。原「核间通信指令」表中的信号现由 `mupc-southd::pcs::PcsHandle` 承载：

| 信号 | 说明 |
|------|------|
| `p_ref` | 有功基准点 (kW)，用于下垂控制公式 |
| `k_droop` | 下垂系数 (kW/V)，用于下垂控制公式 |
| `ai_ready` | AI 引擎就绪状态 |
| `strategy_mode` | 当前策略模式 |

另有 `q_realtime_margin` / `voltage_phase_*`（属 AI 引擎观测空间 `ai-engine/src/data_fusion.rs`，PRD 记为来源于核间 DataUpload 帧；**AI 停用期间停采**）与 `SafetyOverride`（帧类型 0x0040，`intercore/src/protocol.rs`）。

> ⚠️ 02 号设计 §13 **未获门禁标记**（待独立设计评审）；`intercore` 侧的 `pcs` / `pcs_sim` / `modbus_rtu` 模块与 `transport::modbus` 已删除（`mupc/crates/intercore/src/lib.rs` 头注）。

### 南向设备指令

| 指令 | 目标设备 | 说明 |
|------|----------|------|
| `pv_limit` | 光伏逆变器 | ⚠️ **已废弃**（04 号 PRD：不再作为控制指令维度，代码保留不编译） |
| `load_shedding` | 负荷控制装置 | ⚠️ **已废弃**（同上） |

---

## 开发状态

| Phase | 内容 | 状态 |
|-------|------|------|
| Phase 1 | 核心架构（gateway、intercore、data-processing、strategy-engine） | ✅ 完成 |
| Phase 2A | 南向通信（RS485/HPLC）核心架构 | ✅ 基本完成（**5 个测试待修**：`device-trait` 3 / `rs485-plugin` 1 / `iec61850-plugin` 1，2026-09-27 复核仍失败 —— 见 `CLAUDE.md`「已知测试失败」） |
| Phase 2B | MQTT over TLS | ✅ 完成 |
| Phase 2B | SM2/SM4 国密 | ⚠️ **只留框架**（`security/Cargo.toml` 注明 framework-only，2026-09-09）；真实依赖 `gmsm 0.1.0`（非 0.14），SM3/SM4-CBC 为真国密，SM2 签名 / SM4-GCM / HKDF / ECDH 未实现，现由 ring 兜底 |
| Phase 3C | AI 优化引擎（LSTM、MADDPG/PPO、RKNN Runtime） | ✅ 完成（**2026-09-09 起引擎停用**：模型不加载、观测空间停采；框架保留） |
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
| Phase 2+ | WiFi/NearLink/BLE 驱动 | 📋 规划中（RBAC 鉴权中间件随 `web-api` crate 删除，不再适用） |
| 2026-09 | 12 号本地显示终端（触摸式 HMI，LVGL，BECG-3588） | 🚧 **实现已合入 master**（`display-proto` 协议侧 + `local-display` 渲染侧六页；含**外设数值上屏增量** PRD §3.9 F20–F26 / 设计 §15，即 P4 消防页与 P6「装置与外设」页）。三份文档门禁通过（PRD v2.5 / 设计 v2.1-r16 / UI 设计，均按**范围**生效）。**未结 = 真机验收**：设计 §15.8.2 的 D-1…D-6（台账 §8.7 **M-4**；未过不得通过 P4/P6 真机验收） |
| 2026-09 | PCS 通信与控制迁入 `mupc-southd`（02 号设计 §13 / ADR-014·015·016） | 🚧 **实现已合入 master**（`mupc-southd::pcs::PcsHandle` 为 PCS 完整所有者、采集与控制共用一把锁；`intercore` 的 `pcs`/`pcs_sim`/`modbus_rtu` 与 `transport::modbus` 已删）。**设计门禁已关**：§13 经**三轮独立设计评审**（首轮 / 复审 `[DESIGN_REJECTED]` → 第三轮 **`[DESIGN_APPROVED: 2026-09-29]`**；三轮报告见 `docs/superpowers/reports/02-§13-*`）。**未结 = ① T5 写审计**（§13.10 的收口实施项，**已实现于分支 `feature/t5-pcs-write-audit`、待合入**）**② 真机复核 M-10**（前置 M-11 / M-12）。台账 §6.13 的 **U-75 已由 T5 结清**，余 U-76 ~ U-81 |
| 2026-09 | **服务级健康监控**（07 号口径改三层，U-164） | ✅ 已实现（`mupc-core-bin/src/service_health.rs`，周期 15 s + 边沿告警；打通此前**全仓零调用点**的 `ServiceCoordinator`，14 个服务从"注册后无人读"变为每 15 s 回写）。**真机项未验**：`kill -9 mupcd` 核 systemd 拉起与服务级告警行为 |
| 2026-09 | **数据流与策略下发链路完整性审查 + 整改**（台账 U-166 ~ U-172） | ✅ **全收口**：补 `send_dual_param` 输入边界（±100 clamp）、数据超期改**下发归零**（原「停发」会让 PCS 保持陈旧设定数小时）、模式字争用成文、离线期告警节流。报告与方案见 `docs/superpowers/reports/` + `docs/superpowers/plans/2026-09-28-*` |
| 2026-09-29 | **硬件平台口径统一到 BECG-3588**（原 BECG-3568） | ✅ 文档 + 部署配置：6 个站串口节点按新板重映射（**后四路为 `ttyP0–P3`**，A0 为唯一不变项）、DI/DO GPIO 编号全换、急停 DI 极性按规格书推导为 `active_low: false`。**真机待核销**见 `docs/technical-debt.md` §8.7 **M-11 / M-12** |

技术债详见 [`docs/technical-debt.md`](docs/technical-debt.md)

---

## 文档体系

| 文档 | 说明 |
|------|------|
| [`mupc/build.md`](mupc/build.md) | 完整构建指南（三种方式、交叉编译、RKNN SDK） |
| [`mupc/deploy/deploy.md`](mupc/deploy/deploy.md) | 部署指南（一键部署、配置说明、模型部署、运维、故障排查） |
| [`mupc/deploy/local-display.md`](mupc/deploy/local-display.md) | 本地显示终端部署说明 |
| [`CLAUDE.md`](CLAUDE.md) | AI 协作开发指南 |
| [`docs/superpowers/specs/`](docs/superpowers/specs/) | 项目需求与模块 PRD（**12 份模块 PRD**：01–12；08 号已 SUPERSEDED） |
| [`docs/superpowers/plans/`](docs/superpowers/plans/) | 项目设计与模块设计（模块 01–12；**11 号仿真测试环境设计文档**路径 = `plans/modules/11-MUPC-仿真测试环境-设计文档.md`） |
| [`docs/superpowers/plans/modules/12-MUPC-本地显示终端-UI设计文档.md`](docs/superpowers/plans/modules/12-MUPC-本地显示终端-UI设计文档.md) | 12 号本地显示终端 UI 设计（版面几何权威） |
| [`docs/MUPC-数据流与储能调度-综述.md`](docs/MUPC-数据流与储能调度-综述.md) | **面向新人的综述**：正文业务向（怎么采集/处理存储/做储能调度/下发指令到 PCS）+ 技术附录（全景图、四条采集通路、代码入口、术语对照、覆盖范围） |
| [`docs/superpowers/reports/`](docs/superpowers/reports/) | 审查报告、交付报告（含链路完整性审查、代码评审、测试报告） |
| `docs/superpowers/plans/archive/`、`docs/superpowers/specs/archive/`、`docs/superpowers/specs/modules/archive/` | 归档计划 / 归档 PRD（2026-09-27 建立） |
| [`docs/technical-debt.md`](docs/technical-debt.md) | 技术债清单 |

---

## AI 协作开发

本项目配置了一套 AI Agent 协作框架。当需要开发新功能或修复 Bug 时，直接向 AI 助手描述需求，项目经理 Agent 将自动调度需求分析师、架构师、开发工程师等角色，按"合同与路径驱动"流程完成交付。

完整工作流定义见 [`CLAUDE.md`](CLAUDE.md) 和 `/.claude/agents/` 目录。

---

## 命名约定

- 大部分 crate 使用 `mupc-` 前缀（如 `mupc-common`、`mupc-ai-engine`）
- 无前缀的 crate：`device-trait`、`plugin-loader`、`rs485-plugin`、`hplc-plugin`、`display-proto`、`local-display`
- 目录名与 package name 不一致（引用时必须用 package name）：
  - **`mqtt-bridge`** → `mupc_mqtt_bridge`（下划线）
  - **`storage`** → `mupc_storage`（下划线）
  - **`system-monitor`** → `mupc_system_monitor`
  - **`wireless`** → `mupc_wireless`
  - **`mupc-io`** → `mupc-io`（目录名与 package name 均为 `mupc-io`）
  - **`mupc-southd`** → `mupc-southd`（bin `pcs_slave` 需 feature `pcs-slave-bin`）
  - **`sim-bridge`** → `mupc-sim-bridge`

---

## 许可证

MIT © 2026 ANGELICO —— 全文见 [`LICENSE`](LICENSE)
