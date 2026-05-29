# MUPC Web 管理与 AI 可视化模块 — 设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.2 | 2026-05-29 | 架构师 | 合并修订 **[DESIGN_APPROVED]** |

> **v1.2 修订说明：** 同步 v2.0 预设运行场景设计，移除自动识别/置信度/兜底模式/恢复自动识别。详细变更见 `2026-05-29-MUPC-AI预设运行场景与互斥模式选择-设计文档.md` [DESIGN_APPROVED]。

**来源文档：**
- `docs/superpowers/specs/modules/08-MUPC-Web管理与AI可视化-PRD.md` — 产品需求文档 **[REVIEWED: PASS]**
- `docs/superpowers/plans/2026-05-27-MUPC-WebUI-设计.md` — Web UI 设计文档 **[DESIGN_APPROVED]**
- `docs/superpowers/plans/2026-05-29-MUPC-AI可视化与专家干预-设计文档.md` — AI 可视化与专家干预技术设计 **[DESIGN_APPROVED]**

**目标 crate：** `web-api`
**关联 crate：** `ai-engine`、`strategy-engine`、`data-processing`

---

## 1. 模块架构

### 1.1 架构定位

本模块为 MUPC 通信管理模块的本地人机交互层，承载 Web 管理与 AI 可视化核心职责。采用 **"三明治"分层架构**：Web UI（前端）-> web-api（路由/权限/聚合）-> strategy-engine/ai-engine（数据/控制）。

```
┌──────────────────────────────────────────────────────────────────────────┐
│  Web UI 层                                                                │
│  状态监控 | 配置管理 | 日志查看 | AI 决策面板 | 专家干预 | A/B 测试        │
└──────────────────────────────┬───────────────────────────────────────────┘
                                │ HTTP REST + SSE (实时推送)
                                ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  web-api 层 (路由/权限/聚合边界)                                          │
│                                                                           │
│  ┌────────────────────┐ ┌──────────────────┐ ┌────────────────────────┐   │
│  │ 系统管理路由       │ │ AI 可视化路由    │ │ 专家干预路由          │   │
│  │ /api/status        │ │ /api/ai/         │ │ PUT /api/ai/weights   │   │
│  │ /api/config        │ │ predictions      │ │ PUT /api/ai/mode      │   │
│  │ /api/logs          │ │ decision         │ │ PUT /api/v1/mode      │   │
│  │ /ws/logs (WS)      │ │ rewards          │ └──────────┬─────────────┘   │
│  │ /api/auth/*        │ │ status           │            │                 │
│  └────────────────────┘ │ finetuning       │  ┌─────────┴─────────────┐   │
│                         └──────────────────┘  │ A/B 测试 & 模型管理  │   │
│                                                │ /api/ai/models       │   │
│  ┌─────────────────────────────────────────────┤ /api/ai/abtest       │   │
│  │ 基础服务层                                   │ /api/ai/rollback     │   │
│  │ SsePushService (SSE 推送)                   └──────────────────────┘   │
│  │ AuthMiddleware (Session + 角色权限)                                     │
│  │ AuditLogger (审计日志写入)                                              │
│  │ DecisionCache (决策结果缓存)                                            │
│  └──────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────┬───────────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  后端模块 (strategy-engine / ai-engine)                                    │
│                                                                           │
│  strategy-engine::AiIntegrator (服务门面)                                  │
│    ├── get_prediction() → ai-engine::ModelManager::predict()             │
│    ├── get_decision() → DecisionCache                                    │
│    ├── get_reward_history() → SQLite 查询                                │
│    ├── apply_weight_changes() → ModelManager::set_weights()              │
│    ├── apply_mode_switch() → ModeSelector::switch() (v2.0)               │
│    ├── get_model_versions() → manifest.json                              │
│    ├── create_ab_test() → AbTestRouter + ModelManager                    │
│    ├── get_ab_test_result() → SQLite 查询                                │
│    ├── stop_ab_test() → AbTestRouter + ModelManager                      │
│    ├── rollback_model() → ModelManager::rollback_model()                 │
│    └── get_finetune_status() → OnlineUpdater                             │
└──────────────────────────────────────────────────────────────────────────┘
```

### 1.2 设计原则

1. **web-api 作为安全边界**：所有权限校验、输入验证、审计日志记录在 web-api 层完成，后端模块不重复校验。
2. **AiIntegrator 作为服务门面**：web-api 不直接调用 ai-engine，统一通过 strategy-engine 的 AiIntegrator 编排。
3. **只读/写操作分离**：可视化查询（GET）可以适当放宽延迟要求；干预操作（PUT/POST/DELETE）严格要求一致性和审计。
4. **SSE 优先于 WebSocket**：AI 数据流为单向（服务端→前端），SSE 更轻量且与 Axum 集成良好。系统日志推送仍使用 WebSocket（`/ws/logs`），两者并存。
5. **工业控制风格**：以信息展示为主，避免过度装饰，强调功能性和可读性。

### 1.3 技术栈

| 层级 | 技术 | 说明 |
|------|------|------|
| 后端框架 | Axum 0.7 | REST + SSE 原生支持 |
| 前端方案 | 纯 HTML + CSS + JavaScript（或 Vue 3 轻量版） | 页面加载 ≤ 2 秒 |
| 异步运行时 | Tokio | 后台定时任务 + SSE 推送 |
| 实时推送（AI） | SSE (`/api/ai/stream`) | 服务端→浏览器单向 |
| 实时推送（日志） | WebSocket (`/ws/logs`) | 系统日志流 |
| 认证 | Session 登录 | `POST /api/auth/login` |
| HTTP 端口 | 8080 | 默认 |

### 1.4 数据流全景

**决策面板页面加载数据流：**
```
Web UI 打开 /ai/dashboard
  │
  ├── HTTP GET /api/ai/status          → 展示场景模式 + AI 引擎状态
  ├── HTTP GET /api/ai/decision        → 展示系统状态 + 决策动作 + 奖励细分
  ├── HTTP GET /api/ai/predictions     → 展示预测曲线
  ├── HTTP GET /api/ai/rewards?range=24h → 展示奖励趋势图
  │
  └── SSE 连接 /api/ai/stream          → 实时推送更新
       ├── event: status (5s)
       ├── event: decision (决策周期)
       ├── event: predictions (60s)
       └── event: heartbeat (30s)
```

**权重调整操作数据流：**
```
Web UI → 二次确认对话框 → PUT /api/ai/weights
  │
  ├── auth.rs: Session 验证 + 角色检查 (Operator/AiExpert)
  ├── routes/ai/weights.rs: 校验权重值范围 0.0-5.0
  ├── audit/storage.rs: 写入审计日志 (操作前快照)
  ├── strategy-engine AiIntegrator::apply_weight_changes()
  │     └── ai-engine ModelManager::set_weights()
  ├── 持久化到 /etc/mupc/weights.toml
  │
  └── 响应 → Web UI 显示"权重已更新"
       └── SSE 推送最新 status (含权重值)
```

---

## 2. 系统管理设计

### 2.1 系统状态监控

#### 2.1.1 功能描述

提供 MUPC 装置整体运行状态的实时监控看板，作为 Web UI 的默认首页（路由 `/`）。运维人员登录后默认进入该页面。

#### 2.1.2 显示数据

| 数据项 | 说明 | 更新频率 |
|--------|------|----------|
| 固件版本号 | 当前运行固件版本 | 页面加载时 |
| 编译时间 | 固件编译时间戳 | 页面加载时 |
| 运行时间（uptime） | 系统持续运行时长 | 5 秒 |
| CPU 温度 | RK3588 处理器温度 | 5 秒 |
| 内存使用率 | 系统内存占用百分比 | 5 秒 |
| IEC 104 连接状态 | 已连接/连接中/断开/未配置 | 5 秒 |
| intercore 连接状态 | 实时控制模块连接状态 | 5 秒 |
| AI 引擎状态 | 就绪/加载中/异常/未启用 | 5 秒 |
| 策略模式 | 当前运行策略模式 | 5 秒 |
| 最近告警 | 最近 10 条告警记录 | WebSocket 实时推送 |

#### 2.1.3 状态指示灯

| 状态 | 颜色 | 样式 |
|------|------|------|
| 已连接/正常 | `#28A745` | 实心圆 + 颜色表示状态 |
| 连接中/等待 | `#FFC107` | 闪烁动画（1s 间隔） |
| 断开/错误 | `#DC3545` | 实心圆 |
| 未配置 | `#5F6368` | 空心圆 |

#### 2.1.4 验收标准 **[REVIEWED: PASS]**

- [ ] 状态监控页面作为默认首页（路由 `/`），页面自动刷新周期 5 秒
- [ ] 状态卡片以网格布局展示，每个卡片包含图标、标签和动态数值
- [ ] IEC 104 和 intercore 连接状态使用状态指示灯直观展示
- [ ] 告警列表最多展示 10 条最新记录，按时间倒序排列
- [ ] 页面底部状态栏展示系统状态、各模块连接状态和版本信息
- [ ] WebSocket 实时推送连接断开时，状态栏显示红色背景提示

### 2.2 配置管理

#### 2.2.1 功能描述

提供 MUPC 装置运行参数的 Web 配置界面（路由 `/config`），配置保存后自动生效，无需重启装置。

#### 2.2.2 可配置项

| 配置分组 | 参数 | 默认值 | 范围/选项 |
|----------|------|--------|-----------|
| IEC 104 连接参数 | 对端 IP 地址 | — | 合法 IPv4 地址 |
| IEC 104 连接参数 | 端口 | 2404 | 1-65535 |
| IEC 104 连接参数 | 心跳间隔 | 10 秒 | 1 秒 ~ 60 秒 |
| intercore 通信参数 | 本地端口 | 2500 | 1-65535 |
| intercore 通信参数 | 对端端口 | 2501 | 1-65535 |
| 遥测与日志 | 遥测上报周期 | 1 秒 | ≥ 1 秒 |
| 遥测与日志 | 日志级别 | INFO | ERROR / WARN / INFO / DEBUG |

#### 2.2.3 交互规范

- 配置按区域分区展示：IEC 104 连接参数、intercore 通信参数、遥测与日志
- 输入框焦点丢失时实时校验格式，提交保存时全面验证
- 验证失败时，输入框下方显示红色错误文字说明原因
- 保存操作需二次确认对话框确认
- 保存成功后显示绿色 Toast 提示（3 秒自动消失）
- 保存过程中按钮显示 loading 状态（文字变为"保存中..."）
- 配置自动生效，无需重启装置

#### 2.2.4 验收标准 **[REVIEWED: PASS]**

- [ ] 所有配置项通过 Web UI 可读、可写
- [ ] 心跳间隔配置范围限制 1~60 秒，越界时显示具体错误提示
- [ ] 配置保存后自动生效，无需重启
- [ ] 配置修改操作需要二次确认对话框
- [ ] 保存成功后显示成功提示，失败时显示错误提示
- [ ] 权限不足的用户无法看到保存按钮，仅看到只读的当前值

### 2.3 日志管理

#### 2.3.1 功能描述

提供系统日志的 Web 查看、搜索、筛选和导出功能（路由 `/logs`），支持 WebSocket 实时日志推送。

#### 2.3.2 功能要求

| 功能 | 说明 |
|------|------|
| 实时日志查看 | WebSocket 推送，支持 ERROR / WARN / INFO / DEBUG 级别过滤 |
| 时间范围筛选 | 按起始时间和结束时间筛选日志 |
| 关键字搜索 | 支持按关键字搜索日志消息内容 |
| 日志导出 | 下载为 `.log` 格式文件，单次导出最大 10000 条 |
| 分页展示 | 简洁数字分页，每页显示固定条数 |

#### 2.3.3 WebSocket 实时推送规范

