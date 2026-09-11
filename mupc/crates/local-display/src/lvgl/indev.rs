//! 输入设备（工作单元 A1；设计 §5.3 触摸输入）。
//!
//! # 分工
//!
//! - **`touch.rs`（后续工作单元）**：Rust 纯 safe 侧 —— evdev 设备发现、绝对坐标读取、
//!   校准、事件翻译，最终产出一个 [`TouchSnapshot`]（按下 + 屏幕坐标）。
//! - **本模块**：只做「快照 → `lv_indev_data_t`」的桥。命中 / z-order / 弹层拦截 /
//!   滚动判定 / `LV_EVENT_CLICKED` 派发**全部交给 LVGL**（设计 §5.3）。
//!
//! `lv_conf.h` 的 `LV_USE_EVDEV = 0`：**不引入 libevdev**，这条自研路径是唯一入口。
//!
//! # 投递时机
//!
//! 采用 `LV_INDEV_MODE_EVENT` + 事件到达后 [`Indev::read`] 主动投递，避免默认 30 ms
//! 轮询带来的固定延迟（设计 §5.3「读取时机」）。

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

use lvgl_sys as sys;

use super::display::Display;
use super::event::{self, EventCode};
use super::LvglError;

/// 测试专用：快照**回收路径**的执行次数（`indev.rs` 里 `Box::from_raw` 的唯一所在）。
///
/// 为什么不用 `DropSpy`：快照载荷是 `TouchSnapshot`（`Copy`、无 `Drop`），`DropSpy`
/// 那套"捕获物 drop 计数"在此没有可观测对象；但"释放恰好一次"仍需证明，故直接计量
/// 回收路径被执行的次数 —— 恒为 `1` 即证明该 `Box` 被释放恰好一次
/// （不泄漏、不 double free；若执行两次，计数器会读到 2）。生产构建下不存在。
#[cfg(test)]
pub(super) static SNAP_RECLAIMS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

/// 触摸快照：由 evdev 侧写入，由 read_cb 桥读出（单线程，无锁）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TouchSnapshot {
    /// 是否按下（`BTN_TOUCH` / `ABS_MT_TRACKING_ID >= 0`）。
    pub pressed: bool,
    /// 屏幕坐标 X（evdev 侧已完成校准 / 翻转）。
    pub x: i32,
    /// 屏幕坐标 Y。
    pub y: i32,
}

/// 一个 LVGL 指针类输入设备（含 read_cb 桥与快照的 `user_data` 生命周期）。
///
/// 含裸指针 ⇒ 自动 `!Send` / `!Sync`（设计 §5.2 不变量 4）。
pub struct Indev {
    raw: *mut sys::lv_indev_t,
    /// 创建时的 LVGL 世代（见 [`super::generation`]）：识别 `init → deinit → init`
    /// 之后已随上一世代 `lv_deinit()` 释放的底层 indev。
    generation: u64,
}

impl Indev {
    /// 建 pointer 类 indev：绑到 `disp`、挂 read_cb 桥、置 `LV_INDEV_MODE_EVENT`。
    ///
    /// `disp` 必须存活（已初始化 **且** 世代未变）；否则返回
    /// [`LvglError::NotInitialized`]，不会把悬垂的 display 指针交给 LVGL。
    pub fn create_pointer(disp: &Display) -> Result<Self, LvglError> {
        if !disp.is_live() {
            return Err(LvglError::NotInitialized);
        }
        // SAFETY: `disp.is_live()` ⇒ LVGL 已 `init()`。
        let raw = unsafe { sys::lv_indev_create() };
        if raw.is_null() {
            return Err(LvglError::OutOfMemory("lv_indev_create"));
        }

        let snap = Box::into_raw(Box::new(TouchSnapshot::default()));
        // SAFETY: `raw` 刚创建、存活；`snap` 的所有权交给下方 DELETE 回调。
        unsafe {
            sys::lv_indev_set_display(raw, disp.raw());
            sys::lv_indev_set_type(raw, sys::LV_INDEV_TYPE_POINTER);
            sys::lv_indev_set_user_data(raw, snap as *mut c_void);
            sys::lv_indev_set_read_cb(raw, Some(read_trampoline));
            sys::lv_indev_set_mode(raw, sys::LV_INDEV_MODE_EVENT);
            // SAFETY: `raw` 是刚创建、存活的 indev 指针。
            let _ = event::on(raw, EventCode::DELETE, move |_e| {
                #[cfg(test)]
                SNAP_RECLAIMS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                // SAFETY: `snap` 由上面的 `Box::into_raw` 产生，且只在此回收一次。
                drop(Box::from_raw(snap));
            });
        }
        Ok(Self {
            raw,
            generation: super::generation(),
        })
    }

