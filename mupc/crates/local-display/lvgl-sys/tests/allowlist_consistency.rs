//! allowlist 双向一致性校验（设计 §1.1.1.2 / §12.1 的 CI 断言，本 spike 先落地为可运行测试）。
//!
//! 运行：`cd mupc && cargo test -p lvgl-sys --test allowlist_consistency -j 2`
//!
//! 断言：
//!  ① **fn / var / type 导出 ⊆ allowlist.txt**（防"偷偷全量生成"，这是禁 `lv_*` 通配的核心保障）；
//!  ② allowlist 中每个 **fn / var** 在 `src/**` 或 `examples/**` 里确有文本引用（无死符号）。
//!
//! 说明：
//!   - 本测试对 `type` 已是**硬断言**（设计 §1.1.1.2 的"落地路径"：薄层 `src/lvgl/**`
//!     落地后把 `type` 也升为硬断言）。`allowlist_recursively(true)` 必然带出的
//!     **传递依赖 `type`** 已在 `allowlist.txt` 文末分节**显式登记**（含原因说明），
//!     故不需要在测试里做"二值化"特判：`导出 type ⊆ 清单` + `清单 type ⊆ 导出`
//!     **两者同时断言 ⇒ `type` 集合相等**（多一个 / 少一个都失败）——"新增 type
//!     静默通过"的漏洞被彻底堵死。
//!   - `const:`（bindgen 为 enum 成员生成的常量）**不设硬断言**：它们由已断言的
//!     enum `type` 完全决定（新增 enum 必先使 `type` 断言失败），且把 109 个枚举
//!     成员逐条写进清单只是噪音。此处仅打印数量作为"LVGL/bindgen 版本变动"的观察哨。
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
        // bindgen 也会为匿名 union / enum 生成具名条目（如
        // `lv_font_glyph_dsc_t__bindgen_ty_1`）。若不识别，这类 type 就**既不在
        // 清单也不被断言** —— 是"type 集合相等"断言的缺口。`pub enum` 一并识别以
        // 对冲 bindgen 配置漂移（如启用 rustified_enum 后形态变化）。
        } else if let Some(rest) = l.strip_prefix("pub union ") {
            e.types.insert(name_of(rest));
        } else if let Some(rest) = l.strip_prefix("pub enum ") {
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
            let src = fs::read_to_string(&p).unwrap_or_default();
            // 剥离注释与字符串字面量后再累积：死符号检查必须看"真实代码引用"，
            // 否则在注释/字符串里写一遍符号名就能满足旧口径（弱断言）。
            out.push_str(&strip_comments_and_strings(&src));
        }
    }
}

