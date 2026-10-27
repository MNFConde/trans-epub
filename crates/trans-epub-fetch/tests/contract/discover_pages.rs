//! 页面发现三机制测试：sitemap 变体（标准 / index 嵌套 / 非标准）、
//! BFS 深度与模式过滤、跨域剔除、URL 去重、页面帽、非 HTML 过滤。
//! 全程离线（契约 MockFetcher），不依赖外网（40-fetch.md 测试策略）。

use trans_epub_contract::testing::MockFetcher;
use trans_epub_fetch::discover::{self, DiscoverCfg, Discovered};

fn fetcher_with(pages: &[(&str, &str, &str)]) -> MockFetcher {
    let mut fetcher = MockFetcher::new();
    for (url, body, ct) in pages {
        fetcher = fetcher.insert(url, body.as_bytes(), ct);
    }
    fetcher
}

fn urls_of(result: &Discovered) -> Vec<&str> {
    result.urls.iter().map(|u| u.as_str()).collect()
}

// ---------------------------------------------------------------------------
// sitemap 变体
// ---------------------------------------------------------------------------

#[test]
fn sitemap_standard_urlset_order() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>https://a.dev/guide/</loc></url>
  <url><loc>https://a.dev/guide/install.html</loc></url>
  <url><loc>https://a.dev/api.html</loc></url>
</urlset>"#;
    let fetcher = fetcher_with(&[("https://a.dev/sitemap.xml", xml, "application/xml")]);
    let result = discover::from_sitemap(
        &fetcher,
        &"https://a.dev/sitemap.xml".parse().unwrap(),
        None,
    );
    assert_eq!(
        urls_of(&result),
        vec![
            "https://a.dev/guide/",
            "https://a.dev/guide/install.html",
            "https://a.dev/api.html"
        ],
        "条目序 = 书序"
    );
}

#[test]
fn sitemap_index_nests_and_keeps_order() {
    let index = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>https://a.dev/sitemap-a.xml</loc></sitemap>
  <sitemap><loc>https://a.dev/sitemap-b.xml</loc></sitemap>
</sitemapindex>"#;
    let a = r#"<urlset><url><loc>https://a.dev/a1.html</loc></url><url><loc>https://a.dev/a2.html</loc></url></urlset>"#;
    let b = r#"<urlset><url><loc>https://a.dev/b1.html</loc></url></urlset>"#;
    let fetcher = fetcher_with(&[
        ("https://a.dev/sitemap.xml", index, "application/xml"),
        ("https://a.dev/sitemap-a.xml", a, "application/xml"),
        ("https://a.dev/sitemap-b.xml", b, "application/xml"),
    ]);
    let result = discover::from_sitemap(
        &fetcher,
        &"https://a.dev/sitemap.xml".parse().unwrap(),
        None,
    );
    assert_eq!(
        urls_of(&result),
        vec![
            "https://a.dev/a1.html",
            "https://a.dev/a2.html",
            "https://a.dev/b1.html"
        ],
        "index 深度优先展开，子 sitemap 内保持条目序"
    );
}

#[test]
fn sitemap_nonstandard_without_namespace() {
    let xml = r#"<urlset>
  <url><loc>https://a.dev/x.html</loc></url>
</urlset>"#;
    let fetcher = fetcher_with(&[("https://a.dev/sitemap.xml", xml, "text/xml")]);
    let result = discover::from_sitemap(
        &fetcher,
        &"https://a.dev/sitemap.xml".parse().unwrap(),
        None,
    );
    assert_eq!(urls_of(&result), vec!["https://a.dev/x.html"]);
}

#[test]
fn sitemap_missing_and_loop_are_reported() {
    // sitemap 本身 404 → warning，urls 空
    let fetcher = MockFetcher::new();
    let result = discover::from_sitemap(
        &fetcher,
        &"https://a.dev/sitemap.xml".parse().unwrap(),
        None,
    );
    assert!(result.urls.is_empty());
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);

    // index 自引用 → 环被 seen 集合掐断，不无限递归
    let looped =
        r#"<sitemapindex><sitemap><loc>https://a.dev/sitemap.xml</loc></sitemap></sitemapindex>"#;
    let fetcher = fetcher_with(&[("https://a.dev/sitemap.xml", looped, "application/xml")]);
    let result = discover::from_sitemap(
        &fetcher,
        &"https://a.dev/sitemap.xml".parse().unwrap(),
        None,
    );
    assert!(result.urls.is_empty());
}

// ---------------------------------------------------------------------------
// BFS：深度 / 模式过滤 / 跨域 / 去重 / 页面帽
// ---------------------------------------------------------------------------

