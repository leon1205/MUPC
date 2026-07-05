# CMakeLists.txt — MUPC 编译编排 (Rust 项目 Cargo 包装)
#
# CMake 角色: RKNN SDK 检测 → 交叉编译配置 → Cargo 编译 → 产物打包
# 不直接编译 Rust 源码 (由 Cargo 负责), CMake 负责环境准备和产物整理.
#
# 用法:
#   本机构建:  cmake -B build && cmake --build build
#   交叉编译:  cmake -B build-arm64 -DCMAKE_TOOLCHAIN_FILE=cmake/toolchain-aarch64-linux.cmake \
#                -DRKNN_SDK_ROOT=../../rknn-toolkit2-2.3.2 && cmake --build build-arm64
#   指定 feature: cmake -B build -DENABLE_NPU=ON
cmake_minimum_required(VERSION 3.20)
project(MUPC
    VERSION 0.1.0
    DESCRIPTION "微电网特种调控装置通信管理模块"
    LANGUAGES C   # 仅需 C (Rust FFI 链接)，无需 CXX 编译器
)

# ═══════════════════════════════════════════════════════════════
# 1. 选项
# ═══════════════════════════════════════════════════════════════

option(ENABLE_NPU "启用 NPU (RKNN) 推理支持" ON)
option(ENABLE_LTO "启用 Link-Time Optimization" ON)
option(BUILD_TESTS "编译测试" OFF)
option(GENERATE_COMPILE_COMMANDS "生成 compile_commands.json" OFF)

# ═══════════════════════════════════════════════════════════════
# 2. RKNN SDK 检测
# ═══════════════════════════════════════════════════════════════

if(ENABLE_NPU)
    # 允许通过 -DRKNN_SDK_ROOT=/path 指定
    if(DEFINED RKNN_SDK_ROOT)
        set(ENV{RKNN_SDK_ROOT} "${RKNN_SDK_ROOT}")
    elseif(NOT DEFINED ENV{RKNN_SDK_ROOT})
        # 自动检测: 项目父目录的 rknn-toolkit2-2.3.2
        get_filename_component(WORKSPACE_PARENT "${CMAKE_SOURCE_DIR}/../.." ABSOLUTE)
        set(RKNN_AUTO_PATH "${WORKSPACE_PARENT}/rknn-toolkit2-2.3.2")
        if(EXISTS "${RKNN_AUTO_PATH}")
            set(ENV{RKNN_SDK_ROOT} "${RKNN_AUTO_PATH}")
            message(STATUS "RKNN SDK auto-detected: ${RKNN_AUTO_PATH}")
        endif()
    endif()

    list(APPEND CMAKE_MODULE_PATH "${CMAKE_SOURCE_DIR}/cmake")
    find_package(RKNN 2.3)

    if(RKNN_FOUND)
        message(STATUS "RKNN SDK: ${RKNN_INCLUDE_DIR}")
        message(STATUS "RKNN Library: ${RKNN_LIBRARY}")
    else()
        message(WARNING "RKNN SDK not found — building without NPU support")
        set(ENABLE_NPU OFF)
    endif()
endif()

# ═══════════════════════════════════════════════════════════════
# 3. Cargo 构建配置
# ═══════════════════════════════════════════════════════════════

find_program(CARGO_EXECUTABLE cargo REQUIRED)
message(STATUS "Cargo: ${CARGO_EXECUTABLE}")

# 构建目标
set(CARGO_BUILD_TARGET "mupc-core-bin")

# Cargo feature flags
set(CARGO_FEATURES "")
set(CARGO_FEATURES_STR "")
set(CARGO_FEATURE_FLAG "")
if(ENABLE_NPU AND RKNN_FOUND)
    list(APPEND CARGO_FEATURES "npu")
endif()
if(ENABLE_LTO)
    set(CARGO_PROFILE "--release")
else()
    set(CARGO_PROFILE "")
endif()

# 拼接 features
if(CARGO_FEATURES)
    string(JOIN "," CARGO_FEATURES_STR ${CARGO_FEATURES})
    set(CARGO_FEATURE_FLAG "--features=${CARGO_FEATURES_STR}")
else()
    # 无 NPU 时需显式禁用 default features (Cargo.toml 中 default = ["npu"])
    set(CARGO_FEATURE_FLAG "--no-default-features")
endif()

