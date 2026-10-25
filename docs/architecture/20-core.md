---
type: architecture
status: active
summary: "trans-epub-core 转换内核设计：Converter 门面 API 与装配扩展点、pipeline pass 链（顺序契约表）、DOM 一次解析策略、协作式取消、确定性输出（uid 派生/zip 时间戳归一）、开闭原则落地检查表、测试策略（fixtures/golden/mock）"
tags: [trans-epub, architecture, core, pipeline, pass, dom, determinism]
contains: [design]
created: 2026-10-24
updated: 2026-10-25
related: ["../requirements.md", "10-contracts.md"]
authoring_mode: ai_generated
---

# 转换内核（trans-epub-core）

## 定位与红线

core 是**纯转换库**：`Source 进，EPUB 字节出`，其余一概不管。

- **同步内核**：不绑 tokio、不起线程。服务侧用 `spawn_blocking` 包裹执行（comrak/syntect/epub-builder 都是同步 CPU 密集的，强行异步没有收益）。
- **无全局状态、无环境依赖**：不读时钟、不读环境变量、不产生随机数——确定性输出的前提（见下文）。
- **I/O 全注入**：文件经 `Source::Vfs`，远程经 `Fetcher`，进度经 `ProgressSink`。core 里禁止出现 `std::fs`、`reqwest`、`eprintln!`。
- **输入路径禁止 panic**：任何输入数据只产生 `Result` 或 warning，不 unwrap、不 index 越界。

## 模块划分

```text
crates/trans-epub-core/src/
├── lib.rs          // pub use converter::Converter；重导出契约常用类型
├── converter.rs    // 门面：装配（前端注册序 / pass 链 / writer）+ probe + convert 编排
├── dom.rs          // BookDom：章节 HTML 的解析/序列化/DOM 工具（scraper 不出此边界）
├── refs.rs         // 相对引用解析（章节基址 + percent-decode + 自然排序）：
│                   //   前端资产登记与 link-rewrite 共用的地基
├── frontends/
│   ├── mod.rs      // 默认注册序：mdbook → md → web（详见 30-frontends.md）
│   ├── md.rs
│   ├── mdbook.rs
│   └── web.rs
├── pipeline/
│   ├── mod.rs      // Pass trait + PassCtx（解析一次 → 逐 pass → 序列化一次）
│   ├── asset_resolve.rs
│   ├── link_rewrite.rs
│   ├── highlight.rs
│   └── code_wrap.rs
└── writer/
    ├── mod.rs
    ├── epub3.rs    // 详见 50-writer.md
    ├── paths.rs    // EPUB 内部路径规则的唯一权威（link-rewrite 与 writer 共用）
    └── styles/     // base.css / admonition.css（include_str! 内嵌；code.css 运行时生成）
```

迁移注记：现有骨架的 `ir.rs` 内容迁入 trans-epub-contract；`frontends/`、`pipeline/`、`writer/` 目录结构保留，新增 `converter.rs` 与 `dom.rs`。

## Converter 门面

```rust
pub struct Converter {
    frontends: Vec<Box<dyn Frontend>>,   // 注册顺序 = probe 优先级
    passes: Vec<Box<dyn Pass>>,          // 顺序 = 执行顺序（构造时校验顺序约束）
    writer: Box<dyn Writer>,
}

/// 注入的 I/O，由壳层组装
pub struct Io {
    pub fetcher: Arc<dyn Fetcher>,
    pub progress: Arc<dyn ProgressSink>,
}

impl Converter {
    /// 默认装配：mdbook → md → web 前端；标准 pass 链；epub3 writer
    pub fn new() -> Self;

    // ---- 装配扩展点（开闭原则的入口）----
    pub fn with_frontend(mut self, f: Box<dyn Frontend>) -> Self;  // 插入到最高优先级
    pub fn with_pass(mut self, p: Box<dyn Pass>) -> Self;          // 追加到链尾
    pub fn with_writer(mut self, w: Box<dyn Writer>) -> Self;

    /// 探测：返回按优先级排序的全部命中（多命中正常，取第一个；其余供展示）
    pub fn probe(&self, source: &Source) -> Vec<(String, Outline)>;

    /// 转换：probe 选前端 → load → pass 链 → write → BuildOutput
    pub fn convert(
        &self,
        source: &Source,
        config: &ResolvedConfig,
        io: &Io,
    ) -> Result<BuildOutput, BuildError>;
}
```

