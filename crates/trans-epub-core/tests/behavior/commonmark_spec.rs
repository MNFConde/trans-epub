//! CommonMark 0.31.2 官方 spec 语料批测（652 例，spec.json 单一事实源
//! 入库 fixtures）：以独立外部语料背书 md 前端的解析保真——
//!
//! - 每例走 md-file 形态 load（probe 探测 + comrak 解析 + 章节序列化），
//!   断言不 panic、不失败；
//! - 结构保真锚点：参考输出含某语义元素 ⇒ 我们的章节 HTML 含同一元素
//!   （同构 tag 白名单；raw HTML 透传物与 `<br>` 类排版细节不锚——
//!   它们的处置是产品决策而非解析缺陷）。

use std::sync::Arc;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::frontend::{Frontend, LoadEnv};
use trans_epub_contract::progress::NoProgress;
use trans_epub_contract::source::Source;
use trans_epub_contract::testing::MemVfs;
use trans_epub_core::frontends::md::MdFrontend;

#[derive(serde::Deserialize)]
struct SpecCase {
    markdown: String,
    #[allow(dead_code)]
    html: String,
    #[allow(dead_code)]
    section: String,
    example: u32,
}

const SPEC: &str = include_str!("../fixtures/commonmark-spec-0.31.2.json");

/// 参考输出与产出之间要求同构出现的元素（CommonMark 语义元素；raw HTML
/// 透传物 div/table 等与 br/hr 排版细节不在此列）。
const ISOMORPHIC_TAGS: &[&str] = &[
    "em",
    "strong",
    "code",
    "pre",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "ul",
    "ol",
    "li",
    "a",
    "img",
    "p",
];

/// DOM 解析提取元素名集合——字符串匹配会把属性值里的字面 `<em>` 误判
/// 成元素（Raw HTML 例的参考输出就是这么序列化的）。
fn element_tags(html: &str) -> std::collections::HashSet<&'static str> {
    let doc = scraper::Html::parse_fragment(html);
    let mut tags = std::collections::HashSet::new();
    for node in doc.tree.nodes() {
        if let Some(el) = node.value().as_element() {
            if let Some(t) = ISOMORPHIC_TAGS.iter().find(|t| **t == el.name()) {
                tags.insert(*t);
            }
        }
    }
    tags
}

#[test]
fn commonmark_spec_examples_parse_with_fidelity() {
    let cases: Vec<SpecCase> = serde_json::from_str(SPEC).expect("spec.json 解析");
    assert_eq!(
        cases.len(),
        652,
        "0.31.2 语料规模锚点（升级版本须同步本数）"
    );

    let frontend = MdFrontend;
    let config = ResolvedConfig::default();
    let progress = NoProgress;

    for case in &cases {
        let vfs = Arc::new(MemVfs::new().insert("doc.md", case.markdown.as_bytes().to_vec()));
        let draft = match frontend.load(
            &Source::Vfs(vfs),
            &LoadEnv {
                config: &config,
                progress: &progress,
            },
        ) {
            Ok(draft) => draft,
            Err(err) => panic!(
                "[{} #{}] load 失败：{err}\nmarkdown: {:?}",
                case.section, case.example, case.markdown
            ),
        };
        let produced: String = draft.chapters.iter().map(|c| c.html.as_str()).collect();
        if produced.is_empty() && case.markdown.trim().is_empty() {
            continue; // 空文档例无结构可锚
        }
        let produced_tags = element_tags(&produced);
        for tag in element_tags(&case.html) {
            assert!(
                produced_tags.contains(tag),
                "[{} #{}] 参考输出含 <{tag}> 而产出缺失\nmarkdown: {:?}\nproduced: {}",
                case.section,
                case.example,
                case.markdown,
                &produced[..produced.len().min(500)]
            );
        }
    }
}
