---
type: architecture
status: active
summary: "trans-epub 三前端设计：md（结构探测六规则+声明覆盖+comrak 方言）、mdbook（SUMMARY 自解析不拍平+指令展开表）、web（Pages 顺序即书序+选择器预设+language class 存活）；前端选择协调与各前端的 golden fixtures 要求"
tags: [trans-epub, architecture, frontend, md, mdbook, web, comrak]
contains: [design]
created: 2026-10-24
updated: 2026-10-24
related: ["../requirements.md", "10-contracts.md", "20-core.md"]
authoring_mode: ai_generated
---

# 源归一前端（md / mdbook / web）

## 总则

前端的职责一句话：**把一种源格式归一为 BookDraft**。它们是契约里"前端义务清单"（10-contracts.md）的实现者——干净 HTML、heading id、引用保原样、顺序定死、单项失败不中止、不做通用 HTML 处理。

共同的失败哲学：探测拿不准 → `StructureAmbiguous` warning + 默认规则兜底（继续，不中止）；文件读不了/页抽不出 → warning + 跳过该项。

## md 前端：散装 markdown（MVP1）

### probe：结构探测

probe 只看文件名与目录形态（廉价），规则按优先级：

1. **显式声明覆盖一切**：`StructureCfg` 里的章节声明（有序路径列表 / 声明式 TOC）存在时，探测结果就是声明本身。
2. `book.toml` 或 `SUMMARY.md` 存在 → 不命中（让位 mdbook，由注册顺序保证）。
3. front matter 的 `order` / `title` 字段参与排序与命名。
4. 数字前缀 `NN-name.md` → 剥前缀排序、前缀剥离后的名字做标题。
5. `README.md` / `index.md` 上提为所在目录层级的首页。
6. 目录树嵌套 → TocItem 嵌套；纯平铺 → 文件名自然排序。

默认排除：`.` 开头的文件/目录；`StructureCfg.include/exclude` 可覆盖。歧义情形（如同时有数字前缀和 front matter order）按上表优先级取高者并记 warning。

### load：归一化

- **comrak 方言**：GFM 基线（表格/删除线/任务列表）+ front matter 解析（TOML，`title` / `order` 两字段；`---`…`---` 包裹，解析失败保守当普通内容）+ 脚注 + admonition（GitHub alerts 经 comrak `AlertStyleType::Semantic` 渲染为 `<aside class="admonition note">`（类名为 note/tip/important/warning/caution），样式由 writer 的 admonition.css 负责）。
- heading id 生成（义务清单第 2 条）：comrak 渲染后经 DOM 后处理补 id——用 comrak 的 `Anchorizer`（GitHub 式 slug + 去重，与 link-rewrite 的锚点归一同算法）；不启用 comrak 的 `header_id_prefix`（它会往每个 heading 里塞自引用锚链接）。
- 章节来源：`README.md`/`index.md` 上提为所在目录层级的首页（分组 TocItem 直接指向它）、其余 md 一文件一章；目录嵌套映射为 toc 嵌套（不拍平）。
- **资产登记**：本地图片/媒体（相对路径，以章节路径为基解析）→ 由 Vfs 读入为 `AssetData::Owned`（本地读取发生在 load——pass 不再读 Vfs）；`http(s)` 图片 → `AssetData::Remote(url)`（下载归 asset-resolve pass，前端不抓）；`data:` URI → Owned（解码）。非 md 文件不进章节。
- front matter 的 `title` 优先于文件名做章名；单文件书 / 根 README 的 FM title 兜底书名（优先于配置文件的 book.title）。

## mdbook 前端（MVP2）

**路线（需求定稿决策）**：吃 `src/` 原始文件，自解析 `SUMMARY.md`，不拍平层级——不吃 build 产物 HTML（那里代码未染色、语义已损）。

### probe / 结构

