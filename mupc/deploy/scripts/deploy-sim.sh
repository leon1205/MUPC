#!/bin/bash
# deploy-sim.sh — MUPC 仿真测试环境一键部署
#
# 用法:
#   ./deploy-sim.sh <target_ip>                        # 交互输入密码
#   ./deploy-sim.sh <target_ip> --user pi              # 指定 SSH 用户
#   ./deploy-sim.sh <target_ip> --user pi --password pi # 非交互
#   ./deploy-sim.sh <target_ip> --build --start         # 编译 + 启动
#   ./deploy-sim.sh <target_ip> --generate-data         # 生成测试数据
#
# 功能:
#   1. PC 端: 编译 sim-bridge + 配置 Python venv + 复制 mupc_env
#   2. 嵌入式端: 更新 MUPC 配置（仿真模式）+ 重启 mupcd
#   3. 数据生成: 调用 data_loader.py 生成中国合成数据
#   4. 全栈启动: 先启动嵌入式 mupcd → 再启动 PC sim-bridge

set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
log()    { echo -e "${GREEN}[DEPLOY]${NC} $1"; }
warn()   { echo -e "${YELLOW}[WARN]${NC} $1"; }
info()   { echo -e "${BLUE}[INFO]${NC} $1"; }
err()    { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# ── 默认配置 ──
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$(dirname "$SCRIPT_DIR")")"  # mupc/
WORKSPACE_DIR="$(dirname "$PROJECT_DIR")"             # MUPC repo root

TARGET_IP=""
TARGET_USER="pi"
TARGET_PASS=""
DO_BUILD=false
DO_START=false
DO_GENERATE_DATA=false
SIM_SCENARIO="MODE-01"
SIM_STEPS=96
MUPC_DIR="/opt/mupc"
SIM_BROKER_PORT=1884

# ── 参数解析 ──
while [[ $# -gt 0 ]]; do
    case "$1" in
        --user)          TARGET_USER="$2"; shift 2 ;;
        --password)      TARGET_PASS="$2"; shift 2 ;;
        --build)         DO_BUILD=true; shift ;;
        --start)         DO_START=true; shift ;;
        --generate-data) DO_GENERATE_DATA=true; shift ;;
        --scenario)      SIM_SCENARIO="$2"; shift 2 ;;
        --steps)         SIM_STEPS="$2"; shift 2 ;;
        --help|-h)
            echo "Usage: $0 <target_ip> [--user U] [--password P] [--build] [--start] [--generate-data]"
            echo "  target_ip      MUPC 嵌入式设备 IP (如 192.168.3.118)"
            echo "  --build        部署前编译 sim-bridge"
            echo "  --start        部署后启动仿真"
            echo "  --generate-data 生成测试数据"
            echo "  --scenario     仿真场景 (默认 MODE-01)"
            echo "  --steps        仿真步数 (默认 96)"
            echo ""
            echo "示例:"
            echo "  $0 192.168.3.118 --build --generate-data --start"
            echo "  $0 192.168.3.118 --user pi --password pi --start"
            exit 0
            ;;
        *) TARGET_IP="$1"; shift ;;
    esac
done

[ -z "$TARGET_IP" ] && err "请指定目标 IP。用法: $0 <target_ip>"

# ── 密码处理 ──
if [ -z "$TARGET_PASS" ]; then
    read -rsp "输入 $TARGET_USER@$TARGET_IP 的密码: " TARGET_PASS
    echo ""
fi
SSH_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=5"

ssh_run()  { SSHPASS="$TARGET_PASS" sshpass -e ssh $SSH_OPTS "${TARGET_USER}@${TARGET_IP}" "$@"; }
ssh_sudo() { SSHPASS="$TARGET_PASS" sshpass -e ssh $SSH_OPTS "${TARGET_USER}@${TARGET_IP}" "sudo $*"; }
scp_file() { SSHPASS="$TARGET_PASS" sshpass -e scp $SSH_OPTS "$1" "${TARGET_USER}@${TARGET_IP}:$2"; }

# ═══════════════════════════════════════════════════════════════
# Phase 1: 环境检查
# ═══════════════════════════════════════════════════════════════

