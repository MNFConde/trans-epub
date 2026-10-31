//! 脚注语义化（10-contracts.md「EPUB 输出增强三件」之一）：comrak 0.55 的
//! 确定性脚注形态 → EPUB3 结构语义（noteref / footnote / backlink），解锁
//! Kobo/iBooks 弹窗脚注。
//!
//! 字符串级识别、不做 HTML 重序列化：只改写命中形态的片段，其余字节原样；
//! 形态不匹配整段原样通过——writer 无容忍轨出口，宁缺勿错。comrak 的
//! 输出形态固定（class/属性序不随内容变化），识别面据此收敛：
//!
//! ```text
//! 引用：<sup class="footnote-ref"><a href="#fn-1" id="fnref-1" data-footnote-ref="">1</a></sup>
//! 体  ：<section class="footnotes" data-footnotes=""><ol>
//!        <li id="fn-1"> <p>… <a href="#fnref-1" class="footnote-backref" …>↩</a></p> </li>
//!       </ol></section>
//! ```

/// 章节正文 HTML 的脚注语义化入口。
pub(crate) fn semanticize(html: &str) -> String {
    let out = footnotes_section_to_asides(html);
    let out = annotate_noterefs(&out);
    annotate_backrefs(&out)
}

/// comrak 脚注引用：`<sup class="footnote-ref"><a ` 之后补语义属性。
fn annotate_noterefs(html: &str) -> String {
    const NEEDLE: &str = "<sup class=\"footnote-ref\"><a ";
    annotate_tag_after(html, NEEDLE, "epub:type=\"noteref\" role=\"doc-noteref\" ")
}

/// comrak 脚注回链：`<a href="#fnref-…` 且同标签带 `footnote-backref`
/// 才补语义（作者手写的 `[x](#fnref-1)` 无该 class，不误标）。
fn annotate_backrefs(html: &str) -> String {
    const NEEDLE: &str = "<a href=\"#fnref-";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find(NEEDLE) {
        let Some(tag_end_rel) = rest[pos..].find('>') else {
            break;
        };
        let tag_end = pos + tag_end_rel;
        out.push_str(&rest[..pos]);
        if rest[pos..tag_end].contains("class=\"footnote-backref\"") {
            out.push_str("<a epub:type=\"backlink\" role=\"doc-backlink\"");
            out.push_str(&rest[pos + "<a".len()..tag_end]);
            out.push('>');
        } else {
            out.push_str(&rest[pos..=tag_end]);
        }
        rest = &rest[tag_end + 1..];
    }
    out.push_str(rest);
    out
}

/// 在每个 `NEEDLE` 命中点的 `<a ` 之后插入 `attrs`（comrak 形态固定，
/// 属性插入位确定 → 输出确定）。
fn annotate_tag_after(html: &str, needle: &str, attrs: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(pos) = rest.find(needle) {
        let after_a = pos + needle.len();
        out.push_str(&rest[..after_a]);
        out.push_str(attrs);
        rest = &rest[after_a..];
    }
    out.push_str(rest);
    out
}

