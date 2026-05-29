# MUPC 数据处理与存储模块 技术设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 架构师 | 合并版 |

**合并来源：**

- `2026-05-27-MUPC-Phase3A-实施计划.md` — Phase3A 实施计划（data-processing 部分）
- `2026-05-29-MUPC-故障录波完整实现-设计文档.md` [DESIGN_APPROVED]
- `2026-05-29-MUPC-数据存储与设备管理-设计文档.md` [DESIGN_APPROVED]（有条件通过）
- `03-MUPC-数据处理与存储-PRD.md` — 产品需求文档

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

---

## 1. 模块架构

### 1.1 整体架构定位

数据处理与存储模块是 MUPC "异构双核心模块主控架构"中**非实时处理核心（大脑）**的核心数据处理组件，承担以下职责：

- **数据采集**：从 intercore 模块接收实时控制模块的高频采样数据，汇聚为统一数据源
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
| intercore | → data-processing | TCP/RJ45 高频采样数据（10ms 间隔瞬时值帧） |
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
  DataCollector → WriteBuffer → 批量事务(每100ms/100条) → SQLite WAL (按月分区)

故障录波数据流:
  intercore(WaveformSample帧) → DualBufferManager(环形缓冲区)
      → TriggerEngine(触发判定) → capture_waveform() → .wave文件 + SQLite元数据
      → WaveformReporter → MQTT/IEC 104 北向上报

设备台账数据流:
  web-api REST → AssetService → DeviceRepo → SQLite
  plugins → auto_register() → AssetService → DeviceRepo → SQLite
  SQLite → gateway → IEC 104/MQTT 北向上送(定时/变更触发)
```

---

## 2. 遥测采集设计

### 2.1 DataCollector — 数据采集

**职责**：从 intercore 模块接收实时控制模块的数据，汇聚为统一数据源。

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

**[DESIGN_APPROVED] 设计评审: 2026-05-29, 评审人: Design Reviewer, 结论: 通过**

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

**[DESIGN_APPROVED]**

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
│  每100ms或100条   │
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

- 异步通道缓冲，100ms 或积累 100 条触发批量事务提交（先到先执行）
- 写入协程与查询协程通过不同 SQLite 连接实例隔离（读写分离）
- 写入协程使用单个连接，串行化写入
- 查询协程使用独立连接池（最多 4 个连接）

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

**[DESIGN_APPROVED]**

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

*本设计文档合并自 Phase3A 实施计划（data-processing 部分）、故障录波完整实现设计文档 [DESIGN_APPROVED]、数据存储与设备管理设计文档 [DESIGN_APPROVED] 和 PRD。完整定义了遥测采集、故障录波、历史数据存储、设备台账管理、数据生命周期管理五大功能域的技术方案，以及数据模型、接口定义、错误类型、文件结构和关键技术决策。*
