#!/bin/bash
# MUPC 停止脚本
# 发送 SIGTERM 给 mupcd 进程，超时后 SIGKILL

set -euo pipefail

PID_FILE="${PID_FILE:-/var/run/mupcd.pid}"
TIMEOUT="${TIMEOUT:-30}"

# 查找 mupcd 进程
pid=$(pgrep -f "mupcd" 2>/dev/null || true)

if [ -z "$pid" ]; then
    echo "mupcd is not running"
    exit 0
fi

echo "Stopping mupcd (PID: $pid)..."

# 发送 SIGTERM
kill -TERM "$pid" 2>/dev/null || true

# 等待进程退出
count=0
while kill -0 "$pid" 2>/dev/null && [ $count -lt "$TIMEOUT" ]; do
    sleep 1
    count=$((count + 1))
    echo "  Waiting... ${count}s/${TIMEOUT}s"
done

# 超时则 SIGKILL
if kill -0 "$pid" 2>/dev/null; then
    echo "Timeout! Sending SIGKILL to PID $pid"
    kill -KILL "$pid" 2>/dev/null || true
    sleep 1
fi

if kill -0 "$pid" 2>/dev/null; then
    echo "ERROR: Failed to stop mupcd (PID: $pid)"
    exit 1
fi

echo "mupcd stopped successfully"
