//! 静态网站前端（MVP3，已落地）：正文抽取（选择器 + 内置预设，language
//! class 必须存活）与相对 URL 规范化；页面发现与抓取属 trans-epub-fetch，
//! 本前端只吃 [`Source::Pages`]（已抓取页面，顺序即书序）。
//!
//! 抽取语义（30-frontends.md）：
//! - 选择器链：`WebCfg.selectors` > `WebCfg.preset`（mkdocs / docusaurus /
//!   vitepress / gitbook 内置链）> 默认链 `main → article → .content →
//!   .markdown-body → body` 兜底；链内取**首个命中选择器的首个匹配**。
//! - 清理：内置剔除 `nav/footer/aside/script/style/noscript/form/button`
//!   加 `WebCfg.strip`；序列化走 XML 安全规则，script/style/注释从 load
//!   起就不进章 HTML。
//! - **代码块 language class 必须存活**（percollate 丢高亮的根因教训）：
//!   抽取器保守，只删 chrome，不动 pre/code。
//! - 章名：正文首个 h1 → `<title>` → URL path 末段。
//! - 编码：content_type charset > HTML meta 嗅探 > UTF-8（BOM 优先，
//!   encoding_rs 行为）；有损转换记 `EncodingLossy` warning。
//! - 重复 URL → `DuplicateSource` warning 保留首个；非 text/html →
//!   warning 跳过。
//! - 引用保持原始形式（义务清单第 3 条）：`img/src` 仅**登记**（以页面
//!   URL 为基规范化为绝对 URL：http(s) → Remote、data: → Owned），
//!   `a[href]` 原样留给 link-rewrite 的 Url 世界解析。

use std::collections::HashSet;
use std::sync::OnceLock;

use comrak::Anchorizer;
use ego_tree::NodeId;
use scraper::{Html, Selector};
use trans_epub_contract::config::{ResolvedConfig, WebCfg};
use trans_epub_contract::error::{BuildError, Warning, WarningCode};
use trans_epub_contract::frontend::{Frontend, LoadEnv, Outline, OutlineItem, Probe};
use trans_epub_contract::ir::{Asset, AssetData, BookDraft, ChapterId, SourceKey, TocItem};
use trans_epub_contract::progress::ProgressSink;
use trans_epub_contract::source::{FetchedPage, Source};
use url::Url;

use crate::dom;
use crate::frontends::md::decode_data_uri;
use crate::refs;
use crate::reporter;

pub struct WebFrontend;

/// 预处理后的页面（去重 + content-type 过滤完成）。
struct PreparedPage {
    url: Url,
    body: Vec<u8>,
    content_type: String,
}

/// load 一页的产物：章节 id（跳过为 None）+ 文档标题（书名兜底线索）。
struct LoadedPage {
    chapter: Option<ChapterId>,
    doc_title: Option<String>,
}

impl Frontend for WebFrontend {
    fn name(&self) -> &'static str {
        "web"
    }

    fn probe(&self, source: &Source, config: &ResolvedConfig) -> Probe {
        let Source::Pages(pages) = source else {
            return Probe::NoMatch;
        };
        if pages.is_empty() {
            return Probe::NoMatch;
        }
        let (prepared, mut warnings) = prepare_pages(pages);
        warnings.extend(validate_web_cfg(&config.web));
        let items = prepared
            .iter()
            .map(|page| OutlineItem {
                title: probe_title(page, &config.web),
                source: SourceKey::Url(page.url.clone()),
                children: Vec::new(),
            })
            .collect();
        Probe::Matched {
            outline: Outline {
                kind: "web",
                items,
                warnings,
            },
        }
    }

    fn load(&self, source: &Source, env: &LoadEnv<'_>) -> Result<BookDraft, BuildError> {
        let Source::Pages(pages) = source else {
            return Err(BuildError::Source(
                "web frontend requires a Pages source".into(),
            ));
        };
        let mut book = BookDraft::default();
        // 容忍轨合并点走 Reporter（镜像 + 入账同一时刻；probe 侧同批
        // warning 走 Outline 呈现，不镜像——避免 convert 先 probe 后 load 推两次）
        let (prepared, warnings) = prepare_pages(pages);
        reporter::Reporter::new(&mut book.warnings, env.progress).absorb(warnings);
        reporter::Reporter::new(&mut book.warnings, env.progress)
            .absorb(validate_web_cfg(&env.config.web));

        let mut first_doc_title: Option<String> = None;
        let total = prepared.len() as u64;
        for (idx, page) in prepared.iter().enumerate() {
            if env.progress.is_cancelled() {
                return Err(BuildError::Cancelled);
            }
            let loaded = load_page(page, &env.config.web, &mut book, env.progress);
            if let Some(id) = loaded.chapter {
                book.toc.push(TocItem {
                    chapter: Some(id),
                    title: book.chapters[id.0 as usize].title.clone(),
                    children: Vec::new(),
                });
            }
            if idx == 0 {
                first_doc_title = loaded.doc_title;
            }
            env.progress.item(idx as u64 + 1, total, page.url.as_str());
        }

        assemble_meta(&mut book, env.config, first_doc_title);
        register_cover(env.config, &mut book, env.progress);
        Ok(book)
    }
}

