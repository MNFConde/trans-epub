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
