# 安全RL包装器（Safety RL Wrapper）详细设计方案

## 一、核心设计思想

### 1.1 为什么需要安全包装器？

* **ActionValidator的局限性**：现有4条规则（ACT-DUAL-01\~04）只检查动作的静态范围（值域、变化率、调度约束），无法预测动作施加后电网的**短时动态响应**。例如：

  * 一个合法的 `p_ref`（-30kW）在特定工况下可能导致电压在1秒内从0.98p.u.骤降至0.92p.u.（触发低电压保护）。
  * 一个合法的 `k_droop`（15kW/V）在电压剧烈波动时可能使 `P_output`超出逆变器硬件极限。
* **RobustnessManager的滞后性**：应急策略只在异常已经发生（电压<0.9p.u.）时才介入，属于被动防御。安全包装器应做到**事前预测、主动拒绝**。

### 1.2 设计原则

1. **轻量化**：包装器必须能在<5ms内完成一次可行性检查，不显著增加120ms的端到端延迟预算。
2. **保守优先**：当物理模型预测不确定或超时，默认拒绝动作并回退至上一有效动作。
3. **可证明安全**：物理模型应基于简化的电路方程，而非另一个黑盒神经网络，保证可审计、可调试。
4. **与现有模块正交**：不修改RL模型、ActionValidator、RewardCalculator的核心逻辑，仅作为管道中的一个过滤步骤。

---

## 二、架构位置与数据流

```
RLModel.decide() → 原始动作 [p_ref, k_droop]
       ↓
┌─────────────────────────────────┐
│  Safety RL Wrapper              │
│  ┌─────────────────────────┐    │
│  │ 1. 物理模型预测         │    │
│  │ 2. 安全边界检查         │    │
│  │ 3. 决策：通过/拒绝/回退 │    │
│  └─────────────────────────┘    │
└─────────────────────────────────┘
       ↓ (通过或回退后的动作)
ActionValidator.validate_dual() → 约束校验+clamp
       ↓
下发至strategy-engine
```

**输入**：

* 当前 `FusedSystemState`（含三相电压、SOC、负载率等）
* RL输出的原始动作（未经clamp）
* 上一周期有效动作（用于回退）
* 实时模块提供的下垂控制参数（`k_min`, `k_max`, `P_max`等）

**输出**：

* 若安全：原始动作（或轻微调整后的动作）
* 若不安全：上一周期有效动作 + 记录违规日志 + 触发WARN告警

---

## 三、物理模型设计

### 3.1 简化电网模型

为了在5ms内完成预测，使用**单节点戴维南等效电路**近似台区电网：

```
V_grid —— Z_line —— PCC —— 储能逆变器
                       │
                     负载 + 光伏
```

* **V\_grid**：配电网母线电压（假设恒定1.0p.u.，或从最近一次调度指令中获取）
* **Z\_line**：线路阻抗（R+jX），可从台区档案或实时参数辨识获得，默认值R=0.1Ω, X=0.05Ω
* **PCC**：并网点，即储能安装位置
* **储能逆变器**：可四象限运行，输出P+jQ，Q由实时模块闭环控制（已知当前Q输出）

**关键简化假设**：

* 忽略负荷和光伏的短时波动（假设它们在1秒内不变）。
* 忽略相邻馈线耦合（单台区独立分析）。
* 电压变化主要由储能功率注入引起，采用潮流灵敏度近似。

### 3.2 电压变化预测公式

根据灵敏度分析法，PCC电压变化量可近似为：

```
ΔV ≈ (R·ΔP + X·ΔQ) / V₀
```

其中：

* ΔP = 新动作的 `p_ref`- 当前实际 `P_output`（注意：当前 `P_output`已包含下垂分量 `P_output_curr = p_ref_prev + k_droop_prev × ΔV_prev`）
* ΔQ = 实时模块在当前电压下的预期Q调节量（可从 `q_realtime_margin`推算，或假设Q不变）
* V₀ = 当前PCC电压幅值

**实际计算步骤**：

