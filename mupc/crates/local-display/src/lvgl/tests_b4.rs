//! 薄层「能力补齐」自测（工作单元 **B4a**：`obj.rs` 的样式读回 / 事件冒泡标志 / 滚动位置，
//! `display.rs` 的屏旋转，`mod.rs` 的定容池余量）。
//!
//! # ⚠️ 为什么没有自己的 `#[test]`
//!
//! 与 A1/A2/A3 完全同因同法：LVGL 全局状态**非线程安全**（`lv_conf.h`：`LV_USE_OS = LV_OS_NONE`），
//! `cargo test` 默认多线程跑测试函数 ⇒ 本文件的用例**不另起 `#[test]`**，而是由
//! `tests.rs::lvgl_core_bridge_chain`（**唯一的** LVGL `#[test]`）在同一线程内顺序调起。
//!
//! # 覆盖（逐条对应 B4a 的五项能力；每项都有一处"改坏即红"的判据）
//!
//! | # | 能力 | 判据（改什么会红） |
//! |---|------|--------------------|
//! | G1 | [`Obj::bg_opa`] | 挂 `percent(20)` ⇒ 读回 **51**；改档位/不挂样式 ⇒ 红 |
//! | G1 | [`Obj::text_color`] | 读回 == 送进 `set_text_color` 的那个 `Color`（逐分量）；换色 ⇒ 红 |
//! | G1 | [`Obj::text_font`] | 读回 == [`super::font::Font::fallback`]（**默认构建**下 `font_of` 全体降级，见该方法的边界说明）；把 `theme::text` 的 `apply_font` 摘掉 ⇒ 红 |
//! | G2 | [`ObjFlag::EVENT_BUBBLE`] | `has_flag` 置位/清位可读回；`raw()` 值 == C 侧 `LV_OBJ_FLAG_EVENT_BUBBLE`（16384） |
//! | G3 | [`Obj::scroll_to_y`] / [`Obj::scroll_y`] | 内容超视口时 `scroll_to_y(0)` ⇒ 0、`scroll_to_y(50)` ⇒ > 0；不调 ⇒ 0 |
//! | rotation | [`Display::set_rotation`] / [`Display::rotation`] | 四个取值逐一轮转后读回相等；**不调** ⇒ 恒 `Deg0` |
//! | mem_monitor | [`super::mem_monitor`] | 池总容量 == `LV_MEM_SIZE`（1 MiB）；建对象后 `used_cnt` 增长 ⇒ 红 |
//!
//! 所有断言都在**内存 sink** 上完成（与 A1/A2/A3 同一口径）。

use std::cell::RefCell;
use std::rc::Rc;

use super::display::{Area, Display, Rotation, BYTES_PER_PIXEL};
use super::font::Font;
use super::obj::{Obj, ObjFlag};
use super::style::{Color, Opa, Style, StyleSelector};
use super::widgets::ScrollContainer;

/// 测试屏尺寸（与 A3 同口径：小屏便于逐像素核对）。
const W: u32 = 256;
const H: u32 = 192;

/// `LV_MEM_SIZE`（`lvgl-sys/lv_conf.h`：`(1024 * 1024U)`）—— 池总容量的期望值。
///
/// **单一真源**：本文件与 `lv_conf.h` 各写一份是**有意**的 —— 若 `lv_conf.h` 改了池大小
/// 而没人同步，本断言**当场红**（这正是"余量未量化"残余要变成"已被量化"的意义）。
const LV_MEM_SIZE: usize = 1024 * 1024;

