//! trans-epub CLI 薄壳（docs/architecture/60-cli.md）：零业务逻辑，只做
//! 装配与注入——参数解析 → 配置合并（TOML < front matter < flags，前两层
//! 中的 front matter 归前端在 load 应用）→ Source 构造（路径族 FsVfs /
//! 采集族 trans-epub-fetch）→ Io 接线 → dry-run / 落盘 / 报告 / 退出码。
//!
//! 判断 CLI 是否越界的标准：删掉本 crate 换成 HTTP handler，逻辑能否
//! 整体搬进服务？能，就是薄壳。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use trans_epub_core::contract::config::ResolvedConfig;
use trans_epub_core::contract::error::BuildError;
use trans_epub_core::contract::frontend::OutlineItem;
use trans_epub_core::contract::progress::ProgressSink;
use trans_epub_core::contract::source::{FileEntry, RelPath, Source, Vfs, VfsError};
use trans_epub_core::contract::writer::BuildReport;
use trans_epub_core::{Converter, Io};
use trans_epub_fetch::{DiscoverCfg, Discovered, PoliteConfig, PoliteFetcher, discover};
use url::Url;

/// 退出码约定（60-cli.md）：0 成功；1 转换致命失败或 --strict 下 warning；
/// 2 用法/环境错误。
pub const EXIT_OK: u8 = 0;
pub const EXIT_FATAL: u8 = 1;
pub const EXIT_USAGE: u8 = 2;

#[derive(Parser, Debug)]
#[command(
    name = "trans-epub",
    version,
    about = "md/mdBook/静态文档站 → EPUB（三前端已落地；文档站经 crawl 采集）"
)]
pub struct Cli {
    /// 输入：markdown 文件或包含 markdown 的目录（无子命令时）
    pub path: Option<PathBuf>,
    /// 通用输出/配置旗标（路径族与 crawl 共用）
    #[command(flatten)]
    pub common: CommonArgs,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 抓取静态文档站成书：URL 起始页 / sitemap.xml / URL 列表文件
    Crawl(CrawlArgs),
}

#[derive(Args, Debug)]
pub struct CrawlArgs {
    /// 起始 URL（http(s)，非 .xml 结尾）· sitemap URL（.xml 结尾）· 本地 sitemap.xml / list.txt
    pub start: String,
    /// BFS 深度上限（起始页为第 0 层）
    #[arg(long, default_value_t = 3)]
    pub depth: u32,
    /// 页面数安全帽
    #[arg(long, default_value_t = 500)]
    pub max_pages: u32,
    /// include glob（URL path 匹配，可重复）
    #[arg(long)]
    pub include: Vec<String>,
    /// exclude glob（命中即丢弃，可重复）
    #[arg(long)]
    pub exclude: Vec<String>,
    /// 每 host 最小请求间隔（毫秒）
    #[arg(long, default_value_t = 1000)]
    pub rate_ms: u64,
    /// 不遵守 robots.txt（自负其责）
    #[arg(long)]
    pub ignore_robots: bool,
    /// 正文抽取预设：mkdocs / docusaurus / vitepress / gitbook
    #[arg(long)]
    pub preset: Option<String>,
    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(Args, Debug)]
