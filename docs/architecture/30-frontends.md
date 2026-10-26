---
type: architecture
status: active
summary: "trans-epub 三前端（均已落地）：md（结构探测六规则+声明覆盖+comrak 方言）、mdbook（SUMMARY 自解析不拍平+指令展开表）、web（Pages 顺序即书序+选择器预设+language class 存活+编码链）；前端选择协调与各前端的 golden fixtures 要求"
tags: [trans-epub, architecture, frontend, md, mdbook, web, comrak]
contains: [design]
created: 2026-10-24
updated: 2026-10-26
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
- front matter 的 `title` 优先于文件名做章名；单文件书 / 根 README 的 FM title 兜底书名（优先于配置文件的 book.title）。目录书无 FM 书名时次级派生**根 README 首个 h1 文本**（MVP1 真机观察 2，2026-10 落地），仍无则 "Untitled"。

## mdbook 前端（MVP2，已落地）

**路线（需求定稿决策）**：吃 `src/` 原始文件，自解析 `SUMMARY.md`，不拍平层级——不吃 build 产物 HTML（那里代码未染色、语义已损）。

### probe / 结构

- 识别：`src/SUMMARY.md`（宽松：book.toml 缺省也认；src 根默认 `src`、可被 `[book].src` 覆盖；src 根无 SUMMARY 时回退根 `SUMMARY.md`；book.toml 单独存在不命中——mdBook 本体没 SUMMARY 也构建不了）。
- SUMMARY 的 link 文本做章名（优先于文件内 h1，h1 保留在正文）；嵌套列表 → toc 嵌套；任意级别 heading → `TocItem.chapter = None` 的 part 分组节点（首个 "Summary" 惯例标题忽略）；`---` 分隔线忽略；无链接叶子列表项 = draft（跳过 + warning；`skip_drafts = false` 时保留为分组节点）；无链接但带子列表的项 = 子分组标题（mdBook 语义，不是 draft）；顶层裸链接（prefix/suffix chapter）为顶层章，不卷进上一个 part。
- SUMMARY 未列出的文件（hidden 章）不进书、不告警——mdBook 本就只构建 SUMMARY 所列内容。
- `book.toml` 里可收割的元数据（title/authors/language/description）属**源内元数据**，优先于 `BookMetaCfg` 配置兜底（10-contracts.md 元数据优先级）。坏 TOML → warning + 按无 book.toml 处理。

### load：指令展开（对齐 mdbook 语义）

| 指令 | 行为 | 备注 |
|---|---|---|
| `{{#include path}}` | 文件内容内联 | 区间语法（1-based 含端点）：`:N` = 第 N 行到文件尾、`:N:M` = N..=M、`:name` = `// ANCHOR: name` … `// ANCHOR_END: name` 之间（剥锚点行）；区间端点亦可为锚点名 |
| `{{#rustdoc_include path:anchor}}` | 内联并生成行号锚点注释（`// path:N-M`） | anchor 形式 `struct:Foo` / `lines:10-20`（item 定义行到闭合 `}` 行） |
| `{{#playground path}}` | 首期折为代码块 + 指向原文件的相对链接 | `playground_embed = true` 则纯内嵌；链接按统一断链语义降级 |
| `{{#hide}}` | 标记行不进产物 | 行级处理（删除所在行） |
| draft / hidden 章节 | 跳过 | draft 记 warning；hidden 静默 |
| 其他指令（`{{#if}}` 等） | `DirectiveUnresolved` warning + 原文保留 | 保守可见，不做静默丢弃 |

include 的 path 以 src 根（SUMMARY 所在目录）为基经 RelPath 校验（防穿越）；文件缺失 / 区间落空 → warning + 原文保留；展开不递归（目标内的指令保留原文，与 mdBook 一致）。指令是 markdown 渲染前的**文本级替换**——`{{#include}}` 落在作者已写好的 fenced code block 里，语言由外层 fence 决定。

## web 前端：静态文档站（MVP3，已落地）

### 输入约定

- 吃 `Source::Pages`：**顺序即书序**，质量责任在采集方（fetch crate / 服务）。
- 重复 URL → `DuplicateSource` warning，保留首个。
- 非页面资源（content_type 非 text/html）→ warning 跳过。

### load：正文抽取

