//! 台区总表（`role: meter_grid`）电气量「1 分钟聚合落库」—— **纯逻辑、无 IO**
//! （03 设计 §9.1 / 03 PRD §11.2，U-69）。
//!
//! # 为什么在 `storage`
//!
//! 聚合的**输入**（`DataPackage` 的电气量语义）在 `mupc-southd::mapper`，但聚合的**输出形态**
//! （落库记录：通道名 / 时间戳 / quality）是**存储语义**（03 设计 §4.1.1 / 附录 C 的 `quality`）。
//! 放本 crate 与「落库记录形态的唯一所有者」一致；且 `core-bin` 已依赖 `storage`，**零新增依赖边**
//! （§9.1.1 的裁定 B；A 案「在 core-bin 接收闭包内实现」被否，理由是 core-bin 无单测环境 ⇒
//! 周期边界 / 无采样产行 / 极值这类时序逻辑**无法被单测钉住**）。
//!
//! # 唯一落库形态
//!
//! 复用 `telemetry` 窄表（**不新建表**）：每个通道 1 行 ⇒ 每周期 **18 均值 + 2 通道 × 2 极值
//! = 22 行**（§9.1.3）。缺测行 `value = None` ⇒ 库内为**真 NULL**（`telemetry.value` 自本批起
//! 可空，见 §9.1.4 与 `services.rs` 的 `run_migrations`），**严禁写 0**（PRD R-11.2-E）。
//!
//! # 本模块**不认识** `PointQuality`
//!
//! `storage` 不依赖 `data-processing`（§9.8 D-8 的依赖边裁定）⇒ 跨域转换（`PointQuality → i32`）
//! 落装配层 `mupc-core-bin/src/quality_map.rs`；从 `DataPackage` 抽取 [`GridSample`] 同样落装配层。
//! 本模块只拥有 [`Quality`] 枚举（落库记录形态的一部分）。
//!
//! # 时间戳口径
//!
//! 记录时间戳 = 聚合周期**起点**，即 `ts_ms - (ts_ms % period_ms)`，必为 `period_ms` 的整数倍
//! （PRD R-11.2-C）。极值行与均值行**同时间戳**，靠 `metric_name`（`p_total_max` / `p_total_min`）
//! 区分（§9.1.4）。
//!
//! # 不做「回溯补产」
//!
//! [`GridAggregator::observe`] 只闭合**当前**周期：`ts_ms` 直接跳到 N 个周期之后时，中间周期
//! **不补产**（防长断连后一次补出大量行）；跨**重启**的空档同理表现为**时间戳跳变**（可查、可识别），
//! 而非 `NoData` 行 —— 这是本设计的**已知边界**（§9.1.5 / §9.9 C-2）。
//! [`GridAggregator::tick`] 则**逐周期推进**（无采样周期照样产 [`Quality::NoData`] 行，
//! **不设补产上限** —— PRD R-11.2-E 的「不得静默少行」优先，上限会制造不可区分的空洞）。

use chrono::{DateTime, Utc};

/// 落库记录的**数据质量**（§9.1.4 / §9.8 D-7）。
///
/// 与 `mupc-data-processing::latest_values::PointQuality` **一一映射**（同一语义、两处命名）；
/// 映射函数在装配层（`mupc-core-bin/src/quality_map.rs`，D-8），本 crate **不认识** `PointQuality`。
///
/// ⚠️ `Good = 0` **保持既有写入值不变**（U-68 之前 `quality` 在写入侧恒 0，`startup.rs`）
/// ⇒ 「零行为变化」成立；新增取值只占附录 C 未定义的更大取值，**不占用** `reserved` 的语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Quality {
    /// 数据有效（既有写入值，**必须**保持 0）
    Good = 0,
    /// 本周期该通道**无有效采样**（不可得）⇒ 与 `value = None`（真 NULL）配对
    NoData = 1,
    /// 数据无效（采集异常）
    Invalid = 2,
    /// 数据陈旧（超过新鲜度门限）
    Stale = 3,
    /// 未配置 / 从未采集过
    Unconfigured = 4,
}

