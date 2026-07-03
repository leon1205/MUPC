#!/bin/bash
# MUPC 启动脚本
# 用法: ./start.sh [--config /path/to/config.yaml]

set -euo pipefail

BIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/bin"
MUPCD="${MUPCD:-$BIN_DIR/mupcd}"
CONFIG="${CONFIG:-/opt/mupc/config/mupc_core_config.yaml}"
MODEL_DIR="${MODEL_DIR:-/opt/mupc/models}"
LOG_DIR="${LOG_DIR:-/opt/mupc/logs}"

# 检查二进制文件
if [ ! -x "$MUPCD" ]; then
    echo "ERROR: mupcd binary not found or not executable: $MUPCD"
    exit 1
fi

# 检查配置文件
if [ ! -f "$CONFIG" ]; then
    echo "ERROR: Config file not found: $CONFIG"
    exit 1
fi

# 创建必要的目录
mkdir -p "$LOG_DIR" "/opt/mupc/data"

# 设置库路径 (librknnrt.so + 插件 .so)
export LD_LIBRARY_PATH="/opt/mupc/lib:/opt/mupc/lib/plugins:${LD_LIBRARY_PATH:-}"

echo "Starting mupcd..."
echo "  Binary:  $MUPCD"
echo "  Config:  $CONFIG"
echo "  Models:  $MODEL_DIR"
echo "  Logs:    $LOG_DIR"

exec "$MUPCD" \
    --config "$CONFIG" \
    --model-dir "$MODEL_DIR" \
    --log-dir "$LOG_DIR" \
    "$@"