- WebSocket 端点：`/ws/logs`
- 连接状态显示在日志页面顶部
- 连接成功：绿色文字"实时日志已连接"
- 连接断开：红色文字"实时日志已断开，正在重连..."
- 日志自动滚动到最新（除非用户手动滚动到上方查看历史）
- 手动滚动到上方时停止自动滚动，显示"滚动到最新"按钮
- 断开自动重连

#### 2.3.4 验收标准 **[REVIEWED: PASS]**

- [ ] 实时日志通过 WebSocket 推送，延迟 ≤ 2 秒
- [ ] 日志级别筛选支持多选，过滤结果实时更新
- [ ] 关键字搜索结果高亮显示匹配内容
- [ ] 日志导出为 `.log` 格式，单次导出不超过 10000 条
- [ ] 表格使用斑马纹（奇偶行不同背景色），行高 40px
- [ ] WebSocket 断开后自动重连

---

## 3. AI 决策可视化设计

### 3.1 总体设计原则

AI 决策可视化采用 **"SSE 实时推送 + 轮询兜底"** 模式。前端优先通过 SSE 接收实时数据，SSE 连接失败时自动切换为降级轮询。

| 数据项 | 刷新频率 | 推送方式 | 降级方式 |
|--------|----------|----------|----------|
| 当前系统状态 | 1 秒 | SSE event: decision | 1 秒轮询 |
| 预测曲线数据 | 60 秒 | SSE event: predictions | 60 秒轮询 |
| AI 决策动作 | 每决策周期 | SSE event: decision | 轮询 |
| 实时奖励值 | 每决策周期 | SSE event: rewards | 随决策轮询 |
| 奖励趋势图 | 5 分钟 | 聚合数据，SSE event: rewards | 轮询 |
| 微调进度 | 5 秒 | SSE event: finetuning | 5 秒轮询 |
| AI 引擎状态 | 5 秒 | SSE event: status | 5 秒轮询 |

### 3.2 SSE 实时推送服务

#### 3.2.1 方案选择

| 维度 | SSE | WebSocket | 轮询 |
|------|-----|-----------|------|
| 通信方向 | 单向（服务端→客户端） | 双向 | 请求→响应 |
| 连接开销 | 1 个 HTTP 连接 | 1 个 HTTP 升级连接 | 每次请求新连接 |
| 自动重连 | 浏览器原生支持 | 需自行实现 | 需自行实现 |
| Axum 支持 | `axum::response::sse::Sse` | `axum::extract::ws::WebSocket` | 原生 HTTP |
| 适用场景 | 服务端频率推送（最适合） | 双向交互 | 低频查询 |
| 资源占用 | 低（keep-alive） | 中（需管理状态） | 高（频繁 TCP 握手） |

**结论：采用 SSE**。理由：
- 本需求的所有 AI 推送都是服务端→浏览器单向
- SSE 在 Axum 0.7 中原生支持 `axum::response::sse::Sse`
- 浏览器自动重连，减少前端复杂度
- 比 WebSocket 更轻量，无需处理复杂协议
- 系统日志推送（`/ws/logs`）仍保留 WebSocket，两者各司其职

#### 3.2.2 SSE 端点设计

```
GET /api/ai/stream
```

**连接参数（Query）：**
```
session_id: string (必填)    // 用于认证
types: string (可选)         // 订阅类型，逗号分隔：status,decision,predictions,rewards,finetuning
                             // 默认全部订阅
```

**事件格式：**
```
event: decision
data: { "timestamp": "...", "action": {...}, "system_state": {...} }

event: predictions
data: { "timestamp": "...", "pv": {...}, "load": {...} }

event: rewards
data: { "total": 22.5, "components": [...], "timestamp": "..." }

event: status
data: { "engine_status": "ready", "running_mode": {...} }

event: finetuning
data: { "state": "collecting", "buffer_size": 128, ... }

event: heartbeat
data: { "time": "2026-05-29T10:00:00Z" }
```

#### 3.2.3 推送频率

| 事件类型 | 推送频率 | 触发方式 |
|---------|---------|---------|
| `status` | 每 5 秒 | 定时器 |
| `decision` | 每决策周期（约 1-5 秒） | AI 引擎决策完成事件 |
| `predictions` | 每 60 秒 | 定时器 |
| `rewards_current` | 每决策周期 | 随决策事件 |
| `finetuning` | 每 5 秒（微调中）/ 状态变化时 | 定时器 + 事件 |
| `heartbeat` | 每 30 秒 | 定时器 |

#### 3.2.4 SSE 后台架构

```
web-api 进程内：
┌──────────────────────────────────────────────┐
│  SsePushService                              │
│                                              │
│  tokio::sync::broadcast::Sender<SseEvent>    │
│       │                                      │
│       ├── 定时任务1: 每5秒推送 status          │
│       ├── 定时任务2: 每60秒推送 predictions     │
│       ├── 事件通道: 从 AiIntegrator 接收       │
│       │   decision/rewards/finetuning 事件    │
│       │                                      │
│       └── 多个 SSE 消费者 (每个 Web UI 标签页) │
│           └── broadcast::Receiver             │
│               └── Sse::new(receiver)          │
└──────────────────────────────────────────────┘
```

```rust
// 伪代码示意
pub struct SsePushService {
    tx: broadcast::Sender<SseEvent>,
    ai_integrator: Arc<AiIntegrator>,
}

impl SsePushService {
    /// 启动后台推送任务
    pub async fn start(&self) {
        let tx = self.tx.clone();
        let integrator = self.ai_integrator.clone();

        // 定时推送 status (每5秒)
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                let status = integrator.status().await;
                let _ = tx.send(SseEvent::Status(status));
            }
        });

        // 定时推送 predictions (每60秒)
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                let pred = integrator.get_prediction().await;
                let _ = tx.send(SseEvent::Predictions(pred));
            }
        });
    }
}
```

### 3.3 预测曲线展示（路由 `/ai/predictions`） **[DESIGN_APPROVED]**

#### 3.3.1 功能描述

展示 LSTM 模型输出的未来 15-30 分钟预测曲线，包括光伏出力预测和负荷预测两个独立视图。

#### 3.3.2 显示内容

| 数据项 | 说明 | 单位 | 更新频率 |
|--------|------|------|----------|
| 光伏出力预测曲线 | 未来 N 个时间步的光伏出力预测值，叠加实际历史曲线 | kW | 每 60 秒 |
| 负荷预测曲线 | 未来 N 个时间步的负荷预测值，叠加实际历史曲线 | kW | 每 60 秒 |
| 置信度区间 | 每条预测曲线的置信度范围（阴影区域表示） | — | 随预测数据同时更新 |
| 预测时间范围 | 显示预测起始时间和终止时间 | — | 随预测数据同时更新 |

#### 3.3.3 API 响应结构

```json
{
  "timestamp": "2026-05-29T10:00:00Z",
  "predictions": {
    "pv": {
      "history": [
        {"time": "2026-05-29T09:00:00Z", "value": 12.5},
        {"time": "2026-05-29T09:01:00Z", "value": 13.2}
      ],
      "forecast": [
        {"time": "2026-05-29T10:00:00Z", "value": 15.1, "confidence_lower": 14.2, "confidence_upper": 16.0}
      ],
      "confidence": 0.873,
      "horizon_secs": 1800
    },
    "load": {
      "history": [],
      "forecast": [],
      "confidence": 0.912,
      "horizon_secs": 1800
    }
  },
  "model_loaded": true
}
```

#### 3.3.4 验收标准 **[REVIEWED: PASS]**

- [ ] 光伏出力预测曲线和负荷预测曲线分别在独立图表区域展示
- [ ] 预测曲线叠加在过去 60 分钟的实际数据曲线上，形成时间轴连续的视图
- [ ] 置信度以阴影区域形式展示在预测曲线两侧
- [ ] 预测数据每 60 秒自动刷新一次
- [ ] 页面加载时显示当前最新一次的预测结果，无需等待完整周期
- [ ] 预测时间轴标注清晰的最小刻度单位（1 分钟）
- [ ] 鼠标悬停在曲线上时显示该时刻的具体数值
- [ ] 当 LSTM 模型未加载时，页面显示"预测模型未加载"提示

### 3.4 AI 决策逻辑展示（路由 `/ai/decision`） **[DESIGN_APPROVED]**

#### 3.4.1 功能描述

展示 AI 引擎当前的决策状态、决策动作和决策依据，帮助运维人员理解 AI "为什么做出这个决策"。

#### 3.4.2 显示内容

| 区域 | 内容 | 更新频率 |
|------|------|----------|
| 当前系统状态 | SOC、光伏功率、负荷功率、电网功率、变压器负载 | 每 1 秒 |
| 决策动作 | 电池功率设定、负荷切除量、PV 限功率比例、置信度 | 每决策周期 |
| 决策依据 | 当前场景模式 + 各优化目标的实时贡献值 | 每决策周期 |

#### 3.4.3 决策依据展示规则

- 展示当前场景识别结果（如"农网灌溉模式"、"工商业-自主套利模式"等）
- 列出当前场景下的奖励函数细分项及其实时值
- 每个细分项以进度条或数值卡片形式展示其在总奖励中的占比
- 置信度值以百分比形式展示（如 87.3%），并附带色标指示

#### 3.4.4 置信度色标

| 区间 | 颜色 |
|------|------|
| ≥ 80% | 绿色 |
| 60% ~ 80% | 黄色 |
| < 60% | 红色 |

#### 3.4.5 验收标准 **[REVIEWED: PASS]**

- [ ] 当前系统状态数据每 1 秒刷新一次（与遥测频率一致）
- [ ] 决策动作数据随 AI 引擎决策周期同步更新
- [ ] 决策依据区域明确显示当前场景模式名称
- [ ] 奖励函数各细分项的数值实时更新，显示精确到小数点后 2 位
- [ ] 置信度值以百分比展示，附带色标指示
- [ ] 当 AI 引擎未启用时，页面显示"AI 引擎当前未启用，系统运行于本地策略模式"
- [ ] 当 AI 引擎推理异常时，页面显示错误信息及最后一次成功决策的时间戳

### 3.5 实时奖励值展示与历史趋势 **[DESIGN_APPROVED]**

#### 3.5.1 功能描述

在 AI 决策面板中展示实时奖励值及其历史趋势，评估模型收敛状态和决策质量变化。

#### 3.5.2 显示内容

| 数据项 | 说明 | 更新频率 |
|--------|------|----------|
| 当前总奖励值 | 最近一次 RL 决策的总奖励数值 | 每决策周期 |
| 各子奖励值 | 光伏消纳、电压治理、电池损耗等细分项 | 每决策周期 |
| 奖励趋势图 | 过去 24 小时的总奖励时序曲线 | 每 5 分钟聚合 |
| 子奖励趋势图 | 过去 24 小时各子奖励项的时序曲线 | 每 5 分钟聚合 |

#### 3.5.3 数据存储

- 奖励值历史记录存储在 SQLite 数据库（`/var/log/mupc/data.db`）
- 保留最近 30 天的奖励值历史
- 采样粒度：近 1 小时每决策周期记录一次，超过 1 小时每 5 分钟聚合为平均值
- 奖励趋势图时间轴跨度支持 1 小时 / 6 小时 / 24 小时 / 7 天切换

#### 3.5.4 验收标准 **[REVIEWED: PASS]**

- [ ] 当前总奖励值以醒目数值卡片展示，颜色依据正负区分（正值绿色、负值红色）
- [ ] 各子奖励值以列表形式展示，按绝对值从大到小排序
- [ ] 奖励趋势图时间轴跨度支持 1h / 6h / 24h / 7d 切换
- [ ] 趋势图中标注奖励值的最大值、最小值和平均值
- [ ] 历史数据查询响应时间 < 1 秒（查询范围 7 天内）
- [ ] 当历史数据不足时，趋势图显示"数据收集中"提示

### 3.6 运行模式状态展示 **[DESIGN_APPROVED]**

#### 3.6.1 功能描述

