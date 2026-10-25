//! 三种来源前端，全部产出 [`crate::contract::ir::BookDraft`]。
//! 注册顺序即 probe 优先级：mdbook → md → web（ADR-0010）。mdbook / md
//! 吃 `Source::Vfs`、web 只吃 `Source::Pages`（已抓取页面，顺序即书序，
//! 采集归 trans-epub-fetch）——三者在默认注册序里全量落地
//!（docs/architecture/30-frontends.md）。

pub mod md;
pub mod mdbook;
pub mod web;
