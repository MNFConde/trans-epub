//! EPUB3 写出器（docs/architecture/50-writer.md）：BookDraft 进，EPUB3
//! 字节出。只做组装，不改内容。
//!
//! 确定性（20-core.md）：章节文件名按 spine 序；uid 由书名 + 源集与内容
//! 哈希派生；zip 全部条目时间戳归一；central directory 的 host system 字节
//! 归一为 Unix(3)（zip crate 按编译目标写 Dos/Unix，公开 API 不可配）；
//! `dcterms:modified` 用固定常量——同输入字节级相同（跨平台亦然），golden
//! 直接 diff。
//!
//! 基座：直接用 zip crate（epub-builder 不暴露时间戳控制，无法满足
//! 确定性；50-writer.md 的「patch 或换 zip 层」取后者）。

use std::collections::BTreeMap;
use std::io::Write;

use sha2::{Digest, Sha256};
use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::error::BuildError;
use trans_epub_contract::ir::{AssetData, BookDraft, ChapterId, EmbeddedFont, SourceKey, TocItem};
use trans_epub_contract::writer::Writer;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::footnotes;
use super::paths::{self, AssignedAsset};
use crate::pipeline::highlight;

const BASE_CSS: &str = include_str!("styles/base.css");
const ADMONITION_CSS: &str = include_str!("styles/admonition.css");

/// `dcterms:modified` 固定常量：core 不读时钟（ADR-0004），确定性要求
/// 同输入同输出。epubcheck 只校验 ISO 8601 格式。
const FIXED_MODIFIED: &str = "2000-01-01T00:00:00Z";

/// zip 条目统一时间戳（DOS epoch 起点 1980-01-01 00:00:00）。
fn fixed_zip_time() -> zip::DateTime {
    zip::DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0)
        .expect("1980-01-01 00:00:00 is a valid DOS timestamp")
}