在 AI 决策面板顶部显示当前预设运行模式和切换来源。模式由调度主站远程指令或本地策略管理员选择确定（v2.0 预设互斥场景，无自动识别）。

#### 3.6.2 显示内容

| 数据项 | 说明 | 更新频率 |
|--------|------|----------|
| 当前运行模式 | 5 种预设模式之一 | 每决策周期 |
| 模式来源 | 远程调度（IEC 104/61850）/ 本地Web / 配置文件 | 模式变更时 |
| 切换标记 | 远程来源显示"远程"，本地来源显示"本地" | 模式变更时 |
| 生效时间 | 当前模式生效的时间戳 | 模式变更时 |

#### 3.6.3 验收标准 **[REVIEWED: PASS]**

- [ ] 当前运行模式名称在面板顶部居中展示，字号大于其他内容
- [ ] 远程切换的模式带有"远程"标签，本地切换的模式带有"本地"标签
- [ ] 模式切换事件记录时间戳和来源，可供审计查询
- [ ] AI 引擎未启用时显示"AI 引擎未启用，系统运行于本地策略模式"

### 3.7 模型在线微调监控（路由 `/ai/finetuning`） **[DESIGN_APPROVED]**

#### 3.7.1 功能描述

展示 OnlineUpdater 模块的微调状态，包括数据收集进度、微调执行进展、模型效果变化。

#### 3.7.2 显示内容

| 数据项 | 说明 | 更新频率 |
|--------|------|----------|
| 微调状态 | 空闲 / 数据收集中 / 微调中 / 微调完成 / 微调失败 | 实时（2 秒） |
| 数据缓冲区 | 当前缓冲区数据量 / 触发微调阈值（batch_size） | 每次新增数据点 |
| 微调进度 | 当前批次完成百分比 | 微调中每 5 秒 |
| 微调轮次 | 已完成的微调迭代次数 | 每次微调完成 |
| 效果变化 | 微调前后模型在验证集上的指标变化 | 每次微调完成后 |
| 最后微调时间 | 上一次微调完成的时间戳 | 每次微调完成后 |

#### 3.7.3 验收标准 **[REVIEWED: PASS]**

- [ ] 微调状态变化时 2 秒内反映到页面
- [ ] 数据缓冲区以进度条形式展示（已收集 / 目标阈值）
- [ ] 微调进行中显示进度百分比和预计剩余时间
- [ ] 微调完成后展示效果变化（如损失值变化曲线、验证指标对比）
- [ ] 微调失败时显示失败原因和错误详情
- [ ] 效果变化数据至少保留最近 10 次微调记录
- [ ] 当在线微调未启用时，页面显示"在线微调未启用"提示

---

## 4. 专家干预设计

### 4.1 设计原则

1. **web-api 作为安全边界**：所有权限校验、输入验证、审计日志记录在 web-api 层完成，后端模块不重复校验。
2. **干预操作要求一致性和审计**：所有写操作记录完整审计日志，可追溯。
3. **分级确认机制**：根据操作危险程度，分级要求二次确认或三级确认 + 密码验证。

### 4.2 手动调整优化目标权重（路由 `/ai/intervention`）

#### 4.2.1 功能描述

提供滑块/数值输入方式修改各优化目标的权重参数，权重调整实时生效。web-api 将调整请求转发至 strategy-engine 的 AiIntegrator。

#### 4.2.2 可调整权重参数

| 权重参数 | 适用模式 | 默认值 | 调整范围 |
|----------|----------|--------|----------|
| 光伏消纳权重 | 全部模式 | 1.0 | 0.0 ~ 5.0 |
| 电压治理权重 | 农网模式 | 1.0 | 0.0 ~ 5.0 |
| 电池损耗权重 | 全部模式 | 1.0 | 0.0 ~ 5.0 |
| 变压器过载权重 | 全部模式 | 1.0 | 0.0 ~ 5.0 |
| 电价差收益权重 | 工商业-自主套利 | 1.0 | 0.0 ~ 5.0 |
| 需量罚金减免权重 | 工商业-需量控制 | 1.0 | 0.0 ~ 5.0 |
| 绿电消纳权重 | 工商业-极致绿色 | 1.0 | 0.0 ~ 5.0 |

#### 4.2.3 交互规范

- 每个权重参数提供滑块控件（连续滑动）和数值输入框（手动输入），双向绑定
- 权重参数列表根据当前运行模式动态展示（不相关参数隐藏或置灰禁用）
- 修改未应用时，参数显示"已修改未应用"状态（黄色感叹号标记）
- 应用操作需要二次确认对话框确认
- 确认后 2 秒内权重参数生效
- 权重值在设备重启后保持最后一次手动设置的值（持久化到 TOML 配置文件）

#### 4.2.4 处理流程

```
Web UI → 二次确认对话框 → PUT /api/ai/weights
  ├── auth.rs: Session 验证 + 角色检查 (Operator/AiExpert)
  ├── routes/ai/weights.rs: 校验权重值范围 0.0-5.0，校验名称合法性
  ├── audit/storage.rs: 写入审计日志（操作前快照）
  ├── strategy-engine AiIntegrator::apply_weight_changes()
  │     └── ai-engine ModelManager::set_weights()
  ├── 持久化到 /etc/mupc/weights.toml（原子写入）
  └── 响应 → Web UI 显示"权重已更新"
       └── SSE 推送最新 status（含权重值）
```

#### 4.2.5 验收标准 **[REVIEWED: PASS]**

- [ ] 权重参数列表根据当前运行模式动态展示，仅显示该模式下有效的参数
- [ ] 每个权重参数均提供滑块（连续）和数值输入框（支持小数输入）两种操作方式
- [ ] 滑块两端标注参数名称和当前数值
- [ ] 数值输入框校验输入范围（0.0 ~ 5.0），越界时显示红色错误提示
- [ ] 修改未应用时显示黄色感叹号标记
- [ ] 应用操作需要二次确认，确认后 2 秒内生效
- [ ] 操作记录写入审计日志：操作人、时间、调整前值、调整后值
- [ ] 权限不足的用户（调度人员）仅看到只读的当前值，无操作控件
- [ ] 权重值在设备重启后保持最后一次手动设置的值

### 4.3 强制切换运行模式（本地选择） **[DESIGN_APPROVED]**

#### 4.3.1 功能描述

策略管理员通过 Web UI 在 5 种预设运行场景中手动选择当前运行模式。API 端点：`PUT /api/v1/mode`。同一时刻仅 1 种模式生效（互斥），调度主站远程切换优先级高于本地选择。

#### 4.3.2 可切换的运行模式

| 模式名称 | 对应枚举 | 适用场景 |
|----------|---------|----------|
| 农网灌溉模式 | `AgriculturalIrrigation` | 农网台区灌溉季节 |
| 自主套利模式 | `CommercialArbitrage` | 工商业储能峰谷套利 |
| 需量控制模式 | `DemandControl` | 变压器容量受限场景 |
| 虚拟电厂模式 | `VirtualPowerPlant` | 参与 VPP 聚合调度 |
| 极致绿色模式 | `UltraGreen` | 绿色认证要求高的场景 |

> **v2.0 说明：** 无"本地兜底模式"。AI 引擎异常时系统自动降级至本地策略（由 strategy-engine 内部处理）。无"恢复自动识别"功能（不存在自动识别）。

#### 4.3.3 交互规范

- 模式切换下拉列表展示 5 种预设场景（不包含当前模式自身）
- 切换操作需要二次确认对话框确认
- 若同时收到远程调度指令，远程指令优先，本地操作被拒绝并提示"远程调度优先"
- 手动切换的模式在设备重启后保持（持久化到 `/var/lib/mupc/current_mode`）

#### 4.3.4 验收标准 **[REVIEWED: PASS]**

- [ ] 模式切换下拉列表展示 5 种预设场景选项（不含当前模式）
- [ ] 切换操作需要二次确认，确认后 1s 内新模式生效
- [ ] 切换后当前运行模式显示"本地"来源标签
- [ ] 远程指令冲突时显示"远程调度优先"提示
- [ ] 操作记录写入审计日志：操作人、原模式、新模式、来源
- [ ] 权限不足的用户仅看到只读的当前模式信息
- [ ] 手动切换的模式在设备重启后保持

### 4.4 三级确认机制

**实现方式**：二次确认和三级确认在 Web UI 前端实现对话框，后端 API 不做"预确认"逻辑。后端设计为幂等操作，前端先展示确认对话框，用户确认后再发送实际 API 请求。

**二级确认（常规敏感操作）**：
- 应用场景：配置修改、权重调整、模式切换、A/B 测试
- 弹窗标题：「确认操作」
- 弹窗内容：「此操作将修改系统配置，是否继续？」
- 按钮：「取消」（次按钮）、「确认」（主按钮）

**三级确认（高危操作）**：
- 应用场景：执行模型回滚
- 步骤：
  1. 第一层：操作目的确认（"确认要执行此操作？"）
  2. 第二层：操作影响确认（"回滚将替换当前在线模型，决策可能暂时中断"）
  3. 第三层：输入操作人密码进行二次身份验证

### 4.5 审计日志设计

#### 4.5.1 审计记录字段

| 字段 | 类型 | 说明 |
|------|------|------|
| 操作 ID | UUID | 全局唯一标识 |
| 操作时间 | DateTime | 精确到毫秒的 UTC 时间戳 |
| 操作人 | String | 执行操作的用户名 |
| 操作类型 | Enum | weight_adjust / mode_switch / ab_test_start / ab_test_stop / model_rollback |
| 操作详情 | JSON | 操作前后的完整参数快照 |
| 操作结果 | Enum | success / failed |
| 失败原因 | String? | 操作失败时的错误描述 |
| 来源 IP | String | 发起操作的客户端 IP 地址 |

#### 4.5.2 SQLite 存储

**数据库文件**：`/var/log/mupc/audit.db`（独立数据库文件，与 data-processing 的 data.db 分开）

**表结构**：
```sql
CREATE TABLE IF NOT EXISTS audit_log (
    id TEXT PRIMARY KEY,                   -- UUID
    timestamp TEXT NOT NULL,               -- ISO 8601
    operator TEXT NOT NULL,
    action_type TEXT NOT NULL,             -- weight_adjust | mode_switch | ab_test_start | ab_test_stop | model_rollback
    action_detail TEXT NOT NULL,           -- JSON
    result TEXT NOT NULL,                  -- success | failed
    fail_reason TEXT,
    source_ip TEXT NOT NULL
);

CREATE INDEX idx_audit_timestamp ON audit_log(timestamp);
CREATE INDEX idx_audit_operator ON audit_log(operator);
CREATE INDEX idx_audit_action_type ON audit_log(action_type);
```

**安全约束**：
- 数据库文件权限：`0600`（仅 mupc 进程可读写）
- 应用层只提供 `INSERT` 和 `SELECT`，不提供 `UPDATE` 和 `DELETE`
- 启用 SQLite WAL 模式以支持并发读
- 保留期限：不少于 365 天

#### 4.5.3 AuditLogger 接口

```rust
/// 审计日志记录器
pub struct AuditLogger {
    db: rusqlite::Connection,
}

impl AuditLogger {
    /// 记录操作
    pub async fn log(&self, entry: AuditEntry) -> Result<(), AuditError>;

    /// 查询（支持分页筛选）
    pub async fn query(&self, filter: AuditFilter) -> Result<AuditPage, AuditError>;

    /// 导出 CSV
    pub async fn export_csv(&self, filter: AuditFilter) -> Result<String, AuditError>;
}
```

#### 4.5.4 验收标准 **[REVIEWED: PASS]**

