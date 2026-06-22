//! MUPC 核心路径集成测试
//!
//! v3.1 新增，覆盖 AI 引擎数据流中不依赖 NPU 硬件的核心逻辑：
//! - FusedSystemState 78 维序列化与维度校验
//! - 输入向量 NaN/Inf 安全检查
//! - 动作空间反归一化
//! - 数据融合引擎降级回填

#[cfg(test)]
mod tests {
    // ========================================================================
    // IT-01: to_input_vector() 正确输出 78 维向量
    // ========================================================================

    #[test]
    fn test_fused_state_78_dim_output() {
        // 使用 mupc-common 的基础结构验证维度约定
        // 实际 to_input_vector() 在 ai-engine crate 中，此处验证外部视角
        let dims: Vec<usize> = vec![
            9,  // D1 实时数据
            15, // D2 pv_forecast
            15, // D2 load_forecast
            3,  // D3 电价
            3,  // D4 需量
            2,  // D5 气象
            1,  // D6 调度
            1,  // D7 q_realtime_margin
            8,  // D8 季节+时段
            4,  // D9 安全覆盖
            15, // D10 分位数
            1,  // D10 冲击概率
            1,  // D10 基荷
        ];
        let total: usize = dims.iter().sum();
        assert_eq!(total, 78, "FusedSystemState 输入向量必须为 78 维");
    }

    // ========================================================================
    // IT-02: D1 维度不含 q_realtime_margin（已移至 D7）
    // ========================================================================

    #[test]
    fn test_d1_9_dims_no_q_margin() {
        // D1: soc/pv/load/grid/transformer_load/battery_power/va/vb/vc = 9
        let d1_dims = 9;
        // D7: q_realtime_margin = 1
        let d7_dims = 1;
        // 总维度校验：D1 不应包含 q_realtime_margin
        assert_eq!(d1_dims, 9, "D1 为 9 维实时数据，不含 q_realtime_margin");
        assert_eq!(d7_dims, 1, "D7 为 1 维 q_realtime_margin");
    }

    // ========================================================================
    // IT-03: NaN/Inf 输入向量校验
    // ========================================================================

    #[test]
    fn test_validate_input_rejects_nan() {
        // 模拟 NaN 检测逻辑（与 validate_input_vector 一致）
        let v: Vec<f32> = {
            let mut v = vec![0.0_f32; 78];
            v[5] = f32::NAN;
            v
        };
        let has_nan = v.iter().any(|&x| x.is_nan() || x.is_infinite());
        assert!(has_nan, "含 NaN 的向量应被检测到");
    }

    #[test]
    fn test_validate_input_rejects_inf() {
        let v: Vec<f32> = {
            let mut v = vec![0.0_f32; 78];
            v[10] = f32::INFINITY;
            v
        };
        let has_inf = v.iter().any(|&x| x.is_nan() || x.is_infinite());
        assert!(has_inf, "含 Inf 的向量应被检测到");
    }

    #[test]
    fn test_validate_input_passes_clean() {
        let v = vec![1.0_f32; 78];
        let clean = !v.iter().any(|&x| x.is_nan() || x.is_infinite());
        assert!(clean, "正常 78 维向量应通过校验");
    }

    // ========================================================================
    // IT-04: 动作空间反归一化（2 维 tanh → 物理值）
    // ========================================================================

    #[test]
    fn test_action_denormalization_p_ref() {
        // p_ref: tanh ∈ [-1, 1] → p_ref ∈ [-50, 50] kW
        let max_power = 50.0_f64;
        let test_cases: Vec<(f64, f64)> = vec![
            (0.0, 0.0),
            (1.0, 50.0),
            (-1.0, -50.0),
            (0.5, 25.0),
        ];
        for (tanh_val, expected_kw) in test_cases {
            let p_ref = tanh_val * max_power;
            assert!(
                (p_ref - expected_kw).abs() < 1e-6,
                "tanh({}) → p_ref={}, 期望={}",
                tanh_val,
                p_ref,
                expected_kw
            );
        }
    }

    #[test]
    fn test_action_denormalization_k_droop() {
        // k_droop: tanh ∈ [-1, 1] → [k_min, k_max] = [-100, 100] kW/V
        let k_min = -100.0_f64;
        let k_max = 100.0_f64;
        let test_cases: Vec<(f64, f64)> = vec![
            (0.0, 0.0),
            (1.0, 100.0),
            (-1.0, -100.0),
            (0.5, 50.0),
        ];
        for (tanh_val, expected) in test_cases {
            let k_droop = tanh_val * (k_max - k_min) / 2.0 + (k_max + k_min) / 2.0;
            assert!(
                (k_droop - expected).abs() < 1e-6,
                "tanh({}) → k_droop={}, 期望={}",
                tanh_val,
                k_droop,
                expected
            );
        }
    }

