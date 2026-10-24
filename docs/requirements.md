---
type: requirements
status: active
summary: "trans-epub 需求基线 v1.0：定位、MVP1/2/3 范围与验收口径、公共输出层、使用模式与失败策略、非目标、全局默认值，以及 2026-10-10 可行性评估时点快照（依赖健康度/难度分级/工作量配比/风险对策）；验收 = epubcheck + golden 样书 + 真机抽验（Koodo Reader 为主力）"
tags: [trans-epub, requirements, mvp, acceptance, feasibility]
contains: [requirement, conclusion]
created: 2026-10-24
updated: 2026-10-24
related: ["architecture/00-overview.md", "adr/README.md", "research/ecosystem.md"]
authoring_mode: ai_generated
---

# trans-epub 需求基线（v1.0）

> 定稿于 2026-10-10 grill 结构化采访，2026-10-24 自原《mdepub 需求定稿》拆分而来。
> 文档分工：本文回答"**做什么、做到什么程度算合格**"；为什么这样定见 [adr/](adr/README.md)；怎么做见 [architecture/](architecture/00-overview.md)；接下来干什么见根目录 `todo.todo`。

## 定位

一站式 md/文档站 → EPUB 转换工具：一条命令吃**散装 markdown / mdBook 项目 / 静态文档站**三种来源，成书在现代 EPUB3 阅读器上真正可读。差异化 = **编程文档保真**：构建期高亮、代码长行处理、mdBook 语义、链接改写。

形态演进：core crate（管线 + 三前端 + 写出器）→ CLI（一期）→ 简单 GUI（二期）→ 网页服务（三期，独立仓库，axum 包 core）。

## MVP1 · 散装 markdown（地基）

**范围**

- 输入：单文件 / 目录 / 目录树。
- 结构：自动探测 + TOML 声明覆盖（声明优先于探测）。探测清单：① 目录树即章节树（子目录=章分组、文件名自然排序）；② 数字前缀排序（`01-intro.md`）；③ 识别出 `SUMMARY.md`/`book.toml` 自动切换 mdBook 前端；④ `README.md`/`index.md` 上提为章首页；⑤ front matter 的 `order`/`title` 字段；⑥ 纯平铺按文件名排序。
- markdown 方言：GFM 基线（表格/删除线/代码块）+ front matter + 脚注 + admonitions（GitHub `> [!NOTE]` 风格，配 EPUB 醒目色块样式）+ 任务列表。

**保真三件套（必须做好）**

1. 服务端语法高亮（构建期染色，不依赖阅读器）；
2. 远程图片下载嵌入；
3. 内部链接→书内跳转、外部链接保留可点。

**验收口径**：公式保留源文本原样、表格尽力——降级标准是**不炸即可**（完美渲染是第一期非目标）。

## MVP2 · mdBook 前端

- 吃 `src/` 原始文件，自解析 `SUMMARY.md` 顺序与嵌套，**不拍平层级**。
- 指令展开：`{{#include}}`、`{{#rustdoc_include}}`（含行号锚点语法）、`{{#playground}}`、`{{#hide}}` 行隐藏、drafts/hidden 章节。
- 验收口径：语义对齐 mdbook 本体行为。

## MVP3 · 网站前端（仅静态/预渲染站）

- 页面发现三机制全做：URL 列表文件（精确控序）、起始 URL + 同域递归（深度/URL 模式可配）、`sitemap.xml`。**页面顺序是网站成书的质量命门**。
- 正文抽取：选择器 + 内置预设——默认依次尝试 `main`/`article`/`.content`/`markdown-body`，每站可用 TOML 覆盖；内置 mkdocs/docusaurus/vitepress/gitbook 预设。**代码块 language class 必须存活**（percollate 丢高亮的根因）。
- 合规内置：遵守 robots.txt、限速、面向个人学习用途的文档化声明。
- CLI `--dry-run`：只输出探测到的页面清单与顺序，不出书。
- GUI 预览（二期，用户明确要求）：结构清单可见、检查多抓/漏抓、可勾选剔除调整后出书。