pub struct CommonArgs {
    /// 输出 .epub 路径（默认：当前目录/{输入名}.epub）
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// TOML 配置文件（字段与 ResolvedConfig 一一对应）
    #[arg(short = 'c', long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// 书名（覆盖配置文件）
    #[arg(long)]
    pub title: Option<String>,
    /// 作者（可重复，覆盖配置文件）
    #[arg(long = "author")]
    pub authors: Vec<String>,
    /// 语言（BCP-47）
    #[arg(long)]
    pub lang: Option<String>,
    /// 封面图（相对源根的路径或 URL）
    #[arg(long)]
    pub cover: Option<String>,
    /// 离线模式：不拉取远程图片（assets.remote = false）
    #[arg(long)]
    pub no_remote_images: bool,
    /// 代码长行策略
    #[arg(long, value_enum, default_value = "pre-wrap")]
    pub wrap: WrapArg,
    /// 硬折行列宽（--wrap hard 时生效）
    #[arg(long, default_value_t = 92)]
    pub wrap_width: u32,
    /// 自定义 CSS 文件（追加在内置主题之后）
    #[arg(long, value_name = "FILE")]
    pub extra_css: Option<PathBuf>,
    /// 内嵌字体（TransEpub Mono：Maple Mono CN 子集，OFL-1.1 义务随书）
    #[arg(long)]
    pub embed_fonts: bool,
    /// 字体子集模式（preset = 内置 8105 字预子集；exact = 按书内字符，
    /// builtin 源需联网下载钉版全量字体）
    #[arg(long, value_enum)]
    pub font_subset: Option<FontSubsetArg>,
    /// 用户字体文件（ttf；配合 --embed-fonts 使用）
    #[arg(long, value_name = "FILE")]
    pub font_file: Option<PathBuf>,
    /// 报告格式
    #[arg(long, value_enum, default_value = "text")]
    pub report: ReportArg,
    /// 任何 warning 都视为失败
    #[arg(long)]
    pub strict: bool,
    /// 只输出探测到的页面/章节清单与顺序，不出书
    #[arg(long)]
    pub dry_run: bool,
    /// 静默：不输出进度与文本报告
    #[arg(short = 'q', long)]
    pub quiet: bool,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapArg {
    #[value(name = "pre-wrap")]
    PreWrap,
    #[value(name = "hard")]
    Hard,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum ReportArg {
    Text,
    Json,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSubsetArg {
    Preset,
    Exact,
}

/// 入口逻辑（main 只做解析与 exit）。返回进程退出码。
pub fn run(cli: &Cli) -> u8 {
    match run_inner(cli) {
        Ok(code) => code,
        Err(usage_error) => {
            eprintln!("trans-epub: {usage_error}");
            EXIT_USAGE
        }
    }
}

fn run_inner(cli: &Cli) -> Result<u8, String> {
    match &cli.command {
        Some(Command::Crawl(args)) => run_crawl(args),
        None => {
            let Some(path) = &cli.path else {
                return Err(
                    "input path required (or use: trans-epub crawl <start-url | sitemap.xml | list.txt>)"
                        .into(),
                );
            };
            run_convert(path, &cli.common)
        }
    }
}

// ---------------------------------------------------------------------------
// 路径族：Vfs 源
// ---------------------------------------------------------------------------

fn run_convert(path: &Path, common: &CommonArgs) -> Result<u8, String> {
    if !path.exists() {
        return Err(format!("input path not found: {}", path.display()));
    }
    let mut config = load_config(common.config.as_deref())?;
    apply_flags(&mut config, common)?;

    let vfs = FsVfs::new(path).map_err(|e| format!("open input: {e}"))?;
    let source = Source::Vfs(Arc::new(vfs));
    let converter = Converter::new();

    if common.dry_run {
        return dry_run(&converter, &source, &config);
    }

    let progress = Arc::new(TerminalProgress::new(common.quiet));
    let fetcher = PoliteFetcher::with_max_bytes(config.assets.max_file_bytes);
    let io = Io {
        fetcher: Arc::new(fetcher),
        progress: progress.clone(),
    };
    finish_convert(
        &converter,
        source,
        &config,
        common,
        &default_output(path, common.output.as_deref()),
        &io,
    )
}

// ---------------------------------------------------------------------------
// 采集族：crawl 子命令 → fetch crate 发现/抓取 → Pages 源
// ---------------------------------------------------------------------------

fn run_crawl(args: &CrawlArgs) -> Result<u8, String> {
    let mut config = load_config(args.common.config.as_deref())?;
    apply_flags(&mut config, &args.common)?;
    if let Some(preset) = &args.preset {
        config.web.preset = Some(preset.clone());
    }

    let fetcher = PoliteFetcher::new(PoliteConfig {
        min_interval: Duration::from_millis(args.rate_ms),
        max_bytes: config.assets.max_file_bytes,
        respect_robots: !args.ignore_robots,
        ..PoliteConfig::default()
    });
    let discover_cfg = DiscoverCfg {
        max_depth: args.depth,
        max_pages: args.max_pages as usize,
        include: args.include.clone(),
        exclude: args.exclude.clone(),
    };

    let progress = Arc::new(TerminalProgress::new(args.common.quiet));
    progress.stage("discover");
    let mut discovered =
        dispatch_discovery(&fetcher, &args.start, &discover_cfg, progress.as_ref())?;
    for w in &discovered.warnings {
        eprintln!("trans-epub: warning [{}]: {}", w.code, w.message);
    }

    if args.common.dry_run {
        // 发现完成后即打印清单停止（40-fetch.md 的 dry-run 语义）
        println!(
            "trans-epub (crawl dry-run): {} page(s) discovered, in book order:",
            discovered.urls.len()
        );
        for url in &discovered.urls {
            println!("- {url}");
        }
        return Ok(EXIT_OK);
    }

    // list / sitemap 只发现了 URL 清单，页面体按需抓取（BFS 已顺带抓取）
    if discovered.pages.is_empty() && !discovered.urls.is_empty() {
        progress.stage("fetch");
        discovered.fetch_pages(&fetcher, &discover_cfg, Some(progress.as_ref()));
        for w in &discovered.warnings {
            eprintln!("trans-epub: warning [{}]: {}", w.code, w.message);
        }
    }
    if discovered.pages.is_empty() {
        return Err("no pages discovered".into());
    }

    let source = Source::Pages(std::mem::take(&mut discovered.pages));
    let converter = Converter::new();
    let out_name = default_output(Path::new(&args.start), args.common.output.as_deref());
    let io = Io {
        fetcher: Arc::new(fetcher),
        progress: progress.clone(),
    };
    finish_convert(&converter, source, &config, &args.common, &out_name, &io)
}

/// 输入判别（60-cli.md）：http(s) URL 且 path 以 .xml 结尾 → 远程 sitemap；
/// 其他 http(s) URL → BFS 起始页；本地文件按扩展名分 sitemap / list。
fn dispatch_discovery(
    fetcher: &PoliteFetcher,
    start: &str,
    cfg: &DiscoverCfg,
    progress: &dyn ProgressSink,
) -> Result<Discovered, String> {
    if let Ok(url) = Url::parse(start) {
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(format!("unsupported URL scheme: {}", url.scheme()));
        }
        if url.path().ends_with(".xml") {
            return Ok(discover::from_sitemap(fetcher, &url, Some(progress)));
        }
        return Ok(discover::crawl(fetcher, &url, cfg, Some(progress)));
    }
    let path = Path::new(start);
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xml"))
    {
        Ok(discover::from_sitemap_local(
            fetcher,
            &bytes,
            Some(progress),
        ))
    } else {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Ok(discover::from_list(&text))
    }
}

// ---------------------------------------------------------------------------
// 共用尾段：convert → 落盘 / 报告 / 退出码
// ---------------------------------------------------------------------------

fn finish_convert(
    converter: &Converter,
    source: Source,
    config: &ResolvedConfig,
    common: &CommonArgs,
    out_path: &Path,
    io: &Io,
) -> Result<u8, String> {
    match converter.convert(&source, config, io) {
        Err(BuildError::Source(msg)) => {
            eprintln!("trans-epub: {msg}");
            Ok(EXIT_USAGE)
        }
        Err(err) => {
            eprintln!("trans-epub: {err}");
            Ok(EXIT_FATAL)
        }
        Ok(output) => {
            if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("trans-epub: create output dir: {e}");
                    return Ok(EXIT_FATAL);
                }
            }
            if let Err(e) = std::fs::write(out_path, &output.bytes) {
                eprintln!("trans-epub: write {}: {e}", out_path.display());
                return Ok(EXIT_FATAL);
            }
            if !common.quiet {
                eprintln!(
                    "trans-epub: wrote {} ({} chapters, {} warnings)",
                    out_path.display(),
                    output.report.stats.chapters,
                    output.report.warnings.len()
                );
            }
            match common.report {
                ReportArg::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&output.report)
                            .map_err(|e| format!("serialize report: {e}"))?
                    );
                }
                ReportArg::Text => {
                    if !common.quiet {
                        eprintln!("{}", render_report_text(&output.report));
                    }
                }
            }
            if common.strict && !output.report.warnings.is_empty() {
                return Ok(EXIT_FATAL);
            }
            Ok(EXIT_OK)
        }
    }
}

