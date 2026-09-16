//! # `ui/pages/p5_audit.rs` —— P5 审计页（12-MUPC v2.0 工作单元 **B2c-1**，**只读页**）
//!
//! 出处：UI 设计文档 §6.5「P5 审计页（F19，只读）」（线框 + 区块规格表 + 行内容 + 只读约束）、
//! §8.3 的三行专行（`审计源不可用（EDGE-17）` / `日志为空 / 筛选无结果（EDGE-08）` /
//! `检索范围超限（EDGE-15）`）、§5.1 #5/#10/#13/#17/#18（`MultiChipGroup` / `ListItem` /
//! `StatusChip` / `EmptyState` / `UnavailableState`）、§5.2（Chip 选中态 + `✓` 前缀）、
//! §5.3（`MultiChipGroup` / `EmptyState` / `UnavailableState` 行）、§3.6 **P5 行**（上屏文案
//! 唯一真源）、§3.2 色板、§3.3 字号、§3.5 栅格；技术设计 §6.5（P5 环节表）与 §4.5（审计 schema）。
//!
//! ## 本页的边界（与 `p2_config.rs` / `p4_interlock.rs` 同类，但**更窄**）
//!
//! 1. **只读页 —— 零写操作**：UI §6.5「只读约束」行 / PRD PL-02（审计仅追加、不可删改）在
//!    界面上体现为：**不存在**编辑 / 删除 / 清空 / 导出的按钮、图标、菜单项或手势入口。
//!    本文件**不构造**任何按钮 / 弹层 / Toast（唯一可点控件是筛选区的分段控件、多选 Chip 与两个
//!    `DateTimeStepper` 的 `−`/`＋`）；由 [`P5AuditPage::WRITE_ENTRIES`]（**恒空**）+
//!    `ui/tests.rs::p5_static_constraints`（文件名:行号级 token 扫描）+
//!    [`P5AuditPage::list_clickable_count`]（**运行期**读 LVGL 标志）三路锁住。
//! 2. **数据来自控制通道**（`GET /v1/console/audit`、`/v1/console/audit/ops`，设计 §3.4）
//!    ⇒ **不走 1 Hz 显示帧**，注入入口是 [`P5AuditPage::set_page`] / [`P5AuditPage::set_ops`] /
//!    [`P5AuditPage::set_unavailable`]（契约 2′，与 P2 同款）。
//! 3. **本页不发请求、不生成 `request_id`**：筛选变化 / 「加载更多」以**意图回调**交给外部
//!    （B3 的 `console.rs`）—— [`P5AuditPage::set_on_query`] / [`P5AuditPage::set_on_load_more`]，
//!    载荷见 [`AuditQuery`]。**同一意图不重复发**（见 `Core::fire_query` 的去重口径）。
//! 4. **本页不读时钟**：时间戳文本一律由**注入的 `ts_ms`** 经
//!    [`crate::ui::pages::format_epoch_ms_utc`] 生成；`newest_ts_ms = None` ⇒ 显占位符
//!    （**绝不编造时间**，见 [`newest_text`] 与 **AU9**）。
//! 5. **页根 = 滚动容器**（`ui/pages/mod.rs` 契约 1 适用：§6.5 线框**没有**底部固定操作条，
//!    整页纵向滚动 ⇒ 走 [`crate::ui::pages::page_root`]）。
//!
//! ## ⚠️ 已知偏差登记（**独立编号 `AU`** —— 不与 `D*`（页面层）/ `CD*`（`ui/controls.rs`）/
//! `PD*`（`p2_config.rs`）/ `IL*`（`p4_interlock.rs`）/ `FR*`（`filters.rs`）冲突）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | AU1 | **全角标点一律改写**：`，`(U+FF0C) / `：`(U+FF1A) **实测不在生成字体 cmap 内** ⇒ 取 cmap 内的 `·`(U+00B7) 或 ASCII `:`。受影响串逐条：`审计记录仅追加，不可修改或删除` → `审计记录仅追加 · 不可修改或删除`；`检索范围超限，请缩小时间范围` → `检索范围超限 · 请缩小时间范围`；`最近一条审计：X` → `最近一条审计: X`；`原因：X` → `原因: X`；`端口：2404 → 2405` → `端口: 2404 → 2405` | 字库资产（`gen_fonts.sh` + `extract_charset.py`）本轮按 PM 裁定**不动**（与 `pages/mod.rs` 的 **D4** / `p4_interlock.rs` 的 **IL1** 同一口径） | **B2c 之后**的「字体码表 + 文案统一收口批」：扩 §3.6 字符集后**逐字改回契约原文** |
//! | AU2 | 结果胶囊图标：`● 成功` 的 `●` 在 cmap 内（§3.6 符号集），失败串 UI 写 `✕ 失败` 而 `✕`(U+2715) / `✗`(U+2717) **实测不在** cmap 内 ⇒ 取 `×`(U+00D7，§3.6 声明的符号集内、几何等价) | 同 AU1（字库缺口）；与 `p4_interlock.rs` 的 **IL2** 同款处置（「停机失败」亦取 `×`） | 同 AU1 |
//! | AU3 | 操作类型标签取**契约** `ConsoleOp::label()`（`配置保存` / `恢复默认值` / `联锁释放` / **`M1 授权`**），**未**取 §3.6 P5 行的 `M1 授权重启`（差 2 字） | 契约 `ConsoleOp::label()` 是**唯一机器可读的真源**（`/audit/ops` 的 `label` 亦由它生成）；§3.6 P5 行给的是"该页用字集合"。**不**在 UI 层另造一份映射（那会得到"后端标签与屏上标签各一份"的第二真源） | 若 PM 裁定 §3.6 P5 行走字优先：改 `display-proto` 的 `ConsoleOp::label()`（**一处**），UI 自动跟随 |
//! | AU4 | **不可篡改说明条的锁形取 `■`**（U+25A0） | UI §3.6 在"非中文字形"清单里**明写**「`🔒`（**以几何锁形替代**）」—— 即设计本身要求用几何形状替代 emoji；cmap 内可用的几何字形只有 `● ○ ■ ▲ ▼ ⚠ ✓ ×` ⇒ 取 `■`（实心块，与 `UnavailableKind` 的 `?`、空态的 `○` **不同族**） | 无（**按 §3.6 原文**）；若 PM 要求更"锁"的语义，需扩字表 |
//! | AU5 | **EDGE-15（检索范围超限）在 P5 侧生产不可达** —— `AuditPage` **没有** `range_too_large` 字段（对照：`LogPage` 有，见 `display-proto/src/log.rs`），而 §8.3 的 EDGE-15 行明写适用「P3 / **P5**」 | **契约缺口**（`display-proto` 本批冻结、不得改）。**本页不造字段**，而是把 **UI 落点**备齐：`WarnBanner` **懒惰构建**（首个 [`P5AuditPage::set_range_too_large`]`(true)` 才建 —— 代码质量评审 **M5**：既然生产不可达，就不该让每页常驻背着一份"永不显形"的构件）、由显式注入驱动并**逐条可测**。⚠️ **残余（如实）**：B3 **无法**从当前 `AuditPage` 推出该标志 ⇒ 生产路径上该 banner **永不可达** | 契约侧给 `AuditPage` 增 `range_too_large: bool`（与 `LogPage` 同口径：**必需**字段、缺失即 `Err`）；届时 [`P5AuditPage::set_page`] 直接读该字段（**单一分派点**），[`P5AuditPage::set_range_too_large`] 退化为测试入口 **✅ PM 已裁定（2026-09-15）**：契约**不改**；UI §8.3 的 EDGE-15 适用范围由「P3 / P5」**收窄为 P3**（「单次最多扫 5 个日志文件 / 5 万行」是**日志**检索语义，审计页按 `page`/`page_size` 分页、本无超限概念）。本页的 `set_range_too_large` 注入入口**保留**为测试入口，见 UI 附录 **A.7** |
//! | AU6 | **「滚动加载」的触发点在本层不可得** ⇒ 本页提供 [`P5AuditPage::request_next_page`] 作为**触发入口**（由外壳在"列表滚到底"时调用），本页据最近一次注入的 `AuditPage.page` / `has_more` 组装 `page + 1` 的 [`AuditQuery`] 交回外部 | 薄层的 `EventCode` **未镜像** `LV_EVENT_SCROLL`（镜像的事件码实为 `PRESSED` / `PRESS_LOST` / `RELEASED` / `CLICKED` / `LONG_PRESSED` / `LONG_PRESSED_REPEAT` / `VALUE_CHANGED` / `READY` / `CANCEL` / `DELETE` —— **含** `PRESS_LOST` 与 `LONG_PRESSED_REPEAT`，**不含** `SCROLL`），而 `src/lvgl/**` 本批**禁改** ⇒ "检测滚到底"在本单元**结构性不可实现**。故：**意图**由本页组装（含去重所需的 `page`）、**触发与节流**由 B3 承担（与任务书"滚动加载的触发与去重由 B3 负责"一致） | B3 接线时：在滚动容器上挂 `LV_EVENT_SCROLL`（需先在 `src/lvgl/event.rs` 镜像该事件码）后调 `request_next_page()`；或由 B3 自行节流后调用 |
//! | AU7 | **操作类型 chip 组占 2 行（块高 112 px），不是设计线框的一行（64 px）** ⇒ 其下方（表头 / 列表）整体下移 48 px | **根因（结构性，非取舍）**：5 个选项里最长的 `恢复默认值` 在 26 px 档实测 **130 px**，选中态再拼 `✓ ` 前缀（**153.6 px**，UI §5.2 要求）⇒ 单 chip 至少 **192 px**（= 153.6 + 按钮内边距 2×16）；5 × 192 + 4 × 16 = **1024 > 992** ⇒ **单行装不下**（即使把标签列挪到上一行，5 项一行仍需 1024 px）。故取 `columns = 4` 的两行网格（换行能力见 §5.3） | 无（**结构性**）；若 PM 要求单行：需缩短选项文案（改契约 `label()`）或缩小 chip 内边距（`theme.rs`） |
//! | AU8 | 行池上限 **20**（= [`AUDIT_PAGE_SIZE`]，**恰为一页**）：注入超过 20 条时**只渲染前 20 条**（按时间倒序取最新的一页）；**两页共存**时每页只保证 [`COEXIST_ROWS_PER_PAGE`]（= 4）行 | **测量前提（必须与数字一起读）**：`lvgl-sys/lv_conf.h` 的 `LV_MEM_SIZE` = **256 KB**，且离屏链里各页共用**同一个 LVGL 堆**。**① 单页独活**（`pages_chain` 串行建 / 拆六页，逐个 `drop`；此前 `ROW_MAX = 100` 的承诺即在此情形下被证伪）：逐档实测 **N=20 成功、N=22 成功、N=24 即 `lv_realloc: couldn't reallocate memory` + `lv_array_resize` 断言 ⇒ 挂死**（24 复现两次；顺序注入 20→22→24→**26 挂死**）⇒ 单页上界 ≈ **22–24 行** ⇒ 记 [`MEASURED_ROW_CAPACITY`]=22，`ROW_MAX` 收敛到 20（= §6.5 每页条数，对挂死点留 ≥16% 余量）。**② 两页共存**（B2c-1 整改实测 2026-09-13；P3 尚未实现 ⇒ 用两个 P5 实例作代理）：两个**空**页成功；**2 页 × 4 行 / × 5 行成功、2 页 × 6 行即 OOM 挂死**；**单页满行(20) + 第二个空页也 OOM** ⇒ 空页本身 ≈ **12 行**的开销、两页共存的**总**行数上界 ≈ [`MEASURED_COEXIST_TOTAL_ROWS`]=10。**结论（如实）**：256 KB 下**两页各满行（20）不可能**；把 `ROW_MAX` 收敛到"共存可容纳值"（≈5）**也买不到共存**（留给页开销的位置为 0、无任何余量，且等于**连带砍掉单页契约**）⇒ `ROW_MAX` **保持 20**，共存预算另立 [`COEXIST_ROWS_PER_PAGE`]=4（`COEXIST_ROWS_PER_PAGE < ROW_MAX` 由编译期断言钉死）。**与"20 条/页 + 滚动加载"的关系**：本页把 `entries` 视为**外部（B3）组装好的单页窗口** —— 分页 / 累积 / 窗口化在 B3；本页只渲染被喂进来的那一页，**不**自行累加（`apply_page` 是"整体替换窗口"语义，不是 append）。**同一 `Drop` 内建 / 拆 `P5AuditPage` 不泄漏**（实测连拆 30 个空页后仍能建满行页） | **B2c-2（P3 日志页，同样有行池）落地前必须二选一**：① 扩 `LV_MEM_SIZE`（`lv_conf.h`）并**重测两页共存预算**；② 由外壳**串行化页面生命周期**（离开即 `drop` —— 即 `pages_chain` 现模拟的形态）。届时 [`MEASURED_COEXIST_TOTAL_ROWS`] / [`COEXIST_ROWS_PER_PAGE`] 随新实测重新标定，`row_pool_capacity_is_measured` 与 `pages_chain` 的共存预算用例同步更新 |
//! | AU9 | **机器键 / 自由文本的上屏处置**（本单元最高风险项，逐条列在下方「§6 处置表」）：`operator`（`local-console` → `本地控制台`，未知名走 [`free_text_safe`]）、`target`（**已知键映射中文名；未登记键显示经 `display_safe` 归一后的机器键**，见下方"键的处置"）、`before`/`after`（`Value` → 文本，逐类型处置）、`reason`（自由文本 → [`free_text_safe`]） | 三者都**不在** `ui/**` 的源码字面量走查面内（来自 `display-proto` 或运行时），直上屏含 cmap 外 ASCII（小写 / `-`）即**豆腐块**（同 `pages/mod.rs` **D9** / `p4_interlock.rs` **IL6**）。`display_safe` **只改写 ASCII，非 ASCII 缺字挡不住**（残余） | 同 D9（扩字符集后改写面自然收窄）；`target` 的键全集真源 = mupcd 配置服务（见 [`TARGET_LABELS`] 的"只增不改"口径） |
//!
//! **`target` 键的处置（AU9 的整改细则；B2c-1 规格评审 ③，**不得静默隐藏信息**）**：
//!
//! | 键 | 上屏 | 依据 |
//! |----|------|------|
//! | `gateway.port` / `system.log_level` / `telemetry.interval` / `gateway.listen_addr` | 中文标签（[`TARGET_LABELS`]） | §3.6 P2「字段」行；§6.5 行内容示例 |
//! | `interlock.release` / `interlock.ack_m1` | 中文标签 = `ConsoleOp::label()`（**转出契约**：[`INTERLOCK_TARGETS`]） | 契约点名（`display-proto/src/audit.rs` 的 `ConsoleOp` 文档与 `target` 字段注释；设计 §4.5）；两键与两个联锁 `ConsoleOp` **一一对应** ⇒ 标签由 `label()` 转出（**不在 UI 层另抄一份**，与 **AU3** 同口径） |
//! | **未登记键** | **`display_safe(键)` 的保尾截断 + `: ` + 值对**（如 `interlock.flood` → `...OO?: 1 → 2`、`interlock.force` → `...R??: 1 → 2`） | **不得整块不显标签**：原实现只留值对 ⇒ 操作者**无从知道被改的是哪一项**（评审 ③ 的后果）。改走本仓既有的**降级**口径（`p4_interlock.rs` **IL6**：源名未知名走 `display_safe`）—— 保留可辨认的机器键、**不臆造**中文名、经 `display_safe` 保证不出豆腐块。**评审 ④ 整改**：键前缀**保留尾部**（区分位在尾部：`flood` / `force`）、长度由 [`unknown_key_label`] 按**值对实长**分配（上限 [`UNKNOWN_TARGET_MAX_CHARS`]=6、下限 [`UNKNOWN_TARGET_MIN_CHARS`]=5）—— 原先**保头**截断（上界 10）使 `flood` / `force` 屏上**完全相同**，且现实值对 `2404 → 2405` 的后值被挤掉。**残余（如实）**：`display_safe` 只改写 ASCII，且大写可达集有限 ⇒ 产物形如 `...OO?`（`t`/`l`/`c`/`y` 一类落 `?`），**可辨认但不美观**；仅**尾部归一形态相同**的两个键（如 `a.b` / `c.b`）仍会撞形；空键（`""`）不显标签（无标识可显，非隐藏） |
//! | AU10 | 审计行**不画斑马纹**（现状） | **依据（B2c-1 收口 ④ 改写 —— 原理由「§5.2 的斑马纹只落在 P3」不实）**：UI §6.5 的行规格只写「行高 60 px（两行结构）+ 左缘 3 px `#35D0C4` 竖条（**每条都有**）」，其线框亦**未**画斑马纹 ⇒ 本页不做。**但两处口径有张力（如实登记，不是"规范已裁定"）**：§5.2 的 `ListItem` 一行写「斑马纹：偶 `#141F33` / 奇 `#1B2942`」而**未按页面限定**；§5.1 #10 又明写 `ListItem` 用于「日志 / 审计 / 告警 / 触发源」并给出「高 60（审计）」⇒ 同一份设计文档可读成"审计行**也要**斑马纹"。旁证：P1 告警卡同样未用（`p1_status.rs` 对 `SURFACE_ALT` **零引用**） | **请 PM 裁定视觉导则是否统一**（本层**不自行**改实现）：① 若统一为"所有 `ListItem` 都斑马纹" ⇒ 在 `Row::new` 加一条 `theme::surface_alt()` 底（1 处，不影响语义），并同步 §6.5 线框；② 若统一为"仅 P3 日志行" ⇒ 请在 §5.2 的 `ListItem` 行**标注适用页**（当前未限定） **✅ PM 已裁定（2026-09-15）**：维持**不画**；UI §5.2 的 `ListItem` 斑马纹行已标注「**适用范围：仅 P3 日志行**」、§5.1 #10 同步加注，见 UI 附录 **A.7** |
//! | AU11 | 列表底部**常驻一行说明**「`本地屏不支持审计导出` · `无文件与下载通道`」（24 px `text_weak`） | §6.5 的「只读约束」行只写"**无**导出按钮 / 图标或手势"，**没有**画说明行；而 §3.6 **P5 列表行**的用字表里**有**「`无导出`」二字 ⇒ 设计**预留了**该落点。取 P3 §6.3「不支持导出」节的**同款呈现**（说明行常驻可见，避免现场反复尝试），文案随之改成"审计"版 | 无（**有意**）；若 PM 裁定 P5 不显该行，删两个常量与一行 `set_visible` 即可 |
//! | AU12 | `before` / `after` 的**布尔值**取 `开` / `关`；**数组 / 对象**取 `N 条` / `N 字段` | `真`(U+771F) / `假`(U+5047) / `是`(U+662F) / `否`(U+5426) / `项`(U+9879) / `个`(U+4E2A) **逐字实测均不在** cmap 内（已知缺口族，见 `pages/mod.rs` **D4**）；`开` / `关` / `条` / `字` / `段` 均在 cmap 内。**不伪造**：bool 是"开 / 关"这一层语义，数组 / 对象只报**规模**（不把 `[1,2]` 渲染成一串 `?`） | 同 AU1（扩字表后可改成 `真/假`、`N 项`） |
//! | AU13 | 「不可篡改说明条」的样式（底 `Palette::AUDIT_BG` + 左缘 4 px `Palette::SOC_OK`）由**本页**用 `theme` 的**命名常量**组合（`Style::new()` + `set_bg_color(Palette::AUDIT_BG)`），**不是** `theme.rs` 里的现成样式 | `theme.rs` 只提供了两个**色常量**（`Palette::AUDIT_BG` 已按 §6.5 收录），**没有**对应的样式函数；而 `ui/theme.rs` 本批**禁改**。故按"色值只准来自 `theme`（命名常量）"的纪律组合 —— **零裸色值**（`Color::hex` 在本文件零出现，静态网逐条断言）。**并附"应用标记"**（[`Core::immutable_bg`]）：薄层没有"已挂样式读回"通道 ⇒ 页面把**送给 `set_bg_color` 的那个色值**记下来，供 `pages_chain` 断言"页面确实挂了 audit 这一档"（把底色改成 `WARN_BG` 时该断言**真的变红** —— 见 [`P5AuditPage::immutable_skin`]） | `theme.rs` 收口批：上收 `theme::audit_banner()`（本页改为一行调用） |
//! | AU14 | **「加载更多」的入口另设一个意图槽**（[`P5AuditPage::set_on_load_more`]），与「筛选变化」（[`P5AuditPage::set_on_query`]）分开 | 二者**去重口径不同**：筛选变化要与"上次已发查询"逐字段比对（重复点同一段不应重发），而"加载更多"是**显式请求**（同一个下一页可能被外壳多次触发，节流归 B3）。分成两个槽使两条语义**各自可测**，也避免"用 `page` 字段反推意图种类"的脆弱判据 | 无（**有意**）；若 B3 希望单一入口，取其一并在其回调内按 `page` 分派 |
//! | AU15 | **自由文本比 P4 多折一步**：`operator` 未知名与 `reason` 经 [`free_text_safe`]
//! （= [`display_safe`] **+ 六个实测缺字的全角标点折叠**：`，`/`；`/`（`/`）`→`·`，`：`→`:`，`、`→`/`），而 P2 / P4 的同类路径（PD13 / IL17）**只过 `display_safe`** | 两件事：① `display_safe` **只改写 ASCII**，而全角标点是**非 ASCII** ⇒ 直上屏即豆腐块，且它落在**行内**（§8.3 要求失败原因「就地可见」）⇒ 本页把**实测常用**的六个折叠掉（判据写成**码位数值**，理由见 [`fold_fullwidth_punct`]）。② **残余（如实）**：其余全角标点（`？`/`！`/`“”`/`—`/`…`）与**非 ASCII 缺字**（后端直接给中文但用了 cmap 外的字）**仍会是豆腐块**—— 防线在后端字段命名 / 字库（同 **D9** / **AU9**）；且 `display_safe` 的 ASCII 可达集只有 `ABCDEFGIMNOPRSUW` + `hks`（大小写有别）⇒ 自由文本里的 `t`/`T`/`l`/`L` 一类仍落 `?`（可辨但难看） | 同 AU1（扩字表 / 扩 ASCII 子集后，本行的折叠可撤或收窄） |
//! | AU16 | **§5.1 #10「`ListItem` 全行可点」与 §6.5「只读约束」矛盾**（同一份 §5.1 表格里 `ListItem` 的最后一列写"全行可点"） | **取 §6.5**（页面级、且与 PRD PL-02「审计仅追加、不可删改」一致）：§5.1 #10 的"全行可点"是给**有详情页 / 有下钻**的列表用的通用规格；本页是**只读页**、§6.5「只读约束」行明写"**无编辑 / 删除 / 清空 / 导出**按钮、图标或手势"⇒ 行**不得**可点（`list_clickable_count() == 0` 由运行期读 LVGL 标志锁定） | 无（**有意**取页面级规格）；若 PM 裁定 P5 行可点（例如点行看详情），需先补 §6.5 的**详情落点**与文案，再摘掉行上的 `CLICKABLE` 摘除逻辑 |
//! | AU17 | **表头第 4 列用字：§6.5 线框写 `前后值`，§3.6 P5「列表」用字表写 `操作前` / `操作后` / `失败原因`** | **取 §6.5 线框图**：表头是**线框图逐格标注**的（`时间 │ 操作者 │ 操作类型 │ 前后值 │ 结果`），而 §3.6 的 P5 行给的是"**该页用字集合**"（用字表，不代表逐格措辞；`操作前` / `操作后` 在本页**没有**独立的表头落点 —— 本页的值对是"`前后值`"一格内的 `前 → 后`）；§6.5 的**行内容**亦以 `前后值摘要` 表述 | 无（**有意**）；若 PM 裁定 §3.6 用字优先：把 [`TEXT_HEAD_VALUE`] 改成 `操作前` / `操作后` 需先在线框图里把一格拆成两格（**结构变更**，非措辞） |
//!
//! ### §6 处置表：**哪些字段会上屏 + 各自怎么处理**（对应 AU9）
//!
//! | 契约字段 | 上屏？ | 落点 | 处置 | 断言 |
//! |----------|--------|------|------|------|
//! | `ts_ms` | 是 | 行 1 时间列 / 最近审计条 | [`crate::ui::pages::format_epoch_ms_utc`]（**只产数字与 `/` `:`**） | `ui/tests.rs::runtime_formatters_emit_only_cmap_glyphs`（既有）+ `p5_value_texts_emit_only_cmap_glyphs`（本单元新增，逐字查 cmap） |
//! | `operator` | 是 | 行 1 操作者列 | `local-console`（契约 `CONSOLE_OPERATOR`）⇒ 「本地控制台」（§3.6 P5 列表行）；**未知名** ⇒ [`free_text_safe`]（全角标点折叠 + 小写 → 大写同族 + cmap 外 ASCII → `?`，**不伪造中文名**） | `operator_text_maps_known_and_keeps_unknown_ascii_safe` |
//! | `op` | 是 | 行 2 操作类型 | `ConsoleOp::label()`（**已是中文**，在 §3.6 内）；若 `/audit/ops` 给了 label 则优先用它（同一个 `display_safe` 出口） | `op_label_prefers_injected_then_contract` |
//! | `target` | 是 | 行 2 值对的前缀标签 | **已知键**（[`TARGET_LABELS`] + 联锁键 [`INTERLOCK_TARGETS`]，只增不改、逐条有出处）⇒ 中文名 + `: `；**未登记键** ⇒ `display_safe(键)` + `: `（保留可辨认的机器键，**不臆造**中文名、**不静默隐藏**，见 AU9 细则） | `target_label_is_known_set_or_none`、`unknown_target_key_is_shown_not_hidden` |
//! | `before` / `after` | 是 | 行 2 值对 | [`value_text`]：`None` ⇒ 占位符 `–`（**绝不当 0 / 空串**）、字符串 ⇒ `display_safe`、数字 ⇒ 唯一数值出口、bool ⇒ `开`/`关`、数组 / 对象 ⇒ `N 条`/`N 字段`、超长 ⇒ 截断加 `...` | `none_is_placeholder_never_zero`、`value_text_covers_every_json_shape`、`summary_truncates_long_values` |
//! | `reason` | 是（**仅失败行**） | 行 2 原因列 | `原因: ` + [`free_text_safe`]`(trim)`（**AU15**）；空串 / `None` / 非失败 ⇒ **整列不显** | `reason_only_on_failed_rows` |
//! | `result` | 是 | 行 1 结果胶囊 | [`result_text`] / [`result_skin`]（`● 成功` 绿 / `× 失败` 红，见 AU2） | `result_chip_carries_text_and_color` |
//! | `id` / `request_id` | **否** | — | 现场对拍用（设计 §4.5），**不上屏**（§3.6 P5 行无对应字表；且 uuid 是小写 ASCII + `-`） | `machine_only_fields_never_reach_screen` |
//! | `page` / `page_size` / `has_more` | 否（间接） | 底部状态行 | 只驱动 `加载中` / `已加载全部` 与"是否还有下一页" | `footer_reflects_has_more` |
//!
//! **残余局限（如实）**：[`free_text_safe`] 已折叠**六个**实测缺字的全角标点（**AU15**），但
//! 它仍**只改写 ASCII + 这六个标点**；`operator` / `reason` 若带**其它非 ASCII 缺字**（如后端直接给中文
//! 但用了 cmap 外的字），真机仍是豆腐块 —— 防线在后端字段命名 / 字库（同 **D9**）。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use mupc_display_proto::{
    AuditPage, AuditResult, ConsoleAuditEntry, ConsoleOp, LogRange, OpOption, AUDIT_PAGE_SIZE,
    CONSOLE_OPERATOR,
};
use serde_json::Value;

