# MUPC 构建指南

目标平台：RK3588 ARM64, Ubuntu 20.04+

## 前置条件

### 交叉编译工具链

在 x86_64 开发机上编译 ARM64 目标，需要安装 `aarch64-linux-gnu` 工具链。

**方式 A: APT 安装（最简单）**

```bash
sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu
```

**方式 B: Linaro 官方发布（RKNN SDK 推荐，glibc 版本匹配更好）**

```
https://releases.linaro.org/components/toolchain/binaries/latest-7/aarch64-linux-gnu/gcc-linaro-7.5.0-2019.12-x86_64_aarch64-linux-gnu.tar.xz
```

```bash
wget https://releases.linaro.org/components/toolchain/binaries/latest-7/aarch64-linux-gnu/gcc-linaro-7.5.0-2019.12-x86_64_aarch64-linux-gnu.tar.xz
sudo tar -xf gcc-linaro-7.5.0-2019.12-x86_64_aarch64-linux-gnu.tar.xz -C /opt
export PATH=/opt/gcc-linaro-7.5.0-2019.12-x86_64_aarch64-linux-gnu/bin:$PATH
```

**方式 C: ARM 官方 gcc-arm-10.2（RKLLM 推荐）**

```
https://github.com/airockchip/gcc-buildroot-9.3.0-2020.03-x86_64_aarch64-rockchip-linux-gnu
```

**验证安装**

```bash
aarch64-linux-gnu-gcc --version
```

> RKNN SDK 示例 (`rknpu2/examples/*/build-linux.sh`) 使用 `GCC_COMPILER` 环境变量：
> ```bash
> export GCC_COMPILER=/opt/gcc-linaro-7.5.0-2019.12-x86_64_aarch64-linux-gnu/bin/aarch64-linux-gnu
> ```

### 容器化交叉编译（不需要本机工具链）

```bash
cargo install cross
```

### RKNN SDK

SDK 默认路径：`<项目父目录>/rknn-toolkit2-2.3.2/`，或通过环境变量指定。

## 构建方式

### 方式 1: Cargo 直接编译

```bash
# 本机编译 (RK3588 开发板上)
cargo build -p mupc-core-bin --release --features npu

# 交叉编译 (x86_64 → ARM64)
export RKNN_SDK_ROOT=/work/MUPC/rknn-toolkit2-2.3.2
cargo build -p mupc-core-bin --release --features npu --target aarch64-unknown-linux-gnu

# 使用 cross-rs 容器化编译
cross build -p mupc-core-bin --release --features npu --target aarch64-unknown-linux-gnu
```

RKNN SDK 自动检测优先级：
1. `RKNN_VENDOR_DIR` 环境变量 — 直接指定 `librknnrt.so` 所在目录
2. `RKNN_SDK_ROOT` 环境变量 — SDK 根目录
3. 自动检测 `<项目父目录>/rknn-toolkit2-2.3.2`
4. 系统路径 `/opt/rknn`

### 方式 2: CMake 编排

```bash
# 本机构建
cmake -B build -DRKNN_SDK_ROOT=/work/MUPC/rknn-toolkit2-2.3.2
cmake --build build

# 交叉编译 (指定 toolchain)
cmake -B build-arm64 \
    -DCMAKE_TOOLCHAIN_FILE=cmake/toolchain-aarch64-linux.cmake \
    -DRKNN_SDK_ROOT=/work/MUPC/rknn-toolkit2-2.3.2
cmake --build build-arm64

# 禁用 NPU
cmake -B build -DENABLE_NPU=OFF
cmake --build build

# 辅助 targets
cmake --build build --target test      # 运行测试
cmake --build build --target clippy    # 代码检查
cmake --build build --target fmt       # 格式化
cmake --build build --target clean-all # 清理
```

### 方式 3: 一键脚本

```bash
# 本机构建 (RK3588 开发板)
./deploy/scripts/build-for-rk3588.sh

# 交叉编译 (x86_64 → ARM64)
./deploy/scripts/build-for-rk3588.sh --cross

# CMake 包装
./deploy/scripts/build-for-rk3588.sh --cmake

# Docker 构建
./deploy/scripts/build-for-rk3588.sh --docker

# Debug 构建 (不带 --release)
./deploy/scripts/build-for-rk3588.sh --cross --debug

# 禁用 NPU
./deploy/scripts/build-for-rk3588.sh --cross --no-npu
```

## 产物

构建成功后产物位置：

| 产物 | 本机构建 | 交叉编译 |
|------|---------|---------|
| 可执行文件 | `target/release/mupcd` | `target/aarch64-unknown-linux-gnu/release/mupcd` |
| 插件 .so | `target/release/*.so` | `target/aarch64-unknown-linux-gnu/release/*.so` |

## 部署

```bash
# 目标设备上创建目录
sudo mkdir -p /opt/mupc/{bin,lib,config,models,logs,data}

# 复制可执行文件和插件
sudo cp target/aarch64-unknown-linux-gnu/release/mupcd /opt/mupc/bin/
sudo cp target/aarch64-unknown-linux-gnu/release/*.so /opt/mupc/lib/

# 复制 RKNN 运行时库
sudo cp vendor/rknn/librknnrt.so /opt/mupc/lib/

# 复制配置
sudo cp config/*.yaml /opt/mupc/config/

# 复制 systemd 服务
sudo cp deploy/systemd/mupcd.service /etc/systemd/system/

# 启动
sudo systemctl daemon-reload
sudo systemctl enable mupcd
sudo systemctl start mupcd
```
