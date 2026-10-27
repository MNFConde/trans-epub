//! 规模边界四例（SQLite 巨量数据压测的对应物）：超长单行代码块、超大
//! 章节数（万章 spine）、超深目录嵌套、CJK 扩展 B 4 字节码点过字体
//! 子集——断言"不炸、产物结构正确"。规模取"能覆盖边界性质且 debug
//! profile 秒级完成"的量级；性能悬崖监控不是目标（无基线需求）。

use trans_epub_contract::config::{FontSource, FontSubsetMode, ResolvedConfig};
use trans_epub_contract::testing::MemVfs;

use crate::support::{convert_vfs, zip_entries};

/// 10MB 单行代码块（无语言围栏）：管线吞吐 + pre-wrap 长行处理——
/// 长行处理是产品卖点，超长输入不得炸或静默截断。
#[test]
fn huge_single_line_code_block_converts() {
    let line = "a".repeat(10 * 1024 * 1024);
    let md = format!("```\n{line}\n```\n");
    let output = convert_vfs(
        MemVfs::new().insert("doc.md", md),
        ResolvedConfig::default(),
    );

    assert_eq!(output.report.stats.chapters, 1);
    let chapter_xhtml = zip_entries(&output)
        .into_iter()
        .find(|(name, _, _)| name.starts_with("OEBPS/Text/"))
        .expect("章节条目存在");
    let body = String::from_utf8_lossy(&chapter_xhtml.2).into_owned();
    // zip 后字节不能判截断（10MB 重复字符 deflate 后仅 ~16KB）——以
    // 解压后正文的字符量为准
    assert!(
        body.matches('a').count() >= 10 * 1024 * 1024,
        "10MB 单行应完整进入章节（解压后 {} 字节）",
        body.len()
    );
    assert!(
        body.contains("<pre"),
        "代码块结构保留：{}",
        &body[..200.min(body.len())]
    );
}

/// 万章 spine：10000 章 md-flat——探测排序、writer 路径分配（ch00001
/// 起 1 基）、nav/spine 构建在万章规模下结构正确。
#[test]
fn ten_thousand_chapter_spine() {
    let mut vfs = MemVfs::new();
    for i in 0..10000u32 {
        vfs = vfs.insert(&format!("ch{i:05}.md"), format!("# 第{i}章\n\n正文 {i}\n"));
    }
    let output = convert_vfs(vfs, ResolvedConfig::default());

    assert_eq!(output.report.stats.chapters, 10000, "万章全部入账");
    let entries = zip_entries(&output);
    assert!(
        entries
            .iter()
            .any(|(name, _, _)| *name == "OEBPS/Text/ch10000.xhtml"),
        "路径分配 1 基递增至末章：{:?}",
        entries
            .iter()
            .map(|(n, _, _)| n.as_str())
            .take(3)
            .collect::<Vec<_>>()
    );
}

/// 千层目录嵌套：探测树及下游（排序 / nav / Drop）是递归实现——修复
/// 前 1000 层直接栈溢出（2026-10 规模测试抓到）；现行口径：目录深度
/// 上限 32，超限文件跳过 + warning，深病态输入得到明确错误而非崩溃。
#[test]
fn thousand_deep_directory_nesting_is_rejected_not_crash() {
    use trans_epub_contract::progress::NoProgress;
    use trans_epub_contract::source::Source;
    use trans_epub_core::{Converter, Io};

    let deep = vec!["d"; 1000].join("/");
    let vfs =
        std::sync::Arc::new(MemVfs::new().insert(&format!("{deep}/doc.md"), "# Deep\n\n正文\n"));
    let io = Io {
        fetcher: std::sync::Arc::new(trans_epub_contract::testing::MockFetcher::new()),
        progress: std::sync::Arc::new(NoProgress),
    };
    let result = Converter::new().convert(&Source::Vfs(vfs), &ResolvedConfig::default(), &io);
    assert!(
        result.is_err(),
        "超深文件被跳过后应报'无匹配源'错误，而不是爆栈或静默出书"
    );

    // 对照：32 层内（上限边界）照常出书
    let ok_deep = vec!["d"; 32].join("/");
    let output = convert_vfs(
        MemVfs::new().insert(&format!("{ok_deep}/doc.md"), "# Deep\n\n正文\n"),
        ResolvedConfig::default(),
    );
    assert_eq!(output.report.stats.chapters, 1, "32 层边界内正常出书");
    let nav = String::from_utf8(
        zip_entries(&output)
            .into_iter()
            .find(|(name, _, _)| *name == "OEBPS/nav.xhtml")
            .map(|(_, _, bytes)| bytes)
            .unwrap_or_default(),
    )
    .unwrap_or_default();
    // 章标题派生自文件名 stem（fm.title > stem > …，见 md 前端）
    assert!(nav.contains("doc"), "边界内深处的章节照常进 nav：{nav}");
}

/// CJK 扩展 B（4 字节码点 U+20000..U+2A6DF）过用户字体 exact 子集：
/// 内置字体不含这些字形，subset 对无 glyph 字符跳过——转换不炸、
/// 字体产物照出（字形覆盖是字体资产的属性，不是管线的失败）。
#[test]
fn cjk_extension_b_survives_user_font_subset() {
    let md = "# 扩展B测试\n\n𠀀𪛖𠀁 regular text\n";
    let font = include_bytes!("../../assets/fonts/TransEpubMono-Regular.subset.ttf").to_vec();
    let mut config = ResolvedConfig::default();
    config.fonts.embed = true;
    config.fonts.subset = FontSubsetMode::Exact;
    config.fonts.source = FontSource::User {
        path: None,
        data: Some(font),
        family: Some("MyMono".to_string()),
    };
    let output = convert_vfs(MemVfs::new().insert("doc.md", md), config);

    assert!(
        zip_entries(&output)
            .iter()
            .any(|(name, _, _)| name.starts_with("OEBPS/Fonts/")),
        "扩展 B 字符不阻塞字体产物"
    );
    assert!(
        output.report.warnings.is_empty(),
        "无字形≠管线 warning：{:?}",
        output.report.warnings
    );
}
