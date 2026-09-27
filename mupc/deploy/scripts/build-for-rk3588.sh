#!/bin/bash
# build-for-rk3588.sh — MUPC RK3588 本地/交叉编译一键脚本
#
# 用法:
#   本机构建 (RK3588 开发板):  ./build-for-rk3588.sh
#   交叉编译 (x86_64 → ARM64):  ./build-for-rk3588.sh --cross
#   CMake 包装:                 ./build-for-rk3588.sh --cmake
#   Docker 构建:                ./build-for-rk3588.sh --docker
#   只出主控（不编屏程序）:      ./build-for-rk3588.sh [--cross] --no-display
#
# 产物（**两个独立进程 / 两个独立 systemd unit**，U-72）:
#   target/{aarch64-unknown-linux-gnu}/release/mupcd              → mupcd.service
#   target/{aarch64-unknown-linux-gnu}/release/mupc-local-display → mupc-display.service
#
# 环境变量 (可选):
#   RKNN_SDK_ROOT    RKNN Toolkit SDK 根目录
#   CROSS_TARGET     交叉编译目标 (默认 aarch64-unknown-linux-gnu)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# `PROJECT_DIR` = **Cargo workspace 根（`mupc/`）**，不是脚本所在目录。
# ⚠️ 原先写作 `PROJECT_DIR="$SCRIPT_DIR"`（= `deploy/scripts`）⇒ `$PROJECT_DIR/target`、
# `$PROJECT_DIR/docker/Dockerfile.build`、`$PROJECT_DIR/build` **全部指向不存在的目录**
# （末段「=== Build complete ===」因此永远报 WARNING 而看不到真实产物）。本批（U-72）
# 修正为 workspace 根，并把原先靠多写 `../..` 补偿的三处（RKNN 探测路径、docker 挂载、
# docker 构建上下文）一并改回按 workspace 根表达。
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
MODE="native"
CARGO_FLAGS="--release -p mupc-core-bin"
FEATURES=""

# ── U-72：屏程序（12 号本地显示终端）一并构建 ──────────────────────────────
# 修正的背景：屏程序是**独立进程 + 独立 systemd unit**（`deploy/systemd/mupc-display.service`），
# 而它的构建步骤此前**只**存在于 `deploy/local-display.md` 的手工命令里 ⇒ 一键产物可能缺屏
# 程序，是现场「屏不亮」的直接成因之一（技术债 **U-72**）。故这里**默认一并构建**，
# `--no-display` 可关（例如机器上没生成字库、本次只要主控）。
BUILD_DISPLAY="yes"
# ⚠️ 屏程序**必须**带 `noto-font`（`deploy/systemd/mupc-display.service` 明文：「出包必须带
# 该 feature」，否则中文为占位框）。字库产物（`fonts/lv_font_noto_sc_*.c`）不入库 ⇒ 缺失时
# `lvgl-sys` 的 build.rs 会**响亮报错**并给出 `gen_fonts.sh` 指引，**不会**静默编出一个
# 没字库的产物。（`--features npu` 只对 `mupc-core-bin` 有效，故屏程序用**独立的一次 cargo 调用**，
# 不与主控共用 `$FEATURES` —— 避免把 `npu` 传给不含该 feature 的包。）
DISPLAY_FLAGS="--release -p local-display --features noto-font"

# ── 参数解析 ──
while [[ $# -gt 0 ]]; do
    case "$1" in
        --cross)   MODE="cross"; shift ;;
        --cmake)   MODE="cmake"; shift ;;
        --docker)  MODE="docker"; shift ;;
        --debug)   CARGO_FLAGS="-p mupc-core-bin"; DISPLAY_FLAGS="-p local-display --features noto-font"; shift ;;
        --no-npu)  FEATURES=""; shift ;;  # 显式禁用 NPU
        --no-display) BUILD_DISPLAY="no"; shift ;;  # 只出主控（U-72 的退出口）
        --help|-h)
            echo "Usage: $0 [--cross|--cmake|--docker] [--debug] [--no-npu] [--no-display]"
            echo "  默认同时产出 mupcd 与 mupc-local-display（U-72）；--no-display 只出 mupcd。"
            exit 0
            ;;
        *) echo "Unknown flag: $1"; exit 1 ;;
    esac
done

# ── RKNN SDK 自动检测 ──
setup_rknn() {
    if [ -n "${RKNN_SDK_ROOT:-}" ]; then
        echo "[RKNN] Using RKNN_SDK_ROOT=$RKNN_SDK_ROOT"
        return
    fi

    # 自动检测
    local detect_paths=(
        # 仓库根（MUPC2）= workspace 根（mupc）的上一级
        # （早先少一级，导致 SDK 在机器上也探测不到）
        "$PROJECT_DIR/../rknn-toolkit2-2.3.2"
        "/opt/rknn"
        "$HOME/rknn-toolkit2-2.3.2"
    )
    for p in "${detect_paths[@]}"; do
        if [ -d "$p" ]; then
            export RKNN_SDK_ROOT="$p"
            echo "[RKNN] Auto-detected: $p"
            return
        fi
    done

    echo "[RKNN] WARNING: RKNN SDK not found. Building without NPU."
    echo "[RKNN] Set RKNN_SDK_ROOT env var or install to one of: ${detect_paths[*]}"
}

setup_rknn

