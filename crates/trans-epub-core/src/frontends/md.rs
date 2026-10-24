//! 散装 markdown 前端（MVP1）：单文件 / 目录 / 目录树，GFM 基线。
//!
//! 结构探测六规则（优先级自上而下，docs/architecture/30-frontends.md）：
//!
//! 1. 显式声明覆盖一切（`StructureCfg.chapters`，v1 平铺序）；
//! 2. 根下有 `book.toml` / `SUMMARY.md` / `src/SUMMARY.md` → 不命中（让位 mdbook）；
//! 3. front matter 的 `order` / `title` 参与排序与命名；
//! 4. 数字前缀 `NN-name.md` → 剥前缀排序、剥后名字做标题；
//! 5. `README.md` / `index.md` 上提为所在目录层级的首页；
//! 6. 目录树嵌套 → toc 嵌套；纯平铺 → 文件名自然排序。
//!
//! probe 只看目录形态与 front matter（不解析正文）；load 时 comrak 渲染、
//! heading id 补齐、资产登记（本地 Owned / 远程 Remote / data: Owned）。
//! 引用保持原始形式（义务清单第 3 条），改写归 link-rewrite。

use std::sync::OnceLock;

use comrak::Anchorizer;
use scraper::{Html, Selector};
use trans_epub_contract::config::{MarkdownCfg, ResolvedConfig, StructureCfg};
use trans_epub_contract::error::{BuildError, Warning, WarningCode};
use trans_epub_contract::frontend::{Frontend, LoadEnv, Outline, OutlineItem, Probe};
use trans_epub_contract::ir::{Asset, AssetData, BookDraft, ChapterId, SourceKey, TocItem};
use trans_epub_contract::source::{FileEntry, RelPath, Source, Vfs};

use crate::dom;
use crate::refs;

/// 探测期 front matter 的最大读取量（字节）——探测必须廉价。
const FM_PEEK_LIMIT: usize = 64 * 1024;

pub struct MdFrontend;

impl Frontend for MdFrontend {
    fn name(&self) -> &'static str {
        "md"
    }

    fn probe(&self, source: &Source, config: &ResolvedConfig) -> Probe {
        let Source::Vfs(arc) = source else {
            return Probe::NoMatch;
        };
        let vfs: &dyn Vfs = arc.as_ref();
        let Ok(entries) = vfs.walk() else {
            return Probe::NoMatch;
        };
        if yields_to_mdbook(&entries) {
            return Probe::NoMatch;
        }
        let Some((kind, root)) = detect(vfs, &entries, &config.structure) else {
            return Probe::NoMatch;
        };
        Probe::Matched {
            outline: Outline {
                kind,
                items: outline_items(&root, ""),
                warnings: root.warnings,
            },
        }
    }

    fn load(&self, source: &Source, env: &LoadEnv<'_>) -> Result<BookDraft, BuildError> {
        let Source::Vfs(arc) = source else {
            return Err(BuildError::Source(
                "md frontend requires a Vfs source".into(),
            ));
        };
        let vfs: &dyn Vfs = arc.as_ref();
        let entries = vfs
            .walk()
            .map_err(|e| BuildError::Source(format!("walk failed: {e}")))?;
        if yields_to_mdbook(&entries) {
            return Err(BuildError::Source(
                "looks like an mdBook project (book.toml / SUMMARY.md); \
                 mdbook frontend lands in MVP2"
                    .into(),
            ));
        }
        let Some((kind, mut root)) = detect(vfs, &entries, &env.config.structure) else {
            return Err(BuildError::Source("no markdown files found".into()));
        };

        let mut book = BookDraft::default();
        for w in std::mem::take(&mut root.warnings) {
            book.warn(w);
        }
        let total = root.count_chapters() as u64;
        let mut done: u64 = 0;
        book.toc = load_dir(vfs, env, &root, &mut done, total, &mut book)?;

        // 书名线索：单文件书的 FM / 根 README 的 FM
        let lead_fm = if kind == "md-file" {
            root.readme
                .as_ref()
                .or_else(|| root.files.first())
                .and_then(|c| c.fm.clone())
        } else {
            root.readme.as_ref().and_then(|c| c.fm.clone())
        };
        assemble_meta(&mut book, env.config, &root, kind, lead_fm);
        register_cover(vfs, env.config, &mut book);
        Ok(book)
    }
}

