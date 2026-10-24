---
type: testing
status: active
summary: "trans-epub 真机抽验清单：MVP1 起随每次大改动抽验；主力 Koodo Reader，Apple Books / KOReader 各一；检查点覆盖代码高亮亮暗切换、长行折行、嵌套目录跳转、图片显示、脚注与 admonition 渲染、封面"
tags: [trans-epub, testing, manual-checklist, epub-readers]
contains: [checklist]
created: 2026-10-24
updated: 2026-10-24
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

## 样书

用 golden fixtures 生成（覆盖所有特性）：

```bash
cargo run -p trans-epub-cli -- crates/trans-epub-core/tests/fixtures/flat -o flat.epub
cargo run -p trans-epub-cli -- crates/trans-epub-core/tests/fixtures/tree -o tree.epub
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

### 目录与跳转

- [ ] 目录（nav）嵌套层级完整（tree 样书：guide 分组下有子章）
- [ ] 分组节点可点进其 README 章（guide → The Guide）
- [ ] 章内链接跳转到目标章（"link to syntax"）
- [ ] 带锚点链接落到目标标题（"good anchor"）
- [ ] 外部链接保留可点（autolink 的 https://example.com）

### 图片与资产

- [ ] 本地图片显示（diagram.png，1×1 透明，应不报错）
- [ ] 远程图片已嵌入并显示（remote.png）
- [ ] 缺失图片位置显示 `[missing image: ghost]` 占位文本
- [ ] 封面：`--cover` 指定后在书架显示封面图

### md 方言

- [ ] 表格渲染为表格
- [ ] 任务列表复选框显示且不歪
- [ ] 脚注可跳转、可回跳（flat/02-footnotes）
- [ ] admonition 色块（NOTE 蓝 / WARNING 黄）亮暗两套下都可读

### 元数据与基础

- [ ] 书名/作者/语言正确（书架卡片）
- [ ] 章节顺序 = 目录顺序 = 阅读顺序
- [ ] 快速翻章不卡（6 章规模无感即可）

## 阅读器优先级

| 阅读器 | 角色 | 备注 |
|---|---|---|
| Koodo Reader | 主力 | 每次全量 |
| Apple Books | 抽验 | CSS 兼容性风向标（macOS/iOS） |
| KOReader | 抽验 | 嵌入式设备口径（较老的渲染栈） |

发现问题时：记录阅读器 + 版本 + 检查点 + 截图，回到 docs/research/ecosystem.md
的对应坑位；修复后把 case 固化为 fixture/golden（能自动化的部分）。
