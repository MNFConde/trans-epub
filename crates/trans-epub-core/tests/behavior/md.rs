//! markdown 前端行为：flat（保真全景：链接改写/高亮/图片降级/锚点）、
//! tree（嵌套与顺序、封面）、single（单文件成书）。

use std::collections::BTreeSet;
use std::sync::Arc;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::source::Source;
use trans_epub_contract::testing::MemVfs;
use trans_epub_core::Converter;

use crate::support::{FIXTURES, convert, utf8, zip_entries};

#[test]
fn flat_structure_and_fidelity() {
    let output = convert("flat");
    let names: Vec<String> = zip_entries(&output)
        .into_iter()
        .map(|(n, _, _)| n)
        .collect();
    assert_eq!(names.first().unwrap(), "mimetype", "mimetype 必须是首条目");
    for expected in [
        "META-INF/container.xml",
        "OEBPS/content.opf",
        "OEBPS/nav.xhtml",
        "OEBPS/Text/ch00001.xhtml",
        "OEBPS/Text/ch00002.xhtml",
        "OEBPS/Text/ch00003.xhtml",
        "OEBPS/Images/diagram.png",
        "OEBPS/Images/remote.png",
        "OEBPS/Styles/base.css",
        "OEBPS/Styles/code.css",
        "OEBPS/Styles/admonition.css",
    ] {
        assert!(
            names.contains(&expected.to_string()),
            "missing {expected} in {names:?}"
        );
    }
    assert!(
        !names.iter().any(|n| n.contains("ghost")),
        "missing 图片不应落盘"
    );

    // mimetype STORED
    let mut entries = zip_entries(&output);
    let (_, stored, body) = entries.remove(0);
    assert!(stored, "mimetype 必须不压缩");
    assert_eq!(body, b"application/epub+zip");

    // 章内保真：链接改写 / 高亮 / 图片降级 / 锚点
    let ch1 = utf8("OEBPS/Text/ch00001.xhtml", &output);
    assert!(ch1.contains("href=\"ch00002.xhtml\""), "内部链接应改写");
    assert!(
        ch1.contains("href=\"ch00003.xhtml#footnotes--alerts\""),
        "锚点命中应保留（slug 与 heading id 一致）：{ch1}"
    );
    assert!(
        ch1.contains("href=\"ch00003.xhtml\""),
        "落空锚点应去掉 fragment（保持 epubcheck 干净）"
    );
    assert!(!ch1.contains("#nope"), "落空锚点不得残留");
    assert!(!ch1.contains("missing.md"), "断链应去掉 href");
    assert!(ch1.contains("broken link"), "断链保留文字");
    assert!(ch1.contains("../Images/diagram.png"));
    assert!(ch1.contains("../Images/remote.png"), "远程图片应嵌入");
    assert!(ch1.contains("[missing image: ghost]"), "缺图降级为占位文本");
    // data: URI 图片 → 合成文件名
    assert!(ch1.contains("../Images/img00001.png"));

    let ch2 = utf8("OEBPS/Text/ch00002.xhtml", &output);
    assert!(ch2.contains("syn-code"), "代码块应挂主题基类");
    assert!(ch2.contains("syn-source syn-rust"), "syntect 染色 span");
    assert!(
        ch2.contains("+++[-&gt;+++&lt;]&gt;++."),
        "未知语言保持纯文本（HTML 转义形态）：{ch2}"
    );
    assert!(ch2.contains("task-list-item"), "任务列表 class");

    let ch3 = utf8("OEBPS/Text/ch00003.xhtml", &output);
    assert!(ch3.contains("admonition"), "GitHub alert → admonition");
    assert!(
        ch3.contains(
            "<aside class=\"footnote footnote-item\" epub:type=\"footnote\" id=\"fn-1\" role=\"doc-footnote\">"
        ),
        "脚注体 → aside 语义化（id 保留锚点兼容）：{ch3}"
    );
    assert!(
        ch3.contains("<a epub:type=\"noteref\" role=\"doc-noteref\" href=\"#fn-1\""),
        "脚注引用 noteref：{ch3}"
    );
    assert!(
        ch3.contains("epub:type=\"backlink\""),
        "脚注回链 backlink：{ch3}"
    );
    assert!(!ch3.contains("data-footnotes"), "comrak 包裹层摘除：{ch3}");
    assert!(
        ch1.contains("xmlns:epub=\"http://www.idpf.org/2007/ops\""),
        "章节页根元素统一绑 epub 命名空间（无脚注章也一样）"
    );

    // 元数据：根 README 的 front matter title → 书名
    let opf = utf8("OEBPS/content.opf", &output);
    assert!(opf.contains("<dc:title>Sample Book</dc:title>"));
    assert!(opf.contains("urn:uuid:"));
    assert!(opf.contains("2000-01-01T00:00:00Z"));

    // landmarks nav + OPF guide（EPUB 输出增强三件之二；flat 无封面）
    let nav = utf8("OEBPS/nav.xhtml", &output);
    assert!(
        nav.contains("<nav epub:type=\"landmarks\" id=\"landmarks\" hidden=\"hidden\">"),
        "{nav}"
    );
    assert!(
        nav.contains("<a epub:type=\"bodymatter\" href=\"Text/ch00001.xhtml\">Begin Reading</a>"),
        "{nav}"
    );
    assert!(!nav.contains("href=\"#toc\""), "toc 自引用被 RSC-011 拒绝");
    assert!(opf.contains("  <guide>\n"), "{opf}");
    assert!(
        opf.contains("<reference type=\"toc\" title=\"Table of Contents\" href=\"nav.xhtml\"/>"),
        "{opf}"
    );
    assert!(
        opf.contains(
            "<reference type=\"text\" title=\"Begin Reading\" href=\"Text/ch00001.xhtml\"/>"
        ),
        "{opf}"
    );

    // 报告：容忍轨
    let codes: BTreeSet<String> = output
        .report
        .warnings
        .iter()
        .map(|w| w.code.as_str().to_string())
        .collect();
    assert!(codes.contains("asset-fetch-failed"), "{codes:?}");
    assert!(codes.contains("link-unresolved"), "{codes:?}");
    assert_eq!(output.report.stats.chapters, 3);
    assert_eq!(output.report.stats.assets_embedded, 3);
    assert_eq!(output.report.stats.assets_missing, 1);
    assert!(
        output.report.stats.links_rewritten >= 5,
        "{:?}",
        output.report.stats
    );
    // 未解析：断链、缺图、落空锚点
    assert_eq!(output.report.stats.links_unresolved, 3);
}