1. 读取当前状态：`V_a`, `V_b`, `V_c`, `P_cur`, `Q_cur`, `q_margin`, `SOC`。
2. 计算新动作下的预期 `P_output_new`：

   ```
   P_output_new = p_ref_new + k_droop_new × (V_avg - 1.0)
   ```

   这里 `V_avg`是当前三相平均电压，ΔV = V\_avg - 1.0（标幺值，需转换为实际电压，假设基准电压220V，则1p.u.=220V）。
3. 计算 `ΔP = P_output_new - P_cur`。
4. 估算ΔQ：若 `q_margin`较大（>20%），认为实时模块可以维持当前Q不变；若 `q_margin`较小，假设实时模块会全力调节Q到边界，此时 `ΔQ = (1 - q_margin) × Q_max × sign(ΔV)`。
5. 代入灵敏度公式计算 `ΔV_predicted`。
6. 预测新电压：`V_new = V_avg + ΔV_predicted`。

### 3.3 安全边界定义


| 边界条件   | 阈值                                                           | 说明                                    |
| ---------- | -------------------------------------------------------------- | --------------------------------------- |
| 电压下限   | `V_new ≥ 0.93 p.u.`                                           | 略高于RobustnessManager的0.90，留有余量 |
| 电压上限   | `V_new ≤ 1.07 p.u.`                                           | 略低于1.10，留有余量                    |
| 电压变化率 | \`                                                             | ΔV\_predicted                          |
| SOC安全    | 若`p_ref>0`（放电），SOC必须≥12%（比临界10%留余量）           | 防止SOC跌至临界值                       |
| 功率反向   | 若`grid_power`即将从购电变为售电（或反之），检查逆变器能否承受 | 防止功率方向突变                        |

**判定逻辑**：所有边界条件同时满足 → 动作安全；任一不满足 → 拒绝。

---

## 四、实现细节

### 4.1 Rust结构体设计

```
/// 安全包装器
pub struct SafetyRLWrapper {
    /// 线路参数（可从配置或实时辨识更新）
    line_impedance: RwLock<LineImpedance>,
    /// 上一周期有效动作（用于回退）
    last_safe_action: RwLock<ActionOutput>,
    /// 物理模型预测函数（可替换为不同精度的模型）
    predictor: Box<dyn SafetyPredictor + Send + Sync>,
    /// 安全边界配置
    bounds: SafetyBounds,
}

/// 物理模型预测器trait（支持替换为更复杂的模型）
#[async_trait]
pub trait SafetyPredictor: Send + Sync {
    /// 预测动作后的电压和安全状态
    async fn predict(&self, state: &FusedSystemState, action: &ActionOutput) -> Result<PredictionResult, AiEngineError>;
}

/// 预测结果
pub struct PredictionResult {
    pub v_predicted: f64,           // 预测电压（p.u.）
    pub dv_dt: f64,                 // 电压变化率（p.u./s）
    pub soc_after: f64,             // 动作后SOC（若可估算）
    pub is_safe: bool,              // 综合安全标志
    pub reason: Option<String>,     // 不安全原因
}

/// 安全边界
pub struct SafetyBounds {
    pub v_min: f64,         // 0.93
    pub v_max: f64,         // 1.07
    pub dv_dt_max: f64,     // 0.03
    pub soc_margin: f64,    // 0.02（临界SOC+2%）
}
```

### 4.2 主流程方法

```
impl SafetyRLWrapper {
    /// 安全检查入口
    pub async fn check_and_fallback(
        &self,
        state: &FusedSystemState,
        proposed_action: &ActionOutput,
    ) -> (ActionOutput, CheckResult) {
        // 1. 用物理模型预测
        let pred = match self.predictor.predict(state, proposed_action).await {
            Ok(p) => p,
            Err(e) => {
                // 预测失败时保守回退
                tracing::warn!("安全预测失败: {:?}", e);
                return (self.last_safe_action.read().await.clone(), CheckResult::FallbackDueToPredictionError);
            }
        };

        // 2. 检查安全边界
        if !pred.is_safe {
            tracing::warn!(
                "动作被安全包装器拒绝: reason={:?}, proposed_p_ref={}, proposed_k_droop={}",
                pred.reason, proposed_action.p_ref, proposed_action.k_droop
            );
            let fallback = self.last_safe_action.read().await.clone();
            return (fallback, CheckResult::Rejected { reason: pred.reason.unwrap_or_default() });
        }

        // 3. 通过：更新last_safe_action
        *self.last_safe_action.write().await = proposed_action.clone();
        (proposed_action.clone(), CheckResult::Passed)
    }
}

