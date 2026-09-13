//! # `ui/pages/filters.rs` —— **共享**「时间范围筛选」件（P3 日志页 / P5 审计页共用）
//!
//! 出处：UI 设计文档 §6.3 ③（`时间范围 [最近 1 小时][最近 24 小时][自定义]` +
//! 「仅『自定义』时展开 `DateTimeStepper`」，`LG-04`）、§6.5 筛选区（`时间范围` 段，
//! 「同 P3：`SegmentedControl` 3 段」）、§5.1 #4/#8 与 §5.3（`SegmentedControl` /
//! `DateTimeStepper` 的规格）、§3.6 P3/P5 的「筛选」行（上屏文案真源）。
//!
//! ## 职责边界（**本文件只做一件事**）
//!
//! 「时间范围」这一个筛选维度：**档位三选一（常驻）+ 自定义起止两处步进（仅 `Custom` 时展开）**。
//! 它**不**发请求、**不**读时钟、**不**知道日志 / 审计的存在 —— 变化经
//! [`TimeRangeFilter::set_on_change`] 交给外部（B3 的 `console.rs`），由外部组装查询并发送。
//!
//! ## 给 P3（B2c-2）的复用接口（**照此接线即可**）
//!
//! ⚠️ **本块是 `no_run` 而非 `ignore`**（B2c-1 规格评审 ⑥ 的裁定）：`ignore` **不等于**撒谎，
//! 但读者会以为"可编译、只是被跳过" —— 实际它连编译都不做 ⇒ 与 `pub` API 漂移**无人发现**。
//! 改成 `no_run`（**编译但不在 doc test 中执行**，因为真跑要 LVGL 初始化）后，本块由编译器
//! 逐条验证签名与调用式；原先引用的两个**不存在**的标识符（调用方父对象 `scroll` 与
//! `P3_RANGE_ROW_Y`）改由 `#` 隐藏的**最小脚手架**给出（示意值，非规格值）。
//!
//! ```no_run
//! # use mupc_local_display::lvgl::obj::Obj;
//! # use mupc_local_display::ui::pages::filters;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let scroll = Obj::screen()?;      // P3 的页根滚动容器（示意：真实调用方由 P3 传入）
//! # const P3_RANGE_ROW_Y: i32 = 300;  // P3 自己的栅格 y（示意值，非规格值）
//! // ① 构造（P3 的筛选区 y 由 P3 自己排在「级别 / 模块」两行之后）。唯一构造入口是 build()：
//! let filter = filters::build(&scroll, filters::TimeRangeChange::default())?;
//! filter.obj().set_pos(0, P3_RANGE_ROW_Y);
//! // ② 注册变化回调：**只报意图**，不发请求（request_id 由 B3 生成）
//! filter.set_on_change(move |c| { let _ = (c.range, c.start, c.end); });
//! // ③ 档位变化会**当场**改本件体高（自定义展开 48 → 284）⇒ 回调里必须重摆"本件之后"的
//! //    区块：`y = P3_RANGE_ROW_Y + filters::body_h(c.range)`（**用纯函数，不要读 size()** ——
//! //    事件回调内读到的 coords 还是旧的，理由见 `body_h` 文档）。
//! let _y_after = P3_RANGE_ROW_Y + filters::body_h(filter.change().range);
//! // ④ 空态 / 超限（EDGE-08 / EDGE-15）/「回到最新」等**属 P3 页面**，不在本件内。
//! # Ok(())
//! # }
//! ```
//!
//! **P3 与 P5 的差异只有「时间范围的语义落点」**：本件产出的 [`LogRange`] 三档是**与契约
//! `display-proto/src/log.rs` 共用**的类型（见 **FR3**）；P5 侧把它折成审计查询的
//! `from_ms` / `to_ms`（见 `p5_audit::AuditQuery`）。
//!
//! ## ⚠️ 已知偏差登记（**独立编号 `FR`** —— 不与 `D*`（页面层）/ `CD*`（`ui/controls.rs`）/
//! `PD*`（`p2_config.rs`）/ `IL*`（`p4_interlock.rs`）冲突）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | FR1 | **自定义起止的初值取「可表示区间的两端」**（起始 `1970/01/01 00:00`、结束 `2100/12/31 23:59`），**不是**「当前时刻 − 1 小时 / 当前时刻」 | **页面层不读时钟**（`ui/pages/mod.rs` 契约 2 / 设计 §11.1 离屏确定性）：没有 `SystemTime::now()` 就没有"现在"。用户在**选了「自定义」但未动步进器**时必须有**确定**的语义 ⇒ 取"全区间"（等价于不按时间过滤），且它是**屏上可见**的（步进器显示该值），不是静默的。真正的"默认近 1 小时"应由 B3（持有时钟）经 [`TimeRangeFilter::set_change`] 在进入页面时注入 | B3 接线时调 [`TimeRangeFilter::set_change`] 注入"现在 − 1h / 现在"；届时本行改为"默认由 B3 注入" |
//! | FR2 | `DateTimeStepper` 的**跨分量组合合法性不由本件校验**（`2 月 31 日` 一类被接受，折成 `3 月 3 日`） | `ui/controls.rs` 的类型文档**明写**「组合合法性（上层负责）」；上层要"拒绝并给出原因"必须有一句**在 cmap 内的**错误文案，而 §3.6 的 P3/P5 用字表**没有**这一句 ⇒ 本件**不造**文案（与 P2 `PD*` / P4 `IL1` 的"缺字不硬造"同口径）。转换是**确定且可测**的（[`datetime_to_epoch_ms`] 单测逐例钉死），用户改一次步进器即可修正 | 若 PM 要求拦截：需先在 §3.6 补一句错误文案（如「起始时间无效」），再在 [`TimeRangeChange`] 的出口加校验 |
//! | FR3 | 三档**复用** `display-proto::log::LogRange`（`1h` / `24h` / `custom`），**不另造**本页自有类型 | 语义**逐条相同**（「最近 1 小时 / 最近 24 小时 / 自定义起止」三档，见 UI §6.3 ③ 与 §6.5 筛选区），且 P3 / P5 都用这**同一组**档位名；另造一个枚举会得到"同一屏上两个三档类型、序列化口径可能漂移"的**第二份真源**。`LogRange` 本身是**纯选项类型**（不含日志条目字段），跨页复用不引入耦合 | 无（**有意**）；若 PM 裁定审计不得引用 log 契约的类型，则在 `display-proto` 增一个中性的 `TimeRange` 并由两处共用（**不在 UI 层各造一份**） |

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mupc_display_proto::LogRange;