/// central directory 的 version-made-by host system 字节归一为 Unix(3)。
///
/// zip crate 按编译目标写 Dos(windows)/Unix(其余)（其 `write.rs` 的
/// `SYSTEM_BYTE`，`cfg!` 编译期决定、公开 API 不可配）——不归一则 Windows
/// 产物与其余平台必不相同。嵌入文本的 LF 行尾由仓库 `.gitattributes` 保证。
///
/// 定位走 EOCD → central directory 起点按条目结构前移，不对数据区做签名
/// 扫描（压缩流里可能碰巧出现 `PK\x01\x02`，误改会损坏产物）。
fn normalize_zip_system_byte(buf: &mut [u8]) -> Result<(), BuildError> {
    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const CD_SIG: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
    const UNIX: u8 = 3;

    let fatal = |message: String| BuildError::Fatal {
        code: "zip",
        message,
    };
    let eocd = buf
        .windows(4)
        .rposition(|w| w == EOCD_SIG)
        .ok_or_else(|| fatal("zip: EOCD not found".into()))?;
    let tail: [u8; 22] = buf
        .get(eocd..eocd + 22)
        .ok_or_else(|| fatal("zip: EOCD 截断".into()))?
        .try_into()
        .expect("22 字节 EOCD 固定段");
    let comment_len = u16::from_le_bytes([tail[20], tail[21]]) as usize;
    if eocd + 22 + comment_len != buf.len() {
        return Err(fatal(
            "zip: EOCD 位置与注释长度不一致（疑似签名误配）".into(),
        ));
    }
    let cd_size = u32::from_le_bytes(buf[eocd + 12..eocd + 16].try_into().unwrap()) as usize;
    let cd_off = u32::from_le_bytes(buf[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    if cd_off == u32::MAX as usize || cd_size == u32::MAX as usize {
        return Err(fatal(
            "zip: zip64 central directory 不支持（产物不应达到该规模）".into(),
        ));
    }

    let mut p = cd_off;
    while p < cd_off + cd_size {
        let head: [u8; 46] = buf
            .get(p..p + 46)
            .ok_or_else(|| fatal("zip: central directory 截断".into()))?
            .try_into()
            .expect("46 字节 central directory 固定头");
        if head[..4] != CD_SIG {
            return Err(fatal(format!("zip: central directory 条目签名不符 @ {p}")));
        }
        buf[p + 5] = UNIX;
        let nlen = u16::from_le_bytes(head[28..30].try_into().unwrap()) as usize;
        let elen = u16::from_le_bytes(head[30..32].try_into().unwrap()) as usize;
        let clen = u16::from_le_bytes(head[32..34].try_into().unwrap()) as usize;
        p += 46 + nlen + elen + clen;
    }
    Ok(())
}

pub struct Epub3Writer;

impl Writer for Epub3Writer {
    fn format(&self) -> &'static str {
        "epub3"
    }

    fn write(&self, book: &BookDraft, config: &ResolvedConfig) -> Result<Vec<u8>, BuildError> {
        let assigned = paths::assign_assets(&book.assets);
        let uid = derive_uid(book);
        let opf = render_opf(book, config, &assigned, &uid);
        let cover_page = book.meta.cover.as_ref().and_then(|k| assigned.get(k));
        let nav = render_nav(book, cover_page.is_some());
        let code_css = render_code_css(config, &book.fonts);

        let mut buf: Vec<u8> = Vec::new();
        {
            let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buf));
            let mut add = |name: &str, bytes: &[u8], stored: bool| -> Result<(), BuildError> {
                // 全部条目时间戳固定（确定性）；mimetype 另需 STORED 不压缩
                let mut options = SimpleFileOptions::default().last_modified_time(fixed_zip_time());
                if stored {
                    options = options.compression_method(CompressionMethod::Stored);
                } else {
                    options = options.compression_method(CompressionMethod::Deflated);
                }
                zip.start_file(name, options)
                    .map_err(|e| BuildError::Fatal {
                        code: "zip",
                        message: format!("writing {name}: {e}"),
                    })?;
                zip.write_all(bytes).map_err(|e| BuildError::Fatal {
                    code: "zip",
                    message: format!("writing {name}: {e}"),
                })
            };

            // mimetype 必须是首个条目且不压缩（EPUB 规范）
            add("mimetype", b"application/epub+zip", true)?;
            add("META-INF/container.xml", CONTAINER_XML.as_bytes(), false)?;
            add(&paths::opf_zip_path(), opf.as_bytes(), false)?;
            add(&paths::nav_zip_path(), nav.as_bytes(), false)?;

            // 封面页（可选；cover-image 属性在 opf 里）
            if let Some(cover_asset) = cover_page {
                let page = render_cover_page(&cover_asset.href);
                add(&paths::cover_zip_path(), page.as_bytes(), false)?;
            }

            // 章节（脚注先做 EPUB3 语义化：comrak 形态 → noteref/footnote/backlink）
            for chapter in &book.chapters {
                let page = render_chapter_page(
                    &chapter.title,
                    &footnotes::semanticize(&chapter.html),
                    &book.meta.language,
                    &config.theme.extra_css,
                );
                add(&paths::chapter_zip_path(chapter.id), page.as_bytes(), false)?;
            }

            // 资产（只嵌就绪的；顺序 = BTreeMap 键序，确定）
            for (zip_path, bytes) in embedded_assets(book, &assigned) {
                add(&zip_path, &bytes, false)?;
            }

            // 内嵌字体（fonts pass 产出）与 OFL 义务页（spine 末尾）
            for font in &book.fonts {
                add(&paths::font_zip_path(&font.filename), &font.data, false)?;
            }
            if let Some(page) = font_license_page(book) {
                add(&paths::font_license_zip_path(), page.as_bytes(), false)?;
            }

            // 样式表
            add(
                &paths::style_zip_path("base.css"),
                BASE_CSS.as_bytes(),
                false,
            )?;
            add(
                &paths::style_zip_path("code.css"),
                code_css.as_bytes(),
                false,
            )?;
            add(
                &paths::style_zip_path("admonition.css"),
                ADMONITION_CSS.as_bytes(),
                false,
            )?;
            if !config.theme.extra_css.trim().is_empty() {
                add(
                    &paths::style_zip_path("extra.css"),
                    config.theme.extra_css.as_bytes(),
                    false,
                )?;
            }

            zip.finish().map_err(|e| BuildError::Fatal {
                code: "zip",
                message: format!("finishing zip: {e}"),
            })?;
        }
        normalize_zip_system_byte(&mut buf)?;
        Ok(buf)
    }
}

