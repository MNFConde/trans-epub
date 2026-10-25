//! highlight（链位 3）：syntect 构建期染色（ADR-0006）。
//!
//! 只处理「未染色」的 `pre > code.language-X`（已有元素子节点视为外部
//! 已染色，跳过——web 前端的硬要求）。输出为 classed span
//!（`class="syn-source syn-rust …"`，前缀 `syn-` 防止与书内 CSS 冲突），
//! 配色 CSS 由 [`theme_css`] 按主题名生成，writer 的 code.css 内嵌。
//!
//! 语法集来自 syntect-assets 的编译期扩展 dump（bat 同源，比 syntect
//! 默认集多 TypeScript/TSX/JSX/TOML 等语言），主题集仍是 syntect 默认；
//! 两者的二进制 dump 加载昂贵，进程内 `OnceLock` 只加载一次；
//! 加载的是编译期内嵌 dump，同版本字节确定 → 输出确定。

use std::sync::OnceLock;

use syntect::highlighting::{Theme, ThemeSet};
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use trans_epub_contract::error::{BuildError, Warning, WarningCode};

use crate::dom::{self, BookDom};
use crate::pipeline::{Pass, PassCtx};

/// class 前缀：所有 syntect 输出的 class 都带它（含基础 `syn-code`）。
pub const CLASS_PREFIX: &str = "syn-";

pub struct Highlight;

impl Pass for Highlight {
    fn name(&self) -> &'static str {
        "highlight"
    }

    fn run(&self, book: &mut BookDom, ctx: &mut PassCtx<'_>) -> Result<(), BuildError> {
        let total = book.chapters.len();
        let BookDom {
            chapters, warnings, ..
        } = book;
        for (idx, chapter) in chapters.iter_mut().enumerate() {
            ctx.check_cancel()?;
            ctx.progress
                .item(idx as u64, total as u64, &chapter.source.to_string());
            let chapter_source = chapter.source.clone();
            let html = chapter.dom_mut();

            for code_id in dom::select_ids(html, sel_code()) {
                if dom::has_element_child(html, code_id) {
                    continue; // 已染色（外部 HTML）
                }
                let Some(class) = dom::get_attr(html, code_id, "class") else {
                    continue;
                };
                let Some(lang) = class
                    .split_whitespace()
                    .find_map(|c| c.strip_prefix("language-"))
                    .filter(|l| !l.is_empty())
                else {
                    continue; // 无语言标注：纯文本代码块，保持原样
                };
                let code = dom::text_content(html, code_id);
                match highlight_code(&code, lang) {
                    Some(highlighted) => {
                        dom::set_inner_html(html, code_id, &highlighted);
                        // pre 挂 syn-code：syntect 生成 CSS 的基础配色选择器
                        if let Some(pre_id) = dom::parent_id(html, code_id) {
                            let existing = dom::get_attr(html, pre_id, "class").unwrap_or_default();
                            if !existing.split_whitespace().any(|c| c == "syn-code") {
                                let joined = if existing.is_empty() {
                                    format!("{CLASS_PREFIX}code")
                                } else {
                                    format!("{existing} {CLASS_PREFIX}code")
                                };
                                dom::set_attr(html, pre_id, "class", &joined);
                            }
                        }
                    }
                    None => {
                        if !ctx.config.highlight.unknown_language_fallback {
                            warnings.push(Warning::new(
                                WarningCode::HighlightSkipped,
                                Some(chapter_source.clone()),
                                format!("unknown code language: {lang}"),
                            ));
                        }
                        // fallback：保留纯文本
                    }
                }
            }
        }
        Ok(())
    }
}

fn sel_code() -> &'static scraper::Selector {
    static S: OnceLock<scraper::Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("pre > code"))
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        syntect_assets::assets::HighlightingAssets::from_binary()
            .get_syntax_set()
            .expect("crate 内嵌 dump 反序列化不会失败")
            .clone()
    })
}

fn theme_set() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

fn class_style() -> ClassStyle {
    ClassStyle::SpacedPrefixed {
        prefix: CLASS_PREFIX,
    }
}

/// 按主题名取 syntect 内置主题；未知返回 `None`。
pub fn theme_by_name(name: &str) -> Option<&'static Theme> {
    theme_set().themes.get(name)
}

/// 生成主题的 classed CSS（`.syn-code {…}` + 各 scope 选择器）。
/// 同一 syntect 版本输出确定；未知主题返回 `None`。
pub fn theme_css(name: &str) -> Option<String> {
    let theme = theme_by_name(name)?;
    syntect::html::css_for_theme_with_class_style(theme, class_style()).ok()
}

/// 高亮一段代码为 classed HTML。语言未知返回 `None`。
pub fn highlight_code(code: &str, lang: &str) -> Option<String> {
    let ss = syntax_set();
    let syntax = ss.find_syntax_by_token(lang)?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, ss, class_style());
    for line in LinesWithEndings::from(code) {
        generator
            .parse_html_for_line_which_includes_newline(line)
            .ok()?;
    }
    Some(generator.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust() {
        let html = highlight_code("fn main() {}\n", "rust").unwrap();
        assert!(
            html.contains("syn-source syn-rust"),
            "expected classed spans in: {html}"
        );
        assert!(html.contains("fn"));
    }

    #[test]
    fn highlights_typescript() {
        let html = highlight_code(
            "const x: number = 1;\ninterface Foo { bar: string }\n",
            "ts",
        )
        .unwrap();
        assert!(
            html.contains("syn-source syn-ts"),
            "expected classed spans in: {html}"
        );
        assert!(html.contains("interface"));
    }

    #[test]
    fn unknown_language_is_none() {
        assert!(highlight_code("x", "no-such-lang-xyz").is_none());
    }

    #[test]
    fn default_theme_css_available() {
        assert!(theme_css("InspiredGitHub").unwrap().contains(".syn-code"));
        assert!(
            theme_css("base16-ocean.dark")
                .unwrap()
                .contains(".syn-code")
        );
        assert!(theme_css("no-such-theme").is_none());
    }
}
