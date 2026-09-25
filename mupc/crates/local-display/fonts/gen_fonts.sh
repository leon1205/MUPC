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
# ✅ **前置条件（2026-09-18 订正）**：旧注释写"截至 2026-09-13 本机无法端到端重跑本脚本（缺
#    `python3`）"——**这句话是错的**，真实原因不是"缺解释器"：本机装了 Python 3.10（在 `python`
#    这个名字下，实测可用），而 `python3` 解析到 **Windows Store 的空壳别名**
#    （`~/AppData/Local/Microsoft/WindowsApps/python3`：执行**无输出也无报错**）⇒ 脚本 `:82` / `:118`
#    两处 `python3` 静默失败。**解除方式**：给 `python3` 加一个指向真实解释器的 shim（或把这两处
#    改成 `python`）——加 shim 后 **10 档全部端到端跑通**（另一前置 `npx -y lv_font_conv@1.5.2`
#    首次需联网拉取，之后走 npx 缓存）。
#    故入库的 `lv_font_metrics.txt`（**含**其头部"基线档位 / 档位数"两行）**已是本脚本当前版本
#    的产物**：2026-09-18 重跑生成，与 `lv_font_cmap.txt` 同一次运行，头部两行与 `:207-208` 的
#    写法逐字一致。
#    旧口径下的兜底（**保留，仍有效**）：入库数值另有**独立复算核对**（`ui/tests.rs` 的 `adv_w`
#    用例逐值自证：与 `fonts/lv_font_cmap.txt` 的码位一一对应、每个值 `> 0`、
#    汉字步进宽 > ASCII 步进宽、不同档位值不同）。
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
# **T21a（2026-09-25）更新：支持 N 个 cmap。** 背景：U-73 外设上屏把码表从 326 扩到 **464**
# 字符（新增 `ppm` / `kvar` / `Hz` / `PACK` 等单位所需的小写拉丁字母等），ASCII 段因此变"密"
# ⇒ `lv_font_conv` **不再**产出单个 `SPARSE_TINY`，而是切成
#   `cmaps[0] = FORMAT0_FULL`（U+0020..U+0057 的密集段）+ `cmaps[1] = SPARSE_TINY`（其余）。
# 旧提取器（`unicode_list_0` ×1 + `range_start` ×1）**直接报错**，故本块改为按 LVGL 的
# `get_glyph_dsc_id`（`vendor/lvgl/src/font/fmt_txt/lv_font_fmt_txt.c:280-330`）逐 cmap 解析：
#   · `FORMAT0_TINY` / `FORMAT0_FULL` / `SPARSE_TINY` / `SPARSE_FULL` 四种类型全覆盖
#     （`FORMAT0_FULL` 的 `glyph_id_ofs_list[rcp] == 0 && rcp != 0` ⇒ 该码位**无字形**）；
#   · 各字号仍取**交集**（语义不变）。
# ✅ **Rust 侧 `ui/tests.rs` 已同步（同一批，2026-09-25）——两侧口径一致**：本块的
# Python 提取器 `cmap_entries()` 与 `mupc/crates/local-display/src/ui/tests.rs` 的
# `cmap_entries_from_c()` 是**同源的两份实现**（同一套 LVGL `get_glyph_dsc_id` 语义：N 个
# cmap × 四种类型 + `FORMAT0_FULL` 的 `rcp != 0 && ofs == 0` ⇒ 无字形）。`font_cmap_from_c` /
# `adv_w_from_c` 都建在 `cmap_entries_from_c` 之上 ⇒ 本机**存在 `.c` 产物**时那两条交叉校验
# **照常通过**（不再是"单 cmap 假设 ⇒ 响亮失败"）。干净 clone / CI 无 `.c` ⇒ 走入库基线。
# 注意：产物形态若变（如新增 cmap 类型），**两侧都要改**（否则漂移检测会红）。
#
# **同时产出 `lv_font_metrics.txt`（入库！）**：各档 `adv_w` 的**派生基线**（单位 1/16 px，
# 按 `lv_font_cmap.txt` 的**码位升序**逐一对应）。用途见脚本顶部注释与本块末尾说明：
# 干净 clone / CI 没有 `.c` 时，`ui/tests.rs::measured_text_px` 的**宽度类断言**改用它做
# 权威基线，**不再静默跳过**（B2c-1 代码质量评审 ⑤：宽度网此前在 CI 上恒空转）。
python3 - "$CMAP_OUT" "$METRICS_OUT" "${SIZES[@]}" <<'PY'
import re, sys

out_name, metrics_name, sizes = sys.argv[1], sys.argv[2], sys.argv[3:]


