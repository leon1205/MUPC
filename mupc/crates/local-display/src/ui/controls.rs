//! # `ui/controls.rs` —— **输入型**组合控件（12-MUPC 工作单元 **B2b-1**）
//!
//! 本文件交付 UI §5.1 控件总表里的三个**输入型**控件：
//!
//! | # | 控件 | LVGL 实现 | 设计出处 |
//! |---|------|-----------|----------|
//! | 4 | [`SegmentedControl`] | `lv_buttonmatrix`（`one_checked` 单选互斥） | UI §5.1 #4 / §5.2 / §5.3 |
//! | 7 | [`Ipv4Stepper`] | 4 × [`Stepper`] + 汇总 [`Label`] | UI §5.1 #7 / §5.3 |
//! | 8 | [`DateTimeStepper`] | 5 × [`Stepper`] + 5 个列头 [`Label`] | UI §5.1 #8 / §5.3 |
//!
//! ## 为什么与 `components.rs` 分列（职责边界）
//!
//! `components.rs`（B1）交付的是**展示 / 确认型**件：状态胶囊、指示灯、弹层、Toast、
//! 空态 / 不可用态 —— 它们的共同点是**只反映既有数据、不产生新数据**，其输出通道是
//! "视觉状态 + 一次性通知"（`set_on_confirm` / `set_on_cancel`）。
//!
//! 本文件交付的是**输入型**件：它们承载"用户选了什么 / 调到了多少"，是**草稿值的唯一
//! 生产者**，输出通道是"带载荷的变更回调"（`set_on_change` 携带 `usize` / `[u8; 4]` /
//! [`DateTimeValue`]）。两条轴的差别是**契约性**的（有无载荷、是否参与脏标记 / 提交链路），
//! 故**分列而不混装**：`components.rs` 不新增输入控件，本文件不新增展示控件。
//!
//! **不改动 `components.rs`**：本文件只 `use` 它的 [`Stepper`] 作为零件（UI §5.3
//! `Ipv4Stepper` / `DateTimeStepper` 两行原话：「4× `Stepper` 单元」「5× `Stepper` 列」）。
//! [`Stepper`] 的三个内部助手（`layout_box` / `text_label`）是**私有**的，故本文件
//! **复刻**了两个语义逐条一致的 8 行助手（与 `ui/pages/mod.rs` 的处置同法），不修改 B1。
//!
//! ## ⚠️ 已知偏差登记（**沿用 `ui/pages/mod.rs` 的口径，独立编号 `CD`** ——
//! `D*` 是页面层的编号，本表不与它冲突；本表只登记**本文件三个控件**的屏文 / 尺寸与
//! 契约不一致处）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | CD1 | **IPv4 四段总宽 = 792**（`4 × (64 + 64 + 64) + 3 × 8`），UI §5.1 #7 正文写 **856** | **文档自身矛盾**：856 **无法**由其同一行的分项算出（该行的分项按自身口径 = 792）。按 theme 分项常量推导为准（`Dimens::STEPPER_BTN_W × 2 + Dimens::IPV4_VALUE_W`），**不硬编码 856 或 792** | 无（**有意与 theme 分项一致**；建议 UI §5.1 #7 回改 856 → 792） |
//! | CD2 | IPv4 汇总标签宽 = **184**（`CONTENT_W − 792 − GAP_MIN`）⇒ 整件 **992 = 内容区有效宽**，UI §6.1 行型 B 写「控件独占次行 (x36–x988)，用 `Ipv4Stepper` 856×64」 | 856 − 792 = 64 px **放不下** `192.168.1.10`（12 字符）；§6.1 要求控件独占次行（有效宽 992）⇒ 取"四段 + 缝 + 汇总 = 内容区宽" | **B2b-2**（P2 装配时若 PM 要求 x36 起排，需重定汇总标签的位置口径） |
//! | CD3 | **`Dimens::DATETIME_COL_W`（112）在本文件不使用**：日期时间列宽取 **192**（`64 + 64 + 64`），值区宽取 **64**（内容区均分推导） | 112 **不可用**（两重）：① 若作**列宽** ⇒ 列宽下限 = `−`64 + 值 48(TOUCH_MIN) + `+`64 = **176 > 112**（触摸硬约束）；② 若作**值区宽** ⇒ 五列 = `5 × (64+112+64) + 4×8` = **1232**，既超内容区 992、也超屏幕 1024。⇒ 按「内容区有效宽均分五列」推导值区宽 | 建议 UI §5.1 #8 与本文件同法回改（112 → 64，592 → 992） |
//! | CD4 | DateTimeStepper 总高 = **106**（列头 26 + 缝 16 + 步进 64），UI §5.1 #8 写 **64** | 表内 64 **未计入列头**，而同表的文字要求「每列需有列头标签（年 / 月 / 日 / 时 / 分）」⇒ 高度必然 > 64。列头高度取 [`TextSlot::Label`]（26 px，§3.3 控件文字档） | **B2b-2**：P3 的「自定义」展开区（UI 写 Y700 起、64 px）随之增高，装配时按 106 预留 |
//! | CD5 | ~~分段控件的「选中」与「禁用」两条样式挂上但当前不参与绘制~~ **（已在本次提交 `fix(display): 薄层补 buttonmatrix 控制位接口` 修复 ⇒ 四态全部生效）** | **原根因**（v9.5.0 源码事实）：`lv_buttonmatrix` 的每段 `CHECKED` / `DISABLED` 状态由 `ctrl_bits[i]`（`LV_BUTTONMATRIX_CTRL_CHECKED` / `_DISABLED`）驱动（`lv_buttonmatrix.c` 绘制趟 `btn_state` 只由 ctrl 位与 `btn_id_sel` 组装），而**当时薄层 `ButtonMatrix` 未暴露** `lv_buttonmatrix_set_button_ctrl(_all)`；`set_one_checked(true)` 只置"互斥"标志（其内部 `make_one_button_checked` 只在按钮**已** CHECKED 时才保留），故 CHECKABLE 无法置位 ⇒ 用户点选只改变 `btn_id_sel`（PRESSED 高亮可见），CHECKED 从不产生 | **修复方式 / 验证**：薄层补 `set_ctrl_all` / `set_ctrl` / `clear_ctrl_all` / `has_ctrl` 四个薄封装（`src/lvgl/widgets.rs`），本控件改为构造期 `set_ctrl_all(CTRL_CHECKABLE)` + 初始段 `set_ctrl(start, CTRL_CHECKED)`，[`SegmentedControl::set_selected`] 改为"全清 CHECKED → 单段置位"，[`SegmentedControl::set_disabled`] 同步 `CTRL_DISABLED` 位；**回归锁**在 `ui/tests.rs::ui_chain` —— 以 `has_ctrl` 读回断言"每段 CHECKABLE / 恰一段 CHECKED / 切换后旧段已清"（注释掉 `set_ctrl_all(CTRL_CHECKABLE)` 那一行，该用例即变红，已实测） |
//! | CD6 | 列头 / 汇总标签的**文本对齐**靠"窄标签 + [`theme::center_offset`] 定位"实现；IPv4 汇总标签在其 184 px 盒内**左对齐** | 薄层**没有**文本对齐通道（`lv_obj_set_style_text_align` 未封装，见 `components.rs` 模块文档「布局手法」） | 若 PM 要求汇总标签居中 / 右对齐：薄层补 `set_text_align` 后调整 |
//!
//! ## 薄层缺能力（如实标注，**未**在本单元处理）
//!
//! 1. ~~**每段 ctrl 位不可置位** ⇒ CD5（分段控件的选中态视觉不可达）~~ **已消除**：
//!    `ButtonMatrix` 现有 `set_ctrl_all` / `set_ctrl` / `clear_ctrl_all` / `has_ctrl`
//!    （CD5 修复）；
//! 2. **没有 `child(i)` 遍历口**（`Obj` 只有 `child_count()`）⇒ 离屏用例**无法逐对象统计
//!    "可点对象数"**，只能断 `child_count()` 链路（本次用例的做法，见 `ui/tests.rs`）；
//! 3. **没有样式读回 API** ⇒ "某样式确已挂上"只能以**选择器真源**（[`ITEM_STATES`]）为据
//!    （与 `ButtonStyles::entries` 同法）；
//! 4. **没有文本度量 API** ⇒ 汇总标签的 184 px 是否**足够放下** `192.168.1.10`（估算约
//!    154 px）**未能验证**：`LongMode::DOTS` 只影响**绘制**，[`Ipv4Stepper::text`] 仍返回
//!    完整串 ⇒ 离屏断言结构上抓不到"可视截断"；
//! 5. **没有"给单个对象派发事件"以外的驱动口** ⇒ `Stepper` 的 `−` / `＋` 点击闭包挂在
//!    **子按钮**上，而 `Obj::send_event` **只向本对象派发并向父链冒泡**（不下行）⇒
//!    [`Ipv4Stepper`] / [`DateTimeStepper`] 的 `set_on_change` **无法被离屏用例驱动**
//!    （`Stepper::set_value` 按设计**不**触发 `on_change`）。
//!
//! ## 纪律（逐条对应设计要求）
//!
//! - **零文本输入**（红线）：本文件只出现 `lv_button` / `lv_label` / `lv_buttonmatrix`；
//!   `lv_keyboard` / `lv_textarea` / `lv_spinbox` **零出现**（`ui/tests.rs` 静态扫描）；
//! - **无裸色值 / 裸尺寸**：一切外观数值经 [`theme`] 取用；本文件专属的栅格常量集中在
//!   下方 `const` 块，**逐条注明出处**（theme 常量推导 / UI 条款）；
//! - **所有权纪律**：凡 `Obj::create` / `ButtonMatrix::create` / `Label::create_with_text`
//!   返回的**拥有型句柄**一律存进结构体字段（或作为 `Rc<Stepper>` 的字段），**绝不**只作
//!   局部变量 —— 否则构造器返回时即被 `Drop`，LVGL 级联删除其整棵子树（表现为"界面空白
//!   但无报错"，B1 出过此类 UAF 级缺陷）；
//! - **回调纪律**：回调内**不 panic**（无 `unwrap` / 无越界索引 / 无 `panic!`）、不做阻塞
//!   I/O、不删除自身宿主；共享态一律 `try_borrow*`（拿不到即跳过）；
//! - **不提供跨线程 API**：三个类型都含 LVGL 句柄（自动 `!Send` / `!Sync`），全部调用必须
//!   在事件循环线程内（设计 §5.2 不变量 4）。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::lvgl::event::EventCode;
use crate::lvgl::obj::{Obj, ObjFlag};
use crate::lvgl::style::{Color, Part, State, StyleSelector};
use crate::lvgl::widgets::{
    self, ButtonMatrix, Label, LongMode, CTRL_CHECKABLE, CTRL_CHECKED, CTRL_DISABLED,
};
use crate::lvgl::LvglError;
use crate::ui::components::Stepper;
use crate::ui::theme::{self, Dimens, Palette, TextSlot};

