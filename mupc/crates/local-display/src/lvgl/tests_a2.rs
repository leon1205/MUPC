//! 薄层「对象与样式」自测（工作单元 A2：`obj.rs` / `style.rs` / `font.rs`）。
//!
//! # ⚠️ 为什么没有自己的 `#[test]`
//!
//! LVGL 的全局状态**非线程安全**（`lv_conf.h` 的 `LV_USE_OS = LV_OS_NONE`），而
//! `cargo test` 默认**多线程**跑测试函数。A2 的用例因此不另起 `#[test]`，而是由
//! `tests.rs::lvgl_core_bridge_chain`（**唯一的** LVGL `#[test]`）在**同一线程内**顺序调起
//! —— 串行化做法与 A1 完全一致（设计 §11.1；见 `tests.rs` 顶部说明）。
//!
//! # 覆盖（按验收口径逐条）
//!
//! ⑩ [`Obj`] 创建 → 父子 → 换父 → 坐标/尺寸 → 居中（离屏可断言的几何量）；
//! ⑪ [`Style`] **机制层**挂载 → 渲染出该底色（离屏像素断言）；部件/状态选择器可表达；
//! ⑫ 可见性（`HIDDEN` 标志）→ 隐藏后该对象**不再被画**（像素消失）→ 恢复；
//! ⑬ [`Obj::on_clicked`] **安全** API：经 `indev` 喂点驱动 → `LV_EVENT_CLICKED` 达成；
//!    同时验证 `LV_STATE_PRESSED` 选择器的样式在按下期间生效（像素断言）；
//! ⑭ `Obj` `Drop` → 其上闭包**恰好 drop 一次**；[`EventSub::detach`]（**无 unsafe**）同样恰好一次；
//! ⑮ **回调内删除宿主 `Obj`** → 延迟回收生效（回调执行中不得回收）且**回收恰一次**；
//!    宿主失效后 `detach` 自动 no-op（把 A1 `detach` 的 `# Safety` 收进所有权不变量）；
//! ⑯ 字体：10 档契约（档位 = UI §3.3 阶梯）、`noto-font` 取用 / 未启用时的**降级路径**、
//!    内置兜底字体始终可挂进样式；
//! ⑰ 失效句柄的两条来源：**父级联删除**与**世代失配**（`init → deinit → init`）——
//!    失效后读/写/Drop 均为 no-op（防 UAF 与 double free）；其中世代判据用测试专用
//!    伪造句柄**隔离**验证（`alive` 为真、仅世代失配），另覆盖 `Style` 的跨世代失效；
//! ⑱ **Critical 自证**（A2 评审）：`Style` 裸指针的两类 UB 均被消除 —— 反例②（move 后
//!    地址改变）由 `Box<lv_style_t>` 保证地址恒定；反例①（样式先 drop）由 `Obj` 持
//!    共享所有权保证"对象活着时样式不可能被释放"（用 `Weak::strong_count` 直接观测）。
//!
//! 所有断言都在**内存 sink**上完成（同 A1：`Display` 的 flush 桥 → `Vec<u8>`），
//! 与真机 `/dev/fb0` 共用同一条渲染路径（设计 §1.1.1.1 P-1）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use lvgl_sys as sys;

use super::display::{Area, Display, BYTES_PER_PIXEL};
use super::font::{Font, FontSize};
use super::indev::{Indev, TouchSnapshot};
use super::obj::{Obj, ObjFlag};
use super::style::{BorderSide, Color, Opa, Part, State, Style, StyleSelector};
use super::tests::{count_color, DropSpy};

const W: u32 = 256;
const H: u32 = 192;

/// 卡片底色（内存序 `B, G, R` —— XRGB8888；与主题默认色刻意远离，便于像素计数）。
const CARD_BG: [u8; 3] = [0x00, 0x40, 0xE0];
/// 按下态底色（`LV_STATE_PRESSED` 选择器）。
const PRESS_BG: [u8; 3] = [0xE0, 0x00, 0x40];
/// 面板底色（用来验证"我们的样式压过 LVGL 默认主题"）。
const PANEL_BG: [u8; 3] = [0x80, 0x20, 0x00];
/// 场景 ⑱ 的独立底色（与上面各色刻意远离，避免像素计数串色）——均为内存序 `B, G, R`。
const MOVE_BG: [u8; 3] = [0x77, 0x11, 0xEE];
const KEEP_BG: [u8; 3] = [0x22, 0x99, 0x11];
const STALE_BG: [u8; 3] = [0x0A, 0x5A, 0xA5];

