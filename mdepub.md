---
type: project_topic
status: active
summary: "mdepub 需求定稿（v1.0）：一站式 mdBook/散装 markdown/静态文档站 → EPUB 转换工具，Rust CLI 起步，'编程文档保真'（构建期高亮/长代码行/mdBook 语义/链接改写）为差异化定位；含 grill 决策全记录、分期路线与可行性评估（依赖全活跃，无硬技术风险）"
tags: [mdepub, epub, markdown, mdbook, 转换工具, rust, 需求, 可行性]
contains: [decision, conclusion]
created: 2026-10-10
updated: 2026-10-10
related: ["epub-translation-workflow.md", "ebook-reader-selection.md"]
authoring_mode: ai_generated
---

# mdepub：md/网站 → EPUB 转换工具（需求定稿 v1.0）

## 形成背景

2026-10-10 grill-me 结构化采访定稿：两轮共 14 个决策点 + 一组默认值清单，用户逐项确认，项目定名 **mdepub**。

- **与既有知识的衔接**：阅读器选型（Koodo Reader 为主力，Windows 为主手机续读加分）对 EPUB 成书质量敏感；`epub-translate/` 是 EPUB 的"消费/加工"侧，mdepub 是"生产"侧，互为姊妹项目。
- **立项前生态调研**（2026-10-10 后台调研代理，来源为官方页/GitHub/lib.rs 实访）：现有工具**没有一个同时具备** mdBook 目录语义 + 构建期代码高亮 + 代码长行处理。mdbook-epub 0.5.4 单人维护、自述 experimental（嵌套章节拍平、raw HTML 当 XML 处理出错、无 MathJax）；mdbook-pandoc 0.11.1 活跃但 EPUB 路径 best-effort 且丢 class CSS；percollate 4.3 活跃但 Readability 剥离 class 导致代码高亮颜色全失；calibre/ebook-convert 强大但黑盒、质量取决于输入。机会窗口经调研确认真实存在。

## 需求定稿（v1.0）

### 定位与架构骨架

- **价值主张**：一站式 + 编程文档保真——一条命令吃散装 markdown / mdBook 项目 / 静态文档站三种来源，成书在现代 EPUB3 阅读器上真正可读。
- **主骨架**：统一 **HTML→EPUB 流水线**。三种来源各有前端，全部归一为"有序章节列表（每章 = 元数据 + 干净 HTML + 资源）"，共用 EPUB 写出器。
- **三形态分层**（对应用户规划）：`core` crate（管线+三前端+写出器）→ CLI bin（第一期）→ 简单 GUI（第二期：结构预览 + 配置 + 出书）→ 网页服务（第三期，axum 包 core）。

### MVP1 · 散装 markdown（地基）

- 输入：单文件 / 目录 / 目录树。**结构自动探测 + TOML 声明覆盖**（声明优先于探测）。
- 探测的组织形式清单：① 目录树即章节树（子目录=章分组、文件名自然排序）；② 数字前缀排序（`01-intro.md`）；③ 识别出 `SUMMARY.md`/`book.toml` 自动切换 mdBook 前端；④ `README.md`/`index.md` 上提为章首页；⑤ front matter 的 `order`/`title` 字段；⑥ 纯平铺按文件名排序。
- markdown 方言：GFM 基线（表格/删除线/代码块）+ front matter + 脚注 + admonitions（GitHub `> [!NOTE]` 风格，配 EPUB 醒目色块样式）+ 任务列表（comrak 下均为低成本）。
- **保真三件套（必须做好）**：服务端语法高亮（syntect，构建期染成 span+CSS 内嵌，不依赖阅读器）；远程图片下载嵌入；内部链接→书内跳转、外部链接保留可点。
- 公式/表格降级为"不炸即可"：公式保留源文本原样，表格尽力。

### MVP2 · mdBook 前端

- **吃 `src/` 原始文件，自解析 `SUMMARY.md` 顺序与嵌套，不拍平层级**（选此路而非吃 build HTML 的依据：mdbook 高亮是浏览器端 highlight.js，build 产物里代码未染色，吃 HTML 省不了高亮工作反而丢语义；mdbook-epub 的拍平/HTML bug 也印证无现成轮子可拿）。
- 指令展开：`{{#include}}`、`{{#rustdoc_include}}`（含行号锚点语法）、`{{#playground}}`、`{{#hide}}` 行隐藏、drafts/hidden 章节。语义对齐 mdbook，是细心活而非技术难点。

### MVP3 · 网站前端（仅静态/预渲染站）

