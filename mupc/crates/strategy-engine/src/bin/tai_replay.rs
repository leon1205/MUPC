//! 台区储能治理策略离线回放工具
//!
//! 读取 data_rule xlsx（sheet「总表」），按 60s 控制周期调用
//! TaiStorageStrategy::evaluate_sync 逐周期回放，统计 KPI 并打印报告。
//!
//! **net 反馈模型（匹配运行时闭环）：** 台区总表测量的是净功率（储能输出
//! 在测量环内）。回放以「基线 − 储能当前输出（上一周期指令）」作为表计
//! 测量送入控制器，控制器下一周期输出经反馈后再影响测量，形成真实闭环。
//!
//! 用法: cargo run -p mupc-strategy-engine --bin tai_replay -- <xlsx路径> [SOC初值0.0-1.0] [soc_cap_day] [s4_limit_margin_kw] [s3_margin 0|1]

use calamine::{open_workbook, Data, DataType, Reader, Xlsx};
use chrono::NaiveDateTime;
use mupc_data_processing::telemetry::{
    BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus, PhaseElectricalData,
};
use mupc_strategy_engine::{TaiStorageConfig, TaiStorageStrategy};
use std::env;

// xlsx「总表」列索引
const COL_TIME: usize = 0;
const COL_U: usize = 1; // U_A/B/C = 1,2,3
const COL_I: usize = 4; // I_A/B/C = 4,5,6
const COL_P_TOTAL: usize = 7;
const COL_PI: usize = 8; // P_A/B/C = 8,9,10
const COL_QI: usize = 12; // Q_A/B/C = 12,13,14
const COL_PFI: usize = 20; // PF_A/B/C = 20,21,22
const COL_UNBAL: usize = 39; // 三相不平衡度（基线）

fn f(row: &[Data], i: usize) -> f64 {
    row.get(i).and_then(|d| d.get_float()).unwrap_or(0.0)
}

fn parse_time(s: &str) -> Option<i64> {
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(nt) = NaiveDateTime::parse_from_str(s.trim(), fmt) {
            return Some(nt.and_utc().timestamp());
        }
    }
    None
}

