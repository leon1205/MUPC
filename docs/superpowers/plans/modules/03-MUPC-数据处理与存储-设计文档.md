# MUPC 数据处理与存储模块 技术设计文档

> ✅ **`[DESIGN_APPROVED: 2026-09-23, 设计评审员]`** —— **§9「总表电气量聚合落库与存储参数可配置（U-69 / U-67 / U-68③ / U-70）」v1.3 增量复审通过**（上轮 P1–P5 逐条闭合：§9.0 基线逐项标"已修/未修"、`value` 可空化 + 幂等迁移、FLS 交付边界逐条给实现行号、缓冲上界 10 000、映射改落装配层 `quality_map.rs`；`storage/src/services.rs` 行号抽查全部精确）。**有条件项**：4 项文档级勘误（受影响代码点清单误列 `high_freq_telemetry.rs`、"共 12 处"计数、3 处行号微差、迁移仅重建 2 个索引中的 1 个）须随本批开发同一提交修正。**该标记仅覆盖 §9 增量**；§1–§8 既有正文与门禁标记未改。

> **版本：** **v1.3-r4**（2026-09-26，**FLS-03② 告警聚合按 PRD R-11.5-A4 字面改为边沿触发**）—— 承 **v1.3-r3**（2026-09-26，**评审收尾：W-1…W-8 + `AlertFeed` 消费侧登记**）← **v1.3-r2**（2026-09-26，开发交付登记：FLS-03② / FLS-04 落地）← **v1.3-r1**（2026-09-23，设计评审员复审 `[DESIGN_APPROVED: 2026-09-23]`，**仅覆盖 §9 增量**）← **v1.3**（2026-09-23，按设计评审意见修订；设计评审复审通过）

> **v1.3-r4 开发交付（2026-09-26，只改 §9.3 / §9.6（序 12 行号）/ §9.7 的状态与行号 + 本注记，不改 §9 其余口径；本增量未经设计复审）**：**FLS-03② 的告警聚合由"增量 > 0 即发"改为【边沿触发】**，按 03 PRD **v1.1.2**（提交 `effbaa0`）的产品裁定（R-11.5-A4 字面）落地 —— **进入**丢弃态投**恰好 1 条** `major`（含本拍增量 + 累计值）；**持续**丢弃期间**一条都不再发**（各拍增量并入 `episode_points/episode_batches`）；**恢复**（增量回落 0）**只记 `tracing::info!` 日志**（含本段总计 + 累计值）、**不发告警**（理由：`AlertFeed` 只有入、无 ack/清除面，`major` 级"已恢复"与"新发生一次 `major`"对按级过滤的消费者不可区分）；**再次进入**再 1 条 ⇒ **连续丢弃 10 周期 = 1 条**（R-11.5-A4 / FLS-03 的字面判据），**Q-A4 关闭**。**为什么原口径不算聚合**：v1.3-r2/r3 把"只在增量 > 0 时发"读成"自然按周期聚合"—— 它只做到"无丢弃时不发"，在"**每个周期都在丢**"时仍每周期 1 条（10 周期 = 10 条），故当时如实登记"字面未逐字满足"；那是**误读**，**不是**"设计选定口径"（当时那句"按设计选定口径充分"随之作废）。**实现落点（新基线）**：边沿状态机 `storage_health.rs:145-187`、三态 `HealthSignal` `:89-105`、1 s 循环 `:198-241`（`emit` 只在进入臂被调）、生产接线 `:249-275`、级别常量 `:80`、周期常量 `:76`；装配点 `startup.rs:1545-1552`（**本批注释 +2 行 ⇒ 本批基线为 `:1476-1483`；本分支合并后因 PCS 装配段插入再位移至 `:1545-1552`**；仅注释口径同步为边沿触发）。**判别锚（用例）**：`:345` 进入 ⇒ 恰 1 条含两数；`:369` **连续丢弃 10 拍 ⇒ 累计仍恰 1 条**（核心判据）；`:409` 恢复 ⇒ `DropRecovered`（非新告警）；`:440` 恢复后再次进入 ⇒ 2 条；`:329` 无丢弃 ⇒ 0 条；`:493` 任务层同判据（并保留"窗口内又跑了 ≥N 拍"+"窗口内累计丢弃确实在涨"两条假绿排除）；`:321` 级别字面量；`:643` 装配接线/协作名单。**"能红"探针**：把 `poll` 的边沿判定改回"增量 > 0 即发" ⇒ `:369` 与 `:493` 即红（任务层实测 11 拍 11 条 = 告警风暴），`cp` 还原后 `sha256sum`+`cmp` 逐字节一致。⚠️ **行号基线换新**：本批插入 `HealthSignal`/边沿位 ⇒ §9 内引用 `storage_health.rs` 的行号**整体重校**（旧 `:41/:45/:78-92/:101-129/:137-163/:190/:199/:233/:366` → 新 `:76/:80/:145-187/:198-241/:249-275/:321/:329/:345/…`）；`startup.rs` 装配点旧 `:1462-1477` → **`:1545-1552`**；`storage` 侧行号与 §9.0/§9.1 **未动**。**未改**：PRD（需求侧一字未动）、§9.1/§9.2/§9.9 的口径与数字、§4/§6/§7 各口径、顶部既有 `[DESIGN_APPROVED: 2026-09-23]` 标记。

> **v1.3-r3 评审收尾（2026-09-26，只改 §9.2.1 / §9.2.2 / §9.3 与注记，不改语义口径；本增量未经设计复审）**：① **W-1（P2）** `batch_capacity` 下界 **1 → 2**（`capacity = 1` 时采集路径永不 `trim_oldest` ⇒ `max_points + 1` 上界失效）：取**路线①**（`validate_storage` 拒 1 + 错误文案给理由），§9.2.1 表 / §9.2.2 骨架同步；**与 PRD R-11.3-E 的差异已登记、PRD 未改、待产品确认**。② **W-3** `buffer_telemetry` 范围 4 处 `:313-353` 统一为 `:313-343`。③ **W-4** "微秒级"补"与 tick 同时就绪时最多顺延一拍（不丢数据）"。④ **`AlertFeed` 消费侧**如实登记：生产侧在投、`subscribe()` 无消费者 ⇒ FLS-03② 的 `major` 不得读成现场可观测告警。⑤ **Q-A4 / Q-p99 原样登记、不代为裁定**。**未改**：PRD、§4/§6/§7、§9.1/§9.9 口径与数字、顶部既有 `[DESIGN_APPROVED: 2026-09-23]` 标记。

> **v1.3-r2 开发交付（2026-09-26，只改 §9.3 / §9.6 / §9.7 / §9.8 的状态与行号，不改任何语义口径）**：本批把 v1.3 的**两个残留缺口**落地并逐条给出实现行号 —— ① **FLS-04（S-5）**：`buffer_telemetry` 的容量触发**不再在采集调用栈内 `await` 提交**：不再 drain、改投一次**非阻塞**唤醒（`services.rs:355-380`，`mpsc::Sender::try_send`，容量 1、满即合并），DB 活（`begin/INSERT/commit`）搬进**已注册**的 `spawn_flush_timer` 任务（`:476-513` 的 `select!` 第三臂 + `flush_wake_once` `:621-632`）⇒ **采集调用栈里没有任何 DB 调用点**。**未取**设计推荐的 `tokio::spawn(flush_batch(batch))` 最小改法：它需改签名（`'static`）且会造出**不登记在退出编排里**的游离任务 —— 正是 T15/T16 刚修掉的"退出期窄竞态"形态（`startup.rs:459-478` 同一条裁决）⇒ 取"**已在退出编排里的**任务干 DB 活"这一等价替代。**② FLS-03②（S-3②/S-4）**：新增 `mupc-core-bin/src/storage_health.rs`（1 s 巡检，读 `dropped_points()/dropped_batches()` 的**增量**，增量 > 0 才投**恰好一条** `major` 告警，文案含**增量条数 + 累计值**；无增量即静默 ⇒ 缺口 2 的聚合由同一判据闭合），装配点 `startup.rs:1462-1477`（句柄入 `producers` 协作退出名单）。**用例/判别锚**：`storage_health.rs:190/199/233/366`（级别字面量、无增量不发、每周期恰好 1 条、"静默窗内又跑了 ≥4 拍"的假绿排除、装配接线）、`services.rs:1050`（采集入口无 `.await`/无 `flush_batch` 的**结构网**）、`storage/tests/integration.rs:418/458`（唤醒由已注册任务接走；**正常库上容量触发后库里 0 行**且 `pool.close()` 后采集侧仍拿不到 `Err`）。**⚠️ 未取证项（不得据此宣称达标）**：FLS-04 的 **p99 ≤ 10 ms** 需真机/压测，本机不具备 ⇒ 以"**采集调用栈内无 DB 调用点**"的结构判据替代，**HST-ELEC-02 回归须在同一压测中一并复核**；`buffer_telemetry` 的 `Err` 分支因此**退化为永不触发**（返回值恒 `Ok`，签名保留、调用点零改动）。**⚠️ 行号基线漂移（本批引入，必须登记）**：本批在 `WriteBuffer` 段插入代码 ⇒ §9.0/§9.1/§9.3 中**位于该段之后**的 `services.rs` 行号整体后移。对照（旧 `724225c` → 本批工作区）：`DEFAULT_MAX_BUFFERED_POINTS :207 → :208`；`since_attempt :228-238 → :248`；`trim_oldest :308-320 → :371`；`log_dropped :322-330 → :385`；`requeue_front :338-355 → :401`；`buffered_points :358-360 → :421`；`max_points :363-365 → :426`；`dropped_points :368-371 → :431`；`dropped_batches :374-377 → :437`；`requeued_batches :380-383 → :443`；`spawn_flush_timer :407-439 → :476-513`；`flush_batch :470-503 → :544-578`；`capacity :532 → :606`；`flush_interval_ms :536 → :610`；`BatchGuard :550-599 → :643-692`；`RetentionManager :602-639 → :695-732`；`run_migrations :647 → :762`；`ensure_telemetry_value_nullable :650-661 → :888`。**§9.0/§9.1/§9.4 的全量行号未在本批逐条重校**（登记为待办）。**未改**：PRD（需求侧一字未动）、§9.1/§9.2/§9.9 的口径与数字、§4/§6/§7 各口径、**顶部既有 `[DESIGN_APPROVED: 2026-09-23]` 标记**（该标记的覆盖范围仍是 §9 v1.3 增量，**本批未经设计复审**）。

> **v1.3 修订（2026-09-23，按设计评审意见，只改 §9）**：逐条闭合评审 P1–P5 —— ① **P1 基线重校**：§9.0 的行号改写为 HEAD `724225c`，并**逐项标注"已修/未修"**（原稿把已随 U-68③ 落地的"回填/有界/计数"写成未实现）；② **P2 `NoData` 值映射**：`AggregateRow.value: Option<f64>` 与 `telemetry.value REAL NOT NULL` 的冲突已裁定 —— **`value` 列改为可空、缺测行写 `NULL`**（唯一与 PRD R-11.2-E「不得写入 0」字面一致的做法），并给出**幂等迁移**与**5 处代码点**（§9.1.4 / §9.6 序 9）；③ **P3 FLS 交付边界**：§9.3 由"期望接缝、不交付实现"改为「**与已合并实现（`724225c`）的对接说明 + 残留缺口**」——FLS-01/02/05 与 FLS-03①③ **已交付**（逐条给实现行号），FLS-03②（`major` 事件）与 FLS-04（采集路径仍 await）**未交付**，给出**可编码的落地方案**；④ **P4 上界口径统一**：采用实现值 **`10_000`**（`DEFAULT_MAX_BUFFERED_POINTS`）并说明依据、登记与 PRD R-11.5-A2 的偏差（新 Q-9/C-3）；⑤ **P5 依赖边**：`Quality::from_point_quality` **改落装配层** `mupc-core-bin/src/quality_map.rs`（放 `storage` 会新增 `storage → data-processing` 依赖边，现无该边，新 D-8）。另：§9.7 测试行同步（GRD-04 改断言 `value IS NULL`、新增 GRD-09 迁移幂等、FLS 逐条标注状态）。**未改**：PRD、代码、既有门禁标记、§4.2/§4.3/§4.4 粒度、§6.1 保留期、§7.3 容量规划。
>
> **v1.2 增量（2026-09-23）**：新增 **§9 增量设计：总表电气量聚合落库与存储参数可配置（U-69 / U-67 / U-68③ / U-70）**（落 03 PRD §11 `[REVIEWED: PASS: 2026-09-23]`）——① §9.1 `mupc-storage::GridAggregator`（1 分钟聚合，18 通道均值 + 2 通道极值 = **22 行/周期**，周期起点时标、无采样产行、重启不回溯）；② §9.2 `core_config.storage` 段（三键、默认值 = 现实现、非法值拒启动）；③ §9.3 **落库失败语义期望接缝**（`TelemetrySink::offer` 非阻塞 + 回填/上界/计数/聚合告警；**只描述接缝，实现由同期开发承接**）；④ §9.5 最新值入口归属确认（**唯一真源 = 01 设计 §9.1**）；⑤ §9.8 冲突清单 + §9.9 待裁定项。**未改** PRD、`storage`/`core-bin` 代码、§4.2/§4.3/§4.4 粒度口径、§6.1 保留期、§7.3 容量规划。

---

## 目录

