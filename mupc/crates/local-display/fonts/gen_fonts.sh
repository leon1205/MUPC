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
#
# **额外产出（入库！）**：`lv_font_cmap.txt` —— 上述 `.c` 生成物的**实际 cmap 派生清单**
# （每行一个 `U+XXXX`）。本脚本在生成完全部字号后**自动提取并覆写**该文件。
#   · 它**必须随字体重生成而更新并入库**（与 `.c` 产物相反：`.c` 不入库，本清单入库）。
#   · 用途：干净 clone / CI 上**没有 `.c` 产物**，`ui/tests.rs` 的两条码表检查
#     （`ui_texts_covered_by_font_cmap` / `runtime_formatters_emit_only_cmap_glyphs`
#     以及 `contract_strings_emit_only_cmap_glyphs`）**改用它做权威基线**，不再跳过
#     —— 否则"没抓到缺字"其实是"压根没查"（B2a 代码质量评审 I3）。
#   · 取**各字号 cmap 的交集**（不是并集）：走查语义是"该字符在**任一档**上屏都得出字形"，
#     只有交集能保证；并集会让"只在部分档存在"的字蒙混过关（真机某档 = 豆腐块，漏报）。
#     实测 2026-09-11 十档 cmap 完全相同（各 324 码位），两种取法结果一致 —— 取交集是把
#     "未来某档掉字"钉在红灯上。
#   · 改完字库 / 字号档位后**务必把新清单一起提交**，否则 `ui/tests.rs` 的**漂移检测**
#     会红（"生成物实际 cmap ≠ 入库清单"）。
#
# **另一份入库派生项**：`lv_font_metrics.txt` —— 各档 `adv_w`（字形步进宽，单位 1/16 px）
# 按 `lv_font_cmap.txt` 的**码位升序**逐一对齐的基线（由本脚本同一次运行提取）。
#   · 它与 `.c` 一起由同一次 `lv_font_conv` 运行产出；`.c` 仍**不入库**，本基线**入库**。
#   · 用途：`ui/tests.rs::measured_text_px` 的**宽度类断言**（值对列宽 / 时间列等宽 /
#     操作者列宽）在**没有 `.c`** 的干净 clone / CI 上**改用它做权威基线** —— 此前那类断言
#     "读不到产物就 `return`"，在 CI 上**整段跳过且照绿**（B2c-1 代码质量评审 ⑤）。
#   · `.c` 与基线同时存在时，Rust 侧**逐值交叉校验**（漂移即响亮失败）。
#   · 改完字库 / 字号档位后**同样必须提交本文件**。
#
# ⚠️ **如实标注（B2c-1 收口 ⑤）**：入库的 `lv_font_metrics.txt` **不是**由本脚本**当前版本**
#    产出的 —— 截至 **2026-09-13** 本机**无法端到端重跑本脚本**（缺 `python3`，且 `lv_font_conv`
#    走 `npx` 联网拉取），因此新增的"写入 `lv_font_metrics.txt`"这一分支**从未在本机执行验证**。
#    入库数值经**独立复算核对**（`ui/tests.rs` 的 `adv_w` 用例逐值自证：与 `fonts/lv_font_cmap.txt`
#    的码位一一对应、每个值 `> 0`、汉字步进宽 > ASCII 步进宽、不同档位值不同）。
#    **本注释刻意写在这里而不是写进产物文件头**：文件头由本脚本**自动生成**，若在其中写"本机
#    跑不了本脚本"这类**环境性**说明，一旦换台能跑通的机器执行，产物就会**自带一句假话**。
#    文件头的"档位自证"两行（`# 本基线档位：` / `# 本基线档位数：`）由 Rust 侧解析，
#    是档位清单的**唯一真源**（`ui/tests.rs::metrics_tiers`；删掉或写错即**响亮失败**）。
set -euo pipefail

cd "$(dirname "$0")"

CHARSET="font_subset_charset.txt"
FONT="NotoSansSC-Regular.otf"
CMAP_OUT="lv_font_cmap.txt"
METRICS_OUT="lv_font_metrics.txt"
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

