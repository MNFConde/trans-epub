//! link-rewrite（链位 2）：把章节/资产引用改写为 EPUB 内部路径。
//!
//! 前置：asset-resolve 已定资产终态（Missing 的引用在此降级）；
//! 章节集合已冻结（load 之后不变）。改写规则：
//!
//! - `a[href]`：纯锚点原样校验；引用解析按世界分轨（[`resolve_key`]）——
//!   Path 世界带 scheme 一律外部保留；Url 世界（Pages 源）同源绝对链接
//!   也是书内候选。命中章节 → `chNNNNN.xhtml#锚点`，命中资产 → 资产
//!   路径；未解析 → Path 世界降级为纯文本 + `link-unresolved` warning，
//!   Url 世界绝对化保留（站外仍可点）+ 同码 warning。
//! - `img[src]` / `source[src]`：命中就绪资产 → 改写；否则整体替换为
//!   alt 文本占位（EPUB 不抓远程资源，悬空 src 过不了 epubcheck）。
//!   `img` 无 alt 时补 `alt=""`（epubcheck 无障碍要求）。
//!
//! 路径规则来自 [`crate::writer::paths`]（与 writer 同源，改命名只动一处）。
//! 绝对/`data:` URL 与前端登记口径一致：两侧都 `Url::parse` / `Url::join`
//! 同一字符串，url crate 的规范化即去重。

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use comrak::Anchorizer;
use scraper::Selector;
use trans_epub_contract::error::{BuildError, Warning, WarningCode};
use trans_epub_contract::ir::{ChapterId, SourceKey};

use crate::dom::{self, BookDom};
use crate::pipeline::{Pass, PassCtx};
use crate::refs;
use crate::writer::paths;

pub struct LinkRewrite;

impl Pass for LinkRewrite {
    fn name(&self) -> &'static str {
        "link-rewrite"
    }

    fn run(&self, book: &mut BookDom, ctx: &mut PassCtx<'_>) -> Result<(), BuildError> {
        let chapter_map: HashMap<SourceKey, ChapterId> = book
            .chapters
            .iter()
            .map(|c| (c.source.clone(), c.id))
            .collect();
        let assigned = paths::assign_assets(&book.assets);
        // 每章的元素 id 集（含前端补的 heading id）：锚点校验的数据面
        let anchors: Vec<HashSet<String>> = book
            .chapters
            .iter()
            .map(|c| collect_anchor_ids(c.dom()))
            .collect();
        let total = book.chapters.len();

        let BookDom {
            chapters, warnings, ..
        } = book;
        for (idx, chapter) in chapters.iter_mut().enumerate() {
            ctx.check_cancel()?;
            ctx.progress
                .item(idx as u64, total as u64, &chapter.source.to_string());
            let chapter_source = chapter.source.clone();
            let own_anchors = &anchors[idx];
            let html = chapter.dom_mut();

            for a_id in dom::select_ids(html, sel_links()) {
                let Some(href) = dom::get_attr(html, a_id, "href") else {
                    continue;
                };
                let (target, frag) = refs::split_fragment(&href);

                // 纯锚点（#x）：页内校验，落空降级为 "#"
                if target.is_empty() {
                    if let Some(f) = frag {
                        if resolve_anchor(f, own_anchors).is_none() {
                            warnings.push(Warning::new(
                                WarningCode::LinkUnresolved,
                                Some(chapter_source.clone()),
                                format!("anchor not found in this chapter: #{f}"),
                            ));
                            dom::set_attr(html, a_id, "href", "#");
                            ctx.stats.links_unresolved += 1;
                        }
                    }
                    continue;
                }
                // 引用解析：Path 世界只认相对路径（带 scheme 一律外部）；
                // Url 世界（Pages 源）同源绝对链接也是书内候选——文档站
                // 内链常以绝对 URL 书写
                match resolve_key(&chapter_source, target) {
                    Resolved::External | Resolved::Unresolved => continue, // 原样保留
                    Resolved::Key(key) => {
                        if let Some(&cid) = chapter_map.get(&key) {
                            // 锚点校验：原样 → slug 归一 → 落空去掉锚点并 warning
                            let fragment =
                                frag.map(|f| match resolve_anchor(f, &anchors[cid.0 as usize]) {
                                    Some(resolved) => format!("#{resolved}"),
                                    None => {
                                        warnings.push(Warning::new(
                                            WarningCode::LinkUnresolved,
                                            Some(chapter_source.clone()),
                                            format!("anchor not found in {key}: #{f}"),
                                        ));
                                        ctx.stats.links_unresolved += 1;
                                        String::new()
                                    }
                                });
                            let href = format!(
                                "{}{}",
                                paths::chapter_href(cid),
                                fragment.unwrap_or_default()
                            );
                            dom::set_attr(html, a_id, "href", &href);
                            ctx.stats.links_rewritten += 1;
                        } else if let Some(asset) = assigned.get(&key) {
                            dom::set_attr(html, a_id, "href", &asset.href);
                            ctx.stats.links_rewritten += 1;
                        } else {
                            match &chapter_source {
                                // Url 世界：未入书的链接（采集被过滤 / 站点
                                // 改版）绝对化保留——站外仍可点，比降级纯文
                                // 本保留更多信息
                                SourceKey::Url(_) => {
                                    let SourceKey::Url(target_url) = &key else {
                                        unreachable!("Url 世界解析只产出 Url 键")
                                    };
                                    warnings.push(Warning::new(
                                        WarningCode::LinkUnresolved,
                                        Some(chapter_source.clone()),
                                        format!(
                                            "link target not in book; kept as external: {target_url}"
                                        ),
                                    ));
                                    dom::set_attr(html, a_id, "href", target_url.as_str());
                                    ctx.stats.links_unresolved += 1;
                                }
                                SourceKey::Path(_) => {
                                    warnings.push(Warning::new(
                                        WarningCode::LinkUnresolved,
                                        Some(chapter_source.clone()),
                                        format!("unresolved internal link: {href}"),
                                    ));
                                    dom::unwrap_element(html, a_id);
                                    ctx.stats.links_unresolved += 1;
                                }
                            }
                        }
                    }
                }
            }

            for img_id in dom::select_ids(html, sel_media()) {
                let Some(src) = dom::get_attr(html, img_id, "src") else {
                    continue;
                };
                let alt = dom::get_attr(html, img_id, "alt");
                if dom::tag_name(html, img_id).as_deref() == Some("img") && alt.is_none() {
                    dom::set_attr(html, img_id, "alt", "");
                }
                let Some(key) = media_key(&chapter_source, &src) else {
                    continue;
                };
                if let Some(asset) = assigned.get(&key) {
                    dom::set_attr(html, img_id, "src", &asset.href);
                    ctx.stats.links_rewritten += 1;
                } else {
                    let label = alt.unwrap_or_else(|| src.clone());
                    warnings.push(Warning::new(
                        WarningCode::LinkUnresolved,
                        Some(chapter_source.clone()),
                        format!("image not embedded: {src}"),
                    ));
                    dom::replace_with_text(html, img_id, &format!("[missing image: {label}]"));
                    ctx.stats.links_unresolved += 1;
                }
            }
        }
        Ok(())
    }
}