## 公共输出层

- EPUB 3；以现代 EPUB3 阅读器为基准（Apple Books / KOReader / 手机 app），**不为 Kindle 做牺牲性适配**。
- 代码长行：默认 CSS `pre-wrap` 按屏宽自适应折行；提供可选构建期硬折行模式（列宽可配）保底。
- 主题：中性亮色 CSS + `prefers-color-scheme` 暗色适配 + 用户自定义 CSS 叠加。
- 章节粒度一文件/一页一章；目录全嵌套；封面可选（用户供图）。

## 使用模式与失败策略

一次性全量转换，无缓存无增量，重跑即重来。批量中单页失败不中止，记 warning 继续、结尾汇总报告。

## 非目标（Non-goals）

SPA/纯客户端渲染站（headless 浏览器）· 登录/付费内容 · Kindle 专项适配 · 数学公式完美渲染（第一期）· 增量同步 · DRM · EPUB 以外的输出格式（mobi/PDF）。

## 全局默认值与验收标准

- 元数据优先级：TOML 配置 < front matter < CLI flags。
- **验收 = epubcheck 通过 + 2~3 本 golden 样书回归 + 真机抽验**（主力 Koodo Reader，Apple Books / KOReader 各一抽验）。
- 质量标准：先自用，但分层不做死、保留开源可能。
- 覆盖层：纯 TOML 声明式，脚本钩子只留架构接缝（第一期不嵌 Rhai/Lua）。

## 可行性评估（2026-10-10 时点快照）

> 本节为立项时点的评估要点，原样保留作快照；其中"链接改写引擎接口先行设计"一条已于 2026-10-24 在 architecture/10-contracts.md 落地。

**结论：可行，无硬技术风险，属"工作量密集型"而非"技术攻关型"。** 核心依赖全部活跃健康：comrak（GFM，GitHub 在用）、epub-builder 0.8.3（2026-04）、syntect、scraper、reqwest 均在维护中。调研指出的 Rust 生态唯一短板（headless 浏览器绑定不成熟：thirtyfour 尚可、chromiumoxide 维护放缓、playwright crate 滞后）恰好被"仅静态站"的范围决策排除。

| 模块 | 难度 | 说明 |
|---|---|---|
| markdown 管线 + 高亮 | 低 | comrak + syntect，方言扩展全有原生支持 |
| EPUB 写出器 + CSS 主题 | 低-中 | epub-builder 偏底层不带给定模板——CSS 主题恰是差异化所在 |
| TOML 结构探测/覆盖层 | 低-中 | 纯逻辑，golden 测试好写 |
| mdBook 前端 | 中 | 指令语义对齐（rustdoc_include 行号锚点等）是细心活 |
| 网站前端 | 中 | 静态站无硬骨头；长尾在任意网页脏 HTML 的边角案例 |
| 链接改写引擎 | 中 | 三前端共享的核心资产，接口先行设计 |

- **工作量分期**（相对量级）：MVP1 ≈ 40%；MVP2 ≈ +15%；MVP3 ≈ +25%；GUI ≈ +10%；之后为不定量的打磨长尾。业余节奏下 MVP1 数周可出第一本可读的书。
- **风险与对策**：① 脏 HTML 长尾 → golden 样书回归 + 选择器预设兜底；② EPUB 阅读器 CSS 兼容性 → epubcheck + 真机验证清单（Koodo/Apple Books/KOReader 抽验）；③ 未来若要做 SPA → 预留"外置渲染器"接缝（外部工具出 HTML 喂公共管线），现在不付 headless 成本。

## Open Questions（已随拆分转为行动项）

| 遗留问题 | 去向 |
|---|---|
| crates.io 上名称可用性 | todo.todo（发布前确认 trans-epub 系列） |
| GUI 框架选型（Tauri 复用 core vs egui 纯 Rust） | 二期开工时决策并立 ADR |
| 内置文档站预设第一批站点清单与优先级 | MVP3 开工时按实际抓取目标定 |
| SPA 外置渲染器接缝的具体契约 | MVP3 稳定后设计 |