impl Quality {
    /// 落库用的整型编码（`telemetry.quality`）。
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// 落库通道规格（**表驱动**：增删通道 / 开关极值 = 改本表，不改算法）。
///
/// ⚠️ **与设计 §9.1.2 的两处「表驱动补充字段」**（`max_metric` / `min_metric`）：设计的结构体只列了
/// `metric`，而极值行的通道名（§9.1.4 明文 `p_total_max` / `p_total_min`）必须是 `&'static str`
/// （[`AggregateRow::metric_name`] 的类型不改成 `String`/`Cow` ⇒ 不引入每行分配）。
/// 故把两个极值名也放进本表 ⇒ 增删极值通道仍然**只改本表、不改算法**，与设计意图一致。
pub struct ChannelSpec {
    /// 落库用通道名（= `telemetry.metric_name`，均值行）
    pub metric: &'static str,
    /// 是否产出分钟极值（max / min 各 1 行）
    pub extremes: bool,
    /// 取数闭包：从样本取该通道值（`None` = 本周期该通道缺测）
    pub pick: fn(&GridSample) -> Option<f64>,
    /// 表意注记（如「取 A 相」），写入设计对照表与日志，**不改数据**
    pub note: &'static str,
    /// 极值 **max** 行的通道名（`extremes == true` 时必须为 `Some`）
    pub max_metric: Option<&'static str>,
    /// 极值 **min** 行的通道名（`extremes == true` 时必须为 `Some`）
    pub min_metric: Option<&'static str>,
}

/// 一个采样点（core-bin 从 `DataPackage` 抽取后传入；**已换算工程值**）。
///
/// `None` = 不可得（**严禁以 0 顶替**，PRD R-11.2-E）。**缺相量块** ⇒ 全部分相通道 `None`；
/// 顶层缺块 ⇒ `p_total` / `q_total` 为 `None`（§9.6 末段）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GridSample {
    /// 三相电压（V）
    pub u: [Option<f64>; 3],
    /// 三相电流（A，**带符号**：方向由分相有功符号承载）
    pub i: [Option<f64>; 3],
    /// 三相有功（kW，含符号：>0 受电 / <0 返送）
    pub p: [Option<f64>; 3],
    /// 三相无功（kVAr，含符号）
    pub q: [Option<f64>; 3],
    /// 三相功率因数
    pub pf: [Option<f64>; 3],
    /// 总有功（kW；缺 `p_total` 块时 mapper 已降级 Σp）
    pub p_total: Option<f64>,
    /// 总无功（kVAr；= Σq）
    pub q_total: Option<f64>,
}

/// 落库用的（窄表）记录：**与 `TelemetryPoint` 同构的「意图」形态**。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AggregateRow {
    pub metric_name: &'static str,
    /// 聚合周期**起点**（UTC ms，`period_ms` 的整数倍）
    pub timestamp: DateTime<Utc>,
    /// 均值/极值；`None` = 本周期**无有效采样**（不可得）—— **不得写 0**。
    pub value: Option<f64>,
    pub quality: Quality,
}

impl AggregateRow {
    /// **唯一落库转换点**（防「`None` 被某处 `unwrap_or(0.0)` 悄悄变成 0」）。
    /// `device_id = "grid_meter"`（站 id）、`metric_name`、`timestamp`、`quality` 逐字带过。
    pub fn to_telemetry_point(&self, device_id: &str) -> crate::models::TelemetryPoint {
        crate::models::TelemetryPoint {
            id: None,
            device_id: device_id.to_string(),
            timestamp: self.timestamp,
            metric_name: self.metric_name.to_string(),
            value: self.value,
            quality: self.quality.code(),
        }
    }
}

// ── 取数闭包（命名函数：表内 `fn` 指针，无捕获、无分配） ──

fn pick_u0(s: &GridSample) -> Option<f64> {
    s.u[0]
}
fn pick_u1(s: &GridSample) -> Option<f64> {
    s.u[1]
}
fn pick_u2(s: &GridSample) -> Option<f64> {
    s.u[2]
}
fn pick_i0(s: &GridSample) -> Option<f64> {
    s.i[0]
}
fn pick_i1(s: &GridSample) -> Option<f64> {
    s.i[1]
}
fn pick_i2(s: &GridSample) -> Option<f64> {
    s.i[2]
}
fn pick_p0(s: &GridSample) -> Option<f64> {
    s.p[0]
}
fn pick_p1(s: &GridSample) -> Option<f64> {
    s.p[1]
}
fn pick_p2(s: &GridSample) -> Option<f64> {
    s.p[2]
}
fn pick_q0(s: &GridSample) -> Option<f64> {
    s.q[0]
}
fn pick_q1(s: &GridSample) -> Option<f64> {
    s.q[1]
}
fn pick_q2(s: &GridSample) -> Option<f64> {
    s.q[2]
}
fn pick_pf0(s: &GridSample) -> Option<f64> {
    s.pf[0]
}
fn pick_pf1(s: &GridSample) -> Option<f64> {
    s.pf[1]
}
fn pick_pf2(s: &GridSample) -> Option<f64> {
    s.pf[2]
}
fn pick_p_total(s: &GridSample) -> Option<f64> {
    s.p_total
}
fn pick_q_total(s: &GridSample) -> Option<f64> {
    s.q_total
}

