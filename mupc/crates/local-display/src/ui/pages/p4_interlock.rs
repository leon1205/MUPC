//! # `ui/pages/p4_interlock.rs` —— P4 安全 / 联锁页（12-MUPC v2.0 工作单元 **B2b-3**，含写操作）
//!
//! 设计文档 §6.4「P4 安全 / 联锁页（F16–F18，含写操作）」逐条落地；UI 设计文档 §6.4 给版式、
//! 「区块规格」表与「操作与拒绝原因」表（EDGE-12），§8.3「联锁状态不可用（F16.6 / IL-01）」
//! 专行给**本页最重要的不变量**，§7.3 给弹层，§2.5 给强确认分级，§5.1 #2/#13/#14/#17/#18/#19/#20
//! 给控件尺寸，§5.2 给危险态色值，§3.6 P4 行给上屏文案，§3.2 / §3.6 给语义色与字形集。
//!
//! ## 本页与 P2 的两点结构差异（**契约见 `ui/pages/mod.rs` 的「补充（B2b-2）」**）
//!
//! 1. **读路径 = 帧驱动**（与 P2 **不同**）：`InterlockSection` 是 1 Hz 显示帧里的「慢拍 C
//!    （0.5 s）」分段（设计 §3.1）⇒ 展示态走 [`PageInput`]（`input.frame.interlock`），
//!    页面**不自行发请求**。`frame = None` ⇒ 取契约缺省（`available = false`）⇒ 屏显
//!    「联锁状态不可用」，**绝不**显「未联锁」（IL-01.6 / §8.3 专行）。
//! 2. **写路径 = 控制通道意图**（与 P2 **同一口径**）：本页**不发请求、不生成 `request_id`**
//!    —— 未确认 ⇒ 无任何意图；确认完成（L2 长按满 1.0 s）经
//!    [`P4InterlockPage::set_on_release`] / [`P4InterlockPage::set_on_ack_m1`] 把
//!    [`InterlockOpPayload`]（UI **观测到**的 `observed_latched` + `observed_sources`，
//!    供后端做乐观并发检查，EDGE-19）交回外部；`request_id` / `issued_at_ms` / HTTP 由 B3 的
//!    `console.rs` 承担；结果经 [`P4InterlockPage::show_result`] 灌回。
//!
//! 页根形态遵**契约 1′**（与 P2 同：页根容器 + 底部固定操作条）。UI 线框说本页「内容 528 /
//! 视口 552 → 单屏可见，无滚动」—— 仍按契约 1′ 组织（滚动容器高度足够即自动不滚）。
//!
//! ## ⚠️ 已知偏差登记（**独立编号 `IL`** —— `D1~D9` 是页面层编号、`CD1~CD7` 是
//! `ui/controls.rs` 编号、`PD1~PD23` 是 `p2_config.rs` 编号，本表**不与三者冲突**）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | IL1 | **全角标点一律改写**：`，`(U+FF0C) / `：`(U+FF1A) / `、`(U+3001) / `（`(U+FF08) / `）`(U+FF09) **实测均不在生成字体 cmap 内** ⇒ 一律取 cmap 内的 `·`(U+00B7) 或 `/`(U+002F)。受影响串逐条：`处于自锁态，须先释放联锁`→`处于自锁态 · 须先释放联锁`；`触发源未复位：急停、门禁`→`触发源未复位 · 急停/门禁`；`将清除联锁自锁状态，装置可恢复运行`→`将清除联锁自锁状态 · 装置可恢复运行`；`将下发 M1 停机复位，装置可重新启动`→`将下发 M1 停机复位 · 装置可重新启动`；`联锁状态已变化，请刷新后重试`→`联锁状态已变化 · 请刷新后重试`；`审计不可用，操作未执行`→沿用 P2 的 `审计不可用 · 操作未执行`；`触发源（N）`→`触发源 · N`（与 P2 **PD1** 的 `本机监听地址 · IEC 104` 同一处置）；`保持时间不足，还需 N 秒`→`保持时间不足 · 还需 N 秒`；`上一操作正在处理中，请稍候`→`操作进行中`（`理`/`稍`/`候` 三字**均缺** ⇒ 取最短的**在 cmap 内**等价表述）；`内部错误：X`→`内部故障 · X`（`错`/`误` 二字**均缺**，`故`/`障` 在内） | 字库资产（`gen_fonts.sh` + `extract_charset.py`）本轮按 PM 裁定**不动**（见 `pages/mod.rs` 的 **D4** 同一口径） | **B2c 之后**的「字体码表 + 文案统一收口批」：扩 §3.6 字符集后**逐字改回契约原文** |
//! | IL2 | **`✗`(U+2717) / `✕`(U+2715) 实测不在 cmap 内** ⇒ 「停机失败」的图标通道取 `×`(U+00D7，**§3.6 声明的符号集内**、几何等价)；Toast 失败图标沿用 P2 的 `!`（**转出**，不另抄字面量） | 同 IL1（字库缺口）；`✓`(U+2713) 在 cmap 内 ⇒ 「正常」用 `✓`，与 `×` 成一组 | 同 IL1 |
//! | IL3 | **栅格偏差（root cause = `theme` 缺 2 px 档，与 `pages/mod.rs` **D3**、`p2_config.rs` **PD5** 同族）**：联锁总态卡外缘高 **174**（UI 写 180，−6）；触发源卡高 **202**（UI 写 200，+2）；状态三卡高 **106**（UI 写 108，−2）；三卡行页内 y **414**（UI 绝对 492 − 72 = 420，−6，由前两项累积）；操作条内按钮 y **4**（绝对 628，线框 630，−2，与 P2 同款）；「操作将记入审计」y **24**（绝对 648，线框 652，−4）；页内容总高 **514**（UI 写 528） | `theme` **缺 2 px 档**（6 px 档**不缺** —— `Dimens::INTERLOCK_BAR = 6` 存在，本页 `STATE_BAR_LEFT_W` 正在用它），**不为凑像素引入裸规格值**（`ui/tests.rs` 的两张静态网也明令禁止） | 与 D3 / PD5 / PD19 / IL20 同批：`theme` 缺口上收时一并处理 |
//! | IL4 | **在滚动视口与固定操作条之间增设 24 px「就地原因带」**，落在**视口末 24 px**（页内 y528–552 = 绝对 y600–624）；滚动视口高 **528**（UI 线框写 552）、固定操作条仍 **72** 且其上下缘与线框逐像素一致（页内 552–624 = 绝对 624–696） | UI §6.4「操作与拒绝原因」表与 §8.3 联锁专行**均要求**「按钮正上方 24 px 就地显示原因」（EDGE-12 / IL-03 硬要求）。**二者可行**：把该行放在**视口最后 24 px** 即可 —— 线框 `(0,624,1024,696)` 的 72 px 操作条里按钮占 64 px（y630–694），本就**不需要**为原因行腾位置（原因行在按钮**之上**、操作条**之外**；实现正是如此，故操作条与线框逐像素一致）。线框只是**未画**这一行（常态示例），并非与之冲突；本页把线框的「内容 528 / 视口 552」读作「**视口 528 + 原因带 24 = 552**」的另一种切分 | 无（**有意**）；若 PM 要求逐像素回线框，只需在 UI §6.4 / 线框补画该 24 px 行（**不必**删去「按钮正上方 24 px 就地原因」） |
//! | IL5 | 就地原因带为**双槽**：左槽 x0 / 宽 352（放**全局**与**人工释放联锁**相关原因，含「联锁状态不可用」这一**覆盖两按钮**的全局声明）、右槽 x368 / 宽 320（**恰在 `M1 授权重启` 按钮正上方**，放 M1 专属阻塞原因）。两槽**可同时出现**且不重叠 | §8.3 对「联锁状态不可用」只给**一句**全局表述（未要求逐按钮重复）⇒ 落左槽；§6.4 对「M1 处于 latch 态」明写「**按钮正上方**」⇒ 独占右槽。左槽宽 = 右槽起点 − 同组缝（352），本页全部左槽文案实测 ≤ 348 px ⇒ 不越界 | 无（**有意**） |
//! | IL6 | **触发源名映射**：`estop`→`急停`、`door`→`门禁`（UI §3.6 P4「触发源」行）；**其余机器名（含 `flood` / `fire`）取 [`display_safe`] 归一化后的机器名**（小写→大写同族、cmap 外 ASCII→`?`），**不伪造中文名**。残余（如实）：非 ASCII 名（后端直接给中文）**原样透传** ⇒ 含缺字时真机仍是豆腐块，**防线在后端字段命名 / 字库**（与 `p2_config.rs` **PD13** 同口径） | `flood` / `fire` 的候选中文（水浸 / 消防）里 **`水`(U+6C34) / `浸`(U+6D78) / `防`(U+9632) / `火`(U+706B) 逐字实测均不在 cmap 内**，且 §3.6 用字表本身**只列** 急停 / 门禁（不臆造表外中文名）。**不静默**：映射表与未知名的处置**逐条可测**（见本文件单测），且屏上保留可辨认的原文 | 同 IL1（扩字表后可补 `flood` / `fire` 的中文名）；机器名全集真源 = `mupc-core-bin/src/interlock.rs::source_token()` |
//! | IL7 | 状态三灯卡在 `available = false` 时**不**用 [`UnavailableState`] 组件，改为**卡内同源灰度**：图标 = [`UnavailableKind::Interlock`]`.icon()`（`?`）、文案 = 同 kind 的 `.title()`（「联锁状态不可用」）、色 = 同 kind 的 `.accent()`（`#8C98AC`） | [`UnavailableState`] 的固定尺寸是 **992 宽 × 164 高**（`components.rs` 内按 `Dimens::CONTENT_W` 排布），放不进 **236×108** 的灯卡，而 `components.rs` **本批禁改**。取「**同源取值**」（图标 / 文案 / 颜色三通道**全部读自** `UnavailableKind::Interlock`，非另抄）⇒ 与 `UnavailableState` 语义一致、无第二份真源（单测断言两处取值相等） | `components.rs` 收口批：给 `UnavailableState` 增「紧凑 / 自适应宽」形态后改用组件 |
//! | IL8 | 触发源卡在 `available = false` 时用**真** [`UnavailableState`]（`kind = Interlock`），其 `reason` 槽取**空串** | 帧内 `available = false` **不带原因字段**（契约无该槽）⇒ **不臆造**原因文案；`UnavailableState` 的标题已由 `kind` 给定（「联锁状态不可用」） | 若将来帧给出 `available` 的原因，直接注入（本页该槽已是参数化入口） |
//! | IL9 | 触发源**行池 = 4**（`SOURCE_ROW_POOL`）；帧内源数 > 4（契约 `sources: Vec<_>` **未设上限**）⇒ **卡头数量仍显真实 N**、行只铺 4 条；**且卡头补一行可见提示 ` · 还有 N−4 条`**（[`sources_overflow_note`]，B2b-3 代码质量整改 ②） | **上限取值的理由**：后端 distinct token 全集恰为 4 —— `estop` / `flood` / `fire` / `door`（`mupc-core-bin/src/interlock.rs::source_token()`）；行池固定 ⇒ `new()` 一次性建齐、`render()` **不可失败**（同 P1 的 `alarm_rows` 池口径）且 1 Hz **零对象 churn**。**补偿**：UI §6.4（`：599`）明写「**全部源一次性列出，不折叠**」⇒ 超限必须**可见**（不得静默）—— 本行的差额外显即该补偿（该提示词 `还`/`有`/`条` 逐字在 cmap 内）。⚠️ **IL9 的"钉死"是单向的**：`source_token()` 加变体只会让 `source_token()` 的 `match` **编译失败**，**不会**让 `SOURCE_ROW_POOL` 报错 ⇒ 该提示是**唯一的运行期网** | 无（**有意**）；若后端 token 集合扩张到 > 4，**先**同步扩 `SOURCE_ROW_POOL`（在 §3.6 补中文名），提示自动收敛为空串 **✅ PM 已裁定（2026-09-15）**：维持池 = 4 + 「还有 N 条」；UI §6.4 已由「**全部源一次性列出，不折叠**」改为「全部源**计数**一次性列出；行区**最多 4 行**，超出以「`· 还有 N 条`」**可见提示**」，见 UI 附录 **A.7** |
//! | IL10 | 弹层明细取 [`ConfirmDialog`] 的**三段式** `字段 旧值 → 新值`（组件本批禁改，**无** `字段：值` 形态）⇒ §6.4 要求的三项信息按「**当前 → 操作后目标态**」表达；本次操作**不改动**的项取 `新值 = 旧值`（**如实**表达「不变」，不编造目标值） | `ConfirmDialog` 的明细行**恒**渲染四槽（字段 / 旧值 / 箭头 / 新值，颜色亦固定），没有「单值行」口。三段式是本批唯一可行形态；语义仍**具体**（非泛化措辞，满足 §7.3「影响范围 / 明细必须具体」） | `components.rs` 收口批：明细行支持「单值」形态后逐字回契约 |
//! | IL11 | **弹层内**就地红字（UI §6.4 拒绝原因表的「弹层内就地显示」）**不可达** ⇒ 取 `p2_config.rs` **PD7 同款口径**：原因落**页内就地原因带**（红字 24 px `#FF6B6B`），**弹层不自动关闭**（原因常驻可读，用户可「取消」关闭后重试）。⚠️ **本行论证的减损与补正（B2b-3 代码质量整改 ①）**：评审 `PROBE-OCCL` 实测弹层面板底 ≈ y607、就地原因带在 y600–624 ⇒ 原因带**顶部约 7 px 被弹层压住**，而拒绝时弹层**恰恰不关** ⇒「就地可见」在最需要时**被削弱**。补正 = **IL23** ①（同一份原因**同时**走 `layer_top` 的 Toast，无遮挡） | 同上：`ConfirmDialog` 的「影响范围」与明细在**构造期固定**，**没有**可变错误文案口；且关闭弹层不得在 LVGL 事件回调内做 | `components.rs` 补 `set_error()` 后改回弹层内（届时 IL23 ① 的 Toast 通道应**保留** —— 它是遮挡无关的那条） |
//! | IL12 | **控制通道线上路径**：失败时的**具体原因**由 `ControlResponse.message` 承担 —— 契约 `display-proto/src/control.rs` 该字段文档原文：「**人读消息；UI 直接展示（失败时即 EDGE-10 / EDGE-12 要求的「具体原因」）**」⇒ [`P4InterlockPage::show_result`] 就地上屏 `display_safe(message.trim())`（**不吞**，EDGE-12）；空串时退到 §3.6 全局行的「操作失败」（**不造假原因**）。`RejectedPrecondition` **另**置「请求一次状态刷新」标志（[`P4InterlockPage::take_refresh_request`]）—— 被拒即说明屏上观测可能过期，该标志对**各类**前置条件**都正确**，且与文案**解耦**（改了文案也不影响刷新语义）。EDGE-19 的**固定**文案「联锁状态已变化 · 请刷新后重试」由 [`P4InterlockPage::show_conflict`] 承担，它是**显式入口**：只给**客户端本地能判定**「提交时状态已变化」的场合（例如将来页面自己比对 `observed_*` 与当前帧）。⚠️ **PM 裁定（2026-09-16）：不得谎报拒绝原因** —— B3-2b-2 曾按**任务书**在 `control_route` 里给 `RejectedPrecondition` 单开一条臂 ⇒ `show_conflict()`，**该实现已被推翻**（与本节论证冲突：把「状态已变化」之外的拒绝也显成 EDGE-19 固定文案 = **谎报原因**）；现在**所有**业务拒绝（含 `RejectedPrecondition`）**一律**走 [`P4InterlockPage::show_result`]，具体原因由**服务端** `message` 承担 —— 设计 TD:594 明写真·状态变化时服务端返回的 `message` **就是**「联锁状态已变化，请刷新后重试」⇒ EDGE-19 的文案**照样按其本意出现**（由服务端判定，不由客户端猜）。⇒ `show_conflict()` 在**当前生产路径上不可达**（`app.rs` 的那条臂已删除；仅 `ui/tests.rs` 仍作为**显式入口**直接调它、断言固定文案可达）。**当前契约无法自动达成该判定** —— `ControlCode::RejectedPrecondition` 把「状态已变化」与「触发源未复位 / 保持时间不足 / latch / StopPending」**糊在同一个码**里，回执**无**结构化 `InterlockReject` 字段；且 `InterlockReject` 的 **7 个变体里没有「冲突」变体** ⇒ 「B3 自行解析出 `InterlockReject`」对该场景**不可实现**（**契约级缺口**，属 F/G/H/I/J/K 与契约所有者的责任）。⇒ 本页**不假设**该固定文案会被自动触发：它在屏上出现**当且仅当**外部显式调用了 `show_conflict()`。⚠️ **M9**（B3-2b-2 订正）：置位点 2 处（`show_conflict` / `show_result` 的 `RejectedPrecondition` 分支）、**生产读取 1 处**（`app.rs::tick_console` ⑤ 每拍先取，`true` 即把 `next_poll_ms` 置 `None` ⇒ 下一拍补发一次读通道 `GET`；判据 `apply_refresh_request` 有单测、生效次数计入退出统计行 `p4_refresh`）⇒ 该**前向 API 已被消费**（此处原先"M9 生产读取 0 处、B3 必须消费"的陈述**已过期**，B3-2b-2 接线时订正） | 契约冻结（`display-proto` 不得改）；不做「按消息串猜语义」的脆弱解析（猜错即**谎报原因**，与 §2.6「绝不造假」冲突） | 若 `display-proto` 在控制回执中增 `reject: Option<InterlockReject>`（或为 EDGE-19 单列一个 `ControlCode` 变体），则 [`P4InterlockPage::show_result`] 直接分派（单一分派点），固定文案即可自动可达 |
//! | IL13 | 回执 → 展示态的映射（**单一映射点** `Core::apply_ack`）：`latched := ack.latched`、`stop_failed := !ack.stopped`（`InterlockOpAck.stopped` 的契约语义是「操作后停机**确认**态」）；`available` / `enabled` / `sources` / 两灯**不变**（回执不带，等下一帧，最坏 ≤1.35 s） | §6.4「成功」行要求「用回执 `applied` **立即**刷新，不等下一帧」（F17.6 / IL-02）⇒ 回执能覆盖的两项立即刷；其余字段回执确无载体（契约冻结）⇒ 不臆造、由下一帧补。**契约未显式声明** `stopped` 与 `stop_failed` 互补 ⇒ 若后端语义有出入，只改 `Core::apply_ack` 一处 | 契约若明示互补关系，此处改为显式字段 |
//! | IL14 | **保持时间倒计时**（UI §6.4「保持时间不足」行：按钮旁显剩余秒数）**已实现**，时钟由 [`P4InterlockPage::tick`] 注入：收到 `HoldNotElapsed { remaining_secs }` 时记剩余秒数并置「待取基准」标志，**首个 `tick`** 取基准 `Instant`，其后每拍按已过秒数递减（`saturating_sub`，不 panic）；**页面不读 `Instant::now()`**（`Toast::new` 的既有行为除外，同 `p2_config.rs` **PD20**）。倒计时到 `0` 只显示「还需 0 秒」，**不**自作主张放行（是否可操作仍由后端前置判定）；**跨帧存续与否见 IL27**（新帧改变展示态即清，`ts_ms` 不计） | 帧内只有 `release_hold_secs`（**须保持**的时长），**没有**「已保持多久 / 何时复位」⇒ 无法从帧推出绝对剩余时间；唯一可得的绝对量是后端拒绝里的 `remaining_secs` ⇒ 以「拒绝后的首拍」为基准推进是**唯一**不臆造的做法 | 若帧增「源复位时刻 / 已保持秒数」，改为帧驱动（届时删掉基准捕获） |
//! | IL15 | 提交中（[`P4InterlockPage::set_submitting`]）两按钮 `disabled` 且**无按钮级就地原因** | UI §6.4 未定义「提交中」态的就地文案（§3.6 亦无该行）⇒ 只置灰、**不造文案**；防重由 `ConfirmDialog` 自身的 `Debounce`（500 ms，TT-10）与按钮禁用共同承担 | 无（**有意**） |
//! | IL16 | `observed_sources` 取帧内**全部**源名（含未触发），口径与后端 `status_sources()`（列**全部** distinct token）一致 | 只取 `tripped` 的集合会把「某源**复位**」与「源本来就没有」的区分度降低；全量名列表是更严的并发检查（EDGE-19「状态已变化」的判据） | 无（**有意**） |
//! | IL17 | 注入侧 / 契约侧的**自由文本**（源名、拒绝原因、回执 `message`）上屏前一律过 [`display_safe`] | 这些字面量**不在** `ui/**` 的源码字面量走查面内（来自 `display-proto` 或运行时帧），直上屏含 cmap 外 ASCII（小写 / `-`）即豆腐块（同 `pages/mod.rs` **D9** 与 `p2_config.rs` **PD13**）；`display_safe` 只改写 ASCII，**非 ASCII 缺字挡不住**（残余见 IL6 / PD13） | 同 D9（扩字符集后改写面自然收窄） |
//! | IL18 | `人工释放联锁` 在 `available && enabled` 时**恒可用**（除提交中）：**不**按「触发源未复位 / 保持时间不足 / 未处于 latch」等在本地预判置灰 | ① 释放是**安全正向**操作（清 latch），本地预判置灰会挡住该路径；② §6.4 的 EDGE-12 恰恰要求「把**具体**拒绝原因告诉现场」—— 本地预判会**替代**后端的结构化原因（现场只看到灰按钮、看不到「哪个源没复位」）⇒ 与「不得静默失败」相悖 | 无（**有意**） |
//! | IL19 | `ControlResponse::duplicate`（幂等命中）**本页零读取** ⇒ **漏覆盖**（与 `p2_config.rs` **PD18** 同族） | UI §3.6 的 P4 用字表与全局 Toast 行**都没有**「幂等命中 / 重复请求」的文案 ⇒ 无字可上屏；也不能凭一比特**造**一句文案（**绝不造假**） | **§3.6 需补一行文案** ⇒ 收口于 B2c 之后的「字体码表 + 文案统一收口批」（与 IL1 同批）；届时在 [`P4InterlockPage::show_result`] 里读 `resp.duplicate` 并弹提示 **✅ PM 已裁定（2026-09-16）**：见 UI 文档附录 **A.8** |
//! | IL20 | `SOURCE_ROW_POOL` / `INNER_W` / `CARD_HEAD_H` / `CARD_INSET` 与 `p2_config.rs` / `p6_system.rs` **同式重复**（各页各持一份） | **不动**（KISS + 两文件本批**禁改**）：三者都是 `theme` 常量的**一格推导**，上收需要一个新共享模块（结构变更，超出本批范围，与 `p2_config.rs` 的 PD19 同一处置） | **B2c 之后**统一上收 `ui/pages/mod.rs`；在此之前**任一处改 `theme` 派生式必须三处同改** |
//! | IL21 | [`P4InterlockPage::show_audit_unavailable`]（EDGE-18）**除 Toast 外另落就地红字**（操作条上方同一文案） | EDGE-18 只要求 Toast ⇒ 这是**超出规格的 additive 行为**，**保留**：① 与 **IL11** 同款取向（Toast 会过期，而 fail-closed 的「操作**未执行**」这一结论须常驻可读 —— 现场看到灰按钮时能立刻知道原因）；② **零新增**：落点（就地原因带）与文案（转出 `TEXT_AUDIT_UNAVAILABLE`）都是既有件，未新建对象、未添第二份字面量 ⇒ 无屏上冗余（Toast 与红字同文案、位置不同） | 无（**有意**）；若 PM 裁定 Toast 足够，删去 [`P4InterlockPage::show_audit_unavailable`] 里的 `set_plain_reason` 一行即可（其单测断言同步收） |
//! | IL22 | latch 的 `StatusChip` **增了图标通道**（`●` / `○` / `?` 三态，见 [`latch_chip_icon`]） | UI §5.3 对胶囊只要求 **text + color** 两通道 ⇒ 这是**超出规格的 additive 行为**，**保留**：① `StatusChip::new(parent, w, icon, text, skin)` 的**签名强制**要求 icon 实参（`components.rs` 本批**禁改**，无「省略图标」的口）；② 契约未指定字形 ⇒ 取与灯类同族的三态（实心 / 空心 / 问号），不可用态取 `?`、**不**复用 `✓` / `⚠`（与 §8.3「不得复用」一致）；③ 有单测锁住三态互异与不可用态的字形（`p4_interlock.rs::tests::latch_chip_never_says_unheld_when_unavailable`） | 若 `StatusChip` 补「无图标」构造口，可改为 text + color 两通道（须同步改 §5.3 走查与上述单测） |
//! | IL23 | **失败态「具体原因」的双通道上屏**（B2b-3 代码质量整改 ①③）：① 回执 `message` **同时**送进 `Toast`（失败 Toast 的文案由 §3.6 全局行的通用「操作失败」**改为该具体原因**；`message` 为空时**仍**退到「操作失败」）；② 就地原因**左槽**的宽度在**右槽不显**时由 352 px 加宽到整幅 **992 px**（[`REASON_LEFT_FULL_W`]），右槽在显时收回 352（IL5 不重叠） | **①的根因（评审 `PROBE-OCCL` 实测）**：弹层面板底 ≈ y607、就地原因带在 y600–624 ⇒ 原因带**顶部约 7 px 被面板压住**，而 EDGE-12 要求「拒绝时弹层不自动关闭 + 具体原因**就地可见**」—— 恰在最需要它时被遮。`Toast` 在 `layer_top`、且**弹层之后创建**（同图层内后建者绘在上）⇒ 是**无遮挡**通道。**①的代价（如实登记）**：失败 Toast 的**通用**文案被具体原因**取代**（§3.6 全局行仍保留该串，作空 `message` 的兜底 + 就地原因带的兜底）。**③**：`message` 是**自由文本**，长文本在 352 px 槽内被 `LongMode::DOTS` 截成省略号。**实测口径**（按 `fonts/lv_font_noto_sc_24.c` 的 `adv_w` 求和 = 自然宽上界，不计 kerning；由 `ui/tests.rs::measured_text_px` 在用例里复算）：24 px 档**每汉字 ≈ 24 px** ⇒ 352 px 只容 ≈14 字、992 px 容 ≈41 字；例：25 字自由文本「审计服务连接超时，操作未执行：请检查审计服务后重试」= **600 px**（352 槽必截断，992 槽整行放下）。 | **残余（如实）**：① Toast 文本槽仅 **400 px**（≈16 字）且同样 `DOTS` ⇒ **超长文本在 Toast 里也会截断**；② 两槽同时在显（latch 态）时左槽仍 352 px ⇒ **> ≈14 字的 `message` 在屏上仍是省略号**（完整文本此时**无**不截断通道 —— `message` 是自由文本，其长度不受本页控制）。**根治**需 §3.6 给「长原因」的落点（或 `ConfirmDialog` 补 `set_error()`） **✅ PM 已裁定（2026-09-16）**：见 UI 文档附录 **A.8** |
//! | IL24 | **成功文案取 §6.4 成功行（=`§3.6` 全局 Toast 行）而非 §3.6 P4 行**：实现上屏「`已释放联锁`」/「`已授权重启`」（[`TEXT_TOAST_RELEASED`] / [`TEXT_TOAST_ACKED`]），**未**取 §3.6 P4 行的「`联锁释放成功`」/「`授权成功`」 | 两处出处：UI 设计文档 §3.6 **P4 行**（`：257`，P4 页用字表）写「联锁释放成功 / 授权成功」；同节 **全局对话框 / Toast 行**（`：265`）与 **§6.4「成功」行**（`：613`）写「已释放联锁 / 已授权重启」。**取舍**：本页这两个串落在 **Toast** 上，而 Toast **是全局件** ⇒ 其文案由**全局 Toast 行**规范（§6.4 亦逐字给出同一对），§3.6 P4 行给的是「P4 页**用字**集合」，未区分落点。**该分歧此前未登记**（B2b-3 代码质量整改 M4 补登） | 若 PM 裁定 P4 行走字优先，改 [`OpKind::toast_ok`] 两处字面量即可（`ui/tests.rs` 的成功 Toast 断言同步改） **✅ PM 已裁定（2026-09-16）**：见 UI 文档附录 **A.8** |
//! | IL25 | 弹层**打开失败**时**屏上无提示**，只写 stderr（且**已节流**：第 1 次 + 其后每 [`OPEN_FAIL_LOG_EVERY`] 次一次，见 [`Core::note_open_failure`]）—— B2b-3 代码质量整改 M6 | **不加屏上提示是有意的**：该路径的触发条件就是「LVGL 建不出对象」（内存池耗尽一类），而**任何**屏上提示（`Toast` / 标签）都**要求先建对象** ⇒ 在同一失效域内**不可靠**；写 stderr 是唯一不会二次失败的通道。**节流**：不节流则用户每点一次「人工释放联锁」都灌一行（无界日志） | 若将来有「复用既有标签」的诊断槽，可把失败计数上屏（须有 §3.6 文案） **✅ PM 已裁定（2026-09-16）**：见 UI 文档附录 **A.8** |
//! | IL26 | 触发源**机器名 → 中文名**的匹配是 **ASCII 大小写不敏感**（`eq_ignore_ascii_case`，见 [`source_label`]）—— B2b-3 代码质量整改 M2 | 契约 `source_token()` 恒产小写 token，但帧 / 拒绝里的 `name` 是**自由文本**：写成 `ESTOP` / `Door` 时精确匹配落空 ⇒ 屏上落成机器名（大写字形勉强可读），而本页**明明知道**它就是「急停 / 门禁」。**只对 ASCII 生效** ⇒ 中文名（如后端直接给「急停」）不受影响。**残余（如实）**：**分隔符变体**不在映射表内 —— `e_stop` / `e-stop` 长度或字符不同 ⇒ 仍落 `display_safe`（`_` → 短破折 `–`、小写 → 大写同族），屏上呈 `E–STOP`。本页**不猜**（不在 IL6 的「只增不改」映射表里加变体） | 无；若后端**确定**只发小写 token，可退化为精确匹配（须同步改单测）；若需覆盖 `_`/`-` 变体，须在 [`SOURCE_LABELS`] 逐条增行（并重跑 `source_key(` 的计数自证） |
//! | IL27 | **新帧改变展示态时清掉陈旧倒计时**（[`Core::apply_section`] 经 [`section_display_eq`] 判定）—— B2b-3 代码质量整改 M3 | 评审 `PROBE-CD3`：倒计时到 0 后跨帧常驻「还需 0 秒」，且左槽优先级高于 `last_reject` ⇒ 会**顶掉**新到的结构化拒绝原因。**为何按"展示相关字段变化"而不是"每帧清"**：`InterlockSection::ts_ms` **每帧都变**，若纳入比较则 1 Hz 心跳每秒清一次 ⇒ IL14 的倒计时活不过 1 s（判据**忽略 `ts_ms`**，帧内容相同则不清）。**为何不清按钮可用性**：按钮是否可操作**恒由后端态判定**（本页从不本地预判，IL18）⇒ 清理只影响文案通道 | 若帧增「源复位时刻 / 已保持秒数」，倒计时改为帧驱动（届时本处置整体删除，见 IL14） |
//! | IL28 | 意图回调槽的**注册**（`set_on_release` / `set_on_ack_m1`）与**触发**（`fire_release` / `fire_ack_m1`）**两侧统一**用 `try_borrow_mut`（重入时**静默跳过**）—— B2b-3 代码质量整改 M5 | 此前注册侧用 `borrow_mut`：在**槽被借用期间**（= 正在 `fire_*` 里调用户回调）再注册会 **panic**，而该 panic 出自 LVGL 事件回调、被事件桥 `catch_unwind` **静默吞掉**（屏上无任何迹象）—— 与本页「回调绝不 panic」的纪律相悖。**重入语义**：静默跳过本次注册（旧回调保留），**不 panic、不上屏** | 无（**有意**）；若需可观测，可在跳过处 `debug` 级日志（本批不引入） |
//! | IL29 | **联锁总态卡在 488 px 内的「几何不可满足性」总览**（本批 T21c-1 全部取舍的入口；子项 ①–⑦ 逐条在下方）。设计 §15.4 声明该卡「**既有内容 / 文案 / 字号不变**」、UI §6.4.1 声明「**既有联锁区…一律不动**」，而 UI §6.4.1 同时是**几何单一真源**且把该卡定为 **488×164** ⇒ **三条硬约束的交集为空**：卡内容区仅 **454 px**（488 − 2×17），而既有通道求和 = 标题 **112**（4 字 × 28）+ 主值图标 **72** + 96 px 主值词最长 **672**（7 字）+ latch 胶囊 **236** + 文字冻结角标 **192** —— 单是「标题 + 角标 + 胶囊」= **540 > 454**。（评审 N-1 的 **G-13** 即本行的契约根因） | 三条互斥约束：① UI §6.4.1 的 488 宽；② 设计 §15.4 的「文案 / 字号不变」；③ UI §8.3「保留最近有效帧并**每区块**打 `冻结` 角标」的适用范围**明列 P4**。**本批的序（产品裁定 2026-09-25）**：① 取为**硬约束**；② 按「本态（不可用）例外」收窄（见 ①）；③ 按「**图标-only** 形态」保形、只去文字通道（见 ②）；④⑤⑥⑦ 为同批实测暴露的连带栅格 / 落点偏差 | 逐条 ①–⑦。**无工作单元**（均属**有意**）；其中 ③⑤ 与既有 `theme` 缺口上收批（D3 / PD5 / IL3 / IL20）同批，④ 与 `theme::icon_slot(72) ⇒ 64` 缺口（N-3）同批。**回写**：设计 §15.4 / UI §6.4.1 / UI §8.3（T21c-1-r1，2026-09-25） |
//! | IL29① | **「不可用」态的总态词取缩短形态** `TEXT_STATE_UNAVAILABLE_SHORT`，**不**取全串 `TEXT_STATE_UNAVAILABLE`（「联锁状态不可用」）—— 两条常态词「已联锁 / 未联锁」**逐字不变** | **几何实测**：96 px 档下全串 7 字 × 96 = **672 px** > `STATE_TEXT_W` = 454 − 72 − 16 = **366**（`LongMode::DOTS` ⇒ 屏上只剩约 2 字 + `…`；宽度按 `fonts/lv_font_metrics.txt` 的 `adv_w` 逐字求和 = **上界**）。**语义不减**（产品裁定 2026-09-25）：完整语义由 **① 卡标题「联锁状态」+ ② 就地原因带全串（该态下 `refresh_actions` 的右槽 `right = None` ⇒ 左槽扩到 `REASON_LEFT_FULL_W` = **992 px** 并置全串）+ ③ 三态色 / 图标冗余** 三重承担 | **无工作单元**（有意）。**为什么另立常量而不改全串**：全串另有三个落点（就地原因带 / 触发源卡的 `UnavailableState` / latch 胶囊）且被「与 `UnavailableKind::Interlock::title()` 逐字相等」的同源锁含住 ⇒ 改它会连带破坏三处语义。**回写**：设计 §15.4 + UI §6.4.1 已注明本态例外（T21c-1-r1） |
//! | IL29② | **联锁总态卡的 EDGE-03 / EDGE-20 角标保留、但改「图标-only」形态**（28 px 徽标，`pages::frozen_icon_badge`，落在标题与 latch 胶囊之间的空槽）—— 此前 T21c-1 首轮曾**整体移除**该角标（评审 FAILED 的 F2），本轮**恢复**：**只有文字通道被去掉，打标语义与可见性判据一律保留**（判据仍是 `pages::frame_mark` 单一真源） | **几何**：卡头行内「标题 112 + 文字胶囊 `FROZEN_CHIP_W`(192) + latch 胶囊 236」= **540 > 454** ⇒ 文字胶囊与 latch 胶囊**必然重叠**；换 28 px 图标后三件 = 112 < **174**（`STATE_FROZEN_X`）< **202** < **218**（胶囊左缘）⇒ **互不重叠**。**实现事实**：`components.rs` 的 `StatusChip` **无 `set_icon`**（图标只能建时给）且文字槽固定 x=32 ⇒ 无法承担运行期换字形，故取「容器 + 单图标标签」两件（皮肤复用 `ChipSkin::WARNING` 同一份样式，**零新色值**）。**产品裁定（2026-09-25）**：取图标-only | **收口**：`components.rs` 若补 `set_icon` + 自适应宽，可回到单件形态。**代价（如实）**：失去「冻结 / 数据过期」的**文字**区分 ⇒ 改由图标通道 ⚠ / `!` 承担（`pages::frozen_mark_icon`，与 P1 的两个角标**同源**；⚠ = 冻结 / `!` = 数据过期），读口 = `state_frozen_visible()` + `state_frozen_icon()`（**不设**恒空的 `text()` —— 零判别力的 API 不保留）。**回写**：UI §8.3 + UI §6.4.1（T21c-1-r1） |
//! | IL29③ | **总览带 / 视口栅格偏差**：单卡外缘高 **162**（UI §6.4.1 写 164，−2）；滚动视口高 **342**（UI 写 340，+2）；三卡行与 §A 各卡的页内 y 由前两项累积（如 §A 起 y 随视口底 +24 顺移） | **root cause = `theme` 缺 2 px 档**（与 `pages/mod.rs` **D3**、`p2_config.rs` **PD5**、本表 **IL3** 同族）⇒ 本页坚持「**不写裸规格值**」（`ui/tests.rs` 两张静态网明令禁止）。**关键不变量成立**：视口 **342 ≥ 340**，且 §B 既有全量 = `触发源卡 202 + GAP_GROUP 16 + 灯卡 106 = 324 ≤ 342` ⇒ **既有联锁区首屏全可见**（UI §6.4.1 的核心承诺不破） | 与 `theme` 缺口上收批同批（D3 / PD5 / IL3 / IL20 / IL29⑤）。**回写**：UI §6.4.1（T21c-1-r1） |
//! | IL29④ | **火警等级卡图标取 `Dimens::ICON_SM`(28)**（UI §6.4.1 写 **72 px**） | **实际依据 = 主值槽余量 + 档位缺口**：取 28 档 ⇒ 主值槽 `FIRE_VALUE_W` = 454 − (28 + 16) = **410**（4 字枚举 × 64 = 256 ⇒ 余量 154）；取 72 档 ⇒ 366（余量 110）。且 `theme::icon_slot(72)` **本就降档渲染为 64**（该缺口与 IL3 / IL20 同族）。⚠️ **订正（T21c-1-r1 实测）**：本常量原注「64 / 72 档会**挤掉** 4 字枚举文案」**不成立** —— 按 64 px 档逐字复算 256 ≤ 366 仍有余量；该注已改为上述真实依据（余量 + 档位缺口），**不保留不实因果** | 与 `theme` 缺口（`icon_slot(72) ⇒ 64`）上收批同批（承评审 N-3）；届时统一裁定 64 / 72 档取值 |
//! | IL29⑤ | **§A 四组行高一律取 `Dimens::ROW_LOG_H`(44)**（UI §6.4.1 写 A1 / A3 行高 **36**、A2 / A4 行 **40**） | `theme` **只有 44 一档「列表行」**（与 IL3 的「缺 2 px 档」同族：不为凑像素写裸值）。**连带后果（如实）**：A1 卡高 = 卡头 44 + **17 行 × 44** + 上下内边距 34 = **826**（UI 写 616；其中 **17 = 1 行段顶通告 + 16 位行**，见本文件 `A1_CARD_H`）；A 组总高随之变大 ⇒ 滚动更长 —— 属「滚动区已超首屏」的正常后果，**不破坏任何不变量**（§A 本就不承诺首屏可见，见 ⑦） | 与 `theme` 缺口上收批同批（D3 / PD5 / IL3 / IL20 / IL29③）。**回写**：UI §6.4.1（T21c-1-r1） |
//! | IL29⑥ | **下钻明细未做「窗口化」**：**20 行 × 9 格 = 180 个格对象一次性建齐**（`DRILL_ROW_POOL` = 服务端 `page_size` 缺省 20），与设计 §15.5.3「每段列表一律**窗口化**（可视行 ×1.5）」不符。**另**：探测器状态列的**两个位名上提到表头**（行内格位放不下两个位名 —— 位名 120 + 「非活跃」72 = 192 > 164 半槽） | **窗口化在本层结构性不可实现**：复用行对象**必须先知道滚动位置**，而薄层的 `EventCode` **未镜像 `LV_SCROLL`**（`pages/mod.rs` 的 R2）⇒ 收不到滚动事件。⚠️ **IL9 的单向钉死同款**：该理由**只在代码注释里**，设计侧原先仍要求窗口化 ⇒ **已回写**设计 §15.5.3（T21c-1-r1：写明本层无滚动事件 ⇒ 下钻行池整池建齐、窗口化待薄层补 `LV_SCROLL` 后启用） | 薄层补 `LV_SCROLL` 事件镜像后启用窗口化（届时删 `DRILL_ROW_POOL` 的整池建齐与真机余量风险，见 **IL29③⑤ 之外的 D-6 / R-34**）。**残余（如实）**：对象预算因此 742 → **981**（余量 ~0.8 %，见 `ui/tests.rs` 的 `SHELL_OBJECT_BUDGET` 与设计 §15.9 **R-34** 的真机 `lv_mem_monitor` 复核） |
//! | IL29⑦ | **§A 消防区在源数 = 4（触发源卡变高）时首屏不全可见** —— §B 全量仍 `202+16+106 = 324 ≤ 342` 首屏全可见，但 §A 的第一组会被推到视口外 | **与设计一致、无需回写**：UI §6.4.1 原文「新增 §A 只在**向下滚动后**出现」，只承诺 **§B** 首屏全可见（该承诺成立）；本页**不调整**视口或行高去凑 §A 的首屏可见（那会破坏 §B 的承诺或引入裸值） | **无**（有意）。如实登记以免被读成"§A 也应首屏可见" |
//!
//! ## 纪律（逐条对应设计要求）
//!
//! - **零文本输入**（红线）：本文件只出现 `lv_button` / `lv_label` / `lv_led`（经 `TextButton` /
//!   `LedIndicator` / `StatusChip`）—— `lv_keyboard` / `lv_textarea` / `lv_spinbox` **零出现**
//!   （`ui/tests.rs::p4_static_constraints`）；
//! - **无裸色值 / 裸尺寸**：一切外观经 [`theme`]；本页专属栅格常量集中在下方 `const` 块，
//!   **逐条由 theme 常量推导**（`ui/tests.rs::ui_layout_setters_use_theme_constants` /
//!   `ui_const_i32_definitions_derive_from_theme` 两张网扫本文件）；
//! - **所有权纪律**：凡 `Obj` / `Label` / `LedIndicator` / `StatusChip` / `TextButton` /
//!   `EmptyState` / `UnavailableState` / `ConfirmDialog` / `Toast` 返回的**拥有型句柄**一律
//!   存进结构体字段（`Core` / `SourceRow` / `StatusCard`），**绝不**只作局部变量 ——
//!   否则构造器返回时即被 `Drop`、LVGL 级联删除整棵子树（表现为"界面空白但无报错"，
//!   本项目出过此类 UAF 级缺陷）；
//! - **回调纪律**：回调内**不 panic**（无 `unwrap` / 无越界索引）、不做阻塞 I/O、
//!   不删除自身宿主；**不在 LVGL 事件回调内关闭弹层**（`ConfirmDialog::close` 的要求）
//!   —— 取消走 [`P4InterlockPage::tick`] 的延迟关闭，`show_result` / `render` 在事件之外；
//! - **共享态纪律**：可变共享态一律 `Rc<Cell<_>>` / `Rc<RefCell<_>>`（`Cell::clone` 是**值拷贝**
//!   ⇒ 死写，本项目踩过），回调只持 `Weak<Core>`（避免 `Rc` 环令句柄永不落地）；
//! - **不提供跨线程 API**：本页所有类型都含 LVGL 句柄（自动 `!Send` / `!Sync`），全部调用
//!   必须在事件循环线程内（设计 §5.2 不变量 4）；
//! - **渲染期不加对象（一处例外，如实登记）**：源行 / 三卡 / 状态件 / 弹层 / Toast 一律
//!   **预建或在事件外建**，`render`（1 Hz）**只改文本 / 颜色 / 可见性 / 尺寸**，不新建也不删除
//!   对象（§7.5 脏区与动效纪律）。**例外**：`SourceRow::apply` 在**上屏名变化**时会**重建**
//!   该行的两支 `LedIndicator`（`2` 个对象）—— 根因是 `LedIndicator`（`components.rs`，本批
//!   禁改）**没有 `set_text` 口**，上屏名只能建时给；该路径**逐条可测**（见 `SourceRow::build`
//!   的结构体注），其余帧（名不变）零创建。**不是**"模块级零创建"这一强命题。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use mupc_display_proto::peripherals_labels::{
    ui_text, FIRE_DETECTOR_STATE_BITS, FIRE_DET_TEMPLATE_LABELS, FIRE_TRIGGER_BITS, GROUP_TITLES,
};
use mupc_display_proto::{
    BitMeta, CatalogPoint, CatalogStation, ControlCode, ControlResponse, Decompose, FieldFlag,
    FireDetectorPage, InterlockOpAck, InterlockOpPayload, InterlockReject, InterlockSection,
    PeriphRole, PeripheralCatalog, PeripheralStation, PeripheralsSection, PointValue,
    FIRE_DET_POINTS_PER_UNIT,
};