fn dry_run(converter: &Converter, source: &Source, config: &ResolvedConfig) -> Result<u8, String> {
    let matched = converter.probe(source, config);
    let Some((name, outline)) = matched.first() else {
        eprintln!(
            "trans-epub: no frontend matched the input (mdbook / loose markdown / crawled pages supported)"
        );
        return Ok(EXIT_USAGE);
    };
    println!(
        "trans-epub (dry-run): frontend = {name}, structure = {}",
        outline.kind
    );
    for warning in &outline.warnings {
        println!("  warning [{}]: {}", warning.code, warning.message);
    }
    for item in &outline.items {
        print_outline(item, 0);
    }
    Ok(EXIT_OK)
}

fn print_outline(item: &OutlineItem, depth: usize) {
    println!("{}- {} ({})", "  ".repeat(depth), item.title, item.source);
    for child in &item.children {
        print_outline(child, depth + 1);
    }
}

// ---------------------------------------------------------------------------
// 配置合并（TOML < front matter < CLI flags 的壳层一半）
// ---------------------------------------------------------------------------

fn load_config(path: Option<&Path>) -> Result<ResolvedConfig, String> {
    match path {
        None => Ok(ResolvedConfig::default()),
        Some(p) => {
            let text = std::fs::read_to_string(p)
                .map_err(|e| format!("read config {}: {e}", p.display()))?;
            let mut config: ResolvedConfig =
                toml::from_str(&text).map_err(|e| format!("parse config {}: {e}", p.display()))?;
            // [fonts.source] user 的 path 在壳层解析成 data（core 只认 data）
            if let trans_epub_core::contract::config::FontSource::User { path, data, .. } =
                &mut config.fonts.source
            {
                if data.is_none() {
                    if let Some(rel) = path.as_deref() {
                        let font_path = if Path::new(rel).is_absolute() {
                            PathBuf::from(rel)
                        } else {
                            p.parent().unwrap_or(Path::new(".")).join(rel)
                        };
                        let bytes = std::fs::read(&font_path)
                            .map_err(|e| format!("read font {}: {e}", font_path.display()))?;
                        *data = Some(bytes);
                    }
                }
            }
            Ok(config)
        }
    }
}

