//! # `ui/pages/p2_config.rs` —— P2 配置页（12-MUPC v2.0 工作单元 **B2b-2**，含写操作）
//!
//! 设计 §6.2「P2 配置页（F9，含写操作）」逐条落地；UI 设计 §6.2 给版式、§7.3 给弹层、
//! §2.5 给强确认分级、§5.1 #6/#7/#8 给控件尺寸、§5.2 给四态色值。
//!
//! ## 本页与 P1 / P6 的三点结构差异（**契约见 `ui/pages/mod.rs` 的「补充（B2b-2）」**）
//!
//! 1. **不由 [`PageInput`] 驱动**：P2 的数据来自**控制通道**（`GET /v1/console/config`，
//!    另 9811 端口），与 1 Hz 显示帧无关。注入入口见
//!    [`P2ConfigPage::set_config`] / [`P2ConfigPage::set_unavailable`] /
//!    [`P2ConfigPage::set_submitting`] / [`P2ConfigPage::show_result`]；
//!    **降级态只由注入值驱动**（本页不读时钟、不自行发请求）。
//! 2. **页根不是滚动容器**：UI §6.2 线框 `Y624 ┌ 固定操作条（不随滚动）┐` ⇒ 页根是普通容器，
//!    内部再分两层 —— 上为**滚动视口**（`992 × (624 − 72) = 992 × 552`，与 UI 线框「552 px
//!    视口」一致）、下为**固定操作条**（`992 × 72`）。故 `pages::page_root()`（"页根自身即
//!    滚动容器"）**未改语义**，本页自建视口。
//! 3. **「意图」与「请求」分离**：设计 §6.2 末行（UI ↔ 后端 确认-审计链路）——
//!    **确认完成前不发出任何请求**（未确认 = 无网络动作）；确认完成后本页经
//!    [`P2ConfigPage::set_on_submit`] 把 [`ConfigPatch`] **交回外部**，
//!    `request_id`(uuid) / `issued_at_ms` 由 B3 的 `console.rs` 生成（**本页不含 uuid /
//!    时间戳 / HTTP**）。
//!
//! ## 只读字段（设计 §6.2「只读字段」行 / PL-4）
//!
//! `editable == false`（`display.bind_addr` / `display.control_bind_addr`：回环是安全红线）：
//! 控件 `disabled` **且**附说明行 [`TEXT_READONLY_NOTE`]；字段**仍出现在列表中**
//! （以**可见性**换取现场可核查性）。
//!
//! ## ⚠️ 已知偏差登记（**独立编号 `PD`** —— `D1~D9` 是页面层编号、`CD1~CD7` 是
//! `ui/controls.rs` 编号，本表**不与二者冲突**；本表只登记本页的屏文 / 尺寸与契约不一致处）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | PD1 | `gateway.listen_addr` 标签取 **`本机监听地址 · IEC 104`**（PM 裁定串为「本机监听地址（IEC 104）」） | **缺字**：全角括号 `（`/`）`（U+FF08/FF09）不在生成字体 cmap 内 ⇒ 照抄即豆腐块。取 cmap 内的 `·`（U+00B7）作分隔，语义不变（**仍是"本机监听地址"，不是"对端 IP"**） | 字库收口批（扩 §3.6 字符集并重跑 `gen_fonts.sh`）后逐字改回 PM 原串 |
//! | PD2 | `display.bind_addr` / `display.control_bind_addr` 标签取 **`本机地址 · 仅本机`**（设计 §6.2「只读字段」行 / §6.6 的 PM 裁定串为「本机服务地址（仅回环 127.0.0.1）」） | **缺字**：`服`(U+670D) / `务`(U+52A1) / `环`(U+73AF) 与全角括号 / `，` **均不在 cmap 内** ⇒ 无法逐字落地；改述保留两条语义（**本机** + **仅本机可达**）。**为何不与 IEC 104 行同前缀**：设计 §6.2 PM 裁定行 + UI §6.6 / **U-1 / EDGE-24** 要求「监听地址（网关）与回环服务地址**必须可区分、不得互换**」—— 若两行都取「本机监听地址 · …」（这正是改前形态，与 [`TEXT_LISTEN_ADDR`] 共享前缀），现场无法据文字区分"网关监听"与"仅本机回环"，裁定落空 ⇒ 本串**不含「监听」二字**。**为何不再与 P6 的 `TEXT_SERVICE_ADDR` 共用**：P6 那行是**整体服务口径**（读 9810 / 控制 9811 两通道的服务地址），P2 这两行是**本机绑定地址**（网关以外、仅本机可达）—— 同串会让"服务地址"与"绑定地址"两个口径在屏上不可分；改成独立字面量后两页各自可演进（P6 若要回改设计原文，不必牵动 P2） | 同 PD1（扩 §3.6 后**逐字改回设计原文**，届时两处一并回改） |
//! | PD3 | 只读说明行取 **`仅本机访问 · 不可修改`**（设计写「仅本机回环，不可修改」） | 同上：`环`(U+73AF) 与全角逗号 `，`(U+FF0C) **不在 cmap 内** | 同 PD1 |
//! | PD4 | **卡内左右内边距 = 17**（描边 1 + `Dimens::GAP_MIN` 16），UI §6.2 写 **20** | 与 P6 同口径（`theme::card()` 的既有内边距 + 描边）；且 `theme` 无 20 px 档，**不为凑 3 px 引入裸数值**。后果：字段名 x=33（UI 写 36）、行型 A 控件右缘 x=991（UI 写 988），**整体 3 px** | 与 UI §6.2 一并复核（若 PM 要求逐像素，须先在 `theme` 增档） |
//! | PD5 | **行型 B 高 = 122**（上缝 16 + 字段名 26 + 缝 16 + 控件 64），UI §6.2 写 **120** | `theme` 无 2 px 档（同 `pages/mod.rs` 的 **D3** 同款理由） | 同 D3（随 theme 缺口上收一并处理） |
//! | PD6 | **字段错误态**取「行左缘 **4 px** 危险色竖条 + 字段名 / 原因**红字**」双通道；UI §5.2 写「描边 `#FF6B6B` **2 px** + 下方 24 px 红字原因」 | `theme.rs` **本批禁改**，其现有的危险描边只有 3 px 单边（`card_alert_*`）与 2 px **按钮**描边，**没有**「2 px 全框危险」样式；本页不得内联裸色值 ⇒ 复用既有的 `card_head_bar(Palette::DANGER)`（4 px 竖条）+ 红字 | `theme` 收口批：补 `field_error_frame()` 后改为整框 2 px |
//! | PD7 | 失败原因就地显示在**页顶提示行**（`Y80` 槽位，与页说明行**同槽互斥**）与**字段行**；UI §6.2 流程 6 写「**弹层内**就地显示 message」 | `ConfirmDialog`（`components.rs`，**本批禁改**）的「影响范围」文案在构造期固定，**没有**可变错误文案口；且弹层在 `show_result` 时即关闭（`ConfirmDialog::close` 不得在 LVGL 事件回调内调用，见其模块文档） | `components.rs` 补 `set_impact()` 后改回弹层内显示 |
//! | PD8 | 「保存中…」取 **`保存中...`**（三个 ASCII `.`，U+002E） | `…`(U+2026) **不在 cmap 内**；`.` 在（且 `components.rs` 的 `DOTS` 截断同款） | 同 PD1 |
//! | PD9 | `WriteMode::FullRewrite` 的 Toast 取 **`配置已保存 · 原有文字已不存在`**（设计 §4.3.2.1 写「配置文件已整体重写，原有注释不再保留」） | **缺字**：`整`(U+6574) / `写`(U+5199) / `注`(U+6CE8) / `留`(U+7559) / `再`(U+518D) 均不在 cmap 内 ⇒ 无法逐字照抄。改写串保留两条语义：**已保存** + **原有文字（注释）已不存在** | 同 PD1 |
//! | PD10 | **`WriteMode::FullRewrite` 的 Toast 由 `set_config` 统一触发**（`show_result` 成功路径不再叠加"保存成功"Toast —— UI §7.2「同一时刻仅 1 条」，**取信息量更大的那条**） | `ConfigView.write_mode` 的契约语义即「最近一次落盘写模式，`FullRewrite` 时 UI 须明示」（EDGE-23）⇒ 任何携带该值的视图都该明示，故收在唯一入口 | 无（有意） |
//! | PD11 | 保存 / 恢复默认值的分级与「涉及：」列表一律按**本次改动**判定：`save_level` / `reset_level` / [`reconnect_field_labels`] 收**本次补丁的键集合**（保存 = `draft_patch(..).changes`；恢复默认值 = `defaults_patch_of(..).changes`），键集合判定由 [`reconnect_in`] 承担（**不再**看"视图内全部字段"） | **⚠️ 文档内冲突（如实逐列四处原文，不择利引用）** —— 出处文件 = `docs/superpowers/plans/modules/12-MUPC-本地显示终端-UI设计文档.md`（行号为逐行核对结果）：<br/>① **`L2` 行 `:99`** 写「联锁释放、M1 授权、**含连接类字段的配置保存**、恢复默认值」——「含」可读作"**本次**含"（改动口径），措辞本身**不排除**视图口径（歧义行）；<br/>② **`L2+` 行 `:100`** 写「**任一字段** `requires_reconnect == true` 的配置保存」——**视图口径**（"任一字段"= 视图里存在，不限定本次改动）；<br/>③ **§6.2 交互流程 3 `:511`（明细示例）+ `:512`（WarnBanner 插入判据）**：`:511` 的示例是「`端口：2404 → 2405`」+（`:512`）「**涉及：端口**」，而同视图内还有 `监听地址`（§4.3.3 明列 `requires_reconnect=true`）却不进「涉及：」⇒ **只有改动口径**能让该示例成立；但**同一段**的 `:512` 判据原文是「**若任一字段** `requires_reconnect == true` → 插入 `WarnBanner`」= **视图口径** ⇒ **该段自身即自相矛盾**（示例与判据不能同时满足）；<br/>④ **§7.3 `WarnBanner` 行 `:701`** 写「见 §2.5；**当任一字段** `requires_reconnect` **或含连接类字段时**强制出现」——亦为**视图口径**（"或含"把 ① 的歧义行一并读成视图口径）。<br/>⇒ **三处原文（②、③的 `:512`、④）支持视图口径、一处（①，措辞歧义）不排除视图口径、仅 ③的 `:511` 示例支持改动口径**。**为何仍选改动口径**：(a) ③ 的示例是**可执行验收**——示例不成立则实现无法同时满足 §2.5 与 §6.2；(b) §2.6 铁律「降级可见、**绝不造假**」——只改日志级别却弹「生效瞬间通信将短暂中断」是**谎报副作用**；(c) **L1 可达性**——真实字段表含 `gateway.listen_addr` / 核间端口（§4.3.3）⇒ 视图口径下 **L1 永不可达、`WarnBanner` 恒亮**，§2.5 的 L1 与 L2 两行同时报废。<br/>**⛳ 文档内冲突，待 PM 裁定；[`reconnect_in`] 是单一切换点 —— 若裁定视图口径，只需改它一处** | 无（**已按"本次改动"落地**；若 PM 另裁，只需改 [`reconnect_in`] 一处） |
//! | PD12 | **只读字段不进 `defaults_patch()`** | `ConfigField::validate_value()`（契约）对 `editable=false` **一律拒绝**（"只读，不可修改"）⇒ 把只读字段放进 `changes` 会让**整个**恢复请求被后端二次校验打回（PL-4 的红线字段本就不可写） | 无（**有意**；恢复默认值的"本次改动"= 全部 `editable` 字段 —— 与视图口径的差集正是只读字段，见 [`reconnect_in`] 的等价性论证） |
//! | PD13 | 注入侧 `group.label` / `field.label` 与 `Enum` 选项**统一过 [`display_safe`]**（出口 = [`group_label_text`] / [`field_label_text`]） | 改前三条**同类数据两条路径**不一致（`Enum` 选项过了、`group.label` / `field.label` 没过）：后端标签含 cmap 外 ASCII（`-` / 小写）即豆腐块。**残余风险（如实登记，不粉饰）**：`display_safe` **只改写 ASCII**（`-`/`_`→`–`、小写→大写同族、其余→`?`），**非 ASCII（中文）字符一律原样透传** ⇒ 后端 `label` 里的**缺字中文（如 `环` / `服务` / `（`）它挡不住**，真机照样豆腐块。**真正的防线**：后端字段表（`ConfigFieldMeta`）的 `label` 必须约束在 **UI §3.6 用字表**内 —— 属**联调 / 后端**责任（见 UI §3.6 与设计 §6.2）；本页的 [`LABEL_OVERRIDES`] 只是**例外覆盖**机制（只为 PM 裁定键而设），**不是**通用护栏 | **B2c 之后**的「字体码表 + 文案统一收口批」：扩 §3.6 字符集 ⇒ `display_safe` 的"改写面"随 cmap 扩大而收窄，缺字中文风险随之下降（**不会归零** —— 字库永远落后于任意后端文案，后端约束才是根治） |
//! | PD14 | **行型 B 的 `Ipv4Stepper` 横向跨到卡外缘**：实测跨度 **x16–1007**（= [`Dimens::CONTENT_W`] **992 px**，与卡**外缘**齐宽），UI §6.2 写「控件独占次行 **(x36–x988)**」（卡**内**，有效 952 px）—— 行型 A 的内边距偏差已登记 **PD4**，行型 B 这条本次补登记 | **实算根因**：卡内可用宽 [`INNER_W`] = 992 − 2 × **17**（描边 1 + 内边距 16）= **958**，而 `Ipv4Stepper` 整件宽 = [`Dimens::CONTENT_W`] = **992**（`ui/controls.rs` **CD2**：四段 792 + 缝 8 + 汇总 **184**；汇总宽 = "内容区余量"，为容纳 `192.168.1.10` 12 字符）⇒ 控件比卡内宽 **34 px = 两侧各 17 px**。若从卡内容区原点起排（屏幕 x33）则右端 x1024 越出卡外缘（x1007）**17 px** 并被父对象裁剪（`ui/controls.rs` CD2 同款事实）⇒ 取 `ROW_B_CTRL_X = −CARD_INSET`（`−17`）把控件**左端内缩到卡外缘**，实测跨度 x16–1007 = 恰与卡外缘齐宽，**代价 = 吃掉卡左右各 17 px 内边距**（行型 A 控件右缘落在卡内右缘 —— 实测闭区间右缘 x990，即 PD4 记的 x991 排他右缘；两版式的口径**不一致**） | **与 CD2 同批收口**：先由 PM 定 `Ipv4Stepper` 整件宽 —— UI 自身三口径互相矛盾（§5.1 #7 写 **856**、§6.2 写 **952**、实测落地 **992**）；若裁「控件必须在卡内 (x36–x988)」⇒ 需把汇总标签 **184 → 144**（`192.168.1.10` 放不下，须另行设计）或改行型 B 版式（如汇总挪到第二行） |
//! | PD15 | `U16` / `U64` 元数据的 `step == 0` 在页内**折算为 1** 后再建 [`Stepper`]（**不**报错、**不**整页 `Err`） | 契约 [`ConfigKind::validate_value`]（`mupc/crates/display-proto/src/control.rs:528` / `:538`）写 `if *step != 0 && …` ⇒ **显式把 `step == 0` 当合法**（语义 = "不校验步长"）；而 `Stepper::new`（`ui/components.rs`，本批**禁改**）只接受正步长 ⇒ 旧实现返回 `Err` 让**整页**拒绝渲染（一个合法字段白屏全页）。折为 1 是**契约语义的忠实映射**："不校验步长" ⇒ 任何整数值都合法 ⇒ 步长 1 是最细粒度、可达**全部**合法值 | 无（**有意**）；若将来 `Stepper` 支持 `step == 0`（= 无步进约束）可直接回改 |
//! | PD16 | `Enum` 元数据的**字段级降级**（一律**不** `Err`）：可容段数上限 [`ENUM_MAX_SEGMENTS`] = `INNER_W / SEGMENT_MIN_W` = **9**（`9 × 96 = 864 ≤ 958`；`10 × 96 = 960 > 958`）⇒ ① 选项 **> 9** ⇒ 只渲染一个 **9 段窗口**，窗口**必含当前值所在选项**（否则合法值会被挤到窗外 ⇒ 静默改写），该行约束槽上屏「仅显示 9 项」；② 选项 **= 0** ⇒ 渲染**单段 [`PLACEHOLDER`] 占位**的禁用行（`SegmentedControl::new` 拒绝空选项）。两条都保证**其它字段仍可正常渲染与编辑** | 旧实现在 `enum_width(..)` 里对超宽返回 `Err(LvglError::InvalidArgument)` ⇒ **整页**拒绝渲染；而 `ConfigKind::Enum { options }` 的长度**契约上无上限**（`display-proto` 冻结、不得改契约）。**为何选"截断"而非"换控件 / 换版式"**：本页可用的等价控件只有 [`SegmentedControl`]（`lv_dropdown` 的展开列表在离屏**不可断言**且 §5.1 未选它）⇒ 换控件等价于改 `ui/controls.rs`（本批禁改）；换版式（分两排）需新增栅格档、§5.1 #4 未定义多排形态 ⇒ **截断是本批唯一不越界的降级**，且"被隐藏项数"**上屏**（不静默） | 无（**有意**）；若 PM 要求"全选项可达"，须先在 `ui/controls.rs` 增多排 / 可滚动分段控件 |
//! | PD17 | 注入新视图时**保留用户草稿**（I4 取 **(b)**）：`set_config` 先用旧 `touched` 集合采集「键 → 当前控件值」，重建卡片后**只把新元数据认可的**（`kind.validate_value(..).is_ok()`）草稿值写回对应控件并保留其 `touched` 标记；键在新视图里消失 / 新元数据不认可 ⇒ 丢弃该键的草稿（控件显示注入值） | **为何选 (b) 而非 (a)"脏则拒绝覆盖"**：(a) 会把**权威刷新路径**（保存成功回执的 `applied` ⇒ [`show_result`](P2ConfigPage::show_result) ⇒ `set_config`）一起挡掉 —— 该路径被调用时页面**必然是脏的**（用户先改、才可能保存成功），拒绝即等于"保存成功后界面不刷新"，属自伤；为它开例外（`show_result` 先清脏）等价于 (b) 再加一条早清路径，反而更绕。(b) 与 EDGE-10「失败保留用户已输入值」**同一取向**，且**永不阻塞**服务端权威视图落屏。**残余（如实登记）**：草稿值被新元数据丢弃时**无 Toast 提示**（场景 = 联调期后端改了字段元数据而页面正持草稿） | 无（**有意**）；若 PM 要求显式提示，收口在 B2c 之后的文案批 |
//! | PD18 | `ControlResponse::duplicate`（契约**强制字段**：`display-proto/src/control.rs:328`，注释明写用途 = "幂等命中提示"）在**本页零读取** ⇒ **漏覆盖**（不是"有意忽略"） | UI §3.6 的 P2 用字表**没有**"幂等命中 / 重复请求"这一行的文案 ⇒ 本页无字可上屏；也不能凭一比特**造**一句文案（**绝不造假**） | **§3.6 需补一行文案** ⇒ 收口于 **B2c 之后**的「字体码表 + 文案统一收口批」（与 PD1 / PD13 同批）；届时在 [`show_result`](P2ConfigPage::show_result) 里读 `resp.duplicate` 并弹提示 |
//! | PD19 | `CARD_INSET` / `CARD_HEAD_H` / `INNER_W` 三个常量与 `ui/pages/p6_system.rs` **逐字重复**（两页各持一份同式定义） | **不动**（KISS + `p6_system.rs` 本批**禁改**）：三条都是 `theme` 常量的**一格推导**，上收需要一个新共享模块（结构变更，超出本批整改范围） | **B2c 之后**统一上收 `ui/pages/mod.rs`（P1/P2/P6 共用一份）；在此之前**任一处改 `theme` 派生式必须三处同改** |
//! | PD20 | **组件 / 接线缺口（本批不改代码）**：`Toast::new(..)`（`ui/components.rs`）内部取 `Instant::now()`，而本页时钟是**注入**的（`tick(now)`）⇒ Toast 过期时刻与本页业务时钟**不同源**（离屏确定性用例因此不能完全控制 Toast 生命周期） | `components.rs` 本批**禁改** ⇒ 只登记。**⚠️ 订正评审假设**：`components.rs` **已有** `Toast::new_at(..)` ⇒ 缺口**不在组件侧**，而在"本页何时拿到 `now`" —— `set_config` / `show_result` 的签名里**没有** `now`（本页纪律：不读时钟） | **B3 接线时 reconcile**：由 B3 在事件循环里把 `tick` 的 `now` 缓存进 `Cell` 供 `show_toast` 使用，再改用 `Toast::new_at(now, ..)`；**收口前** Toast 的过期语义由注入时刻驱动，两者在真实事件循环里同源（无实际偏差） |
//! | PD21 | **控件值域比契约窄时的屏上回显（C1 的残余，如实登记）** —— 两种子形态：**(i) 注入值非法** ⇒ 该行进错误态 + 控件 `disabled` + **不进草稿**，但**控件本体仍渲染一个"最小可表示值"**（`Enum` ⇒ `options[0]`；`U16`/`U64` ⇒ `min`；`Ipv4` ⇒ `0.0.0.0`）；**(ii) 注入值合法但控件表示不了**（如 `U64::MAX` 超出 `Stepper` 的 `i64` 值域 ⇒ 控件渲染 `i64::MAX`）⇒ 该行**不进错误态**（值确实合法），但同样**不置脏、不进草稿**（拦它的是 [`DraftScope::touched`]，**不是** `invalid`） | 三类控件的取值域**没有"无值"这一档**（`SegmentedControl::new` 拒绝空选项、`Stepper::new` 必须有 `value` 且是 `i64`、`Ipv4Stepper` 四段恒有值），且 `ui/controls.rs` / `ui/components.rs` 本批**禁改**。旧形态的缺陷是"**静默**改写 + 保存可用"（一次点击即可写入屏上从未展示的值，C1）；现形态把 (i) 变为**可见**（行左危险竖条 + 红字 [`TEXT_INVALID_VALUE`]）+ **不可交互**，把 (ii) 变为**不可提交**（不进 `draft` / `is_dirty` / `defaults_patch`）⇒ 风险由"可写入装置"降为"屏上显示一个**不会被写回**的近似值" | 无（**有意**）；根治需给三类控件增"无值 / 非法值 / 超宽值"专用形态（`ui/controls.rs` 收口批）：(i) 改显 [`PLACEHOLDER`]、(ii) 由控件侧支持全 `u64` 值域 |
//!
//! ## 纪律（逐条对应设计要求）
//!
//! - **零文本输入**（红线）：本文件只出现 `lv_button` / `lv_label` / `lv_buttonmatrix`
//!   （经 `Stepper` / `Ipv4Stepper` / `SegmentedControl` / `TextButton`）——
//!   `lv_keyboard` / `lv_textarea` / `lv_spinbox` **零出现**（`ui/tests.rs::p2_static_constraints`）；
//! - **无裸色值 / 裸尺寸**：一切外观经 [`theme`]；本页专属栅格常量集中在下方 `const` 块，
//!   **逐条由 theme 常量推导**（`ui/tests.rs::ui_layout_setters_use_theme_constants` /
//!   `ui_const_i32_definitions_derive_from_theme` 两张网扫本文件）；
//! - **所有权纪律**：凡 `Obj::create` / `TextButton::create` / `Label::create_*` / `Stepper` /
//!   `ConfirmDialog` / `Toast` 返回的**拥有型句柄**一律存进结构体字段（`FieldRow` /
//!   `GroupCard` / [`Core`]），**绝不**只作局部变量 —— 否则构造器返回时即被 `Drop`、
//!   LVGL 级联删除整棵子树（表现为"界面空白但无报错"，B1 出过此类 UAF 级缺陷）；
//! - **回调纪律**：回调内**不 panic**（无 `unwrap` / 无越界索引）、不做阻塞 I/O、
//!   不删除自身宿主；**不在 LVGL 事件回调内关闭弹层**（`ConfirmDialog::close` 的文档要求）
//!   —— 取消走 [`P2ConfigPage::tick`] 的延迟关闭，`show_result` / `set_config` 在事件之外；
//! - **不提供跨线程 API**：本页所有类型都含 LVGL 句柄（自动 `!Send` / `!Sync`），全部调用
//!   必须在事件循环线程内（设计 §5.2 不变量 4）。

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Instant;