- [ ] 所有权重调整、模式切换、A/B 测试、模型回滚操作均记录审计日志
- [ ] 日志记录包含完整的前后状态快照
- [ ] 日志查询支持按日期范围、操作人、操作类型筛选
- [ ] 查询结果分页展示，每页 20 条
- [ ] 支持日志导出为 CSV 格式，单次导出不超过 10000 条
- [ ] 审计日志不可删除、不可修改（仅追加写入）
- [ ] 查询响应时间 < 2 秒（查询范围 365 天内）
- [ ] 缺少权限的用户无法访问审计页面
- [ ] 审计日志保留 365 天

### 4.6 权重持久化

```toml
# /etc/mupc/weights.toml
# 由 web-api 在权重修改时原子写入
[default]
pv_consumption = 1.0
voltage_regulation = 1.0
battery_degradation = 1.0
transformer_overload = 1.0
price_arbitrage = 1.0
demand_penalty = 1.0
green_energy_ratio = 1.0

[overrides]
pv_consumption = 1.5
battery_degradation = 2.0
# 未设置的为 null，使用 default 值
```

### 4.7 模式持久化

v2.0 使用单字节文件存储当前模式（简化为 `RunningMode` 枚举值 1~5）：

```rust
// ModeSelector 内部处理持久化，web-api 不直接读写
// 持久化文件: /var/lib/mupc/current_mode
// 内容: "1"~"5" (RunningMode 枚举值)
// 文件损坏时回退至 AgriculturalIrrigation (MODE-01)
```

**与 v1.1 的区别：**
- v1.1: `/etc/mupc/mode.toml` 含 `mode_source` 和 `manual_mode` 字段
- v2.0: `/var/lib/mupc/current_mode` 单字节文件，由 ModeSelector 管理
- 配置中的 `[mode] default_mode` 仅用于系统首次启动

---

## 5. A/B 测试设计

### 5.1 模型版本管理

#### 5.1.1 数据模型

```json
{
  "model_id": "lstm_v1.2.0",
  "model_type": "lstm",
  "version": "1.2.0",
  "description": "使用 2026Q1 数据训练的光伏预测模型",
  "file_path": "/models/current/lstm/model.rknn",
  "file_size": 4587520,
  "md5": "a1b2c3d4e5f6...",
  "status": "active",
  "deployed_at": "2026-05-28T10:00:00Z",
  "metrics": { "mae": 0.15, "rmse": 0.22, "mape": 8.5 }
}
```

#### 5.1.2 模型状态枚举

| 状态 | 说明 |
|------|------|
| active | 当前在线运行的模型版本（每个模型类型仅一个） |
| standby | 已加载但未激活，可作为 A/B 测试实验组 |
| archived | 已归档的历史版本，不可直接切换但可查询 |
| failed | 加载失败的版本（保留记录供分析） |

#### 5.1.3 验收标准 **[REVIEWED: PASS]**

- [ ] 每个模型类型（lstm / maddpg / ppo）最多同时保留 5 个版本（active + standby）
- [ ] 版本信息持久化存储在 `/etc/mupc/models/manifest.json`
- [ ] 新增版本时自动检查可用磁盘空间，空间不足 200MB 时拒绝添加
- [ ] 版本查询接口响应时间 < 100ms
- [ ] 同一模型类型同一时刻只能有一个 active 版本
- [ ] 归档版本保留至少 30 天后方可物理删除

### 5.2 确定性 Hash 路由

基于 `device_id` 进行一致性哈希分流，确保同一设备在测试期间始终分配到同一模型版本。

```rust
/// A/B 测试流量分配器
pub struct AbTestRouter {
    /// 运行中的 A/B 测试表
    active_tests: Arc<RwLock<HashMap<ModelType, ActiveAbTest>>>,
}

impl AbTestRouter {
    /// 确定设备应路由到哪个模型版本
    pub fn route_device(
        &self,
        device_id: &str,
        model_type: ModelType,
    ) -> RoutingDecision {
        let tests = self.active_tests.read().unwrap();
        match tests.get(&model_type) {
            None => RoutingDecision::UseControl(model_type.default_version()),
            Some(test) => {
                // 确定性 hash: device_id 的 hash 值取模 100
                let hash = self::hash_device_id(device_id);
                if hash < test.traffic_percent {
                    RoutingDecision::UseExperiment(test.experiment_version.clone())
                } else {
                    RoutingDecision::UseControl(test.control_version.clone())
                }
            }
        }
    }
}

/// 基于 device_id 的确定性 hash
/// 使用 DJB2 算法保证分布均匀
fn hash_device_id(device_id: &str) -> u8 {
    let mut hash: u32 = 5381;
    for b in device_id.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u32);
    }
    (hash % 100) as u8  // 0-99
}
```

### 5.3 A/B 测试流程

#### 5.3.1 配置参数

| 参数 | 类型 | 说明 |
|------|------|------|
| 模型类型 | enum | lstm / maddpg / ppo |
| 对照组版本 | string | 当前 active 版本（自动填充，不可修改） |
| 实验组版本 | string | 从 standby 版本中选择 |
| 流量分配比例 | integer | 实验组流量百分比（1-50） |
| 测试时长 | integer | 预计运行小时数（1-168） |
| 评估指标 | array | 用于效果对比的指标列表 |

#### 5.3.2 流量分配生命周期

```
启动测试:
  POST /api/ai/abtest
    → AiIntegrator::create_ab_test(config)
    → 校验 config（实验组必须为 standby、流量比例 1-50、无并行测试）
    → 设置 AbTestRouter.active_tests[model_type] = test_config
    → 5 秒内所有新决策请求按 hash 路由
    → 返回 test_id

运行中:
  每个 AiIntegrator::decide() 请求
    → AbTestRouter::route_device(device_id, model_type)
    → 返回 UseControl / UseExperiment
    → ModelManager 执行对应模型推理
    → 返回 ActionOutput

停止测试:
  DELETE /api/ai/abtest/{id}
    → 从 AbTestRouter 移除 test_config
    → 5 秒内所有流量恢复至对照组
    → 生成最终报告

超时停止:
  后台 tokio 定时任务检查 duration_hours
    → 到期自动停止
    → 生成最终报告

异常停止:
  实验组模型连续推理失败 → 自动暂停，流量切回对照组
  对照组模型异常 → 自动停止，AI 引擎降级至本地策略模式
```

#### 5.3.3 路由集成到 AiIntegrator

```
AiIntegrator::decide()
  ↓
  AbTestRouter::route_device(device_id, model_type)
  ↓
  ┌─ UseControl → ModelManager (当前 active 模型)
  └─ UseExperiment → ModelManager (实验组 standby 模型)
  ↓
  返回 ActionOutput
```

### 5.4 效果对比指标展示

#### 5.4.1 对比指标

| 指标 | 计算方式 |
|------|----------|
| 平均奖励值 | 实验组和对照组分别统计 |
| 光伏消纳率 | 实验组和对照组分别统计 |
| 电压合格率 | 实验组和对照组分别统计 |
| 变压器负载率峰值 | 实验组和对照组分别统计 |
| 电池循环次数 | 实验组和对照组分别统计 |
| 平均决策置信度 | 实验组和对照组分别统计 |

#### 5.4.2 展示方式

- 每个指标以并排双柱图展示（对照组蓝色、实验组绿色）
- 指标差异以百分比变化标注（如 "+5.2%" 或 "-3.1%"）
- 统计显著性标记（p-value < 0.05 时标注"显著"）
- 支持导出对比报告为 JSON 格式

### 5.5 模型安全回滚

#### 5.5.1 回滚操作流程

```
1. AI 运维专家选择目标回滚版本
2. 点击"回滚到该版本"按钮
3. 三级确认：确认回滚 → 确认版本 → 输入密码
4. 系统执行回滚：
   a. 备份当前 active 模型到 rollback 目录
   b. 从目标版本目录复制模型到 current 目录
   c. 通知 AiIntegrator 重新加载模型
   d. 执行模型预热推理
   e. 更新版本记录和 manifest.json
5. 页面显示回滚进度和结果
6. 操作记录写入审计日志
```

#### 5.5.2 自动回滚触发条件

| 条件 | 阈值 |
|------|------|
| 模型加载失败 | 1 次 |
| 模型推理失败 | 连续 3 次 |
| 模型预热超时 | 30 秒 |

#### 5.5.3 验收标准 **[REVIEWED: PASS]**

- [ ] 手动回滚操作需要三级确认 + 密码二次身份验证
- [ ] 回滚执行时间 < 60 秒（含模型加载和预热）
- [ ] 回滚成功后模型恢复正常推理
- [ ] 回滚失败时自动触发恢复机制，恢复到回滚前的版本
- [ ] 连续自动回滚 3 次后触发安全模式（使用本地兜底策略）
- [ ] 回滚操作记录完整审计日志
- [ ] 权限不足的用户无法看到回滚按钮
- [ ] 回滚不影响正在运行的 A/B 测试

#### 5.5.4 A/B 测试验收标准 **[REVIEWED: PASS]**

- [ ] 实验组版本只能从 standby 状态中选择
- [ ] 流量分配比例支持 1% 步进调节，上限 50%
- [ ] 启动测试时校验实验组与对照组属于同一模型类型
- [ ] 同一时刻每个模型类型只能有一个运行中的 A/B 测试
- [ ] 流量分配基于确定性 hash，同一设备始终分配到同一组
- [ ] 测试启动后 5 秒内流量分配规则生效
- [ ] 可随时手动停止测试，停止后 5 秒内所有流量恢复至对照组
- [ ] 测试启动和停止操作均记录审计日志
- [ ] 对比指标以并排柱状图展示，5 分钟内显示初始数据
- [ ] 指标数据每分钟更新一次
- [ ] 对比报告支持导出为 JSON 格式

---

## 6. REST API 接口定义

### 6.1 认证与系统管理（基础端点）

| 方法 | 路由 | 功能 | 认证 | 角色权限 |
|------|------|------|------|---------|
| POST | `/api/auth/login` | 登录认证 | — | — |
| POST | `/api/auth/logout` | 退出登录 | Session | 全部 |
| PUT | `/api/auth/password` | 修改密码 | Session | 全部 |
| GET | `/api/status` | 获取系统状态 | Session | 全部 |
| GET | `/api/config` | 获取配置 | Session | 全部 |
| PUT | `/api/config` | 保存配置 | Session | 本地运维/系统管理员 |
| GET | `/api/logs` | 获取日志列表（分页、筛选） | Session | 全部 |
| GET | `/ws/logs` | WebSocket 实时日志推送 | Session | 全部 |

### 6.2 AI 可视化端点

| 方法 | 路由 | 功能 | 认证 | 角色权限 |
|------|------|------|------|---------|
| GET | `/api/ai/predictions` | 获取预测曲线数据 | Session | 全部角色 |
| GET | `/api/ai/decision` | 获取当前决策状态 | Session | 全部角色 |
| GET | `/api/ai/rewards` | 获取奖励值（含历史） | Session | 全部角色 |
| GET | `/api/ai/status` | 获取 AI 引擎状态和运行模式 | Session | 全部角色 |
| GET | `/api/ai/finetuning` | 获取在线微调状态 | Session | 专家 + 管理员 |
| GET | `/api/ai/stream` | SSE 实时推送连接 | Session | 全部角色 |
| GET | `/api/v1/mode` | 获取当前运行场景 | Session | 全部角色 |
| GET | `/api/v1/mode/list` | 获取所有可用场景列表 | Session | 全部角色 |

### 6.3 专家干预端点

| 方法 | 路由 | 功能 | 认证 | 角色权限 |
|------|------|------|------|---------|
| PUT | `/api/ai/weights` | 更新优化目标权重 | Session | 专家 + 管理员 |
| PUT | `/api/v1/mode` | 切换运行场景（v2.0 替代 /api/ai/mode） | Session | 专家 + 管理员 |
| GET | `/api/v1/mode` | 获取当前运行场景 | Session | 全部角色 |
| GET | `/api/v1/mode/list` | 获取所有可用场景列表 | Session | 全部角色 |
| GET | `/api/ai/audit` | 查询审计日志 | Session | 专家 + 管理员 |