def cmap_entries(path):
    """解析 `cmaps[]`（**支持 N 个 cmap**；T21a 扩充码表后 lv_font_conv 会把 ASCII 密集段
    单独切成一个 `FORMAT0_FULL`，与其余 `SPARSE_TINY` 并列 ⇒ 单 cmap 假设不再成立）。

    返回 `[(码位, glyph_id), ...]`，语义**逐条照抄 LVGL** 的 `get_glyph_dsc_id`
    （`vendor/lvgl/src/font/fmt_txt/lv_font_fmt_txt.c:280-330`）：
      · `FORMAT0_TINY`  ：`glyph_id = glyph_id_start + rcp`
      · `FORMAT0_FULL`  ：`glyph_id = glyph_id_start + glyph_id_ofs_list[rcp]`；该表项为 0
        且 `rcp != 0` ⇒ **该码位无字形**（LVGL 原文注释：首字符必有效、其 offset 恒 0）
      · `SPARSE_TINY`   ：`glyph_id = glyph_id_start + unicode_list 下标`
      · `SPARSE_FULL`   ：`glyph_id = glyph_id_start + glyph_id_ofs_list[下标]`
    """
    src = open(path, encoding="utf-8", errors="replace").read()

    def nums(decl, name):
        """取数组体里的全部整数。`unicode_list` 用**十六进制**、`glyph_id_ofs_list` 用
        **十进制**（lv_font_conv 的实际写法）⇒ 两种都认，避免"只认十六进制"漏读。"""
        i = src.index(f"static const {decl} {name}[]")
        # 必须从声明行**之后**的 `{` 起切：数组名与 `uint8_t` 里都含数字，
        # 从声明行起切会把它们当元素读进来（实测 +2 个假元素）。
        b = src.index("{", i)
        body = src[b + 1:src.index("};", b)]
        return [int(t, 0) for t in re.findall(r"0x[0-9a-fA-F]+|[0-9]+", body)]

    def u16(name):
        return nums("uint16_t", name)

    def u8(name):
        return nums("uint8_t", name)

    pat = re.compile(
        r"\.range_start = (\d+),\s*\.range_length = (\d+),\s*\.glyph_id_start = (\d+),\s*"
        r"\.unicode_list = (\w+),\s*\.glyph_id_ofs_list = (\w+),\s*\.list_length = (\d+),\s*"
        r"\.type = (LV_FONT_FMT_TXT_CMAP_\w+),?"
    )
    entries = []
    for m in pat.finditer(src):
        rs, rl, gis = int(m.group(1)), int(m.group(2)), int(m.group(3))
        ul, ofs, ln, ty = m.group(4), m.group(5), int(m.group(6)), m.group(7)
        if ty == "LV_FONT_FMT_TXT_CMAP_FORMAT0_TINY":
            entries += [(rs + rcp, gis + rcp) for rcp in range(rl)]
        elif ty == "LV_FONT_FMT_TXT_CMAP_FORMAT0_FULL":
            tbl = u8(ofs)
            if len(tbl) != rl:
                raise SystemExit(f"{path}: FORMAT0_FULL 的 glyph_id_ofs_list 长度（{len(tbl)}）≠ range_length（{rl}）")
            entries += [(rs + rcp, gis + tbl[rcp]) for rcp in range(rl) if tbl[rcp] != 0 or rcp == 0]
        elif ty == "LV_FONT_FMT_TXT_CMAP_SPARSE_TINY":
            lst = u16(ul)
            if len(lst) != ln:
                raise SystemExit(f"{path}: SPARSE_TINY 的 unicode_list 长度（{len(lst)}）≠ list_length（{ln}）")
            entries += [(rs + v, gis + i) for i, v in enumerate(lst)]
        elif ty == "LV_FONT_FMT_TXT_CMAP_SPARSE_FULL":
            lst, o16 = u16(ul), u16(ofs)
            entries += [(rs + v, gis + o16[i]) for i, v in enumerate(lst)]
        else:
            raise SystemExit(f"{path}: 未知 cmap 类型 `{ty}`（解析器与生成物脱节）")
    if not entries:
        raise SystemExit(f"{path}: 未解析到任何 cmap 条目（产物形态已变，请同步更新本提取器）")
    return entries


def cmap_of(path):
    return {cp for cp, _ in cmap_entries(path)}


def adv_of(path):
    src = open(path, encoding="utf-8", errors="replace").read()
    adv = [int(m.group(1)) for m in re.finditer(r"\.adv_w\s*=\s*(\d+)", src)]
    out = {}
    for cp, gid in cmap_entries(path):
        if gid < len(adv):
            out[cp] = adv[gid]
    return out


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
        "# 并集会让「只在部分档存在」的字蒙混过关）。\n"
        "#\n"
        "# ⚠️ 字库 / 字号档位变更后**必须重跑 gen_fonts.sh 并提交本文件**，\n"
        "#    否则 ui/tests.rs 的漂移检测（生成物 vs 本清单）会失败。\n"
        f"# 本清单码位数：{len(inter)}\n"
    )
    for cp in sorted(inter):
        f.write(f"U+{cp:04X}\n")
print(f"==> {out_name}（入库派生清单；{len(inter)} 码位，来自 {len(sizes)} 档 cmap 交集）")

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
        "#   · 码位 → glyph_id 走 LVGL `get_glyph_dsc_id` 的四种 cmap 语义（支持 N 个 cmap）；\n"
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