use crate::lvgl::obj::Obj;
use crate::lvgl::widgets::{Label, LongMode};
use crate::lvgl::LvglError;
use crate::ui::controls::{DateTimeStepper, DateTimeValue, SegmentedControl, DATETIME_TOTAL_H};
use crate::ui::pages::{layout_box, set_visible, text_label};
use crate::ui::theme::{self, Dimens, Palette, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 上屏文案（UI §3.6 P3/P5「筛选」行；**落笔前逐字在 `fonts/lv_font_cmap.txt` 核对**）
// ═══════════════════════════════════════════════════════════════════════════

/// 维度名（P3 §6.3 ③ / P5 §6.5 筛选区）。
pub const TEXT_RANGE_LABEL: &str = "时间范围";
/// 档位一（UI §6.3 ③ / §6.5）。
pub const TEXT_RANGE_H1: &str = "最近 1 小时";
/// 档位二。
pub const TEXT_RANGE_H24: &str = "最近 24 小时";
/// 档位三（选中后展开两个 `DateTimeStepper`）。
pub const TEXT_RANGE_CUSTOM: &str = "自定义";
/// 自定义起（UI §3.6 P3「筛选」行）。
pub const TEXT_START: &str = "起始时间";
/// 自定义止（同上）。
pub const TEXT_END: &str = "结束时间";

/// 本件的**全部固定文案**（供 `ui/pages/mod.rs` 的 `ALL_TEXTS` 清册）。
pub const ALL_TEXTS: &[&str] = &[
    TEXT_RANGE_LABEL,
    TEXT_RANGE_H1,
    TEXT_RANGE_H24,
    TEXT_RANGE_CUSTOM,
    TEXT_START,
    TEXT_END,
];

// ═══════════════════════════════════════════════════════════════════════════
// 2. 纯逻辑：档位 ↔ 段下标 ↔ 文案 / 主体高 / 时间换算（**不触碰 LVGL，可独立单测**）
// ═══════════════════════════════════════════════════════════════════════════

/// 三档的**槽位序**（下标 = `SegmentedControl` 的段序；唯一真源）。
pub const RANGE_ORDER: [LogRange; 3] = [LogRange::H1, LogRange::H24, LogRange::Custom];

/// 档位 → 段下标。
pub const fn range_index(r: LogRange) -> usize {
    match r {
        LogRange::H1 => 0,
        LogRange::H24 => 1,
        LogRange::Custom => 2,
    }
}

/// 段下标 → 档位（越界夹取到第 0 档；与 `SegmentedControl` 的"越界即夹取"同口径，**不 panic**）。
pub const fn range_at(index: usize) -> LogRange {
    if index >= RANGE_ORDER.len() {
        return RANGE_ORDER[0];
    }
    RANGE_ORDER[index]
}

/// 档位 → 段文案（UI §3.6 P3/P5「筛选」行逐字）。
pub const fn range_text(r: LogRange) -> &'static str {
    match r {
        LogRange::H1 => TEXT_RANGE_H1,
        LogRange::H24 => TEXT_RANGE_H24,
        LogRange::Custom => TEXT_RANGE_CUSTOM,
    }
}

