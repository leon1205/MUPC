//! 台区储能治理策略（第 4 策略）
//!
//! AI 失效兜底时，通过台区储能 PCS 分相 P/Q 控制实现：
//! 降返送、降三相不平衡度、提功率因数。
//! 设计见 04-MUPC-策略引擎-设计文档 §15。

use crate::config::TaiStorageConfig;
use crate::strategies::{CommandType, ControlCommand, FallbackStrategy, StrategyType};
use async_trait::async_trait;
use mupc_common::MupcError;
use mupc_data_processing::telemetry::DataPackage;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// 台区储能控制器状态（4 状态机）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaiState {
    S1PvAbsorb, // 光伏吸收
    S2Flat,     // 平段
    S3Peak,     // 高峰放电
    S4Clear,    // 日终清空
}

/// 台区总表单周期测量（控制律输入，含符号约定）
#[derive(Debug, Clone)]
pub struct MeterData {
    pub p: f64,        // 三相总有功 (kW，>0 受电 / <0 返送)
    pub q: f64,        // 三相总无功 (kVAr)
    pub pf: [f64; 3],  // 分相功率因数（索引 0/1/2 = A/B/C）
    pub u: [f64; 3],   // 分相电压 (V)
    pub i: [f64; 3],   // 分相电流 (A，带符号)
    pub p_i: [f64; 3], // 分相有功 (kW，含符号)
    pub q_i: [f64; 3], // 分相无功 (kVAr，含符号)
}

impl Default for MeterData {
    fn default() -> Self {
        Self {
            p: 0.0,
            q: 0.0,
            pf: [1.0; 3],
            u: [220.0; 3],
            i: [0.0; 3],
            p_i: [0.0; 3],
            q_i: [0.0; 3],
        }
    }
}

/// 台区储能控制器跨周期状态
#[derive(Debug, Clone)]
pub struct TaiControllerState {
    /// 当前状态机状态（S1~S4）
    pub st: TaiState,
    /// 共模 P 出力 (kW，>0 放电 / <0 充电)
    pub p_st: f64,
    /// 分相无功积分状态 (kVAr)，索引 0/1/2 = A/B/C
    pub q_pcs: [f64; 3],
    /// 分相差模积分状态 (kW)，索引 0/1/2 = A/B/C，三相之和恒为 0
    pub d_p: [f64; 3],
    /// 分相 Q 死区滞回锁存，索引 0/1/2 = A/B/C
    pub q_active: [bool; 3],
    /// 差模死区滞回锁存
    pub d_p_active: bool,
    /// 最近有效 Q (kVAr)，failsafe 用
    pub q_last: [f64; 3],
    /// 滑动滤波窗口缓冲
    pub meter_buf: VecDeque<MeterData>,
    /// 上次控制周期时间戳（节流用）
    pub last_control_ts: u64,
}

impl Default for TaiControllerState {
    fn default() -> Self {
        Self {
            st: TaiState::S2Flat,
            p_st: 0.0,
            q_pcs: [0.0; 3],
            d_p: [0.0; 3],
            q_active: [false; 3],
            d_p_active: false,
            q_last: [0.0; 3],
            meter_buf: VecDeque::new(),
            last_control_ts: 0,
        }
    }
}

/// 每周期向 target 最多移动 step
pub(crate) fn move_toward(x: f64, target: f64, step: f64) -> f64 {
    debug_assert!(step >= 0.0 && step.is_finite(), "step 必须为非负有限值");
    if (x - target).abs() <= step {
        target
    } else {
        x + (target - x).signum() * step
    }
}

