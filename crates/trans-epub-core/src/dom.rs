//! BookDom：BookDraft 的 DOM 形态（scraper / html5ever 不出此边界）。
//!
//! pipeline 入口把每章 HTML **解析一次**，各 pass 在 DOM 上做变更，出口
//! **序列化一次**（docs/architecture/20-core.md「DOM 策略」）。序列化是
//! 自实现的 XML 安全输出：void 元素自闭合、文本/属性转义、注释与
//! script/style 一律丢弃——EPUB3 的内容文档是 XHTML，html5ever 自带的
//! HTML 序列化（`<br>` 不自闭合）在这里不可用。

use std::collections::BTreeMap;

use ego_tree::iter::Edge;
use ego_tree::{NodeId, NodeRef};
use html5ever::{LocalName, QualName, ns};
use scraper::node::{Node, Text as TextNode};
use scraper::{Html, Selector, StrTendril};
use trans_epub_contract::error::{BuildError, Warning};
use trans_epub_contract::ir::{
    Asset, BookDraft, BookMeta, Chapter, ChapterId, EmbeddedFont, SourceKey, TocItem,
};

fn text_node(text: &str) -> TextNode {
    TextNode {
        text: StrTendril::from(text),
    }
}

/// XHTML 里必须自闭合的 void 元素（HTML 规范规定它们无子节点）。
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// 序列化时整体丢弃的元素：script 是 EPUB 安全性要求（未声明 scripting
/// 的内容文档禁止脚本）；style/noscript/template 的样式与行为不该来自
/// 源文档——输出格式约束，不是通用 HTML 处理。
const DROPPED_ELEMENTS: &[&str] = &["script", "style", "noscript", "template"];

/// 单章的 DOM 形态。
pub struct ChapterDom {
    pub id: ChapterId,
    pub title: String,
    pub source: SourceKey,
    dom: Html,
}

impl ChapterDom {
    fn parse(ch: &Chapter) -> Self {
        Self {
            id: ch.id,
            title: ch.title.clone(),
            source: ch.source.clone(),
            dom: Html::parse_fragment(&ch.html),
        }
    }

    fn into_chapter(self) -> Chapter {
        Chapter {
            id: self.id,
            title: self.title,
            source: self.source,
            html: serialize_fragment(&self.dom),
        }
    }

    pub fn dom(&self) -> &Html {
        &self.dom
    }

    pub fn dom_mut(&mut self) -> &mut Html {
        &mut self.dom
    }
}

/// BookDraft 的 DOM 形态：chapters 换成 [`ChapterDom`]，其余字段原样。
pub struct BookDom {
    pub meta: BookMeta,
    pub toc: Vec<TocItem>,
    pub assets: BTreeMap<SourceKey, Asset>,
    pub fonts: Vec<EmbeddedFont>,
    pub warnings: Vec<Warning>,
    pub chapters: Vec<ChapterDom>,
}

impl BookDom {
    pub fn from_book(book: BookDraft) -> Self {
        Self {
            meta: book.meta,
            toc: book.toc,
            assets: book.assets,
            fonts: book.fonts,
            warnings: book.warnings,
            chapters: book.chapters.iter().map(ChapterDom::parse).collect(),
        }
    }

    pub fn into_book(self) -> BookDraft {
        BookDraft {
            meta: self.meta,
            toc: self.toc,
            assets: self.assets,
            fonts: self.fonts,
            warnings: self.warnings,
            chapters: self
                .chapters
                .into_iter()
                .map(ChapterDom::into_chapter)
                .collect(),
        }
    }

    pub fn warn(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }
}

// ---------------------------------------------------------------------------
// 选择与读取
// ---------------------------------------------------------------------------

/// 编译一个 CSS 选择器；本模块内所有选择器都是常量，失败即 bug。
pub fn selector(css: &str) -> Selector {
    Selector::parse(css).unwrap_or_else(|e| panic!("invalid constant selector {css:?}: {e:?}"))
}

/// 返回匹配元素的 NodeId 列表（文档序）。先收集再改，避免借用冲突。
pub fn select_ids(html: &Html, selector: &Selector) -> Vec<NodeId> {
    html.select(selector).map(|er| er.id()).collect()
}