- 识别：`book.toml` + `src/SUMMARY.md`（宽松：有 SUMMARY.md 即可，book.toml 缺省也认）。
- SUMMARY 的 link 文本做章名（优先于文件内 h1）；嵌套列表 → toc 嵌套；`# Part 标题` → `TocItem.chapter = None` 的分组节点；`---` 分隔线忽略；draft 章（无链接或标记）跳过并 warning。
- `book.toml` 里可收割的元数据（title/authors/language）进 BookMeta 兜底值。

### load：指令展开（对齐 mdbook 语义）

| 指令 | 行为 | 备注 |
|---|---|---|
| `{{#include path}}` | 文件内容内联 | 支持 mdbook 的区间语法：`:10`、`:10:20`、`:regex` |
| `{{#rustdoc_include path:anchor}}` | 内联并生成行号锚点注释 | anchor 形式 `struct:Foo` / `lines:10-20` |
| `{{#playground path}}` | 首期折为代码块 + 指向原文件的链接 | 语义对齐 mdbook 的可编辑体验取舍 |
| `{{#hide}}` | 标记行不进产物 | 行级处理 |
| draft / hidden 章节 | 跳过 | |
| 其他指令（`{{#if}}` 等） | `DirectiveUnresolved` warning + 原文保留 | 保守可见，不做静默丢弃 |

include 的 path 以 `src/` 为根经 RelPath 校验（防穿越）；文件缺失 → warning + 原文保留。

## web 前端：静态文档站（MVP3）

### 输入约定

- 吃 `Source::Pages`：**顺序即书序**，质量责任在采集方（fetch crate / 服务）。
- 重复 URL → `DuplicateSource` warning，保留首个。
- 非页面资源（content_type 非 text/html）→ warning 跳过。

### load：正文抽取

- **选择器策略**：`WebCfg.preset`（mkdocs / docusaurus / vitepress / gitbook 内置预设）或 `WebCfg.selectors` 覆盖；都未配置时按默认链依次尝试 `main` → `article` → `.content` → `.markdown-body` → `body` 兜底。
- **硬要求：代码块的 language class 必须存活**（`<pre><code class="language-rust">`）。这是 percollate 丢高亮的根因教训——抽取器宁可保守（保留整个代码块节点）也不做激进清洗。
- 清理规则（可配）：剔除 nav/footer/aside/script/style/noscript/form/button；保留语义内容节点。
- 相对 URL 以**页面 URL 为基**规范化为绝对 URL 后登记：图片 → `AssetData::Remote`；`data:` URI → Owned；`a[href]` 保持绝对形式等 link-rewrite 处理。
- 编码：content_type 的 charset 优先，其次 HTML meta 嗅探，兜底 UTF-8；有损转换记 `EncodingLossy` warning。
- 章名：抽取后正文首个 h1 → 无则 `<title>` → 无则 URL path 末段。

### probe

对 Pages 源，outline 就是页面列表本身 + 轻量标题抽取（不做正文转换）。预设识别（根据 HTML 特征猜测站点类型）在此阶段给出建议。

## 前端选择协调

注册顺序即优先级：**mdbook → md → web**。mdbook 先探（发现 book.toml/SUMMARY.md 即接管，散装 md 规则不会误吃 mdBook 项目）；web 只认 Pages 源，与另两者无冲突。多命中属正常（散装 md 目录同时也是"一堆 md 文件"），取第一个，其余作为 alternatives 呈现给壳层。

## golden fixtures 要求

| 前端 | 必备 fixture |
|---|---|
| md | 纯平铺排序、数字前缀、嵌套目录树、README 上提、front matter order/title、含表格/脚注/admonition/任务列表的方言样本、本地+远程+data: 图片 |
| mdbook | 嵌套 SUMMARY（≥3 层）、part 标题、draft 章、include 区间语法、rustdoc_include 锚点、hide 行、缺失 include 目标 |
| web | 四个预设各一组样本页 + 脏 HTML 边角集（缺 main/article、代码块无 language class、相对/绝对混合链接、非 UTF-8 编码、重复 URL） |
