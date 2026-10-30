---
type: architecture
status: active
summary: "trans-epub 公共契约：输入源两形态（Vfs/Pages）、前端 trait（probe/load 分离）与前端义务清单、pdf 前端设计要点、IR 枢轴 BookDraft 及其不变量、SourceKey 规范化、输出（Writer/BuildOutput/BuildReport）与 EPUB 输出增强三件、错误双轨与故障上报三接收点、进度与取消、ResolvedConfig；契约 crate 的归属判据与稳定性变更流程"
tags: [trans-epub, architecture, contract, ir, source, frontend-trait]
contains: [design, contract]
created: 2026-10-24
updated: 2026-10-30
related: ["../requirements.md", "00-overview.md"]
authoring_mode: ai_generated
---

# 公共契约（trans-epub-contract）

## 定位与归属判据

契约 crate 只放**类型与 trait**，无实现逻辑，依赖仅限 serde / url / thiserror 一级。

**归属判据：会被 core 之外的 crate 实现或消费的进契约；只在 core 内部流转的不进。** 据此：

- 进契约：Source、Vfs、RelPath、FetchedPage、Fetcher（fetch crate 要实现）、Frontend（未来可能有 out-of-tree 前端）、IR（服务要读 Outline/Report）、Writer、ProgressSink、BuildError / Warning、ResolvedConfig。
- 留在 core：**Pass trait 与 DOM 类型**。pass 需要 DOM，而 DOM 库（scraper/html5ever）是重依赖——若进契约，fetch 为实现 Fetcher 就得陪绑整个 HTML 解析栈。第三方扩展 pass 时依赖 core 即可（pass 本来就是转换域的东西）。

IR 是进程内类型，**不做序列化与版本化**；它的"版本管理"就是本文档 + golden 测试。

> 本文中的 Rust 代码是**契约草图**：以字段与语义为准，非最终签名。

## 输入侧：两种源形态

已知的输入格式全部收敛为两种抽象形态：

| 已知输入 | 形态 | 消费前端 |
|---|---|---|
| 散装 markdown（单文件 / 目录 / 目录树） | `Source::Vfs` | md |
| mdBook 项目（book.toml + src/SUMMARY.md） | `Source::Vfs` | mdbook |
| 静态文档站（**已抓取**的页面列表） | `Source::Pages` | web |
| 外置渲染器产物（SPA 预渲染等，三期接缝） | `Source::Pages` | web（复用） |

```rust
/// 一次转换的输入。壳层负责构造：CLI 用磁盘/采集结果，服务用上传内容。
pub enum Source {
    /// 文件树形态（散装 md / mdBook 项目）
    Vfs(Arc<dyn Vfs>),
    /// 页面列表形态（静态站 / 外置渲染器）；**顺序即书序**，由采集方负责
    Pages(Vec<FetchedPage>),
}

/// 归一化相对路径：'/' 分隔、UTF-8、无 `..`、无盘符、无前导 '/'。
/// 安全规则：任何来自输入的路径必须经它校验后才能进 Vfs（防路径穿越）。
pub struct RelPath(String);

pub trait Vfs: Send + Sync {
    fn read(&self, path: &RelPath) -> Result<Vec<u8>, VfsError>;
    fn exists(&self, path: &RelPath) -> bool;
    /// 递归列举全部文件（含大小：供上限检查与结构探测）
    fn walk(&self) -> Result<Vec<FileEntry>, VfsError>;
}

pub struct FileEntry { pub path: RelPath, pub size: u64 }

/// 已抓取的页面。抓取发生在 core 之外（fetch crate / 未来服务）。
pub struct FetchedPage {
    pub url: Url,              // 规范化后的绝对 URL，无 fragment
    pub body: Vec<u8>,         // 原始字节，编码由前端处理
    pub content_type: String,  // 如 "text/html; charset=utf-8"
}

/// 远程资源获取口：pipeline 拉远程图片时使用。
/// robots/限速/缓存/代理全部是实现的事（fetch crate / 服务自带实现），core 只管调用。
pub trait Fetcher: Send + Sync {
    fn fetch(&self, url: &Url) -> Result<FetchedPage, FetchError>;
}
```

要点：

