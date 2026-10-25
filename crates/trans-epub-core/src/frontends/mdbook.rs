//! mdBook 前端（MVP2）：吃 `src/` 原始文件，自解析 SUMMARY.md，不拍平层级
//! （docs/architecture/30-frontends.md）。
//!
//! 识别（宽松）：`src/SUMMARY.md` 存在即接管，book.toml 缺省也认；
//! src 根默认 `"src"`，可被 book.toml 的 `[book].src` 覆盖；src 根下没有
//! SUMMARY 时回退根 `SUMMARY.md`。SUMMARY 是结构唯一真相源：链接文本做
//! 章名（优先于文件内 h1，h1 保留在正文）、嵌套列表 → toc 嵌套、heading
//! → part 分组（`chapter = None`）、`---` 分隔线忽略、无链接的叶子列表项
//! = draft（跳过 + warning；`skip_drafts = false` 时保留为分组节点）；
//! 无链接但带子列表的项 = 子分组标题（mdBook 语义，不是 draft）。
//! SUMMARY 未列出的文件（hidden 章）不进书，也不告警——mdBook 本就只构建
//! SUMMARY 所列内容。
//!
//! 指令展开（markdown 渲染前的文本级替换，`{{#include}}` 的产物落在作者
//! 已写好的 fenced code block 里）：
//!
//! | 指令 | 行为 |
//! |---|---|
//! | `{{#include path}}` | 全文内联 |
//! | `{{#include path:N}}` | 第 N 行到文件尾 |
//! | `{{#include path:N:M}}` | 第 N..=M 行（含端点，1-based） |
//! | `{{#include path:name}}` | `// ANCHOR: name` … `// ANCHOR_END: name` 之间（剥锚点行） |
//! | `{{#rustdoc_include path:struct:Foo}}` | item 定位（`{kw} {Name}` 行到闭合 `}` 行）内联，首行加 `// path:N-M` 行号锚点注释 |
//! | `{{#rustdoc_include path:lines:N-M}}` | 行区间同上 |
//! | `{{#playground path}}` | 折为代码块 + 指向原文件的相对链接（`playground_embed` 则纯内嵌） |
//! | `{{#hide}}` | 删除所在行 |
//! | 其他 | `DirectiveUnresolved` warning + 原文保留 |
//!
//! include 的 path 以 SUMMARY 所在目录（= src 根）为基经 RelPath 校验
//! （防穿越）；目标缺失 / 区间落空 → warning + 原文保留。渲染与资产登记
//! 复用 md 前端（comrak 方言一致，前端义务清单同一套）。

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::error::{BuildError, Warning, WarningCode};
use trans_epub_contract::frontend::{Frontend, LoadEnv, Outline, OutlineItem, Probe};
use trans_epub_contract::ir::{BookDraft, ChapterId, SourceKey, TocItem};
use trans_epub_contract::source::{RelPath, Source, Vfs};

use crate::frontends::md::{post_process, register_cover, render_markdown};
use crate::refs;

pub struct MdbookFrontend;

impl Frontend for MdbookFrontend {
    fn name(&self) -> &'static str {
        "mdbook"
    }

    fn probe(&self, source: &Source, config: &ResolvedConfig) -> Probe {
        let Source::Vfs(arc) = source else {
            return Probe::NoMatch;
        };
        let vfs: &dyn Vfs = arc.as_ref();
        let Some(root) = locate(vfs) else {
            return Probe::NoMatch;
        };
        let Ok(bytes) = vfs.read(&root.summary) else {
            return Probe::NoMatch;
        };
        let items = parse_summary(&String::from_utf8_lossy(&bytes));
        let mut warnings = root.warnings;
        let items = outline_items(
            &items,
            &root.summary,
            config.mdbook.skip_drafts,
            &mut warnings,
        );
        Probe::Matched {
            outline: Outline {
                kind: "mdbook",
                items,
                warnings,
            },
        }
    }

    fn load(&self, source: &Source, env: &LoadEnv<'_>) -> Result<BookDraft, BuildError> {
        let Source::Vfs(arc) = source else {
            return Err(BuildError::Source(
                "mdbook frontend requires a Vfs source".into(),
            ));
        };
        let vfs: &dyn Vfs = arc.as_ref();
        let root = locate(vfs).ok_or_else(|| BuildError::Source("not an mdBook project".into()))?;
        let bytes = vfs.read(&root.summary).map_err(|e| {
            BuildError::Source(format!("unreadable {}: {e}", root.summary.as_str()))
        })?;
        let items = parse_summary(&String::from_utf8_lossy(&bytes));

        let mut book = BookDraft::default();
        for w in root.warnings {
            book.warn(w);
        }
        let mut ctx = LoadCtx {
            vfs,
            env,
            base: &root.summary,
            done: 0,
            total: count_chapters(&items, &root.summary) as u64,
        };
        book.toc = load_items(&mut ctx, &items, &mut book)?;

        // 元数据优先级（10-contracts.md：源内元数据 > BookMetaCfg 兜底）：
        // book.toml [book] 的 title/authors/language/description 先于配置。
        let meta = &root.meta;
        book.meta.title = meta
            .title
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| env.config.book.title.clone().filter(|t| !t.is_empty()))
            .unwrap_or_else(|| "Untitled".to_string());
        book.meta.authors = if meta.authors.is_empty() {
            env.config.book.authors.clone()
        } else {
            meta.authors.clone()
        };
        book.meta.language = meta
            .language
            .clone()
            .filter(|l| !l.is_empty())
            .or_else(|| env.config.book.language.clone().filter(|l| !l.is_empty()))
            .unwrap_or_else(|| "en".to_string());
        book.meta.description = meta
            .description
            .clone()
            .or_else(|| env.config.book.description.clone());
        register_cover(vfs, env.config, &mut book);
        Ok(book)
    }
}

