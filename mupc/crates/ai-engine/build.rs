//! mupc-ai-engine build script
//!
//! ## 职责
//!
//! - `npu` feature 启用时：提供 `librknnrt.so` 的链接搜索路径
//! - 自动检测 RKNN SDK 路径 (优先级: RKNN_VENDOR_DIR > 环境变量 > 自动搜索)
//! - SHA256 完整性校验：确保 vendor 目录下的 `librknnrt.so` 未被篡改
//!
//! ## 环境变量
//!
//! - `RKNN_VENDOR_DIR`: 直接指定 librknnrt.so 所在目录
//! - `RKNN_SDK_ROOT`: RKNN Toolkit SDK 根目录
//!   (自动拼接 `rknpu2/runtime/Linux/librknn_api/aarch64/`)
//! - `CARGO_FEATURE_NPU`: Cargo 自动设置，指示 `npu` feature 是否启用

use std::path::Path;

fn main() {
    let npu_enabled = std::env::var("CARGO_FEATURE_NPU").is_ok();
    let is_linux = std::env::var("CARGO_CFG_TARGET_OS").map(|os| os == "linux").unwrap_or(false);

    if !npu_enabled {
        println!("cargo:warning=AI Engine: npu feature 未启用，跳过 RKNN FFI 链接");
        return;
    }

    if !is_linux {
        println!("cargo:warning=AI Engine: 非 Linux 平台 ({}), npu feature 使用 stub 实现",
            std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default());
        return;
    }

    // ── 1. 确定 RKNN Library 目录 ──
    // 优先级: RKNN_VENDOR_DIR > vendor/rknn (自动复制) > RKNN_SDK_ROOT 自动推导 > 默认 vendor/rknn
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_vendor = Path::new(&manifest_dir)
        .parent()  // crates/
        .unwrap()
        .parent()  // mupc/
        .unwrap()
        .join("vendor")
        .join("rknn");

    // 创建 vendor/rknn 目录 (如果不存在)
    let _ = std::fs::create_dir_all(&workspace_vendor);

    let vendor_dir = if let Ok(custom) = std::env::var("RKNN_VENDOR_DIR") {
        // 用户显式指定
        let p = Path::new(&custom);
        if p.exists() {
            println!("cargo:warning=使用自定义 RKNN_VENDOR_DIR: {}", custom);
            p.to_path_buf()
        } else {
            println!("cargo:warning=RKNN_VENDOR_DIR 指定但不存在: {}，回退默认", custom);
            workspace_vendor
        }
    } else {
        // 自动检测 RKNN SDK
        let auto_vendor = find_rknn_library();
        if let Some(ref auto_path) = auto_vendor {
            println!("cargo:warning=自动检测 RKNN SDK: {}", auto_path.display());
            // 复制到 vendor/rknn/ 用于 SHA256 校验和统一管理
            copy_rknn_to_vendor(auto_path, &workspace_vendor);
        }
        workspace_vendor
    };

    // ── 2. 链接设置 ──
    println!("cargo:rustc-link-search=native={}", vendor_dir.display());
    println!("cargo:rustc-link-lib=dylib=stdc++");
    // 注: #[link(name = "rknnrt")] 在 rknn_runtime_sys.rs 中声明,
    // build.rs 只提供 -L 搜索路径和间接依赖 stdc++

    // ── 3. SHA256 完整性校验 ──
    let so_path = vendor_dir.join("librknnrt.so");
    if so_path.exists() {
        let hash_file = vendor_dir.join("librknnrt.so.sha256");
        let unverified_file = vendor_dir.join("librknnrt.so.sha256.unverified");

        if hash_file.exists() {
            // 已验证文件存在 → 校验 SHA256
            let expected = std::fs::read_to_string(&hash_file)
                .expect("Failed to read SHA256 file");
            let expected = expected
                .split_whitespace()
                .next()
                .expect("Invalid SHA256 file format");
            let actual = compute_sha256(
                &std::fs::read(&so_path).expect("Failed to read librknnrt.so"),
            );
            if expected != actual {
                panic!(
                    "librknnrt.so SHA256 校验失败!\n  Expected: {}\n  Actual:   {}\n  \
                     文件可能被篡改或损坏。如需更换合法版本，请同时删除 .sha256 文件后重新构建。",
                    expected, actual
                );
            }
            println!("cargo:warning=librknnrt.so SHA256 校验通过");
        } else if unverified_file.exists() {
            // 未验证文件存在 → 要求用户手动确认
            panic!(
                "librknnrt.so 未验证！请确认 SHA256 与官方 SDK 一致后:\n  \
                 mv {} {}\n  \
                 官方 SDK: https://github.com/airockchip/rknn-toolkit2",
                unverified_file.display(),
                hash_file.display()
            );
        } else {
            // 首次导入 → 生成 .unverified 文件，强制用户确认
            let actual = compute_sha256(
                &std::fs::read(&so_path).expect("Failed to read librknnrt.so"),
            );
            std::fs::write(&unverified_file, format!("{}  librknnrt.so\n", actual))
                .expect("Failed to write SHA256 file");
            println!("cargo:warning=╔══════════════════════════════════════════════════════════════╗");
            println!("cargo:warning=║  librknnrt.so 首次导入 — SHA256 校验文件已生成               ║");
            println!("cargo:warning=║  SHA256: {} ║", actual);
            println!("cargo:warning=║  请确认此哈希与 Rockchip 官方 SDK 一致后，重命名文件:        ║");
            println!("cargo:warning=║    mv {} {} ║",
                unverified_file.display(), hash_file.display());
            println!("cargo:warning=║  官方 SDK: https://github.com/airockchip/rknn-toolkit2       ║");
            println!("cargo:warning=╚══════════════════════════════════════════════════════════════╝");
            panic!("librknnrt.so 未验证。请按上述提示确认 SHA256 后重命名 .unverified 文件，然后重新构建。");
        }
    } else {
        println!(
            "cargo:warning=librknnrt.so 未找到 ({}), 跳过 SHA256 校验",
            vendor_dir.display()
        );
        println!("cargo:warning=提示: 将 librknnrt.so 复制到 {} 或设置 RKNN_VENDOR_DIR", vendor_dir.display());
    }
}

