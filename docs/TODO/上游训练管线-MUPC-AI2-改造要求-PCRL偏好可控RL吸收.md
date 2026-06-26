# 上游训练管线（MUPC-AI2）改造要求 — PCRL 偏好可控强化学习吸收

> **来源**：朱江峰《基于偏好可控强化学习的分布式储能系统调峰-增容功能复用策略优化方法》(2025)
> **吸收级别**：R1 强烈推荐
> **MUPC 推理侧影响**：中等（SafetyRLWrapper 电池衰减模型升级 + 偏好向量推理）
> **日期**：2026-06-25

---

## 1. 吸收方法

PCRL（Preference-Conditioned RL）框架将当前 MADDPG/PPO 的**标量加权奖励**升级为**向量化多目标 + 动态偏好自适应**，同时引入多因素电池衰减模型。

```
当前架构: 状态 s → MADDPG/PPO → 动作 a (p_ref, k_droop)
                         ↑
                    标量奖励 r = w1·r1 + w2·r2 + ... (人工调权)

新架构:   状态 s → 偏好映射网络 → 偏好向量 λ
                ↓
          [s, λ] → 偏好条件策略网络 → 动作 a (p_ref, k_droop)
                         ↑
                    向量奖励 R = [r1, r2, r3] → 正则化梯度聚合
```

### 1.1 MOMDP 建模升级

当前 MUPC 奖励函数为标量加权：

```
r = w_safety · r_safety + w_econ · r_econ + w_grid · r_grid + w_shock · r_shock
```

升级为向量化奖励 + 动态偏好：

```
R(s, a) = [r_peak(s,a),  r_cap(s,a),  r_battery_life(s,a)]
           调峰收益        增容收益        电池寿命成本
```

偏好向量 `λ = [λ_peak, λ_cap, λ_life]`，满足 `Σλ_i = 1`，由电网状态动态生成。

### 1.2 动态偏好向量映射

从实时电网状态映射到偏好向量：

| 电网状态 | 条件 | 偏好倾向 |
|----------|------|----------|
| 峰谷差大 + 负载率低 | P_peak_valley > 阈值 AND Load_rate < 0.6 | λ_peak ↑（优先调峰） |
| 峰谷差小 + 负载率高 | P_peak_valley < 阈值 AND Load_rate > 0.8 | λ_cap ↑（优先增容/防过载） |
| 电池老化加速 | SOC波动剧烈 + 高C-rate | λ_life ↑（优先保电池） |

映射函数（可选用 MLP 或规则引擎）：

```
λ = softmax( MLP( [P_peak_valley, Load_rate, SOC_std, C_rate_avg, T_ambient] ) )
```

### 1.3 偏好条件策略网络

Actor-Critic 网络结构升级：

```
当前 Actor:  s (78维) → MLP → [p_ref, k_droop]
升级 Actor:  [s (78维), λ (3维)] → MLP → [p_ref, k_droop]
                                81维输入

当前 Critic: (s, a) → MLP → Q(s,a)
升级 Critic: (s, a, λ) → MLP → Q(s,a,λ)
```

**网络结构（参考论文 + MUPC 适配）：**

| 参数 | 论文值 | MUPC 适配值 |
|------|--------|------------|
| 隐藏层数 | 2 | 2（保持兼容） |
| 隐藏层神经元 | [128, 64] | [256, 128]（适配78维输入） |
| 激活函数 | ReLU | ReLU |
| 偏好拼接方式 | 直接拼接 | 直接拼接（81维输入） |

### 1.4 正则化梯度聚合（替代人工调权）

当 `p_ref` 优化方向与 `k_droop` 优化方向冲突时，求解以下优化问题：

```
min_d ∥d - g_avg∥²
s.t.  d^T · g_i ≥ 0  对所有目标 i
```

其中 `g_i` 为第 i 个奖励分量对策略参数的梯度，`d` 为综合更新方向。

简化版（Frank-Wolfe 一步近似）：

```python
def aggregate_gradients(grads, preferences):
    """grads: List[Tensor], 每个目标一个梯度
       preferences: 当前偏好向量 λ"""
    d = sum(pref * g for pref, g in zip(preferences, grads))
    # 投影：确保不损害任一目标
    for i, g in enumerate(grads):
        if torch.dot(d, g) < 0:
            d = d - (torch.dot(d, g) / torch.dot(g, g)) * g
    return d
```

### 1.5 多因素电池衰减模型