/// 元素标签名（小写）；非元素节点返回 `None`。
pub fn tag_name(html: &Html, id: NodeId) -> Option<String> {
    match html.tree.get(id)?.value() {
        Node::Element(el) => Some(el.name.local.to_string()),
        _ => None,
    }
}

/// 读属性（按 local name 匹配，忽略命名空间前缀）。
pub fn get_attr(html: &Html, id: NodeId, name: &str) -> Option<String> {
    match html.tree.get(id)?.value() {
        Node::Element(el) => el
            .attrs
            .iter()
            .find(|(k, _)| &*k.local == name)
            .map(|(_, v)| v.to_string()),
        _ => None,
    }
}

/// 元素是否拥有元素子节点（highlight 用它跳过已染色代码块）。
pub fn has_element_child(html: &Html, id: NodeId) -> bool {
    html.tree
        .get(id)
        .map(|n| n.children().any(|c| matches!(c.value(), Node::Element(_))))
        .unwrap_or(false)
}

/// 子树内全部文本拼接（保留空白）。
pub fn text_content(html: &Html, id: NodeId) -> String {
    let Some(node) = html.tree.get(id) else {
        return String::new();
    };
    let mut out = String::new();
    for edge in node.traverse() {
        if let Edge::Open(n) = edge {
            if let Node::Text(t) = n.value() {
                out.push_str(t);
            }
        }
    }
    out
}

/// 父节点 id（根返回 `None`）。
pub fn parent_id(html: &Html, id: NodeId) -> Option<NodeId> {
    html.tree.get(id)?.parent().map(|p| p.id())
}