### 6.4 A/B 测试与模型管理端点

| 方法 | 路由 | 功能 | 认证 | 角色权限 |
|------|------|------|------|---------|
| GET | `/api/ai/models` | 查询模型版本列表 | Session | AI 运维专家 |
| POST | `/api/ai/abtest` | 创建 A/B 测试 | Session | AI 运维专家 |
| GET | `/api/ai/abtest/{id}` | 查询 A/B 测试结果 | Session | AI 运维专家 |
| DELETE | `/api/ai/abtest/{id}` | 停止 A/B 测试 | Session | AI 运维专家 |
| POST | `/api/ai/rollback` | 执行模型回滚 | Session + 密码 | AI 运维专家 |

### 6.5 各接口详细定义

#### 6.5.1 POST /api/auth/login

**请求体：**
```json
{
  "username": "admin",
  "password": "****"
}
```

**成功响应 200：**
```json
{
  "status": "ok",
  "session_id": "uuid-string",
  "user": {
    "username": "admin",
    "role": "ai_expert"
  }
}
```

**错误响应 401：**
```json
{
  "error": "auth_failed",
  "message": "用户名或密码错误"
}
```

#### 6.5.2 POST /api/auth/logout

**请求头：** `X-Session-Id: <session-id>`

**成功响应 200：**
```json
{ "status": "ok" }
```

#### 6.5.3 PUT /api/auth/password

**请求体：**
```json
{
  "old_password": "****",
  "new_password": "newpass123"
}
```

**密码规则：** 8-20 位，包含字母和数字

#### 6.5.4 GET /api/status

**成功响应 200：**
```json
{
  "firmware_version": "v1.2.0",
  "build_time": "2026-05-20T10:00:00Z",
  "uptime_secs": 478800,
  "cpu_temperature": 45.2,
  "memory_usage": 62,
  "iec104_status": "connected",
  "intercore_status": "connected",
  "ai_engine_status": "ready",
  "strategy_mode": "commercial_arbitrage",
  "recent_alarms": [
    {"time": "2026-05-27T10:30:15Z", "level": "error", "message": "IEC 104 连接断开"}
  ]
}
```

#### 6.5.5 GET /api/config / PUT /api/config

配置接口，GET 返回当前配置，PUT 更新配置（需要操作权限）。

**PUT 请求体示例：**
```json
{
  "iec104": {
    "peer_ip": "192.168.1.10",
    "port": 2404,
    "heartbeat_interval_secs": 10
  },
  "intercore": {
    "local_port": 2500,
    "peer_port": 2501
  },
  "telemetry": {
    "report_interval_secs": 1,
    "log_level": "INFO"
  }
}
```

#### 6.5.6 GET /api/logs

**请求参数：**
```
start: Option<String>      // ISO 8601 开始时间
end: Option<String>        // ISO 8601 结束时间
level: Option<String>      // ERROR,WARN,INFO,DEBUG（多选逗号分隔）
keyword: Option<String>    // 关键字搜索
page: Option<u32>          // 页码，默认 1
page_size: Option<u32>     // 每页条数，默认 50
```

#### 6.5.7 GET /api/ai/predictions

**请求参数：**
```
type: Option<String>  // 可选过滤："pv" | "load"，不传则返回全部
```

**成功响应 200：**
```json
{
  "timestamp": "2026-05-29T10:00:00Z",
  "predictions": {
    "pv": {
      "history": [
        {"time": "2026-05-29T09:00:00Z", "value": 12.5},
        {"time": "2026-05-29T09:01:00Z", "value": 13.2}
      ],
      "forecast": [
        {"time": "2026-05-29T10:00:00Z", "value": 15.1, "confidence_lower": 14.2, "confidence_upper": 16.0}
      ],
      "confidence": 0.873,
      "horizon_secs": 1800
    },
    "load": {
      "history": [],
      "forecast": [],
      "confidence": 0.912,
      "horizon_secs": 1800
    }
  },
  "model_loaded": true
}
```

**数据源：** 调用 `AiIntegrator::get_prediction()` -> `ModelManager::predict()`

#### 6.5.8 GET /api/ai/decision

**成功响应 200：**
```json
{
  "timestamp": "2026-05-29T10:00:05Z",
  "system_state": {
    "battery_soc": 0.65,
    "pv_power_kw": 15.1,
    "load_power_kw": 8.2,
    "grid_power_kw": 3.5,
    "transformer_load_kw": 22.0
  },
  "action": {
    "p_batt_set_kw": 5.0,
    "load_shedding_kw": 0.0,
    "pv_limit_ratio": 1.0,
    "confidence": 0.873
  },
  "mode": {
    "current": "CommercialArbitrage",
    "display_name": "自主套利模式",
    "source": "LocalWeb",
    "switched_at": "2026-05-29T08:00:00Z"
  },
  "reward_breakdown": [
    {"name": "price_arbitrage", "value": 12.5, "weight": 1.0, "percentage": 45.2},
    {"name": "battery_degradation", "value": -3.2, "weight": 1.0, "percentage": -11.6}
  ],
  "ai_engine_enabled": true
}
```

**缓存策略：** web-api 层维护一个 `Arc<RwLock<Option<DecisionSnapshot>>>`，由后台 tokio 任务周期性（或通过 channel 事件驱动）更新。前端 1 秒轮询或 SSE 订阅时读取缓存。

#### 6.5.9 GET /api/ai/rewards

**请求参数：**
```
start: Option<String>    // ISO 8601 开始时间，默认 24 小时前
end: Option<String>      // ISO 8601 结束时间，默认当前时间
range: Option<String>    // 快捷范围："1h" | "6h" | "24h" | "7d"，优先级高于 start/end
```

**成功响应 200：**
```json
{
  "current": {
    "total": 22.5,
    "components": [
      {"name": "price_arbitrage", "value": 12.5, "weight": 1.0},
      {"name": "pv_consumption", "value": 8.1, "weight": 1.0},
      {"name": "battery_degradation", "value": -3.2, "weight": 1.0}
    ],
    "timestamp": "2026-05-29T10:00:05Z"
  },
  "history": [
    {"timestamp": "2026-05-29T09:00:00Z", "total_reward": 20.1, "components": []}
  ],
  "stats": {
    "max": 28.5,
    "min": 15.2,
    "avg": 21.3
  }
}
```

#### 6.5.10 GET /api/ai/status

**成功响应 200：**
```json
{
  "engine_status": "ready",
  "model_status": {
    "lstm": "ready",
    "rl": "ready"
  },
  "running_mode": {
    "current": "CommercialArbitrage",
    "display_name": "自主套利模式",
    "source": "RemoteDispatch",
    "switched_at": "2026-05-29T08:00:00Z"
  },
  "uptime_secs": 36000,
  "ai_engine_enabled": true,
  "fallback_active": false
}
```

#### 6.5.11 GET /api/ai/finetuning

**成功响应 200：**
```json
{
  "enabled": true,
  "state": "collecting",
  "buffer_size": 128,
  "batch_size": 32,
  "buffer_progress": 1.0,
  "progress_percent": null,
  "total_epochs": 5,
  "completed_epochs": 0,
  "last_update": "2026-05-29T09:55:00Z",
  "last_metrics": null,
  "recent_history": [
    {"completed_at": "2026-05-28T22:00:00Z", "loss_before": 0.035, "loss_after": 0.028, "improvement": 0.007}
  ]
}
```

#### 6.5.12 PUT /api/ai/weights

**请求体：**
```json
{
  "weights": [
    {"name": "pv_consumption", "value": 1.5},
    {"name": "battery_degradation", "value": 2.0}
  ]
}
```

**命名约束：** `name` 可选值：`pv_consumption`, `voltage_regulation`, `battery_degradation`, `transformer_overload`, `price_arbitrage`, `demand_penalty`, `green_energy_ratio`

**值范围：** 0.0 ~ 5.0

**成功响应 200：**
```json
{
  "status": "ok",
  "applied_at": "2026-05-29T10:01:00Z",
  "effective_weights": [
    {"name": "pv_consumption", "old_value": 1.0, "new_value": 1.5}
  ]
}
```

#### 6.5.13 PUT /api/v1/mode（v2.0 替代 /api/ai/mode）

**请求体：**
```json
{
  "mode": "CommercialArbitrage"
}
```

**mode 可选值：** `AgriculturalIrrigation`, `CommercialArbitrage`, `DemandControl`, `VirtualPowerPlant`, `UltraGreen`

**成功响应 200：**
```json
{
  "status": "ok",
  "previous_mode": "AgriculturalIrrigation",
  "current_mode": "CommercialArbitrage",
  "display_name": "自主套利模式",
  "switched_at": "2026-05-29T10:02:00Z"
}
```

#### 6.5.15 GET /api/ai/audit

**请求参数：**
```
start: Option<String>       // ISO 8601 开始时间
end: Option<String>         // ISO 8601 结束时间
operator: Option<String>    // 按操作人筛选
action_type: Option<String> // 按操作类型筛选
page: Option<u32>           // 页码，默认 1
page_size: Option<u32>      // 每页条数，默认 20，最大 100
```

**成功响应 200：**
```json
{
  "total": 156,
  "page": 1,
  "page_size": 20,
  "items": [
    {
      "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
      "timestamp": "2026-05-29T10:01:00Z",
      "operator": "admin",
      "action_type": "weight_adjust",
      "action_detail": {"changes": [{"name": "pv_consumption", "old_value": 1.0, "new_value": 1.5}]},
      "result": "success",
      "fail_reason": null,
      "source_ip": "192.168.1.100"
    }
  ],
  "export_supported": true
}
```

**action_type 枚举：** `weight_adjust`, `mode_switch`, `ab_test_start`, `ab_test_stop`, `model_rollback`

**CSV 导出：** 添加 `Accept: text/csv` 请求头时返回 CSV 格式，单次不超过 10000 条。

#### 6.5.16 GET /api/ai/models

**请求参数：**
```
model_type: Option<String>  // "lstm" | "maddpg" | "ppo"
status: Option<String>      // "active" | "standby" | "archived" | "failed"
```

**成功响应 200：**
```json
{
  "models": [
    {
      "model_id": "lstm_v1.2.0",
      "model_type": "lstm",
      "version": "1.2.0",
      "description": "2026Q1 数据训练的光伏预测模型",
      "file_path": "/models/current/lstm/model.rknn",
      "file_size": 4587520,
      "md5": "a1b2c3d4e5f6...",
      "status": "active",
      "deployed_at": "2026-05-28T10:00:00Z",
      "metrics": {"mae": 0.15, "rmse": 0.22, "mape": 8.5}
    }
  ],
  "total_count": 5
}
```

#### 6.5.17 POST /api/ai/abtest

**请求体：**
```json
{
  "model_type": "lstm",
  "experiment_version": "lstm_v1.3.0-rc1",
  "traffic_percent": 20,
  "duration_hours": 48,
  "metrics": ["avg_reward", "pv_consumption_rate", "voltage_qualification_rate"]
}
```

**校验规则：**
- `experiment_version` 必须为 `standby` 状态
- `traffic_percent` 必须在 1-50 之间
- 同一 `model_type` 不能有运行中的 A/B 测试
- 实验组与对照组模型类型一致

**成功响应 201：**
```json
{
  "test_id": "test-20260529-001",
  "status": "running",
  "control_version": "lstm_v1.2.0",
  "experiment_version": "lstm_v1.3.0-rc1",
  "traffic_percent": 20,
  "started_at": "2026-05-29T10:00:00Z",
  "estimated_end_at": "2026-05-31T10:00:00Z"
}
```

#### 6.5.18 GET /api/ai/abtest/{id}