# 目标平台
if(CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64|arm64")
    set(CARGO_TARGET "--target=aarch64-unknown-linux-gnu")
else()
    set(CARGO_TARGET "")
endif()

# RKNN vendor 目录设置
if(ENABLE_NPU AND RKNN_FOUND AND RKNN_LIBRARY_DIR)
    set(ENV{RKNN_VENDOR_DIR} "${RKNN_LIBRARY_DIR}")
    message(STATUS "RKNN_VENDOR_DIR = $ENV{RKNN_VENDOR_DIR}")
endif()

# ═══════════════════════════════════════════════════════════════
# 4. Build target
# ═══════════════════════════════════════════════════════════════

# 生成 vendor/rknn 目录并复制 .so (用于 build.rs SHA256 校验)
if(ENABLE_NPU AND RKNN_FOUND AND RKNN_LIBRARY)
    set(VENDOR_RKNN_DIR "${CMAKE_SOURCE_DIR}/vendor/rknn")
    file(MAKE_DIRECTORY "${VENDOR_RKNN_DIR}")

    add_custom_command(
        OUTPUT "${VENDOR_RKNN_DIR}/librknnrt.so"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
            "${RKNN_LIBRARY}" "${VENDOR_RKNN_DIR}/librknnrt.so"
        COMMENT "Copying librknnrt.so → vendor/rknn/"
    )
    set(RKNN_COPY_TARGET "${VENDOR_RKNN_DIR}/librknnrt.so")
else()
    set(RKNN_COPY_TARGET "")
endif()

# Cargo build — 显式传播环境变量到构建阶段
set(RKNN_VENDOR_ENV "")
if(ENABLE_NPU AND RKNN_FOUND AND RKNN_LIBRARY_DIR)
    set(RKNN_VENDOR_ENV "RKNN_VENDOR_DIR=$ENV{RKNN_VENDOR_DIR}")
endif()

add_custom_target(mupc_build ALL
    COMMAND ${CMAKE_COMMAND} -E env
        CARGO_TERM_COLOR=always
        RUST_BACKTRACE=1
        ${RKNN_VENDOR_ENV}
        ${CARGO_EXECUTABLE} build ${CARGO_PROFILE}
            -p ${CARGO_BUILD_TARGET}
            ${CARGO_FEATURE_FLAG}
            ${CARGO_TARGET}
    DEPENDS ${RKNN_COPY_TARGET}
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    COMMENT "Building MUPC with Cargo..."
    USES_TERMINAL
)

# ═══════════════════════════════════════════════════════════════
# 5. 产物整理
# ═══════════════════════════════════════════════════════════════

# 确定输出目录
if(CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64|arm64")
    set(OUTPUT_DIR "${CMAKE_SOURCE_DIR}/target/aarch64-unknown-linux-gnu/release")
else()
    set(OUTPUT_DIR "${CMAKE_SOURCE_DIR}/target/release")
endif()

# 安装目标
install(DIRECTORY
    "${OUTPUT_DIR}/"
    DESTINATION bin
    FILES_MATCHING
    PATTERN "mupcd"
    PATTERN "*.so"
)

install(DIRECTORY
    "${CMAKE_SOURCE_DIR}/config/"
    DESTINATION etc/mupc
    PATTERN "*.yaml"
    PATTERN "*.toml"
)

install(DIRECTORY
    "${CMAKE_SOURCE_DIR}/deploy/"
    DESTINATION share/mupc
)

# ═══════════════════════════════════════════════════════════════
# 6. 辅助 targets
# ═══════════════════════════════════════════════════════════════

# cmake --build build --target test
add_custom_target(test
    COMMAND ${CARGO_EXECUTABLE} test ${CARGO_FEATURE_FLAG}
        --exclude mupc-iec61850-plugin
        --exclude rs485-plugin
        --exclude device-trait
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
)

# cmake --build build --target clippy
add_custom_target(clippy
    COMMAND ${CARGO_EXECUTABLE} clippy --workspace -- -D warnings
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
)

# cmake --build build --target fmt
add_custom_target(fmt
    COMMAND ${CARGO_EXECUTABLE} fmt --all
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
    USES_TERMINAL
)

# cmake --build build --target clean
add_custom_target(clean-all
    COMMAND ${CARGO_EXECUTABLE} clean
    WORKING_DIRECTORY "${CMAKE_SOURCE_DIR}"
)

# ═══════════════════════════════════════════════════════════════
# 7. 状态摘要
# ═══════════════════════════════════════════════════════════════

message(STATUS "")
message(STATUS "=== MUPC Build Configuration ===")
message(STATUS "  Target:         ${CMAKE_SYSTEM_PROCESSOR}")
message(STATUS "  NPU enabled:    ${ENABLE_NPU}")
message(STATUS "  LTO:            ${ENABLE_LTO}")
message(STATUS "  Build type:     ${CMAKE_BUILD_TYPE}")
message(STATUS "  Cargo target:   ${CARGO_BUILD_TARGET}")
message(STATUS "  Cargo features: ${CARGO_FEATURES_STR}")
message(STATUS "  Cargo profile:  ${CARGO_PROFILE}")
message(STATUS "  Output dir:     ${OUTPUT_DIR}")
if(ENABLE_NPU AND RKNN_FOUND)
    message(STATUS "  RKNN include:   ${RKNN_INCLUDE_DIR}")
    message(STATUS "  RKNN library:   ${RKNN_LIBRARY}")
endif()
message(STATUS "")
