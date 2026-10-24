//! trans-epub CLI 薄壳（docs/architecture/60-cli.md）：零业务逻辑，只做
//! 装配与注入——参数解析 → 配置合并（TOML < front matter < flags，前两层
//! 中的 front matter 归前端在 load 应用）→ Source 构造 → Io 接线 →
//! dry-run / 落盘 / 报告 / 退出码。
//!
//! 判断 CLI 是否越界的标准：删掉本 crate 换成 HTTP handler，逻辑能否
//! 整体搬进服务？能，就是薄壳。

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use trans_epub_core::contract::config::{ResolvedConfig, WrapMode};
use trans_epub_core::contract::error::BuildError;
use trans_epub_core::contract::frontend::OutlineItem;
use trans_epub_core::contract::progress::ProgressSink;
use trans_epub_core::contract::source::{
    FetchError, Fetcher, FileEntry, RelPath, Source, Vfs, VfsError,
};
use trans_epub_core::contract::writer::BuildReport;
use trans_epub_core::{Converter, Io};
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
    about = "md/文档站 → EPUB（MVP1：散装 markdown）"
)]
pub struct Cli {
    /// 输入：markdown 文件或包含 markdown 的目录
    pub path: PathBuf,
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
    /// 报告格式
    #[arg(long, value_enum, default_value = "text")]
    pub report: ReportArg,
    /// 任何 warning 都视为失败
    #[arg(long)]
    pub strict: bool,
    /// 只探测结构（Outline），不出书
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
    if !cli.path.exists() {
        return Err(format!("input path not found: {}", cli.path.display()));
    }
    let mut config = load_config(cli.config.as_deref())?;
    apply_flags(&mut config, cli)?;

    let vfs = FsVfs::new(&cli.path).map_err(|e| format!("open input: {e}"))?;
    let source = Source::Vfs(Arc::new(vfs));
    let converter = Converter::new();

    // ---- dry-run：只 probe，树状打印 Outline ----
    if cli.dry_run {
        return dry_run(&converter, &source, &config);
    }

    let progress = Arc::new(TerminalProgress::new(cli.quiet));
    let fetcher = HttpFetcher::new(config.assets.max_file_bytes);
    let io = Io {
        fetcher: Arc::new(fetcher),
        progress: progress.clone(),
    };

    match converter.convert(&source, &config, &io) {
        Err(BuildError::Source(msg)) => {
            eprintln!("trans-epub: {msg}");
            Ok(EXIT_USAGE)
        }
        Err(err) => {
            eprintln!("trans-epub: {err}");
            Ok(EXIT_FATAL)
        }
        Ok(output) => {
            let out_path = default_output(&cli.path, cli.output.as_deref());
            if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("trans-epub: create output dir: {e}");
                    return Ok(EXIT_FATAL);
                }
            }
            if let Err(e) = std::fs::write(&out_path, &output.bytes) {
                eprintln!("trans-epub: write {}: {e}", out_path.display());
                return Ok(EXIT_FATAL);
            }
            if !cli.quiet {
                eprintln!(
                    "trans-epub: wrote {} ({} chapters, {} warnings)",
                    out_path.display(),
                    output.report.stats.chapters,
                    output.report.warnings.len()
                );
            }
            match cli.report {
                ReportArg::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&output.report)
                            .map_err(|e| format!("serialize report: {e}"))?
                    );
                }
                ReportArg::Text => {
                    if !cli.quiet {
                        eprintln!("{}", render_report_text(&output.report));
                    }
                }
            }
            if cli.strict && !output.report.warnings.is_empty() {
                return Ok(EXIT_FATAL);
            }
            Ok(EXIT_OK)
        }
    }
}

fn dry_run(converter: &Converter, source: &Source, config: &ResolvedConfig) -> Result<u8, String> {
    let matched = converter.probe(source, config);
    let Some((name, outline)) = matched.first() else {
        eprintln!("trans-epub: no frontend matched the input (MVP1: loose markdown only)");
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
            toml::from_str(&text).map_err(|e| format!("parse config {}: {e}", p.display()))
        }
    }
}

fn apply_flags(config: &mut ResolvedConfig, cli: &Cli) -> Result<(), String> {
    if let Some(title) = &cli.title {
        config.book.title = Some(title.clone());
    }
    if !cli.authors.is_empty() {
        config.book.authors = cli.authors.clone();
    }
    if let Some(lang) = &cli.lang {
        config.book.language = Some(lang.clone());
    }
    if let Some(cover) = &cli.cover {
        config.book.cover = Some(cover.clone());
    }
    if cli.no_remote_images {
        config.assets.remote = false;
    }
    config.wrap.mode = match cli.wrap {
        WrapArg::PreWrap => WrapMode::PreWrap,
        WrapArg::Hard => WrapMode::Hard {
            width: cli.wrap_width,
        },
    };
    if let Some(css_path) = &cli.extra_css {
        config.theme.extra_css = std::fs::read_to_string(css_path)
            .map_err(|e| format!("read extra css {}: {e}", css_path.display()))?;
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
// 最小 Fetcher（reqwest 薄包装；MVP3 平移进 trans-epub-fetch）
// ---------------------------------------------------------------------------

pub struct HttpFetcher {
    client: reqwest::blocking::Client,
    max_bytes: u64,
}

impl HttpFetcher {
    pub fn new(max_bytes: u64) -> Self {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("trans-epub/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { client, max_bytes }
    }
}

impl Fetcher for HttpFetcher {
    fn fetch(
        &self,
        url: &Url,
    ) -> Result<trans_epub_core::contract::source::FetchedPage, FetchError> {
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(FetchError::Denied(format!(
                "scheme not allowed: {}",
                url.scheme()
            )));
        }
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .map_err(|e| FetchError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(FetchError::HttpStatus {
                status,
                url: url.to_string(),
            });
        }
        if let Some(len) = response.content_length() {
            if len > self.max_bytes {
                return Err(FetchError::TooLarge(format!(
                    "{len} bytes (limit {}) for {url}",
                    self.max_bytes
                )));
            }
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let mut body = Vec::new();
        let mut limited = (&mut response).take(self.max_bytes + 1);
        limited
            .read_to_end(&mut body)
            .map_err(|e| FetchError::Network(e.to_string()))?;
        if body.len() as u64 > self.max_bytes {
            return Err(FetchError::TooLarge(format!(
                "over {} bytes for {url}",
                self.max_bytes
            )));
        }
        Ok(trans_epub_core::contract::source::FetchedPage::from_bytes(
            url.clone(),
            body,
            content_type,
        ))
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
        assert_eq!(cli.output, Some(PathBuf::from("out.epub")));
        assert_eq!(cli.authors, vec!["A", "B"]);
        assert_eq!(cli.wrap, WrapArg::Hard);
        assert!(cli.strict && cli.dry_run && cli.no_remote_images);
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
        apply_flags(&mut config, &cli).unwrap();
        assert_eq!(config.book.title.as_deref(), Some("From Flag"));
        assert!(!config.assets.remote);
        assert_eq!(config.wrap.mode, WrapMode::Hard { width: 72 });
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
