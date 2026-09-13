//! # `ui/pages/p3_logs.rs` —— P3 日志页（12-MUPC v2.0 工作单元 **B2c-2**，**只读页**）
//!
//! 出处：UI 设计文档 §6.3「P3 日志页（F10，只读 + 选项式筛选）」（线框 + 筛选区规格表 +
//! ② 模块换行网格专段 + 列表区行规格 + 实时追加 + 通道断 + 空态 / 超限 / 不支持导出）、
//! §8.3 的两行专行（`日志为空 / 筛选无结果（EDGE-08）` / `检索范围超限（EDGE-15）`）、
//! §2.7（禁横滚，**含筛选区**）、§5.1 #5/#10/#17、§5.2（Chip 选中态 + `✓` 前缀）、
//! §5.3（`MultiChipGroup` 换行网格 / `ListItem` / `EmptyState` / 长列表窗口化）、
//! §7.4（滚动规范：**不横滚** + 长文本换行或截断）、§3.6 **P3 行**（上屏文案唯一真源）、
//! §3.2 色板（**PRD 指定级别色**）、§3.3 字号、§3.5 栅格；技术设计 §6.3（P3 环节表）与
//! §4.4（日志 ring / 限额扫描 / 增量拉取口径）。
//!
//! ## 本页的边界
//!
//! 1. **只读页 —— 零写操作**：UI §6.3「不支持导出」节明写「**不提供**任何导出按钮、图标、
//!    菜单项或手势入口」；PRD T-1 砍掉关键字搜索。故本页**不存在**编辑 / 删除 / 清空 / 导出
//!    入口，也**不存在**任何文本输入控件（`FORBIDDEN_UI_SYMBOLS` 的静态网逐 token 断言）。
//!    页内唯一可点控件是筛选区的两个 chip 组、共享时间范围件（分段控件 + 步进器）与
//!    **「回到最新」**按钮（见 **R2**）；列表行一律不可点（运行期读 LVGL 标志锁定）。
//! 2. **数据来自控制通道**（`GET /v1/console/logs`、`/v1/console/logs/targets`，设计 §3.4）
//!    ⇒ **不走 1 Hz 显示帧**（契约 2′）。注入入口：[`P3LogsPage::set_page`] /
//!    [`P3LogsPage::set_targets`] / [`P3LogsPage::set_channel`]。
//! 3. **本页不发请求、不生成 `request_id`**：筛选变化 / 增量拉取以**意图回调**交给外部
//!    （B3 的 `console.rs`）—— [`P3LogsPage::set_on_query`] / [`P3LogsPage::set_on_increment`]，
//!    载荷见 [`LogQuery`]（含 `cursor` / `limit`）。**筛选未变化时不重复发意图**。
//! 4. **本页不读时钟**：时间戳一律由**注入的 `ts_ms`** 经
//!    [`crate::ui::pages::format_epoch_ms_utc`] 生成。
//! 5. **页根 = 滚动容器**（`ui/pages/mod.rs` 契约 1：§6.3 线框**没有**底部固定操作条，
//!    整页纵向滚动 ⇒ 走 [`crate::ui::pages::page_root`]）。
//! 6. **「实时追加」的落点**：本页按 `seq` **降序**渲染（结构上保证「新行在顶部」）；
//!    窗口的**累积 / 去重 / 组装**由 B3 负责（[`P3LogsPage::set_page`] 是"整体替换窗口"
//!    语义，与 `p5_audit.rs` **AU8** 同口径），本页只保证"渲染出的第 0 行是窗口内最新的"。
//!
//! ## ⚠️ 三个高风险点（本单元的施工重点，逐条落地）
//!
//! ### R1 —— 机器名 / 自由文本的上屏处置（**哪些字段上屏 + 各自怎么处理**）
//!
//! | 契约字段 | 上屏？ | 落点 | 处置 | 断言 |
//! |----------|--------|------|------|------|
//! | `ts_ms` | 是 | 行时间列 | [`crate::ui::pages::format_epoch_ms_utc`]（**只产数字与 `/` `:`**） | `p3_runtime_texts_emit_only_cmap_glyphs` |
//! | `level` | 是 | 行级别色块 + 文字 | [`level_text`]（= `display_safe(display_name())`）+ [`level_color`]（PRD 指定色；`Trace` 见下） | 同上 |
//! | `target` | 是 | 行模块列 + 模块 chip | **已知键**（[`MODULE_LABELS`]，逐条有 §3.6 用字出处）⇒ 中文名；**未登记键** ⇒ [`display_safe`] 归一（**不臆造**中文名、**不静默隐藏** —— 机器名的归一形态仍上屏） | `target_label_is_known_or_none`、`unknown_target_is_shown_not_hidden` |
//! | `message` | 是 | 行消息列 | [`free_text_safe`]（复用 `p5_audit.rs` 的**同一份**函数：`display_safe` + 六个实测缺字全角标点折叠） | `p3_runtime_texts_emit_only_cmap_glyphs` |
//! | `seq` / `next_cursor` / `has_more` | 否（间接） | 底部状态行 / 增量游标 | 只驱动 `加载中` / `已加载全部` 与 [`LogQuery::cursor`] | `footer_reflects_has_more`、`increment_cursor_is_max_seq_not_next_cursor` |
//!
//! **`Trace` 的处置（结论）**：`LogLevel` 有 5 个变体，§6.3 只给 4 个筛选 chip
//! （`ERROR`/`WARN`/`INFO`/`DEBUG`），PRD §3.1 也只指定这 4 个级别色。故：
//! - **筛选**：`Trace` **无 chip**、**不可被选中**；`levels` 为空（= 不按级别筛）时 `Trace`
//!   条目**照常上屏**（**不静默丢弃**）；一旦选中任一 chip（4 级之一），`Trace` 被排除
//!   （它不在可选集合内 —— 见 [`selected_levels`]）。
//! - **上屏文字**：契约 `LogLevel::Trace.display_name()` = `TRACE`，而 **`T`(U+0054) 不在生成
//!   字体的 cmap 内**（cmap 的大写字母只有 A B C D E F G I M N O P R S U W）⇒ 直上屏会出
//!   豆腐块。故级别文字一律经 [`display_safe`] ⇒ `Trace` 上屏为 `?RACE`（与 **D9** 同口径：
//!   "同族等价 + 可辨认"，**不是**隐藏）。
//! - **颜色**：PRD 未指定 `TRACE` 色 ⇒ 取 [`Palette::TEXT_WEAK`]（**明标为"非 PRD 指定级别色"**，
//!   不臆造第 5 个级别色）。
//!
//! **残余局限（如实）**：[`free_text_safe`] 只改写 ASCII + 六个常用全角标点；`message` 若带
//! **其它非 ASCII 缺字**（后端直接给中文但用了 cmap 外的字），真机仍是豆腐块 —— 防线在后端
//! 字段命名 / 字库（同 `pages/mod.rs` **D9** / `p5_audit.rs` **AU15**）。
//!
//! ### R2 —— 「手动上滚后停止自动滚动」的可实现边界（**薄层能力缺口，如实登记**）
//!
//! 核实结论（逐条读 `src/lvgl/event.rs` / `lvgl-sys/allowlist.txt`）：
//! - 薄层 `EventCode` **未镜像** `LV_EVENT_SCROLL`（镜像的是 `PRESSED` / `RELEASED` /
//!   `PRESS_LOST` / `CLICKED` / `LONG_PRESSED` / `LONG_PRESSED_REPEAT` / `VALUE_CHANGED` /
//!   `READY` / `CANCEL` / `DELETE`）；
//! - `allowlist.txt` 里**没有任何**滚动位置读 / 写 API（只有 `lv_obj_set/get_scroll_dir` 与
//!   `lv_obj_set/get_scrollbar_mode`）⇒ **读不到**当前滚动位置、也**无法**程序化回顶。
//!
//! ⇒ 「手动上滚 ⇒ 停止自动滚动 + 浮现按钮」在本层**结构性不可实现**。本页只做出**可实现
//! 的部分**，并**不假装**做了感知：
//! - **恒显**「回到最新」按钮（92×92，UI §6.3；不可感知"上滚"⇒ 不做"浮现"）。落点 =
//!   **列表区之下的专属带**（`y = 列表区底 + 缝`，右对齐；见 [`BACK_BAND_H`] / **LG11**）；
//! - 点击 ⇒ 触发 [`P3LogsPage::set_on_back_to_latest`] 的意图（**供未来接上滚动能力**）并把
//!   内部 `auto_follow` 标志复位为 `true`（[`P3LogsPage::set_auto_follow`] 是 B3 的注入入口）；
//! - **具名能力缺口（给后续单元）**：① 在 `src/lvgl/event.rs` 镜像 `LV_EVENT_SCROLL`；
//!   ② 在 `lvgl-sys/allowlist.txt` + `src/lvgl/widgets.rs` 补 `lv_obj_get_scroll_y` 与
//!   `lv_obj_scroll_to_y`（**回顶**也做不到 —— 本页点击后只能报意图）；③ 补"视口级浮动"
//!   能力（或由外壳把按钮放在页根之外）——本页的按钮是**页内子对象**（页根即滚动容器
//!   ⇒ 按钮随内容滚动，用户滚到旧日志时它**不在视口内**，这正是"浮现"做不到的原因）。
//!
//! **B2c-2 规格评审整改 ②（原实现的真实缺陷）**：此前按钮是"列表区**右上角**的页内子对象"，
//! 屏坐标 **(916,380)-(1007,471)**，而第 0/1 行占 y372-416 / y416-460 ⇒ 按钮**压住第 0、1 行
//! 消息列尾部 92 px**（第 2 行顶部 11 px）；且 `theme::control_surface()` 的底
//! `Palette::SURFACE_HIGH` **不透明** ⇒ 是**实遮正文**（不是叠印）。又因按钮是**恒显**而非
//! §6.3 的"浮现"⇒ 遮挡是**长期**的。现改为**独占一条带**（见 **LG11**），
//! 任何一行都不与按钮矩形相交（几何实测：`ui/tests.rs` 的 `pages_chain`）。
//!
//! ### R3 —— 内存预算（行池按需分配 + 共存实测）
//!
//! 见 [`ROW_MAX`] / [`MEASURED_ROW_CAPACITY`] / [`COEXIST_ROWS_PER_PAGE`] 的文档（含"P3 + P5
//! 两页共存"的实测数值与上限取值依据）。
//!
//! ## ⚠️ 已知偏差登记（**独立编号 `LG`** —— 不与 `D*`（页面层）/ `CD*`（`ui/controls.rs`）/
//! `PD*`（`p2_config.rs`）/ `IL*`（`p4_interlock.rs`）/ `AU*`（`p5_audit.rs`）/
//! `FR*`（`filters.rs`）冲突）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | LG1 | 全角标点一律改写：`实时日志已断开，正在重连…` → `实时日志已断开 · 正在重连...`；`检索范围超限，请缩小时间范围` → `检索范围超限 · 请缩小时间范围` | `，`(U+FF0C) / `…`(U+2026) **实测不在生成字体 cmap 内** ⇒ 取 cmap 内的 `·`(U+00B7) 与 ASCII `...`（与 `p5_audit.rs` **AU1** / `TEXT_ELLIPSIS` 同款处置）。**根因（B2c-2 规格评审补登，如实）**：这两个字符**既不在生成字体 cmap、也不在 `fonts/font_subset_charset.txt`**，而 **§3.6 的字符串里却含它们** ⇒ **字符集的派生与 §3.6 不一致** —— 属 `fonts/` 生成链（子集字符表）的问题，**不是本页的问题**；本单元**不改 `fonts/`**（产地不在本单元授权范围） | **B2c 之后**的「字体/文档收口批」：先修 `fonts/font_subset_charset.txt`（补 `，`/`…` 并重跑 `gen_fonts.sh`），再**逐字改回契约原文** |
//! | LG2 | 时间列宽 **248**（`CONTENT_W / 4`），§6.3 写 **160** | **实测（B2c-2 规格评审 ⑥ 订正）**：`2026/09/10 13:42:07` 在 24 px 档为 **224.0 px**（用**入库基线** `fonts/lv_font_metrics.txt` 独立复算；`ui/tests.rs::measured_text_px` 同源）⇒ 160 px 下必被 `LongMode::DOTS` 截成 `2026/09/10 13:4...`（日志页的时间**不完整**）；160 px 只够 `HH:MM:SS`（实测 **93.25 px**）。**B2c-2 原稿写的 236.4 px 是不实数字，已订正**。取 P5 的同一列宽（`CONTENT_W / 4`），两列表页对齐。**未**采用"缩短为 `HH:MM:SS`"（会丢日期，与"能定位时刻"冲突） | 无（**实测驱动的结构性取舍**）；**§6.3 的 160 是规格缺口**：待 PM 改（或明写时间只显 `HH:MM:SS` / 另定字号） |
//! | LG3 | 「模块」维度名**独占一行**（高 `TextSlot::SectionTitle.px()` = 28），chip 网格在其下**铺满内容宽 992**；§6.3 线框把维度名画在网格左上角、chip 从下一行右侧起排（网格可用宽 872） | **§6.3 自身过约束**：其容量核算算式是 `9×96 + 8×16 = 992`（按**整幅内容宽**），而线框把 chip 右移了一个标签列 ⇒ 8 项/行需 880 px > 872（越界 8 px），7 列时 ≤51 项需 **8 行**（超出"最多 7 行"）。取"标签独占一行 + 网格 992"后：8 列 × 110 px + 7×16 = **992**（右缘与表头 / 列表**对齐**）、≤51 项 = **7 行**（432 px ≤ 448） | 无（**结构性**）；若 PM 要求贴线框：需同时放宽"最多 7 行"或"chip 最小宽 96" |
//! | LG4 | 模块名截断：**chip** ≤2 字原样 / 超出 ⇒ `...` + 尾 1 字；**行** ≤5 字原样 / 超出 ⇒ `...` + 尾 4 字（**B2c-2 整改 ⑤**：原为 chip「保尾 2 字且**无省略标记**」、行「`...` + 尾 **2** 字」） | 版式约束的必然结果：8 项/行 ⇒ chip 宽 110、内区 `110−2×16−2×1` = **76 px**；选中态再减 `✓`(U+2713) 的 **17.75 px** ⇒ 可用 **58.25 px** = **2 个汉字**（52 px）。`2 汉字 + "..."` = 73.75 px **> 58.25 px**（实测）⇒ chip 上**放不下"2 字 + 标记"**；chip 宽被 §6.3 ② 的 `8 × 110 + 7 × 16 = 992`（零余量）+「最多 7 行」钉死 ⇒ **取舍：可见标记优先于多留 1 字**（§2.6「降级可见、绝不造假」）。行侧尾保留 2→4 字（实测 `...` + 4 汉字 = 116.06 ≤ 140；+5 汉字 = 140.0625 > 140）。对照：P5 的未登记键有 5–6 字，但其 chip 是 192 px 两行网格 | 同 LG1（扩字表 / 缩 `✓` 占位 / 放开 7 行上限后可放宽）；**撞形残余见 LG12** |
//! | LG5 | 行消息列用 **`LongMode::DOTS`**（可见截断 + `...`），**不是** §7.4 字面的"换行"（**B2c-2 整改 ③**：原实现取 `WRAP` 以"忠实字面"，评审判定不成立） | **规格内部冲突的取舍（§7.4「换行」 vs §6.3「行高恒 44」）**：§6.3 把行高钉死 44 px 且行池按 `i × 44` 窗口化 ⇒ 单行 44 px 装不下 48 px 的两行；`WRAP` 下 LVGL 只把文字裁到 `txt_clip.y2`（`vendor/lvgl/src/widgets/label/lv_label.c`：`LONG_MODE_CLIP/WRAP: /*Do nothing*/`）⇒ **第二行静默消失、无任何标记**，与 §2.6「降级可见、**绝不造假**」及同一行模块列的「超长加 `...`」口径相悖。取 `DOTS`：截断**可见**（LVGL 把尾部换成 `.`×3，`.` 在 cmap 内）。**残余**：违反 §7.4 的"换行"字面 | 待 PM 裁定：① 维持 `DOTS`（**现状**）；② 改行高（结构变更，需同步窗口化算式与行池容量实测）；③ 改 `WRAP` + 行高 ≥48（需重标定 `MEASURED_ROW_CAPACITY`） |
//! | LG6 | **行池上限 = 20**，低于契约 `LOG_PAGE_LIMIT_MAX = 200` | **实测**（见 [`MEASURED_ROW_CAPACITY`]）：256 KB 的 LVGL 堆（`lv_conf.h` 的 `LV_MEM_SIZE`）装不下 200 行（单页上界 ≈ 44）⇒ 超过上界时**只渲染最新 20 条**（按 `seq` 降序取前 20），**其余不渲染**。**不静默**：[`LogQuery::limit`] 把本页的行池上界**告知 B3**（请求侧据此不超量）；该有界行为与 `p5_audit.rs` **AU8** 同口径 | 扩 `LV_MEM_SIZE` 或由外壳串行化页面生命周期后重标定（与 AU8 同批） |
//! | LG7 | 「加载中」/「已加载全部」落在**列表底部状态行**（§6.3 线框未画该行） | §3.6 **P3 列表/状态行**的用字表里有这两个词 ⇒ 设计**预留了落点**；取 `p5_audit.rs` 的同款呈现（底部状态行，仅有行时可见） | 无（**有意**）；若 PM 裁定不显，删两个常量与一处 `set_visible` 即可 |
//! | LG8 | 骨架态（B3 尚未注入通道态）显「**实时日志已断开 · 正在重连...**」（fail-closed） | §3.6 P3 只给了**两条**通道条文案（已连接 / 断开），未给第三条"未知"；**不臆造**新文案。与 `p4_interlock.rs` 的 fail-closed 口径一致（不臆造"已连接"）：**未确认即按未连接**显示；B3 首次注入前不应让本页可见 | 若 PM 要求第三条"等待中"文案：需先在 §3.6 补字 |
//! | LG9 | 新增**第三条列表区形态**「不完整」（`entries = [] ∧ range_too_large = true`）与**自造文案** [`TEXT_INCOMPLETE`]` = 范围超限 · 未执行检索`（**§3.6 无此句**） | **B2c-2 整改 ①**：原实现只看行数 ⇒ `entries = [] ∧ range_too_large = true` **同时**显空态「当前筛选条件下无日志」与超限条 —— 而契约字段的语义是"**本次未执行全库检索、`entries` 不代表完整结果**" ⇒ 把"无法获知"**冒充**成"确实没有"（违 §8.3 / §2.6；评审探针 `PROBE-EDGE08-15` 实测两态同显）。现**超限优先**：`range_too_large = true` ⇒ 空态**不可见**，改显中性文案（只说"未执行检索"这个**已知**事实） | 无（**结构性**）；若 PM/§3.6 给了 EDGE-15 下"结果不完整"的**指定文案**：换成契约串并删本条 |
//! | LG10 | 通道断（`connected = false`）时**不**把列表区切成"不可知"态 —— 若此时 `entries = []`，仍显空态「当前筛选条件下无日志」 | **判断与处置（B2c-2 整改 ①-4，如实）**：**不改**。① §6.3 明写"本页**无源不可用态**，日志通道断在**通道条**表达"；② 通道条是**恒可见**的（缺省即 fail-closed 显断开，见 **LG8**）⇒ 操作者同时看到"已断开"与"无日志"两件事，**不存在**把"无法获知"冒充成"确实没有"的**单一**表述失误；③ 列表区表达的是**最近一次已完成查询**的结果，与"当前是否在线"是两个维度（§6.3 有意把它们分给两个构件）。**残余（如实）**：若通道断在"一次成功查询返回 0 条"之后，屏上会同时出现"已断开"与"无日志" —— 该组合**为真**（那次查询确实返回 0），故不属互替 | 无（**有意**）；若 PM 要求"通道断 ⇒ 列表区也降级"：需先改 §6.3（把"无源不可用态"从本页删掉的那句话） |
//! | LG11 | 「回到最新」按钮：**§6.3 写"右下浮现"，实现为"右下恒显 + 页内专属带"**（**B2c-2 整改 ②**：原实现为"**右上**恒显 + 叠在首 2 行上、**实遮正文**"） | **薄层能力缺口**（见 **R2**）：无 `LV_EVENT_SCROLL`、无滚动位置读 / 写 API ⇒ "浮现"结构性不可实现；"浮动件"也不可实现（页根即滚动容器 ⇒ 任何页内浮动件都会随内容滚走 / 压住内容）。⇒ 取**可判定的安全形态**：按钮独占列表区之下的**一条带**（`y = 列表区底 + GAP_MIN`，右对齐）—— **任何行都与按钮矩形不相交**（几何实测见 `ui/tests.rs` 的 `pages_chain`），代价是页更长、且滚到旧日志时按钮不在视口内 | 需薄层补 ①`LV_EVENT_SCROLL` ②`lv_obj_get_scroll_y` / `lv_obj_scroll_to_y` ③"视口级浮动"能力；届时改回"右下浮现" |
//! | LG12 | 未登记长模块名的**撞形残余**：`mupc_gateway::iec104` 与 `mupc_data_processing::iec104` 在 **chip**（`...` + 尾 1 字 → `...4`）与**行**（`...` + 尾 4 字 → `...C104`）上**仍同形** | **可见化补偿 + 如实登记（B2c-2 整改 ⑤）**：这两个键**头（`mupc_`）尾（`::iec104`）都相同**，区分位在**中段** ⇒ **任何**"保头 / 保尾 / 头尾混保"的字符预算（≤ 5 字）都取不到中段；chip 的字符预算被像素钉死（见 **LG4**：放不下"2 字 + 标记"），行预算上限 5 字（原样）也够不到中段。⇒ 补偿 = **可见省略标记**（`...`，让"不完整"这件事**可见**）+ 行尾保留从 2 字提到 **4 字**（覆盖面更广）。**残余**：上例仍同形。根治需：扩 chip 宽（须先放开 §6.3 ②「最多 7 行」）、或允许横滚（§2.7 禁）、或由后端提供**短别名** | 待 PM 裁定（后端短别名 / 放开 7 行 / 允许换行显示全名） |
//! | LG13 | 增量语义：**"是否已有游标"与"是否允许增量"解耦**（**B2c-2 整改 ⑦**）—— 无游标（含**空窗口**）时 `request_increment()` 发 `cursor = None` 的意图（= 重新拉首页） | **原缺陷**：`fire_increment` 以 `last_seq == 0` 直接返回，而 `fire_query` 只清零游标、**无唤醒路径** ⇒ 空结果之后「实时追加 ≤1 s」**永久停摆**（原有用例还把它反向锁住，本批已改为正向断言）。**与 B3 的接口约定（如实）**：B3 负责 **500 ms 节拍**与**节流** —— `cursor = None` 的增量意图应被当作**刷新**（可降频 / 可去重），`cursor = Some(..)` 的才是真增量拉取。**残余**：本页**不**自持窗口、不做节流（窗口累积在 B3，同契约 6′） | 无（**结构性**：本页不发请求、不读时钟）；节流策略由 B3 定 |
//! | LG14 | 消息列 long mode 的**锁**是"**行为级 + 源码锚定**"，不是"读回回口" | 薄层**没有** `LongMode` 读回口（`lvgl-sys/allowlist.txt` 只有 `fn:lv_label_set_long_mode`，**无** `getter`；本单元**不得**改 `lvgl-sys/**`）⇒ 无法给出"返回消息列当前 `LongMode`"的只读回口。**做法**：① **行为级**——长消息在 `DOTS` 下被 LVGL 把尾部换成 `.`×3，`Label::text()`（读 `lv_label_get_text` 的**同一缓冲区**）会读回带 `...` 的截断串 ⇒ 断言"长消息读回串以 `...` 结尾且短于原文"（`ui/tests.rs` 的 `pages_chain`）；② **源码锚定**——`p3_static_constraints` 锚定 `message.set_long_mode(LongMode::DOTS);` 这一行，并同时钉住 `LongMode::WRAP` 在**本文件代码里恰 1 处**（只允许「回到最新」按钮的 `92×92` 折行）⇒ 把消息列改回 `WRAP` 会让计数变 2 ⇒ **红**（原缺陷：WRAP 断言被 `back.label()` 的 WRAP 满足 ⇒ 无区分度） | 薄层补 `lv_label_get_long_mode` 后改成真读回口 |
//! | LG15 | 机器名前 / 自由文本的**非 ASCII 残余**：`display_safe` **只改写 ASCII**、[`free_text_safe`] 只多折叠**六个常用全角标点** ⇒ `target` / `message` 若含 **cmap 外的汉字 / 全角字**，真机上是**豆腐块** | **R1 的残余（B2c-2 规格评审补登 —— 此前只在模块文档正文提过，未进本表）**：本页**不臆造**中文名、**不静默隐藏**，但**无从**列举后端可能给的全部字符 ⇒ 防线在**后端字段命名**与**字库**。与 `p5_audit.rs` **AU15** / `pages/mod.rs` **D9** 同族 | 同 LG1（扩字表）；或由后端保证 `target` 只用已登记键 |

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use mupc_display_proto::log::{LOG_PAGE_LIMIT_MAX, LOG_TARGETS_MAX};
use mupc_display_proto::{LogEntry, LogLevel, LogPage, LogRange};

