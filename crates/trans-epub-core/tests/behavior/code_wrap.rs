//! code-wrap pass 层行为（ADR-0007 硬折行）：Hard 模式下 pre 块在
//! highlight 之后的文本节点被硬切、列位跨 span 连续；width=0 守卫与
//! 病态节点跳过。默认 PreWrap 零改写（golden 字节回归已锚，此处不重复）。

use trans_epub_contract::config::{ResolvedConfig, WrapMode};
use trans_epub_contract::testing::MemVfs;

use crate::support::{convert_vfs, zip_entries};

fn hard_config(width: u32) -> ResolvedConfig {
    let mut config = ResolvedConfig::default();
    config.wrap.mode = WrapMode::Hard { width };
    config
}

fn convert_md(md: &str, config: ResolvedConfig) -> String {
    let output = convert_vfs(MemVfs::new().insert("doc.md", md), config);
    zip_entries(&output)
        .into_iter()
        .find(|(name, _, _)| name.starts_with("OEBPS/Text/"))
        .map(|(_, _, bytes)| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

#[test]
fn hard_mode_wraps_long_code_lines_across_spans() {
    // 12 个 a 无语言围栏（无染色，单文本节点）+ 带 rust 染色的行（跨
    // span：列位连续计数，折行点可落在染色 span 内部）
    let md = "```text\naaaaaaaaaaaa\n```\n\n```rust\nlet x = 1000000;\n```\n";
    let body = convert_md(md, hard_config(8));

    // 12 个 a 在第 8 列处硬切：8 + 4
    assert!(body.contains("aaaaaaaa\naaaa"), "超宽行应被硬切：{body}");
    // 染色行同样硬切，且折行点落在染色 span 内部（列位跨 span 连续）：
    // "let x = " 恰好 8 列触顶，换行插进数字 span 开头（span 边界不重置列位）
    assert!(
        body.contains("=</span> <span class=\"syn-constant syn-numeric syn-integer syn-decimal syn-rust\">\n1000000</span>"),
        "染色代码行按 8 列硬切、折行点落 span 内：{body}"
    );
    // 原始超宽行不应整体存活
    assert!(
        !body.contains("aaaaaaaaaaaa"),
        "原始 12 连 a 不应整体存活：{body}"
    );
}

#[test]
fn hard_mode_zero_width_guard_is_noop() {
    let md = "```text\naaaaaaaaaaaa\n```\n";
    let body = convert_md(md, hard_config(0));
    assert!(
        body.contains("aaaaaaaaaaaa"),
        "width=0 守卫：零操作，内容原样：{body}"
    );
}

#[test]
fn hard_mode_keeps_short_lines_untouched() {
    let md = "```text\nshort\nok\n```\n";
    let body = convert_md(md, hard_config(8));
    assert!(
        body.contains("<pre") && body.contains("short"),
        "短行原样保留：{body}"
    );
}

/// 配置 serde 往返：CLI 从 TOML 读进来的路径（kebab-case 嵌套形态）。
#[test]
fn hard_mode_serde_roundtrip_from_toml() {
    let parsed: WrapMode = toml::from_str("hard = { width = 8 }").expect("serde 往返");
    assert_eq!(parsed, WrapMode::Hard { width: 8 });
}
