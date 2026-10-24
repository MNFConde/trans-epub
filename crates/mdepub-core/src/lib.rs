//! mdepub-core：md/文档站 → EPUB 转换核心。
//!
//! 三种来源（散装 markdown / mdBook / 静态网站）由 [`frontends`] 各自
//! 归一为 [`ir`] 定义的有序章节列表，经 [`pipeline`] 做链接改写等
//! 公共处理，最后由 [`writer`] 写出 EPUB3。

pub mod frontends;
pub mod ir;
pub mod pipeline;
pub mod writer;