/// 自动搜索 RKNN Runtime 库
///
/// 搜索顺序:
///   1. RKNN_SDK_ROOT 环境变量 + 标准子路径
///   2. 项目父目录 rknn-toolkit2-2.3.2
///   3. /opt/rknn
fn find_rknn_library() -> Option<std::path::PathBuf> {
    // 方法 1: RKNN_SDK_ROOT 环境变量
    if let Ok(sdk_root) = std::env::var("RKNN_SDK_ROOT") {
        let candidates = vec![
            Path::new(&sdk_root)
                .join("rknpu2/runtime/Linux/librknn_api/aarch64/librknnrt.so"),
            Path::new(&sdk_root)
                .join("rknpu2/runtime/Linux/librknn_api/armhf/librknnrt.so"),
            Path::new(&sdk_root).join("librknnrt.so"),
        ];
        for c in &candidates {
            if c.exists() {
                return Some(c.clone());
            }
        }
    }

    // 方法 2: 项目父目录搜索
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root =
        Path::new(&manifest_dir).parent()?.parent()?; // crates/ -> mupc/
    let project_parent = workspace_root.parent()?;      // mupc/ -> workspace/

    let rknn_root = project_parent.join("rknn-toolkit2-2.3.2");
    if rknn_root.exists() {
        let candidates = vec![
            rknn_root.join("rknpu2/runtime/Linux/librknn_api/aarch64/librknnrt.so"),
            rknn_root.join("rknpu2/runtime/Linux/librknn_api/armhf/librknnrt.so"),
        ];
        for c in &candidates {
            if c.exists() {
                return Some(c.clone());
            }
        }
    }

    // 方法 3: 系统路径
    let sys_paths = vec![
        Path::new("/opt/rknn/librknnrt.so"),
        Path::new("/usr/local/lib/librknnrt.so"),
    ];
    for p in &sys_paths {
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }

    None
}

/// 将 RKNN .so 复制到 vendor/rknn/ 目录 (保留原始文件)
fn copy_rknn_to_vendor(src: &Path, vendor_dir: &Path) {
    let dest = vendor_dir.join("librknnrt.so");
    if dest.exists() {
        return; // 已存在，不覆盖
    }
    if let Err(e) = std::fs::copy(src, &dest) {
        println!(
            "cargo:warning=复制 librknnrt.so 失败 ({} → {}): {}",
            src.display(),
            dest.display(),
            e
        );
    } else {
        println!(
            "cargo:warning=已将 librknnrt.so 复制到 {}",
            dest.display()
        );
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
