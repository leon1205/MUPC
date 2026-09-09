# 12-MUPC 本地显示终端（HDMI 屏运行状态展示）设计文档

> `[DESIGN_APPROVED: 2026-09-09, 设计评审员]`
>
> `[DESIGN: PENDING REVIEW]` — 技术设计草稿（待评审）
>
> - 版本：v1.0（草稿）
> - 日期：2026-09-09
> - 状态：待评审（`[PENDING REVIEW]`）
> - 关联 PRD：[`12-MUPC-本地显示终端-PRD.md`](../specs/modules/12-MUPC-本地显示终端-PRD.md)（`[REVIEWED: PASS]`）
> - 权威点表：协议 V1.3（EMS）3 区只读点表（FC04）
> - 目标平台：BECG-3568（RK3568 / RK3588，aarch64 Linux），HDMI 1024x768；无浏览器前提

---

## 目录

1. [方案探索与技术选型](#1-方案探索与技术选型)
2. [架构总览](#2-架构总览)
3. [数据通道协议与接口定义](#3-数据通道协议与接口定义)
4. [mupcd 侧改动](#4-mupcd-侧改动)
5. [渲染进程设计](#5-渲染进程设计)
6. [画布布局结构与 PRD 映射](#6-画布布局结构与-prd-映射)
7. [配置项](#7-配置项)
8. [边界与异常处理对照](#8-边界与异常处理对照)
9. [非功能预算落实](#9-非功能预算落实)
10. [测试策略](#10-测试策略)
11. [交叉编译与部署](#11-交叉编译与部署)
12. [技术决策记录（ADR）与选型理由](#12-技术决策记录adr-与选型理由)
13. [开发前置风险 / 待验证项](#13-开发前置风险--待验证项)

---

## 1. 方案探索与技术选型

> KISS 原则贯穿：只做「读展示 + 降级」，不做取数重判、不做交互、不建趋势、不引入浏览器内核。

### 1.1 关键决策 A：跨进程数据通道

候选 3 条路线对比（面向 1 Hz 展示、渲染进程只读、mupcd 崩溃自愈、本机无浏览器）：

| 路线 | 实时性@1Hz | 耦合/安全 | 实现成本 | mupcd 崩溃时渲染进程行为 | 跨平台开发便利（Windows 本地 x86） |
|------|-----------|----------|---------|------------------------|-------------------------------|
| **A1 本地 HTTP（127.0.0.1 GET /latest，轮询 500ms，返回最新帧 JSON）** | 满足；轮询即拉最新帧，允许丢帧 | 只回环不暴露外网；无鉴权面（本地可信）；与 web-api 北向服务解耦 | 低：mupcd 侧一个极简路由，渲染侧一个极简 HTTP GET 客户端（各约 80 行，无 TLS） | 连接拒绝/超时 → 渲染端计数判通道断 → ≤3s 进 6.3 画面；恢复即下轮轮询回实时 | **优**：TCP 回环在 Windows/Linux 同语义，开发期可直接连测试桩 |
| A2 Unix socket（SOCK_STREAM + JSON 行/长度帧） | 同 A1，真推送可更省轮询 | 本地文件权限；不占端口 | 中：需自管监听/重连/成帧；Windows 支持不佳 | EOF 即断，可靠 | **劣**：Windows 上 tokio `Unix*` 不可用，本地开发需抽象双通道，违背 KISS |
| A3 共享内存 + 信号/eventfd | 最低延迟 | mmap 需同机 | 高：需 seqlock 防撕裂、字段级栅栏、崩溃检测自写 | 需时间戳心跳 + 锁方案，正确性成本高 | **劣**：Win/ARM 分配与验证都繁琐，1Hz 场景纯属过度设计 |

**选型结论：A1 —— 本地 HTTP（TCP 回环 127.0.0.1）短轮询「最新帧快照」，轮询周期 500ms。**

理由：
1. **状态在服务端、轮询取快照**，天然满足「允许丢帧但只丢中间帧、总是拿到最新有效帧」（PRD 4.4.1），无需渲染端做重传/排序。
2. 通道断 = 连接失败，判定简单可靠（PRD 4.4.2/6.3）；mupcd 重启后端口恢复，下轮自动回实时（4.3.3 ≤1s）。
3. 500ms 轮询下，一帧发布后最坏 ~500ms 被取到（落入「推送周期」语义），取到后立即上屏 ≤ 数 ms，满足 F5.2「收到→上屏 ≤500ms」。
4. CPU 友好：阻塞式 GET + 等待间隔 sleep，无忙等/自旋（PRD 4.1.3）；断连满载时开销≈一次连接失败/500ms，CPU 趋近 0（4.1.3 断时 ≤5%）。
5. 开发/测试优势：Windows 本地与 Linux 目标同语义，渲染进程可对接任意 stub 服务端，利于「无真屏验证」。
6. 不并入 web-api(8080)（避免把本地遥测暴露到北向网口、避免跟随 https/证书生命周期耦合）；在 mupcd 内独立起一个**仅 127.0.0.1** 的回环监听。

> 备选（评审可议）：若团队偏好既有 SSE 通道，可把帧经 SSE 推送，但需补「last-frame 重放端点」处理重连丢帧，成本 > A1，故不推荐。

### 1.2 关键决策 B：原生渲染栈（RK3568 Linux，1024x768，中文渲染）

候选对比（依赖体积 / 交叉编译 / 中文资源 / CPU≤15% / 内存≤64MB / 崩溃自恢复 / 无真屏可测）：

| 路线 | 依赖体积 | aarch64 交叉 | 中文渲染 | CPU/内存 | 无真屏验证 | 自绘代码量 |
|------|---------|-------------|---------|---------|-----------|-----------|
| **B1 纯 framebuffer 自绘（直写 /dev/fb0 或 DRM dumb-buffer）+ 纯 Rust 光栅化（ab_glyph）渲染少量中文，捆绑 CJK 子集字体** | 无 C 依赖，二进制极小 | **纯 Rust 一次编过**（无 C 需先交叉编 SDL） | 捆绑 OFL 子集 OTF；只渲染 ~60 个汉字 + ASCII 数字 | 极低（离屏 3MB + 字库 <1MB + 图集小）；自绘无运行时 GUI 栈 | 屏幕后端抽象成 `offscreen`（内存 Canvas→PNG），单测/CI 全平台确定 | 中（布局/字形绘制自写，但内容为固定网格 + 少量动态数字/中文词，量可控） |
| B2 SDL2（KMSDRM/fbcon）+ SDL_ttf/FreeType | 中（SDL2+SDL_ttf+FreeType+fontconfig） | **重**：需先交叉编 SDL2 目标库或 vendor aarch64 .so；Dev(win) 还需 SDL2.dll | 任意 TTF 即可；捆绑 Noto/WQY | 低~中；软件渲染够用 | SDL dummy driver 离屏 + 存图 | 少（绘图原语现成） |
| B3 GTK3 / Qt Embedded | 大 | 巨大依赖树 + fontconfig/pango | OK | GTK/Qt 常态基座内存可能顶到/超 64MB 红线 | 需 xvfb/Wayland | 少但引入面大 |
| B4 仅 bundling 系统字库 + 已有 GUI | — | — | 依赖系统字库不确定性高（目标镜像未必带 CJK 字库） | — | — | — |

**选型结论：B1 —— 纯 framebuffer 自绘 + ab_glyph（纯 Rust）光栅化捆绑 CJK 子集字体。**

理由：
1. 屏面内容天然「固定分区布局 + 少量动态数字 + 一组有限中文状态词」，无需通用 GUI 框架；自绘代码被约束在固定网格，量可控且完全确定，可离屏单测。
2. 渲染进程零 C 依赖（ab_glyph 纯 Rust 读 TTF/OTF 并光栅化）→ 交叉编译 aarch64 无 C 工具链前置；二进制与内存都最小，CPU 预算（单帧自绘 + 每 500ms 一次全屏拷贝）轻松落在 ≤15%/≤64MB 内。
3. 崩溃自恢复与主进程隔离与栈无关（进程级 systemd Restart）；B1 不依赖 X/Wayland/合成器，天然适配 headless + HDMI。
4. 屏幕后端用 `Screen` trait 收敛（`fbdev` / `drm` / `offscreen`），**无真屏时以 `offscreen` 跑全链路**并出 PNG，真机只验证驱动层薄薄一段。

> **中文字体资源落点（必须给定）**
> - **来源**：采用开源 OFL 授权中文黑体，首选 **文泉驿微米黑（WenQuanYi Micro Hei）或 Noto Sans SC**（openEuler/Ubuntu apt 可装，RK 镜像一般也带）。全字库体积大（数 MB~10MB）。
> - **处置**：用 `pyftsubset`（fonttools）按「必需码表 + ASCII + 状态词 + 数字符号」**离线子集化**为单个 `.otf`（预期 100KB~500KB），入库路径 `mupc/crates/local-display/fonts/`。
> - **加载**：优先以 `include_bytes!` **编译进渲染进程二进制**（运行期零文件依赖、测试可复现、镜像无需额外字库文件）；渲染进程可用启动参数 `--font <path>` 覆盖为外部/系统字库（含 `/usr/share/fonts/...`），**不入 core 配置**（见 §7.2）。
> - **需覆盖的字集（子集化码表，列出以便开发直接执行）**：`充 放 停 待 机 电 状 态 储 能 系 统 电 池 荷 运 行 三 相 有 功 功 率 电 流 总 量 源 离 线 未 取 数 据 异 常 过 期 失 效 方 向 不 一 致 初 始 化 中 与 主 进 程 断 开 时 间 数 值 低 高 警 示 区 域 本 地 台 区 关 于 B M S P C S 0-9 . - % A 斜杠 / 冒号 :`（精确列表开发期在 font.rs 顶部常量集中维护，避免漏字）。

> 备选（评审可议）：若评审更倾向少自绘、可接受 C 依赖与交叉成本，可退到 B2（SDL2）。本文按 B1 为推荐主方案展开，B2 仅作备选，不双线实现。

---

## 2. 架构总览

### 2.1 进程拓扑

```
┌────────────────────────────── 单机（BECG-3568, Linux） ──────────────────────────────┐
│                                                                                       │
│  进程 P0 = mupcd（主进程 / 大脑）                                                      │
│  ┌─────────────────────────────────────────────────────────────────────────────┐     │
│  │ 核间 modbus (RS485 ↔ PCS, FC04/FC06)                                        │     │
│  │   ModbusRtuTransport·run_heartbeat_loop ──▶ 1013 RUN_STATE(心跳1s, 在线判定)   │     │
│  │   dispatch 决策循环(1s) ── apply_soc_source ──▶ 1010 SOC(单源裁决: BMS优先/核间回落)   │     │
│  │   DisplayDataProvider 采集循环(1s, display.enabled 时) ──▶ 1022/1023/1024     │     │
│  │        (输出电流) + 1029/1030/1031(有功) + 1032(总有功) [单次或两段 FC04]       │     │
│  └───────────────┬─────────────────────────────────────────────────────────────┘     │
│                  │ 域值化(×0.1 量纲 / 量程校验 / 打 valid 标志 / 一致性比对 6.6)        │
│  AiIntegrator  SOC 单源裁决值(唯一裁决点, 见 §4.3) ──────────┐                          │
│  ┌───────────────▼────────────────────────────────────────┐ │                          │
│  │  DisplayDataProvider（新组件, 归属 mupcd, core-bin 内） │ │                          │
│  │  每 1s: 组 DisplayFrame → seq++ / ts → 更新 latest      │◀┘                          │
│  └───────┬────────────────────────────────────────────────┘                           │
│          │ 发布(内存 Mutex<latest> + 序列化)                                            │
│  Loopback HTTP 服务（127.0.0.1:9810, GET /v1/display/latest → JSON 帧）              │
└──────────┬──────────────────────────────────────────────────────────────────────┘     │
           │ 跨进程数据通道（TCP 回环, 只读, 无下行写）                                    │
┌──────────▼──────────────────────────────────────────────────────────────────────┐     │
│  进程 P1 = mupc-local-display（独立渲染进程, systemd Restart=always ≤3s 自恢复）    │     │
│    main ─▶ channel client(500ms 轮询) ─▶ 状态模型/新鲜度 ─▶ 布局绘制 ─▶ Screen.blit  │     │
│    Screen = fbdev(/dev/fb0) | drm(/dev/dri/card0) | offscreen(测试)               │     │
└──────────────────────────────────────┬───────────────────────────────────────────┘     │
                                       │ HDMI
                                  ┌────▼─────┐
                                  │ 8寸屏 1024x768 │
                                  └──────────┘
└───────────────────────────────────────────────────────────────────────────────────┘
```

要点：
- **数据只在 mupcd 内「读点表 → 域值化/裁决 → 推送」**；渲染进程只展示 + 降级，不直连 modbus、不判源、不下行（PRD 边界）。
- 渲染进程可先于 mupcd 启动（显示「初始化中」），通道就绪 ≤1s 切实时；mupcd 崩溃 → 通道断开画面（6.3）；mupcd 恢复即回实时（4.3.3）。
- 通道只承载「读展示 + 心跳/健康」；**无任何下行写**（4.4.4）。

---

## 3. 数据通道协议与接口定义

### 3.1 通道形态（定稿）

- 传输：TCP 回环 `127.0.0.1:<port>`（默认 `9810`）。
- 端点：`GET /v1/display/latest` → `200` + `Content-Type: application/json` + 最新帧 JSON（服务端每请求返回**当前最新帧**；未就绪则返回协议缺省帧或 `503`，渲染端视同无新帧重试）。
- 方法/请求：单 GET，无鉴权（仅回环）、无查询参数、无请求体。
- 内容：帧（见 §3.3）。Content-Length 定长返回；服务端不依赖保活（每次连接可关闭，渲染端每轮新建连接亦可）。
- 语义：**丢中间帧允许**；每次取到即最新有效帧；渲染端以 `seq` 判单调与重排。

### 3.2 发布/订阅两端组件命名

- mupcd 侧发布组件：**DisplayDataProvider**（采集+组帧+发布）＋ **LoopbackHttpPublisher**（回环 GET 服务）。
- 渲染进程侧订阅客户端：**DisplayChannelClient**。

### 3.3 帧数据模型（display-proto crate，跨进程契约）

`crates/display-proto/src/lib.rs`：

```rust
/// 帧协议版本
pub const PROTO_VERSION: u8 = 1;
/// 渲染端判「数据过期」阈值（与 PRD F5.3: 当前时间−帧时间戳 >2s 判过期）
pub const DEFAULT_STALE_MS: u64 = 2000;

/// 点级字段有效/降级标志（渲染端据 flag 决定显示数值或 "--"+ 对应角标）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldFlag {
    Valid,                 // 正常展示
    NotRead,               // "未取数"(点表/采集未覆盖, PRD 6.4)
    Offline,               // "源离线"(PCS 离线/核间读失败, PRD 6.1)
    RangeError,            // "数据异常"(域值化量程/有限性校验不过, PRD 6.5)
}

/// 单数值字段
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Field {
    pub v: Option<f64>,   // 工程值；flag!=Valid 时通常 None（或保留哨兵供调试）
    pub flag: FieldFlag,
}

/// 运行状态（F2，主判据 REG1013）。枚举化保证值域 0..=3：
/// **越界态在帧内不可达**——采集侧心跳已将 1013 越界读数按坏读数滤除（改判离线），渲染侧
/// `Option<RunState>` match 穷尽 Stop/Standby/Charge/Discharge + None 即可
/// （对 PRD §6.5「1013 越界→数据异常」的落地说明：run_state 不入 RangeError 分支，越界在数据侧收敛）。
/// JSON 以判别数 u8 传输（serde_repr 派生或手写 u8 映射），如 "run_state": 2。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum RunState { Stop = 0, Standby = 1, Charge = 2, Discharge = 3 }

/// SOC 展示源标注（三态，对齐 UI 源标签）：Bms / PcsReg1010 / Lost(=双源皆失「失效」)。
/// **不含「双源一致」态**——SOC 源裁决是「优先级+回落」的**单源化**（AiIntegrator
/// `resolve_soc_source`：BMS fresh → BMS，否则活读核间 → PCSReg1010，双失 → Lost），任一时刻
/// 实际取值源唯一，从不双读并列比对，故「一致」在控制面不可达/无判定输入。详见本节末
/// 「对 PRD F1.2『双源一致』态的落地解释」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocSource { Bms, PcsReg1010, Lost }

/// 一帧展示数据（每 1s 由 mupcd 域值化后发布）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DisplayFrame {
    pub version: u8,          // = PROTO_VERSION
    pub seq: u64,             // 单调递增发布序号（重启清零；渲染端判连续/重排）
    pub ts_ms: u64,           // 域值化时刻（Unix 毫秒；新鲜度判据 F5.3）
    /// F1: 裁决后 SOC(%)
    pub soc: Option<f64>,     // None ↔ soc_source=Lost（双源皆失/无 fresh 源）→ 屏显 "--"+"SOC 源失效"，禁沿用旧值
    pub soc_source: SocSource, // 三态源标注；渲染据此画源胶囊（Lost → "SOC 源失效" 警示色）
    pub soc_flag: FieldFlag,  // 所用源点级异常(如 PCS 源 RangeError)辅助
    /// F2: 运行状态（1013 主判据；值域保证 0..=3，见 RunState）
    pub run_state: Option<RunState>, // None = PCS 离线/心跳无有效态（PRD 6.1）
    /// 核间 modbus 链路在线（心跳维护；PCS 离线整体提示 6.1）
    pub pcs_online: bool,
    /// F3: 三相有功(kW) + 设备总有功(kW)——已 ×0.1，渲染端不再换算
    pub p_phase: [Field; 3],  // [A,B,C]
    pub p_total: Field,
    /// F4: 三相电流(A)——已 ×0.1
    pub i_phase: [Field; 3],  // [A,B,C]
    /// 6.6 佐证一致性: 1013(充/放) 与 Σp_phase 方向显著反向 → true（渲染端加"方向不一致"角标，主状态仍以 1013 展示）
    pub inconsistency: bool,
}
```

JSON 示例（开发直接按此造桩/对齐）：

```json
{
  "version": 1,
  "seq": 123,
  "ts_ms": 1757_412_000_000,
  "soc": 65.0,
  "soc_source": "pcs_reg1010",
  "soc_flag": "valid",
  "run_state": 2,
  "pcs_online": true,
  "p_phase": [ {"v": 12.3,"flag":"valid"}, {"v": 11.8,"flag":"valid"}, {"v": 12.0,"flag":"valid"} ],
  "p_total": {"v": 36.1, "flag": "valid"},
  "i_phase": [ {"v": 22.5,"flag":"valid"}, {"v": 22.1,"flag":"valid"}, {"v": 22.3,"flag":"valid"} ],
  "inconsistency": false
}
```

> **对 PRD F1.2「双源一致」态的落地解释（回应评审 F1.2）**
>
> PRD F1.2 要求「SOC 源标注随帧同更新」。若照字面易误读为「需两路源（BMS / 核间 PCS）同时读到并
> 比对外能显示一个『双源一致』角标」。**落地裁定：不引入该态**，理由如下：
>
> - 控制侧 SOC 是**单源裁决**而非双读一致性——`AiIntegrator::apply_soc_source` 调用纯函数
>   `resolve_soc_source`（strategy-engine `ai_integration.rs`）按**「优先级+回落」**：BMS fresh →
>   采用 BMS；BMS 超期/无 → 无条件活读核间 `latest_soc`(REG1010) 采用核间值；仅当两路皆失
>   （`is_dual_source_lost`）才判 Lost。任一时刻写入 `battery.soc` 的**只有唯一一个源的值**，
>   从不两源并列、从不做「一致/不一致」比对。
> - 因而「双源一致」在控制面**不可达、无判定输入、无展示意义**——帧 `soc_source` 若含「一致」
>   取值将永不可能产生，属死代码。
> - 展示侧据此把源标注收敛为**三态 `Bms / PcsReg1010 / Lost`**，与帧 `SocSource` 枚举一一对应；
>   `Lost` 即「SOC 源失效」警示胶囊（PRD 6.2/F1.4 双源皆失降级）。UI 源标签区**不设「双源一致」胶囊**。
> - F1.2「随帧同更新」由本帧 `soc_source` 逐帧携带满足：单源可用（BMS 或 PCS）时显示**正在用的源名**
>   源胶囊（§6.3 F1 行），与 §4.3 `soc_display_snapshot` 收敛出的唯一裁决源保持一致。

### 3.4 新鲜度/超时语义（渲染端规则，定稿）

- 每帧带 `ts_ms`、逐字段 `flag`（PRD F5.4 的点级 valid 即 `FieldFlag`）。
- **数据过期**：`now − ts_ms > stale_ms`（默认取 display-proto 常量 `DEFAULT_STALE_MS=2000`，渲染进程可用 `--stale-ms` 覆盖）→ 全局「数据过期」角标；数值保留最近有效帧展示但**不冒充实时**（PRD F5.3，冻结+打标）。
- **点级独立降级**：单点读失败只置该字段 `flag`，不影响其余字段刷新（F5.5）。
- **通道断（6.3）**：连续 `roundtrip 失败` 或 `无成功 GET > 3000ms` → 切「与主进程数据通道断开」整屏态（可保留最近帧暗化+冻结标）；通道恢复后下一次成功 GET ≤500ms 回实时。
- **禁止**：源失效字段补 0 / 沿用陈旧值冒充实时 / 用功率符号自判充放（§8）。

### 3.5 发布方（mupcd）时序

`publish_ms`（默认 1000，标称 1Hz，可容抖 ±30%）触发一次采集+组帧。发布用 `Arc<Mutex<Option<DisplayFrame>>>` 存最新帧；HTTP 每请求 clone 该帧返回。**不在 HTTP 路径做任何 modbus 读**（采集在专用 task，避免并发总线抖动）。

---

## 4. mupcd 侧改动

### 4.1 intercore：扩展读 1022–1031（+1032）

**pcs.rs 增补常量**：

```rust
pub const REG_I_A:    u16 = 1022; // 3区 输出电流 A相 *0.1A (Int16)
pub const REG_P_A:    u16 = 1029; // 3区 输出有功 A相 *0.1kW (Int16)
pub const REG_P_TOTAL:u16 = 1032; // 3区 设备总有功 *0.1kW (Int16)
pub const SCALE_3PH:  f64 = 0.1;  // 电流/有功统一 0.1 量纲
```

**ModbusRtuTransport 增加读取（读函数 + 复用既有 read_input/bus 锁体系）**：

```rust
/// FC04 读 3 相电流(1022 起 3 字) 与 3 相有功+总有功(1029 起 4 字)。
/// 两段连续读（1025-1028 为表中未命名寄存器，不赌整段 1022..=1032 是否实现，
/// 保守按点表连续子段两笔读）。成功/失败副作用经 read_input 维护在线/离线。
pub async fn read_three_phase(&self) -> Option<PcsThreePhaseRaw> { ... }
/// 返回已解码原始 i16 读数（未乘量纲），域值化在上层 DisplayDataProvider 完成
```

- **为何不并入 SOC/心跳读**：SOC(1010)/心跳(1013) 是控制链路每拍活读，频率与存在性受 dispatch/联锁影响；显示采集需**独立于联锁抑制**持续 1Hz 采样，故由 DisplayDataProvider 自己的 1s task 发起（与心跳同走 `bus` 锁串行，半双工无交错——沿用 W3 互斥，单笔约 ≤30ms@19200）。
- **周期/整合**：不与心跳 SOC 读合并（职责/时序耦合会引入"联锁抑制期间显示冻结"与"心跳改动影响控制"两类回归）；仅**共享** transport 层 `read_input` 的锁与在/离线副作用。

**`IntercoreTransport` trait 扩展**（默认实现返回 None，避免污染既有 Tcp/sim 路径）：

```rust
#[async_trait]
pub trait IntercoreTransport: Send + Sync {
    // ...既有方法...
    /// 三相展示读数（Modbus 实现有效；Tcp/sim 无 PCS 3 区点表，返回 None → 上层打 NotRead）
    async fn read_three_phase(&self) -> Option<PcsThreePhaseRaw> { None }
}
```
`IntercoreClient` 加转发方法 `pub async fn read_three_phase(&self) -> Option<PcsThreePhaseRaw>`。

### 4.2 数据汇聚组件：独立 DisplayDataProvider（不塞进 AiIntegrator）

归属 `mupc-core-bin/src/display_host.rs`（新模块，bin 内）；持：
- `Arc<AiIntegrator>`（取裁决后 SOC 快照，见 §4.3）
- `Arc<IntercoreClient>`（读三相、查 `last_run_state()`、`is_connected()`）
- `display_proto::config::DisplayConfig`（发布周期/回环端口/量程；定义真源见 §7.1）

**采集循环（1s）伪码**：

```
loop { tick(1s)
  snap   = ai_integrator.soc_display_snapshot().await          // SOC 唯一裁决入口
  run    = client.last_run_state()                              // 1013（心跳维护, 离线 None）
  online = client.is_connected().await                          // 核间链路
  three  = client.read_three_phase().await                      // 1022..1032
  pcs_online = online && run.is_some()                          // 简化：有在线链路且心跳出过状态
  frame  = build_frame(snap, run, three, now_ms)                // 域值化+量程+打flag+一致性6.6(见§4.4)
  latest.store(frame)
}
```

**启动装配点**：`startup.rs` 在「策略引擎(第8步) + 决策循环 spawn」之后、`register_service` 列表内新增一步，条件 `config.display.enabled`：
- 先 `tokio::spawn(DisplayDataProvider::run(...))` 采集发布；
- 再 `tokio::spawn(LoopbackHttpPublisher::serve(bind))` 回环 HTTP；
- 两个句柄都 push 进现有 `guard.0`（随优雅退出一并 abort）。
- 主进程**不 spawn/不管理渲染子进程**（渲染生命周期归 systemd，见 §11；4.3.1 归属系统集成侧）。

### 4.3 AiIntegrator：SOC 唯一裁决点收敛 + 展示快照（不在渲染端重判）

为避免「控制每拍 apply_soc_source 判一次、显示又判一次」的分叉，把裁决收敛到**一个私有入口**，控制与展示共用：

```rust
/// AiIntegrator 内新增结构
pub struct SocResolved {
    pub value_pct: Option<f64>, // 裁决后展示用 SOC(0..100)
    pub source: SocSourceKind,  // 内部源枚举 { Bms, PcsReg1010, None }（None=无 fresh 源）；映射到帧三态 SocSource：Some→同名，None→Lost（见 §3.3）
    pub dual_lost: bool,        // = is_dual_source_lost(...)：双源皆失(沿用冻结/纯无SOC)
}
async fn resolve_soc_core(&self, data_soc: Option<f64>) -> SocResolved {
    // BMS cache + (BMS stale? 活读 client.latest_soc() : None) + existing=data_soc
    // 统一调用既有纯函数 resolve_soc_source / is_dual_source_lost（不改裁决逻辑）
    // 返回 value/source/dual_lost，并写 self.soc_resolved_cache（RwLock, TTL≈900ms）
}
```

- `apply_soc_source(&mut data)` **重构**：`let r = self.resolve_soc_core(data.battery.soc).await; data.battery.soc = r.value_pct;` + 沿用双源皆失节流 warn。控制语义不变（保留冻结值驱动 soc_protect）。
- 新增公开方法（供 DisplayDataProvider）：
  ```rust
  pub async fn soc_display_snapshot(&self) -> SocResolved;
  ```
  内部：读 `soc_resolved_cache`，若未过期(<900ms)直接返回（避免与 dispatch 同 tick 双活读 REG_SOC）；否则调 `resolve_soc_core` 刷新。
- **展示规则**（对 PRD §6.2/F1.4 落定口径）：`value_pct=None 或 dual_lost=true` → 帧 `soc=None, soc_source=Lost` → 渲染端 `--` +「SOC 源失效」（警示色，语义对齐 `SocSource::Lost`），**沿用冻结值仅在控制侧内部，不送上屏**（杜绝把旧值冒充实时）。

### 4.4 域值化与 6.6 一致性（mupcd 职责，渲染端不重算）

| 量 | 域值化（mupcd 内） | 量程/有效校验（越界 → flag=RangeError） |
|----|--------------------|-----------------------------------------|
| 三相电流 | `raw_i16 * 0.1` → A | 有限且 `|v| ≤ display.range.current_max_a(默认 300)`；decode 失败/未取到 → Offline；transport 不支持 → NotRead |
| 三相有功 | `raw_i16 * 0.1` → kW | 有限且 `|v| ≤ display.range.phase_power_max_kw(默认 100)` |
| 总有功 | `raw_i16 * 0.1` → kW | 有限且 `|v| ≤ display.range.total_power_max_kw(默认 300)` |
| SOC | 来自 AiIntegrator 裁决 | 非有限/越 0..100 由 resolve 天然规避 |
| run_state | 心跳 last_run_state | 0..=3（枚举化 `RunState`，值域语义保证；越界读数由采集侧心跳按坏读数滤除改判离线，**不入帧**，无 RangeError 分支——PRD §6.5） |

- **6.6 一致性**：当 `run_state∈{2(充),3(放)}` 且 `Σp_phase` 与预期方向显著反向（**追认前不按功率符号定充放，仅作佐证**）且 `|Σp| > display.range.inconsistency_threshold_kw(默认 3.0, =额定 60kW 的 5%)` → `inconsistency=true`；其余为 false。渲染端主状态仍以 `run_state` 呈现，另加角标。

---

## 5. 渲染进程设计

### 5.1 crate 划分与权衡

新增 **两个** workspace crate（权衡：不过度拆分，也不让渲染进程反向依赖重型服务 crate）：

| crate | 类型 | 依赖 | 职责 |
|-------|------|------|------|
| `crates/display-proto`（`mupc_display_proto`） | lib | serde/serde_json | 帧协议类型（§3.3）**+ DisplayConfig（§7.1）**；跨进程两侧 + 测试桩共享的契约单一真源。 |
| `crates/local-display`（`mupc_local_display`） | **lib + bin** | display-proto、ab_glyph、serde_json、serde（自身 CLI 配置 derive）；（可选 png 仅 dev/feature） | 渲染进程本体。lib 暴露可测模块（canvas/layout/font/channel/screen 逻辑），bin = 可执行 `mupc-local-display`。**依赖仅 display-proto 所需**——不引入 serde_yaml、不读 `mupc_core_config.yaml`（§7.2）。 |

> 渲染进程**不建议直接放核心仓库已有 crate**：它必须保持「零依赖核心业务、可在无串口/无 NPU/无策略堆栈的最小环境独立构建与测试」，独立 crate 同时满足进程隔离与构建解耦。
> mupcd 侧（DisplayDataProvider / LoopbackHttpPublisher）不加新 crate，放 `mupc-core-bin` 模块即可（避免第三个 crate 的维护面）。

### 5.2 模块划分

```
crates/local-display/src/
├── lib.rs      // pub mod 汇总；对外暴露 render_frame() 等纯函数供测试
├── main.rs     // bin 入口：读 CLI 参数/默认配置 → 初始化 Screen → 主循环
├── config.rs   // 渲染侧 CLI 参数子集（channel url、interval、stale_ms、backend/fbdev、width/height、font），默认值取 display-proto 常量，不读 core yaml
├── channel.rs  // DisplayChannelClient：阻塞 GET 最新帧（TCP回环，无 TLS）
├── state.rs    // DisplayState：最近帧 + 新鲜度/过期/通道态派生（纯逻辑，可单测）
├── canvas.rs   // PixelBuffer(1024x768x4)；set/rect/text-ready 绘图原语；to_png(dev)
├── font.rs     // 捆绑 CJK 子集 .otf (include_bytes!) → ab_glyph → 启动栅格化图集
├── layout.rs   // 固定网格布局 + 语义色板 + 各 PRD F1..F5 区域绘制（读 state 画帧）
├── screen.rs   // trait Screen{ blit(&[u8]); } + FbdevScreen/DrmScreen/OffscreenScreen
└── run.rs      // 主循环编排（fetch→render→blit→sleep），节拍/自恢复/统计
```

### 5.3 主循环（阻塞同步单线程，节拍 500ms）

```
loop {
  now = Instant::now()
  frame = channel.fetch_latest()            // 阻塞 GET, 2s 超时；失败记一次
  state.update(frame / last_ok / now)       // 更新 seq/ts、过期、通道态
  if state.should_redraw() {                // 有变化 或 阈值态翻转 或 强制2Hz
      canvas = layout.render(&state)        // 离屏整帧(≤30ms 预算)
      screen.blit(canvas)                   // fbdev/drm 提交 / offscreen 记录
  }
  sleep_until(next_500ms_tick)              // 无忙等(PRD 4.1.3)
}
```
- **通道态派生**（state 纯逻辑单测）：
  - `DataFresh`（最新帧 ts 距今 ≤ stale_ms）；
  - `DataStale`（> stale_ms 打「数据过期」角标，保留数值）；
  - `ChannelDown`（无成功 GET ≥ 3000ms → 6.3 整屏态）；
  - `ChannelInit`（尚未首次成功 GET → 「初始化中」占位，PRD 4.3.4）；
  - mupcd 恢复 → 首次成功 GET 即回实时（≤500ms）。
- **崩溃自恢复（本进程内 + 系统级双保险）**：
  - 本进程：任何 panic 由 `main` 捕获级外层兜底打印后退出码非 0 → 交给系统级重启（不吞 panic、不自循环空转）。
  - 系统级：systemd `mupc-display.service` `Restart=always, RestartSec=1`（≤3s 拉起，见 §11）。**渲染进程不写 PCS/核间/主进程**，崩溃不影响 mupcd（进程隔离，PRD 4.3.2）。

### 5.4 字体渲染（font.rs）

- 数据：`include_bytes!("../fonts/NotoSansSC-subset.otf")`（或 fallback 到启动参数 `--font` 指定的外部文件/系统字库，见 §7.2）。
- 启动一次用 `ab_glyph` 把所需字形栅格化进 `GlyphAtlas`（含字号多档：标题/大数值/正文/角标），后续绘制查图集 blit，避免每帧 TTF 解析。
- ASCII 数字/符号（`0-9 . - % / : kW A B C`）与中文字形同源子集化。

---

## 6. 画布布局结构与 PRD 映射

### 6.1 布局原则（对应 PRD 4.2）

- 1024x768 一屏放全，无滚动/分页/交互层；字段位置固定（改值不改布局）；状态「文字 + 语义色 + 图标」三重冗余。
- 静态装饰做防烧屏低影响处理（如非数值区整体周期性 ±几像素微移或反色节拍），**核心数值区不抖动**（4.2.4）。

### 6.2 分区网格（与 UI §4.3/§5.3 唯一对齐，UI 为视觉权威；单位 px）

```
+------------------------------------------------------------------------------------------+
| (16..1008 x 16..72) 页眉条: "MUPC · 台区储能装置运行状态"     [时钟] [●实时 · 通道已连接] |
+----------------------------------------+------------------------------------------------+
| SOC 主区 (16..496 x 88..420)           | PCS 运行状态主区 (528..1008 x 88..420)          |
|   "储能电池 SOC"  [源标签 BMS/PCS/失效] |   "PCS 运行状态"  [REG1013]                     |
|   148px 大数字 65 % + 0-100 分段量程条  |   大字 充电/放电/待机/停机 + 语义色图标+卡描边   |
|   (0-15红 / 15-85青 / 85-100橙)        |   佐证行: 方向一致 ΣP +12.5 kW(不一致→品红角标)  |
+----------------------------------------+------------------------------------------------+
| 三相功率与电流区 (16..1008 x 444..752)：四卡横排 A/B/C/总（每卡内上 P 下 I；总卡无电流行）   |
|   A 卡 (32..260)   B 卡 (276..504)   C 卡 (520..748)   总卡 1032 (764..992)             |
+------------------------------------------------------------------------------------------+
```

- **弃早期「P 横带 / I 横带」两行分离表述**：三相 P 与 I **同卡**（卡内上「有功功率」行、下「电流」
  行），A/B/C/总四卡横排——与 UI §4.3 定案唯一对齐。
- 坐标/字号基准取自 UI（A/B/C/总 卡 228 宽、16 间隙、卡区 Y 508..736，P 值 64px / I 值 44px），
  开发期在 `layout.rs` 顶部常量微调、允许 ±10% 视觉对齐，**分区与三态语义不变**。
- 页眉文案统一为 UI 版「台区储能装置运行状态」；各字段右下/右上角标语固定（就地读屏便于远程
  复述，PRD §2）。

### 6.3 PRD F1–F5 → 绘制/数据映射表

| PRD | 数据（帧字段） | 绘制 | 降级显示 |
|-----|---------------|------|----------|
| F1 SOC | `soc`/`soc_source`/`soc_flag` | 大字号 % + 0-100 分段量程条；`≤15%` 红段 / `≥85%` 橙段警示——15/85 为**展示警示档**（PRD F1.3，仅驱动 UI 断点/描边），与安全配置 soc_min/soc_max 0.10/0.90（**控制硬限**，驱动 soc_protect）作用域不同、不冲突 | `soc=None`（双源皆失/源失效 6.2）→ `soc_source=Lost` → 数值 `--` +「SOC 源失效」警示色 |
| F2 运行状态 | `run_state`/`inconsistency` | 文字(停/待/充/放)+语义色+图标：充=绿、放=蓝、停机=灰、待机=黄（色板可微调但四态可区分）；主判据恒为 run_state，**不**以功率符号判充放 | `run_state=None`(离线)→「PCS 离线」灰；1013 越界**不入帧**（采集侧滤除改判离线；渲染 match 穷尽 Stop/Standby/Charge/Discharge+None） |
| F3 三相有功 | `p_phase`/`p_total`（已 ×0.1 kW） | 三相+总计，1 位小数，按 F2 方向着色 | 各相 `flag=Offline/NotRead/RangeError` → 该相 `--`+对应角标，禁显 0.0 |
| F4 三相电流 | `i_phase`（已 ×0.1 A） | 三相，1 位小数 | 同上 |
| F5 刷新/新鲜度 | 帧 `ts_ms`/`seq`/逐字段 `flag` | 数值变动即上屏；点级独立降级 | 全局过期角标；通道断 → 6.3 整屏态；恢复 ≤1s 回实时 |

---

## 7. 配置项

> 评审修订定稿：DisplayConfig 归属 display-proto（单一真源），渲染进程不读 core 配置（详见下）。

### 7.1 字段定义单一真源：display-proto crate

`DisplayConfig`（发布周期 / 回环端点 / 域值化量程）**结构定义放 display-proto**：
`crates/display-proto/src/config.rs`（lib 内 pub 导出）。理由：mupcd（core-bin）与渲染进程
（local-display）都依赖该结构形态，放 display-proto 单一真源——既避免 core-bin 反向依赖/维护
重型配置 crate，也避免 `mupc_core_config.yaml` 的 display 段结构在两侧各写一遍。

```rust
// crates/display-proto/src/config.rs（示意；字段与 yaml display: 段一一对应）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub enabled: bool,            // true = mupcd 起 DisplayDataProvider + 回环发布
    pub bind_addr: String,        // 回环端点（默认 DEFAULT_BIND "127.0.0.1:9810"）
    pub publish_ms: u64,          // mupcd 采集/组帧/发布周期（默认 1000）
    pub range: DisplayRange,      // 域值化量程（mupcd 消费；PRD §6.5 越界判 RangeError）
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DisplayRange {
    pub current_max_a: f64,               // 默认 300
    pub phase_power_max_kw: f64,          // 默认 100
    pub total_power_max_kw: f64,          // 默认 300
    pub pcs_total_rated_kw: f64,          // 默认 60（6.6 一致性阈值基准）
    pub inconsistency_threshold_kw: f64,  // 默认 3.0 = 60kW*5%（PRD F2 验收3）
}
// 共享默认常量（DEFAULT_BIND / DEFAULT_CHANNEL_URL / DEFAULT_STALE_MS=2000 / DEFAULT_PUBLISH_MS=1000）随 proto 提供。
```

### 7.2 谁解析、谁消费（配置契约）

| 载体 | 解析方 | 消费方 | 承载字段 |
|------|--------|--------|---------|
| `mupc_core_config.yaml` 的 `display:` 段 | **mupcd（core-bin）**——`core_config.rs` 顶层 `CoreConfig` 追加 `#[serde(default)] pub display: display_proto::config::DisplayConfig`（反序列化为 display-proto 结构，**非重复定义**；`validate()` 校验） | mupcd 的 DisplayDataProvider / LoopbackHttpPublisher | `enabled` / `bind_addr` / `publish_ms` / `range`（域值化量程） |
| 渲染进程启动参数（CLI） | **mupc-local-display**（`main.rs` 解析；默认值取 display-proto 常量） | 渲染进程自身 | `--channel <url>`、`--interval 500`、`--stale-ms 2000`、`--backend fbdev\|drm\|offscreen`、`--fbdev-path`、`--width/--height`、`--font <path 或空=捆绑子集>` |

**渲染进程不读 `mupc_core_config.yaml`**（KISS）：渲染端零核心配置依赖——不引入 serde_yaml、
不与 mupcd 的 `validate()`/量程逻辑耦合。其渲染参数（分辨率/字体路径/刷新/通道）一律经
**启动参数**或**通道帧/共享常量**给定。通道端点一致性：proto 默认 `DEFAULT_CHANNEL_URL`
与 mupcd 默认 `bind_addr` 同指一回环端点；生产 unit 显式传 `--channel` 与 core yaml `bind_addr`
对齐（部署清单一条，见 §11）。

### 7.3 mupcd 侧 yaml 段（已移除原 render/driver/font 渲染子段——渲染不再读 core 配置）

```yaml
display:                      # 本地显示终端发布侧（默认整段缺省 = disabled，行为不变）
  enabled: false              # true = mupcd 起 DisplayDataProvider + 回环发布
  bind_addr: "127.0.0.1:9810" # 数据通道回环端点（仅本机）
  publish_ms: 1000            # mupcd 采集/组帧/发布周期(≥1Hz 标称)
  range:                      # 域值化量程(PRD §6.5 越界判 RangeError；§8 追认前宽口径)
    current_max_a: 300
    phase_power_max_kw: 100
    total_power_max_kw: 300
    pcs_total_rated_kw: 60    # 6.6 一致性阈值基准
    inconsistency_threshold_kw: 3.0   # = 60kW*5%（PRD F2 验收3）
```

`core_config.rs::validate()` 增加：`display.enabled && (transport != "modbus_rtu")` 时 warn 但不阻止（仿真可看 SOC/通道，三相将 NotRead）；`bind_addr` 非回环报错（强制仅 127.0.0.1）。

---

## 8. 边界与异常处理对照（PRD §5/§6 全量落点）

| # | 场景 | 数据侧(mupcd DisplayDataProvider) | 渲染端展示 |
|---|------|----------------------------------|-----------|
| 6.1 | PCS 离线（核间读失败） | `pcs_online=false`；三相各 `flag=Offline`、`run_state=None`；SOC 若有另一源(BMS) 则帧带 BMS 裁决值 | 状态区「PCS 离线」；F3/F4 各相 `--`+「源离线」；F1 有 BMS 则显示 BMS 值并标源 |
| 6.2 | SOC 双源皆失 | `soc_source=Lost, soc=None`（冻结值只在控制内部） | SOC 区 `--`+「SOC 源失效」警示色；不补 0、不沿用旧值 |
| 6.3 | 通道断（mupcd/IPC 异常） | 回环端口消失 | 渲染端 ≤3s 切「与主进程数据通道断开」态（可留最近帧暗化+冻结标）；恢复 ≤1s 回实时 |
| 6.4 | 点表未覆盖/未接入(1022-1032 未采集) | transport 不支持/未读 → 三相 `flag=NotRead` | `--`+「未取数」 |
| 6.5 | 域异常(越界/非有限) | 量程校验 → `flag=RangeError` | `--`+「数据异常」，不插值 |
| 6.6 | 1013 与功率方向显著相反 | `inconsistency=true` | 主状态仍 1013，另加「方向不一致」角标 |
| 6.7 | 渲染进程自身异常/启动失败 | 不阻塞主进程（进程隔离） | 黑屏/占位；systemd Restart ≤3s 拉起 |

> 「不造假值」总原则落点：所有数值展示仅当对应 `flag==Valid`；`--` 永不显示为 `0.0`；角标文案集中定义在 `layout.rs`（与 font.rs 码表同步）。
> **对 PRD §6.5「1013 越界→数据异常」的落地说明**：越界只发生在采集侧（心跳读到 0..=3 之外按坏读数
> 滤除、改判离线），故帧内 `run_state` 值域语义保证合法、**RangeError 不可达**；渲染侧对 `RunState`
> 枚举 match 穷尽四态 + `None`，不存在「枚举外值→数据异常」分支（与 F3/F4 数值字段的 6.5 RangeError 不同）。

---

## 9. 非功能预算落实

| NFR(PRD §4) | 预算 | 设计落实 |
|--------------|------|----------|
| 视觉刷新 ≥1Hz；单帧 ≤30ms | 1024x768 | 渲染节拍 500ms(2Hz)；离屏整帧自绘 + 单次 blit 应 ≪30ms（真机首测校准，见 §13）；启动后 `--metrics` 打印平均 draw ms |
| 渲染 CPU ≤15% 单核均值(1s窗) | 上限 | 同步阻塞轮询 + sleep，无忙等；只在需要时重绘；字符走图集 blit，无每帧 TTF 解析 |
| 常驻内存 ≤64MB | 上限 | 离屏 1024x768x4≈3MB + 图集/子集字库(≤~1MB) + 无 GUI 栈；释放每帧中间产物；预估 ≪32MB |
| 通道断满载 CPU ≤5% | 上限 | 断连 = 每次 GET 连接失败(ms 级) + sleep，其余空闲 |
| 崩溃 ≤3s 自恢复 | 3s | systemd Restart=always + RestartSec=1（进程隔离，不影响 mupcd） |
| 渲染进程异常不得影响 mupcd | — | 独立进程 + 独立 crate，只经回环 GET 单向读 |

---

## 10. 测试策略

### 10.1 无真屏环境验证路径（核心）

- **屏幕后端抽象**：`screen.rs` 的 `offscreen` 后端把每帧渲染写入内存 Canvas → 可选导出 PNG（dev feature）。渲染进程 `--backend offscreen --channel http://127.0.0.1:<stub>/v1/display/latest` 即可在 **Windows x86 / Linux CI 无屏**全链路跑通。
- **数据源 stub**：以 `display-proto` 构造固定帧的小 HTTP 桩（测试辅助 bin 或测试内 tokio 起服），对渲染进程注入 6.1–6.7 各态帧。

### 10.2 分层

| 层 | 用例 | 断言 |
|----|------|------|
| display-proto | serde 往返 / 字段默认 / 版本 | JSON 解码=编码；未知字段容忍 |
| intercore | `read_three_phase` 解码 1022-1032（字节互换/i16×0.1）、坏读数/越界判 Offline/RangeError | 向量断言 |
| AiIntegrator(SOC) | `resolve_soc_core`/`soc_display_snapshot`：BMS fresh→BMS、BMS stale→活读核间→PCS、双失→dual_lost、缓存 TTL | 复用既有纯函数测试风格 |
| DisplayDataProvider(host) | 用 stub `Arc<dyn IntercoreTransport>` + 真实 AiIntegrator：各源在/离线/量程外 → 帧 flag 正确；`inconsistency` 判定 | 帧字段断言 |
| 渲染进程(offscreen) | 对 6.1–6.7 帧渲染：各区域非空像素出现/更新、`--`/角标与语义色落区正确；**帧到→上屏耗时 <30ms**（离屏计时断言） | 区域像素直方图 + 计时 |
| 崩溃恢复 | 进程被 kill → 外层 watchdog/systemd 拉起（目标环境 shell 脚本断言重启时间 ≤3s） | 集成(需真机/CI 容器) |
| 真机(标记需硬件) | `/dev/fb0` 映射 HDMI、fb 像素格式(bpp/order)、DRM 主平面、19200 波特两段读通过 | 人工/脚本 |

### 10.3 冒烟回归
- `cargo test --workspace --exclude mupc-iec61850-plugin --exclude rs485-plugin --exclude device-trait`（新增 crate 默认纳入）。
- 渲染进程用 `offscreen` 跑 `--smoke` 一键自检（组一帧 → 出 PNG → 打印时序）并入构建脚本。

---

## 11. 交叉编译与部署

### 11.1 构建（渲染进程为纯 Rust）

- 无需 CMake/RKNN/字体 C 依赖；仅需目标 aarch64 链接器：
  ```bash
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
  cargo build -p mupc-local-display --release --target aarch64-unknown-linux-gnu
  ```
- 落点（本期**不实现**，仅标注）：
  - `deploy/scripts/build-for-rk3588.sh` 增加 `--display` 子模式或在该脚本产出清单追加 `mupc-local-display` 与 `display-proto`；
  - 或新增 `deploy/scripts/build-display.sh`。
- display-proto 为库，随 host（mupcd）与渲染进程各自编译引用。

### 11.2 部署（systemd，渲染进程自恢复）

新 unit `deploy/systemd/mupc-display.service`（本期仅给出模板方向，不提交落地文件）：

```ini
[Unit]
Description=MUPC 本地显示终端渲染进程
After=multi-user.target          # 不强依赖 mupcd（先行可显示"初始化中"）

[Service]
Type=simple
User=mupc
Group=mupc
SupplementaryGroups=dialout video render   # fb/dri 访问；按真机组名校准
ExecStart=/opt/mupc/bin/mupc-local-display \
    --channel http://127.0.0.1:9810/v1/display/latest \
    --backend fbdev --fbdev-path /dev/fb0 --width 1024 --height 768
Restart=always
RestartSec=1                      # ≤3s 自恢复(PRD 4.3.1)
# 允许访问帧缓冲/DRM（按真机加固策略放宽 /dev/fb0 /dev/dri）
# DeviceAllow=char-framebuffer rw   # systemd 语法按目标版本，部署期核对
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

- mupcd 主进程**不拉起/不拥有**渲染子进程（4.3.1 归属系统集成侧）；渲染进程上电可先显示「初始化中」，通道就绪 ≤1s 切实时（4.3.4）。
- 部署文件（字库已在二进制内）：安装产物 `/opt/mupc/bin/mupc-local-display` + systemd unit（上表 CLI 参数）；
  **渲染进程不装/不读 `mupc_core_config.yaml`**。该 yaml 的 `display:` 段（enabled）仅属 mupcd 侧发布配置；
  生产 unit 的 `--channel` 需与 `display.bind_addr` 对齐（一条部署核对项）。

---

## 12. 技术决策记录(ADR)与选型理由

| 决策 | 结论 | 理由（KISS/现实） |
|------|------|------------------|
| D1 数据通道 | 本地 HTTP 回环 `127.0.0.1` GET 最新帧，轮询 500ms | 状态在服务端取快照→允许丢帧只取最新；断连=连接失败判定最简；TCP 回环 Windows/Linux 同语义便于无真屏测试；不并入 web-api 避免北向暴露与生命周期耦合 |
| D2 渲染栈 | 纯 framebuffer 自绘 + ab_glyph(纯 Rust) + 捆绑 CJK 子集字体 | 内容=固定网格+有限中文词，无需 GUI 栈；零 C 依赖交叉一次过；内存/CPU 最省；offscreen 后端支撑无屏验证 |
| D2b 字体资源 | 捆绑 OFL 子集 OTF（include_bytes 进二进制）；预留外部/系统字库覆盖 | 不赌目标镜像带 CJK 字库（B4 不可靠）；可复现、镜像无额外文件 |
| D3 crate 策略 | 新增 display-proto(lib) + local-display(lib+bin)；DisplayDataProvider 放 mupcd(core-bin 模块) | 渲染进程保持零核心依赖、可独立构建测试；协议单一真源供桩复用；不过度拆 crate |
| D4 SOC 数据源 | 收敛到 AiIntegrator 单一裁决入口（resolve_soc_core），控制与展示共用；渲染端不判源 | PRD「不各自重判」；避免控制/显示分叉读 REG_SOC |
| D5 三相读取 | DisplayDataProvider 独立 1s 采集（两段 FC04 连续读），不并入心跳 | 与联锁抑制/心跳职责解耦；共享 bus 锁互斥即可 |

---

## 13. 开发前置风险 / 待验证项

> 以下为真机/厂方侧无法在设计期敲定、需**开发阶段首验或厂方追认**的项；均不阻塞 F1–F5 核心数值展示编码（对应项已按 PRD 降级口径落地）。

1. **（真机）`/dev/fb0` 是否映射到 HDMI 输出及其像素格式/位深/字节序**；否则需 DRM(`/dev/dri/card0`) dumb-buffer 主平面后端（已留 `driver=drm` 切换）。→ 驱动层真机首验。
2. **（真机）1025–1028（1022–1032 间未命名寄存器）是否可整段 FC04 读**；设计默认两段连续读规避，单段读可行性首验后可按需合并省一帧总线往返。
3. **（厂方追认）1022–1024/1029–1031 读回极性与设定侧一致（正=放/负=充）**；追认前 F3/F4 方向一律取 F2(1013) 状态机，功率正负仅佐证（PRD §8）。不阻塞。
4. **（真机）RS485 总线吞吐余量**：心跳(1013)1s + dispatch 活读(1010)1s + 显示两段读 1s + 下发写，@19200 波特是否仍有 <1s 决策余量；若紧可在 provider 合并读。
5. **（已定案）SOC 警示档与安全档分域共存**：展示警示色档取 **15%/85%**（PRD F1.3，仅驱动 UI 量程条断点/描边/数值色）；安全配置 soc_min/soc_max **0.10/0.90** 为控制硬限（驱动 soc_protect）。二者用途不同、不冲突——一句区分见 §6.3 F1 行。
6. **（真机）HDMI 分辨率协商**：若面板非 1024x768 原生需 fb/drm 缩放或改渲染 CLI `--width/--height`（默认 1024x768，见 §7.2）。
7. **（部署）渲染进程访问 /dev/fb0、/dev/dri 的用户/组权限与加固策略**（真机校准后定 unit 的 DeviceAllow/SupplementaryGroups）。
8. **字库**：文泉驿微米黑/Noto Sans SC 子集化产物入库（fonttools subset）；若目标镜像无 pyftsubset 需在 host 生成后提交 .otf（开发环境前置，非运行依赖）。

---

*（End of Design v1.0）*
