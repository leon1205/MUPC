# MUPC AI 场景自适应与强化学习完整定义 - 产品需求文档 (PRD)

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 需求分析师 | 草稿 |

---

## 1. 产品概述

### 1.1 背景与问题

MUPC Phase 3C 已实现 LSTM 时序预测、MADDPG/PPO 强化学习决策、RKNN Runtime NPU 推理的基础框架。当前实现存在以下功能缺口：

| 缺口编号 | 描述 | 优先级 |
|----------|------|--------|
| GAP-01 | AI引擎无场景分类器，无法识别农网灌溉/工商业模式，无法自动切换优化目标 | 高 |
| GAP-02 | ModelInput/SystemState 缺少电价、气象、调度指令、预测数据、电能质量指标维度 | 高 |
| GAP-03 | 5种场景奖励函数公式均未文档化，无法指导模型训练与验收 | 高 |
| GAP-04 | ActionOutput 缺少 Q_batt_set（无功设定）和分相补偿系数 | 高 |
| GAP-05 | 无可量化场景切换响应时间指标，无法验证场景自适应性能 | 中 |

### 1.2 产品定位

本 PRD 定义的功能集是 MUPC AI 优化引擎从"基础推理能力"升级为"场景感知决策系统"的关键增量。通过补充场景分类器、完整状态/动作空间、奖励函数体系，使 AI 引擎能够根据台区实际运行场景自动调整优化策略，实现真正的自适应多目标优化。

### 1.3 核心价值

| 价值 | 说明 | 量化目标 |
|------|------|----------|
| 场景自适应 | 无需人工干预，自动识别运行场景并切换最优策略 | 场景识别准确率 >= 95% |
| 多目标优化 | 根据不同场景动态平衡经济收益、设备寿命、电网安全 | 综合目标函数值提升 >= 20%（相比固定策略） |
| 全维度决策 | 覆盖有功、无功、分相补偿三个控制维度 | 无功调节覆盖率 100% |
| 可量化训练 | 每个场景有明确奖励公式，模型训练目标可量化 | 奖励函数可计算误差 < 1% |

### 1.4 目标平台

| 项目 | 要求 |
|------|------|
| 硬件 | RK3588 (NPU: 6 TOPS) |
| 操作系统 | openEuler 22.03+ |
| 推理框架 | RKNN Runtime (INT8 量化) |
| 训练平台 | x86 服务器 (PyTorch + rknn-toolkit2) |
| 集成模块 | ai-engine, strategy-engine, data-processing |

---

## 2. 用户角色

| 角色 | 描述 | 权限范围 | 使用场景 |
|------|------|----------|----------|
| **AI运维人员** | 负责 AI 模型训练、部署、监控和维护的技术人员 | 模型版本管理、推理参数配置、场景切换阈值调整、在线微调启停 | 模型部署、性能监控、异常诊断 |
| **策略管理员** | 负责配置和调整优化目标权重的电力系统运维人员 | 场景权重配置、奖励函数参数调整、手动模式切换 | 根据季节/电价调整优化目标、手动干预异常行为 |
| **本地运维人员** | 负责 MUPC 装置日常运维的操作人员 | 查看 AI 决策日志、接收模式切换告警、强制降级至本地策略 | 巡检、故障响应 |

**权限优先级规则：**
- AI运维人员可配置所有模型参数和训练参数，但不能手动切换运行模式
- 策略管理员可以手动切换运行模式和调整权重，但不能修改模型结构
- 本地运维人员可强制降级至本地策略模式，覆盖 AI 输出

---

## 3. 核心功能列表

### 3.1 多源数据融合

#### 功能 3.1.1：数据采集与融合引擎

**User Story:**
> 作为 AI 引擎，我需要周期性融合实时电气数据、电池数据、外部信息（电价、气象、调度指令），以便为场景识别和 RL 决策提供完整的输入数据。

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| FUSION-01 | 融合周期固定 1 秒，支持运行时配置（范围 1 秒 ~ 60 秒） | 配置文件验证 + 运行时 API 测试 |
| FUSION-02 | 融合输出数据包含以下 7 个字段（定义见 3.3 节），缺一不可 | 单元测试 |
| FUSION-03 | 每个字段的值类型和取值范围符合 3.3 节定义 | 单元测试 |
| FUSION-04 | 数据源异常（如气象 API 不可用）不阻塞融合流程，缺失字段标记为 NaN，取上一周期值填充 | 集成测试（模拟数据源故障）|
| FUSION-05 | 融合数据写入共享内存缓冲区，供场景分类器和 RL 决策器以读锁方式访问，读写锁冲突等待时间 < 1ms | 性能测试 |
| FUSION-06 | 融合输出带 UTC 时间戳，精度到毫秒 | 单元测试 |

**数据源映射：**

| 融合字段 | 数据来源 | 获取方式 | 更新频率 |
|----------|----------|----------|----------|
| 实时电气量 | intercore 模块（实时控制模块）| 核间 TCP 通信 | 1 Hz |
| 电池数据 | intercore 模块（BMS） | 核间 TCP 通信 | 1 Hz |
| 电价 | data-processing / MQTT 北向（物联平台） | 定时拉取 + 订阅推送 | 15 分钟（或电价变更事件） |
| 气象 | data-processing / MQTT 北向（气象 API） | 定时拉取 | 15 分钟 |
| 调度指令 | gateway (IEC 104 / IEC 61850) | 事件驱动 | 事件触发 |

#### 功能 3.1.2：数据源健康监控

