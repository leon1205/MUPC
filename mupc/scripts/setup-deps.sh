#!/bin/bash
# setup-deps.sh — MUPC 外部依赖一键安装脚本
#
# 用法:
#   ./scripts/setup-deps.sh           # 交互式检查并安装
#   ./scripts/setup-deps.sh --all     # 自动安装全部依赖
#   ./scripts/setup-deps.sh --check   # 仅检查，不安装
#
# 目标: git clone 后一行命令满足所有外部依赖

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
WORKSPACE_DIR="$(dirname "$PROJECT_DIR")"  # /work/MUPC

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
check_mark="${GREEN}✓${NC}"; cross_mark="${RED}✗${NC}"; warn_mark="${YELLOW}⚠${NC}"

MODE="${1:-}"

# ═══════════════════════════════════════════════════════════════
# 1. 系统包检查
# ═══════════════════════════════════════════════════════════════

echo "=== MUPC 外部依赖检查 ==="
echo ""

MISSING_PKGS=""

check_pkg() {
    if dpkg -l "$1" 2>/dev/null | grep -q "^ii"; then
        echo "  ${check_mark} $1"
    else
        echo "  ${cross_mark} $1"
        MISSING_PKGS="$MISSING_PKGS $1"
    fi
}

echo "系统包:"
check_pkg libssl-dev
check_pkg libsqlite3-dev
check_pkg pkg-config

# 交叉编译工具链 (可选)
echo ""
echo "交叉编译工具链 (ARM64 编译需要):"
check_pkg gcc-aarch64-linux-gnu
check_pkg g++-aarch64-linux-gnu

if [ -n "$MISSING_PKGS" ]; then
    echo ""
    echo -e "${YELLOW}建议安装: sudo apt install$MISSING_PKGS${NC}"
    if [ "$MODE" = "--all" ]; then
        echo "正在安装..."
        sudo apt install -y $MISSING_PKGS
    fi
fi

# ═══════════════════════════════════════════════════════════════
# 2. Rust 目标检查
# ═══════════════════════════════════════════════════════════════

echo ""
echo "Rust target:"
if rustup target list --installed 2>/dev/null | grep -q "aarch64-unknown-linux-gnu"; then
    echo "  ${check_mark} aarch64-unknown-linux-gnu"
else
    echo "  ${cross_mark} aarch64-unknown-linux-gnu (ARM64 交叉编译需要)"
    if [ "$MODE" = "--all" ]; then
        echo "正在安装..."
        rustup target add aarch64-unknown-linux-gnu
    else
        echo "        安装: rustup target add aarch64-unknown-linux-gnu"
    fi
fi

# ═══════════════════════════════════════════════════════════════
# 3. RKNN SDK 检查
# ═══════════════════════════════════════════════════════════════

echo ""
echo "RKNN SDK (ARM64 + npu feature 需要):"

RKNN_PATHS=(
    "$WORKSPACE_DIR/rknn-toolkit2-2.3.2"
    "/opt/rknn"
    "$HOME/rknn-toolkit2-2.3.2"
)

RKNN_FOUND=""
for p in "${RKNN_PATHS[@]}"; do
    if [ -f "$p/rknpu2/runtime/Linux/librknn_api/aarch64/librknnrt.so" ]; then
        RKNN_FOUND="$p"
        break
    fi
done

if [ -n "$RKNN_FOUND" ]; then
    echo "  ${check_mark} 已找到: $RKNN_FOUND"