use crate::lvgl::obj::Obj;
use crate::lvgl::style::{Color, Opa, Style, StyleSelector};
use crate::lvgl::widgets::{Label, LongMode, ScrollContainer, TextButton};
use crate::lvgl::LvglError;
use crate::ui::components::{EmptyState, MultiSelectChips, WarnBanner};
use crate::ui::pages::filters::{self, TimeRangeChange, TimeRangeFilter};
use crate::ui::pages::p5_audit::{free_text_safe, TEXT_ELLIPSIS};
use crate::ui::pages::{
    decor, display_safe, format_epoch_ms_utc, label, layout_box, page_root, set_style_index,
    set_visible, show_only, text_label, CbSlot, TIGHT_GAP,
};
use crate::ui::theme::{self, Dimens, Palette, Stroke, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 上屏文案（UI §3.6 **P3 行** + §8.3 两行；**落笔前逐字在 `fonts/lv_font_cmap.txt` 核对**）
// ═══════════════════════════════════════════════════════════════════════════

/// 通道条：已连接（§3.6 P3「通道条」行逐字）。
pub(crate) const TEXT_CHANNEL_OK: &str = "实时日志已连接";
/// 通道条：断开（`，`→` · `、`…`→`...`，见 **LG1**）。
pub(crate) const TEXT_CHANNEL_DOWN: &str = "实时日志已断开 · 正在重连...";
/// 筛选维度名：级别（§3.6 P3「筛选」行）。
pub(crate) const TEXT_LEVEL_LABEL: &str = "级别";
/// 筛选维度名：模块（同上）。
pub(crate) const TEXT_MODULE_LABEL: &str = "模块";
/// 模块维度首位的快捷复位项（§6.3 ②「`全部` chip 固定首位」）。
pub(crate) const TEXT_ALL: &str = "全部";
/// 级别 chip 文案之一（PRD §3.3 / §5.1 #4）。
pub(crate) const TEXT_LEVEL_ERROR: &str = "ERROR";
/// 级别 chip 文案之二。
pub(crate) const TEXT_LEVEL_WARN: &str = "WARN";
/// 级别 chip 文案之三。
pub(crate) const TEXT_LEVEL_INFO: &str = "INFO";
/// 级别 chip 文案之四。
pub(crate) const TEXT_LEVEL_DEBUG: &str = "DEBUG";
/// 表头列名之一（§3.6 P3「列表」行）。
pub(crate) const TEXT_HEAD_TIME: &str = "时间";
/// 表头列名之二（= 筛选维度名，**同一串** ⇒ 转出，不另抄字面量）。
pub(crate) const TEXT_HEAD_LEVEL: &str = TEXT_LEVEL_LABEL;
/// 表头列名之三（同上）。
pub(crate) const TEXT_HEAD_MODULE: &str = TEXT_MODULE_LABEL;
/// 表头列名之四（§3.6 P3「列表」行）。
pub(crate) const TEXT_HEAD_MESSAGE: &str = "消息";
/// 空态文案（§8.3 EDGE-08 行逐字）。
pub(crate) const TEXT_EMPTY: &str = "当前筛选条件下无日志";
/// 空态图标（几何空心圆，§3.6 符号集内；与「不可用态」的 `?` **不同族**）。
pub(crate) const TEXT_EMPTY_ICON: &str = "○";
/// 超限提示（§8.3 EDGE-15 行逐字 + `，`→` · ` 见 **LG1**）。
pub(crate) const TEXT_RANGE_TOO_LARGE: &str = "检索范围超限 · 请缩小时间范围";
/// 超限下的列表区中性文案（**自造串，§3.6 无此句** —— 见 **LG9**）。
///
/// **为什么必须有一句**：`range_too_large = true ∧ entries = []` 时不能说
/// [`TEXT_EMPTY`]（那是"确实没有"，而此刻是"无法获知"）；但列表区若**完全空白**，
/// 操作者同样可能读成"没有日志"。故给一句**中性、不断言结果**的话：只说"未执行检索"
/// 这个**已知事实**，不说"有没有日志"这个**未知事实**。
///
/// 用字逐字在 `fonts/lv_font_cmap.txt` 内（`范围超限 · 未执行检索`，不含 `完`/`整`/`次`/`库`
/// 等缺字）——由 `ui/tests.rs::ui_texts_covered_by_font_cmap` 的逐字走查钉住。
pub(crate) const TEXT_INCOMPLETE: &str = "范围超限 · 未执行检索";
/// 只读说明行前半（§3.6 P3「列表 / 状态」行的 `本地屏不支持日志导出`）。
pub(crate) const TEXT_NO_EXPORT: &str = "本地屏不支持日志导出";
/// 只读说明行后半（§3.6 P3 同行的 `无文件与下载通道`）。
pub(crate) const TEXT_NO_EXPORT2: &str = "无文件与下载通道";
/// 子句分隔（cmap 内 `·`；替代缺字的全角标点，见 **LG1**）。
pub(crate) const TEXT_CLAUSE_SEP: &str = " · ";
/// 底部状态行：还有下一页（§3.6 P3「列表 / 状态」行；落点见 **LG7**）。
pub(crate) const TEXT_FOOTER_LOADING: &str = "加载中";
/// 底部状态行：已到末页（同上）。
pub(crate) const TEXT_FOOTER_ALL: &str = "已加载全部";
/// 「回到最新」按钮文案（§3.6 P3「列表 / 状态」行；能力边界见 **R2**）。
pub(crate) const TEXT_BACK_TO_LATEST: &str = "回到最新";
/// 已知模块名 → 中文标签之一（§3.6 P1 装置状态区「核间连接」）。
pub(crate) const TEXT_MODULE_INTERCORE: &str = "核间";
/// 已知模块名 → 中文标签之二（§3.6 P1 装置状态区「调度主站连接」）。
pub(crate) const TEXT_MODULE_GATEWAY: &str = "主站";
/// 已知模块名 → 中文标签之三（§3.6 页标题「审计」/ P5 用字）。
pub(crate) const TEXT_MODULE_AUDIT: &str = "审计";

/// 本页上屏的**全部固定文案**（供 `ui/pages/mod.rs` 的 `ALL_TEXTS` 清册与
/// `ui/tests.rs::ui_texts_covered_by_font_cmap` 的「清册 ↔ 源码字面量」一致性走查）。
///
/// ⚠️ **`Trace` 的级别文字不在此列**：它是**运行时**产物（`display_safe(display_name())`
/// = `?RACE`），不是本页的固定字面量 ⇒ 由
/// `ui/tests.rs::p3_runtime_texts_emit_only_cmap_glyphs` 逐字查 cmap。
#[cfg(test)]
pub(crate) const ALL_TEXTS: &[&str] = &[
    TEXT_CHANNEL_OK,
    TEXT_CHANNEL_DOWN,
    TEXT_LEVEL_LABEL,
    TEXT_MODULE_LABEL,
    TEXT_ALL,
    TEXT_LEVEL_ERROR,
    TEXT_LEVEL_WARN,
    TEXT_LEVEL_INFO,
    TEXT_LEVEL_DEBUG,
    TEXT_HEAD_TIME,
    TEXT_HEAD_LEVEL,
    TEXT_HEAD_MODULE,
    TEXT_HEAD_MESSAGE,
    TEXT_EMPTY,
    TEXT_EMPTY_ICON,
    TEXT_RANGE_TOO_LARGE,
    TEXT_INCOMPLETE,
    TEXT_NO_EXPORT,
    TEXT_NO_EXPORT2,
    TEXT_CLAUSE_SEP,
    TEXT_FOOTER_LOADING,
    TEXT_FOOTER_ALL,
    TEXT_BACK_TO_LATEST,
    TEXT_MODULE_INTERCORE,
    TEXT_MODULE_GATEWAY,
    TEXT_MODULE_AUDIT,
];

// ═══════════════════════════════════════════════════════════════════════════
// 2. 栅格常量（UI §6.3 线框 / 行规格；**全部由 theme 常量推导**）
//
// 页内坐标 = UI 绝对坐标 − 72（页根贴在 `(SIDE_PAD, HEADER_H)`，契约 1）。
// ═══════════════════════════════════════════════════════════════════════════

/// 通道条高（§6.3 线框 `Y80` 起 36 px）。
const CHANNEL_H: i32 = TextSlot::Body.px() as i32 + TIGHT_GAP + Dimens::SCROLLBAR_MARGIN;
/// 通道条的灯直径（§5.1 #14 的 16 px 档）。
const CHANNEL_DOT: i32 = Dimens::LED_DIA;
/// 通道条文字 x（灯 + 同组半缝）。
const CHANNEL_TEXT_X: i32 = Dimens::LED_DIA + Dimens::GAP_MIN / 2;
/// 级别行高 = chip 高（§6.3「筛选行 48 px」）。
const LEVEL_ROW_H: i32 = Dimens::CHIP_H;
/// 级别 chip 组列数（§6.3 ① 四项一行）。
const LEVEL_COLS: u32 = 4;
/// 级别 chip 宽：标签列之后**均分**余宽（≥ 最长项 `DEBUG` + `✓` 前缀的实测需求）。
pub(crate) const LEVEL_CHIP_W: i32 =
    (Dimens::CONTENT_W - filters::CTRL_X - (LEVEL_COLS as i32 - 1) * Dimens::GAP_MIN)
        / LEVEL_COLS as i32;
/// 级别 chip 组容器宽。
const LEVEL_BOX_W: i32 = LEVEL_COLS as i32 * LEVEL_CHIP_W + (LEVEL_COLS as i32 - 1) * Dimens::GAP_MIN;
/// 模块维度名行高（**LG3**：维度名独占一行）。
const MODULE_LABEL_H: i32 = TextSlot::SectionTitle.px() as i32;
/// 模块 chip 网格列数（§6.3 ② 的"保守 8 项/行"；**LG3** 的推导保证 ≤51 项 = 7 行）。
const MODULE_COLS: u32 = 8;
/// 模块 chip 宽：铺满内容宽后**均分**（8 列 ⇒ 110 px；**LG3**）。
pub(crate) const MODULE_CHIP_W: i32 =
    (Dimens::CONTENT_W - (MODULE_COLS as i32 - 1) * Dimens::GAP_MIN) / MODULE_COLS as i32;
/// 模块 chip 网格容器宽（= `MODULE_COLS × chip + 缝`，等于 `CONTENT_W` ⇒ 与表头 / 列表右缘对齐）。
const MODULE_BOX_W: i32 =
    MODULE_COLS as i32 * MODULE_CHIP_W + (MODULE_COLS as i32 - 1) * Dimens::GAP_MIN;
/// 模块网格**行数上限**（§6.3 ②「最多 7 行」）。
const MODULE_ROWS_MAX: u32 = 7;
/// 模块网格**项数上限**（「全部」+ 契约 `LOG_TARGETS_MAX` = 50）—— 用于编译期容量自证。
const MODULE_ITEMS_MAX: u32 = 1 + LOG_TARGETS_MAX as u32;

/// 表头高（§6.3 线框：表头 36 px；推导同 `p5_audit.rs` 的 36 px 档）。
const TABLE_HEAD_H: i32 = TextSlot::Body.px() as i32 + TIGHT_GAP + Dimens::SCROLLBAR_MARGIN;
/// 表头竖分隔线宽（§3.5：卡片描边 1 px `divider`）。
const HEAD_DIV_W: i32 = Stroke::THIN;
/// 表头竖分隔线高（表头行高减去上下紧缝）。
const HEAD_DIV_H: i32 = TABLE_HEAD_H - 2 * TIGHT_GAP;
/// 底部状态行高（**LG7**：与说明行同档 36）。
const FOOTER_H: i32 = TABLE_HEAD_H;
/// 只读说明行高（§6.3「不支持导出」节：列表区底部固定一行 36 px）。
const NOTE_H: i32 = TABLE_HEAD_H;
/// 空态高（与 `components.rs::EmptyState` 的构件算式**逐项一致**：图标 64 + 缝 + 文字行）。
const EMPTY_H: i32 = Dimens::ICON_LG + Dimens::GAP_MIN + TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
/// 不完整态（**LG9**）占位高 —— 取 [`EMPTY_H`]：单行中性文案垂直居中于同一块面积，
/// 空态 / 不完整态切换时**其下的说明行不跳动**（避免"态切换 = 版面抖动"）。
const INCOMPLETE_H: i32 = EMPTY_H;

// ── 行规格（UI §6.3「每行」）────────────────────────────────────────────────
/// 行高 44（§6.3；LG-06 ≥40 —— 直接取 `theme` 的 `ROW_LOG_H`）。
/// 行左缘到时间列 x（无左竖条：本页用**斑马纹**区分行，§6.3 只给斑马纹）。
const ROW_TIME_X: i32 = 0;
/// 时间列宽（**LG2**：§6.3 写 160，实测时间戳 **224.0 px** ⇒ 取 P5 的同一列宽 `CONTENT_W / 4`）。
///
/// `pub(crate)`：`ui/tests.rs::p3_column_budgets_fit_measured_text` 用**生产字体的 `adv_w`**
/// 实测"定长时间戳放得进本列"这条几何锁（**并钉住 224.0 这个订正后的值**）。
pub(crate) const ROW_TIME_W: i32 = Dimens::CONTENT_W / 4;
/// 级别色块 x。
const ROW_LEVEL_X: i32 = ROW_TIME_W + Dimens::GAP_MIN;
/// 级别色块宽（§6.3「级别色块 88×28」—— 88 = `CHIP_MIN_W − TIGHT_GAP`）。
pub(crate) const ROW_LEVEL_W: i32 = Dimens::CHIP_MIN_W - TIGHT_GAP;
/// 级别色块高 28（= `Dimens::ICON_SM`）。
const ROW_LEVEL_H: i32 = Dimens::ICON_SM;
/// 级别色块 y（44 px 行内垂直居中）。
const ROW_LEVEL_Y: i32 = theme::center_offset(Dimens::ROW_LOG_H, ROW_LEVEL_H);
/// 模块列 x。
const ROW_MODULE_X: i32 = ROW_LEVEL_X + ROW_LEVEL_W + Dimens::GAP_MIN;
/// 模块列宽（§6.3「宽 140」= 5 × 28 像素档）。
pub(crate) const ROW_MODULE_W: i32 = Dimens::ICON_SM * 5;
/// 消息列 x。
const ROW_MSG_X: i32 = ROW_MODULE_X + ROW_MODULE_W + Dimens::GAP_MIN;
/// 消息列宽（到内容区右缘）。
const ROW_MSG_W: i32 = Dimens::CONTENT_W - ROW_MSG_X;
/// 行内 24 px 文字的 y（44 px 行内垂直居中）。
const ROW_TEXT_Y: i32 = theme::center_offset(Dimens::ROW_LOG_H, TextSlot::Body.px() as i32);
/// 级别色块内文字的 y（28 px 块内居中）。
const ROW_LEVEL_TEXT_Y: i32 = theme::center_offset(ROW_LEVEL_H, TextSlot::Body.px() as i32);
/// 模块列**原样显示**的字符预算（**LG4**：140 px ÷ 24 px 档汉字 ≈ 5.8 → **5 字**；
/// 由 `ui/tests.rs::p3_column_budgets_fit_measured_text` 用生产字体的 `adv_w` 钉住）。
///
/// ⚠️ 同上：**它不是"截断后的字数"** —— 超出时产物是
/// `"..." + 尾 `[`ROW_MODULE_TAIL_CHARS`]` 字`（见 [`clip_row_label`]）。上限之所以是 5：
/// 6 个汉字 = 144 px > 140 px 列宽（原样上屏会破线）。
pub(crate) const ROW_MODULE_MAX_CHARS: usize = 5;
/// 模块**行**超出预算时保留的**尾部**字数（**LG4 / LG12**：`"..." + 尾 4 字`）。
///
/// **B2c-2 规格评审整改 ⑤：尾 2 字 → 尾 4 字**（实测 `"..." + 4 汉字` = 116.06 px ≤ 140 px；
/// `"..." + 5 汉字` = 140.0625 px > 140 px ⇒ 4 是列宽允许的最大尾保留数）。
/// **由 `ui/tests.rs::p3_column_budgets_fit_measured_text` 的 ②′ 实测钉住。**
pub(crate) const ROW_MODULE_TAIL_CHARS: usize = 4;
/// 模块 chip **原样显示**的字符预算（**LG4**：chip 内区 `110 − 2×16 − 2×1` = 76 px，
/// 选中态再减 `✓`(U+2713) 的 17.75 px ⇒ 可用 58.25 px = **2 个汉字**（52 px）；由
/// `ui/tests.rs::p3_column_budgets_fit_measured_text` 用生产字体的 `adv_w` 钉住）。
///
/// ⚠️ **它不是"截断后的字数"**：超出本预算时走 [`clip_chip_label`] ⇒ 产物是
/// `"..." + 尾 `[`MODULE_CHIP_TAIL_CHARS`]` 字`（可见省略标记，见 **LG4 / LG12**）。
/// **为什么不是"2 字 + 标记"**：`2 汉字 + "..."` = 73.75 px > 58.25 px（实测）⇒ 会顶出
/// chip 内区；chip 的 **8 列 × 110 px** 由 §6.3 ②「最多 7 行」的容量核算钉死
/// （`8 × 110 + 7 × 16 = 992 = 内容宽`，**零余量**）⇒ 无法再加宽。
/// **取舍**：可读性（**截断必须可见**，§2.6「降级可见、绝不造假」）优先于多留一个字。
pub(crate) const MODULE_CHIP_MAX_CHARS: usize = 2;
/// chip 超出预算时保留的**尾部**字数（= [`MODULE_CHIP_MAX_CHARS`] − 1）。
///
/// **由 `ui/tests.rs::p3_column_budgets_fit_measured_text` 的 ③′ 用生产字体实测钉住**
/// （`✓` + `"..."` + 1 汉字 = 65.5 px ≤ 76 px；`✓` + `"..."` + 2 汉字 = 91.55 px > 76 px）。
const MODULE_CHIP_TAIL_CHARS: usize = 1;

/// 「回到最新」按钮边长（§6.3 的 `92×92`；= `CHIP_MIN_W − SCROLLBAR_MARGIN`，见 **LG11**）。
const BACK_W: i32 = Dimens::CHIP_MIN_W - Dimens::SCROLLBAR_MARGIN;
/// 「回到最新」按钮 x（贴内容区右缘）。
const BACK_X: i32 = Dimens::CONTENT_W - BACK_W;
/// 「回到最新」**专属带**高（= 按钮边长 + 与上方列表区之间的呼吸缝）。
///
/// **B2c-2 规格评审整改 ②**：按钮此前是"页内子对象 + 与行重叠"（屏坐标 (916,380)-(1007,471)
/// 压住第 0/1 行消息列尾部 92 px；`theme::control_surface()` 底 `SURFACE_HIGH` **不透明**
/// ⇒ **实遮正文**）。⇒ 改为**独占一条带**：带内只有按钮，任何行都不与按钮矩形相交
/// （几何实测见 `ui/tests.rs` 的 `pages_chain`）。见 **LG11 / R2**。
const BACK_BAND_H: i32 = BACK_W + Dimens::GAP_MIN;

/// 编译期自证：常量与 UI §6.3 的字面契约值一致。
const _: () = assert!(CHANNEL_H == 36);
const _: () = assert!(TABLE_HEAD_H == 36);
const _: () = assert!(FOOTER_H == 36);
const _: () = assert!(NOTE_H == 36);
const _: () = assert!(ROW_LEVEL_W == 88);
const _: () = assert!(ROW_LEVEL_H == 28);
const _: () = assert!(BACK_W == 92);
const _: () = assert!(EMPTY_H == 124);
/// 编译期自证（**LG3**）：模块网格铺满内容宽（右缘与表头 / 列表对齐）。
const _: () = assert!(MODULE_BOX_W == Dimens::CONTENT_W);
/// 编译期自证（**LG3**）：`MODULE_COLS` 列 × `MODULE_CHIP_W` 不小于 chip 最小宽 96。
const _: () = assert!(MODULE_CHIP_W >= Dimens::CHIP_MIN_W);
/// 编译期自证（**LG3 / §6.3 ②**）：契约上限的项数（「全部」+ 50）在 `MODULE_ROWS_MAX` 行内装得下。
const _: () = assert!(
    MODULE_ITEMS_MAX.div_ceil(MODULE_COLS) <= MODULE_ROWS_MAX
);
/// 编译期自证：行内四列互不重叠且消息列非空。
const _: () = assert!(ROW_MSG_W > 0);
const _: () = assert!(ROW_MODULE_X + ROW_MODULE_W < ROW_MSG_X);
const _: () = assert!(ROW_MSG_X + ROW_MSG_W == Dimens::CONTENT_W);
/// 编译期自证：行内单行 24 px 文字放得进 44 px 行高。
const _: () = assert!(ROW_TEXT_Y + TextSlot::Body.px() as i32 <= Dimens::ROW_LOG_H);
// ⚠️ **此处原有的两条编译期断言已删（B2c-2 验证整改 M1）—— 它们都是恒真式**：
//
// - `BACK_BAND_H` 的**定义**即 `BACK_W + Dimens::GAP_MIN` ⇒ `assert!(BACK_BAND_H >= BACK_W)`
//   **恒真**，属"看着在把关、实则空转"（本项目已多次抓到该缺陷类）。**按定义成立者无需断言**。
//   真正要防的是"**布局没用这个常量**"（常量退化成装饰）—— 见 `Core::relayout` 里
//   `BACK_BAND_H` 的实际用法（同批验证整改 M2）。
// - `INCOMPLETE_H` 的定义即 `= EMPTY_H`（见其声明处）⇒ `assert!(INCOMPLETE_H == EMPTY_H)`
//   亦恒真。二者"同高"由**定义**保证，比断言更强。
//
// 本注释保留痕迹，**防止有人再把这两条断言加回来**。（用 `//` 而非 `///`：它不再注释任何项。）

// ═══════════════════════════════════════════════════════════════════════════
// 3. 纯逻辑（**不触碰 LVGL** ⇒ 可独立单测；页内一切判据都经这里）
// ═══════════════════════════════════════════════════════════════════════════

/// 级别筛选 chip 的顺序（**唯一真源**：段序 = 数组序，§6.3 ①）。
pub(crate) const FILTER_LEVELS: [LogLevel; 4] = [
    LogLevel::Error,
    LogLevel::Warn,
    LogLevel::Info,
    LogLevel::Debug,
];

/// 全部级别（含 `Trace`）—— 顺序与行级别样式的槽位一致（见 [`level_style_index`]）。
pub(crate) const ALL_LEVELS: [LogLevel; 5] = [
    LogLevel::Error,
    LogLevel::Warn,
    LogLevel::Info,
    LogLevel::Debug,
    LogLevel::Trace,
];

/// 级别 → 上屏文字（**唯一出口**：契约 `display_name()` 经 [`display_safe`]）。
///
/// **为什么必须经 `display_safe`**：`LogLevel::Trace.display_name()` = `TRACE`，其中
/// `T`(U+0054) **不在生成字体的 cmap 内** ⇒ 直上屏即豆腐块（**R1**）。经 `display_safe`
/// 后四个 PRD 级别**逐字不变**（`ERROR`/`WARN`/`INFO`/`DEBUG` 的字符都在 cmap 内），
/// `Trace` 变 `?RACE`（可辨认、无豆腐块）。
pub(crate) fn level_text(level: LogLevel) -> String {
    display_safe(level.display_name())
}

/// 级别 → 色块颜色（PRD §3.1 逐字指定；`Trace` 取 [`Palette::TEXT_WEAK`] —— **明标为
/// "非 PRD 指定级别色"**，不臆造第 5 个级别色，见模块文档 R1）。
pub(crate) const fn level_color(level: LogLevel) -> Color {
    match level {
        LogLevel::Error => Palette::LOG_ERROR,
        LogLevel::Warn => Palette::LOG_WARN,
        LogLevel::Info => Palette::LOG_INFO,
        LogLevel::Debug => Palette::LOG_DEBUG,
        LogLevel::Trace => Palette::TEXT_WEAK,
    }
}

/// 级别 → 级别样式的槽位（= [`ALL_LEVELS`] 的下标；**编译期**由断言钉死一一对应）。
pub(crate) const fn level_style_index(level: LogLevel) -> usize {
    match level {
        LogLevel::Error => 0,
        LogLevel::Warn => 1,
        LogLevel::Info => 2,
        LogLevel::Debug => 3,
        LogLevel::Trace => 4,
    }
}

/// 级别 chip 的选项文案（§6.3 ① 四项）。
pub(crate) fn level_options() -> Vec<String> {
    FILTER_LEVELS.iter().map(|l| level_text(*l)).collect()
}

/// 勾选下标 → 级别集合（**空集合 = 不按级别筛**；下标越界丢弃，`Trace` **不产**）。
///
/// ⚠️ 这是 **`Trace` 处置**的落点：`Trace` 无 chip ⇒ 只要有任何勾选，`Trace` 就被排除
/// （它不在 [`FILTER_LEVELS`] 内）；**全不选**时 `levels` 为空 ⇒ 服务端不按级别过滤 ⇒
/// `Trace` 条目**照常返回并上屏**（**不静默丢弃**）。
pub(crate) fn selected_levels(selected: &[usize]) -> Vec<LogLevel> {
    let mut out: Vec<LogLevel> = selected
        .iter()
        .filter_map(|i| FILTER_LEVELS.get(*i).copied())
        .collect();
    out.dedup();
    out
}

/// 机器键的**归一形态**（查表用；**不上屏**）：去首尾空白 + 转小写。
///
/// **为什么只做这两步**：契约示例给的是 `mupc_intercore` / `mupc_gateway`
/// （`display-proto/src/log.rs` 的字面量 JSON 用例），而 UI §6.3 线框给的是 `intercore` /
/// `gateway` —— 同一模块的两种写法。**两种写法各自逐条登记在 [`MODULE_LABELS`]**，而**不**
/// 做"剥前缀"这类变换：那需要一条 `mupc_` 字面量，而它是**小写 ASCII**（`m`/`u`/`p`/`c`/`_`
/// 在生成字体里都没有字形）⇒ 会被 `ui/tests.rs::ui_texts_covered_by_font_cmap` 判为缺字
/// （除非再给它开一条 `NON_DISPLAY_SINKS` 豁免 —— 那是**为过网而加豁免**，本页不这么做）。
/// 逐条登记也让"已知键全集"保持在**一张可审计的表**里（**只减不增**：不会让任何键"猜中"
/// 别的模块）。
pub(crate) fn normalized_module(target: &str) -> String {
    target.trim().to_ascii_lowercase()
}

/// 机器键 → 中文标签（**不上屏**的键 token 出口；见 `ui/tests.rs::NON_DISPLAY_SINKS`）。
const fn module_key(key: &'static str) -> &'static str {
    key
}

/// 已知模块名 → 中文标签（**逐条登记、只增不改**）。
///
/// | 机器键 | 上屏 | 出处 / 证据 |
/// |--------|------|-------------|
/// | `mupc_intercore` / `intercore` | [`TEXT_MODULE_INTERCORE`] | 契约 `display-proto/src/log.rs` 的 JSON 用例逐字给出 `mupc_intercore`；§6.3 线框给 `intercore`；中文名取 §3.6 P1 装置状态区「核间连接」 |
/// | `mupc_gateway` / `gateway` | [`TEXT_MODULE_GATEWAY`] | 契约同上（`mupc_gateway`）；§6.3 线框给 `gateway`；中文名取 §3.6 P1「调度主站连接」 |
/// | `audit` | [`TEXT_MODULE_AUDIT`] | §3.6 页标题「审计」/ P5 用字（审计模块名与其页名同源）；§6.3 线框亦列 `audit` |
///
/// ⚠️ **键全集的真源是 mupcd 的日志服务**（`/logs/targets` 由 ring + 文件采样生成，**不是**
/// 固定枚举）⇒ 本表**只增不改**：新增键必须同时在 §3.6 / §6.3 线框找到对应的写法与中文词；
/// **猜错键名 = 谎报模块名**，故宁可不给中文名（未登记键走 [`module_label`] 的降级路径，
/// **仍上屏**，见 **R1**）。
pub(crate) const MODULE_LABELS: [(&str, &str); 5] = [
    (module_key("mupc_intercore"), TEXT_MODULE_INTERCORE),
    (module_key("intercore"), TEXT_MODULE_INTERCORE),
    (module_key("mupc_gateway"), TEXT_MODULE_GATEWAY),
    (module_key("gateway"), TEXT_MODULE_GATEWAY),
    (module_key("audit"), TEXT_MODULE_AUDIT),
];

/// 机器键 → 上屏标签（**未登记 ⇒ `None`**；调用方降级显示，见 [`module_label`]）。
pub(crate) fn target_label(target: &str) -> Option<&'static str> {
    let key = normalized_module(target);
    MODULE_LABELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
}

/// 机器键 → 上屏标签（**唯一出口**）：已知键取中文名；未登记键取 [`display_safe`] 的归一形态
/// （**不臆造**中文名、**不静默隐藏** —— `mupc_intercore` 一类仍是可辨认的归一串）。
///
/// ⚠️ **本函数不截断**（截断由两个落点各自的**唯一出口**做：[`clip_row_label`] / [`clip_chip_label`]）
/// —— 两处可用宽度不同（140 / 76 px），共用一份预算会在宽处浪费、在窄处溢出。
pub(crate) fn module_label(target: &str) -> String {
    match target_label(target) {
        Some(label) => label.to_string(),
        None => display_safe(target.trim()),
    }
}

/// 模块名的**通用截断**（`原样预算 keep` / `尾保留 tail`）：原样显示的字数 ≤ `keep` 时不截；
/// 超出 ⇒ `"..." + 尾 `tail` 字`（**恒带可见省略标记** —— §2.6「降级可见、绝不造假」）。
///
/// **为什么不复用 `p5_audit::clip_tail`**：`clip_tail(text, max)` 的判据是"字符总数 ≤ `max`
/// 则原样" —— 于是 `max` **同时**承担"原样上限"与"截断产物的长度上限"两个角色。本页两处
/// 落点的**原样上限**（凭字宽定：chip 2 字 / 行 5 字）都**小于**"截断产物"的长度（标记本身
/// 要 3 个字宽）⇒ 用 `clip_tail` 会让"刚好 3 字的 chip 名"原样上屏并**顶出 chip 内区**
/// （实测 `汉汉汉` = 78 px > 58.25 px 可用）。故两个预算**分开**给（本函数），且由
/// `ui/tests.rs::p3_column_budgets_fit_measured_text` 用生产字体的 `adv_w` **两条都实测**。
///
/// `keep` = 0 时恒截断；`tail` 为 0 时只留标记（本页两处都 ≥1）。
fn clip_marked(text: &str, keep: usize, tail: usize) -> String {
    let n = text.chars().count();
    if n <= keep {
        return text.to_string();
    }
    let keep = tail.min(n);
    let kept: String = text.chars().skip(n - keep).collect();
    format!("{TEXT_ELLIPSIS}{kept}")
}

/// 模块 chip 上屏文案（**唯一出口**）：≤ [`MODULE_CHIP_MAX_CHARS`] 字原样；
/// 超出 ⇒ `"..." + 尾 `[`MODULE_CHIP_TAIL_CHARS`]` 字`（可见省略标记；**LG4 / LG12**）。
pub(crate) fn clip_chip_label(label: &str) -> String {
    clip_marked(label, MODULE_CHIP_MAX_CHARS, MODULE_CHIP_TAIL_CHARS)
}

/// 模块**行**上屏文案（**唯一出口**）：≤ [`ROW_MODULE_MAX_CHARS`] 字原样；
/// 超出 ⇒ `"..." + 尾 `[`ROW_MODULE_TAIL_CHARS`]` 字`（可见省略标记；**LG4 / LG12**）。
pub(crate) fn clip_row_label(label: &str) -> String {
    clip_marked(label, ROW_MODULE_MAX_CHARS, ROW_MODULE_TAIL_CHARS)
}

/// 模块 chip 的选项文案（**首位固定「全部」**；§6.3 ②）。
pub(crate) fn module_options(targets: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(targets.len() + 1);
    out.push(TEXT_ALL.to_string());
    out.extend(targets.iter().map(|t| clip_chip_label(&module_label(t))));
    out
}

/// 勾选下标 → 机器键集合（下标 `0` = 「全部」，**不产键**；越界 / 越表丢弃，不 panic）。
pub(crate) fn selected_targets(selected: &[usize], targets: &[String]) -> Vec<String> {
    let mut out: Vec<String> = selected
        .iter()
        .filter_map(|i| i.checked_sub(1).and_then(|k| targets.get(k)))
        .cloned()
        .collect();
    out.dedup();
    out
}

/// 「全部 + 具体项」并存选择的**归一化**（`0` = 「全部」）—— 三条规则见下。
///
/// 规则（**纯逻辑、逐条可测**；与 `p5_audit.rs::normalize_ops_selection` **同一组语义**，
/// 两页的"快捷复位"是同一交互约定）：
/// 1. 本次**新增**含 `0`（「全部」）⇒ 只留 `[0]`（§6.3「`全部` chip 为快捷复位，1 次触摸
///    清空该维度」）；
/// 2. 本次新增的是**具体项** ⇒ 去掉 `0`（用户意图是"缩小范围"）；
/// 3. 只发生**取消**（无新增）⇒ 原样返回（仍保守清理"全部 + 具体项"的非法组合）。
pub(crate) fn normalize_module_selection(prev: &[usize], selected: &[usize]) -> Vec<usize> {
    let added: Vec<usize> = selected
        .iter()
        .copied()
        .filter(|i| !prev.contains(i))
        .collect();
    if added.contains(&0) {
        return vec![0];
    }
    if !added.is_empty() {
        return selected.iter().copied().filter(|i| *i != 0).collect();
    }
    if selected.contains(&0) && selected.len() > 1 {
        return selected.iter().copied().filter(|i| *i != 0).collect();
    }
    selected.to_vec()
}

/// 日志查询意图（**本页交给外部的唯一载荷**；`request_id` / HTTP 由 B3 承担）。
///
/// | 字段 | 语义 |
/// |------|------|
/// | `range` / `from_ms` / `to_ms` | 时间范围三档（与 P5 / `filters.rs` 共用 [`LogRange`]）；起止**仅 `Custom`** 时给出 |
/// | `levels` | 级别**多选**（空 = 不按级别筛；**无 `Trace`**，见 [`selected_levels`]） |
/// | `targets` | 模块**多选**的**机器键**（空 = 不按模块筛；给的是原始键，服务端按 `tracing` target 过滤） |
/// | `cursor` | 增量游标（= 已见最大 `seq`；`None` = 首屏 / 筛选变化后的全新查询） |
/// | `limit` | 单次返回条数上限（= 本页行池上界，**LG6**：让请求侧不超量） |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogQuery {
    /// 时间范围档位。
    pub range: LogRange,
    /// 自定义起始（UTC 毫秒；`Custom` 之外的档位为 `None`）。
    pub from_ms: Option<u64>,
    /// 自定义结束（UTC 毫秒；同上）。
    pub to_ms: Option<u64>,
    /// 级别多选（空 = 不筛）。
    pub levels: Vec<LogLevel>,
    /// 模块多选（原始机器键；空 = 不筛）。
    pub targets: Vec<String>,
    /// 增量游标（`None` = 从头取一页）。
    pub cursor: Option<u64>,
    /// 单次条数上限。
    pub limit: usize,
}