**User Story:**
> 作为 AI 运维人员，我需要监控各数据源的连接状态和时延，以便及时发现数据异常并定位问题。

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| FUSION-07 | 每个数据源记录最后一次成功获取的时间戳和状态码 | 单元测试 |
| FUSION-08 | 数据源连续 3 个周期无更新时，产生 WARN 级别告警 | 集成测试 |
| FUSION-09 | 数据源连续 10 个周期无更新时，产生 ERROR 级别告警并通知策略管理员 | 集成测试 |
| FUSION-10 | 数据源健康状态通过 Web UI 实时展示（绿色=正常，黄色=延迟，红色=断连） | UI 集成测试 |

---

### 3.2 场景自适应识别

#### 功能 3.2.1：场景分类器

**User Story:**
> 作为策略管理员，我需要 AI 引擎根据负荷与电源特征自动识别当前运行场景，以便系统自动选择匹配的优化目标和奖励函数。

**场景定义：**

| 场景 ID | 场景名称 | 特征规则 | 典型时段 |
|---------|----------|----------|----------|
| SCENE-01 | 农网灌溉模式(A) | 灌溉负荷占比 > 60% & 当前月份在灌溉季(4月~9月) | 4月~9月 |
| SCENE-02 | 工商业模式-自主套利(B1) | 工商业负荷占比 > 70% & 分时电价在峰时段 | 峰时段(如 10:00~12:00, 15:00~19:00) |
| SCENE-03 | 工商业模式-需量控制(B2) | 当前需量 > 需量阈值的 90% & 上月最大需量 > 需量合同值 | 每月最后一周 |
| SCENE-04 | 工商业模式-虚拟电厂(B3) | VPP 调度指令有效 & 已注册 VPP 服务 | VPP 调度时段 |
| SCENE-05 | 工商业模式-极致绿色(B5) | 绿色电力消纳比例 < 50% & 碳排强度高于区域均值 | 全天 |

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| SCENE-01 | 基于最近 30 分钟平均负荷特征进行分类，分类周期 <= 60 秒 | 性能测试 |
| SCENE-02 | 场景识别准确率 >= 95%（错判为其他场景视为误判，无法识别视为漏判） | 回测验证（使用历史数据） |
| SCENE-03 | 场景切换响应时间 <= 5 秒（从特征数据到达至分类结果输出） | 集成测试 |
| SCENE-04 | 场景切换时自动更新优化目标权重（权重映射见 3.6 节） | 集成测试 |
| SCENE-05 | 支持通过 Web UI 手动强制指定场景，手动指定后自动场景识别暂停 | UI 集成测试 |
| SCENE-06 | 手动指定的场景生效 30 分钟后自动恢复自动识别模式 | 集成测试 |

**场景分类器接口定义：**

```rust
/// 运行场景枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatingScene {
    /// 农网灌溉模式
    AgriculturalIrrigation,
    /// 工商业模式-自主套利
    CommercialArbitrage,
    /// 工商业模式-需量控制
    DemandControl,
    /// 工商业模式-虚拟电厂
    VirtualPowerPlant,
    /// 工商业模式-极致绿色
    UltraGreen,
    /// 未识别/默认
    Default,
}

/// 场景识别结果
#[derive(Debug, Clone, Serialize)]
pub struct SceneRecognitionResult {
    /// 识别到的场景
    pub scene: OperatingScene,
    /// 置信度 (0.0 ~ 1.0)
    pub confidence: f64,
    /// 各场景概率分布
    pub scene_probabilities: HashMap<OperatingScene, f64>,
    /// 判断依据的特征摘要
    pub features_summary: SceneFeatures,
    /// 时间戳
    pub timestamp: i64,
}

/// 场景特征输入
#[derive(Debug, Clone)]
pub struct SceneFeatures {
    /// 灌溉负荷占比 (0.0 ~ 1.0)
    pub irrigation_load_ratio: f64,
    /// 工商业负荷占比 (0.0 ~ 1.0)
    pub commercial_load_ratio: f64,
    /// 当前需量与需量合同值之比 (0.0 ~ 2.0)
    pub demand_ratio: f64,
    /// VPP 调度指令是否有效
    pub vpp_command_active: bool,
    /// 光伏消纳比例 (0.0 ~ 1.0)
    pub pv_consumption_ratio: f64,
    /// 绿色电力消纳比例 (0.0 ~ 1.0)
    pub green_energy_ratio: f64,
    /// 分时电价时段标识 (valley/flat/peak/shoulder)
    pub tariff_period: TariffPeriod,
}
```

---

### 3.3 完整 RL 状态空间定义

#### 功能 3.3.1：状态空间结构

**User Story:**
> 作为 AI 运维人员，我需要 RL 模型接收包含完整 7 个维度的状态空间，以便模型决策能够综合考虑实时数据、预测数据、电价、需量状态和电能质量指标。

**状态空间维度定义：**

