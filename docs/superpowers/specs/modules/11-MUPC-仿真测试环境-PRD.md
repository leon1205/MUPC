# MUPC 仿真测试环境 — PRD

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.1 | 2026-07-10 | LEON | `[REVIEWED: PASS]` — 修复 PRD Reviewer 6 项反馈 |

---

## 1. 概述

### 1.1 背景

MUPC 已完成嵌入式设备部署（RK3588, mupcd 可执行），但缺乏系统级闭环测试能力：
- 无真实电网可接入进行控制指令验证
- 无自动化回归测试覆盖 5 个场景模式
- 开发者需要在不连接物理设备的情况下验证 AI 引擎全链路

### 1.2 目标

搭建一套**硬件在环（HIL）仿真测试环境**，使嵌入式 MUPC 能够在模拟电网场景下运行完整的决策闭环，验证 LSTM 预测 → RL 决策 → 动作输出的全链路正确性。

### 1.3 范围

| 包含 | 不包含 |
|------|--------|
| PC 端电网仿真引擎（Grid2Op + Pandapower） | 真实电网接入 |
| Rust 仿真桥接代理（sim-bridge） | MUPC Rust 源码修改 |
| Python 仿真引擎通信封装（engine.py） | 训练管线改造 |
| 5 场景闭环测试能力 | 实时控制模块硬件仿真 |
| Episode 指标收集与报告 | 仿真环境的分布式部署 |

> **MUPC 侧适配说明**：MUPC 通过 YAML 配置文件切换仿真模式（`mupc_core_config.yaml` 中修改 `intercore.host` 指向仿真 PC IP、`mqtt-plugin` 订阅 `mupc/sim/observation` topic），**不要求修改 Rust 源码**。这些配置变更属于运维操作范畴。

---

## 2. 架构

### 2.1 物理拓扑

```
┌─────────────────────────┐         ┌──────────────────────────────┐
│    仿真 PC (x86_64)      │         │    嵌入式 MUPC (RK3588)      │
│                          │  MQTT   │                              │
│  sim-bridge ─────────────┼─1883───▶│  mqtt-plugin → DataFusion   │
│     │                    │         │       ↓                      │
│     │ (stdin/stdout)     │  TCP    │  LSTM → RL → ActionOutput   │
│     ▼                    │◄─9100───│  IntercoreClient             │
│  engine.py               │         │                              │
│  └─ mupc_env (Grid2Op)   │         └──────────────────────────────┘
└─────────────────────────┘
```

### 2.2 数据闭环

```
MQTTPublisher ──78维观测──→ MUPC DataFusion
                                    ↓
                             LSTM 预测 (NPU)
                                    ↓
                              RL 决策 (NPU)
                                    ↓
                          ActionOutput {p_ref, k_droop}
                                    ↓
MUPC IntercoreClient ──TCP:9100──→ ActionServer (sim-bridge)
                                    ↓
                            PyEngine::send_step()
                                    ↓
                         engine.py env.step([p_ref, k_droop])
                                    ↓
                     Grid2Op 三相潮流 → Pandapower 更新
                                    ↓
                         78 维观测 ← 回到循环起点
```

### 2.3 场景模式

| 场景 | 关键参数 | 数据要求 |
|------|---------|---------|
| MODE-01 农网季节性 | 光伏 150kW, 负荷峰值 60kW, 灌溉冲击 80~120kW (6-9月, 50%概率), 季节编码按月份自动切换 | 中国合成数据, 15min分辨率, 全年 |
| MODE-02 自主套利 | 峰谷电价差 ≥0.5元/kWh, 电池 100kWh/50kW, 最大套利循环 ≤2次/天 | 分时电价曲线 (peak/valley/flat), 1h分辨率 |
| MODE-03 需量控制 | 合约需量 300kW, 月度峰值跟踪, 超需量罚金 ¥40/kW | 需量合约 + 15min实际需量数据 |
| MODE-04 虚拟电厂 | 辅助服务调度指令 {dispatch_p_set, dispatch_q_set}, 响应精度 ±5%, 响应时间 ≤5min | 调度指令时间序列 (15min分辨率) |
| MODE-05 极致绿色 | 电网排放因子 0.581 kgCO2/kWh, 绿电比例目标 ≥80%, 光伏弃光率 ≤5% | 碳排放因子 + 辐照/温度数据 |

