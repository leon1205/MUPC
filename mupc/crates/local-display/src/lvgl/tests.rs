//! 薄层「核心桥」自测（工作单元 A1；设计 §11.1「HMI 离屏渲染」的可测性支点）。
//!
//! # ⚠️ 为什么只有一个 `#[test]`
//!
//! LVGL 的全局状态**非线程安全**（`lv_conf.h` 的 `LV_USE_OS = LV_OS_NONE`），而
//! `cargo test` 默认**多线程**跑测试函数 —— 多个用例并行触碰 LVGL 会互相踩踏
//! （间歇性崩溃而非稳定失败）。这里用**零依赖**的串行化手段：文件内只放**一个**
//! `#[test]`，四个场景在同一线程内顺序执行。
//!
//! # 为什么放在 `src/lvgl/` 而不是 `tests/`
//!
//! 用例需要用 `lvgl_sys` 的裸符号搭最小控件树，而 `unsafe` **只允许**出现在
//! `lvgl-sys` 与 `src/lvgl/**`（设计 §1.1.1.2 纪律 1）。放 `tests/` 集成测试目录会
//! 在允许边界之外引入 `unsafe` + `lvgl_sys`，故随薄层放在 `src/lvgl/tests.rs`。
//!
//! # 覆盖（设计 §11.1）
//!
//! ① init → display → PARTIAL 双缓冲 → flush 全链，且内存 sink 里出现非背景像素；
//! （场景 ⑩–⑲ 属工作单元 **A2**，见 `tests_a2.rs` —— 仍由本函数在同线程内顺序驱动）
//! ② 回调桥的 `user_data` 在对象删除时**恰好 drop 一次**（不泄漏、不 double free）；
//! ③ 回调内 panic 被拦在桥内、不跨 FFI 展开（跨 FFI 展开 = UB）；
//! ④ indev 的 read_cb 桥喂入坐标快照 → 由 LVGL 完成命中并派发 `LV_EVENT_CLICKED`；
//! ⑤ display 的 flush sink 闭包在 display 删除时**恰好 drop 一次**（`DropSpy` 观测）；
//! ⑥ indev 的快照在 indev 删除时回收路径**恰好执行一次**（`SNAP_RECLAIMS` 计数；
//!    快照是 `Copy` 无 `Drop`，`DropSpy` 不适用 —— 见 `indev.rs` 该静态的说明）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use lvgl_sys as sys;

use super::display::{Area, Display, BYTES_PER_PIXEL, PARTIAL_ROWS_DIVISOR};
use super::event::{self, EventCode};
use super::indev::{Indev, TouchSnapshot};

const W: u32 = 256;
const H: u32 = 192;

/// 屏幕 / 卡片 / 按钮底色（内存序 `B, G, R` —— XRGB8888）。
const SCREEN_BG: [u8; 3] = [0x18, 0x10, 0x10];
const CARD_BG: [u8; 3] = [0x3D, 0x2B, 0x22];
const BTN_BG: [u8; 3] = [0x00, 0x00, 0xFF];

pub(super) fn lv_color(c: [u8; 3]) -> sys::lv_color_t {
    sys::lv_color_t {
        blue: c[0],
        green: c[1],
        red: c[2],
    }
}

/// 不透明纯色底色（`selector = 0` = `LV_PART_MAIN | LV_STATE_DEFAULT`）。
pub(super) unsafe fn set_bg(obj: *mut sys::lv_obj_t, c: [u8; 3]) {
    sys::lv_obj_set_style_bg_color(obj, lv_color(c), 0);
    sys::lv_obj_set_style_bg_opa(obj, 255, 0);
}

/// 容差内统计某底色的像素数（LVGL 纯色填充应当精确，容差只为免于取整噪音）。
///
/// `pub(super)`：A2 的 `tests_a2.rs` 复用（同一条离屏断言口径，避免两套判据漂移）。
pub(super) fn count_color(sink: &[u8], c: [u8; 3]) -> usize {
    sink.chunks_exact(BYTES_PER_PIXEL)
        .filter(|px| (0..3).all(|i| (px[i] as i32 - c[i] as i32).abs() <= 4))
        .count()
}

/// 在对象删除时自增计数的探针：用来**观测**闭包捕获物是否真的被 drop。
pub(super) struct DropSpy(pub(super) Rc<Cell<u32>>);

