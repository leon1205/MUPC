//! WP5 P2-A：LVGL 日志转发（`lvgl/mod.rs` 的 [`super::log_bridge`] → Rust `tracing`）
//! 的判别力用例。
//!
//! # 本文件能证明什么 / 不能证明什么（**先读这一节，别把边界读丢**）
//!
//! **能证明**（每条都有"改什么会让它变红"）：
//!
//! | 用例 | 钉住的事实 | 改什么会红 |
//! |------|-----------|-----------|
//! | [`log_bridge_forwards_each_level_to_tracing_with_mapped_level`] | 五级映射（含 `USER→INFO`）+ `message` 原样透传 + `target` 正确 | 删/改 `forward_log` 的任一臂 |
//! | [`log_bridge_survives_null_non_utf8_and_out_of_range_level`] | 空指针 / 非 UTF-8 / 越界级别**都不 panic**，且"发生过一条日志"不丢 | 把 `to_str()` 的 `Err` 臂改成 `unwrap()` |
//! | [`level_names_cover_all_five_levels_and_fall_back_for_out_of_range`] | 级别名表（`tracing` 与回落 `diag` 共用） | 改名表任一项 |
//! | [`init_registers_the_lvgl_log_callback`] | `init()` 里**注册步骤**确实被走到 | 删 `init()` 中的注册**整步** |
//! | [`init_source_text_calls_the_registration_api`] | 注册**调用行**存在于 `init()` 的代码文本中 | 删（或注释掉）该调用行 |
//! | [`lv_conf_turns_off_printf_and_keeps_log_enabled`] | `LV_USE_LOG 1` 且 `LV_LOG_PRINTF 0`（C 侧 stdout 出口**编译期关死**） | 任一宏值改回 |
//! | [`vendored_lv_log_calls_the_registered_callback_for_all_entry_points`] | 固定版本 LVGL 的两条入口（`lv_log_add` / `lv_log`）都走 `custom_print_cb` | 换 LVGL 版本后该调用被移除 |
//! | [`allowlist_declares_the_log_registration_symbol`] | 清单仍放行注册符号（编译能过的前提） | 删清单条目（先编译失败） |
//!
//! **不能证明（如实登记，勿当成已验）**：
//!
//! 1. **未做 C → Rust 的端到端断言**（即"真在 LVGL 内部触发一条 WARN，看它到达 `tracing`"）。
//!    理由：能确定性触发 LVGL WARN 的入口（`lv_init` 重复初始化 / `lv_indev_create` 无 display /
//!    删除活动屏 …）都要求**在 LVGL 全局会话上制造特定状态**，而本 crate 的用例**并行共享**
//!    同一个 LVGL 全局会话（`ui/**`、`app.rs` 的用例还会 `lv_deinit`）⇒ 那样写会把偶发红
//!    引进测试面。**本文件用"桥本体单测 + 注册簿记 + 源文本/配置断言"替代**，并把边界写在这里。
//! 2. **`LOG_CB_REGISTERED` 只证明"我们调用了注册 API"**，不证明"LVGL 已受理"
//!    （LVGL 没有 callback 读回口）。⚠️ **实测过的边界**：只删 C 调用行、保留簿记时该用例
//!    仍绿 ⇒ 由 [`init_source_text_calls_the_registration_api`] 的源码文本断言补上。
//! 3. **回落分支（`tracing` 未启用 → `diag`）无单测**：`enabled!` 的判定读的是**进程级**
//!    最大级别（`LevelFilter::current()`），而任何一次 `with_default` 都会把它**单向抬到
//!    TRACE 且不再回落**（`tracing-core` 的 dispatcher 注册表只增不减）⇒ 同一进程内
//!    "未启用"这个前置只存在于**第一个**装订阅者的用例之前，断言必然是**顺序相关**的。
//!    故该分支只做**代码 + 文档**核对：它是 `forward_log` 里紧挨 `enabled!` 的一个 `else`
//!    分句，内容是一次 `diag`（`writeln!(stderr, …)`，忽略错误、零分配、不 panic）。
//!    ⚠️ 同一条事实也意味着：**本文件只要有一个用例跑过，进程内就别再断言回落行为**。
//! 4. **回调真的在 LVGL 内部被调用**这件事，由源码断言（`vendored_lv_log_…`）+ `LV_LOG_PRINTF 0`
//!    （`lv_conf` 用例）+ 注册簿记**三条合起来**支撑，不是单条端到端证据。

use super::{level_name, log_bridge, LOG_CB_REGISTERED, LOG_TARGET};

