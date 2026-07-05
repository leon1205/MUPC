#!/bin/bash
# deploy.sh — MUPC 一键部署到 RK3588 目标板
#
# 用法:
#   ./deploy.sh <target_ip>                    # 部署到指定 IP (交互输入密码)
#   ./deploy.sh <target_ip> --user <user>       # 指定 SSH 用户 (默认 pi)
#   ./deploy.sh <target_ip> --password <pwd>    # 指定密码 (跳过交互)
#   ./deploy.sh <target_ip> --build             # 部署前先编译
#   ./deploy.sh <target_ip> --restart           # 部署后重启 mupcd
#   ./deploy.sh <target_ip> --full              # 完整: 编译 + 依赖检查 + 部署 + 重启
#
# 示例:
#   ./deploy.sh 192.168.3.118 --full
#   ./deploy.sh 192.168.3.118 --user root --password mypwd --restart

set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
log()  { echo -e "${GREEN}[DEPLOY]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
err()  { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# ── 默认配置 ──
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"  # mupc/
TARGET_IP=""
TARGET_USER="pi"
TARGET_PASS=""
DO_BUILD=false
DO_RESTART=false
DO_FULL=false
CROSS_TARGET="aarch64-unknown-linux-gnu"

# 目标板路径
TARGET_BIN_DIR="/opt/mupc/bin"
TARGET_LIB_DIR="/opt/mupc/lib"
TARGET_CONFIG_DIR="/opt/mupc/config"
TARGET_LOG_DIR="/opt/mupc/logs"
TARGET_DATA_DIR="/opt/mupc/data"
TARGET_CERT_DIR="/opt/mupc/certs"
TARGET_MODEL_DIR="/opt/mupc/models"
TARGET_MUPC_USER="mupc"

# ── 参数解析 ──
while [[ $# -gt 0 ]]; do
    case "$1" in
        --user)     TARGET_USER="$2"; shift 2 ;;
        --password) TARGET_PASS="$2"; shift 2 ;;
        --build)    DO_BUILD=true; shift ;;
        --restart)  DO_RESTART=true; shift ;;
        --full)     DO_BUILD=true; DO_RESTART=true; shift ;;
        --help|-h)
            echo "Usage: $0 <target_ip> [--user U] [--password P] [--build] [--restart] [--full]"
            exit 0 ;;
        *) TARGET_IP="$1"; shift ;;
    esac
done

[ -z "$TARGET_IP" ] && err "请指定目标板 IP。用法: $0 <target_ip>"

# ── 密码处理 ──
# 使用 SSHPASS 环境变量 + sshpass -e 避免命令行暴露密码
# 生产环境建议 SSH 密钥认证 + sudoers NOPASSWD
if [ -z "$TARGET_PASS" ]; then
    read -rsp "输入 $TARGET_USER@$TARGET_IP 的密码: " TARGET_PASS
    echo ""
fi
export SSHPASS="$TARGET_PASS"

SSH_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=5"

# ── SSH 执行函数 ──
ssh_run() {
    sshpass -e ssh $SSH_OPTS "${TARGET_USER}@${TARGET_IP}" "$@"
}

ssh_sudo() {
    # 要求目标板已配置 sudo NOPASSWD（部署 docs 中有说明）
    # 若未配置，使用 ssh -t 分配 PTY 交互式输入 sudo 密码
    sshpass -e ssh $SSH_OPTS "${TARGET_USER}@${TARGET_IP}" \
        "sudo -n $*" 2>/dev/null || \
    sshpass -e ssh -t $SSH_OPTS "${TARGET_USER}@${TARGET_IP}" \
        "sudo $*" 2>/dev/null
}

# ── SCP 函数 ──
scp_file() {
    sshpass -e scp $SSH_OPTS "$1" "${TARGET_USER}@${TARGET_IP}:$2"
}

# ═══════════════════════════════════════════════════════════════
# 1. 连接检查
# ═══════════════════════════════════════════════════════════════

