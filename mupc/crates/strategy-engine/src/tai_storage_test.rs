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
        assert!((net - cfg.p_tgt_s1).abs() < 1.0, "净功率应到 +2: {}", net);
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
        let m = meter(6.3, [2.1, 2.1, 2.1], [0.0; 3], [220.0; 3], [0.99; 3]);
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
        let m = meter(50.0, [17.0, 17.0, 16.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let _ = control(&mut st, &cfg, &m, 0.5, 3600 * 12);
        // 储能未回归（p_st=-30）→ S1 保持，大步斜坡回 0
        assert_eq!(st.st, TaiState::S1PvAbsorb);
        assert!(st.p_st.abs() < 1.0, "基线受电应大步停充回 0: {}", st.p_st);
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
            p_st1,
            st.p_st
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
        // p_st=30 → S2 斜坡降 5 → 25；d_p=[18,-9,-9]（Σ=0，dp_max=25 内）时
        // A 相合成 ≈ 8.3+18=26.3kW → 119.5A > i_rated=110 → arbitrate 必须裁剪。
        // 注（2026-09-08 60kW 基线）：播种不宜过大——Σ=0 重归一回补会把削减摊回 A 相
        // 使其复超 i_rated（强过流播种在几何上不可同时满足单相限与零净，见 recomputes 注释）。
        st.p_st = 30.0;
        st.d_p = [18.0, -9.0, -9.0];
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
        // 制造单相过流：p_st=30（S2 斜坡降 5 → 25）、d_p=[18,-9,-9]（Σ=0），
        // 不平衡表计 [2,6,2]（unbal≈67%）→ 仲裁前 A 相合成 ≈ 26.3kW → 119.5A > i_rated=110。
        // I-2 修复：每轮顶格重算 pcmd、干净即 break，避免陈旧 pcmd 导致 8×slope 过剪。
        // 注（2026-09-08 60kW 基线 i_rated=110）：播种差模过大（如原 40/-20/-20 → clamp 25）
        // 时 Σ=0 重归一回补会把削减摊回 A 相使其复超 i_rated——真实差模由积分受 k_diff/slope
        // 约束不会瞬间到 dp_max，此处保持物理可达的轻度过流播种即可验证裁剪收敛与 Σ=0。
        st.p_st = 30.0;
        st.d_p = [18.0, -9.0, -9.0];
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
        // ③ 只裁剪到限值附近，而非陈旧 pcmd 的 8×slope 过剪（修复前 A 相会被剪到 ≈83A）。
        // 2026-09-08 60kW 基线 i_rated=110：A 相裁剪目标 ≈110A，下界取 95 排除过剪；
        // d_p[0] 断言原按 190A 基线（保留 >20 差模），新限下 A 差模裁剪目标更小，以 i_a 下界表达即可
        let i_a = (p[0].powi(2) + q[0].powi(2)).sqrt() * 1000.0 / 220.0;
        assert!(
            i_a > 95.0,
            "A 相被过度裁剪（应仅剪到限值附近而非 8×slope 过剪）: {:.1}A",
            i_a
        );
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
        // phase=None ⇒ D-9 failsafe 路径（积分冻结 + 斜坡回零 + 复位 Q 积分）；
        // 冷启动（p_st/d_p/q_last 全 0）下产出分相零设定。
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
        // 见 d9_missing_phase_runs_failsafe_and_resets_q：热态下的 failsafe 全语义判别力用例
    }

    // ══════════════ 全项目审查 WP2 判别力用例（D-1/D-2/D-8/D-9/D-10）══════════════

    /// 构造可控分相包（功率因数/分相无功/分相电流/分相有功逐相给定）。
    /// D-9 需要 Q 通道**真实累积**（qi≠0 且 |pf|<0.95），既有 `create_phase_data` 给不出。
    fn package_phase(ts: u64, pf: f64, q_per_phase: f64, i: [f64; 3], p_i: [f64; 3]) -> DataPackage {
        DataPackage {
            timestamp: ts,
            electrical: ElectricalData {
                voltage: Some(220.0),
                current: Some(i.iter().sum::<f64>()),
                active_power: Some(p_i.iter().sum()),
                reactive_power: Some(q_per_phase * 3.0),
                cos_phi: Some(pf),
                frequency: Some(50.0),
                phase: Some(PhaseElectricalData {
                    voltage: [Some(220.0); 3],
                    current: [Some(i[0]), Some(i[1]), Some(i[2])],
                    active_power: [Some(p_i[0]), Some(p_i[1]), Some(p_i[2])],
                    reactive_power: [Some(q_per_phase); 3],
                    cos_phi: [Some(pf); 3],
                }),
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
        }
    }

    /// **D-1（口径）**：不平衡度必须按**幅值**（设计 04 §2.1 / 台区储能设计 §2.9.1 电网公司口径
    /// `(1 − MIN|Ii|/MAX|Ii|) × 100`），不得用带符号电流。
    ///
    /// 期望值推导：
    /// - ① `[-10,-10,-10]`（三相同向返送、平衡）：MAX=MIN=10 ⇒ 0%（两种口径一致，防误活化）；
    /// - ①' `[-4,-12,-4]`（三相同向返送、**幅值不平衡**）：MAX=12、MIN=4 ⇒ (1−4/12)×100 = **66.67%**。
    ///   带符号实现 `max(0,−4,−12,−4)=0` ⇒ 判 0 ⇒ 差模通道**静默关闭**（本条即红）。
    /// - ② `[-10,8,8]`（A 相返送 10A、B/C 受电 8A ⇒ "单相返送"）：MAX=10、MIN=8
    ///   ⇒ (1−8/10)×100 = **20%**。带符号实现 MAX=8、MIN=−10 ⇒ (1+10/8)×100 = **225%**
    ///   （夸大到 >100%，虚越 25% 差模激活死区）。
    #[test]
    fn d1_unbalance_uses_current_magnitude() {
        use crate::tai_storage::unbalance_pct;
        // ① 三相同向返送、三相平衡 ⇒ 0
        assert_eq!(unbalance_pct(&[-10.0, -10.0, -10.0]), 0.0);
        // ①' 三相同向返送、幅值不平衡 ⇒ 66.67%（带符号实现会静默判 0）
        let got_a = unbalance_pct(&[-4.0, -12.0, -4.0]);
        assert!(
            (got_a - (1.0 - 4.0 / 12.0) * 100.0).abs() < 1e-9,
            "三相同向返送的幅值不平衡须 = 66.67%，实得 {got_a}"
        );
        // ② 单相返送 ⇒ 幅值口径 20%（带符号实现 225%）
        let got_b = unbalance_pct(&[-10.0, 8.0, 8.0]);
        assert!(
            (got_b - 20.0).abs() < 1e-9,
            "单相返送须按幅值口径 = (1−8/10)×100 = 20%，实得 {got_b}"
        );
        assert!(got_b <= 100.0, "幅值口径恒 ≤100%，实得 {got_b}");
        // 除零/极小电流守卫（MAX<1A 判 0）
        assert_eq!(unbalance_pct(&[0.0, 0.0, 0.0]), 0.0);
        assert_eq!(unbalance_pct(&[-0.5, -0.2, -0.1]), 0.0);
    }

    /// **D-1（端到端）**：三相同向返送 + 幅值不平衡（66.67% > 25% 死区）⇒ 差模通道必须激活。
    /// 带符号实现判 0%（< 15%）⇒ `d_p_active` 保持 false ⇒ 本条红。
    #[test]
    fn d1_three_phase_reverse_imbalance_activates_diff_channel() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 全部返送（同向、全负）但幅值不平衡：i=[-4,-12,-4]
        let m = meter(
            -20.0,
            [-4.0, -12.0, -4.0],
            [0.0; 3],
            [220.0; 3],
            [0.99; 3],
        );
        let _ = control(&mut st, &cfg, &m, 0.5, 3600 * 12);
        assert!(
            st.d_p_active,
            "三相同向返送的幅值不平衡（66.67% > 25%）须激活差模；带符号口径判 0 ⇒ 静默关闭"
        );
    }

    /// **D-8**：容量仲裁的末步（共模 clamp + ΣΔP 重归一）之后必须**复检**约束——
    /// 重归一会把 `move_toward` 削掉的差模量摊回相邻相 ⇒ 复超 `i_rated`。
    ///
    /// 构造（默认档 60kW：p_cap=60、i_rated=110A、s_rated=60kVA、slope=6、dp_max=25、q_i_max=25）：
    /// - 播种 `d_p=[0,-12,-12]`（ΣΔP=−24≠0，与 step6 的独立斜坡分支产生的形态同源）；
    /// - step6 积分后 `d_p=[-6,-6,-18]`（inc 三相 = −6/+6/−6，钳 ±slope）；
    /// - `q_pcs=[13,0,0]`（|pf|=0.90 保持 Q 通道激活，qi=0 使 q_pcs 不增）；
    /// - `p_st=60` 经 S2 斜坡 → 54 ⇒ 共模分摊 18kW/相。
    ///
    /// 重归一：mean = (−6−6−18)/3 = −10 ⇒ `d_p=[4,4,−8]` ⇒ A 相 P = 18+4 = 22kW，
    /// `s_A = √(22²+13²) = 25.6kVA > 24.2kVA(=110A@220V)` ⇒ **旧实现**（重归一侧在 break 之后、
    /// 无复检）终态 A 相 **116.1A**，被留给 PCS 静默 clamp；修复后 q_A 先被裁 0、A 相回到 100A。
    #[test]
    fn d8_arbitrate_rechecks_after_renormalization() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        st.st = TaiState::S2Flat;
        st.p_st = 60.0;
        st.d_p = [0.0, -12.0, -12.0];
        st.q_pcs = [13.0, 0.0, 0.0];
        let m = meter(10.0, [2.0, 6.0, 2.0], [0.0; 3], [220.0; 3], [0.90; 3]);
        let (p, q) = control(&mut st, &cfg, &m, 0.5, 3600 * 10);

        for i in 0..3 {
            let s = (p[i].powi(2) + q[i].powi(2)).sqrt();
            let i_phase = s * 1000.0 / 220.0;
            assert!(
                i_phase <= cfg.i_rated + 1e-6,
                "相{i} 终态仍超 i_rated: {i_phase:.1}A (P={:.1}, Q={:.1})——重归一侧后无复检",
                p[i],
                q[i]
            );
        }
        let dsum: f64 = st.d_p.iter().sum();
        assert!(dsum.abs() < 1e-6, "ΣΔP 须=0: {dsum}");
        let psum: f64 = p.iter().sum();
        assert!(
            (psum - st.p_st).abs() < 1e-6,
            "Σp 须=p_st: {psum} vs {}",
            st.p_st
        );
    }

    /// **D-9**：分相测量缺失 ⇒ 设计 §2.7 failsafe（积分冻结 + 斜坡回零 + **复位 Q 积分**），
    /// 不得当成"有效零测量"喂进状态机。
    ///
    /// 改坏实现会怎样红（旧 `data_to_meter → MeterData::default()`，p=0/u=220/pf=1.0/i=0）：
    /// ① 状态机用假 p=0 重建基线（`p_base_est = 0 + p_st = −60 < s1_exit`）⇒ S1 保持目标 −60、
    ///    **不按 slope 回零**；② pf=1.0 ⇒ Q 通道惰化、输出归零（≠ 保持 `q_last`）；
    /// ③ 失效拍被推入滤波窗（`meter_buf` 非空）。
    #[test]
    fn d9_missing_phase_runs_failsafe_and_resets_q() {
        let cfg = TaiStorageConfig::default();
        let strategy = TaiStorageStrategy::new(cfg.clone());
        // 前置：两拍有效分相测量（返送 + 低 PF 无功）⇒ 建立非零 p_st / q_pcs / q_last
        let pkg = |ts: u64| package_phase(ts, 0.90, 5.0, [-10.0; 3], [-10.0; 3]);
        let _ = tokio_test::block_on(strategy.evaluate(&pkg(3600 * 10))).unwrap();
        let _ = tokio_test::block_on(strategy.evaluate(&pkg(3600 * 10 + 60))).unwrap();
        let before = strategy.state_snapshot();
        assert!(
            before.p_st < -1.0,
            "前提：须先建立非零共模出力，实得 {}",
            before.p_st
        );
        assert!(
            before.q_pcs.iter().all(|v| *v > 0.5),
            "前提：Q 积分须已累积: {:?}",
            before.q_pcs
        );
        assert!(!before.meter_buf.is_empty(), "前提：滤波窗已入窗");
        let q_last_before = before.q_last;
        assert!(
            q_last_before.iter().any(|v| v.abs() > 0.5),
            "前提：q_last 须非零: {q_last_before:?}"
        );

        // 分相缺失包（总表电压/电流仍在，但无 phase 段）
        let mut no_phase = pkg(3600 * 10 + 120);
        no_phase.electrical.phase = None;
        let cmd = tokio_test::block_on(strategy.evaluate(&no_phase)).unwrap();
        let after = strategy.state_snapshot();

        assert_eq!(
            after.q_pcs, [0.0; 3],
            "failsafe 须复位 Q 积分（设计 §2.4：恢复后从 0 重新积分）"
        );
        assert!(
            after.meter_buf.is_empty(),
            "failsafe 须清滤波窗（不得把失效拍混入恢复后均值）"
        );
        assert!(
            (after.p_st - (before.p_st + cfg.slope)).abs() < 1e-9,
            "共模须按 slope 斜坡回 0：{} → {}（期望 {}）",
            before.p_st,
            after.p_st,
            before.p_st + cfg.slope
        );
        assert_eq!(
            cmd.phase_q_set,
            Some(q_last_before),
            "failsafe 须保持最后有效 Q（不得归零）"
        );
    }

    /// **D-10**：1 Hz 决策路径不得因内部锁中毒 panic（本 crate 既有口径
    /// `unwrap_or_else(|e| e.into_inner())`）。
    /// 改回 `.lock().unwrap()` ⇒ 中毒后 `evaluate_sync` 直接 panic ⇒ 本条红。
    #[test]
    fn d10_poisoned_locks_still_complete_a_tick() {
        let strategy = TaiStorageStrategy::new(TaiStorageConfig::default());
        strategy.poison_locks_for_test();
        assert!(
            strategy.locks_are_poisoned(),
            "前提：两把锁须真的中毒（否则本条空转）"
        );
        // 中毒后仍须完成一拍且**真的重算**（返送包 ⇒ 非零充电指令，而非返回缓存零指令）
        let cmd = tokio_test::block_on(strategy.evaluate(&create_package(3600 * 10, -30.0, 0.5))).unwrap();
        let sum: f64 = cmd.phase_p_set.unwrap().iter().sum();
        assert!(
            sum < 0.0,
            "中毒锁须被容忍并完成重算（不得 panic、不得返回缓存零指令）: {sum}"
        );
    }

    /// **D-2**：无任何 fresh SOC 源且无可用冻结值 ⇒ **拒绝下发**（分相 P/Q 全 0）+ 节流告警一次。
    /// 改坏实现会怎样红：`data.battery.soc.unwrap_or(50.0)` ⇒ 返送包在 50% 假 SOC 下照常充电（P≠0）。
    #[test]
    fn d2_no_soc_refuses_dispatch_and_warns_once() {
        let strategy = TaiStorageStrategy::new(TaiStorageConfig::default());
        let no_soc = |ts: u64| {
            let mut d = create_package(ts, -30.0, 0.5);
            d.battery.soc = None;
            d
        };
        let cmd1 = tokio_test::block_on(strategy.evaluate(&no_soc(3600 * 10))).unwrap();
        assert_eq!(
            cmd1.phase_p_set,
            Some([0.0; 3]),
            "无 SOC 必须拒绝下发（旧实现以假值 50% 给出非零充电指令）"
        );
        assert_eq!(cmd1.phase_q_set, Some([0.0; 3]));
        assert_eq!(
            strategy.state_snapshot().last_control_ts,
            0,
            "拒绝拍不得消耗控制周期（SOC 恢复后须立即接管）"
        );
        assert_eq!(strategy.soc_missing_warn_count(), 1, "首发须告警一次");
        // 后续两拍（时间戳递进，绕过 60s 控制节流）仍拒绝；30s 节流内**不再**告警
        let _ = tokio_test::block_on(strategy.evaluate(&no_soc(3600 * 10 + 60))).unwrap();
        let _ = tokio_test::block_on(strategy.evaluate(&no_soc(3600 * 10 + 120))).unwrap();
        assert_eq!(
            strategy.soc_missing_warn_count(),
            1,
            "告警须按 30s 节流（每拍刷屏 = 红）"
        );
        // 回拨节流计时 ⇒ 到期后须再次告警（非"一次性永不告警"）
        strategy.backdate_soc_missing_warn(std::time::Duration::from_secs(31));
        let _ = tokio_test::block_on(strategy.evaluate(&no_soc(3600 * 10 + 180))).unwrap();
        assert_eq!(
            strategy.soc_missing_warn_count(),
            2,
            "节流到期后须再次告警"
        );
    }

    /// **D-2（不得误伤冻结值）**：有可用 SOC（`apply_soc_source` 在双源皆失时写回的冻结
    /// existing，或 fresh 源）⇒ 仍按该值驱动；且拒绝**不粘滞**——fresh 源恢复立即正常下发。
    ///
    /// 改坏实现会怎样红：若"非 fresh 即拒绝"（把冻结值也拒掉）⇒ ① 全零 ⇒ 红；
    /// 若拒绝路径消耗了控制周期 ⇒ ② 的 1s 后恢复被 60s 节流挡住、返回缓存零指令 ⇒ 红。
    #[test]
    fn d2_available_soc_still_drives_and_recovers() {
        // ① 冻结值路径（tai 层只见 Some/None，源新鲜度由 apply_soc_source 在上层裁决并写回）
        let s1 = TaiStorageStrategy::new(TaiStorageConfig::default());
        let cmd = tokio_test::block_on(s1.evaluate(&create_package(3600 * 10, -30.0, 0.20))).unwrap();
        let sum: f64 = cmd.phase_p_set.unwrap().iter().sum();
        assert!(
            sum < 0.0,
            "有可用 SOC（冻结或 fresh）须照常驱动 S1 充电: {sum}"
        );

        // ② 拒绝 → fresh 恢复：拒绝拍不消耗周期 ⇒ 下一拍（+1s）立即接管
        let s2 = TaiStorageStrategy::new(TaiStorageConfig::default());
        let mut absent = create_package(3600 * 10, -30.0, 0.5);
        absent.battery.soc = None;
        let refused = tokio_test::block_on(s2.evaluate(&absent)).unwrap();
        assert_eq!(refused.phase_p_set, Some([0.0; 3]));
        let cmd2 =
            tokio_test::block_on(s2.evaluate(&create_package(3600 * 10 + 1, -30.0, 0.5))).unwrap();
        let sum2: f64 = cmd2.phase_p_set.unwrap().iter().sum();
        assert!(
            sum2 < 0.0,
            "fresh SOC 恢复须立即正常下发（不得粘滞在拒绝态）: {sum2}"
        );
    }

    /// **告警文案阈值须随入参跟随**（评审 W-3 的修复判别力，2026-09-29 复核补）。
    ///
    /// 背景：复核实测「把 `stale_after.as_secs()` 换回字面量 `5`」**不会让任何既有用例变红**
    /// ⇒ W-3 的修复本身**可被静默回退**。本用例即为该修复的判别力覆盖。
    ///
    /// **改什么会让本条变红**：
    /// ① `stale_warn_message` 里把 `{}`/`as_secs()` 换回**字面量**（如 `"超过 5s"`）
    ///    ⇒ 传入 7/11 s 的两条断言红（文案恒为 5）；
    /// ② 把 `as_secs()` 改成别的换算（如 `as_millis()`）⇒ 数值断言红。
    #[test]
    fn stale_warn_message_follows_threshold() {
        let m5 = TaiStorageStrategy::stale_warn_message(std::time::Duration::from_secs(5));
        assert!(
            m5.contains("超过 5s 未更新"),
            "5s 阈值须如实进文案: {m5}"
        );

        // 换一个**非 5** 的阈值：若文案里是硬编码字面量，这两条必红
        for secs in [7u64, 11] {
            let m = TaiStorageStrategy::stale_warn_message(std::time::Duration::from_secs(secs));
            assert!(
                m.contains(&format!("超过 {secs}s 未更新")),
                "阈值须随入参跟随（硬编码 5 即红）: {m}"
            );
            assert!(
                !m.contains("超过 5s 未更新"),
                "不得残留字面量 5: {m}"
            );
        }

        // 文案的**其余部分**（归零说明 + 排查指引）不得因参数化而丢失
        let m = TaiStorageStrategy::stale_warn_message(std::time::Duration::from_secs(5));
        assert!(m.contains("已下发归零（分相 P/Q=0）"), "{m}");
        assert!(m.contains("请检查总表站与采集链路"), "{m}");
    }
}
