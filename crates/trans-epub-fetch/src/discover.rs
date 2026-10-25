//! 页面发现三机制（40-fetch.md）：URL 列表文件、sitemap.xml、同域递归
//! （BFS）。产出**有序**清单——顺序即书序，排序是网站成书的质量命门，
//! 每种机制有明确的顺序语义：
//!
//! | 机制 | 顺序语义 |
//! |---|---|
//! | [`from_list`] | 文件行序（`#` 注释与空行跳过） |
//! | [`from_sitemap`] | sitemap 文件内条目序（index 递归展开，深度上限防环） |
//! | [`crawl`] | BFS 发现序（层序）；同层内按页面链接出现序 |
//!
//! [`Discovered::urls`] 恒为有序 URL 清单（dry-run 打印用）；`pages` 在
//! crawl 机制下发现时已顺带抓取（免二次请求），list / sitemap 由壳层按需
//! 调 [`Discovered::fetch_pages`]。
//!
//! sitemap 经 html5ever（scraper）宽松解析：sitemap 是 XML，但其标签集
//! （`urlset/url/loc`、`sitemapindex/sitemap/loc`）小写无前缀、实体集与
//! HTML 一致，按 HTML 解析既正确又免去独立 XML 依赖，对缺 xmlns 等
//! 非标准变体反而更宽容（带命名空间前缀的变体不支持）。

use std::collections::{HashSet, VecDeque};
use std::sync::OnceLock;

use scraper::{Html, Selector};
use trans_epub_contract::error::{Warning, WarningCode};
use trans_epub_contract::progress::ProgressSink;
use trans_epub_contract::source::{FetchedPage, Fetcher};
use url::Url;

/// 发现配置（BFS 用全部字段；list / sitemap 只用 max_pages 语义之外的部分）。
#[derive(Debug, Clone)]
pub struct DiscoverCfg {
    /// BFS 深度上限（起始页为第 0 层）。
    pub max_depth: u32,
    /// 页面数安全帽（防止在大站上失控；BFS 与 fetch_pages 都尊重）。
    pub max_pages: usize,
    /// include glob（对 URL path 去前导 `/` 匹配；空 = 不限）。
    pub include: Vec<String>,
    /// exclude glob（命中即丢弃）。
    pub exclude: Vec<String>,
}

impl Default for DiscoverCfg {
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_pages: 500,
            include: Vec::new(),
            exclude: Vec::new(),
        }
    }
}

/// 发现结果：有序 URL 清单 +（可能已就绪的）页面 + 过程 warning。
pub struct Discovered {
    /// 有序 URL 清单（顺序即书序；dry-run 打印它）。
    pub urls: Vec<Url>,
    /// 已抓取的页面（crawl 机制发现时顺带抓取；list / sitemap 初始为空）。
    pub pages: Vec<FetchedPage>,
    pub warnings: Vec<Warning>,
}