    /// 本句柄的底层 indev 是否**仍存活**（LVGL 已初始化 **且** 世代未变）。
    fn is_live(&self) -> bool {
        super::is_initialized() && self.generation == super::generation()
    }

    /// 写入最新触摸快照（evdev 侧每拍调用）。**不做**命中判定 —— 那是 LVGL 的职责。
    ///
    /// 已 [`super::deinit`] 或处于上一世代时是 **no-op**（那时 indev 已随
    /// `lv_deinit()` 销毁，防 UB）。
    pub fn feed(&self, snapshot: TouchSnapshot) {
        if !self.is_live() {
            return;
        }
        // SAFETY: `self.raw` 存活（世代未变且 LVGL 仍初始化）；单线程调用。
        let p = unsafe { sys::lv_indev_get_user_data(self.raw) } as *mut TouchSnapshot;
        if !p.is_null() {
            // SAFETY: `p` 指向 `create_pointer` 放入的快照；单线程访问。
            unsafe { *p = snapshot };
        }
    }

    /// 主动投递一次：`lv_indev_read()` → LVGL 命中 / z-order / 滚动判定 / `LV_EVENT_*` 派发。
    ///
    /// 事件循环在 `poll()` 返回后、读到 evdev 事件时调用（设计 §5.2 骨架）。
    /// 已 [`super::deinit`] 或处于上一世代时是 **no-op**（防 UB）。
    pub fn read(&self) {
        if !self.is_live() {
            return;
        }
        // SAFETY: `self.raw` 存活且 LVGL 仍初始化（世代未变）；单线程调用。
        unsafe { sys::lv_indev_read(self.raw) };
    }
}

impl Drop for Indev {
    fn drop(&mut self) {
        // `super::deinit()` 已经把全部 indev 删掉（并回收快照）——不能再删一次。
        // 用世代令牌而非仅 `is_initialized()`：`init → deinit → init` 之后该标志又为
        // `true`，但本句柄的 indev 早已随上一世代 `lv_deinit()` 释放（否则 double free）。
        if self.is_live() {
            // SAFETY: 仍存活（已初始化且世代未变）⇒ 该 indev 尚未被删除。
            unsafe { sys::lv_indev_delete(self.raw) };
        }
    }
}

/// C 侧 read_cb 蹦床：把 Rust 快照填进 `lv_indev_data_t`。
///
/// 只写 `state` / `point`：`data` 已由 LVGL 零初始化，且 `point` 预置为上次坐标
/// （抬手未带坐标时不会跳回 (0,0)），`timestamp` 为空时 LVGL 自动补当前 tick。
unsafe extern "C" fn read_trampoline(indev: *mut sys::lv_indev_t, data: *mut sys::lv_indev_data_t) {
    let r = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `indev` 由 LVGL 传入且存活；`user_data` 只由 `create_pointer` 写入。
        let p = unsafe { sys::lv_indev_get_user_data(indev) } as *const TouchSnapshot;
        if p.is_null() || data.is_null() {
            return;
        }
        // SAFETY: `p` 指向存活的快照；单线程访问。
        let s = unsafe { *p };
        // SAFETY: `data` 由 LVGL 传入且指向有效、可写的 `lv_indev_data_t`。
        let d = unsafe { &mut *data };
        d.point.x = s.x;
        d.point.y = s.y;
        d.state = if s.pressed {
            sys::LV_INDEV_STATE_PRESSED
        } else {
            sys::LV_INDEV_STATE_RELEASED
        };
        d.continue_reading = false;
    }));
    if r.is_err() {
        eprintln!("[lvgl] indev read_cb 内 panic 已被拦截；本次快照作废（视为未按下）");
    }
}