log "检查目标板连接 $TARGET_USER@$TARGET_IP..."
if ! sshpass -e ssh $SSH_OPTS "${TARGET_USER}@${TARGET_IP}" "echo ok" 2>/dev/null; then
    err "无法连接到目标板，请检查 IP、用户名和密码"
fi
log "连接成功"

# 检查目标架构
TARGET_ARCH=$(sshpass -e ssh $SSH_OPTS "${TARGET_USER}@${TARGET_IP}" "uname -m" 2>/dev/null)
log "目标架构: $TARGET_ARCH"

# ═══════════════════════════════════════════════════════════════
# 2. 编译 (可选)
# ═══════════════════════════════════════════════════════════════

if $DO_BUILD; then
    log "开始交叉编译..."

    # 检查交叉编译工具链
    if ! command -v aarch64-linux-gnu-gcc &>/dev/null; then
        err "需要交叉编译器。安装: sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu"
    fi

    # 检查 OpenSSL
    OPENSSL_DIR="$PROJECT_DIR/../external/openssl-4.0.1/aarch64-install"
    if [ ! -f "$OPENSSL_DIR/lib/libssl.a" ]; then
        warn "OpenSSL ARM64 未编译，尝试自动编译..."
        OPENSSL_SRC="$PROJECT_DIR/../external/openssl-4.0.1"
        if [ -f "$OPENSSL_SRC/Configure" ]; then
            cd "$OPENSSL_SRC"
            ./Configure linux-aarch64 --cross-compile-prefix=aarch64-linux-gnu- \
                --prefix="$OPENSSL_DIR" no-shared 2>&1 | tail -1
            make -j$(nproc) 2>&1 | tail -1
            make install_sw 2>&1 | tail -1
            cd "$PROJECT_DIR"
        else
            err "OpenSSL 源码未找到: $OPENSSL_SRC。运行 ../scripts/setup-deps.sh --all"
        fi
    fi

    log "编译 mupc-core-bin..."
    export OPENSSL_DIR="$OPENSSL_DIR"
    export PKG_CONFIG_ALLOW_CROSS=1
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

    cargo build --workspace --release --target "$CROSS_TARGET" \
        --exclude mupc-iec61850-plugin --exclude device-trait 2>&1 | tail -3

    BUILD_DIR="$PROJECT_DIR/target/$CROSS_TARGET/release"
    log "编译完成: $BUILD_DIR/mupcd"
else
    BUILD_DIR="$PROJECT_DIR/target/$CROSS_TARGET/release"
    if [ ! -f "$BUILD_DIR/mupcd" ]; then
        err "mupcd 未找到 ($BUILD_DIR/mupcd)。请先编译或使用 --build 参数"
    fi
fi

# ═══════════════════════════════════════════════════════════════
# 3. 目标板环境准备
# ═══════════════════════════════════════════════════════════════

log "准备目标板环境..."

# 停止旧进程
ssh_sudo "pkill mupcd 2>/dev/null" || true
sleep 1

# 创建目录
DIRS=("$TARGET_BIN_DIR" "$TARGET_LIB_DIR" "$TARGET_CONFIG_DIR"
      "$TARGET_LOG_DIR" "$TARGET_DATA_DIR" "$TARGET_CERT_DIR" "$TARGET_MODEL_DIR")
for d in "${DIRS[@]}"; do
    ssh_sudo "mkdir -p $d"
done

# 创建 mupc 用户 (如果不存在)
if ! ssh_run "id mupc 2>/dev/null"; then
    log "创建 mupc 系统用户..."
    ssh_sudo "useradd -r -s /bin/false -d /opt/mupc -M mupc"
fi

# ═══════════════════════════════════════════════════════════════
# 4. 部署文件
# ═══════════════════════════════════════════════════════════════

log "部署可执行文件..."
scp_file "$BUILD_DIR/mupcd" "/tmp/mupcd"
ssh_sudo "mv /tmp/mupcd $TARGET_BIN_DIR/mupcd && chmod +x $TARGET_BIN_DIR/mupcd"