use std::cell::RefCell;
use std::ffi::CString;
use std::sync::atomic::Ordering;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

// ═══════════════════════════════════════════════════════════════════════════
// 捕获型订阅者（手写；**不**引入 `tracing-subscriber` —— 本 crate 的依赖表刻意最小）
// ═══════════════════════════════════════════════════════════════════════════

/// 只做一件事的订阅者：把 `(level, target, message)` 记进 [`SINK`]。
struct Capture;

thread_local! {
    /// 捕获缓冲。用 `thread_local` 是因为 `with_default` 装的订阅者**只作用于本线程**
    /// （测试各自在自己的线程上跑，互不串味）。
    static SINK: RefCell<Vec<(Level, String, String)>> = const { RefCell::new(Vec::new()) };
}

/// 取 `event` 的 `message` 字段（`tracing` 以 `record_debug` 记录它）。
struct MsgVisitor(String);

impl Visit for MsgVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            // `message` 是一个 `Display` 包装 ⇒ `{:?}` 给出**不带引号**的原文。
            self.0 = format!("{value:?}");
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        }
    }
}

impl Subscriber for Capture {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _span: &Id, _values: &Record<'_>) {}
    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut v = MsgVisitor(String::new());
        event.record(&mut v);
        let md = event.metadata();
        SINK.with(|s| {
            s.borrow_mut()
                .push((*md.level(), md.target().to_string(), v.0))
        });
    }
    fn enter(&self, _span: &Id) {}
    fn exit(&self, _span: &Id) {}
}

/// 在作用域订阅者（[`Capture`]）下跑 `f`，返回捕获到的 `(level, target, message)`。
fn capture<F: FnOnce()>(f: F) -> Vec<(Level, String, String)> {
    SINK.with(|s| s.borrow_mut().clear());
    tracing::subscriber::with_default(Capture, f);
    SINK.with(|s| s.borrow().clone())
}

/// 以 C 字符串形态调用 [`log_bridge`]（模拟 LVGL 从 C 侧传 `const char*`）。
fn bridge_with_str(level: i8, s: &str) {
    let c = CString::new(s).expect("测试串不含 NUL");
    // SAFETY: `c` 在本调用期间存活且以 NUL 结尾（`CString` 的契约），正是 `log_bridge`
    // 对 `buf` 的全部要求（见该函数的 SAFETY 契约第 2 条）。
    unsafe { log_bridge(level, c.as_ptr()) };
}

