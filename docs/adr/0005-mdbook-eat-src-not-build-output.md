# 0005. mdBook 吃 src/ 原始文件而非 build 产物

- 状态：accepted
- 日期：2026-10-10（2026-10-24 回填为 ADR）
- 相关：architecture/30-frontends.md、research/ecosystem.md

## 背景

mdbook 的高亮是浏览器端 highlight.js，build 产物里代码未染色——吃 build HTML 省不了高亮工作，反而丢掉 SUMMARY.md 的目录语义。mdbook-epub 的嵌套拍平、raw HTML 当 XML 处理出错等问题，印证没有现成轮子可拿。

## 决策

吃 `src/` 原始文件，自解析 `SUMMARY.md` 的顺序与嵌套，**不拍平层级**；指令（`{{#include}}`/`{{#rustdoc_include}}`/`{{#playground}}`/`{{#hide}}`、draft/hidden 章节）语义对齐 mdbook 本体。

## 后果

- 指令语义对齐是细心活（模块难度评级：中），换来目录语义与高亮的完全控制权。
- 不依赖用户本地装有 mdbook 工具链。
