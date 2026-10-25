//! CLI 端到端：跑真实二进制（`CARGO_BIN_EXE_trans-epub`），覆盖退出码、
//! 产物、dry-run 与机器可读报告（docs/architecture/60-cli.md「测试」）。
//! 全程不访问网络：只用无远程引用的 fixtures。

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_trans-epub");
const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../trans-epub-core/tests/fixtures"
);

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Out {
    let output = Command::new(BIN).args(args).output().expect("binary runs");
    Out {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn converts_tree_fixture_to_epub() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("tree.epub");
    let result = run(&[
        &format!("{FIXTURES}/tree"),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    let bytes = std::fs::read(&out).expect("epub written");
    assert!(bytes.len() > 4000, "epub too small: {}", bytes.len());
    assert_eq!(&bytes[..2], b"PK", "zip magic");
}

#[test]
fn single_file_input_defaults_output_name() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("article.md");
    std::fs::copy(format!("{FIXTURES}/single/article.md"), &out).unwrap();
    let result = run(&[out.to_str().unwrap(), "--quiet"]);
    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    assert!(
        std::path::Path::new("article.epub").exists(),
        "默认输出 = 当前目录下以输入名命名的 .epub"
    );
    let _ = std::fs::remove_file("article.epub");
}

#[test]
fn dry_run_prints_outline_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let result = run(&[
        &format!("{FIXTURES}/single"),
        "--dry-run",
        "--output",
        dir.path().join("x.epub").to_str().unwrap(),
    ]);
    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("md-file"),
        "stdout: {}",
        result.stdout
    );
    assert!(result.stdout.contains("Single Article"));
    assert!(!dir.path().join("x.epub").exists(), "dry-run 不应产出文件");
}

#[test]
fn report_json_is_machine_readable() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("t.epub");
    let result = run(&[
        &format!("{FIXTURES}/tree"),
        "-o",
        out.to_str().unwrap(),
        "--report",
        "json",
        "--quiet",
    ]);
    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    let report: serde_json::Value = serde_json::from_str(&result.stdout)
        .unwrap_or_else(|e| panic!("invalid json report: {e}\n{}", result.stdout));
    assert_eq!(report["stats"]["chapters"], 6);
    assert_eq!(report["warnings"].as_array().map(Vec::len), Some(0));
}

#[test]
fn missing_input_is_usage_error() {
    let result = run(&["no-such-input-dir"]);
    assert_eq!(result.code, 2);
}

#[test]
fn strict_mode_turns_warnings_into_failure() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.md"),
        "# Broken\n\nA [broken link](nope.md) here.\n",
    )
    .unwrap();
    let out = dir.path().join("a.epub");

    let lax = run(&[
        dir.path().to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(lax.code, 0, "默认容忍 warning：{}", lax.stderr);
    assert!(out.exists());

    let strict = run(&[
        dir.path().to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
        "--strict",
    ]);
    assert_eq!(strict.code, 1, "--strict 下 warning 视为失败");
}

// ---------------------------------------------------------------------------
// crawl 子命令：回环迷你 HTTP 服务，端到端不出网（60-cli.md「测试」）
// ---------------------------------------------------------------------------

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

#[test]
fn crawl_missing_local_input_is_usage_error() {
    let result = run(&["crawl", "no-such-list.txt"]);
    assert_eq!(result.code, 2, "stderr: {}", result.stderr);
}