// ---------------------------------------------------------------------------
// 探测
// ---------------------------------------------------------------------------

/// 规则 2：识别出 mdBook 项目特征 → md 前端让位（注册顺序保证 mdbook 先探）。
fn yields_to_mdbook(entries: &[FileEntry]) -> bool {
    entries.iter().any(|e| {
        matches!(
            e.path.as_str(),
            "book.toml" | "SUMMARY.md" | "src/SUMMARY.md" | "src/book.toml"
        )
    })
}

/// front matter 的最小 schema（章节级：title / order）。
#[derive(Debug, Clone, Default)]
struct FrontMatter {
    title: Option<String>,
    order: Option<f64>,
}

struct DetectedChapter {
    path: RelPath,
    /// 文件名去 `.md` 扩展名。
    stem: String,
    /// 数字前缀剥离后的 stem。
    display_stem: String,
    numeric: Option<u64>,
    is_readme: bool,
    fm: Option<FrontMatter>,
}

impl DetectedChapter {
    /// 排序来源档 + 序值：front matter order=0、数字前缀=1、无序=2。
    fn sort_rank(&self) -> (u8, f64) {
        match (self.fm.as_ref().and_then(|f| f.order), self.numeric) {
            (Some(order), _) => (0, order),
            (None, Some(num)) => (1, num as f64),
            (None, None) => (2, 0.0),
        }
    }

    /// 章节标题：FM title > 前缀剥离 stem；README 无 FM 时用父目录名。
    fn title(&self, parent_dir: Option<&str>) -> String {
        if let Some(title) = self.fm.as_ref().and_then(|f| f.title.clone()) {
            return title;
        }
        if self.is_readme {
            if let Some(dir) = parent_dir {
                return prettify(dir);
            }
        }
        prettify(&self.display_stem)
    }

    fn source_key(&self) -> SourceKey {
        SourceKey::Path(self.path.clone())
    }
}

struct DetectedDir {
    /// 目录段名（根为空串）。
    name: String,
    files: Vec<DetectedChapter>,
    /// README.md / index.md（规则 5：上提为层级首页）。
    readme: Option<DetectedChapter>,
    subdirs: Vec<DetectedDir>,
    warnings: Vec<Warning>,
}

impl DetectedDir {
    fn count_chapters(&self) -> usize {
        self.files.len()
            + self.readme.is_some() as usize
            + self
                .subdirs
                .iter()
                .map(DetectedDir::count_chapters)
                .sum::<usize>()
    }

    /// 分组标题：README 的 FM title > 目录名美化。
    fn dir_title(&self) -> String {
        if let Some(title) = self
            .readme
            .as_ref()
            .and_then(|r| r.fm.as_ref())
            .and_then(|f| f.title.clone())
        {
            return title;
        }
        prettify(&self.name)
    }

    /// 取出（或按路径创建）子目录节点。
    fn subdir_mut(&mut self, dir: &str) -> &mut DetectedDir {
        let (head, rest) = match dir.split_once('/') {
            Some((h, r)) => (h, Some(r)),
            None => (dir, None),
        };
        if !self.subdirs.iter().any(|d| d.name == head) {
            self.subdirs.push(DetectedDir {
                name: head.to_string(),
                files: Vec::new(),
                readme: None,
                subdirs: Vec::new(),
                warnings: Vec::new(),
            });
        }
        let child = self
            .subdirs
            .iter_mut()
            .find(|d| d.name == head)
            .expect("just pushed");
        match rest {
            Some(r) => child.subdir_mut(r),
            None => child,
        }
    }

    fn warn(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }
}

