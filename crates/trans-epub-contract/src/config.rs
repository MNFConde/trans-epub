//! 配置模型：已合并的最终配置。
//!
//! 合并优先级（TOML < front matter < CLI flags）是壳层的责任，core
//! 不感知配置从哪来。原则：配置只描述"要什么"，不描述"怎么做"——
//! 不存在"跳过某个 pass"这类过程性开关，扩展行为走 Converter 的
//! 装配点。默认值以需求基线 v1.0 为准（docs/requirements.md）。

use serde::{Deserialize, Serialize};

/// 已合并的最终配置。缺省字段回落到各子配置的默认值。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ResolvedConfig {
    pub book: BookMetaCfg,
    pub structure: StructureCfg,
    pub markdown: MarkdownCfg,
    pub mdbook: MdbookCfg,
    pub web: WebCfg,
    pub assets: AssetsCfg,
    pub highlight: HighlightCfg,
    pub wrap: WrapCfg,
    pub theme: ThemeCfg,
}

/// 元数据兜底值（front matter 与源内元数据优先于这里）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BookMetaCfg {
    pub title: Option<String>,
    pub authors: Vec<String>,
    /// BCP-47；None 时按源推断，最终兜底 "en"。
    pub language: Option<String>,
    pub description: Option<String>,
    /// 封面图路径（Vfs 相对路径或 URL，由前端登记为资产）。
    pub cover: Option<String>,
}

/// 结构探测与声明覆盖（声明 > 探测）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StructureCfg {
    /// 显式章节声明：有序路径列表，存在时探测结果就是声明本身
    /// （v1 为平铺序，嵌套仍由目录树/源语义决定）。
    pub chapters: Vec<DeclaredChapter>,
    /// include/exclude glob（空 = 不限）。
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

/// 声明式章节。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeclaredChapter {
    /// 源相对路径。
    pub path: String,
    pub title: Option<String>,
}

/// markdown 方言开关（comrak；默认全开 = GFM 基线）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkdownCfg {
    pub gfm: bool,
    pub front_matter: bool,
    pub footnotes: bool,
    pub task_lists: bool,
    pub admonitions: bool,
}

impl Default for MarkdownCfg {
    fn default() -> Self {
        Self {
            gfm: true,
            front_matter: true,
            footnotes: true,
            task_lists: true,
            admonitions: true,
        }
    }
}

/// mdBook 指令展开选项。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MdbookCfg {
    /// draft / hidden 章节跳过（对齐 mdbook 默认）。
    pub skip_drafts: bool,
    /// `{{#playground}}` 内嵌完整代码而非"代码块 + 原文件链接"。
    pub playground_embed: bool,
}

impl Default for MdbookCfg {
    fn default() -> Self {
        Self {
            skip_drafts: true,
            playground_embed: false,
        }
    }
}

/// web 正文抽取。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WebCfg {
    /// 站点预设名：mkdocs / docusaurus / vitepress / gitbook。
    pub preset: Option<String>,
    /// 正文选择器覆盖（依次尝试；缺省链 main → article → .content
    /// → .markdown-body → body 兜底）。
    pub selectors: Vec<String>,
    /// 额外剔除的元素选择器（nav/footer 等已内置）。
    pub strip: Vec<String>,
}

/// 资产策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetsCfg {
    /// 是否拉取远程资产（false = 离线模式，远程引用降级 + warning）。
    pub remote: bool,
    /// 单文件大小上限（字节）。
    pub max_file_bytes: u64,
    /// 总大小上限（字节）。
    pub max_total_bytes: u64,
}

impl Default for AssetsCfg {
    fn default() -> Self {
        Self {
            remote: true,
            max_file_bytes: 10 * 1024 * 1024,
            max_total_bytes: 200 * 1024 * 1024,
        }
    }
}

/// 构建期高亮（ADR-0006）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HighlightCfg {
    /// syntect 亮色主题名。
    pub light_theme: String,
    /// syntect 暗色主题名。
    pub dark_theme: String,
    /// 未知语言回退为纯文本（false 则记 warning）。
    pub unknown_language_fallback: bool,
}

impl Default for HighlightCfg {
    fn default() -> Self {
        Self {
            light_theme: "InspiredGitHub".to_string(),
            dark_theme: "base16-ocean.dark".to_string(),
            unknown_language_fallback: true,
        }
    }
}

/// 代码长行策略（ADR-0007）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WrapMode {
    /// CSS pre-wrap 按屏宽自适应折行（默认，零内容改写）。
    PreWrap,
    /// 构建期硬折行（code-wrap pass，必须在 highlight 之后）。
    Hard { width: u32 },
}

/// 长行配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WrapCfg {
    pub mode: WrapMode,
}

impl Default for WrapCfg {
    fn default() -> Self {
        Self {
            mode: WrapMode::PreWrap,
        }
    }
}

/// 主题与 CSS 叠加。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeCfg {
    /// 用户自定义 CSS 内容（壳层已把文件读成字符串；追加在内置主题之后）。
    pub extra_css: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_baseline() {
        let cfg = ResolvedConfig::default();
        assert!(cfg.markdown.gfm && cfg.markdown.footnotes && cfg.markdown.admonitions);
        assert!(cfg.assets.remote);
        assert_eq!(cfg.assets.max_file_bytes, 10 * 1024 * 1024);
        assert_eq!(cfg.wrap.mode, WrapMode::PreWrap);
        assert_eq!(cfg.highlight.light_theme, "InspiredGitHub");
        assert!(cfg.mdbook.skip_drafts);
    }

    #[test]
    fn partial_config_falls_back_to_defaults() {
        // 壳层合并配置的典型形态：只写覆盖项，其余回落默认值
        let cfg: ResolvedConfig = toml::from_str(
            r#"
            [assets]
            remote = false

            [wrap]
            mode = { hard = { width = 80 } }
            "#,
        )
        .unwrap();
        assert!(!cfg.assets.remote);
        assert_eq!(cfg.assets.max_total_bytes, 200 * 1024 * 1024);
        assert_eq!(cfg.wrap.mode, WrapMode::Hard { width: 80 });
        assert!(cfg.markdown.gfm, "未覆盖的分组应回落默认值");
    }
}