/// 落库通道清单（§9.1.3 的表驱动形态）：**18 均值通道 + 2 通道极值 = 22 行/周期**。
///
/// | 组 | 通道 |
/// |----|------|
/// | 电压 / 电流 / 分相有功 / 分相无功 / 分相功率因数 | `u_a..u_c` `i_a..i_c` `p_a..p_c` `q_a..q_c` `pf_a..pf_c`（15） |
/// | 总 | `p_total`（+max/min）`q_total`（+max/min）`pf_total`（3） |
///
/// **不落**（§9.1.3 / §9.9 Q-4/Q-5）：频率（点表无频率寄存器，mapper 恒 `50.0` 常量 ⇒ 常量入库
/// 会污染统计）、视在功率 S（无点表来源）、电能（进/出，点表无电能块）。**不得**为凑维度造数据。
pub const CHANNELS: &[ChannelSpec] = &[
    ChannelSpec {
        metric: "u_a",
        extremes: false,
        pick: pick_u0,
        note: "phase.voltage[0]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "u_b",
        extremes: false,
        pick: pick_u1,
        note: "phase.voltage[1]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "u_c",
        extremes: false,
        pick: pick_u2,
        note: "phase.voltage[2]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "i_a",
        extremes: false,
        pick: pick_i0,
        note: "phase.current[0]（带符号）",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "i_b",
        extremes: false,
        pick: pick_i1,
        note: "phase.current[1]（带符号）",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "i_c",
        extremes: false,
        pick: pick_i2,
        note: "phase.current[2]（带符号）",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "p_a",
        extremes: false,
        pick: pick_p0,
        note: "phase.active_power[0]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "p_b",
        extremes: false,
        pick: pick_p1,
        note: "phase.active_power[1]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "p_c",
        extremes: false,
        pick: pick_p2,
        note: "phase.active_power[2]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "q_a",
        extremes: false,
        pick: pick_q0,
        note: "phase.reactive_power[0]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "q_b",
        extremes: false,
        pick: pick_q1,
        note: "phase.reactive_power[1]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "q_c",
        extremes: false,
        pick: pick_q2,
        note: "phase.reactive_power[2]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "pf_a",
        extremes: false,
        pick: pick_pf0,
        note: "phase.cos_phi[0]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "pf_b",
        extremes: false,
        pick: pick_pf1,
        note: "phase.cos_phi[1]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "pf_c",
        extremes: false,
        pick: pick_pf2,
        note: "phase.cos_phi[2]",
        max_metric: None,
        min_metric: None,
    },
    ChannelSpec {
        metric: "p_total",
        extremes: true,
        pick: pick_p_total,
        note: "electrical.active_power（缺块时 mapper 降级 Σp）",
        max_metric: Some("p_total_max"),
        min_metric: Some("p_total_min"),
    },
    ChannelSpec {
        metric: "q_total",
        extremes: true,
        pick: pick_q_total,
        note: "electrical.reactive_power（= Σq）",
        max_metric: Some("q_total_max"),
        min_metric: Some("q_total_min"),
    },
    ChannelSpec {
        metric: "pf_total",
        extremes: false,
        pick: pick_pf0,
        note: "⚠️ **实为 A 相值**（mapper 的 electrical.cos_phi = pf[0]），语义不严格，见 Q-3",
        max_metric: None,
        min_metric: None,
    },
];

/// 一个周期的累加器（**只存计数 / 和 / 极值**，不存样本点 —— 周期内样本数无上界，
/// 存全部样本会让内存随采集频率线性增长）。
struct PeriodAcc {
    start_ms: u64,
    count: Vec<u32>,
    sum: Vec<f64>,
    max: Vec<f64>,
    min: Vec<f64>,
}

impl PeriodAcc {
    fn new(start_ms: u64, n: usize) -> Self {
        Self {
            start_ms,
            count: vec![0; n],
            sum: vec![0.0; n],
            max: vec![f64::NEG_INFINITY; n],
            min: vec![f64::INFINITY; n],
        }
    }

    /// 喂入一个样本：仅对**该通道取数成功**的通道累加 ⇒ 均值分母是**本通道有效采样数**
    /// （不是周期总采样数，§9.1.4「部分缺测」行）。
    fn feed(&mut self, specs: &[ChannelSpec], s: &GridSample) {
        for (i, spec) in specs.iter().enumerate() {
            let Some(v) = (spec.pick)(s) else { continue };
            // `NaN` / `±inf` 不是「有有效采样」——若计入，均值与极值都会被污染（且违反
            // 「不得冒充有效值」的同一条禁令）⇒ 按缺测处理（`continue`）。
            if !v.is_finite() {
                continue;
            }
            self.count[i] += 1;
            self.sum[i] += v;
            if v > self.max[i] {
                self.max[i] = v;
            }
            if v < self.min[i] {
                self.min[i] = v;
            }
        }
    }
}

/// 台区总表电气量聚合器（纯逻辑、无 IO；**不含任何定时**——tick 由装配层驱动）。
pub struct GridAggregator {
    period_ms: u64,
    specs: &'static [ChannelSpec],
    /// 当前**未闭合**周期（`None` = 尚无锚点：进程刚启动、或刚 `flush` 过）
    cur: Option<PeriodAcc>,
    /// 最近一次**已闭合**周期的起点。用途有二：
    /// ① 挡住「所属周期已被闭合」的迟到 / 乱序样本（不重开、不回溯）；
    /// ② 跨重启空档表现为**时间戳跳变**（`observe` 不从历史周期补产）。
    last_start_ms: Option<u64>,
}

