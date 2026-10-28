//! EPUB 内部路径规则的唯一权威（docs/architecture/20-core.md：link-rewrite
//! 与 epub3 writer 都引用这里，改命名规则只动一处）。
//!
//! 产物结构（docs/architecture/50-writer.md）：
//!
//! ```text
//! mimetype / META-INF/container.xml
//! OEBPS/{content.opf, nav.xhtml, Text/chNNNNN.xhtml, Images/…, Styles/…}
//! ```

use std::collections::{BTreeMap, HashMap};

use trans_epub_contract::ir::{Asset, ChapterId, SourceKey};

const OEBPS: &str = "OEBPS";

/// 章节文件名：按 spine 序派生（1 起，`ch00001.xhtml`），不掺时间/哈希
///（确定性；文件名序与 [`ChapterId`] 差 1，勿混用）。
pub fn chapter_file_name(id: ChapterId) -> String {
    format!("ch{:05}.xhtml", id.0 + 1)
}

/// 章节在 zip 内的路径。
pub fn chapter_zip_path(id: ChapterId) -> String {
    format!("{OEBPS}/Text/{}", chapter_file_name(id))
}

/// 章节之间互链的 href（同目录）。
pub fn chapter_href(id: ChapterId) -> String {
    chapter_file_name(id)
}

/// nav.xhtml（位于 OEBPS 根）指向章节的 href。
pub fn chapter_href_from_nav(id: ChapterId) -> String {
    format!("Text/{}", chapter_file_name(id))
}

/// 封面页固定路径。
pub fn cover_zip_path() -> String {
    format!("{OEBPS}/Text/cover.xhtml")
}

/// nav.xhtml 路径。
pub fn nav_zip_path() -> String {
    format!("{OEBPS}/nav.xhtml")
}

/// content.opf 路径。
pub fn opf_zip_path() -> String {
    format!("{OEBPS}/content.opf")
}

/// 样式表在 zip 内的路径（`name` 如 `"base.css"`）。
pub fn style_zip_path(name: &str) -> String {
    format!("{OEBPS}/Styles/{name}")
}

/// 章节（位于 `Text/`）引用样式表的 href。
pub fn style_href(name: &str) -> String {
    format!("../Styles/{name}")
}

/// 字体文件在 zip 内的路径（`Fonts/` 子目录，filename 来自 EmbeddedFont）。
pub fn font_zip_path(filename: &str) -> String {
    format!("{OEBPS}/Fonts/{filename}")
}

/// 字体许可页固定路径（spine 末尾，linear）。
pub fn font_license_zip_path() -> String {
    format!("{OEBPS}/Text/font-license.xhtml")
}

/// 资产在 EPUB 内的落位与类型信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedAsset {
    /// zip 内完整路径，如 `OEBPS/Images/diagram.png`。
    pub zip_path: String,
    /// 章节（`Text/` 下）引用它的 href，如 `../Images/diagram.png`。
    pub href: String,
    /// manifest media-type。
    pub media_type: &'static str,
}

/// 按 [`SourceKey`] 顺序（BTreeMap = 确定性）给资产分配 EPUB 内路径。
///
/// 命名规则：本地文件用净化后的原文件名（保留可读性）；URL 用末段，
/// `data:` URI 合成 `imgNNNNN`。重名追加 `-2`、`-3`。
pub fn assign_assets(assets: &BTreeMap<SourceKey, Asset>) -> BTreeMap<SourceKey, AssignedAsset> {
    let mut used: HashMap<String, u32> = HashMap::new();
    let mut data_seq: u32 = 0;
    let mut out = BTreeMap::new();
    for (key, asset) in assets {
        // 只给已就绪的资产分配路径；Missing 由 link-rewrite 降级，不落盘
        let bytes = match &asset.data {
            trans_epub_contract::ir::AssetData::Owned(bytes) => bytes,
            _ => continue,
        };
        let Some(media) = detect_media_type(key, bytes) else {
            continue;
        };
        let base = match key {
            SourceKey::Path(rel) => sanitize_file_name(rel.file_name()),
            SourceKey::Url(url) => {
                if url.scheme() == "data" {
                    data_seq += 1;
                    format!("img{data_seq:05}")
                } else {
                    let last = url.path().rsplit('/').next().unwrap_or("");
                    let last = percent_decode_name(last);
                    let sanitized = sanitize_file_name(&last);
                    if sanitized != "asset" {
                        sanitized
                    } else {
                        data_seq += 1;
                        format!("asset{data_seq:05}")
                    }
                }
            }
        };
        // 无扩展名的名字补上识别出的扩展名（manifest 的 media-type 要与
        // 内容一致，文件名带扩展名对阅读器更友好）
        let base = if base.contains('.') {
            base
        } else {
            format!("{base}.{}", media.ext)
        };
        let unique = dedup_name(&base, &mut used);
        out.insert(
            key.clone(),
            AssignedAsset {
                zip_path: format!("{OEBPS}/{}/{}", media.dir, unique),
                href: format!("../{}/{}", media.dir, unique),
                media_type: media.mime,
            },
        );
    }
    out
}

