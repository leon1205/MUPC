# MUPC 推理侧（Rust）改造清单 — 四项吸收方案

> **日期**：2026-06-25
> **关联文档**：
> - `上游训练管线-MUPC-AI2-改造要求-PCRL偏好可控RL吸收.md`（R1）
> - `上游训练管线-MUPC-AI2-改造要求-逆RL奖励函数吸收.md`（R2）
> - `上游训练管线-MUPC-AI2-改造要求-CEEMDAN级联分解与残差修正吸收.md`（P1）
> - `上游训练管线-MUPC-AI2-改造要求-PPO场景规划不确定性建模吸收.md`（R3）

---

## 总览

| 方案 | MUPC 改动量 | 涉及 Crate | 推理延迟影响 |
|------|-----------|-----------|------------|
| R1 PCRL | 中（~300 行） | ai-engine, strategy-engine | +0.5ms（81维 vs 78维） |
| R2 逆RL | **零** | 无 | 无 |
| P1 CEEMDAN | **大（~800 行）** | ai-engine, data-processing | +30~50ms（分量预测+残差修正） |
| R3 场景规划 | **零** | 无 | 无 |

---

## R1 — PCRL 偏好可控 RL

### 1. `mupc/crates/ai-engine/src/safety_config.rs`

**新增** `BatteryAgingModel` 结构体 + 反序列化：

```rust
/// 多因素电池衰减模型（DOD × C-rate × 温度耦合）
#[derive(Debug, Clone, Deserialize)]
pub struct BatteryAgingConfig {
    /// 基础衰减系数 α₀
    pub alpha_0: f64,
    /// DOD 敏感系数
    pub beta_dod: f64,
    /// C-rate 敏感系数
    pub beta_c_rate: f64,
    /// Arrhenius Ea/R (LiFePO4 ≈ 31700 K)
    pub ea_over_r: f64,
    /// Ah 吞吐量指数 β
    pub beta_exponent: f64,
    /// 标称电芯温度 (K)
    pub temp_nominal_kelvin: f64,
    /// 电池替换成本 (元/kWh)
    pub replacement_cost_yuan_per_kwh: f64,
    /// 是否启用衰减计算
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool { false }

impl Default for BatteryAgingConfig {
    fn default() -> Self {
        Self {
            alpha_0: 0.002,
            beta_dod: 1.5,
            beta_c_rate: 0.8,
            ea_over_r: 31700.0,
            beta_exponent: 0.87,
            temp_nominal_kelvin: 298.15,
            replacement_cost_yuan_per_kwh: 800.0,
            enabled: false,
        }
    }
}
```

### 2. `mupc/crates/ai-engine/src/safety_wrapper.rs`

**改动点**：

a. `SafetyRLWrapper` 新增字段：
```rust
pub struct SafetyRLWrapper {
    // ... 现有字段 ...
    battery_aging: BatteryAgingConfig,
    /// 累计 Ah 吞吐量（用于衰减计算）
    cumulative_ah_throughput: f64,
    /// 偏好向量 [λ_peak, λ_cap, λ_life]
    preference_vector: [f64; 3],
}
```

b. `validate_action()` 中新增电池衰减成本计算：
```rust
impl SafetyRLWrapper {
    pub fn validate_action(&mut self, action: &ActionOutput, state: &FusedSystemState) -> ValidationResult {
        // ... 现有校验 ...

        // 新增：电池衰减成本估算
        if self.battery_aging.enabled {
            let dod = state.soc_max_cycle - state.soc_min_cycle; // 本周期 DOD
            let c_rate = (action.p_ref.abs() / state.battery_capacity_kw as f64)
                .clamp(0.0, 5.0);
            let temp_k = self.battery_aging.temp_nominal_kelvin;

            let alpha = self.battery_aging.alpha_0
                * (self.battery_aging.beta_dod * dod).exp()
                * (self.battery_aging.beta_c_rate * c_rate).exp()
                * (self.battery_aging.ea_over_r / temp_k).exp();

            let ah = (action.p_ref.abs() * 0.25 / 400.0).abs(); // 15min Δt, 400V
            let loss_rate = alpha * ah.powf(self.battery_aging.beta_exponent);
            let aging_cost = loss_rate * self.battery_aging.replacement_cost_yuan_per_kwh;
            self.cumulative_ah_throughput += ah;

            // 附加到校验结果（用于日志/审计，不阻止动作）
            result.battery_aging_cost = Some(aging_cost);
            result.capacity_loss_rate = Some(loss_rate);
        }

        result
    }
}
```

