//! epubcheck 验收门：jar 存在（`EPUBCHECK_JAR` 或 scripts/epubcheck.sh 的
//! 默认下载位置）时对**新鲜转换**的产物跑校验；不存在则跳过（本地无
//! Java 环境的场合由 scripts/epubcheck.sh 兜底）。
//!
//! 默认 jar 路径必须是编译期常量：测试进程 cwd 是 crate 根，运行时相对
//! 路径 `target/...` 永远 miss，门会恒 skip（假绿，ADR 0011 的教训）。

use crate::support::{convert, convert_web, tempfile_dir};

/// scripts/epubcheck.sh 的默认下载位置（workspace 根 target/）。
const DEFAULT_JAR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/epubcheck/epubcheck-5.2.1/epubcheck.jar"
);

#[test]
fn epubcheck_gate() {
    let jar = std::env::var("EPUBCHECK_JAR").unwrap_or_else(|_| DEFAULT_JAR.to_string());
    if !std::path::Path::new(&jar).exists() {
        eprintln!("skipping epubcheck gate: {jar} not found (run scripts/epubcheck.sh)");
        return;
    }
    for name in ["flat", "tree", "single", "mdbook"] {
        let output = convert(name);
        let tmp = tempfile_dir().join(format!("{name}.epub"));
        std::fs::write(&tmp, &output.bytes).expect("write temp epub");
        let status = std::process::Command::new("java")
            .arg("-jar")
            .arg(&jar)
            .arg(&tmp)
            .output()
            .expect("java available when jar exists");
        assert!(
            status.status.success(),
            "epubcheck failed for {name}:\n{}",
            String::from_utf8_lossy(&status.stdout)
        );
    }
    // web 样书也过验收门
    let output = convert_web();
    let tmp = tempfile_dir().join("web.epub");
    std::fs::write(&tmp, &output.bytes).expect("write temp epub");
    let status = std::process::Command::new("java")
        .arg("-jar")
        .arg(&jar)
        .arg(&tmp)
        .output()
        .expect("java available when jar exists");
    assert!(
        status.status.success(),
        "epubcheck failed for web:\n{}",
        String::from_utf8_lossy(&status.stdout)
    );
}