// ── 回调槽类型别名（三处 `Rc<RefCell<Option<Box<dyn FnMut(..)>>>>` 的语义化命名）──
//
// 与 `components.rs` 同法：三个别名形状相同、**载荷不同**，分别命名（读签名即知回调会收到
// 什么）。三者都**不出现在任何 `pub` 签名里**（对外入口一律是泛型 `set_on_change<F>`），
// 故不导出。

/// 选中下标回调槽：`Rc<RefCell<Option<Box<dyn FnMut(usize)>>>>`。
type IndexCallback = Rc<RefCell<Option<Box<dyn FnMut(usize)>>>>;
/// 四段 IPv4 值回调槽：`Rc<RefCell<Option<Box<dyn FnMut([u8; 4])>>>>`。
type OctetsCallback = Rc<RefCell<Option<Box<dyn FnMut([u8; 4])>>>>;
/// 日期时间值回调槽：`Rc<RefCell<Option<Box<dyn FnMut(DateTimeValue)>>>>`。
type DateTimeCallback = Rc<RefCell<Option<Box<dyn FnMut(DateTimeValue)>>>>;

// ═══════════════════════════════════════════════════════════════════════════
// 1. 屏上文案（逐字取自 UI §3.6 全屏用字表；**落笔前已在 `fonts/lv_font_cmap.txt`
//    逐字核对**：U+5E74 / U+6708 / U+65E5 / U+65F6 / U+5206 均在 324 码位基线内）
// ═══════════════════════════════════════════════════════════════════════════

/// 列头「年」（UI §5.1 #8 五列之一）。
pub const TEXT_YEAR: &str = "年";
/// 列头「月」（UI §5.1 #8）。
pub const TEXT_MONTH: &str = "月";
/// 列头「日」（UI §5.1 #8）。
pub const TEXT_DAY: &str = "日";
/// 列头「时」（UI §5.1 #8）。
pub const TEXT_HOUR: &str = "时";
/// 列头「分」（UI §5.1 #8）。
pub const TEXT_MINUTE: &str = "分";

/// 日期时间的**列头顺序真源**（年 → 月 → 日 → 时 → 分）—— 构造、[`DateTimeStepper::column_headers`]
/// 与离屏用例**共用同一份**，避免"列头顺序"出现第二个真源。
pub const DATETIME_HEADERS: [&str; 5] = [
    TEXT_YEAR,
    TEXT_MONTH,
    TEXT_DAY,
    TEXT_HOUR,
    TEXT_MINUTE,
];

/// IPv4 四段之间的分隔符（`.`, U+002E —— 在 cmap 内；**不是** cmap 外的 `:`）。
const IPV4_SEP: char = '.';

/// 本文件上屏的全部固定文案（供 `ui/tests.rs` 的**码表覆盖率**走查 —— UI §3.6 是全屏用字
/// 表，漏字即"豆腐块"，设计 §11.1 有同款测试）。
pub const ALL_TEXTS: &[&str] = &[TEXT_YEAR, TEXT_MONTH, TEXT_DAY, TEXT_HOUR, TEXT_MINUTE];

// ═══════════════════════════════════════════════════════════════════════════
// 2. 取值区间（**分量封闭** —— UI §5.3 三行；值域出处逐条注明）
// ═══════════════════════════════════════════════════════════════════════════

/// IPv4 单段下界（协议事实：八位组 0）。
pub const OCTET_MIN: i64 = 0;
/// IPv4 单段上界（协议事实：八位组 255）。
pub const OCTET_MAX: i64 = 255;

/// 年份下界（UI §5.3 `DateTimeStepper` 行 / 本单元规格「年 1970–2100」）。
pub const YEAR_MIN: i64 = 1970;
/// 年份上界（同上）。
pub const YEAR_MAX: i64 = 2100;
/// 月份下界（UI §5.3）。
pub const MONTH_MIN: i64 = 1;
/// 月份上界（UI §5.3）。
pub const MONTH_MAX: i64 = 12;
/// 日下界（UI §5.3；**分量封闭** = 1–31，不做日历校验）。
pub const DAY_MIN: i64 = 1;
/// 日上界（UI §5.3；**2 月 31 日由上层拒绝**，见 [`DateTimeValue`] 的类型文档）。
pub const DAY_MAX: i64 = 31;
/// 小时下界（UI §5.3）。
pub const HOUR_MIN: i64 = 0;
/// 小时上界（UI §5.3）。
pub const HOUR_MAX: i64 = 23;
/// 分钟下界（UI §5.3）。
pub const MINUTE_MIN: i64 = 0;
/// 分钟上界（UI §5.3）。
pub const MINUTE_MAX: i64 = 59;

/// 步进器的步长（各分量均为 1 —— 语义值，不是设计栅格值，故不走 `theme`）。
const STEP: i64 = 1;

/// 无 panic 的整数夹取（`i64::clamp` 在 `min > max` 时会 panic；本文件的两个端点永远是
/// 编译期常量且 `min ≤ max`，但仍不引入那条 panic 路径）。
const fn clamp_i64(v: i64, lo: i64, hi: i64) -> i64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 单个八位组 → `u8`（越界**夹取**，不 panic）。
const fn octet_of(v: i64) -> u8 {
    clamp_i64(v, OCTET_MIN, OCTET_MAX) as u8
}

