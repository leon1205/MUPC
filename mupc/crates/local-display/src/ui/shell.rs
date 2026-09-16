//! # `ui/shell.rs` —— 应用外壳（开发单元 **B2c-3**）
//!
//! 设计出处：UI 设计文档 §4.1（三区固定框架）/ §4.2（六页 ≤2 次触摸导航 / `NavTab` 规格）/
//! §4.3（返回 / 超时回归主状态页）/ §7.5（触摸不可用降级）/ §8.3（EDGE-20 两种状态同显）/
//! §5.1 #1·#3·#13·#15 / §5.2（状态 × 色值矩阵）/ §5.3（页面路由实现要点）；
//! 技术设计 §5.4（页面与状态模型）/ §5.2（事件循环与不变量）。
//!
//! ## 装配结构（三区 + 顶层浮层）
//!
//! ```text
//! root（1024×768，CLICKABLE —— 「全屏输入对象」，PRESSED ⇒ 重置空闲计时）
//! ├── header (0,0,1024,72)     返回 64×64（仅 P2–P6）/ 标题 32 / …… / 触摸角标 / 通道胶囊 / 时钟 26
//! │                           （**页眉右端组**自左向右：触摸角标 → 通道胶囊 → 时钟，见 **SH7**）
//! │                           倒计时胶囊出现时占**时钟左侧**、通道胶囊与角标让位（**SH3**）
//! ├── content (0,72,1024,624)  6 个页根容器（**任一时刻只显一个**）
//! ├── banner (16,72,1008,120)  未保存修改提示条（h48，仅 P2 `dirty` 时可见）
//! ├── nav (0,696,1024,768)     6 个 `NavTab`（170×72）
//! └── overlay (0,0,1024,768)   **整屏降级层（EDGE-03，B3-2b-1）**：**位于最后 ⇒ 画在最上层**；
//!                              常驻（`Shell::new` 里建好）**只切可见性**；**不带 `CLICKABLE`
//!                              ⇒ 可穿透输入**（见 §EDGE-03 的实现裁定）
//! ```
//!
//! ⚠️ 上述坐标**能成立的前提**：`root` 必须**清掉父主题内边距**（`theme::screen_bg()` 带
//! `set_pad_all(0)`），且宿主自身的 `pad_all` 为 0 —— 生产路径 `Obj::screen()` 满足。
//! 否则三区按内边距相对定位、整体内缩并右下出屏。该缺陷**确曾发生**（`screen_bg()` 漏了
//! `pad_all(0)`），**已修并上锁**（绝对坐标断言 + 双向破坏性探针），见偏差表 **SH14**。
//!
//! **弹层 / Toast 不在本文件**：P2 / P4 的 `ConfirmDialog` 与各页 `Toast` 都挂在
//! `lv_layer_top()`（见 `pages/p2_config.rs::show_dialog`）——它们在**整屏降级层之上**
//! （`lv_layer_top()` 高于活动屏），故通道断时弹层与写路径仍然完整可用（§8.3 EDGE-20）。
//!
//! ## EDGE-03 整屏降级：四条实现裁定（**开发单元 B3-2b-1**；主控已裁定，逐条给理由）
//!
//! §8.3 原文：「通道断（EDGE-03）｜全页｜整屏遮罩压暗 20 % + 中央 64 px `#FFB020`
//! 「与主进程数据通道断开」+ 下方 24 px 秒级恢复倒计时；保留最近有效帧并每区块打 `冻结`
//! 角标；恢复 ≤1 s 回实时」。
//!
//! 1. **层建一次、只切可见性**：整屏层在 [`Shell::new`] 里建好（`overlay` 字段，初始隐藏），
//!    运行期只 `set_hidden` / `set_text`。**绝不在渲染路径（`tick` / 回调）里建删 LVGL 对象**
//!    —— 回调内建删对象 = UAF 级风险，且每趟建删会把 LVGL 的**定容池**吃掉（本仓已有 OOM
//!    前科）。回归锁：`shell_chain` 在断态连推 50 拍后断言 **对象数**（`PROBE_MOUNTS`）
//!    **与样式挂载数**（`PROBE_STYLE_ATTACHES`）**双零增长** —— 后者是**另一条独立形态**
//!    （B3-2b-1 代码质量评审 重要 1）：`Obj::add_style` **只增不删**，"对象只建一次、却每拍
//!    挂一条样式"照样让 LVGL 样式表无界增长，而**对象数纹丝不动** ⇒ 单看对象数抓不到。
//! 2. **遮罩必须"可穿透输入"（不拦截触摸）**：§8.3 的 **EDGE-20**（同一条触发条件下）明写
//!    「**P2/P4 写操作仍可用**，实时数值沿用冻结帧并打标」。若遮罩拦截触摸，EDGE-20 即不成立。
//!    取"可穿透"即可**同时满足两行**（整屏压暗提示 + 写路径仍可用）⇒ 本层**显式**
//!    `remove_flag(CLICKABLE)`（`lv_obj` 构造时**默认带** `CLICKABLE`，
//!    `vendor/lvgl/src/core/lv_obj.c:584`；`pages::layout_box` 只摘 `SCROLLABLE`），
//!    子标签经 `pages::label` 建（它已摘 `CLICKABLE` / `SCROLLABLE`）。
//!    ⚠️ **与 `ConfirmDialog` 的遮罩语义相反**：那个遮罩**故意** `add_flag(CLICKABLE)`
//!    （`components.rs::ConfirmDialog::new`，注释原文「全屏、可点以拦穿透」）—— 模态弹层要求
//!    "点外面不落到页面上"；本层要求"点哪里都落到页面上"。两者在 `shell_chain` 里各有一条
//!    旗标断言，**改错方向即红**。
//! 3. **「秒级恢复倒计时」改实现为「秒级递增的已断开时长」**（偏差 **SH15**）：恢复时刻
//!    **不可预知**（取决于 mupcd 何时起）⇒ 递减倒计时**没有分母**。故下方 24 px 行显示的是
//!    「已断开 N 秒」，`N` **只在整秒变化时**才写一次文本（每拍刷 LVGL 文本是红线行为）。
//! 4. **「每区块打 `冻结` 角标」不由本层实现**（偏差 **SH16**）：本层只做"遮罩 + 中央文案 +
//!    时长行"，**不新增**任何按区块的角标。⚠️ **实际缺口是 P2–P6 五页**（早先写成"由既有
//!    §8.2 角标承担"**不实** —— 只有 **P1** 有等价标记；**P3 的构件存在但生产零调用者**）。
//!    逐页核查与两条 PM 裁定项见 **SH16 / SH18 / SH19**。
//!
//! **恢复 ≤1 s 回实时**：`ChannelStatus` 由 `Down` 变回非 `Down` 后，**下一拍**（`Shell::tick`）
//! 即撤遮罩并把断开始刻清零（不需要额外计时）；`shell_chain` 有"恢复即撤"断言。
//!
//! **触发面（有意收窄，如实标注）**：整屏层**只对 `ChannelStatus::Down` 显形**。
//! `ChannelStatus::Init`（尚未首次成功 GET）**不**走整屏层 —— §8.3 的异常态字典里**没有**
//! Init 的整屏行（`正在连接数据通道…` 只出现在 §3.6 **用字表**里），它的既有落点是
//! **页眉胶囊**（[`HeaderChannel::Connecting`]）+ P1 页内首连条（`p1_status::TEXT_CHANNEL_CONNECTING`）。
//! 若 PM 裁定 Init 也要整屏覆盖，改 [`Core::apply_overlay`] 的判据一处即可（并把文案参数化）。
//!
//! ## 本单元的边界
//!
//! **做**：外壳结构 / 路由 / 超时回归 / 未保存提示条 / 页眉右端（时钟 + 通道胶囊 + 触摸角标）/
//! 给 B3 的数据注入位。**B3-2b-1 追加**：**整屏降级（EDGE-03）本身**（上面四条裁定）+
//! 「`--idle-timeout-secs 0` = 禁用空闲回归」这一条 CLI 语义（原 `timing::IdleTimer` 承担，
//! 该类型本单元删除 ⇒ 语义唯一落点搬到 [`Shell::tick`]，见 **SH17**）。
//! **不做**：`console.rs` / 通道客户端 / 真实 HTTP / 帧解析 / `request_id` 生成 / 触摸设备初始化。
//!
//! ## 偏差登记（**编号 SH\*** —— 与 `D*` / `CD*` / `PD*` / `IL*` / `AU*` / `FR*` / `LG*` 不冲突）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 收口 |
//! |---|----------------------|------|------|
//! | SH1 | 导航页签**图标**用几何字形占位（`● ■ ▼ ⚠ ✓ ○`），非语义图标 | 生成字体 cmap 里的几何字形**只有 13 个**（U+2013/2190/2192/2212/2264/2265/25A0/25B2/25BC/25CB/25CF/26A0/2713，实测见 `fonts/lv_font_cmap.txt`），画不出「配置 / 日志 / 审计」这类专属图标；语义由 26 px **文字通道**承担（F14 的"文字 + 颜色"两通道齐备） | 字库扩充批（同 **D4/D5**）：§3.6 补图标清单 + 重跑 `gen_fonts.sh` |
//! | SH2 | 「弹层打开」在**生产侧**拿不到：P2 / P4 的 `with_dialog` 是 `#[cfg(test)]` ⇒ 本层用 [`Shell::set_modal_open`] **注入** | 页面侧没有生产可见的"弹层是否打开"查询口；B3 的 `UiState.confirm: Option<ConfirmDialog>` 本就是权威真源（技术设计 §5.4），由它注入即可 | **B3**：`app.tick` 内 `shell.set_modal_open(state.confirm.is_some())` |
//! | SH3 | **倒计时胶囊出现时，通道胶囊与触摸角标让位（隐藏）** | 页眉五者同时上屏的最小总宽 **> 1024 px**（算式：返回 64 + 标题 376 + 缝 16 + 通道胶囊 280 + 缝 16 + 倒计时胶囊 264 + 缝 16 + 时钟 120 + 右安全边 16 = **1168 px**，缺口 **144 px**；即便去掉返回键仍超 1104 px）——UI §4.1 的"右端时钟 + 通道胶囊"与 §4.3 的"时钟左侧倒计时胶囊"**在 1024 px 上不可共存**。倒计时只出现在回归前 ≤10 s 且最紧急 ⇒ 由它优先占据右端。**归属：PM 裁定**（规格层面的不可共存，非本层实现缺陷；评审已独立复算一致，且内在宽度估算 ≈1104 亦 > 1024） | **PM 裁定**。三个候选：① **让位**（现状，倒计时独占右端，本层已实现）；② **页眉改两行**（§3.5 的 `header_h=72` 要改 ⇒ 三区高度 72+624+72 的等式、全部页面的 y 坐标连锁，属**重新设计**）；③ **缩短文案**（如 `10 秒后返回主状态页` → `10 秒返回`，省 5×24=120 px **仍差 24 px** ⇒ 至少要砍 ≥6 个 24 px 字形才排得下，且须同步改 §3.6 用字表与 §4.3 文案契约）。本层**不自行改版式** **✅ PM 已裁定（2026-09-15）**：维持「**倒计时占槽、通道胶囊与触摸角标让位**」——倒计时胶囊与通道胶囊**本就抢同一个槽位**（都锚在时钟左侧），让位窗口仅**超时前 10 s**、窗口结束即自动回 P1（通道态在 P1 完整重现）。UI §4.1 已补「页眉右端组坐标链 + 让位优先级（倒计时 > 通道态 > 触摸角标）」注，见 UI 附录 **A.7** |
//! | SH4 | 「放弃修改」按钮触区高 **48**（UI §4.3 同一条里又写「**64 高触区**」）—— **规格自身矛盾**（评审 ⑤ 已裁定：取 48 正确，实现不改） | **矛盾**：§4.3 的同一表格行同时给"提示条 h 48"与"按钮 64 高触区"，64 高的按钮放不进 48 高的条（子对象被父对象裁切）。**取 48**，理由：① 48 = `Dimens::TOUCH_MIN`（§2.1 的最小触摸目标**下限**，满足）；② §2.1 的「关键操作 **64×64**」点名清单（保存 / 恢复默认值 / 联锁释放 / M1 授权 / 导航项 / **返回**）**不含**它 ⇒ 无 64 的硬要求；③ 取 64 会让提示条与按钮互相矛盾（只能改条高，而那又违反同一行的 h48） | 同 SH3（若 PM 裁定条高改 64，两者同时改） |
//! | SH5 | 「**任何**触摸事件重置计时」（§4.3）在本层**只覆盖 3 个控件**：返回键 / 6 个页签 / 「放弃修改」键（**按下即重置**）。**不会**重置的按压：页内任意控件（卡片 / 按钮 / 列表行 / chip …）、页内空白处、页眉空白区、导航条页签之外的空隙。**修整说明（评审 ④）**：原文自称覆盖"外壳根（**全屏**）"，**不成立** —— 已删；现有用例里 `sh.obj().send_event(PRESSED)` 是**合成投递**（不经 `lv_indev`），它锁的只是"根的挂钩**在**且能置位 `pending_activity`"，**不等于**产品里页内按压会重置 | 三件事（**② 探针已逐条实测**，见 §验证）：① `crate::lvgl::obj::ObjFlag` **无** `LV_OBJ_FLAG_EVENT_BUBBLE`（只有 HIDDEN / CLICKABLE / CHECKABLE / SCROLLABLE），而 LVGL **默认不上冒**：`lv_obj_event.c:434::event_is_bubbled` 要求**当前目标自带该标志**、`event_send_core` **逐级**检查 ⇒ 事件要到达外壳根，**链上每一层**都得带标志；② `lv_obj` 构造时**默认 `CLICKABLE`**（`vendor/lvgl/src/core/lv_obj.c:584`），而页眉（0,0,1024,72）/ 内容区（0,72,1024,624）/ 导航条（0,696,1024,72）**恰好铺满** 1024×768，`lv_indev.c:618::lv_indev_search_obj` 取"命中的**最深**可点对象" ⇒ **真实触摸永远落在某个后代对象上，外壳根收不到 `PRESSED`**；③ `crate::lvgl::indev::Indev` **未**暴露 `lv_indev_add_event_cb`（该符号**在** `lvgl-sys/allowlist.txt` 里，只是薄层没封装）⇒ `ui/**` 拿不到"任意按压"的全局钩子 | **薄层（`src/lvgl/**`）**，两种**具名**能力需求（二选一）：① `ObjFlag` 补 `EVENT_BUBBLE` —— **注意**：只给页根置位**不够**，须由外壳在装配后**递归**遍历页眉 / 内容区 / 6 页 / 导航条**整棵子树**置位（链上缺一层即断）；② 给 `Indev` 加 `on(EventCode, F)`（`lv_indev_add_event_cb` 直投；`lv_indev.c:997` 的 `send_event(LV_EVENT_PRESSED, indev_act)` 是**每次按压**都发）—— **推荐**：一处挂钩覆盖全屏，且不必触碰 `pages/**`。**两者任一补齐后**，§4.3 的"任何触摸事件重置"才**完全**成立；届时应把 `shell_chain` 的"页内按压**不**重置"那条断言（**现状锁定**，见下）改写成"页内按压**也**重置"，**不得**只是删掉 |
//! | SH6 | 倒计时胶囊的**出现判据取 ≤10 s**（§4.3 表「超时前 10 s」），故文案从 `10 秒后返回主状态页` 起数；§4.3 表内的示例文案写的是 `12 秒后返回主状态页` —— **规格自身矛盾**（评审 ⑤ 已裁定：取 10 s 正确，实现不改） | **§4.3 自相矛盾**（判据 10 s vs 示例 12 s）。**取判据 10 s**，为什么：① 判据是**行为规格**（"超时前 N 秒出现"，可被 PRD F15 与计时器逐拍核验），而 `12 秒后返回主状态页` 只是表格里的一句**示例文案**（同格的判据已写死 10 s，示例与之冲突 ⇒ 示例才是笔误的一方）；② 取 12 s 会出现"判据说 10 s 显、文案从 12 s 起数"的**自相矛盾**，或需要把判据一并改 12 s（改动行为规格，超出本层权限）。文案按**实际剩余秒数**渲染（`countdown_text`） | 无（**有意**取行为规格）；若 PM 裁定 12 s，改 [`COUNTDOWN_WINDOW_SECS`] 一处即可 |
//! | SH7 | **页眉通道胶囊的位置**：EDGE-20 的"页眉**左侧** `与主进程数据通道断开`（红）+ 右侧正常时钟"落成「**页眉右端组**内的红通道胶囊 + 右端时钟**同时可见**」（`HEADER_CHIP_X`：胶囊紧跟时钟、角标在胶囊左侧，三件等缝相连、整组贴右安全边）。**位置取 §4.1（右端），§8.3 的「左侧」不采**；**§8.3 的语义（红断开 + 时钟正常"两状态同显"）完整满足** —— 红胶囊与时钟同屏可见 | §4.1 与 §8.3 对**同一元素**给出不同横坐标（§4.1「右端：时钟 `+` 通道状态胶囊」/ §8.3「左侧」）。取 §4.1 的三条理由：① §4.1 是**版式权威**（页眉各件的矩形与"右端"归属都在它的表里），§8.3 是**异常态语义字典**（管"该显哪种状态"，不管坐标）；② §4.3 把倒计时胶囊钉在"页眉右端（**时钟左侧**）" ⇒ 时钟必须留在右端；若把胶囊改挂左端，同一元素会**按状态跳位**，且左端已被返回键（x 12–76）与标题（P1 标题 x16 起、宽 [`HEADER_TITLE_W_P1`]）占满，移过去还要压标题；③ 采纳 (a)「移到右端」而非 (b)「断开态移左端」正是为了**位置恒定的状态件**（F14 一致性）。**EDGE-20 的完整语义**（"控制通道**可达** vs 读通道**断**"的二元区分）需要**两个**独立通道信号，本层只有一个 `ChannelStatus` 输入 ⇒ 归 **B3** | **B3**：`set_channel` 之外再注入"控制通道态"；若 PM 裁定必须落在"左侧"，需同时裁定标题区收缩 + 胶囊换位（属**重新设计**，本层不自行改）。回归锁见 `shell.rs::tests::header_slots_are_disjoint_and_inside_canvas` 的"右端组右锚定 + 左缘在右半区"两条 |
//! | SH8 | **【✅ 已实现，残余 = SH15 / SH16 / SH17】整屏降级（EDGE-03）：压暗 20 % + 中央 64 px 文案 + 秒级时长**已落成（"每区块 `冻结` 角标"的**实际缺口**见 **SH16**；"秒级倒计时"改"已断开时长"见 **SH15**；`--idle-timeout-secs 0` 语义搬迁见 **SH17**） | 曾属 **B3**（需通道客户端 + 恢复状态机）—— B3-2b-1 已交付 | **✅ 已实现（2026-09-15，B3-2b-1）**：整屏层落成 `Core::overlay`（`root` 的末子 ⇒ 最上层）+ [`Core::apply_overlay`]（每拍唯一落点），四条实现裁定见模块头「EDGE-03 整屏降级」。**B3-2b-1 规格符合性评审整改（建议 4）**：原"只留挂点 `Shell::overlay_layer()`"里的**挂点已删**（它与 `widgets::layer_top()` 等价、只多包一层 `Result`，且**无生产消费者** —— P2/P4 弹层与 Toast 直接调 `widgets::layer_top()`）⇒ 本行不再是"挂点"条目，而是"EDGE-03 已实现"的登记 |
//! | SH15 | **EDGE-03 原文的「秒级恢复倒计时」改实现为「秒级递增的已断开时长」**（下方 24 px 行 = `N 秒`） | 原文写"倒计时"，但**恢复时刻不可预知**（取决于 mupcd 何时起）⇒ 递减倒计时**没有分母**（既无"总时长"，也无"预计恢复时刻"这类输入）。本层只有 `ChannelStatus` 一个通道态输入，**没有**任何可推算恢复时刻的真源 ⇒ 递减会编造一个假的分母 | **PM 裁定项**：若确需递减，须先给出「恢复时刻预测源」（如后端提供重连退避进度 / 预计恢复时刻）；当前不存在 ⇒ 本层按"已断开时长"落地。文案字符全部取自 §3.6 用字表（`秒` + 数字，既有用法：`12 秒后返回主状态页` / `保持时间不足，还需 12 秒`），**未自造任何新字**（**非**"新增上屏串"：见 [`disconnect_elapsed_text`] 的说明） |
//! | SH16 | **EDGE-03 原文的「保留最近有效帧并每区块打 `冻结` 角标」不由本层实现**（本层只做遮罩 + 中央文案 + 时长行）—— **实际缺口 = P2 / P4 / P5 / P6 四页 + P3**（**订正**：早先写成"由既有 §8.2 角标承担"，把 P3 的**构件存在**当成了**标记生效**，**不实**；见下） | 逐页核查（**2026-09-15 规格评审复核后订正**）：§3.6 用字表里确有 `冻结`，但 `ui/pages/**` 六个文件里**没有任何一页**把它上屏（`grep 冻结 src/ui/pages` 结果 = 注释）。各页现有的等价可见标记是**别的名字**：① **P1** —— 页内通道条 [`p1_status::P1StatusPage::channel_text`]（通道断 ⇒ `与主进程数据通道断开`）+ 「`数据过期`」角标（`stale_visible()`，**仅在通道正常但帧旧时**打）+ 逐相/逐字段状态点（`Palette::BORDER_CTRL` 空心 = 停更，见 `p1_status::phase_dot_color` / `state::live_dot_for`）；② **P3** —— 页内 `实时日志已断开 · 正在重连...` 条（**构件存在，但生产零调用者**：`P3LogsPage::set_channel(bool)`（`p3_logs.rs:2118`）全仓只有 `ui/tests.rs` 三条用例调它 ⇒ 读通道断时 P3 **不会**自动显示该条 ⇒ **不计入**"有等价标记"）；③ **P2 / P4 / P5 / P6 在"保留最近有效帧"时没有任何可见标记**（数值照旧显示，与实时不可区分）—— 实测：`grep -n "input.channel" src/ui/pages/*.rs` **只命中 P1**（`p4_interlock.rs:1945` 的注释亦自陈"本页不消费 `freshness` / `channel`"）⇒ 通道级降级对这四页**结构上不可见**。**★ 连带发现（如实登记，非本单元引入）**：P3 的 `connected` 缺省 `false`（`p3_logs.rs:1571`，**B2c-2** 引入）⇒ **生产上 P3 恒显"已断开"**（与实际不符的**静态**文案，方向上 fail-closed）。属**接线缺口**：应由 **B3-2b-2** 把真实读通道态喂进 `set_channel`；**现在缺什么** = `app`/`Shell` 侧对 `p3().set_channel(..)` 的调用（`Shell::set_channel` 只驱动页眉胶囊，**不转发到页**） | **PM 裁定项（本层不擅自改 6 页版式）**：候选 = ① 由各页 §8.2 状态点承担（需先给 P2/P4/P5/P6 补"帧冻结"驱动源 —— 它们**不消费** `freshness`，属**页面层**改动）；② 在整屏层按区块画 `冻结` 角标（需知道"每区块"的矩形，本层拿不到，且会与页面版式耦合）。本单元**只登记、不改六页**（任务书明令）。P3 接线缺口同归 **B3-2b-2** |
//! | SH18 | **弹层遮蔽**：EDGE-20 要求"控制通道可达 ⇒ P2/P4 写操作仍可用"，实现上 `ConfirmDialog` 挂 `lv_layer_top()`、**压在整屏遮罩之上**（模块头裁定 ②）⇒ 写操作走到"确认"阶段时，**屏上没有任何"数值已冻结"提示**（遮罩被弹层盖住）—— 而**这恰是操作者据冻结数值做决定的那一刻**：最需要"这是冻结帧"警示的时机，反而完全无提示 | 层级**有意**如此（`lv_layer_top()` 高于活动屏 ⇒ 弹层永远在整屏层之上，这正是 EDGE-20 写路径可用的**前提**）。问题**不在层级**，而在"提示**只**在整屏层表达" | **PM 裁定项（本单元不实施：跨页改动 + 需 PM 裁定）**。**最小改法建议**：让 P2 / P4 在 `channel == Down` 时对**受影响数值区**复用既有 §8.2 状态点 / `数据过期` 口径（`state::live_dot_for`）—— 即"该页本来就有的构件换一个输入源"，而**不是**把"每区块角标"塞进整屏层 |
//! | SH19 | **EDGE-20 的"打标"要求未落实**：原文要求"实时数值沿用冻结帧**并打标**"。本层实现的是**整屏遮罩** —— 它是**全局**表达（"整屏都不可信"），**替代不了** §8.2 的**逐字段**可信度标记：屏上哪个数值刚收到、哪个是三分钟前的，整屏遮罩**区分不了** | §8.3 的"并打标"与 §8.2 的"逐字段状态点"是**同一套**语义 —— 可信度是**逐字段**属性，遮罩是**全局**属性，两者不同轴 | 同 **SH18**（**PM 裁定项**）：逐字段标记的驱动源在各页（`freshness`）；最小改法 = P2–P6 复用 `state::live_dot_for`，而**不是**在整屏层造角标。本单元**只登记** |
//! | SH17 | **`--idle-timeout-secs 0` 的「0 = 禁用空闲回归」语义**经本单元搬到 [`Shell::tick`]（原落点 `timing::IdleTimer` / `CliConfig::idle_timer` **已按 B3-2b-1 任务书删除**）| 该语义**原先只有 `IdleTimer` 实现**（`is_disabled()`）；删除后若不搬迁，`--idle-timeout-secs 0` 会退化成"每拍强制回 P1"（`remaining_secs` 恒 0 ⇒ `should_return_home(0)` 恒真）—— 与 `--help` 的「0=禁用」和 `config.rs` 用例的直接矛盾，且**屏上表现为"用户根本停不在 P2–P6"**。另一条边界（`IdleTimer::new` 的秒→毫秒**饱和**）经核实**不可达**：`--idle-timeout-secs` 由 `parse_u64_range(.., 0, MAX_IDLE_TIMEOUT_SECS = 3600)` 限定 ⇒ `Duration::from_secs(3600)` 与 `checked_add` 永不溢出 | 无（已实现 + 回归锁：`shell_chain` 的「`--idle-timeout-secs 0` ⇒ 不强制切页 / 不显示倒计时」）。**PM 复核项**：`config.rs` 的 CLI 文案与用例仍以 0=禁用 为准，本层与之对齐 |
//! | SH9 | 页内通道条（P1 的「与主进程数据通道断开」行）与页眉通道胶囊**重复表达**同一事实 | `pages/mod.rs` 的 **D1** 已登记"外壳装配时移除页内通道条"，但本单元**禁改 `ui/pages/**`**（硬约束 5）⇒ 重复仍在 | **B2c 收口 / 后续批**（需 PM 授权改 `p1_status.rs`） |
//! | SH10 | 导航 6 项各取 `Dimens::NAV_ITEM_W` = **170**，共 1020 px < 1024（右端余 **4 px** 无页签） | `Dimens::NAV_ITEM_W`（UI §5.1 #1 取整）是 theme 的**单一真源**，本层不得写 170.7；UI §4.2 写"每项宽 1024/6 ≈ 170.7" | 无（**有意**用 theme 常量；4 px 余量不构成可用触摸区） |
//! | SH11 | **导航页签的图标 / 文字 y 与 §4.2 不符**：§4.2 给「图标 28 px（y 706–734）」⇒ **项内 y 10**、「文字 26 px（y 736–766）」⇒ **项内 y 40**；本层取 [`NAV_ICON_Y`] = **8**、[`NAV_TEXT_Y`] = **44**（图标高 2 px、文字低 4 px） | **不是居中推导**（如实核实）：块高 = 28+缝+26 = 62（含 8 px 缝），真居中应得 5/41；§4.2 的 10/40 自身也不居中（块 10–70、上 10 下 2）。本层取"**半个呼吸缝**"档：图标上沿 = `Dimens::GAP_MIN/2` = 8，图标下沿与文字上沿之间同样 8 ⇒ **等间距**取向，且**全部由 theme 常量派生**（`Dimens::GAP_MIN` / `Dimens::ICON_SM`），零裸值。§4.2 的 10/40 在 theme 里**没有**对应命名常量，而 `ui/theme.rs` 本批**禁改** ⇒ 本层不能写 10/40 | **theme.rs 收口批**：上收 `Dimens::NAV_ICON_Y` / `Dimens::NAV_TEXT_Y`（或 §4.2 逐像素值），本层改为一行引用 **✅ PM 已裁定（2026-09-15）**：UI §4.2 的 y 值已由「图标 706–734 / 文字 736–766」（与**自述的** 28 / 26 px 不符，且 30 px 文字槽未按 26 px 居中）订正为**相对口径**：图标 **y 8**（绝对 704–731）、文字 **y 44**（绝对 740–765），与实现一致，见 UI 附录 **A.7** |
//! | SH12 | **§4.3「超时回归…不改变页面滚动位置以外的状态（P1 始终从顶部开始）」未实现**（**能力缺口**） | 薄层 `src/lvgl/**` **没有**"滚到指定位置"的封装：`Obj` 只暴露 `set/get_scroll_dir` 与 `set/get_scrollbar_mode`，`lvgl-sys/allowlist.txt` 里**也**没有 `lv_obj_scroll_to_y`（`lv_obj_scroll_to_y` / `lv_obj_scroll_to` / `lv_obj_scroll_by` 均未放行）⇒ `ui/**` **做不到**"把 P1 滚回顶部"。**影响**：超时从 P2–P6 回归 P1 时，P1 页内**保留上次的滚动位置**（用户上次在 P1 滚到中段 ⇒ 回归后仍在中段，与 §4.3 的"始终从顶部开始"不符）。**等效替代**（不需新 API，但**未采纳**）：回归时销毁并重建 P1 页根 —— 代价是 P1 的全部注入态（告警列表 / 遥测值 / 滚动条）与回调槽一并丢弃后要由 B3 重灌，且重建/拆除会走 `pages/**`（本批**禁改**），收益不成比例 | **薄层（`src/lvgl/**`）**：具名需求 = 新增 `Obj::scroll_to_y(y)`（或 `scroll_to(x, y)`）一行封装 + `allowlist.txt` 放行 `lv_obj_scroll_to_y`；外壳则在 `Core::select` 切回 P1 时调用一次。**当前无回归锁**（做不到 ⇒ 无法断言），故只登记不锁 **✅ PM 已裁定（2026-09-15）**：判定为**具名能力缺口**（薄层无 `lv_obj_scroll_to_y`），UI §4.3 已加注；补该能力归「薄层收口批」，届时本条改为可断言，见 UI 附录 **A.7** |
//! | SH13 | §4.1 线框图里"页眉与内容区之间 **y72** 那条横线"**未实现**（`header_bg` / `content` 均无描边） | **判定为示意线，非必做项**：① §4.1 的**表格**（版式权威）对 HDR 只写"常驻。左：… 中/右：…"、对 CONTENT 只写"整页纵向滚动（LVGL 滚动容器），左右安全边 16 px"，**均无分隔线项**；② §3.2 的 `divider` 用途表列的是"卡片描边、行分隔、滚动条轨道"，**不含**页眉分界；③ 同一张线框图还画了外框与 y768 底边（画布边界，显然不是 UI 元素）⇒ 该图是**读图辅助**。**影响**：无（页眉 `Palette::SURFACE` 与内容区 `Palette::BG` 本身有底色差，分界可见） | 无（若 PM 裁定必做：`ui/theme.rs` 加 `theme::header_rule()` 并在 `header` 上加下描边 —— 属 **theme.rs 收口批**） **✅ PM 已裁定（2026-09-15）**：判定为**示意图、非必做**（§4.1 未规定该线的线宽 / 色值 / 归属），UI §4.1 已加注，见 UI 附录 **A.7** |
//! | SH14 | **【本批已修】三区绝对坐标曾整体内缩 20 px**：`root` 用的 `theme::screen_bg()` 原本**没有** `set_pad_all(0)`，而 LVGL 默认主题给**每个** `lv_obj` 挂 `card` 样式（`vendor/lvgl/src/themes/default/lv_theme_default.c:262` 给 `styles.card` 设 `pad_all = PAD_DEF`，`:794` 把它挂到每个 `lv_obj`；1024×768 屏实测 = `LV_DPX_CALC(130, 24)` = **20 px**）⇒ `header` / `content` / `nav` 的 `set_pos` 是**内边距相对**值，整层右下出屏。**2026-09-15 实测（生产等价宿主）**：`header` 落 **(20,20)-(1043,91)**（右缘出屏 20 px）、`nav` 底 **787**（出屏 20 px）、`page_host` 落 (36,92) 而非契约的 (16,72) ⇒ **6 页全部偏移**。**修法**：`theme.rs::screen_bg()` 补 `set_pad_all(0)` + `set_radius(Radius::NONE)`（与 `theme::transparent()` / `dialog_mask()` 同款；该函数**仅**本文件 `Shell::new` 一处使用，改动不外溢）；测试宿主同步改为「屏的忠实替身」（挂 `theme::transparent()` —— 生产屏自带 `pad_all = 0`），并在 `shell_chain` 补 **⑦ 三区绝对坐标**断言。**两条破坏性探针**（证明有网）：摘 `screen_bg` 的 `pad_all` ⇒ red `(20,20,1043,91)`；摘宿主的 `transparent()` ⇒ red `(22,22,1045,93)` | 原判「本批不改、另立单元」**已撤销**：该缺陷使 B2c-3 在契约的**绝对坐标**上不合规，而修法只碰 shell 自己用的那个样式函数。**同类陷阱**：`ui/**` 静态扫描只管裸色值/文本输入控件，管不到「沿用父主题内边距」；`pages::layout_box` / `components.rs::layout_box` / `dialog_mask` 早有防护，唯独 `screen_bg` 漏了 ⇒ 日后新增「挂对象上的样式构造函数」**必须**一并清 `pad_all` / `radius` |
//!
//! ## 不变量（编码约束，逐条对应技术设计 §5.2）
//!
//! 1. **回调内不做阻塞 I/O、不 panic**：本文件全部回调只写 `Cell` / `RefCell` 与 LVGL 属性；
//! 2. **不在渲染回调内创建 / 删除对象**：唯一的对象创建在 [`Shell::new`] 与
//!    [`Shell::new`] 的同批装配内；运行期只做 `set_text` / `set_hidden` / `set_pos` / `set_size`；
//! 3. **不读时钟**：`now` 一律经 [`Shell::tick`] 注入 ⇒ 超时链路离屏可确定性复现；
//! 4. **`Rc` 不成环**：外壳持页（单向），页不持外壳；回调一律 `Weak<Core>` + `upgrade`；
//!    回调槽一律走 `pages::CbSlot`（不得手写 take / put-back）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::lvgl::event::EventCode;
use crate::lvgl::obj::{Obj, ObjFlag};
use crate::lvgl::style::{Color, Opa, State, Style, StyleSelector};
use crate::lvgl::widgets::{Label, LongMode, TextButton};
use crate::lvgl::LvglError;
use crate::state::ChannelStatus;
use crate::ui::components::StatusChip;
use crate::ui::pages::{
    decor, label, layout_box, p1_status, p2_config, p3_logs, p4_interlock, p5_audit, p6_system,
    set_style_index, set_visible, CbSlot,
};
use crate::ui::theme::{
    self, ButtonKind, ChipSkin, Dimens, Opacity, Palette, Radius, Stroke, TextSlot,
};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 外壳专属栅格常量（**逐条由 theme 常量推导**；见 UI §4.1 / §4.2 / §4.3）
//
// 纪律（与 `pages/**` 同款）：本块是外壳版式的**唯一**数字来源；调用点的
// `set_size` / `set_pos` 实参一律是常量或变量（`ui/tests.rs::ui_layout_setters_use_theme_constants`
// 静态网逐条扫），且常量初始化式**不得**是裸十进制整数
// （`ui_const_i32_definitions_derive_from_theme` 静态网）。
// ═══════════════════════════════════════════════════════════════════════════