**成功响应 200：**
```json
{
  "test_id": "test-20260529-001",
  "status": "running",
  "started_at": "2026-05-29T10:00:00Z",
  "ended_at": null,
  "elapsed_hours": 2.5,
  "metrics": [
    {
      "metric": "avg_reward",
      "control_value": 21.5,
      "experiment_value": 22.8,
      "change_percent": 6.05,
      "significant": false
    }
  ],
  "samples": {
    "control": 450,
    "experiment": 112
  }
}
```

**status 可选值：** `running`, `stopped`, `completed`, `failed`

#### 6.5.19 DELETE /api/ai/abtest/{id}

**成功响应 200：**
```json
{
  "status": "stopped",
  "test_id": "test-20260529-001",
  "stopped_at": "2026-05-29T12:30:00Z",
  "final_report": {
    "metrics": [],
    "conclusion": "experiment_superior"
  }
}
```

**conclusion 可选值：** `experiment_superior`, `control_superior`, `inconclusive`

#### 6.5.20 POST /api/ai/rollback

**请求体：**
```json
{
  "model_type": "lstm",
  "target_version": "lstm_v1.1.0",
  "reason": "v1.2.0 预测精度下降",
  "password": "****"
}
```

**成功响应 200：**
```json
{
  "status": "ok",
  "previous_version": "lstm_v1.2.0",
  "current_version": "lstm_v1.1.0",
  "rolled_back_at": "2026-05-29T10:05:00Z",
  "warmup_result": "success"
}
```

**错误响应：**
- 400：权重值越界或名称无效
- 403：权限不足或密码验证失败
- 409：A/B 测试冲突
- 503：AI 引擎未启用或繁忙

---

## 7. 前端页面结构

### 7.1 配色方案 **[DESIGN_APPROVED]**

#### 主色调

| 用途 | 颜色名称 | Hex 值 | 说明 |
|------|----------|--------|------|
| 主色（Primary） | 深蓝色 | `#1E3A5F` | 导航栏、主按钮强调、页面标题 |
| 主色变体（Primary Light） | 中蓝色 | `#2E5A8F` | 按钮悬停状态、次级强调 |
| 主色暗色（Primary Dark） | 暗蓝色 | `#0F1F33` | 深色背景区域 |

#### 功能色

| 用途 | 颜色名称 | Hex 值 | 说明 |
|------|----------|--------|------|
| 成功（Success） | 绿色 | `#28A745` | 正常状态、连接成功 |
| 警告（Warning） | 橙色 | `#FFC107` | 告警状态、需要关注 |
| 危险（Danger） | 红色 | `#DC3545` | 错误状态、连接断开 |
| 信息（Info） | 蓝色 | `#17A2B8` | 信息提示、正在连接 |

#### 中性色

| 用途 | 颜色名称 | Hex 值 | 说明 |
|------|----------|--------|------|
| 背景色（Background） | 深灰蓝 | `#1A1D23` | 页面主背景 |
| 卡片背景（Card Background） | 深灰 | `#252A33` | 卡片、面板背景 |
| 边框色（Border） | 灰色 | `#3D4450` | 输入框、卡片边框 |
| 文本主色（Text Primary） | 浅灰白 | `#E8EAED` | 主要文字 |
| 文本次色（Text Secondary） | 中灰 | `#9AA0A6` | 次要文字、标签 |
| 文本禁用（Text Disabled） | 暗灰 | `#5F6368` | 禁用状态文字 |

### 7.2 字体规范 **[DESIGN_APPROVED]**

#### 字体家族

| 用途 | 字体 | 备选字体 |
|------|------|----------|
| 主字体 | `"JetBrains Mono", "SF Mono", "Consolas", monospace` | 等宽字体，便于阅读技术数据 |
| 界面字体 | `"Inter", "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif` | 导航、按钮、标签 |

#### 字号规范

| 用途 | 字号 | 行高 | 字重 |
|------|------|------|------|
| 页面标题（H1） | 24px | 1.3 | 600 |
| 卡片标题（H2） | 18px | 1.3 | 600 |
| 区块标题（H3） | 16px | 1.4 | 500 |
| 正文（Body） | 14px | 1.5 | 400 |
| 标签（Label） | 13px | 1.4 | 500 |
| 辅助文字（Caption） | 12px | 1.4 | 400 |
| 数据值（Value） | 16px | 1.3 | 500 |

### 7.3 整体布局 **[DESIGN_APPROVED]**

```
┌─────────────────────────────────────────────────────────────┐
│  顶部栏（Header） 高度：56px                                  │
│  [Logo/系统名称]              [用户名] [退出登录]            │
├────────────┬────────────────────────────────────────────────┤
│            │                                                │
│  侧边导航  │              主内容区域                          │
│  宽度：220px│              (Content Area)                    │
│            │                                                │
│  - 状态监控│                                                │
│  - 配置管理│                                                │
│  - 日志查看│                                                │
│  - AI 决策 │                                                │
│  - 专家干预│                                                │
│  - A/B 测试│                                                │
├────────────┴────────────────────────────────────────────────┤
│  底部状态栏（Footer）高度：32px                               │
│  [系统状态] [IEC 104状态] [intercore状态]    [版本信息]      │
└─────────────────────────────────────────────────────────────┘
```

#### 响应式断点

| 断点 | 宽度 | 布局变化 |
|------|------|----------|
| 大屏 | ≥ 1200px | 完整布局，侧边栏展开 |
| 中屏 | 768px ~ 1199px | 侧边栏可折叠 |
| 小屏 | < 768px | 侧边栏收起为汉堡菜单（移动端可选） |

### 7.4 页面清单

| 序号 | 页面名称 | 路由 | 功能说明 |
|------|----------|------|----------|
| 1 | 登录页面 | `/login` | 用户认证 |
| 2 | 状态监控 | `/` | 系统状态、固件版本、编译时间、模块连接状态（默认首页） |
| 3 | 配置管理 | `/config` | IEC 104、intercore、遥测、日志配置 |
| 4 | 日志查看 | `/logs` | 日志查看、筛选、导出、WebSocket 实时推送 |
| 5 | 密码修改 | `/password` | 修改当前用户密码 |
| 6 | AI 决策面板 | `/ai/dashboard` | 场景模式、预测曲线、决策状态、奖励趋势 |
| 7 | 专家干预 | `/ai/intervention` | 权重调整、模式切换 |
| 8 | 审计日志 | `/ai/audit` | 干预操作审计日志查询与导出 |
| 9 | A/B 测试管理 | `/ai/abtesting` | 模型版本列表、A/B 测试创建与监控、回滚 |
| 10 | 在线微调监控 | `/ai/finetuning` | 模型在线微调状态监控 |

### 7.5 各页面设计

#### 7.5.1 登录页面 `/login` **[DESIGN_APPROVED]**

居中卡片式布局：
```
┌────────────────────────────────────────┐
│                                        │
│         [系统Logo]                     │
│       MUPC 通信管理模块                 │
│                                        │
│  ┌──────────────────────────────────┐  │
│  │  用户名                          │  │
│  │  [________________________]       │  │
│  │                                  │  │
│  │  密码                            │  │
│  │  [________________________]       │  │
│  │                                  │  │
│  │  [         登录         ]        │  │
│  │                                  │  │
│  │  错误提示（认证失败时显示）        │  │
│  └──────────────────────────────────┘  │
│                                        │
└────────────────────────────────────────┘
```

**组件规范：** 卡片宽度 400px，输入框高度 44px，登录按钮主色高度 44px 圆角 6px。

#### 7.5.2 状态监控页面 `/` **[DESIGN_APPROVED]**

卡片网格布局，默认 3 列：
```
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│ 固件版本     │  │ 编译时间      │  │ 运行时间     │
│ v1.2.0      │  │ 2026-05-20   │  │ 5d 12h 30m   │
└───────────────┘  └───────────────┘  └───────────────┘
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│ 内存使用率   │  │ CPU温度       │  │ intercore状态│
│ 62% ██████░░│  │ 45.2°C ████░░│  │ ● 已连接     │
└───────────────┘  └───────────────┘  └───────────────┘
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│ IEC 104状态   │  │ 策略模式      │  │ AI引擎状态   │
│ ● 已连接     │  │ 兜底策略     │  │ ● 就绪       │
└───────────────┘  └───────────────┘  └───────────────┘
┌─────────────────────────────────────────────────────┐
│ 最近告警                                              │
│ [●] 2026-05-27 10:30:15 IEC 104 连接断开            │
│ [▲] 2026-05-27 09:15:30 CPU 温度过高 (65°C)        │
└─────────────────────────────────────────────────────┘
```

**组件规范：** 卡片最小宽度 280px，高度 120px，指示灯直径 12px，进度条高度 8px。

#### 7.5.3 配置管理页面 `/config` **[DESIGN_APPROVED]**

分区表单式布局。详见本文档第 2.2 节交互规范。

#### 7.5.4 日志查看页面 `/logs` **[DESIGN_APPROVED]**

顶部筛选条件区，中间日志列表（斑马纹表格），底部操作栏。详见本文档第 2.3 节。

#### 7.5.5 密码修改页面 `/password` **[DESIGN_APPROVED]**

居中卡片式布局，卡片宽度 480px，当前密码 + 新密码 + 确认新密码。

### 7.6 组件规范 **[DESIGN_APPROVED]**

#### 按钮

| 类型 | 样式 | 用途 |
|------|------|------|
| 主按钮（Primary） | 背景 `#1E3A5F`，文字 `#E8EAED`，圆角 6px | 主要操作：保存、确认 |
| 次按钮（Secondary） | 背景透明，边框 `#3D4450`，文字 `#E8EAED` | 次要操作：取消、返回 |
| 危险按钮（Danger） | 背景 `#DC3545`，文字白色 | 危险操作：删除、重置 |
| 禁用状态 | 背景 `#3D4450`，文字 `#5F6368` | 不可点击状态 |

**按钮尺寸：** 小 32px / 中 40px / 大 44px

#### 输入框

| 状态 | 样式 |
|------|------|
| 默认 | 背景 `#252A33`，边框 `#3D4450`，文字 `#E8EAED` |
| 聚焦 | 边框 `#1E3A5F`，box-shadow `0 0 0 2px rgba(30,58,95,0.3)` |
| 错误 | 边框 `#DC3545`，box-shadow `0 0 0 2px rgba(220,53,69,0.3)` |
| 禁用 | 背景 `#1A1D23`，文字 `#5F6368` |

#### 表格

| 元素 | 样式 |
|------|------|
| 表头 | 背景 `#1A1D23`，文字 `#9AA0A6`，字号 13px，字重 500 |
| 表格行 | 行高 40px，奇数行背景 `#252A33`，偶数行背景 `#1A1D23` |
| 行悬停 | 背景 `#2E3440` |
| 分割线 | 边框 `#3D4450`，1px |

#### 卡片

| 元素 | 样式 |
|------|------|
| 背景 | `#252A33` |
| 边框 | `#3D4450`，1px，圆角 8px |
| 内边距 | 20px |
| 标题区 | 底部边框 `#3D4450`，高度 48px |
| 阴影 | 无（工业风格扁平化设计） |

#### 日志级别标签

| 类型 | 样式 |
|------|------|
| ERROR | 背景 `rgba(220,53,69,0.2)`，文字 `#DC3545`，边框 `#DC3545` |
| WARN | 背景 `rgba(255,193,7,0.2)`，文字 `#FFC107`，边框 `#FFC107` |
| INFO | 背景 `rgba(23,162,184,0.2)`，文字 `#17A2B8`，边框 `#17A2B8` |
| DEBUG | 背景 `rgba(95,99,104,0.2)`，文字 `#9AA0A6`，边框 `#9AA0A6` |

---

## 8. 文件结构

### 8.1 web-api crate 新增/修改文件