/// 滑动滤波：meter 入窗，返回 n 点均值（缓冲未满直接取当前）
fn sliding_avg(
    state: &mut TaiControllerState,
    config: &TaiStorageConfig,
    meter: &MeterData,
) -> MeterData {
    state.meter_buf.push_back(meter.clone());
    while state.meter_buf.len() > config.window_size as usize {
        state.meter_buf.pop_front();
    }
    let n = (state.meter_buf.len() as f64).max(1.0);
    let mut avg = meter.clone();
    for i in 0..3 {
        avg.u[i] = state.meter_buf.iter().map(|m| m.u[i]).sum::<f64>() / n;
        avg.i[i] = state.meter_buf.iter().map(|m| m.i[i]).sum::<f64>() / n;
        avg.p_i[i] = state.meter_buf.iter().map(|m| m.p_i[i]).sum::<f64>() / n;
        avg.q_i[i] = state.meter_buf.iter().map(|m| m.q_i[i]).sum::<f64>() / n;
        avg.pf[i] = state.meter_buf.iter().map(|m| m.pf[i]).sum::<f64>() / n;
    }
    avg.p = avg.p_i.iter().sum();
    avg.q = avg.q_i.iter().sum();
    avg
}

/// 三相电流不平衡度（**幅值式**，电网公司口径，设计 04 §2.1 / 台区储能设计 §2.9.1）：
/// `(1 − min|Ii| / max|Ii|) × 100`，`max|Ii| < 1A` 判 0（除零/极小电流守卫）。
///
/// ⚠️ **必须取幅值** `|Ii|`，不得用带符号电流：
/// - 三相同向返送（全负）时带符号 `max` 取到 0 ⇒ 恒判 0、**差模通道静默关闭**；
/// - 单相返送（其余相受电）时带符号 `min` 取到负值 ⇒ 不平衡度**被夸大到 >100%**
///   （如 `[-10, 8, 8]`：带符号 225%，幅值口径 20%）。
///
/// 注：`imean`（差模积分输入）仍保留**带符号**语义（见 [`control`] 步骤 6），二者不可混用。
pub(crate) fn unbalance_pct(ii: &[f64; 3]) -> f64 {
    let i_max = ii.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let i_min = ii.iter().fold(f64::MAX, |m, v| m.min(v.abs()));
    if i_max < 1.0 {
        0.0
    } else {
        (1.0 - i_min / i_max) * 100.0
    }
}

/// SOC 保护：充电 ≥90% 剪 0 / 88% 线性降额；放电 ≤10% 剪 0 / 12% 线性降额
fn soc_protect(p_st: f64, soc: f64) -> f64 {
    if p_st < 0.0 {
        // 充电
        if soc >= 0.90 {
            0.0
        } else if soc >= 0.88 {
            p_st * (1.0 - (soc - 0.88) / 0.02)
        } else {
            p_st
        }
    } else if p_st > 0.0 {
        // 放电
        if soc <= 0.10 {
            0.0
        } else if soc <= 0.12 {
            p_st * (soc - 0.10) / 0.02
        } else {
            p_st
        }
    } else {
        p_st
    }
}

