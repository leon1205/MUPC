# MUPC 通信网关模块 — 综合技术设计文档

> ✅ **`[DESIGN_APPROVED: 2026-09-23, 设计评审员]`** —— **§9「外设数据上云（U-74 / U-71）」v1.4 增量复审通过**（上轮 P1–P7 与 AC-U74-05 补行、MQTT 精确值、`x509-parser` 版本逐条闭合；复算项——帧长 25/22/18 B、档位 19/84/56 与 22/153/56、`card(A∪S)=221=133+88`、并集 636/564——全部通过）。**有条件项**：7 项文档级勘误（含 §9.3.7 bms 行 52/289、§9.1.6「289 点」）须随本批开发同一提交修正，**修正前不得回填 PRD §8.7**；Q-B / Q-C·C-18 / Q2 三项须先裁定（见「版本演进」v1.4-r1 行）。**该标记仅覆盖 §9 增量**；§1–§8 既有正文与门禁标记未改。

> **§9 增量 r2（2026-09-23，用户裁定回写）—— ⚠️ 本增量【未经复审】，标注「待复审确认」**：由用户 **2026-09-23** 两项裁定引起（**本条由 2026-09-23 用户裁定引起，待复审确认**）——
> - **裁定 A（Q-B 关闭）**：把位 **424–426（簇一/二/三级告警）+ 454–459（继电器粘连 ×6）+ 460（AFE 故障）共 10 位**纳入 IEC104 上送 —— **落地形态：新增 3 个聚合组**（`bms_aggr_cluster_level` / `bms_aggr_relay_stuck` / `bms_aggr_afe_fault`），段 4 由 **301–312（12 组）扩为 301–315（15 组）**；**其余 50 位维持不入**（用户原话为"**至少**纳入"这三类）。
> - **裁定 B（Q-C 关闭）**：**放宽带宽上界以保 COS ≤ 1 s** —— IEC 104 稳态 **≤ 16 kbps**、MQTT 稳态 **≤ 6 KB/s**，**高峰瞬时不作保证**；PRD §8.7 与 AC-U74-14 同步订正。
>
> **波及章节**：§9.1.6 / §9.2.1.0 / §9.2.1（生成算法与子集规则）/ §9.2.1.2 / §9.2.1.3 / §9.2.2 / §9.2.5 / §9.3.3 / §9.3.7 / §9.5 / §9.7 / §9.8；PRD 侧 §8.2 / §8.3.1 / §8.3.2 / §8.9.1 / §8.10（**只改数字与必要说明**）。
> **重算后的关键量（可复算，全量清单见 §9.2.1.3）**：`card(A∪S) = 221 = 143 + 78`（覆盖 143 / 排除 78）；IEC104 基线 **159/231 → 162/234**；并集 **564/636 → 567/639**；A/B/C 档 **19/84/56 → 19/84/59**（PCS 启用 22/153/59）；突发 **3,975/5,775 B → 4,050/5,850 B**；**帧长不变**（25 / 22 / 18 B），**稳态与高峰带宽不变**（由变位率假设驱动，与点数无关）。
> **原 v1.4 的 7 项勘误（评审员裁定"随本批开发同一提交修正，修正前不得回填 PRD §8.7"）中，①（§9.3.7 `bms` 行 52→53 / 289→288）、②（§9.1.6「289 点」→345 点）、⑦（AC-U74-14 缺 MQTT 侧带宽行）在本增量内一并落地** —— 理由：本增量须回填 PRD §8.7，而上述三项即该回填的显式前置条件；**③④⑤⑥ 未动**，仍待开发同批修正。
> **本增量的数字在复审确认前不得作为开发冻结基线。**

> **§9 增量 r3（2026-09-23，T1 代码评审的 2 项设计侧偏离登记）—— ⚠️ 本增量【未经设计复审】，但**不改任何既有条款/语义/门禁标记**（纯登记）：**T1 `latest_values` 代码评审（`[CODE_REVIEWED: PASS: 2026-09-23]`，报告见 `../../reports/端到端数据流审查-外设采集到显示存储上云-2026-09-23.md` §T1）**判定的两项"设计侧登记缺失"已回写本节，使 §9 正文与实现逐条对齐：**
> - **偏离①（警告）`station_last_poll_ms` 未登记**：12 设计 §15.1.2 / §15.9 的 **R-38** 要求本件提供"该站最近一次成功采集时刻"的读口，实现已就位（`latest_values.rs:249`）而 **§9.1.3 全表原未列** ⇒ 令"方法签名全表"失真。**已在 §9.1.3 补一行** `pub fn station_last_poll_ms(&self, station: &str) -> Option<u64>`（**签名以 T1 实现为准**），并注明"**不自建第二套新鲜度真源**：过期判据仍**唯一**由 `is_fresh` / `station_is_active` 持有，真源仍是注入的 `stale_timeout_s` = 5 s"。
> - **偏离②（警告）`SouthSink::new` 多一个 `grid_station_id` 入参未登记**：§9.4 序 3 原只写"新增 `latest` 入参"，而 §9.1.4 要求 `on_grid_package` 内 `mark_station_polled(id)`、该回调入参**不含站 id**、且 §9.1.1 **明禁改 `StationSink` trait** ⇒ 这是"**取唯一不破 §9.1.1 禁令的解**"：站 id 在**装配期**由 `south_stations.grid_station()` 解析一次、经构造参数注入（`None` ⇒ 不写快照、**不臆造站 id**）。**已在 §9.4 序 3 与 §9.1.8 补登记该参数及其理由**（**不改 trait**）。
> - **改动范围**：**只动 §9.1.3 / §9.1.8 / §9.4 序 3 三处**（**纯登记，无一处改语义、改数字口径或改实现要求**）；**未改** §1–§8、任何代码、PRD、12 号设计、**既有门禁标记**（`[DESIGN_APPROVED: 2026-09-23, 设计评审员]` 原文未动，其覆盖范围仍为 §9 v1.4 增量）。

> **版本：** **v1.4-r3**（2026-09-23，**T1 代码评审的两项设计侧偏离登记（纯登记）**）—— 承 **v1.4-r2**（2026-09-23，用户裁定回写；待复审确认）← **v1.4**（2026-09-23，按设计评审意见修订；**设计评审复审通过**）

> **文档定位：** 本文档记录实现级设计决策。需求级内容（功能描述、验收标准、性能指标）请参考 [01-MUPC-通信网关-PRD](../specs/modules/01-MUPC-通信网关-PRD.md)。
>
> **v1.3 增量（2026-09-23）**：新增 **§9 外设数据上云（U-74 / U-71）设计增量**（落 01 PRD §8 `[REVIEWED: PASS: 2026-09-23]`）——① §9.1 **最新值入口**（01/03/12 三份共用件，归属 `mupc-data-processing`）；② §9.2 IEC 104 **231 点**（**v1.4-r2 后为 234 点**，见上「§9 增量 r2」）机械生成点表 + 总召（`C_IC_NA_1`）+ 连接初始快照 + **帧序号/编码修正**；③ §9.3 **MQTT 真做**（配置承载 / 生产者接线 / 分片主题 / 离线补送 / TLS fail-closed）。§5.5 的北向主题表由 §9.3.4 的新表取代（原行保留作历史）。本章**不改** PRD、**不改** 12 号设计。
>
> **v1.4 修订（2026-09-23，按设计评审意见）**：**只改 §9**，逐条闭合评审 P1–P7 与另两项 —— ① 补 **BMS 12 组聚合的运行期求值点**（写入侧、每轮触发、经唯一写入口进快照，§9.2.1.1）；② 重述 **`build_uplink_points` 生成期自检规则**，与**逐位排除表**（88 位，按 `BitClass` 逐位可核，§9.2.1.3）一致；③ **重算帧长与档位点数**（TI=36 → **25 B**、TI=30 → **22 B**；A/B/C = **19/84/56**（PCS 未启用 = **159**）/ 22/153/56（启用 = **231**））并回填**精确带宽**（§9.2.2、§9.3.7）；④ `ts` 口径**与 PRD §8.3.3 对齐**（UTC ISO-8601 毫秒），并注明线序由设计定（§9.3.4）；⑤ 处置 **`south_sim_loop` 的第二条 IEC104 上送路径**（删除其假遥测上送支路，§9.7 C-15 / §9.4 序 10）；⑥ 按 `positional` 规则**重推 PCS 段全部点名**（总 P = `pcs_3zone_33`、总 Q = `pcs_3zone_37`、总 PF = `pcs_3zone_41`，详见 §9.2.1）；⑦ **全部 `file:line` 按 HEAD `724225c` 全量重校**。另：补 **AC-U74-05 测试行**；补 **MQTT 侧带宽/资源精确值**；`x509-parser` 改 **0.16**（复用 workspace 现有版本）；补登记 **LV-1 对 12 号"间接读"的排除**、**`grid` 6 点派生名的唯一例外**、`bms_alarm_26` 序号核对。

**涵盖 Crate：** `gateway`, `iec61850-plugin`, `mqtt-plugin`, `mqtt-bridge`, `device-trait`

---

## 1. 模块架构

### 1.1 模块定位

通信网关是 MUPC 微电网特种调控装置"异构双核心模块主控架构"中**非实时处理核心**（大脑）的北向通信子系统。功能概述与跨模块关系（OTA、看门狗、安全合规等）详见 PRD 第 1 章。

### 1.2 整体架构

```
调度主站 (IEC 104)      配电自动化 (IEC 61850)      物联平台 (MQTT)
        │                        │                       │
        ▼                        ▼                       ▼
┌───────────────────────────────────────────────────────────────┐
│                      gateway crate                             │
│  ┌─────────────────┐  ┌──────────────────┐  ┌──────────────┐  │
│  │  iec104 server  │  │ iec61850-plugin  │  │ mqtt-plugin  │  │
│  │  (服务端:2404)  │  │ (MMS 客户端)     │  │ (北向客户端)  │  │
│  └────────┬────────┘  └────────┬─────────┘  └──────┬───────┘  │
│           │                    │                    │          │
│           └────────────────────┴────────────────────┘          │
│                              │                                 │
│                        ┌─────▼──────┐                          │
│                        │ MessageBus │                          │
│                        │  (trait)   │                          │
│                        └─────┬──────┘                          │
└──────────────────────────────┼─────────────────────────────────┘
                               │
          ┌────────────────────┼────────────────────┐
          ▼                    ▼                    ▼
   ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
   │data-processing│    │strategy-engine│    │  mqtt-bridge  │
   │  (遥测采集)   │    │  (策略引擎)   │    │(本地mosquitto)│
   └──────────────┘    └──────────────┘    └──────────────┘
          │                    │                    │
          └────────────────────┼────────────────────┘
                               ▼
                        ┌──────────────┐
                        │  intercore   │
                        │  (TCP/RJ45)  │
                        └──────┬───────┘
                               │
                               ▼
                        ┌──────────────┐
                        │ 实时控制模块  │
                        └──────────────┘
```

### 1.3 Crate 职责与状态

| Crate | 职责 | 状态 |
|-------|------|------|
| `gateway` | IEC 104 服务端，接收调度主站连接 | Phase 1 |
| `iec61850-plugin` | IEC 61850 MMS 客户端，连接 IED 设备 | Phase 2+ |
| `mqtt-plugin` | MQTT 北向客户端，连接物联平台 emqx | Phase 2+ |
| `mqtt-bridge` | 分层 MQTT 网桥（本地 mosquitto + 北向 emqx） | Phase 3B |
| `device-trait` | 核心 trait 定义（MessageBus、Plugin、Device 等） | Phase 2+ |

### 1.4 目标平台

目标平台与开发约束（操作系统、硬件、运行时、内存/启动限值）详见 PRD 第 1.2 节。本模块实现级约束：内存 < 50MB，冷启动 < 3s。

### 1.5 用户角色与权限

用户角色定义、协议与权限映射详见 PRD 第 1.4 节。**权限冲突规则：** 本地运维人员指令优先于所有北向指令。北向指令需经本地方略引擎校验后才转发。

> 本地无线运维通道（星闪 / Wi-Fi / 蓝牙）由模块09负责实现，详见模块09-PRD。

---

## 2. IEC 104 协议实现设计

### 2.1 架构设计

IEC 104 网关以**服务端模式**运行，监听 TCP 端口（默认 2404），接受调度主站连接。采用 `Iec104Server` + `Connection` + `Iec104Frame` 三层结构：

```
Iec104Server (TcpListener)
      │
      │ accept()
      ▼
Connection state machine
      │
      ▼
Iec104Frame (protocol parse/encode)
      │
      ├── UFrame: STARTDT/STOPDT/TESTFR
      ├── SFrame: I-frame ACK
      └── IFrame: ASDU data (telemetry/command)
```

### 2.2 数据结构定义

#### 帧格式

IEC 104 帧结构（最小 6 字节）：

```
┌──────┬────────┬────────┬────────┬────────┬──────────┐
│ 0x68 │ Length │ Control│ Control│ Control│ Control  │
│      │        │  1     │  2     │  3     │  4       │
├──────┼────────┼────────┼────────┼────────┼──────────┤
│ u8   │ u8     │ u8     │ u8     │ u8     │ u8       │
└──────┴────────┴────────┴────────┴────────┴──────────┘
```

#### 帧类型枚举 (protocol.rs)

```rust
pub enum FrameType {
    IFrame,  // 编号的信息传输帧
    SFrame,  // 确认帧
    UFrame,  // 控制帧
}

pub enum UFrameType {
    StartDtAct,   // 启动数据传输激活
    StartDtCon,   // 启动数据传输确认
    StopDtAct,    // 停止数据传输激活
    StopDtCon,    // 停止数据传输确认
    TestFrAct,    // 测试帧激活
    TestFrCon,    // 测试帧确认
}
```

#### 类型标识 (TypeId)

支持的 TypeID（代码位置：`gateway/src/iec104/protocol.rs` line 29-46）：

| 方向 | TypeId | 值 | 说明 |
|------|--------|-----|------|
| 监视 | `MSpNa1` | 1 | 单点遥信 (M_SP_NA_1) |
| 监视 | `MDpNa1` | 3 | 双点遥信 (M_DP_NA_1) |
| 监视 | `MMeNa1` | 9 | 测量值，归一化值 (M_ME_NA_1) |
| 监视 | `MMeNc1` | 13 | 测量值，短浮点数 (M_ME_NC_1) |
| 监视 | `MSpTa1` | 30 | 单点遥信带时标 (M_SP_TA_1) |
| 监视 | `MDpTa1` | 31 | 双点遥信带时标 (M_DP_TA_1) |
| 监视 | `MMeTa1` | 34 | 测量值带时标，归一化值 (M_ME_TA_1) |
| 监视 | `MMeTd1` | 35 | 测量值带时标，归一化值 (M_ME_TD_1) |
| 控制 | `CScNa1` | 45 | 单点遥控 (C_SC_NA_1) |
| 控制 | `CDcNa1` | 46 | 双点遥控 (C_DC_NA_1) |
| 控制 | `CSeNa1` | 48 | 调节命令 (C_SE_NA_1) |
| 控制 | `CScTa1` | 58 | 单点遥控带时标 (C_SC_TA_1) |
| 控制 | `CDcTa1` | 59 | 双点遥控带时标 (C_DC_TA_1) |
| 控制 | `CSeTa1` | 61 | 调节命令带时标 (C_SE_TA_1) |

#### 数据值

```rust
pub enum Value {
    SinglePoint(bool),      // 单点 (开/关)
    DoublePoint(u8),        // 双点 (00=中间,01=开,10=关,11=无效)
    Normalized(f64),        // 归一化值 (-1.0 ~ 1.0)
    Scaled(i16),            // 标度化值
    Float(f64),             // 短浮点数
}
```

#### ASDU 头

```rust
pub struct AsduHeader {
    pub type_id: TypeId,
    pub sq_num: u8,
    pub cot: Cot,          // 传输原因
    pub orig_addr: u16,    // 源站地址
}
```

#### 时标规范

所有带时标的 TypeID（M_SP_TA_1、M_DP_TA_1、M_ME_TA_1、M_ME_TD_1、C_SC_TA_1、C_DC_TA_1、C_SE_TA_1）使用时标字段遵循以下规范：

- **时间基准：** 所有时标字段使用 UTC 时间
- **精度：** 毫秒级（milliseconds since epoch）
- **编码：** 按 IEC 60870-5-4 标准的 CP56Time2a 格式编码（7 字节）

#### 连接状态机

```rust
pub enum ConnectionState {
    Disconnected,
    Connecting,
    WaitingStartDt,    // 等待 STARTDT
    Connected,
    Stopped,
}
```

### 2.3 服务器实现 (server.rs)

`Iec104Server` 结构：

```rust
pub struct Iec104Server {
    config: Iec104Config,
    connections: Arc<RwLock<Vec<Arc<RwLock<Connection>>>>>,
    shutdown_tx: broadcast::Sender<()>,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(config)` | 创建服务器实例 |
| `start(command_handler)` | 启动 TCP 监听，接受连接 |
| `shutdown()` | 停止服务器，清理所有连接 |
| `connection_count()` | 获取当前连接数 |

**连接处理流程：**
1. `accept()` 接受新 TCP 连接
2. 检查并发连接数（最大 5 个）
3. 创建 `Connection` 实例，添加到连接池
4. 启动异步任务 `handle_connection()` 处理帧
5. 帧解析 → 状态机处理 → 响应

**周期遥测上送流程：**

1. **周期上送**：监视方向 TypeID（遥信/遥测）按周期上送，默认 1s（≥1Hz，可配置），对齐 PRD §2.4 / IEC104-05
2. **告警即时上送**：告警/变位事件不等待周期，立即上送
3. **时标**：带时标 TypeID 使用 UTC 毫秒时标（CP56Time2a，见 §2.2 时标规范）
4. **上送队列**：主站处理慢时采用背压，优先丢弃过期遥测、保留最新值
   - ⚠️ **第 4 条已由 §9.2.2 细化为"分层背压"（2026-09-23）**：**A/B 档（周期遥测）** = `try_send`
     满则丢弃并计数（"丢旧留新"的落地）；**C 档（遥信变位）** = `send().await` **不丢**
     （变位是事件，无"保留最新值"语义可言，静默丢弃违反 PRD EX-2）。队列容量由 100 改为 2048。
   - 同时：遥测出向 I 帧的**发送序号改由连接层维护**（§9.2.3）——原实现由调用方编帧并恒传
     `seq = 0`，231 点突发下会被主站判为序号错误。

### 2.4 连接管理 (connection.rs)

`Connection` 结构：

```rust
pub struct Connection {
    pub stream: TcpStream,
    pub addr: SocketAddr,
    pub state: ConnectionState,
    pub send_seq: u16,
    pub recv_seq: u16,
    pub heartbeat_interval_secs: u64,
}
```

**U 帧处理：**
- `STARTDT_ACT` → `STARTDT_CON`，状态迁移到 `Connected`
- `STOPDT_ACT` → `STOPDT_CON`，状态迁移到 `Stopped`
- `TESTFR_ACT` → `TESTFR_CON`

**I 帧处理：**
- 序列号校验（`send_seq` / `recv_seq`）
- 发送 S 帧确认
- ASDU 解析

**断线重连流程：**

1. **断开检测**：读循环返回 EOF/Error，或 `connection_timeout_ms` 内无帧，判定连接失效
2. **连接清理**：失效连接从连接池移除，释放 `max_connections` 名额
3. **自动重连**：断连 5s 后开始重连，最多 10 次；10 次后改为每 1 分钟尝试（对齐 PRD §2.2 / IEC104-03）
4. **状态恢复**：重连成功后重新执行 `STARTDT_ACT → STARTDT_CON` 握手，状态迁移到 `Connected`
5. **期间降级**：重连期间暂缓周期遥测上送，连接恢复后按周期重新上送

### 2.5 命令处理 (command.rs)

```rust
pub struct ControlCommand {
    pub cmd_id: u16,
    pub cmd_type: CommandType,  // SwitchControl | PowerRegulation | ChargeDischarge
    pub p_set: Option<f64>,     // 有功设定值 (kW)
    pub q_set: Option<f64>,     // 无功设定值 (kVar)
    pub switch_state: Option<bool>,
    pub k_value: Option<f64>,   // 一次调频 K 值
    pub deadband: Option<f64>,  // 一次调频死区 (Hz)
    pub priority: u8,
}

#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError>;
    fn name(&self) -> &str;
}
```

### 2.6 配置定义

```rust
pub struct Iec104Config {
    pub listen_addr: String,               // "0.0.0.0"
    pub listen_port: u16,                  // 2404
    pub heartbeat_interval_secs: u64,      // 默认 10s
    pub connection_timeout_ms: u64,        // 默认 30000ms
    pub max_connections: usize,            // 默认 5
}
```

### 2.7 性能参数

性能指标（上报周期、指令延迟、并发连接数、心跳/超时/重连参数）详见 PRD 第 2 章和第 6.1 节。

---

## 3. IEC 61850 MMS 客户端设计

### 3.1 架构概述

采用 **libIEC61850 C 库 + Rust FFI 绑定** 实现真正的 MMS 协议栈。采用**短连接模式**（每次请求建立新连接），支持 MMS over TLS。

> **IEC 61850-7-420 DER 逻辑节点说明：** 完整的 IEC 61850-7-420 DER 逻辑节点模型（如 Photovoltaic、Storage、ElectricVehicle 等）延后至 Phase 2+ 实现。Phase 2 实现 MMS 传输层基础读写能力（7-2/7-3/8-1），支持标准 7-4 逻辑节点。

```
┌─────────────────────────────────────────┐
│            mms_client.rs                 │
│  ┌─────────────────────────────────┐    │
│  │       MmsClient                 │    │
│  │  - connect() / disconnect()     │    │
│  │  - read_do(ln, do_name)         │    │
│  │  - write_do(ln, do_name, value) │    │
│  └──────────┬──────────────────────┘    │
│             │                            │
│  ┌──────────▼──────────────────────┐    │
│  │      asn1_utils.rs              │    │
│  │  - encode_mms_request()         │    │
│  │  - decode_mms_response()        │    │
│  └─────────────────────────────────┘    │
└─────────────────────────────────────────┘
```

### 3.2 技术选型

| 特性 | 说明 |
|------|------|
| 协议栈 | libIEC61850 C 库 + iec61850-sys FFI 绑定 |
| 标准 | IEC 61850-7-2/7-3/8-1 (MMS) |
| 连接模式 | 短连接（每次请求建立新连接） |
| 默认端口 | 102 |
| TLS | MMS over TLS 1.2+（可选） |
| 依赖 | `iec61850-sys` (0.3, features: ["tls"]) |
| 证书 | `rustls` (0.23)、`webpki-roots` (0.26) |

### 3.3 连接状态机

```rust
pub enum MmsClientState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}
```

短连接模式：每次 `read_do()` / `write_do()` 调用先建立 TCP 连接，请求完成后断开。

### 3.4 MMS 数据类型 (mms_types.rs)

```rust
/// MMS 数据对象
pub struct DataObject {
    pub ln: String,        // 逻辑节点名（如 "LLN0", "MMXU1"）
    pub do_name: String,   // 数据对象名（如 "ST$Pos", "MX$Meas"）
}

/// MMS 服务类型
pub enum MmsService {
    Read,
    Write,
    DefineVariableAccess,   // 预留
    GetDataAccessAttributes, // 预留
}

/// MMS 请求
pub struct MmsRequest {
    pub service: MmsService,
    pub object: DataObject,
    pub payload: Vec<u8>,
}

/// MMS 响应
pub struct MmsResponse {
    pub success: bool,
    pub data: Vec<u8>,
    pub error: Option<String>,
}
```

便捷构造方法：
- `MmsRequest::read(ln, do_name)` — 创建 Read 请求
- `MmsRequest::write(ln, do_name, value)` — 创建 Write 请求
- `MmsResponse::success(data)` / `MmsResponse::error(msg)` — 创建响应

### 3.5 ASN.1 编码/解码 (asn1_utils.rs)

```rust
/// 编码 MMS 请求为 ASN.1 BER 格式
pub fn encode_mms_request(request: &MmsRequest) -> Result<Vec<u8>>;

/// 解码 ASN.1 BER 响应
pub fn decode_mms_response(data: &[u8]) -> Result<MmsResponse>;
```

**MMS PDU 标签：**

| 标签 | 说明 |
|------|------|
| `CONFIRMED_REQUEST_PDU` (0x01) | 确认请求 PDU |
| `CONFIRMED_RESPONSE_PDU` (0x02) | 确认响应 PDU |
| `CONFIRMED_ERROR_PDU` (0x03) | 确认错误 PDU |
| `UNCONFIRMED_PDU` (0x04) | 非确认 PDU |
| `REJECTED_PDU` (0x05) | 拒绝 PDU |

### 3.6 MMS 客户端实现 (mms_client.rs)

```rust
pub struct MmsClient {
    config: MmsConfig,
    state: Arc<parking_lot::RwLock<MmsClientState>>,
}
```

**核心方法：**

| 方法 | 说明 |
|------|------|
| `new(config)` | 创建 MMS 客户端 |
| `connect()` | 连接到 IED（短连接模式） |
| `disconnect()` | 断开连接 |
| `read_do(ln, do_name)` | 读取数据对象 |
| `write_do(ln, do_name, value)` | 写入数据对象 |
| `get_state()` | 获取客户端状态 |

**请求流程：**
1. 建立 TCP 连接到 IED（支持超时配置）
2. 使用 ASN.1 编码请求
3. 发送请求（`write_all`）
4. 读取响应（支持超时配置）
5. 使用 ASN.1 解码响应
6. 断开连接

### 3.7 MMS Trait (mms_client.rs)

```rust
#[async_trait]
pub trait MmsClientTrait: Send + Sync {
    async fn connect(&self) -> Result<()>;
    fn disconnect(&self);
    fn get_state(&self) -> MmsClientState;
    async fn read_do(&self, ln: &str, do_name: &str) -> Result<Vec<u8>>;
    async fn write_do(&self, ln: &str, do_name: &str, value: &[u8]) -> Result<()>;
}
```

### 3.8 GOOSE 消息订阅 (goose.rs)

| 功能 | 说明 |
|------|------|
| 订阅 GOOSE 消息 | 通过 GOOSE ID (`go_id`) 识别消息源 |
| 回调处理 | 消息通过 MessageHandler 回调 |
| 配置参数 | 本地 IP、本地端口、远端 IP、远端端口 |

### 3.9 配置定义

```rust
/// MMS 配置
pub struct MmsConfig {
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,              // 默认 102
    pub max_connections: u32,
    pub connect_timeout_ms: u64,       // 默认 5000
    pub read_timeout_ms: u64,          // 默认 3000
    pub tls: Option<MmsTlsConfig>,
}

/// MMS TLS 配置
pub struct MmsTlsConfig {
    pub enabled: bool,
    pub ca_cert_path: String,
    pub client_cert_path: String,
    pub client_key_path: String,
    pub verify_peer: bool,
}
```

### 3.10 错误类型

