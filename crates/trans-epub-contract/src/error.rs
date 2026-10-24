//! 错误双轨：致命错误中止整次转换；单项失败记 warning 继续，
//! 结尾随 BuildReport 汇总（服务需要机器可读的报告）。
//!
//! core 的输入处理路径禁止 panic；壳层（服务）可另加 `catch_unwind`
//! 保险，但那不是 core 的契约。

use serde::{Deserialize, Serialize};

use crate::ir::SourceKey;

/// 致命轨：中止整次转换。
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// 协作式取消触发（[`crate::progress::ProgressSink::is_cancelled`]）。
    #[error("cancelled")]
    Cancelled,
    /// 源不可读 / 无前端可识别。
    #[error("source error: {0}")]
    Source(String),
    /// 不可恢复错误。
    #[error("fatal [{code}]: {message}")]
    Fatal { code: &'static str, message: String },
}

/// 容忍轨：单项失败的结构化记录，最终进 BuildReport。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Warning {
    pub code: WarningCode,
    /// 定位到章/页/资产。
    pub source: Option<SourceKey>,
    pub message: String,
}

impl Warning {
    /// 便捷构造：带定位的告警。
    pub fn new(code: WarningCode, source: Option<SourceKey>, message: impl Into<String>) -> Self {
        Self {
            code,
            source,
            message: message.into(),
        }
    }
}

/// 告警类型码（机器可读；只增不改语义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningCode {
    AssetFetchFailed,
    AssetTooLarge,
    AssetUnsupported,
    LinkUnresolved,
    PageExtractFailed,
    DirectiveUnresolved,
    StructureAmbiguous,
    EncodingLossy,
    DuplicateSource,
    /// 代码块语言/主题名无法解析（取决于 unknown_language_fallback 决定降级方式）。
    HighlightSkipped,
}

impl WarningCode {
    /// kebab-case 名（与 serde 序列化一致），供人读日志使用。
    pub fn as_str(&self) -> &'static str {
        match self {
            WarningCode::AssetFetchFailed => "asset-fetch-failed",
            WarningCode::AssetTooLarge => "asset-too-large",
            WarningCode::AssetUnsupported => "asset-unsupported",
            WarningCode::LinkUnresolved => "link-unresolved",
            WarningCode::PageExtractFailed => "page-extract-failed",
            WarningCode::DirectiveUnresolved => "directive-unresolved",
            WarningCode::StructureAmbiguous => "structure-ambiguous",
            WarningCode::EncodingLossy => "encoding-lossy",
            WarningCode::DuplicateSource => "duplicate-source",
            WarningCode::HighlightSkipped => "highlight-skipped",
        }
    }
}

impl std::fmt::Display for WarningCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