use crate::lvgl::obj::{Obj, ObjFlag};
use crate::lvgl::style::{Color, Opa, Style};
use crate::lvgl::widgets::{Label, LongMode, ScrollContainer};
use crate::lvgl::LvglError;
use crate::ui::components::{
    EmptyState, MultiSelectChips, StatusChip, UnavailableKind, UnavailableState, WarnBanner,
};
use crate::ui::pages::filters::{self, TimeRangeChange, TimeRangeFilter};
use crate::ui::pages::p2_config;
use crate::ui::pages::{
    decor, display_safe, format_epoch_ms_utc, label, layout_box, page_root, set_visible, show_only,
    text_label, CbSlot, PLACEHOLDER, TIGHT_GAP,
};
use crate::ui::theme::{self, ChipSkin, Dimens, Palette, Stroke, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 上屏文案（UI §3.6 **P5 行** + §8.3 三行；**落笔前逐字在 `fonts/lv_font_cmap.txt` 核对**）
//
// 与契约串的偏差逐条登记在文件头 `AU1~AU15`（缺字改写 / 口径 / 尺寸 / 降级 / 自由文本）。
// 码表覆盖率走查见 `ui/tests.rs::ui_texts_covered_by_font_cmap`（基线 = 生成字体的实际 cmap）。
// ═══════════════════════════════════════════════════════════════════════════

/// 最近审计条前缀（UI §3.6 P5「头部」行的 `最近一条审计`；`：`→`:` 见 **AU1**）。
pub(crate) const TEXT_NEWEST_PREFIX: &str = "最近一条审计: ";
/// 不可篡改说明条文案（§3.6 P5「头部」行；`，`→` · ` 见 **AU1**）。
pub(crate) const TEXT_IMMUTABLE: &str = "审计记录仅追加 · 不可修改或删除";
/// 不可篡改说明条的几何锁形（§3.6 明写「以几何锁形替代」；取 `■` 见 **AU4**）。
pub(crate) const TEXT_LOCK_ICON: &str = "■";
/// 操作类型筛选的维度名（§3.6 P5「筛选」行）。
pub(crate) const TEXT_OPS_LABEL: &str = "操作类型";
/// 操作类型 chip 组首位的快捷复位项（§6.5 线框 `[全部]`；语义见 §6.3「`全部` chip 为快捷复位」）。
pub(crate) const TEXT_OPS_ALL: &str = "全部";
/// 表头列名之一（§3.6 P5「列表」行）。
pub(crate) const TEXT_HEAD_TIME: &str = "时间";
/// 表头列名之二。
pub(crate) const TEXT_HEAD_OPERATOR: &str = "操作者";
/// 表头列名之三（= 筛选维度名，**同一串** ⇒ 转出，不另抄字面量）。
pub(crate) const TEXT_HEAD_OP: &str = TEXT_OPS_LABEL;
/// 表头列名之四。
pub(crate) const TEXT_HEAD_VALUE: &str = "前后值";
/// 表头列名之五。
pub(crate) const TEXT_HEAD_RESULT: &str = "结果";
/// 操作者：契约 `CONSOLE_OPERATOR`（`local-console`）的上屏中文名（§3.6 P5「列表」行）。
pub(crate) const TEXT_OPERATOR_LOCAL: &str = "本地控制台";
/// 结果胶囊：成功（`●` 在 cmap 内）。
pub(crate) const TEXT_RESULT_OK: &str = "● 成功";
/// 结果胶囊：失败（`✕` 缺字 ⇒ `×`，见 **AU2**）。
pub(crate) const TEXT_RESULT_FAIL: &str = "× 失败";
/// 失败原因前缀（`：`→`:` 见 **AU1**）。
pub(crate) const TEXT_REASON_PREFIX: &str = "原因: ";
/// 值对分隔（UI §6.5 行内容示例 `2404 → 2405` 的箭头）。
pub(crate) const TEXT_PAIR_ARROW: &str = " → ";
/// 标签与值之间的分隔（ASCII `:`，与 §6.5 示例的 `：` 同义，见 **AU1**）。
pub(crate) const TEXT_LABEL_SEP: &str = ": ";
/// 子句分隔（cmap 内 `·`；替代缺字的全角标点，见 **AU1**）。
pub(crate) const TEXT_CLAUSE_SEP: &str = " · ";
/// 值文本：`true` ⇒ 开（见 **AU12**）。
pub(crate) const TEXT_VALUE_ON: &str = "开";
/// 值文本：`false` ⇒ 关（见 **AU12**）。
pub(crate) const TEXT_VALUE_OFF: &str = "关";
/// 值文本：数组规模量词（`项` 缺字，见 **AU12**）。
pub(crate) const TEXT_UNIT_ITEMS: &str = "条";
/// 值文本：对象规模量词。
pub(crate) const TEXT_UNIT_FIELDS: &str = "字段";
/// 值文本：超长截断标记（`…` 不在 cmap 内 ⇒ 取 ASCII `.`）。
pub(crate) const TEXT_ELLIPSIS: &str = "...";
/// 底部状态行：还有下一页（§3.6 P5「列表」行）。
pub(crate) const TEXT_FOOTER_LOADING: &str = "加载中";
/// 底部状态行：已到末页。
pub(crate) const TEXT_FOOTER_ALL: &str = "已加载全部";
/// 只读说明行前半（§3.6 P5「列表」行的 `无导出`；呈现方式见 **AU11**）。
pub(crate) const TEXT_EXPORT_NOTE: &str = "本地屏不支持审计导出";
/// 只读说明行后半（§3.6 P3「列表 / 状态」行的同款说明）。
pub(crate) const TEXT_EXPORT_NOTE2: &str = "无文件与下载通道";
/// 空态文案（§8.3 EDGE-08 行逐字）。
pub(crate) const TEXT_EMPTY: &str = "当前筛选条件下无审计记录";
/// 超限提示（§8.3 EDGE-15 行逐字 + `，`→` · ` 见 **AU1**；契约缺口见 **AU5**）。
pub(crate) const TEXT_RANGE_TOO_LARGE: &str = "检索范围超限 · 请缩小时间范围";
/// 审计库不可用态文案（§8.3 EDGE-17 行逐字）—— 与 [`UnavailableKind::Audit`]`.title()`
/// **逐字相等**（同源，由单测钉住；此处只为清册 / 断言可见）。
pub(crate) const TEXT_UNAVAILABLE: &str = "审计记录不可用";
/// 已知 `target` 键 → 上屏标签之一：端口（§3.6 P2「字段」行；§6.5 行内容示例即 `端口: 2404 → 2405`）。
pub(crate) const TEXT_FIELD_PORT: &str = "端口";
/// 已知 `target` 键 → 上屏标签之二：日志级别（同上）。
pub(crate) const TEXT_FIELD_LOG_LEVEL: &str = "日志级别";
/// 已知 `target` 键 → 上屏标签之三：遥测上报周期（同上）。
pub(crate) const TEXT_FIELD_TELEMETRY: &str = "遥测上报周期";
/// 空态图标（几何空心圆，§3.6 符号集内；与不可用态的 `?` **不同族**）。
pub(crate) const TEXT_EMPTY_ICON: &str = "○";

/// 本页上屏的**全部固定文案**（供 `ui/pages/mod.rs` 的 `ALL_TEXTS` 清册与
/// `ui/tests.rs::ui_texts_covered_by_font_cmap` 的「清册 ↔ 源码字面量」一致性走查）。
#[cfg(test)]
pub(crate) const ALL_TEXTS: &[&str] = &[
    TEXT_NEWEST_PREFIX,
    TEXT_IMMUTABLE,
    TEXT_LOCK_ICON,
    TEXT_OPS_LABEL,
    TEXT_OPS_ALL,
    TEXT_HEAD_TIME,
    TEXT_HEAD_OPERATOR,
    TEXT_HEAD_OP,
    TEXT_HEAD_VALUE,
    TEXT_HEAD_RESULT,
    TEXT_OPERATOR_LOCAL,
    TEXT_RESULT_OK,
    TEXT_RESULT_FAIL,
    TEXT_REASON_PREFIX,
    TEXT_PAIR_ARROW,
    TEXT_LABEL_SEP,
    TEXT_CLAUSE_SEP,
    TEXT_VALUE_ON,
    TEXT_VALUE_OFF,
    TEXT_UNIT_ITEMS,
    TEXT_UNIT_FIELDS,
    TEXT_ELLIPSIS,
    TEXT_FOOTER_LOADING,
    TEXT_FOOTER_ALL,
    TEXT_EXPORT_NOTE,
    TEXT_EXPORT_NOTE2,
    TEXT_EMPTY,
    TEXT_RANGE_TOO_LARGE,
    TEXT_UNAVAILABLE,
    TEXT_FIELD_PORT,
    TEXT_FIELD_LOG_LEVEL,
    TEXT_FIELD_TELEMETRY,
    TEXT_EMPTY_ICON,
];

// ═══════════════════════════════════════════════════════════════════════════
// 2. 栅格常量（UI §6.5 线框 / 区块规格；**全部由 theme 常量推导**）
//
// 页内坐标 = UI 绝对坐标 − 72（页根贴在 `(SIDE_PAD, HEADER_H)`，契约 1）。
// ═══════════════════════════════════════════════════════════════════════════

// ── 头部三条（UI §6.5 线框 `Y80` / `Y116` / `Y172`）──────────────────────────
/// 最近审计条高（UI 线框 `Y80–116` = 36 px = 正文 24 + 上下各 6）。
///
/// `theme` **没有 6 px 的"内边距"档**（`INTERLOCK_BAR` 是 6 但语义是"卡左缘竖条宽"）⇒ 取
/// `TIGHT_GAP`(8) 与 `SCROLLBAR_MARGIN`(4) 组合，并由下方**编译期断言**钉住契约值 36。
const NEWEST_H: i32 = TextSlot::Body.px() as i32 + TIGHT_GAP + Dimens::SCROLLBAR_MARGIN;
/// 不可篡改说明条高（UI 线框 `Y116–164` = 48 = `Dimens::CHIP_H`）。
const IMMUTABLE_H: i32 = Dimens::CHIP_H;
/// 说明条左缘色条宽（UI §6.5：**左缘 4 px `#35D0C4`** ⇒ 取 `Dimens::ACCENT_BAR`）。
const IMMUTABLE_BAR_W: i32 = Dimens::ACCENT_BAR;
/// 说明条内图标宽（UI §6.5：`LockIcon` 28 px；`theme` 的 `ICON_SM` 即该档）。
const IMMUTABLE_ICON_W: i32 = Dimens::ICON_SM;
/// 表头高（UI 线框 `Y300–336` = 36，与最近审计条同档）。
const TABLE_HEAD_H: i32 = NEWEST_H;
/// 筛选区两行的步进（UI §6.5 区块规格：时间范围 `172–236` / 操作类型 `236–300` ⇒ 64）。
///
/// 用于**编译期**钉住「操作类型行紧接时间范围行」这条线框关系（`layout()` 的实际算式是
/// `y_range + filters::body_h(range) + GAP_MIN`，见下）。
const FILTER_STEP: i32 = Dimens::CHIP_H + Dimens::GAP_MIN;
/// 操作类型 chip 组的块高（2 行 × 48 + 缝 16 = 112；**AU7** 给出单行不可达的推导）。
const OPS_BLOCK_H: i32 = 2 * Dimens::CHIP_H + Dimens::GAP_MIN;

/// 编译期自证：头部三条的高度与 §6.5 线框一致。
const _: () = assert!(NEWEST_H == 36);
const _: () = assert!(IMMUTABLE_H == 48);
const _: () = assert!(TABLE_HEAD_H == 36);
/// 编译期自证：`最近 1 小时` 档下，「操作类型」行紧接「时间范围」行（§6.5 线框 `236`）。
const _: () = assert!(filters::body_h(LogRange::H1) + Dimens::GAP_MIN == FILTER_STEP);
/// 编译期自证：行内两行（32 + 26）放得进 60 px 行高。
const _: () = assert!(LINE1_Y + LINE1_H + LINE2_H <= Dimens::ROW_AUDIT_H);
/// 编译期自证：行 2 三列（操作类型 / 值对 / 原因）互不重叠且不越出内容区。
const _: () = assert!(ROW_SUMMARY_W > 0);
const _: () = assert!(ROW_SUMMARY_X + ROW_SUMMARY_W < ROW_REASON_X);
const _: () = assert!(ROW_REASON_X + ROW_REASON_W <= Dimens::CONTENT_W);
/// 编译期自证：单 chip 容得下"最长选项 + `✓ ` 前缀 + 按钮内边距"（**AU7** 的根因算式：
/// `✓ 恢复默认值` 26 px 档实测 153.6 px + `theme` 按钮左右内边距 2 × 16 = 185.6 ⇒ 取 192）。
const _: () = assert!(OPS_CHIP_W >= 2 * Dimens::GAP_MIN + 154);

// ── 列表行（UI §6.5 区块规格「行内容」；行高 60 = `Dimens::ROW_AUDIT_H`）──────
/// 行左缘竖条宽（UI §6.5：**左缘 3 px `#35D0C4` 竖条，每条都有**）。
const ROW_BAR_W: i32 = Stroke::ALERT;
/// 行内容起点 x（竖条 + 同组缝）。
const ROW_X0: i32 = ROW_BAR_W + Dimens::GAP_MIN;
/// 行内容右界 x。
const ROW_RIGHT: i32 = Dimens::CONTENT_W - Dimens::GAP_MIN;
/// 行 1：时间列宽（`CONTENT_W / 4` = 248；24 px 等宽时间戳实测 224 px ⇒ 放得下）。
///
/// `pub(crate)`：`ui/tests.rs::pages_chain` 用**生产字体的 `adv_w`** 实测「时间戳定长且等宽、
/// 且放得进本列」这条几何锁（评审 ⑤.5）。
pub(crate) const ROW_TIME_W: i32 = Dimens::CONTENT_W / 4;
/// 行 1：操作者列宽（`CHIP_MIN_W × 2`；`本地控制台` 24 px 实测 120 px ⇒ 余量充足，
/// 未知名经 `LongMode::DOTS` 截断）。
///
/// `pub(crate)`：理由同 [`ROW_TIME_W`]（操作者列同样要放得下 §3.6 的定长中文名）。
pub(crate) const ROW_OP_W: i32 = Dimens::CHIP_MIN_W * 2;
/// 行 1：结果胶囊宽（`CHIP_MIN_W`；`● 成功` 实测 77.4 px ⇒ 放得下）。
const ROW_RESULT_W: i32 = Dimens::CHIP_MIN_W;
/// 行 2：操作类型列宽（26 px 档最长标签 `恢复默认值` 实测 130 px ⇒ 取 6 字 = 156 px）。
const ROW_OPTYPE_W: i32 = TextSlot::Label.px() as i32 * 6;
/// 行 2：原因列宽（`CONTENT_W / 3` = 330；自由文本，超长走 `LongMode::DOTS`）。
const ROW_REASON_W: i32 = Dimens::CONTENT_W / 3;
/// 行 1 高（取 `STATUS_CHIP_H`：胶囊 32 > 时间 / 操作者文字 24 ⇒ 以最大者为准）。
const LINE1_H: i32 = Dimens::STATUS_CHIP_H;
/// 行 2 高（26 px 的操作类型行）。
const LINE2_H: i32 = TextSlot::Label.px() as i32;
/// 行 1 上缘（两行 32 + 26 = 58 在 60 px 行内垂直居中 ⇒ 上下各 1）。
const LINE1_Y: i32 = theme::center_offset(Dimens::ROW_AUDIT_H, LINE1_H + LINE2_H);
/// 行 2 上缘。
const LINE2_Y: i32 = LINE1_Y + LINE1_H;
/// 行 1 内 24 px 文字 y（在 32 px 行内居中）。
const LINE1_TEXT_Y: i32 = LINE1_Y + theme::center_offset(LINE1_H, TextSlot::Body.px() as i32);
/// 行 2 内 24 px 文字 y（在 26 px 行内居中）。
const LINE2_TEXT_Y: i32 = LINE2_Y + theme::center_offset(LINE2_H, TextSlot::Body.px() as i32);
/// 行 2 内 26 px 文字 y（= 行 2 顶）。
const LINE2_LABEL_Y: i32 = LINE2_Y;
/// 行 1：操作者列 x。
const ROW_OP_X: i32 = ROW_X0 + ROW_TIME_W;
/// 行 1：结果胶囊 x（贴右界）。
const ROW_RESULT_X: i32 = ROW_RIGHT - ROW_RESULT_W;
/// 行 2：值对列 x。
const ROW_SUMMARY_X: i32 = ROW_X0 + ROW_OPTYPE_W;
/// 行 2：原因列 x（贴右界）。
const ROW_REASON_X: i32 = ROW_RIGHT - ROW_REASON_W;
/// 行 2：值对列宽（到原因列左缘减同组缝）。
///
/// `pub(crate)`：`ui/tests.rs::p5_summary_limit_fits_its_column` 用**生产字体的 `adv_w`** 实测
/// 「[`SUMMARY_MAX_CHARS`] 个汉字」与「§6.5 示例 `端口: 2404 → 2405`」都不越出本列（几何锁）。
pub(crate) const ROW_SUMMARY_W: i32 = ROW_REASON_X - Dimens::GAP_MIN - ROW_SUMMARY_X;
/// 行池上限（见 **AU8**；**单页独活**口径）：**恰一页**（`= AUDIT_PAGE_SIZE = 20`）。
///
/// **不是"随手取 20"**：`LV_MEM_SIZE` = 256 KB，**单页独活**（`pages_chain` 串行建 / 拆六页）
/// 逐档实测 —— 一次性注入 **20 / 22 条成功，24 条即 `lv_realloc` 失败 + `lv_array_resize`
/// 断言挂死**（24 复现两次；顺序注入 20→22→24 通过、**26 挂死**）⇒ 单页可容纳上界 ≈ **22–24
/// 行**。取 20（= §6.5 规定的每页条数）既保证"契约承诺的一页必然渲染得出"，又对挂死点（24）
/// 留 ≥16% 余量。
///
/// ⚠️ **测量前提（B2c-1 整改如实标注）**：上面这条**只在"单页独活"时成立**。两页**共存**时
/// （B2c-2 的 P3 日志页同样有行池）空页本身 ≈ 12 行的开销 ⇒ 两页共存的**总**行数上界只有
/// ≈ [`MEASURED_COEXIST_TOTAL_ROWS`] = 10 行（实测 2 页 × 5 行成功、2 页 × 6 行 OOM 挂死）
/// ⇒ 见 [`COEXIST_ROWS_PER_PAGE`]。**把本值收敛到"共存下可容纳的值"并不能买到共存**
/// （那会让每页只剩 5 行、且距挂死点 0% 余量），故本值**保持 20**、共存预算另立常量。
///
/// **改什么会让本条变红**：把本值改回 100（或任何 > [`MEASURED_ROW_CAPACITY`] 的值）——
/// `row_pool_capacity_is_measured` 是**纯逻辑**断言，当场红且**不会**把测试跑挂。
pub(crate) const ROW_MAX: usize = AUDIT_PAGE_SIZE;

/// 行池**实测可容纳上界**（**单页独活**；`pages_chain` 离屏链一次性注入的逐档实测：
/// 22 成功 / 24 挂死；见 **AU8** 的测量记录）。它是 [`ROW_MAX`] 的**上界约束**
/// （纯逻辑断言，不触碰 LVGL）。
///
/// `pub`（不是 `pub(crate)`）：它是**测量结论**、也是 `ui/tests.rs` 与 `p5_audit` 单测共同的
/// 判据常量；且若只在本 crate 的测试里用，非测试构建会报 `dead_code` —— 让它进公开面即可
/// 如实暴露"这台屏能装多少行"这一事实。
pub(crate) const MEASURED_ROW_CAPACITY: usize = 22;

/// **两页共存**时**每页**可安全容纳的行数（见 **AU8**）。
///
/// **测量前提（必须与数字一起读）**：`LV_MEM_SIZE` = **256 KB**、`pages_chain` 同一 LVGL 堆、
/// **两页 P5 同时存活**（P3 尚未实现 ⇒ 用两个 P5 实例作代理）。实测（2026-09-13）：
/// **2 页 × 5 行成功、2 页 × 6 行即 `lv_realloc` 失败 + `lv_array_resize` 断言挂死**；
/// 两个**空**页成功；**单页满行(20) + 一个空页即 OOM**。
/// 取 **4**（= 8 行 / 两页）对实测挂死点（每页 6）留 ≥33% 余量 ⇒ 共存预算用例用它。
///
/// ⚠️ **它不是"新的一页容量"**：共存时每页只保证 4 行，而 [`ROW_MAX`]=20 的**单页**契约
/// **不变**（`COEXIST_ROWS_PER_PAGE < ROW_MAX` 由编译期断言钉死 —— 把"两页都满行不可能"
/// 这件事写在类型层，而不是留在注释里）。
pub(crate) const COEXIST_ROWS_PER_PAGE: usize = 4;

/// **两页共存**实测的**总行数上界**（两页共用同一个 LVGL 堆；见 **AU8** 与
/// [`COEXIST_ROWS_PER_PAGE`] 的测量前提）。
pub(crate) const MEASURED_COEXIST_TOTAL_ROWS: usize = 10;

/// **编译期自证**（**AU8**）：行池上限**不超实测可容纳上界**，且**至少装得下一页**。
///
/// 写成编译期断言（而不是运行期 `assert!`）有两点理由：① 它**不可能被跳过**（任何构建都查）；
/// ② 它**不会**把测试跑挂 —— 真去注入那么多条会触发 `lv_realloc` 失败 + `lv_array_resize`
/// 断言 ⇒ **挂死**（不是"红"）。
///
/// ## ⚠️ 这条网失效时的**失败模式是"挂死"而不是"变红"**（B2c-1 收口 ⑦）
///
/// **为什么不能改写成"先探测可分配上界、不够则显式失败"**（收口 ⑦ 的另一条路）：探测需要
/// 堆内省，而 `lv_mem_monitor` / 任何 `lv_mem_*` **都不在** `lvgl-sys/allowlist.txt` 内
/// （`LV_MEM_SIZE` 只是 `lv_conf.h` 里的编译期定容，运行期**没有**读数通道）；且 OOM 发生在
/// LVGL **内部**为行对象分配时（`lv_realloc` 失败 ⇒ C 侧 `lv_array_resize` 断言），
/// Rust 侧拿不到"待分配字节数"⇒ 探测公式只能是猜的。故取"**编译期钉死预算 + 注释给出
/// 安全复现 / 诊断指引**"，而不是编一条**自身也可能静默失准**的运行时探测。
///
/// **安全复现 / 诊断指引**（改 [`ROW_MAX`] / [`COEXIST_ROWS_PER_PAGE`] / 页构造成本之前**必读**）：
/// 1. **一律加 `timeout`**：`timeout 300 cargo test -p local-display -j 2`（本仓的验证口径）。
///    挂死点**不会**自己退出 —— 它是 C 侧断言 + 反复 `lv_realloc` 重试，进程活着但无进展；
///    无 `timeout` 时表现为"测试卡住"，很容易被误读成"跑得慢"。
/// 2. **挂死的典型输出**（stderr，可能只有前几行）：`lv_realloc: couldn't reallocate memory`
///    随后是 `lv_array_resize` 的断言 —— 见到它即确认是**堆预算**问题，不是逻辑错。
/// 3. **二分定位**：把注入行数**减半**重试（20 → 10 → 5）；`pages_chain` 的 ⑫/⑮ 段有逐档
///    注入的现成写法，改一处数字即可。测得的新上界要**同步** [`MEASURED_ROW_CAPACITY`] /
///    [`MEASURED_COEXIST_TOTAL_ROWS`]（并保留余量，见 AU8 的测量记录）。
/// 4. **前提**：全链共用**同一个** `LVGL` 堆（`lvgl-sys/lv_conf.h` 的 `LV_MEM_SIZE` = 256 KB），
///    且 `pages_chain` 串行建 / 拆各页 ⇒ 上界随"同时存活的页数"和"每页构件数"变化。
const _: () = assert!(ROW_MAX <= MEASURED_ROW_CAPACITY);
const _: () = assert!(ROW_MAX >= AUDIT_PAGE_SIZE);
/// **编译期自证**：共存预算 ≤ 实测共存上界的一半（两页），且**严格小于**单页上限
/// （= "两页各满行在 256 KB 下不可能"这一测量结论的机器可查形式）。
/// 失败模式同为**挂死**（见上方 [`ROW_MAX`] 的收口 ⑦ 指引）。
const _: () = assert!(2 * COEXIST_ROWS_PER_PAGE <= MEASURED_COEXIST_TOTAL_ROWS);
const _: () = assert!(COEXIST_ROWS_PER_PAGE < ROW_MAX);

/// 表头**列名表**（§6.5 线框逐格标注 `时间 │ 操作者 │ 操作类型 │ 前后值 │ 结果`，
/// **顺序 = 线框从左到右**）。
///
/// **M1（B2c-1 代码质量评审）**：表头子件数、列数、分隔线数、逐列 x **全部由本表推导**
/// —— 此前 `HEAD_CHILD_COUNT = 5 + 4 + 1` 与 `head_col_at: [(i32, &str); 5]` 是**两处独立
/// 字面量**：增删一列时改了表头却忘改常量，网就**静默失准**（常量仍自洽、屏上已多/少一件）。
///
/// **改什么会让本条的推导变红**：往本表加一项而 [`HEAD_COL_X`] 不跟着加 ⇒ **编译期**
/// 报数组长度不符；两张表都加了 ⇒ [`HEAD_CHILD_COUNT`] **自动**跟随（无第二处字面量）。
pub(crate) const HEAD_COLS: [&str; 5] = [
    TEXT_HEAD_TIME,
    TEXT_HEAD_OPERATOR,
    TEXT_HEAD_OP,
    TEXT_HEAD_VALUE,
    TEXT_HEAD_RESULT,
];
/// 表头**列数**（由 [`HEAD_COLS`] 的**表长**推导 ⇒ 增删列时自动跟随）。
pub(crate) const HEAD_COL_COUNT: usize = HEAD_COLS.len();
/// 表头**竖分隔线数**（列间分隔 = 列数 − 1）。
const HEAD_DIV_COUNT: usize = HEAD_COL_COUNT - 1;
/// 表头**子件数**（[`HEAD_COLS`] 的列名 + 列间竖分隔线（列数 − 1） + 1 条底线）
/// —— `pages_chain` 用它做**实际子件数**回归锁（读 LVGL 的 `child_count()`）。
#[cfg(test)]
pub(crate) const HEAD_CHILD_COUNT: usize = HEAD_COL_COUNT + HEAD_DIV_COUNT + 1;
/// 表头**逐列 x**（与 [`HEAD_COLS`] **一一对应**，长度由编译器钉死）。
const HEAD_COL_X: [i32; HEAD_COL_COUNT] = [
    ROW_X0,
    ROW_OP_X,
    HEAD_COL_OP_TYPE_X,
    HEAD_COL_VALUE_X,
    ROW_RESULT_X,
];
/// 表头「操作类型」列 x。
const HEAD_COL_OP_TYPE_X: i32 = ROW_OP_X + ROW_OP_W + Dimens::GAP_MIN;
/// 表头「前后值」列 x。
const HEAD_COL_VALUE_X: i32 = HEAD_COL_OP_TYPE_X + ROW_OPTYPE_W + Dimens::GAP_MIN;
/// 表头竖分隔线宽（§3.5：卡片描边 1 px `divider`）。
const HEAD_DIV_W: i32 = Stroke::THIN;
/// 表头竖分隔线高（表头行高减去上下紧缝）。
const HEAD_DIV_H: i32 = TABLE_HEAD_H - 2 * TIGHT_GAP;
/// 列表底部说明行高（§6.5 未画；取与"最近审计条"同档的 36 px，见 **AU11**）。
const NOTE_H: i32 = NEWEST_H;
/// 「不可篡改说明条」内图标 x（左缘色条 + 同组缝）。
const IMMUTABLE_ICON_X: i32 = IMMUTABLE_BAR_W + Dimens::GAP_MIN;
/// 「不可篡改说明条」内正文 x（图标 + 同组缝）。
const IMMUTABLE_TEXT_X: i32 = IMMUTABLE_ICON_X + IMMUTABLE_ICON_W + Dimens::GAP_MIN;
/// 操作类型 chip 组的列数（4 列 × 192 px + 3 × 16 = 816 ≤ 可用宽 872；**AU7**）。
const OPS_COLS: u32 = 4;
/// 单个 chip 宽（= `CHIP_MIN_W × 2` = 192；推导见 **AU7**：最长选项 + `✓ ` 前缀 + 内边距）。
const OPS_CHIP_W: i32 = Dimens::CHIP_MIN_W * 2;
/// 说明条圆角（`theme::Radius::CTRL` 的转出，避免在本文件另抄一个数字）。
const RADIUS_CTRL: i32 = theme::Radius::CTRL;

// ═══════════════════════════════════════════════════════════════════════════
// 3. 纯逻辑（**不触碰 LVGL** ⇒ 可独立单测；页内一切判据都经这里，保证可离线复现）
// ═══════════════════════════════════════════════════════════════════════════

/// 值对的截断长度（**字符数**口径）。
///
/// 推导：值对列宽 [`ROW_SUMMARY_W`]（455 px）÷ 24 px 档**每汉字约 24 px**（实测口径见
/// `ui/tests.rs::measured_text_px`）⇒ **18 字**（向下取整，保守）。**为何不取得更小**：
/// §6.5 的行内容示例 `端口: 2404 → 2405` 实测 **201.3 px / 16 字**，必须**整条放得下**
/// （它是本页唯一的文案范本）；取得更小会把契约示例本身截断。
/// 超出即截断并加 [`TEXT_ELLIPSIS`]。
pub(crate) const SUMMARY_MAX_CHARS: usize = 18;

/// **未登记 `target` 键**上屏前缀的**长度上界**（**AU9**；B2c-1 代码质量评审 ④ 整改）。
///
/// 键是"辨认用"、值对是"操作内容" ⇒ **先给值对留够位置**，否则长的机器键会把值对整段挤掉
/// （那就从"隐藏字段名"变成"隐藏改动内容"，仍是静默丢信息）。
///
/// **整改（评审 ④）**：原值 10 且走 [`clip`]（**保留头部**）⇒ `interlock.flood` 与
/// `interlock.force` 的归一形态前 7 字**逐字相同**（都是 `IN?ER?O`）⇒ 屏上**同为**
/// `IN?ER?O...: 2404 → 2405` 的截断体，**无法区分**；且现实值对 `2404 → 2405`（11 字）的
/// **后值被挤掉**。现改为：
/// - **保留尾部**（`.ood` / `.rce` 的归一形态末 3 字）：`...OO?` / `...R??` ⇒ **尾部不同
///   即可区分**；
/// - 上界收到 6（`...` + 3 字），把省下的 4 字让给值对；
/// - [`unknown_key_label`] 再按**值对实长**动态收窄（下限 [`UNKNOWN_TARGET_MIN_CHARS`]）
///   ⇒ 现实值对 `2404 → 2405` **完整存活**。
pub(crate) const UNKNOWN_TARGET_MAX_CHARS: usize = 6;

/// 未登记键前缀的**长度下界**（`...` + 2 字尾部）—— [`unknown_key_label`] 的收窄下限。
///
/// 2 字尾部是"可区分"的**实测下限**：`interlock.flood` → `...O?`、`interlock.force` →
/// `...??`（[`UNKNOWN_TARGET_MAX_CHARS`] 的 6 字形态是 `...OO?` / `...R??`）。
pub(crate) const UNKNOWN_TARGET_MIN_CHARS: usize = 5;

/// **编译期自证**：长度预算 `[MIN, MAX]` **真是一个区间**（`MIN < MAX`）。
///
/// 写成编译期断言（而不是用例里的运行期 `assert!`）：① 任何构建都查、**不可能被跳过**；
/// ② clippy 的 `assertions_on_constants` 不会对 `const _` 形态告警（B2c-1 收口 ②）。
/// 把关能力**不减**：把 `MIN` 改到 ≥ `MAX`（收窄区间写反）⇒ **编译失败**。
const _: () = assert!(UNKNOWN_TARGET_MIN_CHARS < UNKNOWN_TARGET_MAX_CHARS);

/// 列表区当前形态（**三态互斥**；§8.3 EDGE-17 / EDGE-08 的**结构性区分**）。
///
/// ⚠️ **`Unavailable` 与 `Empty` 是两个不同的态**，语义**不得互替**：
/// 「无审计记录」= **确实没有**（空态），「审计记录不可用」= **无法获知**（EDGE-17）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListView {
    /// 有审计行（时间倒序）。
    Rows,
    /// 空态：**确实没有**（当前筛选条件下）。
    Empty,
    /// 不可用：**无法获知**（`available = false`，EDGE-17）。
    Unavailable,
}

