//! 内嵌字体行为断言（10-contracts.md「内嵌字体」节的三正交）：
//! off 不嵌任何东西、preset 嵌预子集 + OFL 义务页、user 按数据子集
//! 不出义务页、exact 下载失败回落 preset（warning 可追溯）。

use trans_epub_contract::config::{FontSource, FontSubsetMode, ResolvedConfig};
use trans_epub_contract::testing::MockFetcher;

use crate::support::{convert_config, convert_config_with_fetcher, utf8, zip_entries};

/// exact 回落类测试共享的串行锁：缓存 stash/restore 在并行测试间会
/// 交错（A stash 的文件被 B 的 restore 提前恢复 → A 误走缓存命中路径）。
static EXACT_CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 隔离字体缓存：本机 exact 端到端跑过的缓存会让下载被跳过、测试走
/// 成功路径而非回落路径。
fn stash_font_cache() {
    let cache = std::env::temp_dir().join("trans-epub-fonts");
    if let Ok(dir) = std::fs::read_dir(&cache) {
        for entry in dir.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "ttf") {
                let _ = std::fs::rename(&p, p.with_extension("stash"));
            }
        }
    }
}

fn restore_font_cache() {
    let cache = std::env::temp_dir().join("trans-epub-fonts");
    if let Ok(dir) = std::fs::read_dir(&cache) {
        for entry in dir.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "stash") {
                let _ = std::fs::rename(&p, p.with_extension("ttf"));
            }
        }
    }
}

fn font_config(embed: bool, subset: FontSubsetMode, source: FontSource) -> ResolvedConfig {
    let mut config = ResolvedConfig::default();
    config.fonts.embed = embed;
    config.fonts.subset = subset;
    config.fonts.source = source;
    config
}

fn embedded_font_bytes(output: &trans_epub_contract::writer::BuildOutput) -> Vec<u8> {
    zip_entries(output)
        .into_iter()
        .find(|(name, _, _)| name.starts_with("OEBPS/Fonts/"))
        .map(|(_, _, bytes)| bytes)
        .unwrap_or_default()
}

#[test]
fn fonts_off_by_default_embeds_nothing() {
    let output = convert_config("flat", ResolvedConfig::default());
    assert!(
        zip_entries(&output)
            .iter()
            .all(|(name, _, _)| !name.starts_with("OEBPS/Fonts/")),
        "默认不内嵌字体"
    );
    assert!(
        zip_entries(&output)
            .iter()
            .all(|(name, _, _)| !name.contains("font-license")),
        "默认无字体许可页"
    );
    let css = utf8("OEBPS/Styles/code.css", &output);
    assert!(!css.contains("@font-face"), "默认无 @font-face");
    assert!(
        css.contains("Maple Mono NF CN"),
        "off 档退回 CSS 具体名字体链：{css}"
    );
}

#[test]
fn fonts_builtin_preset_embeds_subset_and_license_page() {
    let output = convert_config(
        "flat",
        font_config(true, FontSubsetMode::Preset, FontSource::Builtin),
    );
    let font = embedded_font_bytes(&output);
    assert!(!font.is_empty(), "应内嵌预子集字体");
    // 字节与内置资产一致（原样落盘）
    assert_eq!(
        font,
        include_bytes!("../../assets/fonts/TransEpubMono-Regular.subset.ttf"),
        "preset 子集应原样内嵌"
    );
    let css = utf8("OEBPS/Styles/code.css", &output);
    assert!(css.contains("@font-face"), "@font-face 应存在：{css}");
    assert!(css.contains("\"TransEpub Mono\""), "链首为内嵌族名：{css}");
    assert!(css.contains("src: url(../Fonts/TransEpubMono-Regular.subset.ttf)"));
    // OFL 义务页：spine 末尾 + manifest + 全文
    let license = utf8("OEBPS/Text/font-license.xhtml", &output);
    assert!(license.contains("SIL Open Font License 1.1"));
    assert!(license.contains("Maple Mono"), "溯源上游");
    assert!(license.contains("TransEpub Mono"), "改名声明");
    let opf = utf8("OEBPS/content.opf", &output);
    assert!(opf.contains("application/font-sfnt"), "字体 manifest 登记");
    assert!(opf.contains("idref=\"font-license\""), "许可页进 spine");
}

