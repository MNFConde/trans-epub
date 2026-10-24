---
type: research
status: active
summary: "trans-epub 立项前生态调研（2026-10-10）：现有 md/文档站→EPUB 工具无一同时具备 mdBook 目录语义+构建期代码高亮+代码长行处理；mdbook-epub / mdbook-pandoc / percollate / calibre 竞品明细与机会窗口论证"
tags: [trans-epub, research, ecosystem, competitive-analysis, epub]
contains: [research, conclusion]
created: 2026-10-24
updated: 2026-10-24
related: ["../requirements.md", "../adr/0008-epub3-only-no-kindle.md"]
authoring_mode: ai_generated
---

# 立项前生态调研：md/文档站 → EPUB 工具（2026-10-10）

> 调研方式：后台调研代理实访官方页 / GitHub / lib.rs。本笔记回答"**为什么值得做这个项目**"；需求与验收口径见 [../requirements.md](../requirements.md)。

## 核心结论

现有工具**没有一个同时具备**：mdBook 目录语义 + 构建期代码高亮 + 代码长行处理。机会窗口经调研确认真实存在。

## 竞品明细

| 工具 | 版本/状态 | 关键缺陷 |
|---|---|---|
| mdbook-epub | 0.5.4，单人维护，自述 experimental | 嵌套章节拍平；raw HTML 当 XML 处理出错；无 MathJax |
| mdbook-pandoc | 0.11.1，活跃 | EPUB 路径 best-effort；丢 class CSS（高亮尽失） |
| percollate | 4.3，活跃 | Readability 剥离 class → 代码高亮颜色全失 |
| calibre / ebook-convert | 成熟 | 黑盒；成书质量完全取决于输入 |

## 对差异化定位的支撑

trans-epub 的"编程文档保真"三件套（构建期高亮 / 代码长行处理 / mdBook 语义）恰好逐条命中上表空白：

- percollate 丢 class 的根因分析，直接催生了 web 前端"**代码块 language class 必须存活**"的硬性要求（architecture/30-frontends.md）。
- mdbook-epub 的拍平与 raw-HTML 问题，印证了"吃 src/ 原始文件自解析 SUMMARY"路线的必要性（ADR-0005）。
- send-to-kindle 转换丢 CSS 的确认，支撑了"EPUB3 唯一输出、不做 Kindle 适配"的取舍（ADR-0008）。

## 关联

Rust 生态 headless 浏览器绑带的评估（thirtyfour 尚可、chromiumoxide 维护放缓、playwright crate 滞后）记录于 [../requirements.md](../requirements.md) 可行性一节——它与"仅静态站"的范围决策共同排除了 SPA 路线。