/// 把脏区像素搬进内存 sink（与 A1/A2/A3 同构，**外加越界裁剪**）。
///
/// # 为什么本文件要比 A1/A2/A3 多一层裁剪
///
/// 旋转用例（`disp.set_rotation(Deg90)` 后强制渲染一趟）里，LVGL 交回的脏区在**旋转后的
/// 逻辑空间**（宽高互换）⇒ 会超出本文件 256×192 的物理 sink。这正是 **B4a 如实标注的
/// 能力边界**（"像素级物理旋转属驱动/sink 侧，本项目未实现"）在测试侧的可见形态。
///
/// 生产侧同款裁剪早已存在（`screen::Blitter` "已把区域裁剪到目标范围内"）⇒ 这里按同一口径
/// 裁剪，使本用例只证"**旋转下渲染链路不崩**"，不越界断言（越界写会 panic 在 flush 回调内，
/// 那本身是错误形态）。
fn blit(sink: &Rc<RefCell<Vec<u8>>>, area: Area, px: &[u8]) {
    let mut s = sink.borrow_mut();
    let row_bytes = area.width() as usize * BYTES_PER_PIXEL;
    for row in 0..area.height() as usize {
        let dy = area.y1 + row as i32;
        let dx = area.x1;
        if dy < 0 || dy >= H as i32 || dx < 0 || dx + area.width() as i32 > W as i32 {
            continue; // 越界行整行丢弃（旋转后逻辑空间的坐标）
        }
        let src = row * row_bytes;
        let dst = (dy as usize * W as usize + dx as usize) * BYTES_PER_PIXEL;
        s[dst..dst + row_bytes].copy_from_slice(&px[src..src + row_bytes]);
    }
}

/// 建一块显示 + 内存 sink（`Vec<u8>`）。
fn harness() -> (Rc<RefCell<Vec<u8>>>, Display) {
    let sink: Rc<RefCell<Vec<u8>>> = Rc::new(RefCell::new(vec![0u8; (W * H) as usize * BYTES_PER_PIXEL]));
    let mut disp = Display::create(W, H).expect("Display::create");
    let s = sink.clone();
    disp.set_flush_cb(move |area, px| blit(&s, area, px));
    (sink, disp)
}