/// 场景 ⑮ 的静态探针：**必须用静态**（而非 `Rc<Cell>`），因为要在"回调仍在执行中"那一瞬间
/// 读数 —— 此时若闭包已被释放，`Rc<Cell>` 本身就在已释放内存里（同 A1 场景 ⑦ 的理由）。
static SELF_DELETE_DROPS: AtomicU32 = AtomicU32::new(0);
/// 场景 ⑮：`delete()` 返回**之后**（回调尚未退出）读到的探针计数。
static SELF_DELETE_DROPS_AT_DELETE: AtomicU32 = AtomicU32::new(0);

/// 随闭包 Box 一起 drop 的探针（场景 ⑮）。
struct SelfDeleteSpy;

impl Drop for SelfDeleteSpy {
    fn drop(&mut self) {
        SELF_DELETE_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

/// 场景 ⑭ 的静态探针（`detach` 路径）。
static DETACH_DROPS: AtomicU32 = AtomicU32::new(0);

/// 随闭包 Box 一起 drop 的探针（`detach` 场景）。
struct DetachSpy;

impl Drop for DetachSpy {
    fn drop(&mut self) {
        DETACH_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

/// 场景 ⑲ 的静态探针（`ON(DELETE)` + 回调内 `detach` 自身 ⇒ `reclaim` 双请求窗口）。
static DELETE_DETACH_DROPS: AtomicU32 = AtomicU32::new(0);

/// 随闭包 Box 一起 drop 的探针（`reclaim` 幂等场景）。
struct DeleteDetachSpy;

impl Drop for DeleteDetachSpy {
    fn drop(&mut self) {
        DELETE_DETACH_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

/// 把脏区像素搬进内存 sink（与 A1 用例同构；生产版换成写 `/dev/fb0`）。
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

/// A2 全部场景（由 `tests.rs` 的唯一 LVGL `#[test]` 在同线程内调用）。
pub(super) fn obj_style_font_chain() {
    super::init().expect("lvgl::init (A2)");

    let sink = Rc::new(RefCell::new(vec![0u8; (W * H) as usize * BYTES_PER_PIXEL]));
    let mut disp = Display::create(W, H).expect("Display::create (A2)");
    {
        let sink_w = sink.clone();
        disp.set_flush_cb(move |area: Area, px: &[u8]| blit(&sink_w, area, px));
    }
    // `lv_refr_now` 是测试专用强制渲染（设计 §5.2 不变量 2：生产路径禁用）。
    let refr = || unsafe { sys::lv_refr_now(disp.raw()) };

    // ── ⑩ Obj：创建 / 父子 / 换父 / 坐标 / 尺寸 / 居中 ────────────────────
    let screen = Obj::screen().expect("Obj::screen");
    assert!(screen.is_alive(), "默认屏应存活");

    // 面板：把 pad/border 归零（**本层样式压过 LVGL 默认主题**），使后续居中断言精确。
    let mut panel_style = Style::new();
    panel_style.set_bg_color(Color::rgb(PANEL_BG[2], PANEL_BG[1], PANEL_BG[0]));
    panel_style.set_bg_opa(Opa::COVER);
    panel_style.set_pad_all(0);
    panel_style.set_border_width(0);
    // `Rc` 化 = 交出**共享所有权**（`add_style` 的入参；共享后样式即冻结，见 `style.rs` 文档）。
    let panel_style = Rc::new(panel_style);
    let panel = Obj::create(&screen).expect("Obj::create panel");
    panel.add_style(&panel_style, StyleSelector::main());
    panel.set_pos(20, 20);
    panel.set_size(200, 150);
    assert_eq!(panel.child_count(), 0, "新建对象应无子对象");

    let card = Obj::create(&panel).expect("Obj::create card");
    let other = Obj::create(&panel).expect("Obj::create other");
    assert_eq!(panel.child_count(), 2, "父子关系：面板应有 2 个子对象");
    assert_eq!(screen.child_count(), 1, "屏幕应只有面板这 1 个子对象");

    card.set_pos(10, 20);
    card.set_size(120, 80);
    other.set_pos(110, 120);
    other.set_size(50, 40);
    refr(); // coords/size 由布局趟写入：测试用强制渲染落定（生产由每帧 timer_handler 驱动）

    assert_eq!(card.size(), (120, 80), "尺寸应精确（set_size → size）");
    assert_eq!(other.size(), (50, 40));
    let cc = card.coords();
    let oc = other.coords();
    // 两个兄弟共享同一父内容区原点 ⇒ 其**相对位移**必等于 set_pos 之差（不受主题 padding 影响）。
    assert_eq!(oc.x1 - cc.x1, 100, "相对位移应等于 set_pos 之差（x）");
    assert_eq!(oc.y1 - cc.y1, 100, "相对位移应等于 set_pos 之差（y）");
    assert_eq!(cc.width(), 120, "coords 宽度应等于 set_size");
    assert_eq!(cc.height(), 80);

    // 换父：把 other 挂到 card 下（层级随之变化）。
    other.set_parent(&card);
    assert_eq!(card.child_count(), 1, "换父后 card 应有 1 个子对象");
    assert_eq!(panel.child_count(), 1, "换父后 panel 只剩 card");

    // 居中：面板 pad/border 已归零 ⇒ 内容区 = 面板矩形，居中可精确断言（±2 px 取整容差）。
    card.center();
    refr();
    let cc = card.coords();
    let pc = panel.coords();
    let dx = (cc.x1 + cc.x2) - (pc.x1 + pc.x2);
    let dy = (cc.y1 + cc.y2) - (pc.y1 + pc.y2);
    assert!(
        dx.abs() <= 2 && dy.abs() <= 2,
        "居中后对象中心应落在父内容区中心（偏移 {dx},{dy}）"
    );

    // ── ⑪ Style（机制层）：挂样式 → 渲染出该底色 ─────────────────────────
    // 本层只提供机制，**不含** UI 规格值（那些在 ui/theme.rs）；这里用临时色值验证机制。
    let mut card_style = Style::new();
    card_style.set_bg_color(Color::rgb(CARD_BG[2], CARD_BG[1], CARD_BG[0]));
    card_style.set_bg_opa(Opa::COVER);
    card_style.set_radius(8);
    card_style.set_pad_all(4);
    card_style.set_border_width(1);
    card_style.set_border_color(Color::rgb(0x57, 0x3B, 0x2A));
    card_style.set_border_side(BorderSide::FULL);
    card_style.set_text_color(Color::rgb(0xFF, 0xF7, 0xF4));
    let card_style = Rc::new(card_style);
    card.add_style(&card_style, StyleSelector::main());

    // 状态色 = **另一个 Style + 挂载选择器**（v9 的 `lv_style_set_*` 无 selector 参数）。
    let mut press_style = Style::new();
    press_style.set_bg_color(Color::rgb(PRESS_BG[2], PRESS_BG[1], PRESS_BG[0]));
    press_style.set_bg_opa(Opa::COVER);
    // 同一份 `Rc<Style>` 稍后还要挂到 `btn` 上 —— 共享所有权天然支持"一个样式多处挂载"。
    let press_style = Rc::new(press_style);
    card.add_style(&press_style, StyleSelector::state_of(State::PRESSED));

    // 部件选择器：滚动条部件宽度（UI §5.3 的 8 px 纯指示滚动条即挂 `LV_PART_SCROLLBAR`）。
    let mut scrollbar_style = Style::new();
    scrollbar_style.set_pad_all(0);
    let scrollbar_style = Rc::new(scrollbar_style);
    card.add_style(&scrollbar_style, StyleSelector::part_of(Part::SCROLLBAR));
    // 选择器组合语义（纯 Rust 断言，不需要渲染）：部件 | 状态。
    assert_eq!(
        StyleSelector::new(Part::ITEMS, State::CHECKED).to_sys(),
        Part::ITEMS.raw() | State::CHECKED.raw(),
        "选择器 = 部件 | 状态"
    );
    assert_eq!(StyleSelector::main().to_sys(), 0, "MAIN|DEFAULT 即 C 侧的 0");

    refr();
    {
        let s = sink.borrow();
        assert!(
            count_color(&s, CARD_BG) >= 120 * 80 / 2,
            "挂上样式后应渲染出该底色（实际 {} px）",
            count_color(&s, CARD_BG)
        );
    }

    // ── ⑫ 可见性：隐藏后不再被画，恢复后重新出现 ─────────────────────────
    card.set_hidden(true);
    assert!(card.is_hidden(), "隐藏后 is_hidden 应为真");
    refr();
    assert_eq!(
        count_color(&sink.borrow(), CARD_BG),
        0,
        "隐藏（HIDDEN 标志）后该对象不应再被画（原区域被父底色覆盖）"
    );
    card.set_hidden(false);
    assert!(!card.is_hidden());
    refr();
    assert!(
        count_color(&sink.borrow(), CARD_BG) > 0,
        "取消隐藏后应恢复渲染"
    );

    // 标志机制（A3 会用到的三个标志也走同一 API；此处只验读写一致）。
    assert!(!card.has_flag(ObjFlag::CHECKABLE), "新建对象默认不可勾选");
    card.add_flag(ObjFlag::CHECKABLE);
    assert!(card.has_flag(ObjFlag::CHECKABLE));
    card.remove_flag(ObjFlag::CHECKABLE);
    assert!(!card.has_flag(ObjFlag::CHECKABLE));

    // ── ⑬ 安全 `on_clicked`：经 indev 喂点驱动 → CLICKED ─────────────────
    let btn = Obj::create(&screen).expect("Obj::create btn");
    let mut btn_style = Style::new();
    btn_style.set_bg_color(Color::rgb(CARD_BG[2], CARD_BG[1], CARD_BG[0]));
    btn_style.set_bg_opa(Opa::COVER);
    btn_style.set_pad_all(0);
    let btn_style = Rc::new(btn_style);
    btn.add_style(&btn_style, StyleSelector::main());
    btn.add_style(&press_style, StyleSelector::state_of(State::PRESSED));
    btn.set_pos(0, 0);
    btn.set_size(80, 80);
    refr();

    let hit = Rc::new(Cell::new(false));
    let hit_w = hit.clone();
    // **安全 API**（无 `unsafe`）：内部把宿主裸指针的有效性收进 `Obj` 的所有权不变量。
    let _sub = btn.on_clicked(move |_e| hit_w.set(true));

    let indev = Indev::create_pointer(&disp).expect("Indev::create_pointer (A2)");
    // 按点取按钮**实际**矩形中心（规避主题 padding 对绝对坐标的影响）。
    let bc = btn.coords();
    let (px, py) = ((bc.x1 + bc.x2) / 2, (bc.y1 + bc.y2) / 2);

    indev.feed(TouchSnapshot {
        pressed: true,
        x: px,
        y: py,
    });
    indev.read();
    refr();
    assert!(!hit.get(), "仅按下不应派发 CLICKED");
    assert!(
        count_color(&sink.borrow(), PRESS_BG) > 0,
        "按下期间 `LV_STATE_PRESSED` 选择器的样式应生效（状态色机制）"
    );

    indev.feed(TouchSnapshot {
        pressed: false,
        x: px,
        y: py,
    });
    indev.read();
    // 与 A1 场景 ④ 同法：`lv_indev_read()` 一次即完成 press+release 判定并派发 CLICKED。
    assert!(
        hit.get(),
        "按下+抬起落在同一控件应派发 CLICKED（安全 on_clicked 通了）"
    );
    refr();
    assert_eq!(
        count_color(&sink.borrow(), PRESS_BG),
        0,
        "抬起后不应再是按下态"
    );

    // ── ⑭ `Obj` Drop → 回调恰好 drop 一次；detach 同样恰好一次 ───────────
    let drops = Rc::new(Cell::new(0u32));
    {
        let target = Obj::create(&screen).expect("Obj::create target");
        let spy = DropSpy(drops.clone());
        {
            let _sub = target.on_clicked(move |_e| {
                let _keep_alive = &spy;
            });
            assert_eq!(drops.get(), 0, "仅挂载不应 drop");
        } // 订阅在此**自然出作用域**（丢弃）：闭包仍归 LVGL 事件项所有
        assert_eq!(
            drops.get(),
            0,
            "丢弃订阅不应释放闭包（所有权在事件项上，`EventSub` 无 `Drop` 副作用）"
        );
        drop(target); // `Obj::drop` → lv_obj_delete → DELETE → 桥回收
    }
    assert_eq!(
        drops.get(),
        1,
        "对象 Drop 后其上闭包应恰好 drop 一次（不泄漏、不 double free）"
    );

    // detach 路径（**安全** API，无 unsafe）：摘除即释放，且对象删除时不得二次回收。
    assert_eq!(DETACH_DROPS.load(Ordering::SeqCst), 0, "前置：静态探针应为 0");
    let target2 = Obj::create(&screen).expect("Obj::create target2");
    {
        let spy = DetachSpy;
        let sub = target2.on_clicked(move |_e| {
            let _keep_alive = &spy;
        });
        sub.detach();
        assert_eq!(
            DETACH_DROPS.load(Ordering::SeqCst),
            1,
            "detach 应恰好释放闭包一次"
        );
    }
    drop(target2);
    assert_eq!(
        DETACH_DROPS.load(Ordering::SeqCst),
        1,
        "detach 之后对象删除不得二次回收（不 double free）"
    );

    // ── ⑮ **回调内删除宿主 Obj**：延迟回收（A1 的机制在 `Obj` 层仍然成立）──
    // 宿主由 `Rc<RefCell<Option<Obj>>>` 持有，回调里把它取出并 `delete()` —— 这正是
    // A3/B 页面"点一下就把自己拆掉"的常态路径。
    assert_eq!(SELF_DELETE_DROPS.load(Ordering::SeqCst), 0, "前置：静态探针应为 0");
    let slot: Rc<RefCell<Option<Obj>>> = Rc::new(RefCell::new(None));
    let host = Obj::create(&screen).expect("Obj::create host");
    // 显式派发事件需要一个句柄：`Obj` 把它自己移进 `slot`，这里先留一份裸指针
    //（`raw()` 是 `pub(crate)`，测试在 `src/lvgl/**` 内，符合 unsafe 边界纪律）。
    let host_raw = host.raw();
    *slot.borrow_mut() = Some(host);
    let slot_w = slot.clone();
    {
        let spy = SelfDeleteSpy;
        let borrow = slot.borrow();
        let _sub = borrow
            .as_ref()
            .expect("host 在 slot 内")
            .on_clicked(move |_e| {
                let _keep_alive = &spy;
                // 在自己的（非 DELETE）回调里删除宿主 —— 延迟回收的常态路径。
                if let Some(o) = slot_w.borrow_mut().take() {
                    o.delete();
                }
                // 删除之后闭包仍须存活才能执行到这里：延迟回收成立 ⇒ 探针尚未 drop。
                SELF_DELETE_DROPS_AT_DELETE
                    .store(SELF_DELETE_DROPS.load(Ordering::SeqCst), Ordering::SeqCst);
            });
        drop(borrow);
        // SAFETY: `host_raw` 此刻仍是活对象；显式派发 CLICKED（与 A1 场景 ⑦ 同法）。
        unsafe { sys::lv_obj_send_event(host_raw, sys::LV_EVENT_CLICKED, std::ptr::null_mut()) };
    }
    assert_eq!(
        SELF_DELETE_DROPS_AT_DELETE.load(Ordering::SeqCst),
        0,
        "回调内删除宿主：DELETE 到达时闭包仍在执行，不得立即回收（否则 UAF）"
    );
    assert_eq!(
        SELF_DELETE_DROPS.load(Ordering::SeqCst),
        1,
        "外层回调退出后应恰好回收一次（不泄漏、不 double free）"
    );
    assert!(slot.borrow().is_none(), "宿主已被回调删除");

    // ── ⑯ 字体：10 档契约 / 取用 / 降级路径 ──────────────────────────────
    assert_eq!(FontSize::ALL.len(), 10, "档位 = UI §3.3 阶梯的 10 档");
    let pxs: Vec<u32> = FontSize::ALL.iter().map(|s| s.px()).collect();
    assert_eq!(
        pxs,
        vec![24, 26, 28, 32, 48, 56, 64, 96, 112, 148],
        "档位取值应与 UI §3.3 / 设计 §1.1.2 逐档一致"
    );
    #[cfg(feature = "noto-font")]
    {
        for size in FontSize::ALL {
            assert!(Font::of(size).is_some(), "{size:?} 档应可取到（noto-font 已启用）");
        }
    }
    #[cfg(not(feature = "noto-font"))]
    {
        assert!(
            Font::of(FontSize::S32).is_none(),
            "未启用 noto-font ⇒ Font::of 一律 None（降级路径；ui 侧据此回落）"
        );
    }
    // 兜底字体（LVGL 内置 montserrat_14）**始终**可用，且可挂进样式 → 应用到对象。
    let fallback = Font::fallback();
    let mut text_style = Style::new();
    text_style.set_text_font(&fallback);
    text_style.set_text_color(Color::rgb(0xF4, 0xF7, 0xFF));
    let text_style = Rc::new(text_style);
    btn.add_style(&text_style, StyleSelector::main());
    refr();

    // ── ⑰ 失效句柄的两种来源：父级联删除 / 世代失配（防 UAF 与 double free）──
    // 级联：父被删 → 子随 LVGL 一起删；此时 display 仍在，只有 `lv_obj_is_valid` 能识别。
    let parent = Obj::create(&screen).expect("Obj::create parent");
    let orphan = Obj::create(&parent).expect("Obj::create orphan");
    assert!(orphan.is_alive());
    parent.delete(); // 显式删除（消费父句柄）
    assert!(
        !orphan.is_alive(),
        "父被删后子句柄应判失效（级联删除；`lv_obj_is_valid`）"
    );
    orphan.set_pos(0, 0); // 失效 ⇒ no-op（不得触已释放内存）
    orphan.set_size(10, 10);
    assert_eq!(orphan.size(), (0, 0), "失效句柄的读取应给安全默认值");
    drop(orphan); // 失效 ⇒ Drop 为 no-op（否则对已释放对象二次 delete）
    assert_eq!(screen.child_count(), 2, "父删除后屏幕应只剩 panel 与 btn");

    // ── ⑱ **Critical 自证**：消除 `Style` 裸指针的两类 UB ────────────────
    // 反例②（move 后地址改变）：`lv_style_t` 装在 `Box` 里 ⇒ 多次移动后 LVGL 读到的仍是
    // 同一份数据。若退回"内联在 `Style` 里"，下面每个地址断言都会立刻变红。
    {
        let mut mv_style = Style::new();
        mv_style.set_bg_color(Color::rgb(MOVE_BG[2], MOVE_BG[1], MOVE_BG[0]));
        mv_style.set_bg_opa(Opa::COVER);
        mv_style.set_pad_all(0);
        mv_style.set_border_width(0);
        let mv_addr = mv_style.raw() as usize;
        // 移动①：栈上移动（`fn theme() -> Style { ..; s }` 的返回即移动）。
        let mv_moved = mv_style;
        assert_eq!(
            mv_moved.raw() as usize,
            mv_addr,
            "反例②：`Style` 栈上 move 后 lv_style_t 地址必须不变"
        );
        // 移动②：装箱再移动。
        let mv_boxed = Box::new(mv_moved);
        assert_eq!(
            mv_boxed.raw() as usize,
            mv_addr,
            "反例②：装箱 move 后地址仍须不变"
        );
        // 移动③：进 `Rc`（共享所有权载体，`add_style` 的入参）。
        let mv_rc = Rc::new(*mv_boxed);
        assert_eq!(mv_rc.raw() as usize, mv_addr, "反例②：进 `Rc` 后地址仍须不变");
        let mv_obj = Obj::create(&screen).expect("Obj::create (move proof)");
        mv_obj.set_pos(0, 0);
        mv_obj.set_size(60, 60);
        mv_obj.add_style(&mv_rc, StyleSelector::main());
        refr();
        assert!(
            count_color(&sink.borrow(), MOVE_BG) > 0,
            "反例②：被移动过的 Style 挂载后仍渲染出该底色（LVGL 读到的是同一份数据）"
        );
        drop(mv_obj);

        // 反例①（样式先 drop → 对象持悬垂指针）：`add_style` 交出共享所有权、由本层持住
        // ⇒ 用户句柄 drop 后样式仍活、仍能渲染。用 `Weak::strong_count` **直接**观测
        // 共享所有权（比像素断言更早、更确定地失败）。
        let mut keep_style = Style::new();
        keep_style.set_bg_color(Color::rgb(KEEP_BG[2], KEEP_BG[1], KEEP_BG[0]));
        keep_style.set_bg_opa(Opa::COVER);
        keep_style.set_pad_all(0);
        keep_style.set_border_width(0);
        let keep_rc = Rc::new(keep_style);
        let keep_weak = Rc::downgrade(&keep_rc);
        let keep_obj = Obj::create(&screen).expect("Obj::create (keepalive proof)");
        keep_obj.set_pos(0, 0);
        keep_obj.set_size(60, 60);
        keep_obj.add_style(&keep_rc, StyleSelector::main());
        assert!(
            keep_weak.strong_count() >= 2,
            "反例①：`add_style` 后必须存在第二份强引用（由 Obj/事件项持住），实际 {}",
            keep_weak.strong_count()
        );
        drop(keep_rc); // 用户句柄先 drop（= 反例① 的 `{ let mut s = …; }` 作用域结束）
        assert!(
            keep_weak.strong_count() >= 1,
            "反例①：用户句柄 drop 后样式不得被释放（对象仍活着）"
        );
        refr();
        assert!(
            count_color(&sink.borrow(), KEEP_BG) > 0,
            "反例①：用户句柄 drop 后对象仍能渲染出该底色（样式未被 reset）"
        );
        drop(keep_obj); // 对象删除 ⇒ 共享所有权随之释放（不泄漏）
        assert_eq!(
            keep_weak.strong_count(),
            0,
            "对象删除后其样式的共享所有权应全部释放（不泄漏）"
        );

        // 非拥有句柄（`Obj::screen()`）：共享所有权的锚点是**底层 LVGL 对象的事件项**，
        // 不是 Rust 句柄 ⇒ 句柄先 drop 也不会让屏幕上的样式悬垂（空样式，只验所有权）。
        let scr_rc = Rc::new(Style::new());
        let scr_weak = Rc::downgrade(&scr_rc);
        let scr = Obj::screen().expect("Obj::screen (keepalive proof)");
        scr.add_style(&scr_rc, StyleSelector::main());
        assert!(scr_weak.strong_count() >= 2, "屏幕句柄也应登记共享所有权");
        drop(scr); // `owns = false` ⇒ 底层屏幕仍在（LVGL 拥有）
        assert!(
            scr_weak.upgrade().is_some(),
            "非拥有句柄 drop 后样式仍须存活（屏幕仍引用它）"
        );
    }

    // ── ⑲ **Important 自证**：`reclaim` 幂等（DELETE 回调内 `detach` 自身不得 double free）──
    // 该场景下同一个 `Ctx` 会在**同一延迟窗口**内被两条路径各请求回收一次：DELETE 分支
    // 末尾的 `reclaim` + `CallbackHandle::detach` 内部的 `reclaim`。这里刻意走**裸**
    // `event::on`（`CallbackHandle::detach` 不做存活判定，不依赖 `Obj` 存活探针那条
    // 注册顺序）⇒ "幂等去重"是唯一保障。
    assert_eq!(DELETE_DETACH_DROPS.load(Ordering::SeqCst), 0, "前置：静态探针应为 0");
    let host9 = Obj::create(&screen).expect("Obj::create (delete-detach)");
    let host9_raw = host9.raw();
    let slot9: Rc<RefCell<Option<super::event::CallbackHandle<*mut sys::lv_obj_t>>>> =
        Rc::new(RefCell::new(None));
    let slot9_w = slot9.clone();
    let spy9 = DeleteDetachSpy;
    // SAFETY: `host9_raw` 是刚创建、存活的对象（`event::on` 的宿主有效性前置条件）。
    let h9 = unsafe {
        super::event::on(host9_raw, super::event::EventCode::DELETE, move |_e| {
            let _keep_alive = &spy9;
            if let Some(h) = slot9_w.borrow_mut().take() {
                // 在自己的 **DELETE** 回调里摘除自己 —— 双回收窗口的触发点。
                // SAFETY（由外层 `unsafe` 块承担）：宿主此刻正被 LVGL 删除，裸指针在
                // 本次删除流程内仍有效。
                h.detach();
            }
        })
    };
    *slot9.borrow_mut() = Some(h9);
    drop(host9); // `Obj::drop` → lv_obj_delete → DELETE → 回调内 detach + 桥的 reclaim
    assert_eq!(
        DELETE_DETACH_DROPS.load(Ordering::SeqCst),
        1,
        "DELETE 回调内 detach：闭包仍须恰好释放一次（`reclaim` 幂等 ⇒ 不 double free）"
    );

    // 释放顺序：先对象树（各自的回调随之回收），再 display / indev，最后 deinit。
    drop(btn);
    drop(other);
    drop(card);
    drop(panel);
    drop(screen);
    drop(indev);
    drop(disp);

    // ── ⑰（续）世代令牌：**隔离**判据 + 真实级联路径 + 样式跨世代失效 ────
    // (b) 真实路径的准备：造一个上一世代的 display/对象，随后跨 `deinit → init`。
    let mut stale_style = Style::new();
    stale_style.set_bg_color(Color::rgb(STALE_BG[2], STALE_BG[1], STALE_BG[0]));
    stale_style.set_bg_opa(Opa::COVER);
    let stale_obj = {
        let d = Display::create(W, H).expect("Display::create (A2 stale)");
        let s = Obj::screen().expect("Obj::screen (A2 stale)");
        let o = Obj::create(&s).expect("Obj::create (A2 stale)");
        drop(s); // 非拥有句柄：屏幕归 LVGL
        drop(d); // display 删除 → 屏幕与其子对象随之删除（级联 DELETE）
        o
    };
    super::deinit();
    super::init().expect("lvgl::init (A2, gen C)");

    let disp2 = Display::create(W, H).expect("Display::create (A2 gen C)");
    {
        let s2 = Obj::screen().expect("Obj::screen (A2 gen C)");
        let o2 = Obj::create(&s2).expect("Obj::create (A2 gen C)");
        assert!(o2.is_alive(), "本世代对象应存活");

        // (a) **隔离**验证世代判据：`alive` 为真、底层对象健在，**只有世代失配** ⇒ 必须判失效。
        //     场景 ⑰ 的级联删除会先经 DELETE 探针把 `alive` 置假（两个判据同时为假），
        //     证明不了"世代这一项是必要的"；故用测试专用的伪造句柄把世代变成唯一变量。
        let forged = o2.stale_generation_handle();
        assert!(
            !forged.is_alive(),
            "隔离验证世代判据：`alive` 为真、底层对象健在，仅世代失配 ⇒ 必须判失效"
        );
        forged.set_pos(3, 3); // 失效 ⇒ no-op（不得触活对象）
        assert_eq!(forged.size(), (0, 0));
        assert_eq!(forged.coords().width(), 0, "失效句柄 coords 应为 0×0");
        drop(forged); // 非拥有伪造句柄 ⇒ Drop no-op（不得删掉真对象）
        assert!(o2.is_alive(), "伪造句柄的读写/Drop 不得影响真实对象");

        // (c) 上一世代的**样式**：`is_live()` 为假 ⇒ `add_style` 必须拒绝 —— 用"共享所有权
        //     是否被登记"**确定性地**观测（不依赖"读已失效内存"的 UB 恰好被检出）。
        let stale_rc = Rc::new(stale_style);
        let stale_weak = Rc::downgrade(&stale_rc);
        assert!(!stale_rc.is_live(), "上一世代样式应判失效（世代令牌）");
        assert_eq!(stale_weak.strong_count(), 1, "前置：只有用户句柄这一份强引用");
        o2.add_style(&stale_rc, StyleSelector::main());
        assert_eq!(
            stale_weak.strong_count(),
            1,
            "上一世代的样式不得被 add_style 接受（否则 LVGL 持悬垂属性表指针）"
        );
        drop(stale_rc);
        drop(o2);
        drop(s2);
    }
    drop(disp2);

    // (b) 上一世代的对象：级联删除（`alive = false`）**且**世代失配 —— 防 double free。
    assert!(
        !stale_obj.is_alive(),
        "上一世代的对象在重新 init 后必须判为失效（世代令牌，防 double free）"
    );
    drop(stale_obj); // 失效 ⇒ Drop 为 no-op（否则对已释放对象二次 delete）
    super::deinit();
}
