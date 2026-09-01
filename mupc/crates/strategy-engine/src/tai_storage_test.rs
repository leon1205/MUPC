#[cfg(test)]
mod tai_storage_test {
    use crate::config::TaiStorageConfig;
    use crate::tai_storage::{control, move_toward, MeterData, TaiControllerState, TaiState};

    fn meter(p: f64, pi: [f64; 3], qi: [f64; 3], u: [f64; 3], pfi: [f64; 3]) -> MeterData {
        MeterData {
            p,
            q: qi.iter().sum(),
            pf: pfi,
            u,
            // 带符号电流近似（≈P/U，符号一致，幅值差异用于触发不平衡）
            i: [pi[0], pi[1], pi[2]],
            p_i: pi,
            q_i: qi,
        }
    }

    #[test]
    fn test_s2_flat_no_output_when_normal() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 平衡负载（三相一致）→ unbal=0 → 差模不动作 → 输出 [0,0,0]
        let m = meter(6.0, [2.0, 2.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, q) = control(&mut st, &cfg, &m, 0.5, 3600 * 10); // 10:00
        assert_eq!(st.st, TaiState::S2Flat);
        assert_eq!(p, [0.0; 3]);
        assert_eq!(q, [0.0; 3]);
    }

    #[test]
    fn test_s1_absorb_on_reverse() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 白天 12:00，总表返送 -30kW（三相各有返送）
        let m = meter(
            -30.0,
            [-10.0, -10.0, -10.0],
            [0.0; 3],
            [220.0; 3],
            [0.99; 3],
        );
        let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 12);
        assert_eq!(st.st, TaiState::S1PvAbsorb);
        // 共模应充电（p_st < 0，分相 P 之和 = p_st）
        let sum: f64 = p.iter().sum();
        assert!(sum < 0.0, "S1 应充电吸收返送，p 之和={}", sum);
    }

    #[test]
    fn test_s1_feedforward_absorbs_to_target_import() {
        // v2.22 前馈：返送骤增，一周期吸收到目标进口 +2（替代积分爬坡滞后）
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        st.st = TaiState::S1PvAbsorb;
        // 净 −50（基线返送，储能尚未输出）
        let m = meter(
            -50.0,
            [-20.0, -15.0, -15.0],
            [0.0; 3],
            [220.0; 3],
            [0.99; 3],
        );
        let _ = control(&mut st, &cfg, &m, 0.5, 3600 * 12);
        assert_eq!(st.st, TaiState::S1PvAbsorb);
        // p_base_est = -50 + 0 = -50，target = -50 - 2 = -52，一周期到位
        assert!(
            (st.p_st + 52.0).abs() < 1.0,
            "前馈应一周期吸收到基线-目标: {}",
            st.p_st
        );
        // 净功率 = 基线(-50) - 储能输出(-52) = +2 = 目标进口
        let net = -50.0 - st.p_st;
        assert!(
            (net - cfg.p_tgt_s1).abs() < 1.0,
            "净功率应到 +2: {}",
            net
        );
    }

    #[test]
    fn test_s1_feedforward_reduces_charge_on_reverse_decline() {
        // v2.22：返送减小仍返送（储能超吸收，净从电网取电超目标）→ 降载不停充，
        // 把从电网取电压回 +2（12:01 场景；旧"停充"会让返送反弹回基线）。
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        st.st = TaiState::S1PvAbsorb;
        st.p_st = -60.0; // 深度充电（p_cap 上限）
        // 基线返送降到 −53.7，储能 −60 超吸收 → 净 = +6.3（从电网取电 6.3）
        let m = meter(
            6.3,
            [2.1, 2.1, 2.1],
            [0.0; 3],
            [220.0; 3],
            [0.99; 3],
        );
        let _ = control(&mut st, &cfg, &m, 0.5, 3600 * 12);
        // p_base_est = 6.3 + (-60) = -53.7 < s1_exit=4 → S1 保持
        assert_eq!(st.st, TaiState::S1PvAbsorb, "基线仍返送，S1 应保持");
        // target = -53.7 - 2 = -55.7：降载不停充，从电网取电压回 2
        assert!(
            (st.p_st + 55.7).abs() < 1.0,
            "应降载到 -55.7 不停充: {}",
            st.p_st
        );
        let net = -53.7 - st.p_st;
        assert!(
            (net - cfg.p_tgt_s1).abs() < 1.0,
            "从电网取电应回到 +2: {}",
            net
        );
    }

    #[test]
    fn test_s1_feedforward_stops_on_import() {
        // v2.22：基线骤转受电 → S1 保持并大步回 0（避免 S2 慢斜坡期间从电网取电）
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        st.st = TaiState::S1PvAbsorb;
        st.p_st = -30.0; // 之前充电吸收返送
        // 基线骤转受电 +20（净 = 20 - (-30) = 50）
        let m = meter(
            50.0,
            [17.0, 17.0, 16.0],
            [0.0; 3],
            [220.0; 3],
            [0.99; 3],
        );
        let _ = control(&mut st, &cfg, &m, 0.5, 3600 * 12);
        // 储能未回归（p_st=-30）→ S1 保持，大步斜坡回 0
        assert_eq!(st.st, TaiState::S1PvAbsorb);
        assert!(
            st.p_st.abs() < 1.0,
            "基线受电应大步停充回 0: {}",
            st.p_st
        );
    }

    #[test]
    fn test_s1_feedforward_steady_reverse_no_oscillation() {
        // v2.22：持续返送 → 前馈目标稳定，净功率恒 +2，无振荡（含储能自激场景：
        // 储能加深充电使下周期净收窄，但重构基线不变 → 目标不变 → 无极限环）
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        st.st = TaiState::S1PvAbsorb;
        // 周期1：净 −40（储能初始 0），基线返送 −40
        let m1 = meter(
            -40.0,
            [-14.0, -13.0, -13.0],
            [0.0; 3],
            [220.0; 3],
            [0.99; 3],
        );
        let _ = control(&mut st, &cfg, &m1, 0.5, 3600 * 12);
        let p_st1 = st.p_st;
        // 周期2：基线仍 −40，储能 p_st1 生效 → 净 = -40 - p_st1（接近 +2）
        let m2 = meter(
            -40.0 - p_st1,
            [-14.0, -13.0, -13.0],
            [0.0; 3],
            [220.0; 3],
            [0.99; 3],
        );
        let _ = control(&mut st, &cfg, &m2, 0.5, 3600 * 12);
        // 目标不变（重构基线恒 −40）→ p_st 保持，无自激退出
        assert!(
            (st.p_st - p_st1).abs() < 1.0,
            "持续返送目标稳定，p_st 不应振荡: {} → {}",
            p_st1, st.p_st
        );
        let net = -40.0 - st.p_st;
        assert!(
            (net - cfg.p_tgt_s1).abs() < 2.0,
            "净功率应稳定在 +2 附近: {}",
            net
        );
    }

    #[test]
    fn test_s3_discharge_on_high_load() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        let m = meter(50.0, [20.0, 15.0, 15.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 16);
        assert_eq!(st.st, TaiState::S3Peak);
        let sum: f64 = p.iter().sum();
        assert!(sum > 0.0, "S3 应放电，p 之和={}", sum);
    }

    #[test]
    fn test_s3_margin_limit_prevents_overshoot() {
        let mut cfg = TaiStorageConfig::default();
        cfg.s3_margin_limit = true;
        let mut st = TaiControllerState::default();
        // p=10 不满足进入 S3 的阈值（p_dis_trig=30），直接强制置 S3 以直测限幅分支
        st.st = TaiState::S3Peak;
        // 模拟 S3 已累积大量放电（负荷快速回落前的过冲场景）
        st.p_st = 40.0;
        // 负荷回落到 10kW（目标 5kW）→ 放电应被钳到 ≤5kW，防返送
        let m = meter(10.0, [4.0, 3.0, 3.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let _ = control(&mut st, &cfg, &m, 0.5, 3600 * 16);
        assert_eq!(st.st, TaiState::S3Peak, "S3 应保持（p_st>0 或 p>目标）");
        assert!(
            st.p_st <= 5.0 + 1e-9,
            "S3 限幅应使放电 ≤ 负荷裕度 (10-5): {}",
            st.p_st
        );
        // 对照：关闭限幅时放电不受裕度约束（斜坡/积分继续推高）
        let mut cfg_off = TaiStorageConfig::default();
        cfg_off.s3_margin_limit = false;
        let mut st_off = TaiControllerState::default();
        st_off.st = TaiState::S3Peak;
        st_off.p_st = 40.0;
        let _ = control(&mut st_off, &cfg_off, &m, 0.5, 3600 * 16);
        assert!(
            st_off.p_st > 5.0 + 1e-9,
            "关闭限幅时应保持超裕度放电（对照）: {}",
            st_off.p_st
        );
    }

    #[test]
    fn test_dp_zero_net_energy() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 不平衡：B 相电流大（制造 unbal >25%）
        let m = meter(10.0, [2.0, 6.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        st.d_p_active = true;
        for _ in 0..5 {
            let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 10);
            let sum: f64 = p.iter().sum();
            assert!(
                (sum - st.p_st).abs() < 1e-6,
                "ΣΔP 应守恒: sum={} p_st={}",
                sum,
                st.p_st
            );
        }
        let dsum: f64 = st.d_p.iter().sum();
        assert!(dsum.abs() < 1e-6, "ΣΔP 应=0: {}", dsum);
    }

    #[test]
    fn test_s4_force_discharge_to_soc_floor() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 22:00 已过清空起点，SOC=0.5 → S4 强制放电
        let m = meter(5.0, [2.0, 1.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 22);
        assert_eq!(st.st, TaiState::S4Clear);
        let sum: f64 = p.iter().sum();
        assert!(sum > 0.0, "S4 应强制放电: {}", sum);
    }

    #[test]
    fn test_s4_limit_margin_reduces_force() {
        let mut cfg = TaiStorageConfig::default();
        cfg.s4_limit_margin_kw = 10.0;
        let mut st = TaiControllerState::default();
        // 22:00 S4，夜间低负荷 p=15 → 限幅后 p_force ≤ 25
        let m = meter(15.0, [5.0, 5.0, 5.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let _ = control(&mut st, &cfg, &m, 0.5, 3600 * 22);
        assert_eq!(st.st, TaiState::S4Clear);
        let sum: f64 = st.p_st;
        assert!(sum <= 25.0 + 1e-9, "S4 限幅应约束 p_st ≤ 25: {}", sum);
    }

    #[test]
    fn test_q_channel_compensates_low_pf() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // PF=0.9 < 0.95 → Q 通道激活。注意：积分输入是表计分相无功 qi，
        // 若 qi=0 则 inc=s_q_sign*k_q*qi=0，Q 永远不会注入。
        // 故给非零无功 qi=[5,5,5]，使 q_pcs += s_q_sign*k_q*qi 真实累积。
        let m = meter(5.0, [2.0, 2.0, 2.0], [5.0, 5.0, 5.0], [220.0; 3], [0.90; 3]);
        for _ in 0..3 {
            let (_, q) = control(&mut st, &cfg, &m, 0.5, 3600 * 10);
            // 至少一相 Q 非零（朝向补偿方向）
            assert!(q.iter().any(|x| x.abs() > 1e-9), "Q 应注入补偿: {:?}", q);
            assert!(q.iter().all(|x| x.abs() <= cfg.q_i_max + 1e-9));
        }
    }

    #[test]
    fn test_arbitrate_clips_overcurrent() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 制造过流：显式播种差模出力（I-3 修复后单周期差模仅 ±slope 增量，
        // 无法由 step6 一次拉满单相，故直接注入已累积的 d_p）。
        // p_st=30 → S2 斜坡降 5 → 25；d_p=[40,-20,-20] 经 step6 微调后
        // A 相合成 ≈ 43.3kW → 196.97A > i_rated=190 → arbitrate 必须裁剪。
        st.p_st = 30.0;
        st.d_p = [40.0, -20.0, -20.0];
        st.d_p_active = true;
        let m = meter(10.0, [2.0, 6.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, q) = control(&mut st, &cfg, &m, 0.5, 3600 * 10);
        // 单相电流 ≤ i_rated + 5（裁剪后应回到限值内）
        for i in 0..3 {
            let s = (p[i].powi(2) + q[i].powi(2)).sqrt();
            let i_phase = s * 1000.0 / 220.0;
            assert!(
                i_phase <= cfg.i_rated + 5.0,
                "相{}电流超限: {:.1}A (P={:.1})",
                i,
                i_phase,
                p[i]
            );
        }
        // 总有功 ≤ p_cap
        assert!(p.iter().sum::<f64>().abs() <= cfg.p_cap + 1e-6);
    }

    #[test]
    fn test_arbitrate_recomputes_and_breaks() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 制造单相过流：p_st=30（S2 斜坡降 5 → 25）、d_p=[40,-20,-20]，
        // 不平衡表计 [2,6,2]（unbal≈67%）→ 仲裁前 A 相合成 ≈ 43.3kW → 196.97A > i_rated。
        // I-2 修复：每轮顶格重算 pcmd、干净即 break，避免陈旧 pcmd 导致 8×slope 过剪。
        st.p_st = 30.0;
        st.d_p = [40.0, -20.0, -20.0];
        st.d_p_active = true;
        let m = meter(10.0, [2.0, 6.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, q) = control(&mut st, &cfg, &m, 0.5, 3600 * 10);
        // ① 各相电流回到限值内（裁剪后）
        for i in 0..3 {
            let s = (p[i].powi(2) + q[i].powi(2)).sqrt();
            let i_phase = s * 1000.0 / 220.0;
            assert!(
                i_phase <= cfg.i_rated + 1e-6,
                "相{}电流超限: {:.1}A (P={:.1})",
                i,
                i_phase,
                p[i]
            );
        }
        // ② ΣΔP=0 重归一 + 共模守恒：Σp = p_st
        let dsum: f64 = st.d_p.iter().sum();
        assert!(dsum.abs() < 1e-6, "ΣΔP 应=0: {}", dsum);
        let psum: f64 = p.iter().sum();
        assert!(
            (psum - st.p_st).abs() < 1e-6,
            "Σp 应=p_st: {} vs {}",
            psum,
            st.p_st
        );
        // ③ 只裁剪到限值附近，而非陈旧 pcmd 的 8×slope 过剪（修复前 A 相会被剪到 ≈83A）
        let i_a = (p[0].powi(2) + q[0].powi(2)).sqrt() * 1000.0 / 220.0;
        assert!(
            i_a > 180.0,
            "A 相被过度裁剪（应仅剪到限值附近而非 8×slope 过剪）: {:.1}A",
            i_a
        );
        assert!(st.d_p[0] > 20.0, "A 相差模被过度裁剪: {:.1}", st.d_p[0]);
    }

    #[test]
    fn test_diff_p_slope_limited() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 强不平衡表计 [2,6,2]（unbal≈67%），B 相原始差模增量 ≈ 235kW/周期 ≫ slope。
        // I-3 修复：inc 先被钳到 ±slope(5)，单周期 d_p 跳变受限，避免一次拉满 dp_max(40)。
        // 注：arbitrate 末尾的 ΣΔP=0 重归一会把最大增量再平移至多 slope，
        // 故单周期后的 |d_p| 上界为 2·slope（修复前会被一次积分推到 ≈53kW）。
        st.d_p_active = true;
        st.d_p = [0.0; 3];
        let m = meter(10.0, [2.0, 6.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let _ = control(&mut st, &cfg, &m, 0.5, 3600 * 10);
        // 差模增量应受斜坡限速（含重归一平移）：|d_p| ≤ 2·slope
        assert!(
            st.d_p.iter().all(|v| v.abs() <= 2.0 * cfg.slope + 1e-9),
            "差模增量应受斜坡限速: {:?}",
            st.d_p
        );
    }

    #[test]
    fn test_failsafe_nan_regresses_to_zero() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        st.p_st = -40.0; // 之前充电
        let m = meter(f64::NAN, [f64::NAN; 3], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 10);
        assert!(st.p_st.abs() < 40.0, "failsafe 应斜坡回归: {}", st.p_st);
        assert!(p.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn test_move_toward_behavior() {
        assert_eq!(move_toward(10.0, 0.0, 5.0), 5.0);
        assert_eq!(move_toward(2.0, 0.0, 5.0), 0.0);
        assert_eq!(move_toward(-10.0, 0.0, 3.0), -7.0);
    }

    use crate::strategies::{CommandType, ControlCommand, FallbackStrategy, StrategyType};
    use crate::tai_storage::TaiStorageStrategy;
    use mupc_data_processing::telemetry::{
        BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus, PhaseElectricalData,
    };

    fn create_phase_data(p: f64) -> PhaseElectricalData {
        PhaseElectricalData {
            voltage: [Some(228.0); 3],
            current: [Some(p.signum() * 10.0); 3],
            active_power: [Some(p / 3.0); 3],
            reactive_power: [Some(0.0); 3],
            cos_phi: [Some(0.99); 3],
        }
    }

    fn create_package(timestamp: u64, grid_power: f64, soc: f64) -> DataPackage {
        DataPackage {
            timestamp,
            electrical: ElectricalData {
                voltage: Some(220.0),
                current: Some(30.0),
                active_power: Some(grid_power),
                reactive_power: Some(0.0),
                cos_phi: Some(0.99),
                frequency: Some(50.0),
                phase: Some(create_phase_data(grid_power)),
            },
            device_status: DeviceStatus {
                inverter_status: InverterStatus::Running,
                pv_power: Some(30.0),
                load_power: Some(40.0),
                ev_charger_power: None,
            },
            battery: BatteryData {
                soc: Some(soc * 100.0),
                soh: Some(95.0),
                temperature: Some(25.0),
            },
        }
    }

    fn poll(strategy: &TaiStorageStrategy, ts: u64, grid_power: f64) -> ControlCommand {
        let data = create_package(ts, grid_power, 0.5);
        tokio_test::block_on(strategy.evaluate(&data)).unwrap()
    }

    #[test]
    fn test_strategy_throttles_to_60s() {
        let strategy = TaiStorageStrategy::new(TaiStorageConfig::default());
        // 10:00 受电 10kW → S2（执行控制周期，输出零设定）
        let cmd1 = poll(&strategy, 3600 * 10, 10.0);
        // 1s 后返送 -50kW：若重算，经滑动窗均值 (10 + -50)/2 = -20 < -p_abs_trig，
        // 应进入 S1 充电（负出力）；被节流则返回缓存的 S2 零设定，两者一致。
        let cmd2 = poll(&strategy, 3600 * 10 + 1, -50.0);
        assert_eq!(cmd1.phase_p_set, cmd2.phase_p_set);
    }

    #[test]
    fn test_strategy_name_and_type() {
        let s = TaiStorageStrategy::new(TaiStorageConfig::default());
        assert_eq!(s.name(), "TaiStorageStrategy");
        assert_eq!(s.strategy_type(), StrategyType::Fallback);
    }

    #[test]
    fn test_strategy_outputs_phase_fields() {
        let strategy = TaiStorageStrategy::new(TaiStorageConfig::default());
        let cmd = poll(&strategy, 3600 * 10, 10.0); // 10:00 grid_power=10 → S2
        assert!(cmd.phase_p_set.is_some());
        assert!(cmd.phase_q_set.is_some());
        assert_eq!(cmd.cmd_id, 4);
        assert_eq!(cmd.cmd_type, CommandType::ChargeDischarge);
    }

    #[test]
    fn test_strategy_missing_phase_failsafe() {
        let strategy = TaiStorageStrategy::new(TaiStorageConfig::default());
        // phase=None → data_to_meter 返回默认（零输出），命令为 no-op 零设定
        let data = DataPackage {
            timestamp: 3600 * 10,
            electrical: ElectricalData {
                voltage: Some(220.0),
                current: Some(30.0),
                active_power: Some(-30.0),
                reactive_power: Some(0.0),
                cos_phi: Some(0.99),
                frequency: Some(50.0),
                phase: None,
            },
            device_status: DeviceStatus {
                inverter_status: InverterStatus::Running,
                pv_power: None,
                load_power: None,
                ev_charger_power: None,
            },
            battery: BatteryData {
                soc: Some(50.0),
                soh: None,
                temperature: None,
            },
        };
        let cmd = tokio_test::block_on(strategy.evaluate(&data)).unwrap();
        assert_eq!(cmd.phase_p_set, Some([0.0; 3]));
    }
}