// ── 栅格（UI §6.3 ③ / §6.5 筛选区；**全部由 theme 常量推导**）──────────────────

/// 标签列宽 = 4 个 `TextSlot::Label` 字（`时间范围` / `起始时间` / `结束时间` 都是 4 字；
/// 26 px 档实测 104 px ⇒ 恰好放下，`ui/tests.rs` 有实测断言）。
///
/// `pub`：**同页其它筛选维度（如 P5 的「操作类型」行）必须与它同列**（同一页里两个维度名
/// 左对齐不同会立刻看出来）；也是 P3 复用时的对齐基准。
pub const LABEL_W: i32 = TextSlot::Label.px() as i32 * 4;
/// 首行（标签 + 分段控件）高 = `SegmentedControl` 的规格高（UI §5.1 #4「高 48」）。
const SEG_ROW_H: i32 = Dimens::CHIP_H;
/// 控件列 x（标签列 + 同组缝）。`pub` 的理由同 [`LABEL_W`]。
pub const CTRL_X: i32 = LABEL_W + Dimens::GAP_MIN;
/// 分段控件宽（到内容区右缘；3 段均分 ⇒ 每段 ≈290 px ≥ 最小 96，UI §5.1 #4）。
const SEG_W: i32 = Dimens::CONTENT_W - CTRL_X;
/// 两个块的紧缝（`theme` 无 8 px 档 ⇒ 取 `GAP_MIN / 2`，与 P1 `SOC_BAR_H` 同款口径）。
const TIGHT_GAP: i32 = Dimens::GAP_MIN / 2;
/// 自定义块（两行 `DateTimeStepper`）上缘 y。
const CUSTOM_TOP: i32 = SEG_ROW_H + TIGHT_GAP;
/// 自定义块高 = 起始行 + 同组缝 + 结束行。
const CUSTOM_H: i32 = 2 * DATETIME_TOTAL_H + Dimens::GAP_MIN;

/// 档位 → **[`TimeRangeFilter`] 根对象的体高**（**布局唯一真源**）。
///
/// ⚠️ **调用方必须用它来摆"本件之后"的区块**：`Custom` 展开时体高 48 → 284
/// （UI §6.3 线框「仅『自定义』时展开」，`LG-04`）。**为什么是纯函数而不是读组件实测高**：
/// `Obj::size()` 读的是 LVGL 的 `coords`，要等一次**布局趟**才算出，而页面在**事件回调内**
/// 改档位后**必须立即**重摆 —— 此时读 `size()` 只会拿到旧值（B2b-3 实测过的同类陷阱）。
pub const fn body_h(r: LogRange) -> i32 {
    match r {
        LogRange::H1 | LogRange::H24 => SEG_ROW_H,
        LogRange::Custom => CUSTOM_TOP + CUSTOM_H,
    }
}

/// 编译期自证：三档体高与 §6.3 线框一致（`48` / `284`）。
const _: () = assert!(body_h(LogRange::H1) == 48);
const _: () = assert!(body_h(LogRange::Custom) == 284);

// ── 时间换算（UTC；**页面层不读时钟** ⇒ 只有"分量 ↔ epoch ms"这一条纯函数）──────