`convert` 的内部编排即 00-overview.md 时序图的 core 侧：probe（取最高优先级命中，无命中 → `BuildError::Source`）→ load → pipeline → writer → 组装 `BuildOutput`（report 从 BookDraft.warnings + stats 汇出）。

## pipeline：pass 链

### Pass trait（在 core，不在契约——归属论证见 10-contracts.md）

```rust
pub trait Pass: Send + Sync {
    fn name(&self) -> &'static str;   // 同时用作 ProgressSink::stage 的阶段名
    fn run(&self, book: &mut BookDom, ctx: &mut PassCtx<'_>) -> Result<(), BuildError>;
}

pub struct PassCtx<'a> {
    pub config: &'a ResolvedConfig,
    pub source: &'a Source,           // 预留：Vfs 形态供需要读源的 pass 使用
    pub fetcher: &'a dyn Fetcher,
    pub progress: &'a dyn ProgressSink,
    pub stats: &'a mut BuildStats,    // 链式累计（links_rewritten 等）
}
```

`BookDom` 是 BookDraft 的 DOM 形态：pipeline 入口把每个章节的 HTML **解析一次**，各 pass 在 DOM 上做变更，出口**序列化一次**。禁止 pass 内部自行 parse/serialize。

### 标准链与顺序契约

| # | pass | 职责 | 前置约束 | 配置 |
|---|---|---|---|---|
| 1 | asset-resolve | 把所有 Asset 解析为可嵌入字节：`Remote` 经 Fetcher 拉取（离线模式直接降级）、类型识别（魔数/扩展名，不识别→warning+Missing）、单文件/总量上限校验；本地/data: 资产已由前端在 load 读入为 `Owned`，此处只校验 | 章节与资产集合已定（load 后即冻结） | AssetsCfg |
| 2 | link-rewrite | 解析全部 `href`/`src` → 相对章节 source 解析为 `SourceKey` → 命中章节则改写为 EPUB 路径，命中资产则改写为资产路径；**锚点校验**（原样 → slug 归一 → 落空则去 fragment + warning，杜绝悬空锚点过不了 epubcheck）；未解析→warning+降级（内部链接转纯文本，图片转 alt 文本占位）；外部链接原样保留 | 必须在 asset-resolve 后（Missing 资产影响降级决策）；依赖"章节集合冻结"不变量 | — |
| 3 | highlight | 找出未染色的 `pre > code.language-x`（已有元素子节点视为外部已染色，跳过），syntect 渲染为 `syn-` 前缀 classed span（构建期染色，不依赖阅读器）；未知语言按配置回退纯文本或记 warning | 与 1/2 无属性冲突；必须在 code-wrap 前 | HighlightCfg |
| 4 | code-wrap | 可选的构建期硬折行（列宽可配，按字符计），保底专治不理 pre-wrap 的阅读器 | 必须在 highlight 后（对已染色 span 里的文本节点折行，计数跨 span 连续） | WrapCfg |

**顺序是契约的一部分**：新 pass 必须声明插入位置并把理由写进本表。链执行器在 pass 边界与每章边界检查 `is_cancelled`，命中即返回 `BuildError::Cancelled`。

### link-rewrite 与 writer 的路径约定

EPUB 内部路径规则（章节文件名 `Text/ch00001.xhtml`、资产按类型入 `Images/` `Fonts/` `Media/`）由 `writer::paths` 模块**单点定义**，link-rewrite 与 epub3 writer 都引用它——改命名规则只动一处。这是"共享资产先定接口"原则在 core 内部的落点。

### DOM 策略