/// 四段 → 汇总文本（`192.168.1.10`）。
///
/// 定成自由函数（而不是内联在构造器里）是为了让**纯逻辑**用例能直接核对它的产出形状
/// （含"分隔符在 cmap 内"这一条）。
fn ipv4_text(octets: [u8; 4]) -> String {
    // 固定长度数组的解构是**编译期**越界安全（无运行时 panic 路径）。
    let [a, b, c, d] = octets;
    let sep = IPV4_SEP;
    format!("{a}{sep}{b}{sep}{c}{sep}{d}")
}

/// 分段控件选中下标的**夹取**（`count == 0` 时恒 0 —— 构造器已拒绝空选项，此分支不可达，
/// 但仍不留 panic 路径）。
fn clamp_index(selected: usize, count: usize) -> usize {
    if count == 0 {
        0
    } else {
        selected.min(count - 1)
    }
}

/// 静默触发"选中下标"回调（拿不到借用即跳过，**不 panic**）。
fn fire_index(slot: &IndexCallback, v: usize) {
    if let Ok(mut s) = slot.try_borrow_mut() {
        if let Some(f) = s.as_mut() {
            f(v);
        }
    }
}

/// 静默触发"四段 IPv4"回调。
fn fire_octets(slot: &OctetsCallback, v: [u8; 4]) {
    if let Ok(mut s) = slot.try_borrow_mut() {
        if let Some(f) = s.as_mut() {
            f(v);
        }
    }
}