use mupc_display_proto::{
    ConfigField, ConfigGroup, ConfigKind, ConfigPatch, ConfigView, ControlCode, ControlResponse,
    FieldError, PatchSource, WriteMode,
};
use serde_json::{Map as JsonMap, Value};

use crate::lvgl::obj::Obj;
use crate::lvgl::style::Style;
use crate::lvgl::widgets::{self, Label, LongMode, ScrollContainer, TextButton};
use crate::lvgl::LvglError;
use crate::ui::components::{ConfirmDetail, ConfirmDialog, ConfirmSpec, Stepper, Toast, ToastTone};
use crate::ui::controls::{Ipv4Stepper, SegmentedControl};
use crate::ui::pages::{
    decor, display_safe, label, layout_box, set_style_index, set_visible, text_label, PLACEHOLDER,
};
use crate::ui::theme::{self, ConfirmLevel, Dimens, Palette, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 上屏文案（UI §3.6 P2 行；**落笔前逐字在 `fonts/lv_font_cmap.txt` 核对**）
//
// 与契约串的偏差逐条登记在文件头 `PD1~PD21`（缺字改写 / 缺字号改写 / 口径 / 尺寸 / 降级），此处只放**成品串**。
// 码表覆盖率走查见 `ui/tests.rs::ui_texts_covered_by_font_cmap`（基线 = 生成字体的实际 cmap，
// 待查集合 = 扫 `ui/**` 源码字面量）。
// ═══════════════════════════════════════════════════════════════════════════

/// 保存按钮（UI §3.6 P2「控件 / 状态」行）。
pub(crate) const TEXT_SAVE: &str = "保存";
/// 保存中按钮（UI §3.6；⚠️ `…` 不在 cmap 内 —— 取三个 ASCII `.`，见 **PD8**）。
pub const TEXT_SAVING: &str = "保存中...";
/// 恢复默认值按钮 + 其确认弹层标题（UI §3.6 P2 / §6.2 流程 8）。
pub(crate) const TEXT_RESET_DEFAULT: &str = "恢复默认值";
/// 页顶说明行（UI §6.2 线框 `Y80`；全角逗号 `，` 不在 cmap 内 ⇒ 取 `·`）。
pub const TEXT_PAGE_NOTE: &str = "修改保存后立即生效 · 无需重启装置";
/// 只读字段的说明行（设计 §6.2「只读字段」行；⚠️ 见 **PD3**）。
pub const TEXT_READONLY_NOTE: &str = "仅本机访问 · 不可修改";
/// **注入值不合法**时该字段行的就地原因（C1；⚠️ 见 **PD21**）。
///
/// 为何**不**回显契约 [`ConfigField::validate_value`] 给出的具体原因：那是**运行期**自由文本
/// （形如 ``` `trace` 不在允许选项内 ```），含反引号与 `允`（U+5141，**不在生成字体 cmap 内**）
/// ⇒ 真机出豆腐块。契约原因**另有出口**：保存失败回执的 `field_errors`（[`P2ConfigPage::show_result`]
/// 经 [`Core::apply_field_errors`] 上屏，见 CF-02）—— 那条路径的原因由**后端**二次校验产生，
/// 与"注入期元数据自相矛盾"是两回事。
pub(crate) const TEXT_INVALID_VALUE: &str = "取值无效 · 不可修改";
/// `Enum` 选项数超出可容段数时该行的说明（UI §6.2 行型 A 的约束槽；⚠️ 见 **PD16**）。
///
/// 载荷 = **可显示段数**（`9`）。**不静默**是硬要求：截断这一降级必须让现场看见（否则与
/// "选项本来就这么几个"不可分）。
///
/// ⚠️ **用字偏差（并入 PD16）**：不用「项」—— U+9879 **不在生成字体 cmap 内**（实测缺字，
/// `ui_texts_covered_by_font_cmap` 会点名）⇒ 取 cmap 内的 `段`（本控件即"分段控件"，
/// `SegmentedControl`）。语义不变（"只显示得了 9 段"）。
pub(crate) fn text_enum_truncated(shown: usize) -> String {
    format!("仅显示 {shown} 段")
}
/// `gateway.listen_addr` 的标签（**PM 裁定**，设计 §6.2 / UI 附录 B U-1；⚠️ 见 **PD1**）。
///
/// **不得**表述为「对端 IP / 远程主站地址」—— 现网 `mupc_gateway::Iec104Server` 是**服务端**
/// （`bind` 后监听、接受调度主站连接），不存在"对端 IP"概念（设计 §4.3.4 / R-08）。
pub const TEXT_LISTEN_ADDR: &str = "本机监听地址 · IEC 104";
/// 回环绑定地址字段的标签（`display.bind_addr` / `display.control_bind_addr`；⚠️ 见 **PD2**）。
///
/// **独立字面量 —— 不**与 P6 的 `TEXT_SERVICE_ADDR` 共用，理由两条：
///
/// 1. **口径不同**：P6 那行是**整体服务口径**（读 9810 / 控制 9811 两通道的"本机服务地址"），
///    本页这两行是**本机绑定地址**（网关以外、仅本机可达）—— 同串会让两个口径在屏上不可分；
/// 2. **必须与 IEC 104 行可区分**：设计 §6.2 的 PM 裁定行 + UI §6.6 / **U-1 / EDGE-24** 要求
///    「监听地址（网关）与回环服务地址**必须可区分、不得互换**」。P6 的串是「本机监听地址 · 仅本机」
///    ⇒ 与 [`TEXT_LISTEN_ADDR`]（「本机监听地址 · IEC 104」）**共享前缀**，两行只能靠后缀分辨；
///    本串取 **`本机地址`**（**不出现「监听」二字**），两行前缀即不同。
///
/// **字面量出处**：设计 §6.2 只读字段行 / §6.6 的 PM 裁定串「本机服务地址（仅回环 127.0.0.1）」；
/// 因 `服`(U+670D) / `务`(U+52A1) / `环`(U+73AF) 与全角括号**不在生成字体 cmap 内**（逐字核对见
/// `fonts/lv_font_cmap.txt`）⇒ 无法逐字落地，改述保留「**本机** + **仅本机可达**」两条语义。
pub const TEXT_LOOPBACK_ADDR: &str = "本机地址 · 仅本机";
/// 保存确认弹层标题（UI §6.2 流程 3）。
pub const TEXT_DIALOG_TITLE_SAVE: &str = "确认保存运行参数";
/// 保存确认的「影响范围」段（UI §6.2 流程 3；全角逗号 ⇒ `·`）。
pub(crate) const TEXT_IMPACT_SAVE: &str = "修改将立即生效 · 无需重启装置";
/// 恢复默认值确认的「影响范围」段（设计 §6.2 恢复默认值行；`为` 不在 cmap 内 ⇒ 去之，语义不变）。
pub(crate) const TEXT_IMPACT_RESET: &str = "全部运行参数将恢复默认值并立即生效";
/// 保存成功 Toast（UI §3.6 全局；全角逗号 ⇒ `·`）。
pub const TEXT_TOAST_OK: &str = "保存成功 · 已生效";
/// 保存失败 Toast（UI §3.6 P2「保存失败」）。
pub(crate) const TEXT_TOAST_FAIL: &str = "保存失败";
/// `full_rewrite` 警示 Toast（EDGE-23 / 设计 §4.3.2.1；⚠️ 见 **PD9**）。
pub const TEXT_TOAST_FULL_REWRITE: &str = "配置已保存 · 原有文字已不存在";
/// 审计不可写 Toast（UI §8.3 `审计不可写（EDGE-18）`；fail-closed，**操作未执行**）。
pub const TEXT_AUDIT_UNAVAILABLE: &str = "审计不可用 · 操作未执行";
/// 配置不可用（控制通道取不到 `GET /v1/console/config` 时的降级标题；无对应 `UnavailableKind`，
/// 见 [`show_unavailable`] 的说明）。
pub const TEXT_CONFIG_UNAVAILABLE: &str = "配置不可用";
/// 取值范围分隔符（UI §6.2 行型 A 的「`1 – 65535`」；`–` U+2013 在 cmap 内）。
pub(crate) const TEXT_RANGE_SEP: &str = " – ";
/// Toast 图标：成功（UI §3.6 声明符号集内的 `✓` U+2713）。
pub(crate) const ICON_OK: &str = "✓";
/// Toast 图标：失败（UI 用 `✕`，但 U+2715 **不在 cmap 内** ⇒ 取 `!`）。
pub(crate) const ICON_FAIL: &str = "!";
/// Toast 图标：警示（`⚠` U+26A0，在 cmap 内；与 `WarnBanner` 同款）。
pub(crate) const ICON_WARN: &str = "⚠";

/// 本页上屏的**全部固定文案**（供 `ui/tests.rs::ui_texts_covered_by_font_cmap` 做"清册 ↔ 源码
/// 字面量"一致性走查 —— 清册**不是**覆盖率基线，见该用例文档）。
pub const ALL_TEXTS: &[&str] = &[
    TEXT_SAVE,
    TEXT_SAVING,
    TEXT_RESET_DEFAULT,
    TEXT_PAGE_NOTE,
    TEXT_READONLY_NOTE,
    TEXT_INVALID_VALUE,
    TEXT_LISTEN_ADDR,
    TEXT_LOOPBACK_ADDR,
    TEXT_DIALOG_TITLE_SAVE,
    TEXT_IMPACT_SAVE,
    TEXT_IMPACT_RESET,
    TEXT_TOAST_OK,
    TEXT_TOAST_FAIL,
    TEXT_TOAST_FULL_REWRITE,
    TEXT_AUDIT_UNAVAILABLE,
    TEXT_CONFIG_UNAVAILABLE,
    TEXT_RANGE_SEP,
    ICON_OK,
    ICON_FAIL,
    ICON_WARN,
];

// ═══════════════════════════════════════════════════════════════════════════
// 2. 栅格常量（UI §6.2；**全部由 theme 常量推导** —— 本页不出现裸规格值）
// ═══════════════════════════════════════════════════════════════════════════

/// 卡内容区原点相对卡外缘的偏移（描边 1 + 内边距 16）—— 与 `p6_system.rs` 同口径（见 PD4）。
const CARD_INSET: i32 = theme::Stroke::THIN + Dimens::GAP_MIN;
/// 卡内可用宽。
const INNER_W: i32 = Dimens::CONTENT_W - 2 * CARD_INSET;
/// 卡头高（UI §6.2「卡头 44 px（分组名 28 px + 左侧 4 px 竖条）」= 28 + 同组缝 16）。
const CARD_HEAD_H: i32 = TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
/// 行型 A 高（UI §6.2「行型 A … 88 px」= 控件高 64 + 约束提示行 24）。
const ROW_A_H: i32 = Dimens::STEPPER_H + TextSlot::Body.px() as i32;
/// 行型 A 文本块（字段名 + 提示行）在行内的 y（垂直居中）。
const ROW_A_TEXT_Y: i32 = theme::center_offset(
    ROW_A_H,
    TextSlot::Label.px() as i32 + TextSlot::Body.px() as i32,
);
/// 行型 A 控件 y（行内垂直居中）。
const ROW_A_CTRL_Y: i32 = theme::center_offset(ROW_A_H, Dimens::STEPPER_H);
/// 行型 B 高（UI §6.2「行型 B … 120 px」；实算 122，见 **PD5**）。
const ROW_B_H: i32 = Dimens::GAP_MIN + TextSlot::Label.px() as i32 + Dimens::GAP_MIN + Dimens::STEPPER_H;
/// 行型 B 字段名 y（UI §6.2「字段名 26 px (x36, y+16)」）。
const ROW_B_LABEL_Y: i32 = Dimens::GAP_MIN;
/// 行型 B 控件 y（字段名行 + 同组缝）。
const ROW_B_CTRL_Y: i32 = ROW_B_LABEL_Y + TextSlot::Label.px() as i32 + Dimens::GAP_MIN;
/// 行型 B 控件 x（**负偏移**：抵消卡内边距 ⇒ 控件与卡外缘齐宽，UI §6.2「控件独占次行」）。
///
/// 为何必须负偏移：`Ipv4Stepper` 整件宽 = [`Dimens::CONTENT_W`]（=`ui/controls.rs` 的 CD2），
/// 而卡内容区只有 [`INNER_W`]（= 958）⇒ 若从 0 起排，右端 34 px 会被父对象裁剪（LVGL 子对象
/// 裁剪到父的**外缘**坐标，见 `ui/tests.rs::pages_chain` 的同类口径）。
const ROW_B_CTRL_X: i32 = -CARD_INSET;
/// 状态槽宽（约束提示 / 字段错误原因；两者**同槽互斥**，避免出错时整页重排）。
const STATUS_W: i32 = INNER_W / 2;
/// 状态槽 x（行型 B：与字段名**同行右侧**；行型 A 用 0）。
const STATUS_X: i32 = Dimens::CONTENT_W / 3;
/// 字段行左缘危险竖条宽（复用 `theme` 的"卡头 4 px 竖条"口径；见 **PD6**）。
const ERROR_BAR_W: i32 = Dimens::ACCENT_BAR;
/// 固定操作条高（UI §6.2 线框 `y 624–696` ⇒ 72 = 主按钮高 64 + 内容区上内边距 8）。
const ACTION_BAR_H: i32 = Dimens::BTN_H_PRIMARY + Dimens::CONTENT_PAD_TOP;
/// 滚动视口高（UI §6.2「552 px 视口」= 内容区 624 − 操作条 72）。
const SCROLL_H: i32 = Dimens::CONTENT_H - ACTION_BAR_H;
/// 操作条内左右内边距。
const ACTION_PAD: i32 = Dimens::GAP_MIN;
/// 操作条内按钮 y（垂直居中）。
const ACTION_BTN_Y: i32 = theme::center_offset(ACTION_BAR_H, Dimens::BTN_H_PRIMARY);
/// 分组卡之间的缝（跨区 24，UI §3.5「区块间呼吸缝」）。
const CARD_GAP: i32 = Dimens::GAP_SECTION;
/// 页说明行高（正文 24 + 下缝 16）。
const NOTE_H: i32 = TextSlot::Body.px() as i32 + Dimens::GAP_MIN;
/// 页说明行 y（UI §6.2 线框 `Y80` ⇒ 页内 = 8 = 内容区上内边距）。
const NOTE_Y: i32 = Dimens::CONTENT_PAD_TOP;
/// 首个分组卡 y。
const FIRST_CARD_Y: i32 = NOTE_Y + NOTE_H;
/// 单个 `Stepper` 整件宽（`−` + 值区 + `＋`；UI §5.1 #6）。
const STEPPER_TOTAL_W: i32 = Dimens::STEPPER_BTN_W * 2 + Dimens::STEPPER_VALUE_W;
/// `Enum` **可容段数上限**（UI §5.1 #4「段宽均分，最小 96」⇒ 卡内容区里最多几段）。
///
/// 实算 `INNER_W / SEGMENT_MIN_W = 958 / 96 = 9`（`9 × 96 = 864 ≤ 958`；`10 × 96 = 960 > 958`
/// —— **10 段正是评审实测触发"整页 `Err`"的那一档**）。超出上限的选项走 **PD16** 的窗口截断
/// （**字段级降级，不得整页 `Err`**）。
const ENUM_MAX_SEGMENTS: usize = (INNER_W / crate::ui::controls::SEGMENT_MIN_W) as usize;

// ═══════════════════════════════════════════════════════════════════════════
// 3. 纯逻辑（**不触碰 LVGL** ⇒ 可独立单测；页内逻辑一律经这里，保证可离线复现）
// ═══════════════════════════════════════════════════════════════════════════

/// 一个字段的**变更明细**（弹层「将修改的字段」段的原始材料，见 `ConfirmDetail`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangeDetail {
    /// 稳定字段键（审计 `target`）。
    pub key: String,
    /// 上屏字段名（[`field_label_text`] 的结果）。
    pub label: String,
    /// 旧值 / 当前值（`text_weak`）。
    pub before: String,
    /// 新值（`#2FDB8A`）。
    pub after: String,
}