fn crawl_site() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "https://a.dev/",
            r#"<a href="/a.html">a</a> <a href="/b.html">b</a> <a href="https://else.dev/x.html">ext</a>"#,
            "text/html",
        ),
        (
            "https://a.dev/a.html",
            r#"<a href="/a2.html">deep</a> <a href="/">back</a>"#,
            "text/html",
        ),
        (
            "https://a.dev/b.html",
            r#"<a href="/a.html">dup</a>"#,
            "text/html",
        ),
        ("https://a.dev/a2.html", r"<p>leaf</p>", "text/html"),
    ]
}

#[test]
fn bfs_respects_depth_and_dedupes() {
    let fetcher = fetcher_with(&crawl_site());
    let cfg = DiscoverCfg {
        max_depth: 1,
        ..DiscoverCfg::default()
    };
    let result = discover::crawl(&fetcher, &"https://a.dev/".parse().unwrap(), &cfg, None);
    assert_eq!(
        urls_of(&result),
        vec![
            "https://a.dev/",
            "https://a.dev/a.html",
            "https://a.dev/b.html"
        ],
        "深度 1：a2（第 2 层）不入；/ 与 /a.html 重复链接只算一次；跨域剔除"
    );
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
}

#[test]
fn bfs_depth_2_reaches_leaf() {
    let fetcher = fetcher_with(&crawl_site());
    let cfg = DiscoverCfg {
        max_depth: 2,
        ..DiscoverCfg::default()
    };
    let result = discover::crawl(&fetcher, &"https://a.dev/".parse().unwrap(), &cfg, None);
    assert_eq!(
        urls_of(&result),
        vec![
            "https://a.dev/",
            "https://a.dev/a.html",
            "https://a.dev/b.html",
            "https://a.dev/a2.html",
        ],
        "层序：第 0 层 → 第 1 层（a、b）→ 第 2 层（a2）"
    );
}

#[test]
fn bfs_include_exclude_filters() {
    let fetcher = fetcher_with(&crawl_site());
    let cfg = DiscoverCfg {
        max_depth: 2,
        include: vec!["*.html".to_string()],
        exclude: vec!["b.html".to_string()],
        ..DiscoverCfg::default()
    };
    let result = discover::crawl(&fetcher, &"https://a.dev/".parse().unwrap(), &cfg, None);
    assert_eq!(
        urls_of(&result),
        vec![
            "https://a.dev/",
            "https://a.dev/a.html",
            "https://a.dev/a2.html"
        ],
        "include 不匹配根路径但起始页总是抓；exclude 剔除 b"
    );
}

#[test]
fn bfs_page_cap_warns() {
    let fetcher = fetcher_with(&crawl_site());
    let cfg = DiscoverCfg {
        max_depth: 1,
        max_pages: 2,
        ..DiscoverCfg::default()
    };
    let result = discover::crawl(&fetcher, &"https://a.dev/".parse().unwrap(), &cfg, None);
    assert_eq!(result.pages.len(), 2);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("page cap")),
        "{:?}",
        result.warnings
    );
}

#[test]
fn bfs_skips_non_html_and_missing_pages_with_warning() {
    let fetcher = fetcher_with(&[
        (
            "https://a.dev/",
            r#"<a href="/img.png">pic</a> <a href="/gone.html">gone</a>"#,
            "text/html",
        ),
        ("https://a.dev/img.png", "fakepng", "image/png"),
    ]);
    let result = discover::crawl(
        &fetcher,
        &"https://a.dev/".parse().unwrap(),
        &DiscoverCfg::default(),
        None,
    );
    // /img.png 被 PNG 扩展名黑名单挡在入队前；/gone.html 抓取失败记 warning
    assert_eq!(urls_of(&result), vec!["https://a.dev/"]);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("/gone.html")),
        "{:?}",
        result.warnings
    );
}

// ---------------------------------------------------------------------------
// list + fetch_pages
// ---------------------------------------------------------------------------

#[test]
fn list_then_fetch_pages_filters_non_html() {
    let mut result = discover::from_list(
        "# doc pages\nhttps://a.dev/one.html\n\nhttps://a.dev/two.html\nhttps://a.dev/logo.png\n",
    );
    assert_eq!(
        urls_of(&result),
        vec![
            "https://a.dev/one.html",
            "https://a.dev/two.html",
            "https://a.dev/logo.png"
        ]
    );
    let fetcher = fetcher_with(&[
        ("https://a.dev/one.html", "<p>1</p>", "text/html"),
        ("https://a.dev/two.html", "<p>2</p>", "text/html"),
        ("https://a.dev/logo.png", "png", "image/png"),
    ]);
    result.fetch_pages(&fetcher, &DiscoverCfg::default(), None);
    assert_eq!(result.pages.len(), 2, "image/png 被 warning 跳过");
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("logo.png")),
        "{:?}",
        result.warnings
    );
}

