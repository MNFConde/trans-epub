---
type: testing
status: active
summary: "trans-epub 真机抽验清单：MVP 里程碑合入前全量抽验；主力 Koodo Reader，Apple Books / KOReader 各一；检查点覆盖代码高亮亮暗切换、长行折行、嵌套目录跳转、图片显示、脚注与 admonition 渲染、封面、mdBook 源结构、web 源抽取与乱码、内嵌字体生效"
tags: [trans-epub, testing, manual-checklist, epub-readers]
contains: [checklist]
created: 2026-10-24
updated: 2026-10-31
related: ["../requirements.md", "../architecture/50-writer.md"]
authoring_mode: ai_generated
---

# 真机抽验清单

验收口径（docs/requirements.md）：epubcheck 全绿 + golden 回归 + **真机抽验**。
epubcheck 只能保证结构合法，"在现代 EPUB3 阅读器上真正可读"必须人眼确认。

## 何时抽验

- MVP 里程碑合入前：全量跑一遍。
- 日常改动：只跑受影响的检查点（改 CSS → 亮暗与排版组；改 writer 结构 → 全量）。
- 阅读器升级出兼容问题时：针对该阅读器复验。
- 自动化前置门：`scripts/mutool-smoke.sh`（MuPDF 引擎渲染非空白，epubcheck 之后的引擎档；只验可渲染性，视觉结论仍以本清单为准）。

## 样书

用 golden fixtures 生成（覆盖所有特性）：

```bash
cargo run -p trans-epub-cli -- crates/trans-epub-core/tests/fixtures/flat -o flat.epub
cargo run -p trans-epub-cli -- crates/trans-epub-core/tests/fixtures/tree -o tree.epub
cargo run -p trans-epub-cli -- crates/trans-epub-core/tests/fixtures/mdbook -o mdbook.epub
```

web 源用 `crawl` 对一个小型静态站现抓现转（例如 `trans-epub crawl https://<小站>/ --preset <预设> -o web.epub`）；离线替代：把 golden 的 web 样书页面存成 `list.txt` 后经 crawl 二次转换。

内嵌字体样书（fonts 检查点用，preset 模式约 2.9MB）：

```bash
cargo run -p trans-epub-cli -- crates/trans-epub-core/tests/fixtures/flat -o flat-fonts.epub --embed-fonts
```

## 检查点

### 代码高亮（核心差异化）

- [ ] Rust 代码块有配色（非纯黑）
- [ ] 阅读器暗色模式下代码块切到暗色主题（`prefers-color-scheme`）
- [ ] 未知语言代码块等宽字体、无配色、内容完整
- [ ] 行内 `code` 与代码块样式区分明显

### 长行折行（ADR-0007）

- [ ] 超长代码行按屏宽折行（pre-wrap），无横向滚动
- [ ] 缩进对齐未被打乱

### 内嵌字体（--embed-fonts 样书）

- [ ] @font-face 加载成功，代码块/行内 code 字体链首名命中内嵌族名（TransEpub Mono）
- [ ] 与未嵌字体书并排对比字形一致（子集覆盖范围内）
- [ ] 书末字体许可页可见（OFL 全文 + 修改声明与溯源）

### 目录与跳转

- [ ] 目录（nav）嵌套层级完整（tree 样书：guide 分组下有子章）
- [ ] 分组节点可点进其 README 章（guide → The Guide）
- [ ] 章内链接跳转到目标章（"link to syntax"）
- [ ] 带锚点链接落到目标标题（"good anchor"）
- [ ] 外部链接保留可点（autolink 的 https://example.com）
- [ ] Begin Reading / Cover 快跳入口（landmarks nav；MuPDF 类老引擎走 OPF guide——本机 mutool 冒烟过可渲染性，入口表现记各引擎）

### 图片与资产

- [ ] 本地图片显示（diagram.png，1×1 透明，应不报错）
- [ ] 远程图片已嵌入并显示（remote.png）
- [ ] 缺失图片位置显示 `[missing image: ghost]` 占位文本
- [ ] 封面：`--cover` 指定后在书架显示封面图