/// 静默触发"日期时间"回调。
fn fire_datetime(slot: &DateTimeCallback, v: DateTimeValue) {
    if let Ok(mut s) = slot.try_borrow_mut() {
        if let Some(f) = s.as_mut() {
            f(v);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 私有助手（**复刻** `components.rs` 的同名 / 同语义助手 —— 见模块文档"为什么与
//    components.rs 分列"；本文件不改动 B1 交付物）
// ═══════════════════════════════════════════════════════════════════════════

/// 建一个透明布局容器（行 / 列 / 居中壳）。
fn layout_box(parent: &Obj, w: i32, h: i32) -> Result<Obj, LvglError> {
    let o = Obj::create(parent)?;
    o.set_size(w, h);
    o.add_style(&theme::transparent(), StyleSelector::main());
    o.remove_flag(ObjFlag::SCROLLABLE);
    Ok(o)
}

/// 建一个文本标签（字号 + 颜色都来自 `theme`），并**摘掉可点 / 可滚**——标签是纯显示件，
/// 不得吃掉触摸事件（与 `components.rs::text_label` 同口径）。
fn text_label(parent: &Obj, text: &str, slot: TextSlot, color: Color) -> Result<Label, LvglError> {
    let l = Label::create_with_text(parent, text)?;
    l.add_style(&theme::text(slot, color), StyleSelector::main());
    l.remove_flag(ObjFlag::CLICKABLE);
    l.remove_flag(ObjFlag::SCROLLABLE);
    Ok(l)
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. SegmentedControl —— 分段控件（lv_buttonmatrix 单选互斥）
// ═══════════════════════════════════════════════════════════════════════════

// ── 尺寸（UI §5.1 #4）──────────────────────────────────────────────────────

/// 分段控件高（UI §5.1 #4「高 48」）。
///
/// 取 [`Dimens::CHIP_H`]（= 48，与 [`Dimens::TOUCH_MIN`] **数值相同但语义不同**）：分段控件
/// 与多选 Chip 同视觉族（同底 / 同描边 / 同圆角 / 同高），两者并排时不得差高度。
pub const SEGMENT_H: i32 = Dimens::CHIP_H;

/// 单段最小宽（UI §5.1 #4「段宽均分，最小 96」）。
pub const SEGMENT_MIN_W: i32 = Dimens::CHIP_MIN_W;

/// 分段控件**四态**的样式选择器真源（UI §5.2 `SegmentedControl` 行逐行抄录）。
///
/// 顺序与 §5.2 表的列序一致：**正常 / 按下 / 选中 / 禁用**。构造器按本表施加样式、离屏 /
/// 纯逻辑用例按本表核对 —— 薄层没有"某选择器下挂了哪些样式"的读回 API，故"确已挂上"的
/// 断言只能以本列表为据（与 `ButtonStyles::entries` 同法）。
///
/// 「选中」（`ITEMS × CHECKED`）与「禁用」（`ITEMS × DISABLED`）两条**确已生效**：
/// 构造期 `set_ctrl_all(CTRL_CHECKABLE)`、切换时"全清 + 单段置 CHECKED"、禁用时
/// `set_ctrl_all(CTRL_DISABLED)`（原 CD5 缺陷的根因与修复见模块文档偏差表）。
/// 本表是四态（§5.2 完整矩阵）的单一真源，`ui/tests.rs` 以它核对选择器。
pub const ITEM_STATES: [(Part, State); 4] = [
    (Part::ITEMS, State::DEFAULT),
    (Part::ITEMS, State::PRESSED),
    (Part::ITEMS, State::CHECKED),
    (Part::ITEMS, State::DISABLED),
];

/// 分段控件（UI §5.1 #4：`lv_buttonmatrix` + `one_checked` 单选互斥；高 48、段宽均分、最小 96）。
///
/// **单选语义**：`set_one_checked(true)` 之后 LVGL 只允许一个键处于 `CHECKED`
/// （`lv_buttonmatrix.c`）。选中下标经薄层 [`ButtonMatrix::selected`] /
/// [`ButtonMatrix::set_selected`] 读写。
///
/// **越界不 panic**：构造与 [`SegmentedControl::set_selected`] 的 `selected` 一律 `clamp`
/// 到 `[0, count − 1]`（`count` 由 `options.len()` 给出，且已由"整控件宽 ≤ 内容区宽"与
/// "单段 ≥ 96"两条校验上界 ⇒ 段数 ≤ 10，转 `u32` 不可能截断）。
pub struct SegmentedControl {
    /// 底层键矩阵（**拥有型**：`Rc` 锚住句柄，`Drop` 时删除整棵子树）。
    bm: Rc<ButtonMatrix>,
    /// 段数（= 选项数；构造后不变）。
    count: usize,
    /// 最近一次确认的选中下标 —— **仅**作 LVGL 报"无选中"（`LV_BUTTONMATRIX_BUTTON_NONE`）
    /// 时的回退值，见 [`SegmentedControl::selected`]。
    current: Cell<usize>,
    /// 选中变更回调（`VALUE_CHANGED` 时触发，载荷 = 当前选中下标）。
    on_change: IndexCallback,
}

impl SegmentedControl {
    /// 建分段控件。
    ///
    /// # 参数非法即**响亮失败**（不静默"修正"）
    ///
    /// - `options` 为空：无段可渲染，`selected` 无意义；
    /// - `width < count × `[`SEGMENT_MIN_W`]：违反 UI §5.1 #4 的「段宽均分，最小 96」；
    /// - `width > `[`Dimens::CONTENT_W`]：装不进内容区（UI §3.5「有效宽 992」）。
    ///
    /// `selected` **不**在此列 —— 越界下标按"初始化到合法区间"处理（clamp），
    /// 与 [`Stepper`] 对 `value` 的处置同口径。
    pub fn new(
        parent: &Obj,
        options: &[&str],
        width: i32,
        selected: usize,
    ) -> Result<Self, LvglError> {
        if options.is_empty() {
            return Err(LvglError::InvalidArgument("SegmentedControl: 选项不得为空"));
        }
        let count = options.len();
        let count_i32 = i32::try_from(count)
            .map_err(|_| LvglError::InvalidArgument("SegmentedControl: 选项过多"))?;
        // `checked_mul`：极大 `width` / 段数在 debug 下不得溢出 panic（与 B1 同口径）。
        let Some(min_w) = count_i32.checked_mul(SEGMENT_MIN_W) else {
            return Err(LvglError::InvalidArgument("SegmentedControl: 段数过大"));
        };
        if width < min_w {
            return Err(LvglError::InvalidArgument(
                "SegmentedControl: 整控件宽不足（每段最小 SEGMENT_MIN_W）",
            ));
        }
        if width > Dimens::CONTENT_W {
            return Err(LvglError::InvalidArgument(
                "SegmentedControl: 整控件宽超出内容区",
            ));
        }

        let bm = Rc::new(ButtonMatrix::create(parent)?);
        bm.set_size(width, SEGMENT_H);
        // 容器（`LV_PART_MAIN`）透明且零内边距：段的几何完全由键矩阵均分决定，不得被默认主题
        // 的底色 / 内边距撑歪（设计 §5.6 控件策略行）。
        bm.add_style(&theme::transparent(), StyleSelector::main());
        // 地图整体替换（`ButtonMatrix` 自己锚住这份 `CString` 串与指针数组，不悬垂）。
        bm.set_map(options);
        // §5.2 `SegmentedControl` 行的四态（逐条取 theme；"选中/禁用"两条的现状见 CD5）。
        let styles = [
            theme::control_surface(),
            theme::control_pressed(),
            theme::control_selected(),
            theme::text(TextSlot::Label, Palette::TEXT_DISABLED),
        ];
        for (style, (part, state)) in styles.iter().zip(ITEM_STATES.iter()) {
            bm.add_style(style, StyleSelector::new(*part, *state));
        }
        // 单选互斥（UI §5.1 #4 / §5.3）。
        bm.set_one_checked(true);
        // **每段 CHECKABLE**（UI §5.3 原话）—— v9.5.0 的 toggle 前置条件：不置此位，
        // 点击**永不**产生 `CHECKED` ⇒ `LV_STATE_CHECKED`（"选中"态）永不绘制。
        bm.set_ctrl_all(CTRL_CHECKABLE);

        let start = clamp_index(selected, count);
        bm.set_selected(start as u32);
        // 初始高亮：`set_selected_button` 只写 `btn_id_sel`、**不**置 `CHECKED` ctrl 位，
        // 故必须显式补上（先整体清零再单段置位 ⇒ 恒"恰有一段"选中）。
        bm.clear_ctrl_all(CTRL_CHECKED);
        bm.set_ctrl(start as u32, CTRL_CHECKED);

        let current = Cell::new(start);
        let on_change: IndexCallback = Rc::new(RefCell::new(None));

        // ── 事件：`LV_EVENT_VALUE_CHANGED` 由键矩阵类处理器在**按下**时派发
        //    （v9.5.0 `lv_buttonmatrix.c`：`PRESSED` 分支在非 `CLICK_TRIG` / 非 `POPOVER`
        //    的键上先写 `btn_id_sel` 再送 `VALUE_CHANGED`）—— 故回调里读到的
        //    `selected()` 就是**本次按下的段**。
        //
        //    **用 `Weak` 而非 `Rc`**：该闭包的所有权在 LVGL 事件项上（`event.rs` 桥在对象
        //    删除时回收），若捕获 `Rc<ButtonMatrix>` 就会与"LVGL 对象 → 事件项 → 闭包"形成
        //    环 ⇒ 句柄永不落地、控件永不删除。
        {
            let weak = Rc::downgrade(&bm);
            let cur = current.clone();
            let cb = on_change.clone();
            bm.on(EventCode::VALUE_CHANGED, move |_e| {
                let Some(b) = weak.upgrade() else { return };
                let Some(idx) = b.selected() else { return };
                let idx = idx as usize;
                cur.set(idx);
                fire_index(&cb, idx);
            });
        }

        Ok(Self {
            bm,
            count,
            current,
            on_change,
        })
    }

    /// 底层对象（= 键矩阵本体；改尺寸 / 位置用，亦经 `Deref` 直接可用）。
    pub fn obj(&self) -> &Obj {
        self.bm.obj()
    }

    /// 底层键矩阵（**仅测试**）—— 给 `ui/tests.rs` 一个 `has_ctrl` 读回口，
    /// 用 LVGL 侧真值断言"每段 CHECKABLE / 恰一段 CHECKED"（CD5 的回归锁）。
    /// **不进生产 API**：`#[cfg(test)]` 编译期即不可见（生产侧改控制位只能经本类型的方法）。
    #[cfg(test)]
    pub fn matrix(&self) -> &ButtonMatrix {
        &self.bm
    }

    /// 选项数（= 段数）。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 当前选中下标。
    ///
    /// **读自 LVGL**（[`ButtonMatrix::selected`]，即 `btn_id_sel`），仅在 LVGL 报"无选中"
    /// （`LV_BUTTONMATRIX_BUTTON_NONE`）时回退到最近一次确认值 —— 因此
    /// `set_selected(k)` 若**没有**真正落到 LVGL 上，本方法仍会返回 `k`（回退掩盖），
    /// **要抓这类缺陷用 [`SegmentedControl::raw_selected`]**。
    pub fn selected(&self) -> usize {
        self.raw_selected().unwrap_or(self.current.get())
    }

    /// 底层键矩阵的选中下标**原值**（`None` = LVGL 的"无选中"）—— **不掩盖**的离屏断言口径。
    pub fn raw_selected(&self) -> Option<usize> {
        self.bm.selected().map(|v| v as usize)
    }

    /// 置选中段（越界下标 `clamp` 到最后一 段；**不**触发 [`SegmentedControl::set_on_change`]，
    /// 与 [`Stepper::set_value`] 的"程序化设值不回调"同口径）。
    pub fn set_selected(&self, index: usize) {
        let i = clamp_index(index, self.count);
        self.bm.set_selected(i as u32);
        // 同步 `CHECKED` ctrl 位（"选中态样式"的**唯一**触发源）：先全清再单段置位，
        // 否则旧选中段会保持高亮 ⇒ 界面上出现"两个都亮"。
        self.bm.clear_ctrl_all(CTRL_CHECKED);
        self.bm.set_ctrl(i as u32, CTRL_CHECKED);
        self.current.set(i);
    }

    /// 某段的文本（**不含**任何前缀；越界或句柄失效时为 `None`）。
    ///
    /// 读自地图本身（`lv_buttonmatrix_get_button_text`）而不是另存一份 `Vec<String>` ——
    /// 选项文案只有一个真源。
    pub fn option(&self, index: usize) -> Option<String> {
        let i = u32::try_from(index).ok()?;
        self.bm.button_text(i)
    }

    /// 显式禁用 / 恢复（只读字段、提交中禁用）。
    ///
    /// 置 `LV_STATE_DISABLED` 于键矩阵本体：v9.5.0 的输入路径以该状态位判定"该对象是否
    /// 可交互"（`src/indev/lv_indev.c`：`is_enabled = !lv_obj_has_state(indev_obj_act,
    /// LV_STATE_DISABLED)`）⇒ 禁用后**不再产生** `VALUE_CHANGED`（结构性，不靠回调自觉）。
    /// 另逐段设 / 清 `CTRL_DISABLED` ctrl 位（`0x0040`）—— 每段的**字色 / 底色**变灰由
    /// 该位经 `ITEMS × DISABLED` 样式表达（§5.2「禁用 字 `#5A6780`」；原 CD5 已修复）。
    pub fn set_disabled(&self, disabled: bool) {
        widgets::set_state(self.bm.obj(), State::DISABLED, disabled);
        // 逐段 `DISABLED` ctrl 位：v9.5.0 绘制趟的 `btn_state` 由 ctrl 位组装，
        // 故每段字色 / 底色要变灰必须置此位（对象级 `LV_STATE_DISABLED` 只管"可否交互"）。
        if disabled {
            self.bm.set_ctrl_all(CTRL_DISABLED);
        } else {
            self.bm.clear_ctrl_all(CTRL_DISABLED);
        }
    }

    /// 是否禁用（读自 LVGL 的状态位，是**单一真源**）。
    pub fn is_disabled(&self) -> bool {
        widgets::has_state(self.bm.obj(), State::DISABLED)
    }

    /// 注册选中变更回调（每次 `VALUE_CHANGED` 触发一次，载荷 = 选中下标）。
    ///
    /// **重复点同一段也会回调**（载荷不变）—— 键矩阵的 `VALUE_CHANGED` 语义是"本次按下的
    /// 键"，不是"选中项发生了变化"；调用方按幂等处理（写草稿值 + 置脏标记）。
    pub fn set_on_change<F>(&self, f: F)
    where
        F: FnMut(usize) + 'static,
    {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }
}

impl std::ops::Deref for SegmentedControl {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        self.bm.obj()
    }
}

impl std::fmt::Debug for SegmentedControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedControl")
            .field("count", &self.count)
            .field("raw_selected", &self.raw_selected())
            .field("disabled", &self.is_disabled())
            .finish()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Ipv4Stepper —— 四段步进（4 × Stepper + 汇总 Label）
// ═══════════════════════════════════════════════════════════════════════════

// ── 尺寸（全部由 theme 分项常量推导；**不**硬编码文档里的 856 / 792）──────────

/// IPv4 段数（协议事实：四个八位组 —— 不是设计栅格值）。
const IPV4_OCTETS: usize = 4;

/// 单个 IPv4 段的宽 = `−` + 值区 + `＋`（UI §5.1 #7 的分项）。
pub const IPV4_SEG_W: i32 = Dimens::STEPPER_BTN_W * 2 + Dimens::IPV4_VALUE_W;

/// 四段的总宽（UI §5.1 #7 的分项之和；文档该行正文写的 **856** 与自身分项不符 —— 见 CD1）。
pub const IPV4_SEGMENTS_W: i32 =
    IPV4_OCTETS as i32 * IPV4_SEG_W + (IPV4_OCTETS as i32 - 1) * Dimens::IPV4_GAP;

/// 汇总标签宽 = **内容区余量**（`CONTENT_W − 四段 − 缝`）—— 见 CD2。
pub const IPV4_SUMMARY_W: i32 = Dimens::CONTENT_W - IPV4_SEGMENTS_W - Dimens::GAP_MIN;

/// 整件宽 = 四段 + 缝 + 汇总 = [`Dimens::CONTENT_W`]（UI §6.1 行型 B「控件独占次行」）。
pub const IPV4_TOTAL_W: i32 = IPV4_SEGMENTS_W + Dimens::GAP_MIN + IPV4_SUMMARY_W;

/// 整件高（UI §5.1 #7「…×64」）。
pub const IPV4_TOTAL_H: i32 = Dimens::STEPPER_H;

// 静态不变量（**编译期**校验）：汇总标签必须真的分到正宽 —— 否则 [`IPV4_SUMMARY_W`] 的推导
// （CD2）已被改坏，而 `Stepper` 一侧不会报错、只会静默重叠。
//
// 写成模块级 `const _: () = assert!(..)` 而不是运行期 `assert!`：clippy 的
// `assertions_on_constants` 对"两侧都是编译期常量"的 `assert!` 会告警，而**编译期**校验
// 更强（不可能被 `--skip` / 被过滤掉）。同一口径下的另两条不变量见 [`DATETIME_VALUE_W`]。
const _: () = assert!(IPV4_SEGMENTS_W + Dimens::GAP_MIN < IPV4_TOTAL_W);

/// 四段 IPv4 步进（UI §5.1 #7 / §5.3 `Ipv4Stepper`）。
///
/// **零件**：4 × [`Stepper`]（每段 [`Dimens::IPV4_VALUE_W`] 值区宽、区间 `0–255`）+
/// 末位一个汇总 [`Label`]（显示 `192.168.1.10`）。
///
/// **跨段不进位**（UI §5.3 原文）：每段是独立的有界步进器，`255` 处 `＋` 进 `DISABLED`，
/// 绝不把进位写到左边一段。
///
/// **值只有一个真源**：[`Ipv4Stepper::octets`] 直接读四个 [`Stepper`] 的当前值，本类型
/// **不另存**一份 `[u8; 4]` 缓存（缓存必然与 LVGL 漂移，且是 B1 那类 UAF 缺陷的温床）。
pub struct Ipv4Stepper {
    /// 行容器（**拥有型**：四段与汇总标签的父对象；`Drop` 时级联删除整行）。
    _row: Obj,
    /// 四个段（`Rc` 锚住句柄：它们是 `_row` 的子对象，但 Rust 句柄必须活着才能用
    /// [`Ipv4Stepper::segment`] 读值 / 改值）。
    segments: [Rc<Stepper>; IPV4_OCTETS],
    /// 汇总标签（`Rc` 锚住句柄，否则 [`Ipv4Stepper::text`] 恒为 `None`）。
    summary: Rc<Label>,
    /// 四段任一变更时触发（载荷 = 变更后的四段全值）。
    on_change: OctetsCallback,
}

impl Ipv4Stepper {
    /// 建四段步进（初始四段由 `octets` 给出）。
    pub fn new(parent: &Obj, octets: [u8; 4]) -> Result<Self, LvglError> {
        let row = layout_box(parent, IPV4_TOTAL_W, IPV4_TOTAL_H)?;

        // ── 四段：逐段建 `Stepper`（**复用 B1 的零件，不重新实现**）──
        //
        // `map(...).collect::<Result<Vec<_>, _>>()?`：任一段创建失败即整体 `Err`（不留
        // 半成品）；再由 `try_into` 收成**定长数组** ⇒ 后续取值 / 回调全走定长解构，
        // 不存在运行时越界索引。
        let segments: [Rc<Stepper>; IPV4_OCTETS] = octets
            .iter()
            .map(|v| {
                Stepper::with_value_width(
                    &row,
                    OCTET_MIN,
                    OCTET_MAX,
                    i64::from(*v),
                    STEP,
                    Dimens::IPV4_VALUE_W,
                )
                .map(Rc::new)
            })
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| LvglError::InvalidArgument("Ipv4Stepper: 段数与 IPV4_OCTETS 不符"))?;

        for (i, seg) in segments.iter().enumerate() {
            seg.set_pos(i as i32 * (IPV4_SEG_W + Dimens::IPV4_GAP), 0);
        }

        // ── 汇总标签（末位）──
        let summary = Rc::new(text_label(
            &row,
            &ipv4_text(octets),
            TextSlot::Label,
            Palette::TEXT_PRIMARY,
        )?);
        summary.set_size(IPV4_SUMMARY_W, TextSlot::Label.px() as i32);
        // `DOTS` 是**绘制期**溢出保护（放不下则原地截断收尾），不影响文本属性
        // （[`Ipv4Stepper::text`] 仍返回完整串）—— 见薄层缺能力 4。
        summary.set_long_mode(LongMode::DOTS);
        summary.set_pos(
            IPV4_SEGMENTS_W + Dimens::GAP_MIN,
            theme::center_offset(IPV4_TOTAL_H, TextSlot::Label.px() as i32),
        );

        // ── 事件：任一段变值 ⇒ 重算汇总文本 + 回调（载荷 = 四段全值）──
        //
        // 闭包捕获 **`Weak<Stepper>` ×4**（而非 `Rc`）：`Stepper` 的 `on_change` 槽由它自己
        // 持有，捕获 `Rc<Stepper>` 会形成 `Stepper → 槽 → 闭包 → Rc<Stepper>` 的环 ⇒ 永不回收。
        let on_change: OctetsCallback = Rc::new(RefCell::new(None));
        // `clone().map(..)`：按值映射定长数组 ⇒ **无下标**（连"理论上的越界 panic"都不存在）。
        let weaks: [Weak<Stepper>; IPV4_OCTETS] = segments.clone().map(|s| Rc::downgrade(&s));
        for seg in segments.iter() {
            let weaks = weaks.clone();
            let sum = Rc::downgrade(&summary);
            let cb = on_change.clone();
            // 回调内**不 panic**：`upgrade` 失败即跳过；定长数组 `zip` 遍历，无越界索引。
            seg.set_on_change(move |_v: i64| {
                let mut next = [0u8; IPV4_OCTETS];
                for (slot, w) in next.iter_mut().zip(weaks.iter()) {
                    let Some(s) = w.upgrade() else { return };
                    *slot = octet_of(s.value());
                }
                if let Some(l) = sum.upgrade() {
                    l.set_text(&ipv4_text(next));
                }
                fire_octets(&cb, next);
            });
        }

        Ok(Self {
            _row: row,
            segments,
            summary,
            on_change,
        })
    }

    /// 行容器对象（改位置用；亦经 `Deref` 直接可用）。
    pub fn obj(&self) -> &Obj {
        &self._row
    }

    /// 当前四段值（**读自四个 [`Stepper`]**，即 LVGL 真值）。
    pub fn octets(&self) -> [u8; 4] {
        let mut a = [0u8; IPV4_OCTETS];
        for (slot, s) in a.iter_mut().zip(self.segments.iter()) {
            *slot = octet_of(s.value());
        }
        a
    }

    /// 设四段值（逐段 `set_value`，`Stepper` 自己再 clamp 一次；**不**触发 `on_change`）。
    pub fn set_octets(&self, octets: [u8; 4]) {
        for (s, v) in self.segments.iter().zip(octets.iter()) {
            s.set_value(i64::from(*v));
        }
        self.summary.set_text(&ipv4_text(octets));
    }

    /// 第 `index` 段（0–3；越界 `None`）—— **离屏断言 / 诊断入口**：可读该段的显示值、
    /// 两端禁用态（`display` / `minus_disabled` / `plus_disabled`）。
    ///
    /// ⚠️ **不要在业务里用它改值**：`Stepper::set_value` 按设计**不**触发 `on_change`，
    /// 故经此路径改段值**不会**刷新汇总标签（[`Ipv4Stepper::text`]）——生产路径请用
    /// [`Ipv4Stepper::set_octets`]（它同时更新四段与汇总）。
    ///
    /// **`#[cfg(test)]`**：本访问器**只**给离屏用例 / 诊断用 —— 生产代码若经它改值会静默
    /// 绕过汇总同步（上文即该陷阱），故**编译期**就不给生产侧这条路径。
    #[cfg(test)]
    pub fn segment(&self, index: usize) -> Option<&Stepper> {
        self.segments.get(index).map(|s| &**s)
    }

    /// 汇总标签文本（`192.168.1.10`；句柄失效时为 `None`）。
    pub fn text(&self) -> Option<String> {
        self.summary.text()
    }

    /// 显式禁用 / 恢复（逐段转发到 [`Stepper::set_disabled`]）。
    pub fn set_disabled(&self, disabled: bool) {
        for s in self.segments.iter() {
            s.set_disabled(disabled);
        }
    }

    /// 注册变更回调（任一段变值后触发一次，载荷 = 四段全值）。
    ///
    /// ⚠️ **离屏用例无法驱动本回调**：`Stepper` 的 `−` / `＋` 闭包挂在它的**子按钮**上，
    /// 薄层只能向本对象派发事件（向父链冒泡，不下行）—— 见模块文档「薄层缺能力」5。
    pub fn set_on_change<F>(&self, f: F)
    where
        F: FnMut([u8; 4]) + 'static,
    {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }
}

impl std::ops::Deref for Ipv4Stepper {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self._row
    }
}

impl std::fmt::Debug for Ipv4Stepper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ipv4Stepper")
            .field("octets", &self.octets())
            .field("text", &self.text())
            .finish()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. DateTimeStepper —— 日期时间步进（5 × Stepper + 5 个列头）
// ═══════════════════════════════════════════════════════════════════════════

/// 列数（年 / 月 / 日 / 时 / 分 —— UI §5.1 #8）。
const DATETIME_COLS: i32 = 5;

/// 日期时间列的**值区宽**：内容区有效宽内**均分五列**后剩下的部分。
///
/// 推导：`CONTENT_W − 4 × DATETIME_GAP（列间缝）− 5 × 2 × STEPPER_BTN_W（每列两侧按钮）`
/// 再除以 5 ⇒ **64**。取均分（而不是 UI §5.1 #8 正文的列宽 112）的原因见 **CD3**：
/// 112 既当不了列宽（列宽下限 176），也当不了值区宽（五列 1232 装不下）。
pub const DATETIME_VALUE_W: i32 = (Dimens::CONTENT_W
    - (DATETIME_COLS - 1) * Dimens::DATETIME_GAP
    - DATETIME_COLS * 2 * Dimens::STEPPER_BTN_W)
    / DATETIME_COLS;

/// 单列（一个 [`Stepper`]）的宽。
pub const DATETIME_COL_STEPPER_W: i32 = Dimens::STEPPER_BTN_W * 2 + DATETIME_VALUE_W;

// 静态不变量（**编译期**校验）—— 把 CD3 的算术钉死，防有人照抄 §5.1 #8 正文的 112：
//
// ① 值区宽不得低于最小触摸目标（否则 `Stepper::with_value_width` 运行期会拒绝）；
// ② UI 正文的 **112 连"两侧按钮 + 最小触摸值区"都放不下**（160 = 2×64 + 48 > 112）
//    ⇒ 112 不可能是列宽；
// ③ 同理 112 也不可能是值区宽（五列会宽到 1232，超屏幕）。
const _: () = assert!(DATETIME_VALUE_W >= Dimens::TOUCH_MIN);
const _: () = assert!(Dimens::DATETIME_COL_W < DATETIME_COL_STEPPER_W);
const _: () = assert!(2 * Dimens::STEPPER_BTN_W + Dimens::TOUCH_MIN > Dimens::DATETIME_COL_W);

/// 列头标签的**盒宽**（= 一个字的宽 —— [`TextSlot::Label`]；薄层没有文本对齐通道，见 CD6，
/// 故用"单字盒 + 居中定位"表达列头的居中）。
pub const DATETIME_HEADER_W: i32 = TextSlot::Label.px() as i32;

/// 列头行高。
pub const DATETIME_HEADER_H: i32 = TextSlot::Label.px() as i32;

/// 列头行 y（列表顶部）。
pub const DATETIME_HEADER_Y: i32 = 0;

/// 步进行 y（= 列头行 + 同组呼吸缝）。
pub const DATETIME_STEPPER_Y: i32 = DATETIME_HEADER_Y + DATETIME_HEADER_H + Dimens::GAP_MIN;

/// 整件宽（= 五列 + 四条缝 = [`Dimens::CONTENT_W`]）。
pub const DATETIME_TOTAL_W: i32 =
    DATETIME_COLS * DATETIME_COL_STEPPER_W + (DATETIME_COLS - 1) * Dimens::DATETIME_GAP;

/// 整件高（= 列头行 + 缝 + 步进高；UI §5.1 #8 正文写 64，未计入列头 —— 见 CD4）。
pub const DATETIME_TOTAL_H: i32 = DATETIME_STEPPER_Y + Dimens::STEPPER_H;

/// 一个日期时间分量的组合。
///
/// # ⚠️ 本类型（及 [`DateTimeStepper`]）**不做日历校验**
///
/// UI §5.3 `DateTimeStepper` 行原话：「**月 / 日 / 时 / 分的进借位与上限校验在应用侧
/// （Rust/C）完成**，控件层仅做分量封闭」。故：
///
/// - **分量封闭**（本控件负责）：年 `1970–2100`、月 `1–12`、日 `1–31`、时 `0–23`、分 `0–59`；
/// - **组合合法性**（**上层负责**）：`2 月 31 日`、`4 月 31 日`、`2 月 30 日` 这类**跨分量**
///   非法组合**不会**被本控件拒绝，也不会被进位 / 借位修正 —— 例如月从 `12` 再加仍是 `12`
///   （**不**进位到年）。**调用方（应用侧）必须在提交前拒绝此类组合**，否则一个非法日期
///   会被原样送进业务链路。
///
/// 字段为 `pub`（规格如此），故**结构体字面量可以绕过**下面的构造夹取；经
/// [`DateTimeValue::from_parts`] / [`DateTimeStepper::set_value`] 的路径则一律逐分量夹取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTimeValue {
    /// 年（封闭区间 `1970–2100`）。
    pub year: u16,
    /// 月（封闭区间 `1–12`）。
    pub month: u8,
    /// 日（封闭区间 `1–31`；**不**按月 / 闰年校验）。
    pub day: u8,
    /// 时（封闭区间 `0–23`）。
    pub hour: u8,
    /// 分（封闭区间 `0–59`）。
    pub minute: u8,
}