/// 探测：`(kind, 目录树)`；`None` = 无可见 md 文件。
fn detect(
    vfs: &dyn Vfs,
    entries: &[FileEntry],
    structure: &StructureCfg,
) -> Option<(&'static str, DetectedDir)> {
    let mut root = DetectedDir {
        name: String::new(),
        files: Vec::new(),
        readme: None,
        subdirs: Vec::new(),
        warnings: Vec::new(),
    };

    // 规则 1：显式声明覆盖一切
    if !structure.chapters.is_empty() {
        let mut resolved = 0;
        for declared in &structure.chapters {
            let Ok(rel) = RelPath::new(&declared.path) else {
                root.warn(Warning::new(
                    WarningCode::StructureAmbiguous,
                    None,
                    format!("declared chapter has invalid path: {}", declared.path),
                ));
                continue;
            };
            let exists = entries.iter().any(|e| e.path == rel);
            let is_md = rel.file_name().to_lowercase().ends_with(".md");
            if !exists || !is_md {
                root.warn(Warning::new(
                    WarningCode::StructureAmbiguous,
                    None,
                    format!("declared chapter not found (or not .md): {}", declared.path),
                ));
                continue;
            }
            resolved += 1;
            let (fm, _) = peek_front_matter(vfs, &rel);
            let stem = stem_of(rel.file_name());
            let (display_stem, numeric) = split_numeric_prefix(&stem);
            let mut fm = fm.unwrap_or_default();
            if let Some(title) = &declared.title {
                fm.title = Some(title.clone());
            }
            root.files.push(DetectedChapter {
                path: rel,
                stem,
                display_stem,
                numeric,
                is_readme: false,
                fm: Some(fm),
            });
        }
        if resolved > 0 {
            sort_dir(&mut root);
            return Some(("md-declared", root));
        }
        root.warn(Warning::new(
            WarningCode::StructureAmbiguous,
            None,
            "no declared chapter resolved; falling back to structure detection",
        ));
    }

    let visible = visible_md_files(entries, structure);
    if visible.is_empty() {
        return None;
    }

    let mut md_count = 0usize;
    for entry in visible {
        md_count += 1;
        let name = entry.path.file_name().to_string();
        let stem = stem_of(&name);
        let (display_stem, numeric) = split_numeric_prefix(&stem);
        let lower = name.to_lowercase();
        let is_readme = lower == "readme.md" || lower == "index.md";
        let (fm, _) = peek_front_matter(vfs, &entry.path);
        // 歧义（规则 3 > 4）：既有 FM order 又有数字前缀 → FM 胜出并记 warning
        if fm.as_ref().and_then(|f| f.order).is_some() && numeric.is_some() {
            root.warn(Warning::new(
                WarningCode::StructureAmbiguous,
                Some(SourceKey::Path(entry.path.clone())),
                "both front matter order and numeric prefix; using front matter order",
            ));
        }
        let chapter = DetectedChapter {
            path: entry.path.clone(),
            stem,
            display_stem,
            numeric,
            is_readme,
            fm,
        };
        let dir = match entry.path.parent() {
            None => &mut root,
            Some(parent) => root.subdir_mut(parent.as_str()),
        };
        if chapter.is_readme && dir.readme.is_none() {
            dir.readme = Some(chapter);
        } else {
            dir.files.push(chapter);
        }
    }
    sort_dir(&mut root);

    let kind = if md_count == 1 && root.subdirs.is_empty() {
        "md-file"
    } else if root.subdirs.is_empty() {
        "md-flat"
    } else {
        "md-tree"
    };
    Some((kind, root))
}

/// 可见 md 文件：`.md` 扩展名（大小写不敏感）+ include/exclude + 点规则。
/// include 命中可捞回点开头文件；exclude 总是丢弃。
fn visible_md_files(entries: &[FileEntry], structure: &StructureCfg) -> Vec<FileEntry> {
    let include = compile_globs(&structure.include);
    let exclude = compile_globs(&structure.exclude);
    entries
        .iter()
        .filter(|e| e.path.file_name().to_lowercase().ends_with(".md"))
        .filter(|e| {
            let path = e.path.as_str();
            match &include {
                Some(inc) => inc.is_match(path),
                None => !path.split('/').any(|seg| seg.starts_with('.')),
            }
        })
        .filter(|e| match &exclude {
            Some(ex) => !ex.is_match(e.path.as_str()),
            None => true,
        })
        .cloned()
        .collect()
}

