//! trans-epub-core：md/文档站 → EPUB 转换内核（ADR-0004：同步纯库，
//! 所有 I/O 经契约注入）。
//!
//! 三种来源由 [`frontends`] 归一为 [`contract::ir`] 定义的有序章节列表
//! （[`contract::ir::BookDraft`]），经 [`pipeline`] 的 pass 链做公共处理
//! （asset-resolve → link-rewrite → highlight → code-wrap），最后由
//! [`writer`] 写出 EPUB3。[`Converter`] 是门面：装配（前端注册序 / pass 链 /
//! writer）+ probe/convert 编排，扩展点见 docs/architecture/20-core.md。
//!
//! [`contract`] 是 trans-epub-contract 的整体重导出：core 的公共 API
//! 面 = 契约 + Converter 门面。

pub use trans_epub_contract as contract;

pub mod converter;
pub mod dom;
pub mod frontends;
pub mod pipeline;
pub mod refs;
pub mod writer;

pub use converter::{Converter, Io};
