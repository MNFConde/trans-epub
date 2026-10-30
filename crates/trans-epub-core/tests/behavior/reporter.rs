//! 故障上报三接收点（10-contracts.md「故障上报：三接收点统一」）：实时
//! 镜像（ProgressSink::warning）与权威汇总（BuildReport.warnings）两路
//! 一致——同源、同序、同内容。故障面复用既有注入口径（MockFetcher 拉取
//! 失败 / 离线模式 / mdbook 指令失败），覆盖 load 期与 pass 期两条记录路径。

use std::sync::{Arc, Mutex};

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::error::Warning;
use trans_epub_contract::progress::ProgressSink;
use trans_epub_contract::source::Source;
use trans_epub_contract::testing::{MemVfs, MockFetcher};
use trans_epub_core::{Converter, Io};

/// 记录镜像事件的 sink（core 侧权威对账的"实时通道"替身）。
#[derive(Default)]
struct RecordingSink {
    seen: Mutex<Vec<Warning>>,
}

impl RecordingSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(Vec::new()),
        })
    }

    fn events(&self) -> Vec<Warning> {
        self.seen.lock().unwrap().clone()
    }
}

impl ProgressSink for RecordingSink {
    fn stage(&self, _name: &str) {}
    fn item(&self, _done: u64, _total: u64, _note: &str) {}
    fn warning(&self, w: &Warning) {
        self.seen.lock().unwrap().push(w.clone());
    }
}

/// 带记录 sink 的转换入口（无故障也照常出书——镜像零事件也是对账项）。
fn convert_with_sink(
    source: Source,
    config: &ResolvedConfig,
    fetcher: MockFetcher,
    sink: &Arc<RecordingSink>,
) -> trans_epub_contract::writer::BuildOutput {
    let io = Io {
        fetcher: Arc::new(fetcher),
        progress: sink.clone(),
    };
    Converter::new()
        .convert(&source, config, &io)
        .expect("conversion succeeds")
}

/// 两路对账：镜像事件序列 == report.warnings（内容、顺序全等）。
fn assert_two_paths_match(output: &trans_epub_contract::writer::BuildOutput, sink: &RecordingSink) {
    assert_eq!(
        sink.events(),
        output.report.warnings,
        "实时通道与 BuildReport 必须同源同序（三接收点契约）"
    );
}

#[test]
fn mirror_matches_report_when_pass_level_faults() {
    // pass 期记录路径：flat fixture 的 remote.png 不在 MockFetcher →
    // asset-resolve 记 AssetFetchFailed
    let sink = RecordingSink::new();
    let vfs = MemVfs::from_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/flat"))
        .expect("fixture dir readable");
    let output = convert_with_sink(
        Source::Vfs(Arc::new(vfs)),
        &ResolvedConfig::default(),
        MockFetcher::new(),
        &sink,
    );
    assert!(
        !output.report.warnings.is_empty(),
        "注入面必须至少产生一条 warning"
    );
    assert_two_paths_match(&output, &sink);
}

#[test]
fn mirror_matches_report_when_load_level_faults() {
    // load 期记录路径×2：SUMMARY 引用不存在文件 → PageExtractFailed
    //（load_chapter 读文件失败）；章内 {{#include}} 目标缺失 →
    // DirectiveUnresolved（ExpandCtx 局部批次在合并点入账）
    let sink = RecordingSink::new();
    let vfs = MemVfs::new()
        .insert(
            "src/SUMMARY.md",
            "# Summary\n\n- [Ch1](ch1.md)\n- [Bad](missing.md)\n",
        )
        .insert("src/ch1.md", "# Ch1\n\n{{#include missing.rs}}\n");
    let output = convert_with_sink(
        Source::Vfs(Arc::new(vfs)),
        &ResolvedConfig::default(),
        MockFetcher::new(),
        &sink,
    );
    let codes: Vec<_> = output
        .report
        .warnings
        .iter()
        .map(|w| w.code.as_str())
        .collect();
    assert!(
        codes.contains(&"page-extract-failed") && codes.contains(&"directive-unresolved"),
        "两条 load 期注入都须产生 warning：{codes:?}"
    );
    assert_two_paths_match(&output, &sink);
}

#[test]
fn mirror_matches_report_on_clean_book() {
    // 无故障书：两路都为空——镜像通道不凭空造事件
    let sink = RecordingSink::new();
    let vfs = MemVfs::new().insert("README.md", "# Clean\n\nplain text\n");
    let output = convert_with_sink(
        Source::Vfs(Arc::new(vfs)),
        &ResolvedConfig::default(),
        MockFetcher::new(),
        &sink,
    );
    assert!(output.report.warnings.is_empty());
    assert!(sink.events().is_empty());
}

#[test]
fn warning_channel_never_carries_fatal_track() {
    // 致命轨不走 warning 通道（三接收点口径）：无前端可识别 → Err 且镜像零事件
    let sink = RecordingSink::new();
    let vfs = MemVfs::new().insert("only.bin", b"\x00\x01".to_vec());
    let io = Io {
        fetcher: Arc::new(MockFetcher::new()),
        progress: sink.clone(),
    };
    let result =
        Converter::new().convert(&Source::Vfs(Arc::new(vfs)), &ResolvedConfig::default(), &io);
    assert!(result.is_err(), "致命轨必须走 Result::Err");
    assert!(sink.events().is_empty(), "致命故障不得借 warning 通道泄露");
}
