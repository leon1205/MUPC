/// 削峰填谷配置
#[derive(Debug, Clone)]
pub struct PeakShavingConfig {
    /// 峰时时段
    pub peak_hours: Vec<(u8, u8)>,
    /// 谷时时段
    pub valley_hours: Vec<(u8, u8)>,
    /// SOC 充电上限
    pub soc_charge_max: f64,
    /// SOC 充电下限
    pub soc_charge_min: f64,
    /// 电池容量 (kWh)
    pub battery_capacity: f64,
}

impl Default for PeakShavingConfig {
    fn default() -> Self {
        Self {
            peak_hours: vec![(8, 11), (18, 21)], // 08:00-11:00, 18:00-21:00
            valley_hours: vec![(23, 7)],         // 23:00-07:00
            soc_charge_max: 80.0,
            soc_charge_min: 20.0,
            battery_capacity: 100.0,
        }
    }
}

/// 需量控制配置
#[derive(Debug, Clone)]
pub struct DemandControlConfig {
    /// 变压器容量 (kVA)
    pub transformer_capacity: f64,
    /// 需量因子
    pub demand_factor: f64,
    /// 预警阈值
    pub warning_threshold: f64,
    /// 行动阈值
    pub action_threshold: f64,
    /// 紧急阈值
    pub emergency_threshold: f64,
}

impl Default for DemandControlConfig {
    fn default() -> Self {
        Self {
            transformer_capacity: 200.0,
            demand_factor: 0.85,
            warning_threshold: 0.80,
            action_threshold: 0.90,
            emergency_threshold: 0.95,
        }
    }
}

/// 防逆流配置
#[derive(Debug, Clone)]
pub struct AntiReverseConfig {
    /// 逆功率阈值 (kW)
    pub reverse_power_threshold: f64,
    /// 光伏限制步长
    pub pv_limit_step: f64,
    /// 最大充电功率 (kW)
    pub max_charge_power: f64,
    /// SOC 充电上限
    pub soc_charge_max: f64,
}

impl Default for AntiReverseConfig {
    fn default() -> Self {
        Self {
            reverse_power_threshold: -0.1,
            pv_limit_step: 0.10,
            max_charge_power: 50.0,
            soc_charge_max: 80.0,
        }
    }
}

