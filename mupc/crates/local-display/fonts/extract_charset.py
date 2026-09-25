#!/usr/bin/env python3
"""从 UI 设计文档 §3.6「全屏用字表」提取 lv_font_conv 的字符集（唯一真源）。

用法：
    python extract_charset.py \
        [--ui-doc <path>] [--out font_subset_charset.txt]

产出：**单行**码表文件（无换行分隔），可直接
    lv_font_conv --symbols "$(cat font_subset_charset.txt)"
另把逐字清单打印到 stdout 供人工核对（审计用，不落盘）。

真源与口径：
  · 中文用字 = §3.6 表格「中文用字」列的**全部汉字**（逐字提取，不靠猜）。
  · 非中文字形 = 表末「非中文字形另需纳入子集」段声明的拉丁/数字/符号/几何箭头，
    ∪ 该列里实际出现的 ASCII 可打印字符。
  · 空格 (U+0020) 显式纳入（否则长文案断词无处落笔）。
  · U+1F512 🔒 按文档「以几何锁形替代」**不纳入**（由自绘几何锁形承接）。

R-2 护栏（产品裁定 2026-09-25）——**重跑不得丢字**：
  本脚本以 §3.6 为"唯一真源"并**覆盖写** `--out`；而该表单元格里混有**叙述性文字**
  ⇒ 谁重跑一遍都会得到与入库码表**不同**的结果，且**不报错**（静默漂移）。危险的一侧是
  **丢字**（屏上出豆腐块、真机不可自愈）。故在写入前比对：
  · **硬失败**：新结果 ⊉ 入库码表（丢了字）⇒ 打印缺失字符集并非 0 退出；
  · **可见但不失败**：新结果净增字符（多为表内叙述性文字）⇒ 打印净增集合与条数。
"""
import argparse
import os
import re
import sys

DEFAULT_UI_DOC = os.path.join(
    "..", "..", "..", "..", "docs", "superpowers", "plans", "modules",
    "12-MUPC-本地显示终端-UI设计文档.md",
)
DEFAULT_OUT = "font_subset_charset.txt"

SECTION_RE = re.compile(r"^###\s*3\.6\s")
CJK_RE = re.compile(r"[㐀-䶿一-鿿豈-﫿]")

# 表末声明的非中文字形（逐条来自 UI §3.6 的说明段；下面会断言它们确实出现在文档中）
# **T21a（2026-09-25）扩充**：U-73 外设上屏引入的单位/短标签需要 `ppm` / `kvar` / `Hz` /
# `PACK` / `dB/M` / `A-B` 等拉丁字形（旧声明段只含 `kW` / `A B C` / `MB` 等）⇒
# 声明段与本清单同步扩充，否则重跑本脚本会**静默丢掉**这些字（本仓已有的坑）。
DECLARED_LATIN = (
    "MUPC PCS SOC SOH SOE BMS REG IEC IP CPU M1 DO ERROR WARN INFO DEBUG "
    "kW kWh kVA kvar kvarh kPa kΩ Hz ppm Ah VOC PACK pack dB/M A-B V A B C D H K MB s h"
)
DECLARED_SYMBOLS = "0123456789.:–·/%Σ!?×≤≥→←+−⚠（）℃Ω"
DECLARED_GEOMETRY = "▲▼●○■❚✕✓"
# 🔒 由几何锁形替代，不入码表
EXCLUDED = "🔒"
# Markdown 行内代码反引号是排版语法、不是屏上文案，剔除
EXCLUDED_ASCII = "`"


def section_lines(text):
    out, inside = [], False
    for line in text.splitlines():
        if SECTION_RE.match(line):
            inside = True
            continue
        if inside and line.startswith("## "):
            break
        if inside:
            out.append(line)
    if not out:
        raise SystemExit("未找到 §3.6 全屏用字表")
    return out


def load_committed(path):
    """读**入库**码表原文（单行、无换行；只容忍 CR/LF，**不 strip** —— 空格 U+0020 是
    合法字符且排在码表首位，`strip()` 会把它误删 ⇒ 产生假"净增"）。"""
    try:
        with open(path, encoding="utf-8") as f:
            return f.read().replace("\n", "").replace("\r", "")
    except FileNotFoundError:
        return ""


def enforce_no_char_loss(committed, fresh, out_path):
    """R-2 护栏（产品裁定 2026-09-25）：**重跑不得丢字**。见模块 docstring。

    返回 `None`；丢字时 `raise SystemExit`（响亮失败、非 0 退出码）。
    净增只在 stdout 打印（不失败）。
    """
    missing = sorted(set(committed) - set(fresh), key=ord)
    if missing:
        detail = " ".join(f"{c} U+{ord(c):04X}" for c in missing)
        raise SystemExit(
            f"❌ 码表护栏（R-2）：本次重跑**丢了 {len(missing)} 个入库字符** ⇒ 屏上必出豆腐块。\n"
            f"   缺失：{detail}\n"
            f"   入库 `{out_path}` {len(committed)} 字，本次提取 {len(fresh)} 字。\n"
            f"   处置：① 若**确为有意删字**，先手工改 `{out_path}` 再重跑（删字是显式决定，"
            f"不得靠重跑悄悄发生）；② 否则是 UI §3.6「中文用字」列被改动 / 漏列 ⇒ 补齐后重跑。"
        )
    added = sorted(set(fresh) - set(committed), key=ord)
    if added:
        detail = " ".join(f"{c} U+{ord(c):04X}" for c in added)
        print(
            f"⚠️  码表护栏（R-2）：本次提取**净增 {len(added)} 字**（不失败，仅提示）。\n"
            f"   净增：{detail}\n"
            f"   提示：多半来自 §3.6 单元格内的**叙述性文字**（该表第 3 列应只写用字，"
            f"说明请写进表下的 `>` 段）。这些字会被写进 `{out_path}` ⇒ 若为无意漂移请清理表格；"
            f"若确为新增用字，**必须同批重跑 gen_fonts.sh 并提交 lv_font_cmap.txt / lv_font_metrics.txt**。"
        )