/// 检查结果枚举
pub enum CheckResult {
    Passed,
    Rejected { reason: String },
    FallbackDueToPredictionError,
}
```

### 4.3 与ModelManager集成

在 `full_decision_cycle()`中，Step 6（RL决策）之后、Step 7（ActionValidator）之前插入：

```
// Step 6.5: 安全包装器检查
let (safe_action, check_result) = self.safety_wrapper.check_and_fallback(
    &fused_state,
    &rl_action,
).await;

// 记录检查结果到指标
metrics::counter!("safety_wrapper.rejected_total").increment(check_result.is_rejected() as u64);

// Step 7: 继续使用safe_action进行约束校验
let (validated, violations) = self.action_validator.validate_dual(
    &safe_action,
    fused_state.dispatch_p_set,
    false,
    &self.config.action_constraint,
).await;
```

---

## 五、性能预算与优化


| 阶段         | 最大延迟 | 说明                               |
| ------------ | -------- | ---------------------------------- |
| 物理模型预测 | 3ms      | 仅涉及几次浮点运算和一次灵敏度计算 |
| 边界检查     | 0.5ms    | 比较几个浮点数                     |
| 总开销       | **<5ms** | 远小于120ms预算的5%                |

**优化手段**：

* 使用 `f64`运算，避免不必要的内存分配。
* 将线路阻抗和灵敏度系数预先计算好，每次预测只需乘加。
* 若预测器需要查表（如复杂非线性模型），可将表格预加载到内存中。

---

## 六、测试与验证策略


| 测试场景                               | 预期行为           | 验证方法                   |
| -------------------------------------- | ------------------ | -------------------------- |
| 正常动作（电压0.98，p\_ref=-20kW）     | 通过               | 模拟计算V\_new=0.985，安全 |
| 危险动作（电压0.96，p\_ref=+40kW放电） | 拒绝，回退         | V\_new预测=0.91<0.93       |
| 边界动作（电压1.06，p\_ref=-30kW充电） | 拒绝，回退         | V\_new预测=1.075>1.07      |
| SOC=11%，p\_ref=+10kW放电              | 拒绝               | SOC margin不足             |
| 预测器内部错误                         | 回退至上一动作     | 模拟panic或超时            |
| 连续多次拒绝                           | 持续回退，产生告警 | 模拟连续不安全动作         |

---

## 七、与现有模块的关系

* **与RobustnessManager互补**：安全包装器做**事前预测拒绝**，RobustnessManager做**事后应急响应**。两者共存，前者减少后者触发频率。
* **与ActionValidator不冲突**：ActionValidator检查静态数值约束，安全包装器检查动态物理可行性。两者串联，顺序为：安全包装器 → ActionValidator。
* **与在线微调的关系**：被安全包装器拒绝的动作不会加入训练样本（或标记为“不安全”样本，用于离线分析），避免模型学习到不安全策略。

---

## 八、扩展性考虑

1. **可替换的预测器**：初期使用线性灵敏度模型，后期可升级为基于小信号模型的快速仿真（如MATLAB/Simulink生成的简化状态空间模型），只需实现 `SafetyPredictor`trait。
2. **多台区协同**：若未来需要多台区协同，预测器可引入邻居台区的等值阻抗，预测交互影响。
3. **自学习边界**：安全边界（如 `v_min=0.93`）可根据历史运行数据自动调整，例如：若从未出现过0.93以下的电压，可适当放宽至0.915，提高AI灵活性。

---

**总结**：安全RL包装器是一个轻量、可审计、可扩展的物理模型前置过滤器，能够在毫秒级内预测动作的电网动态响应，提前拦截高风险动作，显著提升系统的本质安全性。它与现有安全体系互补，且对性能影响极小，建议作为Phase 3C的补充功能纳入实现计划。
