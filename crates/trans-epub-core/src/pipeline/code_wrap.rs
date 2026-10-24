//! code-wrap（链位 4，可选）：构建期硬折行（ADR-0007 的保底模式）。
//!
//! 默认 [`WrapMode::PreWrap`] 时是零操作（折行交给 CSS）；仅在
//! `Hard { width }` 时逐 `pre` 块把超过列宽的行硬切——必须在 highlight
//! 之后（对已染色 span 里的文本节点同样生效，计数跨 span 连续）。
//! 按字符计数（CJK 以一字符近似），换行符重置计数。

use std::sync::OnceLock;

use scraper::Selector;
use trans_epub_contract::config::WrapMode;
use trans_epub_contract::error::BuildError;

use crate::dom::{self, BookDom};
use crate::pipeline::{Pass, PassCtx};

pub struct CodeWrap;

impl Pass for CodeWrap {
    fn name(&self) -> &'static str {
        "code-wrap"
    }

    fn run(&self, book: &mut BookDom, ctx: &mut PassCtx<'_>) -> Result<(), BuildError> {
        let WrapMode::Hard { width } = ctx.config.wrap.mode else {
            return Ok(()); // 默认 pre-wrap：折行交给 CSS，内容零改写
        };
        if width == 0 {
            return Ok(());
        }
        let total = book.chapters.len();
        for (idx, chapter) in book.chapters.iter_mut().enumerate() {
            ctx.check_cancel()?;
            ctx.progress
                .item(idx as u64, total as u64, &chapter.source.to_string());
            let html = chapter.dom_mut();
            for pre_id in dom::select_ids(html, sel_pre()) {
                let mut col: u32 = 0;
                for text_id in dom::text_node_ids(html, pre_id) {
                    let Some(text) = dom::get_text(html, text_id) else {
                        continue;
                    };
                    let wrapped = wrap_chars(&text, width, &mut col);
                    if wrapped != text {
                        dom::set_text(html, text_id, &wrapped);
                    }
                }
            }
        }
        Ok(())
    }
}

fn sel_pre() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("pre"))
}

/// 在已有列位 `col` 的基础上硬折一段文本；返回新文本并更新列位。
fn wrap_chars(text: &str, width: u32, col: &mut u32) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    for ch in text.chars() {
        if ch == '\n' {
            *col = 0;
            out.push(ch);
            continue;
        }
        if *col >= width {
            out.push('\n');
            *col = 0;
        }
        out.push(ch);
        *col += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_width() {
        let mut col = 0;
        assert_eq!(wrap_chars("aaaaaaaaaa", 4, &mut col), "aaaa\naaaa\naa");
        assert_eq!(col, 2);
    }

    #[test]
    fn newline_resets_counter() {
        let mut col = 0;
        assert_eq!(wrap_chars("ab\ncdef", 4, &mut col), "ab\ncdef");
        assert_eq!(col, 4);
        let mut col = 3;
        // 起始列位 3：第 4 个字符触发折行
        assert_eq!(wrap_chars("ab\ncdef", 4, &mut col), "a\nb\ncdef");
    }
}