fn compile_globs(patterns: &[String]) -> Option<globset::GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for p in patterns {
        if let Ok(glob) = globset::Glob::new(p) {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

/// 层内排序：文件按（排序来源档, 序值, 自然序 stem）；子目录按自然序。
fn sort_dir(dir: &mut DetectedDir) {
    dir.files.sort_by(|a, b| {
        let (ra, rb) = (a.sort_rank(), b.sort_rank());
        ra.0.cmp(&rb.0)
            .then_with(|| ra.1.total_cmp(&rb.1))
            .then_with(|| refs::natural_cmp(&a.stem, &b.stem))
    });
    dir.subdirs
        .sort_by(|a, b| refs::natural_cmp(&a.name, &b.name));
    for sub in &mut dir.subdirs {
        sort_dir(sub);
    }
}

fn stem_of(file_name: &str) -> String {
    if let Some(stem) = file_name.strip_suffix(".md") {
        return stem.to_string();
    }
    if file_name.to_lowercase().ends_with(".md") {
        file_name[..file_name.len() - 3].to_string()
    } else {
        file_name.to_string()
    }
}

/// `NN-name.md` / `NN_name.md` / `NN.name.md` / `NN name.md` → (剥前缀名, N)。
/// 纯数字名（`42.md`）不算前缀。
fn split_numeric_prefix(stem: &str) -> (String, Option<u64>) {
    let digits = stem.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits == stem.len() {
        return (stem.to_string(), None);
    }
    let num = stem[..digits].parse::<u64>().ok();
    match stem[digits..].strip_prefix(['-', '_', '.', ' ']) {
        Some(display) if !display.is_empty() => (display.to_string(), num),
        _ => (stem.to_string(), None),
    }
}

fn prettify(stem: &str) -> String {
    stem.replace(['-', '_'], " ").trim().to_string()
}

/// 读文件解析 front matter；返回 (FM, 正文)。无 FM / 解析失败 → (None, 原文)。
fn peek_front_matter(vfs: &dyn Vfs, path: &RelPath) -> (Option<FrontMatter>, String) {
    let bytes = match vfs.read(path) {
        Ok(bytes) => bytes,
        Err(_) => return (None, String::new()),
    };
    let text = if bytes.len() > FM_PEEK_LIMIT {
        String::from_utf8_lossy(&bytes[..FM_PEEK_LIMIT]).into_owned()
    } else {
        match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }
    };
    split_front_matter(&text)
}

/// `---\n…\n---\n` → (Some(FM), 正文)；闭合行 `---` 或 `...`。
fn split_front_matter(raw: &str) -> (Option<FrontMatter>, String) {
    let mut lines = raw.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if first.trim_end() != "---" {
        return (None, raw.to_string());
    }
    let mut fm_lines: Vec<&str> = Vec::new();
    let mut consumed = first.len();
    let mut rest_start: Option<usize> = None;
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            rest_start = Some(consumed + line.len());
            break;
        }
        fm_lines.push(line);
        consumed += line.len();
        if fm_lines.len() > 512 {
            return (None, raw.to_string()); // 不像 front matter：保守放弃
        }
    }
    let Some(rest_start) = rest_start else {
        return (None, raw.to_string());
    };
    match parse_front_matter(&fm_lines.join("")) {
        Some(fm) => (Some(fm), raw[rest_start.min(raw.len())..].to_string()),
        None => (None, raw.to_string()),
    }
}

fn parse_front_matter(text: &str) -> Option<FrontMatter> {
    let value: toml::Value = toml::from_str(text).ok()?;
    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let order = value
        .get("order")
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)));
    Some(FrontMatter { title, order })
}

// ---------------------------------------------------------------------------
// probe → Outline
// ---------------------------------------------------------------------------