fn apply_flags(config: &mut ResolvedConfig, common: &CommonArgs) -> Result<(), String> {
    if let Some(title) = &common.title {
        config.book.title = Some(title.clone());
    }
    if !common.authors.is_empty() {
        config.book.authors = common.authors.clone();
    }
    if let Some(lang) = &common.lang {
        config.book.language = Some(lang.clone());
    }
    if let Some(cover) = &common.cover {
        config.book.cover = Some(cover.clone());
    }
    if common.no_remote_images {
        config.assets.remote = false;
    }
    config.wrap.mode = match common.wrap {
        WrapArg::PreWrap => trans_epub_core::contract::config::WrapMode::PreWrap,
        WrapArg::Hard => trans_epub_core::contract::config::WrapMode::Hard {
            width: common.wrap_width,
        },
    };
    if let Some(css_path) = &common.extra_css {
        config.theme.extra_css = std::fs::read_to_string(css_path)
            .map_err(|e| format!("read extra css {}: {e}", css_path.display()))?;
    }
    if common.embed_fonts {
        config.fonts.embed = true;
    }
    if let Some(subset) = common.font_subset {
        config.fonts.subset = match subset {
            FontSubsetArg::Preset => trans_epub_core::contract::config::FontSubsetMode::Preset,
            FontSubsetArg::Exact => trans_epub_core::contract::config::FontSubsetMode::Exact,
        };
    }
    if let Some(font_path) = &common.font_file {
        let data = std::fs::read(font_path)
            .map_err(|e| format!("read font {}: {e}", font_path.display()))?;
        config.fonts.source = trans_epub_core::contract::config::FontSource::User {
            path: None,
            data: Some(data),
            family: None,
        };
        if !config.fonts.embed {
            eprintln!("note: --font-file 已指定但未开启内嵌（--embed-fonts），字体不会嵌入产物");
        }
    }
    Ok(())
}