- [1. 模块架构](#1-模块架构)
- [2. 遥测采集设计](#2-遥测采集设计)
- [3. 故障录波设计](#3-故障录波设计)
- [4. 历史数据存储设计](#4-历史数据存储设计)
- [5. 设备台账管理设计](#5-设备台账管理设计)
- [6. 接口定义](#6-接口定义)
- [7. 文件结构](#7-文件结构)
- [8. 技术决策记录](#8-技术决策记录)
- [9. 增量设计：总表电气量聚合落库与存储参数可配置](#9-增量设计总表电气量聚合落库与存储参数可配置u-69--u-67--u-68--u-70)

---

## 1. 模块架构

### 1.1 整体架构定位

数据处理与存储模块是 MUPC "异构双核心模块主控架构"中**非实时处理核心（大脑）**的核心数据处理组件，承担以下职责：

- **数据采集**：从 intercore 模块接收实时控制模块的高频采样数据，汇聚为统一数据源
  > ⚠️ **2026-09-26：PCS 通道已迁至南向（02 §13）；本节所述的核间数据面在生产路径未启用。**
  > PCS（= 实时控制模块）的通信与控制现由 `mupc-southd::pcs::PcsHandle` 承担（02 号设计 §13），
  > 原 `intercore` 的 Modbus RTU 通道已整体迁出；核间 TCP 帧协议仅作后续演进保留（客户端**只发不收**，
  > 且**生产路径暂无消费者**，02 号设计 Δ-23 / `technical-debt.md` §6.13）。下文同类表述同此注。
- **遥测上送**：以 >= 1Hz 频率将遥测数据通过消息总线分发给消费者（gateway、strategy-engine 等）
- **故障录波**：检测故障条件时录制故障前后波形，支持波形数据的存储、查询、导出和北向上报
- **历史数据存储**：持久化存储周期性电气量数据、电池运行数据、告警日志和系统事件记录
- **设备台账管理**：管理管辖范围内所有设备的资产信息、铭牌参数、维护记录，支持北向上送
- **数据生命周期管理**：按配置策略自动清理过期数据，保障存储空间合理使用

### 1.2 模块关系图

```
实时控制模块 (小核 ADC)
     │
     ▼ (TCP/RJ45 10ms 周期数据帧)
intercore (核间通信)
     │
     ▼ (DataCollector 接收)
data-processing (数据处理 crate)
     ├── collector        → DataCollector（数据采集）
     ├── high_freq_telemetry → 高频遥测 1Hz 上报
     ├── reporter         → DataReporter（消息总线发布）
     ├── recorder         → FaultRecorder trait（故障录波）
     ├── fault_recorder_impl → FaultRecorderImpl（SQLite + 波形文件）
     ├── waveform/        → 故障录波子模块（环形缓冲区、触发引擎、存储、导出、上报）
     │   ├── sampling/ring_buffer/trigger
     │   ├── storage/export
     │   └── report
     └── database         → SQLite 数据库操作
          │
          ▼
mupc-storage (存储 crate，新增)
     ├── StorageService   → 统一存储入口
     ├── AssetService     → 设备台账管理
     ├── TelemetryService → 遥测历史数据管理
     ├── AlarmService     → 告警日志管理
     ├── EventService     → 事件记录管理
     ├── LifecycleService → 数据生命周期管理
     ├── ExportService    → 数据导出
     ├── WriteBuffer      → 异步批量写入器
     └── DbPool           → 读写连接池 (SQLite WAL)
          │
          ├── → SQLite (元数据 + 时序数据)
          ├── → 文件系统 (波形文件 .wave)
          └── → 导出目录 (COMTRADE/CSV)
               │
               ▼
          gateway (IEC 104 / MQTT 北向上报)
          strategy-engine (策略决策)
          web-api (REST API 查询)
```

### 1.3 与上下游模块的关系

| 上游模块 | 数据流向 | 说明 |
|----------|----------|------|
| intercore | → data-processing | TCP/RJ45 高频采样数据（10ms 间隔瞬时值帧）—— ⚠️ **2026-09-26：PCS 通道已迁至南向（02 §13）；本行所述的核间数据面在生产路径未启用** |
| rs485-plugin / hplc-plugin | → data-processing | 南向设备数据采集（Phase 2+ 预留） |
| rs485-plugin / hplc-plugin | → mupc-storage | 设备自动注册（初始化时注册台账） |

| 下游模块 | 数据流向 | 说明 |
|----------|----------|------|
| data-processing → gateway | 遥测、故障、台账上送 | 通过消息总线 + 直接调用 |
| data-processing → strategy-engine | 遥测数据 | 通过消息总线 |
| data-processing → mupc-storage | 遥测/告警/事件持久化 | 通过 WriteBuffer 异步写入 |
| mupc-storage → web-api | 历史数据、台账、告警查询 | REST API 查询接口 |

### 1.4 数据流架构

```
遥测数据流（高频）:
  intercore → DataCollector → HighFrequencyTelemetry → 消息总线(telemetry.high_freq)
                                                          ├── gateway (北向上送)
                                                          └── strategy-engine (策略决策)

遥测数据流（持久化）:
  DataCollector → WriteBuffer → 批量事务(容量1000条 或 间隔5000ms，先到先执行) → SQLite WAL (按月分区)

故障录波数据流:
  intercore(WaveformSample帧) → DualBufferManager(环形缓冲区)
      → TriggerEngine(触发判定) → capture_waveform() → .wave文件 + SQLite元数据
      → WaveformReporter → MQTT/IEC 104 北向上报

设备台账数据流:
  web-api REST → AssetService → DeviceRepo → SQLite
  plugins → auto_register() → AssetService → DeviceRepo → SQLite
  SQLite → gateway → IEC 104/MQTT 北向上送(定时/变更触发)
```

> **实现说明（2026-08-14）**：当前 Phase 1 实现中，遥测数据流为**南向设备直接采集**（`rs485-plugin` 的 `Rs485Device.read()` → 采集循环 → `DataPackage` → WriteBuffer + gateway 北向上送 + AI 融合引擎），尚未经 intercore 中转。`intercore → DataCollector` 数据源为 Phase 2+ 演进方向（接入实时控制模块后切换）。DataCollector/DataReporter/MessageBus 组件保留作为 Phase 2+ 组件化改造基础。

---

## 2. 遥测采集设计

### 2.1 DataCollector — 数据采集

**职责**：从 intercore 模块接收实时控制模块的数据，汇聚为统一数据源。

> ⚠️ **2026-09-26：PCS 通道已迁至南向（02 §13）；本节所述的核间数据面在生产路径未启用。**

#### 接口定义

```rust
pub trait DataCollector {
    async fn start(&mut self) -> Result<(), DataProcessingError>;
    async fn stop(&mut self) -> Result<(), DataProcessingError>;
    fn get_latest_data(&self) -> Option<TelemetryData>;
}
```

#### DataCollectorImpl 实现

```rust
pub struct DataCollectorImpl {
    /// 数据接收通道（从 intercore）
    receiver: Option<mpsc::Receiver<DataPackage>>,
    /// 最新数据缓存
    latest_data: Arc<std::sync::Mutex<Option<DataPackage>>>,
    /// 存储服务引用（可选，集成持久化时注入）
    storage: Option<Arc<StorageService>>,
}

impl DataCollectorImpl {
    pub fn new() -> Self;
    pub fn with_storage(self, storage: Arc<StorageService>) -> Self;
    pub async fn try_collect(&mut self) -> Result<DataPackage, DataProcessingError>;
    pub fn get_latest_data(&self) -> Option<DataPackage>;
}
```

#### 数据来源与采集类型

数据来源：intercore 模块（TCP/RJ45），10ms 周期数据帧。

> ⚠️ **2026-09-26：PCS 通道已迁至南向（02 §13）；本节所述的核间数据面在生产路径未启用。**

| 数据类型 | 说明 | 单位 |
|----------|------|------|
| battery_soc | 电池荷电状态 | % |
| battery_power | 电池充放电功率 | kW |
| pv_output | 光伏出力 | kW |
| load_power | 负荷功率 | kW |
| grid_power | 电网功率（有功） | kW |
| transformer_load | 变压器负载率 | % |

**数据包内容**：电气量、电池数据、设备状态、UTC 时间戳。

#### 验收标准

| ID | 验收条件 | 验证方法 |
|----|----------|----------|
| DP-DC-01 | DataCollector 能从 intercore 接收数据 | 单元测试 |
| DP-DC-02 | get_latest_data() 返回最新的有效数据，无数据时返回 None | 单元测试 |
| DP-DC-03 | start()/stop() 可多次调用，不产生重复资源分配 | 单元测试 |

### 2.2 HighFrequencyTelemetry — 高频遥测

**职责**：以 >= 1Hz 频率上报遥测数据到消息总线。

#### 接口定义

```rust
pub trait HighFrequencyTelemetry {
    async fn start(&mut self) -> Result<(), DataProcessingError>;
    async fn stop(&mut self) -> Result<(), DataProcessingError>;
    fn get_current_value(&self, point: &str) -> Option<f64>;
}
```

#### HighFreqTelemetryImpl 实现

```rust
pub struct HighFreqTelemetryImpl {
    /// 上报周期 (ms)
    period_ms: u64,
    /// 是否运行
    running: bool,
    /// 内存缓冲 (Ring Buffer, 60 条)
    buffer: Arc<Mutex<VecDeque<TelemetryPoint>>>,
    /// 发送通道
    sender: Option<mpsc::Sender<TelemetryPoint>>,
}
```

**上报频率**：1Hz（可配置，动态调整采集周期）。

**内存缓冲**：保留最近 1 分钟数据（60 条记录），使用 VecDeque 环形缓冲。

**消息主题**：

| 主题 | 生产者 | 消费者 | 说明 |
|------|--------|--------|------|
| `telemetry.high_freq` | DataCollector | strategy-engine, gateway | 高频遥测数据 |
| `strategy.decision` | strategy-engine | gateway, intercore | 策略决策结果 |

#### 验收标准

| ID | 验收条件 | 验证方法 |
|----|----------|----------|
| DP-HF-01 | HighFrequencyTelemetry 以 1Hz 上报数据 | 单元测试 |
| DP-HF-02 | 数据在内存中缓冲 60 条 | 单元测试 |
| DP-HF-03 | 支持动态调整采集周期 | 单元测试 |

### 2.3 DataReporter — 数据上报

**职责**：通过消息总线将处理后的数据发送给消费者（gateway、strategy-engine 等）。

#### 接口定义

```rust
pub trait DataReporter {
    async fn report(&self, data: TelemetryData) -> Result<(), DataProcessingError>;
    fn subscribe(&mut self, topic: &str) -> Result<(), DataProcessingError>;
}
```

#### 验收标准

| ID | 验收条件 | 验证方法 |
|----|----------|----------|
| DP-DR-01 | DataReporter 通过消息总线发送数据 | 单元测试 |
| DP-DR-02 | 支持订阅指定主题，收到消息时触发回调 | 单元测试 |

### 2.4 与 intercore 集成

#### 2.4.1 高频采样数据帧格式

当前 intercore 协议使用定长 64 字节帧。为传输 10 通道高频采样数据，增加新的帧类型 `WaveformSample = 0x0040`。

**波形采样数据帧格式（FrameType = 0x0040）：**

```
┌───────────────────────────────────────────────┐
│ Frame Header (8 bytes)                        │
│   magic(0xAA55) + length + type(0x0040) + seq │
├───────────────────────────────────────────────┤
│ Payload (48 bytes):                           │
│   timestamp: i64 (8 bytes)   微秒级时间戳      │
│   ua: f32 (4 bytes)         A相电压瞬时值(V)   │
│   ub: f32 (4 bytes)         B相电压瞬时值(V)   │
│   uc: f32 (4 bytes)         C相电压瞬时值(V)   │
│   ia: f32 (4 bytes)         A相电流瞬时值(A)   │
│   ib: f32 (4 bytes)         B相电流瞬时值(A)   │
│   ic: f32 (4 bytes)         C相电流瞬时值(A)   │
│   u0: f32 (4 bytes)         零序电压瞬时值(V)   │
│   i0: f32 (4 bytes)         零序电流瞬时值(A)   │
│   p: f32 (4 bytes)          有功功率瞬时值(kW)  │
│   q: f32 (4 bytes)          无功功率瞬时值(kVar)│
│   freq: f32 (4 bytes)       频率(Hz)           │
├───────────────────────────────────────────────┤
│ CRC16 (2 bytes)                                │
│ Padding (6 bytes)                               │
├───────────────────────────────────────────────┤
│ Total: 64 bytes                                 │
└───────────────────────────────────────────────┘
```

**说明：** 使用 f32 而非 f64，核间通信以太网链路带宽有限（10/100Mbps），f32 可满足 0.1% 精度要求同时减半带宽占用。波形存储时由 data-processing 转换为 f64。

**传输速率计算：**

| 采样率 | 帧间隔 | 每秒帧数 | 带宽需求 |
|--------|--------|----------|----------|
| 1kHz   | 1ms    | 1000     | 64KB/s   |
| 4kHz(默认) | 250us | 4000   | 256KB/s  |
| 16kHz  | 62.5us | 16000   | 1MB/s    |

#### 2.4.2 intercore 帧类型扩展

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u16)]
pub enum FrameType {
    // ... 已有类型 ...
    Connect         = 0x0001,
    HeartbeatReq    = 0x0002,
    ControlCmd      = 0x0010,
    DataUpload      = 0x0030,
    // === 新增 ===
    WaveformSample  = 0x0040,  // 高频采样数据帧
}
```

#### 2.4.3 data-processing 数据接收适配器

```rust
pub struct IntercoreSampleSource {
    rx: mpsc::Receiver<IntercoreFrame>,
    sampler: Arc<DualBufferManager>,
    trigger_engine: Arc<TriggerEngine>,
}

impl IntercoreSampleSource {
    pub async fn run(mut self) {
        while let Some(frame) = self.rx.recv().await {
            if frame.header.frame_type != FrameType::WaveformSample { continue; }
            // 解析 → 构建 SamplePacket → 写入环形缓冲区 → 触发判定
        }
    }
}
```

---

## 3. 故障录波设计

### 3.1 总体架构

故障录波模块归属于 `data-processing` crate，作为该 crate 的 `waveform` 子模块存在。数据来源为 intercore 核间通信模块提供的 10ms 周期高频采样数据帧，输出到本地文件系统（波形文件）和 SQLite（元数据），并通过 gateway 的 IEC 104 和 MQTT 通道上报北向。

```
┌──────────────────────────────────────────────────────────────┐
│                    data-processing crate                      │
│  ┌─────────────┐   ┌──────────────────────────────────┐     │
│  │  collector   │──▶│   waveform (子模块)               │     │
│  │ (DataCollect)│   │  ┌──────────────────┐            │     │
│  └─────────────┘   │  │  RingBuffer       │ 环形缓冲区  │     │
│                     │  ├──────────────────┤ 双缓冲区    │     │
│  ┌─────────────┐   │  │  TriggerEngine    │ 触发判定    │     │
│  │high_freq_    │──▶│  ├──────────────────┤ 回差逻辑    │     │
│  │telemetry     │   │  │  StorageManager  │ 文件读写    │     │
│  └─────────────┘   │  ├──────────────────┤ 容量管理    │     │
│                     │  │  ComtradeExporter│ COMTRADE    │     │
│  ┌─────────────┐   │  ├──────────────────┤ CSV 导出    │     │
│  │ fault_       │   │  │  WaveformReporter│ 北向上报    │     │
│  │ recorder_    │◀──│  └──────────────────┘            │     │
│  │ impl.rs      │   └──────────────────────────────────┘     │
│  └─────────────┘                                            │
└──────────────────────────────────────────────────────────────┘
```

### 3.2 波形采样架构

#### 3.2.1 环形缓冲区设计

采用**固定大小预分配 Vec + 写入游标**实现，避免运行时动态内存分配。

```rust
pub struct RingBuffer {
    /// 存储矩阵: [channel_count][capacity]，通道连续存储
    data: Vec<f64>,
    /// 通道数 (≤ 10)
    channel_count: usize,
    /// 缓冲区容量（每个通道的样本数）
    //  容量 = sample_rate × max(pre_trigger_ms, post_trigger_ms) / 1000
    capacity: usize,
    /// 写入游标（下一个写入位置，0..capacity 循环）
    write_cursor: usize,
    /// 总写入计数（单调递增，用于计算触发偏移量）
    total_written: u64,
    /// 时间戳缓冲区（每个采样点对应一个微秒时间戳）
    timestamps: Vec<i64>,
}
```

**缓冲区容量计算：**
- 默认配置：4000 Hz × max(200 ms, 1000 ms) = 4000 采样点/通道
- 总内存：10 通道 × 4000 点 × 8B + 4000 × 8B ≈ 352 KB

**关键操作：**

```rust
impl RingBuffer {
    pub fn new(channel_count: usize, capacity: usize) -> Self;
    /// 写入一个采样点（所有通道在 t 时刻的值），O(1)
    pub fn push(&self, samples: &[f64], timestamp: i64);
    /// 从指定偏移量开始读取 N 个采样点
    pub fn read_range(&self, trigger_offset: usize, pre_samples: usize, post_samples: usize) -> Vec<Vec<f64>>;
    pub fn current_position(&self) -> (usize, u64);
    pub fn reset(&self);
}
```

#### 3.2.2 线程安全设计

```rust
use parking_lot::RwLock;

pub struct SafeRingBuffer {
    inner: Arc<RwLock<RingBuffer>>,
}
```

- 生产者（高频采样写入）持写锁，极短持有时间（仅 memcpy）
- 消费者（触发表决、波形读取）持读锁
- 使用 `parking_lot::RwLock` 而非 `std::sync::RwLock`，前者更轻量

#### 3.2.3 双缓冲区机制

两个环形缓冲区交替工作，确保连续故障不丢失数据。

```
                    ┌─────────────────────┐
 稳态采样 ─────────▶│  RingBuffer A (活动)  │──▶ 新数据覆盖旧数据
                    └─────────────────────┘

    故障触发
        │
        ▼
                    ┌─────────────────────┐
                    │  RingBuffer A (冻结)  │──▶ 等待读取 + 写入文件
                    └─────────────────────┘
                    ┌─────────────────────┐
                    │  RingBuffer B (活动)  │──▶ 继续采样（收集 post-trigger 数据）
                    └─────────────────────┘
    录制完成
        │
        ▼
                    ┌─────────────────────┐
                    │  RingBuffer A (重置)  │──▶ 恢复就绪状态
                    └─────────────────────┘
```

**双缓冲区管理器：**

```rust
pub struct DualBufferManager {
    buffers: [SafeRingBuffer; 2],
    active_index: AtomicUsize,
    last_used_index: AtomicUsize,
    state: AtomicU8,  // 0=IDLE, 1=CAPTURING, 2=SAVING
    post_trigger_start: AtomicUsize,
}

impl DualBufferManager {
    pub fn push_samples(&self, samples: &[f64], timestamp: i64);
    pub fn trigger(&self) -> Result<(usize, usize, usize, usize), WaveformError>;
    pub fn capture_waveform(&self, ...) -> WaveformData;
    pub fn release_buffer(&self, buffer_idx: usize);
}
```

**连续故障处理：**

```
场景：500ms 内发生两次故障

t=0          t=200ms    t=500ms     t=1200ms    t=1700ms
 │   Fault A  │          │  Fault B   │            │
 ▼            ▼          ▼            ▼            ▼
BufA: 采样 → 冻结(pre) → 录故障后(post t+1000ms) → 保存完成 → 释放
BufB:          采样(活动) → 冻结(pre) → 录故障后 → 保存完成 → 释放

第三次故障(双缓冲皆满):
  → 检查最旧缓冲区是否已保存完成 → 释放 → 复用
```

#### 3.2.4 多通道同步采样

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveformChannel {
    Ua = 0, Ub = 1, Uc = 2,  // 三相电压 (V)
    Ia = 3, Ib = 4, Ic = 5,  // 三相电流 (A)
    U0 = 6,  // 零序电压 (V)
    I0 = 7,  // 零序电流 (A)
    P  = 8,  // 有功功率 (kW)
    Q  = 9,  // 无功功率 (kVar)
}

pub const WAVEFORM_CHANNEL_COUNT: usize = 10;

/// 通道组掩码
pub struct ChannelMask(u16);
impl ChannelMask {
    pub const VOLTAGE_3PHASE: u16 = 0b0000_0000_0111;
    pub const CURRENT_3PHASE: u16 = 0b0000_0011_1000;
    pub const ZERO_SEQUENCE: u16 = 0b0000_1100_0000;
    pub const POWER: u16        = 0b0011_0000_0000;
    pub const ALL: u16          = 0b0011_1111_1111;
}
```

**通道同步保证：**
1. intercore 数据帧中所有通道数值使用同一个时钟边沿采集（ADC 触发信号硬件同步）
2. 软件层面，`push_samples()` 的 `samples` 切片所有元素对应同一时间戳，精度 ≤ 100μs
3. 每秒执行一次同步校验：注入 50Hz 已知正弦波，确认各通道相位差 ≤ 1°

**采样数据封装：**

```rust
pub struct SamplePacket {
    pub channels: [f64; 10],
    pub timestamp: i64,         // 微秒级 unix 时间戳，精度 ±100μs
    pub quality: SampleQuality,
}

pub enum SampleQuality {
    Good,
    GapDetected,    // 单个采样点丢失
    MajorGap,       // 连续 10+ 采样点丢失
}
```

### 3.3 触发判定引擎

#### 3.3.1 触发条件配置

```rust
pub struct TriggerConfig {
    // 过压触发
    pub over_voltage_enabled: bool,
    pub over_voltage_threshold: f64,     // 默认 420.0 V
    pub over_voltage_hysteresis: f64,    // 默认 10.0 V
    // 欠压触发
    pub under_voltage_enabled: bool,
    pub under_voltage_threshold: f64,    // 默认 200.0 V
    pub under_voltage_hysteresis: f64,   // 默认 10.0 V
    // 过流触发
    pub over_current_enabled: bool,
    pub over_current_threshold: f64,     // 默认 150.0 A
    pub over_current_hysteresis: f64,    // 默认 5.0 A
    // 短路触发
    pub short_circuit_enabled: bool,
    pub short_circuit_threshold: f64,    // 默认 500.0 A（瞬时值）
    // 频率越限触发
    pub freq_upper_enabled: bool,
    pub freq_upper_limit: f64,           // 默认 50.5 Hz
    pub freq_lower_enabled: bool,
    pub freq_lower_limit: f64,           // 默认 49.5 Hz
    pub freq_hysteresis: f64,            // 默认 0.1 Hz
    // 零序过流触发
    pub zero_seq_enabled: bool,
    pub zero_seq_threshold: f64,         // 默认 20.0 A
    // 通用配置
    pub debounce_samples: u32,           // 防抖确认窗口（默认 3）
    pub sample_rate: u32,                // 采样率: 1k/2k/4k/8k/16k Hz
    pub pre_trigger_ms: u32,             // 故障前记录时长 (40~1000ms, 默认 200)
    pub post_trigger_ms: u32,            // 故障后记录时长 (40~5000ms, 默认 1000)
    pub channel_mask: ChannelMask,       // 通道启用掩码
}
```

| 触发条件 | 参数 | 默认值 | 单位 |
|----------|------|--------|------|
| 过压触发 | 阈值/回差 | 420.0/10.0 | V |
| 欠压触发 | 阈值/回差 | 200.0/10.0 | V |
| 过流触发 | 阈值/回差 | 150.0/5.0 | A |
| 短路触发 | 阈值 | 500.0 | A（瞬时值） |
| 频率越限 | 上限/下限/回差 | 50.5/49.5/0.1 | Hz |
| 零序过流 | 阈值 | 20.0 | A |

#### 3.3.2 触发状态机

```rust
pub struct TriggerEngine {
    config: Arc<RwLock<TriggerConfig>>,
    states: [TriggerState; 6],       // 每个条件独立状态
    debounce_counters: [u32; 6],     // 防抖计数器
    cooldown_until: AtomicI64,       // 冷却时间
}

pub enum FaultTriggerType {
    OverVoltage = 0, UnderVoltage = 1, OverCurrent = 2,
    ShortCircuit = 3, FrequencyAbnormal = 4, ZeroSeqOverCurrent = 5,
}

enum TriggerState { Normal, Triggered, HysteresisWaiting }
```

**触发判定流程（每个采样点到达时同步执行）：**

```
1. 冷却检查
   ├── 当前时间 < cooldown_until → 跳过，返回 NO_TRIGGER
   └── 当前时间 >= cooldown_until → 继续

2. 对每个启用的触发条件执行：
   ├── Normal → 检查是否满足阈值
   │   ├── 满足 → debounce_counter++ → >= debounce_samples → 触发
   │   └── 不满足 → debounce_counter = 0
   ├── Triggered → 检查是否退出回差区
   │   └── 完全回到正常范围 → state = Normal
   └── HysteresisWaiting → 检查是否退出回差区

3. 有至少一个条件触发 → TRIGGERED，否则 NO_TRIGGER
```

#### 3.3.3 触发结果

```rust
pub struct TriggerResult {
    pub triggered: bool,
    pub trigger_types: [Option<(FaultTriggerType, f64)>; 6],
    pub trigger_offset: usize,
    pub trigger_count: usize,
}
```

**防抖机制：** 引入 3 个连续采样点的确认窗口，防止瞬态尖峰（< 1ms）误触发。

**回差机制：** 触发后需信号回到"阈值 +/- 回差"范围内才解除，防止临界值附近反复触发。

#### 3.3.4 采样参数配置

| 参数 | 取值范围 | 默认值 | 说明 |
|------|----------|--------|------|
| 采样率 | 1k/2k/4k/8k/16k Hz | 4kHz | 每通道每秒采样点数 |
| 故障前记录时长 | 40~1000ms | 200ms | 触发时刻之前 |
| 故障后记录时长 | 40~5000ms | 1000ms | 触发时刻之后 |
| 总记录时长 | 80~6000ms | 1200ms | 前 + 后 |

#### 采样率与数据量

| 采样率 | 总样本 | 10通道数据量 | 年录波量(50次/天) |
|--------|--------|-------------|------------------|
| 1kHz   | 1200   | 96KB        | ~1.7GB/年        |
| 4kHz(默认) | 4800 | 384KB       | ~6.8GB/年        |
| 16kHz  | 19200  | 1.5MB       | ~27GB/年         |

### 3.4 波形数据存储

#### 3.4.1 存储架构

采用 **SQLite 元数据 + 二进制波形文件分离存储**：

```
/data/mupc/waveforms/
├── index/
│   └── fault_records.db         # SQLite 数据库（元数据）
└── recordings/
    ├── 2026/05/
    │   ├── 20260529_143022_001.wave
    │   └── ...
    └── ...
```

文件名格式：`YYYYMMDD_HHMMSS_seq.wave`

#### 3.4.2 SQLite 元数据扩展

在 Phase 3A 已有 `fault_records` 表基础上扩展字段：

```sql
ALTER TABLE fault_records ADD COLUMN waveform_path TEXT;
ALTER TABLE fault_records ADD COLUMN sample_rate INTEGER DEFAULT 0;
ALTER TABLE fault_records ADD COLUMN pre_trigger_ms INTEGER DEFAULT 0;
ALTER TABLE fault_records ADD COLUMN post_trigger_ms INTEGER DEFAULT 0;
ALTER TABLE fault_records ADD COLUMN channel_mask INTEGER DEFAULT 0;
ALTER TABLE fault_records ADD COLUMN waveform_size INTEGER DEFAULT 0;
ALTER TABLE fault_records ADD COLUMN has_waveform INTEGER DEFAULT 0;
ALTER TABLE fault_records ADD COLUMN trigger_offset INTEGER DEFAULT 0;
ALTER TABLE fault_records ADD COLUMN data_quality TEXT DEFAULT 'good';
ALTER TABLE fault_records ADD COLUMN time_quality TEXT DEFAULT 'synchronized';

CREATE INDEX IF NOT EXISTS idx_has_waveform ON fault_records(has_waveform);
CREATE INDEX IF NOT EXISTS idx_fault_type ON fault_records(fault_type);
```

#### 3.4.3 二进制波形文件格式 (.wave)

**文件头部 (Header) — 64 字节：**

| 偏移 | 长度 | 字段 | 类型 | 说明 |
|------|------|------|------|------|
| 0 | 4B | magic | u32 | 魔数 `WAVE` (0x57415645) |
| 4 | 2B | version | u16 | 文件格式版本号 (v1 = 0x0001) |
| 6 | 2B | channel_count | u16 | 录波通道数 (≤10) |
| 8 | 4B | channel_mask | u32 | 通道启用位掩码 |
| 12 | 4B | reserved1 | u32 | 保留 |
| 16 | 8B | sample_count | u64 | 每通道样本数 |
| 24 | 8B | sample_rate | u64 | 采样率 (Hz) |
| 32 | 8B | trigger_timestamp | i64 | 触发时刻 unix 时间戳 (ms) |
| 40 | 8B | trigger_offset | u64 | 触发点在样本序列中的偏移 |
| 48 | 4B | pre_trigger_nsamples | u32 | 故障前样本数 |
| 52 | 4B | post_trigger_nsamples | u32 | 故障后样本数 |
| 56 | 4B | event_id | u32 | 关联的 fault_records.id |
| 60 | 1B | data_quality | u8 | 0=good, 1=gap_detected, 2=major_gap |
| 61 | 1B | time_quality | u8 | 0=synchronized, 1=unsynchronized |
| 62 | 2B | reserved2 | u16 | 保留 |

**数据体：**

```
┌──────────────────────────────┐
│ Header (64 bytes)            │
├──────────────────────────────┤
│ Channel 0 samples (N × f64)  │  ← 通道连续存储
│ Channel 1 samples (N × f64)  │
│ ...                          │
│ Channel M-1 samples (N × f64)│
├──────────────────────────────┤
│ Timestamps (N × i64)         │  ← 微秒时间戳
├──────────────────────────────┤
│ Footer: CRC64 checksum (8B)  │  ← ECMA-182
└──────────────────────────────┘
```

**读/写接口：**

```rust
pub struct WaveformWriter {
    file: std::fs::File, path: PathBuf, checksum: crc64::Digest,
}
impl WaveformWriter {
    pub fn create(path: &Path, meta: &WaveformMetadata) -> Result<Self, WaveformError>;
    pub fn write_channel(&mut self, samples: &[f64]) -> Result<(), WaveformError>;
    pub fn write_timestamps(&mut self, timestamps: &[i64]) -> Result<(), WaveformError>;
    pub fn finalize(self) -> Result<WaveformFileInfo, WaveformError>;
}

pub struct WaveformReader { file: std::fs::File, metadata: WaveformMetadata }
impl WaveformReader {
    pub fn open(path: &Path) -> Result<Self, WaveformError>;
    pub fn read_all(&mut self) -> Result<(Vec<Vec<f64>>, Vec<i64>), WaveformError>;
    pub fn read_channel(&mut self, channel_index: usize) -> Result<Vec<f64>, WaveformError>;
    pub fn verify_checksum(&mut self) -> Result<bool, WaveformError>;
}
```

#### 3.4.4 存储容量管理

```rust
pub struct StorageManager {
    root_path: PathBuf,
    total_capacity: u64,           // 默认 2GB
    retention_days: u32,           // 默认 30 天
    free_space_threshold: u64,     // 默认 500MB
    daily_limit: u32,              // 默认 1000 次/天
    today_count: AtomicU32,
    last_reset_date: AtomicU64,
}

impl StorageManager {
    pub fn can_record(&self) -> Result<bool, WaveformError>;
    pub fn on_record_completed(&self, file_size: u64);
    pub fn cleanup(&self) -> Result<CleanupReport, WaveformError>;
    pub fn stats(&self) -> StorageStats;
}
```

**容量管理策略：**

| 策略参数 | 默认值 | 说明 |
|----------|--------|------|
| 单次录波存储上限 | 10 MB | 超过时自动等比截断 |
| 总存储空间上限 | 2 GB | 循环覆盖 |
| 保留期限 | 30 天 | 超期自动删除 |
| 空余空间阈值 | 500 MB | 低于时紧急清理 |
| 单日录波次数上限 | 1000 次 | 超限停止录波 |

**清理执行顺序：**
1. 删除超过 30 天保留期限的波形文件
2. 若仍超出 2GB 上限，继续删除最旧文件直到低于 80% 水位线
3. 若磁盘空余空间 < 500MB，触发紧急清理
4. 删除文件后，对应 SQLite 记录的 `has_waveform` 置 0（事件元数据保留）

**单次录波大小上限控制：**

当配置导致单次录波超过 10MB 时，等比缩小录波时长，保证至少 40ms 前后。

### 3.5 COMTRADE / CSV 导出

#### 3.5.1 COMTRADE 导出

COMTRADE 导出为非实时操作（按需生成），使用异步任务执行。

```rust
pub struct ComtradeExporter {
    waveforms_dir: PathBuf,
    export_dir: PathBuf,
    device_id: String,
}

impl ComtradeExporter {
    pub fn export_comtrade(&self, event_id: i64) -> Result<(PathBuf, PathBuf, PathBuf), ExportError>;
    pub fn export_csv(&self, event_id: i64) -> Result<PathBuf, ExportError>;
}
```

**导出格式：**
- COMTRADE：IEEE Std C37.111-1999，生成 .cfg + .dat + .hdr 三个文件
- CSV：UTF-8 with BOM，首行为通道名称，每行一个采样点，Timestamp_ms 为相对触发点偏移

**转换系数：**

| 通道 | 范围 | a (系数) | b (偏移) |
|------|------|---------|----------|
| 电压 | 0~500 V | 500/65536 ≈ 0.007629 | 0 |
| 电流 | 0~2000 A | 2000/65536 ≈ 0.030518 | 0 |
| 零序电压 | 0~100 V | 100/65536 ≈ 0.001526 | 0 |
| 零序电流 | 0~200 A | 200/65536 ≈ 0.003052 | 0 |
| 功率 | 0~5000 kW | 5000/65536 ≈ 0.076294 | 0 |

**导出目录布局：**

```
/data/mupc/waveforms/exports/
├── comtrade/20260529_143022_001/
│   ├── 20260529_143022_001.cfg
│   ├── 20260529_143022_001.dat
│   └── 20260529_143022_001.hdr
└── csv/
    └── 20260529_143022_001.csv
```

### 3.6 上报通道设计

#### 3.6.1 WaveformReporter trait

```rust
#[async_trait]
pub trait WaveformReporter: Send + Sync {
    async fn report_event(&self, event: &FaultEventWithWaveform,
                          summary: &WaveformSummary) -> Result<(), ReportError>;
    async fn report_file(&self, event_id: i64,
                         file_path: &Path) -> Result<(), ReportError>;
    async fn summon_file(&self, event_id: i64,
                         requester: &str) -> Result<Vec<u8>, ReportError>;
}

pub enum ReportError {
    NetworkError(String), RetryExhausted(String),
    FileNotFound(String), ProtocolError(String),
}
```

#### 3.6.2 IEC 104 上报

| 信息对象 | 类型标识 | 内容 |
|----------|---------|------|
| 故障录波事件 | TI=130 (FaultEventReport) | 事件ID、故障类型、触发值、时标 |
| 故障概要统计 | TI=131 (FaultSummaryReport) | 波形概要统计值 |
| 文件传输 | TI=122 (FileTransfer) | COMTRADE 波形文件 |

**IEC 104 文件传输流程：**

```
主站 → MUPC: 文件召唤请求 (C_FILE_CALL, TI=122)
MUPC → 主站: 文件传输开始 (F_FILE_READY)
MUPC → 主站: 文件段传输 (F_FILE_SEGMENT, ≤240字节/段)
MUPC → 主站: 文件传输结束 (F_FILE_FINISH, 校验和)
```

#### 3.6.3 MQTT 上报

| Topic | 内容 |
|-------|------|
| `mupc/north/fault/event` | 故障事件告警（JSON，QOS 1） |
| `mupc/north/fault/file` | 文件分块传输（Base64 编码，每块 2048 字节） |
| `mupc/north/fault/summon` | 主站召唤请求 |

**MQTT 事件消息体：**

```json
{
  "event_id": 12345,
  "fault_type": "OVER_VOLTAGE",
  "trigger_time": "2026-05-29T14:30:22.000Z",
  "trigger_value": 425.0,
  "sample_rate": 4000,
  "duration_ms": 1200,
  "channel_count": 10,
  "summary": {
    "pre_trigger": {"Ua": {"max": 311.5, "min": 308.2, "rms": 310.0}},
    "post_trigger": {"Ua": {"max": 425.8, "min": 200.1, "rms": 380.5}}
  }
}
```

**幂等性设计：** 每个 event_id 上报后记录到 `already_reported` 集合，避免重复上报。

**重试机制：** 上报失败重试 3 次，间隔 30 秒。

#### 3.6.4 MQTT 文件分块传输

```json
{
  "event_id": 12345,
  "file_name": "20260529_143022_001.cfg",
  "total_chunks": 5,
  "chunk_index": 0,
  "data": "<base64 chunk>",
  "checksum_sha256": "a1b2c3d4..."
}
```

### 3.7 波形数据查询与回放

#### 3.7.1 查询维度

| 查询维度 | 查询参数 | 说明 |
|----------|----------|------|
| 时间范围 | start_time, end_time | 按触发时间范围查询 |
| 故障类型 | fault_type | 过滤指定类型 |
| 故障ID | event_id | 精确查询 |
| 分页 | page, page_size | 默认 20，最大 100 |
| 波形存在 | has_waveform | 过滤有无波形文件 |

#### 3.7.2 查询接口

```rust
pub async fn query_events(filter: &FaultEventFilter) -> Result<PaginatedEvents, FaultRecorderError>;
pub async fn get_waveform(event_id: i64) -> Result<WaveformData, FaultRecorderError>;
pub async fn get_waveform_summary(event_id: i64) -> Result<WaveformSummary, FaultRecorderError>;
```

#### 数据结构

```rust
pub struct WaveformSummary {
    pub event_id: i64,
    pub pre_trigger_stats: Vec<ChannelStats>,
    pub post_trigger_stats: Vec<ChannelStats>,
    pub trigger_point: TriggerInfo,
}

pub struct ChannelStats {
    pub channel_name: String,
    pub max: f64, pub min: f64, pub avg: f64, pub rms: f64,
    pub thd: Option<f64>,  // 谐波畸变率（电压通道特有）
}
```

### 3.8 错误处理与边界条件

#### 3.8.1 采样数据丢失

| 场景 | 处理方式 |
|------|----------|
| 单个采样点丢失 | 用 `NaN` 填充，标记 `data_quality=gap_detected` |
| 连续 10+ 采样点丢失 | 中断当前录波，标记 `data_quality=major_gap` |
| 所有通道同时丢失 | 可能为 intercore 通信中断，停止录波，发起重连 |
| 单个通道丢失 | 其他通道继续录波，缺失通道用 `NaN` 填充 |

#### 3.8.2 多故障并发

| 场景 | 处理方式 |
|------|----------|
| 录波中发生第二个故障 | 不中断当前录波，在第二个缓冲区启动第二次录波 |
| 两个缓冲区皆满时第三故障 | 丢弃已保存完成的最旧录波，释放缓冲区 |
| 同一秒内同类型多次触发 | 合并为一次故障事件 |
| 不同类型故障 100ms 内先后发生 | 作为独立事件分别录波 |

#### 3.8.3 存储异常

| 磁盘使用率 | 行为 |
|------------|------|
| < 85% | 正常 |
| >= 85% | 记录 WARN 日志 |
| >= 90% | 触发 minor 告警 |
| >= 95% | 触发 critical 告警，紧急清理，停止新录波 |
| >= 98% | 停止所有数据写入，仅维持只读查询 |

**文件写入失败重试：** 重试 3 次，间隔 30 秒，失败后丢弃数据并记录日志。

#### 3.8.4 配置异常

| 场景 | 处理方式 |
|------|----------|
| 故障前时长 > 故障后时长 × 3 | 拒绝配置，返回错误 |
| 采样率不是可用档位 | 自动四舍五入到最近档位，记录日志 |
| 所有通道均禁用 | 拒绝配置，要求至少启用一个通道组 |
| 总记录时长 < 80ms | 拒绝配置 |

### 3.9 性能设计

#### 3.9.1 录波启动延迟 ≤ 1 采样周期

```
采样帧到达
    ├── 写入环形缓冲区 (当前帧)        ← T0
    ├── 触发判定 (纯内存)              ← T0 + 1μs
    ├── 触发成立: 冻结缓冲区 + 记录位置 ← T0 + 3μs
    ├── 唤醒后台录波任务               ← T0 + 5μs
    └── 总延迟 ≈ 5μs << 250μs (4kHz 的 1 采样周期)
```

关键措施：
- 触发判定在采样接收函数内**同步执行**，不经过 tokio 队列
- 环形缓冲区写入和触发判定使用 `parking_lot::RwLock`，写锁持有时间仅约 1μs
- 波形文件写入在独立 tokio task 中执行，不阻塞采样流程

#### 3.9.2 CPU 占用控制（峰值 ≤ 15%）

| 操作 | 预估耗时 | CPU 占用 |
|------|---------|----------|
| 环形缓冲区写入 | 1μs | 0.4% (4000次/s) |
| 触发判定 | 2μs | 0.8% |
| 波形文件写入(峰值) | 30ms | 3% |
| 总计(稳态) | - | < 2% |
| 总计(录波峰值) | - | < 15% |

#### 3.9.3 内存占用

| 组件 | 稳态 | 峰值 |
|------|------|------|
| 环形缓冲区 A+B | 704KB | 704KB |
| 录波工作缓冲区 | - | 384KB |
| 总计 | ~706KB | ~1.15MB |
| 上限 | 2MB | 10MB |

---

## 4. 历史数据存储设计

### 4.1 技术选型

#### 存储引擎对比

| 维度 | SQLite (WAL) | RocksDB | sled |
|------|-------------|---------|------|
| 交叉编译(RK3588) | 易 (bundled) | 难 (ARM SF) | 易 |
| ACID事务 | 完整支持 | 列族级 | 单键级 |
| SQL查询能力 | 完整SQL | 无SQL | 无SQL |
| 写吞吐(1000条/秒) | 满足 | 优秀 | 良好 |
| 内存占用 | ~2MB | ~10MB+ | ~5MB |
| 现有依赖 | 已使用 | 未使用 | 未使用 |

#### 决策：双引擎混合策略

| 数据类型 | 选型 | 理由 |
|----------|------|------|
| 设备台账、铭牌、维护记录 | SQLite | 关系模型、事务完整性、已有依赖 |
| 告警日志、事件记录 | SQLite | 多条件组合查询、事务完整性 |
| 遥测历史、电池数据 | SQLite (按月分区) | WAL + 批量写入满足吞吐；分区简化清理 |
| 故障录波波形数据 | 文件系统 | 大文件二进制存储，沿用现有方案 |

**不选用 RocksDB/sled 的理由：**
- RK3588 交叉编译 RocksDB 的 C++ 依赖链复杂
- SQLite 按月分区 + 复合索引在 1000 台设备/百万条记录规模下已可满足 3 秒查询限时
- 多引擎增加部署、监控、备份复杂度

#### SQLite 配置

```
PRAGMA journal_mode=WAL;         -- 读写不互斥
PRAGMA synchronous=NORMAL;       -- 平衡安全性与写入性能
PRAGMA busy_timeout=5000;        -- 等待 5 秒后返回忙错误
PRAGMA cache_size=-64000;        -- 64MB 页缓存
PRAGMA temp_store=MEMORY;        -- 临时表在内存
```

### 4.2 mupc-storage crate 架构

#### 4.2.1 新建 crate 理由

1. storage 是一个独立的内聚模块，有自己的清晰职责边界
2. 被多个上层模块依赖(data-processing, web-api, gateway)，放在 data-processing 中会导致循环依赖
3. 独立的 crate 便于单测、维护、后续替换存储引擎

#### 4.2.2 模块划分

```
mupc/crates/storage/
├── Cargo.toml
├── src/
│   ├── lib.rs              # 公开接口导出：StorageService
│   ├── config.rs           # 存储配置
│   ├── error.rs            # StorageError
│   ├── db/                 # 数据库层
│   │   ├── mod.rs          # 初始化、连接池管理、WAL配置
│   │   ├── pool.rs         # 读写连接池实现
│   │   └── migration.rs    # 数据库迁移
│   ├── models/             # 数据模型
│   │   ├── device_asset.rs, nameplate.rs, nameplate_change.rs
│   │   ├── maintenance.rs, telemetry.rs, battery.rs
│   │   ├── alarm.rs, event.rs, storage_status.rs
│   ├── repository/         # Repository 模式 DAO
│   │   ├── device_repo.rs, nameplate_repo.rs, maintenance_repo.rs
│   │   ├── telemetry_repo.rs, battery_repo.rs
│   │   ├── alarm_repo.rs, event_repo.rs
│   ├── service/            # 业务逻辑层
│   │   ├── asset_service.rs, telemetry_service.rs
│   │   ├── alarm_service.rs, event_service.rs
│   │   ├── lifecycle_service.rs, export_service.rs
│   ├── writer.rs           # WriteBuffer 异步批量写入器
│   ├── cleanup.rs          # 数据清理任务
│   ├── export.rs           # CSV 导出实现
│   └── observer.rs         # 存储状态观测与告警
```

#### 4.2.3 Cargo.toml 依赖

```toml
[dependencies]
tokio.workspace = true; tracing.workspace = true
serde.workspace = true; serde_json.workspace = true
chrono.workspace = true; uuid.workspace = true
thiserror.workspace = true; async-trait.workspace = true
rusqlite = { version = "0.32", features = ["bundled", "vtab"] }
csv = "1.3"; parking_lot.workspace = true
mupc-common = { path = "../common", optional = true }
```

#### 4.2.4 核心接口

```rust
pub struct StorageService {
    writer: Arc<WriteBuffer>,
    pool: DbPool,
}

impl StorageService {
    pub async fn init(config: StorageConfig) -> Result<Self, StorageError>;
    pub fn asset_service(&self) -> AssetService<'_>;
    pub fn telemetry_service(&self) -> TelemetryService<'_>;
    pub fn alarm_service(&self) -> AlarmService<'_>;
    pub fn event_service(&self) -> EventService<'_>;
    pub fn lifecycle_service(&self) -> LifecycleService<'_>;
    pub fn export_service(&self) -> ExportService<'_>;
    pub fn storage_status(&self) -> Result<StorageStatus, StorageError>;
    pub async fn shutdown(self);
}
```

### 4.3 数据模型与表结构

> **实现范围说明：** 注意：以下表结构为 Target 设计，当前实际落地范围（已建表/分区状态）见 docs/technical-debt.md。当前实际 SQLite schema 仅 6 张表：`telemetry`（扁平结构，非按月分区）、`faults`、`decisions`、`events`、`assets`（简化字段）、`action_space_config`。`device_nameplate`、`maintenance_record`、`alarm_log`（告警管理）、`battery_YYYYmm`（电池分区表）等均未建表，`telemetry_YYYYmm` 按月分区未实现。

#### 4.3.1 实体关系图

```
DEVICE_ASSET (device_id PK) ──1:1── DEVICE_NAMEPLATE (device_id PK)
    │ 1:N ── MAINTENANCE_RECORD (device_id FK)
    │ 1:N ── NAMEPLATE_CHANGE_LOG (device_id FK)

TELEMETRY_{YYYYmm} (按月分区)     ALARM_LOG
BATTERY_{YYYYmm} (按月分区)       EVENT_LOG
```

#### 4.3.2 设备资产表

```sql
CREATE TABLE device_asset (
    device_id           TEXT PRIMARY KEY,
    device_type         TEXT NOT NULL,   -- ttu/inverter/charger/flexible_load/...
    asset_number        TEXT,
    manufacturer        TEXT NOT NULL,
    model               TEXT NOT NULL,
    serial_number       TEXT NOT NULL,
    firmware_version    TEXT,
    hardware_version    TEXT,
    device_alias        TEXT,
    description         TEXT,
    commissioning_date  TEXT NOT NULL,   -- ISO 8601
    last_maintenance_date TEXT,
    decommissioning_date TEXT,
    warranty_expiry_date TEXT,
    status              TEXT NOT NULL DEFAULT 'active',
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    deleted_at          TEXT             -- 软删除
);
```

#### 4.3.3 铭牌参数表

```sql
CREATE TABLE device_nameplate (
    device_id           TEXT PRIMARY KEY REFERENCES device_asset(device_id),
    rated_power         REAL,            -- 额定有功功率 (kW)
    rated_capacity      REAL,            -- 额定容量 (kWh)
    rated_voltage       REAL,            -- 额定电压 (V)
    rated_current       REAL,            -- 额定电流 (A)
    max_charge_power    REAL,            -- 最大充电功率 (kW)
    max_discharge_power REAL,            -- 最大放电功率 (kW)
    charge_efficiency   REAL,            -- 充电效率 (%)
    discharge_efficiency REAL,           -- 放电效率 (%)
    soc_min             REAL,            -- SOC 下限 (%)
    soc_max             REAL,            -- SOC 上限 (%)
    rated_reactive_power REAL,           -- 额定无功功率 (kVar)
    protection_level    TEXT,            -- 防护等级
    cooling_method      TEXT,            -- 冷却方式
    updated_at          TEXT NOT NULL
);
```

#### 4.3.4 维护记录表

```sql
CREATE TABLE maintenance_record (
    record_id           INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id           TEXT NOT NULL REFERENCES device_asset(device_id),
    maintenance_date    TEXT NOT NULL,
    maintenance_type    TEXT NOT NULL,    -- routine_inspection/fault_repair/firmware_upgrade/...
    description         TEXT NOT NULL,
    operator            TEXT NOT NULL,
    result              TEXT NOT NULL,    -- success/failed/partial
    next_maintenance_date TEXT,
    created_at          TEXT NOT NULL
);
```

#### 4.3.5 遥测历史表（按月分区）

```sql
-- 自动创建: telemetry_202605, telemetry_202606 ...
CREATE TABLE telemetry_202605 (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id           TEXT NOT NULL,
    timestamp           INTEGER NOT NULL,    -- 毫秒时间戳
    phase_a_voltage     REAL, phase_b_voltage REAL, phase_c_voltage REAL,
    phase_a_current     REAL, phase_b_current REAL, phase_c_current REAL,
    total_active_power  REAL, total_reactive_power REAL, total_apparent_power REAL,
    power_factor        REAL, frequency REAL,
    phase_a_power       REAL, phase_b_power REAL, phase_c_power REAL,
    total_import_energy REAL, total_export_energy REAL,
    quality             TEXT NOT NULL DEFAULT 'good'
);
CREATE INDEX idx_telemetry_202605_dev_time ON telemetry_202605(device_id, timestamp DESC);
```

#### 4.3.6 电池历史表（按月分区）

```sql
CREATE TABLE battery_202605 (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id           TEXT NOT NULL,
    timestamp           INTEGER NOT NULL,
    soc REAL, soh REAL,
    battery_temperature REAL, ambient_temperature REAL,
    dc_voltage REAL, dc_current REAL,
    charge_power REAL, discharge_power REAL,
    charge_status TEXT,   -- charging/discharging/idle/fault
    cycle_count INTEGER,
    cell_min_voltage REAL, cell_max_voltage REAL,
    cell_min_temperature REAL, cell_max_temperature REAL
);
```

#### 4.3.7 告警日志表

```sql
CREATE TABLE alarm_log (
    alarm_id            INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id           TEXT NOT NULL,
    alarm_type          TEXT NOT NULL,    -- over_voltage/under_voltage/over_current/...
    severity            TEXT NOT NULL,    -- critical/major/minor/warning
    description         TEXT NOT NULL,
    trigger_time        INTEGER NOT NULL, -- 毫秒时间戳
    acknowledge_time    INTEGER,          -- 确认时间 (nullable)
    acknowledge_by      TEXT,             -- 确认人 (nullable)
    clear_time          INTEGER,          -- 清除时间 (nullable)
    clear_by            TEXT,             -- 清除人 (nullable)
    status              TEXT NOT NULL DEFAULT 'active'
    -- active / acknowledged / cleared
);
```

#### 4.3.8 事件记录表

```sql
CREATE TABLE event_log (
    event_id            INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type          TEXT NOT NULL,    -- device_operation/control_command/system_event/...
    event_time          INTEGER NOT NULL,
    source              TEXT NOT NULL,    -- web_ui/iec104/mqtt/local/system
    operator            TEXT,
    description         TEXT NOT NULL,
    detail              TEXT NOT NULL DEFAULT '{}'  -- JSON
);
```

#### 4.3.9 存储配置表

```sql
CREATE TABLE storage_config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

### 4.4 写入架构

#### 4.4.1 WriteBuffer 设计

```
data-processing / plugins
    │  async channel (tokio::sync::mpsc)
    ▼
┌───────────────────┐
│  WriteBuffer      │  ← 内存缓冲 + 批量合并
│  每1000条或5000ms │  ← 先到先执行
│  触发批量提交     │
└────────┬──────────┘
         │
         ▼
┌───────────────────┐
│  Writer Connection │  ← 独立SQLite连接 (WAL, 单线程序列化)
└────────┬──────────┘
         │
         ▼
┌───────────────────┐          ┌───────────────────┐
│   SQLite (WAL)    │          │ Reader Connection  │
└───────────────────┘          │ Pool (最多4连接)   │
                               └───────────────────┘
```

#### 4.4.2 批量事务策略

```rust
impl WriteBuffer {
    pub async fn write_telemetry(&self, data: TelemetryRecord) -> Result<(), StorageError>;
    pub async fn write_battery(&self, data: BatteryRecord) -> Result<(), StorageError>;
    pub async fn write_alarm(&self, data: AlarmRecord) -> Result<(), StorageError>;
    pub async fn write_event(&self, data: EventRecord) -> Result<(), StorageError>;
}
```

- 异步通道缓冲，**容量满 1000 条或间隔满 5000 ms 触发批量事务提交（先到先执行）**
- 写入协程与查询协程通过不同 SQLite 连接实例隔离（读写分离）
- 写入协程使用单个连接，串行化写入
- 查询协程使用独立连接池（最多 4 个连接）

> **口径对齐（2026-09-23，PM 裁定「保持实现现状」）**：本处原写「100ms 或积累 100 条」，与实现
> 现值不一致 —— 实现为 **容量 1000 条 / 间隔 5000 ms**（装配处 `mupc/crates/mupc-core-bin/src/startup.rs`
> 的 `WriteBuffer::new(1000, 5000, …)`；参数即 `mupc/crates/storage/src/services.rs` 的
> `capacity` 与 `flush_interval_ms` 两个字段，容量触发与定时触发两条路径互为"先到先执行"）。
> **取此量级的理由**：本参数决定缓冲区**提交节拍**；取得过小（100 ms / 100 条）会把每次采集的少量点
> 各包成一次独立事务，产生**高频小事务与写入放大**，而目标存储介质是 **SQLite WAL + SD 卡**
> ⇒ 直接消耗写入寿命与吞吐。故按"容量为主、时间为兜底"取粗节拍。**取值可调，但本轮不引入配置项。**
>
> ⚠️ **概念消歧：「flush 窗口」≠「存储周期」**（2026-09-23 评审指出，两个口径此前易被混为一谈）：
> - **flush 窗口**（即本节的"容量 OR 时间，先到先执行"）＝ 缓冲区**提交节拍**：只管"内存里已采集的
>   点**最迟多久 / 攒到多少条**落进 SQLite"，影响的是落库延迟与事务粒度；
> - **存储周期**（`03 PRD:693`「按可配置的存储周期（默认 1 分钟）将…电气量数据持久化存储」）
>   ＝ **采样/落库聚合周期**：指"电气量历史数据以多长的周期聚合为一条记录"，是**另一个量**。
>   二者不一一对应、也不可相互推导（调 flush 窗口不会改变记录的时间粒度）。
> - **当前差异（另行登记）**：「存储周期**可配置**」**当前未实现** —— `core_config.rs` **无 storage 段**
>   任何字段（本节的 1000 与 5000 均为装配期硬编码常量）。该差异**不在本文档处置**，已登记于
>   技术债（D2）；本处**只做概念消歧，不新增配置项、不改 PRD**。
>
> ✅ **本注已被 §9 取代（2026-09-23）**：03 PRD §11 增量把 1000 / 5000 提升为 `core_config.storage`
> 配置项，并新增第三个键 `grid_aggregate_period_ms`（即上文所指的"**存储周期**"）。本节的
> 「本轮不引入配置项」**自 §9 起不再成立**；两参数正交的口径与落点见 **§9.2.2 末**。

#### 4.4.3 数据分区策略

- 遥测和电池表按月分区：`telemetry_202605`, `telemetry_202606` ...
- 分区表自动创建，应用层通过 `YYYYmm` 格式拼接表名
- 数据清理时直接 `DROP TABLE` 整表删除，避免大量 DELETE 产生 WAL 膨胀

### 4.5 数据生命周期管理

#### 4.5.1 保留策略

| 数据类型 | 默认保留期限 | 配置范围 |
|----------|-------------|----------|
| 电气量历史数据 | 90 天 | 30~365 天 |
| 电池运行数据 | 90 天 | 30~365 天 |
| 告警日志 | 365 天 | 90~730 天 |
| 事件记录 | 730 天 | 365~1095 天 |
| 故障录波数据 | 365 天 | 90~730 天 |
| 设备台账 | 永久 | 不可配置 |

#### 4.5.2 自动数据清理

每日凌晨 2:00（可配置）执行：

```
1. 读取 storage_config 中的保留策略
2. 计算各数据类型截止时间戳
3. 遥测/电池分区表:  DROP TABLE 超出保留期的整表
4. 告警/事件表:       DELETE FROM WHERE trigger_time < cutoff
5. 故障录波:          删除波形文件 + 清除 SQLite 引用
6. PRAGMA wal_checkpoint(TRUNCATE) 截断 WAL
7. 记录清理日志
```

#### 4.5.3 磁盘空间紧急处理

| 使用率 | 行为 |
|--------|------|
| >= 85% | 记录 WARN 日志 |
| >= 90% | 触发 minor 告警 |
| >= 95% | 触发 critical 告警，紧急清理，停止时序写入 |
| >= 98% | 停止所有写入，仅维持只读查询 |

#### 4.5.4 降级模式

| 故障场景 | 降级行为 | 恢复条件 |
|---------|---------|---------|
| 数据库文件损坏 | 停止时序写入，资产只读，告警事件内存缓存 | 修复成功或重新初始化 |
| 磁盘 > 95% | 停止时序写入 | 清理后 < 90% |
| 磁盘 > 98% | 停止所有写入 | 清理后 < 95% |
| 单次写入超时(>5s) | 放弃本次写入，继续下一条 | 正常写入恢复 |
| 连续 10 次写入失败 | 触发告警，每 5 分钟重试 | 重试成功 |

#### 4.5.5 数据库启动自检

```
StorageService::init():
1. 打开 mupc.db
2. PRAGMA integrity_check → 校验数据库完整性
3. 失败: 尝试 quick_check → 仍失败 → 降级模式 + 告警
4. 通过: PRAGMA journal_mode=WAL → 运行迁移 → 初始化连接池
5. 启动后台任务: 写入协程、磁盘监控、数据清理调度
```

### 4.6 存储容量规划

| 数据类型 | 日增量 | 默认保留期 | 存储量 |
|----------|--------|-----------|--------|
| 遥测(100台) | 28.8 MB | 90 天 | 2.59 GB |
| 电池(100台) | 21.6 MB | 90 天 | 1.94 GB |
| 告警日志 | 25 KB | 365 天 | 9 MB |
| 事件记录 | 200 KB | 730 天 | 146 MB |
| 故障录波 | 50 MB | 365 天 | 18 GB |
| **合计** | **~100.6 MB** | - | **~22.7 GB** |

64GB eMMC 分区规划：系统 20GB + 数据 44GB。
数据分区分配：时序 5GB + 告警事件 200MB + 故障录波 18GB + 导出 5GB + WAL 512MB + 预留 15GB。

---

## 5. 设备台账管理设计

### 5.1 设备资产信息管理 (CRUD)

#### 资产信息字段

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| device_id | 字符串 | 是 | 设备唯一标识，全局不可重复 |
| device_type | 枚举 | 是 | ttu/inverter/charger/flexible_load/fire_alarm/battery/grid_connection/other |
| asset_number | 字符串 | 否 | 资产编号 |
| manufacturer | 字符串 | 是 | 厂商全称 |
| model | 字符串 | 是 | 设备型号 |
| serial_number | 字符串 | 是 | 出厂序列号 |
| firmware_version | 字符串 | 否 | 当前固件版本 |
| hardware_version | 字符串 | 否 | 硬件版本 |
| device_alias | 字符串 | 否 | 设备别名 |
| description | 文本 | 否 | 备注 |

**关键约束：**
- `device_id` 必须符合 `^[a-zA-Z0-9_-]{1,64}$` 格式，不可重复
- 删除为软删除（标记 `deleted_at`）
- 每次 CRUD 操作记录事件日志

### 5.2 铭牌参数管理

铭牌参数与设备资产 1:1 关联，修改时自动记录变更历史到 `nameplate_change_log`。

铭牌参数字段：rated_power, rated_capacity, rated_voltage, rated_current, max_charge_power, max_discharge_power, charge_efficiency, discharge_efficiency, soc_min, soc_max, rated_reactive_power, protection_level, cooling_method

### 5.3 维护记录管理

维护记录关联设备资产（N:1），支持按 device_id 查询并按日期倒序排列。

维护类型：routine_inspection, fault_repair, firmware_upgrade, part_replacement, calibration, other

### 5.4 台账北向上送

| 触发方式 | 说明 | 优先级 |
|----------|------|--------|
| 主动全量上报 | 台账变更后 30 秒内自动上送 | 高 |
| 定时全量上报 | 每日凌晨 3:00 定时上送 | 中 |
| 主站召唤上送 | 收到召唤指令后立即上送 | 高 |

重试策略：失败重试 3 次，间隔 10s / 30s / 60s。

### 5.5 插件自动注册

rs485-plugin / hplc-plugin 在启动时通过 `StorageService::asset_service().auto_register()` 自动注册设备：

- 插件初始化时自动填写基础信息（device_id, device_type, manufacturer, model）
- 已存在的 device_id 更新字段，不覆盖已有信息
- 不存在的 device_id 创建新记录

### 5.6 device-trait 类型扩展

需在 `device-trait/src/types.rs` 中将 `DeviceType` 扩展为：

```rust
pub enum DeviceType {
    Ttu, Inverter, Charger, FlexibleLoad,
    FireAlarm, Battery, GridConnection, Other,
}
```

---

## 6. 接口定义

### 6.1 REST API 总览

所有路由挂载在 `/api/v1` 前缀下。

| 路由 | 方法 | 功能 | 集成点 |
|------|------|------|--------|
| **设备台账** | | | |
| `/api/v1/devices` | GET | 查询设备列表(分页+过滤) | web-api → storage |
| `/api/v1/devices` | POST | 创建设备台账 | web-api → storage |
| `/api/v1/devices/{device_id}` | GET | 查询单个设备详情 | web-api → storage |
| `/api/v1/devices/{device_id}` | PUT | 更新设备信息 | web-api → storage |
| `/api/v1/devices/{device_id}` | DELETE | 软删除设备 | web-api → storage |
| `/api/v1/devices/{device_id}/nameplate` | GET | 查询铭牌参数 | web-api → storage |
| `/api/v1/devices/{device_id}/nameplate` | PUT | 更新铭牌参数 | web-api → storage |
| `/api/v1/devices/{device_id}/nameplate/changes` | GET | 查询铭牌变更历史 | web-api → storage |
| `/api/v1/devices/{device_id}/maintenance` | GET | 查询维护记录列表 | web-api → storage |
| `/api/v1/devices/{device_id}/maintenance` | POST | 创建维护记录 | web-api → storage |
| **历史数据** | | | |
| `/api/v1/telemetry` | GET | 查询遥测历史 | web-api → storage |
| `/api/v1/telemetry/latest` | GET | 查询所有设备最新遥测 | web-api → storage |
| `/api/v1/battery` | GET | 查询电池历史 | web-api → storage |
| `/api/v1/battery/trend` | GET | 查询SOC/SOH趋势(日/周/月) | web-api → storage |
| **告警日志** | | | |
| `/api/v1/alarms` | GET | 查询告警列表 | web-api → storage |
| `/api/v1/alarms/{alarm_id}` | GET | 查询单个告警详情 | web-api → storage |
| `/api/v1/alarms/{alarm_id}/acknowledge` | POST | 确认告警 | web-api → storage |
| `/api/v1/alarms/batch-acknowledge` | POST | 批量确认告警 | web-api → storage |
| `/api/v1/alarms/{alarm_id}/clear` | POST | 清除告警 | web-api → storage |
| **事件记录** | | | |
| `/api/v1/events` | GET | 查询事件列表 | web-api → storage |
| `/api/v1/events/{event_id}` | GET | 查询事件详情 | web-api → storage |
| **数据导出** | | | |
| `/api/v1/export/telemetry` | POST | 触发遥测导出 | web-api → storage |
| `/api/v1/export/battery` | POST | 触发电池导出 | web-api → storage |
| `/api/v1/export/alarms` | POST | 触发告警导出 | web-api → storage |
| `/api/v1/export/events` | POST | 触发事件导出 | web-api → storage |
| `/api/v1/export/download/{file_name}` | GET | 下载导出文件 | web-api → static |
| **存储管理** | | | |
| `/api/v1/storage/status` | GET | 查询存储状态 | web-api → storage |
| `/api/v1/storage/config` | GET | 查询保留策略配置 | web-api → storage |
| `/api/v1/storage/config` | PUT | 更新保留策略配置 | web-api → storage |
| `/api/v1/storage/cleanup` | POST | 手动触发数据清理 | web-api → storage |

### 6.2 核心 trait 定义

#### FaultRecorder trait（扩展后）

```rust
#[async_trait]
pub trait FaultRecorder: Send + Sync {
    // 已有方法
    async fn record(&self, event: &FaultCondition) -> Result<(), MupcError>;
    async fn query(&self, start: i64, end: i64) -> Result<Vec<FaultRecord>, MupcError>;
    async fn get_waveform(&self) -> Result<WaveformData, MupcError>;
    fn is_recording(&self) -> bool;

    // 新增方法
    async fn query_events(&self, filter: &FaultEventFilter) -> Result<PaginatedEvents, MupcError>;
    async fn get_waveform_by_id(&self, event_id: i64) -> Result<WaveformData, MupcError>;
    async fn get_waveform_summary(&self, event_id: i64) -> Result<WaveformSummary, MupcError>;
    async fn export_comtrade(&self, event_id: i64, output_dir: &Path) -> Result<ExportResult, MupcError>;
    async fn export_csv(&self, event_id: i64, output_dir: &Path) -> Result<ExportResult, MupcError>;
    async fn update_trigger_config(&self, config: &TriggerConfig) -> Result<(), MupcError>;
    async fn get_trigger_config(&self) -> Result<TriggerConfig, MupcError>;
}
```

#### WaveformReporter trait

```rust
#[async_trait]
pub trait WaveformReporter: Send + Sync {
    async fn report_event(&self, event: &FaultEventWithWaveform,
                          summary: &WaveformSummary) -> Result<(), ReportError>;
    async fn report_file(&self, event_id: i64, file_path: &Path) -> Result<(), ReportError>;
    async fn summon_file(&self, event_id: i64, requester: &str) -> Result<Vec<u8>, ReportError>;
}
```

#### StorageService（统一存储入口）

```rust
pub struct StorageService {
    writer: Arc<WriteBuffer>,
    pool: DbPool,
}

impl StorageService {
    pub async fn init(config: StorageConfig) -> Result<Self, StorageError>;
    pub fn asset_service(&self) -> AssetService<'_>;
    pub fn telemetry_service(&self) -> TelemetryService<'_>;
    pub fn alarm_service(&self) -> AlarmService<'_>;
    pub fn event_service(&self) -> EventService<'_>;
    pub fn lifecycle_service(&self) -> LifecycleService<'_>;
    pub fn export_service(&self) -> ExportService<'_>;
    pub fn storage_status(&self) -> Result<StorageStatus, StorageError>;
    pub async fn shutdown(self);
}
```

### 6.3 错误类型定义

#### DataProcessingError

```rust
#[derive(Error, Debug)]
pub enum DataProcessingError {
    #[error("数据采集失败: {0}")]     CollectionFailed(String),
    #[error("消息发送失败: {0}")]     MessageSendFailed(String),
    #[error("数据库错误: {0}")]       DatabaseError(String),
    #[error("配置错误: {0}")]         ConfigError(String),
    #[error("波形错误: {0}")]         WaveformError(String),
    #[error("触发配置错误: {0}")]     TriggerConfigError(String),
    #[error("导出错误: {0}")]         ExportError(String),
    #[error("存储空间不足: {0}")]     StorageFull(String),
    #[error("文件损坏: {0}")]         FileCorrupted(String),
}
```

#### StorageError

```rust
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("数据库错误: {0}")]           Database(String),
    #[error("设备不存在: {0}")]          DeviceNotFound(String),
    #[error("设备ID重复: {0}")]          DuplicateDeviceId(String),
    #[error("写入通道已关闭: {0}")]      ChannelClosed(String),
    #[error("数据清理失败: {0}")]        CleanupFailed(String),
    #[error("导出失败: {0}")]            ExportFailed(String),
    #[error("配置错误: {0}")]            ConfigError(String),
    #[error("IO错误: {0}")]              Io(#[from] std::io::Error),
    #[error("磁盘空间不足")]             DiskFull,
    #[error("数据库损坏, 进入降级模式")] DatabaseCorrupted,
}
```

---

## 7. 文件结构

### 7.1 data-processing crate

```
mupc/crates/data-processing/
├── Cargo.toml
├── src/
│   ├── lib.rs                         # 模块导出
│   ├── telemetry.rs                   # 遥测接口 trait + DataPackage
│   ├── recorder.rs                    # FaultRecorder trait
│   ├── collector.rs                   # DataCollectorImpl
│   ├── high_freq_telemetry.rs         # HighFreqTelemetryImpl
│   ├── reporter.rs                    # DataReporter
│   ├── fault_recorder_impl.rs         # FaultRecorderImpl（组合 waveform 子模块）
│   ├── database.rs                    # SQLite 初始化 + 操作
│   ├── errors.rs                      # DataProcessingError
│   ├── waveform_config.rs             # TriggerConfig 配置结构体
│   ├── waveform_reporter.rs           # WaveformReporter 适配器
│   └── waveform/                      # 故障录波子模块（新增）
│       ├── mod.rs                     # 模块导出
│       ├── ring_buffer.rs             # 环形缓冲区
│       ├── trigger.rs                 # 触发判定引擎
│       ├── sampling.rs                # 双缓冲区管理器
│       ├── storage.rs                 # 波形文件读写 + 存储容量管理
│       ├── export.rs                  # COMTRADE / CSV 导出
│       └── report.rs                  # 北向上报接口 trait
│           ├── ring_buffer_test.rs    # 单元测试（内联）
│           ├── trigger_test.rs
│           ├── sampling_test.rs
│           ├── storage_test.rs
│           ├── export_test.rs
│           └── report_test.rs
└── tests/
    └── data_processing_tests.rs       # 集成测试
```

### 7.2 mupc-storage crate（新建）

```
mupc/crates/storage/
├── Cargo.toml
├── src/
│   ├── lib.rs                         # 公开接口导出
│   ├── config.rs                      # 存储配置
│   ├── error.rs                       # StorageError
│   ├── db/
│   │   ├── mod.rs                     # 数据库初始化
│   │   ├── pool.rs                    # 读写连接池
│   │   └── migration.rs              # 数据库迁移
│   ├── models/
│   │   ├── mod.rs
│   │   ├── device_asset.rs
│   │   ├── nameplate.rs
│   │   ├── nameplate_change.rs
│   │   ├── maintenance.rs
│   │   ├── telemetry.rs
│   │   ├── battery.rs
│   │   ├── alarm.rs
│   │   ├── event.rs
│   │   └── storage_status.rs
│   ├── repository/
│   │   ├── mod.rs
│   │   ├── device_repo.rs
│   │   ├── nameplate_repo.rs
│   │   ├── maintenance_repo.rs
│   │   ├── telemetry_repo.rs
│   │   ├── battery_repo.rs
│   │   ├── alarm_repo.rs
│   │   └── event_repo.rs
│   ├── service/
│   │   ├── mod.rs
│   │   ├── asset_service.rs
│   │   ├── telemetry_service.rs
│   │   ├── alarm_service.rs
│   │   ├── event_service.rs
│   │   ├── lifecycle_service.rs
│   │   └── export_service.rs
│   ├── writer.rs                       # WriteBuffer 异步批量写入
│   ├── cleanup.rs                      # 数据清理任务
│   ├── export.rs                       # CSV 导出实现
│   └── observer.rs                     # 存储状态观测
```

### 7.3 扩展的既有文件清单

| 文件 | 改动内容 |
|------|----------|
| `device-trait/src/types.rs` | DeviceType 增加 Battery, GridConnection, Other 变体 |
| `intercore/src/protocol.rs` | 增加 FrameType::WaveformSample(0x0040) 及解析方法 |
| `gateway/src/iec104/protocol.rs` | 增加 TypeId::FaultEventReport(130) 等自定义 TI |
| `mqtt-bridge/src/topics.rs` | 增加 NORTH_FAULT_EVENT 等 Topic 定义 |
| `web-api/src/router.rs` | 挂载 StorageService 的路由 |

---

## 8. 技术决策记录

### 8.1 存储引擎选型：SQLite（全场景）

**决策：** 统一采用 SQLite（WAL 模式），不做多引擎混合。

**理由：**
- SQLite 已存在于 data-processing 的依赖中，零新增依赖成本
- WAL 模式（PRAGMA journal_mode=WAL）提供读写不互斥能力，并发性能提升 5-10 倍
- 按月分区（`telemetry_YYYYmm`）+ 复合索引（device_id, timestamp）在 1000 台设备/百万条记录规模下可满足 3 秒查询限时
- RK3588 交叉编译 RocksDB 的 C++ 依赖链复杂，sled/redb 仍需在应用层实现时间范围索引
- 单引擎降低部署、监控、备份复杂度

**SQLite 关键配置：**
- `journal_mode=WAL` — 读写不互斥
- `synchronous=NORMAL` — 平衡安全性与写入性能
- `busy_timeout=5000` — 等待 5 秒后返回忙错误
- `cache_size=-64000` — 64MB 页缓存
- `temp_store=MEMORY` — 临时表在内存

### 8.2 波形存储格式：自定义二进制 .wave

**决策：** 不使用 COMTRADE 作为存储格式，自定义二进制 .wave 格式，COMTRADE 仅按需导出。

**理由：**
- .wave 格式更紧凑（比 COMTRADE 少 50% 体积）
- 写入速度更快（二进制直写，无需格式化转换），适合嵌入式场景
- COMTRADE 生成的 CPU 开销大，不作为录波流程的一部分
- CRC64 校验和保证数据完整性

### 8.3 环形缓冲区实现：双缓冲 Vec 预分配

**决策：** 双缓冲 Vec 预分配，而非 VecDeque。

**理由：**
- 固定大小预分配 Vec 可达到 O(1) 写入且无内存分配
- 避免 VecDeque 的运行时开销
- 内存池预分配模式适合实时性要求高的嵌入式场景

### 8.4 双缓冲与并发控制：parking_lot::RwLock

**决策：** 使用 `parking_lot::RwLock` 保护缓冲区，而非 `std::sync::RwLock`。

**理由：**
- `parking_lot` 比 `std` 更轻量（不维护系统级条件变量）
- 读多写少场景下 RwLock 优于 Mutex
- 生产者（写入）持写锁仅 1μs，消费者（触发判定）持读锁
- 降低锁竞争，保证录波启动延迟 ≤ 1 采样周期

### 8.5 crate 拆分决策：新建 mupc-storage

**决策：** 将历史数据存储、设备台账管理提取为独立 crate `mupc-storage`，而非放在 `data-processing` 中。

**理由：**
- storage 是一个独立的内聚模块，有清晰职责边界
- 被多个上层模块依赖（data-processing, web-api, gateway），放在 data-processing 中会导致循环依赖
- 独立 crate 便于单测、维护、后续替换存储引擎
- 遵循现有架构模式（每个功能域一个 crate）

### 8.6 分区策略：按月分区 + DROP TABLE 清理

**决策：** 遥测和电池数据按月分区，清理时直接 `DROP TABLE`。

**理由：**
- 时序数据的自然清理粒度是时间范围
- `DROP TABLE` 比 `DELETE FROM` 快数个数量级，不产生 WAL 膨胀
- 分区前缀白名单验证（`^telemetry_\d{6}$`）保证 SQL 注入防护

### 8.7 故障录波模块归属：data-processing 内子模块

**决策：** 故障录波作为 `data-processing` crate 的 `waveform` 子模块，不新建独立 crate。

**理由：**
- 避免新建 crate 的跨 crate 复杂接口定义
- 直接复用 data-processing 的 SQLite 连接和数据通道
- 与 DataCollector、HighFrequencyTelemetry 共享同一数据源

### 8.8 未解决问题

| 问题 | 影响 | 决策方 |
|------|------|--------|
| 是否需要将故障录波数据迁移到 mupc-storage 统一管理？ | 数据一致性 vs 改动量 | 项目经理 |
| gateway 台账上送采用 IEC 104 哪类报文（设备参数 C_PL_NA_1 或自定义）？ | 与主站兼容性 | 架构师 + 主站对接 |
| 插件自动注册时，设备 device_id 的命名规范由谁定义？ | 设备标识一致性 | 南向通信团队 |
| 是否需要支持 TF 卡热插拔检测？ | 用户体验 vs 实现复杂度 | 项目经理 |

---

## 9. 增量设计：总表电气量聚合落库与存储参数可配置（U-69 / U-67 / U-68③ / U-70）

> **本章性质**：**实现级设计增量**，落 [03 PRD §11](../specs/modules/03-MUPC-数据处理与存储-PRD.md)（`[REVIEWED: PASS: 2026-09-23]`）。**不改需求**；与 PRD 不一致者集中登记于 §9.9，不得就地改 PRD。
> **与 §4 的关系**：§4.1.1 的「存储周期（默认 1 分钟）」自本章起**适用于台区总表电气量**并以**聚合**实现（§9.1）；§4.2/§4.3/§4.4 的粒度口径不变。冲突时以本章为准。
> **跨文档**：外设遥测「最新值快照 + 变更通知」入口的**唯一设计真源在 [01 设计 §9.1](01-MUPC-通信网关-设计文档.md)**，本章 §9.5 只声明引用与归属确认，**不复制结构**。
> **不做**：U-68 的另两项（API 形态、按月分区重算）不在本章；清理链路装配不在本章（§9.4）。
> **并发改动协同（v1.3 订正）**：`storage` 侧 U-68③ 的**最小加固已合并**（定时 flush 在 `services.rs:407-439`、失败回填/有界/计数在 `:459-503` + `:550-599`，见 `724225c`）——故本章 §9.3 **不再是"期望接缝"，而是"与已合并实现的对接说明 + 残留缺口清单"**。

### 9.0 现状基线与落点

**行号按 HEAD `724225c` 全量重校；同时重校"是否已修"（评审 P1：原稿按旧基线写，把已修项写成未修）**：

| # | 现状（**重校后**） | 证据（**已重校**） |
|---|------|------|
| 1 | `meter_grid` 不落库：`on_grid_package` 只 `set_latest_data` + 广播 IEC104 | `mupc-core-bin/src/startup.rs:526-533` |
| 2 | `WriteBuffer` 容量 1000 / 间隔 5000 为**装配期硬编码**，`core_config` **无 `storage` 段** | `startup.rs:795-799`；`core_config.rs`（无该段，`.storage` 字段不存在） |
| 3 | ✅ **已修（U-68③ 修复，`724225c`）**：提交失败**回填缓冲头部**（<span>不再整批丢</span>），缓冲**有界**（常驻上限 `DEFAULT_MAX_BUFFERED_POINTS = 10_000`），丢弃/回填**均有计数**（`dropped_points` / `dropped_batches` / `requeued_batches`）与 `error!` 日志 | `services.rs:459-503`（`flush_batch` 三层保护）、`:550-599`（`BatchGuard` + `Drop` 回填）、`:207`（常量）、`:308-320`（`trim_oldest`）、`:338-355`（`requeue_front`）、`:368-383`（三个计数器）、`:322-330`（`log_dropped`） |
| 4 | ⚠️ **仍未修**：`buffer_telemetry` 在**容量触发的路径上 `await` 提交**（`self.flush_batch(batch).await?`）⇒ 采集调用栈仍可能被 DB 阻塞 | `services.rs:273-305`，await 在 `:300-303` |
| 5 | `RetentionManager` **未装配**（全仓仅 `lib.rs` 导出 + 集成测试使用，生产装配无引用），§6.2 清理未生效 | `services.rs:602-639`；`storage/src/lib.rs:14`；`tests/integration.rs:816-821` |
| 6 | `quality` 字段在写入侧**恒为 0**（无枚举语义） | `startup.rs:565`（`quality: 0`） |
| 7 | `telemetry` 窄表列约束：`value REAL NOT NULL` / `quality INTEGER NOT NULL DEFAULT 0`（**决定"无采样行"怎么写**，见 §9.1.4） | `services.rs:650-661`（`CREATE TABLE IF NOT EXISTS telemetry`，`value` 在 `:655`） |

**落点**：`storage`（`GridAggregator` + `Quality` 枚举 + 缓冲失败语义加固）+ `core-bin`（装配、样本转发与 tick、`core_config.storage` 配置类型与 `validate_storage`），逐项见 §9.6。

---

### 9.1 总表电气量 1 分钟聚合落库（U-69）

#### 9.1.1 聚合点裁定

**裁定：在 `mupc-storage` 新增 `GridAggregator`（纯逻辑、无 IO），`core-bin` 的 `on_grid_package` 只做"转发样本 + 定时 tick"。**

| 备选 | 评估 | 结论 |
|------|------|------|
| A. `core-bin` 的 grid 接收闭包内实现 | core-bin 是装配层，**无可单测环境**（`initialize_all` 需 DB/intercore/串口全套真环境，见 `startup.rs:1447` 的既有说明）⇒ "周期边界 / 无采样产行 / 极值"这类时序逻辑**无法被单测钉住** | ❌ |
| **B. `storage` 新增 `GridAggregator`** | 可纯逻辑单测（喂 `(ts_ms, sample)` 序列 → 断言产出记录）；`storage` 在 CI 测试面内；装配层只剩"转发 + spawn tick" | ✅ **采用** |

**为什么不是 `data-processing`**：聚合的**输入**（`DataPackage` 的电气量语义）在 `mupc-southd::mapper`，但聚合的**输出形态**（落库记录：通道名/时间戳/quality）是**存储语义**（§4.1.1 / 附录 C 的 `quality`）。放 `storage` 与"落库记录形态的唯一所有者"一致；且 `storage` 已被 core-bin 依赖，**不新增依赖边**。

#### 9.1.2 接口（实现契约）

```rust
// mupc/crates/storage/src/grid_aggregate.rs（新增）

/// 落库通道规格（**表驱动**：增删通道 / 开关极值 = 改本表，不改算法）
pub struct ChannelSpec {
    /// 落库用通道名（= `telemetry.metric_name`）
    pub metric: &'static str,
    /// 是否产出分钟极值（max/min 各 1 行）
    pub extremes: bool,
    /// 取数闭包：从样本取该通道值（None = 本周期该通道缺测）
    pub pick: fn(&GridSample) -> Option<f64>,
    /// 表意注记（如"取 A 相"），写入设计对照表与日志，**不改数据**
    pub note: &'static str,
}

/// 一个采样点（core-bin 从 `DataPackage` 抽取后传入；**已换算工程值**）
#[derive(Debug, Clone, Copy, Default)]
pub struct GridSample {
    pub u: [Option<f64>; 3],
    pub i: [Option<f64>; 3],
    pub p: [Option<f64>; 3],
    pub q: [Option<f64>; 3],
    pub pf: [Option<f64>; 3],
    pub p_total: Option<f64>,
    pub q_total: Option<f64>,
}

/// 落库用的（窄表）记录：**与 `TelemetryPoint` 同构的"意图"形态**
pub struct AggregateRow {
    pub metric_name: &'static str,
    /// 聚合周期**起点**（UTC ms，`period_ms` 的整数倍）
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// 均值/极值；`None` = 本周期**无有效采样**（不可得）——**不得写 0**。
    /// 落库时经 `to_telemetry_point()` 原样传成 `Option<f64>`（`telemetry.value` 已按
    /// §9.1.4 改为**可空**）⇒ 库内为**真 NULL**，不是 0。**禁止**任何 `unwrap_or(0.0)`。
    pub value: Option<f64>,
    pub quality: Quality,     // Good | NoData（见 §9.1.4）
}

impl AggregateRow {
    /// **唯一落库转换点**（防"`None` 被某处 `unwrap_or(0.0)` 悄悄变成 0"）。
    /// `device_id = "grid_meter"`（站 id）、`metric_name`、`timestamp`、`quality` 逐字带过。
    pub fn to_telemetry_point(&self, device_id: &str) -> mupc_storage::TelemetryPoint;
}

pub struct GridAggregator { /* period_ms, specs: &'static [ChannelSpec], cur: Option<PeriodAcc>, last_start_ms: Option<u64> */ }

impl GridAggregator {
    pub fn new(period_ms: u64) -> Self;

    /// 喂入一个采样：若跨过周期边界，返回**已闭合周期**的全部行（0 或 1 个周期）。
    /// ⚠️ **只闭合、不回溯**：`ts_ms` 直接跳到 N 个周期之后时，中间周期**不补产**
    /// （防长断连后一次补出大量行；口径见 §9.1.5）
    pub fn observe(&mut self, ts_ms: u64, s: &GridSample) -> Vec<AggregateRow>;

    /// 时间推进（定时 tick）：闭合"已越过 `start + period` 但仍无新采样"的**当前**周期。
    /// **无采样周期照样产行**（PRD R-11.2-E），quality = NoData
    pub fn tick(&mut self, now_ms: u64) -> Vec<AggregateRow>;

    /// 退出前 flush：把当前未闭合周期闭合并产出（**进程优雅退出**）
    pub fn flush(&mut self, now_ms: u64) -> Vec<AggregateRow>;

    /// 本周期是否已有采样（供 tick 的幂等判断；测试用）
    pub fn current_start_ms(&self) -> Option<u64>;
}
```

#### 9.1.3 通道清单（落库集合）

**默认落库 18 通道 + 2 通道极值**（表驱动，见 §9.9 Q-2/Q-3 的裁定悬挂点）：

| 组 | 通道（`metric_name`） | 均值 | 极值 | 取数来源 | 表意注记 |
|----|----------------------|------|------|----------|----------|
| 电压 | `u_a` / `u_b` / `u_c` | ✅ | — | `phase.voltage[0..3]` | — |
| 电流 | `i_a` / `i_b` / `i_c` | ✅ | — | `phase.current[0..3]`（**带符号**，方向由分相有功符号承载 `mapper.rs:133-149`） | — |
| 分相有功 | `p_a` / `p_b` / `p_c` | ✅ | — | `phase.active_power[0..3]` | — |
| 分相无功 | `q_a` / `q_b` / `q_c` | ✅ | — | `phase.reactive_power[0..3]` | — |
| 分相功率因数 | `pf_a` / `pf_b` / `pf_c` | ✅ | — | `phase.cos_phi[0..3]` | — |
| 总有功 | `p_total` | ✅ | ✅ max/min | `electrical.active_power`（缺块时 mapper 已降级 Σp，`mapper.rs:131`） | — |
| 总无功 | `q_total` | ✅ | ✅ max/min | `electrical.reactive_power`（= Σq，`mapper.rs:163`） | — |
| 总功率因数 | `pf_total` | ✅ | — | `electrical.cos_phi` | ⚠️ **实为 A 相值**（`mapper.rs:164`）。若产品裁"语义不符不落"⇒ 从本表删 1 行（§9.9 Q-3） |
| 频率 | ✗ **不落** | — | — | `electrical.frequency` **恒 50.0 常量**（`mapper.rs:165`） | 常量入库会污染统计（PRD Q5 建议 (b)）⇒ **不落**，登记"无源" |
| 视在功率 S | ✗ 不落 | — | — | 无点表来源 | 避免为凑维度造数据（PRD Q4 口径同源） |
| 电能（进/出） | ✗ 不落 | — | — | 仪表侧无电能块（`meter_grid` 配置 `mupc/deploy/config/mupc_core_config.yaml:413-426`） | 同上 |

**行数**：每周期 = 18（均值） + 2×2（极值） = **22 行**（PRD §11.2-F 的 21+6=27 是**含频率/视在功率**时的上界；本设计按 Q5 建议排除频率后为 22，见 §9.9 C-1）。

**容量重算（回填 PRD §11.2-F 的口径）**：22 行/分钟 ⇒ 31,680 行/天；按 §7.3 的 200 B/行 ⇒ **6.3 MB/天**、90 天 ≈ **570 MB**、年 ≈ 2.3 GB。占数据分区（§7.3：44 GB）约 **1.3%** ⇒ 与 §6.1 的 90 天保留相容。

#### 9.1.4 聚合语义与时间戳

| 项 | 口径 |
|----|------|
| 聚合量 | **算术均值**（`Σx / n`，`n` = 本周期该通道**有效采样数**，非周期总采样数）；极值通道另出 `max` / `min` |
| **不得抽样** | 严禁"取周期内首个/末个瞬时值"（PRD R-11.2-A）。**反向用例**：把实现改为取首值 ⇒ 单测必红（GRD-02） |
| 时间戳 | 周期**起点**：`start = ts_ms - (ts_ms % period_ms)`，UTC 毫秒，**必为 `period_ms` 的整数倍**（PRD R-11.2-C / GRD-03） |
| 极值行的时间戳 | **与均值行同**（同周期起点）；区分靠 `metric_name`（`p_total_max` / `p_total_min`） |
| 极值行的 `metric_name` | `p_total_max` / `p_total_min` / `q_total_max` / `q_total_min` |
| 缺测 | 通道本周期**无有效采样** ⇒ `value = None`、`quality = NoData`；**严禁写 0**（PRD R-11.2-E / GRD-05） |
| 全周期无采样 | **仍产出全部 22 行**（含极值行的 `None`）——"断档可被查询识别，而非静默少行"（PRD R-11.2-E / GRD-04） |
| 部分缺测 | 该通道的均值按**有效采样**算；极值同理（仅用有效采样求） |
| 落库形态 | **复用 `telemetry` 窄表**（每通道 1 行；`INSERT` 在 `services.rs:506-530` 的 `commit_batch`）：`device_id = "grid_meter"`（站 id）、`metric_name` = 通道名、`timestamp` = 周期起点、`value`、`quality`。**不新建表、不加迁移** |
| **NoData 的落库映射（P2：原稿未定义 ⇒ `Option<f64>` 撞 `REAL NOT NULL`）** | **裁定：`telemetry.value` 改为可空，缺测行写 `NULL`（真 NULL），`quality = NoData`。** 机读形态：`value REAL`（去掉 `NOT NULL`）、`TelemetryPoint.value: Option<f64>`。**这是唯一与 PRD R-11.2-E「不得写入 0 冒充有效值」字面一致的做法**——排在前面而**被否决**的两个方案：① 写 `0.0` + `quality = NoData`（**仍写入了 0**，与禁令字面冲突，且任何忽略 quality 的 `AVG` 都会被污染）；② 不产行（**违反 GRD-04"断档可被查询识别，而非静默少行"**）。 |
| **迁移（本章唯一的结构变更，须写进实现）** | 在 `run_migrations`（`services.rs:647`）末尾新增一次**幂等**的"可空化重建"：先 `PRAGMA table_info(telemetry)` 检查 `value` 的 `notnull`，**为 0 则跳过**；否则 `BEGIN` → `ALTER TABLE telemetry RENAME TO telemetry_old` → 按**同一 DDL 但 `value REAL`** 重建（`services.rs:650-661`）→ `INSERT INTO telemetry SELECT id,device_id,timestamp,metric_name,value,quality FROM telemetry_old` → `DROP TABLE telemetry_old` → **重建既有索引** `ON telemetry(metric_name, timestamp)`（`:661`）→ `COMMIT`。**既有行全部为 `Some`** ⇒ 迁移前后**数据语义不变**（R-11.3-C / STG-01 的"零行为变化"仍成立）。 |
| **受影响的代码点（逐处列出，避免漏改）** | ① `storage/src/models.rs:10` `value: f64 → Option<f64>`；② `storage/src/services.rs:520` `.bind(point.value)`（sqlx 自动绑 `NULL`）；③ `storage/src/repository.rs:138` `.bind(point.value)`、`:153/:174` 的 `SELECT … value …` 与 `FromRow`（`telemetry` 行结构体 `:490` 起）改 `Option<f64>`；④ 其余 `TelemetryPoint { … }` 构造点（共 12 处：`data-processing/src/high_freq_telemetry.rs`、`mupc-core-bin/src/startup.rs`、`storage/tests/integration.rs` 等）一律写 `Some(v)`；⑤ `GridAggregator` 的缺测行写 `None`。**⑤ 之外全部是机械替换**（`value: x` → `value: Some(x)`） |
| **查询契约（防"NULL 行进统计"）** | NULL 天然被 SQL 聚合忽略（`AVG/SUM` 跳过 NULL）⇒ **无需额外过滤**即不会污染统计；但仍要求：① 对 `quality` 的非 Good 行做**存在性/计数**查询时显式按 `quality` 过滤；② 该口径写入 `models.rs` 的文档注释（§9.10 同步项）；③ GRD-04 的验收断言 = 注入断连 ⇒ 该周期 22 行存在、`quality = 1`、**`value IS NULL`**；对比用例：注入真实 0 值采样 ⇒ `quality = 0`、`value = 0.0`（**二者在库内可区分**） |
| `quality` 语义 | 现有 `TelemetryPoint.quality: i32`（写入侧恒 0，`startup.rs:565`）⇒ **本章新增枚举** `storage::Quality { Good = 0, NoData = 1, Invalid = 2, Stale = 3, Unconfigured = 4 }`。**`Good = 0` 保持既有写入值不变**（零行为变化）；枚举与 [01 设计 §9.1.2](01-MUPC-通信网关-设计文档.md) 的 `PointQuality` **一一映射**（同一语义、两处命名）。⚠️ **映射函数 `quality_from_point_quality(PointQuality) -> i32` 落在 `mupc-core-bin`（装配层），不落 `storage`** —— 理由见 §9.6 与 §9.8 D-8（放 `storage` 会**新增 `storage → data-processing` 依赖边**，现无该边） |
| 关闭开关 | **不提供**（PRD R-11.1-A / GRD-08）：`storage:` 段无任何字段可关闭本项 |

#### 9.1.5 无采样、断连与重启的行为（逐条明确）

| 情形 | 行为 |
|------|------|
| 单周期内无采样（站离线中） | `tick` 闭合该周期 ⇒ 22 行 `NoData`（**有行、可查**）。tick 由装配层 1 s 定时（周期 ≤ 60 s，粒度足够） |
| 站离线**长时间**（如 30 min） | 每周期 22 行 `NoData` ⇒ 660 行/30 min ⇒ **约 130 KB**，可接受。**不设补产上限**（PRD R-11.2-E 的"不得静默少行"优先；上限会制造不可区分的空洞） |
| **跨重启的空档** | **不回溯补产**（`GridAggregator` 的 `last_start_ms` 不落盘；重启后从当前周期开始）。⇒ 重启造成的空档表现为**时间戳跳变**（可查、可识别），而非 `NoData` 行。**如实声明**：这是本设计的**已知边界**（PRD 未规定跨重启，登记 §9.9 C-2） |
| 退出 | tick 任务作为 **U-64 的"协作生产者"** 入 `producers` 名单：收到 `stop_rx` ⇒ 先 `flush(now)` 入队 ⇒ 收工；随后由既有退出序列最后 flush 遥测缓冲（`startup.rs:804-806`，与 P0-1 同范式）。**不得**绕开该名单自行 `main.rs` 加 flush（会破坏 U-64 的顺序契约） |
| 采样跨多个周期（调度抖动/时钟跳变） | `observe` **只闭合"当前"周期**，中间周期由 `tick` 的 `NoData` 行覆盖（若 tick 先跑过）或跳过（若 tick 未跑）⇒ 由时间戳跳变可识别。**不做**"把样本按时间戳分摊到历史周期"（无定义、易造数） |

---

### 9.2 `storage:` 配置段（U-67）

#### 9.2.1 schema 与默认值

```yaml
# mupc_core_config.yaml 新增段（**不进 DB**，重启生效）
storage:
  batch_capacity: 1000              # 遥测写缓冲批量提交容量（条）
  flush_interval_ms: 5000           # 遥测写缓冲提交间隔（ms）
  grid_aggregate_period_ms: 60000   # 总表电气量聚合落库周期（ms）
```

```rust
// mupc/crates/mupc-core-bin/src/core_config.rs（新增）
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]                       // **整段缺省 ⇒ 取 Default（= 现实现常量）**
pub struct StorageSectionConfig {
    #[serde(default = "default_batch_capacity")]
    pub batch_capacity: u64,
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_grid_aggregate_period_ms")]
    pub grid_aggregate_period_ms: u64,
}
impl Default for StorageSectionConfig { /* 1000 / 5000 / 60000 —— **与现实现逐字相同** */ }
```

| 键 | 默认 | 合法范围 | 依据 |
|----|------|----------|------|
| `batch_capacity` | **1000** | 2 ~ 100000（**禁 1**，见下注） | = 现实现 `startup.rs:795-799`；下界收窄见 §9.2.2 注 |
| `flush_interval_ms` | **5000** | 100 ~ 600000（**禁 0**） | = 现实现 |
| `grid_aggregate_period_ms` | **60000** | 10000 ~ 3600000 **且 `% 1000 == 0`** | §4.1.1「默认 1 分钟」 |

**零行为变化（PRD R-11.3-C / STG-01）**：整段缺省 ⇒ 三值 = 上述默认 ⇒ 与变更前逐条一致。**本段不含保留期字段**（PRD R-11.3-A 的命名消歧）；**不含** `max_retained_points`（`storage:` 段**只含三键**，PRD R-11.3-A 明文）。

> ⚠️ **`batch_capacity` 下界收窄 1 → 2（2026-09-26，评审 W-1，P2）**：PRD R-11.3-E（03 PRD:1540）与本节初稿均写「1 ~ 100000（`≥ 1`）」。但 **`capacity = 1` 时每次 push 都落容量分支**（`since_attempt` 每 push 归零）⇒ **采集路径永不调用 `trim_oldest`**，`max_points` 的**有界性保证失效**（缓冲只由 flush 吞吐 / 失败回填裁剪约束，见 §9.3 缺口 3③）—— 这是**真缺陷**、非"退化配置"。故实现取**下界 2**，并把拒绝理由写进错误文案（§9.2.2 / `core_config::validate_storage`）。**与 PRD 字面的差异如实登记**：实现拒 1 ⇒ 接受域是 PRD 字面域（1 ~ 100000）的**真子集**；**PRD 一字未改**，**待产品确认**是否把该键订正为「2 ~ 100000（须 ≥ 2）」。默认 1000 不受影响，无行为变化。

> **缓冲上界口径的**统一**（P4：原设计写 `2 × batch_capacity`，实现是常量 `10_000`，二者未登记）**：
> **采用实现值 `10_000`**（`services.rs:207` 的 `DEFAULT_MAX_BUFFERED_POINTS`），**并说明依据** —— 代码注释（`services.rs:203-207`）给出两条：① 生产装配为 `capacity = 1000` ⇒ **10,000 = 10 个满批**，一次 `busy_timeout`（5 s 量级）的落库抖动不会触顶，而长故障时又不会无限增长；② 单点 ≈ 100 B ⇒ 满额 ≈ **1 MB**，对 RK3588 安全。
> **与 PRD R-11.5-A2 的差异登记（§9.9 Q-9 / C-3）**：PRD 写"默认 = **2 × batch_capacity**"，实现为**不随 `batch_capacity` 变的常量** —— 默认口径下二者差 5 倍（2,000 vs 10,000）。**本设计裁定：以 10,000 为准**，并**请求 PRD 订正该行为**"默认 **10,000 条**（= 10 × 默认 `batch_capacity`；由 `DEFAULT_MAX_BUFFERED_POINTS` 常量给出，**不随用户配置的 `batch_capacity` 缩放**）"。⇒ **FLS-02 的断言口径按 10,000**（不是 2 × capacity）。若产品要求"随 `batch_capacity` 缩放"⇒ 新增配置键 `storage.max_buffered_points`（PRD Q1 附带项），属**另立需求**。

#### 9.2.2 校验与非法值处置

**裁定：`拒启动`（fail-fast），错误信息点名违规键**（PRD R-11.3-E 建议 (a) / STG-04）。

```rust
// core_config.rs：与既有 validate_io / validate_display / validate_south_stations 同范式
impl CoreConfig {
    fn validate_storage(&self) -> Result<(), String> {
        let s = &self.storage;
        // 下界 2（不是 1）：capacity = 1 时每次 push 都触发容量分支 ⇒ 采集路径永不 trim_oldest
        // ⇒ max_points + 1 的上界论证失效（§9.2.1 注 / §9.3 缺口 3③ / PRD 差异已登记）
        if !(2..=100_000).contains(&s.batch_capacity) {
            return Err(format!(
                "storage.batch_capacity={} 须在 2..=100000（禁 1：每次 push 都触发容量分支，有界性失效）",
                s.batch_capacity
            ));
        }
        if !(100..=600_000).contains(&s.flush_interval_ms) {
            return Err(format!("storage.flush_interval_ms={} 须在 100..=600000（禁 0）", s.flush_interval_ms));
        }
        if !(10_000..=3_600_000).contains(&s.grid_aggregate_period_ms) {
            return Err(format!("storage.grid_aggregate_period_ms={} 须在 10000..=3600000", s.grid_aggregate_period_ms));
        }
        if s.grid_aggregate_period_ms % 1000 != 0 {
            return Err(format!("storage.grid_aggregate_period_ms={} 须为 1000 的整数倍（时间戳按整秒对齐）", s.grid_aggregate_period_ms));
        }
        Ok(())
    }
}
```

**衔接**：在 `CoreConfig::validate()`（`core_config.rs:464`）内、`self.validate_display()?` 之前**新增一行** `self.validate_storage()?;`（存储校验不依赖其它段，放在无跨段依赖的段落即可）。**该函数不读文件、不做跨段校验**（与 `validate_io` 的 `enabled` 门控不同——本段**无 enabled 开关**，PRD R-11.1-A）。

**两参数正交（PRD R-11.3-D / STG-05）**：`batch_capacity`/`flush_interval_ms` = **提交节拍**；`grid_aggregate_period_ms` = **记录时间粒度**。**两个量不可互推**，代码上分属两个类型（`WriteBuffer` / `GridAggregator`），互不引用 ⇒ 正交由结构保证（STG-05 的交叉验证即断言此结构）。

#### 9.2.3 装配点

| 项 | 落点 |
|----|------|
| `WriteBuffer::new` | `startup.rs:795-799` 的硬编码 `1000, 5000` → `cfg.storage.batch_capacity, cfg.storage.flush_interval_ms` |
| `GridAggregator::new` | 同处新增；实例随 `SouthSink` 注入（`Arc<Mutex<GridAggregator>>`，因 `observe` 需 `&mut`） |
| tick 任务（含退出 flush） | 新增 `spawn`：每 1000 ms 调 `tick(now_ms)` → 产出 → `WriteBuffer` 入队；**句柄入 `producers`（U-64 的"协作生产者"名单）**，收到 `stop_rx` 后先 `flush(now)` 入队再收工 —— **与既有 `flush_timer` 同范式、同顺序**（`startup.rs:804-806`；U-64 退出顺序 = 通知生产者收工 → 等它们确认 → 最后 flush 遥测缓冲） |

---

### 9.3 落库失败语义：**与已合并实现的对接说明**（U-68③）

> **本节性质（v1.3 改写，评审 P1/P3）**：U-68③ 的**最小加固已实现并合并**（`724225c`）——本节**不再是"期望接缝"**，而是「**已实现项（对齐 `724225c`）+ 残留缺口（含落地方案）**」。FLS-01～05 的**验收归属**见表末。

**已合并实现的接口面（`storage` 对外，逐条给出实现行号）**：

| S-# | 要求（PRD） | 实现状态 | 实现点（`724225c`） |
|-----|-------------|----------|---------------------|
| S-1 | 提交失败**回填**（保序）、下次触发重试 | ✅ **已实现** | `flush_batch`（`services.rs:470-503`）：失败 ⇒ `guard.requeue_now()`；`BatchGuard` + `Drop`（`:550-599`）兜住 abort/panic 路径；`requeue_front`（`:338-355`）头插保序 + 裁剪计数 |
| S-1' | 回填**不清空**已提交点（`abort`/`panic` 亦不丢） | ✅ **已实现**（超出原要求，登记为**增强**） | `BatchGuard::disarm`（`:570-573`）在 `tx.commit()` 后**无 await 点**处解除守卫 ⇒ 不存在"已提交未解除"窗口 |
| S-2 | 缓冲**有上界**、超限丢**最旧** | ✅ **已实现（上界口径 = 10,000，见下 P4）** | `DEFAULT_MAX_BUFFERED_POINTS = 10_000`（`:207`）；`trim_oldest`（`:308-320`）两条增长路径（新 push `:296-298` / 回填 `:349`）都过同一剪刀 |
| S-3 ① | 丢弃须 `error` 日志（含条数） | ✅ **已实现** | `log_dropped`（`:322-330`，含 `dropped/buffered/max_points/dropped_total`）；失败批日志 `:484-499` |
| S-3 ② | 丢弃须产**一条 `major` 级告警/事件** | ✅ **已实现（v1.3-r2；v1.3-r4 改为边沿触发）** | `storage` crate **仍无告警/事件通道**（依赖方向不允许，原判不变）⇒ 由 **core-bin 健康巡检**承接：判据 `mupc-core-bin/src/storage_health.rs:145-187`（`StorageHealthWatch::poll` 的**边沿状态机**：**进入丢弃态才出文案**，含增量条数 + 累计值）、告警三态 `HealthSignal` `:89-105`、循环 `:198-241`（周期 1 s，`emit` 只在进入臂被调）、生产接线 `:249-275`、装配点 `startup.rs:1545-1552`（句柄入 `producers` 协作退出名单，标签 `storage_health_timer`，级别常量 `:80` = `"major"`） |
| S-3 ③ | 丢弃计数**可观测** | ✅ **已实现**（**接口名与草案不同**，见下） | `dropped_points()`（`:368-371`）、`dropped_batches()`（`:374-377`）、`requeued_batches()`（`:380-383`）、`buffered_points()`（`:358-360`）、`max_points()`（`:363-365`）；v1.3-r2 起**另有消费方**：core-bin 健康巡检（上一条） |
| S-4 | 持续失败**按周期聚合告警**、不风暴 | ✅ **已实现（v1.3-r2；v1.3-r4 改为边沿触发）** | 两层：① 结构性缓解沿旧判 —— `since_attempt`（`:228-238`）使"失败后每个 push 都重试"的活锁式风暴不可能发生（恢复靠定时任务 `:476-513`）；② **告警聚合 = 边沿触发**（`storage_health.rs:145-187`）⇒ 无丢弃的周期**一条不发**；**一段丢弃事件（含"每个周期都在丢"）恰好 1 条**，段内各拍增量由 `episode_points/episode_batches` 累加、退出边沿时进日志 ⇒ 连续失败 10 周期 = 1 条（R-11.5-A4 字面） |
| S-5 | **不得阻塞采集**：`offer` 内不 await | ✅ **已实现（v1.3-r2）** | `buffer_telemetry`（`:313-343`）容量触发路径**已无任何 `.await`**（旧形态 `self.flush_batch(batch).await?` 删除）：触发只投一次**非阻塞**唤醒（`request_flush` `:355-367`，`mpsc::Sender::try_send`，容量 1、满即合并），DB 活（`begin/INSERT/commit`）搬进**已注册**的 `spawn_flush_timer` 任务（`:476-513` 的 `select!` 第三臂 `:501-502` + `flush_wake_once` `:621-632`）⇒ **采集调用栈里没有任何 DB 调用点**。⚠️ **p99 ≤ 10 ms 未在本机实测**（见 FLS-04 行） |

> **⚠️ 行号基线提示（v1.3-r2）**：上表**未标注 v1.3-r2** 的行仍是 `724225c` 基线（列头即声明），其 `services.rs` 行号自本批插入 `WriteBuffer` 段代码后**已整体后移**；标注 v1.3-r2 的 S-3②/S-4/S-5 行给的是**本批工作区**行号，两条基线**不得混用**。完整对照表见顶部 **v1.3-r2 开发交付** 行。

**接口形态订正（**原草案的 `TelemetrySink` trait 与 `OfferOutcome` 未被采用**，如实登记）**：

```rust
// 现状公开面（storage/src/services.rs）—— 采集侧入口仍是 async 的 buffer_telemetry
impl WriteBuffer {
    pub async fn buffer_telemetry(&self, point: TelemetryPoint) -> Result<(), StorageError>; // :273
    pub fn buffered_points(&self) -> usize;      // :358   ← 草案 buffered_len
    pub fn max_points(&self) -> usize;           // :363   ← 草案未列
    pub fn dropped_points(&self) -> u64;         // :368   ← 草案 dropped_total
    pub fn dropped_batches(&self) -> u64;        // :374   ← 草案未列
    pub fn requeued_batches(&self) -> u64;       // :380   ← 草案未列
    pub fn capacity(&self) -> usize;             // :532
    pub fn flush_interval_ms(&self) -> u64;      // :536
}
```

> **为什么没有 `TelemetrySink`/`OfferOutcome`**：加固选择了**最小面**（不新增 trait、不改采集侧调用签名），代价是 **S-5 与 S-3② 未闭合**。⇒ **对接方不得**按草案的 trait 形态接线（会编译不过）；**采集侧照旧 `buffer_telemetry(..).await`**。
>
> **v1.3-r2 补记**：**S-5 与 S-3② 已闭合，但形态仍不是 `TelemetrySink`/`OfferOutcome`** —— S-5 取"**`WriteBuffer` 内建一个容量 1 的唤醒通道 + 由已注册的 flush 任务接走**"（调用侧签名与调用方式**零改动**，`buffer_telemetry(..).await` 照旧）；S-3② 取"**core-bin 巡检读增量 → 既有 `AlertFeed`**"。⇒ **上述"对接方不得按草案 trait 形态接线"的结论不变**。

**残留缺口与落地方案（须由对接方实现，本章给出可编码的方案）**：

| # | 缺口 | 落地条件与方案 | 对应 PRD | 归属 |
|---|------|---------------|----------|------|
| 1 | S-3② 丢弃无 `major` 事件 | **storage 不发事件**（依赖方向不允许）⇒ 由 **core-bin 的健康巡检任务**读 `dropped_points()/dropped_batches()` 的**增量**，**在丢弃事件的边沿**调 `AlertFeed::push_system_alert("major", 文案)`（含增量条数 + 累计值）。**边沿触发**（见缺口 2 行）⇒ 一段丢弃事件恰好 1 条（同时闭合 S-4）。<br>✅ **v1.3-r2 已交付，v1.3-r4 改边沿**：新增 `mupc-core-bin/src/storage_health.rs`（判据 `:145-187`、告警三态 `HealthSignal` `:89-105`、循环 `:198-241`、生产接线 `:249-275`；周期常量 `HEALTH_TICK_MS` `:76` = 1 s；级别常量 `:80` = `"major"`，经**既有** `AlertFeed::push_system_alert` 投递、文案为单行中文含"本周期新增 N 点（M 次超限丢弃），累计 P 点（Q 次）"）；装配点 `startup.rs:1545-1552`（句柄入 `producers` **协作退出名单**，标签 `storage_health_timer`）。<br>**取舍：新增**独立 1 s 任务**而非**复用 `grid_agg_timer` 的 tick —— 理由：聚合任务的收工契约（drain 通道 + 关闭未闭合周期 + 入队）与"读告警计数"无关，混在一个 `select!` 里会让它的停机语义变模糊；"契合既有架构"体现在**同周期（1 s）、同名单（`producers`）**。<br>⚠️ **消费侧如实登记（既有、非本批改动）**：`AlertFeed` 的**生产侧**（`SouthSink` / 联锁 / 本巡检）都已投递，但 `subscribe()` 本期**没有任何生产消费者**（函数带 `#[allow(dead_code)]`；12 号设计 **§4.7** 已把该环定为「可选增强、非 F7 真源」）⇒ **FLS-03② 的 `major` 告警"投得进环、但生产侧无消费者"**，本条只证"告警按周期被投出"，**不得**据此声称"现场可观测告警" | R-11.5-A3② / A4 / FLS-03 | 新增 1 个 core-bin 任务（≤ 30 行） |
| 2 | S-4 聚合告警 | 同上一条：**边沿触发** + 一条 `major` 事件承载**整段**丢弃事件、段内各拍增量累加（"连续失败时段合并为一条并递增计数"）。<br>✅ **已交付**（判据 `storage_health.rs:145-187`）：**进入**丢弃态 ⇒ 恰好 1 条 `major`；**持续**丢弃（连续周期增量都 > 0）⇒ **一条都不再发**（增量并入 `episode_points/episode_batches`）；**恢复**（增量回落 0）⇒ **只记 `tracing::info!` 日志**（含本段总计 + 累计值），**不发告警**（理由：`AlertFeed` 只有入、无 ack/清除面，一条 `major` 级"已恢复"对按级过滤的消费者与"新发生一次 `major`"无法区分 —— 详见 `storage_health.rs` 模块头）；**再次进入** ⇒ 再 1 条（边沿可重现）。<br>⇒ **连续丢弃 10 个周期 = 恰好 1 条告警**；无丢弃 = 0 条。**判别锚**：`:369`（纯判据层 10 拍恰 1 条）、`:493`（任务层同一判据，且"持续窗口内累计丢弃确实 +≥10 点"防"没再丢"的假绿）、`:409`（恢复是 `DropRecovered` 而非 `DropStarted`）、`:440`（边沿重现 ⇒ 2 条）、`:329`（无丢弃 = 0 条）。<br>⚠️ **口径订正（v1.3-r4）**：v1.3-r2/v1.3-r3 曾把"只在**增量 > 0** 时发"写成"**自然按周期聚合**"并据此登记"每个周期都在丢 ⇒ 每周期 1 条"（当时**未逐字满足** PRD 字面，挂 **Q-A4**）—— 那是**误读**：该口径只做到"无丢弃时不发"。产品裁定 2026-09-26（03 PRD **v1.1.2**，提交 `effbaa0`）裁为**边沿触发**，本批（v1.3-r4）按字面落地，**Q-A4 关闭** | R-11.5-A4 | 同上 |
| 3 | S-5 采集路径仍 await | **两条路可选**（本章推荐 (a)）：(a) **把 `buffer_telemetry` 改为非阻塞**：内部改 `mpsc::Sender`（`try_send`），`flush/tick` 逻辑搬进 `spawn_flush_timer` 那个 task（旧基线 `:407-439`，本批后 `:476-513`）——但这会改 `buffer_telemetry` 的签名（`await` 仍在，只是不再干 DB 活）；**最小改法**：容量触发时**不在调用栈内 await**，而是 `tokio::spawn(flush_batch(batch))`（**保住签名、去掉阻塞**，代价是失败回填的时序略变，`BatchGuard` 已覆盖）；(b) 维持现状 + 在装配期给 `capacity` 配足够大（把 await 概率降到"每 1000 点一次"）。**推荐 (a) 的最小改法**。<br>✅ **v1.3-r2 已交付（取 (a) 的等价形态，未取 `tokio::spawn` 最小改法）**：`buffer_telemetry`（`services.rs:313-343`）容量触发**不再 drain、不再 await**，只投一次**非阻塞**唤醒（`request_flush` `:355-367`，`mpsc::Sender::try_send`，容量 1、满即合并）；DB 活在**已注册**的 `spawn_flush_timer` 任务里干（`select!` 第三臂 `:501-502` + `flush_wake_once` `:621-632`）。**为什么不用 `tokio::spawn(flush_batch(batch))`**：`flush_batch` 需 `'static` 而 `&self` 拿不到 `Arc`（要改签名），且游离 spawn **不登记在退出编排里** ⇒ 正是 T15/T16 刚修掉的"退出期窄竞态"形态（`startup.rs:459-478` 有同一条裁决）。**代价（如实登记）**：`buffer_telemetry` 的 `Err` 分支退化为永不触发（返回值恒 `Ok`，签名保留 ⇒ 调用点零改动）；容量触发的提交延迟从"调用栈内即刻"变为"已注册任务下一次 `select!` 醒来"（微秒级，且**不再计入采集耗时**）。⚠️ **口径订正（评审 W-4，"微秒级"偏乐观）**：该延迟**不是无条件微秒级** —— 若唤醒臂与周期 tick 臂**同时就绪**，`select!` 随机取一支，已 `recv()` 的唤醒可能被丢弃 ⇒ 容量触发的提交**最多顺延一拍**（默认 `flush_interval_ms` = 5 s）；**不丢数据**（点仍在缓冲内、有界，且退出路径最后 `flush()` 兜底），见 `services.rs` 的 `flush_wake_once` 注释 | R-11.5-A5 / FLS-04 | 对接方 |

> **§9.3 与 PRD 验收的对应（P3：不留交付缺口；v1.3-r2 收口）**：**FLS-01 / FLS-02 / FLS-03①③ / FLS-05 已随 `724225c` 交付**（本设计只做**对齐说明**）；**FLS-03② / FLS-04 由本节缺口 1/3 承接，已于 v1.3-r2 交付**（落点见上表与 §9.7 的 FLS 行）。⚠️ **唯一未在真机取证的验收项** = FLS-04 的 **p99 ≤ 10 ms**（本机不可测；以"采集调用栈内无 DB 调用点"的结构证据替代，见 §9.7 FLS-04 行）。

**崩溃/断电边界（PRD R-11.5-B）**：进程内不可恢复场景（断电、强杀、介质故障）**不在**上述要求内；内存中未提交数据的丢失属**已知边界**。§7.6「写入失败不上报告警」**仅对单条写入**成立；**批量丢失必须告警**（S-3）。

---

### 9.4 保留期与冷数据：边界声明

| 项 | 本章口径 |
|----|----------|
| 90 天保留（§6.1） | **口径不变**；总表聚合记录同纳 90 天（PRD §11.4） |
| 本期是否需要单独保留期 / 降采样 | **不需要**：90 天聚合记录 ≈ 570 MB（占分区 ~1.3%，§9.1.3） |
| `RetentionManager` | **未装配**（定义 `services.rs:602-639`；仅 `storage/src/lib.rs:14` 导出 + `tests/integration.rs:816-821` 使用，**生产装配无引用**）⇒ §6.2 自动清理**尚未生效**；"90 天保留"当前是**规格而非现状**（PRD §11.4 / Q6 建议 (a)） |
| 清理链路落地 | **另立需求**，不在本章（§8.8 未解决问题表已有"是否迁移到 mupc-storage 统一管理"一行的同源登记） |
| `storage:` 段是否承载保留期 | **不**（PRD R-11.3-A 明确本段只含三项；与 §5.6.2 原规划的 `/api/v1/storage` 不是同一组配置） |
| 冷数据下沉/归档 | **不做**（无需求依据；登记为未提出项） |

---

### 9.5 最新值入口：归属确认与引用（U-70）

**U-70 的消费方改判与数据可用性要求已由 [01 设计 §9.1](01-MUPC-通信网关-设计文档.md) 完整设计**（01/03/12 三份共用件）。本章只做两件事：

| 项 | 本章口径 |
|----|----------|
| **归属确认** | 03 PRD §11.6.2 R-11.6-D1 的「**本模块**须提供」由 `mupc-data-processing::latest_values` 满足——该 crate 即 03 模块的 crate（§1.2/§7.1）。**类型与所有权在 03，写入调用方在 core-bin（装配层）**；依赖方向 `southd → data-processing`、`core-bin → data-processing` 均已存在，**零新增边** |
| **不重复定义** | 数据结构（`PointValue/PointQuality/PointId/ChangeBatch`）、刷新活性（R-11.6-D2）、陈旧表达（R-11.6-D3）、变更通知 ≤1 s（R-11.6-D4）、禁轮询 `telemetry`（R-11.6-D5）**一律以 01 设计 §9.1 为准**，本章不复制、不另立门限 |
| **过期判据单一真源** | `stale_timeout_s = 5 s`（`SouthStationsConfig.stale_timeout_s`，`mupc-southd/src/config.rs:177`）由 `LatestValues::new` **注入**，`is_fresh` 是唯一实现 |
| 消费方分层（R-11.6-A） | `telemetry` 历史表 ⇒ **历史查询类**（报表/导出/复盘；本期无已实现消费方）；实时值 ⇒ 内存快照（上云/屏/策略）。**不得互相替代** |
| 已知设计余量 | `telemetry` 表**只写不读**（R-11.6-B）——本章与 §9.1 的总表聚合**同样只写**；**不得**据此宣称"历史查询已实现"（CNS-03 为评审项） |

---

### 9.6 装配点汇总（core-bin）

| 序 | 落点 | 改动 |
|----|------|------|
| 序 | 落点（**行号按 HEAD `724225c` 重校**） | 改动 |
|----|------|------|
| 1 | `core_config.rs`（`validate()` 在 `:464`、既有各段校验 `:506-515`） | 新增 `StorageSectionConfig` + 字段 `pub storage: StorageSectionConfig` + `validate_storage()` + 在 `validate()` 内调用（放在 `self.validate_display()?`（`:515`）之前） |
| 2 | 两份 deploy YAML（`deploy/config/mupc_core_config.yaml` / `.production.yaml`） | 新增 `storage:` 段（**注释态或显式默认值**，二者等效；建议显式写出以便现场可见） |
| 3 | `startup.rs:795-799` | `WriteBuffer::new(cfg.storage.batch_capacity, cfg.storage.flush_interval_ms, pool)` |
| 4 | `startup.rs`（`SouthSink` 构造前，现 `:1287` 附近） | `Arc<Mutex<GridAggregator::new(cfg.storage.grid_aggregate_period_ms)>>` |
| 5 | `SouthSink::on_grid_package`（`startup.rs:526-533`） | 新增：从 `pkg` 抽取 `GridSample` → `observe(now_ms, &s)` → 产出 `AggregateRow` → `to_telemetry_point("grid_meter")` → `WriteBuffer`。**在现有 `set_latest_data`（`:528`）+ `broadcast_grid_iec104`（`:532`）之后**（既有路径不动，GRD-07）。⚠️ `:532` 的 grid 上送支路按 [01 设计 v1.4 §9.2.2](01-MUPC-通信网关-设计文档.md) **将被删除**（并入 A 档任务）——**两条增量同时落地时以 01 号为准**，本序只管"在其后追加聚合" |
| 6 | `startup.rs`（`flush_timer` 注册处 `:804-806` 旁） | spawn tick 任务（1000 ms）→ `Mutex<GridAggregator>` → `tick(now_ms)` → 入队；**句柄入 `producers`（协作生产者名单，非 abort 名单）**，收工前 `flush(now)` |
| 7 | `main.rs` | **无需改动**（退出 flush 由步骤 6 的协作生产者契约 + 既有退出序列承担） |
| 8 | `storage/src/grid_aggregate.rs` | **新增**（§9.1.2；含 `Quality` 枚举、`AggregateRow::to_telemetry_point`、`CHANNELS` 表与单测） |
| 9 | `storage/src/models.rs` + `services.rs` + `repository.rs` | **`telemetry.value` 可空化**（§9.1.4 的迁移与 5 处代码点）——**本增量唯一的 `storage` 结构变更** |
| 10 | `mupc-core-bin/src/quality_map.rs` | **新增**（`quality_from_point_quality(PointQuality) -> i32`；**落装配层**，见 §9.8 D-8） |
| 11 | `storage/src/services.rs` 失败语义 | **已由 `724225c` 落地**（回填/有界/计数）；**残留 S-3②/S-4/S-5 已由 v1.3-r2 落地**：S-5 落 `services.rs:313-343`（`buffer_telemetry` 无 await）+ `:355-367`（`request_flush`）+ `:476-513`/`:621-632`（已注册 flush 任务接走唤醒）；S-3②/S-4 落新增 `mupc-core-bin/src/storage_health.rs`（§9.3 缺口表逐条给行号） |
| 12 | `mupc-core-bin/src/startup.rs`（`alert_feed` 构造之后） | **v1.3-r2 新增**：`producers.0.push(("storage_health_timer", spawn_storage_health_timer(write_buffer, alert_feed, stop_rx)))`（**v1.3-r4 校正行号：`:1545-1552`**）—— 03 设计 §9.3 缺口 1 的装配点；**与 `flush_timer`/`grid_agg_timer` 同名单**（协作退出），**不得**放入 abort 名单 |

**`GridSample` 抽取点（唯一）**：`mupc-data-processing::DataPackage` 的 `electrical.phase`（`Option<PhaseElectricalData>`）+ 顶层 6 字段。**缺相量块** ⇒ 分相通道全 `None`（产 `NoData` 行）；**顶层缺块** ⇒ 该通道 `None`。映射函数 `GridSample::from_package(&DataPackage) -> GridSample` 落在 `storage`（`From` 实现），单测可直喂构造的 `DataPackage`。

> **⚠️ 勘误（T15/T16 评审裁定，2026-09-24）：本行与 §9.8 D-8 冲突，以 D-8 为准。**
> 事实：`storage` 与 `data-processing` **互不依赖**（只有 `ai-engine`/`core-bin` 依赖 `storage`）⇒ 把 `from_package` 落 `storage` 会**新增 `storage → data-processing` 依赖边**，违 §9.1.1「不新增依赖边」。
> 更硬的一条：装配层写 `impl From<&DataPackage> for GridSample` **违孤儿规则（orphan rule）、不可编译**（两侧类型均为外部类型）⇒ 只能用**自由函数**。
> ⇒ **实现取 D-8**：抽取函数落 **`core-bin` 装配层**（`startup.rs` 内自由函数 `grid_sample_from_package`，语义等价、可单测）。本行中"落在 `storage`（`From` 实现）"作废。

---

### 9.7 测试策略

| 用例（对应 PRD §11.7） | 层次 | 要点 |
|------------------------|------|------|
| GRD-01 每 1 分钟恰 1 个周期 | 集成 | 跑 ≥ 3 min，按 `timestamp` 分组计数 |
| GRD-02 聚合是统计值 + **反例必红** | 单测 | 恒定序列 ⇒ 均值 == 该值；阶梯序列 ⇒ 均值/极值精确；**把实现改成取首值 ⇒ 用例变红**（注入式反向验证） |
| GRD-03 时间戳 = 周期起点且整倍 | 单测 | 注入 `ts = 1_000_000_001`、period 60000 ⇒ `start == 999_960_000` |
| GRD-04 无采样仍产行 + `quality` 可区分 | 单测 + 库内断言 | `tick` 跨 1 周期 ⇒ 22 行存在、`quality == 1(NoData)`、**`value IS NULL`**（§9.1.4 的映射；**不是 0**）；对比用例：注入真实 0 值采样 ⇒ 22 行 `quality == 0`、`value == 0.0` ⇒ **二者在库内可区分**（这正是 PRD 要求"可区分"的机械判据） |
| GRD-05 通道集合 == 表 | 单测 | `CHANNELS` 的 `metric_name` 集合逐一断言；缺测通道 `quality == NoData` |
| GRD-06 行数下降 ≥ 95% | 集成 | 连续 1 h 实测外推（22 行/min vs 逐点 21×60） |
| GRD-07 不影响北向上送与策略 | 集成 | 对照 `on_grid_package` 前后的 IEC104 上送内容与 `latest_data` |
| GRD-08 无"关闭"路径 | 配置测试 | 穷举 `storage:` 取值，无关闭语义 |
| STG-01 缺省 = 现状 | 集成 | 回放对照（**逐条一致**） |
| STG-02 显式配置生效 | 集成 | `batch_capacity: 200` ⇒ 满 200 提交；`flush_interval_ms: 2000` ⇒ 最迟 2000 ms 提交 |
| STG-03 重启生效 | 集成 | 改 YAML → 重启 → 生效；不重启 → 不生效 |
| STG-04 非法值拒启动且点名键 | 单测 | `validate_storage()` 逐字段边界（含 `% 1000 != 0`） |
| STG-05 两参数正交 | 单测 | 只改节拍 ⇒ 周期粒不变；只改周期 ⇒ 提交行为不变（结构断言 + 行为断言） |
| STG-06 不进 DB | 结构检查 | 库内无新表/覆写项 |
| **FLS-01 / FLS-02 / FLS-05** | 集成 | ✅ **已随 `724225c` 交付**：注入提交失败 ⇒ 缓冲条数不减、恢复后**行数守恒**（FLS-01，`services.rs:470-503` + `:550-599`）；持续失败 ⇒ 条数 ≤ **10,000**、丢最旧（FLS-02，`:207/:308-320`）；断电/强杀不在要求内（FLS-05，§9.3 末）⇒ 本设计只需**回放断言**，不需新实现 |
| **FLS-03** | 集成 | ✅ **已交付；② 的告警聚合已按 R-11.5-A4 字面落地（v1.3-r4，边沿触发）**：① error 日志 ✅（`log_dropped` 旧基线 `:322-330` → 本批 `:385-399`、失败批日志旧 `:484-499` → 本批 `:558-573`）；② `major` 事件 ✅ —— **`mupc-core-bin/src/storage_health.rs`**：边沿状态机 `:145-187`（进入丢弃态投**恰好 1 条**，含本拍增量 + 累计值）、三态 `HealthSignal` `:89-105`、1 s 巡检 `:198-241`（`emit` **只在进入臂**被调）、生产接线 `:249-275`、装配 `startup.rs:1545-1552`；③ 计数可观测 ✅（`dropped_points/dropped_batches/requeued_batches` 旧基线 `:368-383` → 本批 `:431-447`）。**判据（判别锚）**：`no_drop_means_no_alert_at_all`（`:329`，全程无丢弃 ⇒ 0 条）、`edge_enter_emits_exactly_one_alert_with_increment_and_total`（`:345`，进入 ⇒ 恰 1 条 + 文案含增量与累计）、`ten_consecutive_drop_ticks_still_yield_exactly_one_alert`（`:369`，**连续丢弃 10 拍 ⇒ 累计仍恰 1 条**）、`recovery_edge_is_signalled_as_log_only_not_as_a_new_alert`（`:409`，恢复是 `DropRecovered` 而非新告警）、`edge_is_reproducible_after_recovery`（`:440`，再次进入 ⇒ 再 1 条）、`edge_triggered_loop_emits_one_alert_per_episode_not_per_tick`（`:493`，任务层同一判据 + 级别字面量 `"major"` + "持续窗口内累计丢弃确实 +≥10 点"与"窗口内又跑了 ≥N 拍"排除假绿）、`drop_alert_level_is_major`（`:321`）、`startup_wires_the_health_timer_into_the_cooperative_list`（`:643`，装配接线/协作名单）；**行为级证据**另在 `storage/tests/integration.rs:418`/`:458` 覆盖"丢弃 → 计数 → 告警"的上游链。⚠️ **恢复只记日志、不发告警**（本批裁定，理由见 `storage_health.rs` 模块头：`AlertFeed` 无 ack/清除面，`major` 级"已恢复"与"新告警"对按级过滤的消费者不可区分）——该侧由 `:409` 与 `:493` 的"恢复后仍恰 1 条"钉住。<br>✅ **Q-A4 关闭**：产品裁定 2026-09-26（03 PRD **v1.1.2**，提交 `effbaa0`）="**连续失败 10 个周期不得产生 ≥10 条告警**"按**边沿触发**落地；本批实现即该口径，**不再有"字面未逐字满足"的保留**（v1.3-r2/r3 曾据"增量 > 0 即发"登记该保留，已随本批作废）。⚠️ **仍未变**：`AlertFeed` 生产侧在投、`subscribe()` 本期**无生产消费者** ⇒ 本条只证"告警按边沿被投出"，**不得**读成"现场可观测告警" |
| **FLS-04** | 性能测试 | ⚠️ **结构已交付、性能验收未取证（v1.3-r2）**：实现 = §9.3 缺口 3（`buffer_telemetry` 容量触发**不再 await**，改投**非阻塞**唤醒，DB 活搬进**已注册**的 `spawn_flush_timer` 任务；**未取**设计推荐的 `tokio::spawn` 最小改法，理由见该行）。**结构证据**：`services.rs:313-343` 函数体内**无任何 `.await`、无 `flush_batch`**（判别锚 `services.rs` 的 `telemetry_ingest_path_has_no_db_call_in_its_call_stack` `:1050`）；**行为证据**：`storage/tests/integration.rs:458`（正常库上容量触发后**库里 0 行**、且 `pool.close()` 后采集侧**仍拿不到 `Err`** —— 旧实现两种情形都红，已用探针 P1 实测）与 `:418`（唤醒由已注册任务接走并落库，`flush_interval_ms=60_000` 排除周期触发）。**验收缺口（如实登记）**：**采集侧写入 p99 ≤ 10 ms 未在本机实测**（本机无南向真源与 NPU/串口链路，压测环境不具备）⇒ 该验收项**仍需真机/压测**；本批以"**采集调用栈里已经没有 DB 调用点**"这一**结构性**判据替代，**不得**据此宣称 p99 已达标（HST-ELEC-02 的回归需在同一真机压测中一并复核） |
| GRD-09（**新增**，本设计） | 单测 | **`value` 可空化的迁移幂等**：对已迁移库再跑 `run_migrations` ⇒ 不重复重建（`PRAGMA table_info` 判 `notnull == 0` 即跳过）、既有行数值不变（§9.1.4） |
| CNS-01/02 | 集成 | 锁 `telemetry` 表 30 s / 注入 DB 写延迟 ⇒ 屏与上送不受影响 |
| CNS-04 | 集成 | 引用 [01 设计 §9.1](01-MUPC-通信网关-设计文档.md) 的快照用例（不重复实现） |

---

### 9.8 与既有设计的冲突清单与取舍

| # | 冲突 | 取舍（本章裁定） |
|---|------|------------------|
| **D-1** | 本章 §9.1.3 落库 **18+4 = 22 行/周期** vs PRD §11.2-F 的 **21+6 = 27 行/周期** | 差异来源：PRD 的 21 含频率与视在功率；本章按 PRD **Q3/Q4/Q5 的建议**（频率"无源"不落、电能不落、S 待定）取 18 均值通道。**表驱动**：产品若改选 Q3(a) 全通道落库 ⇒ 改 `CHANNELS` 表一行，算法与单测不变。§9.9 C-1 |
| **D-2** | §4.4.2 的「容量满 1000 或间隔满 5000 ms」与本章 §9.2 的**可配置化** | **口径不变，来源改为配置**：默认值与 §4.4.2 逐字相同（零行为变化）；§4.4.2 的「本轮不引入配置项」自本章起**被取代**（其口径对齐注记同步更新，见 §9.10） |
| **D-3** | §4.4.2 的「概念消歧」（flush 窗口 ≠ 存储周期）与本章的 `grid_aggregate_period_ms` | **同源落地**：`grid_aggregate_period_ms` 即 §4.4.2 所指的"**存储周期**"（此前"未实现"）；`batch_capacity`/`flush_interval_ms` 即"**flush 窗口**"。两者在**两个类型**中实现（§9.2.2 末），结构上不可互推 |
| **D-4** | §4.1.1「按可配置的存储周期将**电气量数据**持久化」此前只覆盖核间 `TelemetryData`；本章把它落到**台区总表** | 本章为**扩展而非替换**：`telemetry` 表的其它写入方（电池/告警/事件/外设遥测）**不改**。总表以 `device_id = "grid_meter"` 区分 |
| **D-5**（v1.3 订正；**v1.3-r2 已闭合结构面**） | `buffer_telemetry` 在容量路径 `await` 提交（旧基线 `services.rs:300-303`）vs PRD R-11.5-A5 的 p99 ≤ 10 ms | **结构面已闭合（v1.3-r2）**：容量触发**不再 drain、不再 await**，改投**非阻塞**唤醒（`services.rs:355-380`），DB 活搬进**已注册**的 `spawn_flush_timer` 任务（`:476-513`）⇒ 采集调用栈里**没有 DB 调用点**（判别锚 `services.rs` 的 `telemetry_ingest_path_has_no_db_call_in_its_call_stack` `:1050`；行为锚 `storage/tests/integration.rs:458`）。**未取**设计推荐的 `tokio::spawn(flush_batch)` 最小改法 —— 它需改签名（`'static`）且是**未登记在退出编排里**的游离 spawn（T15/T16 刚修掉的窄竞态形态）；等价替代（"**已在退出编排里的**任务干 DB 活"）见 §9.3 缺口 3 行。**仍未闭合面** = **p99 ≤ 10 ms 的真机压测取证**（本机不具备；HST-ELEC-02 回归须同批复核）—— 登记为**唯一剩余验收项**。原稿"期望接缝 = 非阻塞 `offer` trait"**仍未被采用**（接口形态订正，见 §9.3） |
| **D-6** | §7.3「存储容量规划」按"每设备每周期 1 条"的逻辑记录口径；本章按窄表物理行口径（22 行/周期/设备） | **两口径不可互推**（PRD §11.2-F 的口径提示已明示相差约 27×）。本章的容量结论**只以 §9.1.3 的物理行为准**；§7.3 是否按物理行重算**属另立需求**（PRD §11.9 同款登记），本章**不改 §7.3** |
| **D-7** | 附录 C 的 `quality`（`good`/`invalid`/`reserved`）与本章新增的 `Quality` 枚举（5 值） | **扩展枚举**：`Good = 0` **保持既有写入值 0 不变**；`Reserved` 在附录 C 中未定义取值 ⇒ 本章不占用其语义，新增 `NoData`/`Stale`/`Unconfigured` 为更大取值。附录 C 应随本章更新（**登记为文档同步项**，§9.10） |
| **D-8**（v1.3 新增，**依赖边**） | 原稿把 `Quality::from_point_quality` 的落点写成 `storage` —— 但 `storage` **不依赖 `data-processing`**（`storage/Cargo.toml` 无该依赖；反向也无）⇒ 放 `storage` 会**新增 `storage → data-processing` 依赖边**（纯为一次枚举映射，代价不成比例） | **改落点：映射函数落装配层 `mupc-core-bin/src/quality_map.rs`**（core-bin 同时依赖两者，**零新增边**）。`storage` 只拥有 `Quality` 枚举（落库记录形态的唯一所有者），**不认识 `PointQuality`**；`data-processing` 只拥有 `PointQuality`，**不认识 `Quality`**。转换只在装配层发生（与"装配层是跨域转换点"的既有口径一致） |

---

### 9.9 待产品 / 项目经理裁定项

| # | 事项 | 选项 | 本设计默认 |
|---|------|------|-----------|
| Q-2 | 分钟极值覆盖通道（PRD Q2） | (a) 全部可落通道 (b) 仅总有功/总无功/频率 (c) 不存 | **(b)**，但"频率"因无源被排除 ⇒ 实为 `p_total`/`q_total` 两通道（+4 行/周期） |
| Q-3 | 落库通道范围（PRD Q3） | (a) 顶层 6 通道 (b) 顶层 + 分相 21 通道（含视在功率 S） | **(b) 的分相部分**：15 分相 + `p_total` + `q_total` + `pf_total` = 18；**S 不落**（无点表来源） |
| Q-4 | 电能（进/出）无点表来源（PRD Q4） | (a) 本期不落 + 标注"无源" (b) 补点表 | **(a)** |
| Q-5 | 频率（恒 50.0）与总 PF（取 A 相）（PRD Q5） | (a) 照落 (b) 标"无源"不落 (c) 补点表 | **频率 (b) 不落**（常量入库污染统计）；**`pf_total` 保留但须在文档注明"取 A 相"**（若产品要求语义严格 ⇒ 删该行，落库变 17 通道） |
| Q-6 | 保留期与清理链路（PRD Q6） | (a) 沿用 90 天 + 清理另立 (b) 本次一并立 | **(a)**，并如实登记 `RetentionManager` 未装配 |
| Q-8 | 最新值快照是否本期立（PRD Q8） | (a) 本期立 (b) 随 U-73/U-74 另立 | **(b)**：本期只落**数据可用性要求**（已在 01 §9.1 设计），排期随 U-74 |
| **Q-7**（v1.3 新增） | **缺测行的落库形态**（P2）：`AggregateRow.value: Option<f64>` 与 `telemetry.value REAL NOT NULL` 冲突，无采样行的 `value` 怎么写？ | (a) **`telemetry.value` 可空化 + 写 `NULL`**（本章裁定） (b) 维持 `NOT NULL`、写 `0.0` + `quality = NoData` (c) 缺测不产行 | **(a)**；已给出迁移与 5 处代码点（§9.1.4）。**若产品认可 (b)**（零迁移），则需接受"库内缺测行的 `value` 为 0"，并把"先按 `quality` 过滤"写成强制查询契约 |
| **Q-9**（v1.3 新增） | **缓冲上界口径**（P4）：实现常量 **10_000** vs PRD R-11.5-A2 的"默认 **2 × batch_capacity**" | (a) 以实现为准（10,000，请求 PRD 订正） (b) 新增配置键 `storage.max_buffered_points`（PRD Q1 附带项） (c) 改实现为 `2 × batch_capacity`（默认 2,000） | **(a)**；理由见 §9.2 末（10 个满批 + ≈1 MB 上界）。**FLS-02 的断言口径按 10,000** |
| ✚ C-1 | 订正 PRD §11.2-F 的"21 均值 + 6 极值 = 27 行"为本章的 **18+4 = 22 行**（或产品回选含频率 ⇒ 维持 27） | 纯文档订正 | 请项目经理在 PRD 修订时同步 |
| ✚ C-2 | 跨重启空档"不回溯补产"是否可接受（§9.1.5） | (a) 接受（时间戳跳变可识别） (b) 需 `NoData` 行补齐 | **(a)**；若选 (b) 需给 `last_start_ms` 落盘（新增持久化项，不建议） |
| ✚ C-3（v1.3 新增） | 订正 PRD R-11.5-A2 的"默认 = 2 × batch_capacity"为"默认 **10,000 条**（常量，不随 `batch_capacity` 缩放）" | 纯文档订正 | 请项目经理在 PRD 修订时同步（同 Q-9） |
| ✚ C-4（v1.3 新增） | 订正 PRD R-11.2-E 的实现口径：缺测行**写 `NULL`**（而不是任何数值），需在 PRD 注明"`telemetry.value` 可空" | 纯文档订正（并与 §7.3 的"每行 200 B"口径无关） | 请项目经理在 PRD 修订时同步（同 Q-7） |

---

### 9.10 与既有条目的关系与文档同步项

| 既有条目 | 本章的作用 |
|----------|------------|
| §4.1.1 / HST-ELEC-01 | "存储周期可配置（默认 1 分钟）"自本章起由 `storage.grid_aggregate_period_ms` 承载（STG-02 / GRD-01 验收） |
| §4.4.2 | 「本轮不引入配置项」自本章起被 §9.2 取代；「概念消歧」保持不变并落地为两个类型（D-3） |
| §4.5.1 / §6.1 / §6.2 | 口径不变；`RetentionManager` 未装配的事实见 §9.4 |
| §4.5.3/§4.5.4（磁盘空间与降级模式） | 本章**不改**。⚠️ 与 §9.3 的失败语义存在**潜在交叉**（>95% 停止时序写入 vs "不得静默丢弃"）：本设计口径 = 磁盘降级触发的写入停止仍须**计数 + 告警**（不得静默），与 S-3 一致 |
| 附录 C（数据质量标记） | **须随本章更新**（D-7）：新增 `NoData/Stale/Unconfigured` 三个取值并注明与 `PointQuality` 的映射（**映射函数在 core-bin 装配层**，`quality_map.rs`，见 D-8） |
| `models.rs` / `repository.rs` | **须随本章改**：`TelemetryPoint.value: f64 → Option<f64>`（§9.1.4 迁移）；`models.rs` 的 `value` 字段注释须写"`None` = 无数据，**只有总表聚合的缺测行会写 `None`**" |
| §9.3（本章旧版"期望接缝"） | 自 v1.3 起改为"**与已合并实现的对接说明**"（§9.3）；原 `TelemetrySink`/`OfferOutcome` 草案**未被采用**，实现沿用 `WriteBuffer::buffer_telemetry` |
| §7.3 | **不改**（D-6）；按物理行重算属另立需求 |
| §8.8 未解决问题表 | 本章为其中的"是否需要将故障录波迁移到 storage"提供**旁证**（storage 已是落库主体），但不裁定该项 |

---

## 附录 A：非功能性需求汇总

| 指标 | 要求 |
|------|------|
| 遥测数据上送频率 | >= 1Hz（可配置） |
| 数据写入延迟 | <= 10ms（非阻塞写入） |
| 消息总线吞吐量 | >= 1000 msg/s |
| 并发设备支持 | 同时处理 200 台南向设备 |
| 数据写入吞吐量 | >= 1000 条/秒 |
| 并发查询请求 | 最多 10 个并发 |
| 采样值分辨率 | >= 16 bit |
| 采样值精度 | +/- 0.5% of reading |
| 时间戳精度 | +/- 100us |
| 触发检测延迟 | <= 1ms |
| 录波启动延迟 | <= 1 个采样周期 |
| 录波期间 CPU 峰值 | <= 15% (RK3588 @1.8GHz) |
| 稳态内存占用（录波） | <= 2MB |
| 录波峰值内存 | <= 10MB |
| data-processing 整体稳态内存 | < 10MB（不含数据库缓存） |

## 附录 B：数据库文件布局

```
/var/mupc/
├── data/
│   ├── mupc.db                  # 主数据库（台账+告警+事件+遥测）
│   ├── mupc.db-wal              # WAL 日志文件
│   └── mupc.db-shm              # 共享内存文件
├── waveforms/
│   ├── index/
│   │   └── fault_records.db     # 故障录波元数据（独立 SQLite）
│   └── recordings/
│       └── YYYY/MM/
│           └── YYYYMMDD_HHMMSS_seq.wave
├── export/
│   ├── telemetry_*.csv
│   ├── comtrade/YYYYMMDD_HHMMSS_seq/ (cfg + dat + hdr)
│   └── csv/YYYYMMDD_HHMMSS_seq.csv
└── config/
    └── storage.toml             # 存储配置
```

## 附录 C：数据质量标记

| quality 值 | 含义 | 说明 |
|------------|------|------|
| `good` | 数据有效 | 数据采集正常，质量可靠 |
| `invalid` | 数据无效 | 采集异常，数据不可用 |
| `reserved` | 保留 | 备用 |

---

## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| **v1.3-r5（2026-09-26，PCS 迁出后的连带标注；**未加任何门禁标记**）** | **只在四处"intercore 接收实时控制模块高频采样数据"类表述处加注，不改 §9 任何语义口径**。来源 = **PCS 通信与控制整体迁入南向**（02 号设计 **§13** / **ADR-014**）落地（T1–T12，见 02 号设计 §13.13）。加注内容 = **"PCS 通道已迁至南向（02 §13）；本节所述的核间数据面在生产路径未启用"**：PCS（= 实时控制模块）的通信与控制现由 `mupc-southd::pcs::PcsHandle` 承担；`intercore` 仅保留**纯核间 TCP 帧协议**供后续演进，且客户端**只发不收**、**生产路径暂无消费者**（02 号设计 **Δ-23** / `technical-debt.md` §6.13）。落点 = §1.1 职责项、§1.3 上游模块表、§2.1 `DataCollector` 职责、§2.1 数据来源。**未改**：§9 的任何裁定/数字/判据、§1–§8 的其余内容（含 §2.1 既有的 2026-08-14 实现说明）、任何代码/配置/PRD；既有 `[DESIGN_APPROVED: 2026-09-23]`（覆盖 §9）原文未动、覆盖范围不变；**本次未加任何门禁标记**。 |
| v1.0 | 初版：合并 Phase3A 实施计划、故障录波与数据存储设计，定义数据处理与存储五大功能域 |
| v1.1 | flush 口径对齐实现现状（**容量 1000 条 / 间隔 5000 ms**，原写「100ms 或 100 条」）并说明取此量级的理由（避免高频小事务与写入放大，SQLite WAL / SD 卡寿命）；新增概念消歧「**flush 窗口**（提交节拍）≠ **存储周期**（采样/落库聚合周期）」，并注明"存储周期可配置"当前未实现（`core_config` 无 storage 段，另册登记 D2）。本轮只改口径、不新增配置项、不改 PRD |
| **v1.3-r4（2026-09-26，开发交付：FLS-03② 告警聚合改边沿触发）** | **只改 §9.3 / §9.6（序 12 行号）/ §9.7 的状态与行号 + 顶部注记；本增量未经设计复审**（不构成新契约，顶部既有 `[DESIGN_APPROVED: 2026-09-23]` 覆盖范围仍为 §9 v1.3 增量）。**① 依据**：产品裁定 2026-09-26（03 PRD **v1.1.2**，提交 `effbaa0`）—— FLS-03 的告警聚合按 **R-11.5-A4 字面**收口为**边沿触发**，本批落地。**② 口径**：`StorageHealthWatch::poll`（`storage_health.rs:145-187`）改为三态边沿状态机（`HealthSignal` `:89-105`）—— 进入丢弃态投**恰好 1 条** `major`（含本拍增量 + 累计值）/ 持续态**一条不发**（各拍增量并入 `episode_points`、`episode_batches`）/ 恢复**只记 `tracing::info!`、不发告警**（`AlertFeed` 无 ack/清除面 ⇒ `major` 级"已恢复"与"新告警"对按级过滤的消费者不可区分）/ 再次进入再 1 条 ⇒ **连续丢弃 10 周期 = 1 条**。**③ 订正（误读清理）**：v1.3-r2/r3 的"增量 > 0 即发 ⇒ **自然按周期聚合**"系**误读** —— 它只做到"无丢弃时不发"，"**每个周期都在丢**"时仍每周期 1 条（10 周期 = 10 条）⇒ 相关表述在 §9.3（S-3② / S-4 / 缺口 1 / 缺口 2 行）与 §9.7（FLS-03 行）**逐处更正**，**Q-A4 关闭**（原"字面未逐字满足、待裁定"的保留作废）。**④ 判别锚**：`storage_health.rs:321/329/345/369/409/440/493/643`（`:369` 纯判据层 + `:493` 任务层 = "连续丢弃 10 拍仍恰 1 条"的核心判据，后者含"累计丢弃确实在涨"与"窗口内又跑了 ≥N 拍"两条假绿排除）。**⑤ "能红"探针**：`poll` 改回"增量 > 0 即发" ⇒ `:369`/`:493` 红（任务层实测 11 拍 11 条 = 告警风暴），`cp` 还原 + `sha256sum`/`cmp` 逐字节举证。**⑥ 行号换新**：`storage_health.rs` 旧 `:41/:45/:78-92/:101-129/:137-163/:190/:199/:233/:366` → 新 `:76/:80/:145-187/:198-241/:249-275/:321/:329/:345/…`；`startup.rs` 装配点 `:1462-1477` → **`:1545-1552`**（§9.3 与 §9.6 序 12 同步；该锚在本分支因 PCS 装配段插入而位移）；`storage` 侧行号与 §9.0/§9.1 未动。**未改**：PRD、§4/§6/§7、§9.1/§9.2/§9.9 口径与数字、顶部既有门禁标记 |
| **v1.3-r3（2026-09-26，评审收尾：W-1…W-8 + AlertFeed 消费侧登记）** | **只改 §9.2.1 / §9.2.2 / §9.3 / 行号与注记，不改 §9 任何语义口径；本增量未经设计复审**（顶部既有 `[DESIGN_APPROVED: 2026-09-23]` 覆盖范围仍为 §9 v1.3 增量）。**① W-1（P2，真缺陷）已收口**：`batch_capacity` 下界 **1 → 2** —— `capacity = 1` 时每次 push 都落容量分支（`since_attempt` 每 push 归零）⇒ 采集路径**永不 `trim_oldest`** ⇒ `max_points + 1` 上界论证失效。改法取**路线①**（`validate_storage` 拒 1 + 错误文案逐字给出理由，`core_config.rs` 的 `2..=100_000`）；`services.rs` 的"上界仍是 `max_points + 1`"注释**补足前提「capacity ≥ 2」**。⚠️ **与 PRD R-11.3-E（03 PRD:1540 的「1 ~ 100000（须 ≥ 1）」）的差异如实登记**：实现接受域为 PRD 字面域的**真子集**，**PRD 一字未改、待产品确认**；本节 §9.2.1 表 + §9.2.2 代码骨架同步为 `2..=100000`。**"能红"证据**：`stg04_storage_invalid_values_are_rejected_by_name` 新增 `batch_capacity = 1 ⇒ unwrap_err()`，探针把范围改回 `1..=100_000` ⇒ 该断言即红。**② W-3 行号口径统一**：`buffer_telemetry` 的范围在本文内两种写法（`:313-343` 正确 / `:313-353` 错）⇒ 4 处 `:313-353`（§9.3 缺口 3 行 / §9.6 序 11 / §9.8 D-5 / 版本块）**统一为 `:313-343`**（函数体实际止于 `:343`）。**③ W-4 口径订正**：§9.3 缺口 3 行的"容量触发提交延迟（微秒级）"**偏乐观** ⇒ 补"**唤醒臂与 tick 臂同时就绪时最多顺延一拍**（默认 `flush_interval_ms` = 5 s；**不丢数据**、退出路径最后 `flush()` 兜底）"。**④ AlertFeed 消费侧如实登记**：§9.3 缺口 1 行补"`AlertFeed` 生产侧在投，但 `subscribe()` 本期**无生产消费者**（12 号设计 §4.7 已定为可选增强）⇒ FLS-03② 的 `major` **投得进环、无消费者**，不得读成现场可观测告警"（对应评审"既有（非本批）"项）。**⑤ W-2 / W-5 / W-6 / W-7 / W-8 为文案/装饰性收尾**（不在本设计文档内，落 `storage_health.rs` / `alert_feed.rs` / `services.rs` / `storage/tests/integration.rs`）：abort 名单理由改述为"真差别 = 协作名单会 join 确认收工"、`FeedItem.subtype` 注释纳入 `major`、`writebuffer_flush_on_capacity` 文档注纠"库里 0 行"归兄弟用例、"未被取走即丢弃"改为"留在容量 1 通道（满即合并）"、装配网那句 `guard.0.push` 断言**删除**（实测该变异**不可编译**：`guard.0: Vec<JoinHandle<()>>` vs `producers.0: Vec<(&str, JoinHandle<()>)>` ⇒ E0308，属性由**类型系统**保证、断言不可达无判别力）。**⑥ 待裁定项原样登记、不代为裁定**：**Q-A4**（PRD R-11.5-A4 字面 vs 设计选定口径）与 **Q-p99**（真机压测）**维持 v1.3-r2 登记**；另按评审"收尾要求①"**改 §9.7 FLS-03 行措辞** —— 原写"告警按周期聚合…由'仅在增量 > 0 时发'**满足**"⇒ 改为"**按设计选定口径（'丢弃周期'聚合）充分**；**PRD 字面（每周期都在丢 ⇒ 10 条）未逐字满足，待产品/设计裁定**（按字面 ⇒ 需更粗窗口 = 设计变更、须重新走设计评审）"，**不得**再读成"聚合要求已满足"。**未改**：PRD、§4 / §6 / §7、§9.1 / §9.9 口径与数字、顶部既有门禁标记 |
| **v1.3-r2（2026-09-26，开发交付登记：FLS-03② / FLS-04 落地）** | **只改 §9.3 / §9.6 / §9.7 / §9.8 的状态与行号，不改任何语义口径；本增量未经设计复审**（不构成新契约，顶部既有 `[DESIGN_APPROVED: 2026-09-23]` 覆盖范围仍为 §9 v1.3 增量）。**① FLS-04（S-5）已交付**：`buffer_telemetry`（`storage/src/services.rs:313-343`）容量触发**不再 drain、不再 `await`**，改投**非阻塞**唤醒（`request_flush` `:355-367`）；DB 活搬进**已注册**的 `spawn_flush_timer` 任务（`:476-513` 的 `select!` 第三臂 `:501-502` + `flush_wake_once` `:621-632`）⇒ **采集调用栈无 DB 调用点**。**未取**设计推荐的 `tokio::spawn(flush_batch)`：需改签名（`'static`）且是**未登记在退出编排**里的游离 spawn（T15/T16 修复的窄竞态形态，`startup.rs:459-478` 同裁决）。判别锚：`services.rs:1050`（结构网：函数体内无 `.await`/无 `flush_batch`）、`storage/tests/integration.rs:458`（正常库上容量触发后**库里 0 行** + `pool.close()` 后采集侧**仍无 `Err`**）、`:418`（唤醒由已注册任务接走并落库，`flush_interval_ms=60_000` 排除周期触发）。**② FLS-03②（S-3②/S-4）已交付**：新增 `mupc-core-bin/src/storage_health.rs`（判据 `:78-92`、1 s 循环 `:101-129`、生产接线 `:137-163`；级别 `"major"` `:45`），装配 `startup.rs:1462-1477`（`producers` 协作退出名单）；**只在增量 > 0 时投**⇒ 缺口 2 的周期聚合由同一判据闭合。判别锚：`storage_health.rs:190/199/233/366`。**③ 未取证项（不得宣称达标）**：FLS-04 的 **p99 ≤ 10 ms** 需真机/压测（本机不具备），以"采集调用栈内无 DB 调用点"的**结构**判据替代，HST-ELEC-02 回归须同批复核；`buffer_telemetry` 的 `Err` 分支退化为永不触发（恒 `Ok`，签名保留、调用点零改动）。**④ 行号基线漂移已登记**（§9.3 与顶部 v1.3-r2 行给全量对照表）：本批在 `WriteBuffer` 段插入代码 ⇒ `services.rs` 旧基线（`724225c`）行号在该段之后整体后移；**§9.0/§9.1/§9.4 的全量行号未逐条重校**（待办）。**未改**：PRD（需求侧一字未动）、§9.1/§9.2/§9.9 口径与数字、§4/§6/§7、顶部既有门禁标记 |
| **v1.3-r1（2026-09-23）** | **设计评审员复审：`[DESIGN_APPROVED: 2026-09-23]`**（仅覆盖 §9 增量）。**验收结果**：P1 已解决（§9.0 按 HEAD `724225c` 重校并逐项标"已修/未修"；`storage/src/services.rs` 的行号 `:207`/`:273`/`:358-383`/`:407-439`/`:470-503`/`:550-599`/`:602-639`/`:647`/`:650-661` **抽查全部精确**）；P2 已解决（`telemetry.value` 可空化 + 缺测写 `NULL` + 幂等迁移 + 5 处代码点，与 PRD R-11.2-E 字面一致；否决"写 0.0 + quality""不产行"两案的理由成立）；P3 已解决（FLS-01/02/05 与 FLS-03①③ 已交付并逐条给实现行号；FLS-03②/FLS-04 明确标"未交付"并给出可编码落地方案）；P4 已解决（上界采用 `10_000`，与 `services.rs:207` 实现常量一致；与 PRD R-11.5-A2 的偏差已登记 Q-9/C-3）；P5 已解决（`quality_from_point_quality` 改落 `mupc-core-bin/src/quality_map.rs`；经核 `storage/Cargo.toml` 确无 `mupc-data-processing` 依赖、`mupc-southd` 与 core-bin 均已依赖 `data-processing` ⇒ 零新增边成立）。**须随本批开发同一提交修正的勘误（4 项，不改变裁决）**：① §9.1.4 受影响代码点④误列 `data-processing/src/high_freq_telemetry.rs` —— 该文件的 `TelemetryPoint` 是**另一（crate 内私有）类型**、无 `value` 字段，且 `data-processing` 不依赖 `mupc-storage` ⇒ **无需改动**；同处"共 **12** 处"系含该类型**定义**的 grep 计数，`mupc_storage::TelemetryPoint` 构造点实为 7 处（`startup.rs:379/559/1809` + `storage/tests/integration.rs` 4 处），清单须按实数逐处列；② §9.1.4 迁移只重建 **1 个**索引，`services.rs:658-661` 实有**两个**（`idx_telemetry_device_ts` / `idx_telemetry_metric_ts`）⇒ 须一并重建（否则 `query_range` 的 `device_id + timestamp` 路径丢索引，至下次启动 `CREATE INDEX IF NOT EXISTS` 才恢复）；③ 行号微差：`models.rs:10` → **:11**、`repository.rs:490`（`TelemetryRow`）→ **:486**、`config.rs:177`（`stale_timeout_s`）→ **:178**；④ §9.6 序 4/序 5 的 `:1287`（`SouthSink::new`）与 §9.1.3 的 `mupc_core_config.yaml:413-426` 与本文其余行号一致，无需改（仅记录已核）。**开发前必决**：迁移（含两索引）须纳入本批；FLS-03②/FLS-04 须指定归属（§9.3 缺口 1/3 的"对接方"） |
| v1.3 | **按设计评审意见修订 §9（P1–P5 逐条闭合），设计评审复审通过（`[DESIGN_APPROVED: 2026-09-23]`）**：① **P1** §9.0 基线按 HEAD `724225c` 全量重校并逐项标注"已修/未修"（U-68③ 的回填/有界/计数**已实现**：`services.rs:459-503`+`:550-599`+`:207`）；② **P2** 缺测行落库映射裁定 = **`telemetry.value` 可空化 + 写 `NULL`**（`quality = NoData`），含**幂等迁移**与 5 处代码点，与 PRD R-11.2-E「不得写入 0」字面一致（否决"写 0.0 + quality""不产行"两案）；③ **P3** §9.3 由"期望接缝"改为「**与已合并实现的对接说明 + 残留缺口**」：FLS-01/02/05、FLS-03①③ 已交付（逐条实现行号），**FLS-03②（`major` 事件）与 FLS-04（采集路径 await）未交付**并给落地方案（core-bin 健康巡检读增量 → `AlertFeed`；`tokio::spawn(flush_batch)`）；原 `TelemetrySink`/`OfferOutcome` 草案**未被采用**（接口形态订正表）；④ **P4** 缓冲上界统一为 **`10_000`** 并说明依据（10 个满批 / ≈1 MB），登记与 PRD R-11.5-A2「2 × batch_capacity」的偏差（Q-9/C-3）；`storage:` 段仍只含三键；⑤ **P5** `quality_from_point_quality` **改落 `mupc-core-bin/src/quality_map.rs`**（避免新增 `storage → data-processing` 边，D-8）。另：§9.6 装配点补"`telemetry.value` 可空化"条目；§9.7 新增 GRD-09（迁移幂等）、GRD-04 改断言 `value IS NULL`、FLS 行逐条标注状态；§9.9 新增 Q-7/Q-9/C-3/C-4。**未改**：PRD、`storage`/`core-bin` 代码、既有门禁标记、§4/§6/§7 各口径 |
| v1.2 | **新增 §9 增量设计（U-69 / U-67 / U-68③ / U-70）**（对应 03 PRD §11，`[REVIEWED: PASS: 2026-09-23]`）：① **§9.1 总表 1 分钟聚合**——聚合点裁定为 `mupc-storage::GridAggregator`（`core-bin` 只做样本转发 + tick，理由：装配层无单测环境而聚合时序逻辑必须可单测）；接口 `observe/tick/flush`；**通道表驱动**（18 均值 + `p_total`/`q_total` 极值 ×2 = **22 行/周期**；频率因"恒 50.0 无源"不落、电能无点表来源不落、S 不落）；时间戳 = 周期**起点**且为 `period_ms` 整倍；**无采样周期照样产行**（`quality = NoData`，严禁写 0）；跨重启**不回溯补产**（如实声明为已知边界）；落库复用 `telemetry` 窄表（`device_id = "grid_meter"`），新增 `storage::Quality` 枚举（`Good = 0` 保持既有写入值）。② **§9.2 `storage:` 段**——三键 schema（1000 / 5000 / 60000，**默认值 = 现实现 ⇒ 零行为变化**）、`validate_storage()` 与 `CoreConfig::validate()` 衔接（非法值**拒启动**并点名键）、两参数正交由**两个类型**保证。③ **§9.3 落库失败语义期望接缝**——`TelemetrySink::offer`（**非阻塞**，采集 p99 ≤ 10 ms）+ S-1～S-5 契约（回填 / `2×capacity` 上界丢最旧 / 丢弃必告警计数 / 告警按周期聚合 / 不阻塞采集），并**如实登记当前实现的 4 处差距**；**本章只交付接缝、实现由同期开发承接**。④ **§9.4** 保留期边界（`RetentionManager` 未装配，90 天是规格非现状）。⑤ **§9.5** 最新值入口归属确认（**唯一真源 = 01 设计 §9.1**，本章不复制）。⑥ §9.7 测试策略 ↔ PRD GRD/STG/FLS/CNS 逐条映射；§9.8 **D-1～D-7 冲突清单与取舍**；§9.9 **7 项待裁定 + 2 项文档订正**；§9.10 文档同步项（附录 C 的 `quality` 须随本章扩展）。**未改**：PRD、`storage`/`core-bin` 代码、§4.2/§4.3/§4.4 粒度、§6.1 保留期、§7.3 容量规划 |