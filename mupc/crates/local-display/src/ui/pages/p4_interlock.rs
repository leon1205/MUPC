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
//! | IL9 | 触发源**行池 = 4**（`SOURCE_ROW_POOL`）；帧内源数 > 4（契约 `sources: Vec<_>` **未设上限**）⇒ **卡头数量仍显真实 N**、行只铺 4 条 | 后端 distinct token 全集恰为 4：`estop` / `flood` / `fire` / `door`（`mupc-core-bin/src/interlock.rs::source_token()`）；行池固定 ⇒ `new()` 一次性建齐、`render()` **不可失败**（同 P1 的 `alarm_rows` 池口径）且 1 Hz **零对象 churn**。「数量 ≠ 行数」这一差异在屏上**可见**（不静默） | 无（**有意**）；若后端 token 集合扩张，须同步扩 `SOURCE_ROW_POOL` 并在 §3.6 补中文名 |
//! | IL10 | 弹层明细取 [`ConfirmDialog`] 的**三段式** `字段 旧值 → 新值`（组件本批禁改，**无** `字段：值` 形态）⇒ §6.4 要求的三项信息按「**当前 → 操作后目标态**」表达；本次操作**不改动**的项取 `新值 = 旧值`（**如实**表达「不变」，不编造目标值） | `ConfirmDialog` 的明细行**恒**渲染四槽（字段 / 旧值 / 箭头 / 新值，颜色亦固定），没有「单值行」口。三段式是本批唯一可行形态；语义仍**具体**（非泛化措辞，满足 §7.3「影响范围 / 明细必须具体」） | `components.rs` 收口批：明细行支持「单值」形态后逐字回契约 |
//! | IL11 | **弹层内**就地红字（UI §6.4 拒绝原因表的「弹层内就地显示」）**不可达** ⇒ 取 `p2_config.rs` **PD7 同款口径**：原因落**页内就地原因带**（红字 24 px `#FF6B6B`），**弹层不自动关闭**（原因常驻可读，用户可「取消」关闭后重试） | 同上：`ConfirmDialog` 的「影响范围」与明细在**构造期固定**，**没有**可变错误文案口；且关闭弹层不得在 LVGL 事件回调内做 | `components.rs` 补 `set_error()` 后改回弹层内 |
//! | IL12 | **控制通道线上路径**：失败时的**具体原因**由 `ControlResponse.message` 承担 —— 契约 `display-proto/src/control.rs` 该字段文档原文：「**人读消息；UI 直接展示（失败时即 EDGE-10 / EDGE-12 要求的「具体原因」）**」⇒ [`P4InterlockPage::show_result`] 就地上屏 `display_safe(message.trim())`（**不吞**，EDGE-12）；空串时退到 §3.6 全局行的「操作失败」（**不造假原因**）。`RejectedPrecondition` **另**置「请求一次状态刷新」标志（[`P4InterlockPage::take_refresh_request`]）—— 被拒即说明屏上观测可能过期，该标志对**各类**前置条件**都正确**，且与文案**解耦**（改了文案也不影响刷新语义）。EDGE-19 的**固定**文案「联锁状态已变化 · 请刷新后重试」由 [`P4InterlockPage::show_conflict`] 承担，它是**显式入口**：需 B3 在**能判定**「提交时状态已变化」时调用。**当前契约无法自动达成该判定** —— `ControlCode::RejectedPrecondition` 把「状态已变化」与「触发源未复位 / 保持时间不足 / latch / StopPending」**糊在同一个码**里，回执**无**结构化 `InterlockReject` 字段；且 `InterlockReject` 的 **7 个变体里没有「冲突」变体** ⇒ 「B3 自行解析出 `InterlockReject`」对该场景**不可实现**（**契约级缺口**，属 F/G/H/I/J/K 与契约所有者的责任）。⇒ 本页**不假设**该固定文案会被自动触发：它在屏上出现**当且仅当**外部显式调用了 `show_conflict()` | 契约冻结（`display-proto` 不得改）；不做「按消息串猜语义」的脆弱解析（猜错即**谎报原因**，与 §2.6「绝不造假」冲突） | 若 `display-proto` 在控制回执中增 `reject: Option<InterlockReject>`（或为 EDGE-19 单列一个 `ControlCode` 变体），则 [`P4InterlockPage::show_result`] 直接分派（单一分派点），固定文案即可自动可达 |
//! | IL13 | 回执 → 展示态的映射（**单一映射点** `Core::apply_ack`）：`latched := ack.latched`、`stop_failed := !ack.stopped`（`InterlockOpAck.stopped` 的契约语义是「操作后停机**确认**态」）；`available` / `enabled` / `sources` / 两灯**不变**（回执不带，等下一帧，最坏 ≤1.35 s） | §6.4「成功」行要求「用回执 `applied` **立即**刷新，不等下一帧」（F17.6 / IL-02）⇒ 回执能覆盖的两项立即刷；其余字段回执确无载体（契约冻结）⇒ 不臆造、由下一帧补。**契约未显式声明** `stopped` 与 `stop_failed` 互补 ⇒ 若后端语义有出入，只改 `Core::apply_ack` 一处 | 契约若明示互补关系，此处改为显式字段 |
//! | IL14 | **保持时间倒计时**（UI §6.4「保持时间不足」行：按钮旁显剩余秒数）**已实现**，时钟由 [`P4InterlockPage::tick`] 注入：收到 `HoldNotElapsed { remaining_secs }` 时记剩余秒数并置「待取基准」标志，**首个 `tick`** 取基准 `Instant`，其后每拍按已过秒数递减（`saturating_sub`，不 panic）；**页面不读 `Instant::now()`**（`Toast::new` 的既有行为除外，同 `p2_config.rs` **PD20**）。倒计时到 `0` 只显示「还需 0 秒」，**不**自作主张放行（是否可操作仍由后端前置判定） | 帧内只有 `release_hold_secs`（**须保持**的时长），**没有**「已保持多久 / 何时复位」⇒ 无法从帧推出绝对剩余时间；唯一可得的绝对量是后端拒绝里的 `remaining_secs` ⇒ 以「拒绝后的首拍」为基准推进是**唯一**不臆造的做法 | 若帧增「源复位时刻 / 已保持秒数」，改为帧驱动（届时删掉基准捕获） |
//! | IL15 | 提交中（[`P4InterlockPage::set_submitting`]）两按钮 `disabled` 且**无按钮级就地原因** | UI §6.4 未定义「提交中」态的就地文案（§3.6 亦无该行）⇒ 只置灰、**不造文案**；防重由 `ConfirmDialog` 自身的 `Debounce`（500 ms，TT-10）与按钮禁用共同承担 | 无（**有意**） |
//! | IL16 | `observed_sources` 取帧内**全部**源名（含未触发），口径与后端 `status_sources()`（列**全部** distinct token）一致 | 只取 `tripped` 的集合会把「某源**复位**」与「源本来就没有」的区分度降低；全量名列表是更严的并发检查（EDGE-19「状态已变化」的判据） | 无（**有意**） |
//! | IL17 | 注入侧 / 契约侧的**自由文本**（源名、拒绝原因、回执 `message`）上屏前一律过 [`display_safe`] | 这些字面量**不在** `ui/**` 的源码字面量走查面内（来自 `display-proto` 或运行时帧），直上屏含 cmap 外 ASCII（小写 / `-`）即豆腐块（同 `pages/mod.rs` **D9** 与 `p2_config.rs` **PD13**）；`display_safe` 只改写 ASCII，**非 ASCII 缺字挡不住**（残余见 IL6 / PD13） | 同 D9（扩字符集后改写面自然收窄） |
//! | IL18 | `人工释放联锁` 在 `available && enabled` 时**恒可用**（除提交中）：**不**按「触发源未复位 / 保持时间不足 / 未处于 latch」等在本地预判置灰 | ① 释放是**安全正向**操作（清 latch），本地预判置灰会挡住该路径；② §6.4 的 EDGE-12 恰恰要求「把**具体**拒绝原因告诉现场」—— 本地预判会**替代**后端的结构化原因（现场只看到灰按钮、看不到「哪个源没复位」）⇒ 与「不得静默失败」相悖 | 无（**有意**） |
//! | IL19 | `ControlResponse::duplicate`（幂等命中）**本页零读取** ⇒ **漏覆盖**（与 `p2_config.rs` **PD18** 同族） | UI §3.6 的 P4 用字表与全局 Toast 行**都没有**「幂等命中 / 重复请求」的文案 ⇒ 无字可上屏；也不能凭一比特**造**一句文案（**绝不造假**） | **§3.6 需补一行文案** ⇒ 收口于 B2c 之后的「字体码表 + 文案统一收口批」（与 IL1 同批）；届时在 [`P4InterlockPage::show_result`] 里读 `resp.duplicate` 并弹提示 |
//! | IL20 | `SOURCE_ROW_POOL` / `INNER_W` / `CARD_HEAD_H` / `CARD_INSET` 与 `p2_config.rs` / `p6_system.rs` **同式重复**（各页各持一份） | **不动**（KISS + 两文件本批**禁改**）：三者都是 `theme` 常量的**一格推导**，上收需要一个新共享模块（结构变更，超出本批范围，与 `p2_config.rs` 的 PD19 同一处置） | **B2c 之后**统一上收 `ui/pages/mod.rs`；在此之前**任一处改 `theme` 派生式必须三处同改** |
//! | IL21 | [`P4InterlockPage::show_audit_unavailable`]（EDGE-18）**除 Toast 外另落就地红字**（操作条上方同一文案） | EDGE-18 只要求 Toast ⇒ 这是**超出规格的 additive 行为**，**保留**：① 与 **IL11** 同款取向（Toast 会过期，而 fail-closed 的「操作**未执行**」这一结论须常驻可读 —— 现场看到灰按钮时能立刻知道原因）；② **零新增**：落点（就地原因带）与文案（转出 `TEXT_AUDIT_UNAVAILABLE`）都是既有件，未新建对象、未添第二份字面量 ⇒ 无屏上冗余（Toast 与红字同文案、位置不同） | 无（**有意**）；若 PM 裁定 Toast 足够，删去 [`P4InterlockPage::show_audit_unavailable`] 里的 `set_plain_reason` 一行即可（其单测断言同步收） |
//! | IL22 | latch 的 `StatusChip` **增了图标通道**（`●` / `○` / `?` 三态，见 [`latch_chip_icon`]） | UI §5.3 对胶囊只要求 **text + color** 两通道 ⇒ 这是**超出规格的 additive 行为**，**保留**：① `StatusChip::new(parent, w, icon, text, skin)` 的**签名强制**要求 icon 实参（`components.rs` 本批**禁改**，无「省略图标」的口）；② 契约未指定字形 ⇒ 取与灯类同族的三态（实心 / 空心 / 问号），不可用态取 `?`、**不**复用 `✓` / `⚠`（与 §8.3「不得复用」一致）；③ 有单测锁住三态互异与不可用态的字形（`p4_interlock.rs::tests::latch_chip_never_says_unheld_when_unavailable`） | 若 `StatusChip` 补「无图标」构造口，可改为 text + color 两通道（须同步改 §5.3 走查与上述单测） |
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
//! - **渲染期不加对象**：源行 / 三卡 / 状态件 / 弹层 / Toast 一律**预建或在事件外建**，
//!   `render`（1 Hz）**只改文本 / 颜色 / 可见性 / 尺寸**，不新建也不删除对象
//!   （§7.5 脏区与动效纪律）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use mupc_display_proto::{
    ControlCode, ControlResponse, InterlockOpAck, InterlockOpPayload, InterlockReject,
    InterlockSection,
};