use crate::lvgl::obj::Obj;
use crate::lvgl::style::{Color, Style};
use crate::lvgl::widgets::{self, Label, LongMode, ScrollContainer, TextButton};
use crate::lvgl::LvglError;
use crate::state::{self, PeriphView, StationState};
use crate::ui::components::{
    ConfirmDetail, ConfirmDialog, ConfirmSpec, EmptyState, LedIndicator, StatusChip, Toast,
    ToastTone, UnavailableKind, UnavailableState,
};
use crate::ui::pages::p2_config;
use crate::ui::pages::{
    decor, display_safe, label, layout_box, set_style_index, set_visible, show_only, text_label,
    PageInput,
};
use crate::ui::theme::{self, ChipSkin, ConfirmLevel, Dimens, Palette, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 上屏文案（UI §3.6 P4 行 + 全局 Toast 行；**落笔前逐字在 `fonts/lv_font_cmap.txt` 核对**）
//
// 与契约串的偏差逐条登记在文件头 `IL1~IL22`（缺字改写 / 口径 / 尺寸 / 降级），此处只放成品串。
// 码表覆盖率走查见 `ui/tests.rs::ui_texts_covered_by_font_cmap`（基线 = 生成字体的实际 cmap，
// 待查集合 = 扫 `ui/**` 源码字面量）。
// ═══════════════════════════════════════════════════════════════════════════

/// 联锁总态卡卡头（UI §6.4 线框 `Y90` 左侧）。
pub const TEXT_CARD_STATE: &str = "联锁状态";
/// 三态总态词之一：**已联锁**（`#FF6B6B` + `⚠`）。UI §3.6 P4「总态」行。
pub const TEXT_STATE_LATCHED: &str = "已联锁";
/// 三态总态词之二：**未联锁**（`#2FDB8A` + `✓`）。
pub const TEXT_STATE_UNLATCHED: &str = "未联锁";
/// 三态总态词之三：**联锁状态不可用**（灰 `#8C98AC` + `?`）。
///
/// **本页最重要的一个字符串**（UI §8.3 专行 / F16.6 / IL-01）：「不可用」= **无法获知**，
/// 「未联锁」= **确知安全** —— 二者**不得互替**；`available = false` 时屏上**只能**出现本串。
/// 单测断言它与 [`UnavailableKind::Interlock`]`.title()` **逐字相等**（同源 ⇒ 无第二份真源）。
///
/// **落点**：卡标题「联锁状态」旁的**就地原因带**（992 px 全串）、触发源卡的
/// [`UnavailableState`]，以及 latch 胶囊的不可用态（[`latch_chip`]）。
/// ⚠️ **不再是总态卡 96 px 主值词** —— 那个槽只有 366 px，见
/// [`TEXT_STATE_UNAVAILABLE_SHORT`]。
pub const TEXT_STATE_UNAVAILABLE: &str = "联锁状态不可用";
/// 总态卡的**不可用态主值词**（3 字 = 288 px ≤ `STATE_TEXT_W`(366) ✓）——
/// **`IL29①` 的缩短形态**（产品裁定 2026-09-25）。
///
/// # 为什么必须另立一条（而不是改 [`TEXT_STATE_UNAVAILABLE`]）
///
/// ① 那个全串有**别的落点**（就地原因带 992 px / 触发源卡的 `UnavailableState` / latch 胶囊），
/// 且被"与 [`UnavailableKind::Interlock`]`.title()` 逐字相等"的同源锁含住 ⇒ 改它会连带
/// 破坏三处语义；② 总态卡 96 px 档**经实测放不下全串**：「联锁状态不可用」7 字 × 96 =
/// **672 px** > `STATE_TEXT_W`(454 − 72 − 16 = **366**)，`LongMode::DOTS` 下屏上只剩约
/// 2 字 + `…`（宽度按 `fonts/lv_font_metrics.txt` 的 `adv_w` 逐字求和 = 上界）。**完整语义
/// 由卡标题（[`TEXT_CARD_STATE`] = 「联锁状态」）+ 就地原因带全串 + 三态色/图标冗余承担**。
/// ③ 两个常态词（[`TEXT_STATE_LATCHED`] / [`TEXT_STATE_UNLATCHED`]，各 288 px）**未改**。
pub const TEXT_STATE_UNAVAILABLE_SHORT: &str = "不可用";
/// 总态图标：已联锁（§3.6 符号集内）。
pub const ICON_STATE_LATCHED: &str = "⚠";
/// 总态图标：未联锁。
pub const ICON_STATE_UNLATCHED: &str = "✓";
/// 总态图标：不可用 —— **不得**复用 `✓` 或 `⚠`（§8.3 明文）。
///
/// 单测断言它与 [`UnavailableKind::Interlock`]`.icon()` 相等（同源）。
pub const ICON_STATE_UNAVAILABLE: &str = "?";
/// 实心几何字形（`●`，§3.6 符号集内）：已触发源 / latch「已保持」胶囊。
pub(crate) const ICON_FILLED: &str = "●";
/// 空心几何字形（`○`，§3.6 符号集内）：未触发源 / latch「未保持」胶囊 / 空态。
pub(crate) const ICON_HOLLOW: &str = "○";
/// latch 胶囊：已保持（底 `#3A2E12` 字 `#FFD75E` = [`ChipSkin::WARNING`]）。UI §3.6 P4「总态」行。
pub const TEXT_LATCH_HELD: &str = "自锁保持 · 已保持";
/// latch 胶囊：未保持（底 `#1B2942` 字 `#A6B6D6` = [`ChipSkin::NEUTRAL`]）。
///
/// ⚠️ **只有在 `available = true` 时才可出现**（§8.3：不可用时胶囊转灰、**不得**显示「未保持」）
/// —— 结构上由 [`latch_chip`] 的**分支**保证（`Unavailable` 分支**取不到**本串）。
pub const TEXT_LATCH_UNHELD: &str = "自锁保持 · 未保持";
/// 触发源卡卡头（UI §3.6 P4「触发源」行）。
pub const TEXT_SOURCES_TITLE: &str = "触发源";
/// 触发源空态（UI §6.4 / IL-01.2）：`EmptyState`。
pub const TEXT_SOURCES_EMPTY: &str = "当前无联锁触发源";
/// 触发源行状态词：已触发。
pub const TEXT_SRC_TRIPPED: &str = "已触发";
/// 触发源行状态词：未触发。
pub const TEXT_SRC_UNTRIPPED: &str = "未触发";
/// 触发源中文名：急停（UI §3.6 P4「触发源」行）。
pub const TEXT_SRC_ESTOP: &str = "急停";
/// 触发源中文名：门禁（同上）。
pub const TEXT_SRC_DOOR: &str = "门禁";
/// 源名列表分隔符（cmap 内；与 `components::TEXT_WARN_FIELD_SEP` 同款 `/`）。
pub(crate) const TEXT_NAME_SEP: &str = "/";
/// 子句分隔符（cmap 内 `·`；替代缺字的全角标点，见 **IL1**）。
pub(crate) const TEXT_CLAUSE_SEP: &str = " · ";
/// 触发源**超出行池上限**时的可见提示词（` · 还有 N 条`）—— **[`SOURCE_ROW_POOL`] 上限的补偿**。
///
/// **存在理由（IL9）**：行池恒 4 行（后端 distinct token 全集 `estop`/`flood`/`fire`/`door`），
/// 帧内源数 > 4 时只铺 4 行；若不在屏上说出差额，「卡头数量 ≠ 行数」这件事就**静默消失**，
/// 与 UI §6.4（`：599`）「**全部源一次性列出，不折叠**」的明文要求冲突。
/// `还`(U+8FD8) / `有`(U+6709) / `条`(U+6761) **逐字实测在** `fonts/lv_font_cmap.txt` 内。
pub(crate) const TEXT_OVERFLOW_MORE: &str = "还有";
/// 触发源超出提示的量词（见 [`TEXT_OVERFLOW_MORE`]）。
pub(crate) const TEXT_OVERFLOW_UNIT: &str = "条";
/// 状态三卡卡头之一：停机失败（UI §6.4 线框）。
pub const TEXT_CARD_STOP: &str = "停机失败";
/// 状态三卡卡头之二：故障灯。
pub const TEXT_CARD_FAULT_LAMP: &str = "故障灯";
/// 状态三卡卡头之三：运行灯。
pub const TEXT_CARD_RUN_LAMP: &str = "运行灯";
/// 三卡值：正常（`✓` 绿 `#2FDB8A`）。
pub const TEXT_STOP_OK: &str = "✓ 正常";
/// 三卡值：停机失败（`×` 红 `#FF6B6B`；`✗` 缺字见 **IL2**）。
pub const TEXT_STOP_FAIL: &str = "× 停机失败";
/// 三卡值：灯亮（故障灯 `#DC3545` / 运行灯 `#28A745`；色由卡决定，串共用）。
pub const TEXT_LAMP_ON: &str = "● 灯亮";
/// 三卡值：灯灭（`#5F6368` 空心）。
pub const TEXT_LAMP_OFF: &str = "○ 灯灭";
/// 三卡值：未知（`#8C98AC`）—— `fault_lamp` / `run_lamp` 为 `None` 时的**唯一**合法产出，
/// **不得**臆造为「灯灭」（契约 `Option<bool>` 的三态语义）。
pub const TEXT_LAMP_UNKNOWN: &str = "? 未知";
/// 固定操作条左按钮：人工释放联锁（危险按钮 320×64）。
pub const TEXT_RELEASE: &str = "人工释放联锁";
/// 固定操作条右按钮：M1 授权重启（危险按钮 320×64）。
pub const TEXT_ACK_M1: &str = "M1 授权重启";
/// 操作条右端弱注（UI §6.4 固定操作条行）。
pub const TEXT_AUDIT_NOTE: &str = "操作将记入审计";
/// 就地原因：M1 处于 latch 态（§6.4 拒绝原因表 / IL-03；`，`→`·` 见 **IL1**）。
pub const TEXT_REASON_LATCHED: &str = "处于自锁态 · 须先释放联锁";
/// 就地原因：联锁未启用（§6.4 拒绝原因表）。
pub const TEXT_NOT_ENABLED: &str = "联锁未启用";
/// 就地原因：停机未确认（§3.6 P4「操作」行）。
///
/// ⚠️ **只**由后端结构化拒绝 `InterlockReject::StopPending` 经 [`reject_text`] 产出
/// （`停机未确认 · 暂不可授权重启`）—— 页面**不**按帧内 `stop_failed` 本地预判（B2b-3 评审整改 ①；
/// 理由见 [`op_state`] 与 **IL18**：预判会挡掉后端的具体原因）。
pub const TEXT_STOP_PENDING: &str = "停机未确认";
/// 就地原因：提交时状态已变化（EDGE-19；`，`→`·` 见 **IL1**）。
pub const TEXT_CONFLICT: &str = "联锁状态已变化 · 请刷新后重试";
/// 就地原因：触发源未复位（§6.4 拒绝原因表；后接**具体**源名）。
pub const TEXT_REJECT_SOURCES: &str = "触发源未复位";
/// 就地原因：保持时间不足（§6.4；后接「还需 N 秒」）。
pub const TEXT_REJECT_HOLD: &str = "保持时间不足";
/// 倒计时前缀。
pub(crate) const TEXT_HOLD_MORE: &str = "还需";
/// 倒计时 / 保持时长单位。
pub(crate) const TEXT_SECONDS: &str = "秒";
/// 须保持时长前缀。
pub(crate) const TEXT_HOLD_NEED: &str = "须保持";
/// `StopPending` 的完整拒绝文案尾段。
pub(crate) const TEXT_STOP_PENDING_TRAIL: &str = "暂不可授权重启";
/// `Internal` 的完整拒绝文案前缀（`错` / `误` 缺字 ⇒ 取 `故` / `障`，见 **IL1**）。
pub(crate) const TEXT_INTERNAL: &str = "内部故障";
/// `Busy` 的拒绝文案（`理` / `稍` / `候` 缺字 ⇒ 取最短等价表述，见 **IL1**）。
pub(crate) const TEXT_OP_BUSY: &str = "操作进行中";
/// 弹层标题：确认释放联锁（UI §6.4 强确认弹层段）。
pub const TEXT_DIALOG_TITLE_RELEASE: &str = "确认释放联锁";
/// 弹层标题：确认 M1 授权重启。
pub const TEXT_DIALOG_TITLE_ACK_M1: &str = "确认 M1 授权重启";
/// 弹层「影响范围」：释放联锁（`，`→`·`，见 **IL1**）。
pub const TEXT_IMPACT_RELEASE: &str = "将清除联锁自锁状态 · 装置可恢复运行";
/// 弹层「影响范围」：M1 授权重启（同上）。
pub const TEXT_IMPACT_ACK_M1: &str = "将下发 M1 停机复位 · 装置可重新启动";
/// 弹层明细字段名：当前触发源。
pub const TEXT_DETAIL_SOURCES: &str = "当前触发源";
/// 弹层明细字段名：自锁保持。
pub const TEXT_DETAIL_LATCH: &str = "自锁保持";
/// 弹层明细字段名：停机失败（**与状态卡卡头同一串** ⇒ 转出，不另抄字面量）。
pub const TEXT_DETAIL_STOP: &str = TEXT_CARD_STOP;
/// 弹层明细值：无（§3.6 P4「总态」行）。
pub const TEXT_NONE: &str = "无";
/// 成功 Toast：已释放联锁（UI §3.6 全局行）。
pub const TEXT_TOAST_RELEASED: &str = "已释放联锁";
/// 成功 Toast：已授权重启（同上）。
pub const TEXT_TOAST_ACKED: &str = "已授权重启";
/// 失败 Toast：操作失败（同上）。
pub const TEXT_TOAST_FAIL: &str = "操作失败";
/// **重放窗口已过**（`crate::console::ConsoleError::RetryWindowExpired`）的专属上屏文案。
///
/// # 为什么它必须点明出路（B3-2c）
///
/// 该错误意味着「**首次操作可能已经生效**，而重发必被服务端防重放窗口先拒」——
/// 若照通用兜底显「操作失败」，用户会当成"什么都没发生"再点一次（`console.rs` 模块头
/// 第 6 条登记的正是这条静默语义偏差）。出路**只有一条**：当作**新操作**重发 + 按 T-3
/// **重新确认**，故文案逐字写明。
///
/// # 落点为什么在 `ui/**`（不得写进 `state.rs`）
///
/// 本仓码表静态网 `ui/tests.rs::UI_PROD_SOURCES` **只覆盖 `ui/**` 的 13 个文件**；写在
/// `state.rs` 会**漏出扫描面** ⇒ 该串缺字也不会有任何用例变红（真机豆腐块）。
/// 字符**已在**当前字体码表内（`fonts/lv_font_cmap.txt` 逐字核对：`操 U+64CD` / `已 U+5DF2` /
/// `过 U+8FC7` / `期 U+671F` / `请 U+8BF7` / `重 U+91CD` / `新 U+65B0` / `确 U+786E` /
/// `认 U+8BA4` / `后 U+540E` / `试 U+8BD5` / `· U+00B7` 齐备）⇒ **不必**重跑 `gen_fonts.sh`。
///
/// **PM 已批准**（UI 文档 §3.6；B3-2c 裁定批次）。
pub const TEXT_RETRY_EXPIRED: &str = "操作已过期 · 请重新确认后重试";
/// 审计不可写（UI §8.3 EDGE-18，fail-closed）—— **转出** `p2_config` 的同名字面量（**不另抄**，
/// 同一串只允许一份源码字面量，见 `ui/pages/mod.rs` 的 M3 口径）。
pub const TEXT_AUDIT_UNAVAILABLE: &str = p2_config::TEXT_AUDIT_UNAVAILABLE;
/// Toast 图标：成功（`✓`，cmap 内；转出 P2 的同名字面量）。
pub(crate) const ICON_OK: &str = p2_config::ICON_OK;
/// Toast 图标：失败（`!` —— UI 写 `✕`，但 U+2715 缺字，见 **IL2**；转出 P2 的同名字面量）。
pub(crate) const ICON_FAIL: &str = p2_config::ICON_FAIL;

/// 本页上屏的**全部固定文案**（供 `ui/pages/mod.rs` 的 `ALL_TEXTS` 清册与
/// `ui/tests.rs::ui_texts_covered_by_font_cmap` 的「清册 ↔ 源码字面量」一致性走查 ——
/// 清册**不是**覆盖率基线，见该用例文档）。
pub const ALL_TEXTS: &[&str] = &[
    TEXT_CARD_STATE,
    TEXT_STATE_LATCHED,
    TEXT_STATE_UNLATCHED,
    TEXT_STATE_UNAVAILABLE,
    TEXT_STATE_UNAVAILABLE_SHORT,
    ICON_STATE_LATCHED,
    ICON_STATE_UNLATCHED,
    ICON_STATE_UNAVAILABLE,
    ICON_FILLED,
    ICON_HOLLOW,
    TEXT_LATCH_HELD,
    TEXT_LATCH_UNHELD,
    TEXT_SOURCES_TITLE,
    TEXT_SOURCES_EMPTY,
    TEXT_SRC_TRIPPED,
    TEXT_SRC_UNTRIPPED,
    TEXT_SRC_ESTOP,
    TEXT_SRC_DOOR,
    TEXT_NAME_SEP,
    TEXT_CLAUSE_SEP,
    TEXT_OVERFLOW_MORE,
    TEXT_OVERFLOW_UNIT,
    TEXT_CARD_STOP,
    TEXT_CARD_FAULT_LAMP,
    TEXT_CARD_RUN_LAMP,
    TEXT_STOP_OK,
    TEXT_STOP_FAIL,
    TEXT_LAMP_ON,
    TEXT_LAMP_OFF,
    TEXT_LAMP_UNKNOWN,
    TEXT_RELEASE,
    TEXT_ACK_M1,
    TEXT_AUDIT_NOTE,
    TEXT_REASON_LATCHED,
    TEXT_NOT_ENABLED,
    TEXT_STOP_PENDING,
    TEXT_CONFLICT,
    TEXT_REJECT_SOURCES,
    TEXT_REJECT_HOLD,
    TEXT_HOLD_MORE,
    TEXT_SECONDS,
    TEXT_HOLD_NEED,
    TEXT_STOP_PENDING_TRAIL,
    TEXT_INTERNAL,
    TEXT_OP_BUSY,
    TEXT_DIALOG_TITLE_RELEASE,
    TEXT_DIALOG_TITLE_ACK_M1,
    TEXT_IMPACT_RELEASE,
    TEXT_IMPACT_ACK_M1,
    TEXT_DETAIL_SOURCES,
    TEXT_DETAIL_LATCH,
    TEXT_DETAIL_STOP,
    TEXT_NONE,
    TEXT_TOAST_RELEASED,
    TEXT_TOAST_ACKED,
    TEXT_TOAST_FAIL,
    TEXT_RETRY_EXPIRED,
    TEXT_AUDIT_UNAVAILABLE,
    ICON_OK,
    ICON_FAIL,
];

// ═══════════════════════════════════════════════════════════════════════════
// 2. 栅格常量（UI §6.4；**全部由 theme 常量推导** —— 本页不出现裸规格值）
//
// 坐标口径（与 `p2_config.rs` 同，见其 **PD4**）：卡内子对象的 `set_pos` 相对卡的
// **内容区**原点（= 卡外缘 + 描边 1 + 内边距 16 = +`CARD_INSET`）；要越过内边距贴到卡外缘，
// 用**负偏移** `-CARD_INSET`（LVGL 子对象裁剪到父的**外缘**坐标，见 `p2_config.rs` 的
// `ROW_B_CTRL_X` 注）。
// ═══════════════════════════════════════════════════════════════════════════

/// 卡外缘 → 卡内容区原点的偏移（描边 1 + 内边距 16）。
const CARD_INSET: i32 = theme::Stroke::THIN + Dimens::GAP_MIN;
/// 卡内可用宽（`theme::card()` 的左右内边距各吃掉 `CARD_INSET`）。
const INNER_W: i32 = Dimens::CONTENT_W - 2 * CARD_INSET;
/// 卡头高（`SectionTitle` 28 + 同组缝 16；与 `p2_config.rs` 的 `CARD_HEAD_H` 同式）。
const CARD_HEAD_H: i32 = TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
/// 卡头内文字 / 胶囊的垂直居中 y。
const CARD_HEAD_TEXT_Y: i32 = theme::center_offset(CARD_HEAD_H, TextSlot::SectionTitle.px() as i32);

// ── 【安全总览带】（UI §6.4.1「页面几何」+「区块规格」；两卡并排各 `BAND_CARD_W` 488）──
//
// ⚠️ **几何不可满足性（T21c-1 实测，见文件头 **IL29**）**：488 宽卡内**放不下**既有
// 联锁总态卡的全部通道（标题 112 + 图标 72 + 96 px 主值语最长 672 + latch 胶囊 236 +
// 冻结角标 192）。本批的取舍逐条登记在 IL29，**不静默**。
/// 总览带 y（UI 线框 `Y80` = 内容区上内边距）。
const BAND_Y: i32 = Dimens::CONTENT_PAD_TOP;
/// 总览带**卡头行**高（= `STATUS_CHIP_H` 32：标题 28 居中 + latch 胶囊 32 高；UI §6.4.1
/// 「卡头 28 px」与「胶囊高 32」在同一行 ⇒ 行高取二者之大）。
const BAND_HEAD_H: i32 = Dimens::STATUS_CHIP_H;
/// 卡头行内文字 y（垂直居中）。
const BAND_HEAD_TEXT_Y: i32 = theme::center_offset(BAND_HEAD_H, TextSlot::SectionTitle.px() as i32);
/// 总览带卡**内容区**宽（488 − 2×17 = 454）。
const BAND_INNER_W: i32 = Dimens::BAND_CARD_W - 2 * CARD_INSET;
/// 总览带单卡外缘高（卡头 32 + 主值行 96 + 上下内边距 34 = 162；UI 写 164 ⇒ **IL29③**）。
const BAND_CARD_H: i32 = BAND_HEAD_H + TextSlot::InterlockState.px() as i32 + 2 * CARD_INSET;
/// 总览带整体高（= 单卡高；两卡同高并排）。
const BAND_H: i32 = BAND_CARD_H;
/// 火警等级卡 x（联锁总态卡右缘 + 同组缝；UI §6.4.1「`504 + 16 = 520`」的通用式）。
const FIRE_CARD_X: i32 = Dimens::BAND_CARD_W + Dimens::GAP_MIN;

// ── 联锁总态卡（**既有内容 / 文案 / 字号不变**；仅宽度 1008→488、高度 180→162）────
/// 顶部语义横条高（UI §6.4：卡顶 3 px 横条 = 语义色）。
const STATE_BAR_TOP_H: i32 = theme::Stroke::ALERT;
/// 左缘语义竖条宽（UI §6.4：卡左缘 6 px 竖条 = 语义色；`theme` 已有该档）。
const STATE_BAR_LEFT_W: i32 = Dimens::INTERLOCK_BAR;
/// 总态词图标 y（在 96 px 主值行内居中，图标 72 px）。
const STATE_ICON_Y: i32 =
    BAND_HEAD_H + theme::center_offset(TextSlot::InterlockState.px() as i32, Dimens::ICON_XL);
/// 总态词图标宽（= 总态词高，见 `ICON_XL`）。
const STATE_ICON_W: i32 = Dimens::ICON_XL;
/// 总态词 x（图标右侧 + 同组缝）。
const STATE_TEXT_X: i32 = STATE_ICON_W + Dimens::GAP_MIN;
/// 总态词宽（到卡内容区右缘；**含 DOTS 截断** —— 见 **IL29①**）。
const STATE_TEXT_W: i32 = BAND_INNER_W - STATE_TEXT_X;
/// latch 胶囊宽（取 `StatusChip` 契约宽 236；最长文案 9 字 ≈ 204 px + 图标槽 32 ≤ 236）。
const LATCH_CHIP_W: i32 = Dimens::CARD_STATUS_W;
/// latch 胶囊 x（贴卡内容区右缘）。
const LATCH_CHIP_X: i32 = BAND_INNER_W - LATCH_CHIP_W;
/// latch 胶囊 y（在卡头行内居中；`STATUS_CHIP_H` 32 ≥ 28 ⇒ 收敛到 0，与卡头文字同顶）。
const LATCH_CHIP_Y: i32 = theme::center_offset(BAND_HEAD_H, Dimens::STATUS_CHIP_H);
/// 总态卡角标 x（**图标-only 形态**：落在标题（4 字 × 28 = 112）之后、latch 胶囊（236，
/// 贴内容区右缘 x218）之前的空槽 —— 三件 112 < 174 < 202 < 218 **互不重叠**；见 **IL29②**）。
const STATE_FROZEN_X: i32 = LATCH_CHIP_X - crate::ui::pages::FROZEN_BADGE_W - Dimens::GAP_MIN;
/// 总态卡角标 y（卡头行高 32 = 胶囊高 ⇒ 收敛到 0；**与触发源卡不同** —— 那张卡的卡头是
/// `CARD_HEAD_H`(44)，故用共享的 `pages::FROZEN_CHIP_Y`(6)）。
const STATE_FROZEN_Y: i32 = theme::center_offset(BAND_HEAD_H, Dimens::STATUS_CHIP_H);

// ── 火警等级卡（F21 新增；UI §6.4.1「火警等级卡」）────────────────────────────
/// 火警图标宽（**取小档 `ICON_SM`**；见 **IL29④**）。
///
/// **依据（T21c-1-r1 实测订正）**：取 28 档 ⇒ 主值槽 `FIRE_VALUE_W` = `BAND_INNER_W`(454)
/// − (28 + `GAP_MIN` 16) = **410**（4 字枚举 × 64 = 256 ⇒ 余量 **154**）；取 UI §6.4.1 写的
/// 72 档 ⇒ **366**（余量 110）。且 `theme::icon_slot(72)` **本就降档渲染为 64**
/// （该档位缺口与 IL3 / IL20 同族）。
/// ⚠️ 原注「64 / 72 档会**挤掉** 4 字枚举文案」**经逐字复算不成立**（256 ≤ 366 仍有余量）
/// ⇒ 已按上述真实依据改写，**不保留不实因果**（登记见 **IL29④**）。
const FIRE_ICON_W: i32 = Dimens::ICON_SM;
/// 火警主值 x（图标右侧 + 同组缝）。
const FIRE_VALUE_X: i32 = FIRE_ICON_W + Dimens::GAP_MIN;
/// 火警主值宽（到卡内容区右缘；4 字 × 64 px = 256 ≤ 410 ✓）。
const FIRE_VALUE_W: i32 = BAND_INNER_W - FIRE_VALUE_X;
/// 火警主值 y（在 96 px 主值行内居中，文案 64 px）。
const FIRE_VALUE_Y: i32 = BAND_HEAD_H
    + theme::center_offset(
        TextSlot::InterlockState.px() as i32,
        TextSlot::PhasePower.px() as i32,
    );
/// 火警图标 y（与主值行同中线）。
const FIRE_ICON_Y: i32 =
    BAND_HEAD_H + theme::center_offset(TextSlot::InterlockState.px() as i32, FIRE_ICON_W);

// ── 区块级「冻结 / 数据过期」角标（EDGE-03 / EDGE-20；B3-2c）─────────────────
//
// ⚠️ **宽 / y 不在本文件**：它们与**构造点**一起收口在 [`crate::ui::pages::frozen_chip`]
// （图标 + 文字的 192 px 胶囊）与 [`crate::ui::pages::frozen_icon_badge`]（**图标-only** 的
// 28 px 形态）—— 本页两张卡**各按自己的卡头可用宽**取其中一形态，都不在这里内联构造。
// ⚠️ **两张卡的形态不同（IL29②，产品裁定 2026-09-25）**：总态卡 488 px 宽、卡头三件
// （标题 112 + 角标 + latch 胶囊 236）**放不下** 192 px 的文字胶囊（540 > 454）⇒ 取
// **图标-only**（28 px，落标题与胶囊之间的空槽，三件互不重叠）；触发源卡卡头右侧整段空白
// ⇒ 保留既有的**文字胶囊**（EDGE-03 的原文形态）。
/// 触发源卡角标 x（贴卡内容区右缘；该卡头只有标题 + 计数，右侧整段空白）。
const SOURCE_FROZEN_X: i32 = INNER_W - crate::ui::pages::FROZEN_CHIP_W;

// ── 触发源卡（UI §6.4 区块规格「触发源卡」；线框 992×200）─────────────────────
/// 触发源行高（UI §6.4：行高 44；`theme` 的 `ROW_LOG_H` 即该档）。
const SOURCE_ROW_H: i32 = Dimens::ROW_LOG_H;
/// 触发源行池大小（后端 distinct token 全集 `estop`/`flood`/`fire`/`door` = 4，见 **IL9**）。
const SOURCE_ROW_POOL: usize = 4;
/// 行右侧状态词槽宽（`CHIP_MIN_W`；「已触发」实测 72 px ≤ 96）。
const SOURCE_STATUS_W: i32 = Dimens::CHIP_MIN_W;
/// 行右侧状态词 x（贴行右缘）。
const SOURCE_STATUS_X: i32 = INNER_W - SOURCE_STATUS_W;
/// 行左侧 `LedIndicator` 宽（到状态词槽左缘减同组缝）。
const SOURCE_NAME_W: i32 = SOURCE_STATUS_X - Dimens::GAP_MIN;
/// 行内 `LedIndicator` y（在 44 px 行内居中，组件自高 `ICON_SM`）。
const SOURCE_ROW_LED_Y: i32 = theme::center_offset(SOURCE_ROW_H, Dimens::ICON_SM);
/// 行内状态词 y（在 44 px 行内居中，正文 24）。
const SOURCE_ROW_STATUS_Y: i32 = theme::center_offset(SOURCE_ROW_H, TextSlot::Body.px() as i32);
/// [`EmptyState`] 的构件高（图标 64 + 缝 16 + 标题行 44）—— **触发源卡**无源时的卡体高。
///
/// `pub(crate)`：`ui/tests.rs` 用它做**漂移锁**（见下方注）。
///
/// ⚠️ **为何是"推导常量"而不是"读组件实测高"**（实测教训，B2b-3）：`Obj::size()` 读的是
/// LVGL 的 `coords`，而要等一次**布局**才算出（`ui/tests.rs` 里读几何前一律先
/// `disp.refr_now_for_test()`）；页面构造期**不可能**触发渲染（薄层无该能力、且 `ui/**`
/// 禁用 `lv_refr_now`）⇒ 在 `new()` 里"读组件高度"只会拿到 `0`（实测：卡高被算成 78 = 44 + 34），
/// 且**静默**（无报错）。故此处按组件的**构件算式**用 `theme` 常量推导，并由
/// `ui/tests.rs` 的断言把本常量与**组件实测高**钉在一起（漂移即红 —— 与
/// [`TEXT_STATE_UNAVAILABLE`] / [`UnavailableKind::Interlock`] 的"同源锁"同一手法）。
pub(crate) const EMPTY_H: i32 =
    Dimens::ICON_LG + Dimens::GAP_MIN + TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
/// 触发源卡**卡体最小高**（= 空态高；`: usize` 无关的常量别名，语义上一行）。
const SOURCE_BODY_MIN_H: i32 = EMPTY_H;
/// [`UnavailableState`] 的构件高（空态算式 + 原因行 24 + 缝 16）—— 触发源卡不可用时的卡体高。
pub(crate) const UNAVAILABLE_H: i32 = EMPTY_H + TextSlot::Body.px() as i32 + Dimens::GAP_MIN;

// ── 状态三卡（UI §6.4 区块规格「状态三卡」；线框 236×108 × 3）────────────────
/// 单卡宽（`theme` 的 `CARD_STATUS_W` = 236）。
const LAMP_CARD_W: i32 = Dimens::CARD_STATUS_W;
/// 单卡外缘高 = 卡头 28 + 缝 16 + 值行 28 + 上下内边距 34（见 **IL3**，UI 写 108）。
const LAMP_CARD_H: i32 = 2 * CARD_INSET
    + TextSlot::SectionTitle.px() as i32
    + Dimens::GAP_MIN
    + Dimens::ICON_SM;
/// 卡内值行 y。
const LAMP_VALUE_Y: i32 = TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
/// 卡内值行高（图标 28）。
const LAMP_VALUE_H: i32 = Dimens::ICON_SM;
/// 卡内值行可用宽（卡内容区宽）。
const LAMP_VALUE_W: i32 = LAMP_CARD_W - 2 * CARD_INSET;
/// 相邻两张灯卡的水平间距（UI §5.1 #19 卡宽 236 + 同组缝 16 ⇒ 线框 x16/268/520）。
const LAMP_CARD_GAP: i32 = Dimens::GAP_MIN;
/// 第 `i` 张灯卡的 x。
const fn lamp_x(i: i32) -> i32 {
    i * (LAMP_CARD_W + LAMP_CARD_GAP)
}

// ── 页面纵向骨架（页内坐标 = UI 绝对坐标 − (16, 72)）──────────────────────────
//
// 骨架自下而上**反推**（每条都等于设计线的值，见 **IL29** 的差值登记）：
//   内容区 624 = 上内边距 8 + 总览带 `BAND_H` + `GAP_GROUP` 16 + 滚动视口 `VIEWPORT_H`
//                + 就地原因带 24 + 固定操作条 72
const ACTION_BAR_H: i32 = Dimens::BTN_H_PRIMARY + Dimens::CONTENT_PAD_TOP;
/// 就地原因带高（正文 24；**IL4**：线框的视口拆成「滚动视口 + 原因带 24」）。
///
/// ⚠️ **断言口径**（T-25）：`滚动区 ↔ 操作条` 的间距**恰等于** `Dimens::GAP_SECTION`(24)
/// —— 而本常量就是那 24 px 的原因带，故 `REASON_BAND_H == GAP_SECTION` 必须成立。
const REASON_BAND_H: i32 = TextSlot::Body.px() as i32;
/// 滚动视口 y（总览带之下 + 同组缝）。
const VIEWPORT_Y: i32 = BAND_Y + BAND_H + Dimens::GAP_GROUP;
/// 滚动视口高（UI §6.4.1 写 340；本实现 342 —— 见 **IL29③**；**≥ 340 ⇒ §B 首屏全可见不破**）。
const VIEWPORT_H: i32 = Dimens::CONTENT_H - VIEWPORT_Y - REASON_BAND_H - ACTION_BAR_H;
/// 就地原因带 y。
const REASON_BAND_Y: i32 = VIEWPORT_Y + VIEWPORT_H;
/// 操作条 y。
const ACTION_BAR_Y: i32 = REASON_BAND_Y + REASON_BAND_H;
/// 操作条内按钮 y（垂直居中）。
const ACTION_BTN_Y: i32 = theme::center_offset(ACTION_BAR_H, Dimens::BTN_H_PRIMARY);

// ── §B 联锁区在滚动区内的 y（**既有内容原样**，整体下移到滚动区坐标）────────────
/// 触发源卡 y（滚动区首卡，y=0）。
const SOURCE_CARD_Y: i32 = 0;
/// 触发源卡外缘高（2 源时；卡体最小高 + 卡头 + 上下内边距）。
///
/// **滚动区首屏分配校验**：`SOURCE_CARD_H + 16 × 2 + LAMP_CARD_H × 1`
/// = `202 + 32 + 106` = **340 ≤ `VIEWPORT_H`(342)** ⇒ §B 全量首屏可见（UI §6.4.1 的核心不变量）。
const SOURCE_CARD_H: i32 = SOURCE_BODY_MIN_H + CARD_HEAD_H + 2 * CARD_INSET;
/// 状态三卡 y（触发源卡之下 + 同组缝）。
const LAMP_CARD_Y: i32 = SOURCE_CARD_Y + SOURCE_CARD_H + Dimens::GAP_GROUP;

// ── §A 消防四组（UI §6.4.1「§A 消防区」；四组自上而下 A1→A4，组间 `GAP_GROUP`）────
/// A 组行高（UI 写 36 / 40；`theme` 的 `ROW_LOG_H`(44) 是仓内唯一"列表行"档 ⇒ 见 **IL29⑤**）。
const A_ROW_H: i32 = Dimens::ROW_LOG_H;
/// A1 行数（整字位图 16 位逐位一行；F21.1 / EX-09）。
///
/// ⚠️ **位行恒 16、段顶通告另占一行**（T21c-1-r1 订正）：段级 / 站级降级时通告落
/// [`A1_NOTICE_ROWS`] 那一行（= **段顶状态条**，设计 §15.6.2 ① 的口径），**不再**占用
/// 第 0 行 —— 否则 bit0 的语义在本态**不可判读**，且「逐位 16 行」的行数契约被破坏。
///
/// `pub(crate)` 供 `ui/tests.rs` 的**行数断言**引用（单真源：不另抄一个 16）。
pub(crate) const A1_ROWS: usize = 16;
/// A1 卡顶部**段顶通告**行数（恒 1：`!available` ⇒「外设数据不可用」/ 站级降级 ⇒ 站文案）。
///
/// **无论通告是否在显都占行**（通告隐藏时该行留白）⇒ 位行 y 不随降级态跳动，屏上位置稳定
/// （同 `A4_CARD_H` 为「不一致提示行」预留整行的口径）。
const A1_NOTICE_ROWS: usize = 1;
/// A1 **段顶通告**文本槽 y（卡头之下第一行）。
const A1_NOTICE_Y: i32 = CARD_HEAD_H;
/// A1 **位行**首行（bit0）的 y —— 段顶通告行之下（通告恒占行 ⇒ 位行 y 不随降级态跳动）。
const A1_BIT_ROWS_Y: i32 = A1_NOTICE_Y + A1_NOTICE_ROWS as i32 * A_ROW_H;
/// A3 行数（烟感 / 温感 / 可燃 三个状态字）。
const A3_ROWS: usize = 3;
/// A1 卡高（卡头 + **段顶通告 1 行** + 16 位行 + 上下内边距）。
///
/// = `44 + (1 + 16) × 44 + 34` = **826**（UI §6.4.1 写 616 —— 行高取 44 档的连带后果，
/// 见 **IL29⑤**；那 1 行是**段顶状态条**，见 `A1_NOTICE_ROWS`）。
const A1_CARD_H: i32 = CARD_HEAD_H + (A1_NOTICE_ROWS + A1_ROWS) as i32 * A_ROW_H + 2 * CARD_INSET;
/// A1 卡 y（灯卡之下 + 同组缝）。
const A1_CARD_Y: i32 = LAMP_CARD_Y + LAMP_CARD_H + Dimens::GAP_GROUP;
/// A2 卡高（卡头 + 主值行 44 + 上下内边距）。
const A2_CARD_H: i32 = CARD_HEAD_H + A_ROW_H + 2 * CARD_INSET;
/// A2 卡 y。
const A2_CARD_Y: i32 = A1_CARD_Y + A1_CARD_H + Dimens::GAP_GROUP;
/// A3 卡高（卡头 + 3 行 + 上下内边距）。
const A3_CARD_H: i32 = CARD_HEAD_H + A3_ROWS as i32 * A_ROW_H + 2 * CARD_INSET;
/// A3 卡 y。
const A3_CARD_Y: i32 = A2_CARD_Y + A2_CARD_H + Dimens::GAP_GROUP;
/// A4 卡高（卡头 + **汇总行 + 不一致提示行**（2 × `A_ROW_H`）+ 上下内边距）—— 卡头行高取
/// `BTN_H_SECONDARY`(48) 以便放「查看明细」；提示不显时该行**留白**（**不改卡高** ⇒ 位置稳定）。
const A4_CARD_H: i32 = Dimens::BTN_H_SECONDARY + 2 * A_ROW_H + 2 * CARD_INSET;
/// A4 卡 y。
const A4_CARD_Y: i32 = A3_CARD_Y + A3_CARD_H + Dimens::GAP_GROUP;
/// A4「查看明细」按钮宽（`theme` 的 `BTN_MAIN_W` = 200）。
const A4_DETAIL_W: i32 = Dimens::BTN_MAIN_W;
/// A4「查看明细」按钮 x（贴卡内容区右缘）。
const A4_DETAIL_X: i32 = INNER_W - A4_DETAIL_W;
/// A4 汇总行 y（卡头之下第一行）。
const A4_SUMMARY_Y: i32 = Dimens::BTN_H_SECONDARY;
/// A4 不一致提示行 y。
const A4_MISMATCH_Y: i32 = A4_SUMMARY_Y + A_ROW_H;

// ── 下钻视图（UI §6.4.1「下钻视图通用规格」；P4 探测器明细）───────────────────
/// 下钻顶部条高（标题 + 「收起」= `TOUCH_MIN` 48）。
const DRILL_TOP_H: i32 = Dimens::TOUCH_MIN;
/// 下钻表头高（UI 写 32；= `STATUS_CHIP_H` 档）。
const DRILL_HEAD_H: i32 = Dimens::STATUS_CHIP_H;
/// 下钻分页条高（`TOUCH_MIN`）。
const DRILL_PAGE_H: i32 = Dimens::TOUCH_MIN;
/// 下钻行区高（视口余量；UI 写 212）。
const DRILL_ROWS_H: i32 = VIEWPORT_H - DRILL_TOP_H - DRILL_HEAD_H - DRILL_PAGE_H;
/// 下钻行高（UI 写 44 = `ROW_LOG_H`）。
const DRILL_ROW_H: i32 = Dimens::ROW_LOG_H;
/// 下钻行池（= 服务端 `page_size` 缺省 20；**>20 的行不建对象** ⇒ 见 **IL29⑥**）。
const DRILL_ROW_POOL: usize = 20;
/// 「收起」/「上一页」/「下一页」按钮宽（`BTN_MIN_W` 120）。
const DRILL_BTN_W: i32 = Dimens::BTN_MIN_W;
/// 「收起」按钮 x（**视图顶部右端**，UI §6.4.1 / §15.5.3 F11.3）。
const DRILL_COLLAPSE_X: i32 = Dimens::CONTENT_W - DRILL_BTN_W;
// ── 明细表列宽（**逐条由 theme 常量推导**；求和恰 = `CONTENT_W` 992）──────
/// 序号列（`CHIP_MIN_W − GAP_MIN/2` = 88）。
const DRILL_COL_SEQ: i32 = Dimens::CHIP_MIN_W - Dimens::GAP_MIN / 2;
/// 地址列（`CHIP_MIN_W` = 96）。
const DRILL_COL_ADDR: i32 = Dimens::CHIP_MIN_W;
/// 烟雾 / 温度列（`CHIP_MIN_W + ICON_SM + GAP_MIN/2` = 132）。
const DRILL_COL_GAS: i32 = Dimens::CHIP_MIN_W + Dimens::ICON_SM + Dimens::GAP_MIN / 2;
/// CO / VOC / H2 列（`CHIP_MIN_W − GAP_MIN − GAP_MIN/2` = 72）。
const DRILL_COL_GAS_NUM: i32 = Dimens::CHIP_MIN_W - Dimens::GAP_MIN - Dimens::GAP_MIN / 2;
/// 状态列（**余量列**：`992 − 序号 − 地址 − 2×气体 − 3×数值` = 328）。
///
/// `pub(crate)` 供 `ui/tests.rs` 的 T-25 段断言**表头串实测宽 ≤ 本列宽**（W-2 的回归网；
/// 见 `drill_head_texts` 的文档注）—— 该值本就在册（非新增口径）。
pub(crate) const DRILL_COL_STATUS: i32 =
    Dimens::CONTENT_W - DRILL_COL_SEQ - DRILL_COL_ADDR - 2 * DRILL_COL_GAS - 3 * DRILL_COL_GAS_NUM;
/// 状态列的**半槽宽**（报警总状态 / 故障总状态各占一半 = 164）。
const DRILL_HALF_STATUS: i32 = DRILL_COL_STATUS / 2;
/// 明细表的**列数**（序号 │ 地址 │ 状态 │ 烟雾 │ 温度 │ CO │ VOC │ H2）—— 表头用。
const DRILL_COLS: usize = 8;
/// 明细行的**格子数**（状态列拆两个半槽 ⇒ 9 格；表头仍是 8 列）。
const DRILL_CELLS: usize = 9;
/// 第 `i` 列（0..8）的 x —— **表头**用。
const fn drill_col_x(i: usize) -> i32 {
    match i {
        0 => 0,
        1 => DRILL_COL_SEQ,
        2 => DRILL_COL_SEQ + DRILL_COL_ADDR,
        3 => DRILL_COL_SEQ + DRILL_COL_ADDR + DRILL_COL_STATUS,
        4 => DRILL_COL_SEQ + DRILL_COL_ADDR + DRILL_COL_STATUS + DRILL_COL_GAS,
        5 => DRILL_COL_SEQ + DRILL_COL_ADDR + DRILL_COL_STATUS + 2 * DRILL_COL_GAS,
        6 => {
            DRILL_COL_SEQ
                + DRILL_COL_ADDR
                + DRILL_COL_STATUS
                + 2 * DRILL_COL_GAS
                + DRILL_COL_GAS_NUM
        }
        _ => {
            DRILL_COL_SEQ
                + DRILL_COL_ADDR
                + DRILL_COL_STATUS
                + 2 * DRILL_COL_GAS
                + 2 * DRILL_COL_GAS_NUM
        }
    }
}
/// 第 `i` 列（0..8）的宽 —— **表头**用。
const fn drill_col_w(i: usize) -> i32 {
    match i {
        0 => DRILL_COL_SEQ,
        1 => DRILL_COL_ADDR,
        2 => DRILL_COL_STATUS,
        3 | 4 => DRILL_COL_GAS,
        _ => DRILL_COL_GAS_NUM,
    }
}
/// 第 `i` 格（0..9）的 x —— **行**用（格序：序号 / 地址 / 报警总状态 / 故障总状态 / 烟雾 / 温度 / CO / VOC / H2）。
const fn drill_cell_x(i: usize) -> i32 {
    match i {
        0 => 0,
        1 => DRILL_COL_SEQ,
        2 => DRILL_COL_SEQ + DRILL_COL_ADDR,
        3 => DRILL_COL_SEQ + DRILL_COL_ADDR + DRILL_HALF_STATUS,
        4 => DRILL_COL_SEQ + DRILL_COL_ADDR + DRILL_COL_STATUS,
        5 => DRILL_COL_SEQ + DRILL_COL_ADDR + DRILL_COL_STATUS + DRILL_COL_GAS,
        6 => DRILL_COL_SEQ + DRILL_COL_ADDR + DRILL_COL_STATUS + 2 * DRILL_COL_GAS,
        7 => {
            DRILL_COL_SEQ
                + DRILL_COL_ADDR
                + DRILL_COL_STATUS
                + 2 * DRILL_COL_GAS
                + DRILL_COL_GAS_NUM
        }
        _ => {
            DRILL_COL_SEQ
                + DRILL_COL_ADDR
                + DRILL_COL_STATUS
                + 2 * DRILL_COL_GAS
                + 2 * DRILL_COL_GAS_NUM
        }
    }
}
/// 第 `i` 格（0..9）的宽 —— **行**用。
const fn drill_cell_w(i: usize) -> i32 {
    match i {
        0 => DRILL_COL_SEQ,
        1 => DRILL_COL_ADDR,
        2 | 3 => DRILL_HALF_STATUS,
        4 | 5 => DRILL_COL_GAS,
        _ => DRILL_COL_GAS_NUM,
    }
}
/// 危险按钮宽（UI §6.4 固定操作条行写 320 = `TOUCH_CRITICAL` × 5，**推导而非抄数**）。
const OP_BTN_W: i32 = Dimens::TOUCH_CRITICAL * 5;
/// 右按钮 x（= 左按钮右缘 + 危险间距 48，UI §6.4：「两者间距 48 px」）。
const OP_BTN2_X: i32 = OP_BTN_W + Dimens::GAP_DANGER;
/// 操作条右端弱注 x（UI 线框 `x 720–1008` ⇒ 页内 704）。
const OP_NOTE_X: i32 = 2 * OP_BTN_W + Dimens::GAP_DANGER + Dimens::GAP_MIN;
/// 操作条右端弱注宽。
const OP_NOTE_W: i32 = Dimens::CONTENT_W - OP_NOTE_X;
/// 操作条右端弱注 y（垂直居中）。
const OP_NOTE_Y: i32 = theme::center_offset(ACTION_BAR_H, TextSlot::Body.px() as i32);
/// 就地原因**左槽**宽（到右槽起点减同组缝 —— 两槽可同显且不重叠，见 **IL5**）。
const REASON_LEFT_W: i32 = OP_BTN2_X - Dimens::GAP_MIN;
/// 就地原因**左槽**在**右槽不显**时的宽度（加宽到整幅）—— 见 **IL23** ③：右槽空着时
/// 左槽独占整条原因带，长 `message` 因此**单行不截断**（352 px → 992 px；24 px 档按生成字体
/// 的 `adv_w` 实测**每个汉字约 24 px** ⇒ 352 px 只容 ≈14 字、992 px 容 ≈41 字；右槽在显时
/// 仍 352 px，是该处置的**残余**）。
const REASON_LEFT_FULL_W: i32 = Dimens::CONTENT_W;
/// 就地原因**右槽** x（**恰在 `M1 授权重启` 按钮正上方**，UI §6.4 拒绝原因表）。
const REASON_RIGHT_X: i32 = OP_BTN2_X;
/// 就地原因**右槽**宽（= 按钮宽）。
const REASON_RIGHT_W: i32 = OP_BTN_W;
/// 就地原因带内文字 y（垂直居中）。
const REASON_TEXT_Y: i32 = theme::center_offset(REASON_BAND_H, TextSlot::Body.px() as i32);

// ═══════════════════════════════════════════════════════════════════════════
// 3. 纯逻辑（**不触碰 LVGL** ⇒ 可独立单测；页内一切判据都经这里，保证可离线复现）
// ═══════════════════════════════════════════════════════════════════════════

/// 联锁**展示态**（三态互斥；UI §6.4 总态卡 / §8.3 专行）。
///
/// ⚠️ **`Unavailable` 与 `Unlatched` 是两个不同的态**（IL-01.6）：「未联锁」= 确知安全，
/// 「不可用」= 无法获知（fail-closed）。本枚举把这一区分**结构化** —— 由 [`state_view`]
/// 唯一决定，页面任何分支都不得把 `Unavailable` 回落成 `Unlatched`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateView {
    /// 确知**已联锁**（`available && latched`）。
    Latched,
    /// 确知**未联锁**（`available && !latched`）。
    Unlatched,
    /// **无法获知**联锁状态（`!available`）—— 缺帧 / 状态源不可用。
    Unavailable,
}