**当前 MUPC**：SOC 硬约束（soc_min/max），充放电功率限制，无寿命量化。

**论文公式**——融合 DOD × C-rate × 温度的耦合衰减模型：

```
Capacity_loss = α(DOD, C_rate, T) × (Ah_throughput)^β

α(DOD, C_rate, T) = α₀ × exp(β₁·DOD + β₂·C_rate + Ea/(R·T))
```

- `DOD`：放电深度 = SOC 波动范围
- `C_rate`：充放电倍率 = |P_batt| / E_batt_rated
- `T`：绝对温度 (K)
- `Ea/R`：Arrhenius 活化能/气体常数（~31700 K for LiFePO4）
- `α₀, β₁, β₂, β`：电芯特定参数（由厂商 datasheet 或循环老化实验标定）

**MUPC 推理侧落地**——在 `SafetyRLWrapper` 中新增：

```rust
// safety_config.rs 新增
pub struct BatteryAgingModel {
    pub alpha_0: f64,        // 基础衰减系数
    pub beta_dod: f64,       // DOD 敏感系数
    pub beta_c_rate: f64,    // C-rate 敏感系数
    pub ea_over_r: f64,      // Arrhenius Ea/R (K)
    pub beta_exponent: f64,  // Ah吞吐量指数
    pub temp_kelvin: f64,    // 电芯温度 (K)
}

impl BatteryAgingModel {
    pub fn capacity_loss_rate(&self, dod: f64, c_rate: f64, ah_throughput: f64) -> f64 {
        let temp_factor = (self.ea_over_r / self.temp_kelvin).exp();
        let alpha = self.alpha_0 * (self.beta_dod * dod).exp()
                  * (self.beta_c_rate * c_rate).exp()
                  * temp_factor;
        alpha * ah_throughput.powf(self.beta_exponent)
    }
}
```

---

## 2. 训练脚本改造

### 2.1 MOMDP 环境改造

在 `env.py` 中新增多目标奖励计算：

```python
class MOMDPEnv:
    def step(self, action):
        # ... 状态转移 ...
        
        # 向量奖励（替代标量）
        r_peak = self._calc_peak_shaving_reward()      # 调峰收益
        r_cap = self._calc_capacity_reward()            # 增容/防过载收益
        r_life = self._calc_battery_life_cost()         # 电池寿命成本（负值）
        
        reward_vector = np.array([r_peak, r_cap, r_life])
        return state, reward_vector, done, info

    def _calc_battery_life_cost(self):
        """多因素电池衰减成本"""
        dod = self.soc_max - self.soc_min
        c_rate = abs(self.battery_power) / self.battery_capacity
        temp_k = self.battery_temp + 273.15
        alpha = self.alpha_0 * exp(self.beta_dod * dod) \
                * exp(self.beta_c_rate * c_rate) \
                * exp(self.ea_over_r / temp_k)
        ah = abs(self.battery_power) * self.dt / self.battery_voltage
        loss = alpha * ah ** self.beta_exponent
        cost = loss * self.battery_replacement_cost
        return -cost  # 负奖励
```

### 2.2 偏好映射网络

```python
class PreferenceMapper(nn.Module):
    """从电网状态映射动态偏好向量"""
    def __init__(self, state_dim=5, hidden=32, n_prefs=3):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(state_dim, hidden),
            nn.ReLU(),
            nn.Linear(hidden, n_prefs),
            nn.Softmax(dim=-1)
        )
        # 输入: [P_peak_valley, Load_rate, SOC_std, C_rate_avg, T_celsius]
        
    def forward(self, grid_state):
        return self.net(grid_state)  # λ ∈ R³, Σλ=1
```

### 2.3 偏好条件策略网络

```python
class PreferenceConditionedActor(nn.Module):
    """偏好条件 Actor"""
    def __init__(self, state_dim=78, pref_dim=3, hidden=[256, 128], action_dim=2):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(state_dim + pref_dim, hidden[0]),
            nn.ReLU(),
            nn.Linear(hidden[0], hidden[1]),
            nn.ReLU(),
            nn.Linear(hidden[1], action_dim),
            nn.Tanh()
        )

    def forward(self, state, preference):
        x = torch.cat([state, preference], dim=-1)
        return self.net(x)  # [p_ref, k_droop]


class PreferenceConditionedCritic(nn.Module):
    """偏好条件 Critic"""
    def __init__(self, state_dim=78, action_dim=2, pref_dim=3, hidden=[256, 128]):
        super().__init__()
        self.net = nn.Sequential(
            nn.Linear(state_dim + action_dim + pref_dim, hidden[0]),
            nn.ReLU(),
            nn.Linear(hidden[0], hidden[1]),
            nn.ReLU(),
            nn.Linear(hidden[1], 1)
        )

    def forward(self, state, action, preference):
        x = torch.cat([state, action, preference], dim=-1)
        return self.net(x)
```

