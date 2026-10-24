//! asset-resolve（链位 1）：把所有资产解析为可嵌入字节。
//!
//! 前端在 load 时已登记资产（本地/data: 为 Owned，远程为 Remote）；本
//! pass 负责：Remote 经 Fetcher 拉取 → 类型识别（不识别即 Unsupported）
//! → 单文件/总量上限检查 → 超限或失败转 [`AssetData::Missing`] + warning
//!（link-rewrite 会据此降级引用）。资产键集在 load 后冻结。

use trans_epub_contract::error::{BuildError, Warning, WarningCode};
use trans_epub_contract::ir::{AssetData, SourceKey};

use crate::dom::BookDom;
use crate::pipeline::{Pass, PassCtx};
use crate::writer::paths;

pub struct AssetResolve;

impl Pass for AssetResolve {
    fn name(&self) -> &'static str {
        "asset-resolve"
    }

    fn run(&self, book: &mut BookDom, ctx: &mut PassCtx<'_>) -> Result<(), BuildError> {
        let keys: Vec<SourceKey> = book.assets.keys().cloned().collect();
        let total_keys = keys.len();
        let mut total_bytes: u64 = 0;
        for (i, key) in keys.into_iter().enumerate() {
            ctx.check_cancel()?;
            ctx.progress
                .item(i as u64, total_keys as u64, &key.to_string());

            // 1) Remote → 拉取（离线模式直接降级）
            if let Some(url) = remote_url(book, &key) {
                if !ctx.config.assets.remote {
                    book.warn(Warning::new(
                        WarningCode::AssetFetchFailed,
                        Some(key.clone()),
                        "remote assets disabled (assets.remote = false)",
                    ));
                    set_missing(book, &key);
                    continue;
                }
                match ctx.fetcher.fetch(&url) {
                    Ok(page) => {
                        if let Some(asset) = book.assets.get_mut(&key) {
                            asset.data = AssetData::Owned(page.body);
                        }
                    }
                    Err(err) => {
                        book.warn(Warning::new(
                            WarningCode::AssetFetchFailed,
                            Some(key.clone()),
                            format!("fetch {url} failed: {err}"),
                        ));
                        set_missing(book, &key);
                        continue;
                    }
                }
            }

            // 2) Owned → 类型识别 + 上限检查
            let Some(asset) = book.assets.get(&key) else {
                continue;
            };
            let AssetData::Owned(bytes) = &asset.data else {
                continue;
            };
            if bytes.len() as u64 > ctx.config.assets.max_file_bytes {
                book.warn(Warning::new(
                    WarningCode::AssetTooLarge,
                    Some(key.clone()),
                    format!(
                        "{} exceeds per-file limit of {} bytes",
                        bytes.len(),
                        ctx.config.assets.max_file_bytes
                    ),
                ));
                set_missing(book, &key);
                continue;
            }
            if total_bytes + bytes.len() as u64 > ctx.config.assets.max_total_bytes {
                book.warn(Warning::new(
                    WarningCode::AssetTooLarge,
                    Some(key.clone()),
                    format!(
                        "total embedded assets would exceed limit of {} bytes",
                        ctx.config.assets.max_total_bytes
                    ),
                ));
                set_missing(book, &key);
                continue;
            }
            if paths::detect_media_type(&key, bytes).is_none() {
                book.warn(Warning::new(
                    WarningCode::AssetUnsupported,
                    Some(key.clone()),
                    format!("unsupported asset type: {key}"),
                ));
                set_missing(book, &key);
                continue;
            }
            total_bytes += bytes.len() as u64;
        }
        Ok(())
    }
}

fn remote_url(book: &BookDom, key: &SourceKey) -> Option<url::Url> {
    match book.assets.get(key)?.data {
        AssetData::Remote(ref url) => Some(url.clone()),
        _ => None,
    }
}

fn set_missing(book: &mut BookDom, key: &SourceKey) {
    if let Some(asset) = book.assets.get_mut(key) {
        asset.data = AssetData::Missing;
    }
}