/// `(年, 月, 日)` → 自 1970-01-01 起的天数（Howard Hinnant 的 `days_from_civil`）。
///
/// 与 `ui/pages/mod.rs::civil_from_days` 同源算法、同一年代基准（该函数**私有**且方向相反），
/// 故此处独立成式，并由单测与 [`crate::ui::pages::format_epoch_ms_utc`] 的已知向量**互证**。
const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// `DateTimeValue`（**UTC**）→ Unix 毫秒。
///
/// **组合合法性由上层负责**（`ui/controls.rs::DateTimeValue` 的类型文档明写）：`2 月 31 日`
/// 这类非法组合**不报错、不夹取**，按本函数的日历算式落到下月（`2/31 → 3/3`）——
/// 该行为**确定且逐例可测**（见本文件单测与 **FR2**）。
///
/// 年下界 1970（`controls::YEAR_MIN`）⇒ 结果恒 ≥ 0，无需负值分支。
pub fn datetime_to_epoch_ms(v: DateTimeValue) -> u64 {
    let days = days_from_civil(i64::from(v.year), i64::from(v.month), i64::from(v.day));
    let secs = days * 86_400 + i64::from(v.hour) * 3600 + i64::from(v.minute) * 60;
    if secs <= 0 {
        return 0;
    }
    (secs as u64) * 1000
}

/// 自定义区间的**可表示下界**（`1970/01/01 00:00`）—— 见 **FR1**。
pub const CUSTOM_FROM_MIN: DateTimeValue = DateTimeValue::from_parts(1970, 1, 1, 0, 0);
/// 自定义区间的**可表示上界**（`2100/12/31 23:59`）—— 见 **FR1**。
pub const CUSTOM_TO_MAX: DateTimeValue = DateTimeValue::from_parts(2100, 12, 31, 23, 59);

/// 一次「时间范围」变化的**载荷**（经 [`TimeRangeFilter::set_on_change`] 交回外部）。
///
/// **不做 `Option` 化**：三档恒有起止值（`H1`/`H24` 时它们是**当次屏上值**，只是不参与查询
/// ——"相对窗口"由服务端按档位算，口径见 `p5_audit::audit_query`）。外部因此总能看到用户
/// 已调好的自定义值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRangeChange {
    /// 档位。
    pub range: LogRange,
    /// 自定义起始（`Custom` 时参与查询）。
    pub start: DateTimeValue,
    /// 自定义结束（同上）。
    pub end: DateTimeValue,
}