| 维度 | 字段名 | 数据类型 | 取值范围 | 单位 | 说明 | 来源 |
|------|--------|----------|----------|------|------|------|
| D1-实时数据 | battery_soc | f64 | [0.0, 1.0] | 无量纲 | 电池荷电状态 | intercore |
| D1-实时数据 | pv_power | f64 | [-1000.0, 1000.0] | kW | 光伏出力（正值=发电） | intercore |
| D1-实时数据 | load_power | f64 | [-1000.0, 1000.0] | kW | 负荷功率（正值=用电） | intercore |
| D1-实时数据 | grid_power | f64 | [-1000.0, 1000.0] | kW | 电网交换功率（正值=从电网购电） | intercore |
| D1-实时数据 | transformer_load | f64 | [0.0, 2.0] | 无量纲 | 变压器负载率（1.0=额定，>1.0=过载） | intercore |
| D1-实时数据 | battery_power | f64 | [-500.0, 500.0] | kW | 电池当前充放电功率（负值=充电，正值=放电） | intercore |
| D2-预测数据 | pv_forecast_15min | Vec<f64>(15) | [-1000.0, 1000.0] | kW | 未来15分钟光伏预测，每分钟一个采样点 | LSTM |
| D2-预测数据 | load_forecast_15min | Vec<f64>(15) | [-1000.0, 1000.0] | kW | 未来15分钟负荷预测，每分钟一个采样点 | LSTM |
| D3-电价 | current_electricity_price | f64 | [0.0, 2.0] | 元/kWh | 当前实时电价 | 物联平台 |
| D3-电价 | next_period_price | f64 | [0.0, 2.0] | 元/kWh | 下一时段电价（用于套利决策） | 物联平台 |
| D3-电价 | price_tariff_id | u8 | {0=谷, 1=平, 2=峰, 3=尖峰} | 枚举 | 当前分时电价时段标识 | 物联平台 |
| D4-需量状态 | current_demand | f64 | [0.0, 10000.0] | kW | 当前实际需量 | intercore |
| D4-需量状态 | contract_demand | f64 | [0.0, 10000.0] | kW | 需量合同值 | 配置 |
| D4-需量状态 | peak_demand_this_month | f64 | [0.0, 10000.0] | kW | 本月最大需量 | data-processing |
| D5-电能质量 | voltage_phase_a | f64 | [0.8, 1.2] | p.u. | A 相电压标幺值 | intercore |
| D5-电能质量 | voltage_phase_b | f64 | [0.8, 1.2] | p.u. | B 相电压标幺值 | intercore |
| D5-电能质量 | voltage_phase_c | f64 | [0.8, 1.2] | p.u. | C 相电压标幺值 | intercore |
| D5-电能质量 | voltage_unbalance | f64 | [0.0, 0.05] | 无量纲 | 三相电压不平衡度（国标 < 0.02 为合格） | intercore |
| D5-电能质量 | frequency | f64 | [49.5, 50.5] | Hz | 电网频率 | intercore |
| D6-气象 | solar_irradiance | f64 | [0.0, 1500.0] | W/m^2 | 当前光照强度 | 气象 API |
| D6-气象 | temperature | f64 | [-20.0, 60.0] | deg C | 环境温度 | 气象 API |
| D7-调度指令 | dispatch_p_set | Option<f64> | [-1000.0, 1000.0] | kW | 调度主站下发的有功设定值（None=无指令） | gateway |
| D7-调度指令 | dispatch_q_set | Option<f64> | [-1000.0, 1000.0] | kVar | 调度主站下发的无功设定值（None=无指令） | gateway |

**状态空间维度总计：** 7 个大类，23 个具体字段（其中 2 个为向量字段 Vec<f64>(15) + 2 个为 Option 字段）

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| STATE-01 | 状态空间结构包含全部 7 个大类、23 个字段 | 单元测试（字段计数验证） |
| STATE-02 | 每个字段的数据类型和取值范围与定义严格一致 | 单元测试（边界值验证） |
| STATE-03 | 预测数据向量长度固定为 15，超出/不足时自动裁剪/补零 | 单元测试 |
| STATE-04 | Option 字段为 None 时，RL 决策器自动取其维度值 = 0.0 并跳过相关约束 | 集成测试 |
| STATE-05 | 状态空间序列化为推理输入向量时，各维度按定义顺序拼接，向量总长度为 35（9个标量 + Option字段2个 + 2个向量各15个 + 气象2个 + 电能质量5个= 9+2+2+15+15+2+5=50，需精确统计） | 单元测试 |
| STATE-06 | 状态输入到推理开始的总延迟 < 5ms（从融合数据就绪到 RKNN Runtime 接收输入） | 性能测试 |

---

### 3.4 完整 RL 动作空间定义

#### 功能 3.4.1：动作空间结构

**User Story:**
> 作为策略管理员，我需要 RL 模型能够输出包含有功设定、无功设定和分相补偿系数的完整动作空间，以便实现对微电网的三维调控。

**动作空间维度定义：**

| 维度 | 字段名 | 数据类型 | 取值范围 | 单位 | 说明 |
|------|--------|----------|----------|------|------|
| A1 | p_batt_set | f64 | [-500.0, 500.0] | kW | 电池有功功率设定值（负值=充电，正值=放电） |
| A2 | q_batt_set | f64 | [-300.0, 300.0] | kVar | 装置无功功率设定值（负值=感性，正值=容性） |
| A3a | compens_factor_a | f64 | [-1.0, 1.0] | 无量纲 | A 相分相补偿系数（正值=增加该相无功补偿） |
| A3b | compens_factor_b | f64 | [-1.0, 1.0] | 无量纲 | B 相分相补偿系数 |
| A3c | compens_factor_c | f64 | [-1.0, 1.0] | 无量纲 | C 相分相补偿系数 |
| A4 | load_shedding | f64 | [0.0, 500.0] | kW | 可中断负荷切除量 |
| A5 | pv_limit | f64 | [0.0, 1.0] | 无量纲 | 光伏限功率比例（0.0=完全限功率，1.0=不限功率） |

**动作空间维度总计：** 7 个动作维度

**约束规则：**