#[test]
fn tree_nesting_and_order() {
    // tree golden 带封面：锁住 cover-image / 封面页 / legacy meta 的正确性
    let output = convert("tree");
    // spine：封面页 → README → tips(FM order=0) → notes → guide/README → install → usage
    let opf = utf8("OEBPS/content.opf", &output);
    assert!(opf.contains("properties=\"cover-image\""), "封面图属性");
    assert!(
        opf.contains("<meta name=\"cover\" content="),
        "legacy 封面 meta"
    );
    assert!(
        opf.contains("OEBPS/Text/cover.xhtml") || opf.contains("Text/cover.xhtml"),
        "封面页进 manifest"
    );
    let spine: Vec<&str> = opf
        .lines()
        .filter(|l| l.contains("<itemref"))
        .map(|l| l.trim())
        .collect();
    assert_eq!(
        spine,
        vec![
            "<itemref idref=\"cover-page\"/>",
            "<itemref idref=\"ch00001\"/>",
            "<itemref idref=\"ch00002\"/>",
            "<itemref idref=\"ch00003\"/>",
            "<itemref idref=\"ch00004\"/>",
            "<itemref idref=\"ch00005\"/>",
            "<itemref idref=\"ch00006\"/>",
        ],
        "spine 顺序：封面、README、tips、notes、guide/README、install、usage"
    );
    let cover = utf8("OEBPS/Text/cover.xhtml", &output);
    assert!(cover.contains("../Images/cover.png"), "封面页引用嵌入图片");

    // nav 嵌套：guide 分组（README 上提为分组落点）带 children
    let nav = utf8("OEBPS/nav.xhtml", &output);
    let group_pos = nav
        .find("<li><a href=\"Text/ch00004.xhtml\">The Guide</a>")
        .expect("guide 分组节点指向其 README");
    let child_a = nav
        .find("<a href=\"Text/ch00005.xhtml\">install</a>")
        .expect("install");
    let child_b = nav
        .find("<a href=\"Text/ch00006.xhtml\">usage</a>")
        .expect("usage");
    assert!(
        group_pos < child_a && group_pos < child_b,
        "子项须在分组之后"
    );
    assert!(
        nav.contains("<a href=\"Text/ch00002.xhtml\">tips</a>"),
        "FM order=0 的 tips 排在普通文件之前"
    );
    assert_eq!(output.report.stats.chapters, 6);
    assert!(
        output.report.warnings.is_empty(),
        "tree fixture 不应产生 warning：{:?}",
        output.report.warnings
    );
}

#[test]
fn single_file_book() {
    // probe：kind = md-file
    let vfs = MemVfs::from_dir(format!("{FIXTURES}/single")).unwrap();
    let converter = Converter::new();
    let matched = converter.probe(
        &Source::Vfs(Arc::new(vfs.clone())),
        &ResolvedConfig::default(),
    );
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].0, "md");
    assert_eq!(matched[0].1.kind, "md-file");

    let output = convert("single");
    assert_eq!(output.report.stats.chapters, 1);
    let opf = utf8("OEBPS/content.opf", &output);
    assert!(
        opf.contains("<dc:title>Single Article</dc:title>"),
        "单文件 FM title → 书名"
    );
    assert!(output.report.warnings.is_empty());
}
