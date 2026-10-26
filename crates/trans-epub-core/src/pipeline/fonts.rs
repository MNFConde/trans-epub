//! fonts pass：内嵌字体准备（10-contracts.md「内嵌字体」节的三正交落地）。
//!
//! - builtin × preset：内置预子集原样内嵌（零网络）
//! - builtin × exact：经 Fetcher 下载钉版全量 zip → 校验 sha256 → 解出
//!   Regular → 按书内字符子集 → 改名；任一步失败回落 preset + warning
//! - user：data（壳层已解析）按 subset 模式子集，不改名不出义务页
//!
//! 字体缓存是本 pass 对"core 不做文件 IO"的唯一例外（构建产物缓存，非
//! 书籍内容 IO）：`{tmp}/trans-epub-fonts/{sha}.ttf`，命中即免下载。

use std::collections::BTreeSet;
use std::io::Read;

use trans_epub_contract::config::{FontSource, FontSubsetMode};
use trans_epub_contract::error::WarningCode;
use trans_epub_contract::ir::{EmbeddedFont, FontLicense};

use super::{BookDom, Pass, PassCtx};
use crate::ttf;

/// 内置预子集（Maple Mono CN v7.9 子集 + 已改名，生成见 NOTICE.md）。
const PRESET_SUBSET: &[u8] = include_bytes!("../../assets/fonts/TransEpubMono-Regular.subset.ttf");
const OFL_TEXT: &str = include_str!("../../assets/fonts/OFL-1.1.txt");
const HANZI_8105: &str = include_str!("../../assets/fonts/hanzi-8105.txt");
const PRESET_FAMILY: &str = "TransEpub Mono";
const PRESET_FILENAME: &str = "TransEpubMono-Regular.subset.ttf";

/// exact 下载钉版：v7.9 unhinted 全家桶 zip（官方 .sha256 一致）。
const PINNED_ZIP_URL: &str =
    "https://github.com/subframe7536/maple-font/releases/download/v7.9/MapleMono-CN-unhinted.zip";
const PINNED_ZIP_SHA256: &str = "d41cb72721e99cfe4fbd1a7b0f182a013457de46aa612018f924dd024699d3b9";

/// 与 scripts/gen-font-subset.py 的 EXTRA_CHARS 保持同一集合（用户字体
/// preset 模式的子集口径与内置预子集一致）。
const EXTRA_CHARS: &str = "，。、；：？！“”‘’（）《》〈〉【】〔〕「」『』…—·～　％＋－＊／＝＜＞（）［］｛｝‘’“”–—…′″€£¥©®°±·─│┌┐└┘├┤┬┴┼╌╎═║→←↑↓⇒⇐↔↦≈≠≤≥±×÷∞∴∵∈∉⊂⊃∪∩¬√∑∏∫αβγδεζηθικλμνξοπρστυφχψωΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ";

pub struct Fonts;

impl Pass for Fonts {
    fn name(&self) -> &'static str {
        "fonts"
    }

    fn run(
        &self,
        book: &mut BookDom,
        ctx: &mut PassCtx<'_>,
    ) -> Result<(), trans_epub_contract::error::BuildError> {
        let cfg = ctx.config.fonts.clone();
        if !cfg.embed {
            return Ok(());
        }
        let font = match &cfg.source {
            FontSource::Builtin => match cfg.subset {
                FontSubsetMode::Preset => Some(preset_font(None)),
                FontSubsetMode::Exact => Some(builtin_exact(book, ctx).unwrap_or_else(|w| {
                    book.warn(w);
                    preset_font(Some("exact 下载/子集失败，回落 preset 子集"))
                })),
            },
            FontSource::User { data, family, .. } => {
                user_font(book, data.as_deref(), family.clone(), cfg.subset)
            }
        };
        if let Some(f) = font {
            book.fonts.push(f);
        }
        Ok(())
    }
}

fn preset_font(extra: Option<&str>) -> EmbeddedFont {
    let mut notice = format!(
        "字体为 Maple Mono CN（https://github.com/subframe7536/maple-font，\
         release v7.9，zip sha256 {PINNED_ZIP_SHA256}）的子集：\
         《通用规范汉字表》8105 字 + 编程文档常用字符；族名由 \"Maple Mono CN\" \
         改为 \"{PRESET_FAMILY}\"（OFL-1.1 修改版义务）。再生成：\
         scripts/gen-font-subset.py。"
    );
    if let Some(e) = extra {
        notice.push_str(&format!("\n本次构建注记：{e}。"));
    }
    EmbeddedFont {
        family: PRESET_FAMILY.to_string(),
        filename: PRESET_FILENAME.to_string(),
        data: PRESET_SUBSET.to_vec(),
        license: Some(FontLicense {
            ofl_text: OFL_TEXT.to_string(),
            notice,
        }),
    }
}

/// 书内全部字符（章节标题 + 正文文本 + 目录标题；宁多勿缺）。
fn book_charset(book: &BookDom) -> String {
    let mut set = BTreeSet::new();
    for ch in &book.chapters {
        set.extend(ch.title.chars());
        set.extend(ch.dom().root_element().text().collect::<String>().chars());
    }
    collect_toc_chars(&book.toc, &mut set);
    set.into_iter().collect()
}

