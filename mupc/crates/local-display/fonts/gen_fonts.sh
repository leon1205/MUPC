#!/usr/bin/env bash
# 生成 LVGL 位图字体（CJK 子集）——设计 §1.1.2 / §12.3 的"可复现"落点。
#
# 用法：
#   ./gen_fonts.sh              # 按 UI §3.3 全档位（10 档）生成：24 26 28 32 48 56 64 96 112 148
#   ./gen_fonts.sh 32           # 只生成 32 px（spike 的验收口径）
#
# 依赖：node/npx（lv_font_conv，构建期工具，不进交付物）、python3（码表提取）。
#
# 前置：
#   1) 字库源（OFL-1.1，可入库/可再分发）：
#        curl -L -o NotoSansSC-Regular.otf \
#          https://raw.githubusercontent.com/notofonts/noto-cjk/main/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf
#      （许可文件同目录 NotoSansSC-LICENSE.txt，来自 noto-cjk/Sans/LICENSE）
#   2) 码表由 UI 设计文档 §3.6「全屏用字表」提取，**用字表是唯一真源**：
#        python3 extract_charset.py
#
# 说明（spike 实测，见 docs/TODO/12-v2.0-LVGL-spike-报告.md）：
#   · **默认启用 LVGL 内置 RLE 压缩**（即不传 `--no-compress`）：实测位图字节
#     131,793 → 71,078（-46%），无损、不掉画质。要复现设计原文的无压缩口径，
#     用 `NOCOMPRESS=1 ./gen_fonts.sh 32`。
#   · lv_font_conv 只有 `--symbols <串>`，没有 `--symbols-file`；因此码表文件
#     写成**单行**，用 "$(cat …)" 传入（换行会被 shell 吃掉成空格、污染字符集）。
#   · 产物与字库源均**不入库**（见仓库根 .gitignore），构建前须先跑本脚本。
set -euo pipefail

cd "$(dirname "$0")"

CHARSET="font_subset_charset.txt"
FONT="NotoSansSC-Regular.otf"
SIZES=("$@")
if [ ${#SIZES[@]} -eq 0 ]; then
    SIZES=(24 26 28 32 48 56 64 96 112 148)   # UI §3.3 字号阶梯（全档位，共 10 档）
fi

if [ ! -f "$FONT" ]; then
    echo "缺少字库源 $FONT（见本脚本顶部注释的下载命令）" >&2
    exit 1
fi
if [ ! -f "$CHARSET" ]; then
    echo "缺少码表 $CHARSET，先从 UI 设计文档提取：" >&2
    echo "  python3 extract_charset.py" >&2
    exit 1
fi

SYMBOLS="$(cat "$CHARSET")"
CHARS=$(printf '%s' "$SYMBOLS" | python3 -c 'import sys;print(len(sys.stdin.read()))')

for size in "${SIZES[@]}"; do
    out="lv_font_noto_sc_${size}.c"
    extra=()              # 默认（不传 --no-compress）= 启用 LVGL 内置 RLE
    if [ "${NOCOMPRESS:-0}" = "1" ]; then
        extra=(--no-compress)
    fi
    echo "==> $out (${CHARS} 字符, ${size}px, 4bpp${extra:+ [无压缩]})"
    npx -y lv_font_conv@1.5.2 \
        --font "$FONT" \
        --size "$size" \
        --bpp 4 \
        --format lvgl \
        --symbols "$SYMBOLS" \
        "${extra[@]}" \
        --lv-include lvgl.h \
        -o "$out"
    ls -l "$out"
done

echo
echo "提示：产物体积口径是【位图字节数】而非 .c 文本大小（C 十六进制文本约为其 6.3 倍）："
echo "  grep -o '0x[0-9a-fA-F]*' lv_font_noto_sc_32.c | wc -l"