fn outline_items(dir: &DetectedDir, prefix: &str) -> Vec<OutlineItem> {
    let parent = if dir.name.is_empty() {
        None
    } else {
        Some(dir.name.as_str())
    };
    let mut items = Vec::new();
    if let Some(readme) = &dir.readme {
        items.push(OutlineItem {
            title: readme.title(parent),
            source: readme.source_key(),
            children: Vec::new(),
        });
    }
    for file in &dir.files {
        items.push(OutlineItem {
            title: file.title(parent),
            source: file.source_key(),
            children: Vec::new(),
        });
    }
    for sub in &dir.subdirs {
        let sub_prefix = if prefix.is_empty() {
            sub.name.clone()
        } else {
            format!("{prefix}/{}", sub.name)
        };
        let children = outline_items(sub, &sub_prefix);
        items.push(OutlineItem {
            title: sub.dir_title(),
            // 有 README 指向其章节；否则用目录路径做诊断定位键
            source: sub
                .readme
                .as_ref()
                .map(DetectedChapter::source_key)
                .unwrap_or_else(|| {
                    SourceKey::Path(
                        RelPath::new(&sub_prefix).unwrap_or(RelPath::new("_").expect("fallback")),
                    )
                }),
            children,
        });
    }
    items
}

// ---------------------------------------------------------------------------
// load：渲染 + TOC + 资产登记
// ---------------------------------------------------------------------------

/// 加载一层目录，返回本层的 toc 项。分组节点 = 子目录（有 README 时其
/// 章节上提为该分组节点的落点，规则 5）。
fn load_dir(
    vfs: &dyn Vfs,
    env: &LoadEnv<'_>,
    dir: &DetectedDir,
    done: &mut u64,
    total: u64,
    book: &mut BookDraft,
) -> Result<Vec<TocItem>, BuildError> {
    let parent = if dir.name.is_empty() {
        None
    } else {
        Some(dir.name.as_str())
    };
    let mut items = Vec::new();

    if let Some(readme) = &dir.readme {
        if let Some(id) = load_chapter(vfs, env, readme, parent, book, done, total)? {
            items.push(TocItem {
                chapter: Some(id),
                title: readme.title(parent),
                children: Vec::new(),
            });
        }
    }
    for file in &dir.files {
        if let Some(id) = load_chapter(vfs, env, file, parent, book, done, total)? {
            items.push(TocItem {
                chapter: Some(id),
                title: file.title(parent),
                children: Vec::new(),
            });
        }
    }
    for sub in &dir.subdirs {
        let sub_items = load_dir(vfs, env, sub, done, total, book)?;
        // README 上提：分组节点直接指向 README 章节，其余为其 children
        let (chapter, children) = if sub.readme.is_some() {
            let head = sub_items.first().and_then(|t| t.chapter);
            (head, sub_items.into_iter().skip(1).collect())
        } else {
            (None, sub_items)
        };
        items.push(TocItem {
            chapter,
            title: sub.dir_title(),
            children,
        });
    }
    Ok(items)
}