/// B4a 的能力链（由 `tests.rs::lvgl_core_bridge_chain` 在同一线程内调起）。
pub(crate) fn thin_capabilities_chain() {
    super::init().expect("lvgl::init (B4a)");
    let (_sink, disp) = harness();
    let screen = Obj::screen().expect("Obj::screen");

    // ═══ G1：样式读回（`lv_obj_get_style_bg_opa` / `_text_color` / `_text_font` 的等价物）═══
    {
        let box_obj = Obj::create(&screen).expect("Obj::create (G1)");
        box_obj.set_size(100, 60);

        // 未挂任何样式 ⇒ 读回的是**继承/默认**值，而不是"我们送进去的那个"。
        // LVGL 默认主题给裸 `lv_obj` 挂 `card` 样式（`bg_opa = LV_OPA_COVER`），
        // 故这里只断言"读回**不是**我们接下来要送进去的 20 %"，用差异证明读回真的在**读**。
        let before = box_obj.bg_opa();

        let mut st = Style::new();
        st.set_bg_opa(Opa::percent(20));
        st.set_bg_color(Color::rgb(0x10, 0x20, 0x30));
        let st = Rc::new(st);
        box_obj.add_style(&st, StyleSelector::main());

        // `Opa::percent(20)` = 20 * 255 / 100 = **51**（不是 20、也不是 62 % 的 158）。
        assert_eq!(
            box_obj.bg_opa(),
            Opa::percent(20),
            "G1：挂上去的 20 % 必须**逐位**读回（51 / 255）"
        );
        assert_eq!(
            box_obj.bg_opa().raw(),
            51,
            "G1：C 侧 `lv_opa_t` 的原始值 = 51（`Opa::percent` 的算式在这条上被钉死）"
        );
        assert_ne!(
            before,
            box_obj.bg_opa(),
            "G1 对口哨：读回必须能**变**（恒返回同一个值 = 读回没接到对象上）"
        );
        assert_ne!(
            box_obj.bg_opa(),
            Opa::percent(62),
            "G1：**不得**读回模态遮罩那一档（62 %）—— 这正是 EDGE-03 残余要抓的错"
        );

        // 字号（读回的是**指针**）+ 字色（逐分量）。
        let label = super::widgets::Label::create_with_text(&box_obj, "A").expect("Label");
        let lbl_obj: &Obj = label.obj();
        let mut ts = Style::new();
        ts.set_text_font(&Font::fallback());
        ts.set_text_color(Color::rgb(0xFF, 0xB0, 0x20));
        let ts = Rc::new(ts);
        lbl_obj.add_style(&ts, StyleSelector::main());

        assert_eq!(
            lbl_obj.text_color(),
            Color::rgb(0xFF, 0xB0, 0x20),
            "G1：字色逐分量读回（`#FFB020` = EDGE-03 中央大字的契约色）"
        );
        assert_eq!(
            lbl_obj.text_font(),
            Some(Font::fallback()),
            "G1：字体指针读回 == 送进去的那个（默认构建下各档位都降级到 fallback，\
             见 `Obj::text_font` 的能力边界说明）"
        );
        assert!(
            lbl_obj.text_font().is_some(),
            "G1 对口哨：字体读回必须拿到**非空**指针（`None` = 属性根本没生效）"
        );
        drop(label);
        drop(box_obj);

        // ── B4a 整改「建议 3.2」：**失效回落值 == 合法实读值** ⇒ 二者不可区分 ────────
        //
        // `Obj::bg_opa` 在句柄失效时回落 `Opa::TRANSPARENT`(0)，而 0 **也是**对象把
        // `bg_opa` 真的设为 0 % 时的实读值。本段把"**不可区分**"这件事**钉死**：
        // 谁把回落值换成别的哨（`COVER` / `percent(50)` / 报错…）⇒ 下面某条即红。
        // 真需要区分时用 `Obj::is_alive()`（见该方法文档的"二选一取①"说明）。
        {
            let zero_obj = Obj::create(&screen).expect("Obj::create (bg_opa 0)");
            let mut zs = Style::new();
            zs.set_bg_opa(Opa::TRANSPARENT);
            let zs = Rc::new(zs);
            zero_obj.add_style(&zs, StyleSelector::main());
            let live_zero = zero_obj.bg_opa();
            assert_eq!(
                live_zero,
                Opa::TRANSPARENT,
                "前置：活对象**真的**把 `bg_opa` 设为 0 % ⇒ 实读就是 0"
            );

            // 共享句柄 → 删原件：`share_borrowed` 的 `alive` 是同一份 `Rc<Cell>`。
            let dead = zero_obj.share_borrowed();
            drop(zero_obj);
            assert!(!dead.is_alive(), "前置：原件已删 ⇒ 借用句柄失效");
            assert_eq!(
                dead.bg_opa(),
                live_zero,
                "建议 3.2：失效回落值必须 == 合法实读值 0（**二者不可区分**是本口已登记的\
                 边界，不是缺陷；谁改成别的哨值即在此红）"
            );
            assert_eq!(live_zero.raw(), 0, "0 % 的 C 侧原值必须 == 0");
        }
    }

    // ═══ G2：事件冒泡标志（**只镜像枚举值**；本批 `ui/**` 无消费者）═══════════════
    {
        let o = Obj::create(&screen).expect("Obj::create (G2)");
        assert_eq!(
            ObjFlag::EVENT_BUBBLE.raw(),
            16_384,
            "G2：`LV_OBJ_FLAG_EVENT_BUBBLE` = 1 << 14 = 16384（与 C 侧枚举逐位相同）"
        );
        assert!(
            !o.has_flag(ObjFlag::EVENT_BUBBLE),
            "G2 前置：LVGL **默认不上冒**（`event_is_bubbled` 要求目标自带该标志）"
        );
        o.add_flag(ObjFlag::EVENT_BUBBLE);
        assert!(
            o.has_flag(ObjFlag::EVENT_BUBBLE),
            "G2：置位后可读回（「父容器统一处理子事件」一类需求日后靠这条检查）"
        );
        o.remove_flag(ObjFlag::EVENT_BUBBLE);
        assert!(!o.has_flag(ObjFlag::EVENT_BUBBLE), "G2：清位后可读回");
        drop(o);
    }

    // ═══ G3：滚动位置（SH12「回归 P1 始终从顶部」的前提）══════════════════════════
    {
        let sc = ScrollContainer::create(&screen).expect("ScrollContainer");
        sc.set_size(100, 100);
        // 内容 500 px 高 > 视口 100 px ⇒ 可滚 400 px。
        sc.set_pos(0, 0);
        let content = Obj::create(sc.obj()).expect("Obj::create (content)");
        content.set_size(80, 500);
        disp.refr_now_for_test(); // 让布局趟落定（可滚范围在布局后才算得出）

        assert_eq!(
            sc.scroll_y(),
            0,
            "G3 前置：刚建好、未滚动 ⇒ 位置 0（负值只在 overscroll 回弹时出现）"
        );

        // **先滚走再滚回顶部**：否则 `scroll_to_y(0)` 对"本来就在 0"是空操作，
        // 断言会退化成"恒真"（本项目明令禁止的伪门禁）。
        sc.scroll_to_y(50);
        let after = sc.scroll_y();
        assert!(
            after > 0,
            "G3：滚到 50 px 后位置必须 > 0（实得 {after}）—— 恒 0 = 没滚或没接上"
        );

        sc.scroll_to_y(0);
        assert_eq!(
            sc.scroll_y(),
            0,
            "G3：`scroll_to_y(0)` 必须真的回到顶部（SH12 的消费形态）"
        );

        drop(content);
        drop(sc);
    }

    // ═══ rotation：屏旋转的施加与读回 ═══════════════════════════════════════════
    {
        assert_eq!(
            disp.rotation(),
            Rotation::Deg0,
            "rotation 前置：`Display::create` 不设旋转 ⇒ 恒 Deg0"
        );
        for r in [
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
            Rotation::Deg0,
        ] {
            disp.set_rotation(r);
            assert_eq!(
                disp.rotation(),
                r,
                "rotation：施加 {r:?} 后必须逐值读回（`--rotate` 的接线被删/写错即红）"
            );
        }
        assert!(
            Rotation::Deg90.swaps_axes() && Rotation::Deg270.swaps_axes(),
            "rotation：90°/270° 互换逻辑宽高（与 `config::Rotate::swaps_axes` 同口径）"
        );
        assert!(
            !Rotation::Deg0.swaps_axes() && !Rotation::Deg180.swaps_axes(),
            "rotation：0°/180° 不互换宽高"
        );
        // 旋转施加后仍能正常渲染一趟（防"旋转与 PARTIAL 双缓冲打架"）：
        // `.0` 是屏幕底色像素，只要不 panic 且回调被调到即可（`set_flush_cb` 已挂）。
        disp.set_rotation(Rotation::Deg90);
        disp.refr_now_for_test();
        disp.set_rotation(Rotation::Deg0);
    }

    // ═══ mem_monitor：定容池余量（设计 §10 / §14 R-24 的量化口）═══════════════════
    {
        let m = super::mem_monitor();
        // ⚠️ `total_size` **不是常量**：LVGL 逐块累加 `block_size`（`lv_mem_walker`），
        // 而块大小字段**不含块头** ⇒ "可分配总量" = 池面积 − **块数** × 块头开销，
        // 随分配/合并形态变化（实测：本链路 ≈ 1044384，`--smoke` 六页走查后 ≈ 986632）。
        // 判据因此写成"**≤ `LV_MEM_SIZE` 且同一量级**"，绝不写 `== 1 MiB`。
        assert!(
            m.total_size <= LV_MEM_SIZE,
            "mem_monitor：可分配总量不得**超过** `LV_MEM_SIZE`（实得 {}）",
            m.total_size
        );
        assert!(
            m.total_size > LV_MEM_SIZE * 9 / 10,
            "mem_monitor：可分配总量必须与 `LV_MEM_SIZE` **同量级**（实得 {} = {:.1}% —— \
             低于 90% 说明 `lv_conf.h` 的池大小被改动而本断言没同步）",
            m.total_size,
            m.total_size as f64 * 100.0 / LV_MEM_SIZE as f64
        );
        assert!(
            m.used_cnt > 0,
            "mem_monitor：此时屏/对象/样式均已建好 ⇒ 已分配块数必须 > 0（恒 0 = 读口没接上）"
        );
        assert!(
            m.max_used > 0 && m.max_used <= m.total_size,
            "mem_monitor：历史峰值必须落在 (0, total] 内（实得 max_used={}）",
            m.max_used
        );
        assert!(
            m.free_size < m.total_size,
            "mem_monitor：已建对象树 ⇒ 空闲**不等于**总量"
        );
        assert!(
            m.used_pct <= 100 && m.frag_pct <= 100,
            "mem_monitor：两个百分比字段必须落在 [0, 100]（字段错位会读出荒谬值）"
        );

        // **污染即变红**：再建一批对象，`used_cnt` / `max_used` 必须增长
        // （恒定的读数 = 读口读的是同一份陈旧快照）。
        let before = super::mem_monitor();
        let mut keep = Vec::new();
        for _ in 0..50 {
            keep.push(Obj::create(&screen).expect("Obj::create (mem)"));
        }
        let after = super::mem_monitor();
        assert!(
            after.used_cnt > before.used_cnt,
            "mem_monitor：新建 50 个对象后已分配块数必须增长（{} → {}）",
            before.used_cnt,
            after.used_cnt
        );
        assert!(
            after.free_size < before.free_size,
            "mem_monitor：新建 50 个对象后空闲必须减少（{} → {}）",
            before.free_size,
            after.free_size
        );
        assert!(
            after.max_used >= before.max_used,
            "mem_monitor：峰值单调不减"
        );
        assert!(
            after.has_headroom(),
            "mem_monitor：自检链路的峰值必须留有余量（`has_headroom` 是 `--smoke` 的判定口）"
        );
        drop(keep);
    }

    // ═══ B4b：`EventCode::SCROLL` 镜像（P5「滚动加载」的触发源）═════════════════════
    //
    // | # | 能力 | 判据（改什么会红） |
    // |---|------|--------------------|
    // | `EventCode::SCROLL` | 事件码镜像 | `raw()` == C 侧 `LV_EVENT_SCROLL`（15）；把常量绑到别的码 ⇒ 红 |
    {
        use super::event::EventCode;

        // ── `EventCode::SCROLL`（P5「滚动加载」的触发源；UI §6.5 / 偏差 AU6）──
        assert_eq!(
            EventCode::SCROLL.raw(),
            lvgl_sys::LV_EVENT_SCROLL,
            "B4b：`EventCode::SCROLL` 必须**逐位**镜像 C 侧 `lv_event_code_t`"
        );
        assert_eq!(
            EventCode::SCROLL.raw(),
            15,
            "B4b：`LV_EVENT_SCROLL` = 15（`lv_event.h` 的枚举序；与 `PRESS_LOST` 等既有码互不相等）"
        );
        assert_ne!(
            EventCode::SCROLL,
            EventCode::PRESSED,
            "B4b：别把 SCROLL 镜像成 PRESSED（那会让\"滚一下 = 按一下\"）"
        );
    }

    // ── 释放顺序：先对象树，再 display，最后 deinit（与 A1/A2/A3 同口径）──
    drop(screen);
    drop(disp);
    super::deinit();
}