impl GridAggregator {
    /// `period_ms` = 聚合周期（`storage.grid_aggregate_period_ms`，默认 60000）。
    /// 取 `.max(1)` 只为挡住除零（合法配置由 `core_config::validate_storage` 门禁为 ≥ 10000）。
    pub fn new(period_ms: u64) -> Self {
        Self {
            period_ms: period_ms.max(1),
            specs: CHANNELS,
            cur: None,
            last_start_ms: None,
        }
    }

    /// 喂入一个采样：若跨过周期边界，返回**已闭合周期**的全部行（0 或 1 个周期）。
    /// ⚠️ **只闭合、不回溯**：`ts_ms` 直接跳到 N 个周期之后时，中间周期**不补产**
    /// （防长断连后一次补出大量行；口径见 §9.1.5）。
    ///
    /// 迟到样本（所属周期**已闭合**）直接丢弃：设计未要求乱序重排，且重开已闭合周期会
    /// 产**第二条同时间戳**的周期记录（与「一个周期恰有 N 条记录」的口径冲突）。
    pub fn observe(&mut self, ts_ms: u64, s: &GridSample) -> Vec<AggregateRow> {
        let start = self.period_start(ts_ms);
        // 所属周期已闭合（含退出 flush 关掉的那个）⇒ 丢弃，不回溯、不重开。
        if let Some(last) = self.last_start_ms {
            if start <= last {
                return Vec::new();
            }
        }
        match self.cur.as_ref().map(|a| a.start_ms) {
            // 同一周期：累加，无产出。
            Some(cur_start) if cur_start == start => {
                if let Some(acc) = self.cur.as_mut() {
                    acc.feed(self.specs, s);
                }
                Vec::new()
            }
            // 跨过边界（且未闭合过）：闭合旧周期（产出其全部行）+ 开新周期并喂本样本。
            Some(cur_start) if start > cur_start => {
                let rows = self.close_current();
                self.open(start, s);
                rows
            }
            // 迟到（早于当前周期）：丢弃 —— 不得据此「回开」一个更早的周期
            // （那会产第二条同时间戳的周期记录，与「一周期恰 N 条」冲突）。
            Some(_) => Vec::new(),
            // 无锚点（首个样本或 flush 之后）：开新周期，无产出。
            None => {
                self.open(start, s);
                Vec::new()
            }
        }
    }

    /// 时间推进（定时 tick）：闭合「已越过 `start + period` 但仍无新采样」的**当前**周期。
    /// **无采样周期照样产行**（PRD R-11.2-E），`quality = NoData`。
    ///
    /// 逐周期推进（**不设补产上限**）：停机 / 时钟跳变让 `now_ms` 一次性跨过 N 个周期时，
    /// 补齐这 N 个周期各 22 行 `NoData` ——「不得静默少行」优先于「省行数」。
    ///
    /// 无锚点（启动后尚未收到任何样本、也尚未 tick 过）时**只锚定当前周期、不产行**：
    /// 若在此**回溯**补产，「跨重启空档」就会变成一大堆 `NoData` 行，与 §9.1.5 的
    /// 「重启后从当前周期开始、空档表现为时间戳跳变」相反。
    pub fn tick(&mut self, now_ms: u64) -> Vec<AggregateRow> {
        if self.cur.is_none() {
            self.cur = Some(PeriodAcc::new(self.period_start(now_ms), self.specs.len()));
            return Vec::new();
        }
        let mut rows = Vec::new();
        while let Some(cur_start) = self.cur.as_ref().map(|a| a.start_ms) {
            // 周期尚未走完 ⇒ 不闭合（本周期仍可继续收集样本）。
            if now_ms < cur_start.saturating_add(self.period_ms) {
                break;
            }
            rows.extend(self.close_current());
            // 打开下一个（空）周期：连续跨多个周期时循环继续，逐周期产 `NoData` 行。
            self.cur = Some(PeriodAcc::new(
                cur_start.saturating_add(self.period_ms),
                self.specs.len(),
            ));
        }
        rows
    }

    /// 退出前 flush：把当前未闭合周期**闭合并产出**（**进程优雅退出**）。
    ///
    /// 部分周期照原样求值（用已采到的样本算均值 —— 退出不该丢掉最后一段）；若该段一个
    /// 有效采样都没有，则产 22 行 `NoData`（与其他无采样路径一致）。
    ///
    /// **无未闭合周期时无产出**（`cur == None`：进程从未采到样本，或刚 flush 过）——
    /// 不得凭空锚一个周期出来产行：那既是「造行」（没有任何采集事实），也会让「重启后不补产」
    /// （§9.1.5）在退出路径上被绕过。`now_ms` 只入日志（退出时刻，供排障对时间轴）。
    pub fn flush(&mut self, now_ms: u64) -> Vec<AggregateRow> {
        match self.cur.as_ref().map(|a| a.start_ms) {
            None => {
                tracing::debug!(now_ms, "退出 flush：无未闭合周期，无产出");
                Vec::new()
            }
            Some(start_ms) => {
                tracing::debug!(now_ms, start_ms, "退出 flush：闭合未完成周期");
                self.close_current()
            }
        }
    }

    /// 本周期起点（供 tick 的幂等判断 / 测试用）：`None` = 当前无未闭合周期。
    pub fn current_start_ms(&self) -> Option<u64> {
        self.cur.as_ref().map(|a| a.start_ms)
    }