fn collect_toc_chars(items: &[trans_epub_contract::ir::TocItem], set: &mut BTreeSet<char>) {
    for it in items {
        set.extend(it.title.chars());
        collect_toc_chars(&it.children, set);
    }
}

fn preset_charset() -> String {
    let mut set: BTreeSet<char> = HANZI_8105.chars().filter(|c| !c.is_whitespace()).collect();
    set.extend((0x20..0x7f).map(char::from));
    set.extend(EXTRA_CHARS.chars());
    set.into_iter().collect()
}

fn builtin_exact(
    book: &BookDom,
    ctx: &PassCtx<'_>,
) -> Result<EmbeddedFont, trans_epub_contract::error::Warning> {
    use sha2::{Digest, Sha256};
    use trans_epub_contract::error::Warning;

    // 缓存命中免下载
    let cache = std::env::temp_dir().join("trans-epub-fonts");
    let cache_file = cache.join(format!("{PINNED_ZIP_SHA256}.ttf"));
    let full: Vec<u8> = match std::fs::read(&cache_file) {
        Ok(bytes) => bytes,
        Err(_) => {
            let url: url::Url = PINNED_ZIP_URL.parse().map_err(|_| {
                Warning::new(
                    WarningCode::StructureAmbiguous,
                    None,
                    "font: invalid pinned url",
                )
            })?;
            // 钉版下载是用户点名的单资源 → fetch_direct（契约默认实现
            // = fetch；PoliteFetcher 覆写为跳过 robots——github.com 的
            // robots 禁 release 资产路径，2026-10 exact 模式实测被误拒）。
            // 限额提示 256MiB：全家桶 zip ~140MB，远超常规资产限额但
            // 有 sha256 钉版兜底。
            let page = ctx
                .fetcher
                .fetch_direct(&url, Some(256 * 1024 * 1024))
                .map_err(|e| {
                    Warning::new(
                        WarningCode::StructureAmbiguous,
                        None,
                        format!("font download failed: {e}"),
                    )
                })?;
            let mut hasher = Sha256::new();
            hasher.update(&page.body);
            let got: String = hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if got != PINNED_ZIP_SHA256 {
                return Err(Warning::new(
                    WarningCode::StructureAmbiguous,
                    None,
                    format!("font zip sha256 mismatch: {got}"),
                ));
            }
            let ttf = extract_regular(&page.body).ok_or_else(|| {
                Warning::new(
                    WarningCode::StructureAmbiguous,
                    None,
                    "font zip has no Regular ttf",
                )
            })?;
            let _ = std::fs::create_dir_all(&cache);
            let _ = std::fs::write(&cache_file, &ttf);
            ttf
        }
    };

    let chars = book_charset(book);
    let subset = ttf::subset_to_chars(&full, &chars)
        .ok_or_else(|| Warning::new(WarningCode::StructureAmbiguous, None, "font subset failed"))?;
    let renamed = ttf::rename_family(
        &subset,
        PRESET_FAMILY,
        &format!("{PRESET_FAMILY} Regular"),
        "TransEpubMono-Regular",
        "Subsetted to the characters of this book by trans-epub; original: Maple Mono CN (OFL-1.1).",
    )
    .ok_or_else(|| Warning::new(WarningCode::StructureAmbiguous, None, "font rename failed"))?;
    Ok(EmbeddedFont {
        family: PRESET_FAMILY.to_string(),
        filename: "TransEpubMono-Regular.exact.ttf".to_string(),
        data: renamed,
        license: Some(FontLicense {
            ofl_text: OFL_TEXT.to_string(),
            notice: format!(
                "字体为 Maple Mono CN Regular（release v7.9，zip sha256 {PINNED_ZIP_SHA256}）\
                 按本书实际字符精确子集，族名改为 \"{PRESET_FAMILY}\"（OFL-1.1 修改版义务）。"
            ),
        }),
    })
}

/// 从全家桶 zip 解出 Regular（zip crate 读侧，deflate feature 已启用）。
fn extract_regular(zip_bytes: &[u8]) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).ok()?;
    let mut entry = zip.by_name("MapleMono-CN-Regular.ttf").ok()?;
    let mut out = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut out).ok()?;
    Some(out)
}

fn user_font(
    book: &mut BookDom,
    data: Option<&[u8]>,
    family: Option<String>,
    subset: FontSubsetMode,
) -> Option<EmbeddedFont> {
    let Some(data) = data else {
        book.warn(trans_epub_contract::error::Warning::new(
            WarningCode::StructureAmbiguous,
            None,
            "fonts.source = user 但无 data（path 未被壳层解析），跳过字体内嵌",
        ));
        return None;
    };
    let chars = match subset {
        FontSubsetMode::Preset => preset_charset(),
        FontSubsetMode::Exact => book_charset(book),
    };
    let family = family
        .or_else(|| ttf::family_name(data))
        .unwrap_or_else(|| "UserFont".to_string());
    let sub = match ttf::subset_to_chars(data, &chars) {
        Some(s) => s,
        None => {
            book.warn(trans_epub_contract::error::Warning::new(
                WarningCode::StructureAmbiguous,
                None,
                "用户字体子集化失败，跳过字体内嵌",
            ));
            return None;
        }
    };
    let filename = format!(
        "{}-subset.ttf",
        family
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
    );
    Some(EmbeddedFont {
        family,
        filename,
        data: sub,
        license: None,
    })
}