#[test]
fn fonts_builtin_exact_falls_back_to_preset_on_download_failure() {
    let _guard = EXACT_CACHE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    stash_font_cache();

    // MockFetcher 不含钉版 URL → 下载失败 → 回落 preset + warning
    let output = convert_config(
        "flat",
        font_config(true, FontSubsetMode::Exact, FontSource::Builtin),
    );

    restore_font_cache();

    let font = embedded_font_bytes(&output);
    assert!(!font.is_empty(), "回落后仍应内嵌 preset 子集");
    assert_eq!(
        font.len(),
        include_bytes!("../../assets/fonts/TransEpubMono-Regular.subset.ttf").len()
    );
    let warned = output
        .report
        .warnings
        .iter()
        .any(|w| w.message.contains("font download failed"));
    assert!(
        warned,
        "下载失败须伴随 warning（回落本身由 preset 字节断言）：{:?}",
        output.report.warnings
    );
}

/// 故障注入：钉版 URL 下载"成功"但字节被换（半截 zip / 篡改 / 劫持）——
/// sha256 钉版必须拒收并回落 preset。与上一条互补：那条测传输失败，
/// 这条测**内容不对**——校验逻辑本身是安全边界。
#[test]
fn fonts_builtin_exact_rejects_tampered_zip_bytes() {
    let _guard = EXACT_CACHE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    stash_font_cache();

    const PINNED_URL: &str = "https://github.com/subframe7536/maple-font/releases/download/v7.9/MapleMono-CN-unhinted.zip";
    let tampered = MockFetcher::new().insert(
        PINNED_URL,
        "definitely not the pinned zip".as_bytes(),
        "application/zip",
    );
    let output = convert_config_with_fetcher(
        "flat",
        font_config(true, FontSubsetMode::Exact, FontSource::Builtin),
        tampered,
    );

    restore_font_cache();

    let font = embedded_font_bytes(&output);
    assert_eq!(
        font.len(),
        include_bytes!("../../assets/fonts/TransEpubMono-Regular.subset.ttf").len(),
        "拒收篡改字节后回落 preset"
    );
    let warned = output
        .report
        .warnings
        .iter()
        .any(|w| w.message.contains("sha256 mismatch"));
    assert!(
        warned,
        "校验失败须 warning 且信息含 mismatch：{:?}",
        output.report.warnings
    );
}

#[test]
fn fonts_user_source_subsets_without_license_page() {
    let full = include_bytes!("../../assets/fonts/TransEpubMono-Regular.subset.ttf");
    let output = convert_config(
        "flat",
        font_config(
            true,
            FontSubsetMode::Exact,
            FontSource::User {
                path: None,
                data: Some(full.to_vec()),
                family: Some("MyMono".to_string()),
            },
        ),
    );
    let font = embedded_font_bytes(&output);
    assert!(!font.is_empty());
    assert!(font.len() < full.len(), "exact 子集应小于源字体");
    let css = utf8("OEBPS/Styles/code.css", &output);
    assert!(css.contains("\"MyMono\""), "用户族名进链首：{css}");
    assert!(
        zip_entries(&output)
            .iter()
            .all(|(name, _, _)| !name.contains("font-license")),
        "用户字体不出 OFL 义务页"
    );
}

#[test]
fn fonts_user_source_without_data_warns_and_skips() {
    let output = convert_config(
        "flat",
        font_config(
            true,
            FontSubsetMode::Preset,
            FontSource::User {
                path: Some("x.ttf".into()),
                data: None,
                family: None,
            },
        ),
    );
    assert!(embedded_font_bytes(&output).is_empty());
    let warned = output
        .report
        .warnings
        .iter()
        .any(|w| w.message.contains("无 data"));
    assert!(warned, "缺数据须 warning：{:?}", output.report.warnings);
}