```
mupc/crates/web-api/src/
├── lib.rs                      # 添加 sse, audit, ai 模块声明 + AppContext 构建
├── auth.rs                     # 扩展：UserRole 枚举、Session 扩展、角色配置加载
├── ws.rs                       # 不做改造（保留 WebSocket 日志推送）
├── routes/
│   ├── mod.rs                  # 添加 pub mod ai;
│   ├── config.rs               # 不变
│   ├── status.rs               # 不变
│   ├── logs.rs                 # 不变
│   └── ai/                     # 新增：AI 相关路由组
│       ├── mod.rs              ~40 行  聚合所有 AI 路由，返回 Router
│       ├── predictions.rs     ~80 行  GET /api/ai/predictions
│       ├── decision.rs        ~80 行  GET /api/ai/decision
│       ├── rewards.rs         ~80 行  GET /api/ai/rewards
│       ├── status.rs          ~60 行  GET /api/ai/status
│       ├── finetuning.rs      ~60 行  GET /api/ai/finetuning
│       ├── weights.rs         ~100 行 PUT /api/ai/weights (含验证)
│       ├── mode.rs            ~120 行  GET/PUT /api/v1/mode, GET /api/v1/mode/list
│       ├── models.rs          ~60 行  GET /api/ai/models
│       ├── abtest.rs          ~120 行 POST/GET/DELETE /api/ai/abtest
│       └── rollback.rs        ~100 行 POST /api/ai/rollback
├── sse/
│   ├── mod.rs                 ~20 行  模块声明
│   └── ai_sse.rs              ~150 行 SsePushService + SSE 端点
└── audit/
    ├── mod.rs                 ~20 行  模块声明
    ├── storage.rs             ~120 行 SQLite 存储 (AuditLogger)
    └── handler.rs             ~80 行  GET /api/ai/audit 处理器
```

### 8.2 ai-engine crate 新增/修改文件

```
mupc/crates/ai-engine/src/
├── lib.rs                     # 修改：添加 pub mod types; 及 pub use
├── types.rs                   # 新增 ~200 行：场景模式、权重参数、A/B 测试结构体
└── model_manager.rs           # 修改：新增 AiEngineExt trait 实现方法
```

### 8.3 strategy-engine crate 新增/修改文件

```
mupc/crates/strategy-engine/src/
├── lib.rs                     # 修改：添加 pub mod ab_testing;
├── ai_integration.rs          # 修改：新增门面代理方法
└── ab_testing.rs              # 新增 ~150 行：AbTestRouter + 生命周期管理
```

### 8.4 配置文件

```
/etc/mupc/
├── auth/roles.toml            # 角色定义（新增）
├── weights.toml               # 权重持久化（新增）
├── mode.toml                  # 模式持久化（新增）
└── models/manifest.json       # 模型版本清单（新增）
```

### 8.5 代码量预估

| 模块 | 新增/修改行数 | 说明 |
|------|-------------|------|
| web-api AI 路由 | ~860 行 | 14 个端点处理器 |
| SSE 推送服务 | ~170 行 | SsePushService + SSE 端点 |
| 审计日志 | ~220 行 | SQLite 存储 + HTTP 处理器 |
| ai-engine 扩展 | ~250 行 | AiEngineExt + 数据结构 + ModelManager 方法 |
| strategy-engine 扩展 | ~200 行 | AiIntegrator 代理方法 + AbTestRouter |
| 认证扩展 | ~50 行 | UserRole, 角色配置 |
| **总计** | **~1750 行** | |

### 8.6 权限模型与认证

#### 用户角色

| 角色 | 权限等级 | 说明 |
|------|----------|------|
| Viewer（调度人员） | 只读 | 查看状态、决策面板、日志 |
| Operator（策略管理员） | 中级 | 可调整权重和切换模式 |
| AiExpert（AI 运维专家） | 最高级 | 所有操作，含 A/B 测试和回滚 |
| Admin（系统管理员） | 高 | 系统配置管理、固件管理 |

#### 角色配置存储

```toml
# /etc/mupc/auth/roles.toml
[users]
admin = { role = "ai_expert", password_hash = "xxx" }
operator1 = { role = "operator", password_hash = "xxx" }
viewer1 = { role = "viewer", password_hash = "xxx" }
```

#### 权限检查中间件

```rust
/// 用户角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    Viewer,           // 调度人员 - 只读
    Operator,         // 策略管理员 - 可调整权重和模式
    AiExpert,         // AI 运维专家 - 所有操作
}

/// 权限检查守卫：要求角色不低于指定等级
pub fn require_role(required: UserRole) -> axum::middleware::Next {
    // Viewer < Operator < AiExpert
    // 如果不足返回 403
}
```

#### 路由注册示例

```rust
// 只读路由 - 全部角色可访问
Router::new()
    .route("/api/ai/predictions", get(predictions_handler))
    .route("/api/ai/decision", get(decision_handler))

// 写操作路由 - 需要 Operator 及以上
Router::new()
    .route("/api/ai/weights", put(weights_handler))
    .route_layer(middleware::from_fn(require_role(UserRole::Operator)))

// A/B 测试和回滚 - 需要 AiExpert
Router::new()
    .route("/api/ai/abtest", post(abtest_handler))
    .route("/api/ai/rollback", post(rollback_handler))
    .route_layer(middleware::from_fn(require_role(UserRole::AiExpert)))
```

### 8.7 AppContext 共享状态

```rust
/// web-api 应用状态（扩展）
#[derive(Clone)]
pub struct AppContext {
    pub ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    pub session_manager: SessionManager,
    pub audit_logger: Arc<AuditLogger>,
    pub sse_push_service: Arc<SsePushService>,
    pub decision_cache: Arc<tokio::sync::RwLock<Option<DecisionSnapshot>>>,
}
```

### 8.8 Cargo.toml 依赖更新

**web-api 新增依赖：**
```toml
mupc-strategy-engine = { path = "../strategy-engine" }
rusqlite = { workspace = true }    # 从 workspace 继承
```

**ai-engine 新增依赖：**
```toml
serde_json = { workspace = true }  # 新增（用于场景/权重序列化）
```

### 8.9 数据库表扩展

在 data-processing 的 SQLite 数据库（`/var/log/mupc/data.db`）中扩展：

```sql
CREATE TABLE IF NOT EXISTS reward_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,          -- Unix 时间戳秒
    total_reward REAL NOT NULL,
    components_json TEXT NOT NULL,       -- JSON 数组
    running_mode TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_reward_timestamp ON reward_history(timestamp);

-- A/B 测试结果表
CREATE TABLE IF NOT EXISTS ab_test_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    test_id TEXT NOT NULL,
    timestamp INTEGER NOT NULL,
    group_name TEXT NOT NULL,             -- "control" | "experiment"
    metrics_json TEXT NOT NULL            -- JSON
);
CREATE INDEX IF NOT EXISTS idx_abtest_id ON ab_test_metrics(test_id);
```

---

## 9. 边界条件与异常处理

本章定义 AI 可视化与专家干预模块在各种异常场景下的边界条件处理和用户界面表现，确保系统在非正常运行状态下仍能向运维人员提供清晰、可操作的反馈。

### 9.1 AI 引擎未启用

当 AI 引擎因配置、许可或硬件条件不满足而未启用时，系统运行于本地策略模式，相关 AI 功能面板需要进行适配处理。

| 编号 | 触发条件 | UI 表现 | 用户可执行的操作 |
|------|---------|---------|----------------|
| 9.1.1 | AI 引擎未启用 | AI 预测曲线区域显示"AI 引擎未启用"占位提示，使用灰色背景与正常状态区分 | 无操作可执行，此为系统配置状态 |
| 9.1.2 | AI 引擎未启用 | 决策逻辑面板隐藏，显示"当前为本地策略模式"说明文字 | 无操作可执行 |
| 9.1.3 | AI 引擎未启用 | 实时奖励面板隐藏，不展示 RL 决策相关数据 | 无操作可执行 |
| 9.1.4 | AI 引擎未启用 | 场景识别状态显示"手动模式"或"N/A"，置信度百分比字段留空 | 无操作可执行 |

**验收标准：**
- [ ] AI 引擎未启用时，预测曲线区域显示占位提示而非空白
- [ ] 决策逻辑面板和实时奖励面板在 AI 未启用时正确隐藏
- [ ] 场景识别状态字段在 AI 未启用时显示"N/A"
- [ ] 各组件在 AI 引擎重新启用后 5 秒内恢复正常显示

### 9.2 模型异常

AI 模型在加载、推理或运行过程中可能出现各种异常状态，需要及时向运维人员展示错误信息和恢复手段。

| 编号 | 触发条件 | UI 表现 | 用户可执行的操作 |
|------|---------|---------|----------------|
| 9.2.1 | 模型加载失败（文件损坏、版本不兼容等） | AI 面板顶部显示红色错误横幅，文案"模型加载失败：{具体原因}"，提供"重试加载"按钮 | 点击"重试加载"按钮触发模型重新加载；检查模型文件完整性 |
| 9.2.2 | 推理超时（超过 1 秒阈值） | 显示黄色警告横幅"推理超时，已自动降级至 CPU 推理" | 检查 NPU 状态；手动重启 NPU 推理 |
| 9.2.3 | 推理结果异常（NaN/Inf） | 丢弃异常推理结果，使用上一有效值；累计 10 次后在页面顶部显示橙色告警横幅"推理结果异常（已发生 {N} 次）" | 检查模型版本；考虑执行模型回滚 |
| 9.2.4 | NPU 温度过高（超过安全阈值） | 页面顶部显示橙色警告横幅"NPU 温度过高"，显示当前温度及降频状态（如"当前 85°C，已降频至 800MHz"） | 检查散热条件；等待温度恢复正常后系统自动恢复原始频率 |
| 9.2.5 | 模型版本回滚中 | AI 面板显示蓝色进度提示"模型回滚中..."，禁用权重调整和模式切换按钮 | 等待回滚完成；回滚期间不影响系统正常运行（回滚不影响正在运行的推理） |

**验收标准：**
- [ ] 模型加载失败时错误横幅在页面加载完成后 3 秒内出现
- [ ] "重试加载"按钮可触发模型重新加载流程，重试期间按钮显示 loading 状态
- [ ] 推理超时降级提示在超时发生后 2 秒内显示
- [ ] 推理结果异常累计计数器正确工作，达到阈值后告警准确触发
- [ ] NPU 温度过高告警在温度降至安全阈值后自动清除
- [ ] 模型回滚中界面准确反映回滚进度

### 9.3 权重与模式异常

专家干预操作涉及的权重调整和模式切换场景中的边界条件处理。

| 编号 | 触发条件 | UI 表现 | 用户可执行的操作 |
|------|---------|---------|----------------|
| 9.3.1 | 手动权重值超出有效范围（< 0.0 或 > 5.0） | 前端滑块限制在 0.0-5.0 范围内，无法拖出边界；数值输入框在输入越界值时显示红色边框和错误文字"权重值需在 0.0-5.0 之间"；后端二次校验拒绝写入并返回 400 错误 | 将权重值调整至有效范围内重新提交 |
| 9.3.2 | 强制切换模式被后端拒绝（如安全校验未通过） | 显示错误 Toast 提示，文案为拒绝原因（如"模式切换被拒绝：安全校验未通过"），当前模式保持不变 | 确认拒绝原因；联系系统管理员；等待安全条件满足后重试 |
| 9.3.3 | 模式切换冲突（远程调度指令与本地操作并发） | 显示冲突提示"远程调度优先，本地操作被拒绝"；v2.0 优先级：调度主站 > 策略管理员 | 确认冲突来源；等待远程指令完成后重试 |
| 9.3.4 | 权重配置文件损坏（文件解析失败或格式错误） | 页面顶部显示黄色警告横幅"权重配置已重置为默认值"，系统自动加载默认权重替换损坏配置 | 确认当前权重值是否符合预期；可通过专家干预页面重新调整权重 |

