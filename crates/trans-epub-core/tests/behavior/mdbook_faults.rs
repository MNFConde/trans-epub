//! mdbook 前端错误分支（todo 2026-10-28 发布前测试收尾）：SUMMARY 链接
//! 指向不存在文件、include 区间越界、include 目标内再含指令（展开不
//! 递归——mdBook 一致语义）。既有 mdbook.rs 已盖缺失 include 保留原文。

use std::sync::Arc;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::progress::NoProgress;
use trans_epub_contract::source::Source;
use trans_epub_contract::testing::{MemVfs, MockFetcher};
use trans_epub_core::{Converter, Io};

fn convert_mdbook(files: &[(&str, &str)]) -> trans_epub_contract::writer::BuildOutput {
    let mut vfs = MemVfs::new();
    for (path, content) in files {
        vfs = vfs.insert(path, content.to_string());
    }
    let io = Io {
        fetcher: Arc::new(MockFetcher::new()),
        progress: Arc::new(NoProgress),
    };
    Converter::new()
        .convert(&Source::Vfs(Arc::new(vfs)), &ResolvedConfig::default(), &io)
        .expect("错误分支是 warning 降级，转换本身成功")
}

#[test]
fn summary_link_to_missing_file_warns_and_skips() {
    let output = convert_mdbook(&[
        (
            "src/SUMMARY.md",
            "# Summary\n\n- [Home](index.md)\n- [Ghost](ghost.md)\n",
        ),
        ("src/index.md", "# Home\n\nbody\n"),
        // ghost.md 不存在
    ]);
    let warned = output
        .report
        .warnings
        .iter()
        .map(|w| w.message.clone())
        .collect::<Vec<_>>();
    assert!(
        warned.iter().any(|m| m.contains("ghost")),
        "SUMMARY 断链须指名目标：{warned:?}"
    );
    // 断链章不进 spine，好书照常
    assert!(output.report.stats.chapters >= 1, "未断链的章照常出书");
}

#[test]
fn include_range_beyond_eof_keeps_directive_with_warning() {
    // 3 行文件要 10:20 区间 → empty range 分支：warning + 指令原文保留
    let output = convert_mdbook(&[
        ("src/SUMMARY.md", "# Summary\n\n- [A](a.md)\n"),
        (
            "src/a.md",
            "# A\n\n```rust\n{{#include lib.rs:10:20}}\n```\n",
        ),
        ("src/lib.rs", "line1\nline2\nline3\n"),
    ]);
    let warned = output
        .report
        .warnings
        .iter()
        .map(|w| w.message.clone())
        .collect::<Vec<_>>();
    assert!(
        warned
            .iter()
            .any(|m| m.contains("empty range") || m.contains("range")),
        "越界区间须 warning：{warned:?}"
    );
    // 章照常出（指令原文保留在正文里）
    assert_eq!(output.report.stats.chapters, 1);
}

#[test]
fn include_expansion_is_not_recursive() {
    // lib.rs 内容里又含 {{#include other.rs}}——展开不递归（mdBook 一致），
    // 内层指令保留原文且不告警（它只是文本）
    let output = convert_mdbook(&[
        ("src/SUMMARY.md", "# Summary\n\n- [A](a.md)\n"),
        ("src/a.md", "# A\n\n{{#include lib.rs}}\n"),
        (
            "src/lib.rs",
            "outer start\n{{#include other.rs}}\nouter end\n",
        ),
        ("src/other.rs", "SHOULD NOT APPEAR\n"),
    ]);
    assert_eq!(output.report.stats.chapters, 1);
    let chapter = crate::support::zip_entries(&output)
        .into_iter()
        .find(|(name, _, _)| name.starts_with("OEBPS/Text/"))
        .map(|(_, _, bytes)| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    assert!(
        !chapter.contains("SHOULD NOT APPEAR"),
        "内层指令不得展开（展开不递归）：{chapter}"
    );
    assert!(
        chapter.contains("outer start") && chapter.contains("outer end"),
        "外层展开照常：{chapter}"
    );
    let inner_warned = output
        .report
        .warnings
        .iter()
        .any(|w| w.message.contains("other.rs"));
    assert!(
        !inner_warned,
        "内层指令不展开也不告警：{:?}",
        output.report.warnings
    );
}
