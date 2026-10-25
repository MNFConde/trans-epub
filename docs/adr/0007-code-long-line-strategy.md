---
type: adr
status: accepted
summary: "代码长行：pre-wrap 默认 + 可选构建期硬折行"
tags: [trans-epub, adr]
contains: [decision]
created: 2026-10-10
updated: 2026-10-24
related: ["../architecture/20-core.md", "../architecture/50-writer.md"]
authoring_mode: ai_generated
---

# 0007. 代码长行：pre-wrap 默认 + 可选构建期硬折行

- 状态：accepted
- 日期：2026-10-10（2026-10-24 回填为 ADR）
- 相关：architecture/20-core.md（code-wrap pass）、architecture/50-writer.md

## 背景

代码长行处理是编程书成书质量的核心痛点；阅读器对 CSS `pre-wrap` 的支持参差不齐，而构建期硬折行会破坏从书里复制代码的完整性。

## 决策

默认 CSS `pre-wrap` 按屏宽自适应折行（零内容改写）；提供可选的构建期**硬折行**模式（列宽可配）作为保底，专治不理 pre-wrap 的阅读器。硬折行由 code-wrap pass 完成（必须在 highlight 之后），writer 只管 CSS、不改内容。

## 后果

- 默认路径不损伤代码可复制性；硬折行是内容级变换，由用户显式开启并自担后果。