- 页面发现三机制全做：URL 列表文件（精确控序）、起始 URL + 同域递归（深度/URL 模式可配）、`sitemap.xml`。页面顺序是网站成书的质量命门。
- 正文抽取：**选择器 + 内置预设**——默认依次尝试常见容器（`main`/`article`/`.content`/`markdown-body`），每站可用 TOML 覆盖；内置 mkdocs/docusaurus/vitepress/gitbook 预设；**代码块 language class 必须存活**（percollate 丢高亮的根因即 Readability 剥 class）。
- 合规内置：遵守 robots.txt、限速、面向个人学习用途的文档化声明。
- CLI `--dry-run`：只输出探测到的页面清单与顺序不出书——GUI 结构预览的雏形。
- GUI 预览需求（第二期，用户明确要求）：能看到自动识别的页面/章节清单，检查多抓/漏抓，可勾选剔除调整后出书。

### 公共输出层

- EPUB 3；以现代 EPUB3 阅读器为基准（Apple Books / KOReader / 手机 app），**不**为 Kindle 做牺牲性适配（send-to-kindle 转换丢 CSS，代码高亮天然脆弱，调研已确认）。
- 代码长行：默认 CSS `pre-wrap` 按屏宽自适应折行；提供可选构建期硬折行模式（列宽可配）保底，专治不理 pre-wrap 的阅读器。
- 主题：中性亮色 CSS + `prefers-color-scheme` 暗色适配 + 用户自定义 CSS 叠加。
- 章节粒度一文件/一页一章；目录全嵌套；封面可选（用户供图）。

### 使用模式与失败策略

一次性全量转换，无缓存无增量，重跑即重来。批量中单页失败不中止，记 warning 继续、结尾汇总报告。

### 非目标（Non-goals）

SPA/纯客户端渲染站（headless 浏览器）· 登录/付费内容 · Kindle 专项适配 · 数学公式完美渲染（第一期）· 增量同步 · DRM · EPUB 以外的输出格式（mobi/PDF）。

### 默认值（确认定稿）

元数据优先级 TOML 配置 < front matter < CLI flags；验收 = epubcheck 通过 + 2~3 本 golden 样书回归；质量标准 = 先自用但分层不做死、保留开源可能；覆盖层 = 纯 TOML 声明式，脚本钩子只留架构接缝（第一期不嵌 Rhai/Lua）。

## 可行性评估（2026-10-10）

**结论：可行，无硬技术风险，属"工作量密集型"而非"技术攻关型"。** 核心依赖全部活跃健康：comrak（GFM，GitHub 在用）、epub-builder 0.8.3（2026-04）、syntect、scraper、reqwest 均在维护中。调研指出的 Rust 生态唯一短板（headless 浏览器绑定不成熟：thirtyfour 尚可、chromiumoxide 维护放缓、playwright crate 滞后）恰好被"仅静态站"的范围决策排除。

| 模块 | 难度 | 说明 |
|---|---|---|
| markdown 管线 + 高亮 | 低 | comrak + syntect，方言扩展全有原生支持 |
| EPUB 写出器 + CSS 主题 | 低-中 | epub-builder 偏底层不带给定模板——CSS 主题恰是差异化所在 |
| TOML 结构探测/覆盖层 | 低-中 | 纯逻辑，golden 测试好写 |
| mdBook 前端 | 中 | 指令语义对齐（rustdoc_include 行号锚点等）是细心活 |
| 网站前端 | 中 | 静态站无硬骨头；**长尾在任意网页脏 HTML 的边角案例** |
| 链接改写引擎 | 中 | 三前端共享的核心资产，**接口先行设计**（源标识规范化→EPUB 锚点） |

- **工作量分期**（相对量级）：MVP1 ≈ 40%；MVP2 ≈ +15%；MVP3 ≈ +25%；GUI ≈ +10%；之后为不定量的打磨长尾。业余节奏下 MVP1 数周可出第一本可读的书。
- **风险与对策**：① 脏 HTML 长尾 → golden 样书回归 + 选择器预设兜底；② EPUB 阅读器 CSS 兼容性 → epubcheck + 真机验证清单（Koodo/Apple Books/KOReader 抽验）；③ 未来若要做 SPA → 预留"外置渲染器"接缝（外部工具出 HTML 喂公共管线），现在不付 headless 成本。

## Open Questions

- crates.io 上 `mdepub` 名称可用性未查（不影响开工，发布前确认）。
- GUI 框架选型（Tauri 复用 core vs egui 纯 Rust）第二期再定。
- 内置文档站预设的第一批站点清单与优先级，待 MVP3 开工时按实际抓取目标定。
- SPA 外置渲染器接缝的具体契约（输入输出格式）MVP3 稳定后再设计。
