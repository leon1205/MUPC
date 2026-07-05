# MUPC 构建指南

目标平台：RK3588 ARM64, Ubuntu 20.04+

## 快速开始

```bash
# 1. 克隆代码
git clone git@github.com:leon1205/MUPC.git
cd MUPC/mupc

# 2. 一键安装外部依赖
./scripts/setup-deps.sh --all

# 3. 本机构建 (x86_64 开发)
cargo build -p mupc-core-bin --release

# 4. ARM64 交叉编译 (需要 RK3588 部署时)
export OPENSSL_DIR=../external/openssl-4.0.1/aarch64-install
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
cargo build --workspace --release --target aarch64-unknown-linux-gnu \
    --exclude mupc-iec61850-plugin --exclude device-trait
```

> **外部依赖说明**：`rknn-toolkit2-2.3.2` 和 `external/openssl-4.0.1` 不在 git 仓库中。
> 运行 `./scripts/setup-deps.sh` 自动处理。详见下方各章节。

## 外部依赖

### 依赖矩阵

| 依赖 | x86_64 开发 | ARM64 交叉编译 | ARM64 + NPU |
|------|:--:|:--:|:--:|
| libssl-dev (系统包) | 需要 | - | - |
| gcc-aarch64-linux-gnu | - | 需要 | 需要 |
| external/openssl-4.0.1 (ARM64) | - | 需要 | 需要 |
| rknn-toolkit2-2.3.2 | - | - | 需要 |

> `scripts/setup-deps.sh` 自动检测并安装上述依赖。

### rknn-toolkit2-2.3.2（需手动下载）

RKNN SDK 是 Rockchip 专有软件，无法自动下载。获取方式：

