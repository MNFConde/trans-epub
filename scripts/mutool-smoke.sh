#!/usr/bin/env bash
# mutool 渲染冒烟门（docs/testing/manual-checklist.md「阅读器优先级」的引擎口径）：
# 对全部 golden 样书用 MuPDF 引擎渲染前若干页，验证「可解析 + 页面非空白」。
# 只验可渲染性，不验视觉保真——MuPDF 的 CSS 支持远低于浏览器，视觉结论以真机抽验为准。
#
# 用法：
#   scripts/mutool-smoke.sh                        # golden 5 本
#   scripts/mutool-smoke.sh book1.epub book2.epub  # 指定书目
#   MUTOOL_SMOKE_PAGES=20 scripts/mutool-smoke.sh  # 渲染前 20 页（默认 10）
#
# 依赖：mutool（Debian/Ubuntu: apt install mupdf-tools）。
# 未安装时 SKIP 退出 0——本地缺 mutool 不挡门禁；CI 侧先安装再调用即全量生效。
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v mutool >/dev/null 2>&1; then
  echo "==> mutool-smoke: SKIP (mutool 未安装；Debian/Ubuntu: apt install mupdf-tools)"
  exit 0
fi

PAGES="${MUTOOL_SMOKE_PAGES:-10}"
# 非空白阈值：非纯白字节占文件比例（PPM 白像素为 0xff×3，头部文本仅 ~20 字节；
# 正文文本/代码块底色/边框的像素占比远超此值，抗锯齿灰像素亦计入）
THRESHOLD="${MUTOOL_SMOKE_BLANK_RATIO:-0.001}"

if [ $# -gt 0 ]; then
  books=("$@")
else
  books=(crates/trans-epub-core/tests/golden/*.epub)
fi

status=0
for epub in "${books[@]}"; do
  echo "==> mutool-smoke: $epub"
  tmp="$(mktemp -d)"
  # 页范围超界时 mutool 自动 clamp 到实际页数（实测 1-99 对 3 页书退出码 0）
  if ! mutool draw -q -F ppm -o "$tmp/p%d.ppm" "$epub" "1-$PAGES" 2>"$tmp/stderr"; then
    echo "    FAIL: mutool draw 退出非 0"
    sed 's/^/      /' "$tmp/stderr"
    rm -rf "$tmp"
    status=1
    continue
  fi
  rendered=("$tmp"/p*.ppm)
  if [ ! -f "${rendered[0]}" ]; then
    echo "    FAIL: 未渲染出任何页面"
    rm -rf "$tmp"
    status=1
    continue
  fi
  for ppm in "${rendered[@]}"; do
    total=$(wc -c <"$ppm")
    nonwhite=$(tr -d '\377' <"$ppm" | wc -c)
    ratio=$(awk -v a="$nonwhite" -v b="$total" 'BEGIN{printf "%.5f", a/b}')
    page=$(basename "$ppm" .ppm)
    page=${page#p}
    if awk -v r="$ratio" -v t="$THRESHOLD" 'BEGIN{exit !(r > t)}'; then
      echo "    page $page: nonwhite ${ratio} OK"
    else
      echo "    page $page: nonwhite ${ratio} FAIL (≤ $THRESHOLD，疑似空白页)"
      status=1
    fi
  done
  rm -rf "$tmp"
done

if [ "$status" -eq 0 ]; then
  echo "==> mutool-smoke: ALL GREEN"
else
  echo "==> mutool-smoke: FAILURES (见上方输出)"
fi
exit $status
