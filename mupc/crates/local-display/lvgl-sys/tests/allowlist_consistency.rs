//! allowlist 双向一致性校验（设计 §1.1.1.2 / §12.1 的 CI 断言，本 spike 先落地为可运行测试）。
//!
//! 运行：`cd mupc && cargo test -p lvgl-sys --test allowlist_consistency -j 2`
//!
//! 断言：
//!  ① **fn / var 导出 ⊆ allowlist.txt**（防"偷偷全量生成"，这是禁 `lv_*` 通配的核心保障）；
//!  ② allowlist 中每个 **fn / var** 在 `src/**` 或 `examples/**` 里确有文本引用（无死符号）。
//!
//! 说明（诚实标注，见 spike 报告"未决问题"）：
//!   - `type:` / `const:` 条目是 `allowlist_recursively(true)` 带出的**传递闭包**，
//!     机械的 ⊆ 断言不成立。本轮对 type 只做「存在性校验 + 闭包清单报告」；
//!     正式编码期薄层 `src/lvgl/**` 落地后，应把 type 也升为硬断言（那时每个 type
//!     都能在薄层签名里被引用）。
//!   - allowlist 里**不得**出现 `*`（通配），本测试同样断言这一点。

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Default, Debug)]
struct Exports {
    fns: BTreeSet<String>,
    statics: BTreeSet<String>,
    types: BTreeSet<String>,
    consts: BTreeSet<String>,
}

fn parse_bindings(src: &str) -> Exports {
    let mut e = Exports::default();
    let mut in_extern = false;
    for line in src.lines() {
        let l = line.trim_end();
        if l.starts_with("unsafe extern \"C\" {") {
            in_extern = true;
            continue;
        }
        if in_extern {
            if l == "}" {
                in_extern = false;
                continue;
            }
            if let Some(rest) = l.trim_start().strip_prefix("pub fn ") {
                e.fns.insert(name_of(rest));
            } else if let Some(rest) = l.trim_start().strip_prefix("pub static mut ") {
                e.statics.insert(name_of(rest));
            } else if let Some(rest) = l.trim_start().strip_prefix("pub static ") {
                e.statics.insert(name_of(rest));
            }
            continue;
        }
        if let Some(rest) = l.strip_prefix("pub type ") {
            e.types.insert(name_of(rest));
        } else if let Some(rest) = l.strip_prefix("pub struct ") {
            e.types.insert(name_of(rest));
        } else if let Some(rest) = l.strip_prefix("pub const ") {
            e.consts.insert(name_of(rest));
        }
    }
    e
}

/// `lv_init(` → `lv_init`; `lv_font_t = ...` → `lv_font_t`
fn name_of(rest: &str) -> String {
    rest.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("")
        .to_string()
}

#[derive(Default)]
struct Allowlist {
    fns: BTreeSet<String>,
    types: BTreeSet<String>,
    vars: BTreeSet<String>,
}

fn parse_allowlist(src: &str) -> Allowlist {
    let mut a = Allowlist::default();
    for raw in src.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, name) = line.split_once(':').expect("want `fn:|type:|var:`");
        assert!(!name.contains('*'), "通配符禁止出现在 allowlist: {line}");
        match kind {
            "fn" => {
                a.fns.insert(name.to_string());
            }
            "type" => {
                a.types.insert(name.to_string());
            }
            "var" => {
                a.vars.insert(name.to_string());
            }
            other => panic!("未知 allowlist 类型 `{other}`: {line}"),
        }
    }
    a
}

fn collect_rs_sources(dir: &Path, out: &mut String) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let p = entry.expect("entry").path();
        if p.is_dir() {
            collect_rs_sources(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push_str(&fs::read_to_string(&p).unwrap_or_default());
        }
    }
}

#[test]
fn allowlist_is_bidirectionally_consistent() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bindings_path = Path::new(env!("OUT_DIR")).join("bindings.rs");
    let bindings = fs::read_to_string(&bindings_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", bindings_path.display()));
    let allow_src = fs::read_to_string(manifest.join("allowlist.txt")).expect("allowlist.txt");

    let exports = parse_bindings(&bindings);
    let allow = parse_allowlist(&allow_src);

    // 被检查的 Rust 文本：本 crate 的 src/ + examples/
    let mut rust_text = String::new();
    collect_rs_sources(&manifest.join("src"), &mut rust_text);
    collect_rs_sources(&manifest.join("examples"), &mut rust_text);

    // ── ① 导出 ⊆ allowlist（fn / var 硬断言）────────────────────
    let fn_leak: Vec<_> = exports.fns.difference(&allow.fns).cloned().collect();
    let var_leak: Vec<_> = exports.statics.difference(&allow.vars).cloned().collect();

    // 存在性：allowlist 里写的符号必须真的生成了（防 typo 静默失效）
    let fn_missing: Vec<_> = allow.fns.difference(&exports.fns).cloned().collect();
    let var_missing: Vec<_> = allow.vars.difference(&exports.statics).cloned().collect();
    let ty_missing: Vec<_> = allow.types.difference(&exports.types).cloned().collect();

    // ── ② allowlist 每个 fn/var 在 src|examples 里确有引用 ───────
    let dead: Vec<_> = allow
        .fns
        .iter()
        .chain(allow.vars.iter())
        .filter(|n| !rust_text.contains(n.as_str()))
        .cloned()
        .collect();

    // 闭包报告：bindings 里出现但 allowlist 没有的 type/const（recursively 带出来的）
    let closure_types: Vec<_> = exports.types.difference(&allow.types).cloned().collect();
    let closure_consts = exports.consts.len();

    println!("bindings.rs: {} 行", bindings.lines().count());
    println!(
        "导出: {} fn / {} static / {} type / {} const",
        exports.fns.len(),
        exports.statics.len(),
        exports.types.len(),
        exports.consts.len()
    );
    println!(
        "allowlist: {} fn / {} type / {} var",
        allow.fns.len(),
        allow.types.len(),
        allow.vars.len()
    );
    println!("① 越界导出 fn {fn_leak:?} / var {var_leak:?}");
    println!("① 缺失符号 fn {fn_missing:?} / var {var_missing:?} / type {ty_missing:?}");
    println!("② 无引用（死符号候选）: {dead:?}");
    println!("recursively 带出的额外 type（{}）: {closure_types:?}", closure_types.len());
    println!("recursively 带出的 enum const 数: {closure_consts}");

    // 未引用的闭包 type 允许存在，但必须是"传递闭包"而非新函数
    let mut problems: Vec<String> = Vec::new();
    if !fn_leak.is_empty() {
        problems.push(format!("① 有 {fn_leak:?} 函数未在 allowlist 中"));
    }
    if !var_leak.is_empty() {
        problems.push(format!("① 有 {var_leak:?} 变量未在 allowlist 中"));
    }
    if !fn_missing.is_empty() {
        problems.push(format!("allowlist 声明的函数在 bindings 里不存在: {fn_missing:?}"));
    }
    if !var_missing.is_empty() {
        problems.push(format!("allowlist 声明的变量在 bindings 里不存在: {var_missing:?}"));
    }
    if !ty_missing.is_empty() {
        problems.push(format!("allowlist 声明的类型在 bindings 里不存在: {ty_missing:?}"));
    }
    if !dead.is_empty() {
        problems.push(format!("② allowlist 里的死符号（无任何 Rust 引用）: {dead:?}"));
    }
    assert!(problems.is_empty(), "allowlist 一致性校验失败:\n{}", problems.join("\n"));

    let _: HashSet<&str> = HashSet::new();
    println!("PASS: allowlist 双向一致");
}