/// 读入并渲染一章；失败（读不了）→ warning + 跳过，返回 `None`。
fn load_chapter(
    vfs: &dyn Vfs,
    env: &LoadEnv<'_>,
    chapter: &DetectedChapter,
    parent: Option<&str>,
    book: &mut BookDraft,
    done: &mut u64,
    total: u64,
) -> Result<Option<ChapterId>, BuildError> {
    if env.progress.is_cancelled() {
        return Err(BuildError::Cancelled);
    }
    let raw = match vfs.read(&chapter.path) {
        Ok(bytes) => bytes,
        Err(e) => {
            book.warn(Warning::new(
                WarningCode::PageExtractFailed,
                Some(chapter.source_key()),
                format!("unreadable chapter: {e}"),
            ));
            *done += 1;
            env.progress.item(*done, total, chapter.path.as_str());
            return Ok(None);
        }
    };
    let (text, lossy) = match String::from_utf8(raw) {
        Ok(text) => (text, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
    };
    if lossy {
        book.warn(Warning::new(
            WarningCode::EncodingLossy,
            Some(chapter.source_key()),
            "chapter is not valid UTF-8; lossy conversion applied",
        ));
    }
    let text = text.trim_start_matches('\u{feff}').to_string();

    let (fm, body) = if env.config.markdown.front_matter {
        match split_front_matter(&text) {
            (Some(fm), body) => (Some(fm), body),
            (None, body) => (chapter.fm.clone(), body),
        }
    } else {
        (None, text)
    };
    let title = fm
        .as_ref()
        .and_then(|f| f.title.clone())
        .unwrap_or_else(|| chapter.title(parent));

    let html = render_markdown(&body, &env.config.markdown);
    let html = post_process(&html, &chapter.path, book, vfs);
    let id = book.push_chapter(title, chapter.source_key(), html);

    *done += 1;
    env.progress.item(*done, total, chapter.path.as_str());
    Ok(Some(id))
}

fn render_markdown(md: &str, cfg: &MarkdownCfg) -> String {
    let mut options = comrak::Options::default();
    options.extension.table = cfg.gfm;
    options.extension.strikethrough = cfg.gfm;
    options.extension.autolink = cfg.gfm;
    options.extension.tasklist = cfg.task_lists;
    options.extension.footnotes = cfg.footnotes;
    options.extension.alerts = cfg.admonitions;
    options.render.tasklist_classes = true;
    // GitHub alerts 渲染为 <aside class="admonition admonition-note">
    //（30-frontends.md 承诺的 admonition 形态，配色由 writer 的 CSS 负责）
    options.render.alert_style = comrak::options::AlertStyleType::Semantic;
    // raw HTML 透传（前端不做通用 HTML 处理；script 等由 dom 序列化层丢弃）
    options.render.r#unsafe = true;
    comrak::markdown_to_html(md, &options)
}

fn sel_headings() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("h1, h2, h3, h4, h5, h6"))
}

fn sel_media() -> &'static Selector {
    static S: OnceLock<Selector> = OnceLock::new();
    S.get_or_init(|| dom::selector("img[src], source[src]"))
}

/// 渲染后处理（前端义务 2/3）：heading 补稳定 id（comrak 的 GitHub 式
/// slug + 去重）；img/source 的 src 登记为资产。**不改写任何 href/src**。
fn post_process(html_str: &str, base: &RelPath, book: &mut BookDraft, vfs: &dyn Vfs) -> String {
    let mut html = Html::parse_fragment(html_str);
    let mut anchorizer = Anchorizer::new();
    for h_id in dom::select_ids(&html, sel_headings()) {
        if dom::get_attr(&html, h_id, "id").is_none() {
            let text = dom::text_content(&html, h_id);
            let id = anchorizer.anchorize(&text);
            dom::set_attr(&mut html, h_id, "id", &id);
        }
    }
    for media_id in dom::select_ids(&html, sel_media()) {
        if let Some(src) = dom::get_attr(&html, media_id, "src") {
            register_asset(base, &src, book, vfs);
        }
    }
    dom::serialize_fragment(&html)
}

