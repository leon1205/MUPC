//! 位图字体的**注册与取用**（工作单元 A2；设计 §1.1.2 / §5.6）。
//!
//! # 字体从哪来
//!
//! `lv_font_conv` 生成的 C 数组（`crates/local-display/fonts/lv_font_noto_sc_<px>.c`，
//! 由 `fonts/gen_fonts.sh` 复现）在 `lvgl-sys/build.rs` 里被编成静态库，符号形如
//! `lv_font_noto_sc_24`；本模块把它们按**档位**取出来，供 `ui/theme.rs` 挂进样式
//! （`Style::set_text_font`）——页面**不得**硬编码字体引用（设计 §5.6 NF-04 行）。
//!
//! 10 档与 UI 设计 §3.3 的字号阶梯**逐档对应**：148 / 112 / 96 / 64 / 56 / 48 / 32 / 28 / 26 / 24。
//!
//! # 产物不在仓库里（设计 §1.1.2）
//!
//! `fonts/*.c` 与字库源 `*.otf` 均**不入库**（构建前须先跑 `gen_fonts.sh`）。因此：
//!
//! - **未启用 `noto-font` feature（默认）**：本模块仍**整体编译**，只是
//!   [`Font::of`] 一律返回 `None`（"字体不可用"的降级路径），而 [`Font::fallback`]
//!   始终可用（LVGL 内置 `lv_font_montserrat_14`，ASCII/符号可读，中文走
//!   `LV_USE_FONT_PLACEHOLDER` 的占位框）；
//! - **启用 `local-display/noto-font`**（转发到 `lvgl-sys/noto-font`）：[`Font::of`]
//!   返回对应档位；若生成物缺失，`lvgl-sys/build.rs` 会给出**可读的编译期报错**
//!   （列出缺哪几档 + 复现命令），不退化为晦涩的 file not found / 链接错误。
//!
//! **为什么"整个模块都在、只让 [`Font::of`] 返回 `None`"，而不是把模块 feature-gate**：
//! 这样 `ui/theme.rs`（B 单元）与 `widgets.rs`（A3）**两种构建配置下都是同一份代码**，
//! 不必在页面里写 `#[cfg(feature = …)]`——降级路径收敛在**一个** `Option` 上（KISS，
//! 也让"未生成字库"的开发机与真机跑同一套测试）。
//!
//! # 线程
//!
//! [`Font`] 只含裸指针（⇒ 自动 `!Send` / `!Sync`），与薄层其余句柄同纪律：**只在事件循环
//! 线程内使用**（设计 §5.2 不变量 4）。LVGL 拥有字体数据本体，[`Font`] **不**负责释放。

use lvgl_sys as sys;

/// 字号档位（设计 §1.1.2 / UI 设计 §3.3 的 10 档，**逐档一一对应**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FontSize {
    /// 正文 / 弱注（全屏下限）。
    S24,
    /// 字段名 / 控件文字 / 按钮字。
    S26,
    /// 区块标题 / 相标。
    S28,
    /// 页标题 / 装置卡值 / 审计值。
    S32,
    /// 三相 I 数值。
    S48,
    /// 单位（`%`）。
    S56,
    /// 三相 P 数值。
    S64,
    /// P4 联锁总态词。
    S96,
    /// P1 PCS 状态词。
    S112,
    /// P1 SOC 主数值。
    S148,
}

impl FontSize {
    /// 全部档位（**升序**；顺序即 UI §3.3 阶梯的小→大）。
    pub const ALL: [FontSize; 10] = [
        FontSize::S24,
        FontSize::S26,
        FontSize::S28,
        FontSize::S32,
        FontSize::S48,
        FontSize::S56,
        FontSize::S64,
        FontSize::S96,
        FontSize::S112,
        FontSize::S148,
    ];

    /// 档位的像素高度（= `lv_font_conv --size`，也 = 对外契约里的档位名）。
    pub const fn px(self) -> u32 {
        match self {
            FontSize::S24 => 24,
            FontSize::S26 => 26,
            FontSize::S28 => 28,
            FontSize::S32 => 32,
            FontSize::S48 => 48,
            FontSize::S56 => 56,
            FontSize::S64 => 64,
            FontSize::S96 => 96,
            FontSize::S112 => 112,
            FontSize::S148 => 148,
        }
    }
}

/// 字体句柄（LVGL 静态字体数据的只读引用；**不**负责释放，也无 `Drop`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Font {
    raw: *const sys::lv_font_t,
}

impl Font {
    /// 按档位取 CJK 位图字体（`lv_font_conv` 产物）。
    ///
    /// - 启用 `noto-font` 时：返回对应档位（产物缺失则**编译期**报错，见模块文档）；
    /// - 未启用时：`None` —— 调用方走降级路径（[`Font::fallback`] 或直接不设字体）。
    #[cfg(feature = "noto-font")]
    pub fn of(size: FontSize) -> Option<Font> {
        // `addr_of!` 只取地址、不产生 `&'static` 引用（LVGL 侧数据是 `extern const`）。
        let raw: *const sys::lv_font_t = match size {
            FontSize::S24 => std::ptr::addr_of!(sys::lv_font_noto_sc_24),
            FontSize::S26 => std::ptr::addr_of!(sys::lv_font_noto_sc_26),
            FontSize::S28 => std::ptr::addr_of!(sys::lv_font_noto_sc_28),
            FontSize::S32 => std::ptr::addr_of!(sys::lv_font_noto_sc_32),
            FontSize::S48 => std::ptr::addr_of!(sys::lv_font_noto_sc_48),
            FontSize::S56 => std::ptr::addr_of!(sys::lv_font_noto_sc_56),
            FontSize::S64 => std::ptr::addr_of!(sys::lv_font_noto_sc_64),
            FontSize::S96 => std::ptr::addr_of!(sys::lv_font_noto_sc_96),
            FontSize::S112 => std::ptr::addr_of!(sys::lv_font_noto_sc_112),
            FontSize::S148 => std::ptr::addr_of!(sys::lv_font_noto_sc_148),
        };
        Some(Font { raw })
    }

    /// 按档位取 CJK 位图字体 —— 未启用 `noto-font` 的降级路径：一律 `None`。
    #[cfg(not(feature = "noto-font"))]
    pub fn of(_size: FontSize) -> Option<Font> {
        None
    }

    /// LVGL 内置字体（`lv_font_montserrat_14`，ASCII + 常用符号），**始终可用**。
    ///
    /// 用途：CJK 位图字体不可用时的兜底（中文由 `LV_USE_FONT_PLACEHOLDER` 画占位框），
    /// 以及不依赖中文字形的场景（S-2 离屏 spike 即用它做对照）。
    pub fn fallback() -> Font {
        Font {
            raw: std::ptr::addr_of!(sys::lv_font_montserrat_14),
        }
    }

    /// 原始字体指针（薄层内部：`style.rs` 设字号用）。
    pub(crate) fn raw(self) -> *const sys::lv_font_t {
        self.raw
    }
}
