//! lvgl-sys 构建脚本（设计 §12.1 的四个关键动作）
//!
//! 1. `cc` 递归编译 `mupc/vendor/lvgl/src/**\*.c`（**排除** `src/libs/**`，那是
//!    png/thorvg/freetype 一类第三方库，本项目全不启用）+ `.include(vendor/lvgl)` +
//!    `.include(lvgl-sys 自身目录)` + `.define("LV_CONF_INCLUDE_SIMPLE", None)`。
//! 2. bindgen 读 `allowlist.txt` 逐项 allowlist（禁 `lv_*` 通配），
//!    `allowlist_recursively(true)` 带上传递依赖类型。
//! 3. `rerun-if-changed` 覆盖 LVGL 源 + `lv_conf.h` + `allowlist.txt` + `fonts/`。
//! 4. 生成物写 `OUT_DIR/bindings.rs`，由 `src/lib.rs` `include!` 之。

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    // crates/local-display/lvgl-sys -> 仓库根/mupc/vendor/lvgl
    let lvgl_root = manifest_dir
        .join("..")
        .join("..")
        .join("..")
        .join("vendor")
        .join("lvgl");
    let lvgl_root = lvgl_root
        .canonicalize()
        .unwrap_or_else(|e| panic!("LVGL submodule not found at {}: {e}", lvgl_root.display()));
    // Windows: `canonicalize()` 返回 verbatim 前缀 `\\?\E:\...`，MSVC 的 cl.exe 打不开
    // 这种路径（C1083: 无法打开源文件）。必须剥掉前缀。
    let lvgl_root = strip_verbatim(lvgl_root);
    let lvgl_src = lvgl_root.join("src");
    assert!(
        lvgl_src.is_dir(),
        "LVGL submodule looks empty: {} (did `git submodule update --init` run?)",
        lvgl_root.display()
    );

    println!("cargo:rerun-if-changed=lv_conf.h");
    println!("cargo:rerun-if-changed=allowlist.txt");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", lvgl_src.display());
    println!("cargo:rerun-if-changed={}", lvgl_root.join("lvgl.h").display());

    // ── 1. cc：编译 LVGL 核心 C 源码 ─────────────────────────────────
    let sources = collect_c_sources(&lvgl_src);
    assert!(
        sources.len() > 200,
        "expected >200 LVGL .c sources, found {}",
        sources.len()
    );

    let mut build = cc::Build::new();
    build
        .files(&sources)
        .include(&lvgl_root)
        .include(&manifest_dir) // 让 `#include "lv_conf.h"`（LV_CONF_INCLUDE_SIMPLE）生效
        .define("LV_CONF_INCLUDE_SIMPLE", None)
        // 静默 LVGL 的第三方依赖探测（本项目全不开，但头文件里会探测宏）
        .warnings(false)
        .flag_if_supported("-w");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        build.flag_if_supported("-fvisibility=hidden");
    }
    build.compile("lvgl");

    // 字体产物（S-3）：crates/local-display/fonts/*.c，存在即编进来
    let fonts_dir = manifest_dir.join("..").join("fonts");
    if fonts_dir.is_dir() {
        println!("cargo:rerun-if-changed={}", fonts_dir.display());
        let fonts: Vec<PathBuf> = collect_ext(&fonts_dir, "c");
        if !fonts.is_empty() {
            let mut fb = cc::Build::new();
            fb.files(&fonts)
                .include(&lvgl_root)
                .include(&manifest_dir)
                .define("LV_CONF_INCLUDE_SIMPLE", None)
                .warnings(false)
                .flag_if_supported("-w");
            fb.compile("lvgl_fonts");
        }
    }

    // ── 2/4. bindgen：精确 allowlist → OUT_DIR/bindings.rs ───────────
    let mut builder = bindgen::Builder::default()
        .header(manifest_dir.join("wrapper.h").to_string_lossy().to_string())
        .clang_arg(format!("-I{}", lvgl_root.display()))
        .clang_arg(format!("-I{}", manifest_dir.display()))
        .clang_arg("-DLV_CONF_INCLUDE_SIMPLE")
        .allowlist_recursively(true)
        .layout_tests(false)
        .derive_default(false)
        .prepend_enum_name(false)
        .generate_comments(false);

    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    if !target.is_empty() && target != host {
        // 交叉编译：bindgen 在宿主上跑，但按目标三元组定尺寸。
        // 需要目标 libc 头（sysroot）；若报 stdarg.h/stdint.h 找不到，
        // 用 BINDGEN_EXTRA_CLANG_ARGS 补 `-isystem <sysroot>/usr/include`（设计 §12.1 预告）。
        builder = builder.clang_arg(format!("--target={target}"));
    }

    let allowlist = std::fs::read_to_string(manifest_dir.join("allowlist.txt"))
        .expect("allowlist.txt missing");
    let mut n_fn = 0usize;
    let mut n_ty = 0usize;
    let mut n_va = 0usize;
    for raw in allowlist.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, name) = line
            .split_once(':')
            .unwrap_or_else(|| panic!("bad allowlist line (want `fn:|type:|var:`): {line}"));
        assert!(
            !name.contains('*'),
            "wildcards are forbidden in allowlist.txt: {line}"
        );
        match kind {
            "fn" => {
                builder = builder.allowlist_function(name);
                n_fn += 1;
            }
            "type" => {
                builder = builder.allowlist_type(name);
                n_ty += 1;
            }
            "var" => {
                builder = builder.allowlist_var(name);
                n_va += 1;
            }
            other => panic!("unknown allowlist kind `{other}` in: {line}"),
        }
    }
    println!(
        "cargo:warning=lvgl-sys allowlist: {n_fn} fn / {n_ty} type / {n_va} var = {} entries",
        n_fn + n_ty + n_va
    );

    let bindings = builder
        .generate()
        .expect("bindgen failed (check LIBCLANG_PATH / BINDGEN_EXTRA_CLANG_ARGS)");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs");
    bindings
        .write_to_file(&out)
        .expect("failed to write bindings.rs");

    println!("cargo:rustc-link-lib=static=lvgl");
}