| 规则 ID | 约束条件 | 说明 |
|---------|----------|------|
| ACT-01 | p_batt_set 的变化率 <= 50 kW/周期 (1s) | 防止电池功率突变 |
| ACT-02 | q_batt_set 的变化率 <= 30 kVar/周期 (1s) | 防止无功突变 |
| ACT-03 | p_batt_set 与 q_batt_set 须满足视在功率约束：sqrt(p_batt_set^2 + q_batt_set^2) <= S_max，S_max = 500kVA (可配置) | 功率圆限制 |
| ACT-04 | compens_factor_a + compens_factor_b + compens_factor_c = 0.0（三相总无功补偿代数和为0）| 分相补偿仅调节三相不平衡，不改变总无功 |
| ACT-05 | pv_limit 不得低于 0.1（最低保留 10% PV 出力，防逆流场景除外允许降至 0.0）| 光伏限功率下限保护 |
| ACT-06 | 当 dispatch_p_set 有效时，p_batt_set 的绝对值不得超过 dispatch_p_set 的绝对值 | 调度指令权限约束 |

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| ACT-07 | 动作空间结构包含全部 7 个动作维度 | 单元测试（字段计数验证） |
| ACT-08 | 每个动作维度的取值范围严格执行定义边界 | 单元测试（边界值验证 + clamp 验证）|
| ACT-09 | 5 条约束规则均在动作输出时执行校验，违反约束时自动 clamp 并记录 WARN 日志 | 集成测试 |
| ACT-10 | Constraint ACT-04（三相补偿和为 0）违反时自动归一化处理，不得丢弃 | 集成测试 |
| ACT-11 | 约束校验总延迟 < 0.5ms | 性能测试 |

**接口定义：**

```rust
/// 强化学习决策输出（完整动作空间）
#[derive(Debug, Clone)]
pub struct ActionOutput {
    /// 电池有功功率设定值 (kW), 范围 [-500.0, 500.0], 负值=充电, 正值=放电
    pub p_batt_set: f64,
    /// 装置无功功率设定值 (kVar), 范围 [-300.0, 300.0], 负值=感性, 正值=容性
    pub q_batt_set: f64,
    /// A 相分相补偿系数, 范围 [-1.0, 1.0]
    pub compens_factor_a: f64,
    /// B 相分相补偿系数, 范围 [-1.0, 1.0]
    pub compens_factor_b: f64,
    /// C 相分相补偿系数, 范围 [-1.0, 1.0]
    pub compens_factor_c: f64,
    /// 可中断负荷切除量 (kW), 范围 [0.0, 500.0]
    pub load_shedding: f64,
    /// 光伏限功率比例, 范围 [0.0, 1.0], 0.0=完全限功率, 1.0=不限功率
    pub pv_limit: f64,
    /// 决策置信度 (0.0 ~ 1.0), 模型输出
    pub confidence: f64,
}
```

---

### 3.5 5 种场景奖励函数

#### 功能 3.5.1：场景奖励函数体系

**User Story:**
> 作为 AI 运维人员，我需要每个场景有明确可计算的奖励函数公式，以便在模型训练时提供一致的优化目标，在部署后能够实时计算奖励值用于在线微调。

##### 3.5.1.1 农网灌溉模式 (SCENE-01)

**优化目标：** 最大化光伏消纳 + 电压治理，最小化变压器过载风险

**奖励函数公式：**

```
R_agri = w1 * R_pv_consumption + w2 * R_voltage_quality - w3 * P_battery_degradation - w4 * P_transformer_overload

R_pv_consumption = min(P_pv_self_consume / P_pv_total, 1.0) * 100
  -- P_pv_self_consume: 光伏自发自用电量 (kWh)
  -- P_pv_total: 光伏总发电量 (kWh)

R_voltage_quality = 100 * max(0, 1 - |V_a - 1.0| / 0.1 - |V_b - 1.0| / 0.1 - |V_c - 1.0| / 0.1)
  -- V_a, V_b, V_c: 三相电压标幺值
  -- 电压偏离 1.0 p.u. 越远，惩罚线性增大，偏离超过 0.1 p.u. 时奖励归零

P_battery_degradation = alpha * |delta_SOC| / SOC_total_range * 100
  -- alpha: 电池退化系数，默认 0.1
  -- delta_SOC: 单步 SOC 变化量
  -- SOC_total_range: SOC 可用范围 (default: 0.9 - 0.1 = 0.8)

P_transformer_overload = 200 * max(0, L_transformer - 1.0)
  -- L_transformer: 变压器负载率 (当前负载/额定容量)
  -- 仅在过载时产生惩罚，过载 10% 时惩罚值 = 20
```

**权重配置（默认值）：**

| 权重 | 默认值 | 说明 | 可配置范围 |
|------|--------|------|------------|
| w1 | 1.0 | 光伏消纳奖励权重 | [0.0, 3.0] |
| w2 | 1.0 | 电压质量奖励权重 | [0.0, 3.0] |
| w3 | 0.5 | 电池损耗惩罚权重 | [0.0, 2.0] |
| w4 | 2.0 | 变压器过载惩罚权重 | [0.0, 5.0] |

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| REWARD-A1 | 光伏完全消纳(P_pv_self_consume = P_pv_total)时 R_pv_consumption = 100 | 单元测试 |
| REWARD-A2 | 电压 1.0 p.u. 时 R_voltage_quality = 100, 电压 1.1 p.u. 时 R_voltage_quality = 0 | 单元测试 |
| REWARD-A3 | 变压器负载率 1.0 时 P_transformer_overload = 0, 1.1 时 = 20 | 单元测试 |
| REWARD-A4 | delta_SOC = 0 时 P_battery_degradation = 0 | 单元测试 |
| REWARD-A5 | 奖励函数完整计算时间 < 1ms | 性能测试 |

##### 3.5.1.2 工商业模式-自主套利 (SCENE-B1)

**优化目标：** 最大化峰谷电价差收益，最小化电池损耗

**奖励函数公式：**

```
R_arbitrage = w1 * R_price_spread - w2 * P_battery_degradation

R_price_spread = sum_over_steps(P_batt_set * delta_t * (price_sell - price_buy)) * conversion_factor
  -- P_batt_set: 电池功率 (kW, 正值=放电/卖电, 负值=充电/买电)
  -- delta_t: 时间步长 (小时)
  -- price_sell: 售电价 (元/kWh), 当前时段
  -- price_buy: 购电价 (元/kWh), 充电时段
  -- conversion_factor: 收益到奖励的转换系数，默认 10 (元 → 奖励积分)

P_battery_degradation = beta * sum_over_steps(|P_batt_set| * delta_t) / E_battery_total * 100
  -- beta: 套利场景电池退化因子，默认 0.15
  -- E_battery_total: 电池总容量 (kWh)
```