/// 子树内全部文本节点的 NodeId（文档序）——code-wrap 的操作对象。
pub fn text_node_ids(html: &Html, id: NodeId) -> Vec<NodeId> {
    let Some(node) = html.tree.get(id) else {
        return Vec::new();
    };
    node.traverse()
        .filter_map(|edge| match edge {
            Edge::Open(n) => match n.value() {
                Node::Text(_) => Some(n.id()),
                _ => None,
            },
            Edge::Close(_) => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 变更
// ---------------------------------------------------------------------------

/// 读文本节点的当前内容；非文本节点返回 `None`。
pub fn get_text(html: &Html, id: NodeId) -> Option<String> {
    match html.tree.get(id)?.value() {
        Node::Text(t) => Some(t.to_string()),
        _ => None,
    }
}

/// 覆写文本节点内容（节点类型不变）。
pub fn set_text(html: &mut Html, id: NodeId, text: &str) {
    if let Some(mut node) = html.tree.get_mut(id) {
        *node.value() = Node::Text(text_node(text));
    }
}

/// 设置属性（存在则覆盖，不存在则追加到末尾——顺序确定）。
pub fn set_attr(html: &mut Html, id: NodeId, name: &str, value: &str) {
    if let Some(mut node) = html.tree.get_mut(id) {
        if let Node::Element(ref mut el) = *node.value() {
            let local = LocalName::from(name);
            if let Some((_, v)) = el.attrs.iter_mut().find(|(k, _)| k.local == local) {
                *v = StrTendril::from(value);
            } else {
                let qn = QualName::new(None, ns!(html), local);
                el.attrs.insert(qn, StrTendril::from(value));
            }
        }
    }
}

/// 移除属性（不存在则无操作）。
pub fn remove_attr(html: &mut Html, id: NodeId, name: &str) {
    if let Some(mut node) = html.tree.get_mut(id) {
        if let Node::Element(ref mut el) = *node.value() {
            let local = LocalName::from(name);
            if let Some(pos) = el.attrs.iter().position(|(k, _)| k.local == local) {
                el.attrs.shift_remove_index(pos);
            }
        }
    }
}

/// 删除元素（连同子树）。
pub fn remove_element(html: &mut Html, id: NodeId) {
    if let Some(mut node) = html.tree.get_mut(id) {
        node.detach();
    }
}

/// 把元素替换为一个文本节点（图片降级占位用）。
pub fn replace_with_text(html: &mut Html, id: NodeId, text: &str) {
    if let Some(mut node) = html.tree.get_mut(id) {
        node.insert_after(Node::Text(text_node(text)));
        node.detach();
    }
}

/// 去掉元素标签、保留子节点（内部链接转纯文本用）。
pub fn unwrap_element(html: &mut Html, id: NodeId) {
    let child_ids = child_ids(html, id);
    for cid in child_ids {
        if let Some(mut node) = html.tree.get_mut(id) {
            node.insert_id_before(cid);
        }
    }
    if let Some(mut node) = html.tree.get_mut(id) {
        node.detach();
    }
}

/// 用 HTML 字符串整体替换元素内容（highlight 染色用）。
///
/// 实现：清空子节点 → 解析片段树 → `append_subtree` 嫁接到目标下 →
/// 下钻过片段的 `<html>` 外壳、把真实内容的子节点上提到目标，丢弃外壳。
/// 全部是同树操作。
pub fn set_inner_html(html: &mut Html, id: NodeId, inner: &str) {
    for cid in child_ids(html, id) {
        if let Some(mut c) = html.tree.get_mut(cid) {
            c.detach();
        }
    }
    let frag = Html::parse_fragment(inner);
    let grafted = match html.tree.get_mut(id) {
        Some(mut node) => node.append_subtree(frag.tree).id(),
        None => return,
    };
    // 嫁接进来的根是 Fragment，其下是 <html> 外壳，再下才是内容
    let content_parent = html
        .tree
        .get(grafted)
        .map(descend_html_shell)
        .map(|n| n.id())
        .unwrap_or(grafted);
    for gid in child_ids(html, content_parent) {
        if let Some(mut node) = html.tree.get_mut(id) {
            node.append_id(gid);
        }
    }
    for husk in [content_parent, grafted] {
        if husk != id {
            if let Some(mut h) = html.tree.get_mut(husk) {
                h.detach();
            }
        }
    }
}

fn child_ids(html: &Html, id: NodeId) -> Vec<NodeId> {
    html.tree
        .get(id)
        .map(|n| n.children().map(|c| c.id()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// XML 安全序列化
// ---------------------------------------------------------------------------

/// 序列化片段：html5ever 片段解析会给内容套 `<html>` 外壳，这里输出
/// 外壳之下的内容。
pub fn serialize_fragment(html: &Html) -> String {
    let mut out = String::new();
    if let Some(root) = html.tree.get(html.tree.root().id()) {
        let start = descend_html_shell(root);
        for child in start.children() {
            serialize_node(child, &mut out);
        }
    }
    out
}

/// 序列化某元素的全部子节点（web 前端抽取正文根用；与片段序列化同一
/// XML 安全规则——script/style/注释在这一步就被剔除，"干净 HTML"义务
/// 从 load 起算）。
pub(crate) fn serialize_children(html: &Html, id: NodeId) -> String {
    let Some(node) = html.tree.get(id) else {
        return String::new();
    };
    let mut out = String::new();
    for child in node.children() {
        serialize_node(child, &mut out);
    }
    out
}

/// 从 `node`（Fragment/Document 根）下钻过 `<html>`（及可能的
/// `<body>`）外壳，返回承载真实内容的节点。
fn descend_html_shell(node: NodeRef<'_, Node>) -> NodeRef<'_, Node> {
    let mut cur = node;
    while let Some(only) = single_child(cur) {
        match only.value() {
            Node::Element(el) if matches!(el.name.local.as_ref(), "html" | "body") => cur = only,
            _ => break,
        }
    }
    cur
}

fn single_child(node: NodeRef<'_, Node>) -> Option<NodeRef<'_, Node>> {
    let mut iter = node.children();
    let first = iter.next()?;
    iter.next().is_none().then_some(first)
}

fn serialize_node(node: NodeRef<'_, Node>, out: &mut String) {
    match node.value() {
        Node::Text(t) => out.push_str(&escape_text(t)),
        Node::Element(el) => {
            let name = el.name.local.as_ref();
            if DROPPED_ELEMENTS.contains(&name) {
                return;
            }
            out.push('<');
            write_qual_name(out, &el.name);
            for (k, v) in el.attrs.iter() {
                out.push(' ');
                write_qual_name(out, k);
                out.push_str("=\"");
                out.push_str(&escape_attr(v));
                out.push('"');
            }
            if VOID_ELEMENTS.contains(&name) {
                // html5ever 保证 void 元素无子节点
                out.push_str(" />");
            } else {
                out.push('>');
                for child in node.children() {
                    serialize_node(child, out);
                }
                out.push_str("</");
                out.push_str(name);
                out.push('>');
            }
        }
        // 注释 / doctype / PI 不进 XHTML 产物；Document/Fragment 不会被走到
        _ => {}
    }
}

fn write_qual_name(out: &mut String, q: &QualName) {
    if let Some(prefix) = &q.prefix {
        out.push_str(prefix);
        out.push(':');
    }
    out.push_str(&q.local);
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

/// [`BuildError::Fatal`] 便捷构造（pass 内部不可恢复错误）。
pub fn fatal(code: &'static str, message: impl Into<String>) -> BuildError {
    BuildError::Fatal {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trans_epub_contract::source::RelPath;

    fn path_key(s: &str) -> SourceKey {
        SourceKey::Path(RelPath::new(s).unwrap())
    }

    #[test]
    fn serializes_xml_safe() {
        let html = Html::parse_fragment(
            "<p>a &amp; b <span data-x=\"1\">c</span><br>done</p><!-- gone --><script>bad()</script>",
        );
        let out = serialize_fragment(&html);
        assert_eq!(
            out,
            r#"<p>a &amp; b <span data-x="1">c</span><br />done</p>"#
        );
        assert!(!out.contains("script") && !out.contains("gone"));
    }

    #[test]
    fn set_inner_html_replaces_content() {
        let mut html =
            Html::parse_fragment("<pre><code class=\"language-rust\">fn main() {}</code></pre>");
        let code_id = select_ids(&html, &selector("pre > code"))[0];
        set_inner_html(
            &mut html,
            code_id,
            "<span class=\"syn-keyword\">fn</span> main",
        );
        let out = serialize_fragment(&html);
        assert_eq!(
            out,
            r#"<pre><code class="language-rust"><span class="syn-keyword">fn</span> main</code></pre>"#
        );
    }

    #[test]
    fn unwrap_and_replace() {
        let mut html =
            Html::parse_fragment("<p>see <a href=\"x.md\">chapter <em>x</em></a> now</p>");
        let a_id = select_ids(&html, &selector("a"))[0];
        unwrap_element(&mut html, a_id);
        assert_eq!(
            serialize_fragment(&html),
            "<p>see chapter <em>x</em> now</p>"
        );

        let mut html = Html::parse_fragment("<p><img src=\"a.png\" alt=\"pic\" />x</p>");
        let img_id = select_ids(&html, &selector("img"))[0];
        replace_with_text(&mut html, img_id, "[missing image: pic]");
        assert_eq!(serialize_fragment(&html), "<p>[missing image: pic]x</p>");
    }

    #[test]
    fn attr_mutation_roundtrip() {
        let mut html = Html::parse_fragment("<a href=\"old\">t</a>");
        let a_id = select_ids(&html, &selector("a"))[0];
        set_attr(&mut html, a_id, "href", "ch00002.xhtml");
        set_attr(&mut html, a_id, "title", "ch2");
        assert_eq!(get_attr(&html, a_id, "href").unwrap(), "ch00002.xhtml");
        assert_eq!(get_attr(&html, a_id, "title").unwrap(), "ch2");
        remove_attr(&mut html, a_id, "title");
        assert_eq!(get_attr(&html, a_id, "title"), None);
        assert_eq!(serialize_fragment(&html), "<a href=\"ch00002.xhtml\">t</a>");
    }

    #[test]
    fn book_dom_roundtrip_keeps_chapters() {
        let mut book = BookDraft::default();
        book.push_chapter("Ch", path_key("a.md"), "<p>hello &amp; <b>world</b></p>");
        let dom = BookDom::from_book(book);
        let book = dom.into_book();
        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.chapters[0].html, "<p>hello &amp; <b>world</b></p>");
        assert_eq!(book.chapters[0].title, "Ch");
    }
}
