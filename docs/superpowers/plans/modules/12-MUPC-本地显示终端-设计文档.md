# 12-MUPC 本地显示终端（触摸式本地 HMI）设计文档

> ✅ **`[DESIGN_APPROVED: 2026-09-11, 设计评审员]`**（**原文未改**；**仅对 §1–§14 的既有范围有效**——不覆盖 §15 增量）

> ⏳ **`[2026-09-23 · U-73 外设数值上屏增量 · 待设计评审]`** —— 2026-09-23 追加 **§15 外设数值上屏（U-73 增量）**，对应 PRD v2.2 的 **§3.9 F20–F26**（`[REVIEWED: PASS: 2026-09-23]`）与 **T-8 裁定（两处拆）**。**该增量未获批前，其对应实现不得启动**；文首 `[DESIGN_APPROVED: 2026-09-11]` 与本增量**互不覆盖**。**§1–§14 的正文、门禁标记与 §14 条目一律未改。**
>
> 🔁 **`[2026-09-23 · v2.1-r2 · 按设计评审意见返工 · 待复审]`** —— 首次评审为 **REJECTED**（5 严重 / 6 中等 / 5 处事实与交叉引用错误）⇒ 本版**逐条返工**（逐项见 §15 开头的「修订声明」与附录 A 的 v2.1-r2 行）。**改动范围**：① 本文件 **§15**；② **12-UI 设计文档**新增 **§6.4.1 / §6.6.1 / §5.1 #21 / §2.1 补注**（补齐 **R-36**「P4/P6 版面」这一"HMI 编码前必须先补"的硬前置 —— 视觉权威在 UI 文档，故版面落该文档，本文件只引其常量与断言）。**未改**：PRD（冲突只登记 **R-40 / R-42**）、01 / 03 号设计、任何代码、任何既有门禁标记。
>
> 🔁 **`[2026-09-23 · v2.1-r3 · 末轮返工（只改文字与数字，不重构） · 待复审]`** —— 第二次评审**确认**守卫链 / 接口真实性 / 555 点 / PRD 引用**独立复算通过**，但仍有 **5 严重（依据与数字）+ 6 中等** ⇒ 本版**逐条返工**（逐项见 §15 开头的 v2.1-r3「修订声明」与附录 A 的 v2.1-r3 行）。**改动范围**：① 本文件**文首「权威点表」行 + §15**；② **12-UI 设计文档**的 **LVGL 依据四处**（§2.1 补注 / §5.1 #21 / §5.3 / §6.6.1，另同步 §10 映射表行）。**未改**：PRD（自身冲突只登记 **R-40 / R-42**）、01 / 02 / 03 号设计、任何代码、任何既有门禁标记（`[DESIGN_APPROVED: 2026-09-11, 设计评审员]` **原文未动**，且**仍不覆盖 §15**）；**R-45 是对 02 号的依赖登记**（非 PRD 冲突）。

> - 关联 PRD：[`12-MUPC-本地显示终端-PRD.md`](../specs/modules/12-MUPC-本地显示终端-PRD.md)（v2.2，`[REVIEWED: PASS: 2026-09-23]`）——**本设计的上位权威**，冲突以 PRD 为准
> - 关联 UI 设计：[`12-MUPC-本地显示终端-UI设计文档.md`](12-MUPC-本地显示终端-UI设计文档.md)——视觉与交互规范的权威。**分工**：本文件给**功能与结构**（分区顺序 / 控件类型 / 常量断言），UI 文档给**版面几何**（矩形 / 间距 / 字号 / 行高）。本增量按 **R-36** 的裁定**追加了 UI 文档的 §6.4.1 / §6.6.1**（该追加节**不在** UI 文档 `[DESIGN_APPROVED: 2026-09-10]` 的覆盖范围内，**待 UI 评审员复审**），**UI 文档 v2.0 正文未改**
> - 权威点表：协议 V1.3（EMS）3 区只读点表（FC04）；`REG_SOC=1010` / `REG_RUN_STATE=1013` 在 `mupc/crates/intercore/src/pcs.rs:20` / `:22`，**`REG_I_A=1022` / `REG_P_A=1029` 不在 `pcs.rs`**，而是 `mupc/crates/intercore/src/transport/modbus.rs:170-171` 的**文件私有常量**（同处另有 `REG_P_TOTAL=1032`；`pcs.rs` 侧提公有留待后续）
> - 目标平台：BECG-3568（RK3568，aarch64 Linux，openEuler 22.03+ / Ubuntu 20.04+），HDMI 外接 8 寸 **1024×768 触摸屏**，**无浏览器 / 无显示服务器（无 X11 / Wayland）**
> - 本期边界：**移除 `web-api` crate**；6 页信息架构（P1–P6）；写操作仅限「配置保存 / 联锁释放 / M1 授权」（PRD §0 B5）
>
> **⚠️ 本文档的四个诚实前提（不粉饰）**
> 1. **既有的手绘渲染层（`layout.rs` / `font.rs` / `Canvas` 绘制原语 / `OffscreenCanvas`）被判废弃**，仅保留其 **fbdev 像素通道（`FbCanvas`，见 §1.1.1 与 §8.1）**、色板与字库子集资产。保留 / 废弃逐项见 §8。
> 2. **本模块的净新增工作量主要集中在 `mupcd` 侧**（配置热生效子系统、日志服务、审计服务、控制接口、系统/告警采集），HMI 侧因改用成熟 GUI 框架反而收缩。工作量分布见 §12.4。
> 3. **LVGL 是 C 库，Rust 集成必须自建 FFI 绑定层**——上游 `lvgl-rs`（`lv_binding_rust`）**最新版 0.6.2（2023-04）仍停留在 LVGL 8.3.5 且已停更**（LVGL 官方 issue #7298「Development of `lv_binding_rust` has stalled」），**LVGL v9 无任何维护中的安全绑定**。本设计选**自写绑定**（绑定 C 层 + 薄安全层），**绑定层是确定性工作量（约 1000–1800 行 Rust），不可默认其"免费"**。选型论证见 §1.1.1，风险见 R-19 / R-20 / R-21。
> 4. **构建链为「Rust + LVGL C 源码」**：需要 C 编译器与 `bindgen`（**Windows 开发机需装 LLVM/libclang**）、aarch64 交叉 C 工具链（**项目已有**，见 `mupc/build.md`）。这是**可预估、可验证**的成本（§12.1），但**须在编码前跑通三平台（Windows 本机 / x86_64 Linux CI / aarch64 交叉）的 LVGL 编译 spike**（R-20；**完成度 3/3，三平台均已实测走通**（2026-09-19 Linux 构建机实测；证据链见 `docs/technical-debt.md` §8.6.1 L-1））。

---

## 目录

1. [方案探索与技术选型](#1-方案探索与技术选型)
2. [架构总览](#2-架构总览)
3. [通道协议（读通道 + 控制通道）](#3-通道协议读通道--控制通道)
4. [mupcd 侧改动](#4-mupcd-侧改动)
5. [HMI 进程设计](#5-hmi-进程设计)
6. [6 页详细设计](#6-6-页详细设计)
7. [web-api 移除方案与出口迁移表](#7-web-api-移除方案与出口迁移表)
8. [复用 / 废弃清单（既有资产去留）](#8-复用--废弃清单既有资产去留)
9. [边界与异常对照 PRD §5](#9-边界与异常对照-prd-5)
10. [非功能预算落实](#10-非功能预算落实)
11. [测试策略](#11-测试策略)
12. [部署与交叉编译](#12-部署与交叉编译)
13. [技术决策记录（ADR）](#13-技术决策记录adr)
14. [待真机 / 待确认项](#14-待真机--待确认项)
15. [外设数值上屏（U-73 增量：F20–F26 / T-8 落点）](#15-外设数值上屏u-73-增量f20f26--t-8-落点)

---

## 1. 方案探索与技术选型

> KISS 校准：既有的自绘路线在「固定网格 + 少量中文词 + 只读」前提下是 KISS 的；本版需求变为**6 页 + 常驻导航 + 滚动列表 + 多选选项 + 数字步进 + 模态确认弹层 + Toast + 焦点管理 + 中文文本换行**，自绘路线的成本结构发生反转——**此时引入成熟 GUI 框架才是 KISS**。但框架必须满足「可交叉编译 / 无显示服务器 / 触摸 / 中文 / ≤256MB·≤40% 单核」五重约束。
>


### 1.1 关键决策 A：GUI 框架选型

候选路线（按引入面从小到大）：

| 路线 | 交互式控件/页面/导航 | 中文字形 | 交叉编译 | 无显示服务器 | 内存/CPU 适配 | 无真屏可测 | 成熟度/维护 | **许可证** |
|------|----------------------|----------|----------|--------------|---------------|------------|-------------|-----------|
| **L-1 LVGL v9（C 库 + Rust FFI 绑定，选定）** | ✅ **内置控件即够用**（`lv_btn`/`lv_label`/`lv_list`/`lv_table`/`lv_chart`/`lv_dropdown`/`lv_switch`/`lv_msgbox`/`lv_tabview`/`lv_buttonmatrix`/`lv_bar`/`lv_led`/`lv_checkbox`；**步进器 / IPv4 / 日期时间一律 `lv_btn` + `lv_label` 组合**，**弃 `lv_spinbox`**，理由见 §5.6 F12 行），配 `lv_style` 主题即可落 UI 色板与尺寸；**不再自绘控件**（见 §5.6 反转） | ✅ `lv_font_conv` 离线生成 CJK 子集（size/bpp 可控）→ **无"限西文脚本"类限制**；断行/LVGL `LV_LABEL_LONG_WRAP` 原生 | ⚠️ 引入 **LVGL C 源码编译**（`cc` crate / CMake）→ 需 C 编译器（三平台均需），但**交叉工具链项目已有**（`mupc/build.md`） | ✅ **原生 `LV_USE_LINUX_FBDEV`**（fbdev 驱动在 LVGL 主干内）+ 我们可注册**自定义 `lv_display` flush_cb** 直写 fb；DRM 亦原生支持（备选） | ✅ LVGL 为嵌入式而生（**KB 级基座**）；脏区重绘（`lv_obj_invalidate`）；1024×768 单帧缓冲 ≈3 MB | ✅ **可注册"内存 display"离屏后端**（自定义 `lv_display` + flush_cb 写 `Vec<u8>`）→ 本机可断言像素 | ✅ **LVGL 9.5 主干活跃**（LVGL 官方维护，嵌入式业界事实标准，数月一版）；⚠️ **但 Rust 绑定侧不活跃**（见 §1.1.1） | ✅ **MIT**（可闭源商用、可静态链接、无 royalty、无归属展示义务） |
| L-2 egui + eframe | ✅ 即时模式控件齐全，但布局需手写坐标/容器，无声明式 | ✅ 成熟（`FontDefinitions` + CJK TTF 子集，纯 Rust 栅格化） | ⚠️ 官方路径需 winit+GL → 目标需 EGL/GBM/X11 开发库；**无官方 framebuffer 后端** | ⚠️ 需自研 fb 后端（非官方路径） | ⚠️ 即时模式**全帧重绘**语义，1024×768 软光栅稳态 CPU 难守 ≤40%；需靠 `request_repaint` 节流 | ✅ `egui_kittest` / 直接跑 `Context` 断言 | ✅ 活跃 | ✅ `MIT OR Apache-2.0` |
| L-3 GTK3/4 | ✅ 完整 | ✅（pango/fontconfig） | ❌ 巨型 C 依赖树 + pango + fontconfig + 交叉工具链 | ⚠️ 需 X/Wayland（GTK4 Broadway/无头受限） | ⚠️ 基座内存逼近/超出 256MB | ⚠️ 需 xvfb | ✅ | ⚠️ LGPL（静态链接需合规评估） |
| L-4 Qt (Qt for Embedded) | ✅ 完整 | ✅ | ❌ 重量级 C++ 依赖 + qmake/cmake 编排 | ✅（linuxfb/eglfs） | ⚠️ 重 | ⚠️ | ✅ | ❌ 非 Rust + 商业授权 |
| L-5 自研控件库（扩既有 ab_glyph 手绘） | ❌ **全部自建**：滚动容器、多选 chip、步进器、模态弹层与焦点、Toast、页面路由、命中测试、中文断行 | ✅（已有 ab_glyph 与子集字库） | ✅ 纯 Rust | ✅（已有 fbdev） | ✅ 最省 | ⚠️ 命中测试/交互状态机难以离屏覆盖，需自建测试框架 | ❌ 无上游、无社区 | ✅ 自有 |

**选型结论：L-1 —— LVGL v9（C，MIT）+ 自写 Rust FFI 绑定层（§1.1.1）+ 自定义 `lv_display`/`lv_indev` 后端（§1.1.1.1 / §1.2）。**

理由：
1. **需求形态决定了"现成控件 + 声明式布局"是 KISS**。v2.0 的页面/弹层/列表/表格/选项式输入在 LVGL 里几乎一一对应（`lv_tabview`/`lv_list`/`lv_table`/`lv_msgbox`/`lv_dropdown`/`lv_switch` + 步进器用 `lv_btn`+`lv_label` 组合，见 §5.6），而自研路线的 L-5 需要重建整个 UI 框架（保守估计 3000–6000 行 + 一套交互状态机测试框架），**违背 KISS 且显著高于引入框架的成本**。
2. **许可证是本次切换的唯一动因，且被彻底消除**：LVGL 为 **MIT**，闭源商用嵌入式交付**零成本、零义务、可静态链接**（§1.1.4）。这是 L-1 相对 L-2（egui，许可亦无成本）的**决定性优势**（不选 L-2 的理由见下）。
3. **中文不再有框架级风险**：`lv_font_conv` 生成 CJK 子集是 LVGL 生态的标准做法（§1.1.2），**不存在 Slint"软渲染文本限西文脚本"的官方限制**（原 R-01 从"高"降为"低"）。
4. **显示后端有官方 fbdev 驱动**，且**我们仍保留自研 flush_cb 的能力**（§1.1.1.1）——既可用官方 `LV_USE_LINUX_FBDEV`，也可复用既有已落地的 `FbCanvas`（fb0 mmap + 格式探测 + 逐行写）作为**自定义 display 后端**。这使**像素格式风险归零**（论证同构，只是载体从 `LineBufferProvider` 换成 `flush_cb`）。
5. **触摸仍由我们自己的 Rust evdev 栈掌控**：LVGL 允许注册**自定义 `lv_indev` + `read_cb`**（§1.2），因此 **既有 §1.2/§5.3 的触摸设计（设备发现/多候选报错/校准/CLI 覆盖/缺失容错）逐条保留**，且**不引入 libevdev C 依赖**（LVGL 自带的 `lv_evdev` 驱动**依赖 libevdev**，本设计**不启用** `LV_USE_EVDEV`，见 §1.2）。
6. **可测性达标**：LVGL 支持"内存 display"离屏后端（自定义 `lv_display`，`flush_cb` 写入 `Vec<u8>`，`lv_refr_now()` 强制渲染）→ 在开发机（Windows x86，无屏）即可对区域像素断言并**真实渲染中文字形**（§11.1）。
7. **L-2（egui）不选**的理由：其官方嵌入式路径要求 winit+GL（引入 X11/EGL/GBM C 依赖且**仍无 fb 后端**），自研 fb 后端可行但**即时模式全帧重绘**与 PRD §4.1「稳态 CPU ≤40%、空闲让出 CPU、不得忙等」相冲；要压住必须自实现重绘节流与脏区（回到手写）。**egui 保留为 L-1 绑定工作量不可接受时的备选**（许可证无成本，代价是 CPU 预算需靠节流补偿）。
8. L-3/L-4 因依赖体积、交叉成本、内存基座与许可证（LGPL/商业）出局；L-5 因工作量与可测性出局。

> **备选路线（评审可议）**：
> - **若 §1.1.1 的绑定层工作量被判定不可接受** → 切 **L-2（egui + 自研 fbdev/evdev）**，此时 §3/§4/§6 的通道、页面与 mupcd 侧设计**完全不变**（框架只影响 §5 的 HMI 渲染层与 §8 的 `local-display` 模块清单）。
> - **若 LVGL C 编译链路在真机/交叉环境受阻** → 见 R-20 的处置（放宽到 `bindgen` 预生成 + 提交生成物、或改用 L-2）。
> 这是把框架风险局限在 HMI 进程内部的设计目标。

#### 1.1.1 ⚠️ 关键决策 A2：LVGL 与 Rust 的集成方式（FFI 绑定选型）

> **这是本模块的核心新增风险项**（R-19）：LVGL 是 **C 库**，Rust 侧**必须**有一层 FFI 绑定。

**现状核查（诚实结论，2026-09-10）**：

| 方案 | 版本/状态（核查事实） | 安全绑定 | LVGL 版本 | 维护 | 结论 |
|------|----------------------|----------|-----------|------|------|
| **B-1 上游 `lvgl-rs`（`lv_binding_rust`）** | crates.io 最新 **`lvgl` 0.6.2（2023-04-02）**；`lvgl-sys` 同步 0.6.2；GitHub `lvgl/lv_binding_rust` | ✅ 有（`lvgl-codegen` 生成的**全量**安全封装）+ `lvgl-sys`（bindgen 原始绑定） | **LVGL 8.3.5**（**非 v9**） | ❌ **已停滞**：LVGL 官方 issue **#7298**「Development of `lv_binding_rust` has stalled」；LVGL 明确**无法派员工维护**、**近期无 v9 升级计划**；社区提出的 `lvgl_rust_sys` 解耦方案尚未落地到该 crate | ❌ **不作为主选**：① 版本落后一个大版本（8.x 为 legacy 线，v9 为当前线，API 大幅变更）；② 项目停更，无人跟 LVGL 上游；③ 有未修的 build/lifetime/segfault 类 issue 报告 |
| **B-2 v9 时代的**原始** sys 绑定 crate** | 存在两类：`lvgl_rust_sys`（fork，用 `cc` 编译 LVGL **v9.5.0**，已验证可交叉到 Xtensa/ESP32）与 **`lightvgl-sys` 9.5.3**（crates.io，bindgen 原始绑定，跟踪 LVGL 9.5.0/9.4/9.3，**显式支持交叉编译 env：`CROSS_COMPILE` / `BINDGEN_EXTRA_CLANG_ARGS` / `LIBCLANG_PATH`**，需 `DEP_LV_CONFIG_PATH` 指向 `lv_conf.h`） | ❌ 仅**原始 `unsafe` FFI**（无安全层） | **v9.5**（当前线） | ⚠️ 较新但**用户面窄、非官方**（无 LVGL 官方背书，维护者单一） | ⚠️ **可作为 sys 层的省力起点**（省去自写 `build.rs`/bindgen 配置与 `lv_conf.h` 接线），但**不可依赖其长期维护** |
| **B-3 自写绑定（选定）**：vendor LVGL v9.5 源码 + 自写 `build.rs`（`cc` crate 编译 + `lv_conf.h`）+ `bindgen` 生成原始绑定（**精确 allowlist 逐符号限定**，见 §1.1.1.2）+ **自写薄安全层** | 我们自持：**LVGL 源码以 git submodule / vendor 目录 pin 到具体 tag**（如 `v9.5.0`），可随时升级 | ✅ **自写**（仅覆盖本项目实际用到的 API 子集） | **v9.5**（由我们 pin，可升级） | ✅ **我们自持**：无第三方维护风险；升级 = 换 tag + 重跑 binding 生成 + 修编译错（LVGL 官方提供 v8→v9 迁移指南） | ✅ **选定** |

**选型结论：B-3 —— 自写绑定（自持 LVGL 源码 + `cc` 编译 + bindgen allowlist + 薄安全层）。**

理由：
1. **B-1 不可用**（LVGL 8.3.5 + 停更 + 与 v9 生态割裂，前述引用的 LVGL issue #7298 为官方定性）；**B-2 只能省掉"接线"，省不掉"安全层"**，且把 sys 层交给单一非官方维护者，与本项目「自持关键路径」的原则不符。
2. **绑定面被需求本身限定得很小**。本项目实际需要绑定的 C API 是**有限且可枚举**的（约 20 个控件 + 样式/主题 + 字体注册 + display/indev 注册 + tick/timer + 事件回调），而非"整个 LVGL"。用 bindgen **逐符号枚举**（`allowlist_function` / `allowlist_type` / `allowlist_var` + `allowlist_recursively(true)`）**只生成所需符号**，可显著压缩生成物与 `unsafe` 面。**这里的 allowlist 必须是精确清单（禁止 `lv_*` 通配）**——通配 `lv_*` 等于**全量生成**，与本条的目标（压缩生成物/`unsafe` 面）自相矛盾（设计评审订正项，机制见下）。
3. **绑定层是"机械但确定"的工作量，不是研究风险**。本设计给出**绑定层的工作量边界与验收口径**（§1.1.1.2），使 PM 可据此排期，而不是把它当作"未知"。
4. **版本可控**：pin tag = 锁定 ABI；升级路径明确（LVGL 官方 v8→v9 迁移指南 + 我们自持的薄层是唯一需要跟改的地方）。

##### 1.1.1.1 显示/输入后端接入：三条路径

| 路径 | 做法 | 依赖 | 结论 |
|------|------|------|------|
| **P-1 自定义 `lv_display` + 自研 `flush_cb` 直写 fb0（选定，默认）** | `lv_display_create(w,h)` → `lv_display_set_buffers(buf1, buf2, size, LV_DISPLAY_RENDER_MODE_PARTIAL)` → `lv_display_set_flush_cb(cb)`，`cb` 内把 LVGL 渲染好的区域**做像素格式转换后写 `/dev/fb0`**（复用既有 `FbCanvas` 的 mmap/格式探测/逐行写逻辑） | **仅 LVGL 本体**（无额外 C 库）；fb 由我们 mmap | **✅ 选定**。① **像素格式完全自控**（不依赖 LVGL fbdev 驱动对目标面板格式的支持）；② **与离屏测试后端共用同一条 flush 路径**（仅 sink 不同：`Vec<u8>` vs fb0）→ 测试与生产同源，可测性最好；③ **`FbCanvas` 资产被复用**（而非 r1 的"降级"） |
| P-2 LVGL 官方 `LV_USE_LINUX_FBDEV` 驱动 | `lv_linux_fbdev_create()` + `lv_linux_fbdev_set_file(disp, "/dev/fb0")`（v9 主干内置；设备可用 `LV_LINUX_FBDEV_DEVICE` 覆盖） | LVGL 本体，**无额外 C 库** | ⚠️ **作为 P-1 的一行开关式备选**（若 P-1 的 flush 路径出现性能问题）。**不作为默认**：其像素格式/双缓冲策略由驱动决定，遇非常规面板格式时不如 P-1 可控；且与离屏后端不是同一条代码路径 |
| P-3 LVGL 官方 `LV_USE_LINUX_DRM`（DRM/KMS） | `lv_linux_drm_create()` + `lv_linux_drm_set_file()` | LVGL 本体（可选 libdrm） | ⚠️ **不在本期自研/主用**。仅当真机 `/dev/fb0` 不可用时启用（R-03）；已知 v9 早期版本 DRM 路径有分辨率硬编码与 `lv_tick_set_cb` 缺失导致锁死的报告，启用前须核对所用 tag 的修复状态 |

> **主循环与 tick 契约（P-1 必备）**：LVGL 需要 `lv_tick_set_cb()` 提供单调毫秒时基（由我们以 `Instant` 实现）；`lv_timer_handler()` 需被周期性调用，**其返回值即"距下次需要处理的时间"→ 直接作为我们 `poll()` 的超时上界**，天然满足「唯一阻塞点、无忙等」（§5.2 不变量 1）。

###### 1.1.1.2 绑定层的范围、分层与工作量边界

```
crates/local-display/
├── lvgl-sys/           # 原始绑定（bindgen 生成，unsafe）
│   ├── build.rs        #   cc 编译 LVGL C 源码（含 lv_conf.h）+ bindgen(精确 allowlist)
│   ├── allowlist.txt   #   【新】bindgen 精确 allowlist（逐符号枚举，禁 lv_* 通配，机制见下）
│   ├── lv_conf.h       #   LVGL 配置（本设计给定初值，见下）
│   └── src/lib.rs      #   include!(OUT_DIR/bindings.rs)（实测 852 行；预生成入库 feature prebuilt-bindings 本轮未做，见 §12.1 / R-26）
└── src/lvgl/           # 薄安全层（自写，本项目唯一的 unsafe 边界收敛处）
    ├── mod.rs
    ├── obj.rs          #   Obj 包装（创建/父子/坐标/可见性/样式引用）
    ├── widgets.rs      #   控件的类型化构造与属性 setter（§5.6 控件映射表）
    ├── style.rs        #   Style 机制层：类型化 lv_style_* setter / 选择器 / 部件枚举
    │                   #   （外观数值不在此处——唯一真源见 ui/theme.rs；落 UI §3.2/§3.5）
    ├── font.rs         #   字体注册（lv_font_conv 产物）+ 文本设置
    ├── display.rs      #   lv_display 注册 + PARTIAL 双缓冲 + flush 桥
    ├── indev.rs        #   lv_indev 注册 + read_cb 桥（接 Rust evdev）
    └── event.rs        #   事件回调桥（C 回调 → Rust closure，含 user_data 生命周期管理）
```

**allowlist 的形态（精确、可审计，取代 `lv_*` 通配）**：`lvgl-sys/allowlist.txt` 逐行列出所需符号（`fn:lv_obj_create` / `type:lv_obj_t` / `var:lv_font_noto_sc_24`），`build.rs` 读该文件逐项调用 `allowlist_function` / `allowlist_type` / `allowlist_var`（`allowlist_recursively(true)` 以带上传递依赖类型）；**禁止任何 `lv_*` 通配**。双向约束（CI 断言，见 §12.1）：① `bindings.rs` 导出的符号集合 ⊆ `allowlist.txt`（多一个即失败）；② `allowlist.txt` 中每个符号在 `src/lvgl/**` + `ui/**` 中确有引用（无死符号）。**新增控件用点时必须同步补清单**——该纪律与下述 unsafe 边界纪律同等强制（薄层是唯一用点，故清单与薄层一一对应、可机械核对）。

**实测口径澄清**：双向断言已落地为**可运行测试** `mupc/crates/local-display/lvgl-sys/tests/allowlist_consistency.rs`（**尚未接 CI**）。实测：`bindings.rs` **852 行** / 导出 **21 fn · 2 static · 26 type · 18 const** / allowlist **35 条**（21 fn / 12 type / 2 var）。**断言①（导出 ⊆ 清单）对 `fn`/`static` 是硬断言**；对 **`type`/`const` 目前只做存在性校验 + 打印闭包清单**——因 `allowlist_recursively(true)` **必然**带出 **14 个传递依赖 `type`**（`_lv_obj_t`、`lv_draw_buf_t` …）与 **18 个 enum `const`**，机械的"⊆"在 `type` 上**不成立**。**落地路径（须落实）**：薄层 `src/lvgl/**` 写完后，**把 `type` 也升为硬断言**（清单须显式列入全部被引用 `type`，CI 即按硬断言跑）。**断言②（无死符号）当前实测通过**（清单内 21 fn + 2 var 均在 `src/**`/`examples/**` 被引用）。

**`lv_conf.h` 关键项（本设计初值；**逐条结论见 §12.1**）**：`LV_COLOR_DEPTH 32`、`LV_USE_LINUX_FBDEV 0`（走 P-1，不启用官方 fbdev 驱动）、`LV_USE_EVDEV 0`（**不引入 libevdev**，走自研 indev，§1.2）、`LV_USE_LINUX_DRM 0`、`LV_USE_SDL/GLFW/X11/WAYLAND 0`、`LV_USE_LOG 1`（转发到 Rust `tracing`）、`LV_MEM_SIZE`（**已定稿 1 MB**：实施期实测 256 KB / 512 KB 均不足、1 MB 通过，见 §10；`lv_mem_monitor_t` 真机实测仍待，见 §14 R-24）、`LV_USE_OS` 与线程策略（本设计**单线程**：UI 全部在事件循环线程内，见 §5.2）。

**工作量边界（诚实标注，供 PM）**：薄安全层 **约 1000–1800 行 Rust**（控件 setter 占比最大，机械度高）；`build.rs` + `lv_conf.h` + allowlist 维护约 **150–300 行**。**不含** `lv_font_conv` 产物（C 文件，由工具生成）。该工作量已计入 §12.4 工作单元 A。

**实测锚点（R-19）**：**35 条 allowlist（21 fn / 12 type / 2 var）即跑通** `init/tick/display/双缓冲/flush/obj/label/style/font/render` **全链**（S-2），`bindings.rs` 852 行；按"每控件 ≈ 1 构造 + 6–10 个 setter"外推 **约 1300–2200 行**（设计估 1000–1800 略偏乐观，量级吻合）。**判定：机械但确定，不是研究风险，可排期**（详见 §12.4 工作单元 A 与 §14 R-19）。

**unsafe 边界纪律（编码约束）**：
1. `unsafe` **只允许**出现在 `lvgl-sys` 生成物与 `src/lvgl/*` 薄层内部；**`lvgl-sys` 不得被 `pages`/`state`/`channel` 等模块直接引用**（CI 以源码扫描断言，§11.1 静态约束 ⑥）。
2. 所有 `lv_*` 调用的**调用线程必须是事件循环线程**（LVGL 非线程安全）；薄层不提供任何跨线程 API。
3. C 回调 → Rust 的 `user_data` 生命周期由 `event.rs` 统一管理（`Box::into_raw` / `from_raw` 配对，对象删除时 drop），**禁止**在回调内 `panic`（跨 FFI 展开为 UB；统一 `.catch_unwind` 或改为错误码返回）。**回调内的诊断输出必须走 `lvgl::diag`，禁用 `eprintln!`/`println!`** —— 后者的"写失败即 panic"发生在 `catch_unwind` 之外、栈上已是 C 帧，同样构成 UB（2026-09-19 补）。
4. **具名豁免（两条，均不引用 `lvgl_sys`，与纪律 1 的立意不冲突）**——2026-09-19 补记，此前仅存在于代码注释中：
   - **`src/canvas.rs::fbdev::FbCanvas`**（v1.0 既有资产，设计 §8.3 保留、v2.0 升为 `flush_cb` 的像素 sink）：其 `unsafe` 为 libc `open`/`ioctl`/`mmap`/`munmap`，**不涉及任何 LVGL 绑定**；
   - **`src/timing.rs`**（`FdPoller::poll_checked` 的 `libc::poll` 与 `install_stop_signals` 的信号处理器）：系统调用类，落点由工作单元 C 定于本文件，**同类同处**。
   ⚠️ 两处均**刻意不实现 `Send`/`Sync`**（含 mmap 裸指针/事件循环独占），把单线程约束交给编译器。
   **若将来要求严格回到"`unsafe` 只在 `src/lvgl/**`"**，应把这两处一并上收为 `src/lvgl/os.rs` 之类的薄层（属结构性调整，不在本期）。

#### 1.1.2 中文文本与字体资源（`lv_font_conv`）

> **本节按实测整改**（证据：`docs/TODO/12-v2.0-LVGL-spike-报告.md` §3）。**体积预算与入库口径以实测为准**。

- **字库来源与生成命令**：沿用既有的 OFL 中文黑体选型（**选 Noto Sans SC**，OFL-1.1，8,331,336 B / 7.9 MB），经 `lv_font_conv@1.5.2`（Node CLI，推荐 CLI 以便 CI/离线复现）**离线子集化并编译为 C 数组**：
  ```bash
  # 默认：启用 LVGL 内置 RLE 压缩（命令不传 --no-compress）
  npx -y lv_font_conv --font NotoSansSC-Regular.otf --size 32 --bpp 4 --format lvgl \
      --symbols "$(cat font_subset_charset.txt)" --lv-include lvgl.h \
      -o lv_font_noto_sc_32.c
  # 同法生成其余档位（档位数组固定为 24 26 28 32 48 56 64 96 112 148，共 10 档，
  #   与 UI §3.3 字号阶梯一一对应；由 fonts/gen_fonts.sh 遍历生成，每档同用 §3.6 全用字表）
  # 复议"无压缩"口径：NOCOMPRESS=1 ./gen_fonts.sh 32（等价于原先的 --no-compress）
  ```
  **为何默认去掉 `--no-compress`**：实测同一码表 32 px 档，**启用 RLE 后位图 131,793 → 71,078 B（-46%）**，且 **RLE 无损、不掉画质**。`--no-compress` **恰是导致体积超标的那个开关**，故默认命令去掉它；仅在复现无压缩口径时用 `NOCOMPRESS=1` 显式开启。**优先级明确**：**RLE（无损）优于"降到 2 bpp"**（2 bpp 无抗锯齿，会牺牲 UI V-4 的密笔画可辨性）。
- **码表**：真源为 **UI 设计 §3.6 的全屏用字表**，由 `fonts/extract_charset.py` **自动提取**（不做猜测；脚本内置**漂移断言**：表末声明的每个字形/字串——`MUPC`/`ERROR`/`kW`/`0–9`/`Σ`/`⚠`/`▲` 等——必须真的出现在 §3.6 正文，否则**直接报错退出**；`🔒` 按 UI 文档"以几何锁形替代"**不纳入**）。实测产出：**271 汉字 + 32 拉丁/数字 + 35 符号几何 = 326 字符**（`font_subset_charset.txt`，901 B）。漏字即屏上出现豆腐块，故 §11.1 保留**码表覆盖率测试**。
  - **格式硬约束**：`font_subset_charset.txt` **必须写成单行（无换行）**——`lv_font_conv` **只有 `--symbols <串>`、没有 `--symbols-file`**，而 `"$(cat …)"` 会把换行折叠成**空格、污染字符集**（空格不是合法字符分隔符）。
- **字号档位与体积预算（全档位 + 全码表，10 档逐档实测）**：**共 10 档**，与 **UI 设计 §3.3 字号阶梯一一对应**；**每档均使用 UI §3.6 全用字表（326 字符），不做分档码表**。4 bpp 抗锯齿、**默认启用 LVGL 内置 RLE**。**下表为单机实测（非外推）**：

  | 档（px） | UI §3.3 层名 / 用途 | 位图字节（实测，RLE） |
  |---:|------|---:|
  | 148 | **L1-特大**（P1 SOC 数值） | 503,819 |
  | 112 | **L1-大**（P1 PCS 状态词） | 342,075 |
  | 96 | **L1-大**（P4 联锁总态词） | 273,300 |
  | 64 | **L2-主值**（三相 P 数值） | 164,803 |
  | 56 | **L1-单位**（`%`） | 140,719 |
  | 48 | **L2-次值**（三相 I 数值） | 118,532 |
  | 32 | **L2-页标题 / L2-数值**（页眉、装置卡值、审计值） | 71,078 |
  | 28 | **标签-大**（区块标题 / 相标） | 59,526 |
  | 26 | **标签**（字段名 / 控件文字 / 按钮字） | 53,805 |
  | 24 | **正文 / 弱注**（列表正文、图例、刻度；全屏下限） | 48,423 |
  | **10 档合计** | — | **1,776,080 B ≈ 1.69 MB** |

  - **口径（重要）**：上表为**单机实测**（RLE 启用、326 字符码表、`lv_font_conv 1.5.2`），**直接采用，非外推**。合计 1,776,080 B 为**表内十行逐行相加所得**。生成耗时：单档 **5–9 s（实测）**；**全 10 档约 1–2 分钟为估算（未整批实测）**（CI 步骤相应增多，但工具驱动、确定性高，§12.4 工作单元 D）。
  - **位图体积不随字号平方增长（实测）**：148 px 字号为 32 px 的 **4.625 倍**，位图仅 **7.1 倍**（503,819 / 71,078）——**实测增长显著低于平方**（平方会给出 ~21 倍）。**故不得以平方律外推任何档位体积。**
  - **32 px 档的口径对照（实测）**：位图 **131,793 B（128.7 KB，未压缩）/ 71,078 B（71.1 KB，RLE，-46%）**；单档 `.c` 文本 **830,590 B（811 KB）/ 489,851 B（RLE）**；编译产物 `liblvgl_fonts.a`（release，1 档 32 px、326 字符）**136,892 B（134 KB）**。
  - **⚠️ 口径陷阱（必须遵守）**：若按"构建后 `ls -l` 复核"，**会把 811 KB 的 `.c` 文本当成本档体积，误差约 6 倍**（32 px 档 `.c` 文本 ≈ 位图的 **6.3 倍**，RLE 档为 6.9 倍）。**正确复核口径 = 位图字节数**（`grep -o '0x[0-9a-f]*' \| wc -l` 计数）**或编译产物 `.a` 落盘体积**（`ls -l` 只对 `.a` 有效，**不对 `.c` 有效**）。**任何体积断言/预算行不得以 `.c` 文本大小为准。**
- **资产入库策略（字库源与生成物均不入库）**：**`fonts/*.otf` 与 `fonts/*.c` 都不入库**（已写入仓库根 `.gitignore`）。**入库项仅 4 个**：`gen_fonts.sh`、`extract_charset.py`、`font_subset_charset.txt`、`NotoSansSC-LICENSE.txt`（OFL-1.1 许可文本，随附以满足再分发要求）。理由：`.c` 文本约为位图的 6–7 倍，**10 档合计外推约 10–12 MB**，若入库会平白膨胀仓库；OTF 可公开下载；生成物属构建产物。**纪律：构建前必须先跑一次 `gen_fonts.sh`**（CI/构建脚本须编排此步）。
- **`noto-font` feature 的构建前置（必须写进 `build.rs`/CI）**：生成物不在仓库里，故未跑 `gen_fonts.sh` 时，`lvgl-sys` 的 **`noto-font` feature 必须给出清晰可读的编译期报错**（例如 `compile_error!("未找到 fonts/lv_font_noto_sc_*.c，请先运行 fonts/gen_fonts.sh")` 或 `build.rs` 的 `panic!` 附操作指引），**不得退化为晦涩的 file not found**。**默认构建（不启用 `noto-font`）不受影响**（字体 `../fonts/*.c` 仅在存在时编入独立的 `lvgl_fonts` 静态库）。
- **中文断行**：LVGL `lv_label` 的 `LV_LABEL_LONG_WRAP` 原生支持按宽度换行；CJK 无空格断行由 LVGL 的字符级 wrap 处理。**日志长消息**仍需按 UI §3.4 的**软断点后处理**（在 Rust 侧对消息按字数插入软断点，不改数据）。
- **字体应用**：`lv_font_noto_sc_NN` 通过 `lv_style_set_text_font()` 挂在主题样式上（§5.6），**页面不硬编码字体引用**（与"页面不硬编码色值"同口径）。

#### 1.1.3 CJK 字形风险

- **背景**：Slint 官方文档明示其**软件渲染器**「Text rendering is limited to western scripts」，这是当初把本项列为高风险的原因。
- **LVGL 下的结论**：**该风险不存在**。LVGL 的文本渲染是「codepoint → `lv_font_t` 的 cmap 查字形 → 栅格化位图」，`lv_font_conv` 为 CJK 生成的位图字体是 LVGL 生态的**标准做法**（多年、大量中文产品在用），**没有"限西文脚本"一类限制**。
- **降级后的残余风险（保留为 R-01）**：**位图字体的清晰度**（UI V-4 的"密笔画字"如「联锁」「遥测」在 24 px 下的可辨性）——这是**视觉标定问题**，不是可行性问题；处置：字号上调 / 提高 bpp / 加 1 px 描边（UI §11 V-4 已列）。
- **验证方式（仍保留，但定位为"回归"而非"门禁"）**：离屏渲染一屏含「充电/放电/停机/待机/储能电池 SOC/联锁/审计」的界面 → 断言目标文本区域**与背景不同，且以"文本区墨量密度（前景像素数）"为主判据**（§11.1），可在开发机（Windows）完成。
  - **判据（实测）**：判据为「**文本区前景像素数（墨量密度）**」。**不能用"字形连通区域数量 ≈ 字数"**——**判别力弱**：**缺字占位方框同样是"每字一块"**（实测：默认字体 6 个连通域 / 真字形 16 个，**二者不能清晰区分**），故连通域数**降为辅助判据**；墨量密度实测 **缺字 517 → 真字形 2,949，信噪比 ×5.7**，可清晰区分。**正式测试（§11.1）按墨量密度判，或做逐字模板比对**（后者**本轮未做**，列入 §14 未决项）。

#### 1.1.4 许可证合规（**LVGL = MIT，闭源商用嵌入式无成本**）

**为何不用 Slint（许可判定，选型动因）**：Slint 为三许可模型 `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0`，在「闭源商用嵌入式交付」前提下**三条路径逐条不可用**：

| 许可路径 | 事实 | 判定 |
|------|------|------|
| GPLv3 | copyleft：分发即须**整体开源** | ❌ MUPC 为**闭源商用装置**，会污染整个交付物（含 mupcd 全部业务逻辑） |
| Royalty-free 2.0 | 条款**明确排除嵌入式**场景，且须 `AboutSlint` 归属展示 | ❌ BECG-3568 屏是「随装置预装、非通用计算机」的嵌入式场景，**正落在排除范围** |
| 商业许可（Software-3.0） | 闭源可用、覆盖嵌入式，但**付费**（按台 royalty 或买断） | ⚠️ **唯一可用路径 = 新增交付成本**，不可接受 ⇒ **改用 MIT 的 LVGL** |

**结论（须显式声明）**：

1. **LVGL 采用 MIT 许可证**：**可闭源商用、可静态链接进闭源二进制、无 royalty、无源码公开义务、无归属展示义务**（MIT 仅要求保留版权与许可声明文本，做法：在 HMI 二进制随附的 `THIRD-PARTY-NOTICES` 或 P6「关于」页脚内放一行 `LVGL (MIT) — Copyright (c) LVGL Kft` 即可，**成本为零**）。
2. **本模块交付因此无任何许可证成本或法务风险**——这正是选择 LVGL 的**唯一动因与达成结果**（见上方「为何不用 Slint」）。
3. **`lv_font_conv`**：为 LVGL 官方工具（MIT），仅构建期使用，不进入交付物。
4. **字体文件许可**：**文泉驿微米黑（GPLv2 + 字体例外）** 或 **Noto Sans SC / 思源黑体（OFL-1.1）**——**选定 OFL-1.1 系（Noto Sans SC / 思源黑体）**，因其对"嵌入/再分发"无 copyleft 牵连，与闭源交付兼容；**`lv_font_conv` 产物为位图数据、不构成字体衍生作品的分发争议**，但仍应在 `THIRD-PARTY-NOTICES` 中列出字体名与许可。**注意**：若选文泉驿，其 GPLv2+字体例外需法务确认（**建议直接用 OFL 系，规避该议题**）。
5. **Rust 侧依赖**：`cc` / `bindgen`（均为 MIT/Apache-2.0）——无风险。
6. **相关 ADR**：§13 **D15**（LVGL 许可证）。

### 1.2 关键决策 B：触摸输入栈

| 路线 | 依赖 | 校准 | 结论 |
|------|------|------|------|
| **B-1 `/dev/input/eventX` + Rust `evdev` crate + **自定义 `lv_indev`**（选定）** | 纯 Rust（`evdev` crate，直接 ioctl/read）；**不引入 libevdev** | 读 `EVIOCGABS(ABS_X/ABS_Y)` 的 min/max 做线性映射；提供 CLI 覆盖与轴交换/反向 | **✅ 选定**：`lv_indev_create()` + `lv_indev_set_read_cb()` 把 Rust 读到的坐标喂给 LVGL；**设备发现/校准/容错全在 Rust 侧**（设计逐条保留）；**绕开 LVGL `lv_evdev` 驱动的 libevdev C 依赖** |
| B-2 LVGL 官方 `lv_evdev` 驱动（`LV_USE_EVDEV=1`） | ⚠️ **强制依赖 `libevdev` C 库**（LVGL 文档明确「always requires libevdev」）→ 交叉编译须为目标架构额外构建/提供 libevdev + 头文件 | 内建 `lv_evdev_set_calibration()` / `lv_evdev_set_swap_axes()` | ❌ **不选**：① 引入额外 C 依赖（libevdev），显著加重交叉与部署前置；② 设备发现/多候选报错/启动报错等**容错语义不在我们手里**（EDGE-13 要求"触摸失效不影响数据刷新"）；③ 已知 v9.1 起「手工 `lv_indev_create` 与 `lv_evdev_create` 混用会失效」的报告（LVGL issue #6721）——我们不去踩这个坑 |
| B-3 `/dev/input/mice` 或读 tslib | — | — | ❌ 不适用（触摸屏为 evdev 绝对坐标设备，非 PS/2 鼠标） |

**设计要点（事件投递终点为 LVGL `indev`）**：
- **设备发现**：遍历 `/dev/input/event0..N`，用 `evdev` 打开并读能力位：优先 `EV_ABS` + `ABS_MT_POSITION_X/Y`（多点协议 B），退化 `ABS_X/ABS_Y`（单点）。命中多个候选时**报错退出并列出候选**（不静默取第一个，避免在柜面选错设备）。
- **显式指定**：生产 unit 固定 `--touch-device /dev/mupc-touch`；部署侧以 udev 规则建稳定符号链接（见 §12.2）。
- **校准**：`ABS_X/Y` 的 min/max → 线性映射到 `[0, width/height)`。`max<=min` 或 ioctl 失败 → **启动即报错**（不静默用默认值）。CLI 覆盖：`--touch-calib xmin,xmax,ymin,ymax`、`--touch-swap-xy`、`--touch-invert-x`、`--touch-invert-y`（真机现场适配，不写进 core 配置）。
- **单点语义**：本期按单点触摸设计（PRD §1.4）；多点协议只取第一个触点（`ABS_MT_TRACKING_ID` 首个 slot）。
- **投递给 LVGL（`indev.rs` 薄层）**：`lv_indev_create()` → `lv_indev_set_type(POINTER)` → `lv_indev_set_read_cb(cb, user_data)`，`cb` 内把 Rust 侧的 `(pressed, x, y)` 填入 `lv_indev_data_t`（`point`/`state`）。**读取时机**：以 `lv_indev_set_mode(LV_INDEV_MODE_EVENT)` + 事件到达后 `lv_indev_read(indev)` 主动投递（避免 LVGL 内部 30 ms 定时轮询带来的固定延迟）；`lv_conf.h` 的 `LV_USE_EVDEV` **置 0**。
- **命中/z-order/弹层拦截**：由 **LVGL 控件树**负责（`lv_obj` 树 + `lv_layer_top()` 弹层），正确性远优于自研。
- **时延预算**：`poll` 唤醒 ≤1 ms → `lv_indev_read` → LVGL 命中与脏区失效 → `lv_timer_handler` 重绘 ≤30 ms（局部）→ fb blit ≤5 ms ⇒ **按下反馈 ≤100 ms**（PRD TT-04）可达。**真机实测点**（§14 R-05）。
- **误触与滑动**：控件尺寸/间距由**主题样式常量**强制（§5.6）；LVGL 的**滚动容器**（`lv_obj` + `LV_OBJ_FLAG_SCROLLABLE`）内，子对象的 `LV_EVENT_CLICKED` **仅在按下-抬起落在同一对象且未转化为滚动时触发**，满足 TT-11（需真机/离屏验证）；500 ms 防抖在 UI 层按钮回调内实现（记录上次触发时刻）。
- **LVGL 单击判定可调**：LVGL 有 `LV_INDEV_DEF_SCROLL_LIMIT` / `LV_INDEV_DEF_SCROLL_THROW` 等常量（`lv_conf.h`）控制"拖动多远就不算点击"，**该项列入编码前标定**（离屏可先验，真机复核，R-10）。

### 1.3 关键决策 C：显示后端

| 路线 | 依赖/成本 | 结论 |
|------|-----------|------|
| **C-1 framebuffer `/dev/fb0`（选定，默认）** | 沿用既有已在 `canvas.rs::FbCanvas` 落地的 mmap + 像素格式探测逻辑，作为 **`flush_cb` 的像素 sink** | **✅ 默认**。与 §1.1.1.1 的 P-1（自定义 `lv_display` + `flush_cb`）直连；`mupc-display.service` 已含 `video` 组 |
| C-2 DRM/KMS（`/dev/dri/card0`） | **不选（自研）**：本期改用 **LVGL 官方 `LV_USE_LINUX_DRM` 驱动（P-3）**，而非自研 dumb buffer + page flip | ⚠️ **不在本期启用**。仅当真机 `fb0` 不可用时作为 R-03 的处置（LVGL 原生支持，无需自研） |
| C-3 X11 / Wayland | 目标镜像无显示服务器 | ❌（LVGL 侧对应 `LV_USE_X11`/`LV_USE_WAYLAND` 亦关闭） |

> **格式兼容**：因走 P-1（我们自己的 `flush_cb`），`/dev/fb0` 的 `bits_per_pixel` / `red/green/blue` 位偏移由我们在 `FbCanvas` 内读取并做转换（既有逻辑），**与 LVGL 的像素格式支持无关**。LVGL 侧只需把 `LV_COLOR_DEPTH` 与我们要求的目标格式钉死（`lv_conf.h`），渲染结果一律经 `FbCanvas` 转换后落 fb。
>
> **离屏测试后端（`--backend offscreen`）**：同一套 P-1 代码，`flush_cb` 的 sink 由 `FbCanvas` 换成内存 `Vec<u8>`（并可导出 PNG 供人工核对）→ **生产/测试同一条渲染路径**，这是本模块可测性的支柱（§11.1/§11.2）。

### 1.4 关键决策 D：应用架构与通道形态

**进程拓扑保持两进程分离**（数据与控制集中在 `mupcd`，HMI 只做渲染与输入）：

- 理由 1（PRD 硬约束）：PRD §4.4 第 6 条「显示进程**任何情况下**不得直连核间 modbus / PCS 总线」；PRD §1.4「写操作经 mupcd 提供的受控接口完成」。**单进程（把取数/写入搬进 HMI）直接违反 PRD**。
- 理由 2（可靠性）：PRD §4.3「显示进程崩溃/重启不得影响 mupcd」——同进程则 `panic` 即全灭。
- 理由 3（复用）：既有 `DisplayDataProvider`（域值化/量程/一致性）与 `AiIntegrator::soc_display_snapshot` 已在 mupcd 侧正确落地，迁移成本为零。

**通道形态：双监听（读 / 控制分离）**，这是对 PRD §4.4 第 4/5 条与 PL-8 的直接落地：

| 通道 | 端点 | 语义 | 变化 |
|------|------|------|------|
| **读通道（展示）** | `127.0.0.1:9810`（`display.bind_addr`） | **仅 GET，无参数，无副作用**。承载全部实时展示量（F1–F8、F16 联锁状态、F7 告警） | 既有 `GET /v1/display/latest`；本版 **扩展帧内容**（不新增端点，保持客户端简单） |
| **控制通道（受控接口）** | `127.0.0.1:9811`（`display.control_bind_addr`，新） | **读查询（带参、有限额）+ 写操作（校验/幂等/审计/回执）**：配置读写、日志查询、审计查询、联锁读/写 | **v2.0 新增**，独立监听 |

- **为何两个监听而非一个监听的两种路径**：PL-8 要求「展示通道不承载下行写；写操作走独立受控接口」需**可被架构检查**。物理分离使「读通道的代码路径中不存在任何写能力」成为结构事实（不同模块、不同 handler 集合、不同句柄），而非依赖审查者逐行确认；同时控制侧故障/限流不影响展示实时性。
- **为何不并回 `web-api` 的 8080**：PRD §3.7 PL-4/PL-5 要求取消对外 HTTP 面；两个监听**均硬绑 127.0.0.1**（`validate()` 强制回环，非回环即启动报错），对外不可达。
- **为何不选 Unix socket / 共享内存**：见既有 §1.1 论证（Windows 本地开发不可用 / 1Hz 场景过度设计），结论不变——本机开发与 CI 必须能跑通全链路。

### 1.5 关键决策 E：`web-api` 移除方式

**结论：整 crate 删除（`crates/web-api` 从 workspace members 移除）**，其中**仍被复用的类型先行迁出**，然后删除。不保留「内部模块」形态——理由：PRD PL-4/PL-5 要求对外行为消失，而 `web-api` 的全部价值面（Axum 路由 + 会话 + SSE + 静态资源）都与「对外 HTTP」绑定；保留其 crate 只会保留一棵需要持续维护的死依赖树（`axum` / `tower-http` / `jsonwebtoken`）。

**必须先迁出的三处耦合**（这是删除的真实阻碍，不是「删目录」那么简单）：

| 被复用资产 | 现位置 | 新落点 | 影响 |
|------------|--------|--------|------|
| `InterlockApi` trait + `InterlockStatus` / `InterlockSourceStatus` | `web-api/src/app_state.rs`（被 `mupc-core-bin/src/interlock.rs` 实现） | **`display-proto::interlock`**（HMI 契约单一真源） | `interlock.rs` 改 `use`；同时把错误类型结构化（§4.6） |
| `SsePushService` | `web-api/src/sse/`（被 `core-bin/src/startup.rs::SouthSink` 使用） | **mupcd 内 `AlertFeed`**（有界 ring，见 §4.7） | `SouthSink.sse` 字段替换为 `AlertFeed` 句柄 |
| `AuditLogger` / `LogsHandler` / `SystemStatus` | `web-api/src/audit`、`routes/logs.rs`、`routes/status.rs` | **mupcd `ConsoleAuditService` / `LogService`**；`SystemStatus` **删除**（全为占位值） | 见 §4.5 / §4.4 |

完整逐出口迁移归类与受影响文件清单见 **§7**。

### 1.6 复用 / 废弃判定（摘要）

完整清单见 §8。**一句话结论**：
- **复用（升级）**：`display-proto`（帧模型/配置/字段标志）、`local-display::channel`（回环 HTTP 客户端）、`local-display::state`（三态归一/新鲜度纯逻辑）、**`local-display::canvas::FbCanvas`（fb0 mmap + 格式转换 + 逐行写）——升为"`flush_cb` 的像素 sink"（**性质从"被迫自绘的遗留"变为"显示后端的确定组成部分"**，复用度更高）**、`local-display::config`（CLI 解析）、既有的**色板常量**（迁入 LVGL 主题样式）与**字库子集码表**（`font_subset_charset.txt`，**码表复用、产物重建**为 `lv_font_conv` 的 C 字体，见 §1.1.2）；mupcd 侧 `DisplayDataProvider` / `LoopbackHttpPublisher` / `soc_display_snapshot` / `read_three_phase` 全部保留。
- **废弃**：`local-display::layout`（1003 行固定网格自绘）、`local-display::font`（ab_glyph 光栅化与图集）、`local-display::run`（500 ms 定拍自绘主循环）、`Canvas` 的**绘制原语契约**与 `OffscreenCanvas` 的布局用途、`local-display/tests/full_chain.rs`（改为 **LVGL 离屏渲染 + 页面断言**）。
- **复核要点（逐项重判，见 §8.3）**：① `FbCanvas` 从"降级"回到"保留"；② `font.rs` 仍废弃（LVGL 接管文本），但其**码表资产**仍复用；③ 新增**自写绑定层**（不是复用项，是净新增，§1.1.1.2）；④ 既有代码没有任何"输入"资产（原只读屏无触摸），`touch.rs` 是**净新增**。

---

## 2. 架构总览

### 2.1 进程拓扑

```
┌──────────────────────────── 单机（BECG-3568, Linux, 无 X11/Wayland） ────────────────────────────┐
│                                                                                                  │
│  进程 P0 = mupcd（主进程 / 数据 + 控制后端）                                                       │
│  ┌────────────────────────────────────────────────────────────────────────────────────────┐   │
│  │ 既有：核间 modbus(FC04/FC06) · gateway(IEC104) · 策略引擎 · AI(停用) · storage · sys-monitor │   │
│  └───────────┬────────────────────────────────────────────────────────────────────────────┘   │
│              │                                                                                  │
│  ┌───────────▼───────────────────── display_host（既有，扩展）──────────────────────┐  │
│  │ DisplayDataProvider                                                                        │  │
│  │  快拍 1 Hz：SOC 裁决快照 · run_state/三相(intercore) ────────────────────┐                  │  │
│  │  慢拍 3 s：uptime/CPU 温度/内存(system-monitor) · IEC104 连接态(gateway)  ├→ 缓存(Arc<RwLock>)│  │
│  │  快拍 0.5 s：告警(storage.events) · 联锁态(InterlockController) ─────────┘                  │  │
│  │  组帧 v2（含 device / alarms / info / interlock）→ SharedLatest（变更即组帧）               │  │
│  └───────────┬────────────────────────────────────────────────────────────────────────────┘  │
│  ┌───────────▼───────────────── LoopbackHttpPublisher（读，仅 GET）──────────────────────────┐  │
│  │ 127.0.0.1:9810  GET /v1/display/latest                                                     │  │
│  └────────────────────────────────────────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────── ConsoleHost（新，控制，受控接口）───────────────────────────┐  │
│  │ 127.0.0.1:9811                                                                             │  │
│  │  ConfigService  ← CoreConfig 内存副本 + 原子落盘 + watch 热生效                             │  │
│  │  LogService     ← tracing 层 ring(实时) + 日志文件区间扫描(历史, 限额)                       │  │
│  │  ConsoleAuditService ← JSONL 追加 + 既有审计链双写（SHA-256） + 分页/筛选查询               │  │
│  │  InterlockOps   ← InterlockController（结构化拒绝原因）                                     │  │
│  │  管线：参数校验 → 幂等(request_id) → 审计(前置 fail-closed) → 执行 → 回执                    │  │
│  └───────────┬────────────────────────────────────────────────────────────────────────────┘  │
│              │                                                                                  │
│  进程 P1 = mupc-local-display（HMI，独立进程，systemd Restart=always, RestartSec=1）              │
│  ┌───────────▼────────────────────────────────────────────────────────────────────────────┐    │
│  │ LVGL v9（C）+ 自写 Rust FFI 薄层（lvgl-sys / src/lvgl/*，§1.1.1.2）                        │    │
│  │  event loop: poll(evdev_fd, lv_timer_handler 返回的剩余时间) ─ 三源合流                     │    │
│  │   ① Rust evdev 触摸 ──→ 自定义 lv_indev.read_cb ──→ lv_indev_read()                       │    │
│  │   ② 读通道轮询(500ms, GET /v1/display/latest) ──→ UiState ──→ lv_label/lv_bar setter        │    │
│  │   ③ lv_timer_handler() / lv_tick_set_cb()（动画 / 超时回归 / 防抖）                          │    │
│  │  lv_timer_handler() → LVGL 脏区重绘 → flush_cb → /dev/fb0（FbCanvas 格式转换 + 区域写）      │    │
│  │  写操作 ──→ ConsoleClient(POST /v1/console/*) ──→ 结果回填 UiState + lv_msgbox/lv_toast     │    │
│  └─────────────────────────────────────┬──────────────────────────────────────────────────┘    │
│                                        │ HDMI                                                     │
│                                   ┌────▼──────┐                                                   │
│                                   │ 8 寸 1024×768 触摸屏 │                                          │
│                                   └───────────┘                                                   │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

**边界要点（对应 PRD）**：
- HMI 进程**只读展示通道 + 只写控制通道**；**无任何直连核间/南向/北向的代码路径**（PRD §4.4.6）。
- 写操作**永不**经读通道（PRD §4.4.4 / PL-8）。
- 两个监听均强制回环（`validate()` 拒绝非 127.0.0.1）。
- HMI 可先于 mupcd 启动（显示「初始化中」占位）；mupcd 上线 ≤1 s 转实时（PRD §4.3）。
- mupcd 不 spawn / 不管理 HMI 子进程（生命周期归 systemd，PRD §4.3.1 属系统集成侧）。

### 2.2 crate 与模块拓扑

| 载体 | 类型 | 职责 | 变化 |
|------|------|------|-----------|
| `crates/display-proto`（`mupc_display_proto`） | lib | **HMI 契约单一真源**：`frame`（读帧）/ `control`（写请求·回执·错误码）/ `interlock` / `log` / `audit` DTO + `DisplayConfig` | **扩展**（既有仅 `frame`+`config`） |
| `crates/local-display`（`mupc_local_display`） | lib + bin | HMI 进程：LVGL 页面/控件/主题、自写 FFI 薄层、`lv_display`/`lv_indev` 后端、Rust evdev 触摸、通道客户端、页面状态 | **重构**（渲染层换 LVGL + 新增自写绑定层 `lvgl-sys`/`src/lvgl/`；`channel`/`state`/`config`/`FbCanvas` 保留） |
| `crates/local-display/lvgl-sys`（内部子 crate，`mupc-lvgl-sys`） | lib（`-sys`） | LVGL v9 C 源码的 `cc` 编译 + `bindgen` 原始绑定（§1.1.1.2） | **新增** |
| `mupc/vendor/lvgl/`（或 git submodule） | C 源码 | LVGL v9.5 官方源码，**pin tag**（§12.1） | **新增** |
| `crates/mupc-core-bin`（模块 `display_host`） | bin 内模块 | 读通道：采集/组帧/发布 | **扩展**（新增慢拍采集与段） |
| `crates/mupc-core-bin`（模块 `console_host`，新） | bin 内模块 | 控制通道：Config / Log / Audit / InterlockOps / HTTP | **新增** |
| `crates/web-api` | — | — | **删除**（§7） |

> **不新建第三个 crate 的取舍**：`ConsoleHost` 放 `mupc-core-bin` 模块内（与 `display_host` 同构），避免为「一个进程内的两组 HTTP handler」再拆 crate（KISS）。

---

## 3. 通道协议（读通道 + 控制通道）

> 契约类型全部定义在 `display-proto`，两侧 + 测试桩共享同一真源。所有 JSON 字段 `snake_case`；枚举 `snake_case` 字符串（`RunState` 例外，维持既有判别数 u8 传输）。
> **前向兼容**：新增段一律 `#[serde(default)]`；客户端对未知字段容忍（已测）。

### 3.1 读通道：`DisplayFrame` v2

- 端点：`GET /v1/display/latest`（`127.0.0.1:9810`），无参数、无鉴权（仅回环）、`Connection: close`。
- 语义：状态在服务端，**每请求返回当前最新帧**；允许丢中间帧（PRD §4.4.1）。
- 发布节拍：**主拍 1 s** ∪ **慢拍段内容变更即组帧**（`Notify` 唤醒，合并窗口 ≥250 ms；见 §4.2.1）；`device` / `alarms` / `interlock` 段由**独立采集任务**写入缓存（**3 s / 0.5 s / 0.5 s**），组帧时读缓存（**帧路径零阻塞 I/O、零 DB 查询**）。

```rust
// crates/display-proto/src/frame.rs
pub const PROTO_VERSION: u8 = 2;                 // v1 → v2（段扩展）

/// 一帧展示数据（1 Hz 发布；子段各自带采集时刻与可用性）
pub struct DisplayFrame {
    pub version: u8,
    pub seq: u64,
    pub ts_ms: u64,                              // 组帧时刻（新鲜度判据，沿用 DEFAULT_STALE_MS=2000）

    // ── F1–F5（字段原样保留，语义不变）──
    pub soc: Option<f64>,
    pub soc_source: SocSource,                   // Bms / PcsReg1010 / Lost
    pub soc_flag: FieldFlag,
    pub run_state: Option<RunState>,
    pub pcs_online: bool,
    pub p_phase: [Field; 3],
    pub p_total: Field,
    pub i_phase: [Field; 3],
    pub inconsistency: bool,

    // ── v2 新增分节（全部 serde(default)，旧客户端可容忍）──
    #[serde(default)] pub device: DeviceSection,      // F6 装置整体状态
    #[serde(default)] pub alarms: AlarmsSection,      // F7 告警列表
    #[serde(default)] pub info: InfoSection,          // F8 版本与装置信息
    #[serde(default)] pub interlock: InterlockSection,// F16 联锁状态
}

/// F6 装置整体状态（3 s 采集；端到端 ≤3.85 s，见 §4.2.1）
#[derive(Default)]
pub struct DeviceSection {
    pub ts_ms: u64,
    pub uptime_secs: Option<u64>,
    pub cpu_temp_c: Option<f64>,
    pub mem_used_pct: Option<f64>,
    pub iec104: LinkState,          // 调度主站链路
    pub intercore: LinkState,       // 核间链路
    pub hmi_channel: LinkState,     // 跨进程数据通道（HMI 侧自判，见 §5.5；服务端给 Unknown）
    pub control_source: ControlSource,
}

#[serde(rename_all = "snake_case")]
pub enum LinkState { Connected, Connecting, Disconnected, NotConfigured, Unknown }

#[serde(rename_all = "snake_case")]
pub enum ControlSource { LocalStrategy, AiDisabled /* 固定文案「AI 引擎已停用…」*/, Unknown }

/// F7 告警（0.5 s 采集 + 变更即组帧；端到端 ≤1.35 s，见 §4.2.1；items ≤10，时间倒序）
#[derive(Default)]
pub struct AlarmsSection {
    pub ts_ms: u64,
    /// 源可用性（EDGE-09：false → 屏显「告警源不可用」，**不得**显「无告警」）
    pub available: bool,
    pub items: Vec<AlarmItem>,
}
pub struct AlarmItem { pub ts_ms: u64, pub level: AlarmLevel, pub message: String }
#[serde(rename_all = "snake_case")]
pub enum AlarmLevel { Error, Warn, Info }

/// F8 版本与装置信息（页面加载一次性）
#[derive(Default)]
pub struct InfoSection {
    pub firmware_version: String,
    pub build_time: Option<String>,   // None → 屏显「未提供」（EDGE-16）
    pub model: Option<String>,
    pub serial: Option<String>,
    /// 服务监听口径：读/控制通道**仅回环 127.0.0.1**（安全红线）。
    /// 恒为 `LoopbackOnly`，作为「对外不可达」的机器可读声明；UI 展示时**必须与
    /// `mgmt_ipv4` 分列**，不得让现场据此认为 HMI 接口可从远端访问。
    #[serde(default)] pub service_scope: ServiceScope,
    /// 设备管理 IP：`getifaddrs` 取首个 UP 的**非回环** IPv4。
    /// 语义是「该装置在管理网上的地址」，与 `service_scope`（本机服务只监听回环）是**两个不同概念**。
    /// 不可得 → None → 屏显「未提供」（EDGE-16 同口径，不臆造）。
    #[serde(default)] pub mgmt_ipv4: Option<String>,
}

/// 服务可达范围（当前恒为 LoopbackOnly；枚举化以便未来若开管理面时有显式声明点）
#[derive(Default, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceScope { #[default] LoopbackOnly }

/// F16 联锁状态（0.5 s 采集 + 变更即组帧；端到端 ≤1.35 s，见 §4.2.1）
#[derive(Default)]
pub struct InterlockSection {
    pub ts_ms: u64,
    /// 状态源可用性（IL-01.6：false → 显「联锁状态不可用」，**不得**显「未联锁」）
    pub available: bool,
    pub enabled: bool,
    pub latched: bool,
    pub stop_failed: bool,
    pub sources: Vec<InterlockSourceItem>,  // {name, tripped}
    /// ⚠️ 语义名优先，不绑 DO 号（PRD F16 与现有代码注释的 DO1/DO2 归属不一致，见 §6.4 备注）
    pub fault_lamp: Option<bool>,           // None = 未知
    pub run_lamp: Option<bool>,
    /// UI 用于提示「须保持 N 秒」等释放前置条件
    pub release_hold_secs: u64,
}
pub struct InterlockSourceItem { pub name: String, pub tripped: bool }
```

**JSON 兼容性说明**：v1 客户端（旧渲染进程）收到 v2 帧时因字段全部 `#[serde(default)]` 仍可反序列化（忽略新段）；v2 客户端收 v1 帧时新段取 `Default`（`available=false` → 屏显「不可用」，**恰好**符合 §9 降级语义，不会伪装成正常）。

### 3.2 读通道端点清单

| 端点 | 方法 | 参数 | 返回 | 用途 |
|------|------|------|------|------|
| `/v1/display/latest` | GET | — | `DisplayFrame` v2（200）/ 503 未就绪 / 404 | P1、P4（状态）、P6 的全部实时量 |

> 读侧**只有这一个端点**——保持「小、快、无副作用、客户端简单」。日志/审计/配置查询均为**带参、有限额、可能昂贵**的受控读，归控制通道（§3.4）。

### 3.3 控制通道：通用信封与管线

- 端点：`127.0.0.1:9811`（`display.control_bind_addr`），全部为 `/v1/console/*`。
- 方法约定：**查询用 GET（参数在 query）**，**写操作用 POST（JSON body）**。

```rust
// crates/display-proto/src/control.rs

/// 写操作请求信封（所有 POST 共用）
pub struct ControlRequest<T> {
    /// 客户端生成的 UUID v4；服务端按 (op, request_id) 做幂等去重（防重放/防重复生效）
    pub request_id: String,
    /// 客户端签发时刻（Unix ms）；服务端拒绝 |now − issued_at| > 30_000 的请求（防重放窗口）
    pub issued_at_ms: u64,
    /// 操作名（与路径末段一致，服务端校验，防误路由）
    pub op: String,
    pub payload: T,
}

/// 回执（所有写操作共用）
pub struct ControlResponse<T> {
    pub request_id: String,
    pub ok: bool,
    pub code: ControlCode,
    /// 人读消息；UI 直接展示（失败时即 EDGE-10/EDGE-12 要求的「具体原因」）
    pub message: String,
    /// 成功时的生效回执（新值 / 新状态），供 UI 立即刷新（不等下一帧）
    pub applied: Option<T>,
    /// 字段级校验错误（配置页逐字段标红用，CF-02 要求「具体错误」）
    pub field_errors: Vec<FieldError>,
    /// 审计记录 ID（成功与失败均返回；便于现场对拍）
    pub audit_id: Option<String>,
    /// 幂等命中标记：true 表示本条为重复请求，返回的是首次执行的原始结果
    pub duplicate: bool,
    pub at_ms: u64,
}

pub struct FieldError { pub field: String, pub reason: String }

#[serde(rename_all = "snake_case")]
pub enum ControlCode {
    Ok,
    /// 前置条件不满足（联锁触发源未复位 / 保持时间不足 / 处于 latch 态）→ 必须明示原因（EDGE-12）
    RejectedPrecondition,
    /// 参数校验失败（越界 / 非法枚举）→ 逐字段原因（EDGE-10）
    RejectedValidation,
    /// 执行/落盘/生效失败（EDGE-10：装置须保持原配置运行，不得半生效）
    ApplyFailed,
    /// 审计不可写（fail-closed，见下）
    AuditUnavailable,
    /// 后端不可用（联锁未启用 / 日志或审计源不可用）
    Unavailable,
    /// 同 request_id 请求仍在处理中
    Busy,
    Internal,
}
```

**控制管线（所有写操作共用，顺序固定）**：

```
1. 路由与方法校验（路径 →
2. 信封解析：request_id 非空 / issued_at_ms 在 ±30 s 窗口内 → 否则 RejectedValidation
3. 幂等查表：(op, request_id)
     - 命中且已完成 → 返回首次的原始 ControlResponse（duplicate=true，ok 不变）※真幂等
     - 命中且处理中 → Busy
     - 未命中 → 占位后继续
4. 参数校验（字段级） → 失败 RejectedValidation（含 field_errors）
5. 审计占位写入（intent：op + 请求摘要 + operator=local-console）
     - 失败 → AuditUnavailable 并终止（fail-closed，见下）
6. 执行（配置落盘 / 联锁释放 / 授权）
7. 结果审计（outcome：before/after/result/reason），返回 audit_id
8. 回执（含 applied 新值）
```

**审计 fail-closed 裁决（设计裁决，需 PM 知悉）**：无登录（T-3）后，**审计是唯一的操作凭据**。因此「审计不可写 → 拒绝执行写操作并按 `AuditUnavailable` 明示」。
- 立场：若允许「审计失败但操作生效」，则 T-3 的补偿模型（无身份把关 → 以审计兜底）在技术上失效。
- 同时，审计写失败本身以 `tracing::error!` 落 journal 作为**第二凭据**（syslog 不受磁盘满以外因素影响）。
- **需 PM 确认的边界**：若认为「联锁释放属恢复安全态的操作、其可用性优先于留痕」，则联锁释放/授权可改为 fail-open（仍尽力审计 + journal 双写），配置保存保持 fail-closed。**本设计默认全部 fail-closed**（更严格），改动仅一行分支。

**二次确认的技术落点（T-3 落地）**：

| 要求 | 落点 | 说明 |
|------|------|------|
| 「将执行的操作 + 影响范围 + 前后值摘要」 | **UI 层**（LVGL `lv_msgbox`/自定义弹层） | 前端固有；后端不重复实现确认流程 |
| 默认焦点在「取消」 | **UI 层** | LVGL 弹层把「取消」置为 `lv_group` 的默认聚焦对象（`lv_group_focus_obj`） |
| 防重放 | **后端**（`issued_at_ms` 30 s 窗口 + `request_id` 幂等表） | 30 s TTL 的有界 LRU（容量 256，足够现场操作） |
| 防误触/防抖 | **UI 层**（500 ms 内按钮禁用） + **后端**（幂等兜底） | 双层 |
| 审计留痕 | **后端**（ConsoleAuditService） | 成功与失败均写；operator 固定 `local-console` |
| 「权限」 | **无**（T-3 无登录/会话/PIN/RBAC） | 不实现任何鉴权中间件，避免造出无用的"占位鉴权"（`RequireRole` 占位实现随 web-api 一并删除，技术债 U-01 对本模块不再适用） |

### 3.4 控制通道端点清单

| 端点 | 方法 | 请求 | 返回 | 承接组件 |
|------|------|------|------|----------|
| `/v1/console/config` | GET | — | `ConfigView`（分组 + 字段元数据 + 当前值） | `ConfigService` |
| `/v1/console/config/apply` | POST | `ConfigPatch { changes: Map<String,Value>, from: "edit"\|"reset_default" }` | `ConfigView`（新值） | `ConfigService` |
| `/v1/console/logs` | GET | `levels`(多值) `targets`(多值) `range=1h\|24h\|custom` `from`/`to`(ms) `cursor`(可选) `limit`(≤200) | `LogPage { entries, next_cursor, has_more, range_too_large }` | `LogService` |
| `/v1/console/logs/targets` | GET | — | `Vec<String>`（模块选项，≤50） | `LogService` |
| `/v1/console/audit` | GET | `from`/`to`(ms) `ops`(多值) `page`(1-based) `page_size`(=20) | `AuditPage { entries, page, page_size, has_more, newest_ts_ms, available }` | `ConsoleAuditService` |
| `/v1/console/audit/ops` | GET | — | `Vec<{op, label}>`（操作类型选项） | `ConsoleAuditService` |
| `/v1/console/interlock/release` | POST | `InterlockOpPayload { observed_latched, observed_sources: Vec<String> }` | `InterlockOpAck { latched, stopped }` | `InterlockOps` |
| `/v1/console/interlock/ack_m1` | POST | `InterlockOpPayload`（同上） | `InterlockOpAck` | `InterlockOps` |

> **多值 query 的线上约定（2026-09-15 补）**：`levels` / `targets` / `ops` 一律用**重复键**编码（如 `levels=error&levels=warn`），不用逗号拼接。`ConsoleClient`（`src/console.rs`）按此发送，`mupcd` 侧须按同口径解码。
>
> **GET 失败的错误通道（2026-09-15 补）**：§3.3 的 `ControlResponse` 信封**只用于 POST 写操作**；**GET 查询返回裸 DTO**（本表"返回"列即其类型），失败时**不走信封**，由 **HTTP 状态码**表达（`ConsoleClient` 落为 `Error::HttpStatus`：非 2xx 即算通道失败，**不**把错误体当数据解析）。⇒ `mupcd` 侧 GET 的错误响应**必须给非 2xx**，不得用 `200` + 错误体。
> **单条日志消息长度上限（2026-09-15 补）**：`LogPage` 的 `LogEntry.message` 在 `display-proto` 中**未设上限**，而 `ConsoleClient` 的响应体上限按「`LOG_PAGE_LIMIT_MAX`（200）× 单条 ≤ 1 KiB」推算 ⇒ **`mupcd` 的日志服务必须限制单条消息长度 ≤ 1 KiB**（超出即截断并在条目上标注），否则两侧上限须一起上调。

**`observed_*` 的作用**：UI 在弹层中展示的是「它看到的联锁态」。提交时携带该观测值，后端与服务端当前态比对——若已变化（例如触发源刚被复位或刚被触发），返回 `RejectedPrecondition` + 消息「联锁状态已变化，请刷新后重试」。这是**无并发控制场景下的乐观并发检查**，防止"基于过期画面执行破坏性操作"。

**`ConfigView` 字段元数据（关键，决定 UI 无需硬编码）**：

```rust
pub struct ConfigView {
    pub groups: Vec<ConfigGroup>,
    pub revision: u64,
    /// 【新·§4.3.2.1】最近一次落盘的写模式：
    /// `TextPreserve` = 保留式编辑（正常路径，注释/未建模键未动）；
    /// `FullRewrite`  = 无法定位目标键而整体序列化回写 → **既有注释已丢失**，UI 须 Toast 明示。
    pub write_mode: WriteMode,
}
#[serde(rename_all = "snake_case")]
pub enum WriteMode { TextPreserve, FullRewrite }
pub struct ConfigGroup { pub id: String, pub label: String, pub fields: Vec<ConfigField> }
pub struct ConfigField {
    pub key: String,             // 稳定键，如 "system.log_level"
    pub label: String,           // 中文标签
    pub kind: ConfigKind,        // Ipv4 / U16{min,max,step} / U64{min,max,step} / Enum{options}
    pub value: serde_json::Value,
    pub default: serde_json::Value,
    pub unit: Option<String>,
    pub requires_reconnect: bool, // true → 弹层须提示「生效时链路将短暂中断」
    pub editable: bool,          // false → UI 只读展示（步进器/选项均 disabled）。
                                 // 用于 display.bind_addr / display.control_bind_addr：回环是安全红线（PL-4），
                                 // 不可经屏修改（改了即自断通道或违反回环约束），只能展示（见 §6.2 / §6.6）。
}
pub enum ConfigKind { Ipv4, U16 { min: u16, max: u16, step: u16 }, U64 { min: u64, max: u64, step: u64 }, Enum { options: Vec<OptionItem> } }
pub struct OptionItem { pub value: String, pub label: String }
```

> `kind` 直接驱动 LVGL 的控件选择（`Ipv4` → 四段**受约束步进**；`U16/U64` → 受约束步进器；此二者均为 **`lv_btn` + `lv_label` 组合**（`−` / 值 / `＋` 三件）**弃 `lv_spinbox`**（理由见 §5.6 F12 行）；越界值在控件层不可达——`value == min` 时 `−` 置 `LV_STATE_DISABLED`，`value == max` 时 `＋` 同理（TT-03）；`Enum` → `lv_dropdown` / `lv_buttonmatrix`），**满足 F12「零键盘」**：后端字段表里不存在「自由文本」这一 kind，因此 UI **无处可放文本输入框**——把「零文本输入」变成类型系统层面的约束，而不是纪律要求。

### 3.5 通道可靠性对照（PRD §4.4）

| PRD §4.4 条款 | 落实 |
|---------------|------|
| 1 允许丢帧、不得静默损坏 | `seq` 单调 + JSON 定长体（`Content-Length`）；解析失败丢弃并计数（不崩溃）；帧内 `version` 校验 |
| 2 断连 ≤3 s 感知 / 恢复 ≤1 s | 读通道连续失败 >3 s → `ChannelStatus::Down`（整屏降级）；恢复后首次成功 GET ≤500 ms 回实时（沿用既有 `state.rs` 逻辑） |
| 3 畸形帧防护 | 客户端对帧大小设上限（64 KB）、`serde` 失败丢弃、`version` 不匹配告警；`SharedLatest` 服务端 `Mutex` 毒化不 panic（已有 `unwrap_or_else(into_inner)` 范式） |
| 4 展示通道单向 | 读 handler 仅实现 `GET /v1/display/latest`，其余方法与路径 404/405；**代码路径中无任何 `POST` 处理**（架构检查项 PL-08） |
| 5 写操作独立受控接口 | 独立监听 9811 + 独立模块 `console_host`；校验/幂等/审计/失败原因回传齐备（§3.3） |
| 6 禁止直连 | HMI 进程依赖图中**不存在** `mupc-intercore` / `mupc-southd` / `mupc-gateway`（在 §11.4 以依赖断言测试固化） |

---

## 4. mupcd 侧改动

### 4.1 数据源落实（PRD §6.2 T-5 五项逐项结论）

这是 PRD 明确留给设计阶段的**待确认清单**，逐项给出定位与结论。**含两项"现状即不可用"的诚实发现（T-5 #3 与 #4 的严重程度高于 PRD 描述）**。

| # | PRD 待确认项 | 核对结论（代码事实） | 设计落点 | 风险 |
|---|--------------|----------------------|----------|------|
| **1** | **IEC 104 连接状态真源** | 现状 `web-api::StatusHandler` 硬编码 `"unknown"`（占位）。真实状态在 `mupc_gateway::iec104::connection::Connection::state`（`Disconnected/Connecting/WaitingStartDt/Connected/Stopped`），但 `Iec104Server` **对外只暴露 `connection_count()`**，无状态查询 | **gateway crate 新增** `Iec104Server::link_state() -> LinkState`：聚合内部连接表（任一连 `Connected` → `Connected`；有连接但均未 `Connected` → `Connecting`；已启动且无连接 → `Disconnected`；未配置/未启动 → `NotConfigured`）。改动**局限在 `gateway/src/iec104/server.rs`**（+1 方法与枚举），不触碰协议逻辑 | 低 |
| **2** | **intercore 连接状态真源** | ✅ **已可得**：`IntercoreClient::is_connected()` 已被 `display_host` 使用 | 直接复用，写 `DeviceSection.intercore` | 无 |
| **3** | **告警汇聚点** | ⚠️ **诚实结论：PRD 假设的 `AlertManager` 不可用。** `mupc_security::alarm::AlertManager` 在全仓库**没有任何实例化点**（仅 `security/src/lib.rs` re-export）；其 `AlertType` 全为安全类（证书/隧道/合规/安全启动），**不含运行类告警**；且无任何模块向其 `raise()`。**它是死代码。** 真实运行告警的现有载体是 `mupc_storage::EventRepository` 的 `SystemEvent`（`core-bin/src/startup.rs` 已在写：`south_station.<id>.offline/online` 等） | **本期以 `storage.events`（SystemEvent）为 F7 唯一真源**：mupcd 新增 **0.5 s** 采集任务查最近 10 条（倒序）写入缓存，并在内容变化时立即唤醒组帧（F7.3 ≤2 s 的前提，见 §4.2.1）；查询失败 → `alarms.available=false` → 屏显「告警源不可用」（EDGE-09 精确落地）。**可选增强（不在本期承诺）**：新增 mupcd 内 `AlertFeed`（有界 ring + `tracing` WARN/ERROR 层 + 南向事件双写）以覆盖"未落库也上屏" | **中**（需 PM 知悉：原 08 的告警源是空壳；本模块上屏的告警仅是"已落库的系统事件"） |
| **4** | **配置真实落点与生效链路** | ⚠️ **诚实结论：现状不存在任何可用的配置读写链路。** `web-api::routes::config::AppConfig` 是**进程内内存值**（`Arc<RwLock<>>`，启动时构造 `Default`），**既未落盘、也未被任何模块消费**；真实参数在 `mupc_core_config.yaml` → `CoreConfig`，在 `startup.rs` 启动时读取一次并分发，**全仓无热重载机制**（`grep reload/watch` 在 core-bin 无命中） | **新建 `ConfigService` 子系统**，详见 §4.3（本模块**最大**的净新增工作） | **高** |
| **5** | **编译时间戳** | 现状 `web-api::StatusHandler` 的 `build_time` 与 `firmware_version` **取同一常量** `env!("CARGO_PKG_VERSION")`（占位） | `mupc-core-bin/build.rs` 发出 `cargo:rustc-env=BUILD_TIMESTAMP=<RFC3339>`（取 `SOURCE_DATE_EPOCH` 优先，保证可复现构建），代码用 `option_env!("BUILD_TIMESTAMP")`；`InfoSection.build_time: Option<String>`，取不到即 `None` → 「未提供」 | 低 |

**新增项（PRD §6.2 未列但设计必须给）**：

| 需求项 | 真源 | 结论 |
|--------|------|------|
| F6 uptime | `MetricsStore` 启动时刻 或 `std::time::Instant`（需跨模块统一：以 `mupcd` 进程启动时刻为准） | 可得 |
| F6 CPU 温度 | `mupc_system_monitor::collectors::TemperatureCollector`（`TemperatureMetrics.cpu_temp_c`） | 可得；需确认 `MetricsStore` 已启动（`startup.rs` 步骤 12 已在跑） |
| F6 内存使用率 | `MemoryCollector`（`MemoryMetrics`） | 可得 |
| F6 跨进程数据通道状态 | **HMI 侧自判**（`ChannelStatus`），mupcd 侧给 `Unknown` | 见 §5.5 |
| F6 当前控制源 | `AiIntegrator::engine_status()` / `is_local_priority()`；AI 停用期 → 固定语义 `LocalStrategy` | 可得（既有已用） |
| **F8 型号 / 序列号** | Linux：`/proc/device-tree/model`（型号）；序列号**无可靠真源** | **序列号显「未提供」**（EDGE-16，不臆造）；型号取 device-tree，取不到亦「未提供」 |
| **服务监听口径 + 设备管理 IP** | `ServiceScope::LoopbackOnly` 为常量，与 `display.bind_addr` / `control_bind_addr` 的**回环硬校验同源**（§4.9）；`mgmt_ipv4` 取 `getifaddrs` 首个 UP 的**非回环** IPv4 | 可得；两字段语义分离，展示口径见 §6.6（P6 分两行）与 §6.2（回环地址只读） |
| F16–F18 联锁 | `core-bin/src/interlock.rs::InterlockController`（已实现 `InterlockApi`） | 可得，需错误类型结构化（§4.6） |
| PL-1/PL-2 审计 | `mupc_security::audit::AuditLogger`（JSONL + 哈希链，**现网为 SHA-256 实现**——`security/src/audit.rs` 注释自陈「当前使用 SHA-256 替代 SM3，Phase 2+ 替换为国密 SM3」；本设计按事实表述，不称 SM3） | 可得；需新增本机控制台条目 schema（§4.5） |
| F10 日志 | `common::logging` `RollingFileAppender`（DAILY）→ `/var/log/mupc/mupc.log`（JSON 行：`timestamp`/`level`/`target`/`fields.message`） | 可得；**必须重构筛选路径**（选项式 + 限额，去掉关键字；见 §4.4） |

### 4.2 读通道扩展：`DisplayDataProvider` 慢拍采集

在既有 `display_host.rs` 之上新增三个**独立 tokio 任务**（互不阻塞、各自失败各自降级），写入共享缓存；**并在缓存内容发生变化时主动唤醒组帧任务**——后者是 F7.3 / F16.5「≤2 s 上屏」的达成机制：

```
DisplayDataProvider（主拍 publish_ms=1 s，已有逻辑；新增「内容变更即组帧」唤醒路径）
  └ 组帧时读缓存（不阻塞、不做 I/O）：
      device_cache:  Arc<RwLock<DeviceSection>>   ← 慢拍任务 A（3 s）
      alarms_cache:  Arc<RwLock<AlarmsSection>>   ← 慢拍任务 B（0.5 s）
      interlock_cache: Arc<RwLock<InterlockSection>> ← 慢拍任务 C（0.5 s）
      info:           Arc<OnceLock<InfoSection>>  ← 启动时一次性
  组帧触发源 = ① 主拍 tick（1 s 心跳） ∪ ② 慢拍任务写缓存后的变更通知（Notify）
```

| 任务 | 节拍 | 数据来源 | 失败降级 |
|------|------|----------|----------|
| A 装置状态 | **3 s**（`device_poll_ms`） | `system-monitor`（温度/内存）+ 进程 uptime + `Iec104Server::link_state()` + `IntercoreClient::is_connected()` + `AiIntegrator`（控制源） | 单字段 `None` → UI 显「未知」；**不得**显为「正常」（F6.5） |
| B 告警 | **0.5 s**（`alarm_poll_ms`） | `storage.events` 最近 10 条（倒序） | 查询失败 → `available=false` |
| C 联锁 | **0.5 s**（`interlock_poll_ms`） | `InterlockController::status()` | 未注入（`io.enabled=false`）→ `available=true, enabled=false`；调用失败 → `available=false` |

**为何告警 / 联锁取 0.5 s 而非 2 s**：节拍**直接进入端到端时延链条**（§4.2.1）。原 2 s 节拍叠加 1 s 组帧与 500 ms 轮询后最坏达 **3.0–3.5 s 上屏**，**不满足 F7.3 / F16.5 的 ≤2 s**（初审正确指出）。取 ≤1 s 的上限只留极小余量，故设计取 **0.5 s**；其成本为一次本地 `SQLite LIMIT 10` 查询与一次**纯读**的 `status()` 调用，可忽略。

**为何装置状态取 3 s 而非 5 s**：F6.3 的 ≤5 s 是**端到端上屏**口径；5 s 采样 + 0.25 s 组帧 + 0.5 s 轮询 + 0.1 s 渲染 = **5.85 s 已越界**（初审同风险提示）。3 s 采样使最坏 **3.85 s** 达标并留 1.15 s 余量。

> **为何联锁采集不直接复用 `io.poll_ms`（默认 100 ms）**：`InterlockController` 内部已有 DI 轮询；本任务只做**状态读取**（`status()` 为纯读），取 `interlock_poll_ms`（默认 500 ms）即可满足 F16.5「变化 ≤2 s 上屏」，无需以 100 ms 直连轮询放大 CPU。

**「内容变更即组帧」的约束（不得破坏既有不变量）**：

1. **帧路径仍零阻塞 I/O**：变更判据在**慢拍任务侧**完成（新段与缓存旧值序列化后比较，不等才算变更），组帧任务只读内存缓存——§2.1「帧路径零阻塞 I/O、零 DB 查询」不变。
2. **合并窗口 `min_publish_interval_ms ≥ 250 ms`**：突发多次变更合并为一次发布 ⇒ 发布率上界 = `max(1 Hz 主拍, 4 Hz 突发)`，慢源抖动不会打爆读通道，也不违背 D6「帧率不被慢源拖累」的初衷。
   > ⚠️ **【A-2 裁定 · 2026-09-17】本约束与 §4.9/§11.1 原先的 `[100, …]` 打架**（同一份设计两节口径不一致）。现已由契约统一到 **250**：`display-proto/src/config.rs::MIN_MERGE_WINDOW_MS = 250` 是**唯一真源**，本节与 §4.9/§11.1 一律**以契约 250 为准**。
3. `seq` 单调、`ts_ms` 仍取**组帧时刻**、`DEFAULT_STALE_MS=2000` 语义均不变（帧更密只会让"过期"判定更保守）。
4. 慢拍任务全部失败时退化为「1 Hz 主拍 + 各段 `available=false`」，与既有行为一致。

#### 4.2.1 上屏时延追踪（F6.3 / F7.3 / F16.5 —— 端到端拆解）

链路固定四段：**采样 → 组帧（含发布）→ HMI 轮询取帧 → 渲染上屏**。

| 指标（验收 ID） | ① 采样 | ② 组帧 | ③ HMI 轮询 | ④ 渲染 | **最坏合计** | 目标 | 余量 |
|-----------------|--------|--------|------------|--------|--------------|------|------|
| **F7.3 新增告警 ≤2 s**（ST-16） | ≤0.5 s（`alarm_poll_ms`） | ≤0.25 s（变更即组帧，受合并窗口约束） | ≤0.5 s（`--poll-ms` 默认 500） | ≤0.1 s（§10 重绘预算） | **≤1.35 s** | ≤2 s | **0.65 s** |
| **F16.5 联锁变化 ≤2 s**（IL-01 / IL-02） | ≤0.5 s（`interlock_poll_ms`） | ≤0.25 s | ≤0.5 s | ≤0.1 s | **≤1.35 s** | ≤2 s | **0.65 s** |
| **F6.3 装置状态刷新 ≤5 s** | ≤3.0 s（`device_poll_ms`） | ≤0.25 s | ≤0.5 s | ≤0.1 s | **≤3.85 s** | ≤5 s | **1.15 s** |

**落地约束（下列任一项被改动即视为破坏上屏时延达标，须同步更新本表并在评审中复算）**：

1. `alarm_poll_ms ≤ 1000`、`interlock_poll_ms ≤ 1000`（设计取 500）；
2. `device_poll_ms ≤ 4000`（设计取 3000）；
3. HMI 读帧轮询 `--poll-ms ≤ 500`（默认 500，HMI 侧 CLI 硬校验，见 §5.5）；
4. **慢拍段必须走「变更即组帧」**——若退化为纯 1 Hz 主拍，F7.3 / F16.5 最坏变为 `0.5 + 1.0 + 0.5 + 0.1 = 2.1 s`，**超出 ≤2 s**（这正是初审判定的失败算式，须在实现与回归中钉死）；
5. `mupcd` 侧 `CoreConfig::validate()` 对 1/2/`min_publish_interval_ms` 做**硬校验**（§4.9），非法配置**启动即报错**，不留"性能调优空间"的解释余地。
6. **`publish_ms ≤ 4000`**（契约 `MAX_PUBLISH_MS` = 4000）——`publish_ms` 是本表**主拍路径**的周期项：主拍一旦慢于最慢的一段采集（= 约束 2 的 4000），本表的拆解即失效（瓶颈只剩主拍自己）。该上界对"更慢的主拍"**没有验收意义**，它的作用是把 `min = publish = 60_000` 这类**退化组合**（端到端 ≈61 s，远超验收 ≤2 s）**挡在启动期**，而不是留到现场用手感发现。取 4000 与约束 2 **同量级**（两条各自独立、服务不同指标：本条服务本节 ≤2 s，约束 2 服务 F6.3 ≤5 s）。

**验证**：`display_host` 单测以**假时钟**推进（不 `sleep`）断言「缓存写入 → 发布」的唤醒时延与发布率上界（≤4 Hz）；集成测试（§11.1 双通道集成）断言注入一条告警后 ≤1.5 s 内新帧含该条；真机以时间戳探针复核（§14 R-05）。

### 4.3 配置写入的真实落点与生效链路（T-5 #4 —— 本模块最大净新增项）

#### 4.3.1 现状与结论

| 事实 | 影响 |
|------|------|
| `web-api::AppConfig` 是内存占位、**未被任何模块消费** | 原 Web 配置页即使"保存成功"也**从未生效**；新设计不得复用它 |
| 真实参数在 `mupc_core_config.yaml` → `CoreConfig`（`core_config.rs`），启动时读取一次 | 需要**新的可写真源 + 生效分发机制** |
| 全仓无 `reload` / `watch` / 热重载机制 | 需要**逐模块接线**，这是主要工作量 |

#### 4.3.2 `ConfigService` 设计

```
真源文件：--config 指定的 yaml（生产 /opt/mupc/config/mupc_core_config.yaml，须可写）
内存副本：Arc<RwLock<CoreConfig>>（进程内唯一权威读源）
字段元数据：ConfigFieldMeta 静态表（key / label / kind / 范围 / 单位 / requires_reconnect /
            editable / **yaml_path**（保留式编辑的定位依据，见 §4.3.2.1）），
            与 CoreConfig 字段一一映射（编译期由单测保证无遗漏、无多余）

写流程（POST /v1/console/config/apply）：
 ① 字段级校验：key 存在 + editable 检查 + 值域（kind 硬约束）+ 语义校验（复用 CoreConfig::validate() 的思路，逐字段化）
 ② 生成新文本：**保留式编辑**（§4.3.2.1，正常路径）→ 写后自检（可解析 + 仅目标键变化）
 ③ 原子落盘：写 mupc_core_config.yaml.tmp → fsync → 备份 mupc_core_config.yaml.bak → rename
 ④ 进程内生效：按 ApplyMode 分发表逐项 dispatch（见下表）
 ⑤ 更新内存副本 + revision++
 ⑥ 审计（before/after 逐字段 + write_mode）+ 回执（新 ConfigView，含 write_mode）
任一步失败 → 回滚内存副本、保留 .bak 不动、返回结构化失败原因
（EDGE-10：装置保持原配置运行，**不得半生效**）
（EDGE-23：若 §4.3.2.1 回退到整体回写，回执与审计均带 write_mode=full_rewrite，UI 须明示）
```

#### 4.3.2.1 yaml 回写语义（保留式编辑）

背景：`CoreConfig` 现**仅** `derive(Deserialize)`。若把整棵结构序列化回写，会**丢注释**并**丢未建模键**（含现场 legacy `web_api:` 段），与 §7.3「现场既有 yaml 仍可正常加载、不强制运维立即改文件」的兼容性主张**直接冲突**（未知键被静默抹掉，与"被忽略"是两回事）。本设计明确选型如下，不留二义。

| 项 | 设计 |
|----|------|
| **回写方式** | **保留式编辑（text-preserving edit）——选定**：以原始 yaml **文本**为基础，仅替换目标键所在的**标量行**（由 `ConfigFieldMeta.yaml_path` 定位，如 `intercore.port` → 缩进两空格 + `port: <旧值>` 一行），其余字节**逐字不变**（注释、空行、键顺序、未建模键、legacy `web_api:` 段全部原样保留） |
| **可行性前提** | 本期 F9 全部可写字段均为**标量叶子**（ipv4 字符串 / `u16` / `u64` / 枚举字面量），**无列表、无嵌套对象、无多行标量** ⇒ 行级替换语义完备、无歧义。此前提是选型的**硬条件**：若未来新增列表 / 嵌套字段，须先扩展编辑算法或退回回退路径（写入 §14 增量项） |
| **写后自检（落盘前）** | 替换后的完整文本必须同时满足：① 能被 `serde_yaml` 解析为 `CoreConfig`；② 解析结果与内存副本逐字段比对，**只有目标键变化**。任一不满足 → 本次编辑失败，`.tmp` 丢弃、**不落盘**，返回 `ApplyFailed` |
| **回退路径（显式接受降级）** | 仅当保留式编辑**无法定位**（键缺行 / 结构异常 / 值非标量）时，回退为**整体序列化回写**（依赖 `CoreConfig: Serialize`）。此时**注释与未建模键会丢失**，且**必须显式**：回执 `ConfigView` 带 `write_mode`、审计记 `write_mode = full_rewrite`、UI 以 Toast 明示「配置文件已整体重写，原有注释不再保留」。**禁止静默回退**（静默回退 = 现场注释在某次保存后无声消失，属不可接受的隐性数据损失） |
| **`Serialize` 的边界** | `CoreConfig` 增加 `#[derive(Serialize)]`，**仅为回退路径与往返单测服务**；**不新增** `deny_unknown_fields`（保持未知字段容忍，否则现场 legacy yaml 将直接启动失败） |
| **与运维说明的接口** | 部署文档 `deploy/deploy.md` 增补一条固定口径：**正常保存采用保留式编辑，文件注释与未识别键不会被改写；仅当出现 `full_rewrite` 提示时才发生整体重写**——把"注释会不会丢"从不确定预期变为明确契约 |

**往返（round-trip）单测（落点 §11.1 `console_host` 层）**：

1. **保留式路径（主用例）**：输入「含注释 + 含 legacy `web_api:` 段 + 含未建模键」的现场样例 yaml，改 1 个键 → 断言除该标量行外**逐行字节级完全一致**；并断言解析后仅目标键变化。
2. **多键批量**：同一次 `changes` 含 N 个键 → 恰有 N 行被替换，其余不变。
3. **回退路径（把降级行为钉死在测试里）**：构造不可定位样例 → 断言回退到整体回写、`write_mode = full_rewrite`、且注释确实丢失（**降级不是"写在文档里的可能"，而是有测试断言的确定行为**）。
4. **序列化值等价往返**：`CoreConfig` → `serde_yaml` → `CoreConfig` 逐字段相等（防 `skip_serializing_if` / `default` 不对称导致字段静默丢失）。
5. **键一致性**：`ConfigFieldMeta.key` ↔ `yaml_path` ↔ `CoreConfig` 字段三者一一对应（并入 §11.3 元数据一致性测试）。

#### 4.3.3 ApplyMode 分发表（决定「自动生效」的达成度）

> ⚠️ **订正（2026-09-19）**：下表原有两行（**遥测上报周期**、**IEC 104 心跳间隔**）写了 `watch` 热生效方式，而**现网 `CoreConfig` 根本没有对应配置键** ⇒ 该两行的"生效方式"是**零实现**（不是"实现了但慢"）。本表按**代码事实**重写为"**无承载**"，与 §4.3.5 的计数口径（`FIELDS` 9 键 / 可写 7 / 热生效 1 / 需重启 6）**对齐**。对应的 PRD 字段级降级已由 PM 于 2026-09-19 裁定（见 PRD §3.2 F9 第二处补注块）。

| F9 配置项（PRD §3.2 表） | 现网真实 key | 生效方式 | 时效 | 副作用 |
|-----------|--------------|----------|------|--------|
| 日志级别 | `system.log_level` | `tracing_subscriber::reload` handle（`tracing_subscriber` 已具备 reload 能力，需在 logging 初始化处保留 handle） | ≤1 s | 无 |
| **遥测上报周期** | ⚠️ **无对应配置项**（上送节拍在 `startup.rs` 是**硬编码常量**，`CoreConfig` 无该项）⇒ **本期不可读写** | — | — | **见 §4.3.5 与 PRD F9 补注** |
| 核间「对端端口」 | `intercore.port` | 落盘 + 内存副本；**需重启 `mupcd` 进程生效**（`hot_apply.rs` 判 `RestartRequired`） | 重启 | 弹层须提示**链路瞬断**（`requires_reconnect=true`） |
| 核间「对端地址」（**PRD 未列**，实现多出；UI §6.2 标签 = 「对端地址」） | `intercore.host` | 同上一行：**需重启进程生效** | 重启 | 同上（`requires_reconnect=true`） |
| 核间「本地端口」 | ⚠️ **无对应配置项**（`InterCoreConfig` 只有 `host` / `port`——均为**对端**——无本地绑定端口）⇒ **本期不可读写** | — | — | **见 §4.3.5 与 PRD F9 补注** |
| 核间心跳/重连间隔（**PRD 未列**，实现多出） | `intercore.heartbeat_interval_sec` / `reconnect_interval_sec` | 落盘 + 内存副本；**需重启进程生效** | 重启 | 无 |
| **IEC 104 心跳间隔** | ⚠️ **无对应配置项**（`GatewayConfig` 只有 `listen_addr` / `listen_port`，无心跳字段；`Iec104Config.heartbeat_interval_secs` 恒取默认 10 s）⇒ **本期不可读写** | — | — | **见 §4.3.5 与 PRD F9 补注** |
| IEC 104 监听地址 | ⚠️ **无对应配置项**（现网是服务端模型，「对端 IP」不存在）⇒ 按 §4.3.4 落为 `gateway.listen_addr`（**本机监听地址**） | 落盘 + 内存副本；**需重启进程生效** | 重启 | 弹层须提示**调度通道瞬断**（`requires_reconnect=true`，高风险须明示） |
| IEC 104 端口 | `gateway.listen_port` | 同上一行 | 重启 | 同上（`requires_reconnect=true`） |

> **本表与实现的对账（2026-09-19，逐字段核 `mupc/crates/mupc-core-bin/src/console_host.rs` 的 `FIELDS`）**：
> 实现侧 **9 键**（7 可写 + 2 只读），与上表的关系是 **−3 / +3**：
> **少 3**（PRD 有、实现无承载）＝ 遥测上报周期、核间本地端口、IEC 104 心跳间隔；
> **多 3**（实现可写、PRD 未列）＝ `intercore.host`、`intercore.heartbeat_interval_sec`、`intercore.reconnect_interval_sec`。
> 另有 2 键只读（`display.bind_addr` / `display.control_bind_addr`，`editable=false`，见 §6.2）。

#### 4.3.4 两个必须让 PM 拍板的口径问题（诚实标注）

1. **PRD F9 的「IEC 104 对端 IP 地址」在现网配置结构中不存在。** `mupc_gateway::Iec104Server` 是**服务端**（`bind` 后监听，接受调度主站连接），配置只有 `listen_addr`；没有"对端 IP"这一概念（对端 IP 由 TCP 连接决定，且可能多个）。该配置项来自原 08 的 Web 表单，与现网结构不符。
   - **处置建议**：将该项替换为 **`gateway.listen_addr`（本机监听地址/端口）**，并在设计中提供映射；**或**由 PM 确认「对端 IP」指白名单（允许连接的调度主站 IP，需在 gateway 侧新增白名单能力——属新功能，不在本期）。
   - **设计默认**：按 `gateway.listen_addr` 落地，PRD 措辞回写事项提交 PM（§14 R-08）。
2. **「配置自动生效，无需重启」的达成度边界**：`log_level` / 各类周期参数可做到即时；**连接类参数（监听地址、核间端点）"生效"必然伴随一次链路重建**（≤5 s 内完成，但期间通道瞬断）。这满足 CF-04「≤5 s 内新参数在运行行为中可见」，但**不是无感的**——须在二次确认弹层明确写出「生效瞬间通信将短暂中断」。**该提示文案是 PRD F14.3「影响范围」的实质内容，不可省略。**

#### 4.3.5 工作量与降级方案（诚实）

- 本项是**独立子系统的净新增**（配置写 + 原子落盘 + 元数据表 + 多模块 `watch` 接线 + 校验 + 审计 + 测试），是全模块**最大的工作量单元**（见 §13.4 工作量表，标记为 **L**）。
- **若工期不足的降级方案（须 PM 裁决，因它偏离 CF-04）**：本期仅支持 **HotApply 子集**（`system.log_level` / 遥测周期 / 心跳类），连接类参数**只落盘 + 提示「需重启 mupcd 生效」**。此方案必须回写 PRD（CF-04 降级）并获 PM 同意，**不得静默实施**。
  - **✅ 已裁定（2026-09-16，PM）：接受本降级**（**不投入**"把 6 个字段做成真热生效"的改造）。**实测计数口径**（G-2 交付；逐字段依据见 `mupc/crates/mupc-core-bin/src/hot_apply.rs` 的结论表）：字段表 **9** 键 ⇒ `editable=true` 可写 **7** ⇒ **真热生效 1**（`system.log_level`）⇒ **需重启 6**（`intercore.host` / `intercore.port` / `intercore.heartbeat_interval_sec` / `intercore.reconnect_interval_sec` / `gateway.listen_addr` / `gateway.listen_port`）。
    > ⚠️ **口径澄清（2026-09-19）**：这里的「字段表 9 键」指**实现侧 `FIELDS`**（`console_host.rs`），它**不等于** PRD §3.2 F9 的 7 个配置项 —— 二者差 **−3 / +2**：**缺** IEC 104 心跳间隔 / 核间本地端口 / 遥测上报周期（**无配置承载**，见 §4.3.3 订正），**多** `intercore.heartbeat_interval_sec` / `reconnect_interval_sec`（PRD 未列）。因此"可写 7"与"PRD 的 7 项"是**两个不同的 7**，不可互相印证；PRD 侧的真实达成度见 PRD §3.2 F9 的第二处补注块（**字段级 4/7**）。
  - **回写落点**：PRD（头部补注 + §3.2 F9 补注块，标 CF-04 降级）与 UI 设计文档（§3.6 P2 行 / §6.2 线框 `Y80` 行 / §6.2 流程 3「影响范围」/ §6.2 流程 5 / §7.3 弹层线框；版本表补注 4）。**屏上口径**：页面说明行与弹层「影响范围」改为分级口径「**日志级别立即生效 · 连接类参数需重启进程生效**」；保存成功 Toast **并入后端回执 `message`**（后端逐字点名需重启的键），不再统一写「已生效」。实现落点 = `local-display/src/ui/pages/p2_config.rs`（其偏差登记 **PD24**）。
  - **⚠️ 残余（如实登记）**：`Toast` 文本区 400 px（≈16 字，`DOTS` 截断）⇒ 长回执的**具体键名可能被截掉**；回执 `message` 的用字（`项` / 全角括号等）**不在字体码表控制面内**（真机豆腐块）。两条同属既有「自由文本不受码表约束」口径，收口批见 PD24。

### 4.4 日志服务（F10）

**现状问题**：`web-api::LogsHandler::get_logs` 每次请求**逐行读取全部日志文件**并做**内存关键字过滤**，无索引、无上限；随日志增长必然劣化，且其「关键字过滤」正是 PRD T-1 明确砍掉的能力。

**设计（`LogService`）**：

| 能力 | 实现 | 约束 |
|------|------|------|
| **实时推送 ≤2 s** | 自定义 `tracing_subscriber` **Layer**：把已格式化条目（`seq` 单调、`ts_ms`、`level`、`target`、`message`）推入**有界 ring**（`VecDeque`，容量 **2000**，`Mutex`） | ring 只能含**当前级别可见**的条目（受 `log_level` 过滤）；内存上界固定（满足 PRD §4.1.4「内存不得单调增长」） |
| 增量拉取 | `GET /v1/console/logs?cursor=<seq>&limit=`：返回 `seq > cursor` 的条目；HMI 每 500 ms 拉一次 → 延迟 ≤1 s（优于 PRD 要求的 2 s） | 无需长轮询/WebSocket（KISS） |
| 等级筛选 | `levels` 多值（选项式） | — |
| 模块筛选 | `targets` 多值；选项列表来自 ring + 最近日志文件采样（去重排序，≤50 项） | LG-03「选项列表，无文本输入框」 |
| 时间范围 | 预设 `1h` / `24h` / `custom(from,to)`；`custom` 的起止由 UI 用**日期+时间选项式步进**给出 | LG-04 |
| 历史分页 | 文件扫描（复用 `LogsHandler` 的文件定位/解析逻辑，去掉 `keyword`） | **限额**：单次请求最多扫描 **5 个日志文件**且总行数上限 **50 000**；超限 → `range_too_large=true`（EDGE-15，UI 提示缩小范围），**不执行全库检索** |
| ~~关键字搜索~~ | **不做**（T-1） | — |
| ~~导出~~ | **不做**（T-2，无任何入口） | — |

> **迁移落点**：`web-api::routes::logs::{LogsHandler, LogQuery, LogEntry}` 的解析逻辑迁入 `mupcd/src/console_host/log_service.rs`；`LogQuery` 去掉 `keyword`，加 `cursor` / 多值筛选；`export_logs()` **删除**。

### 4.5 审计服务（PL-1 / PL-2）

```rust
// crates/display-proto/src/audit.rs
pub struct ConsoleAuditEntry {
    pub id: String,            // uuid
    pub ts_ms: u64,
    pub operator: String,      // 固定 "local-console"（T-3：无登录）
    pub op: ConsoleOp,         // 枚举，选项式筛选的维度
    pub target: String,        // 如 "system.log_level" / "interlock.release"
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub result: AuditResult,   // Ok / Failed
    pub reason: Option<String>,// 失败原因（成功为 None）
    pub request_id: String,    // 与写请求信封对应（现场对拍用）
}

#[serde(rename_all = "snake_case")]
pub enum ConsoleOp { ConfigApply, ConfigResetDefault, InterlockRelease, InterlockAckM1 }
```

**存储与查询**：

| 项 | 设计 |
|----|------|
| 落点 | `{system.log_dir}/audit/console-audit-YYYY-MM-DD.jsonl`（append-only；与既有 security 审计分文件，避免 schema 冲突） |
| **双写** | 同时调用 `mupc_security::audit::AuditLogger::log(GenericOperation, ...)` 写一条摘要进**既有哈希链审计**，保持「统一合规凭据」不被本次改造破坏 |
| 查询 | 按日期定位文件（**不扫全库**）→ 解析 → 按 `from/to/ops` 过滤 → 倒序 → 分页 20 → 返回 `has_more` 与 `newest_ts_ms`（F19.8） |
| 不可用 | 目录不可读/解析失败 → 结构化错误 → UI 显「审计记录不可用」（EDGE-17），**不得**显「无审计记录」 |
| 不可删改 | 无删除/清空接口；文件以 append 打开；PL-02 的「仅追加、不可删改」由接口面 + 文件模式共同保证 |
| 字段口径（PL-1） | 原 `WebAuditEntry` 的 `user / role / ip_address / user_agent` **全部去除**（无登录、无网络面，这些字段失去语义）；新增 `request_id` 作为可追溯标识。**这是 PRD §6.2 要求的「字段口径须在设计阶段定稿」的定稿** |

### 4.6 联锁控制接口（F16–F18）

**迁移**：`InterlockApi` / `InterlockStatus` / `InterlockSourceStatus` 从 `web-api::app_state` 迁至 `display-proto::interlock`。

**改造：错误结构化**（PRD EDGE-12 / IL-02 / IL-03 要求「明示具体拒绝原因」，现状 `Result<(), String>` 只够打日志，不够驱动 UI）：

```rust
#[serde(rename_all = "snake_case")]
pub enum InterlockReject {
    SourcesNotReset { remaining: Vec<String> },        // 触发源未复位（列出具体源）
    HoldNotElapsed { need_secs: u64, remaining_secs: u64 },
    Latched,                                            // 处于 latch 态（ack_m1 前置）
    StopPending,                                        // 停机未确认（stop_failed）
    NotEnabled,                                         // io.enabled=false
    Busy,
    Internal(String),
}
impl InterlockReject { pub fn user_message(&self) -> String { /* 中文用户可读文案，UI 直接展示 */ } }

#[async_trait]
pub trait InterlockApi: Send + Sync {
    async fn status(&self) -> InterlockView;                        // View 含 available/enabled
    async fn request_release(&self) -> Result<(), InterlockReject>;
    async fn ack_m1(&self) -> Result<(), InterlockReject>;
}
```

**改动影响面（诚实标注）**：`crates/mupc-core-bin/src/interlock.rs`（约 1300 行）需改造其 trait 实现与错误构造路径（含既有单测）；这是**回归风险中等**的改动点，建议独立提交 + 单测覆盖全部 reject 分支（§11.3）。

**`DO1/DO2` 命名冲突（须真机核对）**：PRD F16 写「指示灯 DO1 / DO2（故障灯 / 运行灯）」，而现有 `web-api::InterlockStatus` 注释为 `fault_lamp: 故障灯（DO2）` / `run_lamp: 运行灯（DO1）`——**两者对 DO 号的归属相反**。设计以**语义名**（`fault_lamp` / `run_lamp`）为准，DO 号映射由 `io.do` 配置表决定（部署侧配置即真源），UI 只显示语义与灯态。**此不一致提交真机/厂方核对（§14 R-09）。**

### 4.7 告警源（`AlertFeed` 的可选性）

- **本期承诺**：F7 由 `storage.events` 提供（§4.1 #3）。
- **`SsePushService` 的处置**：其唯一现有用途是 `SouthSink` 在写 `SystemEvent` 后推一条 SSE 事件给 Web 客户端。随 `web-api` 删除，将其替换为 mupcd 内 **`AlertFeed`**：
  - 最小形态（本期）：一个 `tokio::sync::broadcast`（容量 64）或 `Mutex<VecDeque<AlarmItem>>`（容量 100），`SouthSink` 写入系统事件时同时投递。
  - 本期**不**作为 F7 真源（避免双源），仅作为「未落库也能上屏」的**可选增强**留给后续。
  - 若评审认为 F7 必须覆盖「未落库的即时告警」，则把 `AlertFeed` 提升为 F7 的一路源并与 `storage.events` 合并（`available = 任一可用`）。**这是待 PM/评审裁决的一个范围点（§14 R-07）。**

### 4.8 编译时间戳（T-5 #5）

`crates/mupc-core-bin/build.rs`（新增）：

```rust
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // SOURCE_DATE_EPOCH 优先 → 交叉编译可复现；否则取当前时间
    let ts = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .map(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .flatten()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    println!("cargo:rustc-env=BUILD_TIMESTAMP={ts}");
}
```

消费：`InfoSection.build_time = option_env!("BUILD_TIMESTAMP").map(str::to_string)`；`firmware_version = env!("CARGO_PKG_VERSION")`。取不到 → `None` → UI「未提供」。

### 4.9 启动装配与 mupcd 配置段

`startup.rs` 步骤 10 整块（原 Web API 装配，约 90 行）**替换**为：

```rust
// ── 10. 本地 HMI 后端（读通道 + 控制通道）──
tracing::info!("[10/14] 初始化本地 HMI 后端...");
if config.display.enabled {
    // 10.1 读通道（既有）
    let latest: SharedLatest = Arc::new(Mutex::new(None));
    tokio::spawn(DisplayDataProvider::new(ai_integrator.clone(), intercore.clone(),
                 &config.display, modbus, latest.clone(),
                 /* 新增：*/ device_sampler, alarm_sampler, interlock_sampler).run());
    let l1 = TcpListener::bind(&config.display.bind_addr).await?;   // 强制回环（validate）
    tokio::spawn(LoopbackHttpPublisher::new(latest).serve(l1));

    // 10.2 控制通道（新）
    let console = ConsoleHost::new(ConsoleDeps {
        config_path: cli.config.clone(),
        core_config: core_config_arc.clone(),
        log_dir: config.system.log_dir.clone(),
        audit: console_audit.clone(),
        interlock: interlock_ctl.clone(),
        apply_registry: apply_registry.clone(),   // watch 发送端集合
    });
    let l2 = TcpListener::bind(&config.display.control_bind_addr).await?;
    tokio::spawn(console.serve(l2));
    coord.register_service("hmi_backend", ServiceStatus::Running);
} else {
    tracing::info!("display.enabled=false：本地 HMI 后端未启动（部署行为不变）");
}
```

`mupc_core_config.yaml` 新增/扩展：

```yaml
display:
  enabled: true                    # 启用本地 HMI 后端（读 + 控制）
  bind_addr: "127.0.0.1:9810"      # 读通道（既有）
  control_bind_addr: "127.0.0.1:9811"   # 【新】控制通道；validate 强制回环
  publish_ms: 1000
  min_publish_interval_ms: 250     # 【新】「变更即组帧」合并窗口（§4.2.1 约束 5）
  # 【新】慢拍节拍（上界为**时延达标红线**，非性能调优项，见 §4.2.1）
  device_poll_ms: 3000             # F6.3 ≤5 s：3.0+0.25+0.5+0.1 = 3.85 s
  alarm_poll_ms: 500               # F7.3 ≤2 s：0.5+0.25+0.5+0.1 = 1.35 s
  interlock_poll_ms: 500           # F16.5 ≤2 s：同上
  alarm_page_size: 10              # F7 最多展示条数
  log:                             # 【新】日志服务限额（EDGE-15）
    max_files: 5
    max_lines: 50000
    live_ring: 2000
  range: { ... }                   # 既有（域值化量程）
```

`CoreConfig::validate()` 新增：
- `display.bind_addr` / `display.control_bind_addr` **必须是回环**（非回环 → `Err`，强制满足 PL-4 与「仅回环 127.0.0.1」）；
- 两地址**不得相同**；
- `publish_ms >= 100`（**上界 4000 见下条「时延红线」**）、`live_ring >= 100`；
- **时延红线（§4.2.1 约束 1/2/5/6，越界即启动报错，不解释为"调优空间"）**：
  - `alarm_poll_ms <= 1000`、`interlock_poll_ms <= 1000`（F7.3 / F16.5 端到端 ≤2 s 的前提）；
  - `device_poll_ms <= 4000`（F6.3 端到端 ≤5 s 的前提）；
  - `publish_ms <= 4000`（契约 `MAX_PUBLISH_MS`；§4.2.1 约束 6 的落地——主拍慢于最慢一段采集即无验收意义，该上界用于把 `min = publish = 60_000` 这类退化组合挡在启动期）；
  - `250 <= min_publish_interval_ms <= publish_ms`（合并窗口下界防抖动打爆通道，上界防退化为纯主拍）。
    > **两条上界的依据**：`min_publish_interval_ms` 下界 **250** 与 §4.2.1 约束 2 一致（低于此值合并窗口过小，抖动会打爆通道）；`publish_ms` 上界 **4000** 与约束 6 一致——主拍慢于**最慢的一段采集**（`device_poll_ms` ≤4000）时 §4.2.1 的拆解表失效，该上界的作用是把 `min = publish = 60_000` 这类**退化组合**（端到端 ≈61 s，验收 ≤2 s）**挡在启动期**。两条上界**各自独立、服务不同指标（≤2 s / ≤5 s），不得互相绑定**。契约对应 `display-proto/src/config.rs::MIN_MERGE_WINDOW_MS` 与 `::MAX_PUBLISH_MS`。

> **校验的唯一真源 = 契约** `DisplayConfig::validate()`（`display-proto/src/config.rs`）；`CoreConfig::validate_display()`（`mupc-core-bin/src/core_config.rs`）只做**转发**，不再手写校验。含义有二：
> ① 回环口径**只认字面量** `127.0.0.1` / `::1`（`localhost` 等**名字**一律拒——名字可经 hosts 重映射，安全红线不接受名字；渲染端 `console.rs` 同口径），且端口 ∈ [1,65535]、两址不得相同；
> ② 转发的是**全集**校验 ⇒ 上列**非地址**不变量（时延 / 环容量 / 量程 / 告警页条数）**同样在启动期 fail-fast**，现场 yaml 不合规者升级后 `mupcd` **启动失败**（迁移核对清单见 `mupc/deploy/deploy.md` §9.4）。

**删除**：`CoreConfig.web_api` 字段 + `WebApiConfig` + 其 `validate()` 校验 + 默认值函数 + 单测样例中的 `web_api:` 段（约 20 处）。

---

## 5. HMI 进程设计

### 5.1 crate 与模块划分

```
crates/local-display/
├── lvgl-sys/                # 【新】LVGL v9 C 源码的原始绑定（§1.1.1.2）
│   ├── build.rs             #   cc 编译 vendor/lvgl 源码 + bindgen(精确 allowlist) 生成绑定
│   ├── allowlist.txt        #   【新】bindgen 精确 allowlist（逐符号枚举，禁 lv_* 通配，§1.1.1.2 / §12.1）
│   ├── lv_conf.h            #   LVGL 配置（色深/fbdev-off/evdev-off/内存/日志转发，见 §1.1.1.2）
│   └── src/lib.rs           #   include!(OUT_DIR/bindings.rs)（852 行；预生成入库 feature prebuilt-bindings 本轮未做，§12.1）
├── fonts/                   # lv_font_conv 资产（**不在 src/ 下**，与 §1.1.2 / §12.3 一致）
│                            #   入库仅 gen_fonts.sh / extract_charset.py / font_subset_charset.txt /
│                            #   NotoSansSC-LICENSE.txt；字库源 *.otf 与生成物 *.c 不入库（.gitignore）；
│                            #   构建前先跑一次 gen_fonts.sh
├── src/
│   ├── lib.rs               # pub mod 汇总（保留可测模块出口）
│   ├── main.rs              # 【重写】bin 入口：CLI → LVGL init → display/indev 注册 → 事件循环 → 优雅退出
│   ├── lvgl/                # 【新】薄安全层（唯一允许 unsafe 的 Rust 侧除 lvgl-sys 外之处）
│   │   ├── mod.rs           #   lv_init / lv_deinit / lv_tick_set_cb 封装
│   │   ├── obj.rs           #   Obj 包装（创建/父子/坐标/可见/样式）
│   │   ├── widgets.rs       #   控件构造与 setter（§5.6 控件映射表）
│   │   ├── style.rs         #   样式的机制层：类型化 lv_style_* setter / 选择器 / 部件枚举
│   │   │                    #   （外观数值**不在此处**——单一真源见 ui/theme.rs 与 §5.6）
│   │   ├── font.rs          #   lv_font_conv 产物注册与文本设置
│   │   ├── display.rs       #   lv_display 注册 + PARTIAL 双缓冲 + flush 桥（§1.1.1.1 P-1）
│   │   ├── indev.rs         #   lv_indev 注册 + read_cb 桥（接 Rust evdev）
│   │   └── event.rs         #   C 回调 → Rust closure 的桥（user_data 生命周期 + 不 panic 纪律）
│   ├── timing.rs            # 【新】事件循环骨架：poll 多路复用 + lv_timer_handler 驱动（§5.2）
│   ├── touch.rs             # 【新】evdev 设备发现 / 绝对坐标读取 / 校准 / 事件翻译（§1.2，纯 Rust）
│   ├── screen.rs            # 【新】flush_cb 的像素 sink：/dev/fb0（复用 canvas.rs 的 FbCanvas）
│   │                        #   + 内存 sink（--backend offscreen，导出 PNG）
│   ├── ui/                  # 【新】页面与控件装配（Rust 代码，调用 src/lvgl 薄层）
│   │   ├── mod.rs           #   页面路由（lv_tabview 或自建 6 页容器 + 导航栏）
│   │   ├── theme.rs         #   外观数值的**唯一真源**：色板/尺寸/字号常量 → 经 style.rs 机制施加 lv_style
│   │   │                    #   （页面内不得硬编码裸色值/裸尺寸，CI 静态断言见 §11.1 约束 ④′）
│   │   ├── components.rs    #   StatusChip / LedIndicator / Stepper / MultiSelectChips /
│   │   │                    #   ConfirmDialog / Toast / EmptyState / UnavailableState（薄层之上的组合）
│   │   └── pages/           #   p1_status.rs / p2_config.rs / p3_logs.rs /
│   │                        #   p4_interlock.rs / p5_audit.rs / p6_system.rs
│   ├── channel.rs           # 【保留+扩展】回环 HTTP 客户端（GET 帧，500 ms）
│   ├── console.rs           # 【新】控制通道客户端（POST/GET /v1/console/*，含信封构造与幂等重试）
│   ├── state.rs             # 【保留+扩展】三态归一/新鲜度/通道态 + 新段视图 + 页面状态机
│   ├── config.rs            # 【保留+扩展】CLI 参数（+ --control-channel / --touch-* / --rotate）
│   ├── canvas.rs            # 【保留】FbCanvas（fb0 打开/格式探测/区域写）→ 升为 flush_cb 的 sink（§8.3）
│   │                        #   绘制原语契约与 OffscreenCanvas 布局用途废弃
│   ├── font.rs              # 【废弃】ab_glyph 光栅化（码表资产迁至 fonts/，渲染交给 LVGL）
│   ├── layout.rs            # 【废弃】固定网格自绘
│   └── run.rs               # 【废弃】旧 500 ms 定拍自绘循环 → 新事件循环（见 timing.rs）
└── tests/
    ├── ui_offscreen.rs      # 【重写】LVGL 内存 display 离屏渲染：页面非空 + 中文文案区域 + 降级态断言
    ├── interaction.rs       # 【新】触摸事件注入（直投 indev）→ 命中/防抖/超时回归/草稿保留/长按
    └── channel_chain.rs     # 【新】stub 服务端（读帧 + 控制回执）→ 全链路
```

> **UI 组件策略**：**采用 LVGL 内置控件 + 自定义主题**（**不**全自绘、**不**禁用内置控件）。理由：① LVGL 内置控件**本就为嵌入式小屏设计**，`lv_style`/`lv_theme` 的样式覆盖能力覆盖 UI 需要的全部维度（色/字号/圆角/内边距/描边/状态色），逐项可控；② 全部自绘等于**放弃 LVGL 最核心的价值**（控件/滚动/焦点/弹层/命中测试），把工作量推回 L-5 自研路线，**违背 KISS 且与切换动因不符**；③ 与 r1 的"Slint 内置 `ComboBox` 弹出层不可控"不同，LVGL 的下拉/弹层/列表样式**均可通过 `lv_style` 与自定义主题覆盖**。
> **保留的约束**：**控件外观必须来自 `ui/theme.rs` 的单一真源**（页面内**不得硬编码裸色值/裸尺寸**），且**不得使用 LVGL 默认主题（`LV_USE_THEME_DEFAULT`）的配色作为最终外观**——默认主题仅作控件行为基线，外观一律由我们的 `lv_style` 覆盖（CI 静态断言，§11.1 约束 ④′）。
>
> `display-proto` 保持**唯一跨进程契约 crate**，不引入 LVGL/FFI 依赖（契约层零 UI 依赖，测试桩可在任意环境编译）。crate 名称保留 `display-proto` 以避免跨 3 个 crate 的改名 churn；**可选**（评审可议）改名为 `hmi-proto`。

### 5.2 LVGL 集成与事件循环

```rust
// src/main.rs + src/timing.rs（骨架，标注关键 API 与不变量）
fn main() -> Result<()> {
    let cli  = Config::parse(std::env::args())?;         // 保留既有 config.rs（扩展）
    let fb   = FbCanvas::open(&cli.fbdev_path, cli.width, cli.height)?;  // 复用 canvas.rs（保留）
    let touch = TouchDevice::open(&cli.touch)?;          // 新：Rust evdev 发现/校验/校准

    lvgl::init();                                        // lv_init()
    lvgl::set_tick_cb(Instant::now());                   // lv_tick_set_cb(→Instant 单调 ms)
    let disp = lvgl::display::create(cli.width, cli.height);   // lv_display_create
    lvgl::display::set_buffers_partial(disp, /* buf1/buf2 尺寸按 §10 预算 */);
    lvgl::display::set_flush_cb(disp, |area, px_map| screen::blit(&fb, area, px_map)); // P-1：fbsink
    let indev = lvgl::indev::create_event_driven(&touch);      // lv_indev_create + read_cb + MODE_EVENT

    let mut app = ui::App::new(disp, indev, channel, console, cli)?;  // 建 6 页控件树 + 主题

    // ── 事件循环：poll 多路复用（evdev fd）；超时取 LVGL 的下次处理时刻；无忙等（PRD §4.1.1）──
    loop {
        let lv_next = lvgl::timer_handler();             // lv_timer_handler() → 距下次处理 ms（重绘/动画/定时器）
        let timeout = min(lv_next, app.next_deadline()); // ∪ 通道 500ms 轮询截止
        poll(&touch.fd, timeout)?;                       // 唯一阻塞点；空闲即让出 CPU
        touch.pump()?;                                   // 读 evdev 事件 → 更新 (pressed,x,y) 快照
        lvgl::indev::read(indev);                        // lv_indev_read() → LVGL 命中/派发（LV_EVENT_*）
        app.on_lv_events();                              // 从事件队列取 UI 动作 → 业务（切页/提交/防抖）
        app.tick(Instant::now())?;                       // 拉帧 → 更新 state → 刷新受影响的 lv_obj
        lvgl::timer_handler();                           // 再次驱动：把本拍的 lv_obj 变更立刻落到脏区重绘
    }
}
```

**架构说明**：
- LVGL **不是**应用框架——它只提供**渲染与控件**，**没有**自己的事件循环/平台抽象。因此本设计的事件循环是「驱动 LVGL 的 `lv_timer_handler`」，**无需实现任何框架契约**。
- `flush_cb` 按**脏区矩形**写 fb：LVGL 以 `lv_area_t` 为单位失效/重绘，**不需要整行扫描**，且内部对脏区做并集合并 ⇒ **粒度更粗更省**。
- `lv_timer_handler()`（LVGL 自有定时器/动画/重绘调度）的**返回值即下次需要处理的时间**，直接作为 `poll` 超时。

**不变量（编码约束，须在 review 中检查）**：
1. 事件循环**唯一**阻塞点是 `poll`（超时 = `min(lv_timer_handler 返回值, 通道截止, ≤500 ms)`），**不得**出现 `loop {}` 自旋或 `sleep` 与 `poll` 混用的忙等（PRD §4.1.1 / NF-02）。**特别注意**：LVGL 的 `lv_timer_handler()` **必须被周期性调用**（否则动画/滚动/超时回归停摆）——这与"不许忙等"不冲突，因为其返回值为毫秒级间隔，`poll` 会**阻塞那么久**；**禁止**写成无超时的紧循环。
2. **LVGL 渲染只在 `lv_timer_handler()` 内发生**（LVGL 自己决定何时重绘脏区）；**不得**在循环里手动调用 `lv_refr_now()`（那是测试专用强制渲染，生产用会退化为全帧重绘，CPU 预算失控）。
3. 任何阻塞 I/O（通道 GET/POST）**不得**在 `flush_cb` / `read_cb` / LVGL 事件回调内执行（回调内只做像素搬运或状态快照）。通道 I/O 在 `app.tick()` 内以**非阻塞 + 超时**方式完成（`O_NONBLOCK` 状态机，见 §5.5）。
4. **所有 LVGL 调用必须在事件循环线程内**（LVGL 非线程安全，`lv_conf.h` 不开多线程）；**禁止**跨线程触碰 `lv_obj`。
5. 回调内**不得** `panic`（跨 FFI 展开为 UB）；Rust 侧回调统一以错误码/日志返回，异常状态经 `app` 状态机在循环内处理。

### 5.3 触摸输入（`touch.rs`）

| 环节 | 设计 |
|------|------|
| 发现 | 扫描 `/dev/input/event*` → `evdev::Device::open` → 检查 `supported_events()` 含 `EV_ABS`，且 `absolute_axes()` 含 `ABS_X/Y` 或 `ABS_MT_POSITION_X/Y` |
| 多候选 | **报错退出并列出全部候选**（附设备名），不猜 |
| 显式指定 | `--touch-device /dev/mupc-touch`（生产固定；udev 规则见 §12.2） |
| 校准 | `EVIOCGABS(ABS_X/ABS_Y)` → `(raw − min) / (max − min) × screen`；`max ≤ min` 或 ioctl 失败 → **启动报错** |
| 覆盖 | `--touch-calib xmin,xmax,ymin,ymax` / `--touch-swap-xy` / `--touch-invert-x` / `--touch-invert-y` |
| 单点 | 取首个活动 slot（`ABS_MT_TRACKING_ID ≥ 0`）；若为单点设备则用 `ABS_X/Y` + `BTN_TOUCH` |
| 事件翻译 → LVGL | 读到 `EV_KEY/BTN_TOUCH`（或 MT slot 的 `ABS_MT_TRACKING_ID`）与 `ABS_*` 坐标后，写入 Rust 侧 `(pressed, x, y)` 快照；`src/lvgl/indev.rs` 的 `read_cb` 把它填进 `lv_indev_data_t`（`point`/`state`）→ **LVGL 负责命中/z-order/弹层拦截/滚动判定/`LV_EVENT_CLICKED` 派发** |
| 读取时机 | `lv_indev_set_mode(indev, LV_INDEV_MODE_EVENT)` + 事件到达后 `lv_indev_read(indev)` **主动投递**（避免 LVGL 默认 30 ms 轮询带来的固定延迟）；`lv_conf.h` 的 `LV_USE_EVDEV` **置 0**（不引入 libevdev，见 §1.2） |
| 缺失/异常 | 触摸设备打开失败 → **进程仍运行**（只读展示照常），启动日志 `warn` + 屏幕上角标「触摸不可用」（EDGE-13；PRD 明确「触摸失效不得影响数据刷新」） |

### 5.4 页面与状态模型

- **页面路由**：LVGL 侧用**6 个页面容器（`lv_obj`）+ `lv_tabview`（隐藏内置标签栏）**或自建 `page_index` + `lv_obj_add_flag(..., LV_OBJ_FLAG_HIDDEN)` 切换。顶部/底部导航栏常驻（6 个 `lv_btn`），任意页到任意页 **1 次触摸**（满足 F11.2 的 ≤2 次，且**没有 AI 类入口**、**无置灰占位**）。
  - **取舍**：`lv_tabview` 自带页面生命周期与切换管理（**更 KISS，默认选它**）；自建 `page_index` 则在"切页时不想重建控件树"的场合更可控。**编码期二选一，但必须在 §11.1 的离屏测试中固化"6 页均可直达且可回"的行为**。
  - **隐藏标签栏的写法**：v9.5.0 **不存在** `LV_TAB_POS_NONE`（`LV_TAB_POS` 在 `mupc/vendor/lvgl/` 全树 **0 处匹配**，逐文件核 `src/widgets/tabview/lv_tabview.h` 亦无）。**正确写法**：`lv_tabview_get_tab_bar(tv)`（`src/widgets/tabview/lv_tabview.h:133`）取到标签栏对象后 `lv_obj_add_flag(tab_bar, LV_OBJ_FLAG_HIDDEN)`（`LV_OBJ_FLAG_HIDDEN = (1u<<0)`，`src/core/lv_obj.h:49`）；薄层封装为 **`tab_bar()?.set_hidden(true)`**（实现见 `mupc/crates/local-display/src/lvgl/widgets.rs`）。**视觉/功能等价**——唯一差别是标签栏仍占一个子槽（不参与布局，可忽略）。
- **子页返回**：固定位置返回控件（同一应用内位置一致，F11.3）。
- **当前页选中态**：导航项 `selected` 视觉 + 文字（F11.4）。
- **状态模型**（`state.rs`，保留既有纯逻辑并扩展）：

```rust
pub struct UiState {
    pub frame: Option<DisplayFrame>,   pub last_ok_at: Option<Instant>,
    pub channel: ChannelStatus,        // Init / Live / Stale / Down（语义不变）
    // 既有派生视图（保留）：soc / run_state / 三相 NumView（Valid | Missing(reason)）
    // v2 新增派生：
    pub device: DeviceView, pub alarms: AlarmView, pub info: InfoView, pub interlock: InterlockView,
    pub logs: LogPageState, pub audit: AuditPageState, pub config: ConfigPageState,
    pub toast: Option<Toast>,          // 3 s 自动消失（F9.5）
    pub confirm: Option<ConfirmDialog>,// 模态；打开期间暂停超时回归（F15.4）
    pub dirty: bool,                   // 配置页有未保存修改（F15.2 / EDGE-11）
}
```

- **三态归一的复用**：`state.rs` 中「`FieldFlag` → `NumView{Valid(v) | Missing(reason)}`」的映射、`Freshness`、`ChannelStatus`、`ScreenMode` 逻辑**逐条保留**，扩展用于新段（`LinkState` → `LedView{state, text}`；`LinkState::Unknown`/`NotConfigured` → 「未知」/「未配置」，**绝不映射为「正常」**，F6.5）。

### 5.5 通道客户端（读 + 控制）

| 组件 | 设计 |
|------|------|
| `channel.rs`（保留+扩展） | 回环 HTTP 客户端：GET `/v1/display/latest`，单次请求超时 2 s，节拍 `--poll-ms`（默认 **500 ms**）。**非阻塞状态机**：`TcpStream::set_nonblocking(true)` + `connect → write → read` 三阶段状态机，由 `app.tick()` **每拍推进一次**，超时以 `Instant` 截止时刻强制（到期即 abort、计一次通道失败）。**禁止 `block_on`、禁止在 `tick` 内同步等待**——这是 §5.2 不变量 3（非阻塞）的唯一实现形态，**不留"编码时择一"的余地**。约束：单次轮询耗时上界 2 s 且不影响触摸响应（事件循环 `poll` 超时仍 ≤ 500 ms，见 §5.2 不变量 1）。 |
| `console.rs`（新） | 控制通道客户端：`request_id`（uuid）生成、`issued_at_ms`、`op` 校验；GET 查询 + POST 写；**与 `channel.rs` 同构的非阻塞状态机**（同样禁止 `block_on`，由 `tick` 推进），单次超时 5 s；**幂等重试**：同 `request_id` 重试安全（服务端返回首次结果，`duplicate=true`）。结果填入 `UiState` + `Toast`。 |
| `hmi_channel` 段 | `DeviceSection.hmi_channel` 由 HMI **本地覆盖**为自身 `ChannelStatus`（服务端给 `Unknown`）——避免"由服务端报告客户端自己的连接状态"这一语义倒置。 |

> **`--poll-ms` 的硬上界（F7.3 / F16.5 红线）**：HMI 侧 CLI 校验 `--poll-ms ∈ [100, 500]`，`> 500` **直接报错退出**（错误信息指向 §4.2.1 时延拆解）。理由：该值直接进入 F7.3 / F16.5 的端到端算式，放开即可能**静默破坏 ≤2 s 验收**。

### 5.6 交互规范落实（PRD §3.6 / §4.2 / UI 设计 §2.5 / §7.3 / §10）

| PRD 条款 | 落实 |
|----------|------|
| TT-08 控件 ≥48×48 px；关键操作 ≥64×64 px；间距 ≥16 px；危险控件间距 ≥48 px | `ui/theme.rs` 定义 `Dimens::TOUCH_MIN = 48` / `TOUCH_CRITICAL = 64` / `GAP_MIN = 16` / `GAP_DANGER = 48`，经 `lv_obj_set_size()` / `lv_obj_set_style_pad_*()` 施加；**所有**可点控件显式引用这些常量（review 检查项：`ui/**` 内不得出现裸数值尺寸） |
| NF-04 字号 ≥64 / ≥32 / ≥24 px | **控件字号一律取自 `ui/theme.rs` 单一真源**（页面不硬编码字体引用，与"页面不硬编码色值"同口径）；`lv_style_set_text_font(style, &lv_font_noto_sc_NN)` 的 **NN 取自 §1.1.2 的 10 档**（148/112/96/64/56/48/32/28/26/24，`lv_font_conv` 产物，与 UI §3.3 阶梯逐档对应）；NF-04 的下限 ≥64 / ≥32 / ≥24 px 分别落在 L1-特大与 L2-主值、L2-页标题与 L2-数值、正文与弱注；0.5–1.5 m 读距 |
| TT-09 二次确认，默认焦点「取消」 | `ConfirmDialog` 组件（`lv_msgbox` 或 `lv_layer_top()` 上的自定义模态容器）：标题=操作，正文=影响范围（**含 `requires_reconnect` 提示**），明细=前后值列表；**LVGL 输入组** `lv_group` 把「取消」设为默认聚焦对象（`lv_group_focus_obj`） |
| TT-10 500 ms 防重 | 按钮回调内 `last_fire_at` 判定（Rust 侧）；同时以 `lv_obj_add_state(btn, LV_STATE_DISABLED)` 给出禁用视觉反馈 |
| TT-11 滑动不误触发点击 | 依赖 **LVGL 滚动容器语义**：子对象的 `LV_EVENT_CLICKED` 仅在按下-抬起落在同一对象且**未转化为滚动**时派发；阈值由 `lv_conf.h` 的 `LV_INDEV_DEF_SCROLL_LIMIT`（像素）控制。**列为真机/离屏验证项（§14 R-10）**，标定值须固化进 `lv_conf.h` |
| TT-12 60 s 超时回归主状态页 | Rust 侧在事件循环维护 `idle_deadline`（`--idle-timeout-secs`，默认 60）；触摸事件重置；**若 `dirty=true` 则不强制切页**，改为显示顶部提示条「有未保存修改」（不得静默丢弃）；确认弹层打开时不计时（TT-13） |
| F12 零键盘 | **编译期结构性保证（比静态扫描更强）**：`lv_conf.h` 置 `LV_USE_TEXTAREA = 0`、`LV_USE_KEYBOARD = 0`、**`LV_USE_SPINBOX = 0`**。**三项必须同时为 0，其中"弃 `lv_spinbox`"是另两项能成立的前提**——`lv_spinbox` 以 `lv_textarea` 为基类，其头文件带编译期守卫（v9.5.0 `src/widgets/spinbox/lv_spinbox.h`：`#if LV_USE_TEXTAREA == 0` → `#error "lv_spinbox: lv_ta is required. Enable it in lv_conf.h (LV_USE_TEXTAREA  1) "`），故 **`LV_USE_SPINBOX=1` + `LV_USE_TEXTAREA=0` 是 C 编译期直接报错**。**步进器 / IPv4 / 日期时间因此改用 `lv_btn` + `lv_label` 组合**（§5.7 映射表；UI 文档 §5.3 #6/#8 同口径）——**弃用后，本项达成的结论"构建产物中根本不存在文本输入控件"成立**。辅以 §11.4 静态约束 ①（`ui/**` 不得引用 `lv_textarea` / `lv_keyboard` / `lv_spinbox` 符号）防回退 |
| F14 语义三重冗余 | `StatusChip` / `LedIndicator` 组件强制 `text + color + icon` 三通道（组件构造函数签名层面强制，不做纯色块）；`lv_led` 只用其"灯"语义，`.text` 必须并列设置 |
| F5.3 / 防烧屏（NF-05） | 静态装饰周期性微移仅作用于**非数值区**；核心数值区不动（沿用既有原则）；实现为 `lv_timer` 驱动的 `lv_obj_set_pos` 微移，**不得**用 `lv_anim` 循环动画（§5.6 动效纪律） |
| **控件策略** | **采用 LVGL 内置控件 + 自定义主题**（**不作**"禁内置控件皮肤、全自绘"）：`lv_btn`/`lv_label`/`lv_list`/`lv_table`/`lv_bar`/`lv_led`/`lv_dropdown`/`lv_switch`/`lv_msgbox`/`lv_buttonmatrix`/`lv_checkbox`/`lv_tabview` 直接使用；外观一律由 `ui/theme.rs` 的 `lv_style_t` 覆盖。**`lv_spinbox` 不在列**：步进器 / IPv4 / 日期时间一律 **`lv_btn` + `lv_label` 组合**（F12 行）。**保留的硬约束**：① 页面内**不得硬编码裸色值/裸尺寸**（必须经 `theme.rs`）；② **不得把 LVGL 默认主题配色当作最终外观**（`LV_USE_THEME_DEFAULT` 仅提供控件行为基线，色彩/字号/圆角/描边全量覆盖）。**静态断言**：CI 扫描 `ui/**` 不得出现 `lv_color_hex` / `lv_color_make` 之类的裸色值调用（§11.4 ④′） |
| **滚动与滚动指示** | **内容滚动保留交互**：滚动容器（`lv_obj` + `LV_OBJ_FLAG_SCROLLABLE`）支持**内容拖拽 + 惯性（`LV_INDEV_DEF_SCROLL_THROW`）**，`lv_obj_set_scroll_dir(obj, LV_DIR_VER)` 结构性禁止横滚。**滚动条严格为"纯指示、不可拖"**。理由：8 px 触区若可点即违反 UI §2.1「≥48 px 触摸目标」硬规则；UI 文档 §5.3 #16 / §6.1 / §6.4 / §7.4 / §10-2 已同口径。**v9.5.0 源码核实与两手显式实现见下方「5.6.1 滚动条」** |
| **长按保持 1.0 s**（§7.3 L2） | **用 LVGL 内建长按事件**。**API 事实（v9.5.0 源码核实）**：v9.5.0 **不存在**逐对象的 `lv_obj_set_long_press_time`（**v8 遗留 API**；`mupc/vendor/lvgl/src/**` 全文搜该符号 **0 处匹配**）。v9 的长按阈值**收敛于 indev（输入设备）**——须在注册触摸 indev 时设 **`lv_indev_set_long_press_time(indev, 1000)`**（`src/indev/lv_indev.h:173`、`src/indev/lv_indev.c:389`；形参 `uint16_t`，单位 ms）。确认按钮回调监听 `LV_EVENT_LONG_PRESSED`（提交）与 `LV_EVENT_PRESSED` / `LV_EVENT_RELEASED`（进度条 `lv_bar` 开始/复位；`LV_EVENT_PRESS_LOST` 亦须复位）。**中途松手即取消并复位**（LVGL 不派发 `LONG_PRESSED` 即为自然取消）。**由框架保证边界语义**。**⚠️ 三处语义差异（B 阶段须照做）**：① v9 的长按阈值**逐 indev**，**不是逐控件**；② **本机单屏单输入设备 ⇒ 与"逐对象设 1000 ms"语义等价**（实现已按 `lv_indev_set_long_press_time(indev, 1000)` 落地，`mupc/crates/local-display/src/lvgl/indev.rs`）；③ **若将来需要"不同控件不同长按阈值"，当前框架下不可达**，须另立方案（例如在回调内自行计时，但**必须遵守本表「动效纪律」：禁循环/装饰性动画**）。**须先离屏验证**（§11.1 交互层，虚拟时钟推进，不真等 1 s）再上真机（§14 R-10） |
| **确认强度分级**（UI §2.5 / §7.3） | L0 无确认（浏览/筛选/切页）/ **L1 双步确认**（默认焦点「取消」）/ **L2 双步确认 + 长按 1.0 s**（危险色 `#FF6B6B`）/ **L2+ 追加 `WarnBanner`**（本次改动涉及的字段中任一 `requires_reconnect == true`）。落点为 `ConfirmDialog` 的 `level` 参数，**调用方必须显式传入（无默认值，防漏配）**；各页映射见 §6.2（保存 = L1 或 L2+、恢复默认值 = L2）与 §6.4（释放 / M1 授权 = L2） |
| **动效纪律**（新增，LVGL 特有） | 仅允许 `lv_anim` 用于**状态切换**（如弹层淡入，≤200 ms）；**禁止**循环/装饰性动画（CPU 与"空闲让出 CPU"约束，PRD §4.1.1）；进度条类动效由 `lv_bar` 值驱动而非 `lv_anim` |
| **服务地址展示口径** | 「本机服务地址（仅回环）」与「设备管理 IP」**必须分列展示、不得混为一谈**；前者只读（`ConfigField.editable=false`，回环是安全红线，见 §3.4 / §6.2），后者缺失显「未提供」（§6.6）。UI 不得呈现任何"可从远端访问本机接口"的暗示 |

#### 5.6.1 滚动条：LVGL v9.5 行为核实 + 显式实现为"纯指示、不可拖"

> **核实方法**：直接读 **LVGL `v9.5.0`** 源码（非文档推断、不假设框架默认），文件与行号为该 tag 的实际内容。

| 核实点 | 源码事实 | 推论 |
|--------|----------|------|
| 滚动条是否绘制 | `src/core/lv_obj.c:781` `static void draw_scrollbar(lv_obj_t * obj, lv_layer_t * layer);`，唯一调用点在该文件 `LV_EVENT_DRAW_POST` 分支（`:756`）；`src/core/lv_obj_draw.c` 全文无滚动条逻辑 | 滚动条是**纯绘制部件** |
| 按下滚动条会发生什么 | `src/indev/lv_indev_scroll.c:267` `lv_indev_find_scroll_obj()`：滚动目标**只来自 `indev->pointer.act_obj`**（`:280`，即被按下的 `lv_obj`）及其祖先链；**该文件全文 0 处 `scrollbar` 引用**（同法核对 v8 亦为 0） | **不存在**"滚动条命中测试 / 抓 thumb / 点轨道跳转"；按在滚动条区域 = 按在容器上 = **普通内容拖拽**（thumb 随内容比例移动，非抓取） |
| 能否"移除滚动条的 clickable 标志" | `LV_PART_SCROLLBAR` 是**样式部件枚举**（`src/core/lv_obj_style.h:62`，`lv_part_t = 0x010000`），**不是 `lv_obj`**；`LV_OBJ_FLAG_CLICKABLE` 定义在 `src/core/lv_obj.h:50` 的**对象**标志位上 | 滚动条**没有**可加/可去的 `LV_OBJ_FLAG_CLICKABLE`——**"移除滚动条 clickable 标志"在本框架下无对应 API**（该说法仅适用于自建对象，见方案 B） |
| 相关 API | `lv_obj_set_scrollbar_mode()`（`src/core/lv_obj_scroll.c:62`）、`lv_obj_get_scrollbar_mode()`（`:98`）、绘制区 `lv_obj_get_scrollbar_area()`（`:465`）、滚动位置 `lv_obj_get_scroll_y/top/bottom()`（`:128/134/140`） | 官方 API 只提供**模式与样式**，不提供交互开关 |

**结论（钉死）**：**LVGL v9.5 的滚动条不可点击、不可拖动，是既成行为**（"纯指示、不可拖"与框架行为**一致**，无需对抗框架）。

**显式实现（两手，取其一；默认 A）**：

- **(A) 原生 `LV_PART_SCROLLBAR` + 不引入交互 + 测试钉死（默认）**：`lv_obj_set_scrollbar_mode(obj, LV_SCROLLBAR_MODE_AUTO)`（内容高于视口才出现）、`lv_obj_set_style_width(obj, 8, LV_PART_SCROLLBAR)`（轨道 `#141F33`、thumb `#3B4A6B`、`LV_STATE_SCROLLED` → `#4EA6FF`，色值取自 `theme.rs`）；**不注册任何滚动条相关事件回调、不实现任何"拖 thumb / 点轨道"逻辑**（框架亦无处可挂）；并以 §11.1 交互用例**断言**：在滚动条区域（x ∈ [右缘−8, 右缘]）注入"按下→移动→抬起"→ `lv_obj_get_scroll_y()` 的变化量与**同等手势落在内容区**完全一致（语义 = 内容拖拽）且**无跳变**（证明不存在 thumb 抓取）。**可靠性来源**：源码事实 + 回归用例双锁；若上游换 tag 改变了行为，用例会失败（不会静默退化）。
- **(B) 自绘指示（结构性保险，不依赖框架内部实现；若评审认为 A "依赖默认行为"则改此路）**：`lv_obj_set_scrollbar_mode(obj, LV_SCROLLBAR_MODE_OFF)` 关闭原生条；另建 **8 px 宽的子 `lv_obj`** 作指示，`lv_obj_remove_flag(bar, LV_OBJ_FLAG_CLICKABLE)` + `lv_obj_remove_flag(bar, LV_OBJ_FLAG_SCROLLABLE)` 使其 **hit-transparent**（此处的 `LV_OBJ_FLAG_CLICKABLE` 确实存在——因为它**是我们的 `lv_obj`**，见上表第 3 行；`lv_obj_remove_flag` 见 `lv_obj.h:209`）；位置与长度在 `LV_EVENT_SCROLL` 回调内由 `lv_obj_get_scroll_y()` / `lv_obj_get_scroll_top()` / `lv_obj_get_scroll_bottom()` 计算。此分支下"不可交互"由**我们自己的对象标志**保证，与框架内部实现解耦。

> **补记（供工作单元 B 直接使用，⚠️ 不改本节的滚动条结论）**：本节（方案 A）与 §5.6「控件策略」行所依赖的两个样式通道**已在薄安全层就位**（`mupc/crates/local-display/src/lvgl/style.rs`）：**`Style::set_width`**（即方案 A 的 `lv_obj_set_style_width(obj, 8, LV_PART_SCROLLBAR)` 所需的**滚动条 8 px 宽度**通道）与部件枚举 **`Part::{KNOB, SELECTED}`**（取值 `0x030000` / `0x040000`，对齐 `vendor/lvgl/src/core/lv_obj_style.h:64/65` 的 `lv_part_t`——供 `lv_switch` 旋钮与 `lv_dropdown` 选中项的**主题色**施加）。**B 可直接调用，无需再补薄层通道**。**本节结论「滚动条纯指示、不可拖」基于 v9.5.0 源码核实，未变**。

**触摸目标口径（与 UI §2.1 的一并对齐）**：滚动条**自始不是触摸目标**（8 px 仅为其视觉宽度），触摸目标是整个滚动容器（≥ 视口尺寸）——UI §2.1「≥48 px」硬规则的适用对象是"可点控件"，故 **U-3 的冲突结构性消解**；现场 V-7 仅复核**指示清晰度**，不含"是否可拖"。

**须 pin 复核（R-21）**：以上结论基于 **`v9.5.0`**（pin tag 见 §12.1）；**更换 tag 须重核 `src/core/lv_obj.c` 与 `src/indev/lv_indev_scroll.c` 两文件**——若上游新增滚动条拖拽，则改走方案 B。

### 5.7 LVGL 组件映射（对照 UI 设计 §10）

> UI 设计文档 §10 与本节描述同一件事（视觉规范 → 框架能力的映射），两侧须逐条对齐。**功能语义与视觉规格（色板 / 尺寸 / 字号 / 文案 / 确认分级）一律不变**，变的只是"用什么框架能力实现"。

| UI 文档 §10 条目 | LVGL 侧等价映射 |
|--------------------|---------------------------|
| `global Palette / Dimens / Fonts` | `ui/theme.rs` 的 `lv_style_t` 集合 + `const` 常量（**仍是单一真源**；页面不硬编码） |
| 6 页 + 常驻导航（`page-index`） | `lv_tabview`（隐藏内置标签栏）或 6 个 `lv_obj` 容器 + `LV_OBJ_FLAG_HIDDEN` 切换；导航栏 = 6 个 `lv_btn` |
| 整页纵向滚动（`Flickable`，不横滚） | 滚动容器 `lv_obj` + `LV_OBJ_FLAG_SCROLLABLE`；`lv_obj_set_scroll_dir(obj, LV_DIR_VER)` 即**结构性禁止横滚** |
| 长列表（可视行裁剪） | `lv_list` 或 `lv_table`（LVGL 不自带虚拟滚动裁剪；**长列表须自行做"窗口化"**：只保留可视行数 ×1.5 的 `lv_obj`，滚动时复用）。见 §12.4 工作单元 B 与 R-22 |
| 分段控件 / 多选 Chip（不用内置 `ComboBox`） | `lv_checkbox`（多选）/ `lv_buttonmatrix`（分段，v9.5 规范名；`lv_btnmatrix` 为 `lv_api_map_v8.h` 的兼容别名）/**允许** `lv_dropdown`（LVGL 的下拉弹出层样式可经 `lv_style` + `LV_PART_*` 覆盖，"内置弹层不可控"的顾虑在 LVGL 下不成立） |
| 步进器 / IPv4 / 日期时间 | **`lv_btn` + `lv_label` 组合（`−` / 值 / `＋` 三件；弃 `lv_spinbox`，见 §5.6 F12 行）**：值区为**纯 `lv_label`**（不挂 `LV_OBJ_FLAG_CLICKABLE`），`−`/`＋` 为 `lv_btn`（`−` 在 `value==min`、`＋` 在 `value==max` 时置 `LV_STATE_DISABLED` 给类型+控件双层越界约束）；IPv4 四段 = 4 个该组合（每段 0–255）；日期时间 = 5 个该组合（**不用 `lv_roller` 备选**，与 UI §5.3 #8 一致） |
| 确认对话框（`PopupWindow` + 遮罩 + 默认焦点 + 长按） | `lv_msgbox` 或 `lv_layer_top()` 上的模态容器（`lv_obj_add_flag(..., LV_OBJ_FLAG_CLICKABLE)` 拦截穿透）；遮罩用 `lv_obj_set_style_bg_opa`；长按用 **`LV_EVENT_LONG_PRESSED` + `lv_indev_set_long_press_time(indev, 1000)`**（逐控件 API 在 v9.5.0 不存在，§5.6） |
| Toast（`Timer 3s`） | `lv_msgbox`/自定义浮层 + `lv_timer_create(..., 3000, ...)`（或 Rust 侧 `idle_deadline` 语义） |
| 状态胶囊 / 指示灯（签名强制 `text`） | `StatusChip`/`LedIndicator` 组合件（`lv_obj` + `lv_label` + `lv_led`），构造函数强制三通道（§5.6 F14 行） |
| 脏区渲染 / 低 CPU（`draw_if_needed(render_by_line)`） | LVGL 内建脏区失效（`lv_obj_invalidate`）+ `lv_timer_handler()` 按需重绘；`lv_display` 用 `LV_DISPLAY_RENDER_MODE_PARTIAL` + 双缓冲（§1.1.1.1） |
| 动效（仅 `animate 200ms`） | `lv_anim`（仅状态切换，≤200 ms；禁循环动画，§5.6 动效纪律行） |
| 零文本输入 | **`lv_conf.h` 置 `LV_USE_TEXTAREA=0` / `LV_USE_KEYBOARD=0` / `LV_USE_SPINBOX=0`**（三者必须同时为 0；弃 `lv_spinbox` 是前提，步进器改 `lv_btn`+`lv_label` 组合，§5.6 F12 行）→ 编译期不存在任何文本输入控件，强于源码扫描 |
| 导航 ≤2 次触摸的可测性（离屏事件注入） | 离屏测试直投 `lv_indev`（`lv_indev_read`）→ 断言当前页（§11.1 交互层） |
| 禁内置控件皮肤 | **不适用**：采用 LVGL 内置控件 + `ui/theme.rs` 自定义主题（§5.6 控件策略行，理由见 §5.1 注） |
| 滚动条"纯指示不响应触摸" | **内容滚动手势保留**（拖拽 + 惯性），**滚动条严格纯指示、不可拖**。§5.6 已给 **v9.5.0 源码依据**（滚动条为纯绘制部件，输入侧无命中测试；`LV_PART_SCROLLBAR` 非 `lv_obj`，无部件级 clickable）与**两手实现**（A：原生部件 + 不引入交互 + 交互用例钉死；B：自绘 hit-transparent 指示）。UI 文档侧对应条目同口径；**8 px 与 UI §2.1 的 48 px 不冲突**（滚动条非触摸目标，容器才是） |
| 长按 1.0 s | 用 LVGL 内建 `LV_EVENT_LONG_PRESSED` + **`lv_indev_set_long_press_time(indev, 1000)`**（v9.5.0 无逐控件长按时长 API，阈值收敛于 indev，§5.6 长按行） |

> **与 UI 侧的对应关系（已对齐）**：① 控件策略（内置控件 + 自定义主题）不影响 UI §5.2 的"状态 × 色值矩阵"表述（矩阵本身不变，只是由 `lv_style` 施加）；② 滚动条口径"纯指示、不可拖"，UI 文档对应条目已同口径（实现分支按 §5.6.1 的 A/B 取一）；③ UI §11 的真机验证项 V-4/V-5/V-7 判据不变（**V-7 仅验指示清晰度**，不含"是否可拖"），但 V-5（列表滚动 ≥30 fps）**须关注"窗口化列表"的实现方式**（R-22）。

---

## 6. 6 页详细设计

> 每页给出：PRD 功能映射 → 数据来源 → 交互流程 → 写操作 → 降级。所有页面**无文本输入控件**。

### 6.1 P1 主状态页（默认页 / 超时回归目标页）

| PRD | 内容 | 数据来源 | 交互 |
|-----|------|----------|------|
| F1 SOC | 大字号 %（**UI §3.3 L1-特大 = 148 px**；单位 `%` 取 L1-单位 56 px）+ 源胶囊（`BMS` / `PCS(REG1010)` / `SOC 源失效`）+ 0–100 量程条（15 %/85 % 警示档） | 帧 `soc`/`soc_source`/`soc_flag` | 只读 |
| F2 PCS 状态 | 四态文字+语义色+图标；「方向不一致」角标 | 帧 `run_state`/`inconsistency`（主判据 REG1013） | 只读 |
| F3/F4 三相 P/I | 四卡横排（A/B/C/总，上 P 下 I），1 位小数 | 帧 `p_phase`/`p_total`/`i_phase` | 只读 |
| F5 刷新/新鲜度 | 「数据过期」角标（>2 s）；通道断整屏降级 | 帧 `ts_ms`/`seq` + `ChannelStatus` | 只读 |
| F6 装置状态 | 状态卡网格：版本、编译时间、uptime、CPU 温度、内存、IEC104 / 核间 / 通道、控制源 | 帧 `device` + `info` | 只读 |
| F7 告警 | 最多 10 条，倒序，级别色+文字；空态/源不可用分别显式 | 帧 `alarms` | 只读 |

- 布局（1024×768，**主读数区不横滚**，整页为 LVGL 纵向滚动容器（`lv_obj` + `LV_OBJ_FLAG_SCROLLABLE` + `lv_obj_set_scroll_dir(LV_DIR_VER)`），PRD T-6/B8）：页眉（时钟 + 通道状态）→ SOC 卡 | PCS 卡 → 三相四卡 → 装置状态网格 → 告警列表。
- 布局固定分区、字段位置稳定（值变化不改布局，PRD §4.2.4）。
- 无写操作。

### 6.2 P2 配置页（F9，含写操作）

| 环节 | 设计 |
|------|------|
| 进入 | 触摸导航「配置」→ `GET /v1/console/config` → 按 `groups` 渲染分组（IEC 104 / 核间 / 遥测与日志），组内字段纵向排列 |
| 控件生成 | 由 `ConfigField.kind` 驱动：`Ipv4` → 四段数字步进（每段 0–255）；`U16/U64{min,max,step}` → 受约束步进器 —— **此二者均为 `lv_btn` + `lv_label` 组合（`−` / 值 / `＋`），弃 `lv_spinbox`（§5.6 F12 行）**，`−` 在 `value==min` / `＋` 在 `value==max` 时 `LV_STATE_DISABLED`，**越界值在控件层不可达**（TT-03）；`Enum{options}` → 选项列表（`lv_dropdown` / `lv_buttonmatrix`） |
| **只读字段**（`editable=false`） | 本地 HMI 自身的服务地址（`display.bind_addr` / `display.control_bind_addr`）**只读展示**：控件 `disabled` + 附「仅本机回环，不可修改」说明行。理由：回环是 PL-4 安全红线，经屏可改即等于把"只回环"变成可撤销的约定（§3.4 / §4.9）；字段仍出现在列表中以**可见性**换取现场可核查性 |
| 监听地址字段口径 | `gateway.listen_addr` 的标签为「**本机监听地址（IEC 104）**」，**不得**表述为「对端 IP / 远程主站地址」；`Ipv4` 步进的语义是**本机绑定地址**。同页若出现回环服务地址，须按「本机服务地址（仅回环 127.0.0.1）」独立成行标注，与设备管理 IP 区分（§6.6） |
| 即时校验 | 值变更即本地校验（步进器天然受限）；后端二次校验（`RejectedValidation` + `field_errors` → 字段红框 + 具体原因，红色边框 + 文字） |
| 保存 | `保存` 按钮 → **按 UI §2.5 分级确认**：本次改动**未**涉及 `requires_reconnect == true` 字段 → **L1**（双步：模态弹层列出「字段：旧值 → 新值」，默认焦点「取消」）；本次改动涉及任一 `requires_reconnect=true` 字段 → **L2+**（危险色 + **长按保持 1.0 s** + 必出 `WarnBanner`「生效瞬间通信将短暂中断（≤5 s）」及「涉及：<字段名>」）→ 确认完成后 `POST /v1/console/config/apply`（`from: "edit"`） |
| 保存中 | 按钮 `disabled` + 文案「保存中…」（禁重复触发，F9.6） |
| 成功 | `Toast`（3 s 自动消失）+ 用回执 `applied` 刷新本地值（不等下一帧） |
| 失败 | `Toast`（错误色）+ **保留用户已输入值** + 明示原因（EDGE-10：校验失败 / 落盘失败 / 落盘成功但生效失败须区分） |
| **恢复默认值**（F14.3 / T-3 确认落点） | 独立控件，与「保存」间距 ≥48 px（CF-07）。**属"生效性写"→ 必须走二次确认（L2），不得只有间距保护**：`ConfirmDialog(level = L2)` 危险变体；标题「恢复默认值」；**「影响范围」段必出**（「全部运行参数将恢复为默认值并立即生效」）；明细段列出「字段：当前值 → 默认值」（「当前值」取**后端注入的真值**、不是控件近似显示值，取不到时显占位符 `–`；只列真值 ≠ 默认值的字段，避免「`1 → 1`」式伪变更行；「默认值」取自 `ConfigField.default`；超 8 行内部滚动）；若恢复值涉及 `requires_reconnect` 字段则追加 `WarnBanner`；**确认按钮须长按保持 1.0 s**（中途松手即取消并复位），**默认焦点「取消」**。确认完成 → `POST /v1/console/config/apply`，携带 `ConfigPatch{ changes: <全字段默认值>, from: "reset_default" }` |
| **UI ↔ 后端 确认-审计链路（T-3 闭环）** | **UI 侧**：长按 / 双步确认**完成前不发出任何请求**（未确认 = 无网络动作）；确认完成 → `console.rs` 生成 `request_id`(uuid) + `issued_at_ms` → POST。**后端侧**：`ConfigService` 走 §3.3 固定管线（字段校验 → 幂等占位 → **审计 intent 前置写入，fail-closed** → 执行 → 结果审计），回执带 `audit_id`；审计条目 `op = ConfigApply \| ConfigResetDefault`、`target` = 逐字段键、`before` / `after`、`result` / `reason`、`write_mode`（§4.3.2.1）。**「确认」与「审计」是两条独立证据链**：确认防误操作、审计做留痕，缺一不可（D7 / D8）；失败时 UI 保留已输入值并就地展示 `message` |
| 未保存草稿 | `dirty=true` → 顶部提示条；超时回归不强制切页（F15.2/EDGE-11） |

### 6.3 P3 日志页（F10，只读 + 选项式筛选）

| 环节 | 设计 |
|------|------|
| 实时通道状态 | 顶部：「实时日志已连接」（绿）/「实时日志已断开，正在重连…」（红）。断开由控制通道请求失败判定（连续 2 次失败），恢复后 ≤1 s 回绿（LG-07 的重连 ≤5 s 由客户端 500 ms 重试节拍天然满足） |
| **筛选控件常驻页面**（关键设计） | 三行常驻 chip 组：① 级别多选（ERROR/WARN/INFO/DEBUG）② 模块多选（选项来自 `/logs/targets`）③ 时间范围（最近 1 h / 24 h / 自定义起止） —— **常驻而非抽屉**，使「3 维度各 1 次触摸 = 3 次」满足 LG-05「≤3 次触摸」；自定义起止展开后为日期+时间步进（LG-04） |
| 列表 | **斑马纹（仅 P3 日志行；审计行不画）**、行高 ≥40 px（LG-06）、级别色块+文字；每行「时间 级别 模块 消息」；消息列**单行截断 + `…`**（不换行） |
| 滚动 | LVGL 滚动容器（`LV_OBJ_FLAG_SCROLLABLE`）+ **窗口化列表**（只保留可视行 ×1.5 的 `lv_obj`，§5.7 / R-22）+ 滚动加载（`cursor` 增量）；手动上滚 → **停止自动滚动** + 右下**恒显**「回到最新」按钮（落在列表区下方专属带内、不遮正文；「浮现」因薄层无滚动事件而不可实现）（F10 实时推送规范 2） |
| 实时追加 | 每 500 ms 拉 `cursor` 增量（延迟 ≤1 s，优于 LG-01 的 2 s）；重连期间**不清空**已展示内容（F10 规范 3） |
| 空态 | 「当前筛选条件下无日志」（EDGE-08） |
| 超限 | `range_too_large=true` → 提示「检索范围超限，请缩小时间范围」（EDGE-15） |
| 明确不做 | 关键字搜索（T-1）、导出（T-2）、任何文本输入（LG-10） |

### 6.4 P4 安全 / 联锁页（F16–F18，含写操作）

| PRD | 内容 | 数据来源 | 交互 |
|-----|------|----------|------|
| F16 状态 | 总态「已联锁 / 未联锁」（文字+色+图标）；触发源明细列表（含数量）；latch 态（已保持/未保持）；停机失败标志；DO1/DO2 灯（灯+文字，未知显「未知」）；空态「当前无联锁触发源」 | 帧 `interlock`（2 s） | 只读 |
| F17 释放 | 按钮 ≥64×64 px → **L2 强确认**（UI §2.5 / §7.3）：危险色弹层 + 影响范围段（当前触发源与 latch 态）+ 明细列表 + **确认按钮长按保持 1.0 s**（中途松手取消）+ **默认焦点「取消」** → `POST /v1/console/interlock/release`（带 `observed_latched` / `observed_sources`） | 控制通道 | 写 |
| F18 M1 授权 | 按钮 ≥64×64 px；**latch 态下 `disabled` 并就地明示「处于 latch 态，须先释放联锁」**（F18.4，不得静默失败）→ **L2 强确认**（同 F17 规格）→ `POST .../ack_m1` | 控制通道 | 写 |
| 失败 | `RejectedPrecondition` → 弹层内就地显示 `message`（如「触发源未复位：estop, door」「保持时间不足，还需 12 s」） | — | — |
| 成功 | ≤2 s 内联锁态更新（F17.6/IL-02）：**优先**由回执 `applied` **立即**刷新（不等下一帧）；退路为下一帧（慢拍 C 0.5 s + 变更即组帧，最坏 ≤1.35 s，§4.2.1）；`Toast` 成功 |
| 不可用 | `interlock.available=false` → 整区显「联锁状态不可用」，**不得**显「未联锁」（IL-01.6/EDGE-12） |

> **DO 命名备注**：页面上只显示语义（「故障灯」/「运行灯」）与灯态，不显示 DO1/DO2 编号（避免 PRD 与代码注释不一致导致的现场误读，§4.6 备注 / §14 R-09）。

### 6.5 P5 审计页（F19，只读）

| 环节 | 设计 |
|------|------|
| 顶部 | 最近一条审计时间戳（F19.8，判断审计链路是否持续写入） |
| 筛选（常驻） | 时间范围（1 h / 24 h / 自定义起止，选项式步进）+ 操作类型多选（选项来自 `/audit/ops`） |
| 列表 | 每页 20 条，时间倒序，滚动加载；每条：时间 / 操作者（`local-console`）/ 操作类型 / 前后值摘要 / 结果（成功/失败）/ 失败原因 |
| 只读 | 无编辑/删除/清空入口（PL-02 的界面体现）；无导出（T-2 同口径） |
| 空态 / 不可用 | 空态「当前筛选条件下无审计记录」；`available=false` → 「审计记录不可用」（EDGE-17，二者严格区分） |

### 6.6 P6 系统 / 关于页（F8，只读）

| 字段 | 来源 | 缺失时 |
|------|------|--------|
| 固件版本号 | `env!("CARGO_PKG_VERSION")` | 不适用 |
| 编译时间 | `option_env!("BUILD_TIMESTAMP")` | 「未提供」 |
| 装置型号 | `/proc/device-tree/model` | 「未提供」 |
| 序列号 | **无可靠真源** | 「未提供」（不臆造，EDGE-16/F8.4） |
| **本机服务地址（仅回环）** | `InfoSection.service_scope = LoopbackOnly`（常量）+ 读/控制通道端点 `127.0.0.1:9810` / `127.0.0.1:9811` | 恒有值（非「未提供」） |
| **设备管理 IP** | `InfoSection.mgmt_ipv4`（`getifaddrs` 首个 UP 非回环 IPv4） | 「未提供」 |

- 「字段名 + 值」列表形式；不随数据刷新跳动（一次性读取，F8.3）。
- **服务地址展示口径**：上两行**必须分列、字段名不可互换**——
  「本机服务地址（仅回环）」表示**服务只监听 127.0.0.1，对外不可达**；
  「设备管理 IP」表示**该装置在管理网上的地址**。
  页面不得出现任何"可从远端访问本机 HMI 接口"的暗示（例如把管理 IP 与服务端口并列成"访问地址"）。此口径同时约束 §6.2 的监听地址字段。
- 无写操作。

---

## 7. web-api 移除方案与出口迁移表

### 7.1 出口迁移逐条表（PRD §3.7 的 16 行 → 技术落点）

| # | 原 web-api 出口 | PRD 处置 | **技术落点** | 承接组件 | 工作量 |
|---|------------------|----------|--------------|----------|--------|
| 1 | `GET /api/v1/interlock/status` | 迁移（P4/F16） | 读帧 `interlock` 段（0.5 s 节拍 + 变更即组帧，端到端 ≤1.35 s，§4.2.1） | `DisplayDataProvider` + 任务 C | S |
| 2 | `POST /api/v1/interlock/release` | 迁移（P4/F17） | `POST /v1/console/interlock/release` | `InterlockOps` + `ConsoleAuditService` | M |
| 3 | `POST /api/v1/interlock/ack_m1` | 迁移（P4/F18） | `POST /v1/console/interlock/ack_m1` | 同上 | M |
| 4 | `GET /api/v1/status` | 迁移（F6/F7/F8） | 读帧 `device`/`alarms`/`info` 段 | `DisplayDataProvider` + 任务 A/B + build.rs | **M**（四字段由占位转真值，含 gateway `link_state` 新增） |
| 5 | `GET/PUT /api/v1/config` | 迁移（P2/F9） | `GET /v1/console/config` + `POST /v1/console/config/apply`（+ reset） | `ConfigService`（**新建子系统**） | **L** |
| 6 | `GET /api/v1/logs` | 迁移（P3/F10） | `GET /v1/console/logs`（选项式 + cursor + 限额；**去掉 keyword/export**） | `LogService`（迁移 `LogsHandler`） | M |
| 7 | `GET /ws/logs` | 迁移（F10 实时） | `GET /v1/console/logs?cursor=`（500 ms 增量拉取，≤1 s） | `LogService` live ring | M |
| 8 | `GET /api/v1/strategy-mode` | 迁移（只读） | 读帧 `device.control_source` | `AiIntegrator::engine_status()` | S |
| 9 | `PUT /api/v1/strategy-mode` | **暂停** | 无端点（后端已 503 拒绝；代码随 crate 删除） | — | S（删除） |
| 10 | `GET/PUT /api/v1/mode`、`/mode/list` | **暂停** | 无端点 | — | S（删除） |
| 11 | `GET /api/v1/ai/*` | **暂停** | 无端点 | — | S（删除） |
| 12 | `PUT /api/ai/weights`、`GET /api/ai/audit` | **暂停**；但「写操作审计留痕与查询」作为通用能力保留 | **审计查询迁移** → `GET /v1/console/audit` + `/v1/console/audit/ops`；AI 干预类端点删除 | `ConsoleAuditService` | M |
| 13 | `/api/ai/models`、`/api/ai/abtest/*`、`/api/ai/rollback` | **暂停** | 无端点 | — | S（删除） |
| 14 | `/api/auth/*`（login/logout/password） | **取消**（T-3 无登录） | 全删（含 `AuthHandler` / `SessionManager` / `RequireRole`） | — | S（删除） |
| 15 | `GET /api/v1/ai/stream`（SSE，真路由名） | **迁移（能力级）**；其 AI/场景事件随 §3.8 暂停 | 实时刷新能力由**读帧 1 Hz + 日志 cursor 轮询**独立满足，**不复用该端点** | 读通道 + `LogService` | S |
| 16 | 静态 Web 资源 | **取消**（本就未挂载） | 删除孤立 `static/*.html` | — | S（删除） |

**归类统计（与 PRD §3.7「迁移 10 / 暂停 4 / 取消 2」对齐）**：

| 归类 | 条目 | 小计 |
|------|------|------|
| **迁移** | #1 #2 #3 #4 #5 #6 #7 #8 + #12（审计查询能力）+ #15（实时推送能力级） | **10** |
| **暂停** | #9 #10 #11 #13（#12 的 AI 干预部分并入 #11/#13；#15 的 AI 事件部分并入暂停） | **4** |
| **取消** | #14（登录面）+ #16（静态资源） | **2** |

> 与 PRD 表一致；本表的增量信息是**每一行的具体技术落点与工作量**，其中 **#5（配置）与 #4（状态真值化）**是净新增工作的重心。

### 7.2 `web-api` crate 处置

**处置：整 crate 删除。**

删除顺序（**必须按此序，否则编译中断**）：

```
Step 1  迁出类型：InterlockApi/InterlockStatus/InterlockSourceStatus → display-proto::interlock（含结构化错误改造）
                 同时改造 mupc-core-bin/src/interlock.rs 的 impl 与 use
Step 2  新建 mupcd 承接组件：ConfigService / LogService / ConsoleAuditService / InterlockOps / ConsoleHost / AlertFeed
Step 3  替换 startup.rs 步骤 10 整块（AppState 装配 + Router + 监听 + register_service）
        替换 SouthSink.sse 字段 → AlertFeed
Step 4  删除 CoreConfig.web_api 字段 + WebApiConfig + validate 校验 + 默认值函数 + 单测样例
Step 5  删除 crates/web-api 目录 + workspace members 条目 + core-bin Cargo.toml 的 mupc-web-api 依赖
        ⚠️ 本行原写「/ axum 依赖」——**该部分已过期（单元 K 实测）**：`console_host` 直接用
        axum，`axum` **必须保留**（逐字理由见 §7.3 的 core-bin / workspace 两行注记）
Step 6  清理残余：yaml 段、部署文档、注释性引用（无害但应清）
```

### 7.3 受影响代码改动清单（逐文件）

| 文件 | 改动 | 说明 |
|------|------|------|
| `mupc/Cargo.toml` | 删 member `crates/web-api`；清理 `[workspace.dependencies]` 中仅 web-api 使用的 `tower-http` / `jsonwebtoken`（须先确认无其它使用者）<br>⚠️ **「清理 `axum`」已过期（单元 K 实测）**——`axum` **必须保留**（`console_host` 直接用 axum，理由同下一张表 core-bin 行）；**本行仅把 `axum` 从清理清单划掉，其余处置不变** | — |
| `mupc/crates/web-api/**`（32 个 .rs + static） | **整目录删除** | 含其全部单测 |
| `mupc/crates/mupc-core-bin/Cargo.toml` | 删 `mupc-web-api`、`axum`（core-bin 直接用 axum 仅为止 Router 装配）；**LVGL 相关依赖不在此时 crate**（HMI 才需要）<br>⚠️ **「删 `axum`」已过期（单元 K 实测）**——该判断写于 `console_host` 落地**之前**（"删掉 web-api 后 core-bin 只剩 Router 装配"）；而单元 G/H/I/J/K 新增的本地 HMI 控制通道**正在用 axum**（`console_host.rs:134-139` 的 `Router` / `extract::{Query, State}` / `Json`，`:385` 的 `axum::serve` 听 `display.control_bind_addr`）⇒ **`axum` 必须保留**，删掉即本 crate 编译失败。**本行仅订正「删 `axum`」这半句，其余处置不变** | — |
| `mupc/crates/mupc-core-bin/src/core_config.rs` | 删 `web_api` 字段 / `WebApiConfig` / `default_listen_addr` / `default_enable_https` / `validate()` 该校验 / **36 处**单测 yaml 样例中的 `web_api:` 段（⚠️ 订正 S-6：原文写「约 20 处」，单元 K 实测为 36，量法 `git diff -- mupc/crates/mupc-core-bin/src/core_config.rs \| grep -c '^-.*web_api:'`）；**新增** `display` 段新字段的 validate | 改动面大但机械 |
| `mupc/crates/mupc-core-bin/src/startup.rs` | 步骤 10 整块替换（约 −90 行 / +约 60 行）；`SouthSink.sse` → `alert_feed`；`register_service("web_api")` → `("hmi_backend")`；`ota_manager` 的创建失去唯一消费者（见下） | 关键路径 |
| `mupc/crates/mupc-core-bin/src/interlock.rs`（约 1300 行） | `use` 改指 `display-proto::interlock`；`request_release`/`ack_m1` 返回 `Result<(), InterlockReject>`；错误构造改造；既有单测同步 | **中风险回归点** |
| `mupc/crates/mupc-core-bin/src/display_host.rs` | 扩展：慢拍任务 A/B/C + 缓存 + 帧组装的四个新段；`LoopbackHttpPublisher` 保持 | 核心复用面 |
| `mupc/crates/mupc-core-bin/src/console_host/**`（新） | `mod.rs` / `config_service.rs` / `log_service.rs` / `audit_service.rs` / `interlock_ops.rs` / `http.rs` / `error.rs` | 新增 |
| `mupc/crates/mupc-core-bin/build.rs`（新） | `BUILD_TIMESTAMP` | 新增 |
| `mupc/crates/mupc-core-bin/src/main.rs` | 装配顺序/步骤编号 10–14 → 调整 | 小 |
| `mupc/crates/display-proto/src/**` | `frame.rs` 扩段 + `PROTO_VERSION=2`；新增 `control.rs` / `interlock.rs` / `log.rs` / `audit.rs`；`config.rs` 扩字段 | 契约核心 |
| `mupc/crates/local-display/**` | 见 §5.1 模块树（渲染层换 **LVGL**；`font.rs`/`layout.rs`/`run.rs` 废弃；**新增** `lvgl-sys/`、`src/lvgl/**`、`timing.rs`、`ui/**`、`fonts/`（`lv_font_conv` 产物）；`canvas.rs` **保留**） | HMI 核心 |
| `mupc/vendor/lvgl/**`（**新**） | LVGL v9.5 官方源码，**pin tag**（git submodule 或 vendor 目录，§12.1）；`lv_conf.h` 由我们提供 | 新增（C 依赖） |
| `mupc/Cargo.toml`（workspace） | `members` 增 `crates/local-display/lvgl-sys`；`[workspace.dependencies]` 增 `cc` / `bindgen`（**build-dependencies 语义**，见 §12.1） | 新增 |
| `mupc/build.md` / `mupc/deploy/scripts/build-for-rk3588.sh` | 增补 **C 工具链前置**（aarch64 gcc 已有）+ `LIBCLANG_PATH`/`BINDGEN_EXTRA_CLANG_ARGS` 说明 + `--hmi` 子模式（§12.1） | 构建 |
| `mupc/crates/gateway/src/iec104/server.rs` | 新增 `link_state()` + 状态枚举 | T-5 #1 |
| `mupc/crates/ai-engine/src/model_manager.rs` | 注释中「供 bin crate / web-api SSE 推送使用」→ 改述（纯注释） | 无害 |
| `mupc/crates/strategy-engine/src/ai_integration.rs` | 注释中「web-api 的服务门面」→ 改述（纯注释） | 无害 |
| `mupc/crates/security/src/audit.rs` | 单测中的 `"web-api"` 字面量 → 通用串（纯测试数据） | 无害 |
| `mupc/deploy/config/mupc_core_config.yaml` 与 `.production.yaml` | 删 `web_api:` 段；加 `display:` 的 `enabled/control_bind_addr/*_poll_ms/log:` | 部署 |
| `mupc/deploy/config/mupc_core_config.production.yaml` | 同上 + 开启 `display.enabled: true` | 部署 |
| `mupc/deploy/systemd/mupc-display.service` | 改 `ExecStart` 参数（+ `--control-channel` / `--touch-device` / `--font`）；内存上限按预算调整 | 部署 |
| `mupc/deploy/systemd/mupcd.service` | 若启用 `ProtectSystem=strict`，须加 `ReadWritePaths=/opt/mupc/config`（**配置热写前提**，须核对现有 unit） | 部署 |
| `mupc/deploy/deploy.md` / `deploy/scripts/*.sh` | 8080 相关描述/检查清理；HMI 构建打包 | 部署文档 |

**未决/附带影响（诚实标注）**：
- `ota_manager` 在 `startup.rs` 中**唯一消费者是 web-api 的 AppState**。删除后该实例失去用途；OTA 属 §3.8 暂停项。建议：**保留 `ota_update` 服务注册与实例（不删除能力），但不启动任何服务面**；或在后续 OTA 需求中重新接线。**不要**因此连带删除 `mupc-ota-update` crate。
- `mode_selector` / `ab_test_manager` / `online_updater` / `storage` 均非 web-api 专有（`storage` 有独立用途；其余属 AI 暂停项）。
- **配置文件向后兼容（不止"能加载"，还要"不被抹掉"）**：`CoreConfig` 未设 `deny_unknown_fields`，故**现场既有的带 `web_api:` 段的 yaml 仍可正常加载**（未知字段被忽略），不强制运维立即改文件。**进一步地**：配置**回写**必须同样不破坏现场文件——本设计采用**保留式编辑**，保存时注释、未建模键与 legacy `web_api:` 段**逐字保留**；仅在不可定位时显式回退整体回写并声明丢失（见 **§4.3.2.1 / D16 / EDGE-23**）。这两点合起来才是完整的兼容性主张：**"能读"且"写了不丢"**。

---

## 8. 复用 / 废弃清单（既有资产去留）

### 8.1 明确复用（保留 / 小改）

| 资产 | 位置 | 处置 | 理由 |
|------|------|------|------|
| 帧字段标志与语义 | `display-proto::frame::{FieldFlag, Field, RunState, SocSource}` | **原样保留** | 与 PRD「不造假值」语义强绑定，已单测覆盖（含越界拒绝与 JSON 往返） |
| `DisplayFrame` | `display-proto::frame` | **扩展为 v2**（新增 4 段 + `PROTO_VERSION=2`），旧字段不动 | 向后兼容（`serde(default)`），HMI 侧改动可控 |
| `DisplayConfig` / `DisplayRange` | `display-proto::config` | **扩展**（+ `control_bind_addr` / `min_publish_interval_ms` / 慢拍节拍（3 s / 0.5 s / 0.5 s）/ `log` 限额） | 单一真源不变 |
| 域值化 / 量程 / 一致性 | `core-bin::display_host::DisplayDataProvider` | **保留**（`scalar_field` / `phase_fields` / `check_inconsistency` 原样） | 与 PRD EDGE-05/06 逐条对齐，已单测 |
| 1 Hz 采集与发布循环 | `display_host::{sample_once, build_frame}` | **保留并扩段** | 核心复用 |
| 读通道 HTTP 发布 | `display_host::LoopbackHttpPublisher`（含头读超时、毒化不 panic、序列化失败 500） | **原样保留** | 已过评审；仅新增第二监听 |
| SOC 裁决快照 | `strategy-engine::AiIntegrator::{soc_display_snapshot, resolve_soc_core}` | **原样保留（不改）** | 唯一裁决入口，控制/展示不分叉 |
| 三相读 | `intercore::read_three_phase` / `last_run_state` / `is_connected` | **原样保留（不改）** | 点表读取与在线副作用已落地 |
| 回环 HTTP 客户端 | `local-display::channel::DisplayChannelClient`（裸 tokio + 手写 HTTP，无 reqwest） | **保留并泛化**为通用客户端（GET + POST），供控制通道复用 | 依赖面最小、可 mock |
| 三态归一 / 新鲜度 / 通道态 | `local-display::state::{NumView, Freshness, ChannelStatus, ScreenMode, SocView, LiveDot}` | **保留**（纯逻辑，可单测）并扩展新段视图 | 已过评审；是「不造假值」在前端的落点 |
| fbdev 像素后端 | `local-display::canvas::FbCanvas`（`/dev/fb0` mmap + bpp/位偏移格式探测 + 区域写） | **保留**：作为 `lv_display` 的 `flush_cb` **像素 sink**（§1.1.1.1 P-1）；仅其**绘制原语契约**与 `OffscreenCanvas` 布局用途废弃 | 以自控的 flush 路径规避驱动对像素格式的约束；真机像素格式处理逻辑已写且已过评审 |
| CLI 解析 | `local-display::config`（手写解析 + 校验 + 平台可用性判定） | **保留并扩展**（+ `--control-channel` / `--touch-*` / `--idle-timeout-secs`） | 依赖面最小、有单测 |
| 字库**码表**与子集化流程 | `font_subset_charset.txt`（既有码表）+ UI §3.6 用字表 | **码表复用并扩充**；**产物重建**为 `lv_font_conv` 的 C 字体数组（§1.1.2）——**既有 `pyftsubset` OTF 产物不再使用** | 不赌目标镜像带 CJK 字库；LVGL 需要编译期 C 字体而非运行时 OTF |
| 语义色板 | `local-display::layout` 内 `#0B1220 / #141F33 / #28A745 / #FFC107 / #DC3545 / #17A2B8 / #9AA0A6 …`（`layout.rs` 顶部 `const`） | **迁入** `ui/theme.rs` 的 `const` + `lv_style_t`（页面不得硬编码，§5.6） | 与 PRD §3.1/§3.3 与 UI §3.2 色值一一对应 |
| 联锁 controller | `core-bin::interlock::InterlockController` | **保留**（实现迁出的 trait + 错误结构化） | 已实现 DI 去抖/latch/停机确认/DO 灯 |
| 安全审计底座 | `mupc_security::audit::AuditLogger`（JSONL + **SHA-256** 哈希链；SM3 替换为 Phase 2+ 遗留项） | **复用**为审计双写的一端 | 不破坏既有合规凭据 |
| 事件存储 | `mupc_storage::EventRepository`（`SystemEvent`） | **复用**为 F7 告警真源 | 已有写入路径 |
| 系统指标 | `mupc_system_monitor::{TemperatureCollector, MemoryCollector}` | **复用**为 F6 数据源 | 已在 `startup.rs` 运行 |
| 部署 unit 骨架 | `deploy/systemd/mupc-display.service` | **保留并改内容**（组、参数、内存上限） | 部署脚本引用不变 |

### 8.2 明确废弃（删除）

| 资产 | 位置 | 规模 | 废弃理由 |
|------|------|------|----------|
| 固定网格自绘布局 | `local-display/src/layout.rs` | 1003 行 | 被 LVGL 页面/控件树取代；只有色板常量被迁出保留 |
| ab_glyph 文本光栅化与图集 | `local-display/src/font.rs` | 439 行 | 文本渲染交给 LVGL（**码表资产**迁至 `fonts/`，渲染与图集全部废弃） |
| 定拍自绘主循环 | `local-display/src/run.rs` | 426 行 | 被「evdev + `lv_timer_handler` + 通道」事件循环取代（`timing.rs`） |
| 离屏画布与绘制原语 | `local-display/src/canvas.rs` 的 `Canvas` trait / 绘制原语 / `OffscreenCanvas` 布局用途 | 部分 | 绘制由 LVGL 负责；离屏测试改用 LVGL 内存 display；**`FbCanvas` 本体保留**（§8.1） |
| 既有端到端测试 | `local-display/tests/full_chain.rs` | 405 行 | 断言基于自绘像素网格；改为 LVGL 离屏 + 页面状态断言 |
| **既有 `pyftsubset` 的 OTF 子集产物** | `local-display/fonts/*.otf` | — | LVGL 需编译期 C 字体（`lv_font_conv`），OTF 产物不再被使用；**码表资产保留**（§8.1） |
| `web-api` crate | `crates/web-api/**` | 32 文件 | PRD B4/PL-4/PL-5（§7.2） |
| 既有 UI 设计文档 | `docs/superpowers/plans/modules/12-MUPC-本地显示终端-UI设计文档.md` | — | 其「只读状态屏」版已废弃；**现行版为触摸式本地 HMI**（6 页 IA + 控件集 + 强确认分级），其 §10「与技术设计的可映射性」已按 LVGL 重写——本设计在 **§5.7** 给出对应的 LVGL 侧映射表；U-1 的回环口径裁定（→ §3.1 / §6.2 / §6.6 / EDGE-24）仍有效 |

### 8.3 复用清单复核结论

| 复核项 | 结论 | 依据 |
|--------|------|------|
| `canvas.rs::FbCanvas` | ✅ **保留**——它是 `flush_cb` 的 **sink**，是显示后端 P-1 的**确定组成部分** | §1.1.1.1 / §1.3 |
| `canvas.rs` 的 `Canvas` trait / 绘制原语 / `OffscreenCanvas` | ✅ **废弃**（绘制归 LVGL；离屏归 LVGL 内存 display） | §1.1.1.1 |
| `layout.rs`（固定网格自绘） | ✅ **废弃**（LVGL 控件树取代）；**色板常量迁出保留** | §8.1 |
| `font.rs`（ab_glyph 光栅化） | ✅ **废弃**（LVGL 接管文本）；**但码表资产保留、OTF 产物废弃** | §1.1.2 / §8.1 |
| `run.rs`（500 ms 定拍自绘主循环） | ✅ **废弃**（新事件循环，由 `lv_timer_handler` 驱动） | §5.2 |
| `channel.rs` / `state.rs` / `config.rs` | ✅ **保留**（与框架无关，纯 Rust） | §8.1 |
| `full_chain.rs` | ✅ **废弃重写**（断言方式改为 LVGL 内存 display 离屏） | §11.1 |
| **既有代码中有无"输入/触摸"资产可复用？** | ❌ **无**。既有屏是只读屏、无触摸栈 → `touch.rs` 为**净新增** | §1.2 |
| **既有代码中有无"中文断行/软断点"资产？** | ⚠️ **部分**：日志长消息的软断点后处理逻辑（若有）可复用；LVGL `LV_LABEL_LONG_WRAP` 已覆盖一般换行 | §1.1.2 |

> **代码规模净变化（估算，供 PM 参考）**：废弃约 **2400 行**（`local-display` 的 layout/font/run/canvas 原语 + OTF 产物引用 + full_chain 测试）；新增 HMI 侧约 **2500–4300 行**（`lvgl-sys` build+bindgen 约 150–300 行、`src/lvgl/**` 薄安全层 **1000–1800 行**、`ui/**` 6 页 + 组件 + 主题 **800–1500 行**、`timing.rs`/`touch.rs`/`screen.rs`/`console.rs` **500–700 行**）+ `lv_font_conv` 生成的 C 字体（工具产物，不计人力）；mupcd 侧净新增约 **2000–3000 行**（console_host 四服务 + 配置元数据表 + 采集任务 + 测试）。**净增约 2100–4900 行**——主要差异来自**自写 FFI 绑定层的 1000–1800 行**（§1.1.1.2）。绑定层实测外推 **~1300–2200 行**（§12.4 / R-19），据此净增区间上沿可达约 **5300 行**。

---

## 9. 边界与异常对照 PRD §5

| PRD ID | 场景 | 数据侧（mupcd） | HMI 侧展示 | 落地位置 |
|--------|------|------------------|------------|----------|
| EDGE-01 | PCS 离线 | 帧 `pcs_online=false`、`run_state=None`、三相 `Offline`；SOC 有 BMS 源则给 BMS 值 | F2「PCS 离线」；F3/F4 各相 `--`+「源离线」；F1 显示 BMS 值并标源 | 已有 |
| EDGE-02 | SOC 双源皆失 | `soc=None`/`Lost`（冻结值不上屏） | `--` + 「SOC 源失效」警示色 | 已有 |
| EDGE-03 | 通道断 | 回环端口消失 | ≤3 s 切「与主进程数据通道断开」；可保留最近帧暗化+冻结标；恢复 ≤1 s | `state.rs` |
| EDGE-04 | 点表未覆盖 | transport 不支持 → `NotRead` | `--` + 「未取数」 | 已有 |
| EDGE-05 | 数值域异常 | 量程/有限性校验 → `RangeError` | `--` + 「数据异常」 | 已有 |
| EDGE-06 | 1013 与功率方向相反 | `inconsistency=true` | 主状态仍 1013 + 「方向不一致」角标 | 已有 |
| EDGE-07 | HMI 进程异常 | 不阻塞 mupcd | 黑屏/占位；systemd `Restart=always` ≤3 s | unit 已有 |
| EDGE-08 | 日志为空/筛选无结果 | — | 「当前筛选条件下无日志」（`has_more=false` 且 `entries` 空） | `LogService` + P3 |
| EDGE-09 | 告警源不可用 | `alarms.available=false` | 「告警源不可用」，**不得**显「无告警」 | 任务 B |
| EDGE-10 | 配置写入失败 | 校验/落盘/生效分阶段错误；**回滚保证不半生效** | 明示原因 + **保留用户已输入值** + 装置保持原配置 | `ConfigService` + P2 |
| EDGE-11 | 保存被超时回归/切页打断 | — | `dirty` 时**不强制切页**，顶部提示「有未保存修改」 | P2 + 超时逻辑 |
| EDGE-12 | 联锁释放/M1 授权被拒 | `InterlockReject` 结构化拒绝原因 | 弹层就地显示具体原因（含剩余保持秒数/未复位源名） | `InterlockOps` + P4 |
| EDGE-13 | 触摸无效/设备异常 | — | 数据刷新与其他显示**不受影响**；上角标「触摸不可用」 | `touch.rs` 容错 |
| EDGE-14 | 触摸抖动/重复点击 | `request_id` 幂等表（30 s 窗口） | 500 ms 防抖（按钮 `disabled`） | UI + `ConsoleHost` |
| EDGE-15 | 检索范围超限 | `max_files` / `max_lines` 超限即拒 | 「检索范围超限，请缩小时间范围」 | `LogService` + P3 |
| EDGE-16 | 型号/序列号缺失 | 字段 `None` | 「未提供」 | `InfoSection` + P6 |
| EDGE-17 | 审计源不可用 | 结构化错误 + `available=false` | 「审计记录不可用」，**不得**显「无审计记录」 | `ConsoleAuditService` + P5 |

**补充边界（设计新增，PRD 未列但必须处理）**：

| ID | 场景 | 处理 |
|----|------|------|
| EDGE-18 | **写操作审计不可写** | fail-closed：拒绝执行 + `AuditUnavailable` + `tracing::error!` 落 journal（§3.3 裁决） |
| EDGE-19 | **提交时联锁态已变化** | `observed_*` 乐观并发检查 → `RejectedPrecondition`「联锁状态已变化，请刷新后重试」 |
| EDGE-20 | **控制通道可达但读通道断** | 允许（两通道独立）：配置/联锁仍可操作，实时数据沿用冻结帧并打标；顶栏同时显示两种状态 |
| EDGE-21 | **HMI 与 mupcd 的 `display` 配置不一致**（端口/节拍不符） | HMI 侧显式报「数据通道不可达」（不静默重试到天荒地老）；部署核对项写入 `local-display.md` |
| EDGE-22 | **配置保存后生效失败（落盘成功、模块拒绝）** | 恢复内存副本 + `.bak` 不删 → 返回 `ApplyFailed` + 明示「已回滚，装置维持原配置」；审计记失败 |
| EDGE-23 | **配置文件被整体重写（保留式编辑不可定位）** | 回退整体序列化回写：回执 `ConfigView.write_mode = full_rewrite` + 审计记同名字段 + UI **Toast 明示**「配置文件已整体重写，原有注释不再保留」。**禁止静默**（§4.3.2.1 / D16）；正常路径 `write_mode = text_preserve` 时 UI 不提示（避免噪音） |
| EDGE-24 | **把回环服务地址当成可远端访问的地址**（现场误判） | 结构性防错：读/控制通道地址 `editable=false`（P2 只读 + 说明行），P6 将「本机服务地址（仅回环）」与「设备管理 IP」**分两行**展示，任何页面不出现可推导出"远程可达"的呈现（§6.2 / §6.6） |

---

## 10. 非功能预算落实

> PRD §4.1 已**重新定档**为交互式 HMI（内存 ≤256 MB / 稳态 CPU ≤40 % 单核 / 瞬时 ≤60 % / 断连态 ≤10 % / 磁盘 ≤100 MB）。本设计的**目标值低于上限**，为 1024×768 软渲染留余量，并在真机实测后回写。

| 指标（PRD §4.1） | 上限 | 设计目标 | 落实手段 | 验证 |
|------------------|------|----------|----------|------|
| 常驻内存 | ≤256 MB | **≤96 MB** | LVGL 基座为 KB 级 + **PARTIAL 模式双绘制缓冲**（建议 2 × 1/10 屏 ≈ 2 × 314 KB = 628 KB，按 `lv_conf.h` 定稿）+ fb 由内核 mmap 持有；`LV_MEM_SIZE` **已定稿 1 MB（实测）**：外壳要求同时持有 6 页，而**256 KB 实测只够 P1+P6+P2 三页**（建第 4 页 P4 即 `lv_realloc` 失败）、**512 KB 亦不足**、**1 MB 通过** ⇒ 取 1 MB；该池只装对象树 / 样式 / 定时器，**绘制缓冲走系统堆、不与它竞争**；不变量已上锁（`ui/tests.rs::pages_chain` 的「6 页共存」用例）。**残余**：`lv_mem_monitor_t` 真机堆占用实测仍待（见 §14 R-24）；CJK 位图字体为**只读 C 常量**（不占堆）；日志 ring 定容（2000 条） | 真机 `ps`/`smaps` + 长稳 |
| 稳态 CPU（1 s 窗） | ≤40 % 单核 | **≤10 %** | LVGL 脏区失效 + `lv_timer_handler` 按需重绘；帧到达 1 Hz 主拍（慢拍变更时 ≤4 Hz 突发，合并窗口限幅）→ 每帧仅 SOC/数值区域标记脏；`poll` 阻塞无忙等（超时由 `lv_timer_handler` 返回值给出） | 真机 `pidstat` |
| 瞬时 CPU（页切换/列表刷新） | ≤60 % 单核 | ≤45 % | 页切换为一次性全帧重绘（≤300 ms 预算内）；列表滚动按可视行重绘（**窗口化列表**，§5.7 / R-22） | 真机 + 离屏计时 |
| 断连态 CPU | ≤10 % 单核 | **≤5 %** | 断连时每 500 ms 一次连接失败（毫秒级）+ `poll` 阻塞 | 真机（拔网/停 mupcd） |
| 磁盘（可执行 + 资源） | ≤100 MB | ≤40 MB | LVGL 静态库（Release，关无用模块）+ **10 档 CJK 位图字体合计 1.69 MB（RLE，实测，§1.1.2）** + 单个 HMI 二进制。**现值来源**：`liblvgl.a` release **3.27 MB**（再关 30 个控件宏 → **2.18 MB，-36.5%**）、**字体 10 档位图合计 1,776,080 B ≈ 1.69 MB（实测；`liblvgl_fonts.a` **落盘体积为估算**——含对齐开销会略高于位图之和，仅 32 px 单档实测 134 KB）**——**两项合计 ≈ 5.0 MB（按 3.27 MB 计；此为估算），复核结论：仍远低于 ≤40 MB，指标成立**。**本档指标由 ≤30 MB 上调**：LVGL C 代码静态链接进二进制；实测定稿 | 构建产物**位图字节数或 `.a` 落盘体积**（**不得用 `.c` 文本大小**，见 §1.1.2 口径陷阱） |
| 无忙等 / 空闲让出 CPU（§4.1.1 / NF-02） | — | 强制 | 唯一阻塞点为 `poll(min(lv_timer_handler 返回, 通道截止, ≤500 ms))`；禁止自旋；**禁止生产路径调用 `lv_refr_now()`**（§5.2 不变量 1/2） | 代码审查 + 真机空载 CPU |
| 内存不单调增长（§4.1.4 / NF-03） | — | 强制 | 日志列表：ring 定容 + **窗口化列表固定控件数**（不随数据增长）+ 分页；审计分页 20；告警固定 ≤10 | 长稳（≥24 h） |
| 触摸反馈 ≤100 ms（P95 ≤200 ms，TT-04） | — | 目标 | evdev（Rust）→ `lv_indev_read` → 命中/脏区 → `flush_cb` → fb 的时延预算见 §1.2；**须真机实测** | 真机计时探针 |
| 页切换 ≤300 ms（TT-05） | — | 目标 | 全帧重绘预算（离屏可先测，真机复核） | 离屏 + 真机 |
| 滚动 ≥30 fps（TT-06） | — | 目标 | **LVGL 滚动 + 窗口化列表**；**若真机不达标** → 降低可视行数 / 降滚动帧率（`LV_DEF_REFR_PERIOD`） / 关闭惯性动画 | **真机实测（§14 R-05 / R-22）** |
| **告警上屏 ≤2 s**（F7.3 / ST-16） | — | **≤1.35 s** | 慢拍 B 0.5 s + 变更即组帧 ≤0.25 s + HMI 轮询 0.5 s + 渲染 0.1 s（拆解与落地约束见 **§4.2.1**） | 假时钟单测 + 集成测试（注入告警断言 ≤1.5 s 到帧）+ 真机探针 |
| **联锁变化上屏 ≤2 s**（F16.5 / IL-01） | — | **≤1.35 s** | 同上（慢拍 C 0.5 s） | 同上 |
| **装置状态刷新 ≤5 s**（F6.3） | — | **≤3.85 s** | 慢拍 A 3 s + 组帧 ≤0.25 s + 轮询 0.5 s + 渲染 0.1 s（§4.2.1） | 同上 |
| 配置写入送达率 ≥99.99 %（§4.3） | — | 目标 | 落盘 `fsync` + `.bak` + 回滚；失败即明示 | 单测 + 真机 |
| 界面可用性 ≥99.9 %（NF-08） | — | — | 进程隔离 + systemd 自恢复 | 长稳 |

**预算口径说明（与 PRD 一致）**：本预算基于「选项式触摸 + 只读文本渲染」，**不含虚拟键盘 / IME 栈 / 可编辑文本框**（T-1 裁定）。设计上由 §3.4 的 `ConfigKind` 类型约束 + §5.6 的 **`lv_conf.h` 编译期禁用 `LV_USE_TEXTAREA=0` / `LV_USE_KEYBOARD=0` / `LV_USE_SPINBOX=0`** 共同保证该前提**在构建产物层面**不被破坏（强于源码扫描）。**注**：三项**必须同时为 0**——`lv_spinbox` 以 `lv_textarea` 为基类、其头文件带 `#error` 守卫，**启用 spinbox 就不可能关掉 textarea**（§5.6 F12 行）；步进器 / IPv4 / 日期时间一律 `lv_btn`+`lv_label` 组合，故"不含可编辑文本框"这一预算口径**成立**。

---

## 11. 测试策略

### 11.1 分层与可测性边界

| 层 | 用例 | 断言方式 | 可运行环境 |
|----|------|----------|------------|
| `display-proto` | 帧 v2 往返（含新段缺省）、控制信封/回执/错误码、`InterlockReject::user_message` 文案、旧帧兼容（v1 帧 → v2 类型）、未知字段容忍 | 纯单测 | 全平台（含 Windows） |
| `display_host` | 慢拍任务 A/B/C 的降级（源失败 → `available=false`/字段 `None`）、四段组装、缓存读不阻塞 | stub（`IntercoreTransport` 桩 + 假 storage/gateway 句柄） | 全平台 |
| `console_host` | **ConfigService**：字段元数据表与 `CoreConfig` 字段一一对应（防漏项/多项）、越界拒绝、原子落盘、`.bak` 保留、生效失败回滚、`revision` 递增；**LogService**：限额拒绝（EDGE-15）、cursor 增量、选项列表去重、ring 定容不增长；**ConsoleAuditService**：追加、查询筛选、分页、倒序、`newest_ts_ms`、不可用降级；**幂等**：同 `request_id` 重放返回首次结果、30 s 窗口外拒绝、`Busy`；**yaml 保留式编辑（§4.3.2.1）**：改 1 键后其余**逐行字节级不变**（注释 / 未建模键 / legacy `web_api:` 段原样保留）、多键批量替换行数精确、**不可定位时显式回退 `full_rewrite` 且注释确实丢失**、`CoreConfig` 序列化值等价往返、`key ↔ yaml_path ↔ 字段` 一致性 | 单测 + 临时目录 + 假时钟 | 全平台 |
| **HMI 离屏渲染** | 6 页渲染到**内存 display**（自定义 `lv_display`，`flush_cb` 写 `Vec<u8>`）：非空断言 + 关键区域语义色断言 + 降级态断言（`--`/角标/空态/「不可用」文案）+ **中文文案区域墨量密度断言（前景像素数为主判据、连通域数为辅；R-01 的固化回归，§1.1.3）**；并导出 PNG 供人工核对 | LVGL 内存 display + `lv_refr_now()`（**仅测试**）+ 区域采样/像素直方图 | 全平台（**R-20 三平台已全部实测走通**，见 §14 R-20 / `docs/technical-debt.md` §8.6.1）← **本模块最强的可测性支点** |
| **HMI 交互** | **直投 `lv_indev`**（构造 `lv_indev_data_t` 或经 `indev.rs` 的测试注入口）模拟按下/移动/抬起：导航 ≤2 次触摸到任一页、防抖（500 ms 内二次点击只生效一次）、超时回归 + 草稿保留、确认弹层默认焦点、滑动不误触发点击、**长按 1.0 s**（驱动 `lv_timer_handler` 并以**假 tick 回调**推进虚拟时间，不真等：未满 1.0 s 松手 → 不派发 `LV_EVENT_LONG_PRESSED` 且进度复位；满 1.0 s → 派发）、**只读字段（`editable=false`）不可被改动**、**滚动条不可交互**（在滚动条区域 x ∈ [右缘−8, 右缘] 注入"按下→移动→抬起" → 断言 `lv_obj_get_scroll_y()` 的变化量与**同等手势落在内容区完全一致**且**无 thumb 跳变**，§5.6） | LVGL indev 注入 + 假 tick + `UiState` 断言 | 全平台（同上；**R-20 三平台已全部走通**） |
| **双通道集成** | stub 服务端（读帧 + 控制回执）→ HMI 全链路：数据上屏、写操作下发与回执展示、失败 Toast、通道断/恢复 | tokio stub + 离屏渲染 | 全平台 |
| 静态约束（**架构检查自动化**） | ① `ui/**` 与 `src/**` **不得**引用 `lv_textarea` / `lv_keyboard` / **`lv_spinbox`** 符号（F12 零键盘；**编译期已由 `lv_conf.h` 三者置 0 保证**，本项防回退）；② `local-display` 依赖图**不得**含 `mupc-intercore`/`mupc-southd`/`mupc-gateway`（§4.4.6 禁直连）；③ 读通道 handler 集合**不得**含 POST 路径（PL-08）；④ `ui/**` **不得**出现裸色值/裸尺寸（`lv_color_hex`/`lv_color_make`/字面量尺寸），必须经 `theme.rs`（§5.6 控件策略行）；⑤ `lvgl-sys` **不得**被 `ui`/`state`/`channel`/`console` 直接 `use`（unsafe 边界收敛，§1.1.1.2 纪律 1）；⑥ `ui/**` **不得**调用 `lv_refr_now`（仅测试可用，§5.2 不变量 2）。**另**：`bindgen` 绑定面断言（`bindings.rs` 导出符号 ⊆ `allowlist.txt`，且清单符号均有实际引用；**实测口径**：对 `fn`/`static` 是硬断言，对 `type`/`const` 暂为存在性校验，**薄层 `src/lvgl/**` 落地后 `type` 升为硬断言**）——见 §12.1，同样进 CI | 源码扫描测试 / `cargo tree` 断言 | CI |
| **时延拆解回归**（F6.3 / F7.3 / F16.5） | 断言 §4.2.1 的落地约束全部成立：`alarm_poll_ms ≤1000`、`interlock_poll_ms ≤1000`、`device_poll_ms ≤4000`、`publish_ms ∈ [100, 4000]`（**上界 = 契约 `MAX_PUBLISH_MS` = 4000**，对应 §4.2.1 约束 6；成对边界用例 `publish_ms_upper_bound_4000_paired_boundary`：**4000 过 / 4001 拒**）、`min_publish_interval_ms ∈ [250, publish_ms]`（**下界 = 契约 `MIN_MERGE_WINDOW_MS` = 250**，对应 §4.2.1 约束 2）；并断言**慢拍写缓存后被唤醒组帧**（假时钟下「写入 → 发布」间隔 < 合并窗口上限），防退化回纯 1 Hz 主拍（该退化会使 F7.3/F16.5 变为 2.1 s，超差） | `display_host` 单测（假时钟）+ 配置校验单测 | 全平台 |
| **码表覆盖率**（新增，§1.1.2） | 静态扫描 `ui/**` + `state.rs` 中的中文字面量 → 断言**全部字符落在 `font_subset_charset.txt` 内**（防漏字出豆腐块） | 源码扫描单测 | 全平台 |
| 真机（标记需硬件） | `/dev/fb0` 映射与像素格式、evdev 触摸设备与校准、时延（TT-04/05/06）、资源实测（§10）、长稳 | 脚本 + 人工 | BECG-3568 |

### 11.2 本机 vs 真机边界（诚实）

| 可在开发机（Windows x86 / Linux CI）完成 | **必须真机** |
|-------------------------------------------|--------------|
| 全部契约/服务层单测；**6 页 LVGL 离屏渲染（含中文字形 R-01）**；交互注入；双通道集成；静态架构断言；`--backend offscreen` 端到端。**✅ 前提已全部具备**：Windows 离屏（MSVC 19.33 自动探测 + LLVM 23 的 `bindgen`；CLI 仅需 `export LIBCLANG_PATH=<LLVM>/bin`）、x86_64-Linux（本机 `cargo test -p local-display --lib` = 369 passed）与 aarch64 交叉（`lvgl-sys` + `local-display` 产物验为 ARM aarch64）**均已实测走通**（2026-09-19 Linux 构建机实测；证据链见 `docs/technical-debt.md` §8.6.1 L-1）（§12.1 / §14 R-20） | `FbCanvas` 真写 `/dev/fb0`（像素格式/字节序/分辨率）；evdev 触摸设备发现/协议/校准；HDMI 分辨率协商；时延与 CPU/内存实测；交叉编译产物运行；systemd 权限（video/input 组） |

### 11.3 特殊测试要求

- **`interlock.rs` 改造回归**：`InterlockReject` 的**每个变体**须有对应用例（触发源未复位/保持不足/latch 态/停机未确认/未启用），因为这是 EDGE-12「不得静默失败」的直接落点。
- **配置元数据一致性测试**：以反射式清单（手写 `const KEYS: &[(&str, ...)]`）与 `CoreConfig` 字段逐项比对，**防止「UI 能改一个不存在的键」这类静默失效**（这是原 `web-api::AppConfig` 的实际失败模式）。
- **审计不可用演练**：目录置只读 → 断言写操作被拒（fail-closed）且返回 `AuditUnavailable`（EDGE-18）。

### 11.4 冒烟与回归

- `cargo test --workspace --exclude mupc-iec61850-plugin --exclude rs485-plugin --exclude device-trait`（沿用项目既有冒烟口径；`web-api` 用例随 crate 删除）。
- HMI：`--backend offscreen --channel <stub> --control-channel <stub> --smoke` 一键自检（渲染 6 页 → 导出 PNG → 打印时序）。
- CI 中固化 §11.1 的**六条**静态约束（**零键盘——禁 `lv_textarea` / `lv_keyboard` / `lv_spinbox` 符号**；步进器一律 `lv_btn`+`lv_label` 组合 / 禁直连 / 读通道无 POST / 禁裸色值尺寸 / `lvgl-sys` 不被业务模块直接引用 / `ui/**` 禁 `lv_refr_now`），外加 §12.1 的 **bindgen allowlist 双向断言**。

---

## 12. 部署与交叉编译

### 12.1 构建（**Rust + LVGL C 源码**）

**LVGL C 源码的引入方式（选定）**：

| 方式 | 做法 | 结论 |
|------|------|------|
| **S-1 git submodule（选定）** | `mupc/vendor/lvgl/` 为 submodule，**pin 到具体 tag（如 `v9.5.0`）**；`lvgl-sys/build.rs` 用 **`cc` crate** 编译其源文件（`src/**` 递归）+ 把我们的 `lv_conf.h` 放到 `include` 路径 | ✅ **选定**：版本可审计（submodule commit 即锁定）、`cc` 原生支持交叉（`CC_aarch64_unknown_linux_gnu` 环境变量/`CARGO_TARGET_*_LINKER` 由 cargo 传递）、与 Cargo 构建一体化（`cargo build -p mupc-local-display` 一条命令搞定）**（已落地 submodule，pin `85aa60d18b3d5e5588d7b247abf90198f07c8a63` = `refs/tags/v9.5.0`；落地形态见下方 ⑧）** |
| S-2 vendor 源码直接入库 | 把 LVGL 源码复制进仓库 | ⚠️ 仓库膨胀、升级需人工 diff；**仅在 S-1 不可用（离线环境）时采用** |
| S-3 系统预装 liblvgl + `pkg-config` | 目标 rootfs 预装 LVGL 共享库 | ❌ 排除：目标镜像白名单与"随二进制自足"原则冲突（与对 linuxkms 的论证同款），且版本不受我们控制 |

**`lvgl-sys/build.rs` 的关键动作**（编码直接照做）：
1. `cc::Build::new()` + `.files(lvgl 源文件清单)` + `.include("vendor/lvgl")` + `.include("lvgl-sys/include")` + **`.define("LV_CONF_INCLUDE_SIMPLE", None)`** → 产出静态库并入链接。
2. **`bindgen` 在 `build.rs` 中运行（精确 allowlist，禁用通配）**：读取 `lvgl-sys/allowlist.txt`（逐行 `fn:<符号>` / `type:<符号>` / `var:<符号>`），逐项调用 `allowlist_function` / `allowlist_type` / `allowlist_var` + `allowlist_recursively(true)`（带上传递依赖类型），再以 `clang_arg` 传 `--target=aarch64-linux-gnu`（交叉时）与 LVGL include 路径 → 生成 `bindings.rs`。**禁止 `allowlist_function("lv_*")` 一类通配**——通配等于**全量生成**，与 §1.1.1.2 理由 2「只生成所需符号、压缩 `unsafe` 面」自相矛盾。**CI 双向断言**：① `bindings.rs` 导出的符号集合 ⊆ `allowlist.txt`（多一个即失败）；② `allowlist.txt` 中每个符号在 `src/lvgl/**` + `ui/**` 中确有引用（无死符号）。**新增控件用点时必须同步补清单**（与 §1.1.1.2 的 unsafe 边界纪律同等强制）。
3. **`println!("cargo:rerun-if-changed=...")`** 覆盖 LVGL 源与 `lv_conf.h`。
4. **可选加速（R-26 决策项）**：把 bindgen 生成物**提交入库**，`build.rs` 用 feature `prebuilt-bindings` 直接 `include!`，从而**免除终端用户的 libclang 前置**（代价是升级 LVGL 时须重生成）。**状态：未做**（见 §14 R-26）。

**⚠️ 两条必须写进 `build.rs` 的硬知识（实测，不做必踩）**：

| # | 现象（真实报错） | 处置 | 性质 |
|---|------------------|------|------|
| **①** | **跳过 `src/libs/**` → 链接期 `LNK2019: 无法解析的外部符号 lv_bin_decoder_init`** | `src/lv_init.c:298` **无条件**调用 `lv_bin_decoder_init();`（**没有 `LV_USE_*` 守卫**，源码实测），其定义在 `src/libs/bin_decoder/lv_bin_decoder.c`。**正确做法：跳过 `src/libs/**`，但白名单 `bin_decoder` + `rle` 两个子目录**（`rle` 是 `bin_decoder` 的依赖）。任何"只编核心"的想法**必在链接期爆掉** | **架构级必改** |
| **②** | **`c1: fatal error C1083: 无法打开源文件: "\\?\E:\..."`** | `Path::canonicalize()` 在 Windows 返回 `\\?\E:\...` verbatim 前缀，`cl.exe` **不认**（include 路径同受影响）。`build.rs` 必须实现 **`strip_verbatim()`** 剥离前缀（含 **`\\?\UNC\` 分支**）；**Linux 上 no-op** | **Windows 必改** |

**其余实测锚点与结论（供排期与复核）**：

- **源码规模与构建耗时（⑤）**：LVGL `src/**` 共 **463 个 `.c`**（其中 `src/libs/**` 56 个）→ **实际编译 409 个**；**冷构建 1m07s**（含 bindgen）、**增量 ~41s**、release **49–82s**；`bindings.rs` **852 行**；`liblvgl.a` release **3,433,108 B（3.27 MB）**；再关 30 个控件宏 → **2,180,294 B（2.18 MB，-36.5%）**。
- **`lv_conf.h` 宏关闭实测（⑥）**：设计关闭的**全部宏**（含红线 `LV_USE_TEXTAREA=0` / `LV_USE_KEYBOARD=0` / `LV_USE_SPINBOX=0`）**无一引发编译错误**——**红线成立**（`lv_label` 不依赖 `lv_textarea`；`lv_spinbox` 的 `#error` 守卫未触发，因其基类 `lv_textarea` 已一并关闭）。**追加实验**：再关 30 个 widget/模块宏亦**全部通过**（产物 -36.5%）。故 §1.1.1.2 的"编码前随 spike 复核"**已复核，结论如上**。
- **allowlist 双向断言（⑦）**：已落地为**可运行测试** `lvgl-sys/tests/allowlist_consistency.rs`（**尚未接 CI**）。**口径**：断言①（导出 ⊆ allowlist）对 **`fn`/`static` 是硬断言**；对 **`type`/`const` 只做存在性校验**（`allowlist_recursively(true)` 必然带出 14 个传递依赖 `type` 与 18 个 enum `const`，机械"⊆"不成立）——**薄层 `src/lvgl/**` 落地后把 `type` 升为硬断言**（详见 §1.1.1.2 / §11.1）。断言②（无死符号）当前**通过**。
- **submodule 落地形态（⑧）**：`.gitmodules` 已含 **`shallow = true` / `branch = v9.5.0`**；**CI 用 `git submodule update --init --depth 1`**。`lvgl-sys` 作为 **path 依赖**列入根 `mupc/Cargo.toml` 的 `members`（该列表**显式、非 glob**）——与 §5.1 目录树一致。`lvgl-sys` crate：名 `lvgl-sys` / lib 名 `lvgl_sys` / `links = "lvgl"` / `build = "build.rs"` / feature `default = []` + **`noto-font`**（用前须先跑 `fonts/gen_fonts.sh`，默认生成全部 10 档）。
- **交叉 bindgen 的 sysroot 风险（③，本机未实测 aarch64）**：`TARGET != HOST` 时 `build.rs` 追加 `--target=$TARGET`，clang 需目标 libc 头（sysroot），否则 `stdint.h`/`stdarg.h` 找不到。**两个处置（实施时择一验证）**：**①** `BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/aarch64-linux-gnu/include"`；**②** **64 位宿主 → 64 位目标**时基本类型尺寸一致，可**不传 `--target`**（直接用宿主 triple 生成），**彻底绕开 sysroot 依赖**。另注意 `-fvisibility=hidden`（linux target 下已加）。**建议**：实施第一步就跑 `cargo build --target aarch64-unknown-linux-gnu -p lvgl-sys`（一条命令暴露全部问题）。
- **`BINDGEN_EXTRA_CLANG_ARGS` 降级为兜底（④）**：Windows 实测**全程未用到**（clang 23 自动定位到 MSVC 的 UCRT/STL 头）→ 该条是**有备无患的兜底，非必经步骤**（下表 Windows 行已改述）。

**三平台构建前置（诚实清单）**：

> **✅ R-20 完成度 3/3（编码前门禁，已通过）**：Windows MSVC、**x86_64-Linux** 与 **aarch64 交叉** 三平台均已实测走通，实施第一步门禁 `cargo build --target aarch64-unknown-linux-gnu -p lvgl-sys` 已跑绿（2026-09-19 Linux 构建机实测；证据链见 `docs/technical-debt.md` §8.6.1 L-1）。**首跑确如预期变红（27 处）**，根因是「代码从未在 Linux 上编译过」（`lv_event_code_t` 的宿主/目标表示差异 + Linux-only 分支漏 import），**不是 CI 语义问题**——已逐条修掉，该 job 自此为硬门禁。

| 平台 | Rust 侧 | **C 侧（新增）** |
|------|---------|------------------|
| **aarch64 交叉（生产）** | `rustup target add aarch64-unknown-linux-gnu`；`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc` | `gcc-aarch64-linux-gnu`（**项目已在 `mupc/build.md` / `build-for-rk3588.sh` 中要求，本项无新增前置**）+ `bindgen` 所需的 **libclang**（**在宿主 x86_64 上运行，与目标架构无关**）+ `CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc`。**✅ 已实测走通**（2026-09-19 Linux 构建机实测；证据链见 `docs/technical-debt.md` §8.6.1 L-1）：`cargo build -p lvgl-sys --target aarch64-unknown-linux-gnu` 通过（1m28s）；`local-display --release --features noto-font` 产物经 `file` 验为 `ELF 64-bit LSB pie, ARM aarch64`（解释器 `/lib/ld-linux-aarch64.so.1`）。**实测口径**：`LIBCLANG_PATH` 直指宿主 libclang 库文件即可（**无需 `libclang-dev` 包**）；`BINDGEN_EXTRA_CLANG_ARGS=-isystem /usr/aarch64-linux-gnu/include` |
| **x86_64 Linux CI** | 常规 | `build-essential`（gcc）+ `libclang-dev`（装 `bindgen` 依赖）。**⚠️ 未实测**（与 aarch64 同批留 Linux 构建机） |
| **Windows 开发机（离屏测试）** | 常规 | **✅ 实测走通**：`cc` 自动探测到 **MSVC 19.33**（VS2022 BuildTools，**无需手工配置**）+ **装 LLVM（实测 23.1.1）并设 `LIBCLANG_PATH='<LLVM>\bin'`**（当前 shell 不刷新 `setx` 值，每次 cargo 前 export）——`bindgen` **开箱即用**。**`BINDGEN_EXTRA_CLANG_ARGS` 全程未用到**（clang 23 自动定位 MSVC 的 UCRT/STL 头）→ 该条为**有备无患的兜底、非必经**（若确实报 `stdarg.h` 找不到，再加 `-isystem <clang-resource-dir>/include`）。**LVGL 本体可编译**（其 Linux 专用驱动已被 `lv_conf.h` 全关，只编核心 + 内存 display） |

```bash
# ── 生产（aarch64 交叉；⚠️ 未实测 → 留 Linux 构建机，实施第一步门禁）──
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export LIBCLANG_PATH=/usr/lib/llvm-17/lib           # 宿主侧，供 bindgen
# 【门禁】先单独跑通 sys 层（一条命令暴露交叉 bindgen 的 sysroot 等全部问题），再跑 HMI：
cargo build --target aarch64-unknown-linux-gnu -p lvgl-sys
cargo build -p mupc-local-display --release --target aarch64-unknown-linux-gnu

# ── Windows 开发机（离屏；✅ 已实测走通）──
export LIBCLANG_PATH='C:\Program Files\LLVM\bin'
cargo build -p lvgl-sys -j 2                                  # 冷构建 1m07s（含 bindgen）
cargo run  -p lvgl-sys --example spike_offscreen -j 2         # S-2 离屏渲染 proof
cargo run  -p lvgl-sys --example spike_offscreen --features noto-font -j 2   # S-3；需先跑 gen_fonts.sh

# ── 本机离屏冒烟（x86_64 Linux / WSL）──
cargo test -p local-display --features offscreen
```

- **mupcd**：构建方式不变（移除 `web-api` 后依赖树更小）。⚠️ **订正（2026-09-19）**：本行原写「`axum` 不再需要」，与 §7.2 / §7.3 的两处「已过期」注记**自相矛盾**——`axum` **必须保留**（`console_host` 直接用 axum 装配控制通道，见 §7.3 的 core-bin 行逐字理由）。
- 脚本：`deploy/scripts/build-for-rk3588.sh` 增加 `--hmi` 子模式（设好上述 env 后产出 `mupc-local-display`），或新增 `build-hmi.sh`。
- **CI 影响（诚实）**：`cargo test --workspace` **现在会触发 LVGL C 编译**（数十秒到数分钟，取决于机器）→ 建议 CI 加缓存（`target/` 目录）与"`lvgl-sys` 未变更则跳过"的 `rerun-if-changed` 精确化。**实测**：冷构建 **1m07s**（409 个 `.c` + bindgen）、增量 ~41s、release 49–82s → `target/` 缓存收益明显；`rerun-if-changed` 已覆盖 `lv_conf.h` / `allowlist.txt` / `build.rs` / `vendor/lvgl/src` / `vendor/lvgl/lvgl.h` / `../fonts/`（spike 已实现）。CI 另需 `git submodule update --init --depth 1`（⑧）。

### 12.2 权限与 udev

- `mupc-display.service`：`SupplementaryGroups=dialout video input`（`input` 为新增，用于 `/dev/input/eventX`）。
- 稳定设备名：建议 udev 规则建符号链接（避免 `eventN` 漂移）：
  ```
  # /etc/udev/rules.d/99-mupc-touch.rules（示例，须按真机 vendor/product 填写）
  SUBSYSTEM=="input", ATTRS{name}=="<触摸屏名>", SYMLINK+="mupc-touch"
  ```
  生产 unit `--touch-device /dev/mupc-touch`。
- `mupcd.service`：若启用 `ProtectSystem=strict`，须加 `ReadWritePaths=/opt/mupc/config`（配置热写前提）。**注意**：mupcd 的 unit 我未在本次阅读中逐行核对，实施前须确认（§14 R-11）。
- HMI 只需**读** `/dev/fb0`（写像素）与 `/dev/input/eventX`；不需要其它写权限（`ReadWritePaths` 仅日志目录）。

### 12.3 字体与后端

- **字体**：`lv_font_conv` 生成的 C 数组（`fonts/lv_font_noto_sc_*.c`）随 `local-display` 一起编译链接 → **镜像零字库文件依赖**（与"编译期嵌入 OTF"效果等价，机制不同）。**档位固定为 10 档**（`gen_fonts.sh` 的档位数组 `(24 26 28 32 48 56 64 96 112 148)`，与 UI §3.3 阶梯逐档对应；**每档同用 §3.6 全用字表 326 字符**），10 档位图合计 **1.69 MB（实测）**（§1.1.2）。`fonts/gen_fonts.sh` 固化生成命令与码表路径，**确保可复现**（升级文案即重跑；全 10 档生成约 1–2 分钟，**此耗时为估算**）。**⚠️ 字库源（`.otf`）与生成物（`.c`）均不入库**（`.gitignore`，§1.1.2）→ **构建前必须先跑一次 `fonts/gen_fonts.sh`**（CI/构建脚本须编排此步）；**未生成时 `noto-font` feature 须给出清晰可读的编译期报错**（不得退化为 file not found，§1.1.2）。默认命令**启用 LVGL 内置 RLE 压缩**（`NOCOMPRESS=1` 可复议无压缩口径）。
- **后端**：默认 **P-1（自定义 `lv_display` + `flush_cb` → `FbCanvas` → `/dev/fb0`）**；`--backend drm` 保持"未实现即明确报错"（**不自研 DRM**）。若真机 `fb0` 不可用 → 见 §14 R-03 的处置（**改用 LVGL 官方 `LV_USE_LINUX_DRM`**，无需自研、无需第三方 crate）。
- **`--backend offscreen`**：`flush_cb` 的 sink 换成内存缓冲并可选导出 PNG（测试/CI 用，§11.1）。

### 12.4 工作量分布（供 PM 排期）

| 工作单元 | 规模 | 说明 |
|----------|:----:|------|
| **A. LVGL 构建接入 + 自写 FFI 绑定层**（`lvgl-sys` build/`lv_conf.h`/**`allowlist.txt` 精确清单**/bindgen + `src/lvgl/**` 薄安全层） | **XL**（**最大的 HMI 侧不确定项**） | §1.1.1.2：绑定层 **1000–1800 行**（设计估）／**~1300–2200 行**（实测外推，两者并列）+ build/flags/allowlist 清单维护 **150–300 行**（实测 build.rs ~190 + `lv_conf.h` 100 + allowlist 57 ≈ **347 行**，注释占比高）（**逐符号枚举，禁 `lv_*` 通配**；双向 CI 断言见 §12.1）。**R-19 判定：机械但确定，不是研究风险，可排期**。**实测锚点**：**21 个 C 函数 + 35 条 allowlist** 即跑通 `init/display/双缓冲/flush/obj/label/style/font/render` **全链**。**风险集中点（最不机械的两处）**：`event.rs` 的 **`user_data` 生命周期**、`indev.rs` 的 **`read_cb` 桥**。**含 R-20 编码前门禁（✅ 三平台已完成，2026-09-19，见 `docs/technical-debt.md` §8.6.1）** |
| **B. 6 页 UI 装配 + 组件库 + 主题**（`ui/**`） | **L** | 采用 LVGL 内置控件（比自绘控件**省**），但**长列表窗口化需自建**（R-22） |
| C. 显示/输入后端（`display.rs`/`indev.rs`/`touch.rs`/`screen.rs`/`timing.rs`） | **M** | `touch.rs` 为 Rust evdev；`flush_cb` 复用 `FbCanvas`；事件循环因 LVGL **更简单**（无需实现框架 Platform） |
| D. `lv_font_conv` 字体链（生成脚本 + **10 档** + 码表 + 覆盖率测试） | **S–M** | 工具链**已验证**（`lv_font_conv 1.5.2` 一次成功、单档 5–9 s）但**构建前置 + 不入库**带来"**构建前必跑生成脚本**"的编排与 CI 步骤，且 **`noto-font` feature 缺产物时的清晰报错处理**要写（§1.1.2）——故由 S **上调为 S–M**。**10 档口径**：全档位、每档全码表 326 字符，**生成耗时与 CI 步骤相应增多**（全 10 档约 1–2 分钟，**此耗时为估算**），但**仍属"工具驱动、确定性高"**，规模**维持 S–M**；体积实测合计 **1.69 MB**（§1.1.2） |
| E. `display-proto` v2 契约 + 控制信封 | M | 契约先行，两侧共用 |
| F. mupcd `display_host` 四段采集 | M | 复用 provider |
| G. **`ConfigService`（配置写 + 生效链路 + 保留式 yaml 编辑）** | **XL** | 本模块**最大**净新增项（§4.3）；**保留式编辑与回退路径的往返测试并入本项工时**（§4.3.2.1） |
| H. `LogService`（ring + 限额扫描） | M | 迁移 + 重构筛选 |
| I. `ConsoleAuditService`（schema + 双写 + 查询） | M | — |
| J. 联锁控制 + 错误结构化（含 `interlock.rs` 回归） | M | 中风险回归点 |
| K. **`web-api` 移除与出口迁移** | M | 机械但触面广（§7.3 清单） |
| L. 测试（离屏渲染/交互/双通道/服务层/静态断言/码表覆盖） | **L** | 与实现同步 |

---



## 13. 技术决策记录（ADR）

| 决策 | 结论 | 理由 |
|------|------|------|
| **D1 GUI 框架** | **LVGL v9（C，MIT）+ 自写 Rust FFI 绑定层**；**采用 LVGL 内置控件 + `ui/theme.rs` 自定义主题** | **选型动因**：Slint 在闭源商用嵌入式下**只剩付费商业许可**，成本不可接受 → 改用 **MIT 的 LVGL**（许可零成本，§1.1.4）。**技术理由**：① 6 页交互式 HMI 下"现成控件 + 自定义主题"才是 KISS（自绘路线 = L-5 = 重建框架）；② 内置控件**本就为嵌入式小屏设计**，`lv_style`/`lv_theme` 可逐项覆盖（"内置弹层不可控"的顾虑在 LVGL 下不成立，§5.1 注）；③ 中文由 `lv_font_conv` 标准方案解决，无框架级限制；④ fbdev/evdev 生态成熟且有官方驱动（我们仍走自控的 flush_cb/indev）。**代价（诚实）**：引入 C 编译链 + 自写绑定层（D17/R-19/R-20）。备选 egui（L-2，`MIT OR Apache-2.0`） |
| **D2 显示后端** | framebuffer `/dev/fb0`（默认）：**自定义 `lv_display` + `flush_cb`** → `FbCanvas`（P-1）；官方 `LV_USE_LINUX_FBDEV` 为一行开关备选；`LV_USE_LINUX_DRM` 仅在 fb0 不可用时启用 | 复用既有已落地的 fb 后端与像素格式处理（**像素格式风险归零**）；离屏与生产**共用同一条 flush 路径**（可测性）；不自研 DRM（LVGL 原生有） |
| **D3 触摸栈** | `/dev/input/eventX` + **Rust `evdev` crate** + **自定义 `lv_indev`（`read_cb`）** | 保留既有触摸设计（发现/多候选报错/校准/CLI 覆盖/缺失容错）；**不启用 `LV_USE_EVDEV`** ⇒ **不引入 libevdev C 依赖**（LVGL 官方 evdev 驱动强制依赖它） |
| **D4 进程拓扑** | **保持两进程**（mupcd = 数据+控制；HMI = 渲染+输入） | PRD §1.4/§4.4.6 硬约束（禁直连总线）；§4.3.2 进程隔离；复用既有已评审的 provider |
| **D5 通道形态** | **双监听**：9810 读（仅 GET）/ 9811 控制（受控接口），均强制回环 | PL-8「展示通道不承载下行写」需**结构性**可验证；故障与限流隔离；不并入已移除的 8080 |
| **D6 读侧数据面** | 保留**单一读端点**，`DisplayFrame` 扩 v2 段；慢拍数据走缓存，帧路径零 I/O | 客户端最简、失败面最少；避免帧率被慢源拖累 |
| **D7 控制面** | 统一信封（`request_id` + `issued_at_ms`）+ 幂等表 + 结构化错误码 + **审计 fail-closed** | T-3「无登录 → 审计 + 二次确认」的技术补偿；`request_id` 同时满足防重放与防重复生效（EDGE-14） |
| **D8 二次确认** | **UI 层**（LVGL 模态弹层 + `lv_group` 默认聚焦取消），并按 UI §2.5 **分级**：L1 双步 / **L2 双步 + 长按 1.0 s**（**LVGL `LV_EVENT_LONG_PRESSED` + `lv_indev_set_long_press_time(indev, 1000)`**：v9.5.0 无逐控件 API）/ L2+ 追加 `WarnBanner`；**恢复默认值、联锁释放、M1 授权、含连接类字段的保存均归 L2（或 L2+）**，由 `ConfirmDialog` 的 `level` **显式传入**（§5.6）；后端只做幂等与审计 | 确认是界面语义，后端重复实现会造出第二套状态机（KISS）；不加 `confirm_hash`（`request_id` 已足够）。**分级吸收自 UI 设计**。用 LVGL 内建长按事件而非手工 `animate` 计时，边界语义由框架保证 |
| **D9 零键盘** | 由 `ConfigKind` **类型约束** + **`lv_conf.h` 编译期置 `LV_USE_TEXTAREA=0` / `LV_USE_KEYBOARD=0` / `LV_USE_SPINBOX=0`** + 静态扫描（禁 `lv_textarea` / `lv_keyboard` / `lv_spinbox` 符号）三重保证；**步进器 / IPv4 / 日期时间 = `lv_btn` + `lv_label` 组合**（控件清单里**不含 spinbox**） | 把「不做文本输入」从纪律要求变为**类型/构建产物/测试**三层约束（编译期禁用比源码扫描**更强**），防未来回退。**弃 `lv_spinbox`**：其以 `lv_textarea` 为基类，`LV_USE_SPINBOX=1` + `LV_USE_TEXTAREA=0` 会 **C 编译期报错**（v9.5.0 `lv_spinbox.h` 的 `#error` 守卫），**弃用后"构建产物中根本不存在文本输入控件"这一结论才成立**（§5.6 F12 行） |
| **D10 配置生效** | `CoreConfig` 内存副本 + 原子落盘 + 逐项 `watch` 热生效；连接类参数**明示瞬断** | CF-04「自动生效无需重启」；对连接类参数的不可感知性诚实告知（§4.3.4） |
| **D11 告警源** | 本期用 `storage.events`（`AlertManager` 实证为死代码） | 唯一现有真源；`AlertFeed` 列为可选增强（§4.7），需评审裁决是否纳入本期 |
| **D12 审计双写** | 新 JSONL schema（结构化前后值）+ 既有哈希链（**SHA-256**，非 SM3）摘要 | 结构化字段为 F19 所需；既有链接续保证统一合规凭据。**口径**：`security/src/audit.rs` 自陈现网以 SHA-256 替代 SM3，设计不得称其为 SM3 |
| **D13 web-api 处置** | **整 crate 删除**，先迁出三处耦合类型 | 见 §7.2；保留 crate 的「内部模块」形态只会保留死依赖树 |
| **D14 crate 命名** | `display-proto` 名称保留（可选改名 `hmi-proto`，不在本期） | 改名跨 3 crate 的 churn 不带来功能收益 |
| **D15 LVGL 许可证** | **LVGL = MIT：可闭源商用、可静态链接、无 royalty、无源码公开义务；仅在产物中保留版权/许可声明文本（`THIRD-PARTY-NOTICES` 一行）** | §1.1.4。**零成本、零法务风险**。**字体**选 OFL-1.1 系（Noto Sans SC / 思源黑体）以规避文泉驿的 GPLv2+字体例外议题 |
| **D16 配置文件回写语义** | **保留式编辑（文本行级替换目标标量键）为主；仅当不可定位时显式回退整体序列化回写，并在回执/审计/UI 三处声明"注释与未建模键丢失"** | `CoreConfig` 仅 `Deserialize` + 整树回写会丢注释与现场 legacy `web_api:` 段，与 §7.3 兼容性主张冲突。本期 F9 可写字段**全为标量叶子**（无列表/无嵌套），行级替换语义完备无歧义；`Serialize` 仅为回退路径与往返单测（§4.3.2.1） |
| **D17 LVGL 与 Rust 的集成方式** | **自写绑定**：vendor LVGL v9.5 源码（pin tag）+ `cc` 编译 + `bindgen`（**精确 allowlist**：`lvgl-sys/allowlist.txt` 逐符号枚举，**禁 `lv_*` 通配**——通配即全量生成、与压缩 `unsafe` 面的目标矛盾；双向 CI 断言见 §12.1）+ 自写薄安全层（`src/lvgl/**`） | **不用 `lvgl-rs`**：其最新 0.6.2（2023-04）停留在 **LVGL 8.3.5** 且**官方定性为停滞**（LVGL issue #7298），与 v9 生态割裂；**不用第三方 v9 sys crate**（`lvgl_rust_sys`/`lightvgl-sys`）：只省接线、省不掉安全层，且把关键路径交给单一非官方维护者。自写使**版本可控、绑定面可枚举、升级路径明确**（§1.1.1）。**代价**：1000–1800 行机械工作量（§1.1.1.2 / R-19）。**实测**：形态与量级可信（外推 ~1300–2200 行，35 条 allowlist 即跑通全链、`bindings.rs` 852 行），**R-19 判定为"机械但确定、可排期"**（§12.4 工作单元 A） |

---

## 14. 待真机 / 待确认项

> 分类：**【开发机可先验】**（SPIKE）、**【真机】**、**【PM/评审裁决】**、**【厂方追认】**。诚实标注：以下项中 **R-03 / R-05 / R-07 / R-08 / R-12 / R-19 / R-20 / R-22** 对本设计的成立性或范围有实质影响。
>
> ⚠️ **实际执行顺序见 `docs/technical-debt.md` §8.6「Linux 环境交接清单」**（2026-09-19 新增；含逐条**可执行命令**与**通过判据**，并把本表的风险项按"在哪个环境做"重排）。**L-1（aarch64 交叉首次跑通）不过，勿动真机验收项。**


| ID | 项 | 类型 | 影响 | 处置 / 时间点 |
|----|----|------|------|---------------|
| **R-01** | **CJK 位图字体的清晰度**（本模块已改用 LVGL，`lv_font_conv` 是 LVGL 生态标准做法 ⇒ **"能否渲染 CJK"的可行性风险不存在**） | **真机/视觉标定** | **低** | **可行性已验证**——`lv_font_conv 1.5.2` **一次成功、5.3 s、字形肉眼可辨**（ASCII 预览可辨为「储能电池 SOC 92%」；文本区 271×35 px、前景像素 517→2,949）。**残余风险保留**：位图字体清晰度（密笔画字在 24 px 下的可辨性，UI V-4）——处置：字号上调 / 提高 bpp / 加 1 px 描边。**体积预算**：实测 32 px 位图 128.7 KB（未压缩）/71.1 KB（RLE）；10 档逐档实测合计 1.69 MB（RLE，实测非外推）（§1.1.2 / §10）。**离屏回归仍保留**（§11.1，**判据为"文本区墨量密度（前景像素数）"**），但不是编码前门禁 |
| **R-03** | 真机 `/dev/fb0` 是否映射 HDMI 输出、像素格式/位深/字节序；HDMI 是否 1024×768 原生 | **真机** | 高：决定显示后端 | 若 `fb0` 不可用：**改用 LVGL 官方 `LV_USE_LINUX_DRM`（P-3）**（LVGL 原生支持），且仍需核对所选 tag 的 DRM 路径修复状态（v9 早期有分辨率硬编码与 `lv_tick_set_cb` 缺失报告）。既有实现已留此判断（`--backend drm` 明确报错） |
| **R-04** | 触摸设备节点/协议：`/dev/input/eventX` 编号是否稳定、`ABS_X/Y` 的 min/max、是否 MT 协议 B、内核上报速率 | **真机** | 中：影响触摸可用性 | 真机首验 + udev 符号链接（§12.2） |
| **R-05** | **时延与资源实测**：触摸反馈 ≤100 ms、页切换 ≤300 ms、**滚动 ≥30 fps**、稳态 CPU/内存 | **真机** | 中–高：滚动帧率是**软渲染最可疑项**（LVGL 亦然） | 真机探测；不达标时的降级手段：降低可视行数（窗口化）、调 `LV_DEF_REFR_PERIOD`、关闭惯性动画、缩小绘制缓冲 |
| **R-06** | aarch64 交叉编译产物运行（**LVGL 静态库 + 位图字体 + `--backend offscreen`** 在目标机跑通） | **真机/CI 容器** | 中 | 构建流水线先跑；**与 R-20 同批** |
| **R-07** | **F7 告警源口径**：本期是否仅用 `storage.events`（已落库事件），还是必须新增 `AlertFeed` 覆盖「未落库即时告警」 | **PM/评审裁决** | 中：决定 F7 覆盖面与工作量 | 若要求覆盖，则 `AlertFeed` 进本期（§4.7），工作量 +M |
| **R-08** | **PRD F9 配置项与现网配置结构不一致**：「IEC 104 对端 IP 地址」在现网（服务端模型）无对应项；其余项与 `CoreConfig` 的映射须逐项确认；CF-04「自动生效」对连接类参数**必然伴随链路瞬断**，是否接受 | **PM 裁决** | 中：决定 F9 的字段集与验收口径 | 建议：以 `gateway.listen_addr` 替换「对端 IP」；确认「瞬断 ≤5 s」计入 CF-04 达成 |
| **R-09** | **DO1/DO2 与故障灯/运行灯的归属**：PRD F16 与现有代码注释相反 | **厂方/真机核对** | 低：UI 只显语义名已规避 | 真机点灯核对；UI 不显示 DO 编号 |
| **R-10** | **LVGL 交互语义三连**：① 滚动容器内「滑动不误触发点击」（TT-11）——阈值 `LV_INDEV_DEF_SCROLL_LIMIT`；② **L2 长按保持 1.0 s**（`LV_EVENT_LONG_PRESSED` + **`lv_indev_set_long_press_time(indev, 1000)`**：v9.5.0 无逐控件长按时长 API，阈值收敛于 indev，单屏单输入设备下语义等价）；③ **滚动条"纯指示、不可拖"**——LVGL v9.5 的滚动条是纯绘制部件（`lv_obj.c::draw_scrollbar()`，仅 `LV_EVENT_DRAW_POST`），输入侧无命中测试（`lv_indev_scroll.c::lv_indev_find_scroll_obj()` 只取 `indev->pointer.act_obj`），**不存在"默认可拖"**（§5.6 引源码）；本设计按 §5.6 的 A 方案落实并以交互用例钉死 | **SPIKE（离屏可先验）+ 真机** | 低：① 的阈值与时延待标定 | **编码前**用离屏事件注入验证 ①②（§11.1 交互层已列用例）；③ 仅需在实现中**不改写交互**并由 §11.1 用例断言（滚动手势仍走内容拖拽）；再上真机手感复核（V-2 / V-7） |
| **R-11** | `mupcd.service` 的沙箱配置是否允许写 `/opt/mupc/config`（配置热写前提）；Production 是否启用 `ProtectSystem=strict` | **真机/部署核对** | 中：决定 CF-04 能否成立 | 部署前核对 unit，必要时加 `ReadWritePaths` |
| **R-12** | **配置热生效的模块改造范围**：`intercore` / `gateway` 的 `watch` 接线改造量（重连、重绑定） | **设计深化 + 实现** | **高**：D 项工作量的主要不确定性 | 编码前对 intercore/gateway 各做一次改动面评估；工期紧张时启用 §4.3.5 降级方案（须 PM 同意） |
| **R-13** | `Iec104Server` 内部连接表能否安全聚合出 `link_state()`（并发/锁语义） | **实现细节** | 低 | 编码时确认；改动限于 `server.rs` |
| **R-14** | 1022–1032 实读（单段 vs 两段）、读回极性符号语义（正=放/负=充） | **真机 + 厂方追认** | 低（F3/F4 数值展示不阻塞） | 沿用既有口径：追认前方向一律取 F2 状态机 |
| **R-15** | 交叉编译时间戳可复现（`SOURCE_DATE_EPOCH` 是否有值） | 部署 | 低 | 构建脚本显式传入 |
| **R-16** | `ota_manager` 失去 web-api 消费者后的归属（保留实例不启动服务面 / 或随 OTA 需求重新接线） | PM/架构 | 低 | §7.3 备注 |
| **R-18** | **`CoreConfig` 可写字段新增列表 / 嵌套类型时的回写语义**：保留式行级编辑的前提是"全部可写字段为标量叶子"（§4.3.2.1） | 增量项（本期无） | 低（本期） | 新增此类字段时须扩展编辑算法或接受 `full_rewrite` 回退，并同步 §4.3.2.1 与 §11.1 用例 |
| **R-19** | **自写 LVGL FFI 绑定层的工作量能否接受**：薄安全层 1000–1800 行 + build/bindgen 150–300 行；**须覆盖本项目实际用到的 ~20 控件 + display/indev/style/font/event** | **设计深化 + 工作量裁决（编码前）** | **高**：决定 L-1 路线是否经济；也决定 HMI 侧工期 | **✅ 实测完成，工作量与形态可信**：**35 条 allowlist（21 fn / 12 type / 2 var）即跑通** `init/display/双缓冲/flush/obj/label/style/font/render` **全链**，`bindings.rs` **852 行**；外推 **~1300–2200 行**（设计估 1000–1800 略偏乐观，量级吻合；build/flags/allowlist 实测 ≈ 347 行）。**判定：机械但确定，不是研究风险，可排期**（§12.4 工作单元 A）。**风险集中点**：`event.rs` 的 `user_data` 生命周期、`indev.rs` 的 `read_cb` 桥。**结论：L-1 路线保持，不切 egui** |
| **R-20** | **LVGL C 编译链在三个环境的可用性**（**编码前门禁**）：① aarch64 交叉（`cc` + `CC_aarch64_unknown_linux_gnu` + 宿主 `libclang`）；② x86_64 Linux CI；③ **Windows 开发机**（`cc` 用 MSVC/MinGW + `bindgen` **必须 libclang**：装 LLVM 并设 `LIBCLANG_PATH`，必要时 `BINDGEN_EXTRA_CLANG_ARGS`） | **SPIKE（开发机 + 交叉环境）** | **高**：决定"本机能否离屏测试"与"交叉能否产出" | **完成度 3/3 —— ✅ 三平台均已实测走通**（2026-09-19 Linux 构建机实测；证据链见 `docs/technical-debt.md` §8.6.1 L-1）：**① Windows MSVC**（MSVC 19.33 自动探测 + LLVM 23 的 `bindgen` 开箱即用，`BINDGEN_EXTRA_CLANG_ARGS` **未用到**；踩坑见 §12.1 两条硬知识）；**② x86_64-Linux**（宿主 `cargo test -p local-display --lib` = 369 passed / 0 failed）；**③ aarch64 交叉**（`lvgl-sys` 通过、`local-display` 产物验为 ARM aarch64）。**实施第一步门禁**已跑绿并被 CI 的 `hmi` job 固化为硬门禁；**首跑变红 27 处**的真实根因是「Linux 分支从未过编译器」（见 §14 说明块与 `docs/technical-debt.md` §8.6.1 L-1）。**可选加速**：提交 bindgen 生成物（feature `prebuilt-bindings`）以消除终端用户的 libclang 前置（未做，见 R-26） |
| **R-21** | **LVGL 版本 pin 与升级策略**：pin 的 tag（拟 `v9.5.0`）在所选 API 上的**行为与文档一致性**（如 P-1 的 `flush_cb` 契约、`lv_timer_handler` 返回语义、`PARTIAL` 双缓冲尺寸约束）；**另含 §5.6 的滚动条不可交互结论**（基于 `v9.5.0` 的 `src/core/lv_obj.c` 与 `src/indev/lv_indev_scroll.c` 源码）；未来升级（v9.x→v9.y）的改动面 | **实现期 + 长期维护** | 中：升级成本与 API 稳定性 | pin tag 后**先做 P-1 最小闭环**验证契约；升级时以"重跑 bindgen + 修 `src/lvgl/**` 编译错"为固定流程（薄层是唯一跟改面，D17 的收益即在此）；**换 tag 须重核 §5.6 引用的两个源码文件是否仍无滚动条输入逻辑**（若上游新增滚动条拖拽，须显式关闭或改走 §5.6 的 B 方案） |
| **R-22** | **长列表窗口化（LVGL 无内建虚拟滚动）**：日志/审计长列表的"可视行 ×1.5 复用"实现方式与滚动 ≥30 fps 的关系（§5.7） | **实现 + 真机** | 中：直接影响 TT-06 与 §10 内存/CPU 预算 | 编码期与 §5.7 的列表实现同步；真机复核滚动帧率（与 R-05 合并）；不达标 → 降低可视行数 / 降帧率 |
| **R-23** | **aarch64 交叉编译未实测** | **SPIKE（Linux 构建机）** | **中–高**：与 R-20 ②③ 同一事实 | 见 R-20。**✅ 已实测走通**（2026-09-19 Linux 构建机实测；证据链见 `docs/technical-debt.md` §8.6.1 L-1）：`cargo build -p lvgl-sys --target aarch64-unknown-linux-gnu` + `local-display --release --features noto-font` 全链通过，产物验为 ARM aarch64 |
| **R-24** | **`LV_MEM_SIZE`：已定稿 1 MB**；**`lv_mem_monitor_t` 真机堆占用实测仍待** | **真机** | **低**：口径为「实测区间 + 上锁不变量」 | 实测：**256 KB 只够 P1+P6+P2 三页**（建第 4 页 P4 即 `lv_realloc` 失败）→ **512 KB 亦不足** → **1 MB 通过** ⇒ 取 1 MB（1 MB 是**实测通过值**、非最小理论值）。不变量由 `ui/tests.rs::pages_chain` 的「6 页共存」用例锁住（**再调小或某页膨胀即红**）。**残余**：本机 `lvgl-sys/allowlist.txt` **无 `lv_mem_monitor`** ⇒ 未读到 `free / max_used / frag`，故 **1 MB 的余量倍数未量化**；真机可用 `ps`/`smaps` 复核 BSS 占用 |
| **R-25** | **render 性能只有 debug 数字** | **实施期 + 真机** | **中**：影响 TT-05「页切换 ≤300 ms」 | 全屏强制重绘 **41.7 ms（debug 构建）**；设计 §10 的"页切换 ≤300 ms"须在 **release + 真机** 核实（与 R-05 合并） |
| **R-26** | **`prebuilt-bindings`（预生成 bindings 入库，免终端用户 libclang）未做** | 实现期（可选加速） | **低–中** | §12.1 动作 4；当前冷构建实测 **1m07s**（409 个 `.c` + bindgen），CI 建议加 `target/` 缓存 |
| **R-27** | **CJK 字形验证的逐字模板比对未做** | **实现期 + 视觉标定** | **低** | 判据已定为**"文本区墨量密度（前景像素数）"**（§1.1.3 / §11.1）；**逐字模板比对仍未做**，列为正式测试的可选强化项 |

---

## 15. 外设数值上屏（U-73 增量：F20–F26 / T-8 落点）

> **本节为 2026-09-23 追加的增量设计**，对应 PRD v2.2 的 **§3.9（F20–F26，`[REVIEWED: PASS: 2026-09-23]`）**、**§3.9.0（数据可用性要求）**、**§4.5 刷新频率行**、**§5 EDGE-18~24**、**§8 T-8~T-12**、**附录 B EX-01~EX-34**。
>
> **门禁声明**：本节产物**不受**文首 `[DESIGN_APPROVED: 2026-09-11]` 覆盖（该标记只对 §1–§14 有效）；本节待评审获批后方可编码。
>
> **修订声明（v2.1-r2，2026-09-23）**：本节按**设计评审意见（REJECTED，5 严重 / 6 中等 / 5 处事实与交叉引用错误）**逐条返工：① 容量守卫**按实测编码长度统一口径**重算（§15.2.4，`MAX_PERIPH_BYTES` 40 KiB → **56 KiB**，守卫式与结论自洽）；② 撤销对 01 设计不存在的 `snapshot_roles` / `round_seq` / 站级 `online` 的**假设性依赖**，改为「**已存在的接口** + **显式登记的新增接口要求**」（§15.1.1 / §15.1.2 / §15.9 **R-38 / R-39**）；③ 变更通知**由 `Notify/watch` 订正为 `broadcast`（容量 64，`Lagged` 丢帧）**，≤2 s 上屏改为「**丢失由兜底 tick 收敛**」（§15.6.1）；④ 登记 PRD **自身冲突**（F24 含 1049 vs §7 #11 排除 1046–1065）与 **EX-05/14/18/19/20/22「常显」 vs 分段控件同屏仅 1 段**（§15.9 **R-40 / R-42**）；⑤ 补 **bit15 通信状态的厂方追认**登记（§15.9 **R-41**）；⑥ **R-36 版面**补齐（P4 / P6 可实施版面规格落 **UI 设计文档 §6.4.1 / §6.6.1 / §5.1 #21**，本节只引其常量）。**改动仅限本文件与 12-UI 设计文档**；PRD 冲突**只登记不改**；01/03 号设计不动。
>
> **修订声明（v2.1-r3，2026-09-23）**：本节按**设计评审意见（REJECTED：5 严重 / 6 中等）**做**末轮**返工——**只改文字与数字，不重构**（守卫链 / 接口真实性 / 555 点 / PRD 引用经评审独立复算通过，一律不动）。逐条：① **LVGL 依据订正**（**结论"用 `SegmentedTabs`"不变**）：`lv_buttonmatrix` **有**段间距 API（`pad_column`，`lv_buttonmatrix.c:1024-1029`）；真正不达标的是**命中区按 `pcol/2+1` 外扩**（`:892-935`，上限 `LV_DPI_DEF/10 = 13`）⇒ `pad_column = 16` 时净距 **−2 px**（§15.5.1 + UI §2.1 补注 / §5.1 #21 / §5.3 / §6.6.1；**并裁定 R-44**：与 `SegmentedControl` **不合并实现、只合并视觉规格真源**）；② **容量表 n=20 行按自述公式重算**（20.1 / 21.7 → **20.6 / 22.2 KiB**，全部数字给出字面算式，§15.2.4）；③ **"元数据入帧 ⇒ 爆帧"的三套并存数字收敛为同一算式**（取 **118 B/条下界** ⇒ 元数据 64.0 KiB、整帧 **102.7 KiB**；§15.2.2 的「69.7」与 §15.3.1 的「67.9」同步订正）；④ **R-42 依据订正**：排除侧依据 = **PRD §7 #11（`:1042`）**，**不是** EX-23 / F24 验收 2（`:1279` / `:773` 是「不得**被标注为**…」的**误用禁止**）⇒ **选项 B 无需改 EX-23**；⑤ **§15.7.3 按字面量重列**分组标题（含 `（20）`/`（288）`/`（10）`/`（累计量）`）⇒ 去重后 **31**（不是 30），**H-2 补全角括号与数字**；⑥ 中等项：P6 全量 **555 → 428 + 装置段**（fire 在 P4、且 P6 与 n 无关）、R-43 **66.6 → 64.96 KiB**（KB/KiB 混用）、`truncated` 示例 **101 → 111**、**F25.4 补时延落点 + 依赖项 R-45（02 号"快采"，未落地，含降级口径）**、文首 `:11` 的 `REG_I_A`/`REG_P_A` 归属订正（→ `transport/modbus.rs:170-171`）、**全节 `file:line` 回源复校**。**改动仅限本文件 §15 与文首该行 + 12-UI 设计文档**；PRD / 01 / 03 / 代码 / 既有门禁标记一律未动；**未新增任何"假定已存在"的接口**。
>
> **范围外（明确不做，且与 PRD 一致）**：
> 1. **不改** F1–F19 既有字段的语义 / 量纲 / 缺省 / `Field.flag` 四态（F26.7）；
> 2. **不新增页面**（仍 6 页；F11 / TT-01 口径不变，见 §15.5.3）；
> 3. **不做**台区关口总表（`meter_grid`）上屏（PRD §7 #10）；
> 4. **不做** PCS 点表 1046–1065（STS / 负载区）上屏（PRD §7 #11 / F24 明确排除）；
> 5. **不另立**新鲜度门限（唯一真源 = 采集侧 `stale_timeout_s = 5 s`；F25.4）；
> 6. **不引入任何写操作**——P4/P6 的外设区**全部只读**，不触发 PL-1 审计与二次确认（PRD §3.9.1 第 2 条）。

### 15.0 前置事实核对（本节的写作依据，逐条可核）

| # | 事实（代码 / 配置实证） | 证据（file:line） | 对设计的影响 |
|---|------------------------|------------------|--------------|
| N-1 | `PROTO_VERSION = 2`；`MAX_FRAME_BYTES = 64 KiB`；`Field { v: Option<f64>, flag: FieldFlag }`，四态 = `Valid / NotRead / Offline / RangeError` | `mupc/crates/display-proto/src/frame.rs:21` / `:30` / `:46` / `:59` | 增量按 **v3** 处理（§15.2.1）；四态语义不动（§15.2.3） |
| N-2 | 帧现有 5 段（F1–F5 顶层 + `device` / `alarms` / `info` / `interlock`），**无外设段** | `frame.rs:380-419`（`DisplayFrame` 起、`interlock` 止） | 新增 `peripherals` 段（§15.2.2） |
| N-3 | 组帧路径**只读内存缓存**；慢拍任务写缓存 + `Notify` 唤醒组帧（`PublishPacer`，合并窗口 `min_publish_interval_ms`） | `mupc/crates/mupc-core-bin/src/display_host.rs:86-91`（`SlowCaches`）、`:666`（`DisplayDataProvider`）、`:799-821`（`slow_tick`，fn 起于 `:799`）、`:823-842`（`run_publish_loop`）、`:951-1020`（`build_frame`） | 外设走**同一机制**（慢拍 D），不引入新范式（§15.1.1）。**注：这里的 `Notify` 是 display_host 进程内的唤醒**（`display_host.rs:57` / `:699`），**与 N-18 的 `latest_values` 广播不是一回事**，两者不得混用 |
| N-4 | 南向真值的运行时出口 = `StationSink` 两回调：`on_station_telemetry(id, role, Vec<(metric, value, is_event)>)` / `on_station_offline(id, role, reason)` | `mupc/crates/mupc-southd/src/scheduler.rs:44-76`；`mupc/crates/mupc-core-bin/src/startup.rs:535`（`on_station_telemetry`）、`:583`（`on_station_offline`） | `latest_values` 的写入方 = core-bin `SouthSink`（唯一，除 §15.1.3 的 PCS 缺口） |
| N-5 | 点名 = `<块名>_<块内偏移+1>`；显式 `name` 覆盖（`soc` / `fire_det_count`） | `mupc/crates/mupc-southd/src/points.rs:128-131`（`fn positional`） | 帧内键直接用该点名（§15.2.2） |
| N-6 | **位点也落 telemetry / 逐位产点**（`func: discrete`） | `mupc/crates/mupc-southd/src/mapper.rs:975`（用例注释） | 位点与标量在帧内**同构**（都走 `PointValue`），**无需位图类型** |
| N-7 | 点表**登记** 618 点（n=20 展开）：`hvac` 34 / `fire` 127 / `battery` 345 / `meter_batt` 40 / `pcs` 72。**登记 ≠ 上屏**：本增量上屏的是**白名单**（§15.5.2 逐条枚举） | `mupc/crates/mupc-southd/src/point_table.rs:7`（模块头「618 点」）、`:827-863`（hvac 34）、`:802-825`（fire）、`:323-675`（battery 345）、`:753-800`（meter_batt 40）、`:677-751`（pcs 72） | **容量核算一律用白名单数**（§15.2.4 表「白名单」列），登记数只作分母对照 |
| N-8 | `hvac_in` 只产 3 点（`hvac_in_1` / `_3` / `_4`，显式 `points` 列表）；`fire_det` 114 寄存器**逐寄存器 1 点** | `mupc/deploy/config/mupc_core_config.production.yaml:401`（`hvac_in` 块起）、`:385`（`fire_det` 块起） | 点名与点数**不做推算，按配置实证**（§15.5.2 字段表） |
| N-9 | 生成字体 cmap 实测 **324 码位**；`font_subset_charset.txt` 全用字表 326 字符 | `mupc/crates/local-display/fonts/lv_font_cmap.txt`（头行「本清单码位数：324」）；`fonts/font_subset_charset.txt` | 字体门禁（§15.7） |
| N-10 | 既有「码表覆盖率」测试**只扫 `ui/**` + `state.rs` 的源码中文字面量** | `mupc/crates/local-display/src/ui/tests.rs:1066-1200`、`:1708-1727` | 若中文名以**运行时字符串**（帧 / catalog）到达 HMI ⇒ **既有测试覆盖不到**；本设计以「短标签表入 `display-proto` + 白名单」**结构性消除**该盲区（§15.7 F-5 / F-6 / **H-2**） |
| N-11 | HMI 通道层已有版本拒绝：`Error::ProtoVersion(url, got, expected)` | `mupc/crates/local-display/src/error.rs:59`（变体声明）；`mupc/crates/local-display/src/channel.rs:455-459`（产生点） | 只需**归因与文案**（§15.6 情形③） |
| N-12 | `ChannelStatus` **仅 3 态**：`Init / Connected / Down`，**无「版本不匹配」态**；**`Stale` 不属 `ChannelStatus`，而在独立枚举 `Freshness { Fresh, Stale }` 内**；`ScreenMode` 仅 `ChannelDown / Init / Live` | `mupc/crates/local-display/src/state.rs:45-52`（`ChannelStatus`）、`:56-59`（`Freshness`）、`:63-70`（`ScreenMode`）、`:294-299`（`freshness()`） | 需新增归因分支（§15.6 情形③）。**订正记录**：本节 v2.1-r1 曾写「`ChannelStatus` 4 态含 `Stale`」——**错误**，`state.rs:43` 的注释原文即「设计 §5.3 四种态里与主进程连通性相关的**三种**；新鲜度另由 `Freshness` 表达」 |
| N-13 | `display` 配置段已有 `device/alarm/interlock_poll_ms`、`alarm_page_size`、`min_publish_interval_ms`；`validate()` fail-fast | `mupc/deploy/config/mupc_core_config.production.yaml:46-73`；契约侧 `mupc/crates/display-proto/src/config.rs:218-226`（越界即 `Err`）、`:60`（`MIN_MERGE_WINDOW_MS = 250`） | 新增 `periph_poll_ms` / `periph_page_size`（§15.1.1 / §15.3.2） |
| N-14 | 消防「钢瓶气压未配置」接缝已存在但 **core-bin 零调用** | `mupc/crates/mupc-southd/src/scheduler.rs:752-760`（`cylinder_pressure_configured`） | 需显式接线（§15.2.2 站级标志 + §15.11） |
| N-15 | `south_stations.pcs` **整段注释**；`intercore` 当前**仅**读 1010 / 1013 / 1022–1024 / 1029–1032 | `production.yaml:236-284`（注释段）；`mupc/crates/intercore/src/transport/modbus.rs:170-172`（**文件私有**常量 `REG_I_A=1022`（`:170`）/ `REG_P_A=1029`（`:171`）/ `REG_P_TOTAL=1032`（`:172`），同属一路 FC04 3 区读）；`mupc/crates/intercore/src/pcs.rs:20` / `:22`（`REG_SOC=1010` / `REG_RUN_STATE=1013`） | F24 真源 = **intercore**（非 southd 站）⇒ **写入方缺口**（§15.1.3 报告项 R-28 / R-29） |
| N-16 | `slow_tick` 的内容比较函数**显式忽略 `ts_ms`**（如 `device_changed`） | `display_host.rs:105-131`（`device_changed` / `alarms_changed` / `interlock_changed`） | 外设段的比较函数须同样忽略 `ts_ms` / `last_ok_ms`（否则每次采样都"变更"⇒ 发布率打满 4 Hz） |
| N-17 | 帧段的时间戳：`DisplayFrame.ts_ms` = **组帧时刻**；`DEFAULT_STALE_MS = 2000` 供渲染端判「通道卡住」 | `frame.rs:24`；`mupc/crates/local-display/src/state.rs:292-297` | 帧级判据**不得**外推到外设值新鲜度（§15.2.3 口径澄清） |
| **N-18** | **`latest_values` 的变更通知是 `tokio::sync::broadcast`**（容量 **64**），落后 ⇒ `RecvError::Lagged` ⇒ **消费方必须全量重读**；**不是** `Notify` / `watch`，**无"必达"语义** | 01 设计 §9.1.2（`change_tx: tokio::sync::broadcast::Sender<ChangeBatch>`）/ §9.1.3（`subscribe` 批注「广播容量 64；落后丢帧 ⇒ 消费方必须全量重读」）/ §9.1.5（`Lagged ⇒ 全量重读` 的消费方标准形态） | **订正 v2.1-r1 的错误**（该版写 `Notify / watch` 并把 ≤2 s 压在"通知必达"上）：外设段唤醒**允许丢**，丢失由**兜底 tick 收敛**（§15.1.1 / §15.6.1） |
| **N-19** | `latest_values` 的**已存在**读取面：`get(&PointId)` / `station_snapshot(&str) -> Vec<PointView>`（**单次持读锁克隆该站全部已登记点**，含不可得态）/ `all()` / `station_is_active(&str, now_ms) -> bool`（站级活性，**`stale_timeout_s` 的唯一持有者**）。**不存在** `snapshot_roles` / `round_seq` / 站级 `online` 字段 / 站级 `last_ok_ms` 字段 / `station_last_poll_ms` getter | 01 设计 §9.1.2（`map` / `station_poll_ms` 均为**私有字段**）/ §9.1.3（方法签名全表） | ⇒ 本节**不得**写 `snapshot_roles(...)`（**改为逐站 `station_snapshot`**）；**站级在线**用已存在的 `station_is_active`；**「最后成功时刻」与点级「本轮未更新」需新增接口** ⇒ 登记 **R-38**（§15.1.2 / §15.9） |
| **N-20** | `latest_values::PointValue` 的字段是 `value: Option<f64>` / `ts_ms: u64` / `quality: PointQuality{Ok,Stale,Invalid,Unconfigured}`；`apply` **无条件 upsert `ts_ms` / `quality`**（值不变也刷新时标、**不广播**）⇒ **标量点的 `ts_ms` = 本轮轮询成功时刻**；位点的 `ts_ms` = **最后变化时刻**（判据走站级活性） | 01 设计 §9.1.2（`PointValue`）/ §9.1.3（`apply` 批注）/ §9.1.5（「值不变时的时标」行）/ §9.1.4 的时标语义注 | 点级 `NotRead` 判据改由 **`point.ts_ms` vs 站级「最后成功时刻」** 表达（**不引入任何新时间阈值**，§15.1.2）；**位点不适用**该判据 |
| **N-21** | 编码实测（本节脚本，`serde_json` 紧凑形态）：单点典型 `{"at":201,"v":1.0,"flag":"valid"}` = **33 B**；白名单可达最坏 `{"at":594,"v":-214748364.7,"flag":"range_error"}` = **48 B**；f64 理论极值形态 = **60 B** | 本节 §15.2.4 的实测脚本（输入 = `PeripheralsSection` 的 serde 形态）；`FieldFlag` 的 JSON 名由 `frame.rs:46` 的 `rename_all = "snake_case"` 决定（`"valid"` / `"not_read"` / `"offline"` / `"range_error"`） | **订正 v2.1-r1 的 28 B / 30 B / 48 B 三套口径混用**：容量陈述用 **33 B**，守卫预检用 **48 B**，兜底说明用 **60 B**（§15.2.4） |

---

### 15.1 数据面（一）：`latest_values` 的消费方式

#### 15.1.1 归属（已由他路设计裁定，本节只消费）与消费形态（本节选定）

**归属（既定，本节不代改）**：共享前置「外设最新值入口」落在 **`mupc-data-processing::latest_values`**；写入方 = core-bin `SouthSink`；读取方 = 01 号上送器 + **`display_host`（本节）** + 策略引擎；**`mupc-southd::scheduler::StationSink` trait 不变**。

**消费形态（选定）：慢拍 D + 「变更通知 ∪ 兜底 tick」，与既有慢拍 A/B/C 逐条同构。**

```
DisplayDataProvider（§4.2 既有结构 + 新增一路 D）
  ├ 慢拍 D：外设段
  │     触发 = latest_values 变更广播（broadcast，容量 64；**落后可丢**，N-18）
  │            ∪  兜底 tick（periph_poll_ms = 500）
  │            ★ 触发源只影响"多久跑一次"，**不影响正确性**：任一次采样都是
  │              「读全量 → 重建整段」的无状态操作 ⇒ 丢一次广播 = 晚 ≤periph_poll_ms 收敛
  │     读法 = 逐站 latest_values.station_snapshot(station)（5 站 5 调，N-19）  ← 纯内存读
  │            （禁用任何 DB / telemetry 表读取——RQ-9.0-1）
  │     产出 = PeripheralsSection → slow_tick() 写 peripherals_cache → 内容真变化才 notify
  └ 组帧（build_frame，1 Hz 主拍 ∪ 变更唤醒）：
        只做 `read_cache(&peripherals_cache)` 的一次克隆——**帧路径零阻塞 I/O、零 DB 查询不变**（§2.1 / D6）
```

> **`Notify` 与 `broadcast` 的分工（必须分清，否则会写出错误的可靠性论证）**：
> ① **process 内唤醒**（display_host 的慢拍 A/B/C 与本节的 D 共用）：`tokio::sync::Notify`（`display_host.rs:57` / `:699`）——**拍内必达**，但只解决"进程内何时跑"，**不承载跨模块事件**；
> ② **`latest_values` → 本进程的事件源**：`broadcast<ChangeBatch>`（容量 64）——**会丢**（`Lagged`）。
> ⇒ 本节**不**把任何时延指标压在 ② 的"必达"上（v2.1-r1 的错误即在此）；② 只用来**降低时延**，收敛由兜底 tick 保证（§15.6.1 约束 1）。

**三类被否方案与理由（留痕）**：

| 方案 | 做法 | 否决理由 |
|------|------|----------|
| **A（否决）** | `build_frame` 每帧直读 `latest_values` | 把点数线性成本与锁争用带进 1 Hz 主拍路径；破坏「帧路径只读内存」不变量（N-3） |
| **B（否决）** | 纯订阅回调：每次变更通知即重建段 + 立即组帧 | BMS 288 位 / 探测器 114 点是**整块刷新**⇒ 通知密集突发 ⇒ 发布率被点数放大；且丢失"内容未变不发帧"的抑制 |
| **C（否决）** | 屏侧/组帧侧轮询 `telemetry` 表取最后一条 | **PRD RQ-9.0-1 明令禁止** |
| **D（选定）** | 慢拍 D + 变更通知 ∪ 兜底 tick + 内容比较 + 合并窗口 | 与既有 A/B/C 同构（KISS）；帧路径零 I/O；发布率上界仍为 `max(1 Hz, 4 Hz)`（§4.2 约束 2 不破） |

**节拍取值**：新增配置键 `display.periph_poll_ms`，**默认 500**，`validate()` 硬校验 `∈ [1, 1000]`（与 `alarm_poll_ms` / `interlock_poll_ms` 同口径）。选 500 的理由：使**帧发布侧**的兜底最坏值 `0.5 + 0.25 = 0.75 s ≤ 1 s` 自身达标（PRD **F25 验收 3 的分句①**「≤ 站周期 + 1 帧节拍（≤1 s）」），**不产生"广播漏一次就不达标"的隐性依赖**。⚠️ **该取值不足以保证"告警位变化后 ≤2 s 上屏"在任何丢帧下成立**——见 §15.6.1 约束 1 的完整算式与两侧选项。

**变更比较函数的硬要求**（对齐 N-16）：

```rust
/// 内容真变化判定：**忽略 ts_ms / last_ok_ms / block.ts_ms / catalog_rev**，
/// 只比较 (站集合, 站 online 与 last_ok_ms, 块集合, 点集合的 (key, v, flag))。
/// 否则每 500 ms 的兜底采样都会因 ts 前进被判"变更" ⇒ 发布率打满 4 Hz（违背 §4.2 约束 2 的初衷）。
fn peripherals_changed(a: &PeripheralsSection, b: &PeripheralsSection) -> bool;
```

#### 15.1.2 我方向 `latest_values` 设计提出的**消费契约**（供该设计对接；本节不代改其设计）

**口径分三段（必须分清，否则会把"已存在的接口"误当"待新增的接口"）**：

| 段 | 内容 |
|----|------|
| **① 已存在**（01 设计 §9.1.2 / §9.1.3 **原文即含**，本节**直接消费**，零新增） | `subscribe() -> broadcast::Receiver<ChangeBatch>`（容量 64，`Lagged` 丢帧）；`get(&PointId) -> PointView`；`station_snapshot(&str) -> Vec<PointView>`（**单次持读锁克隆该站全部已登记点**，含不可得态）；`all()`；`station_is_active(&str, now_ms) -> bool`（**站级在线**，判定阈值 `stale_timeout_s` 的唯一持有者，内部持有，调用方只给 `now_ms`）；`is_fresh(&PointId, &PointValue, is_bit, now_ms) -> bool` |
| **② 本节曾错误假设**（**01 中不存在**，本节**已撤销该依赖**，N-19） | ~~`snapshot_roles(&[Role])`~~ → 改逐站 `station_snapshot`（5 站 5 调，KISS，零新增）；~~站级字段 `online` / `last_ok_ms`~~ → 改 `station_is_active()` + 见 ③；~~点级 `round_seq` / `upd_ms`~~ → 改用 `PointValue.ts_ms`（N-20）表达同一语义 |
| **③ 需新增**（**登记为对 01 设计的接口要求**，见 §15.9 **R-38 / R-39**） | `station_last_poll_ms(&self, station: &str) -> Option<u64>`（返回 `station_poll_ms[station]`；`None` = 从未轮询成功）——**唯一缺失项**，其余全部已具备 |

**消费契约（逐条）**：

| # | 我方的消费要求 | 01 现状 | 我方处置 |
|---|----------------|---------|----------|
| **C-1** | 内存储存 + 按站取快照，**非阻塞**、不查 DB、不返回 `Result` | **已满足**：`station_snapshot(&str)`（单次读锁克隆）+ `all()` | 直接消费；**本节所有取样点一律写 `station_snapshot(<站 id>)`**，不写 `snapshot_roles` |
| **C-2** | 变更通知 | **已满足**：`subscribe()` = `broadcast<ChangeBatch>`，容量 **64**，**落后即丢**（`Lagged`） | **按"会丢"设计**：通知只降时延，**收敛由兜底 tick 保证**（§15.1.1 / §15.6.1）。**不再要求必达** |
| **C-3** | 站级：`id` / `role` / **在线** / **最后成功时刻** | 在线 **已满足**（`station_is_active(station, now_ms)`，阈值真源在 `LatestValues` 内）；**`role` 由本侧配置给出**（`south_stations` 的 `stations[].role`，`config.rs:45-58`）；**「最后成功时刻」无公开读口**（`station_poll_ms` 是私有字段） | ⇒ **R-38**：请 01 增补 `station_last_poll_ms(station) -> Option<u64>`（**1 个 getter，不改任何既有语义**）。若 01 不接纳 ⇒ 「最后成功时刻」显 `--`（**不臆造、不由本侧自维护第二份真源**），EX-28 该字段不达成 |
| **C-4** | 点级：值 / 时刻 / 「本轮未更新」 | `value` / `ts_ms` / `quality` **已满足**；`apply` 无条件刷新 `ts_ms`（N-20）⇒「本轮是否更新过」**可由 `point.ts_ms` 与站级「最后成功时刻」比较得出**（同轮写入的点 `ts_ms == station_poll_ms`；本轮未写过的点 `ts_ms < station_poll_ms`） | ⇒ **撤销 `round_seq` 依赖**，改用上述**等价判据**（**仍不引入任何时间阈值**：比的是"同一轮"，不是"now − ts > X"）；该判据**同样依赖 R-38** 的 getter |
| **C-5** | **不得**把 `telemetry` 表作为取数路径 | — | 直接违反 RQ-9.0-1；本节零 DB 读 |
| **C-6** | 写入方 = core-bin `SouthSink` 的两回调；**`StationSink` trait 不改** | **已满足** | PRD §3.9 的跨文档约束；PCS 第二写入方见 §15.1.3 / R-28 |

**点级 `flag` 判定（组帧侧，纯内存；**不引入任何时间阈值**——只比"同一轮"）**：

```
站 station_is_active(站, now) == false     → Offline   (v = None)
点缺（station_snapshot 无此 metric 键）     → NotRead   (v = None)
标量点 且 点.ts_ms < 站.station_last_poll_ms → NotRead   (v = None)  ← 本轮该点未更新（含越界被 mapper 滤除）
位点（is_bit = true）                       → 跳过上一行判据（位点 ts_ms = 最后变化时刻，天然旧）
                                             只按 quality 判（01 §9.1.3 的位点口径）
point.quality != Ok                        → NotRead   (v = None)   ← 01 已判过陈旧 / 未配置，本节不重判
值非有限（NaN / ±Inf）                      → RangeError(v = None)
其余                                       → Valid     (v = Some(工程值))
```

> **为什么比"同一轮"而不是"最后更新时刻 + 阈值"**：硬约束要求「过期判据单一真源 = `stale_timeout_s` = 5 s，**不另立门限**」。**轮次边界**（`point.ts_ms` vs 站级 `station_last_poll_ms`）精确表达"本轮没采到这个点"，且**不引入任何新阈值**。**屏侧与组帧侧都不得**写 `now − ts_ms > X` 形式的判定（§15.2.3 同口径）。
>
> **R-38 未落地时的退化（诚实登记）**：无 `station_last_poll_ms` ⇒ 第 3 条判据**不可判** ⇒ 标量点"本轮未更新"退化为"点存在即按 `quality` 展示"。后果 = mapper 滤除越界点时**该点保留上一轮的值与 `ts_ms`**，屏上按 `Valid` 展示 ⇒ **陈旧值被显示为实时值**。这是 §15.9 **R-30** 的完整口径，**不由屏侧用时间阈值补**（补了就是第二套新鲜度判据，违反 F25.1）。

#### 15.1.3 ⚠️ 报告项：F24（PCS）缺第二写入方 + 10 号模块动作

PRD F24 明确「该站当前未启用；若本项上屏，PCS 增量读数的真源为 **`intercore` 的 3 区读通道**，而非 `south_stations.pcs` 站——两者不得同时启用」。而 `latest_values` 的写入方限定为 `SouthSink`（南向站回调）⇒ **`pcs_3zone_*` 键在 `latest_values` 中不会被写入**。

| 处置 | 说明 | 取舍 |
|------|------|------|
| **处置 1（本节采用）** | `latest_values` 增加**第二条写入路径**：core-bin 既有的 intercore 采集环（`read_three_phase` / `last_run_state` 同源）把 PCS 3 区扩展点表按点名写入**同一份**快照 | 守住 RQ-9.0-3「单一真源」（上云若也要 PCS 增量则共用同一份）；代价 = `latest_values` 写入方由 1 个变 2 个，**须其设计明文接纳** |
| **处置 2** | `display_host` 直读 intercore 扩展点表（不经 `latest_values`） | 不触及 `latest_values` 边界；但若 01 号也要求 PCS 增量上云，会出现**第二真源** ⇒ 违反 RQ-9.0-3 |

**本节的落地口径（按处置 1）**：`display_host` 对 `pcs_3zone_*` 与其余外设**一视同仁**地从段内取键。若该键缺失（处置 1 未落地）⇒ 按 §15.1.2 规则置 `NotRead` ⇒ 屏显「未取数」；**PCS 段仍可渲染**（不崩、不补 0），但 **EX-22「常显」不达成**。⚠️ 「常显」本身另有一条**独立于本节的**需求冲突（分段控件同屏仅 1 段可见），见 §15.9 **R-40**。

**另需 10 号（核间通信）配合**：F24 的 6 组新增量（1018–1021 / 1025–1040 / 1008–1012 / 1041 / 1042–1045 / 1066 / 1071 / 1072–1075）需 `intercore` **扩展读取**（当前仅读 5 个量，N-15）。该动作**不在 12 号范围**，且 PRD 的 **T-12 只登记了 01 / 03 号** ⇒ **本节据此报告：T-12 需增补 10 号**（§15.9 R-29）。

---

### 15.2 帧契约扩展（`display-proto` v3）

#### 15.2.1 版本号 2 → 3 与兼容策略

| 项 | 设计 |
|----|------|
| 版本号 | `PROTO_VERSION: u8 = 3`（`frame.rs:21` 单点改动）。**单一真源**，各段不带版本（F26.1） |
| 新段 | `DisplayFrame.peripherals: PeripheralsSection`，**`#[serde(default)]`**（与 v2 四段同范式，`frame.rs:406-418`） |
| **拒帧规则** | **不放宽**：`check_version()` / `from_json_slice()` 逐字不动（F26.2）。**不得**实现任何"兼容模式 / 容忍任意版本" |
| 旧帧落入新类型 | v2 帧（无 `peripherals`）反序列化到 v3 类型 ⇒ 段取 `Default` ⇒ `available=false` ⇒ 「外设数据不可用」（EDGE-22）；**但该帧仍会在 `check_version` 处被拒**（`version=2 ≠ 3`）——两条防线**语义不同、缺一不可** |
| 同版本未知键 | 保留容忍（既有回归锚点 `frame.rs:876-891`），与"拒异版本"不冲突（F26.6） |
| 既有字段 | F1–F5 / `device` / `alarms` / `info` / `interlock` 的**字段名、含义、量纲、缺省语义、`Field.flag` 四态**一概不改（F26.7） |
| 部署 | `deploy/deploy.md` 增补固定口径：**`mupcd` 与 `mupc-local-display` 必须同版本发布**（F26.3 末条） |

**两端共存（与 §15.6 情形③ 配套）**：

| 组合 | 帧层行为 | 屏上表现 |
|------|----------|----------|
| 旧 HMI（期望 2）+ 新 mupcd（发 3） | `from_json_slice` → `Err(ProtoVersionMismatch { got: 3, expected: 2 })`（`frame.rs:426-434`）⇒ HMI 侧归一为 `Error::ProtoVersion(url, 3, 2)`（`error.rs:59`） | ≤3 s 进「版本不匹配」降级画面（§15.6 情形③） |
| 新 HMI（期望 3）+ 旧 mupcd（发 2） | 同上（`got: 2, expected: 3`） | 同上（文案带两端版本号） |
| 两端同版本 | 正常 | 正常 |

#### 15.2.2 新增段类型（`display-proto`；编码直接照做）

**新建 `mupc/crates/display-proto/src/peripherals.rs`**，由 `lib.rs`（现 57 行）`pub use`。

```rust
/// U-73 外设段（F20–F24）。**元数据（中文名 / 单位 / 小数位 / 位语义 / 枚举文案）不在本段**——
/// 走 §15.3 的 catalog 只读端点，因元数据入帧是**结构性爆帧**：仅元数据一项即
/// `555 × 118 B = 65,490 B = 63.96 KiB` ≈ 整帧上限，整帧（n=100）**≈102.7 KiB ≫ 64 KiB**（§15.2.4 反证）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PeripheralsSection {
    pub ts_ms: u64,        // 本段组帧时刻（Unix ms；0 = 未采集）
    pub available: bool,   // 段可用性；Default=false ⇒ 「外设数据不可用」（EDGE-22），绝不伪装
    pub catalog_rev: u32,  // 元数据版本（§15.3.1）；屏侧据此决定是否重取 catalog
    /// 被帧预算守卫裁剪的块（形如 `"fire_det:119→111"`，**左 = 登记只数、右 = 实际携带只数**；
    /// `119→111` 的 n 与 `k_max` 取自 §15.2.4「守卫取值」表，二者必须一致）。
    /// 常态为空（PRD 上限 n=100 不触发，§15.2.4）。**屏侧必须显式提示，不得静默**（F21.4）。
    pub truncated: Vec<String>,
    /// 站点，顺序 = 站配置顺序（稳定不跳位，PRD §4.2.4）。
    pub stations: Vec<PeripheralStation>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PeripheralStation {
    /// 南向站 id（`south_stations.stations[].id`，如 "hvac" / "fire" / "bms" / "meter_batt" / "pcs"）。
    pub id: String,
    /// role 字面量（snake_case；见 [`PeriphRole`]）。
    pub role: PeriphRole,
    /// 站在线。**采集侧按 `stale_timeout_s = 5 s` 判定后的结论**；屏侧**不重判、不另立门限**。
    pub online: bool,
    /// 最后一次采集成功时刻（Unix ms；0 = 从未成功）→ 屏显「最后成功 12:03:44」。
    ///
    /// **来源 = 01 设计 §9.1 的 `station_last_poll_ms(station)`**（`station_poll_ms` 私有字段的
    /// 公开读口，**新增接口要求 R-38**）。未落地 ⇒ **本字段恒 0**（屏显 `--`，**不臆造**）。
    /// **禁止**由 `display_host` 自维护第二份"最后成功时刻"（同一事实不记两份）。
    pub last_ok_ms: u64,
    /// 消防「钢瓶气压未配置」标志（EDGE-23；**仅 role=Fire 有意义**）：
    /// `Some(false)` ⇒ 屏显「未配置」（**忽略 `v`**，不得显 `0 kPa`，不产告警）；
    /// `Some(true)` ⇒ 正常按值展示；`None` ⇒ 不可得（非消防站 / 接缝未接线）⇒ 按值正常展示。
    pub cylinder_configured: Option<bool>,
    /// 块，顺序 = 站配置 `regs` 顺序。
    pub blocks: Vec<PeripheralBlock>,
}

/// 外设 role。与 `mupc-southd::config::Role` 的 serde 名（`rename_all = "snake_case"`，
/// `config.rs:42-58`）逐字对应；**不含 `MeterGrid`**——台区总表不在本增量内（§15 范围外 #3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriphRole {
    Hvac,
    Fire,
    Battery,
    MeterBatt,
    Pcs,
    /// 不可得 / 未登记（**缺省态**，不臆造 role）。
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PeripheralBlock {
    /// 块名（点名前缀）：`hvac_in` / `hvac_di` / `fire_sys` / `fire_det` / `bms_io` /
    /// `bms_energy` / `bms_meta` / `bms_term` / `bms_cap` / `bms_alarm` / `mb_ui` /
    /// `mb_freq_line` / `mb_power` / `mb_phase` / `mb_e_act_*` / `pcs_3zone`。
    pub name: String,
    /// 本块最后一次成功读取时刻（Unix ms；0 = 未读）→ 屏显「最近更新 …」（F25.2）。
    pub ts_ms: u64,
    /// 显式点名覆盖：`(at, name)`（如 `(7, "fire_det_count")`、`(19, "soc")`）；空 = 全位置式。
    /// **由组帧侧从站配置的 `PointConf.name` 直接投影**（`points.rs:128-131` 同源，无第二份命名规则）。
    pub renames: Vec<(u16, String)>,
    /// 点值，顺序 = `at` 升序。**白名单内每一点都在**（含未取到的点，其 `flag != Valid`、`v = None`）
    /// ⇒ 屏侧行数与 catalog 行数恒等，**不因取数成败而变行**（F25 / §4.2.4「值变化不改布局」）。
    pub values: Vec<PointValue>,
}

impl PeripheralBlock {
    /// **点键的唯一构造点**：显式 `name` 覆盖优先，否则 `<块名>_<at>`（位置式口径）。
    /// 屏侧**不得**自行拼接或重命名。
    pub fn key(&self, pv: &PointValue) -> String {
        match self.renames.iter().find(|(at, _)| *at == pv.at) {
            Some((_, n)) => n.clone(),
            None => format!("{}_{}", self.name, pv.at),
        }
    }
}

/// 单点值。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PointValue {
    /// 块内偏移 + 1（位置式口径）。
    pub at: u16,
    /// 工程值：**量纲已由采集侧按登记 `scale` / `offset` 消解，屏侧不重算**（F20.1 / F22.4 / F24.1）；
    /// 位点以 `0.0` / `1.0` 表达（`discrete` 块逐位产点，N-6）。
    /// **`flag != Valid` 时必须为 `None`**（不补 0、不沿用旧值）。
    pub v: Option<f64>,
    /// 点级有效 / 降级标志。**四态语义与取值不得改**（F26.7）。
    pub flag: FieldFlag,
}

/// 帧预算守卫的常量（与 [`MAX_FRAME_BYTES`] 同处定义，**单一真源**）。
/// 三档口径**不得互相顶替**（§15.2.4 的实测依据，N-21）：
/// - `TYPICAL` 用于**容量陈述**（"n=100 占帧多少"）；
/// - `UPPER`   用于**守卫预检**（决定裁不裁、裁到几只）；
/// - `F64_ABS_MAX` 只用于**兜底说明**（出口守卫的失效模式），**不作预检口径**。
pub const POINT_JSON_BYTES_TYPICAL: usize = 33;   // {"at":201,"v":1.0,"flag":"valid"} = 33 B（实测）

/// 白名单内的**可达**上界：`at ≤ 594`（3 位）+ 工程值 ≤ 12 字符 + 最长 flag。
/// 实测形态 `{"at":594,"v":-214748364.7,"flag":"range_error"}` = **48 B**。
pub const POINT_JSON_BYTES_UPPER: usize = 48;

/// f64 最短往返表示的**理论**极值（`-1.7976931348623157e308`，23 字符）⇒ 60 B。
/// **工程不可达**（登记 `scale` 已消解量纲、幅值受 `reg_format` 约束）；仅用于说明
/// 出口守卫的失效边界。
pub const POINT_JSON_BYTES_F64_ABS_MAX: usize = 60;

/// 5 站 + 21 块的**固定字段** + `renames` + `truncated` 的保守上界（字节）。
/// 实测 ≈2.7 KiB（`name` / `id` / `role` / `online` / 时间戳 / 空数组括号）⇒ 取 4 KiB。
pub const PERIPH_FIXED_JSON_BYTES: usize = 4 * 1024;

/// 给既有 5 段（F1–F5 + `device` / `alarms` / `info` / `interlock`）的预留。
/// 实测基线（10 条短告警）≈**1.6 KiB** ⇒ 8 KiB = **≥5× 余量**。
/// ⚠️ **不覆盖** 10 条 × `MAX_ALARM_MESSAGE_BYTES`(1 KiB) = 10 KiB 的极端组合；
/// 该组合由 §15.2.4 步骤 5 的**出口守卫**兜底（登记为 R-43）。
pub const EXISTING_SEGMENTS_RESERVE: usize = 8 * 1024;

/// 外设段预算 = 帧上限 − 既有段预留 = **56 KiB**（87.5 %）。
/// 取值由「n=100（PRD 探测器上限）**不触发裁剪**」反解并留余量（§15.2.4 的算式）：
/// k_max = ⌊(56 KiB − 441×48 B − 4 KiB) / (6×48 B)⌋ = **111 只** > 99 只。
pub const MAX_PERIPH_BYTES: usize = MAX_FRAME_BYTES - EXISTING_SEGMENTS_RESERVE;   // = 56 KiB
```

> **为什么位点不做位图压缩（刻意不用）**：位点与标量同构（N-6）使两侧代码只需一条路径（KISS），且**实测**容量在 64 KiB 下有 ≈1.7× 余量（典型 33 B/点、n=100 占 63 %，§15.2.4）；引入位图会带来"位序/字节序/极性与其它位相反（探测器 bit15）"三类新歧义，收益仅 ~8 KB。**KISS 优先。**
>
> ⚠️ **命名冲突提醒（实现时必读）**：本段的 `PointValue`（`display-proto` 的**线格式**点值，字段 `at` / `v` / `flag`）与 01 设计 §9.1.2 的 `mupc_data_processing::latest_values::PointValue`（**内存快照**，字段 `value` / `ts_ms` / `quality`）**同名不同物**。两处在 `display_host.rs` 内会同时可见 ⇒ 实现**必须**用限定路径（`display_proto::PointValue` vs `latest_values::PointValue`）或给前者起别名（`use ... as FramePoint`），**不得**靠 `use` 通配把两者混在一处。

#### 15.2.3 `Field.flag` 四态如何表达「不可得 / 陈旧」

| 语义（EDGE / F 项） | `flag` | `v` | 屏上（P4 / P6） |
|---------------------|--------|-----|------------------|
| 正常 | `Valid` | `Some(x)` | 数值 + 单位（**小数位取自 catalog 的 `decimals`**，屏侧不自行决定） |
| 站离线（EDGE-18） | `Offline` | `None` | 该站**全部**行 `--` + 「**站离线**」；段顶站状态条显最后成功时刻 |
| 点未覆盖 / 本轮未取（EDGE-19） | `NotRead` | `None` | `--` + 「**未取数**」 |
| 值无效 / 非有限（EDGE-20） | `RangeError` | `None` | `--` + 「**数据异常**」 |
| 消防钢瓶气压未配置（EDGE-23） | 任意（由数据侧给） | 任意 | **优先级最高**：`station.cylinder_configured == Some(false)` ⇒ 「**未配置**」（忽略 `v`），**不得**含字符串 `0 kPa`，**不产生告警条目** |
| 段缺失 / 未采集（EDGE-22） | — | — | 整段「**外设数据不可用**」（`available=false`） |

**「陈旧」不新增第五态**（F26.7 硬约束），由**三层既有机制**表达，三层**语义不混、互不替代**：

| 层级 | 判据 | 单一真源 | 屏上 |
|------|------|----------|------|
| **帧 / 通道级** | `now − frame.ts_ms > DEFAULT_STALE_MS (2000 ms)` | `display-proto::DEFAULT_STALE_MS`（**既有**） | 「数据过期」角标（F5.3，**整屏级**） |
| **站级** | 采集侧按 `stale_timeout_s = 5 s` 判定后给出的 `online` | `south_stations.stale_timeout_s`（**既有**；本节**不重定义**） | 「站离线」+ 最后成功时刻（F25.4） |
| **块级（仅信息性）** | `block.ts_ms` / `station.last_ok_ms` | 采集侧写值时刻 | 「最近更新 12:03:44」（F25.2）——**屏侧只显示时刻，绝不据此判定** |

> ⚠️ **口径澄清（必须写进实现，否则必然误报）**：HVAC 站周期 **5000 ms** > `DEFAULT_STALE_MS` **2000 ms**（`production.yaml:399` vs `frame.rs:24`）。若屏侧用帧级 2 s 判据去判"HVAC 值过期"，**HVAC 会常年被标过期**。⇒ **帧级 2 s 判据只判"通道是否卡住"，不外推到外设值的新鲜度**；外设值新鲜度**只**由采集侧站级 `online` 表达（F25.1 的"沿用 F5 基线，不新造第二套新鲜度口径"由此成立）。

#### 15.2.4 容量结论：624 点是否需要分页 / 分帧

**编码口径（实测，N-21；本节所有数字由同一脚本按 `serde_json` 紧凑形态生成，可复现）**

| 形态 | JSON | 字节 |
|------|------|:----:|
| 典型 | `{"at":201,"v":1.0,"flag":"valid"}` | **33** |
| 白名单可达上界（`at ≤ 594` 3 位 + 工程值 ≤ 12 字符 + `range_error`） | `{"at":594,"v":-214748364.7,"flag":"range_error"}` | **48** |
| f64 理论极值（**工程不可达**，仅供兜底说明） | `{"at":594,"v":-1.7976931348623157e308,"flag":"range_error"}` | **60** |
| 未取数（`not_read`，`v=null`） | `{"at":65535,"v":null,"flag":"not_read"}` | 39 |

> ⚠️ **订正 v2.1-r1**：该版写「典型 28 B」——那是把 JSON 键当成 `"f"` 算的（真键名是 **`"flag"`**，`frame.rs:46` 的 `rename_all = "snake_case"` 决定取值为 `"valid"/"not_read"/"offline"/"range_error"`）；又写「上界 48 B」但给的样例是 `-1.2345678901234e-10`（20 字符，比工程可达值**长**），按真键名算是 51 B ⇒ **依据链不成立**。本版三档口径固定为 **33 / 48 / 60**，用途互不顶替（§15.2.2 常量注）。

**白名单点数（容量核算的分子；与 §15.5.2 字段表逐条对应）**

| 站 | 登记点（N-7） | **白名单点（n=20）** | 差（结构性不上屏） | 依据 |
|----|:---:|:---:|:---:|------|
| `bms` | 345 | **330**（`bms_io` 17 / `bms_meta` 8 / `bms_energy` 9 / `bms_term` 4 / `bms_cap` 4 / `bms_alarm` 288） | 15 | §15.5.2 段「电池」 |
| `meter_batt` | 40 | **38**（`mb_*` 38） | 2（PT / CT `mb_phase_7/_8`） | §15.5.2 段「储能表」 |
| `fire` | 127 | **127**（`fire_sys` 13 + `fire_det` 6×(n−1)） | 0 | §15.5.2 段「P4 消防」 |
| `hvac` | 34 | **33**（`hvac_in` 3 + `hvac_di` 30） | 1（位 25 保留 = `hvac_di_26`） | §15.5.2 段「空调」 |
| `pcs` | 72 | **27**（`pcs_3zone` 27） | 45（1043/1045/1073/1075 高半字 4 + **1046–1065 排除 20** + 其余未列入 21） | §15.5.2 段「PCS」 |
| **合计** | 618 | **555** | 63 | |

> **口径纪律**：v2.1-r1 的表把「登记点数」当容量分子（`pcs` 记 **55**、`bms` 记 **345**）——`pcs` 的 55 既非登记数（72）也非白名单数（27），是本设计**自造**的数；`bms` 的 345 含 15 个**不上屏**的点。⇒ 本版**一律用白名单数**；登记数只作对照列。**帧里装的是白名单点，多算的点根本不会出现在 JSON 里。**

**n=20 / n=100 的帧长（两口径并列，**不得混用**）**

> **本表每一格都是「一行一算式」，可独立复算**（KiB = 1024 B；帧预算 = `MAX_FRAME_BYTES` = 64 KiB = **65,536 B**；百分比 = 字节数 / 65,536，四舍五入取整）：
> - **实测典型** = `点数 × 33 B` + `2,772 B`（2.7 KiB，脚本实测固定开销）
> - **上界口径** = `点数 × 48 B` + `4,096 B`（`PERIPH_FIXED_JSON_BYTES`）
> - **整帧** = `peripherals 段` + `1,643 B`（1.6 KiB，既有段实测基线）

| 项 | n=20（555 点） | n=100（PRD 上限；1035 点） |
|----|------|-------------------|
| `fire_det` 寄存器数 = 6×(n−1) | 114（= 19 只） | **594（= 99 只）** |
| `peripherals` 白名单点数 | **555** | **1035**（555 + 480） |
| `peripherals` 段 —— **实测典型** | `555×33 + 2,772 = 21,087 B` ⇒ **20.6 KiB（32 %）** | `1035×33 + 2,772 = 36,927 B` ⇒ **36.1 KiB（56 %）** |
| `peripherals` 段 —— **上界口径** | `555×48 + 4,096 = 30,736 B` ⇒ **30.0 KiB（47 %）** | `1035×48 + 4,096 = 53,776 B` ⇒ **52.5 KiB（82 %）** |
| 既有段（F1–F5 + `device`/`alarms`/`info`/`interlock`，10 条短告警） | **实测 1,643 B = 1.6 KiB** | 同左 |
| **整帧 —— 典型** | `21,087 + 1,643 = 22,730 B` ⇒ **22.2 KiB（35 %）** | `36,927 + 1,643 = 38,570 B` ⇒ **37.7 KiB（59 %）** |
| **整帧 —— 上界** | `30,736 + 1,643 = 32,379 B` ⇒ **31.6 KiB（49 %）** | `53,776 + 1,643 = 55,419 B` ⇒ **54.1 KiB（85 %）** |

> ⚠️ **订正 v2.1-r2（本表 n=20 行，本版重算）**：该版写 **20.1 / 21.7 KiB（31 % / 34 %）**——与它**自述的算式**不符：反解只有 **≈32 B/点**（而非陈述的 33 B/点），且 n=20 行与 n=100 行差 **16.0 KiB** 而按算式应为 **15.47 KiB**。⇒ 本版按算式重算：`555×33 + 2,772 = 21,087 B = 20.6 KiB（32 %）`、整帧 `22,730 B = 22.2 KiB（35 %）`。**同表其余行逐格复核后不变**（30.0 / 52.5 / 31.6 / 54.1 KiB 与算式一致；§15.6.3 的 20.1 / 21.7 两处同步订正）。

> 两口径的**用途不同、不得顶替**：**实测典型**回答"平时占多少"（容量陈述）；**上界口径**回答"守卫何时裁"（§15.2.2 常量注：`TYPICAL` 只作陈述，`UPPER` 才是守卫预检的输入）。

**结论（与守卫式自洽）**：① 主帧**不分帧、不分页**；② **n ≤ 100（PRD 上限）在典型与上界两种口径下都装得下，且都不触发裁剪**（守卫按**上界**预检仍判"不裁"，见下「守卫取值」表）；③ 元数据必须出帧；④ 探测器明细必须有帧预算守卫；⑤ 明细下钻必须分页（一屏装不下 114 / 594 行，PRD F21.4）。

**反证：若把元数据（短标签 / 单位 / 小数位 / 位语义 / 枚举文案 / 拆解规格）逐帧携带**

`CatalogPoint` 的**实测**编码（同一脚本）：无位图 / 无拆解 / 无枚举 = **118 B**；带 2 位位图 = **346 B**；带拆解 + 枚举 = **336 B** ⇒ 白名单 555 条的元数据 = **118–346 B/条 × 555 = 64.0–188 KiB**（区间两端均由实测点值给出；**无"平均值"**）。

**判定"是否爆帧"一律取下界 118 B/条**（对"爆不爆"取最保守侧）：

```
元数据下界 = 555 × 118 B = 65,490 B = 63.96 KiB ≈ 64.0 KiB   ← 仅此一项 ≈ 整帧上限
站 / 块包装 ≈ 1 KiB
```

| 情形 | 整帧（= 既有段 1.6 + 外设值段 + 元数据下界 64.0 + 包装 1.0） | 后果 |
|------|------|------|
| n=20 | 22.2 + 65.0 ≈ **87.2 KiB** | **远超 64 KiB**——连 n=20 都装不下 |
| n=100 | 37.7 + 65.0 ≈ **102.7 KiB** | **直接爆帧**（≈1.6 × 上限）⇒ 整帧被 `FrameTooLarge` 丢弃 ⇒ 画面冻结且**发布方无法定位责任方**（正是 `frame.rs:32-38` 那条注释警告的失效模式） |

> ⚠️ **订正（三处依据不实，本版按推导重算）**：
> ① **「≈100.5 KiB」无推导**：它是 `555 × 185 B`，而 **185 B/条**这一"均值"在实测里**不存在**（实测只有 118 / 336 / 346 三个点值）⇒ 本版**只用 118 B/条 的下界**，并把算式逐项写出（见上）。
> ② **同一事实曾有三套并存数字**：§15.2.2 注写「69.7 KiB」、§15.3.1 写「≈67.9 KiB」、此处写「≈138 KiB」⇒ 本版**全部收敛到同一算式** `整帧(n=100) ≈ 102.7 KiB`（§15.2.2 / §15.3.1 已同步订正）。
> ③ **v2.1-r1 的「600 条目 × ~50 B ≈ 30 KB」低估约 2.4×**（实测真值 118–346 B/条）⇒ 该版 n=100 的"65 KB"虽**结论方向正确**，但**依据链不成立**（按它自述的 30 B/点口径只到 **62.5 KB，并未越 64 KiB**）。
> ④ §15.3.2 端点表与 §15.6.3 的 catalog 体量同步改为**实测区间 64–188 KiB**（不再用无出处的均值）。

⇒ **设计裁决（三条）**：

1. **元数据一律出帧**，走 §15.3 的 catalog 只读端点（一次性 + `catalog_rev` 比对）；
2. **`fire_det` 是唯一"随 n 无上界增长"的块** ⇒ 加**帧预算守卫** + **被裁显式标记**（下方伪码）；
3. **主帧不分帧、不分页**；**逐只明细下钻**走分页端点（§15.3.2）——因为**一屏装不下 114（或 594）行**（PRD F21.4「一屏不可全显」）。

**帧预算守卫（组帧侧硬要求，不得省）**：

```
组帧顺序（build_frame 内 peripherals 段部分）：
 1) 既有段（F1–F5 + device/alarms/info/interlock）——**永不裁剪**（F26.7：新增不得拖垮既有）
 2) peripherals 段可用性：latest_values 句柄缺失 / 快照取不到 ⇒ available=false（整段降级），跳过 3–5
 3) 非 fire_det 块：全量携带（白名单 441 点，与 n 无关）
 4) fire_det 块预算预检（**按 POINT_JSON_BYTES_UPPER = 48 B**，不按典型值）：
     其余块上界 = 441 × 48 + PERIPH_FIXED_JSON_BYTES(4096) = 25,264 B
     fire_det 上界 = 6×(n−1) × 48
     若 其余块上界 + fire_det 上界 > MAX_PERIPH_BYTES(57,344) ⇒ 裁剪：
       k_max = ⌊(MAX_PERIPH_BYTES − 其余块上界) / (6 × 48)⌋ = ⌊32,080 / 288⌋ = 111 只
       ⇒ 截断 fire_det 到前 k_max 只（**探测器按地址升序，前缀截断，保证可复现**）
       ⇒ truncated.push(format!("fire_det:{}→{}", n_total, k_max))
 5) 出口守卫：`DisplayFrame::to_json_slice()`（**既有唯一出口，不得绕开**，`frame.rs:462-481`）
      若 Err(FrameTooLarge)（估算失准的兜底）：
         tracing::error!("peripherals 段超帧预算，本段置不可用（不重试、不发超限帧）")
         available = false；**该帧照常发布**（既有段可用性不受影响）
```

**守卫取值与"何时裁剪"（逐条自洽核对，本节的关键表）**

| 判据 | 取值 | 结论 |
|------|------|------|
| `MAX_PERIPH_BYTES` | **56 KiB（= 64 KiB − 8 KiB 既有段预留）** | 不是取值 40 KiB 的拍脑袋数，而由「n=100 不裁」反解 |
| 预检口径 | `POINT_JSON_BYTES_UPPER = 48 B`（可达上界） | 用上界而非典型值，故**不会"按典型说不裁、按实际却爆帧"** |
| `k_max` | **111 只**（= 32,080 / 288） | — |
| **n=100 是否裁** | **不裁**（99 只 ≤ 111 只） | ✅ 与"PRD 上限内不裁"的结论**一致** |
| **裁剪触发条件** | `fire_det` 只数 **> 111** ⇒ **n ≥ 113** | 超出 PRD 上限（100）**13 只**才裁；现场每片 ≤120 寄存器 ≈ ≤20 只/片 ⇒ 需 ≥6 片串联才可能触发 |
| 裁剪后屏上表现 | `truncated` 非空 + 「明细超出帧预算已截断至 111 只」 | **不得静默**（F21.4） |
| 典型整帧（n=100） | **37.7 KiB（59 %）** | 余 **26 KiB** |
| 上界整帧（n=100） | **54.1 KiB（85 %）** | 余 **10 KiB** |
| **兜底边界（R-43）** | 若出现 f64 理论极值形态（`POINT_JSON_BYTES_F64_ABS_MAX = 60 B`/点，**上取整的保守常量**，**工程不可达**）⇒ `(441+594)×60 + 2,772 + 1,643 = 62,100 + 4,415 = 66,515 B = 64.96 KiB ≈ 65.0 KiB > 64 KiB` ⇒ 步骤 5 把**整段**置 `available=false`（既有段照发） | 已登记的失效模式：**比裁剪更粗**，故步骤 4 的**上界预检必须保留**（它能在绝大多数情形下提前收敛为"裁几只"而不是"整段丢"）。**订正 v2.1-r2**：该版写「≈66.6 KiB」——那是把 **KB**（66.5 KB）当 **KiB**（64.96 KiB）写混了单位；本版统一为 KiB 并给出字节算式 |

> **`EXISTING_SEGMENTS_RESERVE = 8 KiB` 的口径**：覆盖**实测**基线（≈1.6 KiB）**≥5×**。**不覆盖**「10 条告警 × 每条 `MAX_ALARM_MESSAGE_BYTES`(1 KiB) = 10 KiB」的极端组合（`frame.rs:32-38` 的守卫说明 / `:38` 的常量）——该组合下整帧可达 56 + 10 + 1.5 ≈ 67.5 KiB，由步骤 5 兜底（**登记 R-43**）。选 8 KiB 而非 12 KiB 的理由：12 KiB 会把 `k_max` 压到 97 只 < 99 只 ⇒ **PRD 上限内就要裁**，与本节的容量结论直接冲突；而告警消息的现实长度（单行 UI 文案）为数十字节量级，10 KiB 组合需 10 条同时写满 1 KiB。

---

### 15.3 元数据（点表目录）与明细下钻：三个只读端点

#### 15.3.1 为什么元数据必须出帧

| 理由 | 说明 |
|------|------|
| **帧预算** | §15.2.4 反证（**下界口径**）：仅元数据一项即 `555 × 118 B = 65,490 B = 63.96 KiB` ≈ 整帧上限 ⇒ n=100 整帧 **≈102.7 KiB ≫ 64 KiB ⇒ 爆帧**（n=20 亦达 **≈87.2 KiB**） |
| **语义** | 元数据**静态**（除现场改配置外不变），逐帧重发是纯浪费；主帧应只载「值」（KISS） |
| **键与量纲真源** | **点名（键）** 与 **单位 / 小数位** 的真源 = 站配置（块名 / `at` / `name`）+ `point_table.rs` 登记（`scale`）；由 mupcd 一次性投影下发，**屏侧不硬编码、不重算** ⇒ 现场改配置后屏自动跟随 |
| **文案真源** | **屏用短标签**的真源 = `display-proto` 的**短标签表白名单**（D22 / §15.7 F-4）——**不得**把 `point_table` 的登记 `label`（含全角括号的登记说明文本）直接上屏 |

**被否决的备选：HMI 侧 build-time codegen 静态名表**（从 `point_table` 生成 `.rs` 表入库）。
理由：① 会引入 `local-display → mupc-southd` 的**构建期依赖**，与 §11.4 静态约束 ②（`local-display` 依赖图不得含 `mupc-southd`）的**架构意图**冲突，且需为构建依赖另立豁免口径；② 静态表**不跟随现场配置**（现场改块名 / 启用状态 / n ⇒ 屏上名字全丢，只能显「未命名点」）——对一个"现场可配点表"的系统是真缺陷。⇒ **选运行时 catalog 端点。**

`catalog_rev` 的产生与比对：

```rust
/// 元数据版本 = catalog **稳定文本**的 CRC32（同一函数同时产出
/// `PeripheralCatalog.rev` 与 `PeripheralsSection.catalog_rev`，**单一真源**）。
/// 输入 = 站配置（id/role/块名/at/name/format）∪ point_table 登记（scale / BitClass）∪
///        短标签表（`peripherals_labels`：label / unit / group）∪ 展示层附加（bits / enum_labels / decompose）。
pub fn catalog_rev(cat: &PeripheralCatalog) -> u32;
```

屏侧行为：`frame.peripherals.catalog_rev != 本地缓存.rev` ⇒ **异步重取 catalog（不阻塞显示）**；重取失败 ⇒ 保留旧 catalog + 顶部提示「名称表可能过期」；**从未取到** ⇒ 值照常显示（按点名），中文名位显「**名称未获取**」，并给出「重试」按钮（**不臆造中文名**）。

#### 15.3.2 三个端点定义（**控制通道**，只读、无副作用、不进 PL-1 审计）

| 端点 | 方法 | 参数 | 返回 | 用途 |
|------|------|------|------|------|
| `/v1/console/peripherals/catalog` | GET | — | `PeripheralCatalog`（白名单 555 条；`CatalogPoint` 实测 **118–346 B/条** ⇒ **≈64–188 KiB**） | P4 / P6 首次进入、或 `catalog_rev` 变化时读取（**一次性**；**不进帧**，故不受 64 KiB 帧预算约束） |
| `/v1/console/peripherals/fire_detectors` | GET | `page`（1-based，默认 1）、`page_size`（默认 20，**≤50**） | `FireDetectorPage` | P4 探测器明细分页（F21.4 的「登记数与明细一致、不静默裁剪」） |
| `/v1/console/peripherals/bms_alarms` | GET | `page`（1-based，默认 1）、`page_size`（默认 50，**≤100**） | `BmsAlarmPage` | P6 电池段「查看全部 288 位」下钻（F22.3） |

**放控制通道的三条理由**：① **不改 D6**「读侧只有一个端点」——读通道保持"小、快、无副作用、客户端简单"；② 复用 `ConsoleClient` 的**非阻塞状态机**与"GET 失败走 HTTP 状态码、不把错误体当数据解析"的既有口径（§3.4 两条补注）；③ 与 `/config`（`GET /v1/console/config`）、`/logs`、`/audit` 同属**"一次性 / 带参 / 有限额的受控读"**（§3.4）。

**失败语义（与既有 GET 一致）**：非 2xx ⇒ `ConsoleError::HttpStatus`（`console.rs:185`）⇒ 按"该端点不可用"处理，**不阻断实时值显示**（值走读通道）。

```rust
// crates/display-proto/src/peripherals.rs（续）

/// 外设点表目录（**静态元数据**，一次性读取；不进主帧）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PeripheralCatalog {
    pub rev: u32,             // = catalog_rev(自身)；与帧内 catalog_rev 同源同值
    pub generated_ms: u64,    // 构建时刻（Unix ms）
    pub stations: Vec<CatalogStation>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CatalogStation {
    pub id: String,
    pub role: PeriphRole,
    /// 该站是否在 `south_stations` 中启用（false ⇒ 屏侧在 P6「装置」段的站状态表显「未启用」）
    pub enabled: bool,
    pub blocks: Vec<CatalogBlock>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CatalogBlock {
    pub name: String,
    pub kind: CatalogBlockKind,          // 决定屏侧呈现：数值行 vs 位行
    pub renames: Vec<(u16, String)>,     // 与 [PeripheralBlock::renames] 同源同值
    pub points: Vec<CatalogPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogBlockKind { #[default] Scalar, Discrete }

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CatalogPoint {
    pub at: u16,
    /// **屏用短标签**。真源 = `display-proto::peripherals_labels` 的**白名单表**（D22 / §15.7 F-4），
    /// **不是** `point_table` 的登记 `label`（后者是含全角括号的登记说明文本，直上屏会：
    /// ① 实测字库缺口 189 码位；② 界面噪音；③ 且以运行时字符串到达 HMI ⇒ 既有码表覆盖率测试看不到）。
    pub label: String,
    /// 单位（如 `℃` / `%RH` / `V` / `A` / `kW` / `kvar` / `kVA` / `kPa` / `ppm` / `dB/M` /
    /// `kWh` / `kvarh` / `kΩ` / `Hz`）；`None` = 无量纲（如功率因数）。
    pub unit: Option<String>,
    /// 小数位。**由登记 `scale` 派生**（§15.3.2 映射表）；屏侧不得自行决定。
    pub decimals: u8,
    /// 位语义。**两种位形态统一表达**（见下）。
    pub bits: Vec<BitMeta>,
    /// 枚举文案（值 → 文案）；空 = 非枚举 / 文案未登记（屏侧显「模式 <值>」，**不猜**）。
    pub enum_labels: Vec<(u16, String)>,
    /// 展示层拆解规格（F21.5「数据 1」；空 = 不拆解、按整字显示）。
    pub decompose: Vec<Decompose>,
    /// 页内分组键（固定枚举字面量，见 §15.4 / §15.5 分组表）。HMI 用**静态 map** 把键映射到
    /// **中文分组标题**（标题是 UI 字面量，受既有码表扫描覆盖；见 §15.7）。
    pub group: String,
}

/// 位语义。**两种承载形态统一表达**（N-6 已确认位点与标量同构）：
/// - **离散位块**的点（`hvac_di` / `bms_alarm`）：`bits` 恰含 1 项，`index` = 该点自身位号（= `at − 1`）；
/// - **字内位图**的点（消防 `fire_sys_1` / `fire_sys_3` / `_4` / `_5`、探测器 `+1 状态` 整字）：
///   `bits` 含该字的**全部 16 位**；**未定义位 `defined = false`** ⇒ 屏显「**未定义位 n**」，
///   **不猜语义**（F21.1 / EX-09）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BitMeta {
    pub index: u8,                // 位号（0 起）
    pub label: String,            // 已定义位的中文名；未定义位为空串
    pub class: CatalogBitClass,
    pub defined: bool,
    /// 该位的活跃语义（屏侧文案用：「故障 / 动作」等）；`None` = 用通用「活跃 / 非活跃」。
    pub active_text: Option<String>,
    /// 与其余位**极性相反**的位（如探测器 bit15「0 = 离线」）：
    /// `true` ⇒ 屏侧文案为「在线 / 离线」，**不得**并入报警位图解读。
    ///
    /// **R-41 裁定前无生产者**：catalog 构建器一律填 `false`；探测器 bit15 走
    /// `defined: false` ⇒ 屏显「未定义位 15」（点表明令「不猜、不造判据」+ PRD F21 未要求）。
    /// 若产品 / 厂方追认「通信状态」，**只改白名单 + 本字段**即可启用（屏侧代码零改动）。
    pub inverted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogBitClass { Alarm, State, #[default] Reserved }

/// 展示层拆解（F21.5）。`DecodeFrom` 是**唯一**的字节语义来源，屏侧不自行猜位序。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Decompose {
    pub label: String,
    pub unit: Option<String>,
    pub decimals: u8,
    pub from: DecodeFrom,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecodeFrom {
    Whole    { scale: f64, offset: f64 },
    HighByte { scale: f64, offset: f64 },
    LowByte  { scale: f64, offset: f64 },
}

/// 探测器明细分页（F21.4）。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FireDetectorPage {
    pub page: u32,
    pub page_size: u32,
    /// 登记数（`fire_det_count`）；`None` = 未取数（屏显 `--`）。
    pub total: Option<u16>,
    /// 实际可读只数（由配置展开决定）。`expanded != total` ⇒ 屏侧**显式提示不一致**，
    /// **不得静默裁剪**（F21.4 / EX-12）。
    pub expanded: u16,
    pub has_more: bool,
    /// 消防源可用性。`false` ⇒ 「消防源不可用」，**不得**显为"无探测器"（EDGE-09 同口径）。
    pub available: bool,
    pub items: Vec<FireDetectorItem>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FireDetectorItem {
    /// 探测器序号（1 起，与登记数对齐）。
    pub index: u16,
    pub addr: PointValue,   // `+0 地址`（1–254；Q-9 地址序核对源）
    pub state: PointValue,  // `+1 状态`（整字；位语义见 catalog `bits`）
    pub data1: PointValue,  // `+2 数据 1`（整字；拆解见 catalog `decompose`，F21.5）
    pub co: PointValue,     // `+3` CO ppm
    pub voc: PointValue,    // `+4` VOC ppm
    pub h2: PointValue,     // `+5` H₂ ppm
}

/// BMS 告警位下钻分页（F22.3）。**名称由 catalog 按下标提供**，本 DTO 不重复携带。
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BmsAlarmPage {
    pub page: u32,
    pub page_size: u32,
    /// 位数总量（288；供窗口化列表算总高）。
    pub total: u32,
    /// 当前活跃位数。
    pub active_total: u32,
    pub has_more: bool,
    /// 告警源可用性：`false` ⇒ 「BMS 告警源不可用」（**≠** 「无活跃告警位」，EDGE-24）。
    pub available: bool,
    pub items: Vec<BmsAlarmItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BmsAlarmItem {
    pub at: u16,        // 点内偏移 + 1（键 = `bms_alarm_{at}`）
    pub active: bool,
}
```

**`decimals_from_scale` 映射（唯一实现，放 `display-proto`）**：

| 登记 `scale` | `decimals` | 覆盖用例 | 与 PRD 声明值的比对结论 |
|--------------|-----------|----------|-------------------------|
| `1.0` | **0** | 温度整数（含 BMS 端子 / 极柱 / 单体 / 模块）/ SOH / SOE / 绝缘电阻 / 从控数量 / 消防全部（含 kPa）/ **BMS 累计容量 `Ah`（`bms_cap_*`，见 §15.9 R-31）** | 一致（`Ah` 一项与 PRD 声明不符 ⇒ R-31） |
| `0.1` | **1** | HVAC 温湿度 / BMS 簇组电压·电流 / 功率 / 累计电量（`kWh`）/ ADL400 电压 / 线电压 / 不平衡度 / PCS 相电压 / PCS 累计电量 | 一致 |
| `0.01` | **2** | ADL400 电流 / 频率 / 电能 / 零序 / PCS 交流母线频率 | 一致 |
| `0.001` | **3** | BMS 单体电压 / 电压极差 / ADL400 功率 / PF / PCS 视在 · 无功 · PF | 一致 |

> **`decimals = 0 if scale >= 1.0 else (-log10(scale)).round() as u8`（`scale` 仅取 1.0 / 0.1 / 0.01 / 0.001 四值，登记表已核对无其它值）**；单测需遍历 in-scope 登记行断言四值覆盖完备。

**catalog 是白名单投影（三条硬要求）**：

| # | 要求 | 落地断言 |
|---|------|----------|
| **W-1** | **只有 §15.4 / §15.5 字段表列出的点才进 catalog**（白名单按 `(role, block, at)` 枚举）——`pcs_3zone` 的 1046–1065（STS / 负载区，**含 `_50` = 1049**，见下）、`mb_phase_7/_8`（PT/CT）、`hvac_di_26`（位 25 保留位）等**结构性不在表内** | 负向验收（EX-08 / EX-21 / EX-23）由"表里没有"直接成立；HMI 静态用例 T-19 |
| **W-2** | **量纲投影一致性**：`unit` / `decimals` 的**唯一来源** = `point_table::lookup_in(role, space, addr)` 的 `scale` + 短标签表的单位；catalog 构建器**不得**另写数值字面表 | 单测：对 catalog 每一行，`lookup_in` 必命中且 `decimals == decimals_from_scale(scale)`；短标签表的键**必须命中一个登记行或站配置点**（否则红） |
| **W-3** | **文案覆盖性**：白名单的**每一项**都有短标签（行数相等 ⇒ 防"上了帧却没有屏上文案"） | `display-proto` 单测（§15.8 T-7 / H-3）。⚠️ "行数相等"要能**机械写出来**，前提是白名单**可枚举** —— 见下 |
| **W-4** | **白名单本身必须可枚举**（否则 W-1 / W-3 的"行数相等"断言写不出来） | 白名单**不得**是散落在文档里的表格，而须是**代码内的常量数组**：`pub const PERIPH_WHITELIST: &[(PeriphRole, &str, u16)]`（由 §15.11 落点 3 的 `peripherals_labels.rs` 导出，与短标签表**同一文件、同一批字面量**）。断言 = `PERIPH_WHITELIST.len() == LABELS.len()`（**行数相等**）+ `PERIPH_WHITELIST ⊆ lookup_in 命中集`（W-2）+ 全覆盖于可用静态枚举。**注**：`fire_det` 段随 n 展开 ⇒ 该块在常量表内以**模板**记 6 行（`(Fire, "fire_det", 1..=6)`），运行期按 `FIRE_DET_TEMPLATE_START` / `FIRE_DET_STRIDE`（`point_table.rs:867` / `:869`）展开 —— **模板 + 展开公式**是机械可断言的（T-6 已锁 `key()` 公式），不存在"写不出的断言" |

**探测器点名的构造公式（catalog 构建器直接照做；`PointConf` 无 `name` ⇒ 位置式）**：

| 探测器 | 地址 | 状态 | 数据 1 | CO | VOC | H₂ |
|--------|------|------|--------|----|-----|----|
| **k = 1**（在 `fire_sys` 块内） | `fire_sys_8` | `fire_sys_9` | `fire_sys_10` | `fire_sys_11` | `fire_sys_12` | `fire_sys_13` |
| **k ≥ 2**（在 `fire_det` 块内） | `fire_det_{6(k−2)+1}` | `fire_det_{6(k−2)+2}` | `fire_det_{6(k−2)+3}` | `…+4` | `…+5` | `…+6` |

> 依据：`production.yaml:365-390`（`fire_sys` addr 4 / count 13，`at` 8–13 为探测器 1；`fire_det` addr 17 起、每只 6 寄存器）；`point_table.rs:866-869`（`FIRE_DET_TEMPLATE_START = 17` / `FIRE_DET_STRIDE = 6`）。k ≥ 2 时末只 `k = n` 的末寄存器 `at = 6(n−2)+6 = 6n−6`，n=20 ⇒ 114（与配置 `count: 114` 逐字吻合）。

**枚举文案的登记（`enum_labels` 的两个来源，二者都不许屏侧猜）**：

| 点 | `enum_labels` | 来源（**逐条指出真实出处**） |
|----|---------------|------|
| `fire_sys_6`（火警等级） | `[(0,"正常"),(1,"一级报警"),(2,"二级火警"),(3,"未定义"),(4,"紧急启动"),(5,"紧急停止")]` | **六个文案的唯一权威 = PRD §3.9 F21 的展示表**（原文「0 正常 / 1 一级报警 / 2 二级火警 / **3 预留 ⇒ 显「未定义」** / 4 紧急启动 / 5 紧急停止；**枚举外值显「未知」**」）。点表侧 `point_table.rs:810` 的登记串（`0 正常/1 一级报警/2 二级火警/3 预留/4 紧急启动/5 紧急停止`）与之逐值一致，可作**交叉佐证**。**表外值 ⇒ 屏显「未知」，绝不落「正常」**（F21.2 / EX-10） |
| `pcs_3zone_67`（工作模式判断） | **空**（厂方未给值枚举） | `point_table.rs:744` 仅登记「（枚举）」无值表 ⇒ 屏显「模式 `<值>`」并在旁标注「（枚举文案待厂方追认）」；**不臆造**（§15.9 **R-32**） |
| 其余 | 空 | — |

> ⚠️ **订正 v2.1-r1**：该版把 6 值枚举归因到 `SIG_FIRE_LEVEL`（`point_table.rs:275-300`）——**不实**。该表是**事件产点**的登记，实为 **`static SIG_FIRE_LEVEL: [SignalSpec; 4]`（仅 4 条）**：`level1(1)` / `level2(2)` / `emg_start(4)` / `emg_stop(5)`，**无「0 正常」（= 非活跃）也无「3 未定义」（= 预留）**（`point_table.rs:274` 的注释即明写「值 3 预留不入 `active`；值 0 工作正常 = 非活跃」）。⇒ 展示层 6 值**不得**引它为依据；事件层的 4 值**不得**被当成展示层值域。

**`decompose` 的具名用例（唯一的拆解点，`fire_sys_10` 与 `fire_det_{6(k−2)+3}`，即「数据 1」）**：

```rust
decompose: vec![
    Decompose { label: "烟雾".into(), unit: Some("dB/M".into()), decimals: 1,
                from: DecodeFrom::HighByte { scale: 0.1, offset: 0.0 } },
    Decompose { label: "温度".into(), unit: Some("℃".into()), decimals: 0,
                from: DecodeFrom::LowByte { scale: 1.0, offset: -55.0 } },
]
```

> 依据：`point_table.rs:814`「高字节烟雾 0.1 dB/M、低字节温度 raw−55 ℃，拆解在展示层 G-6」；`point_table.rs:814` 明写**整字口径不变**（telemetry 侧不拆）⇒ 拆解**只发生在 HMI 展示层**（F21.5 / EX-13 的"telemetry 侧整字值不变"由本设计保证：`latest_values` 与帧内 `v` 都是**整字**）。

**`bits` 的具名用例（三类，其余按登记投影）**：

| 点 | `bits` 内容 | 依据 |
|----|-------------|------|
| `hvac_di_{at}`（离散位，每点 1 位） | `[{ index: at−1, label: <登记 label>, class: <BitClass 映射>, defined: class != Reserved, active_text: Some("运行"/"告警"等), inverted: false }]` | `point_table.rs:833-863`（`BitClass::Alarm` 20 条 / `State` 10 条 / `Reserved` 1 条，位 25 保留） |
| `fire_sys_1`（字内位图，16 位） | 6 个已定义位（`index` 14/13/11/10/9/8，label 取 `SIG_FIRE_SYS_STATUS` 的中文名 + `point_table.rs:805` 的括号说明）+ 10 个 `defined: false` | `point_table.rs:232-257`（`SIG_FIRE_SYS_STATUS` 6 条）、`:805` |
| `fire_sys_9` / `fire_det_{...+2}`（探测器状态整字） | **2 个有意义位**（`index` 12 报警总状态 / 14 故障总状态）+ **14 个 `defined: false`** | `point_table.rs:302-316`（`SIG_FIRE_DETECTOR` 2 条）+ PRD F21 展示表（「bit12 报警总状态 / bit14 故障总状态」） |

> ⚠️ **bit15「通信状态」的处理（不得默默使用）**：`SIG_FIRE_DETECTOR` 的注释**明写** bit15「**0 = 离线，与其余位极性相反**」并把它列在「**不登记**」清单里，理由是「极性反转位与反馈位**无明确告警语义 ⇒ 不猜、不造判据**」（`point_table.rs:302-306`）；而 PRD F21 展示表也只要求 bit12 / bit14。⇒ **本设计默认不把它当"第 3 个有意义位"**（列为 `defined: false` ⇒ 屏显「未定义位 15」），并**登记 R-41 由产品 / 厂方追认**。`BitMeta.inverted` 字段**保留**（结构性支持），但在 R-41 裁定前**无生产者**（catalog 构建器一律填 `false`）。
>
> ⚠️ **"不猜"纪律**：**未定义位一律 `defined: false`**，屏显「未定义位 n」，**禁止**为凑满 16 位而编造语义（F21.1 / EX-09）。
>
> **订正 v2.1-r1**：该版把 bit15 当第 3 个有意义位（`inverted: true`）——**越界**（PRD 未要求、点表明令"不猜"）。

---

### 15.4 显示面（一）：P4 安全 · 联锁页（F21 消防）

**页面归属依据**：T-8 裁定（PRD §3.9.1）——消防与联锁**同源**，现场处置「看状态 → 释放 → 授权」与消防状态**须同页可见**。**页数不增（仍 6 页）**。

**版面（1024×768；**R-36 已补齐** —— 可实施规格落 **UI 设计文档 §6.4.1**，本节只给结构与归属）**：

| 分区（自上而下） | 矩形 y | 高度 | 内容 | 是否滚动 |
|------------------|--------|:----:|------|----------|
| 页眉（既有） | 0–72 | 72 | 时钟 │ 通道状态 │ 「触摸不可用」角标（EDGE-13） | 否 |
| **【安全总览带】**（常驻） | **80–244** | **164** | 联锁总态卡 `(16,80,504,244)` **488×164** ＋ 火警等级卡 `(520,80,1008,244)` **488×164**（**卡间隙 16 px = `GAP_MIN`**） | 否 |
| **【滚动区】** `LV_DIR_VER` | **260–600** | **340** | §B 联锁明细（既有 `216 + 16 + 108 = 340` ⇒ **首屏全可见**）→ §A 消防四组 | 是 |
| **【固定操作条】**（既有，**位置不动**） | 624–696 | 72 | 「人工释放联锁」/「M1 授权重启」320×64 两枚，**间距 48 px = `GAP_DANGER`** | 否 |
| 底部导航（既有） | 696–768 | 72 | 6 页页签 | 否 |

> **几何单一真源 = UI 设计文档 §6.4.1**（本节只列分区与高度，矩形以 UI 为准）。

```
┌ 页眉（既有）：时钟 │ 通道状态 │ 「触摸不可用」角标（EDGE-13）
├ 【安全总览带】常驻不滚动，两卡并排（各 488×164，间隙 16 px）
│    ┌ 联锁总态卡（F16 既有语义，文案/字号不变）┐  ┌ 火警等级卡（F21 新增）┐
│    │ 「已联锁 / 未联锁 / 联锁状态不可用」96 px │  │ 枚举文案 96 px + 图标   │
│    └──────────────────────────────────────────┘  └────────────────────────┘
├ 【滚动区】LV_DIR_VER（y260–600，视口 340 px）
│   §B 联锁（F16 既有，原样不动）：触发源明细 / 停机失败 / 故障灯 / 运行灯（首屏即全可见）
│   §A 消防（F21 新增，四组，自上而下）：
│      A1 系统状态(16 行) → A2 灭火瓶压力 → A3 探测器触发 → A4 探测器（汇总 + 明细分页）
├ 【固定操作条】（F17 / F18 既有，位置与规格逐字不变）：释放联锁 / M1 授权
```

**为什么「写操作」**不进**滚动区**（对 v2.1-r1 的收紧）**：v2.1-r1 把 §C 写操作放进滚动区。⇒ 破坏性按钮会随滚动移出视口，且会与 A 组内容**共处同一命中面**（F14.2「破坏性操作与取消/返回间距 ≥48 px」在滚动容器内**无法静态保证**）。本版把 §C **固定在 y624–696**（与既有 §6.4 逐字一致）⇒ ① 既有 `IL-01~IL-03` / `PL-04~PL-06` 回归面**最小**；② `GAP_DANGER = 48 px` 仍是**常量可断言**的；③ 「看状态 → 释放 → 授权」闭环不变（总览带常驻 + 操作条常驻）。

**分组顺序的理由**：既有联锁区在滚动区**顶部**（首屏即全可见，不与新内容争夺第一屏）；消防的**最高优先级信息（火警等级）已在常驻总览带**，明细下移不影响判读（"同页可见"成立）。

> ⚠️ **「常显」口径冲突（登记 R-40，待产品裁定）**：PRD EX-05 / EX-14 / EX-18 / EX-19 / EX-20 / EX-22 对 HVAC / BMS / 储能表 / PCS 各组用了「**常显**」二字，而 P6 的**分段控件同屏仅 1 段可见**（且 PRD B8 的"分区切换"许可**只限主状态页**，PRD §0 B8 / §4.2 原文）。本设计的**建议口径**：**「常显」= 段内无隐藏条件**（进入该段即可见，无需二次点击 / 不因取数成败而隐藏 / 不折叠）；**两侧选项与影响见 §15.9 R-40**。**本设计不自行改需求**。

**P4 新增分组与字段表（catalog 的 `group` 键 → 呈现规格）**：

| 分组键 | 分组标题（**UI 字面量**） | 点位（键） | 单位 | 小数位 | 字号 | 呈现规格 |
|--------|---------------------------|-----------|------|--------|------|----------|
| `fire_level` | **火警等级**（**总览带卡**） | `fire_sys_6` | — | — | **≥64 px**（L1-主值） | 枚举文案（§15.3.2 表）；**表外值 ⇒ 「未知」**（中性色，**绝不用绿色**，F21.2 / EX-10）；**文字 + 语义色 + 图标三重冗余**（F14，`StatusChip` 组件） |
| `fire_sys_status` | 消防系统状态 | `fire_sys_1`（整字位图） | — | — | 24 px（标注） | **逐位 16 行**：已定义 6 位（主电故障 / 备电故障 / 驱动电路 / 压力传感器 / 电磁阀 / 喷洒标记）显「位名 + 活跃 / 非活跃」；**未定义位显「未定义位 n」**，不猜语义（F21.1 / EX-09） |
| `fire_cylinder` | 灭火瓶压力 | `fire_sys_2` | `kPa` | 0 | 32 px（过程量） | `cylinder_configured == Some(false)` ⇒ 显「**未配置**」（**忽略 `v`**；断言不含 `0 kPa`）；否则显数值 + `kPa`；**该点不产告警条目**（EDGE-23 / EX-11） |
| `fire_trigger` | 探测器触发 | `fire_sys_3` / `_4` / `_5` | — | — | 24 px | 各 3 位：bit0 干接点触发 / bit1 复合触发 / **bit2 预留 ⇒ 显「预留」**（不显 0/1 语义） |
| `fire_detector` | 探测器 | 汇总：`fire_det_count`；明细：`fire_sys_8.._13`（第 1 只）+ `fire_det_{6(k−2)+1..+6}` | 见下 | 见下 | 24 px | ① 汇总行：「登记 **N** 只 / 可读 **M** 只 / 报警 **x** / 故障 **y** / 离线 **z**」；`N != M` ⇒ **显式提示不一致**（**不得静默裁剪**，F21.4 / EX-12）。② 「查看明细」⇒ **分页表**（`page_size = 20`，末页不足不补齐）：列 = 序号 │ 地址 │ 状态 │ 烟雾 `dB/M`(1) │ 温度 `℃`(0) │ CO `ppm`(0) │ VOC `ppm`(0) │ H₂ `ppm`(0)；「上一页 / 下一页」（`≥48×48 px`）＋「收起」（`≥48×48 px`，返回汇总行） |

**明细列的展示规则（逐条可测）**：

| 列 | 来源 | 规则 |
|----|------|------|
| 地址 | `+0 地址`（`fire_sys_8` / `fire_det_{…+1}`） | 原值（1–254）；未取数 ⇒ `--` +「未取数」 |
| 状态 | `+1 状态`（整字） | 按 catalog `bits` 渲染 **2 个已定义位**（**报警总状态** bit12 / **故障总状态** bit14）——每格 = `LedIndicator`（圆 16 px，色 + 文字双通道）+ 位名 24 px；其余 14 位**不展开**（明细表列宽有限）。**bit15 通信状态不在此列**：点表登记明确「不猜、不造判据」且 PRD F21 未要求 ⇒ 屏显依 A1 口径为「未定义位 15」（**待 R-41 追认后才可能启用**） |
| 烟雾 / 温度 | `+2 数据 1`（整字） | **按 catalog `decompose` 拆解**（高字节 ×0.1 `dB/M`；低字节 ×1.0 −55 `℃`）；**拆解只发生在 HMI 展示层**，帧内 `v` 与 `latest_values` 里保持**整字**（F21.5 / EX-13） |
| CO / VOC / H₂ | `+3` / `+4` / `+5` | 原值 + `ppm`（0 位小数） |

**与既有联锁区共存的三条不冲突保证**：① 联锁区**字段、文案、写操作、二次确认级别一律不动**（§6.4 原文有效；写操作条**位置也不动**，见上表）；② 新增区**全部只读**，不产生写操作 ⇒ 不触发 PL-1 审计与 §3.6 F14 二次确认（PRD §3.9.1 第 2 条）；③ 视觉分隔与触摸目标（**常量取自 `theme.rs` 单一真源，页面不得写裸值**）：

| 相邻关系 | 常量 | 值 | 落点（**几何以 UI §6.4.1 为准**） |
|----------|------|:--:|------|
| 联锁总态卡 ↔ 火警等级卡 | `Dimens::GAP_MIN` | **16 px** | 总览带两卡之间（`504 + 16 = 520`） |
| 总览带 ↔ 滚动区 | `Dimens::GAP_GROUP` | **16 px** | y244 → y260 |
| 滚动区 ↔ 固定操作条 | `Dimens::GAP_SECTION` | **24 px** | y600 → y624 |
| 滚动区各分组卡之间 | `Dimens::GAP_GROUP` | **16 px** | §B / A1–A4 之间 |
| **火警等级卡 ↔ 释放联锁（危险按钮）** | `Dimens::GAP_DANGER` | **≥48 px** | 横向：火警卡（x520–1008）与危险按钮（x16–336 / x384–704）**x 区间不相交**（最小间距 = 384 − ? 二者纵向相隔 ≥380 px，取纵向计）；纵向：火警卡底 y244 与操作条顶 y624 相距 **380 px** ⇒ 远超 48 px ✓ |
| **新增只读控件（下钻「收起」y260–308）↔ 危险按钮** | `Dimens::GAP_DANGER` | **≥48 px** | 纵向 **316 px** ⇒ ✓ |
| 分页「上一页 ↔ 下一页」 | `Dimens::GAP_MIN` | **16 px** | 相邻**独立**可点控件（实测 `136 → 152`） |
| 新增只读控件的触摸目标 | `Dimens::TOUCH_MIN` | **≥48×48 px** | 分页「上一页 / 下一页 / 收起」、A1 逐位行（不可点则非触摸目标） |
| 既有写操作按钮 | `Dimens::TOUCH_CRITICAL` | **≥64×64 px** | 「人工释放联锁」/「M1 授权重启」（**级别不变**，F17.1） |

---

### 15.5 显示面（二）：P6「装置与外设」页（F20 / F22 / F23 / F24 + F8）

**页面改名依据**：T-8 裁定——P6 由「系统 / 关于页」**改名为「装置与外设」**（F8 装置信息 + 外设只读数值同属"只读、非处置"类信息）。**页数不增。**

#### 15.5.1 页面结构（分段控件 + 段内滚动 + 下钻）

```
┌ 页眉（既有）
├ 【分段控件 SegmentedTabs】单选，5 段（每段 185×48 px；**段间隙 16 px**）：
│     [ 装置 ] [ 空调 ] [ 电池 ] [ 储能表 ] [ PCS ]
│     几何：5×185 + 4×16 = 989 ≤ 992（内容区宽，余 3 px 不收尾） ✓
└ 【段内容】（LV_DIR_VER 滚动容器；主读数区不横滚）
     段「装置」  = ① 外设站在线状态表  ② F8 既有六行（**顺序与文案逐字不变**）
     段「空调」  = F20（§15.5.2）
     段「电池」  = F22（§15.5.2）
     段「储能表」= F23（§15.5.2）
     段「PCS」   = F24（§15.5.2）
```

> ⚠️ **控件选型（F14.2 的 16 px 段间距）——依据是「命中区外扩」，不是「无段间距 API」（订正 v2.1-r2）**：
>
> ① **`lv_buttonmatrix` 本身**有**段间距 API**：`update_map()` 读 `pad_column`，并以 `btn * pcol` 偏移各分段（`mupc/vendor/lvgl`，pin **v9.5.0**；`src/widgets/buttonmatrix/lv_buttonmatrix.c:1024-1029` / `:1059` / `:1067-1068`）⇒ 取 **`pad_column = 16` 即得 16 px 视觉隙**，与本节 `SegmentedTabs` 的 `5×185 + 4×16 = 989` **几何等价**。**v2.1-r2 写的"无段间距 API / 分段填满各自 cell / 首尾相接"不实。**
>
> ② **真正不达标的是「触摸目标间距」**：同文件的分段命中测试把每个分段的命中区**按 `pcol = (pcol / 2) + 1 + (pcol & 1)` 向两侧各扩张一次**（`:892-935`，注释原文 "Button look larger with this value. (+1 for rounding error)"），扩张量上限 `BTN_EXTRA_CLICK_AREA_MAX = LV_DPI_DEF / 10`（`:29` = **13 px**）。⇒ **`pad_column = 16` 时每侧外扩 9 px：16 px 视觉隙被 2×9 = 18 px 命中扩张吃掉 ⇒ 净距 −2 px**（两段命中区**重叠**）。⇒ 净距 = `pad_column − 2 × min((pad_column / 2) + 1 + (pad_column & 1), 13)`：`pad_column = 16` 时 = **−2 px**，且 **`pad_column ≤ 24` 恒为 −2 px**（25 / 26 时 −1 / 0 px），此后净距 = `pad_column − 26`，**直到 `pad_column ≥ 42` 才首次达到 16 px**（42→16 px），届时视觉几何（42 px 隙）已不是 16 px 规格。⇒ **`lv_buttonmatrix` 结构性不满足** PRD F14.2「相邻可点控件触摸目标间距 ≥ 16 px」。
>
> ③ ⇒ P6 的 5 段控件**不使用 `lv_buttonmatrix`**（**结论与 v2.1-r2 相同，理由换成 ②**），改用 **UI 设计文档 §5.1 #21 `SegmentedTabs`**：Flex 行容器 + 5× `lv_button`（`LV_OBJ_FLAG_CHECKABLE`）+ `lv_group` 单选，**`style_pad_column = Dimens::SEG_GAP = 16`**。`lv_button` 的命中区**即其自身边界**（无外扩）⇒ **净距 = 16 px 精确成立**。
>
> ④ **R-44 裁定（本版给出，全文见 §15.9）**：`SegmentedTabs` 与 §5.1 #4 `SegmentedControl`（P3 / P5 的 ≤3 段筛选）**不合并为同一实现**——两者在 F14.2 上**不等价**（理由即 ②），**不存在"改用既有控件即可达标"的路径**；**但合并同一视觉规格真源**（`style_seg_*` + §5.2 的 `SegmentedControl` 行，**不新增色值**）。**新增使用点一律 `SegmentedTabs`**；P3 / P5 是否迁移属**独立动作**（改的是已 `[DESIGN_APPROVED]` 的页面）⇒ **本节不改其页面**。
>
> **尺寸与字号（R-36 已定稿，见 UI §6.6.1）**：段高 48 px（`TOUCH_MIN` 下限的 1.0×）、段字 26 px（`FontSize::S26`）、选中态底 `#1E4E8C` + 描边 `#4EA6FF` + 字 `#F4F7FF`（取 UI §5.2 `SegmentedControl` 行，**不新增色值**）；未选中底 `#24334F` 描边 `#3B4A6B` 字 `#A6B6D6`。分段控件与段内容之间留 `GAP_GROUP = 16 px`。

**段内数据行统一呈现规格**（受 `theme.rs` 单一真源约束，页面内不硬编码色值 / 尺寸）：

| 行类型 | 呈现 | 字号 | 降级 |
|--------|------|------|------|
| 站点状态条（每段顶部） | 「站 `<role 中文名>` │ 在线 / **站离线** │ 最后成功 12:03:44 │ 最近更新 12:03:46」 | 24 px | 离线 ⇒ 状态条警示色；**段内全部行** `--` +「站离线」 |
| 数值行 | `标签` + 值 + 单位（**小数位取自 catalog `decimals`**） | 标签 28 px / 值 **32 px** | `--` + 三语义之一（§15.2.3） |
| 位行 | `位名` + 活跃 / 非活跃（LED + 文字，**不只靠颜色**） | 24 px | 整站离线 ⇒ `--` +「站离线」 |
| 枚举行 | `标签` + 文案 | 32 px | 表外值 ⇒ 「未知」 |

**遍历顺序固定**：站按配置顺序、块按配置顺序、点按 `at` 升序 —— **字段位置稳定不跳变**（PRD §4.2.4：值变化不改布局）。

#### 15.5.2 P6 各段字段表（**短标签 + 点名 + 单位 + 小数位**；点名与量纲已逐条按登记/配置复核）

> **口径（两条数不得混用）**：
> ① **`点名` 列 = 帧内键 = catalog 键**（白名单）；`单位 / 小数位` 由 catalog 给出（`decimals` 由登记 `scale` 派生，§15.3.2）；**短标签**来自 `display-proto` 的短标签表（§15.7 / D22），**不是** `point_table` 的登记 `label`（后者是含全角括号的登记说明文本，不上屏）。
> ② **点数一律按"白名单"计**（下表逐条相加 = §15.2.4 的容量分子）：`hvac` 33 / `fire` 13 + 6×(n−1) / `bms` 330 / `meter_batt` 38 / `pcs` **27**。**登记点数**（34 / 127 / 345 / 40 / 72 = 618，N-7）只作对照，**不得**用作容量分子（v2.1-r1 的 PCS「≈55」既非登记数也非白名单数，是自造数——**已删**）。

**段「装置」**

| 分组键 | 分组标题 | 内容 |
|--------|----------|------|
| `station_status` | 外设站状态 | 5 行站状态条（`hvac` / `fire` / `bms` / `meter_batt` / `pcs`）：站 id │ role 中文名 │ **在线 / 站离线** │ **最后成功时刻** │ 最近更新（F25.4 的**结构化字段**，非告警文本；EX-28）。**字段来源**：在线 ← `latest_values::station_is_active()`（**已存在**）；最后成功时刻 ← `station_last_poll_ms()`（**新增接口要求 R-38**；未落地 ⇒ 该列显 `--`，**不臆造、不由本侧自维护**）；最近更新 ← 各块 `ts_ms`。**版面**：行高 48、字号 24 px、同一行四段（UI §6.6.1）；**行数与在线状态无关**（离线行**不消失、不折叠**，F25.5） |
| `device_info` | 装置信息 | F8 既有六行（固件版本 / 编译时间 / 装置型号 / 序列号 / 本机服务地址（仅回环）/ 设备管理 IP），**顺序、文案与 §6.6 逐字一致** |

**段「空调」（F20；站 `hvac`，`interval_ms = 5000`）**

| 分组键 | 分组标题 | 显示项（短标签） | 点名（键） | 单位 | 小数位 |
|--------|----------|------------------|-----------|------|--------|
| `hvac_measure` | 测量值 | 柜内温度 / 内盘管温度 / 柜内湿度 | `hvac_in_1` / `hvac_in_3` / `hvac_in_4` | `℃` / `℃` / `%RH` | 1 / 1 / 1 |
| `hvac_run` | 运行状态 | 系统运行 | `hvac_di_8`（位 7） | — | — （「停止 / 运行」文字 + 语义色 + 图标，EX-06） |
| `hvac_alarm` | 告警位（20） | 见下清单 | `hvac_di_{9..24, 26..29}` → 键 `hvac_di_10`… | — | — （**逐位**，位名 + 活跃/非活跃；**不得只给汇总**，EX-07） |
| `hvac_state` | 辅助状态位（10） | 内风机 / 应急风机 / 制冷 / 加热 / 制冷除湿 / 加热除湿 / 自检 / 报警继电器输出 / 循环模式 | `hvac_di_1..7`、`hvac_di_9`、`hvac_di_31` | — | — |
| —（不上屏） | — | `hvac_di_26`（**位 25 保留位**） | — | — | 白名单外 ⇒ **结构性不上屏**（`BitClass::Reserved`，`point_table.rs:858`） |

**HVAC 20 个告警位（逐位可见清单，短标签）**：柜内温感故障 / 柜内高温 / 柜内低温 / 柜外温感故障 / 柜外高温 / 柜外低温 / 柜内湿感故障 / 柜内高湿 / 柜内低湿 / 柜外湿感故障 / 柜外高湿 / 柜外低湿 / 压缩机高压 / 压缩机低压 / 制冷失效 / 制热失效 / 内盘管温感故障 / 内盘管低温 / 三相电报警 / 接管回风温差。
> 依据 `point_table.rs:842-862`（`BitClass::Alarm` 20 条，位 9–24 + 26–29）。
> ⚠️ **防臆造（硬约束）**：其中「柜外…」六项**只是告警位名称**，**屏上不得出现「柜外温度 / 柜外湿度」的数值字段**——点表**只登记 3 个 HVAC 标量**（柜内温度 / 内盘管温度 / 柜内湿度，`point_table.rs:829-831`），**无柜外温湿度测量值**（F20.4 / EX-08；柜外温湿度数值须厂方补点，PRD P-3 / T-10）。

**段「电池」（F22；站 `bms`，`interval_ms = 1000`）**

| 分组键 | 分组标题 | 显示项（短标签） | 点名（键） | 单位 | 小数位 |
|--------|----------|------------------|-----------|------|--------|
| `bms_core` | 簇组核心量 | 簇组电压 / 簇组电流 / 簇组模块温度 | `bms_io_16` / `bms_io_17` / `bms_io_18` | `V` / `A` / `℃` | 1 / 1 / 0 |
| `bms_health` | 健康度 | SOH / SOE / 绝缘电阻 | `bms_io_20` / `bms_meta_3` / `bms_io_21` | `%` / `%` / `kΩ` | 0 / 0 / 0 |
| `bms_cell_extreme` | 单体极值 | 平均 / 最高 / 最低单体电压 + **各自对应点**；平均 / 最高 / 最低单体温度 + **各自对应点** | `bms_io_22/24/26` + `_25`/`_27`；`bms_io_23/28/30` + `_29`/`_31` | `V` / `℃` | **3** / 0（对应点为整字，**无单位、0 位**） |
| `bms_delta` | 极差 | 单体温度极差 / 单体电压极差 / 最高单体温升 | `bms_meta_4` / `bms_meta_5` / `bms_energy_13` | `℃` / `V` / `℃` | 0 / **3** / 0 |
| `bms_power` | 功率 | 簇实时充放电功率 / 允许充电最大功率 / 允许放电最大功率 | `bms_meta_6` / `bms_io_2` / `bms_io_3` | `kW` | 1 / 1 / 1 |
| `bms_term` | 端子温度 | 端子温度 001–004 | `bms_term_1`–`_4` | `℃` | 0（**采集侧已消解 −40 偏移**，屏侧不重算，F22.4 / EX-17） |
| `bms_energy` | 累计量 | 累计充电电量 / 累计放电电量 / 单次累计充电 / 单次累计放电 / 可充电量 / 可放电量 | `bms_energy_1` / `_3` / `_5` / `_7` / `_9` / `_11` | `kWh` | 1（**累计量原值直接展示**，不做差分，F23.3 同口径） |
| `bms_energy`（续） | 累计量 | 累计充电容量 / 累计放电容量 / 单次累计充电容量 / 单次累计放电容量 | `bms_cap_1` / `_3` / `_5` / `_6` | `Ah` | **0**（登记 `scale = 1.0`，`point_table.rs:382-385`）——⚠️ 与 PRD F22 声明的"1 位"不符，见 §15.9 **R-31** |
| `bms_pole_temp` | 极柱温度 | 最高单体极柱温度 / 最低单体极柱温度 | `bms_energy_17` / `_19` | `℃` | 0 |
| `bms_device` | 装置信息 | 主控程序版本号 / 从控数量 / PACK 组压最高 / PACK 组压最低 | `bms_meta_1` / `_2` / `_7` / `_9` | — / 个 / `V` / `V` | 0 / 0 / 1 / 1 |
| `bms_alarm` | 告警位（288） | ① 「活跃告警位 **x** 个」 ② **活跃位清单**（按位号升序，短标签 + 位号） ③ 「查看全部 288 位」按钮 ⇒ `BmsAlarmPage` 分页（50/页，窗口化列表） | `bms_alarm_{at}`（at = 位地址 − 199，位 200–487） | — | — |

**`bms_alarm` 的三态文案（**必须互不相同**，EDGE-24 / EX-16）**：

| 情形 | 判定 | 屏上文案 |
|------|------|----------|
| 站在线 ∧ 块存在 ∧ 活跃数 = 0 | `available = true` ∧ `active_total = 0` | 「**无活跃告警位**」 |
| 站在线 ∧ 块**不存在**（现场配置未含 `bms_alarm`） | `available = false` | 「**BMS 告警源不可用**」 |
| 站离线 | `station.online = false`（**优先级最高**） | 「**站离线**」（EDGE-18） |

> 三态由**组帧侧**给出（`available` 字段），屏侧**不重判**；三条文案字符串**两两不相等**（测试断言，§15.8 T-14）。

**段「储能表」（F23；站 `meter_batt`，`interval_ms = 1000`）**

| 分组键 | 分组标题 | 显示项（短标签，**全部以「储能表·」开头**） | 点名（键） | 单位 | 小数位 |
|--------|----------|---------------------------------------------|-----------|------|--------|
| `mb_u_i` | 电压电流 | 储能表·A/B/C 相电压；储能表·A/B/C 相电流；储能表·线电压 A-B / C-B / A-C | `mb_ui_1/2/3`；`mb_ui_4/5/6`；`mb_freq_line_2/3/4` | `V` / `A` / `V` | 1 / **2** / 1 |
| `mb_freq` | 频率 | 储能表·频率 | `mb_freq_line_1` | `Hz` | **2** |
| `mb_power` | 功率 | 储能表·A/B/C/总 有功；A/B/C/总 无功；A/B/C/总 视在 | `mb_power_1/3/5/7`；`_9/_11/_13/_15`；`_17/_19/_21/_23` | `kW` / `kvar` / `kVA` | **3** / **3** / **3** |
| `mb_pf` | 功率因数 | 储能表·A/B/C/总 功率因数 | `mb_power_25`–`_28` | — | **3** |
| `mb_energy` | 电能（累计量） | 储能表·组合 / 正向 / 反向有功总电能；组合 / 正向 / 反向无功总电能；储能表·分相正向有功电能 A/B/C | `mb_e_act_comb_1` / `mb_e_act_fwd_1` / `mb_e_act_rev_1`；`mb_e_rea_comb_1` / `mb_e_rea_fwd_1` / `mb_e_rea_rev_1`；`mb_phase_1/3/5` | `kWh` / `kvarh` / `kWh` | **2**（**原值直接展示**，不做日增量差分，EX-20） |
| `mb_quality` | 质量指标 | 储能表·零序电流；储能表·电压不平衡度；储能表·电流不平衡度 | `mb_phase_12`；`mb_phase_13`；`mb_phase_14` | `A` / `%` / `%` | **2** / 1 / 1 |
| —（不上屏） | — | PT / CT 只读对照（`mb_phase_7` / `_8`） | — | — | **白名单外 ⇒ 结构性不上屏**（非本增量展示项） |

**误用防护（硬约束，可与测试机械对齐）**：① 本段**只列上述白名单键**；② 屏上**不存在**任何以「台区 / 关口 / 总表」命名且取自 `meter_batt` 的字段（标签由 catalog 给定，含「台区/关口/总表」的键**不在白名单**⇒ 结构性成立，EX-21）；③ 每行**标签前缀「储能表·」**由 catalog 提供（屏侧不拼接）；④ 台区关口总表数值**不在本增量内**（U-69 / U-74）。

**段「PCS」（F24；站 `pcs`；真源 = **`intercore` 3 区读通道**，见 §15.1.3）**

| 分组键 | 分组标题 | 显示项（短标签） | 点名（键） | 单位 | 小数位 |
|--------|----------|------------------|-----------|------|--------|
| `pcs_ac_u_f` | 交流电压频率 | 电网 A/B/C 相电压；交流母线频率 | `pcs_3zone_19/20/21`；`_22` | `V` / `Hz` | 1 / **2** |
| `pcs_ac_power` | 交流功率 | 视在 A/B/C/总；无功 A/B/C/总；功率因数 A/B/C/总 | `pcs_3zone_26`–`_29`；`_34`–`_37`；`_38`–`_41` | `kVA` / `kvar` / — | 1 / 1 / **3** |
| `pcs_dc` | 直流侧 | BMS 系统总电压 / 系统总电流 / 直流母线电压 / 直流中点电压 | `pcs_3zone_9` / `_10` / `_12` / `_13` | `V` / `A` / `V` / `V` | 1 |
| `pcs_temp` | 温度 | PCS 温度 / DCDC 温度 | `pcs_3zone_42` / `_72` | `℃` | 0 |
| `pcs_energy` | 累计电量 | 交流累计充电 / 放电电量；直流累计充电 / 放电电量 | `pcs_3zone_43` / `_45` / `_73` / `_75` | `kWh` | 1 |
| `pcs_mode` | 工作模式 | 工作模式 | `pcs_3zone_67` | — | 枚举文案未登记 ⇒ 「模式 `<值>`」+ 旁注「（枚举文案待厂方追认）」，**不臆造**（§15.9 R-32） |
| —（**明确排除**） | — | **1046–1065 全部**（`pcs_3zone_47`–`_66`：STS 网侧电压 1046–1048 / **STS 电网电压幅值 1049 = `pcs_3zone_50`** / STS 电网电压频率 1050 = `_51` / 负载电流·视在·有功·无功·PF 1051–1065） | — | — | **白名单外 ⇒ 结构性不上屏**（PRD §7 #11 / F24 排除段 / EX-23）。**不得**标注为「PCS 输出 / 充放功率 / 台区负荷」 |

**PCS 白名单点数 = 27**（不是 v2.1-r1 写的 55；登记数 72，差 45 = 高半字 4（1043/1045/1073/1075）+ 1046–1065 排除 20 + 其余未列入 21）。

> ⚠️ **PRD 自身冲突（登记 R-42，待产品裁定；本设计取排除侧）**：
> **PRD F24 的"展示内容"表把 1049（STS 电网电压幅值）列入**（PRD `:757`，原文「电网 A/B/C 相电压（1018–1020）、**STS 电网电压幅值（1049）**、交流母线频率（1021）」），而 **同一 PRD 的 §7 范围外清单第 11 条把 1046–1065 整段排除**（PRD `:1042`，原文「**PCS 点表的 STS / 负载区（1046–1065）上屏**：不在本期」）⇒ **PRD 内部自相矛盾**。
>
> **本设计的处置**：**取排除侧**（`_50` 不进白名单），依据 = **PRD §7 #11（`:1042`）的范围外声明**——这是"**该键不上屏**"的**唯一**权威依据。
>
> **⚠️ 订正 v2.1-r2（依据错引，本版改正）**：该版把依据写成 **EX-23 / F24 验收 2**（PRD `:1279` / `:773`）——**不实**。那两处原文是「屏上**不存在**任何取自 1046–1065（STS / 负载区）的字段**被标注为**『PCS 输出 / 充放功率 / 台区负荷』」，即**误用禁止**（不得把 STS / 负载区数据**当成** PCS 输出展示），**不含"键缺失"要求**：一个上屏但**正确标注**为「STS 电网电压幅值」的 `_50` **并不违反** EX-23。⇒ 排除依据**只能是 §7 #11**。
>
> 两侧选项：
> - **选项 A（本节采用）**：`_50` 不上屏（依据 = PRD §7 #11）⇒ **§7 #11 成立**；代价 = **F24 验收 1 的"上表各量常显"对 1049 一项不达成**。
> - **选项 B**：`_50` 上屏（按 F24 展示表）⇒ 只需**回写 PRD §7 #11**（1046–1065 改为「**除 1049 外**」）；**EX-23 无需改动**——把 `_50` 正确标注为「STS 电网电压幅值」本就合规（T-19 亦只断言"取自 1046–1065 **且被标注为** PCS 输出 / 充放 / 台区负荷"的字段不存在）。⇒ 需求回写量比 v2.1-r2 所述**更小**；仍属**需求回写**，非设计可自行决定。
>
> **为什么 `_51`（1050 STS 电网电压频率）没有同样的问题**：F24 展示表**未**列它，排除段与 §7 #11 一致排除 ⇒ 无冲突。

> **既有 F2 / F3 / F4 仍在 P1 主状态页，落点不变**（F24 是其**扩展读数**）；本段与之**数值同源不同键**（P1 用帧顶层 `run_state` / `p_phase` 等，本段用 `pcs_3zone_*`）——两者**不得互相"纠偏"或合并展示**（避免引入第二套判据）。

#### 15.5.3 导航与触摸：**不新增页面层级**（F11 / TT-01 口径核对）

| 判据 | 结论 | 机械断言（§15.8 T-19 / T-20） |
|------|------|------------------------------|
| 页数 = **6** | **不变**：P6 仅**改名**（「装置与外设」） | 导航栏项数 == 6；页面注册表长度 == 6 |
| 任意页 → 任意页 **≤2 次触摸** | **不变**：新增内容全在既有页内 | 既有导航用例（§11.1 交互层）原样跑通 |
| **不新增页面层级** | **成立**：P6 的**分段控件**（5 段）与 P4/P6 的**下钻视图**（探测器分页 / BMS 288 位）**都不是页面**——无独立页面状态、不改 `current_page`、不经导航路由 | 断言：切换分段 / 进入下钻后 `current_page` **仍为 P6**（P4 亦然）；且**不允许**为其新增路由项 |
| 「子页有返回」（F11.3） | 下钻视图提供**同位置固定「收起」控件**（`≥48×48 px`，**位于下钻视图顶部右端**，与进入点同区）；因它不构成子页，故**不要求**"返回"语义，但**必须**可退出（不得困住用户）。**三条下钻的返回路径逐条明确**：① 探测器明细（P4 A4）→「收起」回汇总行；② BMS 288 位（P6 段「电池」）→「收起」回 `bms_alarm` 摘要；③ 两者均可再由**底部导航**直达任一页（≤1 次触摸） | 交互用例：进下钻 → 「收起」→ 回列表；逆向「进入 → 导航切走 → 再切回 P6」不残留下钻态 |
| 触摸目标（TT-08） | 分段每段 **185×48 px**（≥48×48 ✓）且**段间隙 16 px**（F14.2，见 §15.5.1 ② 的**命中区外扩**）、分页按钮 ≥48×48 px、`P4` 既有写操作仍 ≥64×64 px（关键操作，**不改级别**） | `theme.rs` 常量引用检查（§11.1 静态约束 ④）；新增常量 `Dimens::SEG_GAP = 16` |
| 无 AI 类入口（F11.5） | 不变 | 既有断言 |

**P6 容器策略（承 §5.4 的 `lv_tabview` / 自建二选一）**：段内容采用 **`SegmentedTabs`（Flex + `lv_button` 单选组，见 §15.5.1）+ 各段独立滚动容器**（**不用 `lv_buttonmatrix`**，理由 = F14.2 的 16 px 触摸目标间距，见 §15.5.1 ②）；**分段控件惰性创建、切换仅改可见性**（不销毁重建，避免 LVGL 内存池碎片）。**每段列表一律窗口化**（可视行 ×1.5，沿用 §5.7 / R-22 范式）——理由：P6 全量展开 **428 行 + 装置段 11 行**（白名单口径 = §15.2.4 表：`hvac` 33 + `bms` 330 + `meter_batt` 38 + `pcs` 27 = **428**；装置段 = 站状态条 5 行 + F8 六行 = 11），**单段最大 = 段「电池」330 行**（其中 `bms_alarm` 288 位走下钻，段内实际渲染 ≈44 行）⇒ 若逐行建对象会冲击 `LV_MEM_SIZE = 1 MB`（R-24 实测口径）。落地后的余量须真机复核（§15.9 **R-34**）。
>
> ⚠️ **订正 v2.1-r2（两处）**：① 该版写「P6 全量展开 **555 行**」——**555 含 `fire` 127，而消防落 P4（§15.4）**，P6 应为 **428 行 + 装置段 11 行**；② 该版写「n=100 时 1035 行」——**P6 与 n 无关**：唯一随 n 增长的块是 `fire_det`，而它在 **P4**（P4 侧 n=100 时 = 13 + 6×99 = **607 行**）。**"≤ n=100 不裁"的容量结论不受影响**（它针对整帧，§15.2.4）。

---

### 15.6 刷新节拍、降级与性能

#### 15.6.1 刷新节拍与上屏时延拆解

**承载方式**：外设变更**不新开节拍**——复用既有「**主拍 1000 ms ∪ 内容变更即组帧**」（§4.2 / §4.2.1），新增的只有慢拍 D 的**触发源**（`latest_values` 变更**广播** ∪ `periph_poll_ms` 兜底，§15.1.1）。

链路五段：**① 站采集完成 → ② 入口（`latest_values` → 慢拍 D 取样）→ ③ 段重建 → ④ 组帧（含发布）→ ⑤ HMI 轮询 + 渲染**。

| 指标（验收 ID） | ① 站周期 | ② 入口 | ③ 段重建 | ④ 组帧 | ⑤ HMI 轮询 + 渲染 | **最坏合计** | 目标 |
|-----------------|----------|--------|----------|--------|-------------------|--------------|------|
| **外设量（口径 A：采集完成 → **帧发布**）** | HVAC **5.0 s** / BMS·消防·储能表 **1.0 s** / PCS **1.0 s** | **≤0.5 s**（兜底；走广播时 ≈0） | ≤1 ms | **≤0.25 s**（合并窗口 `MIN_MERGE_WINDOW_MS = 250`） | — | 站周期 + **0.75 s** | **≤ 站周期 + 1 s**（PRD F25 验收 3 · 分句①）⇒ 余量 **0.25 s** |
| **外设量（口径 B：含屏侧链路，与 §4.2.1 同口径）** | 同上 | ≤0.5 s | ≤1 ms | ≤0.25 s | **≤0.6 s**（轮询 0.5 + 渲染 0.1） | 站周期 + **1.35 s** | PRD **未**为该口径定值（登记备查，见 §15.9 **R-33** 的口径澄清） |
| **离散告警位（变化后）** | BMS·消防·储能表 **1.0 s** | ≤0.5 s | ≤1 ms | ≤0.25 s | ≤0.6 s | **≤2.35 s** | ≤2 s（PRD F25 验收 3 · 分句②）⇒ **两段可达性不同**，见约束 1 |
| **站离线态（F25.4；PRD §4.5 `:932`「变化后 ≤2 s 上屏」）** | **≥ `stale_timeout_s` = 5 s**（**判定下界，物理不可提前**：`station_is_active()` 须"最后一次成功轮询后满 5 s"才给出「站离线」结论） | ≤0.5 s | ≤1 ms | ≤0.25 s | ≤0.6 s | **5 s + 1.35 s = 6.35 s** | **口径 A（结论成立 → 屏上可见）= `0.5 + 0.001 + 0.25 + 0.6 = 1.35 s ≤ 2 s` ⇒ 达标**；**口径 B（端到端）= 6.35 s ⇒ 不达标**（依赖 **R-45**，见约束 3） |

**三条落地约束（关键性同 §4.2.1 的六条）**：

1. **离散告警位的 ≤2 s —— 不得压在"通知必达"上（本节的核心订正）**。
   - **事实**：`latest_values` 的通知是 **`broadcast`（容量 64）**，**落后即丢**（`RecvError::Lagged`；01 §9.1.2 / §9.1.3 / §9.1.5，见 N-18）。**没有任何"必达"保证**。v2.1-r1 写「`Notify` / `watch` 通知必达」是**错的**。
   - **正确表述**：**通知路径**（②≈0）下 `1.0 + 0.25 + 0.5 + 0.1 = 1.85 s` **达标**；**广播丢失时由兜底 tick 收敛**，收敛上界 = `站周期 + periph_poll_ms + 0.25 + 0.5 + 0.1` = **`1.0 + 0.5 + 0.85 = 2.35 s`（超差 0.35 s）**——与 §4.2.1 约束 4 的失效算式**同构**。
   - **两条可选口径（本设计不自行改需求，登记 R-33 一并裁定）**：
     - **选项 A（本设计默认配置）**：`periph_poll_ms = 500` ⇒ 只保证**通知路径**达标；**丢帧窗口内最坏 2.35 s**。
     - **选项 B（要"任何丢帧都 ≤2 s"）**：`periph_poll_ms ≤ 2.0 − 1.0 − 0.85 = 0.15 s` ⇒ 取 **`periph_poll_ms = 100`**（`validate()` 的 `∈[1,1000]` **已覆盖，零代码改动**，仅改配置）。代价 = 取样频率 ×5 ⇒ 实测成本**很小**：白名单 555 点 / 次取样（内存读 + 组装）≈ **≤0.5 ms**（§15.6.3）⇒ 10 Hz 仅 **≤0.5 % 单核**，**§4.1 的 40 % 不破**。
   - ⇒ **本设计不再声称"通知是必达路径"**；`latest_values` 的接口**无需为此改动**（`broadcast` 已够用，丢失由兜底 tick 承担）。
2. **HVAC 的"告警位变化 ≤2 s" 物理不可达**：HVAC 站周期 **5000 ms**（`production.yaml:399`）⇒ 一个位的跳变**最早**在 5 s 后才被采到。故 `PRD F25 验收 3 · 分句②` 与 `F20 / §4.5` 行的"站周期 5 s + 告警位变化 ≤2 s"**不可同时成立** ⇒ **报告产品裁定**（§15.9 **R-33**）。**本设计不自行改需求**：屏侧行为按"站周期 + 1 帧节拍"实现，HVAC 的 ≤2 s 口径待裁。
3. **F25.4「站离线变化 ≤2 s 上屏」的时延落点与依赖项（R-45）** —— v2.1-r2 **漏给落点**，本版补齐。
   - **判定侧下界（物理，非设计可改）**：站离线结论由 `latest_values::station_is_active(station, now_ms)` 给出，阈值真源 = `south_stations.stale_timeout_s` = **5 s**（`production.yaml:144`）。**没有任何"快速判定"能在 5 s 之前给出「站离线」结论**——判据本身要求"超过 5 s 无成功轮询"。
   - **落点（本设计可保证，机械可断言）**：**口径 A =「站离线结论成立 → 屏上可见」= ② 0.5 + ③ 0.001 + ④ 0.25 + ⑤ 0.6 = 1.35 s ≤ 2 s ⇒ 达标**。**四段**定值（②–⑤）即上表第 2–5 列，**不引入任何新阈值**（结论由 `station_is_active()` 驱动）。断言见 §15.8 **T-22**。
   - **口径 B（端到端 = 最后一次成功轮询 → 屏上可见）= `5 s + 1.35 s = 6.35 s` ⇒ 不满足 2 s**（如实登记，不粉饰）。
   - **⇒ 依赖项（登记 R-45；**尚未落地，本设计不假定其存在**）**：用户裁定的解法是 **「告警位 / 站活性单独快采」**——由 **02 号南向通信**提供一条**独立于站轮询周期**的短周期采样通道，使"离线 / 告警位变化"的判定**不必等 `stale_timeout_s` 的粗粒度轮询**。**该能力属 02 号新需求**；本设计**只登记依赖，不登记其接口签名、不新增任何"假定已存在"的接口**。
   - **未落地时的降级口径（本设计默认执行）**：① 屏侧与组帧侧**一律不得**为此新造第二套离线判据（**禁止** `now − last_ok_ms > 2 s` 形式——那会与 `stale_timeout_s` 构成双真源，违反 F25.1「沿用 F5 基线，不新造第二套新鲜度口径」）；② 站离线态按**口径 A（1.35 s）达标**、**口径 B（6.35 s）如实登记为不达标**；③ 该不达标**不阻塞**本次上屏实现——屏侧行为与 R-45 是否落地**解耦**：02 号落地后判定侧变快 ⇒ 口径 B 自动收敛，**屏侧零改动**。

**验证**：`display_host` 单测以**假时钟**推进（不 `sleep`），断言「`latest_values` 写入 → 段缓存更新 → 发布」间隔 < 合并窗口上限；集成测试（stub 服务端）注入一次外设值变化，断言 ≤1.5 s 内新帧含该值；真机以时间戳探针复核（§14 R-05 同法）。

#### 15.6.2 四种情形的屏上表现（逐条）

| # | 情形（PRD ID） | 数据侧（mupcd） | 屏上表现（**唯一文案**） | 不变量 |
|---|----------------|------------------|--------------------------|--------|
| **①** | **外设站离线**（EDGE-18 / F25.4） | `latest_values`：站活性由 `station_is_active()` 给出（阈值真源 `stale_timeout_s` = 5 s）+ `last_ok_ms`（**待 R-38**）；组帧侧把该站**全部**点置 `v=None, flag=Offline` | 段顶状态条：`站 <角色> │ 站离线 │ 最后成功 12:03:44`（警示色 + 图标）；段内**全部**行 `--` + 「**站离线**」；**不保留旧值、不补 0** | 其余站与 PCS 主链路刷新**不受影响**（站级隔离，F25.5 / EX-04）；**通道级**「数据过期」角标**不因**站离线而点亮（两者语义不同，§15.2.3）；**上屏时延口径见 §15.6.1 约束 3**（口径 A ≤2 s 达标 / 口径 B 6.35 s 待 **R-45**） |
| **②** | **点未取数 / 值无效**（EDGE-19 / EDGE-20） | `NotRead` / `RangeError`（判定规则见 §15.1.2，**无时间阈值**） | 该行 `--` + 「**未取数**」/「**数据异常**」；**不做插值、不沿用旧值** | 与①③的文案**两两互异**（EX-28 的三语义互异）；`0` 与「不可得」严格区分（不造假值总原则） |
| **③** | **帧版本不匹配**（EDGE-21 / F26.3） | 帧 `version = 3`（单一真源）；两端同版本发布（部署口径） | **≤3 s** 进入「版本不匹配」降级画面：灰底 + 「**屏与主进程版本不匹配：屏 v3 / 主进程 v2，请刷同版本固件**」+ 「最后成功 12:03:44」；**不黑屏、不显示半帧 / 混版帧、不显示任何数值** | 文案与④**字符串不相等**（现场排障需区分：一个刷固件、一个查进程）；`Incompatible` 为**粘性**态，须**一次成功帧**才清除（避免与偶发超时之间抖动） |
| **④** | **通道断开**（EDGE-03） | 读通道端口消失 / 连续失败 > 3 s | 既有整屏降级：暗化 + 「**与主进程数据通道断开（重试中）**」+ 冻结标（可保留最近帧）；外设段随**整屏**进入该态 | **外设段不得**在通道断开时单独显示「站离线」（避免把"通道断"误读为"站离线"）——整屏层**优先于**段级语义 |
| **⑤** | **外设段缺失 / 未采集**（EDGE-22） | `peripherals.available = false`（句柄未接线 / 快照不可得） | P4 消防区与 P6 外设各段显「**外设数据不可用**」（`--`/「未提供」）；**不得**显 0 / 「正常」/「无告警」 | 与「无活跃告警位」互异（EDGE-24 同口径） |
| **⑥** | **明细端点不可用**（`/fire_detectors`、`/bms_alarms` 非 2xx） | — | 下钻视图显「**明细不可用（`<message>`）**」+ 「重试」；汇总行仍按帧内数据展示 | **不阻断**实时值显示（值走读通道，与明细端点解耦） |

**HMI 侧的改动落点（§15.11 逐文件）**：

```rust
// mupc/crates/local-display/src/state.rs
/// ⚠️ **既有枚举的现状是 3 态**（`Init / Connected / Down`，`state.rs:45-52`），
/// **`Stale` 不在其内**（属 `Freshness`，`state.rs:56-59`）。本增量**只增 1 个变体**：
pub enum ChannelStatus {
    Init, Connected, Down,                   // 既有 3 态，语义不动（N-12）
    /// 帧版本不匹配（EDGE-21 / F26.3）。`got` = 帧内版本，`expected` = 本地 PROTO_VERSION。
    /// 归因来源 = `Error::ProtoVersion(url, got, expected)`（`error.rs:59`；产生点 `channel.rs:455-459`）。
    Incompatible { got: u8, expected: u8 },
}
pub enum ScreenMode { ChannelDown, Init, Live, VersionMismatch { got: u8, expected: u8 } }

/// 三态归一 + 新增 reason（**「站离线 / 未取数 / 数据异常」三语义互异**）。
pub enum MissingReason {
    StationOffline,   // 「站离线」
    NotRead,          // 「未取数」
    RangeError,       // 「数据异常」
    NotConfigured,    // 「未配置」（仅消防钢瓶气压；来自 station.cylinder_configured == Some(false)）
    NameUnavailable,  // 「名称未获取」（catalog 未取到；**不臆造中文名**）
    DetailUnavailable,// 「明细不可用」（下钻端点失败）
}
```

#### 15.6.3 性能：新增字段对组帧耗时与回环载荷的影响（量级估算）

| 项 | 估算 | 依据 / 结论 |
|----|------|-------------|
| 段重建（慢拍 D 单次） | 5 次 `station_snapshot` 读（内存，每次单次持读锁克隆）+ ≈**555**（n=20）/**1035**（n=100）个点值组装 ⇒ **≤0.5 ms** | 与 `alarm` 慢拍同量级；**无 I/O**。**这是选项 B（`periph_poll_ms = 100`）成本的依据**（§15.6.1 约束 1） |
| 变更比较 | O(点数) 比较，前置 O(1) 短路（先比站数 / 站 `online` 与 `last_ok_ms` / 块数）⇒ **≤0.05 ms** | 500 ms 一次 ⇒ CPU 可忽略 |
| 组帧（`build_frame`） | +1 次段克隆（**≈20.6 KiB** 结构（n=20）/ **≈36.1 KiB**（n=100）+ 555 / 1035 个 `Copy` 点值）⇒ **≤0.3 ms** | 帧路径仍**零 I/O**（N-3 不变量不破） |
| 回环 HTTP 载荷 | 帧 JSON 由 ≈1.6 KiB → **≈22.2 KiB**（n=20）/ **≈37.7 KiB**（n=100）；1 Hz ⇒ **≈22–38 KiB/s** | 127.0.0.1 回环，**可忽略** |
| HMI 解析（serde_json） | 555–1035 个点值 ⇒ 本机 1–3 ms/帧；真机（Cortex-A55，按 3–8× 外推）**≤25 ms/帧** | 轮询周期 500 ms ⇒ 占用 <5 %，**不阻塞触摸**（F13.4 不破） |
| HMI 刷新 | 只为**当前分段**的可见行更新控件（窗口化列表）⇒ 脏区 ≈ 既有 P1 量级 | 分段切换沿用 §10 的「≤300 ms 全帧重绘」预算 |
| **CPU（稳态）** | 新增 ≤1 % 单核（mupcd 侧 ≤0.5 %，HMI 侧 ≤0.5 %） | **§4.1 的 ≤40 % 单核不破**（EX-33） |
| **内存** | mupcd：段缓存 ≈20.6–36.1 KiB；HMI：catalog（白名单 555 条 + 位语义；`CatalogPoint` 实测 **118–346 B/条** ⇒ **≈64–188 KiB**） + 分段控件（≤~300 obj，窗口化约束） | `LV_MEM_SIZE = 1 MB` 余量**须真机复核**（§15.9 **R-34**） |
| 字体体积 | cmap 由 324 → `324 + G` 码位（`G` = 短标签表定稿后的缺口，§15.7）；按线性外推 10 档位图由 **1.69 MB ⇒ ≈1.7–2.7 MB** | 磁盘预算 ≤100 MB（§10）**不破** |

---

### 15.7 字体门禁（**硬要求**，F26 关联 / §3.9.1 第 4 条 / EX-24）

#### 15.7.1 事实与量化结论

| # | 事实 / 结论 | 证据 |
|---|-------------|------|
| F-1 | 生成字体 cmap 实测 **324 码位**；`font_subset_charset.txt` = 全用字表 **326 字符**（两者由漂移检测绑定） | `fonts/lv_font_cmap.txt` 头行；`ui/tests.rs:1066-1200` / `:1676-1727` |
| F-2 | **PRD 已算的下界 = 41 码位缺口**（`柜火防压湿热盘管烟感探钢瓶高低冷风表量视频零极累绝缘阻项循环子容差平衡等器体空气燃`） | PRD §3.9.1 第 4 条 |
| F-3 | **设计阶段实测**：若**直接把 `point_table` 的登记 `label` 上屏**（in-scope 5 个 role 共 525 条 label），**缺口 = 189 码位**（含 `簇` `极` `循` `障` `雾` `瓶` `馈` `驱` `阀` `闭` 等，以及 `℃`/`（`/`）`/`：`/`；`/`、`） | 本节实测（脚本：读 `lv_font_cmap.txt` 的码位集 ∪ `point_table.rs` 中 `Role ∈ {Hvac,Fire,Battery,MeterBatt,Pcs}` 的 label 字面量，取差集） |
| F-4 | ⇒ **设计裁决（D22）**：**禁止**把登记 `label` 直接上屏；上屏文案一律取自 **`display-proto` 的短标签表白名单**（值 = 精炼短标签） | §15.5.2 口径注 / §15.3.2 |
| F-5 | 既有码表覆盖率测试**只扫 `ui/**` + `state.rs` 的中文字面量**（N-10）；若中文名以**运行时字符串**（帧 / catalog）到达 HMI，该测试**看不到** | `ui/tests.rs:1066-1200` |
| F-6 | ⇒ **设计裁决**：短标签表**放在 `display-proto`**（`local-display` 的**依赖**）⇒ 全部上屏中文回到**可被 `local-display` 静态扫描的源码**内，**消除 F-5 的覆盖盲区**（无需新增跨 crate 扫描） | §15.11 落点 |

#### 15.7.2 硬要求（H-1 ~ H-5，未完成不得通过 P4 / P6 真机验收）

| ID | 要求 | 验证方式 |
|----|------|----------|
| **H-1** | **扩字库**：把「P4/P6 新增全部中文文案」的字符**并集**写入 `fonts/font_subset_charset.txt`，重跑 `fonts/gen_fonts.sh`（10 档），**同批提交** `lv_font_cmap.txt`（漂移检测绑定两者，缺一即红） | 构建机执行；`ui/tests.rs` 的漂移检测通过 |
| **H-2** | **新增静态覆盖率用例**（补 F-5 的盲区）：待查集合 = ① `ui/**` + `state.rs` 既有字面量（既有用例覆盖）∪ ② **`display-proto` 短标签表的全部 `label` / `unit` 字符串** ∪ ③ **UI 固定文案常量表**（§15.7.3 清单）∪ ④ **分组标题（§15.7.3，共 31 项，按字面量——含全角括号 `（` `）` 与数字 `10 / 20 / 288`）**；断言 ⊆ cmap 基线 | `local-display` 单测（`ui/tests.rs` 新增用例），全平台可跑 |
| **H-3** | **短标签表覆盖性**：`display-proto` 单测断言**§15.4 / §15.5 字段表的每一项（按白名单键枚举）都有短标签**（行数相等，防漏项；防"某点上了帧却没有屏上文案"）。**"行数相等"的可写前提 = W-4**：白名单须落为**常量数组** `PERIPH_WHITELIST`（与短标签表**同文件、同批字面量**），断言 = `PERIPH_WHITELIST.len() == LABELS.len()` | `display-proto` 单测（**不需要 cmap**，可独立跑） |
| **H-4** | **真机逐字核对**：`--features noto-font` + `gen_fonts.sh` 产物构建后，在 P4 / P6 逐页核对**豆腐块**（`--backend offscreen` 导出 PNG 逐字核对亦可，但**真机为准**） | QA + 真机（§15.8.2 真机档 D-1） |
| **H-5** | **槽宽门禁**：新增**带符号数值**（如 BMS 簇组电流 `-1600.0`、PCS 有功 `-60.0`）与**长标签**（如「储能表·电压不平衡度」「柜外湿感故障」）**不得越出槽宽**（U-42 同类风险）；字号满足 §4.2（主读数 ≥64 / 过程量 ≥32 / 标注 ≥24） | 真机（§15.8.2 真机档 D-2） |

#### 15.7.3 新增中文文案清单（**UI 固定文案 + 分组标题**，逐条列出；短标签表另见 §15.5.2）

**分组标题（**按字面量 31 个**；`H-2` / `T-23` 的待查集合即本清单，与 §15.4 / §15.5.2 的「分组标题」列**逐字字面量**一一对应——**括号与其中的数字都是字面量的一部分，不得剥掉**）**：

| 页 / 段 | 分组标题（**逐字字面量**，抄自 §15.4 / §15.5.2） | 个数 |
|---------|--------------------------------------------------------|:----:|
| P4（§15.4） | **火警等级** / 消防系统状态 / 灭火瓶压力 / 探测器触发 / 探测器 | 5 |
| P6 段「装置」 | 外设站状态 / 装置信息 | 2 |
| P6 段「空调」 | 测量值 / 运行状态 / **告警位（20）** / **辅助状态位（10）** | 4 |
| P6 段「电池」 | 簇组核心量 / 健康度 / 单体极值 / 极差 / 功率 / 端子温度 / 累计量 / 极柱温度 / 装置信息 / **告警位（288）** | 10 |
| P6 段「储能表」 | 电压电流 / 频率 / 功率 / 功率因数 / **电能（累计量）** / 质量指标 | 6 |
| P6 段「PCS」 | 交流电压频率 / 交流功率 / 直流侧 / 温度 / 累计电量 / 工作模式 | 6 |
| **合计（按字面量）** | — | **33** |
| **去重后** | `装置信息` ×2、`功率` ×2 各计 1；**`告警位（20）` 与 `告警位（288）` 字面量不同，不合并** | **31** |

> ⚠️ **订正（三处，v2.1-r3）**：
> ① 小标题曾写「分组标题（**11 个**）」而实列 **29** 个——数字与清单**不符**（本版改为表格逐段点数）；
> ② 清单曾**漏列 `fire_level` 的标题「火警等级」**（含 `火` 字，而 `火` 在 PRD 的 41 码位缺口清单内 ⇒ **漏了会漏掉一块真实缺口**）⇒ 已补入；
> ③ **v2.1-r2 把字面量里的括号剥掉了**——写成 `告警位` / `辅助状态位` / `电能`（**字段表的原文是 `告警位（20）` / `告警位（288）` / `辅助状态位（10）` / `电能（累计量）`**），并据此把 `告警位` 当作"×2 去重"⇒ 去重后**误算为 30**（**应为 31**）。⇒ 本版按**字面量**重列；**`H-2` / `T-23` 的待查集合必须含全角括号 `（` `）` 与数字 `10 / 20 / 288`**——`（`/`）` **已在 F-3 的 189 码位缺口清单内**（`point_table` 登记 label 里的全角括号），**不补就会在屏上出豆腐块**。**H-2 的待查集合以本表为准。**

**UI 固定文案（新增，全部为字面量常量）**：

| 用途 | 文案 |
|------|------|
| 站位/取值降级 | 站离线 / 未取数 / 数据异常 / 未配置 / 名称未获取 / 明细不可用 / 不可用 |
| 段级降级 | 外设数据不可用 / 消防源不可用 / 无活跃告警位 / BMS 告警源不可用 / 装置与外设 |
| 位语义 | 未定义位 / 预留 / 活跃 / 非活跃 / 在线 / 离线 / 报警总状态 / 故障总状态 / 通信状态 |
| 枚举文案 | 正常 / 一级报警 / 二级火警 / 未定义 / 紧急启动 / 紧急停止 / 未知 / 停止 / 运行 |
| 时刻与提示 | 最后成功 / 最近更新 / 登记 / 可读 / 报警 / 故障 / 名称表可能过期 / 明细超出帧预算已截断至 / 枚举文案待厂方追认 |
| 分页与下钻 | 查看明细 / 查看全部 / 上一页 / 下一页 / 收起 / 重试 / 第 / 页 |
| 版本降级 | 屏与主进程版本不匹配 / 请刷同版本固件 / 与主进程数据通道断开（重试中） |

> **不新增**的文案（刻意）：**负向验收项（"无 STS 误用" / "无混标" / "无柜外温湿度数值"）不配可见声明文案**——EX-08 / EX-21 / EX-23 的判据是「**屏上无该字段**」（测试断言），加一句"本页不含…"只会扩大字库缺口、增加界面噪音（KISS）。

**最终缺口以短标签表 + 上表定稿后重算为准**；设计给出**方法**：`H-2` 用例失败时报出的缺失码位集即最终缺口（用例输出该集合，**便于开发直接补 `font_subset_charset.txt`**）。下界 **41**（PRD）、上界 **189**（若误用登记 label 上屏的实测值）**均不得作为定稿值**。

---

### 15.8 测试策略（本机 / 真机分档）

#### 15.8.1 层与本机可验证（x86_64 默认 feature，无 `noto-font`）

| 层 | 用例 ID | 断言 |
|----|---------|------|
| `display-proto` | T-1 | `PeripheralsSection` JSON 往返（含 `v=None` / 空段 / `renames` / `truncated`）；**`Default` 落在 `available=false`**（EDGE-22 语义）；未知键容忍仍成立（F26.6 回归） |
| `display-proto` | T-2 | **版本协商**：`version=2` 的帧 → `Err(ProtoVersionMismatch{got:2, expected:3})`；`version=4` 同样拒；**既有 4 条版本用例逐条仍绿**（EX-29 / F26.2 不得放宽） |
| `display-proto` | T-3 | **字段缺失不补 0**：段缺失 / `v=null` / `flag != valid` 时**绝不**产出 `v = Some(0.0)`（EX-30） |
| `display-proto` | T-4 | **帧尺寸**：n=20 与 **n=100**（`fire_det` = **594 寄存器 = 99 只探测器**的合成帧；总计 1035 点）整帧 ≤ `MAX_FRAME_BYTES`；编 / 解码侧**同源同值**（`MAX_PERIPH_BYTES` 裁剪后仍 ≤ 上限）。**三口径各断言一次**：典型 33 B/点 ⇒ 整帧 **≈37.7 KiB（59 %）**；上界 48 B/点 ⇒ **≈54.1 KiB（85 %）**；`POINT_JSON_BYTES_F64_ABS_MAX` 形态 ⇒ 触发步骤 5 兜底（`available=false`）且**既有段逐字段不变**（EX-31 / F26.5） |
| `display-proto` | T-4b | **常量自洽（防回归）**：`MAX_PERIPH_BYTES == MAX_FRAME_BYTES − EXISTING_SEGMENTS_RESERVE`；`POINT_JSON_BYTES_TYPICAL(33) ≤ POINT_JSON_BYTES_UPPER(48) ≤ POINT_JSON_BYTES_F64_ABS_MAX(60)`；且以 `POINT_JSON_BYTES_UPPER` 反解的 `k_max ≥ 99`（**PRD 上限内不裁**的机械保证） |
| `display-proto` | T-5 | **既有字段语义冻结**：F1–F5 / 四段的字段名 / 语义 / 缺省 / `Field.flag` 四态**逐条快照回归**（EX-32 / F26.7）；`Field` 序列化形态不变 |
| `display-proto` | T-6 | `PeripheralBlock::key` 的位置式与 `renames` 覆盖（`fire_det_count` / `soc`）逐例正确；`decimals_from_scale` 四值覆盖完备（in-scope 登记行遍历） |
| `display-proto` | T-7 | **短标签表覆盖性（H-3 / W-4）**：`PERIPH_WHITELIST.len() == LABELS.len()`（**行数相等可机械断言**：两者均为常量数组）；白名单键逐项有短标签；**短标签表不含单位外的全角括号**（防登记 label 误入）；`fire_det` 以**模板 6 行**登记、由 `FIRE_DET_TEMPLATE_START(17)` / `FIRE_DET_STRIDE(6)` 展开（`point_table.rs:867` / `:869`） |
| `display_host` | T-8 | 慢拍 D 降级：`latest_values` 未接线 / 快照不可得 ⇒ `available=false`（**不得**出空段伪装正常） |
| `display_host` | T-9 | 点级 flag 判定**六分支**（§15.1.2 伪码逐行）：`station_is_active==false`→`Offline`、点缺→`NotRead`、**标量点 `ts_ms < station_last_poll_ms`→`NotRead`**、**位点跳过上一判据**（用假时钟 + 假快照构造：位点 `ts_ms` 恒旧但站活性在窗内 ⇒ 仍 `Valid`）、`quality != Ok`→`NotRead`、非有限→`RangeError`、其余→`Valid`；**不得**出现 `v=None` 但 `flag=Valid` |
| `display_host` | T-10 | 站离线时该站**全部**点 `v=None`；**其余站不受影响**（站级隔离，EX-04） |
| `display_host` | T-11 | **帧预算守卫三档**：① n=100 ⇒ **不裁**（`truncated` 空）；② 合成 **n=120（119 只 > `k_max`=111）** ⇒ 触发裁剪，`truncated = ["fire_det:119→111"]` 且**前缀截断**（按地址升序，可复现）；③ 人为把某点值构造成 `POINT_JSON_BYTES_F64_ABS_MAX` 形态 ⇒ 步骤 5 兜底，`peripherals.available=false`，**既有段逐字段不受影响**、帧照常发布 |
| `display_host` | T-12 | 「`latest_values` 写入 → 段缓存更新 → 发布」在**假时钟**下 < 合并窗口上限；且**忽略 `ts_ms`/`last_ok_ms` 的内容比较**不会误判变更（发布率上界 ≤4 Hz） |
| `console_host` | T-13 | 三端点：catalog 条目数上限、`rev` 与帧内 `catalog_rev` **同源同值**、`page_size` 上限拒绝、只读（无 POST 路径）、非 2xx 错误面；`fire_detectors` 的 `expanded != total` 如实返回（**不静默裁剪**） |
| HMI 逻辑 | T-14 | `MissingReason` 五语义**字符串两两互异**（重点：「站离线」/「未取数」/「数据异常」/「未配置」/「外设数据不可用」）；`cylinder_configured == Some(false)` ⇒ 「未配置」且**不含 `0 kPa`**、且**不产生告警条目**（EX-11） |
| HMI 逻辑 | T-15 | 枚举：火警等级 **6 值文案齐全**（`0/1/2/3/4/5`，值域出处 = **PRD F21 展示表**，非 `SIG_FIRE_LEVEL`，见 §15.3.2）；**表外值 ⇒ 「未知」**（**断言不等于「正常」**）；`pcs_3zone_67` ⇒ 「模式 `<值>`」+「（枚举文案待厂方追认）」（EX-10） |
| HMI 逻辑 | T-16 | **版本不匹配 vs 通道断开**：两文案字符串**不相等**；`Incompatible` 粘性（须成功帧清除）；`got/expected` 数值正确渲染（EX-29 的屏侧一半） |
| HMI 逻辑 | T-17 | 探测器「数据 1」拆解：`(raw >> 8) * 0.1` `dB/M` 与 `(raw & 0xFF) − 55` `℃`；**帧内整字值不变**（EX-13） |
| HMI 离屏 | T-18 | P4 / P6 各段渲染非空；关键区域语义色；降级态（`--` + 角标 + 「不可用」）；中文墨量密度断言（沿用 §11.1 判据）；导出 PNG |
| HMI 静态 | T-19 | **负向验收（结构性）**：P6 段标签集合中**不存在**「柜外温度 / 柜外湿度」**数值**行（EX-08）；**不存在**「台区 / 关口 / 总表」命名的 `meter_batt` 字段（EX-21）；**不存在**取自 `point_table` Role::Pcs 的 1046–1065 键**被标注为**「PCS 输出 / 充放功率 / 台区负荷」（**EX-23 的原文判据**，PRD `:1279`；**订正 v2.1-r2**：EX-23 是**误用禁止**，**不含"键缺失"要求**）。**另按本设计的选择**（§15.5.2 选项 A / R-42）**单独断言** `pcs_3zone_50`（1049）**整体不在白名单** —— 该条是**设计决策**的断言，**不是** EX-23 的判据 |
| HMI 静态 | T-19b | **bit15 不越界**：探测器状态整字的 `bits` 中 **bit15 `defined == false`** 且无 `label`；`BitMeta.inverted` **全表恒 `false`**（R-41 追认前无生产者）；已定义位恰为 `{12, 14}` |
| HMI 静态 | T-20 | **页数不变**：导航项 == 6；无新增路由项；`current_page` 在分段切换 / 下钻时不变（§15.5.3） |
| 静态约束 | T-21 | 沿用 §11.4 的六条 + `bindgen` allowlist 双向断言；**新增**：`local-display` 不得依赖 `mupc-southd`（承 §11.4 ②，且**本增量未放宽**——短标签表在 `display-proto` 正为此） |
| 时延回归 | T-22 | §15.6.1 的常量断言：`periph_poll_ms ∈ [1,1000]`、`MIN_MERGE_WINDOW_MS == 250`、`publish_ms ∈ [100,4000]`；**且"走广播路径"的算式（1.85 s）< "走兜底路径"的算式（`站周期 + periph_poll_ms + 0.85`）**（防退化）。**并断言**：默认配置下**广播路径 ≤2 s** 而**兜底路径可能 >2 s** —— 即 **"丢帧最坏"是一个被显式承认的、由 R-33 裁定的口径**，不得被 quietly 当成达标（§15.6.1 约束 1）。**增补（F25.4 / R-45，约束 3）**：断言口径 A 的算式 `0.5 + 0.001 + 0.25 + 0.6 = 1.35 s ≤ 2 s`（**判定侧 `stale_timeout_s` 不参与该口径**），并**静态断言屏侧 / 组帧侧源码中不存在 `now − ts > X` 形式的离线 / 新鲜度判定**（防第二套新鲜度口径） |
| 字体 | T-23 | **H-2 覆盖率用例**：短标签表 ∪ UI 固定文案 ∪ **分组标题（§15.7.3 的 31 项，按字面量，含 `（` `）` 与 `10 / 20 / 288`）** ∪ `ui/**` 字面量 ⊆ cmap 基线（失败时打印缺失码位集合） |
| `display_host` | T-24 | **接口缺口的退化（R-38）**：以「`station_last_poll_ms` 不存在」的桩运行 ⇒ ① `station.last_ok_ms == 0` ⇒ 屏显 `--`（**不臆造、不由本侧自维护**）；② 标量点退化按 `quality` 展（**断言不出现 `now − ts_ms > X` 形式的判定**，即 R-30 的退化被如实接受而非被时间阈值掩盖） |
| HMI 静态 | T-25 | **R-36 版面常量断言**（UI §6.4.1 / §6.6.1）：P4 总览带两卡间隙 `== GAP_MIN(16)`、总览带↔滚动区 `== GAP_GROUP(16)`、滚动区↔操作条 `== GAP_SECTION(24)`、火警卡↔危险按钮 **≥`GAP_DANGER(48)`**；P6 分段段宽 `185`、**段间隙 `== SEG_GAP(16)`**、段高 `≥TOUCH_MIN(48)`；`ui/**` **零裸尺寸 / 零裸色值**（承 §11.4 ④′） |

#### 15.8.2 需真机验证（`--features noto-font` + 真机屏 / 真实串口）

| ID | 项 | 判据 |
|----|----|------|
| **D-1** | **新增中文标签豆腐块**（H-4 / EX-24） | 逐页核对 §15.7.3 + 短标签表全部文案**无豆腐块**（缺字形须先扩字库，**未过不得通过 P4/P6 真机验收**） |
| **D-2** | **槽宽**（H-5 / EX-25） | 带符号数值（BMS 簇组电流 `-1600.0`、PCS 有功 `-60.0`）与长标签（储能表·电压不平衡度 / 接管回风温差报警）不越槽、不截断 |
| **D-3** | 字号与 0.5–1.5 m 可读性；1024×768 的实际列数 / 分组排布 | §4.2 判据 |
| **D-4** | 触摸：分段切换 ≤300 ms；P4 / P6 滚动 ≥30 fps；下钻 / 收起可达性 | TT-05 / TT-06 |
| **D-5** | **资源实测**：`LV_MEM_SIZE = 1 MB` 在 P4/P6 新增对象后的余量（`lv_mem_monitor` 或 `smaps`）；稳态 CPU ≤40 % 单核 | §10 / R-24 / R-34 |
| **D-6** | 端到端时延探针（外设值变化 → 屏上生效） | §15.6.1 的口径 A / B 各测一次；HVAC 与 BMS 各一例 |

#### 15.8.3 需现场验证（承 PRD §3.9.2 第三档）

各站 `port` / `slave` / `baud_rate` / `parity` 校准（RC-1/3/5/8/11）；HVAC `interval_ms = 5000` 下屏端"最近更新"是否被误读（F25.2）；消防探测器登记数与地址序（RC-5/RC-10）；`fire_sys_2` 的"未配置"判定与现场实际一致（EDGE-23）。

---

### 15.9 待裁定 / 跨文档报告项（**本节只报告，不自行改需求**）

| ID | 项 | 类型 | 影响 | 本设计的默认处置 |
|----|----|------|------|------------------|
| **R-28** | **F24（PCS）缺第二写入方**：`latest_values` 写入方限定为 `SouthSink`，而 F24 真源是 `intercore` | **跨设计 + PM** | 高：决定 EX-22 能否达成 | 按 §15.1.3 处置 1（`latest_values` 接纳第二条写入路径）；若其设计不接纳 ⇒ PCS 段显「未取数」，**EX-22 不达成** |
| **R-29** | **T-12 需增补 10 号（核间通信）**：F24 的 6 组新增量需 `intercore` 扩展读取（现仅读 5 个量） | **跨文档（10 号）** | 高：F24 前置 | 登记为 12 号之外的**独立动作**；PRD 的 T-12 只覆盖 01 / 03，**本节报告补 10** |
| **R-30** | `latest_values` **不提供**「站级最后成功时刻」的公开读口 ⇒ 点级「本轮未更新」**不可判**（详见 R-38） | **跨设计** | 中：mapper 滤除越界点时**会把陈旧值显示为实时值**（该点 `ts_ms` 停在上一轮、`quality` 仍为 `Ok`） | 退化为「点存在且 `quality == Ok` 即按 `Valid` 展示」，**如实登记**；**屏侧不得用时间阈值补**（补了即第二套新鲜度判据，违反 F25.1）。**撤销 v2.1-r1 的 `round_seq` 表述**（01 中无此概念，N-19） |
| **R-31** | **PRD F22 的 `Ah` 容量声明"1 位小数"与登记不符**：`bms_cap_*` 四行 `scale = 1.0` | **需求 vs 登记** | 低 | **以登记为准（整数）**；请产品确认或由现场 RC 一并核对（Q-16「16/32 位未明确」） |
| **R-32** | `pcs_3zone_67`（工作模式）**枚举文案未登记**（`point_table.rs:744` 仅写"（枚举）"） | **厂方追认** | 低 | 屏显「模式 `<值>`」+「（枚举文案待厂方追认）」；**不臆造**；F24 的"枚举外值显「未知」"因无值域**暂不可判** ⇒ 需厂方补值表 |
| **R-33** | **PRD F25 验收 3 · 分句②「离散告警位变化后 ≤2 s」的两条口径未闭合**：① 与 **HVAC 站周期 5 s** 不可同时成立（物理不可达）；② 对 1 s 站，**走广播路径 1.85 s 达标、广播丢失走兜底路径 2.35 s 超差**（广播**会丢**，N-18） | **产品裁定** | 中：告警位上屏的验收口径 | **两侧选项（均已在 §15.6.1 落为可执行配置）**：**A（默认）** `periph_poll_ms = 500` ⇒ 只保证**广播路径**达标，**丢帧窗口最坏 2.35 s**；**B** 要"任何丢帧都 ≤2 s" ⇒ `periph_poll_ms = 100`（**仅改配置，零代码改动**；成本 ≤0.5 % 单核，§15.6.1 约束 1）。**HVAC 部分**建议口径：**≤2 s 适用于站周期 ≤1 s 的站**（BMS / 消防 / 储能表），HVAC 取「站周期 + 1 帧节拍」 |
| **R-34** | `LV_MEM_SIZE = 1 MB` 在 P4/P6 新增对象后的**余量未量化** | 真机（承 R-24） | 中 | 分段惰性创建 + 窗口化列表已按此约束设计；真机 `lv_mem_monitor` / `smaps` 复核（D-5） |
| **R-35** | **字库缺口最终值**：下界 41（PRD）/ 上界 189（误用登记 label 的实测） | 设计 → 实现 | 中 | 以 `T-23` 用例输出为准重算并补 `font_subset_charset.txt`（§15.7） |
| **R-36** | **12-UI 设计文档需补 P4 / P6 版面**（视觉规范权威在 UI 文档） | **UI 评审员 / 产品** | 中：P4/P6 的视觉定稿 | ✅ **本版已补齐**：可实施版面规格落 **UI 设计文档 §6.4.1（P4）/ §6.6.1（P6）/ §5.1 #21（`SegmentedTabs`）/ §2.1 补注（段间距适用范围）**，本设计 §15.4 / §15.5.1 只引其常量；机械断言见 **T-25**。**仍待 UI 评审员复审**（该文档的文首标记只对 v2.0 有效，本增量为追加节） |
| **R-37** | **明细下钻口径**（PRD T-11 的 P-4 / P-5）：探测器"逐只罗列 vs 汇总 + 下钻"与 BMS 288 位的下钻形态 | **产品 + UI** | 中 | 本设计默认：**汇总 + 分页下钻**，探测器 20 只/页、BMS 告警 50 位/页；需产品 / UI 追认。**版面细节见 UI §6.4.1**（行高 44 / 按钮 48×48 / 「收起」在视图顶部右端） |
| **R-38** | **`latest_values` 缺「站级最后成功时刻」读口**：`station_poll_ms` 是**私有字段**，公开面只有 `station_is_active(&str, now_ms) -> bool`（在线），**无"最后成功时刻"** | **跨设计（01 号）** | **高**：F25.4 / EX-28 的「每站**在线 / 离线 / 最后成功时刻**」+ 本设计的点级「本轮未更新」判据**都依赖它** | **接口要求（1 个 getter，不改任何既有语义）**：`pub fn station_last_poll_ms(&self, station: &str) -> Option<u64>`（返回 `station_poll_ms[station]`；`None` = 从未轮询成功）。**建议纳入 01 §9.1.3**。若 01 不接纳 ⇒ ① `station.last_ok_ms` 恒 0 ⇒ 屏显 `--`（**不臆造**）；② 点级「本轮未更新」退化为 R-30；**本设计不自行在 display_host 记第二份"最后成功时刻"**（违反"同一事实不记两份"） |
| **R-39** | **撤销的接口假设（留痕）**：v2.1-r1 曾要求 `snapshot_roles(&[Role])` / 站级 `online` 字段 / 点级 `round_seq` / `upd_ms` —— **01 §9.1 中均不存在** | **本节内部（已处置）** | — | **已全部撤销**：`snapshot_roles` → 逐站 `station_snapshot`（**已存在**）；站级在线 → `station_is_active`（**已存在**）；`round_seq`/`upd_ms` → `PointValue.ts_ms` 与站级时刻比较（**等价判据**，且**不引入新阈值**）。**唯一净新增 = R-38 的一个 getter** |
| **R-40** | **「常显」与分段控件的冲突**：PRD `EX-05 / EX-14 / EX-18 / EX-19 / EX-20 / EX-22` 要求各外设量「常显」，而 P6 的**分段控件同屏仅 1 段可见**；且 PRD **B8 的"分区切换"许可只限主状态页**（§0 B8 / §4.2 原文：「**主状态页**：主读数区不横向滚动，允许整页纵向滚动」） | **产品裁定** | **中–高**：决定 P6 的信息架构是否成立 | **建议口径**：**「常显」= 段内无隐藏条件**（进入该段即可见；不因取数成败隐藏；不折叠；不需二次点击）。**两侧选项**：**A（推荐）** 采纳上述口径 ⇒ 现有版面成立，EX 的"常显"按"段内常显"逐条可测（**本设计默认 A**）；**B** 若坚持字面"同屏常显" ⇒ 必须 ① 放宽 B8 的"分区切换仅主状态页"或 ② 回退到 P7 独立页（但那要回写 F11 / TT-01 的 6 页口径）⇒ **属需求回写，非设计可自行决定**。**本设计不自行改需求** |
| **R-41** | **探测器状态位 bit15「通信状态」须厂方追认**：PRD F21 只要求 bit12 / bit14；`SIG_FIRE_DETECTOR` 的注释把 bit15 列在「**不登记**」清单（理由：「极性反转位…无明确告警语义 ⇒ **不猜、不造判据**」，`point_table.rs:302-306`） | **产品 + 厂方追认** | 低–中：影响"逐只探测器在线/离线"能否上屏 | **默认不上屏**：bit15 ⇒ `defined = false` ⇒ 屏显「未定义位 15」（**不得默默使用**）。**备选（若追认）**：由 catalog 携 `BitMeta{ index:15, inverted:true }` 单点启用 ⇒ **屏侧代码零改动**（`inverted` 字段已预留，见 §15.3.2）。**两侧选项均在**，由产品裁定 |
| **R-42** | **PRD 自身冲突**：F24 展示表含 **1049**（PRD `:757`），而 §7 范围外第 11 条排除 **1046–1065**（PRD `:1042`） | **产品裁定（PRD 回写）** | 中：决定 `pcs_3zone_50`（STS 电网电压幅值）上不上屏 | **依据订正（v2.1-r2）**：排除侧的真实依据 = **PRD §7 #11（`:1042`）**；**不是** EX-23 / F24 验收 2（`:1279` / `:773`）——那两处是「不得**被标注为**『PCS 输出 / 充放功率 / 台区负荷』」的**误用禁止**，**不含"键缺失"要求**（一个上屏但**正确标注**为「STS 电网电压幅值」的 `_50` 并不违反 EX-23）。**本设计取排除侧**（`_50` 不进白名单）。**若裁定为"上屏"** ⇒ **只需回写 PRD §7 #11**（改为「除 1049 外」），**EX-23 无需改动** ⇒ 回写量比 v2.1-r2 所述更小（**需求回写**，非设计可自行决定）。**并登记第二个 PRD 冲突**：PRD T-11 写 n=100 ⇒ `fire_det` **595 点**，而 §3.9 F21.4 与 §7 #11 的算式均为 **594 寄存器**（6×(n−1)）——**差 1**，本设计按 **594（= 99 只）** 计 |
| **R-43** | **极端组合下外设段整体降级**：`EXISTING_SEGMENTS_RESERVE = 8 KiB` **不覆盖**「10× `MAX_ALARM_MESSAGE_BYTES`(1 KiB) 告警 + 外设段近满」的极端组合（`56 + 10 + 1.5 ≈ 67.5 KiB > 64 KiB`） | **设计（已登记 / 已兜底）** | 低（工程不可达） | 由 §15.2.4 步骤 5 的**出口守卫**兜底：`peripherals.available = false`、既有段照发、**不黑屏**。**同类兜底边界之二（f64 理论极值形态）**：`(441+594)×60 + 2,772 + 1,643 = 66,515 B = 64.96 KiB ≈ 65.0 KiB > 64 KiB`（**单位统一为 KiB**，订正 v2.1-r2 的「66.6 KiB」= 66.5 KB）。**为什么不把预留抬到 12 KiB**：那会把 `k_max` 压到 97 < 99 ⇒ **PRD 上限内就要裁**，与本节的容量结论冲突（取舍已写明） |
| **R-44** | **`lv_buttonmatrix` 不满足 F14.2 的 16 px 触摸目标间距**（**依据订正 v2.1-r2**：该控件**有**段间距 API `pad_column`——`lv_buttonmatrix.c:1024-1029`；真正的问题是**命中区按 `pcol/2+1` 向两侧外扩**，`:892-935`，上限 `LV_DPI_DEF/10 = 13 px` ⇒ 净距 = `pad_column − 2 × min((pad_column/2)+1+(pad_column&1), 13)`：`pad_column = 16` 时 **−2 px**、`≤ 24` 恒为 −2 px、**须 `≥ 42` 才够 16 px**） | **UI 评审员**（**架构侧已裁定**，见右） | 中：P3 / P5 的既有 `SegmentedControl`（3 段筛选）同样受影响 | **架构侧裁定（v2.1-r3，替代 v2.1-r2 的"仅登记"）**：① **不合并为同一实现**——两者在 F14.2 上**不等价**（`lv_button` 命中区 = 自身边界、无外扩；`lv_buttonmatrix` 有外扩）⇒ **不存在"改用既有控件即可达标"的路径**（`SegmentedControl` 所在的 P3 / P5 也**未达标**）；② **但合并同一视觉规格真源**：`style_seg_*` + §5.2 的 `SegmentedControl` 行，**不新增色值**；③ **新增使用点一律 `SegmentedTabs`**（`SEG_GAP = 16`，见 §15.5.1 / UI §5.1 #21）；④ **P3 / P5 是否迁移 = 独立动作**（改的是已 `[DESIGN_APPROVED]` 的 §1–§14 页面）⇒ **本节不改其页面**，由 UI 评审员另立条目裁定 |
| **R-45** | **F25.4「站离线变化 ≤2 s 上屏」的端到端口径依赖 02 号南向的「快采」能力**（v2.1-r2 **漏给时延落点**）：判定侧下界 = `stale_timeout_s` = 5 s（**物理**）⇒ 端到端 **6.35 s**（§15.6.1 约束 3） | **跨文档（02 号）+ 产品** | 中：决定 F25.4 按哪条口径验收 | **本设计可保证**：口径 A（**结论成立 → 屏上可见**）= `0.5 + 0.001 + 0.25 + 0.6 = 1.35 s`，**≤ 2 s ⇒ 达标**（**四段**定值 ②–⑤ 见 §15.6.1 表；断言见 T-22）。**口径 B（端到端）= `5 + 1.35 = 6.35 s` ⇒ 不达标**，需 02 号提供**独立于站轮询周期的告警位 / 站活性快采**（用户裁定）。**该能力尚未落地** ⇒ ① 本设计**只登记依赖，不登记其接口签名、不假定其存在**；② **降级口径**：屏侧 / 组帧侧**不得**为此新造第二套离线判据（禁止 `now − last_ok_ms > 2 s`，违反 F25.1 单真源），口径 B 如实登记为不达标；③ **不阻塞**本次实现（02 号落地后口径 B 自动收敛，**屏侧零改动**） |

---

### 15.10 新增技术决策记录（ADR 续，D18–D22）

| 决策 | 结论 | 理由 / 被否方案 |
|------|------|-----------------|
| **D18 外设数据面** | **慢拍 D（`periph_poll_ms = 500`）+ `latest_values` 内存快照 + 变更通知 ∪ 兜底 tick + 内容比较 + 合并窗口** | 与既有慢拍 A/B/C 同构（KISS）；守住"帧路径零阻塞 I/O、零 DB 查询"（§2.1 / D6）。**否决**：① 每帧直读快照（把点数成本与锁争用带进 1 Hz 主拍）；② 纯订阅回调（整块刷新突发 ⇒ 发布率被点数放大）；③ 轮询 `telemetry` 表（RQ-9.0-1 明令禁止） |
| **D19 帧承载** | `peripherals` 段签入 **v3**；**值逐点（位点与标量同构）**；**元数据一律出帧**；新增 `MAX_PERIPH_BYTES = 56 KiB`（= 帧上限 − 8 KiB 既有段预留）预算守卫 + `truncated` 显式标记 | §15.2.4 的容量实证（**实测**典型 33 B/点：n=20 ⇒ 整帧 **22.2 KiB / 35 %**，n=100 ⇒ **37.7 KiB / 59 %**；上界 48 B/点 ⇒ n=100 **54.1 KiB / 85 %**；而元数据入帧（**下界** 118 B/条）⇒ 仅元数据即 **64.0 KiB**、整帧 **≈102.7 KiB** 爆帧）；守卫按**可达上界**预检 ⇒ `k_max = 111` 只 > 99 只 ⇒ **PRD 上限内不裁**（自洽）。KISS（不引入位图 / 字节序 / 极性三类新歧义，收益仅 ~8 KB） |
| **D20 元数据通道** | **控制通道三个只读 GET 端点**（catalog / 探测器分页 / BMS 告警分页） | 不改 D6「读侧只有一个端点」；复用 `ConsoleClient` 的既有非阻塞状态机与错误面；与 `/config` `/logs` `/audit` 同属"一次性 / 带参 / 有限额的受控读"。**否决**：① 元数据入帧（爆预算）；② HMI 侧 build-time codegen 静态名表（引入 `local-display → mupc-southd` 构建期依赖，与 §11.4 ② 的架构意图冲突；且**不跟随现场配置**） |
| **D21 页内组织** | P6 = **分段控件（5 段）+ 段内滚动 + 窗口化列表**；P4/P6 明细 = **下钻视图**；**均不构成新页面**，页数仍 6 | T-8 裁定的"不新增页"必须落实为**实现层面的页面数不变**（§15.5.3 的机械断言）；P6 全量约 200 行，逐行建对象会冲击 `LV_MEM_SIZE`（R-24 / R-34） |
| **D22 展示文案源** | 上屏中文一律来自 **`display-proto` 的短标签表白名单**；**禁止**把 `point_table` 的登记 `label` 直接上屏 | 登记 `label` 是**登记说明文本**（含全角括号与内部注记），直接上屏：① 实测字库缺口 **189** 码位（vs 短标签的 ≈41 下界）；② 界面噪音大；③ 且**以运行时字符串到达 HMI** ⇒ 既有码表覆盖率测试**看不到**（N-10 / F-5）。放 `display-proto` 后全部上屏中文回到**可静态扫描的源码**内，**消除该盲区**（F-6 / H-2） |

---

### 15.11 编码落点清单（逐文件；供开发直接开工）

| # | 文件 | 改动 | 规模 |
|---|------|------|:----:|
| 1 | `mupc/crates/display-proto/src/frame.rs` | `PROTO_VERSION` 2→3；`DisplayFrame` 增 `peripherals`（`#[serde(default)]`）；**五个常量**：`POINT_JSON_BYTES_TYPICAL(33)` / `POINT_JSON_BYTES_UPPER(48)` / `POINT_JSON_BYTES_F64_ABS_MAX(60)` / `PERIPH_FIXED_JSON_BYTES(4096)` / `EXISTING_SEGMENTS_RESERVE(8 KiB)` / `MAX_PERIPH_BYTES(56 KiB)`（§15.2.2） | S |
| 2 | `mupc/crates/display-proto/src/peripherals.rs` | **【新】**§15.2.2 + §15.3.2 的全部 DTO + `key()` / `catalog_rev()` / `decimals_from_scale()` | M |
| 3 | `mupc/crates/display-proto/src/peripherals_labels.rs` | **【新】**短标签表白名单（`label_for(role, block, at) -> Option<&'static str>`）+ **`PERIPH_WHITELIST: &[(PeriphRole, &str, u16)]` 常量数组**（W-4 / H-3 的"行数相等"断言前提）+ 分组键常量 + UI 固定文案常量 | M |
| 4 | `mupc/crates/display-proto/src/config.rs` | `DisplayConfig` 增 `periph_poll_ms`（默认 500，`[1,1000]`）/ `periph_page_size`（默认 20，≤50）；`validate()` 同步 | S |
| 5 | `mupc/crates/data-processing/src/latest_values.rs` | **【他路设计负责】**本节只登记**消费契约**（§15.1.2 ①/③）与 **R-28 的第二写入方**、**R-38 的 `station_last_poll_ms` getter** 两项诉求 | — |
| 6 | `mupc/crates/mupc-core-bin/src/display_host.rs` | 慢拍 D（`run_periph_sampler` / `sample_peripherals` / `peripherals_changed`）；`peripherals_cache`；`build_frame` 组装 + 预算守卫；`PeripheralSource` 注入接缝 | L |
| 7 | `mupc/crates/mupc-core-bin/src/startup.rs` | `SouthSink` 写 `latest_values`（承 N-4）；`CylinderPressureQuery` 适配器（包 `SouthStations::cylinder_pressure_configured`，N-14）；display 装配注入 | M |
| 8 | `mupc/crates/mupc-core-bin/src/console_host.rs` | 三个只读 GET + catalog 构建器（白名单投影 + `rev`） | M |
| 9 | `mupc/crates/local-display/src/state.rs` | `ChannelStatus::Incompatible{got,expected}` / `ScreenMode::VersionMismatch` / `MissingReason` 五态 / `PeriphView` 派生 | M |
| 10 | `mupc/crates/local-display/src/channel.rs` | 把 `Error::ProtoVersion` 归一为 `Incompatible`（粘性） | S |
| 11 | `mupc/crates/local-display/src/console.rs` | 三个新 GET（DPO + 错误面沿用既有五类） | S |
| 12 | `mupc/crates/local-display/src/ui/pages/p4_interlock.rs` | 总览带（火警等级卡，**版面见 UI §6.4.1**）+ 消防四组 + 探测器分页下钻；**既有写操作条位置不动**（§15.4） | L |
| 13 | `mupc/crates/local-display/src/ui/pages/p6_system.rs` | 更名「装置与外设」；`SegmentedTabs` + 5 段；各段分组渲染 + 窗口化列表；BMS 告警下钻（**版面见 UI §6.6.1**） | L |
| 14 | `mupc/crates/local-display/src/ui/pages/mod.rs` | 导航标签 P6 → 「装置与外设」；**页数不变** | S |
| 15 | `mupc/crates/local-display/src/ui/theme.rs` | 新增 **`Dimens::SEG_GAP = 16`**（F14.2 段间距，§15.5.1）+ `SegmentedTabs` / 下钻的尺寸与状态色（**页面仍不得硬编码裸值**） | S |
| 16 | `mupc/crates/local-display/fonts/font_subset_charset.txt` + `gen_fonts.sh` 重跑 + `lv_font_cmap.txt` | §15.7 H-1（**同批提交**） | S |
| 17 | `mupc/crates/local-display/src/ui/tests.rs` | H-2 覆盖率用例（T-23）+ P4/P6 离屏与静态用例（T-18~T-20） | M |
| 18 | `mupc/deploy/config/mupc_core_config.production.yaml` | `display:` 段增 `periph_poll_ms: 500` / `periph_page_size: 20` | S |
| 19 | `mupc/deploy/deploy.md` | **「`mupcd` 与 `mupc-local-display` 必须同版本发布」**（F26.3）+ 字库门禁说明（H-1） | S |

**工作量粗估（供 PM）**：契约层 **S–M**、mupcd 侧 **M–L**、HMI 侧 **M–L**（含分段与下钻）、字库与门禁 **S**、测试 **M** ⇒ 合计 **L**（不含 R-28 / R-29 的跨模块动作）。

**实施顺序（强依赖）**：① 契约（1–4）→ ② `latest_values` 消费联调（5–7）→ ③ 端点与屏侧客户端（8, 11）→ ④ HMI 逻辑与页面（9, 10, 12–15）→ ⑤ 字库门禁（16, 17）→ ⑥ 部署文档（18, 19）。**任一阶段未过评审不得推进下一阶段**。

---

## 附录 A：版本历史

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| v1.0 | 只读状态屏设计（其前提已被 PRD v2.0 推翻，仅作历史追溯） |
| v2.0 | 触摸式本地 HMI：合并 08 模块的本地展示与操作，移除 `web-api` |
| v2.0-r2 | GUI 框架由 Slint 切换为 LVGL（Slint 闭源商用嵌入式交付必须付费商业许可） |
| v2.0-r3 | LVGL spike 实测整改：字体链、体积预算、CJK 字形判据与构建硬知识按实测订正 |
| v2.0-r4 | LVGL v9.5.0 API 事实订正；`LV_MEM_SIZE` 按实施期实测定稿 1 MB |
| **v2.1-r3（2026-09-23）** | **末轮返工（第二次评审 REJECTED：5 严重 + 6 中等；"只需改文字与数字，不需重构"）** —— ① **LVGL 依据订正（结论不变）**：`lv_buttonmatrix` **有**段间距 API（`pad_column`，`lv_buttonmatrix.c:1024-1029` / `:1059` / `:1067-1068`），真正不达标的是**命中区按 `pcol/2+1` 外扩**（`:892-935`，上限 `LV_DPI_DEF/10 = 13 px`）⇒ `pad_column = 16` 净距 **−2 px**；**同时裁定 R-44**（与 `SegmentedControl` **不合并实现、只合并视觉规格真源**，P3/P5 迁移另立条目）；② **容量表 n=20 行按自述公式重算**（20.1 / 21.7 → **20.6 / 22.2 KiB**；全表改「一行一算式」可复算，§15.2.4 / §15.6.3）；③ **元数据入帧反证的"三值并存"收敛**：取 **118 B/条下界** ⇒ 元数据 **64.0 KiB**、整帧 n=100 **102.7 KiB**、n=20 **87.2 KiB**（订正 §15.2.2 的「69.7」、§15.3.1 的「67.9」、§15.2.4 的「100.5 KiB / 122 / 138」）；④ **R-42 依据订正**：排除侧依据 = **PRD §7 #11（`:1042`）**，**不是** EX-23 / F24 验收 2（`:1279` / `:773` 是"不得**被标注为**…"的**误用禁止**）⇒ **选项 B 无需改 EX-23**（T-19 判据同步订正）；⑤ **§15.7.3 按字面量重列**分组标题 ⇒ 去重后 **31**（不是 30），**H-2 / T-23 补全角括号 `（` `）` 与数字**；⑥ 中等项：P6 全量 **555 → 428 + 装置段（11）** 且**与 n 无关**（fire 落 P4）、R-43 **66.6 → 64.96 KiB**（KB/KiB 统一）、`truncated` 示例 **101 → 111**、**F25.4 补时延落点（口径 A 1.35 s ≤ 2 s）+ 依赖项 R-45（02 号"告警位 / 站活性快采"，**未落地**，含"不得新造第二套离线判据"的降级口径）**、文首「权威点表」行的 `REG_I_A` / `REG_P_A` 归属订正（→ `transport/modbus.rs:170-171`，非 `pcs.rs`）、**全节 `file:line` 回源复校**（含 `display_host.rs:799-821` 的 `slow_tick` 起点修正）。**报告项 R-28~R-45**；**未改** PRD / 01 / 02 / 03 / 代码，**既有门禁标记未动** |
| **v2.1-r2（2026-09-23）** | **按设计评审意见修订（REJECTED 返工；待复审）** —— 逐条修 **5 严重 + 6 中等 + 5 处事实/交叉引用错误**：① **容量守卫按实测编码长度统一口径**（33 / 48 / 60 B 三档不再互串；`MAX_PERIPH_BYTES` 40 KiB → **56 KiB**，`k_max = 111` 只 > 99 只 ⇒ **PRD 上限内不裁**，守卫式与结论一致；元数据入帧反证改为 **≈122–138 KiB（远超 64 KiB）**）；② **撤销 01 设计中不存在的接口假设**（`snapshot_roles` / `round_seq` / 站级 `online` 字段 / `upd_ms`），改为「**已存在接口 + 显式接口要求 R-38（一个 getter）**」；③ **变更通知 `Notify/watch` → `broadcast`（容量 64，`Lagged` 丢帧）**，≤2 s 改为「**丢失由兜底 tick 收敛**」并给两侧可执行配置（§15.6.1 约束 1）；④ 登记 **PRD 自身冲突**（F24 含 1049 vs §7 #11 排除 1046–1065；T-11 的 595 vs 594）与「**常显** vs 分段控件」冲突（**R-40 / R-42**）；⑤ 补 **bit15 通信状态的厂方追认**登记（**R-41**，默认不上屏）；⑥ **`SIG_FIRE_LEVEL` 仅 4 条** ⇒ 火警等级 6 值改引 **PRD F21**；⑦ **PCS 白名单 55 → 27**、全表改按**白名单**统计（555 点）；⑧ 短标签表**可枚举**（`PERIPH_WHITELIST` 常量数组）⇒ H-3「行数相等」可写；⑨ 补 **`SEG_GAP = 16`**（F14.2）与 **R-36 版面定稿**（P4/P6 落 UI 文档 §6.4.1 / §6.6.1 / §5.1 #21）；⑩ 事实订正：`ChannelStatus` **3 态**（`Stale` 属 `Freshness`）、`§15.10 R-31/R-33` → **§15.9**、T-4「594 只探测器」→ **594 寄存器（99 只）**、§15.7.3「11 个」→ **30 个**（并补漏列的火警等级）、§15.11 第 5 行路径 `mupc-data-processing/` → **`mupc/crates/data-processing/`**；**全节 `file:line` 回源重校**。**报告项 R-28~R-44**；**未改** PRD / 01 / 03 / 代码，**既有门禁标记未动** |
| **v2.1-r1（2026-09-23）** | *(历史)* **「外设数值上屏」增量（技术债 U-73；对应 PRD v2.2 §3.9 F20–F26 / T-8 裁定）** —— **追加 §15**（15.0 前置事实核对 / 15.1 `latest_values` 消费方式与消费契约 / 15.2 帧契约 v3 与容量结论 / 15.3 catalog 与明细下钻三端点 / 15.4 P4 消防 / 15.5 P6「装置与外设」 / 15.6 刷新与四种降级 + 性能 / 15.7 字体门禁 / 15.8 测试策略 / 15.9 报告项 R-28~R-37 / 15.10 ADR D18–D22 / 15.11 逐文件改动清单）；同步文首状态行、目录、附录 A。**§1–§14 的正文、`[DESIGN_APPROVED: 2026-09-11]` 标记与 §14 条目一律未改**（该标记**不覆盖**本增量） |