// ---------------------------------------------------------------------------
// 预处理：去重 + content-type 过滤
// ---------------------------------------------------------------------------

fn prepare_pages(pages: &[FetchedPage]) -> (Vec<PreparedPage>, Vec<Warning>) {
    let mut prepared = Vec::new();
    let mut warnings = Vec::new();
    let mut seen: HashSet<Url> = HashSet::new();
    for page in pages {
        if !seen.insert(page.url.clone()) {
            warnings.push(Warning::new(
                WarningCode::DuplicateSource,
                Some(SourceKey::Url(page.url.clone())),
                format!("duplicate page URL; keeping the first: {}", page.url),
            ));
            continue;
        }
        if !is_html(&page.content_type) {
            warnings.push(Warning::new(
                WarningCode::PageExtractFailed,
                Some(SourceKey::Url(page.url.clone())),
                format!(
                    "non-HTML page skipped ({}): {}",
                    page.content_type, page.url
                ),
            ));
            continue;
        }
        prepared.push(PreparedPage {
            url: page.url.clone(),
            body: page.body.clone(),
            content_type: page.content_type.clone(),
        });
    }
    (prepared, warnings)
}

/// content_type 是否 text/html（与 fetch crate 的采集口径一致；core 不
/// 依赖 fetch，5 行重复可接受）。
fn is_html(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|mt| mt.trim().eq_ignore_ascii_case("text/html"))
}

// ---------------------------------------------------------------------------
// 选择器链与清理
// ---------------------------------------------------------------------------