    /// 每周期产出行数（18 均值 + 2×2 极值 = 22，§9.1.3）。由表算出，不写死常量。
    pub fn rows_per_period(&self) -> usize {
        self.specs
            .iter()
            .map(|s| 1 + if s.extremes { 2 } else { 0 })
            .sum()
    }

    /// 周期起点：`start = ts_ms - (ts_ms % period_ms)`（PRD R-11.2-C）。
    fn period_start(&self, ts_ms: u64) -> u64 {
        ts_ms - (ts_ms % self.period_ms)
    }

    /// 开一个周期并喂入首个样本。
    fn open(&mut self, start_ms: u64, s: &GridSample) {
        let mut acc = PeriodAcc::new(start_ms, self.specs.len());
        acc.feed(self.specs, s);
        self.cur = Some(acc);
    }

    /// 闭合当前周期 ⇒ 产出**全部**行（缺测行 `value = None` + `NoData`，**不写 0**），
    /// 并推进 `last_start_ms`。
    fn close_current(&mut self) -> Vec<AggregateRow> {
        let Some(acc) = self.cur.take() else {
            return Vec::new();
        };
        let timestamp = ms_to_utc(acc.start_ms);
        let mut rows = Vec::with_capacity(self.rows_per_period());
        for (i, spec) in self.specs.iter().enumerate() {
            let (value, quality) = if acc.count[i] > 0 {
                (Some(acc.sum[i] / f64::from(acc.count[i])), Quality::Good)
            } else {
                // 本周期该通道**无有效采样** ⇒ 真 NULL + NoData（**不得写 0 冒充**）。
                (None, Quality::NoData)
            };
            rows.push(AggregateRow {
                metric_name: spec.metric,
                timestamp,
                value,
                quality,
            });
            if spec.extremes {
                // 极值行与均值行**同时间戳**（§9.1.4），仅 `metric_name` 不同。
                let (max_name, min_name) = (
                    spec.max_metric
                        .expect("extremes=true 的通道必须给 max_metric"),
                    spec.min_metric
                        .expect("extremes=true 的通道必须给 min_metric"),
                );
                for name in [max_name, min_name] {
                    let (value, quality) = if acc.count[i] > 0 {
                        let v = if name == max_name {
                            acc.max[i]
                        } else {
                            acc.min[i]
                        };
                        (Some(v), Quality::Good)
                    } else {
                        (None, Quality::NoData)
                    };
                    rows.push(AggregateRow {
                        metric_name: name,
                        timestamp,
                        value,
                        quality,
                    });
                }
            }
        }
        self.last_start_ms = Some(acc.start_ms);
        rows
    }
}