/// 页眉：返回键 x（UI §4.1「x 12–76」= 左安全边 16 内缩一个强调条宽 4）。
const HEADER_BACK_X: i32 = Dimens::SIDE_PAD - Dimens::ACCENT_BAR;
/// 页眉：返回键 y（72 − 64 后居中 → 4；UI §4.1「y 4–68」）。
const HEADER_BACK_Y: i32 = (Dimens::HEADER_H - Dimens::TOUCH_CRITICAL) / 2;
/// 页眉：P1 标题 x（UI §4.1「P1 左为标题文字 x 16」）。
const HEADER_TITLE_X: i32 = Dimens::SIDE_PAD;
/// 页眉：P2–P6 标题 x（UI §4.1「x 96 起」= 返回键 64 + 左右各 16）。
const HEADER_TITLE_X_PAGED: i32 = Dimens::TOUCH_CRITICAL + 2 * Dimens::SIDE_PAD;
/// 页眉：标题 y（72 − 32 后居中 → 20）。
const HEADER_TITLE_Y: i32 =
    theme::center_offset(Dimens::HEADER_H, TextSlot::PageTitle.px() as i32);
/// 页眉：时钟宽（UI §4.1「时钟 26 px 等宽」，`HH:MM:SS` 八字符）。
const HEADER_CLOCK_W: i32 = Dimens::BTN_MIN_W;
/// 页眉：时钟 x（贴右安全边）。
const HEADER_CLOCK_X: i32 = Dimens::SCREEN_W - Dimens::SIDE_PAD - HEADER_CLOCK_W;
/// 页眉：时钟 y（72 − 26 后居中 → 23）。
const HEADER_CLOCK_Y: i32 = theme::center_offset(Dimens::HEADER_H, TextSlot::Label.px() as i32);
/// 页眉：通道胶囊宽（`与主进程数据通道断开` 10 字 × 24 + 图标位 32 + 右留白 16 = 288 ⇒ 取 280+）。
const HEADER_CHIP_W: i32 = Dimens::BTN_MAIN_W + Dimens::TOUCH_CRITICAL + Dimens::GAP_MIN;
/// 页眉：通道胶囊 x —— **页眉右端组的最左成员，紧跟时钟**（UI §4.1「右端：时钟 26 px 等宽
/// + 通道状态胶囊 h 32」；**SH7** 登记了同一元素在 §8.3 EDGE-20 里被写成「左侧」的冲突）。
///
/// 右端组 = `… 触摸角标 → 通道胶囊 → 时钟 →|右安全边`，相邻件之间恒一个呼吸缝：
/// `HEADER_CHIP_X + HEADER_CHIP_W + GAP_GROUP == HEADER_CLOCK_X`。
/// `shell.rs` 的 [`tests::header_slots_are_disjoint_and_inside_canvas`] **逐条断言**这条锚定链。
///
/// **`pub` 是必需的**：`ui/tests.rs::shell_chain` 要拿它做**精确相等**断言
/// （`shell::channel_chip_x() == Some(HEADER_CHIP_X)`）。早先那条断言写成"左缘落在右半区"
/// （`>= SCREEN_W/2`），**有 80 px 盲窗**：把 `Shell::new` 的调用点写成 512（与触摸角标重叠
/// 64 px、离契约位 80 px）时**纯逻辑网与对象网同时保持绿**（2026-09-15 代码质量评审探针 P5
/// 实测）。**教训**：凡"落在某半区 / 在某范围"的判据都要问一句"窗口里还有多少错值能通过"。
pub const HEADER_CHIP_X: i32 = HEADER_CLOCK_X - Dimens::GAP_GROUP - HEADER_CHIP_W;
/// 页眉：胶囊类元素 y（72 − 32 后居中 → 20）。
const HEADER_CHIP_Y: i32 = theme::center_offset(Dimens::HEADER_H, Dimens::STATUS_CHIP_H);
/// 页眉：触摸不可用角标宽（`触摸不可用` 5 字 × 24 + 右留白）。
const HEADER_BADGE_W: i32 = Dimens::BTN_MIN_W + Dimens::TOUCH_MIN;
/// 页眉：触摸不可用角标 x（**通道胶囊左侧**一个呼吸缝 —— 角标属 §7.5 的「页眉右端」，
/// 与通道胶囊 / 时钟同处右端组；见 [`HEADER_CHIP_X`] 与 **SH7**）。
const HEADER_BADGE_X: i32 = HEADER_CHIP_X - Dimens::GAP_GROUP - HEADER_BADGE_W;
/// 页眉：触摸不可用角标 y（72 − 24 后居中 → 24）。
const HEADER_BADGE_Y: i32 = theme::center_offset(Dimens::HEADER_H, TextSlot::Body.px() as i32);
/// 页眉：倒计时胶囊宽（`10 秒后返回主状态页` 11 字形 × 24 + 两侧留白 = 264）。
const HEADER_CAPSULE_W: i32 = Dimens::BTN_MAIN_W + Dimens::TOUCH_CRITICAL;
/// 页眉：倒计时胶囊 x（UI §4.3「时钟左侧」）。
const HEADER_CAPSULE_X: i32 = HEADER_CLOCK_X - Dimens::GAP_GROUP - HEADER_CAPSULE_W;
/// 页眉：P2–P6 标题宽（到**页眉右端组最左成员**（触摸角标）左侧一个呼吸缝为止）。
const HEADER_TITLE_W: i32 = HEADER_BADGE_X - HEADER_TITLE_X_PAGED - Dimens::GAP_GROUP;
/// 页眉：P1 标题宽（无返回键，故比 P2–P6 多出返回键与两缝的宽度）。
const HEADER_TITLE_W_P1: i32 = HEADER_BADGE_X - HEADER_TITLE_X - Dimens::GAP_GROUP;