# 设置 NPU feature
if [ -n "${RKNN_SDK_ROOT:-}" ]; then
    # 尝试找到 librknnrt.so 并设置 vendor dir
    RKNN_LIB=$(find "$RKNN_SDK_ROOT" -name "librknnrt.so" 2>/dev/null | head -1 || true)
    if [ -n "$RKNN_LIB" ]; then
        export RKNN_VENDOR_DIR="$(dirname "$RKNN_LIB")"
        echo "[RKNN] Library: $RKNN_LIB"
        echo "[RKNN] Vendor dir: $RKNN_VENDOR_DIR"
        FEATURES="--features npu"
    fi
fi

# ── 屏程序构建（U-72）──
# 参数：$1 = 目标三元组（空 = 本机）；$2 = 构建器（`cargo` / `cross`，默认 `cargo`）
build_display() {
    if [ "$BUILD_DISPLAY" != "yes" ]; then
        echo "[display] skipped (--no-display)"
        return 0
    fi
    local target="${1:-}"
    local builder="${2:-cargo}"
    local target_arg=""
    [ -n "$target" ] && target_arg="--target $target"
    echo "=== Build display terminal (mupc-local-display, noto-font) ==="
    $builder build $DISPLAY_FLAGS $target_arg
}

# ── 构建 ──
case "$MODE" in
    native)
        echo "=== Native build (aarch64) ==="
        cargo build $CARGO_FLAGS $FEATURES
        build_display ""
        ;;
    cross)
        echo "=== Cross-compile build (x86_64 → aarch64) ==="
        CROSS_TARGET="${CROSS_TARGET:-aarch64-unknown-linux-gnu}"

        # 检查交叉编译器
        if ! command -v aarch64-linux-gnu-gcc &>/dev/null; then
            echo "ERROR: aarch64-linux-gnu-gcc not found."
            echo "  Install: sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu"
            exit 1
        fi

        # 检查 cross 工具
        if command -v cross &>/dev/null; then
            echo "Using cross-rs for containerized build..."
            BUILDER="cross"
            if [ -n "${RKNN_VENDOR_DIR:-}" ]; then
                cross build $CARGO_FLAGS --target "$CROSS_TARGET" $FEATURES
            else
                cross build $CARGO_FLAGS --target "$CROSS_TARGET"
            fi
        else
            echo "Using native cross-compiler..."
            BUILDER="cargo"
            if [ -n "${RKNN_VENDOR_DIR:-}" ]; then
                cargo build $CARGO_FLAGS --target "$CROSS_TARGET" $FEATURES
            else
                cargo build $CARGO_FLAGS --target "$CROSS_TARGET"
            fi
        fi
        # 屏程序与主控用**同一个**构建器 / 同一个目标三元组，避免两侧产物架构不一致。
        build_display "$CROSS_TARGET" "$BUILDER"
        ;;
    cmake)
        echo "=== CMake build ==="
        BUILD_DIR="$PROJECT_DIR/build"
        cmake -B "$BUILD_DIR" -DCMAKE_BUILD_TYPE=Release -DENABLE_NPU=ON
        cmake --build "$BUILD_DIR"
        # ⚠️ CMakeLists.txt 的 CARGO_BUILD_TARGET 只含 mupc-core-bin ⇒ 屏程序**不在** CMake
        # 产物内（U-72 同源缺口），故这里补一次本机 cargo 构建。
        build_display ""
        ;;
    docker)
        echo "=== Docker build ==="
        DOCKERFILE="$PROJECT_DIR/docker/Dockerfile.build"
        IMAGE="mupc-build:latest"

        # 构建镜像
        # 上下文 = workspace 根（mupc）。该 Dockerfile **无 COPY/ADD**（纯工具链镜像，
        # 源码靠下面的 `-v` 挂载），故上下文仅作占位；但不可写成 `$PROJECT_DIR/../..`
        # —— 在 `PROJECT_DIR` = workspace 根之后那会变成整块盘符根，docker 会试图打包整个盘。
        docker build -t "$IMAGE" -f "$DOCKERFILE" "$PROJECT_DIR"

        # 运行编译
        MOUNTS="-v $PROJECT_DIR:/workspace/MUPC"
        if [ -n "${RKNN_SDK_ROOT:-}" ]; then
            MOUNTS="$MOUNTS -v $RKNN_SDK_ROOT:/opt/rknn"
        fi

        INNER="cargo build $CARGO_FLAGS $FEATURES"
        if [ "$BUILD_DISPLAY" = "yes" ]; then
            INNER="$INNER && cargo build $DISPLAY_FLAGS"
        fi
        docker run --rm $MOUNTS "$IMAGE" sh -c "$INNER"
        ;;
esac

# ── 显示产物 ──
echo ""
echo "=== Build complete ==="
TARGET_DIR="$PROJECT_DIR/target"
if [ "$MODE" = "cross" ]; then
    TARGET_DIR="$TARGET_DIR/aarch64-unknown-linux-gnu"
fi

show_bin() {
    local bin="$TARGET_DIR/release/$1"
    if [ -f "$bin" ]; then
        echo "Binary: $bin"
        file "$bin"
        ls -lh "$bin"
    else
        echo "WARNING: $1 binary not found. Check build output for errors."
    fi
}

show_bin mupcd
# U-72：屏程序是**独立**产物（独立 systemd unit `mupc-display.service`）⇒ 单独核对落点。
if [ "$BUILD_DISPLAY" = "yes" ]; then
    show_bin mupc-local-display
fi