// ---------------------------------------------------------------------------
// 定位与 book.toml 收割
// ---------------------------------------------------------------------------

/// 一个命中的 mdBook 项目根。
struct MdbookRoot {
    /// SUMMARY.md 路径：SUMMARY 链接与 include 路径共同的解析基。
    summary: RelPath,
    meta: BookTomlMeta,
    warnings: Vec<Warning>,
}

/// book.toml `[book]` 表里可收割的元数据（全 Option/空 = 没得收）。
#[derive(Default)]
struct BookTomlMeta {
    title: Option<String>,
    authors: Vec<String>,
    language: Option<String>,
    description: Option<String>,
    src: Option<String>,
}

/// 定位 mdBook 项目：`{src}/SUMMARY.md`（src 默认 `"src"`、可被 book.toml
/// 覆盖）→ 回退根 `SUMMARY.md`；都没有 → `None`（book.toml 单独存在不算
/// ——mdBook 本体没有 SUMMARY 也构建不了）。
fn locate(vfs: &dyn Vfs) -> Option<MdbookRoot> {
    let mut warnings = Vec::new();
    let mut meta = BookTomlMeta::default();
    let mut src = "src".to_string();
    if let Ok(bytes) = vfs.read(&RelPath::new("book.toml").expect("constant is valid")) {
        match parse_book_toml(&String::from_utf8_lossy(&bytes)) {
            Ok(parsed) => {
                if let Some(configured) = &parsed.src {
                    src = configured.clone();
                }
                meta = parsed;
            }
            Err(_) => warnings.push(Warning::new(
                WarningCode::StructureAmbiguous,
                None,
                "book.toml is not valid TOML; its metadata is ignored",
            )),
        }
    }
    if let Ok(rel) = RelPath::new(&format!("{src}/SUMMARY.md")) {
        if vfs.exists(&rel) {
            return Some(MdbookRoot {
                summary: rel,
                meta,
                warnings,
            });
        }
    }
    let root_summary = RelPath::new("SUMMARY.md").expect("constant is valid");
    if vfs.exists(&root_summary) {
        return Some(MdbookRoot {
            summary: root_summary,
            meta,
            warnings,
        });
    }
    None
}

