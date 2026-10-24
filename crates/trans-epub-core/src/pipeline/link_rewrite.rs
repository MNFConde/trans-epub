//! link-rewrite（链位 2）：把章节/资产引用改写为 EPUB 内部路径。
//!
//! 前置：asset-resolve 已定资产终态（Missing 的引用在此降级）；
//! 章节集合已冻结（load 之后不变）。改写规则：
//!
//! - `a[href]`：纯锚点与外部链接（http(s)/mailto/…）原样保留；相对路径
//!   解析为 [`SourceKey`]——命中章节 → `chNNNNN.xhtml#锚点`，命中资产 →
//!   资产路径；未解析 → 降级为纯文本 + `link-unresolved` warning。
//! - `img[src]` / `source[src]`：命中就绪资产 → 改写；否则整体替换为
//!   alt 文本占位（EPUB 不抓远程资源，悬空 src 过不了 epubcheck）。
//!   `img` 无 alt 时补 `alt=""`（epubcheck 无障碍要求）。
//!
//! 路径规则来自 [`crate::writer::paths`]（与 writer 同源，改命名只动一处）。
//! 绝对/`data:` URL 与前端登记口径一致：两侧都 `Url::parse` 同一字符串，
//! url crate 的规范化即去重。

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
                if refs::is_external(target) {
                    continue; // 外部链接：原样保留
                }
                let Some(key) = resolve_key(&chapter_source, target) else {
                    continue; // Url 世界章节（MVP3）或无法解析的引用
                };
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
                    warnings.push(Warning::new(
                        WarningCode::LinkUnresolved,
                        Some(chapter_source.clone()),
                        format!("unresolved internal link: {href}"),
                    ));
                    dom::unwrap_element(html, a_id);
                    ctx.stats.links_unresolved += 1;
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

/// 相对引用 → SourceKey（Path 世界）。Url 世界（Pages 源）为 MVP3 工作，
/// 当前返回 `None` 走"保留原样"。
fn resolve_key(chapter_source: &SourceKey, target: &str) -> Option<SourceKey> {
    match chapter_source {
        SourceKey::Path(base) => refs::resolve_rel(base, target).map(SourceKey::Path),
        SourceKey::Url(_) => None, // TODO(MVP3)：以页面 URL 为基解析
    }
}

/// 媒体 src → SourceKey：相对路径走 [`resolve_key`]；带 scheme（http(s) /
/// data:）直接 `Url::parse`——与前端登记同一口径。
fn media_key(chapter_source: &SourceKey, src: &str) -> Option<SourceKey> {
    if refs::is_external(src) {
        let url: url::Url = src.parse().ok()?;
        Some(SourceKey::Url(url))
    } else {
        resolve_key(chapter_source, src)
    }
}
