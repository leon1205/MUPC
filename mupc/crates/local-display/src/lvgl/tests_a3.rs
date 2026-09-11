//! 薄层「控件」自测（工作单元 A3：`widgets.rs`；设计 §5.6 控件映射表）。
//!
//! # ⚠️ 为什么没有自己的 `#[test]`
//!
//! LVGL 全局状态**非线程安全**（`lv_conf.h`：`LV_USE_OS = LV_OS_NONE`），`cargo test`
//! 默认多线程跑测试函数。A3 的用例因此不另起 `#[test]`，而是由
//! `tests.rs::lvgl_core_bridge_chain`（**唯一的** LVGL `#[test]`）在同一线程内顺序调起 ——
//! 串行化做法与 A1/A2 完全一致（见 `tests.rs` / `tests_a2.rs` 顶部说明）。
//!
//! # 覆盖（按验收口径逐条）
//!
//! ① 每个新封装的控件**能创建并设置属性**（离屏可断言：尺寸 / 文本 / 勾选 / 值 / 选中项 /
//!    单元格），且确实进了渲染树（按钮底色像素断言）；
//! ② **滚动容器**：`LV_DIR_VER`（结构性禁横滚）+ `LV_SCROLLBAR_MODE_AUTO` 生效、可读回；
//! ③ **长按**：入口 [`Indev::set_long_press_time`] 存在且可用；`LONG_PRESSED` / `PRESSED` /
//!    `RELEASED` / `PRESS_LOST` 四个事件码可经 A2 的安全 `Obj::on` 注册并在离屏推进中不崩
//!    （不真等 1.0 s —— 长按的"满 1.0 s 才派发"由 §11.1 的假 tick 用例覆盖）；
//! ④ **结构性不暴露三个文本输入控件**：源码扫描 `widgets.rs`（本文件用拼接构造符号名，
//!    以免本文件自身被同一个扫描规则误伤）。
//!
//! 所有断言都在**内存 sink** 上完成（与 A1/A2 同一口径；生产版换成写 `/dev/fb0`）。
//!
//! # 用例布局的一处讲究（踩过的坑，如实记录）
//!
//! `lv_tabview` 与 `ScrollContainer` 盖住左上角，会挡住 [③] 要按下的按钮（z-order 更高者
//! 先命中），故 [③] 的触摸用例排在它们**之前**创建。另有 `lv_dropdown` 在构造期就会在
//! **屏幕**上挂一个隐藏的弹出列表（v9.5.0 `lv_dropdown.c` 的 constructor 调
//! `lv_dropdown_list_create(lv_obj_get_screen(obj))`）；它带 `HIDDEN` ⇒ 命中测试会跳过
//! （`lv_indev.c:623`），不影响按下。

use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::rc::Rc;
use std::sync::atomic::Ordering;

use lvgl_sys as sys;

use super::display::{Area, Display, BYTES_PER_PIXEL};
use super::event::EventCode;
use super::indev::{Indev, TouchSnapshot};
use super::obj::{Obj, ObjFlag};
use super::style::{Color, Opa, Part, State, Style, StyleSelector};
use super::tests::count_color;
use super::widgets::{
    self, Bar, BarMode, Button, ButtonMatrix, Checkbox, Dir, Dropdown, Group, Label, Led, List,
    LongMode, MsgBox, ScrollContainer, ScrollMode, Switch, TabView, Table, TextButton,
};

const W: u32 = 256;
const H: u32 = 192;

/// 按钮底色（内存序 `B, G, R` —— XRGB8888；与其它用例的色值刻意远离，防串色）。
const BTN_BG: [u8; 3] = [0x11, 0x88, 0x33];

/// 把脏区像素搬进内存 sink（与 A1/A2 同构）。
fn blit(sink: &Rc<RefCell<Vec<u8>>>, area: Area, px: &[u8]) {
    let mut s = sink.borrow_mut();
    let row_bytes = area.width() as usize * BYTES_PER_PIXEL;
    for row in 0..area.height() as usize {
        let dy = area.y1 as usize + row;
        let dx = area.x1 as usize;
        let off = (dy * W as usize + dx) * BYTES_PER_PIXEL;
        let src = &px[row * row_bytes..(row + 1) * row_bytes];
        s[off..off + row_bytes].copy_from_slice(src);
    }
}

