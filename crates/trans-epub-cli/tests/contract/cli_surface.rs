//! CLI 接口契约：退出码语义（0 成功 / 1 转换失败 / 2 用法错误）、
//! dry-run 与 `--report json` 的 stdout 形态。

use crate::support::{FIXTURES, run};

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

#[test]
fn crawl_missing_local_input_is_usage_error() {
    let result = run(&["crawl", "no-such-list.txt"]);
    assert_eq!(result.code, 2, "stderr: {}", result.stderr);
}

// ---------------------------------------------------------------------------
// P3 错误路径：CLI 每个用户可触发错误一条断言（错误信息可读也是产品面）
// ---------------------------------------------------------------------------

#[test]
fn empty_input_tree_is_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let result = run(&[dir.path().to_str().unwrap()]);
    assert_eq!(result.code, 2, "空目录无 md 源：{}", result.stderr);
}

#[test]
fn missing_font_file_is_usage_error_with_readable_message() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.epub");
    let result = run(&[
        &format!("{FIXTURES}/single/article.md"),
        "-o",
        out.to_str().unwrap(),
        "--embed-fonts",
        "--font-file",
        "no-such-font.ttf",
    ]);
    assert_eq!(result.code, 2, "stderr: {}", result.stderr);
    assert!(
        result.stderr.contains("read font"),
        "错误信息应指明读字体失败：{}",
        result.stderr
    );
}

/// 现状口径：`--font-file` 内容非法时宽容降级——warning 跳过字体内嵌、
/// 产物照常出书（用户字体是增强项不是硬依赖）。
#[test]
fn garbage_font_file_degrades_to_warning_not_failure() {
    let dir = tempfile::tempdir().unwrap();
    let font = dir.path().join("bad.ttf");
    std::fs::write(&font, b"definitely not a font").unwrap();
    let out = dir.path().join("x.epub");
    let result = run(&[
        &format!("{FIXTURES}/single/article.md"),
        "-o",
        out.to_str().unwrap(),
        "--embed-fonts",
        "--font-file",
        font.to_str().unwrap(),
        "--report",
        "json",
    ]);
    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    assert!(out.exists(), "产物照常出书");
    let report: serde_json::Value =
        serde_json::from_str(&result.stdout).unwrap_or_else(|e| panic!("invalid json: {e}"));
    let warned = report["warnings"].as_array().is_some_and(|ws| {
        ws.iter()
            .any(|w| w["message"].as_str() == Some("用户字体子集化失败，跳过字体内嵌"))
    });
    assert!(warned, "报告应含降级 warning：{}", result.stdout);
}

#[test]
fn missing_extra_css_is_usage_error_with_readable_message() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.epub");
    let result = run(&[
        &format!("{FIXTURES}/single/article.md"),
        "-o",
        out.to_str().unwrap(),
        "--extra-css",
        "no-such.css",
    ]);
    assert_eq!(result.code, 2, "stderr: {}", result.stderr);
    assert!(
        result.stderr.contains("read extra css"),
        "错误信息应指明读 CSS 失败：{}",
        result.stderr
    );
}

/// 故障注入：落盘失败（输出路径被目录占位）→ 退出码 1（转换失败，
/// 非用法错误）+ 错误信息可读。现状口径：writer 产物在内存，单次
/// fs::write 落盘——不存在"写一半"的中间态；本测试同时钉住"失败
/// 不静默"。
#[test]
fn write_failure_is_fatal_with_readable_message() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("blocked.epub");
    std::fs::create_dir(&out).unwrap(); // 输出路径被目录占位 → write 必败
    let result = run(&[
        &format!("{FIXTURES}/single"),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(result.code, 1, "落盘失败是转换失败：{}", result.stderr);
    assert!(
        result.stderr.contains("write"),
        "错误信息应指明写文件失败：{}",
        result.stderr
    );
    assert!(out.is_dir(), "失败后路径保持原状（无半成品文件）");
}