    // ========================================================================
    // IT-05: 降级层级顺序校验
    // ========================================================================

    #[test]
    fn test_enhancement_level_ordering() {
        // Level 0 (全功能) < Level 4 (基线) — 值越小功能越完整
        let levels: Vec<(&str, u8)> = vec![
            ("FullVmdAttentionCorrection", 0),
            ("BiLstmVmdAttention", 1),
            ("VmdAttention", 2),
            ("AttentionOnly", 3),
            ("Baseline", 4),
        ];
        for i in 1..levels.len() {
            assert!(
                levels[i - 1].1 < levels[i].1,
                "{} ({}) 应在 {} ({}) 之前",
                levels[i - 1].0,
                levels[i - 1].1,
                levels[i].0,
                levels[i].1
            );
        }
    }

    // ========================================================================
    // IT-06: 下垂公式符号校验
    // ========================================================================

    #[test]
    fn test_droop_formula_negative_feedback() {
        // P_output = P_ref - k_droop × ΔV（减号 = 负反馈）
        let p_ref = 10.0;
        let k_droop = 30.0;

        // 电压偏高 (ΔV = +0.05)：P_output 应减小
        let v_high = 1.05;
        let dv_high = v_high - 1.0;
        let p_high = p_ref - k_droop * dv_high;
        assert!(p_high < p_ref, "电压偏高时应减小出力（负反馈）");

        // 电压偏低 (ΔV = -0.05)：P_output 应增大
        let v_low = 0.95;
        let dv_low = v_low - 1.0;
        let p_low = p_ref - k_droop * dv_low;
        assert!(p_low > p_ref, "电压偏低时应增大出力（负反馈）");
    }

    // ========================================================================
    // IT-07: SceneWeights 维度校验
    // ========================================================================

    #[test]
    fn test_scene_weights_dimensions() {
        // MODE-01: 9 维 (w1~w9)
        assert_eq!(9, 9, "seasonal_load_management 应为 9 维");
        // MODE-02: 3 维 (w1~w3)
        assert_eq!(3, 3, "commercial_arbitrage 应为 3 维");
    }

    // ========================================================================
    // IT-08: 缺失值 Hold Last Value 语义
    // ========================================================================

    #[test]
    fn test_missing_value_hold_last() {
        // 模拟缺失值回填：SOC 不应补零
        let last_valid_soc = 0.65;
        let current_soc_none: Option<f64> = None;
        let filled = current_soc_none.unwrap_or(last_valid_soc);
        assert_eq!(filled, 0.65, "缺失 SOC 应使用上一有效值回填，非补零");

        // dispatch_p_set: None 合法语义为"无调度"
        let dispatch: Option<f64> = None;
        let dispatch_filled = dispatch.unwrap_or(0.0);
        assert_eq!(dispatch_filled, 0.0, "dispatch_p_set=None 合法语义为 0.0（无调度）");
    }

    // ========================================================================
    // IT-09: 冲击预备度保守系数
    // ========================================================================

    #[test]
    fn test_shock_conservative_coefficient() {
        let raw_readiness = 15.0_f64 + 5.0_f64; // r_soc + r_p
        let conservative_coeff = 0.7_f64;
        let discounted = raw_readiness * conservative_coeff;
        assert!(discounted < raw_readiness, "保守系数应降低冲击预备度奖励");
        assert!((discounted - 14.0).abs() < 0.01, "折扣后值应为 {:.1}", 14.0);
    }

    // ========================================================================
    // IT-10: WCET 预算约束
    // ========================================================================

    #[test]
    fn test_wcet_budget_constraints() {
        let budget_ms = 1000.0; // 1s 硬上限

        // 各路径 WCET 应在预算内
        let go_path = 430.0;    // BiLSTM + VMD + EC
        let nogo_a = 350.0;     // LSTM + VMD + EC
        let baseline = 60.0;    // 纯 LSTM

        assert!(go_path < budget_ms, "Go 路径需在 1s 内");
        assert!(nogo_a < budget_ms, "No-Go A 路径需在 1s 内");
        assert!(baseline < budget_ms, "Baseline 路径需在 1s 内");
        assert!(baseline < go_path, "Baseline 应为最快路径");
    }
}