/// 三个展示态的**固定槽位序**（索引 = 胶囊 / 样式表下标；唯一真源）。
pub const STATE_VIEW_ORDER: [StateView; 3] = [
    StateView::Latched,
    StateView::Unlatched,
    StateView::Unavailable,
];

/// 帧内联锁段 → 展示态（**唯一判据**）。
///
/// `!available` **优先**于 `latched`：状态源不可用时，`latched` 这一比特**本身不可信**
/// （它可能只是缺省 `false`），故一律落 [`StateView::Unavailable`]（§8.3：不得显示为「未联锁」）。
pub const fn state_view(s: &InterlockSection) -> StateView {
    if !s.available {
        StateView::Unavailable
    } else if s.latched {
        StateView::Latched
    } else {
        StateView::Unlatched
    }
}

/// 展示态 → 样式 / 槽位下标（与 [`STATE_VIEW_ORDER`] 一一对应）。
pub const fn state_index(v: StateView) -> usize {
    match v {
        StateView::Latched => 0,
        StateView::Unlatched => 1,
        StateView::Unavailable => 2,
    }
}

/// 展示态 → 总态词（UI §3.6 P4「总态」行逐字；**不可用态取缩短形态** —— `IL29①`，
/// 「已联锁 / 未联锁」两词逐字不变）。
pub const fn state_text(v: StateView) -> &'static str {
    match v {
        StateView::Latched => TEXT_STATE_LATCHED,
        StateView::Unlatched => TEXT_STATE_UNLATCHED,
        StateView::Unavailable => TEXT_STATE_UNAVAILABLE_SHORT,
    }
}

/// 展示态 → 图标字形（§8.3：不可用**不得**复用 `✓` 与 `⚠`）。
pub const fn state_icon(v: StateView) -> &'static str {
    match v {
        StateView::Latched => ICON_STATE_LATCHED,
        StateView::Unlatched => ICON_STATE_UNLATCHED,
        StateView::Unavailable => ICON_STATE_UNAVAILABLE,
    }
}

/// 展示态 → 语义色（§6.4：已联锁 `#FF6B6B` / 未联锁 `#2FDB8A` / 不可用灰 `#8C98AC`）。
pub const fn state_color(v: StateView) -> Color {
    match v {
        StateView::Latched => Palette::DANGER,
        StateView::Unlatched => Palette::OK,
        StateView::Unavailable => Palette::STOPPED,
    }
}

/// latch 胶囊的三态（槽位下标 + 文案 + 皮肤；槽位 = [`STATE_VIEW_ORDER`] 的位置）。
///
/// ⚠️ **结构性 fail-closed**：`Unavailable` 分支**只**返回「联锁状态不可用」——
/// 「未保持」在该分支**取不到**（§8.3 明文要求）。这是分支返回的**唯一性**，不是"约定别写错"。
pub const fn latch_chip(v: StateView) -> (usize, &'static str, ChipSkin) {
    match v {
        StateView::Latched => (0, TEXT_LATCH_HELD, ChipSkin::WARNING),
        StateView::Unlatched => (1, TEXT_LATCH_UNHELD, ChipSkin::NEUTRAL),
        StateView::Unavailable => (2, TEXT_STATE_UNAVAILABLE, ChipSkin::UNAVAILABLE),
    }
}

/// latch 胶囊的图标字形（`StatusChip` 强制要求 icon 通道；契约未指定字形 ⇒ 取实心 / 空心 /
/// 问号三态，与灯类语义同族；不可用态取 [`ICON_STATE_UNAVAILABLE`]，**不**复用 `✓` / `⚠`）。
pub const fn latch_chip_icon(v: StateView) -> &'static str {
    match v {
        StateView::Latched => ICON_FILLED,
        StateView::Unlatched => ICON_HOLLOW,
        StateView::Unavailable => ICON_STATE_UNAVAILABLE,
    }
}

/// 三态标志（灯 / 停机）的**统一视图**：`Some(true)` / `Some(false)` / `None`（未知）。
///
/// `None` ⇒ [`TriView::Unknown`] —— **不得**臆造成 `Off`（契约 `Option<bool>` 的三态语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriView {
    /// 亮 / 成立。
    On,
    /// 灭 / 不成立。
    Off,
    /// **未知**（`None`）。
    Unknown,
}

/// `Option<bool>` → 三态视图。
pub const fn tri_view(v: Option<bool>) -> TriView {
    match v {
        Some(true) => TriView::On,
        Some(false) => TriView::Off,
        None => TriView::Unknown,
    }
}

/// 停机失败卡的值文案（`0` = `✓ 正常` / `1` = `× 停机失败` / `2` = `? 未知`）。
///
/// 返回 `(文案, 样式槽位 0/1/2)`。
pub const fn stop_view(failed: bool) -> (&'static str, usize) {
    if failed {
        (TEXT_STOP_FAIL, 1)
    } else {
        (TEXT_STOP_OK, 0)
    }
}

/// 灯类卡的值（文案, 样式槽位 0/1/2）—— `None` ⇒ 「未知」（槽位 2），**不**臆造为「灯灭」。
pub const fn lamp_view(v: TriView) -> (&'static str, usize) {
    match v {
        TriView::On => (TEXT_LAMP_ON, 0),
        TriView::Off => (TEXT_LAMP_OFF, 1),
        TriView::Unknown => (TEXT_LAMP_UNKNOWN, 2),
    }
}

/// 灯类卡「亮」时的色（故障灯 `#DC3545` / 运行灯 `#28A745`）。
pub const fn lamp_on_color(is_fault: bool) -> Color {
    if is_fault {
        Palette::LINK_DOWN
    } else {
        Palette::LINK_OK
    }
}

/// 灯类卡「灭」时的色（`#5F6368` 空心）。
pub const fn lamp_off_color() -> Color {
    Palette::LINK_UNCONFIGURED
}

/// 未知 / 不可用时的灰（`#8C98AC`）。
pub const fn gray_color() -> Color {
    Palette::STOPPED
}

/// 触发源的**机器键**（**不上屏** —— 只用于与契约 `InterlockSourceItem.name` 比对）。
///
/// 存在的唯一理由与 `p2_config.rs` 的 `config_key` **同款**：`ui/tests.rs::ui_texts_covered_by_font_cmap`
/// 把 `ui/**` 里**所有**字符串字面量一律当"上屏候选"逐字查字形（宁可多查），而机器键是小写
/// ASCII（`estop` 里的 `e`/`t`/`o`/`p` **在生成字体里没有字形**）。经本函数标注 = 声明
/// "这个字面量**从不进 `lv_label`**"（见 `ui/tests.rs::NON_DISPLAY_SINKS` 的逐条白名单；
/// 其计数由 `p4_static_constraints` 自证为**恰 2 处**）。**不是**为了过网而把文案写残。
const fn source_key(name: &'static str) -> &'static str {
    name
}

/// 机器名 → 中文名的映射表（**逐条登记、只增不改**）；未命中 ⇒ [`display_safe`] 兜底（见 **IL6**）。
///
/// | 机器名 | 上屏 | 出处 |
/// |--------|------|------|
/// | `estop` | [`TEXT_SRC_ESTOP`] | UI §3.6 P4「触发源」行 |
/// | `door` | [`TEXT_SRC_DOOR`] | 同上 |
pub const SOURCE_LABELS: [(&str, &str); 2] = [
    (source_key("estop"), TEXT_SRC_ESTOP),
    (source_key("door"), TEXT_SRC_DOOR),
];

/// 触发源**上屏名**：先查 [`SOURCE_LABELS`]，未命中则取 [`display_safe`] 归一化后的机器名。
///
/// ⚠️ **未知名的行为（IL6，逐条可测）**：
/// - 命中映射表 ⇒ 中文名（`estop` → 「急停」）；
/// - 未命中 ⇒ **原样保留可辨认的机器名**（ASCII 经 `display_safe`：小写 → 大写同族、
///   cmap 外 ASCII → `?`）—— **不伪造**中文名、**不出豆腐块**；
/// - **非 ASCII**（后端直接给了中文名）⇒ `display_safe` **原样透传** ⇒ 含缺字时仍是豆腐块
///   （残余风险，防线在后端字段命名 / 字库，与 `p2_config.rs` 的 PD13 同口径）。
///
/// ⚠️ **匹配是 ASCII 大小写不敏感的**（**IL26**）：契约 `source_token()` 产出的是**小写** token，
/// 但帧 / 拒绝路径的 `name` 是**自由文本**；写成 `ESTOP` / `E_stop` 时若按精确匹配落空，屏上会
/// 出现 `ESTOP`（大写同族，勉强可读）甚至 `E–STOP`（`_` 被 `display_safe` 改写成短破折）——
/// 而这里**明明知道**它就是「急停」。故用 `eq_ignore_ascii_case`（**只对 ASCII** 生效，
/// 中文名不受影响）。
pub fn source_label(name: &str) -> String {
    for (key, label) in SOURCE_LABELS {
        if name.eq_ignore_ascii_case(key) {
            return (*label).to_string();
        }
    }
    display_safe(name)
}

/// 触发源行状态词（文字信道；色 / 图形由行内 `LedIndicator` 承担）。
pub const fn source_status_text(tripped: bool) -> &'static str {
    if tripped {
        TEXT_SRC_TRIPPED
    } else {
        TEXT_SRC_UNTRIPPED
    }
}

/// 触发源卡卡头（含数量，IL-01.2：`触发源 · N` —— 全角括号缺字，见 **IL1**）。
pub fn sources_title(count: usize) -> String {
    format!("{TEXT_SOURCES_TITLE}{TEXT_CLAUSE_SEP}{count}")
}

/// 触发源**超出 [`SOURCE_ROW_POOL`] 上限**时的卡头提示（` · 还有 N 条`；未超出 ⇒ 空串）。
///
/// **这是 `IL9` 上限取值的补偿**：行池固定 4 行是为了「`new()` 一次性建齐 + `render()`
/// 不可失败 + 1 Hz 零对象 churn」（同 P1 `alarm_rows` 池口径），但 UI §6.4（`：599`）明写
/// 「**全部源一次性列出，不折叠**」⇒ 超限必须**可见**（不得静默）。**不改行池上限**
/// （版式取向，且 IL9 已登记），补偿就是这一行卡头提示。
///
/// **命中面**：未超出时返回空串 ⇒ 卡头与既有文案**逐字不变**（`触发源 · N`）。
pub fn sources_overflow_note(count: usize) -> String {
    let hidden = count.saturating_sub(SOURCE_ROW_POOL);
    if hidden == 0 {
        String::new()
    } else {
        format!("{TEXT_CLAUSE_SEP}{TEXT_OVERFLOW_MORE} {hidden} {TEXT_OVERFLOW_UNIT}")
    }
}

/// 两个操作按钮的可用性 + **就地表原因**（UI §6.4「操作与拒绝原因」表逐行）。
///
/// 判据顺序（**从高到低**，先满足者胜）：
/// 1. `!available` ⇒ 两按钮 `disabled` +「联锁状态不可用」（§8.3 专行，fail-closed）；
/// 2. `!enabled` ⇒ 两按钮 `disabled` +「联锁未启用」；
/// 3. `submitting` ⇒ 两按钮 `disabled`（无就地文案，见 **IL15**）；
/// 4. `M1 授权重启` 追加：`latched` ⇒「处于自锁态 · 须先释放联锁」（IL-03；§6.4 明列）。
///
/// **两按钮都不因「后端才会判定的前置条件」在本地预判置灰**（B2b-3 评审整改 ①；取向见 **IL18**）：
/// - `人工释放联锁`：不因「源未复位 / 保持不足 / 非 latch」置灰；
/// - `M1 授权重启`：**不因 `stop_failed` 置灰**。§6.4「操作与拒绝原因」表与 §9 F18 **只**要求
///   **latch 态**置灰 M1，**没有** `stop_failed` 这一行；本地预判「停机未确认」会**替代**后端回的
///   `RejectedPrecondition[StopPending]` 的**具体**原因（现场只看到灰按钮、看不到为什么）⇒ 与
///   **IL18** 对 `release` 的取向是同一条道理（EDGE-12「不得静默失败」）。
///
///   ⚠️ **订正（单元 J 第一轮整改；第二轮 I-2 再订正措辞）**：放开后后端**实际由 `latched`
///   规则先挡** —— `stop_failed ⟹ latched` 已由 `mark_stop_failed` 的 `latched` 门**按构造成立**
///   （写点在锁内判定，详见 `interlock.rs` 的同名文档）⇒ `!latched && stop_failed` **已被后端
///   构造性关闭**，故后端回的是 `Latched`（`处于自锁态 · 须先释放联锁`），**不是** `StopPending`。
///   `StopPending` 的具体原因（`停机未确认 · 暂不可授权重启`，见 [`reject_text`]）在后端这道门
///   **成立期间不可达**（纯纵深防御门）。此订正**只改注释**：本地仍**不**按 `stop_failed` 预判置灰
///   （取向不变，IL18 的结论不受影响）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpState {
    /// 释放按钮是否禁用。
    pub release_disabled: bool,
    /// 释放按钮的就地原因（`None` = 无）。
    pub release_reason: Option<&'static str>,
    /// M1 授权重启按钮是否禁用。
    pub restart_disabled: bool,
    /// M1 授权重启按钮的就地原因（`None` = 无）。
    pub restart_reason: Option<&'static str>,
}

/// 计算两个操作按钮的可用性矩阵（见 [`OpState`] 的判据顺序）。
pub fn op_state(s: &InterlockSection, submitting: bool) -> OpState {
    // ①②：全局阻塞（状态源不可用 / 功能未启用）—— 两按钮同因。
    let global: Option<&'static str> = if !s.available {
        Some(TEXT_STATE_UNAVAILABLE)
    } else if !s.enabled {
        Some(TEXT_NOT_ENABLED)
    } else {
        None
    };
    // ③：提交中（无就地文案）。
    let busy = global.is_none() && submitting;
    let release_disabled = global.is_some() || busy;
    // ④：M1 专属阻塞（仅在无全局阻塞时才有意义 —— 全局原因优先级更高）。
    // **只有 latch 态**（§6.4 拒绝原因表 / IL-03）；`stop_failed` **不**参与 ——
    // 它是后端 `StopPending` 的判据，本地预判会替代后端的具体原因（见上文与 **IL18**）。
    let restart_local: Option<&'static str> = if s.latched {
        Some(TEXT_REASON_LATCHED)
    } else {
        None
    };
    OpState {
        release_disabled,
        release_reason: if global.is_some() { global } else { None },
        restart_disabled: release_disabled || restart_local.is_some(),
        restart_reason: global.or(restart_local),
    }
}

