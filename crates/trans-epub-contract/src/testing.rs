//! 测试工具：契约 trait 的内存实现（`test-util` feature 门控）。
//!
//! 供 trans-epub-core / trans-epub-cli / 未来服务的测试复用：mock 文件树、
//! mock 远程资源，全程不碰磁盘与网络（ADR-0004：core 自身永远不做 I/O，
//! 测试里也一样）。生产代码禁止引用本模块。

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::source::{FetchError, FetchedPage, Fetcher, FileEntry, RelPath, Vfs, VfsError};

/// 内存文件树：`Vfs` 的确定性实现。
///
/// `walk()` 按 [`RelPath`] 排序返回（BTreeMap 天然有序），保证探测/探测
/// 复跑结果一致。
#[derive(Debug, Clone, Default)]
pub struct MemVfs {
    files: BTreeMap<RelPath, Vec<u8>>,
}

impl MemVfs {
    pub fn new() -> Self {
        Self::default()
    }

    /// 插入一个文件；路径不合法直接 panic（测试代码，构造期失败可接受）。
    pub fn insert(mut self, path: &str, bytes: impl Into<Vec<u8>>) -> Self {
        let rel = RelPath::new(path).expect("test fixture path must be valid RelPath");
        self.files.insert(rel, bytes.into());
        self
    }

    /// 把磁盘目录整树读进内存（golden 测试从 fixtures 目录构造输入用）。
    pub fn from_dir(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut vfs = Self::new();
        let root = root.as_ref();
        if root.is_file() {
            let name = root.file_name().expect("file has a name");
            vfs.files.insert(
                RelPath::new(&name.to_string_lossy()).expect("valid name"),
                std::fs::read(root)?,
            );
            return Ok(vfs);
        }
        Self::walk_dir(root, "", &mut vfs.files)?;
        Ok(vfs)
    }

    fn walk_dir(
        dir: &Path,
        prefix: &str,
        out: &mut BTreeMap<RelPath, Vec<u8>>,
    ) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                Self::walk_dir(&path, &rel, out)?;
            } else {
                out.insert(
                    RelPath::new(&rel).expect("valid fs name"),
                    std::fs::read(&path)?,
                );
            }
        }
        Ok(())
    }
}

impl Vfs for MemVfs {
    fn read(&self, path: &RelPath) -> Result<Vec<u8>, VfsError> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| VfsError::NotFound(path.as_str().to_string()))
    }

    fn exists(&self, path: &RelPath) -> bool {
        self.files.contains_key(path)
    }

    fn walk(&self) -> Result<Vec<FileEntry>, VfsError> {
        Ok(self
            .files
            .iter()
            .map(|(path, bytes)| FileEntry {
                path: path.clone(),
                size: bytes.len() as u64,
            })
            .collect())
    }
}

/// mock 远程资源：按 URL 应答固定字节，未登记的 URL 返回网络错误。
#[derive(Debug, Clone, Default)]
pub struct MockFetcher {
    pages: HashMap<url::Url, (Vec<u8>, String)>,
}

impl MockFetcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个应答；`content_type` 如 `"image/png"`。
    pub fn insert(mut self, url: &str, body: impl Into<Vec<u8>>, content_type: &str) -> Self {
        let url: url::Url = url.parse().expect("test fixture url must be valid");
        self.pages
            .insert(url, (body.into(), content_type.to_string()));
        self
    }
}

impl Fetcher for MockFetcher {
    fn fetch(&self, url: &url::Url) -> Result<crate::source::FetchedPage, FetchError> {
        match self.pages.get(url) {
            Some((body, content_type)) => Ok(FetchedPage::from_bytes(
                url.clone(),
                body.clone(),
                content_type.clone(),
            )),
            None => Err(FetchError::Network(format!("mock: no fixture for {url}"))),
        }
    }
}