/// link-rewrite 查询：某资产是否就绪并得到 EPUB 路径。
pub fn asset_href_of(
    assigned: &BTreeMap<SourceKey, AssignedAsset>,
    key: &SourceKey,
) -> Option<String> {
    assigned.get(key).map(|a| a.href.clone())
}

/// 识别出的媒体类型（目录 + MIME + 扩展名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Media {
    /// EPUB 内目录名（`Images` / `Fonts` / `Media`）。
    pub dir: &'static str,
    /// manifest media-type。
    pub mime: &'static str,
    /// 合成文件名时用的扩展名。
    pub ext: &'static str,
}

const IMAGE_PNG: Media = Media {
    dir: "Images",
    mime: "image/png",
    ext: "png",
};
const IMAGE_JPEG: Media = Media {
    dir: "Images",
    mime: "image/jpeg",
    ext: "jpg",
};
const IMAGE_GIF: Media = Media {
    dir: "Images",
    mime: "image/gif",
    ext: "gif",
};
const IMAGE_SVG: Media = Media {
    dir: "Images",
    mime: "image/svg+xml",
    ext: "svg",
};
const IMAGE_WEBP: Media = Media {
    dir: "Images",
    mime: "image/webp",
    ext: "webp",
};
const IMAGE_BMP: Media = Media {
    dir: "Images",
    mime: "image/bmp",
    ext: "bmp",
};
const FONT_OTF: Media = Media {
    dir: "Fonts",
    mime: "font/otf",
    ext: "otf",
};
const FONT_TTF: Media = Media {
    dir: "Fonts",
    mime: "font/ttf",
    ext: "ttf",
};
const AUDIO_MP4: Media = Media {
    dir: "Media",
    mime: "audio/mp4",
    ext: "m4a",
};

/// 识别资产类型：扩展名优先，其次魔数嗅探；识别不出返回 `None`
///（调用方据此 `AssetUnsupported` 降级）。
pub fn detect_media_type(key: &SourceKey, bytes: &[u8]) -> Option<Media> {
    let by_name = key_name(key)
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .and_then(|ext| match ext.as_str() {
            "png" => Some(IMAGE_PNG),
            "jpg" | "jpeg" => Some(IMAGE_JPEG),
            "gif" => Some(IMAGE_GIF),
            "svg" => Some(IMAGE_SVG),
            "webp" => Some(IMAGE_WEBP),
            "bmp" => Some(IMAGE_BMP),
            "otf" => Some(FONT_OTF),
            "ttf" => Some(FONT_TTF),
            "m4a" => Some(AUDIO_MP4),
            _ => None,
        });
    by_name.or_else(|| sniff(bytes))
}

fn sniff(bytes: &[u8]) -> Option<Media> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(IMAGE_PNG)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(IMAGE_JPEG)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(IMAGE_GIF)
    } else if bytes.starts_with(b"BM") {
        Some(IMAGE_BMP)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(IMAGE_WEBP)
    } else if looks_like_svg(bytes) {
        Some(IMAGE_SVG)
    } else {
        None
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_lowercase();
    (head.trim_start().starts_with("<svg") || head.contains("<?xml")) && head.contains("<svg")
}

fn key_name(key: &SourceKey) -> String {
    match key {
        SourceKey::Path(rel) => rel.file_name().to_string(),
        SourceKey::Url(url) => {
            if url.scheme() == "data" {
                // data:image/png;base64,…
                url.path().split(';').next().unwrap_or("").to_string()
            } else {
                url.path().rsplit('/').next().unwrap_or("").to_string()
            }
        }
    }
}

/// 净化文件名：只保留 `[A-Za-z0-9._-]`，其余折叠为 `_`，去首部点号。
fn sanitize_file_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_underscore = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
            out.push(c);
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    let trimmed = out.trim_matches('.').to_string();
    if trimmed.is_empty() {
        "asset".to_string()
    } else {
        trimmed
    }
}

fn percent_decode_name(s: &str) -> String {
    crate::refs::percent_decode(s)
}