/// 审计页 → 列表区形态（**唯一判据**）。
///
/// `!available` **优先**：此时 `entries` 这一向量**本身不可信**（可能只是缺省空值），
/// 故一律落 [`ListView::Unavailable`]（§8.3 EDGE-17：「**不得**显『无审计记录』」）。
#[cfg(test)]
pub(crate) fn list_view(page: &AuditPage) -> ListView {
    list_view_of(page.available, page.entries.len())
}

/// **形态判据的单一真源**（`!available` 优先于行数）—— 页内核（`Core::view`）与 [`list_view`]
/// **共用本函数**：两处各写一遍会把"不可用 vs 空态"的不变量变成**两份真源**（改一处漏一处 =
/// §8.3 禁止的语义互替；B2c-1 的探针 P2 实测：只在其中一处制造互替，另一处**照旧全绿**）。
///
/// `rows` = **实际在屏的行数**（页内核传"已渲染条数"，契约侧传 `entries.len()`）。
pub(crate) const fn list_view_of(available: bool, rows: usize) -> ListView {
    if !available {
        ListView::Unavailable
    } else if rows == 0 {
        ListView::Empty
    } else {
        ListView::Rows
    }
}

/// **行序**（§6.5「**时间倒序**」）—— 返回注入向量的**下标序**（稳定排序：`ts_ms` 相同者
/// 保持注入时的相对次序）。
///
/// 为什么在**本页**排一次（而不是"依赖后端保证"）：§6.5 把"时间倒序"写在本页的列表规格里，
/// 后端若因任何原因给出乱序（或 B3 组装窗口时拼接了两页），屏上就会是乱序 —— 而"审计链的
/// 可信度"正是本页的语义；本地排序是**廉价**的（一页 ≤ 20 条）且让页面**自洽**，不改变
/// `page` / `has_more` 等分页语义（那些仍以注入为准）。
///
/// **单一真源**：[`Core::apply_page`] 与 [`Core::refresh_op_labels`] **共用本函数**
/// （两处各自排一遍会出现"行按序渲染、标签按注入序刷新"的错位）。
pub(crate) fn row_order(entries: &[ConsoleAuditEntry]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..entries.len()).collect();
    idx.sort_by(|a, b| entries[*b].ts_ms.cmp(&entries[*a].ts_ms));
    idx
}

/// 最近审计条文案（`newest_ts_ms = None` ⇒ 占位符，**绝不编造时间**）。
///
/// **改什么会让本条变红**：把 `None` 分支写成 `format_epoch_ms_utc(0)`（即"1970/01/01"这种
/// **编造**的时间）—— `newest_never_fabricates_time` 用例立刻红。
pub(crate) fn newest_text(newest_ts_ms: Option<u64>) -> String {
    match newest_ts_ms {
        Some(ms) => format!("{TEXT_NEWEST_PREFIX}{}", format_epoch_ms_utc(ms)),
        None => format!("{TEXT_NEWEST_PREFIX}{PLACEHOLDER}"),
    }
}

/// `result` → 胶囊文案（§3.6 P5 列表行的 `● 成功` / `× 失败`）。
pub(crate) const fn result_text(r: AuditResult) -> &'static str {
    match r {
        AuditResult::Ok => TEXT_RESULT_OK,
        AuditResult::Failed => TEXT_RESULT_FAIL,
    }
}

/// `result` → 胶囊皮肤（`theme::ChipSkin` 的 `SUCCESS` / `FAILURE` —— 该二者**本就是**
/// 按 §6.5 的 `● 成功` / `✕ 失败` 收录的，故不存在第二份色值真源）。
pub(crate) const fn result_skin(r: AuditResult) -> ChipSkin {
    match r {
        AuditResult::Ok => ChipSkin::SUCCESS,
        AuditResult::Failed => ChipSkin::FAILURE,
    }
}

/// **实测缺字的全角标点** → cmap 内等价符号（**逐条登记、只增不改**）。
///
/// | 源（**缺字**，实测不在 cmap 内） | 上屏 | 依据 |
/// |----------------------------------|------|------|
/// | `，`(U+FF0C) / `；`(U+FF1B) / `（`(U+FF08) / `）`(U+FF09) | `·`(U+00B7) | P4 **IL1** 同款处置（P4 的静态文案就是这么改的）；`·` 在 cmap 内 |
/// | `：`(U+FF1A) | `:`(U+003A) | **AU1**（P2 的 `涉及:` 先例） |
/// | `、`(U+3001) | `/`(U+002F) | **IL1**（`触发源未复位 · 急停/门禁`） |
///
/// **判据写成码位数值（`0xFF0C` 一类）而不是字符字面量** —— 与 `pages/mod.rs::display_safe_char`
/// 同款理由：`ui/tests.rs::ui_texts_covered_by_font_cmap` 会把 `\u{XXXX}` 还原成**真字形**再查
/// cmap，故把缺字写进字面量（哪怕用转义）会被当场判成"源码用了 cmap 外的字"。
///
/// ⚠️ **只折叠这 6 个**（它们在契约文案与拒绝原因里**实测常用**：`触发源未复位：estop、door`）。
/// 其余全角标点（`？`/`！`/`“”`/`—`/`…` 等）**未折叠** ⇒ 自由文本里出现时仍是豆腐块
/// （残余，登记在 **AU15**；防线在后端字段命名 / 字库，同 D9）。
fn fold_fullwidth_punct(c: char) -> char {
    match c as u32 {
        0xFF0C | 0xFF1B | 0xFF08 | 0xFF09 => '\u{00B7}',
        0xFF1A => ':',
        0x3001 => '/',
        _ => c,
    }
}

/// **自由文本**（`operator` 未知名 / `reason`）→ 上屏文本：
/// [`display_safe`] + [`fold_fullwidth_punct`]（**先折全角标点，再走 ASCII 归一**）。
///
/// 为什么在 `display_safe` 之外多做一步：`display_safe` 只改写 **ASCII**（非 ASCII 原样透传，
/// 见其文档），而全角标点是**非 ASCII** ⇒ 直上屏即豆腐块。P2 / P4 的自由文本路径（PD13 / IL17）
/// 只过 `display_safe`；本页**多折叠 6 个常用全角标点**（见 **AU15**：这是**有意**比 P4 更严的
/// 一步，因为它落在**行内**、且 §8.3 要求失败原因「**就地可见**」）。
pub(crate) fn free_text_safe(text: &str) -> String {
    let folded: String = text.chars().map(fold_fullwidth_punct).collect();
    display_safe(&folded)
}

/// `operator` → 上屏文本。
///
/// 逐字规则（同 `p4_interlock.rs` **IL6** 的处置取向）：
/// - 等于契约 `CONSOLE_OPERATOR`（`local-console`，ASCII 大小写不敏感）⇒ 「本地控制台」
///   （§3.6 P5 列表行；**不**把 `local-console` 直上屏 —— 小写 ASCII 在生成字体里没有字形）；
/// - 未知名 ⇒ [`free_text_safe`]（小写 → 大写同族、全角标点折叠、cmap 外 ASCII → `?`），
///   **不伪造**中文名；
/// - 非 ASCII 且**缺字形**（后端直接给中文但用了 cmap 外的字）⇒ 仍是豆腐块（残余，见 **AU9**）。
pub(crate) fn operator_text(operator: &str) -> String {
    if operator.eq_ignore_ascii_case(CONSOLE_OPERATOR) {
        TEXT_OPERATOR_LOCAL.to_string()
    } else {
        free_text_safe(operator)
    }
}

/// `target` 的**机器键**（**不上屏** —— 只用于与 `ConsoleAuditEntry.target` 比对）。
///
/// 存在的唯一理由与 `p2_config.rs::config_key` / `p4_interlock.rs::source_key` **同款**：
/// `ui/tests.rs::ui_texts_covered_by_font_cmap` 把 `ui/**` 里**所有**字符串字面量一律当
/// "上屏候选"逐字查字形（宁可多查），而机器键是小写 ASCII（`gateway.port` 里的 `g`/`a`/`t`/`e`
/// **在生成字体里没有字形**）。经本函数标注 = 声明"这个字面量**从不进 `lv_label`"
/// （见 `ui/tests.rs::NON_DISPLAY_SINKS` 的逐条白名单；其计数由 `p5_static_constraints`
/// 自证为**恰 4 处**）。**不是**为了过网而把文案写残。
const fn audit_key(key: &'static str) -> &'static str {
    key
}

/// 已知 `target` 键 → 上屏标签的映射表（**逐条登记、只增不改**）；未命中 ⇒ `None`（**不显标签**）。
///
/// | 机器键 | 上屏 | 出处 / 证据 |
/// |--------|------|-------------|
/// | `gateway.port` | [`TEXT_FIELD_PORT`] | §3.6 P2「字段」行（`端口`）；§6.5 行内容示例 `端口: 2404 → 2405` |
/// | `system.log_level` | [`TEXT_FIELD_LOG_LEVEL`] | §3.6 P2「字段」行；契约 `audit.rs` / `control.rs` 的**字面量示例**即该键 |
/// | `telemetry.interval` | [`TEXT_FIELD_TELEMETRY`] | §3.6 P2「字段」行（`遥测上报周期`）；`p2_config.rs` 用例字段表的键 |
/// | `gateway.listen_addr` | `p2_config::TEXT_LISTEN_ADDR` | §6.2 PM 裁定串（R-08 / U-1）；**转出** P2 的常量，不另抄 |
///
/// ⚠️ **键全集的真源在 mupcd 的配置服务**（本仓当前只有上述 4 个键有字面量证据）⇒ 表**只增不改**：
/// 新增键必须同时在 §3.6 找到对应中文标签；**猜错键名 = 谎报字段名**，故宁可不显标签（见 **AU9**）。
pub(crate) const TARGET_LABELS: [(&str, &str); 4] = [
    (audit_key("gateway.port"), TEXT_FIELD_PORT),
    (audit_key("system.log_level"), TEXT_FIELD_LOG_LEVEL),
    (audit_key("telemetry.interval"), TEXT_FIELD_TELEMETRY),
    (audit_key("gateway.listen_addr"), p2_config::TEXT_LISTEN_ADDR),
];

/// **契约点名的联锁 `target` 键** → 联锁 `ConsoleOp`（**已知键**，标签由 `label()` 转出）。
///
/// | 机器键 | 契约出处 | 上屏标签（= `ConsoleOp::label()`） |
/// |--------|----------|-----------------------------------|
/// | `interlock.release` | `display-proto/src/audit.rs` 的 `ConsoleAuditEntry::target` 注释（`如 "system.log_level" / "interlock.release"`）与 `ConsoleOp::InterlockRelease` 文档（`POST /interlock/release`）；设计 §4.5 同款字面量 | `联锁释放` |
/// | `interlock.ack_m1` | `ConsoleOp::InterlockAckM1` 文档（`POST /interlock/ack_m1`）；设计 §4.5 端点表 | `M1 授权` |
///
/// ⚠️ **为什么不把它们写进 [`TARGET_LABELS`] 的标签列**：那需要在 UI 层**另抄一份**中文
/// 字面量，而这两个键与两个联锁 `ConsoleOp` **一一对应**、标签已由契约 `label()` 给出
/// ⇒ 抄一份 = 本项目明令避免的"**第二份真源**"（**AU3** 的同一条理由）。故只登记**键 → op**
/// 的对应，标签在 [`target_label`] 里**转出**。`audit_key(..)` 计数随之从 4 变 6
/// （`ui/tests.rs::p5_static_constraints` 同步钉住，防上屏串混入豁免）。
pub(crate) const INTERLOCK_TARGETS: [(&str, ConsoleOp); 2] = [
    (audit_key("interlock.release"), ConsoleOp::InterlockRelease),
    (audit_key("interlock.ack_m1"), ConsoleOp::InterlockAckM1),
];

/// `target` → 上屏标签（未命中 ⇒ `None`）。
///
/// 判定顺序：① [`TARGET_LABELS`]（配置字段键，中文标签是**唯一真源**）；②
/// [`INTERLOCK_TARGETS`]（联锁键，标签转出 `ConsoleOp::label()`）；③ 都不命中 ⇒ `None`
/// （调用方 [`summary_text`] 走"降级显示机器键"的路径，**不静默隐藏**，见 **AU9**）。
pub(crate) fn target_label(target: &str) -> Option<&'static str> {
    if let Some((_, v)) = TARGET_LABELS.iter().find(|(k, _)| *k == target) {
        return Some(v);
    }
    INTERLOCK_TARGETS
        .iter()
        .find(|(k, _)| *k == target)
        .map(|(_, op)| op.label())
}