/// 锚点解析：原样命中 → slug 归一（与前端 heading id 同算法）命中 → `None`。
fn resolve_anchor(fragment: &str, anchors: &HashSet<String>) -> Option<String> {
    if anchors.contains(fragment) {
        return Some(fragment.to_string());
    }
    let slug = Anchorizer::new().anchorize(fragment);
    anchors.contains(&slug).then_some(slug)
}

/// 章节内全部元素 id（前端义务：heading 全有稳定 id；含内嵌 HTML 的 id）。
fn collect_anchor_ids(html: &scraper::Html) -> HashSet<String> {
    dom::select_ids(html, sel_anchored())
        .iter()
        .filter_map(|id| dom::get_attr(html, *id, "id"))
        .collect()
}

fn sel_anchored() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("[id]"))
}

fn sel_links() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("a[href]"))
}

fn sel_media() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("img[src], source[src]"))
}

/// 引用解析结论：书内候选 / 外部（原样保留）/ 无法解析（原样保留）。
enum Resolved {
    Key(SourceKey),
    External,
    Unresolved,
}

/// `a[href]` 引用 → [`Resolved`]。
///
/// - Path 世界：带 scheme 一律外部；相对路径以章节目录为基
///   （[`refs::resolve_rel`]）。
/// - Url 世界：相对引用（含 `/abs-path`、`../x`、`//host` 协议相对）以
///   页面 URL join 成绝对 URL；绝对 http(s) 与页面**同源**才算书内候选
///   （scheme+authority 一致），跨源外部；mailto: / data: 等外部。
fn resolve_key(chapter_source: &SourceKey, target: &str) -> Resolved {
    match chapter_source {
        SourceKey::Path(base) => {
            if refs::is_external(target) {
                Resolved::External
            } else {
                match refs::resolve_rel(base, target) {
                    Some(rel) => Resolved::Key(SourceKey::Path(rel)),
                    None => Resolved::Unresolved,
                }
            }
        }
        SourceKey::Url(base) => resolve_url_key(base, target),
    }
}

fn resolve_url_key(base: &url::Url, target: &str) -> Resolved {
    let candidate = if target.starts_with("//") {
        // 协议相对：以页面 URL 补全 scheme
        base.join(target).ok()
    } else if refs::is_external(target) {
        match refs::scheme_of(target) {
            Some(scheme) if scheme == "http" || scheme == "https" => {
                target.parse::<url::Url>().ok()
            }
            Some(_) => return Resolved::External, // mailto: / data: 等
            None => None,
        }
    } else {
        base.join(target).ok()
    };
    match candidate {
        Some(url) if same_origin(base, &url) => Resolved::Key(SourceKey::Url(url)),
        Some(_) => Resolved::External,
        None => Resolved::Unresolved,
    }
}