> 所有场景共享：变压器 200kVA, 电池 100kWh/50kW, SOC [10%,90%], 过载阈值 85%, 时间步长 15min, episode 96 步 (24h)。

---

## 3. 功能需求

### 3.1 Rust 仿真桥接代理 (sim-bridge)

| ID | 需求 | 优先级 | 验收标准 |
|----|------|:--:|---------|
| SB-01 | 解析 YAML 配置文件，加载仿真场景参数 | P0 | Given 有效的 `sim_config.yaml`，When sim-bridge 启动，Then 5 场景参数（scenario/broker/addr 等）成功加载并打印 info 日志。Given 缺少必填字段的 YAML，When 启动，Then 输出错误并 exit(1) |
| SB-02 | 启动 Python 仿真引擎子进程，建立 stdin/stdout JSONL 通信 | P0 | Given Python venv 已配置，When sim-bridge 启动，Then engine.py 进程 PID 被记录，stdin/stdout 就绪。10s 内无响应 → panic 退出 |
| SB-03 | 通过 MQTT 发布 78 维观测数据到嵌入式 MUPC（QoS 0, topic: `mupc/sim/observation`） | P0 | Given engine.py 返回 obs 数组，When MQTT publish 执行，Then topic `mupc/sim/observation` 在 50ms 内收到 78 个 float 的 JSON 数组，数值误差 < 1e-6。连续 3 次 publish 失败 → 错误日志 + exit(1) |
| SB-04 | 监听 TCP 9100 端口，接收 MUPC IntercoreClient 发来的动作指令 | P0 | Given MUPC 连接 9100，When IntercoreClient 发送 ControlCommand 帧，Then sim-bridge 在 100ms 内解析出 p_ref 和 k_droop。绑定失败 → exit(1) |
| SB-05 | 将动作指令转发给 Python 引擎，等待下一观测返回 | P0 | Given 接收到有效动作，When send_step() 调用，Then engine.py 在 5s 内返回下一 obs。5s 超时 → WARN + 跳过本步继续 |
| SB-06 | 支持 episode 重置：当 done=true 时自动 send_reset() | P0 | Given engine.py 返回 done=true，When send_reset() 执行，Then 仿真环境重置为新 episode，obs 重新发布 |
| SB-07 | 优雅退出：接收 SIGINT/SIGTERM → 发送 shutdown → 等待子进程退出 | P1 | Given 仿真运行中，When Ctrl+C，Then shutdown JSONL 发送 → 等待 5s → kill 子进程 → 打印汇总指标 → exit(0) |
| SB-08 | 记录每步指标（延迟/奖励/违规次数），episode 结束后输出 JSON 报告 | P1 | Given episode 完成，When 退出前，Then `sim_metrics.json` 包含 total_steps/reward/soc_violations 等字段 |
| SB-09 | 未来扩展：Web 实时显示仿真状态（Phase 2） | P2 | — |

### 3.2 Python 仿真引擎 (sim-env)

| ID | 需求 | 优先级 | 验收标准 |
|----|------|:--:|---------|
| PE-01 | 实现 stdin/stdout JSONL 通信，支持 step/reset/shutdown | P0 | Given `{"type":"step",...}` 输入，When engine.py 读取，Then `{"type":"obs",...}` 在 2s 内输出。输入非 JSON → 跳过该行 + WARN |
| PE-02 | 复用 `mupc_env` 环境（78维观测、2维动作、5场景奖励） | P0 | Given MODE-01 场景，When env.step()，Then obs.shape=(78,), action.shape=(2,), reward 为 float |
| PE-03 | 默认 Grid2Op + Pandapower，降级 VoltageSimulator | P0 | Given Grid2Op 可用，When env.reset()，Then 使用 lightsim2grid 后端。Grid2Op 不可用时自动降级 + WARN |
| PE-04 | 支持 `--no-grid2op` 切换简化电压模型 | P1 | Given `--no-grid2op` 参数，When 启动，Then 使用 VoltageSimulator 模式 |
| PE-05 | 输出包含 reward/done/info 的完整响应 | P0 | Given env.step() 返回，When JSONL 输出，Then 包含 `data`(78维), `reward`, `done`, `info` 四字段 |

### 3.3 配置文件

| ID | 需求 | 优先级 | 验收标准 |
|----|------|:--:|---------|
| CF-01 | YAML 格式，定义场景/网络/步间隔/episode长度 | P0 | Given 配置文件，When 解析，Then 所有必填字段存在（scenario/mqtt_broker/action_listen_addr）。缺失 → error + exit(1) |
| CF-02 | 支持命令行参数覆盖配置文件值 | P1 | Given `--scenario MODE-02` 命令行参数，When 启动，Then 覆盖 YAML 中的 scenario 值 |