### 3. `mupc/crates/ai-engine/src/env_config.rs`

**新增配置段**：

```rust
pub struct EnvConfig {
    // ... 现有字段 ...
    pub battery_aging: BatteryAgingConfig,
}
```

### 4. `mupc/config/mupc_env_config.yaml`

**新增 YAML 配置段**：

```yaml
battery_aging:
  enabled: false            # 初始关闭，训练验证后开启
  alpha_0: 0.002
  beta_dod: 1.5
  beta_c_rate: 0.8
  ea_over_r: 31700.0
  beta_exponent: 0.87
  temp_nominal_kelvin: 298.15
  replacement_cost_yuan_per_kwh: 800.0

operational:
  # 新增
  default_preference: [0.5, 0.3, 0.2]  # [调峰, 增容, 保电池]
```

### 5. `mupc/crates/ai-engine/src/model_manager.rs`

**改动点**：

a. ONNX 模型加载时解析 `mupc_with_preference` metadata，确定输入维度：
```rust
// 读取 ONNX metadata
let with_preference = session.metadata()?
    .get("mupc_with_preference")
    .map(|v| v == "true")
    .unwrap_or(false);

let expected_input_dim = if with_preference { 81 } else { 78 };
```

b. 若启用偏好，构建推理输入时拼接偏好向量：
```rust
fn build_input_vector(&self, state: &FusedSystemState) -> Vec<f32> {
    let mut v = state.to_input_vector(); // 78 维

    if self.with_preference {
        v.extend_from_slice(&[
            self.preference[0] as f32,
            self.preference[1] as f32,
            self.preference[2] as f32,
        ]); // → 81 维
    }
    v
}
```

### 6. `mupc/crates/strategy-engine/src/ai_integration.rs`

**改动点**：从 DB 加载偏好向量配置：

```rust
// 从 operational 表读取 default_preference
let pref = db_config.get_preference_vector()
    .unwrap_or([0.5, 0.3, 0.2]);
model_manager.set_preference(pref);
```

---

## P1 — CEEMDAN 级联分解 + 残差修正

### 预测管线架构总览

```
当前 (Rust):
  data_fusion → to_input_vector(78维) → ONNX推理(LSTM→Attention) → 预测值

升级后 (Rust):
  data_fusion → to_input_vector(78维)
    → [新] CEEMDAN 级联分解 (历史序列预处理)
    → [新] 4 分量独立推理 (4×ONNX 或 1×多输出ONNX)
    → [新] 分量求和 → 一阶预测
    → [新] BiLSTM 残差修正推理
    → 最终预测值
```

### 1. `mupc/crates/ai-engine/src/ceemdan_decomposer.rs`（**新文件**）

CEEMDAN 分解在 Rust 侧的实现。因 CEEMDAN 含迭代加噪+EMD+平均，计算量较大，采用**周期性离线更新**策略（每 N 步更新一次分解，而非每步）：

