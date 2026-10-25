//! 三种来源前端，全部产出 [`crate::contract::ir::BookDraft`]。
//! 注册顺序即 probe 优先级：mdbook → md → web（ADR-0010）。mdbook / md
//! 已落地并进入 [`crate::Converter`] 默认注册序；web 于 MVP3 加入
//! （30-frontends.md）。

pub mod md;
pub mod mdbook;
pub mod web;