/// 遍历视图内全部字段（组序 × 组内序）—— 唯一的口径，避免各处再写嵌套循环。
pub(crate) fn iter_fields(view: &ConfigView) -> impl Iterator<Item = &ConfigField> {
    view.groups.iter().flat_map(|g| g.fields.iter())
}

/// 分组名的**上屏文本**（契约 `ConfigGroup.label` 的自由文本 → 安全改写）。
///
/// 与 [`field_label_text`] / `Enum` 选项**同一处理**：契约标签不在 `ui/**` 字面量走查面内
/// （它们来自后端 / 运行时帧），直上屏时含 ASCII `-` / 小写即豆腐块 ⇒ 一律过 [`display_safe`]
/// （**同类数据同一处理**，见 **PD13**；其**非 ASCII 缺字挡不住**的残余风险同样见 PD13）。
pub(crate) fn group_label_text(group: &ConfigGroup) -> String {
    display_safe(&group.label)
}

/// 字段的**上屏标签**（设计 §6.2「监听地址字段口径」PM 裁定行 + UI 附录 B U-1）。
///
/// **契约的 `label` 是默认路径**（元数据驱动 UI）；仅当键命中**PM 裁定表**
/// （[`LABEL_OVERRIDES`]）时改用它 —— 目的是让"不得表述为『对端 IP / 远程主站地址』"这条
/// 裁定在**屏上**成立（后端标签漂移时页仍不违规）。
///
/// 两条路径**最后都过 [`display_safe`]**（与 `Enum` 选项 / [`group_label_text`] 同口径）：
/// 注入标签是自由文本，含 cmap 外 ASCII（`-` / 小写）时真机是豆腐块（**PD13**）。
/// `LABEL_OVERRIDES` 的覆盖串本身逐字在 cmap 内 ⇒ 改写对它们是**恒等**。
pub(crate) fn field_label_text(field: &ConfigField) -> String {
    let raw = LABEL_OVERRIDES
        .iter()
        .find(|(key, _)| *key == field.key)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| field.label.clone());
    display_safe(&raw)
}

/// 配置字段的**机器键**（**不上屏** —— 只用于与 `ConfigField.key` 比对）。
///
/// 存在的唯一理由：`ui/tests.rs::ui_texts_covered_by_font_cmap` 把 `ui/**` 里**所有**字符串
/// 字面量一律当"上屏候选"逐字查字形（宁可多查），而机器键是小写 ASCII（`gateway.listen_addr`
/// 里的 `g`/`a`/`t`/`e`/`w` 等**在生成字体里没有字形**）。经本函数标注 = 声明"这个字面量
/// **从不进 `lv_label`**"，与 `invalid_argument(` / `env!(` 一类**同一条**非屏显口径
/// （见 `ui/tests.rs::NON_DISPLAY_SINKS`；那里的登记是**逐条列出、不靠正则猜**）。
///
/// ⚠️ 它不是"为了过网而把文案写残"：键本来就**不上屏**，此处只是把这一事实**写在调用点**。
const fn config_key(key: &'static str) -> &'static str {
    key
}

/// PM 裁定的标签口径覆盖表（键 → 上屏标签）—— **逐条登记、只增不改**。
///
/// | 键 | 标签 | 出处 |
/// |----|------|------|
/// | `gateway.listen_addr` | [`TEXT_LISTEN_ADDR`] | 设计 §6.2 PM 裁定行（R-08 / U-1） |
/// | `display.bind_addr` | [`TEXT_LOOPBACK_ADDR`] | 设计 §6.2 只读字段行（PL-4 回环红线） |
/// | `display.control_bind_addr` | [`TEXT_LOOPBACK_ADDR`] | 同上 |
pub const LABEL_OVERRIDES: [(&str, &str); 3] = [
    (config_key("gateway.listen_addr"), TEXT_LISTEN_ADDR),
    (config_key("display.bind_addr"), TEXT_LOOPBACK_ADDR),
    (config_key("display.control_bind_addr"), TEXT_LOOPBACK_ADDR),
];

/// 值 → 上屏文本（弹层明细与断言口径的**唯一**出口）。
///
/// **合法值**（`kind.validate_value(value).is_ok()`）⇒ 按 kind 格式化；**否则** ⇒ [`PLACEHOLDER`]。
///
/// **为何"非法即占位"**（**C1**）：旧实现在整数分支用 `int_of(value).unwrap_or_default()` ⇒ 类型错配
/// 被兜底成 **`0`**，而 `0` **本身是合法配置值** ⇒ "值不合法"被**伪装**成"值为 0"（屏上无法分辨，
/// 违反"降级可见、绝不造假"）。占位符 `–` 的语义（PRD F1.4 / F3.4 / F4.3「显 `--`，**严禁补 0**」）
/// 正是为这一类情形而设。
///
/// **出口统一过 [`display_safe`]**（**I3**）：本函数的产物**直进** `ConfirmDialog` 的
/// `before` / `after`（`components.rs` 用 `text_label` 上屏，**不经** `display_safe`）⇒ 小写机器值
/// （如 `trace` 的 `t`/`r`/`a`/`c`/`e`）在真机是豆腐块。合法值里可能出现的字符（数字、`.`、`%`、`·`、
/// 选项标签的中文）**都在** [`ASCII_DISPLAY_ALPHABET`](crate::ui::pages::ASCII_DISPLAY_ALPHABET)
/// 或 cmap 内 ⇒ `display_safe` 对合法路径是**恒等**（由
/// `format_value_is_identity_safe_for_legal_values` 逐例锁住）。
///
/// - `Ipv4`：点分十进制；
/// - `U16`/`U64`：十进制整数 + 可选单位（单位由契约给，如 `秒`）；
/// - `Enum`：命中选项 → **选项标签**；未命中 ⇒ [`PLACEHOLDER`]（**不**回显机器值）。
pub(crate) fn format_value(kind: &ConfigKind, value: &Value, unit: Option<&str>) -> String {
    // 合法性先判：三类 kind 的"无法格式化"**统一**收敛到占位符（不合情况各写各的兜底）。
    if kind.validate_value(value).is_err() {
        return PLACEHOLDER.to_string();
    }
    let raw = match kind {
        ConfigKind::Ipv4 => value.as_str().unwrap_or_default().to_string(),
        ConfigKind::U16 { .. } | ConfigKind::U64 { .. } => {
            let n = int_of(value).unwrap_or_default();
            match unit.filter(|u| !u.is_empty()) {
                Some(u) => format!("{n} {u}"),
                None => n.to_string(),
            }
        }
        // 已过校验 ⇒ `find` 必命中；兜底仍取占位符（**不**回显机器值）。
        ConfigKind::Enum { options } => options
            .iter()
            .find(|o| o.value == value.as_str().unwrap_or_default())
            .map(|o| o.label.clone())
            .unwrap_or_else(|| PLACEHOLDER.to_string()),
    };
    display_safe(&raw)
}

/// 约束提示（UI §6.2 行型 A「字段名下方 24 px `text_weak` 显示约束『1 – 65535』」）。
///
/// `Ipv4` / `Enum` 返回 `None`：UI §6.2 的行型 B 与 Enum 行**不画**约束提示
/// （四段 0–255 与选项本身已由控件表达）。
pub(crate) fn range_hint(kind: &ConfigKind, unit: Option<&str>) -> Option<String> {
    let (lo, hi) = match kind {
        ConfigKind::U16 { min, max, .. } => (u64::from(*min), u64::from(*max)),
        ConfigKind::U64 { min, max, .. } => (*min, *max),
        ConfigKind::Ipv4 | ConfigKind::Enum { .. } => return None,
    };
    let base = format!("{lo}{TEXT_RANGE_SEP}{hi}");
    Some(match unit.filter(|u| !u.is_empty()) {
        Some(u) => format!("{base} {u}"),
        None => base,
    })
}

/// 注入视图 → **当前值**快照（`key → value`）。
///
/// **`#[cfg(test)]`**（**M5 取证**：本文件外零引用，生产侧也不需要"视图 → 快照"这一步 ——
/// 生产读的是**控件**，见 [`Core::current_values`]）⇒ 编译期即不进入产物。
#[cfg(test)]
pub(crate) fn initial_values(view: &ConfigView) -> BTreeMap<String, Value> {
    iter_fields(view)
        .map(|f| (f.key.clone(), f.value.clone()))
        .collect()
}

/// 草稿的**门控集合**（**C1**）：只有「用户经控件真的改过」**且**「注入值合法」的键才参与
/// `draft()` / `is_dirty()` / 明细列表的差异计算。
///
/// 两个集合各自的职责（**缺一不可**）：
///
/// - [`touched`](Self::touched) —— **注入本身永不置脏**。注入的非法值会被控件渲染成"最小可表示值"
///   （`Enum` ⇒ `options[0]`、`U16`/`U64` ⇒ `min`，见 **PD21**）；若按"值与视图不同即脏"判定，
///   页面**未经过任何用户操作**就已经是脏的、保存按钮**可用** ⇒ 一次点击即把屏上**从未展示过**的
///   值写进装置（评审实测的 C1）；
/// - [`invalid`](Self::invalid) —— 注入值**不合法**（`kind.validate_value` 拒绝）⇒ 该行错误态 +
///   控件 `disabled` + **该键不进任何补丁**（既不进草稿，也不进"恢复默认值"）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DraftScope {
    /// 用户经控件改动过的键（**注入永不入内**）。
    pub(crate) touched: BTreeSet<String>,
    /// 注入值不合法（`kind.validate_value` 拒绝）的键。
    pub(crate) invalid: BTreeSet<String>,
}

impl DraftScope {
    /// 该键是否可参与差异计算（**两个门**：用户改过 **且** 注入值合法）。
    pub(crate) fn admits(&self, key: &str) -> bool {
        self.touched.contains(key) && !self.invalid.contains(key)
    }
}

/// 草稿补丁：**只含用户真改过**（[`DraftScope::touched`]）、**值合法**（非 `invalid`）**且与视图不同**
/// 的字段（`from = Edit`）。
///
/// "不同"由 `serde_json::Value` 的相等判定（整数一律 `PosInt`，故 `2404` 与 `2404u64` 相等）。
pub(crate) fn draft_patch(
    view: &ConfigView,
    current: &BTreeMap<String, Value>,
    scope: &DraftScope,
) -> ConfigPatch {
    let mut changes = JsonMap::new();
    for f in iter_fields(view) {
        if !scope.admits(&f.key) {
            continue;
        }
        let Some(now) = current.get(&f.key) else {
            continue;
        };
        if *now != f.value {
            changes.insert(f.key.clone(), now.clone());
        }
    }
    ConfigPatch {
        changes,
        from: PatchSource::Edit,
    }
}

/// 是否有未保存修改（= 草稿非空）。口径见 [`DraftScope`]（**注入永不置脏**）。
pub(crate) fn is_dirty(
    view: &ConfigView,
    current: &BTreeMap<String, Value>,
    scope: &DraftScope,
) -> bool {
    !draft_patch(view, current, scope).changes.is_empty()
}

/// 恢复默认值补丁：**全部 `editable` 且注入值合法**的字段 → 其 `default`（`from = ResetDefault`）。
///
/// **只读字段一律不进 `changes`**（见 **PD12**）：契约 [`ConfigField::validate_value`] 对
/// `editable=false` **一律拒绝**，混进去会让整个恢复请求被后端二次校验打回。
///
/// **注入值非法的字段同样不进**（**C1**）：它的元数据与当前值已自相矛盾（该行错误态 + 控件 `disabled`），
/// 替它写 `default` 属"屏上不可核查的写入"—— 与"降级可见、绝不造假"冲突。
pub(crate) fn defaults_patch_of(view: &ConfigView, invalid: &BTreeSet<String>) -> ConfigPatch {
    let mut changes = JsonMap::new();
    for f in iter_fields(view).filter(|f| f.editable && !invalid.contains(&f.key)) {
        changes.insert(f.key.clone(), f.default.clone());
    }
    ConfigPatch {
        changes,
        from: PatchSource::ResetDefault,
    }
}

/// 视图内**是否存在** `requires_reconnect` 字段 —— **视图口径**，**仅测试对照用**。
///
/// ⚠️ **按 PD11 明令禁止生产分级使用它**：视图里"存在"瞬断字段 ≠ 本次改动会瞬断链路。
/// 真实字段表含 `gateway.listen_addr` / 核间端口（设计 §4.3.3）⇒ 视图口径下 **L1 永不可达**、
/// `WarnBanner` 恒亮（"降级可见、绝不造假"被违反）。生产路径一律用按键集合的 [`reconnect_in`]。
///
/// 以 `#[cfg(test)]` 收口 ⇒ **结构上不可能**被生产代码调用（防止口径回流）；它在测试里的
/// 唯一用途是把"视图口径"与"改动口径"的**差集**写成断言。
#[cfg(test)]
pub(crate) fn has_reconnect_field(view: &ConfigView) -> bool {
    iter_fields(view).any(|f| f.requires_reconnect)
}

/// **本次改动**是否触及任何 `requires_reconnect` 字段 —— **唯一的分级判据**（PD11）。
///
/// `changed` = **本次补丁的键集合**：
/// - 保存：`draft_patch(view, current).changes.keys()`（只含**值与视图不同**的字段）；
/// - 恢复默认值：`defaults_patch_of(view).changes.keys()`（= 全部 `editable` 字段，PD12 排除只读）。
///
/// # 为什么不能用"视图内全部字段"（三条，逐条对应设计）
///
/// 1. 设计 §6.2 流程 3 的示例是「`端口：2404 → 2405`」+「**涉及：端口**」—— 同视图内还有
///    `监听地址`（§4.3.3 明列 `requires_reconnect=true`）却不进「涉及：」⇒ **只有**本次改动口径
///    能让该示例成立；
/// 2. §2.5 两行都写「**含**连接类字段的配置保存」（"含" = **本次**含），§7.3 明细段标题是
///    「**将修改的字段**」；
/// 3. §2.6 铁律「降级可见、**绝不造假**」—— 只改日志级别却弹「生效瞬间通信将短暂中断」是
///    **谎报副作用**。
///
/// # 恢复默认值路径的等价性论证（**改动口径才正确**在何处）
///
/// 当且仅当**所有** `requires_reconnect` 字段都 `editable == true` 时，"视图口径" ≡ "改动口径"
/// —— 因为 [`defaults_patch_of`] 覆盖**全部** `editable` 字段。一旦存在**只读的**瞬断字段
/// （例如 `display.bind_addr` 被后端标成 `requires_reconnect=true`：设计 §6.2 明列它是只读的
/// 安全红线字段），**视图口径会把它列进「涉及：」并把分级抬到 L2+，而它既不会进补丁、也不会
/// 经屏生效** ⇒ 视图口径在此**必然错**（谎报一个改不动的字段会瞬断链路）。改动口径取的是
/// `defaults_patch_of` 的键集合，只读字段天然被 PD12 排除在外 ⇒ 不会列出它。
pub(crate) fn reconnect_in<'a, I>(view: &ConfigView, changed: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    let keys: BTreeSet<&str> = changed.into_iter().collect();
    iter_fields(view).any(|f| f.requires_reconnect && keys.contains(f.key.as_str()))
}

/// 保存的确认强度（设计 §6.2 保存行 / UI §2.5）：**本次改动**不含瞬断字段 ⇒
/// [`ConfirmLevel::L1`]（单击生效）；**含**任一 `requires_reconnect` 字段 ⇒
/// [`ConfirmLevel::L2Plus`]（危险色 + 长按 1.0 s + **必出 `WarnBanner`**）。
///
/// `changed` 的口径见 [`reconnect_in`]（PD11：**不是**"视图内全部字段"）。
pub(crate) fn save_level<'a, I>(view: &ConfigView, changed: I) -> ConfirmLevel
where
    I: IntoIterator<Item = &'a str>,
{
    if reconnect_in(view, changed) {
        ConfirmLevel::L2Plus
    } else {
        ConfirmLevel::L1
    }
}

/// 恢复默认值的确认强度（设计 §6.2 恢复默认值行）：**最低 L2**（生效性写、不得只有间距保护）；
/// **本次改动**触及 `requires_reconnect` 字段时升为 [`ConfirmLevel::L2Plus`]（追加 `WarnBanner`）。
pub(crate) fn reset_level<'a, I>(view: &ConfigView, changed: I) -> ConfirmLevel
where
    I: IntoIterator<Item = &'a str>,
{
    if reconnect_in(view, changed) {
        ConfirmLevel::L2Plus
    } else {
        ConfirmLevel::L2
    }
}

/// L2+ 的「涉及：<字段名列表>」—— **本次改动中** `requires_reconnect` 字段的上屏标签（UI §2.5）。
///
/// 键集合口径同 [`reconnect_in`]（PD11）：既只列**改动的**，也只列**真瞬断的**。
pub(crate) fn reconnect_field_labels<'a, I>(view: &ConfigView, changed: I) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let keys: BTreeSet<&str> = changed.into_iter().collect();
    iter_fields(view)
        .filter(|f| f.requires_reconnect && keys.contains(f.key.as_str()))
        .map(field_label_text)
        .collect()
}

/// 保存路径的变更明细（逐字段「旧值 → 新值」，UI §7.3 明细列表）。
pub(crate) fn save_details(
    view: &ConfigView,
    current: &BTreeMap<String, Value>,
    scope: &DraftScope,
) -> Vec<ChangeDetail> {
    let mut out = Vec::new();
    for f in iter_fields(view) {
        // 口径与 [`draft_patch`] **逐条一致**（否则"明细列了、补丁没写"= 谎报）：用户没改过的、
        // 注入值非法的，都不进明细。
        if !scope.admits(&f.key) {
            continue;
        }
        let Some(now) = current.get(&f.key) else {
            continue;
        };
        if *now == f.value {
            continue;
        }
        out.push(ChangeDetail {
            key: f.key.clone(),
            label: field_label_text(f),
            before: format_value(&f.kind, &f.value, f.unit.as_deref()),
            after: format_value(&f.kind, now, f.unit.as_deref()),
        });
    }
    out
}

