#!/usr/bin/env bash
# epubcheck 验收门（docs/architecture/50-writer.md「验收」）：
# 对全部 golden 样书跑 epubcheck，全绿 = 可合入。
#
# 用法：
#   scripts/epubcheck.sh
#   EPUBCHECK_JAR=/path/to/epubcheck.jar scripts/epubcheck.sh
#
# epubcheck jar 缺失时自动下载到 target/epubcheck（不进仓库）。
# 依赖：java 8+。
set -euo pipefail
cd "$(dirname "$0")/.."

JAR="${EPUBCHECK_JAR:-target/epubcheck/epubcheck-5.2.1/epubcheck.jar}"

if [ ! -f "$JAR" ]; then
  echo "==> downloading epubcheck 5.2.1 to target/epubcheck ..."
  mkdir -p target/epubcheck
  curl -sL -o target/epubcheck/epubcheck.zip \
    https://github.com/w3c/epubcheck/releases/download/v5.2.1/epubcheck-5.2.1.zip
  unzip -oq target/epubcheck/epubcheck.zip -d target/epubcheck
fi

status=0
for epub in crates/trans-epub-core/tests/golden/*.epub; do
  echo "==> epubcheck: $epub"
  if ! java -jar "$JAR" "$epub"; then
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "==> epubcheck: ALL GREEN"
else
  echo "==> epubcheck: FAILURES (见上方输出)"
fi
exit $status
