//! # `ui` —— 本地显示终端的界面层（12-MUPC v2.0）
//!
//! 设计 §5.1 的 `src/ui/` 子树：
//!
//! | 模块 | 职责 | 工作单元 |
//! |------|------|----------|
//! | [`theme`] | 界面外观的**单一真源**：色板 / 尺寸 / 字号 / 圆角 / 描边 / 状态色 → `lv_style` | **B1** |
//! | [`components`] | 薄层之上的**展示 / 确认型**组合控件（8 件 + `WarnBanner` + TT-10 防重） | **B1** |
//! | [`controls`] | 薄层之上的**输入型**组合控件（`SegmentedControl` / `Ipv4Stepper` / `DateTimeStepper`） | **B2b-1** |
//! | `pages` | 6 页布局（`p1_status` … `p6_system`） | **B2** |
//! | [`shell`] | 应用外壳：页眉 + 底部导航（6 `NavTab`）+ 页面路由 + 超时回归 + 未保存提示条 | **B2c-3** |
//!
//! ## 本轮的边界（B1 + B2a + B2b-1）
//!
//! `theme` / `components` 是 **B1** 的交付；`pages` 是 **B2** 的交付，其中 **B2a** 落了
//! `p1_status`（P1 主状态页）与 `p6_system`（P6 系统 / 关于页）两页；**B2b-1** 补了
//! [`controls`] 的三个输入型控件（P2 参数 / P3·P5 时间范围要用），P2–P5 页面属 B2b-2/B2c。
//!
//! **页面路由与底部导航的装配不做**（B2c）：本文件只导出 `pages`，不在此建页面容器、
//! 不装配 `lv_tabview` / 导航栏（`UiState` 扩展与控制通道接线属 B3）。
//!
//! > **B2c-3 补充（上文那句已被取代，保留以备追溯）**：页面路由 / 底部导航 / 页眉 /
//! > 超时回归 / 未保存提示条的装配**已在** [`shell`]（`ui/shell.rs`，工作单元 **B2c-3**）。
//! > 实现取 §5.3 的**后者**（6 个页面容器 + `HIDDEN` 显隐），**不用** `lv_tabview`
//! > —— 因为外壳要"一次建 6 页、常驻不销毁"，而 `lv_tabview` 的页生命周期归它自己管。
//! > **`UiState` 扩展 / 控制通道接线 / 触摸设备初始化仍属 B3**；[`shell`] 只留**注入位**
//! > （见 `Shell` 的 `p1()` … `p6()` / `set_channel` / `set_touch_available` /
//! > `set_idle_timeout` / `set_modal_open` / `overlay_layer`）。
//!
//! `controls` 与 `components` **分列**的口径（职责边界）：`components` 只反映既有数据、
//! 不产生新数据（输出 = 视觉状态 + 无载通知）；`controls` 是**草稿值的生产者**（输出 =
//! 带载荷的 `set_on_change`）。两者的契约轴不同，故不混装 —— 详见 [`controls`] 模块文档。
//!
//! ## 两条贯穿整个 `ui/**` 的硬约束（设计 §11.4 静态约束，`ui/tests.rs` 有扫描用例）
//!
//! 1. **不得出现裸色值 / 裸尺寸**：一切外观数值经 [`theme`] 取用；
//! 2. **不得引用文本输入控件符号**，也不得直连底层绑定（一律经 `crate::lvgl` 薄安全层）。

pub mod components;
pub mod controls;
pub mod pages;
// 应用外壳（开发单元 B2c-3）：页眉 / 底部导航 / 页面路由 / 超时回归 / 未保存修改提示条。
pub mod shell;
pub mod theme;

// `pub(crate)`：LVGL 侧唯一的 `#[test]`（`src/lvgl/tests.rs::lvgl_core_bridge_chain`）
// 需要在同一线程内顺序调起 [`tests::ui_chain`]（LVGL 非线程安全，不得另起 `#[test]`）。
#[cfg(test)]
pub(crate) mod tests;