- **web 输入是"已抓取"的**。core 不含任何爬取逻辑；`Pages` 的顺序就是成书顺序，页面发现与排序质量是采集方（CLI 的 crawl 命令 / 服务）的责任。
- `Fetcher` 是同步 trait（core 是同步内核）；异步实现由服务侧用 `spawn_blocking` 适配。
- **`Fetcher` 的职责止于传输与发现**：产出原始字节，不做内容理解——正文抽取、编码判定、站点适配全在 core web 前端。站点适配（预设 / `selectors` / `strip`）的单一事实源是 `WebCfg`（数据），不随 Fetcher 实现走：CLI 与服务可各换采集实现，抽取规则永远只有 core 一套。
- **`fetch_direct`（2026-10-26 增）**：用户明确点名的单资源获取（封面图 / 钉版字体下载等）——trait 默认实现 = `fetch`（对实现零破坏，mock/服务实现自动兼容）；`PoliteFetcher` 覆写为跳过 robots 检查（robots 管的是"爬虫发现式抓取"，用户点名的 URL 不属此类；github.com robots 禁 release 资产路径曾致 exact 字体下载被误拒）。限速 / 重试语义不变；`max_bytes` 参数是调用方的显式上限提示（钉版字体 zip ~140MB 高于常规资产限额），`None` = 实现自身默认。
- 离线模式不是 Fetcher 的事：`AssetsCfg.remote = false` 时 asset-resolve pass 直接跳过远程拉取并降级。

## 前端契约：probe / load 分离

```rust
pub trait Frontend: Send + Sync {
    fn name(&self) -> &'static str;   // "md" | "mdbook" | "web" | ...
    /// 能否识别该源；能识别则给出章节清单预览（不做内容转换）。
    /// 带 config：结构声明覆盖（声明 > 探测）在 probe 就要生效，
    /// dry-run 才能反映真实书序。
    fn probe(&self, source: &Source, config: &ResolvedConfig) -> Probe;
    /// 归一化：源 → BookDraft。顺序与嵌套在此定死。
    fn load(&self, source: &Source, env: &LoadEnv) -> Result<BookDraft, BuildError>;
}

pub enum Probe {
    NoMatch,
    Matched { outline: Outline },
}

pub struct Outline {
    pub kind: &'static str,            // "md-file" / "md-flat" / "md-tree" / "md-declared" / "mdbook" / "web" / ...
    pub items: Vec<OutlineItem>,       // 嵌套清单
    pub warnings: Vec<Warning>,
}

pub struct OutlineItem {
    pub title: String,
    pub source: SourceKey,             // 指向章节来源（诊断 + GUI 勾选剔除的定位键）
    pub children: Vec<OutlineItem>,
}

pub struct LoadEnv<'a> {
    pub config: &'a ResolvedConfig,
    pub progress: &'a dyn ProgressSink,
}
```

分离的理由：dry-run、GUI 结构预览、服务的"先给用户看目录再确认"都只需要 Outline，不应付出整书转换的代价。probe 必须廉价（只看文件名/URL 与少量元数据，不解析正文）。

前端优先级 = Converter 的注册顺序（默认 mdbook → md → web），多个命中取第一个。

### 前端义务清单（load 产出的硬性要求）

1. **干净 HTML**：`Chapter.html` 中无源格式残留——mdBook 指令已展开、markdown 语法已渲染、网页 chrome（导航/页脚/脚本）已剔除。golden 测试守住这条线。
2. **heading 全部有稳定 id**：缺失则生成 slug 并去重（intra-page 锚点链接依赖它）。
3. **引用保留原样**：`href`/`src` 保持源的相对/绝对原始形式，改写只属于 link-rewrite pass。
4. **顺序与嵌套在 load 时定死**：`toc` 树一旦产出，后续 pass 不得改动。
5. **单项失败 → warning 继续**：绝不因单个文件/页面失败中止，更不允许 panic。
6. **不做通用 HTML 处理**：高亮、链接改写、折行归 pass，前端不做——这是防止职责漂移的关键条款。

### pdf 前端：设计要点（2026-10-30 评估定稿，未实施）

非扫描版 PDF → EPUB 的**重排路线**（提取文字与插图重建语义，非页面图嵌入）。输入侧第四前端，与 Writer 处"未来 pdf 输出格式"的注释无关。架构上是标准新前端活：归一为 BookDraft 后，后续管线（asset-resolve / writer / golden）零改动复用；实施时按上述义务清单执行，并在 30-frontends.md 增补专节。要点：