log "Phase 1/5: 环境检查"

# PC 检查
for cmd in cargo python3 sshpass; do
    if ! command -v $cmd &>/dev/null; then
        err "PC 缺少: $cmd。请安装后再运行。"
    fi
done
info "PC 环境就绪 (cargo, python3, sshpass)"

# 目标板检查
if ! SSHPASS="$TARGET_PASS" sshpass -e ssh $SSH_OPTS "${TARGET_USER}@${TARGET_IP}" "echo ok" 2>/dev/null; then
    err "无法连接到 $TARGET_USER@$TARGET_IP"
fi
TARGET_ARCH=$(ssh_run "uname -m" 2>/dev/null)
info "目标板: $TARGET_ARCH, 用户: $TARGET_USER"

# ═══════════════════════════════════════════════════════════════
# Phase 2: PC 端 — 编译 sim-bridge + Python venv
# ═══════════════════════════════════════════════════════════════

log "Phase 2/5: PC 端编译"

if $DO_BUILD || [ ! -f "$PROJECT_DIR/target/release/mupc-sim-bridge" ]; then
    info "编译 sim-bridge..."
    cargo build -p mupc-sim-bridge --release --manifest-path "$PROJECT_DIR/Cargo.toml" 2>&1 | tail -3
    info "sim-bridge 编译完成: $PROJECT_DIR/target/release/mupc-sim-bridge"
else
    info "sim-bridge 已存在，跳过编译"
fi

# Python venv
VENV_DIR="$WORKSPACE_DIR/sim-env/venv"
if [ ! -f "$VENV_DIR/bin/python3" ]; then
    info "创建 Python venv..."
    python3 -m venv "$VENV_DIR"
    info "安装依赖..."
    "$VENV_DIR/bin/pip" install numpy grid2op pandapower lightsim2grid 2>&1 | tail -3
    info "Python venv 就绪: $VENV_DIR"
else
    info "Python venv 已存在"
fi

# 检查 mupc_env
MUPC_ENV_DIR="$WORKSPACE_DIR/sim-env/mupc_env"
if [ ! -d "$MUPC_ENV_DIR" ]; then
    MUPC_AI_ENV="/work/MUPC-AI/mupc_env"
    if [ -d "$MUPC_AI_ENV" ]; then
        info "从 MUPC-AI2 复制 mupc_env..."
        cp -r "$MUPC_AI_ENV" "$MUPC_ENV_DIR"
        # 复制 grid2op 子模块
        if [ -d "/work/MUPC-AI/mupc_env/grid2op" ]; then
            cp -r "/work/MUPC-AI/mupc_env/grid2op" "$MUPC_ENV_DIR/"
        fi
        info "mupc_env 已复制到 sim-env/"
    else
        warn "MUPC-AI2 mupc_env 未找到，请手动复制到 $MUPC_ENV_DIR"
        warn "  cp -r /work/MUPC-AI/mupc_env $MUPC_ENV_DIR"
    fi
else
    info "mupc_env 已存在"
fi

# ═══════════════════════════════════════════════════════════════
# Phase 3: 数据生成（可选）
# ═══════════════════════════════════════════════════════════════

log "Phase 3/5: 数据准备"

SIM_DATA_DIR="$WORKSPACE_DIR/sim-env/data"
mkdir -p "$SIM_DATA_DIR"

if $DO_GENERATE_DATA; then
    DATA_LOADER="/work/MUPC-AI/data_loader.py"
    if [ -f "$DATA_LOADER" ]; then
        info "生成中国合成数据 (上海, 31.23N 121.47E)..."
        "$VENV_DIR/bin/python3" "$DATA_LOADER" \
            --generate --lat 31.23 --lon 121.47 --year 2023 \
            --output "$SIM_DATA_DIR/china_synth.csv" 2>&1 | tail -3
        info "测试数据已生成: $SIM_DATA_DIR/china_synth.csv"
    else
        warn "data_loader.py 未找到 ($DATA_LOADER)。跳过数据生成。"
        warn "engine.py 将使用内建模拟数据运行。"
    fi
else
    info "跳过数据生成（使用 --generate-data 启用）"
