//! golden 端到端测试：fixtures → Converter → EPUB。
//!
//! 三个层次（docs/architecture/20-core.md 测试策略）：
//! 1. 结构断言（zip 条目、nav/spine、链接改写、高亮、降级、warning）——
//!    失败时可读的因果；
//! 2. 确定性：同一输入连转两次字节一致；
//! 3. golden 字节比对：`tests/golden/{name}.epub`；更新用
//!    `UPDATE_GOLDEN=1 cargo test -p trans-epub-core --test golden`。

use std::collections::BTreeSet;
use std::io::Read;
use std::sync::Arc;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::progress::NoProgress;
use trans_epub_contract::source::Source;
use trans_epub_contract::testing::{MemVfs, MockFetcher};
use trans_epub_contract::writer::BuildOutput;
use trans_epub_core::{Converter, Io};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");

const REMOTE_PNG_URL: &str = "https://example.com/remote.png";

fn png_bytes() -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}/flat/img/diagram.png")).expect("fixture png exists")
}

/// 每本样书的配置（golden 字节比对与验收门共用同一口径）。
fn fixture_config(name: &str) -> ResolvedConfig {
    let mut config = ResolvedConfig::default();
    if name == "tree" {
        config.book.cover = Some("cover.png".to_string());
    }
    config
}

fn convert(name: &str) -> BuildOutput {
    convert_with(name, fixture_config(name))
}

fn convert_with(name: &str, config: ResolvedConfig) -> BuildOutput {
    let vfs = MemVfs::from_dir(format!("{FIXTURES}/{name}")).expect("fixture dir readable");
    let fetcher = MockFetcher::new().insert(REMOTE_PNG_URL, png_bytes(), "image/png");
    let io = Io {
        fetcher: Arc::new(fetcher),
        progress: Arc::new(NoProgress),
    };
    Converter::new()
        .convert(&Source::Vfs(Arc::new(vfs)), &config, &io)
        .expect("conversion succeeds")
}

fn zip_entries(output: &BuildOutput) -> Vec<(String, bool, Vec<u8>)> {
    let reader = std::io::Cursor::new(&output.bytes);
    let mut archive = zip::ZipArchive::new(reader).expect("valid epub zip");
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).expect("index in range");
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).expect("entry readable");
        let stored = file.compression() == zip::CompressionMethod::Stored;
        out.push((file.name().to_string(), stored, bytes));
    }
    out
}

fn entry_bytes(output: &BuildOutput, name: &str) -> Vec<u8> {
    zip_entries(output)
        .into_iter()
        .find(|(n, _, _)| n == name)
        .unwrap_or_else(|| panic!("missing zip entry {name}"))
        .2
}

fn utf8(name: &str, output: &BuildOutput) -> String {
    String::from_utf8(entry_bytes(output, name)).expect("utf-8 entry")
}

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
    assert!(ch3.contains("footnotes"), "脚注 section");

    // 元数据：根 README 的 front matter title → 书名
    let opf = utf8("OEBPS/content.opf", &output);
    assert!(opf.contains("<dc:title>Sample Book</dc:title>"));
    assert!(opf.contains("urn:uuid:"));
    assert!(opf.contains("2000-01-01T00:00:00Z"));

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

    // appendix：缺失 include / 未知指令 → 原文保留（代码块内的指令
    // 文本被高亮 span 切开，断言取未切开的子串）
    let appendix = utf8("OEBPS/Text/ch00006.xhtml", &output);
    assert!(appendix.contains("missing.rs"), "{appendix}");
    assert!(appendix.contains("{{#if feature}}"));

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

#[test]
fn deterministic_output() {
    let a = convert("flat");
    let b = convert("flat");
    assert_eq!(a.bytes, b.bytes, "同输入必须字节级一致");
}

#[test]
fn golden_bytes() {
    for name in ["flat", "tree", "single", "mdbook"] {
        let output = convert(name);
        let golden_path = format!("{GOLDEN}/{name}.epub");
        if std::env::var("UPDATE_GOLDEN").is_ok() {
            std::fs::write(&golden_path, &output.bytes).expect("write golden");
            continue;
        }
        let golden = std::fs::read(&golden_path)
            .unwrap_or_else(|_| panic!("golden {name}.epub 缺失；先运行 UPDATE_GOLDEN=1 生成"));
        assert_eq!(
            output.bytes, golden,
            "{name} 与 golden 不一致（意图变更请 UPDATE_GOLDEN=1 重新生成并 review diff）"
        );
    }
}

/// epubcheck 验收门：jar 存在（`EPUBCHECK_JAR` 或 scripts/epubcheck.sh 的
/// 默认下载位置）时对**新鲜转换**的产物跑校验；不存在则跳过（本地无
/// Java 环境的场合由 scripts/epubcheck.sh 兜底）。
#[test]
fn epubcheck_gate() {
    let jar = std::env::var("EPUBCHECK_JAR")
        .unwrap_or_else(|_| "target/epubcheck/epubcheck-5.2.1/epubcheck.jar".to_string());
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
}

fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("trans-epub-epubcheck-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