```rust
/// CEEMDAN 分解器（简化版，适配嵌入式部署）
pub struct CeemdanDecomposer {
    /// 加噪次数
    pub n_ensemble: usize,      // 默认 50
    /// 噪声标准差系数
    pub noise_std: f64,         // 默认 0.2
    /// 最大 IMF 数
    pub max_imf: usize,         // 默认 12
    /// 样本熵嵌入维数
    pub se_m: usize,            // 默认 2
    /// 样本熵相似容限系数
    pub se_r_ratio: f64,        // 默认 0.2
    /// 二次重构 SE 阈值
    pub se_threshold: f64,      // 默认 0.5
    /// 上次分解的 IMF 缓存
    cached_imfs: Option<Vec<Vec<f64>>>,
    /// 上次分解的数据窗口（用于检测是否需要重新分解）
    cached_data_hash: u64,
    /// 分解更新间隔（步）
    update_interval: usize,     // 默认 96 (1天)
    steps_since_update: usize,
}

impl CeemdanDecomposer {
    /// 对历史序列做级联分解，返回 4 个重构分量
    pub fn cascade_decompose(&mut self, series: &[f64]) -> [Vec<f64>; 4] {
        // 1. 检查是否需要重新分解
        if self.steps_since_update < self.update_interval {
            if let Some(ref imfs) = self.cached_imfs {
                // 追加最新数据点，滑动窗口
                return self.slide_and_reconstruct(series, imfs);
            }
        }

        // 2. 一次 CEEMDAN 分解
        let imfs1 = self.ceemdan(series, self.max_imf);

        // 3. 样本熵计算 + 一次重构（3 分量）
        let se_values: Vec<f64> = imfs1.iter()
            .map(|imf| sample_entropy(imf, self.se_m, self.se_r_ratio))
            .collect();
        let (comp_high, comp_mid, comp_low) = cluster_by_se(&imfs1, &se_values);

        // 4. 高频分量二次 CEEMDAN
        let sub_imfs = self.ceemdan(&comp_high, self.max_imf / 2);

        // 5. 二次 SE 重构（阈值 0.5）
        let sub_se: Vec<f64> = sub_imfs.iter()
            .map(|imf| sample_entropy(imf, self.se_m, self.se_r_ratio))
            .collect();

        let mut comp_a = vec![0.0; series.len()]; // 高-高频
        let mut comp_b = vec![0.0; series.len()]; // 高-低频
        for (i, (imf, &se)) in sub_imfs.iter().zip(sub_se.iter()).enumerate() {
            if se > self.se_threshold {
                for (j, &v) in imf.iter().enumerate() { comp_a[j] += v; }
            } else {
                for (j, &v) in imf.iter().enumerate() { comp_b[j] += v; }
            }
        }

        // 6. 缓存
        self.cached_imfs = Some(vec![comp_a.clone(), comp_b.clone(),
                                      comp_mid.clone(), comp_low.clone()]);
        self.steps_since_update = 0;

        [comp_a, comp_b, comp_mid, comp_low]
    }

    /// 简化 EMD 分解（不使用外部 FFT 依赖，用极值包络法实现）
    fn emd(&self, signal: &[f64]) -> Vec<Vec<f64>> {
        // 使用极值点检测 + 三次样条插值 → IMF 迭代提取
        // 依赖 ndarray + 自实现样条插值，避开 FFTW/FFTPACK 依赖
        todo!("EMD implementation — 约 200 行")
    }

    fn ceemdan(&self, signal: &[f64], max_imf: usize) -> Vec<Vec<f64>> {
        // CEEMDAN = 多次 EMD + 噪声对消
        // 每轮：加白噪声 → EMD → 取 IMF1 → 平均 → 残差 → 下一轮
        todo!("CEEMDAN implementation — 约 150 行")
    }
}

/// 样本熵计算
fn sample_entropy(x: &[f64], m: usize, r_ratio: f64) -> f64 {
    let r = r_ratio * std_dev(x);
    if r < 1e-10 { return 0.0; }
    // 模板匹配计数 → -ln(A/B)
    // 约 50 行
    todo!()
}

/// 按 SE 值聚类为 3 组
fn cluster_by_se(imfs: &[Vec<f64>], se_values: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    // 排序 SE → 相邻差距 < 0.1 合并 → 得 3 组
    // 约 60 行
    todo!()
}
```

### 2. `mupc/crates/ai-engine/src/prediction_pipeline.rs`

**改动点**：

a. 新增 `CascadePredictionEngine`：
```rust
pub struct CascadePredictionEngine {
    /// CEEMDAN 分解器
    decomposer: CeemdanDecomposer,
    /// 分量预测 ONNX Session（输出 4 分量 × 预测值）
    component_session: OrtSession,
    /// 残差修正 ONNX Session
    residual_session: Option<OrtSession>,
    /// 历史序列缓冲区（用于分解）
    history_buffer: VecDeque<f64>,
    /// 外部特征缓冲区（温度/湿度/节假日等，用于残差修正）
    external_features: VecDeque<[f32; 7]>,
}

impl CascadePredictionEngine {
    pub fn predict(&mut self, state: &FusedSystemState) -> Result<f32> {
        // 1. 更新历史缓冲区
        self.history_buffer.push_back(state.load_power as f64);
        if self.history_buffer.len() > 288 { // 保留 3 天 (96×3)
            self.history_buffer.pop_front();
        }

        // 2. 级联分解（可能命中缓存，不每步重新分解）
        let components = self.decomposer.cascade_decompose(
            &make_contiguous(&self.history_buffer)
        );
        // 取最新窗口用于预测输入

        // 3. 4 分量独立预测（一次 ONNX 推理，batch=4 或多输出）
        let component_preds = self.run_component_inference(&components)?;
        let first_order = component_preds.iter().sum::<f32>();

        // 4. 残差修正（可选）
        let final_pred = if let Some(ref session) = self.residual_session {
            let residual_input = self.build_residual_input(first_order);
            let correction = self.run_residual_inference(session, &residual_input)?;
            first_order + correction
        } else {
            first_order
        };

        Ok(final_pred)
    }
}
```