def main():
    # 护栏与逐字清单都会打印**非 GBK** 字符（`⚠` / `❚` / `✕` …）。Windows 默认 stdout 编码
    # 是 GBK ⇒ 会 `UnicodeEncodeError` **打断护栏本身**（响亮失败变成莫名崩溃）。
    # 统一强制 UTF-8（Python ≥3.7），与"响亮失败必须真的能喊出来"的要求一致。
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="backslashreplace")
        sys.stderr.reconfigure(encoding="utf-8", errors="backslashreplace")
    except (AttributeError, ValueError):
        pass

    ap = argparse.ArgumentParser()
    ap.add_argument("--ui-doc", default=DEFAULT_UI_DOC)
    ap.add_argument("--out", default=DEFAULT_OUT)
    args = ap.parse_args()

    here = os.path.dirname(os.path.abspath(__file__))
    ui_doc = args.ui_doc if os.path.isabs(args.ui_doc) else os.path.join(here, args.ui_doc)
    with open(ui_doc, encoding="utf-8") as f:
        doc = f.read()
    lines = section_lines(doc)
    section = "\n".join(lines)

    # 断言：表末声明的字形/字串确实出现在本节（防"文档改了脚本没跟"）
    # 数字是 `0–9` 的**区间**写法，按字面断言该区间描述而非逐个数字。
    must_literal = [
        "MUPC", "PCS", "SOC", "SOH", "SOE", "BMS", "REG", "IEC", "IP", "CPU", "M1", "DO",
        "ERROR", "WARN", "INFO", "DEBUG", "kW", "kWh", "kVA", "kvar", "kvarh", "kPa",
        "kΩ", "Hz", "ppm", "Ah", "VOC", "PACK", "dB/M", "A-B", "MB",
        "0–9", ".", ":", "–", "·", "/", "%", "Σ", "!", "?", "×", "≤", "≥",
        "→", "←", "+", "−", "⚠", "（", "）", "℃", "Ω",
        "▲", "▼", "●", "○", "■", "❚", "✕", "✓",
    ]
    for tok in must_literal:
        if tok not in section:
            raise SystemExit(f"声明字形 `{tok}` 未出现在 §3.6 —— 文档与脚本已漂移")

    han = set()
    ascii_used = set()
    n_rows = 0
    for line in lines:
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        # 只跳表头与分隔行；**续行**的首格是空的（页面/区块列合并），不能按空跳过。
        if len(cells) < 3 or cells[0].startswith("页面") or cells[0].startswith("--"):
            continue
        n_rows += 1
        col = cells[2]  # 「中文用字」列
        han |= set(CJK_RE.findall(col))
        ascii_used |= {
            c for c in col
            if 0x20 <= ord(c) < 0x7F and c != " " and c not in EXCLUDED_ASCII
        }

    latin = set(DECLARED_LATIN) | ascii_used | set("0123456789")
    # 字符集 = 汉字 ∪ 拉丁/数字 ∪ 声明符号/几何 ∪ 空格
    charset = han | latin | set(DECLARED_SYMBOLS) | set(DECLARED_GEOMETRY) | {" "}
    charset -= set(EXCLUDED)

    ordered = sorted(charset, key=ord)
    fresh = "".join(ordered)
    # ── R-2 护栏：**写入前**比对入库码表（丢字 ⇒ 硬失败；净增 ⇒ 可见但不失败）──
    committed = load_committed(args.out)
    enforce_no_char_loss(committed, fresh, args.out)
    print(
        f"码表护栏（R-2）：入库 {len(committed)} 字 → 本次 {len(fresh)} 字"
        f"（缺失 0；净增 {len(set(fresh) - set(committed))}）"
    )
    with open(args.out, "w", encoding="utf-8", newline="") as f:
        f.write(fresh)

    print(f"UI 文档: {os.path.relpath(ui_doc, here)}")
    print(f"用字表行数: {n_rows}")
    print(f"汉字: {len(han)} | 拉丁/数字: {len(latin)} | 符号+几何: {len(set(DECLARED_SYMBOLS) | set(DECLARED_GEOMETRY))}")
    print(f"总字符数（含空格）: {len(ordered)}")
    print(f"产出: {args.out} ({os.path.getsize(args.out)} B, 单行)")
    print("── 汉字逐字清单（审计）──")
    print("".join(sorted(han, key=ord)))
    print("── 非汉字清单（审计）──")
    print("".join(sorted(charset - han, key=ord)))


if __name__ == "__main__":
    sys.exit(main())