/// 单个 JSON 值的上屏文本（**唯一出口**；逐类型处置见 **AU12**）。
///
/// | 输入 | 输出 | 理由 |
/// |------|------|------|
/// | 字符串 | [`display_safe`] | 机器值（`debug` / `info`）是小写 ASCII，直上屏即豆腐块；`display_safe` 折成同族大写（`DEBUG`），**保留可辨认性** |
/// | 整数 | `pages::fmt_int0` 口径（负号恒 `−`） | 与全屏"数值 → 文本"的**唯一出口**同口径（ASCII `-` 无字形） |
/// | 小数 | `pages::fmt_signed_1dp` | 同上；1 位小数是本屏既有的数值精度口径（P1 同款） |
/// | 布尔 | `开` / `关` | `真`/`假`/`是`/`否` **均不在 cmap 内**（**AU12**） |
/// | 数组 | `N 条` | 只报**规模**：`[1,2]` 的 `[` `,` `]` 都不在 cmap 内，逐字渲染会得到一串 `?`（比"只报规模"更糟） |
/// | 对象 | `N 字段` | 同上 |
/// | `null` | 占位符 `–` | 「本字段存在但值为空」（与 `None` 同口径，**绝不补 0**） |
pub(crate) fn value_text(v: &Value) -> String {
    match v {
        Value::Null => PLACEHOLDER.to_string(),
        Value::Bool(true) => TEXT_VALUE_ON.to_string(),
        Value::Bool(false) => TEXT_VALUE_OFF.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                crate::ui::pages::fmt_int0(i as f64)
            } else if let Some(f) = n.as_f64() {
                crate::ui::pages::fmt_signed_1dp(f)
            } else {
                // 理论上不可达的数值形态：取占位符而**不是**空串 / 0（"不可表示"必须可见）。
                PLACEHOLDER.to_string()
            }
        }
        Value::String(s) => display_safe(s),
        Value::Array(a) => format!("{} {TEXT_UNIT_ITEMS}", a.len()),
        Value::Object(o) => format!("{} {TEXT_UNIT_FIELDS}", o.len()),
    }
}

/// `Option<Value>` 的一侧文本（`None` ⇒ 占位符 `–`）。
///
/// ⚠️ **`None` 绝不当 `0` / 空串**（PRD「显 `--`，**严禁补 0**」；本项目已有 C1 级前车之鉴：
/// 旧实现把类型错配 `unwrap_or_default()` 兜底成 `0`，而 `0` 本身是合法配置值 ⇒
/// "不合法"被**伪装**成"值为 0"）。**改什么会让本条变红**：把 `None` 分支写成
/// `"0".to_string()` 或 `String::new()` —— `none_is_placeholder_never_zero` 立刻红。
pub(crate) fn side_text(v: Option<&Value>) -> String {
    match v {
        Some(x) => value_text(x),
        None => PLACEHOLDER.to_string(),
    }
}

/// 按**字符数**截断（超出 ⇒ 末三字符换成 [`TEXT_ELLIPSIS`]，**不 panic**）。
pub(crate) fn clip(text: &str, max_chars: usize) -> String {
    let n = text.chars().count();
    if n <= max_chars {
        return text.to_string();
    }
    let ell = TEXT_ELLIPSIS.chars().count();
    // ⚠️ **M2（B2c-1 代码质量评审）**：`max_chars < 3` 时**放不下**省略标记 ⇒ 原先的
    // `keep = max − 3 = 0` 会产出 `...`（**3 字 > 上限**，与"按上限截断"的契约相悖）。
    // 放不下省略标记时改为**纯尾截**（无标记），保证"产物长度恒 ≤ 上限"。
    if max_chars <= ell {
        return text.chars().take(max_chars).collect();
    }
    let keep = max_chars - ell;
    let head: String = text.chars().take(keep).collect();
    format!("{head}{TEXT_ELLIPSIS}")
}

/// 按**字符数**截断、**保留尾部**（超出 ⇒ 首部换成 [`TEXT_ELLIPSIS`]，**不 panic**）。
///
/// 与 [`clip`] 互补：`clip` 保**前缀**（如值对——"前值"最重要），本函数保**后缀**。
/// 用途见 [`unknown_key_label`]：机器键的**区分位在尾部**（`…ood` / `…rce`），保头会把
/// 不同的键截成同一个产物。
///
/// 同样遵守"产物长度恒 ≤ 上限"（`max_chars < 3` 时改为纯尾截、无标记）。
pub(crate) fn clip_tail(text: &str, max_chars: usize) -> String {
    let n = text.chars().count();
    if n <= max_chars {
        return text.to_string();
    }
    let ell = TEXT_ELLIPSIS.chars().count();
    if max_chars <= ell {
        return text.chars().skip(n - max_chars).collect();
    }
    let keep = max_chars - ell;
    let tail: String = text.chars().skip(n - keep).collect();
    format!("{TEXT_ELLIPSIS}{tail}")
}

/// **未登记 `target` 键**的上屏前缀（`display_safe` 归一 + [`clip_tail`] **保尾**截断）。
///
/// `pair_len` = 同一行的**值对实长**（字符数）—— 键的长度**只在值对拿完之后**才分配
/// （见 [`unknown_key_budget`]），即"操作内容优先于辨认标签"。
///
/// **改什么会让本条变红**：把 [`clip_tail`] 换回 [`clip`]（保头）⇒
/// `interlock.flood` 与 `interlock.force` 的产物**再次相同**，
/// `unknown_target_key_is_shown_not_hidden` 的第 ③ 组断言立刻红。
pub(crate) fn unknown_key_label(target: &str, pair_len: usize) -> String {
    clip_tail(&display_safe(target), unknown_key_budget(pair_len))
}

/// [`unknown_key_label`] 的长度预算（**纯函数**，拆出便于单测）。
///
/// `budget = clamp(SUMMARY_MAX_CHARS − pair_len − len(": "), MIN, MAX)` ——
/// **值对优先**：值对越短，键能拿到的位置越多（上限 6）；值对越长，键越短（下限 5）。
///
/// ⚠️ **如实限定（评审 ④ ②）**：`值对完整存活`的**充分条件是 `pair_len + 2 + MIN ≤
/// SUMMARY_MAX_CHARS`**（即 ≤ 11 字）；更长的值对**会被截断**（那是"值对本身超长"的既有
/// 口径 —— 键已退到下限，不再侵占）。**残余**：仅**尾部归一形态相同**的两个未登记键
/// （如 `a.b` / `c.b`）仍会撞形 —— 保尾截断的固有边界，如实登记在 **AU9**。
pub(crate) fn unknown_key_budget(pair_len: usize) -> usize {
    SUMMARY_MAX_CHARS
        .saturating_sub(pair_len + TEXT_LABEL_SEP.chars().count())
        .clamp(UNKNOWN_TARGET_MIN_CHARS, UNKNOWN_TARGET_MAX_CHARS)
}

/// 行 2 的**前后值摘要**（UI §6.5 行内容：「前后值摘要 24 px `text_second`（如 `端口: 2404 → 2405`）」）。
///
/// 形态：`[<标签>: ]<前值> → <后值>` ——
/// - **已知键**（[`TARGET_LABELS`] / [`INTERLOCK_TARGETS`]）⇒ 中文标签（与 §6.5 示例同形）；
/// - **未登记键** ⇒ **降级为 `display_safe(键)` 的保尾截断**（如 `interlock.flood`
///   → `...OO?`、`interlock.force` → `...R??`）—— **不得整块不显标签**：那会让操作者
///   **无从知道被改的是哪一项**（**AU9** 的整改细则）。降级口径与 `p4_interlock.rs`
///   **IL6**（源名未知名）一致：**不臆造**中文名、经 `display_safe` 保证不出豆腐块；
///   键前缀长度由 [`unknown_key_label`] 按**值对实长**动态分配（**值对优先**，见
///   [`UNKNOWN_TARGET_MAX_CHARS`] / 评审 ④）；
/// - **空键**（`""`）⇒ 不显标签（无标识可显，**不是**隐藏信息）；
/// - 两侧都经 [`side_text`]（`None` ⇒ `–`）与 [`clip`]（超长截断）。
pub(crate) fn summary_text(before: Option<&Value>, after: Option<&Value>, target: &str) -> String {
    let pair = format!("{}{TEXT_PAIR_ARROW}{}", side_text(before), side_text(after));
    let label = match target_label(target) {
        Some(label) => label.to_string(),
        None if target.is_empty() => return clip(&pair, SUMMARY_MAX_CHARS),
        // 未登记键：**保尾**截断，长度按值对实长动态分配（见 [`unknown_key_label`]）。
        None => unknown_key_label(target, pair.chars().count()),
    };
    clip(&format!("{label}{TEXT_LABEL_SEP}{pair}"), SUMMARY_MAX_CHARS)
}

/// 失败原因的上屏文本（**仅失败行**；`None` / 空串 ⇒ `None`）。
///
/// **`result` 是入参而不是调用方的自觉**：成功行**结构性**取不到原因文案
/// （§6.5 行内容：「`原因：…`（**仅失败行**）」）—— 即使契约给了矛盾的
/// `result = ok` + `reason = Some(..)`，屏上也不会出现原因（"绝不造假"的同一取向）。
pub(crate) fn reason_text(result: AuditResult, reason: Option<&str>) -> Option<String> {
    if result != AuditResult::Failed {
        return None;
    }
    let raw = reason.unwrap_or_default().trim();
    if raw.is_empty() {
        return None;
    }
    Some(format!("{TEXT_REASON_PREFIX}{}", free_text_safe(raw)))
}

/// 底部状态行文案（§3.6 P5「列表」行的 `加载中` / `已加载全部`；无行 ⇒ `None`）。
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

/// 审计查询意图（**本页交给外部的唯一载荷**；`request_id` / HTTP 由 B3 承担）。
///
/// | 字段 | 语义 |
/// |------|------|
/// | `range` | 时间范围三档（[`LogRange`]；与 P3 共用，见 `filters.rs` **FR3**） |
/// | `from_ms` / `to_ms` | **仅 `Custom` 时**给出（`H1` / `H24` 的相对窗口由服务端按档位算） |
/// | `ops` | 操作类型**多选**；**空集合 = 不按操作类型筛选**（等价「全部」） |
/// | `page` | 请求页（1-based）；筛选变化恒为 `1`，「加载更多」为 `当前页 + 1` |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditQuery {
    /// 时间范围档位。
    pub range: LogRange,
    /// 自定义起始（UTC 毫秒；`Custom` 之外的档位为 `None`）。
    pub from_ms: Option<u64>,
    /// 自定义结束（UTC 毫秒；同上）。
    pub to_ms: Option<u64>,
    /// 操作类型多选（空 = 不筛）。
    pub ops: Vec<ConsoleOp>,
    /// 页码（1-based）。
    pub page: u32,
}

impl Default for AuditQuery {
    fn default() -> Self {
        Self {
            range: LogRange::H1,
            from_ms: None,
            to_ms: None,
            ops: Vec::new(),
            page: 1,
        }
    }
}

/// 组装查询（**纯逻辑**；筛选态 → 查询语义的唯一映射点）。
///
/// - `H1` / `H24` ⇒ `from_ms` / `to_ms` 一律 `None`（相对窗口由服务端算，UI 不读时钟）；
/// - `Custom` ⇒ 两侧都由 [`filters::datetime_to_epoch_ms`] 折算（**确定、可测**，见 **FR2**）。
pub(crate) fn audit_query(change: &TimeRangeChange, ops: &[ConsoleOp], page: u32) -> AuditQuery {
    let (from_ms, to_ms) = match change.range {
        LogRange::H1 | LogRange::H24 => (None, None),
        LogRange::Custom => (
            Some(filters::datetime_to_epoch_ms(change.start)),
            Some(filters::datetime_to_epoch_ms(change.end)),
        ),
    };
    AuditQuery {
        range: change.range,
        from_ms,
        to_ms,
        ops: ops.to_vec(),
        page,
    }
}

/// 「全部 + 具体项」并存选择的**归一化**（`selected` = chip 组的勾选下标集合，`0` = 「全部」）。
///
/// 规则（**纯逻辑、逐条可测**）：
/// 1. 本次**新增**的选中项含 `0`（「全部」）⇒ 只留 `[0]` —— UI §6.3「`全部` chip 为快捷复位
///    （1 次触摸清空该维度）」；
/// 2. 本次新增的是**具体项** ⇒ 去掉 `0`（用户意图是"缩小范围"）；
/// 3. 只发生**取消**（无新增）⇒ 原样返回。
///
/// `prev` = 上一次**归一化后**的选择（用于识别"本次新增了哪个"）。
pub(crate) fn normalize_ops_selection(prev: &[usize], selected: &[usize]) -> Vec<usize> {
    let added: Vec<usize> = selected
        .iter()
        .copied()
        .filter(|i| !prev.contains(i))
        .collect();
    if added.contains(&0) {
        // 规则 1：点了「全部」⇒ 复位。
        return vec![0];
    }
    if !added.is_empty() {
        // 规则 2：点了具体项 ⇒ 让出「全部」。
        return selected.iter().copied().filter(|i| *i != 0).collect();
    }
    // 规则 3：只取消 ⇒ 原样（仍做一次保守清理，避免留下"全部 + 具体项"的非法组合）。
    if selected.contains(&0) && selected.len() > 1 {
        return selected.iter().copied().filter(|i| *i != 0).collect();
    }
    selected.to_vec()
}

/// 勾选下标 → 操作类型集合（下标 `0` = 「全部」，**不产 `ConsoleOp`**）。
///
/// 越界下标 / 已不在注入列表内的下标一律**丢弃**（不 panic）—— 与"选项集合由注入决定"一致。
pub(crate) fn selected_ops(selected: &[usize], opts: &[OpOption]) -> Vec<ConsoleOp> {
    let mut out: Vec<ConsoleOp> = selected
        .iter()
        .filter_map(|i| i.checked_sub(1).and_then(|k| opts.get(k)))
        .map(|o| o.op)
        .collect();
    out.dedup();
    out
}

/// 操作类型 chip 组的**上屏选项**：`[全部] + 各注入标签`（各自经 [`display_safe`]）。
pub(crate) fn op_options(opts: &[OpOption]) -> Vec<String> {
    let mut out = Vec::with_capacity(opts.len() + 1);
    out.push(TEXT_OPS_ALL.to_string());
    out.extend(opts.iter().map(|o| display_safe(&o.label)));
    out
}

/// 契约给出的**规范选项表**（`/audit/ops` 未注入时的缺省选项 = `ConsoleOp::ALL`）。
///
/// **为什么缺省可用**：`display-proto` 的 `impl From<ConsoleOp> for OpOption` 就是用
/// `ConsoleOp::label()` 生成推荐的 `label` ⇒ 缺省与"服务端按契约返回"**逐字一致**；
/// 服务端若给了不同的 label，[`P5AuditPage::set_ops`] 会以注入为准并重建 chip 组。
pub(crate) fn canonical_ops() -> Vec<OpOption> {
    ConsoleOp::ALL.iter().copied().map(OpOption::from).collect()
}