/// 宽松解析：只取 `[book]` 表的字段，多余结构一概忽略。
fn parse_book_toml(text: &str) -> Result<BookTomlMeta, ()> {
    let value: toml::Value = toml::from_str(text).map_err(|_| ())?;
    let book = value.get("book");
    let str_field = |key: &str| {
        book.and_then(|b| b.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let authors = book
        .and_then(|b| b.get("authors"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(BookTomlMeta {
        title: str_field("title"),
        authors,
        language: str_field("language"),
        description: str_field("description"),
        src: str_field("src"),
    })
}

// ---------------------------------------------------------------------------
// SUMMARY.md 解析（行级；mdBook 惯例语法的实用子集）
// ---------------------------------------------------------------------------

/// SUMMARY 节点种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    /// heading（`# Part …`）→ 分组节点，无 warning 语义。
    Part,
    /// 列表项 / 顶层裸链接。
    Entry,
}

#[derive(Debug, Clone)]
struct SummaryItem {
    kind: ItemKind,
    title: String,
    /// 相对 SUMMARY 所在目录的链接目标（锚点已剥、可能为空串）。
    link: Option<String>,
    children: Vec<SummaryItem>,
}

impl SummaryItem {
    /// 有效链接（存在且非空）→ 解析为 RelPath；None = draft / 分组。
    fn resolved_link(&self, base: &RelPath) -> Option<RelPath> {
        let link = self.link.as_deref().filter(|l| !l.is_empty())?;
        refs::resolve_rel(base, link)
    }

    fn is_draft(&self) -> bool {
        self.kind == ItemKind::Entry
            && self.link.as_deref().is_none_or(|l| l.is_empty())
            && self.children.is_empty()
    }
}

/// 解析事件流（扫描阶段产物）。
enum Ev {
    Heading(String),
    Entry {
        indent: usize,
        title: String,
        link: Option<String>,
        /// 顶层裸链接（prefix/suffix chapter）：结束当前段、自成一段。
        bare: bool,
    },
}

/// SUMMARY.md → 顶层节点序列。非结构行（说明文字等）忽略；链接目标里的
/// `#anchor` 切分不是 MVP2 语义，静默剥掉。
fn parse_summary(text: &str) -> Vec<SummaryItem> {
    let mut events = Vec::new();
    let mut seen_heading = false;
    for raw in text.lines() {
        let line = expand_leading_tabs(raw);
        let bare = line.trim_start();
        if bare.trim().is_empty() {
            continue;
        }
        let hashes = bare.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && bare[hashes..].starts_with(' ') {
            let title = bare[hashes..].trim().to_string();
            // 第一个 heading 文本为 "Summary" 是 mdBook 的惯例标题，忽略
            let is_summary_title = !seen_heading && title.eq_ignore_ascii_case("summary");
            if !is_summary_title {
                events.push(Ev::Heading(title));
            }
            seen_heading = true;
            continue;
        }
        if is_hr(bare) {
            continue;
        }
        let indent = line.len() - bare.len();
        if let Some(rest) = strip_list_marker(bare) {
            match parse_link_label(rest) {
                Some((title, link)) => events.push(Ev::Entry {
                    indent,
                    title,
                    link: Some(link),
                    bare: false,
                }),
                None => events.push(Ev::Entry {
                    indent,
                    title: rest.trim().to_string(),
                    link: None,
                    bare: false,
                }),
            }
            continue;
        }
        // 顶层裸链接（prefix/suffix chapter）；其余行忽略
        if let Some((title, link)) = parse_link_label(bare) {
            events.push(Ev::Entry {
                indent,
                title,
                link: Some(link),
                bare: true,
            });
        }
    }
    build_tree(events)
}

/// 行首 tab 按 4 空格展开（缩进可比性）。
fn expand_leading_tabs(line: &str) -> String {
    if !line.starts_with('\t') {
        return line.to_string();
    }
    let mut out = String::new();
    for c in line.chars() {
        if c == '\t' {
            out.push_str("    ");
        } else {
            out.push(c);
        }
    }
    out
}

/// `-` / `*` / `+` / `1.` 列表标记 → 余下内容。
fn strip_list_marker(bare: &str) -> Option<&str> {
    let rest = bare
        .strip_prefix("- ")
        .or_else(|| bare.strip_prefix("* "))
        .or_else(|| bare.strip_prefix("+ "));
    if let Some(rest) = rest {
        return Some(rest);
    }
    let digits = bare.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(rest) = bare[digits..].strip_prefix(". ") {
            return Some(rest);
        }
    }
    None
}

/// `---` / `***` / `___`（3 个及以上同字符）。
fn is_hr(bare: &str) -> bool {
    match bare.chars().next() {
        Some(first) => {
            matches!(first, '-' | '*' | '_')
                && bare.chars().count() >= 3
                && bare.chars().all(|c| c == first)
        }
        None => false,
    }
}

/// `[text](target)` 行首链接 → (文本, 目标)；target 里的锚点剥掉。
fn parse_link_label(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    let rest = s.strip_prefix('[')?;
    let close = rest.find("](")?;
    let title = &rest[..close];
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    let target = &after[..end];
    let target = target.split('#').next().unwrap_or(target);
    Some((title.trim().to_string(), target.to_string()))
}

/// 事件流 → 树：缩进栈挂接（entry 只在最深层存一份，弹层时并入父层
/// 最后项的 children）；heading 开新 part 段；顶层裸链接先结束当前段、
/// 自成一段（否则 part 后的 suffix 章会被卷进上一个 part）。
fn build_tree(events: Vec<Ev>) -> Vec<SummaryItem> {
    fn collapse(levels: &mut Vec<(usize, Vec<SummaryItem>)>) -> Vec<SummaryItem> {
        while levels.len() > 1 {
            let (_, items) = levels.pop().expect("len > 1");
            if let Some((_, top)) = levels.last_mut() {
                match top.last_mut() {
                    Some(last) => last.children.extend(items),
                    None => top.extend(items),
                }
            }
        }
        levels.pop().map(|(_, items)| items).unwrap_or_default()
    }

    /// 段落收尾：items 挂到未闭合的 part，或直接进顶层。
    fn attach(
        items: Vec<SummaryItem>,
        pending: &mut Option<SummaryItem>,
        roots: &mut Vec<SummaryItem>,
    ) {
        if let Some(mut part) = pending.take() {
            part.children = items;
            roots.push(part);
        } else {
            roots.extend(items);
        }
    }

    let mut roots: Vec<SummaryItem> = Vec::new();
    let mut levels: Vec<(usize, Vec<SummaryItem>)> = Vec::new();
    let mut pending_part: Option<SummaryItem> = None;

    for ev in events {
        match ev {
            Ev::Heading(title) => {
                let items = collapse(&mut levels);
                attach(items, &mut pending_part, &mut roots);
                pending_part = Some(SummaryItem {
                    kind: ItemKind::Part,
                    title,
                    link: None,
                    children: Vec::new(),
                });
            }
            Ev::Entry {
                indent,
                title,
                link,
                bare,
            } => {
                let item = SummaryItem {
                    kind: ItemKind::Entry,
                    title,
                    link,
                    children: Vec::new(),
                };
                if bare {
                    let items = collapse(&mut levels);
                    attach(items, &mut pending_part, &mut roots);
                    levels = vec![(indent, vec![item])];
                    continue;
                }
                while levels.len() > 1 && levels.last().expect("checked").0 > indent {
                    let (_, items) = levels.pop().expect("len > 1");
                    if let Some((_, top)) = levels.last_mut() {
                        match top.last_mut() {
                            Some(last) => last.children.extend(items),
                            None => top.extend(items),
                        }
                    }
                }
                match levels.last_mut() {
                    Some((top_indent, items)) if *top_indent == indent => items.push(item),
                    // 更深缩进开新层（弹层时并入父层最后项）；无层开第 0 层
                    _ => levels.push((indent, vec![item])),
                }
            }
        }
    }
    let items = collapse(&mut levels);
    attach(items, &mut pending_part, &mut roots);
    roots
}

/// 可加载章数（进度分母；draft / 分组不计）。
fn count_chapters(items: &[SummaryItem], base: &RelPath) -> usize {
    items
        .iter()
        .map(|it| {
            usize::from(it.resolved_link(base).is_some()) + count_chapters(&it.children, base)
        })
        .sum()
}

// ---------------------------------------------------------------------------
// probe → Outline
// ---------------------------------------------------------------------------

fn outline_items(
    items: &[SummaryItem],
    base: &RelPath,
    skip_drafts: bool,
    warnings: &mut Vec<Warning>,
) -> Vec<OutlineItem> {
    let mut out = Vec::new();
    for item in items {
        if let Some(rel) = item.resolved_link(base) {
            out.push(OutlineItem {
                title: item.title.clone(),
                source: SourceKey::Path(rel),
                children: outline_items(&item.children, base, skip_drafts, warnings),
            });
            continue;
        }
        if item.is_draft() {
            if skip_drafts {
                warnings.push(Warning::new(
                    WarningCode::StructureAmbiguous,
                    Some(SourceKey::Path(base.clone())),
                    format!("draft chapter skipped: {}", item.title),
                ));
                continue;
            }
            // skip_drafts = false：draft 无源可载，保留为分组节点
            out.push(OutlineItem {
                title: item.title.clone(),
                source: SourceKey::Path(base.clone()),
                children: Vec::new(),
            });
            continue;
        }
        // part / 子分组标题：chapter = None 的分组节点
        let children = outline_items(&item.children, base, skip_drafts, warnings);
        let source = children
            .first()
            .map(|c| c.source.clone())
            .unwrap_or_else(|| SourceKey::Path(base.clone()));
        out.push(OutlineItem {
            title: item.title.clone(),
            source,
            children,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// load：章节加载 + 指令展开
// ---------------------------------------------------------------------------

/// 章节加载的共享上下文（收敛参数个数：Vfs / 环境 / 指令展开基 / 进度）。
struct LoadCtx<'a> {
    vfs: &'a dyn Vfs,
    env: &'a LoadEnv<'a>,
    /// SUMMARY 路径：SUMMARY 链接与 include 路径共同的解析基。
    base: &'a RelPath,
    done: u64,
    total: u64,
}

fn load_items(
    ctx: &mut LoadCtx<'_>,
    items: &[SummaryItem],
    book: &mut BookDraft,
) -> Result<Vec<TocItem>, BuildError> {
    let mut out = Vec::new();
    for item in items {
        if let Some(rel) = item.resolved_link(ctx.base) {
            if !rel.file_name().to_lowercase().ends_with(".md") {
                book.warn(Warning::new(
                    WarningCode::StructureAmbiguous,
                    Some(SourceKey::Path(rel.clone())),
                    format!(
                        "summary link is not a markdown file, skipped: {}",
                        rel.as_str()
                    ),
                ));
                continue;
            }
            // 先父后子：ChapterId 按加载顺序递增 = spine 序（父章在前）
            let Some(id) = load_chapter(ctx, &rel, &item.title, book)? else {
                // 章节读失败被跳过：子项仍保留结构（挂分组节点）
                let children = load_items(ctx, &item.children, book)?;
                out.push(TocItem {
                    chapter: None,
                    title: item.title.clone(),
                    children,
                });
                continue;
            };
            let children = load_items(ctx, &item.children, book)?;
            out.push(TocItem {
                chapter: Some(id),
                title: item.title.clone(),
                children,
            });
            continue;
        }
        if item.is_draft() {
            if ctx.env.config.mdbook.skip_drafts {
                book.warn(Warning::new(
                    WarningCode::StructureAmbiguous,
                    Some(SourceKey::Path(ctx.base.clone())),
                    format!("draft chapter skipped: {}", item.title),
                ));
                continue;
            }
            out.push(TocItem {
                chapter: None,
                title: item.title.clone(),
                children: Vec::new(),
            });
            continue;
        }
        // part / 子分组 / 被跳过章的容器
        let children = load_items(ctx, &item.children, book)?;
        out.push(TocItem {
            chapter: None,
            title: item.title.clone(),
            children,
        });
    }
    Ok(out)
}

/// 读入一章：UTF-8 → 指令展开 → comrak 渲染 → 后处理（heading id / 资产）。
/// 读不了 → warning + 跳过（容忍轨）。mdBook 本体不支持章节 front matter，
/// 这里对齐（不做 FM 拆分）。
fn load_chapter(
    ctx: &mut LoadCtx<'_>,
    rel: &RelPath,
    title: &str,
    book: &mut BookDraft,
) -> Result<Option<ChapterId>, BuildError> {
    if ctx.env.progress.is_cancelled() {
        return Err(BuildError::Cancelled);
    }
    let key = SourceKey::Path(rel.clone());
    let bytes = match ctx.vfs.read(rel) {
        Ok(bytes) => bytes,
        Err(e) => {
            book.warn(Warning::new(
                WarningCode::PageExtractFailed,
                Some(key),
                format!("unreadable chapter: {e}"),
            ));
            ctx.done += 1;
            ctx.env.progress.item(ctx.done, ctx.total, rel.as_str());
            return Ok(None);
        }
    };
    let (text, lossy) = match String::from_utf8(bytes) {
        Ok(text) => (text, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
    };
    if lossy {
        book.warn(Warning::new(
            WarningCode::EncodingLossy,
            Some(key.clone()),
            "chapter is not valid UTF-8; lossy conversion applied",
        ));
    }
    let text = text.trim_start_matches('\u{feff}');

    let mut expand = ExpandCtx {
        vfs: ctx.vfs,
        base: ctx.base,
        chapter: key.clone(),
        chapter_path: rel,
        playground_embed: ctx.env.config.mdbook.playground_embed,
        warnings: Vec::new(),
    };
    let expanded = expand_directives(text, &mut expand);
    for w in expand.warnings {
        book.warn(w);
    }

    let html = render_markdown(&expanded, &ctx.env.config.markdown);
    let html = post_process(&html, rel, book, ctx.vfs);
    let id = book.push_chapter(title, key, html);
    ctx.done += 1;
    ctx.env.progress.item(ctx.done, ctx.total, rel.as_str());
    Ok(Some(id))
}

// ---------------------------------------------------------------------------
// 指令展开
// ---------------------------------------------------------------------------

/// 展开上下文：路径解析基 + warning 记账（先攒局部，回 load 统一入账）。
struct ExpandCtx<'a> {
    vfs: &'a dyn Vfs,
    /// include / playground 路径解析基（SUMMARY 所在目录 = src 根）。
    base: &'a RelPath,
    /// warning 定位键（当前章节）。
    chapter: SourceKey,
    /// playground 源文件链接的解析基（章节自身路径）。
    chapter_path: &'a RelPath,
    playground_embed: bool,
    warnings: Vec<Warning>,
}

impl ExpandCtx<'_> {
    fn warn(&mut self, message: String) {
        self.warnings.push(Warning::new(
            WarningCode::DirectiveUnresolved,
            Some(self.chapter.clone()),
            message,
        ));
    }
}

/// 文本级指令展开：逐行替换 `{{#…}}` 片段（指令独占一行是惯例，混排也
/// 支持——只替换片段保留前后文本）。`{{#hide}}` 删除所在行。展开不递归
/// （include 目标里的指令保留原文，与 mdBook 一致）。
fn expand_directives(text: &str, ctx: &mut ExpandCtx<'_>) -> String {
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some(expanded) = expand_line(line, ctx) {
            lines.push(expanded);
        }
    }
    lines.join("\n")
}

/// 单行展开；`None` = 删除该行（`{{#hide}}`）。
fn expand_line(line: &str, ctx: &mut ExpandCtx<'_>) -> Option<String> {
    let Some(start) = line.find("{{#") else {
        return Some(line.to_string());
    };
    let Some(rel_end) = line[start..].find("}}") else {
        return Some(line.to_string());
    };
    let end = start + rel_end + 2;
    let body = &line[start + 3..end - 2];
    let (name, arg) = match body.split_once(char::is_whitespace) {
        Some((n, a)) => (n, a.trim()),
        None => (body, ""),
    };
    let replacement = match name {
        "hide" => return None,
        "include" => expand_include(arg, false, ctx),
        "rustdoc_include" => expand_include(arg, true, ctx),
        "playground" => expand_playground(arg, ctx),
        _ => {
            ctx.warn(format!(
                "unknown directive {{{{#{name}}}}} kept as-is: {line}"
            ));
            return Some(line.to_string());
        }
    };
    let replacement = replacement.unwrap_or_else(|| line[start..end].to_string());
    Some(format!("{}{}{}", &line[..start], replacement, &line[end..]))
}

/// `{{#include}}` / `{{#rustdoc_include}}`：读目标文件按 spec 取区间。
/// 失败（路径解析不了 / 文件缺失 / 区间落空）→ warning + `None`
/// （调用方保留指令原文）。
fn expand_include(arg: &str, rustdoc: bool, ctx: &mut ExpandCtx<'_>) -> Option<String> {
    let (path_part, spec) = match arg.split_once(':') {
        Some((p, s)) => (p, Some(s)),
        None => (arg, None),
    };
    let Some(rel) = refs::resolve_rel(ctx.base, path_part) else {
        ctx.warn(format!(
            "include path unresolvable (traversal?), kept as-is: {arg}"
        ));
        return None;
    };
    let Ok(bytes) = ctx.vfs.read(&rel) else {
        ctx.warn(format!("include target not found, kept as-is: {arg}"));
        return None;
    };
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = resolve_range(spec, &lines) else {
        ctx.warn(format!(
            "range or anchor not found in {}, kept as-is: {arg}",
            rel.as_str()
        ));
        return None;
    };
    if start > end || start >= lines.len() {
        ctx.warn(format!(
            "empty range for include {arg} (file has {} lines)",
            lines.len()
        ));
        return None;
    }
    let end = end.min(lines.len() - 1);
    let selected = lines[start..=end].join("\n");
    if rustdoc {
        // 行号锚点注释：产物可追溯到源文件与行区间（rust 源为主，`//` 注释）
        Some(format!(
            "// {}:{}-{}\n{}",
            rel.as_str(),
            start + 1,
            end + 1,
            selected
        ))
    } else {
        Some(selected)
    }
}

/// 区间端点：1-based 行号或 ANCHOR 名（`ANCHOR: x` / `ANCHOR_END: x` 注释行）。
enum Endpoint<'a> {
    Line(usize),
    Anchor(&'a str),
}

fn parse_endpoint(s: &str) -> Endpoint<'_> {
    match s.parse::<usize>() {
        Ok(n) => Endpoint::Line(n),
        Err(_) => Endpoint::Anchor(s),
    }
}

/// spec → 0-based 含端点区间。`None` = 区间落空（warning 由调用方记）。
/// - `None` → 全文；
/// - `N` → 第 N 行到文件尾；`N:M` → N..=M（端点可为 ANCHOR 名）；
/// - `name` → `ANCHOR: name` 到 `ANCHOR_END: name`（剥锚点行）；
/// - rustdoc 专属：`lines:N-M`、`kw:Name`（item 定义行到闭合 `}` 行）。
fn resolve_range<'a>(spec: Option<&'a str>, lines: &[&'a str]) -> Option<(usize, usize)> {
    let Some(spec) = spec else {
        return Some((0, lines.len().saturating_sub(1)));
    };
    // rustdoc_include 的 item / lines 形态
    if let Some((prefix, rest)) = spec.split_once(':') {
        if prefix == "lines" {
            let (a, b) = rest.split_once('-')?;
            let (a, b) = (a.parse::<usize>().ok()?, b.parse::<usize>().ok()?);
            return Some((a.saturating_sub(1), b.saturating_sub(1)));
        }
        if !prefix.is_empty() && prefix.chars().all(|c| c.is_alphabetic()) && !rest.is_empty() {
            let pat = format!("{prefix} {rest}");
            let s = lines.iter().position(|l| l.contains(&pat))?;
            let e = lines[s..]
                .iter()
                .position(|l| l.trim_start().starts_with('}'))?
                + s;
            return Some((s, e));
        }
    }
    match spec.split_once(':') {
        Some((a, b)) => {
            let start = endpoint_line(parse_endpoint(a), lines, false)?;
            let end = endpoint_line(parse_endpoint(b), lines, true)?;
            Some((start.saturating_sub(1), end.saturating_sub(1)))
        }
        None => match parse_endpoint(spec) {
            Endpoint::Line(n) => Some((n.saturating_sub(1), lines.len().saturating_sub(1))),
            Endpoint::Anchor(name) => {
                let s = anchor_line(lines, name, "ANCHOR:")?;
                let e = anchor_line(lines, name, "ANCHOR_END:")?;
                // 两者均 1-based 注释行；内容 = 注释行之间的行（0-based：
                // start = s（注释行下一行），end = e - 2（注释行前一行的前一值））
                Some((s, e.saturating_sub(2)))
            }
        },
    }
}

/// 端点 → "伪 1-based" 行号（调用方 -1 转 0-based）：行号直接用；
/// 锚点名按 `ANCHOR:` 起点取注释行的下一行、`ANCHOR_END:` 终点取注释
/// 行的前一行（注释行本身不进产物）。
fn endpoint_line(ep: Endpoint<'_>, lines: &[&str], is_end: bool) -> Option<usize> {
    match ep {
        Endpoint::Line(n) => Some(n),
        Endpoint::Anchor(name) => {
            let marker = if is_end { "ANCHOR_END:" } else { "ANCHOR:" };
            let hit = anchor_line(lines, name, marker)?;
            if is_end {
                Some(hit.saturating_sub(1))
            } else {
                Some(hit + 1)
            }
        }
    }
}

/// 含 `ANCHOR: name` / `ANCHOR_END: name` 的行（1-based）。
fn anchor_line(lines: &[&str], name: &str, marker: &str) -> Option<usize> {
    lines
        .iter()
        .position(|l| {
            let compact = l.replace(' ', "");
            compact.contains(&format!("{marker}{name}"))
        })
        .map(|i| i + 1)
}

/// `{{#playground path}}` → 四反引号 fence（防内容自带 ``` 截断）+
/// 指向原文件的相对链接（link-rewrite 按统一语义处理）。
fn expand_playground(arg: &str, ctx: &mut ExpandCtx<'_>) -> Option<String> {
    let rel = refs::resolve_rel(ctx.base, arg)?;
    let bytes = ctx.vfs.read(&rel).ok()?;
    // trim 尾部换行：fence 的闭合由模板补，避免内容自带 \n 造成空行
    let content = String::from_utf8_lossy(&bytes)
        .trim_end_matches('\n')
        .to_string();
    let lang = match rel.file_name().rsplit_once('.') {
        Some((_, "rs")) => "rust",
        Some((_, "md")) => "markdown",
        Some((_, "toml")) => "toml",
        Some((_, "js")) | Some((_, "mjs")) => "javascript",
        Some((_, "py")) => "python",
        Some((_, "sh")) | Some((_, "bash")) => "bash",
        _ => "",
    };
    let mut out = format!("````{lang}\n{content}\n````");
    if !ctx.playground_embed {
        let href = relative_href(ctx.chapter_path, &rel);
        out.push_str(&format!("\n\n[playground source]({href})"));
    }
    Some(out)
}

/// 从章节所在目录指向 target 的相对 href（`../` 串 + 尾段）。
fn relative_href(chapter: &RelPath, target: &RelPath) -> String {
    let from: Vec<String> = chapter
        .parent()
        .map(|p| p.as_str().split('/').map(str::to_string).collect())
        .unwrap_or_default();
    let to: Vec<&str> = target.as_str().split('/').collect();
    let mut common = 0;
    while common < from.len() && common < to.len() && from[common] == to[common] {
        common += 1;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(to[common..].iter().map(|s| s.to_string()));
    if parts.is_empty() {
        target.file_name().to_string()
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trans_epub_contract::testing::MemVfs;

    fn summary_tree(text: &str) -> Vec<SummaryItem> {
        parse_summary(text)
    }

    #[test]
    fn parses_nesting_parts_and_drafts() {
        let text = "# Summary\n\
                    [Preface](preface.md)\n\
                    # Part One\n\
                    - [Ch1](part1/ch1.md)\n\
                    \x20\x20- [Sub](part1/ch1/sub.md)\n\
                    \x20\x20\x20\x20- [Deep](part1/ch1/sub/deep.md)\n\
                    - [Draft chapter]()\n\
                    ---\n\
                    # Part Two\n\
                    - [Group title]\n\
                    \x20\x20- [Ch2](ch2.md)\n\
                    [Appendix](appendix.md)\n";
        let roots = summary_tree(text);
        // 顶层：Preface、Part One、Part Two、Appendix
        assert_eq!(roots.len(), 4);
        assert_eq!(roots[0].title, "Preface");
        assert_eq!(
            roots[0]
                .resolved_link(&RelPath::new("src/SUMMARY.md").unwrap())
                .unwrap()
                .as_str(),
            "src/preface.md"
        );
        let part1 = &roots[1];
        assert_eq!(part1.kind, ItemKind::Part);
        assert_eq!(part1.children.len(), 2); // Ch1、Draft
        let ch1 = &part1.children[0];
        assert_eq!(ch1.children.len(), 1); // Sub
        assert_eq!(ch1.children[0].children.len(), 1); // Deep（3 层嵌套）
        assert!(part1.children[1].is_draft());
        // 纯文本 + 子列表 = 分组标题，不是 draft
        let part2 = &roots[2];
        let group = &part2.children[0];
        assert!(!group.is_draft());
        assert_eq!(group.children.len(), 1);
        assert_eq!(roots[3].title, "Appendix");
    }

    #[test]
    fn link_anchor_stripped_and_hr_ignored() {
        let text = "# Summary\n- [Ch](ch.md#section)\n***\n- [Next](next.md)\n";
        let roots = summary_tree(text);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].link.as_deref(), Some("ch.md"));
    }

    #[test]
    fn locate_variants() {
        let standard = MemVfs::new()
            .insert("book.toml", "[book]\ntitle = \"T\"\n")
            .insert("src/SUMMARY.md", "# Summary\n");
        let root = locate(&standard).unwrap();
        assert_eq!(root.summary.as_str(), "src/SUMMARY.md");
        assert_eq!(root.meta.title.as_deref(), Some("T"));

        let root_summary_only = MemVfs::new().insert("SUMMARY.md", "# Summary\n");
        let root = locate(&root_summary_only).unwrap();
        assert_eq!(root.summary.as_str(), "SUMMARY.md");

        let custom_src = MemVfs::new()
            .insert("book.toml", "[book]\nsrc = \"docs\"\n")
            .insert("docs/SUMMARY.md", "# Summary\n");
        let root = locate(&custom_src).unwrap();
        assert_eq!(root.summary.as_str(), "docs/SUMMARY.md");

        let not_mdbook = MemVfs::new().insert("book.toml", "[book]\n");
        assert!(locate(&not_mdbook).is_none());

        let bad_toml = MemVfs::new()
            .insert("book.toml", "not toml [[[\n")
            .insert("src/SUMMARY.md", "# Summary\n");
        let root = locate(&bad_toml).unwrap();
        assert_eq!(root.warnings.len(), 1, "坏 book.toml 应记 warning");
    }

    #[test]
    fn include_full_line_and_anchor_ranges() {
        let code = "l1\nl2\nl3\nl4\nl5\n";
        let anchored = "// ANCHOR: hello\nfn hello() {}\n// ANCHOR_END: hello\n";
        let vfs = MemVfs::new()
            .insert("src/SUMMARY.md", "# Summary\n")
            .insert("src/code.rs", code)
            .insert("src/anchored.rs", anchored);
        let base = RelPath::new("src/SUMMARY.md").unwrap();
        let mut ctx = test_ctx(&vfs, &base);
        assert_eq!(
            expand_include("code.rs", false, &mut ctx).unwrap(),
            code.trim_end()
        );
        assert_eq!(
            expand_include("code.rs:2:4", false, &mut ctx).unwrap(),
            "l2\nl3\nl4"
        );
        assert_eq!(
            expand_include("code.rs:4", false, &mut ctx).unwrap(),
            "l4\nl5"
        );
        // 锚点对：剥锚点注释行
        assert_eq!(
            expand_include("anchored.rs:hello", false, &mut ctx).unwrap(),
            "fn hello() {}"
        );
        assert!(ctx.warnings.is_empty());
    }

    #[test]
    fn include_missing_and_rustdoc_forms() {
        let vfs = MemVfs::new()
            .insert("src/SUMMARY.md", "# Summary\n")
            .insert(
                "src/item.rs",
                "use x;\nstruct Foo {\n    a: u32,\n}\nfn main() {}\n",
            );
        let base = RelPath::new("src/SUMMARY.md").unwrap();
        let mut ctx = test_ctx(&vfs, &base);
        // 缺失目标 → warning + None（原文保留由调用方负责）
        assert!(expand_include("missing.rs", false, &mut ctx).is_none());
        assert_eq!(ctx.warnings.len(), 1, "缺失目标应记 directive-unresolved");
        assert_eq!(ctx.warnings[0].code, WarningCode::DirectiveUnresolved);
        // rustdoc：struct:Foo → 定义行到闭合 }，含行号锚点注释
        let out = expand_include("item.rs:struct:Foo", true, &mut ctx).unwrap();
        assert!(out.starts_with("// src/item.rs:2-4\n"), "{out}");
        assert!(out.contains("struct Foo {"));
        assert!(out.contains("a: u32,"));
        // rustdoc：lines:N-M
        let out = expand_include("item.rs:lines:4-5", true, &mut ctx).unwrap();
        assert!(out.starts_with("// src/item.rs:4-5\n"));
        assert!(out.contains("fn main() {}"));
    }

    #[test]
    fn playground_and_hide_and_unknown() {
        let vfs = MemVfs::new()
            .insert("src/SUMMARY.md", "# Summary\n")
            .insert("src/ch/play.md", "x")
            .insert("src/pg.rs", "fn main() {}\n");
        let base = RelPath::new("src/SUMMARY.md").unwrap();
        let chapter = RelPath::new("src/ch/play.md").unwrap();
        let mut ctx = ExpandCtx {
            vfs: &vfs,
            base: &base,
            chapter: SourceKey::Path(chapter.clone()),
            chapter_path: &chapter,
            playground_embed: false,
            warnings: Vec::new(),
        };
        let out = expand_playground("pg.rs", &mut ctx).unwrap();
        assert!(out.starts_with("````rust\nfn main() {}\n````"), "{out}");
        assert!(out.ends_with("[playground source](../pg.rs)"), "{out}");
        ctx.playground_embed = true;
        let out = expand_playground("pg.rs", &mut ctx).unwrap();
        assert!(!out.contains("playground source"));

        // hide 删行；未知指令原文保留 + warning
        let text = "a\n{{#hide}} b\nc {{#if feat}} d\n";
        let out = expand_directives(text, &mut ctx);
        assert_eq!(out, "a\nc {{#if feat}} d");
        assert_eq!(ctx.warnings.len(), 1);
        assert_eq!(ctx.warnings[0].code, WarningCode::DirectiveUnresolved);
    }

    #[test]
    fn relative_href_walks_up() {
        let chapter = RelPath::new("src/deep/ch.md").unwrap();
        assert_eq!(
            relative_href(&chapter, &RelPath::new("src/pg.rs").unwrap()),
            "../pg.rs"
        );
        assert_eq!(
            relative_href(&chapter, &RelPath::new("src/deep/x.rs").unwrap()),
            "x.rs"
        );
        assert_eq!(
            relative_href(&chapter, &RelPath::new("root.rs").unwrap()),
            "../../root.rs"
        );
    }

    fn test_ctx<'a>(vfs: &'a MemVfs, base: &'a RelPath) -> ExpandCtx<'a> {
        ExpandCtx {
            vfs,
            base,
            chapter: SourceKey::Path(base.clone()),
            chapter_path: base,
            playground_embed: false,
            warnings: Vec::new(),
        }
    }
}