/// `InterlockReject` → **就地文案**（UI §6.4 拒绝原因表 + §3.6 P4「操作」行；EDGE-12：
/// **不得静默失败** —— 每个变体都有**具体**文案，7 个变体逐一可测）。
///
/// ⚠️ **不使用契约的 `user_message()`**：它含全角 `：`/`，` 与**小写机器名**（`estop`），
/// 两者都不在生成字体 cmap 内 ⇒ 真机是豆腐块。本函数按**结构化字段**重建文案
/// （源名经 [`source_label`] 映射），是「显示侧」的唯一出口（契约本身不动，见 **IL17**）。
pub fn reject_text(r: &InterlockReject) -> String {
    match r {
        InterlockReject::SourcesNotReset { remaining } => {
            if remaining.is_empty() {
                // 契约允许空 `remaining`（无字段约束）⇒ 只报**能确定**的事实，不编造源名。
                TEXT_REJECT_SOURCES.to_string()
            } else {
                let names: Vec<String> = remaining.iter().map(|n| source_label(n)).collect();
                format!(
                    "{TEXT_REJECT_SOURCES}{TEXT_CLAUSE_SEP}{}",
                    names.join(TEXT_NAME_SEP)
                )
            }
        }
        // 单行字面量（**不得**用 `\` 行接续）：`ui/tests.rs` 的字面量扫描器按「非原始字符串
        // 不跨行」自证 —— 跨行会被判成**扫描器失真**并响亮失败（B2b-3 实测踩过这一条）。
        InterlockReject::HoldNotElapsed {
            need_secs,
            remaining_secs,
        } => format!("{TEXT_REJECT_HOLD}{TEXT_CLAUSE_SEP}{TEXT_HOLD_MORE} {remaining_secs} {TEXT_SECONDS}{TEXT_CLAUSE_SEP}{TEXT_HOLD_NEED} {need_secs} {TEXT_SECONDS}"),
        InterlockReject::Latched => TEXT_REASON_LATCHED.to_string(),
        InterlockReject::StopPending => {
            format!("{TEXT_STOP_PENDING}{TEXT_CLAUSE_SEP}{TEXT_STOP_PENDING_TRAIL}")
        }
        InterlockReject::NotEnabled => TEXT_NOT_ENABLED.to_string(),
        InterlockReject::Busy => TEXT_OP_BUSY.to_string(),
        InterlockReject::Internal(detail) => {
            format!("{TEXT_INTERNAL}{TEXT_CLAUSE_SEP}{}", display_safe(detail))
        }
    }
}

/// 倒计时文案（UI §6.4「保持时间不足」行：按钮旁显剩余秒数）。
pub fn hold_text(remaining_secs: u64) -> String {
    format!("{TEXT_REJECT_HOLD}{TEXT_CLAUSE_SEP}{TEXT_HOLD_MORE} {remaining_secs} {TEXT_SECONDS}")
}

/// 提交载荷：UI **观测到**的联锁态（供后端做乐观并发检查，EDGE-19）。
///
/// - `observed_latched` = 帧内 `latched`；
/// - `observed_sources` = 帧内**全部**源名（**机器名、非中文标签** —— 后端按 token 比对；
///   取全量而非仅 `tripped` 的理由见 **IL16**）。
pub fn op_payload(s: &InterlockSection) -> InterlockOpPayload {
    InterlockOpPayload {
        observed_latched: s.latched,
        observed_sources: s.sources.iter().map(|i| i.name.clone()).collect(),
    }
}

/// 源名列表的上屏文本（弹层明细；无源 ⇒ [`TEXT_NONE`]）。
pub fn sources_text(s: &InterlockSection) -> String {
    if s.sources.is_empty() {
        TEXT_NONE.to_string()
    } else {
        let names: Vec<String> = s.sources.iter().map(|i| source_label(&i.name)).collect();
        names.join(TEXT_NAME_SEP)
    }
}

/// 操作种类（决定弹层文案 / 成功 Toast / 意图槽）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpKind {
    /// 人工释放联锁（`POST .../interlock/release`）。
    Release,
    /// M1 授权重启（`POST .../interlock/ack_m1`）。
    AckM1,
}

impl OpKind {
    /// 确认弹层标题（UI §6.4 强确认弹层段）。
    pub(crate) const fn dialog_title(self) -> &'static str {
        match self {
            OpKind::Release => TEXT_DIALOG_TITLE_RELEASE,
            OpKind::AckM1 => TEXT_DIALOG_TITLE_ACK_M1,
        }
    }

    /// 「影响范围」段（§7.3：L2 **必须**为**具体副作用**，不得泛化措辞）。
    pub(crate) const fn impact(self) -> &'static str {
        match self {
            OpKind::Release => TEXT_IMPACT_RELEASE,
            OpKind::AckM1 => TEXT_IMPACT_ACK_M1,
        }
    }

    /// 成功 Toast 文案（UI §3.6 全局行）。
    pub(crate) const fn toast_ok(self) -> &'static str {
        match self {
            OpKind::Release => TEXT_TOAST_RELEASED,
            OpKind::AckM1 => TEXT_TOAST_ACKED,
        }
    }
}

/// 弹层明细的一行（三段式的**可拥有**形态；[`ConfirmDetail`] 借用 `&str`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetailRow {
    /// 字段名（静态字面量）。
    pub field: &'static str,
    /// 旧值 / 当前值。
    pub before: String,
    /// 新值 / 操作后目标态。
    pub after: String,
}

/// 构造弹层明细（UI §6.4 强确认弹层段「明细列出…」的三项）。
///
/// 三段式口径见 **IL10**：`当前 → 操作后目标态`；本次操作**不改动**的项取 `新值 = 旧值`。
pub(crate) fn dialog_details(s: &InterlockSection, op: OpKind) -> Vec<DetailRow> {
    let src = sources_text(s);
    let latch = if s.latched {
        TEXT_LATCH_HELD
    } else {
        TEXT_LATCH_UNHELD
    };
    let stop = if s.stop_failed {
        TEXT_STOP_FAIL
    } else {
        TEXT_STOP_OK
    };
    match op {
        // 释放：触发源是**前置条件**（须全部复位）⇒ 目标态「无」；latch 由「已保持」清为「未保持」；
        // 停机确认态本操作不改动 ⇒ 前后同值（如实表达"不变"）。
        OpKind::Release => vec![
            DetailRow {
                field: TEXT_DETAIL_SOURCES,
                before: src,
                after: TEXT_NONE.to_string(),
            },
            DetailRow {
                field: TEXT_DETAIL_LATCH,
                before: latch.to_string(),
                after: TEXT_LATCH_UNHELD.to_string(),
            },
            DetailRow {
                field: TEXT_DETAIL_STOP,
                before: stop.to_string(),
                after: stop.to_string(),
            },
        ],
        // M1 授权重启：触发源不改动；latch 本操作的**前提**是「未保持」（latch 时按钮禁用）⇒ 不变；
        // 授权后停机确认态（`ack.stopped`）成立 ⇒ 目标「正常」。
        OpKind::AckM1 => vec![
            DetailRow {
                field: TEXT_DETAIL_SOURCES,
                before: src.clone(),
                after: src,
            },
            DetailRow {
                field: TEXT_DETAIL_LATCH,
                before: latch.to_string(),
                after: latch.to_string(),
            },
            DetailRow {
                field: TEXT_DETAIL_STOP,
                before: stop.to_string(),
                after: TEXT_STOP_OK.to_string(),
            },
        ],
    }
}

/// 两段联锁状态的**展示相关等价**（**忽略 `ts_ms`**）—— 陈旧倒计时的清理判据（**IL27**）。
///
/// **为何必须忽略 `ts_ms`**：它是帧内采集时刻，**每帧都变**；若纳入比较，1 Hz 的正常心跳会让
/// 「内容没变」被误判成「新帧」⇒ 每秒清一次倒计时 ⇒ IL14 的「还需 N 秒」在屏上活不过 1 s。
/// 其余字段全部参与（任一变化都可能使「保持时间」的基准失效）。
pub fn section_display_eq(a: &InterlockSection, b: &InterlockSection) -> bool {
    a.available == b.available
        && a.enabled == b.enabled
        && a.latched == b.latched
        && a.stop_failed == b.stop_failed
        && a.sources == b.sources
        && a.fault_lamp == b.fault_lamp
        && a.run_lamp == b.run_lamp
        && a.release_hold_secs == b.release_hold_secs
}

/// 就地原因带的档位（样式表下标即 [`ReasonSlot::index`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReasonSlot {
    /// 灰（不可用）。
    Gray,
    /// 琥珀（前置条件 / 倒计时）—— `#FFB020`，UI §6.4「保持时间不足」行指定的倒计时色。
    Amber,
    /// 危险红（拒绝 / 冲突）—— `#FF6B6B`，UI §6.4 拒绝原因表指定的红字色。
    Red,
}

impl ReasonSlot {
    /// 样式表下标（与 `Core::reason_styles` 的槽位一一对应）。
    pub(crate) const fn index(self) -> usize {
        match self {
            ReasonSlot::Gray => 0,
            ReasonSlot::Amber => 1,
            ReasonSlot::Red => 2,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3′. U-73 消防上屏的**纯逻辑**（不触碰 LVGL ⇒ 可独立单测）
// ═══════════════════════════════════════════════════════════════════════════

/// 分组标题（**唯一来源** = `display-proto::peripherals_labels::GROUP_TITLES`）。
///
/// 本页**不自造中文标题**（硬约束：上屏中文一律取自 `display-proto`）；键不存在 ⇒ 返回空串
/// （**不臆造**），并由单测 `p4_group_keys_exist` 把本页用到的键逐个钉死。
pub(crate) fn group_title(key: &str) -> &'static str {
    GROUP_TITLES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, t)| *t)
        .unwrap_or("")
}

/// **块名 / 分组键等机器 token**（小写 ASCII，**从不上屏**）的显式标注。
///
/// 与 `source_key(` / `config_key(` / `module_key(` **同一条非屏显口径**（`ui/tests.rs` 的
/// [`NON_DISPLAY_SINKS`]）：帧内的块名（`fire_sys` / `fire_det`）与分组键（`fire_level` …）是
/// **机器键**，上屏的是它们查出来的中文（短标签 / 标题）；键本身的小写字母与 `_` 在生成字体里
/// **没有字形**。本函数是**恒等映射**，只为把"这些字面量不进 `lv_label`"写在调用点 ——
/// 计数由 `ui/tests.rs::p4_static_constraints` 钉死（防把**上屏串**塞进来静默逃过码表网）。
pub(crate) const fn block_key(name: &'static str) -> &'static str {
    name
}

/// 下钻表头 8 列的文本（**逐列取自 `display-proto`**，本页只拼分隔符）。
///
/// 列序：序号 │ 地址 │ 状态 │ 烟雾 `dB/M` │ 温度 `℃` │ CO `ppm` │ VOC `ppm` │ H2 `ppm`。
/// 「状态」列的**位名**（报警总状态 / 故障总状态）也在此列出 —— 行内格位放不下两个位名
/// （见 **IL29⑥**），故位名上提到表头，行内保留「圆点 + 活跃 / 非活跃」双通道。
///
/// ⚠️ **分隔符 = 紧凑式 `报警总状态/故障总状态`**（T21c-1-r1 订正，评审 W-2）：带列名 /
/// 带空格的旧形态 `状态 · 报警总状态 / 故障总状态` 实测 **342 px** > 列宽
/// [`DRILL_COL_STATUS`](328) ⇒ `LongMode::DOTS` 会把**第 2 个位名**截成「故障总…」。
/// 去列名与空格后实测 **249 px ≤ 328** ⇒ 两个位名（bit12 / bit14）**都完整可读**；
/// 列的语义由内容自身承担（该列两半槽各一位名，见表头右侧 6 列仍带单位）。
/// 本断言由 `ui/tests.rs` 的 T-25 段用**生产字体的 `adv_w`** 复算（口径同
/// `measured_text_px`；`adv_w` 求和为**上界**、不计 kerning）。
pub(crate) fn drill_head_texts() -> Vec<String> {
    let t = FIRE_DET_TEMPLATE_LABELS;
    let specs = mupc_display_proto::peripherals_labels::data1_decompose();
    let with_unit = |label: &str, unit: Option<&str>| match unit {
        Some(u) => format!("{label} {u}"),
        None => label.to_string(),
    };
    vec![
        ui_text::COL_SEQ.to_string(),
        t[0].to_string(),
        // 紧凑式（去列名与空格）⇒ 两个位名都放得下（见本函数的文档注；T21c-1-r1 订正）
        format!("{}/{}", ui_text::BIT_ALARM_TOTAL, ui_text::BIT_FAULT_TOTAL),
        with_unit(&specs[0].label, specs[0].unit.as_deref()),
        with_unit(&specs[1].label, specs[1].unit.as_deref()),
        with_unit(
            t[3],
            mupc_display_proto::peripherals_labels::unit_for(
                PeriphRole::Fire,
                block_key("fire_det"),
                4,
            ),
        ),
        with_unit(
            t[4],
            mupc_display_proto::peripherals_labels::unit_for(
                PeriphRole::Fire,
                block_key("fire_det"),
                5,
            ),
        ),
        with_unit(
            t[5],
            mupc_display_proto::peripherals_labels::unit_for(
                PeriphRole::Fire,
                block_key("fire_det"),
                6,
            ),
        ),
    ]
}

/// catalog 里的站（`None` = 该站未进 catalog / catalog 未取到）。
fn cat_station(cat: Option<&PeripheralCatalog>, role: PeriphRole) -> Option<&CatalogStation> {
    cat.and_then(|c| c.stations.iter().find(|s| s.role == role))
}

/// catalog 里的点（`(role, block, at)` 三元组；唯一查找入口）。
fn cat_point<'a>(
    cat: Option<&'a PeripheralCatalog>,
    role: PeriphRole,
    block: &str,
    at: u16,
) -> Option<&'a CatalogPoint> {
    cat_station(cat, role)?
        .blocks
        .iter()
        .find(|b| b.name == block)?
        .points
        .iter()
        .find(|p| p.at == at)
}

/// 帧内的站（`None` = 帧内**不含**该站 ⇒ 缺席，§15.2.2）。
fn frame_station(sec: &PeripheralsSection, role: PeriphRole) -> Option<&PeripheralStation> {
    sec.stations.iter().find(|s| s.role == role)
}

/// 帧内的点（`(role, block, at)` 三元组）。
fn frame_point<'a>(
    sec: &'a PeripheralsSection,
    role: PeriphRole,
    block: &str,
    at: u16,
) -> Option<&'a PointValue> {
    frame_station(sec, role)?
        .blocks
        .iter()
        .find(|b| b.name == block)?
        .values
        .iter()
        .find(|p| p.at == at)
}

/// 站级态（catalog `enabled` ∪ 帧内 `online`；**判据单一** —— `state::station_state`）。
fn station_state_of(
    cat: Option<&PeripheralCatalog>,
    sec: &PeripheralsSection,
    role: PeriphRole,
) -> StationState {
    state::station_state(
        cat_station(cat, role).map(|s| s.enabled),
        frame_station(sec, role).map(|s| s.online),
    )
}

/// 单点展示视图（帧内点值 × 站级态 × catalog 元数据；`None` = 帧内**没有这个点**）。
///
/// 缺 catalog ⇒ `label = None` ⇒ 屏显「名称未获取」（**值照常显示**，§15.3.1）。
fn periph_view(
    cat: Option<&PeripheralCatalog>,
    sec: &PeripheralsSection,
    role: PeriphRole,
    block: &str,
    at: u16,
) -> Option<PeriphView> {
    let pv = frame_point(sec, role, block, at)?;
    let st = station_state_of(cat, sec, role);
    Some(PeriphView::derive(st, pv, cat_point(cat, role, block, at)))
}

/// 数值行的展示文本（`数值 单位` / `--`）。降级时**不含单位**（不伪造量纲）。
fn periph_row_text(v: &PeriphView) -> String {
    match v.value {
        Some(x) => match &v.unit {
            Some(u) => format!("{} {u}", crate::ui::pages::fmt_decimals(x, v.decimals)),
            None => crate::ui::pages::fmt_decimals(x, v.decimals),
        },
        None => crate::ui::pages::PLACEHOLDER.to_string(),
    }
}

/// **A2 灭火瓶压力**行的展示文本 + 样式档（**纯函数** ⇒ T-14 可独立断言）。
///
/// 判据（EDGE-23 / EX-11）：
/// - 段不可用（`section_unavailable`）⇒ 段级通告（「外设数据不可用」）；
/// - `cylinder_configured == Some(false)` ⇒ **「未配置」**且**忽略 `v`**
///   （`value` 已由 [`PeriphView::apply_cylinder`] 清成 `None` ⇒ 屏上**不含 `0 kPa`**）；
/// - 站在线且已配置 ⇒ `数值 + kPa`（降级时也不带单位）。
pub(crate) fn a2_display(
    view: Option<&PeriphView>,
    notice: Option<&str>,
    section_unavailable: bool,
) -> (String, usize) {
    if section_unavailable {
        return (
            notice.unwrap_or(ui_text::PERIPH_UNAVAILABLE).to_string(),
            A2_STYLE_WEAK,
        );
    }
    match view {
        Some(v) => match v.missing {
            Some(_) => (
                v.missing_text().unwrap_or(ui_text::UNAVAILABLE).to_string(),
                A2_STYLE_WEAK,
            ),
            None => (periph_row_text(v), A2_STYLE_NORMAL),
        },
        None => (
            notice.unwrap_or(ui_text::NAME_UNKNOWN).to_string(),
            A2_STYLE_WEAK,
        ),
    }
}

/// 「数据 1」的拆解规格：**catalog 优先**（运行时真源），缺则回退 `display-proto` 的锁定模板
/// （[`mupc_display_proto::peripherals_labels::data1_decompose`]）—— 两条来源都是
/// `DecodeFrom`，屏侧**不自行猜位序**（F21.5）。
fn data1_specs(cat: Option<&PeripheralCatalog>) -> Vec<Decompose> {
    match cat_point(cat, PeriphRole::Fire, block_key("fire_det"), 3) {
        Some(p) if !p.decompose.is_empty() => p.decompose.clone(),
        _ => mupc_display_proto::peripherals_labels::data1_decompose(),
    }
}

/// 探测器汇总（A4 汇总行）：登记数取 catalog 的 `fire_det_count`（renames），可读 / 报警 /
/// 故障 / 离线由**帧内 `fire_det` 携带的点**（以及 `fire_sys_8..13` 这一只）算得。
fn fire_summary(cat: Option<&PeripheralCatalog>, sec: &PeripheralsSection) -> FireSummary {
    // 登记数：`fire_sys_7` 在 catalog 里被 `renames` 覆盖为 `fire_det_count`（组帧侧口径）。
    let registered = cat_station(cat, PeriphRole::Fire)
        .and_then(|s| s.blocks.iter().find(|b| b.name == block_key("fire_sys")))
        .and_then(|b| {
            b.renames
                .iter()
                .find(|(at, _)| *at == 7)
                .map(|(_, n)| n.clone())
        })
        .and_then(|name| {
            sec.stations
                .iter()
                .find(|s| s.role == PeriphRole::Fire)
                .and_then(|s| s.blocks.iter().find(|b| b.name == block_key("fire_sys")))
                .and_then(|b| b.values.iter().find(|pv| b.key(pv) == name))
                .and_then(|pv| pv.v)
        })
        .map(|v| v.round().clamp(0.0, f64::from(u16::MAX)) as u16);

    // 可读只数 = 帧内 `fire_det` 块携带的探测器只数（**含第 1 只之外的**）；报警 / 故障 / 离线
    // 由每只的状态字（bit12 / bit14）与地址点是否可读算得。
    let mut sum = FireSummary {
        registered,
        ..Default::default()
    };
    let station = frame_station(sec, PeriphRole::Fire);
    let Some(station) = station else {
        return sum;
    };
    // 探测器 1 在 `fire_sys` 的 at 8..13；其余在 `fire_det`（每只 6 点）
    let mut units: Vec<(&PointValue, &PointValue)> = Vec::new();
    if let Some(b) = station
        .blocks
        .iter()
        .find(|b| b.name == block_key("fire_sys"))
    {
        if let (Some(addr), Some(st)) = (
            b.values.iter().find(|pv| pv.at == 8),
            b.values.iter().find(|pv| pv.at == 9),
        ) {
            units.push((addr, st));
        }
    }
    if let Some(b) = station
        .blocks
        .iter()
        .find(|b| b.name == block_key("fire_det"))
    {
        for chunk in b.values.chunks_exact(FIRE_DET_POINTS_PER_UNIT) {
            if let (Some(addr), Some(st)) = (chunk.first(), chunk.get(1)) {
                units.push((addr, st));
            }
        }
    }
    sum.readable = units.len().min(usize::from(u16::MAX)) as u16;
    for (addr, st) in &units {
        let readable = addr.flag == FieldFlag::Valid && addr.v.is_some_and(f64::is_finite);
        if !readable {
            sum.offline += 1;
        }
        if let Some(w) = st.v {
            if state::bit_active(w, 12) {
                sum.alarm += 1;
            }
            if state::bit_active(w, 14) {
                sum.fault += 1;
            }
        }
    }
    sum
}

/// 火警等级值 → 语义色档（**唯一映射点**；表外 / 未定义 ⇒ 中性档，**绝不用绿**）。
// ⚠️ **非 `const fn`（MSRV 1.75）**：`f64::is_finite` / `f64::round` 的 **const 求值**
// 分别到 Rust 1.83 / 1.90 才稳定，而本 crate 的 MSRV 是 1.75（`clippy::incompatible_msrv` 会红）
// ⇒ 本函数只在运行期调用（构造样式表 / 用例），不需要 const 上下文。
pub(crate) fn fire_level_slot(v: f64) -> usize {
    if !v.is_finite() {
        return FIRE_SLOT_UNKNOWN;
    }
    match v.round() as i64 {
        0 => FIRE_SLOT_OK,
        1 => FIRE_SLOT_WARN,
        2 | 4 => FIRE_SLOT_DANGER,
        5 => FIRE_SLOT_STOP,
        _ => FIRE_SLOT_UNKNOWN,
    }
}

/// 火警等级色档 → **色值**（**不新增色值**：全部取 UI §3.2 既有语义色）。
///
/// **单一真源**：构造期用它建三份样式表（竖条 / 图标 / 文案），单测也断言它 ⇒
/// "「未知」绝不用绿"这条判据**机械可测**（T-15）。
pub(crate) const fn fire_slot_color(slot: usize) -> Color {
    match slot {
        FIRE_SLOT_OK => Palette::OK,
        FIRE_SLOT_WARN => Palette::STALE,
        FIRE_SLOT_DANGER => Palette::DANGER,
        FIRE_SLOT_STOP => Palette::STOPPED,
        _ => Palette::TEXT_WEAK,
    }
}

/// A1 位行的色档 → 色值（活跃 = 告警红 / 非活跃 = 次文 / 未定义 = 弱注）。
pub(crate) const fn a1_slot_color(slot: usize) -> Color {
    match slot {
        A1_STYLE_ACTIVE => Palette::DANGER,
        A1_STYLE_INACTIVE => Palette::TEXT_SECOND,
        _ => Palette::TEXT_WEAK,
    }
}

/// 火警等级的图标（**三重冗余的图标通道**）：正常 `✓` / 报警类 `⚠` / 紧急停止 `■` /
/// 表外与不可得 `?`。全部字形在生成字体 cmap 内（`✓ ⚠ ■ ?`）。
pub(crate) fn fire_level_icon(text: &str, view: Option<&PeriphView>) -> &'static str {
    let v = view.and_then(|v| v.value);
    match v {
        Some(v) if v.is_finite() => match v.round() as i64 {
            0 => ICON_STATE_UNLATCHED,
            1..=4 => ICON_STATE_LATCHED,
            5 => "■",
            _ => ICON_STATE_UNAVAILABLE,
        },
        // 未取得值 ⇒ 按**文案**判（catalog 缺、值缺时文案恒「未知」⇒ `?`）
        _ => {
            let _ = text;
            ICON_STATE_UNAVAILABLE
        }
    }
}

