//! EPUB3 写出器与 CSS 主题（亮色基线 + prefers-color-scheme 暗色 + 用户叠加）。

pub mod epub3;
pub mod paths;

pub use epub3::Epub3Writer;