/// 选择器链：cfg.selectors > 预设 > 默认链（30-frontends.md 落地口径）。
fn selector_chain(cfg: &WebCfg) -> Vec<String> {
    if !cfg.selectors.is_empty() {
        return cfg.selectors.clone();
    }
    let chain: [&str; 5] = match cfg.preset.as_deref() {
        Some("mkdocs") => [".md-content", "article", "main", "", ""],
        Some("docusaurus") => ["article", "main", "", "", ""],
        Some("vitepress") => [".vp-doc", "main", "article", "", ""],
        Some("gitbook") => ["article", ".markdown-body", "main", "", ""],
        _ => ["main", "article", ".content", ".markdown-body", "body"],
    };
    chain
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// 内置清理选择器（网页 chrome）。
const BUILTIN_STRIP: &str = "nav, footer, aside, script, style, noscript, form, button";

/// 编译选择器；坏选择器记 StructureAmbiguous warning 并跳过（保守可运行）。
fn compile_selector(css: &str, book_warnings: &mut Vec<Warning>) -> Option<Selector> {
    match Selector::parse(css) {
        Ok(sel) => Some(sel),
        Err(_) => {
            book_warnings.push(Warning::new(
                WarningCode::StructureAmbiguous,
                None,
                format!("invalid selector ignored: {css}"),
            ));
            None
        }
    }
}

/// WebCfg 校验（未知预设名 / 坏 strip 选择器 → StructureAmbiguous warning；
/// probe 与 load 各自独立跑一遍，两条 warning 轨对齐）。
fn validate_web_cfg(cfg: &WebCfg) -> Vec<Warning> {
    let mut out = Vec::new();
    if let Some(preset) = &cfg.preset {
        if !matches!(
            preset.as_str(),
            "mkdocs" | "docusaurus" | "vitepress" | "gitbook"
        ) {
            out.push(Warning::new(
                WarningCode::StructureAmbiguous,
                None,
                format!("unknown web preset {preset:?}; using the default selector chain"),
            ));
        }
    }
    for css in &cfg.strip {
        if Selector::parse(css).is_err() {
            out.push(Warning::new(
                WarningCode::StructureAmbiguous,
                None,
                format!("invalid strip selector ignored: {css}"),
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 编码
// ---------------------------------------------------------------------------

/// 解码：content_type charset > meta 嗅探 > UTF-8；BOM 优先（encoding_rs
/// 行为）。返回 (text, 有损)。
fn decode_page(page: &PreparedPage) -> (String, bool) {
    let label =
        charset_from_content_type(&page.content_type).or_else(|| sniff_meta_charset(&page.body));
    let encoding = label
        .as_deref()
        .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _encoding_used, had_errors) = encoding.decode(&page.body);
    (text.into_owned(), had_errors)
}

/// `text/html; charset=gbk` → `gbk`。
fn charset_from_content_type(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    let pos = lower.find("charset=")?;
    let val = content_type[pos + "charset=".len()..].trim_start();
    let val = val.trim_start_matches(['"', '\'']);
    let end = val.find([';', '"', '\'', ' ', '\t']).unwrap_or(val.len());
    let charset = val[..end].trim();
    (!charset.is_empty()).then(|| charset.to_string())
}

/// 头 2048 字节嗅探 `<meta charset=…>`（含 http-equiv content-type 形态）。
fn sniff_meta_charset(bytes: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    let pos = head.find("charset")?;
    let after = head[pos + "charset".len()..]
        .trim_start()
        .strip_prefix('=')?
        .trim_start()
        .trim_start_matches(['"', '\'']);
    let val: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .collect();
    (!val.is_empty()).then_some(val)
}

// ---------------------------------------------------------------------------
// probe：轻量标题
// ---------------------------------------------------------------------------

fn probe_title(page: &PreparedPage, cfg: &WebCfg) -> String {
    let (text, _) = decode_page(page);
    let doc = Html::parse_document(&text);
    let mut warnings = Vec::new();
    let chain = compile_chain(cfg, &mut warnings);
    let root = find_root(&doc, &chain);
    let root_id = root.map(|el| el.id());
    page_title(&doc, root_id, &page.url)
}

// ---------------------------------------------------------------------------
// load：正文抽取
// ---------------------------------------------------------------------------

fn load_page(
    page: &PreparedPage,
    cfg: &WebCfg,
    book: &mut BookDraft,
    progress: &dyn ProgressSink,
) -> LoadedPage {
    let key = SourceKey::Url(page.url.clone());
    let (text, lossy) = decode_page(page);
    if lossy {
        reporter::report(
            &mut book.warnings,
            progress,
            Warning::new(
                WarningCode::EncodingLossy,
                Some(key.clone()),
                "page is not valid in its declared encoding; lossy conversion applied",
            ),
        );
    }

    let mut doc = Html::parse_document(&text);
    let doc_title = doc_title(&doc);
    let chain = compile_chain(cfg, &mut Vec::new()); // 校验 warning 已由 validate_web_cfg 记过
    let Some(root_id) = find_root(&doc, &chain).map(|el| el.id()) else {
        reporter::report(
            &mut book.warnings,
            progress,
            Warning::new(
                WarningCode::PageExtractFailed,
                Some(key),
                format!("no content root found in {}", page.url),
            ),
        );
        return LoadedPage {
            chapter: None,
            doc_title,
        };
    };

    strip_chrome(&mut doc, root_id, cfg);
    ensure_heading_ids(&mut doc, root_id);

    // 资产登记（不改动 src 属性——义务清单第 3 条）
    for media_id in dom::select_ids(&doc, sel_media()) {
        if !within(&doc, media_id, root_id) {
            continue;
        }
        if let Some(src) = dom::get_attr(&doc, media_id, "src") {
            register_asset(&page.url, &src, book, progress);
        }
    }

    let title = page_title(&doc, Some(root_id), &page.url);
    let html = dom::serialize_children(&doc, root_id);
    let id = book.push_chapter(title, SourceKey::Url(page.url.clone()), html);
    LoadedPage {
        chapter: Some(id),
        doc_title,
    }
}

/// 编译选择器链（坏选择器已在 validate 阶段警告，这里直接丢弃）。
fn compile_chain(cfg: &WebCfg, warnings: &mut Vec<Warning>) -> Vec<Selector> {
    selector_chain(cfg)
        .iter()
        .filter_map(|css| compile_selector(css, warnings))
        .collect()
}

/// 正文根：链内首个命中选择器的**首个匹配**；全落空（仅自定义链可能）
/// → StructureAmbiguous + body 兜底。
fn find_root<'d>(doc: &'d Html, chain: &[Selector]) -> Option<scraper::ElementRef<'d>> {
    for sel in chain {
        if let Some(el) = doc.select(sel).next() {
            return Some(el);
        }
    }
    None
}

/// 预设级补充剔除：生成器渲染进**正文容器内**的页面 chrome（适配分界
/// 口径——生成器级共性进代码，30-frontends.md）。mkdocs 的 edit/view
/// 源按钮 `.md-content__button`（含 SVG）在 .md-content__inner 里，
/// 2026-10 实抓 squidfunk.github.io/mkdocs-material 时发现漏进正文。
fn preset_strip(cfg: &WebCfg) -> &'static [&'static str] {
    match cfg.preset.as_deref() {
        Some("mkdocs") => &[".md-content__button"],
        _ => &[],
    }
}

/// 剔除 chrome：内置清单 + 预设级补充 + cfg.strip；不动正文根本身（根外
/// 元素反正不会被序列化，一并删掉无害）。
fn strip_chrome(doc: &mut Html, root_id: NodeId, cfg: &WebCfg) {
    let builtin = Selector::parse(BUILTIN_STRIP).expect("constant selector");
    let strips: Vec<Selector> = std::iter::once(builtin)
        .chain(
            preset_strip(cfg)
                .iter()
                .map(|css| Selector::parse(css).expect("constant preset selector")),
        )
        .chain(cfg.strip.iter().filter_map(|css| Selector::parse(css).ok()))
        .collect();
    for sel in &strips {
        for id in dom::select_ids(doc, sel) {
            if id != root_id {
                dom::remove_element(doc, id);
            }
        }
    }
}

/// `id` 是否在 `ancestor` 子树内。
fn within(doc: &Html, id: NodeId, ancestor: NodeId) -> bool {
    let mut cur = Some(id);
    while let Some(node_id) = cur {
        if node_id == ancestor {
            return true;
        }
        cur = dom::parent_id(doc, node_id);
    }
    false
}

/// 正文内 heading 缺 id 的补稳定 id（GitHub 式 slug + 去重，义务清单第 2 条）。
fn ensure_heading_ids(doc: &mut Html, root_id: NodeId) {
    let mut anchorizer = Anchorizer::new();
    for h_id in dom::select_ids(doc, sel_headings()) {
        if !within(doc, h_id, root_id) {
            continue;
        }
        if dom::get_attr(doc, h_id, "id").is_none() {
            let text = dom::text_content(doc, h_id);
            let id = anchorizer.anchorize(&text);
            dom::set_attr(doc, h_id, "id", &id);
        }
    }
}

/// 章名：正文首个 h1 → `<title>` → URL path 末段（percent-decoded）→ host。
fn page_title(doc: &Html, root_id: Option<NodeId>, url: &Url) -> String {
    if let Some(root) = root_id {
        for h1_id in dom::select_ids(doc, sel_h1()) {
            if within(doc, h1_id, root) {
                let text = collapse_ws(&dom::text_content(doc, h1_id));
                if !text.is_empty() {
                    return text;
                }
                break;
            }
        }
    }
    if let Some(title) = doc_title(doc).filter(|t| !t.is_empty()) {
        return title;
    }
    let last = url.path().rsplit('/').next().unwrap_or("");
    let last = refs::percent_decode(last);
    if !last.is_empty() {
        return last;
    }
    url.host_str().unwrap_or("untitled").to_string()
}

/// 文档 `<title>` 文本。
fn doc_title(doc: &Html) -> Option<String> {
    let id = dom::select_ids(doc, sel_title()).first().copied()?;
    let text = collapse_ws(&dom::text_content(doc, id));
    (!text.is_empty()).then_some(text)
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// 资产登记（Url 世界）
// ---------------------------------------------------------------------------

/// 相对 src 以页面 URL 为基规范化为绝对 URL；http(s) → Remote、data: →
/// 解码为 Owned。不改动 HTML 属性。
fn register_asset(page_url: &Url, src: &str, book: &mut BookDraft, progress: &dyn ProgressSink) {
    let key = asset_key(page_url, src, book, progress);
    let Some(key) = key else {
        return; // 失败已在 asset_key 里 warning
    };
    if book.assets.contains_key(&key) {
        return;
    }
    let SourceKey::Url(url) = &key else {
        return;
    };
    let data = if url.scheme() == "data" {
        match decode_data_uri(url.as_str()) {
            Some(bytes) => AssetData::Owned(bytes),
            None => {
                reporter::report(
                    &mut book.warnings,
                    progress,
                    Warning::new(
                        WarningCode::AssetFetchFailed,
                        Some(key.clone()),
                        "invalid data: URI",
                    ),
                );
                AssetData::Missing
            }
        }
    } else {
        AssetData::Remote(url.clone())
    };
    book.assets.insert(key.clone(), Asset { source: key, data });
}

/// src → 资产键：data: / http(s)（绝对或以页面 URL join 相对）。
fn asset_key(
    page_url: &Url,
    src: &str,
    book: &mut BookDraft,
    progress: &dyn ProgressSink,
) -> Option<SourceKey> {
    let (target, _) = refs::split_fragment(src);
    if target.is_empty() {
        return None; // 纯锚点 src：无意义图片引用，不登记
    }
    let joined = if refs::is_external(target) {
        target.parse::<Url>().ok()
    } else {
        page_url.join(target).ok()
    };
    match joined {
        Some(mut url) => {
            url.set_fragment(None);
            Some(SourceKey::Url(url))
        }
        None => {
            reporter::report(
                &mut book.warnings,
                progress,
                Warning::new(
                    WarningCode::LinkUnresolved,
                    Some(SourceKey::Url(page_url.clone())),
                    format!("unresolvable asset reference: {src}"),
                ),
            );
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 书籍元数据
// ---------------------------------------------------------------------------

/// 书名：配置 > 第一页文档标题 > "Untitled"。
fn assemble_meta(book: &mut BookDraft, config: &ResolvedConfig, first_doc_title: Option<String>) {
    book.meta.title = config
        .book
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .or(first_doc_title)
        .unwrap_or_else(|| "Untitled".to_string());
    book.meta.authors = config.book.authors.clone();
    book.meta.language = config
        .book
        .language
        .clone()
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "en".to_string());
    book.meta.description = config.book.description.clone();
}

/// 封面（Pages 世界）：仅接受 URL / data: URI；相对路径需要文件树源，
/// 给 warning 而不是静默丢弃。
fn register_cover(config: &ResolvedConfig, book: &mut BookDraft, progress: &dyn ProgressSink) {
    let Some(cover) = config.book.cover.as_ref().filter(|c| !c.is_empty()) else {
        return;
    };
    let key = if refs::is_external(cover) {
        match cover.parse::<Url>() {
            Ok(url) => SourceKey::Url(url),
            Err(_) => {
                reporter::report(
                    &mut book.warnings,
                    progress,
                    Warning::new(
                        WarningCode::AssetFetchFailed,
                        None,
                        format!("invalid cover URL: {cover}"),
                    ),
                );
                return;
            }
        }
    } else {
        reporter::report(
            &mut book.warnings,
            progress,
            Warning::new(
                WarningCode::AssetFetchFailed,
                None,
                "cover path requires a file-tree source; use a URL or data: URI for Pages",
            ),
        );
        return;
    };
    let SourceKey::Url(url) = &key else {
        return;
    };
    let data = if url.scheme() == "data" {
        match decode_data_uri(cover) {
            Some(bytes) => AssetData::Owned(bytes),
            None => {
                reporter::report(
                    &mut book.warnings,
                    progress,
                    Warning::new(
                        WarningCode::AssetFetchFailed,
                        Some(key.clone()),
                        "invalid cover data: URI",
                    ),
                );
                return;
            }
        }
    } else {
        AssetData::Remote(url.clone())
    };
    book.assets.insert(
        key.clone(),
        Asset {
            source: key.clone(),
            data,
        },
    );
    book.meta.cover = Some(key);
}

// ---------------------------------------------------------------------------
// 选择器常量
// ---------------------------------------------------------------------------

fn sel_headings() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("h1, h2, h3, h4, h5, h6"))
}

fn sel_h1() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("h1"))
}

fn sel_title() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("title"))
}

