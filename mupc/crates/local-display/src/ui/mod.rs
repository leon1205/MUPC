//! # `ui` —— 本地显示终端的界面层（12-MUPC v2.0）
//!
//! 设计 §5.1 的 `src/ui/` 子树：
//!
//! | 模块 | 职责 | 工作单元 |
//! |------|------|----------|
//! | [`theme`] | 界面外观的**单一真源**：色板 / 尺寸 / 字号 / 圆角 / 描边 / 状态色 → `lv_style` | **B1** |
//! | [`components`] | 薄层之上的**组合控件**（8 件 + `WarnBanner` + TT-10 防重） | **B1** |
//! | `pages` | 6 页布局（`p1_status` … `p6_system`） | **B2** |
//! | 页面路由 / 底部导航 | 6 页容器 + `NavTab`（`lv_tabview` 隐藏标签栏，或自建容器显隐） | **B2** |
//!
//! ## 本轮的边界（B1）
//!
//! 本模块目前**只有** `theme` 与 `components` 两个子模块 —— 即工作单元 B1 的交付范围。
//! 页面路由、导航栏与 6 个页面（`pages/**`）属 **B2**，由 B2 在本文件追加
//! `pub mod pages;` 与 `App` 装配（设计 §5.2 的 `ui::App::new(disp, indev, channel, console, cli)`）。
//! 这里**不预置**任何空壳，避免给 B2 留下"半成品页面"。
//!
//! ## 两条贯穿整个 `ui/**` 的硬约束（设计 §11.4 静态约束，`ui/tests.rs` 有扫描用例）
//!
//! 1. **不得出现裸色值 / 裸尺寸**：一切外观数值经 [`theme`] 取用；
//! 2. **不得引用文本输入控件符号**，也不得直连底层绑定（一律经 `crate::lvgl` 薄安全层）。

pub mod components;
pub mod theme;

// `pub(crate)`：LVGL 侧唯一的 `#[test]`（`src/lvgl/tests.rs::lvgl_core_bridge_chain`）
// 需要在同一线程内顺序调起 [`tests::ui_chain`]（LVGL 非线程安全，不得另起 `#[test]`）。
#[cfg(test)]
pub(crate) mod tests;
