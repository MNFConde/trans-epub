//! IR 枢轴：frontends 的终点、pipeline 的起点。
//!
//! 前端义务（docs/architecture/10-contracts.md「前端义务清单」）：
//! 干净 HTML、heading 全有稳定 id、引用保留原始 href/src、顺序与嵌套
//! 在 load 时定死、单项失败记 warning 继续、不做通用 HTML 处理。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::Warning;
use crate::source::RelPath;

/// 书稿：三种来源归一后的统一形态。
///
/// 不变量（违反即 bug）：
/// - [`ChapterId`] 恒等于 `chapters` 下标；
/// - load 之后 pass 不得增删章节或改变顺序（章节身份必须先稳定，
///   link-rewrite 依赖此前提）；
/// - `assets` 的键与章节 `source` 同处一个 [`SourceKey`] 编址空间；
/// - `warnings` 单调递增；任何"跳过/降级/剔除"决定都必须伴随一条
///   warning（可追溯）。
#[derive(Debug, Clone, Default)]
pub struct BookDraft {
    pub meta: BookMeta,
    /// 嵌套目录，引用 [`ChapterId`]；part 分组节点 `chapter` 为 `None`。
    pub toc: Vec<TocItem>,
    /// 顺序即 spine 顺序。
    pub chapters: Vec<Chapter>,
    /// 章节外资源（图片/字体/封面），键与章节 source 同一编址空间。
    pub assets: BTreeMap<SourceKey, Asset>,
    /// 全程累积，只增不减。
    pub warnings: Vec<Warning>,
}

impl BookDraft {
    /// 追加一章并返回其 id（spine 序）。
    pub fn push_chapter(
        &mut self,
        title: impl Into<String>,
        source: SourceKey,
        html: impl Into<String>,
    ) -> ChapterId {
        let id = ChapterId(self.chapters.len() as u32);
        self.chapters.push(Chapter {
            id,
            title: title.into(),
            source,
            html: html.into(),
        });
        id
    }

    /// 登记一条 warning（容忍轨：单项失败不中止）。
    pub fn warn(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }
}

/// 章节。`html` 为干净 HTML，保留原始 href/src（改写属于 link-rewrite
/// pass）；heading 均有稳定 id（前端义务）。
#[derive(Debug, Clone)]
pub struct Chapter {
    pub id: ChapterId,
    pub title: String,
    /// 来源标识：诊断定位 + 相对引用的解析基址。
    pub source: SourceKey,
    pub html: String,
}

/// 章节标识 = `chapters` 下标 = spine 序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChapterId(pub u32);

/// 目录节点。嵌套不拍平（mdBook 语义，ADR-0005）。
#[derive(Debug, Clone)]
pub struct TocItem {
    /// `None` = 纯分组标题（mdBook part）。
    pub chapter: Option<ChapterId>,
    pub title: String,
    pub children: Vec<TocItem>,
}

/// 章节外的资源。
#[derive(Debug, Clone)]
pub struct Asset {
    pub source: SourceKey,
    pub data: AssetData,
}

/// 资产数据的三个状态。
#[derive(Debug, Clone)]
pub enum AssetData {
    /// load 时已就绪（本地文件 / data: URI / 封面）。
    Owned(Vec<u8>),
    /// 由 asset-resolve pass 经 Fetcher 拉取。
    Remote(Url),
    /// resolve 失败的终态；引用方降级处理。
    Missing,
}

/// 书籍元数据。
#[derive(Debug, Clone, Default)]
pub struct BookMeta {
    pub title: String,
    pub authors: Vec<String>,
    /// BCP-47；空串表示未定（写出器用配置兜底）。
    pub language: String,
    pub description: Option<String>,
    /// 指向 assets 里的封面资源。
    pub cover: Option<SourceKey>,
}

/// 资源编址：章节与资产共用同一编址空间（链接改写的地基）。
///
/// 规范化：Path 侧由 [`RelPath`] 保证；Url 侧为绝对 URL、无 fragment、
/// scheme/host 小写、无默认端口、path 与 query 原样保留。Path 键与
/// Url 键永不互相解析——跨世界引用即 `LinkUnresolved`。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SourceKey {
    /// Vfs 世界：相对源根的规范化路径。
    Path(RelPath),
    /// Pages 世界：规范化 URL（无 fragment）。
    Url(Url),
}

impl std::fmt::Display for SourceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceKey::Path(p) => f.write_str(p.as_str()),
            SourceKey::Url(u) => f.write_str(u.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_chapter_assigns_spine_ids() {
        let mut book = BookDraft::default();
        let a = book.push_chapter(
            "A",
            SourceKey::Path(RelPath::new("a.md").unwrap()),
            "<p>a</p>",
        );
        let b = book.push_chapter(
            "B",
            SourceKey::Path(RelPath::new("b.md").unwrap()),
            "<p>b</p>",
        );
        assert_eq!((a.0, b.0), (0, 1));
        assert_eq!(book.chapters.len(), 2);
        // ChapterId 恒等于下标（不变量）
        assert_eq!(book.chapters[b.0 as usize].id, b);
    }
}