**验收标准：**
- [ ] 前端滑块控件无法拖出 0.0-5.0 范围边界
- [ ] 后端二次校验在收到越界值时返回明确的 400 错误及错误描述
- [ ] 模式切换被拒绝时错误提示显示具体拒绝原因，非通用错误信息
- [ ] 冲突检测在操作执行前完成校验
- [ ] 权重配置文件损坏时系统自动回退至默认权重，不影响核心决策功能
- [ ] 权重重置提示在下次页面加载或 SSE 状态推送时显示

### 9.4 A/B 测试异常

A/B 测试流程中的边界条件处理和异常恢复机制。

| 编号 | 触发条件 | UI 表现 | 用户可执行的操作 |
|------|---------|---------|----------------|
| 9.4.1 | 流量分配比例总和不为 100% | 前端在提交前校验，若比例 ≠ 100% 则拒绝提交，在输入框下方显示红色错误文字"流量分配总和必须为 100%"，提交按钮保持 disabled 状态 | 调整流量分配比例直至总和为 100% |
| 9.4.2 | 模型 B（实验组）加载失败 | A/B 测试页面顶部显示红色告警横幅"实验组模型加载失败，已自动将所有流量切至模型 A"；A/B 测试状态标记为 `failed` | 检查实验组模型文件；待问题修复后重新创建 A/B 测试 |
| 9.4.3 | 效果对比数据不足（样本数 < 100） | 效果对比指标区域显示"数据收集中，至少需要 100 个样本"提示，附带进度条展示当前已收集样本数 / 100 | 等待数据收集完成；此状态为正常等待阶段 |
| 9.4.4 | 模型回滚失败（目标版本文件缺失、磁盘空间不足等） | 页面顶部显示红色错误横幅"回滚失败：{失败原因}"，并在横幅下方提供"手动回滚"操作指引链接 | 点击"手动回滚"链接查看详细操作指引；检查目标版本文件完整性；确保磁盘空间充足后重试 |

**验收标准：**
- [ ] 前端流量分配校验实时生效，比例不正确时提交按钮不可点击
- [ ] 实验组模型加载失败后流量自动切回对照组，切换延迟 < 5 秒
- [ ] 数据不足提示显示当前样本计数，计数实时更新
- [ ] 回滚失败后系统保持在回滚前版本继续运行，不中断业务
- [ ] "手动回滚"操作指引内容明确，步骤清晰

### 9.5 微调异常

模型在线微调过程中的边界条件处理和状态展示。

| 编号 | 触发条件 | UI 表现 | 用户可执行的操作 |
|------|---------|---------|----------------|
| 9.5.1 | 微调触发条件不满足（如数据缓冲区未满、距上次微调间隔不足） | 微调监控页面状态显示"微调等待中"，并在状态下方列出不满足的具体条件（如"数据缓冲区：18/32"、"距上次微调：45 分钟 / 60 分钟"） | 等待条件满足自动触发；此为正常等待状态 |
| 9.5.2 | 微调过程中 Loss 发散（连续 N 个 epoch Loss 不降反升） | 微调自动停止，页面顶部显示红色警告横幅"微调已停止：Loss 发散"，显示发散时的 Loss 曲线截图 | 检查训练数据质量；考虑调整微调超参数；手动触发重新微调 |
| 9.5.3 | 微调中系统负载过高（CPU 使用率 > 90% 或内存使用率 > 85%） | 微调监控页面显示黄色警告"系统负载过高，微调已暂停"，显示当前 CPU/内存使用率；微调在负载降至阈值以下后自动恢复 | 检查系统负载来源；等待负载降低后自动恢复；或手动停止微调 |
| 9.5.4 | 微调训练数据不足（缓冲区数据量 < batch_size） | 微调监控页面状态显示"训练数据收集中"，附带进度展示"缓冲区: X / {batch_size}"（如"缓冲区: 12/32"） | 等待数据收集完成；可通过增加数据采集频率加速收集 |

**验收标准：**
- [ ] 微调等待中状态明确列出所有不满足的条件及当前进度
- [ ] Loss 发散检测在连续 3 个 epoch Loss 上升后自动触发停止
- [ ] 微调暂停后系统负载低于阈值（CPU < 70%、内存 < 75%）持续 30 秒后自动恢复
- [ ] 数据收集进度每新增一条数据时实时更新
- [ ] 微调异常状态下核心决策功能不受影响

### 9.6 通用异常处理原则

以下原则适用于本文档定义的所有边界条件与异常场景：

1. **降级优先**：异常发生时优先保持系统核心决策功能的运行，非关键功能可降级或暂停
2. **明确反馈**：所有异常状态必须有明确的用户界面反馈，禁止静默失败
3. **可操作告警**：告警信息必须包含可操作的恢复指引，无法自动恢复的场景提供手动操作入口
4. **自动恢复优先**：能够自动恢复的异常场景，设计自动恢复机制并向用户展示恢复进度
5. **审计可追溯**：所有异常状态变更记录到审计日志，包括异常时间、类型、持续时间、恢复操作

---

## 10. 技术决策记录

### 10.1 SSE vs WebSocket 选择

**问题：** AI 可视化实时数据推送应使用 SSE 还是 WebSocket？

**决策：** SSE

| 维度 | SSE | WebSocket |
|------|-----|-----------|
| 通信方向 | 单向（服务端→客户端） | 双向 |
| 自动重连 | 浏览器原生支持 | 需自行实现 |
| Axum 支持 | `axum::response::sse::Sse` | `axum::extract::ws::WebSocket` |
| 资源占用 | 低（keep-alive） | 中（需管理状态） |
| 适用场景 | 服务端频率推送（最适合） | 双向交互 |

**理由：** 本需求所有 AI 推送都是服务端→浏览器单向，SSE 更轻量，浏览器原生自动重连，Axum 原生支持。系统日志推送（`/ws/logs`）仍保留 WebSocket，两者各司其职。

### 10.2 前端框架方案

**问题：** 选择什么前端框架？

**决策：** 纯 HTML + CSS + JavaScript 或 Vue 3（轻量版）。不引入重型框架（如 React、Angular），保证页面加载速度 ≤ 2 秒。

### 10.3 审计日志存储方案

**问题：** 审计日志与奖励历史共用数据库还是独立？

**决策：** 审计日志使用独立 SQLite 数据库（`/var/log/mupc/audit.db`），与 data-processing 的 data.db 分开。理由：审计日志安全性要求高（仅追加、不可删除），独立存储方便权限控制；奖励历史使用 data.db 减少重复存储。

### 10.4 权重持久化方案

**问题：** 使用 TOML 文件还是 SQLite 存储权重？

**决策：** TOML 文件。理由：权重配置量小（7 个参数），TOML 易读易写，可手工编辑；原子写入保证一致性。模式配置同理。

### 10.5 AiIntegrator 作为服务门面

**问题：** web-api 是否可以直接调用 ai-engine？

**决策：** 不可以。web-api 统一通过 strategy-engine 的 `AiIntegrator` 编排，不直接调用 ai-engine。理由：AiIntegrator 已承担安全校验、AI 指令兜底校验职责，统一入口确保干预指令经过安全验证。

### 10.6 缓存策略

决策状态缓存：web-api 层维护 `Arc<RwLock<Option<DecisionSnapshot>>>`，由后台 tokio 任务周期性更新。前端 1 秒轮询或 SSE 订阅时读取缓存，避免每次请求触发 AI 推理。

### 10.7 分阶段实施计划

| 阶段 | 内容 | 估算工时 |
|------|------|---------|
| Phase 1 | ai-engine types.rs + AiEngineExt 接口定义 + ModelManager 扩展方法（桩实现） | 1 天 |
| Phase 2 | strategy-engine AiIntegrator 代理方法 + AbTestRouter | 1 天 |
| Phase 3 | web-api AI 路由（只读 5 个 GET 端点）+ SSE 推送 | 2 天 |
| Phase 4 | web-api 写操作路由（weights, mode）+ 审计日志存储 | 1.5 天 |
| Phase 5 | A/B 测试路由 + 模型版本管理 + 回滚 | 1.5 天 |
| Phase 6 | 权限中间件 + 角色配置 + 集成测试 | 1 天 |
| **总计** | | **8 天** |

### 10.8 测试策略

- **单元测试**：每个路由处理器测试请求/响应格式；AbTestRouter 测试 hash 分布均匀性；AuditLogger 测试写入和查询
- **集成测试**：使用 `axum::test` 启动测试服务器，模拟完整请求链路
- **性能测试**：SSE 连接数压测（目标 100 并发连接）；审计日志查询性能（365 天数据量）

---

**文档状态：** 修订版（v1.1）

**来源文档状态：**
- `08-MUPC-Web管理与AI可视化-PRD.md` — **[REVIEWED: PASS]**
- `2026-05-27-MUPC-WebUI-设计.md` — **[DESIGN_APPROVED]**
- `2026-05-29-MUPC-AI可视化与专家干预-设计文档.md` — **[DESIGN_APPROVED]**

**变更说明：**
- **v1.0**：合并三份文档中与 Web 管理和 AI 可视化相关的全部内容
  - 保留所有 [REVIEWED: PASS] 和 [DESIGN_APPROVED] 标记
  - 按"模块架构 → 系统管理 → AI 可视化 → 专家干预 → A/B 测试 → API → 前端 → 文件结构 → 技术决策"重组
  - 去重合并配置管理、日志管理、状态监控等重叠内容
  - 附录：术语表、参考文档、外部依赖见 PRD 对应章节
- **v1.1**：新增第 9 章"边界条件与异常处理"
  - 补充 5 大类 21 种异常/边界场景的 UI 处理和用户操作指引
  - 新增通用异常处理原则（降级优先、明确反馈、可操作告警、自动恢复优先、审计可追溯）
  - 原第 9 章"技术决策记录"重新编号为第 10 章

### v1.1 修订记录

| 修订项 | 说明 |
|--------|------|
| 新增章节 | 第 9 章"边界条件与异常处理" |
| 覆盖场景 | AI 引擎未启用（4 种）、模型异常（5 种）、权重与模式异常（4 种）、A/B 测试异常（4 种）、微调异常（4 种） |
| 新增原则 | 9.6 通用异常处理原则（5 条设计原则） |
| 章节重编号 | 原"第 9 章 技术决策记录" → "第 10 章 技术决策记录" |
| 版本更新 | v1.0 → v1.1 |
| 修订依据 | 设计覆盖度审查 CONDITIONAL_PASS 整改要求（PRD 第 9 章边界条件未在设计文档中体现） |

### v1.2 修订记录

| 修订项 | 说明 |
|--------|------|
| 同步 v2.0 预设运行场景 | 全局替换：自动识别→预设选择、SceneClassifier→ModeSelector |
| 删除 `/api/ai/mode/auto` | 无自动识别功能，删除恢复端点（API 表 + 6.5.14 节 + routes 清单） |
| 更新模式切换 API | `/api/ai/mode` → `/api/v1/mode`，mode 值重命名（agriculture→AgriculturalIrrigation 等） |
| 新增模式查询 API | `GET /api/v1/mode` + `GET /api/v1/mode/list` |
| 移除"本地兜底模式" | 5 种预设场景（非 6 种），AI 降级由系统内部处理 |
| 移除"置信度"概念 | 3.6 + 6.5.8 + 6.5.10: scene_mode → running_mode，删除 confidence 字段 |
| 更新模式持久化 | mode.toml → current_mode 单字节文件 + config [mode] 段 |
| 更新审计操作类型 | 移除 resume_auto 枚举值 |
| 更新边界条件 | 9.3.3: "AI 自动"优先级移除，改为"调度主站 > 策略管理员" |
| 版本更新 | v1.1 → v1.2 |
| 修订依据 | `2026-05-29-MUPC-AI预设运行场景与互斥模式选择-PRD.md` [REVIEWED: PASS] |
| 配套设计 | `2026-05-29-MUPC-AI预设运行场景与互斥模式选择-设计文档.md` [DESIGN_APPROVED] |