/// 恢复默认值路径的变更明细（「字段：当前值 → 默认值」；范围与 [`defaults_patch_of`] **逐条一致**）。
///
/// **注入值非法的字段不进明细**（**C1**）：它们的 `default` 不会进 [`defaults_patch_of`] 的
/// `changes` ⇒ 列在弹层里就是**谎报**（"要改"而实际不改）。
pub(crate) fn reset_details(
    view: &ConfigView,
    current: &BTreeMap<String, Value>,
    invalid: &BTreeSet<String>,
) -> Vec<ChangeDetail> {
    let mut out = Vec::new();
    for f in iter_fields(view).filter(|f| f.editable && !invalid.contains(&f.key)) {
        let Some(now) = current.get(&f.key) else {
            continue;
        };
        out.push(ChangeDetail {
            key: f.key.clone(),
            label: field_label_text(f),
            before: format_value(&f.kind, now, f.unit.as_deref()),
            after: format_value(&f.kind, &f.default, f.unit.as_deref()),
        });
    }
    out
}

/// 配置不可用时的提示文本（`reason` 为空则只显标题）。
pub(crate) fn unavailable_text(reason: &str) -> String {
    let r = display_safe(reason.trim());
    if r.is_empty() {
        TEXT_CONFIG_UNAVAILABLE.to_string()
    } else {
        format!("{TEXT_CONFIG_UNAVAILABLE} · {r}")
    }
}

/// `Value` → `i64`（`U16`/`U64` 两类整数；负数与越界由 `Stepper` 自身 clamp）。
fn int_of(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().and_then(|u| i64::try_from(u).ok()))
        .or_else(|| v.as_f64().map(|f| f as i64))
}

/// 字段的初始四段（`Ipv4`）：值不可解析时退回 `default`，两者都不可解析才退回 `0.0.0.0`
/// （**不静默**：往 stderr 留一条诊断）。
fn octets_of(field: &ConfigField) -> [u8; 4] {
    let parse = |v: &Value| {
        v.as_str()
            .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
            .map(|a| a.octets())
    };
    parse(&field.value)
        .or_else(|| parse(&field.default))
        .unwrap_or_else(|| {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "P2 配置页：IPv4 字段 `{}` 的值与默认值都无法解析，按 0.0.0.0 显示（不静默）",
                field.key
            );
            [0, 0, 0, 0]
        })
}

/// 字段的初始整数值（`U16`/`U64`）；不可得时取 `default`，再不可得取 0。
fn initial_int(field: &ConfigField) -> i64 {
    int_of(&field.value)
        .or_else(|| int_of(&field.default))
        .unwrap_or_default()
}

/// `Enum` 值 → 选项下标（未命中取 0）。
fn enum_index(kind: &ConfigKind, v: &Value) -> usize {
    if let ConfigKind::Enum { options } = kind {
        if let Some(s) = v.as_str() {
            if let Some(i) = options.iter().position(|o| o.value == s) {
                return i;
            }
        }
    }
    0
}

/// 行高（由 `kind` 驱动，UI §6.2 的两版式）。
fn row_height(kind: &ConfigKind) -> i32 {
    match kind {
        ConfigKind::Ipv4 => ROW_B_H,
        _ => ROW_A_H,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 字段行 / 分组卡（**拥有型句柄的存活锚点**，见模块文档「所有权纪律」）
// ═══════════════════════════════════════════════════════════════════════════

/// 一个字段的输入控件（三类，由 `ConfigField.kind` 驱动）。
enum FieldControl {
    /// `Ipv4` → 四段步进 + 汇总标签（UI §5.1 #7）。
    Ipv4(Rc<Ipv4Stepper>),
    /// `U16`/`U64` → 受约束步进器（UI §5.1 #6）。
    Int(Rc<Stepper>),
    /// `Enum` → 分段控件（UI §5.1 #4；**不用 `lv_dropdown`** —— §5.1 未选它，且其展开列表
    /// 在离屏不可断言）。
    ///
    /// `start` / `shown` = 本控件渲染的是**选项窗口** `[start, start + shown)`（**PD16**：选项数
    /// 超出 [`ENUM_MAX_SEGMENTS`] 时只渲染一个**含当前值**的窗口，而不是整页 `Err`）。
    /// `shown == 0` = **空选项的占位行**（单段 [`PLACEHOLDER`] + 恒禁用）—— 见 `enum_view`。
    Enum {
        /// 分段控件句柄。
        ctrl: Rc<SegmentedControl>,
        /// 窗口在**契约选项表**里的起始下标（段下标 = 契约下标 − `start`）。
        start: usize,
        /// 窗口段数（**可寻址**的段数；`0` = 占位行）。
        shown: usize,
    },
}

impl FieldControl {
    /// 当前值（**读自控件** —— 唯一真源，不另存影子副本）。
    fn current(&self, kind: &ConfigKind) -> Value {
        match self {
            Self::Ipv4(s) => Value::from(ipv4_text(s.octets())),
            Self::Int(s) => Value::from(s.value()),
            Self::Enum { ctrl, start, .. } => {
                let idx = ctrl.raw_selected().unwrap_or_else(|| ctrl.selected());
                let raw = match kind {
                    ConfigKind::Enum { options } => options
                        .get(start + idx)
                        .map(|o| o.value.clone())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                Value::from(raw)
            }
        }
    }

    /// 编程式设值（**不触发 `on_change`** —— `Stepper` / `Ipv4Stepper` / `SegmentedControl`
    /// 的 `set_*` 都是这个语义，故调用方必须自己做变更后的簿记）。
    ///
    /// **窗口外的值不动控件**（宁可保持原值，也**不**静默改写成另一个值 —— "绝不造假"）：
    /// 生产路径不会走到（[`Core::discard`] 回退的是 [`FieldRow::initial`]，而窗口正是按注入值
    /// 选出来的；[`Core::rebind_scope`] 亦按同一口径设值）。
    fn set(&self, kind: &ConfigKind, v: &Value) {
        match self {
            Self::Ipv4(s) => s.set_octets(octets_of_value(kind, v)),
            Self::Int(s) => s.set_value(int_of(v).unwrap_or_default()),
            Self::Enum {
                ctrl,
                start,
                shown,
            } => {
                if let Some(local) = enum_index(kind, v)
                    .checked_sub(*start)
                    .filter(|l| *l < *shown)
                {
                    ctrl.set_selected(local);
                }
            }
        }
    }

    /// 显式禁用 / 恢复。
    fn set_disabled(&self, on: bool) {
        match self {
            Self::Ipv4(s) => s.set_disabled(on),
            Self::Int(s) => s.set_disabled(on),
            Self::Enum { ctrl, .. } => ctrl.set_disabled(on),
        }
    }

    /// 控件当前是否禁用（**读自 LVGL**，不是本页自己的标志位）。
    ///
    /// `Ipv4Stepper` 没有整件的 `is_disabled()` 读回口（`ui/controls.rs` 本批禁改）⇒ 取第 0 段
    /// 的读回值（四段由 [`Ipv4Stepper::set_disabled`] 一次全设，取一段即整件）。
    ///
    /// **`#[cfg(test)]`**：`Ipv4Stepper::segment` 本身只在测试构建下提供（同因），且生产侧
    /// 不需要"读回控件禁用态"（置位才是生产需求）。
    #[cfg(test)]
    fn is_disabled(&self) -> bool {
        match self {
            Self::Ipv4(s) => s.segment(0).map(Stepper::is_disabled).unwrap_or(false),
            Self::Int(s) => s.is_disabled(),
            Self::Enum { ctrl, .. } => ctrl.is_disabled(),
        }
    }

    /// 当前值区文字（离屏断言口径）。
    fn display(&self) -> Option<String> {
        match self {
            Self::Ipv4(s) => s.text(),
            Self::Int(s) => s.display(),
            // ⚠️ `option(i)` 的 `i` 是**控件内**的按钮下标（`lv_buttonmatrix_get_button_text`），
            // **不是**契约选项表下标 ⇒ 这里**不得**再加窗口偏移 `start`。
            Self::Enum { ctrl, .. } => {
                let idx = ctrl.raw_selected().unwrap_or_else(|| ctrl.selected());
                ctrl.option(idx)
            }
        }
    }
}

/// 一个字段行（`UI §6.2` 的两种版式共用本结构，差别只在坐标）。
struct FieldRow {
    /// 稳定字段键。
    key: String,
    /// 控件类型（驱动取值 / 默认值 / 设值）。
    kind: ConfigKind,
    /// 注入时该字段的值（**唯一旧值真源**：脏判定与"放弃修改"回退都用它）。
    initial: Value,
    /// 字段名标签（离屏断言口径）。
    label: Label,
    /// 字段名两态样式（[0] 正常 / [1] 危险红）。
    label_styles: [Rc<Style>; 2],
    label_style: Cell<usize>,
    /// 状态槽：约束提示 **或** 字段错误原因（同槽互斥 ⇒ 出错不重排）。
    status: Label,
    /// 状态槽两态样式（[0] `text_weak` / [1] 危险红）。
    status_styles: [Rc<Style>; 2],
    status_style: Cell<usize>,
    /// 只读字段的说明行（`editable == false` 才有；与状态槽同坐标，二者互斥显示）。
    note: Option<Label>,
    /// 行左缘危险竖条（隐藏态；字段错误时显形 —— 见 **PD6**）。
    error_bar: Obj,
    /// 输入控件。
    control: FieldControl,
    /// 约束提示文本（`Ipv4` / `Enum` 为 `None` —— UI §6.2 不画）。
    hint: Option<String>,
}

impl FieldRow {
    /// 按当前错误态刷新本行的视觉（`Some(reason)` = 标红 + 就地表原因）。
    fn refresh_error(&self, reason: Option<&str>) {
        match reason {
            Some(r) => {
                self.error_bar.set_hidden(false);
                set_style_index(self.label.obj(), &self.label_styles, &self.label_style, 1);
                set_style_index(self.status.obj(), &self.status_styles, &self.status_style, 1);
                self.status.set_text(r);
                set_visible(self.status.obj(), true);
                if let Some(n) = &self.note {
                    set_visible(n.obj(), false);
                }
            }
            None => {
                self.error_bar.set_hidden(true);
                set_style_index(self.label.obj(), &self.label_styles, &self.label_style, 0);
                set_style_index(self.status.obj(), &self.status_styles, &self.status_style, 0);
                // 无错误 ⇒ 恢复"提示 / 只读说明 / 都不显示"三取一。
                if let Some(n) = &self.note {
                    n.set_text(TEXT_READONLY_NOTE);
                    set_visible(n.obj(), true);
                    set_visible(self.status.obj(), false);
                } else if let Some(hint) = &self.hint {
                    self.status.set_text(hint);
                    set_visible(self.status.obj(), true);
                } else {
                    set_visible(self.status.obj(), false);
                }
            }
        }
    }
}

/// 一个分组卡（卡头 + 字段行 + 分隔线）。
struct GroupCard {
    /// 卡对象（**拥有型**：`Drop` 即 `lv_obj_delete`，级联删除整棵子树 ⇒ 换视图 = 换句柄）。
    /// **仅为保持子树存活**（本页不直接读卡对象；`Deref` 亦不需要）。
    _obj: Obj,
    /// 分组名标签（离屏断言口径）。
    label: Label,
    /// 卡头竖条与行分隔线（**仅为保持子树存活** —— 局部变量随函数返回被 `Drop` ⇒ 屏幕上少一根线）。
    _decor: Vec<Obj>,
    /// 组内字段行（含控件句柄）。
    rows: Vec<FieldRow>,
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 页面核心（**共享可变态**：控件回调经 `Weak<Core>` 回访，避免 `Rc` 环）
// ═══════════════════════════════════════════════════════════════════════════

/// 页面的共享核心：控件回调只持有它的 `Weak`（强引用在 [`P2ConfigPage`] 上）——
/// `Stepper → 回调闭包 → Rc<Core> → Stepper` 会成环，句柄永不落地。
struct Core {
    /// 页根容器（`992 × 624`，**不滚动** —— 见模块文档第 2 条）。
    root: Obj,
    /// 滚动视口（`992 × 552`）。
    scroll: ScrollContainer,
    /// 固定操作条（`992 × 72`，不随滚动）。
    action_bar: Obj,
    /// 保存按钮（主按钮）。
    save: Rc<TextButton>,
    /// 恢复默认值按钮（危险按钮，与保存间距 ≥ [`Dimens::GAP_DANGER`]）。
    reset: Rc<TextButton>,
    /// 页说明行（常态文案）。
    note: Label,
    /// 失败 / 降级原因行（与 `note` **同槽互斥**；见 **PD7**）。
    fail: Label,
    /// 当前视图产生的分组卡（换视图即整体替换 ⇒ 旧句柄 `Drop` ⇒ 级联删子树）。
    cards: RefCell<Vec<GroupCard>>,
    /// 注入的视图快照（`None` = 尚未加载 / 不可用）。
    view: RefCell<Option<ConfigView>>,
    /// 后端二次校验的**逐字段**原因（`key → reason`；CF-02）。
    errors: RefCell<BTreeMap<String, String>>,
    /// **用户经控件改动过**的键集合（**C1**：注入本身永不置脏 —— 见 [`DraftScope`]）。
    touched: RefCell<BTreeSet<String>>,
    /// **注入值不合法**（`kind.validate_value` 拒绝）的键集合（**C1**：该行进错误态 + 控件
    /// `disabled` + 该键不进任何补丁）。
    invalid: RefCell<BTreeSet<String>>,
    /// 提交中（F9.6：保存按钮 `disabled` + 文案「保存中...」）。
    submitting: Cell<bool>,
    /// 配置是否可用（控制通道注入成功）。
    available: Cell<bool>,
    /// **延迟关闭弹层**的标志（`ConfirmDialog::close` 不得在 LVGL 事件回调内调用
    /// —— 取消按钮的回调只置本标志，真正的关闭在 [`P2ConfigPage::tick`] 里做）。
    pending_close: Cell<bool>,
    /// 当前打开的确认弹层（同一时刻至多 1 个）。
    dialog: RefCell<Option<ConfirmDialog>>,
    /// 当前 Toast（同一时刻至多 1 条，UI §7.2）。
    toast: RefCell<Option<Toast>>,
    /// 「提交意图」回调（确认完成时调用一次；**本页不生成 `request_id`**）。
    on_submit: SubmitSlot,
}

/// 提交意图回调槽：`RefCell<Option<Box<dyn FnMut(ConfigPatch, ConfirmLevel)>>>`。
///
/// 语义同 `ui/components.rs` 的三个 `*Callback` 别名：`Option` = "尚未接线"（未接线即 no-op）、
/// `Box<dyn FnMut>` 让调用方传任意捕获闭包。**不出现在任何 `pub` 签名里**（对外入口是泛型
/// [`P2ConfigPage::set_on_submit`]），故不导出；起别名只为让 [`Core`] 的字段类型可读。
type SubmitSlot = RefCell<Option<Box<dyn FnMut(ConfigPatch, ConfirmLevel)>>>;

/// 弹层种类（决定标题 / 影响范围 / 分级 / 明细 / 补丁来源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    /// 保存（`from = Edit`）。
    Save,
    /// 恢复默认值（`from = ResetDefault`）。
    Reset,
}

impl Core {
    /// 逐字段原因写入 + 行内刷新。
    ///
    /// **注入期**的不合法字段（[`Core::invalid`]）不被后端 `field_errors` 覆盖：它是**更低一层**
    /// 的判据（注入值连自己的元数据都不满足）⇒ 该行**恒**显 [`TEXT_INVALID_VALUE`]（C1 / PD21）。
    fn apply_field_errors(&self, errs: &[FieldError]) {
        let map: BTreeMap<String, String> = errs
            .iter()
            .map(|e| (e.field.clone(), display_safe(&e.reason)))
            .collect();
        let invalid = self.invalid.borrow();
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                let reason = map
                    .get(&row.key)
                    .map(String::as_str)
                    .or_else(|| invalid.contains(&row.key).then_some(TEXT_INVALID_VALUE));
                row.refresh_error(reason);
            }
        }
        *self.errors.borrow_mut() = map;
    }

    /// 用户改了某个字段（控件回调入口）：**记入 `touched`**（C1 的"用户真改过"）、清该字段的错误、
    /// 撤下上一次失败提示、刷新按钮态。
    fn after_field_edit(&self, key: &str) {
        self.touched.borrow_mut().insert(key.to_string());
        self.errors.borrow_mut().remove(key);
        self.show_note();
        self.refresh_actions();
    }

    /// 草稿门控的**只读快照**（两个 `RefCell` 的借用不外泄）。
    fn scope(&self) -> DraftScope {
        DraftScope {
            touched: self.touched.borrow().clone(),
            invalid: self.invalid.borrow().clone(),
        }
    }

    /// **用户已改过**的键 → 其当前控件值（**I4 取 (b)**：注入新视图时据此保活草稿）。
    ///
    /// 只取 `touched` 且**当前值合法**的键：非法值写回新控件只会再造一个"最小可表示值"假象。
    fn draft_values(&self) -> BTreeMap<String, Value> {
        if self.touched.borrow().is_empty() {
            return BTreeMap::new();
        }
        let cur = self.current_values();
        self.touched
            .borrow()
            .iter()
            .filter_map(|k| cur.get(k).map(|v| (k.clone(), v.clone())))
            .collect()
    }

    /// 清空草稿标记（**服务端回执为权威**时调用 —— 见 [`P2ConfigPage::show_result`] 成功路径）。
    fn clear_draft(&self) {
        self.touched.borrow_mut().clear();
    }

    /// 建卡失败时回滚 [`Core::clear_draft`]（草稿标记按原样恢复）。
    fn restore_touched(&self, carried: &BTreeMap<String, Value>) {
        *self.touched.borrow_mut() = carried.keys().cloned().collect();
    }

    /// 新视图落屏后**重建草稿门控**（**I4 (b)**）：对**新元数据认可**的旧草稿值写回控件并保留
    /// `touched`；不认可 / 键已消失 ⇒ 丢弃该键的草稿（控件保持注入值）—— **不**静默改写。
    fn rebind_scope(&self, carried: &BTreeMap<String, Value>) {
        let invalid = self.invalid.borrow();
        let mut touched = BTreeSet::new();
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                if invalid.contains(&row.key) {
                    continue;
                }
                let Some(v) = carried.get(&row.key) else {
                    continue;
                };
                if row.kind.validate_value(v).is_err() {
                    continue;
                }
                row.control.set(&row.kind, v);
                touched.insert(row.key.clone());
            }
        }
        *self.touched.borrow_mut() = touched;
    }

    /// 全部字段的**当前值**快照（读自控件）。
    fn current_values(&self) -> BTreeMap<String, Value> {
        let mut m = BTreeMap::new();
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                m.insert(row.key.clone(), row.control.current(&row.kind));
            }
        }
        m
    }

    /// 草稿补丁（只含**用户真改过**且**值合法**的字段；C1）。
    fn draft(&self) -> ConfigPatch {
        match self.view.borrow().as_ref() {
            Some(v) => draft_patch(v, &self.current_values(), &self.scope()),
            None => ConfigPatch {
                changes: JsonMap::new(),
                from: PatchSource::Edit,
            },
        }
    }

    /// 恢复默认值补丁（排除只读字段与**注入值非法**的字段；PD12 / C1）。
    fn defaults_patch(&self) -> ConfigPatch {
        match self.view.borrow().as_ref() {
            Some(v) => defaults_patch_of(v, &self.invalid.borrow()),
            None => ConfigPatch {
                changes: JsonMap::new(),
                from: PatchSource::ResetDefault,
            },
        }
    }

    /// 是否有未保存修改（口径见 [`DraftScope`]：**注入永不置脏**）。
    fn is_dirty(&self) -> bool {
        match self.view.borrow().as_ref() {
            Some(v) => is_dirty(v, &self.current_values(), &self.scope()),
            None => false,
        }
    }

    /// 放弃修改：全部字段回退到注入值，**清空草稿标记**，清掉错误与失败提示。
    ///
    /// **注入值非法的行保持错误态**（C1 / PD21）：它不因"放弃修改"而变成合法 —— 那是它的**固有**
    /// 状态，不是草稿。
    fn discard(&self) {
        let invalid = self.invalid.borrow();
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                row.control.set(&row.kind, &row.initial);
                row.refresh_error(invalid.contains(&row.key).then_some(TEXT_INVALID_VALUE));
            }
        }
        self.touched.borrow_mut().clear();
        self.errors.borrow_mut().clear();
        self.show_note();
        self.refresh_actions();
    }

    /// 显示常态说明行（隐藏失败行）。
    fn show_note(&self) {
        set_visible(self.fail.obj(), false);
        set_visible(self.note.obj(), true);
    }

    /// 显示失败 / 降级原因行（隐藏常态说明行）。
    fn show_fail(&self, text: &str) {
        self.fail.set_text(text);
        set_visible(self.note.obj(), false);
        set_visible(self.fail.obj(), true);
    }

    /// 刷新操作条（保存 / 恢复默认值的可用态 + 保存文案）。
    fn refresh_actions(&self) {
        let submitting = self.submitting.get();
        self.save.set_text(if submitting { TEXT_SAVING } else { TEXT_SAVE });
        let usable = self.available.get() && !submitting;
        // 无改动 / 有字段错误 ⇒ 保存置灰（EDGE-10 / CF-02 的"保存按钮置灰"）。
        let can_save = usable && self.is_dirty() && self.errors.borrow().is_empty();
        self.save.set_disabled(!can_save);
        self.reset.set_disabled(!usable);
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

    /// 触发「提交意图」回调（**不发起任何请求**；`request_id` 由外部生成）。
    fn fire_submit(&self, patch: ConfigPatch, level: ConfirmLevel) {
        if let Ok(mut slot) = self.on_submit.try_borrow_mut() {
            if let Some(f) = slot.as_mut() {
                f(patch, level);
            }
        }
    }

    /// 按 key 访问字段行（`RefCell::borrow` 借用的生命周期**不外泄** ⇒ 用闭包）。
    fn with_row<R>(&self, key: &str, f: impl FnOnce(&FieldRow) -> R) -> Option<R> {
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                if row.key == key {
                    return Some(f(row));
                }
            }
        }
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. 卡片装配
// ═══════════════════════════════════════════════════════════════════════════