impl DateTimeValue {
    /// 由各分量构造（**逐分量夹取**到封闭区间，不 panic）。
    pub const fn from_parts(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> Self {
        Self {
            year: clamp_i64(year, YEAR_MIN, YEAR_MAX) as u16,
            month: clamp_i64(month, MONTH_MIN, MONTH_MAX) as u8,
            day: clamp_i64(day, DAY_MIN, DAY_MAX) as u8,
            hour: clamp_i64(hour, HOUR_MIN, HOUR_MAX) as u8,
            minute: clamp_i64(minute, MINUTE_MIN, MINUTE_MAX) as u8,
        }
    }
}

/// 日期时间步进（UI §5.1 #8 / §5.3：5 列 [`Stepper`] —— 年 / 月 / 日 / 时 / 分，
/// 每列一个列头 [`Label`]）。
///
/// **零件与 [`Stepper`] 完全一致**（`lv_button` + `lv_label` 组合；**PM 裁定弃
/// `lv_spinbox`、不用 `lv_roller`**）⇒ "界面不存在可编辑文本区"是**结构性**的。
///
/// **跨分量语义不进控件层**：见 [`DateTimeValue`] 的类型文档（本条是**必读**，不是补充）。
pub struct DateTimeStepper {
    /// 列容器（**拥有型**：五列与五个列头的父对象）。
    _box: Obj,
    /// 五列步进器（`Rc` 锚住句柄；`Drop` 时不会误删 LVGL 对象 —— 由 `_box` 级联）。
    columns: [Rc<Stepper>; DATETIME_COLS as usize],
    /// 五个列头标签（**Rc 锚住**，否则 [`DateTimeStepper::column_headers`] 恒为空）。
    headers: [Rc<Label>; DATETIME_COLS as usize],
    /// 任一分量变更时触发（载荷 = 变更后的全值）。
    on_change: DateTimeCallback,
}

impl DateTimeStepper {
    /// 建日期时间步进（各分量由 `value` 给出，越界分量被**夹取**）。
    pub fn new(parent: &Obj, value: DateTimeValue) -> Result<Self, LvglError> {
        let box_ = layout_box(parent, DATETIME_TOTAL_W, DATETIME_TOTAL_H)?;

        let v = DateTimeValue::from_parts(
            i64::from(value.year),
            i64::from(value.month),
            i64::from(value.day),
            i64::from(value.hour),
            i64::from(value.minute),
        );
        let starts: [i64; DATETIME_COLS as usize] = [
            i64::from(v.year),
            i64::from(v.month),
            i64::from(v.day),
            i64::from(v.hour),
            i64::from(v.minute),
        ];
        let ranges: [(i64, i64); DATETIME_COLS as usize] = [
            (YEAR_MIN, YEAR_MAX),
            (MONTH_MIN, MONTH_MAX),
            (DAY_MIN, DAY_MAX),
            (HOUR_MIN, HOUR_MAX),
            (MINUTE_MIN, MINUTE_MAX),
        ];

        // ── 五列（复用 B1 的 `Stepper`；`try_into` 收成定长数组 ⇒ 取值无越界索引）──
        let columns: [Rc<Stepper>; DATETIME_COLS as usize] = starts
            .iter()
            .zip(ranges.iter())
            .map(|(start, (lo, hi))| {
                Stepper::with_value_width(&box_, *lo, *hi, *start, STEP, DATETIME_VALUE_W)
                    .map(Rc::new)
            })
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| LvglError::InvalidArgument("DateTimeStepper: 列数与 DATETIME_COLS 不符"))?;

