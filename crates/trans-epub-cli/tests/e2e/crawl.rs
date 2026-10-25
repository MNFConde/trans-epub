//! crawl 子命令端到端：回环迷你 HTTP 服务，不出网（60-cli.md「测试」）。

use crate::support::run;

/// 起一个最小静态服务：`(path, body)` 应答 200 text/html，其余 404。
/// 返回 base URL；robots.txt 未注册（404 → 采集器视同无限制）。
fn serve_pages(pages: Vec<(String, String)>) -> String {
    use std::io::{Read, Write};
    use std::sync::Arc;
    let pages = Arc::new(pages);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let Ok(n) = stream.read(&mut buf) else {
                continue;
            };
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, body) = pages
                .iter()
                .find(|(p, _)| *p == path)
                .map(|(_, b)| ("200 OK", b.clone()))
                .unwrap_or_else(|| ("404 Not Found", "not found".to_string()));
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

fn crawl_site_pages() -> Vec<(String, String)> {
    vec![
        (
            "/".into(),
            r#"<html><head><title>Crawl Book</title></head><body><main>
            <h1>Home</h1>
            <p>See <a href="/guide/one.html">chapter one</a>.</p>
            <pre><code class="language-rust">fn a() {}</code></pre>
            </main></body></html>"#
                .to_string(),
        ),
        (
            "/guide/one.html".into(),
            r#"<html><head><title>One</title></head><body><main>
            <h1>Chapter One</h1><p>body</p>
            </main></body></html>"#
                .to_string(),
        ),
    ]
}

#[test]
fn crawl_local_site_to_epub() {
    let base = serve_pages(crawl_site_pages());
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("site.epub");
    let result = run(&[
        "crawl",
        &base,
        "-o",
        out.to_str().unwrap(),
        "--rate-ms",
        "1",
        "--depth",
        "1",
        "--quiet",
    ]);
    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    let bytes = std::fs::read(&out).expect("epub written");
    assert_eq!(&bytes[..2], b"PK", "zip magic");
    assert!(bytes.len() > 3000, "epub too small: {}", bytes.len());
}

#[test]
fn crawl_dry_run_prints_url_list_without_writing() {
    let base = serve_pages(crawl_site_pages());
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("site.epub");
    let result = run(&[
        "crawl",
        &base,
        "-o",
        out.to_str().unwrap(),
        "--rate-ms",
        "1",
        "--dry-run",
    ]);
    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("2 page(s)"),
        "stdout: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("/guide/one.html"),
        "stdout: {}",
        result.stdout
    );
    assert!(!out.exists(), "dry-run 不应产出文件");
}