- 解析器：scraper 0.29（html5ever 0.40 + ego-tree 0.11；启用 `deterministic` feature，属性序稳定）。变更走 ego-tree 的 `append_subtree`/`append_id`/`detach` 与 scraper `Element.attrs`（pub），全部封装在 dom.rs，scraper 类型不出此边界。
- 序列化是**自实现的 XML 安全输出**：EPUB3 内容文档是 XHTML，[【已否决】 html5ever 自带的 HTML 序列化](#html5ever-序列化被否决)（`<br>` 不自闭合，XHTML 不可用）——dom.rs 负责 void 元素自闭合、文本/属性转义；注释与 script/style/noscript/template 一律丢弃（输出格式约束，不是通用 HTML 处理）。归一化由 golden 锁定，不追求与源字节一致。
- 前端产出 HTML 字符串进 IR，pipeline 再解析——**字符串边界**刻意保留：comrak 升级、scraper 升级互不波及，前端与 pass 解耦。
- md 前端经 comrak 渲染 HTML 时**不做**高亮/链接处理（义务清单第 6 条），保证三种来源进 pipeline 时同型。

#### html5ever 序列化被否决

> [!CAUTION] 【已否决】 html5ever 自带的 HTML 序列化
> 原因：`<br>` 等 void 元素不自闭合，XHTML 输出不可用。
>
> 改用 dom.rs 自实现 XML 安全输出（void 元素自闭合、文本/属性转义），归一化由 golden 锁定。

## 开闭原则落地检查表

新增一种能力时"要动哪里/不动哪里"——评审扩展设计时逐行核对：

| 扩展 | 要动 | 不动 |
|---|---|---|
| 新输入格式 | 新 frontend 模块 + `frontends/mod.rs` 注册 + 30-frontends.md | 其他前端、全部 pass、writer、契约、Converter 编排逻辑 |
| 新处理能力 | 新 pass 模块 + 本文顺序表（声明位置）+ Converter 默认链装配处 | 已有 pass、前端、writer、契约 |
| 新输出格式 | 新 writer 模块 + Converter 默认装配 | pipeline、前端、契约（Writer trait 已就位） |
| 新告警类型 | WarningCode 增枚举值 | 已有枚举语义、消费方 |
| 新阶段进度 | pass name 自然成为 stage 名 | ProgressSink 签名 |

**结构性禁止**（开闭的敌人）：对输入格式做 enum 全处 match；pass 之间隐式顺序耦合（顺序约束必须写进上表）；pass 直接读环境/时钟/随机。

## 确定性输出

相同输入 → 字节级相同的 EPUB：

- 章节文件名按 spine 序派生（`ch00001.xhtml`…，1 起），不掺内容哈希或时间。
- EPUB unique-identifier 由书名 + 源集（排序后的 SourceKey、章节内容、资产内容哈希）SHA-256 派生，同输入同 uid。
- zip 全部条目时间戳归一为 1980-01-01 00:00:00；`dcterms:modified` 用固定常量（core 不读时钟）。epub-builder 不暴露时间戳控制，故**直接用 zip crate 写包**（50-writer.md 的「patch 或换 zip 层」取后者，已落地验证）。
- 价值：golden 样书可做**字节级比对**（已实现：同输入连转两次、以及与提交的 golden `.epub` 比对）；服务化后同任务结果可缓存、可去重。

## 测试策略

- **单测**：pass 用 mock Vfs / mock Fetcher（契约 trait 的内存实现，`trans-epub-contract` 的 `test-util` feature，位于 `contract::testing`）；link-rewrite 的解析/规范化用表驱动用例覆盖边角。
- **fixtures**（`tests/fixtures/`）：md（平铺+README/数字前缀+GFM 全家桶/断链缺图/嵌套树/front matter order/单文件）；mdbook（3 层嵌套 SUMMARY+part+draft+include 全形态+rustdoc 锚点+hide+缺失目标+hidden 章，MVP2 落地）。web 的 fixtures 随 MVP3 补。
- **golden**（`tests/golden/`）：端到端产物**字节级比对**（依赖确定性；`UPDATE_GOLDEN=1` 重新生成并 review diff）；比对失败时 diff 解包后的 XHTML 快照定位。已有 4 本：flat / tree / single / mdbook。
- **验收门**：epubcheck 全绿（`scripts/epubcheck.sh` + 环境可用的门测试）；真机抽验清单（Koodo / Apple Books / KOReader）见 docs/testing/manual-checklist.md。
