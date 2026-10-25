//! mdBook 前端行为：SUMMARY 嵌套 / part / draft / include 区间 /
//! rustdoc 锚点 / hide / 缺失 include（30-frontends.md 的 fixture 要求）。

use std::collections::BTreeSet;

use crate::support::{convert, plain_text, utf8};

#[test]
fn mdbook_structure_parts_and_directives() {
    // 30-frontends.md 的 mdbook fixture 要求：嵌套 SUMMARY（3 层）、part、
    // draft、include 区间、rustdoc 锚点、hide、缺失 include 目标
    let output = convert("mdbook");

    // spine：intro → ch1 → sub → ch2 → play → appendix（draft 跳过、
    // part 是分组不进 spine、hidden 未列出不进书）
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
            "<itemref idref=\"ch00004\"/>",
            "<itemref idref=\"ch00005\"/>",
            "<itemref idref=\"ch00006\"/>",
        ],
        "spine 顺序即 SUMMARY 顺序"
    );

    // book.toml 元数据收割（源内元数据 > 配置兜底）
    assert!(opf.contains("<dc:title>The Little MDBook</dc:title>"));
    assert!(opf.contains("Ada Lovelace"));

    // nav：part 分组（span）+ 嵌套
    let nav = utf8("OEBPS/nav.xhtml", &output);
    assert!(nav.contains("<span>Part One</span>"), "{nav}");
    assert!(nav.contains("<span>Part Two</span>"), "{nav}");
    let ch1_pos = nav
        .find("<a href=\"Text/ch00002.xhtml\">Chapter 1</a>")
        .expect("chapter 1");
    let deep_pos = nav.find("Deep nested").expect("deep nested");
    let ch2_pos = nav.find("Chapter 2</a>").expect("chapter 2");
    assert!(
        ch1_pos < deep_pos && deep_pos < ch2_pos,
        "Deep nested 应嵌套在 Chapter 1 与 Chapter 2 之间"
    );
    assert!(!nav.contains("Draft chapter"), "draft 不得进 nav");
    assert!(!nav.contains("Hidden"), "SUMMARY 未列文件不得进 nav");

    // intro：内部链接以章节为基解析 → 书内跳转
    let intro = utf8("OEBPS/Text/ch00001.xhtml", &output);
    assert!(intro.contains("href=\"ch00002.xhtml\""), "{intro}");

    // ch1：include 的锚点对 / 行区间 / 单数字到尾
    let ch1 = utf8("OEBPS/Text/ch00002.xhtml", &output);
    assert!(ch1.contains("hello, mdbook"), "锚点区间内容");
    assert!(!ch1.contains("ANCHOR"), "锚点注释行必须剥掉");
    assert!(ch1.contains("line two") && ch1.contains("line four"));
    assert!(!ch1.contains("line one"), "2:4 区间不含首行");
    assert!(ch1.contains("line five"), "单数字区间到文件尾");

    // ch2：rustdoc_include 行号锚点注释 + {{#hide}} 删行
    let ch2 = utf8("OEBPS/Text/ch00004.xhtml", &output);
    //（高亮会把注释的 "//" 包进 span，断言去掉前缀）
    assert!(ch2.contains("src/code/item.rs:3-5"), "{ch2}");
    assert!(ch2.contains("struct") && ch2.contains("Widget"), "{ch2}");
    assert!(ch2.contains("visible"), "{ch2}");
    assert!(!ch2.contains("inline_hidden"), "hide 行必须删除");

    // play：playground 折为代码块；原文件链接断链降级为文字
    let play = utf8("OEBPS/Text/ch00005.xhtml", &output);
    assert!(play.contains("playground example"), "{play}");
    assert!(play.contains("playground source"), "断链保留文字");
    assert!(!play.contains("example.rs"), "断链不得残留 href");

    // appendix：缺失 include / 未知指令 → 原文保留。指令文本在代码块内，
    // 高亮 span 会把连续文本逐 token 切开，断言先去标签再比对纯文本。
    let appendix = utf8("OEBPS/Text/ch00006.xhtml", &output);
    let plain = plain_text(&appendix);
    assert!(plain.contains("{{#include code/missing.rs}}"), "{plain}");
    assert!(plain.contains("{{#if feature}}"));

    // 报告：draft(structure-ambiguous) + missing include 与未知指令
    // (directive-unresolved ×2) + playground 断链(link-unresolved)
    let codes: BTreeSet<String> = output
        .report
        .warnings
        .iter()
        .map(|w| w.code.as_str().to_string())
        .collect();
    assert!(codes.contains("structure-ambiguous"), "{codes:?}");
    assert!(codes.contains("directive-unresolved"), "{codes:?}");
    assert!(codes.contains("link-unresolved"), "{codes:?}");
    assert_eq!(
        output.report.warnings.len(),
        4,
        "{:?}",
        output.report.warnings
    );
    assert_eq!(output.report.stats.chapters, 6);
}