/// 容量仲裁：每相电流 / 总视在 / 总有功约束 + ΔP 重归一
///
/// 迭代裁剪：每轮先做**收敛步**（共模 clamp + 差模零净重归一 + 由当前 state 重算 pcmd），
/// 再由该轮终态**复检** i_rated / s_rated 并裁剪；无违规则收敛退出（多数场景 1~2 轮）。
///
/// ⚠️ **收敛步必须在环内、复检必须在收敛步之后**（D-8）：
/// 旧实现把 clamp + 重归一放在循环**之后** ⇒ `break` 的检验对象是重归一**前**的状态，
/// 而重归一会把 `move_toward` 削掉的差模量**摊回**相邻相 ⇒ 复超 `i_rated`
/// （`config.rs` 自述 slope=8 标定期遗留 0.9A 过限，当时靠回调 slope 规避而非补复检）。
/// 重算 pcmd 也放在检验前，避免用陈旧值导致 8×slope 过剪或 ×scale⁸ 反复缩放。
fn arbitrate(
    pcmd: &mut [f64; 3],
    q: &mut [f64; 3],
    state: &mut TaiControllerState,
    config: &TaiStorageConfig,
    u: &[f64; 3],
) {
    for _ in 0..8 {
        // 收敛步（幂等）：共模 ±p_cap；差模 ΣΔP=0 重归一；随后按 state 重算分相指令
        converge_step(state, config);
        for (i, v) in pcmd.iter_mut().enumerate() {
            *v = state.p_st / 3.0 + state.d_p[i];
        }
        // 复检 + 裁剪（顺序 ①Q ②差模 P；s_rated 超限则等比缩放差模）
        let mut s_total = 0.0;
        let mut violated = false;
        for i in 0..3 {
            let s = (pcmd[i].powi(2) + q[i].powi(2)).sqrt();
            s_total += s;
            let i_phase = s * 1000.0 / u[i].max(1.0);
            if i_phase > config.i_rated {
                violated = true;
                if q[i].abs() > 0.1 {
                    q[i] = 0.0; // 裁剪顺序 ①Q
                } else if state.d_p[i].abs() > 0.1 {
                    state.d_p[i] = move_toward(state.d_p[i], 0.0, config.slope);
                    // ②差模P
                }
            }
        }
        if s_total > config.s_rated {
            violated = true;
            let scale = (config.s_rated / s_total).min(1.0);
            for i in 0..3 {
                state.d_p[i] *= scale;
            }
        }
        if !violated {
            return; // 收敛：pcmd 与 state 一致且约束满足
        }
    }
    // 8 轮未收敛（几何不可行：如共模顶格 + 强单相差模时「单相限 ∧ ΣΔP=0」无解）：
    // 再走一次收敛步保证 pcmd 与 state 一致，并**显式告警**——不把残限留给 PCS 静默 clamp。
    converge_step(state, config);
    for (i, v) in pcmd.iter_mut().enumerate() {
        *v = state.p_st / 3.0 + state.d_p[i];
    }
    let i_peak = (0..3)
        .map(|i| (pcmd[i].powi(2) + q[i].powi(2)).sqrt() * 1000.0 / u[i].max(1.0))
        .fold(0.0f64, f64::max);
    tracing::warn!(
        i_peak,
        i_rated = config.i_rated,
        "容量仲裁 8 轮未收敛（约束几何不可行：单相电流限与 ΣΔP=0 零净冲突），残留过限已显式告警"
    );
}

/// 仲裁收敛步（幂等）：共模 ±p_cap 钳位 + 差模 ΣΔP=0 重归一。
/// pcmd 由调用方在收敛步之后按 `state` 重算，保证「检验对象 = 终态」。
fn converge_step(state: &mut TaiControllerState, config: &TaiStorageConfig) {
    state.p_st = state.p_st.clamp(-config.p_cap, config.p_cap);
    let d_sum = state.d_p.iter().sum::<f64>() / 3.0;
    for v in state.d_p.iter_mut() {
        *v -= d_sum;
    }
}

/// failsafe（设计 §2.7/§4）：测量不可用（总表分相缺失或校验失败）→ **冻结积分、斜坡回归 0**、
/// 保持最后有效 Q（无功补偿相对安全），并**复位 Q 积分状态**（§2.4「恢复后从 0 重新积分」）。
///
/// 返回 `(分相 P, 分相 Q)`：P 为共模按 slope 向 0 逼近后的分相分摊，Q 为 `q_last` 保持。
fn failsafe(state: &mut TaiControllerState, config: &TaiStorageConfig) -> ([f64; 3], [f64; 3]) {
    state.p_st = move_toward(state.p_st, 0.0, config.slope);
    for i in 0..3 {
        state.d_p[i] = move_toward(state.d_p[i], 0.0, config.slope);
    }
    // 复位积分状态：否则恢复后 Q 从旧积分值续算（设计 §2.4 要求「从 0 重新积分」）
    state.q_pcs = [0.0; 3];
    // 清滤波窗：不得把失效拍的测量混入恢复后的滑动均值
    state.meter_buf.clear();
    let pcmd = [
        state.p_st / 3.0 + state.d_p[0],
        state.p_st / 3.0 + state.d_p[1],
        state.p_st / 3.0 + state.d_p[2],
    ];
    (pcmd, state.q_last)
}