# ── 提取实际 cmap → lv_font_cmap.txt（**入库派生清单**，见本脚本顶部注释）────────────
# 口径与 `mupc/crates/local-display/src/ui/tests.rs::font_cmap_from_c` **逐条一致**
#   · 码点 = `.range_start + unicode_list_0[i]`（数组存**相对偏移**，不是码点本身）；
#   · 只认"单 cmap + 单 unicode_list + SPARSE_TINY"形态，形态不符即**响亮失败**；
#   · 各字号取**交集**。
# 注意：产物形态若变（如多 cmap / 非 SPARSE_TINY），**两侧都要改**（否则漂移检测会红）。
#
# **同时产出 `lv_font_metrics.txt`（入库！）**：各档 `adv_w` 的**派生基线**（单位 1/16 px，
# 按 `lv_font_cmap.txt` 的**码位升序**逐一对应）。用途见脚本顶部注释与本块末尾说明：
# 干净 clone / CI 没有 `.c` 时，`ui/tests.rs::measured_text_px` 的**宽度类断言**改用它做
# 权威基线，**不再静默跳过**（B2c-1 代码质量评审 ⑤：宽度网此前在 CI 上恒空转）。
python3 - "$CMAP_OUT" "$METRICS_OUT" "${SIZES[@]}" <<'PY'
import re, sys

out_name, metrics_name, sizes = sys.argv[1], sys.argv[2], sys.argv[3:]

def cmap_of(path):
    src = open(path, encoding="utf-8", errors="replace").read()
    if src.count("static const uint16_t unicode_list_0[]") != 1 or src.count(".range_start =") != 1:
        raise SystemExit(f"{path}: lv_font_conv 输出形态已变（多 cmap），请同步更新提取器")
    if "LV_FONT_FMT_TXT_CMAP_SPARSE_TINY" not in src:
        raise SystemExit(f"{path}: cmap 类型不是 SPARSE_TINY，偏移语义不同，请复核")
    rs = int(re.search(r"\.range_start\s*=\s*(\d+)", src).group(1))
    i = src.index("static const uint16_t unicode_list_0[]")
    body = src[i:src.index("};", i)]
    return {rs + int(h, 16) for h in re.findall(r"0x([0-9a-fA-F]+)", body)}

sets = []
for s in sizes:
    p = f"lv_font_noto_sc_{s}.c"
    try:
        sets.append(cmap_of(p))
    except FileNotFoundError:
        raise SystemExit(f"缺少 {p}（本次生成应已产出）")
inter = set.intersection(*sets)
if not inter:
    raise SystemExit("cmap 交集为空 —— 提取器与生成物脱节（不得写出空清单）")
with open(out_name, "w", encoding="utf-8", newline="\n") as f:
    f.write(
        "# lv_font_cmap.txt —— 生成字体（lv_font_cmap 子集）的**实际 cmap 派生清单**，**入库**。\n"
        "#\n"
        "# 由 fonts/gen_fonts.sh 在生成 .c 的同时自动提取（见该脚本顶部注释）；\n"
        "# 每行一个 `U+XXXX`（码位，非字形索引）；`#` 开头为说明行。\n"
        "#\n"
        "# 口径 = 各字号 cmap 的**交集**（走查语义：字符在任一档上屏都得出字形；\n"
        "# 并集会让「只在部分档存在」的字蒙混过关）。实测 2026-09-11 十档相同。\n"
        "#\n"
        "# ⚠️ 字库 / 字号档位变更后**必须重跑 gen_fonts.sh 并提交本文件**，\n"
        "#    否则 ui/tests.rs 的漂移检测（生成物 vs 本清单）会失败。\n"
        f"# 本清单码位数：{len(inter)}\n"
    )
    for cp in sorted(inter):
        f.write(f"U+{cp:04X}\n")
print(f"==> {out_name}（入库派生清单；{len(inter)} 码位，来自 {len(sizes)} 档 cmap 交集）")