        // ── 五个列头（单字盒 + 居中定位；薄层无文本对齐通道 —— CD6）──
        let headers: [Rc<Label>; DATETIME_COLS as usize] = DATETIME_HEADERS
            .iter()
            .map(|t| {
                text_label(&box_, t, TextSlot::Label, Palette::TEXT_SECOND).map(|l| {
                    l.set_size(DATETIME_HEADER_W, DATETIME_HEADER_H);
                    Rc::new(l)
                })
            })
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| LvglError::InvalidArgument("DateTimeStepper: 列头数与 DATETIME_COLS 不符"))?;

        // 逐个摆位（列 x 由常量推导，不写裸坐标）。
        for (i, (col, head)) in columns.iter().zip(headers.iter()).enumerate() {
            let x = i as i32 * (DATETIME_COL_STEPPER_W + Dimens::DATETIME_GAP);
            col.set_pos(x, DATETIME_STEPPER_Y);
            head.set_pos(
                x + theme::center_offset(DATETIME_COL_STEPPER_W, DATETIME_HEADER_W),
                DATETIME_HEADER_Y,
            );
        }

        // ── 事件：任一列变值 ⇒ 读回五列 + 回调 ──
        //
        // 同样用 `Weak` 防环（理由见 [`Ipv4Stepper::new`]）；回调内五行 `Some(..) else return`
        // 解构 + `from_parts` 夹取 ⇒ **无 `unwrap`、无索引越界**。
        let on_change: DateTimeCallback = Rc::new(RefCell::new(None));
        // `clone().map(..)`：按值映射定长数组 ⇒ **无下标**（连"理论上的越界 panic"都不存在）。
        let weaks: [Weak<Stepper>; DATETIME_COLS as usize] =
            columns.clone().map(|c| Rc::downgrade(&c));
        for col in columns.iter() {
            let weaks = weaks.clone();
            let cb = on_change.clone();
            col.set_on_change(move |_v: i64| {
                let [wy, wmo, wd, wh, wmi] = &weaks;
                let (Some(y), Some(mo), Some(d), Some(h), Some(mi)) = (
                    wy.upgrade(),
                    wmo.upgrade(),
                    wd.upgrade(),
                    wh.upgrade(),
                    wmi.upgrade(),
                ) else {
                    return;
                };
                let next = DateTimeValue::from_parts(
                    y.value(),
                    mo.value(),
                    d.value(),
                    h.value(),
                    mi.value(),
                );
                fire_datetime(&cb, next);
            });
        }