fn sel_media() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("img[src], source[src]"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use trans_epub_contract::source::FetchedPage;

    fn page(url: &str, body: &[u8], ct: &str) -> FetchedPage {
        FetchedPage::from_bytes(url.parse().unwrap(), body.to_vec(), ct)
    }

    fn book_from(pages: &[FetchedPage]) -> BookDraft {
        load_pages(pages, &WebCfg::default())
    }

    fn load_pages(pages: &[FetchedPage], cfg: &WebCfg) -> BookDraft {
        use trans_epub_contract::progress::NoProgress;
        let env = LoadEnv {
            config: &ResolvedConfig {
                web: cfg.clone(),
                ..ResolvedConfig::default()
            },
            progress: &NoProgress,
        };
        WebFrontend
            .load(&Source::Pages(pages.to_vec()), &env)
            .expect("load ok")
    }

    const MKDOCS_PAGE: &str = r#"<!DOCTYPE html>
<html><head><title>Install Guide</title><meta charset="utf-8"></head>
<body>
<nav class="site-nav"><a href="/">Home</a></nav>
<main>
  <article class="md-content">
    <h1>Installation</h1>
    <p>Follow the steps and code below.</p>
    <pre><code class="language-rust">fn main() { println!("hi"); }</code></pre>
    <img src="img/arch.png" alt="arch">
    <aside class="ad">promo</aside>
    <script>alert("no")</script>
  </article>
</main>
<footer>© 2026</footer>
</body></html>"#;

    #[test]
    fn extracts_content_strips_chrome_keeps_language_class() {
        let book = book_from(&[page(
            "https://docs.example.dev/guide/install.html",
            MKDOCS_PAGE.as_bytes(),
            "text/html",
        )]);
        assert_eq!(book.chapters.len(), 1);
        let ch = &book.chapters[0];
        assert_eq!(ch.title, "Installation", "正文首个 h1 为章名");
        assert!(
            ch.html.contains(r#"<code class="language-rust">"#),
            "{}",
            ch.html
        );
        assert!(ch.html.contains("println"), "{}", ch.html);
        assert!(!ch.html.contains("site-nav"), "nav 剔除");
        assert!(!ch.html.contains("© 2026"), "footer 剔除");
        assert!(!ch.html.contains("promo"), "aside 剔除");
        assert!(!ch.html.contains("alert"), "script 剔除");
        // 相对图片以页面 URL 为基登记为 Remote
        let key = SourceKey::Url(
            "https://docs.example.dev/guide/img/arch.png"
                .parse()
                .unwrap(),
        );
        assert!(matches!(book.assets.get(&key), Some(a) if matches!(a.data, AssetData::Remote(_))));
        assert_eq!(book.meta.title, "Install Guide", "第一页 <title> 兜底书名");
    }

    #[test]
    fn selector_chain_presets_and_overrides() {
        let mut cfg = WebCfg::default();
        assert_eq!(selector_chain(&cfg)[0], "main");
        cfg.preset = Some("vitepress".into());
        assert_eq!(selector_chain(&cfg)[0], ".vp-doc");
        cfg.selectors = vec![".custom".into()];
        assert_eq!(selector_chain(&cfg), vec![".custom"]);
    }

    #[test]
    fn preset_unknown_falls_back_with_warning() {
        let cfg = WebCfg {
            preset: Some("wordpress".into()),
            ..WebCfg::default()
        };
        let book = load_pages(
            &[page("https://a.dev/x", MKDOCS_PAGE.as_bytes(), "text/html")],
            &cfg,
        );
        assert_eq!(book.chapters.len(), 1, "默认链兜底");
        assert!(
            book.warnings
                .iter()
                .any(|w| w.message.contains("unknown web preset")),
            "{:?}",
            book.warnings
        );
    }

    #[test]
    fn duplicate_and_non_html_pages_warn() {
        let pages = vec![
            page("https://a.dev/x", b"<main><h1>X</h1></main>", "text/html"),
            page("https://a.dev/x", b"<main><h1>X2</h1></main>", "text/html"),
            page("https://a.dev/logo.png", b"png", "image/png"),
        ];
        let book = book_from(&pages);
        assert_eq!(book.chapters.len(), 1);
        let codes: Vec<&str> = book.warnings.iter().map(|w| w.code.as_str()).collect();
        assert!(codes.contains(&"duplicate-source"), "{codes:?}");
        assert!(codes.contains(&"page-extract-failed"), "{codes:?}");
    }

    #[test]
    fn title_falls_back_to_doc_title_then_path() {
        // 无 h1 → <title>
        let book = book_from(&[page(
            "https://a.dev/no-h1.html",
            b"<html><head><title>From Title</title></head><body><main><p>text</p></main></body></html>",
            "text/html",
        )]);
        assert_eq!(book.chapters[0].title, "From Title");
        // 无 h1 无 title → path 末段
        let book = book_from(&[page(
            "https://a.dev/guide/last-resort.html",
            b"<html><body><main><p>text</p></main></body></html>",
            "text/html",
        )]);
        assert_eq!(book.chapters[0].title, "last-resort.html");
    }

    #[test]
    fn non_utf8_page_decodes_with_meta_charset_and_warns_on_lossy() {
        // latin-1：é = 0xE9（非法 UTF-8）；meta charset 声明在头 2048 字节内
        let raw = "<html><head><meta charset=\"iso-8859-1\"></head><body><main><h1>Café</h1></main></body></html>";
        let bytes: Vec<u8> = raw
            .chars()
            .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
            .collect();
        let book = book_from(&[page("https://a.dev/enc.html", &bytes, "text/html")]);
        assert_eq!(book.chapters[0].title, "Caf\u{00E9}", "meta charset 解码");
        assert!(
            book.warnings.is_empty(),
            "合法 latin-1 不应有有损告警：{:?}",
            book.warnings
        );

        // 未声明编码的非法 UTF-8 字节 → UTF-8 兜底 + EncodingLossy warning
        let bad = b"<html><body><main><h1>Bad \xFF\xFE</h1></main></body></html>";
        let book = book_from(&[page("https://a.dev/bad.html", bad, "text/html")]);
        assert!(
            book.warnings
                .iter()
                .any(|w| w.code.as_str() == "encoding-lossy"),
            "{:?}",
            book.warnings
        );
    }

    #[test]
    fn charset_from_content_type_variants() {
        assert_eq!(
            charset_from_content_type("text/html; charset=utf-8").as_deref(),
            Some("utf-8")
        );
        assert_eq!(
            charset_from_content_type("text/html; CHARSET=\"gbk\"").as_deref(),
            Some("gbk")
        );
        assert_eq!(charset_from_content_type("text/html").as_deref(), None);
    }

    #[test]
    fn meta_charset_sniffing() {
        assert_eq!(
            sniff_meta_charset(b"<html><head><meta charset=\"gbk\"></head>").as_deref(),
            Some("gbk")
        );
        assert_eq!(
            sniff_meta_charset(
                b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=big5\">"
            )
            .as_deref(),
            Some("big5")
        );
        assert_eq!(sniff_meta_charset(b"<html><body>plain</body>"), None);
    }

    #[test]
    fn probe_outline_lists_pages_in_order() {
        let pages = vec![
            page(
                "https://a.dev/1.html",
                b"<main><h1>One</h1></main>",
                "text/html",
            ),
            page(
                "https://a.dev/2.html",
                b"<main><h1>Two</h1></main>",
                "text/html",
            ),
        ];
        let probe = WebFrontend.probe(&Source::Pages(pages), &ResolvedConfig::default());
        let Probe::Matched { outline } = probe else {
            panic!("Pages 源必须命中 web 前端");
        };
        assert_eq!(outline.kind, "web");
        let titles: Vec<&str> = outline.items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["One", "Two"]);
        // Vfs 源不命中
        use trans_epub_contract::testing::MemVfs;
        let vfs = MemVfs::new().insert("a.md", "# A");
        assert!(matches!(
            WebFrontend.probe(
                &Source::Vfs(std::sync::Arc::new(vfs)),
                &ResolvedConfig::default()
            ),
            Probe::NoMatch
        ));
    }

    #[test]
    fn heading_ids_are_added() {
        let body = b"<html><body><main><h2>Getting Started</h2><p>x</p></main></body></html>";
        let book = book_from(&[page("https://a.dev/h.html", body, "text/html")]);
        assert!(
            book.chapters[0]
                .html
                .contains(r#"<h2 id="getting-started">"#),
            "{}",
            book.chapters[0].html
        );
    }

    #[test]
    fn data_uri_image_registered_owned() {
        let png1x1 = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let body =
            format!(r#"<html><body><main><img src="{png1x1}" alt="dot"></main></body></html>"#);
        let book = book_from(&[page("https://a.dev/d.html", body.as_bytes(), "text/html")]);
        let keys: Vec<&SourceKey> = book.assets.keys().collect();
        assert_eq!(keys.len(), 1, "{keys:?}");
        assert!(
            matches!(book.assets.values().next(), Some(a) if matches!(a.data, AssetData::Owned(_)))
        );
    }
}