/// 按视图建全部分组卡（**先建后换**：任一环节失败 ⇒ 返回 `Err` 且旧视图原样保留）。
///
/// 返回 `(卡片, 注入值非法的键集合)` —— 非法集**在建卡时一次算出**（与各行看到的判据同源，
/// 见 [`build_row`] 的 `invalid` 参数），供 [`Core::invalid`] 与草稿门控使用（**C1**）。
fn build_cards(
    core: &Rc<Core>,
    view: &ConfigView,
) -> Result<(Vec<GroupCard>, BTreeSet<String>), LvglError> {
    let label_styles = [
        theme::text(TextSlot::Label, Palette::TEXT_PRIMARY),
        theme::text(TextSlot::Label, Palette::DANGER),
    ];
    let status_styles = [
        theme::text(TextSlot::Body, Palette::TEXT_WEAK),
        theme::text(TextSlot::Body, Palette::DANGER),
    ];

    // **注入即校验**（C1 第 1 条）：逐字段用契约自带的 `ConfigKind::validate_value` 预校验。
    //
    // 为何取 `f.kind.validate_value(..)` 而**不是** `f.validate_value(..)`（[`ConfigField`] 上那个）：
    // 后者把 `editable == false` 也当拒绝（"只读，不可修改"）⇒ 只读字段会被误判成"值非法"，
    // 而只读是**改不动**、不是**值不合法**（本页对只读的既有处置是"控件 disabled + 说明行"，
    // 见设计 §6.2 / PL-4）。这是**值域**校验，不是**可写性**校验。
    let mut invalid: BTreeSet<String> = BTreeSet::new();
    let mut cards: Vec<GroupCard> = Vec::with_capacity(view.groups.len());
    let mut y = FIRST_CARD_Y;
    for g in &view.groups {
        cards.push(build_card(
            core,
            g,
            y,
            &label_styles,
            &status_styles,
            &mut invalid,
        )?);
        y += card_height(g) + CARD_GAP;
    }
    Ok((cards, invalid))
}

/// 分组卡高（卡头 + 各行 + 行间分隔线 + 上下内边距）。
fn card_height(group: &ConfigGroup) -> i32 {
    let rows: i32 = group.fields.iter().map(|f| row_height(&f.kind)).sum();
    let dividers = if group.fields.is_empty() {
        0
    } else {
        (group.fields.len() as i32 - 1) * theme::Stroke::THIN
    };
    CARD_HEAD_H + rows + dividers + 2 * CARD_INSET
}

/// 建一张分组卡。
fn build_card(
    core: &Rc<Core>,
    group: &ConfigGroup,
    y: i32,
    label_styles: &[Rc<Style>; 2],
    status_styles: &[Rc<Style>; 2],
    invalid: &mut BTreeSet<String>,
) -> Result<GroupCard, LvglError> {
    let obj = decor(&core.scroll, Dimens::CONTENT_W, card_height(group), &theme::card())?;
    obj.set_pos(0, y);

    // 卡头：4 px 强调竖条 + 分组名 28 px（UI §6.2「卡头 44 px」）。
    let bar = decor(
        &obj,
        Dimens::ACCENT_BAR,
        CARD_HEAD_H,
        &theme::card_head_bar(Palette::INFO),
    )?;
    bar.set_pos(0, 0);
    let label = text_label(
        &obj,
        &group_label_text(group),
        TextSlot::SectionTitle,
        Palette::TEXT_PRIMARY,
    )?;
    label.set_pos(
        Dimens::ACCENT_BAR + Dimens::GAP_MIN,
        theme::center_offset(CARD_HEAD_H, TextSlot::SectionTitle.px() as i32),
    );

    let mut decor_keep = vec![bar];
    let mut rows = Vec::with_capacity(group.fields.len());
    let mut ry = CARD_HEAD_H;
    for (i, f) in group.fields.iter().enumerate() {
        rows.push(build_row(
            core,
            &obj,
            f,
            ry,
            label_styles,
            status_styles,
            invalid,
        )?);
        ry += row_height(&f.kind);
        // 行间 1 px 分隔（UI §6.2「最后一行不画」）。
        if i + 1 < group.fields.len() {
            let d = decor(
                &obj,
                INNER_W,
                theme::Stroke::THIN,
                &theme::card_head_bar(Palette::DIVIDER),
            )?;
            d.set_pos(0, ry);
            decor_keep.push(d);
            ry += theme::Stroke::THIN;
        }
    }

    Ok(GroupCard {
        _obj: obj,
        label,
        _decor: decor_keep,
        rows,
    })
}

/// 建一个字段行（两种版式由 `kind` 决定）。
///
/// **注入即校验**（**C1**）：`f.value` 不满足自身 `kind` 的值域 ⇒ `bad = true` ⇒ 该行
/// **错误态 + 控件 `disabled` + 该键记入 `invalid`**（→ 不进草稿 / 不进恢复默认值补丁）。
/// 判据只有一个（[`ConfigKind::validate_value`]），**不在下游再写第二套**。
fn build_row(
    core: &Rc<Core>,
    card: &Obj,
    f: &ConfigField,
    y: i32,
    label_styles: &[Rc<Style>; 2],
    status_styles: &[Rc<Style>; 2],
    invalid: &mut BTreeSet<String>,
) -> Result<FieldRow, LvglError> {
    let ipv4 = matches!(f.kind, ConfigKind::Ipv4);
    let bad = f.kind.validate_value(&f.value).is_err();
    if bad {
        invalid.insert(f.key.clone());
    }
    let mut hint = range_hint(&f.kind, f.unit.as_deref());

    // 字段名。
    let name_l = text_label(card, &field_label_text(f), TextSlot::Label, Palette::TEXT_PRIMARY)?;
    let label_y = if ipv4 {
        y + ROW_B_LABEL_Y
    } else {
        y + ROW_A_TEXT_Y
    };
    name_l.set_pos(0, label_y);
    let label_style = Cell::new(usize::MAX);
    set_style_index(name_l.obj(), label_styles, &label_style, 0);

    // 状态槽（约束提示 / 错误原因）。
    let status = label(card, TextSlot::Body, Palette::TEXT_WEAK)?;
    status.set_size(STATUS_W, TextSlot::Body.px() as i32);
    status.set_long_mode(LongMode::DOTS);
    let status_y = if ipv4 {
        y + ROW_B_LABEL_Y
            + theme::center_offset(TextSlot::Label.px() as i32, TextSlot::Body.px() as i32)
    } else {
        y + ROW_A_TEXT_Y + TextSlot::Label.px() as i32
    };
    status.set_pos(if ipv4 { STATUS_X } else { 0 }, status_y);
    let status_style = Cell::new(usize::MAX);
    set_style_index(status.obj(), status_styles, &status_style, 0);

    // 只读字段的说明行（设计 §6.2：控件 `disabled` + 说明行）。
    let note = if f.editable {
        None
    } else {
        let n = text_label(card, TEXT_READONLY_NOTE, TextSlot::Body, Palette::TEXT_WEAK)?;
        n.set_size(STATUS_W, TextSlot::Body.px() as i32);
        n.set_long_mode(LongMode::DOTS);
        n.set_pos(if ipv4 { STATUS_X } else { 0 }, status_y);
        Some(n)
    };

    // 行左缘危险竖条（隐藏态；出错显形）。
    let error_bar = decor(
        card,
        ERROR_BAR_W,
        row_height(&f.kind),
        &theme::card_head_bar(Palette::DANGER),
    )?;
    error_bar.set_pos(0, y);
    error_bar.set_hidden(true);

    // 输入控件（`kind` 驱动；**零文本输入**）。
    let control = match &f.kind {
        ConfigKind::Ipv4 => {
            let s = Rc::new(Ipv4Stepper::new(card, octets_of(f))?);
            s.obj().set_pos(ROW_B_CTRL_X, y + ROW_B_CTRL_Y);
            FieldControl::Ipv4(s)
        }
        ConfigKind::U16 { .. } | ConfigKind::U64 { .. } => {
            let (lo, hi, step) = int_bounds(&f.kind);
            let s = Rc::new(Stepper::new(card, lo, hi, initial_int(f), step)?);
            s.obj()
                .set_pos(INNER_W - STEPPER_TOTAL_W, y + ROW_A_CTRL_Y);
            FieldControl::Int(s)
        }
        ConfigKind::Enum { options } => {
            // **PD16**：选项超宽 ⇒ 只渲染一个**含当前值**的 9 段窗口（**不**整页 `Err`）；
            // 空选项 ⇒ 单段占位。两者都由 `enum_view` 决定，宽度恒 ≤ `INNER_W`。
            let view = enum_view(options, enum_index(&f.kind, &f.value));
            let labels: Vec<String> = view
                .options
                .iter()
                .map(|o| display_safe(&o.label))
                .collect();
            let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            let s = Rc::new(SegmentedControl::new(
                card,
                &refs,
                enum_width(view.options.len()),
                view.selected,
            )?);
            s.obj().set_pos(INNER_W - enum_width(view.options.len()), y + ROW_A_CTRL_Y);
            if let Some(hidden) = view.hidden_note {
                // 截断这一降级必须**上屏**（不得与"选项本来就这么几个"不可分）。
                hint = Some(hidden);
            }
            FieldControl::Enum {
                ctrl: s,
                start: view.start,
                shown: view.shown,
            }
        }
    };
    // 只读 ⇒ 禁用（PL-4）；**注入值非法 ⇒ 同样禁用**（C1：不可交互 + 不进草稿）。
    if !f.editable || bad {
        control.set_disabled(true);
    }

    // ── 变更回调：只报"这一行被改了"，簿记在 [`Core::after_field_edit`] ──
    {
        let w = Rc::downgrade(core);
        let key = f.key.clone();
        let on_edit = move || {
            if let Some(c) = w.upgrade() {
                c.after_field_edit(&key);
            }
        };
        match &control {
            FieldControl::Ipv4(s) => {
                let cb = on_edit.clone();
                s.set_on_change(move |_o| cb());
            }
            FieldControl::Int(s) => {
                let cb = on_edit.clone();
                s.set_on_change(move |_v| cb());
            }
            FieldControl::Enum { ctrl, .. } => {
                ctrl.set_on_change(move |_i| on_edit());
            }
        }
    }

    let row = FieldRow {
        key: f.key.clone(),
        kind: f.kind.clone(),
        initial: f.value.clone(),
        label: name_l,
        label_styles: [Rc::clone(&label_styles[0]), Rc::clone(&label_styles[1])],
        label_style,
        status,
        status_styles: [Rc::clone(&status_styles[0]), Rc::clone(&status_styles[1])],
        status_style,
        note,
        error_bar,
        control,
        hint,
    };
    // 注入值非法 ⇒ 该行**就地**（错误态）说明；否则常态（约束提示 / 只读说明 / 空）。
    row.refresh_error(bad.then_some(TEXT_INVALID_VALUE));
    Ok(row)
}

/// `Enum` 的**待渲染窗口**（**PD16**：把"选项多到装不下"从**整页 `Err`** 降为**字段级降级**）。
struct EnumView {
    /// 窗口内的选项（`SegmentedControl` 的段文案源）。
    options: Vec<mupc_display_proto::OptionItem>,
    /// 窗口在**契约选项表**里的起始下标。
    start: usize,
    /// 窗口段数（可寻址的段数；`0` = 空选项的占位行）。
    shown: usize,
    /// 初始选中段（**窗口内**下标）。
    selected: usize,
    /// 截断说明（上屏到约束槽；`None` = 未截断）。
    hidden_note: Option<String>,
}

/// 由契约选项表 + 当前值下标算出待渲染窗口。
///
/// 三条分支：
///
/// 1. **装得下**（`count ≤ `[`ENUM_MAX_SEGMENTS`]）⇒ 全量渲染，`start = 0`；
/// 2. **装不下** ⇒ 取一个**含当前值**的 `ENUM_MAX_SEGMENTS` 段窗口（`start = selected + 1 − n`，
///    再夹到 `[0, count − n]`）—— 窗口**必须含当前值**，否则合法值会被挤到窗外、`enum_index`
///    读回 `0` ⇒ **静默改写**成 `options[0]`（正是 C1 要根除的形态）；窗口位置随值滑动，
///    并在约束槽上屏「仅显示 N 项」；
/// 3. **空选项** ⇒ 单段占位（[`PLACEHOLDER`]，`shown = 0`）：`SegmentedControl::new` 拒绝空选项，
///    而契约允许 `options` 为空（该情形下 `validate_value` 必拒任何值 ⇒ 本行一定同时是 C1 的
///    "注入值非法"行：错误态 + 禁用 + 不进草稿）。
fn enum_view(options: &[mupc_display_proto::OptionItem], selected: usize) -> EnumView {
    if options.is_empty() {
        return EnumView {
            options: vec![mupc_display_proto::OptionItem {
                value: String::new(),
                label: PLACEHOLDER.to_string(),
            }],
            start: 0,
            shown: 0,
            selected: 0,
            hidden_note: None,
        };
    }
    let count = options.len();
    if count <= ENUM_MAX_SEGMENTS {
        return EnumView {
            options: options.to_vec(),
            start: 0,
            shown: count,
            selected: clamp_index(selected, count),
            hidden_note: None,
        };
    }
    let n = ENUM_MAX_SEGMENTS;
    let sel = clamp_index(selected, count);
    let start = (sel + 1).saturating_sub(n).min(count - n);
    EnumView {
        options: options[start..start + n].to_vec(),
        start,
        shown: n,
        selected: sel - start,
        hidden_note: Some(text_enum_truncated(n)),
    }
}

/// 下标夹到 `[0, count − 1]`（`count == 0` ⇒ `0`）。
fn clamp_index(i: usize, count: usize) -> usize {
    if count == 0 {
        0
    } else {
        i.min(count - 1)
    }
}

/// `U16`/`U64` → `(min, max, step)` 的 `i64` 形式（`u64` 超 `i64` 时收敛到 `i64::MAX`）。
///
/// **`step == 0` 折为 1**（**PD15**）：契约 [`ConfigKind::validate_value`] 写
/// `if *step != 0 && …` ⇒ **显式把 `step == 0` 当合法**（语义 = "不校验步长"）；而
/// [`Stepper::new`] 只接受正步长 ⇒ 照搬 0 会返回 `Err` 让**整页**拒绝渲染。折为 1 = 契约语义的
/// **忠实映射**（"不校验步长" ⇒ 任何整数值都合法 ⇒ 步长 1 可达**全部**合法值）。
fn int_bounds(kind: &ConfigKind) -> (i64, i64, i64) {
    match kind {
        ConfigKind::U16 { min, max, step } => (
            i64::from(*min),
            i64::from(*max),
            int_step(i64::from(*step)),
        ),
        ConfigKind::U64 { min, max, step } => (
            i64::try_from(*min).unwrap_or(i64::MAX),
            i64::try_from(*max).unwrap_or(i64::MAX),
            int_step(i64::try_from(*step).unwrap_or(1)),
        ),
        _ => (0, 0, 1),
    }
}

/// 契约步长 → `Stepper` 步长（`0` 折为 `1`，其余非正数同样折为 `1`；见 [`int_bounds`] 的 PD15）。
fn int_step(step: i64) -> i64 {
    if step > 0 {
        step
    } else {
        1
    }
}

/// 分段控件宽（`count × `[`ui::controls::SEGMENT_MIN_W`]）。
///
/// **不可失败**（**PD16**）：调用点只传 [`enum_view`] 给出的 `1..=`[`ENUM_MAX_SEGMENTS`] 段，
/// 故 `count × 96 ≤ 864 ≤` [`INNER_W`]；旧实现对超宽返回 `Err` ⇒ **整页**白屏（评审 I1 实测）。
fn enum_width(count: usize) -> i32 {
    // 段数上界由 ENUM_MAX_SEGMENTS 保证 ⇒ 乘法不可能溢出 i32（不为一个恒真上界写分支）。
    (count as i32) * crate::ui::controls::SEGMENT_MIN_W
}

/// `Value` → 四段（`Ipv4` 设值用；不可解析取 `0.0.0.0`）。
fn octets_of_value(_kind: &ConfigKind, v: &Value) -> [u8; 4] {
    v.as_str()
        .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
        .map(|a| a.octets())
        .unwrap_or([0, 0, 0, 0])
}