### 2.4 训练循环改造

```python
# 原训练循环
action = actor(state)
scalar_reward = w1 * r1 + w2 * r2 + w3 * r3  # 人工调权
critic_loss = (q_value - scalar_reward).pow(2).mean()

# 新训练循环
preference = preference_mapper(grid_state)
action = actor(state, preference)
vector_reward = [r1, r2, r3]  # 向量奖励

# 正则化梯度聚合
grads = []
for i, r_i in enumerate(vector_reward):
    critic_loss_i = (q_i - r_i).pow(2).mean()
    grads.append(torch.autograd.grad(critic_loss_i, actor.parameters()))

aggregated_grad = aggregate_gradients(grads, preference)
# 手动更新参数
for param, g in zip(actor.parameters(), aggregated_grad):
    param.grad = g
optimizer.step()
```

---

## 3. MUPC 推理侧改造

### 3.1 SafetyRLWrapper 电池模型升级

| 文件 | 改动内容 |
|------|----------|
| `safety_config.rs` | 新增 `BatteryAgingModel` 结构体 |
| `safety_wrapper.rs` | `validate_action()` 中调用 `capacity_loss_rate()` 计算本次动作的电池衰减成本 |
| `env_config.rs` | 新增 `battery_aging` 配置段（YAML 可配） |
| `mupc_env_config.yaml` | 追加电池衰减参数 |

YAML 配置新增：

```yaml
battery_aging:
  alpha_0: 0.002
  beta_dod: 1.5
  beta_c_rate: 0.8
  ea_over_r: 31700.0    # LiFePO4 Arrhenius
  beta_exponent: 0.87
  temp_nominal_kelvin: 298.15
  replacement_cost_yuan_per_kwh: 800.0
```

### 3.2 偏好向量（推理时不需 mapper，偏好为固定或 DB 配置）

推理时偏好向量从 DB 读取（无需电网状态 → 偏好的实时映射），默认值：

```yaml
operational:
  default_preference: [0.5, 0.3, 0.2]  # [调峰, 增容, 保电池]
```

### 3.3 ONNX 导出影响

- 偏好条件 Actor 输入维度：78 + 3 = **81 维**（原 78 维）
- ONNX metadata_props 追加：`mupc_with_preference: "true"`
- RKNN 兼容：仅 Linear + ReLU + Tanh，无新增算子类型

---

## 4. 测试与验证

### 4.1 消融实验

| 配置 | 说明 |
|------|------|
| MADDPG + 标量奖励（基线） | 当前 R1 架构 |
| MADDPG + 向量奖励 + 固定偏好 | 仅升级奖励结构 |
| PCRL + 动态偏好映射 | 完整偏好可控 |
| PCRL + 电池衰减模型 | 本文完整方案 |

### 4.2 精度与效果目标

| 指标 | 目标 | 测量方法 |
|------|------|----------|
| 调峰收益（vs 基线） | 提升 ≥ 5% | 仿真测试集 |
| 电池等效循环寿命 | 延长 ≥ 20% | 循环老化模型估算 |
| 策略网络推理延迟增加 | ≤ 0.5ms（81维 vs 78维） | RK3588 端到端计时 |
| 多目标权衡偏好方向余弦 | ≥ 0.90 | 偏好向量与动作方向一致性 |

---

## 5. 非目标

| 项 | 状态 | 理由 |
|----|------|------|
| MOMDP 4+目标 | 不做 | 3目标（调峰+增容+电池）已覆盖 MUPC 核心场景 |
| 偏好映射网络端侧部署 | 不做 | 偏好向量由 DB 配置固定，无需端侧推理 |
| TD3/SAC 算法替换 | 不做 | MADDPG/PPO 保持主线，PCRL 是框架升级而非算法替换 |
| 电芯级电化学模型 | 不做 | 半经验模型（Arrhenius+DOD+C-rate）已足够量化 |

---

**文档状态**：待 MUPC-AI2 训练管线团队评审
