# TransEpub Mono（Maple Mono CN 预子集）溯源

- 源字体：Maple Mono CN Regular（unhinted），https://github.com/subframe7536/maple-font
- 源版本：release v7.9，zip = MapleMono-CN-unhinted.zip，sha256
  `d41cb72721e99cfe4fbd1a7b0f182a013457de46aa612018f924dd024699d3b9`（官方 .sha256 一致）
- 修改（OFL-1.1 修改版义务）：子集化为《通用规范汉字表》8105 字 + 编程文档
  常用字符（ASCII/中西标点/制表符/箭头/希腊）；族名由 "Maple Mono CN" 改为
  "TransEpub Mono"（上游未声明 RFN，改名系保守合规）。再生成命令见
  `scripts/gen-font-subset.py`（uv run --with fonttools）。
- 许可：SIL OFL 1.1，全文见同目录 OFL-1.1.txt（含上游版权行与修改声明）。
  内嵌出书时两者随书分发（writer 的字体许可页）。
- 遗留：8105 字中 281 个扩展区生僻字源字体未覆盖，按阅读器回退字体渲染。
