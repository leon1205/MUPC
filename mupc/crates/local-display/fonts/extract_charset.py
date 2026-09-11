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
DECLARED_LATIN = "MUPC PCS SOC BMS REG IEC IP CPU M1 DO ERROR WARN INFO DEBUG kW A B C MB s h"
DECLARED_SYMBOLS = "0123456789.:–·/%Σ!?×≤≥→←+−⚠"
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


def main():
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
        "MUPC", "PCS", "SOC", "BMS", "REG", "IEC", "IP", "CPU", "M1", "DO",
        "ERROR", "WARN", "INFO", "DEBUG", "kW", "MB",
        "0–9", ".", ":", "–", "·", "/", "%", "Σ", "!", "?", "×", "≤", "≥",
        "→", "←", "+", "−", "⚠",
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
    with open(args.out, "w", encoding="utf-8", newline="") as f:
        f.write("".join(ordered))

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