else
    echo "  ${cross_mark} 未找到 RKNN Toolkit 2 (v2.3.2)"
    echo ""
    echo "  RKNN SDK 需从 Rockchip 官方获取，无法自动下载。请执行以下步骤："
    echo ""
    echo "  1. 下载 RKNN SDK:"
    echo "     https://console.zbox.filez.com/l/I00fc3 (提取码: rknn)"
    echo "     或联系 Rockchip FAE 获取"
    echo ""
    echo "  2. 解压到项目父目录:"
    echo "     unzip rknn-toolkit2-2.3.2.zip -d $WORKSPACE_DIR/"
    echo ""
    echo "  3. 重新运行本脚本验证"
    echo ""
    echo "  ${warn_mark} 没有 RKNN SDK 仍可进行 x86_64 开发和编译（npu feature 自动使用 stub）"
    echo "     ARM64 交叉编译在无 RKNN SDK 时也可进行（npu feature 使用 stub）"
fi

# ═══════════════════════════════════════════════════════════════
# 4. OpenSSL 交叉编译
# ═══════════════════════════════════════════════════════════════

echo ""
echo "OpenSSL ARM64 (ARM64 交叉编译需要):"

OPENSSL_DIR="$WORKSPACE_DIR/external/openssl-4.0.1"
OPENSSL_INSTALL="$OPENSSL_DIR/aarch64-install"

if [ -f "$OPENSSL_INSTALL/lib/libssl.a" ] && [ -f "$OPENSSL_INSTALL/lib/libcrypto.a" ]; then
    echo "  ${check_mark} 已编译: $OPENSSL_INSTALL"
else
    echo "  ${cross_mark} 未编译 OpenSSL ARM64"

    # 检查源码是否存在
    if [ ! -f "$OPENSSL_DIR/Configure" ]; then
        echo ""
        echo "  OpenSSL 源码不存在，正在下载..."
        OPENSSL_VERSION="4.0.1"
        OPENSSL_URL="https://github.com/openssl/openssl/releases/download/openssl-${OPENSSL_VERSION}/openssl-${OPENSSL_VERSION}.tar.gz"

        if command -v wget &>/dev/null; then
            mkdir -p "$(dirname "$OPENSSL_DIR")"
            wget -q --show-progress -O /tmp/openssl-${OPENSSL_VERSION}.tar.gz "$OPENSSL_URL" || {
                echo "  ${cross_mark} 下载失败，请手动下载并解压到 $OPENSSL_DIR"
                echo "  URL: $OPENSSL_URL"
            }
            if [ -f /tmp/openssl-${OPENSSL_VERSION}.tar.gz ]; then
                echo "  正在解压..."
                tar -xf /tmp/openssl-${OPENSSL_VERSION}.tar.gz -C "$(dirname "$OPENSSL_DIR")"
                rm /tmp/openssl-${OPENSSL_VERSION}.tar.gz
                echo "  ${check_mark} OpenSSL 源码已解压到 $OPENSSL_DIR"
            fi
        else
            echo "  请安装 wget 或手动下载:"
            echo "  URL: $OPENSSL_URL"
            echo "  解压到: $OPENSSL_DIR"
        fi
    fi

    # 交叉编译
    if [ -f "$OPENSSL_DIR/Configure" ] && [ "$MODE" = "--all" ]; then
        if ! command -v aarch64-linux-gnu-gcc &>/dev/null; then
            echo "  ${cross_mark} 需要交叉编译器: sudo apt install gcc-aarch64-linux-gnu"
        else
            echo "  正在交叉编译 OpenSSL for aarch64..."
            cd "$OPENSSL_DIR"
            ./Configure linux-aarch64 \
                --cross-compile-prefix=aarch64-linux-gnu- \
                --prefix="$OPENSSL_INSTALL" \
                no-shared 2>&1 | tail -1
            make -j$(nproc) 2>&1 | tail -1
            make install_sw 2>&1 | tail -1
            echo "  ${check_mark} OpenSSL ARM64 编译完成: $OPENSSL_INSTALL"
        fi
    elif [ -f "$OPENSSL_DIR/Configure" ]; then
        echo "  运行 '$0 --all' 自动编译，或手动执行:"
        echo "    cd $OPENSSL_DIR"
        echo "    ./Configure linux-aarch64 --cross-compile-prefix=aarch64-linux-gnu- \\"
        echo "        --prefix=$OPENSSL_INSTALL no-shared"
        echo "    make -j\$(nproc) && make install_sw"
    fi