/// 从时间单元格解析 unix 秒：data_rule xlsx 的时间列以 datetime 序列存储，
/// 部分版本也可能为字符串，故两种均支持。
fn cell_timestamp(d: &Data) -> Option<i64> {
    if let Some(dt) = d.get_datetime() {
        if let Some(nd) = dt.as_datetime() {
            return Some(nd.and_utc().timestamp());
        }
    }
    if let Some(s) = d.get_string() {
        if let Some(t) = parse_time(s) {
            return Some(t);
        }
    }
    None
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("用法: tai_replay <xlsx路径> [SOC初值]");
    let soc_init: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.50);
    if soc_init != soc_init.clamp(0.10, 0.90) {
        eprintln!(
            "警告: SOC 初值超出 [0.10, 0.90]，已裁剪为 {}",
            soc_init.clamp(0.10, 0.90)
        );
    }
    let soc_init = soc_init.clamp(0.10, 0.90);

    let mut workbook: Xlsx<_> = open_workbook(path).expect("无法打开 xlsx");
    let range = workbook
        .worksheet_range("总表")
        .expect("找不到「总表」sheet");

    let mut cfg = TaiStorageConfig::default();
    // 可选参数覆盖：args[3]=soc_cap_day，args[4]=s4_limit_margin_kw，args[5]=s3_margin(0|1)
    if let Some(v) = args.get(3) {
        cfg.soc_cap_day = v.parse().unwrap_or(cfg.soc_cap_day);
    }
    if let Some(v) = args.get(4) {
        cfg.s4_limit_margin_kw = v.parse().unwrap_or(cfg.s4_limit_margin_kw);
    }
    if let Some(v) = args.get(5) {
        cfg.s3_margin_limit = v == "1";
    }
    let strategy = TaiStorageStrategy::new(cfg.clone());

    // 回放统计
    let mut soc = soc_init;
    let mut last_ts: i64 = 0;
    let mut last_p_set: Option<[f64; 3]> = None;
    let mut last_q_set: Option<[f64; 3]> = None;
    // net 反馈：最近一次控制指令的分相 P/Q 输出（初始为 0，尚未放电）
    let mut last_p_out = [0.0; 3];
    let mut last_q_out = [0.0; 3];
    let mut n_samples = 0usize;
    let mut n_control = 0usize;
    let mut reverse_base_secs = 0.0; // 无储能返送时长
    let mut reverse_ctrl_secs = 0.0; // 有储能返送时长
    let mut reverse_base_peak = 0.0f64; // 无储能返送峰值
    let mut reverse_ctrl_peak = 0.0f64;
    let mut unbal_base_ok_secs = 0.0; // 基线不平衡<20% 时长
    let mut unbal_ctrl_ok_secs = 0.0; // 控制后不平衡<20% 时长
    let mut pf_good_secs = 0.0; // 基线分相 PF>0.95 时长（控制未建模 Q 补偿，取基线）
    let mut soc_min = f64::MAX;
    let mut soc_max = f64::MIN;
    let mut reverse_ctrl_energy = 0.0; // 有储能返送能量 kwh
    let mut total_secs = 0.0; // 回放总时长（秒，各 KPI 占比的分母）
    let mut prev_ts: i64 = 0;
    // 削峰诊断（评估 net 反馈下 S3 clamp 是否存在欠输送）：基线处于高峰
    // （> p_dis_trig）时的净进口均值 + 储能放电总能量
    let mut peak_net_sum = 0.0;
    let mut peak_net_n = 0usize;
    let mut discharge_energy = 0.0; // 储能放电总能量 kwh
    let mut charge_energy = 0.0; // 储能充电总能量 kwh
    let mut floor_secs = 0.0; // SOC 处于 10% 地板时长（诊断）
    let mut import_peak_base = 0.0f64; // 基线受电峰值（削峰诊断）
    let mut import_peak_ctrl = 0.0f64; // 控制后受电峰值（削峰诊断）

    for row in range.rows().skip(1) {
        let ts = match row.get(COL_TIME).and_then(cell_timestamp) {
            Some(t) => t,
            None => continue,
        };
        // 首行 prev_ts == 0 无法求差值，假设 60s 控制间隔；后续行按实际时间差
        let dt = if prev_ts == 0 {
            60.0
        } else {
            (ts - prev_ts) as f64
        };

        let p_total = f(row, COL_P_TOTAL);
        let pi = [f(row, COL_PI), f(row, COL_PI + 1), f(row, COL_PI + 2)];
        let qi = [f(row, COL_QI), f(row, COL_QI + 1), f(row, COL_QI + 2)];
        let u = [f(row, COL_U), f(row, COL_U + 1), f(row, COL_U + 2)];
        let i_mag = [f(row, COL_I), f(row, COL_I + 1), f(row, COL_I + 2)];
        let pfi = [f(row, COL_PFI), f(row, COL_PFI + 1), f(row, COL_PFI + 2)];
        let unbal_base = f(row, COL_UNBAL);

        // 基线（无储能）KPI
        if p_total < 0.0 {
            reverse_base_secs += dt;
            reverse_base_peak = reverse_base_peak.max(-p_total);
        }
        if unbal_base < 20.0 {
            unbal_base_ok_secs += dt;
        }

        // 生效输出：更新前的 last_p_out/last_q_out（上一控制点发出的命令，
        // 覆盖 [t_{k-1}, t_k] 间隔）。net 测量、KPI/SOC 记账一律用它——
        // 控制周期行上 evaluate_sync 产生的新命令要到下一间隔才生效。
        let p_out_prev = last_p_out;
        let q_out_prev = last_q_out;

        // net 反馈模型：台区总表测量净功率 = 基线 − 储能生效输出（上一周期指令），
        // 与运行时闭环一致（储能输出在测量环内）。电压/电流幅值/PF 取基线
        // （简化，见计划文档说明），带符号电流方向以净分相有功符号承载。
        let p_i_net = [
            pi[0] - p_out_prev[0],
            pi[1] - p_out_prev[1],
            pi[2] - p_out_prev[2],
        ];
        let q_i_net = [
            qi[0] - q_out_prev[0],
            qi[1] - q_out_prev[1],
            qi[2] - q_out_prev[2],
        ];
        let p_net_total = p_i_net.iter().sum();

        // 控制：按 60s 周期节流
        let (cmd_p, cmd_q) = if last_ts == 0 || ts.saturating_sub(last_ts) as u64 >= cfg.control_period_s {
            // 契约：分相电流须带符号（正=受电 / 负=返送），xlsx 仅给幅值，
            // 以净分相有功符号承载方向。
            let i_sign = [
                p_i_net[0].signum() * i_mag[0],
                p_i_net[1].signum() * i_mag[1],
                p_i_net[2].signum() * i_mag[2],
            ];
            let phase = PhaseElectricalData {
                voltage: [Some(u[0]), Some(u[1]), Some(u[2])],
                current: [Some(i_sign[0]), Some(i_sign[1]), Some(i_sign[2])],
                active_power: [Some(p_i_net[0]), Some(p_i_net[1]), Some(p_i_net[2])],
                reactive_power: [Some(q_i_net[0]), Some(q_i_net[1]), Some(q_i_net[2])],
                cos_phi: [Some(pfi[0]), Some(pfi[1]), Some(pfi[2])],
            };
            let pkg = DataPackage {
                timestamp: ts as u64,
                electrical: ElectricalData {
                    voltage: Some(u[0]),
                    current: Some(i_mag[0]),
                    active_power: Some(p_net_total),
                    reactive_power: Some(q_i_net.iter().sum()),
                    cos_phi: Some(pfi[0]),
                    frequency: Some(50.0),
                    phase: Some(phase),
                },
                device_status: DeviceStatus {
                    inverter_status: InverterStatus::Running,
                    pv_power: None,
                    load_power: None,
                    ev_charger_power: None,
                },
                battery: BatteryData {
                    soc: Some(soc * 100.0),
                    soh: None,
                    temperature: None,
                },
            };
            let c = strategy.evaluate_sync(&pkg);
            last_ts = ts;
            n_control += 1;
            (c.phase_p_set, c.phase_q_set)
        } else {
            (last_p_set, last_q_set)
        };
        last_p_set = cmd_p;
        last_q_set = cmd_q;
        // 储能当前输出 = 最新指令（供下一周期测量反馈；本行 KPI/SOC 用生效输出 p_out_prev）
        last_p_out = cmd_p.unwrap_or([0.0; 3]);
        last_q_out = cmd_q.unwrap_or([0.0; 3]);

        // 控制后净功率 = 基线 - 储能注入（生效间隔输出 p_out_prev）
        let p_st = p_out_prev.iter().sum::<f64>();
        let p_ctrl = p_total - p_st; // 储能放电 p_st>0 → 净进口下降
        if p_ctrl < 0.0 {
            reverse_ctrl_secs += dt;
            reverse_ctrl_peak = reverse_ctrl_peak.max(-p_ctrl);
            reverse_ctrl_energy += -p_ctrl * dt / 3600.0;
        }
        // 削峰诊断：基线高峰窗口内的净进口均值（欠输送 → 均值偏高）
        if p_total > cfg.p_dis_trig {
            peak_net_sum += p_ctrl;
            peak_net_n += 1;
        }
        if p_st > 0.0 {
            discharge_energy += p_st * dt / 3600.0;
        } else {
            charge_energy += -p_st * dt / 3600.0;
        }
        if soc <= 0.1001 {
            floor_secs += dt;
        }
        import_peak_base = import_peak_base.max(p_total);
        import_peak_ctrl = import_peak_ctrl.max(p_ctrl);

        // 控制后不平衡：以基线不平衡近似（差模均衡后电流差异应下降，此处保守取基线）
        let unbal_ctrl = unbal_base;
        if unbal_ctrl < 20.0 {
            unbal_ctrl_ok_secs += dt;
        }
        if pfi.iter().all(|&x| x.abs() > 0.95) {
            pf_good_secs += dt;
        }

        // SOC 积分（P_st 负=充电）
        soc += -p_st * dt / 3600.0 / cfg.battery_capacity_kwh;
        soc = soc.clamp(0.10, 0.90);
        soc_min = soc_min.min(soc);
        soc_max = soc_max.max(soc);
        prev_ts = ts;
        total_secs += dt;
        n_samples += 1;
    }

    let pct = |x: f64| x / total_secs * 100.0;
    println!("=== 台区储能治理策略回放报告 ===");
    println!("数据源: {}", path);
    println!("样本数: {}  控制周期: {}", n_samples, n_control);
    println!(
        "返送时长占比: 基线 {:.1}% → 控制后 {:.1}%",
        pct(reverse_base_secs),
        pct(reverse_ctrl_secs)
    );
    println!(
        "返送峰值(kW): 基线 {:.1} → 控制后 {:.1}",
        reverse_base_peak, reverse_ctrl_peak
    );
    println!(
        "不平衡<20% 达标占比: 基线 {:.1}% → 控制后 {:.1}%",
        pct(unbal_base_ok_secs),
        pct(unbal_ctrl_ok_secs)
    );
    println!("基线分相 PF>0.95 占比: {:.1}%", pct(pf_good_secs));
    println!(
        "SOC 范围: {:.1}% ~ {:.1}%  日终: {:.1}%",
        soc_min * 100.0,
        soc_max * 100.0,
        soc * 100.0
    );
    println!("控制后返送能量: {:.1} kWh", reverse_ctrl_energy);
    if peak_net_n > 0 {
        println!(
            "削峰诊断(基线>{}kW 时段): 净进口均值 {:.1} kW / 放电 {:.1} kWh / 充电 {:.1} kWh / SOC地板占比 {:.1}%",
            cfg.p_dis_trig,
            peak_net_sum / peak_net_n as f64,
            discharge_energy,
            charge_energy,
            pct(floor_secs)
        );
    }
    println!(
        "受电峰值(kW): 基线 {:.1} → 控制后 {:.1}",
        import_peak_base, import_peak_ctrl
    );
}
