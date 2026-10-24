//! 输入侧：两种源形态与 I/O 注入口。
//!
//! 所有已知输入收敛为两种抽象（ADR-0003）：文件树世界（散装 md、
//! mdBook 项目）走 [`Source::Vfs`]；页面世界（静态站、外置渲染器）走
//! [`Source::Pages`]，且是**已抓取**的页面——抓取发生在 core 之外
//! （trans-epub-fetch / 未来服务），core 不含任何爬取逻辑。

use url::Url;

/// 一次转换的输入。壳层负责构造：CLI 用磁盘/采集结果，服务用上传内容。
pub enum Source {
    /// 文件树形态（散装 md / mdBook 项目）。
    Vfs(std::sync::Arc<dyn Vfs>),
    /// 页面列表形态（静态站 / 外置渲染器）；**顺序即书序**，由采集方负责。
    Pages(Vec<FetchedPage>),
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Vfs(_) => f.debug_tuple("Source::Vfs(..)").finish(),
            Source::Pages(pages) => f.debug_tuple("Source::Pages").field(&pages.len()).finish(),
        }
    }
}

/// 归一化相对路径：'/' 分隔、无 `..`、无盘符、无前导 '/'、无反斜杠。
///
/// 安全规则：任何来自输入的路径必须经 [`RelPath::new`] 校验后才能进
/// [`Vfs`]（防路径穿越）。构造时归一化：丢弃 `.` 段与空段
/// （`a//b` ≡ `a/b`）。
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct RelPath(String);

impl RelPath {
    /// 校验、归一化并创建。
    pub fn new(path: &str) -> Result<Self, InvalidPath> {
        if path.is_empty() {
            return Err(InvalidPath::Empty);
        }
        if path.starts_with('/') || path.starts_with('\\') {
            return Err(InvalidPath::Absolute(path.to_string()));
        }
        let mut parts = Vec::new();
        for seg in path.split('/') {
            if seg.is_empty() || seg == "." {
                continue; // 归一化：丢弃空段与当前目录段
            }
            if seg == ".." {
                return Err(InvalidPath::ParentTraversal(path.to_string()));
            }
            if seg.contains('\\') || seg.contains(':') || seg.contains('\0') {
                return Err(InvalidPath::Illegal(format!("segment {seg:?}")));
            }
            parts.push(seg);
        }
        if parts.is_empty() {
            return Err(InvalidPath::Empty);
        }
        Ok(Self(parts.join("/")))
    }

    /// 归一化后的路径字符串（'/' 分隔）。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 父路径（顶层返回 `None`）。
    pub fn parent(&self) -> Option<RelPath> {
        let parent = self.0.rsplit_once('/')?.0;
        if parent.is_empty() {
            None
        } else {
            Some(RelPath(parent.to_string()))
        }
    }

    /// 末段文件名。
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }
}

/// [`RelPath`] 校验失败的原因。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidPath {
    /// 路径为空（或归一化后为空，如 `"."`）。
    #[error("path is empty")]
    Empty,
    /// 绝对路径（前导 `/` 或 `\`）。
    #[error("absolute path: {0}")]
    Absolute(String),
    /// 含 `..` 段，拒绝以防路径穿越。
    #[error("parent traversal: {0}")]
    ParentTraversal(String),
    /// 段内含非法字符（反斜杠 / 盘符冒号 / NUL）。
    #[error("illegal segment: {0}")]
    Illegal(String),
}

/// 文件树读取口：磁盘 / 内存 / 对象存储由壳层各自实现。
pub trait Vfs: Send + Sync {
    /// 读取文件内容；不存在返回 [`VfsError::NotFound`]。
    fn read(&self, path: &RelPath) -> Result<Vec<u8>, VfsError>;
    /// 文件是否存在。
    fn exists(&self, path: &RelPath) -> bool;
    /// 递归列举全部文件（含大小：供上限检查与结构探测）。
    fn walk(&self) -> Result<Vec<FileEntry>, VfsError>;
}

/// [`Vfs::walk`] 的条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: RelPath,
    pub size: u64,
}

/// [`Vfs`] 错误。
#[derive(Debug, thiserror::Error)]
pub enum VfsError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("io error at {path}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Other(String),
}

/// 已抓取的页面。抓取发生在 core 之外（fetch crate / 未来服务）。
#[derive(Debug, Clone)]
pub struct FetchedPage {
    /// 规范化后的绝对 URL，无 fragment。
    pub url: Url,
    /// 原始字节，编码由前端处理。
    pub body: Vec<u8>,
    /// 如 `"text/html; charset=utf-8"`。
    pub content_type: String,
}

impl FetchedPage {
    /// 从已得字节组装一个页面（Fetcher 实现与测试用）。
    pub fn from_bytes(url: Url, body: Vec<u8>, content_type: impl Into<String>) -> Self {
        Self {
            url,
            body,
            content_type: content_type.into(),
        }
    }
}

/// 远程资源获取口：pipeline 拉远程图片时使用。
///
/// robots / 限速 / 缓存 / 代理全部是实现的事（trans-epub-fetch 或
/// 服务自带实现），core 只管调用。同步 trait（core 是同步内核，
/// ADR-0004）；异步实现由服务侧用 `spawn_blocking` 适配。
pub trait Fetcher: Send + Sync {
    fn fetch(&self, url: &Url) -> Result<FetchedPage, FetchError>;
}

/// [`Fetcher`] 错误。
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("network error: {0}")]
    Network(String),
    /// robots.txt / 离线策略等拒绝获取。
    #[error("denied by policy: {0}")]
    Denied(String),
    #[error("resource too large: {0}")]
    TooLarge(String),
    #[error("http {status} for {url}")]
    HttpStatus { status: u16, url: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_path_normalizes() {
        assert_eq!(RelPath::new("a/./b//c.md").unwrap().as_str(), "a/b/c.md");
        assert_eq!(RelPath::new("README.md").unwrap().file_name(), "README.md");
        assert_eq!(
            RelPath::new("a/b/c.md").unwrap().parent().unwrap().as_str(),
            "a/b"
        );
        assert_eq!(RelPath::new("top.md").unwrap().parent(), None);
    }

    #[test]
    fn rel_path_rejects_unsafe() {
        assert!(matches!(RelPath::new(""), Err(InvalidPath::Empty)));
        assert!(matches!(RelPath::new("."), Err(InvalidPath::Empty)));
        assert!(matches!(
            RelPath::new("/abs"),
            Err(InvalidPath::Absolute(_))
        ));
        assert!(matches!(
            RelPath::new("../up"),
            Err(InvalidPath::ParentTraversal(_))
        ));
        assert!(matches!(
            RelPath::new("a/b/../c"),
            Err(InvalidPath::ParentTraversal(_))
        ));
        assert!(matches!(RelPath::new("a\\b"), Err(InvalidPath::Illegal(_))));
        assert!(matches!(RelPath::new("C:/x"), Err(InvalidPath::Illegal(_))));
    }
}
