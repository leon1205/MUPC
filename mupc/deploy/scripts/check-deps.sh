#!/bin/bash
# MUPC 运行时依赖检查脚本
# 用法: ./check-deps.sh
# 在目标设备 (Ubuntu 22.04 ARM64) 上运行以验证所有系统依赖

set -euo pipefail

echo "=== MUPC 运行时依赖检查 ==="
FAILURES=0

# glibc >= 2.35
echo -n "Checking glibc... "
if ldd_version=$(ldd --version 2>&1 | head -1 | grep -oP '\d+\.\d+' || true); then
    if [ "$(echo "$ldd_version >= 2.35" | bc 2>/dev/null || echo 0)" -eq 1 ]; then
        echo "OK: glibc $ldd_version"
    else
        echo "FAIL: glibc >= 2.35 required, got $ldd_version"
        FAILURES=$((FAILURES + 1))
    fi
else
    echo "WARN: cannot determine glibc version"
fi

# libssl >= 3.0
echo -n "Checking libssl... "
if dpkg -l libssl3 > /dev/null 2>&1; then
    echo "OK: libssl3"
else
    echo "FAIL: libssl3 not installed"
    FAILURES=$((FAILURES + 1))
fi

# libsqlite3 >= 3.37
echo -n "Checking libsqlite3... "
if dpkg -l libsqlite3-0 > /dev/null 2>&1; then
    echo "OK: libsqlite3-0"
else
    echo "FAIL: libsqlite3-0 not installed"
    FAILURES=$((FAILURES + 1))
fi

# libgcc_s
echo -n "Checking libgcc_s... "
if find /usr/lib -name "libgcc_s.so.1" 2>/dev/null | head -1 | grep -q .; then
    echo "OK: libgcc_s.so.1"
else
    echo "WARN: libgcc_s.so.1 not found (required by librknnrt.so)"
fi

# libstdc++ GLIBCXX version
echo -n "Checking libstdc++... "
libstdcpp=$(find /usr/lib/aarch64-linux-gnu -name "libstdc++.so.6" 2>/dev/null | head -1 || true)
if [ -n "$libstdcpp" ]; then
    if strings "$libstdcpp" 2>/dev/null | grep -q "GLIBCXX_3.4.29"; then
        echo "OK: libstdc++ with GLIBCXX_3.4.29"
    else
        echo "WARN: libstdc++ found but GLIBCXX_3.4.29 not detected"
    fi
else
    echo "WARN: libstdc++.so.6 not found (required by librknnrt.so)"
fi

# systemd
echo -n "Checking systemd... "
if systemctl --version > /dev/null 2>&1; then
    echo "OK: systemd"
else
    echo "WARN: systemd not available (can still run manually)"
fi

# user mupc
echo -n "Checking user mupc... "
if id -u mupc > /dev/null 2>&1; then
    echo "OK: user mupc"
else
    echo "FAIL: user 'mupc' not found. Run: useradd -r -s /bin/false mupc"
    FAILURES=$((FAILURES + 1))
fi

# dialout group
echo -n "Checking dialout group... "
if groups mupc 2>/dev/null | grep -q dialout; then
    echo "OK: dialout group"
else
    echo "WARN: user 'mupc' not in dialout group. Run: usermod -aG dialout mupc"
fi

# file handles limit
echo -n "Checking file handles... "
nofile=$(ulimit -n 2>/dev/null || echo 0)
if [ "$nofile" -ge 4096 ]; then
    echo "OK: $nofile"
else
    echo "WARN: file handles limit is $nofile (recommended >= 4096)"
fi

echo ""
if [ "$FAILURES" -gt 0 ]; then
    echo "=== $FAILURES dependency check(s) FAILED ==="
    exit 1
else
    echo "=== All dependency checks passed ==="
fi