fi

# ═══════════════════════════════════════════════════════════════
# Phase 4: 嵌入式端 — 配置 MUPC 仿真模式
# ═══════════════════════════════════════════════════════════════

log "Phase 4/5: 嵌入式 MUPC 配置"

# 备份当前配置
BACKUP_TIME=$(date +%Y%m%d_%H%M%S)
ssh_sudo "cp $MUPC_DIR/config/mupc_core_config.yaml $MUPC_DIR/config/mupc_core_config.yaml.bak.$BACKUP_TIME 2>/dev/null || true"
info "已备份 MUPC 配置"

# 更新 intercore 目标 IP 为仿真 PC
SIM_PC_IP=$(hostname -I | awk '{print $1}')
info "仿真 PC IP: $SIM_PC_IP"

ssh_sudo "sed -i 's/host:.*/host: \"$SIM_PC_IP\"/' $MUPC_DIR/config/mupc_core_config.yaml"
info "intercore.host → $SIM_PC_IP (仿真 PC)"

# 停止 mupcd（如果运行中）
ssh_sudo "systemctl stop mupcd 2>/dev/null || pkill mupcd 2>/dev/null" || true
info "MUPC 已停止"

# 重启
ssh_sudo "systemctl start mupcd 2>/dev/null || nohup sudo -u mupc $MUPC_DIR/bin/mupcd &>/dev/null &"
sleep 2
info "MUPC 已启动"

# 检查
if ssh_run "pgrep mupcd" 2>/dev/null; then
    info "mupcd 进程运行中"
else
    warn "mupcd 进程未检测到，请手动检查: sudo -u mupc /opt/mupc/bin/mupcd"
fi

# ═══════════════════════════════════════════════════════════════
# Phase 5: 启动仿真
# ═══════════════════════════════════════════════════════════════

log "Phase 5/5: 启动仿真"

SIM_BRIDGE="$PROJECT_DIR/target/release/mupc-sim-bridge"
SIM_CONFIG="$PROJECT_DIR/config/sim_config.yaml"

# 更新配置文件中的 broker IP
if [ -f "$SIM_CONFIG" ]; then
    cp "$SIM_CONFIG" "$SIM_CONFIG.bak"  # O4: 备份后修改
    sed -i "s/^mqtt_broker:.*/mqtt_broker: \"$TARGET_IP:$SIM_BROKER_PORT\"/" "$SIM_CONFIG"
    sed -i "s/^scenario:.*/scenario: \"$SIM_SCENARIO\"/" "$SIM_CONFIG"
    info "sim_config.yaml: broker=$TARGET_IP:$SIM_BROKER_PORT, scenario=$SIM_SCENARIO"
fi

if $DO_START; then
    info "启动 sim-bridge (场景: $SIM_SCENARIO)..."
    info "=============================================="
    info "  仿真 PC:  $SIM_PC_IP"
    info "  嵌入式:   $TARGET_IP (MUPC)"
    info "  场景:     $SIM_SCENARIO"
    info "  步数:     $SIM_STEPS"
    info "  Broker:   $TARGET_IP:$SIM_BROKER_PORT"
    info "  TCP:      0.0.0.0:9100"
    info "=============================================="

    # 确保 engine.py 路径正确
    cd "$WORKSPACE_DIR"
    exec "$SIM_BRIDGE" \
        --config "$SIM_CONFIG" \
        --scenario "$SIM_SCENARIO"
else
    info "仿真已就绪，手动启动:"
    echo ""
    echo "  cd $WORKSPACE_DIR"
    echo "  $SIM_BRIDGE --config $SIM_CONFIG --scenario $SIM_SCENARIO"
    echo ""
    echo "=============================================="
    info "部署摘要:"
    info "  PC sim-bridge:  $SIM_BRIDGE"
    info "  PC engine.py:   $WORKSPACE_DIR/sim-env/engine.py"
    info "  PC venv:        $VENV_DIR"
    info "  嵌入式 MUPC:    $TARGET_IP ($MUPC_DIR)"
    info "  Broker:         $TARGET_IP:$SIM_BROKER_PORT"
    info "=============================================="
fi