/// 核心控制器：单周期控制（纯函数，跨周期状态由 state 承载）
///
/// 返回 (分相有功 P [kW], 分相无功 Q [kVAr])，P>0 放电/注入，P<0 充电/吸收。
/// soc 为 0~1 小数。
pub fn control(
    state: &mut TaiControllerState,
    config: &TaiStorageConfig,
    meter: &MeterData,
    soc: f64,   // 0..1
    t_now: u64, // unix 秒
) -> ([f64; 3], [f64; 3]) {
    debug_assert!(soc.is_finite(), "soc 必须为有限值");
    // 1. 滤波
    let f = sliding_avg(state, config, meter);
    let p = f.p;
    let pi = f.p_i;
    let qi = f.q_i;
    let u = f.u;
    let pfi = f.pf;
    let ii = f.i;

    // 2. failsafe：数据异常 → 积分冻结 + 斜坡回归 0，保持最近有效 Q（并复位 Q 积分）
    if !p.is_finite() || pi.iter().any(|x| !x.is_finite()) {
        return failsafe(state, config);
    }

    // 3. 状态机（优先级 S4 > S1 > S3 > S2；滞回）
    let secs = (t_now % 86400) as f64;
    let soc_cap = if secs < config.t_release_secs {
        config.soc_cap_day
    } else {
        0.90
    };
    let hours_to_clear = ((config.t_clear_end_secs - secs) / 3600.0).max(0.1);
    let mut p_force =
        ((soc - 0.10) * config.battery_capacity_kwh / hours_to_clear).clamp(0.0, config.p_cap);
    // S4 限幅：避免强制放电超出受电 + 裕度，减少夜间过度反送（可牺牲部分日终清空）
    if config.s4_limit_margin_kw > 0.0 {
        p_force = p_force.min(p + config.s4_limit_margin_kw);
    }
    if u.iter().any(|&x| x > 235.0) {
        p_force = p_force.min(p.max(0.0)); // 电压越限保护
    }

    // v2.22 前馈：重构外部基线返送 = 当前净功 meter.p + 上周期储能输出 state.p_st
    // （net 闭环下净功含储能自身效应）。S1 进出与目标均基于该基线——前馈下净功率被
    // 拉到目标进口 +2，不再反映返送是否存在，故状态机改用基线判断。
    let p_base_est = meter.p + state.p_st;

    state.st = if secs >= config.t_clear_start_secs && soc > 0.10 {
        TaiState::S4Clear
    } else {
        match state.st {
            TaiState::S1PvAbsorb => {
                // 保持 S1：基线返送仍存在（< s1_exit），或储能尚未回归 0（基线骤转受电后
                // 需大步斜坡回 0 再退出，避免 S2 慢斜坡期间储能从电网取电）
                if (p_base_est < config.s1_exit || state.p_st < -1.0) && soc < soc_cap {
                    TaiState::S1PvAbsorb
                } else {
                    TaiState::S2Flat
                }
            }
            TaiState::S3Peak => {
                if state.p_st > 0.0 || p > config.p_tgt_s3 {
                    TaiState::S3Peak
                } else {
                    TaiState::S2Flat
                }
            }
            _ => {
                // 基线返送超阈值 → 进入 S1（前馈下净功率恒 ≈+2，不能用净功判断）
                if p_base_est < -config.p_abs_trig && soc < soc_cap - config.soc_hys {
                    TaiState::S1PvAbsorb
                } else if p > config.p_dis_trig {
                    TaiState::S3Peak
                } else {
                    TaiState::S2Flat
                }
            }
        }
    };

    // 4. 分相 Q（积分式，把表计无功归零）
    let mut q = [0.0; 3];
    for i in 0..3 {
        if pfi[i].abs() > 0.98 {
            state.q_active[i] = false;
        } else if pfi[i].abs() < 0.95 {
            state.q_active[i] = true;
        }
        if state.q_active[i] {
            let inc = config.s_q_sign * config.k_q * qi[i];
            state.q_pcs[i] = (state.q_pcs[i] + inc).clamp(-config.q_i_max, config.q_i_max);
            q[i] = state.q_pcs[i];
        } else {
            state.q_pcs[i] = move_toward(state.q_pcs[i], 0.0, config.q_i_max);
            q[i] = state.q_pcs[i];
        }
    }

    // 5. 共模 P
    // S1 前馈吸收（v2.22）：net 闭环下净功含储能自身效应，重构外部基线
    // P_基线 = 当前净功 meter.p + 上周期输出 state.p_st，直接按基线返送充电，
    // 目标 P_st = P_基线 − P_目标进口，大步斜坡 s1_ff_step_kw 一周期到位。
    // 替代 v2.20 动态斜坡 boost 与 v2.21 Δp_base 判别（反馈积分滞后一拍、自激极限环一并消除）。
    state.p_st = match state.st {
        TaiState::S1PvAbsorb => {
            // 前馈目标 = 基线返送 − 目标进口。clamp 上限 0：基线受电时停充；
            // 下限 −p_cap。大步斜坡一周期到位，返送减小仍返送时 target 自动降载不停充。
            let p_st_target = (p_base_est - config.p_tgt_s1).clamp(-config.p_cap, 0.0);
            move_toward(state.p_st, p_st_target, config.s1_ff_step_kw)
        }
        TaiState::S2Flat => move_toward(state.p_st, 0.0, config.slope),
        TaiState::S3Peak => {
            let inc = (config.kp * (p - config.p_tgt_s3)).clamp(-config.slope, config.slope);
            let mut p_st = (state.p_st + inc).clamp(0.0, config.p_cap);
            if config.s3_margin_limit {
                // 放电不超当前负荷裕度（防 S3 过冲返送：负荷快速回落时即时跟随，而非靠斜坡缓慢降）
                p_st = p_st.min((p - config.p_tgt_s3).max(0.0));
            }
            p_st
        }
        TaiState::S4Clear => move_toward(state.p_st, p_force, config.slope),
    };
    state.p_st = soc_protect(state.p_st, soc);

    // 6. 差模 P（积分式，零净能量；I_i 带符号）
    // imean 保留**带符号**：它是差模积分的参考量（离开自身均值的带符号偏差）。
    let imean = ii.iter().sum::<f64>() / 3.0;
    // 不平衡度按**幅值**口径（D-1，见 unbalance_pct 文档）
    let unbal = unbalance_pct(&ii);
    if unbal < 15.0 {
        state.d_p_active = false;
    } else if unbal > 25.0 {
        state.d_p_active = true;
    }
    for i in 0..3 {
        // 差模增量斜坡限速：大不平衡单周期跳变 ≤ slope（设计 §15.6 ΔP_i 每周期 ≤5kW），
        // 避免一次积分跳满 dp_max（25kW/相，≈110A@230V，PCS 单相 ±25 硬限）造成过流
        let inc = (config.k_diff * u[i] * (ii[i] - imean)).clamp(-config.slope, config.slope);
        if state.d_p_active && inc.abs() > 0.5f64.max(0.05 * pi[i].abs()) {
            state.d_p[i] = (state.d_p[i] + inc).clamp(-config.dp_max, config.dp_max);
        } else {
            state.d_p[i] = move_toward(state.d_p[i], 0.0, config.dp_max);
        }
    }

    // 7. 指令合成
    let mut pcmd = [0.0; 3];
    for (i, v) in pcmd.iter_mut().enumerate() {
        *v = state.p_st / 3.0 + state.d_p[i];
    }

    // 8. 容量仲裁
    arbitrate(&mut pcmd, &mut q, state, config, &u);

    state.q_last = q;
    (pcmd, q)
}