        Ok(Self {
            _box: box_,
            columns,
            headers,
            on_change,
        })
    }

    /// 列容器对象（改位置用；亦经 `Deref` 直接可用）。
    pub fn obj(&self) -> &Obj {
        &self._box
    }

    /// 当前五分量值（**读自五个 [`Stepper`]**，即 LVGL 真值；逐分量再夹取一次 ⇒ 恒在封闭区间内）。
    pub fn value(&self) -> DateTimeValue {
        let mut v = [0i64; DATETIME_COLS as usize];
        for (slot, c) in v.iter_mut().zip(self.columns.iter()) {
            *slot = c.value();
        }
        let [y, mo, d, h, mi] = v;
        DateTimeValue::from_parts(y, mo, d, h, mi)
    }

    /// 设五分量值（逐列 `set_value`，越界分量被夹取；**不**触发 `on_change`）。
    pub fn set_value(&self, value: DateTimeValue) {
        let v = DateTimeValue::from_parts(
            i64::from(value.year),
            i64::from(value.month),
            i64::from(value.day),
            i64::from(value.hour),
            i64::from(value.minute),
        );
        let parts: [i64; DATETIME_COLS as usize] = [
            i64::from(v.year),
            i64::from(v.month),
            i64::from(v.day),
            i64::from(v.hour),
            i64::from(v.minute),
        ];
        for (c, p) in self.columns.iter().zip(parts.iter()) {
            c.set_value(*p);
        }
    }

    /// 第 `index` 列（0–4；越界 `None`）—— **离屏断言 / 诊断入口**：可读该列的显示值、
    /// 两端禁用态。
    ///
    /// ⚠️ 与 [`Ipv4Stepper::segment`] 同款提醒：经此路径改列值**不会**触发 `on_change`
    /// （`Stepper::set_value` 按设计不回调）——生产路径请用 [`DateTimeStepper::set_value`]。
    /// 同 [`Ipv4Stepper::segment`]：**`#[cfg(test)]`**（生产侧不得经它改值）。
    #[cfg(test)]
    pub fn column(&self, index: usize) -> Option<&Stepper> {
        self.columns.get(index).map(|c| &**c)
    }

    /// 五个列头标签的文本（顺序 = [`DATETIME_HEADERS`]）。
    ///
    /// **不发**静默补位：句柄失效（被误删）时对应项直接缺席 ⇒ 长度短于 5（用例据此变红）。
    pub fn column_headers(&self) -> Vec<String> {
        self.headers.iter().filter_map(|h| h.text()).collect()
    }

    /// 显式禁用 / 恢复（逐列转发到 [`Stepper::set_disabled`]）。
    pub fn set_disabled(&self, disabled: bool) {
        for c in self.columns.iter() {
            c.set_disabled(disabled);
        }
    }

    /// 注册变更回调（任一分量变值后触发一次，载荷 = 全值）。
    ///
    /// ⚠️ 离屏用例无法驱动本回调 —— 见 [`Ipv4Stepper::set_on_change`] 的同款说明。
    pub fn set_on_change<F>(&self, f: F)
    where
        F: FnMut(DateTimeValue) + 'static,
    {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }
}