use crate::lvgl::obj::Obj;
use crate::lvgl::style::{Color, Style};
use crate::lvgl::widgets::{self, Label, LongMode, ScrollContainer, TextButton};
use crate::lvgl::LvglError;
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
pub const TEXT_STATE_UNAVAILABLE: &str = "联锁状态不可用";
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

// ── 联锁总态卡（UI §6.4 区块规格「联锁总态卡」；线框 992×180）────────────────
/// 总态卡**内容**高 = 卡头 44 + 总态词 96（外缘高 = 内容 + 2 × `CARD_INSET` = 174，见 **IL3**）。
const STATE_CARD_BODY_H: i32 = CARD_HEAD_H + TextSlot::InterlockState.px() as i32;
/// 总态卡外缘高。
const STATE_CARD_H: i32 = STATE_CARD_BODY_H + 2 * CARD_INSET;
/// 顶部语义横条高（UI §6.4：卡顶 3 px 横条 = 语义色）。
const STATE_BAR_TOP_H: i32 = theme::Stroke::ALERT;
/// 左缘语义竖条宽（UI §6.4：卡左缘 6 px 竖条 = 语义色；`theme` 已有该档）。
const STATE_BAR_LEFT_W: i32 = Dimens::INTERLOCK_BAR;
/// 总态词图标 y（在 96 px 行内居中，图标 72 px）。
const STATE_ICON_Y: i32 =
    CARD_HEAD_H + theme::center_offset(TextSlot::InterlockState.px() as i32, Dimens::ICON_XL);
