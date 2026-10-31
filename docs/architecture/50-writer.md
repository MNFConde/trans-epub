---
type: architecture
status: active
summary: "trans-epub EPUB3 写出器设计：BookDraft→字节、产出物结构（spine/nav/元数据/封面）、CSS 主题体系（亮色基线+prefers-color-scheme 暗色+用户叠加+代码与 admonition 样式）、长行策略（pre-wrap 默认/硬折行归 pass）、确定性（文件名/uid/zip 时间戳与 system 字节归一）、epubcheck 验收"
tags: [trans-epub, architecture, writer, epub3, css, theme]
contains: [design]
created: 2026-10-24
updated: 2026-10-31
related: ["../requirements.md", "10-contracts.md", "20-core.md"]
authoring_mode: ai_generated
---

# EPUB3 写出器（core::writer）

## 定位

`Writer` trait 的第一个（初期唯一）实现：**BookDraft 进，EPUB3 字节出**。

- 输入 BookDraft 已经过完整 pass 链：链接已改写、资产已就绪（Owned/Missing）、高亮已染色、折行已完成——writer **只做组装，不改内容**。
- 返回字节而非路径（契约决定）：core 不碰文件系统。
- 基座：[【当前】 zip crate 直写](#当前基座zip-crate-直写)（[【已否决】 epub-builder](#epub-builder-被否决)——不暴露条目时间戳控制，无法满足确定性；本文原「patch 或换 zip 层」取后者，已落地验证——mimetype 首条目 STORED、其余 Deflated、时间戳全部归一、central directory system 字节归一）。
- **不做 Kindle 牺牲性适配**（需求定稿决策）：以现代 EPUB3 阅读器为基准（Apple Books / KOReader / 手机 app）。

### 当前基座：zip crate 直写

> [!IMPORTANT] 【当前】 zip crate 直写
>
> mimetype 首条目 STORED、其余 Deflated、条目时间戳全部归一、central directory 的 version-made-by system 字节归一为 Unix(3)——确定性输出在 writer 层闭环，已落地验证（system 字节归一为 2026-10-29 跨平台 CI 实证后补：zip crate 按编译目标写 Dos/Unix 且公开 API 不可配，见 20-core.md 确定性输出节）。

### epub-builder 被否决

> [!CAUTION] 【已否决】 epub-builder
> 原因：不暴露条目时间戳控制，无法满足确定性输出（同内容必须同字节）。
>
> 「patch 或换 zip 层」二选一取后者：直接换 zip 层。

## 产出物结构

```text
mimetype                        # application/epub+zip（首个条目、不压缩）
META-INF/container.xml
OEBPS/
├── content.opf                 # 元数据 + manifest + spine（= chapters 顺序）+ guide（EPUB2 legacy 快跳转）
├── nav.xhtml                   # 嵌套目录（toc 树原样映射，不拍平）+ landmarks nav（hidden）
├── Text/ch00001.xhtml…         # 一章一文件，按 spine 序命名
│   └── font-license.xhtml      # 字体许可页（仅内嵌带许可字体时，spine 末尾）
├── Images/…  Media/…           # 资产按类型分目录（命名规则出自 writer::paths）
├── Fonts/…                     # 内嵌字体（fonts pass 产出，默认不嵌）
└── Styles/
    ├── base.css                # 排版基线
    ├── code.css                # 代码块 + 高亮配色（亮/暗两套）
    ├── admonition.css          # GitHub 风格提示色块
    └── extra.css               # 用户自定义 CSS（原样追加在最后）
```

EPUB 内部路径规则集中在 `writer::paths` 模块单点定义，link-rewrite pass 引用同一处（20-core.md）。

## 各部分要点

- **spine**：chapters 顺序即阅读顺序（IR 不变量）；`linear="yes"`。
- **nav.xhtml**：toc 树直接映射（part 分组节点 = 无链接的标题层级），阅读器侧显示完整嵌套；外加 `epub:type="landmarks"` hidden nav（cover + bodymatter=Begin Reading；不放 toc 自引用——nav 不进 spine，epubcheck RSC-011 拒绝）。
- **OPF guide**：EPUB2 legacy `<guide>`（cover/toc/text 三 reference，epubcheck 5.2.1 实证 3.3 包内合法），供 MuPDF 类老引擎快跳转。
- **脚注语义化**：`writer/footnotes.rs` 按 comrak 确定性形态字符串级转换（引用→`epub:type="noteref"`、`li`→`aside epub:type="footnote"`、回链→`epub:type="backlink"`；section/ol 包裹摘除、id 保留）；形态不匹配原样通过（writer 无容忍轨出口）。契约细节见 10-contracts.md「EPUB 输出增强三件」。
- **语言属性**：章节页与 nav 的 `<html>` 写 `lang`+`xml:lang`（book.meta.language）——base.css `hyphens: auto` 的生效前提；`pre/code` 显式禁断词。
- **元数据**：BookMeta → DC 元素；language 必填（BCP-47）；unique-identifier 确定性派生（书名+源集哈希）。
- **封面**：`meta.cover` 指向资产，可选（用户供图，需求定稿）。
- **Missing 资产**：引用已在 link-rewrite 降级（占位文本），writer 不再处理，只保证不写悬空引用。
- **内嵌字体**（`BookDraft.fonts` 非空时）：字体文件落 `OEBPS/Fonts/`（manifest media-type `application/font-sfnt`）；code.css 头部生成 `@font-face` 并把字体族名插进代码字体链首（链尾保持具体名字体链 + monospace 兜底——老栈不解析泛型族，MVP1 真机观察 3）；带许可的字体（builtin）在 spine 末尾追加**字体许可页**（OFL-1.1 全文 + 上游版权 + 修改声明（子集 + 改名）+ 溯源），用户字体不出义务页（许可责任在配置方）。资产溯源与再生成见 `crates/trans-epub-core/assets/fonts/NOTICE.md`。

## CSS 主题体系

差异化所在的"EPUB 成书质量"，很大一半落在这里：

- **base.css**：中性亮色排版基线——正文/标题/引用/表格/图片自适应（`max-width:100%`）；代码块轻底色 + 边框 + padding（与正文纸面拉开区分，MVP1 真机观察 1）；含暗色适配块。
- **code.css**：`pre`/`code` 基础样式 + 代码字体链（具体名优先，泛型 monospace 兜底；内嵌字体时族名插链首）+ `pre { white-space: pre-wrap; word-break: break-word; }`（pre-wrap 默认，ADR-0007）+ syntect 主题配色——亮色主题 CSS 平铺 + 暗色主题包在 `@media (prefers-color-scheme: dark)` 里；span class 带 `syn-` 前缀防与书内 CSS 冲突。主题 CSS 由 syntect 运行时生成（`OnceLock` 缓存，同一 syntect 版本字节确定），主题名可配、未知名回落默认。
- **admonition.css**：GitHub 风格提示色块（note/tip/important/warning/caution 五色，亮暗两套）。
- **extra.css**：`ThemeCfg.extra_css` 原样追加，选择器优先级天然靠后（用户叠加不改内置主题）。
- base/admonition 以 `include_str!` 内嵌于二进制，版本随 crate 走。

## 长行策略（与 pass 的分工）

- **默认**：pre-wrap——`code.css` 里 `pre { white-space: pre-wrap; word-break: break-word; }` 按屏宽自适应折行，零内容改写。
- **保底**：构建期硬折行由 code-wrap pass 完成（列宽可配），writer 不做任何内容级改写——"CSS 能解决的不动内容，内容级处理都在 pipeline"。

## 确定性（与 20-core.md 呼应）

- 章节文件名 `Text/chNNNNN.xhtml` 按 spine 序，不掺时间/哈希。
- zip 全部条目时间戳归一为固定值 → 同输入字节级相同，golden 直接 diff。
- CSS/模板内嵌于二进制（`include_str!`），版本随 crate 走。

## 验收

- **epubcheck** 全绿是合入门槛：`scripts/epubcheck.sh`（jar 缺失自动下载到 target/）+ golden 测试里的环境可用门测试。MVP1 现状：flat / tree / single 三本 golden **零错误零警告**。
- golden 样书字节比对（依赖确定性）：`UPDATE_GOLDEN=1` 重新生成并 review diff。
- 真机抽验清单：docs/testing/manual-checklist.md（Koodo Reader 主力 + Apple Books / KOReader 各一，检查点：代码高亮亮暗切换、长行折行、嵌套目录跳转、图片显示、脚注/admonition 渲染）。