fn default_output(input: &Path, explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "book".to_string());
    PathBuf::from(format!("{stem}.epub"))
}

// ---------------------------------------------------------------------------
// 磁盘 Vfs（契约 Vfs 的第一个实现）
// ---------------------------------------------------------------------------

/// 目录树（或单文件，根 = 其父目录但 walk 只返回该文件）的磁盘实现。
pub struct FsVfs {
    root: PathBuf,
    single_file: Option<String>,
}

impl FsVfs {
    pub fn new(path: &Path) -> std::io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        if meta.is_file() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| std::io::Error::other("input file has no file name"))?;
            let root = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            Ok(Self {
                root,
                single_file: Some(name),
            })
        } else {
            Ok(Self {
                root: path.to_path_buf(),
                single_file: None,
            })
        }
    }

    fn real_path(&self, path: &RelPath) -> PathBuf {
        // RelPath 已保证无 `..`/盘符/反斜杠，join 安全
        self.root.join(path.as_str())
    }
}

impl Vfs for FsVfs {
    fn read(&self, path: &RelPath) -> Result<Vec<u8>, VfsError> {
        std::fs::read(self.real_path(path)).map_err(|e| VfsError::Io {
            path: path.as_str().to_string(),
            source: e,
        })
    }

    fn exists(&self, path: &RelPath) -> bool {
        self.real_path(path).exists()
    }

    fn walk(&self) -> Result<Vec<FileEntry>, VfsError> {
        let mut out = Vec::new();
        if let Some(name) = &self.single_file {
            let path = self.root.join(name);
            let size = std::fs::metadata(&path)
                .map_err(|e| VfsError::Io {
                    path: name.clone(),
                    source: e,
                })?
                .len();
            if let Ok(rel) = RelPath::new(name) {
                out.push(FileEntry { path: rel, size });
            }
            return Ok(out);
        }
        for entry in walkdir::WalkDir::new(&self.root).sort_by_file_name() {
            let entry = entry.map_err(|e| VfsError::Other(format!("walk: {e}")))?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&self.root) else {
                continue;
            };
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let Ok(rel) = RelPath::new(&rel) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(entry.path()) else {
                continue;
            };
            out.push(FileEntry {
                path: rel,
                size: meta.len(),
            });
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 终端 ProgressSink（stage 行 + indicatif 按章心跳；Ctrl-C 置取消位）
// ---------------------------------------------------------------------------

pub struct TerminalProgress {
    quiet: bool,
    cancelled: Arc<AtomicBool>,
    bar: Mutex<Option<ProgressBar>>,
}

impl TerminalProgress {
    pub fn new(quiet: bool) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        // Ctrl-C 触发协作式取消（失败则静默跳过，例如重复注册）
        let _ = ctrlc::try_set_handler({
            let cancelled = cancelled.clone();
            move || cancelled.store(true, Ordering::SeqCst)
        });
        Self {
            quiet,
            cancelled,
            bar: Mutex::new(None),
        }
    }
}

impl ProgressSink for TerminalProgress {
    fn stage(&self, name: &str) {
        if self.quiet {
            return;
        }
        if let Ok(mut guard) = self.bar.lock() {
            if let Some(bar) = guard.take() {
                bar.finish_and_clear();
            }
        }
        eprintln!("==> {name}");
    }

