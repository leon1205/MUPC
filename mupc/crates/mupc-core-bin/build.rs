//! 编译时间戳注入（12-本地显示终端 设计 §4.8 / PRD T-5 #5）。
//!
//! `info.build_time` 的真源：构建期把 **秒级 Unix 时间戳**写进环境变量，代码用
//! `option_env!("BUILD_TIMESTAMP_EPOCH")` 读取并格式化为 RFC3339（见 `display_host::build_time_rfc3339`）。
//! 取不到 ⇒ `None` ⇒ 屏显「未提供」（EDGE-16，不臆造）。
//!
//! - **优先 `SOURCE_DATE_EPOCH`**（秒级 Unix 时间）：可复现构建（同一 epoch ⇒ 同一上屏字符串）。
//! - 否则取当前系统时间。
//! - **口径（如实写清）**：该值 = **本 build script 上次运行的时刻**，**不是**「源码最后修改时间」。
//!   由于下面声明的 `rerun-if-changed=build.rs`（仅为保住增量缓存），**改动其它任何源文件都不会
//!   令本脚本重跑** ⇒ `info.build_time` 会**静默变旧**（屏上仍显示上次构建时刻，且无任何提示）。
//!   契约只要求它是「编译时间」，本实现满足该口径；如将来要「源码最后修改时刻」须换真源。
//! - **时钟早于 epoch ⇒ 不注入**（2026-09-16 整改）：原实现链条末端用 `.unwrap_or(0)`，把
//!   「取不到时间」伪装成「1970-01-01 00:00:00Z」这一**合法**值 ⇒ 与"显 `--`、**严禁补 0**"的
//!   项目口径相反（同 `p5_audit::side_text` 的 C1 级前车之鉴）。现改为 `.ok()` → `None` ⇒
//!   不 `println!` ⇒ 运行期 `option_env!` 得 `None` ⇒ 屏显「未提供」（EDGE-16，不臆造）。
//! - 本脚本只用 `std`，**不引入 build-dependency**（build script 只能使用 `[build-dependencies]`；
//!   加 `chrono` 会新增构建期依赖）。日期换算交由运行期已依赖的 `chrono`，避免在 build script
//!   里手写日历算法（build script 的 `#[cfg(test)]` **不会**被 `cargo test` 执行 ⇒ 手写算法无网）。
fn main() {
    // 只在 build.rs 自身变化时重跑：否则每次构建都重发时间戳会打掉增量缓存。
    println!("cargo:rerun-if-changed=build.rs");

    // `None` = 时间不可得（时钟早于 epoch / 无可用时钟）⇒ **不注入**（见头部说明）
    let secs: Option<i64> = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });

    match secs {
        Some(s) => println!("cargo:rustc-env=BUILD_TIMESTAMP_EPOCH={s}"),
        None => {
            // 不注入：`option_env!("BUILD_TIMESTAMP_EPOCH")` ⇒ None ⇒ 屏显「未提供」
        }
    }
}