impl Drop for DropSpy {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

/// 场景 ⑦ 的静态探针计数：**必须用静态**（而非 `Rc<Cell>`），因为要在"回调仍在执行
/// 中"的那一瞬间读取计数 —— 此时若闭包已被释放，`Rc<Cell>` 本身就在已释放内存里。
static CB_SELF_DELETE_DROPS: AtomicU32 = AtomicU32::new(0);
/// 场景 ⑦：在 `lv_obj_delete(host)` 返回**之后**（回调尚未退出）读到的探针计数。
static CB_SELF_DELETE_DROPS_AT_DELETE: AtomicU32 = AtomicU32::new(0);

/// 随闭包 Box 一起 drop 的静态探针（无捕获状态，Drop 即自增静态计数）。
struct StaticSpy;

impl Drop for StaticSpy {
    fn drop(&mut self) {
        CB_SELF_DELETE_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

/// 场景 ⑧ 的静态探针计数（`detach` 在回调内调用）。
static CB_DETACH_DROPS: AtomicU32 = AtomicU32::new(0);

/// 随闭包 Box 一起 drop 的静态探针（`detach` 场景）。
struct DetachSpy;

impl Drop for DetachSpy {
    fn drop(&mut self) {
        CB_DETACH_DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn lvgl_core_bridge_chain() {
    super::init().expect("lvgl::init");

    // ── 场景 ① init → display → PARTIAL 双缓冲 → flush 全链 ──────────────
    let sink = Rc::new(RefCell::new(vec![0u8; (W * H) as usize * BYTES_PER_PIXEL]));
    let sink_w = sink.clone();
    let mut disp = Display::create(W, H).expect("Display::create");
    assert_eq!(disp.width(), W);
    assert_eq!(disp.height(), H);
    // 设计 §10：PARTIAL 双缓冲各 1/10 屏（除数引用 `display.rs` 的**单一真源**常量）。
    assert_eq!(disp.buffer_rows(), H / PARTIAL_ROWS_DIVISOR);
    disp.set_flush_cb(move |area: Area, px: &[u8]| {
        // 生产版把这段换成 FbCanvas 写 /dev/fb0；逻辑同构（设计 §1.1.1.1 P-1）。
        let mut s = sink_w.borrow_mut();
        let row_bytes = area.width() as usize * BYTES_PER_PIXEL;
        for row in 0..area.height() as usize {
            let dy = area.y1 as usize + row;
            let dx = area.x1 as usize;
            let off = (dy * W as usize + dx) * BYTES_PER_PIXEL;
            let src = &px[row * row_bytes..(row + 1) * row_bytes];
            s[off..off + row_bytes].copy_from_slice(src);
        }
    });

    let screen = unsafe { sys::lv_screen_active() };
    assert!(!screen.is_null(), "默认屏应存在");
    unsafe { set_bg(screen, SCREEN_BG) };
    let card = unsafe { sys::lv_obj_create(screen) };
    unsafe {
        set_bg(card, CARD_BG);
        sys::lv_obj_set_size(card, 120, 80);
        sys::lv_obj_center(card);
    }

    // 正规驱动：返回值即"距下次需要处理 ms"（事件循环拿它当 poll 超时上界）。
    // ⚠️ `lv_refr_now()` 只许测试用（设计 §5.2 不变量 2），生产由 timer_handler 驱动。
    let next_ms = super::timer_handler();
    assert!(
        next_ms <= 1000,
        "timer_handler 应给出毫秒级间隔（实际 {next_ms}）"
    );
    unsafe { sys::lv_refr_now(disp.raw()) };

    {
        let s = sink.borrow();
        let card_px = count_color(&s, CARD_BG);
        let screen_px = count_color(&s, SCREEN_BG);
        assert!(
            card_px > 0,
            "flush 后 sink 里应有卡片底色像素（实际 {card_px}）"
        );
        assert!(
            screen_px > 0,
            "flush 后 sink 里应有屏幕底色像素（实际 {screen_px}）"
        );
        // 卡片在屏幕正中 120×80 ⇒ 内部填充区因默认主题的圆角/描边略小于外框面积，
        // 这里只做"确实大面积画上了"的量级断言。
        assert!(
            card_px >= 120 * 80 / 2,
            "卡片底色像素数应达到卡片面积量级（实际 {card_px}）"
        );
    }

    // ── 场景 ② user_data 生命周期：对象删除时恰好 drop 一次 ─────────────
    let drops = Rc::new(Cell::new(0u32));
    let obj = unsafe { sys::lv_obj_create(screen) };
    let spy = DropSpy(drops.clone());
    // SAFETY: `obj` 是刚 `lv_obj_create` 的活对象。
    let _h = unsafe {
        event::on(obj, EventCode::CLICKED, move |_e| {
            let _keep_alive = &spy;
        })
    };
    assert_eq!(drops.get(), 0, "仅挂载不应 drop");
    unsafe { sys::lv_obj_delete(obj) };
    assert_eq!(
        drops.get(),
        1,
        "对象删除时应恰好 drop 一次（不泄漏、不 double free）"
    );

    // ── 场景 ③ 回调内 panic 不得跨 FFI 展开 ─────────────────────────────
    let fired = Rc::new(Cell::new(false));
    let fired_w = fired.clone();
    let spy2 = DropSpy(drops.clone());
    let obj_panic = unsafe { sys::lv_obj_create(screen) };
    // SAFETY: `obj_panic` 是刚 `lv_obj_create` 的活对象。
    let _h2 = unsafe {
        event::on(obj_panic, EventCode::CLICKED, move |_e| {
            let _keep_alive = &spy2;
            fired_w.set(true);
            panic!("故意 panic：必须被桥 catch_unwind 拦住，不得跨 FFI 展开");
        })
    };
    // 直接派发 CLICKED（不经 indev）：若 panic 逃逸，本进程即 UB/崩溃。
    // 注：下面的 `故意 panic` 是**预期输出** —— 默认 panic hook 会照常打印，但展开被
    // `catch_unwind` 拦住，随后的 `EPRINTLN 已被拦截` 与本测试继续通过即为证明。
    unsafe {
        sys::lv_obj_send_event(obj_panic, sys::LV_EVENT_CLICKED, std::ptr::null_mut());
    }
    assert!(fired.get(), "闭包应确实被调用过");
    // 桥在 panic 之后仍然自洽：删除对象时照样回收 user_data。
    unsafe { sys::lv_obj_delete(obj_panic) };
    assert_eq!(
        drops.get(),
        2,
        "panic 过的闭包同样在宿主删除时被回收（桥未失效）"
    );

    // ── 场景 ④ indev read_cb 桥：坐标快照 → LVGL 命中 → CLICKED ─────────
    let btn = unsafe { sys::lv_obj_create(screen) };
    unsafe {
        set_bg(btn, BTN_BG);
        sys::lv_obj_set_pos(btn, 0, 0);
        sys::lv_obj_set_size(btn, 48, 48);
    }
    let hit = Rc::new(Cell::new(false));
    let hit_w = hit.clone();
    // SAFETY: `btn` 是刚 `lv_obj_create` 的活对象。
    let _h3 = unsafe { event::on(btn, EventCode::CLICKED, move |_e| hit_w.set(true)) };

    // 命中判定读的是 `obj->coords`，而它由布局/刷新趟（`lv_display_refr_timer`）写入。
    // 生产由每拍 `timer_handler()` 落定；这里用测试专用强制渲染把布局坐实。
    unsafe { sys::lv_refr_now(disp.raw()) };

    let indev = Indev::create_pointer(&disp).expect("Indev::create_pointer");
    indev.feed(TouchSnapshot {
        pressed: true,
        x: 24,
        y: 24,
    });
    indev.read();
    assert!(!hit.get(), "仅按下不应派发 CLICKED");
    indev.feed(TouchSnapshot {
        pressed: false,
        x: 24,
        y: 24,
    });
    indev.read();
    assert!(
        hit.get(),
        "按下+抬起落在同一控件应派发 CLICKED（read_cb 桥 → LVGL 命中通了）"
    );

    // ── 场景 ⑤ flush sink 闭包：display 删除时恰好 drop 一次 ─────────────
    // `display.rs` 的 `set_flush_cb` 用 `Box::into_raw` 把 sink 交给 DELETE 回调回收
    // （与 `event::on` 同一套 `user_data` 契约，见设计 §1.1.1.2 纪律 3）。把 `DropSpy`
    // 捕获进 sink 本体，即可直接观测这本"所有权契约"在 display 侧同样恰好履约一次。
    //
    // **两次 `set_flush_cb`**：只有把两个 sink 都纳入观测，才能证明"重复挂载时旧 sink
    // 也被恰好回收一次"——若回归成"单次 DELETE 登记"，旧 sink 会静默泄漏而后一个 sink
    // 仍绿。故此处断言：两个 sink 各 drop 恰好 1 次。
    let sink_a_drops = Rc::new(Cell::new(0u32));
    let sink_b_drops = Rc::new(Cell::new(0u32));
    {
        let spy_a = DropSpy(sink_a_drops.clone());
        disp.set_flush_cb(move |_a: Area, _px: &[u8]| {
            let _keep_alive = &spy_a;
        });
        let spy_b = DropSpy(sink_b_drops.clone());
        disp.set_flush_cb(move |_a: Area, _px: &[u8]| {
            let _keep_alive = &spy_b;
        });
    }
    assert_eq!(sink_a_drops.get(), 0, "仅挂载 flush sink 不应 drop（display 尚存活）");
    assert_eq!(sink_b_drops.get(), 0, "仅挂载 flush sink 不应 drop（display 尚存活）");

    // ── 场景 ⑥ indev 快照：indev 删除时回收路径恰好执行一次 ──────────────
    // 快照是 `Copy` 无 `Drop`，`DropSpy` 手法不适用；改由 `indev.rs` 的
    // `#[cfg(test)] SNAP_RECLAIMS` 直接计量回收路径（`Box::from_raw` 唯一所在）。
    assert_eq!(
        super::indev::SNAP_RECLAIMS.load(Ordering::SeqCst),
        0,
        "仅创建 indev 不应触发快照回收（indev 尚存活）"
    );

    // ── 场景 ⑦ **回调内删除宿主**：延迟回收（Critical 1 的自证）─────────
    // 在**非 DELETE**（这里是 CLICKED）回调内调用 `lv_obj_delete(host)`：LVGL 会在
    // 当前回调栈帧内**立即**派发 DELETE。若桥当场 `from_raw`，正在执行的闭包连同其
    // 捕获状态被释放 ⇒ 回调继续执行即为 UAF。延迟回收下：
    //   - DELETE 到达时探针计数必须仍为 0（闭包还活着）；
    //   - 外层回调退出后计数恰好为 1（不泄漏、不 double free）。
    // 用静态计数才能"在回调执行中"读数（见 `CB_SELF_DELETE_DROPS` 注释）。
    assert_eq!(CB_SELF_DELETE_DROPS.load(Ordering::SeqCst), 0, "前置：静态探针应为 0");
    let host7 = unsafe { sys::lv_obj_create(screen) };
    let spy7 = StaticSpy;
    let cb7 = move |_e| {
        let _keep_alive = &spy7;
        // 在自己的（非 DELETE）回调里删除宿主 —— A2/A3 `Obj::delete()` 的常态路径。
        // SAFETY: `host7` 此刻仍存活（首次删除）。
        unsafe { sys::lv_obj_delete(host7) };
        // 删除之后闭包仍须存活才能执行到这里：延迟回收成立 ⇒ 探针尚未 drop。
        CB_SELF_DELETE_DROPS_AT_DELETE
            .store(CB_SELF_DELETE_DROPS.load(Ordering::SeqCst), Ordering::SeqCst);
    };
    // SAFETY: `host7` 是刚 `lv_obj_create` 的活对象。
    let _h7 = unsafe { event::on(host7, EventCode::CLICKED, cb7) };
    // SAFETY: `host7` 仍是活对象；显式派发 CLICKED。
    unsafe { sys::lv_obj_send_event(host7, sys::LV_EVENT_CLICKED, std::ptr::null_mut()) };
    assert_eq!(
        CB_SELF_DELETE_DROPS_AT_DELETE.load(Ordering::SeqCst),
        0,
        "回调内删除宿主：DELETE 到达时闭包仍在执行，不得立即回收（否则 UAF）"
    );
    assert_eq!(
        CB_SELF_DELETE_DROPS.load(Ordering::SeqCst),
        1,
        "外层回调退出后应恰好回收一次（不泄漏、不 double free）"
    );

    // ── 场景 ⑧ **回调内 `detach`**：同样不得 UAF、恰好回收一次 ───────────
    // `detach` 是 A2/A3 会暴露的常规 API；在回调内摘除自己时，释放必须延迟到回调退出。
    assert_eq!(CB_DETACH_DROPS.load(Ordering::SeqCst), 0, "前置：静态探针应为 0");
    let host8 = unsafe { sys::lv_obj_create(screen) };
    let slot: Rc<RefCell<Option<event::CallbackHandle<*mut sys::lv_obj_t>>>> =
        Rc::new(RefCell::new(None));
    let slot_w = slot.clone();
    let spy8 = DetachSpy;
    let cb8 = move |_e| {
        let _keep_alive = &spy8;
        // 在自己的回调内摘除（并释放）自己。
        if let Some(h) = slot_w.borrow_mut().take() {
            // SAFETY: `host8` 此刻仍存活（尚未删除），满足 detach 的前置条件。
            unsafe { h.detach() };
        }
    };
    // SAFETY: `host8` 是刚 `lv_obj_create` 的活对象。
    let h8 = unsafe { event::on(host8, EventCode::CLICKED, cb8) };
    *slot.borrow_mut() = Some(h8);
    // SAFETY: `host8` 仍是活对象。
    unsafe { sys::lv_obj_send_event(host8, sys::LV_EVENT_CLICKED, std::ptr::null_mut()) };
    assert_eq!(
        CB_DETACH_DROPS.load(Ordering::SeqCst),
        1,
        "回调内 detach：闭包应恰好 drop 一次且不得 UAF"
    );
    // 事件已被摘除 ⇒ 再派发不应再进入闭包。
    unsafe { sys::lv_obj_send_event(host8, sys::LV_EVENT_CLICKED, std::ptr::null_mut()) };
    assert_eq!(
        CB_DETACH_DROPS.load(Ordering::SeqCst),
        1,
        "detach 后不应再被派发"
    );
    unsafe { sys::lv_obj_delete(host8) };
    assert_eq!(
        CB_DETACH_DROPS.load(Ordering::SeqCst),
        1,
        "对象删除后不得二次回收（不 double free）"
    );

    // 释放顺序：先 indev / display（各自回收自己的 user_data），再 lv_deinit。
    drop(indev);
    assert_eq!(
        super::indev::SNAP_RECLAIMS.load(Ordering::SeqCst),
        1,
        "indev 删除时应恰好回收快照一次（不泄漏、不 double free）"
    );
    drop(disp);
    assert_eq!(
        sink_a_drops.get(),
        1,
        "旧 flush sink（被后一次 set_flush_cb 顶替）也应恰好 drop 一次（不泄漏）"
    );
    assert_eq!(
        sink_b_drops.get(),
        1,
        "display 删除时应恰好 drop flush sink 一次（不泄漏、不 double free）"
    );
    super::deinit();

    // ── 场景 ⑨ init → deinit → init 后旧句柄不得二次删除（世代令牌）───────
    // 该时序下旧句柄的底层对象已被 `lv_deinit()` 释放，而重新 init 后
    // `is_initialized()` 又为 true —— 若 `Drop` 只看该标志就会二次删除（double free）。
    super::init().expect("lvgl::init (gen A)");
    let stale_disp = Display::create(W, H).expect("Display::create (stale)");
    let stale_indev = Indev::create_pointer(&stale_disp).expect("Indev (stale)");
    super::deinit(); // 底层 display/indev 在此被释放（DELETE 已回收各自 user_data）
    super::init().expect("lvgl::init (gen B)");
    assert!(
        super::is_initialized(),
        "重新 init 后 `is_initialized()` 为 true —— 正是旧句柄 Drop 容易误删的前提"
    );
    let reclaims_before = super::indev::SNAP_RECLAIMS.load(Ordering::SeqCst);
    drop(stale_indev); // 世代失配 ⇒ no-op（否则对已释放 indev 二次 delete）
    drop(stale_disp); // 世代失配 ⇒ no-op（否则 double free）
    assert_eq!(
        super::indev::SNAP_RECLAIMS.load(Ordering::SeqCst),
        reclaims_before,
        "旧世代句柄的 Drop 不得再回收（世代失配 ⇒ no-op；否则 double free）"
    );
    super::deinit();

    // ── A2「对象与样式」场景 ⑩–⑲（`tests_a2.rs`）────────────────────────
    // **同一个 `#[test]` 内顺序执行**：LVGL 非线程安全（`LV_USE_OS = LV_OS_NONE`），
    // `cargo test` 默认多线程跑测试函数 —— A2 的用例因此**不另起 `#[test]`**，
    // 而是由本函数在同一线程内继续驱动（沿用 A1 的串行化做法）。
    super::tests_a2::obj_style_font_chain();
}
