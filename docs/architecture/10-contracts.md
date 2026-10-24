---
type: architecture
status: active
summary: "trans-epub 公共契约：输入源两形态（Vfs/Pages）、前端 trait（probe/load 分离）、IR 枢轴 BookDraft 及其不变量、SourceKey 规范化、输出（Writer/BuildOutput/BuildReport）、错误双轨、进度与取消、ResolvedConfig；契约 crate 的归属判据与稳定性变更流程"
tags: [trans-epub, architecture, contract, ir, source, frontend-trait]
contains: [design, contract]
created: 2026-10-24
updated: 2026-10-24
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

## 进度与取消

```rust
pub trait ProgressSink: Send + Sync {
    fn stage(&self, name: &str);                        // 阶段切换：probe/load/各 pass 名/write
    fn item(&self, done: u64, total: u64, note: &str);   // 细粒度心跳（按章/按页）
    fn is_cancelled(&self) -> bool { false }             // 取消检查点
}
```

- `stage` 用字符串而非枚举：**自定义 pass 也要能报进度**，不因扩展改契约类型（开闭原则的细节体现）。
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
}
```

各字段的默认值以需求定稿 v1.0 为准。原则：**配置只描述"要什么"，不描述"怎么做"**——不存在"跳过某个 pass"这类过程性开关，扩展行为通过 Converter 的装配点完成。

## 稳定性与变更流程

- 契约 crate 的全部 pub 项就是 semver 契约面。
- 破坏性变更流程：先改本文档 → 同步更新受影响的组件文档与 golden → 代码实现 → 版本号 minor 递增。禁止"代码先行、文档追认"。
- IR 字段演进**只加不改**：新增字段带默认值，不重命名、不改已有字段语义。
- Outline / BuildReport 是面向最终用户的展示数据（GUI、服务、dry-run 都直接消费），稳定性要求最高。