/// 导航：图标 y（项内上沿；UI §4.2「图标 28 px，y 706–734」⇒ 项内 y 10，**本层取 8** ——
/// 见偏差 **SH11**：8 = 半个呼吸缝，与规格的 10 差 2 px）。
const NAV_ICON_Y: i32 = Dimens::GAP_MIN / 2;
/// 导航：文字 y（图标下沿 + 半个呼吸缝 ⇒ 44；UI §4.2 给的是 40 —— 见偏差 **SH11**）。
const NAV_TEXT_Y: i32 = NAV_ICON_Y + Dimens::ICON_SM + Dimens::GAP_MIN / 2;
/// 导航：相邻项竖分隔线宽（UI §4.2「1 px `#2A3B57`」）。
const NAV_DIVIDER_W: i32 = Stroke::THIN;
/// 导航：顶部选中条 y（贴项顶）。
const NAV_BAR_Y: i32 = 0;

/// 内容区：页根容器的 y（页眉之下）。
const CONTENT_Y: i32 = Dimens::HEADER_H;

/// 未保存提示条：高（UI §4.3「h 48」= 最小触摸目标）。
const BANNER_H: i32 = Dimens::TOUCH_MIN;
/// 未保存提示条：图标 x（UI §2.5 警示行左内边距）。
const BANNER_ICON_X: i32 = Dimens::GAP_MIN;
/// 未保存提示条：文案 x（图标 + 图标宽 + 呼吸缝）。
const BANNER_TEXT_X: i32 = BANNER_ICON_X + Dimens::ICON_SM + Dimens::GAP_MIN;
/// 未保存提示条：图标 y（48 − 28 后居中 → 10）。
const BANNER_ICON_Y: i32 = theme::center_offset(BANNER_H, Dimens::ICON_SM);
/// 未保存提示条：文案 y（48 − 24 后居中 → 12）。
const BANNER_TEXT_Y: i32 = theme::center_offset(BANNER_H, TextSlot::Body.px() as i32);
/// 未保存提示条：「放弃修改」按钮宽（逐字宽 = 字号；薄层无文本度量接口，与 `pages/**` 同口径）。
fn discard_w() -> i32 {
    TEXT_DISCARD.chars().count() as i32 * TextSlot::Label.px() as i32 + 2 * Dimens::GAP_MIN
}

/// 倒计时窗口：进入"超时前 N 秒"即显示胶囊（UI §4.3 表「超时前 10 s」）。
pub const COUNTDOWN_WINDOW_SECS: u64 = 10;

// ── 整屏降级层（EDGE-03，B3-2b-1）──────────────────────────────────────────
//
// 版式：全屏遮罩（压暗 20 %）+ **中央** 64 px 大字 + **其下** 24 px 时长行。
// 「中央」的判定与页眉各槽位同口径：整块（大字 + 跨区呼吸缝 + 时长行）**垂直居中**，
// 大字**水平居中**（时长行随文本宽度逐次居中，见 `overlay_elapsed_x`）。

/// 中央大字文案（UI §3.6「全局降级」行 = `与主进程数据通道断开`，**与页眉红胶囊同一份
/// 字面量** —— 唯一真源 `p1_status::TEXT_CHANNEL_DOWN`）。
const OVERLAY_TEXT: &str = p1_status::TEXT_CHANNEL_DOWN;
/// 时长行文案的模板（`N 秒`；数字与 `秒` 均取自 §3.6 用字表，**未自造新字**）。
const OVERLAY_ELAPSED_SUFFIX: &str = " 秒";

// ── 两个标签的「槽 + 色」**单一绑定**（B3-2b-1 规格评审 **重要 1+2**）────────────
//
// **背景（评审实测的三处静默退化之一）**：原实现里"建标签"写 `TextSlot::PhasePower`、
// "设尺寸"写 `overlay_title_w()`（它**自己内部**又写了一遍 `TextSlot::PhasePower`），
// 两处**各写各的常量** ⇒ 把其中一处换成别的槽时另一处不受影响，于是"用错常量"只表现为
// **字号与对象尺寸不一致**（屏上肉眼可见的字变小 / 色变红），而**全套用例全绿**
// （薄层无 `text_font` / `text_color` / `bg_opa` 读回，见 `src/lvgl/mod.rs` 的
// 「薄层能力缺口登记」）。
//
// **收口口径**：把两处**收敛成同一个表达式** —— 标签构造与 `set_size` 都读这里的常量，
// 而 `OVERLAY_BLOCK_H` / `OVERLAY_ELAPSED_Y` / `overlay_*_w()` 也**全部**由它们派生
// ⇒ "换槽"只可能改这一个地方，改完 `shell_chain` 的对象级尺寸断言**必红**（实测见交付报告）。
//
// **改什么会让本条变红**：把 [`OVERLAY_TITLE_SLOT`] 换成 `TextSlot::Body`（或
// [`OVERLAY_ELAPSED_SLOT`] 换成 `TextSlot::PhasePower`）⇒ 对象尺寸随槽同变 ⇒
// `ui/tests.rs::shell_chain` 的"大字对象高 == `PhasePower.px()`"当场红。
/// 中央大字的**字号槽**（UI §8.3 EDGE-03 原文「中央 **64 px**」= `TextSlot::PhasePower`）。
const OVERLAY_TITLE_SLOT: TextSlot = TextSlot::PhasePower;
/// 中央大字的**字色**（UI §8.3 EDGE-03 原文 `#FFB020` = `Palette::STALE`）。
///
/// ⚠️ **本值在对象层读不回来**（薄层无 `text_color` getter）⇒ 换色**抓不到**，
/// 属已登记的残余；换成 [`OVERLAY_TITLE_SLOT`] 那种"尺寸可断言"的档才有网。
const OVERLAY_TITLE_COLOR: Color = Palette::STALE;
/// 时长行的**字号槽**（UI §8.3 EDGE-03 原文「下方 **24 px**」= `TextSlot::Body`）。
const OVERLAY_ELAPSED_SLOT: TextSlot = TextSlot::Body;
/// 时长行的**字色**（同 [`OVERLAY_TITLE_COLOR`] 的残余说明：色在对象层不可判）。
const OVERLAY_ELAPSED_COLOR: Color = Palette::STANDBY;

/// 中央大字宽（逐字宽 = 字号；薄层无文本度量接口，与 `discard_w()` / `tab_text_w()` 同口径）。
fn overlay_title_w() -> i32 {
    OVERLAY_TEXT.chars().count() as i32 * OVERLAY_TITLE_SLOT.px() as i32
}

/// 中央大字 x（水平居中）。
fn overlay_title_x() -> i32 {
    theme::center_offset(Dimens::SCREEN_W, overlay_title_w())
}

/// 时长行宽（同上口径；随位数增长 ⇒ 由 [`Core::apply_overlay`] 在整秒变化时同步刷新）。
fn overlay_elapsed_w(text: &str) -> i32 {
    text.chars().count() as i32 * OVERLAY_ELAPSED_SLOT.px() as i32
}

/// 时长行 x（水平居中）。
fn overlay_elapsed_x(text: &str) -> i32 {
    theme::center_offset(Dimens::SCREEN_W, overlay_elapsed_w(text))
}

/// 整块高（64 + 跨区呼吸缝 24 + 24）—— **两档字号都取单一绑定的槽**。
const OVERLAY_BLOCK_H: i32 =
    OVERLAY_TITLE_SLOT.px() as i32 + Dimens::GAP_SECTION + OVERLAY_ELAPSED_SLOT.px() as i32;
/// 中央大字 y（整块在 768 高画布上垂直居中）。
const OVERLAY_TITLE_Y: i32 = theme::center_offset(Dimens::SCREEN_H, OVERLAY_BLOCK_H);
/// 时长行 y（大字下沿 + 一个跨区呼吸缝）。
const OVERLAY_ELAPSED_Y: i32 =
    OVERLAY_TITLE_Y + OVERLAY_TITLE_SLOT.px() as i32 + Dimens::GAP_SECTION;

/// 整屏降级遮罩的不透明度档位（**单一真源**）—— UI §8.3 EDGE-03「整屏遮罩压暗 **20 %**」。
///
/// **为什么要有这个常量**（B3-2b-1 规格评审 **重要 1+2**）：薄层**没有样式读回**
/// （无 `lv_obj_get_style_bg_opa` ⇒ 遮罩对象上读不回实际 `bg_opa`，见 `src/lvgl/mod.rs`
/// 的「薄层能力缺口登记」）⇒ 把 [`overlay_mask`] 里写的档位偷换成
/// [`Opacity::MASK_PERCENT`]（62 %，模态遮罩那一档）时，**全套用例全绿**（评审实测）。
/// 把档位提成常量后，"用了哪一档"至少是**可判定的单点**：`shell_chain` 断言
/// 本常量 == [`Opacity::DEGRADE_PERCENT`] 且 != [`Opacity::MASK_PERCENT`]。
///
/// **残余（如实登记）**：若有人**绕过本常量**、直接在 [`overlay_mask`] 的样式上写死别的
/// 档位，用例仍抓不到 —— 根因是薄层缺 `bg_opa` 读回（补读回属「薄层收口批」）。
///
/// **改什么会让本条变红**：把 `Opacity::DEGRADE_PERCENT` 换成 `Opacity::MASK_PERCENT`
/// ⇒ `shell_chain` 的两条档位断言（== 20 / != 62）同时红。
pub const OVERLAY_MASK_OPACITY: u8 = Opacity::DEGRADE_PERCENT;

// ═══════════════════════════════════════════════════════════════════════════
// 2. 上屏固定文案（UI §3.6「页眉 / 底部导航 / 超时」行的**唯一真源**）
// ═══════════════════════════════════════════════════════════════════════════

/// 页眉返回键文案（UI §3.6 页眉行）。
pub const TEXT_BACK: &str = "返回";
/// 未保存提示条主文案（UI §3.6 超时行 / §4.3）。
pub const TEXT_DIRTY_BANNER: &str = "有未保存修改";
/// 未保存提示条右侧文字按钮（UI §3.6 超时行 / §4.3）。
pub const TEXT_DISCARD: &str = "放弃修改";
/// 未保存提示条图标（UI §4.3「图标 `⚠`」）。
pub const ICON_WARN: &str = "⚠";
/// 通道正常时页眉胶囊（UI §3.6 页眉「通道 / 新鲜度」行）。
pub const TEXT_CHANNEL_OK: &str = "通道已连接";
/// 触摸不可用角标（UI §7.5 / EDGE-13）。
pub const TEXT_TOUCH_UNAVAILABLE: &str = "触摸不可用";