**权重配置（默认值）：**

| 权重 | 默认值 | 说明 | 可配置范围 |
|------|--------|------|------------|
| w1 | 1.0 | 电价差收益权重 | [0.0, 3.0] |
| w2 | 1.0 | 电池损耗惩罚权重 | [0.0, 3.0] |

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| REWARD-B1 | 峰时放电(P_batt_set > 0)、谷时充电(P_batt_set < 0)时 R_price_spread > 0 | 单元测试 |
| REWARD-B2 | 峰时充电(P_batt_set < 0)时 R_price_spread < 0（策略错误惩罚）| 单元测试 |
| REWARD-B3 | 电池无动作(P_batt_set = 0)时 R_arbitrage = 0 | 单元测试 |
| REWARD-B4 | 奖励函数完整计算时间 < 1ms | 性能测试 |

##### 3.5.1.3 工商业模式-需量控制 (SCENE-B2)

**优化目标：** 减免需量罚金

**奖励函数公式：**

```
R_demand = w1 * R_demand_penalty_avoidance - w2 * P_comfort_loss

R_demand_penalty_avoidance = max(0, D_peak_baseline - D_peak_actual) * penalty_rate
  -- D_peak_baseline: 上月最大需量 (kW), 或当前自然月截至上一周期的最大需量
  -- D_peak_actual: 当前周期内实际最大需量 (kW)
  -- penalty_rate: 需量罚金率 (元/kW), 默认 30 元/kW, 可配置

P_comfort_loss = gamma * P_load_shed * delta_t * price_loss
  -- gamma: 舒适度损失因子，默认 1.0
  -- P_load_shed: 切负荷量 (kW)
  -- delta_t: 切负荷持续时间 (小时)
  -- price_loss: 单位电量损失估价 (元/kWh), 默认 5 元/kWh
```

**权重配置（默认值）：**

| 权重 | 默认值 | 说明 | 可配置范围 |
|------|--------|------|------------|
| w1 | 1.0 | 需量罚金减免权重 | [0.0, 3.0] |
| w2 | 0.5 | 舒适度损失惩罚权重 | [0.0, 3.0] |

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| REWARD-C1 | 成功削峰 100kW(D_peak_actual < D_peak_baseline)时 R_demand_penalty_avoidance = 100 * penalty_rate | 单元测试 |
| REWARD-C2 | D_peak_actual >= D_peak_baseline 时 R_demand_penalty_avoidance = 0 | 单元测试 |
| REWARD-C3 | 无切负荷时 P_comfort_loss = 0 | 单元测试 |
| REWARD-C4 | 切负荷量加倍时 P_comfort_loss 线性加倍 | 单元测试 |

##### 3.5.1.4 工商业模式-虚拟电厂 (SCENE-B3)

**优化目标：** 最大化辅助服务收益 + 响应精度

**奖励函数公式：**

```
R_vpp = w1 * R_ancillary_service + w2 * R_response_accuracy - w3 * P_deadline_deviation

R_ancillary_service = P_regulation_capacity * capacity_price + P_regulation_mileage * mileage_price
  -- P_regulation_capacity: 调频/调峰中标容量 (MW)
  -- capacity_price: 容量价格 (元/MW/小时), 由 VPP 合同约定
  -- P_regulation_mileage: 调频里程 (MW)
  -- mileage_price: 里程价格 (元/MW)

R_response_accuracy = 100 * max(0, 1 - |P_actual - P_target| / P_target_range)
  -- P_actual: 实际响应功率 (MW)
  -- P_target: VPP 调度目标功率 (MW)
  -- P_target_range: VPP 调度目标允许偏差范围 (MW), default: 0.1 * P_target

P_deadline_deviation = delta_t_response / T_allowed * 100
  -- delta_t_response: 实际响应延迟 (秒)
  -- T_allowed: 允许最大响应延迟 (秒), 默认 60 秒
```

**权重配置（默认值）：**

| 权重 | 默认值 | 说明 | 可配置范围 |
|------|--------|------|------------|
| w1 | 1.0 | 辅助服务收益权重 | [0.0, 3.0] |
| w2 | 2.0 | 响应精度权重（VPP 考核重点）| [0.0, 5.0] |
| w3 | 1.0 | 响应延迟惩罚权重 | [0.0, 3.0] |

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| REWARD-D1 | P_actual = P_target 时 R_response_accuracy = 100 | 单元测试 |
| REWARD-D2 | P_actual 偏离超过 P_target_range 时 R_response_accuracy = 0 | 单元测试 |
| REWARD-D3 | 响应延迟 delta_t <= T_allowed 时 P_deadline_deviation <= 100 | 单元测试 |
| REWARD-D4 | VPP 指令无效时 R_vpp 强制置 0 | 集成测试 |

##### 3.5.1.5 工商业模式-极致绿色 (SCENE-B5)

**优化目标：** 最大化绿电消纳比例，最小化碳排放

**奖励函数公式：**

```
R_green = w1 * R_green_consumption + w2 * R_carbon_reduction

R_green_consumption = 100 * E_green_self_consume / E_total_consume
  -- E_green_self_consume: 绿电自发自用量 (kWh)
  -- E_total_consume: 总用电量 (kWh)

R_carbon_reduction = 100 * (C_baseline - C_actual) / C_baseline
  -- C_baseline: 基准碳排放强度 (kg CO2/kWh), 由区域电网平均碳排放因子确定
  -- C_actual: 实际碳排放强度 (kg CO2/kWh)
  -- 计算方式: C_actual = (E_grid_purchase * grid_emission_factor) / E_total_consume
```

