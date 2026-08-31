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
        // 制造过流：先抬高共模 P 出力，再用强不平衡电流让差模积分拉满单相。
        // st.p_st=30 → step5 累加 slope=5 至 35；
        // 不平衡表计 [60,0,0] → unbal=100% → d_p 积分至 [40,-40,-40]。
        // 合成后 A 相 ≈ 35/3+40 ≈ 51.7kW → 235A > i_rated=190 → arbitrate 必须裁剪。
        st.p_st = 30.0;
        let m = meter(60.0, [60.0, 0.0, 0.0], [0.0; 3], [220.0; 3], [0.99; 3]);
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