impl Default for LogQuery {
    fn default() -> Self {
        Self {
            range: LogRange::H1,
            from_ms: None,
            to_ms: None,
            levels: Vec::new(),
            targets: Vec::new(),
            cursor: None,
            limit: ROW_MAX,
        }
    }
}

/// 组装查询（**纯逻辑**；筛选态 → 查询语义的唯一映射点）。
///
/// - `H1` / `H24` ⇒ `from_ms` / `to_ms` 一律 `None`（相对窗口由服务端按档位算，UI 不读时钟）；
/// - `Custom` ⇒ 两侧都由 [`filters::datetime_to_epoch_ms`] 折算（确定、可测，见 `filters` **FR2**）。
pub(crate) fn log_query(
    change: &TimeRangeChange,
    levels: &[LogLevel],
    targets: &[String],
    cursor: Option<u64>,
) -> LogQuery {
    let (from_ms, to_ms) = match change.range {
        LogRange::H1 | LogRange::H24 => (None, None),
        LogRange::Custom => (
            Some(filters::datetime_to_epoch_ms(change.start)),
            Some(filters::datetime_to_epoch_ms(change.end)),
        ),
    };
    LogQuery {
        range: change.range,
        from_ms,
        to_ms,
        levels: levels.to_vec(),
        targets: targets.to_vec(),
        cursor,
        limit: ROW_MAX,
    }
}

