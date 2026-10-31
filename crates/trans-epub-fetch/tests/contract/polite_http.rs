//! PoliteFetcher 集成测试：真回环 HTTP 服务覆盖 robots（允许/禁止/缺失/
//! 5xx 禁抓/缓存/开关）、限速节拍、重试与退避、4xx 不重试、大小上限
//!（40-fetch.md 测试策略清单）。

use std::time::{Duration, Instant};

use trans_epub_contract::source::{FetchError, Fetcher};
use trans_epub_fetch::{PoliteConfig, PoliteFetcher};

use crate::support::mock_server::MockServer;

fn fast_config() -> PoliteConfig {
    PoliteConfig {
        min_interval: Duration::from_millis(20),
        retry_backoff: Duration::from_millis(5),
        // 回环 mock 流量不得出系统代理：代理会把传输层故障"洗"成 502
        //（WSL 本地代理 2026-10-31 实证），契约语义要求看到原始网络错误。
        respect_system_proxy: false,
        ..PoliteConfig::default()
    }
}

#[test]
fn fetch_returns_body_and_content_type() {
    let server = MockServer::new();
    server
        .route("/", 200, "text/html; charset=utf-8", "<html></html>")
        .route("/page", 200, "text/html; charset=utf-8", "<p>hi</p>");
    let base = server.serve();

    let fetcher = PoliteFetcher::new(fast_config());
    let page = fetcher
        .fetch(&base.join("/page").unwrap())
        .expect("fetch ok");
    assert_eq!(page.body, b"<p>hi</p>");
    assert_eq!(page.content_type, "text/html; charset=utf-8");
    assert_eq!(page.url.as_str(), base.join("page").unwrap().as_str());
}

#[test]
fn robots_disallow_denies_and_public_allows() {
    let server = MockServer::new();
    server
        .route(
            "/robots.txt",
            200,
            "text/plain",
            "User-agent: *\nDisallow: /private/\n",
        )
        .route("/private/x", 200, "text/html", "<p>secret</p>")
        .route("/public", 200, "text/html", "<p>ok</p>");
    let base = server.serve();

    let fetcher = PoliteFetcher::new(fast_config());
    let denied = fetcher.fetch(&base.join("/private/x").unwrap());
    assert!(
        matches!(denied, Err(FetchError::Denied(ref msg)) if msg.contains("robots")),
        "{denied:?}"
    );
    assert_eq!(
        fetcher.fetch(&base.join("/public").unwrap()).unwrap().body,
        b"<p>ok</p>"
    );
    // robots 只拉一次（按 origin 缓存）
    assert_eq!(server.hits("/robots.txt"), 1);
}

#[test]
fn robots_missing_allows_all() {
    // 无 robots.txt 路由（404）→ 全放行（RFC 9309：4xx 视同无约束）
    let server = MockServer::new();
    server.route("/any", 200, "text/html", "x");
    let base = server.serve();

    let fetcher = PoliteFetcher::new(fast_config());
    assert!(fetcher.fetch(&base.join("/any").unwrap()).is_ok());
    // 404 的 robots 也只拉一次（缓存以"结论"为粒度，不因缺失重试）
    let robots_requests = server
        .request_log()
        .iter()
        .filter(|(p, _)| p == "/robots.txt")
        .count();
    assert_eq!(robots_requests, 1);
}

#[test]
fn robots_5xx_treats_site_as_disallowed() {
    let server = MockServer::new();
    server
        .route("/robots.txt", 500, "text/plain", "boom")
        .route("/page", 200, "text/html", "x");
    let base = server.serve();

    let fetcher = PoliteFetcher::new(fast_config());
    let result = fetcher.fetch(&base.join("/page").unwrap());
    assert!(matches!(result, Err(FetchError::Denied(_))), "{result:?}");
}

#[test]
fn ignore_robots_switch_disables_check() {
    let server = MockServer::new();
    server
        .route(
            "/robots.txt",
            200,
            "text/plain",
            "User-agent: *\nDisallow: /\n",
        )
        .route("/page", 200, "text/html", "x");
    let base = server.serve();

    let config = PoliteConfig {
        respect_robots: false,
        ..fast_config()
    };
    let fetcher = PoliteFetcher::new(config);
    assert!(fetcher.fetch(&base.join("/page").unwrap()).is_ok());
    assert_eq!(server.hits("/robots.txt"), 0, "关掉开关就不碰 robots");
}

#[test]
fn rate_limit_paces_requests_per_host() {
    let interval = Duration::from_millis(80);
    let server = MockServer::new();
    server.route("/p", 200, "text/html", "x");
    let base = server.serve();

    let config = PoliteConfig {
        min_interval: interval,
        ..fast_config()
    };
    let fetcher = PoliteFetcher::new(config);
    let start = Instant::now();
    for _ in 0..3 {
        fetcher.fetch(&base.join("/p").unwrap()).expect("fetch ok");
    }
    let elapsed = start.elapsed();
    // robots(1) + 3 次页面请求，相邻请求至少隔一个 interval：≥ 3 × 80ms
    let floor = 3 * interval;
    assert!(
        elapsed >= floor,
        "3 次请求应被限速拉开：elapsed = {elapsed:?} < {floor:?}"
    );
}