fn same_origin(a: &url::Url, b: &url::Url) -> bool {
    a.scheme() == b.scheme() && a.authority() == b.authority()
}

/// 媒体 src → SourceKey：带 scheme（http(s) / data:，跨源图片也是资产）
/// 直接 `Url::parse`；相对引用按世界各解析——与前端登记同一口径。
fn media_key(chapter_source: &SourceKey, src: &str) -> Option<SourceKey> {
    if refs::is_external(src) {
        return src.parse::<url::Url>().ok().map(SourceKey::Url);
    }
    match chapter_source {
        SourceKey::Path(base) => refs::resolve_rel(base, src).map(SourceKey::Path),
        SourceKey::Url(base) => base.join(src).ok().map(SourceKey::Url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trans_epub_contract::source::RelPath;

    fn page_key(url: &str) -> SourceKey {
        SourceKey::Url(url.parse().expect("test url"))
    }

    fn path_key(p: &str) -> SourceKey {
        SourceKey::Path(RelPath::new(p).unwrap())
    }

    /// Url 世界解析表：相对 join、协议相对、同源绝对、跨源、其他 scheme。
    #[test]
    fn url_world_resolution_table() {
        let base = page_key("https://docs.dev/guide/install.html");

        // 相对引用 → 以页面 URL 为基 join，同源 → 书内候选
        let key = resolve_key(&base, "next.html");
        assert!(matches!(&key, Resolved::Key(SourceKey::Url(u))
            if u.as_str() == "https://docs.dev/guide/next.html"));

        // 目录上溯与根相对路径
        let key = resolve_key(&base, "../api.html");
        assert!(matches!(&key, Resolved::Key(SourceKey::Url(u))
            if u.as_str() == "https://docs.dev/api.html"));
        let key = resolve_key(&base, "/top");
        assert!(matches!(&key, Resolved::Key(SourceKey::Url(u))
            if u.as_str() == "https://docs.dev/top"));

        // 同源绝对 → 书内候选；跨源 → 外部
        let key = resolve_key(&base, "https://docs.dev/guide/other.html");
        assert!(matches!(key, Resolved::Key(_)));
        let key = resolve_key(&base, "https://other.dev/guide/other.html");
        assert!(matches!(key, Resolved::External));
        // https → http 混写视作跨源（scheme 不同）
        let key = resolve_key(&base, "http://docs.dev/guide/other.html");
        assert!(matches!(key, Resolved::External));

        // 协议相对：继承页面 scheme，同源判断照常
        let key = resolve_key(&base, "//docs.dev/assets/style.css");
        assert!(matches!(key, Resolved::Key(_)));
        let key = resolve_key(&base, "//cdn.dev/lib.js");
        assert!(matches!(key, Resolved::External));

        // 其他 scheme 一律外部
        assert!(matches!(
            resolve_key(&base, "mailto:a@b.dev"),
            Resolved::External
        ));
        assert!(matches!(
            resolve_key(&base, "data:image/png;base64,x"),
            Resolved::External
        ));
    }

    /// Path 世界解析表：带 scheme 外部；相对解析成 Path 键。
    #[test]
    fn path_world_resolution_table() {
        let base = path_key("guide/ch01.md");
        let key = resolve_key(&base, "ch02.md");
        assert!(matches!(&key, Resolved::Key(SourceKey::Path(p)) if p.as_str() == "guide/ch02.md"));
        assert!(matches!(
            resolve_key(&base, "https://x.dev/a"),
            Resolved::External
        ));
        assert!(matches!(
            resolve_key(&base, "mailto:a@b"),
            Resolved::External
        ));
        // 越界相对 → 无法解析（保留原样，不告警）
        assert!(matches!(
            resolve_key(&base, "../../up.md"),
            Resolved::Unresolved
        ));
    }

    /// 媒体键：跨源绝对图也是资产键；Url 世界相对图 join。
    #[test]
    fn media_key_worlds() {
        let page = page_key("https://docs.dev/guide/a.html");
        let key = media_key(&page, "img/pic.png").unwrap();
        assert!(
            matches!(&key, SourceKey::Url(u) if u.as_str() == "https://docs.dev/guide/img/pic.png")
        );
        let key = media_key(&page, "https://cdn.dev/x.png").unwrap();
        assert!(matches!(&key, SourceKey::Url(u) if u.as_str() == "https://cdn.dev/x.png"));

        let file = path_key("guide/ch01.md");
        let key = media_key(&file, "pic.png").unwrap();
        assert!(matches!(&key, SourceKey::Path(p) if p.as_str() == "guide/pic.png"));
    }
}
