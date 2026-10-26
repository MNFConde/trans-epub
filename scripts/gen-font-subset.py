#!/usr/bin/env python3
"""生成 trans-epub 内嵌字体的预子集资产（一次性/再生成工具，非门禁）。

输入 Maple Mono CN 全量 TTF（unhinted、sha256 钉版，见 scripts.md 台账），
按《通用规范汉字表》8105 字 + 编程文档常用字符子集并改名（OFL-1.1 修改版
义务：族名避开原族名，name 表 0 号保留版权并附修改声明）。

用法（uv 提供 fonttools，无需系统安装）：
    uv run --with fonttools python scripts/gen-font-subset.py \
        --input  <MapleMono-CN-Regular.ttf> \
        --hanzi  <8105 字表 txt> \
        --output crates/trans-epub-core/assets/fonts/TransEpubMono-Regular.subset.ttf

产物入 crates/trans-epub-core/assets/fonts/，由 core 以 include_bytes! 内置；
OFL 义务文件（OFL-1.1.txt / NOTICE）同目录随书分发。
"""

import argparse
import sys

from fontTools import subset
from fontTools.ttLib import TTFont

# 改名后的字体族名：OFL 修改版不得沿用原族名（保守合规，上游未声明 RFN）。
FAMILY = "TransEpub Mono"
FULL_NAME = "TransEpub Mono Regular"
PS_NAME = "TransEpubMono-Regular"

# 编程文档正文/代码块常用非汉字字符（ASCII 之外的补充集）。
EXTRA_CHARS = (
    # CJK 标点与全角形式
    "，。、；：？！“”‘’（）《》〈〉【】〔〕「」『』…—·～"
    "　％＋－＊／＝＜＞（）［］｛｝"
    # 西文排版标点
    "‘’“”–—…′″€£¥©®°±·"
    # 制表符/树形结构（代码块与 dry-run 输出）
    "─│┌┐└┘├┤┬┴┼╌╎═║"
    # 箭头与常用数学符
    "→←↑↓⇒⇐↔↦≈≠≤≥±×÷∞∴∵∈∉⊂⊃∪∩¬√∑∏∫"
    # 希腊字母（文档中的数学/符号名）
    "αβγδεζηθικλμνξοπρστυφχψωΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ"
)


def build_charset(hanzi_path: str) -> str:
    hanzi = "".join(c for c in open(hanzi_path, encoding="utf-8").read() if not c.isspace())
    if len(hanzi) != 8105 or len(set(hanzi)) != 8105:
        sys.exit(f"hanzi table must be 8105 unique chars, got {len(hanzi)}/{len(set(hanzi))}")
    ascii_printable = "".join(chr(c) for c in range(0x20, 0x7F))
    return hanzi + ascii_printable + EXTRA_CHARS


def rename(font: TTFont) -> None:
    name = font["name"]
    year = "2026"
    for pid, eid, lid in [(3, 1, 0x409), (1, 0, 0)]:
        name.setName(FAMILY, 1, pid, eid, lid)
        name.setName("Regular", 2, pid, eid, lid)
        name.setName(f"{FULL_NAME}; subset of Maple Mono CN; {year}", 3, pid, eid, lid)
        name.setName(FULL_NAME, 4, pid, eid, lid)
        name.setName(PS_NAME, 6, pid, eid, lid)
    # typographic subfamily（16/17）：单字重不需要，移除避免残留原族名
    name.removeNames(nameID=16)
    name.removeNames(nameID=17)
    # 0 号版权：保留原版权行 + 修改声明（OFL 1.1 条款 1/2 义务）
    for rec in name.names:
        if rec.nameID == 0:
            original = rec.toUnicode()
            notice = (
                f"{original}\n"
                f"Modified by trans-epub ({year}): subsetted to the Table of General "
                f"Standard Chinese Characters (8105) plus programming-doc punctuation, "
                f"and renamed to '{FAMILY}'. Original: Maple Mono CN, "
                "https://github.com/subframe7536/maple-font (OFL-1.1)."
            )
            name.setName(notice, 0, rec.platformID, rec.platEncID, rec.langID)
            break


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--input", required=True)
    ap.add_argument("--hanzi", required=True)
    ap.add_argument("--output", required=True)
    args = ap.parse_args()

    text = build_charset(args.hanzi)
    opts = subset.Options()
    opts.layout_features = ["*"]  # 保留 liga 等（阅读器不识别则自然不触发）
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    font = subset.load_font(args.input, opts)
    subs = subset.Subsetter(options=opts)
    subs.populate(text=text)
    subs.subset(font)
    rename(font)
    font.save(args.output)

    out = TTFont(args.output)
    cmap = out.getBestCmap()
    missing = [c for c in set(text) if ord(c) not in cmap]
    print(f"glyphs: {out['maxp'].numGlyphs}, cmap entries: {len(cmap)}")
    if missing:
        print(f"note: {len(missing)} chars not in source font: {''.join(missing[:20])}…")


if __name__ == "__main__":
    main()