| 错误 | 说明 |
|------|------|
| `MmsConnectFailed` | MMS 连接失败（含 TLS 连接失败） |
| `MmsTimeout` | MMS 请求超时（连接/读取） |
| `MmsProtocolError` | MMS 协议错误 |
| `MmsInvalidResponse` | 无效响应 |
| `DataObjectNotFound` | 数据对象不存在 |
| `WriteFailed` | 写操作失败 |
| `TlsConnectFailed` | TLS 连接失败 |
| `CertVerifyFailed` | 证书验证失败 |
| `Asn1EncodeFailed` | ASN.1 编码失败 |
| `Asn1DecodeFailed` | ASN.1 解码失败 |
| `LibIec61850Error` | libIEC61850 底层错误 |

### 3.11 性能要求

性能指标（连接建立时间、读写响应时间、操作成功率）详见 PRD 第 3.8 节。

### 3.12 架构决策

**目标架构：** 通过 `iec61850-sys` FFI 绑定调用 libIEC61850 C 库实现完整 MMS 协议栈，覆盖 IEC 61850-7-2（ACSI）、7-3（公用数据类）、8-1（特定通信服务映射 SCSM）。

**为何不继续使用纯自研方案：**
- IEC 61850 MMS 协议栈复杂度高（ASN.1 BER/DER、ACSI 服务映射、SCSM T-Profile/A-Profile），自研完整协议栈工作量巨大
- libIEC61850 是成熟的工业级 C 实现，已在大量 IED 设备中验证互操作性
- FFI 方案允许 Phase 2 快速提供基础读写能力，后续按需扩展高级 ACSI 服务（报告、日志、定值组）

**Feature Flag 策略：**

本 crate 通过 Cargo features 支持双模式编译，便于开发阶段在没有 C 库环境下编译测试：

```toml
[features]
default = ["real_iec61850"]
real_iec61850 = ["dep:iec61850-sys"]   # 生产模式：链接 libIEC61850 C 库
fake_iec61850 = []                      # 开发模式：使用纯 Rust ASN.1 自实现
```

各模块通过 `#[cfg(feature = "real_iec61850")]` 条件编译，在 Fake 模式下使用纯 Rust ASN.1 编解码，Real 模式下委托给 libIEC61850。

### 3.13 构建管线（cmake + libIEC61850）

libIEC61850 C 库需通过 CMake 交叉编译为目标平台（openEuler / RK3588 aarch64）的静态库（`.a`）或动态库（`.so`）。使用 `cmake` crate 作为 build-dependency 驱动编译：

```toml
[build-dependencies]
cmake = "0.1"
```

`build.rs` 负责：
1. 检测目标平台（`TARGET` 环境变量）
2. 调用 cmake 编译 libIEC61850 C 源码（需预置于 `iec61850-sys/vendor/` 或通过 git submodule 引入）
3. 设置 `cargo:rustc-link-lib=static=IEC61850` 和 `cargo:rustc-link-search` 指向编译产物目录
4. 生成 FFI 绑定（通过 `bindgen` 或预生成的 `src/bindings.rs`）

依赖关系链：`iec61850-plugin → iec61850-sys → libIEC61850.so`

### 3.14 ASN.1 BER TLV 编码细节

MMS 协议使用 ASN.1 Basic Encoding Rules (BER) 的 TLV（Tag-Length-Value）格式。本 crate 实现了最小化的 BER 编码器，覆盖 MMS Read/Write 请求及响应解码。

**TLV 长度编码算法（encode_length）：**

```
短格式（长度 < 128）：
  [length]                           → 1 字节，bit7 = 0

长格式 1 字节（128 <= length < 256）：
  [0x81][length]                     → 2 字节，首字节 bit7 = 1 指示长格式

长格式 2 字节（256 <= length < 65536）：
  [0x82][length_hi][length_lo]       → 3 字节，大端序
```

**实现代码：**

```rust
fn encode_length(buf: &mut Vec<u8>, len: usize) {
    if len < 128 {
        buf.push(len as u8);           // 短格式
    } else if len < 256 {
        buf.push(0x81);                // 长格式，1 字节长度
        buf.push(len as u8);
    } else {
        buf.push(0x82);                // 长格式，2 字节长度
        buf.push((len >> 8) as u8);
        buf.push((len & 0xFF) as u8);
    }
}
```

**MMS PDU 标签定义：**

```rust
mod pdu_tags {
    pub const CONFIRMED_REQUEST_PDU:  u8 = 0x01;
    pub const CONFIRMED_RESPONSE_PDU: u8 = 0x02;
    pub const CONFIRMED_ERROR_PDU:    u8 = 0x03;
    pub const UNCONFIRMED_PDU:        u8 = 0x04;
    pub const REJECTED_PDU:           u8 = 0x05;
}
```

### 3.15 MMS PDU 构造细节

**Read Request APDU 结构**（IEC 61850-8-1 SSAP）：

```
Confirmed-RequestPDU ::= CHOICE {
    [1] IMPLICIT Confirmed-RequestPDU-inner
}

Confirmed-RequestPDU-inner ::= SEQUENCE {
    invokeId          [0] IMPLICIT Integer32,
    confirmedService  [2] ConfirmedService
}

ConfirmedService ::= CHOICE {
    read  [4] Read-Request
}

Read-Request ::= SEQUENCE {
    specification-with-result  [0] IMPLICIT SpecificationWithResult OPTIONAL,
    variableAccessSpecification [1] VariableAccessSpecification
}
```

**实现流程（encode_read_request）：**

1. APDU 头 → `0x01`（Confirmed-RequestPDU）
2. invokeId → Tag `0x81` + Length `0x01` + Value `0x01`（invokeId 固定为 1）
3. 服务类型 → Tag `0x82`（confirmedService）
4. Read Service → Tag `0x24`（SEQUENCE OF）+ 长度 + 内容
5. 内容：`0xA0`（list-of-variable-access-specification）→ `0xA1`（variable-specification）→ `0x80`（object-name）+ 长度 + UTF-8 路径名

**Write Request 额外增加：**
- Tag `0x84`（data）+ 长度 + payload octet-string

**响应解码（decode_mms_response）：**
- 首字节为 `0x02` → 成功响应（Confirmed-ResponsePDU）
- 首字节为 `0x03` → 协议错误（Confirmed-ErrorPDU）
- 首字节为 `0x05` → 被拒绝（RejectedPDU）
- 其他值 → 未知响应类型错误

### 3.16 MMS 客户端请求流程（send_request 详解）

短连接模式下的完整请求-响应流程：

```
┌──────────────────────────────────────────────────────────────────┐
│                    send_request(request)                          │
│                                                                   │
│  1. 状态检查: state == Connected? ──No──→ Err("未连接")           │
│       │                                                           │
│       Yes                                                         │
│       ▼                                                           │
│  2. ASN.1 编码: encode_mms_request(&request) → Vec<u8>            │
│       │  ┌─ Read  → encode_read_request()                         │
│       │  ├─ Write → encode_write_request()                        │
│       │  ├─ DefineVariableAccess → Err("未实现")                  │
│       │  └─ GetDataAccessAttributes → Err("未实现")               │
│       ▼                                                           │
│  3. TCP 连接 (短连接):                                            │
│     timeout(connect_timeout_ms,                                    │
│       TcpStream::connect(remote_ip:102))                           │
│       │                                                           │
│       ├─ 超时 → Err(MmsTimeout)                                   │
│       └─ TCP 错误 → Err(MmsConnectFailed)                         │
│       ▼                                                           │
│  4. 发送请求: stream.write_all(&req_data)                         │
│       │                                                           │
│       └─ 失败 → Err(ProtocolError)                                │
│       ▼                                                           │
│  5. 读取响应: buf = [0u8; 8192]                                   │
│     timeout(read_timeout_ms, stream.read(&mut buf))                │
│       │                                                           │
│       ├─ 超时 → Err(MmsTimeout)                                   │
│       ├─ TCP 错误 → Err(ProtocolError)                            │
│       └─ n 字节 → 继续                                            │
│       ▼                                                           │
│  6. ASN.1 解码: decode_mms_response(&buf[..n])                    │
│       │                                                           │
│       ├─ 成功 → Ok(MmsResponse { success: true, ... })            │
│       └─ 失败 → Err(MmsProtocolError / MmsInvalidResponse)        │
│       ▼                                                           │
│  7. 返回结果（连接随函数返回自动关闭）                              │
└──────────────────────────────────────────────────────────────────┘
```

**超时配置（默认值）：**
- `connect_timeout_ms`: 5000ms（IED 连接超时）
- `read_timeout_ms`: 3000ms（响应读取超时）

**读取缓冲区：** 固定 8192 字节（8KB），足以容纳典型 MMS 响应。

**注意：** 当前实现每次请求都执行 `TcpStream::connect()`，完成后连接自动丢弃。这意味着 `connect()` 方法主要用于状态转换（Disconnected → Connecting → Connected），而非保持长连接。实际请求连接在 `send_request()` 内部管理。

### 3.17 并发模型选型（parking_lot::RwLock）

MmsClient 的状态字段使用 `parking_lot::RwLock` 而非 `tokio::sync::RwLock`：

```rust
pub struct MmsClient {
    config: MmsConfig,
    state: Arc<parking_lot::RwLock<MmsClientState>>,
}
```

**选型理由：**
- MmsClient 的状态读写是**同步操作**（内存赋值），不涉及 `.await`，无需异步锁
- `parking_lot::RwLock` 在无竞争时的开销低于 `tokio::sync::RwLock`（无调度器交互）
- MmsClient 自身不是 `Clone`，但 `Arc<parking_lot::RwLock<...>>` 允许在多个异步任务间共享客户端实例

**依赖：** 需在 `Cargo.toml` 中添加 `parking_lot = "0.12"`。

### 3.18 测试策略

各模块采用内联单元测试（`#[cfg(test)] mod tests`），按模块分组：

| 模块 | 测试数量 | 覆盖内容 | 关键测试 |
|------|---------|---------|---------|
| `mms_types.rs` | 6 | DataObject 解析/序列化，MmsRequest 构建，MmsResponse 成功/错误 | `test_data_object_from_str`（LLN0$ST$Pos 分割）, `test_mms_request_write`（payload 载体）, `test_mms_response_error`（错误消息保留） |
| `asn1_utils.rs` | 6 | Read/Write 请求编码，响应解码（成功/错误/空响应/拒绝），长度编码 | `test_encode_read_request`（产出首字节=0x01）, `test_encode_length`（127/200 边界值） |
| `mms_client.rs` | 4 | 客户端创建，状态转换，未连接时读/写错误 | `test_mms_client_read_do_not_connected`（异步测试）, `test_mms_client_write_do_not_connected`（异步测试） |
| `lib.rs` | 2 | Iec61850Status Display，MmsService 导出验证 | `test_iec61850_status_display`, `test_mms_types_export` |

**测试运行：**
```bash
cargo test -p mupc-iec61850-plugin                    # 全部 18 个测试
cargo test -p mupc-iec61850-plugin mms_types           # 6 个
cargo test -p mupc-iec61850-plugin asn1_utils           # 6 个
cargo test -p mupc-iec61850-plugin mms_client           # 4 个
```

**已知限制：** MmsClient 的网络集成测试（真实 IED 连接）未包含在单元测试中，因需要可用的 IED 设备。`TestMmsClient` mock（通过 `MmsClientTrait` trait）用于上层集成测试。

### 3.19 未实现的 ACSI 服务

以下 MMS/ACSI 服务在当前计划中有占位定义但返回"未实现"错误：

| 服务 | MmsService 枚举值 | 状态 | 说明 |
|------|-------------------|------|------|
| DefineVariableAccess | `DefineVariableAccess` | 未实现 | 用于预先定义变量访问路径以优化批量读取；非 Phase 2 核心需求 |
| GetDataAccessAttributes | `GetDataAccessAttributes` | 未实现 | 用于查询数据对象的访问属性（读/写权限）；非 Phase 2 核心需求 |

这两个服务在 `asn1_utils.rs` 中对应函数直接返回 `Err(Asn1EncodeFailed("...未实现"))`。Phase 2 仅需 Read/Write 两个基础服务即可满足 DER 数据采集与控制需求。

---

## 4. MQTT 通信设计

### 4.1 分层 MQTT 架构

MQTT 功能分为两个层次：

1. **MQTT 北向客户端（mqtt-plugin）**：与物联平台 / VPP 通信，基于 emqx 企业级 MQTT Broker
2. **MQTT 本地网桥（mqtt-bridge）**：进程间通信，基于本地 mosquitto Broker

```
┌─────────────────────────────────────────────────────────────┐
│                   emqx（北向/云端）                             │
│         物联平台 + 配电自动化主站                              │
└─────────────────────────┬───────────────────────────────────┘
                          │ MQTT + TLS + 证书认证
                          │ Port: 8883
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    本地 mosquitto                             │
│                   （进程间通信）                                │
│                   Port: 1883                                 │
└───────┬─────────────────┬─────────────────┬─────────────────┘
        │                 │                 │
        ▼                 ▼                 ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│data-processing│  │strategy-engine│  │   其他模块   │
│   （发布）     │  │   （订阅）     │  │             │
└──────────────┘  └──────────────┘  └──────────────┘
```

### 4.2 MQTT 北向客户端 (mqtt-plugin)

#### 核心结构

```rust
pub struct MqttClient {
    config: MqttConfig,
    inner: AsyncClient,                    // rumqttc
    state: Arc<RwLock<MqttClientState>>,
    event_tx: broadcast::Sender<Event>,
}
```

#### 连接状态

```rust
pub enum MqttClientState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}
```

#### 核心方法

| 方法 | 说明 |
|------|------|
| `new(config)` | 创建客户端（自动启动事件循环） |
| `connect()` | 连接到 MQTT Broker |
| `disconnect()` | 断开连接 |
| `subscribe(topic, qos)` | 订阅主题 |
| `publish(topic, payload, qos, retain)` | 发布消息 |
| `get_state()` | 获取客户端状态 |

#### QoS 枚举

```rust
pub enum MqttQos {
    AtMostOnce = 0,   // QoS 0
    AtLeastOnce = 1,  // QoS 1
    ExactlyOnce = 2,  // QoS 2
}
```

#### 配置

```rust
pub struct MqttConfig {
    pub broker_addr: String,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub use_tls: bool,
    pub ca_cert: Option<String>,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
    pub qos: MqttQos,
    pub keepalive_secs: u16,
    pub clean_session: bool,
}
```

#### 错误类型

| 错误 | 说明 |
|------|------|
| `ConnectFailed` | 连接失败 |
| `AuthFailed` | 认证失败 |
| `SubscribeFailed` | 订阅失败 |
| `PublishFailed` | 发布失败 |
| `Disconnected` | 连接已断开 |
| `TlsConfigError` | TLS 配置错误 |
| `QosNotSupported` | QoS 不支持 |
| `ProtocolError` | 协议错误 |

#### TLS 配置

支持 MQTT over TLS 1.2+，双向证书认证：
- CA 证书（`ca_cert`）
- 客户端证书（`client_cert`）
- 客户端私钥（`client_key`）

#### 断线重连策略

| 参数 | 值 |
|------|-----|
| 初始间隔 | 1 秒 |
| 指数退避 | 2 倍 |
| 最大间隔 | 60 秒 |
| 重连次数 | 无上限 |

### 4.3 MQTT 本地网桥 (mqtt-bridge)

#### MqttBridge Trait

```rust
#[async_trait]
pub trait MqttBridge: Send + Sync {
    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), MqttBridgeError>;
    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), MqttBridgeError>;
    fn is_connected(&self) -> bool;
}
```

#### LocalMqttClient

- 连接本地 mosquitto（`127.0.0.1:1883`）
- 用于进程间通信
- QoS 0/1
- 无 TLS

```rust
pub struct LocalMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<Mutex<bool>>,
}
```

**事件循环与连接状态管理：** LocalMqttClient 通过 `process_events()` 轮询 rumqttc 的事件循环，在收到 `Event::Connected` 时将 `connected` 置为 `true`，在收到 `Event::Disconnected` 时置为 `false` 并返回 `MqttBridgeError::Disconnected`。该机制确保连接状态与实际的 MQTT Broker 连接保持同步，供上层通过 `is_connected()` 查询。

#### NorthMqttClient

- 连接 emqx（可配置地址 `:8883`）
- 用于北向通信
- QoS 1/2
- TLS + 双向证书认证

```rust
pub struct NorthMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<Mutex<bool>>,
}
```

#### mqtt-bridge 配置

```rust
pub struct MqttConfig {
    pub local: LocalMqttConfig,
    pub north: NorthMqttConfig,
}

pub struct LocalMqttConfig {
    pub broker_addr: String,       // "127.0.0.1:1883"
    pub client_id: String,
    pub clean_session: bool,
    pub keepalive_secs: u64,
    pub reconnect: ReconnectConfig,
}

pub struct NorthMqttConfig {
    pub broker_addr: String,       // "mqtt.example.com:8883"
    pub client_id: String,
    pub keepalive_secs: u64,
    pub tls: TlsConfig,
    pub reconnect: ReconnectConfig,
}

pub struct TlsConfig {
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

pub struct ReconnectConfig {
    pub initial_interval_secs: u64,   // 默认 1s
    pub max_interval_secs: u64,       // 默认 60s
    pub backoff_multiplier: f64,      // 默认 2.0
}
```

#### mqtt-bridge 错误类型

```rust
pub enum MqttBridgeError {
    ConnectionFailed(String),
    SubscribeFailed(String),
    PublishFailed(String),
    TlsError(String),
    CertificateError(String),
    Disconnected(String),
    MaxReconnectAttemptsReached(usize),
    Timeout(String),
}
```

---

## 5. 消息总线设计

### 5.1 MessageBus Trait

定义于 `device-trait/src/message_bus.rs`：

```rust
/// 消息总线接口
pub trait MessageBus: Send + Sync {
    fn publish(&self, msg: Message) -> Result<(), BusError>;
    fn subscribe(&self, topic: Topic, handler: Arc<dyn MessageHandler>) -> Result<(), BusError>;
    fn unsubscribe(&self, topic: &Topic) -> Result<(), BusError>;
    fn subscriber_count(&self, topic: &Topic) -> usize;
    fn subscribed_topics(&self) -> Vec<Topic>;
}
```

### 5.2 消息处理器

```rust
pub trait MessageHandler: Send + Sync {
    fn handle(&self, msg: Message);
}
```

### 5.3 数据类型

```rust
/// 消息主题
pub struct Topic(String);

/// 消息封装
pub struct Message {
    pub topic: Topic,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}
```

### 5.4 错误类型

```rust
pub enum BusError {
    TopicNotFound(String),
    PublishFailed(String),
    SubscribeFailed(String),
    UnsubscribeFailed(String),
    Other(String),
}
```

### 5.5 Topic 定义

#### 北向 Topic（emqx）

| 常量 | Topic | 方向 | QoS | 说明 |
|------|-------|------|-----|------|
| `NORTH_TELEMETRY` | `mupc/north/telemetry` | → 物联平台 | 1 | 高频遥测数据 |
| `NORTH_FAULT` | `mupc/north/fault` | → 物联平台 | 2 | 故障事件 |
| `NORTH_STRATEGY_COMMAND` | `mupc/north/strategy/command` | ← 物联平台 | 2 | 下行指令 |
| `NORTH_STATUS` | `mupc/north/status` | ↔ 双方 | 0 | 设备状态 |

#### 进程间 Topic（mosquitto）

| 常量 | Topic | 方向 | QoS | 说明 |
|------|-------|------|-----|------|
| `LOCAL_TELEMETRY` | `mupc/local/telemetry` | → | 0 | 遥测数据 |
| `LOCAL_STRATEGY_COMMAND` | `mupc/local/strategy/command` | → | 1 | 策略指令 |
| `LOCAL_AI_READY` | `mupc/local/ai/ready` | → | 0 | AI 就绪状态 |

常量定义位置：`mqtt-bridge/src/topics.rs`

> ⚠️ **本表已由 §9.3.4 取代（2026-09-23）**：PRD §8.3.3 强制 `{station_id}` 分片，北向遥测/事件
> 主题改为 `mupc/north/telemetry/{station_id}` / `mupc/north/event/{station_id}`（`north_telemetry(id)` /
> `north_event(id)` 函数式常量）；`NORTH_STATUS` / `NORTH_FAULT` 不变。原表保留作历史对照。

### 5.6 实现策略演进

| Phase | 实现方式 | 说明 |
|-------|----------|------|
| Phase 1 | `tokio::sync::mpsc` | 进程内通信，最简实现 |
| Phase 2+ | device-trait::MessageBus trait | 可替换为 AMQP/MQTT |
| Phase 3B | mosquitto（进程间）+ emqx（北向） | 分层 MQTT 总线架构 |

### 5.7 数据流设计

#### 遥测数据流

```
intercore → DataCollector → LocalMqttClient → mosquitto (本地)
                                                   ↓
                                           NorthMqttClient
                                                   ↓
                                            emqx (云端)
                                                   ↓
                                            物联平台
```

#### 策略指令流

```
物联平台 → emqx → NorthMqttClient → LocalMqttClient → mosquitto
                                                            ↓
                                                  AiCommandValidator
                                                            ↓
                                                  intercore → 实时控制模块
```

### 5.8 消息持久化策略

| 消息类型 | QoS | 持久化 | 说明 |
|---------|-----|--------|------|
| 遥测数据 | 1 | 否 | 仅实时展示 |
| 故障事件 | 2 | 是 | 需要事后分析 |
| 策略指令 | 2 | 是 | 断线重连恢复 |
| 设备状态 | 0 | 否 | 周期性刷新 |

### 5.9 性能要求

性能指标（消息总线吞吐量、进程间消息延迟）详见 PRD 第 5.5 节。

---

## 6. 接口定义

### 6.1 插件接口 (Plugin)

定义于 `device-trait/src/plugin.rs`：

```rust
pub trait Plugin: Send + Sync {
    fn meta(&self) -> PluginMeta;
    fn init(&self, config: serde_json::Value) -> Result<(), PluginError>;
    fn start(&self) -> Result<(), PluginError>;
    fn stop(&self) -> Result<(), PluginError>;
    fn shutdown(self: Box<Self>) -> Result<(), PluginError>;
}

pub enum PluginState {
    Loaded,
    Initialized,
    Running,
    Stopped,
    Unloaded,
}
```

### 6.2 插件加载器 (PluginLoader)

定义于 `device-trait/src/plugin_loader.rs`：

```rust
pub trait PluginLoader: Send + Sync {
    fn load(&self, plugin_path: &str, config: serde_json::Value) -> Result<(), PluginError>;
    fn unload(&self, plugin_name: &str) -> Result<(), PluginError>;
    fn list(&self) -> Vec<PluginMeta>;
    fn get(&self, plugin_name: &str) -> Option<Arc<dyn Plugin>>;
    fn is_loaded(&self, plugin_name: &str) -> bool;
    fn plugin_count(&self) -> usize;
    fn unload_all(&self) -> Result<(), PluginError>;
}
```

### 6.3 插件元信息

```rust
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}
```

### 6.4 FFI 导出规范

动态插件（.so/.dll）需导出以下函数：

```rust
// 插件工厂
#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn Plugin;

// 插件元信息
#[no_mangle]
pub extern "C" fn plugin_meta() -> PluginMeta;
```

**插件生命周期：** Load → Init → Start → Stop → Unload

### 6.5 设备注册表 (DeviceRegistry)

定义于 `device-trait/src/registry.rs`：

```rust
pub trait DeviceRegistry: Send + Sync {
    fn register(&self, device: Arc<dyn Device>) -> Result<(), RegistryError>;
    fn unregister(&self, device_id: &str) -> Result<(), RegistryError>;
    fn get(&self, device_id: &str) -> Option<Arc<dyn Device>>;
    fn query_by_type(&self, device_type: &str) -> Vec<Arc<dyn Device>>;
    fn list_all(&self) -> Vec<String>;
    fn count(&self) -> usize;
    fn clear(&self) -> Result<(), RegistryError>;
}
```

### 6.6 MessageBus 接口