/// `Ipv4Stepper::octets()` → 文本（`192.168.1.10`；分隔符 `.` 在 cmap 内）。
fn ipv4_text(o: [u8; 4]) -> String {
    let [a, b, c, d] = o;
    format!("{a}.{b}.{c}.{d}")
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. 页面
// ═══════════════════════════════════════════════════════════════════════════

/// P2 配置页（**控制通道驱动**；见模块文档第 1 条）。
///
/// # 意图如何交给外部
///
/// 本页**不发请求、不生成 `request_id`**：用户点「保存」/「恢复默认值」只**开确认弹层**；
/// 弹层确认完成（L1 单击 / L2·L2+ 长按满 1.0 s）经 [`P2ConfigPage::set_on_submit`] 注册的
/// 回调交出一份 [`ConfigPatch`] 与所用 [`ConfirmLevel`]；外部（B3 `console.rs`）据此生成
/// `request_id`(uuid) + `issued_at_ms` 后 POST `/v1/console/config/apply`，并把回执经
/// [`P2ConfigPage::show_result`] 灌回本页。
pub struct P2ConfigPage {
    core: Rc<Core>,
}

impl P2ConfigPage {
    /// 在 `parent` 下建页（页根 `992 × 624`；**摆放由调用方负责**，与 P1/P6 同口径）。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = layout_box(parent, Dimens::CONTENT_W, Dimens::CONTENT_H)?;

        // 滚动视口（页根自建 —— `pages::page_root()` 的"页根即滚动容器"语义留给 P1/P6）。
        let scroll = ScrollContainer::create(&root)?;
        scroll.set_size(Dimens::CONTENT_W, SCROLL_H);
        scroll.set_pos(0, 0);
        scroll.add_style(&theme::transparent(), crate::lvgl::style::StyleSelector::main());

        // 固定操作条（不随滚动；UI §6.2 线框 `Y624`）。
        //
        // 样式取 `card_head_bar(SURFACE)`：它**零内边距 + 无描边 + 直角**（`theme.rs` 里唯一
        // "整块纯色"的组合），与本页"用绝对坐标排布操作条内元素"的需要一致 ——
        // `theme::surface_alt()` 未显式置零内边距，会被默认主题的内边距把按钮推偏。
        let action_bar = decor(
            &root,
            Dimens::CONTENT_W,
            ACTION_BAR_H,
            &theme::card_head_bar(Palette::SURFACE),
        )?;
        action_bar.set_pos(0, SCROLL_H);

        let save = Rc::new(TextButton::create(&action_bar, TEXT_SAVE)?);
        save.set_size(Dimens::BTN_MAIN_W, Dimens::BTN_H_PRIMARY);
        save.set_pos(
            Dimens::CONTENT_W - ACTION_PAD - Dimens::BTN_MAIN_W,
            ACTION_BTN_Y,
        );
        save.label().center();
        theme::button(theme::ButtonKind::Primary).apply(&save);

        let reset = Rc::new(TextButton::create(&action_bar, TEXT_RESET_DEFAULT)?);
        reset.set_size(Dimens::BTN_MAIN_W, Dimens::BTN_H_PRIMARY);
        reset.set_pos(ACTION_PAD, ACTION_BTN_Y);
        reset.label().center();
        theme::button(theme::ButtonKind::Danger).apply(&reset);

        // 页说明行 / 失败行（**同槽互斥**；都在滚动区内 = UI 线框 `Y80` 随页滚动）。
        let note = text_label(&scroll, TEXT_PAGE_NOTE, TextSlot::Body, Palette::TEXT_WEAK)?;
        note.set_pos(0, NOTE_Y);
        let fail = text_label(&scroll, "", TextSlot::Body, Palette::DANGER)?;
        fail.set_size(Dimens::CONTENT_W, TextSlot::Body.px() as i32);
        fail.set_long_mode(LongMode::DOTS);
        fail.set_pos(0, NOTE_Y);
        fail.set_hidden(true);

        let core = Rc::new(Core {
            root,
            scroll,
            action_bar,
            save,
            reset,
            note,
            fail,
            cards: RefCell::new(Vec::new()),
            view: RefCell::new(None),
            errors: RefCell::new(BTreeMap::new()),
            touched: RefCell::new(BTreeSet::new()),
            invalid: RefCell::new(BTreeSet::new()),
            submitting: Cell::new(false),
            available: Cell::new(false),
            pending_close: Cell::new(false),
            dialog: RefCell::new(None),
            toast: RefCell::new(None),
            on_submit: RefCell::new(None),
        });

        // 按钮：只**开弹层**（确认完成前不发任何请求 —— 设计 §6.2 末行）。
        {
            let w = Rc::downgrade(&core);
            core.save.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                if let Err(e) = open_dialog(&c, DialogKind::Save) {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr(), "P2 配置页：打开保存确认弹层失败：{e}");
                }
            });
        }
        {
            let w = Rc::downgrade(&core);
            core.reset.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                if let Err(e) = open_dialog(&c, DialogKind::Reset) {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr(), "P2 配置页：打开恢复默认值确认弹层失败：{e}");
                }
            });
        }

        let page = Self { core };
        page.set_unavailable("");
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

    /// 注册「提交意图」回调：确认完成时**调用一次**，载荷 = 待写补丁 + 所用确认强度。
    ///
    /// ⚠️ **回调内不得回灌本页会关弹层的方法**（[`P2ConfigPage::show_result`] /
    /// [`P2ConfigPage::set_config`]）：它们会 `ConfirmDialog::close`，而本回调正是从该弹层的
    /// 事件回调里出来的（`components.rs` 明确要求"关闭由调用方在自己的 tick 里做"）。
    /// 正确姿势：回调内只把补丁交给网络任务，结果回来后在**事件循环的下一拍**调
    /// [`P2ConfigPage::show_result`]。
    pub fn set_on_submit<F>(&self, f: F)
    where
        F: FnMut(ConfigPatch, ConfirmLevel) + 'static,
    {
        *self.core.on_submit.borrow_mut() = Some(Box::new(f));
    }

    /// 注入配置视图（`GET /v1/console/config` 的结果 / 成功回执的 `applied`）。
    ///
    /// `WriteMode::FullRewrite` ⇒ Toast 明示「原有文字不再存在」（EDGE-23，见 **PD9/PD10**）。
    ///
    /// **逐字段注入即校验**（**C1**）：`f.value` 不满足自身 `kind` 的值域 ⇒ 该行进错误态 +
    /// 控件 `disabled` + 该键**不进任何补丁**（不置脏、不进草稿、不进恢复默认值）。
    ///
    /// **草稿保活**（**I4 取 (b)**，见 **PD17**）：重建卡片时，**用户已改过**（`touched`）且新元数据
    /// **认可**的值会写回新控件并保留脏标记；键已消失 / 新元数据不认可 ⇒ 丢弃该键的草稿。
    ///
    /// **先建后换**：**建卡失败** ⇒ 返回 `Err` 且**旧视图原样保留**（不留半成品界面），草稿标记亦
    /// 原样保留。
    ///
    /// ⚠️ **订正（M3）**：本段承诺的"原样保留"**只覆盖建卡失败**。建卡**成功之后**仍有一步可能
    /// `Err` —— `FullRewrite` 的警示 Toast（`show_toast` 需向 `layer_top()` 建对象）。此时
    /// `cards` / `view` / `available` **已经提交**（视图已换、仅提示缺失）⇒ **不**满足"原样保留"。
    /// 之所以不为此重排顺序：Toast 失败是**显示层**失败（`layer_top` 不可用），重排（先弹 Toast
    /// 再换卡）会把"建卡失败"这一**更严重**的失败变得无法回滚；而静默吞掉 `Err` 与"绝不造假"冲突
    /// ⇒ 保留上抛、如实登记。
    pub fn set_config(&self, view: &ConfigView) -> Result<(), LvglError> {
        self.core.close_dialog();
        // I4 (b)：先采集旧草稿（键 → 当前控件值），再建新卡 —— 建卡失败时这些**不做任何改动**。
        let carried = self.core.draft_values();
        let (cards, invalid) = build_cards(&self.core, view)?;
        *self.core.cards.borrow_mut() = cards;
        *self.core.view.borrow_mut() = Some(view.clone());
        *self.core.invalid.borrow_mut() = invalid;
        self.core.errors.borrow_mut().clear();
        self.core.available.set(true);
        self.core.rebind_scope(&carried);
        self.core.show_note();
        self.core.refresh_actions();
        if view.write_mode == WriteMode::FullRewrite {
            self.core
                .show_toast(ToastTone::Warning, ICON_WARN, TEXT_TOAST_FULL_REWRITE)?;
        }
        Ok(())
    }

    /// 控制通道取不到配置时的降级态（**只由注入值驱动**）。
    ///
    /// ⚠️ 本页**不**用 [`crate::ui::components::UnavailableState`]：它的
    /// [`crate::ui::components::UnavailableKind`] 只有「告警源 / 审计 / 联锁」三个场景，
    /// **没有配置场景**（`components.rs` 本批禁改）⇒ 自建"标题 + 原因"两段式提示行，
    /// 语义仍是**不可用（无法获知）**，不是"空"（UI §8.3 的"空 vs 不可用"口径不破）。
    pub fn set_unavailable(&self, reason: &str) {
        self.core.close_dialog();
        self.core.cards.borrow_mut().clear();
        *self.core.view.borrow_mut() = None;
        self.core.errors.borrow_mut().clear();
        self.core.touched.borrow_mut().clear();
        self.core.invalid.borrow_mut().clear();
        self.core.available.set(false);
        self.core.show_fail(&unavailable_text(reason));
        self.core.refresh_actions();
    }

    /// 提交中（F9.6）：保存按钮 `disabled` + 文案「保存中...」。
    pub fn set_submitting(&self, on: bool) {
        self.core.submitting.set(on);
        self.core.refresh_actions();
    }

    /// 回执注入（成功 / 失败两条路径，设计 §6.2「成功」「失败」两行）。
    ///
    /// - **成功**：用 `applied` 立即刷新本地值（不等下一帧）+ Toast；
    /// - **失败**：Toast + **保留用户已输入值**（EDGE-10）+ 逐字段标红并显示**具体**原因
    ///   （CF-02）+ 保存按钮置灰；`AuditUnavailable` 走 UI §8.3 的固定文案（EDGE-18）。
    ///
    /// ⚠️ **`duplicate`（幂等命中）本页未读取** —— 见 **PD18**（UI §3.6 无对应文案 ⇒ 漏覆盖）。
    pub fn show_result(&self, resp: &ControlResponse<ConfigView>) -> Result<(), LvglError> {
        self.core.close_dialog();
        if resp.ok {
            let full_rewrite = resp
                .applied
                .as_ref()
                .is_some_and(|v| v.write_mode == WriteMode::FullRewrite);
            if let Some(applied) = resp.applied.clone() {
                // **回执为权威**：写操作已被装置接受 ⇒ 草稿标记作废（否则会把"已被接受的编辑"
                // 当成待提交草稿继续挂着 = 界面上多出一个不存在的差异）。
                // `set_config` 真正落屏后再清 —— 建卡失败时**回滚**（草稿标记原样保留）。
                let carried = self.core.draft_values();
                self.core.clear_draft();
                if let Err(e) = self.set_config(&applied) {
                    self.core.restore_touched(&carried);
                    return Err(e);
                }
            }
            // `full_rewrite` 的信息量更大（数据损失提示），此时 `set_config` 已弹警示 Toast
            // —— 不再叠加"保存成功"（UI §7.2：同一时刻仅 1 条）。见 PD10。
            if !full_rewrite {
                self.core
                    .show_toast(ToastTone::Success, ICON_OK, TEXT_TOAST_OK)?;
            }
        } else {
            self.core.apply_field_errors(&resp.field_errors);
            // EDGE-18（审计不可写）走 UI §8.3 的**固定文案**，且**落在 Toast 上**
            // （该行判据原文即「Toast『审计不可用，操作未执行』」）；其余失败按
            // UI §6.2 流程 6：Toast「保存失败」+ 就地（页顶行 / 字段行）显示**具体**原因。
            let audit = resp.code == ControlCode::AuditUnavailable;
            let text = if audit {
                TEXT_AUDIT_UNAVAILABLE.to_string()
            } else {
                display_safe(resp.message.trim())
            };
            self.core.show_fail(&text);
            let toast = if audit {
                TEXT_AUDIT_UNAVAILABLE
            } else {
                TEXT_TOAST_FAIL
            };
            self.core.show_toast(ToastTone::Failure, ICON_FAIL, toast)?;
            self.core.refresh_actions();
        }
        Ok(())
    }

    /// 每个事件循环拍调一次：执行**延迟动作**（取消后的弹层关闭 / Toast 过期）。
    ///
    /// 时钟**注入**（`now`）⇒ 离屏可确定性驱动；本页自身不读时钟做业务判断。
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
    }

    /// 是否有未保存修改（EDGE-11）。
    pub fn is_dirty(&self) -> bool {
        self.core.is_dirty()
    }

    /// 放弃修改：全部字段回退到注入值。
    pub fn discard_draft(&self) {
        self.core.discard();
    }

    /// 草稿补丁（只含被改动的字段，`from = Edit`）。
    pub fn draft(&self) -> ConfigPatch {
        self.core.draft()
    }

    /// 恢复默认值补丁（全部 `editable` 字段，`from = ResetDefault`）。
    pub fn defaults_patch(&self) -> ConfigPatch {
        self.core.defaults_patch()
    }

    /// 配置是否可用。
    pub fn is_available(&self) -> bool {
        self.core.available.get()
    }

    /// 是否提交中（**M5 取证**：本文件外零引用 ⇒ 与 [`P2ConfigPage::with_dialog`] /
    /// [`P2ConfigPage::field_disabled`] **同款**收成测试期读回口径）。
    ///
    /// ⚠️ B3 若要在生产侧读它，**去掉 `#[cfg(test)]` 即可**（本方法本身不依赖测试设施）。
    #[cfg(test)]
    pub(crate) fn is_submitting(&self) -> bool {
        self.core.submitting.get()
    }

    // ── 离屏断言口径 ─────────────────────────────────────────────────────

    /// 分组数。
    pub fn group_count(&self) -> usize {
        self.core.cards.borrow().len()
    }

    /// 第 `gi` 组的分组名。
    pub fn group_label(&self, gi: usize) -> Option<String> {
        self.core.cards.borrow().get(gi).and_then(|c| c.label.text())
    }

    /// 第 `gi` 组的字段数。
    pub fn field_count(&self, gi: usize) -> usize {
        self.core
            .cards
            .borrow()
            .get(gi)
            .map(|c| c.rows.len())
            .unwrap_or(0)
    }

    /// 第 `gi` 组第 `fi` 个字段的**上屏标签**。
    pub fn field_label(&self, gi: usize, fi: usize) -> Option<String> {
        self.core
            .cards
            .borrow()
            .get(gi)
            .and_then(|c| c.rows.get(fi))
            .and_then(|r| r.label.text())
    }

    /// 某字段当前**显示值文本**（读自控件：值区 / 汇总标签 / 选中段）。
    pub fn field_value_text(&self, key: &str) -> Option<String> {
        self.core.with_row(key, |r| r.control.display()).flatten()
    }

    /// 某字段的只读说明行文本（非只读字段为 `None`）。
    pub fn field_note_text(&self, key: &str) -> Option<String> {
        self.core
            .with_row(key, |r| r.note.as_ref().and_then(|n| n.text()))
            .flatten()
    }

    /// 某字段的只读说明行是否**可见**（`None` = 该字段没有说明行）。
    pub fn field_note_visible(&self, key: &str) -> Option<bool> {
        self.core
            .with_row(key, |r| r.note.as_ref().map(|n| !n.obj().is_hidden()))
            .flatten()
    }

    /// 某字段状态槽当前文本（无提示 / 无错误 ⇒ 空串）。
    pub fn field_status_text(&self, key: &str) -> Option<String> {
        self.core.with_row(key, |r| r.status.text()).flatten()
    }

    /// 某字段状态槽是否可见（约束提示或错误原因）。
    ///
    /// **M5 取证**：本文件外零引用 ⇒ 收成测试期读回口径（同 [`P2ConfigPage::field_disabled`]）。
    #[cfg(test)]
    pub(crate) fn field_status_visible(&self, key: &str) -> Option<bool> {
        self.core
            .with_row(key, |r| !r.status.obj().is_hidden())
    }

    /// 某字段的错误竖条是否显形（**字段级标红**的读回口径）。
    pub fn field_error_visible(&self, key: &str) -> Option<bool> {
        self.core
            .with_row(key, |r| !r.error_bar.is_hidden())
    }

    /// 保存按钮。
    pub fn save_button(&self) -> &TextButton {
        &self.core.save
    }

    /// 恢复默认值按钮。
    pub fn reset_button(&self) -> &TextButton {
        &self.core.reset
    }

    /// 保存按钮当前文案。
    pub fn save_text(&self) -> Option<String> {
        self.core.save.text()
    }

    /// 保存按钮当前是否禁用（读自 LVGL 状态位）。
    pub fn save_disabled(&self) -> bool {
        widgets::has_state(self.core.save.button().obj(), crate::lvgl::style::State::DISABLED)
    }

    /// 恢复默认值按钮当前是否禁用。
    pub fn reset_disabled(&self) -> bool {
        widgets::has_state(self.core.reset.button().obj(), crate::lvgl::style::State::DISABLED)
    }

    /// 当前 Toast 文案（无 Toast ⇒ `None`）。
    pub fn toast_text(&self) -> Option<String> {
        self.core.toast.borrow().as_ref().and_then(|t| t.text())
    }

    /// 当前 Toast 语义（无 ⇒ `None`）。
    pub fn toast_tone(&self) -> Option<ToastTone> {
        self.core.toast.borrow().as_ref().map(|t| t.tone())
    }

    /// 页顶**失败 / 降级**行文本（即使当前不可见也可读回）。
    pub fn fail_text(&self) -> Option<String> {
        self.core.fail.text()
    }

    /// 页顶失败行是否可见。
    pub fn fail_visible(&self) -> bool {
        !self.core.fail.obj().is_hidden()
    }

    /// 页顶**常态说明行**文本。
    pub fn note_text(&self) -> Option<String> {
        self.core.note.text()
    }

    /// 页顶常态说明行是否可见。
    pub fn note_visible(&self) -> bool {
        !self.core.note.obj().is_hidden()
    }

    /// **仅测试**：以闭包访问当前弹层（`RefCell` 借用不外泄）—— 给离屏用例读分级 / 标题 /
    /// `WarnBanner`，并向「确认」按钮派发事件（`Obj::send_event`）。
    ///
    /// **`#[cfg(test)]`**：生产侧不需要"从外部拿弹层"，故编译期即不提供该路径。
    #[cfg(test)]
    pub fn with_dialog<R>(&self, f: impl FnOnce(&ConfirmDialog) -> R) -> Option<R> {
        self.core.dialog.borrow().as_ref().map(f)
    }

    /// **仅测试**：程序化设某字段的值（**走与控件回调同一条簿记**：脏标记 / 清错 / 刷按钮）。
    ///
    /// 为什么必须有它：`Stepper` 的 `−` / `＋` 点击闭包挂在**子按钮**上，而
    /// `Obj::send_event` **只向本对象派发并向父链冒泡**（不下行）⇒ 离屏用例无法经事件驱动
    /// 步进器（`ui/controls.rs` 模块文档「薄层缺能力」5 的同款事实）。返回 `false` = 无此字段。
    #[cfg(test)]
    pub fn set_field_value(&self, key: &str, value: &Value) -> bool {
        let hit = self.core.with_row(key, |r| {
            r.control.set(&r.kind, value);
        });
        if hit.is_none() {
            return false;
        }
        self.core.after_field_edit(key);
        true
    }

    /// **仅测试**：某字段控件当前是否禁用（读自 LVGL —— 生产侧无此读回需求，见
    /// [`FieldControl::is_disabled`]；`Ipv4Stepper` 的读回口本身也只在 `cfg(test)` 下提供）。
    #[cfg(test)]
    pub fn field_disabled(&self, key: &str) -> Option<bool> {
        self.core.with_row(key, |r| r.control.is_disabled())
    }
}