// ═══════════════════════════════════════════════════════════════════════════
// 3. 纯逻辑：导航目标（**不触碰 LVGL** ⇒ 可独立 `#[test]`）
// ═══════════════════════════════════════════════════════════════════════════

/// 六个导航目标（页签索引 ↔ 页面的**单一对应表**）。
///
/// **为什么用枚举而不是裸 `usize`**：路由、页签选中态、页眉标题、返回键可见性、超时回归目标
/// 五处都要"由页得索引 / 由索引得页"，裸整数会让五处各写一份映射（本仓已出过"第二份真源"类
/// 缺陷）。此处**只有这一份**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavPage {
    /// P1 主状态页（默认页 / 超时回归目标页）。
    Main,
    /// P2 配置页。
    Config,
    /// P3 日志页。
    Logs,
    /// P4 安全 / 联锁页。
    Interlock,
    /// P5 审计页。
    Audit,
    /// P6 系统 / 关于页。
    System,
}

impl NavPage {
    /// 全部页（**页签从左到右的顺序**，UI §4.2）。
    pub const ALL: [NavPage; 6] = [
        NavPage::Main,
        NavPage::Config,
        NavPage::Logs,
        NavPage::Interlock,
        NavPage::Audit,
        NavPage::System,
    ];

    /// 页签索引 → 页面（越界 ⇒ `None`，**不 panic**）。
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(NavPage::Main),
            1 => Some(NavPage::Config),
            2 => Some(NavPage::Logs),
            3 => Some(NavPage::Interlock),
            4 => Some(NavPage::Audit),
            5 => Some(NavPage::System),
            _ => None,
        }
    }

    /// 页面 → 页签索引（**与 [`NavPage::from_index`] 互逆**，由用例逐条往返断言）。
    pub const fn index(self) -> usize {
        match self {
            NavPage::Main => 0,
            NavPage::Config => 1,
            NavPage::Logs => 2,
            NavPage::Interlock => 3,
            NavPage::Audit => 4,
            NavPage::System => 5,
        }
    }

    /// 底部导航页签文案（UI §3.6 底部导航行）。
    pub const fn nav_label(self) -> &'static str {
        match self {
            NavPage::Main => "主状态",
            NavPage::Config => "配置",
            NavPage::Logs => "日志",
            NavPage::Interlock => "安全联锁",
            NavPage::Audit => "审计",
            NavPage::System => "系统",
        }
    }

    /// 页签几何图标（**SH1**：cmap 内几何字形占位，语义由文字通道承担）。
    pub const fn nav_icon(self) -> &'static str {
        match self {
            NavPage::Main => "●",
            NavPage::Config => "■",
            NavPage::Logs => "▼",
            NavPage::Interlock => "⚠",
            NavPage::Audit => "✓",
            NavPage::System => "○",
        }
    }

    /// 页眉标题（UI §3.6 页眉行：`台区储能装置运行状态` / `运行参数配置` / `系统信息` …）。
    pub const fn title(self) -> &'static str {
        match self {
            NavPage::Main => "台区储能装置运行状态",
            NavPage::Config => "运行参数配置",
            NavPage::Logs => "日志",
            NavPage::Interlock => "安全联锁",
            NavPage::Audit => "审计",
            NavPage::System => "系统信息",
        }
    }

    /// 页眉 `返回` 键是否可见（UI §4.1：**仅 P2–P6 显示，P1 不显示**）。
    pub const fn shows_back(self) -> bool {
        !matches!(self, NavPage::Main)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 纯逻辑：空闲超时（**时钟一律注入** ⇒ 离屏可确定性复现）
// ═══════════════════════════════════════════════════════════════════════════

/// 距超时还剩多少秒（**向上取整**；已到期 ⇒ `0`）。
///
/// 为什么向上取整：UI §4.3 要求文案「每秒递减」且判据是「超时前 10 s」—— `60 s` 的整拍上要
/// 恰好显示 `10 秒后返回主状态页`。向下取整会在 `t = 50 s` 显示 10、`t = 50.999 s` 仍是 10，
/// 到 `t = 51 s` 才跳到 9 ⇒ 实际是"每 1 s 递减"但起点错半拍；向上取整使**每一秒区间**
/// `(50, 51]` 都映射到同一个整数，与"每秒递减"逐拍一致。
pub fn remaining_secs(now: Instant, last_activity: Instant, timeout: Duration) -> u64 {
    let rem = last_activity
        .checked_add(timeout)
        .map(|deadline| deadline.saturating_duration_since(now))
        .unwrap_or_default();
    (rem.as_millis() as u64).div_ceil(1000)
}

/// 倒计时胶囊文案（UI §3.6 超时行 `秒后返回主状态页`）。
pub fn countdown_text(secs: u64) -> String {
    format!("{secs} 秒后返回主状态页")
}

/// 是否应显示倒计时胶囊（UI §4.3：「超时前 10 s」出现）。
pub const fn shows_countdown(secs: u64) -> bool {
    secs > 0 && secs <= COUNTDOWN_WINDOW_SECS
}

/// 是否应切回主状态页（UI §4.3：「到达 0 s 自动切 P1」）。
pub const fn should_return_home(secs: u64) -> bool {
    secs == 0
}

// ═══════════════════════════════════════════════════════════════════════════
// 4′ 纯逻辑：整屏降级（EDGE-03）的时长文案（**不触碰 LVGL** ⇒ 可独立 `#[test]`）
// ═══════════════════════════════════════════════════════════════════════════

/// 已断开时长文案（`N 秒`）—— **偏差 SH15**：§8.3 原文写"秒级恢复倒计时"，本实现为
/// "秒级递增的已断开时长"（恢复时刻不可预知 ⇒ 递减没有分母）。
///
/// **为什么不算"新增上屏串"**：本函数只是把**既有**的 `秒`（§3.6 用字表，用法见
/// `countdown_text` 与 P4 的「保持时间不足，还需 12 秒」）与**数字**拼起来，
/// **没有引入任何新字**（码表网 `ui_texts_covered_by_font_cmap` 逐字核对；运行时的
/// 数字字形由 `runtime_formatters_emit_only_cmap_glyphs` 走查）。
///
/// **改什么会让本条变红**：把模板改成 `{secs} 秒后恢复`（`后` / `恢` / `复` 虽在 cmap 内，
/// 但语义变成"倒计时中"= **谎报**）、或改成半角分隔（ASCII `-` / `,` 在生成字体里没字形）。
pub fn disconnect_elapsed_text(secs: u64) -> String {
    format!("{secs}{OVERLAY_ELAPSED_SUFFIX}")
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 纯逻辑：页眉通道状态（UI §4.1 / §8.3 EDGE-20）
// ═══════════════════════════════════════════════════════════════════════════

/// 页眉通道胶囊的三态（由注入的 [`ChannelStatus`] 派生，**本层不自行判断**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderChannel {
    /// 读通道通（`●` 绿）。
    Connected,
    /// 尚未首连（`●` 琥珀）。
    Connecting,
    /// 读通道断（`!` 红）——EDGE-20「控制通道可达但读通道断」的**红**通道。
    Down,
}

impl HeaderChannel {
    /// 三态（**数组下标 = [`HeaderChannel::index`]**，装配时逐件建胶囊）。
    pub const ALL: [HeaderChannel; 3] = [
        HeaderChannel::Connected,
        HeaderChannel::Connecting,
        HeaderChannel::Down,
    ];

    /// 由状态层的通道态派生。
    pub const fn from_status(status: ChannelStatus) -> Self {
        match status {
            ChannelStatus::Connected => HeaderChannel::Connected,
            ChannelStatus::Init => HeaderChannel::Connecting,
            ChannelStatus::Down => HeaderChannel::Down,
        }
    }

    /// 三个胶囊在 [`Core::chips`] 数组里的下标（**唯一对应表**）。
    pub const fn index(self) -> usize {
        match self {
            HeaderChannel::Connected => 0,
            HeaderChannel::Connecting => 1,
            HeaderChannel::Down => 2,
        }
    }

    /// 胶囊文案。
    pub const fn text(self) -> &'static str {
        match self {
            HeaderChannel::Connected => TEXT_CHANNEL_OK,
            HeaderChannel::Connecting => p1_status::TEXT_CHANNEL_CONNECTING,
            HeaderChannel::Down => p1_status::TEXT_CHANNEL_DOWN,
        }
    }

    /// 胶囊图标（F14 三通道之一）。
    pub const fn icon(self) -> &'static str {
        match self {
            HeaderChannel::Connected | HeaderChannel::Connecting => "●",
            HeaderChannel::Down => "!",
        }
    }

    /// 胶囊皮肤（颜色通道）。
    pub const fn skin(self) -> ChipSkin {
        match self {
            HeaderChannel::Connected => ChipSkin::SUCCESS,
            HeaderChannel::Connecting => ChipSkin::NEUTRAL,
            HeaderChannel::Down => ChipSkin::FAILURE,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. 外壳内部类型
// ═══════════════════════════════════════════════════════════════════════════

/// 导航页签**某一档底色的应用标记**（**① 回归锁的读回值**）。
///
/// 薄层**没有**"已挂样式读回"通道（`Style` 一旦 `Rc` 共享即冻结，且 `Obj` 不暴露
/// `lv_obj_get_style_*`）⇒ 本层把**送进 `Style::set_bg_color(..)` 的那个值**记下来。
/// 记法与 `pages/p5_audit.rs::immutable_bg`（**AU13**）同款：**唯一真源常量**
/// （[`NAV_BG_DEFAULT`] / [`NAV_BG_SELECTED`] / [`NAV_BG_PRESSED`]）同时喂样式与标记 ⇒
/// 改底色常量（哪怕只改成另一个 `Palette` 档）标记**必然随之变**，用例当场红。
///
/// **如实标注**：它不是从 LVGL 读回的对象实际底色，是"本层挂了哪一档"的可读回记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabBg {
    /// 透明底（未选中：露出导航条的 `Palette::SURFACE`）。
    Transparent,
    /// 命名色常量的实心底（色值一律来自 `theme::Palette`，本层零裸色值）。
    Solid(Color),
}

/// 导航页签的**三档视觉态**（**下标 = [`NavTab::bg_marks`] 的槽位**，也是挂样式时的顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabState {
    /// 未选中（LVGL `DEFAULT` 态）：底**透明**（UI §4.2「未选中：底色透明」）。
    Default,
    /// 选中（`LV_STATE_CHECKED`）：底 `surface_alt`（UI §4.2「选中：底 `#1B2942`」）。
    Selected,
    /// 按下（`LV_STATE_PRESSED`）：底 `#2E4066`（UI §4.2「按下 底 `#2E4066`」/ §5.2 `NavTab` 行）。
    Pressed,
}

impl TabState {
    /// 三档（**数组下标 = 样式槽位**）。
    pub const ALL: [TabState; 3] = [TabState::Default, TabState::Selected, TabState::Pressed];

    /// 槽位下标（`bg_styles` / `bg_marks` 的口径）。
    pub const fn slot(self) -> usize {
        match self {
            TabState::Default => 0,
            TabState::Selected => 1,
            TabState::Pressed => 2,
        }
    }

    /// 本档底色的**唯一真源常量**（[`NAV_BG_DEFAULT`] / [`NAV_BG_SELECTED`] /
    /// [`NAV_BG_PRESSED`]）。
    pub const fn bg(self) -> TabBg {
        match self {
            TabState::Default => NAV_BG_DEFAULT,
            TabState::Selected => NAV_BG_SELECTED,
            TabState::Pressed => NAV_BG_PRESSED,
        }
    }
}

/// 导航项**未选中**档底色的唯一真源（透明 —— 露出导航条底色，UI §4.2）。
///
/// **改什么会让本条变红**：把它改成 `TabBg::Solid(..)` ⇒ `shell_chain` 的
/// 「未选中页签 == 透明」与「未选中 ≠ 选中那一档」两条同时红。
const NAV_BG_DEFAULT: TabBg = TabBg::Transparent;
/// 导航项**选中**档底色的唯一真源（UI §4.2「选中：底 `#1B2942`」= `Palette::SURFACE_ALT`）。
///
/// **改什么会让本条变红**：把 `Palette::SURFACE_ALT` 换成**另一个** `Palette` 常量
/// （如 `Palette::SURFACE_HIGH`）⇒ `shell_chain` 的
/// `tab_bg(.., TabState::Selected) == Some(TabBg::Solid(Palette::SURFACE_ALT))` 当场红
/// —— 这正是 ① 评审探针（底色无回归锁）的封堵点。
const NAV_BG_SELECTED: TabBg = TabBg::Solid(Palette::SURFACE_ALT);
/// 导航项**按下**档底色的唯一真源（UI §4.2 / §5.2：底 `#2E4066` = `Palette::SURFACE_PRESS`）。
const NAV_BG_PRESSED: TabBg = TabBg::Solid(Palette::SURFACE_PRESS);

/// 一个底部导航页签（UI §4.2 / §5.1 #1）。
///
/// **句柄全部存进结构体**（R1）：页签是"拥有型 LVGL 句柄"的密集处 —— 顶部选中条 / 分隔线 /
/// 图标 / 文案四个子对象若只在构造器里当局部变量，`Drop` 会级联删掉它们（屏上只剩空按钮而
/// 无任何报错）。故四个字段**都是字段**，并各配一个读回口供离屏断言。
struct NavTab {
    /// 页签按钮（`CHECKABLE`；选中态由 `LV_STATE_CHECKED` 表达）。
    btn: TextButton,
    /// 顶部 4 px 选中条（未选中时 `HIDDEN`）。
    bar: Obj,
    /// 左缘 1 px 竖分隔（第 1 项无；`None`）。
    divider: Option<Obj>,
    /// 几何图标（28 px）。
    icon: Rc<Label>,
    /// 页签文案（26 px）。
    text: Rc<Label>,
    /// 图标的两档样式（0 = 未选中 / 1 = 选中）—— 选中态**双通道**的"文字色"通道之一。
    icon_styles: [Rc<Style>; 2],
    /// 文案的两档样式。
    text_styles: [Rc<Style>; 2],
    /// 图标两档**色值**（与 `icon_styles` 逐项对应）。
    ///
    /// **应用标记**（与 `p5_audit.rs` 的 `immutable_skin` 同法）：薄层没有"已挂样式读回"通道
    /// ⇒ 本层把**送进 `theme::text(..)` 的那个色值**记下来，供离屏断言核对"选中态第二通道
    /// 真的换了色"。把 `icon_styles` 的两个色值对调 ⇒ [`Shell::tab_icon_color`] 随之变化 ⇒ 用例红。
    icon_colors: [Color; 2],
    /// 文案两档**色值**（同上）。
    text_colors: [Color; 2],
    /// 当前挂着的图标样式下标（`usize::MAX` = 尚未挂过）。
    icon_idx: Cell<usize>,
    /// 当前挂着的文案样式下标。
    text_idx: Cell<usize>,
    /// 三档**底色**的应用标记（下标 = [`TabState::slot`]）—— **① 底色回归锁的读回值**。
    ///
    /// 底色样式是**状态选择器**（`DEFAULT` / `CHECKED` / `PRESSED` 三条）一次性挂上的，
    /// 运行期不再切换 ⇒ 标记按**槽位**记录（不是"当前生效档"，与 [`NavTab::text_colors`]
    /// 的 `text_idx` 口径不同，见 [`Shell::tab_bg`] 的文档）。
    bg_marks: [TabBg; 3],
}

