//! asset-resolve 错误分支注入（todo 2026-10-28 发布前测试收尾）：远程
//! 拉取失败 / 超限 / 不支持的类型在 pass 层的降级行为——warning 可追溯、
//! 产物照常出书（Missing 资产不落盘，link-rewrite 降级引用）。

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::testing::{MemVfs, MockFetcher};

use crate::support::{convert_config_with_fetcher, zip_entries};

/// 远程图片引用（remote 模式走 MockFetcher 应答；flat fixture 的
/// remote.png 配空 fetcher 即注入拉取失败）。
#[allow(dead_code)]
fn md_with_remote(url: &str) -> String {
    format!("# T\n\n![pic]({url})\n")
}

fn warnings_of(output: &trans_epub_contract::writer::BuildOutput) -> Vec<String> {
    output
        .report
        .warnings
        .iter()
        .map(|w| w.message.clone())
        .collect()
}

#[test]
fn remote_404_falls_back_to_missing_with_warning() {
    // MockFetcher 未登记该 URL → fetch Err → AssetFetchFailed + Missing
    let config = ResolvedConfig::default();
    let output = convert_config_with_fetcher(
        "flat",
        config,
        MockFetcher::new(), // 不含 remote.png → 远程图必失败
    );
    let warned = warnings_of(&output);
    assert!(
        warned
            .iter()
            .any(|m| m.contains("fetch") && m.contains("failed")),
        "远程拉取失败须 warning：{warned:?}"
    );
    assert!(
        !zip_entries(&output)
            .iter()
            .any(|(name, _, _)| name.contains("remote.png")),
        "失败资产不落盘"
    );
}

#[test]
fn remote_declared_but_disabled_degrades_with_warning() {
    let mut config = ResolvedConfig::default();
    config.assets.remote = false;
    let output = convert_config_with_fetcher(
        "flat",
        config,
        MockFetcher::new().insert(
            "https://example.com/remote.png",
            b"png".to_vec(),
            "image/png",
        ),
    );
    let warned = warnings_of(&output);
    assert!(
        warned.iter().any(|m| m.contains("remote assets disabled")),
        "离线模式远程资产须 warning：{warned:?}"
    );
    assert!(
        !zip_entries(&output)
            .iter()
            .any(|(name, _, _)| name.contains("remote.png")),
        "离线模式远程资产不落盘"
    );
}

#[test]
fn oversized_local_asset_exceeds_per_file_limit() {
    // MemVfs 塞一个超 max_file_bytes 的"图片"（字节内容无所谓——上限
    // 检查在类型识别之前）
    let vfs = MemVfs::new()
        .insert("doc.md", "# T\n\n![big](big.png)\n")
        .insert("big.png", vec![0x89, b'P', b'N', b'G', 0x0D]); // 5 字节 PNG 头
    let mut config = ResolvedConfig::default();
    config.assets.max_file_bytes = 4;
    let output = crate::support::convert_vfs(vfs, config);
    let warned = warnings_of(&output);
    assert!(
        warned.iter().any(|m| m.contains("exceeds per-file limit")),
        "超单文件上限须 warning：{warned:?}"
    );
    assert!(
        !zip_entries(&output)
            .iter()
            .any(|(name, _, _)| name.contains("big.png")),
        "超限资产不落盘"
    );
}

#[test]
fn unsupported_content_is_rejected_with_warning() {
    // 有扩展名但内容既不在扩展名白名单语义内也嗅探不出（.binx 无效扩展
    // + 纯文本字节）→ AssetUnsupported
    let vfs = MemVfs::new()
        .insert("doc.md", "# T\n\n![weird](data.binx)\n")
        .insert("data.binx", b"not an image at all".to_vec());
    let output = crate::support::convert_vfs(vfs, ResolvedConfig::default());
    let warned = warnings_of(&output);
    assert!(
        warned.iter().any(|m| m.contains("unsupported asset type")),
        "不可识别类型须 warning：{warned:?}"
    );
    assert!(
        !zip_entries(&output)
            .iter()
            .any(|(name, _, _)| name.contains("data.binx")),
        "不支持资产不落盘"
    );
}

#[test]
fn malformed_data_uri_degrades_without_crash() {
    // data: URI 前端解码失败的形态由 md 前端决定；这里注入"声明为 png
    // 但 base64 非法"的 URI——全链不 panic、产物出书（图片缺失按需
    // warning 或纯文本降级，现状口径宽松断言）
    let vfs = MemVfs::new().insert(
        "doc.md",
        "# T\n\n![bad](data:image/png;base64,!!!not-base64!!!)\n",
    );
    let output = crate::support::convert_vfs(vfs, ResolvedConfig::default());
    assert_eq!(output.report.stats.chapters, 1, "书照常出");
}