/// 台区储能治理策略（第 4 策略）
///
/// 内部持跨周期控制状态（Arc<Mutex>），实现 FallbackStrategy。
/// 控制周期节流：距上次控制 ≥ control_period_s 才执行 control()。
pub struct TaiStorageStrategy {
    config: TaiStorageConfig,
    state: Arc<Mutex<TaiControllerState>>,
    last_cmd: Arc<Mutex<ControlCommand>>,
    /// D-2：SOC 全缺告警节流时刻（每 30s 一次，防每拍刷屏；有可用 SOC 时不写）
    soc_missing_warned: Mutex<Option<std::time::Instant>>,
    /// D-2：SOC 全缺告警**实际发射**次数（判别力用例观测「告警一次」；无生产消费者）
    soc_missing_warn_count: std::sync::atomic::AtomicU64,
    /// B-3：**数据超期**告警节流时刻（每 30s 一次，防每拍刷屏）。与 `soc_missing_warned`
    /// **各自独立**：两条降级路径的告警不得互相吞掉（同一时刻可能只走其中一条）。
    stale_warned: Mutex<Option<std::time::Instant>>,
    /// B-3：数据超期告警**实际发射**次数（判别力用例观测；无生产消费者）
    stale_warn_count: std::sync::atomic::AtomicU64,
}

