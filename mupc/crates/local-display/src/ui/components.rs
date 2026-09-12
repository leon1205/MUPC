//! # `ui/components.rs` —— 薄层之上的**组合控件**（12-MUPC 工作单元 B1）
//!
//! 设计 §5.1 原话列举的 8 个组合件全在这里：
//! [`StatusChip`] / [`LedIndicator`] / [`Stepper`] / [`MultiSelectChips`] /
//! [`ConfirmDialog`] / [`Toast`] / [`EmptyState`] / [`UnavailableState`]；
//! 另含 `ConfirmDialog` 的 L2+ 强制项 [`WarnBanner`]（设计 §2.5）与 TT-10 防重的纯逻辑件
//! [`Debounce`]。
//!
//! ## 纪律（逐条对应设计要求，评审检查项）
//!
//! - **本文件不含任何裸数值**：所有色 / 尺寸 / 字号 / 时长一律经 `crate::ui::theme` 取用
//!   （设计 §5.6「控件策略」行：页面内不得硬编码裸色值 / 裸尺寸）。
//! - **不直连底层绑定**：只经 `crate::lvgl` 薄安全层办事（设计 §11.4 静态约束 ⑤）。
//! - **F14 语义三重冗余**：[`StatusChip`] / [`LedIndicator`] 的**构造签名强制**
//!   `icon + text + color` 三个通道 —— 没有"只给颜色"的实例化路径，故"纯色块"结构性不可达
//!   （设计 §5.6 F14 行 / UI §5.3）。
//! - **不可用 ≠ 空**：[`UnavailableState`] 与 [`EmptyState`] 是两个类型、两个构造入口、
//!   两个语义标记（[`StateSemantics`]）；且 [`UnavailableState`] **只接受**
//!   [`UnavailableKind`]（标题与图标由枚举给定），调用方**无法**把它写成「无 X」/「未联锁」
//!   （设计 §5.6 / UI §8.3 EDGE-09 / F16.6 fail-closed）。
//! - **零文本输入**：步进器用 `lv_button` + `lv_label` 组合（设计 §5.6 F12 行：弃
//!   `lv_spinbox`）。本文件不出现任何文本输入控件符号。
//! - **回调内绝不 panic**：回调里不做 `unwrap` / 不做可能 panic 的索引；共享状态一律
//!   `try_borrow*`（拿不到就跳过）。
//! - **不提供跨线程 API**：本文件所有类型都含 LVGL 句柄（自动 `!Send` / `!Sync`），全部调用
//!   必须在事件循环线程内（设计 §5.2 不变量 4）。
//! - **回调内不删除自身对象**：`ConfirmDialog` 的按钮回调只**发通知**，弹层由调用方在自己的
//!   tick 里 [`ConfirmDialog::close`]。
//!
//! ## 布局手法（薄层定位能力有限，如实标注）
//!
//! 薄层只有 [`Obj::set_pos`] / [`Obj::set_size`] / [`Obj::center`]，**没有**
//! `lv_obj_align` / 文本对齐样式（→ 见 B1 报告的「薄层缺能力」清单）。故本文件统一用
//! **显式坐标 + theme 常量**排布，用 [`theme::center_offset`] 表达"在 N px 的槽内垂直居中
//! 一个 M px 的元素"；需要**水平**居中的场景用"等宽行容器 + [`Obj::center`]"两层结构实现。
//! 这与 UI §2.1「控件横向排布一律使用 Flex 布局**或显式设计常量**，禁止裸数值」的口径一致。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use crate::lvgl::event::EventCode;
use crate::lvgl::obj::{Obj, ObjFlag};
use crate::lvgl::style::{Color, Part, State, Style, StyleSelector};
use crate::lvgl::widgets::{self, Anim, Bar, Group, Label, Led, LongMode, ScrollContainer, TextButton};
use crate::lvgl::LvglError;
use crate::ui::theme::{self, ChipSkin, ConfirmLevel, Dimens, Palette, TextSlot};

// ── 回调槽类型别名（三处 `Rc<RefCell<Option<Box<dyn FnMut(..)>>>>` 的语义化命名）──
//
// 三个别名形状相同、**语义不同**（有无载荷、载荷是什么），故分别命名而非共用一个
// `Callback` —— 读签名时即可知道回调会收到什么。三者都**不出现在任何 `pub` 签名里**
// （对外的接线入口一律是泛型 `set_on_*<F: FnMut(..)>`），故不导出。

/// 无参回调槽：`Rc<RefCell<Option<Box<dyn FnMut()>>>>`。
///
/// 组合件用 `Rc<RefCell<…>>` 持有回调，使 LVGL 事件闭包能在 `&self` 下替换 / 触发它；
/// `Option` 表示"尚未接线"（未接线即 no-op），`Box<dyn FnMut()>` 让调用方传任意捕获闭包。
/// 现有用处：[`ConfirmDialog`] 的确认 / 取消**通知**（只报"已决定"，不携带载荷），
/// 经 `fire_unit` 静默触发（借用失败即跳过，**不 panic**，见模块文档）。
type NotifyCallback = Rc<RefCell<Option<Box<dyn FnMut()>>>>;

/// 数值变更回调槽：`Rc<RefCell<Option<Box<dyn FnMut(i64)>>>>`。
///
/// 语义同 [`NotifyCallback`]，只是携带**变更后的数值**：[`Stepper`] 在 `−` / `＋` 让值真的
/// 改变时经 `fire_value` 回调一次。
type ValueCallback = Rc<RefCell<Option<Box<dyn FnMut(i64)>>>>;

/// 选中项变更回调槽：`Rc<RefCell<Option<Box<dyn FnMut(Vec<usize>)>>>>`。
///
/// 载荷是**当前被勾选的全部下标**（升序快照），由 [`MultiSelectChips`] 在每次勾选翻转后回调。
type SelectionCallback = Rc<RefCell<Option<Box<dyn FnMut(Vec<usize>)>>>>;

// ── 文案常量（逐字取自 UI §3.6 全屏用字表；全屏文案的唯一 Rust 落点）──────────

/// 弹层取消按钮文案（UI §7.3）。
pub const TEXT_CANCEL: &str = "取消";
/// 「影响范围」段标题（UI §7.3）。
pub const TEXT_IMPACT_HEADING: &str = "影响范围";
/// 明细段标题（UI §7.3）。
pub const TEXT_DETAILS_HEADING: &str = "将修改的字段";
/// 新旧值之间的箭头（UI §7.3）。
pub const TEXT_ARROW: &str = "→";
/// WarnBanner 固定文案（UI §2.5；**与文档的逐字偏差见下**）。
///
/// ⚠️ **偏差（如实标注，已列入 B1 报告）**：UI §2.5 写作
/// 「生效瞬间通信将短暂中断**（**≤ 5 s**）**」，但 §3.6 的**声明符号集**
/// （`0–9 . : – · / % Σ ! ? × ≤ ≥ → ← + − ⚠`）**没有圆括号**，
/// `fonts/font_subset_charset.txt`（实测 326 字符）里也**没有任何括号字形** —— 照抄会在屏上
/// 出豆腐块。故此处按"**以声明的字符集为准**"取**无括号**写法（语义不变）。
/// 若 PM 裁定保留括号，须先把括号补进 §3.6 与 `extract_charset.py` 的字符集并重跑
/// `gen_fonts.sh`。
pub const TEXT_WARN_BANNER: &str = "生效瞬间通信将短暂中断 ≤ 5 s";
/// WarnBanner 第二行前缀（UI §2.5「涉及：<字段名列表>」）。
///
/// 同上：冒号取 §3.6 声明的**半角** `:`（全角 `：` 不在字符集内）。
pub const TEXT_WARN_FIELDS_PREFIX: &str = "涉及:";
/// WarnBanner 第二行字段分隔符（§3.6 声明集内的 `/`；`、` 与 `,` 均不在字符集内）。
pub const TEXT_WARN_FIELD_SEP: &str = "/";
/// 多选 Chip 选中态前缀（UI §5.2「选中 + 前缀 ✓」）。
pub const TEXT_CHECK_PREFIX: &str = "✓";
/// 步进器减号（§3.6 声明集内的 `−` U+2212）。
pub const TEXT_STEP_MINUS: &str = "−";
/// 步进器加号（§3.6 声明集内的**半角** `+`；全角 `＋` 不在字符集内）。
pub const TEXT_STEP_PLUS: &str = "+";

/// 本文件上屏的全部固定文案（供 `ui/tests.rs` 做**码表覆盖率**走查 —— UI §3.6 是全屏
/// 用字表，漏字即"豆腐块"，设计 §11.1 有同款测试）。
pub const ALL_TEXTS: &[&str] = &[
    TEXT_CANCEL,
    TEXT_IMPACT_HEADING,
    TEXT_DETAILS_HEADING,
    TEXT_ARROW,
    TEXT_WARN_BANNER,
    TEXT_WARN_FIELDS_PREFIX,
    TEXT_WARN_FIELD_SEP,
    TEXT_CHECK_PREFIX,
    TEXT_STEP_MINUS,
    TEXT_STEP_PLUS,
];

// ═══════════════════════════════════════════════════════════════════════════
// TT-10：500 ms 防重（纯逻辑，不触碰 LVGL —— 可独立单测）
// ═══════════════════════════════════════════════════════════════════════════

/// TT-10 防重器（设计 §5.6 / UI §7.3「提交后按钮 disabled ≥500 ms」）。
///
/// **Rust 侧 `last_fire_at`**（设计原话）：LVGL 侧只负责"禁用态视觉反馈"
/// （[`TextButton::set_disabled`]），"同一窗口内只生效一次"的判定在这里 —— 它是**业务
/// 不变量**，不该依赖某个控件的内部状态。
///
/// 时钟**注入**（[`Debounce::try_accept`] 吃 `now`），故可离线确定性单测，不用真等 500 ms。
#[derive(Debug)]
pub struct Debounce {
    window: Duration,
    last: Cell<Option<Instant>>,
}

impl Debounce {
    /// 以给定窗口构造（窗口值取自 [`theme::Timing::debounce`]）。
    pub const fn new(window: Duration) -> Self {
        Self {
            window,
            last: Cell::new(None),
        }
    }

    /// 尝试"放行"一次触发：窗口内重复触发返回 `false`，且**不刷新**窗口起点
    /// （否则"连点续命"会让按钮永不恢复）。
    pub fn try_accept(&self, now: Instant) -> bool {
        match self.last.get() {
            Some(prev) if now.saturating_duration_since(prev) < self.window => false,
            _ => {
                self.last.set(Some(now));
                true
            }
        }
    }