/// 外壳的全部拥有型状态 + LVGL 句柄。
///
/// **`Rc` 不成环**：本类型持 6 页（单向允许）；页**不**持本类型；LVGL 回调只持
/// `Weak<Core>`（见 [`Shell::wire`]）⇒ `drop(Shell)` 必然级联删除整棵子树。
struct Core {
    /// 外壳根（全屏；`CLICKABLE` ⇒ 「全屏输入对象」）。
    root: Obj,
    /// 页眉容器。
    header: Obj,
    /// 返回键（仅 P2–P6 可见）。
    back: TextButton,
    /// 页标题。
    title: Rc<Label>,
    /// 时钟文本（**注入**）。
    clock: Rc<Label>,
    /// 通道胶囊三件（下标见 [`HeaderChannel::index`]）。
    chips: [StatusChip; 3],
    /// 倒计时胶囊（底色 + 描边 + 文案）。
    capsule: Obj,
    /// 倒计时胶囊文案。
    capsule_text: Rc<Label>,
    /// 触摸不可用角标（EDGE-13）。
    badge: Rc<Label>,
    /// 内容区容器（6 个页根的宿主）。
    content: Obj,
    /// 页根的直接宿主（`CONTENT_W × CONTENT_H`，`content` 内左移 `SIDE_PAD`）。
    ///
    /// ⚠️ **必须是字段**：写成 `Shell::new` 里的局部变量 ⇒ 构造器返回时 `Drop` 它会**级联删除
    /// 6 个页根**（屏上六页全空，而任何"构造没报错"的断言都不会发现）。这正是本仓复发了三次的
    /// `R1` 缺陷类；`shell_chain` 的"页根须存活"断言就是为它设的。
    page_host: Obj,
    /// 未保存修改提示条（EDGE-11）。
    banner: Obj,
    /// 提示条图标。
    banner_icon: Rc<Label>,
    /// 提示条文案。
    banner_text: Rc<Label>,
    /// 提示条右侧「放弃修改」文字按钮。
    discard: TextButton,
    /// 底部导航容器。
    nav: Obj,
    /// 6 个页签。
    tabs: [NavTab; 6],
    // ── 整屏降级层（EDGE-03，B3-2b-1）：**root 的末子 ⇒ 画在最上层**；常驻、只切可见性 ──
    /// 整屏遮罩（1024×768，压暗 20 %，**不带 `CLICKABLE` ⇒ 可穿透输入**）。
    overlay: Obj,
    /// 中央 64 px 大字（`与主进程数据通道断开`，`#FFB020`）。
    overlay_title: Rc<Label>,
    /// 下方 24 px 时长行（`N 秒`）。
    overlay_elapsed: Rc<Label>,
    // ── 6 页（外壳**持**页；页不持外壳）──
    p1: p1_status::P1StatusPage,
    p2: p2_config::P2ConfigPage,
    p3: p3_logs::P3LogsPage,
    p4: p4_interlock::P4InterlockPage,
    p5: p5_audit::P5AuditPage,
    p6: p6_system::P6SystemPage,
    // ── 状态（全部 `Cell` / `RefCell`，回调经 `Weak` 写）──
    /// 当前页签下标。
    current: Cell<usize>,
    /// 最近一次"用户活动"时刻（**注入时钟**）。
    /// `None` = 尚未收到任何 `tick` ⇒ 构造期**不读** `Instant::now()`（首拍由 [`Shell::tick`] 落定）。
    last_activity: RefCell<Option<Instant>>,
    /// 空闲超时时长（`--idle-timeout-secs`；默认 [`theme::Timing::IDLE_TIMEOUT_SECS`]）。
    timeout: Cell<Duration>,
    /// 是否有 LVGL 事件（`PRESSED`）自上次 [`Shell::tick`] 以来到达 —— **待消费**。
    pending_activity: Cell<bool>,
    /// 是否有确认弹层打开（**注入**：页侧无生产可见查询口，见 **SH2**）。
    modal_open: Cell<bool>,
    /// 触摸设备是否可用（EDGE-13）。
    touch_available: Cell<bool>,
    /// 页眉通道态。
    channel: Cell<HeaderChannel>,
    /// 倒计时胶囊当前是否可见（**应用标记**：薄层没有"已挂样式读回"通道 ⇒ 记下本层送出的状态）。
    capsule_on: Cell<bool>,
    /// 未保存提示条当前是否可见（同上）。
    banner_on: Cell<bool>,
    /// 整屏降级层当前是否可见（**应用标记**，同 [`Core::banner_on`] 口径）。
    overlay_on: Cell<bool>,
    /// 通道断的**起始时刻**（**注入时钟**；`None` = 当前未断）—— EDGE-03 时长行的分母起点。
    /// 用 `RefCell`（不是 `Cell`）：`Instant` 非 `Copy` 且此处"读改"要在一个借用里完成。
    overlay_since: RefCell<Option<Instant>>,
    /// 已上屏的**整秒数**（`None` = 尚未上屏）—— 判据「**只在整秒变化时**改文本」的读回值。
    overlay_secs: Cell<Option<u64>>,
    /// 时长行文本的**实际写入次数**（判据「每拍刷 LVGL 文本」= 红线行为：`ui/shell.rs`
    /// 的不变量 2 只允许 `set_text` 等轻量属性写，但**每拍写同一条文本**是纯浪费 +
    /// 让"整秒才更新"这一要求无从验证 ⇒ 用计数器把它变成可断言的事实）。
    overlay_writes: Cell<u64>,
    /// 切页通知槽（**契约**：回调内不得再调 [`Shell::show`]；见 [`Shell::set_on_page_change`]）。
    on_page_change: CbSlot<NavPage>,
}

/// 应用外壳（页眉 + 内容区 + 底部导航 + 顶层提示条）。
pub struct Shell {
    /// **`Rc` 是必需的**：LVGL 回调要经 `Weak` 回指（否则 `Core` 无法在回调里被访问）。
    core: Rc<Core>,
}

impl Shell {
    /// 装配外壳：建根 / 页眉 / 内容区（**含 6 页**）/ 提示条 / 底部导航，并挂钩回调。
    ///
    /// `parent` 通常是活动屏（`Obj::screen()`），但也可以是任意容器（测试用宿主容器）。
    /// 外壳根是 `parent` 的**子对象**、尺寸 `SCREEN_W × SCREEN_H`、自身 `(0, 0)`。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = Obj::create(parent)?;
        root.set_size(Dimens::SCREEN_W, Dimens::SCREEN_H);
        root.set_pos(0, 0);
        root.add_style(&theme::screen_bg(), StyleSelector::main());
        root.remove_flag(ObjFlag::SCROLLABLE);

        let header = layout_box(&root, Dimens::SCREEN_W, Dimens::HEADER_H)?;
        header.set_pos(0, 0);
        header.add_style(&header_bg(), StyleSelector::main());

        // ── 返回键（64×64，仅 P2–P6 可见）──
        let back = TextButton::create(&header, TEXT_BACK)?;
        back.set_size(Dimens::TOUCH_CRITICAL, Dimens::TOUCH_CRITICAL);
        back.set_pos(HEADER_BACK_X, HEADER_BACK_Y);
        theme::button(ButtonKind::Secondary).apply(&back);

        // ── 标题（32 px；位置 / 宽度随页切换）──
        let title = Rc::new(label(&header, TextSlot::PageTitle, Palette::TEXT_PRIMARY)?);
        title.set_long_mode(LongMode::DOTS);

        // ── 时钟（26 px，注入文本）──
        let clock = Rc::new(label(&header, TextSlot::Label, Palette::TEXT_SECOND)?);
        clock.set_size(HEADER_CLOCK_W, TextSlot::Label.px() as i32);
        clock.set_pos(HEADER_CLOCK_X, HEADER_CLOCK_Y);
        clock.set_long_mode(LongMode::CLIP);

        // ── 通道胶囊 ×3（三态各一件，**只显其一**）──
        let mut chips = Vec::with_capacity(HeaderChannel::ALL.len());
        for ch in HeaderChannel::ALL {
            let chip = StatusChip::new(&header, HEADER_CHIP_W, ch.icon(), ch.text(), ch.skin())?;
            chip.set_pos(HEADER_CHIP_X, HEADER_CHIP_Y);
            set_visible(chip.obj(), false);
            chips.push(chip);
        }
        let chips: [StatusChip; 3] = chips
            .try_into()
            .map_err(|_| LvglError::InvalidArgument("通道胶囊三态"))?;

        // ── 倒计时胶囊（底 `#3A2E12` 描边 `#FFB020`，文案 24 px `#FFD75E`）──
        let capsule = layout_box(&header, HEADER_CAPSULE_W, Dimens::STATUS_CHIP_H)?;
        capsule.set_pos(HEADER_CAPSULE_X, HEADER_CHIP_Y);
        capsule.add_style(&capsule_skin(), StyleSelector::main());
        let capsule_text = Rc::new(label(&capsule, TextSlot::Body, Palette::STANDBY)?);
        capsule_text.set_size(
            HEADER_CAPSULE_W - Dimens::GAP_MIN,
            TextSlot::Body.px() as i32,
        );
        capsule_text.set_pos(
            Dimens::GAP_MIN / 2,
            theme::center_offset(Dimens::STATUS_CHIP_H, TextSlot::Body.px() as i32),
        );
        capsule_text.set_long_mode(LongMode::CLIP);
        set_visible(&capsule, false);

        // ── 触摸不可用角标（EDGE-13；24 px `#FFB020`）──
        let badge = Rc::new(label(&header, TextSlot::Body, Palette::STALE)?);
        badge.set_text(TEXT_TOUCH_UNAVAILABLE);
        badge.set_size(HEADER_BADGE_W, TextSlot::Body.px() as i32);
        badge.set_pos(HEADER_BADGE_X, HEADER_BADGE_Y);
        badge.set_long_mode(LongMode::CLIP);
        set_visible(&badge, false);

        // ── 内容区（6 页的宿主；页根由外壳摆放在 `(SIDE_PAD, CONTENT_Y)`）──
        let content = layout_box(&root, Dimens::SCREEN_W, Dimens::CONTENT_H)?;
        content.set_pos(0, CONTENT_Y);
        let page_host = layout_box(&content, Dimens::CONTENT_W, Dimens::CONTENT_H)?;
        page_host.set_pos(Dimens::SIDE_PAD, 0);

        let p1 = p1_status::P1StatusPage::new(&page_host)?;
        let p2 = p2_config::P2ConfigPage::new(&page_host)?;
        let p3 = p3_logs::P3LogsPage::new(&page_host)?;
        let p4 = p4_interlock::P4InterlockPage::new(&page_host)?;
        let p5 = p5_audit::P5AuditPage::new(&page_host)?;
        let p6 = p6_system::P6SystemPage::new(&page_host)?;

        // ── 未保存修改提示条（EDGE-11；`h48`）──
        let banner = layout_box(&root, Dimens::CONTENT_W, BANNER_H)?;
        banner.set_pos(Dimens::SIDE_PAD, CONTENT_Y);
        banner.add_style(&banner_skin(), StyleSelector::main());
        let banner_icon = Rc::new(label(&banner, TextSlot::SectionTitle, Palette::STALE)?);
        banner_icon.set_text(ICON_WARN);
        banner_icon.set_size(Dimens::ICON_SM, Dimens::ICON_SM);
        banner_icon.set_pos(BANNER_ICON_X, BANNER_ICON_Y);
        let banner_text = Rc::new(label(&banner, TextSlot::Body, Palette::STANDBY)?);
        banner_text.set_text(TEXT_DIRTY_BANNER);
        banner_text.set_size(
            Dimens::CONTENT_W - BANNER_TEXT_X - discard_w() - Dimens::GAP_MIN,
            TextSlot::Body.px() as i32,
        );
        banner_text.set_pos(BANNER_TEXT_X, BANNER_TEXT_Y);
        banner_text.set_long_mode(LongMode::CLIP);
        let discard = TextButton::create(&banner, TEXT_DISCARD)?;
        // 触区高 = 提示条高（48 = `Dimens::TOUCH_MIN`；UI §4.3 的「64 高触区」与同一行的
        // 「条高 h48」冲突，取条高 —— 见偏差 **SH4**）。
        discard.set_size(discard_w(), BANNER_H);
        discard.set_pos(Dimens::CONTENT_W - discard_w() - Dimens::GAP_MIN, 0);
        theme::button(ButtonKind::Text).apply(&discard);
        set_visible(&banner, false);

        // ── 底部导航（6 项常驻，每项 `NAV_ITEM_W × NAV_ITEM_H`）──
        let nav = layout_box(&root, Dimens::SCREEN_W, Dimens::NAV_H)?;
        nav.set_pos(0, Dimens::HEADER_H + Dimens::CONTENT_H);
        nav.add_style(&nav_bg(), StyleSelector::main());
        let item_styles = [
            nav_item_skin(NAV_BG_DEFAULT),
            nav_item_skin(NAV_BG_SELECTED),
            nav_item_skin(NAV_BG_PRESSED),
        ];
        let mut tabs = Vec::with_capacity(NavPage::ALL.len());
        for page in NavPage::ALL {
            tabs.push(build_tab(&nav, page, &item_styles)?);
        }
        let tabs: [NavTab; 6] = tabs
            .try_into()
            .map_err(|_| LvglError::InvalidArgument("导航六项"))?;

        // ── 整屏降级层（EDGE-03）──
        // **建一次、只切可见性**（裁定 ①）：在装配期建好、`HIDDEN`；运行期只 `set_hidden` /
        // `set_text`。它是 `root` 的**末子** ⇒ LVGL 按创建序绘制 ⇒ 画在页眉 / 内容区 / 提示条 /
        // 导航**之上**（弹层与 Toast 另挂 `lv_layer_top()`，仍在它之上 ⇒ EDGE-20 的写路径可用）。
        // **可穿透输入**（裁定 ②）：`lv_obj` 构造时默认带 `CLICKABLE`
        // （`vendor/lvgl/src/core/lv_obj.c:584`），而 `pages::layout_box` 只摘 `SCROLLABLE`
        // ⇒ 此处必须**显式**摘掉，否则整屏遮罩会吃掉所有触摸、EDGE-20 不成立。
        // **对照**：`components.rs::ConfirmDialog::new` 的遮罩反过来 `add_flag(CLICKABLE)`
        // （模态弹层要求"点外面不落到页面上"）—— 两者语义相反，`shell_chain` 各有一条断言。
        let overlay = layout_box(&root, Dimens::SCREEN_W, Dimens::SCREEN_H)?;
        overlay.set_pos(0, 0);
        overlay.add_style(&overlay_mask(), StyleSelector::main());
        overlay.remove_flag(ObjFlag::CLICKABLE);
        // **槽 + 色 = 单一绑定**（重要 1+2）：构造与 `set_size` **同源**，见
        // [`OVERLAY_TITLE_SLOT`] 上方说明 —— 两处各写各的常量时"换错槽"抓不到。
        let overlay_title = Rc::new(label(&overlay, OVERLAY_TITLE_SLOT, OVERLAY_TITLE_COLOR)?);
        overlay_title.set_text(OVERLAY_TEXT);
        overlay_title.set_size(overlay_title_w(), OVERLAY_TITLE_SLOT.px() as i32);
        overlay_title.set_pos(overlay_title_x(), OVERLAY_TITLE_Y);
        overlay_title.set_long_mode(LongMode::CLIP);
        let overlay_elapsed = Rc::new(label(&overlay, OVERLAY_ELAPSED_SLOT, OVERLAY_ELAPSED_COLOR)?);
        overlay_elapsed.set_text(&disconnect_elapsed_text(0));
        overlay_elapsed.set_size(
            overlay_elapsed_w(&disconnect_elapsed_text(0)),
            OVERLAY_ELAPSED_SLOT.px() as i32,
        );
        overlay_elapsed.set_pos(
            overlay_elapsed_x(&disconnect_elapsed_text(0)),
            OVERLAY_ELAPSED_Y,
        );
        overlay_elapsed.set_long_mode(LongMode::CLIP);
        set_visible(&overlay, false);