log "部署插件..."
for so in "$BUILD_DIR"/*.so; do
    if [ -f "$so" ]; then
        so_name=$(basename "$so")
        scp_file "$so" "/tmp/$so_name"
        ssh_sudo "mv /tmp/$so_name $TARGET_LIB_DIR/$so_name"
        log "  $so_name"
    fi
done

log "部署配置文件..."
for conf in "$PROJECT_DIR/config"/*.yaml; do
    if [ -f "$conf" ]; then
        conf_name=$(basename "$conf")
        scp_file "$conf" "/tmp/$conf_name"
        ssh_sudo "mv /tmp/$conf_name $TARGET_CONFIG_DIR/$conf_name"
        log "  $conf_name"
    fi
done

# 部署 RKNN 运行时库 (如果存在)
log "检查 RKNN 运行时库..."
RKNN_SO="$PROJECT_DIR/vendor/rknn/librknnrt.so"
if [ -f "$RKNN_SO" ]; then
    scp_file "$RKNN_SO" "/tmp/librknnrt.so"
    ssh_sudo "mv /tmp/librknnrt.so $TARGET_LIB_DIR/librknnrt.so"
    log "  librknnrt.so 已部署"
else
    warn "librknnrt.so 未找到 ($RKNN_SO)，跳过 (npu feature 将使用 stub)"
    warn "如需 NPU 推理，请将 librknnrt.so 复制到 $TARGET_LIB_DIR/"
fi

# 部署 AI 模型 (如果存在)
MODEL_DIR="$PROJECT_DIR/../etc/mupc/models"
if [ -d "$MODEL_DIR" ] && [ "$(ls -A "$MODEL_DIR" 2>/dev/null)" ]; then
    log "部署 AI 模型..."
    for model in "$MODEL_DIR"/*.rknn; do
        if [ -f "$model" ]; then
            model_name=$(basename "$model")
            scp_file "$model" "/tmp/$model_name"
            ssh_sudo "mv /tmp/$model_name $TARGET_MODEL_DIR/$model_name"
            log "  $model_name"
        fi
    done
fi

# ═══════════════════════════════════════════════════════════════
# 5. 权限设置
# ═══════════════════════════════════════════════════════════════

log "设置权限..."
ssh_sudo "chown -R mupc:mupc /opt/mupc"

# ═══════════════════════════════════════════════════════════════
# 6. systemd 服务
# ═══════════════════════════════════════════════════════════════

SERVICE_FILE="$PROJECT_DIR/deploy/systemd/mupcd.service"
if [ -f "$SERVICE_FILE" ]; then
    log "安装 systemd 服务..."
    scp_file "$SERVICE_FILE" "/tmp/mupcd.service"
    ssh_sudo "mv /tmp/mupcd.service /etc/systemd/system/mupcd.service"
    ssh_sudo "systemctl daemon-reload"
    ssh_sudo "systemctl enable mupcd"
fi

# ═══════════════════════════════════════════════════════════════
# 7. 验证
# ═══════════════════════════════════════════════════════════════

log "验证部署..."
scp_file "$PROJECT_DIR/deploy/scripts/check-deps.sh" "/tmp/check-deps.sh"
ssh_sudo "bash /tmp/check-deps.sh" 2>/dev/null || true

# ═══════════════════════════════════════════════════════════════
# 8. 重启 (可选)
# ═══════════════════════════════════════════════════════════════

if $DO_RESTART; then
    log "重启 mupcd..."
    ssh_sudo "systemctl restart mupcd"
    sleep 2
    log "服务状态:"
    ssh_run "systemctl status mupcd --no-pager -l" 2>/dev/null || true
else
    log "部署完成。手动启动:"
    echo "  sudo -u mupc $TARGET_BIN_DIR/mupcd"
    echo "  或 systemctl start mupcd"
fi

log "部署完成！"