impl std::ops::Deref for DateTimeStepper {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self._box
    }
}

impl std::fmt::Debug for DateTimeStepper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DateTimeStepper")
            .field("value", &self.value())
            .field("headers", &self.column_headers())
            .finish()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. 纯逻辑单测（**不触碰 LVGL** —— LVGL 非线程安全，触碰它的用例只能由
//    `src/lvgl/tests.rs::lvgl_core_bridge_chain` 串行调起，见 `ui/tests.rs`）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 下标夹取：**越界被夹而非 panic**（构造器与 `set_selected` 共用同一函数）。
    ///
    /// 敏感性：把 [`clamp_index`] 改成直接返回 `selected` ⇒ 第 2、3 条断言立刻变红。
    #[test]
    fn segment_index_is_clamped() {
        assert_eq!(clamp_index(0, 4), 0, "下界不动");
        assert_eq!(clamp_index(3, 4), 3, "上界不动");
        assert_eq!(clamp_index(99, 4), 3, "越上界夹到最后一 段");
        // `count == 0` 的分支不可达（构造器已拒绝空选项），但必须**不 panic**。
        assert_eq!(clamp_index(7, 0), 0, "空集合不得 panic / 不得下溢");
    }

    /// 八位组夹取：`0–255` 封闭。
    ///
    /// 敏感性：把 [`octet_of`] 改成 `v as u8` ⇒ 第 2、3 条立刻变红（`300 as u8 == 44`、
    /// `-1 as u8 == 255`，正是"看不见的静默错值"）。
    #[test]
    fn octet_is_clamped() {
        assert_eq!(octet_of(0), 0);
        assert_eq!(octet_of(255), 255);
        assert_eq!(octet_of(300), 255, "越上界夹取（**不是** 300 as u8 = 44）");
        assert_eq!(octet_of(-1), 0, "越下界夹取（**不是** -1 as u8 = 255）");
    }

    /// 汇总文本形状：四段以 `.` 连接（**分隔符必须在字体 cmap 内**）。
    #[test]
    fn ipv4_text_shape() {
        assert_eq!(ipv4_text([192, 168, 1, 10]), "192.168.1.10");
        assert_eq!(ipv4_text([0, 0, 0, 0]), "0.0.0.0");
        assert_eq!(ipv4_text([255, 255, 255, 255]), "255.255.255.255");
        // 分段数 = 3 个分隔符（写成 `:` 或漏一段会立刻变红）。
        assert_eq!(ipv4_text([1, 2, 3, 4]).matches(IPV4_SEP).count(), 3, "四段三个点");
    }

    /// [`DateTimeValue`] 的分量封闭（**越界被夹而非 panic**）+ 各边界。
    ///
    /// 敏感性：任一分量的 `clamp` 去掉 / 上下界写反 ⇒ 对应断言立刻变红。
    #[test]
    fn datetime_value_clamps_every_component() {
        assert_eq!(
            DateTimeValue::from_parts(2026, 9, 12, 13, 42),
            DateTimeValue {
                year: 2026,
                month: 9,
                day: 12,
                hour: 13,
                minute: 42
            },
            "合法值原样通过（往返）"
        );
        // 下界
        assert_eq!(
            DateTimeValue::from_parts(1969, 0, 0, -1, -1),
            DateTimeValue {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0
            },
            "各分量越下界夹到 min"
        );
        // 上界
        assert_eq!(
            DateTimeValue::from_parts(9999, 13, 32, 24, 60),
            DateTimeValue {
                year: 2100,
                month: 12,
                day: 31,
                hour: 23,
                minute: 59
            },
            "各分量越上界夹到 max"
        );
        // 文档给出的四条边界必须"恰好在界内"（写窄 1 格即红）
        assert_eq!(DateTimeValue::from_parts(1970, 1, 1, 0, 0).year, 1970);
        assert_eq!(DateTimeValue::from_parts(2100, 1, 1, 0, 0).year, 2100);
        assert_eq!(DateTimeValue::from_parts(2026, 12, 31, 23, 59).minute, 59);
        assert_eq!(DateTimeValue::from_parts(2026, 12, 31, 23, 59).day, 31);
        // **不做日历校验**（UI §5.3：跨分量语义在上层）—— 2 月 31 日**必须原样通过**，
        // 若有人在此加了日历校验，本条即红（那属于越权，会与上层的校验打架）。
        assert_eq!(
            DateTimeValue::from_parts(2026, 2, 31, 0, 0),
            DateTimeValue {
                year: 2026,
                month: 2,
                day: 31,
                hour: 0,
                minute: 0
            },
            "控件层**不做**日历校验（2 月 31 日原样通过，拒收是上层责任）"
        );
    }

    /// 分段控件的四态选择器真源 = UI §5.2 `SegmentedControl` 行的四列（正常 / 按下 / 选中 / 禁用）。
    ///
    /// 敏感性：删掉「选中」（`ITEMS × CHECKED`）或把部件写成 `MAIN` ⇒ 立刻变红。
    #[test]
    fn segmented_style_states_match_ui_spec() {
        assert_eq!(
            ITEM_STATES,
            [
                (Part::ITEMS, State::DEFAULT),
                (Part::ITEMS, State::PRESSED),
                (Part::ITEMS, State::CHECKED),
                (Part::ITEMS, State::DISABLED),
            ],
            "§5.2 四态：正常 / 按下 / 选中 / 禁用，且必须挂在 LV_PART_ITEMS（不是 MAIN）"
        );
        assert_eq!(ITEM_STATES.len(), 4, "四态齐备");
    }

    /// 尺寸推导：**必须由 theme 常量算出**（防"硬编码文档里的 856 / 592"）。
    ///
    /// 敏感性：把某个尺寸改成字面量（如 `856`）⇒ 对应断言立刻变红。
    #[test]
    fn sizes_are_derived_from_theme_constants() {
        assert_eq!(
            SEGMENT_H, Dimens::CHIP_H,
            "分段控件高取 Chip 高（§5.1 #4 高 48）"
        );
        assert_eq!(SEGMENT_MIN_W, Dimens::CHIP_MIN_W, "单段最小宽取 Chip 最小宽 96");

        // IPv4：四段 = 4 × (64 + 64 + 64) + 3 × 8 = 792；文档正文的 856 **算不出来**（CD1）。
        assert_eq!(IPV4_SEG_W, 192, "单段 = STEPPER_BTN_W × 2 + IPV4_VALUE_W");
        assert_eq!(IPV4_SEGMENTS_W, 792, "四段总宽（**不是**文档正文的 856）");
        assert_eq!(IPV4_TOTAL_W, Dimens::CONTENT_W, "整件铺满内容区有效宽（CD2）");
        assert_eq!(IPV4_TOTAL_H, Dimens::STEPPER_H);
        // （"汇总标签分到正宽"是**编译期**不变量，见 `IPV4_SUMMARY_W` 下方的 `const _` 断言。）

        // DateTime：五列 = 5 × (64 + 64 + 64) + 4 × 8 = 992（**不是**文档的 592，CD3/CD4）。
        assert_eq!(DATETIME_VALUE_W, 64, "值区宽 = 内容区均分五列的结果");
        assert_eq!(DATETIME_COL_STEPPER_W, 192, "单列 = STEPPER_BTN_W × 2 + 值区宽");
        assert_eq!(DATETIME_TOTAL_W, Dimens::CONTENT_W, "五列铺满内容区有效宽");
        assert_eq!(DATETIME_TOTAL_H, DATETIME_HEADER_H + Dimens::GAP_MIN + Dimens::STEPPER_H);
        // （"值区宽 ≥ TOUCH_MIN"与"112 当列宽不可行"同为**编译期**不变量，见下方的 `const _` 断言。）
    }

    /// 列头文案 = UI §5.1 #8 的五个字（**逐字**，且都在字体 cmap 内）。
    #[test]
    fn datetime_headers_match_ui_spec() {
        assert_eq!(DATETIME_HEADERS, ["年", "月", "日", "时", "分"]);
        assert_eq!(ALL_TEXTS.len(), 5, "清册与列头一一对应");
    }
}