/// `<section class="footnotes" …>` → 逐 `<li>` 变 `<aside epub:type="footnote">`
/// 平铺（section/ol 包裹层摘除，li 的 id 原样保留）。任何形态出入
/// （无 id、未闭合、嵌套异常）整段放弃改写、原样返回。
fn footnotes_section_to_asides(html: &str) -> String {
    const OPEN: &str = "<section class=\"footnotes\"";
    const CLOSE: &str = "</section>";

    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(OPEN) {
        let Some(close_rel) = rest[start..].find(CLOSE) else {
            break;
        };
        let end = start + close_rel + CLOSE.len();
        let inner = &rest[start + OPEN.len()..start + close_rel];

        let Some(asides) = top_level_lis_to_asides(inner) else {
            // 形态不匹配：原样交回这一段，继续找后续 section
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        };

        out.push_str(&rest[..start]);
        out.push_str(&asides);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// section 内层 → aside 串。只认顶层 `<li id="fn…">`（comrak 形态）；
/// 脚注体内嵌套列表随内容保留为 li（深度配对扫描）。
fn top_level_lis_to_asides(inner: &str) -> Option<String> {
    let mut asides = String::new();
    let mut rest = inner;
    let mut found = false;
    while let Some(pos) = find_top_level_li(rest) {
        let after_tag = pos + rest[pos..].find('>')? + 1;
        let open_tag = &rest[pos..after_tag];
        let id = li_id(open_tag)?;
        let body_end = match_li_close(rest, after_tag)?;
        asides.push_str(&format!(
            "<aside class=\"footnote footnote-item\" epub:type=\"footnote\" id=\"{id}\" role=\"doc-footnote\">{}",
            &rest[after_tag..body_end]
        ));
        asides.push_str("</aside>\n");
        rest = &rest[body_end + "</li>".len()..];
        found = true;
    }
    found.then_some(asides)
}

/// 下一个顶层 `<li` 的位置：跳过脚注体内嵌套的 li（深度配对）。
/// `rest` 总是从顶层语境起扫（上一个顶层 li 已整体消费），首个合法
/// `<li` 即顶层；深度机制只对破损形态兜底。
fn find_top_level_li(rest: &str) -> Option<usize> {
    let mut cursor = 0;
    let mut depth = 0i32;
    while cursor < rest.len() {
        let open = rest[cursor..].find("<li").map(|i| cursor + i);
        let close = rest[cursor..].find("</li>").map(|i| cursor + i);
        let take_open = match (open, close) {
            (Some(op), Some(cv)) => op < cv,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return None,
        };
        if take_open {
            let pos = open.expect("checked");
            // `<li` 后须是空白或 `>`（防 <link 之类误配）
            let tail = &rest[pos + 3..];
            let well_formed = tail.starts_with('>') || tail.starts_with(char::is_whitespace);
            if depth == 0 && well_formed {
                return Some(pos);
            }
            if well_formed {
                depth += 1;
            }
            cursor = pos + 3;
        } else {
            let cv = close.expect("checked");
            depth -= 1;
            cursor = cv + "</li>".len();
            if depth < 0 {
                return None; // 破损形态
            }
        }
    }
    None
}

/// 从 `rest[from..]` 找与开标签配对的 `</li>`，返回其起始位置；
/// 未闭合返回 None。
fn match_li_close(rest: &str, from: usize) -> Option<usize> {
    let mut cursor = from;
    let mut depth = 1i32;
    while cursor < rest.len() {
        let open = rest[cursor..].find("<li").map(|i| cursor + i);
        let close = rest[cursor..].find("</li>").map(|i| cursor + i);
        let take_open = match (open, close) {
            (Some(op), Some(cv)) => op < cv,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return None, // 未闭合
        };
        if take_open {
            let pos = open.expect("checked");
            let tail = &rest[pos + 3..];
            if tail.starts_with('>') || tail.starts_with(char::is_whitespace) {
                depth += 1;
            }
            cursor = pos + 3;
        } else {
            let cv = close.expect("checked");
            depth -= 1;
            if depth == 0 {
                return Some(cv);
            }
            cursor = cv + "</li>".len();
        }
    }
    None
}

/// 开标签里的 `id="…"` 值。
fn li_id(open_tag: &str) -> Option<&str> {
    let start = open_tag.find(" id=\"")? + " id=\"".len();
    let rest = &open_tag[start..];
    let end = rest.find('"')?;
    let id = &rest[..end];
    (!id.is_empty()).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// comrak 0.55 实际输出形态（取自 golden flat 样书）。
    const COMRAK_REF: &str = "<sup class=\"footnote-ref\"><a href=\"#fn-1\" id=\"fnref-1\" data-footnote-ref=\"\">1</a></sup>";
    const COMRAK_SECTION: &str = "<section class=\"footnotes\" data-footnotes=\"\">\n<ol>\n<li id=\"fn-1\">\n<p>the footnote body. <a href=\"#fnref-1\" class=\"footnote-backref\" data-footnote-backref=\"\" data-footnote-backref-idx=\"1\" aria-label=\"Back to reference 1\">\u{21a9}</a></p>\n</li>\n</ol>\n</section>";

    #[test]
    fn noteref_gets_epub_semantics() {
        let out = semanticize(COMRAK_REF);
        assert!(
            out.contains(
                "<sup class=\"footnote-ref\"><a epub:type=\"noteref\" role=\"doc-noteref\" href=\"#fn-1\""
            ),
            "{out}"
        );
        // sup 内 href="#fn-" 不是回链形态，不得标 backlink
        assert!(!out.contains("backlink"), "{out}");
    }

    #[test]
    fn section_becomes_flat_asides_with_semantics() {
        let out = semanticize(COMRAK_SECTION);
        assert!(
            out.starts_with(
                "<aside class=\"footnote footnote-item\" epub:type=\"footnote\" id=\"fn-1\" role=\"doc-footnote\">"
            ),
            "{out}"
        );
        assert!(out.ends_with("</aside>\n"), "{out}");
        assert!(!out.contains("<section"), "包裹层摘除：{out}");
        assert!(!out.contains("<ol>"), "{out}");
        assert!(!out.contains("<li"), "{out}");
        assert!(
            out.contains(
                "<a epub:type=\"backlink\" role=\"doc-backlink\" href=\"#fnref-1\" class=\"footnote-backref\""
            ),
            "回链补语义、原属性保留：{out}"
        );
        assert!(out.contains("aria-label=\"Back to reference 1\""), "{out}");
    }

    #[test]
    fn ref_and_section_transform_together_in_document() {
        let doc = format!("<p>x{COMRAK_REF}</p>\n{COMRAK_SECTION}");
        let out = semanticize(&doc);
        assert!(out.contains("epub:type=\"noteref\""), "{out}");
        assert!(out.contains("epub:type=\"footnote\" id=\"fn-1\""), "{out}");
        assert!(out.contains("epub:type=\"backlink\""), "{out}");
    }

    #[test]
    fn multiple_footnotes_keep_order_and_ids() {
        let li = |n: u32| {
            format!(
                "<li id=\"fn-{n}\">\n<p>note {n}. <a href=\"#fnref-{n}\" class=\"footnote-backref\" data-footnote-backref=\"\" aria-label=\"Back to reference {n}\">\u{21a9}</a></p>\n</li>"
            )
        };
        let section = format!(
            "<section class=\"footnotes\" data-footnotes=\"\">\n<ol>\n{}\n{}\n</ol>\n</section>",
            li(1),
            li(2)
        );
        let out = semanticize(&section);
        let a = out.find("id=\"fn-1\"").expect("fn-1");
        let b = out.find("id=\"fn-2\"").expect("fn-2");
        assert!(a < b, "顺序保持：{out}");
        assert_eq!(out.matches("epub:type=\"footnote\"").count(), 2, "{out}");
    }

    #[test]
    fn nested_list_inside_footnote_body_survives() {
        let section = "<section class=\"footnotes\" data-footnotes=\"\">\n<ol>\n<li id=\"fn-1\">\n<p>outer</p>\n<ol>\n<li>inner item</li>\n</ol>\n<p>tail <a href=\"#fnref-1\" class=\"footnote-backref\" aria-label=\"Back to reference 1\">\u{21a9}</a></p>\n</li>\n</ol>\n</section>";
        let out = semanticize(section);
        assert!(out.contains("epub:type=\"footnote\" id=\"fn-1\""), "{out}");
        assert!(
            out.contains("<ol>\n<li>inner item</li>\n</ol>"),
            "嵌套 li 原样保留：{out}"
        );
        assert!(
            out.contains("epub:type=\"backlink\""),
            "顶层回链仍标注：{out}"
        );
        assert_eq!(out.matches("<aside").count(), 1, "{out}");
    }

    #[test]
    fn plain_html_passthrough_byte_identical() {
        let html = "<h1 id=\"a\">T</h1>\n<p>hello <a href=\"x.xhtml\">link</a></p>\n";
        assert_eq!(semanticize(html), html);
    }

    #[test]
    fn foreign_footnote_shape_left_untouched() {
        // li 无 id（非 comrak 形态）→ 整段原样
        let no_id = "<section class=\"footnotes\" data-footnotes=\"\">\n<ol>\n<li><p>x</p></li>\n</ol>\n</section>";
        assert_eq!(semanticize(no_id), no_id);
        // 未闭合 → 原样
        let unclosed = "<section class=\"footnotes\" data-footnotes=\"\">\n<ol>\n<li id=\"fn-1\">\n<p>x</p>\n</ol>\n</section>";
        assert_eq!(semanticize(unclosed), unclosed);
    }

    #[test]
    fn author_link_to_fnref_not_misannotated() {
        let html = "<p><a href=\"#fnref-1\">back</a></p>";
        assert_eq!(semanticize(html), html);
    }
}