- **选择器链**（取链内首个命中选择器的首个匹配）：`WebCfg.selectors` 覆盖 > `WebCfg.preset` 内置链 > 默认链。内置预设链：mkdocs = `.md-content → article → main`；docusaurus = `article → main`；vitepress = `.vp-doc → main → article`；gitbook = `article → .markdown-body → main`。默认链 `main → article → .content → .markdown-body → body` 兜底。未知预设名 / 坏选择器 → `StructureAmbiguous` warning + 回落默认链；全链落空 → body 兜底。
- **适配分界（口径，评审红线）**：生成器级共性 → 内置预设（代码，个位数、跨站稳定）；站点级个性 → `WebCfg.selectors` / `strip`（配置，数据）。不把站点特例内化进 web.rs。预设链与清理规则是 CLI 与三期服务共用的单一事实源，同一份站点适配配置两种入口通用。
- **硬要求：代码块的 language class 必须存活**（`<pre><code class="language-rust">`）。这是 percollate 丢高亮的根因教训——抽取器宁可保守（保留整个代码块节点）也不做激进清洗。语言不在语法集时静默回退纯文本、类名保留（语法集为 syntect-assets 扩展 dump，覆盖 TypeScript/TSX/JSX/TOML 等，见 20-core.md highlight pass）。
- 清理规则：内置剔除 `nav / footer / aside / script / style / noscript / form / button` 加**预设级补充**（mkdocs：`.md-content__button` 编辑/查看源按钮，2026-10 实抓发现）加 `WebCfg.strip`；正文经 **XML 安全序列化**直接产出（script/style/注释从 load 起就不进章 HTML）。
- **引用保持原始形式**（义务清单第 3 条），登记不改写：`img/source` 的 src 以**页面 URL 为基**规范化为绝对 URL 后登记（http(s) → `AssetData::Remote`、`data:` → 解码 Owned、纯锚点 src 不登记）；`a[href]` 原样留给 link-rewrite 的 Url 世界解析。
- 编码：content_type 的 charset 优先，其次 HTML meta 嗅探（头 2048 字节，含 http-equiv 形态），兜底 UTF-8；BOM 优先（encoding_rs 行为）；有损转换记 `EncodingLossy` warning。
- 章名：抽取后正文首个 h1 → 无则 `<title>` → 无则 URL path 末段（percent-decoded，空则 host）。书名：配置 > 第一页 `<title>` > "Untitled"。封面仅接受 URL / data: URI（Pages 世界无 Vfs，相对路径记 warning）。
- heading 缺 id 的补 GitHub 式 slug（与另两个前端同一算法，intra-page 锚点依赖它）。

### probe

对 Pages 源，outline 就是页面列表本身 + 轻量标题抽取（与 load 同一回退链，但不做资产登记/清理等正文转换）；kind = `web`。Vfs 源不命中。

### link-rewrite 的 Url 世界（配合条款）

Url 世界章节的 `a[href]` 解析：相对引用以页面 URL join 成绝对 URL；**同源**（scheme+authority 一致）绝对 http(s) 链接也算书内候选——文档站内链常以绝对 URL 书写；`//host` 协议相对补全后同判。命中章节/资产照常改写；**未入书的同源链接绝对化保留**（站外仍可点）+ `LinkUnresolved` warning，不像 Path 世界那样降级纯文本（悬空相对引用会过不了 epubcheck，绝对化两全）。跨源与 mailto: 等原样保留。

## 前端选择协调

注册顺序即优先级：**mdbook → md → web**（默认注册序全量落地）。mdbook 先探（发现 book.toml/SUMMARY.md 即接管，散装 md 规则不会误吃 mdBook 项目）；web 只认 Pages 源，md / mdbook 只认 Vfs 源，互不冲突。多命中属正常（散装 md 目录同时也是"一堆 md 文件"），取第一个，其余作为 alternatives 呈现给壳层。

## golden fixtures 要求

| 前端 | 必备 fixture |
|---|---|
| md | 纯平铺排序、数字前缀、嵌套目录树、README 上提、front matter order/title、含表格/脚注/admonition/任务列表的方言样本、本地+远程+data: 图片 |
| mdbook | 嵌套 SUMMARY（≥3 层）、part 标题、draft 章、include 区间语法、rustdoc_include 锚点、hide 行、缺失 include 目标 |
| web | 四个预设各一组样本页（`fixtures/web/presets/`）+ 脏 HTML 边角集（golden 样书 `web`：缺 main/article 的 `.markdown-body` 页、代码块无 language class、相对/绝对/同源绝对混合链接、非 UTF-8 编码页、重复 URL、非 HTML 资源） |