### 3.4 错误处理与恢复

| ID | 故障场景 | 预期行为 |
|----|---------|---------|
| EH-01 | MQTT Broker 不可达 | 启动阶段：panic 退出，提示检查 Broker 地址。运行阶段：连续 3 次 publish 失败 → ERROR 日志 + exit(1) |
| EH-02 | Python 子进程崩溃 | 检测 stdout EOF → ERROR 日志 → 等待 2s 重试 spawn（最多 3 次）→ 仍失败则 exit(1) |
| EH-03 | TCP 连接被 MUPC 关闭 | WARN 日志 → 继续监听新连接（MUPC 可能重启） |
| EH-04 | MUPC 动作超时未到达（>30s） | WARN 日志 "等待 MUPC 动作超时" → 跳过本步，发布当前 obs 继续 |
| EH-05 | engine.py 返回畸形 JSONL | ERROR 日志记录原始行 → 跳过该行 → 继续读取下一行 |
| EH-06 | p_ref/k_droop 超出物理约束 | WARN 日志记录原始值 → 按 §2.3 约束 clamp 后传递给 engine.py |
| EH-07 | engine.py 返回 done=true 且 reward 异常 | INFO 日志 → 正常触发 reset，不中断仿真 |

---

## 4. 接口定义

### 4.1 sim-bridge ↔ Python 引擎 (JSONL)

**请求格式**（sim-bridge → engine.py）：

```json
{"type":"reset","scenario":"MODE-01"}
{"type":"step","p_ref":25.0,"k_droop":0.3}
{"type":"shutdown"}
```

**响应格式**（engine.py → sim-bridge）：

```json
{"type":"obs","data":[0.5,75.0,30.0,...78 floats...],"reward":1.23,"done":false,"info":{"soc":0.52,"v_avg":1.01}}
{"type":"shutdown_ack"}
```

### 4.2 sim-bridge ↔ MUPC (MQTT)

```json
// Topic: mupc/sim/observation
// Payload: 78 维 float 数组
[0.52, 75.0, 30.0, -10.0, 0.35, 0.0, 1.01, 1.02, 0.99, ...]
```

> **安全隔离**：仿真 MQTT topic 使用 `mupc/sim/` 前缀命名空间，与生产数据 `mupc/telemetry/` 等 topic 物理隔离。建议仿真环境使用独立 MQTT Broker（端口 1884）或在生产 Broker 上配置 ACL 限制仿真 topic 的订阅/发布权限，避免仿真数据串扰生产系统。测试脚本应在启动前验证 Broker 连接的是仿真端口。

### 4.3 MUPC ↔ sim-bridge (TCP)

sim-bridge 监听 TCP 9100 端口，伪装为实时控制模块。MUPC IntercoreClient 通过 TCP 连接发送控制指令帧。

**帧格式**（复用 intercore `ControlCommand` 结构体二进制序列化）：

```
字节偏移  | 长度  | 字段        | 类型   | 说明
---------|-------|-------------|--------|------------------
0        | 4     | frame_id    | u32    | 帧序号 (大端)
4        | 1     | cmd_type    | u8     | 0x01 = 控制指令
5        | 1     | reserved    | u8     | 保留 (0x00)
6        | 2     | payload_len | u16    | 载荷长度 = 16 (大端)
8        | 8     | p_ref       | f64    | 有功基准点 kW (大端, IEEE 754)
16       | 8     | k_droop     | f64    | 下垂系数 kW/V (大端, IEEE 754)
24       | 2     | crc16       | u16    | CRC-16/MODBUS (大端)
```

**总帧长**：26 字节。

**sim-bridge 解析逻辑**：
1. 读取前 4 字节 → frame_id (u32 BE)
2. 读取 cmd_type (1 byte) → 仅处理 0x01
3. 读取 payload_len (2 bytes BE) → 验证 = 16
4. 读取 p_ref (8 bytes BE, f64) + k_droop (8 bytes BE, f64)
5. 读取 crc16 (2 bytes BE) → 验证 CRC-16/MODBUS
6. CRC 验证失败 → WARN 日志 + 丢弃帧

---

## 5. 硬件要求