#[test]
fn retries_on_5xx_then_succeeds() {
    let server = MockServer::new();
    server.route_fail_first_n("/flaky", 2, "ok");
    let base = server.serve();

    let fetcher = PoliteFetcher::new(fast_config());
    let page = fetcher
        .fetch(&base.join("/flaky").unwrap())
        .expect("重试后成功");
    assert_eq!(page.body, b"ok");
    assert_eq!(server.hits("/flaky"), 3, "2 次 500 + 1 次成功");
}

#[test]
fn no_retry_on_404() {
    let server = MockServer::new();
    let base = server.serve();

    let fetcher = PoliteFetcher::new(fast_config());
    let result = fetcher.fetch(&base.join("/gone").unwrap());
    assert!(
        matches!(result, Err(FetchError::HttpStatus { status: 404, .. })),
        "{result:?}"
    );
}

#[test]
fn too_large_response_is_rejected() {
    let server = MockServer::new();
    server.route("/big", 200, "text/html", vec![b'x'; 1024]);
    let base = server.serve();

    let config = PoliteConfig {
        max_bytes: 64,
        ..fast_config()
    };
    let fetcher = PoliteFetcher::new(config);
    let result = fetcher.fetch(&base.join("/big").unwrap());
    assert!(matches!(result, Err(FetchError::TooLarge(_))), "{result:?}");
}

/// 契约钉子：`Fetcher::fetch_direct`（用户点名单资源）跳过 robots，而默认
/// `fetch` 仍受 robots 约束——github.com robots 禁 release 资产路径曾误拒
/// 钉版字体下载（2026-10）。跳 robots 不等于绕过 scheme 白名单与大小上限。
#[test]
fn fetch_direct_skips_robots_while_fetch_stays_bound() {
    let server = MockServer::new();
    server
        .route(
            "/robots.txt",
            200,
            "text/plain",
            "User-agent: *\nDisallow: /releases/\n",
        )
        .route("/releases/font.zip", 200, "application/zip", "ZIPDATA");
    let base = server.serve();
    let asset = base.join("/releases/font.zip").unwrap();

    let fetcher = PoliteFetcher::new(fast_config());

    // 对照：默认 fetch 遵 robots 被拒
    assert!(
        matches!(
            fetcher.fetch(&asset),
            Err(FetchError::Denied(ref msg)) if msg.contains("robots")
        ),
        "默认 fetch 必须仍受 robots 约束"
    );

    // 正例：点名单资源跳过 robots 拿到内容
    let page = fetcher
        .fetch_direct(&asset, None)
        .expect("点名单资源跳过 robots");
    assert_eq!(page.body, b"ZIPDATA");

    // max_bytes 显式上限取代配置默认（None 时用配置默认）
    assert!(matches!(
        fetcher.fetch_direct(&asset, Some(4)),
        Err(FetchError::TooLarge(_))
    ));

    // scheme 白名单不因跳 robots 而失效
    let ftp = "ftp://example.com/x.zip".parse().unwrap();
    assert!(matches!(
        fetcher.fetch_direct(&ftp, None),
        Err(FetchError::Denied(_))
    ));
}

// ---------------------------------------------------------------------------
// 传输层故障注入（todo 2026-10-28 发布前测试收尾）：连接中途 reset 与
// 响应截断走"网络错误"路径——与 5xx 同属可重试类
// ---------------------------------------------------------------------------

use crate::support::mock_server::FaultKind;

#[test]
fn connection_reset_mid_response_is_retried_then_fails() {
    // 永久故障路由：每次都在半截响应头处断连 → 重试耗尽后报网络错误
    let server = MockServer::new();
    server.route_fault("/flaky", FaultKind::ResetMidResponse);
    let base = server.serve();

    let fetcher = PoliteFetcher::new(fast_config());
    let result = fetcher.fetch(&base.join("/flaky").unwrap());
    assert!(
        matches!(result, Err(FetchError::Network(_))),
        "reset 属网络错误（重试耗尽）：{result:?}"
    );
    // 总尝试 = retries(2) + 1 = 3 次（含 robots 拉取不受此路由影响）
    assert_eq!(server.hits("/flaky"), 3, "中途 reset 必须重试到底");
}

#[test]
fn truncated_body_is_not_silently_accepted() {
    // 声明 1MB 只发 64 字节：截断响应不得被当作完整 body 静默接受
    let server = MockServer::new();
    server.route_fault("/cut", FaultKind::TruncatedBody);
    let base = server.serve();

    let fetcher = PoliteFetcher::new(fast_config());
    let result = fetcher.fetch(&base.join("/cut").unwrap());
    match result {
        // reqwest 对 Content-Length 不匹配的表现可能是网络错误或短 body——
        // 两种形态都不允许"成功返回 64 字节且无异常"的静默截断
        Err(FetchError::Network(_)) => {}
        Ok(page) => panic!("截断响应不得成功返回 {} 字节", page.body.len()),
        other => panic!("意外结果：{other:?}"),
    }
}