**权重配置（默认值）：**

| 权重 | 默认值 | 说明 | 可配置范围 |
|------|--------|------|------------|
| w1 | 1.0 | 绿电消纳比例权重 | [0.0, 3.0] |
| w2 | 1.0 | 碳减排量权重 | [0.0, 3.0] |

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| REWARD-E1 | 全部用电来自绿电(E_green_self_consume = E_total_consume)时 R_green_consumption = 100 | 单元测试 |
| REWARD-E2 | C_actual = 0（完全零碳）时 R_carbon_reduction = 100 | 单元测试 |
| REWARD-E3 | C_actual >= C_baseline 时 R_carbon_reduction = 0 | 单元测试 |
| REWARD-E4 | 电网排放因子 grid_emission_factor 从配置文件读取，默认 0.581 kg CO2/kWh（华中电网2024年均值）| 配置验证 |

---

### 3.6 动态权重调整机制

#### 功能 3.6.1：场景-权重自动映射

**User Story:**
> 作为策略管理员，我需要优化目标权重根据场景自动调整，以便在不同的运行模式下实现差异化的优化目标。

**场景-权重映射表：**

| 场景 | w1(op1) | w2(op2) | w3(op3) | w4(op4) | 说明 |
|------|---------|---------|---------|---------|------|
| 农网灌溉 | 1.0(光伏消纳) | 1.0(电压质量) | 0.5(电池损耗) | 2.0(变压器) | 变压器过载惩罚最重 |
| 自主套利 | 1.0(电价收益) | 1.0(电池损耗) | - | - | 经济性主导 |
| 需量控制 | 1.0(需量减免) | 0.5(舒适损失) | - | - | 需量费减免为主 |
| VPP | 1.0(辅助收益) | 2.0(响应精度) | 1.0(延迟惩罚) | - | 响应精度权重最高 |
| 极致绿色 | 1.0(绿电消纳) | 1.0(碳减排) | - | - | 环境效益导向 |

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| WEIGHT-01 | 场景切换时权重映射自动更新，延迟 < 1s | 集成测试 |
| WEIGHT-02 | 权重映射表可通过配置文件修改，支持热加载 | 配置测试 |
| WEIGHT-03 | 策略管理员可通过 Web UI 手动调整当前场景权重，调整即时生效 | UI 集成测试 |
| WEIGHT-04 | 手动调整的权重在下一次场景切换时复位为权重映射表默认值 | 集成测试 |
| WEIGHT-05 | 权重修改记录操作日志，包含修改人、修改时间、修改前后值 | 日志验证 |

---

### 3.7 NPU 推理性能要求

#### 功能 3.7.1：专用计算资源调度

**User Story:**
> 作为 AI 运维人员，我需要 AI 推理被分配专用 NPU 计算资源，以确保推理延迟满足实时性要求。

**验收标准：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| NPU-01 | AI 推理任务独占 RK3588 NPU 核心，不与非 AI 任务共享 | 内核调度配置验证 |
| NPU-02 | NPU 推理延迟 < 100ms（从输入张量就绪到输出张量就绪）| 性能测试（1000 次采样，P99 < 100ms）|
| NPU-03 | 模型推理总延迟（状态输入 + 推理 + 动作输出校验）< 120ms | 性能测试 |
| NPU-04 | NPU 推理失败时自动降级至 CPU 推理，降级延迟 < 5s | 集成测试（模拟 NPU 故障）|
| NPU-05 | CPU 推理模式下推理延迟 < 500ms（降级模式，仅维持基本功能）| 性能测试 |
| NPU-06 | NPU 温度监控，温度超过 85 deg C 时触发降频保护，推理频率降低不超过初始频率的 50% | 压力测试 |

**推理延迟预算分配：**

| 阶段 | 最大延迟 | 说明 |
|------|----------|------|
| 状态输入准备 | 5ms | 融合数据读取 + 特征向量序列化 |
| NPU 推理 | 100ms | RKNN Runtime run() 调用 |
| 动作输出校验 | 0.5ms | 约束规则校验 + clamp |
| 总端到端延迟 | 120ms | 从状态输入就绪到动作输出可用 |

---

## 4. 非功能性需求

### 4.1 推理性能

| 指标 | 要求 | 测量方法 |
|------|------|----------|
| NPU 推理延迟 | < 100ms (P99) | 1000 次连续推理，计算 P99 百分位 |
| 场景识别延迟 | < 5s (从特征数据到达至分类结果输出) | 模拟 100 次场景切换，计算最大延迟 |
| 状态空间构建延迟 | < 5ms | 1000 次构建，计算平均延迟 |
| 动作约束校验延迟 | < 0.5ms | 1000 次校验，计算平均延迟 |
| 奖励函数计算延迟 | < 1ms (单场景) | 各场景 1000 次计算，计算平均延迟 |
| 数据融合周期 | 1Hz（默认），可配置范围 1s ~ 60s | 运行时观测 |
| AI 完整决策周期 | 1Hz（默认，与融合周期一致）| 运行时观测 |
| 在线微调延迟 | <= 10s（单次微调，batch_size=32）| 性能测试 |

### 4.2 模型精度

| 指标 | 要求 | 测量方法 |
|------|------|----------|
| 场景识别准确率 | >= 95% | 使用标注历史数据回测，混淆矩阵评估 |
| 光伏预测 MAPE | <= 10%（15 分钟预测范围） | 回测验证，Mean Absolute Percentage Error |
| 负荷预测 MAPE | <= 15%（15 分钟预测范围） | 回测验证 |
| RL 决策综合回报 | 相比固定策略提升 >= 20% | 对比实验（相同数据，AI 策略 vs 固定策略）|

### 4.3 模型大小与资源占用