| 项目 | 最低要求 | 推荐 |
|------|---------|------|
| 仿真PC CPU | 4 核 x86_64 | 8 核 |
| 仿真PC 内存 | 8 GB | 16 GB |
| 网络 | 100 Mbps 以太网 | 1 Gbps |
| 嵌入式设备 | RK3588, Ubuntu 20.04+ | NanoPC-T6-LTS |
| 仿真PC Python | 3.9+ | 3.11+ |
| 仿真PC Rust | 1.88+ | 1.88+ |

---

## 6. 性能要求

| 指标 | 目标值 | 说明 |
|------|:--:|------|
| 仿真步延迟（action→obs） | ≤ 500ms | 含 Grid2Op 50ms + 网络 RTT + MUPC 推理 |
| MUPC 推理耗时 | ≤ 200ms | LSTM + RL NPU 推理 |
| 网络往返延迟 | ≤ 50ms | 局域网 MQTT + TCP |
| episode 运行时间（96步） | ≤ 48s | 96 × 500ms |
| 12 小时稳定性 | 0 崩溃 | 持续运行 |

---

## 7. 项目结构

```
MUPC/
├── mupc/crates/sim-bridge/       # Rust 仿真代理（新增）
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs               # CLI + 主循环
│       ├── mqtt.rs               # MQTT 发布器
│       ├── action_server.rs      # TCP 动作服务器
│       ├── py_engine.rs          # Python 子进程管理
│       ├── scenario.rs           # 场景管理
│       ├── metrics.rs            # 指标收集
│       └── config.rs             # 配置结构
├── mupc/config/sim_config.yaml   # 仿真配置文件
├── sim-env/                      # Python 仿真引擎（新增, 从 MUPC-AI2 复制）
│   ├── mupc_env/                 # 复制核心文件
│   ├── engine.py                 # 新增：JSONL 主循环
│   └── requirements.txt
└── docs/superpowers/specs/modules/
    └── 11-MUPC-仿真测试环境-PRD.md   # 本文档
```

**不与 MUPC-AI2 训练项目耦合**：`sim-env/mupc_env/` 是从 MUPC-AI2 复制的一次性快照，独立维护。

---

## 8. 运行方式

### 8.1 前置条件

```bash
# 安装 Rust
cargo build -p mupc-sim-bridge --release

# 安装 Python 依赖
cd sim-env && python3 -m venv venv && source venv/bin/activate
pip install -r requirements.txt
```

### 8.2 启动仿真

```bash
# 终端 1: 启动 MUPC (嵌入式)
sudo -u mupc /opt/mupc/bin/mupcd

# 终端 2: 启动仿真桥接 (PC)
cd mupc && cargo run -p mupc-sim-bridge --release -- \
    --config config/sim_config.yaml \
    --scenario MODE-01
```

### 8.3 查看结果

```bash
# 仿真结束后查看指标
cat sim_metrics.json
```

---

## 9. 测试策略

| 测试类型 | 覆盖范围 | 方法 |
|---------|---------|------|
| 单元测试 | MQTT 发布/订阅、TCP Server/Client、JSONL 编解码 | `cargo test -p mupc-sim-bridge` |
| 集成测试 | sim-bridge ↔ engine.py 完整通信链路 | Python 脚本模拟 engine.py |
| 系统测试 | 5 场景 × 96 步全闭环 | 手动启动 MUPC + sim-bridge |
| 压力测试 | 1000 步连续运行，无内存泄漏、无 panic | 循环测试脚本 |

---

## 10. 里程碑

| Phase | 内容 | 预计工期 |
|:--:|------|:--:|
| Phase 1 | sim-bridge 核心框架：CLI + 配置 + Python 子进程 + 主循环 | 2 天 |
| Phase 2 | MQTT 发布器 + TCP 动作服务器 | 1 天 |
| Phase 3 | sim-env 复制 + engine.py 实现 | 1 天 |
| Phase 4 | 5 场景闭环调通，指标收集 | 1 天 |
| Phase 5 | 文档 + 测试 + 优化 | 1 天 |

---

**文档状态**: `[REVIEWED: PASS]` (v1.1 — 6 项审查反馈已修复)

**关联文档**:
- `mupc/crates/sim-bridge/` — Rust 仿真代理源码
- `sim-env/` — Python 仿真引擎
- `mupc/config/sim_config.yaml` — 仿真配置文件
- MUPC-AI2 `mupc_env/` — 训练环境（仿真环境的数据来源）