        let core = Rc::new(Core {
            root,
            header,
            back,
            title,
            clock,
            chips,
            capsule,
            capsule_text,
            badge,
            content,
            page_host,
            banner,
            banner_icon,
            banner_text,
            discard,
            nav,
            tabs,
            overlay,
            overlay_title,
            overlay_elapsed,
            p1,
            p2,
            p3,
            p4,
            p5,
            p6,
            current: Cell::new(NavPage::Main.index()),
            last_activity: RefCell::new(None),
            timeout: Cell::new(Duration::from_secs(theme::Timing::IDLE_TIMEOUT_SECS)),
            pending_activity: Cell::new(false),
            modal_open: Cell::new(false),
            touch_available: Cell::new(true),
            channel: Cell::new(HeaderChannel::Connected),
            capsule_on: Cell::new(false),
            banner_on: Cell::new(false),
            overlay_on: Cell::new(false),
            overlay_since: RefCell::new(None),
            overlay_secs: Cell::new(None),
            overlay_writes: Cell::new(0),
            on_page_change: CbSlot::new(),
        });
        let shell = Shell { core };
        shell.wire();
        // 初始页 = P1（首次进入不触发切页通知 —— 尚未有订阅者）。
        shell.core.select(NavPage::Main, false);
        shell.core.apply_header_channel();
        Ok(shell)
    }

    /// 挂钩全部 LVGL 回调（**回调一律 `Weak<Core>` + `upgrade`** —— 写成 `Rc<Core>` 即
    /// `Core → 控件 → 回调 → Rc<Core>` 强引用环，`drop(Shell)` 不释放任何对象、
    /// 建/拆第 2 次即 OOM；本仓 P5 已复发过一次）。
    fn wire(&self) {
        let weak = Rc::downgrade(&self.core);
        // ①**全屏输入对象**：任何落在"非控件"区域的按压都命中外壳根（UI §4.3「计时重置」）。
        //    页内控件上的按压不会上冒（**SH5**：`ObjFlag` 未镜像 `EVENT_BUBBLE`）。
        self.core.root.on(EventCode::PRESSED, {
            let weak = weak.clone();
            move |_e| {
                if let Some(c) = weak.upgrade() {
                    c.pending_activity.set(true);
                }
            }
        });
        // ② 返回键 ⇒ 切 P1（UI §4.3「主动返回」）。
        self.core.back.on_clicked({
            let weak = weak.clone();
            move |_e| {
                if let Some(c) = weak.upgrade() {
                    c.pending_activity.set(true);
                    c.select(NavPage::Main, true);
                }
            }
        });
        // ③ 6 个页签 ⇒ 1 次触摸直达（UI §4.2）。
        for (i, tab) in self.core.tabs.iter().enumerate() {
            let weak = weak.clone();
            tab.btn.on_clicked(move |_e| {
                if let Some(c) = weak.upgrade() {
                    c.pending_activity.set(true);
                    if let Some(page) = NavPage::from_index(i) {
                        c.select(page, true);
                    }
                }
            });
        }
        // ④ 「放弃修改」⇒ 丢弃 P2 草稿（脏态清 ⇒ 下一拍恢复倒计时）。
        self.core.discard.on_clicked({
            let weak = weak.clone();
            move |_e| {
                if let Some(c) = weak.upgrade() {
                    c.pending_activity.set(true);
                    c.p2.discard_draft();
                    c.apply_dirty();
                }
            }
        });
    }

    // ── 路由 ──────────────────────────────────────────────────────────────

    /// 切到指定页（**1 次触摸**；不做切换动画，UI §7.5）。
    pub fn show(&self, page: NavPage) {
        self.core.select(page, true);
    }

    /// 当前页。
    pub fn current(&self) -> NavPage {
        NavPage::from_index(self.core.current.get()).unwrap_or(NavPage::Main)
    }

    /// 注册「切页」通知（**仅在页真正变化时**触发一次；初始页不触发）。
    ///
    /// **契约**：回调**不得**再调 [`Shell::show`] / [`Shell::tick`]（重入布局）；
    /// 回调内自替换的语义与 `ui/components.rs` 同款（本次由旧回调跑完、新回调自下次生效）。
    pub fn set_on_page_change<F>(&self, f: F)
    where
        F: FnMut(NavPage) + 'static,
    {
        self.core.on_page_change.set(f);
    }

    // ── B3 注入位 ─────────────────────────────────────────────────────────

    /// 数据注入位 ①：各页（**真实数据源接线属 B3**）。
    pub fn p1(&self) -> &p1_status::P1StatusPage {
        &self.core.p1
    }

    /// 数据注入位 ①：P2 配置页（控制通道驱动，见 `pages/mod.rs` 契约 2′）。
    pub fn p2(&self) -> &p2_config::P2ConfigPage {
        &self.core.p2
    }

    /// 数据注入位 ①：P3 日志页。
    pub fn p3(&self) -> &p3_logs::P3LogsPage {
        &self.core.p3
    }

    /// 数据注入位 ①：P4 安全 / 联锁页。
    pub fn p4(&self) -> &p4_interlock::P4InterlockPage {
        &self.core.p4
    }

    /// 数据注入位 ①：P5 审计页。
    pub fn p5(&self) -> &p5_audit::P5AuditPage {
        &self.core.p5
    }

    /// 数据注入位 ①：P6 系统 / 关于页。
    pub fn p6(&self) -> &p6_system::P6SystemPage {
        &self.core.p6
    }

    /// 数据注入位 ③：读通道态 ⇒ 页眉通道胶囊（EDGE-20 的"红 + 时钟同显"落点）。
    pub fn set_channel(&self, channel: ChannelStatus) {
        self.core.channel.set(HeaderChannel::from_status(channel));
        self.core.apply_header_channel();
    }

    /// 数据注入位 ④：触摸设备可用性（EDGE-13）。
    ///
    /// `false` ⇒ 页眉右端常驻 24 px `#FFB020`「触摸不可用」角标；**数据刷新不受影响**。
    pub fn set_touch_available(&self, available: bool) {
        self.core.touch_available.set(available);
        self.core.apply_header_channel();
    }

    /// 数据注入位 ⑤：空闲超时时长（`--idle-timeout-secs`；默认 60 s）。
    ///
    /// `0` ⇒ **禁用**空闲回归（CLI `--help` 的「0=禁用」；语义唯一落点在 [`Shell::tick`]，
    /// 见偏差 **SH17**）。
    pub fn set_idle_timeout(&self, secs: u64) {
        self.core.timeout.set(Duration::from_secs(secs));
    }

    /// 数据注入位 ⑥：是否有**确认弹层**打开（打开期间暂停计时且不显示倒计时，UI §4.3）。
    ///
    /// ⚠️ **页侧拿不到**（P2 / P4 的 `with_dialog` 是 `#[cfg(test)]`）⇒ 由 B3 从
    /// `UiState.confirm.is_some()` 注入 —— 见偏差 **SH2**。`false` 且 P2 不脏时恢复计时
    /// （从**满时长**重新起算）。
    pub fn set_modal_open(&self, open: bool) {
        self.core.modal_open.set(open);
    }

    /// 手工记一次"用户活动"（回调之外的入口：B3 若在别处收到输入事件可直接调用）。
    ///
    /// 效果与 LVGL `PRESSED` 一致：下一次 [`Shell::tick`] 把空闲计时重置为**那一刻**。
    pub fn note_activity(&self) {
        self.core.pending_activity.set(true);
    }

    // ── 每拍推进 ──────────────────────────────────────────────────────────

    /// 每个事件循环拍调一次：刷新时钟文本、转发 P2 / P4 的 `tick`、消费 `PendingActivity`、
    /// 推进空闲超时状态机（胶囊 / 回归主状态页）、同步未保存提示条。
    ///
    /// **时钟经 `now` / `clock_text` 注入** ⇒ 本外壳不读 `Instant::now()`，超时链路离屏可确定性复现。
    pub fn tick(&self, now: Instant, clock_text: &str) {
        let c = &self.core;
        c.clock.set_text(clock_text);
        // 页内延迟动作（弹层关闭 / Toast 过期）—— 时钟同为注入。
        c.p2.tick(now);
        c.p4.tick(now);

        // ① 消费按压（**消费时刻即"活动时刻"**：真实循环里 `lv_indev_read` 与 `app.tick`
        //    同拍，故误差 ≤ 一拍；离屏用例则完全确定）。首拍（`None`）在此落定 —— 构造期
        //    **不取时钟**（本文件对"时钟一律注入"的纪律是逐条的）。
        let mut last = c.last_activity.borrow_mut();
        if c.pending_activity.replace(false) || last.is_none() {
            *last = Some(now);
        }

        // ② 暂停判据：弹层打开 **或** P2 有未保存修改 ⇒ 不倒计时、不强制切页（UI §4.3）。
        //    实现 = 把"最后活动时刻"钉在当下（等价于"计时暂停"）；放开后从**满时长**重算。
        //
        //    ②′ `--idle-timeout-secs 0` = **禁用**空闲回归（CLI「0=禁用」；原 `timing::IdleTimer`
        //    的 `is_disabled()` 语义，该类型已按 B3-2b-1 任务书删除 ⇒ 语义唯一落点在此，
        //    见偏差 **SH17**）。口径与"暂停"相同：不显示倒计时、不强制切页。
        //    **改什么会让本条变红**：删掉 `disabled` 项 ⇒ `remaining_secs` 恒 0 ⇒
        //    `should_return_home(0)` 恒真 ⇒ 每拍强制回 P1（用户停不在 P2–P6）。
        let disabled = c.timeout.get().is_zero();
        let paused = disabled || c.modal_open.get() || c.p2.is_dirty();
        if paused {
            *last = Some(now);
        }

        let secs = remaining_secs(now, last.unwrap_or(now), c.timeout.get());
        if !paused && should_return_home(secs) {
            // 到 0 ⇒ 自动切 P1（**不产生审计记录**；UI §4.3）。
            *last = Some(now);
            drop(last); // 切页会读页句柄 / 写 LVGL，先放开借用（`select` 不碰 `last_activity`，此处仍求稳）。
            c.select(NavPage::Main, true);
        }
        let show_capsule = !paused && shows_countdown(secs);
        if show_capsule != c.capsule_on.get() {
            c.capsule_on.set(show_capsule);
            set_visible(&c.capsule, show_capsule);
        }
        if show_capsule {
            c.capsule_text.set_text(&countdown_text(secs));
        }

        // ③ 页眉右端：倒计时胶囊出现时通道胶囊与触摸角标让位（**SH3**）。
        c.apply_header_channel();

        // ③′ 整屏降级（EDGE-03）：**通道断 ⇒ 遮罩 + 中央文案 + 已断开时长**；
        //     通道恢复 ⇒ **本拍即撤**（≤1 s 回实时，见模块头「EDGE-03 整屏降级」）。
        //     时长用**注入**的 `now`（本文件不读 `Instant::now()`，离屏可确定性复现）。
        c.apply_overlay(now);

        // ④ 未保存提示条（EDGE-11）由 P2 的 `is_dirty()` 驱动。
        c.apply_dirty();
    }

    // ── 离屏断言口径（**只读**）───────────────────────────────────────────

    /// 外壳根对象（`drop` 它即整棵子树级联删除）。
    pub fn obj(&self) -> &Obj {
        &self.core.root
    }

    /// 页眉容器。
    pub fn header_obj(&self) -> &Obj {
        &self.core.header
    }

    /// 内容区容器（6 页的宿主）。
    pub fn content_obj(&self) -> &Obj {
        &self.core.content
    }

    /// 页根的直接宿主（`(SIDE_PAD, 0)`，尺寸 `CONTENT_W × CONTENT_H`）—— 6 个页根的父对象。
    ///
    /// **读回口存在的理由**（R1）：它是"拥有型句柄必须是字段"的**探针点** —— 写成局部变量时
    /// 构造器返回即级联删除 6 个页根（屏上全空、无报错）。
    pub fn page_host_obj(&self) -> &Obj {
        &self.core.page_host
    }

    /// 底部导航容器。
    pub fn nav_obj(&self) -> &Obj {
        &self.core.nav
    }

    /// 未保存提示条容器。
    pub fn banner_obj(&self) -> &Obj {
        &self.core.banner
    }

    /// 提示条右侧「放弃修改」按钮。
    pub fn discard_button(&self) -> &TextButton {
        &self.core.discard
    }

    /// 返回键（仅 P2–P6 可见）。
    pub fn back_button(&self) -> &TextButton {
        &self.core.back
    }

    /// 返回键当前是否可见（UI §4.1：P1 不显示）。
    pub fn back_visible(&self) -> bool {
        !self.core.back.is_hidden()
    }

    /// 页标题文本。
    pub fn title_text(&self) -> Option<String> {
        self.core.title.text()
    }

    /// 时钟文本（注入回读）。
    pub fn clock_text(&self) -> Option<String> {
        self.core.clock.text()
    }

    /// 页眉通道胶囊文本（**当前可见**的那一件；全部隐藏时 `None`）。
    pub fn channel_text(&self) -> Option<String> {
        let mut out = None;
        for chip in &self.core.chips {
            if !chip.obj().is_hidden() {
                out = chip.text();
            }
        }
        out
    }

    /// 页眉通道胶囊（**当前可见**那一件）的**屏内绝对左缘 x**（离屏断言口径）；
    /// 三件全隐藏时 `None`。
    ///
    /// **为什么要有它**（② 评审的"落到对象上"的那一半）：`shell.rs` 内
    /// [`tests::header_slots_are_disjoint_and_inside_canvas`] 断言的是**常量层**
    /// （`HEADER_CHIP_X` 与时钟的锚定链），抓不到"调用点把 `set_pos` 的实参换掉"；
    /// 本读口取 [`Obj::coords`]（**布局趟落定后**的实际屏内坐标）。
    ///
    /// ⚠️ **必须与 [`HEADER_CHIP_X`] 做精确相等断言**，不要写成"落在右半区"之类的区间判据：
    /// 区间判据有 **80 px 盲窗**（实测把调用点写成 512 时全绿 —— 见 [`HEADER_CHIP_X`] 的说明）。
    /// ⚠️ `coords` 须等一次布局趟（生产 = 每拍 `timer_handler()`；测试 = 强制渲染）。
    pub fn channel_chip_x(&self) -> Option<i32> {
        let mut out = None;
        for chip in &self.core.chips {
            if !chip.obj().is_hidden() {
                out = Some(chip.obj().coords().x1);
            }
        }
        out
    }

    /// 未保存提示条是否可见。
    pub fn banner_visible(&self) -> bool {
        !self.core.banner.is_hidden()
    }

    /// 提示条文案。
    pub fn banner_text(&self) -> Option<String> {
        self.core.banner_text.text()
    }

    /// 提示条图标字形（`⚠`）。
    pub fn banner_icon_text(&self) -> Option<String> {
        self.core.banner_icon.text()
    }

    /// 倒计时胶囊是否可见。
    pub fn countdown_visible(&self) -> bool {
        !self.core.capsule.is_hidden()
    }

    /// 倒计时胶囊文案。
    pub fn countdown_text_value(&self) -> Option<String> {
        self.core.capsule_text.text()
    }

    /// 整屏降级层（EDGE-03）的根对象。
    ///
    /// **读回口存在的理由**：① 它是"层建一次、常驻"的探针点（写成 `Shell::new` 的局部变量 ⇒
    /// 构造器返回即级联删除 ⇒ 遮罩永不可见）；② 它的旗标是"**可穿透输入**"（EDGE-20）的
    /// **唯一可断言落点**（`CLICKABLE` 必须**不在**；对照 `ConfirmDialog` 的遮罩**必须在**）。
    pub fn overlay_obj(&self) -> &Obj {
        &self.core.overlay
    }

    /// 整屏降级层是否可见（通道断 ⇒ `true`）。
    pub fn overlay_visible(&self) -> bool {
        !self.core.overlay.is_hidden()
    }

    /// 整屏降级中央 64 px 大字的**对象**（**对象级尺寸断言口径** —— 重要 1+2）。
    ///
    /// **读回口存在的理由**：薄层**没有**字号读回（无 `lv_obj_get_style_text_font`）⇒
    /// "建标签时用了哪个字号槽"在对象层不可直接问；但 `Obj::size()` **是**可读的，而
    /// 本层的 `set_size(.., <槽>.px())` 与"建标签的那个槽"**同源**（见 [`OVERLAY_TITLE_SLOT`]）
    /// ⇒ 断言 `size().1 == 槽档高` 即可抓住"换错槽"。⚠️ 这**不是**"字体读回"，
    /// 是"尺寸这个可读可见量由同一绑定派生"（残余：字色 / 真实字体仍不可判）。
    pub fn overlay_title_obj(&self) -> &Obj {
        self.core.overlay_title.obj()
    }

    /// 整屏降级中央 64 px 大字文案（`与主进程数据通道断开`）。
    pub fn overlay_title_text(&self) -> Option<String> {
        self.core.overlay_title.text()
    }

    /// 下方时长行的**对象**（**对象级尺寸断言口径**，同 [`Shell::overlay_title_obj`]）。
    pub fn overlay_elapsed_obj(&self) -> &Obj {
        self.core.overlay_elapsed.obj()
    }

    /// 下方时长行文案（`N 秒`）。
    pub fn overlay_elapsed_text(&self) -> Option<String> {
        self.core.overlay_elapsed.text()
    }

    /// 已上屏的**整秒数**（`None` = 通道未断 / 已恢复）。
    pub fn overlay_elapsed_secs(&self) -> Option<u64> {
        self.core.overlay_secs.get()
    }

    /// 时长行文本的**实际写入次数**（判据「只在整秒变化时改文本」—— 每拍刷文本 ⇒ 本值每拍 +1 ⇒ 红）。
    pub fn overlay_text_writes(&self) -> u64 {
        self.core.overlay_writes.get()
    }

    /// 触摸不可用角标是否可见（EDGE-13）。
    pub fn touch_badge_visible(&self) -> bool {
        !self.core.badge.is_hidden()
    }

    /// 触摸不可用角标文案。
    pub fn touch_badge_text(&self) -> Option<String> {
        self.core.badge.text()
    }

    /// 第 `i` 个页签（按 `NavPage::ALL` 顺序）。
    pub fn tab(&self, page: NavPage) -> Option<&TextButton> {
        self.core.tabs.get(page.index()).map(|t| &t.btn)
    }

    /// 第 `i` 个页签的顶部选中条是否可见（**选中态双通道之一**）。
    pub fn tab_bar_visible(&self, page: NavPage) -> Option<bool> {
        self.core
            .tabs
            .get(page.index())
            .map(|t| !t.bar.is_hidden())
    }

    /// 第 `i` 个页签的左缘竖分隔线（**第 1 项无 ⇒ `None`**；UI §4.2）。
    pub fn tab_divider(&self, page: NavPage) -> Option<&Obj> {
        self.core
            .tabs
            .get(page.index())
            .and_then(|t| t.divider.as_ref())
    }

    /// 第 `i` 个页签**当前生效**的文字色（= 选中态双通道的"文字"通道）。
    ///
    /// `None` = 尚未摆过选中态（应有且仅有装配前的一瞬）。取值口径 = `text_idx` 指向的
    /// **实际被送进 `theme::text(..)` 的色值**（应用标记，见 `NavTab::text_colors`）。
    pub fn tab_text_color(&self, page: NavPage) -> Option<Color> {
        let t = self.core.tabs.get(page.index())?;
        t.text_colors.get(t.text_idx.get()).copied()
    }

    /// 第 `i` 个页签**当前生效**的图标色（同 [`Shell::tab_text_color`] 的口径）。
    pub fn tab_icon_color(&self, page: NavPage) -> Option<Color> {
        let t = self.core.tabs.get(page.index())?;
        t.icon_colors.get(t.icon_idx.get()).copied()
    }

    /// 第 `i` 个页签在**指定档**（[`TabState`]）的**底色应用标记**（**① 的读回口**）。
    ///
    /// 与 [`Shell::tab_text_color`] 的差别（**有意**）：文字 / 图标色是"**当前生效**"档
    /// （运行期经 `set_style_index` 在 0/1 两档间切换），而底色是**状态选择器**（`DEFAULT` /
    /// `CHECKED` / `PRESSED`）一次挂齐、由 LVGL 按状态自选 ⇒ 这里按**槽位**读，问的是
    /// "这一档挂的是哪个底色"。口径 = [`NavTab::bg_marks`]（**应用标记**：记的是送进
    /// `set_bg_color(..)` 的那个值，不是从 LVGL 读回的实际底色 —— 薄层无该通道）。
    pub fn tab_bg(&self, page: NavPage, state: TabState) -> Option<TabBg> {
        let t = self.core.tabs.get(page.index())?;
        t.bg_marks.get(state.slot()).copied()
    }

    /// 第 `i` 个页签的图标字形。
    pub fn tab_icon_text(&self, page: NavPage) -> Option<String> {
        self.core
            .tabs
            .get(page.index())
            .and_then(|t| t.icon.text())
    }

    /// 第 `i` 个页签的文案。
    pub fn tab_text(&self, page: NavPage) -> Option<String> {
        self.core
            .tabs
            .get(page.index())
            .and_then(|t| t.text.text())
    }

    /// 6 页中当前可见的页根对象（**唯一**一个；用于"任一时刻只显一页"的离屏断言）。
    pub fn visible_pages(&self) -> Vec<NavPage> {
        let objs = self.core.page_objs();
        NavPage::ALL
            .into_iter()
            .filter(|p| !objs[p.index()].is_hidden())
            .collect()
    }

    /// 某页页根对象（装配断言口径）。
    pub fn page_obj(&self, page: NavPage) -> &Obj {
        self.core.page_objs()[page.index()]
    }
}