b. 在 `try_promote()` 中新增增强等级：

```rust
pub enum EnhancementLevel {
    // ... 现有 ...
    /// CEEMDAN 级联分解 + 4 分量预测（替代 VMD）
    CascadeDecomposition,
    /// 级联分解 + BiLSTM 残差修正
    CascadeWithResidual,
}
```

### 3. `mupc/crates/ai-engine/Cargo.toml`

**新增依赖**：

```toml
[dependencies]
ndarray = { version = "0.15", features = ["approx"] }  # 矩阵运算（EMD 样条插值）
# 注意：不引入 FFTW/FFTPACK 等重量级依赖，EMD 用极值包络法实现
```

### 4. ONNX 模型加载变更

`model_manager.rs` 需支持加载两个 ONNX 模型：
- 主模型：分量预测（输入 78 维 → 输出 4×1 维）
- 残差模型：残差修正（输入 7 维外部特征 → 输出 1 维修正值）

```rust
pub struct ModelManager {
    // ... 现有 ...
    component_session: OrtSession,       // 升级：分量预测
    residual_session: Option<OrtSession>, // 新增：残差修正
}
```

### 5. 计算复杂度评估

| 操作 | 频率 | 耗时估算 | 说明 |
|------|------|---------|------|
| CEEMDAN 一次分解 | 每 96 步 (1天) | ~200ms（288点，50次EMD） | 离线预处理 |
| 4 分量预测推理 | 每步 | ~15ms | 4×LSTM 并行推理 |
| BiLSTM 残差修正 | 每步 | ~8ms | 64+32 神经元 |
| **总增量** | 每步 | **~23ms**（非分解步）/ **~223ms**（分解步） | RK3588 NPU 加速后更低 |

---

## R2 — 逆强化学习奖励函数 & R3 — 场景规划

**MUPC 推理侧零改动。**

理由：两种技术均为训练阶段方法，产出的策略网络结构与当前完全一致（78 维输入 → 2 维输出 → ONNX → RKNN），仅网络权重不同。需追加 ONNX metadata 用于审计：

| 元数据键 | 值 | 说明 |
|----------|-----|------|
| `mupc_reward_source` | `"reirl"` / `"manual"` | 奖励函数来源 |
| `mupc_scenario_planning` | `"lhs_sbr"` / `"none"` | 是否经场景规划训练 |

---

## 汇总：MUPC 文件改动清单

| 文件 | R1 | P1 | 改动类型 |
|------|:--:|:--:|----------|
| `ai-engine/src/safety_config.rs` | ✓ | | 新增 BatteryAgingConfig |
| `ai-engine/src/safety_wrapper.rs` | ✓ | | 新增电池衰减计算 + 偏好字段 |
| `ai-engine/src/env_config.rs` | ✓ | | 新增 battery_aging 段 |
| `ai-engine/src/model_manager.rs` | ✓ | ✓ | 偏好维度适配 + 双 ONNX 加载 |
| `ai-engine/src/prediction_pipeline.rs` | | ✓ | 新增 CascadePredictionEngine + EnhancementLevel |
| `ai-engine/src/ceemdan_decomposer.rs` | | ✓ | **新文件** ~400 行 |
| `ai-engine/src/data_fusion.rs` | | ✓ | 外部特征采集（7维：温度/湿度/节假日等） |
| `ai-engine/Cargo.toml` | | ✓ | 新增 ndarray 依赖 |
| `config/mupc_env_config.yaml` | ✓ | | 新增 battery_aging + preference 段 |
| `strategy-engine/src/ai_integration.rs` | ✓ | | 偏好向量 DB 加载 |
| **总计新增/改动行数** | ~300 行 | ~800 行 | |

---

## 落地依赖关系

```
P1 (预测升级)
 │
 ├── 先落地 CEEMDAN 级联分解
 │    └── 提升预测精度 → 缩小后续 σ 误差
 │
 ├── 再落地 TCN 前置特征提取（已有方案）
 │    └── P1 的分量预测 BiLSTM 前可插入 TCN
 │
 └── 最后落地 R1 (PCRL) + R3 (场景规划)
      └── P1 提供更准的预测 → R3 场景规划的 σ 更窄 → 场景更集中
      └── R1 和 R3 可并行训练（不同维度改进）
```

**R2 逆RL**可独立评估，不依赖其他改造（仅改变训练阶段奖励标定方式）。

---

**文档状态**：待评审