/// 某个操作类型的上屏标签：**优先用注入的 label**（服务端真源），否则回退契约 `ConsoleOp::label()`。
pub(crate) fn op_label_of(op: ConsoleOp, opts: &[OpOption]) -> String {
    opts.iter()
        .find(|o| o.op == op)
        .map(|o| display_safe(&o.label))
        .unwrap_or_else(|| op.label().to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 列表行（**行池 / 复用**；每个拥有型句柄都存进字段）
// ═══════════════════════════════════════════════════════════════════════════

/// 一条审计行（60 px，两行结构；UI §6.5「行内容」）。
///
/// **只读行**：全部零件是 `decor` + `Label` + `StatusChip` —— **没有任何按钮 / 可点对象**
/// （§5.3「非交互列表不用 `lv_list`」；行内不存在编辑 / 删除入口）。
/// 行左缘 3 px `#35D0C4` 竖条**每条都有**（§6.5）。
///
/// **结果胶囊预建两支**（成功 / 失败）：`StatusChip` 的**皮肤在构造期固定**（`components.rs`
/// 只能改文本，没有改皮肤的接口 —— `p4_interlock.rs` 的 latch 胶囊同款处置），故按
/// `result_skin` 的两个取值各建一支，`apply` 时用 `show_only` 互斥切换。
struct Row {
    /// 行容器（**拥有型**：`Drop` 即级联删除整行）。
    obj: Obj,
    /// 左缘 3 px 竖条（`#35D0C4`）。
    #[allow(dead_code)]
    bar: Obj,
    /// 行 1：时间戳（`text_second`）。
    time: Label,
    /// 行 1：操作者（`text_weak`）。
    operator: Label,
    /// 行 1：结果胶囊（按 [`result_skin`] 的两个取值各建一支）。
    results: [Rc<StatusChip>; 2],
    /// 行 2：操作类型（26 px `text_primary`）。
    op: Label,
    /// 行 2：前后值摘要（24 px `text_second`）。
    summary: Label,
    /// 行 2：失败原因（24 px `#FF6B6B`，**仅失败行可见**）。
    reason: Label,
}

impl Row {
    /// 建一行（第 `index` 行；y = `index × 行高`）。
    fn new(parent: &Obj, index: usize) -> Result<Self, LvglError> {
        let obj = layout_box(parent, Dimens::CONTENT_W, Dimens::ROW_AUDIT_H)?;
        obj.set_pos(0, index as i32 * Dimens::ROW_AUDIT_H);
        // **行不吃触摸事件**（§5.3：非交互列表用 `lv_obj` 行容器而非可点的 `lv_list` item；
        // 行内也不存在任何操作入口）。`lv_obj` 出厂带 `CLICKABLE` ⇒ 显式摘掉，并作为
        // `list_clickable_count() == 0` 这条只读回归锁的**结构性前提**。
        obj.remove_flag(ObjFlag::CLICKABLE);
        // 左缘 3 px 竖条（§6.5：**每条都有**，强化"不可篡改链"语义）。
        let bar = decor(
            &obj,
            ROW_BAR_W,
            Dimens::ROW_AUDIT_H,
            &theme::card_head_bar(Palette::SOC_OK),
        )?;
        bar.set_pos(0, 0);

        let time = text_label(&obj, "", TextSlot::Body, Palette::TEXT_SECOND)?;
        time.set_size(ROW_TIME_W, TextSlot::Body.px() as i32);
        time.set_long_mode(LongMode::DOTS);
        time.set_pos(ROW_X0, LINE1_TEXT_Y);
        let operator = text_label(&obj, "", TextSlot::Body, Palette::TEXT_WEAK)?;
        operator.set_size(ROW_OP_W, TextSlot::Body.px() as i32);
        operator.set_long_mode(LongMode::DOTS);
        operator.set_pos(ROW_OP_X, LINE1_TEXT_Y);
        // 两支结果胶囊（`show_only` 在 `apply` 里切换；构造期只给缺省文本，**不预判结果**）。
        let results = [
            Rc::new(StatusChip::new(
                &obj,
                ROW_RESULT_W,
                "",
                result_text(AuditResult::Ok),
                result_skin(AuditResult::Ok),
            )?),
            Rc::new(StatusChip::new(
                &obj,
                ROW_RESULT_W,
                "",
                result_text(AuditResult::Failed),
                result_skin(AuditResult::Failed),
            )?),
        ];
        for c in &results {
            c.set_pos(ROW_RESULT_X, LINE1_Y);
            // 胶囊是**展示件**（`StatusChip` 的 `lv_obj` 出厂带 `CLICKABLE`）⇒ 同样摘掉：
            // 它不承担任何交互，且不该吃掉页面级的纵向滚动手势。
            c.obj().remove_flag(ObjFlag::CLICKABLE);
        }

        let op = text_label(&obj, "", TextSlot::Label, Palette::TEXT_PRIMARY)?;
        op.set_size(ROW_OPTYPE_W, TextSlot::Label.px() as i32);
        op.set_long_mode(LongMode::DOTS);
        op.set_pos(ROW_X0, LINE2_LABEL_Y);
        let summary = text_label(&obj, "", TextSlot::Body, Palette::TEXT_SECOND)?;
        summary.set_size(ROW_SUMMARY_W, TextSlot::Body.px() as i32);
        summary.set_long_mode(LongMode::DOTS);
        summary.set_pos(ROW_SUMMARY_X, LINE2_TEXT_Y);
        let reason = text_label(&obj, "", TextSlot::Body, Palette::DANGER)?;
        reason.set_size(ROW_REASON_W, TextSlot::Body.px() as i32);
        reason.set_long_mode(LongMode::DOTS);
        reason.set_pos(ROW_REASON_X, LINE2_TEXT_Y);
        reason.set_hidden(true);

        Ok(Self {
            obj,
            bar,
            time,
            operator,
            results,
            op,
            summary,
            reason,
        })
    }

    /// 把一条审计记录写到本行（**只改文本 / 可见性**，不新建 / 不删除对象）。
    fn apply(&self, e: &ConsoleAuditEntry, op_text: &str) {
        self.time.set_text(&format_epoch_ms_utc(e.ts_ms));
        self.operator.set_text(&operator_text(&e.operator));
        let idx = match e.result {
            AuditResult::Ok => 0,
            AuditResult::Failed => 1,
        };
        let objs: Vec<&Obj> = self.results.iter().map(|c| c.obj()).collect();
        show_only(&objs, Some(idx));
        self.op.set_text(op_text);
        self.summary
            .set_text(&summary_text(e.before.as_ref(), e.after.as_ref(), &e.target));
        match reason_text(e.result, e.reason.as_deref()) {
            Some(t) => {
                self.reason.set_text(&t);
                set_visible(self.reason.obj(), true);
            }
            None => set_visible(self.reason.obj(), false),
        }
    }

    /// 当前在显的结果胶囊下标（`0` = 成功 / `1` = 失败）。
    #[cfg(test)]
    fn result_index(&self) -> Option<usize> {
        self.results.iter().position(|c| !c.obj().is_hidden())
    }

    /// 本行可点子对象计数（只读回归锁：**必须恒 `0`**）。
    #[cfg(test)]
    fn clickable_parts(&self) -> usize {
        let mut parts: Vec<&Obj> = vec![&self.obj, &self.bar, self.time.obj(), self.operator.obj()];
        parts.extend(self.results.iter().map(|c| c.obj()));
        parts.push(self.op.obj());
        parts.push(self.summary.obj());
        parts.push(self.reason.obj());
        parts
            .iter()
            .filter(|o| o.has_flag(ObjFlag::CLICKABLE))
            .count()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 页面内核
// ═══════════════════════════════════════════════════════════════════════════

/// 意图回调槽（[`CbSlot`]；与 `p4_interlock.rs::IntentSlot` **同语义**）。
///
/// **不是**裸 `RefCell<Option<Box<dyn FnMut(..)>>>`（收口 ①）：裸槽把内部借用**外露**给
/// 调用点 ⇒ 能写出"持 `try_borrow_mut` 借用直调用户回调"的旧写法（本单元修的 ③：回调内
/// 自替换被静默丢弃）。[`CbSlot`] 只给 `set` / `fire`，承载字段私有 ⇒ 旧写法**编译不过**。
type QuerySlot = CbSlot<AuditQuery>;

/// 页内核（全部 LVGL 句柄 + 共享态）。
///
/// ## ⚠️ 收口 ③：公共面收窄后为何下面（以及 `Row`）出现 `#[allow(dead_code)]`
///
/// 本单元（B2c-1 收口 ③ / **M4**）把断言口收进 `#[cfg(test)]`（`row_bar_size` /
/// `head_divs_alive` / `immutable_bg` 一类）⇒ 若干**拥有型字段**在**非测试构建**里唯一的
/// 读者就是那些被 cfg 掉的断言口，rustc 判它们"从未被读"。**但字段必须留在生产构建里**
/// （`Drop` 即级联删除子树 —— 见模块头的所有权纪律），故**逐个字段**就近加
/// `#[allow(dead_code)]`。**这不是放宽判据**：只抑制"字段未被读"这一条 lint，不改变所有权
/// 语义与运行期行为。（`Cell` 形态的 `immutable_bg` 是"应用标记"，其唯一读者是色相断言的
/// 页面侧那一半。）
struct Core {
    /// 页根（= 纵向滚动容器；契约 1）。
    root: ScrollContainer,
    // ── 头部三条 ──
    /// 最近审计条文案。
    newest: Label,
    /// 不可篡改说明条的底（**唯一**用 `Palette::AUDIT_BG` 的地方，见 **AU13**）。
    immutable: Obj,
    /// 说明条**实际写入样式的底色**（**应用标记**，见 **AU13** / 色相断言的"页面确实用了它"那一半）。
    ///
    /// ⚠️ **薄层没有 `bg_color` 读回**（`Obj` 读不回 `bg_color`；B4a 补齐的 `bg_opa` /
    /// `text_color` / `text_font` **不含它**，见 [`audit_banner_style`] 的说明）⇒ 本条记录的是
    /// **送给样式构造器的那个色值**（与 `set_bg_color(..)` 收到的是**同一个表达式**）
    /// ⇒ 把 [`audit_banner_style`] 的底色改成 `WARN_BG`，本标记**跟着变**，
    /// `pages_chain` 的 `immutable_skin() == BannerSkin::Audit` 立刻红。**如实标注**：
    /// 它不是从 LVGL 读回的"对象实际底色"，是"我挂了哪一档"的可读回记录。
    #[allow(dead_code)]
    immutable_bg: Cell<Color>,
    /// 说明条左缘 4 px 色条。
    immutable_bar: Obj,
    /// 说明条锁形图标。
    immutable_icon: Label,
    /// 说明条正文。
    immutable_text: Label,
    // ── 筛选区 ──
    /// 共享「时间范围」件（`filters.rs`）。
    filter: Rc<TimeRangeFilter>,
    /// 操作类型维度名标签。
    ops_label: Label,
    /// chip 组容器（重建 chip 组时替换其中的子对象）。
    ops_box: Obj,
    /// chip 组（可点；`None` = 尚未建 / 建失败）。
    ops: RefCell<Option<Rc<MultiSelectChips>>>,
    /// chip 组当前**选项文案**（重建判据；也是"选项集合以注入为准"的读回口径）。
    ops_texts: RefCell<Vec<String>>,
    /// 当前**归一化后**的勾选集合（跨重建保持）。
    ops_sel: RefCell<Vec<usize>>,
    /// 注入的选项表（缺省 = [`canonical_ops`]）。
    ops_opts: RefCell<Vec<OpOption>>,
    // ── 超限提示（**AU5**）──
    /// `WarnBanner`（**懒惰构建**：首个 [`P5AuditPage::set_range_too_large`]`(true)` 才建 ——
    /// 见 **AU5** / **M5**：EDGE-15 在当前契约下**生产不可达**（`AuditPage` 无该字段），
    /// 常驻构建等于让每一页都背着一份"永不显形"的构件）。
    warn: RefCell<Option<Rc<WarnBanner>>>,
    /// 超限标志（唯一来源是 [`P5AuditPage::set_range_too_large`]，见 **AU5**）。
    range_too_large: Cell<bool>,
    // ── 表头 ──
    /// 表头容器（5 列名 + 4 条竖分隔线 + 底线）。
    head: Obj,
    /// 表头 **5 个列名标签**（**拥有型句柄，必须锚定在这里** —— 见模块头的所有权纪律。
    /// 它们若是 `new()` 的局部变量，返回时即 `Drop` ⇒ `lv_obj_delete` **级联删除整棵子树**
    /// ⇒ 表头在屏上整块空白，而容器仍存活、尺寸仍 36 ⇒ "网看着在、实则没把住"）。
    #[allow(dead_code)]
    head_cols: Vec<Label>,
    /// 表头 **4 条竖分隔线**（同上：拥有型 `Obj`，必须锚定）。
    #[allow(dead_code)]
    head_divs: Vec<Obj>,
    /// 表头 **底线**（同上）。
    #[allow(dead_code)]
    head_rule: Obj,
    // ── 列表 ──
    /// 列表容器（行 + 底部状态行 + 说明行 + 空 / 不可用态）。
    list_box: Obj,
    /// 行池（**只增不减**：避免滚动加载过程中的对象 churn；上限见 **AU8**）。
    rows: RefCell<Vec<Row>>,
    /// 当前应显示的行数（`apply_page` 写入；`layout` 据此显隐）。
    shown: Cell<usize>,
    /// 空态（EDGE-08）。
    empty: Rc<EmptyState>,
    /// 不可用态（EDGE-17；标题 / 图标 / 色**全部**取自 `UnavailableKind::Audit`）。
    ///
    /// `RefCell` 是因为 `UnavailableState` 的**原因文案在构造期固定**（`components.rs` 无 setter）
    /// ⇒ 原因变化时**重建**该组件（注入路径的对象 churn，非渲染路径；与 chip 组重建同款处置）。
    unavailable: RefCell<Rc<UnavailableState>>,
    /// 最近一次不可用原因（重建判据）。
    unav_reason: RefCell<String>,
    /// 底部状态行（`加载中` / `已加载全部`）。
    footer: Label,
    /// 只读说明行（**AU11**）。
    note: Label,
    // ── 注入态 ──
    /// 最近一次注入的可用性（`false` ⇒ EDGE-17；缺省即契约缺省的**安全方向**）。
    available: Cell<bool>,
    /// 最近一次注入的页对象（供"加载更多"取 `page` / `has_more`，以及选项变化后重刷标签）。
    last_page: RefCell<Option<AuditPage>>,
    /// 最近一次注入的 `has_more`。
    has_more: Cell<bool>,
    /// 最近一次**已发出**的筛选意图（**去重**：相同条件不重复发）。
    last_query: RefCell<Option<AuditQuery>>,
    /// 筛选变化意图（**本页不生成 `request_id`**）。
    on_query: QuerySlot,
    /// 「加载更多」意图（见 **AU6** / **AU14**）。
    on_load_more: QuerySlot,
    /// 内核自引用（`Weak`，**不构成 `Rc` 环**）：chip 组在**重建路径**里需要它挂回调。
    me: RefCell<Weak<Core>>,
}

impl Core {
    // ── 布局（**唯一摆放点**：档位 / 超限 / 行数变化后都要重摆）────────────────

    /// 按当前状态摆放全部区块（只改位置 / 尺寸 / 可见性，**不新建对象**）。
    ///
    /// y 锚点（页内 = UI 绝对 − 72）：最近审计条 8（绝对 80）→ 说明条 44（116）→
    /// 时间范围 100（172）→ 操作类型（时间范围体高之后）→【超限条】→ 表头 → 列表。
    fn layout(&self) {
        let y_newest = Dimens::CONTENT_PAD_TOP;
        let y_immutable = y_newest + NEWEST_H;
        let y_range = y_immutable + IMMUTABLE_H + TIGHT_GAP;
        // ⚠️ 必须用 `filters::body_h`（**纯函数**）而不是 `size()`：本函数会在**事件回调内**
        // 被调用（用户切到「自定义」的当场），此时 LVGL 的 `coords` 还没重算。
        let filter_h = self.filter.body_h();
        let y_ops = y_range + filter_h + Dimens::GAP_MIN;
        let y_after_ops = y_ops + OPS_BLOCK_H;
        // **超限条是懒惰构建的**（M5）：没建出来时即使 `range_too_large` 为真也**不占位**
        // （否则表头 / 列表会被一条不存在的 banner 顶下去，屏上留下一段空白）。
        // 克隆 `Rc` 后**立刻放掉借用**（本函数会在事件回调内被调用 ⇒ 不得跨调用持借用）。
        let warn: Option<Rc<WarnBanner>> = self.warn.borrow().clone();
        let warn_on = self.range_too_large.get() && warn.is_some();
        let y_head = if warn_on {
            y_after_ops + Dimens::BANNER_H + TIGHT_GAP
        } else {
            y_after_ops
        };
        let y_list = y_head + TABLE_HEAD_H;

        self.newest.set_pos(
            0,
            y_newest + theme::center_offset(NEWEST_H, TextSlot::Body.px() as i32),
        );
        self.immutable.set_pos(0, y_immutable);
        self.immutable_bar.set_pos(0, 0);
        self.immutable_icon
            .set_size(IMMUTABLE_ICON_W, IMMUTABLE_ICON_W);
        self.immutable_icon
            .set_pos(IMMUTABLE_ICON_X, theme::center_offset(IMMUTABLE_H, IMMUTABLE_ICON_W));
        self.immutable_text.set_pos(
            IMMUTABLE_TEXT_X,
            theme::center_offset(IMMUTABLE_H, TextSlot::Body.px() as i32),
        );
        self.filter.obj().set_pos(0, y_range);
        self.ops_label.set_pos(
            0,
            y_ops + theme::center_offset(Dimens::CHIP_H, TextSlot::Label.px() as i32),
        );
        self.ops_box.set_pos(filters::CTRL_X, y_ops);
        if let Some(w) = warn.as_ref() {
            w.obj().set_pos(0, y_after_ops);
            set_visible(w.obj(), warn_on);
        }
        self.head.set_pos(0, y_head);
        self.list_box.set_pos(0, y_list);

        // 列表区三态（互斥）—— `available = false` 时**只**显不可用态（EDGE-17）：
        // 此时 `entries` 不可信 ⇒ 行数按 **0** 处理（即使注入了条目也不上屏）。
        let shown = if self.available.get() {
            self.shown.get()
        } else {
            0
        };
        {
            let rows = self.rows.borrow();
            for (i, r) in rows.iter().enumerate() {
                set_visible(&r.obj, i < shown);
            }
        }
        let view = self.view();
        set_visible(self.empty.obj(), view == ListView::Empty);
        set_visible(self.unavailable.borrow().obj(), view == ListView::Unavailable);
        // 列表容器高（行 + 状态行 + 说明行）；空 / 不可用态各占其自身构件高。
        let list_h = match view {
            ListView::Rows => shown as i32 * Dimens::ROW_AUDIT_H + 2 * NOTE_H,
            _ => unavailable_h(),
        };
        self.list_box.set_size(Dimens::CONTENT_W, list_h);
        // 底部状态行（无行时不显）/ 只读说明行。
        let footer = footer_text(self.has_more.get(), shown);
        match footer {
            Some(t) => {
                self.footer.set_text(t);
                self.footer.set_pos(0, shown as i32 * Dimens::ROW_AUDIT_H);
                set_visible(self.footer.obj(), view == ListView::Rows);
            }
            None => set_visible(self.footer.obj(), false),
        }
        self.note
            .set_pos(0, shown as i32 * Dimens::ROW_AUDIT_H + NOTE_H);
        set_visible(self.note.obj(), view == ListView::Rows);
        // 空 / 不可用态贴列表区顶部。
        self.empty.set_pos(0, 0);
        self.unavailable.borrow().set_pos(0, 0);
    }

    /// 当前列表区形态（**与 [`list_view`] 共用 [`list_view_of`] 这一份判据** —— 见其文档）。
    fn view(&self) -> ListView {
        list_view_of(self.available.get(), self.shown.get())
    }

    // ── 注入（外部 → 屏）───────────────────────────────────────────────────

    /// 注入一页（`entries` = 外部组装好的**列表窗口**；`page` / `has_more` = 服务端分页态）。
    fn apply_page(&self, page: &AuditPage) {
        self.available.set(page.available);
        self.has_more.set(page.has_more);
        let want = page.entries.len().min(ROW_MAX);
        // 行池**按需分配**（评审 ② 的确认）：**只按本次 `entries` 条数建**（上限 [`ROW_MAX`]），
        // **不是**一次建满 [`ROW_MAX`] —— 少于 20 条的注入只付它自己的堆开销。
        // 池**只增不减**（避免每页重建对象 churn）。
        {
            let mut rows = self.rows.borrow_mut();
            while rows.len() < want {
                match Row::new(&self.list_box, rows.len()) {
                    Ok(r) => rows.push(r),
                    Err(_) => break, // 建不出即停（**不 panic**）
                }
            }
            // ⚠️ **`shown` 以"实际建出 / 池内现有"的条数为准**：此前先写 `want` 再建行，
            // 建不出时 `shown` 仍留在 `want` ⇒ 布局按"有 want 行"摆底部状态行 / 说明行
            // （它们会被推到屏外），而实际只有更少的行 —— 静默的形态不一致。
            self.shown.set(rows.len().min(want));
        }
        // **时间倒序由本页保证**（§6.5；见 [`row_order`]）—— 注入乱序也按 `ts_ms` 降序上屏。
        let order = row_order(&page.entries);
        let opts = self.ops_opts.borrow().clone();
        {
            let rows = self.rows.borrow();
            for (i, k) in order.iter().take(want).enumerate() {
                if let Some(r) = rows.get(i) {
                    r.apply(&page.entries[*k], &op_label_of(page.entries[*k].op, &opts));
                }
            }
        }
        // 最近审计条（`None` ⇒ 占位，**绝不编造**）。
        self.newest.set_text(&newest_text(page.newest_ts_ms));
        *self.last_page.borrow_mut() = Some(page.clone());
        self.layout();
    }

    /// 按当前选项表刷新**已上屏行**的操作类型列（只改文本，不动对象）。
    ///
    /// **必须与 [`Core::apply_page`] 用同一个行序**（[`row_order`]）：否则会"行按时间倒序显示、
    /// 标签却按注入序刷新" ⇒ 第 i 行的操作类型与第 i 行的记录错位。
    fn refresh_op_labels(&self) {
        let opts = self.ops_opts.borrow().clone();
        let page = self.last_page.borrow().clone();
        let Some(page) = page else { return };
        let order = row_order(&page.entries);
        let rows = self.rows.borrow();
        for (i, k) in order.iter().take(self.shown.get()).enumerate() {
            if let Some(r) = rows.get(i) {
                r.apply(&page.entries[*k], &op_label_of(page.entries[*k].op, &opts));
            }
        }
    }

    /// 重建 chip 组（**仅在选项文案变化时**；对象 churn 限于注入路径，非渲染路径）。
    fn rebuild_ops(&self, texts: Vec<String>) {
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        // 先放掉旧件（`Drop` ⇒ `lv_obj_delete` ⇒ 级联删除旧 chip 与它们的回调 ——
        // 事件桥的延迟回收保证闭包恰好 drop 一次，见 `src/lvgl/event.rs`）。
        drop(self.ops.borrow_mut().take());
        match MultiSelectChips::new(&self.ops_box, &refs, OPS_COLS, OPS_CHIP_W) {
            Ok(chips) => {
                let chips = Rc::new(chips);
                // 恢复当前勾选（`set_selected` 是程序化设置，**不触发**回调 ⇒ 不误发意图）。
                for i in self.ops_sel.borrow().iter().copied() {
                    chips.set_selected(i, true);
                }
                // ⚠️ **回调只持 `Weak<Core>`**（**C1**）：chip 组的回调槽由 `Core.ops` 持有
                // （`Core → ops → chips.on_change`）⇒ 闭包若捕获 `Rc<Core>` 就构成**强引用环**
                // ⇒ `drop(P5AuditPage)` **不释放任何 LVGL 对象**（整棵树连 `root` 一起永久留在
                // 宿主上），二次构造即 `OutOfMemory`。照 `p2_config.rs` / `p4_interlock.rs` 的
                // 同款处置：捕获 `Weak`，回调内 `upgrade()`（自引用弱化，是本页**唯一**的环）。
                let w = self.me.borrow().clone();
                chips.set_on_change(move |sel| {
                    let Some(me) = w.upgrade() else { return };
                    me.on_ops_changed(sel);
                });
                *self.ops.borrow_mut() = Some(chips);
            }
            Err(e) => {
                // 建不出 chip 组：**不 panic**、不上屏（上屏文案必须逐字在 cmap 内），只写 stderr
                // （与 `p4_interlock.rs` **IL25** 同口径：同一失效域内上屏不可靠）。
                report_ops_rebuild_failure(&e);
            }
        }
        *self.ops_texts.borrow_mut() = texts;
        self.layout();
    }

    /// chip 勾选变化：归一化（「全部」语义）→ 报意图。
    fn on_ops_changed(&self, selected: Vec<usize>) {
        let norm = {
            let prev = self.ops_sel.borrow();
            normalize_ops_selection(&prev, &selected)
        };
        *self.ops_sel.borrow_mut() = norm.clone();
        if let Some(chips) = self.ops.borrow().as_ref() {
            let cur = chips.selected();
            for i in 0..chips.len() {
                if cur.contains(&i) != norm.contains(&i) {
                    chips.set_selected(i, norm.contains(&i));
                }
            }
        }
        self.fire_query();
    }

    // ── 意图（屏 → 外部）────────────────────────────────────────────────────

    /// 组装**当前**筛选态 + 指定页号的查询。
    fn current_query(&self, page: u32) -> AuditQuery {
        let change = self.filter.change();
        let sel = self.ops_sel.borrow();
        let opts = self.ops_opts.borrow();
        audit_query(&change, &selected_ops(&sel, &opts), page)
    }

    /// 发「筛选变化」意图（**去重**：与上一次**已发出**的查询相同则不发）。
    ///
    /// 回调经 [`CbSlot::fire`]（take/put-back）触发：**调用期不持借用** ⇒ 回调内再
    /// [`P5AuditPage::set_on_query`] 时 `try_borrow_mut` 必然成功、新回调自**下一次**通知
    /// 起生效（本单元 ③ 的整改点：原实现持借用直调 ⇒ 自替换被**静默丢弃**）。
    ///
    /// ⚠️ **本函数刻意不自行 take/put-back**（收口 ①）：`self.on_query` 是 [`CbSlot`]，
    /// 内部借用不外露 ⇒ 此处**写不出**旧写法（只有 `set` / `fire` 两个动作）。
    fn fire_query(&self) {
        let q = self.current_query(1);
        let same = self.last_query.borrow().as_ref() == Some(&q);
        if same {
            return; // 「未变化时不发意图」——离屏用例逐条锁住
        }
        *self.last_query.borrow_mut() = Some(q.clone());
        self.on_query.fire(q);
    }

    /// 发「加载更多」意图（`has_more = false` ⇒ **不发**；页号 = 当前页 + 1）。
    ///
    /// 回调触发口径同 [`Core::fire_query`]（[`CbSlot::fire`] 的 take/put-back，调用期不持借用）。
    fn fire_load_more(&self) {
        if !self.has_more.get() {
            return;
        }
        let cur = self
            .last_page
            .borrow()
            .as_ref()
            .map(|p| p.page)
            .unwrap_or(1);
        let q = self.current_query(cur.saturating_add(1));
        self.on_load_more.fire(q);
    }

    /// **懒惰构建**超限提示条（幂等；见 **AU5** / **M5**）。
    ///
    /// 返回"现在是否已有该构件"。建不出 ⇒ 只写 stderr（**不 panic**、不上屏 —— 与
    /// `p4_interlock.rs` **IL25** 同口径），且**不占布局位**（见 [`Core::layout`]）。
    #[cfg(test)]
    fn ensure_warn(&self) -> bool {
        if self.warn.borrow().is_some() {
            return true;
        }
        match WarnBanner::new(&self.root, Dimens::CONTENT_W, TEXT_RANGE_TOO_LARGE, &[]) {
            Ok(b) => {
                let b = Rc::new(b);
                b.set_hidden(true); // 建出来先隐（显隐由 `range_too_large` 决定）
                *self.warn.borrow_mut() = Some(b);
                true
            }
            Err(e) => {
                report_warn_build_failure(&e);
                false
            }
        }
    }

    /// 重建不可用态（原因文案在构造期固定 ⇒ 原因变化时换一个组件实例）。
    fn set_unavailable_reason(&self, reason: &str) {

        if *self.unav_reason.borrow() == reason {
            return;
        }
        *self.unav_reason.borrow_mut() = reason.to_string();
        match UnavailableState::new(&self.list_box, UnavailableKind::Audit, reason) {
            Ok(s) => *self.unavailable.borrow_mut() = Rc::new(s),
            Err(e) => report_unavailable_rebuild_failure(&e),
        }
    }
}

/// 空 / 不可用态占位高（两者的**构件算式**与 `components.rs` 内一致；页面构造期读不到实测高，
/// 与 `p4_interlock.rs` 的 `EMPTY_H` / `UNAVAILABLE_H` 同法，并由 `ui/tests.rs` 钉住）。
fn unavailable_h() -> i32 {
    let empty_h = Dimens::ICON_LG
        + Dimens::GAP_MIN
        + TextSlot::SectionTitle.px() as i32
        + Dimens::GAP_MIN;
    empty_h + TextSlot::Body.px() as i32 + Dimens::GAP_MIN
}

/// chip 组重建失败的 stderr 诊断（**绝不 panic、绝不上屏**）。
fn report_ops_rebuild_failure(e: &LvglError) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "P5 审计页：操作类型选项重建失败：{e}");
}

/// 不可用态重建失败的 stderr 诊断（同上，**保留旧件**）。
fn report_unavailable_rebuild_failure(e: &LvglError) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "P5 审计页：不可用态重建失败：{e}");
}

/// 超限提示条构建失败的 stderr 诊断（**懒惰构建**的失败路径；同上，不上屏）。
#[cfg(test)]
fn report_warn_build_failure(e: &LvglError) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "P5 审计页：超限提示条构建失败：{e}");
}

/// 说明条**样式族**（[`P5AuditPage::immutable_skin`] 的取值）—— 由**实际写入样式的底色**
/// 判定，不是"本文件自称用了哪一档"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum BannerSkin {
    /// 审计专属底（`Palette::AUDIT_BG`，§6.5 的"合规凭据"色）。
    Audit,
    /// 警示条底（`Palette::WARN_BG`）。
    Warn,
    /// 危险条底（`Palette::DANGER_BG`）。
    Danger,
    /// 卡内嵌区底（`Palette::SURFACE_ALT`）。
    SurfaceAlt,
    /// 卡片底（`Palette::SURFACE`）。
    Surface,
    /// 其它（未登记的色值）。
    Other,
}

/// 底色 → 样式族（**唯一判据**；纯函数、可独立单测）。
#[cfg(test)]
pub(crate) fn banner_skin_of(bg: Color) -> BannerSkin {
    if bg == Palette::AUDIT_BG {
        BannerSkin::Audit
    } else if bg == Palette::WARN_BG {
        BannerSkin::Warn
    } else if bg == Palette::DANGER_BG {
        BannerSkin::Danger
    } else if bg == Palette::SURFACE_ALT {
        BannerSkin::SurfaceAlt
    } else if bg == Palette::SURFACE {
        BannerSkin::Surface
    } else {
        BannerSkin::Other
    }
}

/// 不可篡改说明条的**底色**（**唯一色值真源**；**AU13** / B2c-1 代码质量评审 ⑥）。
///
/// **同一个值**既送 [`audit_banner_style`] 的 `set_bg_color(..)`，又作**应用标记**
/// [`Core::immutable_bg`]（[`audit_banner_bg`] 是本常量的唯一读口）。
const BANNER_BG: Color = Palette::AUDIT_BG;

/// 说明条的**应用标记**（= [`BANNER_BG`]，**同一常量**；色值在本文件只出现一次）。
pub(crate) fn audit_banner_bg() -> Color {
    BANNER_BG
}