- **引擎选型（实施时拍板）**：建议 `mupdf-rs` 优先——AGPL-3.0 与本项目 License 同源（ADR 0012），MuPDF 已随 mutool 冒烟验证过生态；备选 `pdfium-render`（提取质量相当，但六个发布目标各带一份动态库，Releases 流水线负担不小）。
- **乱码防线**：非标准编码 / 缺 ToUnicode 的嵌入字体提取出乱码——按提取结果 CJK/控制字符比例检查，报容忍轨 warning，不静默产出坏书。
- **文字复原**：断词跨行合并（de-hyphenation）、连字 fi/fl 规范化、CJK 不插假空格（PDF 内 CJK 无空格是常态，勿按西文逻辑补格）。
- **噪音剔除**：跨页重复模式检测剔除页眉/页脚/页码——reflow 质量第一杀手。
- **结构重建**：标题层级按同文档内字号相对聚类（不拍绝对 pt 值）；代码块按等宽字体聚类；阅读顺序 XY-cut；表格第一版放弃（区域截图 + warning）；矢量插图渲染位图为有损降级（+ warning）；链接保留 URI annotation，内部 GoTo 尽力转锚点、失败降级纯文本。
- **扫描版显式拒绝**：整本文本量为 0 → 致命轨"疑似扫描版需 OCR"，不静默出空书。
- **确定性**：同一 PDF 字节必须产出同一 EPUB 字节（golden 回归可测），字号聚类等浮点/顺序判定钉死确定性排序。
- 每个启发式配一条 `WarningCode` 扩码（只增不改语义）与测试；故障上报按三接收点口径。

## IR 枢轴：BookDraft

IR 是 frontends 的终点、pipeline 的起点，也是整个架构的中心契约。

```rust
pub struct BookDraft {
    pub meta: BookMeta,
    pub toc: Vec<TocItem>,                   // 嵌套目录，引用 ChapterId
    pub chapters: Vec<Chapter>,              // 顺序即 spine 顺序
    pub assets: BTreeMap<SourceKey, Asset>,  // 章节外资源（图片/字体/封面）
    pub warnings: Vec<Warning>,              // 全程累积，只增不减
}

pub struct Chapter {
    pub id: ChapterId,
    pub title: String,
    pub source: SourceKey,   // 来源标识：诊断定位 + 相对引用的解析基址
    pub html: String,        // 干净 HTML，保留原始 href/src
}

pub struct ChapterId(pub u32);   // = chapters 下标 = spine 序

pub struct TocItem {
    pub chapter: Option<ChapterId>,  // None = 纯分组标题（mdBook part）
    pub title: String,
    pub children: Vec<TocItem>,      // 嵌套不拍平（mdBook 语义）
}

pub struct Asset {
    pub source: SourceKey,
    pub data: AssetData,
}

pub enum AssetData {
    Owned(Vec<u8>),   // load 时已就绪（本地文件 / data: URI / 封面）
    Remote(Url),      // 由 asset-resolve pass 经 Fetcher 拉取
    Missing,          // resolve 失败的终态；引用方降级处理
}

pub struct BookMeta {
    pub title: String,
    pub authors: Vec<String>,
    pub language: String,          // BCP-47
    pub description: Option<String>,
    pub cover: Option<SourceKey>,  // 指向 assets 里的封面
}
```

### 不变量（违反即 bug）

1. `ChapterId` 恒等于 chapters 下标；`toc` 只引用存在的 ChapterId。
2. **load 之后 pass 不得增删章节、不得改变顺序**——pass 只能改章节内容与资产状态。这是 link-rewrite 依赖的前提（章节身份必须先稳定）。
3. 章节 HTML 中对其他章节/资产的引用，在 link-rewrite 之前必须保持原始形式。
4. `assets` 的键与章节 `source` 同处一个 `SourceKey` 编址空间。
5. `warnings` 单调递增；任何"跳过/降级/剔除"决定都必须伴随一条 warning（可追溯）。

### SourceKey 规范化（链接改写的地基）

```rust
pub enum SourceKey {
    Path(RelPath),  // Vfs 世界：相对源根的规范化路径
    Url(Url),       // Pages 世界：规范化 URL（无 fragment）
}
```

- Path 侧：'/' 分隔、去 `./`、拒绝 `..`；保留原始大小写（大小写折叠由 Vfs 实现负责一致性）。
- Url 侧：绝对化、去 fragment、scheme/host 小写、去默认端口、path 与 query 原样保留。
- **Path 键与 Url 键永不互相解析**——两个世界各管各的；跨世界引用本身就是 `LinkUnresolved`。

## 输出侧

```rust
pub trait Writer: Send + Sync {
    fn format(&self) -> &'static str;   // "epub3"；未来 "pdf" 等在此扩展
    fn write(&self, book: &BookDraft, config: &ResolvedConfig) -> Result<Vec<u8>, BuildError>;
}

/// 由 Converter 组装的最终产物
pub struct BuildOutput {
    pub format: String,
    pub bytes: Vec<u8>,      // EPUB 字节；落盘/上传由壳层决定
    pub report: BuildReport,
}

pub struct BuildReport {
    pub warnings: Vec<Warning>,
    pub stats: BuildStats,   // 章节数 / 资产嵌入数 / 资产缺失数 / 链接改写数 / 未解析链接数…
}
```

