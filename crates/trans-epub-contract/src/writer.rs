//! 输出侧：Writer 返回字节而非文件路径（core 不碰文件系统，ADR-0004）；
//! BuildReport 机器可读（code + 定位）。

use serde::{Deserialize, Serialize};

use crate::config::ResolvedConfig;
use crate::error::{BuildError, Warning};
use crate::ir::BookDraft;

/// 写出器契约。初期唯一实现是 trans-epub-core 的 epub3 writer。
pub trait Writer: Send + Sync {
    /// 输出格式标识（"epub3"；未来格式在此扩展，ADR-0008）。
    fn format(&self) -> &'static str;
    /// BookDraft 已经过完整 pass 链：writer 只做组装，不改内容。
    fn write(&self, book: &BookDraft, config: &ResolvedConfig) -> Result<Vec<u8>, BuildError>;
}

/// 最终产物（由 Converter 组装）：字节 + 报告。落盘/上传由壳层决定。
#[derive(Debug, Serialize, Deserialize)]
pub struct BuildOutput {
    pub format: String,
    pub bytes: Vec<u8>,
    pub report: BuildReport,
}

/// 转换报告：warning 汇总 + 统计。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildReport {
    pub warnings: Vec<Warning>,
    pub stats: BuildStats,
}

/// 转换统计。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BuildStats {
    pub chapters: u64,
    pub assets_embedded: u64,
    pub assets_missing: u64,
    pub links_rewritten: u64,
    pub links_unresolved: u64,
}