impl Discovered {
    fn empty() -> Self {
        Self {
            urls: Vec::new(),
            pages: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn warn(&mut self, message: String) {
        self.warnings
            .push(Warning::new(WarningCode::PageExtractFailed, None, message));
    }

    /// 把 `urls` 抓取为页面（list / sitemap 的第二步；crawl 已顺带抓取，
    /// 无需再调）。失败与非 text/html 资源 → warning 跳过（40-fetch.md：
    /// 只保留 text/html）；受 `max_pages` 安全帽约束。
    pub fn fetch_pages(
        &mut self,
        fetcher: &dyn Fetcher,
        cfg: &DiscoverCfg,
        progress: Option<&dyn ProgressSink>,
    ) {
        let total = self.urls.len().min(cfg.max_pages) as u64;
        let urls: Vec<Url> = self.urls.clone();
        for (idx, url) in urls.iter().enumerate() {
            if idx >= cfg.max_pages {
                self.warn(format!(
                    "page cap reached ({}); skipping the rest",
                    cfg.max_pages
                ));
                break;
            }
            if let Some(p) = progress {
                p.item(idx as u64, total, url.as_str());
            }
            match fetcher.fetch(url) {
                Ok(page) if is_html(&page.content_type) => self.pages.push(page),
                Ok(page) => self.warn(format!(
                    "non-HTML resource skipped ({}): {url}",
                    page.content_type
                )),
                Err(err) => self.warn(format!("fetch {url} failed: {err}")),
            }
        }
    }
}

/// BFS 链接抽取的扩展名黑名单：这些 path 不可能是 HTML 页面，跳过以省
/// 一次请求（最终裁决仍是响应的 content-type）。
const NON_PAGE_EXTS: &[&str] = &[
    "css", "js", "mjs", "json", "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "woff",
    "woff2", "ttf", "otf", "eot", "zip", "tar", "gz", "bz2", "7z", "pdf", "epub", "mp3", "mp4",
    "webm", "ogg", "wav", "xml", "txt", "csv", "rss",
];

/// sitemap 递归展开的深度上限（防 index 环）。
const MAX_SITEMAP_DEPTH: u32 = 5;

// ---------------------------------------------------------------------------
// 机制 1：URL 列表文件
// ---------------------------------------------------------------------------

/// 解析 URL 列表文本（一行一 URL；`#` 注释与空行跳过），保持行序。
/// 非法行 / 非 http(s) 行记 warning 跳过，不中止。不抓取。
pub fn from_list(text: &str) -> Discovered {
    let mut result = Discovered::empty();
    for (idx, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match normalize_url(line) {
            Some(url) => result.urls.push(url),
            None => result.warn(format!("ignoring invalid list line {}: {line}", idx + 1)),
        }
    }
    result
}

// ---------------------------------------------------------------------------
// 机制 2：sitemap.xml
// ---------------------------------------------------------------------------

/// sitemap 条目序 = 书序；index sitemap 递归展开（深度上限防环）。
/// 只抓 sitemap 本身，页面体由 [`Discovered::fetch_pages`] 按需抓取。
pub fn from_sitemap(
    fetcher: &dyn Fetcher,
    sitemap_url: &Url,
    progress: Option<&dyn ProgressSink>,
) -> Discovered {
    let mut result = Discovered::empty();
    let mut seen = HashSet::new();
    collect_sitemap(fetcher, sitemap_url, 0, &mut seen, &mut result, progress);
    result
}

/// 本地 sitemap 文件入口：直接解析给定字节（本文件不经网络）；index 里
/// 引用的子 sitemap 是绝对 URL，仍经 fetcher 抓取。
pub fn from_sitemap_local(
    fetcher: &dyn Fetcher,
    bytes: &[u8],
    progress: Option<&dyn ProgressSink>,
) -> Discovered {
    let mut result = Discovered::empty();
    let mut seen = HashSet::new();
    ingest_sitemap(fetcher, bytes, 0, &mut seen, &mut result, progress);
    result
}

/// 递归拉取一个 sitemap：`urlset` 的 `loc` 收为页面 URL，`sitemapindex`
/// 的 `loc` 作为子 sitemap 继续展开。
fn collect_sitemap(
    fetcher: &dyn Fetcher,
    url: &Url,
    depth: u32,
    seen: &mut HashSet<Url>,
    result: &mut Discovered,
    progress: Option<&dyn ProgressSink>,
) {
    if depth > MAX_SITEMAP_DEPTH {
        result.warn(format!(
            "sitemap nesting deeper than {MAX_SITEMAP_DEPTH}: {url}"
        ));
        return;
    }
    if !seen.insert(url.clone()) {
        return; // 环 / 重复引用
    }
    if let Some(p) = progress {
        p.item(seen.len() as u64, seen.len() as u64 + 1, url.as_str());
    }
    let page = match fetcher.fetch(url) {
        Ok(page) => page,
        Err(err) => {
            result.warn(format!("fetch sitemap {url} failed: {err}"));
            return;
        }
    };
    ingest_sitemap(fetcher, &page.body, depth, seen, result, progress);
}

/// 摄入一份 sitemap 字节：index → 递归子 sitemap；urlset → 收 URL。
fn ingest_sitemap(
    fetcher: &dyn Fetcher,
    bytes: &[u8],
    depth: u32,
    seen: &mut HashSet<Url>,
    result: &mut Discovered,
    progress: Option<&dyn ProgressSink>,
) {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let doc = Html::parse_document(&text);
    let is_index = doc.select(sel_sitemap_index()).next().is_some();
    for el in doc.select(sel_any_loc()) {
        let loc = el.text().collect::<String>().trim().to_string();
        match normalize_url(&loc) {
            Some(child) if is_index => {
                collect_sitemap(fetcher, &child, depth + 1, seen, result, progress)
            }
            Some(child) => result.urls.push(child),
            None => result.warn(format!(
                "sitemap entry is not an absolute http(s) URL: {loc}"
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// 机制 3：同域递归（BFS）
// ---------------------------------------------------------------------------

/// 从起始页 BFS：发现序（层序）= 书序。跨域链接、非页面扩展名、
/// include/exclude 不匹配的链接不入队；URL 去重以规范化 URL（无 fragment）
/// 为准；页面数达 [`DiscoverCfg::max_pages`] 即止并记 warning。
/// 页面体在发现时顺带保留（免二次抓取）。
pub fn crawl(
    fetcher: &dyn Fetcher,
    start: &Url,
    cfg: &DiscoverCfg,
    progress: Option<&dyn ProgressSink>,
) -> Discovered {
    let mut result = Discovered::empty();
    let filters = PathFilters::new(&cfg.include, &cfg.exclude);
    let mut visited: HashSet<Url> = HashSet::new();
    let mut queue: VecDeque<(Url, u32)> = VecDeque::from([(start.clone(), 0)]);

    while let Some((url, depth)) = queue.pop_front() {
        if !visited.insert(url.clone()) {
            continue;
        }
        if let Some(p) = progress {
            p.item(
                result.pages.len() as u64,
                cfg.max_pages as u64,
                url.as_str(),
            );
        }
        let page = match fetcher.fetch(&url) {
            Ok(page) => page,
            Err(err) => {
                result.warn(format!("fetch {url} failed: {err}"));
                continue;
            }
        };
        if !is_html(&page.content_type) {
            result.warn(format!(
                "non-HTML resource skipped ({}): {url}",
                page.content_type
            ));
            continue;
        }
        let links = extract_links(&page);
        result.urls.push(page.url.clone());
        result.pages.push(page);
        if result.pages.len() >= cfg.max_pages {
            result.warn(format!(
                "page cap reached ({}); stopping crawl",
                cfg.max_pages
            ));
            break;
        }
        if depth >= cfg.max_depth {
            continue;
        }
        for link in links {
            if !same_origin(&link, start) || !filters.allows(&link) || visited.contains(&link) {
                continue;
            }
            queue.push_back((link, depth + 1));
        }
    }
    result
}

/// 页面内 `a[href]` → 规范化绝对 URL（fragment 剥掉、仅 http(s)、
/// 非 HTML 扩展名剔除）；文档序去重。
fn extract_links(page: &FetchedPage) -> Vec<Url> {
    let text = String::from_utf8_lossy(&page.body);
    let doc = Html::parse_document(&text);
    let mut out: Vec<Url> = Vec::new();
    let mut seen = HashSet::new();
    for el in doc.select(sel_links()) {
        let Some(href) = el.value().attr("href") else {
            continue;
        };
        let Ok(joined) = page.url.join(href) else {
            continue;
        };
        let Some(url) = normalize_url(joined.as_str()) else {
            continue; // mailto: / javascript: / 非 http(s)
        };
        if non_page_path(url.path()) || !seen.insert(url.clone()) {
            continue;
        }
        out.push(url);
    }
    out
}

// ---------------------------------------------------------------------------
// 共用
// ---------------------------------------------------------------------------

/// URL 规范化入口：仅接受绝对 http(s) URL，剥 fragment。
pub fn normalize_url(input: &str) -> Option<Url> {
    let url = Url::parse(input).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    let mut url = url;
    url.set_fragment(None);
    Some(url)
}

/// content_type 是否为 HTML（`text/html` 前缀，参数忽略）。
pub fn is_html(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|mt| mt.trim().eq_ignore_ascii_case("text/html"))
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme() && a.authority() == b.authority()
}

fn non_page_path(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| NON_PAGE_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// include/exclude glob 过滤（对 URL path 去前导 `/` 匹配）。
struct PathFilters {
    include: Option<globset::GlobSet>,
    exclude: Option<globset::GlobSet>,
}

impl PathFilters {
    fn new(include: &[String], exclude: &[String]) -> Self {
        Self {
            include: compile_globs(include),
            exclude: compile_globs(exclude),
        }
    }

    fn allows(&self, url: &Url) -> bool {
        let path = url.path().trim_start_matches('/');
        if let Some(ex) = &self.exclude {
            if ex.is_match(path) {
                return false;
            }
        }
        match &self.include {
            Some(inc) => inc.is_match(path),
            None => true,
        }
    }
}

fn compile_globs(patterns: &[String]) -> Option<globset::GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for p in patterns {
        if let Ok(glob) = globset::Glob::new(p) {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

fn sel_any_loc() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| Selector::parse("loc").expect("valid selector"))
}

fn sel_sitemap_index() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| Selector::parse("sitemapindex").expect("valid selector"))
}

fn sel_links() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| Selector::parse("a[href]").expect("valid selector"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_parse_keeps_order_and_comments() {
        let result =
            from_list("# 注释\nhttps://a.dev/one\n\nhttps://a.dev/two\nnotaurl\nftp://a.dev/x\n");
        let paths: Vec<&str> = result.urls.iter().map(|u| u.path()).collect();
        assert_eq!(paths, vec!["/one", "/two"]);
        assert_eq!(result.warnings.len(), 2, "{:?}", result.warnings);
    }

    #[test]
    fn url_normalization() {
        assert!(
            normalize_url("https://a.dev/x#frag")
                .unwrap()
                .fragment()
                .is_none()
        );
        assert!(normalize_url("mailto:a@b.dev").is_none());
        assert!(normalize_url("relative/path").is_none());
    }

    #[test]
    fn html_content_type_detection() {
        assert!(is_html("text/html"));
        assert!(is_html("text/html; charset=utf-8"));
        assert!(is_html("TEXT/HTML; charset=gbk"));
        assert!(!is_html("application/xml"));
        assert!(!is_html("image/png"));
    }

    #[test]
    fn non_page_extensions() {
        assert!(non_page_path("/a/logo.png"));
        assert!(non_page_path("/a/sitemap.xml"));
        assert!(!non_page_path("/a/guide/"));
        assert!(!non_page_path("/a/guide/intro"));
    }

    #[test]
    fn links_extracted_and_deduped() {
        let page = FetchedPage::from_bytes(
            "https://a.dev/guide/page.html".parse().unwrap(),
            br#"<html><body>
                <a href="next.html">next</a>
                <a href="https://a.dev/guide/next.html#sec">dup</a>
                <a href="/top">top</a>
                <a href="https://other.dev/away">away</a>
                <a href="mailto:x@y">mail</a>
                <a href="/img/pic.png">pic</a>
            </body></html>"#
                .to_vec(),
            "text/html",
        );
        let links = extract_links(&page);
        let urls: Vec<&str> = links.iter().map(|u| u.as_str()).collect();
        // 跨域链接也保留（同域过滤是 crawl 的职责）；去重剥掉 fragment
        assert_eq!(
            urls,
            vec![
                "https://a.dev/guide/next.html",
                "https://a.dev/top",
                "https://other.dev/away",
            ]
        );
    }
}