// ═══════════════════════════════════════════════════════════════════════════
// 级别名表（`tracing` 与回落 `diag` 共用同一张表）
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn level_names_cover_all_five_levels_and_fall_back_for_out_of_range() {
    // 取值域与 `lv_log.h` 的 `LV_LOG_LEVEL_*` 一一对应。
    assert_eq!(level_name(0), "TRACE");
    assert_eq!(level_name(1), "INFO");
    assert_eq!(level_name(2), "WARN");
    assert_eq!(level_name(3), "ERROR");
    assert_eq!(level_name(4), "USER");
    // 越界（含 `LV_LOG_LEVEL_NONE` = 5）：必须有**不 panic** 的返回值。
    for bad in [5i8, 6, -1, i8::MIN, i8::MAX] {
        assert_eq!(level_name(bad), "?", "越界级别 {bad} 应回落 \"?\"");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 桥本体：级别映射 / 透传
// ═══════════════════════════════════════════════════════════════════════════

/// 五级各转出一条，且**级别映射**与 target 都对。
///
/// **判别力**：删掉 `forward_log` 里的任一臂（该级别不再有 `tracing` 调用）⇒ 捕获条数
/// 不足 5 或某条级别不符 ⇒ 红；把 `USER` 与 `WARN` 的 arm 对调 ⇒ 红。
#[test]
fn log_bridge_forwards_each_level_to_tracing_with_mapped_level() {
    const CASES: [(i8, Level); 5] = [
        (0, Level::TRACE),
        (1, Level::INFO),
        (2, Level::WARN),
        (3, Level::ERROR),
        // `LV_LOG_LEVEL_USER` 无对应 tracing 级 ⇒ 有意映射到 INFO（信息性）。
        (4, Level::INFO),
    ];
    let got = capture(|| {
        for (lv, _) in CASES {
            bridge_with_str(lv, "hello-lvgl");
        }
    });
    assert_eq!(got.len(), CASES.len(), "五级应各转出一条：{got:?}");
    for (i, (lv, expect)) in CASES.iter().enumerate() {
        assert_eq!(got[i].0, *expect, "级别 {lv} 应映射为 {expect:?}");
        assert_eq!(got[i].1, LOG_TARGET, "target 应为 {LOG_TARGET}");
        assert_eq!(got[i].2, "hello-lvgl", "message 应**原样**透传");
    }
}

/// 恶劣输入**不 panic**、且"发生过一条日志"这个事实不丢。
///
/// **判别力**：把 `log_bridge` 的 `Err(_)` 臂改成 `to_str().unwrap()` ⇒ 本用例 panic（红）。
/// 这正是 C 回调最怕的失败形态 —— 跨 FFI 展开 = UB（本模块纪律 3）。
#[test]
fn log_bridge_survives_null_non_utf8_and_out_of_range_level() {
    // ① NULL 缓冲：LVGL 契约上不会发生，但必须兜住（不 panic、仍报一条）。
    let got = capture(|| {
        // SAFETY: 本函数的全部目的就是验证"NULL 进来也不崩"；`log_bridge` 自己先判 `is_null()`。
        unsafe { log_bridge(2, std::ptr::null()) };
    });
    assert_eq!(got.len(), 1, "NULL 缓冲也必须报出一条：{got:?}");
    assert!(
        got[0].2.contains("空日志缓冲"),
        "文案应指明空缓冲：{:?}",
        got[0]
    );

    // ② 非 UTF-8 缓冲（截断的 UTF-8 序列 + NUL）：不 panic，报"内容已丢弃"。
    let bad: [u8; 3] = [0xE4, 0xB8, 0x00];
    let got = capture(|| {
        // SAFETY: `bad` 以 NUL 结尾、在本调用期间存活；`log_bridge` 只按 C 字符串读。
        unsafe { log_bridge(2, bad.as_ptr() as *const std::ffi::c_char) };
    });
    assert_eq!(got.len(), 1, "非 UTF-8 也必须报出一条：{got:?}");
    assert!(
        got[0].2.contains("非 UTF-8"),
        "文案应指明不可解码：{:?}",
        got[0]
    );

    // ③ 越界级别：按 WARN 报，且带上原值（便于定位"LVGL 传了什么"）。
    let got = capture(|| bridge_with_str(9, "boom"));
    assert_eq!(got.len(), 1, "越界级别也必须报出一条：{got:?}");
    assert_eq!(got[0].0, Level::WARN);
    assert!(got[0].2.contains("level=9"), "应带上原级别：{:?}", got[0]);
}

// ═══════════════════════════════════════════════════════════════════════════
// 注册簿记
// ═══════════════════════════════════════════════════════════════════════════

/// `init()` 确实走到了"把 [`log_bridge`] 注册给 LVGL"这一步。
///
/// **判别力（实测，见本轮报告）**：把 `init()` 里的注册**整步**删除 ⇒ 簿记永不置位 ⇒ 红。
/// ⚠️ **边界（实测过，别读丢）**：本用例**抓不到**"只删 C 调用行、保留簿记"的拆解
/// （实测：删掉 `sys::lv_log_register_print_cb(...)` 一行后本条仍绿）—— 簿记式断言的固有上限。
/// 该缺口由 [`init_source_text_calls_the_registration_api`]（剥离注释后的源码文本断言）补上。
/// 两者合起来覆盖"删调用"与"删整步"两种改坏方式；均**不**证明 LVGL 存下了指针（无读回口），
/// 那一环由 [`vendored_lv_log_calls_the_registered_callback_for_all_entry_points`] 承担。
#[test]
fn init_registers_the_lvgl_log_callback() {
    super::init().expect("lvgl::init");
    assert!(
        LOG_CB_REGISTERED.load(Ordering::SeqCst),
        "init() 必须把 log_bridge 注册给 LVGL（否则 LV_LOG_PRINTF=0 下 LVGL 日志全丢）"
    );
}

/// `init()` 的源码文本（**剥离注释行**后）确实包含注册调用。
///
/// 补 [`init_registers_the_lvgl_log_callback`] 的缺口：簿记与调用虽相邻，却是两条语句 ⇒
/// 只删调用行时簿记仍在。本用例直接盯**代码文本**：删掉该行即红；把它注释掉也红
/// （注释行被滤掉 ⇒ 不构成"有引用"，同 `lvgl-sys/tests/allowlist_consistency.rs` 的口径）。
#[test]
fn init_source_text_calls_the_registration_api() {
    const SRC: &str = include_str!("mod.rs");
    let code: Vec<&str> = SRC
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect();
    let code = code.join("\n");
    assert!(
        code.contains("sys::lv_log_register_print_cb(Some(log_bridge))"),
        "init() 必须调用注册 API（本断言剥离注释行 ⇒ 注释里写一遍不算）"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 配置 / 源文本断言（"printf 不再是出口"的机械证据）
// ═══════════════════════════════════════════════════════════════════════════

/// `lv_conf.h`：日志开启（否则回调永不被调用）+ **C 侧 stdout 出口关死**。
///
/// **判别力**：把 `LV_USE_LOG` 改回 0（日志链路整条失效）或把 `LV_LOG_PRINTF` 改回 1
/// （C 侧又出现 `printf` 直写 stdout，与本模块"回调内禁用 printf"的纪律冲突）⇒ 红。
#[test]
fn lv_conf_turns_off_printf_and_keeps_log_enabled() {
    const CONF: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/lvgl-sys/lv_conf.h"));
    assert!(
        CONF.contains("#define LV_USE_LOG 1"),
        "LV_USE_LOG 必须为 1：置 0 后 lv_log_add 整条被编译掉 ⇒ log_bridge 永不被调用"
    );
    assert!(
        CONF.contains("#define LV_LOG_PRINTF 0"),
        "LV_LOG_PRINTF 必须为 0：置 1 则 C 侧仍有 printf 直写 stdout（且发生在渲染调用栈内）"
    );
    assert!(
        CONF.contains("#define LV_LOG_LEVEL LV_LOG_LEVEL_WARN"),
        "级别初值应为 WARN（设计 §1.1.1.2）"
    );
}

/// 固定版本（v9.5）的 LVGL：**两条入口**都把日志交给已注册回调，且回调优先于 `printf`。
///
/// 这条是"我们注册的回调**会**被调用"在 C 侧的根据（台账里 LVGL 是 vendor pin 的，故可按
/// 源文本钉住）。**判别力**：升级 LVGL 后若 `custom_print_cb` 的调用点被删/改名 ⇒ 红
/// ⇒ 强制复核本转发链。
#[test]
fn vendored_lv_log_calls_the_registered_callback_for_all_entry_points() {
    const SRC: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/lvgl/src/misc/lv_log.c"
    ));
    assert!(
        SRC.contains("if(custom_print_cb) {"),
        "lv_log_add/lv_log 都必须先看 custom_print_cb"
    );
    assert!(
        SRC.contains("custom_print_cb(level, buf);"),
        "lv_log_add 必须把 (level, buf) 交给已注册回调 —— 这正是 log_bridge 的形参来源"
    );
    assert!(
        SRC.contains("custom_print_cb(LV_LOG_LEVEL_USER, buf);"),
        "lv_log()（LV_LOG_USER）也必须走回调，否则 USER 级日志整条丢掉"
    );
    // 优先关系：`printf` 是"回调为空"的 else。判据取**首次回调检查之后**的文本切片，
    // 断言 `#if LV_LOG_PRINTF` 出现在 `else {` **之前**（即它正是那个 else 的编译开关）。
    // ⚠️ 不能用"文件里 `#if LV_LOG_PRINTF` 的首次出现"—— 它在顶部 INCLUDES 段
    // （`#include <stdio.h>` 的守卫）就已出现，那样断言会退化成与优先关系无关的东西。
    let cb = SRC.find("if(custom_print_cb) {").expect("上文已断言其存在");
    let tail = &SRC[cb..];
    let printf = tail
        .find("#if LV_LOG_PRINTF")
        .expect("回调检查之后应有 printf 回落分支");
    let else_ = tail
        .find("else {")
        .expect("printf 回落应写作 else（回调非空时不可达）");
    assert!(
        printf < else_,
        "`#if LV_LOG_PRINTF` 必须紧接在 `else {{` 之前 —— 否则 printf 与回调不是 if/else 关系，\
         「回调优先 ⇒ printf 不可达」这条结论不成立"
    );
}

/// allowlist 仍放行注册符号（编译能通过的前提；也是"清单=用点"双向断言的输入）。
///
/// **判别力**：删掉任一条目 ⇒ 本用例红（且 `lvgl-sys` 侧会先编译失败）。
#[test]
fn allowlist_declares_the_log_registration_symbol() {
    const ALLOW: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/lvgl-sys/allowlist.txt"
    ));
    let has = |want: &str| ALLOW.lines().any(|l| l.trim() == want);
    assert!(has("fn:lv_log_register_print_cb"), "缺 fn 条目");
    assert!(has("type:lv_log_level_t"), "缺 lv_log_level_t");
    assert!(has("type:lv_log_print_g_cb_t"), "缺 lv_log_print_g_cb_t");
}