/// 开确认弹层（**唯一的弹层构造点**；确认完成前不发任何请求）。
fn open_dialog(core: &Rc<Core>, kind: DialogKind) -> Result<(), LvglError> {
    // 同一时刻至多 1 个弹层；不可用 / 提交中不开。
    if core.dialog.borrow().is_some() || !core.available.get() || core.submitting.get() {
        return Ok(());
    }
    let Some(view) = core.view.borrow().as_ref().cloned() else {
        return Ok(());
    };
    let current = core.current_values();
    // 草稿门控快照（C1）：用户改过 / 注入值非法 —— 两个分支共用同一份口径。
    let scope = core.scope();
    let invalid = core.invalid.borrow().clone();

    let (title, impact, level, details_src, warn_src) = match kind {
        DialogKind::Save => {
            // 无改动不弹（「保存」是"提交草稿"，不是"重放"）。
            if !is_dirty(&view, &current, &scope) {
                return Ok(());
            }
            // **本次改动**的键集合 = 草稿补丁的键（PD11：分级与「涉及：」一律按它判定，
            // **不是**"视图内全部字段"）。迭代器在本分支内即时消费，不逃逸出 `changed`。
            let changed = draft_patch(&view, &current, &scope);
            (
                TEXT_DIALOG_TITLE_SAVE,
                TEXT_IMPACT_SAVE,
                save_level(&view, changed.changes.keys().map(String::as_str)),
                save_details(&view, &current, &scope),
                reconnect_field_labels(&view, changed.changes.keys().map(String::as_str)),
            )
        }
        DialogKind::Reset => {
            // 恢复默认值的"本次改动" = 恢复补丁的键（全部 `editable` 字段；只读字段被 PD12 排除，
            // 注入值非法的字段被 C1 排除）。
            let patch = defaults_patch_of(&view, &invalid);
            (
                TEXT_RESET_DEFAULT,
                TEXT_IMPACT_RESET,
                reset_level(&view, patch.changes.keys().map(String::as_str)),
                reset_details(&view, &current, &invalid),
                reconnect_field_labels(&view, patch.changes.keys().map(String::as_str)),
            )
        }
    };

    let details: Vec<ConfirmDetail> = details_src
        .iter()
        .map(|d| ConfirmDetail {
            field: &d.label,
            before: &d.before,
            after: &d.after,
        })
        .collect();
    let warn: Vec<&str> = warn_src.iter().map(|s| s.as_str()).collect();
    let spec = ConfirmSpec {
        title,
        impact,
        details: &details,
        warn_fields: &warn,
    };

    let layer = widgets::layer_top()?;
    let dialog = ConfirmDialog::new(&layer, &spec, level)?;

    // 确认完成 ⇒ **只报意图**（不发请求、不生成 `request_id`）。
    {
        let w = Rc::downgrade(core);
        dialog.set_on_confirm(move || {
            let Some(c) = w.upgrade() else { return };
            let patch = match kind {
                DialogKind::Save => c.draft(),
                DialogKind::Reset => c.defaults_patch(),
            };
            c.fire_submit(patch, level);
        });
    }
    // 取消 ⇒ 只置"待关闭"标志（不得在事件回调里删弹层，见 `Core::pending_close`）。
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

// ═══════════════════════════════════════════════════════════════════════════
// 8. 纯逻辑单测（**不触碰 LVGL** —— LVGL 非线程安全，触碰它的用例只能由
//    `src/lvgl/tests.rs::lvgl_core_bridge_chain` 串行调起，见 `ui/tests.rs`）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_display_proto::OptionItem;

    /// 造一个字段（**默认全部可编辑、无单位、不瞬断**，用例只覆写关心的那几项）。
    fn field(key: &str, label: &str, kind: ConfigKind, value: Value) -> ConfigField {
        ConfigField {
            key: key.into(),
            label: label.into(),
            kind,
            default: value.clone(),
            value,
            unit: None,
            requires_reconnect: false,
            editable: true,
        }
    }

    /// 三组视图：IEC 104（Ipv4 + U16）/ 核间（U16）/ 遥测与日志（U64 + Enum + 只读 Ipv4）。
    fn view() -> ConfigView {
        let u16k = ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 1,
        };
        let mut port = field("gateway.port", "端口", u16k.clone(), Value::from(2404));
        port.default = Value::from(2404);
        let mut listen = field(
            "gateway.listen_addr",
            "对端 IP 地址",
            ConfigKind::Ipv4,
            Value::from("127.0.0.1"),
        );
        listen.default = Value::from("127.0.0.1");
        let mut period = field(
            "telemetry.interval",
            "遥测上报周期",
            ConfigKind::U64 {
                min: 1,
                max: 300,
                step: 1,
            },
            Value::from(1u64),
        );
        period.unit = Some("秒".into());
        period.default = Value::from(60u64);
        let level = field(
            "system.log_level",
            "日志级别",
            ConfigKind::Enum {
                options: vec![
                    OptionItem {
                        value: "error".into(),
                        label: "ERROR".into(),
                    },
                    OptionItem {
                        value: "info".into(),
                        label: "INFO".into(),
                    },
                ],
            },
            Value::from("info"),
        );
        let mut bind = field(
            "display.bind_addr",
            "本机服务地址（仅回环）",
            ConfigKind::Ipv4,
            Value::from("127.0.0.1"),
        );
        bind.editable = false;
        ConfigView {
            groups: vec![
                ConfigGroup {
                    id: "iec104".into(),
                    label: "IEC 104 连接参数".into(),
                    fields: vec![listen, port],
                },
                ConfigGroup {
                    id: "intercore".into(),
                    label: "核间通信参数".into(),
                    fields: vec![field(
                        "intercore.port",
                        "本地端口",
                        u16k.clone(),
                        Value::from(2500),
                    )],
                },
                ConfigGroup {
                    id: "telemetry".into(),
                    label: "遥测与日志".into(),
                    fields: vec![period, level, bind],
                },
            ],
            revision: 7,
            write_mode: WriteMode::TextPreserve,
        }
    }

    /// 把某字段标成 `requires_reconnect`（造"瞬断字段"用；找不到键即响亮失败）。
    fn mark_reconnect(v: &mut ConfigView, key: &str) {
        let mut hit = false;
        for g in &mut v.groups {
            for f in &mut g.fields {
                if f.key == key {
                    f.requires_reconnect = true;
                    hit = true;
                }
            }
        }
        assert!(hit, "视图里没有键 `{key}` —— 用例的构造前提不成立");
    }

    /// 造一个"用户改过这些键"的门控（C1）。
    fn touched(keys: &[&str]) -> DraftScope {
        DraftScope {
            touched: keys.iter().map(|k| (*k).to_string()).collect(),
            invalid: BTreeSet::new(),
        }
    }

    /// 草稿只含**用户真改过**的字段；改动前后的脏判定对称。
    ///
    /// 敏感性：把 [`draft_patch`] 的 `*now != f.value` 改成无条件插入 ⇒ 第 1、4 条变红；
    /// 改成恒 `false` ⇒ 第 2、3 条变红。
    #[test]
    fn draft_patch_contains_only_changed_fields() {
        let v = view();
        let mut cur = initial_values(&v);

        // 未改动 ⇒ 空补丁、不脏。
        assert!(draft_patch(&v, &cur, &touched(&[])).changes.is_empty());
        assert!(!is_dirty(&v, &cur, &touched(&[])));

        // 改一个 ⇒ 只含该字段。
        cur.insert("gateway.port".into(), Value::from(2405));
        let p = draft_patch(&v, &cur, &touched(&["gateway.port"]));
        assert_eq!(p.from, PatchSource::Edit);
        assert_eq!(p.changes.len(), 1);
        assert_eq!(p.changes.get("gateway.port"), Some(&Value::from(2405)));
        assert!(is_dirty(&v, &cur, &touched(&["gateway.port"])));

        // 改回 ⇒ 又不脏（**值相等判定**，不是"碰过就脏"）。
        cur.insert("gateway.port".into(), Value::from(2404));
        assert!(!is_dirty(&v, &cur, &touched(&["gateway.port"])));
        assert!(draft_patch(&v, &cur, &touched(&["gateway.port"]))
            .changes
            .is_empty());
    }

    /// **C1 的纯逻辑回归锁**（touched 门控 + invalid 排除）—— 本条就是"注入非法值"在**无 LVGL**
    /// 层面的等价形态：控件的值（`cur`）与视图值不同，但**用户一次都没碰过**。
    ///
    /// 敏感性（**探针实测**）：把 [`DraftScope::admits`] 里 `self.touched.contains(key)` 去掉
    /// （回到"与注入值不等即脏"）⇒ 本条前 4 条断言立刻变红。
    #[test]
    fn draft_is_gated_by_touched_and_not_by_value_difference() {
        let v = view();
        let mut cur = initial_values(&v);
        // 模拟"控件把非法注入值渲染成 options[0]"：`system.log_level` 的注入值是 `info`（合法），
        // 这里直接把它换成**另一个合法选项** `error` —— 等价于"控件显示值与视图值不同"。
        cur.insert("system.log_level".into(), Value::from("error"));

        // ① **没碰过** ⇒ 不脏、草稿为空（**注入/渲染差异永不置脏**）。
        assert!(!is_dirty(&v, &cur, &touched(&[])), "值有差异 ≠ 用户改过");
        assert!(draft_patch(&v, &cur, &touched(&[])).changes.is_empty());
        assert!(save_details(&v, &cur, &touched(&[])).is_empty(), "明细也不得凭空冒出一行");

        // ② **碰过** ⇒ 才进草稿（且只进这一个键）。
        let p = draft_patch(&v, &cur, &touched(&["system.log_level"]));
        assert_eq!(p.changes.len(), 1);
        assert_eq!(p.changes.get("system.log_level"), Some(&Value::from("error")));

        // ③ **注入值非法** ⇒ 即便 `touched` 里**有**它（异常路径 / 竞态），也**不得**进草稿。
        let scoped = DraftScope {
            touched: ["system.log_level"].iter().map(|k| (*k).to_string()).collect(),
            invalid: ["system.log_level"].iter().map(|k| (*k).to_string()).collect(),
        };
        assert!(!scoped.admits("system.log_level"), "invalid 是**否决**票");
        assert!(
            draft_patch(&v, &cur, &scoped).changes.is_empty(),
            "注入值非法的键不得进草稿（否则会把屏上不可核查的值写进装置）"
        );
        assert!(!is_dirty(&v, &cur, &scoped));
    }

    /// **C1 / PD12 共用的排除口径**：注入值非法的键不进"恢复默认值"补丁，**也不进**其明细
    /// （明细与补丁必须逐条一致，否则弹层列了、实际没写 = 谎报）。
    ///
    /// 敏感性：把 [`defaults_patch_of`] 的 `!invalid.contains(..)` 去掉 ⇒ 第 1 条变红；
    /// 把 [`reset_details`] 的同一过滤去掉 ⇒ 第 2 条变红。
    #[test]
    fn invalid_keys_are_excluded_from_defaults_and_details() {
        let v = view();
        let cur = initial_values(&v);
        let invalid: BTreeSet<String> = ["system.log_level"].iter().map(|k| (*k).to_string()).collect();

        let p = defaults_patch_of(&v, &invalid);
        assert!(
            !p.changes.contains_key("system.log_level"),
            "注入值非法的字段不得进恢复默认值补丁"
        );
        assert!(p.changes.contains_key("gateway.port"), "其余可编辑字段照旧");

        let d = reset_details(&v, &cur, &invalid);
        assert!(
            d.iter().all(|x| x.key != "system.log_level"),
            "明细不得列出**不会写**的字段（逐条一致）"
        );
        assert_eq!(
            d.len(),
            iter_fields(&v).filter(|f| f.editable).count() - 1,
            "其余可编辑字段照旧全列"
        );
        // 空 invalid ⇒ 与旧口径一致（回退对照）。
        let full = defaults_patch_of(&v, &BTreeSet::new());
        assert!(full.changes.contains_key("system.log_level"));
    }

    /// 恢复默认值补丁：**覆盖全部 `editable` 字段** + `from = ResetDefault` +
    /// **只读字段不得混入**（契约 `validate_value` 会拒绝整单，见 PD12）。
    ///
    /// 敏感性：把 `filter(|f| f.editable)` 去掉 ⇒ 第 3 条变红；把 `f.default` 写成 `f.value`
    /// ⇒ 第 2 条变红（`telemetry.interval` 的当前值 1 ≠ 默认值 60）。
    #[test]
    fn defaults_patch_covers_all_editable_fields() {
        let v = view();
        let p = defaults_patch_of(&v, &BTreeSet::new());
        assert_eq!(p.from, PatchSource::ResetDefault);
        let editable: Vec<&str> = iter_fields(&v).filter(|f| f.editable).map(|f| f.key.as_str()).collect();
        assert_eq!(p.changes.len(), editable.len(), "覆盖全部可编辑字段");
        for k in &editable {
            assert!(p.changes.contains_key(*k), "缺字段 {k}");
        }
        assert!(
            !p.changes.contains_key("display.bind_addr"),
            "只读字段不得进补丁（后端二次校验会拒绝整单）"
        );
        assert_eq!(
            p.changes.get("telemetry.interval"),
            Some(&Value::from(60u64)),
            "取的是 default 而不是当前值"
        );
    }

    /// 分级：**本次改动触及**瞬断字段 ⇒ L2+；否则 L1（口径见 PD11）。
    ///
    /// 敏感性：把 [`save_level`] 的键集合判定改成"看第一个字段"⇒ 第 4 条变红
    /// （本视图里瞬断字段排在最后）。
    #[test]
    fn save_level_follows_requires_reconnect() {
        let mut v = view();
        assert_eq!(save_level(&v, ["gateway.port"]), ConfirmLevel::L1, "无瞬断字段 ⇒ L1");
        mark_reconnect(&mut v, "telemetry.interval");
        assert_eq!(
            save_level(&v, ["telemetry.interval"]),
            ConfirmLevel::L2Plus,
            "本次改动触及 requires_reconnect 字段 ⇒ L2+"
        );
        assert_eq!(
            save_level(&v, ["gateway.port"]),
            ConfirmLevel::L1,
            "只改**非**瞬断字段 ⇒ 仍 L1"
        );
        assert_eq!(
            reconnect_field_labels(&v, ["telemetry.interval"]),
            vec!["遥测上报周期".to_string()],
            "「涉及：」= 本次触及的瞬断字段的上屏名"
        );
        assert_eq!(
            reset_level(&v, ["telemetry.interval"]),
            ConfirmLevel::L2Plus,
            "涉及瞬断 ⇒ 恢复默认值升 L2+"
        );
    }

    /// **PD11 区分锁（本条 = "视图口径"与"改动口径"的分界线）**：
    /// 视图含瞬断字段、但本次**未改动**它 ⇒ 保存必须 **L1**、且「涉及：」为空。
    ///
    /// 敏感性（**探针 ① 实测**）：把 [`save_level`] / [`reconnect_field_labels`] 改回"视图口径"
    /// （用 [`has_reconnect_field`]、无视键集合）⇒ 本条第 1、2 条立刻变红。
    #[test]
    fn save_level_scopes_to_changed_keys_not_view() {
        let mut v = view();
        mark_reconnect(&mut v, "gateway.port"); // 视图里**存在**瞬断字段

        // 本次只改**非**瞬断字段 ⇒ L1（视图口径会误报 L2+，并谎报"通信将中断"）。
        assert_eq!(
            save_level(&v, ["telemetry.interval"]),
            ConfirmLevel::L1,
            "视图含瞬断字段但本次未触及 ⇒ **L1**"
        );
        assert!(
            reconnect_field_labels(&v, ["telemetry.interval"]).is_empty(),
            "「涉及：」不得列出本次没改的字段"
        );

        // 本次改的**就是**瞬断字段 ⇒ L2+，且「涉及：」含它。
        assert_eq!(save_level(&v, ["gateway.port"]), ConfirmLevel::L2Plus);
        assert_eq!(
            reconnect_field_labels(&v, ["gateway.port"]),
            vec!["端口".to_string()]
        );

        // 混合改动：只列**既改了又瞬断**的那一个。
        assert_eq!(
            reconnect_field_labels(&v, ["telemetry.interval", "gateway.port"]),
            vec!["端口".to_string()]
        );

        // 两个口径的差集 = "视图里有、本次没改" —— 视图口径**看得见**它（不是"看不见"）：
        assert!(
            has_reconnect_field(&v),
            "视图口径仍看得见该瞬断字段 —— 两口径的差别在「是否本次改动」，不在可见性"
        );
    }

    /// **PD11 恢复默认值**：全 `editable` 瞬断字段 ⇒ 改动口径 ≡ 视图口径（两个口径一致）；
    /// **只读**瞬断字段 ⇒ 改动口径**不列**它（视图口径会谎报 L2+）。
    ///
    /// 敏感性（**探针 ① 的姊妹条**）：把 [`reset_level`] 的键集合判定换成 [`has_reconnect_field`]
    /// ⇒ 第 2 段第 3 条（`L2`）变红（会读到 L2+）。
    #[test]
    fn reset_level_scopes_to_reset_patch_keys() {
        // 场景 A：**可编辑**的瞬断字段 —— 两个口径**一致**（都升 L2+）。
        let mut a = view();
        mark_reconnect(&mut a, "gateway.port");
        let keys_a: Vec<String> = defaults_patch_of(&a, &BTreeSet::new())
            .changes
            .keys()
            .cloned()
            .collect();
        assert!(
            keys_a.iter().any(|k| k == "gateway.port"),
            "恢复补丁覆盖全部 editable 字段"
        );
        assert_eq!(
            reset_level(&a, keys_a.iter().map(String::as_str)),
            ConfirmLevel::L2Plus
        );
        assert_eq!(
            reconnect_field_labels(&a, keys_a.iter().map(String::as_str)),
            vec!["端口".to_string()]
        );

        // 场景 B：**只读**的瞬断字段（`display.bind_addr`，editable = false）——
        // 视图口径看得见它（谎报），改动口径**不列**（PD12 只读不进补丁 ⇒ 也改不动）。
        let mut b = view();
        mark_reconnect(&mut b, "display.bind_addr");
        assert!(
            has_reconnect_field(&b),
            "视图口径看得见这个只读瞬断字段 —— 正是它会让视图口径误报"
        );
        let keys_b: Vec<String> = defaults_patch_of(&b, &BTreeSet::new())
            .changes
            .keys()
            .cloned()
            .collect();
        assert!(
            !keys_b.iter().any(|k| k == "display.bind_addr"),
            "只读字段不进恢复补丁（PD12）"
        );
        assert_eq!(
            reset_level(&b, keys_b.iter().map(String::as_str)),
            ConfirmLevel::L2,
            "**改不动**的瞬断字段不得把分级抬到 L2+（视图口径在此必错）"
        );
        assert!(
            reconnect_field_labels(&b, keys_b.iter().map(String::as_str)).is_empty(),
            "「涉及：」不得列出改不动的字段"
        );
    }

    /// 恢复默认值**最低 L2**（生效性写，不得只靠间距保护）。
    ///
    /// 敏感性：把 [`reset_level`] 的 `else` 分支改成 `L1` ⇒ 本条变红。
    #[test]
    fn reset_level_is_at_least_l2() {
        let v = view();
        let keys: Vec<String> = defaults_patch_of(&v, &BTreeSet::new())
            .changes
            .keys()
            .cloned()
            .collect();
        assert_eq!(reset_level(&v, keys.iter().map(String::as_str)), ConfirmLevel::L2);
        assert_ne!(reset_level(&v, keys.iter().map(String::as_str)), ConfirmLevel::L1);
    }

    /// 字段标签口径（**PM 裁定**，设计 §6.2 / UI 附录 B U-1）：
    /// `gateway.listen_addr` **不得**上屏为「对端 IP 地址」；两个回环绑定字段用回环标签；
    /// 其余字段**透传契约标签**（元数据驱动）。
    ///
    /// 敏感性：把 [`LABEL_OVERRIDES`] 删掉 ⇒ 第 1、3 条变红；把 `find` 的键写成别的
    /// ⇒ 第 1 条变红。
    #[test]
    fn field_labels_follow_pm_ruling() {
        let v = view();
        let listen = iter_fields(&v).find(|f| f.key == "gateway.listen_addr").expect("字段");
        assert_eq!(field_label_text(listen), TEXT_LISTEN_ADDR);
        assert_ne!(field_label_text(listen), "对端 IP 地址", "不得表述为「对端 IP」");
        assert!(field_label_text(listen).contains("监听"), "口径 = 本机监听地址");
        let bind = iter_fields(&v).find(|f| f.key == "display.bind_addr").expect("字段");
        assert_eq!(field_label_text(bind), TEXT_LOOPBACK_ADDR);
        assert_ne!(field_label_text(bind), TEXT_LISTEN_ADDR, "回环绑定地址与 IEC 104 监听地址**分列**");
        let port = iter_fields(&v).find(|f| f.key == "gateway.port").expect("字段");
        assert_eq!(field_label_text(port), "端口", "非裁定键**透传**契约标签");
    }

    /// **PD2 / PD14（③）**：回环绑定地址标签必须与 IEC 104 行**明显可区分**，
    /// 且**不**与 P6 的服务地址标签共用（口径不同）。
    ///
    /// 敏感性（**探针 ③ 实测**）：把 `TEXT_LOOPBACK_ADDR` 改回 `TEXT_SERVICE_ADDR`
    /// （= 「本机监听地址 · 仅本机」）⇒ 第 1、2 条立刻变红；改回 `TEXT_LISTEN_ADDR` ⇒ 第 3 条变红。
    /// 字面量的**逐字 cmap 齐备**由 `ui/tests.rs::ui_texts_covered_by_font_cmap` 把关
    /// （它是 `ui/**` 生产字面量，本页写死任何 cmap 外字符都会在那里变红）。
    #[test]
    fn loopback_label_is_distinct_from_iec104_and_p6() {
        use crate::ui::pages::p6_system::TEXT_SERVICE_ADDR;
        // ① 不得与 P6 的「本机服务地址」串共用（口径不同：P6 = 整体服务口径）。
        assert_ne!(
            TEXT_LOOPBACK_ADDR, TEXT_SERVICE_ADDR,
            "P2 本机绑定地址 ≠ P6 本机服务地址 —— 不得共用同一字面量"
        );
        // ② 不得与 IEC 104 行**共享「本机监听地址」前缀**（U-1 / EDGE-24：两行必须可区分、
        //    不得互换）—— P6 的串正是共享前缀的负例。
        assert!(
            TEXT_SERVICE_ADDR.starts_with("本机监听地址"),
            "负例自证：P6 的串确实共享「本机监听地址」前缀（否则本条的区分判据就落空）"
        );
        assert!(
            !TEXT_LOOPBACK_ADDR.contains("监听"),
            "本串不得含「监听」二字 —— 否则与 IEC 104 行（{TEXT_LISTEN_ADDR}）同前缀"
        );
        assert_ne!(TEXT_LOOPBACK_ADDR, TEXT_LISTEN_ADDR);
        // ③ 两条语义必须保留：**本机** + **仅本机可达**。
        assert!(TEXT_LOOPBACK_ADDR.contains("本机"));
        assert!(TEXT_LOOPBACK_ADDR.contains("仅本机"), "「仅本机可达」是 PL-4 回环红线的屏上表达");
    }

    /// **PD13（②）**：注入侧 `group.label` / `field.label` 与 `Enum` 选项**同一处理**
    /// （一律过 [`display_safe`]）—— 后端标签含 cmap 外 ASCII（`-` / 小写）时真机不落豆腐块。
    ///
    /// 敏感性（**探针 ② 实测**）：把 [`group_label_text`] / [`field_label_text`] 里的
    /// `display_safe` 去掉 ⇒ 本条变红。
    ///
    /// ⚠️ **本用例只证明 ASCII 改写**：`display_safe` **不处理非 ASCII**（中文缺字如 `环` / `服务`
    /// 照样透传）—— 该残余风险见 **PD13** 登记表（真正的防线在后端把 `label` 约束在 UI §3.6 内）。
    #[test]
    fn injected_labels_go_through_display_safe() {
        // 自证改写**真的发生**（否则下面两条"改写后"的断言无意义）。
        assert_eq!(display_safe("core-bin"), "CORE\u{2013}BIN");

        // 分组名（自由文本）。
        let g = ConfigGroup {
            id: "g".into(),
            label: "core-bin 参数".into(),
            fields: vec![],
        };
        assert_eq!(group_label_text(&g), "CORE\u{2013}BIN 参数");

        // 字段名：**非**裁定键 ⇒ 透传契约标签 + 安全改写。
        let f = field("x.y", "a-b", ConfigKind::Ipv4, Value::from("127.0.0.1"));
        assert_eq!(field_label_text(&f), "A\u{2013}B");

        // 裁定键：覆盖串本身逐字在 cmap 内 ⇒ 改写对它是**恒等**（覆盖语义不被 `display_safe` 破坏）。
        let l = field(
            "gateway.listen_addr",
            "对端 IP 地址",
            ConfigKind::Ipv4,
            Value::from("127.0.0.1"),
        );
        assert_eq!(field_label_text(&l), TEXT_LISTEN_ADDR);
    }


    /// 值格式化：整数带单位 / 枚举取**选项标签** / IPv4 原样。
    ///
    /// **非法值一律 [`PLACEHOLDER`]**（**C1**）：整数类型错配**不得**兜底成 `0`（`0` 是**合法**
    /// 配置值 ⇒ 兜底会把"值不合法"伪装成"值为 0"）；枚举未命中**不得**回显机器值。
    ///
    /// 敏感性：把 [`format_value`] 开头的 `validate_value` 判据去掉（回到 `unwrap_or_default`）
    /// ⇒ 第 4、5、6 条变红（会读到 `0` / 空串 / `trace`）。
    #[test]
    fn format_value_shapes() {
        let u16k = ConfigKind::U16 {
            min: 1,
            max: 300,
            step: 1,
        };
        assert_eq!(format_value(&u16k, &Value::from(30), None), "30");
        assert_eq!(format_value(&u16k, &Value::from(30), Some("秒")), "30 秒");
        assert_eq!(
            format_value(&ConfigKind::Ipv4, &Value::from("127.0.0.1"), None),
            "127.0.0.1"
        );
        let ek = ConfigKind::Enum {
            options: vec![OptionItem {
                value: "info".into(),
                label: "信息".into(),
            }],
        };
        assert_eq!(format_value(&ek, &Value::from("info"), None), "信息");

        // ① 枚举未命中（`trace` 不在选项内）⇒ 占位符，**不**回显机器值。
        assert_eq!(format_value(&ek, &Value::from("trace"), None), PLACEHOLDER);
        assert_ne!(format_value(&ek, &Value::from("trace"), None), "trace");
        // ② 类型错配（`U16` 收到字符串）⇒ 占位符，**不**兜底成 `0`（`0` 是合法值！）。
        assert_eq!(format_value(&u16k, &Value::from("abc"), None), PLACEHOLDER);
        assert_ne!(format_value(&u16k, &Value::from("abc"), None), "0");
        // ③ 越界（合法类型、非法值域）⇒ 同样是占位符。
        assert_eq!(format_value(&u16k, &Value::from(65535), None), PLACEHOLDER);
        // ④ `Ipv4` 类型错配 ⇒ 占位符（旧实现回显空串）。
        assert_eq!(format_value(&ConfigKind::Ipv4, &Value::from(7), None), PLACEHOLDER);
    }

    /// **I3**：`format_value` 的出口过了 [`display_safe`] —— 且对**合法值恒等**。
    ///
    /// 两件事一起锁：
    ///
    /// ① **改写确实挂在出口上**：给一个 `Enum` 选项标签含 cmap 外 ASCII（小写 / `-`）的样例，
    ///    断言产物是**改写后**的（`display_safe` 在 `format_value` 内部）；
    /// ② **对合法路径恒等**（否则"统一过 `display_safe`"会把屏上合法值改花）：逐类合法值
    ///    （IPv4 点分十进制 / 整数 + `秒` / 选项标签）过 [`display_safe`] **逐字不变**。
    ///
    /// 敏感性：把 `format_value` 末尾的 `display_safe(&raw)` 去掉 ⇒ 第 1、2 条变红
    /// （产物会是原样的小写 `rc1` / `core-x`）。
    #[test]
    fn format_value_is_identity_safe_for_legal_values() {
        // ① 改写确实发生（自证：`display_safe` 对这个串**非恒等**）。
        //
        // 样例取「小写 + ASCII 连字符」：`a`→`A`（大写同族**有**字形）、`-`→`–`（U+2013）。
        // **不取 `x`**：`X` 恰**不在** [`crate::ui::pages::ASCII_DISPLAY_ALPHABET`] 内
        // ⇒ `display_safe` 会把它兜底成 `?`（可读性差，且与本条要证明的事无关）。
        let raw = "a-b";
        assert_ne!(display_safe(raw), raw, "自证：该串会被 display_safe 改写");
        assert_eq!(display_safe(raw), "A\u{2013}B");

        // ② 合法值经 display_safe **逐字不变**（这是"过一遍不改花合法值"的断言）。
        let legal = [
            "127.0.0.1",   // IPv4 点分十进制
            "0.0.0.0",
            "30",          // 无单位整数
            "30 秒",       // 带单位（空格 + cmap 内的 `秒`）
            "1 – 300 秒",  // 约束提示口径（`–` U+2013）
            "ERROR",       // 大写选项标签
            "INFO",
            "信息",
            "100%",        // `%` 在 ASCII_DISPLAY_ALPHABET 内
            "本机监听地址 · IEC 104", // `·` U+00B7
        ];
        for s in legal {
            assert_eq!(display_safe(s), s, "合法路径上 display_safe 必须恒等：`{s}`");
        }
        // ③ 端到端：三类合法值经 `format_value` 后与"手写期望串"逐字相等（= 未被改花）。
        let u16k = ConfigKind::U16 {
            min: 1,
            max: 300,
            step: 1,
        };
        assert_eq!(format_value(&u16k, &Value::from(30), Some("秒")), "30 秒");
        assert_eq!(
            format_value(&ConfigKind::Ipv4, &Value::from("192.168.1.10"), None),
            "192.168.1.10"
        );
        let ek = ConfigKind::Enum {
            options: vec![OptionItem {
                value: "info".into(),
                label: "INFO".into(),
            }],
        };
        assert_eq!(format_value(&ek, &Value::from("info"), None), "INFO");
        // ④ 出口**确实**挂着 display_safe：选项标签含小写 / `-` 时产物被改写。
        let lk = ConfigKind::Enum {
            options: vec![OptionItem {
                value: "a-b".into(),
                label: raw.into(),
            }],
        };
        assert_eq!(
            format_value(&lk, &Value::from("a-b"), None),
            display_safe(raw),
            "出口 = display_safe（同一条路径）"
        );
        assert_eq!(format_value(&lk, &Value::from("a-b"), None), "A\u{2013}B");
    }

    /// **I1 / PD15**：`step == 0`（契约**合法**：`validate_value` 写 `if *step != 0`）折为 **1**，
    /// **不得**原样传给 `Stepper`（会 `Err` 让整页白屏）。
    ///
    /// 敏感性：把 [`int_step`] 的 `if step > 0` 去掉（原样返回）⇒ 第 1、3 条变红。
    #[test]
    fn int_bounds_folds_zero_step_to_one() {
        let z16 = ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 0,
        };
        assert_eq!(int_bounds(&z16), (1, 65535, 1), "step==0 ⇒ 1（不是 0、也不报错）");
        let z64 = ConfigKind::U64 {
            min: 0,
            max: 100,
            step: 0,
        };
        assert_eq!(int_bounds(&z64), (0, 100, 1));
        // 非零步长原样保留（不得顺手改成 1）。
        let s5 = ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 5,
        };
        assert_eq!(int_bounds(&s5), (1, 65535, 5));
    }

    /// **I1 / PD16**：`Enum` 窗口 —— 装得下 ⇒ 全量；装不下 ⇒ **含当前值**的 9 段窗口；
    /// 空选项 ⇒ 单段占位（`shown = 0`）。
    ///
    /// 敏感性：把 [`enum_view`] 的窗口起点写成固定 `0` ⇒ 第 2 条（`value = 9` 时窗口须含它）
    /// 变红 —— 那正是"合法值被挤到窗外 ⇒ 静默改写"的形态。
    #[test]
    fn enum_view_windows_keep_selected_option_visible() {
        let opts = |n: usize| -> Vec<OptionItem> {
            (0..n)
                .map(|i| OptionItem {
                    value: format!("v{i}"),
                    label: format!("L{i}"),
                })
                .collect()
        };

        // ① 装得下 ⇒ 全量、起点 0、选中即下标。
        let k = ConfigKind::Enum { options: opts(9) };
        if let ConfigKind::Enum { options } = &k {
            let v = enum_view(options, 3);
            assert_eq!((v.start, v.shown, v.selected), (0, 9, 3));
            assert_eq!(v.options.len(), 9);
            assert!(v.hidden_note.is_none(), "未截断 ⇒ 不弹说明");
        }

        // ② 装不下（10 段 = 评审实测触发"整页 Err"的那一档）⇒ 9 段窗口，**必含当前值**。
        let k10 = ConfigKind::Enum { options: opts(10) };
        if let ConfigKind::Enum { options } = &k10 {
            let last = enum_view(options, 9); // 当前值在最后一个
            assert_eq!(last.shown, ENUM_MAX_SEGMENTS);
            assert!(last.start + last.shown > 9, "窗口必须覆盖下标 9");
            assert_eq!(last.start + last.selected, 9, "窗口内选中段映射回契约下标 9");
            assert!(last.hidden_note.is_some(), "截断必须**上屏**说明（不静默）");

            let first = enum_view(options, 0);
            assert_eq!((first.start, first.selected), (0, 0));

            let mid = enum_view(options, 5);
            assert_eq!(mid.start + mid.selected, 5);
        }

        // ③ 空选项 ⇒ 单段占位（`SegmentedControl` 拒绝空列表 ⇒ 不能真建 0 段）。
        let k0 = ConfigKind::Enum { options: vec![] };
        if let ConfigKind::Enum { options } = &k0 {
            let v = enum_view(options, 0);
            assert_eq!(v.options.len(), 1);
            assert_eq!(v.options[0].label, PLACEHOLDER);
            assert_eq!((v.start, v.shown, v.selected), (0, 0, 0));
        }
    }

    /// 约束提示：整数类给「min – max [单位]」，`Ipv4` / `Enum` 不画（UI §6.2 两版式）。
    ///
    /// 敏感性：给 `Ipv4` 分支也返回 `Some` ⇒ 第 3 条变红（行型 B 会多出一行提示）。
    #[test]
    fn range_hint_shapes() {
        let u64k = ConfigKind::U64 {
            min: 1,
            max: 300,
            step: 1,
        };
        assert_eq!(range_hint(&u64k, None).as_deref(), Some("1 – 300"));
        assert_eq!(range_hint(&u64k, Some("秒")).as_deref(), Some("1 – 300 秒"));
        assert_eq!(range_hint(&ConfigKind::Ipv4, None), None);
        assert_eq!(
            range_hint(
                &ConfigKind::Enum {
                    options: vec![]
                },
                None
            ),
            None
        );
    }

    /// 不可用文案：空 reason 只显标题；非空则「标题 · 原因」且原因经 `display_safe`。
    ///
    /// 敏感性：去掉 `display_safe` ⇒ 第 3 条变红（ASCII `-` 须改写成 `–` U+2013，且小写
    /// 须转大写同族 —— 生成字体 cmap 里没有 `-`、多数小写也没有字形）。
    #[test]
    fn unavailable_text_shapes() {
        assert_eq!(unavailable_text(""), TEXT_CONFIG_UNAVAILABLE);
        assert_eq!(unavailable_text("   "), TEXT_CONFIG_UNAVAILABLE);
        // 自证：`display_safe("ab-cd")` 必须是 `AB–CD`（否则本条第 3 条断言的"改写确实发生"
        // 就不成立；`s` 是小写里少数有字形的字符之一，故取不含 `s` 的样例）。
        assert_eq!(display_safe("ab-cd"), "AB\u{2013}CD");
        assert_eq!(unavailable_text("ab-cd"), "配置不可用 · AB\u{2013}CD");
    }

    /// 变更明细：保存路径只列**本次改动**的字段；恢复路径列**全部可编辑**字段。
    ///
    /// 敏感性：把 `save_details` 的 `*now == f.value` 判断删掉 ⇒ 第 1 条变红；
    /// 把 `reset_details` 的 `filter(|f| f.editable)` 删掉 ⇒ 第 2 条变红。
    #[test]
    fn change_details_shapes() {
        let v = view();
        let mut cur = initial_values(&v);
        assert!(save_details(&v, &cur, &touched(&[])).is_empty(), "无改动 ⇒ 明细为空");
        cur.insert("gateway.port".into(), Value::from(2405));
        let d = save_details(&v, &cur, &touched(&["gateway.port"]));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].label, "端口");
        assert_eq!(d[0].before, "2404");
        assert_eq!(d[0].after, "2405");

        let r = reset_details(&v, &cur, &BTreeSet::new());
        let editable = iter_fields(&v).filter(|f| f.editable).count();
        assert_eq!(r.len(), editable, "恢复默认值列出全部可编辑字段");
        assert!(
            r.iter().all(|d| d.key != "display.bind_addr"),
            "只读字段不进明细"
        );
    }

    /// **弹层明细的"无法格式化"口径**（C1 的第 2 条）：`before` 侧回显的是**注入值**，它若不合法
    /// （如 `U16` 收到字符串）⇒ 明细里必须是 [`PLACEHOLDER`] 而**不是** `0`。
    ///
    /// 敏感性：把 [`format_value`] 的 `validate_value` 判据去掉（回到 `int_of(..).unwrap_or_default()`）
    /// ⇒ 第 1 条变红（读到 `"0"` —— 而 `0` 是**合法**配置值，等于把错误伪装成合法值）。
    #[test]
    fn change_details_never_disguise_bad_values_as_zero() {
        let mut v = view();
        // 造一个"注入值类型错配"的字段（`U16` 收到字符串），并让它**看起来**被用户改动过
        // —— 明细的 `before` 取自注入值，与 touched 无关，这里直接调 `reset_details` 更直白。
        for g in &mut v.groups {
            for f in &mut g.fields {
                if f.key == "intercore.port" {
                    f.value = Value::from("abc");
                    f.default = Value::from("abc");
                }
            }
        }
        let cur = initial_values(&v);
        let d = reset_details(&v, &cur, &BTreeSet::new());
        let row = d.iter().find(|x| x.key == "intercore.port").expect("明细含该字段");
        assert_eq!(row.before, PLACEHOLDER, "无法格式化 ⇒ 占位符");
        assert_ne!(row.before, "0", "**不得**把类型错配伪装成合法值 0");
        assert_eq!(row.after, PLACEHOLDER, "默认值同样不可格式化 ⇒ 占位符");
    }

    /// 整数边界：`u64` 超 `i64` 不 panic（收敛到 `i64::MAX`）。
    #[test]
    fn int_bounds_saturates() {
        let k = ConfigKind::U64 {
            min: 0,
            max: u64::MAX,
            step: 1,
        };
        assert_eq!(int_bounds(&k), (0, i64::MAX, 1));
        let u = ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 5,
        };
        assert_eq!(int_bounds(&u), (1, 65535, 5));
    }
}
