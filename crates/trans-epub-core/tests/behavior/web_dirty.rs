//! web 前端脏输入边角（todo 2026-10-28 发布前测试收尾）：坏 charset 声明、
//! 重复 id、超深嵌套、巨属性值——抽取链宽容降级不崩，产物照常出书。

use std::sync::Arc;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::progress::NoProgress;
use trans_epub_contract::source::{FetchedPage, Source};
use trans_epub_contract::testing::MockFetcher;
use trans_epub_core::{Converter, Io};

fn convert_pages(pages: Vec<FetchedPage>) -> trans_epub_contract::writer::BuildOutput {
    let io = Io {
        fetcher: Arc::new(MockFetcher::new()),
        progress: Arc::new(NoProgress),
    };
    Converter::new()
        .convert(&Source::Pages(pages), &ResolvedConfig::default(), &io)
        .expect("脏输入也不应让转换失败——降级出书是设计意图")
}

fn page(body: &str, content_type: &str) -> FetchedPage {
    FetchedPage::from_bytes(
        "https://a.dev/p1".parse().unwrap(),
        body.as_bytes().to_vec(),
        content_type,
    )
}

#[test]
fn bogus_charset_label_falls_back_gracefully() {
    // charset 标签非法（encoding_rs for_label 不认）→ 回落 UTF-8 解码
    let pages = vec![page(
        "<html><head><meta charset='not-a-real-charset'></head>\
         <body><main><p>ok</p></main></body></html>",
        "text/html",
    )];
    let output = convert_pages(pages);
    assert_eq!(output.report.stats.chapters, 1);
}

#[test]
fn gbk_bytes_with_declared_charset_decode_or_warn() {
    // 声明 GBK 且字节真是 GBK → 正常解码（"中文" 的 GBK 编码）
    let gbk = [0xd6, 0xd0, 0xce, 0xc4]; // "中文"
    let mut body = b"<html><head><meta charset='gbk'></head><body><main><p>".to_vec();
    body.extend_from_slice(&gbk);
    body.extend_from_slice(b"</p></main></body></html>");
    let pages = vec![FetchedPage::from_bytes(
        "https://a.dev/p1".parse().unwrap(),
        body,
        "text/html",
    )];
    let output = convert_pages(pages);
    assert_eq!(output.report.stats.chapters, 1);
    // 有损与否取决于解码结果——GBK 四字节两汉字有效，不应有损
    assert!(
        !output
            .report
            .warnings
            .iter()
            .any(|w| w.message.contains("lossy")),
        "有效 GBK 字节不应记有损 warning：{:?}",
        output.report.warnings
    );
}

#[test]
fn duplicate_ids_and_deep_nesting_extract() {
    // 重复 id + 500 层 div 嵌套（html5ever 解析器不得爆栈，正文抽得出）
    let mut deep = String::new();
    for _ in 0..500 {
        deep.push_str("<div id='dup'>");
    }
    deep.push_str("<p>deep content</p>");
    for _ in 0..500 {
        deep.push_str("</div>");
    }
    let pages = vec![page(
        &format!("<html><body><main>{deep}</main></body></html>"),
        "text/html",
    )];
    let output = convert_pages(pages);
    assert_eq!(output.report.stats.chapters, 1, "深嵌套照常出章");
}

#[test]
fn huge_attribute_value_survives() {
    // 100KB 属性值（真实世界见过 data-* 塞 base64 缩略图的）——不崩不挂
    let huge_attr = "x".repeat(100 * 1024);
    let pages = vec![page(
        &format!("<html><body><main data-thumb='{huge_attr}'><p>ok</p></main></body></html>"),
        "text/html",
    )];
    let output = convert_pages(pages);
    assert_eq!(output.report.stats.chapters, 1);
}