/// 剥离 Rust 源码里的注释与字符串/字符字面量（保留其余代码文本）。
///
/// 处理：行注释、**可嵌套**块注释、普通字符串、raw 字符串（`r"…"` / `r#"…"#`）、
/// 字符字面量；`'a`（生命周期）**不**当字符字面量剥掉。
fn strip_comments_and_strings(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        // 行注释
        if c == '/' && i + 1 < b.len() && b[i + 1] == '/' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // 块注释（Rust 支持嵌套）
        if c == '/' && i + 1 < b.len() && b[i + 1] == '*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if b[i] == '/' && i + 1 < b.len() && b[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if b[i] == '*' && i + 1 < b.len() && b[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            out.push(' ');
            continue;
        }
        // raw 字符串（`r"…"` / `r#"…"#` / `r##"…"##`）
        if c == 'r' {
            if let Some(next) = skip_raw_string(&b, i) {
                out.push(' ');
                i = next;
                continue;
            }
        }
        // 普通字符串
        if c == '"' {
            i += 1;
            while i < b.len() {
                if b[i] == '\\' {
                    i += 2;
                    continue;
                }
                if b[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(' ');
            continue;
        }
        // 字符字面量（`'x'` / `'\n'` / `'\''`）；生命周期（`'a`）保持原样。
        if c == '\'' {
            let is_char_lit = (i + 1 < b.len() && b[i + 1] == '\\')
                || (i + 2 < b.len() && b[i + 2] == '\'');
            if is_char_lit {
                i += 1;
                while i < b.len() {
                    if b[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == '\'' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                out.push(' ');
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 若 `b[start]` 起是 raw 字符串，返回其结束后的下标。
fn skip_raw_string(b: &[char], start: usize) -> Option<usize> {
    if b[start] != 'r' {
        return None;
    }
    let mut i = start + 1;
    let mut hashes = 0usize;
    while i < b.len() && b[i] == '#' {
        hashes += 1;
        i += 1;
    }
    if i >= b.len() || b[i] != '"' {
        return None;
    }
    i += 1;
    while i < b.len() {
        if b[i] == '"' {
            let mut j = i + 1;
            let mut h = 0usize;
            while j < b.len() && b[j] == '#' && h < hashes {
                h += 1;
                j += 1;
            }
            if h == hashes {
                return Some(j);
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    Some(b.len())
}

/// 以**标识符词边界**匹配 `needle`（ASCII 符号名）是否作为独立 token 出现在 `hay`。
///
/// 优于 `hay.contains(needle)`：`lv_init` 不会因为 `lv_init_foo` / `my_lv_init` 而误判
/// 为"有引用"。
fn contains_ident(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let bytes = hay.as_bytes();
    let mut from = 0usize;
    while from + needle.len() <= hay.len() {
        let Some(rel) = hay[from..].find(needle) else {
            return false;
        };
        let pos = from + rel;
        let end = pos + needle.len();
        let before_ok = pos == 0 || !is_ident_byte(bytes[pos - 1]);
        let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        // 推进一个字节：`needle` 为 ASCII，命中处必为 char 边界。
        from = pos + 1;
    }
    false
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
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

    // 被检查的 Rust 文本 —— 这是**真实用点面**（设计 §12.1 ②："清单内每个符号在
    // `src/lvgl/**` + `ui/**` 中确有引用"）：
    //   ① 本 crate 的 src/ + examples/（spike 期用点）；
    //   ② **上层 `local-display/src/**`（薄安全层 `src/lvgl/**` 在此）** —— 工作单元 A1
    //      起，清单符号的主要引用者已从 examples 迁到薄层；只扫本 crate 会把薄层刚用上的
    //      符号误判为"死符号"。取并集，断言只增不减。
    let mut rust_text = String::new();
    collect_rs_sources(&manifest.join("src"), &mut rust_text);
    collect_rs_sources(&manifest.join("examples"), &mut rust_text);
    let thin_layer_and_ui = manifest.join("..").join("src");
    if thin_layer_and_ui.is_dir() {
        collect_rs_sources(&thin_layer_and_ui, &mut rust_text);
    }

    // ── ① 导出 ⊆ allowlist（fn / var / type 均为硬断言）───────────
    let fn_leak: Vec<_> = exports.fns.difference(&allow.fns).cloned().collect();
    let var_leak: Vec<_> = exports.statics.difference(&allow.vars).cloned().collect();
    // type 硬断言（设计 §1.1.1.2）：递归带出的传递依赖 type 已在 allowlist.txt
    // 文末分节显式登记，故此处可严格 ⊆ —— 新增 type 不再静默通过。
    let ty_leak: Vec<_> = exports.types.difference(&allow.types).cloned().collect();

    // 存在性：allowlist 里写的符号必须真的生成了（防 typo 静默失效）
    let fn_missing: Vec<_> = allow.fns.difference(&exports.fns).cloned().collect();
    let var_missing: Vec<_> = allow.vars.difference(&exports.statics).cloned().collect();
    let ty_missing: Vec<_> = allow.types.difference(&exports.types).cloned().collect();

    // ── ② allowlist 每个 fn/var 在 src|examples 里确有引用 ───────
    // type 不参与：其中多数是 bindgen 递归带出的传递依赖，无直接文本引用。
    // 口径已加强：`rust_text` 是**剥离注释与字符串字面量**后的代码文本，且按
    // **标识符词边界**匹配 —— 只在注释/字符串里写一遍符号名，或仅是别的标识符的
    // 子串（`lv_init` vs `lv_init_foo`），都不再算"有引用"。
    let dead: Vec<_> = allow
        .fns
        .iter()
        .chain(allow.vars.iter())
        .filter(|n| !contains_ident(&rust_text, n.as_str()))
        .cloned()
        .collect();

    // enum const 仅作版本变动观察哨（见文件头说明：由已断言的 enum type 完全决定）。
    let const_count = exports.consts.len();

    println!("bindings.rs: {} 行", bindings.lines().count());
    println!(
        "导出: {} fn / {} static / {} type / {} const",
        exports.fns.len(),
        exports.statics.len(),
        exports.types.len(),
        const_count
    );
    println!(
        "allowlist: {} fn / {} type / {} var",
        allow.fns.len(),
        allow.types.len(),
        allow.vars.len()
    );
    println!("① 越界导出 fn {fn_leak:?} / var {var_leak:?} / type {ty_leak:?}");
    println!("① 缺失符号 fn {fn_missing:?} / var {var_missing:?} / type {ty_missing:?}");
    println!("② 无引用（死符号候选）: {dead:?}");
    println!("enum const 数（观察哨，非断言）: {const_count}");

    // ① 的"⊆"与存在性同时成立 ⇒ fn/var/type 三类的导出集与清单集**逐类相等**。
    let mut problems: Vec<String> = Vec::new();
    if !fn_leak.is_empty() {
        problems.push(format!("① 越界: 以下 fn 已导出但未登记在 allowlist.txt: {fn_leak:?}"));
    }
    if !var_leak.is_empty() {
        problems.push(format!("① 越界: 以下 var 已导出但未登记在 allowlist.txt: {var_leak:?}"));
    }
    if !ty_leak.is_empty() {
        problems.push(format!(
            "① 越界: 以下 type 已导出但未登记在 allowlist.txt（新增 type 必须显式登记，\
             递归带出的请归入文末'递归带出的传递依赖 type'分节）: {ty_leak:?}"
        ));
    }
    if !fn_missing.is_empty() {
        problems.push(format!("allowlist 声明的 fn 在 bindings 里不存在（拼写错误？）: {fn_missing:?}"));
    }
    if !var_missing.is_empty() {
        problems.push(format!("allowlist 声明的 var 在 bindings 里不存在（拼写错误？）: {var_missing:?}"));
    }
    if !ty_missing.is_empty() {
        problems.push(format!(
            "allowlist 声明的 type 在 bindings 里不存在（拼写错误？或该 type 已随 LVGL/bindgen \
             升级消失，需删条目并确认闭包未变）: {ty_missing:?}"
        ));
    }
    if !dead.is_empty() {
        problems.push(format!("② allowlist 里的死符号（无任何 Rust 引用）: {dead:?}"));
    }
    assert!(problems.is_empty(), "allowlist 一致性校验失败:\n{}", problems.join("\n"));

    let _: HashSet<&str> = HashSet::new();
    println!("PASS: allowlist 双向一致（fn/var/type 三类集合相等）");
}