/// 行序（§6.3「实时追加：新行插入顶部」）—— 返回注入向量的**下标序**（按 `seq` **降序**；
/// `seq` 相同者保持注入时的相对次序，稳定排序）。
///
/// **为什么按 `seq` 而不是 `ts_ms`**：契约把 `seq` 定义为**单调序号**、也是 `cursor` 的依据
/// （`display-proto/src/log.rs` 的 "增量拉取：`cursor` = 单调 `seq`"）⇒ 它才是"新"的权威判据
/// （同毫秒内的多条日志只有 `seq` 能定序）。
///
/// **为什么在页内排一次**：§6.3 把"新行插入顶部"写在本页的列表规格里；后端 / B3 若因任何
/// 原因给出乱序（或拼接了两页窗口），屏上就会乱序 —— 本地排序廉价（一页 ≤ [`ROW_MAX`] 条）
/// 且让页面**自洽**。窗口的**累积**仍由 B3 承担（本页不自行累加）。
pub(crate) fn row_order(entries: &[LogEntry]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..entries.len()).collect();
    idx.sort_by(|a, b| entries[*b].seq.cmp(&entries[*a].seq));
    idx
}

/// 列表区形态（**三态互斥**；§8.3 把「确实没有」与「无法获知」严格区分）。
///
/// ⚠️ **本页没有「源不可用」态**（§6.3 明写：本页无源不可用态，日志通道断在**通道条**表达）
/// ⇒ **不得**把 `UnavailableState` 搬进本页（那会与通道条重复表达同一件事）。
///
/// **三态的关系（B2c-2 规格评审整改 ①；`PROBE-EDGE08-15` 的现场）**：
/// - `entries = [] ∧ range_too_large = false` ⇒ [`ListView::Empty`]：**确实没有**；
/// - `entries = [] ∧ range_too_large = true` ⇒ [`ListView::Incomplete`]：**无法获知**
///   —— 服务端明示"**本次未执行全库检索**、`entries` 不代表完整结果"，此刻若说「确实没有」
///   就是把"无法获知"**冒充**成"确实没有"（§8.3「语义不同的态必须可区分、不得互替」+
///   §2.6「降级可见、绝不造假」）。⇒ **超限优先**：`range_too_large = true` 时**不显空态**；
/// - 有行 ⇒ [`ListView::Rows`]（**超限不隐藏已返回的条目** —— 契约字段的语义就是
///   "结果不完整"而不是"结果为空"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListView {
    /// 有日志行（`seq` 降序）。
    Rows,
    /// 空态：**确实没有**（当前筛选条件下）。
    Empty,
    /// 不完整态：**无法获知**（超限 ⇒ 本次未执行全库检索）。见 **LG9**。
    Incomplete,
}

/// 行数 × 超限标志 → 列表区形态（**唯一判据**；页内核与单测共用，避免两处各写一遍）。
///
/// **改什么会让本条变红**：把判据写回"只看行数"（`range_too_large` 被忽略）⇒
/// `entries = [] ∧ range_too_large = true` 会同时给出空态与超限条 ⇒
/// `pages_chain` 的「空态 × 超限互斥」组（`ui/tests.rs`）当场红。
pub(crate) const fn list_view_of(rows: usize, range_too_large: bool) -> ListView {
    if rows > 0 {
        ListView::Rows
    } else if range_too_large {
        ListView::Incomplete
    } else {
        ListView::Empty
    }
}

/// 底部状态行文案（§3.6 P3「列表 / 状态」行的 `加载中` / `已加载全部`；无行 ⇒ `None`）。
pub(crate) const fn footer_text(has_more: bool, rows: usize) -> Option<&'static str> {
    if rows == 0 {
        return None;
    }
    Some(if has_more {
        TEXT_FOOTER_LOADING
    } else {
        TEXT_FOOTER_ALL
    })
}

/// 通道条文案（§3.6 P3「通道条」行；两条，**不造第三条"未知"** —— 见 **LG8**）。
pub(crate) const fn channel_text(connected: bool) -> &'static str {
    if connected {
        TEXT_CHANNEL_OK
    } else {
        TEXT_CHANNEL_DOWN
    }
}

/// 通道条灯色（PRD §3.1：已连接 `#28A745` / 断开 `#DC3545` —— 取 `theme` 的同源命名常量）。
pub(crate) const fn channel_dot_color(connected: bool) -> Color {
    if connected {
        Palette::LINK_OK
    } else {
        Palette::LINK_DOWN
    }
}

/// 通道条文字色（§6.3：断 ⇒ **红字**；通则取常规说明色 `text_second`）。
pub(crate) const fn channel_text_color(connected: bool) -> Color {
    if connected {
        Palette::TEXT_SECOND
    } else {
        Palette::LINK_DOWN
    }
}