### md 方言

- [ ] 表格渲染为表格
- [ ] 任务列表复选框显示且不歪
- [ ] 脚注引用弹窗（noteref/footnote 语义化后，Kobo/iBooks 类阅读器点引用应弹卡；不支持弹窗的引擎至少可跳转，flat/02-footnotes）
- [ ] 脚注可跳转、可回跳（flat/02-footnotes）
- [ ] admonition 色块（NOTE 蓝 / WARNING 黄）亮暗两套下都可读

### mdBook 源（MVP2，用 mdbook 样书）

- [ ] part 分组在 nav 中显示为纯标题（Part One / Part Two，不可点）
- [ ] 章名取 SUMMARY 链接文本（Chapter 1，而非文件内 h1 差异）
- [ ] `{{#include}}` 展开的代码块完整且染色（hello.rs 锚点区间 / lines.rs 行区间）
- [ ] `{{#rustdoc_include}}` 代码块首行有 `// src/...:N-M` 来源注释
- [ ] `{{#playground}}` 折为代码块 + "playground source" 文字（断链降级，无死链点击）
- [ ] draft 章（Draft chapter）与未列出的 hidden.md 不出现在目录与正文
- [ ] 缺失 include 与未知指令以原文可见（appendix 章内可见指令文本）

### web 源（MVP3，crawl 现抓或 web 样书）

- [ ] 章节顺序 = crawl 发现顺序（dry-run 清单与成书目录一致）
- [ ] 站点 chrome 不残留（导航/侧栏/页脚/按钮不出现在正文）
- [ ] 代码块 language class 存活并染色；未知语言（如 ts）等宽纯文本
- [ ] 相对与同源绝对内链都跳到目标章；未入站页面链接保持站外可点；跨站链接原样
- [ ] 页面图片（含 CDN 跨源）已嵌入显示
- [ ] 非 UTF-8 页面（meta charset）无乱码（web 样书 api 章 "Référence"）
- [ ] 章名合理（正文 h1 优先；无 h1 时页面 title / URL 末段兜底）

### 元数据与基础

- [ ] 书名/作者/语言正确（书架卡片）
- [ ] 章节顺序 = 目录顺序 = 阅读顺序
- [ ] 快速翻章不卡（6 章规模无感即可）

## 阅读器优先级

| 阅读器 | 角色 | 备注 |
|---|---|---|
| Koodo Reader | 主力 | 每次全量；可 CDP 驱动（`scripts/koodo-scan.mjs`，命令行开书 + 翻页收集 + 截图） |
| Apple Books | 抽验 | CSS 兼容性风向标（macOS/iOS）；暂缺设备，首发前补验 |
| KOReader | 抽验 | 嵌入式设备口径（较老的渲染栈）；无 Windows 构建，走 WSL Ubuntu 源码构建（kodev build）+ busted spec 驱动 |

## 阅读器行为观察（累积，随抽验更新）

阅读器主题系统与书内 CSS 的领地之争属行业常态，以下为实测确认的各阅读器行为，抽验结论按下表口径解读：

- **Koodo 2.4.6**（2026-10-26，CDP 驱动）：阅读主题按元素选择器注入背景色，**pre 背景被接管**（白天=阅读背景色，书内 `#f6f8fa` 不生效），但边框/圆角/padding/字体链/折行/行内 code 底色全部生效；`prefers-color-scheme` **不映射**应用主题——黑夜主题下正文文字被阅读器换白，代码块保持白底深字（刺眼但可读），书内暗色 CSS 套不激活。判定：阅读器领地行为，不修产物；视觉区分由边框保底。
- **MuPDF 1.23**（mutool 冒烟）：EPUB 3.0 版本串告警（"unknown epub version: 3.0"）非致命，正常渲染；image-only 封面页按 img 自然尺寸渲染不放大。

发现问题时：记录阅读器 + 版本 + 检查点 + 截图，回到 docs/research/ecosystem.md
的对应坑位；修复后把 case 固化为 fixture/golden（能自动化的部分）。