/// 相对 src 以章节路径为基解析；http(s) → Remote；data: → 解码为 Owned。
fn register_asset(base: &RelPath, src: &str, book: &mut BookDraft, vfs: &dyn Vfs) {
    let key = if refs::is_external(src) {
        match src.parse::<url::Url>() {
            Ok(url) => SourceKey::Url(url),
            Err(_) => {
                book.warn(Warning::new(
                    WarningCode::LinkUnresolved,
                    Some(SourceKey::Path(base.clone())),
                    format!("invalid asset URL: {src}"),
                ));
                return;
            }
        }
    } else {
        match refs::resolve_rel(base, src) {
            Some(rel) => SourceKey::Path(rel),
            None => {
                book.warn(Warning::new(
                    WarningCode::LinkUnresolved,
                    Some(SourceKey::Path(base.clone())),
                    format!("unresolvable asset reference: {src}"),
                ));
                return;
            }
        }
    };
    if book.assets.contains_key(&key) {
        return;
    }
    let data = match &key {
        SourceKey::Path(rel) => match vfs.read(rel) {
            Ok(bytes) => AssetData::Owned(bytes),
            Err(_) => {
                book.warn(Warning::new(
                    WarningCode::AssetFetchFailed,
                    Some(key.clone()),
                    format!("local asset not found: {}", rel.as_str()),
                ));
                AssetData::Missing
            }
        },
        SourceKey::Url(url) if url.scheme() == "data" => match decode_data_uri(src) {
            Some(bytes) => AssetData::Owned(bytes),
            None => {
                book.warn(Warning::new(
                    WarningCode::AssetFetchFailed,
                    Some(key.clone()),
                    "invalid data: URI",
                ));
                AssetData::Missing
            }
        },
        SourceKey::Url(url) => AssetData::Remote(url.clone()),
    };
    book.assets.insert(key.clone(), Asset { source: key, data });
}

fn decode_data_uri(src: &str) -> Option<Vec<u8>> {
    let data_url = data_url::DataUrl::process(src).ok()?;
    let (bytes, _) = data_url.decode_to_vec().ok()?;
    Some(bytes)
}

// ---------------------------------------------------------------------------
// 书籍元数据
// ---------------------------------------------------------------------------

/// 书名：源内 front matter（单文件 FM / 根 README FM）> 配置兜底 > 单文件
/// 文件名 > "Untitled"（BookMetaCfg 是兜底值，front matter 优先于它）。
fn assemble_meta(
    book: &mut BookDraft,
    config: &ResolvedConfig,
    root: &DetectedDir,
    kind: &'static str,
    lead_fm: Option<FrontMatter>,
) {
    let fm_title = lead_fm.as_ref().and_then(|f| f.title.clone());
    let derived = if kind == "md-file" {
        root.readme
            .as_ref()
            .or_else(|| root.files.first())
            .map(|c| prettify(&c.display_stem))
            .filter(|t| !t.is_empty())
    } else {
        None
    };
    book.meta.title = fm_title
        .or_else(|| config.book.title.clone().filter(|t| !t.is_empty()))
        .or(derived)
        .unwrap_or_else(|| "Untitled".to_string());
    book.meta.authors = config.book.authors.clone();
    book.meta.language = config
        .book
        .language
        .clone()
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "en".to_string());
    book.meta.description = config.book.description.clone();
}

