#!/bin/bash
# build-for-rk3588.sh — MUPC RK3588 本地/交叉编译一键脚本
#
# 用法:
#   本机构建 (RK3588 开发板):  ./build-for-rk3588.sh
#   交叉编译 (x86_64 → ARM64):  ./build-for-rk3588.sh --cross
#   CMake 包装:                 ./build-for-rk3588.sh --cmake
#   Docker 构建:                ./build-for-rk3588.sh --docker
#
# 产物: target/{aarch64-unknown-linux-gnu}/release/mupcd
#
# 环境变量 (可选):
#   RKNN_SDK_ROOT    RKNN Toolkit SDK 根目录
#   CROSS_TARGET     交叉编译目标 (默认 aarch64-unknown-linux-gnu)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$SCRIPT_DIR"
MODE="native"
CARGO_FLAGS="--release -p mupc-core-bin"
FEATURES=""

# ── 参数解析 ──
while [[ $# -gt 0 ]]; do
    case "$1" in
        --cross)   MODE="cross"; shift ;;
        --cmake)   MODE="cmake"; shift ;;
        --docker)  MODE="docker"; shift ;;
        --debug)   CARGO_FLAGS="-p mupc-core-bin"; shift ;;
        --no-npu)  FEATURES=""; shift ;;  # 显式禁用 NPU
        --help|-h)
            echo "Usage: $0 [--cross|--cmake|--docker] [--debug] [--no-npu]"
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
        "$PROJECT_DIR/../../rknn-toolkit2-2.3.2"
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

# ── 构建 ──
case "$MODE" in
    native)
        echo "=== Native build (aarch64) ==="
        cargo build $CARGO_FLAGS $FEATURES
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
            if [ -n "$RKNN_VENDOR_DIR" ]; then
                cross build $CARGO_FLAGS --target "$CROSS_TARGET" $FEATURES
            else
                cross build $CARGO_FLAGS --target "$CROSS_TARGET"
            fi
        else
            echo "Using native cross-compiler..."
            if [ -n "$RKNN_VENDOR_DIR" ]; then
                cargo build $CARGO_FLAGS --target "$CROSS_TARGET" $FEATURES
            else
                cargo build $CARGO_FLAGS --target "$CROSS_TARGET"
            fi
        fi
        ;;
    cmake)
        echo "=== CMake build ==="
        BUILD_DIR="$PROJECT_DIR/build"
        cmake -B "$BUILD_DIR" -DCMAKE_BUILD_TYPE=Release -DENABLE_NPU=ON
        cmake --build "$BUILD_DIR"
        ;;
    docker)
        echo "=== Docker build ==="
        DOCKERFILE="$PROJECT_DIR/docker/Dockerfile.build"
        IMAGE="mupc-build:latest"

        # 构建镜像
        docker build -t "$IMAGE" -f "$DOCKERFILE" "$PROJECT_DIR/../.."

        # 运行编译
        MOUNTS="-v $PROJECT_DIR/../..:/workspace/MUPC"
        if [ -n "${RKNN_SDK_ROOT:-}" ]; then
            MOUNTS="$MOUNTS -v $RKNN_SDK_ROOT:/opt/rknn"
        fi

        docker run --rm $MOUNTS "$IMAGE" \
            cargo build $CARGO_FLAGS $FEATURES
        ;;
esac

# ── 显示产物 ──
echo ""
echo "=== Build complete ==="
TARGET_DIR="$PROJECT_DIR/target"
if [ "$MODE" = "cross" ]; then
    TARGET_DIR="$TARGET_DIR/aarch64-unknown-linux-gnu"
fi

BINARY="$TARGET_DIR/release/mupcd"
if [ -f "$BINARY" ]; then
    echo "Binary: $BINARY"
    file "$BINARY"
    ls -lh "$BINARY"
else
    echo "WARNING: mupcd binary not found. Check build output for errors."
fi
