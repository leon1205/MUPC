# FindRKNN.cmake — 定位 RKNN Runtime SDK (librknnrt.so)
#
# 用法: find_package(RKNN REQUIRED)
#
# 定义变量:
#   RKNN_FOUND          - 是否找到
#   RKNN_INCLUDE_DIR    - 头文件目录
#   RKNN_LIBRARY        - librknnrt.so 路径
#   RKNN_LIBRARY_DIR    - 库文件目录 (用于 -L 和 RPATH)
#   RKNN_VERSION        - SDK 版本
#
# 搜索优先级:
#   1. 环境变量 RKNN_SDK_ROOT
#   2. 项目根目录 ../rknn-toolkit2-2.3.2/rknpu2/runtime/Linux/librknn_api/
#   3. 系统默认路径 /opt/rknn /usr/local/rknn

set(RKNN_SEARCH_PATHS)

# 环境变量
if(DEFINED ENV{RKNN_SDK_ROOT})
    list(APPEND RKNN_SEARCH_PATHS "$ENV{RKNN_SDK_ROOT}/rknpu2/runtime/Linux/librknn_api")
    list(APPEND RKNN_SEARCH_PATHS "$ENV{RKNN_SDK_ROOT}")
endif()

# 项目相对路径 (MUPC 仓库与 rknn-toolkit2 平级)
get_filename_component(PROJECT_PARENT "${CMAKE_SOURCE_DIR}/../.." ABSOLUTE)
list(APPEND RKNN_SEARCH_PATHS "${PROJECT_PARENT}/rknn-toolkit2-2.3.2/rknpu2/runtime/Linux/librknn_api")
list(APPEND RKNN_SEARCH_PATHS "${PROJECT_PARENT}/rknn-toolkit2-2.3.2/rknpu2/runtime/Linux/librknn_api/include")
list(APPEND RKNN_SEARCH_PATHS "${PROJECT_PARENT}/rknn-toolkit2-2.3.2")

# 系统默认路径
list(APPEND RKNN_SEARCH_PATHS "/opt/rknn" "/usr/local/rknn" "/usr/local/lib/rknn")

# ── 查找头文件 ──
find_path(RKNN_INCLUDE_DIR
    NAMES rknn_api.h
    PATHS ${RKNN_SEARCH_PATHS}
    PATH_SUFFIXES include include/rknn
    DOC "RKNN Runtime API header directory"
)

# ── 查找库文件 (优先 aarch64) ──
set(RKNN_LIB_NAME "librknnrt.so")

# 根据目标架构选择库目录
if(CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64|arm64|ARM64")
    set(RKNN_ARCH "aarch64")
elseif(CMAKE_SYSTEM_PROCESSOR MATCHES "armv7|armhf|arm")
    set(RKNN_ARCH "armhf")
else()
    # x86_64 开发机 — 不链接，仅需头文件供 bindgen/FFI 使用
    set(RKNN_ARCH "host")
endif()

find_library(RKNN_LIBRARY
    NAMES rknnrt ${RKNN_LIB_NAME}
    PATHS ${RKNN_SEARCH_PATHS}
    PATH_SUFFIXES "${RKNN_ARCH}" "lib/${RKNN_ARCH}" "lib"
    DOC "RKNN Runtime shared library"
)

# ── 推导库目录 ──
if(RKNN_LIBRARY)
    get_filename_component(RKNN_LIBRARY_DIR "${RKNN_LIBRARY}" DIRECTORY)
endif()

# ── 版本检测 ──
set(RKNN_VERSION "2.3.2")
if(RKNN_INCLUDE_DIR AND EXISTS "${RKNN_INCLUDE_DIR}/rknn_api.h")
    file(STRINGS "${RKNN_INCLUDE_DIR}/rknn_api.h" RKNN_VER_LINE
        REGEX "^#define RKNN_API_VERSION")
    if(RKNN_VER_LINE)
        string(REGEX MATCH "([0-9]+)" RKNN_VER_MAJOR "${RKNN_VER_LINE}")
        if(RKNN_VER_MAJOR)
            set(RKNN_VERSION "${RKNN_VER_MAJOR}.x")
        endif()
    endif()
endif()

# ── 标准检查 ──
include(FindPackageHandleStandardArgs)
find_package_handle_standard_args(RKNN
    REQUIRED_VARS RKNN_INCLUDE_DIR
    VERSION_VAR RKNN_VERSION
)

# ── imported target ──
if(RKNN_FOUND AND NOT TARGET RKNN::Runtime)
    add_library(RKNN::Runtime SHARED IMPORTED)
    set_target_properties(RKNN::Runtime PROPERTIES
        IMPORTED_LOCATION "${RKNN_LIBRARY}"
        INTERFACE_INCLUDE_DIRECTORIES "${RKNN_INCLUDE_DIR}"
    )
endif()

# ── 输出信息 ──
mark_as_advanced(RKNN_INCLUDE_DIR RKNN_LIBRARY RKNN_LIBRARY_DIR)
