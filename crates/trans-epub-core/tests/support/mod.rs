//! 测试 helper：fixtures/golden 路径、转换入口（MemVfs + MockFetcher）、
//! zip 解包与文本断言工具。本模块无 #[test]；分层地图见 tests/main.rs。

use std::io::Read;
use std::sync::Arc;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::progress::NoProgress;
use trans_epub_contract::source::{FetchedPage, Source};
use trans_epub_contract::testing::{MemVfs, MockFetcher};
use trans_epub_contract::writer::BuildOutput;
use trans_epub_core::{Converter, Io};

pub(crate) const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
pub(crate) const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");

const REMOTE_PNG_URL: &str = "https://example.com/remote.png";

pub(crate) fn png_bytes() -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}/flat/img/diagram.png")).expect("fixture png exists")
}

/// 每本样书的配置（golden 字节比对与验收门共用同一口径）。
pub(crate) fn fixture_config(name: &str) -> ResolvedConfig {
    let mut config = ResolvedConfig::default();
    if name == "tree" {
        config.book.cover = Some("cover.png".to_string());
    }
    config
}

pub(crate) fn convert(name: &str) -> BuildOutput {
    convert_with(name, fixture_config(name))
}

fn convert_with(name: &str, config: ResolvedConfig) -> BuildOutput {
    let vfs = MemVfs::from_dir(format!("{FIXTURES}/{name}")).expect("fixture dir readable");
    let fetcher = MockFetcher::new().insert(REMOTE_PNG_URL, png_bytes(), "image/png");
    let io = Io {
        fetcher: Arc::new(fetcher),
        progress: Arc::new(NoProgress),
    };
    Converter::new()
        .convert(&Source::Vfs(Arc::new(vfs)), &config, &io)
        .expect("conversion succeeds")
}

// ---------------------------------------------------------------------------
// web（MVP3）：Pages 源 → 抽取 → 改写（30-frontends.md 的 fixture 要求）
// ---------------------------------------------------------------------------

pub(crate) fn web_page(file: &str, url: &str, content_type: &str) -> FetchedPage {
    FetchedPage::from_bytes(
        url.parse().expect("fixture url"),
        std::fs::read(format!("{FIXTURES}/web/{file}")).expect("fixture page readable"),
        content_type,
    )
}

/// web 样书：3 页有效（默认链）+ 1 重复 URL + 1 非 HTML 资源。
pub(crate) fn convert_web() -> BuildOutput {
    let pages = vec![
        web_page(
            "home.html",
            "https://docs.example.dev/",
            "text/html; charset=utf-8",
        ),
        web_page(
            "guide/install.html",
            "https://docs.example.dev/guide/install.html",
            "text/html",
        ),
        web_page(
            "guide/api.html",
            "https://docs.example.dev/guide/api.html",
            "text/html",
        ),
        // 重复 URL：保留首个 + DuplicateSource warning
        web_page("home.html", "https://docs.example.dev/", "text/html"),
        // 非 text/html：跳过 + warning
        FetchedPage::from_bytes(
            "https://docs.example.dev/feed.txt".parse().unwrap(),
            b"plain".to_vec(),
            "text/plain",
        ),
    ];
    let fetcher = MockFetcher::new()
        .insert(
            "https://docs.example.dev/img/arch.png",
            png_bytes(),
            "image/png",
        )
        .insert(
            "https://cdn.example.dev/diagram.png",
            png_bytes(),
            "image/png",
        );
    let io = Io {
        fetcher: Arc::new(fetcher),
        progress: Arc::new(NoProgress),
    };
    Converter::new()
        .convert(&Source::Pages(pages), &ResolvedConfig::default(), &io)
        .expect("web conversion succeeds")
}

pub(crate) fn zip_entries(output: &BuildOutput) -> Vec<(String, bool, Vec<u8>)> {
    let reader = std::io::Cursor::new(&output.bytes);
    let mut archive = zip::ZipArchive::new(reader).expect("valid epub zip");
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).expect("index in range");
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("entry readable");
        let stored = file.compression() == zip::CompressionMethod::Stored;
        out.push((file.name().to_string(), stored, bytes));
    }
    out
}

fn entry_bytes(output: &BuildOutput, name: &str) -> Vec<u8> {
    zip_entries(output)
        .into_iter()
        .find(|(n, _, _)| n == name)
        .unwrap_or_else(|| panic!("missing zip entry {name}"))
        .2
}

pub(crate) fn utf8(name: &str, output: &BuildOutput) -> String {
    String::from_utf8(entry_bytes(output, name)).expect("utf-8 entry")
}

/// 去掉 HTML 标签只留文本：高亮 span 会把代码块内连续文本逐 token
/// 切开，内容断言比对纯文本更稳。
pub(crate) fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

pub(crate) fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("trans-epub-epubcheck-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
