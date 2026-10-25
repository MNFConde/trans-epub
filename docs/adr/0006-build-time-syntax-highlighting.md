---
type: adr
status: accepted
summary: "构建期 syntect 高亮"
tags: [trans-epub, adr]
contains: [decision]
created: 2026-10-10
updated: 2026-10-24
related: ["../architecture/20-core.md", "../architecture/50-writer.md", "../research/ecosystem.md"]
authoring_mode: ai_generated
---

# 0006. 构建期 syntect 高亮

- 状态：accepted
- 日期：2026-10-10（2026-10-24 回填为 ADR）
- 相关：architecture/20-core.md（highlight pass）、architecture/50-writer.md、research/ecosystem.md

## 背景

阅读器端没有可靠的高亮能力；调研中 percollate 的教训表明即便保住 class，阅读器也不会染色。代码高亮是"编程文档保真"差异化的第一支柱。

## 决策

syntect 在**构建期**把代码染成 span + CSS 内嵌进 EPUB，不依赖阅读器；CSS 亮/暗两套按 `prefers-color-scheme` 切换。三种来源统一在 pipeline 的 highlight pass 处理（HTML 层同型），前端不做高亮。

## 后果

- 所有现代 EPUB3 阅读器呈现一致；web 来源已染色的代码保留，未染色（裸 `pre > code.language-x`）的统一补染。
- 代价：EPUB 体积增加、代码块 HTML 变重。