/// 不可篡改说明条的样式（**AU13**：`theme.rs` 无现成样式，用**命名常量**组合，零裸色值）。
///
/// 色值对齐 UI §6.5：底 `#14231F`（[`BANNER_BG`]）；左缘 4 px `#35D0C4`
/// （`Dimens::ACCENT_BAR` + `Palette::SOC_OK`，由 `immutable_bar` 子对象承担）。
/// **无描边、零内边距**（与 `theme::warn_banner` / `theme::card_head_bar` 同口径：
/// 位置由调用方 `set_pos` 显式给出）。
///
/// ⚠️ **薄层没有 `bg_color` 读回**（**B4a 补齐的三个读回 `bg_opa` / `text_color` /
/// `text_font` 不含它**；`lv_obj_get_style_*` 那族 C 侧 `static inline` 仍不入绑定
/// —— 薄层是经 `lv_obj_get_style_prop` + `LV_STYLE_*` 复刻的，见 `src/lvgl/obj.rs`）
/// ⇒ 光靠本函数的入参**无法证明**"页面确实用了这一档"。
/// 故把**真正的一读**放在渲染侧：`pages_chain` 直接读**像素**（`sink` 中说明条内的取样点
/// 必须等于 `Palette::AUDIT_BG` 的 BGR 分量）—— 那是**样式被真的施加**之后的结果；
/// 应用标记只作第二道（记录"我打算用哪一档"），两者**同时**断言。
fn audit_banner_style() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(BANNER_BG);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(RADIUS_CTRL);
    s.set_border_width(Stroke::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. 页面
// ═══════════════════════════════════════════════════════════════════════════

/// P5 审计页（**只读**；控制通道驱动，见模块文档）。
///
/// # 意图如何交给外部
///
/// 本页**不发请求、不生成 `request_id`**：
/// - 筛选条件变化（时间范围档位 / 自定义起止 / 操作类型多选）⇒ [`P5AuditPage::set_on_query`]
///   注册的回调收到一份 [`AuditQuery`]（`page` 恒为 `1`）；
/// - 「加载更多」（滚动加载，见 **AU6**）⇒ [`P5AuditPage::request_next_page`] 触发
///   [`P5AuditPage::set_on_load_more`] 的回调（`page` = 当前页 + 1）。
///
/// 外部（B3 的 `console.rs`）据此生成 `request_id` / 发 `GET /v1/console/audit`，并把结果经
/// [`P5AuditPage::set_page`] 灌回。
pub struct P5AuditPage {
    core: Rc<Core>,
}

impl P5AuditPage {
    /// 在 `parent` 下建页（页根 `992 × 624`，**自身即纵向滚动容器** —— 契约 1；
    /// 摆放由调用方负责）。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = page_root(parent)?;

        // ── ① 头部三条 ──
        let newest = text_label(&root, &newest_text(None), TextSlot::Body, Palette::TEXT_SECOND)?;
        newest.set_size(Dimens::CONTENT_W, TextSlot::Body.px() as i32);
        newest.set_long_mode(LongMode::DOTS);

        // 不可篡改说明条：底 `Palette::AUDIT_BG` + 左缘 4 px `Palette::SOC_OK`（**AU13**）。
        // `immutable_bg` 是**应用标记**（与 `set_bg_color` 收到的是**同一个常量** [`BANNER_BG`]）；
        // "页面确实用了这一档"另由 `pages_chain` 的**像素读回**独立锁住（评审 ⑥）。
        let immutable_bg = audit_banner_bg();
        let immutable = decor(&root, Dimens::CONTENT_W, IMMUTABLE_H, &audit_banner_style())?;
        let immutable_bar = decor(
            &immutable,
            IMMUTABLE_BAR_W,
            IMMUTABLE_H,
            &theme::card_head_bar(Palette::SOC_OK),
        )?;
        immutable_bar.set_pos(0, 0);
        let immutable_icon = text_label(
            &immutable,
            TEXT_LOCK_ICON,
            theme::icon_slot(IMMUTABLE_ICON_W),
            Palette::SOC_OK,
        )?;
        let immutable_text = text_label(
            &immutable,
            TEXT_IMMUTABLE,
            TextSlot::Body,
            Palette::TEXT_SECOND,
        )?;
        immutable_text.set_size(
            Dimens::CONTENT_W - IMMUTABLE_TEXT_X,
            TextSlot::Body.px() as i32,
        );
        immutable_text.set_long_mode(LongMode::DOTS);

        // ── ② 筛选区（共享「时间范围」件 + 操作类型 chip 组）──
        let filter = filters::build(&root, TimeRangeChange::default())?;
        let ops_label = text_label(&root, TEXT_OPS_LABEL, TextSlot::Label, Palette::TEXT_SECOND)?;
        ops_label.set_size(filters::LABEL_W, TextSlot::Label.px() as i32);
        ops_label.set_long_mode(LongMode::DOTS);
        let ops_box = layout_box(&root, OPS_CHIP_W * OPS_COLS as i32, OPS_BLOCK_H)?;

        // ── ③ 超限提示条（EDGE-15 的 UI 落点；契约缺口见 **AU5**）──
        // **懒惰构建（M5）**：此处**不建**。EDGE-15 在 P5 侧生产不可达（`AuditPage` 无
        // `range_too_large` 字段 ⇒ B3 无从推出该标志），常驻构建等于每页都背一份"永不显形"
        // 的构件；改由 [`Core::ensure_warn`] 在首个 `set_range_too_large(true)` 时建。

        // ── ④ 表头（5 列名 + 4 条竖分隔线 + 底线）──
        //
        // ⚠️ **所有权纪律（本单元阻断级缺陷的修复点）**：表头的**每一个**子件都存进 `Core`
        // 的字段（`head_cols` / `head_divs` / `head_rule`），**不得**留在 `new()` 的局部变量里
        // —— 局部句柄随函数返回被 `Drop`（= `lv_obj_delete`）⇒ LVGL **级联删除整棵子树**
        // ⇒ 表头在屏上整块空白，而容器 `head_obj()` 仍存活、尺寸仍是 36（这正是"网看着在、
        // 实则没把住"的形态）。回归锁见 `ui/tests.rs::pages_chain` 的 `head_child_count()` +
        // 逐列文案读回。
        let head = layout_box(&root, Dimens::CONTENT_W, TABLE_HEAD_H)?;
        // 列名与逐列 x **由 [`HEAD_COLS`] / [`HEAD_COL_X`] 一一对应**（M1：单一真源，
        // 两处长度由编译器钉死；子件数 [`HEAD_CHILD_COUNT`] 自动跟随）。
        let mut head_cols: Vec<Label> = Vec::with_capacity(HEAD_COL_COUNT);
        for (t, x) in HEAD_COLS.iter().zip(HEAD_COL_X) {
            let l = text_label(&head, t, TextSlot::Body, Palette::TEXT_WEAK)?;
            l.set_size(Dimens::CHIP_MIN_W, TextSlot::Body.px() as i32);
            l.set_long_mode(LongMode::DOTS);
            l.set_pos(x, theme::center_offset(TABLE_HEAD_H, TextSlot::Body.px() as i32));
            head_cols.push(l);
        }
        // 竖分隔线 = **列间**分隔（列数 − 1 条），x 取"右邻列的列首 x"（同一张 [`HEAD_COL_X`]）。
        let mut head_divs: Vec<Obj> = Vec::with_capacity(HEAD_DIV_COUNT);
        for x in HEAD_COL_X.iter().skip(1) {
            let d = decor(
                &head,
                HEAD_DIV_W,
                HEAD_DIV_H,
                &theme::card_head_bar(Palette::DIVIDER),
            )?;
            d.set_pos(x - TIGHT_GAP, TIGHT_GAP);
            head_divs.push(d);
        }
        let head_rule = decor(
            &head,
            Dimens::CONTENT_W,
            HEAD_DIV_W,
            &theme::card_head_bar(Palette::DIVIDER),
        )?;
        head_rule.set_pos(0, TABLE_HEAD_H - HEAD_DIV_W);

        // ── ⑤ 列表区（行池 + 空态 + 不可用态 + 状态行 + 说明行）──
        let list_box = layout_box(&root, Dimens::CONTENT_W, unavailable_h())?;
        let empty = Rc::new(EmptyState::new(&list_box, TEXT_EMPTY_ICON, TEXT_EMPTY)?);
        let unavailable = Rc::new(UnavailableState::new(
            &list_box,
            UnavailableKind::Audit,
            // 契约 `AuditPage` **不带不可用原因**（只有 `available: bool`）⇒ **不臆造**原因
            // （与 `p4_interlock.rs` **IL8** 同口径；该槽已是参数化入口，见 `set_unavailable`）。
            "",
        )?);
        let footer = label(&list_box, TextSlot::Body, Palette::TEXT_WEAK)?;
        footer.set_size(Dimens::CONTENT_W, TextSlot::Body.px() as i32);
        footer.set_long_mode(LongMode::DOTS);
        let note = text_label(
            &list_box,
            &format!("{TEXT_EXPORT_NOTE}{TEXT_CLAUSE_SEP}{TEXT_EXPORT_NOTE2}"),
            TextSlot::Body,
            Palette::TEXT_WEAK,
        )?;
        note.set_size(Dimens::CONTENT_W, TextSlot::Body.px() as i32);
        note.set_long_mode(LongMode::DOTS);

        let core = Rc::new(Core {
            root,
            newest,
            immutable,
            immutable_bg: Cell::new(immutable_bg),
            immutable_bar,
            immutable_icon,
            immutable_text,
            filter: Rc::clone(&filter),
            ops_label,
            ops_box,
            ops: RefCell::new(None),
            ops_texts: RefCell::new(Vec::new()),
            ops_sel: RefCell::new(vec![0]),
            ops_opts: RefCell::new(canonical_ops()),
            warn: RefCell::new(None),
            range_too_large: Cell::new(false),
            head,
            head_cols,
            head_divs,
            head_rule,
            list_box,
            rows: RefCell::new(Vec::new()),
            shown: Cell::new(0),
            empty,
            unavailable: RefCell::new(unavailable),
            unav_reason: RefCell::new(String::new()),
            footer,
            note,
            available: Cell::new(false),
            last_page: RefCell::new(None),
            has_more: Cell::new(false),
            last_query: RefCell::new(None),
            on_query: CbSlot::new(),
            on_load_more: CbSlot::new(),
            me: RefCell::new(Weak::new()),
        });
        *core.me.borrow_mut() = Rc::downgrade(&core);

        // chip 组（缺省 = 契约规范选项表；服务端注入后由 `set_ops` 替换）。
        let texts = op_options(&core.ops_opts.borrow());
        core.rebuild_ops(texts);

        // 时间范围变化 ⇒ 重摆（自定义展开会改体高）+ 报意图（去重由 `fire_query` 承担）。
        {
            let w = Rc::downgrade(&core);
            core.filter.set_on_change(move |_c: TimeRangeChange| {
                let Some(c) = w.upgrade() else { return };
                c.layout();
                c.fire_query();
            });
        }

        let page = Self { core };
        // 骨架态 = 无注入 ⇒ 契约缺省 `available = false` ⇒ 屏显「审计记录不可用」
        // （**绝不**显「无审计记录」—— §8.3 EDGE-17）。
        page.core.layout();
        Ok(page)
    }

    // ── 对象读回（装配 / 断言口径）──────────────────────────────────────────

    /// 页根（= 纵向滚动容器）。
    pub fn obj(&self) -> &Obj {
        &self.core.root
    }

    /// 最近审计条文案（**读回口**）。
    #[cfg(test)]
    pub(crate) fn newest_text(&self) -> Option<String> {
        self.core.newest.text()
    }

    /// 最近审计条**对象**（装配断言 / 存活探针口径；**仅测试可见**）。
    #[cfg(test)]
    pub(crate) fn newest_obj(&self) -> &Obj {
        self.core.newest.obj()
    }

    /// 不可篡改说明条的底（装配 / **存活锚点**：它被误删会让色条与文案一起消失）。
    #[cfg(test)]
    pub(crate) fn immutable_obj(&self) -> &Obj {
        &self.core.immutable
    }

    /// 不可篡改说明条文案。
    #[cfg(test)]
    pub(crate) fn immutable_text(&self) -> Option<String> {
        self.core.immutable_text.text()
    }

    /// 不可篡改说明条图标字形。
    #[cfg(test)]
    pub(crate) fn immutable_icon(&self) -> Option<String> {
        self.core.immutable_icon.text()
    }

    /// 不可篡改说明条的左缘色条对象（**每条装配断言的锚点**）。
    #[cfg(test)]
    pub(crate) fn immutable_bar_obj(&self) -> &Obj {
        &self.core.immutable_bar
    }

    /// 不可篡改说明条**实际应用**的样式底（**应用标记**；见 **AU13**）。
    ///
    /// **如实标注**：它不是从 LVGL 读回的对象底色（薄层无该通道），而是页面送给样式构造器的
    /// 那个色值 —— 与 `set_bg_color(..)` 收到的是同一个表达式。
    #[cfg(test)]
    pub(crate) fn immutable_bg(&self) -> Color {
        self.core.immutable_bg.get()
    }

    /// 不可篡改说明条的**样式族**（= [`banner_skin_of`]`(`[`Self::immutable_bg`]`())`）。
    ///
    /// **改什么会让本条变红**：把 [`audit_banner_style`] 的底色换成 `Palette::WARN_BG`
    /// ⇒ 本值变成 [`BannerSkin::Warn`] ⇒ `ui/tests.rs::pages_chain` 的断言立刻红。
    #[cfg(test)]
    pub(crate) fn immutable_skin(&self) -> BannerSkin {
        banner_skin_of(self.core.immutable_bg.get())
    }

    /// 表头**子件数**（读 LVGL 的 `child_count()`，恒 [`HEAD_CHILD_COUNT`]）。
    ///
    /// **这条网捕什么**：表头子件是 `new()` 的局部变量时，函数返回即 `Drop`
    /// （= `lv_obj_delete`）⇒ 级联删除整棵子树 ⇒ 本值从 10 **掉到 0**，屏上表头整块空白。
    /// 只断言"容器存活 / 容器高 36"**抓不到**它（容器本身还在）。
    #[cfg(test)]
    pub(crate) fn head_child_count(&self) -> usize {
        self.core.head.child_count() as usize
    }

    /// 第 `i` 列表头文案（越界 ⇒ `None`）—— 与 [`Self::head_child_count`] 一读一写两路锁住
    /// "表头真的在屏上且内容正确"。
    #[cfg(test)]
    pub(crate) fn head_col_text(&self, i: usize) -> Option<String> {
        self.core.head_cols.get(i).and_then(|l| l.text())
    }

    /// 存活的表头竖分隔线数（锚定回归锁；恒 4）。
    #[cfg(test)]
    pub(crate) fn head_divs_alive(&self) -> usize {
        self.core
            .head_divs
            .iter()
            .filter(|o| o.is_alive())
            .count()
    }

    /// 表头底线是否存活（锚定回归锁）。
    #[cfg(test)]
    pub(crate) fn head_rule_alive(&self) -> bool {
        self.core.head_rule.is_alive()
    }

    /// 共享「时间范围」件（装配 / 断言口径）。
    pub fn filter(&self) -> &Rc<TimeRangeFilter> {
        &self.core.filter
    }

    /// 操作类型维度名标签。
    #[cfg(test)]
    pub(crate) fn ops_label_text(&self) -> Option<String> {
        self.core.ops_label.text()
    }

    /// 操作类型 chip 组容器。
    #[cfg(test)]
    pub(crate) fn ops_obj(&self) -> &Obj {
        &self.core.ops_box
    }

    /// chip 数（`0` = 尚未建 / 建失败）。
    #[cfg(test)]
    pub(crate) fn ops_chip_count(&self) -> usize {
        self.core.ops.borrow().as_ref().map(|c| c.len()).unwrap_or(0)
    }

    /// 第 `i` 个 chip 的**当前显示文本**（含选中前缀 `✓ `）。
    #[cfg(test)]
    pub(crate) fn ops_chip_display(&self, i: usize) -> Option<String> {
        self.core
            .ops
            .borrow()
            .as_ref()
            .and_then(|c| c.chip_display(i))
    }

    /// 当前勾选集合（归一化后）。
    #[cfg(test)]
    pub(crate) fn ops_selected(&self) -> Vec<usize> {
        self.core.ops_sel.borrow().clone()
    }

    /// 注入的选项表（读回）。
    #[cfg(test)]
    pub(crate) fn ops_options(&self) -> Vec<OpOption> {
        self.core.ops_opts.borrow().clone()
    }

    /// 超限提示条是否在显（EDGE-15；**AU5**）。
    ///
    /// **懒惰构建（M5）**：尚未建出 ⇒ 恒 `false`（不是"没显"，是"还没有这个构件"）。
    #[cfg(test)]
    pub(crate) fn warn_visible(&self) -> bool {
        self.core
            .warn
            .borrow()
            .as_ref()
            .is_some_and(|w| !w.obj().is_hidden())
    }

    /// 超限提示条文案（未懒惰建出 ⇒ `None`）。
    #[cfg(test)]
    pub(crate) fn warn_text(&self) -> Option<String> {
        self.core.warn.borrow().as_ref().and_then(|w| w.text())
    }

    /// 表头容器（装配断言口径）。
    #[cfg(test)]
    pub(crate) fn head_obj(&self) -> &Obj {
        &self.core.head
    }

    /// 列表容器（装配断言口径）。
    #[cfg(test)]
    pub(crate) fn list_obj(&self) -> &Obj {
        &self.core.list_box
    }

    /// 已建行对象数（＝行池长度；可见性见 [`Self::visible_rows`]）。
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

    /// 存活的**行对象**数（行池里 `is_alive()` 为真的个数 —— **所有权锚定回归锁**：
    /// 行句柄若退化成 `new()` 的局部变量，返回时即被 `Drop` ⇒ 行整棵子树被级联删除，
    /// 本条立刻小于池长，且屏上"有行但空白"）。
    #[cfg(test)]
    pub(crate) fn rows_alive(&self) -> usize {
        self.core
            .rows
            .borrow()
            .iter()
            .filter(|r| r.obj.is_alive())
            .count()
    }

    /// 第 `i` 行**容器**的尺寸（布局断言口径）。
    #[cfg(test)]
    pub(crate) fn row_size(&self, i: usize) -> Option<(i32, i32)> {
        self.core.rows.borrow().get(i).map(|r| r.obj.size())
    }

    /// 第 `i` 行**容器**的位置（布局断言口径）。
    #[cfg(test)]
    pub(crate) fn row_pos(&self, i: usize) -> Option<(i32, i32)> {
        let c = self.core.rows.borrow().get(i).map(|r| r.obj.coords())?;
        Some((c.x1, c.y1))
    }

    /// 第 `i` 行左缘竖条的**尺寸**（装配断言口径：宽 = 3 px、高 = 行高）。
    ///
    /// ⚠️ 只能返回**值**（尺寸 / 坐标），不能返回 `&Obj`：`RefCell` 的借用守卫不能逃逸出函数
    /// （`Obj` 的句柄本身是 `Copy` 语义的借用 + 内部 `Rc`，但**列在 `RefCell` 里**就受此约束）。
    #[cfg(test)]
    pub(crate) fn row_bar_size(&self, i: usize) -> Option<(i32, i32)> {
        self.core.rows.borrow().get(i).map(|r| r.bar.size())
    }

    /// 第 `i` 行时间列文本。
    #[cfg(test)]
    pub(crate) fn row_time(&self, i: usize) -> Option<String> {
        self.core.rows.borrow().get(i).and_then(|r| r.time.text())
    }

    /// 第 `i` 行操作者列文本。
    #[cfg(test)]
    pub(crate) fn row_operator(&self, i: usize) -> Option<String> {
        self.core
            .rows
            .borrow()
            .get(i)
            .and_then(|r| r.operator.text())
    }

    /// 第 `i` 行**结果胶囊**的文本（读在显的那一支）。
    #[cfg(test)]
    pub(crate) fn row_result(&self, i: usize) -> Option<String> {
        let rows = self.core.rows.borrow();
        let r = rows.get(i)?;
        let k = r.result_index()?;
        r.results.get(k)?.text()
    }

    /// 第 `i` 行结果胶囊的**色通道**（三重冗余的颜色通道；读在显的那一支的皮肤）。
    #[cfg(test)]
    pub(crate) fn row_result_accent(&self, i: usize) -> Option<Color> {
        let rows = self.core.rows.borrow();
        let r = rows.get(i)?;
        let k = r.result_index()?;
        r.results.get(k).map(|c| c.accent())
    }

    /// 第 `i` 行操作类型文本。
    #[cfg(test)]
    pub(crate) fn row_op(&self, i: usize) -> Option<String> {
        self.core.rows.borrow().get(i).and_then(|r| r.op.text())
    }

    /// 第 `i` 行前后值摘要。
    #[cfg(test)]
    pub(crate) fn row_summary(&self, i: usize) -> Option<String> {
        self.core
            .rows
            .borrow()
            .get(i)
            .and_then(|r| r.summary.text())
    }

    /// 第 `i` 行失败原因文本（**即使隐藏也返回其内容**；可见性见 [`Self::row_reason_visible`]）。
    #[cfg(test)]
    pub(crate) fn row_reason(&self, i: usize) -> Option<String> {
        self.core.rows.borrow().get(i).and_then(|r| r.reason.text())
    }

    /// 第 `i` 行失败原因列是否可见（**仅失败行可见**）。
    #[cfg(test)]
    pub(crate) fn row_reason_visible(&self, i: usize) -> bool {
        self.core
            .rows
            .borrow()
            .get(i)
            .map(|r| !r.reason.obj().is_hidden())
            .unwrap_or(false)
    }

    /// **只读回归锁**：列表区内**可点子对象计数**（恒 `0`）。
    ///
    /// 读的是 LVGL 的 `LV_OBJ_FLAG_CLICKABLE` 真值（不是"本文件没写按钮"的自述）——
    /// 一旦有人在行里加按钮 / 让行本体可点（长按菜单一类入口），本条立刻 > 0。
    #[cfg(test)]
    pub(crate) fn list_clickable_count(&self) -> usize {
        self.core
            .rows
            .borrow()
            .iter()
            .map(|r| r.clickable_parts())
            .sum()
    }

    /// 列表区三态（`Rows` / `Empty` / `Unavailable`）。
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

    /// 不可用态是否在显（EDGE-17）。
    #[cfg(test)]
    pub(crate) fn unavailable_visible(&self) -> bool {
        !self.core.unavailable.borrow().obj().is_hidden()
    }

    /// 不可用态标题（**由 `UnavailableKind::Audit` 决定**，不是调用方给的）。
    #[cfg(test)]
    pub(crate) fn unavailable_title(&self) -> Option<String> {
        self.core.unavailable.borrow().title()
    }

    /// 不可用态场景。
    #[cfg(test)]
    pub(crate) fn unavailable_kind(&self) -> UnavailableKind {
        self.core.unavailable.borrow().kind()
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

    /// 只读说明行文本（**AU11**）。
    #[cfg(test)]
    pub(crate) fn note_text(&self) -> Option<String> {
        self.core.note.text()
    }

    /// **只读页的结构性声明**：写入口清单**恒空**（UI §6.5「只读约束」行 / PL-02）。
    ///
    /// 它不是"文档口号"：`ui/tests.rs::p5_static_constraints` 断言本页源码里**不出现**
    /// 任何按钮构造 / 删除清空导出类标识符，且 [`Self::list_clickable_count`] 在运行期读
    /// LVGL 标志作第二道网。
    pub const WRITE_ENTRIES: [&'static str; 0] = [];

    // ── 数据入口（**控制通道驱动**，契约 2′）──────────────────────────────────

    /// 注入一页审计记录（`entries` = 外部组装好的窗口；`available = false` ⇒ EDGE-17）。
    pub fn set_page(&self, page: &AuditPage) {
        self.core.apply_page(page);
    }

    /// 注入操作类型选项（`/v1/console/audit/ops`；**选项文件变化即重建 chip 组**）。
    ///
    /// 选项集合**以注入为准**（服务端真源）：文案与当前不一致时才重建（避免无谓 churn）；
    /// 重建在**注入路径**（非渲染路径）发生，旧 chip 组随替换被 `Drop`（级联删除旧对象与回调）。
    pub fn set_ops(&self, opts: &[OpOption]) {
        *self.core.ops_opts.borrow_mut() = opts.to_vec();
        let texts = op_options(opts);
        if *self.core.ops_texts.borrow() != texts {
            self.core.rebuild_ops(texts);
        }
        // 标签可能变了 ⇒ 已上屏行的操作类型列跟着刷新（同一数据、同一出口）。
        self.core.refresh_op_labels();
    }

    /// 审计库不可用（EDGE-17）——**显式入口**（`reason` 为自由文本，经 [`display_safe`]）。
    ///
    /// ⚠️ 与 [`Self::set_page`] 的关系：`set_page` 已按注入的 `available` 字段自动分派；
    /// 本入口供 B3 在**整条控制通道读失败**（连 `AuditPage` 都拿不到）时表达同一语义 ——
    /// **不得**用它表达"无审计记录"（那是空态，见 §8.3 的两行专行）。
    pub fn set_unavailable(&self, reason: &str) {
        self.core.set_unavailable_reason(&display_safe(reason));
        self.core.available.set(false);
        self.core.has_more.set(false);
        self.core.layout();
    }

    /// **超限（EDGE-15）显式注入入口**（**AU5**：契约 `AuditPage` 无该字段）。
    ///
    /// `true` ⇒ 列表区上方出现 `WarnBanner`「检索范围超限 · 请缩小时间范围」。
    ///
    /// **懒惰构建（M5）**：`WarnBanner` 在**首次** `set_range_too_large(true)` 时才建
    /// （此前不占任何内存、不占布局位）；建失败 ⇒ 该次调用**不显**（只留 stderr 诊断）。
    #[cfg(test)]
    pub(crate) fn set_range_too_large(&self, on: bool) {
        if on {
            self.core.ensure_warn();
        }
        self.core.range_too_large.set(on);
        self.core.layout();
    }

    // ── 意图回调（屏 → 外部；**本页不发请求 / 不生成 `request_id`**）────────────

    /// 注册「筛选变化」意图回调（载荷 = [`AuditQuery`]，`page` 恒 `1`）。
    ///
    /// **去重口径**：与上一次**已发出**的查询**逐字段相同**时不重复回调 —— 用户重复点同一段
    /// （键矩阵的 `VALUE_CHANGED` 会照发）不会产生重复请求。**改什么会让本条变红**：去掉
    /// `Core::fire_query` 里的去重判断 ⇒ 离屏用例「未变化时不发意图」立刻红。
    pub fn set_on_query<F>(&self, f: F)
    where
        F: FnMut(AuditQuery) + 'static,
    {
        self.core.on_query.set(f);
    }

    /// 注册「加载更多」意图回调（载荷 = [`AuditQuery`]，`page` = 当前页 + 1）。
    pub fn set_on_load_more<F>(&self, f: F)
    where
        F: FnMut(AuditQuery) + 'static,
    {
        self.core.on_load_more.set(f);
    }

    /// **「加载更多」的触发入口**（见 **AU6**：滚动事件在本层不可得）。
    ///
    /// 外壳（B3）在检测到"列表滚到底"时调用；`has_more = false` ⇒ **不发意图**
    /// （最后一页不再空转）。本页据**最近一次注入**的 `AuditPage.page` 组装 `page + 1`。
    pub fn request_next_page(&self) {
        self.core.fire_load_more();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. 纯逻辑单测（**不触碰 LVGL** ⇒ 可独立 `#[test]`；离屏链路在 `ui/tests.rs::pages_chain`）
//
// 每条断言旁写「改什么会让本条变红」—— 本项目多次抓到"宣称会红但实测不红"的断言，
// 故本节的每条都在 B2c-1 收尾时**实测过**（见报告的探针记录）。
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::controls::DateTimeValue;

    /// 造一条审计记录（默认成功、无前后值）。
    fn entry(op: ConsoleOp, result: AuditResult, target: &str) -> ConsoleAuditEntry {
        ConsoleAuditEntry {
            id: "a1b2c3d4-0000-0000-0000-000000000001".into(),
            ts_ms: 1_789_047_727_000,
            operator: CONSOLE_OPERATOR.into(),
            op,
            target: target.into(),
            before: None,
            after: None,
            result,
            reason: None,
            request_id: "0f7a3e10-1111-4222-8333-444455556666".into(),
        }
    }

    // ── 列表三态（§8.3 EDGE-17 / EDGE-08 不得互替）────────────────────────────

    /// `available = false` ⇒ **不可用**，即使 `entries` 为空也**不得**落空态。
    ///
    /// **改什么会让本条变红**：把 `list_view` 的判断顺序倒过来（先看 `entries.is_empty()`）
    /// —— 第 2 / 3 条立刻红（审计源不可用会被显示成「无审计记录」，正是 §8.3 禁止的互替）。
    #[test]
    fn list_view_prefers_unavailable_over_empty() {
        let base = AuditPage {
            entries: Vec::new(),
            page: 1,
            page_size: AUDIT_PAGE_SIZE as u32,
            has_more: false,
            newest_ts_ms: None,
            available: false,
        };
        assert_eq!(list_view(&base), ListView::Unavailable, "EDGE-17");
        let empty_ok = AuditPage {
            available: true,
            ..base.clone()
        };
        assert_eq!(list_view(&empty_ok), ListView::Empty, "EDGE-08");
        // 两条路径的产物**必须不同**（同一条 `entries`，仅 `available` 之差）。
        assert_ne!(list_view(&base), list_view(&empty_ok));
        // 有记录 ⇒ 行态。
        let with_rows = AuditPage {
            available: true,
            entries: vec![entry(ConsoleOp::ConfigApply, AuditResult::Ok, "gateway.port")],
            newest_ts_ms: Some(1),
            ..base.clone()
        };
        assert_eq!(list_view(&with_rows), ListView::Rows);
        // 不可用时**即使有 entries 也不显行**（`entries` 不可信 —— 安全方向）。
        let unav_with_rows = AuditPage {
            available: false,
            ..with_rows
        };
        assert_eq!(list_view(&unav_with_rows), ListView::Unavailable);
    }

    /// 不可用态组件给的上屏文案与 §3.6 / §8.3 的 `审计记录不可用` **逐字相等**（同源锁）。
    #[test]
    fn unavailable_text_comes_from_the_component_kind() {
        assert_eq!(UnavailableKind::Audit.title(), TEXT_UNAVAILABLE);
        assert_ne!(
            UnavailableKind::Audit.title(),
            TEXT_EMPTY,
            "「不可用」与「无记录」的文案**不得相同**（EDGE-17 vs EDGE-08）"
        );
        assert!(!UnavailableKind::Audit.means_nothing_happened());
    }

    // ── 头部：最近审计条（F19.8）──────────────────────────────────────────────

    /// `newest_ts_ms = None` ⇒ **占位符**，不得编造时间。
    ///
    /// **改什么会让本条变红**：把 `None` 分支改成 `format_epoch_ms_utc(0)`（"1970/01/01"）。
    #[test]
    fn newest_never_fabricates_time() {
        let none = newest_text(None);
        assert!(
            none.contains(PLACEHOLDER),
            "无最近审计记录 ⇒ 占位符（实际：{none}）"
        );
        assert!(
            !none.contains("1970"),
            "**不得**把 None 折算成纪元原点时间（编造）（实际：{none}）"
        );
        assert!(
            !none.contains("20"),
            "**不得**出现任何年份数字（实际：{none}）"
        );
        let some = newest_text(Some(1_789_047_727_000));
        assert_eq!(some, format!("{TEXT_NEWEST_PREFIX}2026/09/10 13:42:07"));
        assert_ne!(some, none);
    }

    // ── 结果胶囊（文字 + 颜色两通道）──────────────────────────────────────────

    /// 两种结果的文案 / 皮肤**各自不同**，且与 §3.6 P5 行的用字一致（`✕` 缺字见 **AU2**）。
    #[test]
    fn result_chip_carries_text_and_color() {
        assert_eq!(result_text(AuditResult::Ok), "● 成功");
        assert_eq!(
            result_text(AuditResult::Failed),
            "× 失败",
            "UI 写 ✕（U+2715）但该字形不在 cmap 内 ⇒ 取 ×（AU2）"
        );
        assert_ne!(
            result_text(AuditResult::Ok),
            result_text(AuditResult::Failed)
        );
        assert_eq!(result_skin(AuditResult::Ok), ChipSkin::SUCCESS);
        assert_eq!(result_skin(AuditResult::Failed), ChipSkin::FAILURE);
        // 颜色通道（文字色）必须不同 —— 改什么会让本条变红：两个分支返回同一个皮肤。
        assert_ne!(
            result_skin(AuditResult::Ok).accent(),
            result_skin(AuditResult::Failed).accent()
        );
        assert_ne!(
            result_skin(AuditResult::Ok).bg,
            result_skin(AuditResult::Failed).bg
        );
    }

    // ── 机器键 / 自由文本（AU9：本单元最高风险项）─────────────────────────────

    /// 操作者：契约名 → 中文名；未知名 → `display_safe`（**保留可辨认性、不出豆腐块**）。
    ///
    /// **改什么会让本条变红**：把 `operator_text` 改成直接返回 `operator`（`local-console`
    /// 里的小写 ASCII 在生成字体里没有字形 ⇒ 真机豆腐块）。
    #[test]
    fn operator_text_maps_known_and_keeps_unknown_ascii_safe() {
        assert_eq!(operator_text(CONSOLE_OPERATOR), TEXT_OPERATOR_LOCAL);
        assert_ne!(
            operator_text(CONSOLE_OPERATOR),
            CONSOLE_OPERATOR,
            "机器名不得直上屏"
        );
        // 大小写变体同样命中（`LOCAL-CONSOLE` / `Local-Console`）。
        assert_eq!(operator_text("LOCAL-CONSOLE"), TEXT_OPERATOR_LOCAL);
        // 未知名：小写 → 大写同族（`debug` → `DEBUG`），不伪造中文名。
        // ⚠️ 字母表里**保留了 h/k/s 三个小写**（它们在生成字体里确有字形，见
        // `pages::ASCII_DISPLAY_ALPHABET`）⇒ `hmi` 落在屏上是 `hMI`，仍是"无豆腐块"的产物。
        assert_eq!(operator_text("debug"), "DEBUG");
        assert_eq!(operator_text("hmi"), "hMI");
        assert!(!operator_text("debug")
            .chars()
            .any(|c| c.is_ascii_lowercase()));
        // 中文名（后端直接给）原样透传（残余局限见 AU9）。
        assert_eq!(operator_text("本地控制台"), "本地控制台");
    }

    /// `target` 标签：**已知键才有中文标签**；未登记键一律 `None`（由调用方降级显示，见下一条）。
    ///
    /// **改什么会让本条变红**：给未登记键返回一个中文兜底标签（**臆造**字段名）；
    /// 或改坏 [`INTERLOCK_TARGETS`] 的键 → op 对应（联锁键会掉进"未登记"路径）。
    #[test]
    fn target_label_is_known_set_or_none() {
        assert_eq!(target_label("gateway.port"), Some(TEXT_FIELD_PORT));
        assert_eq!(target_label("system.log_level"), Some(TEXT_FIELD_LOG_LEVEL));
        assert_eq!(
            target_label("telemetry.interval"),
            Some(TEXT_FIELD_TELEMETRY)
        );
        assert_eq!(
            target_label("gateway.listen_addr"),
            Some(p2_config::TEXT_LISTEN_ADDR),
            "该键沿用 P2 的 PM 裁定标签（同一份常量，不另抄）"
        );
        // **契约点名的两个联锁键**（`display-proto/src/audit.rs` 的 `target` 注释 /
        // `ConsoleOp::InterlockRelease` / `InterlockAckM1` 文档；设计 §4.5）—— 标签**转出**
        // 契约 `label()`（单一真源，不另抄中文）。
        assert_eq!(
            target_label("interlock.release"),
            Some(ConsoleOp::InterlockRelease.label()),
            "联锁释放（契约点名键，`POST /interlock/release`）"
        );
        assert_eq!(
            target_label("interlock.ack_m1"),
            Some(ConsoleOp::InterlockAckM1.label()),
            "M1 授权（契约点名键，`POST /interlock/ack_m1`）"
        );
        // 未登记键 ⇒ `None`（不臆造中文名）。
        for unknown in ["display.publish_ms", "interlock.flood", ""] {
            assert_eq!(
                target_label(unknown),
                None,
                "未登记键 ⇒ 不显中文标签（**不臆造**字段名）"
            );
        }
        // 两张表**无重复键**（只增不改的前提），且整体键集合唯一。
        let keys: Vec<&str> = TARGET_LABELS
            .iter()
            .map(|(k, _)| *k)
            .chain(INTERLOCK_TARGETS.iter().map(|(k, _)| *k))
            .collect();
        let mut uniq = keys.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(keys.len(), uniq.len(), "映射表键必须唯一（两张表合起来）");
        assert_eq!(keys.len(), 6, "4 个配置字段键 + 2 个联锁键");
    }

    /// **未登记 `target` 键不得静默隐藏**（评审 ③）：屏上仍须有**可辨认**的标识，且
    /// **不得**出现伪造的中文标签。
    ///
    /// **改什么会让本条变红**：把 [`summary_text`] 的未登记分支改回"只留值对"
    /// （`clip(&pair, ..)`，即原实现）—— 第 1 组断言立刻红。**这正是被修的缺陷**：
    /// 操作者当时**无从知道被改的是哪一项**（`interlock.release` / `interlock.ack_m1`
    /// 这两个契约点名的键当时都不在映射表里 ⇒ 联锁类审计行渲染成裸值对）。
    #[test]
    fn unknown_target_key_is_shown_not_hidden() {
        let before = Some(Value::from(1));
        let after = Some(Value::from(2));
        let pair_len = "1 → 2".chars().count();
        // ① 未登记键：标签位仍有**可辨认的机器键**（`display_safe` 归一的**保尾**截断），
        //    **不是**空白；且值对**必须存活**（键有长度上界，不许把值对挤掉）。
        let s = summary_text(before.as_ref(), after.as_ref(), "interlock.flood");
        let want_key = unknown_key_label("interlock.flood", pair_len);
        assert_eq!(
            s,
            format!("{want_key}: 1 → 2"),
            "未登记键 ⇒ display_safe(键) 的保尾截断 + 值对（**不整块不显**）"
        );
        assert!(
            s.contains("1 → 2"),
            "值对**不得**被长的机器键挤掉（实际：{s}）"
        );
        assert!(
            s.starts_with(TEXT_ELLIPSIS),
            "键超长 ⇒ 保**尾**截断（带省略标记）（实际：{s}）"
        );
        // ② **不臆造**中文标签：产物里不得出现任何**已登记**的中文标签。
        for (_, label) in TARGET_LABELS
            .iter()
            .map(|(k, v)| (*k, *v))
            .chain(INTERLOCK_TARGETS.iter().map(|(k, op)| (*k, op.label())))
        {
            assert!(
                !s.contains(label),
                "未登记键**不得**借用已知键的中文标签 `{label}`（实际：{s}）"
            );
        }
        // ③ `display_safe` 保证不出豆腐块（字符全部在 ASCII 内）。
        for ch in display_safe("interlock.flood").chars() {
            assert!(
                ch.is_ascii(),
                "降级文本必须是 ASCII（`display_safe` 只管 ASCII；实际：{s}）"
            );
        }
        // ④ 已知键仍走中文标签（未登记路径**不夺**已知键的表现）。
        assert_eq!(
            summary_text(before.as_ref(), after.as_ref(), "interlock.release"),
            format!("{}: 1 → 2", ConsoleOp::InterlockRelease.label())
        );
        // ⑤ 空键：**无标识可显** ⇒ 只留值对（不是"隐藏信息"）。
        assert_eq!(summary_text(before.as_ref(), after.as_ref(), ""), "1 → 2");
    }

    /// **不同未登记键 ⇒ 屏上产物必须不同**（B2c-1 代码质量评审 ④ 的补网）。
    ///
    /// **改什么会让本条变红**：把 [`unknown_key_label`] 的 [`clip_tail`] 换回 [`clip`]
    /// （**保头**）—— 原实现下 `interlock.flood` 与 `interlock.force` 前 7 字逐字相同
    /// （`IN?ER?O`）⇒ 两条屏上文案**完全相同**（`IN?ER?O...: 2404 → 2405` 的截断体），
    /// 第 1 组断言立刻红。这正是被修的缺陷（操作者无法分辨被改的是哪一项）。
    #[test]
    fn unknown_target_keys_are_distinguishable() {
        let before = Some(Value::from(2404));
        let after = Some(Value::from(2405));
        let a = summary_text(before.as_ref(), after.as_ref(), "interlock.flood");
        let b = summary_text(before.as_ref(), after.as_ref(), "interlock.force");
        let c = summary_text(before.as_ref(), after.as_ref(), "display.publish_ms");
        assert_ne!(a, b, "`interlock.flood` 与 `interlock.force` 必须可区分");
        assert_ne!(a, c, "`interlock.flood` 与 `display.publish_ms` 必须可区分");
        assert_ne!(b, c);
        // 现实值对 `2404 → 2405`（11 字）**完整存活**（键已按值对实长让位到下限）。
        for s in [&a, &b, &c] {
            assert!(
                s.contains("2404 → 2405"),
                "现实值对必须完整存活（键按值对实长让位；实际：{s}）"
            );
        }
        // 产物长度恒 ≤ 摘要上限（保尾截断同样不得越界）。
        for s in [&a, &b, &c] {
            assert!(s.chars().count() <= SUMMARY_MAX_CHARS);
        }
    }

    /// 未登记键的长度预算：**值对优先**、夹在 `[MIN, MAX]` 内。
    #[test]
    fn unknown_key_budget_prefers_the_value_pair() {
        // 短值对 ⇒ 键拿满上限。
        assert_eq!(unknown_key_budget(1), UNKNOWN_TARGET_MAX_CHARS);
        // 现实值对 `2404 → 2405`（11 字）⇒ 键退到下限，值对恰好占满剩余（18 − 5 − 2 = 11）。
        assert_eq!(unknown_key_budget(11), UNKNOWN_TARGET_MIN_CHARS);
        assert_eq!(
            UNKNOWN_TARGET_MIN_CHARS + TEXT_LABEL_SEP.chars().count() + 11,
            SUMMARY_MAX_CHARS,
            "下限 + 分隔 + 现实值对 恰为摘要上限（值对完整存活）"
        );
        assert_eq!(UNKNOWN_TARGET_MIN_CHARS, TEXT_ELLIPSIS.chars().count() + 2, "`...` + 2 字尾部");
        // 极长值对：键**不再**往里挤（停在 MIN），值对自身走既有截断口径。
        assert_eq!(unknown_key_budget(10_000), UNKNOWN_TARGET_MIN_CHARS);
        // `MIN < MAX` 由**常量区**的 `const _: () = assert!(..)`（`UNKNOWN_TARGET_MIN_CHARS`
        // 上方）钉死 —— 本用例不再重复一条运行期 `assert!`（收口 ②：它会被 clippy 判
        // `assertions_on_constants`；编译期形态的**把关能力更强**且零告警）。
    }

    /// `clip` / `clip_tail` 的**边界**：产物长度**恒 ≤ 上限**（**M2**：原 `clip(t, n<3)`
    /// 会返回比上限**更长**的 `...`，与"按上限截断"的契约相悖）。
    #[test]
    fn clip_never_exceeds_its_limit() {
        let long = "一二三四五六七八九十";
        for n in 0..=(long.chars().count() + 2) {
            let h = clip(long, n);
            assert!(
                h.chars().count() <= n,
                "clip(_, {n}) 产物 {} 字 > 上限（实际：{h}）",
                h.chars().count()
            );
            let t = clip_tail(long, n);
            assert!(
                t.chars().count() <= n,
                "clip_tail(_, {n}) 产物 {} 字 > 上限（实际：{t}）",
                t.chars().count()
            );
        }
        // `n < 3`（放不下省略标记）⇒ **纯截断、无标记**，绝不返回 `...`。
        assert_eq!(clip(long, 0), "");
        assert_eq!(clip(long, 1), "一");
        assert_eq!(clip(long, 2), "一二");
        assert_eq!(clip_tail(long, 0), "");
        assert_eq!(clip_tail(long, 2), "九十", "**保尾**（与 clip 的保头相反）");
        // 不超长 ⇒ 原样（不无故截断）；保尾截断的形态自证。
        assert_eq!(clip(long, 10), long);
        assert_eq!(clip_tail(long, 10), long);
        // 4 字上限 = `...` + **1** 字（省略标记占 3）。
        assert_eq!(clip("一二三四五", 4), format!("一{TEXT_ELLIPSIS}"));
        assert_eq!(clip_tail("一二三四五", 4), format!("{TEXT_ELLIPSIS}五"));
        assert_eq!(clip_tail("一二三四五六七八", 6), format!("{TEXT_ELLIPSIS}六七八"));
        // 多字节按**字符**截断（不切半个汉字）。
        assert!(clip_tail(long, 4).is_char_boundary(clip_tail(long, 4).len()));
    }

    /// **行序 = 时间倒序**（§6.5；`row_order` 是 `apply_page` / `refresh_op_labels` 的共用真源）。
    ///
    /// **改什么会让本条变红**：把 `row_order` 改成不排序（返回 `0..n`）—— 第 1 组断言红；
    /// 或改成升序、或改成**不稳定**排序导致同 `ts_ms` 的相对次序漂移（第 3 组红）。
    #[test]
    fn row_order_is_time_desc() {
        let mk = |ts: u64| {
            let mut e = entry(ConsoleOp::ConfigApply, AuditResult::Ok, "gateway.port");
            e.ts_ms = ts;
            e
        };
        let list = vec![mk(300), mk(100), mk(200)];
        assert_eq!(row_order(&list), vec![0, 2, 1], "按 ts_ms 降序");
        // 已有序 ⇒ 恒等映射（稳定）。
        let sorted = vec![mk(300), mk(200), mk(100)];
        assert_eq!(row_order(&sorted), vec![0, 1, 2]);
        // 同 `ts_ms` 保持**注入相对次序**（稳定排序）。
        let ties = vec![mk(100), mk(200), mk(100), mk(200)];
        assert_eq!(row_order(&ties), vec![1, 3, 0, 2], "同刻者保注入序");
        // 空 / 单元素（边界，不 panic）。
        assert_eq!(row_order(&[]), Vec::<usize>::new());
        assert_eq!(row_order(&[mk(1)]), vec![0]);
    }

    /// **行池上界与实测容量**（**AU8**）。
    ///
    /// 两条**上界约束**由**编译期**断言给出（见常量区 `const _: () = assert!(..)`）：
    /// `ROW_MAX ≤ MEASURED_ROW_CAPACITY` 且 `ROW_MAX ≥ AUDIT_PAGE_SIZE`。**为什么用编译期
    /// 断言**：真去注入超界条数会触发 `lv_realloc` 失败 + `lv_array_resize` 断言 ⇒ **测试挂死**
    /// （不是"红"）—— 见 AU8 的实测记录（20 / 22 成功，24 挂死且复现两次；顺序注入 26 挂死）。
    ///
    /// **改什么会让本条变红**：把 [`MEASURED_ROW_CAPACITY`] 改成与实测不符的值
    /// （它是"测量结论"，改动必须**重跑测量**；把它改成 100 而 `ROW_MAX` 仍是 20，
    /// 本条的 `assert_eq!` 立刻红）。
    #[test]
    fn row_pool_capacity_is_measured() {
        assert_eq!(
            MEASURED_ROW_CAPACITY, 22,
            "单页独活的实测上界：22 成功 / 24 挂死（AU8；改此值前必须重跑 pages_chain 的逐档注入测量）"
        );
        assert_eq!(ROW_MAX, AUDIT_PAGE_SIZE, "上限恰一页（AU8）");
        // ── 共存预算（B2c-1 整改 ②）────────────────────────────────────────────
        assert_eq!(
            MEASURED_COEXIST_TOTAL_ROWS, 10,
            "两页共存的实测总行数上界：2 页 × 5 行成功 / 2 页 × 6 行 OOM 挂死 \
             （前提：LV_MEM_SIZE=256KB、两页同时存活；见 AU8 —— 改此值前必须重跑测量）"
        );
        assert_eq!(COEXIST_ROWS_PER_PAGE, 4, "共存时每页 4 行（对挂死点 6 留 ≥33% 余量）");
        // 下面两条**上界关系**由常量区的 `const _: () = assert!(..)` 给出（见 `COEXIST_ROWS_PER_PAGE`
        // 附近的四条编译期断言）：
        //   · `2 * COEXIST_ROWS_PER_PAGE <= MEASURED_COEXIST_TOTAL_ROWS`（预算不超实测总上界）；
        //   · `COEXIST_ROWS_PER_PAGE < ROW_MAX`（**如实钉住"两页各满行不可能"** —— 这不是
        //     "取舍"，是 256 KB 堆下的测量结论，AU8 的 ②）。
        // 本用例不再重复运行期 `assert!`（收口 ②：`assertions_on_constants` 告警；
        // 编译期形态**任何构建都查、不可能被跳过**，把关能力不减）。
    }

    /// 说明条**样式族判定**（[`banner_skin_of`]）—— 色相断言的"页面确实用了它"那一半的判据。
    #[test]
    fn banner_skin_classifies_palette_constants() {
        assert_eq!(banner_skin_of(Palette::AUDIT_BG), BannerSkin::Audit);
        assert_eq!(banner_skin_of(Palette::WARN_BG), BannerSkin::Warn);
        assert_eq!(banner_skin_of(Palette::DANGER_BG), BannerSkin::Danger);
        assert_eq!(banner_skin_of(Palette::SURFACE_ALT), BannerSkin::SurfaceAlt);
        assert_eq!(banner_skin_of(Palette::SURFACE), BannerSkin::Surface);
        assert_eq!(banner_skin_of(Palette::BG), BannerSkin::Other);
        // 判定必须**能把 WARN_BG 与 AUDIT_BG 分开**（否则"页面确实用了 audit 那一档"是空转）。
        assert_ne!(banner_skin_of(Palette::AUDIT_BG), banner_skin_of(Palette::WARN_BG));
    }

    /// 表头子件数常量 = 实际建出的子件数（5 列名 + 4 竖线 + 1 底线）—— 纯逻辑侧的定点，
    /// 运行期由 `ui/tests.rs::pages_chain` 读 LVGL 的 `child_count()` 钉死。
    #[test]
    fn head_child_count_matches_spec() {
        assert_eq!(HEAD_CHILD_COUNT, 10, "§6.5 线框的 5 列名 + 4 竖线 + 1 底线");
        assert_eq!(HEAD_COL_COUNT, 5, "§6.5 线框 5 列");
        assert_eq!(HEAD_DIV_COUNT, HEAD_COL_COUNT - 1, "竖线 = 列间分隔");
        // **M1 的机制自证**：子件数**由表长推导**（不是第二处字面量）—— 三者的关系恒成立，
        // 增删列时只需动 `HEAD_COLS` / `HEAD_COL_X`（两者长度由编译器钉死）。
        assert_eq!(HEAD_CHILD_COUNT, HEAD_COLS.len() + HEAD_DIV_COUNT + 1);
        assert_eq!(HEAD_COL_X.len(), HEAD_COLS.len(), "逐列 x 与列名一一对应");
    }

    /// `None`（前后值缺失）**绝不**变成 `0` / 空串 —— 占位符 `–`。
    ///
    /// **改什么会让本条变红**：把 `side_text` 的 `None` 分支写成 `"0".to_string()` 或
    /// `String::new()`（PRD「显 `--`，严禁补 0」；本项目的 C1 前车之鉴）。
    #[test]
    fn none_is_placeholder_never_zero() {
        assert_eq!(side_text(None), PLACEHOLDER);
        assert_ne!(side_text(None), "0");
        assert_ne!(side_text(None), "");
        // 值对：两侧都缺 ⇒ `– → –`（显式，不是空白）。
        let s = summary_text(None, None, "gateway.port");
        assert!(s.contains(PLACEHOLDER));
        assert!(!s.trim().is_empty(), "**不得**静默成空串");
        assert!(!s.contains('0'), "不得补 0（实际：{s}）");
        // JSON `null` 与 Rust `None` 同口径（都是"有字段、无值"）。
        assert_eq!(side_text(Some(&Value::Null)), PLACEHOLDER);
    }

    /// 每种 JSON 形态都有**确定且可读**的产物（不得 panic、不得空白）。
    #[test]
    fn value_text_covers_every_json_shape() {
        use serde_json::json;
        assert_eq!(value_text(&json!("info")), "INFO", "小写机器值 → 大写同族");
        assert_eq!(value_text(&json!("debug")), "DEBUG");
        assert_eq!(value_text(&json!("127.0.0.1")), "127.0.0.1");
        assert_eq!(value_text(&json!(2404)), "2404");
        assert_eq!(value_text(&json!(1.5)), "1.5");
        assert_eq!(
            value_text(&json!(-3)),
            "\u{2212}3",
            "负号取 U+2212（ASCII `-` 无字形）"
        );
        assert_eq!(value_text(&json!(true)), TEXT_VALUE_ON);
        assert_eq!(value_text(&json!(false)), TEXT_VALUE_OFF);
        assert_eq!(value_text(&json!([1, 2, 3])), format!("3 {TEXT_UNIT_ITEMS}"));
        assert_eq!(value_text(&json!({"a": 1})), format!("1 {TEXT_UNIT_FIELDS}"));
        assert_eq!(value_text(&Value::Null), PLACEHOLDER);
        // 复合值**只报规模**（不把 `[1,2]` 渲染成一串 `?`）。
        let arr = value_text(&json!([1, 2]));
        assert!(!arr.contains('?'));
        assert!(!arr.contains('['));
    }

    /// 超长值按**字符数**截断并带省略标记（不 panic、不丢前缀）。
    #[test]
    fn summary_truncates_long_values() {
        let long = "x".repeat(200);
        let s = summary_text(Some(&Value::from(long)), None, "system.log_level");
        assert!(
            s.chars().count() <= SUMMARY_MAX_CHARS,
            "截断后不得超长（实际 {} 字：{s}）",
            s.chars().count()
        );
        assert!(s.ends_with(TEXT_ELLIPSIS), "必须带省略标记（实际：{s}）");
        assert!(s.starts_with(TEXT_FIELD_LOG_LEVEL), "保留标签前缀");
        // 未超长 ⇒ 原样（不无故截断）。
        let short = summary_text(
            Some(&Value::from(2404)),
            Some(&Value::from(2405)),
            "gateway.port",
        );
        assert_eq!(short, "端口: 2404 → 2405", "与 §6.5 的行内容示例同形");
        assert!(!short.contains(TEXT_ELLIPSIS));
        // 多字节（中文）按**字符**而非字节截断：不得把某个汉字切一半。
        let zh = summary_text(Some(&Value::from("一二三四五六七八九十".repeat(3))), None, "");
        assert!(zh.chars().count() <= SUMMARY_MAX_CHARS);
        assert!(zh.is_char_boundary(zh.len()));
    }

    /// 原因：**仅失败行**有文案；成功行 / 空原因 ⇒ `None`（整列不显）。
    ///
    /// **改什么会让本条变红**：去掉 `reason_text` 的 `result != Failed ⇒ None` 前置判断
    /// （成功行会显示出一条矛盾的原因）。
    #[test]
    fn reason_only_on_failed_rows() {
        // 自由文本经 `free_text_safe`：全角冒号 → `:`（AU15 折叠）、小写 → 大写同族。
        // ⚠️ `estop` 里的 `t` **上屏是 `?`**：大写 `T` 不在 cmap 内、小写 `t` 也没有字形
        //（ASCII 可达集实测 = `ABCDEFGIMNOPRSUW` + `hks` + `0123456789` + ` !%+./:?`）。
        // 这是**如实**的产物（不是伪造一个更漂亮的标签），残余见 AU9 / AU15。
        assert_eq!(
            reason_text(AuditResult::Failed, Some("触发源未复位：estop")),
            // ⚠️ 折叠后是 `：` → `:` **紧贴**前字（折叠不插空格；`原因: ` 前缀自带一个空格）。
            Some(format!("{TEXT_REASON_PREFIX}触发源未复位:Es?OP"))
        );
        // 折叠本身可测：全角 → `·` / `/` / `:`（六个源字符逐一覆盖）。
        assert_eq!(
            free_text_safe("a，b；c（d）e：f、g"),
            "A\u{00B7}B\u{00B7}C\u{00B7}D\u{00B7}E:F/G"
        );
        assert_eq!(reason_text(AuditResult::Failed, None), None);
        assert_eq!(
            reason_text(AuditResult::Failed, Some("   ")),
            None,
            "空白原因不显"
        );
        assert_eq!(
            reason_text(AuditResult::Ok, Some("不该出现的原因")),
            None,
            "成功行**结构性**取不到原因（即使契约给了矛盾的 reason）"
        );

        // 行级：成功行即使带 reason，`Row` 也不该显 —— 该行为由上面的纯函数保证，
        // 离屏链路另有 `row_reason_visible` 断言（见 `ui/tests.rs::pages_chain`）。
        let mut ok = entry(ConsoleOp::ConfigApply, AuditResult::Ok, "gateway.port");
        ok.reason = Some("矛盾".into());
        assert_eq!(reason_text(ok.result, ok.reason.as_deref()), None);
    }

    /// **机器键 / 请求号从不上屏**（`id` / `request_id` 只作现场对拍）。
    ///
    /// ⚠️ **口径订正（评审 ③ 整改）**：本用例此前断言"未登记的机器键不上屏" —— 那是**旧**
    /// 处置（未登记键整块不显标签）。现改为"未登记键**降级显示** `display_safe(键)`"
    /// （**可辨认** > 静默隐藏，见 **AU9** / `unknown_target_key_is_shown_not_hidden`）⇒
    /// 本条只保证：**契约字段 `id` / `request_id` 的任何形态都不上屏**，且**已登记**键的
    /// **原始机器键字面量**不上屏（上屏的是中文标签）。
    #[test]
    fn machine_only_fields_never_reach_screen() {
        let e = entry(
            ConsoleOp::InterlockRelease,
            AuditResult::Failed,
            "interlock.release",
        );
        let rendered = format!(
            "{}{}{}{}",
            format_epoch_ms_utc(e.ts_ms),
            operator_text(&e.operator),
            summary_text(e.before.as_ref(), e.after.as_ref(), &e.target),
            reason_text(e.result, e.reason.as_deref()).unwrap_or_default()
        );
        assert!(!rendered.contains(&e.id), "记录 id 不上屏");
        assert!(!rendered.contains(&e.request_id), "request_id 不上屏");
        assert!(
            !rendered.contains(&e.target),
            "**已登记**键上屏的是中文标签、不是原始机器键（实际：{rendered}）"
        );
        assert!(
            rendered.contains(ConsoleOp::InterlockRelease.label()),
            "已登记键必须带上屏（实际：{rendered}）"
        );
        // 未登记键：上屏的是 `display_safe` **归一 + 保尾截断**的形态（不是原字面量、不是空白）。
        let raw = "interlock.flood";
        // 本行的值对（两侧都缺）—— `summary_text` 内部按它的实长给键分配预算。
        let pair = format!("{PLACEHOLDER}{TEXT_PAIR_ARROW}{PLACEHOLDER}");
        let shown = summary_text(e.before.as_ref(), e.after.as_ref(), raw);
        assert!(!shown.contains(raw), "原始小写机器键不上屏（实际：{shown}）");
        assert!(
            shown.starts_with(&unknown_key_label(raw, pair.chars().count())),
            "未登记键上屏的是 display_safe 归一的**保尾**形态（实际：{shown}）"
        );
    }

    // ── 底部状态行 / 筛选选项 ─────────────────────────────────────────────────

    /// 底部状态行：`has_more` ⇒ `加载中`；否则 `已加载全部`；无行 ⇒ 不显。
    #[test]
    fn footer_reflects_has_more() {
        assert_eq!(footer_text(true, 3), Some(TEXT_FOOTER_LOADING));
        assert_eq!(footer_text(false, 3), Some(TEXT_FOOTER_ALL));
        assert_eq!(footer_text(true, 0), None, "无行时不显状态行");
        assert_eq!(footer_text(false, 0), None);
        assert_ne!(TEXT_FOOTER_LOADING, TEXT_FOOTER_ALL);
    }

    /// chip 选项：**首位恒为「全部」**（快捷复位），其余取注入标签。
    #[test]
    fn op_options_prepends_all_and_canonical_matches_contract() {
        let canon = canonical_ops();
        assert_eq!(canon.len(), ConsoleOp::ALL.len());
        for (i, op) in ConsoleOp::ALL.iter().enumerate() {
            assert_eq!(canon[i].op, *op);
            assert_eq!(canon[i].label, op.label(), "缺省 label = 契约 label()");
        }
        let opts = op_options(&canon);
        assert_eq!(opts.len(), ConsoleOp::ALL.len() + 1);
        assert_eq!(opts[0], TEXT_OPS_ALL, "「全部」固定首位");
        assert_eq!(opts[1], ConsoleOp::ConfigApply.label());
        // 注入标签也过 display_safe（服务端给的小写 ASCII 不会变豆腐块）。
        let injected = vec![OpOption {
            op: ConsoleOp::ConfigApply,
            label: "debug".into(),
        }];
        assert_eq!(op_options(&injected)[1], "DEBUG");
    }

    /// 操作类型标签：**注入优先、契约兜底**。
    #[test]
    fn op_label_prefers_injected_then_contract() {
        let injected = vec![OpOption {
            op: ConsoleOp::InterlockAckM1,
            label: "M1 授权重启".into(),
        }];
        assert_eq!(
            op_label_of(ConsoleOp::InterlockAckM1, &injected),
            "M1 授权重启"
        );
        assert_eq!(
            op_label_of(ConsoleOp::InterlockAckM1, &[]),
            ConsoleOp::InterlockAckM1.label(),
            "未注入 ⇒ 回退契约 label()"
        );
    }

    /// 「全部」语义归一化：四条规则逐条可测。
    ///
    /// **改什么会让本条变红**：把 `normalize_ops_selection` 改成恒返回 `selected`
    /// （「全部」就不再是快捷复位，且会留下"全部 + 具体项"的非法组合）。
    #[test]
    fn normalize_selection_rules() {
        // 规则 1：新增「全部」⇒ 只留「全部」。
        assert_eq!(normalize_ops_selection(&[2], &[2, 0]), vec![0]);
        assert_eq!(normalize_ops_selection(&[], &[0]), vec![0]);
        // 规则 2：新增具体项 ⇒ 让出「全部」。
        assert_eq!(normalize_ops_selection(&[0], &[0, 2]), vec![2]);
        assert_eq!(normalize_ops_selection(&[], &[1, 3]), vec![1, 3]);
        // 规则 3：只取消 ⇒ 原样。
        assert_eq!(normalize_ops_selection(&[1, 2], &[1]), vec![1]);
        // 保守清理：非法组合（全部 + 具体）在"只取消"路径也不得留存。
        assert_eq!(normalize_ops_selection(&[0, 2], &[0, 2]), vec![2]);
        // 空缺省 = 仅「全部」（= 不筛）。
        assert_eq!(normalize_ops_selection(&[0], &[]), Vec::<usize>::new());
    }

    /// 勾选下标 → 操作类型集合（下标 0 = 全部 ⇒ 不产任何 `ConsoleOp`）。
    #[test]
    fn selected_ops_maps_and_drops_out_of_range() {
        let opts = canonical_ops();
        assert_eq!(
            selected_ops(&[0], &opts),
            Vec::<ConsoleOp>::new(),
            "全部 = 不筛"
        );
        assert_eq!(
            selected_ops(&[1, 3], &opts),
            vec![ConsoleOp::ConfigApply, ConsoleOp::InterlockRelease]
        );
        // 越界 / 已失效的下标被丢弃（不 panic）。
        assert_eq!(selected_ops(&[99], &opts), Vec::<ConsoleOp>::new());
        assert_eq!(selected_ops(&[0, 99], &opts), Vec::<ConsoleOp>::new());
        // 空选项表 ⇒ 任何下标都映射不出（不 panic）。
        assert_eq!(selected_ops(&[1], &[]), Vec::<ConsoleOp>::new());
    }

    // ── 查询意图（筛选态 → 查询语义）─────────────────────────────────────────

    /// `H1` / `H24` ⇒ 不带起止（相对窗口由服务端算）；`Custom` ⇒ 两侧都带（UTC 毫秒）。
    ///
    /// **改什么会让本条变红**：让 `H1` 也带上 `from_ms`（UI 层就要自己算"现在"⇒ 读时钟，
    /// 破坏"页面不读时钟"的不变量）。
    #[test]
    fn audit_query_shapes() {
        let d = TimeRangeChange::default();
        let q = audit_query(&d, &[], 1);
        assert_eq!(q.range, LogRange::H1);
        assert_eq!(q.from_ms, None);
        assert_eq!(q.to_ms, None);
        assert_eq!(q.page, 1);
        assert!(q.ops.is_empty(), "缺省（仅「全部」）⇒ 不筛");

        let h24 = TimeRangeChange {
            range: LogRange::H24,
            ..TimeRangeChange::default()
        };
        let q = audit_query(&h24, &[ConsoleOp::ConfigApply], 2);
        assert_eq!(q.from_ms, None);
        assert_eq!(q.to_ms, None);
        assert_eq!(q.ops, vec![ConsoleOp::ConfigApply]);
        assert_eq!(q.page, 2);

        let custom = TimeRangeChange {
            range: LogRange::Custom,
            start: filters::CUSTOM_FROM_MIN,
            end: filters::CUSTOM_TO_MAX,
        };
        let q = audit_query(&custom, &[], 1);
        assert_eq!(q.range, LogRange::Custom);
        assert_eq!(q.from_ms, Some(0), "1970/01/01 00:00 ⇒ 0");
        assert!(q.to_ms.is_some() && q.to_ms.unwrap() > q.from_ms.unwrap());

        // 具体日期（§3.6 / §6.5 示例时刻）→ 与 `format_epoch_ms_utc` 互证。
        let one_hour = TimeRangeChange {
            range: LogRange::Custom,
            start: DateTimeValue::from_parts(2026, 9, 10, 12, 42),
            end: DateTimeValue::from_parts(2026, 9, 10, 13, 42),
        };
        let q = audit_query(&one_hour, &[], 1);
        assert_eq!(
            format_epoch_ms_utc(q.from_ms.unwrap()),
            "2026/09/10 12:42:00"
        );
        assert_eq!(format_epoch_ms_utc(q.to_ms.unwrap()), "2026/09/10 13:42:00");
        assert_eq!(
            q.to_ms.unwrap() - q.from_ms.unwrap(),
            3_600_000,
            "恰 1 小时"
        );
    }

    /// 查询的 `Default` 与 `TimeRangeChange::default()` 同口径（避免两处缺省不一致）。
    #[test]
    fn query_default_matches_change_default() {
        let q = AuditQuery::default();
        let c = TimeRangeChange::default();
        assert_eq!(q.range, c.range);
        assert_eq!(q.page, 1);
        assert_eq!(audit_query(&c, &[], 1), q);
    }

    // ── 只读约束 / 色相 / 栅格（结构性断言）───────────────────────────────────

    /// **只读页**：写入口清册恒空，且全部固定文案里不含任何"写操作"字样。
    ///
    /// **改什么会让本条变红**：往 `WRITE_ENTRIES` 加一项，或往文案里加动作型写操作词
    /// （如「清空记录」）—— 后者本条查关键词，另有 `ui/tests.rs::p5_static_constraints`
    /// 查**代码层**（构造按钮 / 调删除 API / 出现导出标识符）。
    #[test]
    fn write_entries_is_empty() {
        assert_eq!(P5AuditPage::WRITE_ENTRIES.len(), 0);
        let mut texts = ALL_TEXTS.to_vec();
        texts.extend_from_slice(filters::ALL_TEXTS);
        for t in texts {
            for forbidden in ["删除", "清空", "编辑"] {
                // 「不可修改或删除」是**否定句**（说明条），故只拦"动作型"用法。
                if t.contains(forbidden) && !t.contains("不可") {
                    panic!("固定文案 `{t}` 含写操作字样 `{forbidden}` —— 只读页不得有该提示");
                }
            }
        }
    }

    /// 不可篡改说明条的**色值契约**（§6.5：底 `#14231F`；与页面上其它提示条**均不同**）。
    ///
    /// ⚠️ **本条只钉"规格色值"这一半**（评审 ② 的整改：原断言**只比 `Palette` 常量、不读
    /// 对象底色** ⇒ 把 `audit_banner_style()` 改成 `WARN_BG` **照样全绿**，注释自称会红是
    /// **空转**）。"页面**确实用了**这一档"由**两处**承接：
    /// ① 纯逻辑侧 [`banner_skin_classifies_palette_constants`]（判据 [`banner_skin_of`]）；
    /// ② 运行期侧 `ui/tests.rs::pages_chain` 的 `immutable_skin() == BannerSkin::Audit`
    ///    （读页面记录的**应用标记** —— 薄层没有"已挂样式读回"通道，故用标记而非读对象底色）。
    ///
    /// **改什么会让本条变红**：改 `Palette::AUDIT_BG` 的分量值（下面前两条）。
    #[test]
    fn audit_banner_hue_is_exclusive() {
        // 本页说明条用的两个命名常量（AU13：theme 无现成样式 ⇒ 本页组合，仍**零裸色值**）。
        let (bg, accent) = (Palette::AUDIT_BG, Palette::SOC_OK);
        // **规格值锚定**：§6.5 明写底 `#14231F`（不写 `Color::hex(..)` —— 本文件禁裸色值
        // 构造，故逐分量钉住）。
        assert_eq!(
            (bg.r, bg.g, bg.b),
            (0x14, 0x23, 0x1F),
            "说明条底必须恰为 §6.5 的 #14231F"
        );
        assert_eq!(
            (accent.r, accent.g, accent.b),
            (0x35, 0xD0, 0xC4),
            "左缘色条必须恰为 §6.5 的 #35D0C4"
        );
        assert_ne!(bg, Palette::WARN_BG, "不得与警示条同底");
        assert_ne!(bg, Palette::DANGER_BG, "不得与危险条同底");
        assert_ne!(bg, Palette::SURFACE_ALT, "不得与卡内嵌区同底");
        assert_ne!(bg, Palette::SURFACE, "不得与卡片同底");
        assert_ne!(accent, Palette::STALE, "左缘色不得与警示色相同");
        assert_ne!(accent, Palette::DANGER, "左缘色不得与危险色相同");
        // 行左缘竖条用的也是 `SOC_OK`（§6.5：「左缘 3 px `#35D0C4` 竖条（每条都有）」）。
        assert_eq!(accent, Palette::SOC_OK);
    }

    /// 栅格常量与 UI §6.5 的线框一致（含"操作类型 chip 组装得下"这条 **AU7** 的前提）。
    #[test]
    fn geometry_matches_wireframe() {
        assert_eq!(Dimens::ROW_AUDIT_H, 60, "行高 60 px");
        assert_eq!(ROW_BAR_W, 3, "左缘 3 px 竖条");
        assert_eq!(IMMUTABLE_BAR_W, 4, "说明条左缘 4 px 色条");
        // 行内两行 / 三列的几何关系由**编译期**断言钉住（见本文件常量区 `const _: () = assert!`）；
        // 此处只钉"三列之和 = 行内容宽"的**数值**口径。
        assert_ne!(ROW_SUMMARY_W, 0, "值对列必须有宽（AU7 的列宽算式）");
        assert_eq!(
            ROW_X0 + ROW_OPTYPE_W + ROW_SUMMARY_W + ROW_REASON_W + Dimens::GAP_MIN,
            ROW_RIGHT,
            "行 2 三列 + 列间缝 = 行内容右界（列宽算式自洽）"
        );
        // chip 网格必须装进内容区（**AU7** 的算术前提：4 列 × 192 + 3 × 16 = 816）。
        let grid_w = OPS_COLS as i32 * OPS_CHIP_W + (OPS_COLS as i32 - 1) * Dimens::GAP_MIN;
        assert!(
            filters::CTRL_X + grid_w <= Dimens::CONTENT_W,
            "chip 网格不得越出内容区（{}+{} > {}）",
            filters::CTRL_X,
            grid_w,
            Dimens::CONTENT_W
        );
        // 单 chip 的 AU7 算式（192 ≥ 32 + 153.6）由**编译期**断言钉住（见常量区）。
        assert_eq!(ROW_MAX, AUDIT_PAGE_SIZE, "行池上限 = 恰一页（AU8）");
    }
}
