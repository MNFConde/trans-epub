//! trans-epub-contract：跨 crate 公共契约。
//!
//! 只放类型与 trait，无实现逻辑。归属判据：会被 core 之外的 crate
//! 实现或消费的进契约；只在 core 内部流转的——如 Pass 与 DOM——留在
//! trans-epub-core。依赖仅限 serde / url / thiserror 一级（ADR-0002）。
//!
//! - [`source`]：输入源两形态（[`source::Source::Vfs`] 文件树 /
//!   [`source::Source::Pages`] 页面列表）与 [`source::Vfs`]、
//!   [`source::Fetcher`] 注入口
//! - [`frontend`]：前端契约，probe（廉价探测）/ load（归一化）分离
//! - [`ir`]：枢轴中间表示 [`ir::BookDraft`]（有序章节 + 资产 + 嵌套目录）
//! - [`writer`]：写出器契约与产物 [`writer::BuildOutput`]
//! - [`error`]：错误双轨（[`error::BuildError`] 致命中止 /
//!   [`error::Warning`] 容忍累积）
//! - [`progress`]：进度与协作式取消 [`progress::ProgressSink`]
//! - [`config`]：已合并的最终配置 [`config::ResolvedConfig`]
//!
//! 设计文档：docs/architecture/10-contracts.md。契约变更须先改文档
//! 再动代码，破坏性变更同步 golden。

pub mod config;
pub mod error;
pub mod frontend;
pub mod ir;
pub mod progress;
pub mod source;
pub mod writer;

/// 测试工具（`test-util` feature）：契约 trait 的内存实现，供各 crate 的
/// 测试复用，生产代码禁止引用。
#[cfg(feature = "test-util")]
pub mod testing;
