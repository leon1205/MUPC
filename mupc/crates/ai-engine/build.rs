//! mupc-ai-engine build script
//!
//! ## 职责
//!
//! - `npu` feature 启用时：提供 `librknnrt.so` 的链接搜索路径
//! - SHA256 完整性校验：确保 vendor 目录下的 `librknnrt.so` 未被篡改
//! - 首次导入时自动生成 `.sha256` 校验文件
//!
//! ## 环境变量
//!
//! - `RKNN_VENDOR_DIR`: 覆盖默认的 `vendor/rknn/` 搜索路径
//! - `CARGO_FEATURE_NPU`: Cargo 自动设置，指示 `npu` feature 是否启用

use std::path::Path;

fn main() {
    let npu_enabled = std::env::var("CARGO_FEATURE_NPU").is_ok();

    if !npu_enabled {
        println!("cargo:warning=AI Engine: npu feature 未启用，跳过 RKNN FFI 链接");
        return;
    }

    // ── RKNN Vendor 目录 ──
    // 优先级: 环境变量 RKNN_VENDOR_DIR > 默认 vendor/rknn (workspace 根)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let default_vendor = Path::new(&manifest_dir)
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap() // mupc/
        .join("vendor")
        .join("rknn");

    let vendor_dir = std::env::var("RKNN_VENDOR_DIR")
        .unwrap_or_else(|_| default_vendor.to_string_lossy().to_string());

    println!("cargo:rustc-link-search=native={}", vendor_dir);
    println!("cargo:rustc-link-lib=dylib=stdc++");
    // 注: #[link(name = "rknnrt")] 在 rknn_runtime_sys.rs 中声明，
    // build.rs 只提供 -L 搜索路径和间接依赖 stdc++

    // ── SHA256 完整性校验 ──
    let so_path = Path::new(&vendor_dir).join("librknnrt.so");
    if so_path.exists() {
        let hash_file = Path::new(&vendor_dir).join("librknnrt.so.sha256");
        if hash_file.exists() {
            let expected = std::fs::read_to_string(&hash_file)
                .expect("Failed to read SHA256 file");
            let expected = expected
                .split_whitespace()
                .next()
                .expect("Invalid SHA256 file format");
            let actual = compute_sha256(
                &std::fs::read(&so_path).expect("Failed to read librknnrt.so"),
            );
            assert_eq!(
                expected, actual,
                "librknnrt.so SHA256 mismatch!\n  Expected: {}\n  Got:      {}",
                expected, actual
            );
            println!("cargo:warning=librknnrt.so SHA256 校验通过");
        } else {
            // 首次导入：自动生成 .sha256 文件
            let actual = compute_sha256(
                &std::fs::read(&so_path).expect("Failed to read librknnrt.so"),
            );
            std::fs::write(&hash_file, format!("{}  librknnrt.so\n", actual))
                .expect("Failed to write SHA256 file");
            println!("cargo:warning=已自动生成 librknnrt.so.sha256 校验文件");
        }
    } else {
        println!("cargo:warning=librknnrt.so 未找到，跳过 SHA256 校验");
    }
}

fn compute_sha256(data: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    let hash = sha2::Sha256::digest(data);
    let mut s = String::with_capacity(64);
    for byte in hash.iter() {
        write!(&mut s, "{:02x}", byte).unwrap();
    }
    s
}
