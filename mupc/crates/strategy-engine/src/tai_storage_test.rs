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
}
