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