impl Core {
    /// 6 个页根（按 `NavPage::ALL` 顺序）—— **唯一**的"页 ↔ 下标"取值处。
    fn page_objs(&self) -> [&Obj; 6] {
        [
            self.p1.obj(),
            self.p2.obj(),
            self.p3.obj(),
            self.p4.obj(),
            self.p5.obj(),
            self.p6.obj(),
        ]
    }

    /// 切页：显隐页根 + 页签选中态（双通道）+ 页眉返回键 / 标题。
    ///
    /// `notify` 由调用方给：装配期的初始摆放**不**通知（那时还没有订阅者）。
    fn select(&self, page: NavPage, notify: bool) {
        let idx = page.index();
        let changed = self.current.get() != idx;
        self.current.set(idx);

        for (i, obj) in self.page_objs().iter().enumerate() {
            set_visible(obj, i == idx);
        }
        for (i, tab) in self.tabs.iter().enumerate() {
            let on = i == idx;
            tab.btn.set_checked(on);
            set_style_index(tab.icon.obj(), &tab.icon_styles, &tab.icon_idx, usize::from(on));
            set_style_index(tab.text.obj(), &tab.text_styles, &tab.text_idx, usize::from(on));
            set_visible(&tab.bar, on);
        }

        set_visible(&self.back, page.shows_back());
        self.title.set_text(page.title());
        if page.shows_back() {
            self.title.set_pos(HEADER_TITLE_X_PAGED, HEADER_TITLE_Y);
            self.title.set_size(HEADER_TITLE_W, TextSlot::PageTitle.px() as i32);
        } else {
            self.title.set_pos(HEADER_TITLE_X, HEADER_TITLE_Y);
            self.title.set_size(HEADER_TITLE_W_P1, TextSlot::PageTitle.px() as i32);
        }

        if changed && notify {
            self.on_page_change.fire(page);
        }
    }

    /// 页眉右端三件（通道胶囊 / 触摸角标 / 倒计时）的显隐 —— **唯一**落点。
    ///
    /// **SH3**：倒计时胶囊出现时，通道胶囊与触摸角标让位（1024 px 放不下五者）。
    fn apply_header_channel(&self) {
        let capsule = self.capsule_on.get();
        let ch = self.channel.get();
        for (i, chip) in self.chips.iter().enumerate() {
            set_visible(chip.obj(), !capsule && i == ch.index());
        }
        set_visible(&self.badge, !capsule && !self.touch_available.get());
    }

    /// 整屏降级层（EDGE-03）的显隐与时长文本 —— **唯一**落点（每拍由 [`Shell::tick`] 调）。
    ///
    /// 语义（三条，逐条对应模块头的实现裁定）：
    /// 1. **只切可见性、也不挂样式**：本函数**不建、不删**任何 LVGL 对象（创建全在 [`Shell::new`]）、
    ///    更**不**调 [`Obj::add_style`]（`lv_obj_add_style` 只增不删 ⇒ "每拍挂一条"是**独立于
    ///    对象数**的泄漏形态）。回归锁 = `shell_chain` 的"断态连推 50 拍 ⇒ `PROBE_MOUNTS`
    ///    **与** `PROBE_STYLE_ATTACHES` **双零增长**"；
    /// 2. **断态 ⇔ 可见**：通道态取自 `Core::channel`（[`HeaderChannel::Down`] 即 EDGE-03 / EDGE-20
    ///    的共同触发条件）；非断态**下一拍**即撤遮罩并把断开始刻清零（"恢复 ≤1 s 回实时"）；
    /// 3. **时长只在整秒变化时写**：首拍落 `Some(now)`（⇒ 从 `0 秒` 起），此后每拍算
    ///    `now − since` 的**整秒**值，与上次上屏值不同才 `set_text`（并同步居中宽度 / x）。
    ///    `now` 由调用方注入 ⇒ 离屏可逐拍确定性断言。
    fn apply_overlay(&self, now: Instant) {
        // ── 非断态：撤遮罩 + 清断开始刻（**唯一**的"恢复"路径）──
        if !matches!(self.channel.get(), HeaderChannel::Down) {
            *self.overlay_since.borrow_mut() = None;
            self.overlay_secs.set(None);
            if self.overlay_on.replace(false) {
                set_visible(&self.overlay, false);
            }
            return;
        }
        // ── 断态：断开始刻只在**进入断态的第一拍**落定（`now` 注入 ⇒ 可复现）──
        let secs = {
            let mut since = self.overlay_since.borrow_mut();
            match *since {
                Some(t) => now.saturating_duration_since(t).as_secs(),
                None => {
                    *since = Some(now);
                    0
                }
            }
        };
        if self.overlay_secs.get() != Some(secs) {
            let text = disconnect_elapsed_text(secs);
            // 位数变化 ⇒ 宽度变 ⇒ 重新水平居中（`≤1 Hz` 的写，不在每拍路径上）。
            self.overlay_elapsed
                .set_size(overlay_elapsed_w(&text), OVERLAY_ELAPSED_SLOT.px() as i32);
            self.overlay_elapsed
                .set_pos(overlay_elapsed_x(&text), OVERLAY_ELAPSED_Y);
            self.overlay_elapsed.set_text(&text);
            self.overlay_secs.set(Some(secs));
            self.overlay_writes.set(self.overlay_writes.get() + 1);
        }
        if !self.overlay_on.replace(true) {
            set_visible(&self.overlay, true);
        }
    }

    /// 未保存提示条的显隐（EDGE-11）—— **唯一**落点。
    fn apply_dirty(&self) {
        let dirty = self.p2.is_dirty();
        if dirty != self.banner_on.get() {
            self.banner_on.set(dirty);
            set_visible(&self.banner, dirty);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. 样式与零件构造（色值 / 尺寸**一律**取 `theme` 命名常量；零裸值）
// ═══════════════════════════════════════════════════════════════════════════

/// 页眉底：`Palette::SURFACE`、直角、无描边。
///
/// ⚠️ 原文档曾写"下方 1 px 分隔线由 `nav` 一侧表达" —— **不实**：`nav_bg()` 是
/// `Stroke::NONE` 且无上描边，页眉与内容区之间**没有**分隔线（见 **SH13**：§3.5 里那条线
/// 属示意图，规格未给出线宽/色值/归属）。2026-09-15 代码质量评审点出该矛盾后删去。
fn header_bg() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE);
    s.set_bg_opa(Opa::COVER);
    s.set_border_width(Stroke::NONE);
    s.set_pad_all(0);
    s.set_radius(Radius::NONE);
    Rc::new(s)
}

/// 底部导航底（UI §4.2 默认态底 `#141F33`）。
fn nav_bg() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE);
    s.set_bg_opa(Opa::COVER);
    s.set_border_width(Stroke::NONE);
    s.set_pad_all(0);
    s.set_radius(Radius::NONE);
    Rc::new(s)
}

/// 倒计时胶囊皮肤（UI §4.3：底 `#3A2E12`、描边 `#FFB020`、全圆端）。
fn capsule_skin() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::WARN_BG);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::STALE);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_radius(Radius::CHIP);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 整屏降级遮罩皮肤（UI §8.3 EDGE-03：`#0B1220`、**压暗 20 %**）。
///
/// ⚠️ **必须清 `pad_all` 与 `radius`**（与 `theme::screen_bg()` / `dialog_mask()` 同款）：
/// 本对象是**全屏**遮罩，若吃到 LVGL 默认主题的 `card` 内边距（≈20 px）就会缩进、四边露白。
/// 这是 **SH14**（同类陷阱）复发的封堵点。
///
/// 与 `theme::dialog_mask()`（62 %，模态）**不是同一件事**：EDGE-03 的压暗只提示"数据已冻结"，
/// 底层必须仍然可读（EDGE-20）⇒ 20 %。
///
/// **档位只许经 [`OVERLAY_MASK_OPACITY`] 取用**（重要 1+2 的单点收口）—— 不在此处内联
/// `Opacity::*` 常量：那样"换档"会绕过唯一真源，而对象层又读不回 `bg_opa`（见该常量的残余说明）。
fn overlay_mask() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::BG);
    s.set_bg_opa(Opa::percent(OVERLAY_MASK_OPACITY));
    s.set_border_width(Stroke::NONE);
    s.set_radius(Radius::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 未保存提示条皮肤（UI §4.3：底 `#3A2E12`、描边 `#FFB020`、h48）。
fn banner_skin() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::WARN_BG);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::STALE);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_radius(Radius::CTRL);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 导航项的一档"皮肤"：**底色样式** + 它的**应用标记**（两者**同源** —— 由同一个
/// [`TabBg`] 派生，见 [`Shell::tab_bg`] 的回归锁说明）。
struct NavSkin {
    /// 挂到页签按钮上的状态选择器样式。
    style: Rc<Style>,
    /// 该档底色的应用标记（= 送进 `set_bg_color(..)` 的那个值）。
    bg: TabBg,
}

/// 建一档导航项样式（**底色由 [`TabBg`] 唯一决定** ⇒ 样式与标记不可能各说各话）。
///
/// **改什么会让本条变红**：把 [`NAV_BG_SELECTED`] 的 `Palette::SURFACE_ALT` 换成别的
/// `Palette` 常量 ⇒ 本函数拿到的是新 `TabBg` ⇒ 挂上屏的底色与 `shell_chain` 读回的标记
/// **一起变** ⇒ 断言红。
fn nav_item_skin(bg: TabBg) -> NavSkin {
    let mut s = Style::new();
    match bg {
        // 未选中：透明底（露出导航条的 `Palette::SURFACE`，UI §4.2）。
        TabBg::Transparent => s.set_bg_opa(Opa::TRANSPARENT),
        // 选中 `surface_alt` / 按下 `#2E4066`（UI §4.2 / §5.2）。
        TabBg::Solid(c) => {
            s.set_bg_color(c);
            s.set_bg_opa(Opa::COVER);
        }
    }
    s.set_border_width(Stroke::NONE);
    s.set_radius(Radius::NONE);
    s.set_pad_all(0);
    NavSkin {
        style: Rc::new(s),
        bg,
    }
}

/// 建一个导航页签（按钮 + 顶部选中条 + 左缘竖分隔 + 图标 + 文案）。
fn build_tab(parent: &Obj, page: NavPage, item_skins: &[NavSkin; 3]) -> Result<NavTab, LvglError> {
    let btn = TextButton::create(parent, "")?;
    btn.set_size(Dimens::NAV_ITEM_W, Dimens::NAV_ITEM_H);
    btn.set_pos(Dimens::NAV_ITEM_W * page.index() as i32, 0);
    btn.set_checkable(true);
    // 三档底色：一次挂齐（LVGL 按状态自选），运行期不再切换 ⇒ 标记按槽位随样式同批记下。
    for st in TabState::ALL {
        let skin = &item_skins[st.slot()];
        let sel = match st {
            TabState::Default => StyleSelector::state_of(State::DEFAULT),
            TabState::Selected => StyleSelector::state_of(State::CHECKED),
            TabState::Pressed => StyleSelector::state_of(State::PRESSED),
        };
        btn.add_style(&skin.style, sel);
    }
    let bg_marks = [
        item_skins[TabState::Default.slot()].bg,
        item_skins[TabState::Selected.slot()].bg,
        item_skins[TabState::Pressed.slot()].bg,
    ];

    // 顶部 4 px 选中条（UI §4.2；默认隐藏，选中时由 `Core::select` 点亮）。
    let bar = decor(
        &btn,
        Dimens::NAV_ITEM_W,
        Dimens::NAV_SELECT_BAR_H,
        &theme::card_head_bar(Palette::INFO),
    )?;
    bar.set_pos(0, NAV_BAR_Y);
    set_visible(&bar, false);

    // 左缘 1 px 竖分隔（UI §4.2：第 2–6 项左侧；第 1 项无）。
    let divider = if page.index() == 0 {
        None
    } else {
        let d = decor(&btn, NAV_DIVIDER_W, Dimens::NAV_ITEM_H, &theme::card_head_bar(Palette::DIVIDER))?;
        d.set_pos(0, 0);
        Some(d)
    };

    let icon_colors = [Palette::TEXT_WEAK, Palette::INFO];
    let text_colors = [Palette::TEXT_SECOND, Palette::TEXT_PRIMARY];
    let icon_styles = [
        theme::text(TextSlot::SectionTitle, icon_colors[0]),
        theme::text(TextSlot::SectionTitle, icon_colors[1]),
    ];
    let text_styles = [
        theme::text(TextSlot::Label, text_colors[0]),
        theme::text(TextSlot::Label, text_colors[1]),
    ];

    // 图标 / 文案标签**不预先挂颜色样式**：颜色是"选中态双通道"的一半，由
    // [`set_style_index`] 在 0（未选中）/ 1（选中）两档间切换（先挂一条固定色再叠加，
    // 会让 LVGL 样式表里永久留一条无用项 —— 见 `Core::select` 的第一次调用：`cur = usize::MAX`
    // ⇒ 直接挂目标档）。字体仍由档位样式给出（`icon_styles` / `text_styles` 内含 `set_text_font`）。
    let icon = Rc::new(Label::create_with_text(&btn, page.nav_icon())?);
    icon.remove_flag(ObjFlag::CLICKABLE);
    icon.remove_flag(ObjFlag::SCROLLABLE);
    icon.set_size(Dimens::ICON_SM, Dimens::ICON_SM);
    icon.set_pos(tab_text_x(page.nav_icon()), NAV_ICON_Y);
    icon.set_long_mode(LongMode::CLIP);

    let text = Rc::new(Label::create_with_text(&btn, page.nav_label())?);
    text.remove_flag(ObjFlag::CLICKABLE);
    text.remove_flag(ObjFlag::SCROLLABLE);
    text.set_size(tab_text_w(page.nav_label()), TextSlot::Label.px() as i32);
    text.set_pos(tab_text_x(page.nav_label()), NAV_TEXT_Y);
    text.set_long_mode(LongMode::CLIP);

    Ok(NavTab {
        btn,
        bar,
        divider,
        icon,
        text,
        icon_styles,
        text_styles,
        icon_colors,
        text_colors,
        icon_idx: Cell::new(usize::MAX),
        text_idx: Cell::new(usize::MAX),
        bg_marks,
    })
}

/// 页签文案的估算宽（CJK 逐字宽 = 字号；薄层无文本度量接口，与 `pages/**` 同口径）。
fn tab_text_w(label: &str) -> i32 {
    label.chars().count() as i32 * TextSlot::Label.px() as i32
}