| 指标 | 要求 |
|------|------|
| 单模型 INT8 量化后大小 | <= 5MB（.rknn 文件）|
| 推理运行时内存占用 | <= 200MB（所有模型加载后）|
| 训练数据本地存储 | <= 1GB（保留最近 30 天训练数据）|
| 日志存储 | 按现有滚动策略（单文件 10MB，保留 10 个）|

### 4.4 可靠性

| 指标 | 要求 |
|------|------|
| AI 引擎 MTBF | >= 1,000 小时（不含硬件故障）|
| AI 失效时自动降级至本地策略 | < 2s（从检测到异常至切换完成）|
| 模型热加载（替换不中断推理）| 支持（双缓冲模式，加载新模型期间旧模型继续服务）|
| 模型版本回滚 | 支持回滚至上一次稳定版本，回滚时间 < 30s |

### 4.5 安全性

| 需求 | 说明 |
|------|------|
| 模型文件完整性 | 模型文件加载前进行 SHA256 校验，校验失败拒绝加载 |
| 推理输入验证 | 对输入张量进行 NaN/Inf 检查，异常输入拒绝推理并告警 |
| 动作输出限幅 | 所有动作输出经安全限幅后再下发，防止异常值导致设备损坏 |
| 在线微调防护 | 在线微调仅在系统闲时（负荷率 < 30%）触发，微调不得影响推理性能 |
| 配置加密 | 奖励函数权重参数存储在加密配置文件中 |

---

## 5. 边界条件与异常处理

### 5.1 场景误判处理

| 异常场景 | 检测条件 | 处理措施 |
|----------|----------|----------|
| 场景分类置信度低于阈值 | confidence < 0.6 | 切换至 Default 模式，使用均衡权重，记录 WARN 日志 |
| 场景频繁切换（振荡）| 5 分钟内切换 >= 3 次 | 锁定当前场景 30 分钟，降低分类器灵敏度，记录 ALERT 日志 |
| 手动/自动场景冲突 | 手动指定的场景到期时间与自动识别结果不一致 | 手动指定到期后，先应用 Default 模式运行 5 分钟，再切换至自动识别 |
| 无场景匹配 | 所有场景的概率均 < 0.4 | 进入 Default 模式，记录 INFO 日志 |

### 5.2 数据缺失处理

| 缺失数据 | 处理方式 | 告警级别 |
|----------|----------|----------|
| 电价数据 | 使用上一有效值，连续缺失 3 个周期后使用默认分时电价表 | WARN |
| 气象数据 | 使用上一有效值，连续缺失 10 个周期后 R_green 奖励置 0 | WARN |
| 调度指令 | 状态空间对应字段置 None，RL 决策跳过相关约束 | INFO |
| 预测数据（LSTM） | 使用全零向量，RL 决策仅依赖实时数据 | WARN |
| 实时数据（intercore） | 使用上一有效值，连续缺失 3 个周期后触发 AI 降级 | ERROR → 降级 |

### 5.3 模型退化处理

| 退化场景 | 检测条件 | 处理措施 |
|----------|----------|----------|
| 推理精度持续下降 | 在线微调阶段的 loss 连续 10 个周期不下降或上升 | 停止在线微调，回滚模型至上一次检查点，通知 AI 运维人员 |
| 推理延迟持续超标 | 连续 100 次推理中 > 10% 超出 150ms | 降级至 CPU 推理模式，记录 ALERT 日志 |
| 模型文件损坏 | SHA256 校验失败 | 拒绝加载，尝试从 OTA 备份恢复，恢复失败则触发 AI 降级 |
| 奖励函数计算异常 | 奖励值明显偏离正常范围（超出 [0, 200]）| 截断至边界值，记录 ERROR 日志，通知 AI 运维人员 |

### 5.4 数据融合异常降级流程

```
任一数据源连续3个周期无更新
    ↓
产生 WARN 告警
    ↓
使用上一有效值填充（最多持续 10 个周期）
    ↓
超过 10 个周期仍未恢复
    ↓
触发 AI 降级流程
    ↓
strategy-engine 进入兜底模式
    ↓
本地策略引擎接管控制
    ↓
待 AI 所需全部数据源恢复 5 个连续周期后，自动切回 AI 模式
```

---

## 6. 与现有系统的集成点

### 6.1 ai-engine 模块变更

| 变更项 | 当前状态 | 目标状态 | 涉及文件 |
|--------|----------|----------|----------|
| SystemState 扩展 | 5 个字段（battery_soc, pv_power, load_power, grid_power, transformer_load）| 23 个字段（7 个大类）| `rl_model.rs` |
| ActionOutput 扩展 | 4 个字段（p_batt_set, load_shedding, pv_limit, confidence）| 8 个字段（+q_batt_set, compens_factor_a/b/c）| `rl_model.rs` |
| 新增场景分类器 | 不存在 | SceneClassifier 模块 | 新增 `scene_classifier.rs` |
| 新增奖励函数计算器 | 不存在 | RewardCalculator 模块 | 新增 `reward_calculator.rs` |
| 新增数据融合器 | 不存在 | DataFusionEngine 模块 | 新增 `data_fusion.rs` |
| 新增动作约束校验器 | 不存在 | ActionValidator 模块 | 新增 `action_validator.rs` |
| RlConfig 扩展 | 无 action_space 字段 | 增加 action_space 和 constraint 配置 | `config.rs` |
| ModelManager 扩展 | 仅管理 LSTM 和 RL | 增加场景分类器、奖励计算器 | `model_manager.rs` |

### 6.2 strategy-engine 集成