/// 封面（用户供图，需求定稿）：配置路径相对源根；URL/data: 也接受。
fn register_cover(vfs: &dyn Vfs, config: &ResolvedConfig, book: &mut BookDraft) {
    let Some(cover) = config.book.cover.as_ref().filter(|c| !c.is_empty()) else {
        return;
    };
    let key = if refs::is_external(cover) {
        match cover.parse::<url::Url>() {
            Ok(url) => SourceKey::Url(url),
            Err(_) => {
                book.warn(Warning::new(
                    WarningCode::AssetFetchFailed,
                    None,
                    format!("invalid cover URL: {cover}"),
                ));
                return;
            }
        }
    } else {
        match RelPath::new(cover) {
            Ok(rel) => SourceKey::Path(rel),
            Err(_) => {
                book.warn(Warning::new(
                    WarningCode::AssetFetchFailed,
                    None,
                    format!("invalid cover path: {cover}"),
                ));
                return;
            }
        }
    };
    let data = match &key {
        SourceKey::Path(rel) => match vfs.read(rel) {
            Ok(bytes) => AssetData::Owned(bytes),
            Err(_) => {
                book.warn(Warning::new(
                    WarningCode::AssetFetchFailed,
                    Some(key.clone()),
                    format!("cover not found: {}", rel.as_str()),
                ));
                return;
            }
        },
        SourceKey::Url(url) if url.scheme() == "data" => match decode_data_uri(cover) {
            Some(bytes) => AssetData::Owned(bytes),
            None => {
                book.warn(Warning::new(
                    WarningCode::AssetFetchFailed,
                    Some(key.clone()),
                    "invalid cover data: URI",
                ));
                return;
            }
        },
        SourceKey::Url(url) => AssetData::Remote(url.clone()),
    };
    book.assets.insert(
        key.clone(),
        Asset {
            source: key.clone(),
            data,
        },
    );
    book.meta.cover = Some(key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use trans_epub_contract::testing::MemVfs;

    fn detect_with(vfs: &MemVfs) -> Option<(&'static str, DetectedDir)> {
        let entries = vfs.walk().unwrap();
        detect(vfs, &entries, &StructureCfg::default())
    }

    #[test]
    fn flat_numeric_ordering() {
        let vfs = MemVfs::new()
            .insert("10-advanced.md", "# Advanced")
            .insert("01-intro.md", "# Intro")
            .insert("appendix.md", "# Appendix");
        let (kind, root) = detect_with(&vfs).unwrap();
        assert_eq!(kind, "md-flat");
        let titles: Vec<String> = root.files.iter().map(|f| f.title(None)).collect();
        assert_eq!(titles, vec!["intro", "advanced", "appendix"]);
    }

    #[test]
    fn readme_promoted_and_tree_kind() {
        let vfs = MemVfs::new()
            .insert("README.md", "# Home")
            .insert("guide/README.md", "# Guide")
            .insert("guide/02-b.md", "# B")
            .insert("guide/01-a.md", "# A")
            .insert("notes.md", "# Notes");
        let (kind, root) = detect_with(&vfs).unwrap();
        assert_eq!(kind, "md-tree");
        assert!(root.readme.is_some());
        assert_eq!(root.files.len(), 1); // notes.md
        let guide = &root.subdirs[0];
        assert_eq!(guide.name, "guide");
        assert!(guide.readme.is_some());
        let stems: Vec<&str> = guide.files.iter().map(|f| f.stem.as_str()).collect();
        assert_eq!(stems, vec!["01-a", "02-b"]);
    }

    #[test]
    fn front_matter_order_beats_prefix() {
        let vfs = MemVfs::new()
            .insert("10-z.md", "---\ntitle = \"Zero\"\norder = 1\n---\n\n# Zero")
            .insert("02-b.md", "# B");
        let (kind, root) = detect_with(&vfs).unwrap();
        assert_eq!(kind, "md-flat");
        assert_eq!(root.files[0].title(None), "Zero");
        assert!(!root.warnings.is_empty(), "歧义应记 warning");
    }

    #[test]
    fn yields_to_mdbook_projects() {
        let vfs = MemVfs::new()
            .insert("book.toml", "")
            .insert("src/a.md", "# A");
        let entries = vfs.walk().unwrap();
        assert!(yields_to_mdbook(&entries));
    }

    #[test]
    fn dotfiles_hidden() {
        let vfs = MemVfs::new()
            .insert(".hidden.md", "# X")
            .insert("a.md", "# A")
            .insert("sub/.draft.md", "# D")
            .insert("sub/b.md", "# B");
        let (kind, root) = detect_with(&vfs).unwrap();
        assert_eq!(kind, "md-tree");
        assert_eq!(root.files.len(), 1);
        assert_eq!(root.subdirs[0].files.len(), 1);
    }

    #[test]
    fn split_fm_and_numeric() {
        let (fm, body) = split_front_matter("---\ntitle = \"T\"\norder = 2.5\n---\nbody");
        let fm = fm.expect("valid TOML front matter");
        assert_eq!(fm.title.as_deref(), Some("T"));
        assert_eq!(fm.order, Some(2.5));
        assert_eq!(body, "body");
        // 非法 TOML（冒号语法）→ 保守当普通内容
        assert!(split_front_matter("---\ntitle: T\n---\nbody").0.is_none());
        assert!(split_front_matter("no fm here").0.is_none());
        assert_eq!(
            split_numeric_prefix("01-intro"),
            ("intro".to_string(), Some(1))
        );
        assert_eq!(split_numeric_prefix("42"), ("42".to_string(), None));
    }
}