/// 总态词 x（图标右侧 + 同组缝）。
const STATE_TEXT_X: i32 = Dimens::ICON_XL + Dimens::GAP_MIN;
/// 总态词宽（到卡内容区右缘）。
const STATE_TEXT_W: i32 = INNER_W - STATE_TEXT_X;
/// latch 胶囊宽（取 `StatusChip` 契约宽 236；最长文案 9 字 ≈ 204 px + 图标槽 32 ≤ 236）。
const LATCH_CHIP_W: i32 = Dimens::CARD_STATUS_W;
/// latch 胶囊 x（贴卡内容区右缘）。
const LATCH_CHIP_X: i32 = INNER_W - LATCH_CHIP_W;
/// latch 胶囊 y（在卡头行内居中；`STATUS_CHIP_H` 32 > 28 ⇒ 收敛到 0，与卡头文字同顶）。
const LATCH_CHIP_Y: i32 = theme::center_offset(CARD_HEAD_H, Dimens::STATUS_CHIP_H);

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
/// 联锁总态卡 y（UI 线框 `Y80`）。
const STATE_CARD_Y: i32 = Dimens::CONTENT_PAD_TOP;
/// 触发源卡 y（总态卡 + 跨区缝）。
const SOURCE_CARD_Y: i32 = STATE_CARD_Y + STATE_CARD_H + Dimens::GAP_SECTION;
/// 固定操作条高（UI §6.4 线框 `y 624–696` ⇒ 72 = 危险按钮高 64 + 内容区上内边距 8）。
const ACTION_BAR_H: i32 = Dimens::BTN_H_PRIMARY + Dimens::CONTENT_PAD_TOP;
/// 就地原因带高（正文 24；**IL4**：线框的 552 视口拆成「视口 528 + 原因带 24」）。
const REASON_BAND_H: i32 = TextSlot::Body.px() as i32;
/// 滚动视口高（内容区 624 − 操作条 72 − 原因带 24 = 528）。
const VIEWPORT_H: i32 = Dimens::CONTENT_H - ACTION_BAR_H - REASON_BAND_H;
/// 就地原因带 y。
const REASON_BAND_Y: i32 = VIEWPORT_H;
/// 操作条 y。
const ACTION_BAR_Y: i32 = VIEWPORT_H + REASON_BAND_H;
/// 操作条内按钮 y（垂直居中）。
const ACTION_BTN_Y: i32 = theme::center_offset(ACTION_BAR_H, Dimens::BTN_H_PRIMARY);
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