    /// 清空窗口（弹层重开、字段再次被合法修改等）。
    pub fn reset(&self) {
        self.last.set(None);
    }

    /// 窗口长度。
    pub fn window(&self) -> Duration {
        self.window
    }

    /// 上次放行时刻。
    pub fn last(&self) -> Option<Instant> {
        self.last.get()
    }
}

impl Default for Debounce {
    fn default() -> Self {
        Self::new(theme::Timing::debounce())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 内部小工具
// ═══════════════════════════════════════════════════════════════════════════

/// 建一个透明布局容器（行 / 列 / 居中壳）。
fn layout_box(parent: &Obj, w: i32, h: i32) -> Result<Obj, LvglError> {
    let o = Obj::create(parent)?;
    o.set_size(w, h);
    o.add_style(&theme::transparent(), StyleSelector::main());
    o.remove_flag(ObjFlag::SCROLLABLE);
    Ok(o)
}

/// 建一个纯装饰块（色条 / 分隔线）：不可点、不可滚。
fn decor(parent: &Obj, w: i32, h: i32, style: &Rc<Style>) -> Result<Obj, LvglError> {
    let o = Obj::create(parent)?;
    o.set_size(w, h);
    o.add_style(style, StyleSelector::main());
    o.remove_flag(ObjFlag::CLICKABLE);
    o.remove_flag(ObjFlag::SCROLLABLE);
    Ok(o)
}

/// 建一个文本标签（字号 + 颜色都来自 `theme`）。
fn text_label(parent: &Obj, text: &str, slot: TextSlot, color: Color) -> Result<Label, LvglError> {
    let l = Label::create_with_text(parent, text)?;
    l.add_style(&theme::text(slot, color), StyleSelector::main());
    l.remove_flag(ObjFlag::CLICKABLE);
    l.remove_flag(ObjFlag::SCROLLABLE);
    Ok(l)
}

/// 静默触发一个"无参"回调：拿不到借用就跳过（**不 panic**，见模块文档）。
fn fire_unit(slot: &NotifyCallback) {
    if let Ok(mut s) = slot.try_borrow_mut() {
        if let Some(f) = s.as_mut() {
            f();
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// StatusChip —— 状态胶囊（F14 三通道强制）
// ═══════════════════════════════════════════════════════════════════════════

/// 状态胶囊（UI §5.1 #13：圆角胶囊，高 32、文字 24，**非交互**）。
///
/// **F14 三重冗余在签名层面强制**：构造必须同时给出
///
/// - `icon`：**图形 / 图标通道**（几何字形，如 `✓` / `✕` / `⚠`）；
/// - `text`：**文字通道**（如 `未取数` / `数据过期`）；
/// - `skin`：**颜色通道**（[`ChipSkin`]；其 `bg`/`border`/`text` 三色全部来自
///   [`theme::Palette`]，[`ChipSkin::accent`] 即该通道的值）。
///
/// 三者**都没有默认值**，故"纯色块"（只有颜色、没有文字 / 图标）**无法被构造出来**。
#[derive(Debug)]
pub struct StatusChip {
    obj: Obj,
    icon: Rc<Label>,
    text: Rc<Label>,
    skin: ChipSkin,
}

impl StatusChip {
    /// 建胶囊。`width` 由调用方给 —— 文本宽度需要字体度量，薄层不提供度量接口，
    /// 故宽度不可能由本组件"测"出来，只能由知道版式的调用方给（见模块文档「布局手法」）。
    pub fn new(
        parent: &Obj,
        width: i32,
        icon: &str,
        text: &str,
        skin: ChipSkin,
    ) -> Result<Self, LvglError> {
        let obj = layout_box(parent, width, Dimens::STATUS_CHIP_H)?;
        obj.add_style(&skin.style(), StyleSelector::main());
        let y = theme::center_offset(Dimens::STATUS_CHIP_H, TextSlot::Body.px() as i32);
        let icon_l = text_label(&obj, icon, TextSlot::Body, skin.text)?;
        icon_l.set_pos(0, y);
        let text_l = text_label(&obj, text, TextSlot::Body, skin.text)?;
        text_l.set_pos(Dimens::GAP_MIN + Dimens::STATUS_CHIP_H / 2, y);
        Ok(Self {
            obj,
            icon: Rc::new(icon_l),
            text: Rc::new(text_l),
            skin,
        })
    }

    /// 底层对象（改尺寸 / 位置用；亦经 `Deref` 直接可用）。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 图标通道的底层对象（**只读断言用**：核对它确实挂在胶囊内、未被容器 padding 挤偏）。
    ///
    /// 与 [`ConfirmDialog::title`]/[`Toast::accent_bar`] 同类：为让"子对象位置"可被离屏断言，
    /// 需要暴露只读句柄（`ui/**` 不得直连 `lvgl_sys`，无此访问器就无法断言位置口径）。
    pub fn icon_obj(&self) -> &Obj {
        self.icon.obj()
    }

    /// 改文字（**只改内容，不改三通道结构**）。
    pub fn set_text(&self, text: &str) {
        self.text.set_text(text);
    }

    /// 当前文字（文字通道）。
    pub fn text(&self) -> Option<String> {
        self.text.text()
    }

    /// 当前图标字形（图标通道）。
    pub fn icon_text(&self) -> Option<String> {
        self.icon.text()
    }

    /// 当前皮肤。
    pub fn skin(&self) -> ChipSkin {
        self.skin
    }

    /// **颜色通道**的取值（F14 断言口径 = [`ChipSkin::accent`]）。
    pub fn accent(&self) -> Color {
        self.skin.accent()
    }
}

impl std::ops::Deref for StatusChip {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// LedIndicator —— 指示灯（lv_led + 文字，F14 三通道强制）
// ═══════════════════════════════════════════════════════════════════════════

/// 指示灯指示器（UI §5.1 #14：`lv_led` + `lv_label`，圆 16 px + 文字 24）。
///
/// **F14 三重冗余在签名层面强制**：`icon`（几何字形：实心 `●` / 空心 `○` / 未知 `?`）+
/// `text`（文字信道）+ `color`（[`Led`] 的灯色）。`lv_led` **只承担"灯"这一个语义**
/// （设计 §5.6 F14 行原话："`.text` 必须并列设置"），故本组件把"图形 + 灯 + 文字"三件一次
/// 建出，**不存在只建灯不建文字的路径**。
#[derive(Debug)]
pub struct LedIndicator {
    obj: Obj,
    led: Rc<Led>,
    icon: Rc<Label>,
    text: Rc<Label>,
    color: Color,
    lit: Cell<bool>,
}

impl LedIndicator {
    /// 建指示器（`width` 由调用方给，理由同 [`StatusChip::new`]）。
    pub fn new(
        parent: &Obj,
        width: i32,
        icon: &str,
        text: &str,
        color: Color,
    ) -> Result<Self, LvglError> {
        let obj = layout_box(parent, width, Dimens::ICON_SM)?;

        let icon_l = text_label(&obj, icon, TextSlot::SectionTitle, color)?;
        icon_l.set_size(Dimens::ICON_SM, Dimens::ICON_SM);
        icon_l.set_pos(0, 0);

        // `lv_led` 只作"灯"：颜色是控件字段（不是样式），必须由此路径显式设定。
        let led = Rc::new(Led::create(&obj)?);
        led.set_size(Dimens::LED_DIA, Dimens::LED_DIA);
        led.set_pos(
            Dimens::ICON_SM + Dimens::GAP_MIN / 2,
            theme::center_offset(Dimens::ICON_SM, Dimens::LED_DIA),
        );
        led.set_color(color);
        led.on();

        let text_l = text_label(&obj, text, TextSlot::Label, Palette::TEXT_SECOND)?;
        text_l.set_pos(
            Dimens::ICON_SM + Dimens::GAP_MIN / 2 + Dimens::LED_DIA + Dimens::GAP_MIN / 2,
            theme::center_offset(Dimens::ICON_SM, TextSlot::Label.px() as i32),
        );

        Ok(Self {
            obj,
            led,
            icon: Rc::new(icon_l),
            text: Rc::new(text_l),
            color,
            lit: Cell::new(true),
        })
    }

    /// 底层对象。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 点灯 / 灭灯。
    pub fn set_lit(&self, lit: bool) {
        if lit {
            self.led.on();
        } else {
            self.led.off();
        }
        self.lit.set(lit);
    }

    /// 本组件记录的点亮态。
    pub fn is_lit(&self) -> bool {
        self.lit.get()
    }

    /// 灯的实际亮度（0–255；`lv_led` 内部按 `LV_LED_BRIGHT_MIN/MAX` 夹取）。
    pub fn brightness(&self) -> u8 {
        self.led.brightness()
    }

    /// **颜色通道**（F14 断言口径）。
    pub fn color(&self) -> Color {
        self.color
    }

    /// 文字通道。
    pub fn text(&self) -> Option<String> {
        self.text.text()
    }

    /// 图标通道。
    pub fn icon_text(&self) -> Option<String> {
        self.icon.text()
    }
}

impl std::ops::Deref for LedIndicator {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Stepper —— 受约束步进器（lv_button + lv_label 组合，零文本输入）
// ═══════════════════════════════════════════════════════════════════════════

/// 有界数值步进器（UI §5.1 #6 / §5.3 `Stepper`）。
///
/// **零件**：`−` [`TextButton`] 64×64 + 值区（透明容器 + 纯 [`Label`]，**不挂
/// `LV_OBJ_FLAG_CLICKABLE`**）+ `＋` [`TextButton`] 64×64 —— 设计 §5.6 F12 行：**弃
/// `lv_spinbox`**，改用 `lv_btn` + `lv_label` 组合，故"界面不存在可编辑文本区"是**结构性**的。
///
/// **越界双层约束**（TT-03）：`−` 在 `value == min`、`＋` 在 `value == max` 时进
/// `LV_STATE_DISABLED`（视觉 + 命中），且 [`Stepper::set_value`] 自身也 clamp（类型层）——
/// **越界值在控件层不可达**。
pub struct Stepper {
    obj: Obj,
    minus: Rc<TextButton>,
    plus: Rc<TextButton>,
    /// 值区容器（透明底 + 描边的装饰块）—— `value` 标签的**父对象**。
    ///
    /// **仅为保持子树存活**：LVGL 的对象是拥有型句柄，父句柄 `Drop` 会 `lv_obj_delete`
    /// 该父对象并**级联删除全部子对象**。若本字段缺省，`with_value_width` 返回时
    /// `value_box` 即被删除 ⇒ 子 `value` 标签随之失效（[`Stepper::display`] 恒为 `None`）。
    _value_box: Obj,
    value: Rc<Label>,
    current: Rc<Cell<i64>>,
    min: i64,
    max: i64,
    step: i64,
    /// 调用方显式禁用（只读字段：UI §6.2「`editable=false` 控件 `disabled`」）。
    user_disabled: Rc<Cell<bool>>,
    on_change: ValueCallback,
}

impl Stepper {
    /// 标准步进器（值区宽 = [`Dimens::STEPPER_VALUE_W`]）。
    pub fn new(parent: &Obj, min: i64, max: i64, value: i64, step: i64) -> Result<Self, LvglError> {
        Self::with_value_width(parent, min, max, value, step, Dimens::STEPPER_VALUE_W)
    }

    /// 指定值区宽的步进器（IPv4 段用 [`Dimens::IPV4_VALUE_W`]、日期时间列用
    /// [`Dimens::DATETIME_COL_W`]，均取自 `theme`）。
    ///
    /// # 参数非法即**响亮失败**（不静默"修正"）
    ///
    /// 此前 `step <= 0` 被静默当 `1`、`min > max` 被静默交换 —— 这会把调用方的真 bug
    /// 粉饰成"正常"（本项目最忌静默）。现一律返回 [`LvglError::InvalidArgument`]：
    ///
    /// - `step <= 0`：步长为 0 会让 `−`/`＋` 恒无操作；
    /// - `min > max`：区间倒置 ⇒ 调用方多半传错了参数顺序；
    /// - `value_w < `[`Dimens::TOUCH_MIN`]：值区（连同两侧按钮）不得小于最小触摸目标口径。
    ///
    /// 只有 `value` 仍按 `[min, max]` **clamp**（那是 TT-03 的**类型层**越界约束，语义上
    /// 是"初始值收敛到合法区间"，不是参数非法）。
    pub fn with_value_width(
        parent: &Obj,
        min: i64,
        max: i64,
        value: i64,
        step: i64,
        value_w: i32,
    ) -> Result<Self, LvglError> {
        if step <= 0 {
            return Err(LvglError::InvalidArgument("Stepper: step 必须为正数"));
        }
        if min > max {
            return Err(LvglError::InvalidArgument("Stepper: min 不得大于 max"));
        }
        if value_w < Dimens::TOUCH_MIN {
            return Err(LvglError::InvalidArgument(
                "Stepper: 值区宽不得小于 TOUCH_MIN",
            ));
        }
        // 上界：整件（− / 值区 / ＋）必须装得进内容区，且**乘法不得溢出**。
        // 用 checked 运算而非 `>` 比较 —— 极大 `i64`/`i32` 入参在 debug 下会让
        // `STEPPER_BTN_W * 2 + value_w` 溢出 panic（评审 Minor #3）。
        let Some(total_w) = Dimens::STEPPER_BTN_W
            .checked_mul(2)
            .and_then(|w| w.checked_add(value_w))
        else {
            return Err(LvglError::InvalidArgument("Stepper: 值区宽过大（溢出）"));
        };
        if total_w > Dimens::CONTENT_W {
            return Err(LvglError::InvalidArgument(
                "Stepper: 整件宽度超出内容区",
            ));
        }
        let (lo, hi) = (min, max);
        let start = value.clamp(lo, hi);

        // 复用上面已做 checked 校验的 `total_w`（不重复计算，避免再引入一处可能溢出的加法）。
        let obj = layout_box(parent, total_w, Dimens::STEPPER_H)?;
        obj.remove_flag(ObjFlag::CLICKABLE);

        let current = Rc::new(Cell::new(start));
        let user_disabled = Rc::new(Cell::new(false));
        let on_change: ValueCallback = Rc::new(RefCell::new(None));
        let btn_styles = theme::button(theme::ButtonKind::Secondary);

        // `−`
        let minus = Rc::new(TextButton::create(&obj, TEXT_STEP_MINUS)?);
        minus.set_size(Dimens::STEPPER_BTN_W, Dimens::STEPPER_H);
        minus.set_pos(0, 0);
        minus.label().center();
        minus.add_style(&theme::stepper_button(), StyleSelector::main());
        btn_styles.apply(&minus);

        // 值区（非交互：不挂 CLICKABLE，也不可滚）
        let value_box = decor(&obj, value_w, Dimens::STEPPER_H, &theme::stepper_value())?;
        value_box.set_pos(Dimens::STEPPER_BTN_W, 0);
        let value = Rc::new(text_label(
            &value_box,
            &start.to_string(),
            TextSlot::Label,
            Palette::TEXT_PRIMARY,
        )?);
        value.center();

        // `＋`
        let plus = Rc::new(TextButton::create(&obj, TEXT_STEP_PLUS)?);
        plus.set_size(Dimens::STEPPER_BTN_W, Dimens::STEPPER_H);
        plus.set_pos(Dimens::STEPPER_BTN_W + value_w, 0);
        plus.label().center();
        plus.add_style(&theme::stepper_button(), StyleSelector::main());
        btn_styles.apply(&plus);

        // ── 事件：只调值 + 刷新；业务经 `on_change` 交回调用方 ──
        minus.on_clicked(step_closure(
            &current,
            &value,
            &minus,
            &plus,
            &user_disabled,
            &on_change,
            -step,
            lo,
            hi,
        ));
        plus.on_clicked(step_closure(
            &current,
            &value,
            &minus,
            &plus,
            &user_disabled,
            &on_change,
            step,
            lo,
            hi,
        ));

        let me = Self {
            obj,
            minus,
            plus,
            _value_box: value_box,
            value,
            current,
            min: lo,
            max: hi,
            step,
            user_disabled,
            on_change,
        };
        me.refresh();
        Ok(me)
    }

    /// 底层对象。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 当前值。
    pub fn value(&self) -> i64 {
        self.current.get()
    }

    /// 下界。
    pub fn min(&self) -> i64 {
        self.min
    }

    /// 上界。
    pub fn max(&self) -> i64 {
        self.max
    }

    /// 步长。
    pub fn step(&self) -> i64 {
        self.step
    }

    /// 设值（**clamp 到 `[min, max]`** —— 类型层越界约束，TT-03）。
    pub fn set_value(&self, v: i64) {
        let next = v.clamp(self.min, self.max);
        if next == self.current.get() {
            return;
        }
        self.current.set(next);
        self.value.set_text(&next.to_string());
        self.refresh();
    }

    /// 显式禁用 / 恢复（只读字段、提交中禁用）。
    ///
    /// 禁用时 `−` / `＋` 一律置 `LV_STATE_DISABLED`；恢复时按当前值重算越界禁用态。
    pub fn set_disabled(&self, disabled: bool) {
        self.user_disabled.set(disabled);
        self.refresh();
    }

    /// 是否被调用方显式禁用。
    pub fn is_disabled(&self) -> bool {
        self.user_disabled.get()
    }

    /// `−` 当前是否禁用（越界或显式禁用；离屏断言口径）。
    pub fn minus_disabled(&self) -> bool {
        widgets::has_state(&self.minus, State::DISABLED)
    }

    /// `＋` 当前是否禁用（越界或显式禁用）。
    pub fn plus_disabled(&self) -> bool {
        widgets::has_state(&self.plus, State::DISABLED)
    }

    /// 值区当前文字。
    pub fn display(&self) -> Option<String> {
        self.value.text()
    }

    /// 注册变值回调（每次变化后调用一次；调用方据此更新草稿 / 脏标记）。
    pub fn set_on_change<F>(&self, f: F)
    where
        F: FnMut(i64) + 'static,
    {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }

    /// 按当前值重算视觉状态。
    fn refresh(&self) {
        refresh_bounds(
            &Rc::downgrade(&self.minus),
            &Rc::downgrade(&self.plus),
            self.current.get(),
            self.min,
            self.max,
            self.user_disabled.get(),
        );
    }
}

impl std::ops::Deref for Stepper {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}

impl std::fmt::Debug for Stepper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stepper")
            .field("obj", &self.obj)
            .field("value", &self.current.get())
            .field("min", &self.min)
            .field("max", &self.max)
            .field("step", &self.step)
            .field("user_disabled", &self.user_disabled.get())
            .finish()
    }
}

/// 造一个"按步长调值"的点击闭包（`−` 传 `-step`、`＋` 传 `+step`）。
///
/// 返回值是 `FnMut(Event)`，直接交给 [`Obj::on_clicked`]。
#[allow(clippy::too_many_arguments)]
fn step_closure(
    current: &Rc<Cell<i64>>,
    value: &Rc<Label>,
    minus: &Rc<TextButton>,
    plus: &Rc<TextButton>,
    user_disabled: &Rc<Cell<bool>>,
    on_change: &ValueCallback,
    delta: i64,
    lo: i64,
    hi: i64,
) -> impl FnMut(crate::lvgl::event::Event) + 'static {
    let cur = current.clone();
    let val = value.clone();
    let minus_h = Rc::downgrade(minus);
    let plus_h = Rc::downgrade(plus);
    let ud = user_disabled.clone();
    let cb = on_change.clone();
    move |_e: crate::lvgl::event::Event| {
        let base = cur.get();
        let next = (base + delta).clamp(lo, hi);
        if next == base {
            return;
        }
        cur.set(next);
        val.set_text(&next.to_string());
        refresh_bounds(&minus_h, &plus_h, next, lo, hi, ud.get());
        fire_value(&cb, next);
    }
}

/// 重算 `−` / `＋` 的禁用态（越界 + 显式禁用）。
fn refresh_bounds(
    minus: &Weak<TextButton>,
    plus: &Weak<TextButton>,
    value: i64,
    min: i64,
    max: i64,
    user_disabled: bool,
) {
    if let Some(m) = minus.upgrade() {
        m.set_disabled(user_disabled || value <= min);
    }
    if let Some(p) = plus.upgrade() {
        p.set_disabled(user_disabled || value >= max);
    }
}

/// 触发变值回调（拿到借用才调，**不 panic**）。
fn fire_value(slot: &ValueCallback, v: i64) {
    if let Ok(mut s) = slot.try_borrow_mut() {
        if let Some(f) = s.as_mut() {
            f(v);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// MultiSelectChips —— 多选 Chip 组（换行网格，不横滚）
// ═══════════════════════════════════════════════════════════════════════════

/// 多选 Chip 组（UI §5.1 #5 / §5.3 `MultiChipGroup`）。
///
/// - 每个 Chip 是 `LV_OBJ_FLAG_CHECKABLE` 的 `lv_button`（勾选态由 LVGL 的
///   `LV_STATE_CHECKED` 维护，**不另存一份状态**，故不会与视觉脱节）；
/// - 选中态文案加前缀 [`TEXT_CHECK_PREFIX`]（UI §5.2）；
/// - **换行网格**（结构性禁横滚，UI §2.7 / §6.3 ②）：`columns` 定行、`cell_w` ×
///   [`Dimens::CHIP_H`] 定尺寸、间距取 [`Dimens::GAP_MIN`]。
pub struct MultiSelectChips {
    obj: Obj,
    chips: Rc<RefCell<Vec<TextButton>>>,
    labels: Rc<Vec<String>>,
    columns: u32,
    on_change: SelectionCallback,
}

impl MultiSelectChips {
    /// 建组（`columns` = 每行 Chip 数；`cell_w` = 单 Chip 宽，取 [`Dimens::CHIP_MIN_W`] 或更大）。
    ///
    /// # 参数非法即**响亮失败**（不静默"修正"）
    ///
    /// 此前 `columns` 被静默 `max(1)`、`cell_w` 被静默 `max(CHIP_MIN_W)` —— 调用方的真 bug
    /// 会被粉饰成"正常"（本项目最忌静默）。现一律返回 [`LvglError::InvalidArgument`]：
    ///
    /// - `columns == 0`：除零 / 行数无意义；
    /// - `cell_w < `[`Dimens::CHIP_MIN_W`]：Chip 宽低于 UI §5.1 #5 的「最小宽 96」。
    pub fn new(
        parent: &Obj,
        options: &[&str],
        columns: u32,
        cell_w: i32,
    ) -> Result<Self, LvglError> {
        if columns == 0 {
            return Err(LvglError::InvalidArgument("MultiSelectChips: columns 必须 ≥ 1"));
        }
        if cell_w < Dimens::CHIP_MIN_W {
            return Err(LvglError::InvalidArgument(
                "MultiSelectChips: 单 Chip 宽不得小于 CHIP_MIN_W",
            ));
        }
        // 上界：`columns` 列装得进内容区，且**乘法不得溢出**
        // （`columns` 为 `u32`，极大值转 `i32` / 相乘都会在 debug 下 panic —— 评审 Minor #3）。
        let Ok(cols_i32) = i32::try_from(columns) else {
            return Err(LvglError::InvalidArgument("MultiSelectChips: columns 过大"));
        };
        let Some(row_w) = cols_i32
            .checked_mul(cell_w)
            .and_then(|w| {
                // 列间呼吸缝：`(columns - 1) * GAP_GROUP`
                cols_i32
                    .saturating_sub(1)
                    .checked_mul(Dimens::GAP_GROUP)
                    .and_then(|g| w.checked_add(g))
            })
        else {
            return Err(LvglError::InvalidArgument(
                "MultiSelectChips: 行宽过大（溢出）",
            ));
        };
        if row_w > Dimens::CONTENT_W {
            return Err(LvglError::InvalidArgument(
                "MultiSelectChips: 一行装不下（超出内容区）",
            ));
        }
        let n = options.len() as u32;
        let rows = if n == 0 { 0 } else { n.div_ceil(columns) };
        let w = if rows == 0 {
            0
        } else {
            columns as i32 * (cell_w + Dimens::GAP_MIN) - Dimens::GAP_MIN
        };
        let h = if rows == 0 {
            0
        } else {
            rows as i32 * (Dimens::CHIP_H + Dimens::GAP_MIN) - Dimens::GAP_MIN
        };
        let obj = layout_box(parent, w, h)?;

        let labels: Rc<Vec<String>> = Rc::new(options.iter().map(|s| s.to_string()).collect());
        let chips: Rc<RefCell<Vec<TextButton>>> = Rc::new(RefCell::new(Vec::new()));
        let on_change: SelectionCallback = Rc::new(RefCell::new(None));
        let btn_styles = theme::button(theme::ButtonKind::Secondary);

        for (i, label) in labels.iter().enumerate() {
            let idx = i as u32;
            let chip = TextButton::create(&obj, label)?;
            chip.set_checkable(true);
            chip.set_size(cell_w, Dimens::CHIP_H);
            chip.set_pos(
                (idx % columns) as i32 * (cell_w + Dimens::GAP_MIN),
                (idx / columns) as i32 * (Dimens::CHIP_H + Dimens::GAP_MIN),
            );
            chip.label().center();
            chip.add_style(&theme::control_surface(), StyleSelector::main());
            btn_styles.apply(&chip);

            let weak = Rc::downgrade(&chips);
            let labels_c = labels.clone();
            let cb = on_change.clone();
            chip.on_clicked(move |_| {
                // LVGL 在 `LV_EVENT_RELEASED` 已完成 CHECKED 翻转，`LV_EVENT_CLICKED` 在其后
                // 到达，故此处读到的就是**翻转后**的状态（v9.5.0 `lv_obj.c` 的按钮类处理）。
                let Some(c) = weak.upgrade() else { return };
                let Ok(v) = c.try_borrow() else { return };
                let Some(clicked) = v.get(i) else { return };
                let on = clicked.is_checked();
                // 选中态文案加前缀（UI §5.2）。
                if let Some(l) = labels_c.get(i) {
                    clicked.set_text(&chip_text(l, on));
                }
                let sel: Vec<usize> = v
                    .iter()
                    .enumerate()
                    .filter(|(_, x)| x.is_checked())
                    .map(|(k, _)| k)
                    .collect();
                // 先放掉对 `chips` 的借用，再回调 —— 回调里若调 `selected()` 也要借它。
                drop(v);
                if let Ok(mut s) = cb.try_borrow_mut() {
                    if let Some(f) = s.as_mut() {
                        f(sel);
                    }
                }
            });
            if let Ok(mut v) = chips.try_borrow_mut() {
                v.push(chip);
            }
        }

        Ok(Self {
            obj,
            chips,
            labels,
            columns,
            on_change,
        })
    }

    /// 底层对象。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 每行 Chip 数。
    pub fn columns(&self) -> u32 {
        self.columns
    }

    /// 选项数。
    pub fn len(&self) -> usize {
        self.labels.len()
    }

    /// 是否无选项。
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// 选项文案（不含选中前缀）。
    pub fn option(&self, index: usize) -> Option<&str> {
        self.labels.get(index).map(|s| s.as_str())
    }

    /// 某 Chip **当前显示**的文案（含选中前缀 —— 离屏断言口径）。
    pub fn chip_display(&self, index: usize) -> Option<String> {
        self.chips
            .try_borrow()
            .ok()
            .and_then(|v| v.get(index).and_then(|c| c.text()))
    }

    /// 当前选中项（**升序索引**，读自 LVGL 的勾选态）。
    pub fn selected(&self) -> Vec<usize> {
        self.chips
            .try_borrow()
            .ok()
            .map(|v| {
                v.iter()
                    .enumerate()
                    .filter(|(_, c)| c.is_checked())
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 置某索引的勾选态（越界索引 no-op）。
    pub fn set_selected(&self, index: usize, on: bool) {
        let label = match self.labels.get(index) {
            Some(s) => s.clone(),
            None => return,
        };
        if let Ok(v) = self.chips.try_borrow() {
            if let Some(chip) = v.get(index) {
                chip.set_checked(on);
                chip.set_text(&chip_text(&label, on));
            }
        }
    }

    /// 清空全选。
    pub fn clear(&self) {
        for i in 0..self.labels.len() {
            self.set_selected(i, false);
        }
    }

    /// 注册选择变化回调（每次点击生效一次，参数 = 当前选中索引集合）。
    pub fn set_on_change<F>(&self, f: F)
    where
        F: FnMut(Vec<usize>) + 'static,
    {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }
}

impl std::ops::Deref for MultiSelectChips {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}

impl std::fmt::Debug for MultiSelectChips {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiSelectChips")
            .field("obj", &self.obj)
            .field("option_count", &self.labels.len())
            .field("columns", &self.columns)
            .field("selected", &self.selected())
            .finish()
    }
}

/// Chip 文案（选中加 [`TEXT_CHECK_PREFIX`] 前缀）。
fn chip_text(label: &str, checked: bool) -> String {
    if checked {
        format!("{}{}", TEXT_CHECK_PREFIX, label)
    } else {
        label.to_string()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// WarnBanner —— 警示行（UI §2.5 / §5.1 #12）
// ═══════════════════════════════════════════════════════════════════════════

/// 琥珀警示行（UI §2.5 / §5.1 #12：全宽 × 56、非交互、**不闪烁常亮**）。
///
/// 使用点：`ConfirmDialog` 的 L2+ 强制项（本文件内）、P2 的「有未保存修改」、
/// P3/P5 的「检索范围超限」（后续页面用）。设计 §5.1 的"8 个组合控件"清单未列它，这里
/// 仍做成公开件，避免 B2 另起一套。
#[derive(Debug)]
pub struct WarnBanner {
    obj: Obj,
    icon: Rc<Label>,
    text: Rc<Label>,
    /// 「涉及:…」第二行 —— **无字段时不建**（不给空行白花一个对象；见 [`WarnBanner::new`]）。
    fields: Option<Rc<Label>>,
}

impl WarnBanner {
    /// 建警示行：`text` = 主文案；`fields` = 「涉及：<字段名列表>」的字段名（空则不出第二行）。
    pub fn new(parent: &Obj, width: i32, text: &str, fields: &[&str]) -> Result<Self, LvglError> {
        let obj = layout_box(parent, width, Dimens::BANNER_H)?;
        obj.add_style(&theme::warn_banner(), StyleSelector::main());
        obj.remove_flag(ObjFlag::CLICKABLE);

        let icon = Rc::new(text_label(&obj, "⚠", TextSlot::SectionTitle, Palette::STALE)?);
        icon.set_size(Dimens::ICON_SM, Dimens::ICON_SM);
        icon.set_pos(
            Dimens::BANNER_PAD,
            theme::center_offset(Dimens::BANNER_H, Dimens::ICON_SM),
        );

        let text_x = Dimens::BANNER_PAD + Dimens::ICON_SM + Dimens::GAP_MIN;
        let text_w = (width - text_x - Dimens::BANNER_PAD).max(0);
        let has_fields = !fields.is_empty();
        // 两行时整体上移，使两行块在 56 px 内垂直居中。
        let text_y = if has_fields {
            theme::center_offset(Dimens::BANNER_H, 2 * TextSlot::Body.px() as i32)
        } else {
            theme::center_offset(Dimens::BANNER_H, TextSlot::Body.px() as i32)
        };

        let main = Rc::new(text_label(&obj, text, TextSlot::Body, Palette::STANDBY)?);
        main.set_size(text_w, TextSlot::Body.px() as i32);
        main.set_long_mode(LongMode::WRAP);
        main.set_pos(text_x, text_y);

        // 第二行**只在有字段时建**：此前无条件建出再 `set_hidden(true)`，白花一个对象
        // （评审 Minor 2）。隐藏对象虽不参与布局，但仍是 LVGL 内存池里的一个 `lv_obj`。
        let fields_line = if has_fields {
            let l = Rc::new(text_label(
                &obj,
                &format!(
                    "{}{}",
                    TEXT_WARN_FIELDS_PREFIX,
                    fields.join(TEXT_WARN_FIELD_SEP)
                ),
                TextSlot::Body,
                Palette::STANDBY,
            )?);
            l.set_size(text_w, TextSlot::Body.px() as i32);
            l.set_long_mode(LongMode::DOTS);
            l.set_pos(text_x, text_y + TextSlot::Body.px() as i32);
            Some(l)
        } else {
            None
        };

        Ok(Self {
            obj,
            icon,
            text: main,
            fields: fields_line,
        })
    }

    /// 底层对象。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 主文案。
    pub fn text(&self) -> Option<String> {
        self.text.text()
    }

    /// 图标字形（恒为 `⚠`，UI §2.5）。
    pub fn icon_text(&self) -> Option<String> {
        self.icon.text()
    }

    /// 是否显示「涉及：…」第二行（= 第二行**确实建了**）。
    pub fn has_fields_line(&self) -> bool {
        self.fields.is_some()
    }
}

impl std::ops::Deref for WarnBanner {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ConfirmDialog —— 强确认（L1 / L2 / L2+）
// ═══════════════════════════════════════════════════════════════════════════

/// 明细列表的一行（UI §7.3：`字段  旧值 → 新值`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConfirmDetail<'a> {
    /// 字段名。
    pub field: &'a str,
    /// 旧值（`text_weak`）。
    pub before: &'a str,
    /// 新值（`#2FDB8A`）。
    pub after: &'a str,
}

/// 确认弹层的**内容规格**（不含"强度" —— 强度是 [`ConfirmLevel`]，单独传）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfirmSpec<'a> {
    /// 标题（= 操作名）。
    pub title: &'a str,
    /// 「影响范围」段正文（**L2 起必须为具体副作用**，UI §7.3）。
    pub impact: &'a str,
    /// 明细列表（「字段 旧值 → 新值」；超 8 行时列表区内部滚动）。
    pub details: &'a [ConfirmDetail<'a>],
    /// `requires_reconnect` 字段名列表 → L2+ 的 WarnBanner 第二行（UI §2.5）。
    pub warn_fields: &'a [&'a str],
}

impl<'a> ConfirmSpec<'a> {
    /// 便捷构造（无明细、无瞬断字段）。
    pub const fn new(title: &'a str, impact: &'a str) -> Self {
        Self {
            title,
            impact,
            details: &[],
            warn_fields: &[],
        }
    }
}

/// 明细列表最多直接摆放的行数（UI §7.3「超过 8 行时列表区内部滚动」）。
const DETAILS_MAX_ROWS: usize = 8;

// ── 弹层纵向布局链（全部由 `theme` 常量推导，无裸规格值）─────────────────────
const BAR_H: i32 = Dimens::DIALOG_BAR_H;
const TITLE_Y: i32 = BAR_H + Dimens::GAP_MIN;
const TITLE_H: i32 = TextSlot::PageTitle.px() as i32 + Dimens::GAP_MIN / 2;
const DIVIDER_Y: i32 = TITLE_Y + TITLE_H + Dimens::GAP_MIN;
const HEAD_H: i32 = TextSlot::Label.px() as i32 + Dimens::GAP_MIN / 2;
const IMPACT_HEAD_Y: i32 = DIVIDER_Y + theme::Stroke::THIN + Dimens::GAP_MIN;
const IMPACT_Y: i32 = IMPACT_HEAD_Y + HEAD_H;
const IMPACT_H: i32 = 2 * TextSlot::Body.px() as i32;
const DETAILS_HEAD_Y: i32 = IMPACT_Y + IMPACT_H + Dimens::GAP_MIN;
const DETAILS_TOP: i32 = DETAILS_HEAD_Y + HEAD_H;

/// 强确认弹层（UI §2.5 / §7.3；设计 §5.6「确认强度分级」行）。
///
/// # 强度与形态的对应（**结构性**，不是"约定"）
///
/// | `level` | 确认按钮 | 长按 1.0 s | 顶部色条 | WarnBanner |
/// |---------|----------|:----------:|----------|:----------:|
/// | [`ConfirmLevel::L1`] | 主按钮「确认执行」 | 否 | `#4EA6FF` | 否 |
/// | [`ConfirmLevel::L2`] | 危险按钮「按住确认」 | **是** | `#FF6B6B` | 否 |
/// | [`ConfirmLevel::L2Plus`] | 危险按钮「按住确认」 | **是** | `#FF6B6B` | **强制出现** |
///
/// `level` **必须显式传入**（无默认值）：Rust 没有默认参数，故"漏配强度"在编译期即不成立；
/// [`ConfirmLevel`] 亦不实现 `Default`。
///
/// # 长按阈值由 **indev** 设定（⚠️ v2.0-r4 订正）
///
/// v9.5.0 **不存在**逐控件的长按时长 API（那是 v8 遗留），阈值收敛于**输入设备**：事件循环
/// 注册触摸设备时须调 `lvgl::indev::Indev::set_long_press_time(theme::Timing::long_press_u16())`
/// （= [`theme::Timing::LONG_PRESS_MS`] = 1000 ms）。本组件只监听 `LONG_PRESSED` /
/// `PRESSED` / `RELEASED` / `PRESS_LOST` 四个事件码。
///
/// # 默认焦点「取消」（TT-09）
///
/// 用 `lv_group`：把「取消」入组并聚焦 ⇒ 该对象获得 `LV_STATE_FOCUSED`。
/// **视觉环**由 [`theme::focus_ring`] 承担 —— 它经 [`theme::ButtonStyles::apply`] 挂到
/// `FOCUSED` 态（「取消」/「确认」按钮都由 `theme::button(..).apply(..)` 装配），故
/// "状态位 + 焦点环"两者齐备；状态位的读回口径见
/// [`ConfirmDialog::default_focus_is_cancel`]，焦点环的施加口径见
/// [`theme::ButtonStyles::entries`]。
///
/// # L2 / L2+ 的进度反馈
///
/// 进度是 `lv_bar` 的**值**驱动（设计 §5.6 动效纪律：进度类动效由 `lv_bar` 值驱动而非
/// `lv_anim`）：`PRESSED` 归零并显形，`RELEASED` / `PRESS_LOST` 归零并隐藏，`LONG_PRESSED`
/// 满格并提交。中间过程的平滑由**事件循环**按 tick 调 [`ConfirmDialog::set_progress`] 推进
/// （本组件不持有时钟，也不做循环动画）。
pub struct ConfirmDialog {
    mask: Obj,
    panel: Obj,
    level: ConfirmLevel,
    // ── 弹层静态子树：全是 `panel` 的子对象。**仅为保持子树存活**（见 `Stepper::_value_box`
    // 的同款说明）——拥有型句柄若在 `new` 返回时被 `Drop`，LVGL 会 `lv_obj_delete` 它们
    // （连带其子对象），弹层将只剩按钮与 WarnBanner。故一律存进结构体。 ──
    /// 顶部级别色条（4 px）。
    _level_bar: Obj,
    /// 标题标签（离屏读回入口见 [`ConfirmDialog::title`]）。
    title: Label,
    /// 标题下分隔线。
    _divider: Obj,
    /// 「影响范围」段标题。
    _impact_head: Label,
    /// 「影响范围」段正文。
    _impact: Label,
    /// 「将修改的字段」段标题。
    _detail_head: Label,
    /// 明细滚动列表（行标签见 `_detail_labels`）。
    _list: ScrollContainer,
    /// 明细各行标签（字段 / 旧值 / 箭头 / 新值，按行序）。
    _detail_labels: Vec<Label>,
    cancel: Rc<TextButton>,
    confirm: Rc<TextButton>,
    progress: Option<Rc<Bar>>,
    warn: Option<WarnBanner>,
    group: Group,
    on_confirm: NotifyCallback,
    on_cancel: NotifyCallback,
    debounce: Rc<Debounce>,
}

impl ConfirmDialog {
    /// 在 `parent`（通常 `lvgl::widgets::layer_top()`）下建模态弹层。
    pub fn new(parent: &Obj, spec: &ConfirmSpec<'_>, level: ConfirmLevel) -> Result<Self, LvglError> {
        // ── 遮罩：全屏、可点以拦截穿透；**不挂任何关闭回调** ⇒ 点击不关闭 ──
        let mask = Obj::create(parent)?;
        mask.set_size(Dimens::SCREEN_W, Dimens::SCREEN_H);
        mask.set_pos(0, 0);
        mask.add_flag(ObjFlag::CLICKABLE);
        mask.add_style(&theme::dialog_mask(), StyleSelector::main());

        // ── 弹层主体：宽 720，高按内容自适应（下限 420）──
        let rows = spec.details.len().min(DETAILS_MAX_ROWS);
        let details_h = rows as i32 * Dimens::DIALOG_ROW_H;
        let warn_h = if level.requires_warn_banner() {
            Dimens::GAP_MIN + Dimens::BANNER_H
        } else {
            0
        };
        let panel_h = (DETAILS_TOP
            + details_h
            + warn_h
            + Dimens::GAP_SECTION
            + Dimens::BTN_H_PRIMARY
            + Dimens::DIALOG_PAD)
            .max(Dimens::DIALOG_MIN_H);
        let panel = Obj::create(&mask)?;
        panel.set_size(Dimens::DIALOG_W, panel_h);
        panel.add_style(&theme::dialog_panel(level), StyleSelector::main());
        panel.center();

        let x = Dimens::DIALOG_PAD;
        let inner_w = Dimens::DIALOG_W - 2 * Dimens::DIALOG_PAD;

        // 顶部级别色条（4 px，与弹层上缘齐平 —— 弹层不带内边距，见 `theme::dialog_panel`）。
        let _bar = decor(&panel, Dimens::DIALOG_W, BAR_H, &theme::dialog_level_bar(level))?;

        // 标题。
        let title = text_label(&panel, spec.title, TextSlot::PageTitle, Palette::TEXT_PRIMARY)?;
        title.set_size(inner_w, TITLE_H);
        title.set_pos(x, TITLE_Y);

        // 分隔线。
        let divider = decor(&panel, inner_w, theme::Stroke::THIN, &theme::card_head_bar(Palette::DIVIDER))?;
        divider.set_pos(x, DIVIDER_Y);

        // 「影响范围」段。
        let impact_head =
            text_label(&panel, TEXT_IMPACT_HEADING, TextSlot::Label, Palette::TEXT_PRIMARY)?;
        impact_head.set_pos(x, IMPACT_HEAD_Y);
        let impact = text_label(&panel, spec.impact, TextSlot::Body, Palette::TEXT_SECOND)?;
        impact.set_size(inner_w, IMPACT_H);
        // ── 溢出保护：**定尺寸 + 截断**（`DOTS`），不用 `WRAP` ──────────────────
        // `WRAP` 的语义是「保宽换行、**高度自适应**」（`vendor/lvgl/src/widgets/label/
        // lv_label.h:50`），在 `lv_obj_set_size` 给定显式高度时仍需由 label 自身把文本
        // 折到固定的 2 行盒内才安全；而 `DOTS` 的语义直接是「**Keep the size** and write
        // dots at the end if the text is too long」（同文件 :51），即：文本在
        // `IMPACT_H`（= 2 行）内换行、放不下则**原地截断并以 `…` 收尾**
        //（`lv_label.c:1314` 的 DOTS 分支按可用高度回退到「最后一行 + 点号」）。
        // 这样 `_impact` 的高度恒为 `IMPACT_H`，不会把后续「将修改的字段」段标题压下去。
        // 省略号用的是 `.`（UI §3.6 声明符号集内），不会出豆腐块。
        impact.set_long_mode(LongMode::DOTS);
        impact.set_pos(x, IMPACT_Y);

        // 「将修改的字段」+ 明细列表。
        let detail_head =
            text_label(&panel, TEXT_DETAILS_HEADING, TextSlot::Label, Palette::TEXT_PRIMARY)?;
        detail_head.set_pos(x, DETAILS_HEAD_Y);
        let list = ScrollContainer::create(&panel)?;
        list.set_size(inner_w, details_h.max(Dimens::DIALOG_ROW_H));
        list.set_pos(x, DETAILS_TOP);
        list.add_style(&theme::transparent(), StyleSelector::main());
        let field_w = Dimens::DIALOG_ROW_H * 5;
        let value_w = Dimens::TOUCH_MIN * 3 - Dimens::GAP_MIN;
        let arrow_w = Dimens::DIALOG_ROW_H;
        // 行标签句柄必须存住（`_detail_labels`）：它们是 `list` 的子对象，构造器返回时
        // 若被 `Drop`，`lv_obj_delete` 会把每一行的标签从列表里删掉（列表明细整段消失）。
        let mut detail_labels: Vec<Label> = Vec::with_capacity(DETAILS_MAX_ROWS * 4);
        for (i, d) in spec.details.iter().take(DETAILS_MAX_ROWS).enumerate() {
            let y = i as i32 * Dimens::DIALOG_ROW_H;
            let f = text_label(&list, d.field, TextSlot::Body, Palette::TEXT_SECOND)?;
            f.set_size(field_w, TextSlot::Body.px() as i32);
            f.set_long_mode(LongMode::DOTS);
            f.set_pos(0, y);
            let b = text_label(&list, d.before, TextSlot::Body, Palette::TEXT_WEAK)?;
            b.set_size(value_w, TextSlot::Body.px() as i32);
            b.set_long_mode(LongMode::DOTS);
            b.set_pos(field_w, y);
            let a = text_label(&list, TEXT_ARROW, TextSlot::Body, Palette::TEXT_WEAK)?;
            a.set_size(arrow_w, TextSlot::Body.px() as i32);
            a.set_pos(field_w + value_w, y);
            let n = text_label(&list, d.after, TextSlot::Body, Palette::OK)?;
            n.set_size(value_w, TextSlot::Body.px() as i32);
            n.set_long_mode(LongMode::DOTS);
            n.set_pos(field_w + value_w + arrow_w, y);
            detail_labels.extend([f, b, a, n]);
        }

        // WarnBanner —— **L2+ 强制**（与"调用方是否记得传"无关，见 `ConfirmLevel`）。
        let warn = if level.requires_warn_banner() {
            let b = WarnBanner::new(&panel, inner_w, TEXT_WARN_BANNER, spec.warn_fields)?;
            b.set_pos(x, DETAILS_TOP + details_h + Dimens::GAP_MIN);
            Some(b)
        } else {
            None
        };

        // ── 按钮行（右侧对齐；危险级别与「取消」间距 ≥ 48 px，UI §2.1）──
        let btn_y = panel_h - Dimens::DIALOG_PAD - Dimens::BTN_H_PRIMARY;
        let gap = if level.is_dangerous() {
            Dimens::GAP_DANGER
        } else {
            Dimens::GAP_MIN
        };
        let confirm_x = Dimens::DIALOG_W - Dimens::DIALOG_PAD - Dimens::BTN_MAIN_W;
        let cancel_x = confirm_x - gap - Dimens::BTN_MAIN_W;

        let cancel = Rc::new(TextButton::create(&panel, TEXT_CANCEL)?);
        cancel.set_size(Dimens::BTN_MAIN_W, Dimens::BTN_H_PRIMARY);
        cancel.set_pos(cancel_x, btn_y);
        cancel.label().center();
        theme::button(theme::ButtonKind::Secondary).apply(&cancel);

        let confirm_kind = if level.is_dangerous() {
            theme::ButtonKind::Danger
        } else {
            theme::ButtonKind::Primary
        };
        let confirm = Rc::new(TextButton::create(&panel, level.confirm_text())?);
        confirm.set_size(Dimens::BTN_MAIN_W, Dimens::BTN_H_PRIMARY);
        confirm.set_pos(confirm_x, btn_y);
        confirm.label().center();
        theme::button(confirm_kind).apply(&confirm);

        let on_confirm: NotifyCallback = Rc::new(RefCell::new(None));
        let on_cancel: NotifyCallback = Rc::new(RefCell::new(None));
        let debounce = Rc::new(Debounce::new(theme::Timing::debounce()));

        // ── 长按进度条（仅危险级别；值驱动，非动画）──
        let progress = if level.requires_long_press() {
            let bar = Bar::create(confirm.button().obj())?;
            // **填满按钮**（UI §7.3「按住确认」的进度反馈要覆盖整个按钮，不是内容区）。
            //
            // 进度条是按钮的**子对象**，而子对象坐标相对父的**内容区**（= 外缘 + 描边 +
            // 内边距）：危险按钮由 `theme::button(Danger)` 带 `Stroke::DANGER`(2) 描边 +
            // `GAP_MIN`(16) 内边距 ⇒ 内容区原点相对外缘内缩 18 px。故这里反向偏移
            // `描边 + 内边距`，再给足整个按钮的尺寸，`coords()` 才与按钮**外缘**重合
            //（此前按内容区写尺寸 ⇒ 内缩 18 px、右/下各被裁 2 px，评审 Important 1）。
            // `progress` 仅存在于危险级别（`requires_long_press`），故描边取 Danger 的 2 px。
            let inset = Dimens::GAP_MIN + theme::Stroke::DANGER;
            bar.set_size(Dimens::BTN_MAIN_W, Dimens::BTN_H_PRIMARY);
            bar.set_pos(-inset, -inset);
            bar.set_range(0, 100);
            bar.set_value(0, Anim::OFF);
            bar.remove_flag(ObjFlag::CLICKABLE);
            bar.remove_flag(ObjFlag::SCROLLABLE);
            bar.add_style(&theme::progress_track(), StyleSelector::main());
            bar.add_style(&theme::progress_indicator(), StyleSelector::part_of(Part::INDICATOR));
            bar.set_hidden(true);
            Some(Rc::new(bar))
        } else {
            None
        };

        // 默认焦点「取消」（TT-09）：入组 + 聚焦 ⇒ `LV_STATE_FOCUSED`。
        let group = Group::create()?;
        group.add(&cancel);
        group.focus(&cancel);

        // ── 事件装配 ──
        {
            let cb = on_cancel.clone();
            cancel.on_clicked(move |_| fire_unit(&cb));
        }
        if level.requires_long_press() {
            // 按下：进度归零、显形。
            {
                let prog = progress.clone();
                confirm.on(EventCode::PRESSED, move |_| {
                    if let Some(p) = prog.as_ref() {
                        p.set_value(0, Anim::OFF);
                        p.set_hidden(false);
                    }
                });
            }
            // 松手 / 按下丢失：归零、隐藏。**未满 1.0 s 即自然取消** —— LVGL 不派发
            // `LONG_PRESSED`，故"中途松手即取消"由框架保证，我们无需自己计时。
            for code in [EventCode::RELEASED, EventCode::PRESS_LOST] {
                let prog = progress.clone();
                confirm.on(code, move |_| {
                    if let Some(p) = prog.as_ref() {
                        p.set_value(0, Anim::OFF);
                        p.set_hidden(true);
                    }
                });
            }
            // 满 1.0 s：满格 → 防重判定 → 发通知 → 禁用视觉反馈（TT-10）。
            {
                let cb = on_confirm.clone();
                let db = debounce.clone();
                let prog = progress.clone();
                let weak = Rc::downgrade(&confirm);
                confirm.on(EventCode::LONG_PRESSED, move |_| {
                    if let Some(p) = prog.as_ref() {
                        p.set_value(100, Anim::OFF);
                    }
                    if !db.try_accept(Instant::now()) {
                        return;
                    }
                    if let Some(c) = weak.upgrade() {
                        c.set_disabled(true);
                    }
                    fire_unit(&cb);
                });
            }
        } else {
            // L1：单击生效（仍走防重，防"连点发两次请求"）。
            let cb = on_confirm.clone();
            let db = debounce.clone();
            let weak = Rc::downgrade(&confirm);
            confirm.on_clicked(move |_| {
                if !db.try_accept(Instant::now()) {
                    return;
                }
                if let Some(c) = weak.upgrade() {
                    c.set_disabled(true);
                }
                fire_unit(&cb);
            });
        }

        Ok(Self {
            mask,
            panel,
            level,
            _level_bar: _bar,
            title,
            _divider: divider,
            _impact_head: impact_head,
            _impact: impact,
            _detail_head: detail_head,
            _list: list,
            _detail_labels: detail_labels,
            cancel,
            confirm,
            progress,
            warn,
            group,
            on_confirm,
            on_cancel,
            debounce,
        })
    }

    /// 遮罩对象（全屏、可点、**点击不关闭**）。
    pub fn mask(&self) -> &Obj {
        &self.mask
    }

    /// 弹层主体对象。
    pub fn panel(&self) -> &Obj {
        &self.panel
    }

    /// 标题标签（离屏读回入口：仍在树上 / 文本可读回）。
    pub fn title(&self) -> &Label {
        &self.title
    }

    /// 「影响范围」段正文标签（离屏读回入口；`coords()` 用于断言"固定 2 行盒内不溢出"
    /// 的溢出保护，见该标签的 `DOTS` 长模式说明）。
    pub fn impact(&self) -> &Label {
        &self._impact
    }

    /// 长按进度条（若有；`coords()` 用于断言"填满确认按钮"，见 [`ConfirmDialog::new`]）。
    pub fn progress_bar(&self) -> Option<&Bar> {
        self.progress.as_deref()
    }

    /// 顶部级别色条（离屏读回入口：仍在树上）。
    pub fn level_bar(&self) -> &Obj {
        &self._level_bar
    }

    /// 标题下分隔线（离屏读回入口：仍在树上）。
    pub fn divider(&self) -> &Obj {
        &self._divider
    }

    /// 构造时给定的强度级别。
    pub fn level(&self) -> ConfirmLevel {
        self.level
    }

    /// 是否出现 `WarnBanner`（`L2+` 恒为 `true` —— 结构性，不依赖调用方）。
    pub fn has_warn_banner(&self) -> bool {
        self.warn.is_some()
    }

    /// `WarnBanner`（若有）。
    pub fn warn_banner(&self) -> Option<&WarnBanner> {
        self.warn.as_ref()
    }

    /// 是否出现长按进度条（`L2` / `L2+` 恒为 `true`）。
    pub fn has_progress(&self) -> bool {
        self.progress.is_some()
    }

    /// 长按进度条当前值（0–100；无进度条时为 `None`）。
    pub fn progress_value(&self) -> Option<i32> {
        self.progress.as_ref().map(|p| p.value())
    }

    /// 推进长按进度（0–100，越界 clamp）。由事件循环按 tick 调用。
    ///
    /// **不做循环动画**（设计 §5.6 动效纪律）：这里只写值，平滑程度由调用方的 tick 频率决定。
    pub fn set_progress(&self, percent: i32) {
        if let Some(p) = self.progress.as_ref() {
            let v = percent.clamp(0, 100);
            p.set_value(v, Anim::OFF);
            p.set_hidden(v == 0);
        }
    }

    /// 「取消」按钮当前是否处于默认焦点（TT-09 的**可断言**落点）。
    pub fn default_focus_is_cancel(&self) -> bool {
        widgets::has_state(&self.cancel, State::FOCUSED)
    }

    /// 「取消」按钮。
    pub fn cancel_button(&self) -> &TextButton {
        &self.cancel
    }

    /// 确认按钮（危险级别下其内叠加长按进度条）。
    pub fn confirm_button(&self) -> &TextButton {
        &self.confirm
    }

    /// 确认按钮当前是否禁用（TT-10 提交后 `disabled`）。
    pub fn confirm_disabled(&self) -> bool {
        widgets::has_state(&self.confirm, State::DISABLED)
    }

    /// 防重窗口（供调用方断言 / 重置）。
    pub fn debounce(&self) -> &Debounce {
        &self.debounce
    }

    /// 注册"确认完成"回调（**只发信号**：L1 单击生效 / L2·L2+ 长按满 1.0 s 时调用一次）。
    /// 弹层的关闭由调用方在 tick 内 [`ConfirmDialog::close`]。
    pub fn set_on_confirm<F>(&self, f: F)
    where
        F: FnMut() + 'static,
    {
        *self.on_confirm.borrow_mut() = Some(Box::new(f));
    }

    /// 注册"取消"回调（同上，只发信号）。
    pub fn set_on_cancel<F>(&self, f: F)
    where
        F: FnMut() + 'static,
    {
        *self.on_cancel.borrow_mut() = Some(Box::new(f));
    }

    /// 关闭并销毁弹层（含遮罩）。**禁止在 LVGL 事件回调内调用**（见模块文档）。
    pub fn close(self) {
        // 先摘掉输入组的焦点，再删对象树（`Group` 的 `Drop` 也会做同样的事，此处显式化
        // 只为让"先解绑焦点、再删对象"的顺序在代码里可见）。
        self.group.remove(&self.cancel);
        self.group.remove(&self.confirm);
        self.mask.delete();
    }

    /// 弹层是否仍存活。
    pub fn is_alive(&self) -> bool {
        self.panel.is_alive()
    }
}

impl std::fmt::Debug for ConfirmDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfirmDialog")
            .field("panel", &self.panel)
            .field("level", &self.level)
            .field("has_warn_banner", &self.has_warn_banner())
            .field("has_progress", &self.has_progress())
            .field("default_focus_is_cancel", &self.default_focus_is_cancel())
            .finish()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Toast —— 结果提示条
// ═══════════════════════════════════════════════════════════════════════════

/// Toast 语义（UI §7.2：成功 / 失败 / 警示；另加信息类供后续使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToastTone {
    /// 成功（`#2FDB8A`）。
    Success,
    /// 失败（`#FF6B6B`）。
    Failure,
    /// 警示（`#FFB020`）。
    Warning,
    /// 信息（`#4EA6FF`）。
    Info,
}

impl ToastTone {
    /// 左缘色条色（UI §7.2）。
    pub const fn accent(self) -> Color {
        match self {
            ToastTone::Success => Palette::OK,
            ToastTone::Failure => Palette::DANGER,
            ToastTone::Warning => Palette::STALE,
            ToastTone::Info => Palette::INFO,
        }
    }
}

/// Toast 提示条（UI §5.1 #15 / §7.2：≤480×60、左缘 4 px 色条、3 s 自动消失、同时刻仅 1 条）。
///
/// **3 s 计时归调用方**（事件循环）：本组件只记录 `expires_at` 并提供 [`Toast::is_expired`]，
/// 不做循环定时器（设计 §5.6 动效纪律；且薄层没有定时器封装）。时钟经 [`Toast::new_at`]
/// **注入**，可确定性单测。
#[derive(Debug)]
pub struct Toast {
    obj: Obj,
    /// 左缘色条（4 px）。**仅为保持子树存活**：它是 `obj` 的子对象，且是独立装饰块，
    /// 若句柄在构造器返回时被 `Drop`，色条会立即从树上消失（连"仍有子对象"都没有 ⇒
    /// 更隐蔽）。故存进结构体（离屏读回入口见 [`Toast::accent_bar`]）。
    _accent: Obj,
    icon: Rc<Label>,
    text: Rc<Label>,
    tone: ToastTone,
    expires_at: Instant,
}

impl Toast {
    /// 以"现在"为起点建 Toast（[`theme::Timing::TOAST_MS`] 后过期）。
    pub fn new(parent: &Obj, tone: ToastTone, icon: &str, text: &str) -> Result<Self, LvglError> {
        Self::new_at(parent, tone, icon, text, Instant::now())
    }

    /// 以**注入的时刻**为起点建 Toast（单测用）。
    pub fn new_at(
        parent: &Obj,
        tone: ToastTone,
        icon: &str,
        text: &str,
        now: Instant,
    ) -> Result<Self, LvglError> {
        let obj = Obj::create(parent)?;
        obj.set_size(Dimens::TOAST_W, Dimens::TOAST_H);
        obj.set_pos(Dimens::TOAST_X, Dimens::TOAST_Y);
        obj.add_style(&theme::toast(), StyleSelector::main());
        obj.remove_flag(ObjFlag::SCROLLABLE);

        let _accent = decor(
            &obj,
            Dimens::TOAST_ACCENT_W,
            Dimens::TOAST_H,
            &theme::toast_accent(tone.accent()),
        )?;
        // **左缘色条贴 Toast 外缘、通高**（UI §7.2「左缘 4 px 色条」）。
        //
        // 子对象坐标相对父**内容区** = 外缘 + 描边 + 内边距；`theme::toast` 已把内边距
        // 置 0（与 `warn_banner` / `dialog_panel` 同口径），故只剩 1 px 描边要反向抵消。
        // 此前 `theme::toast` 带 `GAP_MIN` 内边距而色条仍按外缘写坐标 ⇒ 实测
        // `accent.x1 = toast.x1 + 17`（16 内边距 + 1 描边）、可视高被裁到 28 px
        //（评审 Important 1）。
        _accent.set_pos(-theme::Stroke::THIN, -theme::Stroke::THIN);

        let icon_l = text_label(&obj, icon, TextSlot::SectionTitle, tone.accent())?;
        icon_l.set_size(Dimens::ICON_SM, Dimens::ICON_SM);
        icon_l.set_pos(
            Dimens::TOAST_ACCENT_W + Dimens::GAP_MIN,
            theme::center_offset(Dimens::TOAST_H, Dimens::ICON_SM),
        );
        let text_l = text_label(&obj, text, TextSlot::Body, Palette::TEXT_PRIMARY)?;
        text_l.set_size(
            Dimens::TOAST_W - Dimens::TOAST_ACCENT_W - 3 * Dimens::GAP_MIN - Dimens::ICON_SM,
            TextSlot::Body.px() as i32,
        );
        text_l.set_long_mode(LongMode::DOTS);
        text_l.set_pos(
            Dimens::TOAST_ACCENT_W + Dimens::GAP_MIN + Dimens::ICON_SM + Dimens::GAP_MIN,
            theme::center_offset(Dimens::TOAST_H, TextSlot::Body.px() as i32),
        );

        Ok(Self {
            obj,
            _accent,
            icon: Rc::new(icon_l),
            text: Rc::new(text_l),
            tone,
            expires_at: now + theme::Timing::toast(),
        })
    }

    /// 底层对象。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 左缘色条（离屏读回入口：仍在树上）。
    pub fn accent_bar(&self) -> &Obj {
        &self._accent
    }

    /// 语义（左缘色条色由它决定）。
    pub fn tone(&self) -> ToastTone {
        self.tone
    }

    /// 图标字形。
    pub fn icon_text(&self) -> Option<String> {
        self.icon.text()
    }

    /// 文案。
    pub fn text(&self) -> Option<String> {
        self.text.text()
    }

    /// 改文案（同一 Toast 复用时就地更新）。
    pub fn set_text(&self, text: &str) {
        self.text.set_text(text);
    }

    /// 过期时刻。
    pub fn expires_at(&self) -> Instant {
        self.expires_at
    }

    /// 是否已到 3 s（调用方据此 [`Toast::close`]）。
    pub fn is_expired(&self, now: Instant) -> bool {
        now >= self.expires_at
    }

    /// 剩余存活时长（已过期时为 0）。
    pub fn remaining(&self, now: Instant) -> Duration {
        self.expires_at.saturating_duration_since(now)
    }

    /// 关闭并销毁。
    pub fn close(self) {
        self.obj.delete();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// EmptyState / UnavailableState —— 空态与不可用态（语义必须可区分）
// ═══════════════════════════════════════════════════════════════════════════

/// 状态语义标记（UI §8.3「唯一落点表」：语义不同的态**必须有独立落点**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateSemantics {
    /// 空态：**确实没有**（如「近 24 小时无告警」）。
    Empty,
    /// 不可用态：**无法获知**（如「告警源不可用」）。
    Unavailable,
}

/// 空态（UI §5.1 #17 / §8.3：居中，图标 64 + 文字 28）。
#[derive(Debug)]
pub struct EmptyState {
    obj: Obj,
    /// 图标行容器 —— `icon` 标签的父对象。**仅为保持子树存活**（否则 `icon_text()` 恒为 `None`）。
    _icon_row: Obj,
    /// 文字行容器 —— `text` 标签的父对象。**仅为保持子树存活**（否则 `text()` 恒为 `None`）。
    _text_row: Obj,
    icon: Rc<Label>,
    text: Rc<Label>,
}

impl EmptyState {
    /// 本类型的语义标记（恒为 [`StateSemantics::Empty`]）。
    pub const SEMANTICS: StateSemantics = StateSemantics::Empty;

    /// 建空态（`icon` 由调用方给几何字形 —— 不同空态可有不同图形）。
    pub fn new(parent: &Obj, icon: &str, text: &str) -> Result<Self, LvglError> {
        let text_row_h = TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
        let obj = layout_box(parent, Dimens::CONTENT_W, Dimens::ICON_LG + Dimens::GAP_MIN + text_row_h)?;

        // 水平居中用"等宽行容器 + `Obj::center`"两层结构（薄层没有对齐 API）。
        let icon_row = layout_box(&obj, Dimens::CONTENT_W, Dimens::ICON_LG)?;
        icon_row.set_pos(0, 0);
        let icon_l = Rc::new(text_label(
            &icon_row,
            icon,
            theme::icon_slot(Dimens::ICON_LG),
            Palette::TEXT_WEAK,
        )?);
        icon_l.set_size(Dimens::ICON_LG, Dimens::ICON_LG);
        icon_l.center();

        let text_row = layout_box(&obj, Dimens::CONTENT_W, text_row_h)?;
        text_row.set_pos(0, Dimens::ICON_LG + Dimens::GAP_MIN);
        let text_l = Rc::new(text_label(
            &text_row,
            text,
            TextSlot::SectionTitle,
            Palette::TEXT_WEAK,
        )?);
        text_l.center();

        Ok(Self {
            obj,
            _icon_row: icon_row,
            _text_row: text_row,
            icon: icon_l,
            text: text_l,
        })
    }

    /// 语义标记（断言口径）。
    pub fn semantics(&self) -> StateSemantics {
        Self::SEMANTICS
    }

    /// 底层对象。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 文案。
    pub fn text(&self) -> Option<String> {
        self.text.text()
    }

    /// 图标字形。
    pub fn icon_text(&self) -> Option<String> {
        self.icon.text()
    }
}

impl std::ops::Deref for EmptyState {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}

/// 不可用态的场景（UI §8.3 逐场景落点：EDGE-09 / EDGE-17 / F16.6）。
///
/// **标题与图标由枚举给定**：调用方**无法**把「不可用」写成「无 X」或「未联锁」——
/// 这正是设计 §5.6 / UI §8.3 要的"结构性区分"，而不是"约定不要写错"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnavailableKind {
    /// EDGE-09：告警源不可用（**不得**显「无告警」）。
    AlertSource,
    /// EDGE-17：审计源不可用（**不得**显「无审计记录」）。
    Audit,
    /// F16.6 / IL-01：联锁状态不可用（**不得**显「未联锁」，fail-closed）。
    Interlock,
}

impl UnavailableKind {
    /// 全部场景。
    pub const ALL: [UnavailableKind; 3] = [
        UnavailableKind::AlertSource,
        UnavailableKind::Audit,
        UnavailableKind::Interlock,
    ];

    /// 该场景的固定标题（UI §3.6 用字表逐字）。
    pub const fn title(self) -> &'static str {
        match self {
            UnavailableKind::AlertSource => "告警源不可用",
            UnavailableKind::Audit => "审计记录不可用",
            UnavailableKind::Interlock => "联锁状态不可用",
        }
    }

    /// 该场景的固定图标字形。
    ///
    /// UI §8.3 对联锁不可用要求"问号 / 断链几何图形，**不复用 ✓ 与 ⚠**"；用字表内没有断链
    /// 字形，故统一取 `?` —— **既不与空态图形相同，也不与警示图形相同**。
    pub const fn icon(self) -> &'static str {
        "?"
    }

    /// 该场景的灰化色（UI §6.4 / §8.3 均为 `#8C98AC`）。
    pub const fn accent(self) -> Color {
        Palette::STOPPED
    }

    /// 本态是否**等价于"确知无事"**——恒为 `false`。
    ///
    /// 这是给评审 / 测试的**结构性声明**：不可用态在任何场景下都**不得**被当作"确知安全 /
    /// 确知没有"（UI §8.3 联锁专行：「未联锁」= 确知安全、「不可用」= 无法获知）。
    pub const fn means_nothing_happened(self) -> bool {
        false
    }
}

/// 不可用态（UI §5.1 #18 / §8.3：居中，图标 64 + 标题 28 + 原因 24）。
#[derive(Debug)]
pub struct UnavailableState {
    obj: Obj,
    /// 图标行容器 —— `icon` 的父对象。**仅为保持子树存活**（否则 `icon_text()` 恒为 `None`）。
    _icon_row: Obj,
    /// 标题行容器 —— `title` 的父对象。**仅为保持子树存活**（否则 `title()` 恒为 `None`）。
    _title_row: Obj,
    /// 原因行容器 —— `reason` 的父对象。**仅为保持子树存活**（否则 `reason()` 恒为 `None`）。
    _reason_row: Obj,
    icon: Rc<Label>,
    title: Rc<Label>,
    reason: Rc<Label>,
    kind: UnavailableKind,
}

impl UnavailableState {
    /// 本类型的语义标记（恒为 [`StateSemantics::Unavailable`]）。
    pub const SEMANTICS: StateSemantics = StateSemantics::Unavailable;

    /// 按场景建不可用态（标题与图标不由调用方给 —— 见 [`UnavailableKind`]）。
    pub fn new(parent: &Obj, kind: UnavailableKind, reason: &str) -> Result<Self, LvglError> {
        let title_row_h = TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
        let reason_row_h = TextSlot::Body.px() as i32 + Dimens::GAP_MIN;
        let obj = layout_box(
            parent,
            Dimens::CONTENT_W,
            Dimens::ICON_LG + Dimens::GAP_MIN + title_row_h + reason_row_h,
        )?;

        let icon_row = layout_box(&obj, Dimens::CONTENT_W, Dimens::ICON_LG)?;
        icon_row.set_pos(0, 0);
        let icon_l = Rc::new(text_label(
            &icon_row,
            kind.icon(),
            theme::icon_slot(Dimens::ICON_LG),
            kind.accent(),
        )?);
        icon_l.set_size(Dimens::ICON_LG, Dimens::ICON_LG);
        icon_l.center();

        let title_row = layout_box(&obj, Dimens::CONTENT_W, title_row_h)?;
        title_row.set_pos(0, Dimens::ICON_LG + Dimens::GAP_MIN);
        let title_l = Rc::new(text_label(
            &title_row,
            kind.title(),
            TextSlot::SectionTitle,
            kind.accent(),
        )?);
        title_l.center();

        let reason_row = layout_box(&obj, Dimens::CONTENT_W, reason_row_h)?;
        reason_row.set_pos(0, Dimens::ICON_LG + Dimens::GAP_MIN + title_row_h);
        let reason_l = Rc::new(text_label(
            &reason_row,
            reason,
            TextSlot::Body,
            Palette::PLACEHOLDER,
        )?);
        reason_l.set_long_mode(LongMode::WRAP);
        reason_l.center();

        Ok(Self {
            obj,
            _icon_row: icon_row,
            _title_row: title_row,
            _reason_row: reason_row,
            icon: icon_l,
            title: title_l,
            reason: reason_l,
            kind,
        })
    }

    /// 语义标记（断言口径）。
    pub fn semantics(&self) -> StateSemantics {
        Self::SEMANTICS
    }

    /// 场景。
    pub fn kind(&self) -> UnavailableKind {
        self.kind
    }

    /// 底层对象。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 标题（**由 [`UnavailableKind`] 决定**，不是调用方给的）。
    pub fn title(&self) -> Option<String> {
        self.title.text()
    }

    /// 原因行。
    pub fn reason(&self) -> Option<String> {
        self.reason.text()
    }

    /// 图标字形。
    pub fn icon_text(&self) -> Option<String> {
        self.icon.text()
    }
}

impl std::ops::Deref for UnavailableState {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}