/// 页签内容的水平居中 x（`(项宽 − 文案宽) / 2`）。
fn tab_text_x(label: &str) -> i32 {
    (Dimens::NAV_ITEM_W - tab_text_w(label)) / 2
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. 纯逻辑单测（**不触碰 LVGL** —— LVGL 非线程安全，触碰它的用例只能由
//    `src/lvgl/tests.rs::lvgl_core_bridge_chain` 经 `ui/tests.rs` 串行调起）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    // ── 导航目标（页签索引 ↔ 页面）──

    /// **往返互逆**：`ALL[i].index() == i` 且 `from_index(i) == Some(ALL[i])`。
    ///
    /// **改什么会让本条变红**：把 [`NavPage::index`] 里任意两个分支对调（例如
    /// `Logs => 3` / `Interlock => 2`）⇒ 往返断言当场红；把 `ALL` 的顺序改成非"页签从左到右"
    /// （如把 `Audit` 提到 `Logs` 前）⇒ `ALL[i].index() == i` 红。
    #[test]
    fn nav_page_index_round_trips() {
        assert_eq!(NavPage::ALL.len(), 6, "六页签常驻（UI §4.2）");
        for (i, p) in NavPage::ALL.into_iter().enumerate() {
            assert_eq!(p.index(), i, "{p:?} 的页签下标必须是它在 ALL 里的位次");
            assert_eq!(
                NavPage::from_index(i),
                Some(p),
                "`from_index` 与 `index` 必须互逆"
            );
        }
        assert_eq!(NavPage::from_index(6), None, "越界索引 ⇒ None（不 panic）");
    }

    /// 页眉 `返回` 键**仅 P2–P6 显示**（UI §4.1）。
    ///
    /// **改什么会让本条变红**：把 [`NavPage::shows_back`] 改成 `true`（P1 也显示返回键）、
    /// 或把 `matches!` 反过来（只有 P1 显示）。
    #[test]
    fn back_key_hidden_on_main_page_only() {
        assert!(!NavPage::Main.shows_back(), "P1 不显示返回键（UI §4.1）");
        for p in [
            NavPage::Config,
            NavPage::Logs,
            NavPage::Interlock,
            NavPage::Audit,
            NavPage::System,
        ] {
            assert!(p.shows_back(), "{p:?} 必须显示返回键");
        }
    }

    // ── 空闲超时（给定 `now` 序列 ⇒ 胶囊出现 / 文案 / 到 0 切页）──

    /// 60 s 空闲的**逐拍**语义：11 s 前无胶囊、10 s 起有胶囊、到 0 该切页。
    ///
    /// **改什么会让本条变红**：把 [`COUNTDOWN_WINDOW_SECS`] 从 10 改成 12（`t=50` 那一条仍是
    /// 胶囊 —— 不红；但 `t=49` 会变成"应显示胶囊"⇒ 红）；把 [`remaining_secs`] 的
    /// `div_ceil` 换成 `as_secs()`（向下取整）⇒ `t=50` 的 `secs` 变 10 仍红不了，但
    /// `t=49.5`（下面用 49 s + 500 ms 覆盖）会落到 10 ⇒ 与本条的"11 s 前无胶囊"冲突而红。
    #[test]
    fn idle_countdown_boundaries() {
        let base = t0();
        let timeout = Duration::from_secs(theme::Timing::IDLE_TIMEOUT_SECS);
        let at = |s: u64, ms: u64| base + Duration::from_secs(s) + Duration::from_millis(ms);

        // t=0：满时长，无胶囊。
        assert_eq!(remaining_secs(at(0, 0), base, timeout), 60);
        assert!(!shows_countdown(60));
        // t=49：还剩 11 s ⇒ 不在窗口内。
        assert_eq!(remaining_secs(at(49, 0), base, timeout), 11);
        assert!(!shows_countdown(11), "超时前 11 s 尚不显示胶囊");
        // t=49.5：还剩 10.5 s ⇒ 向上取整 = 11 ⇒ 仍不显示（若改向下取整会得到 10 ⇒ 红）。
        assert_eq!(remaining_secs(at(49, 500), base, timeout), 11);
        assert!(!shows_countdown(11));
        // t=50：还剩 10 s ⇒ 窗口起点。
        assert_eq!(remaining_secs(at(50, 0), base, timeout), 10);
        assert!(shows_countdown(10), "超时前 10 s 起显示胶囊（UI §4.3）");
        assert_eq!(countdown_text(10), "10 秒后返回主状态页");
        // t=59：还剩 1 s。
        assert_eq!(remaining_secs(at(59, 0), base, timeout), 1);
        assert!(shows_countdown(1));
        assert_eq!(countdown_text(1), "1 秒后返回主状态页");
        // t=60：到 0 ⇒ 该切页。
        assert_eq!(remaining_secs(at(60, 0), base, timeout), 0);
        assert!(should_return_home(0));
        assert!(!shows_countdown(0), "到 0 时胶囊不再显示（本拍即切页）");
        // 超时后再推进：仍停在 0（饱和，不回绕）。
        assert_eq!(remaining_secs(at(90, 0), base, timeout), 0);
    }

    /// 胶囊显隐判据与"到 0 切页"判据**互斥**（同一 `secs` 不可能两者都真）。
    ///
    /// **改什么会让本条变红**：把 [`shows_countdown`] 的条件写成 `secs <= 10`（漏掉 `secs > 0`）
    /// ⇒ `secs == 0` 时两者同时为真 ⇒ 红。
    #[test]
    fn countdown_and_return_are_mutually_exclusive() {
        for secs in 0..=70u64 {
            assert!(
                !(shows_countdown(secs) && should_return_home(secs)),
                "secs={secs} 不得既显胶囊又切页"
            );
        }
        assert!(shows_countdown(1) && !should_return_home(1));
        assert!(!shows_countdown(0) && should_return_home(0));
    }

    /// 倒计时文案的整数口径（UI §3.6 页眉超时行）。
    ///
    /// **改什么会让本条变红**：把 [`countdown_text`] 的格式串改成 `{secs} 秒后返回` 一类
    /// （少字 / 换字 / 换分隔符）。
    #[test]
    fn countdown_text_matches_spec_wording() {
        assert_eq!(countdown_text(10), "10 秒后返回主状态页");
        assert_eq!(countdown_text(3), "3 秒后返回主状态页");
    }

    // ── 整屏降级（EDGE-03）──

    /// 时长文案的整数口径（`N 秒`；**偏差 SH15**：是"已断开时长"，不是倒计时）。
    ///
    /// **改什么会让本条变红**：把模板改成 `{secs} 秒后恢复`（谎报语义）/ `{secs} 秒后返回`
    /// （那是页眉倒计时胶囊的口径，两者混用即"通道断时说还有多久恢复"= 造假）。
    #[test]
    fn disconnect_elapsed_text_is_plain_seconds() {
        assert_eq!(disconnect_elapsed_text(0), "0 秒");
        assert_eq!(disconnect_elapsed_text(1), "1 秒");
        assert_eq!(disconnect_elapsed_text(59), "59 秒");
        assert_eq!(disconnect_elapsed_text(3600), "3600 秒");
        assert_ne!(
            disconnect_elapsed_text(10),
            countdown_text(10),
            "EDGE-03 的时长行与页眉倒计时胶囊是**两种语义**，文案不得互相借用"
        );
    }

    /// 整屏层版式：大字 + 时长行**在画布内垂直居中**（UI §8.3「中央」的可判定口径）。
    ///
    /// **改什么会让本条变红**：把 `OVERLAY_BLOCK_H` 里的呼吸缝换成别的档（如 0 或
    /// `GAP_MIN`）⇒ 上下留白不再相等 ⇒ 第一条红；把大字字号档从 64 px 换掉 ⇒ 第二 / 三条红。
    #[test]
    fn overlay_block_is_centered_and_uses_64px_title() {
        assert_eq!(
            TextSlot::PhasePower.px(),
            64,
            "中央大字必须取 64 px 档（UI §8.3 EDGE-03 原文「中央 64 px」）"
        );
        // 两个标签的**字号槽必须不同**（64 ≠ 24）—— 防"两处写反"（重要 1+2 的纯逻辑那一半；
        // 对象级那一半见 `ui/tests.rs::shell_chain`）。**改什么会让本条变红**：把
        // `OVERLAY_TITLE_SLOT` 与 `OVERLAY_ELAPSED_SLOT` 换成同一个槽。
        assert_eq!(OVERLAY_TITLE_SLOT, TextSlot::PhasePower, "大字的唯一绑定槽");
        assert_eq!(OVERLAY_ELAPSED_SLOT, TextSlot::Body, "时长行的唯一绑定槽");
        assert_ne!(
            OVERLAY_TITLE_SLOT.px(),
            OVERLAY_ELAPSED_SLOT.px(),
            "大字与时长行必须取**不同**字号槽（64 ≠ 24），否则两行分不出主次"
        );
        assert_eq!(
            OVERLAY_TITLE_Y,
            Dimens::SCREEN_H - OVERLAY_ELAPSED_Y - TextSlot::Body.px() as i32,
            "整块须在画布内垂直居中（上留白 == 下留白）"
        );
        assert_eq!(OVERLAY_ELAPSED_Y, OVERLAY_TITLE_Y + 64 + Dimens::GAP_SECTION);
        assert_eq!(
            2 * OVERLAY_TITLE_Y + OVERLAY_BLOCK_H,
            Dimens::SCREEN_H,
            "整块 + 上下等留白 == 画布高（大字与时长行都落在画布内）"
        );
        assert_eq!(
            overlay_title_x(),
            (Dimens::SCREEN_W - overlay_title_w()) / 2,
            "大字水平居中"
        );
        // 大字宽 = 逐字宽 × 字数（与 `discard_w()` / `tab_text_w()` 同口径）。
        assert_eq!(
            overlay_title_w(),
            OVERLAY_TEXT.chars().count() as i32 * 64,
            "大字宽按逐字宽估算（薄层无文本度量接口）"
        );
    }

    // ── 页眉通道状态（连接 / 断开 / EDGE-20 双状态）──

    /// 三态派生 + 文案 / 图标 / 皮肤（颜色通道）齐备。
    ///
    /// **改什么会让本条变红**：把 `ChannelStatus::Down` 映射到 [`HeaderChannel::Connecting`]
    /// （断开显示成"连接中"）⇒ 第一条断言红；把 `HeaderChannel::Down::skin()` 改成
    /// `ChipSkin::SUCCESS`（红变绿）⇒ 皮肤断言红。
    #[test]
    fn header_channel_maps_status_to_three_channel_chip() {
        assert_eq!(
            HeaderChannel::from_status(ChannelStatus::Connected),
            HeaderChannel::Connected
        );
        assert_eq!(
            HeaderChannel::from_status(ChannelStatus::Init),
            HeaderChannel::Connecting
        );
        assert_eq!(
            HeaderChannel::from_status(ChannelStatus::Down),
            HeaderChannel::Down
        );
        // F14 三通道：文字 / 图标 / 颜色**三者都有**，且三态互不相同。
        let all: Vec<(usize, &str, &str, ChipSkin)> = [
            HeaderChannel::Connected,
            HeaderChannel::Connecting,
            HeaderChannel::Down,
        ]
        .into_iter()
        .map(|c| (c.index(), c.text(), c.icon(), c.skin()))
        .collect();
        let idx: Vec<usize> = all.iter().map(|x| x.0).collect();
        assert_eq!(idx, vec![0, 1, 2], "三态的下标必须是 0/1/2（唯一对应表）");
        let texts: Vec<&str> = all.iter().map(|x| x.1).collect();
        assert_eq!(
            texts,
            vec![
                TEXT_CHANNEL_OK,
                p1_status::TEXT_CHANNEL_CONNECTING,
                p1_status::TEXT_CHANNEL_DOWN
            ]
        );
        assert_eq!(
            HeaderChannel::Down.text(),
            p1_status::TEXT_CHANNEL_DOWN,
            "EDGE-20 的红通道文案（与 P1 页内通道条**同一份字面量**）"
        );
        assert_eq!(HeaderChannel::Down.skin(), ChipSkin::FAILURE, "断 = 红");
        assert_eq!(HeaderChannel::Connected.skin(), ChipSkin::SUCCESS, "通 = 绿");
        assert_ne!(
            HeaderChannel::Connected.icon(),
            HeaderChannel::Down.icon(),
            "图标通道必须可区分（● vs !）"
        );
    }

    // ── 页眉栅格常量自洽（**不改 LVGL 也能查的版式算术**）──

    /// 页眉各槽位**不重叠**、且都在画布内（SH3 的让位规则是它们的**行为**面）。
    ///
    /// **右端组右锚定**（② 的回归锁）：`触摸角标 → 通道胶囊 → 时钟 →|右安全边` 三件以
    /// [`Dimens::GAP_GROUP`] 等缝相连、整组贴右安全边 ⇒ **通道胶囊不再落在页眉中段**。
    ///
    /// **改什么会让本条变红**：把 [`HEADER_CHIP_X`] 改回页眉中段（如
    /// `Dimens::CONTENT_W * 2 / 5 + Dimens::GAP_GROUP` = 412）⇒ 「胶囊左缘在右半区」与
    /// 「胶囊 + 缝 == 时钟左缘」**两条同时红**；把 [`HEADER_TITLE_W_P1`] 的推导改成
    /// "不减呼吸缝"⇒ 标题右缘压到右端组上，相交断言红。
    #[test]
    fn header_slots_are_disjoint_and_inside_canvas() {
        let p1_title = (HEADER_TITLE_X, HEADER_TITLE_X + HEADER_TITLE_W_P1);
        let paged_title = (HEADER_TITLE_X_PAGED, HEADER_TITLE_X_PAGED + HEADER_TITLE_W);
        let chip = (HEADER_CHIP_X, HEADER_CHIP_X + HEADER_CHIP_W);
        let badge = (HEADER_BADGE_X, HEADER_BADGE_X + HEADER_BADGE_W);
        let capsule = (HEADER_CAPSULE_X, HEADER_CAPSULE_X + HEADER_CAPSULE_W);
        let clock = (HEADER_CLOCK_X, HEADER_CLOCK_X + HEADER_CLOCK_W);

        // 返回键独占 x 12–76（UI §4.1）。
        assert_eq!(HEADER_BACK_X, 12);
        assert_eq!(HEADER_BACK_X + Dimens::TOUCH_CRITICAL, 76);
        // ── **右端组**（UI §4.1「右端：时钟 + 通道状态胶囊」；§7.5 的角标也在右端）──
        // ① 通道胶囊**左缘落在右半区**（"右端"的可判定判据：中段布局 412 < 512 会被抓）。
        assert!(
            chip.0 >= Dimens::SCREEN_W / 2,
            "通道胶囊必须落在页眉**右半区**（右端组），实得 x={}",
            chip.0
        );
        // ② 胶囊**紧跟时钟**（中间只隔一个呼吸缝）—— 右锚定链的第一环。
        assert_eq!(
            chip.1 + Dimens::GAP_GROUP,
            clock.0,
            "通道胶囊右缘 + 一个呼吸缝 == 时钟左缘（§4.1「右端」；改回中段 ⇒ 红）"
        );
        // ③ 触摸角标**紧跟胶囊**（同在右端组，§7.5）。
        assert_eq!(
            badge.1 + Dimens::GAP_GROUP,
            chip.0,
            "触摸角标右缘 + 一个呼吸缝 == 通道胶囊左缘（右端组等缝相连）"
        );
        // ④ 整组贴右安全边（时钟右缘 + 安全边 == 画布宽）。
        assert_eq!(clock.1 + Dimens::SIDE_PAD, Dimens::SCREEN_W);
        // ── 互斥（Title vs 右端组）──
        assert!(p1_title.1 <= badge.0, "P1 标题不得压到右端组最左成员（触摸角标）");
        assert!(paged_title.1 <= badge.0, "分页标题不得压到右端组最左成员");
        assert!(
            paged_title.0 > HEADER_BACK_X + Dimens::TOUCH_CRITICAL,
            "标题须在返回键右侧"
        );
        // 倒计时胶囊**必然**与通道胶囊相交 ⇒ SH3 的让位是必需的，不是可选的。
        assert!(
            capsule.0 < chip.1,
            "倒计时胶囊与通道胶囊相交 ⇒ SH3 让位规则必需（若二者不再相交，SH3 应被删除）"
        );
        // SH3 的根判据是**内在宽度和**（与位置无关；② 改版式后仍然成立）：
        // 返回 64 + 标题（P1 实测需 320）+ 缝 + 通道胶囊 280 + 缝 + 倒计时胶囊 264 + 缝 + 时钟 120
        // + 右安全边 16 > 1024 ⇒ 五者**不可能**同时上屏（见 SH3 的算式）。
        let min_total = Dimens::TOUCH_CRITICAL
            + HEADER_TITLE_W_P1
            + Dimens::GAP_GROUP
            + HEADER_CHIP_W
            + Dimens::GAP_GROUP
            + HEADER_CAPSULE_W
            + Dimens::GAP_GROUP
            + HEADER_CLOCK_W
            + Dimens::SIDE_PAD;
        assert!(
            min_total > Dimens::SCREEN_W,
            "页眉五者的内在宽度和 {min_total} 必须 > 画布宽 {} ⇒ SH3 让位必需（防登记腐化：\
             若某天排得下五者，SH3 与让位逻辑应被删除）",
            Dimens::SCREEN_W
        );
    }

    /// 导航三档底色的**唯一真源**与**互斥性**（**①** 的纯逻辑那一半；LVGL 侧读回见
    /// `ui/tests.rs::shell_chain`）。
    ///
    /// **改什么会让本条变红**：把 [`NAV_BG_SELECTED`] / [`NAV_BG_PRESSED`] 换成同一个值
    /// （三档同色 —— "全都一样"的蒙混形态）⇒ 互斥断言红；把选中档换成 `Palette::SURFACE`
    /// （画布/导航条同色 ⇒ 选中态在导航条上看不出来）⇒ 真源断言红。
    #[test]
    fn nav_item_bg_marks_are_distinct_and_from_theme() {
        assert_eq!(
            NAV_BG_DEFAULT,
            TabBg::Transparent,
            "未选中档必须是**透明**底（UI §4.2「未选中：底色透明」）"
        );
        assert_eq!(
            NAV_BG_SELECTED,
            TabBg::Solid(Palette::SURFACE_ALT),
            "选中档必须是 `surface_alt`（UI §4.2「选中：底 `#1B2942`」）"
        );
        assert_eq!(
            NAV_BG_PRESSED,
            TabBg::Solid(Palette::SURFACE_PRESS),
            "按下档必须是 `surface_press`（UI §4.2「按下 底 `#2E4066`」）"
        );
        // 互斥：任何两档都不得同值（防"三档一个色"蒙过"选中态有底色"的断言）。
        for a in TabState::ALL {
            for b in TabState::ALL {
                if a != b {
                    assert_ne!(a.bg(), b.bg(), "{a:?} 与 {b:?} 的底色不得同值");
                }
            }
        }
        // 槽位表自洽（`TabState::ALL` 的位次 == `slot()`）。
        for (i, st) in TabState::ALL.into_iter().enumerate() {
            assert_eq!(st.slot(), i, "{st:?} 的槽位必须是它在 ALL 里的位次");
        }
    }

    /// 导航 6 项铺满可视宽且不越界（`NAV_ITEM_W` 是 theme 的单一真源，见 SH10）。
    ///
    /// **改什么会让本条变红**：把 `NAV_ITEM_W` 用到第 7 项、或把项宽调大到越界。
    #[test]
    fn nav_items_fit_canvas() {
        let total = Dimens::NAV_ITEM_W * NavPage::ALL.len() as i32;
        assert!(total <= Dimens::SCREEN_W, "6 项合计 {total} 必须 ≤ 画布宽");
        assert_eq!(Dimens::HEADER_H + Dimens::CONTENT_H + Dimens::NAV_H, Dimens::SCREEN_H);
    }
}