/// 触发源卡的**卡体高**（`apply_section` 与 §A 组的 y 联动共用 —— 单一算式，见
/// `Core::layout_fire_groups`）。
fn source_body_h(s: &InterlockSection) -> i32 {
    if s.available {
        (s.sources.len().min(SOURCE_ROW_POOL) as i32 * SOURCE_ROW_H).max(EMPTY_H)
    } else {
        UNAVAILABLE_H
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 静态子树件（**全部拥有型句柄存进结构体** —— 见模块文档「所有权纪律」）
// ═══════════════════════════════════════════════════════════════════════════

/// 一行触发源（左侧 `LedIndicator` 双形态 + 右侧状态词）。
///
/// **两种 `LedIndicator` 并建、`show_only` 切换**：组件的图标字形与灯色在**构造期固定**
/// （`set_lit` 只改灯的亮灭，不改字形与颜色），而「已触发 `●` 红实心 / 未触发 `○` 灰空心」
/// 需要字形与颜色**同时**切换 ⇒ 预建两支、切换可见性（**不重建对象** ⇒ 1 Hz 零 churn）。
///
/// ⚠️ **组件能力缺口（如实标注）**：`LedIndicator`（`ui/components.rs`，本批**禁改**）**没有
/// `set_text` 口**（只有 `text()` 读回）⇒ 上屏名**只能建时给**。故本行为「名」保留**一次性重建**
/// 路径：[`SourceRow::apply`] 只在**上屏名变化**时重建两支 `LedIndicator`（其余帧只改文本 /
/// 可见性）；重建失败（LVGL 内存池耗尽一类）**只记 stderr 并保留旧件** —— 不 panic、不上屏、
/// 不影响 `render` 的不可失败性。**若组件补 `set_text`，此路径可整体删除**（届时渲染期零创建）。
struct SourceRow {
    /// 行容器（**拥有型**：Drop 即级联删除本行子树）。
    obj: Obj,
    /// 已触发形态（`●` + 灯亮 + `#DC3545`）。
    tripped_led: RefCell<Option<LedIndicator>>,
    /// 未触发形态（`○` + 灯灭 + `#5F6368`）。
    untripped_led: RefCell<Option<LedIndicator>>,
    /// 两支 `LedIndicator` 当前已建的**上屏名**（`None` = 尚未建）—— 变化检测的**唯一**依据。
    built_name: RefCell<Option<String>>,
    /// 右侧状态词（文字信道；`已触发` / `未触发`）。
    status: Label,
}

impl SourceRow {
    /// 建一行（`index` 决定 y 与斑马纹底色；`LedIndicator` 由首次 [`SourceRow::apply`] 建）。
    fn new(parent: &Obj, index: usize) -> Result<Self, LvglError> {
        // 斑马纹（UI §5.2 `ListItem`：偶 `#141F33` / 奇 `#1B2942`）；取零内边距的纯色块样式，
        // 免得默认主题的内边距把行内绝对定位元素推偏。
        let tint = if index % 2 == 0 {
            Palette::SURFACE
        } else {
            Palette::SURFACE_ALT
        };
        let obj = decor(parent, INNER_W, SOURCE_ROW_H, &theme::card_head_bar(tint))?;
        obj.set_pos(0, index as i32 * SOURCE_ROW_H);

        let status = text_label(&obj, "", TextSlot::Body, Palette::TEXT_SECOND)?;
        status.set_size(SOURCE_STATUS_W, TextSlot::Body.px() as i32);
        status.set_long_mode(LongMode::DOTS);
        status.set_pos(SOURCE_STATUS_X, SOURCE_ROW_STATUS_Y);

        Ok(Self {
            obj,
            tripped_led: RefCell::new(None),
            untripped_led: RefCell::new(None),
            built_name: RefCell::new(None),
            status,
        })
    }

    /// 重建两支 `LedIndicator`（**只有上屏名变化时**才走这里）。
    fn build(&self, shown: &str) -> Result<(), LvglError> {
        let tripped =
            LedIndicator::new(&self.obj, SOURCE_NAME_W, ICON_FILLED, shown, Palette::LINK_DOWN)?;
        tripped.set_pos(0, SOURCE_ROW_LED_Y);
        let untripped =
            LedIndicator::new(&self.obj, SOURCE_NAME_W, ICON_HOLLOW, shown, Palette::LINK_UNCONFIGURED)?;
        untripped.set_pos(0, SOURCE_ROW_LED_Y);
        untripped.set_lit(false);
        *self.tripped_led.borrow_mut() = Some(tripped);
        *self.untripped_led.borrow_mut() = Some(untripped);
        *self.built_name.borrow_mut() = Some(shown.to_string());
        Ok(())
    }

    /// 按 `(name, tripped)` 刷新本行（**正常路径只改文本 / 可见性**；见结构体文档的能力缺口注）。
    fn apply(&self, name: &str, tripped: bool) {
        let shown = source_label(name);
        let stale = match &*self.built_name.borrow() {
            Some(n) => n != &shown,
            None => true,
        };
        if stale {
            if let Err(e) = self.build(&shown) {
                report_led_rebuild_failure(&e);
                // 保留旧件（若有）：宁可显示上一拍的名字，也不清空 / 不 panic。
                return;
            }
        }
        self.status.set_text(source_status_text(tripped));
        let t = self.tripped_led.borrow();
        let u = self.untripped_led.borrow();
        if let (Some(t), Some(u)) = (t.as_ref(), u.as_ref()) {
            let objs = [t.obj(), u.obj()];
            show_only(&objs, Some(usize::from(!tripped)));
        }
    }

    /// 整行隐藏（该行无对应源）。
    fn hide(&self) {
        set_visible(&self.obj, false);
    }

    /// 行当前是否可见。
    fn visible(&self) -> bool {
        !self.obj.is_hidden()
    }

    /// 当前可见形态：`Some(true)` = 已触发形态 / `Some(false)` = 未触发形态 / `None` = 未建或
    /// 两支同时可见（不应发生 ⇒ 判成 `None` 而不是给一个可能错的答案）。
    fn tripped_visible(&self) -> Option<bool> {
        let t = self.tripped_led.borrow();
        let u = self.untripped_led.borrow();
        let (t, u) = (t.as_ref()?, u.as_ref()?);
        match (t.obj().is_hidden(), u.obj().is_hidden()) {
            (false, true) => Some(true),
            (true, false) => Some(false),
            _ => None,
        }
    }

    /// 当前可见形态的灯色通道。
    fn visible_color(&self) -> Option<Color> {
        match self.tripped_visible()? {
            true => self.tripped_led.borrow().as_ref().map(|l| l.color()),
            false => self.untripped_led.borrow().as_ref().map(|l| l.color()),
        }
    }

    /// 上屏名（读自当前形态的 `LedIndicator`）。
    fn shown_name(&self) -> Option<String> {
        self.tripped_led
            .borrow()
            .as_ref()
            .and_then(|l| l.text())
    }
}

/// 一张状态卡（卡头 `SectionTitle` + 值行「图标 + 文字」同标签 + 三态色）。
struct StatusCard {
    /// 卡对象（**拥有型**：Drop 即级联删除本卡子树）。
    obj: Obj,
    /// 卡头标签（离屏断言口径）。
    head: Label,
    /// 值行标签（图标与文字**同一标签**，如 `✓ 正常`；三通道 = 字形 + 文字 + 颜色）。
    value: Label,
    /// 值行三态样式（[0] 正 / [1] 负 / [2] 灰）。
    styles: [Rc<Style>; 3],
    /// 当前挂着的样式下标。
    style: Cell<usize>,
}

impl StatusCard {
    /// 建卡（`title` = 卡头文案；`colors` = [正, 负, 灰] 三态色）。
    fn new(parent: &Obj, x: i32, title: &str, colors: [Color; 3]) -> Result<Self, LvglError> {
        let obj = decor(parent, LAMP_CARD_W, LAMP_CARD_H, &theme::card())?;
        obj.set_pos(x, 0);

        let head = text_label(&obj, title, TextSlot::SectionTitle, Palette::TEXT_WEAK)?;
        head.set_pos(0, 0);

        let value = label(&obj, TextSlot::Label, colors[0])?;
        value.set_size(LAMP_VALUE_W, LAMP_VALUE_H);
        value.set_pos(0, LAMP_VALUE_Y);

        let styles = [
            theme::text(TextSlot::Label, colors[0]),
            theme::text(TextSlot::Label, colors[1]),
            theme::text(TextSlot::Label, colors[2]),
        ];
        let style = Cell::new(usize::MAX);
        set_style_index(value.obj(), &styles, &style, 0);

        Ok(Self {
            obj,
            head,
            value,
            styles,
            style,
        })
    }

    /// 设值（`slot` = 样式槽 0 / 1 / 2）。
    fn set(&self, text: &str, slot: usize) {
        self.value.set_text(text);
        set_style_index(self.value.obj(), &self.styles, &self.style, slot);
    }

    /// 移到 (x, y)。
    fn set_pos(&self, x: i32, y: i32) {
        self.obj.set_pos(x, y);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 页面核心（**共享可变态**：控件回调经 `Weak<Core>` 回访，避免 `Rc` 环）
// ═══════════════════════════════════════════════════════════════════════════

/// 意图回调槽（`Option` = 尚未接线 ⇒ no-op；**不出现在任何 `pub` 签名里**）。
type IntentSlot = RefCell<Option<Box<dyn FnMut(InterlockOpPayload)>>>;

/// 分页意图回调槽（U-73 下钻：请求第 N 页探测器明细；语义同 [`IntentSlot`]）。
type PageReqSlot = RefCell<Option<Box<dyn FnMut(u32)>>>;

/// 下钻明细表的**一行**（[`DRILL_CELLS`] 格；状态列占两格 —— 报警总状态 / 故障总状态）。
struct DrillRow {
    /// 格子（序号 / 地址 / 报警总状态 / 故障总状态 / 烟雾 / 温度 / CO / VOC / H2）。
    cells: Vec<Label>,
    /// 每格当前样式下标（避免每拍重复挂样式 —— 与 `set_style_index` 的短路口径一致）。
    cell_style: Vec<Cell<usize>>,
}

impl DrillRow {
    /// 整行显隐（**不新建 / 不删除对象**）。
    fn set_visible(&self, on: bool) {
        for c in &self.cells {
            c.set_hidden(!on);
        }
    }

    /// 置某格文本 + 样式档。
    fn set_cell(&self, i: usize, text: &str, styles: &[Rc<Style>; 4], slot: usize) {
        if let (Some(l), Some(cell)) = (self.cells.get(i), self.cell_style.get(i)) {
            l.set_text(text);
            set_style_index(l.obj(), styles, cell, slot);
        }
    }

    /// 置某格**数值**格（帧内点值；`flag != Valid` / 非有限 ⇒ `--` + 降级档）。
    fn set_num(&self, i: usize, pv: &PointValue, decimals: u8, styles: &[Rc<Style>; 4]) {
        let ok = pv.flag == FieldFlag::Valid && pv.v.is_some_and(f64::is_finite);
        match pv.v.filter(|_| ok) {
            Some(v) => self.set_cell(
                i,
                &crate::ui::pages::fmt_decimals(v, decimals),
                styles,
                DRILL_SLOT_VALUE,
            ),
            None => self.set_cell(
                i,
                crate::ui::pages::PLACEHOLDER,
                styles,
                DRILL_SLOT_DEGRADED,
            ),
        }
    }

    /// 置某格**由拆解算得**的数值（`None` ⇒ `--` + 降级档）。
    fn set_num_opt(&self, i: usize, v: Option<f64>, decimals: u8, styles: &[Rc<Style>; 4]) {
        match v.filter(|x| x.is_finite()) {
            Some(v) => self.set_cell(
                i,
                &crate::ui::pages::fmt_decimals(v, decimals),
                styles,
                DRILL_SLOT_VALUE,
            ),
            None => self.set_cell(
                i,
                crate::ui::pages::PLACEHOLDER,
                styles,
                DRILL_SLOT_DEGRADED,
            ),
        }
    }
}

/// 探测器汇总（A4 汇总行的数据源；由 catalog 的 `fire_det_count` ∪ 帧内 `fire_det` 展开算得）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FireSummary {
    /// 登记只数（catalog 的 `fire_sys_7` 重命名为 `fire_det_count` 的点；`None` = 未取数）。
    pub(crate) registered: Option<u16>,
    /// 实际可读只数（由帧内 `fire_det` 块携带的点数 / 6 决定）。
    pub(crate) readable: u16,
    /// 报警只数（状态字 bit12 活跃）。
    pub(crate) alarm: u16,
    /// 故障只数（状态字 bit14 活跃）。
    pub(crate) fault: u16,
    /// 离线只数（`+0 地址` 为 `RangeError` / 非有限 ⇒ 视为不可读 ⇒ 计离线）。
    pub(crate) offline: u16,
}

impl FireSummary {
    /// `登记 != 可读` ⇒ **必须显式提示**（F21.4 / EX-12：**不得静默裁剪**）。
    pub(crate) fn mismatch(&self) -> bool {
        matches!(self.registered, Some(n) if u32::from(n) != u32::from(self.readable))
    }
}

/// 下钻明细格的样式档（[`Core::drill_cell_styles`] 的下标）。
const DRILL_SLOT_VALUE: usize = 0;
const DRILL_SLOT_DEGRADED: usize = 1;
const DRILL_SLOT_ACTIVE: usize = 2;
const DRILL_SLOT_INACTIVE: usize = 3;

/// 火警等级的样式档（[`Core::fire_*_styles`] 的下标；**中立档 `未知` 绝不落绿**）。
const FIRE_SLOT_OK: usize = 0;
const FIRE_SLOT_WARN: usize = 1;
const FIRE_SLOT_DANGER: usize = 2;
const FIRE_SLOT_STOP: usize = 3;
const FIRE_SLOT_UNKNOWN: usize = 4;

/// A1 位行的样式档。
const A1_STYLE_ACTIVE: usize = 0;
const A1_STYLE_INACTIVE: usize = 1;
const A1_STYLE_UNDEF: usize = 2;
/// A2 主值的样式档。
const A2_STYLE_NORMAL: usize = 0;
const A2_STYLE_WEAK: usize = 1;

/// 页面共享核心。
struct Core {
    /// 页根容器（`992 × 624`，**不滚动** —— 契约 1′）。
    root: Obj,
    /// 滚动视口（`992 × 528`）。
    scroll: ScrollContainer,
    /// 固定操作条（`992 × 72`，不随滚动）。
    action_bar: Obj,
    /// 操作条右端弱注（**拥有型**；离屏断言口径）。
    audit_note: Label,
    /// 【安全总览带】容器（**常驻不滚动**；§15.4 / UI §6.4.1）。
    band: Obj,
    // ── 联锁总态卡 ──
    /// 联锁总态卡（**拥有型**：`Drop` 即 `lv_obj_delete`，**级联删除整张卡的子树** ——
    /// 本字段曾因是 `new()` 里的**局部变量**而在返回时被 `Drop`，导致总态卡（卡头 / 三态胶囊 /
    /// 图标 / 总态词 / 两条语义色条）**整棵消失**、而页面其余部分正常（"局部空白但无报错"）。
    /// 由 `ui/tests.rs::pages_chain` 的「骨架态 ⇒ 联锁状态不可用」断言当场抓出（B2b-3）。
    /// **凡 `decor` / `layout_box` / `Obj::create` 的返回值一律存进本结构体**，见模块文档纪律）。
    state_card: Obj,
    /// 卡顶 3 px 语义横条。
    state_bar_top: Obj,
    /// 卡左缘 6 px 语义竖条。
    state_bar_left: Obj,
    /// 卡头文案（静态）。
    _state_title: Label,
    /// latch 胶囊三态（已保持 / 未保持 / 不可用）—— `show_only` 切换，皮肤构造期固定。
    state_chips: [Rc<StatusChip>; 3],
    /// 总态卡的**区块级「冻结 / 数据过期」角标**（EDGE-03 / EDGE-20；**图标-only 形态**，
    /// 见 **IL29②**）。构造期建好、运行期**只切可见性 + 换图标**（判据 =
    /// [`crate::ui::pages::frame_mark`] 单一真源）；与触发源卡那张（192 px 文字胶囊）
    /// **不同形态、同一个共享构造点族**（`ui/pages/mod.rs`）。
    state_frozen: crate::ui::pages::FrozenBadge,
    /// 总态图标（`⚠` / `✓` / `?`）。
    state_icon: Rc<Label>,
    /// 总态词（96 px）。
    state_text_l: Rc<Label>,
    /// 三条语义色的样式表（横条 / 竖条 / 图标 / 总态词共用同一份色序）。
    state_styles: [Rc<Style>; 3],
    state_icon_styles: [Rc<Style>; 3],
    state_text_styles: [Rc<Style>; 3],
    state_bar_top_style: Cell<usize>,
    state_bar_left_style: Cell<usize>,
    state_icon_style: Cell<usize>,
    state_text_style: Cell<usize>,
    // ── 火警等级卡（F21；总览带右卡）──
    fire_card: Obj,
    /// 卡头文案（静态 = `GROUP_TITLES["fire_level"]`）。
    _fire_title: Label,
    /// 卡左缘语义竖条（三重冗余的**色**通道之一）。
    fire_bar_left: Obj,
    /// 图标（**图标**通道）。
    fire_icon: Rc<Label>,
    /// 枚举文案（64 px；**文字**通道）。
    fire_value: Rc<Label>,
    /// 5 档语义色样式（正常 / 警示 / 危险 / 停机 / 中性）。
    fire_bar_styles: [Rc<Style>; 5],
    fire_icon_styles: [Rc<Style>; 5],
    fire_value_styles: [Rc<Style>; 5],
    fire_bar_style: Cell<usize>,
    fire_icon_style: Cell<usize>,
    fire_value_style: Cell<usize>,
    // ── §A 消防四组（滚动区内；只读）──
    a1_card: Obj,
    /// A1 **段顶通告**行（段级 / 站级降级时的唯一通告行；无降级 ⇒ 隐藏并留白）。
    a1_notice: Label,
    a1_notice_style: Cell<usize>,
    /// A1 逐位 16 行（已定义位「位名 活跃/非活跃」；未定义位「未定义位 n」）。
    a1_rows: Vec<Label>,
    a1_row_style: Vec<Cell<usize>>,
    a1_styles: [Rc<Style>; 3],
    a2_card: Obj,
    /// A2 灭火瓶压力主值（32 px；`Some(false)` ⇒「未配置」）。
    a2_value: Label,
    a2_styles: [Rc<Style>; 2],
    a2_style: Cell<usize>,
    a3_card: Obj,
    /// A3 三行（烟感 / 温感 / 可燃状态字，各 3 段）。
    a3_rows: Vec<Label>,
    a4_card: Obj,
    /// A4 汇总行（登记 / 可读 / 报警 / 故障 / 离线）。
    a4_summary: Label,
    /// A4 不一致提示（`N != M` 时可见；**不得静默裁剪**）。
    a4_mismatch: Label,
    /// A4「查看明细」按钮（进入下钻）。
    a4_detail: Rc<TextButton>,
    // ── 下钻视图（探测器明细；**不是页面**）──
    drill: Obj,
    drill_title: Label,
    /// 表头 8 列。
    drill_head: Vec<Label>,
    /// 行区（**独立滚动容器**）。
    drill_rows_box: ScrollContainer,
    /// 行池（[`DRILL_ROW_POOL`]）。
    drill_rows: Vec<DrillRow>,
    drill_prev: Rc<TextButton>,
    drill_next: Rc<TextButton>,
    /// 「收起」（**视图顶部右端**，F11.3 的固定出口）。
    drill_collapse: Rc<TextButton>,
    drill_page_text: Label,
    /// 失败 / 源不可用文案（「明细不可用」/「消防源不可用」）。
    drill_fail: Label,
    drill_retry: Rc<TextButton>,
    /// 明细格 4 档样式（值 / 降级 / 位活跃 / 位非活跃）。
    drill_cell_styles: [Rc<Style>; 4],
    // ── U-73 数据（帧 + catalog + 明细页；全部由外部注入）──
    /// 最近一帧的外设段（缺帧 ⇒ 契约缺省 = 不可用）。
    periph: RefCell<PeripheralsSection>,
    /// 元数据目录（一次性 GET；`None` = 未取到 ⇒ 中文名显「名称未获取」）。
    catalog: RefCell<Option<PeripheralCatalog>>,
    /// 探测器明细当前页（`None` = 未取到 / 未请求）。
    fire_page: RefCell<Option<FireDetectorPage>>,
    /// 明细端点失败（非 2xx ⇒ 屏上只显本地固定文案 + 重试；R-4）。
    fire_page_failed: Cell<bool>,
    /// 最近一次请求的页码（1 起）。
    fire_page_req: Cell<u32>,
    /// 本页的分页大小（= 配置 `display.periph_page_size`；**绑定线层注入**，缺省取
    /// `display-proto` 的 `DEFAULT_PERIPH_PAGE_SIZE`）。只用于「第 X / Y 页」的**分母**；
    /// 真源仍是响应里的 `page_size`（见 `refresh_drill`）。
    fire_page_size: Cell<u32>,
    /// 下钻视图是否打开。
    drill_open: Cell<bool>,
    /// A4 汇总（供断言与页内复用）。
    fire_summary: RefCell<FireSummary>,
    // ── 触发源卡 ──
    /// 卡对象（**拥有型**；高度随源数自适应）。
    source_card: Obj,
    /// 卡头文案（`触发源 · N`）。
    source_title: Label,
    /// 行容器（高度随源数 / 不可用态高自适应）。
    source_rows_box: Obj,
    /// 行池（见 **IL9**）。
    source_rows: Vec<SourceRow>,
    /// 触发源卡的**区块级「冻结 / 数据过期」角标**（口径同 [`Core::state_frozen`]）。
    source_frozen: StatusChip,
    /// 空态（`当前无联锁触发源`）。
    source_empty: Rc<EmptyState>,
    /// 不可用态（`UnavailableKind::Interlock`）。
    source_unavailable: Rc<UnavailableState>,
    // ── 状态三卡 ──
    /// 三卡（停机失败 / 故障灯 / 运行灯）。
    lamps: Vec<StatusCard>,
    // ── 固定操作条 ──
    release: Rc<TextButton>,
    restart: Rc<TextButton>,
    // ── 就地原因带 ──
    reason_left: Label,
    reason_right: Label,
    reason_styles: [Rc<Style>; 3],
    reason_left_style: Cell<usize>,
    reason_right_style: Cell<usize>,
    // ── 渲染期状态 ──
    /// 最近一帧的联锁段（缺帧 ⇒ 契约缺省 = **不可用**）。
    section: RefCell<InterlockSection>,
    /// 是否曾注入过**有效**帧（`available` 曾为 true）—— 仅作断言口径，不参与判据。
    ever_available: Cell<bool>,
    /// 提交中（按钮置灰；见 **IL15**）。
    submitting: Cell<bool>,
    /// 最近一次意图的种类（决定 `show_result` 成功路径的 Toast 文案）。
    last_op: Cell<OpKind>,
    /// 最近一次**结构化**拒绝（`HoldNotElapsed` 走倒计时槽，不入此槽）。
    last_reject: RefCell<Option<InterlockReject>>,
    /// 一次性就地文案覆盖（回执 `message` / 审计不可写这类**非结构化**文案）。
    plain_reason: RefCell<Option<(String, ReasonSlot)>>,
    /// EDGE-19 固定文案是否在显。
    conflict: Cell<bool>,
    // ── 保持时间倒计时（**IL14**）──
    countdown_on: Cell<bool>,
    countdown_remaining: Cell<u64>,
    countdown_base: Cell<Option<Instant>>,
    countdown_left: Cell<u64>,
    // ── 待外部刷新（EDGE-19 / 前置条件被拒，见 **IL12**）──
    refresh_requested: Cell<bool>,
    /// **延迟关闭弹层**的标志（`ConfirmDialog::close` 不得在 LVGL 事件回调内调用）。
    pending_close: Cell<bool>,
    /// 当前打开的确认弹层（同一时刻至多 1 个）。
    dialog: RefCell<Option<ConfirmDialog>>,
    /// 当前 Toast（同一时刻至多 1 条，UI §7.2）。
    toast: RefCell<Option<Toast>>,
    /// 「人工释放联锁」意图回调（**本页不生成 `request_id`**）。
    on_release: IntentSlot,
    /// 「M1 授权重启」意图回调。
    on_ack_m1: IntentSlot,
    /// 探测器明细分页意图回调（U-73；**本页不发请求**）。
    on_fire_page: PageReqSlot,
    /// 弹层打开**失败**的累计次数（**节流用**，见 **IL25**：不节流则每次点击都写 stderr）。
    open_fail_logs: Cell<u32>,
}

impl Core {
    // ── 渲染：帧 → 屏（**只改文本 / 颜色 / 可见性 / 尺寸**，不新建对象）────────

    /// 注入一帧的联锁段（`None` ⇒ 契约缺省 = **不可用**）。
    /// 区块级「冻结 / 数据过期」角标（EDGE-03 / EDGE-20；B3-2c）。
    ///
    /// **只切可见性 + 换文案**（对象在构造期就建好了）—— `render` 是 1 Hz 热路径，
    /// 在其中建 / 删对象会把 LVGL 定容池吃光（本仓既有先例）。
    ///
    /// **打标 ≠ 清值**：本函数一个字都不碰联锁段的数据（`apply_section` 照常把帧里的值
    /// 铺上屏）—— EDGE-03 明文要求"**保留**最近有效帧"，操作者正是据这份**冻结数值**决策。
    ///
    /// ⚠️ **两张卡的角标形态不同**（**IL29②**，产品裁定 2026-09-25）：总态卡取**图标-only**
    /// （488 宽卡内放不下 192 px 文字胶囊 ⇒ 与 latch 胶囊重叠），触发源卡保留**文字胶囊**。
    /// **两处都受本函数驱动**（判据 [`crate::ui::pages::frame_mark`] 单一真源）。
    fn apply_frame_mark(&self, mark: Option<&str>) {
        let visible = mark.is_some();
        if let Some(m) = mark {
            self.source_frozen.set_text(m);
            self.state_frozen.set_mark(m);
        }
        set_visible(self.source_frozen.obj(), visible);
        set_visible(self.state_frozen.obj(), visible);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // U-73：§A 消防四组 + 火警等级卡 + 下钻视图的渲染（**只改文本 / 颜色 / 可见性 / 位置**）
    // ═══════════════════════════════════════════════════════════════════════

    /// 注入一帧的外设段（`None` ⇒ 契约缺省 = **不可用** ⇒ 显「外设数据不可用」，EDGE-22）。
    fn apply_periph(&self, sec: &PeripheralsSection) {
        *self.periph.borrow_mut() = sec.clone();
        self.refresh_fire();
        self.layout_fire_groups();
    }

    /// 按当前的 `periph` / `catalog` 重画 §A 四组 + 火警等级卡（含下钻，若开着）。
    fn refresh_fire(&self) {
        let sec = self.periph.borrow();
        let cat_guard = self.catalog.borrow();
        let cat = cat_guard.as_ref();
        let station = station_state_of(cat, &sec, PeriphRole::Fire);

        // ── 火警等级卡（常驻总览带；**三重冗余**）──
        let level_view = periph_view(cat, &sec, PeriphRole::Fire, block_key("fire_sys"), 6);
        let (level_text, level_slot) = match &level_view {
            Some(v) if v.value.is_some() => {
                let t = state::fire_level_text(v.value.unwrap_or(f64::NAN), &v.enum_labels);
                let slot = fire_level_slot(v.value.unwrap_or(f64::NAN));
                (t, slot)
            }
            // 无值 / 无 catalog ⇒ 「未知」（**中性色，绝不用绿**，F21.2 / EX-10）
            _ => (ui_text::ENUM_UNKNOWN.to_string(), FIRE_SLOT_UNKNOWN),
        };
        self.fire_value.set_text(&level_text);
        self.fire_icon
            .set_text(fire_level_icon(&level_text, level_view.as_ref()));
        set_style_index(
            self.fire_value.obj(),
            &self.fire_value_styles,
            &self.fire_value_style,
            level_slot,
        );
        set_style_index(
            self.fire_icon.obj(),
            &self.fire_icon_styles,
            &self.fire_icon_style,
            level_slot,
        );
        set_style_index(
            &self.fire_bar_left,
            &self.fire_bar_styles,
            &self.fire_bar_style,
            level_slot,
        );

        // 段级 / 站级通告（**唯一真源**：`!available` 优先，其次站级态）
        let notice = if !sec.available {
            Some(ui_text::PERIPH_UNAVAILABLE)
        } else {
            station.section_text()
        };

        // ── A1 段顶通告行（设计 §15.6.2 ①；**不隐藏、不静默**，但**不占位行**）──
        //
        // ⚠️ **T21c-1-r1 订正**：此前通告落在位行第 0 行 ⇒ 本态下 bit0 的语义**不可判读**，
        // 且「逐位 16 行」的行数契约被破坏（评审 W-3②）。现通告独立成行（`A1_NOTICE_ROWS`）。
        match notice {
            Some(n) => {
                self.a1_notice.set_text(n);
                set_style_index(
                    self.a1_notice.obj(),
                    &self.a1_styles,
                    &self.a1_notice_style,
                    A1_STYLE_UNDEF,
                );
                set_visible(self.a1_notice.obj(), true);
            }
            None => set_visible(self.a1_notice.obj(), false),
        }

        // ── A1 消防系统状态（逐位 16 行；**本态下位行不受通告影响**）──
        let sys_view = periph_view(cat, &sec, PeriphRole::Fire, block_key("fire_sys"), 1);
        let sys_word = sys_view.as_ref().and_then(|v| v.value);
        let bits: Vec<BitMeta> = sys_view
            .as_ref()
            .map(|v| v.bits.clone())
            .unwrap_or_default();
        for i in 0..A1_ROWS {
            let (text, slot) = match bits.iter().find(|b| b.index as usize == i) {
                Some(b) => {
                    let active = sys_word
                        .map(|w| state::bit_active(w, b.index))
                        .unwrap_or(false);
                    (
                        format!(
                            "{} {}",
                            if active { ICON_FILLED } else { ICON_HOLLOW },
                            state::bit_text(
                                b.index,
                                b.defined,
                                &b.label,
                                active,
                                b.active_text.as_deref(),
                                b.inverted,
                            )
                        ),
                        if !b.defined {
                            A1_STYLE_UNDEF
                        } else if active {
                            A1_STYLE_ACTIVE
                        } else {
                            A1_STYLE_INACTIVE
                        },
                    )
                }
                // catalog 未取到 ⇒ **位名不可得**：显「名称未获取」（§15.3.1 的"中文名位"）。
                // catalog 在、但该位不在表内 ⇒ 才是「未定义位 n」（**不猜语义**，EX-09）。
                None => (
                    if cat.is_none() {
                        ui_text::NAME_UNKNOWN.to_string()
                    } else {
                        state::bit_text(i as u8, false, "", false, None, false)
                    },
                    A1_STYLE_UNDEF,
                ),
            };
            if let Some(l) = self.a1_rows.get(i) {
                l.set_text(&text);
                set_style_index(l.obj(), &self.a1_styles, &self.a1_row_style[i], slot);
            }
        }

        // ── A2 灭火瓶压力（EDGE-23：`Some(false)` ⇒「未配置」并**忽略 `v`**）──
        let mut cyl = periph_view(cat, &sec, PeriphRole::Fire, block_key("fire_sys"), 2);
        if let Some(v) = cyl.as_mut() {
            let configured = sec
                .stations
                .iter()
                .find(|s| s.role == PeriphRole::Fire)
                .and_then(|s| s.cylinder_configured);
            v.apply_cylinder(configured);
        }
        let (a2_text, a2_slot) = a2_display(cyl.as_ref(), notice, !sec.available);
        self.a2_value.set_text(&a2_text);
        set_style_index(
            self.a2_value.obj(),
            &self.a2_styles,
            &self.a2_style,
            a2_slot,
        );

        // ── A3 探测器触发（3 字 × 各 3 位：bit0 干接点触发 / bit1 复合触发 / bit2 预留）──
        for (i, at) in [3u16, 4, 5].iter().enumerate() {
            let view = periph_view(cat, &sec, PeriphRole::Fire, block_key("fire_sys"), *at);
            let text = match notice {
                Some(n) if i == 0 => n.to_string(),
                _ => {
                    let label = view
                        .as_ref()
                        .map(|v| v.label_text().to_string())
                        .unwrap_or_else(|| ui_text::NAME_UNKNOWN.to_string());
                    let word = view.as_ref().and_then(|v| v.value);
                    let segs = FIRE_TRIGGER_BITS
                        .iter()
                        .map(|(idx, name)| {
                            // bit2 是**预留**：只显「预留」，**不显 0/1 语义**（§15.4 A3 组）
                            if *idx == 2 {
                                name.to_string()
                            } else {
                                let active =
                                    word.map(|w| state::bit_active(w, *idx)).unwrap_or(false);
                                format!(
                                    "{} {}",
                                    name,
                                    if active {
                                        ui_text::BIT_ACTIVE
                                    } else {
                                        ui_text::BIT_INACTIVE
                                    }
                                )
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" · ");
                    format!("{label} {segs}")
                }
            };
            if let Some(l) = self.a3_rows.get(i) {
                l.set_text(&text);
            }
        }

        // ── A4 探测器汇总（**§15.4 逐字口径**：`登记 N 只 / 可读 M 只 / 报警 x / 故障 y / 离线 z`）──
        //
        // ⚠️ **分隔符 = ` / `（T21c-1-r1 订正）**：本节曾以**单个空格**分隔五元组，与 §15.4
        // 原文（含 `/`）不符 ⇒ 现按合同补回（`/` 与空格均在生成字体 cmap 内、992 px 槽放得下，
        // 无需等价替换）。`registered` 取不到时退 `--`（**不臆造**，同 §8.2 占位符口径）。
        let summary = fire_summary(cat, &sec);
        *self.fire_summary.borrow_mut() = summary;
        let a4_text = if !sec.available {
            ui_text::PERIPH_UNAVAILABLE.to_string()
        } else if let Some(n) = station.section_text() {
            n.to_string()
        } else {
            let registered = summary
                .registered
                .map(|n| n.to_string())
                .unwrap_or_else(|| crate::ui::pages::PLACEHOLDER.to_string());
            // 「`<标签> <数> 只`」两段共用（量词只在这两段出现，见 §15.4）
            let with_unit = |l: &str, n: &str| format!("{l} {n} {}", ui_text::COUNT_UNIT);
            [
                with_unit(ui_text::REGISTERED, &registered),
                with_unit(ui_text::READABLE, &summary.readable.to_string()),
                format!("{} {}", ui_text::COUNT_ALARM, summary.alarm),
                format!("{} {}", ui_text::COUNT_FAULT, summary.fault),
                format!("{} {}", ui_text::OFFLINE, summary.offline),
            ]
            .join(" / ")
        };
        self.a4_summary.set_text(&a4_text);
        let mismatch_visible = sec.available && summary.mismatch();
        self.a4_mismatch
            .set_text(ui_text::REGISTERED_READABLE_MISMATCH);
        set_visible(self.a4_mismatch.obj(), mismatch_visible);

        // 下钻（若开着）随新数据重画
        if self.drill_open.get() {
            self.refresh_drill();
        }
    }

    /// §A 四组卡片的 y（**随 §B 触发源卡实测高联动** —— 源数 3/4 时源卡会变高，
    /// 固定 const 会与灯卡重叠）。构造期与每帧各调一次（只 `set_pos`，不建对象）。
    fn layout_fire_groups(&self) {
        let s = self.section.borrow();
        let body_h = source_body_h(&s);
        let lamps_y = SOURCE_CARD_Y + (body_h + CARD_HEAD_H + 2 * CARD_INSET) + Dimens::GAP_SECTION;
        let a1_y = lamps_y + LAMP_CARD_H + Dimens::GAP_GROUP;
        let a2_y = a1_y + A1_CARD_H + Dimens::GAP_GROUP;
        let a3_y = a2_y + A2_CARD_H + Dimens::GAP_GROUP;
        let a4_y = a3_y + A3_CARD_H + Dimens::GAP_GROUP;
        self.a1_card.set_pos(0, a1_y);
        self.a2_card.set_pos(0, a2_y);
        self.a3_card.set_pos(0, a3_y);
        self.a4_card.set_pos(0, a4_y);
    }

    /// 下钻视图：显隐 + 标题 / 页码 / 行内容（**只改文本 / 颜色 / 可见性**）。
    fn refresh_drill(&self) {
        let page_guard = self.fire_page.borrow();
        let page = page_guard.as_ref();
        let req = self.fire_page_req.get();
        let failed = self.fire_page_failed.get();

        // 失败 / 源不可用 ⇒ 行区让位给「明细不可用 + 重试」（R-4：**只显本地固定文案**，
        // 服务端 400/503 的原因串**只进日志**、不上屏 —— §15.6.2 ⑥）。
        let source_unavailable = page.is_some_and(|p| !p.available);
        let show_fail = failed || source_unavailable;
        let total = page.and_then(|p| p.total).map(u32::from);
        let expanded = page.map(|p| u32::from(p.expanded)).unwrap_or(0);
        // 页大小取**响应里的 `page_size`**（真源 = 服务端实际切分），未取到时用本地注入值
        // （`periph_page_size` 接线）；**不得**拿"当前页码"当页大小（那是两件事）。
        let size = page
            .map(|p| p.page_size)
            .filter(|s| *s > 0)
            .unwrap_or_else(|| self.fire_page_size.get().max(1));
        let pages = crate::console::page_count(total.unwrap_or(expanded), size);
        let cur = page.map(|p| p.page).unwrap_or(req);
        self.drill_title.set_text(&format!(
            "{} · {} {cur} / {pages} {}",
            group_title("fire_detector"),
            ui_text::PAGE_PREFIX,
            ui_text::PAGE_SUFFIX,
        ));
        self.drill_page_text
            .set_text(&format!("{cur} / {pages} {}", ui_text::PAGE_SUFFIX));
        let has_more = page.is_some_and(|p| p.has_more);
        self.drill_prev.set_disabled(cur <= 1);
        self.drill_next.set_disabled(!has_more);
        self.drill_fail.set_text(if source_unavailable {
            ui_text::FIRE_SOURCE_UNAVAILABLE
        } else {
            ui_text::DETAIL_UNAVAILABLE
        });
        set_visible(self.drill_fail.obj(), show_fail);
        set_visible(self.drill_retry.button().obj(), failed);
        self.drill_rows_box.set_hidden(show_fail);

        // 「数据 1」的拆解规格（catalog 优先；缺则用 `display-proto` 的锁定模板 —— 单一真源）
        let specs = data1_specs(self.catalog.borrow().as_ref());
        let items = page.map(|p| p.items.as_slice()).unwrap_or(&[]);
        for (r, row) in self.drill_rows.iter().enumerate() {
            match items.get(r) {
                Some(it) => {
                    row.set_visible(true);
                    row.set_cell(
                        0,
                        &format!("{}", it.index),
                        &self.drill_cell_styles,
                        DRILL_SLOT_VALUE,
                    );
                    row.set_num(1, &it.addr, 0, &self.drill_cell_styles);
                    // 状态列 = **2 个已定义位**（bit12 报警总状态 / bit14 故障总状态）：
                    // 圆点（`●`/`○`）+ 「活跃 / 非活跃」文字**双通道**；其余 14 位不展开。
                    for (k, (idx, _)) in FIRE_DETECTOR_STATE_BITS.iter().enumerate() {
                        let active = it
                            .state
                            .v
                            .map(|w| state::bit_active(w, *idx))
                            .unwrap_or(false);
                        row.set_cell(
                            2 + k,
                            &format!(
                                "{} {}",
                                if active { ICON_FILLED } else { ICON_HOLLOW },
                                if active {
                                    ui_text::BIT_ACTIVE
                                } else {
                                    ui_text::BIT_INACTIVE
                                }
                            ),
                            &self.drill_cell_styles,
                            if active {
                                DRILL_SLOT_ACTIVE
                            } else {
                                DRILL_SLOT_INACTIVE
                            },
                        );
                    }
                    // 烟雾 / 温度：**只在此处拆解**（F21.5 / EX-13；帧内 `v` 与 `latest_values`
                    // 仍是整字 —— 本处只读 `data1.v`，不写回）
                    let raw = it.data1.v;
                    let smoke = specs
                        .first()
                        .filter(|_| raw.is_some())
                        .map(|d| state::decompose_value(d.from, raw.unwrap_or(f64::NAN)));
                    let temp = specs
                        .get(1)
                        .filter(|_| raw.is_some())
                        .map(|d| state::decompose_value(d.from, raw.unwrap_or(f64::NAN)));
                    row.set_num_opt(4, smoke, 1, &self.drill_cell_styles);
                    row.set_num_opt(5, temp, 0, &self.drill_cell_styles);
                    row.set_num_opt(6, it.co.v, 0, &self.drill_cell_styles);
                    row.set_num_opt(7, it.voc.v, 0, &self.drill_cell_styles);
                    row.set_num_opt(8, it.h2.v, 0, &self.drill_cell_styles);
                }
                None => row.set_visible(false),
            }
        }
    }

    /// 开 / 关下钻视图（**不改 `current_page`**、不经导航 —— §15.5.3 / T-20）。
    fn set_drill_open(&self, on: bool) {
        self.drill_open.set(on);
        set_visible(&self.drill, on);
        // 进入下钻：§B/§A 仍在滚动区里（下层），把整个滚动区隐掉以免穿透触摸
        set_visible(&self.scroll, !on);
        if on {
            self.refresh_drill();
        }
    }

    /// 请求某一页探测器明细（**本页不发请求** —— 经意图回调交回接线层，与 P2/P4 同口径）。
    fn request_fire_page(&self, page: u32) {
        let p = page.max(1);
        self.fire_page_req.set(p);
        self.fire_page_failed.set(false);
        if let Ok(mut slot) = self.on_fire_page.try_borrow_mut() {
            if let Some(f) = slot.as_mut() {
                f(p);
            }
        }
        self.refresh_drill();
    }

    fn apply_section(&self, s: &InterlockSection) {
        // ⓪ **陈旧倒计时清理（M3 / IL27）**：新帧**改变了展示相关字段** ⇒ 之前那条
        // 「保持时间不足 · 还需 N 秒」的基准已失效（源已复位 / latch 已变），必须清掉 ——
        // 否则左槽会**跨帧常驻**旧倒计时，且它的优先级高于 `last_reject`（会顶掉新原因）。
        //
        // ⚠️ 判据**忽略 `ts_ms`**：它每帧都变（帧内时间戳），若纳入比较则 1 Hz 心跳**每秒**
        // 都会清掉倒计时，IL14 的倒计时将形同虚设。**内容相同的帧（含仅 `ts_ms` 变）不清**。
        let changed = !section_display_eq(&self.section.borrow(), s);
        *self.section.borrow_mut() = s.clone();
        if changed {
            self.clear_countdown();
        }
        if s.available {
            self.ever_available.set(true);
        }

        // ① 联锁总态（三态互斥：**`!available` 优先**，绝不回落「未联锁」）。
        let view = state_view(s);
        let idx = state_index(view);
        self.state_text_l.set_text(state_text(view));
        self.state_icon.set_text(state_icon(view));
        set_style_index(
            self.state_text_l.obj(),
            &self.state_text_styles,
            &self.state_text_style,
            idx,
        );
        set_style_index(
            self.state_icon.obj(),
            &self.state_icon_styles,
            &self.state_icon_style,
            idx,
        );
        set_style_index(
            &self.state_bar_top,
            &self.state_styles,
            &self.state_bar_top_style,
            idx,
        );
        set_style_index(
            &self.state_bar_left,
            &self.state_styles,
            &self.state_bar_left_style,
            idx,
        );
        let chip_objs: Vec<&Obj> = self.state_chips.iter().map(|c| c.obj()).collect();
        show_only(&chip_objs, Some(idx));

        // ② 触发源卡（行池 + 空态 + 不可用态，三者互斥；卡高随源数自适应）。
        let shown_rows = if s.available {
            s.sources.len().min(SOURCE_ROW_POOL)
        } else {
            0
        };
        // 卡头 = `触发源 · N`；**超出**行池上限时**可见地**补差额（IL9 的补偿，② ——
        // `available = false` 时不铺行，也就无「超出」可言）。**不改行池上限**（版式取向）。
        let title = if s.available {
            format!(
                "{}{}",
                sources_title(s.sources.len()),
                sources_overflow_note(s.sources.len())
            )
        } else {
            sources_title(s.sources.len())
        };
        self.source_title.set_text(&title);
        for (i, row) in self.source_rows.iter().enumerate() {
            match s.sources.get(i).filter(|_| i < shown_rows) {
                Some(item) => {
                    row.apply(&item.name, item.tripped);
                    set_visible(&row.obj, true);
                }
                None => row.hide(),
            }
        }
        // 卡体高：可用时 = max(空态高, 行数 × 行高)；不可用时 = 不可用态高
        //（两个高度都按**组件构件算式**推导，并由 `ui/tests.rs` 与组件实测高对齐 —— 见 [`EMPTY_H`]）。
        // ⚠️ 算式**收口在 [`source_body_h`]**（U-73）：`Core::layout_fire_groups` 要用**同一个**
        // 算式把 §A 四组接在灯卡之下 —— 两处各写一份就会在 3/4 源时错位/重叠。
        let body_h = source_body_h(s);
        self.source_rows_box.set_size(INNER_W, body_h);
        self.source_card
            .set_size(Dimens::CONTENT_W, body_h + CARD_HEAD_H + 2 * CARD_INSET);
        set_visible(self.source_empty.obj(), s.available && shown_rows == 0);
        set_visible(self.source_unavailable.obj(), !s.available);
        self.source_empty
            .set_pos(-CARD_INSET, theme::center_offset(body_h, EMPTY_H));
        self.source_unavailable
            .set_pos(-CARD_INSET, theme::center_offset(body_h, UNAVAILABLE_H));

        // ③ 状态三卡（停机失败 / 故障灯 / 运行灯）—— `!available` ⇒ 三卡**同步转灰**（IL7）。
        let lamps_y = SOURCE_CARD_Y
            + (body_h + CARD_HEAD_H + 2 * CARD_INSET)
            + Dimens::GAP_SECTION;
        for (i, card) in self.lamps.iter().enumerate() {
            card.set_pos(lamp_x(i as i32), lamps_y);
        }
        if !s.available {
            // 三通道**同源**取 `UnavailableKind::Interlock`（图标 / 文案 / 色），见 **IL7**。
            let kind = UnavailableKind::Interlock;
            let text = format!("{} {}", kind.icon(), kind.title());
            for card in &self.lamps {
                card.set(&text, 2);
            }
        } else {
            let (t, slot) = stop_view(s.stop_failed);
            self.lamps[0].set(t, slot);
            let (t, slot) = lamp_view(tri_view(s.fault_lamp));
            self.lamps[1].set(t, slot);
            let (t, slot) = lamp_view(tri_view(s.run_lamp));
            self.lamps[2].set(t, slot);
        }

        // ④ 操作按钮 + 就地原因带。
        self.refresh_actions();

        // ⑤ §A 四组随 §B 实测高联动（源数 3/4 时源卡变高，**固定 const 会与灯卡重叠**）。
        self.layout_fire_groups();
    }

    /// 刷新两个操作按钮的可用性 + 就地原因带。
    fn refresh_actions(&self) {
        let st = op_state(&self.section.borrow(), self.submitting.get());
        self.release.set_disabled(st.release_disabled);
        self.restart.set_disabled(st.restart_disabled);

        // 右槽先算：**M1 专属**阻塞（全局阻塞时不重复显示；`op_state` 已把全局原因并入其取值）。
        let right = if self.section.borrow().available && self.section.borrow().enabled {
            st.restart_reason.map(str::to_string)
        } else {
            None
        };
        // **左槽宽度随右槽是否在显自适应**（**IL23** ③）：右槽空着时左槽独占整条原因带
        // （992 px），长 `message` 因此**单行不截断**；右槽在显时收回 352 px（两槽不重叠，
        // IL5）。只改 `set_size`，**不新建 / 不删除对象**（渲染期纪律）。
        self.reason_left.set_size(
            if right.is_some() {
                REASON_LEFT_W
            } else {
                REASON_LEFT_FULL_W
            },
            TextSlot::Body.px() as i32,
        );

        // 左槽优先级：全局（不可用 / 未启用）> 一次性文案 > 倒计时 > EDGE-19 > 结构化拒绝。
        let left: Option<(String, ReasonSlot)> = if !self.section.borrow().available {
            Some((TEXT_STATE_UNAVAILABLE.to_string(), ReasonSlot::Gray))
        } else if !self.section.borrow().enabled {
            Some((TEXT_NOT_ENABLED.to_string(), ReasonSlot::Amber))
        } else if let Some((t, slot)) = self.plain_reason.borrow().as_ref() {
            Some((t.clone(), *slot))
        } else if self.countdown_on.get() {
            Some((hold_text(self.countdown_left.get()), ReasonSlot::Amber))
        } else if self.conflict.get() {
            Some((TEXT_CONFLICT.to_string(), ReasonSlot::Red))
        } else {
            self.last_reject
                .borrow()
                .as_ref()
                .map(|r| (reject_text(r), ReasonSlot::Red))
        };

        match left {
            Some((t, slot)) => {
                self.reason_left.set_text(&t);
                set_style_index(
                    self.reason_left.obj(),
                    &self.reason_styles,
                    &self.reason_left_style,
                    slot.index(),
                );
                set_visible(self.reason_left.obj(), true);
            }
            None => set_visible(self.reason_left.obj(), false),
        }

        match right {
            Some(t) => {
                self.reason_right.set_text(&t);
                set_style_index(
                    self.reason_right.obj(),
                    &self.reason_styles,
                    &self.reason_right_style,
                    ReasonSlot::Amber.index(),
                );
                set_visible(self.reason_right.obj(), true);
            }
            None => set_visible(self.reason_right.obj(), false),
        }
    }

    /// 回执 → 展示态（**单一映射点**，见 **IL13**）。
    fn apply_ack(&self, ack: &InterlockOpAck) {
        let mut s = self.section.borrow_mut();
        s.latched = ack.latched;
        s.stop_failed = !ack.stopped;
    }

    /// 按当前 [`Core::section`] **重画**整段（回执改过字段后调用 —— 否则屏上仍停在旧态：
    /// 本页的"改屏"只发生在 [`Core::apply_section`] 内，`apply_ack` 只改**数据**）。
    fn rerender(&self) {
        let s = self.section.borrow().clone();
        self.apply_section(&s);
    }

    /// 清空「最近一次拒绝 / 冲突 / 倒计时 / 一次性文案」（发起新意图或成功时调用）。
    fn clear_reason(&self) {
        *self.last_reject.borrow_mut() = None;
        *self.plain_reason.borrow_mut() = None;
        self.conflict.set(false);
        self.clear_countdown();
        self.refresh_actions();
    }

    /// 只清**倒计时**槽（**IL14 / IL27**：新帧改变了展示态 ⇒ 旧基准失效）。
    fn clear_countdown(&self) {
        self.countdown_on.set(false);
        self.countdown_base.set(None);
        self.countdown_left.set(0);
    }

    /// 弹层打开失败的诊断：**节流**（**IL25**）—— 第 1 次必写，其后每
    /// [`OPEN_FAIL_LOG_EVERY`] 次写一行（不节流则每次点击都往 stderr 灌一行）。
    fn note_open_failure(&self, op: &str, e: &LvglError) {
        let n = self.open_fail_logs.get().saturating_add(1);
        self.open_fail_logs.set(n);
        if n == 1 || n % OPEN_FAIL_LOG_EVERY == 0 {
            report_open_failure(op, e);
        }
    }

    /// 置一次性就地文案（回执 `message` / 审计不可写这类非结构化原因）。
    fn set_plain_reason(&self, text: &str, slot: ReasonSlot) {
        *self.plain_reason.borrow_mut() = Some((text.to_string(), slot));
        self.refresh_actions();
    }

    /// 关闭弹层（**只可在 LVGL 事件回调之外调用**）。
    fn close_dialog(&self) {
        if let Some(d) = self.dialog.borrow_mut().take() {
            d.close();
        }
    }

    /// 关闭 Toast（同上）。
    fn close_toast(&self) {
        if let Some(t) = self.toast.borrow_mut().take() {
            t.close();
        }
    }

    /// 弹一条 Toast（同一时刻仅 1 条：新的覆盖旧的，UI §7.2）。
    fn show_toast(&self, tone: ToastTone, icon: &str, text: &str) -> Result<(), LvglError> {
        self.close_toast();
        let layer = widgets::layer_top()?;
        let t = Toast::new(&layer, tone, icon, text)?;
        *self.toast.borrow_mut() = Some(t);
        Ok(())
    }

    /// 触发「人工释放联锁」意图回调（**不发起任何请求**；`request_id` 由外部生成）。
    fn fire_release(&self) {
        let payload = op_payload(&self.section.borrow());
        self.last_op.set(OpKind::Release);
        self.clear_reason();
        if let Ok(mut slot) = self.on_release.try_borrow_mut() {
            if let Some(f) = slot.as_mut() {
                f(payload);
            }
        }
    }

    /// 触发「M1 授权重启」意图回调（语义同 [`Core::fire_release`]）。
    fn fire_ack_m1(&self) {
        let payload = op_payload(&self.section.borrow());
        self.last_op.set(OpKind::AckM1);
        self.clear_reason();
        if let Ok(mut slot) = self.on_ack_m1.try_borrow_mut() {
            if let Some(f) = slot.as_mut() {
                f(payload);
            }
        }
    }
}

/// 开确认弹层（**唯一的弹层构造点**；确认完成前不发任何请求 —— 设计 §6.4 / UI §6.4）。
///
/// 分级恒为 [`ConfirmLevel::L2`]（UI §2.5：联锁释放 / M1 授权 = **L2 破坏性 / 生效性写** ⇒
/// 危险色弹层 + 「按住确认」长按 1.0 s + 必须出现「影响范围」段；**不是** L2+ —— 本页不涉及
/// 瞬断链路，故不强制 `WarnBanner`）。
fn open_dialog(core: &Rc<Core>, op: OpKind) -> Result<(), LvglError> {
    // 同一时刻至多 1 个弹层；该操作被本地阻塞时不弹（按钮已 `disabled`，此处是**不依赖 GUI
    // 禁用判定**的第二道保险）。
    if core.dialog.borrow().is_some() {
        return Ok(());
    }
    let st = op_state(&core.section.borrow(), core.submitting.get());
    let blocked = match op {
        OpKind::Release => st.release_disabled,
        OpKind::AckM1 => st.restart_disabled,
    };
    if blocked {
        return Ok(());
    }

    let rows = dialog_details(&core.section.borrow(), op);
    let details: Vec<ConfirmDetail> = rows
        .iter()
        .map(|d| ConfirmDetail {
            field: d.field,
            before: &d.before,
            after: &d.after,
        })
        .collect();
    let spec = ConfirmSpec {
        title: op.dialog_title(),
        impact: op.impact(),
        details: &details,
        // 本页无「瞬断链路」字段 ⇒ L2 不强制 `WarnBanner`，`warn_fields` 恒空。
        warn_fields: &[],
    };

    let layer = widgets::layer_top()?;
    let dialog = ConfirmDialog::new(&layer, &spec, ConfirmLevel::L2)?;

    // 确认完成（L2：长按满 1.0 s）⇒ **只报意图**。
    {
        let w = Rc::downgrade(core);
        dialog.set_on_confirm(move || {
            let Some(c) = w.upgrade() else { return };
            match op {
                OpKind::Release => c.fire_release(),
                OpKind::AckM1 => c.fire_ack_m1(),
            }
        });
    }
    // 取消 ⇒ 只置"待关闭"标志（**不得**在事件回调里删弹层）。
    {
        let w = Rc::downgrade(core);
        dialog.set_on_cancel(move || {
            if let Some(c) = w.upgrade() {
                c.pending_close.set(true);
            }
        });
    }

    *core.dialog.borrow_mut() = Some(dialog);
    Ok(())
}

/// 弹层打开失败的 stderr 诊断**节流窗口**（**IL25**：第 1 次 + 其后每 N 次各写一行）。
const OPEN_FAIL_LOG_EVERY: u32 = 64;

/// 开弹层失败只写 stderr（**绝不 panic**、**绝不上屏** —— 上屏文案必须逐字在 cmap 内）。
///
/// **调用点已节流**（[`Core::note_open_failure`]，**IL25**）：本函数自身不做判断。
fn report_open_failure(op: &str, e: &LvglError) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "P4 安全联锁页：打开确认弹层失败（{op}）：{e}");
}

/// 源行 `LedIndicator` 重建失败（上屏名变化时）—— 同样只写 stderr，**保留旧件**。
fn report_led_rebuild_failure(e: &LvglError) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "P4 安全联锁页：触发源行重建失败：{e}");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. 页面
// ═══════════════════════════════════════════════════════════════════════════

/// P4 安全 / 联锁页（**帧驱动展示 + 控制通道意图**；见模块文档）。
///
/// # 意图如何交给外部
///
/// 本页**不发请求、不生成 `request_id`**：用户点「人工释放联锁」/「M1 授权重启」只**开确认弹层**；
/// 弹层确认完成（L2：长按满 1.0 s）经 [`P4InterlockPage::set_on_release`] /
/// [`P4InterlockPage::set_on_ack_m1`] 注册的回调交出一份 [`InterlockOpPayload`]（UI **观测到**的
/// `observed_latched` + `observed_sources`，**机器名**，供后端做乐观并发检查，EDGE-19）；
/// 外部（B3 `console.rs`）据此生成 `request_id`(uuid) + `issued_at_ms` 后 POST 对应端点，
/// 并把回执经 [`P4InterlockPage::show_result`] 灌回本页。
pub struct P4InterlockPage {
    core: Rc<Core>,
}

impl P4InterlockPage {
    /// 在 `parent` 下建页（页根 `992 × 624`；**摆放由调用方负责**，与 P1 / P2 / P6 同口径）。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = layout_box(parent, Dimens::CONTENT_W, Dimens::CONTENT_H)?;

        // 滚动视口（页根自建 —— `pages::page_root()` 的"页根即滚动容器"语义留给 P1 / P6）。
        // **U-73 起下移到总览带之下**（§15.6.2 的版面：总览带常驻 + 滚动区）。
        let scroll = ScrollContainer::create(&root)?;
        scroll.set_size(Dimens::CONTENT_W, VIEWPORT_H);
        scroll.set_pos(0, VIEWPORT_Y);
        scroll.add_style(
            &theme::transparent(),
            crate::lvgl::style::StyleSelector::main(),
        );

        // ── ⓪ 【安全总览带】（**常驻不滚动**；UI §6.4.1）──
        let band = layout_box(&root, Dimens::CONTENT_W, BAND_H)?;
        band.set_pos(0, BAND_Y);

        // ── ① 联锁总态卡（**既有内容 / 文案 / 字号不变**，仅几何收窄，见 **IL29**）──
        let state_card = decor(&band, Dimens::BAND_CARD_W, BAND_CARD_H, &theme::card())?;
        state_card.set_pos(0, 0);
        // 顶部 3 px 横条 + 左缘 6 px 竖条（**贴卡外缘** ⇒ 用负偏移越过内边距，见常量块注）。
        let state_bar_top = decor(
            &state_card,
            Dimens::BAND_CARD_W,
            STATE_BAR_TOP_H,
            &theme::card_head_bar(Palette::STOPPED),
        )?;
        state_bar_top.set_pos(-CARD_INSET, -CARD_INSET);
        let state_bar_left = decor(
            &state_card,
            STATE_BAR_LEFT_W,
            BAND_CARD_H,
            &theme::card_head_bar(Palette::STOPPED),
        )?;
        state_bar_left.set_pos(-CARD_INSET, -CARD_INSET);
        let state_title = text_label(
            &state_card,
            TEXT_CARD_STATE,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        state_title.set_pos(0, BAND_HEAD_TEXT_Y);
        // 区块级「冻结 / 数据过期」角标（EDGE-03 / EDGE-20）：**图标-only 形态**（`IL29②`）。
        // 与触发源卡那张（192 px 文字胶囊）**同在 `ui/pages/mod.rs` 的共享构造点族**里；
        // 形态不同是因为本卡 488 px 宽 —— 标题 112 + 192 + latch 胶囊 236 = 540 > 454。
        let state_frozen =
            crate::ui::pages::frozen_icon_badge(&state_card, STATE_FROZEN_X, STATE_FROZEN_Y)?;
        // 三态胶囊：**三支各预建一次**（皮肤在构造期固定），按 [`STATE_VIEW_ORDER`] 的槽位序摆放；
        // 文案 / 皮肤**逐个取自** [`latch_chip`]（单一真源），图标取自 [`latch_chip_icon`]。
        let chip_l = latch_chip(STATE_VIEW_ORDER[0]);
        let chip_u = latch_chip(STATE_VIEW_ORDER[1]);
        let chip_n = latch_chip(STATE_VIEW_ORDER[2]);
        let state_chips = [
            Rc::new(StatusChip::new(
                &state_card,
                LATCH_CHIP_W,
                latch_chip_icon(STATE_VIEW_ORDER[0]),
                chip_l.1,
                chip_l.2,
            )?),
            Rc::new(StatusChip::new(
                &state_card,
                LATCH_CHIP_W,
                latch_chip_icon(STATE_VIEW_ORDER[1]),
                chip_u.1,
                chip_u.2,
            )?),
            Rc::new(StatusChip::new(
                &state_card,
                LATCH_CHIP_W,
                latch_chip_icon(STATE_VIEW_ORDER[2]),
                chip_n.1,
                chip_n.2,
            )?),
        ];
        for c in &state_chips {
            c.set_pos(LATCH_CHIP_X, LATCH_CHIP_Y);
        }
        let state_icon = Rc::new(text_label(
            &state_card,
            ICON_STATE_UNAVAILABLE,
            theme::icon_slot(Dimens::ICON_XL),
            Palette::STOPPED,
        )?);
        state_icon.set_size(Dimens::ICON_XL, Dimens::ICON_XL);
        state_icon.set_pos(0, STATE_ICON_Y);
        let state_text_l = Rc::new(text_label(
            &state_card,
            TEXT_STATE_UNAVAILABLE,
            TextSlot::InterlockState,
            Palette::STOPPED,
        )?);
        state_text_l.set_size(STATE_TEXT_W, TextSlot::InterlockState.px() as i32);
        state_text_l.set_long_mode(LongMode::DOTS);
        state_text_l.set_pos(STATE_TEXT_X, BAND_HEAD_H);

        // ── ①′ 火警等级卡（F21 新增；总览带右卡，常驻）──
        //
        // **三重冗余**（F14 / §15.4）：文案（64 px 枚举）+ 语义色（卡左缘竖条 + 图标 + 文字三处
        // 同色）+ 图标。表外值 ⇒ 「未知」**中性色、绝不用绿色**（F21.2 / EX-10）。
        let fire_card = decor(&band, Dimens::BAND_CARD_W, BAND_CARD_H, &theme::card())?;
        fire_card.set_pos(FIRE_CARD_X, 0);
        let fire_bar_left = decor(
            &fire_card,
            STATE_BAR_LEFT_W,
            BAND_CARD_H,
            &theme::card_head_bar(Palette::STOPPED),
        )?;
        fire_bar_left.set_pos(-CARD_INSET, -CARD_INSET);
        let fire_title = text_label(
            &fire_card,
            group_title("fire_level"),
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        fire_title.set_pos(0, BAND_HEAD_TEXT_Y);
        let fire_icon = Rc::new(text_label(
            &fire_card,
            ICON_STATE_UNAVAILABLE,
            theme::icon_slot(FIRE_ICON_W),
            Palette::STOPPED,
        )?);
        fire_icon.set_size(FIRE_ICON_W, FIRE_ICON_W);
        fire_icon.set_pos(0, FIRE_ICON_Y);
        let fire_value = Rc::new(text_label(
            &fire_card,
            ui_text::ENUM_UNKNOWN,
            TextSlot::PhasePower,
            Palette::TEXT_WEAK,
        )?);
        fire_value.set_size(FIRE_VALUE_W, TextSlot::PhasePower.px() as i32);
        fire_value.set_long_mode(LongMode::DOTS);
        fire_value.set_pos(FIRE_VALUE_X, FIRE_VALUE_Y);

        // ── ② 触发源卡 ──
        let source_card = decor(
            &scroll,
            Dimens::CONTENT_W,
            SOURCE_BODY_MIN_H + CARD_HEAD_H + 2 * CARD_INSET,
            &theme::card(),
        )?;
        source_card.set_pos(0, SOURCE_CARD_Y);
        let source_title = text_label(
            &source_card,
            &sources_title(0),
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        source_title.set_pos(0, CARD_HEAD_TEXT_Y);
        // 同「总态卡角标」：**同一构造点**（[`crate::ui::pages::frozen_chip`]），只有 x 不同。
        let source_frozen = crate::ui::pages::frozen_chip(&source_card, SOURCE_FROZEN_X)?;
        let source_rows_box = layout_box(&source_card, INNER_W, SOURCE_BODY_MIN_H)?;
        source_rows_box.set_pos(0, CARD_HEAD_H);
        let mut source_rows = Vec::with_capacity(SOURCE_ROW_POOL);
        for i in 0..SOURCE_ROW_POOL {
            source_rows.push(SourceRow::new(&source_rows_box, i)?);
        }
        let source_empty = Rc::new(EmptyState::new(
            &source_rows_box,
            ICON_HOLLOW,
            TEXT_SOURCES_EMPTY,
        )?);
        let source_unavailable = Rc::new(UnavailableState::new(
            &source_rows_box,
            UnavailableKind::Interlock,
            // 帧内 `available = false` **不带原因** ⇒ 空串（**不臆造**原因文案，见 **IL8**）。
            "",
        )?);

        // ── ③ 状态三卡（停机失败 / 故障灯 / 运行灯）──
        let lamp_stop = StatusCard::new(
            &scroll,
            lamp_x(0),
            TEXT_CARD_STOP,
            [Palette::OK, Palette::DANGER, Palette::STOPPED],
        )?;
        let lamp_fault = StatusCard::new(
            &scroll,
            lamp_x(1),
            TEXT_CARD_FAULT_LAMP,
            [lamp_on_color(true), lamp_off_color(), gray_color()],
        )?;
        let lamp_run = StatusCard::new(
            &scroll,
            lamp_x(2),
            TEXT_CARD_RUN_LAMP,
            [lamp_on_color(false), lamp_off_color(), gray_color()],
        )?;
        let lamps = vec![lamp_stop, lamp_fault, lamp_run];

        // ── ③′ §A 消防四组（**只读**；F21 / §15.4；组间 `GAP_GROUP`）──────────────
        //
        // 逐组的对象在**构造期一次建齐** ⇒ `render`（1 Hz 热路径）只改文本 / 颜色 / 可见性 /
        // 位置（本页的渲染期纪律，见模块头）。四组自上而下：A1 系统状态 → A2 灭火瓶压力 →
        // A3 探测器触发 → A4 探测器（汇总 + 下钻入口）。
        let a1_card = decor(&scroll, Dimens::CONTENT_W, A1_CARD_H, &theme::card())?;
        a1_card.set_pos(0, A1_CARD_Y);
        let a1_title = text_label(
            &a1_card,
            group_title("fire_sys_status"),
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        a1_title.set_pos(0, CARD_HEAD_TEXT_Y);
        // A1 **段顶通告**行（设计 §15.6.2 ① 的口径）：**独立于位行** ⇒ 段级 / 站级降级时
        // bit0 的语义仍可判读，且「逐位 16 行」的行数契约不被破坏（T21c-1-r1 订正）。
        // 建好即隐藏（常态留白 ⇒ 位行 y 不随降级态跳动）。
        let a1_notice = label(&a1_card, TextSlot::Body, Palette::TEXT_SECOND)?;
        a1_notice.set_size(INNER_W, A_ROW_H);
        a1_notice.set_long_mode(LongMode::DOTS);
        a1_notice.set_pos(0, A1_NOTICE_Y);
        set_visible(a1_notice.obj(), false);
        // A1 逐位 16 行（**行池恰 16**：整字位图恒 16 位 ⇒ 行数不随数据变，F25）。
        let mut a1_rows = Vec::with_capacity(A1_ROWS);
        let mut a1_row_style = Vec::with_capacity(A1_ROWS);
        for i in 0..A1_ROWS {
            let l = label(&a1_card, TextSlot::Body, Palette::TEXT_SECOND)?;
            l.set_size(INNER_W, A_ROW_H);
            l.set_long_mode(LongMode::DOTS);
            l.set_pos(0, A1_BIT_ROWS_Y + i as i32 * A_ROW_H);
            a1_rows.push(l);
            a1_row_style.push(Cell::new(usize::MAX));
        }

        // A2 灭火瓶压力（1 行 32 px 过程量；`cylinder_configured == Some(false)` ⇒「未配置」）
        let a2_card = decor(&scroll, Dimens::CONTENT_W, A2_CARD_H, &theme::card())?;
        a2_card.set_pos(0, A2_CARD_Y);
        let a2_title = text_label(
            &a2_card,
            group_title("fire_cylinder"),
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        a2_title.set_pos(0, CARD_HEAD_TEXT_Y);
        let a2_value = label(&a2_card, TextSlot::CardValue, Palette::TEXT_PRIMARY)?;
        a2_value.set_size(INNER_W, A_ROW_H);
        a2_value.set_long_mode(LongMode::DOTS);
        a2_value.set_pos(0, CARD_HEAD_H);

        // A3 探测器触发（3 行 × 3 段：bit0 干接点触发 / bit1 复合触发 / bit2 预留）
        let a3_card = decor(&scroll, Dimens::CONTENT_W, A3_CARD_H, &theme::card())?;
        a3_card.set_pos(0, A3_CARD_Y);
        let a3_title = text_label(
            &a3_card,
            group_title("fire_trigger"),
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        a3_title.set_pos(0, CARD_HEAD_TEXT_Y);
        let mut a3_rows = Vec::with_capacity(A3_ROWS);
        for i in 0..A3_ROWS {
            let l = label(&a3_card, TextSlot::Body, Palette::TEXT_SECOND)?;
            l.set_size(INNER_W, A_ROW_H);
            l.set_long_mode(LongMode::DOTS);
            l.set_pos(0, CARD_HEAD_H + i as i32 * A_ROW_H);
            a3_rows.push(l);
        }

        // A4 探测器（汇总 + 分页下钻入口；`N != M` ⇒ 显式提示，**不得静默裁剪**）
        let a4_card = decor(&scroll, Dimens::CONTENT_W, A4_CARD_H, &theme::card())?;
        a4_card.set_pos(0, A4_CARD_Y);
        let a4_title = text_label(
            &a4_card,
            group_title("fire_detector"),
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        a4_title.set_pos(
            0,
            theme::center_offset(Dimens::BTN_H_SECONDARY, TextSlot::SectionTitle.px() as i32),
        );
        let a4_summary = label(&a4_card, TextSlot::Body, Palette::TEXT_PRIMARY)?;
        a4_summary.set_size(INNER_W, A_ROW_H);
        a4_summary.set_long_mode(LongMode::DOTS);
        a4_summary.set_pos(0, A4_SUMMARY_Y);
        let a4_mismatch = label(&a4_card, TextSlot::Body, Palette::STALE)?;
        a4_mismatch.set_size(INNER_W, A_ROW_H);
        a4_mismatch.set_long_mode(LongMode::DOTS);
        a4_mismatch.set_pos(0, A4_MISMATCH_Y);
        a4_mismatch.set_hidden(true);
        let a4_detail = Rc::new(TextButton::create(&a4_card, ui_text::VIEW_DETAIL)?);
        a4_detail.set_size(A4_DETAIL_W, Dimens::BTN_H_SECONDARY);
        a4_detail.set_pos(A4_DETAIL_X, 0);
        a4_detail.label().center();
        theme::button(theme::ButtonKind::Secondary).apply(&a4_detail);

        // ── ③″ 下钻视图（探测器明细；UI §6.4.1「下钻视图通用规格」）───────────────
        //
        // **不是页面**：它是页内的一层覆盖容器（`current_page` 不变、不经导航路由，
        // §15.5.3 / T-20）。**只读**；「上一页 / 下一页 / 收起」均 ≥ `TOUCH_MIN`(48×48)，
        // 相邻可点控件间距 = `GAP_MIN`(16)。
        let drill = layout_box(&root, Dimens::CONTENT_W, VIEWPORT_H)?;
        drill.set_pos(0, VIEWPORT_Y);
        drill.set_hidden(true);

        // 顶部条：标题（左） + 「收起」120×48（**视图顶部右端**，F11.3 的固定出口）
        let drill_title = label(&drill, TextSlot::Body, Palette::TEXT_SECOND)?;
        drill_title.set_size(DRILL_COLLAPSE_X - Dimens::GAP_MIN, DRILL_TOP_H);
        drill_title.set_long_mode(LongMode::DOTS);
        drill_title.set_pos(
            0,
            theme::center_offset(DRILL_TOP_H, TextSlot::Body.px() as i32),
        );
        let drill_collapse = Rc::new(TextButton::create(&drill, ui_text::COLLAPSE)?);
        drill_collapse.set_size(DRILL_BTN_W, DRILL_TOP_H);
        drill_collapse.set_pos(DRILL_COLLAPSE_X, 0);
        drill_collapse.label().center();
        theme::button(theme::ButtonKind::Secondary).apply(&drill_collapse);

        // 表头（8 列 24 px；状态列为紧凑式「报警总状态/故障总状态」—— D-2 去掉列名与分隔空格后
        // 两个位名都放得下；位名入表头，见 **IL29⑥**）
        let mut drill_head = Vec::with_capacity(DRILL_COLS);
        for (i, text) in drill_head_texts().into_iter().enumerate() {
            let l = label(&drill, TextSlot::Weak, Palette::TEXT_WEAK)?;
            l.set_size(drill_col_w(i), DRILL_HEAD_H);
            l.set_long_mode(LongMode::DOTS);
            l.set_pos(drill_col_x(i), DRILL_TOP_H);
            l.set_text(&text);
            drill_head.push(l);
        }

        // 行区（**独立滚动容器**；行池 = 服务端页大小缺省 20）
        let drill_rows_box = ScrollContainer::create(&drill)?;
        drill_rows_box.set_size(Dimens::CONTENT_W, DRILL_ROWS_H);
        drill_rows_box.set_pos(0, DRILL_TOP_H + DRILL_HEAD_H);
        drill_rows_box.add_style(
            &theme::transparent(),
            crate::lvgl::style::StyleSelector::main(),
        );
        let mut drill_rows = Vec::with_capacity(DRILL_ROW_POOL);
        for r in 0..DRILL_ROW_POOL {
            let mut cells = Vec::with_capacity(DRILL_CELLS);
            let mut cell_style = Vec::with_capacity(DRILL_CELLS);
            for c in 0..DRILL_CELLS {
                let l = label(&drill_rows_box, TextSlot::Body, Palette::TEXT_PRIMARY)?;
                l.set_size(drill_cell_w(c), DRILL_ROW_H);
                l.set_long_mode(LongMode::DOTS);
                l.set_pos(drill_cell_x(c), r as i32 * DRILL_ROW_H);
                cells.push(l);
                cell_style.push(Cell::new(usize::MAX));
            }
            drill_rows.push(DrillRow { cells, cell_style });
        }

        // 分页条：「上一页」/「下一页」（各 120×48，**间距 = `GAP_MIN`(16)**） + 页码
        let drill_prev = Rc::new(TextButton::create(&drill, ui_text::PREV_PAGE)?);
        drill_prev.set_size(DRILL_BTN_W, DRILL_PAGE_H);
        drill_prev.set_pos(0, VIEWPORT_H - DRILL_PAGE_H);
        drill_prev.label().center();
        theme::button(theme::ButtonKind::Secondary).apply(&drill_prev);
        let drill_next = Rc::new(TextButton::create(&drill, ui_text::NEXT_PAGE)?);
        drill_next.set_size(DRILL_BTN_W, DRILL_PAGE_H);
        drill_next.set_pos(DRILL_BTN_W + Dimens::GAP_MIN, VIEWPORT_H - DRILL_PAGE_H);
        drill_next.label().center();
        theme::button(theme::ButtonKind::Secondary).apply(&drill_next);
        let drill_page_text = label(&drill, TextSlot::Body, Palette::TEXT_SECOND)?;
        drill_page_text.set_size(
            Dimens::CONTENT_W - 2 * (DRILL_BTN_W + Dimens::GAP_MIN),
            DRILL_PAGE_H,
        );
        drill_page_text.set_long_mode(LongMode::DOTS);
        drill_page_text.set_pos(
            2 * (DRILL_BTN_W + Dimens::GAP_MIN),
            VIEWPORT_H - DRILL_PAGE_H
                + theme::center_offset(DRILL_PAGE_H, TextSlot::Body.px() as i32),
        );

        // 失败态（`/fire_detectors` 非 2xx ⇒ **只显本地固定文案**，R-4 产品裁定）+ 「重试」
        let drill_fail = label(&drill, TextSlot::SectionTitle, Palette::STOPPED)?;
        drill_fail.set_size(
            Dimens::CONTENT_W - DRILL_BTN_W - Dimens::GAP_MIN,
            DRILL_PAGE_H,
        );
        drill_fail.set_long_mode(LongMode::DOTS);
        drill_fail.set_pos(0, DRILL_TOP_H + DRILL_HEAD_H);
        drill_fail.set_text(ui_text::DETAIL_UNAVAILABLE);
        let drill_retry = Rc::new(TextButton::create(&drill, ui_text::RETRY)?);
        drill_retry.set_size(DRILL_BTN_W, DRILL_PAGE_H);
        drill_retry.set_pos(DRILL_COLLAPSE_X, DRILL_TOP_H + DRILL_HEAD_H);
        drill_retry.label().center();
        theme::button(theme::ButtonKind::Secondary).apply(&drill_retry);
        drill_fail.set_hidden(true);
        drill_retry.set_hidden(true);
        drill_rows_box.set_hidden(false);

        // ── ④ 固定操作条（不随滚动；UI §6.4 线框 `Y624`）──
        let action_bar = decor(
            &root,
            Dimens::CONTENT_W,
            ACTION_BAR_H,
            &theme::card_head_bar(Palette::SURFACE),
        )?;
        action_bar.set_pos(0, ACTION_BAR_Y);
        let release = Rc::new(TextButton::create(&action_bar, TEXT_RELEASE)?);
        release.set_size(OP_BTN_W, Dimens::BTN_H_PRIMARY);
        release.set_pos(0, ACTION_BTN_Y);
        release.label().center();
        theme::button(theme::ButtonKind::Danger).apply(&release);
        let restart = Rc::new(TextButton::create(&action_bar, TEXT_ACK_M1)?);
        restart.set_size(OP_BTN_W, Dimens::BTN_H_PRIMARY);
        restart.set_pos(OP_BTN2_X, ACTION_BTN_Y);
        restart.label().center();
        theme::button(theme::ButtonKind::Danger).apply(&restart);
        let audit_note = text_label(
            &action_bar,
            TEXT_AUDIT_NOTE,
            TextSlot::Body,
            Palette::TEXT_WEAK,
        )?;
        audit_note.set_size(OP_NOTE_W, TextSlot::Body.px() as i32);
        audit_note.set_long_mode(LongMode::DOTS);
        audit_note.set_pos(OP_NOTE_X, OP_NOTE_Y);

        // ── ⑤ 就地原因带（**IL4 / IL5**：视口与操作条之间的 24 px 双槽）──
        let reason_left = label(&root, TextSlot::Body, Palette::STOPPED)?;
        reason_left.set_size(REASON_LEFT_W, TextSlot::Body.px() as i32);
        reason_left.set_long_mode(LongMode::DOTS);
        reason_left.set_pos(0, REASON_BAND_Y + REASON_TEXT_Y);
        reason_left.set_hidden(true);
        let reason_right = label(&root, TextSlot::Body, Palette::STOPPED)?;
        reason_right.set_size(REASON_RIGHT_W, TextSlot::Body.px() as i32);
        reason_right.set_long_mode(LongMode::DOTS);
        reason_right.set_pos(REASON_RIGHT_X, REASON_BAND_Y + REASON_TEXT_Y);
        reason_right.set_hidden(true);

        // ── 样式表（**只建一次**：避免 1 Hz 每拍 new Style 与无界挂样式）──
        let state_styles = [
            theme::card_head_bar(state_color(STATE_VIEW_ORDER[0])),
            theme::card_head_bar(state_color(STATE_VIEW_ORDER[1])),
            theme::card_head_bar(state_color(STATE_VIEW_ORDER[2])),
        ];
        let state_icon_styles = [
            theme::icon(Dimens::ICON_XL, state_color(STATE_VIEW_ORDER[0])),
            theme::icon(Dimens::ICON_XL, state_color(STATE_VIEW_ORDER[1])),
            theme::icon(Dimens::ICON_XL, state_color(STATE_VIEW_ORDER[2])),
        ];
        let state_text_styles = [
            theme::text(TextSlot::InterlockState, state_color(STATE_VIEW_ORDER[0])),
            theme::text(TextSlot::InterlockState, state_color(STATE_VIEW_ORDER[1])),
            theme::text(TextSlot::InterlockState, state_color(STATE_VIEW_ORDER[2])),
        ];
        let reason_styles = [
            theme::text(TextSlot::Body, Palette::STOPPED),
            theme::text(TextSlot::Body, Palette::STALE),
            theme::text(TextSlot::Body, Palette::DANGER),
        ];
        // ── U-73 样式表（**只建一次**；色值全部取 UI §3.2 既有语义色，**不新增色值**）──
        //
        // 火警等级 5 档（正常 / 警示 / 危险 / 停机 / 中性）：`未知` 与 `未定义` 一律**中性**
        // （`TEXT_WEAK`）—— **绝不用绿色**（F21.2 / EX-10）。三条通道（卡左缘竖条 / 图标 /
        // 文案）**共用同一档**（三重冗余）。
        let fire_bar_styles = [
            theme::card_head_bar(fire_slot_color(FIRE_SLOT_OK)),
            theme::card_head_bar(fire_slot_color(FIRE_SLOT_WARN)),
            theme::card_head_bar(fire_slot_color(FIRE_SLOT_DANGER)),
            theme::card_head_bar(fire_slot_color(FIRE_SLOT_STOP)),
            theme::card_head_bar(fire_slot_color(FIRE_SLOT_UNKNOWN)),
        ];
        let fire_icon_styles = [
            theme::icon(FIRE_ICON_W, fire_slot_color(FIRE_SLOT_OK)),
            theme::icon(FIRE_ICON_W, fire_slot_color(FIRE_SLOT_WARN)),
            theme::icon(FIRE_ICON_W, fire_slot_color(FIRE_SLOT_DANGER)),
            theme::icon(FIRE_ICON_W, fire_slot_color(FIRE_SLOT_STOP)),
            theme::icon(FIRE_ICON_W, fire_slot_color(FIRE_SLOT_UNKNOWN)),
        ];
        let fire_value_styles = [
            theme::text(TextSlot::PhasePower, fire_slot_color(FIRE_SLOT_OK)),
            theme::text(TextSlot::PhasePower, fire_slot_color(FIRE_SLOT_WARN)),
            theme::text(TextSlot::PhasePower, fire_slot_color(FIRE_SLOT_DANGER)),
            theme::text(TextSlot::PhasePower, fire_slot_color(FIRE_SLOT_STOP)),
            theme::text(TextSlot::PhasePower, fire_slot_color(FIRE_SLOT_UNKNOWN)),
        ];
        // A1 位行 3 档（活跃 = 告警红 / 非活跃 = 次文 / 未定义 = 弱注）。
        let a1_styles = [
            theme::text(TextSlot::Body, a1_slot_color(A1_STYLE_ACTIVE)),
            theme::text(TextSlot::Body, a1_slot_color(A1_STYLE_INACTIVE)),
            theme::text(TextSlot::Body, a1_slot_color(A1_STYLE_UNDEF)),
        ];
        // A2 主值 2 档（正常 / 「未配置」弱注）。
        let a2_styles = [
            theme::text(TextSlot::CardValue, Palette::TEXT_PRIMARY),
            theme::text(TextSlot::CardValue, Palette::TEXT_WEAK),
        ];
        // 下钻明细格 4 档（正常值 / 降级 / 位活跃 / 位非活跃）。
        let drill_cell_styles = [
            theme::text(TextSlot::Body, Palette::TEXT_PRIMARY),
            theme::text(TextSlot::Body, Palette::TEXT_WEAK),
            theme::text(TextSlot::Body, Palette::DANGER),
            theme::text(TextSlot::Body, Palette::TEXT_SECOND),
        ];

        let core = Rc::new(Core {
            root,
            band,
            scroll,
            action_bar,
            audit_note,
            state_card,
            state_bar_top,
            state_bar_left,
            _state_title: state_title,
            state_chips,
            state_frozen,
            state_icon,
            state_text_l,
            state_styles,
            state_icon_styles,
            state_text_styles,
            state_bar_top_style: Cell::new(usize::MAX),
            state_bar_left_style: Cell::new(usize::MAX),
            state_icon_style: Cell::new(usize::MAX),
            state_text_style: Cell::new(usize::MAX),
            fire_card,
            _fire_title: fire_title,
            fire_bar_left,
            fire_icon,
            fire_value,
            fire_bar_styles,
            fire_icon_styles,
            fire_value_styles,
            fire_bar_style: Cell::new(usize::MAX),
            fire_icon_style: Cell::new(usize::MAX),
            fire_value_style: Cell::new(usize::MAX),
            a1_card,
            a1_notice,
            a1_notice_style: Cell::new(usize::MAX),
            a1_rows,
            a1_row_style,
            a1_styles,
            a2_card,
            a2_value,
            a2_styles,
            a2_style: Cell::new(usize::MAX),
            a3_card,
            a3_rows,
            a4_card,
            a4_summary,
            a4_mismatch,
            a4_detail,
            drill,
            drill_title,
            drill_head,
            drill_rows_box,
            drill_rows,
            drill_prev,
            drill_next,
            drill_collapse,
            drill_page_text,
            drill_fail,
            drill_retry,
            drill_cell_styles,
            source_card,
            source_title,
            source_rows_box,
            source_rows,
            source_frozen,
            source_empty,
            source_unavailable,
            lamps,
            release,
            restart,
            reason_left,
            reason_right,
            reason_styles,
            reason_left_style: Cell::new(usize::MAX),
            reason_right_style: Cell::new(usize::MAX),
            section: RefCell::new(InterlockSection::default()),
            periph: RefCell::new(PeripheralsSection::default()),
            catalog: RefCell::new(None),
            fire_page: RefCell::new(None),
            fire_page_failed: Cell::new(false),
            fire_page_req: Cell::new(1),
            fire_page_size: Cell::new(mupc_display_proto::DEFAULT_PERIPH_PAGE_SIZE),
            drill_open: Cell::new(false),
            fire_summary: RefCell::new(FireSummary::default()),
            ever_available: Cell::new(false),
            submitting: Cell::new(false),
            last_op: Cell::new(OpKind::Release),
            last_reject: RefCell::new(None),
            plain_reason: RefCell::new(None),
            conflict: Cell::new(false),
            countdown_on: Cell::new(false),
            countdown_remaining: Cell::new(0),
            countdown_base: Cell::new(None),
            countdown_left: Cell::new(0),
            refresh_requested: Cell::new(false),
            pending_close: Cell::new(false),
            dialog: RefCell::new(None),
            toast: RefCell::new(None),
            on_release: RefCell::new(None),
            on_ack_m1: RefCell::new(None),
            on_fire_page: RefCell::new(None),
            open_fail_logs: Cell::new(0),
        });

        // 按钮：只**开弹层**（确认完成前不发任何请求）。
        {
            let w = Rc::downgrade(&core);
            core.release.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                if let Err(e) = open_dialog(&c, OpKind::Release) {
                    c.note_open_failure(TEXT_RELEASE, &e);
                }
            });
        }
        {
            let w = Rc::downgrade(&core);
            core.restart.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                if let Err(e) = open_dialog(&c, OpKind::AckM1) {
                    c.note_open_failure(TEXT_ACK_M1, &e);
                }
            });
        }

        // U-73 下钻的五枚只读控件（**全部是意图 / 视图动作**，不发请求 —— 见模块文档）。
        {
            let w = Rc::downgrade(&core);
            core.a4_detail.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                c.set_drill_open(true);
                c.request_fire_page(1);
            });
        }
        {
            let w = Rc::downgrade(&core);
            core.drill_collapse.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                c.set_drill_open(false);
            });
        }
        {
            let w = Rc::downgrade(&core);
            core.drill_prev.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                let p = c.fire_page_req.get().saturating_sub(1).max(1);
                c.request_fire_page(p);
            });
        }
        {
            let w = Rc::downgrade(&core);
            core.drill_next.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                let p = c.fire_page_req.get().saturating_add(1);
                c.request_fire_page(p);
            });
        }
        {
            let w = Rc::downgrade(&core);
            core.drill_retry.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                let p = c.fire_page_req.get();
                c.request_fire_page(p);
            });
        }

        let page = Self { core };
        // 骨架态 = **无帧** ⇒ 契约缺省（`available = false`）⇒ 屏显「联锁状态不可用」；
        // 消防区同样落在契约缺省（外设段 `available = false` ⇒「外设数据不可用」，EDGE-22）。
        page.core.apply_section(&InterlockSection::default());
        page.core.apply_periph(&PeripheralsSection::default());
        // 下钻骨架：行池全隐、失败态隐藏（**不显示半截空表**）。
        page.core.refresh_drill();
        page.core.drill_rows_box.set_hidden(false);
        Ok(page)
    }

    /// 页根对象。
    pub fn obj(&self) -> &Obj {
        &self.core.root
    }

    /// 滚动视口对象（装配断言口径）。
    pub fn scroll_obj(&self) -> &Obj {
        &self.core.scroll
    }

    /// 固定操作条对象（装配断言口径）。
    pub fn action_bar_obj(&self) -> &Obj {
        &self.core.action_bar
    }

    /// 联锁总态卡对象（装配断言口径；**存活锚点** —— 见 `Core::state_card` 的字段注）。
    pub fn state_card_obj(&self) -> &Obj {
        &self.core.state_card
    }

    /// 触发源卡对象（装配断言口径）。
    pub fn source_card_obj(&self) -> &Obj {
        &self.core.source_card
    }

    /// 联锁总态卡的**区块级「冻结 / 数据过期」角标**当前是否可见（EDGE-03 / EDGE-20）。
    ///
    /// **真实读回**（`IL29②` 的产品裁定 2026-09-25：**恢复**该卡的打标，取**图标-only** 的
    /// 不重叠替代布局，见 [`crate::ui::pages::frozen_icon_badge`]）—— 判据与触发源卡同一拍、
    /// 同一真源（[`crate::ui::pages::frame_mark`]），本谓词只**观测**，不另立判据。
    ///
    /// **改什么会让消费它的断言变红**：把 `Core::apply_frame_mark` 里对本卡的
    /// `set_visible` 删掉（或写死 `false`）⇒ 断言的"通道断 / 帧旧 ⇒ 可见"两条立刻红。
    pub fn state_frozen_visible(&self) -> bool {
        !self.core.state_frozen.obj().is_hidden()
    }

    /// 上述角标的当前**图标字形**（`⚠` = 冻结 / `!` = 数据过期）——
    /// 图标-only 形态下这是**唯一**的标记身份通道（映射见
    /// [`crate::ui::pages::frozen_mark_icon`]，与 P1 的两个角标同源）。
    ///
    /// ⚠️ **没有 `state_frozen_text()`**：本形态**结构上没有文字通道**（28 px 徽标只放图标，
    /// 这是 `IL29②` 的代价、已登记）—— 保留一个恒空的 `text()` 读口只会是"零判别力的 API"。
    pub fn state_frozen_icon(&self) -> Option<String> {
        self.core.state_frozen.icon_text()
    }

    /// 触发源卡的同类角标是否可见。
    pub fn source_frozen_visible(&self) -> bool {
        !self.core.source_frozen.obj().is_hidden()
    }

    /// 触发源卡角标的当前文案。
    pub fn source_frozen_text(&self) -> Option<String> {
        self.core.source_frozen.text()
    }

    /// **仅测试**：触发源卡角标的构造侧读回（与 P6 的两张**同一个构造点**
    /// [`crate::ui::pages::frozen_chip`]；见 B3-2c 整改 **重要 4**）。
    /// ⚠️ 读尺寸前须有一次布局趟（`Display::refr_now_for_test`）。
    #[cfg(test)]
    pub(crate) fn source_frozen_chip(&self) -> &StatusChip {
        &self.core.source_frozen
    }

    /// **仅测试**：联锁总态卡角标（**图标-only** 形态）的构造侧读回（与触发源卡同族构造点
    /// [`crate::ui::pages::frozen_icon_badge`]；见 **IL29②**）。
    /// ⚠️ 读几何前须有一次布局趟（`Display::refr_now_for_test`）。
    #[cfg(test)]
    pub(crate) fn state_frozen_badge(&self) -> &crate::ui::pages::FrozenBadge {
        &self.core.state_frozen
    }

    /// **仅测试**：联锁总态卡卡头的**标题**标签（几何断言口径 —— **S-1** 的「卡头三件互不
    /// 重叠」读它取右缘）。宽由文本自增（未 `set_size`）⇒ `coords()` 量到的是**真实占宽**。
    /// ⚠️ 读几何前须有一次布局趟（`Display::refr_now_for_test`）。
    #[cfg(test)]
    pub(crate) fn state_title_obj(&self) -> &Obj {
        self.core._state_title.obj()
    }

    /// **仅测试**：联锁总态卡**当前可见**的那支 latch 胶囊对象（几何断言口径；三态互斥 ⇒
    /// 屏上恒 1 支，见 [`P4InterlockPage::latch_chip_visible_count`]）。
    /// ⚠️ 读几何前须有一次布局趟（`Display::refr_now_for_test`）。
    #[cfg(test)]
    pub(crate) fn state_latch_chip_obj(&self) -> &Obj {
        self.core.state_chips[state_index(self.state_view())].obj()
    }

    // ── 数据入口（**读路径 = 帧驱动**）─────────────────────────────────────

    /// 注入一帧（`frame = None` ⇒ 该段取契约缺省 = **不可用**）。
    ///
    /// **`freshness` / `channel` 的消费点（B3-2c 起）**：只用于**区块级可信度打标**
    /// （[`crate::ui::pages::frame_mark`] ⇒ 卡头「冻结 / 数据过期」角标）。
    /// ⚠️ **联锁段自身的 `available` 仍是展示判据**（`frame = None` ⇒ 不可用，绝不回落
    /// 「未联锁」）；通道级**整屏**降级另由外壳遮罩承担（UI §8.3，`ui/shell.rs`）。
    ///
    /// **打标不改变数值**：`Down` / `Stale` 时帧被**保留**（EDGE-03 明文），本方法照常把
    /// 那份冻结数据铺上屏（`apply_section`），只是多打一个角标。
    pub fn render(&self, input: &PageInput<'_>) {
        let (section, periph) = match input.frame {
            Some(f) => (f.interlock.clone(), f.peripherals.clone()),
            None => (InterlockSection::default(), PeripheralsSection::default()),
        };
        self.core.apply_section(&section);
        self.core
            .apply_frame_mark(crate::ui::pages::frame_mark(input));
        // U-73：外设段（缺帧 / 旧帧 ⇒ 契约缺省 = `available = false`）
        self.core.apply_periph(&periph);
    }

    /// 直接注入联锁段（等价于 [`P4InterlockPage::render`] 的帧内那一段；供 B3 接线与离屏用例）。
    pub fn set_section(&self, section: &InterlockSection) {
        self.core.apply_section(section);
    }

    // ── U-73 外设 / 消防数据入口（**契约 2**：本页不自行发请求）────────────────

    /// 注入外设段（等价于 [`P4InterlockPage::render`] 的帧内那一段）。
    pub fn set_periph(&self, sec: &PeripheralsSection) {
        self.core.apply_periph(sec);
    }

    /// 注入元数据目录（`GET /v1/console/peripherals/catalog` 的结果；设计 §15.3.1）。
    ///
    /// 读取时机由**接线层**按帧内 `catalog_rev` 判定（首次进入 / rev 变化）；本页只消费。
    pub fn set_catalog(&self, cat: &PeripheralCatalog) {
        *self.core.catalog.borrow_mut() = Some(cat.clone());
        self.core.refresh_fire();
    }

    /// catalog **未取到 / 重取失败** ⇒ 清掉：中文名位显「名称未获取」（`ui_text::NAME_UNKNOWN`）、
    /// **值照常显示**（按点名）—— **不臆造中文名**（§15.3.1）。
    pub fn clear_catalog(&self) {
        *self.core.catalog.borrow_mut() = None;
        self.core.refresh_fire();
    }

    /// 注入探测器明细页（`GET /v1/console/peripherals/fire_detectors` 的结果；F21.4）。
    pub fn set_fire_page(&self, page: &FireDetectorPage) {
        self.core.fire_page_failed.set(false);
        *self.core.fire_page.borrow_mut() = Some(page.clone());
        self.core.refresh_drill();
    }

    /// 明细端点**不可用**（非 2xx / 超时 / 解码失败）⇒ 下钻视图显「明细不可用」+「重试」。
    ///
    /// ⚠️ **只传失败事实、不传错误串**（**R-4 产品裁定**）：服务端 400/503 的原因串只进日志 /
    /// 现场排障 —— 它是**外部错误串**（含 cmap 外的字，上屏必出豆腐块）且属**运行期字符串**，
    /// 码表覆盖率用例天生扫不到（H-2 盲区）。同款先例 = `post_config_apply` 的固定 `message` 口径。
    pub fn set_fire_page_failed(&self) {
        self.core.fire_page_failed.set(true);
        *self.core.fire_page.borrow_mut() = None;
        self.core.refresh_drill();
    }

    /// 注入分页大小（= 配置 `display.periph_page_size`；接线层在装配时喂一次）。
    ///
    /// 只影响「第 X / Y 页」的分母（响应自带 `page_size` 时以响应为准）。
    pub fn set_fire_page_size(&self, size: u32) {
        self.core.fire_page_size.set(size.max(1));
        self.core.refresh_drill();
    }

    /// **等价于点击「查看明细」**：开下钻 + 请求第 1 页（与按钮回调走**同一个**
    /// `Core::set_drill_open` / `Core::request_fire_page`）。
    pub fn show_detail(&self) {
        self.core.set_drill_open(true);
        self.core.request_fire_page(1);
    }

    /// **等价于点击「收起」**：关下钻视图（**不改 `current_page`**）。
    pub fn collapse_detail(&self) {
        self.core.set_drill_open(false);
    }

    /// **等价于点击「上一页 / 下一页」**：请求第 `page` 页（≥1）。
    pub fn goto_detail_page(&self, page: u32) {
        self.core.request_fire_page(page);
    }

    /// 注册「请求第 N 页探测器明细」意图回调（`N` 1 起；本页**不发请求**）。
    pub fn set_on_fire_page<F>(&self, f: F)
    where
        F: FnMut(u32) + 'static,
    {
        if let Ok(mut slot) = self.core.on_fire_page.try_borrow_mut() {
            *slot = Some(Box::new(f));
        }
    }

    /// 下钻视图是否打开（装配 / 离屏断言口径；**等价于 §15.5.3 的"是否在下钻态"**）。
    pub fn drill_open(&self) -> bool {
        self.core.drill_open.get()
    }

    /// 下钻容器对象（**不是页面**：`current_page` 不变，T-20）。
    pub fn drill_obj(&self) -> &Obj {
        &self.core.drill
    }

    /// 【安全总览带】容器对象（T-25 版面断言用）。
    pub fn band_obj(&self) -> &Obj {
        &self.core.band
    }

    /// 火警等级卡对象（T-25：两卡间隙断言用）。
    pub fn fire_card_obj(&self) -> &Obj {
        &self.core.fire_card
    }

    /// 火警等级**文案**（T-15）。
    pub fn fire_value_text(&self) -> Option<String> {
        self.core.fire_value.text()
    }

    /// 火警等级**图标**（T-15：三重冗余的图标通道）。
    pub fn fire_icon_text(&self) -> Option<String> {
        self.core.fire_icon.text()
    }

    /// 火警等级当前**样式档**（T-15：`未知` 必须是中性档 —— 由 [`fire_slot_color`] 钉死）。
    pub fn fire_slot(&self) -> usize {
        self.core.fire_value_style.get()
    }

    /// A1 逐位 16 行的文案（T-19b / T-18）。
    pub fn a1_row_text(&self, i: usize) -> Option<String> {
        self.core.a1_rows.get(i).and_then(Label::text)
    }

    /// A1 第 `i` 行的样式档（0 活跃 / 1 非活跃 / 2 未定义）。
    pub fn a1_row_slot(&self, i: usize) -> Option<usize> {
        self.core.a1_row_style.get(i).map(Cell::get)
    }

    /// A1 **段顶通告**行的文案（段级 / 站级降级时「外设数据不可用」/ 站文案；无降级 ⇒ `None`）。
    ///
    /// **为什么它是独立读口**（T21c-1-r1 / 评审 W-3②）：通告**不再**占用位行第 0 行 ⇒
    /// 「通告在不显」与「bit0 的语义」是两件事，**必须**能用两个读口分别观测
    /// （否则「bit0 语义在本态可判读」这条断言没有判据）。
    pub fn a1_notice_text(&self) -> Option<String> {
        self.core.a1_notice.text()
    }

    /// A1 段顶通告行是否在显。
    pub fn a1_notice_visible(&self) -> bool {
        !self.core.a1_notice.obj().is_hidden()
    }

    /// A1 **位行**的行数（恒 `A1_ROWS` = 16；整字位图 16 位 ⇒ **不随数据 / 降级态变**）。
    ///
    /// 判据口径与实现同源：`a1_rows` 池在构造期一次性建齐 16 条（`for i in 0..A1_ROWS`），
    /// `render` 只改文本 / 样式 —— 故 `a1_row_text(15).is_some() && a1_row_text(16).is_none()`
    /// 即「位行恰 16」。本读口把它写成一条显式断言所需的数字。
    pub fn a1_row_count(&self) -> usize {
        self.core.a1_rows.len()
    }

    /// A2 灭火瓶压力行的文案（T-14：`Some(false)` ⇒「未配置」且**不含 `0 kPa`**）。
    pub fn a2_text(&self) -> Option<String> {
        self.core.a2_value.text()
    }

    /// A2 行的样式档（`未配置` ⇒ 弱注档）。
    pub fn a2_slot(&self) -> usize {
        self.core.a2_style.get()
    }

    /// A3 三行的文案。
    pub fn a3_row_text(&self, i: usize) -> Option<String> {
        self.core.a3_rows.get(i).and_then(Label::text)
    }

    /// A4 汇总行文案（T-18：登记 / 可读 / 报警 / 故障 / 离线）。
    pub fn a4_summary_text(&self) -> Option<String> {
        self.core.a4_summary.text()
    }

    /// A4「登记数与可读数不一致」提示是否在显（T-18：`N != M` **不得静默裁剪**）。
    pub fn a4_mismatch_visible(&self) -> bool {
        !self.core.a4_mismatch.obj().is_hidden()
    }

    /// A4「登记数与可读数不一致」提示文案（`ui_text::REGISTERED_READABLE_MISMATCH`）。
    pub fn a4_mismatch_text(&self) -> Option<String> {
        self.core.a4_mismatch.text()
    }

    /// A4「查看明细」按钮对象（T-25 / 触摸目标断言）。
    pub fn a4_detail_button(&self) -> &TextButton {
        &self.core.a4_detail
    }

    /// A1 / A2 / A3 / A4 卡对象（T-25 / 版面断言）。
    pub fn a_card_obj(&self, i: usize) -> Option<&Obj> {
        Some(match i {
            1 => &self.core.a1_card,
            2 => &self.core.a2_card,
            3 => &self.core.a3_card,
            4 => &self.core.a4_card,
            _ => return None,
        })
    }

    /// 下钻标题（「探测器 · 第 X / Y 页」）。
    pub fn drill_title_text(&self) -> Option<String> {
        self.core.drill_title.text()
    }

    /// 下钻页码（「X / Y 页」）。
    pub fn drill_page_text(&self) -> Option<String> {
        self.core.drill_page_text.text()
    }

    /// 下钻表头第 `i` 列文本（列名 / 单位 / 位名的**逐列走查口径**）。
    pub fn drill_head_text(&self, i: usize) -> Option<String> {
        self.core.drill_head.get(i).and_then(Label::text)
    }

    /// 下钻第 `r` 行第 `c` 格的文案（T-17 / T-18）。
    pub fn drill_cell_text(&self, r: usize, c: usize) -> Option<String> {
        self.core
            .drill_rows
            .get(r)
            .and_then(|row| row.cells.get(c))
            .and_then(Label::text)
    }

    /// 下钻第 `r` 行是否可见。
    pub fn drill_row_visible(&self, r: usize) -> bool {
        self.core
            .drill_rows
            .get(r)
            .and_then(|row| row.cells.first())
            .is_some_and(|l| !l.obj().is_hidden())
    }

    /// 下钻失败态文案（「明细不可用」/「消防源不可用」；R-4）。
    pub fn drill_fail_text(&self) -> Option<String> {
        self.core.drill_fail.text()
    }

    /// 下钻失败态是否在显。
    pub fn drill_fail_visible(&self) -> bool {
        !self.core.drill_fail.obj().is_hidden() && self.core.drill_open.get()
    }

    /// 下钻「重试」按钮是否在显。
    pub fn drill_retry_visible(&self) -> bool {
        !self.core.drill_retry.button().obj().is_hidden() && self.core.drill_open.get()
    }

    /// 下钻「上一页」/「下一页」/「收起」对象（T-25：≥48×48 与间距 16）。
    pub fn drill_prev_button(&self) -> &TextButton {
        &self.core.drill_prev
    }
    pub fn drill_next_button(&self) -> &TextButton {
        &self.core.drill_next
    }
    pub fn drill_collapse_button(&self) -> &TextButton {
        &self.core.drill_collapse
    }

    /// 下钻「上一页」是否禁用（首页）。
    pub fn drill_prev_disabled(&self) -> bool {
        widgets::has_state(
            self.core.drill_prev.button().obj(),
            crate::lvgl::style::State::DISABLED,
        )
    }

    /// 下钻「下一页」是否禁用（末页）。
    pub fn drill_next_disabled(&self) -> bool {
        widgets::has_state(
            self.core.drill_next.button().obj(),
            crate::lvgl::style::State::DISABLED,
        )
    }

    /// 最近一次请求的页码（意图断言口径）。
    pub fn fire_page_req(&self) -> u32 {
        self.core.fire_page_req.get()
    }

    /// A4 汇总的**结构化**数据（T-14 / T-18：登记 / 可读 / 报警 / 故障 / 离线）。
    #[cfg(test)]
    pub(crate) fn fire_summary(&self) -> FireSummary {
        *self.core.fire_summary.borrow()
    }

    // ── 写路径入口 ────────────────────────────────────────────────────────

    /// 注册「人工释放联锁」意图回调（确认完成时**调用一次**，载荷 = UI 观测态）。
    ///
    /// ⚠️ **回调内不得回灌本页会关弹层的方法**（[`P4InterlockPage::show_result`] /
    /// [`P4InterlockPage::set_section`]）：那些会 `ConfirmDialog::close`，而本回调正是从该弹层的
    /// 事件回调里出来的（`components.rs` 明确要求"关闭由调用方在自己的 tick 里做"）。
    /// 正确姿势：回调内只把载荷交给网络任务，结果回来后在**事件循环的下一拍**调
    /// [`P4InterlockPage::show_result`]。
    pub fn set_on_release<F>(&self, f: F)
    where
        F: FnMut(InterlockOpPayload) + 'static,
    {
        // **重入语义（M5 / IL28）**：槽正被借用（= 正在 `fire_release` 里调用户回调）时
        // **静默跳过**本次注册 —— 与 `fire_*` 侧的 `try_borrow_mut` **同一纪律**。
        // 用 `borrow_mut` 会在重入时 panic，而 panic 出事件回调虽被事件桥 `catch_unwind`
        // 兜住，却是**静默吞掉**（屏上无任何迹象）⇒ 与本页「回调绝不 panic」的纪律相悖。
        if let Ok(mut slot) = self.core.on_release.try_borrow_mut() {
            *slot = Some(Box::new(f));
        }
    }

    /// 注册「M1 授权重启」意图回调（语义同 [`P4InterlockPage::set_on_release`]，含**重入时
    /// 静默跳过**的同一处置）。
    pub fn set_on_ack_m1<F>(&self, f: F)
    where
        F: FnMut(InterlockOpPayload) + 'static,
    {
        if let Ok(mut slot) = self.core.on_ack_m1.try_borrow_mut() {
            *slot = Some(Box::new(f));
        }
    }

    /// 提交中（外部发出请求后置 `true`；回执到达后由 [`P4InterlockPage::show_result`] 复位）。
    /// 两按钮 `disabled`（**IL15**：§3.6 无该态就地文案 ⇒ 不造）。
    pub fn set_submitting(&self, on: bool) {
        self.core.submitting.set(on);
        self.core.refresh_actions();
    }

    /// **结构化**拒绝注入（`InterlockApi` 直连路径 / B3 自行解析出 `InterlockReject` 时调用）。
    ///
    /// ⚠️ **可达性登记（当前生产路径不可达；行为保持不变）**：控制回执契约
    /// （`display-proto/src/control.rs`）**没有** `reject: Option<InterlockReject>` 这类字段
    /// （见 **IL12**）⇒ 线上路径**取不到**结构化的 `InterlockReject`，`show_result` 的分派只会
    /// 走"服务端 `message`"那条。本方法保留给**将来**契约补上结构化拒绝字段（或 `InterlockApi`
    /// 直连路径复活）时的单一入口；**不得**为了"让它有用"而在 `control_route` 里按 `code` /
    /// 消息串**猜**出一个 `InterlockReject`（猜错即**谎报原因**，见 PM 裁定 1）。
    ///
    /// 文案由 [`reject_text`] 按**结构化字段**重建（含 `SourcesNotReset` 的**具体**源名 ——
    /// EDGE-12「不得静默失败」），**不用**契约的 `user_message()`（含缺字全角标点与小写机器名）。
    /// `HoldNotElapsed` 走**倒计时**槽（UI §6.4「保持时间不足」行；见 **IL14**）。
    /// **弹层不自动关闭**（IL11）。
    pub fn show_reject(&self, r: &InterlockReject) {
        self.core.submitting.set(false);
        *self.core.plain_reason.borrow_mut() = None;
        self.core.conflict.set(false);
        match r {
            InterlockReject::HoldNotElapsed { remaining_secs, .. } => {
                self.core.countdown_remaining.set(*remaining_secs);
                self.core.countdown_base.set(None);
                self.core.countdown_left.set(*remaining_secs);
                self.core.countdown_on.set(true);
                *self.core.last_reject.borrow_mut() = None;
            }
            _ => {
                self.core.countdown_on.set(false);
                *self.core.last_reject.borrow_mut() = Some(r.clone());
            }
        }
        self.core.refresh_actions();
    }

    /// EDGE-19：提交时状态已变化（UI §6.4 → 固定文案「联锁状态已变化 · 请刷新后重试」）。
    ///
    /// 「自动触发一次状态刷新」由**标志**表达：外部调 [`P4InterlockPage::take_refresh_request`]
    /// 取走并执行一次 `GET`（本页**不发请求** —— 与 P2 同一口径，见 **IL12**）。
    ///
    /// # ⚠️ 可达性登记（**当前生产路径不可达**）+ 沿革（PM 裁定 1，2026-09-16）
    ///
    /// 本方法是**显式入口**，只在**客户端本地能判定**「提交时状态已变化」时才该被调用
    /// （例如将来页面自己比对 `observed_latched` / `observed_sources` 与当前帧 —— 契约目前
    /// **没有**能自动达成该判定的字段，见 **IL12**）。
    ///
    /// **沿革（不得抹去）**：B3-2b-2 曾按**任务书**在 `control_route::route` 里给
    /// [`ControlCode::RejectedPrecondition`] 单开一条臂 ⇒ 本方法（即把**所有**前置条件拒绝
    /// 都显成这条**固定**文案）。**该实现已被主控推翻**（**谎报原因**：`RejectedPrecondition`
    /// 把「状态已变化」与「触发源未复位 / 保持时间不足 / latch / `StopPending`」糊在同一个码里）
    /// ⇒ 那条臂已**整条删除**，一切业务拒绝统一走 [`P4InterlockPage::show_result`]（具体原因由
    /// **服务端** `message` 承担 —— 真·状态变化时服务端返回的就是这句话，EDGE-19 的文案
    /// **照样按其本意出现**，只是判定方归服务端）。故本方法在**生产路径上零调用者**：
    /// 当前唯一调用点是 `ui/tests.rs` 的**显式入口**断言（固定文案仍可达、仍可测）。
    pub fn show_conflict(&self) {
        self.core.submitting.set(false);
        *self.core.last_reject.borrow_mut() = None;
        *self.core.plain_reason.borrow_mut() = None;
        self.core.countdown_on.set(false);
        self.core.conflict.set(true);
        self.core.refresh_requested.set(true);
        self.core.refresh_actions();
    }

    /// EDGE-18：审计不可写（fail-closed —— **操作未执行**；UI §8.3 明文的**固定**文案）。
    ///
    /// 落点两条（**不吞**）：Toast「审计不可用 · 操作未执行」+ 就地原因带同文案（红）；
    /// **弹层不自动关闭**（IL11：原因常驻可读，用户可关闭后重试）。
    pub fn show_audit_unavailable(&self) -> Result<(), LvglError> {
        self.core.submitting.set(false);
        self.core.countdown_on.set(false);
        self.core.conflict.set(false);
        *self.core.last_reject.borrow_mut() = None;
        self.core
            .show_toast(ToastTone::Failure, ICON_FAIL, TEXT_AUDIT_UNAVAILABLE)?;
        self.core
            .set_plain_reason(TEXT_AUDIT_UNAVAILABLE, ReasonSlot::Red);
        Ok(())
    }

    /// 回执注入（成功 / 失败两条路径，设计 §6.4「成功」「失败」两行）。
    ///
    /// - **成功**：关弹层（确认链路走完）+ 用回执 `applied` **立即**刷新（不等下一帧，
    ///   F17.6 / IL-02，映射见 **IL13**）+ Toast 成功；
    /// - **失败**：**弹层不自动关闭**（IL11）+ 就地原因带上屏 `display_safe(message)`（EDGE-12）；
    ///   `AuditUnavailable` 走 UI §8.3 的**固定**文案（EDGE-18，fail-closed）；
    ///   `RejectedPrecondition` 另置「请求一次状态刷新」标志（**IL12**）。
    ///
    /// ⚠️ `duplicate`（幂等命中）本页未读取 —— 见 **IL19**（UI §3.6 无对应文案 ⇒ 漏覆盖）。
    pub fn show_result(&self, resp: &ControlResponse<InterlockOpAck>) -> Result<(), LvglError> {
        self.core.submitting.set(false);
        if resp.ok {
            self.core.close_dialog();
            if let Some(ack) = resp.applied.as_ref() {
                self.core.apply_ack(ack);
                // **回执立即刷屏**（不等下一帧，F17.6 / IL-02）：`apply_ack` 只改数据 ⇒ 必须重画。
                self.core.rerender();
            }
            self.core.clear_reason();
            self.core.show_toast(
                ToastTone::Success,
                ICON_OK,
                self.core.last_op.get().toast_ok(),
            )?;
            return Ok(());
        }

        // 失败：**不关弹层**（保留可读原因 + 用户可「取消」关闭后重试）。
        if resp.code == ControlCode::AuditUnavailable {
            return self.show_audit_unavailable();
        }
        self.core.countdown_on.set(false);
        self.core.conflict.set(false);
        *self.core.last_reject.borrow_mut() = None;
        let msg = display_safe(resp.message.trim());
        // 回执消息为空 ⇒ **不造假原因**，退到 §3.6 全局行的结果陈述「操作失败」。
        let text = if msg.is_empty() {
            TEXT_TOAST_FAIL.to_string()
        } else {
            msg
        };
        self.core.set_plain_reason(&text, ReasonSlot::Red);
        // **Toast 携带同一份「具体原因」**（① / **IL23**）：就地原因带会被弹层面板压住
        // （实测弹层底 ≈ y607 压掉原因带顶部），而 Toast 在 `layer_top`、弹层**之后**创建
        // ⇒ 同图层里绘在弹层之上，是**无遮挡**的那条通道。空 `message` 时 `text` 已是
        // §3.6 全局行的「操作失败」⇒ 与既有兜底同口径。
        self.core.show_toast(ToastTone::Failure, ICON_FAIL, &text)?;
        if resp.code == ControlCode::RejectedPrecondition {
            self.core.refresh_requested.set(true);
        }
        Ok(())
    }

    /// 取走「请求一次状态刷新」标志（EDGE-19 / 前置条件被拒；**取后即清**）。
    pub fn take_refresh_request(&self) -> bool {
        let v = self.core.refresh_requested.get();
        self.core.refresh_requested.set(false);
        v
    }

    /// 每个事件循环拍调一次：执行**延迟动作**（取消后的弹层关闭 / Toast 过期 / 倒计时推进）。
    ///
    /// 时钟**注入**（`now`）⇒ 离屏可确定性驱动；本页自身不读时钟（**IL14**）。
    pub fn tick(&self, now: Instant) {
        if self.core.pending_close.get() {
            self.core.pending_close.set(false);
            self.core.close_dialog();
        }
        let expired = self
            .core
            .toast
            .borrow()
            .as_ref()
            .is_some_and(|t| t.is_expired(now));
        if expired {
            self.core.close_toast();
        }
        if self.core.countdown_on.get() {
            let base = match self.core.countdown_base.get() {
                Some(b) => b,
                None => {
                    // 首个 tick 取基准（`show_reject` 时刻不可读时钟 —— 本页不读 `Instant::now()`）。
                    self.core.countdown_base.set(Some(now));
                    now
                }
            };
            let elapsed = now.saturating_duration_since(base).as_secs();
            let left = self.core.countdown_remaining.get().saturating_sub(elapsed);
            if left != self.core.countdown_left.get() {
                self.core.countdown_left.set(left);
                self.core.refresh_actions();
            }
        }
    }

    // ── 离屏断言口径（只读；生产侧无此需求）───────────────────────────────

    /// 联锁总态词文本。
    pub fn state_text(&self) -> Option<String> {
        self.core.state_text_l.text()
    }

    /// 联锁总态图标字形（三通道之一）。
    pub fn state_icon_text(&self) -> Option<String> {
        self.core.state_icon.text()
    }

    /// 当前展示态（三态互斥的**结构化**判据）。
    pub fn state_view(&self) -> StateView {
        state_view(&self.core.section.borrow())
    }

    /// 总态词当前应着的色（与 [`state_color`] 同源）。
    pub fn state_color(&self) -> Color {
        state_color(self.state_view())
    }

    /// latch 胶囊文本（当前可见的那一支）。
    pub fn latch_chip_text(&self) -> Option<String> {
        self.core.state_chips[state_index(self.state_view())].text()
    }

    /// latch 胶囊颜色通道（当前可见的那一支）。
    pub fn latch_chip_color(&self) -> Color {
        self.core.state_chips[state_index(self.state_view())].accent()
    }

    /// 屏上**可见**的 latch 胶囊个数（恒为 1 —— 三态互斥的结构性断言口径）。
    pub fn latch_chip_visible_count(&self) -> usize {
        self.core
            .state_chips
            .iter()
            .filter(|c| !c.obj().is_hidden())
            .count()
    }

    /// 触发源卡卡头文本（含数量）。
    pub fn sources_title_text(&self) -> Option<String> {
        self.core.source_title.text()
    }

    /// 第 `i` 行的源名（上屏名）。
    pub fn source_row_name(&self, i: usize) -> Option<String> {
        self.core.source_rows.get(i).and_then(|r| r.shown_name())
    }

    /// 第 `i` 行的状态词（文字信道）。
    pub fn source_row_status(&self, i: usize) -> Option<String> {
        self.core.source_rows.get(i).and_then(|r| r.status.text())
    }

    /// 第 `i` 行的可见形态：`Some(true)` = 已触发形态（`●` 红实心）/ `Some(false)` = 未触发形态
    /// （`○` 灰空心）/ `None` = 整行隐藏或该行未建。
    pub fn source_row_tripped(&self, i: usize) -> Option<bool> {
        let r = self.core.source_rows.get(i)?;
        if !r.visible() {
            return None;
        }
        r.tripped_visible()
    }

    /// 第 `i` 行当前可见形态的灯色通道。
    pub fn source_row_color(&self, i: usize) -> Option<Color> {
        self.core.source_rows.get(i)?.visible_color()
    }

    /// 可见行数。
    pub fn source_rows_visible(&self) -> usize {
        self.core.source_rows.iter().filter(|r| r.visible()).count()
    }

    /// 触发源空态文本。
    pub fn source_empty_text(&self) -> Option<String> {
        self.core.source_empty.text()
    }

    /// 触发源空态是否可见。
    pub fn source_empty_visible(&self) -> bool {
        !self.core.source_empty.obj().is_hidden()
    }

    /// 触发源**不可用态**是否可见（§8.3：`available = false` ⇒ 与空态**互斥**地显示不可用态）。
    pub fn source_unavailable_visible(&self) -> bool {
        !self.core.source_unavailable.obj().is_hidden()
    }

    /// 触发源不可用态的标题（= `UnavailableKind::Interlock.title()`；`available = false` 时屏上
    /// **不得**出现「当前无联锁触发源」）。
    pub fn source_unavailable_title(&self) -> Option<String> {
        self.core.source_unavailable.title()
    }

    /// 触发源卡当前外缘高（随源数 / 不可用态自适应）。
    ///
    /// ⚠️ 读的是 LVGL 的 `coords.height` ⇒ **须有一次布局**才是当前值（离屏用例读它之前先
    /// `disp.refr_now_for_test()`；生产侧由渲染循环自然保证）。
    pub fn source_card_height(&self) -> i32 {
        self.core.source_card.size().1
    }

    /// 空态对象（**实测高的漂移锁**口径：与 [`EMPTY_H`] 对齐）。
    pub fn source_empty_obj(&self) -> &Obj {
        self.core.source_empty.obj()
    }

    /// 不可用态对象（**实测高的漂移锁**口径：与 [`UNAVAILABLE_H`] 对齐）。
    pub fn source_unavailable_obj(&self) -> &Obj {
        self.core.source_unavailable.obj()
    }

    /// 第 `i` 张状态卡的卡头文本。
    pub fn lamp_head_text(&self, i: usize) -> Option<String> {
        self.core.lamps.get(i).and_then(|c| c.head.text())
    }

    /// 第 `i` 张状态卡的值文本。
    pub fn lamp_value_text(&self, i: usize) -> Option<String> {
        self.core.lamps.get(i).and_then(|c| c.value.text())
    }

    /// 停机失败卡值文本（`✓ 正常` / `× 停机失败` / 不可用时同「`? 联锁状态不可用`」）。
    pub fn stop_card_text(&self) -> Option<String> {
        self.lamp_value_text(0)
    }

    /// 故障灯卡值文本。
    pub fn fault_lamp_text(&self) -> Option<String> {
        self.lamp_value_text(1)
    }

    /// 运行灯卡值文本。
    pub fn run_lamp_text(&self) -> Option<String> {
        self.lamp_value_text(2)
    }

    /// 人工释放联锁按钮。
    pub fn release_button(&self) -> &TextButton {
        &self.core.release
    }

    /// M1 授权重启按钮。
    pub fn restart_button(&self) -> &TextButton {
        &self.core.restart
    }

    /// 人工释放联锁按钮是否禁用（读自 LVGL 状态位）。
    pub fn release_disabled(&self) -> bool {
        widgets::has_state(
            self.core.release.button().obj(),
            crate::lvgl::style::State::DISABLED,
        )
    }

    /// M1 授权重启按钮是否禁用（读自 LVGL 状态位）。
    pub fn restart_disabled(&self) -> bool {
        widgets::has_state(
            self.core.restart.button().obj(),
            crate::lvgl::style::State::DISABLED,
        )
    }

    /// 就地原因**左槽**文本（即使当前不可见也可读回）。
    pub fn reason_left_text(&self) -> Option<String> {
        self.core.reason_left.text()
    }

    /// 就地原因左槽是否可见。
    pub fn reason_left_visible(&self) -> bool {
        !self.core.reason_left.obj().is_hidden()
    }

    /// 就地原因**右槽**文本（M1 按钮正上方）。
    pub fn reason_right_text(&self) -> Option<String> {
        self.core.reason_right.text()
    }

    /// 就地原因右槽是否可见。
    pub fn reason_right_visible(&self) -> bool {
        !self.core.reason_right.obj().is_hidden()
    }

    /// 操作条右端弱注文本。
    pub fn audit_note_text(&self) -> Option<String> {
        self.core.audit_note.text()
    }

    /// 当前 Toast 文案。
    pub fn toast_text(&self) -> Option<String> {
        self.core.toast.borrow().as_ref().and_then(|t| t.text())
    }

    /// 当前 Toast 语义。
    pub fn toast_tone(&self) -> Option<ToastTone> {
        self.core.toast.borrow().as_ref().map(|t| t.tone())
    }

    /// 最近一次**结构化**拒绝（`HoldNotElapsed` 走倒计时槽 ⇒ 此处为 `None`）。
    pub fn last_reject(&self) -> Option<InterlockReject> {
        self.core.last_reject.borrow().clone()
    }

    /// 当前是否在显 EDGE-19 冲突文案。
    pub fn conflict_visible(&self) -> bool {
        self.core.conflict.get()
    }

    /// 当前倒计时剩余秒数（无倒计时 ⇒ `None`）。
    pub fn countdown_secs(&self) -> Option<u64> {
        self.core
            .countdown_on
            .get()
            .then(|| self.core.countdown_left.get())
    }

    /// **仅测试**：是否曾注入过**有效**帧（`available` 曾为 true）—— 断言口径，**不参与任何
    /// 判据**（M1：与 `with_dialog` 同一纪律，`#[cfg(test)]` 门控）。
    #[cfg(test)]
    pub(crate) fn ever_available(&self) -> bool {
        self.core.ever_available.get()
    }

    /// **生产可见的「确认弹层是否打开」查询口**（B3-2c，闭合 TT-13；与
    /// [`p2_config::P2ConfigPage::dialog_open`] **同款**）。
    ///
    /// `Shell::set_modal_open`（弹层打开 ⇒ 暂停空闲计时 / 不显示倒计时 / 不强制切页，
    /// UI §4.3）是 B2c-3 起的已登记契约，而当时页面侧**没有任何生产可见的查询口**
    /// （[`P4InterlockPage::with_dialog`] 是 `#[cfg(test)]`）⇒ 接线层恒喂 `false`
    /// （`ui/shell.rs` 偏差 **SH2**）。
    ///
    /// **语义**：`true` = 此刻屏上有一个**未关闭**的确认弹层（本页的弹层唯一创建点
    /// = `open_dialog`，关闭点 = `close_dialog`）。⚠️ 关闭**延迟到下一拍**
    /// （`ConfirmDialog::close` 不得在 LVGL 事件回调内调用，见 [`P4InterlockPage::tick`]）
    /// ⇒ 「刚点完取消」的那一瞬间仍为 `true` —— 这正是要的语义（弹层此刻真的还在屏上）。
    pub fn dialog_open(&self) -> bool {
        self.core.dialog.borrow().is_some()
    }

    /// **仅测试**：就地原因**左槽**当前宽度（加宽策略的断言口径，见 **IL23** ③）。
    ///
    /// 装配后须有一次布局（离屏用例先 `refr_now_for_test()`）才是当前值。
    #[cfg(test)]
    pub(crate) fn reason_left_width(&self) -> i32 {
        self.core.reason_left.size().0
    }

    /// **仅测试**：以闭包访问当前弹层（`RefCell` 借用不外泄）。
    #[cfg(test)]
    pub(crate) fn with_dialog<R>(&self, f: impl FnOnce(&ConfirmDialog) -> R) -> Option<R> {
        self.core.dialog.borrow().as_ref().map(f)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. 纯逻辑单测（**不触碰 LVGL** —— LVGL 非线程安全，触碰它的用例只能由
//    `src/lvgl/tests.rs::lvgl_core_bridge_chain` 经 `ui/tests.rs::pages_chain` 串行调起）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_display_proto::InterlockSourceItem;

    /// 造一个联锁段（默认全 false = 契约缺省 = **不可用**）。
    fn sect(available: bool, enabled: bool, latched: bool) -> InterlockSection {
        InterlockSection {
            available,
            enabled,
            latched,
            ..Default::default()
        }
    }

    /// 带两个源（已触发 + 未触发）的联锁段。
    fn sect_with_sources() -> InterlockSection {
        InterlockSection {
            available: true,
            enabled: true,
            latched: true,
            sources: vec![
                InterlockSourceItem {
                    name: "estop".into(),
                    tripped: true,
                },
                InterlockSourceItem {
                    name: "door".into(),
                    tripped: false,
                },
            ],
            ..Default::default()
        }
    }

    // ── ① 三态判定：**`available = false` 绝不回落「未联锁」**（本页最重要的不变量）──
    //
    // 敏感性：把 `state_view` 的 `if !s.available` 分支删掉（或改成先判 `latched`），本条立刻变红
    // （这就是**探针 P2**；fail-closed 的回归锁）。
    #[test]
    fn unavailable_state_never_falls_back_to_unlatched() {
        // 契约缺省（= 帧缺省、= `frame = None` 时页面取的缺省）必须是**不可用**，不是「未联锁」。
        let d = InterlockSection::default();
        assert!(!d.available);
        assert_eq!(state_view(&d), StateView::Unavailable);
        // ⚠️ **IL29①**：96 px 主值槽放不下全串（7 字 × 96 = 672 > `STATE_TEXT_W` 366）⇒
        // 该槽取**缩短形态**；全串仍是就地原因带 / 触发源卡 / latch 胶囊的落点
        // （同源锁见 `unavailable_text_and_icon_share_the_component_source`）。
        assert_eq!(state_text(state_view(&d)), TEXT_STATE_UNAVAILABLE_SHORT);
        assert_ne!(
            TEXT_STATE_UNAVAILABLE_SHORT, TEXT_STATE_UNAVAILABLE,
            "两形态**必须互异** —— 否则「缩短」是空操作，读者会误以为主值槽显的是全串"
        );
        assert_ne!(
            state_text(state_view(&d)),
            TEXT_STATE_UNLATCHED,
            "**不可用 ≠ 未联锁**：无法获知（fail-closed）不得显示为确知安全"
        );
        // 即使 `latched = false`（缺省值）也不得读成「未联锁」。
        let u = sect(false, true, false);
        assert_eq!(state_view(&u), StateView::Unavailable);
        // `latched = true` 但状态源不可用 ⇒ 仍**不可用**（该比特不可信）。
        let u2 = sect(false, true, true);
        assert_eq!(state_view(&u2), StateView::Unavailable);
        // 只有 `available = true` 才谈得上「已 / 未联锁」。
        assert_eq!(state_view(&sect(true, true, true)), StateView::Latched);
        assert_eq!(state_view(&sect(true, true, false)), StateView::Unlatched);
        // `enabled = false` 不影响「状态可得性」（未启用 ≠ 状态不可得）。
        assert_eq!(state_view(&sect(true, false, false)), StateView::Unlatched);
    }

    // ── ② 三态词 / 图标 / 色：`unavailable` 复用的是「问号」而不是 `✓` / `⚠`（§8.3 明文）──
    //
    // 敏感性：把 `state_icon(Unavailable)` 改成 `ICON_STATE_LATCHED`（`⚠`）或
    // `ICON_STATE_UNLATCHED`（`✓`）⇒ 本条变红。
    #[test]
    fn three_states_are_visually_distinguishable() {
        use crate::ui::theme::Palette as P;
        let all = STATE_VIEW_ORDER;
        // 词 / 图标 / 色三通道**两两互异**（§8.3：「视觉 / 文案 / 图标三通道均须可区分」）。
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(state_text(all[i]), state_text(all[j]));
                assert_ne!(state_icon(all[i]), state_icon(all[j]));
                assert_ne!(state_color(all[i]), state_color(all[j]));
            }
        }
        assert_eq!(state_color(StateView::Latched), P::DANGER);
        assert_eq!(state_color(StateView::Unlatched), P::OK);
        assert_eq!(state_color(StateView::Unavailable), P::STOPPED);
        // `?` / `✓` / `⚠` 三角色分明（并且不可用**不是**前两者中的任何一个）。
        assert_ne!(state_icon(StateView::Unavailable), ICON_STATE_UNLATCHED);
        assert_ne!(state_icon(StateView::Unavailable), ICON_STATE_LATCHED);
    }

    /// **同源锁**：本页的「联锁状态不可用」文案 / 图标与 `components.rs` 的
    /// [`UnavailableKind::Interlock`] **逐字相等**（**无第二份真源**）。
    ///
    /// 敏感性：改动本页任一字面量（或组件侧的 `title()` / `icon()`）⇒ 本条变红。
    #[test]
    fn unavailable_text_and_icon_share_the_component_source() {
        let kind = UnavailableKind::Interlock;
        assert_eq!(TEXT_STATE_UNAVAILABLE, kind.title());
        assert_eq!(ICON_STATE_UNAVAILABLE, kind.icon());
        assert_eq!(gray_color(), kind.accent());
        assert!(!kind.means_nothing_happened());
    }

    // ── ③ latch 胶囊：`available = false` 时**不得**显示「未保持」（§8.3 明文）──
    //
    // 敏感性：把 `latch_chip` 的 `Unavailable` 分支改成返回 `TEXT_LATCH_UNHELD` ⇒ 本条变红。
    #[test]
    fn latch_chip_never_says_unheld_when_unavailable() {
        let (idx, text, skin) = latch_chip(StateView::Unavailable);
        assert_eq!(idx, 2);
        assert_eq!(text, TEXT_STATE_UNAVAILABLE);
        assert_ne!(text, TEXT_LATCH_UNHELD);
        assert_eq!(skin, ChipSkin::UNAVAILABLE);
        // 已 / 未保持两态的槽位与皮肤（UI §6.4 逐条给值）。
        let (i0, t0, s0) = latch_chip(StateView::Latched);
        let (i1, t1, s1) = latch_chip(StateView::Unlatched);
        assert_eq!((i0, t0, s0), (0, TEXT_LATCH_HELD, ChipSkin::WARNING));
        assert_eq!((i1, t1, s1), (1, TEXT_LATCH_UNHELD, ChipSkin::NEUTRAL));
        // 三态互斥 + 槽位与 `STATE_VIEW_ORDER` 一致（构造期数组序 = 运行期下标）。
        for (i, v) in STATE_VIEW_ORDER.iter().enumerate() {
            assert_eq!(latch_chip(*v).0, i, "槽位序必须与 STATE_VIEW_ORDER 一致");
            assert_eq!(state_index(*v), i);
        }
        // 不可用态的胶囊图标也不得复用 `✓` / `⚠`。
        assert_eq!(latch_chip_icon(StateView::Unavailable), ICON_STATE_UNAVAILABLE);
        assert_ne!(latch_chip_icon(StateView::Unavailable), ICON_STATE_UNLATCHED);
    }

    // ── ④ 按钮可用性矩阵（UI §6.4「操作与拒绝原因」表逐行）──
    //
    // 敏感性：把 `op_state` 里 `!available` 的分支去掉 ⇒ 「两按钮均 disabled」两条变红；
    // 给 `M1 授权重启` **加回** `stop_failed` 的本地预判 ⇒「`stop_failed` 不置灰 M1」那条变红
    //（B2b-3 评审整改 ① 的回归锁，探针实测输出见整改报告）。
    #[test]
    fn button_matrix_follows_reject_table() {
        // 正常（未联锁、无停机失败）⇒ 两按钮皆可用、无就地原因。
        let ok = op_state(&sect(true, true, false), false);
        assert!(!ok.release_disabled && !ok.restart_disabled);
        assert_eq!(ok.release_reason, None);
        assert_eq!(ok.restart_reason, None);

        // §8.3 联锁专行：`available = false` ⇒ **两按钮均 disabled** + 就地「联锁状态不可用」。
        let na = op_state(&sect(false, true, true), false);
        assert!(na.release_disabled && na.restart_disabled);
        assert_eq!(na.release_reason, Some(TEXT_STATE_UNAVAILABLE));
        assert_eq!(na.restart_reason, Some(TEXT_STATE_UNAVAILABLE));

        // §6.4「联锁未启用」行 ⇒ 两按钮 disabled + 说明「联锁未启用」。
        let off = op_state(&sect(true, false, false), false);
        assert!(off.release_disabled && off.restart_disabled);
        assert_eq!(off.release_reason, Some(TEXT_NOT_ENABLED));
        assert_eq!(off.restart_reason, Some(TEXT_NOT_ENABLED));

        // §6.4「M1 处于 latch 态」行 ⇒ **只** M1 禁用 + **按钮正上方**原因；释放仍可用（IL18）。
        let la = op_state(&sect(true, true, true), false);
        assert!(!la.release_disabled, "释放是安全正向操作，不得因 latch 置灰（IL18）");
        assert_eq!(la.release_reason, None);
        assert!(la.restart_disabled);
        assert_eq!(la.restart_reason, Some(TEXT_REASON_LATCHED));

        // `stop_failed` **不**参与按钮可用性（B2b-3 评审整改 ①）：§6.4 拒绝原因表 / §9 F18
        // **只**要求 latch 态置灰 M1，**没有** `stop_failed` 这一行；本地预判「停机未确认」会替代
        // 后端 `RejectedPrecondition[StopPending]` 的**具体**原因（现场只看到灰按钮）⇒
        // 两按钮均可用、**无就地原因**，让后端的具体原因可达（与 IL18 对 `release` 同一取向）。
        let mut sf = sect(true, true, false);
        sf.stop_failed = true;
        let sf = op_state(&sf, false);
        assert!(!sf.release_disabled);
        assert!(!sf.restart_disabled, "`stop_failed` 不得本地预判置灰 M1（评审整改 ①）");
        assert_eq!(
            sf.restart_reason, None,
            "`stop_failed` 不得产出本地原因（评审整改 ①）"
        );
        // 但 `stop_failed` 的**屏显**照旧（整改 ① 只移除「按钮级本地预判」，不动状态卡）。
        assert_eq!(stop_view(true), (TEXT_STOP_FAIL, 1));

        // 提交中 ⇒ 两按钮 disabled，且**无**就地文案（IL15：不得造 §3.6 没有的文案）。
        let busy = op_state(&sect(true, true, false), true);
        assert!(busy.release_disabled && busy.restart_disabled);
        assert_eq!(busy.release_reason, None);
        assert_eq!(busy.restart_reason, None);

        // 全局阻塞优先于 M1 专属阻塞（原因**不得**被后者的文案顶掉）。
        let both = op_state(&sect(false, true, true), false);
        assert_eq!(both.restart_reason, Some(TEXT_STATE_UNAVAILABLE));
    }

    // ── ⑤ 三态标志：`None` ⇒ 未知（**绝不**臆造为「灯灭」）──
    //
    // 敏感性：把 `tri_view(None)` 改成 `TriView::Off` ⇒ 本条与 ⑤′ 一起变红。
    #[test]
    fn none_means_unknown_not_off() {
        assert_eq!(tri_view(None), TriView::Unknown);
        assert_eq!(tri_view(Some(true)), TriView::On);
        assert_eq!(tri_view(Some(false)), TriView::Off);
        let (t, slot) = lamp_view(tri_view(None));
        assert_eq!(t, TEXT_LAMP_UNKNOWN);
        assert_ne!(t, TEXT_LAMP_OFF, "灯态未知 ≠ 灯灭（契约 Option<bool> 的三态语义）");
        assert_eq!(slot, 2, "未知与不可用同走灰档（UI §6.4 色值）");
        // 亮 / 灭两态的文案与色（故障灯红 `#DC3545`、运行灯绿 `#28A745`、灭灰 `#5F6368`）。
        assert_eq!(lamp_view(TriView::On), (TEXT_LAMP_ON, 0));
        assert_eq!(lamp_view(TriView::Off), (TEXT_LAMP_OFF, 1));
        assert_eq!(lamp_on_color(true), Palette::LINK_DOWN);
        assert_eq!(lamp_on_color(false), Palette::LINK_OK);
        assert_eq!(lamp_off_color(), Palette::LINK_UNCONFIGURED);
        // 停机失败卡：`stop_failed = false` ⇒ `✓ 正常`（绿）；`true` ⇒ `× 停机失败`（红）。
        assert_eq!(stop_view(false), (TEXT_STOP_OK, 0));
        assert_eq!(stop_view(true), (TEXT_STOP_FAIL, 1));
        assert!(!TEXT_STOP_OK.contains(ICON_STATE_LATCHED), "正常态不得复用 `⚠`");
    }

    // ── ⑥ 源名映射（含**未知名**的处置）──
    //
    // 敏感性：把 `source_label` 的映射表删掉（改成恒 `display_safe`）⇒ 前两条变红；
    // 把它改成「未知名一律返回中文占位」⇒ 第 ③ 条（保留可辨认原文）变红。
    #[test]
    fn source_label_maps_known_and_keeps_unknown_ascii_safe() {
        // ① 已登记的两条（UI §3.6 P4「触发源」行逐字）。
        assert_eq!(source_label("estop"), TEXT_SRC_ESTOP);
        assert_eq!(source_label("door"), TEXT_SRC_DOOR);
        // ①′ **ASCII 大小写不敏感**（M2 / IL26）：`ESTOP` / `Door` 也是「急停 / 门禁」，
        // 不得因大小写之差落成「`E–sTOP`」这类机器名上屏（`_` 还会被 `display_safe` 改写成
        // 短破折）。敏感性：把 `eq_ignore_ascii_case` 改回 `==` ⇒ 本条两条立刻变红。
        assert_eq!(source_label("ESTOP"), TEXT_SRC_ESTOP, "大写机器名仍须映射（M2/IL26）");
        assert_eq!(source_label("Door"), TEXT_SRC_DOOR, "混合大小写仍须映射（M2/IL26）");
        // ①″ 残余（如实）：**分隔符变体**（`e_stop` / `e-stop`）**不在**映射表内 —— 大小写
        // 不敏感只管大小写，`_` 仍会落成 `E–STOP`（`_` → 短破折）。本页**不**猜。
        assert_eq!(source_label("e_stop"), display_safe("e_stop"));
        assert_ne!(source_label("e_stop"), TEXT_SRC_ESTOP);
        // ② 未登记名 ⇒ **原样保留可辨认的机器名**（经 `display_safe`），**不伪造中文名**。
        for raw in ["flood", "fire", "unknown_src", "di3"] {
            let shown = source_label(raw);
            assert!(!shown.is_empty(), "{raw} 的上屏名不得为空");
            assert_ne!(shown, TEXT_SRC_ESTOP, "{raw} 不得被冒名成「急停」");
            assert_ne!(shown, TEXT_SRC_DOOR);
            assert_eq!(shown, display_safe(raw), "未登记名必须与 display_safe 同口径（IL6）");
        }
        // ③ `display_safe` 的产出 ⊆ cmap 安全字母表（ASCII 侧）—— 未知名的**豆腐块防线**。
        for raw in ["flood", "no_such_source", "ABC", "di-3"] {
            for c in source_label(raw).chars() {
                if c.is_ascii() {
                    assert!(
                        crate::ui::pages::ASCII_DISPLAY_ALPHABET.contains(c),
                        "{raw} 上屏后出现 cmap 外 ASCII `{c}`（真机豆腐块）"
                    );
                }
            }
        }
        // ④ 非 ASCII（后端直接给中文名）⇒ 原样透传（含缺字时的残余风险见 IL6 / PD13）。
        assert_eq!(source_label(TEXT_SRC_ESTOP), TEXT_SRC_ESTOP);
        // ⑤ 映射表与状态词表的口径：已触发 / 未触发（零歧义）。
        assert_eq!(source_status_text(true), TEXT_SRC_TRIPPED);
        assert_eq!(source_status_text(false), TEXT_SRC_UNTRIPPED);
        // ⑥ 行池 = 后端 distinct token 全集（estop / flood / fire / door = 4，IL9）。
        assert_eq!(SOURCE_ROW_POOL, 4);
    }

    // ── ⑦ `InterlockReject` **7 个变体**各自的就地文案（EDGE-12：不得静默失败）──
    //
    // 敏感性（**探针 P4**）：把任一变体的分支改成空串或泛化文案 ⇒ 对应断言立刻变红
    // （每条断言都钉了**该变体独有**的具体成分，而不是"非空"这种弱判据）。
    #[test]
    fn every_reject_variant_has_specific_inplace_text() {
        // ① 触发源未复位 ⇒ **列出具体源名**（且经中文映射，不是机器名）。
        let t = reject_text(&InterlockReject::SourcesNotReset {
            remaining: vec!["estop".into(), "door".into()],
        });
        assert!(t.starts_with(TEXT_REJECT_SOURCES));
        assert!(t.contains(TEXT_SRC_ESTOP) && t.contains(TEXT_SRC_DOOR), "实际: {t}");
        assert!(!t.contains("estop"), "不得上屏机器名（缺字形 + 现场不可读）: {t}");
        assert_eq!(t, "触发源未复位 · 急停/门禁");
        // 空 `remaining`（契约合法）⇒ 只报能确定的事实，不编造源名。
        assert_eq!(
            reject_text(&InterlockReject::SourcesNotReset { remaining: vec![] }),
            TEXT_REJECT_SOURCES
        );

        // ② 保持时间不足 ⇒ 同时给出**剩余**与**须保持**秒数。
        let t = reject_text(&InterlockReject::HoldNotElapsed {
            need_secs: 30,
            remaining_secs: 12,
        });
        assert!(t.contains("12") && t.contains("30"), "实际: {t}");
        assert!(t.contains(TEXT_REJECT_HOLD));

        // ③ 处于 latch 态 ⇒ 与就地置灰原因**同一口径**（同一句话，不两说）。
        assert_eq!(reject_text(&InterlockReject::Latched), TEXT_REASON_LATCHED);

        // ④ 停机未确认。
        let t = reject_text(&InterlockReject::StopPending);
        assert!(t.starts_with(TEXT_STOP_PENDING));
        assert!(t.contains(TEXT_STOP_PENDING_TRAIL));

        // ⑤ 联锁未启用。
        assert_eq!(reject_text(&InterlockReject::NotEnabled), TEXT_NOT_ENABLED);

        // ⑥ 上一操作仍在处理中。
        assert_eq!(reject_text(&InterlockReject::Busy), TEXT_OP_BUSY);

        // ⑦ 内部错误 ⇒ **透传具体原因**（不吞），且经 `display_safe`。
        let t = reject_text(&InterlockReject::Internal("io-3".into()));
        assert!(t.starts_with(TEXT_INTERNAL));
        assert!(t.contains(&display_safe("io-3")), "实际: {t}");

        // ⑧ 交叉：7 个变体的文案**两两不同**（防"泛化文案"把多个变体糊成一句）。
        let all = [
            reject_text(&InterlockReject::SourcesNotReset {
                remaining: vec!["estop".into()],
            }),
            reject_text(&InterlockReject::HoldNotElapsed {
                need_secs: 30,
                remaining_secs: 12,
            }),
            reject_text(&InterlockReject::Latched),
            reject_text(&InterlockReject::StopPending),
            reject_text(&InterlockReject::NotEnabled),
            reject_text(&InterlockReject::Busy),
            reject_text(&InterlockReject::Internal("io-3".into())),
        ];
        assert_eq!(all.len(), 7, "全部 7 个变体均须有对应用例（设计 §11.3）");
        for i in 0..all.len() {
            assert!(!all[i].is_empty(), "变体 {i} 文案为空 = 静默失败");
            for j in (i + 1)..all.len() {
                assert_ne!(all[i], all[j], "变体 {i} 与 {j} 的文案被糊成同一句");
            }
        }
    }

    /// 倒计时文案（UI §6.4「保持时间不足」行）。
    ///
    /// 敏感性：把 `hold_text` 改成不含秒数 ⇒ 本条 + ⑨ 变红。
    #[test]
    fn hold_countdown_text_carries_seconds() {
        assert_eq!(hold_text(12), "保持时间不足 · 还需 12 秒");
        assert_eq!(hold_text(0), "保持时间不足 · 还需 0 秒");
        assert!(hold_text(7).contains(TEXT_HOLD_MORE));
    }

    // ── ⑧ 提交载荷：**机器名（不是中文标签）** + 全部源（IL16）──
    //
    // 敏感性：把 `op_payload` 的 `observed_sources` 改成 `source_label(..)` 的产物 ⇒ 本条变红
    // （后端按 token 比对，中文名会让并发检查恒不匹配）。
    #[test]
    fn payload_carries_machine_names_and_observed_latch() {
        let s = sect_with_sources();
        let p = op_payload(&s);
        assert!(p.observed_latched, "观测到的 latch 态");
        assert_eq!(p.observed_sources, vec!["estop".to_string(), "door".to_string()]);
        assert!(
            !p.observed_sources.iter().any(|n| n == TEXT_SRC_ESTOP),
            "载荷必须是**机器名**，不得是上屏中文标签"
        );
        // 未触发源**也在**列表里（IL16：全量名列表才是更严的并发判据）。
        assert_eq!(p.observed_sources.len(), s.sources.len());
        // 缺省段 ⇒ 空列表 + `false`（与契约缺省同口径）。
        let d = op_payload(&InterlockSection::default());
        assert!(!d.observed_latched && d.observed_sources.is_empty());
    }

    /// 源名列表上屏文本（弹层明细 / 无源 ⇒ `无`）。
    #[test]
    fn sources_text_maps_and_handles_empty() {
        assert_eq!(sources_text(&sect_with_sources()), "急停/门禁");
        let mut empty = InterlockSection::default();
        empty.sources.clear();
        assert_eq!(sources_text(&empty), TEXT_NONE);
    }

    /// 卡头含数量（IL-01.2：`触发源 · N`；全角括号缺字见 IL1）。
    ///
    /// 敏感性：把 `sources_title` 里的数量去掉 ⇒ 本条变红。
    #[test]
    fn sources_title_carries_count() {
        assert_eq!(sources_title(0), "触发源 · 0");
        assert_eq!(sources_title(2), "触发源 · 2");
        assert!(sources_title(11).contains("11"));
    }

    /// 触发源**超出行池上限**的可见提示（B2b-3 代码质量整改 ②；IL9 的补偿）。
    ///
    /// 敏感性：把 `sources_overflow_note` 改成恒返回空串（= 去掉提示）⇒ 本条后两条立刻变红。
    #[test]
    fn sources_overflow_is_visibly_stated() {
        // 未超出 ⇒ **无提示**（卡头逐字不变，`触发源 · N`）。
        for n in 0..=SOURCE_ROW_POOL {
            assert_eq!(
                sources_overflow_note(n),
                "",
                "{n} 源未超上限 ⇒ 不得多出提示（卡头逐字 = `触发源 · N`）"
            );
        }
        // 超出 ⇒ **可见地**说出差额（UI §6.4 `：599`「全部源一次性列出，不折叠」的补偿）。
        assert_eq!(
            sources_overflow_note(SOURCE_ROW_POOL + 1),
            " · 还有 1 条",
            "5 源 ⇒ 第 5 条必须**在屏上**被说出来（不得静默）"
        );
        assert!(sources_overflow_note(SOURCE_ROW_POOL + 3).contains('3'), "7 源 ⇒ 差额 3");
        // 提示词与数字都在 cmap 内（`还`/`有`/`条` 逐字实测在 `fonts/lv_font_cmap.txt`）——
        // 逐字核对由 `runtime_texts_are_cmap_safe`（产出串 ⊆ 安全字母表）+ 码表网承担。
        // 行池上限**未被改动**（本处置是补偿，不是扩容）。
        assert_eq!(SOURCE_ROW_POOL, 4, "上限不动（IL9）：补偿 = 可见提示");
    }

    /// 陈旧倒计时的清理判据（B2b-3 代码质量整改 M3；IL27）。
    ///
    /// 敏感性：把 [`section_display_eq`] 改成恒 `true`（= 永不清）或恒 `false`（= 每帧清）
    /// ⇒ 本条前两条立刻变红。
    #[test]
    fn section_display_eq_ignores_only_ts_ms() {
        let mut a = sect(true, true, true);
        a.ts_ms = 1_000;
        let mut b = a.clone();
        b.ts_ms = 2_000;
        assert!(
            section_display_eq(&a, &b),
            "**仅 `ts_ms` 变** ⇒ 视作同一帧（否则 1 Hz 心跳每秒清掉倒计时，IL14 形同虚设）"
        );
        // 任一**展示相关**字段变 ⇒ 不等于（这些变化都可能使倒计时基准失效）。
        for mut c in [
            sect(false, true, true),
            sect(true, false, true),
            sect(true, true, false),
        ] {
            c.ts_ms = 1_000;
            assert!(!section_display_eq(&a, &c), "展示相关字段变了 ⇒ 必须判为「新帧」");
        }
        let mut sf = a.clone();
        sf.stop_failed = true;
        assert!(!section_display_eq(&a, &sf));
        let mut src = a.clone();
        src.sources = vec![mupc_display_proto::InterlockSourceItem {
            name: "estop".into(),
            tripped: true,
        }];
        assert!(!section_display_eq(&a, &src));
        let mut lamp = a.clone();
        lamp.fault_lamp = Some(true);
        assert!(!section_display_eq(&a, &lamp));
        let mut hold = a.clone();
        hold.release_hold_secs = 30;
        assert!(!section_display_eq(&a, &hold));
    }

    // ── ⑨ 弹层明细（UI §6.4 强确认弹层段的「明细列出…」三项）──
    //
    // 敏感性：把 `dialog_details` 的任一行删掉 / 字段名写错 ⇒ 本条变红。
    #[test]
    fn dialog_details_list_three_rows() {
        let s = sect_with_sources(); // latched = true、有 2 个源、无停机失败
        let d = dialog_details(&s, OpKind::Release);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].field, TEXT_DETAIL_SOURCES);
        assert_eq!(d[0].before, "急停/门禁");
        assert_eq!(d[1].field, TEXT_DETAIL_LATCH);
        assert_eq!(d[1].before, TEXT_LATCH_HELD, "当前已保持");
        assert_eq!(d[1].after, TEXT_LATCH_UNHELD, "释放后目标态 = 未保持");
        assert_eq!(d[2].field, TEXT_DETAIL_STOP);
        assert_eq!(d[2].before, d[2].after, "本操作不改动停机确认态 ⇒ 如实表达为不变");
        // 释放的目标态：触发源「无」（释放的**前提**就是全部复位）。
        assert_eq!(d[0].after, TEXT_NONE);

        let m = dialog_details(&s, OpKind::AckM1);
        assert_eq!(m.len(), 3);
        assert_eq!(m[2].after, TEXT_STOP_OK, "授权后停机确认态成立 ⇒ 目标「正常」");
        assert_eq!(m[0].before, m[0].after, "M1 不改动触发源 ⇒ 前后同值");
        // 弹层标题 / 影响范围（L2 必须为**具体副作用**，§7.3）。
        assert_eq!(OpKind::Release.dialog_title(), TEXT_DIALOG_TITLE_RELEASE);
        assert_eq!(OpKind::AckM1.dialog_title(), TEXT_DIALOG_TITLE_ACK_M1);
        assert_ne!(OpKind::Release.impact(), OpKind::AckM1.impact());
        for op in [OpKind::Release, OpKind::AckM1] {
            assert!(op.impact().contains("装置"), "影响范围必须具体（含「装置」这一副作用对象）");
        }
    }

    // ── ⑩ 上屏文案的**结构性自检**（码表网的姊妹网：这一条扫的是**函数产出**，不是字面量）──
    //
    // 码表网（`ui_texts_covered_by_font_cmap`）只扫**源码字面量**；`format!` 的产出是运行时的。
    // 本页的产出串全部由**字面量 + 数字**拼成 ⇒ 这里断言「产出 ⊆ cmap 安全字母表（ASCII 侧）」，
    // 把"运行时新引入 cmap 外 ASCII"这条缺口一并堵上（与 `runtime_formatters_emit_only_cmap_glyphs`
    // 同一手法）。
    //
    // 敏感性：把 `sources_title` 改成 `format!("触发源 ({n})")`（ASCII 圆括号 ⇒ cmap 外）
    // ⇒ 本条立刻变红（这正是 IL1 登记的那类缺字）。
    #[test]
    fn runtime_texts_are_cmap_safe() {
        let samples: Vec<String> = vec![
            sources_title(0),
            sources_title(3),
            sources_overflow_note(0),
            sources_overflow_note(SOURCE_ROW_POOL + 1),
            sources_overflow_note(SOURCE_ROW_POOL + 12),
            hold_text(0),
            hold_text(12),
            source_label("estop"),
            source_label("flood"),
            source_label("no_such_src"),
            source_status_text(true).to_string(),
            source_status_text(false).to_string(),
            reject_text(&InterlockReject::SourcesNotReset {
                remaining: vec!["estop".into(), "door".into(), "flood".into()],
            }),
            reject_text(&InterlockReject::HoldNotElapsed {
                need_secs: 30,
                remaining_secs: 12,
            }),
            reject_text(&InterlockReject::Latched),
            reject_text(&InterlockReject::StopPending),
            reject_text(&InterlockReject::NotEnabled),
            reject_text(&InterlockReject::Busy),
            reject_text(&InterlockReject::Internal("io-3".into())),
            sources_text(&sect_with_sources()),
        ]
        .into_iter()
        .chain(ALL_TEXTS.iter().map(|t| (*t).to_string()))
        .collect();
        assert!(samples.len() > 60, "样本量过小 ⇒ 本网形同虚设");
        for s in &samples {
            for c in s.chars() {
                if c.is_ascii() {
                    assert!(
                        crate::ui::pages::ASCII_DISPLAY_ALPHABET.contains(c),
                        "运行时文案 `{s}` 含 cmap 外 ASCII `{c}`（真机豆腐块）"
                    );
                }
            }
            // 固定文案还须**逐字**在 `display_safe` 下不变（= 全 ASCII 都在安全字母表内）。
            assert_eq!(&display_safe(s), s, "文案 `{s}` 经 display_safe 被改写 ⇒ 含 cmap 外字符");
        }
    }

    /// `ALL_TEXTS` 清册自检：非空、每条非空、规模合理（防"清册腐化"）。
    ///
    /// ⚠️ **不禁止"值相同"**：不同**语义槽位**合法地共用同一串 —— 已实测两处：
    /// ① [`ICON_STATE_UNLATCHED`] 与 [`ICON_OK`] 同为 `<✓>`（总态图标 / Toast 成功图标，
    /// 后者是从 `p2_config` **转出**的，本文件**无**第二份字面量）；② [`TEXT_DETAIL_STOP`]
    /// 与 [`TEXT_CARD_STOP`] 同为「停机失败」（明细字段名 = 卡头名，前者已是后者的**转出**）。
    /// 本用例只钉"清册确实写全、没写空"，**重复串的判据是"是否有第二份字面量"**，由
    /// `ui_texts_covered_by_font_cmap` 的「逐字出现在源码字面量里」那条覆盖。
    #[test]
    fn all_texts_manifest_is_sane() {
        assert!(ALL_TEXTS.len() > 50, "清册条目过少 ⇒ 大概率漏登记");
        for t in ALL_TEXTS {
            assert!(!t.is_empty(), "清册不得有空条目");
        }
        // 清册里**不得**出现只有 1 个字符的 ASCII 之外的可疑项（防误把占位符写进来）。
        assert!(ALL_TEXTS.iter().all(|t| !t.starts_with("--")));
    }
}