/// 读回 `lv_label` 的文本（测试专用；`obj` 必须是 label）。
///
/// 用于 Critical 回归：`MsgBox::add_title` 返回的是 [`Obj`]（不是 [`Label`]），要断言
/// "第二次 add_title 的文本落定正确"需直接读底层 label。（本文件位于 `src/lvgl/**`，
/// unsafe 边界内；与 `lv_refr_now` 的测试用法同一口径。）
fn label_text(obj: &Obj) -> String {
    // SAFETY: `obj` 由本测试创建且存活；其为 lv_label（MsgBox 标题）。
    let p = unsafe { sys::lv_label_get_text(obj.raw()) };
    assert!(!p.is_null(), "lv_label_get_text 不应返回 NULL");
    // SAFETY: `p` 非空且指向 LVGL 维护的 NUL 结尾串。
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// A3 全部场景（由 `tests.rs` 的唯一 LVGL `#[test]` 在同线程内调用）。
pub(super) fn widgets_chain() {
    super::init().expect("lvgl::init (A3)");

    let sink = Rc::new(RefCell::new(vec![0u8; (W * H) as usize * BYTES_PER_PIXEL]));
    let mut disp = Display::create(W, H).expect("Display::create (A3)");
    {
        let sink_w = sink.clone();
        disp.set_flush_cb(move |area: Area, px: &[u8]| blit(&sink_w, area, px));
    }
    // `lv_refr_now` 是测试专用强制渲染（设计 §5.2 不变量 2：生产路径禁用）。
    let refr = || unsafe { sys::lv_refr_now(disp.raw()) };
    let screen = Obj::screen().expect("Obj::screen (A3)");

    // ── ①a Button / TextButton / Label：创建 + 属性 + 真的被画出来 ──────────
    // 尺寸/坐标是**测试取位**（本层不含 UI 规格值，全部由调用方给）。
    let mut bg = Style::new();
    bg.set_bg_color(Color::rgb(BTN_BG[2], BTN_BG[1], BTN_BG[0]));
    bg.set_bg_opa(Opa::COVER);
    bg.set_radius(0);
    bg.set_pad_all(0);
    let bg = Rc::new(bg);

    let btn = TextButton::create(&screen, "主状态").expect("TextButton::create");
    btn.set_size(80, 80);
    btn.set_pos(0, 0);
    btn.add_style(&bg, StyleSelector::main());
    assert_eq!(btn.text().as_deref(), Some("主状态"), "文字应可读回");
    btn.set_text("配置");
    assert_eq!(btn.text().as_deref(), Some("配置"), "改字应生效");
    refr();
    assert!(
        count_color(&sink.borrow(), BTN_BG) >= 80 * 80 / 2,
        "按钮应真的进入渲染树（底色像素 {}）",
        count_color(&sink.borrow(), BTN_BG)
    );
    assert_eq!(
        btn.size(),
        (80, 80),
        "句柄 Deref 到 Obj ⇒ set_size/size 可用"
    );
    // 组合关系：标签是按钮的子对象 ⇒ 按钮删除时标签随之回收（无孤儿子句柄）。
    assert_eq!(btn.child_count(), 1, "TextButton = 按钮 + 一个子标签");
    assert!(btn.button().is_alive() && btn.label().is_alive());

    // 裸 Button + 裸 Label（步进器 / IPv4 / 日期时间的原件，设计 §5.6 F12）。
    let bare = Button::create(&screen).expect("Button::create");
    bare.set_size(60, 60);
    bare.set_pos(100, 0);
    let value = Label::create_with_text(&bare, "128").expect("Label::create_with_text");
    value.set_size(40, 20);
    value.set_long_mode(LongMode::WRAP);
    assert_eq!(value.text().as_deref(), Some("128"));
    assert_eq!(LongMode::WRAP.raw(), sys::LV_LABEL_LONG_MODE_WRAP as u32);
    // 状态机制（TT-10 按钮禁用 / §6.2 步进器越界共用这一入口）。
    assert!(!widgets::has_state(&bare, State::DISABLED));
    widgets::set_state(&bare, State::DISABLED, true);
    assert!(widgets::has_state(&bare, State::DISABLED));
    widgets::set_state(&bare, State::DISABLED, false);
    assert!(!widgets::has_state(&bare, State::DISABLED));

    // ── ①b List / Table ─────────────────────────────────────────────────
    let list = List::create(&screen).expect("List::create");
    list.set_size(120, 60);
    list.set_pos(0, 90);
    let row = list
        .add_text("2026-09-11 WARN 通道抖动")
        .expect("List::add_text");
    assert_eq!(list.child_count(), 1, "add_text 应产生一个行对象");
    assert_eq!(row.text().as_deref(), Some("2026-09-11 WARN 通道抖动"));

    let table = Table::create(&screen).expect("Table::create");
    table.set_pos(130, 90);
    table.set_size(120, 80);
    table.set_column_count(3);
    table.set_row_count(2);
    table.set_cell(0, 0, "字段");
    table.set_cell(0, 1, "旧值");
    table.set_cell(0, 2, "新值");
    table.set_cell(1, 0, "soc_min");
    table.set_cell(1, 1, "0.10");
    table.set_cell(1, 2, "0.20");
    table.set_column_width(0, 48);
    assert_eq!(table.column_count(), 3);
    assert_eq!(table.row_count(), 2);
    assert_eq!(table.cell(1, 1).as_deref(), Some("0.10"), "单元格应可读回");

    // ── ①c Bar / Led ────────────────────────────────────────────────────
    let bar = Bar::create(&screen).expect("Bar::create");
    bar.set_pos(0, 160);
    bar.set_size(120, 16);
    bar.set_range(0, 100);
    bar.set_mode(BarMode::NORMAL);
    bar.set_value(42, widgets::Anim::OFF);
    assert_eq!(bar.value(), 42, "值应可读回");
    bar.set_start_value(15, widgets::Anim::OFF);
    bar.set_mode(BarMode::RANGE);

    // F14：灯 + 文本并列（`create_with_text` 把"必须并列"变成一处构造）。
    let (led, led_text) = Led::create_with_text(&screen, "已联锁").expect("Led::create_with_text");
    led.set_pos(130, 160);
    led.set_size(24, 24);
    led.set_color(Color::rgb(0xFF, 0x6B, 0x6B));
    led.on();
    // `LV_LED_BRIGHT_MAX` 是 `#define 255`（v9.5.0 `lv_led.h:30`，不经 bindgen 生成）。
    assert_eq!(led.brightness(), 255, "on() 即最大亮度");
    // 亮度被 LVGL 夹在 `LV_LED_BRIGHT_MIN..=LV_LED_BRIGHT_MAX`（v9.5.0 = 80..=255）。
    led.set_brightness(120);
    assert_eq!(led.brightness(), 120);
    led.set_brightness(10);
    assert_eq!(led.brightness(), 80, "低于下限应被夹到 LV_LED_BRIGHT_MIN");
    // `lv_led_toggle` = 在 MIN（灭）与 MAX（亮）之间翻转（不经过 0 —— v9.5.0 `lv_led.c`）。
    led.off();
    assert_eq!(led.brightness(), 80, "off() 即最小亮度（灭）");
    led.toggle();
    assert_eq!(led.brightness(), 255, "toggle 后应为亮");
    assert_eq!(
        led_text.text().as_deref(),
        Some("已联锁"),
        "灯的文本通道必须并列存在"
    );

    // ── ①d Dropdown / Switch / Checkbox / ButtonMatrix ───────────────────
    let dd = Dropdown::create(&screen).expect("Dropdown::create");
    dd.set_pos(150, 0);
    dd.set_size(100, 40);
    dd.set_options("全部\n最近 1 h\n最近 24 h");
    assert_eq!(dd.selected(), 0);
    dd.set_selected(2);
    assert_eq!(dd.selected(), 2, "选中项应可读回");
    dd.set_dir(Dir::BOTTOM);
    dd.set_text("时间范围");

    let sw = Switch::create(&screen).expect("Switch::create");
    sw.set_size(50, 24);
    // 位置只为"不遮住下面要按的按钮"（坐标是测试取位，不是 UI 规格）。
    sw.set_pos(200, 120);
    assert!(!sw.is_checked(), "新建开关默认未勾选");
    sw.set_checked(true);
    assert!(sw.is_checked(), "勾选态 = LV_STATE_CHECKED");

    let cb = Checkbox::create(&screen).expect("Checkbox::create");
    cb.set_pos(200, 150);
    cb.set_text("ERROR");
    cb.set_checked(true);
    assert!(cb.is_checked());

    let bm = ButtonMatrix::create(&screen).expect("ButtonMatrix::create");
    bm.set_size(50, 20);
    bm.set_pos(200, 170);
    // `"\n"` = 换行（LVGL 语义）；地图被 `ButtonMatrix` 锚住（不悬垂）。
    bm.set_map(&["1", "2", "3", "\n", "4", "5", "6"]);
    bm.set_one_checked(true);
    bm.set_selected(4);
    assert_eq!(bm.selected(), Some(4), "选中键应可读回");
    assert_eq!(
        bm.button_text(4).as_deref(),
        Some("5"),
        "地图被锚住且可读回（`\\n` 不占键号）"
    );
    assert_eq!(bm.button_text(2).as_deref(), Some("3"));

    // ── ①e MsgBox（确认弹层底座）────────────────────────────────────────
    let top = widgets::layer_top().expect("layer_top");
    let top_children_before = top.child_count();
    {
        let mbox = MsgBox::modal().expect("MsgBox::modal");
        let title = mbox.add_title("恢复默认值").expect("add_title");
        let text = mbox
            .add_text("全部运行参数将恢复为默认值并立即生效")
            .expect("add_text");
        let cancel = mbox.add_footer_button("取消").expect("add_footer_button");
        let confirm = mbox.add_footer_button("确认").expect("add_footer_button");
        assert!(title.is_alive() && text.is_alive());
        assert!(cancel.is_alive() && confirm.is_alive());
        assert!(mbox.is_alive(), "弹层应存活");

        // TT-09 的底座能力：输入组 + 默认焦点「取消」。
        let group = Group::create().expect("Group::create");
        group.add(&cancel);
        group.add(&confirm);
        group.focus(&cancel);
        assert!(
            widgets::has_state(&cancel, State::FOCUSED),
            "默认焦点应落在「取消」上（LV_EVENT_FOCUSED ⇒ LV_STATE_FOCUSED）"
        );
        group.remove(&confirm);
        drop(group);

        drop(cancel);
        drop(confirm);
        drop(text);
        drop(title);
        let content = mbox.content().expect("MsgBox::content");
        assert!(content.is_alive());
        drop(content);
        mbox.close(); // 关闭：自动遮罩一并销毁
    }
    assert_eq!(
        widgets::layer_top().expect("layer_top").child_count(),
        top_children_before,
        "关闭后自动遮罩不应留在顶层图层上"
    );

    // ── Critical 回归：`lv_msgbox_add_title` 返回**缓存单例** ⇒ 必须非拥有 ────
    // 评审场景复现：拿到标题句柄 → drop → **再次** add_title。若按拥有语义收尾，
    // drop 会 `lv_obj_delete(mbox->title)`，而 `mbox->title` 仍指向已释放块 ⇒ 第二次
    // `lv_label_set_text(悬垂指针)` = **UAF**（评审探针实测：>60 s 挂死，exit 143）。
    // 修正后：drop 不删标题；第二次调用只更新文本，且返回同一底层对象。
    {
        let mbox = MsgBox::modal().expect("MsgBox::modal (title regression)");
        let t1 = mbox.add_title("P").expect("add_title #1");
        let raw1 = t1.raw();
        assert_eq!(label_text(&t1), "P", "首次标题文本应正确");
        drop(t1); // 非拥有 ⇒ 不得删除底层标题对象（UAF 的触发前置）

        let t2 = mbox.add_title("P").expect("add_title #2（评审 UAF 复现点）");
        assert_eq!(t2.raw(), raw1, "LVGL 缓存标题：两次 add_title 应返回同一对象");
        assert!(t2.is_alive(), "第二次 add_title 后标题应存活（未挂/未崩）");
        assert_eq!(label_text(&t2), "P", "第二次 add_title 文本应正确落定");

        let t3 = mbox.add_title("Q").expect("add_title #3");
        assert_eq!(t3.raw(), raw1, "重复调用仍是同一对象");
        assert_eq!(label_text(&t3), "Q", "改标题文本应可读回");
        drop(t3);
        mbox.close();
    }

    // ── Important #2 回归：会话级单例不得每次调用都挂探针（无界增长）───────
    // `Obj::screen()` / `layer_top()` 若每次调用都走 `from_raw`，会对**同一底层对象**
    // 反复挂 DELETE 探针 + `Ctx`（只在对象删除时回收）⇒ 每帧 / 每次弹层无界累积。
    // 修正后句柄由"探针种子"共享派生 ⇒ 32 次调用新增探针应为 0（≤2 容忍偶发重建）。
    {
        let before = super::obj::PROBE_MOUNTS.load(Ordering::SeqCst);
        for _ in 0..32 {
            let s = Obj::screen().expect("Obj::screen (singleton cache)");
            let t = widgets::layer_top().expect("layer_top (singleton cache)");
            assert!(s.is_alive() && t.is_alive(), "单例句柄应存活");
        }
        let mounted = super::obj::PROBE_MOUNTS.load(Ordering::SeqCst) - before;
        assert!(
            mounted <= 2,
            "会话级单例应复用探针：32 次调用新增 {mounted} 条（期望 0，上界 2）"
        );
    }

    // ── Minor #1 回归：`Group::remove` 只影响**本组**（别组对象不得被摘）────
    {
        let g1 = Group::create().expect("Group::create (remove semantics)");
        let g2 = Group::create().expect("Group::create (remove semantics)");
        let member = Button::create(&screen).expect("Button::create (remove semantics)");
        g1.add(&member);
        // 入 g2：LVGL 会先把 member 从 g1 摘掉 ⇒ member 此刻只属 g2。
        g2.add(&member);
        // 关键：对**本组(g1)**调用 remove 移除一个不属它的对象 ⇒ 必须 no-op。
        // 旧实现（无条件 `lv_group_remove_obj(member)`）会摘掉 member 在 g2 的归属。
        g1.remove(&member);
        g2.focus(&member);
        assert!(
            widgets::has_state(&member, State::FOCUSED),
            "remove 不得摘除别组对象（member 应仍属 g2、可被 g2 聚焦）"
        );
        // 正例：对象确属本组时 remove 应生效（聚焦状态随之清除）。
        g2.remove(&member);
        g2.focus(&member);
        assert!(
            !widgets::has_state(&member, State::FOCUSED),
            "对象已移出 g2 后不应再被 g2 聚焦"
        );
        drop(member);
        drop(g2);
        drop(g1);
    }

    // ── Important (d)：`Style::set_width` + `Part::{KNOB, SELECTED}` ─────────
    {
        // 取值与 C 端 `lv_part_t` 逐位一致（评审已核，测试再钉一遍）。
        assert_eq!(Part::KNOB.raw(), sys::LV_PART_KNOB as u32);
        assert_eq!(Part::SELECTED.raw(), sys::LV_PART_SELECTED as u32);
        assert_eq!(Part::KNOB.raw(), 0x030000, "LV_PART_KNOB");
        assert_eq!(Part::SELECTED.raw(), 0x040000, "LV_PART_SELECTED");
        // 已登记的四个取值与原值不冲突（无重复 / 错值）。
        assert_eq!(Part::MAIN.raw(), 0x000000);
        assert_eq!(Part::SCROLLBAR.raw(), 0x010000);
        assert_eq!(Part::INDICATOR.raw(), 0x020000);
        assert_eq!(Part::ITEMS.raw(), 0x050000);
        assert_eq!(Part::ANY.raw(), 0x0F0000);

        // set_width 写回可读回：样式中的 width 参与布局解算 ⇒ 探针对象宽度即为该值。
        let mut width_style = Style::new();
        width_style.set_width(8);
        let width_style = Rc::new(width_style);
        let probe = Obj::create(&screen).expect("Obj::create (width probe)");
        probe.add_style(&width_style, StyleSelector::main());
        refr();
        assert_eq!(probe.size().0, 8, "set_width 应写回并决定对象宽度");
        drop(probe); // 立即回收，避免影响后续命中/像素断言

        // 滚动条部件（§5.6-A 的 8 px 纯指示滚动条即 Part::SCROLLBAR + 本 setter）。
        let mut sb = Style::new();
        sb.set_width(8);
        let sb = Rc::new(sb);
        let holder = ScrollContainer::create(&screen).expect("ScrollContainer::create (sb width)");
        holder.add_style(&sb, StyleSelector::part_of(Part::SCROLLBAR));
        assert!(holder.is_alive());
        drop(holder);
    }

    // ── ③ 长按：入口存在 + 四个事件码可注册 + 离屏推进不崩 ──────────────
    // 排在 ScrollContainer / TabView 之前：那两个会盖住下面的按钮，抢走命中。
    // 长按阈值在 v9 上属 **indev**（逐对象 API 是 v8 产物，v9.5.0 不存在，见 indev.rs 文档）。
    let indev = Indev::create_pointer(&disp).expect("Indev::create_pointer (A3)");
    indev.set_long_press_time(1000); // 阈值由 theme 传入；这里是测试值
    let pressed = Rc::new(Cell::new(0u32));
    let released = Rc::new(Cell::new(0u32));
    let lost = Rc::new(Cell::new(0u32));
    let long = Rc::new(Cell::new(0u32));
    {
        let (p, r, l, g) = (
            pressed.clone(),
            released.clone(),
            lost.clone(),
            long.clone(),
        );
        // A2 的**安全** API：无 unsafe。
        let _sub_p = btn.on(EventCode::PRESSED, move |_e| p.set(p.get() + 1));
        let _sub_r = btn.on(EventCode::RELEASED, move |_e| r.set(r.get() + 1));
        let _sub_l = btn.on(EventCode::PRESS_LOST, move |_e| l.set(l.get() + 1));
        let _sub_g = btn.on(EventCode::LONG_PRESSED, move |_e| g.set(g.get() + 1));
    }
    refr();
    let bc = btn.coords();
    assert_eq!(
        (bc.x1, bc.y1, bc.x2, bc.y2),
        (0, 0, 79, 79),
        "按钮矩形（离屏断言：set_pos/set_size 已落定）"
    );
    let (cx, cy) = ((bc.x1 + bc.x2) / 2, (bc.y1 + bc.y2) / 2);
    indev.feed(TouchSnapshot {
        pressed: true,
        x: cx,
        y: cy,
    });
    indev.read();
    // 离屏推进（虚拟时钟不真等 1.0 s：满 1.0 s 的派发由 §11.1 假 tick 用例覆盖）。
    super::timer_handler();
    indev.feed(TouchSnapshot {
        pressed: false,
        x: cx,
        y: cy,
    });
    indev.read();
    super::timer_handler();
    assert_eq!(pressed.get(), 1, "PRESSED 应派发一次（回调注册生效）");
    assert_eq!(released.get(), 1, "RELEASED 应派发一次");
    assert_eq!(lost.get(), 0, "未滑出控件，不应有 PRESS_LOST");
    assert_eq!(
        long.get(),
        0,
        "未满 1.0 s，不应派发 LONG_PRESSED（框架语义）"
    );

    // ── ② 滚动容器：结构性禁横滚 + 纯指示滚动条 ────────────────────────
    let sc = ScrollContainer::create(&screen).expect("ScrollContainer::create");
    sc.set_pos(0, 0);
    sc.set_size(80, 60);
    assert_eq!(
        sc.scroll_dir(),
        Dir::VER,
        "滚动方向应为垂直（结构性禁横滚）"
    );
    assert_eq!(
        sc.scrollbar_mode(),
        ScrollMode::AUTO,
        "滚动条应为 AUTO（内容超出才出现、纯指示不可拖）"
    );
    assert!(sc.has_flag(ObjFlag::SCROLLABLE), "容器应可滚动");
    // 反向：显式改回水平亦应如实读回（证明不是"读了个常量"）。
    widgets::set_scroll_dir(&sc, Dir::HOR);
    assert_eq!(sc.scroll_dir(), Dir::HOR);
    widgets::set_scroll_dir(&sc, Dir::VER);
    widgets::set_scrollbar_mode(&sc, ScrollMode::OFF);
    assert_eq!(sc.scrollbar_mode(), ScrollMode::OFF);
    widgets::set_scrollbar_mode(&sc, ScrollMode::AUTO);

    // ── ①f TabView（6 页导航候选之一）────────────────────────────────────
    let tabs = TabView::create(&screen).expect("TabView::create");
    tabs.set_size(W as i32, H as i32);
    tabs.set_pos(0, 0);
    let page1 = tabs.add_tab("主状态").expect("TabView::add_tab");
    assert!(page1.is_alive(), "新增页应存活");
    // v9.5.0 无 `LV_TAB_POS_NONE`：隐藏内置标签栏 = 取到标签栏后挂 HIDDEN。
    let bar_obj = tabs.tab_bar().expect("TabView::tab_bar");
    bar_obj.set_hidden(true);
    assert!(bar_obj.is_hidden(), "内置标签栏应可隐藏");
    tabs.set_tab_bar_position(Dir::BOTTOM);
    tabs.set_tab_bar_size(48);
    let page2 = tabs.add_tab("配置").expect("TabView::add_tab");
    assert!(page2.is_alive());
    assert_eq!(
        tabs.content().expect("TabView::content").child_count(),
        2,
        "内容容器应有两页（6 页导航的底座能力）"
    );

    // ── ④ 结构性不暴露三个文本输入控件（源码扫描）─────────────────────
    // 符号名由**拼接**构造：这样本文件自身（与 widgets.rs 同属 `src/**`）不会被
    // 同一条扫描规则误伤（设计 §11.1 静态约束 ① 的扫描范围含 `src/**`）。
    let src = include_str!("widgets.rs").to_ascii_lowercase();
    let forbidden = [
        concat!("lv_", "spin", "box"),
        concat!("lv_", "text", "area"),
        concat!("lv_", "key", "board"),
    ];
    for needle in forbidden {
        assert!(
            !src.contains(needle),
            "widgets.rs 结构性不得出现文本输入控件符号：{needle}"
        );
    }

    // ── Important #2 回归（身份变化路径）：切换默认屏后不得返回过期缓存屏 ──────
    // `lv_display_set_default` 换默认屏后 `lv_screen_active()` 指向**另一个（仍存活的）**
    // 屏幕：仅靠 `is_alive()` 的旧种子会过期 ⇒ 缓存必须比对"当前指针"。放在链路末尾：
    // 切换默认 display 会改变全局默认屏，故避开前面的渲染/命中断言。
    {
        let disp2 = Display::create(W, H).expect("Display::create (default switch)");
        let s_now = Obj::screen().expect("Obj::screen (default switch)");
        assert_ne!(
            s_now.raw(),
            screen.raw(),
            "默认屏切换后 screen() 必须返回新屏（不得返回过期缓存）"
        );
        assert!(s_now.is_alive(), "新默认屏应存活");
        drop(s_now);
        drop(disp2); // 删默认屏：LVGL 回落到另一个 display（本链路的 `disp`）
        assert_eq!(
            Obj::screen().expect("Obj::screen (default revert)").raw(),
            screen.raw(),
            "默认屏回落后 screen() 应再次指向原屏"
        );
    }

    // 释放顺序：先控件树，再 indev / display，最后 deinit（与 A1/A2 同口径）。
    drop(tabs);
    drop(sc);
    drop(bm);
    drop(cb);
    drop(sw);
    drop(dd);
    drop(led_text);
    drop(led);
    drop(bar);
    drop(table);
    drop(list);
    drop(value);
    drop(bare);
    drop(btn);
    drop(indev);
    drop(screen);
    drop(disp);
    super::deinit();
}