/// 展示态 → 总态词（UI §3.6 P4「总态」行逐字）。
pub const fn state_text(v: StateView) -> &'static str {
    match v {
        StateView::Latched => TEXT_STATE_LATCHED,
        StateView::Unlatched => TEXT_STATE_UNLATCHED,
        StateView::Unavailable => TEXT_STATE_UNAVAILABLE,
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
pub fn source_label(name: &str) -> String {
    for (key, label) in SOURCE_LABELS {
        if name == key {
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
///   **IL18** 对 `release` 的取向是同一条道理（EDGE-12「不得静默失败」）。放开后后端必回
///   `StopPending`，其具体原因（`停机未确认 · 暂不可授权重启`，见 [`reject_text`]）随之可达。
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
    // ── 触发源卡 ──
    /// 卡对象（**拥有型**；高度随源数自适应）。
    source_card: Obj,
    /// 卡头文案（`触发源 · N`）。
    source_title: Label,
    /// 行容器（高度随源数 / 不可用态高自适应）。
    source_rows_box: Obj,
    /// 行池（见 **IL9**）。
    source_rows: Vec<SourceRow>,
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
}

impl Core {
    // ── 渲染：帧 → 屏（**只改文本 / 颜色 / 可见性 / 尺寸**，不新建对象）────────

    /// 注入一帧的联锁段（`None` ⇒ 契约缺省 = **不可用**）。
    fn apply_section(&self, s: &InterlockSection) {
        *self.section.borrow_mut() = s.clone();
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
        self.source_title.set_text(&sources_title(s.sources.len()));
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
        let body_h = if s.available {
            (shown_rows as i32 * SOURCE_ROW_H).max(EMPTY_H)
        } else {
            UNAVAILABLE_H
        };
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
    }

    /// 刷新两个操作按钮的可用性 + 就地原因带。
    fn refresh_actions(&self) {
        let st = op_state(&self.section.borrow(), self.submitting.get());
        self.release.set_disabled(st.release_disabled);
        self.restart.set_disabled(st.restart_disabled);

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

        // 右槽：**M1 专属**阻塞（全局阻塞时不重复显示；`op_state` 已把全局原因并入其取值）。
        let right = if self.section.borrow().available && self.section.borrow().enabled {
            st.restart_reason.map(str::to_string)
        } else {
            None
        };
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
        self.countdown_on.set(false);
        self.countdown_base.set(None);
        self.countdown_left.set(0);
        self.refresh_actions();
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

/// 开弹层失败只写 stderr（**绝不 panic**、**绝不上屏** —— 上屏文案必须逐字在 cmap 内）。
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
        let scroll = ScrollContainer::create(&root)?;
        scroll.set_size(Dimens::CONTENT_W, VIEWPORT_H);
        scroll.set_pos(0, 0);
        scroll.add_style(
            &theme::transparent(),
            crate::lvgl::style::StyleSelector::main(),
        );

        // ── ① 联锁总态卡 ──
        let state_card = decor(&scroll, Dimens::CONTENT_W, STATE_CARD_H, &theme::card())?;
        state_card.set_pos(0, STATE_CARD_Y);
        // 顶部 3 px 横条 + 左缘 6 px 竖条（**贴卡外缘** ⇒ 用负偏移越过内边距，见常量块注）。
        let state_bar_top = decor(
            &state_card,
            Dimens::CONTENT_W,
            STATE_BAR_TOP_H,
            &theme::card_head_bar(Palette::STOPPED),
        )?;
        state_bar_top.set_pos(-CARD_INSET, -CARD_INSET);
        let state_bar_left = decor(
            &state_card,
            STATE_BAR_LEFT_W,
            STATE_CARD_H,
            &theme::card_head_bar(Palette::STOPPED),
        )?;
        state_bar_left.set_pos(-CARD_INSET, -CARD_INSET);
        let state_title = text_label(
            &state_card,
            TEXT_CARD_STATE,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        state_title.set_pos(0, CARD_HEAD_TEXT_Y);
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
        state_text_l.set_pos(STATE_TEXT_X, CARD_HEAD_H);

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

        let core = Rc::new(Core {
            root,
            scroll,
            action_bar,
            audit_note,
            state_card,
            state_bar_top,
            state_bar_left,
            _state_title: state_title,
            state_chips,
            state_icon,
            state_text_l,
            state_styles,
            state_icon_styles,
            state_text_styles,
            state_bar_top_style: Cell::new(usize::MAX),
            state_bar_left_style: Cell::new(usize::MAX),
            state_icon_style: Cell::new(usize::MAX),
            state_text_style: Cell::new(usize::MAX),
            source_card,
            source_title,
            source_rows_box,
            source_rows,
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
        });

        // 按钮：只**开弹层**（确认完成前不发任何请求）。
        {
            let w = Rc::downgrade(&core);
            core.release.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                if let Err(e) = open_dialog(&c, OpKind::Release) {
                    report_open_failure(TEXT_RELEASE, &e);
                }
            });
        }
        {
            let w = Rc::downgrade(&core);
            core.restart.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                if let Err(e) = open_dialog(&c, OpKind::AckM1) {
                    report_open_failure(TEXT_ACK_M1, &e);
                }
            });
        }

        let page = Self { core };
        // 骨架态 = **无帧** ⇒ 契约缺省（`available = false`）⇒ 屏显「联锁状态不可用」。
        page.core.apply_section(&InterlockSection::default());
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

    // ── 数据入口（**读路径 = 帧驱动**）─────────────────────────────────────

    /// 注入一帧（`frame = None` ⇒ 该段取契约缺省 = **不可用**）。
    ///
    /// ⚠️ `freshness` / `channel` 本页**不消费**：联锁段自身的 `available` 才是判据；通道级降级
    /// 由 B2c 的外壳遮罩承担（UI §8.3 `通道断（EDGE-03）` 的整屏降级行）。
    pub fn render(&self, input: &PageInput<'_>) {
        let section = match input.frame {
            Some(f) => f.interlock.clone(),
            None => InterlockSection::default(),
        };
        self.core.apply_section(&section);
    }

    /// 直接注入联锁段（等价于 [`P4InterlockPage::render`] 的帧内那一段；供 B3 接线与离屏用例）。
    pub fn set_section(&self, section: &InterlockSection) {
        self.core.apply_section(section);
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
        *self.core.on_release.borrow_mut() = Some(Box::new(f));
    }

    /// 注册「M1 授权重启」意图回调（语义同 [`P4InterlockPage::set_on_release`]）。
    pub fn set_on_ack_m1<F>(&self, f: F)
    where
        F: FnMut(InterlockOpPayload) + 'static,
    {
        *self.core.on_ack_m1.borrow_mut() = Some(Box::new(f));
    }

    /// 提交中（外部发出请求后置 `true`；回执到达后由 [`P4InterlockPage::show_result`] 复位）。
    /// 两按钮 `disabled`（**IL15**：§3.6 无该态就地文案 ⇒ 不造）。
    pub fn set_submitting(&self, on: bool) {
        self.core.submitting.set(on);
        self.core.refresh_actions();
    }

    /// **结构化**拒绝注入（`InterlockApi` 直连路径 / B3 自行解析出 `InterlockReject` 时调用）。
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
        self.core
            .show_toast(ToastTone::Failure, ICON_FAIL, TEXT_TOAST_FAIL)?;
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

    /// 是否曾注入过**有效**帧（断言口径）。
    pub fn ever_available(&self) -> bool {
        self.core.ever_available.get()
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
        assert_eq!(state_text(state_view(&d)), TEXT_STATE_UNAVAILABLE);
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
