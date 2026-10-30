//! 公共管线：pass 链（asset-resolve → link-rewrite → highlight →
//! code-wrap → fonts），对三种来源的 HTML 统一处理——复用资产全在这层。
//! 顺序契约与 Pass trait 见 docs/architecture/20-core.md（Pass 与
//! DOM 类型留 core，不进契约 crate）。
//!
//! 各 pass 在 [`crate::dom::BookDom`] 上工作：入口解析一次、出口序列化
//! 一次，pass 内部禁止 parse/serialize。pass 边界与每章边界检查取消。

pub mod asset_resolve;
pub mod code_wrap;
pub mod fonts;
pub mod highlight;
pub mod link_rewrite;

pub use asset_resolve::AssetResolve;
pub use code_wrap::CodeWrap;
pub use fonts::Fonts;
pub use highlight::Highlight;
pub use link_rewrite::LinkRewrite;

use trans_epub_contract::config::ResolvedConfig;
use trans_epub_contract::error::BuildError;
use trans_epub_contract::progress::ProgressSink;
use trans_epub_contract::source::{Fetcher, Source};
use trans_epub_contract::writer::BuildStats;

use crate::dom::BookDom;
use crate::reporter::Reporter;

/// 一个处理步骤。`name` 同时用作 ProgressSink::stage 的阶段名。
///
/// 新 pass 必须在 docs/architecture/20-core.md 的顺序表里声明插入位置
/// （顺序是契约的一部分）。
pub trait Pass: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, book: &mut BookDom, ctx: &mut PassCtx<'_>) -> Result<(), BuildError>;
}

/// pass 的执行环境：配置 + 注入的 I/O + 统计累加器 + 故障上报单点。
///
/// `reporter` 的权威 vec 是执行器（[`crate::converter::Converter::convert`])
/// 持有的中转批次，每个 pass 跑完并入 `BookDom.warnings`——实时镜像在
/// [`Reporter::report`] 即刻发生，权威入账不晚于 pass 边界，顺序与
/// pass 执行序一致。
pub struct PassCtx<'a> {
    pub config: &'a ResolvedConfig,
    /// 原始输入（Vfs 形态可供 pass 读文件；Pages 形态仅作参考）。
    pub source: &'a Source,
    pub fetcher: &'a dyn Fetcher,
    pub progress: &'a dyn ProgressSink,
    pub stats: &'a mut BuildStats,
    /// 容忍轨上报单点：report = 实时镜像 + 批次入账（两路同源不分叉）。
    pub reporter: Reporter<'a>,
}

impl PassCtx<'_> {
    /// 取消检查点（pass 在每章边界调用）。
    pub fn check_cancel(&self) -> Result<(), BuildError> {
        if self.progress.is_cancelled() {
            Err(BuildError::Cancelled)
        } else {
            Ok(())
        }
    }
}