fi

# ═══════════════════════════════════════════════════════════════
# 5. liblzma 交叉编译
# ═══════════════════════════════════════════════════════════════

echo ""
echo "liblzma ARM64 (OTA 压缩依赖):"

LIBLZMA_DIR="$WORKSPACE_DIR/external/liblzma-master"
LIBLZMA_INSTALL="$LIBLZMA_DIR/aarch64-install"

if [ -f "$LIBLZMA_INSTALL/lib/liblzma.so" ] || [ -f "$LIBLZMA_INSTALL/lib/liblzma.a" ]; then
    echo "  ${check_mark} 已编译: $LIBLZMA_INSTALL"
else
    echo "  ${cross_mark} 未编译 liblzma ARM64"

    if [ -f "$LIBLZMA_DIR/configure" ]; then
        if [ "$MODE" = "--all" ]; then
            if ! command -v aarch64-linux-gnu-gcc &>/dev/null; then
                echo "  ${cross_mark} 需要交叉编译器"
            else
                echo "  正在交叉编译 liblzma for aarch64..."
                cd "$LIBLZMA_DIR"
                ./configure --host=aarch64-linux-gnu \
                    --prefix="$LIBLZMA_INSTALL" \
                    --disable-xz --disable-xzdec --disable-lzmadec \
                    --disable-lzmainfo --disable-scripts 2>&1 | tail -1
                make -j$(nproc) 2>&1 | tail -1
                make install 2>&1 | tail -1
                echo "  ${check_mark} liblzma ARM64 编译完成: $LIBLZMA_INSTALL"
            fi
        else
            echo "  运行 '$0 --all' 自动编译，或手动执行:"
            echo "    cd $LIBLZMA_DIR"
            echo "    ./configure --host=aarch64-linux-gnu --prefix=$LIBLZMA_INSTALL"
            echo "    make -j\$(nproc) && make install"
        fi
    else
        echo "  ${cross_mark} liblzma 源码不存在: $LIBLZMA_DIR"
    fi
fi

# ═══════════════════════════════════════════════════════════════
# 6. 总结
# ═══════════════════════════════════════════════════════════════

echo ""
echo "=== 检查完成 ==="
echo ""

# 开发能力矩阵
DEV_READY=1
echo "开发能力:"
echo -n "  x86_64 cargo check  "
if command -v cargo &>/dev/null; then echo -e "${check_mark} 可用"; else echo -e "${cross_mark} 不可用"; DEV_READY=0; fi
echo -n "  x86_64 cargo build  "
if dpkg -l libssl-dev 2>/dev/null | grep -q "^ii"; then echo -e "${check_mark} 可用"; else echo -e "${cross_mark} 需要 libssl-dev"; DEV_READY=0; fi
echo -n "  ARM64 交叉编译     "
if command -v aarch64-linux-gnu-gcc &>/dev/null && [ -f "$OPENSSL_INSTALL/lib/libssl.a" ]; then
    echo -e "${check_mark} 可用"
else
    echo -e "${cross_mark} 需要交叉编译工具链 + OpenSSL ARM64"
    DEV_READY=0
fi
echo -n "  ARM64 + NPU        "
if [ -n "$RKNN_FOUND" ] && [ -f "$OPENSSL_INSTALL/lib/libssl.a" ]; then
    echo -e "${check_mark} 可用"
else
    echo -e "${warn_mark} 需要 RKNN SDK + OpenSSL ARM64 (npu 可用 stub 替代)"
fi

echo ""
if [ "$DEV_READY" -eq 1 ]; then
    echo -e "${GREEN}基本开发环境就绪，可执行: cd mupc && cargo build -p mupc-core-bin --release${NC}"
else
    echo -e "${YELLOW}部分依赖缺失，运行 '$0 --all' 自动安装${NC}"
fi