Writer 返回**字节而非文件路径**是刻意决定：core 不碰文件系统，产物处置权在壳层。

### EPUB 输出增强三件（2026-10-30 评估定稿，未实施）

对照 Pandoc 默认模板与 EPUB 惯例的差距盘点：基础盘不动（代码保真线已是差异化强项，暗色模式超 Pandoc 默认），补三件，全部落在 writer 层、前端零改动：

1. **脚注语义化**（收益最大）：comrak 脚注现渲染为普通链接 + 章末 `.footnotes` 节；改 Pandoc 式 `<aside epub:type="footnote">` + `<a epub:type="noteref">` + 回链结构，解锁 Kobo/iBooks 等阅读器的弹窗脚注。
2. **landmarks nav + OPF guide**：nav.xhtml 现只有 `epub:type="toc"`；补 `epub:type="landmarks"`（含 Begin Reading 锚点）与 OPF `<guide>`，供 MuPDF 类老引擎快跳转。
3. **base.css 微补**：`hyphens` / `-epub-hyphens: auto`（须配 `lang` 属性传递）、`widows` / `orphans`、表与图 `break-inside: avoid`。

每件随实施走本文档先行 + golden 再生流程；真机弹窗表现记 manual-checklist。

## 错误模型：双轨

```rust
pub enum BuildError {
    Cancelled,                                // 协作式取消触发
    Source(String),                           // 源不可读 / 无前端可识别
    Fatal { code: &'static str, message: String }, // 不可恢复错误
}

pub struct Warning {
    pub code: WarningCode,
    pub source: Option<SourceKey>,   // 定位到章/页/资产
    pub message: String,
}

pub enum WarningCode {
    AssetFetchFailed, AssetTooLarge, AssetUnsupported,
    LinkUnresolved, PageExtractFailed, DirectiveUnresolved,
    StructureAmbiguous, EncodingLossy, DuplicateSource,
    HighlightSkipped,   // 代码语言/主题名无法解析（MVP1 补充）
    // … 只增不改语义
}
```

- **致命轨**：`Result::Err` 中止整次转换（取消、源不可读、写出失败）。
- **容忍轨**：warning 进 `BookDraft.warnings`，最终进 BuildReport——机器可读（code + 定位），服务与 `--strict` 模式据此决策。
- core 的输入处理路径**禁止 panic**；壳层（服务）可另加 `catch_unwind` 保险，但那不是 core 的契约。

### 故障上报：三接收点统一（2026-10-30 增）

出口形态（CLI / GUI / 未来服务）可以不同，**接收点唯一**——共三处，全部落在既有契约上，不新建平行故障体系：

1. **实时**：`ProgressSink::warning`（见下节，默认空实现）——容忍轨构建中逐条镜像推送。
2. **结局**：`BuildReport.warnings`——容忍轨权威汇总；实时通道只是它的视图。
3. **致命**：`Result<BuildOutput, BuildError>` 返回值。致命故障**不走** warning 通道：出口必须处理它，不存在漏看；warning 可忽略，所以才配专门通道。

三路数据同源：core 内部收敛到一个 **Reporter**（收集 `BookDraft.warnings` + 同时刻镜像推 `ProgressSink::warning`）单点出口，各组件不直接操作 warnings vec——两路永不分叉。组件失败点随此扩码补齐 `WarningCode`（只增不改语义；已知缺口如 ttf 解析失败的降级路径——fuzz 曾在其相邻路径抓到 panic，此类"能降级的失败"应报 warning 退默认形态而非中止）。

**出口适配是壳层的事**（core 不碰 stderr / UI / 网络，同 Writer 只回字节的分界）：CLI 做实时 stderr 行 + 结尾汇总表 + 退出码映射；GUI 把回调投递到 UI 线程再呈现（core 在工作线程跑），`Warning.source`（SourceKey）供点击定位到章/页；服务可选流式（SSE / JSON 行）透传实时事件，report 本就 Serialize 可直接作 API 响应字段、致命轨映射 HTTP 状态码。未来新形态（TUI / WASM 前端等）实现同一个 `ProgressSink`、读同一份 report、处理同一个 `Err`，core 零改动。

## 进度与取消

```rust
pub trait ProgressSink: Send + Sync {
    fn stage(&self, name: &str);                        // 阶段切换：probe/load/各 pass 名/write
    fn item(&self, done: u64, total: u64, note: &str);   // 细粒度心跳（按章/按页）
    fn warning(&self, w: &Warning) {}                    // 容忍轨实时镜像；默认空实现对现有实现零破坏
    fn is_cancelled(&self) -> bool { false }             // 取消检查点
}
```