impl TaiStorageStrategy {
    /// 命令 ID（与调度约定）
    const CMD_ID: u16 = 4;
    /// D-2：SOC 全缺告警节流间隔（防每 dispatch 拍刷屏，与 SOC 双源告警同量级）
    const SOC_MISSING_WARN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
    /// B-3：数据超期告警节流间隔（与 SOC 全缺告警同值、但**独立**计时）
    const STALE_WARN_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

    /// 「不下发设定」指令：分相 P/Q 全 0（SOC 全缺时下发，等效取消储能出力设定）。
    fn zero_command() -> ControlCommand {
        ControlCommand {
            cmd_id: Self::CMD_ID,
            cmd_type: CommandType::ChargeDischarge,
            p_batt_set: Some(0.0),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: Some(true),
            priority: 3,
            phase_p_set: Some([0.0; 3]),
            phase_q_set: Some([0.0; 3]),
        }
    }

    pub fn new(config: TaiStorageConfig) -> Self {
        let last_cmd = Self::zero_command();
        Self {
            config,
            state: Arc::new(Mutex::new(TaiControllerState::default())),
            last_cmd: Arc::new(Mutex::new(last_cmd)),
            soc_missing_warned: Mutex::new(None),
            soc_missing_warn_count: std::sync::atomic::AtomicU64::new(0),
            stale_warned: Mutex::new(None),
            stale_warn_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// D-2：SOC 全缺（无任何 fresh 源且无可用冻结值）⇒ **拒绝下发设定**（分相 P/Q 全 0）
    /// + 节流告警；**不得**用 50% 这类假值驱动 [`soc_protect`]（假 SOC 会把充放电保护带
    /// 剪到错误档位，比不控制更危险）。
    ///
    /// 内部状态一并归零（共模/差模/Q 积分与滤波窗）：无 SOC 时不得保留力指令，恢复后从 0 起算。
    /// 本函数不更新 `last_control_ts` —— 保护性拒绝**不得消耗控制周期**，SOC 恢复后立即接管。
    fn refuse_missing_soc(&self, state: &mut TaiControllerState) -> ControlCommand {
        state.p_st = 0.0;
        state.d_p = [0.0; 3];
        state.q_pcs = [0.0; 3];
        state.meter_buf.clear();
        let cmd = Self::zero_command();
        *self.last_cmd.lock().unwrap_or_else(|e| e.into_inner()) = cmd.clone();

        let now = std::time::Instant::now();
        let due = {
            let mut w = self
                .soc_missing_warned
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let due = w.map_or(true, |t| {
                now.saturating_duration_since(t) > Self::SOC_MISSING_WARN_INTERVAL
            });
            if due {
                *w = Some(now);
            }
            due
        };
        if due {
            self.soc_missing_warn_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::warn!(
                "SOC 全缺（无 fresh 源且无可用冻结值）：台区储能拒绝下发设定（分相 P/Q=0）——\
                 请检查 BMS 站与核间 SOC 通路（不得以假值驱动充放电保护）"
            );
        }
        cmd
    }

    /// B-3 裁定（2026-09-29）：**遥测数据超期**（总表 > 5s 未更新）⇒ 与
    /// [`Self::refuse_missing_soc`] **同款处置**（分相 P/Q 全 0 + 节流告警）。
    ///
    /// 为什么必须归零而非停发：分相 P/Q 写在 PCS **保持寄存器**（FC06 1006-1011），是
    /// **设定值不是脉冲**——停发不会令 PCS 回零，而是「一直按最后一条指令跑」；数据源断连
    /// 时这条陈旧设定可能维持数小时。故显式下发归零，与 D-2（SOC 全缺）口径一致：两者
    /// 同属「驱动数据不可信」，同一语义不得有两种相反处置。
    ///
    /// 内部状态清零与 `refuse_missing_soc` 完全一致（共模/差模/Q 积分与滤波窗）：无可用
    /// 测量时不得保留力指令，恢复后从 0 起算。本函数**不**更新 `last_control_ts`
    /// —— 保护性拒绝**不得消耗控制周期**，数据恢复后立即接管。
    /// `stale_after`：超期阈值，**由调用方传入**（`AiIntegrator::DATA_STALE_AFTER`）。
    /// **不得**在此写死字面量 —— 阈值真源在 `mupc_data_processing::DATA_FRESHNESS_MS`，
    /// 跨 crate 复制成字面量即"日志可能与实现不同的阈值"（评审 W-3，2026-09-29）。
    pub fn refuse_stale_data(&self, stale_after: std::time::Duration) -> ControlCommand {
        {
            // 短锁块：先清零内部状态、出块即释放（MutexGuard 非 Send，不跨 await 持有）。
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.p_st = 0.0;
            state.d_p = [0.0; 3];
            state.q_pcs = [0.0; 3];
            state.meter_buf.clear();
            // 注：**不**动 state.last_control_ts（保护性拒绝不消耗控制周期）。
        }
        let cmd = Self::zero_command();
        *self.last_cmd.lock().unwrap_or_else(|e| e.into_inner()) = cmd.clone();

        let now = std::time::Instant::now();
        let due = {
            let mut w = self.stale_warned.lock().unwrap_or_else(|e| e.into_inner());
            let due = w.map_or(true, |t| {
                now.saturating_duration_since(t) > Self::STALE_WARN_INTERVAL
            });
            if due {
                *w = Some(now);
            }
            due
        };
        if due {
            self.stale_warn_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // 阈值取自**调用方传入**的 `stale_after`（真源 = `DATA_STALE_AFTER`）⇒
            // 改阈值时文案自动跟随，不再有"改阈值须同步文案"的隐患（评审 W-3）。
            tracing::warn!(
                "遥测数据超过 {}s 未更新（数据源可能断连）：台区储能已下发归零（分相 P/Q=0）\
                 ——请检查总表站与采集链路",
                stale_after.as_secs()
            );
        }
        cmd
    }

    /// 同步评估（用于测试与回放）：内部执行控制周期
    pub fn evaluate_sync(&self, data: &DataPackage) -> ControlCommand {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        // D-2：SOC 全缺 ⇒ 拒绝下发 + 告警。置于控制周期节流**之前**：保护性拒绝不被 60s
        // 节流拖延，且不消耗控制周期（SOC 恢复后立即接管）。
        let Some(soc_pct) = data.battery.soc else {
            return self.refuse_missing_soc(&mut state);
        };
        if data.timestamp.saturating_sub(state.last_control_ts) < self.config.control_period_s {
            return self.last_cmd.lock().unwrap_or_else(|e| e.into_inner()).clone();
        }
        state.last_control_ts = data.timestamp;

        // D-9：总表分相测量缺失 ⇒ 设计 §2.7 failsafe（积分冻结 + 斜坡回零 + 复位 Q 积分），
        // **不得**当作「有效零测量」喂进状态机（旧实现 MeterData::default() 的
        // p=0/u=220/pf=1.0 会令 S1 态被 p_base_est = p_st < −2 长期驻留 ≈ 持续充电）。
        let (p, q) = match data_to_meter(data) {
            Some(meter) => control(
                &mut state,
                &self.config,
                &meter,
                soc_pct / 100.0, // 百分比 → 0~1 小数
                data.timestamp,
            ),
            None => failsafe(&mut state, &self.config),
        };

        let cmd = ControlCommand {
            cmd_id: Self::CMD_ID,
            cmd_type: CommandType::ChargeDischarge,
            p_batt_set: Some(p.iter().sum()),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: Some(true),
            priority: 3,
            phase_p_set: Some(p),
            phase_q_set: Some(q),
        };
        *self.last_cmd.lock().unwrap_or_else(|e| e.into_inner()) = cmd.clone();
        cmd
    }

    /// 测试观测口（D-2/D-9/D-10 判别力用例）：跨周期状态快照；生产构建不编入。
    #[cfg(test)]
    pub(crate) fn state_snapshot(&self) -> TaiControllerState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 测试观测口（D-2）：SOC 全缺告警的**实际发射**次数。
    #[cfg(test)]
    pub(crate) fn soc_missing_warn_count(&self) -> u64 {
        self.soc_missing_warn_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 测试观测口（D-2）：把节流计时回拨，验证「节流到期后会再次告警」（非一次性）。
    #[cfg(test)]
    pub(crate) fn backdate_soc_missing_warn(&self, d: std::time::Duration) {
        let mut w = self
            .soc_missing_warned
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *w = Some(std::time::Instant::now() - d);
    }

    /// 测试观测口（B-3）：数据超期告警的**实际发射**次数。
    #[cfg(test)]
    pub(crate) fn stale_warn_count(&self) -> u64 {
        self.stale_warn_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 测试观测口（D-10）：在持有内部锁时 panic，令两把锁中毒（1 Hz 决策路径不得因中毒 panic）。
    #[cfg(test)]
    pub(crate) fn poison_locks_for_test(&self) {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let s = self.state.clone();
        let _ = catch_unwind(AssertUnwindSafe(move || {
            let _g = s.lock().unwrap_or_else(|e| e.into_inner());
            panic!("poison TaiStorageStrategy::state（测试注入）");
        }));
        let c = self.last_cmd.clone();
        let _ = catch_unwind(AssertUnwindSafe(move || {
            let _g = c.lock().unwrap_or_else(|e| e.into_inner());
            panic!("poison TaiStorageStrategy::last_cmd（测试注入）");
        }));
    }

    /// 测试观测口（D-10）：两把锁确已中毒（前提守护——中毒失败则本组用例空转）。
    #[cfg(test)]
    pub(crate) fn locks_are_poisoned(&self) -> bool {
        self.state.is_poisoned() && self.last_cmd.is_poisoned()
    }
}

/// DataPackage → MeterData；**无分相测量 ⇒ `None`**（调用方走设计 §2.7 failsafe）。
///
/// 旧实现返回 `MeterData::default()`（p=0/u=220/pf=1.0/i=0）——那是**假测量**，会被
/// 状态机当成"三相平衡的零功率"从而进入 S1/S2 并维持力指令（D-9 缺陷）。
fn data_to_meter(data: &DataPackage) -> Option<MeterData> {
    let phase = data.electrical.phase.as_ref()?;
    let get = |a: &[Option<f64>; 3]| {
        [
            a[0].unwrap_or(0.0),
            a[1].unwrap_or(0.0),
            a[2].unwrap_or(0.0),
        ]
    };
    let p_i = get(&phase.active_power);
    let q_i = get(&phase.reactive_power);
    let u = get(&phase.voltage);
    let i = get(&phase.current);
    let pf = get(&phase.cos_phi);
    Some(MeterData {
        p: p_i.iter().sum(),
        q: q_i.iter().sum(),
        pf,
        u,
        i,
        p_i,
        q_i,
    })
}

#[async_trait]
impl FallbackStrategy for TaiStorageStrategy {
    async fn evaluate(&self, data: &DataPackage) -> Result<ControlCommand, MupcError> {
        Ok(self.evaluate_sync(data))
    }

    fn strategy_type(&self) -> StrategyType {
        StrategyType::Fallback
    }

    fn name(&self) -> &str {
        "TaiStorageStrategy"
    }
}