#[test]
fn list_fetch_respects_page_cap() {
    let mut result =
        discover::from_list("https://a.dev/1.html\nhttps://a.dev/2.html\nhttps://a.dev/3.html\n");
    let fetcher = fetcher_with(&[
        ("https://a.dev/1.html", "1", "text/html"),
        ("https://a.dev/2.html", "2", "text/html"),
        ("https://a.dev/3.html", "3", "text/html"),
    ]);
    let cfg = DiscoverCfg {
        max_pages: 2,
        ..DiscoverCfg::default()
    };
    result.fetch_pages(&fetcher, &cfg, None);
    assert_eq!(result.pages.len(), 2);
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("page cap")),
        "{:?}",
        result.warnings
    );
}

// ---------------------------------------------------------------------------
// 契约钉子：已定型语义（40-fetch.md 已知边界 / guide/cli.md 旗标口径）
// ---------------------------------------------------------------------------

/// 任何请求都"重定向"到固定最终 URL 的 stub：模拟起点 host 经 301 跳到
/// 新域后 fetch 返回的 page.url 是最终域（PoliteFetcher 行为）。
struct RedirectingFetcher {
    final_url: url::Url,
    body: &'static str,
}

impl trans_epub_contract::source::Fetcher for RedirectingFetcher {
    fn fetch(
        &self,
        _url: &url::Url,
    ) -> Result<trans_epub_contract::source::FetchedPage, trans_epub_contract::source::FetchError>
    {
        Ok(trans_epub_contract::source::FetchedPage::from_bytes(
            self.final_url.clone(),
            self.body.as_bytes().to_vec(),
            "text/html",
        ))
    }
}

/// 契约钉子（40-fetch.md 已知边界）：BFS 的同域判定按**起点 host**，
/// 不按重定向后的最终 host——起点域经重定向换域后，站内链接（以最终
/// URL 为 base 解析）全被判跨域 → 0 发现。保守正确但反直觉；改此语义
/// 须先动本测试与 40-fetch.md。
#[test]
fn crawl_same_origin_is_judged_by_start_host_not_final_url() {
    let fetcher = RedirectingFetcher {
        final_url: "https://final.dev/".parse().unwrap(),
        body: r#"<a href="/guide/intro">intro</a>"#,
    };

    // 起点在 start.dev，重定向落在 final.dev：链接以 final.dev 为 base
    // 解析，与起点 start.dev 不同域 → 不入队，仅起点页（按最终 URL 记账）
    let redirected = discover::crawl(
        &fetcher,
        &"https://start.dev/".parse().unwrap(),
        &DiscoverCfg::default(),
        None,
    );
    assert_eq!(
        urls_of(&redirected),
        vec!["https://final.dev/"],
        "重定向后站内链接应被判跨域（保守口径）"
    );
    assert!(redirected.warnings.is_empty(), "{:?}", redirected.warnings);

    // 对照：起点直接用最终域（正常按 URL 应答）→ 同一条链接正常发现
    let direct_fetcher = fetcher_with(&[
        (
            "https://final.dev/",
            r#"<a href="/guide/intro">intro</a>"#,
            "text/html",
        ),
        ("https://final.dev/guide/intro", "<p>1</p>", "text/html"),
    ]);
    let direct = discover::crawl(
        &direct_fetcher,
        &"https://final.dev/".parse().unwrap(),
        &DiscoverCfg::default(),
        None,
    );
    assert_eq!(
        urls_of(&direct),
        vec!["https://final.dev/", "https://final.dev/guide/intro"],
        "换最终域名起点后同链接立刻满发现"
    );
}

/// 契约钉子（guide/cli.md 口径）：include/exclude glob 对**去前导 `/` 的
/// URL path** 匹配——`guide/*` 命中，`/guide/*` 带斜杠永不命中。
#[test]
fn include_glob_matches_path_without_leading_slash() {
    let fetcher = fetcher_with(&[
        (
            "https://a.dev/",
            r#"<a href="/guide/one">g</a> <a href="/blog/two">b</a>"#,
            "text/html",
        ),
        ("https://a.dev/guide/one", "<p>g1</p>", "text/html"),
        ("https://a.dev/blog/two", "<p>b1</p>", "text/html"),
    ]);

    let with_slash_stripped = DiscoverCfg {
        max_depth: 1,
        include: vec!["guide/*".to_string()],
        ..DiscoverCfg::default()
    };
    let result = discover::crawl(
        &fetcher,
        &"https://a.dev/".parse().unwrap(),
        &with_slash_stripped,
        None,
    );
    assert_eq!(
        urls_of(&result),
        vec!["https://a.dev/", "https://a.dev/guide/one"],
        "guide/* 命中 /guide/one，blog/two 不匹配"
    );

    let with_leading_slash = DiscoverCfg {
        max_depth: 1,
        include: vec!["/guide/*".to_string()],
        ..DiscoverCfg::default()
    };
    let result = discover::crawl(
        &fetcher,
        &"https://a.dev/".parse().unwrap(),
        &with_leading_slash,
        None,
    );
    assert_eq!(
        urls_of(&result),
        vec!["https://a.dev/"],
        "带前导斜杠的 pattern 永不命中（path 已去斜杠），仅起始页入账"
    );
}