- `stage` 用字符串而非枚举：**自定义 pass 也要能报进度**，不因扩展改契约类型（开闭原则的细节体现）。
- `warning`（2026-10-30 增）是容忍轨的实时镜像：pipeline 记 warning 的同一时刻推给壳层，供 CLI/GUI 实时呈现；权威汇总仍只有 BuildReport 一份（见"故障上报：三接收点统一"）。默认空实现——不关心实时的调用方（含既有实现与测试 mock）一行不改。
- 取消是协作式的：pipeline 在 pass 边界与每章边界检查 `is_cancelled`，命中即返回 `BuildError::Cancelled`。core 自己不建线程、不起运行时。

## 配置模型

```rust
/// 已合并的最终配置。合并优先级（TOML < front matter < CLI flags）是壳层的责任，
/// core 不感知配置从哪来。
pub struct ResolvedConfig {
    pub book: BookMetaCfg,        // 元数据兜底值：标题/作者/语言/封面路径
    pub structure: StructureCfg,  // 探测规则开关、显式章节声明（声明 > 探测）、include/exclude
    pub markdown: MarkdownCfg,    // 方言开关（默认 GFM + front matter + 脚注 + 任务列表 + admonition）
    pub mdbook: MdbookCfg,        // 指令展开选项
    pub web: WebCfg,              // 正文抽取：预设名 / 选择器覆盖 / 清理规则
    pub assets: AssetsCfg,        // remote: bool、单文件大小上限、总大小上限
    pub highlight: HighlightCfg,  // 亮/暗主题、未知语言回退
    pub wrap: WrapCfg,            // PreWrap（默认）| Hard { width: 92 }
    pub theme: ThemeCfg,          // 内置主题 + extra_css: String（壳层已把文件读成字符串）
    pub fonts: FontsCfg,          // 内嵌字体（见下）
}
```

各字段的默认值以需求定稿 v1.0 为准。原则：**配置只描述"要什么"，不描述"怎么做"**——不存在"跳过某个 pass"这类过程性开关，扩展行为通过 Converter 的装配点完成。

### 内嵌字体（FontsCfg，2026-10-26 增）

```rust
pub struct FontsCfg {
    pub embed: bool,               // 默认 false：产物保持轻量，CJK 书可开
    pub subset: FontSubsetMode,    // Preset（默认，内置 8105 字预子集）| Exact（按书内字符精确子集）
    pub source: FontSource,        // Builtin（默认，Maple Mono CN 子集）| User { data, family }
}
```

- **三正交**：内嵌 on/off × 子集模式 preset/exact × 字体源 builtin/用户路径。用户路径由壳层读成字节进配置（同 `ThemeCfg.extra_css` 先例——core 不做文件 IO 决策）；`family` 缺省时 core 读 name 表得出。
- **builtin 资产**：`crates/trans-epub-core/assets/fonts/TransEpubMono-Regular.subset.ttf`（Maple Mono CN v7.9 子集 + 改名 "TransEpub Mono"，OFL-1.1；溯源与再生成见同目录 NOTICE.md 与 `scripts/gen-font-subset.py`）。**exact × builtin 需要全量字体**：经 Fetcher 下载钉版 URL（release v7.9、sha256 校验），壳层缓存于本地缓存目录；下载/校验失败回落 preset 子集 + warning，不断书。
- **OFL-1.1 义务随书**：内嵌时 writer 在书末加字体许可页（OFL 全文 + 上游版权 + 修改声明（子集 + 改名）+ 溯源 URL），manifest 登记；用户字体不走该页，许可责任在配置方（CLI 对此提示）。
- **off 档**：不嵌任何字体，CSS 代码字体链退回具体名字体链（20-core.md / 50-writer.md 口径）。
- **矛盾配置**：`source = User` 且 `embed = false` 时行为以 embed 为准（不内嵌），CLI 层给出提示（服务侧由前端提示）。

## 稳定性与变更流程

- 契约 crate 的全部 pub 项就是 semver 契约面。
- 破坏性变更流程：先改本文档 → 同步更新受影响的组件文档与 golden → 代码实现 → 版本号 minor 递增。禁止"代码先行、文档追认"。
- IR 字段演进**只加不改**：新增字段带默认值，不重命名、不改已有字段语义。
- Outline / BuildReport 是面向最终用户的展示数据（GUI、服务、dry-run 都直接消费），稳定性要求最高。