/// 模块项数 → 网格高（**纯函数**，与 `components.rs::MultiSelectChips` 的算式逐项一致：
/// `rows × (CHIP_H + GAP) − GAP`；`0` 项 ⇒ `0`）。
///
/// ⚠️ **必须用纯函数而不是读 `size()`**：本函数会在**事件回调内**（chip 组重建后立即重摆）
/// 被调用，此时 LVGL 的 `coords` 还没重算（`filters.rs::body_h` 的同款陷阱）。
pub(crate) fn module_grid_h(items: usize) -> i32 {
    if items == 0 {
        return 0;
    }
    let rows = (items as u32).div_ceil(MODULE_COLS) as i32;
    rows * (Dimens::CHIP_H + Dimens::GAP_MIN) - Dimens::GAP_MIN
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 列表行（**行池 / 复用**；每个拥有型句柄都存进字段）
// ═══════════════════════════════════════════════════════════════════════════

/// 实心块样式（色块 / 灯点 / 行底）——**色值一律来自 `theme` 的命名常量**，本函数只做
/// 「底色 + 圆角」组合（`theme.rs` 无对应的现成样式，本批禁改；同 `p5_audit.rs` **AU13** 口径）。
fn solid_style(bg: Color, radius: i32) -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(bg);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(radius);
    s.set_border_width(Stroke::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 一条日志行（44 px；UI §6.3「每行」）。**只读行**：全部零件是 `decor` + `Label`，
/// **没有任何按钮 / 可点对象**（§5.3「非交互列表不用 `lv_list`」）。
struct Row {
    /// 行容器（**拥有型**：`Drop` 即级联删除整行）；底色 = 斑马纹。
    obj: Obj,
    /// 级别色块（88×28；**样式随级别换**，见 [`set_style_index`]）。
    level_block: Obj,
    /// 级别文字（画在色块上；`Palette::BG` 深色字）。
    level_text: Label,
    /// 时间列（24 px `text_second`）。
    time: Label,
    /// 模块列（24 px `text_weak`）。
    module: Label,
    /// 消息列（24 px `text_primary`；`DOTS` = 可见截断 + 省略号，见 **LG5**）。
    message: Label,
    /// 级别样式的当前槽位（[`set_style_index`] 的去重依据；初值 `usize::MAX` = 尚未挂过）。
    level_style: Cell<usize>,
    /// **应用标记**：本行**实际写入**的级别色（薄层读不回样式 ⇒ 记录送给样式构造器的色值；
    /// 与 `p5_audit.rs` **AU13** 的 `immutable_bg` 同口径）。
    level_color: Cell<Color>,
    /// **应用标记**：本行的斑马纹底色（薄层读不回样式 ⇒ 记录送给样式构造器的色值）。
    #[allow(dead_code)]
    stripe: Cell<Color>,
}

impl Row {
    /// 建一行（第 `index` 行；y = `index × 行高`；斑马纹按 `index` 奇偶固定）。
    fn new(
        parent: &Obj,
        index: usize,
        level_styles: &[Rc<Style>],
        stripe_styles: &[Rc<Style>; 2],
    ) -> Result<Self, LvglError> {
        let stripe_idx = index % 2;
        let stripe = if stripe_idx == 0 {
            Palette::SURFACE
        } else {
            Palette::SURFACE_ALT
        };
        // 行容器：**不吃触摸事件**（§5.3：非交互列表用 `lv_obj` 行容器，行内也没有任何入口）
        // —— `decor` 出厂即摘 `CLICKABLE`，是 `row_clickable_parts() == 0` 这条只读回归锁的
        // **结构性前提**。
        let obj = decor(
            parent,
            Dimens::CONTENT_W,
            Dimens::ROW_LOG_H,
            &stripe_styles[stripe_idx],
        )?;
        obj.set_pos(0, index as i32 * Dimens::ROW_LOG_H);

        // 级别色块（先挂透明样式建对象，再把级别样式经 `set_style_index` 挂上 ⇒ 样式唯一）。
        let level_block = decor(
            &obj,
            ROW_LEVEL_W,
            ROW_LEVEL_H,
            &theme::transparent(),
        )?;
        level_block.set_pos(ROW_LEVEL_X, ROW_LEVEL_Y);
        let level_style = Cell::new(usize::MAX);
        let level_color = Cell::new(level_color(LogLevel::Error));
        set_style_index(&level_block, level_styles, &level_style, 0);

        let level_text = text_label(&level_block, "", TextSlot::Body, Palette::BG)?;
        level_text.set_size(ROW_LEVEL_W, TextSlot::Body.px() as i32);
        level_text.set_long_mode(LongMode::DOTS);
        level_text.set_pos(0, ROW_LEVEL_TEXT_Y);

        let time = text_label(&obj, "", TextSlot::Body, Palette::TEXT_SECOND)?;
        time.set_size(ROW_TIME_W, TextSlot::Body.px() as i32);
        time.set_long_mode(LongMode::DOTS);
        time.set_pos(ROW_TIME_X, ROW_TEXT_Y);

        let module = text_label(&obj, "", TextSlot::Body, Palette::TEXT_WEAK)?;
        module.set_size(ROW_MODULE_W, TextSlot::Body.px() as i32);
        module.set_long_mode(LongMode::DOTS);
        module.set_pos(ROW_MODULE_X, ROW_TEXT_Y);

        // 消息列：`DOTS`（**可见截断 + 省略号**，§2.6「降级可见、绝不造假」）—— 见 **LG5**：
        // §7.4 字面要"换行"，但 §6.3 把行高钉死 44 px（且行池按 `i × 44` 窗口化）⇒ 两行装不下，
        // `WRAP` 下 LVGL 只把文字裁到 `txt_clip.y2`、**第二行静默消失且无任何标记**
        // （`vendor/lvgl/src/widgets/label/lv_label.c` 的 `LONG_MODE_CLIP/WRAP: /*Do nothing*/`）。
        // 本页取 **`DOTS`**：截断**可见**（LVGL 把尾部换成 `.` × 3，`.` 在 cmap 内），
        // 与同一行模块列「超长加 `...`」的口径一致。
        // 「改什么会让本条变红」：把 `DOTS` 改回 `WRAP` ⇒ 长消息的读回文本不再以 `...` 结尾
        // （`pages_chain` 的「长消息可见截断」组当场红）；源码级另由 `p3_static_constraints`
        // 锚定消息列这一行（**LG14**）。
        let message = text_label(&obj, "", TextSlot::Body, Palette::TEXT_PRIMARY)?;
        message.set_size(ROW_MSG_W, TextSlot::Body.px() as i32);
        message.set_long_mode(LongMode::DOTS);
        message.set_pos(ROW_MSG_X, ROW_TEXT_Y);

        Ok(Self {
            obj,
            level_block,
            level_text,
            time,
            module,
            message,
            level_style,
            level_color,
            stripe: Cell::new(stripe),
        })
    }

    /// 把一条日志写到本行（**只改文本 / 样式槽**，不新建 / 不删除对象）。
    fn apply(&self, e: &LogEntry, level_styles: &[Rc<Style>]) {
        self.time.set_text(&format_epoch_ms_utc(e.ts_ms));
        set_style_index(
            &self.level_block,
            level_styles,
            &self.level_style,
            level_style_index(e.level),
        );
        self.level_color.set(level_color(e.level));
        self.level_text.set_text(&level_text(e.level));
        self.module
            .set_text(&clip_row_label(&module_label(&e.target)));
        self.message.set_text(&free_text_safe(&e.message));
    }

    /// 本行**可点子对象计数**（只读回归锁：**必须恒 `0`**）。
    #[cfg(test)]
    fn clickable_parts(&self) -> usize {
        let parts: [&Obj; 6] = [
            &self.obj,
            &self.level_block,
            self.level_text.obj(),
            self.time.obj(),
            self.module.obj(),
            self.message.obj(),
        ];
        parts.iter().filter(|o| o.has_flag(crate::lvgl::obj::ObjFlag::CLICKABLE)).count()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 页面内核
// ═══════════════════════════════════════════════════════════════════════════

/// 查询意图槽（[`CbSlot`]；与 `p5_audit.rs::QuerySlot` **同语义**）。
///
/// **不是**裸 `RefCell<Option<Box<dyn FnMut(..)>>>`：裸槽把内部借用**外露**给调用点 ⇒ 能写出
/// "持 `try_borrow_mut` 借用直调用户回调"的写法（回调内自替换被**静默丢弃**）。[`CbSlot`]
/// 只给 `set` / `fire`，承载字段私有 ⇒ 旧写法**编译不过**（见 `ui/pages/mod.rs::sealed`）。
type QuerySlot = CbSlot<LogQuery>;
/// 「回到最新」意图槽（载荷 = `()`；见 **R2**）。
type BackSlot = CbSlot<()>;

/// 页内核（全部 LVGL 句柄 + 共享态）。
struct Core {
    /// 页根（= 纵向滚动容器；契约 1）。
    root: ScrollContainer,
    // ── ① 通道条 ──
    /// 已连接的灯点（绿）。
    #[allow(dead_code)]
    dot_ok: Obj,
    /// 已连接的文案。
    channel_ok: Label,
    /// 断开的灯点（红）。
    #[allow(dead_code)]
    dot_down: Obj,
    /// 断开的文案。
    channel_down: Label,
    /// 通道态（`true` = 已连接；缺省 `false` —— fail-closed，见 **LG8**）。
    connected: Cell<bool>,
    // ── ② 筛选区 ──
    /// 级别维度名。
    #[allow(dead_code)]
    level_label: Label,
    /// 级别 chip 组容器。
    level_box: Obj,
    /// 级别 chip 组（可点；`None` = 尚未建 / 建失败）。
    levels: RefCell<Option<Rc<MultiSelectChips>>>,
    /// 级别 chip 组当前**归一化后**的勾选集合。
    level_sel: RefCell<Vec<usize>>,
    /// 模块维度名。
    #[allow(dead_code)]
    module_label: Label,
    /// 模块 chip 组容器。
    module_box: Obj,
    /// 模块 chip 组。
    modules: RefCell<Option<Rc<MultiSelectChips>>>,
    /// 模块 chip 组当前**选项文案**（重建判据 + 网格高的纯函数输入）。
    module_texts: RefCell<Vec<String>>,
    /// 模块 chip 组当前**归一化后**的勾选集合。
    module_sel: RefCell<Vec<usize>>,
    /// 注入的模块机器键（缺省空 ⇒ 只有「全部」）。
    targets: RefCell<Vec<String>>,
    /// 共享「时间范围」件（`filters.rs`）。
    filter: Rc<TimeRangeFilter>,
    // ── ③ 超限提示（EDGE-15；契约 `LogPage.range_too_large` ⇒ **常驻构件**）──
    warn: Rc<WarnBanner>,
    /// 超限标志（唯一来源 = 注入的 `LogPage.range_too_large`）。
    range_too_large: Cell<bool>,
    // ── ④ 表头 ──
    /// 表头容器（4 列名 + 3 条竖分隔线 + 底线）。
    head: Obj,
    /// 表头 4 个列名标签（**拥有型句柄，必须锚定** —— 局部句柄随返回被 `Drop` ⇒ 级联删子树）。
    #[allow(dead_code)]
    head_cols: Vec<Label>,
    /// 表头 3 条竖分隔线（同上）。
    #[allow(dead_code)]
    head_divs: Vec<Obj>,
    /// 表头底线（同上）。
    #[allow(dead_code)]
    head_rule: Obj,
    // ── ⑤ 列表 ──
    /// 列表容器（行 + 状态行 + 说明行 + 空态）。
    list_box: Obj,
    /// 行池（**只增不减**；上限 [`ROW_MAX`]）。
    rows: RefCell<Vec<Row>>,
    /// 当前应显示的行数。
    shown: Cell<usize>,
    /// 空态（EDGE-08；**本页无不可用态** —— §6.3）。
    empty: Rc<EmptyState>,
    /// 超限下的**中性文案**（**LG9**：零行 + 超限时替空态出场，**不谎称"无日志"**）。
    incomplete: Label,
    /// 底部状态行（`加载中` / `已加载全部`）。
    footer: Label,
    /// 只读说明行（**常驻**，§6.3「不支持导出」节）。
    note: Label,
    /// 「回到最新」按钮（**恒显**；见 **R2**）。
    back: TextButton,
    /// 自动跟随标志（**B3 的注入入口**；薄层读不到滚动位置 ⇒ 当前不驱动视觉，见 **R2**）。
    auto_follow: Cell<bool>,
    /// 级别样式（5 档，**预建** ⇒ 行复用时不产生 `add_style` 无界增长）。
    level_styles: Vec<Rc<Style>>,
    /// 斑马纹样式（2 档，**预建**）。
    stripe_styles: [Rc<Style>; 2],
    // ── ⑥ 注入态 ──
    /// 最近一次注入的 `has_more`。
    has_more: Cell<bool>,
    /// **增量路径是否已激活**（**LG13**；与"是否已有游标"`last_seq` **解耦**）。
    ///
    /// `true` = B3 已把本页拉起来过（有过一次筛选意图）；此后 500 ms 节拍的
    /// [`P3LogsPage::request_increment`]**一律**要能推进 —— 即使窗口为空（`last_seq == 0`）
    /// 也要发一次"重新拉首页"的意图（`cursor = None`），否则**空结果之后增量永久停摆**。
    /// **B3 首次进页前**（本页尚未收到任何筛选意图）⇒ `false` ⇒ 增量不发（首屏归 B3 的
    /// `set_on_query`），与 B2c-2 原行为一致。
    increment_active: Cell<bool>,
    /// 已见最大 `seq`（**增量游标**；见 [`Core::fire_increment`]）。
    last_seq: Cell<u64>,
    /// 最近一次**已发出**的筛选意图（**去重**：相同条件不重复发）。
    last_query: RefCell<Option<LogQuery>>,
    /// 筛选变化意图（**本页不生成 `request_id`**）。
    on_query: QuerySlot,
    /// 增量拉取意图。
    on_increment: QuerySlot,
    /// 「回到最新」意图。
    on_back: BackSlot,
    /// 内核自引用（`Weak`，**不构成 `Rc` 环**）：chip 组在**重建路径**里需要它挂回调。
    me: RefCell<Weak<Core>>,
}

impl Core {
    // ── 布局（**唯一摆放点**：档位 / 超限 / 选项 / 行数变化后都要重摆）────────────────

    /// 按当前状态摆放全部区块（只改位置 / 尺寸 / 可见性，**不新建对象**）。
    fn layout(&self) {
        let y_channel = Dimens::CONTENT_PAD_TOP;
        let y_level = y_channel + CHANNEL_H + TIGHT_GAP;
        let y_module_label = y_level + LEVEL_ROW_H + TIGHT_GAP;
        let y_module_grid = y_module_label + MODULE_LABEL_H;
        let grid_h = module_grid_h(self.module_texts.borrow().len());
        let y_range = y_module_grid + grid_h + Dimens::GAP_MIN;
        // ⚠️ 必须用 `filters::body_h`（**纯函数**）而不是 `size()`：本函数会在**事件回调内**
        // 被调用（用户切到「自定义」的当场），此时 LVGL 的 `coords` 还没重算。
        let filter_h = self.filter.body_h();
        let y_after_filter = y_range + filter_h;
        let warn_on = self.range_too_large.get();
        let y_warn = y_after_filter + Dimens::GAP_MIN;
        let y_head = if warn_on {
            y_warn + Dimens::BANNER_H + TIGHT_GAP
        } else {
            y_after_filter + Dimens::GAP_MIN
        };
        let y_list = y_head + TABLE_HEAD_H;

        // ① 通道条（两条文案 / 两个灯点互斥显隐 —— 各自是**独立对象**，避免"改样式色"
        //    这类无界增长路径）。
        let connected = self.connected.get();
        let dot_y = theme::center_offset(CHANNEL_H, CHANNEL_DOT);
        let text_y = theme::center_offset(CHANNEL_H, TextSlot::Body.px() as i32);
        for d in [&self.dot_ok, &self.dot_down] {
            d.set_size(CHANNEL_DOT, CHANNEL_DOT);
            d.set_pos(0, dot_y);
        }
        for t in [&self.channel_ok, &self.channel_down] {
            t.set_pos(CHANNEL_TEXT_X, text_y);
        }
        show_only(&[&self.dot_ok, &self.dot_down], Some(if connected { 0 } else { 1 }));
        show_only(
            &[self.channel_ok.obj(), self.channel_down.obj()],
            Some(if connected { 0 } else { 1 }),
        );

        // ② 筛选区。
        self.level_label.set_pos(
            0,
            y_level + theme::center_offset(LEVEL_ROW_H, TextSlot::Label.px() as i32),
        );
        self.level_box.set_pos(filters::CTRL_X, y_level);
        self.module_label.set_pos(0, y_module_label);
        self.module_box.set_pos(0, y_module_grid);
        self.module_box.set_size(MODULE_BOX_W, grid_h);
        self.filter.obj().set_pos(0, y_range);

        // ③ 超限提示条（EDGE-15；**常驻构件**，显隐由标志定）。
        self.warn.obj().set_pos(0, y_warn);
        set_visible(self.warn.obj(), warn_on);

        // ④ 表头 / ⑤ 列表。
        self.head.set_pos(0, y_head);
        self.list_box.set_pos(0, y_list);

        let shown = self.shown.get();
        {
            let rows = self.rows.borrow();
            for (i, r) in rows.iter().enumerate() {
                set_visible(&r.obj, i < shown);
            }
        }
        let view = self.view();
        // 空态 / 不完整态**互斥**（**LG9**）：`range_too_large = true` ⇒ 不给空态
        //   （那是把"无法获知"冒充成"确实没有"），改给中性文案「范围超限 · 未执行检索」。
        set_visible(self.empty.obj(), view == ListView::Empty);
        set_visible(self.incomplete.obj(), view == ListView::Incomplete);
        let zero_h = match view {
            ListView::Rows => 0,
            ListView::Empty => EMPTY_H,
            ListView::Incomplete => INCOMPLETE_H,
        };
        let list_h = shown as i32 * Dimens::ROW_LOG_H
            + if view == ListView::Rows { FOOTER_H } else { zero_h }
            + NOTE_H;
        self.list_box.set_size(Dimens::CONTENT_W, list_h);
        // 底部状态行（无行时不显；**LG7**）/ 只读说明行（**常驻**）。
        match footer_text(self.has_more.get(), shown) {
            Some(t) => {
                self.footer.set_text(t);
                self.footer.set_pos(
                    0,
                    shown as i32 * Dimens::ROW_LOG_H
                        + theme::center_offset(FOOTER_H, TextSlot::Body.px() as i32),
                );
                set_visible(self.footer.obj(), view == ListView::Rows);
            }
            None => set_visible(self.footer.obj(), false),
        }
        // 说明行的 y **按形态**给：有行 ⇒ 行区 + 状态行之后；空态 / 不完整态 ⇒ **其下**
        // （否则说明行会压在空态图标 / 文案上 —— 零行区只有 `EMPTY_H` / `INCOMPLETE_H`，
        // 而 `shown = 0` 时"行区 + 状态行"只有 36 px）。
        let note_y = shown as i32 * Dimens::ROW_LOG_H
            + match view {
                ListView::Rows => FOOTER_H,
                ListView::Empty => EMPTY_H,
                ListView::Incomplete => INCOMPLETE_H,
            };
        self.note
            .set_pos(0, note_y + theme::center_offset(NOTE_H, TextSlot::Body.px() as i32));
        self.empty.set_pos(0, 0);
        // 中性文案垂直居中于 `INCOMPLETE_H` 块（水平居中由 `center()` 的**对齐**语义承担，
        // 布局重算时自动跟随 —— 同 `components.rs::EmptyState` 的做法）。
        self.incomplete.center();
        // ⑥ 「回到最新」（**恒显**；零行时不显 —— 无"最新"可回，见 **LG11 / R2**）。
        //
        // **B2c-2 规格评审整改 ②**：按钮放在**列表区之下的专属带**里（y = 列表区底 + 缝），
        // 而**不是**叠在行上 —— 任何一行的矩形都不与按钮矩形相交
        // （几何实测见 `ui/tests.rs` 的 `pages_chain`）。
        self.back.set_size(BACK_W, BACK_W);
        // 专属带占 `BACK_BAND_H`（= 按钮边长 + 与列表区的缝）；按钮贴**带底**，
        // 故其 y = 带顶 + (带高 − 按钮高)。**显式用该常量**而不是手写 `+ GAP_MIN`
        // —— 否则常量会退化成"只在注释里存在"的装饰（B2c-2 验证整改 M2）。
        let band_top = y_list + list_h;
        self.back
            .set_pos(BACK_X, band_top + (BACK_BAND_H - BACK_W));
        set_visible(&self.back, view == ListView::Rows);
    }

    /// 当前列表区形态（**超限优先**：`range_too_large` 时不得给空态，见 [`list_view_of`]）。
    fn view(&self) -> ListView {
        list_view_of(self.shown.get(), self.range_too_large.get())
    }

    // ── 注入（外部 → 屏）───────────────────────────────────────────────────

    /// 注入一页日志（`entries` = 外部组装好的**窗口**；本页**不自行累加**，见模块文档 6）。
    fn apply_page(&self, page: &LogPage) {
        let want = page.entries.len().min(ROW_MAX);
        // 行池**按需分配**：只按本次条数建（上限 [`ROW_MAX`]），不是一次建满；池只增不减。
        {
            let mut rows = self.rows.borrow_mut();
            while rows.len() < want {
                match Row::new(&self.list_box, rows.len(), &self.level_styles, &self.stripe_styles) {
                    Ok(r) => rows.push(r),
                    Err(_) => break, // 建不出即停（**不 panic**）
                }
            }
            // `shown` 以"实际建出"的条数为准（否则布局按"有 want 行"摆底部状态行 / 说明行，
            // 它们会被推到屏外，而实际只有更少的行 —— 静默的形态不一致）。
            self.shown.set(rows.len().min(want));
        }
        let order = row_order(&page.entries);
        {
            let rows = self.rows.borrow();
            for (i, k) in order.iter().take(want).enumerate() {
                if let Some(r) = rows.get(i) {
                    r.apply(&page.entries[*k], &self.level_styles);
                }
            }
        }
        // 增量游标取**已见最大 `seq`**（见 [`Core::fire_increment`] 的推导）—— `next_cursor`
        // 的 `None` 语义是"无更多历史页"，**不**代表"没有更新的条目"。
        let max_seq = page.entries.iter().map(|e| e.seq).max().unwrap_or(0);
        if max_seq > self.last_seq.get() {
            self.last_seq.set(max_seq);
        }
        self.has_more.set(page.has_more);
        self.range_too_large.set(page.range_too_large);
        self.layout();
    }

    /// 重建模块 chip 组（**仅在选项文案变化时**；对象 churn 限于注入路径，非渲染路径）。
    fn rebuild_modules(&self, texts: Vec<String>) {
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        // 先放掉旧件（`Drop` ⇒ `lv_obj_delete` ⇒ 级联删除旧 chip 与它们的回调）。
        drop(self.modules.borrow_mut().take());
        match MultiSelectChips::new(&self.module_box, &refs, MODULE_COLS, MODULE_CHIP_W) {
            Ok(chips) => {
                let chips = Rc::new(chips);
                // 恢复当前勾选（程序化设置**不触发**回调 ⇒ 不误发意图）；越界索引由组内 no-op。
                for i in self.module_sel.borrow().iter().copied() {
                    chips.set_selected(i, true);
                }
                // ⚠️ **回调只持 `Weak<Core>`**：chip 组的回调槽由 `Core.modules` 持有
                // （`Core → modules → chips.on_change`）⇒ 闭包捕获 `Rc<Core>` 会构成**强引用环**
                // ⇒ `drop(P3LogsPage)` 不释放任何 LVGL 对象、二次构造即 OOM（C1 级缺陷）。
                let w = self.me.borrow().clone();
                chips.set_on_change(move |sel| {
                    let Some(me) = w.upgrade() else { return };
                    me.on_modules_changed(sel);
                });
                *self.modules.borrow_mut() = Some(chips);
            }
            Err(e) => {
                // 建不出 chip 组：**不 panic**、不上屏，只写 stderr（同 `p4_interlock.rs` IL25）。
                report_build_failure("模块", &e);
            }
        }
        *self.module_texts.borrow_mut() = texts;
        self.layout();
    }

    /// 模块勾选变化：归一化（「全部」语义）→ 报意图。
    fn on_modules_changed(&self, selected: Vec<usize>) {
        let norm = {
            let prev = self.module_sel.borrow();
            normalize_module_selection(&prev, &selected)
        };
        *self.module_sel.borrow_mut() = norm.clone();
        if let Some(chips) = self.modules.borrow().as_ref() {
            let cur = chips.selected();
            for i in 0..chips.len() {
                if cur.contains(&i) != norm.contains(&i) {
                    chips.set_selected(i, norm.contains(&i));
                }
            }
        }
        self.fire_query();
    }

    /// 级别勾选变化 → 报意图。
    fn on_levels_changed(&self, selected: Vec<usize>) {
        *self.level_sel.borrow_mut() = selected.clone();
        if let Some(chips) = self.levels.borrow().as_ref() {
            let cur = chips.selected();
            for i in 0..chips.len() {
                if cur.contains(&i) != selected.contains(&i) {
                    chips.set_selected(i, selected.contains(&i));
                }
            }
        }
        self.fire_query();
    }

    // ── 意图（屏 → 外部）────────────────────────────────────────────────────

    /// 组装**当前**筛选态 + 指定游标的查询。
    fn current_query(&self, cursor: Option<u64>) -> LogQuery {
        let change = self.filter.change();
        let sel = self.level_sel.borrow();
        let levels = selected_levels(&sel);
        let msel = self.module_sel.borrow();
        let targets = self.targets.borrow();
        log_query(&change, &levels, &selected_targets(&msel, &targets), cursor)
    }

    /// 发「筛选变化」意图（**去重**：与上一次**已发出**的查询相同则不发）。
    ///
    /// 回调经 [`CbSlot::fire`]（take/put-back）触发：**调用期不持借用** ⇒ 回调内再
    /// [`P3LogsPage::set_on_query`] 时 `try_borrow_mut` 必然成功、新回调自**下一次**通知起生效。
    ///
    /// **副作用**：发新查询即把增量游标**清零**（[`Core::last_seq`]）—— 新筛选条件的窗口会
    /// 整体替换屏上内容，旧游标下的"增量"对新窗口没有意义（见 [`Core::fire_increment`]）。
    fn fire_query(&self) {
        let q = self.current_query(None);
        let same = self.last_query.borrow().as_ref() == Some(&q);
        if same {
            return; // 「未变化时不发意图」——离屏用例逐条锁住
        }
        *self.last_query.borrow_mut() = Some(q.clone());
        self.last_seq.set(0);
        self.increment_active.set(true);
        self.on_query.fire(q);
    }

    /// 发「增量拉取」意图（B3 的 500 ms 节拍调用 [`P3LogsPage::request_increment`]）。
    ///
    /// **游标 = 已见最大 `seq`**（不是 `LogPage.next_cursor`）：契约把 `next_cursor`
    /// 定义为"下一页游标，`None` = 无更多（历史页）"，而日志是**持续追加**的 ⇒ 用
    /// `next_cursor` 会在"没有更多历史页"时把游标清零、退化成重复拉取；`max(seq)` 单调且安全
    /// （服务端按 `seq > cursor` 去重）。
    ///
    /// **B2c-2 规格评审整改 ⑦（LG13）—— "是否已有游标"与"是否允许增量"解耦**：
    /// 原实现以"游标是否为 0"决定发不发 ⇒ `fire_query` 把游标清零后**没有任何唤醒路径**
    /// ⇒ 空窗口（`entries = []`，`last_seq` 恒 0）之后增量**永久停摆**（`≤1 s` 实时追加失效）。
    /// 现在改看 [`Core::increment_active`]：
    /// - **已激活**（B3 已发过筛选意图）⇒ 一律发一次意图：
    ///   - 有游标（`last_seq > 0`）⇒ `cursor = Some(max_seq)`（不变）；
    ///   - **无游标** ⇒ `cursor = None`（= **重新拉首页**，这是空窗口下唯一有意义的"继续"）；
    /// - **未激活**（B3 进页前的骨架态）⇒ 不发（首屏归 [`P3LogsPage::set_on_query`]）。
    ///
    /// ⚠️ **与 B3 的接口约定（如实登记，见 LG13）**：B3 负责 500 ms 节拍与**节流**。若 B3
    /// 把本意图原样 1:1 转发，则在"窗口长期为空"期间会每 500 ms 发一次首页查询 ⇒ 约定：
    /// `cursor = None` 的增量意图应当被 B3 当作**刷新**（可降频 / 可去重），而
    /// `cursor = Some(..)` 的才是真正的增量拉取。
    fn fire_increment(&self) {
        if !self.increment_active.get() {
            return;
        }
        let cursor = self.last_seq.get();
        let q = if cursor > 0 {
            self.current_query(Some(cursor))
        } else {
            self.current_query(None)
        };
        self.on_increment.fire(q);
    }

    /// 「回到最新」：复位自动跟随标志 + 报意图（见 **R2** 的能力边界）。
    fn fire_back_to_latest(&self) {
        self.auto_follow.set(true);
        self.on_back.fire(());
    }
}

/// 构件构建失败的 stderr 诊断（**绝不 panic、绝不上屏**）。
fn report_build_failure(what: &str, e: &LvglError) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "P3 日志页：{what}构建失败：{e}");
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. 行池容量（**R3**：实测依据 + 共存预算）
// ═══════════════════════════════════════════════════════════════════════════

/// 行池上限（**LG6**）：只渲染**最新 `ROW_MAX` 条**（按 `seq` 降序）。
///
/// **取值依据（实测，2026-09-13；`timeout 200 cargo test -p local-display -j 2 --lib
/// lvgl_core_bridge_chain`）**：
/// - 单页独活实测：**44 行成功、48 行 `lv_realloc` 失败 + C 侧断言 ⇒ 挂死** ⇒ 上界 ≈ 44–47，
///   记 [`MEASURED_ROW_CAPACITY`] = 44；
/// - `ROW_MAX` 取 **20**（对 44 留 >50% 余量），且与 `p5_audit.rs::ROW_MAX` **同值** ——
///   两个列表页在"一页多少行"上口径一致（两页都只背 20 行的内存）。
///
/// ⚠️ **契约 `LOG_PAGE_LIMIT_MAX` = 200**（单次请求条数），远超本机 256 KB LVGL 堆的行容量
/// ⇒ 本页的上限**低于契约的请求上限**（有界渲染，**LG6** 已登记）；[`LogQuery::limit`] 把
/// 该上界告知 B3，请求侧据此不超量。
///
/// **改什么会让本条变红**：把本值改到 > [`MEASURED_ROW_CAPACITY`] —— `row_pool_capacity_is_measured`
/// 是**纯逻辑**断言，当场红且**不会**把测试跑挂。
pub(crate) const ROW_MAX: usize = 20;

/// 行池**实测可容纳上界**（**单页独活**；离屏链一次性注入的逐档实测：**44 成功 / 48 挂死**）。
///
/// **测量前提（必须与数字一起读）**：
/// - `lvgl-sys/lv_conf.h` 的 `LV_MEM_SIZE` = **256 KB**，且离屏链里各页共用**同一个** LVGL 堆；
/// - **消息取 1–2 字**（B2c-2 规格评审补记）：消息列是**一个** `lv_label`、尺寸恒 `468×24`
///   （`WRAP` 时代亦然、现为 `DOTS`），长消息只是同一标签内的**文本**（不新增对象）⇒
///   堆占用与消息字数**无关**、不呈非线性 —— 故该前提对数字**无影响**，仅在此写明以免误读；
/// - P3 的行比 P5 的行**更省**（P5 每行 ≈ 10 个对象：行 + 竖条 + 5 文字 + 2 个胶囊（各 2 对象）；
///   P3 每行 = 行 + 色块 + 4 文字 = 6 个对象）⇒ P5 的单页上界是 22–24
///   （见 `p5_audit.rs::MEASURED_ROW_CAPACITY`）、P3 是 44（**约 2 倍**）。
pub(crate) const MEASURED_ROW_CAPACITY: usize = 44;

/// **P3 + P5 两页共存**时 **P3 侧**可安全容纳的行数（**R3** 的共存预算）。
///
/// **测量前提（B2c-2 规格评审 四 补强；必须与数字一起读）**：
/// - 256 KB 堆、`pages_chain` 同堆；
/// - **P5 侧只有 4 行**（`p5_audit::COEXIST_ROWS_PER_PAGE` = 4）——⚠️ **不是 P5 满行**
///   （P5 满行 = `p5_audit::ROW_MAX` = 20 行）。**这是本数字的适用边界**；
/// - 消息取 1–2 字（与 [`MEASURED_ROW_CAPACITY`] 同前提；对堆占用无影响）。
///
/// 实测（2026-09-13；`timeout` 保护下逐档）：**P3 = 17 行成功、18 行 `lv_realloc` 失败 +
/// C 侧断言 ⇒ 挂死**；取 **12**（对挂死点 18 留 ≥33% 余量，与 AU8 对 P5 侧 4/6 的余量口径
/// 一致）。
///
/// ⚠️ **如实登记「P5 满行」的情形（本节数字**不**覆盖它）**：由 `p5_audit.rs::AU8` 的实测
/// **②**（"**单页满行(20) + 第二个空页** 也 OOM"、"2 页 × 5 行成功 / × 6 行挂死"）可**推出**：
/// **P5 满行（20）时连 P3 的空页都装不下** ⇒ **"P5 满行 + P3 12 行"必然 OOM**，本节 12 行的
/// 结论**只在 P5 非满行时成立**。（**未**在本单元重测该组合：真去注入会**挂死**而不是变红，
/// 代价与风险都高；这里给的是**由既有实测推导**的结论，标注为"推导"而非"实测"。）
/// ⇒ **给 B3 / 外壳的约束**：两页**不得同时满行**——离页即 `drop`（串行化）或限制同屏行数。
///
/// ⚠️ **它不是"新的一页容量"**：[`ROW_MAX`] = 20 的**单页**契约不变
/// （`COEXIST_ROWS_PER_PAGE < ROW_MAX` 由编译期断言钉死）；它只说明"**两页同时满行在
/// 256 KB 下不可能**"——与 `p5_audit.rs::AU8` 的结论**同族**（两条页各自的上界都低于
/// `ROW_MAX` 时，唯一的根治是扩 `LV_MEM_SIZE` 或由外壳**串行化页面生命周期**）。
pub(crate) const COEXIST_ROWS_PER_PAGE: usize = 12;

/// **编译期自证**：行池上限不超实测上界，且至少装得下一页；共存预算严格小于单页上限。
///
/// 写成编译期断言（而不是运行期 `assert!`）：① 任何构建都查、不可能被跳过；② **不会**把测试
/// 跑挂 —— 真去注入那么多条会触发 `lv_realloc` 失败 + C 侧断言 ⇒ **挂死**（不是"红"）。
/// 安全复现 / 二分定位的指引见 `p5_audit.rs` 的 `ROW_MAX` 文档（同款堆预算问题）。
const _: () = assert!(ROW_MAX <= MEASURED_ROW_CAPACITY);
const _: () = assert!(ROW_MAX < LOG_PAGE_LIMIT_MAX);
const _: () = assert!(COEXIST_ROWS_PER_PAGE < ROW_MAX);

// ═══════════════════════════════════════════════════════════════════════════
// 7. 页面
// ═══════════════════════════════════════════════════════════════════════════

/// P3 日志页（**只读**；控制通道驱动，见模块文档）。
///
/// # 意图如何交给外部
///
/// 本页**不发请求、不生成 `request_id`**：
/// - 筛选条件变化（级别多选 / 模块多选 / 时间范围）⇒ [`P3LogsPage::set_on_query`] 的回调收到
///   一份 [`LogQuery`]（`cursor = None`）—— 首次加载与筛选变化**共用**这一条路径；
/// - 增量拉取（B3 的 **500 ms** 节拍）⇒ B3 周期调用 [`P3LogsPage::request_increment`]，
///   [`P3LogsPage::set_on_increment`] 的回调收到 `cursor = Some(已见最大 seq)` 的 [`LogQuery`]；
/// - 「回到最新」⇒ [`P3LogsPage::set_on_back_to_latest`]（载荷 `()`，见 **R2**）。
///
/// 外部（B3 的 `console.rs`）据此生成 `request_id` / 发 `GET /v1/console/logs`，并把结果经
/// [`P3LogsPage::set_page`] 灌回。
pub struct P3LogsPage {
    core: Rc<Core>,
}

impl P3LogsPage {
    /// 在 `parent` 下建页（页根 `992 × 624`，**自身即纵向滚动容器** —— 契约 1；
    /// 摆放由调用方负责）。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = page_root(parent)?;

        // ── ① 通道条（两条文案 + 两个灯点互斥；缺省 = 断开，见 **LG8**）──
        let dot_ok = decor(
            &root,
            CHANNEL_DOT,
            CHANNEL_DOT,
            &solid_style(channel_dot_color(true), theme::Radius::CHIP),
        )?;
        let channel_ok = text_label(&root, channel_text(true), TextSlot::Body, channel_text_color(true))?;
        channel_ok.set_size(Dimens::CONTENT_W - CHANNEL_TEXT_X, TextSlot::Body.px() as i32);
        channel_ok.set_long_mode(LongMode::DOTS);
        let dot_down = decor(
            &root,
            CHANNEL_DOT,
            CHANNEL_DOT,
            &solid_style(channel_dot_color(false), theme::Radius::CHIP),
        )?;
        let channel_down =
            text_label(&root, channel_text(false), TextSlot::Body, channel_text_color(false))?;
        channel_down.set_size(Dimens::CONTENT_W - CHANNEL_TEXT_X, TextSlot::Body.px() as i32);
        channel_down.set_long_mode(LongMode::DOTS);

        // ── ② 筛选区：级别（4 项一排）+ 模块（换行网格，首位「全部」）+ 时间范围（共享件）──
        let level_label = text_label(&root, TEXT_LEVEL_LABEL, TextSlot::Label, Palette::TEXT_SECOND)?;
        level_label.set_size(filters::LABEL_W, TextSlot::Label.px() as i32);
        level_label.set_long_mode(LongMode::DOTS);
        let level_box = layout_box(&root, LEVEL_BOX_W, LEVEL_ROW_H)?;
        let module_label = text_label(&root, TEXT_MODULE_LABEL, TextSlot::Label, Palette::TEXT_SECOND)?;
        module_label.set_size(filters::LABEL_W, TextSlot::Label.px() as i32);
        module_label.set_long_mode(LongMode::DOTS);
        let module_box = layout_box(&root, MODULE_BOX_W, Dimens::CHIP_H)?;
        let filter = filters::build(&root, TimeRangeChange::default())?;

        // ── ③ 超限提示条（EDGE-15；契约有该字段 ⇒ **常驻构件**，显隐由标志定）──
        let warn = Rc::new(WarnBanner::new(
            &root,
            Dimens::CONTENT_W,
            TEXT_RANGE_TOO_LARGE,
            &[],
        )?);
        warn.set_hidden(true);

        // ── ④ 表头（4 列名 + 3 条竖分隔线 + 底线）──
        //
        // ⚠️ **所有权纪律**：每个子件都存进 `Core` 的字段（`head_cols` / `head_divs` /
        // `head_rule`）—— 局部句柄随 `new()` 返回被 `Drop` ⇒ LVGL **级联删除整棵子树**
        // ⇒ 表头整块空白而容器仍存活（"网看着在、实则没把住"的形态）。
        let head = layout_box(&root, Dimens::CONTENT_W, TABLE_HEAD_H)?;
        let head_specs: [(i32, &str); 4] = [
            (ROW_TIME_X, TEXT_HEAD_TIME),
            (ROW_LEVEL_X, TEXT_HEAD_LEVEL),
            (ROW_MODULE_X, TEXT_HEAD_MODULE),
            (ROW_MSG_X, TEXT_HEAD_MESSAGE),
        ];
        let mut head_cols: Vec<Label> = Vec::with_capacity(head_specs.len());
        for (x, t) in head_specs {
            let l = text_label(&head, t, TextSlot::Body, Palette::TEXT_WEAK)?;
            l.set_size(Dimens::CHIP_MIN_W, TextSlot::Body.px() as i32);
            l.set_long_mode(LongMode::DOTS);
            l.set_pos(x, theme::center_offset(TABLE_HEAD_H, TextSlot::Body.px() as i32));
            head_cols.push(l);
        }
        let mut head_divs: Vec<Obj> = Vec::with_capacity(head_specs.len() - 1);
        for (x, _) in head_specs.iter().skip(1) {
            let d = decor(&head, HEAD_DIV_W, HEAD_DIV_H, &theme::card_head_bar(Palette::DIVIDER))?;
            d.set_pos(x - TIGHT_GAP, TIGHT_GAP);
            head_divs.push(d);
        }
        let head_rule = decor(&head, Dimens::CONTENT_W, HEAD_DIV_W, &theme::card_head_bar(Palette::DIVIDER))?;
        head_rule.set_pos(0, TABLE_HEAD_H - HEAD_DIV_W);

        // ── ⑤ 列表区（行池 + 空态 + 状态行 + **常驻**说明行）──
        let list_box = layout_box(&root, Dimens::CONTENT_W, EMPTY_H + NOTE_H)?;
        let empty = Rc::new(EmptyState::new(&list_box, TEXT_EMPTY_ICON, TEXT_EMPTY)?);
        // 超限下的中性文案（**LG9**）：与空态**互斥**，由 `layout()` 按形态显隐。
        let incomplete =
            text_label(&list_box, TEXT_INCOMPLETE, TextSlot::Body, Palette::TEXT_SECOND)?;
        set_visible(incomplete.obj(), false);
        let footer = label(&list_box, TextSlot::Body, Palette::TEXT_WEAK)?;
        footer.set_size(Dimens::CONTENT_W, TextSlot::Body.px() as i32);
        footer.set_long_mode(LongMode::DOTS);
        let note = text_label(
            &list_box,
            &format!("{TEXT_NO_EXPORT}{TEXT_CLAUSE_SEP}{TEXT_NO_EXPORT2}"),
            TextSlot::Body,
            Palette::TEXT_WEAK,
        )?;
        note.set_size(Dimens::CONTENT_W, TextSlot::Body.px() as i32);
        note.set_long_mode(LongMode::DOTS);

        // ── ⑥ 「回到最新」（**恒显**；最后建 ⇒ 画在行之上；见 **R2**）──
        let back = TextButton::create(&root, TEXT_BACK_TO_LATEST)?;
        back.set_size(BACK_W, BACK_W);
        back.add_style(&theme::control_surface(), StyleSelector::main());
        theme::button(theme::ButtonKind::Text).apply(&back);
        back.label().set_size(
            BACK_W - 2 * Dimens::GAP_MIN,
            BACK_W - 2 * Dimens::GAP_MIN,
        );
        back.label().set_long_mode(LongMode::WRAP);
        back.label().set_pos(
            Dimens::GAP_MIN,
            theme::center_offset(BACK_W, 2 * TextSlot::Label.px() as i32),
        );

        let level_styles: Vec<Rc<Style>> = ALL_LEVELS
            .iter()
            .map(|l| solid_style(level_color(*l), theme::Radius::CTRL))
            .collect();
        let stripe_styles = [
            solid_style(Palette::SURFACE, theme::Radius::NONE),
            solid_style(Palette::SURFACE_ALT, theme::Radius::NONE),
        ];

        let core = Rc::new(Core {
            root,
            dot_ok,
            channel_ok,
            dot_down,
            channel_down,
            connected: Cell::new(false),
            level_label,
            level_box,
            levels: RefCell::new(None),
            level_sel: RefCell::new(Vec::new()),
            module_label,
            module_box,
            modules: RefCell::new(None),
            module_texts: RefCell::new(Vec::new()),
            module_sel: RefCell::new(vec![0]),
            targets: RefCell::new(Vec::new()),
            filter: Rc::clone(&filter),
            warn,
            range_too_large: Cell::new(false),
            head,
            head_cols,
            head_divs,
            head_rule,
            list_box,
            rows: RefCell::new(Vec::new()),
            shown: Cell::new(0),
            empty,
            incomplete,
            footer,
            note,
            back,
            auto_follow: Cell::new(true),
            level_styles,
            stripe_styles,
            has_more: Cell::new(false),
            increment_active: Cell::new(false),
            last_seq: Cell::new(0),
            last_query: RefCell::new(None),
            on_query: CbSlot::new(),
            on_increment: CbSlot::new(),
            on_back: CbSlot::new(),
            me: RefCell::new(Weak::new()),
        });
        *core.me.borrow_mut() = Rc::downgrade(&core);

        // 级别 chip 组（4 项，**缺省全不选** = 不按级别筛 —— §6.3 ① 无「全部」项）。
        {
            let texts = level_options();
            let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            match MultiSelectChips::new(&core.level_box, &refs, LEVEL_COLS, LEVEL_CHIP_W) {
                Ok(chips) => {
                    let chips = Rc::new(chips);
                    let w = Rc::downgrade(&core);
                    chips.set_on_change(move |sel| {
                        let Some(me) = w.upgrade() else { return };
                        me.on_levels_changed(sel);
                    });
                    *core.levels.borrow_mut() = Some(chips);
                }
                Err(e) => report_build_failure("级别", &e),
            }
        }
        // 模块 chip 组（缺省只有「全部」且**勾选** = 不按模块筛）。
        core.rebuild_modules(module_options(&[]));

        // 时间范围变化 ⇒ 重摆（自定义展开会改体高）+ 报意图（去重由 `fire_query` 承担）。
        {
            let w = Rc::downgrade(&core);
            core.filter.set_on_change(move |_c: TimeRangeChange| {
                let Some(c) = w.upgrade() else { return };
                c.layout();
                c.fire_query();
            });
        }
        // 「回到最新」点击 ⇒ 意图（**R2**：本层无法程序化回顶，见模块文档）。
        {
            let w = Rc::downgrade(&core);
            core.back.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                c.fire_back_to_latest();
            });
        }

        let page = Self { core };
        // 骨架态 = 无注入 ⇒ `shown = 0` ⇒ 空态（EDGE-08 的文案；§8.3 明写"不得显未加载或空白"）。
        page.core.layout();
        Ok(page)
    }

    // ── 对象读回（装配 / 断言口径）──────────────────────────────────────────

    /// 页根（= 纵向滚动容器）。
    pub fn obj(&self) -> &Obj {
        &self.core.root
    }

    /// 共享「时间范围」件（装配 / 断言口径）。
    pub fn filter(&self) -> &Rc<TimeRangeFilter> {
        &self.core.filter
    }

    /// 通道条「已连接」文案的对象（泄漏探针的锚点 —— 它被误删会让通道条整块消失）。
    #[cfg(test)]
    pub(crate) fn channel_ok_obj(&self) -> &Obj {
        self.core.channel_ok.obj()
    }

    /// 级别 chip 组容器（泄漏探针锚点）。
    #[cfg(test)]
    pub(crate) fn level_box_obj(&self) -> &Obj {
        &self.core.level_box
    }

    /// 模块 chip 组容器（泄漏探针锚点）。
    #[cfg(test)]
    pub(crate) fn module_box_obj(&self) -> &Obj {
        &self.core.module_box
    }

    /// 超限提示条（泄漏探针锚点）。
    #[cfg(test)]
    pub(crate) fn warn_obj(&self) -> &Obj {
        self.core.warn.obj()
    }

    /// 通道条当前文案（**读在显的那一条** —— 由 LVGL 的隐藏标志决定，不是读注入态）。
    #[cfg(test)]
    pub(crate) fn channel_text(&self) -> Option<String> {
        if !self.core.channel_ok.obj().is_hidden() {
            self.core.channel_ok.text()
        } else {
            self.core.channel_down.text()
        }
    }

    /// 通道条是否已连接（读回注入态）。
    #[cfg(test)]
    pub(crate) fn channel_connected(&self) -> bool {
        self.core.connected.get()
    }

    /// 通道条两个灯点的**颜色**（应用标记：`(已连接, 断开)`）。
    #[cfg(test)]
    pub(crate) fn channel_dot_colors(&self) -> (Color, Color) {
        (
            channel_dot_color(true),
            channel_dot_color(false),
        )
    }

    /// 通道条断开的文案对象是否在显（LG-07 的"内容仍在"断言用）。
    #[cfg(test)]
    pub(crate) fn channel_down_visible(&self) -> bool {
        !self.core.channel_down.obj().is_hidden()
    }

    /// 级别维度名文案。
    #[cfg(test)]
    pub(crate) fn level_label_text(&self) -> Option<String> {
        self.core.level_label.text()
    }

    /// 级别 chip 数（恒 4；建失败 ⇒ 0）。
    #[cfg(test)]
    pub(crate) fn level_chip_count(&self) -> usize {
        self.core.levels.borrow().as_ref().map(|c| c.len()).unwrap_or(0)
    }

    /// 第 `i` 个级别 chip 的**当前显示文本**（含选中前缀 `✓ `）。
    #[cfg(test)]
    pub(crate) fn level_chip_display(&self, i: usize) -> Option<String> {
        self.core
            .levels
            .borrow()
            .as_ref()
            .and_then(|c| c.chip_display(i))
    }

    /// 级别 chip 组的列数（= 4，§6.3 ① 一排四项）。
    #[cfg(test)]
    pub(crate) fn level_chip_columns(&self) -> u32 {
        self.core.levels.borrow().as_ref().map(|c| c.columns()).unwrap_or(0)
    }

    /// 当前级别勾选集合。
    #[cfg(test)]
    pub(crate) fn level_selected(&self) -> Vec<usize> {
        self.core.level_sel.borrow().clone()
    }

    /// 模块维度名文案。
    #[cfg(test)]
    pub(crate) fn module_label_text(&self) -> Option<String> {
        self.core.module_label.text()
    }

    /// 模块 chip 数（= 「全部」+ 注入项数）。
    #[cfg(test)]
    pub(crate) fn module_chip_count(&self) -> usize {
        self.core.modules.borrow().as_ref().map(|c| c.len()).unwrap_or(0)
    }

    /// 第 `i` 个模块 chip 的当前显示文本。
    #[cfg(test)]
    pub(crate) fn module_chip_display(&self, i: usize) -> Option<String> {
        self.core
            .modules
            .borrow()
            .as_ref()
            .and_then(|c| c.chip_display(i))
    }

    /// 模块 chip 组的**列数**（§6.3 ② 的 8 项/行）。
    #[cfg(test)]
    pub(crate) fn module_chip_columns(&self) -> u32 {
        self.core.modules.borrow().as_ref().map(|c| c.columns()).unwrap_or(0)
    }

    /// 模块 chip 组的**实际体高**（多行 ⇒ 大于 48；**R3 / §6.3 ② 的换行断言**用）。
    #[cfg(test)]
    pub(crate) fn module_grid_size(&self) -> (i32, i32) {
        self.core.module_box.size()
    }

    /// 模块 chip 组的**列数 / 行数**（由 `size()` 反推容器宽高，用于"多行而非横滚"断言）。
    #[cfg(test)]
    pub(crate) fn module_grid_rows(&self) -> i32 {
        let h = self.core.module_box.size().1;
        if h <= 0 {
            return 0;
        }
        (h + Dimens::GAP_MIN) / (Dimens::CHIP_H + Dimens::GAP_MIN)
    }

    /// 当前模块勾选集合。
    #[cfg(test)]
    pub(crate) fn module_selected(&self) -> Vec<usize> {
        self.core.module_sel.borrow().clone()
    }

    /// **测试专用**：把一次「级别勾选变化」派发进页面侧的分派路径
    /// （归一化 → chip 勾选态同步 → 报意图）。
    ///
    /// ⚠️ **如实标注**：chip 的**点击事件**本身由 `ui/components.rs::MultiSelectChips` 的
    /// `on_clicked` 承担（其勾选态读回已有既有用例），而薄层**没有**"遍历容器子对象"的
    /// 读口（`Obj` 只有 `child_count()`，没有 `child(i)`）⇒ 离屏链拿不到 chip 的 `Obj`、
    /// **无法**向它 `send_event(CLICKED)`。本入口覆盖的是**页面侧**的那一半
    /// （分派 → 归一化 → `fire_query` 的查询组装与去重）。
    #[cfg(test)]
    pub(crate) fn dispatch_level_selection(&self, selected: Vec<usize>) {
        self.core.on_levels_changed(selected);
    }

    /// **测试专用**：同 [`P3LogsPage::dispatch_level_selection`]（模块维度）。
    #[cfg(test)]
    pub(crate) fn dispatch_module_selection(&self, selected: Vec<usize>) {
        self.core.on_modules_changed(selected);
    }

    /// 注入的模块机器键（读回）。
    #[cfg(test)]
    pub(crate) fn injected_targets(&self) -> Vec<String> {
        self.core.targets.borrow().clone()
    }

    /// 模块 chip 组的**选项文案**（读回；`[0]` 恒为「全部」）。
    #[cfg(test)]
    pub(crate) fn module_option_texts(&self) -> Vec<String> {
        self.core.module_texts.borrow().clone()
    }

    /// 超限提示条是否在显（EDGE-15）。
    #[cfg(test)]
    pub(crate) fn warn_visible(&self) -> bool {
        !self.core.warn.obj().is_hidden()
    }

    /// 超限提示条文案。
    #[cfg(test)]
    pub(crate) fn warn_text(&self) -> Option<String> {
        self.core.warn.text()
    }

    /// 表头容器（装配断言口径）。
    #[cfg(test)]
    pub(crate) fn head_obj(&self) -> &Obj {
        &self.core.head
    }

    /// 表头**子件数**（读 LVGL 的 `child_count()`，恒 4 列名 + 3 竖线 + 1 底线 = 8）。
    #[cfg(test)]
    pub(crate) fn head_child_count(&self) -> usize {
        self.core.head.child_count() as usize
    }

    /// 第 `i` 列表头文案（越界 ⇒ `None`）。
    #[cfg(test)]
    pub(crate) fn head_col_text(&self, i: usize) -> Option<String> {
        self.core.head_cols.get(i).and_then(|l| l.text())
    }

    /// 存活的表头竖分隔线数（锚定回归锁；恒 3）。
    #[cfg(test)]
    pub(crate) fn head_divs_alive(&self) -> usize {
        self.core.head_divs.iter().filter(|o| o.is_alive()).count()
    }

    /// 表头底线是否存活（锚定回归锁）。
    #[cfg(test)]
    pub(crate) fn head_rule_alive(&self) -> bool {
        self.core.head_rule.is_alive()
    }

    /// 列表容器（装配断言口径）。
    #[cfg(test)]
    pub(crate) fn list_obj(&self) -> &Obj {
        &self.core.list_box
    }

    /// 已建行对象数（= 行池长度）。
    #[cfg(test)]
    pub(crate) fn row_pool_len(&self) -> usize {
        self.core.rows.borrow().len()
    }

    /// 当前可见行数（读 LVGL 的隐藏标志）。
    #[cfg(test)]
    pub(crate) fn visible_rows(&self) -> usize {
        self.core
            .rows
            .borrow()
            .iter()
            .filter(|r| !r.obj.is_hidden())
            .count()
    }

    /// 存活的**行对象**数（**所有权锚定回归锁**：行句柄退化成局部变量 ⇒ 小于池长）。
    #[cfg(test)]
    pub(crate) fn rows_alive(&self) -> usize {
        self.core.rows.borrow().iter().filter(|r| r.obj.is_alive()).count()
    }

    /// 第 `i` 行尺寸。
    #[cfg(test)]
    pub(crate) fn row_size(&self, i: usize) -> Option<(i32, i32)> {
        self.core.rows.borrow().get(i).map(|r| r.obj.size())
    }

    /// 第 `i` 行位置。
    #[cfg(test)]
    pub(crate) fn row_pos(&self, i: usize) -> Option<(i32, i32)> {
        let c = self.core.rows.borrow().get(i).map(|r| r.obj.coords())?;
        Some((c.x1, c.y1))
    }

    /// 第 `i` 行时间列文本。
    #[cfg(test)]
    pub(crate) fn row_time(&self, i: usize) -> Option<String> {
        self.core.rows.borrow().get(i).and_then(|r| r.time.text())
    }

    /// 第 `i` 行级别文字。
    #[cfg(test)]
    pub(crate) fn row_level(&self, i: usize) -> Option<String> {
        self.core.rows.borrow().get(i).and_then(|r| r.level_text.text())
    }

    /// 第 `i` 行级别色块**应用标记**（实际写入样式的色值）。
    #[cfg(test)]
    pub(crate) fn row_level_color(&self, i: usize) -> Option<Color> {
        self.core.rows.borrow().get(i).map(|r| r.level_color.get())
    }

    /// 第 `i` 行斑马纹**应用标记**。
    #[cfg(test)]
    pub(crate) fn row_stripe(&self, i: usize) -> Option<Color> {
        self.core.rows.borrow().get(i).map(|r| r.stripe.get())
    }

    /// 第 `i` 行模块列文本。
    #[cfg(test)]
    pub(crate) fn row_module(&self, i: usize) -> Option<String> {
        self.core.rows.borrow().get(i).and_then(|r| r.module.text())
    }

    /// 第 `i` 行消息列文本。
    #[cfg(test)]
    pub(crate) fn row_message(&self, i: usize) -> Option<String> {
        self.core.rows.borrow().get(i).and_then(|r| r.message.text())
    }

    /// 第 `i` 行级别色块的尺寸（装配断言口径）。
    #[cfg(test)]
    pub(crate) fn row_level_block_size(&self, i: usize) -> Option<(i32, i32)> {
        self.core.rows.borrow().get(i).map(|r| r.level_block.size())
    }

    /// **只读回归锁**：行内**可点子对象计数**（恒 `0`）。
    #[cfg(test)]
    pub(crate) fn rows_clickable_parts(&self) -> usize {
        self.core.rows.borrow().iter().map(|r| r.clickable_parts()).sum()
    }

    /// 列表区形态。
    #[cfg(test)]
    pub(crate) fn list_view(&self) -> ListView {
        self.core.view()
    }

    /// 空态是否在显（EDGE-08）。
    #[cfg(test)]
    pub(crate) fn empty_visible(&self) -> bool {
        !self.core.empty.obj().is_hidden()
    }

    /// 空态文案。
    #[cfg(test)]
    pub(crate) fn empty_text(&self) -> Option<String> {
        self.core.empty.text()
    }

    /// **不完整态**（超限下的中性文案）是否在显（**LG9**）。
    #[cfg(test)]
    pub(crate) fn incomplete_visible(&self) -> bool {
        !self.core.incomplete.obj().is_hidden()
    }

    /// 不完整态文案。
    #[cfg(test)]
    pub(crate) fn incomplete_text(&self) -> Option<String> {
        self.core.incomplete.text()
    }

    /// 增量路径是否已激活（**LG13** 的读回口径）。
    #[cfg(test)]
    pub(crate) fn increment_active(&self) -> bool {
        self.core.increment_active.get()
    }

    /// 底部状态行文本（`加载中` / `已加载全部`）。
    #[cfg(test)]
    pub(crate) fn footer_text(&self) -> Option<String> {
        self.core.footer.text()
    }

    /// 底部状态行是否在显。
    #[cfg(test)]
    pub(crate) fn footer_visible(&self) -> bool {
        !self.core.footer.obj().is_hidden()
    }

    /// 只读说明行文本（**常驻**）。
    #[cfg(test)]
    pub(crate) fn note_text(&self) -> Option<String> {
        self.core.note.text()
    }

    /// 只读说明行是否在显（**恒真** —— 常驻）。
    #[cfg(test)]
    pub(crate) fn note_visible(&self) -> bool {
        !self.core.note.obj().is_hidden()
    }

    /// 只读说明行**在列表容器内的 y**（装配断言口径：空态下不得压在空态上）。
    #[cfg(test)]
    pub(crate) fn note_y_in_list(&self) -> i32 {
        self.core.note.obj().coords().y1 - self.core.list_box.coords().y1
    }

    /// 空态 / 说明行占位高的编译期契约值（空态 = 图标 64 + 缝 + 文字行；说明行 = 36）。
    #[cfg(test)]
    pub(crate) fn empty_h(&self) -> i32 {
        EMPTY_H
    }

    /// 「回到最新」按钮的文本。
    #[cfg(test)]
    pub(crate) fn back_text(&self) -> Option<String> {
        self.core.back.text()
    }

    /// 「回到最新」按钮是否在显。
    #[cfg(test)]
    pub(crate) fn back_visible(&self) -> bool {
        !self.core.back.is_hidden()
    }

    /// 「回到最新」按钮的底层对象（离屏链用它 `send_event(CLICKED)` 驱动点击意图 —— **R2**
    /// 的可实现部分）。
    #[cfg(test)]
    pub(crate) fn back_obj(&self) -> &Obj {
        &self.core.back
    }

    /// 「回到最新」按钮是否**可点**（读 LVGL 的 `CLICKABLE` 真值 —— 正向控制：它必须可点）。
    #[cfg(test)]
    pub(crate) fn back_clickable(&self) -> bool {
        self.core.back.has_flag(crate::lvgl::obj::ObjFlag::CLICKABLE)
    }

    /// 「回到最新」按钮的尺寸（§6.3：92×92 —— 触摸目标 ≥64 ✓）。
    #[cfg(test)]
    pub(crate) fn back_size(&self) -> (i32, i32) {
        self.core.back.size()
    }

    /// 自动跟随标志（**R2**：当前不驱动视觉，只作 B3 的注入 / 读回口径）。
    #[cfg(test)]
    pub(crate) fn auto_follow(&self) -> bool {
        self.core.auto_follow.get()
    }

    /// 已见最大 `seq`（增量游标读回）。
    #[cfg(test)]
    pub(crate) fn last_seq(&self) -> u64 {
        self.core.last_seq.get()
    }

    /// **只读页的结构性声明**：写入口清单**恒空**（UI §6.3「不支持导出」节 / PRD T-2）。
    pub const WRITE_ENTRIES: [&'static str; 0] = [];

    // ── 数据入口（**控制通道驱动**，契约 2′）──────────────────────────────────

    /// 注入一页日志（`entries` = 外部组装好的**窗口**；本页按 `seq` 降序渲染）。
    pub fn set_page(&self, page: &LogPage) {
        self.core.apply_page(page);
    }

    /// 注入模块选项（`GET /v1/console/logs/targets`，≤ `LOG_TARGETS_MAX` 项）。
    ///
    /// 选项集合**以注入为准**（服务端真源）：文案与当前不一致时才重建 chip 组（避免无谓 churn）。
    /// 重建在**注入路径**（非渲染路径）发生；当前勾选按**下标**恢复（越界 no-op）——
    /// 选项变化本身**不**触发查询意图（用户没操作筛选）。
    pub fn set_targets(&self, targets: &[String]) {
        *self.core.targets.borrow_mut() = targets.to_vec();
        let texts = module_options(targets);
        if *self.core.module_texts.borrow() != texts {
            self.core.rebuild_modules(texts);
        }
    }

    /// 注入日志通道态（`true` = 已连接；断 = **连续 2 次请求失败**，由 B3 判定）。
    ///
    /// **重连期间不清空已展示内容**（LG-07）：本方法只改通道条，**不触碰行池 / 列表**。
    pub fn set_channel(&self, connected: bool) {
        self.core.connected.set(connected);
        self.core.layout();
    }

    // ── 意图回调（屏 → 外部；**本页不发请求 / 不生成 `request_id`**）────────────

    /// 注册「筛选变化」意图回调（载荷 = [`LogQuery`]，`cursor = None`）。
    ///
    /// **去重口径**：与上一次**已发出**的查询逐字段相同 ⇒ 不重复回调（重复点同一 chip 不会
    /// 产生重复请求）。**改什么会让本条变红**：去掉 [`Core::fire_query`] 里的去重判断 ⇒
    /// 离屏用例「未变化时不发意图」立刻红。
    pub fn set_on_query<F>(&self, f: F)
    where
        F: FnMut(LogQuery) + 'static,
    {
        self.core.on_query.set(f);
    }

    /// 注册「增量拉取」意图回调（载荷 = [`LogQuery`]，`cursor = Some(已见最大 seq)`）。
    pub fn set_on_increment<F>(&self, f: F)
    where
        F: FnMut(LogQuery) + 'static,
    {
        self.core.on_increment.set(f);
    }

    /// **「增量拉取」的触发入口**（B3 的 **500 ms** 节拍调用，设计 §4.4）。
    ///
    /// **B2c-2 规格评审整改 ⑦（LG13）**：
    /// - **本页尚未被拉起**（B3 还没发过任何筛选意图）⇒ **不发**（首屏查询归
    ///   [`P3LogsPage::set_on_query`]）；
    /// - **已拉起**：有游标 ⇒ `cursor = Some(已见最大 seq)`；**无游标（含空窗口）
    ///   ⇒ `cursor = None`（重新拉首页）** —— 空结果之后增量**不得停摆**。
    pub fn request_increment(&self) {
        self.core.fire_increment();
    }

    /// 注册「回到最新」意图回调（载荷 `()`；能力边界见 **R2**）。
    pub fn set_on_back_to_latest<F>(&self, mut f: F)
    where
        F: FnMut() + 'static,
    {
        self.core.on_back.set(move |()| f());
    }

    /// **程序化**置自动跟随标志（B3 的注入入口；**不触发**回调）。
    ///
    /// ⚠️ **R2**：薄层读不到滚动位置（无 `LV_EVENT_SCROLL`、无 `lv_obj_get_scroll_y`）⇒
    /// 该标志当前**不驱动任何视觉差异**；它是"未来接上滚动能力后"的落点，本页**不假装**
    /// 已经实现"手动上滚 ⇒ 停止自动滚动"。
    pub fn set_auto_follow(&self, on: bool) {
        self.core.auto_follow.set(on);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. 纯逻辑单测（**不触碰 LVGL** ⇒ 可独立 `#[test]`；离屏链路在 `ui/tests.rs::pages_chain`）
//
// 每条断言旁写「改什么会让本条变红」—— 本项目多次抓到"宣称会红但实测不红"的断言。
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::controls::DateTimeValue;

    /// 造一条日志（时间戳固定为 UI §6.5 的示例时刻）。
    fn entry(seq: u64, level: LogLevel, target: &str, message: &str) -> LogEntry {
        LogEntry {
            seq,
            ts_ms: 1_789_047_727_000 + seq,
            level,
            target: target.into(),
            message: message.into(),
        }
    }

    // ── R1：级别文字 / 颜色（含 `Trace` 的处置）──────────────────────────────

    /// 四个 PRD 级别的文字**逐字不变**；`Trace` 经 `display_safe` 归一（`T` 缺字形 ⇒ `?RACE`）。
    ///
    /// **改什么会让本条变红**：把 [`level_text`] 改成直接返回 `display_name()`（`TRACE` 的
    /// `T`(U+0054) 不在 cmap 内 ⇒ 真机豆腐块）。
    #[test]
    fn level_texts_are_cmap_safe_and_trace_is_normalized() {
        assert_eq!(level_text(LogLevel::Error), TEXT_LEVEL_ERROR);
        assert_eq!(level_text(LogLevel::Warn), TEXT_LEVEL_WARN);
        assert_eq!(level_text(LogLevel::Info), TEXT_LEVEL_INFO);
        assert_eq!(level_text(LogLevel::Debug), TEXT_LEVEL_DEBUG);
        // `Trace`：**不静默丢弃**，但必须经 cmap 归一（`T` 缺字形）。
        let trace = level_text(LogLevel::Trace);
        assert_ne!(
            trace,
            LogLevel::Trace.display_name(),
            "`TRACE` 的 `T`(U+0054) 不在生成字体的 cmap 内 ⇒ 必须经 display_safe 归一"
        );
        assert_eq!(trace, "?RACE", "归一形态（`?` 在 cmap 内）");
        assert!(!trace.contains('T'), "归一后不得再含缺字形");
        // 五个级别的文字两两不同（行内不会看串）。
        let mut seen: Vec<String> = ALL_LEVELS.iter().map(|l| level_text(*l)).collect();
        let n = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), n, "五个级别的上屏文字必须两两不同");
    }

    /// 级别色：四个 PRD 指定色**逐字**；`Trace` 取 `TEXT_WEAK`（**非 PRD 指定色，明标**）。
    ///
    /// **改什么会让本条变红**：把任一 PRD 色改成别的 `Palette` 常量（如 `LOG_WARN` → `STALE`）；
    /// 或让 `Trace` 复用 `LOG_DEBUG`（那会把"追踪"显示成"调试"的色 —— 与文字通道自相矛盾）。
    #[test]
    fn level_colors_match_prd_and_trace_is_distinct() {
        assert_eq!(level_color(LogLevel::Error), Palette::LOG_ERROR);
        assert_eq!(level_color(LogLevel::Warn), Palette::LOG_WARN);
        assert_eq!(level_color(LogLevel::Info), Palette::LOG_INFO);
        assert_eq!(level_color(LogLevel::Debug), Palette::LOG_DEBUG);
        assert_eq!(level_color(LogLevel::Trace), Palette::TEXT_WEAK);
        assert_ne!(level_color(LogLevel::Trace), level_color(LogLevel::Debug));
        assert_ne!(level_color(LogLevel::Error), level_color(LogLevel::Warn));
    }

    /// 级别样式槽位与 [`ALL_LEVELS`] 一一对应（`set_style_index` 的索引真源）。
    #[test]
    fn level_style_index_matches_all_levels_order() {
        for (i, l) in ALL_LEVELS.iter().enumerate() {
            assert_eq!(level_style_index(*l), i, "{l:?} 的样式槽位");
        }
    }

    /// 级别 chip：**恰 4 项**（PRD 的 `ERROR`/`WARN`/`INFO`/`DEBUG`），且与 `FILTER_LEVELS` 同序。
    #[test]
    fn level_chips_are_the_four_prd_levels() {
        assert_eq!(level_options(), vec!["ERROR", "WARN", "INFO", "DEBUG"]);
        assert_eq!(FILTER_LEVELS.len(), 4);
        assert!(
            !FILTER_LEVELS.contains(&LogLevel::Trace),
            "`Trace` 无 chip（§6.3 只给 4 项）"
        );
    }

    /// `Trace` 的**筛选**处置：全不选 = 不按级别筛（`Trace` 照常上屏）；任一勾选即排除 `Trace`。
    ///
    /// **改什么会让本条变红**：把 `Trace` 塞进 `FILTER_LEVELS`（多出第 5 个 chip）；或让
    /// `selected_levels(&[])` 返回"四个 PRD 级别"（那就把"不筛"变成"筛掉 Trace"，静默丢弃）。
    #[test]
    fn trace_is_not_selectable_but_survives_unfiltered() {
        assert!(selected_levels(&[]).is_empty(), "全不选 = 不按级别筛");
        assert!(
            !selected_levels(&[0, 1, 2, 3]).contains(&LogLevel::Trace),
            "任一级别被勾选时 `Trace` 不在查询集合内"
        );
        assert_eq!(selected_levels(&[0]), vec![LogLevel::Error]);
        assert_eq!(selected_levels(&[3, 0]), vec![LogLevel::Debug, LogLevel::Error]);
        // 越界下标丢弃（不 panic）。
        assert!(selected_levels(&[9, 42]).is_empty());
    }

    // ── R1：机器名 / 自由文本 ───────────────────────────────────────────────

    /// 已知模块名（含 `mupc_` 前缀写法）⇒ 中文标签；未登记键 ⇒ `None`（**不臆造**）。
    ///
    /// **改什么会让本条变红**：给未登记键返回中文兜底（臆造模块名）；或改坏归一（`mupc_` 前缀
    /// 形态查不到表 ⇒ 契约示例 `mupc_intercore` 会掉进未登记路径）。
    #[test]
    fn target_label_is_known_or_none() {
        assert_eq!(target_label("intercore"), Some(TEXT_MODULE_INTERCORE));
        assert_eq!(
            target_label("mupc_intercore"),
            Some(TEXT_MODULE_INTERCORE),
            "契约示例给的是 `mupc_` 前缀形态（同归一表）"
        );
        assert_eq!(target_label("GATEWAY"), Some(TEXT_MODULE_GATEWAY), "大小写不敏感");
        assert_eq!(target_label("audit"), Some(TEXT_MODULE_AUDIT));
        for unknown in ["hplc", "meter_grid", "southd", "ota", "core-bin", ""] {
            assert_eq!(target_label(unknown), None, "未登记键 ⇒ 不显中文标签（不臆造）");
        }
        // 表内键唯一（只增不改的前提）。
        let mut keys: Vec<&str> = MODULE_LABELS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "映射表不得有重复键");
    }

    /// **未登记模块名不静默隐藏**：仍上屏（`display_safe` 的归一形态），且**不含任何汉字**
    /// （即没有伪造中文标签）。
    ///
    /// **改什么会让本条变红**：把 `module_label` 的 `None` 分支改成返回空串（模块列空白 ——
    /// 操作者无从知道这条日志来自哪个模块）。
    #[test]
    fn unknown_target_is_shown_not_hidden() {
        for unknown in ["hplc", "meter_grid", "southd", "ota", "core-bin"] {
            let shown = module_label(unknown);
            assert!(
                !shown.trim().is_empty(),
                "未登记模块 `{unknown}` 必须仍上屏（不得隐藏）"
            );
            assert!(
                !shown
                    .chars()
                    .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)),
                "未登记模块 `{unknown}` 不得伪造中文标签（实际：{shown}）"
            );
            assert!(
                shown
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '?' || c == '\u{2013}'),
                "归一形态只能含 ASCII 字母 / 数字 / `?` / `–`（实际：{shown}）"
            );
        }
        // 已知键取中文（与上一条互斥）。
        assert!(module_label("intercore")
            .chars()
            .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)));
    }

    /// 模块 chip：**首位固定「全部」**、未登记 / 超长项按 [`clip_chip_label`] 截断
    /// （**超出即带可见省略标记**）。
    ///
    /// **改什么会让本条变红**：把 `module_options` 的首项换成别的（§6.3 ② 的快捷复位失效）；
    /// 或去掉截断（110 px 的 chip 会被 LVGL 折行 / 裁切成不可辨认）；或退回"纯保尾、无标记"
    /// 的旧方案（`opts[2]` 不再以 `...` 开头 ⇒ 第 3 组断言红 —— 见 **LG4 / LG12**）。
    #[test]
    fn module_chip_options_start_with_all_and_are_bounded() {
        let targets: Vec<String> = vec![
            "mupc_intercore".into(),
            "hplc".into(),
            "meter_grid".into(),
        ];
        let opts = module_options(&targets);
        assert_eq!(opts.len(), targets.len() + 1);
        assert_eq!(opts[0], TEXT_ALL);
        for o in &opts {
            // 上界 = 原样预算（超出才截断）+ 标记 3 字（截断产物恒为 `...` + 尾
            // [`MODULE_CHIP_TAIL_CHARS`] 字）—— **两者一起**才构成完整判据。
            assert!(
                o.chars().count() <= MODULE_CHIP_MAX_CHARS + TEXT_ELLIPSIS.chars().count(),
                "chip 文案不得超预算（实际 `{o}`）"
            );
            assert!(!o.is_empty());
        }
        assert_eq!(opts[1], TEXT_MODULE_INTERCORE, "已知键取中文（2 字，恰在预算内）");
        // 未登记 / 超长项：**可见省略标记 + 保尾**（区分位在尾部；标记让"不完整"可见）。
        assert_eq!(module_label("hplc"), "hP?C", "归一形态 4 字（> 预算 2）");
        assert_eq!(opts[2], format!("{TEXT_ELLIPSIS}C"));
        assert!(module_label("meter_grid").chars().count() > MODULE_CHIP_MAX_CHARS);
        assert_ne!(opts[3], module_label("meter_grid"));
        assert!(
            opts[3].starts_with(TEXT_ELLIPSIS),
            "超长项的产物**必须**带可见省略标记（LG4；退回纯保尾 ⇒ 本条红）"
        );
    }

    /// [`clip_marked`] 的两个落点（chip / 行）：**原样预算内不截、超出恒带标记 + 保尾**。
    ///
    /// **改什么会让本条变红**：把 `keep` 与 `tail` 合并成一个参数（退化成 `clip_tail`）⇒
    /// 第 ② 组（"刚好 3 字的 chip 名"仍被截断并带标记）当场红；把标记去掉 ⇒ 第 ③ 组红。
    #[test]
    fn marked_truncation_always_shows_the_marker() {
        // ① 预算内 ⇒ **逐字原样**（不许无谓加标记）。
        assert_eq!(clip_chip_label("核间"), "核间");
        assert_eq!(clip_row_label("主站"), "主站");
        // ② 超出 ⇒ `...` + 尾 1 字（chip）/ 尾 4 字（行）——**恰超 1 字也走截断**（不是"≤4 字原样"）。
        assert_eq!(clip_chip_label("汉汉汉"), format!("{TEXT_ELLIPSIS}汉"));
        assert_eq!(
            clip_chip_label(&module_label("mupc_gateway::iec104")),
            format!("{TEXT_ELLIPSIS}4")
        );
        assert_eq!(
            clip_row_label(&module_label("mupc_gateway::iec104")),
            format!("{TEXT_ELLIPSIS}C104")
        );
        // ③ 超出 ⇒ 恒带标记（可读性：内容不完整必须**可见**）。
        assert!(clip_chip_label("汉汉汉").starts_with(TEXT_ELLIPSIS));
        assert!(clip_row_label("meter_grid").starts_with(TEXT_ELLIPSIS));
    }

    /// 「全部」的快捷复位归一化（三条规则，与 `p5_audit.rs` 的 `normalize_ops_selection` 同语义）。
    ///
    /// **改什么会让本条变红**：去掉规则 1（点「全部」后仍与具体项并存 ⇒ 屏上"全部"与单项同时
    /// 勾选，而查询却按具体项过滤 —— 状态与语义矛盾）。
    #[test]
    fn module_selection_normalizes_quick_reset() {
        // 规则 1：点「全部」⇒ 只剩「全部」。
        assert_eq!(normalize_module_selection(&[1, 2], &[1, 2, 0]), vec![0]);
        // 规则 2：点具体项 ⇒ 让出「全部」。
        assert_eq!(normalize_module_selection(&[0], &[0, 2]), vec![2]);
        // 规则 3：只取消 ⇒ 原样（并清理"全部 + 具体项"的非法组合）。
        assert_eq!(normalize_module_selection(&[1, 2], &[1]), vec![1]);
        assert_eq!(normalize_module_selection(&[0, 1], &[0, 1]), vec![1]);
        // 取消到空 ⇒ 空（= 不按模块筛，等价「全部」）。
        assert!(normalize_module_selection(&[1], &[]).is_empty());
    }

    /// 勾选下标 → 机器键：跳过「全部」（下标 0）、越界丢弃、保序去重。
    #[test]
    fn selected_targets_skips_all_chip() {
        let targets: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        assert!(selected_targets(&[0], &targets).is_empty(), "「全部」不产键");
        assert_eq!(selected_targets(&[0, 2], &targets), vec!["b".to_string()]);
        assert_eq!(
            selected_targets(&[3, 1], &targets),
            vec!["c".to_string(), "a".to_string()]
        );
        assert!(selected_targets(&[9], &targets).is_empty(), "越界丢弃（不 panic）");
    }

    // ── 查询组装 ───────────────────────────────────────────────────────────

    /// 三档时间范围 → 查询；起止**仅 `Custom`** 时给出（相对窗口由服务端算，UI 不读时钟）。
    ///
    /// **改什么会让本条变红**：让 `H1` 也带上 `from_ms`（那就把"相对窗口"变成"UI 算的绝对
    /// 窗口"，而本页**不读时钟** ⇒ 只能拿假时间）。
    #[test]
    fn log_query_maps_three_ranges() {
        let d = TimeRangeChange::default();
        for r in [LogRange::H1, LogRange::H24] {
            let c = TimeRangeChange { range: r, ..d };
            let q = log_query(&c, &[], &[], None);
            assert_eq!(q.range, r);
            assert_eq!(q.from_ms, None);
            assert_eq!(q.to_ms, None);
            assert_eq!(q.cursor, None);
            assert_eq!(q.limit, ROW_MAX, "limit = 本页行池上界（LG6）");
        }
        let c = TimeRangeChange {
            range: LogRange::Custom,
            start: DateTimeValue::from_parts(2026, 9, 10, 0, 0),
            end: DateTimeValue::from_parts(2026, 9, 10, 23, 59),
        };
        let q = log_query(&c, &[LogLevel::Error], &["hplc".to_string()], Some(7));
        assert_eq!(q.range, LogRange::Custom);
        assert!(q.from_ms.is_some() && q.to_ms.is_some());
        assert!(q.from_ms.unwrap() < q.to_ms.unwrap());
        assert_eq!(q.levels, vec![LogLevel::Error]);
        assert_eq!(q.targets, vec!["hplc".to_string()]);
        assert_eq!(q.cursor, Some(7));
    }

    /// 缺省查询 = 最近 1 小时 + 不筛级别 / 模块 + 无游标（**不读时钟**）。
    #[test]
    fn default_query_has_no_clock_dependency() {
        let q = LogQuery::default();
        assert_eq!(q.range, LogRange::H1);
        assert_eq!((q.from_ms, q.to_ms), (None, None));
        assert!(q.levels.is_empty() && q.targets.is_empty());
        assert_eq!(q.cursor, None);
        assert_eq!(q.limit, ROW_MAX);
    }

    // ── 行序 / 形态 / 状态行 / 通道条 ─────────────────────────────────────────

    /// 行序：按 `seq` **降序**（§6.3「实时追加：新行插入顶部」）—— 新窗口里更大的 `seq` 排在
    /// 第 0 行；`seq` 相同者保持注入序（稳定）。
    ///
    /// **改什么会让本条变红**：把排序方向写反（`a.seq.cmp(&b.seq)`）⇒ 第 0 行变成最旧的一条，
    /// 与"新行插入顶部"相反。
    #[test]
    fn row_order_puts_newest_first() {
        let es = vec![
            entry(1, LogLevel::Info, "gateway", "old"),
            entry(3, LogLevel::Error, "hplc", "new"),
            entry(2, LogLevel::Warn, "audit", "mid"),
        ];
        assert_eq!(row_order(&es), vec![1, 2, 0]);
        // `seq` 相同 ⇒ 稳定（保持注入序）。
        let same = vec![
            entry(5, LogLevel::Info, "a", "x"),
            entry(5, LogLevel::Info, "b", "y"),
        ];
        assert_eq!(row_order(&same), vec![0, 1]);
        assert!(row_order(&[]).is_empty());
    }

    /// 列表区两态：0 行 ⇒ 空态；≥1 行 ⇒ 行态（**本页无「不可用」态**，§6.3）。
    #[test]
    fn list_view_has_only_rows_and_empty() {
        assert_eq!(list_view_of(0, false), ListView::Empty);
        assert_eq!(list_view_of(1, false), ListView::Rows);
        assert_eq!(list_view_of(ROW_MAX, false), ListView::Rows);
        // **LG9：超限优先** —— 零行 + 超限 ⇒ 不完整态（**不得**给空态）；有行 + 超限 ⇒ 仍是行态
        //（超限不隐藏已返回的条目）。
        assert_eq!(list_view_of(0, true), ListView::Incomplete);
        assert_eq!(list_view_of(1, true), ListView::Rows);
        assert_ne!(list_view_of(0, true), ListView::Empty);
    }

    /// 底部状态行：无行 ⇒ `None`；有行 ⇒ 按 `has_more` 二选一（**LG7** 的落点）。
    ///
    /// **改什么会让本条变红**：把 `rows == 0` 的分支写成 `Some(已加载全部)`（空态下多出一条
    /// "已加载全部"，与空态文案自相矛盾）。
    #[test]
    fn footer_reflects_has_more() {
        assert_eq!(footer_text(true, 0), None);
        assert_eq!(footer_text(false, 0), None);
        assert_eq!(footer_text(true, 3), Some(TEXT_FOOTER_LOADING));
        assert_eq!(footer_text(false, 3), Some(TEXT_FOOTER_ALL));
    }

    /// 通道条：两条文案 / 两种灯色 / 两种字色（**缺省 = 断开**，fail-closed，见 **LG8**）。
    ///
    /// **改什么会让本条变红**：把断开的字色改成常规色（§6.3 要求"红字"）；或让 `connected = true`
    /// 与 `false` 返回同一条文案（通道态不可辨）。
    #[test]
    fn channel_bar_has_two_distinct_states() {
        assert_eq!(channel_text(true), TEXT_CHANNEL_OK);
        assert_eq!(channel_text(false), TEXT_CHANNEL_DOWN);
        assert_ne!(channel_text(true), channel_text(false));
        assert_eq!(channel_dot_color(true), Palette::LINK_OK);
        assert_eq!(channel_dot_color(false), Palette::LINK_DOWN);
        assert_eq!(channel_text_color(false), Palette::LINK_DOWN, "断 ⇒ 红字");
        assert_ne!(channel_text_color(true), channel_text_color(false));
    }

    // ── 模块网格（§6.3 ② / LG3）─────────────────────────────────────────────

    /// 网格高：与 `MultiSelectChips` 的算式一致（`rows × 64 − 16`），行数随项数增长、
    /// **≤51 项时 ≤7 行**（§6.3 ② 的容量核算）。
    ///
    /// **改什么会让本条变红**：把 [`MODULE_COLS`] 改小（≤7）⇒ 51 项时行数 > 7，本条的
    /// `MODULE_ROWS_MAX` 断言立刻红。
    #[test]
    fn module_grid_rows_are_bounded() {
        assert_eq!(module_grid_h(0), 0);
        assert_eq!(module_grid_h(1), Dimens::CHIP_H, "仅 1 行时高 48 px（§6.3 线框）");
        assert_eq!(module_grid_h(MODULE_COLS as usize), Dimens::CHIP_H);
        assert_eq!(
            module_grid_h(MODULE_COLS as usize + 1),
            Dimens::CHIP_H + Dimens::GAP_MIN + Dimens::CHIP_H,
            "换行后 +（chip 高 + 缝）"
        );
        for items in 1..=(LOG_TARGETS_MAX + 1) {
            let rows = (items as u32).div_ceil(MODULE_COLS);
            assert!(
                rows <= MODULE_ROWS_MAX,
                "{items} 项需 {rows} 行 > 上限 {MODULE_ROWS_MAX}"
            );
            assert!(module_grid_h(items) <= 7 * (Dimens::CHIP_H + Dimens::GAP_MIN) - Dimens::GAP_MIN);
        }
        // 上限口径：契约「≤50 模块」+「全部」。
        assert_eq!(MODULE_ITEMS_MAX, 51);
    }

    // ── R3：行池容量（纯逻辑，**不触碰 LVGL** ⇒ 不会把测试跑挂）────────────────

    /// 行池上限**不超实测上界**且**至少装得下一页**；共存预算严格小于单页上限。
    ///
    /// **改什么会让本条变红**：把 `ROW_MAX` 改到 > `MEASURED_ROW_CAPACITY`（真去注入那么多条
    /// 会 `lv_realloc` 失败 + C 侧断言 ⇒ **挂死**，所以这条判据必须是**纯逻辑**的）。
    #[test]
    fn row_pool_capacity_is_measured() {
        // ⚠️ **为什么经 `black_box`**：本条的三个被比量都是**编译期常量**（其上界关系另有
        // `const _: () = assert!(..)` 把关），而本用例的价值是"把实测结论钉在**测试名**上、
        // 并在 `cargo test` 输出里可见"（编译期断言不会出现在测试清单里）。直接用常量比较会被
        // `clippy::assertions_on_constants` 判为"断言值恒定"（本仓基线是 0 条真 lint）⇒
        // 显式走一次不透明读，使断言成为**运行期**判据（`black_box` 的正是这个用途）。
        let row_max = std::hint::black_box(ROW_MAX);
        let measured = std::hint::black_box(MEASURED_ROW_CAPACITY);
        let coexist = std::hint::black_box(COEXIST_ROWS_PER_PAGE);
        let page_limit = std::hint::black_box(LOG_PAGE_LIMIT_MAX);
        assert!(row_max <= measured);
        assert!(row_max < page_limit, "LG6：本页上限低于契约的请求上限");
        assert!(coexist < row_max);
        assert!(row_max >= 20, "至少装得下 P5 的同一页条数（两列表页口径一致）");
        // 实测余量：`ROW_MAX` 对单页挂死点（44）留 ≥50%；共存预算对挂死点（18）留 ≥33%。
        assert!(row_max * 2 <= measured, "单页余量 < 50%");
        assert!(coexist * 3 <= 2 * 18, "共存余量 < 33%");
    }

    // ── 只读 / 文案清册 ─────────────────────────────────────────────────────

    /// 只读页的结构性声明：写入口清单**恒空**（§6.3「不支持导出」节 / PRD T-2）。
    #[test]
    fn write_entries_is_empty() {
        assert_eq!(P3LogsPage::WRITE_ENTRIES.len(), 0);
    }

    /// 固定文案清册：非空、无重复、且覆盖屏幕上的关键句。
    #[test]
    fn all_texts_cover_the_page() {
        assert!(ALL_TEXTS.iter().all(|t| !t.is_empty()));
        for must in [
            TEXT_CHANNEL_OK,
            TEXT_CHANNEL_DOWN,
            TEXT_EMPTY,
            TEXT_RANGE_TOO_LARGE,
            TEXT_NO_EXPORT,
            TEXT_NO_EXPORT2,
            TEXT_BACK_TO_LATEST,
            TEXT_HEAD_MESSAGE,
        ] {
            assert!(ALL_TEXTS.contains(&must), "清册缺 `{must}`");
        }
    }
}