impl Default for TimeRangeChange {
    /// 缺省 = `最近 1 小时` + 可表示全区间（**不读时钟**，见 **FR1**）。
    fn default() -> Self {
        Self {
            range: RANGE_ORDER[0],
            start: CUSTOM_FROM_MIN,
            end: CUSTOM_TO_MAX,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 共享件本体
// ═══════════════════════════════════════════════════════════════════════════

/// 变更回调槽（与 `p4_interlock.rs::IntentSlot` 同型；抽别名避免 `type_complexity` 告警）。
type RangeSlot = RefCell<Option<Box<dyn FnMut(TimeRangeChange)>>>;

/// 时间范围筛选件（UI §6.3 ③ / §6.5）。
///
/// **零文本输入**：全部控件是 `SegmentedControl`（`lv_buttonmatrix`）与 `DateTimeStepper`
/// （`lv_button` + `lv_label` 组合）—— 无文本输入控件（`ui_static_constraints` 的静态网逐 token 断言）。
///
/// **所有权纪律**：所有拥有型 LVGL 句柄都存进字段（本项目出过 UAF 级缺陷：局部句柄随返回被
/// `Drop` ⇒ 级联删除子树 ⇒ "界面空白但无报错"）。
pub struct TimeRangeFilter {
    /// 本件根容器（高 = [`body_h`]）。
    root: Obj,
    /// 标签列容器（标签的父对象，**仅为保持子树存活**）。
    label_box: Obj,
    /// 维度名标签（`时间范围`）。
    title: Label,
    /// 档位分段控件（**唯一可点控件之一**）。
    seg: Rc<SegmentedControl>,
    /// 自定义块容器（仅 `Custom` 时可见）。
    custom_box: Obj,
    /// 起始行标签（`起始时间`）。
    start_label: Label,
    /// 起始步进器（5 列：年 / 月 / 日 / 时 / 分）。
    start: Rc<DateTimeStepper>,
    /// 结束行标签（`结束时间`）。
    end_label: Label,
    /// 结束步进器（5 列）。
    end: Rc<DateTimeStepper>,
    /// 当前起止值的**权威副本**（不依赖"上屏时机"的读回口径）。
    ///
    /// ⚠️ **必须是 `Rc<Cell<_>>`（共享）而不是裸 `Cell` + `.clone()`** —— 后者是**值拷贝**
    /// ⇒ 回调写进副本 = 死写（本项目踩过，见 `ui/controls.rs::SegmentedControl::current` 的同款注）。
    start_v: Rc<Cell<DateTimeValue>>,
    /// 结束值（同 [`TimeRangeFilter::start_v`]）。
    end_v: Rc<Cell<DateTimeValue>>,
    /// 用户变更回调（**只报意图、不发请求**）。
    on_change: RangeSlot,
}

impl TimeRangeFilter {
    /// 建件（`initial` 给三档与起止初值；缺省见 [`TimeRangeChange::default`]）。
    ///
    /// ⚠️ **建完必须接线** —— 对外请用 [`build`]（唯一构造入口），直接调本函数会得到一个
    /// "不响应用户操作"的件。
    fn new(parent: &Obj, initial: TimeRangeChange) -> Result<Self, LvglError> {
        let range = initial.range;
        let root = layout_box(parent, Dimens::CONTENT_W, body_h(range))?;

        // ── 首行：维度名 + 三档分段控件 ──
        let label_box = layout_box(&root, LABEL_W, SEG_ROW_H)?;
        label_box.set_pos(0, 0);
        let title = text_label(
            &label_box,
            TEXT_RANGE_LABEL,
            TextSlot::Label,
            Palette::TEXT_SECOND,
        )?;
        title.set_size(LABEL_W, TextSlot::Label.px() as i32);
        title.set_long_mode(LongMode::DOTS);
        title.set_pos(
            0,
            theme::center_offset(SEG_ROW_H, TextSlot::Label.px() as i32),
        );
        let options: [&str; 3] = [
            range_text(RANGE_ORDER[0]),
            range_text(RANGE_ORDER[1]),
            range_text(RANGE_ORDER[2]),
        ];
        let seg = Rc::new(SegmentedControl::new(
            &root,
            &options,
            SEG_W,
            range_index(range),
        )?);
        seg.set_pos(CTRL_X, 0);

        // ── 自定义块：起始 / 结束各一组 `DateTimeStepper` ──
        let custom_box = layout_box(&root, Dimens::CONTENT_W, CUSTOM_H)?;
        custom_box.set_pos(0, CUSTOM_TOP);
        let name_y = theme::center_offset(DATETIME_TOTAL_H, TextSlot::Label.px() as i32);
        let start_label = text_label(
            &custom_box,
            TEXT_START,
            TextSlot::Label,
            Palette::TEXT_SECOND,
        )?;
        start_label.set_size(LABEL_W, TextSlot::Label.px() as i32);
        start_label.set_long_mode(LongMode::DOTS);
        start_label.set_pos(0, name_y);
        let start = Rc::new(DateTimeStepper::new(&custom_box, initial.start)?);
        start.set_pos(CTRL_X, 0);
        let end_label = text_label(&custom_box, TEXT_END, TextSlot::Label, Palette::TEXT_SECOND)?;
        end_label.set_size(LABEL_W, TextSlot::Label.px() as i32);
        end_label.set_long_mode(LongMode::DOTS);
        end_label.set_pos(0, DATETIME_TOTAL_H + Dimens::GAP_MIN + name_y);
        let end = Rc::new(DateTimeStepper::new(&custom_box, initial.end)?);
        end.set_pos(CTRL_X, DATETIME_TOTAL_H + Dimens::GAP_MIN);

        let start_v = Rc::new(Cell::new(initial.start));
        let end_v = Rc::new(Cell::new(initial.end));

        let this = Self {
            root,
            label_box,
            title,
            seg,
            custom_box,
            start_label,
            start,
            end_label,
            end,
            start_v,
            end_v,
            on_change: RefCell::new(None),
        };
        // 初值即"最新已知态"：初始化**不触发**回调（与 `SegmentedControl::set_selected`
        // 的"程序化设值不回调"同口径 —— 意图只由**用户操作**产生）。
        this.apply_layout(range);
        Ok(this)
    }

    /// 改档位的**内部**动作（不触发回调）：段选中 + 自定义块显隐 + 根体高。
    fn apply_layout(&self, r: LogRange) {
        self.seg.set_selected(range_index(r));
        set_visible(&self.custom_box, r == LogRange::Custom);
        self.root.set_size(Dimens::CONTENT_W, body_h(r));
    }

    /// 组装当前载荷。
    fn change_now(&self, range: LogRange) -> TimeRangeChange {
        TimeRangeChange {
            range,
            start: self.start_v.get(),
            end: self.end_v.get(),
        }
    }

    /// 触发用户回调（**绝不 panic**；重入时静默跳过，与 `p4_interlock.rs` 的 **IL28** 同口径）。
    fn fire(&self, c: TimeRangeChange) {
        if let Ok(mut s) = self.on_change.try_borrow_mut() {
            if let Some(f) = s.as_mut() {
                f(c);
            }
        }
    }

    /// **接线**（只在 [`build`] 内调用一次）：段控件 / 两个步进器 → 本件回调。
    fn wire(me: &Rc<Self>, start_v: &Rc<Cell<DateTimeValue>>, end_v: &Rc<Cell<DateTimeValue>>) {
        // 段控件：`VALUE_CHANGED` 的载荷是**本次按下的段**（键矩阵语义，见 `ui/controls.rs`）。
        {
            let w = Rc::downgrade(me);
            me.seg.set_on_change(move |i| {
                let Some(f) = w.upgrade() else { return };
                let r = range_at(i);
                // 先展开 / 收起（调用方在回调里据 `change.range` 重摆后续区块），再报意图。
                f.apply_layout(r);
                f.fire(f.change_now(r));
            });
        }
        // 起始 / 结束步进器：任一分量变值 ⇒ 写回权威副本 + 报意图。
        {
            let w = Rc::downgrade(me);
            let v = Rc::clone(start_v);
            me.start.set_on_change(move |next: DateTimeValue| {
                let Some(f) = w.upgrade() else { return };
                v.set(next);
                f.fire(f.change_now(f.range()));
            });
        }
        {
            let w = Rc::downgrade(me);
            let v = Rc::clone(end_v);
            me.end.set_on_change(move |next: DateTimeValue| {
                let Some(f) = w.upgrade() else { return };
                v.set(next);
                f.fire(f.change_now(f.range()));
            });
        }
    }

    /// 本件根对象（**摆放与「之后区块」的重摆都由调用方负责**，见 [`body_h`]）。
    pub fn obj(&self) -> &Obj {
        &self.root
    }

    /// 标签列容器（装配断言口径）。
    pub fn label_obj(&self) -> &Obj {
        &self.label_box
    }

    /// 维度名文案（装配断言口径）。
    pub fn title_text(&self) -> Option<String> {
        self.title.text()
    }

    /// 起始 / 结束行标签文案（`0` = 起始，`1` = 结束；越界 `None`）。
    pub fn name_text(&self, index: usize) -> Option<String> {
        match index {
            0 => self.start_label.text(),
            1 => self.end_label.text(),
            _ => None,
        }
    }

    /// 分段控件本体（装配 / 断言口径；**改档位请用 [`TimeRangeFilter::set_range`]**）。
    pub fn seg(&self) -> &SegmentedControl {
        &self.seg
    }

    /// 自定义块容器（装配断言口径）。
    pub fn custom_obj(&self) -> &Obj {
        &self.custom_box
    }

    /// 起始 / 结束步进器（`0` = 起始，`1` = 结束；越界 `None`）。
    pub fn stepper(&self, index: usize) -> Option<&DateTimeStepper> {
        match index {
            0 => Some(&self.start),
            1 => Some(&self.end),
            _ => None,
        }
    }

    /// 当前档位（读自**段控件的实测选中位**，不另存副本 —— 与本仓"单一真源"口径一致）。
    pub fn range(&self) -> LogRange {
        range_at(self.seg.selected())
    }

    /// 当前起始值。
    pub fn start(&self) -> DateTimeValue {
        self.start_v.get()
    }

    /// 当前结束值。
    pub fn end(&self) -> DateTimeValue {
        self.end_v.get()
    }

    /// 当前载荷（档位 + 起止）。
    pub fn change(&self) -> TimeRangeChange {
        self.change_now(self.range())
    }

    /// 当前档位对应的体高（= [`body_h`]`(`[`TimeRangeFilter::range`]`)`）。
    pub fn body_h(&self) -> i32 {
        body_h(self.range())
    }

    /// 自定义块是否在显（读回 LVGL 的隐藏标志，**不另存副本**）。
    pub fn custom_visible(&self) -> bool {
        !self.custom_box.is_hidden()
    }

    /// **程序化**置档（段选中 + 展开 / 收起 + 体高），**不触发** [`TimeRangeFilter::set_on_change`]。
    ///
    /// 供"外部状态 → 屏"这一路使用（B3 收到回执 / 进入页面时回填；见 **FR1**）。
    pub fn set_range(&self, r: LogRange) {
        self.apply_layout(r);
    }

    /// **程序化**置起止（**不触发**回调）。
    pub fn set_bounds(&self, start: DateTimeValue, end: DateTimeValue) {
        self.start_v.set(start);
        self.end_v.set(end);
        self.start.set_value(start);
        self.end.set_value(end);
    }

    /// **程序化**置全量（档位 + 起止；**不触发**回调）。
    pub fn set_change(&self, c: TimeRangeChange) {
        self.set_bounds(c.start, c.end);
        self.apply_layout(c.range);
    }

    /// 注册用户变更回调。
    ///
    /// ⚠️ **可在回调体内自替换** —— 注册侧走 `try_borrow_mut`（在回调期间再注册不得 panic，
    /// 该 panic 会被事件桥 `catch_unwind` 静默吞掉，屏上无任何迹象）。语义为"新回调自下一次
    /// 通知起生效"（与 `ui/controls.rs` 的 `set_on_change` 同款）。
    pub fn set_on_change<F>(&self, f: F)
    where
        F: FnMut(TimeRangeChange) + 'static,
    {
        if let Ok(mut s) = self.on_change.try_borrow_mut() {
            *s = Some(Box::new(f));
        }
    }
}

/// 建件 + 接线（**唯一的对外构造入口**）。
///
/// # 为什么拆成两步再合并成一个入口
///
/// 接线需要 `&Rc<Self>`（回调只持 `Weak`，避免 `Rc` 环令句柄永不落地），而 `new` 返回 `Self`
/// ⇒ 接线只能发生在 `Rc::new` **之后**。对外只暴露本函数 ⇒ 调用方**拿不到"建了但没接线"的件**。
pub fn build(parent: &Obj, initial: TimeRangeChange) -> Result<Rc<TimeRangeFilter>, LvglError> {
    let f = Rc::new(TimeRangeFilter::new(parent, initial)?);
    TimeRangeFilter::wire(&f, &f.start_v, &f.end_v);
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 档位 ↔ 段下标 ↔ 文案（纯逻辑）──

    /// 三档的槽位序 = UI §6.3 ③ 的段序（`最近 1 小时` / `最近 24 小时` / `自定义`）。
    ///
    /// **改什么会让本条变红**：改动 `RANGE_ORDER` 的次序、或 `range_index` / `range_at`
    /// 任一侧的映射（段序与档位错位 ⇒ 用户点第 3 段却拿到第 1 档）。
    #[test]
    fn range_order_matches_wireframe() {
        assert_eq!(RANGE_ORDER, [LogRange::H1, LogRange::H24, LogRange::Custom]);
        assert_eq!(range_text(RANGE_ORDER[0]), "最近 1 小时");
        assert_eq!(range_text(RANGE_ORDER[1]), "最近 24 小时");
        assert_eq!(range_text(RANGE_ORDER[2]), "自定义");
        for (i, r) in RANGE_ORDER.iter().enumerate() {
            assert_eq!(range_index(*r), i, "档位 → 段下标");
            assert_eq!(range_at(i), *r, "段下标 → 档位");
        }
        // 越界**不 panic**、夹取到第 0 档。
        assert_eq!(range_at(3), RANGE_ORDER[0]);
        assert_eq!(range_at(usize::MAX), RANGE_ORDER[0]);
    }

    /// 自定义展开 ⇒ 体高从 48 涨到 284（UI §6.3「仅『自定义』时展开」，`LG-04`）。
    ///
    /// **改什么会让本条变红**：把 `body_h` 的 `Custom` 分支改成与另两档同值 —— 页面就会把
    /// 表头 / 列表压在**展开的**自定义块下面（重叠）。
    #[test]
    fn body_height_grows_only_for_custom() {
        assert_eq!(body_h(LogRange::H1), SEG_ROW_H);
        assert_eq!(body_h(LogRange::H24), SEG_ROW_H);
        assert_eq!(body_h(LogRange::H1), body_h(LogRange::H24));
        assert_eq!(body_h(LogRange::Custom), 284, "48 + 8 + 106 + 16 + 106");
        assert!(
            body_h(LogRange::Custom) > body_h(LogRange::H24),
            "自定义块必须真的把高度撑开（否则展开后与列表重叠）"
        );
    }

    /// 时间换算与 `ui/pages::format_epoch_ms_utc` 的**已知向量互证**（同一 UI 文档示例时刻）。
    ///
    /// **改什么会让本条变红**：`days_from_civil` 的基点 / 月份基点写错（如 `mp = m - 3` 的
    /// 边界、`- 719468` 写错）—— 第一、二条即红，且**屏上表现为查询窗口整体偏移**。
    #[test]
    fn datetime_epoch_ms_matches_known_vectors() {
        let t = DateTimeValue::from_parts(2026, 9, 10, 13, 42);
        assert_eq!(datetime_to_epoch_ms(t), 1_789_047_720_000);
        assert_eq!(
            crate::ui::pages::format_epoch_ms_utc(datetime_to_epoch_ms(t)),
            "2026/09/10 13:42:00"
        );
        // 纪元原点（UI §6.5 线框示例时刻 13:42:07 = 1_789_047_727_000）
        assert_eq!(
            datetime_to_epoch_ms(DateTimeValue::from_parts(1970, 1, 1, 0, 0)),
            0
        );
        // 闰日（2024-02-29）
        assert_eq!(
            crate::ui::pages::format_epoch_ms_utc(datetime_to_epoch_ms(DateTimeValue::from_parts(2024, 2, 29, 12, 34))),
            "2024/02/29 12:34:00"
        );
        // 上界（2100/12/31 23:59）
        assert_eq!(
            crate::ui::pages::format_epoch_ms_utc(datetime_to_epoch_ms(CUSTOM_TO_MAX)),
            "2100/12/31 23:59:00"
        );
        // 单调：越晚的时刻毫秒越大。
        let a = datetime_to_epoch_ms(DateTimeValue::from_parts(2026, 1, 1, 0, 0));
        let b = datetime_to_epoch_ms(DateTimeValue::from_parts(2026, 12, 31, 23, 59));
        assert!(a < b);
    }

    /// **FR2**：跨分量非法组合（`2 月 31 日`）**不报错也不夹取**，按日历算式落到 `3 月 3 日`。
    ///
    /// 本条的用途是把该行为**钉死**（而非宣称它"正确"）：它确定、可测、且用户改一次步进器
    /// 即修正；UI 层没有（也不硬造）§3.6 之外的错误文案。
    #[test]
    fn invalid_calendar_combo_falls_through_deterministically() {
        let feb31 = DateTimeValue::from_parts(2026, 2, 31, 0, 0);
        assert_eq!(
            crate::ui::pages::format_epoch_ms_utc(datetime_to_epoch_ms(feb31)),
            "2026/03/03 00:00:00"
        );
    }

    /// 缺省载荷 = 第 0 档 + 可表示全区间（**不读时钟** —— 见 **FR1**）。
    #[test]
    fn default_change_has_no_clock_dependency() {
        let d = TimeRangeChange::default();
        assert_eq!(d.range, LogRange::H1);
        assert_eq!(d.start, CUSTOM_FROM_MIN);
        assert_eq!(d.end, CUSTOM_TO_MAX);
        assert_eq!(
            datetime_to_epoch_ms(d.start),
            0,
            "缺省起始 = 纪元原点（可表示下界）"
        );
        assert!(
            datetime_to_epoch_ms(d.end)
                > datetime_to_epoch_ms(DateTimeValue::from_parts(2100, 1, 1, 0, 0)),
            "缺省结束 = 可表示上界（不是「现在」）"
        );
    }

    /// 三段选项文案都在清册内（防"改了档位忘了改清册"）。
    #[test]
    fn texts_cover_the_three_segments() {
        for r in RANGE_ORDER {
            assert!(
                ALL_TEXTS.contains(&range_text(r)),
                "段文案必须在 ALL_TEXTS 清册内"
            );
        }
        assert_eq!(ALL_TEXTS.len(), 6);
    }
}