# ── 提取各档 adv_w → lv_font_metrics.txt（**入库派生清单**，与上面的 cmap 同一次运行）──────
# 口径与 `ui/tests.rs::adv_w_from_c` **逐条一致**：
#   · `.adv_w` 按 `glyph_dsc[]` 顺序出现；码位 = `.range_start + unicode_list_0[i]`，
#     其 glyph id = i + 1 ⇒ `adv_w[glyph_id]`（id 0 = reserved，Rust 侧同一处口径）；
#   · 输出按 `lv_font_cmap.txt` 的**码位升序**（= 上面 sorted(inter)）逐一对齐，每档一行。
# 用途：干净 clone / CI 无 `.c` 产物时，`ui/tests.rs::measured_text_px` 的宽度类断言（值对
# 列宽 / 时间列等宽 / 操作者列宽）**改用它做权威基线**；两处都在时由 Rust 侧**交叉校验**。
def adv_of(path):
    src = open(path, encoding="utf-8", errors="replace").read()
    rs = int(re.search(r"\.range_start\s*=\s*(\d+)", src).group(1))
    i = src.index("static const uint16_t unicode_list_0[]")
    body = src[i:src.index("};", i)]
    unis = [int(h, 16) for h in re.findall(r"0x([0-9a-fA-F]+)", body)]
    adv = [int(m.group(1)) for m in re.finditer(r"\.adv_w\s*=\s*(\d+)", src)]
    return {rs + off: adv[idx + 1] for idx, off in enumerate(unis) if idx + 1 < len(adv)}

cps = sorted(inter)
tables = []
for s in sizes:
    m = adv_of(f"lv_font_noto_sc_{s}.c")
    missing = [c for c in cps if c not in m]
    if missing:
        raise SystemExit(f"{s}px: cmap 交集里有 {len(missing)} 个码位在该档没有 adv_w（提取器脱节）")
    tables.append([m[c] for c in cps])
with open(metrics_name, "w", encoding="utf-8", newline="\n") as f:
    f.write(
        "# lv_font_metrics.txt —— 生成字体的 **adv_w 派生基线**（**入库项**，体积极小）。\n"
        "#\n"
        "# 由 fonts/gen_fonts.sh 在生成 `.c` 产物时**自动提取并覆写**（与 lv_font_cmap.txt 同一次运行）。\n"
        "#\n"
        "# 格式（每档一行；`#` 开头为说明行）：\n"
        "#   <px> <adv_w_1> <adv_w_2> ... <adv_w_N>\n"
        "# 其中 N = lv_font_cmap.txt 的码位数，按**码位升序**与该清单的每一行**一一对应**；\n"
        "# adv_w 单位 = **1/16 px**（LVGL `glyph_dsc.adv_w` 原值；不计 kerning —— 逐字求和所得为保守上界）。\n"
        "#\n"
        "# 用途（I3 / B2c-1 代码质量评审 ⑤）：干净 clone / CI **没有 `.c` 产物**（产物不入库）时，\n"
        "# 宽度类断言（ui/tests.rs::measured_text_px —— P3/P5 的值对列宽、时间列等宽、操作者列宽）\n"
        "# 用它做**权威基线**，不再\"读不到就静默跳过\"（那等于宽度网在 CI 上恒空转）。\n"
        "#\n"
        "# 口径与 ui/tests.rs::measured_text_px 的 `.c` 路径**逐条一致**（同一次提取逻辑）：\n"
        "#   · 码点 = `.range_start + unicode_list_0[i]`，其 glyph id = i + 1；\n"
        "#   · adv_w 按 glyph id 顺序取自 `glyph_dsc[]`。\n"
        "# 当 `.c` 与入库清单在**同一台机器上同时存在**时，二者会被**交叉校验**（漂移检测）。\n"
        "#\n"
        "# ⚠️ 字库 / 字号档位变更后**必须重跑 gen_fonts.sh 并提交本文件**。\n"
        f"# 本基线档位：{' '.join(str(s) for s in sorted(int(x) for x in sizes))}\n"
        f"# 本基线档位数：{len(sizes)}\n"
    )
    for s, vals in zip(sizes, tables):
        f.write(f"{s} " + " ".join(str(v) for v in vals) + "\n")
print(f"==> {metrics_name}（入库派生基线；{len(sizes)} 档 × {len(cps)} 码位 adv_w）")
PY