/// 就绪资产 → (zip 路径, 字节)，顺序与 assign_assets 一致。
fn embedded_assets(
    book: &BookDraft,
    assigned: &BTreeMap<SourceKey, AssignedAsset>,
) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for (key, asset) in &book.assets {
        let AssetData::Owned(bytes) = &asset.data else {
            continue;
        };
        if let Some(assigned_asset) = assigned.get(key) {
            out.push((assigned_asset.zip_path.clone(), bytes.clone()));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// content.opf
// ---------------------------------------------------------------------------

const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

fn render_opf(
    book: &BookDraft,
    config: &ResolvedConfig,
    assigned: &BTreeMap<SourceKey, AssignedAsset>,
    uid: &str,
) -> String {
    let mut manifest = String::new();
    let mut spine = String::new();
    let mut extra_meta = String::new();

    manifest.push_str(
        "    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n",
    );
    manifest
        .push_str("    <item id=\"css-base\" href=\"Styles/base.css\" media-type=\"text/css\"/>\n");
    manifest
        .push_str("    <item id=\"css-code\" href=\"Styles/code.css\" media-type=\"text/css\"/>\n");
    manifest.push_str(
        "    <item id=\"css-admonition\" href=\"Styles/admonition.css\" media-type=\"text/css\"/>\n",
    );
    if !config.theme.extra_css.trim().is_empty() {
        manifest.push_str(
            "    <item id=\"css-extra\" href=\"Styles/extra.css\" media-type=\"text/css\"/>\n",
        );
    }

    // 封面：图片带 cover-image 属性 + 独立封面页。封面页用 linear（默认
    // yes）：作为阅读流首页；用 linear="no" 会被 epubcheck 判
    // OPF-096（non-linear 内容必须有超链接可达）。
    if let Some(cover_key) = &book.meta.cover {
        if let Some(asset) = assigned.get(cover_key) {
            let id = manifest_id_for(assigned, cover_key);
            manifest.push_str(&format!(
                "    <item id=\"{id}\" href=\"{}\" media-type=\"{}\" properties=\"cover-image\"/>\n",
                opf_href(&asset.zip_path),
                asset.media_type
            ));
            manifest.push_str(
                "    <item id=\"cover-page\" href=\"Text/cover.xhtml\" media-type=\"application/xhtml+xml\"/>\n",
            );
            extra_meta.push_str(&format!("    <meta name=\"cover\" content=\"{id}\"/>\n"));
            spine.push_str("    <itemref idref=\"cover-page\"/>\n");
        }
    }

    for chapter in &book.chapters {
        let id = chapter_item_id(chapter.id);
        manifest.push_str(&format!(
            "    <item id=\"{id}\" href=\"Text/{}\" media-type=\"application/xhtml+xml\"/>\n",
            paths::chapter_file_name(chapter.id)
        ));
        spine.push_str(&format!("    <itemref idref=\"{id}\"/>\n"));
    }

    // 其余资产（封面已登记的跳过）
    for key in assigned.keys() {
        if book.meta.cover.as_ref() == Some(key) {
            continue;
        }
        let asset = &assigned[key];
        let id = manifest_id_for(assigned, key);
        manifest.push_str(&format!(
            "    <item id=\"{id}\" href=\"{}\" media-type=\"{}\"/>\n",
            opf_href(&asset.zip_path),
            asset.media_type
        ));
    }

    // 内嵌字体 + OFL 义务页（spine 末尾 linear；epubcheck 对 non-linear
    // 要求超链接可达，义务页按普通内容页处理最稳）
    for (i, font) in book.fonts.iter().enumerate() {
        manifest.push_str(&format!(
            "    <item id=\"font{i:02}\" href=\"Fonts/{}\" media-type=\"application/font-sfnt\"/>\n",
            opf_href(&paths::font_zip_path(&font.filename))
        ));
    }
    if book.fonts.iter().any(|f| f.license.is_some()) {
        manifest.push_str(
            "    <item id=\"font-license\" href=\"Text/font-license.xhtml\" media-type=\"application/xhtml+xml\"/>\n",
        );
        spine.push_str("    <itemref idref=\"font-license\"/>\n");
    }

    let mut creators = String::new();
    for author in &book.meta.authors {
        creators.push_str(&format!(
            "    <dc:creator>{}</dc:creator>\n",
            xml_escape(author)
        ));
    }
    let description = book
        .meta
        .description
        .as_ref()
        .map(|d| format!("    <dc:description>{}</dc:description>\n", xml_escape(d)))
        .unwrap_or_default();

    // OPF guide（EPUB2 legacy；epubcheck 5.2.1 实证 EPUB 3.3 包内合法）：
    // MuPDF 类老引擎的快跳转入口。toc 恒在；cover/text 随条件。
    let has_cover_page = book
        .meta
        .cover
        .as_ref()
        .and_then(|k| assigned.get(k))
        .is_some();
    let guide = format!(
        "  <guide>\n{}  </guide>\n",
        render_guide_items(has_cover_page, first_chapter_id(book))
    );

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid" xml:lang="{lang}">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:uuid:{uid}</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:language>{lang}</dc:language>
{creators}{description}{extra_meta}    <meta property="dcterms:modified">{modified}</meta>
  </metadata>
  <manifest>
{manifest}  </manifest>
  <spine>
{spine}  </spine>
{guide}</package>
"#,
        lang = xml_escape(&book.meta.language),
        uid = uid,
        title = xml_escape(&book.meta.title),
        creators = creators,
        description = description,
        extra_meta = extra_meta,
        modified = FIXED_MODIFIED,
        manifest = manifest,
        spine = spine,
        guide = guide,
    )
}

/// spine 首章 id（Begin Reading / guide text 的锚）。
fn first_chapter_id(book: &BookDraft) -> Option<ChapterId> {
    book.chapters.first().map(|c| c.id)
}

/// 快跳入口的单点决策（landmarks 与 guide 共用——dryer 0.92 对的收敛点，
/// 「有哪些入口、href 是什么」只活在这里）：cover 页（有封面时）与 spine
/// 首章。kind 用 landmarks 的 epub:type 词汇；guide 侧 bodymatter→text
/// 的类型名映射留在其渲染器。
fn quick_jump_targets(
    has_cover_page: bool,
    first_chapter: Option<ChapterId>,
) -> Vec<(&'static str, String, &'static str)> {
    let mut targets = Vec::new();
    if has_cover_page {
        targets.push(("cover", "Text/cover.xhtml".to_string(), "Cover"));
    }
    if let Some(id) = first_chapter {
        targets.push((
            "bodymatter",
            paths::chapter_href_from_nav(id),
            "Begin Reading",
        ));
    }
    targets
}

/// OPF guide reference 条目（EPUB2 legacy，epubcheck 5.2.1 实证 3.3 包内
/// 合法）；toc 恒在最前（与 nav.xhtml 的 toc-nav-then-landmarks 结构呼应），
/// 其后按公共决策序。标签固定英文串（不承担 i18n）。
fn render_guide_items(has_cover_page: bool, first_chapter: Option<ChapterId>) -> String {
    let mut items = String::new();
    items
        .push_str("    <reference type=\"toc\" title=\"Table of Contents\" href=\"nav.xhtml\"/>\n");
    for (kind, href, label) in quick_jump_targets(has_cover_page, first_chapter) {
        let guide_type = if kind == "cover" { "cover" } else { "text" };
        items.push_str(&format!(
            "    <reference type=\"{guide_type}\" title=\"{label}\" href=\"{href}\"/>\n"
        ));
    }
    items
}

/// manifest item id：按 assigned 键序编号（确定性）。
fn manifest_id_for(assigned: &BTreeMap<SourceKey, AssignedAsset>, key: &SourceKey) -> String {
    let index = assigned.keys().position(|k| k == key).unwrap_or_default();
    format!("ast{index:04}")
}

fn chapter_item_id(id: ChapterId) -> String {
    paths::chapter_file_name(id)
        .trim_end_matches(".xhtml")
        .to_string()
}

/// zip 路径 → OPF 内 href（去掉 `OEBPS/` 前缀）。
fn opf_href(zip_path: &str) -> String {
    zip_path
        .strip_prefix("OEBPS/")
        .unwrap_or(zip_path)
        .to_string()
}

// ---------------------------------------------------------------------------
// nav.xhtml
// ---------------------------------------------------------------------------

fn render_nav(book: &BookDraft, has_cover_page: bool) -> String {
    let mut list = String::new();
    render_nav_items(&book.toc, &mut list);
    let landmarks = render_landmarks_items(has_cover_page, first_chapter_id(book));
    // 条目为零时整块不产出（空 nav 无用）；有封面或章节才出 landmarks
    let landmarks_block = if landmarks.is_empty() {
        String::new()
    } else {
        format!(
            "  <nav epub:type=\"landmarks\" id=\"landmarks\" hidden=\"hidden\">\n    \
             <h2>Landmarks</h2>\n    <ol>\n{landmarks}    </ol>\n  </nav>\n"
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="{lang}" xml:lang="{lang}">
<head>
  <title>{title}</title>
</head>
<body>
  <nav epub:type="toc" id="toc">
    <h1>{title}</h1>
    <ol>
{list}    </ol>
  </nav>
{landmarks_block}</body>
</html>
"#,
        title = xml_escape(&book.meta.title),
        lang = xml_escape(&book.meta.language),
        list = list,
        landmarks_block = landmarks_block,
    )
}

/// landmarks nav 条目（hidden nav，nav 不进 spine——对 `#toc` 的片段
/// 自引用被 epubcheck RSC-011 拒绝，故只列 cover 与首章）。
fn render_landmarks_items(has_cover_page: bool, first_chapter: Option<ChapterId>) -> String {
    let mut items = String::new();
    for (kind, href, label) in quick_jump_targets(has_cover_page, first_chapter) {
        items.push_str(&format!(
            "      <li><a epub:type=\"{kind}\" href=\"{href}\">{label}</a></li>\n"
        ));
    }
    items
}

fn render_nav_items(items: &[TocItem], out: &mut String) {
    for item in items {
        if item.children.is_empty() {
            match item.chapter {
                Some(id) => out.push_str(&format!(
                    "      <li><a href=\"{}\">{}</a></li>\n",
                    paths::chapter_href_from_nav(id),
                    xml_escape(&item.title)
                )),
                None => out.push_str(&format!(
                    "      <li><span>{}</span></li>\n",
                    xml_escape(&item.title)
                )),
            }
            continue;
        }
        let mut nested = String::new();
        render_nav_items(&item.children, &mut nested);
        match item.chapter {
            // 分组节点自带章节（README 上提）：a 之后再挂嵌套列表
            Some(id) => out.push_str(&format!(
                "      <li><a href=\"{}\">{}</a>\n      <ol>\n{}      </ol>\n      </li>\n",
                paths::chapter_href_from_nav(id),
                xml_escape(&item.title),
                nested
            )),
            None => out.push_str(&format!(
                "      <li><span>{}</span>\n      <ol>\n{}      </ol>\n      </li>\n",
                xml_escape(&item.title),
                nested
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// 章节页 / 封面页
// ---------------------------------------------------------------------------

fn render_chapter_page(title: &str, body_html: &str, language: &str, extra_css: &str) -> String {
    let extra = if extra_css.trim().is_empty() {
        String::new()
    } else {
        format!(
            "  <link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>\n",
            paths::style_href("extra.css")
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="{lang}" xml:lang="{lang}">
<head>
  <title>{title}</title>
  <link rel="stylesheet" type="text/css" href="{base}"/>
  <link rel="stylesheet" type="text/css" href="{code}"/>
  <link rel="stylesheet" type="text/css" href="{admonition}"/>
{extra}</head>
<body>
{body}
</body>
</html>
"#,
        title = xml_escape(title),
        lang = xml_escape(language),
        base = paths::style_href("base.css"),
        code = paths::style_href("code.css"),
        admonition = paths::style_href("admonition.css"),
        extra = extra,
        body = body_html,
    )
}

fn render_cover_page(cover_href: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
  <title>Cover</title>
</head>
<body style="margin:0;padding:0;text-align:center;">
  <img src="{href}" alt="Cover" style="max-width:100%;max-height:100%;"/>
</body>
</html>
"#,
        href = xml_escape(cover_href),
    )
}

// ---------------------------------------------------------------------------
// code.css（含 syntect 主题配色 + 内嵌字体 @font-face）
// ---------------------------------------------------------------------------

/// 无内嵌字体时的代码字体链：具体名优先（MuPDF 等老栈不解析泛型族回退
/// Times，MVP1 真机观察 3）；有内嵌字体时在链首插入其族名。
const CODE_FONT_FALLBACK: &str =
    "\"Maple Mono NF CN\", \"Cascadia Code\", Consolas, \"Noto Sans Mono CJK SC\", monospace";

fn render_code_css(config: &ResolvedConfig, fonts: &[EmbeddedFont]) -> String {
    let light_name = config.highlight.light_theme.as_str();
    let dark_name = config.highlight.dark_theme.as_str();
    let light = highlight::theme_css(light_name)
        .or_else(|| highlight::theme_css("InspiredGitHub"))
        .unwrap_or_default();
    let dark = highlight::theme_css(dark_name)
        .or_else(|| highlight::theme_css("base16-ocean.dark"))
        .unwrap_or_default();

    let mut font_faces = String::new();
    let mut families: Vec<String> = Vec::new();
    for font in fonts {
        if !families.iter().any(|f| f == &font.family) {
            families.push(font.family.clone());
        }
        font_faces.push_str(&format!(
            "@font-face {{\n  font-family: \"{}\";\n  src: url(../Fonts/{});\n  font-weight: normal;\n  font-style: normal;\n}}\n",
            xml_escape(&font.family),
            font.filename
        ));
    }
    let family_chain = if families.is_empty() {
        CODE_FONT_FALLBACK.to_string()
    } else {
        let quoted: Vec<String> = families.iter().map(|f| format!("\"{f}\"")).collect();
        format!("{}, {CODE_FONT_FALLBACK}", quoted.join(", "))
    };
    let font_rules =
        format!("pre, code, kbd, samp {{ font-family: {family_chain}; }}\n{font_faces}");

    format!(
        "/* trans-epub code.css —— 代码块基线 + syntect 主题配色 */\n\
         /* 默认 pre-wrap 按屏宽自适应折行（ADR-0007）；硬折行归 code-wrap pass */\n\
         pre {{ white-space: pre-wrap; word-break: break-word; }}\n\
         pre code {{ background: transparent; padding: 0; }}\n\
         {font_rules}\n\
         /* 亮色主题（{light_name}） */\n{light}\n\n\
         @media (prefers-color-scheme: dark) {{\n{dark}}}\n",
        light_name = light_name,
        light = light,
        dark = dark,
    )
}

// ---------------------------------------------------------------------------
// 字体许可页（OFL-1.1 义务随书）
// ---------------------------------------------------------------------------

/// 有带许可的字体才出页；多个字体合一页（当前实际只 builtin 一种）。
fn font_license_page(book: &BookDraft) -> Option<String> {
    let licensed: Vec<&EmbeddedFont> = book.fonts.iter().filter(|f| f.license.is_some()).collect();
    if licensed.is_empty() {
        return None;
    }
    let mut sections = String::new();
    for font in &licensed {
        let license = font.license.as_ref().expect("filtered");
        sections.push_str(&format!(
            "<section><h2>{}（修改声明与溯源）</h2><p>{}</p></section>\n",
            xml_escape(&font.family),
            xml_escape(&license.notice)
        ));
    }
    // OFL 全文取首个字体的（builtin 场景全文一致）
    let ofl = licensed[0]
        .license
        .as_ref()
        .expect("filtered")
        .ofl_text
        .clone();
    Some(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head>
  <title>字体许可</title>
  <link rel="stylesheet" type="text/css" href="../Styles/base.css"/>
</head>
<body>
 <h1>字体许可</h1>
{sections}<section><h2>SIL Open Font License 1.1</h2>
<pre xml:space="preserve">{ofl}</pre></section>
</body>
</html>
"#,
        sections = sections,
        ofl = xml_escape(&ofl),
    ))
}

// ---------------------------------------------------------------------------
// uid 派生与 XML 转义
// ---------------------------------------------------------------------------

/// uid = SHA-256(书名、语言、逐章 source+标题+HTML、逐资产键+内容哈希)，
/// 截取为 8-4-4-4-12 十六进制。同输入同 uid（确定性，20-core.md）。
fn derive_uid(book: &BookDraft) -> String {
    let mut hasher = Sha256::new();
    hasher.update(book.meta.title.as_bytes());
    hasher.update([0]);
    hasher.update(book.meta.language.as_bytes());
    hasher.update([0]);
    for chapter in &book.chapters {
        hasher.update(*b"C");
        hasher.update(chapter.source.to_string().as_bytes());
        hasher.update([0]);
        hasher.update(chapter.title.as_bytes());
        hasher.update([0]);
        hasher.update(chapter.html.as_bytes());
        hasher.update([0]);
    }
    for (key, asset) in &book.assets {
        hasher.update(*b"A");
        hasher.update(key.to_string().as_bytes());
        hasher.update([0]);
        match &asset.data {
            AssetData::Owned(bytes) => {
                hasher.update((bytes.len() as u64).to_le_bytes());
                hasher.update(Sha256::digest(bytes));
            }
            _ => hasher.update([0]),
        }
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use zip::ZipArchive;

    /// 构造一个多条目 zip（含 STORED 首条目，模拟 EPUB 形态）。
    fn sample_zip() -> Vec<u8> {
        let mut buf = Vec::new();
        let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buf));
        for (name, stored) in [
            ("mimetype", true),
            ("META-INF/container.xml", false),
            ("OEBPS/text.xhtml", false),
        ] {
            let mut options = SimpleFileOptions::default().last_modified_time(fixed_zip_time());
            if stored {
                options = options.compression_method(CompressionMethod::Stored);
            }
            zip.start_file(name, options).expect("start_file");
            zip.write_all(format!("content of {name}").as_bytes())
                .expect("write");
        }
        zip.finish().expect("finish");
        buf
    }

    /// central directory 逐条目的 (name, host system 字节)。
    /// 测试数据小，从 EOCD 定位，走与生产一致的解析路径。
    fn central_entries(buf: &[u8]) -> Vec<(String, u8)> {
        let eocd = buf
            .windows(4)
            .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])
            .expect("eocd");
        let cd_size = u32::from_le_bytes(buf[eocd + 12..eocd + 16].try_into().unwrap()) as usize;
        let cd_off = u32::from_le_bytes(buf[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        let mut out = Vec::new();
        let mut p = cd_off;
        while p < cd_off + cd_size {
            let nlen = u16::from_le_bytes(buf[p + 28..p + 30].try_into().unwrap()) as usize;
            let elen = u16::from_le_bytes(buf[p + 30..p + 32].try_into().unwrap()) as usize;
            let clen = u16::from_le_bytes(buf[p + 32..p + 34].try_into().unwrap()) as usize;
            out.push((
                String::from_utf8_lossy(&buf[p + 46..p + 46 + nlen]).into_owned(),
                buf[p + 5],
            ));
            p += 46 + nlen + elen + clen;
        }
        out
    }

    #[test]
    fn zip_system_byte_normalized_to_unix() {
        let mut buf = sample_zip();
        let before: Vec<u8> = central_entries(&buf).iter().map(|e| e.1).collect();
        assert!(
            before.iter().all(|&b| b == before[0]),
            "同平台一次写出的 system 字节应一致，实际 {before:?}"
        );

        normalize_zip_system_byte(&mut buf).expect("normalize");

        let entries = central_entries(&buf);
        assert_eq!(entries.len(), 3);
        assert!(
            entries.iter().all(|(_, sys)| *sys == 3),
            "归一后应全为 Unix(3)，实际 {entries:?}"
        );

        // 归一只动 central directory 的 system 字节，zip 仍可读、内容不损
        let mut archive = ZipArchive::new(std::io::Cursor::new(&buf)).expect("open");
        let mut s = String::new();
        archive
            .by_name("OEBPS/text.xhtml")
            .expect("entry")
            .read_to_string(&mut s)
            .expect("read");
        assert_eq!(s, "content of OEBPS/text.xhtml");
        assert_eq!(
            archive.by_name("mimetype").expect("entry").compression(),
            zip::CompressionMethod::Stored
        );
    }

    #[test]
    fn zip_system_byte_rejects_truncated_central_directory() {
        let mut buf = sample_zip();
        let len = buf.len();
        buf.truncate(len - 8); // 掐掉 EOCD 尾部，结构破损必须报错而非 panic
        assert!(normalize_zip_system_byte(&mut buf).is_err());
    }

    #[test]
    fn landmarks_items_pin_cover_and_bodymatter_without_toc_selfref() {
        let both = render_landmarks_items(true, Some(ChapterId(0)));
        assert!(
            both.contains("<a epub:type=\"cover\" href=\"Text/cover.xhtml\">Cover</a>"),
            "{both}"
        );
        assert!(
            both.contains(
                "<a epub:type=\"bodymatter\" href=\"Text/ch00001.xhtml\">Begin Reading</a>"
            ),
            "{both}"
        );
        assert!(!both.contains("href=\"#toc\""), "toc 自引用禁入：{both}");

        let no_cover = render_landmarks_items(false, Some(ChapterId(0)));
        assert!(!no_cover.contains("cover\""), "{no_cover}");
        assert!(no_cover.contains("bodymatter"), "{no_cover}");

        // 无封面无章节 → 空条目（render_nav 据此整块省略）
        assert_eq!(render_landmarks_items(false, None), "");
    }

    #[test]
    fn chapter_page_carries_language_for_hyphenation() {
        let page = render_chapter_page("T", "<p>x</p>", "zh-CN", "");
        assert!(
            page.contains(
                "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"zh-CN\" xml:lang=\"zh-CN\">"
            ),
            "{page}"
        );
    }

    #[test]
    fn guide_items_pin_three_references() {
        let both = render_guide_items(true, Some(ChapterId(0)));
        assert!(
            both.contains("<reference type=\"cover\" title=\"Cover\" href=\"Text/cover.xhtml\"/>"),
            "{both}"
        );
        assert!(
            both.contains(
                "<reference type=\"toc\" title=\"Table of Contents\" href=\"nav.xhtml\"/>"
            ),
            "{both}"
        );
        assert!(
            both.contains(
                "<reference type=\"text\" title=\"Begin Reading\" href=\"Text/ch00001.xhtml\"/>"
            ),
            "{both}"
        );
        // 顺序钉：toc 恒在最前，其后按公共决策序（cover、text）
        let toc = both.find("type=\"toc\"").expect("toc");
        let cover = both.find("type=\"cover\"").expect("cover");
        let text = both.find("type=\"text\"").expect("text");
        assert!(toc < cover && cover < text, "{both}");

        let bare = render_guide_items(false, None);
        assert!(bare.contains("type=\"toc\""), "toc 恒在：{bare}");
        assert!(!bare.contains("type=\"cover\""), "{bare}");
        assert!(!bare.contains("type=\"text\""), "{bare}");
    }
}