/// 递归收集 `dir` 下的 `.c`，**排除** `src/libs/**`（第三方库）。
fn collect_c_sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut |p| {
        if p.extension().and_then(|e| e.to_str()) == Some("c") {
            out.push(p.to_path_buf());
        }
    });
    out
}

/// 递归收集 `dir` 下指定扩展名的文件。
fn collect_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut |p| {
        if p.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(p.to_path_buf());
        }
    });
    out
}

/// Windows 长路径/verbatim 前缀剥离（MSVC cl.exe 不认 `\\?\` 前缀）。
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("\\\\?\\") {
        // `\\?\UNC\server\share` → `\\server\share`
        if let Some(unc) = rest.strip_prefix("UNC\\") {
            return PathBuf::from(format!("\\\\{unc}"));
        }
        return PathBuf::from(rest.to_string());
    }
    p
}

fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            // src/libs/**：png/thorvg/freetype/gif/... 第三方库，本项目一律不启用。
            // 例外（spike 实测发现）：`lv_init.c` 里 **无条件** 调用
            // `lv_bin_decoder_init()`（无 LV_USE_* 守卫），而它定义在
            // src/libs/bin_decoder/ —— 所以这两个子目录**必须**编进来，否则
            // 链接期报 LNK2019: 无法解析的外部符号 lv_bin_decoder_init。
            if path.file_name().and_then(|n| n.to_str()) == Some("libs") {
                for allowed in ["bin_decoder", "rle"] {
                    let sub = path.join(allowed);
                    if sub.is_dir() {
                        walk(&sub, f);
                    }
                }
                continue;
            }
            walk(&path, f);
        } else {
            f(&path);
        }
    }
}