| 集成点 | 说明 | 接口 |
|--------|------|------|
| AiIntegrator | 现有策略引擎与 AI 引擎的集成桥接，增加场景信息和奖励值传递 | 扩展现有 trait |
| AiCommandValidator | 使用新的 ActionValidator 替换原有简单校验，增加全部 6 条约束规则 | 扩展 `validate()` 方法 |
| 场景状态通知 | 场景切换时 strategy-engine 接收通知，用于调整本地策略参数 | 消息总线 topic: `ai/scene_change` |
| 兜底策略联动 | 不同场景下本地兜底策略参数自动适配（如农网模式降低防逆流阈值）| 扩展 `FallbackStrategy` trait |

### 6.3 data-processing 集成

| 集成点 | 说明 |
|--------|------|
| 电价数据管道 | data-processing 从 MQTT 订阅物联平台电价数据，按 topic `price/real_time` 发布 |
| 气象数据管道 | data-processing 定时调用气象 API，按 topic `weather/forecast` 发布 |
| 需量数据加工 | data-processing 计算滚动需量值（滑窗 15 分钟），按 topic `demand/current` 发布 |
| 电能质量数据 | data-processing 从 intercore 数据计算三相不平衡度，按 topic `power_quality/voltage_unbalance` 发布 |

### 6.4 消息总线集成

| Topic | 发布者 | 订阅者 | 数据格式 | 频率 |
|-------|--------|--------|----------|------|
| `ai/fused_state` | DataFusionEngine | RLModel, SceneClassifier | `FusedSystemState` JSON | 1Hz |
| `ai/scene_change` | SceneClassifier | ModelManager, Web UI | `SceneRecognitionResult` JSON | 事件驱动 |
| `ai/action_output` | ModelManager | strategy-engine, intercore | `ActionOutput` JSON | 1Hz |
| `ai/reward_value` | RewardCalculator | OnlineUpdater, Web UI | `RewardValue` JSON | 1Hz |
| `ai/model_status` | ModelManager | Web UI, 告警模块 | `ModelStatus` JSON | 1Hz |
| `price/real_time` | data-processing | DataFusionEngine | `ElectricityPrice` JSON | 15min/事件 |
| `weather/forecast` | data-processing | DataFusionEngine | `WeatherData` JSON | 15min |
| `demand/current` | data-processing | DataFusionEngine | `DemandData` JSON | 1Hz |
| `power_quality/voltage_unbalance` | data-processing | DataFusionEngine | `VoltageUnbalance` JSON | 1Hz |

---

## 7. 术语表

| 术语 | 说明 |
|------|------|
| NPU | Neural Processing Unit，神经网络处理器 |
| RKNN | Rockchip Neural Network，瑞芯微 NPU 模型格式 |
| LSTM | Long Short-Term Memory，长短期记忆网络 |
| MADDPG | Multi-Agent Deep Deterministic Policy Gradient，多智能体深度确定性策略梯度 |
| PPO | Proximal Policy Optimization，近端策略优化 |
| MAPE | Mean Absolute Percentage Error，平均绝对百分比误差 |
| SOC | State of Charge，电池荷电状态 |
| VPP | Virtual Power Plant，虚拟电厂 |
| p.u. | Per Unit，标幺值（电压/功率等电气量的归一化表示）|
| 需量 | 指定周期内（通常 15 分钟）的平均功率最大值 |

---

## 8. 附录

### 8.1 功能缺口与 PRD 章节映射

| 缺口编号 | 描述 | 对应 PRD 章节 |
|----------|------|---------------|
| GAP-01 | AI引擎无场景分类器 | 3.2 场景自适应识别 |
| GAP-02 | 状态空间缺少维度 | 3.3 完整 RL 状态空间定义 |
| GAP-03 | 奖励函数未文档化 | 3.5 5 种场景奖励函数 |
| GAP-04 | 动作空间缺少维度 | 3.4 完整 RL 动作空间定义 |
| GAP-05 | 无可量化场景切换响应时间 | 3.2.1 验收标准 SCENE-03 + 4.1 推理性能 |

### 8.2 参考文档

| 文档 | 路径 |
|------|------|
| 功能清单 | `docs/微电网特种调控装置（MUPC）通信管理模块功能清单.md` |
| AI 优化引擎技术设计 | `docs/superpowers/plans/2026-05-28-MUPC-Phase3C-AI优化引擎-设计文档.md` |
| 通信管理模块 PRD | `docs/superpowers/specs/2026-05-27-MUPC-通信管理模块-PRD.md` |
| Phase 3A 规格文档 | `docs/superpowers/specs/2026-05-27-MUPC-Phase3A-规格文档.md` |
| 技术债清单 | `docs/technical-debt.md` |

### 8.3 待澄清问题

| 序号 | 问题 | 优先级 | 影响评估 |
|------|------|--------|----------|
| 1 | 气象数据的外部来源是何种 API（如 和风天气、中国气象局）？是否需要额外的商务授权？ | 高 | 影响 DataFusionEngine 的气象数据获取实现 |
| 2 | 电价数据是直接来自物联平台下发，还是需要通过 MUPC 本地配置？ | 高 | 影响 DataFusionEngine 的电价数据管道设计 |
| 3 | 分相补偿系数的硬件限制（实际的 SVG/APF 能否按系数调节三相无功）？ | 高 | 影响 ActionValidator 的硬件约束规则 |
| 4 | VPP 辅助服务的容量价格和里程价格是否有标准合同模板？还是由 VPP 平台实时下发？ | 中 | 影响 R_ancillary_service 的参数来源 |
| 5 | 在线微调是否需要经过审批流程（安全考虑）？还是自动触发？ | 中 | 影响 OnlineUpdater 的触发策略 |

---

**文档状态：** 草稿（v1.0）

**文档变更记录：**

| 版本 | 日期 | 变更内容 |
|------|------|----------|
| v1.0 | 2026-05-29 | 初始创建，覆盖全部 5 个功能缺口 |