参见本文档 [第 5 章](#5-消息总线设计)。

### 6.7 MqttBridge 接口

参见本文档 [4.3 节](#43-mqtt-本地网桥-mqtt-bridge)。

### 6.8 IEC 104 命令处理器

```rust
#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError>;
    fn name(&self) -> &str;
}

pub struct ControlCommand {
    pub cmd_id: u16,
    pub cmd_type: CommandType,
    pub p_set: Option<f64>,
    pub q_set: Option<f64>,
    pub switch_state: Option<bool>,
    pub k_value: Option<f64>,   // 一次调频 K 值
    pub deadband: Option<f64>,  // 一次调频死区 (Hz)
    pub priority: u8,
}
```

### 6.9 MMS Client Trait

```rust
#[async_trait]
pub trait MmsClientTrait: Send + Sync {
    async fn connect(&self) -> Result<()>;
    fn disconnect(&self);
    fn get_state(&self) -> MmsClientState;
    async fn read_do(&self, ln: &str, do_name: &str) -> Result<Vec<u8>>;
    async fn write_do(&self, ln: &str, do_name: &str, value: &[u8]) -> Result<()>;
}
```

---

## 7. 文件结构

### 7.1 gateway crate

```
mupc/crates/gateway/src/
├── lib.rs                  # 模块导出，pub use iec104::*
└── iec104/
    ├── mod.rs              # 子模块声明
    ├── protocol.rs         # Iec104Frame 帧解析/编码，TypeId，Cot，Value
    ├── server.rs           # Iec104Server TCP 服务器
    ├── connection.rs       # Connection 连接管理，状态机，帧处理
    └── command.rs          # CommandHandler trait，ControlCommand
```

### 7.2 iec61850-plugin crate

```
mupc/crates/iec61850-plugin/src/
├── lib.rs                  # 模块导出
├── mms_client.rs           # MMS 客户端封装（短连接模式）
├── mms_types.rs            # MMS 数据类型（DataObject, MmsRequest, MmsResponse）
├── asn1_utils.rs           # ASN.1 BER 编码/解码工具
├── config.rs               # MmsConfig, MmsTlsConfig
├── device.rs               # Iec61850Device trait
├── goose.rs                # GOOSE 消息订阅
└── errors.rs               # Iec61850Error 错误类型
```

### 7.3 mqtt-plugin crate

```
mupc/crates/mqtt-plugin/src/
├── lib.rs                  # 模块导出
├── client.rs               # MqttClient 北向 MQTT 客户端
├── config.rs               # MqttConfig, MqttQos, TlsConfig
└── errors.rs               # MqttError 错误类型
```

### 7.4 mqtt-bridge crate

```
mupc/crates/mqtt-bridge/src/
├── lib.rs                  # 模块导出
├── client.rs               # MqttBridge trait
├── local_client.rs         # LocalMqttClient（本地 mosquitto）
├── north_client.rs         # NorthMqttClient（北向 emqx）
├── config.rs               # MqttConfig, LocalMqttConfig, NorthMqttConfig, TlsConfig, ReconnectConfig
├── topics.rs               # Topic 常量定义
└── error.rs                # MqttBridgeError 错误类型
```

### 7.5 device-trait crate

```
mupc/crates/device-trait/src/
├── lib.rs                  # 模块导出
├── device.rs               # Device trait, DeviceCommand 枚举
├── south_device.rs         # SouthDevice trait, ProtocolHandler, HplcDriver
├── message_bus.rs          # MessageBus trait, MessageHandler
├── plugin.rs               # Plugin trait, PluginState
├── plugin_loader.rs        # PluginLoader trait
├── registry.rs             # DeviceRegistry trait, DeviceQuery
├── types.rs                # Topic, Message, DataFrame, PluginMeta, Rs485Config
└── errors.rs               # BusError, DeviceError, PluginError, RegistryError
```

### 7.6 Mosquitto Docker 配置

文件结构：

```
mupc/docker/mosquitto/
├── Dockerfile              # eclipse-mosquitto:2 镜像
└── config/
    └── mosquitto.conf      # 本地 MQTT Broker 配置
```

**Dockerfile:**

```dockerfile
FROM eclipse-mosquitto:2
COPY config/mosquitto.conf /mosquitto/config/mosquitto.conf
EXPOSE 1883 8883
CMD ["mosquitto", "-c", "/mosquitto/config/mosquitto.conf"]
```

**mosquitto.conf:**

```conf
listener 1883
protocol mqtt

allow_anonymous true

log_dest stdout
log_type error
log_type warning
log_type notice
log_type information

persistence true
persistence_location /mosquitto/data/

max_connections -1
```

---

## 8. 技术决策记录

### 8.1 IEC 104 服务端模式

| 决策 | 选择 | 理由 |
|------|------|------|
| 通信模式 | **服务端** | MUPC 对调度主站而言是被控端，主站主动发起连接 |
| 帧解析 | **逐字节解析** | IEC 104 是定长/变长混合协议，无需额外序列化框架 |
| 并发模型 | **每连接一个 Task** | Tokio async 原生支持，最大 5 连接开销可控 |
| 状态管理 | **显式状态机** | IEC 104 STARTDT/STOPDT 协议要求严格状态转换 |

### 8.2 IEC 61850 MMS 短连接模式

| 决策 | 选择 | 理由 |
|------|------|------|
| 连接模式 | **短连接** | IED 设备资源有限，避免长期占用连接；简化连接管理 |
| 协议栈 | **libIEC61850 C + FFI** | 成熟的 C 实现，覆盖完整 MMS 协议栈；避免从零实现 ASN.1 |
| TLS | **可选**（MmsTlsConfig） | 配电自动化网络通常为专网，TLS 按需启用 |
| ASN.1 编解码 | **自实现 BER 编解码** | libIEC61850 处理核心 MMS PDU，辅助工具自行实现 |

### 8.3 MQTT 分层架构

| 决策 | 选择 | 理由 |
|------|------|------|
| 南向 Broker | **mosquitto** | 轻量开源，适合本地进程间通信 |
| 北向 Broker | **emqx** | 企业级 MQTT Broker，支持 TLS、集群、规则引擎 |
| TLS 要求 | **北向强制，本地不启用** | 北向经过公网，本地内网无需加密 |
| 客户端库 | **rumqttc** | Rust 原生异步 MQTT 客户端，社区活跃 |
| 连接管理 | **指数退避重连（无上限）** | 确保断线后最终恢复，适用于工业场景 |

### 8.4 消息总线演进

| 决策 | Phase 1 | Phase 3B |
|------|---------|----------|
| 实现 | `tokio::sync::mpsc` | mqtt-bridge (mosquitto + emqx) |
| 接口 | MessageBus trait（device-trait） | MqttBridge trait + MessageBus trait |
| 适用范围 | 进程内 | 进程间 + 进程内 |

### 8.5 插件体系

| 决策 | 选择 | 理由 |
|------|------|------|
| 插件 trait | **device-trait::Plugin** | 统一所有插件（IEC61850、MQTT、RS485 等） |
| 动态加载 | **libloading** | 跨平台 POSIX 支持 |
| 生命周期 | **Load → Init → Start → Stop → Unload** | 标准插件生命周期 |

### 8.6 错误处理策略

```
错误层次：
  ApplicationError  → 业务逻辑错误、策略执行失败
  ProtocolError     → IEC 104 帧错误、MQTT 协议错误
  IoError           → TCP 连接断开、超时
  DeviceError       → 设备离线、无响应
```

每层错误都实现 `std::error::Error`，支持 `error.source()` 错误链。

### 8.7 实施风险

| 风险 | 等级 | 对策 |
|------|------|------|
| IEC 61850 协议栈复杂度高 | 高 | 使用成熟开源库（libIEC61850）或 Rust 实现子集 |
| MQTT TLS 握手性能开销 | 中 | 优化连接复用，减少握手次数 |
| IEC 61850-7-420 DER 逻辑节点模型完整度 | 中 | Phase 2 实现基础读写，7-420 全模型延后至 Phase 2+ |

---

## 9. 外设数据上云（U-74，含 U-71「MQTT 真做」）设计增量

> **本章性质**：**实现级设计增量**，落 PRD [01 PRD §8](../specs/modules/01-MUPC-通信网关-PRD.md)（`[REVIEWED: PASS: 2026-09-23]`）。**不改需求**；凡与 PRD 不一致者集中登记于 §9.8，不得就地改 PRD。
> **跨文档**：本章 §9.1「最新值入口」为 **01 / 03 / 12 三份设计的共用件**（03 设计 §9.5 引用、12 设计另行引用），**唯一真源在本章**，另两份不复制数据结构、不另立门限。
> **不做的事**：外设数值上屏（U-73，12 号另立）不在本章；南向采集语义（02 号 §9）不改；落库语义（03 号）改在 03 设计 §9。

### 9.0 增量范围、现状基线与依赖方向

**现状基线（审查报告 §二/§四实证；行号按 HEAD `724225c` 全量重校，2026-09-23）**：

| # | 现状 | 证据（**已重校**） |
|---|------|------|
| 1 | IEC 104 仅上送总表 6 点，IOA 1–6 为**手写常量**（固定表 + 逐点组帧广播） | `mupc-core-bin/src/startup.rs:502-510`（固定表）、`:511-520`（`make_i_frame(0,0,…)` + `broadcast_telemetry`） |
| 2 | IEC 104 不认识 `C_IC_NA_1`；无总召 | `gateway/src/iec104/protocol.rs:50-68`（`from_u8` 无 100）、`:395-399`（用例断言 `from_u8(100) == None`）、`connection.rs:237-274`（`parse_control_command` 只认遥控/调节，`_ => None`） |
| 3 | 连接建立前无快照（只向已连接 tx 广播） | `gateway/src/iec104/server.rs:298-303` |
| 4 | MQTT 两条链路均**无生产者**；`mqtt-plugin::start()` 空实现 | `mqtt-bridge/src/north_client.rs:143`（`publish` 无调用方）、`mqtt-plugin/src/lib.rs:75` |
| 5 | MQTT 无配置承载；启用时真连 `NorthMqttConfig::default()`（假域名 + dummy 证书路径） | `mupc-core-bin/src/core_config.rs:302-314`、`startup.rs:1513-1556`（步骤 13）、`mqtt-bridge/src/config.rs:52-68` |
| 6 | 外设无可读内存快照（`meter_batt`/`hvac`/`fire`/`pcs` 的 `DataPackage` 仍 `empty_package()`） | `mupc-southd/src/mapper.rs:461-463` |
| 7 | **`south_sim_loop` 的第二条 IEC104 上送路径**：pv/load 南向模拟以 `ioa_seq` 自 1 递增发**点表外 IOA**（代码自注 FIXME） | `mupc-core-bin/src/startup.rs:1360-1409`（`:1365` `ioa_seq`、`:1390-1402` 上送支路） |

**依赖方向约束（本章方案的全部可行性依据）**：

```
mupc-southd ──→ mupc-data-processing          （已存在：mapper 用 meter_regs / DataPackage）
mupc-core-bin ──→ mupc-southd / data-processing / gateway / mqtt-bridge / strategy-engine（已存在，唯一装配者）
mupc-strategy-engine ──→ mupc-data-processing （已存在）
mupc-southd ──✗→ mupc-strategy-engine         （**禁止**，既有约束）
gateway ──✗→ mupc-southd / data-processing    （本章**不新增**该依赖）
```

⇒ 本章三处新增（最新值注册表、上送点表生成、上送驱动器）的落点由此唯一确定。

---

### 9.1 最新值入口（LV-1～LV-6）—— 01/03/12 共用件

#### 9.1.1 归属裁定（评审点名必须项）

**裁定：归属 `mupc-data-processing` crate，新增模块 `latest_values`（`mupc/crates/data-processing/src/latest_values.rs`）。**

| 维度 | 结论 | 理由 |
|------|------|------|
| **类型与所有权** | `mupc-data-processing::latest_values` | ① 03 PRD §11.6.2 R-11.6-D1 明文「**本模块**须提供…最新值快照」，而 03 模块的 crate 即 `data-processing`（03 设计 §1.2/§7.1）⇒ PRD 口径**字面成立**，无需改需求；② 依赖图上 `southd → data-processing` 与 `core-bin → data-processing` **均已存在**，**零新增边、零反向依赖、零循环**；③ 不新建 crate（KISS），且该 crate 在 CI 测试面内（`device-trait` 被 `cargo test --workspace --exclude` 排除，不适合作承载） |
| **写入方** | `mupc-core-bin` 的 `SouthSink`（实现体 `startup.rs:400-522`；`StationSink` 三个回调 `:526` / `:535` / `:583`） | 采集回调的**消费端**在装配层：southd 只负责把 `(metric, value, is_event)` 交出来（既有 `StationSink` 契约，**本增量不改 southd 的 trait**）。southd 不得依赖 strategy-engine（既有约束），也不应认识"快照"这一数据面职责 |
| **读取方** | core-bin 的 IEC 104 驱动器 / MQTT 发布器（§9.2/§9.3）、`display_host`（同进程）、策略引擎（可选，后续） | 全部满足依赖方向；**跨进程**消费（12 号 HMI 渲染进程）**不**直连本入口，仍走 12 号既有的回环 HTTP 帧通道（`display_host.rs:1114` `LoopbackHttpPublisher::serve`）——本入口在 `mupcd` 进程内，是那条通道的**上游取数点**。该"间接读"**显式登记为 LV-1 的唯一例外**，见 §9.8 C-17 |
| **不放入何处** | ❌ `mupc-southd`（会让 display/上送反向依赖南向）、❌ `mupc-gateway`（北向 crate 不应承载南向点值）、❌ `mupc-common`（基础设施 crate 不放领域类型）、❌ `device-trait`（被 CI 排除） | |

> **对 03 PRD 措辞的确认**：03 PRD 说「本模块提供」与「采集回调在 southd/core-bin」并不矛盾——**本模块（data-processing）提供类型与注册表**，**装配层（core-bin）是写入调用方**。该分工即 03 PRD R-11.6-D1 的可实现解读；03 设计 §9.5 已同步此口径。

#### 9.1.2 数据结构

```rust
// mupc/crates/data-processing/src/latest_values.rs

/// 点位质量（**与 01 PRD §8.3.3 载荷的 `q` 枚举逐字一致**，单一枚举来源）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointQuality {
    /// 有有效值且未过期
    Ok,
    /// 有值但已超过新鲜度门限
    Stale,
    /// 采集失败 / 站点离线（值可保留为最后一次有效值原值）
    Invalid,
    /// 站点未启用 / 点未配置（**从未采集过**）
    Unconfigured,
}

/// 单点最新值
#[derive(Debug, Clone, PartialEq)]
pub struct PointValue {
    /// 工程值；`None` = **不可得**（严禁以 0.0 顶替，LV-6）
    pub value: Option<f64>,
    /// **采集时刻** UTC 毫秒（不得用写入时刻 / 发送时刻顶替）。
    /// 语义分两类（**这是位点与标点的唯一差别**）：
    /// - **标量点**：每轮都读回 ⇒ `ts_ms` = **本轮轮询成功时刻**；
    /// - **位点**：southd 只按"变化沿"交付（`scheduler.rs:650-667`；落库节流 D2 口径）
    ///   ⇒ `ts_ms` = **该位最后一次变化的时刻**（"该状态自何时起有效"）。位点的
    ///   **可得性判据不是逐点的 5 s 新鲜度**（否则长跑装置接主站时，稳态位点会因"久未变化"
    ///   被整批判为过期 ⇒ 与 PRD §8.6.3「位块首轮全量 ≈319 点」直接冲突），而是
    ///   **站级轮询活性 ∧ 点位质量**，见 §9.1.3 `is_fresh` / `mark_station_polled`
    pub ts_ms: u64,
    /// 质量
    pub quality: PointQuality,
}

/// 点位主键。`metric` 与南向点名**逐字一致**（02 PRD §9.4.2.2）。
/// ⚠️ `station` 仅用于**唯一性**（同名点跨站），**不构成改名**：
/// 上云载荷的 `n` 字段只写 `metric`（§9.3.4），显示侧键同样只暴露 `metric`。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PointId {
    pub station: String,
    pub metric: String,
}

/// 读结果视图（**永远返回**，不可得也返回显式态，不返回 `None`）
#[derive(Debug, Clone, PartialEq)]
pub struct PointView {
    pub id: PointId,
    pub value: PointValue,
}

/// 一次变更批（**天然合并**：同一 `apply` 调用内的全部点打包为一批，满足 LV-4 的"允许合并"）
#[derive(Debug, Clone)]
pub struct ChangeBatch {
    /// 单调递增版本号（批序号）
    pub seq: u64,
    /// 本批变更点（按 station 分组，便于分片发布）
    pub changed: Vec<PointId>,
}
```

**快照存储**：

```rust
pub struct LatestValues {
    /// 上界 = 站数 × 站内点数（见 §9.1.6），构造期**预留容量**
    map: parking_lot::RwLock<HashMap<PointId, PointValue>>,
    /// **站级轮询活性**：站 id → 最近一次"本轮轮询成功"的时刻（ms）。
    /// 位点的可得性据此判定（见 `is_fresh`）；标量点另有逐点 `ts_ms`。
    station_poll_ms: parking_lot::RwLock<HashMap<String, u64>>,
    /// 新鲜度门限（**注入，不另立常量**）
    stale_timeout_s: u64,
    /// 变更广播
    change_tx: tokio::sync::broadcast::Sender<ChangeBatch>,
    seq: std::sync::atomic::AtomicU64,
}
```

#### 9.1.3 方法签名（实现契约）

```rust
impl LatestValues {
    /// `stale_timeout_s` 来自 `SouthStationsConfig::stale_timeout_s`（唯一真源，02 PRD §9.7.1 同源）。
    /// **禁止**在调用点传字面量 5；本结构体是判据的唯一持有者。
    pub fn new(stale_timeout_s: u64) -> Self;

    /// 订阅变更（广播容量 64；落后丢帧 ⇒ 消费方必须全量重读，见 §9.1.5）
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ChangeBatch>;

    /// 批写入（**唯一写入口**）。`samples` 内每项 (PointId, PointValue)。
    /// 返回本次是否产生了变更批（无变更 ⇒ 不广播，满足 COS 语义：值不变不发）。
    pub fn apply(&self, samples: Vec<(PointId, PointValue)>) -> Option<u64>;

    /// 单点读（不可得 ⇒ `value: None` + 显式 `quality`，**不返回 Option**）
    pub fn get(&self, id: &PointId) -> PointView;

    /// 按站取全部点（MQTT 分片发布 / 聚合求值用）：返回该站**全部已登记点**（含不可得态）。
    /// **单次持读锁克隆**（聚合求值须一次拿到 288 位，不得逐点 `get` —— 那会取 288 次锁）。
    pub fn station_snapshot(&self, station: &str) -> Vec<PointView>;

    /// 全量取（快照 / 总召 / 首轮 COS 用）
    pub fn all(&self) -> Vec<PointView>;

    /// **本站在采集活性窗口内**（`now - station_poll_ms[station] <= stale_timeout_s*1000`）。
    /// 站未知（从未轮询成功）⇒ `false`。
    pub fn station_is_active(&self, station: &str, now_ms: u64) -> bool;

    /// **该站最近一次成功采集时刻**（= `station_poll_ms[station]` 的公开读口）。
    /// `None` = **从未轮询成功**（含已被 `mark_station_offline` 清除者）——消费方须显示
    /// 不可得（如 `--`），**不得**臆造、**不得**自维护第二份真源。
    ///
    /// **接口要求 `R-38` 的落地登记（v1.13 补，评审偏离①）**：该 getter 是 12 号设计
    /// **§15.1.2 / §15.9 `R-38`** 提出的"对 01 的接口要求"（外设站状态条要显示"最后成功
    /// 时刻"），实现已就位（`latest_values.rs:249`）但 §9.1.3 全表原未列 ⇒ 令"方法签名全表"
    /// 失真。**本行即其登记**（实现签名以 T1 为准：`pub fn station_last_poll_ms(&self,
    /// station: &str) -> Option<u64>`）。
    ///
    /// ⚠️ **它不自建第二套新鲜度真源**：本 getter **不新增任何判据** —— 过期判据仍**唯一**由
    /// [`is_fresh`] / [`station_is_active`] 持有，真源仍是注入的 `stale_timeout_s`（**5 s**，
    /// 02 PRD §9.7.1 同源）；本方法只是**同一字段的读取口**（12 设计 §15.1.2 亦已按此口径登记）。
    pub fn station_last_poll_ms(&self, station: &str) -> Option<u64>;

    /// **活性刷新（唯一调用点 = `SouthSink` 每次站轮成功）**：更新 `station_poll_ms[station] = now_ms`。
    /// 只刷新"站在采"这一事实，**不改任何点值/点位时标** ⇒ 不产生变更批、不影响 COS 语义。
    pub fn mark_station_polled(&self, station: &str, now_ms: u64);

    /// 有效点判定（**过期判据单一实现**）：
    /// `v.quality == Ok` **且**
    /// - 标量点：`now - v.ts_ms <= stale_timeout_s*1000`；
    /// - 位点：`station_is_active(站, now)`（位点时标语义 = 最后变化时刻，见 `PointValue.ts_ms`）。
    ///
    /// 由调用方给出 `id.station` / 是否为位（`is_bit`），避免本类型猜语义：
    /// `pub fn is_fresh(&self, id: &PointId, v: &PointValue, is_bit: bool, now_ms: u64) -> bool;`
    pub fn is_fresh(&self, id: &PointId, v: &PointValue, is_bit: bool, now_ms: u64) -> bool;

    /// 站点离线：该站全部点 `quality = Invalid`（**保留原值与原始时标**，口径 = PRD EX-1）；
    /// 同时清 `station_poll_ms[station]`（恢复在线前 `station_is_active == false`）。
    pub fn mark_station_offline(&self, station: &str);
}
```

#### 9.1.4 写入侧口径（core-bin `SouthSink` 装配点）

| 采集事件（既有接缝，**不改 southd**） | 写入动作 |
|---|---|
| `StationSink::on_grid_package(pkg)`（`startup.rs:526`） | 从 `DataPackage.electrical` 顶层字段映射 **6 个派生量**（点名见 §9.2.1 段 1）→ `apply`；`ts_ms = Utc::now()`（与既有 grid 上送支路同刻）；`AiIntegrator.set_latest_data` **原样保留**（§9.1.7）。并 `mark_station_polled(id, now)` |
| `StationSink::on_station_telemetry(id, role, pts)`（`startup.rs:535`） | ① **先** `mark_station_polled(id, now_ms)`（**本站在采**，与点数无关；`online` 合成事件的那次调用同样刷新）；② `is_event == false` 的每一项 → `apply`（**含位点**，LV-5 要求位块点同样有入口）；③ `is_event == true` 的项**不写**快照（它们是站级/信号级事件，归 `events` 表，不属"点位最新值"）；④ **`role == Battery` ⇒ 本批 `apply` 之后立即求值 15 组聚合并 `apply`**（§9.2.1.1，写入侧求值点） |
| `StationSink::on_station_offline(id, role, reason)`（`startup.rs:583`） | `mark_station_offline(id)`（全站置 `Invalid`、清站活性，**保原值**，EX-1） |
| 站恢复（`online` 事件，`scheduler.rs:743`） | 下轮 `on_station_telemetry` 自然把点刷回 `Ok` 并刷新站活性（**无需**单独的 online 钩子） |

> **时标语义**：标量点 `ts_ms` 取"该轮轮询成功时刻"（`Utc::now()` 在 `SouthSink` 内取，与既有落库路径 `startup.rs:562` 的 `Utc::now()` 同一时刻口径）。southd 的 `mapper` 逐点不携带独立时标（`TelemetrySample` 无时标字段），因此**同一轮交付的全部标量点共用同一 `ts_ms`** —— 这与 02 PRD 的"站轮次"语义一致，且满足 LV-3 与 EX-1（保留原始时标）。
>
> **位点为何走站级活性**（**本节新增裁定，替代原"全部点统一 5 s 新鲜度"**）：`scheduler` 对位点**按变化沿交付**（`scheduler.rs:650-667`：稳态只交变化位 + 首轮/恢复全量），故位点的逐点 `ts_ms` 恒等于"最后变化时刻"。若可得性仍按逐点 5 s 判，则装置连续运行 > 5 s 后，任何未变位的位点在主站**重连初始快照**中都会被判过期而整体消失 —— 与 PRD §8.6.3「位块首轮全量 ≈319 点」、§8.4 GI-5/GI-6 的意图（防"陈旧值冒充实时值"）**均相悖**。裁定：**位点的可得性 = 站级轮询活性 ∧ `quality == Ok`**；逐点 `ts_ms` 仍如实保留为"最后变化时刻"并用于载荷 `ts`（"该状态自何时起有效"）。该裁定只改**判据**，不改任何"不造假值"口径。

#### 9.1.5 变更通知与消费契约

```rust
// 消费方标准形态（IEC104 / MQTT / 未来 12 号外设页）：
let mut rx = latest.subscribe();
loop {
    match rx.recv().await {
        Ok(batch) => { /* 增量：只处理 batch.changed */ }
        Err(RecvError::Lagged(_)) => { /* **必须**全量重读 latest.all() 重建基线 */ }
        Err(RecvError::Closed) => break,
    }
}
```

| 契约 | 值 | 依据 |
|------|-----|------|
| 通知时延 | 写入 `apply` → 订阅方唤醒 **≤ 1 s**（实测为微秒级；1 s 是 PRD LV-4 的上界口径） | 01 PRD LV-4 / 03 PRD R-11.6-D4 / 12 PRD RQ-9.0-2（同口径） |
| 合并 | **允许**：一次 `apply` = 一批；跨 `apply` 不合并 | PRD LV-4 |
| 丢帧 | `Lagged` ⇒ 消费方**全量重读**（"允许丢帧、不允许静默传输损坏数值"，12 PRD §4.4） | 12 PRD §4.4 |
| 值不变 | **不广播**（`apply` 内逐点比较 `value`，全部相同 ⇒ 返回 `None`）——这是 MQTT/IEC104 的 **COS 语义实现点** | PRD §8.6.3 C 档 |
| 值不变时的时标 | **仍更新 `ts_ms` / `quality`**（`apply` 无条件 upsert 这两个字段）——"不广播"**不等于**"不刷新时标"。理由：标量点每轮都被读回，"本轮读到"就是它的采集事实；若值不变即冻结时标，恒定值会在 5 s 后集体变 `stale` 而从上送中消失（与 PRD EX-1 的"站离线才判无效"相悖） | 本设计裁定（与 §9.1.4 位点裁定同批） |

#### 9.1.6 并发、锁与内存上界

| 项 | 设计 | 说明 |
|----|------|------|
| 锁 | `parking_lot::RwLock<HashMap<…>>` | 与 `mupc-southd`/`data-processing` 既有选型一致；**读多写少**（写每站每轮一次、读含上送/组帧多路） |
| 持锁边界 | `apply` 内只做 upsert + 差异收集，**不持锁跨 await**（`broadcast::send` 在释放锁后调用） | 防死锁 / 阻塞采集 |
| 读者 | `get`/`station_snapshot`/`all` 持读锁克隆所需子集后即释放 | 单次全量 **639 点**克隆 ≈ 数十微秒；**聚合求值走一次 `station_snapshot("bms")`**（**345 点** = 57 标量 + 288 位；⚠️ 原稿"289 点"系 288 位 + 1 个重复计数的笔误，随评审勘误②一并订正），不得逐点取锁 |
| 内存上界（常驻） | **639 点**（= MQTT 全量 624 ∪ IEC104 聚合 **15**，PCS 启用；未启用 **567 点**，§9.2.1.0）× `PointId`(≈48 B 堆 + 24 B 结构) + `PointValue`(8+8+1 + 对齐) ≈ **≈ 75–95 KB**；`HashMap` 负载因子开销后上界 **≤ 128 KB** | 与 02 PRD §9.8.3 的"南向增量 < 100 KB"**同量级**；设计值按 128 KB 声明（含预留容量）。**接受**：639 点在装置内存（RK3588，GB 级）上可忽略（较 636 点 +3 条，量级不变） |
| 上界保障 | `apply` **不做**"点数上限裁剪"（点数由配置唯一决定，配置期已校验）；`HashMap` 在构造期 `with_capacity(点表规模)` 预留；站活性表另 ≤ 站数 条 | 无动态增长 ⇒ 无 OOM 面 |

#### 9.1.7 与既有数据面的边界（**防第二真源**）

| 既有面 | 边界（**本增量不改它、也不被它替代**） |
|---|---|
| `AiIntegrator::set_latest_data`（`strategy-engine/src/ai_integration.rs:186`） | 承载 **grid 的 `DataPackage`（含分相）**，用途是**策略 phase 输入**（`on_grid_package` 是唯一写方，M-4 防双写）。**保留不动**。快照中的 grid 6 点是**同一 `DataPackage` 的派生子集**，在同一次 `on_grid_package` 内写入 ⇒ **同一写方、同一时刻，不构成双源** |
| `AiIntegrator::set_battery_soc`（`:218`） | SOC 双源裁决（04 §2.11.1）**保留不动**；快照里 `soc` 点是**另一用途**（上送/上屏），其值取自同轮 `on_station_telemetry` 的 `soc` 点（`mapper` 解码后同值） |
| `display_host` 的 `latest: SharedLatest` + `SlowCaches`（`display_host.rs:681/697`） | 是**消费端派生视图**（显示帧 + 慢拍段缓存），**不是**第二份点值真源。本章**不改** 12 号帧契约；U-73（外设上屏）落地时**必须**读本快照，不得自建缓存 |
| `DataCollectorImpl::latest_data`（`data-processing/src/collector.rs:19`） | 只覆盖 **intercore 侧**（核间）数据，与南向外设**不同域**（03 PRD §8.5 口径）。**保留不动**，本快照**不并入**它（并入会把两个生命周期混成一个） |
| `WriteBuffer` / `telemetry` 表 | **历史**通道，与实时值**不是一回事、不得互相替代**（01 PRD §8.5 / 03 PRD R-11.6-A）。本章上送路径 **LV-2 禁轮询 DB** |

#### 9.1.8 装配点（core-bin）

| 项 | 落点 |
|---|---|
| 构造 | `startup.rs` 步骤 8（南向调度装配）之前：`let latest = Arc::new(LatestValues::new(config.south_stations.stale_timeout_s));` |
| 注入 `SouthSink` | `SouthSink::new(..., latest: Arc<LatestValues>, grid_station_id: Option<String>)`（**新增第 6 / 第 7 两个入参**；构造点 `startup.rs:1287` 调用处、定义 `startup.rs:414-430`）<br>**`grid_station_id` 的理由（v1.13 补登记，评审偏离②）**：§9.1.4 要求 `on_grid_package` 内 `mark_station_polled(id)`，而**该回调的入参不含站 id**（既有 `StationSink` 契约），且 §9.1.1 **明禁改 `StationSink` trait** ⇒ 站 id 只能在**装配期**从 `south_stations.grid_station()` 解析一次并作为构造参数注入。`None` = 未配 `meter_grid` ⇒ 该回调**不写快照**（**不臆造站 id**）。实现：`startup.rs:420`（字段）/ `:424-443`（`new`）/ `:456-459`（`Some` 分支早返回）。**私有类型、不改公开面、不改任何语义、不触落库路径** |
| 注入上送器 | §9.2 的 `Iec104UplinkDriver`、§9.3 的 `MqttUplinkPublisher` 各持 `Arc<LatestValues>` |
| 点表生成 | §9.2.1 的 `build_uplink_points(&config.south_stations)`，装配期调用一次；**失败 ⇒ 拒启动**（`Err` 冒泡，与 `validate_south_stations`（`core_config.rs:679`）同范式） |
| **站活性刷新** | 三个 `StationSink` 回调（`startup.rs:526/535`）内 `mark_station_polled(id, now_ms)`；无需新增任务 |
| **聚合求值** | `role == Battery` 时在 `on_station_telemetry` 末尾调 `uplink::evaluate_bms_aggregates(..)`（纯函数，§9.2.1.1） |
| 验收映射 | AC-U74-12 / AC-U74-13（mock 消费方零 SQL、不可得显式化） |

---

### 9.2 IEC 104 上送（234 点 / 总召 / 初始快照）

#### 9.2.1 上送点表：机械生成（消灭"手写 IOA 常量"）

**生成器落点**：`mupc/crates/mupc-southd/src/uplink.rs`（新增模块）。

**为什么放 southd**：IOA 的**契约**由 01 PRD §8.3.2 拥有，但它的**输入**是南向点表的两项唯一真源——`points::expand`（点名与序号）与 `point_table`（位语义）。放在同 crate 可保证"**校验期与运行期同一函数**"这一既有不变量（`points.rs:1-16` 的同一原则），且 **southd 不新增任何依赖**（输出是纯数据）。

```rust
// mupc/crates/mupc-southd/src/uplink.rs
pub const SEG_GRID: u32 = 0;        // IOA = 段基址 + 段内 1 基序号
pub const SEG_METER_BATT: u32 = 100;
pub const SEG_BMS_TELEM: u32 = 200;
pub const SEG_BMS_ALARM: u32 = 300;
pub const SEG_PCS: u32 = 400;
pub const SEG_FIRE: u32 = 500;
pub const SEG_HVAC: u32 = 600;

/// 上送档位（PRD §8.6.3 A/B/C 档，**唯一分档真源**）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataClass { A, B, C }

/// **上送通道**（本章新增）：两条北向通道的点集**不同**，必须逐点带掩码
/// （IEC104 = 调度子集 234；MQTT = 全量 624；两者并集 = 639，含 IEC104 独有的 15 聚合）。
pub struct ChannelMask(pub u8);
impl ChannelMask {
    pub const IEC104: ChannelMask = ChannelMask(0b01);
    pub const MQTT:   ChannelMask = ChannelMask(0b10);
    pub const BOTH:   ChannelMask = ChannelMask(0b11);
}
impl ChannelMask { pub fn has(self, c: ChannelMask) -> bool { self.0 & c.0 != 0 } }

/// 上送条目（**含逐点 IOA 与通道归属**）
pub struct UplinkPoint {
    /// IEC104 段内 IOA；**仅当 `channels.has(IEC104)` 时有意义**（MQTT-only 点为 0）
    pub ioa: u32,
    pub station: String,   // 站 id（MQTT 分片用）
    pub metric: String,    // 南向点名或聚合点名
    pub kind: UplinkKind,  // Scalar | Bit
    pub class: DataClass,
    pub channels: ChannelMask,
    /// 语义标签（中文，供对点清单与日志；来自 `point_table::label` 或本模块的聚合表）
    pub label: &'static str,
}

/// 生成**并集**上送条目（含 15 个 IEC104 独有聚合）。**失败即拒启动**（配置/点表漂移的 fail-fast 点）。
pub fn build_uplink_points(cfg: &SouthStationsConfig) -> Result<Vec<UplinkPoint>, String>;

/// **BMS 15 组聚合的运行期求值**（纯函数，无 IO / 无锁）：入参 = 位查询闭包，
/// 出参 = `(metric, 值可选, 质量)`；完整语义见 §9.2.1.1。
/// ⚠️ `PointQuality` 的所属 crate 是 `mupc-data-processing`（`southd → data-processing` 依赖已存在），
/// 故此处用 `mupc_data_processing::latest_values::PointQuality`（**不新增依赖边**）。
pub fn evaluate_bms_aggregates(
    lookup: &dyn Fn(u16 /*位地址*/) -> Option<(f64, mupc_data_processing::latest_values::PointQuality)>,
) -> Vec<(&'static str, Option<f64>, mupc_data_processing::latest_values::PointQuality)>;
```

**9.2.1.0 两条通道的点集（新增，消除"同一份点表"的歧义）**：

| 通道 | 点集 | 点数（PCS 未启用 / 启用） | 说明 |
|------|------|--------------------------|------|
| **IEC 104** | 段 1–7（§8.3.2 分段）| **162 / 234** | 不含 288 个 BMS 告警位与 114 个消防探测器明细（BMS 以 **15** 聚合替代，PRD §8.3.1；**v1.4-r2**：12 → 15，见 §9.2.1.2） |
| **MQTT（北向）** | 全量 618 + 总表 6 | **552 / 624** | 不含 **15** 个 IEC104 聚合（聚合是**调度侧派生量**，物联平台订阅 288 个原始位）⇒ **本行不受裁定 A 影响** |
| **并集**（快照与点表 JSON 的规模） | IEC104 ∪ MQTT | **567 / 639** | 快照内存上界按此计（§9.1.6） |

> **该表的必要性**：原稿写"MqttUplinkPublisher 持与 IEC104 **同一份** `Vec<UplinkPoint>`"，但两者点集**不同**（234 ≠ 624）。若不区分，任一侧必然点数错。故 `UplinkPoint` 带 `channels`，`build_uplink_points` 产**并集**，两个上送器各按 `channels.has(..)` 过滤；`build_uplink_points` 的**点数断言**即上表三行（生成期自检项之一）。

**生成算法（唯一，可机械执行）**：

```
for 站 in cfg.stations（按 cfg 顺序）:
    role = 站.role
    if role == MeterGrid:
        段 = SEG_GRID;  条目 = GRID_DERIVED_6  (channels = BOTH)   // 见下表（不按 points::expand）
    else:
        段 = 段基址(role)
        标量 = 站.blocks.flat_map(points::expand).filter(kind==Scalar)      // 按 (block.addr, 块内偏移) 升序
        位点 = 站.blocks.flat_map(points::expand).filter(kind==Bit)         // 按位地址升序
        按 role 施加**子集规则**（下表）:
           Battery   → 标量全量(BOTH) + 位点**不进 IEC104**（MQTT-only）
                       + **15 个聚合**（IEC104-only，段 4；v1.4-r2 由 12 增为 15）
           MeterBatt → 标量全量(BOTH)
           Pcs       → 标量全量(BOTH)（**站未配置在 cfg 中时整段不产条目**）
           Fire      → `fire_sys` 13 点(BOTH) + `fire_det` 114 点(MQTT-only)
           Hvac      → `hvac_di` 31 点(BOTH) + `hvac_in` 3 点(MQTT-only)
    IEC104 段内序号 = 1..n（标量在前、位点在后；段 4 的 15 聚合即段内序 1..15）
    IOA = 段 + 段内序号（仅 IEC104 侧计数；MQTT-only 点不占 IOA）
for 每条: 生成期自检（下表"自检"列），任一失败 → Err(点名 + 原因)
```

**子集规则与自检**：

| role | 段基址 | 条目来源（IEC104 侧） | 点数 | 生成期自检（失败即拒启动） |
|------|--------|----------------------|------|---------------------------|
| `meter_grid` | 0 | **派生 6 点**（固定表，见下） | 6 | 无（静态表） |
| `meter_batt` | 100 | `points::expand` 标量全量 | 40 | 点数 == 40（否则拒绝启动并报实测值） |
| `battery`（遥测段） | 200 | 标量全量 | 57 | 点数 == 57 |
| `battery`（遥信段） | 300 | **聚合**（`BMS_ALARM_GROUPS`） | **15** | 见 §9.2.1.3 的 **G-1～G-4 四条**（引用位存在 / 组间互斥 / **扣减排除表后覆盖 Alarm ∪ State** / 排除表逐位类别与 `point_table` 一致） |
| `pcs` | 400 | 标量全量 | 72 | 点数 == 72；**站不在 cfg ⇒ 不产条目**（EX-7 由此天然成立，无需运行期特判） |
| `fire` | 500 | 仅 `fire_sys` 块 | 13 | 块名 `fire_sys` 必须存在；`fire_det` 块**不得进 IEC104**（点数断言 == 13） |
| `hvac` | 600 | 仅 `func: discrete` 块 | 31 | 点数 == 31；`hvac_in` 块**不得进 IEC104** |
| 全局 | — | 并集 | 567 / 639 | **三通道点数断言**（§9.2.1.0 表：IEC104 **162/234**、MQTT 552/624、并集 **567/639**） |

**段 1（grid 6 点，IOA 1–6）——固定表（"现场追认"口径，**不得改号**）**：

| IOA | metric（= 既有命名） | 来源字段 | 档 |
|-----|---------------------|----------|-----|
| 1 | `active_power` | `DataPackage.electrical.active_power`（= `p_total`） | A |
| 2 | `reactive_power` | `.reactive_power`（= Σq） | A |
| 3 | `voltage` | `.voltage`（**现取 A 相** `u[0]`，`mapper.rs:160`） | A |
| 4 | `current` | `.current`（**现取 A 相幅值** `mapper.rs:161`） | A |
| 5 | `cos_phi` | `.cos_phi`（**现取 A 相** `mapper.rs:164`） | A |
| 6 | `frequency` | `.frequency`（**现为常量 50.0**，`mapper.rs:165`） | A |

> ⚠️ 上表 3/4/5/6 的"取 A 相 / 常量"是**现状如实登记**（与 03 PRD Q5 同源问题）。本设计**不改**其口径（改则破坏"现场追认"），但它们**进 MQTT 全量**（§9.3.4）⇒ 05 号/Q5 的裁定同样影响上云。登记于 §9.8 C-13。

#### 9.2.1.1 15 组聚合的**运行期求值点**（P1：原稿缺此定义 ⇒ 234 点不可得）

**问题**：原稿只写"写入侧写单点（288 个 `bms_alarm_*`）、读侧只查值"，**没有任何一处把 288 位求值成 12 个聚合点** ⇒ 段 4 的 12 个 IOA 在快照里永远不存在。**（v1.4-r2：聚合组由 12 增为 15，求值点与机制**一字不变** —— 仍是同一纯函数、同一触发时机、同一唯一写入口，仅 `BMS_ALARM_GROUPS` 常量由 12 项增为 15 项。）**

**裁定（三句闭合）**：**在写入侧求值；每轮站轮成功后触发一次；结果以聚合点名走同一个 `apply` 进快照。**

| 项 | 设计 | 依据 |
|----|------|------|
| **在哪一侧** | **写入侧**（`mupc-core-bin::SouthSink`，装配层）；求值器是 **`mupc-southd::uplink::evaluate_bms_aggregates`**（纯函数，与聚合表同 crate ⇒ **生成期与运行期同一份常量**，与 `points.rs:1-16` 的同源不变量一致） | 读侧求值会给"每个读方各算一遍"制造第二真源；放 southd 保证"表在哪、算在哪" |
| **触发时机** | `StationSink::on_station_telemetry(id, role, pts)` 内，**`role == Battery`** 且 **①`mark_station_polled` ②本批 `apply` 之后**立即执行。`scheduler` 对同一站**每轮至多调用两次**（遥测一次 `scheduler.rs:669`、事件一次 `:683`）⇒ 求值一次或两次，**幂等**（见下"不变不发"） | 本轮标量/变位位都已写入 ⇒ 求值看到的是**本轮的一致视图**；不必另起定时任务 |
| **求值输入** | **快照里该站 288 个位点的当前值**（一次 `station_snapshot("bms")` 取读锁，**不得逐点 `get`**）—— **位地址 `199+k` ↔ 点名 `bms_alarm_k`（k = 1..288）**（`bms_alarm` 块 `addr: 200` + `points::expand` 的 `positional()` ⇒ 块内偏移 = 位地址 − 200，序号 = 偏移 + 1；逐位登记见 `point_table.rs:387-675`） | **不能用"本批交付的位"求值**：位点按变化沿交付（`scheduler.rs:650-667`），稳态轮里没有未变位 ⇒ 按批求值会漏掉已置位的位（假 0） |
| **结果如何进快照** | 以 `PointId { station: 站 id（= "bms"）, metric: "bms_aggr_*" }` 交 **唯一写入口 `LatestValues::apply`**，与其它点同批/同刻；`ts_ms = Utc::now()`（本轮） | 走唯一入口 ⇒ C 档变更订阅、IEC104 总召/快照、MQTT（若订阅）**天然可见**；不存在第二写方 |
| **COS** | 值不变 ⇒ `apply` 不广播（§9.1.5）⇒ 不会因为"每轮都算"而刷屏 | 同上 |
| **停机/离线** | 站离线由 `mark_station_offline(id)` 把该站**全部点**（含聚合点，因同 station）置 `Invalid` 并保原值原时标 ⇒ 与 EX-1 一致，**无需**聚合专用分支 | §9.1.3 |

**求值规则（写进 `evaluate_bms_aggregates` 的文档注释，可机械实现）**：

```
对每组 g:
  bits = g.位地址列表（BMS_AGGR_GROUPS）
  可读 = { (v, q) | lookup(addr) == Some((v,q)) }，无值（None）= 不可得
  若 可读 为空                     → (metric, None, 站位质量)     // 不可得：不写值，保原值
  否则 若 全部可读位 q == Ok
       且 无缺位（可读.len() == bits.len()） → (metric, Some(OR(位)) , Ok)
  否则（存在 q != Ok 的可读位 **或存在缺位**） → (metric, Some(OR(位)) ,
                                          worst(q of 该组可读位; 缺位按 Unconfigured))   // Ok < Stale < Invalid < Unconfigured
```

> **⚠️ 勘误（v1.4-r4，2026-09-24；T11 实现 + 两轮取证）**：本文的**伪码分支 2** 原条件只写"**全部可读位** `q == Ok`"，**与同节正文（下一行"不可得的位…通过质量降级表达'这一组未读全'"）矛盾** —— 缺位（`None`）时按伪码字面会返回 `(Some(OR), Ok)`，即"6 位里 1 位读不到、其余 5 位全 0"⇒ 主站看到 `Ok + 0` = **确证无告警**，正是 PRD §8.3.2 末注与 EX-6 要禁的**静默漏报**。**已按正文口径修正伪码**（加"且无缺位"，缺位按 `Unconfigured` 参与取最差）：三处合同表述（本节正文 + §9.5「组内部分缺位 ⇒ 质量降级且不写 0」）本已一致，**伪码是欠规格**。**实现已按修正后口径**（`uplink.rs` 的 `evaluate_bms_aggregates`，注释登记该口径差）；**判别锚** = `evaluate_partial_missing_degrades_quality_and_never_fabricates_zero`（改回伪码旧字面口径 ⇒ 该用例与 `evaluate_or_semantics_*` **恰好 2 条红**，已注入验证）。

- **不可得的位不当作 0**（不参与 OR，且通过**质量降级**表达"这一组未读全"）；全组不可得 ⇒ `None`（**严禁写 0**，PRD §8.3.2 末注 / EX-6）。
- 质量降级时**保留原值**：写入侧按 `PointValue{ value: 上轮值, ts_ms: 上轮时标, quality: 降级后质量 }`（= EX-1 "保留原值与原始时标"）。
- `OR` 语义 = "只并粒度、不丢语义"（任一位 1 ⇒ 聚合 1），**不是**"取最重一级"。

#### 9.2.1.2 聚合组定义（段 4，IOA 301–312）

| 段内序 / IOA | metric | 聚合位（`point_table` 位地址） | 语义 | 含位类别 | 位数 |
|-----|--------|------------------------------|------|----------|------|
| 1 / 301 | `bms_aggr_cluster_voltage` | 201–206 | 簇端电压 欠压/过压 × 轻/中/重 | Alarm | 6 |
| 2 / 302 | `bms_aggr_cluster_current` | 207–212 | 簇端充/放电电流 × 轻/中/重 | Alarm | 6 |
| 3 / 303 | `bms_aggr_cell_voltage` | 213–218 | 单体 欠压/过压 × 轻/中/重 | Alarm | 6 |
| 4 / 304 | `bms_aggr_cell_temp` | 219–224 | 单体 欠温/过温 × 轻/中/重 | Alarm | 6 |
| 5 / 305 | `bms_aggr_soc_low` | 225–227 | SOC 低 × 轻/中/重 | Alarm | 3 |
| 6 / 306 | `bms_aggr_soh_low` | 228–230 | SOH 低 × 轻/中/重 | Alarm | 3 |
| 7 / 307 | `bms_aggr_cell_spread` | 231–236 | 单体压差 + 温差 × 轻/中/重 | Alarm | 6 |
| 8 / 308 | `bms_aggr_slave_comm_lost` | 237–276 | 从控 1–40 通讯失联（**逐从控位聚合**） | Alarm | 40 |
| 9 / 309 | `bms_aggr_terminal_pack` | 277–285 | 端子/箱体温度过高 ×3 + PACK 电压过/低 ×3 | Alarm | 9 |
| 10 / 310 | `bms_aggr_acquire_fault` | 286–287 | 单体电压/温度采集故障 | Alarm | 2 |
| 11 / 311 | `bms_aggr_slave_di` | 299–339 | 从控 DI 告警（风扇/气溶胶/MSD）+ 逐从控 DI 定制告警 | Alarm | 41 |
| 12 / 312 | `bms_aggr_run_abnormal` | 289,293,294,295,296 | 运行异常：充电态 289 + 禁充 293 + 禁放 294 + 充放禁止 295 + 故障 296 | State | 5 |
| **13 / 313** | **`bms_aggr_cluster_level`** | **424–426** | **簇级告警 一级/二级/三级**（v1.4-r2 裁定 A 新增） | **Alarm** | **3** |
| **14 / 314** | **`bms_aggr_relay_stuck`** | **454–459** | **继电器粘连：总正 / 总负 / 预充 / 风扇 / 休眠 / 断路器**（v1.4-r2 裁定 A 新增） | **Alarm** | **6** |
| **15 / 315** | **`bms_aggr_afe_fault`** | **460** | **AFE 故障**（v1.4-r2 裁定 A 新增） | **Alarm** | **1** |

> **覆盖合计 143 位**（36+40+9+2+5+41 = 133，另加 v1.4-r2 的 3+6+1 = 10；其中 **Alarm 138 + State 5**）。指数区间逐位与 `point_table.rs:387-675` 的 `BitClass` 一致。
> **v1.4-r2 裁定 A 的逐位清单（10 位，从 `BMS_AGGR_EXCLUDED` 移入 `∪组`）**：`424 簇一级告警`、`425 簇二级告警`、`426 簇三级告警`（`mupc/crates/mupc-southd/src/point_table.rs:612-614`）；`454 总正继电器粘连`、`455 总负继电器粘连`、`456 预充继电器粘连`、`457 风扇继电器粘连`、`458 休眠继电器粘连`、`459 断路器粘连`（`:642-647`）；`460 AFE 故障`（`:648`）。全部 `BitClass::Alarm`，**逐位已回源核实**。
> **为什么是"3 个聚合组"而不是"10 个单列 IOA"**（落地形态裁定）：① 段 4 的**需求契约**是"BMS 遥信（**告警位聚合**）"（PRD §8.3.2 段 4 行）——单列 10 位会把段 4 从"聚合段"变成"聚合 + 单列混合段"，**属改需求语义**，非本文档授权范围；② 与既有 12 组同构，**点数与 IOA 契约变化最小**（+3 IOA / +10 IOA）；③ 满足"**只并粒度、不丢语义**"（任一位 1 ⇒ 组内聚合 1，不漏报）；④ 三类**各自成组**（而非合成 1 组），保证"簇级告警 / 继电器粘连 / AFE 故障"**三类之间不丢语义**（跨类合并会违反第 ③ 条）。**备选方案**（合并为 2 组 / 单列 10 位）登记为 **§9.8 Q-B-1 待裁定**。

#### 9.2.1.3 生成期自检（P2：原稿"覆盖 Alarm ∪ State"与排除表矛盾 ⇒ 按字面实现会拒启动）

**原稿缺陷**：自检写"并集必须覆盖 `Alarm ∪ State` **全部位**"，但排除表本身剔除了 88 位（原稿只列出 28 位，且把 364–423/484–487 与 **424–483** 混为一行"Reserved"，而**代码里 424–483 是 `Alarm`**）⇒ 自检与排除表**互相矛盾**：按字面实现，`build_uplink_points` 必 `Err` ⇒ **拒启动**。**（v1.4-r2：该 60 位中的 10 位已由裁定 A 移入 `∪组` ⇒ 排除表降为 78 位、覆盖升至 143 位，见下表。）**

**修正后的自检（`BMS_AGGR_EXCLUDED` 成为自检的显式扣减项）**：

```rust
/// 显式排除表：**只列 `Alarm ∪ State` 中"有意不入聚合"的位**（Reserved 不在判据内，不列）。
/// 每行 `(位地址, 期望 BitClass, 理由)`；`期望 BitClass` **必须与 `point_table` 实际值相等**，
/// 否则生成期 `Err`（防"把 Alarm 误标 Reserved"这类静默漏报）。
pub const BMS_AGGR_EXCLUDED: &[(u16, BitClass, &str)] = &[ /* 78 行，见下表 */ ];
```

| 自检 | 断言（生成期，任一失败 ⇒ `Err(点名/地址 + 原因)` 并拒启动） |
|------|------------|
| **G-1 引用存在** | 组内每个位地址必须存在于 `points::expand(bms_alarm)` 的位集合中，且其 `BitClass ∈ {Alarm, State}`（`point_table::lookup` 可查） |
| **G-2 组间互斥** | **15** 组的位集合两两不交（`∪组` 的基数 == 各组基数之和 == **143**） |
| **G-3 扣减后全覆盖** | `(Alarm ∪ State) − BMS_AGGR_EXCLUDED == ∪组`（**集合相等**，不是"覆盖"）—— 等价计数式 **`card(A∪S) == 143 + card(EXCLUDED) == 143 + 78 == 221`** |
| **G-4 排除表一致** | ① `BMS_AGGR_EXCLUDED` 与 `∪组` 不交；② 每行的 `期望 BitClass` == `point_table` 实际类别；③ 排除表**逐位展开**后的基数 == **78**（防"用一段区间糊过去"） |
| **G-5 通道点数** | §9.2.1.0 的三行断言（IEC104 **162/234**、MQTT 552/624、并集 **567/639**） |

**v1.4-r2 重算全量（可复算，改变量逐项列出）**：

| 量 | v1.4（12 组） | **v1.4-r2（15 组）** | 算式 |
|----|---------------|---------------------|------|
| `∪组` 覆盖位数 | 133 | **143** | 133 + (3+6+1) |
| 其中 Alarm / State | 128 / 5 | **138 / 5** | +10 Alarm |
| `card(EXCLUDED)` | 88 | **78** | 88 − 10 |
| 其中 Alarm / State | 84 / 4 | **74 / 4** | −10 Alarm |
| **`card(A∪S)`** | 221 | **221（不变）** | 143 + 78 = 133 + 88 |
| IEC104 点数（未启用 / 启用） | 159 / 231 | **162 / 234** | +3（段 4 12→15） |
| MQTT 点数 | 552 / 624 | **552 / 624（不变）** | 聚合为 IEC104-only |
| 并集 | 564 / 636 | **567 / 639** | +3 |
| A/B/C 档 | 19/84/56 | **19/84/59** | C 档 56→59 |
| 总召/快照突发 | 3,975 / 5,775 B | **4,050 / 5,850 B** | ×25 B/帧 |
| 帧长、稳态带宽、高峰带宽 | 25/22/18 B；917 / 1,337 B/s | **不变** | 带宽由变位率假设驱动，与点数无关 |

**逐位排除表（78 行按类别归并，但**代码里必须逐位展开**——区间只是文档的简写）**：

| 位地址（逐位） | 位数 | 语义 | 类别（**与代码一致**） | 不入聚合的理由 |
|--------|------|------|------|-----------|
| 288 | 1 | 簇初始状态 | **State** | 正常态而非异常；"是否在充/放"由 `bms_io_1` 簇状态枚举 + 聚合 312 组合表达 |
| 290 | 1 | 簇放电 | **State** | 正常态 |
| 291 | 1 | 簇就绪 | **State** | 正常态 |
| 298 | 1 | 簇高压箱状态 | **State** | **待产品确认**（若需上主站：段 4 在原 12 组基础上扩至 **316**，改表不改码） |
| 340–363 | 24 | 单体充/放电过温欠温 ×3、温升过大 ×3、极柱温度过/欠温 ×6、电压变化过大 ×3、…（各轻/中/重） | **Alarm** | **待产品确认（Q-A）**；默认不入：与 304（单体温度）/303（单体电压）语义部分重叠；**若要"不丢语义"⇒ 加 1 组 `bms_aggr_cell_temp_charge_discharge`，段 4 = 301–316**（新增组的段内序 / IOA 随采纳顺序顺延，**不与其它待裁定项同时占同一号**） |
| **427–453 + 461–483** | **50** | 端子温度过低 ×3（427–429）、MOS 过温/欠温 ×6（430–435）、SOE 低 ×3（436–438）、正/负极柱温度 ×12（439–450）、绝缘检测低 ×3（451–453）；单体温度短路/断路 ×2（461–462）、MOS 温度故障（463）、均衡 MOS 故障（464）、从控通讯/供电/风扇/程序升级/参数设置故障 ×5（465–469）、从控供电过压 ×3（470–472）、主控供电故障（473）、主控程序升级故障（474）、主控供电过压 ×3（475–477）、EEPROM 存储（478）、地址编码（479）、CAN 电流采集（480）、485-1 通讯失联（481）、485-2 通讯失联（482）、PCS 失联（483） | **Alarm**（⚠️ 原稿把这整段 60 位误标为 Reserved） | **Q-B 已裁定（2026-09-23，用户）**：该段中 **424–426 / 454–459 / 460 共 10 位已移入 `∪组`（见 §9.2.1.2）**，**其余 50 位维持不入**（用户原话为"**至少**纳入"那三类）。维持不入的理由：主站侧为**安全总貌量**，逐项细分无调度用途；且其中多数为**检修/自检类故障**（EEPROM/地址编码/CAN 采集/主控从控供电过压）而非运行安全量。**若产品后续要求"全 60 位纳入"⇒ 加 1 组 `bms_aggr_system_fault`（段内序随采纳顺序顺延），段 4 相应 +1** |

> **Reserved 位不参与 G-1～G-4 任何判据**：`point_table` 中 67 位 Reserved（200、292、297、364–423、484–487）既不在 `∪组`、也不在 `EXCLUDED`，**不影响自检**（这正是"扣减后全覆盖"比"覆盖"更准确的原因）。**回源核对**：`mupc/crates/mupc-southd/src/point_table.rs` 的 `Role::Battery` 位表逐位统计为 `Alarm 212 / State 9 / Reserved 67`（合计 288），本节的 143 + 78 = 221 与 `212 + 9` 相符。
> **与 PRD 的偏差登记**：PRD §8.3.2 写"建议 12 个"但括号内枚举到 21 项（含"运行态（充/放/就绪/禁充放/故障）"这类多值项）。本设计**取 12 组 + 裁定 A 的 3 组 = 15 组**（v1.4-r2 后随 PRD §8.3.2 同步订正为"15 点 / IOA 301–315"），组内构成见 §9.2.1.2，逐位覆盖/排除**逐位可验证**（G-1～G-4）。见 §9.8 C-4 / Q-A / Q-B（**Q-B 已裁定**）。

**逐点 IOA（其余段）—— 块级区间 + 块内序**（块内顺序 = `points::expand` 产出的**顶点序**，点名 = 配置显式 `name` 优先、否则 `<块名>_<低地址寄存器偏移+1>`）：

| 段 | 站 | 块（按 `addr` 升序） | 点数 | IOA 区间 | 点名区间 |
|----|----|----------------------|------|----------|----------|
| 2 | `meter_batt` | `mb_e_act_comb` / `_fwd` / `_rev` / `mb_e_rea_comb` / `_fwd` / `_rev`（各 1 点） | 6 | 101–106 | `mb_e_act_comb_1` … `mb_e_rea_rev_1` |
| 2 | `meter_batt` | `mb_ui` | 6 | 107–112 | `mb_ui_1`–`mb_ui_6` |
| 2 | `meter_batt` | `mb_freq_line` | 4 | 113–116 | `mb_freq_line_1`–`mb_freq_line_4` |
| 2 | `meter_batt` | `mb_phase` | 8 | 117–124 | `mb_phase_1,_3,_5,_7,_8,_12,_13,_14` |
| 2 | `meter_batt` | `mb_power` | 16 | 125–140 | `mb_power_1,_3,…,_23,_25,_26,_27,_28` |
| 3 | `bms` | `bms_io` | 31 | 201–231 | `bms_io_1`–`bms_io_31`（含 `soc` = `bms_io_19` 的显式名，点名即 `soc`） |
| 3 | `bms` | `bms_energy` | 10 | 232–241 | `bms_energy_1,_3,_5,…,_17,_19` |
| 3 | `bms` | `bms_meta` | 8 | 242–249 | `bms_meta_1…_7,_9` |
| 3 | `bms` | `bms_term` | 4 | 250–253 | `bms_term_1`–`bms_term_4` |
| 3 | `bms` | `bms_cap` | 4 | 254–257 | `bms_cap_1,_3,_5,_6` |
| 5 | `pcs`（**条件性**） | `pcs_3zone` | 72 | 401–472 | **`pcs_3zone_1`–`pcs_3zone_43`、`pcs_3zone_45`、`pcs_3zone_47`–`pcs_3zone_73`、`pcs_3zone_75`**（共 43+1+27+1 = 72 项） |
| 6 | `fire` | `fire_sys` | 13 | 501–513 | `fire_sys_1`–`fire_sys_6`、**`fire_det_count`**、`fire_sys_8`–`fire_sys_13`（配置 `at: 7` 的显式 `name` 覆盖位置命名） |
| 7 | `hvac` | `hvac_di` | 31 | 601–631 | `hvac_di_1`–`hvac_di_31` |

**PCS 段关键点名（P6：按 `positional` 规则重推 —— 点名序号 = 块内寄存器偏移 + 1）**：

| 语义（配置 `at` / count） | 块内偏移 | **正确点名** | 原稿 | 说明 |
|---|---|---|---|---|
| 状态组 `at:14, count:5`（运行/故障/降额/并离网/故障码） | 13–17 | `pcs_3zone_14`…`_18` | `_14` ✓ | 运行状态 = **`pcs_3zone_14`** |
| 视在功率 `at:26, count:4`（A/B/C/**总**） | 25–28 | `_26`…`_29` | — | 总视在 = `pcs_3zone_29` |
| 有功功率 `at:30, count:4`（A/B/C/**总**） | 29–32 | `_30`…`_33` | 写 `_37` ✗ | **总 P = `pcs_3zone_33`** |
| 无功功率 `at:34, count:4`（A/B/C/**总**） | 33–36 | `_34`…`_37` | 写 `_41` ✗ | **总 Q = `pcs_3zone_37`** |
| 功率因数 `at:38, count:4`（A/B/C/**总**） | 37–40 | `_38`…`_41` | — | 总 PF = `pcs_3zone_41` |
| 两个 32 位电量 `at:43` / `at:45`（各占 2 寄存器） | 42–43 / 44–45 | `_43` / `_45` | ✓ | 偏移 43、45 是**高半字**，不产生点名 ⇒ **不存在的点名 = `_44`、`_46`、`_74`、`_76`** |

> **块内序的纪律**：点名序号 = **低地址寄存器偏移 + 1**（不是"第 k 个点"）——由 `points.rs:128-131` 的 `positional()` 决定。32 位点因此出现编号跳号（`_43` → `_45`），**这是既有命名规则，不得在本章改**。上表已按此规则**重推全部 PCS 点名**；原稿的 `_37`（总 P）/`_41`（总 Q）是**按"第 k 个点"误算**（把 4 点组的末项当成唯一总项），已订正。
> **PCS 段配置位置**：`pcs_3zone` 现为**整段注释占位**（`mupc/deploy/config/mupc_core_config.yaml:231-279`）；配表启用后须按 02 PRD §9.5 逐点复核（RC-U74-06），本表按其注释态配置（`addr:1000, count:76`）推导。
> **其余段的配置位置**（引用核对）：`bms` 站 `:142-230`、`meter_batt` 站 `:281-349`、`fire` 站 `:351-385`、`hvac` 站 `:387-409`、`grid_meter` 站 `:413-426`。
> **总点数**：6+40+57+**15**+13+31 = **162**（PCS 未启用）；+72 = **234**（PCS 启用）。〔v1.4-r2：段 4 由 12 增至 15，见下 §9.2.1.2〕

**启动期产物**：装配成功后把 `Vec<UplinkPoint>` 以 JSON 落 `system.data_dir/uplink_points.json`（**供 RC-U74-02 与主站逐点对点**），并打印一条 INFO（点数 + 段摘要）。**不得**由开发者手写 IOA 常量（既有 6 点的硬编码表由本生成器**取代**）。

#### 9.2.2 档位、节流与背压

**档位分配（唯一表驱动，落在 `uplink.rs`）**：

**档位分配（唯一表驱动，落在 `uplink.rs`）**：

| 档 | 内容（点名，**逐点可枚举**） | 点数（PCS 未启用） | 点数（PCS 启用） | 周期 |
|----|--------------|-------------------|------------------|------|
| **A** | ① grid：`active_power` `reactive_power` `voltage` `current` `cos_phi` `frequency`（6）；② `meter_batt`：`mb_power_7`(总 P) `mb_power_15`(总 Q) `mb_ui_1..3`(相电压) `mb_ui_4..6`(相电流) `mb_freq_line_1`(频率)（9）；③ `bms`：`soc`(=`bms_io_19` 的显式名) `bms_io_16`(簇组电压) `bms_io_17`(簇组电流) `bms_meta_6`(实时充放电功率)（4）；④ **PCS 启用后另加**：`pcs_3zone_14`(运行状态) **`pcs_3zone_33`(总 P)** **`pcs_3zone_37`(总 Q)**（3） | **19** | **22** | 1000 ms（可配，PRD §8.7 "周期须可配置"） |
| **B** | 其余**标量**点（电度、单体极值、剩余可充放、PT/CT、不平衡度、PCS 明细…） | **84** | **153** | 5000 ms（可配） |
| **C** | 全部**位点 + 状态枚举 + 消防系统态**：`bms_aggr_*`×**15** + `hvac_di_1..31`（46）+ fire 系统态 13 点（PRD §8.3.2 定其"变化上送"） | **59** | **59** | COS（值变化后 ≤ 1 s 内发出） |
| **合计** | 与 §9.2.1.0 的 IEC104 行一致 | **19+84+59 = 162** ✓ | **22+153+59 = 234** ✓ | — |

> **P3 重算说明（原稿 24/83/56 与 159 不自洽）**：原稿 A 档写 24，但**逐点枚举只有 19 项**（grid 6 + meter_batt 9 + bms 4）；B 档写 83 亦随之错 1（159 − 19 − 56 = **84**）。**本表为唯一真源**，且**三档之和必须等于 §9.2.1.0 的通道点数**（生成期 G-5 断言的一部分）。**PCS 启用时的 +3 只落在 A 档**，故 B 档 = 175 − 22 = **153**（不是 84+72）。
> **v1.4-r2 的 +3 只落在 C 档**（段 4 由 12 组增为 15 组，§9.2.1.2）⇒ 三档由 19/84/56 变为 **19/84/59**（PCS 启用 22/153/59）；恒等式仍成立：**234 − 59 = 175 = 22 + 153** ✓。

**节流与背压（复用 01 设计 §2.3 既有口径）**：

| 机制 | 设计 |
|------|------|
| A/B 档节流 | 每档一个 `tokio::time::interval_cancelled` 单拍任务：到点从快照 `all()` 过滤本档点 → 编码 → 入队。**不逐点各起定时器**（点数 × 定时器 = 无谓开销） |
| grid 既有 1 Hz 钳位 | `broadcast_grid_iec104`（`startup.rs:486-521`）的 `grid_bcast_at` 节流（`:487-500`）**由 A 档任务统一承担**（同一周期），旧函数**删除**（避免两条 A 档路径） |
| **`south_sim_loop` 的假遥测支路** | **删除**（`startup.rs:1365/1390-1402`）——它发点表外 IOA（`ioa_seq` 自 1 递增），与段 1（IOA 1–6）语义相撞且破坏 AC-U74-01；详见 §9.7 C-15 |
| C 档 | 订阅 §9.1.5 的变更批 → 过滤 C 档点 → **一次 `publish` 批量入队**（合并）；**值不变不发**（由 §9.1.5 的 `apply` 去重保证） |
| 背压 | **分层**：A/B 档 `try_send`，通道满 ⇒ **丢弃该批并计数**（`iec104_dropped_total` 指标 + 限频 WARN）；C 档 `send().await`（**阻塞等待，绝不静默丢遥信变位**，PRD EX-2）。理由：周期遥测下一周期自然重发（lossy OK），变位是事件（lossy 不可接受） |
| 队列容量 | `telemetry_txs` 的 `mpsc::channel::<Vec<u8>>(100)`（`server.rs:132`）→ **2048**。依据：**最坏一阵同刻到齐** = 总召/初始快照的 **234** 帧（234 × 25 B ≈ **5.7 KB**）+ 5 连接的并发写 ⇒ 2048 帧 ≈ **8 轮**余量（2048 ÷ 234 = 8.75）（单点单 ASDU 形态下"帧数"即"条数"）。**容量在连接建立处一处修改**（`server.rs:132`） |
| 慢主站 | 写任务按连接独立、单连接慢不影响其它连接（既有 `mpsc` per-connection 结构不变，`server.rs:192-204`） |

**带宽（设计精确值，回填 PRD §8.7 的"上界"表述）**：

**帧长（P3 重算；原稿算错）**：

| 帧 | 构成 | **帧长** |
|----|------|---------|
| 带时标遥测 `M_ME_TF_1`（**TI=36**） | APCI 6（`68`+len+4 控制字节）+ ASDU **19** = 类型(1)+VSQ(1)+COT(1)+CA(2)+IOA(3)+**短浮点(4)**+**CP56Time2a(7)** | **25 B**（原稿写 22 B ✗——把"浮点+时标"当成了 8 B 且漏算 4 B 浮点） |
| 带时标位 `M_SP_TB_1`（**TI=30**） | APCI 6 + ASDU **16** = 1+1+1+2+3+**SIQ(1)**+**CP56Time2a(7)** | **22 B**（原稿写 21 B ✗） |
| 既有总表 6 点现行 `M_ME_NC_1`（**TI=13**，无时标） | APCI 6 + ASDU 12（`encode_telemetry_asdu`，`protocol.rs:344-357`） | **18 B**（= PRD §8.7 的 18 B 口径） |

**带宽（可复算；稳态假设 C 档 1 变位/s，与 PRD §8.7 同假设）**：

| 档 | 计算（PCS 未启用） | 值 | PCS 启用 |
|----|------|-----|------|
| A | 19 × 25 B / 1 s | **475 B/s** | 22 × 25 = 550 B/s |
| B | 84 × 25 B / 5 s | **420 B/s** | 153 × 25 / 5 = 765 B/s |
| C（稳态） | 1 位/s × 22 B | **22 B/s** | 22 B/s |
| **稳态合计** | | **917 B/s ≈ 7.3 kbps** | **1,337 B/s ≈ 10.7 kbps** |
| C（高峰，PRD §8.7 的 10 位/s） | 10 × 22 B | +220 B/s ⇒ **≈ 8.9 kbps** | ⇒ **≈ 12.3 kbps** |
| **总召 / 初始快照突发** | **162** × 25 B | **4,050 B ≈ 4.0 KB** | **234** × 25 = **5,850 B ≈ 5.7 KB** |

> **点数 +3 只改"点表规模 / C 档条目数 / 一次性突发量"，不改任何速率**：A/B 档带宽 = 点数 × 帧长 ÷ 周期（点数未变）；C 档带宽 = **变位率** × 帧长（**与点表点数无关**，稳态 1 位/s、高峰 10 位/s 的假设未变）⇒ **稳态 7.3/10.7 kbps 与高峰 12.3 kbps 三值在 v1.4-r2 后一字不变**。唯一变化的是突发（3.9/5.6 KB → **4.0/5.7 KB**）。

> **与 PRD 上界的对照（v1.4-r2 重述，原登记 C-18 的张力已由裁定 B 关闭）**：
> ① **稳态**：**7.3 / 10.7 kbps ≤ 新上界 16 kbps** ✓（对 10.7 kbps 的余量 ≈ **50%**；对 7.3 kbps 余量 ≈ 119%）。
> ② **高峰瞬时（PCS 启用 + 10 变位/s）≈ 12.3 kbps ≤ 16 kbps** ✓ —— 该值即用户所述"**实测 IEC104 峰值 12.3 kbps**"，与本节复算值**逐位一致**（= (550 + 765 + 220) B/s × 8）。**但 PRD §8.7 已显式声明"高峰瞬时不保证"** ⇒ 16 kbps 仅作**验收（60 s 滑动平均）与组网规划**之用，不构成对任一瞬间的保证；**硬约束是 COS ≤ 1 s**（裁定 B）。
> ③ **突发 5.7 KB**（PCS 启用）仍高于 PRD 原估的 4.2 KB（后者按 18 B/帧且**不含 PCS**）⇒ 已回填为 **4.0 KB（未启用）/ 5.7 KB（启用）**（见 PRD §8.7）。
> ④ 压缩手段仍保留但不启用：若主站实测受限，按 PRD §8.7 允许的 **SQ=1 顺序 ASDU** 压缩（A 档 19 点可并为 1 帧 ≈ 6+1+1+1+2+19×(3+4+7) = 277 B，A 档降到 ≈ 277 B/s）——**本期仍取单点单 ASDU**（与既有实现同形态，KISS，且与 §9.2.3 的帧头修正解耦）。**裁定 B 后本项由"必需"降为"可选余量"。**

> **MQTT 侧带宽/资源**：见 §9.3.7（原稿只给了 IEC104 侧，评审已指出）。

#### 9.2.3 帧编码修正（**本增量的硬前提**）

**现状缺陷（必须同批修正，否则 234 点上送会被主站判为序号错误）**：

| # | 缺陷 | 证据 | 后果 |
|---|------|------|------|
| 1 | `make_i_frame` 的 `i2`/`i4` **恒为 0x00** ⇒ 发送序号只写得进低 7 位 | `gateway/src/iec104/protocol.rs:270,272` | 序号 **≥ 64 即回绕**；**234** 点一轮必然回绕 ⇒ 主站判 `send_seq` 错乱 |
| 2 | 所有遥测 I 帧的 `send_seq` **恒传 0**（调用方 `make_i_frame(0, 0, …)`） | `startup.rs:517`（grid 支路）、`:1397-1399`（sim 支路，本增量**删除**，见下） | 全部帧序号相同 |
| 3 | `Connection::send_seq` **无任何自增点** | `grep send_seq` 仅字段/读取（`connection.rs:71/82/198`） | 无出向序号维护 |
| 4 | `make_s_frame` 把 `send_seq` 写进第 3/4 字节（S 帧应只带**接收**序号，且第 3 字节须为 `0x01`） | `protocol.rs:252-259` vs 调用 `connection.rs:198` | S 帧格式不合规 |
| 5 | 无 k/w 窗口（未确认帧计数）控制 | — | 主站若 k 饱和，行为未定义（**登记为遗留，不在本增量**） |

**修正设计（最小面）**：

```rust
// protocol.rs —— 修正为 15 位序号（低 7 位 / 高 8 位两字节），签名不变
pub fn make_i_frame(send_seq: u16, recv_seq: u16, asdu: &[u8]) -> Vec<u8> {
    let s = send_seq & 0x7FFF;
    let r = recv_seq & 0x7FFF;
    // c1 = (s << 1) & 0xFE ; c2 = s >> 7 ; c3 = (r << 1) & 0xFE ; c4 = r >> 7
}

/// S 帧只携带"本端已接收序号"；原 `send_seq` 形参**语义本就多余**，故改签名（唯一调用点在 connection.rs）
pub fn make_s_frame(recv_seq: u16) -> Vec<u8>;   // 68 04 01 00 <r<<1> <r>>7>
```

**逐连接出向序号（新增，装配点 = `Iec104Server` 连接处理）**：

```rust
// server.rs：每连接一份出向序号（与写半锁同生命周期）
struct OutboundSeq(std::sync::atomic::AtomicU16);
impl OutboundSeq {
    /// **必须在持有写半锁时调用**，否则序号与写序不一致（契约写进文档注释）
    fn next(&self) -> u16 { self.0.fetch_add(1, SeqCst) & 0x7FFF }
}
```

**遥测通道载荷改为 ASDU（不含 I 帧头）**：

```rust
// 现状：broadcast_telemetry(Vec<u8>) 收的是"完整 I 帧"，由调用方塞死 seq=0
// 改为：
impl Iec104Server {
    /// 批量投递 ASDU（**不含 I 帧头**）。连接层负责：取本连接出向序号 → 组 I 帧 → 写。
    /// `class` 决定背压策略（§9.2.2）：A/B 档 try_send 丢并计数；C 档 send().await 不丢。
    pub async fn publish_asdus(&self, asdus: Vec<Vec<u8>>, class: DataClass) -> PublishOutcome;
    /// 兼容壳（**仅过渡**）：内部转 `publish_asdus(vec![asdu], A)`。唯一既有调用方 core-bin 同批改为新 API。
    #[deprecated(note = "改用 publish_asdus（逐连接序号由连接层维护）")]
    pub async fn broadcast_telemetry(&self, frame: Vec<u8>);
}
```

**写任务改造**（`server.rs:192-204`：`telemetry_write` 句柄 `:192`、`while let Some(data) = telemetry_rx.recv().await` `:195`、`w.write_all(&data)` `:197`）：从 `telemetry_rx` 收到 ASDU 后，在**同一把写半锁**内 `let seq = outbound_seq.next(); w.write_all(&make_i_frame(seq, recv_seq.load(), &asdu))`；`recv_seq` 由读循环在每收到 I 帧后更新（新增共享 `Arc<AtomicU16>`）。

**受影响的既有断言（须同步更新，非弱化）**：

| 文件（**行号已重校**） | 断言 | 处置 |
|------|------|------|
| `protocol.rs:508-532`（`test_i_frame_sequence` `:509-520` / `test_i_frame_make` `:523-532`） | I 帧 `send_seq`/`recv_seq` 往返（seq = 5 / 10，均在 7 位内） | 保持（新编码对 < 64 的序号结果不变）；**新增** seq ≥ 128 与 32767 回绕的往返用例（回归钉） |
| `protocol.rs:486-494`（`test_s_frame_make`） | S 帧字节（仅断言 `len`/`[0]`/`[1]`/`[2]`） | **签名 2 参 → 1 参**（`make_s_frame(recv_seq)`）⇒ 用例调用处必须改；断言 0–2 字节不变 ⇒ **新增** 第 3/4 字节为 `recv_seq<<1`（LE）的用例 |
| `protocol.rs:394-400`（`test_type_id_from_u8_invalid`；断言在 `:398`） | `from_u8(100) == None` | **必然变红**：改为 `from_u8(100) == Some(TypeId::CIcNa1)`；`from_u8(2)` 仍应为 `None`（2 = M_SP_TA_1 **未实现**，保持断言）。另 `protocol.rs:366-373` 的 `required_type_ids` 表须补 `(30, MSpTb1)`/`(36, MMeTf1)`/`(100, CIcNa1)` 三行 |

#### 9.2.4 TypeID 与编码器（时标口径）

**新增/订正 TypeID**（**不改既有变体值**，避免破坏既有报文与用例；**既有变体名逐字照 `protocol.rs:29-46` 现状列出**——原稿此处把 34/35 的枚举名写错，已订正）：

```rust
pub enum TypeId {
    MSpNa1 = 1,    // 单点遥信（无时标）—— 位点**无时标**回退路径，本期不用
    MDpNa1 = 3,
    MMeNa1 = 9,
    MMeNc1 = 13,   // 短浮点（无时标）—— **既有总表 6 点现行形态**
    // —— 以下 3 个是**既有变体**（`protocol.rs:35-38`），只订正注释、不改名不改值 ——
    MSpTa1 = 30,   // ⚠️ 名标 M_SP_TA_1，**标准值 30 = M_SP_TB_1（带 CP56Time2a）**；本期按 30 组帧
    MDpTa1 = 31,   // ⚠️ 标准值 31 = M_DP_TB_1；本期不用
    MMeTa1 = 34,   // ⚠️ 名标 *TA*，**标准值 34 = M_ME_TD_1（归一化 + CP56Time2a）**；本期不用
    MMeTd1 = 35,   // ⚠️ 名标 *TD*，**标准值 35 = M_ME_TE_1（标度化 + CP56Time2a）**；本期不用
    // —— 以下为**新增** ——
    MSpTb1 = 30,   // ✅ 新增**别名**（与 MSpTa1 同值同物：M_SP_TB_1，带 CP56Time2a）
    MMeTf1 = 36,   // ✅ 新增：短浮点 + CP56Time2a —— **本期遥测统一形态**
    CIcNa1 = 100,  // ✅ 新增：站总召（C_IC_NA_1）
    // …控制方向（45/46/48/58/59/61）不变
}
```

> `MSpTb1 = 30` 与既有 `MSpTa1 = 30` **同值**：Rust 枚举不允许两个变体同值 ⇒ 实现取 **`pub const MSpTb1: TypeId = TypeId::MSpTa1;`**（关联常量别名，值语义完全等价），并在 `MSpTa1` 的文档注释上写标准名订正。**不改既有变体名**是为避免破坏 `required_type_ids` 用例与既有报文（登记 §9.8 C-6）。

**COT 常量**（新增，`protocol.rs`）：

```rust
pub const COT_SPONT: u8 = 3;      // 突发/变位（C 档）
pub const COT_REQ: u8 = 5;        // 请求（主站 → 装置）
pub const COT_ACT: u8 = 6;        // 激活（装置 → 主站，应答遥控）
pub const COT_ACT_CON: u8 = 7;    // 激活确认（总召 ACT_CON）
pub const COT_ACT_TERM: u8 = 10;  // 激活终止（总召 ACT_TERM）
pub const COT_INTROGEN: u8 = 20;  // 响应站召唤（总召数据）
pub const COT_CYCLIC: u8 = 1;     // 周期（A/B 档）
```

**编码器（新增，`protocol.rs`）**：

```rust
/// M_ME_TF_1(36)：IOA(3) + 短浮点(4, LE) + CP56Time2a(7)
pub fn encode_me_tf1(ioa: u32, value: f32, ts_ms: u64, cot: u8) -> Vec<u8>;
/// M_SP_TB_1(30)：IOA(3) + SIQ(1) + CP56Time2a(7)
pub fn encode_sp_tb1(ioa: u32, value: bool, ts_ms: u64, cot: u8) -> Vec<u8>;
/// C_IC_NA_1(100) 的 ACT_CON/ACT_TERM 空应答：IOA=0 + QOI=0
pub fn encode_ic_term(cot: u8) -> Vec<u8>;
```

`CP56Time2a` 编码：`ms(2, LE 小端 0–59999) | 分(1, bit7=无效) | 时(1) | 日+星期(1) | 月(1) | 年(1, = year-2000)`，**UTC**（设计 §2.2「时标规范」既有口径）。

**时标口径裁定（与 PRD 的差异登记）**：PRD §8.7 要求"IEC 104 使用带时标 TypeID"，但既有总表 6 点现行形态是 **TI=13（无时标）** 且被 02/01 PRD 定为"**现场追认**、不得改"。本设计取：

- **新增外设点**（**162/234** 点）一律用 **TI=36 / TI=30 带时标**（满足 §8.4/§8.7 的"不得用响应时刻重新打时标"）；
- **既有总表 6 点**：**待产品/主站裁定**（方案 A：同批改 TI=36，IOA 1–6 不变；方案 B：维持 TI=13，则 §8.4 的时标要求在总表 6 点上作废）。**默认按方案 A 落设计**（同一通道内两种时标形态会造成主站侧对点歧义），登记于 §9.8 C-3。

#### 9.2.5 总召（C_IC_NA_1）与连接初始快照

**数据源接缝（新增，`command.rs`）**：

```rust
#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError>;
    fn name(&self) -> &str;

    /// **站召唤数据源**（新增，**默认实现返回空** ⇒ 既有 3 个 impl 不改也编译通过）。
    /// core-bin 的 `StrategyCommandHandler`（定义 `startup.rs:201`、`impl` `:222`）覆写：
    /// 从 `LatestValues::all()` 过滤 `channels.has(IEC104) && is_fresh(..)` 的点
    /// （**位点走站级活性判据**，§9.1.3），按 §9.2.1 的 IOA 表转成 `TelemetryItem`。
    /// **无有效值的点不出现**（PRD GI-3：不得以 0 或旧值顶替）。
    async fn on_interrogation(&self) -> Vec<TelemetryItem> { Vec::new() }

    /// **连接初始快照数据源**（新增，默认空）。与 `on_interrogation` 同源同实现
    /// （二者是同一份"当前值"的两种触发方式；保持**一个实现两处调用**，防两套口径）。
    async fn on_connection_snapshot(&self) -> Vec<TelemetryItem> { self.on_interrogation().await }
}

/// 上送条目（**协议层类型，定义在 gateway**，避免 gateway 依赖 data-processing）
#[derive(Debug, Clone, Copy)]
pub struct TelemetryItem {
    pub ioa: u32,
    pub kind: TelemetryKind,   // Scalar | Bit
    pub value: f32,            // Bit ⇒ 0.0 / 1.0
    pub ts_ms: u64,            // **采集时刻**（不是发送时刻）
    pub cot: u8,               // 周期=1 / 突发=3 / 总召响应=20
}
```

**总召处理（落在 `Connection::handle_i_frame`）**：

```
收到 I 帧 → 解析 ASDU 头
  if header.type_id == TypeId::CIcNa1:
      qoi = asdu[8]                       // IOA(3) 之后 1 字节
      if qoi == 20:                       // 站召唤（GI-1）
          ① 写 ACT_CON     : encode_ic_term(COT_ACT_CON)     ← 立即（≤ 首帧）
          ② 取 items = handler.on_interrogation().await      ← **先取数、再持锁写**（不跨 await 持锁）
          ③ 逐条写 encode_me_tf1/TI=30，cot = COT_INTROGEN(20)，**ts = 该点采集时刻**
          ④ 写 ACT_TERM    : encode_ic_term(COT_ACT_TERM)
      else if 21..=36:                    // 分组召唤
          **本轮不实现**（GI-4）：回 ACT_CON + ACT_TERM（无数据），并产出一条 `info` 级事件
          「收到分组召唤 QOI=<n>，本轮未实现」——**不得静默忽略**
      else: 忽略 + WARN
```

| 约束 | 设计 |
|------|------|
| 响应时延（PRD 4.x：≤ 5 s，**234** 点） | **234 点 × 25 B ≈ 5.7 KB** 直写；实测应为**毫秒级**。设计声明 **≤ 1 s**（远优于 5 s 要求），并在集成测试断言 |
| 三段完整性（GI-2） | ACT_CON → 数据 → ACT_TERM **在同一把写半锁内顺序写出**（同一 `write_all` 序列），不存在交错 |
| 时标（§8.4） | 数据帧时标 = 快照的 `ts_ms`（采集时刻），**不得**用响应时刻（回归断言：注入 ts=1000 的点，断言帧内 CP56Time2a == 1000）。**位点例外**：其 `ts_ms` = 最后变化时刻（§9.1.2），**这是"该状态自何时起有效"的正确语义**，仍满足"不得用响应时刻重打" |
| 并发/多主站 | 每连接独立处理各自的总召（读循环 per-connection），互不影响 |
| 主站未连接时的总召 | 不适用（总召只能由已连接主站发起） |

**连接初始快照（GI-5）**：

| 项 | 设计 |
|----|------|
| 触发点 | 读循环在 `handle_frame` 返回后检查 `conn.take_just_connected()` —— 由 `handle_u_frame` 在 `StartDtAct → state = Connected`（`connection.rs:126-137`）处置位；`StartDtCon` 分支（`:138-141`）同样置位（对称，防主站先发 CON 的少见形态） |
| 时序 | 先发 `STARTDT_CON`（协议要求）→ 再取快照（`handler.on_connection_snapshot().await`）→ **入本连接的遥测通道**（复用 C 档 `send().await` 不丢语义）→ 由写任务按本连接序号发出 |
| 时延（PRD ≤ 5 s） | 实测毫秒级；设计声明 ≤ 1 s |
| 内容 | `channels.has(IEC104) && is_fresh(..)` 的全部点（**无有效值者不出现**，PRD GI-3/GI-6）。**位点按站级活性判定**（§9.1.3）——否则装置长跑后接主站，稳态位点会整批消失，与 PRD §8.6.3「位块首轮全量 ≈319 点」冲突 |
| **回归点（AC-U74-11）** | 快照必须含"**连接建立之前**产生的值"——因为取数读的是**内存快照**（不是"连接后新采集"），天然满足；测试用"先 `apply` 若干点 → 再握手 → 断言收到"钉住 |
| 与 C 档 MQTT 首轮全量的对应 | 两者同源（同一快照），但**通道独立**：IEC104 快照以 `cot=20` 发；MQTT 以站内合并消息发（§9.3.4） |

#### 9.2.6 站离线 / 不可得 / PCS 未启用的上送行为

| 情形 | IEC 104 行为 | 实现点 |
|------|--------------|--------|
| 站离线（`mark_station_offline` 后） | 该站全部点 `quality = Invalid`（聚合点同在，因同 station）⇒ **从 A/B 档周期上送与总召/快照响应中整体消失**；另产出 `south_station.<id>.offline` 遥信事件（`quality` 非 Ok 的位点**不入 C 档变更批**） | §9.2.2 过滤条件 `quality == Ok` |
| 值过期（**标量点** `ts_ms` 超 `stale_timeout_s`，或**位点所在站**超 `stale_timeout_s` 未成功轮询） | 同上（`is_fresh == false` ⇒ 不发）。**"超过 5 s 旧值不再重复上送"由该条件天然成立**（PRD EX-1 末句） | `LatestValues::is_fresh`（§9.1.3：标量用逐点时标、位点用站级活性） |
| 点未配置（站从未采集） | 快照中无该点 / `Unconfigured` ⇒ 不产上送条目（生成器已按点表产出条目，运行期无值 ⇒ 不发） | PRD EX-6 |
| **PCS 站未启用** | 生成器**不产条目**（站不在 `south_stations.stations`）⇒ 72 点既不占 IOA、也不进总召/快照（PRD EX-7 的"不以 0 顶替"由"根本不存在该点"满足） | §9.2.1 子集规则 |
| 主站未连接 | **不缓存、不入队**（A/B 档直接不发）；连接后由初始快照补齐（PRD EX-2）。**变位不丢**：C 档 `send().await` 在"无连接"时通道无接收者 ⇒ 由 `publish_asdus` 在**无连接时直接返回 `NoSubscriber` 并计数**，不阻塞（**变位由连接后的初始快照补齐**，口径同 EX-2 的"或随快照/总召送达"） | `publish_asdus` |

---

### 9.3 MQTT 真做（U-71）

#### 9.3.1 承载收敛裁定（PRD Q6）

**裁定：保留 `mqtt-bridge`（北向 `NorthMqttClient` + 本地 `LocalMqttClient`），下架 `mqtt-plugin`。**

| 决策 | 内容 | 理由 |
|------|------|------|
| 保留 | `mqtt-bridge`（`rumqttc` 0.24，已有 TLS/重连/事件循环） | ① 它是 01 设计 §4.3 的既定形态；② 它已具备**可用的 TLS 双向证书与断线重连**实现（`north_client.rs:32-45` 读三件证书 + `Transport::tls`、`:49-56` 连接态与重连配置）；③ `data-processing` 已依赖它（`Cargo.toml`），下架成本更高 |
| 下架 | `mqtt-plugin`：① **从两份 deploy YAML 的 `plugins.auto_load` 移除**（`mupc/deploy/config/mupc_core_config.yaml:87-90`、`…production.yaml:90-93`）——这是**唯一有效的下架动作**：插件由 `plugin_loader::PluginLoaderImpl`（`startup.rs:865` 构造）按名从 `plugin_dir` 动态加载，**不在 core-bin 的 Cargo 依赖表里**（`Cargo.toml` 仅依赖 `mupc_mqtt_bridge`）⇒ 无需改 Cargo.toml，也**没有**"编译期依赖"可移除；② crate 源码暂留并在 `lib.rs` 顶部标注 `//! ⚠️ 已下架（U-71/Q6，2026-09-23）：不参与运行，勿新增调用方`；③ 若现场 `plugin_dir` 已投放 `mqtt_plugin.so`，须一并移除（**部署检查项**） | PRD CFG-4「不得存在注册为 Running 但无行为的模块」。`mqtt-plugin::start()` 是空实现（`mqtt-plugin/src/lib.rs:75`），且其 `MqttConfig` 与 `mqtt-bridge` 的**同名不同型**（两套配置各自演化）——保留双实现 = 双份缺陷面 |
| **不做** | 不删除 `mqtt-plugin` crate 目录 | 一次性删除会牵动 workspace 成员表、`plugin-loader` 的示例与既有 FFI 测试；下架（不加载）已满足 CFG-4，删除可另立清理批次 |

#### 9.3.2 配置承载（`core_config.mqtt_bridge` 段）

**schema（新增/替换 `core_config.rs:302-314` 的 2-bool 结构 `MqttBridgeConfig`）**：

```yaml
mqtt_bridge:
  enabled: false                 # 总开关；缺省 false ⇒ **零连接尝试**（CFG-2）
  north:
    enabled: false
    broker: ""                   # "host:port"；enabled=true 时**不得为空**（validate 拒）
    client_id: ""                # 空 ⇒ 装配期取 dev_id（§9.3.6），仍空 ⇒ validate 拒
    username: null               # 可选
    password: null               # 可选；**不得进日志/载荷**（§9.3.6）
    tls:
      ca_cert: ""                # enabled=true 时三路径均不得为空且文件须可读
      client_cert: ""
      client_key: ""
      allow_plaintext: false     # 仅非生产构建 + 显式 true 允许明文（Q9，默认 false）
    topic_prefix: "mupc/north"
    qos: 1
    periods:
      a_ms: 1000
      b_ms: 5000
      cos_merge_ms: 200          # COS 合并窗（上限 1000，超限 validate 拒）
    cache:
      max_age_s: 1800            # 断线缓存时间窗（PRD BF-4 建议 ≥ 30 min）
      max_messages: 10000        # 条数上限；与 max_age_s **先到先淘汰**
  local:
    enabled: false
    broker: "127.0.0.1:1883"
    client_id: "mupc-local"
```

**Rust 结构（`core_config.rs`）**：

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]                        // 整段缺省 ⇒ 全 false / 空 ⇒ **零行为变化**
pub struct MqttBridgeConfig {
    pub enabled: bool,
    pub north: NorthCfg,
    pub local: LocalCfg,
}
impl Default for MqttBridgeConfig { /* 全 false；broker 空串（**不是 example.com**） */ }
```

**`validate_mqtt_bridge()`（新增，`core_config.rs`，与 `validate_io`/`validate_display` 同范式，由 `validate()` 调用）**：

| 校验 | 违规处置 |
|------|----------|
| `enabled == true` ⇒ `north.enabled == true` 或 `local.enabled == true` | `Err("mqtt_bridge.enabled=true 但 north/local 均未启用")` |
| `north.enabled == true` ⇒ `broker` 非空、可解析为 `host:port`（`port ∈ 1..=65535`） | `Err("mqtt_bridge.north.broker …")`（**点名键**） |
| `north.enabled == true` ⇒ `client_id` 非空 | `Err(...)` |
| `north.enabled == true && !allow_plaintext` ⇒ 三条证书路径均非空、文件存在且可读 | `Err(...)`（**fail-closed 前置**；运行期再由 `NorthMqttClient::new` 二次把关） |
| `north.enabled == true && allow_plaintext == true` ⇒ 须 `cfg!(debug_assertions)` 或 `MUPC_ALLOW_PLAINTEXT_MQTT=1` | `Err(...)`（**生产构建禁止明文**；TLS-2/TLS-4） |
| `topic_prefix` 非空且不以 `/` 结尾 | `Err(...)` |
| `qos ∈ 0..=2`；`a_ms ∈ 100..=60000`；`b_ms ∈ a_ms..=600000`；`cos_merge_ms ∈ 1..=1000` | `Err(...)` |
| `cache.max_age_s ≥ 60`、`cache.max_messages ≥ 100` | `Err(...)` |

**部署 YAML（两份）新增 `mqtt_bridge:` 段**：开发模板给 `enabled: false` + 全空/占位注释；**生产模板不得出现 `mqtt.example.com` 或 dummy 证书路径**（CFG-3）。

**装配点（`startup.rs` 步骤 13 —— **现状 `:1513-1556`**，本设计替换该段；`:1537` 即"`NorthMqttClient::new(&::default())`"缺陷点）**：

```rust
// 现状（缺陷）：NorthMqttClient::new(&NorthMqttConfig::default())  ← 假域名 + dummy 证书
// 改为：从配置构造；enabled=false 时**一行连接代码都不执行**
if cfg.mqtt_bridge.north.enabled {
    let north = mupc_mqtt_bridge::NorthMqttConfig { /* 由 cfg.mqtt_bridge.north 逐字段映射 */ };
    match mupc_mqtt_bridge::NorthMqttClient::new(&north) {
        Ok(c) => { let c = Arc::new(c);
                   guard.0.push(tokio::spawn({ let c = c.clone(); async move { let _ = c.run().await; } }));
                   guard.0.push(tokio::spawn(mqtt_uplink_publisher(latest.clone(), c.clone(), cfg.clone())));
                   coord.register_service("mqtt_bridge_north", ServiceStatus::Running); }
        Err(e) => { tracing::error!(...); coord.register_service("mqtt_bridge_north", ServiceStatus::Failed); }
    }
}
```

> **`NorthMqttConfig::default()` 的 `broker_addr` 必须改为空串**（`mqtt-bridge/src/config.rs:52-68` 的 `impl Default`，`:56` 现为 `mqtt.example.com:8883`）：保留假域名会让"任何漏改的默认路径"重新真连。**同时删除 `NorthMqttConfig::test_config()`**（`:73-78`，它把 `default()` 当测试替身，等于固化该缺陷）。`enabled` 字段（`:42-43`，现状缺省 `false`）保留。

#### 9.3.3 生产者接线（`MqttUplinkPublisher`）

**落点**：新增 `mupc/crates/mupc-core-bin/src/uplink.rs`（**两个上送器同文件**：`Iec104UplinkDriver` + `MqttUplinkPublisher`，共用**同一份**点表与过滤逻辑）。

```rust
pub struct MqttUplinkPublisher {
    latest: Arc<mupc_data_processing::LatestValues>,
    /// §9.2.1 生成的**并集**点表；发布时**必须**按 `channels.has(ChannelMask::MQTT)` 过滤
    /// ⇒ 恰好 624 点（PCS 未启用 552），**不含 15 个 IEC104 独有的 BMS 聚合**
    points: Arc<Vec<UplinkPoint>>,
    client: Arc<mupc_mqtt_bridge::NorthMqttClient>,
    cfg: MqttPublishCfg,
    cache: Arc<Mutex<OfflineCache>>,       // §9.3.5
    seq: AtomicU64,                        // 单调轮次序号（载荷 `seq`）
}
```

**行为**：

| 触发 | 动作 |
|------|------|
| 启动 / broker 每次连上 | ① **C 档立即全量一次**（**按站分条**：bms 288 位 1 条、hvac 31 位 1 条、fire 13 点 1 条）——**站间不合并**（PRD §8.3.3 分片强制）；② 解除离线缓存并补送 |
| A 档定时（`a_ms`） | 逐站 `station_snapshot(station)` → 过滤 `channels.has(MQTT)` ∧ A 档 ∧ `quality == Ok && is_fresh` → 组 JSON → `publish(telemetry/{station})` QoS1 |
| B 档定时（`b_ms`） | 同上（B 档） |
| C 档（订阅快照变更批） | 按 `cos_merge_ms` 合并窗聚合 → 逐站组 JSON → `publish(telemetry/{station})`（QoS1；故障类事件走 `event/{station}` QoS2） |
| **点数断言（装配期 + 集成测试）** | 一次全量发布的消息点集并集 == **624**（PCS 未启用 **552**）——AC-U74-02 的机械判据 |
| **禁止** | ❌ 任何路径读 `telemetry` 表 / `WriteBuffer`（PRD LV-2 = CNS-01） |

**离线缓存（内存态，无持久化 = PRD Q7 建议①）**：

```rust
struct OfflineCache { q: VecDeque<Pending>, bytes: usize, dropped: u64, first_ts_ms: Option<u64> }
struct Pending { topic: String, payload: Vec<u8>, qos: u8, seq: u64, ts_ms: u64 }
```

| 规则 | 设计 |
|------|------|
| 入缓存条件 | `!client.is_connected()` 或 `publish` 返回失败 |
| 上限 | `max_age_s`（默认 1800 = 30 min，按 `ts_ms` 淘汰）**与** `max_messages`（默认 10000）**先到先淘汰**（BF-4） |
| 淘汰 | 丢**最旧**；每次淘汰累计 `dropped` + 记录 `[first_ts, last_ts]` 范围 |
| 留证（BF-4/EX-9） | ① WARN 日志（含丢弃条数 + 时间范围，**按 1 min 聚合，不风暴**）；② 一条 `major` 事件（`mqtt_cache_overflow`）；③ 计数可通过 `MqttUplinkPublisher::stats()` 查询（供后续管理面/屏显示） |
| 补送（BF-2/BF-3） | 重连后**按 `seq` 单调顺序**逐条补送，`payload.ts` **保持原采集时刻**；补送在独立 `spawn` 任务中跑，**与 A 档实时发布并发**（实时链路不被补送阻塞）；补送期间新数据仍正常发（BF-3） |
| 补送失败 | 重新入队（**回填到队首**，保持顺序）；连续失败按 §9.3.6 计数 |
| 掉电 | **不落盘**（Q7 ①）；掉电期间历史由 03 号 `telemetry` 表兜底（03 PRD §4.1） |

#### 9.3.4 主题与载荷（含对 §4.5/§5.5 既有主题表的修订）

**主题（修订 `mqtt-bridge/src/topics.rs` 为函数式；旧常量 `#[deprecated]` 保留以免大规模改名）**：

```rust
pub fn north_telemetry(station_id: &str) -> String { format!("mupc/north/telemetry/{station_id}") }
pub fn north_event(station_id: &str)     -> String { format!("mupc/north/event/{station_id}") }
pub const NORTH_STATUS: &str = "mupc/north/status";   // 不变
pub const NORTH_FAULT:  &str = "mupc/north/fault";    // 不变
```

**对 01 设计 §5.5 北向主题表的修订**（**本行表为准**，原行由本表取代）：

| 常量 | Topic | 方向 | QoS | 说明 |
|------|-------|------|-----|------|
| `north_telemetry(id)` | `mupc/north/telemetry/{station_id}` | → 物联平台 | 1 | **分片遥测**（A/B 档 + C 档首轮全量），`{station_id} ∈ {grid_meter, bms, meter_batt, pcs, fire, hvac}` |
| `north_event(id)` | `mupc/north/event/{station_id}` | → 物联平台 | 1（故障类 2） | 变位/站 offline/online/SOC 越界/消防登记数不一致 |
| `NORTH_STATUS` | `mupc/north/status` | ↔ | 0 | 装置状态（不变） |
| `NORTH_FAULT` | `mupc/north/fault` | → | 2 | 故障事件（不变） |

**兼容性**：订阅 `mupc/north/telemetry/#` 仍可收到全部站（PRD §8.3.3 强制）；**每条消息只含一个站**；**站间不合并**（含首轮全量——见 §9.8 C-5 对 PRD §8.6.3「允许合并为一条」的收敛）。

**载荷（JSON / UTF-8，字段 = 01 PRD §8.3.3 逐字）**：

```json
{
  "ts": "2026-09-23T08:00:00.123Z",
  "dev": "MUPC-0001",
  "station": "bms",
  "role": "battery",
  "seq": 42,
  "points": [ { "n": "soc", "v": 78.5, "u": "%", "q": "ok" },
              { "n": "bms_alarm_26", "v": 1.0, "u": "bool", "q": "ok" } ]
}
```

| 规则 | 设计 |
|------|------|
| `ts` | **采集时刻，UTC**，**线序 = ISO-8601 毫秒字符串**（`YYYY-MM-DDThh:mm:ss.sssZ`，如 `"2026-09-23T08:00:00.123Z"`）—— **与 PRD §8.3.3 的表述逐字一致**（P4：原稿写"整数 epoch"，与 PRD 不一致，已按 PRD 口径改）。**说明**：PRD 只规定语义与时区（"采集时刻 + UTC + 毫秒"），**具体线序由本设计定** = ISO-8601**字符串**；内部 `ts_ms: u64`（§9.1.2）在组包时格式化（**非**把 epoch 整数直接序列化）。选字符串的理由：① 与 PRD 字段表逐字一致，避免下游对"1758604800123 是秒还是毫秒"的歧义；② 云端免时区推断。**若主站/物联平台坚持整数 epoch ⇒ 属"待确认"项（§9.8 Q-E），改一行格式化代码**。同一消息内所有点共用一个 `ts`（同一轮）；不同轮次的点**不得**混入同一条消息 |
| `n` | **南向点名逐字**（`UplinkPoint.metric`），**零改名、零前缀、零序号替代**（LV-5 / PRD §8.3.3）。**唯一例外 = grid 6 点**（派生名 `active_power` 等，非 02 号点表展开名）——已登记 §9.8 C-17 |
| `v` | 工程值；`q != ok` 时**保留原值**；不可得 ⇒ `null`（**严禁 0 顶替**） |
| `u` | 工程单位（来自 02 PRD §9.7.5；`bool` 用于位点） |
| `q` | `ok` / `stale` / `invalid` / `unconfigured`（映射自 §9.1.2 的 `PointQuality`，**一一对应**） |
| `seq` | 单调递增轮次（每 `station` 独立计数，供乱序/重复检测） |
| `dev` | 装置标识；**来源未定**（PRD Q10）⇒ 配置项 `mqtt_bridge.north.client_id` 缺省时取 `system.dev_id`（若存在），**均无 ⇒ 载荷中 `dev` 写 `null` 并在日志标注"未提供"**（PRD EX-10：**不得臆造**） |

> **载荷示例中的 `bms_alarm_26` 序号核对（评审要求项）**：位点命名 = `bms_alarm_<位地址 − 199>`（`point_table.rs:387` 起 `bit(Role::Battery, 200, …)`，`points::expand` 的 `positional(块名, 位偏移)` 规则）⇒ **`bms_alarm_26` ↔ 位地址 225 = "簇 SOC 低·轻"（`BitClass::Alarm`）**，是**合法存在**的点，且属于聚合组 305（`bms_aggr_soc_low`）。**示例保留**。

#### 9.3.5 TLS 与凭据保护

| 项 | 设计 |
|----|------|
| TLS 版本 | `rumqttc` + `rustls` ⇒ TLS 1.2+（既有） |
| 双向认证 | CA + client_cert + client_key（既有 `Transport::tls`） |
| **fail-closed（TLS-2）** | ① 配置期：三路径为空/文件不可读 ⇒ **拒启动**（§9.3.2 validate）；② 运行期：`NorthMqttClient::new` 读证书失败 ⇒ `CertificateError`，装配点记 `error` + 服务注册为 `Failed`，**不重试明文**；③ **不存在**任何 `allow_plaintext` 之外的明文分支 |
| 明文例外（TLS-4/Q9） | 仅当 `allow_plaintext: true` **且**（`debug_assertions` 或 `MUPC_ALLOW_PLAINTEXT_MQTT=1`）；启动打 **ERROR 级**日志 + 产出一条 `major` 事件（响亮度要求：不得静默） |
| 证书到期监控（TLS-3） | 新增依赖 **`x509-parser = "0.16"`**（纯 Rust，无 C 依赖，交叉编译友好）到 `mqtt-bridge` —— ⚠️ **版本更正（评审要求项）**：原稿写 `"0.15"`，但 **workspace 已在用 0.16**（`crates/security/Cargo.toml:16` 为 `"0.16"`，`Cargo.lock:5237-5239` 锁定 **0.16.0**）⇒ 写 0.15 会引入**重复版本树**。**本次改为 0.16，复用 workspace 现有版本**（语义化范围相同、API 一致）。启动期 + 每日定时解析 `client_cert` 的 `notAfter`：剩余 < 30 天 ⇒ 产出一条 `major` 事件（`mqtt_cert_expiring`，含剩余天数）；解析失败 ⇒ WARN + 继续（**不阻断连接**） |
| **凭据不入日志/载荷** | ① `core_config::MqttBridgeConfig` 的 `password` 字段**手写 `Debug`**（打印 `***`）——`#[derive(Debug)]` 必须移除；② `tracing::*!` 中**不打印** `MqttOptions`/`password`/`username`；③ 载荷 JSON **不含**凭据字段；④ 错误文案不含凭据（既有 `MqttBridgeError` 文案已满足；新增文案须遵守） |
| **验收** | 单测：`format!("{:?}", cfg)` 不含密码明文；日志捕获快照不含密钥内容（AC-U74-07 的一部分） |

#### 9.3.6 健康与可观测

| 指标 | 载体 |
|------|------|
| 连接状态 | `MqttUplinkPublisher::is_connected()`（复用 `NorthMqttClient::is_connected`）；服务注册状态（`coord.register_service`）+ 一条 WARN/INFO 迁移日志 |
| 发布计数/失败计数/丢弃计数 | `MqttUplinkStats { published_total, failed_total, cached_len, dropped_total, last_error: Option<String> }`，`stats()` 返回快照（BF-6：**不得静默丢弃**） |
| 连续失败告警 | 连续 `failed_total` ≥ 10 ⇒ 一条 `major` 事件，之后**每 5 min 最多一条**（不风暴） |

#### 9.3.7 MQTT 侧带宽与资源精确值（回填 PRD §8.7 的另一行）

> **为什么单列**：PRD §8.7 有**两行**带宽（IEC 104 侧 / MQTT 侧），并要求"设计阶段须给出精确值并回填本表"。原稿只在 §9.2.2 给了 IEC104 侧，MQTT 侧缺失。本节按**真实点名与真实 JSON 字节**复算（可机械复现：逐点 `{"n":…,"v":…,"u":…,"q":…}` + 每条消息头 `{"ts","dev","station","role","seq","points"}` ≈ **110 B**；每点 ≈ **50 B**（点名 ≤ 20 B、值 ≤ 10 B、单位 ≤ 4 B、质量 ≤ 5 B + 引号逗号））。

**单条消息字节（按站按档，PCS 未启用）**：

| 站 | A 档（点/字节） | B 档（点/字节） | C 档首轮（点/字节） |
|----|----------------|----------------|---------------------|
| `grid_meter` | 6 / **372** | — | — |
| `meter_batt` | 9 / **551** | 31 / **1,711** | — |
| `bms` | 4 / **291** | **53** / **2,736** | **288** / **14,542** |
| `fire` | — | 114 / **5,922** | 13 / **768** |
| `hvac` | — | 3 / **249** | 31 / **1,670** |
| `pcs`（启用后） | 3 / **256** | 65 / **3,469** | 4 / **304** |

> **⚠️ 评审勘误① 在本增量内落地**：`bms` 行原写 B 档 **52** / C 档 **289**，与 PRD §8.3.1「遥测 **57** + 告警位 **288**」及本文 §9.3.3「bms **288** 位 1 条」冲突 ⇒ 订正为 **B 档 53（= 57 遥测 − 4 个 A 档点）/ C 档 288（= 告警位数）**，字节按本节同一模型（`点数 × 50 B + 该站消息头`）重算：`53×50+86 = 2,736`、`288×50+142 = 14,542`。其余各站与总线不受影响。
> **裁定 A 对本节无影响**：3 个新聚合组为 **IEC104-only**，不进 MQTT ⇒ 本节**点数全部不变**。

**带宽（稳态；COS 假设与 PRD §8.7 同：1 变位/s 稳态、10 变位/s 高峰）**：

| 项 | 计算（PCS 未启用） | 值 | PCS 启用 |
|----|------|-----|------|
| A 档 | 3 条/秒（grid + meter_batt + bms）= 372+551+291 | **1,214 B/s** | 4 条 = 1,470 B/s |
| B 档 | 4 条/5 秒 = 1,711+**2,736**+5,922+249 | **10,618 B / 5 s = 2,124 B/s** | **14,087**/5 = 2,818 B/s |
| C 档（稳态） | 1 变位/s（站内合并窗 200 ms ⇒ ≤ 1 条/s，≈ 150 B/条） | **≈ 150 B/s** | ≈ 150 B/s |
| **稳态合计** | | **≈ 3.5 KB/s ≈ 28 kbps** | **≈ 4.4 KB/s ≈ 35 kbps** |
| C 档高峰（10 变位/s） | 5 窗/s × (110 + 2×50) ≈ 210 B | +1,050 B/s ⇒ **≈ 4.5 KB/s ≈ 36 kbps** | ⇒ **≈ 5.5 KB/s ≈ 44 kbps** |
| **首轮全量快照（一次性）** | Σ（A + B + C 首轮） | **≈ 28.8 KB** | **≈ 32.8 KB** |
| 其中 C 档位块首轮（bms+hvac+fire） | **332** 点，3 条消息（**站间不合并**） | **≈ 17.0 KB** | 同 |

> **换算口径（本节统一，可复算）**：**KB/s = B/s ÷ 1000**、**kbps = B/s × 8 ÷ 1000**。例：启用稳态 `1,470 + 2,818 + 150 = 4,438 B/s` ⇒ `4.438 KB/s ≈ 4.4 KB/s`、`35.5 kbps ≈ 35 kbps` ✓（**与用户所述"实测 MQTT 稳态 4.4 KB/s"一致**）。
> **首轮全量算式**（未启用）：`A 372+551+291 = 1,214` + `B 1,711+2,736+5,922+249 = 10,618` + `C 14,542+768+1,670 = 16,980` ⇒ **28,812 B ≈ 28.8 KB**；PCS 启用再加 `256+3,469+304 = 4,029` ⇒ **32,841 B ≈ 32.8 KB**。**（v1.4-r2 订正：原稿写 29.1 / 33.0 KB，但其算式"624 点 × 50 B + 6 条消息头"与上表逐站值不符；本行改为按上表逐站求和，口径与其余各行一致。）**

> **与 PRD §8.7 的对照（v1.4-r2 重述；原"略超 5 KB/s"的张力已由裁定 B 关闭）**：
> ① **稳态 28 / 35 kbps ≤ 新上界 6 KB/s（48 kbps）** ✓（= 3,488 / 4,438 B/s vs 6,000 B/s ⇒ 余量 ≈ **72% / 35%**）。
> ② **C 档高峰（PCS 启用）≈ 5.5 KB/s ≤ 6 KB/s** ✓ —— **不再超界**；原稿"高峰 5.4 KB/s 超 5 KB/s"的处置方案（**把 `cos_merge_ms` 提到 2000 ms**）**作废**：该方案会破坏 §8.6.3 的"变化后 ≤ 1 s 内发出"，与裁定 B「**保 COS ≤ 1 s**」直接冲突。
> ③ **首轮快照 ≈ 28.8–32.8 KB**，**高于** PRD 原估的 20 KB（PRD 按"点名 18 B + 值/单位/质量 25 B = 43 B/点"估，实际点名更长：如 `mb_e_act_comb_1`）⇒ **已回填为 PRD §8.7 的"≤ 33 KB"**（取整上界，覆盖 PCS 启用的 32.8 KB）。

> ✅ **§9.8 Q-C（PRD §8.6.3「COS ≤ 1 s」vs §8.7「≤ 5 KB/s」不可兼得）—— 已由用户 2026-09-23 裁定关闭**：裁定为**放宽带宽上界**（IEC 104 → 16 kbps、MQTT → 6 KB/s 稳态），**COS ≤ 1 s 为硬约束**。`cos_merge_ms` **维持 200 ms**（v1.4 原默认即为保时延取值，裁定后无需改动）；**高峰瞬时不作带宽保证**（PRD §8.7 已显式声明），故不再需要"压带宽"的合并窗调整。

**资源增量（PRD §8.7 要求"CPU/内存增量须由设计给出"）**：

| 项 | 设计值 | 依据 |
|----|--------|------|
| 快照内存（共用件） | **≤ 128 KB**（**639** 点） | §9.1.6 |
| 点表常驻（`Vec<UplinkPoint>` **639** 条） | **≈ 90 KB**（条 ≈ 140 B：2×`String` + 3 字段 + 对齐） | 生成期一次，不可变 |
| `uplink_points.json`（落盘） | **≈ 121 KB**（**639** 条 × ~190 B JSON） | §9.2.1 启动期产物，供主站点表对点（RC-U74-02） |
| **MQTT 离线缓存（DB 断开期间）** | **典型 6–8 MB / 上界 ≤ 10 MB**（详见下） | 见下方计算 |
| IEC104 新增任务 | 3 条（A/B/C）+ 每连接 1 条写任务（既有结构） | §9.2.2 |
| MQTT 新增任务 | 2 条（A/B 定时）+ 1 条补送（重连期间） | §9.3.3 |
| CPU 增量 | **< 2%**（RK3588 @1.8 GHz）；编码在周期任务内完成，无逐点定时器 | 5 条常驻任务 × 每周期 ≤ 300 点的 JSON/ASDU 编码；量级参照 02 PRD §9.8.3 的 < 1% |
| 串口/网络新增占用 | **无新增串口**；MQTT 1 条出向 TLS 连接（既有） | PRD §8.7"上送侧不新增串口占用" |

> **离线缓存上界的算法（可复算）**：`cache.max_age_s = 1800` 与 `cache.max_messages = 10000` **先到先淘汰**。按上表，**生产节奏 = A 3 条/s + B 0.8 条/s ≈ 3.8 条/s、≈ 3.3 KB/s**（= 1,214 + 2,124 = 3,338 B/s）⇒ 30 min 内累计 **≈ 6,800 条 / ≈ 6.0 MB**（PCS 启用 ≈ 7.7 MB）—— **时间窗先到**（条数远未达 10,000）。故 **设计值声明：典型 6–8 MB，上界 ≤ 10 MB**（若站点数/档位数翻倍使条数先生效，则按 10,000 × ≈1 KB ≈ 10 MB 封顶）。
> **该 6–8 MB 是本次上云改造的**最大单笔内存增量** ⇒ 登记为待确认项（§9.8 Q-D）：建议投产按实测把 `max_age_s` 收敛到 **600–900 s**（≈ 2–3 MB），或在掉电不丢口径（PRD Q7 的方案②）确认后改为落盘。

---

### 9.4 装配点汇总（core-bin）

| 序 | 落点（**行号按 HEAD `724225c` 重校**） | 改动 |
|----|------|------|
| 1 | `startup.rs` 南向装配前（现 `:1255-1290` 段） | `LatestValues::new(cfg.south_stations.stale_timeout_s)` |
| 2 | `startup.rs` | `build_uplink_points(&cfg.south_stations)?`（失败拒启动）→ `Arc<Vec<UplinkPoint>>`；落 `uplink_points.json` |
| 3 | `SouthSink::new(..)`（现 `:1287` 调用 / `:414-430` 定义） | 新增 **`latest` + `grid_station_id` 两个入参**；三个 sink 回调写快照 + `mark_station_polled` + BMS 聚合求值（§9.1.4 / §9.2.1.1）。<br>**`grid_station_id` 的登记与理由（v1.13，评审偏离②）**：§9.1.4 要求 `on_grid_package` 内 `mark_station_polled(id)`，但**该回调入参不含站 id**，而 §9.1.1 **明禁改 `StationSink` trait** ⇒ 这是"取唯一不破 §9.1.1 禁令的解"——站 id 在**装配期**由 `south_stations.grid_station()` 解析一次、经构造参数注入；`None`（未配 `meter_grid`）⇒ **不写快照、不臆造站 id**。详见 §9.1.8 的"注入 `SouthSink`"行 |
| 4 | `startup.rs:486-521`（`broadcast_grid_iec104` + `grid_bcast_at`） | **删除**（并入 A 档任务，§9.2.2） |
| 5 | `startup.rs` 步骤 9（gateway 之后） | spawn `Iec104UplinkDriver { latest, points, server }`：A 档 / B 档 / C 档三条任务 |
| 6 | `StrategyCommandHandler`（定义 `startup.rs:201`、`impl` `:222`） | 新增 `latest` + `points` 字段；覆写 `on_interrogation`（快照 → `TelemetryItem`） |
| 7 | `startup.rs:1513-1556`（步骤 13） | MQTT 装配重写（§9.3.2）；`enabled=false` ⇒ 零连接尝试 |
| 8 | `mupc-core-bin/src/uplink.rs` | **新增文件**（两上送器 + 档位过滤 + JSON 组包 + 离线缓存） |
| 9 | 两份 deploy YAML（`deploy/config/mupc_core_config.yaml:87-90`、`…production.yaml:90-93`） | 新增 `mqtt_bridge:` 段；`auto_load` 移除 `mqtt_plugin` |
| **10** | **`startup.rs:1360-1409`（`south_sim_loop`）** | **删除其 IEC104 上送支路**（`:1365` `ioa_seq`、`:1390-1402` 的 `make_i_frame(0,0,…)` + `broadcast_telemetry`）——**保留** `set_latest_data` 注入（`:1378-1380`）与 `buffer_telemetry` 落库（`:1381-1385`）。理由见 §9.7 C-15 |

**新增/改动文件清单**：

| 文件 | 动作 |
|------|------|
| `mupc/crates/data-processing/src/latest_values.rs` | 新增（§9.1） |
| `mupc/crates/data-processing/src/lib.rs` | 导出 `latest_values::*` |
| `mupc/crates/mupc-southd/src/uplink.rs` | 新增（§9.2.1 生成器 + 档位表 + 聚合表） |
| `mupc/crates/mupc-southd/src/lib.rs` | `pub mod uplink;` |
| `mupc/crates/gateway/src/iec104/protocol.rs` | TypeID/COT 常量、帧编码修正、编码器 |
| `mupc/crates/gateway/src/iec104/command.rs` | `CommandHandler` 两个默认方法 + `TelemetryItem` |
| `mupc/crates/gateway/src/iec104/connection.rs` | `handle_i_frame` 总召分支；`take_just_connected`；S 帧调用点 |
| `mupc/crates/gateway/src/iec104/server.rs` | `publish_asdus`、逐连接 `OutboundSeq`、写任务改造、通道容量 |
| `mupc/crates/mqtt-bridge/src/topics.rs` | 函数式主题 |
| `mupc/crates/mqtt-bridge/src/config.rs` | `default` 改空串、删 `test_config` |
| `mupc/crates/mqtt-bridge/src/north_client.rs` | 证书到期解析（`x509-parser`） |
| `mupc/crates/mqtt-bridge/Cargo.toml` | `x509-parser = "0.16"`（**复用 workspace 现有版本**，与 `security` 一致，见 §9.3.5） |
| `mupc/crates/mupc-core-bin/src/quality_map.rs` | **新增**（`PointQuality → i32` 映射；落装配层，**不新增 `storage → data-processing` 依赖边**） |
| `mupc/crates/mupc-core-bin/src/core_config.rs` | `MqttBridgeConfig` 重定义 + `validate_mqtt_bridge` |
| `mupc/crates/mupc-core-bin/src/uplink.rs` | **新增**（两上送器） |
| `mupc/crates/mupc-core-bin/src/startup.rs` | 装配（§9.4） |
| `mupc/deploy/config/mupc_core_config.yaml` / `.production.yaml` | `mqtt_bridge:` 段 + `plugins.auto_load` 移除 `mqtt_plugin` |
| `mupc/crates/mupc-core-bin/Cargo.toml` | **无需改动**（`mqtt-plugin` 不在依赖表；插件按名动态加载） |

---

### 9.5 测试策略

| 层 | 用例（与 PRD §8.9 验收 ID 对应） |
|----|-----------------------------------|
| `data-processing` 单测 | 快照并发（多写多读）、`apply` 去重不广播、**`apply` 值不变仍刷新 `ts_ms`**（§9.1.5）、`mark_station_offline` 保原值 + 清站活性、`is_fresh` 边界（标量 =5 s 真/假各一侧；**位点 = 站级活性真/假各一侧**）、`Lagged` 后全量重读、内存上界（**639** 点构造 + `HashMap` 容量断言）→ AC-U74-12/13 |
| `mupc-southd` 单测 | `build_uplink_points`：**三通道点数断言**（IEC104 **162/234**、MQTT 552/624、并集 **567/639**）、**段基址**、**段内序**（块 addr 升序 + 位块在后）、**子集规则**（fire 排 `fire_det`、hvac 排 `hvac_in`、battery 位点不进 IEC104）、**聚合自检 G-1～G-5**（引用位存在 / 组间互斥 / 扣减排除表后全覆盖 / 排除表类别与 `point_table` 一致 / `EXCLUDED` 基数 == **78**）、**裁定 A 的 10 位逐位入组断言**（424–426 → 组 313、454–459 → 组 314、460 → 组 315；且 `EXCLUDED` 中**不含**这 10 位）、**`evaluate_bms_aggregates`**（OR 语义 / 组内部分缺位 ⇒ 质量降级且不写 0 / 全缺 ⇒ `None`）、配置变更 ⇒ 点数/IOA 变化（防手写常量回归）→ AC-U74-01 |
| `gateway` 单测 | I 帧 15 位序号往返（含 ≥ 128、32767 回绕 0）、S 帧字节、`encode_me_tf1`/`encode_sp_tb1` 字节级（含 CP56Time2a UTC 编解码）、**帧长断言（TI=36 ⇒ 25 B / TI=30 ⇒ 22 B）**、总召 `ACT_CON → 数据 → ACT_TERM` 顺序（用 mock writer）、QOI≠20 不静默忽略 → AC-U74-10 |
| `core-bin` 集成 | mock broker（本地 mosquitto）：点数 624/552、点名逐字比对、**分片不合并**、COS 不变不发、首轮 C 档全量、断线 60 s 补送（`ts` 为原采集时刻 + `seq` 单调）、缓存淘汰留证、TLS fail-closed（不存在的证书路径 / 过期证书 ⇒ 建连失败且无明文）、未配置段 ⇒ **零连接尝试**（日志断言）→ AC-U74-02/03/04/06/07/08 |
| **`core-bin` 集成（AC-U74-05 补行）** | **无效标记不假值**：mock 使某站采集失败 ⇒ ① MQTT 侧该站点位 `q` 变 `invalid`（`v` **保持原值**）或点从未采集时为 `null`（`q = unconfigured`），**断言 `v` 不为 0**；② 新增边界用例：**先注入"值为 0 的合法采样"，再注入站失败** —— 前者 `q = ok, v = 0`、后者 `q = invalid`，**二者在载荷上必须可区分**（这是"严禁用 0 顶替"的机械判据）；③ 站恢复 ⇒ `q` 回 `ok`；④ IEC104 侧同场景：该点从周期上送与总召响应中**消失**（不发 0）→ **AC-U74-05** |
| 本机 IEC104 客户端 | IOA 逐点对点（**162** 点）、总召、初始快照（**含连接前的值 + 装置长跑后稳态位点仍在**，§9.1.3 的位点活性回归）、站离线不发、PCS 段缺席、**带宽计数（60 s 滑动平均 ≤ 16 kbps；突发 ≤ 4.0 KB）→ AC-U74-14** |
| **本机 IEC104 客户端（勘误⑦ 补行）** | **COS 时延**：注入变位后，断言帧在 **≤ 1 s** 内发出（P99）；这是裁定 B 的**硬约束**，与带宽同批断言 → AC-U74-14 |
| **mock broker（勘误⑦ 补行）** | **MQTT 侧带宽计数**：稳态（60 s 滑动平均）**≤ 6 KB/s**、首轮全量突发 **≤ 33 KB**；同时断言 COS 合并窗 200 ms 下端到端 **≤ 1 s** → AC-U74-14 |
| 静态/结构 | 两份 YAML 的 `auto_load` 不含 `mqtt_plugin`（AC-U74-09；插件按名动态加载 ⇒ **只有配置层可断言**，无编译期依赖可断）；`uplink_points.json` 存在且与内存点表一致；**`south_sim_loop` 内不含 `broadcast_telemetry`/`make_i_frame`**（§9.4 序 10 的删除断言，防回退） |

### 9.6 增量实施顺序（依赖序，供排期）

1. `data-processing::latest_values`（含 `mark_station_polled` / 站级活性判据 / `is_fresh` 双语义）+ 单测（无消费方，可独立合入）
2. `mupc-southd::uplink` 生成器（含 `channels` 掩码、`BMS_AGGR_EXCLUDED`、G-1～G-5 自检、`evaluate_bms_aggregates`）+ 单测（无消费方）
3. `gateway` 协议修正（帧序号/编码器/总召接缝）+ 单测
4. core-bin 装配：快照写入 + 站活性 + **BMS 聚合求值** + `Iec104UplinkDriver` ⇒ **IEC104 半边闭环**（AC-U74-01/05(部分)/10/11/14 可验）
5. **删除 `south_sim_loop` 的 IEC104 假遥测支路**（§9.4 序 10）+ 静态断言
6. `core_config` MQTT 段 + validate + YAML（**先校验、后接线**）
7. `mqtt-bridge` 主题/默认值/证书到期（`x509-parser 0.16`）
8. core-bin `MqttUplinkPublisher` + 离线缓存 ⇒ **MQTT 半边闭环**（AC-U74-02..09）
9. 下架 `mqtt-plugin`、文档-代码一致性复核

---

### 9.7 与既有设计的冲突清单与取舍

| # | 冲突 | 取舍（本章裁定） |
|---|------|------------------|
| **C-1** | PRD §8.3.1 称 `grid_meter` 采集点数 **6**；而 `points::expand` 对该站配置（`mupc/deploy/config/mupc_core_config.yaml:413-426`）产出 **16** 点（p/q/pf/u/i 各 3 + `p_total_1`） | **按 6 落设计**（既有派生量 + IOA 1–6 不动，保"现场追认"）：MQTT 与 IEC104 的 grid 均为 6；**分相 15 点不进快照**（策略侧仍由 `AiIntegrator` 的 `DataPackage` 承载）。⇒ 若产品要分相上云，MQTT 全量变 639、grid IOA 段需重排（**破坏现场追认**），登记 §9.8 Q1 |
| **C-2** | PRD §8.3.1 合计行「231（**含 PCS 则 303**）」算术不自洽：231 已含 PCS 72；不含 PCS 应为 **159** | 按用户裁定与 AC-U74-01 的「PCS 未启用时不含 72」落设计：**启用 231 / 未启用 159**。**v1.4-r2 追加**：裁定 A 使两个值各 +3 ⇒ **启用 234 / 未启用 162**；该括注**已在本增量内落入 PRD §8.3.1**（订正为「PCS 未启用 **162**，启用 **234**」），C-2 由此**关闭** |
| **C-3** | PRD §8.7「IEC 104 使用带时标 TypeID」 vs 既有总表 6 点现行 **TI=13（无时标）** 且被定为"现场追认" | 新增点统一 **TI=36/30 带 CP56Time2a**；既有 6 点**默认同改**（方案 A，IOA 不变），**待主站/产品确认**（方案 B = 维持 13，则 §8.4 时标要求对总表作废）⇒ §9.8 Q2 |
| **C-4** | PRD §8.3.2 称 BMS 聚合"建议 **12** 个"但枚举出 **21** 项；且未列出位 340–363（单体充放电过温欠温等 24 位） | **取 12 组**（IOA 301–312 契约不变），组内构成与**逐位排除**见 §9.2.1；340–363 默认不入（与 303/304 语义重叠），**待产品确认**（并入则扩为 **16** 组）⇒ §9.8 Q3。**v1.4-r2 追加**：裁定 A 使组数 **12 → 15**（IOA **301–315**），PRD §8.3.2 已同步订正 |
| **C-5** | PRD §8.6.3「位块首轮 ≈319 点**允许合并为一条消息**」 vs §8.3.3「**不得**把多站数据合并进一条消息」 | **站内合并、站间不合并**：bms 288 位 1 条、hvac 31 位 1 条。理由：分片是**主题契约**（`{station_id}`），跨站合并会破坏订阅方路由；§8.6.3 的"允许合并"按"同一站内"解释 ⇒ 登记 §9.8 C-5 |
| **C-6** | `TypeId` 枚举**名称与标准值错位**（30 标 `MSpTa1` 实为 M_SP_TB_1；34/35 同名错位） | **只增别名与注释，不改既有变体值**（改值会破坏既有报文/用例）；新增 `MSpTb1=30` 别名、`MMeTf1=36`、`CIcNa1=100` ⇒ §9.8 C-6（命名错位登记为遗留） |
| **C-7** | `make_i_frame` 序号 7 位饱和 + 调用方恒塞 `seq=0` + `Connection::send_seq` 无自增 + `make_s_frame` 字节不合规（§9.2.3 表） | **同批修正**（本增量硬前提）：15 位编码、逐连接 `OutboundSeq`、S 帧签名改正；受影响断言 3 处同步更新（非弱化） |
| **C-8** | `Iec104Server::broadcast_telemetry(Vec<u8>)` 收完整 I 帧，序号由调用方决定 | **改 API** 为 `publish_asdus(Vec<Vec<u8>>, DataClass)`（ASDU 级别），旧方法 `#[deprecated]` 过渡；唯一调用方 core-bin 同批改 |
| **C-9** | `mqtt-bridge` 主题常量**无 `{station_id}` 分片**（`topics.rs:4-12`：`NORTH_TELEMETRY = "mupc/north/telemetry"` 等 7 个常量），PRD §8.3.3 强制分片 | 改为函数式 `north_telemetry(id)`/`north_event(id)`；旧常量保留 `#[deprecated]`；**01 设计 §5.5 北向主题表由 §9.3.4 的新表取代** |
| **C-10** | `mqtt-plugin`（北向客户端）与 `mqtt-bridge::NorthMqttClient` **两套并存且都空转**，且前者在 `auto_load` 里 | **下架 `mqtt-plugin`**（**唯一动作 = 从两份 YAML 的 `auto_load` 移除** + 删现场 `mqtt_plugin.so`；crate 标注 deprecated）⇒ §9.8 Q4（PRD Q6 建议①） |
| **C-11** | `mqtt-bridge/src/config.rs:52-68` 的 `NorthMqttConfig::default()` 指向 `mqtt.example.com:8883`（`:56`）+ dummy 证书（`:60-63`）；且 `test_config()`（`:73-78`）把它当测试替身 | `default()` 改**空串/空路径**；删 `test_config()`；装配点**禁止**再用 `::default()` |
| **C-12** | 01 设计 §4.2/§4.3 的 `MqttConfig` 字段名（`broker_addr`/`use_tls`）与 §9.3.2 的 YAML schema 不同层（YAML 是 core_config 层） | **分层映射**：YAML(`core_config.mqtt_bridge`) → Rust(`core_config::MqttBridgeConfig`) → `mqtt-bridge::NorthMqttConfig`（装配期逐字段映射，写成本函数 + 单测）。§4.2/§4.3 的**结构本身不改**（只补 `#[deprecated]` 说明它由装配层填充） |
| **C-13** | 快照中 grid 的 `voltage`/`current`/`cos_phi` 取 **A 相**、`frequency` 为**常量 50.0**（`mapper.rs:160-165`）；PRD §8.3.2 称表 6 点为"U / I / cosφ / f" | **如实照落**（不改口径，避免破坏现场追认），但**与 03 PRD Q5 同源**：这 4 个值进 MQTT 全量后，"频率恒 50.0"会污染云端统计 ⇒ §9.8 Q5（建议：与 03 号同批裁定"频率标无源"） |
| **C-14** | 01 设计 §2.3「上送队列：主站处理慢时采用背压，**优先丢弃过期遥测、保留最新值**」 vs 本设计的**分层背压**（A/B 丢弃计数、C 阻塞不丢） | 本设计**是**该口径的落地：A/B（周期遥测）丢旧留新；C（变位）不以"最新值"语义适用（变位是事件，丢即失语义）⇒ 分层，并在 §2.3 处以本章为准（登记） |
| **C-15**（v1.4 新增） | `south_sim_loop` 的**第二条 IEC104 上送路径**（`startup.rs:1360-1409`）：pv/load 南向模拟以 `ioa_seq` **自 1 递增**发 `make_i_frame(0,0,..)`，其 IOA 与段 1（总表 IOA 1–6）**相撞**；代码自注 `FIXME: IOA 分配和发送序号按连接维护，这里用固定值` | **删除其 IEC104 上送支路**（**保留** `set_latest_data` 注入与 `buffer_telemetry` 落库）。理由：① pv/load 是 `create_rs485_device` 造的**模拟设备**（`startup.rs:935`），**不在 `south_stations`** ⇒ 其点**无点表 IOA**，任何"给它编号"都是发明；② 该支路**只发有功一个量**（"取有功功率作为示例"），对主站无对点价值；③ **AC-U74-01 要求"收到点数 == 声明点数"**，任何点表外 IOA 都会破坏该验收；④ 该分支仅在 `!grid_on`（未配 `meter_grid`）时生效，属兜底场景，此时**主站本就无点表依据**。删除后 §9.4 序 10 与 §9.5 的静态断言共同防回退 |
| **C-16**（v1.4 新增） | 原稿写 `MqttUplinkPublisher` 持"**与 IEC104 同一份** `Vec<UplinkPoint>`"，但两条通道点集**不同**（IEC104 231 含 12 聚合；MQTT 624 不含） ⇒ 按字面实现必有一侧点数错 | `UplinkPoint` 新增 **`channels: ChannelMask`**；`build_uplink_points` 产**并集 639/567**，两侧各按掩码过滤（§9.2.1.0）。**15 个 BMS 聚合 = IEC104-only**（物联平台订阅 288 个原始位）；**288 位 + 114 探测器 + 3 温湿度 = MQTT-only**（不上主站） |
| **C-17**（v1.4 新增，**LV-1 例外 + 点名例外**） | ① **LV-1** 要求"任何合法消费方必须能读该入口"；本设计**明确排除 12 号 HMI 渲染进程直读**（它走 12 号既有的回环 HTTP 帧通道，`display_host.rs:1114`）；② PRD §8.3.3 要求 MQTT 的 `n` 与 02 号 §9.4.2.2 点名**逐字一致**，而 **grid 6 点用的是派生名**（`active_power`…，非 02 号点表展开名 `p_total_1`/`p_1`…） | ① **登记为 LV-1 的唯一例外**：12 号是**独立进程**，跨进程直读同一内存对象不可行（也不在 12 号设计授权内）；本入口是那条帧通道的**上游取数点** ⇒ 语义上仍满足"可读该入口"。② **登记 grid 6 点为点名的唯一例外**：它们是 `DataPackage.electrical` 的**派生量**（既有"现场追认"口径，MQTT/策略/03 号同源），改名为 `p_total_1` 会同时破坏 ①既有对点 ②`AiIntegrator` 的键；**请求 PRD 追认该例外**（§9.8 文档订正项） |
| **C-18**（v1.4 新增，**带宽口径**） | PRD §8.7 的两行带宽是"按 18 B/帧 / 43 B/点"的**上界量级**；本设计按真实帧长（**25 B**）与真实点名长度复算后：IEC104 稳态 7.3/10.7 kbps ✓、**PCS 启用 + C 档高峰 12.3 kbps 略超 12 kbps**；MQTT 稳态 28/35 kbps ✓、**首轮快照 29–33 KB 高于 PRD 估 20 KB**、**PCS 启用 + 高峰 5.4 KB/s 略超 5 KB/s** | **以设计精确值为准并请求 PRD 回填**（PRD §8.7 自注"不得作为精确值引用"）。**v1.4-r2（裁定 B 关闭本行）**：PRD §8.7 上界已放宽为 **IEC 104 16 kbps / MQTT 6 KB/s（均按 60 s 滑动平均）**，并显式声明"**高峰瞬时不保证**" ⇒ ①②③ 三项超界**全部消散**（10.7 / 12.3 ≤ 16 kbps；5.5 ≤ 6 KB/s）；首轮快照已回填为"**≤ 33 KB**"（设计值 28.8 / 32.8 KB，PCS 启用）。**SQ=1 顺序 ASDU 压缩由"必需"降为"可选余量"**；`cos_merge_ms` **维持 200 ms**（保 COS ≤ 1 s） |
| **C-19**（v1.4-r2 新增，**基线变更**） | 用户 2026-09-23 裁定 A 把 10 位安全位纳入 ⇒ **IEC104 点数契约（159/231）与 PRD §8.3.2「段 4 = 301–312（12 点）」同时失效**；且该 10 位在 `point_table.rs` 中本为 `Alarm`（原稿误标 Reserved 才"从未进入讨论"） | **按裁定落设计并同步 PRD 最小订正**：段 4 **301–315（15 组）**、IEC104 **162/234**、并集 **567/639**。**只改数字与必要说明**，**不改需求语义**（段 4 仍是"BMS 遥信聚合段"，不引入单列位）。备选形态（2 组 / 10 单列）登记 §9.8 **Q-B-1** |

### 9.8 待产品 / 项目经理裁定项

| # | 事项 | 选项 | 本设计默认 |
|---|------|------|-----------|
| Q1 | grid 分相 15 点是否上云（C-1） | (a) 不上（保持 6 点） (b) 上（MQTT 639 点，IOA 段 1 扩到 16） | **(a)** |
| Q2 | 既有总表 6 点是否同批改带时标 TI（C-3） | (a) 同改 TI=36 (b) 维持 TI=13（§8.4 时标要求对总表作废） | **(a)**，须主站确认 |
| Q3 | BMS 聚合组数（C-4） | (a) 12 组（IOA 301–312） (b) 13 组（并入位 340–363） | **(a)**；**Q-A/Q-B 为其细分项**。**v1.4-r2**：裁定 A 后组数为 **15**（IOA 301–315），本行作为"原始议案"保留 |
| Q4 | 北向 MQTT 承载收敛（C-10，PRD Q6） | (a) 下架 `mqtt-plugin` (b) 两套并存 | **(a)** |
| Q5 | grid `frequency`（常量 50.0）与总 PF（取 A 相）是否上云（C-13，= 03 Q5） | (a) 照落并在文档标注 (b) 标"无源"不上云 | **(b) 倾向**，与 03 号同批裁定 |
| Q6 | 装置标识 `dev` 来源（PRD Q10） | (a) 装置序列号 (b) 其它 | **(a)**；现状无来源 ⇒ 无值写 `null`，**不臆造** |
| Q7 | 仿真环境明文 MQTT（PRD Q9） | (a) 允许（显式开关 + 非生产 + 响亮告警） (b) 一律禁 | **(a)**，默认 `allow_plaintext: false` |
| Q8 | 证书到期告警门限（PRD Q11） | (a) 30 天 (b) 其它 | **(a)** |
| Q9 | 离线缓存上限（PRD Q8） | (a) 30 min (b) 10000 条 | **两者先到先淘汰**（= PRD 建议） |
| Q10 | 分组召唤（PRD Q5/GI-4） | (a) 本轮不做（回 ACT_CON+TERM + 事件） (b) 本轮做 | **(a)** |
| **Q-A**（v1.4 新增） | **24 位单体温度/电压细分 Alarm**（位 340–363：充/放电过温欠温 ×3、温升过大 ×3、极柱温度过/欠温 ×6、电压变化过大 ×3…）是否入主站聚合（C-4 的细分） | (a) 不入（与 304/303 语义重叠，主站要总貌） (b) 入 —— 加第 **16** 组 `bms_aggr_cell_temp_charge_discharge`，**段 4 扩为 301–316**（v1.4-r2 后编号随裁定 A 顺延） | **(a)**；改表不改码 |
| **Q-B**（v1.4 新增，**高优先**）✅ **已裁定并关闭（2026-09-23，用户）** | **60 位 Alarm（位 424–483）是否入主站聚合** —— 含**簇一/二/三级告警、AFE 故障、继电器粘连 ×6、绝缘检测低 ×3、从控/主控故障、SOE 低、MOS 过温、正/负极柱温度…**（⚠️ 原稿把它们误标为 Reserved，故从未进入任何讨论） | (a) 不入 (b) 入 | **裁定：采纳 (b) 的"最小范围"** —— 用户裁定原文为「**至少**纳入**簇级告警 / AFE 故障 / 继电器粘连**这三类」⇒ **本轮仅纳入这 10 位**：`424–426`（簇一/二/三级告警，3 位）、`454–459`（继电器粘连，6 位）、`460`（AFE 故障，1 位）。**其余 50 位（427–453、461–483）维持不入**，仍留 `BMS_AGGR_EXCLUDED`（§9.2.1.3）。**落地形态**：新增 **3 个聚合组**（313/314/315），段 4 = **301–315**。**（本条由 2026-09-23 用户裁定引起，待复审确认）** |
| **Q-B-1**（v1.4-r2 新增，**待裁定**） | **Q-B 的落地形态是否保持"3 个聚合组"** —— 即：三类是否必须**各自成组**（当前采纳），或可合并 / 单列 | (a) **3 组各自成组**（当前采纳；跨类不丢语义） (b) 合并为 2 组（`454–460` 合为 `bms_aggr_power_path_fault`，省 1 个 IOA） (c) **10 位单列 IOA**（主站可定位到"具体哪个继电器 / AFE"；**IOA 起止依方案而定**：「原 12 聚合 + 10 单列」= **301–322**、「3 新聚合 + 10 单列」= **301–325**（= 15 聚合 + 10 单列）；代价 = +10 IOA 且**段 4 语义由"聚合段"变"混合段"⇒ 须改 PRD §8.3.2 需求口径**） | **(a)**；若主站要求"继电器定位到具体某一路"则须改 (c)，**属改需求，须产品/主站发起** |
| **Q-C**（v1.4 新增）✅ **已裁定并关闭（2026-09-23，用户）** | **MQTT C 档时延 vs 带宽**：PRD §8.6.3「变化后 ≤ 1 s 发出」与 §8.7「稳态 ≤ 5 KB/s」在"PCS 启用 + COS 高峰 10 变位/s"下**不可兼得**（§9.3.7 C-18） | (a) 保时延（`cos_merge_ms = 200`，高峰 ≈ 5.5 KB/s，请 PRD 订正上界） (b) 保带宽（合并窗 ≥ 1000 ms，违反 ≤1 s 时延） (c) 降 COS 点位范围（只发 Alarm 类） | **裁定：采纳 (a)** —— 用户裁定「**放宽带宽上界**（保 COS ≤ 1 s）」⇒ 新上界 **IEC 104 ≤ 16 kbps / MQTT ≤ 6 KB/s**（60 s 滑动平均；**高峰瞬时不作保证**），`cos_merge_ms` **维持 200 ms**。§9.3.7 / PRD §8.7 / AC-U74-14 已同步。**（本条由 2026-09-23 用户裁定引起，待复审确认）** |
| **Q-D**（v1.4 新增） | **MQTT 离线缓存内存（6–8 MB，上界 10 MB）** 是否可接受（§9.3.7） | (a) 接受 30 min 窗 (b) 收敛到 10 min（≈ 2–3 MB） (c) 改落盘（须先与 03 号划界，PRD Q7 方案②） | **(a) 可接受但建议 (b)**；投产按实测收敛 |
| **Q-E**（v1.4 新增） | MQTT 载荷 `ts` 的**线序**（语义已定：采集时刻 UTC 毫秒，与 PRD §8.3.3 一致） | (a) **ISO-8601 字符串**（`"2026-09-23T08:00:00.123Z"`，与 PRD 字段表字面一致） (b) 整数 epoch 毫秒（`1758604800123`） | **(a)**；若物联平台侧解析器只吃整数 ⇒ 改 (b)（一行格式化代码） |
| C-2 | 订正 PRD §8.3.1 合计行括注（"含 PCS 则 303" → 不含 PCS **162** / 含 PCS **234**） | 纯文档订正 | ✅ **v1.4-r2 已落入 PRD §8.3.1**（含 §8.3.2 段 4 = 301–315） |
| C-5 | PRD §8.6.3「允许合并为一条消息」与 §8.3.3 分片要求的关系 | 在 PRD 加一句"合并限同一站内" | 建议 PRD 澄清 |
| **C-17（文档订正）** | PRD §8.3.3「`n` 必须与 02 号 §9.4.2.2 点名逐字一致」与 grid 6 点派生名的冲突（§9.7 C-17 ②） | 在 PRD 加一句例外："总表 6 点沿用派生名（现场追认）" | 建议 PRD 追认 |
| **C-18（文档订正）** | PRD §8.7 两行带宽/快照量级按 18 B/帧估，与设计精确值不符 | 以本设计 §9.2.2 / §9.3.7 的精确值 + **裁定 B 的新上界**回填该两行 | ✅ **v1.4-r2 已落入 PRD §8.7**（含 AC-U74-14 判据订正） |

---

## 附录 A：验收标准参考

验收标准（IEC 104 / IEC 61850 / MQTT / 消息总线 / 安全 / 质量）详见 [PRD 第 7 章](../specs/modules/01-MUPC-通信网关-PRD.md#7-验收标准汇总)。

---

---

## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| **v1.4-r4（2026-09-24，T11 实现暴露的 §9.2.1.x 口径勘误与登记）** | **只订正 §9.2.1.1 的伪码与登记 4 项设计缺口，不改任何点数/IOA/掩码/档位/自检判据**。来源 = **T11 `mupc-southd::uplink` 实现（`c5af8b8`）+ 规格/质量评审**（规格 **SPEC_COMPLIANT**、质量 **APPROVED_WITH_CONCERNS**；3 次注入探针）。<br>**① 勘误（安全相关）**：§9.2.1.1 **伪码分支 2** 原只写"**全部可读位** `q == Ok`"⇒ 缺位时按字面返回 `(Some(OR), Ok)` = **把"未读全"报成确证无告警（静默漏报）**，与同节正文"不可得的位…通过质量降级表达"**矛盾**。**已修正伪码为"全部可读位 `q == Ok` **且无缺位**"**，并加勘误块（含判别锚 `evaluate_partial_missing_*`；改回旧字面口径 ⇒ 2 条用例红，已注入验证）。<br>**② 登记（签名决定，非缺陷）**：`evaluate_bms_aggregates` 的签名`&dyn Fn(u16) -> Option<(f64, PointQuality)>` **拿不到站级状态** ⇒ 伪码"全组不可得 ⇒ **站位质量**"**无法按签名实现**；实现取 `Unconfigured`（§9.1.2 语义"从未采集过"），**写入侧可按站级状态覆盖**。本行即其登记。<br>**③ 登记（欠规格）**：**站内相对次序**（MQTT-only 条目 vs IEC104 条目）设计未定义，只定义了各自内部次序；实现取"**IEC104 条目在前（标量→聚合/位）、MQTT-only 在后**"，并以 `build_uplink_points_is_deterministic` 钉住（全文件无 `HashMap` 迭代）。<br>**④ 登记（命名歧义）**：§9.2.1 表写 `BMS_ALARM_GROUPS`、§9.2.1.1 写 `BMS_AGGR_GROUPS` —— **同一常量两个称谓**；实现**只落 `BMS_AGGR_GROUPS`**（避免双真源），本行即其裁定。<br>**⑤ 登记（空条件）**：§9.2.1 的"段内**标量在前、位点在后**"子句在**当前点表下不可观测**（无任何 IEC104 段同时含标量与位点）⇒ 无用例可钉，属**空条件**。<br>**⑥ 登记（Q-A/Q-B 的落地代价）**：§9.2.1.3 附近"改表不改码"的说法**过于乐观** —— 若纳入 Q-A（340–363）/ Q-B 余项，除改表外**须同步 5 处契约常量**（`AGGR_COVERED_BITS` / `AGGR_EXCLUDED_BITS` / `check_channel_counts` 的期望表 / `expect_abc`），否则会**拒启动**。<br>**未改**：§9.2.1 的点数/IOA 分配/`channels` 掩码/排除表、§9.2.1.2 的 15 组定义、§9.2.1.3 的 G-1…G-5 判据、§9.2.2–§9.2.6、§9.1/§9.3/§9.4 与 §1–§8、任何代码、PRD、12 号设计；**既有门禁标记**（`[DESIGN_APPROVED: 2026-09-23, 设计评审员]` 原文未动，覆盖范围仍为 §9 v1.4 增量）。**本增量未经设计复审**（勘误 + 登记类，不构成新契约）。 |
| **v1.4-r3（2026-09-23，T1 代码评审的 2 项设计侧偏离登记）** | **纯登记，只动 §9.1.3 / §9.1.8 / §9.4 序 3 三处，不改任何语义、数字口径或实现要求。** 来源 = **T1 `latest_values` 代码评审**（`[CODE_REVIEWED: PASS: 2026-09-23]` + `[TEST_PASSED: 2026-09-23]`，报告 `…-2026-09-23.md` §T1 的"偏离清单"①②）。<br>**偏离①（警告）**：12 设计 §15.1.2 / §15.9 的 **R-38** 要求"该站最近一次成功采集时刻"读口，实现已就位（`latest_values.rs:249`）而 §9.1.3 全表未列 ⇒ **§9.1.3 补一行** `pub fn station_last_poll_ms(&self, station: &str) -> Option<u64>`（**签名以 T1 实现为准**），并注明"**不自建第二套新鲜度真源**：过期判据仍唯一由 `is_fresh` / `station_is_active` 持有，真源仍是注入的 `stale_timeout_s` = 5 s"。<br>**偏离②（警告）**：`SouthSink::new` 实为"新增 `latest` + `grid_station_id` **两个**入参"（实现在 `startup.rs:420/424-443/456-459`），而 §9.4 序 3 只写"新增 `latest` 入参" —— 该参数是"**取唯一不破 §9.1.1「禁改 `StationSink` trait」禁令的解**"（`on_grid_package` 入参不含站 id，只能在装配期由 `south_stations.grid_station()` 解析一次注入；`None` ⇒ 不写快照、**不臆造站 id**）⇒ **§9.4 序 3 与 §9.1.8 已补登记该参数及其理由**。<br>**未改**：§1–§8、任何代码/PRD/12 号设计、**既有门禁标记**（`[DESIGN_APPROVED: 2026-09-23, 设计评审员]` 原文未动，覆盖范围仍为 §9 v1.4 增量）。**本增量未经设计复审**（登记类，不构成新契约）。 |
| **v1.4-r2（2026-09-23，用户裁定回写）⚠️ 待复审确认** | **本条由 2026-09-23 用户裁定引起，待复审确认**。**只改 §9 与 PRD §8 的数字与必要说明，不改需求语义、不改代码、不改 03/12 号文档。** ① **裁定 A（Q-B 关闭）**：把位 **424–426 / 454–459 / 460 共 10 位**（簇级告警 / 继电器粘连 / AFE 故障）纳入 IEC104，**落地形态 = 新增 3 个聚合组**（`bms_aggr_cluster_level` / `bms_aggr_relay_stuck` / `bms_aggr_afe_fault`），段 4 **301–312 → 301–315**；**其余 50 位维持不入**。② **裁定 B（Q-C 关闭）**：**放宽带宽上界**（IEC 104 ≤ **16 kbps**、MQTT ≤ **6 KB/s**，60 s 滑动平均；**高峰瞬时不作保证**；`cos_merge_ms` 维持 200 ms 保 COS ≤ 1 s）⇒ PRD §8.7 与 **AC-U74-14** 判据同步重写。③ **全量重算**：`card(A∪S) = 221 = 143 + 78`（覆盖 133→**143**、排除 88→**78**）；IEC104 **159/231 → 162/234**；并集 **564/636 → 567/639**；A/B/C 档 **19/84/56 → 19/84/59**；突发 **3,975/5,775 → 4,050/5,850 B**；**帧长与稳态/高峰带宽不变**（由变位率假设驱动）。④ **原 v1.4 的 7 项勘误中 ①②⑦ 一并落地**（§9.3.7 `bms` 行 52→53 / 289→288 并重算下游；§9.1.6「289 点」→345 点；§9.5 补 MQTT 侧 + COS 时延带宽测试行），**③④⑤⑥ 未动**。⑤ 新增 **C-19**（基线变更）与 **Q-B-1**（落地形态待裁定：3 组 / 2 组 / 10 单列）。**门禁标记未改**（保留 v1.4 的 `[DESIGN_APPROVED: 2026-09-23]` 及其"有条件项"原文）。 |
| v1.0 | 初版：综合 5 份来源文档，定义通信网关实现级设计 |
| v1.1 | 补全 `ControlCommand` 一次调频参数、明确 UTC 时标规范（CP56Time2a）、补充跨模块引用 |
| v1.2 | 新增 IEC 61850 MMS 客户端实现级细节（ASN.1 BER 编码、PDU 构造、send_request 流程、测试策略），归档实施计划 |
| **v1.4-r1（2026-09-23）** | **设计评审员复审：`[DESIGN_APPROVED: 2026-09-23]`**（仅覆盖 §9 增量）。**验收结果**：P1 已解决（§9.2.1.1 写入侧求值点，`scheduler.rs:669/683` 每轮两口幂等核实）；P2 已解决且**算术精确**（424–483 逐位核为 `BitClass::Alarm`；Alarm 212 + State 9 = **221 = 133 + 88**；排除表 4+24+60 = 88；Reserved 67 不参与）；P3 已解决（TI=36 ⇒ **25 B**、TI=30 ⇒ **22 B**、既有 TI=13 ⇒ 18 B；A/B/C = 19/84/56 = 159、22/153/56 = 231；7.3/10.7 kbps 与突发 3.9/5.6 KB 均可复算）；P4 已解决（PRD §8.3.3 原文即"UTC ISO-8601（毫秒）"，Q-E 登记合理）；P5 已解决（`:1378` 与 `:1390` 两支路同受 `!grid_on` 门控，两份部署模板均含 `meter_grid` ⇒ 现状为死码）；P6 已解决（`:33`/`:37`/`:29`/`:41` 正确、`_44`/`_46`/`_74`/`_76` 确不存在，72 = 43+1+27+1 均按 `positional` 复推通过）；P7 **部分解决**（抽查 ≈45 处，40+ 精确，偏差见勘误④）；AC-U74-05 补测试行 ✓；MQTT 侧精确值内部自洽 ✓；`x509-parser` 0.16 ✓（`crates/security/Cargo.toml:16` 与 `Cargo.lock:5237` 均 0.16.0）；§9.2.1 逐点枚举（bms 57 = 31+10+8+4+4、meter_batt 40、fire 13、hvac 34、`soc` = `bms_io_19`、`mb_power_7/_15`、`fire_det_count`、PCS 72）**逐项回源通过**。**须随本批开发同一提交修正的勘误（7 项，不改变裁决）**：① §9.3.7 的 `bms` 行 B 档 **52 → 53**、C 档 **289 → 288**（与 PRD §8.3.1「遥测 57 + 告警位 288」及本文 §9.3.3「bms 288 位 1 条」冲突；总线与其余各站不受影响）；② §9.1.6「`station_snapshot("bms")`（**289 点**）」→ **345 点**（57 标量 + 288 位）；③ §9.1.8「**步骤 8**（南向调度装配）」误标 —— 步骤 8 = 策略引擎，南向装配在步骤 9 段（约 `:1269–1290`）；④ §9.2.5「`impl` `:222`」→ **`:225`**、§9.2.3「`required_type_ids` `:366-373`」→ **`:365-380`**；⑤ 删除 sim 支路后，**无 `meter_grid` 部署**（`grid_on=false`）将"IEC104 零上送"⇒ 须补启动 WARN 与 C-15 文案；⑥ hvac 站 `interval_ms = 5000` 恰等于 `stale_timeout_s = 5000`（配置期仅对 `meter_grid`/`battery` 强制 `< 5000`）⇒ 该站 3 个 B 档标量点新鲜度**零余量**，须登记或留余量；⑦ AC-U74-14 缺 **MQTT 侧**带宽测试行（§9.5 仅列 IEC104 侧）。**开发前必决**：Q-B（424–483 是否入聚合，冻结 159/231 基线）> Q-C/C-18（PRD §8.6.3「COS ≤ 1 s」与 §8.7「≤ 5 KB/s」在 PCS + 高峰不可兼得；IEC104 峰值 12.3 kbps 略超 12 kbps）> Q2（总表 6 点 TI=36/13 须主站确认） |
| v1.4 | **按设计评审意见修订 §9（P1–P7 逐条闭合），设计评审复审通过（`[DESIGN_APPROVED: 2026-09-23]`）**：① **P1** 补 BMS 12 组聚合的**运行期求值点**（写入侧 core-bin `SouthSink`、每轮 `on_station_telemetry` 后触发、经唯一写入口 `apply` 进快照、求值器 `uplink::evaluate_bms_aggregates` 纯函数），并定义 OR/质量降级/不可得三态（§9.2.1.1）；② **P2** 重述生成期自检为 **G-1～G-5**（扣减 `BMS_AGGR_EXCLUDED` 后集合相等），排除表**逐位重建为 88 位并按 `point_table` 实际 `BitClass` 标注**（⚠️ 更正原稿把 **424–483 的 60 位 Alarm 误标 Reserved**，从而从未进入讨论 ⇒ 新增 Q-B）；③ **P3** 重算帧长（TI=36 ⇒ **25 B**、TI=30 ⇒ **22 B**）与档位点数（**19/84/56 = 159**；PCS 启用 **22/153/56 = 231**），回填**精确带宽**与突发量；④ **P4** `ts` 对齐 PRD §8.3.3（**UTC ISO-8601 毫秒字符串**），注明线序由设计定、并登记为待确认项 **Q-E**（若对端要整数 epoch 则改一行格式化）；⑤ **P5** **删除 `south_sim_loop` 的第二条 IEC104 假遥测上送支路**（C-15）；⑥ **P6** 按 `positional` 规则**重推 PCS 段全部点名**（总 P `pcs_3zone_33`、总 Q `_37`、总 PF `_41`、总视在 `_29`；不存在 `_44/_46/_74/_76`）；⑦ **P7 全部 `file:line` 按 HEAD `724225c` 全量重校**。另：新增 **`channels` 通道掩码**（C-16：IEC104 231 / MQTT 624 / 并集 636，原稿"同一份点表"按字面实现必错）；新增**位点走站级活性的新鲜度裁定**（否则长跑装置接主站时稳态位点会整批被判过期，与 PRD §8.6.3「位块首轮全量 319 点」冲突）；补 **AC-U74-05 测试行**；补 **§9.3.7 MQTT 带宽/资源精确值**；`x509-parser` 改 **0.16**（复用 workspace 版本）；登记 **LV-1 对 12 号间接读的例外**与 **grid 6 点派生名例外**（C-17）、**带宽口径偏差**（C-18）。**未改**：PRD、代码、12 号设计、既有门禁标记 |
| v1.3 | **新增 §9 外设数据上云（U-74，含 U-71「MQTT 真做」）设计增量**（对应 01 PRD §8，`[REVIEWED: PASS: 2026-09-23]`）：① **§9.1 最新值入口**——归属裁定为 `mupc-data-processing::latest_values`（03 模块的 crate ⇒ 03 PRD「本模块提供」字面成立；`southd → data-processing` 与 `core-bin → data-processing` 依赖已存在，零新增边；写入方 = core-bin `SouthSink`），定义 `PointValue/PointQuality/PointId/ChangeBatch`、变更广播（`Lagged` ⇒ 全量重读）、过期判据单一真源（注入 `stale_timeout_s`）、内存上界（624 点 ≤ 128 KB）、与 `AiIntegrator`/`display_host`/`collector`/`telemetry` 四面的边界；② **§9.2 IEC 104**——IOA 表由 `mupc-southd::uplink::build_uplink_points` **机械生成**（段基址 + 段内序 + 子集规则 + 生成期自检，消灭手写常量），**231 点**（PCS 未启用 159）+ **12 组 BMS 告警聚合**（逐位覆盖/排除表）+ A/B/C 档与分层背压 + **帧编码修正**（15 位 I 帧序号、逐连接 `OutboundSeq`、S 帧签名）+ **总召 `C_IC_NA_1`**（ACT_CON→数据→ACT_TERM）+ **连接初始快照**；③ **§9.3 MQTT 真做**——`core_config.mqtt_bridge` 段 schema + 8 条 validate（未配置即不连接）、承载收敛（下架 `mqtt-plugin`）、生产者接内存快照（禁轮询 DB）、`{station_id}` 分片主题（**§5.5 原表由 §9.3.4 取代**）、内存离线缓存（时间窗 ∨ 条数先到先淘汰 + 留证）、TLS fail-closed 与凭据不入日志/载荷（新增 `x509-parser` 做到期监控）；④ §9.7 **14 条冲突清单与取舍**（含 PRD 合计行算术不自洽 C-2、时标口径 C-3、聚合组数 C-4、分片/合并 C-5）；⑤ §9.8 **10 项待裁定**。**未改**：PRD、12 号设计、§4 的 MQTT 分层结构（仅标注装配层填充）、既有门禁标记 |