1. [Rockchip RKNPU2 SDK](https://console.zbox.filez.com/l/I00fc3)（提取码: rknn）
2. 或联系 Rockchip FAE 获取
3. 解压到项目父目录：`unzip rknn-toolkit2-2.3.2.zip -d /work/MUPC/`

> 无 RKNN SDK 时，npu feature 自动使用 stub 实现，x86_64 和 ARM64（无 NPU）开发不受影响。

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

**RKNN Toolkit 2 (v2.3.2)** 是 Rockchip 提供的 NPU 模型部署工具链，位于项目父目录 `../rknn-toolkit2-2.3.2/`。

SDK 目录结构：

```
rknn-toolkit2-2.3.2/
├── doc/                    # 文档（快速入门、用户指南、API 参考）
├── rknn-toolkit2/          # Python 模型转换工具（PC 端使用）
├── rknn-toolkit-lite2/     # Python 推理接口（目标板使用）
├── rknpu2/
│   ├── runtime/
│   │   └── Linux/librknn_api/
│   │       ├── include/    # C API 头文件（rknn_api.h）
│   │       ├── aarch64/    # ARM64 librknnrt.so（目标平台）
│   │       └── armhf/      # ARM 32-bit librknnrt.so
│   └── examples/           # C/C++ 示例（含 CMake 交叉编译脚本）
└── README.md
```

本项目的使用方式：

| 用途 | 路径 |
|------|------|
| FFI 头文件引用 | `rknpu2/runtime/Linux/librknn_api/include/rknn_api.h` |
| 目标平台运行时库 | `rknpu2/runtime/Linux/librknn_api/aarch64/librknnrt.so` |
| 交叉编译链接 | `build.rs` 自动搜索 `RKNN_SDK_ROOT` 并复制 `.so` 到 `vendor/rknn/` |
| 部署 | 随 `mupcd` 一同复制到 `/opt/mupc/lib/librknnrt.so` |

环境变量：

```bash
# 指定 SDK 根目录
export RKNN_SDK_ROOT=/work/MUPC/rknn-toolkit2-2.3.2

# 或直接指定 .so 所在目录
export RKNN_VENDOR_DIR=/work/MUPC/rknn-toolkit2-2.3.2/rknpu2/runtime/Linux/librknn_api/aarch64
```

### OpenSSL 4.0.1

`mupc-core-bin` 依赖 `openssl-sys` crate，交叉编译时需要目标平台的 OpenSSL 库。

本项目使用本地源码 `../external/openssl-4.0.1/` 进行交叉编译，无需系统安装：

```bash
# 进入 OpenSSL 源码目录
cd /work/MUPC/external/openssl-4.0.1

# 配置 ARM64 交叉编译
./Configure linux-aarch64 \
    --cross-compile-prefix=aarch64-linux-gnu- \
    --prefix=/work/MUPC/external/openssl-4.0.1/aarch64-install \
    no-shared

# 编译并安装到本地目录
make -j$(nproc)
make install_sw
```

编译后库文件位置：

```
external/openssl-4.0.1/aarch64-install/
├── include/openssl/   # 头文件
├── lib/
│   ├── libssl.a       # SSL 静态库
│   ├── libcrypto.a    # 加密静态库
│   └── pkgconfig/     # pkg-config 文件
└── bin/openssl        # ARM64 openssl 工具（可选）
```

构建 `mupc-core-bin` 时通过环境变量引用：

```bash
export OPENSSL_DIR=/work/MUPC/external/openssl-4.0.1/aarch64-install
export PKG_CONFIG_ALLOW_CROSS=1
```

> **注意**：由于 `librknnrt.so` 仅在 ARM64 目标平台存在，无 NPU feature 时 `rknn_runtime_sys.rs` 自动使用 stub 实现（所有 FFI 函数返回 -1），`build.rs` 跳过链接，无需提供 `librknnrt.so`。

## 构建方式

### 方式 1: Cargo 直接编译

```bash
# 本机编译 (RK3588 开发板上) — npu feature 默认启用
cargo build -p mupc-core-bin --release

# 交叉编译 (x86_64 Linux → ARM64) — npu feature 默认启用
export RKNN_SDK_ROOT=/work/MUPC/rknn-toolkit2-2.3.2
cargo build -p mupc-core-bin --release --target aarch64-unknown-linux-gnu

# Windows 开发环境 — npu feature 自动使用 stub 实现，无需 --no-default-features
cargo build -p mupc-core-bin --release

# 使用 cross-rs 容器化编译
cross build -p mupc-core-bin --release --target aarch64-unknown-linux-gnu
```

> **npu feature 默认行为**：
> - Linux: `npu` 默认启用，`librknnrt.so` 真实链接
> - Windows: `npu` 默认启用但自动降级为 stub 实现（`rknn_runtime_sys.rs` 中 `target_os = "linux"` 条件编译），无需手动 `--no-default-features`
> - 显式禁用: `cargo build --no-default-features`

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

### 一键部署脚本

```bash
# 编译 + 部署 + 重启 (一键完成)
./deploy/scripts/deploy.sh 192.168.3.118 --full

# 仅部署已编译的产物
./deploy/scripts/deploy.sh 192.168.3.118 --restart

# 指定用户名和密码
./deploy/scripts/deploy.sh 192.168.3.118 --user root --password mypwd --full

# 仅部署，不重启
./deploy/scripts/deploy.sh 192.168.3.118
```

依赖 `sshpass`：`sudo apt install sshpass`

### 手动部署

```bash
# 目标设备上创建目录
sudo mkdir -p /opt/mupc/{bin,lib,config,models,logs,data,certs}

# 复制可执行文件和插件
sudo cp target/aarch64-unknown-linux-gnu/release/mupcd /opt/mupc/bin/
sudo cp target/aarch64-unknown-linux-gnu/release/*.so /opt/mupc/lib/

# 复制 RKNN 运行时库
sudo cp vendor/rknn/librknnrt.so /opt/mupc/lib/

# 复制配置
sudo cp config/*.yaml /opt/mupc/config/

# 创建 mupc 用户并设置权限
sudo useradd -r -s /bin/false -d /opt/mupc -M mupc
sudo chown -R mupc:mupc /opt/mupc

# 安装 systemd 服务
sudo cp deploy/systemd/mupcd.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable mupcd

# 启动
sudo systemctl start mupcd
```