/// `u64` 毫秒 → UTC 时间。非法值（超出 chrono 可表示范围）以 epoch 兜底并**响亮**记录
/// （同 `repository::ts_to_datetime` 的范式：不静默）。
fn ms_to_utc(ms: u64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms.min(i64::MAX as u64) as i64).unwrap_or_else(|| {
        tracing::error!(
            timestamp_ms = ms,
            "聚合周期起点非法的毫秒时间戳，使用 epoch 兜底"
        );
        DateTime::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个 18 通道全有值的样本（`p_total`/`q_total` 由分相和给出 ⇒ 便于断言极值）。
    fn full_sample(base: f64) -> GridSample {
        let p_raw = [base, base + 1.0, base + 2.0];
        let q_raw = [base + 3.0, base + 4.0, base + 5.0];
        let p = [Some(p_raw[0]), Some(p_raw[1]), Some(p_raw[2])];
        let q = [Some(q_raw[0]), Some(q_raw[1]), Some(q_raw[2])];
        GridSample {
            u: [Some(base + 10.0), Some(base + 11.0), Some(base + 12.0)],
            i: [Some(base + 13.0), Some(base + 14.0), Some(base + 15.0)],
            p,
            q,
            pf: [Some(base + 16.0), Some(base + 17.0), Some(base + 18.0)],
            p_total: Some(p_raw.iter().sum()),
            q_total: Some(q_raw.iter().sum()),
        }
    }

    fn row<'a>(rows: &'a [AggregateRow], metric: &str) -> &'a AggregateRow {
        rows.iter()
            .find(|r| r.metric_name == metric)
            .unwrap_or_else(|| panic!("产出中缺通道 {metric}"))
    }

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}（误差须 < 1e-9）");
    }

    /// **GRD-05**：通道集合 == 表；行数 == 22（18 均值 + 2×2 极值）。
    #[test]
    fn channels_table_is_18_means_plus_2_extremes() {
        let names: Vec<&str> = CHANNELS.iter().map(|c| c.metric).collect();
        assert_eq!(names.len(), 18, "均值通道数 = 18");
        let expected = [
            "u_a", "u_b", "u_c", "i_a", "i_b", "i_c", "p_a", "p_b", "p_c", "q_a", "q_b", "q_c",
            "pf_a", "pf_b", "pf_c", "p_total", "q_total", "pf_total",
        ];
        assert_eq!(names, expected.to_vec(), "通道名集合与设计 §9.1.3 逐条一致");
        // 极值只覆盖 p_total / q_total（Q-2 裁定 (b)；频率因「无源」被排除）。
        let extreme_bases: Vec<&str> = CHANNELS
            .iter()
            .filter(|c| c.extremes)
            .map(|c| c.metric)
            .collect();
        assert_eq!(extreme_bases, vec!["p_total", "q_total"]);
        for c in CHANNELS.iter().filter(|c| c.extremes) {
            assert!(c.max_metric.is_some() && c.min_metric.is_some());
        }
        let agg = GridAggregator::new(60_000);
        assert_eq!(agg.rows_per_period(), 22, "每周期 22 行");
        let rows = {
            let mut agg = GridAggregator::new(60_000);
            agg.observe(0, &full_sample(0.0));
            agg.flush(0)
        };
        assert_eq!(rows.len(), 22);
        let mut produced: Vec<&str> = rows.iter().map(|r| r.metric_name).collect();
        produced.sort_unstable();
        let mut wanted: Vec<&str> = expected
            .iter()
            .copied()
            .chain(["p_total_max", "p_total_min", "q_total_max", "q_total_min"])
            .collect();
        wanted.sort_unstable();
        assert_eq!(produced, wanted, "22 行 = 18 均值 + 4 极值，名字逐条一致");
    }

    /// **GRD-02（正向）**：均值 == 算术平均、极值 == max/min（误差 < 1e-9）。
    ///
    /// **反向（本条为何能变红）**：把 `PeriodAcc::feed` 改成「只记首值」（或把 `close_current`
    /// 改成返回 `samples[0]`）⇒ 阶梯序列的均值就不再是 200.0（首值 100.0）⇒ 本用例红。
    #[test]
    fn grd02_mean_and_extremes_are_statistics_not_first_sample() {
        let mut agg = GridAggregator::new(60_000);
        // 同一周期内 3 个样本：p_total = 100 / 200 / 300（阶梯）⇒ 均值 200、max 300、min 100
        for (ts, v) in [(0u64, 100.0), (1_000, 200.0), (2_000, 300.0)] {
            let mut s = full_sample(0.0);
            s.p_total = Some(v);
            assert!(agg.observe(ts, &s).is_empty(), "周期未闭合前不产行");
        }
        let rows = agg.observe(60_000, &full_sample(0.0));
        assert_eq!(rows.len(), 22);
        approx(row(&rows, "p_total").value.unwrap(), 200.0);
        approx(row(&rows, "p_total_max").value.unwrap(), 300.0);
        approx(row(&rows, "p_total_min").value.unwrap(), 100.0);
        assert_eq!(row(&rows, "p_total").quality, Quality::Good);
        // 首值 100.0 ≠ 均值 200.0 ⇒ 「取首值」的实现必然在此红（R-11.2-A 反例）。
        assert_ne!(row(&rows, "p_total").value.unwrap(), 100.0);
    }

    /// **GRD-02（恒定序列）**：恒定输入 ⇒ 均值 == 该值（且极值同值）。
    #[test]
    fn grd02_constant_sequence_mean_equals_the_value() {
        let mut agg = GridAggregator::new(60_000);
        for ts in (0..60_000).step_by(1_000) {
            agg.observe(ts, &full_sample(0.0));
        }
        let rows = agg.observe(60_000, &full_sample(0.0));
        assert_eq!(rows.len(), 22);
        for r in &rows {
            assert!(r.value.is_some(), "样本 18 通道全有值 ⇒ 不得有缺测行");
            assert_eq!(r.quality, Quality::Good);
        }
        approx(row(&rows, "u_a").value.unwrap(), 10.0);
        approx(row(&rows, "u_b").value.unwrap(), 11.0);
        approx(row(&rows, "pf_total").value.unwrap(), 16.0);
        // 极值与均值同值（恒定序列）
        approx(row(&rows, "p_total").value.unwrap(), 3.0);
        approx(row(&rows, "p_total_max").value.unwrap(), 3.0);
        approx(row(&rows, "p_total_min").value.unwrap(), 3.0);
    }

    /// **GRD-03**：时间戳 = 周期**起点**且为 `period_ms` 的整数倍。
    #[test]
    fn grd03_timestamp_is_period_start_multiple_of_period() {
        let mut agg = GridAggregator::new(60_000);
        let sample = full_sample(0.0);
        assert!(agg.observe(1_000_000_001, &sample).is_empty());
        assert_eq!(agg.current_start_ms(), Some(999_960_000));
        let rows = agg.observe(1_000_060_000, &sample);
        assert_eq!(rows.len(), 22);
        for r in &rows {
            assert_eq!(r.timestamp.timestamp_millis(), 999_960_000);
            assert_eq!(
                r.timestamp.timestamp_millis() % 60_000,
                0,
                "必为 period 的整数倍"
            );
        }
        // 极值行与均值行**同时间戳**
        assert_eq!(
            row(&rows, "p_total_max").timestamp,
            row(&rows, "p_total").timestamp
        );
    }

    /// **GRD-04（单测半边）**：无采样周期照样产行 ⇒ 22 行 `NoData`、`value = None`（**不是 0**）。
    #[test]
    fn grd04_no_sample_period_still_produces_22_nodata_rows() {
        let mut agg = GridAggregator::new(60_000);
        // 无锚点：先锚定当前周期（**不回溯补产**）
        assert!(agg.tick(0).is_empty());
        assert!(agg.tick(59_999).is_empty(), "周期未走完不闭合");
        let rows = agg.tick(60_000);
        assert_eq!(rows.len(), 22, "无采样周期照样 22 行");
        for r in &rows {
            assert_eq!(r.value, None, "缺测写 None（真 NULL），严禁写 0");
            assert_eq!(r.quality, Quality::NoData);
            assert_eq!(r.quality.code(), 1);
            assert_eq!(r.timestamp.timestamp_millis(), 0, "时间戳 = 周期起点");
        }
        // 库内形态：真 NULL，不是 0
        for r in &rows {
            let tp = r.to_telemetry_point("grid_meter");
            assert_eq!(tp.value, None);
            assert_eq!(tp.quality, 1);
        }
    }

    /// **GRD-04（对比半边）**：真实 0 值采样 ⇒ `value = Some(0.0)` + `Good` ⇒ 与缺测**可区分**。
    #[test]
    fn grd04_real_zero_sample_is_distinguishable_from_nodata() {
        let mut agg = GridAggregator::new(60_000);
        let zero = GridSample {
            u: [Some(0.0), Some(0.0), Some(0.0)],
            ..Default::default()
        };
        agg.observe(0, &zero);
        let rows = agg.tick(60_000);
        assert_eq!(rows.len(), 22);
        // 有采样的通道：真 0.0（**不是 NULL**）
        assert_eq!(row(&rows, "u_a").value, Some(0.0));
        assert_eq!(row(&rows, "u_a").quality, Quality::Good);
        // 无采样的通道：NULL（与真 0 在库内可区分 —— 这正是「可区分」的机械判据）
        assert_eq!(row(&rows, "i_a").value, None);
        assert_eq!(row(&rows, "i_a").quality, Quality::NoData);
    }

    /// **§9.1.5 断连**：长断连期间 tick 逐周期补齐（**不设上限**），时间戳严格连续。
    #[test]
    fn long_disconnect_tick_backfills_every_period_with_nodata() {
        let mut agg = GridAggregator::new(60_000);
        agg.tick(0);
        // 30 min 无人调用 tick（例如采集任务饿死）⇒ 一次 tick 补齐 30 个周期
        let rows = agg.tick(30 * 60_000);
        assert_eq!(
            rows.len(),
            30 * 22,
            "30 周期 × 22 行 = 660 行（§9.1.5 的容量口径）"
        );
        let mut starts: Vec<i64> = rows
            .iter()
            .map(|r| r.timestamp.timestamp_millis())
            .collect();
        starts.dedup();
        assert_eq!(starts.len(), 30, "每周期一个时间戳");
        assert_eq!(starts[0], 0);
        assert_eq!(starts[29], 29 * 60_000);
        assert!(rows.iter().all(|r| r.value.is_none()));
    }

    /// **§9.1.5 跨重启**：重启后从**当前**周期开始，不回溯补产历史空档
    /// （空档表现为时间戳跳变，而非 `NoData` 行）。
    #[test]
    fn restart_does_not_backfill_gap_with_nodata_rows() {
        let mut agg = GridAggregator::new(60_000);
        // 重启时刻 = 2 h 之后：首个 tick 只锚定当前周期、**不产行**
        let boot_ms = 2 * 3_600_000;
        assert!(
            agg.tick(boot_ms).is_empty(),
            "重启后不得为历史空档补产 NoData 行"
        );
        assert_eq!(agg.current_start_ms(), Some(boot_ms));
        // 下一周期才产行，且时间戳从 boot 起（跳变 —— 空档可识别）
        let rows = agg.tick(boot_ms + 60_000);
        assert_eq!(rows.len(), 22);
        assert_eq!(rows[0].timestamp.timestamp_millis(), boot_ms as i64);
    }

    /// **§9.1.5 观察跨多个周期/迟到样本**：`observe` 只闭合当前周期，不补中间周期；
    /// 已闭合周期的迟到样本被丢弃（不重开、不产第二条同时间戳记录）。
    #[test]
    fn observe_closes_only_current_period_and_drops_late_samples() {
        let mut agg = GridAggregator::new(60_000);
        let s = full_sample(0.0);
        agg.observe(0, &s);
        // 跳到 5 个周期之后：只闭合周期 0（不补 1..4）
        let rows = agg.observe(300_000, &s);
        assert_eq!(rows.len(), 22);
        assert_eq!(rows[0].timestamp.timestamp_millis(), 0);
        assert_eq!(agg.current_start_ms(), Some(300_000));
        // 迟到样本（周期 0 / 60_000 已闭合）⇒ 丢弃，无产出、不动当前周期
        assert!(agg.observe(1_000, &s).is_empty());
        assert!(agg.observe(60_000, &s).is_empty());
        assert_eq!(agg.current_start_ms(), Some(300_000));
    }

    /// **退出 flush**：部分周期照原样闭合并产出（不丢最后一段）；**无锚点则不产行**
    /// （不得凭空造一个周期出来，`flush` 两次也不重复产行）。
    #[test]
    fn flush_closes_partial_period_and_is_idempotent_afterwards() {
        let mut agg = GridAggregator::new(60_000);
        let mut s = full_sample(0.0);
        s.p_total = Some(42.0);
        agg.observe(120_000, &s);
        let rows = agg.flush(130_000);
        assert_eq!(rows.len(), 22);
        assert_eq!(rows[0].timestamp.timestamp_millis(), 120_000);
        approx(row(&rows, "p_total").value.unwrap(), 42.0);
        // flush 后无未闭合周期 ⇒ 再 flush 不重复产行（幂等）
        assert!(agg.flush(140_000).is_empty());
        // 从未 observe / tick 过：**无锚点 ⇒ 无产出**（不凭空造一个 NoData 周期）
        let mut fresh = GridAggregator::new(60_000);
        assert!(fresh.flush(3_600_000).is_empty());
        assert_eq!(fresh.current_start_ms(), None);
        // 无采样但**有锚点**（tick 锚过）⇒ flush 产出该周期 22 行 NoData
        let mut anchored = GridAggregator::new(60_000);
        anchored.tick(0);
        let rows = anchored.flush(30_000);
        assert_eq!(rows.len(), 22);
        assert!(rows
            .iter()
            .all(|r| r.value.is_none() && r.quality == Quality::NoData));
    }

    /// **部分缺测**：均值分母 = 本通道**有效采样数**（不是周期总采样数）；缺测通道 NoData。
    #[test]
    fn partial_missing_channel_uses_its_own_valid_sample_count() {
        let mut agg = GridAggregator::new(60_000);
        // 样本 1：i_a = 10；样本 2：i_a 缺测（None），p_total = 分别 1 / 3
        for (ts, i_a, p_total) in [(0u64, Some(10.0), 1.0), (1_000, None, 3.0)] {
            let mut s = full_sample(0.0);
            s.i[0] = i_a;
            s.p_total = Some(p_total);
            agg.observe(ts, &s);
        }
        let rows = agg.tick(60_000);
        // i_a 只有 1 个有效采样 ⇒ 均值 = 10（不是 (10+0)/2）
        assert_eq!(row(&rows, "i_a").value, Some(10.0));
        assert_eq!(row(&rows, "i_a").quality, Quality::Good);
        // p_total 两个有效采样 ⇒ 均值 2（分母是「本通道有效采样数」= 2，不是周期总采样数 2 的巧合：
        // 见下条 —— 缺测通道分母为 1）
        approx(row(&rows, "p_total").value.unwrap(), 2.0);
        // u_a 两轮都有值 ⇒ 均值 = base+10 = 10.0
        approx(row(&rows, "u_a").value.unwrap(), 10.0);
    }

    /// **NaN / inf 不算「有效采样」**：不得让非有限值污染均值与极值（等同缺测）。
    #[test]
    fn non_finite_values_are_treated_as_missing() {
        let mut agg = GridAggregator::new(60_000);
        let mut s = full_sample(0.0);
        s.u[0] = Some(f64::NAN);
        agg.observe(0, &s);
        s.u[0] = Some(f64::INFINITY);
        agg.observe(1_000, &s);
        let rows = agg.tick(60_000);
        assert_eq!(row(&rows, "u_a").value, None);
        assert_eq!(row(&rows, "u_a").quality, Quality::NoData);
        assert_eq!(row(&rows, "u_b").value, Some(11.0));
    }

    /// `Quality` 的落库编码（`Good = 0` 保持既有写入值不变，D-7）。
    #[test]
    fn quality_codes_match_design() {
        assert_eq!(Quality::Good.code(), 0);
        assert_eq!(Quality::NoData.code(), 1);
        assert_eq!(Quality::Invalid.code(), 2);
        assert_eq!(Quality::Stale.code(), 3);
        assert_eq!(Quality::Unconfigured.code(), 4);
    }

    /// `to_telemetry_point`：`None` **原样**传成 `Option<f64>`（唯一转换点，禁止在此 `unwrap_or(0.0)`）。
    #[test]
    fn to_telemetry_point_preserves_none_as_null_intent() {
        let rows = {
            let mut agg = GridAggregator::new(60_000);
            agg.tick(0);
            agg.tick(60_000)
        };
        let tp = row(&rows, "p_total").to_telemetry_point("grid_meter");
        assert_eq!(tp.device_id, "grid_meter");
        assert_eq!(tp.metric_name, "p_total");
        assert_eq!(tp.value, None, "缺测必须原样为 None（落库即真 NULL）");
        assert_eq!(tp.quality, 1);
        assert!(tp.id.is_none());
    }
}
