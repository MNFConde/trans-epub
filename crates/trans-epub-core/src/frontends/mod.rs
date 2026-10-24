//! 三种来源前端，全部产出 [`crate::contract::ir::BookDraft`]。
//! 注册顺序即 probe 优先级：mdbook → md → web（ADR-0010）。
//! MVP1 仅实现 md；mdbook / web 分别在 MVP2 / MVP3 落地后加入
//! [`crate::Converter`] 的默认注册序（30-frontends.md）。

pub mod md;
pub mod mdbook;
pub mod web;
