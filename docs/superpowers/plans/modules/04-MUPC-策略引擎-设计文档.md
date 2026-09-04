# MUPC 策略引擎模块设计文档

> **文档定位：** 本文档记录实现级设计决策（架构、Rust 结构体/trait、状态机、配置结构、测试策略、文件组织）。需求级内容（功能描述、验收标准、性能指标）请参考 [04-MUPC-策略引擎-PRD](../specs/modules/04-MUPC-策略引擎-PRD.md)。

---

## 目录

1. [模块架构](#1-模块架构)
2. [台区储能治理策略](#2-台区储能治理策略)
3. [AI 指令安全校验](#3-ai-指令安全校验)
4. [AI 引擎集成](#4-ai-引擎集成)
5. [策略模式切换](#5-策略模式切换)
6. [接口定义](#6-接口定义)
7. [文件结构](#7-文件结构)
8. [错误处理](#8-错误处理)
9. [配置管理](#9-配置管理)
10. [测试体系](#10-测试体系)
11. [演进路线](#11-演进路线)

---

## 1. 模块架构

### 1.1 定位与职责

策略引擎（Strategy Engine）是 MUPC 通信管理模块的**本地决策核心**，对应 workspace crate `mupc-strategy-engine`。

**核心职责：**
- 提供单一兜底策略：台区储能治理（AI 失效时经核间下发分相 P/Q）
- 对 AI 引擎输出的指令进行安全校验（`AiCommandValidator`）
- 管理策略模式切换（AI 模式 / 本地兜底模式 / 基础模式）
- 通过消息总线接收遥测数据，输出控制指令
- 集成 AI 优化引擎

### 1.2 "AI 优先、本地兜底"机制

```
AI 引擎正常运行:
  AI 决策 → AiCommandValidator 安全校验 → 通过 → 指令下发
                                        → 不通过 → 降级至本地策略

AI 引擎失效:
  检测异常（心跳/状态码）→ 自动切换至本地策略引擎 → 发出告警
  → 本地策略接管控制 → AI 引擎恢复后自动切回 AI 模式
```

| 模式 | 决策源 | 校验方式 | 适用场景 |
|------|--------|----------|----------|
| AI 智能模式 | LSTM/TCN + MADDPG/PPO | AiValidator 安全校验 | 配置 `local_priority=false` 或 Web API 切换（默认关闭） |
| 本地兜底模式 | 台区储能治理 | 策略内置边界检查 | AI 失效/指令校验不通过 |
| **本地优先模式** | **台区储能治理（AI 旁路参考，不下发）** | 策略内置边界检查 | **部署默认**（`ai_engine.local_priority` 默认 true）；Web API `/api/v1/strategy-mode` 可切换 |
| 基础模式 | 无自动控制 | 手动操作 | 调试/维护 |

**本地优先模式（部署默认）**：`ai_engine.local_priority` 默认 `true`（代码 serde 默认 + 部署配置显式声明），开机即生效；也可经 Web API `/api/v1/strategy-mode` 运行时热切换。生效时 `dispatch_ai_decision` 直接执行本地台区储能治理策略（分相 P/Q 经核间下发）；AI 引擎仍加载、仍运行决策循环，但结果仅作旁路参考（记录日志，不下发核间指令）。需 AI 智能控制时置 `local_priority=false`。

### 1.3 模块依赖关系

```
mupc-strategy-engine
  ├── mupc-common              (MupcError, ErrorCode)
  ├── mupc-data-processing     (DataPackage, telemetry 类型)
  └── mupc-ai-engine           (ModelManager, FusedSystemState, ActionOutput, ModelStatus)
```

### 1.4 整体数据流

```
intercore (TCP/RJ45)
    │
    ▼
DataCollector → HighFrequencyTelemetry (1Hz)
    │
    ▼
Message Bus (tokio::sync::mpsc)
    │
    ▼
strategy-engine
    │
    ▼
AiCommandValidator (可插拔 AI 模型)
    │
    ▼
┌──────────────────────────────────────────────────────────────┐
│  AI→  p_ref + k_droop → IntercoreClient → 实时控制模块     │
│  本地策略→ 台区储能分相 P/Q → IntercoreClient → 实时控制模块 │
└──────────────────────────────────────────────────────────────┘
```

> **分发路径：** p_ref + k_droop 由 AI 引擎输出并通过核间通信下发至实时控制模块；台区储能治理策略（AI 失效兜底）经核间 V3 帧下发分相 P/Q 至实时控制模块。

### 1.5 策略 ID 分配

| 策略 | cmd_id | 说明 |
|------|--------|------|
| 台区储能治理 | 4 | 固定 ID（AI 失效兜底，经核间 V3 帧下发分相 P/Q） |
| 保留 | 5-10 | 供后续扩展策略使用 |

### 1.6 性能与可靠性

> 非功能性需求详见 [PRD §10](../specs/modules/04-MUPC-策略引擎-PRD.md)。本条记录设计层面的关键实现约束与**可验收量化标准**：

| 指标 | 量化标准 | 验证方法 | 当前实现评估 |
|------|----------|----------|----------|
| 策略决策延迟 | < 100ms（收到数据 → 输出指令） | 基准测试：`dispatch_ai_decision` 单次调用耗时 | ✅ 1s 决策循环 + 台区储能 60s 节流，单次控制 <1ms |
| 单实例内存 | < 10MB/策略实例 | `cargo bench` / 运行时 `mallinfo` | ✅ 纯计算无大对象，滑动窗 ≤5 点 |
| 并发评估 | 兜底策略与 AI 校验独立运行、互不阻塞 | 审查调用链（dispatch 内异步、无长阻塞） | ✅ dispatch_ai_decision 异步，best-effort 下发 |
| AI 失效检测 | < 1 个心跳周期（1s） | 心跳/状态码超时测试 | ⚠️ 依赖 AI 引擎心跳，策略侧 1s 轮询检测 |
| 模式切换时间 | < 50ms | `switch_mode` 基准 | ⚠️ 未单独基准，需补充 |
| 无单点故障 | 任一策略故障不影响其他 | 故障注入测试 | ✅ 兜底 best-effort，失败仅告警 |

> **注**：`策略决策延迟`/`内存`/`模式切换时间` 三条为 PRD §10.1/§10.2 硬指标，验收时须以基准测试/运行时采样确认达标；其余为设计约束。

---

## 2. 台区储能治理策略

> 整合自台区储能控制策略设计文档（方案A：分时状态机 + 共模/差模分解），作为策略引擎**兜底策略**，在 AI 引擎不生效时实现台区储能的台区治理目标。

### 2.1 定位与目标

台区配光伏 + 储能 + 三相四桥臂 PCS，当 AI 引擎失效（兜底模式）时，由本策略接管储能控制，实现三个治理目标（按优先级）：

1. **降低光伏返送**：缩短返送时长、压缩返送幅值（软目标，偶尔返送可接受）；
2. **降低三相电流不平衡度**：目标 <20%（电网公司口径 `(1 − MIN(Ia,Ib,Ic)/MAX(Ia,Ib,Ic)) × 100%`，幅值式）；
3. **提高功率因数**：大部分时间接近 1。

**目标优先级**：日终 SOC 清空（S4 硬约束）> 不平衡度 <20%（物理极限内尽力）> 降低返送（软目标）> PF（软目标）——受电池容量限制，"零返送"与"晚峰全削峰"不可同时完美达成，冲突时按此顺序妥协。

### 2.2 硬件与约束

| 项 | 取值 |
|---|---|
| PCS | 125kW，三相四桥臂，分相 PQ 独立可控 |
| 电池 | 60kW / 120kWh，SOC 运行带 10%~90%，日终回到 10% |
| 测量 | 台区总表（20s 延时），无本地实时测点；PCS/EMS 均无交采模块 |
| 控制 | 分钟级（T=60s）下发分相 P/Q 设定值 |
| 预测 | 纯实时，无光伏/负荷预测 |
| PCS 容量边界 | 每相/中线额定电流 190A，过载 1.1×长期（209A）/1.2×1min（228A）；总视在 125kVA |
| 分时 SOC 上限 | 18:00 前 SOC ≤70%（可标定），之后释放至 90% |

**SOC 系统约束**（全局硬约束，贯穿所有状态）：
- **运行带**：10%~90%；接近 90%（≥88%）充电线性降额至 90% 归零，接近 10%（≤12%）放电线性降额至 10% 归零；
- **分时上限**：18:00 前 SOC ≤70%（`soc_cap_day`），18:00 后释放至 90%（为晚峰放电留容量）；上限值用离线回放标定（扫 60/70/80%），平衡"白天消纳"与"晚间反送"；
- **日终清空（硬约束）**：日终必须回到 10%，允许晚间反送（线损管理优先）。前提是**电网允许晚间反送**——若电网禁止，S4 无法完全执行，须与电网公司确认边界。

### 2.3 架构（融入策略引擎）

```
台区总表(分相 P/Q/PF/U/I，20s) → DataPackage.ElectricalData.phase(扩展)
        ↓ (南向采集循环写入 set_latest_data)
AiIntegrator.run_fallback_strategies()          ← AI 失效时调用
        ↓
TaiStorageStrategy (兜底策略，持 Arc<Mutex<TaiControllerState>>)
        · 4 状态机 S1/S2/S3/S4 + 积分器(共模P/差模P/分相Q)
        · 每 60s 一个控制周期 → ControlCommand(phase_p_set/phase_q_set)
        ↓
IntercoreClient.send_tai_command()              ← 新增核间 V3 帧(分相 P/Q)
        ↓
实时控制模块 → 台区储能 PCS(分相 P/Q 设定)
```

**单一兜底策略**：策略引擎现仅保留台区储能治理策略作为本地兜底。AI 失效或指令校验不通过时，降级由该策略生成台区储能分相 P/Q，经核间 V3 帧下发至实时控制模块。原削峰填谷/需量控制/防逆流三策略已废弃（代码保留不编译）。

### 2.4 状态机（4 状态）

| 状态 | 时段/触发 | 主目标 | 共模 P 方向 |
|---|---|---|---|
| S1 光伏吸收 | 白天，`P_基线 < −P_abs_trig`（−2kW）且 SOC < 分时上限−滞回 | 吸收返送 | 充电 |
| S2 平段 | 其他时间 | 三相平衡 + PF | 0 |
| S3 高峰放电 | 任一时刻 `P_表 > P_dis_trig`（+30kW） | 放电供负荷 + 平衡 | 放电 |
| S4 日终清空 | 临近日终且 SOC>10% | 强制放电到 10% | 强制放电（允许晚反送） |

**S1 进出用「重构基线」判断**：S1 前馈吸收把净功率稳定拉到目标进口 +2kW，**净功率不再反映返送是否存在**。因此 S1 进出判断一律用**重构基线** `P_基线 = P_表净 + P_out[k−1]`（当前净功率 + 储能上周期输出，抵消储能自身出力效应）。S1 退出延迟到「P_基线 ≥ 退出阈值 **且** 储能已大步回 0」，防基线骤转受电时 S2 慢斜坡期间储能从电网取电。

**切换规则**：
- 触发带滞回（进入阈值 ≠ 退出阈值）；S1 退出在目标另一侧（进口 ≥+4kW 或 SOC 顶格），防"达目标即退→返送复现→重进"抖振；
- S3 退出 = 共模积分回零且进口 ≤目标 +5kW（削峰完成）；退出不可设在进口数值上（积分把进口恒压到 +5，固定阈值不可达）；
- S3 全天负荷触发（无时段门控）；优先级 S4 > S1 > S3 > S2；
- **failsafe**：总表数据超时（>150s）或坏数 → 冻结积分并斜坡回归 0，保持最后有效 Q，恢复后从 0 重新积分。

#### 2.4.1 一次控制周期怎么运行（通俗版）

每隔 `control_period_s=60` 秒，策略做一次决定：**看一眼台区总表，让储能干合适的事**。核心思路一句话：**台区缺电（受电）储能就待着；台区多电（光伏返送）储能就充电把多出来的电吸走；晚上储能把白天吸的电放出去、把电池清空**，同时顺手平衡三相、补无功。

**四个状态是干什么的（大白话）**：
- **S1 光伏吸收**（白天光伏发多了）：总表显示电在往电网倒灌（返送）→ 储能**充电**，把返送的电吸进电池，不让它白白送回电网；
- **S2 平段**（没大返送也没大负荷）：储能**待机**，共模出力归 0，只剩"平衡三相 + 补无功"两个软任务；
- **S3 高峰放电**（晚上负荷大）：总表受电功率超过 `p_dis_trig=30kW` → 储能**放电**，帮电网供一部分负荷，把高峰削下来；
- **S4 日终清空**（21:00 后）：把电池里剩的电**全放出去**，回到 10%，保证明天还有空间再吸收返送（允许少量晚反送，线损优先）。

**关键一问：怎么知道"返送了多少"？** 台区总表读到的其实是**净功率**（已经减掉了储能自己刚出的力）。策略用一个巧办法还原真实差额：**重构基线 = 总表净功率 + 储能上一次出力**。这样就知道"光伏和负荷的真实差"到底是多少，然后让储能出力 = 这个差额 − 2kW（留 2kW 给电网，防止吸收过头倒灌到反方向）。这就是 S1 的**前馈吸收**——一周期就能把返送吸干净，不用像老算法那样慢慢爬坡。

**用 7-04 数据走一遍（11:58→12:01，返送尖峰）：**

| 时刻 | 总表净功率 | 储能上次出力 | 重构基线返送 | 策略让储能 | 效果 |
|---|---|---|---|---|---|
| 11:58 | −15.9 | 0 | ≈15.9 | 充电 ≈16 | 返送被吸走，净功率回到 +2 |
| 11:59 | −50.3 | −18 | ≈50.3 | 充电 ≈52（大步斜坡一周期到位） | 净返送瞬间压住，只剩第一拍滞后 |
| 12:00 | −57.0 | −52 | ≈57 | 充电 ≈59（到 60 上限） | 净功率 ≈+2 |
| 12:01 | 基线返送回落到 53.7 | −60 | ≈53.7 | 自动降到 ≈55.7 | 吸收过头从电网取 6.3 → 收敛回 +2 |

**储能出力还有两个"辅助通道"**（跟共模充电同时进行，互不干扰）：
- **差模 P**（三相之间微调）：三相电流不平衡时，把充电/放电往电流大的相多分一点、电流小的相少分一点。三相之间倒来倒去，**总量不变、不额外耗电池**；
- **分相 Q**（无功）：哪相功率因数低了，就发/收无功把它补回接近 1。

所以每个周期最终下发的是**六个数字**：A/B/C 三相各自的有功设定 + 三相各自的无功设定（`phase_p_set` / `phase_q_set`），经核间 V3 帧给到台区储能 PCS。

### 2.5 控制律（三通道）

```
Q_i = clamp(Q_i[k−1] + s·K_q × Q_meter_i, −Q_i_max, +Q_i_max)   # 分相 Q（PF，积分式，常开）
P_st = move_toward(P_st[k−1], clamp(P_表净[k] + P_out[k−1] − P_目标进口, −P_cap, 0), s1_ff_step_kw)  # 共模 P（前馈吸收）
ΔP_i = clamp(ΔP_i[k−1] + K_diff×U_i×(I_i[k]−I_均值), −ΔP_max, +ΔP_max)  # 差模 P（三相平衡，积分式）
P_i = P_st/3 + ΔP_i    # 每相合成
Q_i = Q_i_补偿
```

**要点**：
- **必须积分式**：表计无功/电流包含 PCS 自身注入（作动器在测量回路内），比例式闭环特征根 +1，稳态只收敛一半；积分式稳态收敛到目标（`Q_表=0`、各相电流=均值）；
- **差模 P 零净能量**：三相增量 Σ=0（Σ(I_i−均值)=0），ΣΔP 恒为 0，不耗电池；电压不平衡时轻微残差（数据核实中位 <1%，可忽略）；
- **I_i 必须为带符号电流**（由分相有功 Pa 的符号或相角导出），不能用幅值——单相返送时幅值法方向相反、反向加重不平衡；
- **Q 通道与 ΔP 通道协作**：Q 先把各相 PF 校正到 1（使 S→P），ΔP 再平衡各相净有功/电流，不重复作用；
- **单相/两相返送（光伏随机接相）**：某相返送而总表仍净受电 → 不触发 S1，改由差模 P 拉向均值（零净能量、不耗电池）；总表净返送超阈值才叠加 S1 充电；
- **S1 前馈吸收**：net 闭环下净功率混入储能自身效应，且反馈积分爬坡**滞后一拍**——返送陡坡那一拍储能跟不上，造成峰值。改为**前馈直接吸收基线返送**：重构基线 `P_表基线 = P_表净[k] + P_out[k−1]`（当前净功率 + 储能上周期输出），前馈目标 `P_st目标 = P_表基线 − P_目标进口`，clamp `[−P_cap, 0]`（基线受电时 → 0 停充），以**大步斜坡** `move_toward`（步长 `s1_ff_step_kw`，默认 = `p_cap`，一周期到位）逼近。该目标自动覆盖全部场景，无需条件分支：持续返送 → 净恒 +2 无振荡；返送减小仍返送（净超 +2 超调）→ target 自动降载、**不停充**，把从电网取电压回 +2；基线受电 → clamp 0 停充；基线骤转受电 → 大步斜坡一周期回 0。回放验证见 §2.12。
- **s 符号**（Q 积分方向）：s=±1 以表计/PCS 约定为准，发散则翻转；投运前用小幅 Q 阶跃 + 分相注流核相（强制）。

### 2.6 容量仲裁（每相、每周期）

- 约束：每相/中线电流 ≤190A、总视在 ≤125kVA、总有功 ≤60kW（电池）；
- 裁剪顺序（按优先级）：先减 **Q**（PF，软目标）→ 再减 **差模 P**（不平衡）→ 最后减 **共模 P**（返送/能量，S4 不可剪）；仅 SOC 保护可剪共模 P；
- ΔP 裁剪后重归一化 ΣΔP=0（等比缩差模后均匀回补残差）；
- SOC 保护：充电 ≥90% 共模 P 剪 0、放电 ≤10% 共模 P=0；88%/12% 线性降额；
- 斜坡限速：S2/S3/S4 的 P_cm 与 ΔP_i 每周期变化 ≤6kW（`slope=6.0`）；S1 前馈大步斜坡不受 slope 限制，由 `s1_ff_step_kw`（默认 = p_cap）控制。

### 2.7 数据接入（DataPackage 扩展）

`ElectricalData` 新增分相字段（`mupc-data-processing/src/telemetry.rs`）：

```rust
/// 分相电气数据（台区总表）
#[derive(Debug, Clone, Default)]
pub struct PhaseElectricalData {
    pub voltage: [Option<f64>; 3],        // Ua/Ub/Uc (V)
    pub current: [Option<f64>; 3],        // Ia/Ib/Ic (A，带符号)
    pub active_power: [Option<f64>; 3],   // Pa/Pb/Pc (kW，含符号：>0 受电 / <0 返送)
    pub reactive_power: [Option<f64>; 3], // Qa/Qb/Qc (kVAr，含符号)
    pub cos_phi: [Option<f64>; 3],        // PFa/PFb/PFc
}

pub struct ElectricalData {
    // ... 现有三相总字段
    /// 分相数据（台区总表），None = 不可用
    pub phase: Option<PhaseElectricalData>,
}
```

- **台区总表数据源（U-26，投产必需）**：策略测量须来自台区总表分相数据。startup 装配「台区总表」RS485 Modbus 设备（`master_meter` 配置段：串口/从站地址/分相量寄存器映射 `reg_map`），按映射读保持寄存器 → 经 `mupc_data_processing::meter_regs` 解码（float32 / int32_scaled，Modbus 大端）→ 组装 `PhaseElectricalData`（电流方向由分相有功符号承载）→ `set_latest_data` 注入策略。`master_meter.enabled=true` 时总表 pkg 作为策略测量，南向模拟数据不再覆盖；
- 分相数据缺失时：策略按 failsafe 处理（积分冻结、斜坡回归 0）；
- `DataPackage` 构造处（`dataframe_to_datapackage` 等）同步更新，未填分相字段时 `phase=None`，不破坏现有调用方。**投产前提**：填真实总表点表（`reg_map` 各量起始寄存器）+ U-27 现场 Q 相序核验（`s_q_sign`）。

### 2.8 执行路径（核间协议 V3）

核间协议新增分相下发通道（`mupc-intercore`）：

```rust
// tcp_server.rs
/// 控制指令 JSON Payload v3.0（分相模式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCmdPayloadV3 {
    #[serde(rename = "frame_version")]
    pub frame_version: Option<u8>,          // = 3
    #[serde(rename = "p_ref")]
    pub p_ref: Option<f64>,                 // 兼容 v2 双参数（分相模式可为 None）
    #[serde(rename = "k_droop")]
    pub k_droop: Option<f64>,
    #[serde(rename = "phase_p_set")]
    pub phase_p_set: Option<[f64; 3]>,      // 分相有功 (kW)
    #[serde(rename = "phase_q_set")]
    pub phase_q_set: Option<[f64; 3]>,      // 分相无功 (kVAr)
    #[serde(rename = "ai_ready")]
    pub ai_ready: Option<bool>,
    #[serde(rename = "strategy_mode")]
    pub strategy_mode: Option<String>,
    #[serde(rename = "timestamp_ms")]
    pub timestamp_ms: Option<u64>,
}
```

- **复用 `IntercoreFrameType::ControlCmd` 帧类型**（帧定长 64 字节，JSON 数据区可承载分相六维），仅新增 payload 版本（`detect_version` 按 `frame_version` 区分 v1/v2/v3）；向后兼容，不新增帧类型；
- `IntercoreClient` 新增 `send_tai_command(&self, p: [f64;3], q: [f64;3], strategy_mode: &str)`，封装 V3 帧发送，复用现有持久连接。

### 2.9 带状态控制器设计

现有 `FallbackStrategy` 为无状态纯函数（`evaluate(&self, &DataPackage)`）。台区治理策略需跨周期积分状态（`st/P_st/Q_pcs/dP/...`），采用**策略实例内部持状态**方式，不改动 trait 签名：

```rust
// strategy-engine/src/tai_storage.rs
/// 台区储能控制器状态（4 状态机）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaiState {
    S1PvAbsorb,   // 光伏吸收
    S2Flat,       // 平段
    S3Peak,       // 高峰放电
    S4Clear,      // 日终清空
}

/// 台区总表单周期测量（控制律输入，含符号约定）
#[derive(Debug, Clone)]
pub struct MeterData {
    pub p: f64,               // 三相总有功 (kW，>0 受电 / <0 返送)
    pub q: f64,               // 三相总无功 (kVAr)
    pub pf: [f64; 3],         // 分相功率因数
    pub u: [f64; 3],          // 分相电压 (V)
    pub i: [f64; 3],          // 分相电流 (A，带符号)
    pub p_i: [f64; 3],        // 分相有功 (kW，含符号)
    pub q_i: [f64; 3],        // 分相无功 (kVAr，含符号)
}

/// 台区储能控制器跨周期状态（由策略实例持 Arc<Mutex> 保存）
pub struct TaiControllerState {
    pub st: TaiState,                 // S1/S2/S3/S4
    pub p_st: f64,                    // 共模出力 (kW)
    pub q_pcs: [f64; 3],              // 无功积分状态
    pub d_p: [f64; 3],                // 差模积分状态
    pub q_active: [bool; 3],          // 死区滞回锁存
    pub d_p_active: bool,             // 差模死区锁存
    pub q_last: [f64; 3],             // 最近有效 Q（failsafe 用）
    pub meter_buf: VecDeque<MeterData>, // 滑动滤波窗口（window_size=5 点）
    pub last_control_ts: u64,         // 上次控制时间戳（60s 节流）
}

pub struct TaiStorageStrategy {
    config: TaiStorageConfig,
    state: Arc<Mutex<TaiControllerState>>,   // 跨周期状态（纯函数 control() 的显式状态存储）
}
```

- `evaluate(&self, data)` 内：取锁 → `control()` 纯函数计算（跨周期状态读-算-写）→ 组装 `ControlCommand{ phase_p_set, phase_q_set }` → 释放锁；
- **控制周期节流**：`evaluate` 每周期被 `run_fallback_strategies` 调用；内部按 `timestamp` 判断距上次控制 ≥60s 才执行 `control()`，未到期则返回上次指令（避免 1s 决策循环与 60s 控制周期不匹配）；
- 首次周期初值：`st=S2, p_st=0, q_pcs=d_p=0, q_active=d_p_active=false, q_last=0, meter_buf=空`。

#### 2.9.1 单周期控制主流程伪代码（实现基准）

**接口**：`control(meter_data, soc, t_now, st, P_st, Q_pcs, dP, Q_active, dP_active, Q_last, meter_buf) → (P_A,Q_A, P_B,Q_B, P_C,Q_C, 更新后状态量)`（纯函数，见 §2.9）
输入含三相总/分相 P、Q（含符号）、PF、视在、电流幅值、电压；输出为 PCS 分相 P/Q 设定（正=放电/注入）。soc 用 0~1 小数。
**跨周期状态量**：`st`、`P_st`（共模出力）、`Q_pcs[3]`（无功积分）、`dP[3]`（差模积分）、`Q_active[3]`/`dP_active`（死区锁存）、`Q_last[3]`（最近有效 Q）、`meter_buf`（滑动滤波窗口）。
**首周期初值**：st=S2、P_st=0、Q_pcs=dP=0、Q_active=dP_active=False、Q_last=0、meter_buf=空。

```
# ---------- 0 常量（值见 §2.10）----------
P_abs_trig=2; P_dis_in=30                    # S1 返送 / S3 高峰触发阈值
S1_exit=4                                    # S1 退出阈值（重构基线≥+4 且储能已回0）
P_tgt={S1:+2, S3:+5}; P_cap=60; SLOPE=6      # 目标进口 / 电池功率 / 斜坡限速
S1_FF_STEP=60; Kp_S3=0.6; S3_MARGIN_LIMIT=True  # S1 前馈大步斜坡 / S3 增益 / S3 裕度限幅
K_diff=0.4; K_q=0.4                          # 差模/无功积分增益
DP_max=40; Q_i_max=30                        # 差模上限 / 无功上限
I_rated=190; S_rated=125                     # 电流 / 视在限
SOC_cap_day=0.70; SOC_hys=0.03; T_release=18:00  # 分时SOC上限 / 滞回 / 释放时刻
T_clr=[21:00, 23:30]; STALE_T=150            # S4 清空时段 / failsafe 超时

def control(meter, soc, t_now, st, P_st, Q_pcs, dP, Q_active, dP_active, Q_last, meter_buf):
    # 1 滤波与符号 --------------------------------------------------
    P, Pi, Qi, PFi, Ui, Ii_mag = sliding_avg(meter_buf, meter, window_size)  # 低通
    Ii = sign(Pi) * Ii_mag                              # 带符号电流（§2.5）
    Imean = mean(Ii)
    I_max = max(Ii_mag)
    unbal = 0 if I_max < 1 else (1 - min(Ii_mag)/I_max)*100  # 电网公司口径；除零守卫

    # 2 failsafe（§2.4）-----------------------------------------------
    if stale(meter) or bad(meter):
        P_st = move_toward(P_st, 0, SLOPE)              # 斜坡回归 0
        return per_phase(P_st, Q_last), (st, P_st, Q_pcs, dP, Q_active, dP_active, Q_last, meter_buf)

    # 3 状态机（§2.4，滞回锁存；优先级 S4>S1>S3>S2）-------------------
    # 重构基线：当前净功 + 上周期储能输出，抵消储能自身出力效应
    P_基线 = P + P_st
    SOC_cap = SOC_cap_day if t_now < T_release else 0.90
    P_force = clamp((soc-0.10)*120 / max(hours_to(T_clr[1]), 0.1), 0, P_cap)
    if max(Ui) > 235: P_force = min(P_force, max(P, 0))  # 电压越限保护：限夜间反送抬压
    if t_now >= T_clr[0] and soc > 0.10:                  # 进入 S4（未到 10% 则继续，P_force 限幅兜底）
        st = S4
    elif st == S1:                                        # 持至基线返送消失且储能回 0，或 SOC 顶格
        st = S1 if ((P_基线 < S1_exit or P_st < -1) and soc < SOC_cap) else S2
    elif st == S3:                                        # 持至积分回零且进口≤目标（削峰完成）才退
        st = S3 if (P_st > 0 or P > P_tgt[S3]) else S2
    elif P_基线 < -P_abs_trig and soc < SOC_cap - SOC_hys:
        st = S1
    elif P > P_dis_in:
        st = S3
    else:
        st = S2

    # 4 分相 Q（§2.5.1，积分式 + 死区锁存）-------------------
    Q = [0,0,0]
    for i in 0..2:
        if abs(PFi[i]) > 0.98: Q_active[i] = False       # 退出：PF 已好
        elif abs(PFi[i]) < 0.95: Q_active[i] = True      # 进入：PF 差
        if Q_active[i]:
            Q_pcs[i] = clamp(Q_pcs[i] + s*K_q*Qi[i], -Q_i_max, Q_i_max)  # 积分归零表计无功
            Q[i] = Q_pcs[i]
        else:
            Q_pcs[i] = move_toward(Q_pcs[i], 0, Q_i_max)  # 惰化：斜坡回归 0
            Q[i] = Q_pcs[i]

    # 5 共模 P（§2.5）----------------------------------------------
    if   st==S1: P_st = move_toward(P_st, clamp(P_基线 - P_tgt[S1], -P_cap, 0), S1_FF_STEP)  # 前馈吸收
    elif st==S2: P_st = move_toward(P_st, 0, SLOPE)      # 斜坡回归 0，防阶跃
    elif st==S3:                                          # 放电补到进口≈+5kW
        P_st = clamp(P_st + clamp(Kp_S3*(P - P_tgt[S3]), -SLOPE, SLOPE), 0, P_cap)
        if S3_MARGIN_LIMIT: P_st = min(P_st, max(P - P_tgt[S3], 0))  # 放电不超当前负荷裕度
    elif st==S4: P_st = move_toward(P_st, P_force, SLOPE)  # 斜坡逼近强制值
    P_st = soc_protect(P_st, soc)                        # ≥88% 充电降额/≤12% 放电降额；90%/10% 钳位

    # 6 差模 P（§2.5，积分式 + 死区锁存，零净能量）-----------
    if unbal < 15: dP_active = False
    elif unbal > 25: dP_active = True
    for i in 0..2:
        inc = K_diff * Ui[i] * (Ii[i] - Imean)
        if dP_active and abs(inc) > max(0.5, 0.05*abs(Pi[i])):   # 增量死区
            dP[i] = clamp(dP[i] + inc, -DP_max, DP_max)          # 积分：三相增量 Σ=0
        else:
            dP[i] = move_toward(dP[i], 0, DP_max)                # 惰化：斜坡回归 0

    # 7 指令合成（§2.5）--------------------------------------------
    Pcmd = [P_st/3 + dP[i] for i in 0..2]

    # 8 容量仲裁/裁剪（§2.6）---------------------------------------
    #   (a) ΔP 裁剪后重归一化 ΣΔP=0（否则 ΣP_i≠P_st，破电池 60kW 总量限）
    #   (b) 裁剪顺序：①Q → ②差模P → ③共模P（S4 不可剪）
    #   (c) 约束：每相/中线电流 ≤190A、总视在 ≤125kVA、总有功 ≤60kW
    #   (d) SOC 保护：充电≥90% 共模P剪0、放电≤10% 共模P=0
    Pcmd, Q = arbitrate(Pcmd, Q, st, P_st, I_rated, S_rated, P_cap, SLOPE)

    Q_last = Q                                            # 更新最近有效 Q
    return (Pcmd[0],Q[0], Pcmd[1],Q[1], Pcmd[2],Q[2]), (st, P_st, Q_pcs, dP, Q_active, dP_active, Q_last, meter_buf)
```

**辅助函数语义**：
- `sliding_avg(buf, meter, n)`：meter 推入窗口缓冲，返回 n 点均值（缓冲未满时直接取当前值）；
- `sign(x)`=符号；`hours_to(t)`=距 t 时刻的小时数；`move_toward(x,t,s)`=x 每周期向 t 最多移动 s；`clamp(x,lo,hi)`=限幅；
- `soc_protect(P_st,soc)`=降额/钳位：soc≥88% 充电线性降额至 90% 归零；soc≤12% 放电线性降额至 10% 归零；
- `per_phase(P_st,Q)`=把共模 P_st 均分三相与 Q 合成分相指令元组（failsafe 用）；
- `arbitrate(Pcmd,Q,st,P_st,...)`=§2.6：统一限 ΔP 斜坡 → 逐相电流 ≤190A / 中线 ≤190A / 总视在 ≤125kVA 钳位 → 按 ①Q ②差模P ③共模P 顺序裁剪 → **ΔP 重归一**（裁剪后若 `resid=ΣΔP≠0`，`dP[i] −= resid/3` 均匀回补）→ 共模 P 总量 ≤60kW。中线电流 `I_N=|Σ_i I_i∠θ_i|`（用 data_rule 相角列计算）≤190A。可选 PF 地板：若启用 `|PF_i|≥PF_floor`，差模 P 先让保 Q。

### 2.10 配置（TaiStorageConfig）

| 参数 | 值 | 作用 |
|---|---|---|
| `control_period_s` | 60 | 控制周期 |
| `p_abs_trig` / `p_dis_trig` | 2.0 / 30.0 (kW) | S1 返送 / S3 高峰触发阈值 |
| `s1_exit` / `p_tgt_s1` / `p_tgt_s3` | 4 / 2 / 5 (kW) | S1 退出阈值 / 目标进口 |
| `p_cap` / `slope` | 60 / 6.0 | 电池功率上限 / 斜坡限速 (kW/周期) |
| `kp` / `k_diff` / `k_q` | 0.6 / 0.4 / 0.4 | 共模/差模/无功积分增益 |
| `dp_max` / `q_i_max` | 40 / 30 | 差模上限 (kW/相) / 无功上限 (kVAr/相) |
| `i_rated` / `s_rated` | 190 / 125 | 每相·中线电流限 (A) / 总视在限 (kVA) |
| `soc_cap_day` / `soc_hys` | 0.70 / 0.03 | 分时 SOC 上限 / 滞回 |
| `t_release_secs` / `t_clear_start_secs` / `t_clear_end_secs` | 18:00 / 21:00 / 23:30 | 分时上限释放 / S4 清空时段 |
| `s4_limit_margin_kw` | 0.0 | S4 清空限幅裕度（>0 时 P_强制≤P_表+裕度，防夜间过送；0=不限幅） |
| `s3_margin_limit` | true | S3 放电裕度限幅（`p_st=min(p_st,(P_表−p_tgt_s3).max(0))`，防负荷回落过冲返送） |
| `s1_ff_step_kw` | 60 | S1 前馈大步斜坡步长 (kW/周期)，默认 = p_cap（一周期到位） |
| `window_size` | 5 | 滑动滤波窗口（点数） |
| `battery_capacity_kwh` | 120 | 电池容量 |

初始值为占位，最终值在离线回放（§2.12）中标定（分时 SOC 上限扫 60/70/80%、P_abs_trig 扫 5/10/15/20、Kp 灵敏度）。当前标定值：`p_abs_trig=2.0`、`slope=6.0`、`kp=0.6`（S3 放电）、`s3_margin_limit=true`/`s4_limit_margin_kw=0.0`（S3/S4 防过送）、`s1_ff_step_kw=60`（S1 前馈大步斜坡）。

#### 2.10.1 配置参数怎么用（结合数据）

下面把每个参数"在什么时候用、值影响什么"讲清楚，都用 7-04 实测数据举例。

**① 状态机触发（决定储能"动不动"）**

| 参数 | 值 | 怎么用 |
|---|---|---|
| `p_abs_trig` | 2.0 kW | **进 S1 的门槛**：重构基线返送超过 2kW 就进 S1 充电吸收。例：11:58 基线返送 15.9kW（>2）→ 进 S1。标定前是 10kW，调成 2kW 后能捕获更小返送（7-04 返送时长 9.6%→5.6%） |
| `s1_exit` | 4 kW | **出 S1 的门槛**：基线受电达到 4kW **且储能已停充**才退出 S1。退出在目标另一侧（目标 +2，退出 +4），防止"刚吸到目标就退→返送复现→又进"抖振 |
| `p_dis_trig` | 30 kW | **进 S3 的门槛**：总表受电超过 30kW 进 S3 放电削峰。例：7-04 晚峰 77.6kW → S3 放电削峰 |

**② 目标进口（储能要把净功率停在哪）**

| 参数 | 值 | 怎么用 |
|---|---|---|
| `p_tgt_s1` | 2 kW | S1 吸收返送时，让净功率停在"受电 2kW"（留 2kW 给电网）。例：基线返送 50.3kW，储能充 50.3−2=48.3kW → 净功率正好 +2。**这 2kW 允许储能从电网少量取电，避免吸收过头** |
| `p_tgt_s3` | 5 kW | S3 削峰时，让净功率停在"受电 5kW"（保持受电裕度，不反送） |

**③ 功率边界（储能能出多大力）**

| 参数 | 值 | 怎么用 |
|---|---|---|
| `p_cap` | 60 kW | 储能最大充/放电功率（电池额定）。例：基线返送 57kW，储能最多充 60kW，净功率 = 57−60 = +3（受电） |
| `s1_ff_step_kw` | 60 | **S1 前馈大步斜坡**：每周期最多改变 60kW，所以从任何值到目标一周期到位。例：11:59 储能从 −18 直接跳到 −52（一步吸收 50kW 返送），这是峰值压降的关键（47.9→32.5kW） |
| `slope` | 6 kW/周期 | **S2/S3/S4 斜坡限速**：这些状态每周期最多变 6kW（平缓、防过调）。S1 不受此限（用大步斜坡） |
| `dp_max` | 40 kW/相 | 差模 P 每相最多调 40kW（受 190A 电流限制折算） |
| `q_i_max` | 30 kVAr/相 | 每相无功补偿最多 30kVAr |

**④ 积分增益（响应快慢）**

| 参数 | 值 | 怎么用 |
|---|---|---|
| `kp` | 0.6 | S3 放电的积分增益（只 S3 用；S1 前馈不需要）。越大响应越快，过大易超调 |
| `k_diff` | 0.4 | 差模 P 积分增益：三相不平衡时，每周期按"该相电流−三相均值"积分，把电流大的相拉下来 |
| `k_q` | 0.4 | 分相 Q 积分增益：把各相表计无功积分归零（PF→1） |

**⑤ 安全与 SOC 边界（保护设备）**

| 参数 | 值 | 怎么用 |
|---|---|---|
| `i_rated` | 190 A | 每相/中线电流上限，超了仲裁裁剪（先减 Q → 再减差模 → 最后减共模） |
| `s_rated` | 125 kVA | 总视在上限，超了等比缩小差模 |
| `soc_cap_day` | 0.70 | **18:00 前 SOC 上限 70%**：防止白天把电池充太满，导致晚上被迫大功率反送清空。例：7-04 SOC 峰值 61.8%（<70% 未触顶） |
| `soc_hys` | 0.03 | SOC 滞回：进 S1 需 SOC < 70%−3%=67%，防临界点反复进出 |
| `s3_margin_limit` | true | S3 放电不超当前负荷裕度：`p_st = min(p_st, P_表−p_tgt_s3)`，负荷快速回落时即时跟随，杜绝过冲返送 |
| `s4_limit_margin_kw` | 0.0 | S4 清空限幅裕度：0 = 满额清空（允许晚反送）；>0 则 `P_强制 ≤ P_表+裕度`（防夜间过度反送，牺牲部分清空） |

**⑥ 时序与滤波**

| 参数 | 值 | 怎么用 |
|---|---|---|
| `control_period_s` | 60 | 控制周期（决定多久做一次决定） |
| `t_release_secs` | 18:00 | 分时 SOC 上限释放时刻：18:00 后 `soc_cap_day` 放宽到 90%，为晚峰放电留容量 |
| `t_clear_start_secs` / `t_clear_end_secs` | 21:00 / 23:30 | S4 清空时段：21:00 起强制放电到 10%，23:30 为达标目标 |
| `window_size` | 5 | 滑动平均窗口（5 点≈5 分钟）：平滑 20s 表计延时噪声，只响应慢变分量，忽略秒级瞬态 |
| `battery_capacity_kwh` | 120 | 电池容量，用于 SOC 积分与 S4 `P_强制` 计算 |

**参数之间的配合关系（直观）**：`p_abs_trig`(2) 决定"什么时候开始吸" → `p_tgt_s1`(2) 决定"吸到哪停" → `s1_ff_step_kw`(60) 决定"一周期能吸多快" → `p_cap`(60) 决定"最多吸多少"。白天这条链把返送吸掉；`p_dis_trig`(30) → `p_tgt_s3`(5) → `slope`(6) 这条链在晚峰放电削峰；`soc_cap_day`(0.70) 在中间平衡"白天吸多少"与"晚上放多少"。

### 2.11 集成点（AiIntegrator）

- `AiIntegrator` 新增字段 `tai_storage: Arc<Mutex<TaiStorageStrategy>>`；
- `set_tai_storage_strategy()` 注入（startup 装配时创建并注入）；
- `run_fallback_strategies()` 中追加：调用 `tai_storage.evaluate(&data)`，产出分相指令 → 经 `intercore_client.send_tai_command()` 下发（若未注入核间客户端则跳过并记录警告）。

### 2.12 离线回放验证

**工具形态**：独立二进制 `mupc-tai-replay`（workspace 下新增 bin crate 或 `tests/` 集成测试），读取历史 data_rule 数据逐周期回放，输出 KPI 报告。

**数据源**：`E:/MUPC2/数据/2026_06_27_data_rule.xlsx`（低负荷日）、`2026_07_04_data_rule.xlsx`（高负荷日）。xlsx 解析用 `calamine` crate（新增 dev-dependency），列含 A/B/C 相有功/无功（含符号）、功率因数、电压、电流、调控值。

**回放流程**：
1. 读 xlsx → 按 60s 步进对齐时间戳，缺段跳过；
2. 逐周期调用 `TaiStorageStrategy` 的纯函数 `control()`（跨周期状态由回放循环保存回传，与运行时 Mutex 等价）；
3. 累计 SOC（±120kWh）；初值 SOC=0.50；
4. 统计 KPI：返送时长/幅值（vs 无储能基线）、不平衡度 <20% 达标时长占比（目标 ≥80%）、PF 接近 1 占比、SOC 日终回到 10% 且始终在 10~90% 带内、晚反送量。

**边界用例**：中午大返送+SOC 快满、S1 到分时上限后返送仍持续（无 S1↔S2 振荡）、晚峰负荷小（S4 晚反送）、返送+不平衡同时（仲裁顺序）、单相返送、21:00 S3→S4 交接、通信故障注入、数据缺口、SOC 误差 ±3%。

**回放报告**：打印每日 KPI 表 + 参数灵敏度（扫分时 SOC 上限、P_abs_trig、Kp），供标定初始值。

**net 反馈模型**：回放由 gross（开环）改为 **net 反馈模型**——储能输出反馈到表计测量（`p_i_net = p_i_base − last_p_out`），与运行时总表实测净功率一致；KPI/SOC 用生效间隔输出记账。S1 前馈的重构基线 `P_基线 = P_净 + P_out[k−1]` 正依赖该模型。

**回放验证结果（net 闭环模型，SOC 初值 0.50）：**

| 日 | 指标 | 基线（无储能） | 控制后（当前前馈） |
|---|---|---|---|
| 7-04 | 返送时长 | 16.5% | **5.6%** |
| 7-04 | 返送峰值(kW) | 57.0 | **32.5** |
| 7-04 | 返送能量(kWh) | 58.0 | **8.5** |
| 7-04 | SOC 日终 | — | 10.0% |
| 6-27 | 返送时长 | 6.2% | **2.7%** |
| 6-27 | 返送峰值(kW) | 25.3 | **10.4** |
| 6-27 | 返送能量(kWh) | 10.0 | **2.3** |
| 6-27 | SOC 日终 | — | 10.2% |

两日受电峰值不劣化（7-04 77.6 / 6-27 41.5 kW）；SOC 日终清空不变。前馈把 7-04 返送峰值压至 32.5kW（≈第一拍滞后极限）。峰值压降受陡坡（≈39kW/min）超过 60s 控制周期能力所限，储能侧到顶，压真峰需光伏限功率联动。

**已知局限**：回放对控制后三相不平衡度/PF 用基线近似（无相角/电流重分布模型），差模 P 与分相 Q 通道效果未建模，待实机/带相角仿真验证。

### 2.13 测试体系

| 测试文件 | 用例数 | 测试内容 |
|----------|--------|----------|
| `tai_storage_test.rs` | ~15 | 状态机切换（S1~S4 进入/退出/滞回）、积分收敛（共模/差模/Q）、零净能量 ΣΔP=0、容量仲裁裁剪顺序、failsafe 数据超时、控制周期节流 |
| `mupc-tai-replay` | 集成 | 6-27/7-04 回放 KPI 断言（不平衡 <20% 达标时长 ≥80% 等） |
| 核间 V3 帧 | ~4 | `ControlCmdPayloadV3` 序列化/反序列化、版本检测（v1/v2/v3）、`send_tai_command` 帧组装 |

### 2.14 依赖清单（实现前确认）

1. 台区总表实时接口提供分相 Q（含符号）与分相 PF（data_rule 字段已确认）；
2. PCS 通信接受分相 P/Q 设定值（已确认）；实时控制模块能转发分相 P/Q 到 PCS（**需与实时控制模块协议确认 V3 帧对接**）；
3. PCS 每相/中线电流限值、总视在额定（已确认：190A/125kVA）；
4. 状态机时段参数初值（已用 6-27/7-04 data_rule 负荷曲线标定，P_dis_trig=30kW、T_清空 21:00/23:30）；
5. 电池充/放电功率限值 60kW（已确认）；
6. 通信协议细节：设定值下发瞬时生效或斜坡生效、超时/失败响应、时钟同步；现场核相流程（强制）。

### 2.15 电压越限与三相不平衡补偿（原接口预留，已由台区储能实现）

### 5.1 概述

通过电池逆变器提供无功功率支撑，改善台区电压质量和三相不平衡度。当前为**接口预留**，完整的决策逻辑和实现后续补充。

### 5.2 已预留接口

`ControlCommand` 中已包含以下字段，供无功补偿策略使用：

| 字段 | 类型 | 用途 | 范围 |
|------|------|------|------|
| `q_batt_set` | `Option<f64>` | 无功由实时控制模块闭环调节 | - |
| `phase_compensation` | `Option<[f64; 3]>` | A/B/C 三相分相补偿系数 | 各相独立设置 |

### 5.3 计划策略

| 策略 | 触发条件 | 动作 |
|------|----------|------|
| 电压越限补偿 | 电压超出额定范围 ±7%（或 ±10%，按国标要求） | 电池吸收/发出无功 |
| 三相不平衡补偿 | 三相电流不平衡度 > 15% | 分相无功补偿 |

---

## 3. AI 指令安全校验

### 3.1 概述

`AiCommandValidatorImpl` 作为 AI 引擎与执行层之间的**安全闸门**，对所有 AI 决策指令进行校验。校验不通过时自动降级至本地兜底模式。

### 3.2 架构

- **trait**: `AiCommandValidator`（定义于 `strategies.rs`）
- **实现**: `AiCommandValidatorImpl`（定义于 `ai_validator.rs`）
- **可插拔 AI 模型**: `AiModel` trait（定义于 `ai_validator.rs`）
- **默认模型**: `MockAiModel`（模拟预测逻辑）

### 3.3 接口定义

```rust
/// AI 指令校验器 Trait（可插拔）
#[async_trait]
pub trait AiCommandValidator: Send + Sync {
    async fn validate(&self, cmd: &ControlCommand) -> ValidationResult;
    fn name(&self) -> &str;
}

/// AI 模型 Trait（可插拔，可替换为真实预测模型）
pub trait AiModel: Send + Sync {
    fn predict(&self, input: &ModelInput) -> ModelOutput;
}

/// 模型输入
pub struct ModelInput {
    pub battery_soc: f64,     // 电池 SOC（0.0-1.0）
    pub pv_power: f64,        // 光伏功率（kW）
    pub load_power: f64,      // 负荷功率（kW）
    pub grid_power: f64,      // 电网功率（kW）
}

/// 模型输出
pub struct ModelOutput {
    pub recommended_p_batt: f64,  // 推荐电池功率（kW）
    pub confidence: f64,          // 置信度（0.0-1.0）
}
```

### 3.4 校验规则

```rust
pub fn validate_sync(&self, cmd: &ControlCommand) -> ValidationResult {
    // 1. 无模型时默认通过
    if self.model.is_none() {
        return ValidationResult::valid();
    }

    // 2. 只校验功率调节命令，开关命令直接通过
    if cmd.cmd_type != CommandType::PowerRegulation {
        return ValidationResult::valid();
    }

    // 3. 无 p_ref 时默认通过（双参数模式）
    let p_ref = match cmd.p_ref {
        Some(p) => p,
        None => return ValidationResult::valid(),
    };

    // 4. 调用 AI 模型预测比较
    let model_output = model.predict(&model_input);
    let diff = (p_batt - model_output.recommended_p_batt).abs();
    if diff > 10.0 && model_output.confidence < 0.7 {
        return ValidationResult::invalid("...");
    }

    ValidationResult::valid()
}
```

### 3.5 MockAiModel 模拟逻辑

```rust
impl AiModel for MockAiModel {
    fn predict(&self, input: &ModelInput) -> ModelOutput {
        let recommended_p_batt = if input.battery_soc > 0.8 {
            (input.pv_power - input.load_power).max(0.0)   // SOC 高，优先放电
        } else if input.battery_soc < 0.2 {
            (input.pv_power - input.load_power).min(0.0)   // SOC 低，优先充电
        } else {
            0.0                                              // SOC 中等，待机
        };
        ModelOutput { recommended_p_batt, confidence: 0.5 }
    }
}
```

### 3.6 降级流程

```
AiCommandValidator.validate(cmd)
  ├── 校验通过 → 指令继续下发
  └── 校验不通过 →
        ├── 记录告警日志
        ├── 丢弃 AI 指令
        ├── 切换至本地兜底模式
        └── FallbackStrategy.evaluate(data) 生成兜底指令
```

### 3.7 测试覆盖

| 测试用例 | 文件 | 验证点 |
|----------|------|--------|
| `test_mock_ai_model_predict_high_soc` | `ai_validator_test.rs` | SOC 高时推荐放电 |
| `test_mock_ai_model_predict_low_soc` | `ai_validator_test.rs` | SOC 低时推荐充电 |
| `test_mock_ai_model_predict_mid_soc` | `ai_validator_test.rs` | SOC 中等时推荐待机 |
| `test_validator_without_model` | `ai_validator_test.rs` | 无模型时校验默认通过 |
| `test_validator_with_model` | `ai_validator_test.rs` | 有模型时调用 predict 校验 |
| `test_validator_switch_command_passthrough` | `ai_validator_test.rs` | 开关命令直接通过 |
| `test_validator_name` | `ai_validator_test.rs` | 校验器名称返回正确 |
| `test_validator_async_validate` | `ai_validator_test.rs` | 异步 validate 接口正常 |

---

## 4. AI 引擎集成

### 4.1 概述

`AiIntegrator` 负责管理 AI 模型生命周期，提供 AI 决策接口。位于 `ai_integration.rs`。

### 4.2 结构体定义

```rust
pub struct AiIntegrator {
    model_manager: Arc<RwLock<Option<Arc<ModelManager>>>>,
    status: Arc<RwLock<ModelStatus>>,
    /// 核间通信客户端（p_ref/k_droop 双参数 + 台区储能分相 V3 帧下发）
    intercore_client: Option<Arc<IntercoreClient>>,
    /// 双参数降级缓存（通信中断时使用）
    last_valid_p_ref: RwLock<Option<f64>>,
    last_valid_k_droop: RwLock<Option<f64>>,
    /// 降级状态（AI 失效触发）
    fallback_active: RwLock<bool>,
    /// 最新遥测数据（南向采集循环写入，供兜底策略 evaluate）
    latest_data: Arc<RwLock<Option<DataPackage>>>,
    /// 台区储能治理策略（唯一兜底策略）
    tai_storage: Option<Arc<TaiStorageStrategy>>,
    /// 本地策略优先模式：true 时 AI 旁路、控制以本地台区储能策略为准
    local_priority: RwLock<bool>,
}
```

### 4.3 关键方法

| 方法 | 说明 | 异步 |
|------|------|------|
| `new()` | 创建 AI 集成器，初始状态为 Unloaded | 否 |
| `initialize(config)` / `set_model_manager()` | 加载/注入 AI 模型 | 是 |
| `set_intercore_client()` | 注入核间通信客户端 | 否 |
| `set_tai_storage_strategy()` | 注入台区储能治理策略 | 否 |
| `set_local_priority()` / `is_local_priority()` | 设置/查询本地优先模式 | 是 |
| `set_latest_data()` | 写入最新遥测（南向采集循环调用） | 是 |
| `dispatch_ai_decision()` | 决策主循环：部署默认本地优先（直接走本地台区储能，AI 旁路）；`local_priority=false` 时 AI 决策（失败/校验不过降级本地） | 是 |
| `is_ready()` / `status()` | 查询 AI 就绪/状态 | 是 |

### 4.4 状态管理

| AiIntegrator 状态 | 策略模式 | 说明 |
|--------------------|----------|------|
| `Unloaded` | Fallback / Basic | 模型未加载，使用兜底策略 |
| `Loading` | Fallback | 模型加载中，暂用兜底策略 |
| `Ready` | Intelligent | 模型就绪，AI 决策 + Validator 校验 |
| `Error` | Fallback | 模型异常，自动降级 |

### 4.5 数据集成

```rust
// strategy-engine 通过 AiIntegrator 集成 AI 引擎
strategy-engine ←→ AiIntegrator ←→ ai-engine::ModelManager
                                  ├── 决策接口 → ActionOutput (p_ref, k_droop)
                                  └── 状态管理 → ModelStatus

数据流（部署默认本地优先；AI 优先需 `local_priority=false`）：
1. LSTM/TCN 时序预测（光伏出力/负荷）
2. MADDPG/PPO 基于预测结果决策，输出 2 维动作（p_ref, k_droop）
3. AiCommandValidator 校验 AI 指令安全性
4. AI 指令分发：
   - p_ref + k_droop → IntercoreClient → 实时控制模块（闭环下垂控制）
5. AI 失效/校验不通过 → 降级本地兜底：
   - 台区储能治理(TaiStorageStrategy) → IntercoreClient.send_tai_command()（核间 V3 帧）→ 实时控制模块 → 台区储能 PCS（分相 P/Q）

本地优先模式（`local_priority=true`）：
- `dispatch_ai_decision` 开头判断 `local_priority`，为 true 时直接走本地台区储能治理策略（分相 P/Q 经核间下发）
- AI 引擎仍加载、仍运行 `full_decision_cycle()`，但结果仅作旁路参考（debug 日志），**不下发核间指令**
- 通过 YAML 配置 `ai_engine.local_priority` 或 Web API `/api/v1/strategy-mode` 运行时切换
```

---

## 5. 策略模式切换

### 5.1 模式定义

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrategyType {
    Basic,         // 基础模式 - 无自动控制
    Intelligent,   // 智能模式 - AI 引擎决策
    Fallback,      // 兜底模式 - 本地策略引擎
}
```

### 5.2 切换触发器

| 当前模式 | 切换条件 | 目标模式 |
|----------|----------|----------|
| 本地优先 | **部署默认**（`ai_engine.local_priority=true` 启动） | 本地优先（AI 旁路，分相 P/Q 下发） |
| Intelligent | AI 引擎心跳超时 / 状态异常 | Fallback |
| Intelligent | AiValidator 校验不通过 | Fallback |
| Fallback | AI 引擎恢复（status == Ready） | Intelligent |
| Any | 运维人员手动切换 | Basic / Intelligent / Fallback |
| Basic | 运维人员手动切换 | Intelligent / Fallback |
| 本地优先 | Web API `PUT /api/v1/strategy-mode`（local_priority=false）或配置 false 重启 | AI 智能（恢复 AI 控制） |

> **注**：`Intelligent/Fallback` 间的自动切换仅发生在 AI 控制模式（`local_priority=false`）。部署默认本地优先模式下 AI 旁路运行，其决策结果不下发，故不触发上述自动降级/恢复路径。

### 5.3 核间通信信号

策略模式通过 TCP 帧中的 `strategy_mode` 字段同步给实时控制模块：

| 值 | 模式 | 说明 |
|----|------|------|
| 0 | 基础模式 | Basic |
| 1 | 智能模式 | Intelligent |
| 2 | 兜底模式 | Fallback |

同时，`ai_ready` 字段（u8, 0/1）指示 AI 引擎可用状态。

---

## 6. 接口定义

### 6.1 FallbackStrategy Trait（strategies.rs）

```rust
#[async_trait]
pub trait FallbackStrategy: Send + Sync {
    /// 评估数据并生成控制命令
    async fn evaluate(&self, data: &DataPackage) -> Result<ControlCommand, MupcError>;

    /// 获取策略类型
    fn strategy_type(&self) -> StrategyType;

    /// 获取策略名称
    fn name(&self) -> &str;
}
```

当前唯一兜底策略实现此 trait 的三个方法：
- `evaluate()` — 根据遥测数据生成控制命令
- `strategy_type()` — 均返回 `StrategyType::Fallback`
- `name()` — 返回策略名称字符串

### 6.2 ControlCommand 结构体

```rust
#[derive(Debug, Clone)]
pub struct ControlCommand {
    pub cmd_id: u16,                          // 命令 ID（4-台区储能治理）
    pub cmd_type: CommandType,                // 命令类型
    pub p_batt_set: Option<f64>,             // 电池有功设定 (kW)，AI 指令校验与台区储能共模输出共用
    #[deprecated] pub q_batt_set: Option<f64>, // 无功由实时控制模块闭环调节
    pub phase_compensation: Option<[f64; 3]>, // 分相补偿系数 [预留]
    pub start_stop: Option<bool>,            // 启停命令
    pub priority: u8,                        // 优先级（0-3）
    pub phase_p_set: Option<[f64; 3]>,       // 台区储能分相有功设定 (kW) [A/B/C]，正=放电/注入，仅由台区储能治理策略设置
    pub phase_q_set: Option<[f64; 3]>,       // 台区储能分相无功设定 (kVAr) [A/B/C]，仅由台区储能治理策略设置
}
```

> **分相设定字段：** `phase_p_set` / `phase_q_set` 为台区储能分相有功/无功设定，仅由台区储能治理策略（`TaiStorageStrategy`，见 §2）设置。设定值经核间 V3 帧下发到实时控制模块，由其转发至台区储能 PCS（三相四桥臂分相 PQ 独立可控）。

### 6.3 CommandType 枚举

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandType {
    SwitchControl,      // 开关控制
    PowerRegulation,    // 功率调节
    ChargeDischarge,    // 充放电控制
}
```

### 6.4 AiCommandValidator Trait

```rust
#[async_trait]
pub trait AiCommandValidator: Send + Sync {
    /// 校验 AI 命令
    async fn validate(&self, cmd: &ControlCommand) -> ValidationResult;
    /// 获取校验器名称
    fn name(&self) -> &str;
}
```

### 6.5 ValidationResult 结构体

```rust
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,                              // 是否通过
    pub message: String,                          // 错误消息
    pub suggested_command: Option<ControlCommand>,  // 建议命令
}

impl ValidationResult {
    pub fn valid() -> Self;                       // 创建通过结果
    pub fn invalid(message: impl Into<String>) -> Self;  // 创建失败结果
}
```

### 6.6 AiCommand 结构体

```rust
#[derive(Debug, Clone)]
pub struct AiCommand {
    pub cmd_id: u16,           // 命令 ID
    pub p_set: f64,            // 有功设定值 (kW)
    pub q_set: f64,            // 无功设定值 (kVar)
    pub priority: u8,          // 优先级
    pub raw_command: String,   // 原始命令 JSON
}
```

---

## 7. 文件结构

```
mupc/crates/strategy-engine/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # 模块导出，AI Engine re-export
│   │
│   ├── strategies.rs             # FallbackStrategy trait, ControlCommand,
│   │                             # CommandType, AiCommandValidator trait,
│   │                             # ValidationResult, StrategyType, AiCommand
│   │
│   ├── tai_storage.rs            # 台区储能治理策略实现（唯一兜底策略）
│   │
│   │  # 注：已废弃三策略文件（peak_shaving.rs / demand_control.rs / anti_reverse.rs
│   │  # 及其测试文件）保留于 src/ 但不再编译（lib.rs 不再 mod 声明）
│   │
│   ├── ai_validator.rs           # AiCommandValidatorImpl 可插拔校验器
│   │                             # AiModel trait, MockAiModel, ModelInput/Output
│   │
│   ├── ai_integration.rs         # AiIntegrator（AI 引擎集成）
│   │
│   ├── config.rs                 # TaiStorageConfig
│   ├── errors.rs                 # StrategyError 枚举
│   │
│   ├── ai_validator_test.rs      # AI 校验器单元测试（8 tests）
│   └── tai_storage_test.rs       # 台区储能治理策略单元测试（~15 tests）
```

### lib.rs 模块导出

```rust
pub mod strategies;
pub mod tai_storage;          // 台区储能治理策略（唯一兜底策略）
pub mod ai_validator;
pub mod config;
pub mod errors;
pub mod ai_integration;       // AI 引擎集成

pub use tai_storage::{TaiControllerState, TaiStorageStrategy, TaiState};
pub use ai_validator::{AiCommandValidatorImpl, AiModel, ModelInput, ModelOutput, MockAiModel};
pub use config::TaiStorageConfig;
pub use errors::StrategyError;
pub use strategies::{FallbackStrategy, AiCommandValidator, StrategyType, ControlCommand, CommandType, ValidationResult};
pub use mupc_ai_engine::{ModelManager, FusedSystemState, ActionOutput, ModelStatus, RobustnessManager, AnomalyType};
```

---

## 8. 错误处理

### 8.1 StrategyError 枚举

```rust
#[derive(Error, Debug)]
pub enum StrategyError {
    #[error("策略执行失败: {0}")]
    ExecutionFailed(String),

    #[error("AI 模型错误: {0}")]
    ModelError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),
}
```

### 8.2 错误使用场景

| 错误类型 | 触发场景 | 处理方式 |
|----------|----------|----------|
| `ExecutionFailed` | 策略 evaluate() 内部计算异常 | 返回默认安全指令（p_batt=0） |
| `ModelError` | AI 模型加载失败、预测异常 | 自动降级至兜底模式 |
| `ConfigError` | 配置参数无效（如空时段列表） | 使用默认配置替代 |

---

## 9. 配置管理

### 9.1 运行时配置热加载

- 当前实现：所有配置通过 `Default` trait 提供默认值，构造时传入
- 规划：支持配置文件热加载（修改无需重启）、运行时动态调整

---

## 10. 测试体系

### 10.1 测试覆盖统计

| 测试文件 | 测试用例数 | 测试内容 |
|----------|-----------|----------|
| `ai_validator_test.rs` | 8 | 模型预测（3）、无模型/有模型校验、开关命令、异步接口 |
| `tai_storage_test.rs` | ~15 | 状态机切换（S1~S4 进入/退出/滞回）、积分收敛、容量仲裁、failsafe（详见 §2.13） |

**总计：约 23 个单元测试**

### 10.2 测试数据构造模式

所有策略测试共用 `mupc_data_processing::telemetry::DataPackage` 作为输入，通过辅助函数构造：

```rust
fn create_test_data(timestamp: u64, battery_soc: f64, pv_power: f64, load_power: f64) -> DataPackage {
    DataPackage {
        electrical: ElectricalData { ... },
        battery: BatteryData { soc: Some(battery_soc), ... },
        device_status: DeviceStatus { pv_power: Some(pv_power), load_power: Some(load_power), ... },
        timestamp,
    }
}
```

### 10.3 验证要求

每次代码变更必须通过：

- [ ] `cargo build --release` 编译成功
- [ ] `cargo clippy` 无警告
- [ ] `cargo test -p mupc-strategy-engine` 全部通过
- [ ] `cargo fmt` 格式化通过

---

## 11. 演进路线

| Phase | 内容 | 说明 | 状态 |
|-------|------|------|------|
| Phase 1 | 接口定义：`FallbackStrategy` trait 和 `AiCommandValidator` trait | 仅接口预留 | 已完成 |
| Phase 3A | 兜底策略与 `AiCommandValidatorImpl` + `MockAiModel` | 台区储能治理策略实现 | 已完成（三策略已废弃） |
| Phase 3C | AI 引擎集成：`AiIntegrator` 集成 LSTM/TCN + MADDPG/PPO | 替换 MockAiModel，真实 AI 决策 + 校验 | 已完成 |
| Phase 2+ | 电压越限无功补偿 | 完整策略实现 | 规划中 |
| Phase 2+ | 三相不平衡补偿 | 分相无功补偿 | 规划中 |
| Phase 2+ | 运行时配置热加载 | 配置修改无需重启 | 规划中 |
| Phase 2+ | 消息总线扩展（AMQP/MQTT） | 支持更多消费者 | 规划中 |
| — | Q 控制 | 无功由实时控制模块闭环调节，ControlCommand 中 q_batt_set 已废弃 | 已关闭 |

---

## 附录 A：Cargo.toml 依赖

```toml
[package]
name = "mupc-strategy-engine"
edition.workspace = true

[dependencies]
tokio.workspace = true
tracing.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
async-trait = "0.1"
mupc-common = { path = "../common" }
mupc-data-processing = { path = "../data-processing" }
mupc-ai-engine = { path = "../ai-engine" }

[dev-dependencies]
tokio-test = "0.4"
```

## 附录 B：术语表

| 术语 | 说明 |
|------|------|
| AiIntegrator | AI 集成器，管理 AI 模型生命周期 |
| AiCommandValidator | AI 指令校验器，安全闸门 |
| FallbackStrategy | 兜底策略 trait，所有策略实现此接口 |
| SOC | 电池荷电状态（%） |
| PV | 光伏（Photovoltaic） |
| 台区储能治理 | 单一兜底策略，AI 失效时经核间 V3 帧下发台区储能分相 P/Q |
| DataPackage | 遥测数据包结构体（定义于 mupc-data-processing） |

---

## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| v1.0 | 初版：定义 `FallbackStrategy`/`AiCommandValidator` 接口与三种兜底策略 |
| v1.1 | 农网台区参数更新（变压器容量 500kVA→200kVA） |
| v2.15 | 动作空间精简：AI 2 维动作（p_ref + k_droop），load_shedding/pv_limit 下沉至本地策略 |
| v2.16 | 新增第 4 策略「台区储能治理」：扩展 DataPackage 分相字段、ControlCommand 分相设定、核间 V3 帧、带状态控制器、离线回放验证 |
| v2.17 | 策略引擎精简为单一兜底策略「台区储能治理」；三策略（削峰填谷/需量控制/防逆流）废弃（代码保留不编译），pv_limit/load_shedding 从 ControlCommand 移除 |
| v2.18 | 文档结构重构：台区储能治理策略提升为核心章节 §2，全文档章节重排为连续编号 |
| v2.19 | 新增「本地策略优先」模式：YAML 配置 ai_engine.local_priority + Web API /api/v1/strategy-mode 热切换，AI 旁路运行，控制以本地台区储能策略为准 |
| v2.20 | 台区储能策略标定同步：S1 动态斜坡 boost（返送陡增加速充电）、S1 激进调参（p_abs_trig=2.0/slope=6.0/kp=0.6）、S3 放电裕度限幅（s3_margin_limit）；回放改 net 闭环模型；配置表同步实际默认值 |
| v2.21 | S1 ②分支外部基线变化判别：Δp_base=Δp+Δp_out 区分自激与外部返送消失，仅外部突变才快速退出；普通收窄走正常积分收敛到 +2kW 目标进口（消除 net 闭环自激极限环） |
| v2.22 | S1 共模改前馈吸收：重构基线 P_表基线=P_表净+P_out[k−1]，目标 P_st=P_表基线−P_目标进口，大步斜坡 s1_ff_step_kw 一周期到位；替代 v2.20 boost 与 v2.21 Δp_base 判别（移除 s1_boost_* 配置与 prev_p/prev_p_st 状态），峰值压至第一拍滞后极限 |
| v2.23 | 「本地优先」改为部署默认：ai_engine.local_priority 默认 true（代码 serde 默认 + 部署配置显式声明），开机即本地台区储能策略控制、AI 旁路；需 AI 控制经配置/Web API 切 false |