/// 台区储能治理策略配置
///
/// 注：soc 相关字段均为 0~1 小数，与 DataPackage.battery.soc（百分比）在边界处转换
#[derive(Debug, Clone)]
pub struct TaiStorageConfig {
    /// 控制周期 (s)
    pub control_period_s: u64,
    /// S1 返送吸收触发阈值 (kW)：P_表 < -p_abs_trig 进入 S1
    /// 默认 2.0（2026-08-31 S1 激进调参标定，原 10.0）：捕获更小返送，
    /// 组合 kp=0.6/slope=6.0 下 7-04 返送时长 12.3%→9.4%、峰值 52.0→48.6kW、
    /// 能量 30.1→22.9kWh；6-27 5.1%→4.0%、21.4→11.0kW、6.9→3.8kWh。
    /// 代价：S1 触发更频繁 → 更多电池循环。
    pub p_abs_trig: f64,
    /// S3 高峰放电触发阈值 (kW)：P_表 > p_dis_trig 进入 S3
    pub p_dis_trig: f64,
    /// S1 退出阈值 (kW)：P_表 >= s1_exit 退出 S1（目标 +4，留 2kW 裕度）
    pub s1_exit: f64,
    /// S1 目标进口 (kW)
    pub p_tgt_s1: f64,
    /// S3 目标进口 (kW)
    pub p_tgt_s3: f64,
    /// 电池功率上限 (kW)
    pub p_cap: f64,
    /// 斜坡限速 (kW/周期)
    /// 默认 6.0（2026-08-31 S1 激进调参标定，原 5.0）：S1 充电更快达目标吸收功率，
    /// 7-04 返送 12.3%→9.4%、峰值 52.0→48.6kW；6-27 5.1%→4.0%、峰值 21.4→11.0kW。
    /// 注：标定曾试 slope=8 效果反而略差（6-27 峰值 16.6kW）且使 arbitrate 重归一
    /// 遗留 0.9A 过限（test_arbitrate_recomputes_and_breaks 失败），故取 6.0 平衡。
    /// 代价：斜坡更快 → 响应过冲风险略增。
    pub slope: f64,
    /// 共模 P 积分增益
    /// 默认 0.6（2026-08-31 S1 激进调参标定，原 0.4）：共模响应更快，
    /// 配合 slope=6 更有效压降返送。代价：增益更高 → 临界点振荡/超调风险略增。
    pub kp: f64,
    /// 差模 P 积分增益
    pub k_diff: f64,
    /// 无功积分增益
    pub k_q: f64,
    /// 无功积分方向符号（±1，按表计/PCS 约定；发散则翻转）
    pub s_q_sign: f64,
    /// 差模上限 (kW/相)
    pub dp_max: f64,
    /// 无功上限 (kVAr/相)
    pub q_i_max: f64,
    /// 每相/中线电流额定 (A)
    pub i_rated: f64,
    /// 总视在额定 (kVA)
    pub s_rated: f64,
    /// 分时 SOC 上限（18:00 前，0~1 小数；DataPackage.battery.soc 为百分比，边界处需 /100 转换）
    pub soc_cap_day: f64,
    /// SOC 滞回（0~1 小数）
    pub soc_hys: f64,
    /// 分时 SOC 上限释放时刻（当日秒）
    pub t_release_secs: f64,
    /// S4 清空起点（当日秒）
    pub t_clear_start_secs: f64,
    /// S4 清空截止（当日秒，达标目标）
    pub t_clear_end_secs: f64,
    /// S4 日终清空限幅裕度 (kW)：>0 时 P_强制 = min(P_强制, P_表 + 裕度)，避免夜间过度反送（0 = 不限幅，保持满额清空）
    pub s4_limit_margin_kw: f64,
    /// S3 放电裕度限幅（防负荷回落过冲返送）：true 时 S3 放电不超当前负荷裕度
    /// `p_st = min(p_st, (P_表 - p_tgt_s3).max(0))`，负荷回落时放电即时跟随，杜绝过冲返送。
    /// 稳态下 S3 目标即 p_st = P_表 - p_tgt_s3，故该钳位不影响目标跟踪，仅拦截积分过冲。
    /// 默认 true（2026-08-31 S3 专项回放：两日控制后返送均降至基线以下且 SOC 日终仍达 10% 地板）
    pub s3_margin_limit: bool,
    /// S1 动态斜坡开关（返送陡增加速充电 / 受电快升快速退出）
    pub s1_boost_enabled: bool,
    /// S1 斜坡放大倍数（返送陡增时 slope×factor）
    pub s1_boost_factor: f64,
    /// S1 返送陡增变化率阈值 (kW/周期)：Δp < -thr 视为返送陡然变大
    pub s1_boost_rate_thr: f64,
    /// S1 受电快速上升变化率阈值 (kW/周期)：Δp > +thr 且充电未变时快速退出
    pub s1_cut_rate_thr: f64,
    /// S1 净进口接近 0 判定 (kW)：p > -near_zero 视为返送已吸收到接近 0，停止加速充电
    pub s1_near_zero_thr: f64,
    /// 滑动滤波窗口（点数）
    pub window_size: u32,
    /// 电池容量 (kWh)
    pub battery_capacity_kwh: f64,
}

impl Default for TaiStorageConfig {
    fn default() -> Self {
        Self {
            control_period_s: 60,
            p_abs_trig: 2.0, // 2026-08-31 S1 激进调参标定（原 10.0）：更强返送吸收
            p_dis_trig: 30.0,
            s1_exit: 4.0,
            p_tgt_s1: 2.0, // 保持 +2 设计裕度（p_tgt_s1=0 在回放中效果更差且有临界点振荡风险）
            p_tgt_s3: 5.0,
            p_cap: 60.0,
            slope: 6.0, // 2026-08-31 S1 激进调参标定（原 5.0；8.0 使 arbitrate 重归一过限 0.9A，弃）
            kp: 0.6,    // 2026-08-31 S1 激进调参标定（原 0.4）
            k_diff: 0.4,
            k_q: 0.4,
            s_q_sign: 1.0,
            dp_max: 40.0,
            q_i_max: 30.0,
            i_rated: 190.0,
            s_rated: 125.0,
            soc_cap_day: 0.70,
            soc_hys: 0.03,
            t_release_secs: 18.0 * 3600.0,     // 18:00
            t_clear_start_secs: 21.0 * 3600.0, // 21:00
            t_clear_end_secs: 23.5 * 3600.0,   // 23:30
            s4_limit_margin_kw: 0.0,
            s3_margin_limit: true,
            s1_boost_enabled: true,
            s1_boost_factor: 3.0,
            s1_boost_rate_thr: 15.0,
            s1_cut_rate_thr: 15.0,
            s1_near_zero_thr: 5.0,
            window_size: 5,
            battery_capacity_kwh: 120.0,
        }
    }
}
