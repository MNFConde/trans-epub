//! highlight 错误分支（todo 2026-10-28 发布前测试收尾）：未知语言静默
//! 回退（默认 unknown_language_fallback=true 不告警；关掉则告警）与
//! 超长单 token 的染色行为。单元级 API（highlight_code）已有覆盖，这里
//! 补 pass 层配置分支与超长输入。

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::testing::MemVfs;

use crate::support::{convert_vfs, zip_entries};

const MD_UNKNOWN_LANG: &str = "# T\n\n```brainfuck-x\nsome code\n```\n";

fn chapter_html(output: &trans_epub_contract::writer::BuildOutput) -> String {
    zip_entries(output)
        .into_iter()
        .find(|(name, _, _)| name.starts_with("OEBPS/Text/"))
        .map(|(_, _, bytes)| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

#[test]
fn unknown_language_falls_back_silently_by_default() {
    let output = convert_vfs(
        MemVfs::new().insert("doc.md", MD_UNKNOWN_LANG),
        ResolvedConfig::default(),
    );
    let body = chapter_html(&output);
    assert!(
        body.contains("some code"),
        "未知语言回退纯文本，内容不丢：{body}"
    );
    assert!(!body.contains("syn-code"), "回退块不加染色 class：{body}");
    assert!(
        !output
            .report
            .warnings
            .iter()
            .any(|w| w.message.contains("unknown code language")),
        "默认静默回退不告警（unknown_language_fallback=true）：{:?}",
        output.report.warnings
    );
}

#[test]
fn unknown_language_warns_when_fallback_disabled() {
    let mut config = ResolvedConfig::default();
    config.highlight.unknown_language_fallback = false;
    let output = convert_vfs(MemVfs::new().insert("doc.md", MD_UNKNOWN_LANG), config);
    assert!(
        output
            .report
            .warnings
            .iter()
            .any(|w| w.message.contains("unknown code language")),
        "关闭回退须告警：{:?}",
        output.report.warnings
    );
    // 内容仍不丢（回退纯文本是渲染策略，丢弃才是缺陷）
    assert!(chapter_html(&output).contains("some code"));
}

#[test]
fn very_long_single_token_highlights_without_hang() {
    // 100KB 单标识符：syntect 正则引擎对超长 token 的最坏情况——不能
    // 明显卡死（测试无超时机制，靠整体测试时长兜底观察）
    let token = "a".repeat(100 * 1024);
    let md = format!("```rust\nlet {token} = 1;\n```\n");
    let output = convert_vfs(
        MemVfs::new().insert("doc.md", md),
        ResolvedConfig::default(),
    );
    let body = chapter_html(&output);
    assert!(
        body.contains("syn-code"),
        "超长 token 照常染色：{}",
        &body[..200.min(body.len())]
    );
}
