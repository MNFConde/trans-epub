//! EPUB3 写出器（docs/architecture/50-writer.md）：BookDraft 进，EPUB3
//! 字节出。只做组装，不改内容。
//!
//! 确定性（20-core.md）：章节文件名按 spine 序；uid 由书名 + 源集与内容
//! 哈希派生；zip 全部条目时间戳归一；`dcterms:modified` 用固定常量——
//! 同输入字节级相同，golden 直接 diff。
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

pub struct Epub3Writer;

impl Writer for Epub3Writer {
    fn format(&self) -> &'static str {
        "epub3"
    }

    fn write(&self, book: &BookDraft, config: &ResolvedConfig) -> Result<Vec<u8>, BuildError> {
        let assigned = paths::assign_assets(&book.assets);
        let uid = derive_uid(book);
        let opf = render_opf(book, config, &assigned, &uid);
        let nav = render_nav(book);
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
            if let Some(cover_asset) = book.meta.cover.as_ref().and_then(|k| assigned.get(k)) {
                let page = render_cover_page(&cover_asset.href);
                add(&paths::cover_zip_path(), page.as_bytes(), false)?;
            }

            // 章节
            for chapter in &book.chapters {
                let page =
                    render_chapter_page(&chapter.title, &chapter.html, &config.theme.extra_css);
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
</package>
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
    )
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

fn render_nav(book: &BookDraft) -> String {
    let mut list = String::new();
    render_nav_items(&book.toc, &mut list);
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
  <title>{title}</title>
</head>
<body>
  <nav epub:type="toc" id="toc">
    <h1>{title}</h1>
    <ol>
{list}    </ol>
  </nav>
</body>
</html>
"#,
        title = xml_escape(&book.meta.title),
        list = list,
    )
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

fn render_chapter_page(title: &str, body_html: &str, extra_css: &str) -> String {
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
<html xmlns="http://www.w3.org/1999/xhtml">
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
