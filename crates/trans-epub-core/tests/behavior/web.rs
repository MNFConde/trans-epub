//! web 前端行为：Pages 源抽取与改写、四站点预设样本（30-frontends.md）。

use std::collections::BTreeSet;
use std::sync::Arc;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::progress::NoProgress;
use trans_epub_contract::source::Source;
use trans_epub_contract::testing::MockFetcher;
use trans_epub_core::{Converter, Io};

use crate::support::{convert_web, utf8, web_page};

#[test]
fn web_pages_extraction_and_rewrite() {
    let output = convert_web();

    // spine 3 章：home → install → api（顺序即 Pages 顺序）
    let opf = utf8("OEBPS/content.opf", &output);
    let spine: Vec<&str> = opf
        .lines()
        .filter(|l| l.contains("<itemref"))
        .map(|l| l.trim())
        .collect();
    assert_eq!(
        spine,
        vec![
            "<itemref idref=\"ch00001\"/>",
            "<itemref idref=\"ch00002\"/>",
            "<itemref idref=\"ch00003\"/>",
        ],
        "spine 顺序即页面顺序"
    );
    assert!(
        opf.contains("<dc:title>Docs Home</dc:title>"),
        "书名 = 第一页 <title>"
    );

    let nav = utf8("OEBPS/nav.xhtml", &output);
    assert!(
        nav.contains(">Welcome</a>") && nav.contains(">Installation</a>"),
        "{nav}"
    );

    // ch1（home）：三种链接 + rust 染色 + 相对图片
    let ch1 = utf8("OEBPS/Text/ch00001.xhtml", &output);
    assert!(
        ch1.contains("href=\"ch00002.xhtml\""),
        "相对链接 → install：{ch1}"
    );
    assert!(
        ch1.contains("href=\"ch00003.xhtml#usage\""),
        "同源绝对链接 + 锚点：{ch1}"
    );
    assert!(
        ch1.contains("href=\"https://example.org/external\""),
        "外部链接保留可点：{ch1}"
    );
    assert!(
        ch1.contains("syn-source syn-rust"),
        "language-rust 存活 → 染色"
    );
    assert!(
        ch1.contains("../Images/arch.png"),
        "相对图片以页面 URL 为基登记嵌入"
    );
    assert!(!ch1.contains("site footer chrome"), "root 内 footer 剔除");

    // ch2（install）：上溯链接 / 未入书同源链接绝对化 / 跨源图片 / 方言
    let ch2 = utf8("OEBPS/Text/ch00002.xhtml", &output);
    assert!(ch2.contains("href=\"ch00001.xhtml\""), "../ 回 home：{ch2}");
    assert!(
        ch2.contains("href=\"https://docs.example.dev/missing-page.html\""),
        "未入书的同源链接绝对化保留：{ch2}"
    );
    assert!(ch2.contains("../Images/diagram.png"), "跨源图片嵌入");
    // bash 作用域是 source.shell.bash → 类链 syn-source syn-shell syn-bash
    assert!(ch2.contains("syn-source syn-shell"), "bash 染色：{ch2}");
    assert!(
        ch2.contains("<code>plain text block without language</code>"),
        "无 language class 的块保持纯文本：{ch2}"
    );
    assert!(
        !ch2.contains("sidebar chrome") && !ch2.contains("tracking") && !ch2.contains("subscribe"),
        "aside/script/form/button 剔除：{ch2}"
    );

    // ch3（api）：latin-1 解码 + slug 锚点目标
    let ch3 = utf8("OEBPS/Text/ch00003.xhtml", &output);
    assert!(ch3.contains("Référence"), "iso-8859-1 meta 解码：{ch3}");
    assert!(ch3.contains("<h2 id=\"usage\">"), "heading id 补齐：{ch3}");

    // 报告：恰 3 条 warning（dup / 非页面 / 未入书链接），无有损告警
    let codes: BTreeSet<String> = output
        .report
        .warnings
        .iter()
        .map(|w| w.code.as_str().to_string())
        .collect();
    assert_eq!(
        output.report.warnings.len(),
        3,
        "{:?}",
        output.report.warnings
    );
    assert!(codes.contains("duplicate-source"), "{codes:?}");
    assert!(codes.contains("page-extract-failed"), "{codes:?}");
    assert!(codes.contains("link-unresolved"), "{codes:?}");
    assert_eq!(output.report.stats.chapters, 3);
    assert_eq!(output.report.stats.assets_embedded, 2);
    assert_eq!(
        output.report.stats.links_rewritten, 5,
        "{:?}",
        output.report.stats
    );
    assert_eq!(output.report.stats.links_unresolved, 1);
}

/// 四个站点预设各一组样本页（30-frontends.md fixture 要求）：预设链命中
/// 正文根、chrome 剔除、language class 存活。vitepress 样本用真实的
/// `language-ts`——语法集为 syntect-assets 扩展 dump（含 TypeScript），
/// 四预设样本均应染色且 `language-*` class 存活。
#[test]
fn web_preset_samples_extract() {
    let samples = [
        ("mkdocs", "Material for MkDocs", true),
        ("docusaurus", "Docusaurus Docs", true),
        ("vitepress", "VitePress Guide", true),
        ("gitbook", "GitBook Page", true),
    ];
    for (name, h1, highlighted) in samples {
        let page = web_page(
            &format!("presets/{name}.html"),
            &format!("https://preset.dev/{name}.html"),
            "text/html",
        );
        let mut config = ResolvedConfig::default();
        config.web.preset = Some(name.to_string());
        let io = Io {
            fetcher: Arc::new(MockFetcher::new()),
            progress: Arc::new(NoProgress),
        };
        let output = Converter::new()
            .convert(&Source::Pages(vec![page]), &config, &io)
            .expect("preset conversion succeeds");
        assert_eq!(output.report.stats.chapters, 1, "{name}");
        assert!(
            output.report.warnings.is_empty(),
            "{name}: {:?}",
            output.report.warnings
        );
        let ch1 = utf8("OEBPS/Text/ch00001.xhtml", &output);
        if highlighted {
            assert!(
                ch1.contains("syn-source syn-"),
                "{name}: 代码块应染色：{ch1}"
            );
        } else {
            assert!(
                ch1.contains(r#"<code class="language-"#),
                "{name}: language class 必须存活：{ch1}"
            );
        }
        assert!(!ch1.contains("chrome"), "{name}: chrome 应剔除：{ch1}");
        assert!(ch1.contains(h1), "{name}: 章名 = 正文 h1：{ch1}");
    }
}