    fn item(&self, done: u64, total: u64, note: &str) {
        if self.quiet {
            return;
        }
        if let Ok(mut guard) = self.bar.lock() {
            let bar = guard.get_or_insert_with(|| {
                let bar = ProgressBar::new(total).with_style(
                    ProgressStyle::with_template("{bar:28} {pos}/{len} {msg}")
                        .expect("valid indicatif template"),
                );
                // 非 tty（重定向/测试）时 indicatif 自动隐藏
                bar.set_draw_target(ProgressDrawTarget::stderr());
                bar
            });
            bar.set_length(total);
            bar.set_position(done.min(total));
            bar.set_message(note.to_string());
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

// ---------------------------------------------------------------------------
// 报告
// ---------------------------------------------------------------------------

pub fn render_report_text(report: &BuildReport) -> String {
    let s = &report.stats;
    let mut out = format!(
        "report: {} chapters, {} assets embedded, {} assets missing, {} links rewritten, {} unresolved",
        s.chapters, s.assets_embedded, s.assets_missing, s.links_rewritten, s.links_unresolved
    );
    if !report.warnings.is_empty() {
        out.push_str(&format!("\nwarnings ({}):", report.warnings.len()));
        for w in &report.warnings {
            let source = w
                .source
                .as_ref()
                .map(|k| format!(" {k}:"))
                .unwrap_or_default();
            out.push_str(&format!("\n  [{}]{} {}", w.code, source, w.message));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_flags() {
        let cli = Cli::try_parse_from([
            "trans-epub",
            "book/",
            "-o",
            "out.epub",
            "--title",
            "T",
            "--author",
            "A",
            "--author",
            "B",
            "--wrap",
            "hard",
            "--wrap-width",
            "80",
            "--strict",
            "--report",
            "json",
            "--no-remote-images",
            "--dry-run",
        ])
        .expect("valid flags");
        assert_eq!(cli.common.output, Some(PathBuf::from("out.epub")));
        assert_eq!(cli.common.authors, vec!["A", "B"]);
        assert_eq!(cli.common.wrap, WrapArg::Hard);
        assert!(cli.common.strict && cli.common.dry_run && cli.common.no_remote_images);
        assert!(cli.path.as_deref() == Some(Path::new("book/")));
        assert!(cli.command.is_none());
    }

    #[test]
    fn crawl_subcommand_parses() {
        let cli = Cli::try_parse_from([
            "trans-epub",
            "crawl",
            "https://docs.example.dev/",
            "--depth",
            "2",
            "--max-pages",
            "50",
            "--include",
            "guide/*",
            "--exclude",
            "blog/*",
            "--rate-ms",
            "500",
            "--ignore-robots",
            "--preset",
            "mkdocs",
            "--title",
            "Site Book",
        ])
        .expect("valid crawl flags");
        let Some(Command::Crawl(args)) = &cli.command else {
            panic!("crawl 子命令必须解析");
        };
        assert_eq!(args.start, "https://docs.example.dev/");
        assert_eq!(args.depth, 2);
        assert_eq!(args.max_pages, 50);
        assert_eq!(args.include, vec!["guide/*"]);
        assert_eq!(args.exclude, vec!["blog/*"]);
        assert_eq!(args.rate_ms, 500);
        assert!(args.ignore_robots);
        assert_eq!(args.preset.as_deref(), Some("mkdocs"));
        assert_eq!(args.common.title.as_deref(), Some("Site Book"));
    }

    #[test]
    fn sitemap_url_is_recognized_in_dispatch() {
        // dispatch 判别逻辑在 dispatch_discovery 内联；这里验证 URL 形态判别
        // 依赖的规则本身（.xml 后缀 → sitemap），完整链路见 e2e 测试
        let url = Url::parse("https://a.dev/sitemap.xml").unwrap();
        assert!(url.path().ends_with(".xml"));
        let url = Url::parse("https://a.dev/guide/").unwrap();
        assert!(!url.path().ends_with(".xml"));
    }

    #[test]
    fn flags_override_config() {
        let cli = Cli::try_parse_from([
            "trans-epub",
            "x",
            "--title",
            "From Flag",
            "--no-remote-images",
            "--wrap",
            "hard",
            "--wrap-width",
            "72",
        ])
        .unwrap();
        let mut config = ResolvedConfig::default();
        config.book.title = Some("From Toml".into());
        apply_flags(&mut config, &cli.common).unwrap();
        assert_eq!(config.book.title.as_deref(), Some("From Flag"));
        assert!(!config.assets.remote);
        assert_eq!(
            config.wrap.mode,
            trans_epub_core::contract::config::WrapMode::Hard { width: 72 }
        );
    }

    #[test]
    fn partial_toml_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("t.toml");
        std::fs::write(&cfg_path, "[assets]\nremote = false\n").unwrap();
        let config = load_config(Some(&cfg_path)).unwrap();
        assert!(!config.assets.remote);
        assert!(config.markdown.gfm, "未覆盖项回落默认值");
    }

    #[test]
    fn command_has_expected_surface() {
        Cli::command().debug_assert();
    }
}
