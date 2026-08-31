//! 台区储能治理策略（第 4 策略）
//!
//! AI 失效兜底时，通过台区储能 PCS 分相 P/Q 控制实现：
//! 降返送、降三相不平衡度、提功率因数。
//! 设计见 04-MUPC-策略引擎-设计文档 §15。

use crate::config::TaiStorageConfig;
use std::collections::VecDeque;

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
fn arbitrate(
    pcmd: &mut [f64; 3],
    q: &mut [f64; 3],
    state: &mut TaiControllerState,
    config: &TaiStorageConfig,
    u: &[f64; 3],
) {
    for _ in 0..8 {
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
            break;
        }
    }
    // 总有功 ≤ p_cap（共模限制；差模零净）
    state.p_st = state.p_st.clamp(-config.p_cap, config.p_cap);
    // 差模重归一 ΣΔP=0
    let d_sum = state.d_p.iter().sum::<f64>() / 3.0;
    for v in state.d_p.iter_mut() {
        *v -= d_sum;
    }
    for (i, v) in pcmd.iter_mut().enumerate() {
        *v = state.p_st / 3.0 + state.d_p[i];
    }
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

    // 2. failsafe：数据异常 → 斜坡回归 0，保持最近有效 Q
    if !p.is_finite() || pi.iter().any(|x| !x.is_finite()) {
        state.p_st = move_toward(state.p_st, 0.0, config.slope);
        for i in 0..3 {
            state.d_p[i] = move_toward(state.d_p[i], 0.0, config.slope);
        }
        state.meter_buf.clear();
        let pcmd = [
            state.p_st / 3.0 + state.d_p[0],
            state.p_st / 3.0 + state.d_p[1],
            state.p_st / 3.0 + state.d_p[2],
        ];
        return (pcmd, state.q_last);
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
    if u.iter().any(|&x| x > 235.0) {
        p_force = p_force.min(p.max(0.0)); // 电压越限保护
    }

    state.st = if secs >= config.t_clear_start_secs && soc > 0.10 {
        TaiState::S4Clear
    } else {
        match state.st {
            TaiState::S1PvAbsorb => {
                if p < config.s1_exit && soc < soc_cap {
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
                if p < -config.p_abs_trig && soc < soc_cap - config.soc_hys {
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

    // 5. 共模 P（增量式积分 + 斜坡限速）
    state.p_st = match state.st {
        TaiState::S1PvAbsorb => {
            let inc = (config.kp * (p - config.p_tgt_s1)).clamp(-config.slope, config.slope);
            (state.p_st + inc).clamp(-config.p_cap, 0.0)
        }
        TaiState::S2Flat => move_toward(state.p_st, 0.0, config.slope),
        TaiState::S3Peak => {
            let inc = (config.kp * (p - config.p_tgt_s3)).clamp(-config.slope, config.slope);
            (state.p_st + inc).clamp(0.0, config.p_cap)
        }
        TaiState::S4Clear => move_toward(state.p_st, p_force, config.slope),
    };
    state.p_st = soc_protect(state.p_st, soc);

    // 6. 差模 P（积分式，零净能量；I_i 带符号）
    let imean = ii.iter().sum::<f64>() / 3.0;
    let i_max = ii.iter().cloned().fold(0.0f64, f64::max);
    let i_min = ii.iter().cloned().fold(f64::MAX, f64::min);
    let unbal = if i_max < 1.0 {
        0.0
    } else {
        (1.0 - i_min / i_max) * 100.0
    };
    if unbal < 15.0 {
        state.d_p_active = false;
    } else if unbal > 25.0 {
        state.d_p_active = true;
    }
    for i in 0..3 {
        let inc = config.k_diff * u[i] * (ii[i] - imean);
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
