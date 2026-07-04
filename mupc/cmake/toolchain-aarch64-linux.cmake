# toolchain-aarch64-linux.cmake — MUPC 目标平台交叉编译工具链
#
# 目标: RK3588 ARM64, Ubuntu 20.04+ (glibc 2.31+)
# 用法: cmake -DCMAKE_TOOLCHAIN_FILE=cmake/toolchain-aarch64-linux.cmake ..
#
# 需要安装: gcc-aarch64-linux-gnu g++-aarch64-linux-gnu

set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)

# ── 编译器 ──
set(CMAKE_C_COMPILER    aarch64-linux-gnu-gcc)
set(CMAKE_ASM_COMPILER  aarch64-linux-gnu-gcc)
# CXX 非必需 (Rust 项目不编译 C++ 源码)

# ── 链接器 ──
set(CMAKE_LINKER         aarch64-linux-gnu-ld)
set(CMAKE_AR             aarch64-linux-gnu-ar)
set(CMAKE_OBJCOPY        aarch64-linux-gnu-objcopy)
set(CMAKE_OBJDUMP        aarch64-linux-gnu-objdump)
set(CMAKE_STRIP          aarch64-linux-gnu-strip)
set(CMAKE_RANLIB         aarch64-linux-gnu-ranlib)

# ── 系统根 (可选, 用于 sysroot) ──
# 若安装了 aarch64-linux-gnu 的 sysroot，取消注释:
# set(CMAKE_SYSROOT /usr/aarch64-linux-gnu)

# ── 查找模式 ──
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# ── RPATH 设置 (运行时动态库搜索) ──
# librknnrt.so 放在可执行文件同目录的 lib/ 下
set(CMAKE_INSTALL_RPATH "$ORIGIN/lib")
set(CMAKE_BUILD_RPATH "$ORIGIN/lib")

# ── Cargo 交叉编译环境变量 ──
set(ENV{CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER} "aarch64-linux-gnu-gcc")
set(ENV{CC_aarch64_unknown_linux_gnu} "aarch64-linux-gnu-gcc")
set(ENV{CXX_aarch64_unknown_linux_gnu} "aarch64-linux-gnu-g++")
set(ENV{AR_aarch64_unknown_linux_gnu} "aarch64-linux-gnu-ar")
set(ENV{CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS}
    "-C link-arg=-Wl,--dynamic-linker=/lib/ld-linux-aarch64.so.1")