fn dedup_name(base: &str, used: &mut HashMap<String, u32>) -> String {
    let count = used.entry(base.to_string()).or_insert(0);
    *count += 1;
    if *count == 1 {
        base.to_string()
    } else {
        match base.rsplit_once('.') {
            Some((stem, ext)) => format!("{stem}-{count}.{ext}"),
            None => format!("{base}-{count}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trans_epub_contract::ir::AssetData;
    use trans_epub_contract::source::RelPath;

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D,
    ];

    fn asset(path: &str, data: AssetData) -> (SourceKey, Asset) {
        let key = SourceKey::Path(RelPath::new(path).unwrap());
        (key.clone(), Asset { source: key, data })
    }

    #[test]
    fn chapter_paths_are_order_derived() {
        assert_eq!(chapter_file_name(ChapterId(0)), "ch00001.xhtml");
        assert_eq!(chapter_file_name(ChapterId(41)), "ch00042.xhtml");
        assert_eq!(chapter_zip_path(ChapterId(1)), "OEBPS/Text/ch00002.xhtml");
        assert_eq!(chapter_href(ChapterId(2)), "ch00003.xhtml");
        assert_eq!(chapter_href_from_nav(ChapterId(2)), "Text/ch00003.xhtml");
        assert_eq!(style_href("base.css"), "../Styles/base.css");
    }

    #[test]
    fn asset_names_sanitized_and_deduped() {
        let mut assets = BTreeMap::new();
        assets.extend([
            asset("img/diagram v2.png", AssetData::Owned(PNG.to_vec())),
            asset("img2/diagram v2.png", AssetData::Owned(PNG.to_vec())),
            asset("noext", AssetData::Owned(PNG.to_vec())),
        ]);
        let assigned = assign_assets(&assets);
        assert_eq!(
            assigned
                .get(&SourceKey::Path(
                    RelPath::new("img/diagram v2.png").unwrap()
                ))
                .unwrap()
                .zip_path,
            "OEBPS/Images/diagram_v2.png"
        );
        assert_eq!(
            assigned
                .get(&SourceKey::Path(
                    RelPath::new("img2/diagram v2.png").unwrap()
                ))
                .unwrap()
                .zip_path,
            "OEBPS/Images/diagram_v2-2.png"
        );
        // 无扩展名：魔数嗅探出 png，合成扩展名
        let noext = assigned
            .get(&SourceKey::Path(RelPath::new("noext").unwrap()))
            .unwrap();
        assert_eq!(noext.media_type, "image/png");
        assert!(noext.zip_path.ends_with(".png"), "{}", noext.zip_path);
    }

    #[test]
    fn missing_assets_get_no_path() {
        let mut assets = BTreeMap::new();
        assets.extend([asset("gone.png", AssetData::Missing)]);
        assert!(assign_assets(&assets).is_empty());
    }

    #[test]
    fn sniffs_all_magic_numbers() {
        // 无扩展名 → 走 sniff 魔数链（分支覆盖定位的洼地：此前只有 PNG 被走）
        let key = SourceKey::Path(RelPath::new("blob").unwrap());
        let sniffed = |bytes: &[u8]| detect_media_type(&key, bytes).map(|m| m.mime);
        assert_eq!(
            sniffed(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A]),
            Some("image/png")
        );
        assert_eq!(sniffed(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniffed(b"GIF87a-rest"), Some("image/gif"));
        assert_eq!(sniffed(b"GIF89a-rest"), Some("image/gif"));
        assert_eq!(sniffed(b"BM\x36\x00\x00\x00"), Some("image/bmp"));
        // RIFF+WEBP：需要 >= 12 字节且 [8..12] == "WEBP"
        assert_eq!(sniffed(b"RIFF\x00\x00\x00\x00WEBP"), Some("image/webp"));
        assert_eq!(
            sniffed(b"RIFF\x00\x00\x00"),
            None,
            "不足 12 字节不嗅探 WEBP"
        );
        // SVG：头部 <svg 或含 <?xml…<svg
        assert_eq!(sniffed(b"<svg xmlns='x'>"), Some("image/svg+xml"));
        assert_eq!(sniffed(b"  <SVG "), Some("image/svg+xml"), "大小写不敏感");
        assert_eq!(
            sniffed(b"<?xml version='1.0'?><svg xmlns='x'/>"),
            Some("image/svg+xml")
        );
        assert_eq!(sniffed(b"<div>not svg</div>"), None);
        assert_eq!(sniffed(b""), None);
    }

    #[test]
    fn declared_extension_wins_over_sniff() {
        // 扩展名优先于魔数（口径见 detect_media_type 注释）；未知扩展名回落嗅探
        let jpeg_bytes: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0];
        let png_named = SourceKey::Path(RelPath::new("fake.png").unwrap());
        assert_eq!(
            detect_media_type(&png_named, jpeg_bytes).map(|m| m.mime),
            Some("image/png"),
            ".png 扩展名优先，字节是 JPEG 也不改判"
        );
        let unknown_named = SourceKey::Path(RelPath::new("asset.binx").unwrap());
        assert_eq!(
            detect_media_type(&unknown_named, jpeg_bytes).map(|m| m.mime),
            Some("image/jpeg"),
            "未知扩展名回落魔数嗅探"
        );
        // URL 源：http 路径取末段扩展名；data: URI 的 mime 段无点号扩展名
        //（"image/png" 取不出白名单扩展），回落魔数嗅探——现状口径
        let http_url = SourceKey::Url("https://a.dev/img/logo.png".parse().unwrap());
        assert_eq!(
            detect_media_type(&http_url, jpeg_bytes).map(|m| m.mime),
            Some("image/png"),
            "URL 末段扩展名优先"
        );
        let data_url = SourceKey::Url("data:image/jpeg;base64,aGVsbG8=".parse().unwrap());
        assert_eq!(
            detect_media_type(&data_url, jpeg_bytes).map(|m| m.mime),
            Some("image/jpeg"),
            "data: URI 无点号扩展名 → sniff 兜底"
        );
    }
}
